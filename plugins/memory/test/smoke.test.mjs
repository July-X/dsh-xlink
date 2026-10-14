import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

import { TOKEN_ENV, URL_ENV, MAX_PATH_BYTES, buildPath, callMemory, readCredentials } from '../host/bridge.js';
import {
  afterCompaction,
  MAX_INJECTIONS_PER_SESSION,
  MAX_LIBRARY_REFRESHES_PER_SESSION,
  MAX_QUERY_CHARS,
  MAX_TOTAL_BYTES_PER_SESSION,
  applyFailure,
  applyResult,
  applyStep,
  beginStep,
  libraryChanged,
  noteProbe,
  resolveWorkspaceId,
  retarget,
  shouldProbeLibrary,
  shouldRefresh,
  userText,
} from '../host/policy.js';

/**
 * 这一半没有外部依赖，所以能真的跑：编解码、错误分支与状态机都是纯函数。
 * 装配层（`host/index.js`）反过来只留「接线」，它的问题在那边的注释里写明——
 * 这里测不到的（agent.ctx 注册、pre-step 顺序）只能在真机启动时验。
 */

test('凭据两样缺一就等于没有桥', () => {
  assert.deepEqual(readCredentials({ [URL_ENV]: 'http://127.0.0.1:1', [TOKEN_ENV]: 't' }), {
    url: 'http://127.0.0.1:1',
    token: 't',
  });
  assert.equal(readCredentials({ [URL_ENV]: 'http://127.0.0.1:1' }), null);
  assert.equal(readCredentials({ [TOKEN_ENV]: 't' }), null);
  assert.equal(readCredentials({ [URL_ENV]: '', [TOKEN_ENV]: '' }), null);
  assert.equal(readCredentials({ [URL_ENV]: 1, [TOKEN_ENV]: 't' }), null);
  assert.equal(readCredentials({}), null);
});

test('路径拼装：中文与空格必须编码，空值不进查询串', () => {
  const path = buildPath('context', { q: '构建 命令', workspace: '/tmp/my repo', empty: '' });
  assert.match(path, /^\/v1\/memory\/context\?/);
  assert.ok(!path.includes('empty='));
  const query = new URLSearchParams(path.split('?')[1]);
  assert.equal(query.get('q'), '构建 命令');
  assert.equal(query.get('workspace'), '/tmp/my repo');
  // 原始路径里不该出现裸中文（桥那边按 URL 解析，裸字节会被截断）。
  assert.ok(!/[^\x00-\x7f]/.test(path), path);
  assert.equal(buildPath('propose', {}), '/v1/memory/propose');
});

test('没有凭据时不发请求，直接给出可读原因', async () => {
  let called = 0;
  const result = await callMemory(
    { credentials: null, route: 'context', params: {} },
    () => {
      called += 1;
      return Promise.resolve({ status: 200, body: '{}' });
    }
  );
  assert.equal(called, 0);
  assert.equal(result.ok, false);
  assert.match(result.reason, /记忆桥/);
});

test('成功路径把 JSON 交给调用方', async () => {
  const result = await callMemory(
    { credentials: { url: 'http://127.0.0.1:1', token: 't' }, route: 'context', params: { q: '中文' } },
    async ({ path, token }) => {
      assert.equal(token, 't');
      const query = new URLSearchParams(path.split('?')[1]);
      assert.equal(query.get('q'), '中文');
      return { status: 200, body: JSON.stringify({ enabled: true, text: '一条记忆', hits: [] }) };
    }
  );
  assert.equal(result.ok, true);
  assert.equal(result.value.text, '一条记忆');
});

test('业务错误原样透出，不抛异常', async () => {
  const unauthorized = await callMemory(
    { credentials: { url: 'http://127.0.0.1:1', token: 'stale' }, route: 'context', params: {} },
    async () => ({ status: 401, body: JSON.stringify({ error: 'unauthorized' }) })
  );
  assert.equal(unauthorized.ok, false);
  assert.equal(unauthorized.reason, 'unauthorized');

  const failed = await callMemory(
    { credentials: { url: 'http://127.0.0.1:1', token: 't' }, route: 'read', params: { id: 'x' } },
    async () => ({ status: 404, body: JSON.stringify({ error: '找不到记忆 x' }) })
  );
  assert.equal(failed.ok, false);
  assert.match(failed.reason, /找不到记忆/);

  const garbled = await callMemory(
    { credentials: { url: 'http://127.0.0.1:1', token: 't' }, route: 'context', params: {} },
    async () => ({ status: 200, body: '<html>' })
  );
  assert.equal(garbled.ok, false);
  assert.match(garbled.reason, /无法解析|HTTP 200/);
});

test('连不上桥与超时都收敛成 ok:false', async () => {
  const offline = await callMemory(
    { credentials: { url: 'http://127.0.0.1:1', token: 't' }, route: 'context', params: {} },
    () => Promise.reject(new Error('connect ECONNREFUSED'))
  );
  assert.equal(offline.ok, false);
  assert.match(offline.reason, /无法连接记忆桥/);

  const timeout = await callMemory(
    { credentials: { url: 'http://127.0.0.1:1', token: 't' }, route: 'context', params: {} },
    () => Promise.reject(new Error('桥在 1500ms 内没有响应'))
  );
  assert.equal(timeout.ok, false);
  assert.match(timeout.reason, /1500ms/);
});

test('查询文本只认用户消息：别的注入消息不当查询', () => {
  const messages = [
    { source: { kind: 'agent-instructions' }, content: [{ type: 'text', text: '工作区指令' }] },
    { source: { kind: 'user' }, content: [{ type: 'text', text: '  修一下构建  ' }] },
    { source: { kind: 'runtime-context' }, content: [{ type: 'text', text: '运行上下文' }] },
  ];
  assert.equal(userText(messages), '修一下构建');
  assert.equal(userText([]), '');
  assert.equal(userText(undefined), '');
  assert.equal(userText([{ source: { kind: 'user' }, content: [] }]), '');
});

/**
 * 长消息必须被截尾，而不是把请求行撑到 8 KB 之外。
 *
 * 桥的请求头缓冲是固定 8192 字节，查询串与请求头共用这一个额度；中文经
 * `URLSearchParams` 编码后 1 字符 → 9 字节。实测 1000 字的中文消息让请求行到
 * 9216 字节——越界之后桥**一个字节的响应都不写**就关连接，插件只看到
 * `socket hang up`，那一回合静默地没有记忆，而插件日志在这个内核里一行都不出。
 * 这条钉住的就是那条静默失效的前半段。
 */
test('超长的用户消息按尾部截断，拼出来的请求行仍在 8 KB 之内', () => {
  const long = '构建'.repeat(1000); // 2000 个中文字符
  const text = userText([{ source: { kind: 'user' }, content: [{ type: 'text', text: long }] }]);
  assert.equal(text.length, MAX_QUERY_CHARS);
  assert.ok(text.endsWith('构建'), '要保留最靠近当前要求的尾部');

  const path = buildPath('context', {
    q: text,
    workspace: '/Users/somebody/workdir/github/dsh-xlink',
    workspace_id: 'a-uuid-like-workspace-id',
  });
  // 8192 是壳那边 `read_request` 的缓冲：请求行 + Host + Authorization + 空行。
  const request = `GET ${path} HTTP/1.1\r\nHost: 127.0.0.1:54321\r\nauthorization: Bearer ${'a'.repeat(64)}\r\n\r\n`;
  assert.ok(
    Buffer.byteLength(request, 'utf8') <= 8192,
    `请求行 ${Buffer.byteLength(request, 'utf8')} 字节，超了桥的缓冲`
  );
});

/**
 * 截尾不能切出半个代理对。
 *
 * `slice` 按 UTF-16 码元切，emoji 占 2 个码元。切点落在中间时 `URLSearchParams`
 * **不会抛**（WHATWG 序列化器替换成 U+FFFD），症状是静默的：那一个字在检索时永远
 * 匹配不上。这条用「emoji 恰好横跨切点」的那个长度钉住——实测扫 41 个相邻长度里
 * 命中 1 个，也就是说常规测试很容易漏掉它。
 */
test('截尾落在代理对中间时砍掉落单的高代理', () => {
  // 501 个码元：切点落在第 0 个与第 1 个码元之间，正好把 emoji 劈开。
  const straddling = `😀${'b'.repeat(499)}`;
  assert.equal(straddling.length, 501);
  const text = userText([
    { source: { kind: 'user' }, content: [{ type: 'text', text: straddling }] },
  ]);
  assert.ok(text.length <= MAX_QUERY_CHARS);
  assert.ok(
    !/[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/.test(text),
    '不该留下落单的代理'
  );
  assert.ok(!text.includes('�'), '也不该把它换成替换字符');
  // 不落在中间时一个码元都不许少：全 ASCII、以及切点正好落在代理对**起点**的两种。
  assert.equal(
    userText([{ source: { kind: 'user' }, content: [{ type: 'text', text: 'x'.repeat(1200) }] }]).length,
    MAX_QUERY_CHARS
  );
  const emojiOnly = '😀'.repeat(400); // 800 个码元，切点落在第 300 个（某一对的起点）
  const kept = userText([
    { source: { kind: 'user' }, content: [{ type: 'text', text: emojiOnly }] },
  ]);
  assert.equal(kept.length, MAX_QUERY_CHARS, '切点在代理对起点时不该少任何码元');
  assert.ok(!kept.includes('�'));
  // 刚好一个代理对：两端都是**好的**那一半，一个码元都不许动。
  assert.equal(userText([{ source: { kind: 'user' }, content: [{ type: 'text', text: '😀' }] }]), '😀');
});

/**
 * 还是太长时要在**发出去之前**说清楚，而不是让连接被掐断。
 *
 * `memory_propose` 的正文上限是 8 KiB，而通道只吃得下大约九分之一的中文——不给一道
 * 提前拦，用户与模型只会看到「无法连接记忆桥」，会去查壳的进程，而不是知道该拆分。
 */
test('超过通道上限的内容在发出前就被拒，并说清是长度问题', async () => {
  let called = 0;
  const result = await callMemory(
    {
      credentials: { url: 'http://127.0.0.1:1', token: 't' },
      route: 'propose',
      params: { title: '约定', body: '提'.repeat(3000), scope: 'project', workspace: '/tmp/x' },
    },
    () => {
      called += 1;
      return Promise.resolve({ status: 200, body: '{}' });
    }
  );
  assert.equal(called, 0, '不能发出去——发出去只会换一个看不懂的 socket hang up');
  assert.equal(result.ok, false);
  assert.match(result.reason, /太长/);
  assert.match(result.reason, /拆成更短的几条/);
  // 刚好在上限内的照常发。
  const okPath = buildPath('propose', { body: 'a'.repeat(MAX_PATH_BYTES - 100) });
  assert.ok(Buffer.byteLength(okPath, 'utf8') <= MAX_PATH_BYTES);
});

test('每会话最多注入三次，同回合不重复检索', () => {
  let state = { turn: -1, text: '', injections: 0, workspace: '' };
  assert.equal(shouldRefresh(state, 1), true);
  state = applyResult(state, 1, '第一条');
  assert.equal(state.injections, 1);
  assert.equal(shouldRefresh(state, 1), false, '同一回合只检索一次');
  assert.equal(shouldRefresh(state, 2), true);

  state = applyResult(state, 2, '第二条');
  state = applyResult(state, 3, '第三条');
  assert.equal(state.injections, MAX_INJECTIONS_PER_SESSION);
  assert.equal(shouldRefresh(state, 4), false, '到顶后不再刷新');
});

/**
 * 换工作目录必须清空缓存，而**注入预算是会话级的、换目录不重置**。
 *
 * 这是「A 项目保存的记忆不进入 B 项目」在运行侧的最后一道：用户在内核里中途换目录
 * 是支持的。清缓存治好了泄漏，但若顺带把计数归零，就等于把「换目录」变成一个绕开
 * 预算的开关——来回切五次就是十五份快照（内核每份不同快照留一条 user 消息到压缩），
 * 而用户对这件事毫无感知。所以 `injections` 原样带过去。
 *
 * 代价要钉住：预算用完之后再换目录，这段会话**一段记忆都不注入**。宁可少注入，也
 * 不要把 A 的记忆送进 B；新开会话即恢复。
 */
test('压缩之后预算清零——否则长会话压缩后再也拿不到记忆', () => {
  const spent = {
    turn: 9,
    text: '旧正文',
    injections: MAX_INJECTIONS_PER_SESSION,
    bytes: 30000,
    workspace: '/repo-a',
    revision: 12,
  };
  assert.equal(shouldRefresh(spent, 10), false, '压缩之前：预算已满，不再刷新');

  const fresh = afterCompaction(spent);
  assert.equal(fresh.text, '', '旧正文丢掉（内核那些快照已经被压缩掉了）');
  assert.equal(fresh.injections, 0);
  assert.equal(fresh.bytes, 0);
  assert.equal(fresh.workspace, '/repo-a', '压缩不换目录：留着，免得被当成换目录多清一次');
  assert.equal(fresh.revision, 12, '文档代次与会话无关');
  assert.equal(shouldRefresh(fresh, 10), true, '压缩之后要能重新检索');

  // 与「换目录」那条区分开：换目录清正文但**不**归零预算，压缩是归零（内核那边快照真没了）。
  const moved = retarget(spent, '/repo-b');
  assert.equal(moved.injections, MAX_INJECTIONS_PER_SESSION);
  assert.equal(shouldRefresh(moved, 10), false);
});

test('切换工作目录清空缓存，但不重置会话级的注入预算', () => {
  let state = { turn: -1, text: '', injections: 0, workspace: '/repo-a' };
  // 三段**不同**的文本：内容相同不算一次注入（下面那条用例钉着），所以要用不同的。
  state = applyResult(state, 1, 'A 的记忆一');
  state = applyResult(state, 2, 'A 的记忆二');
  state = applyResult(state, 3, 'A 的记忆三');
  assert.equal(state.injections, MAX_INJECTIONS_PER_SESSION);
  assert.equal(shouldRefresh(state, 4), false, 'A 的预算已用满');

  // 换目录：A 的记忆不许跟着进 B。
  const moved = retarget(state, '/repo-b');
  assert.equal(moved.text, '', 'A 的记忆不许跟着进 B');
  assert.equal(moved.workspace, '/repo-b');
  assert.equal(moved.injections, MAX_INJECTIONS_PER_SESSION, '预算不该被换目录重置');
  assert.equal(shouldRefresh(moved, 4), false, '换目录之后这一段记忆是空的，不是 A 的');

  // 来回切再多次也拿不回额度——换目录不是绕开预算的开关。
  let flapped = moved;
  for (let round = 0; round < 5; round += 1) {
    flapped = retarget(flapped, round % 2 === 0 ? '/repo-a' : '/repo-b');
    assert.equal(shouldRefresh(flapped, 100 + round), false, `第 ${round} 次来回切之后额度不该回来`);
    assert.equal(flapped.text, '', '缓存必须一直是空的');
  }

  // 预算没用满时换目录仍然能重新检索（清缓存不等于什么都不给）。
  const early = applyResult(applyResult(retarget({ turn: -1, text: '', injections: 0, workspace: '' }, '/repo-x'), 1, 'x'), 2, 'y');
  assert.equal(shouldRefresh(early, 3), true, '还有额度时新目录要能检索到');
  // 目录没变就原样返回同一个对象：不重置、不凭空多消耗一次额度。
  assert.equal(retarget(early, '/repo-x'), early, '同一个目录不该重置');
  assert.equal(early.injections, 2);
  assert.equal(shouldRefresh(early, 2), false, '同一回合不重复检索');
  assert.equal(early.text, 'y', '同一个目录不该清掉缓存');
});

test('预算用满之后换目录：缓存也要立刻清空（接线，不只是纯函数）', () => {
  // 这条钉的是 `beginStep` 把 retarget 的结果**写回状态**这件事。写成
  // `{ state, refresh: shouldRefresh(retarget(state, ws), …) }` 时，预算用满的
  // 分支会把结果丢掉——上一版就是这么写的：`retarget` 自己的用例全绿，而真机上
  // A 项目的记忆在切到 B 之后还继续注入。
  let state = { turn: -1, text: '', injections: 0, workspace: '/repo-a' };
  state = applyResult(state, 1, 'A 的记忆一');
  state = applyResult(state, 2, 'A 的记忆二');
  state = applyResult(state, 3, 'A 的记忆三');
  assert.equal(state.injections, MAX_INJECTIONS_PER_SESSION);

  const spent = beginStep(state, '/repo-b', 4);
  assert.equal(spent.refresh, false, '预算已满，这一步不重新检索');
  assert.equal(spent.state.workspace, '/repo-b', '目录要立刻生效');
  assert.equal(spent.state.text, '', 'A 的记忆不许留在注入块里');
  assert.equal(spent.state.injections, MAX_INJECTIONS_PER_SESSION, '换目录不重置预算');

  // 还有额度时：换目录要能重新检索，且状态带着新目录。
  const early = beginStep({ turn: -1, text: 'x', injections: 0, workspace: '/repo-a' }, '/repo-b', 5);
  assert.equal(early.refresh, true);
  assert.equal(early.state.workspace, '/repo-b');
  assert.equal(early.state.text, '');

  // 目录没变：原样返回同一个对象，不重置也不清缓存（回合标记也照旧）。
  const applied = applyResult(early.state, 5, 'y');
  const again = beginStep(applied, '/repo-b', 5);
  assert.equal(again.state, applied, '同一个目录不该重建状态');
  assert.equal(again.state.text, 'y', '同一个目录不该清掉缓存');
  assert.equal(again.refresh, false, '同一回合不重复检索');
});

test('累计字节到顶之后不再刷新——次数封顶挡不住「每次都写满」', () => {
  // 单次 12 KiB × (3 + 2) 次最坏是 60 KiB，而设计稿写的是累计 ≤36 KiB。
  // 一条刚好过半的单条正文：两次就到顶（中文一字 3 字节，按字节算而不是码元）。
  const big = '关'.repeat(Math.ceil(MAX_TOTAL_BYTES_PER_SESSION / 2 / 3) + 1);
  assert.ok(Buffer.byteLength(big, 'utf8') <= MAX_TOTAL_BYTES_PER_SESSION);
  assert.ok(Buffer.byteLength(big, 'utf8') * 2 > MAX_TOTAL_BYTES_PER_SESSION, '两次就该到顶');

  let state = { turn: -1, text: '', injections: 0, bytes: 0, workspace: '/w', revision: null };
  state = applyResult(state, 1, big, 10);
  assert.equal(state.bytes, Buffer.byteLength(big, 'utf8'), '累计按 UTF-8 字节算，不是 UTF-16 码元');
  assert.equal(shouldRefresh(state, 2), true, '还没到顶');
  state = applyResult(state, 2, big + '尾巴', 10);
  assert.ok(state.bytes >= MAX_TOTAL_BYTES_PER_SESSION, '第二次到顶');
  assert.equal(state.injections, 2, '次数还没用完');
  assert.equal(shouldRefresh(state, 3), false, '字节到顶就不再刷新');
  assert.equal(shouldProbeLibrary(state, 3), false, '探测那条路同样受字节闸约束');

  // 内容没变的那一次不占额度（否则重复渲染会把额度吃光而什么都没注入）。
  const same = applyResult(state, 3, state.text, 10);
  assert.equal(same.bytes, state.bytes);
  assert.equal(same.injections, state.injections);

  // 换目录不重置累计字节（与会话级次数同一个理由）。
  assert.equal(retarget(state, '/other').bytes, state.bytes);
  // 失败清空正文也不退额度（额度按「内核里真的留下过几条快照」算）。
  assert.equal(applyFailure(state, 4).bytes, state.bytes);
});

test('装配层真的接上了这些判据（helper 有测试、接线没有是本仓反复栽的形状）', () => {
  const src = readFileSync(new URL('../host/index.js', import.meta.url), 'utf8');
  // 注释里也原样引用这些名字，先剥掉注释再判——否则判据会命中自己写的解释。
  //
  // **块注释也要剥**：这里踩过一次真坑——`interpolate: false` 那条判据原本只剥行注释，
  // 而文件头的 `/** … */` 里正好写着这句，于是把真代码改成 `interpolate: true` 之后
  // 判据**照样绿**（反向验才发现的）。剥完注释，判据才真的在判代码。
  const code = src.replace(/\/\*[\s\S]*?\*\//g, '').replace(/^\s*\/\/.*$/gm, '');
  for (const call of ['beginStep(', 'shouldProbeLibrary(', 'applyStep(']) {
    assert.ok(code.includes(call), `index.js 里少了 ${call} 的调用`);
  }
  assert.match(
    code,
    /applyStep\(\s*box\.state,\s*turn,\s*probe \? 'probe' : 'turn'/,
    '两条路要真的分开：预算内取到什么用什么，探测那条路只认代次'
  );
  // 压缩事件必须订阅在**插件根 ctx** 上：`session/event` 在会话 scope 上派发，而 `dsh-scope`
  // 的约定是「事件沿 scope 向上流、绝不向下」——挂在 `agent.ctx` 上一条都收不到，且没有任何
  // 症状（压缩之后只是再也不注入记忆）。这条判据盯的就是那个挂点。
  assert.match(code, /ctx\.on\('session\/event'/, '压缩事件要订阅在插件根 ctx 上');
  assert.ok(
    !code.includes("agent.ctx.on('session/event'"),
    '挂到 agent scope 上收不到（事件不向下流）'
  );
  assert.match(code, /event\?\.type !== 'compaction\/end'/, '只认「压缩完成」那一条');
  assert.match(code, /box\.state = afterCompaction\(box\.state\)/, '压缩后要真的把预算清零');
  assert.ok(
    !code.includes('applyResult('),
    '装配层不该自己调 applyResult——「结果怎么用」全在 applyStep 里'
  );
  // `next()` 的拒绝必须在 try **之外**：被 catch 吞掉就等于「别的插件拒绝或取消了这一步，
  // 我们照样改了缓存」——而缓存一变，注入块就跟着变（内核会为它多留一条 user 消息）。
  // 这条判据盯的是**顺序**（`await next()` 在 `try` 之前），不是文案。
  assert.match(
    code,
    /const decision = await next\(\);\s*\n\s*try \{/,
    'next() 要在 try 之外 await：拒绝要往外抛，不能被 catch 吞掉'
  );
  assert.match(code, /return decision;/, 'pre-step 要原样返回别的插件的决定');
  // 注入块**不做变量插值**：记忆正文里的 `${…}` / `{{…}}` 必须原样带出去。开了插值时
  // 内核会拿它去查变量表，正文会被悄悄改写——而「我看到的就是它说的」是这条链路的底线。
  assert.match(code, /interpolate: false/, '注入块要显式关掉插值');
  assert.match(code, /if \(applied\.applied\)/, '只在**真的用了**这次结果时才回报注入');
  assert.match(code, /route: 'note'/, '注入之后要回报给壳（面板的「最近注入」卡片靠它）');
});

test('预算之外只按数据代次刷新，额度用完就不再探', () => {
  let state = { turn: -1, text: '', injections: 0, workspace: '/w', revision: null };
  state = applyResult(state, 1, '一', 10);
  assert.equal(state.revision, 10, '正文要记下它对应的代次');
  state = applyResult(state, 2, '二', 10);
  state = applyResult(state, 3, '三', 10);
  assert.equal(state.injections, MAX_INJECTIONS_PER_SESSION);
  assert.equal(shouldProbeLibrary(state, 4), true, '预算用完之后要能探');
  assert.equal(shouldProbeLibrary(state, 3), false, '同一回合不重复探');

  const probed = noteProbe(state, 4, 10);
  assert.equal(probed.text, '三', '库没变就不许换正文');
  assert.equal(probed.injections, MAX_INJECTIONS_PER_SESSION, '探一次不算注入');
  assert.equal(probed.turn, 4);
  assert.equal(shouldProbeLibrary(probed, 4), false, '同一回合只探一次');

  assert.equal(libraryChanged(probed, 11), true, '代次变了 = 用户在会话里改了库');
  assert.equal(libraryChanged(probed, 10), false);
  assert.equal(libraryChanged(probed, undefined), false, '拿不到代次就不动');

  // 两次库变化额度用满之后不再探。
  const once = applyStep(probed, 5, 'probe', { ok: true, enabled: true, text: '新一', generation: 11 }).state;
  const twice = applyStep(once, 6, 'probe', { ok: true, enabled: true, text: '新二', generation: 12 }).state;
  assert.equal(twice.injections, MAX_INJECTIONS_PER_SESSION + MAX_LIBRARY_REFRESHES_PER_SESSION);
  assert.equal(shouldProbeLibrary(twice, 7), false, '额度用满之后不再探');

  // 换目录不丢代次（代次是整份文档的，与目录无关）；响应缺代次时保留上一次的。
  assert.equal(retarget(state, '/other').revision, 10);
  assert.equal(applyResult(state, 4, 'x').revision, 10);
});

test('结果怎么用：探测那条路只在库变了时才换正文', () => {
  const base = { turn: 3, text: 'A 的记忆', injections: 3, workspace: '/w', revision: 10 };

  // 同一个代次 + 完全不同的正文（这就是「用户换了下一句话」的常态）：不许换。
  const unchanged = applyStep(base, 4, 'probe', {
    ok: true,
    enabled: true,
    text: '另一条消息检索出来的完全不同的正文',
    generation: 10,
  });
  assert.equal(unchanged.applied, false);
  assert.equal(unchanged.state.text, 'A 的记忆', '文本变了不算库变化');
  assert.equal(unchanged.state.injections, 3);

  // 代次变了：换，并记一次注入（额度从这里扣）。
  const changed = applyStep(base, 4, 'probe', {
    ok: true,
    enabled: true,
    text: 'A 的记忆 + 刚批准的那条',
    generation: 11,
  });
  assert.equal(changed.applied, true);
  assert.equal(changed.state.text, 'A 的记忆 + 刚批准的那条');
  assert.equal(changed.state.injections, 4);

  // 探测失败：正文留着——一次抖动不是「用户改了库」。
  const failedProbe = applyStep(base, 4, 'probe', { ok: false });
  assert.equal(failedProbe.applied, false);
  assert.equal(failedProbe.state.text, 'A 的记忆');
  assert.equal(failedProbe.state.turn, 4, '失败也要标记回合，别在同一步里反复探');

  // 回合内失败：照旧清空（这一回合没有记忆要如实反映）。
  assert.equal(applyStep(base, 4, 'turn', { ok: false }).state.text, '');

  // 会话中途把开关关掉：代次变了、enabled=false → 正文清空。
  const off = applyStep(base, 5, 'probe', { ok: true, enabled: false, generation: 12 });
  assert.equal(off.state.text, '', '关掉开关之后模型不该再看到正文');
});

test('文本没变不算一次注入；失败清空缓存而不是沿用', () => {
  let state = applyResult({ turn: -1, text: '', injections: 0, workspace: '/w' }, 1, '同一条');
  assert.equal(state.injections, 1);
  state = applyResult(state, 2, '同一条');
  assert.equal(state.injections, 1, '内容相同不会新增内核快照，也不该消耗配额');

  // 停用/删除/桥挂掉之后：本回合看到的是空，而不是上一次的内容。
  const failed = applyFailure({ turn: 2, text: '旧记忆', injections: 1, workspace: '/w' }, 3);
  assert.equal(failed.text, '');
  assert.equal(failed.turn, 3);
  assert.equal(failed.workspace, '/w', '失败不该把目录也丢了，否则下一回合会当成换目录');
});

test('工作区 id 解析：拿到就带上，拿不到一律降级成空串', async () => {
  const registry = { resolveByPath: async (path) => (path === '/tmp/known' ? { id: 'ws-1' } : undefined) };
  assert.equal(await resolveWorkspaceId(registry, '/tmp/known'), 'ws-1');
  // 目录不存在时 resolveByPath 会 reject（它内部做 realpath）——不能因此让检索失败。
  assert.equal(
    await resolveWorkspaceId({ resolveByPath: async () => { throw new Error('ENOENT'); } }, '/tmp/gone'),
    ''
  );
  // 服务还没就绪（按需注入的回调还没跑）：退回路径匹配。
  assert.equal(await resolveWorkspaceId(null, '/tmp/known'), '');
  assert.equal(await resolveWorkspaceId({}, '/tmp/known'), '');
  assert.equal(await resolveWorkspaceId(registry, ''), '');
});
