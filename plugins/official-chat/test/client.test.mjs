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

// **协议口径只有一份，在 `host/protocol.js`**：v0.2 起 id 由 host 分配
// （客户端不自造 seq），所以**客户端束里已经没有**短码算法副本。
// 下面这个桩扮演的是 **host**：它按 host 的口径算 id，回给客户端。
import {
  PROVIDER_SHORT,
  makeSessionShort,
  makeSnapshotId,
  groupKeyOf,
  resolveCaptureCase,
} from '../host/protocol.js';

/**
 * 载入客户端束并造一个假的内核 ctx。
 *
 * @param {object} [options]
 * @param {boolean} [options.withInputTriggers] - 是否提供 inputTriggers 服务。
 * @param {boolean} [options.withSessions] - 是否提供 sessions 服务。
 * @param {boolean} [options.withHostRpc] - 是否提供内核 `connection` 服务
 *   （RPC 通道的宿主，host 半边的命令从它上面过来）。**默认给**：P1 起落盘是
 *   发送路径的前置条件。
 * @param {Function} [options.bail] - `actx.bail` 的实现（返回 true 表示编辑器接下了）。
 * @param {object} [options.config] - `window.__DSH_G1_CONFIG__`。
 * @param {boolean} [options.failDuplicates] - 让桩的 `duplicates` 端点失败（造
 *   「核对重复失败」的场景，协议 §2.3）。
 * @param {boolean} [options.multiGroup] - 让桩的 `duplicates` 端点回**两组**
 *   （协议 §3.2「多组形态」），且**故意按 groupKey 降序**给出 ⇒ 客户端必须
 *   自己按升序排。
 * @param {number} [options.bodyBytes] - 让桩给出**大正文**（造超预算场景，
 *   协议 §4）；不给就用固定夹具正文。
 * @param {object|null} [options.captureFailure] - 让桩的 `capture` 端点**失败**
 *   （造「第 14 码 `body_budget_exceeded`」这类协议错误）：`{code, detail}`。
 *   `detail` 照 host 的契约给（`host/rpc.js` 的 `toRpcError` 把它放在
 *   `details.detail`）；`detail` 省略就是**没有 detail**。
 */
function loadClient({
  withInputTriggers = true,
  withSessions = true,
  withHostRpc = true,
  bail,
  config = {},
  failDuplicates = false,
  multiGroup = false,
  bodyBytes = 0,
  captureFailure = null,
} = {}) {
  const registered = [];
  const effects = [];
  const bailCalls = [];
  const notices = [];
  const snapshots = new Map();
  const rpcCalls = [];
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
    // **客户端自己发 same-origin fetch**（协议 v0.4 §5）。
    // **惰性转发**：`fetchImpl` 在下面才声明，直接引用会撞 TDZ。
    // **没有桩时就是「没有 fetch」**——客户端据此报「host 不可达」。
    fetch: withHostRpc ? (...args) => fetchImpl(...args) : undefined,
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
    // **注意：不再提供 `connection` 服务**——协议 v0.4 起客户端**自己发
    // same-origin fetch** 打 exact route，不再读 `ctx.get('connection')`。
  };

  /**
   * 客户端用的 **same-origin fetch** 桩（协议 v0.4 §5 的通道形态）。
   *
   * **为什么不是 `connection.rpc.call` 桩**：浏览器侧内核自带的 `rpc.call` 打的
   * URL 是 `` `${channel}/${endpoint}`.slice(1) ``（相对路径、**不带 `/api``），
   * 打不到 `/api` 命名空间下的 exact route ⇒ 客户端改为自己发 fetch。
   *
   * **桩扮演 host，id 按 `host/protocol.js` 的口径算**：v0.2 起 id 由 host
   * 分配，客户端只取用——所以「算 id」这件事的权威口径在 host 半边，
   * 桩从这里取，与真实现（`host/seq-store.js`）同源。
   * 客户端自造 id 的那条路另有反向回归用例钉着（见下）。
   *
   * **信封照抄内核**：请求 `{type:'client-request', rpcId, method, payload}`，
   * 响应 `{type:'server-response', rpcId, result}`。
   *
   * @param {string|Request} input - URL。
   * @param {object} [init] - fetch init。
   * @returns {Promise<Response>}
   */
  const fetchImpl = withHostRpc
    ? async (input, init = {}) => {
        const url = new URL(String(input), 'http://127.0.0.1');
        const message = JSON.parse(String(init?.body ?? '{}'));
        rpcCalls.push({
          path: url.pathname,
          method: String(init?.method ?? 'GET'),
          endpoint: String(message.method ?? ''),
          payload: message.payload,
        });
        let result;
        try {
          const value = await hostCommands(message.method, message.payload);
          result = { ok: true, value };
        } catch (error) {
          result = {
            ok: false,
            error: {
              code: String(error?.code ?? 'host_error'),
              message: String(error?.message ?? error),
              // **与真 host 的 `toRpcError` 同构**：`detail` 走在 `details` 里
              // （`host/rpc.js:121-135`）⇒ 客户端要从这儿取（第 14 码的三个数
              // 只能从这儿来）。
              details: { detail: error?.detail ?? null },
            },
          };
        }
        return Response.json({ type: 'server-response', rpcId: message.rpcId, result });
      }
    : undefined;

  /** host 半边的桩：五个命令（协议 §5）。 */
  const hostCommands = async (endpoint, payload) => {
    if (endpoint === 'capabilities') return { ok: true, usedSeqs: {} };
    if (endpoint === 'catalog') {
      // **目录与 id 分配都在 host**（协议 v0.2 S-1）：客户端这一侧没有目录。
      //
      // 目录项的键面照协议 P4 §2.4 面二 / §4.3 ②（**恰好 6 个键**）：会话 id 的
      // 键名是 **`id`**（旧形态那个 `sessionId` 键已经换掉了）⇒ ⇒ 客户端读
      // `item.id`，读旧键名会拿到**空串**（那是一条会「看起来正常」的坏链）。
      const items = Object.entries(FIXTURE_BODIES).map(([sessionId, body]) => ({
        id: sessionId,
        title: sessionId === 'g1-alpha' ? ALPHA : BETA,
        title_type: 'USER',
        model_type: 'deepseek-chat',
        pinned: false,
        updated_at: 0,
        body,
      }));
      const allocations = {};
      for (const item of items) {
        allocations[item.id] = makeSnapshotId({
          providerShort: PROVIDER_SHORT,
          sessionShort: makeSessionShort(item.id),
          seq: 1,
        });
      }
      return {
        ok: true,
        page: {
          accountScope: '',
          providerId: PROVIDER_SHORT,
          items: items.map(({ body: _body, ...rest }) => rest),
          cursor: '',
          hasMore: false,
          fetchedAt: '1970-01-01T00:00:00.000Z',
        },
        allocations,
        cached: false,
      };
    }
    if (endpoint === 'capture') {
      const sessionId = String(payload?.sessionId ?? '');
      const snapshotId = String(payload?.snapshotId ?? '');
      // **造协议错误**（第 14 码这类）：`captureFailure` 给了就照它失败。
      if (captureFailure !== null) {
        const error = new Error(
          String(captureFailure.message ?? `桩注入的失败：${String(captureFailure.code)}`),
        );
        error.code = String(captureFailure.code);
        if (captureFailure.detail !== undefined) {
          error.detail = captureFailure.detail;
        }
        throw error;
      }
      if (snapshots.has(snapshotId)) {
        const error = new Error(`快照已存在（同 id 不重写）：${snapshotId}`);
        error.code = 'snapshot_exists';
        throw error;
      }
      // **大正文场景**（P2 §4 预算预检）：给了 `bodyBytes` 就用它 ⇒ 两份大正文
      // 合计超过单次消息预算（262144）⇒ 触发「预算超限」那条提示。
      const body = bodyBytes > 0 ? 'A'.repeat(bodyBytes) : (FIXTURE_BODIES[sessionId] ?? '');
      const snapshot = {
        schemaVersion: 1,
        snapshotId,
        providerId: PROVIDER_SHORT,
        accountScope: '',
        sessionId,
        branch: 'main',
        messages: [{ role: 'user', text: body }],
        contentDigest: 'stub-digest',
        captureResult: { status: 'ok', omitted: [], unsupported: [] },
        capturedAt: '',
        sourceUpdatedAt: '',
        sourceFingerprint: '',
      };
      snapshots.set(snapshotId, { snapshot, body });
      return { ok: true, snapshot };
    }
    if (endpoint === 'preview') {
      const found = snapshots.get(String(payload?.snapshotId ?? ''));
      if (found === undefined) {
        const error = new Error(`快照不存在：${String(payload?.snapshotId ?? '')}`);
        error.code = 'snapshot_invalid';
        throw error;
      }
      return { ok: true, snapshot: found.snapshot, body: found.body, state: 'ready' };
    }
    if (endpoint === 'duplicates') {
      if (multiGroup) {
        // **多组场景**（协议 §3.2「多组形态」· PM 01:3x）：两组。
        //
        // **桩扮演 host ⇒ 也要按 `groupKey` 升序产出**（PM 01:3x：排序的落点在
        // host 侧 ⇒ 任何消费方都拿到稳定顺序；client 照单渲染）。
        const multiGroups = [
          {
            groupKey: 'ffffffffffffffff',
            members: [
              { snapshotId: 'ds~m~0001', capturedAt: '2026-10-21T00:00:00.000Z', contentDigest: 'd2' },
              { snapshotId: 'ds~m~0002', capturedAt: '2026-10-21T00:00:00.000Z', contentDigest: 'd2' },
              { snapshotId: 'ds~m~0003', capturedAt: '2026-10-21T00:00:00.000Z', contentDigest: 'd2' },
            ],
            redundantBytes: 200,
          },
          {
            groupKey: '0000000000000001',
            members: [
              { snapshotId: 'ds~m~0004', capturedAt: '2026-10-20T00:00:00.000Z', contentDigest: 'd1' },
              { snapshotId: 'ds~m~0005', capturedAt: '2026-10-20T00:00:00.000Z', contentDigest: 'd1' },
            ],
            redundantBytes: 100,
          },
        ];
        multiGroups.sort((a, b) => (a.groupKey < b.groupKey ? -1 : a.groupKey > b.groupKey ? 1 : 0));
        return {
          ok: true,
          groups: multiGroups,
          budget: { limitBytes: 262144, projectedBytes: 300, wouldExceed: false, exceedByBytes: 0 },
        };
      }
      if (failDuplicates) {
        // **造「核对重复失败」**（协议 §2.3 的失败路径）。
        const error = new Error('快照不存在：桩注入的失败');
        error.code = 'snapshot_invalid';
        throw error;
      }
      // **P2 §2.2 的出参**：按固定 id 读盘、按「五字段全等」分组（协议 §1.1）。
      const ids = Array.isArray(payload?.snapshotIds) ? payload.snapshotIds : [];
      const rows = [];
      for (const raw of ids) {
        const found = snapshots.get(String(raw ?? ''));
        if (found === undefined) {
          const error = new Error(`快照不存在：${String(raw ?? '')}`);
          error.code = 'snapshot_invalid';
          throw error;
        }
        rows.push({
          id: String(raw),
          snapshot: found.snapshot,
          bytes: Buffer.byteLength(String(found.body ?? ''), 'utf8'),
        });
      }
      const byKey = new Map();
      for (const row of rows) {
        const key = groupKeyOf(row.snapshot);
        if (!byKey.has(key)) byKey.set(key, []);
        byKey.get(key).push(row);
      }
      const groups = [];
      for (const [groupKey, members] of byKey) {
        // **只报成员数 ≥ 2 的组**（单成员组不是重复，报它没有意义）。
        if (members.length < 2) continue;
        groups.push({
          groupKey,
          members: members.map((one) => ({
            snapshotId: one.id,
            capturedAt: String(one.snapshot?.capturedAt ?? ''),
            contentDigest: String(one.snapshot?.contentDigest ?? ''),
          })),
          redundantBytes: (members.length - 1) * members[0].bytes,
        });
      }
      const projectedBytes = rows.reduce((sum, one) => sum + one.bytes, 0);
      const limitBytes = 262144;
      return {
        ok: true,
        groups,
        budget: {
          limitBytes,
          projectedBytes,
          wouldExceed: projectedBytes > limitBytes,
          exceedByBytes: Math.max(0, projectedBytes - limitBytes),
        },
      };
    }
    const error = new Error(`未知端点：${endpoint}`);
    error.code = 'source_unsupported';
    throw error;
  };

  const ctx = {
    get: (name) => services[name],
    // cordis 的 effect 是**立即执行**的（挂载时跑一次，卸载时回收），
    // 这里照抄：只记录不执行的话，来源根本不会注册，测试会假红。
    effect: (fn, label) => { effects.push({ fn, label }); fn(); },
  };

  const api = module.factory();
  return { api, ctx, registered, effects, bailCalls, notices, sandbox, snapshots, rpcCalls };
}

/**
 * 造一个候选行（与内核菜单里的行同构）。
 *
 * **v0.2：候选行带 `snapshotId`**——id 由 host 在 `catalog` 那一步分配好，
 * 客户端只取用（协议 v0.2 S-1：id 分配权归 host，client 不得自造 seq）。
 */
let candidateSeq = 0;
function candidate(id, title) {
  candidateSeq += 1;
  const snapshotId = makeSnapshotId({
    providerShort: PROVIDER_SHORT,
    sessionShort: makeSessionShort(id),
    seq: candidateSeq,
  });
  return {
    name: title,
    description: '固定夹具 · G1 验证用',
    icon: 'session',
    section: 'Web Chat',
    value: JSON.stringify({ kind: 'official-chat', id, title, snapshotId }),
  };
}

const ALPHA = 'G1 固定对话 Alpha';
const BODY_ALPHA = '【G1 固定正文 Alpha】第一行固定正文。\n第二行固定正文。';
const BETA = 'G1 固定对话 Beta';
const BODY_BETA = '【G1 固定正文 Beta】只有一行。';

/** 夹具正文表（host 侧的桩按官网会话 id 取正文）。 */
const FIXTURE_BODIES = { 'g1-alpha': BODY_ALPHA, 'g1-beta': BODY_BETA };

/**
 * pick 一次，返回 `{outcome, ref}`。
 *
 * **P1 起 `ref` 不是对话 id**（协议 S-1：`ref === snapshotId`），所以断言必须
 * 拿 onPick 真实返回的那一串，不能写死 'g1-alpha'。
 */
function pick(source, id = 'g1-alpha', title = ALPHA, sessionId = 's1', row = null) {
  const outcome = source.onPick({
    candidate: row ?? candidate(id, title),
    session: { sessionId },
    span: { start: 0, end: 1, draftRev: 7 },
  });
  return { outcome, ref: outcome?.insert?.ref };
}

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
  assert.deepEqual([...api.inject], ['inputTriggers', 'sessions'], 'inject 装的是服务名，不是包名（v0.4 起不再用 connection 服务）');
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
  const { outcome } = pick(source);
  assert.ok(outcome && typeof outcome === 'object', 'onPick 必须同步返回对象，不能返回 Promise');
  assert.ok(!('then' in outcome), '返回 Promise 会让内核掉进 insert 分支插 undefined');
  assert.equal(outcome.insert.source, 'official-chat');
  assert.equal(outcome.insert.label, ALPHA);
  // P1（S-1）：`ref` 是**快照 id**，不是对话 id。
  assert.notEqual(outcome.insert.ref, 'g1-alpha', 'ref 不再是对话 id');
  assert.match(outcome.insert.ref, /^[a-z0-9]{1,4}~[a-z0-9]{1,12}~\d{4}$/, 'ref 必须符合协议 S-1 的形状');
  assert.ok(outcome.insert.ref.length <= 24, 'ref 整串 ≤ 24 可见字符（chip 宽度）');
  assert.equal(outcome.insert.clipboardText, outcome.insert.ref, 'clipboardText 就是 ref 本身');
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
  // 读还没回来时点 chip → 报「读取中」；读完并落盘再点 → 报「已取到并落盘」。
  // 两种措辞必须不同，且 pending 那一支**不许**出现「就绪」。
  const { api, ctx, registered, notices } = loadClient({ config: { readDelayMs: 80 } });
  api.apply(ctx);
  const source = registered[0];
  const { ref } = pick(source);

  assert.equal(source.openReference({ sessionId: 's1' }, { ref, appearance: 'session' }), true);
  assert.equal(notices.length, 1);
  assert.equal(notices[0].level, 'info');
  assert.match(notices[0].text, /读取中|尚未就绪/);
  assert.doesNotMatch(notices[0].text, /已就绪/, 'pending 不得说成已就绪');

  await new Promise((r) => setTimeout(r, 160));
  assert.equal(source.openReference({ sessionId: 's1' }, { ref, appearance: 'session' }), true);
  await new Promise((r) => setTimeout(r, 20));
  // 落盘那条 + 详情那条，两条都必须是「已取到」，不许出现「已就绪」。
  const texts = notices.map((n) => n.text);
  assert.ok(
    texts.filter((t) => /已取到/.test(t)).length >= 2,
    `落定与详情两条提示都该是「已取到」：${JSON.stringify(notices)}`,
  );
  assert.ok(
    texts.every((t) => !/已就绪/.test(t)),
    `不许出现「已就绪」：${JSON.stringify(notices)}`,
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
  // **只断「状态提示恰好一条」**：P2 协议 §3 要求「重复发生时」另有一条可见
  // 提示（承载位同为内核输入区 notice）⇒ 提示条数不再恒为 1。
  // **这里断的是状态变化本身**（「已取到」那条），不是「总共几条通知」。
  assert.equal(
    notices.filter((n) => /已取到/.test(String(n?.text ?? ''))).length,
    1,
    '读取完成必须让用户看到状态变化',
  );
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
  const { ref } = pick(source);
  await new Promise((r) => setTimeout(r, 60));
  source.openReference({ sessionId: 's1' }, { ref, appearance: 'session' });
  // 落定那条 + 详情那条，两条都必须是 error 级且都说「读取失败」——
  // 失败被降级成 info 或含糊措辞，用户会以为还能发。
  assert.ok(notices.length >= 2, `落定与详情都该有提示：${JSON.stringify(notices)}`);
  assert.ok(
    notices.every((n) => n.level === 'error' && /读取失败/.test(n.text)),
    `失败提示必须是 error 级且说清失败：${JSON.stringify(notices)}`,
  );
});

test('serialize 等的是 onPick 那次读取（同一条链，不是重读）', async () => {
  const { api, ctx, registered } = loadClient({ config: { readDelayMs: 40 } });
  api.apply(ctx);
  const source = registered[0];
  const { ref } = pick(source);
  const text = await source.codec.serialize(ref, new AbortController().signal);
  assert.equal(text, BODY_ALPHA);
});

test('读取未完成时 serialize 不 resolve（内核因此会「等」而不是漏发）', async () => {
  const { api, ctx, registered } = loadClient({ config: { readDelayMs: 400 } });
  api.apply(ctx);
  const source = registered[0];
  const { ref } = pick(source);
  let settled = false;
  const pending = source.codec.serialize(ref, new AbortController().signal).then((v) => { settled = true; return v; });
  await new Promise((r) => setTimeout(r, 80));
  assert.equal(settled, false, '读取没完成就不许 resolve');
  const text = await pending;
  assert.equal(text, BODY_ALPHA);
});

test('读取失败 → serialize reject（内核据此拒绝发送，不静默降级）', async () => {
  const { api, ctx, registered } = loadClient({ config: { readDelayMs: 10, failRead: true } });
  api.apply(ctx);
  const source = registered[0];
  const { ref } = pick(source);
  await assert.rejects(() => source.codec.serialize(ref, new AbortController().signal), /固定读取失败/);
});

test('serialize 遇到不认识的 ref 就报错（不返回空串糊弄过去）', async () => {
  const { api, ctx, registered } = loadClient();
  api.apply(ctx);
  await assert.rejects(
    () => registered[0].codec.serialize('never-picked', new AbortController().signal),
    /快照不存在|snapshot_invalid/,
    '不认识的 ref 必须被拒，不许返回空串',
  );
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
  // **分组标题**（izzy 2026-10-21 当面点名）：「官方对话」⇒「Web Chat」⇒
  // **断言保留、只改值**（它就是钉这个字面量的，别删）。
  assert.equal(rows[0].section, 'Web Chat');
  assert.ok(rows.every((r) => JSON.parse(r.value).kind === 'official-chat'));
  // 目录项的会话 id 键名是 **`id`**（协议 P4 §2.4 面二 / §4.3 ②）⇒ ⇒ 候选行
  // 必须真的带上它。**读旧键名会拿到空串** —— 那是「看起来正常」的坏链，
  // 所以这条断言不能省。
  const ids = rows.map((r) => JSON.parse(r.value).id);
  assert.deepEqual(ids.sort(), ['g1-alpha', 'g1-beta']);
});

// ---------------------------------------------------------------------------
// P1 协议：S-1 单一 id 制度 / S-3 单一事实源 / S-4 状态口径门控
// ---------------------------------------------------------------------------

test('S-1：同一条对话选两次 → 两个 ref → 第二个不覆盖第一个', async () => {
  // G1 现状 `reads` 以对话 id 为主键，选两次会互相覆盖（第一个 chip 的正文被
  // 换成第二次的）。P1 的唯一性粒度 = **一次 pick**。
  const { api, ctx, registered, snapshots } = loadClient({ config: { readDelayMs: 20 } });
  api.apply(ctx);
  const source = registered[0];
  const first = pick(source);
  const second = pick(source);
  assert.notEqual(first.ref, second.ref, '两次 pick 必须给出两个不同的 ref');
  assert.match(first.ref, /~\d{4}$/);
  assert.match(second.ref, /~\d{4}$/);
  await new Promise((r) => setTimeout(r, 120));
  assert.equal(snapshots.size, 2, '两份快照都在（第二个没覆盖第一个）');
  assert.equal(snapshots.get(first.ref).body, BODY_ALPHA);
  assert.equal(snapshots.get(second.ref).body, BODY_ALPHA);
});

test('S-1：ref 整串 ≤ 24 可见字符（ref 变长 = chip 变宽 = 用户直接看见）', () => {
  const { api, ctx, registered } = loadClient();
  api.apply(ctx);
  const { ref } = pick(registered[0]);
  assert.ok(ref.length <= 24, `ref 过长：${ref}`);
});

test('S-1：句柄就是 host 分配的快照 id（客户端不自造 seq，协议 v0.2 S-1）', () => {
  const { api, ctx, registered } = loadClient();
  api.apply(ctx);
  const source = registered[0];
  const row = candidate('g1-alpha', ALPHA);
  const { ref } = pick(source, 'g1-alpha', ALPHA, 's1', row);
  const picked = JSON.parse(row.value);
  assert.equal(ref, picked.snapshotId, 'ref 必须就是 host 分配的那个 id');
  assert.match(ref, /^[a-z0-9]{1,4}~[a-z0-9]{1,12}~\d{4}$/, 'id 形态与协议 S-1 一致');
});

test('通道：客户端自己发 same-origin fetch 打 exact route，端点名是协议 §5 的五个之一', async () => {
  const { api, ctx, registered, rpcCalls } = loadClient({ config: { readDelayMs: 20 } });
  api.apply(ctx);
  const source = registered[0];
  const { ref } = pick(source);
  await new Promise((r) => setTimeout(r, 120));
  // 正文要走 `preview`（按固定 id 读盘）——所以这里真的发一次。
  await source.codec.serialize(ref, new AbortController().signal);
  assert.ok(rpcCalls.length > 0, '必须真的发 fetch');
  for (const call of rpcCalls) {
    // **全等匹配**：内核是 `fetchRoutes.get(url.pathname)`，错一个字符就 404。
    assert.equal(call.path, '/api/xlink-official-chat', '路径必须与 host 半边注册的 exact route 全等');
    assert.equal(call.method, 'POST', 'exact route 只接 POST');
    // **P1 §5 的 5 个 ＋ P2 §2.2 的 `duplicates`**（重复识别 ＋ 预算预检）
    // ⇒ 端点集合是 6 个（P1 §5 的清单仍写 5 个，协议侧待 PM 同步）。
    assert.ok(
      ['capabilities', 'catalog', 'capture', 'cancel', 'preview', 'duplicates'].includes(call.endpoint),
      `端点名不在协议 §5 里：${call.endpoint}`,
    );
  }
  assert.ok(rpcCalls.some((c) => c.endpoint === 'capture'), '落盘必须经 capture');
  assert.ok(rpcCalls.some((c) => c.endpoint === 'preview'), '正文必须经 preview（按固定 id 读盘）');
  // `capture` 必须带上**官网会话 id**与**host 分配的 id**：句柄是 host 分配的，
  // 客户端把它原样带回来（协议 v0.2 S-1：client 不得自造 seq）。
  const capture = rpcCalls.find((c) => c.endpoint === 'capture');
  assert.equal(capture.payload.sessionId, 'g1-alpha', 'capture 必须带官网会话 id');
  assert.equal(capture.payload.snapshotId, ref, 'capture 必须带 host 分配的快照 id');
  assert.equal(capture.payload.seq, undefined, '客户端不许自造 seq（v0.2 S-1）');
});

test('S-3：落盘成功后状态由快照文件派生（内存里不再留第二条真相）', async () => {
  const { api, ctx, registered, sandbox } = loadClient({ config: { readDelayMs: 20 } });
  api.apply(ctx);
  const source = registered[0];
  const { ref } = pick(source);
  await new Promise((r) => setTimeout(r, 120));
  const probe = sandbox.__DSH_XLINK_OFFICIAL_CHAT_PROBE__;
  assert.ok(
    probe.events.some((e) => e.kind === 'snapshot-saved' && e.ref === ref),
    '落盘必须留下 snapshot-saved 事件',
  );
  // 落盘后再点 chip：状态**从文件**读（事件里 from=snapshot-file），不是从内存。
  source.openReference({ sessionId: 's1' }, { ref, appearance: 'session' });
  await new Promise((r) => setTimeout(r, 20));
  const opened = probe.events.filter((e) => e.kind === 'open-reference').pop();
  assert.equal(opened.from, 'snapshot-file', '已落盘的句柄必须从文件派生状态');
});

test('S-4：进入模型的正文取自落盘内容（逐字节相同）', async () => {
  const { api, ctx, registered, snapshots, sandbox } = loadClient({ config: { readDelayMs: 20 } });
  api.apply(ctx);
  const source = registered[0];
  const { ref } = pick(source);
  await new Promise((r) => setTimeout(r, 120));
  const text = await source.codec.serialize(ref, new AbortController().signal);
  assert.equal(text, BODY_ALPHA, '模型输入的正文必须与落盘内容逐字节相同');
  assert.equal(text, snapshots.get(ref).body, '模型输入 = 落盘内容');
  const probe = sandbox.__DSH_XLINK_OFFICIAL_CHAT_PROBE__;
  const ok = probe.events.filter((e) => e.kind === 'serialize-ok').pop();
  assert.equal(ok.fromFile, true, 'serialize 必须是从文件取的正文');
});

test('S-4 反向回归：快照被清理后 → 拒（不静默重读、不回退到内存那份）', async () => {
  const { api, ctx, registered, snapshots } = loadClient({ config: { readDelayMs: 20 } });
  api.apply(ctx);
  const source = registered[0];
  const { ref } = pick(source);
  await new Promise((r) => setTimeout(r, 120));
  snapshots.delete(ref);
  await assert.rejects(
    () => source.codec.serialize(ref, new AbortController().signal),
    /快照不存在|snapshot_invalid/,
    '快照没了必须拒，不许悄悄重读',
  );
});

test('没有 host 通道 → 落盘失败 → 发送时拒（不把内存那份发给模型）', async () => {
  const { api, ctx, registered } = loadClient({
    config: { readDelayMs: 20 },
    withHostRpc: false,
  });
  api.apply(ctx);
  const source = registered[0];
  const { ref } = pick(source);
  await new Promise((r) => setTimeout(r, 120));
  await assert.rejects(
    () => source.codec.serialize(ref, new AbortController().signal),
    /host 不可达/,
    '没有 host 通道就不许把内存正文发出去',
  );
});

test('P2：选中第二条同组引用 ⇒ 出现 info 档的重复提示（承载位 = 输入区 notice）', async () => {
  const { api, ctx, registered, notices } = loadClient({ config: { readDelayMs: 20 } });
  api.apply(ctx);
  const source = registered[0];
  // **同一行 pick 两次** ⇒ 两个**不同**的 `snapshotId`（协议 S-1：id 不复用），
  // 但正文逐字节相同 ⇒ 五字段全等 ⇒ **同组**（协议 §1.1）。
  const first = pick(source);
  const second = pick(source);
  assert.notEqual(first.ref, second.ref, '两次 pick 必须拿到两个不同的快照 id');
  await new Promise((r) => setTimeout(r, 140));
  const dup = notices.filter((n) => /同组 \d+ 条内容完全相同/.test(String(n?.text ?? '')));
  assert.equal(dup.length, 1, `同组提示应恰好一条：${JSON.stringify(notices)}`);
  // **协议 v0.2 §3.2**：必须是 `info` 档——内核只渲染 `info` 档的**输入区常驻条**，
  // `error` 档只弹瞬态 toast（内核 `:18808`）⇒ 用 `error` 档当场违反「可复查」。
  assert.equal(dup[0].level, 'info', '重复提示必须是 info 档（常驻条，不是瞬态 toast）');
  // **文案必须自包含**（内核 notice 是**单槽**，最新一条覆盖旧条）⇒ 条数与字节数
  // 写在同一条里，不能指望另一条还在。
  assert.match(dup[0].text, /同组 2 条内容完全相同/, '提示要自包含同组条数');
  assert.match(dup[0].text, /重复可省 \d+ 字节/, '提示要自包含重复字节数');
  // **成品文案（UX 定稿 2026-10-21 · 协议 v0.6 §3.2 收录原文）**⇒ 实现侧照抄
  // 原文：「本次核对：」抗 stale（条会被内核顶掉）、用「引用正文」不用「消息正文」。
  assert.ok(
    dup[0].text.startsWith('本次核对：'),
    `提示要用「本次核对：」开头：${dup[0].text}`,
  );
  assert.match(
    dup[0].text,
    /引用正文 \d+ 字节，未超出单次消息预算 262144 字节。$/,
    `未超限时的预算口径照抄定稿原文：${dup[0].text}`,
  );
});

test('P2：核对重复失败 ⇒ info 档可见提示 + 不阻塞发送（协议 §2.3）', async () => {
  const { api, ctx, registered, notices } = loadClient({
    config: { readDelayMs: 20 },
    failDuplicates: true,
  });
  api.apply(ctx);
  const source = registered[0];
  pick(source);
  await new Promise((r) => setTimeout(r, 140));
  const failed = notices.filter((n) => /无法核对是否重复/.test(String(n?.text ?? '')));
  assert.equal(failed.length, 1, `失败提示应恰好一条：${JSON.stringify(notices)}`);
  // **协议 v0.2 §3.2**：失败提示也要 `info` 档（`error` 档只弹瞬态 toast）。
  assert.equal(failed[0].level, 'info', '失败提示必须是 info 档（常驻条，不是瞬态 toast）');
  // **措辞纪律（协议 §2.3）**：界面文案**不得断言发送结果**——正文能不能真发出
  // 由 P1 S-5 / S-6 判定，不是 P2 能承诺的。
  assert.doesNotMatch(
    failed[0].text,
    /已发送|发送失败|发送被阻止|发送被拦/,
    `失败文案不得断言发送结果：${failed[0].text}`,
  );
  // **成品文案（UX 定稿 · 协议 v0.6 §3.2 收录原文）**⇒ 实现侧照抄原文。
  assert.equal(failed[0].text, '无法核对是否重复。', `失败文案照抄定稿原文：${failed[0].text}`);
  // **失败不阻塞发送**（协议 §2.3「不额外拦截发送」）⇒ 正文仍取得到。
  const { ref } = pick(source);
  await new Promise((r) => setTimeout(r, 60));
  const text = await source.codec.serialize(ref, new AbortController().signal);
  assert.equal(text, BODY_ALPHA, '核对失败不得影响正文照发');
});

test('P2：多组 ⇒ 文案「N 组重复：第 i 组 …」，逐组各一句（协议 §3.2 多组形态）', async () => {
  // **多组形态**（协议 §3.2「多组形态」· PM 01:3x）：
  // · 逐组各一句，每句带**自己那组**的条数与字节数（组内口径）；
  // · **先说组数、再逐组编号** ⇒ 不用「同组 N 条」并排（会被读成「同一组既是
  //   3 条又是 2 条」的表面矛盾）；
  // · 顺序按 host 给的（host 按 `groupKey` 升序产出）⇒ client 照单渲染。
  const { api, ctx, registered, notices, sandbox } = loadClient({
    config: { readDelayMs: 20 },
    multiGroup: true,
  });
  api.apply(ctx);
  const source = registered[0];
  pick(source);
  await new Promise((r) => setTimeout(r, 140));
  const probe = sandbox.__DSH_XLINK_OFFICIAL_CHAT_PROBE__;
  const group = probe.events.filter((e) => e.kind === 'duplicate-groups').pop();
  assert.ok(group, '多组时必须出重复组提示');
  // **逐字照抄定稿原文**（PM 01:3x 的多组形态原文）。
  assert.equal(
    group.text,
    '本次核对：2 组重复：第 1 组 2 条内容完全相同，重复可省 100 字节；' +
      '第 2 组 3 条内容完全相同，重复可省 200 字节；' +
      '引用正文 300 字节，未超出单次消息预算 262144 字节。',
    `多组文案必须逐字等于定稿原文：${group.text}`,
  );
  // **notice.groups 与文案同序**（host 给的序就是渲染的序）。
  // （**用字符串比较而不是 deepEqual**：这些数组是在 vm 沙箱 realm 里造的 ⇒
  //  `deepStrictEqual` 会因**原型不同**判不等，值其实一样。）
  const keys = group.groups.map((g) => String(g.groupKey));
  assert.equal(
    keys.join(','),
    [...keys].sort().join(','),
    `notice.groups 必须按 groupKey 升序：${keys.join(' / ')}`,
  );
  // **用户侧看到的那条**也是同一句（内核 notify 只带 level+text）。
  const shown = notices.filter((n) => /本次核对：/.test(String(n?.text ?? '')));
  assert.equal(shown.length, 1, `用户侧应看到一条提示：${JSON.stringify(notices)}`);
  assert.equal(shown[0].text, group.text, '界面那条与结构化那条必须是同一句');
});

test('P2：同组 + 超预算 ⇒ 两个触发点的提示都出（协议 §3.1 各自独立）', async () => {
  // **早退会把预算提示吃掉**：协议 §3.1 的两个触发点（「重复发生时」「超预算
  // 时」）**各自独立**，两者同时成立时**两条提示都要出**。
  const { api, ctx, registered, notices, sandbox } = loadClient({
    config: { readDelayMs: 20 },
    bodyBytes: 150_000,
  });
  api.apply(ctx);
  const source = registered[0];
  pick(source);
  pick(source);
  await new Promise((r) => setTimeout(r, 180));
  // **按 kind 区分两条**（内核 notify 只带 level+text ⇒ 靠文案分不出来：协议
  // v0.6 §3.2 要求**两条逐字相同**）⇒ 从探针事件里取 kind。
  const probe = sandbox.__DSH_XLINK_OFFICIAL_CHAT_PROBE__;
  const group = probe.events.filter((e) => e.kind === 'duplicate-groups').pop();
  const budget = probe.events.filter((e) => e.kind === 'budget-exceeded').pop();
  assert.ok(group, '重复组提示必须发（协议 §3.1-1）');
  assert.ok(budget, '预算提示必须发（协议 §3.1-2：早退会把它吃掉）');
  // **两条逐字相同**（协议 v0.6 §3.2）——后发那条自包含全部事实 ⇒ 两句是同一句。
  assert.equal(
    group.text,
    budget.text,
    `两条文案必须逐字相同：${group.text} / ${budget.text}`,
  );
  // **成品文案照抄 UX 定稿原文**（协议 v0.6 §3.2 收录原文）⇒ 逐字比对。
  assert.equal(
    group.text,
    '本次核对：同组 2 条内容完全相同，重复可省 150000 字节；' +
      '引用正文 300000 字节，超出单次消息预算 262144 字节（超 37856 字节）。',
    `成品文案必须逐字等于定稿原文：${group.text}`,
  );
  // **两条都必须是 `info` 档**（协议 v0.2 §3.2：error 档只弹瞬态 toast）。
  const shown = notices.filter((n) => /本次核对：/.test(String(n?.text ?? '')));
  assert.equal(shown.length, 2, `用户侧应看到两条提示：${JSON.stringify(notices)}`);
  assert.ok(
    shown.every((n) => n.level === 'info'),
    `两条提示都必须是 info 档：${JSON.stringify(notices)}`,
  );
  // **数字全在文案里**（自包含：内核 notice 是单槽 ⇒ 覆盖后剩下哪条，用户都得
  // 同时看到「同组几条」与「超了多少字节」）⇒ 这几条断言钉住它。
  assert.match(group.text, /同组 2 条内容完全相同/, '要自包含同组条数');
  assert.match(group.text, /重复可省 150000 字节/, '要自包含重复字节数');
  assert.match(group.text, /引用正文 300000 字节/, '要自包含预估字节数');
  assert.match(group.text, /超 37856 字节/, '要自包含超出字节数');
});

// ─────────────────────────────────────────────────────────────────────────────
// P1 §3 **第 14 码** `body_budget_exceeded` 的界面文案。
//
// **为什么要专门一条**：其它码落的那条固定串（「G1：引用「…」的正文读取失败，
// 发送时会拒绝这次提交」）与 `body_too_large` **完全同文** ⇒ 用户分不出
// 「这一条太长」与「整条超 256 KB」，而这两件事的**下一步不同**。
// ─────────────────────────────────────────────────────────────────────────────

test('P1 第 14 码 body_budget_exceeded ⇒ 专属文案（带三个数字，info 档常驻条）', async () => {
  // **`detail` 取真 host 的产出，不手抄** —— 手抄的那一串只证明「我抄对了」，
  // **证明不了「客户端吃得住真 host 的形状」** ⇒ 万一 host 那边措辞漂了一字，
  // 端侧用例会照样绿、生产却静默退化成无数字文案 ⇒ ⇒ 这里直接调真代码取。
  const resolved = resolveCaptureCase({
    kind: 'capture',
    schemaVersion: 1,
    messages: [{ role: 'user', text: 'A'.repeat(70000) }],
  });
  assert.equal(resolved.kind, 'error', '70000 字节的单条正文必须被拒');
  assert.equal(resolved.code, 'body_budget_exceeded');
  const detail = String(resolved.detail);
  // **契约形态本身也要钉住**（三个数与措辞）⇒ 客户端不是靠「碰巧对」过的。
  assert.equal(
    detail,
    '单条正文超单条正文预算：正文 70000 字节，超出单条正文预算 65536 字节（超 4464 字节）',
    `detail 契约形态必须逐字不变：${detail}`,
  );
  const { api, ctx, registered, notices, sandbox } = loadClient({
    config: { readDelayMs: 10 },
    captureFailure: { code: 'body_budget_exceeded', detail },
  });
  api.apply(ctx);
  const source = registered[0];
  pick(source);
  await new Promise((r) => setTimeout(r, 80));
  const shown = notices.filter((n) => /这条引用太长/.test(String(n?.text ?? '')));
  assert.equal(shown.length, 1, `第 14 码应恰好一条专属文案：${JSON.stringify(notices)}`);
  // **成品文案逐字照抄定稿原文**（PM 2026-10-21 给定，不许改字）。
  assert.equal(
    shown[0].text,
    '这条引用太长：正文 70000 字节，超出单条正文预算 65536 字节（超 4464 字节）。' +
      '请移除这条引用，换一段更短的正文。',
    `第 14 码文案必须逐字等于定稿原文：${shown[0].text}`,
  );
  // **必须是 `info` 档**：内核只渲染 `info` 档的**输入区常驻条**，`error` 档
  // 只弹瞬态 toast（内核 `:18808` / `:18982-18986`）⇒ 用 `error` 档就没有常驻条。
  assert.equal(shown[0].level, 'info', '第 14 码必须是 info 档（常驻条，不是瞬态 toast）');
  // **只出这一条**：一句「太长」＋一句「读取失败」＝ 互相矛盾的两句。
  assert.equal(notices.length, 1, `第 14 码不得同时落别的提示：${JSON.stringify(notices)}`);
  assert.equal(
    notices.filter((n) => /正文读取失败/.test(String(n?.text ?? ''))).length,
    0,
    `第 14 码不得同时落那条固定串：${JSON.stringify(notices)}`,
  );
  // **探针里的 kind 要分得开**（内核 notify 只带 level+text ⇒ 判据/探针靠 kind
  // 区分这一条）。
  const probe = sandbox.__DSH_XLINK_OFFICIAL_CHAT_PROBE__;
  const event = probe.events.filter((e) => e.kind === 'body-budget-exceeded').pop();
  assert.ok(event, '第 14 码必须出一条带 kind 的提示记录');
  assert.equal(event.text, shown[0].text, '界面那条与结构化那条必须是同一句');
  // **措辞纪律**：不写「别的引用不受影响」——**已裁定：该语义在第 14 码下成立**
  // （各引用各自落盘、互不牵连），但**文案按 PM 裁定继续回避**（用户此刻只有
  // 一个下一步＝移除这条太长的引用）⇒ 断言照旧：文案里不许出现这句话。
  // 也不写「重试／再试一次」（这个码**不可重试**）、不把错误码显示给用户
  // （码只进 `detail`）。
  assert.doesNotMatch(
    shown[0].text,
    /不受影响|重试|再试一次|body_budget_exceeded/,
    `第 14 码文案不得出现这些表述：${shown[0].text}`,
  );
});

test('其它码（如 body_too_large）⇒ 现有固定串一字不变（不因第 14 码改口径）', async () => {
  const { api, ctx, registered, notices } = loadClient({
    config: { readDelayMs: 10 },
    captureFailure: { code: 'body_too_large', detail: '正文超限：300000 字节' },
  });
  api.apply(ctx);
  const source = registered[0];
  const { ref } = pick(source);
  await new Promise((r) => setTimeout(r, 80));
  const shown = notices.filter((n) => /正文读取失败/.test(String(n?.text ?? '')));
  assert.equal(shown.length, 1, `失败应恰好一条固定串：${JSON.stringify(notices)}`);
  // **固定串逐字不变**（其它码的既有口径一个字都不许动）。
  assert.equal(
    shown[0].text,
    `G1：引用「${ref}」的正文读取失败，发送时会拒绝这次提交`,
    `其它码的固定串必须一字不变：${shown[0].text}`,
  );
  // **档位也不变**：其它码仍是 `error` 档 ⇒ 第 14 码的改法不许顺带改到它们。
  assert.equal(shown[0].level, 'error', '其它码的档位必须不变');
  // **不许落第 14 码的专属文案**。
  assert.equal(
    notices.filter((n) => /这条引用太长/.test(String(n?.text ?? ''))).length,
    0,
    `其它码不得落第 14 码的文案：${JSON.stringify(notices)}`,
  );
});

test('第 14 码的 detail 取不到三个数 ⇒ 降级文案（不编数字）', async () => {
  // **三种取不到的样子**：根本没有 detail｜只有原始码（存储层失败那条路就
  // 是这样，`host/commands.js` 把 `detail` 设成原始码）｜形态不合约。
  for (const detail of [null, 'body_budget_exceeded', '单条正文超单条正文预算：正文 abc 字节']) {
    const { api, ctx, registered, notices } = loadClient({
      config: { readDelayMs: 10 },
      captureFailure: { code: 'body_budget_exceeded', detail },
    });
    api.apply(ctx);
    const source = registered[0];
    pick(source);
    await new Promise((r) => setTimeout(r, 80));
    const shown = notices.filter((n) => /这条引用太长/.test(String(n?.text ?? '')));
    assert.equal(shown.length, 1, `降级文案应恰好一条：${JSON.stringify(notices)}`);
    // **降级文案逐字照抄定稿原文**。
    assert.equal(
      shown[0].text,
      '这条引用太长，请移除它，换一段更短的正文。',
      `降级文案必须逐字等于定稿原文：${shown[0].text}`,
    );
    // **不许编数字**。
    assert.doesNotMatch(shown[0].text, /\d/, `降级文案不得出现数字：${shown[0].text}`);
    // **降级也必须是 `info` 档**（常驻条，不是瞬态 toast）。
    assert.equal(shown[0].level, 'info', '降级文案也必须是 info 档');
  }
});
