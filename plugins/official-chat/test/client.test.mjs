// G1 客户端半边的**行为**判据（真内核实跑之外的第二道网）。
//
// ## 为什么需要这条
//
// 真机实跑（`docs/features/official-chat/` 的 G1 验证）贵：起隔离内核 +
// 浏览器驱动，一轮几分钟，而且**只能证明那一轮**。这里把「插件自己承诺的
// 行为」钉成可重复跑的断言：同步 onPick、serialize 等的是同一条读取、
// late 模式不插隐藏占位符、CAS 被拒时不假装成功。
//
// 这些断言**不能替代**真机实跑：它们跑在一个假的 ctx 上，证不了内核的
// 发送路径会不会真的 await 我们的 serialize。真机结论在群里的交付里另列。
//
// ## 覆盖的范围（PM 2026-10-20 拍板：只留甲案一条路）
//
// 同步 onPick、serialize 等的是同一条读取、读取未完成时 serialize 不 resolve、
// 读取失败即 reject、pending 句柄不得被渲染成就绪、openReference 能区分
// pending 与 ready，以及**插件不自己插 chip**（不绕开内核的输入事件）。
//
// ## 怎么加载
//
// 客户端束是浏览器脚本（`window.__ModuleLoader__.load({id, factory})`），
// 这里用 `node:vm` 造一个最小的 `window` 与 `__ModuleLoader__`，
// 把 factory 取出来直接调——与内核加载的是同一份源码。

import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { createContext, runInContext } from 'node:vm';

const CLIENT_SOURCE = readFileSync(new URL('../client/client.js', import.meta.url), 'utf8');

/**
 * 载入客户端束并造一个假的内核 ctx。
 *
 * @param {object} [options]
 * @param {boolean} [options.withInputTriggers] - 是否提供 inputTriggers 服务。
 * @param {boolean} [options.withSessions] - 是否提供 sessions 服务。
 * @param {Function} [options.bail] - `actx.bail` 的实现（返回 true 表示编辑器接下了）。
 * @param {object} [options.config] - `window.__DSH_G1_CONFIG__`。
 */
function loadClient({ withInputTriggers = true, withSessions = true, bail, config = {} } = {}) {
  const registered = [];
  const effects = [];
  const bailCalls = [];
  const notices = [];
  let module = null;

  const sandbox = {
    console: { info: () => {}, warn: () => {}, error: () => {}, log: () => {} },
    setTimeout,
    clearTimeout,
    Promise,
    JSON,
    Object,
    Array,
    Error,
    String,
    Number,
    Boolean,
    __DSH_G1_CONFIG__: config,
  };
  sandbox.window = sandbox;
  sandbox.globalThis = sandbox;
  sandbox.__ModuleLoader__ = { load: (entry) => { module = entry; } };
  sandbox.window.__ModuleLoader__ = sandbox.__ModuleLoader__;

  createContext(sandbox);
  runInContext(CLIENT_SOURCE, sandbox, { filename: 'client/client.js' });

  const actx = {
    bail: (self, event, payload) => {
      bailCalls.push({ event, payload });
      return bail ? bail(event, payload) : true;
    },
    get: (name) => (name === 'conversation'
      ? { input: { for: () => ({ notify: (level, text) => notices.push({ level, text }) }) } }
      : undefined),
  };

  const services = {
    inputTriggers: withInputTriggers
      ? { registerSource: (source) => registered.push(source) }
      : undefined,
    sessions: withSessions ? { scope: () => actx } : undefined,
  };

  const ctx = {
    get: (name) => services[name],
    // cordis 的 effect 是**立即执行**的（挂载时跑一次，卸载时回收），
    // 这里照抄：只记录不执行的话，来源根本不会注册，测试会假红。
    effect: (fn, label) => { effects.push({ fn, label }); fn(); },
  };

  const api = module.factory();
  return { api, ctx, registered, effects, bailCalls, notices, sandbox };
}

/** 造一个候选行（与内核菜单里的行同构）。 */
function candidate(id, title) {
  return { name: title, description: '固定夹具 · G1 验证用', icon: 'session', section: '官方对话', value: JSON.stringify({ kind: 'official-chat', id, title }) };
}

const ALPHA = 'G1 固定对话 Alpha';
const BODY_ALPHA = '【G1 固定正文 Alpha】第一行固定正文。\n第二行固定正文。';

test('束是自包含单文件：没有顶层 const/let/class（client-hmr 重求值会直接 SyntaxError）', () => {
  // 顶层声明在页面的全局词法作用域里，HMR 第二次求值就是
  // "Identifier has already been declared"，整条 entry 当场死掉且**静默**。
  const topLevel = CLIENT_SOURCE
    .replace(/\/\*[\s\S]*?\*\//g, '')
    .replace(/^\s*\/\/.*$/gm, '')
    .split('\n')
    .filter((line) => /^(const|let|class|var)\s/.test(line));
  assert.deepEqual(topLevel, [], `顶层声明会踩 HMR：${topLevel.join(' | ')}`);
  assert.ok(!/require\s*\(/.test(CLIENT_SOURCE), '客户端束不许 require 别的文件（浏览器里会 404）');
});

test('apply 注册了 @ 来源，且 inject 声明的是服务名', () => {
  const { api, ctx, registered, effects } = loadClient();
  api.apply(ctx);
  assert.equal(registered.length, 1, '应当注册一个来源');
  assert.equal(registered[0].trigger, '@');
  assert.equal(registered[0].name, 'official-chat');
  // 数组来自 vm 的另一个 realm：先摊成宿主数组再比，否则 deepStrictEqual
  // 会因为原型不同而判不相等（值一模一样）。
  assert.deepEqual([...api.inject], ['inputTriggers', 'sessions'], 'inject 装的是服务名，不是包名');
  assert.equal(effects.length, 1, '注册必须挂在 effect 上，卸载时才收得掉');
});

test('服务缺失时不注册来源（不假装成功）', () => {
  const { api, ctx, registered, sandbox } = loadClient({ withSessions: false });
  api.apply(ctx);
  assert.equal(registered.length, 0, '缺 sessions 就不该注册');
  const probe = sandbox.__DSH_XLINK_OFFICIAL_CHAT_PROBE__;
  assert.ok(probe, '探针应当还在（自报，不作判据）');
  assert.ok(
    probe.events.some((e) => e.kind === 'apply-aborted'),
    '缺服务时探针要留下 apply-aborted',
  );
});

test('onPick 同步返回引用句柄（pending 态）——内核的 execute 只认普通对象', () => {
  const { api, ctx, registered } = loadClient();
  api.apply(ctx);
  const source = registered[0];
  const outcome = source.onPick({ candidate: candidate('g1-alpha', ALPHA), session: { sessionId: 's1' }, span: { start: 0, end: 1, draftRev: 7 } });
  assert.ok(outcome && typeof outcome === 'object', 'onPick 必须同步返回对象，不能返回 Promise');
  assert.ok(!('then' in outcome), '返回 Promise 会让内核掉进 insert 分支插 undefined');
  assert.equal(outcome.insert.source, 'official-chat');
  assert.equal(outcome.insert.ref, 'g1-alpha');
  assert.equal(outcome.insert.label, ALPHA);
});

test('pending 句柄不得被渲染成就绪（PM 2026-10-20 拍板第 2 条）', () => {
  // pick 时刻插入的是**引用句柄（pending 态）**，对应 P1 状态机的 `selected`；
  // `ready` 只在 serialize 成功那一刻产生，且只影响模型输入、不回填编辑器。
  // 所以 chip 的 label 里**不许**出现「就绪」字样——写了就是「假装就绪」。
  const { api, ctx, registered } = loadClient();
  api.apply(ctx);
  const outcome = registered[0].onPick({ candidate: candidate('g1-alpha', ALPHA), session: { sessionId: 's1' }, span: { start: 0, end: 1, draftRev: 7 } });
  assert.doesNotMatch(
    outcome.insert.label,
    /就绪|已就绪|ready/i,
    'pending 句柄的 label 不得声称就绪',
  );
  assert.doesNotMatch(
    outcome.insert.clipboardText,
    /就绪|已就绪|ready/i,
    'pending 句柄的剪贴板文本同样不得声称就绪',
  );
});

test('openReference 区分 pending 与 ready（源侧必须能分辨状态）', async () => {
  // 读还没回来时点 chip → 报「读取中」；读完再点 → 报「已取到」。
  // 两种措辞必须不同，且 pending 那一支**不许**出现「就绪」。
  const { api, ctx, registered, notices } = loadClient({ config: { readDelayMs: 80 } });
  api.apply(ctx);
  const source = registered[0];
  source.onPick({ candidate: candidate('g1-alpha', ALPHA), session: { sessionId: 's1' }, span: { start: 0, end: 1, draftRev: 7 } });

  assert.equal(source.openReference({ sessionId: 's1' }, { ref: 'g1-alpha', appearance: 'session' }), true);
  assert.equal(notices.length, 1);
  assert.equal(notices[0].level, 'info');
  assert.match(notices[0].text, /读取中|尚未就绪/);
  assert.doesNotMatch(notices[0].text, /已就绪/, 'pending 不得说成已就绪');

  await new Promise((r) => setTimeout(r, 140));
  assert.equal(source.openReference({ sessionId: 's1' }, { ref: 'g1-alpha', appearance: 'session' }), true);
  // 读取落定会先给一条「已取到」（拍板第 3 条：状态变化用户要看得见），
  // 这次点击再给一条详情提示——两条都必须是「已取到」，不许出现「已就绪」。
  const last = notices[notices.length - 1];
  assert.match(last.text, /已取到/);
  assert.doesNotMatch(last.text, /已就绪/);
  assert.ok(
    notices.filter((n) => /已取到/.test(n.text)).length >= 2,
    `落定与详情两条提示都该是「已取到」：${JSON.stringify(notices)}`,
  );
});

test('读取落定时用户能看到状态变化（PM 2026-10-20 拍板第 3 条）', async () => {
  // 内核**没有**「更新既有 chip 状态」的接口（chip 是内核自己的草稿节点，
  // 插件拿不到编辑器），而状态写进 label 是纪律明令禁止的。于是这条变化
  // 只能由内核 notify 承载：落定 = 用户看得见；不落定 = 用户只看到
  // 自己没动过。
  const { api, ctx, registered, notices } = loadClient({ config: { readDelayMs: 30 } });
  api.apply(ctx);
  registered[0].onPick({ candidate: candidate('g1-alpha', ALPHA), session: { sessionId: 's1' }, span: { start: 0, end: 1, draftRev: 7 } });
  assert.equal(notices.length, 0, '读取还没落定时不该有提示');
  await new Promise((r) => setTimeout(r, 90));
  assert.equal(notices.length, 1, '读取完成必须让用户看到状态变化');
  assert.equal(notices[0].level, 'info');
  assert.match(notices[0].text, /已取到/);
});

test('读取失败也要让用户看到（error 级），不静默', async () => {
  const { api, ctx, registered, notices } = loadClient({ config: { readDelayMs: 10, failRead: true } });
  api.apply(ctx);
  registered[0].onPick({ candidate: candidate('g1-alpha', ALPHA), session: { sessionId: 's1' }, span: { start: 0, end: 1, draftRev: 7 } });
  await new Promise((r) => setTimeout(r, 60));
  assert.equal(notices.length, 1, '读取失败必须立刻让用户看到');
  assert.equal(notices[0].level, 'error');
  assert.match(notices[0].text, /读取失败/);
});

test('读取失败 → openReference 报 error（不是假装还悬着）', async () => {
  const { api, ctx, registered, notices } = loadClient({ config: { readDelayMs: 10, failRead: true } });
  api.apply(ctx);
  const source = registered[0];
  source.onPick({ candidate: candidate('g1-alpha', ALPHA), session: { sessionId: 's1' }, span: { start: 0, end: 1, draftRev: 7 } });
  await new Promise((r) => setTimeout(r, 60));
  source.openReference({ sessionId: 's1' }, { ref: 'g1-alpha', appearance: 'session' });
  // 落定那条 + 详情那条，两条都必须是 error 级且都说「读取失败」——
  // 失败被降级成 info 或含糊措辞，用户会以为还能发。
  assert.ok(notices.length >= 2, `落定与详情都该有提示：${JSON.stringify(notices)}`);
  assert.ok(
    notices.every((n) => n.level === 'error' && /读取失败/.test(n.text)),
    `失败提示必须是 error 级且说清失败：${JSON.stringify(notices)}`,
  );
});

test('serialize 等的是 onPick 那次读取（同一条 promise，不是重读）', async () => {
  const { api, ctx, registered } = loadClient({ config: { readDelayMs: 40 } });
  api.apply(ctx);
  const source = registered[0];
  source.onPick({ candidate: candidate('g1-alpha', ALPHA), session: { sessionId: 's1' }, span: { start: 0, end: 1, draftRev: 7 } });
  const text = await source.codec.serialize('g1-alpha', new AbortController().signal);
  assert.equal(text, BODY_ALPHA);
});

test('读取未完成时 serialize 不 resolve（内核因此会「等」而不是漏发）', async () => {
  const { api, ctx, registered } = loadClient({ config: { readDelayMs: 400 } });
  api.apply(ctx);
  const source = registered[0];
  source.onPick({ candidate: candidate('g1-alpha', ALPHA), session: { sessionId: 's1' }, span: { start: 0, end: 1, draftRev: 7 } });
  let settled = false;
  const pending = source.codec.serialize('g1-alpha', new AbortController().signal).then((v) => { settled = true; return v; });
  await new Promise((r) => setTimeout(r, 80));
  assert.equal(settled, false, '读取没完成就不许 resolve');
  const text = await pending;
  assert.equal(text, BODY_ALPHA);
});

test('读取失败 → serialize reject（内核据此拒绝发送，不静默降级）', async () => {
  const { api, ctx, registered } = loadClient({ config: { readDelayMs: 10, failRead: true } });
  api.apply(ctx);
  const source = registered[0];
  source.onPick({ candidate: candidate('g1-alpha', ALPHA), session: { sessionId: 's1' }, span: { start: 0, end: 1, draftRev: 7 } });
  await assert.rejects(() => source.codec.serialize('g1-alpha', new AbortController().signal), /固定读取失败/);
});

test('serialize 遇到不认识的 ref 就报错（不返回空串糊弄过去）', async () => {
  const { api, ctx, registered } = loadClient();
  api.apply(ctx);
  await assert.rejects(() => registered[0].codec.serialize('never-picked', new AbortController().signal), /找不到 ref/);
});

test('插件不自己插 chip（不绕开内核的输入事件）', async () => {
  // 插 chip 的唯一正规通路是内核的 `execute()`。插件自己 bail
  // `slash/input-insert-reference` 就是在绕开内核的 span CAS（用户继续编辑后
  // rev 变了，旧 span 插不进去），也正是计划 §5 禁止的「用事件/DOM 规避约束」。
  // 甲案不需要它——chip 由 onPick 的 insert 结果产生。
  const { api, ctx, registered, bailCalls } = loadClient({ config: { readDelayMs: 10 } });
  api.apply(ctx);
  registered[0].onPick({ candidate: candidate('g1-alpha', ALPHA), session: { sessionId: 's1' }, span: { start: 0, end: 1, draftRev: 7 } });
  await new Promise((r) => setTimeout(r, 80));
  assert.equal(bailCalls.length, 0, '插件不得自己插 chip');
});

test('序列化器必须挂在 codec 下（内核 source 契约，不是风格问题）', () => {
  // 内核发送路径按 owner 找序列化器：
  //   `owner?.codec === void 0` → reject「no serializer for reference source」
  //   → `owner.codec.serialize(ref, signal)`
  // 见 K\dsh-client-ui-input-trigger\lib\client.js:1249-1253。
  //
  // **G1 第一版就踩在这里**：serialize 写在 source 顶层 → chip 能插进去、
  // 菜单能出来，但内核永远不调用它，而且不报错，只是「没有序列化事件」。
  const { api, ctx, registered } = loadClient();
  api.apply(ctx);
  const source = registered[0];
  assert.equal(typeof source.codec, 'object', 'source.codec 必须是对象');
  assert.equal(typeof source.codec.serialize, 'function', '序列化器在 source.codec.serialize');
  assert.equal(typeof source.codec.clipboardText, 'function', '剪贴板文本在 source.codec.clipboardText');
  assert.equal(source.serialize, undefined, 'source 顶层不该再有 serialize（内核不会读它）');
  assert.equal(source.clipboardText, undefined, 'source 顶层不该再有 clipboardText');
});

test('candidates 只出固定夹具，且带分组标题', async () => {
  const { api, ctx, registered } = loadClient();
  api.apply(ctx);
  const rows = await registered[0].candidates({ sessionId: 's1' }, { query: '' });
  assert.equal(rows.length, 2);
  assert.equal(rows[0].section, '官方对话');
  assert.ok(rows.every((r) => JSON.parse(r.value).kind === 'official-chat'));
});
