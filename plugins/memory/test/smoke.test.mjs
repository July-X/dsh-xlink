import assert from 'node:assert/strict';
import test from 'node:test';

import { TOKEN_ENV, URL_ENV, MAX_PATH_BYTES, buildPath, callMemory, readCredentials } from '../host/bridge.js';
import {
  MAX_INJECTIONS_PER_SESSION,
  MAX_QUERY_CHARS,
  applyFailure,
  applyResult,
  resolveWorkspaceId,
  retarget,
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
