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
import { readFileSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
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
  BODY_SOURCE_PENDING_DETAIL,
} from '../host/protocol.js';

// **壳侧那半边的真函数**（PM 2026-10-22 14:2x 派单：短码与 `site` 那一格）。
//
// · `parseSite` —— **形态校验**（认不出的短码 ⇒ 请求体**不带** `site`）；
// · `buildRequestBody` / `buildBodyRequestBody` —— **请求体真的长什么样**。
//
// ⇒ ⇒ 客户端用的短码必须**过得了 `parseSite`**，而「哪几个站是真的」由壳侧
// Rust 那三个常量定（下面那条用例做**双向对账**）⇒ ⇒ **不许自造短码**。
import { parseSite, buildRequestBody, buildBodyRequestBody, createSiteSource } from '../host/site-source.js';

// **host 那半边的真装配**（PM 2026-10-22 14:5x 收尾单）：「切站之后**不串缓存**」
// 这件事**只有在真目录缓存逻辑下**才测得出来（假 host 桩没有缓存 ⇒ ⇒ 测了也
// 测不到）⇒ ⇒ 这里把**真** `createCommands` ＋ **真** `createCatalogCache` 接到
// 客户端的 fetch 桩上。
import { createCommands, createCatalogCache } from '../host/commands.js';

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
 * @param {object|null} [options.capturePartial] - 让桩的 `capture` 端点**成功但
 *   带省略事实**（造「只带出了部分正文」那一档，PM 2026-10-22 08:3x 裁定 ·
 *   档 D ①）：`{omitted: [{kind, count}]}`。**形态照协议 P1 §1.1**（`captureResult`
 *   ＝ `{status:'partial', omitted, unsupported}`）⇒ ⇒ 桩给的是**结构**，不是
 *   文案 ⇒ ⇒ 界面那句「另有 N 条内容没能带进来」的 N 由客户端**自己算**。
 * @param {object|null} [options.catalogFailure] - 让桩的 `catalog` 端点**失败**
 *   （造「目录失败 ⇒ 按码给成品文案」的场景）：`{code}`。
 * @param {Function|null} [options.realHost] - **真 host 半边**的命令实现
 *   （`(endpoint, payload) => Promise<value>`）：给了就**转发**给它，不走下面的
 *   桩 ⇒ ⇒ 「切站之后不串缓存」那条用例要靠它（**真**目录缓存 ＋ **真**
 *   `site-source.js`）。
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
  capturePartial = null,
  pageSource = undefined,
  catalogFailure = null,
  realHost = null,
} = {}) {
  const registered = [];
  const effects = [];
  const bailCalls = [];
  const notices = [];
  const snapshots = new Map();
  const rpcCalls = [];
  /** **目录查询**的载荷（协议 §4.6 修法 3：`refresh` 那一栏是重点）。 */
  const catalogCalls = [];
  /** **正文取数**的载荷（PM 2026-10-22 14:2x：`site` 那一栏是重点）。 */
  const captureCalls = [];
  /** **读盘**（引用详情 / 发送前那一趟）的载荷。 */
  const previewCalls = [];
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
    // **记下这一次的载荷**（`refresh` 那一栏是「用户刚打开候选组」的判据；
    // `site` 那一栏是「这次取的是哪个站」——PM 2026-10-22 14:2x 派单）。
    //
    // ⚠️ **真 host 模式也记**（先记、再转发）⇒ ⇒ 那条端到端用例才读得到载荷。
    if (endpoint === 'catalog') {
      catalogCalls.push({
        query: String(payload?.query ?? ''),
        refresh: payload?.refresh === true,
        // **没带这个键时是 `undefined`** ⇒ ⇒ 「未选站 ⇒ 请求体不带这个键」
        // 这条能被直接读出来。
        site: payload?.site,
        hasSite: Object.prototype.hasOwnProperty.call(payload ?? {}, 'site'),
      });
    }
    // **真 host 半边**（PM 2026-10-22 14:5x 收尾单）：不走下面那些桩 ⇒ ⇒ 真目录
    // 缓存、真 `site-source.js`、真协议校验全都在这条路上。
    if (realHost !== null) return realHost(endpoint, payload);
    if (endpoint === 'capabilities') return { ok: true, usedSeqs: {} };
    if (endpoint === 'catalog') {
      // **造目录失败**（造「按码给成品文案」的场景）：`catalogFailure` 给了就
      // 照它失败（`{code}`；不给 `code` 就是 RPC 层抛错那条路）。
      if (catalogFailure !== null) {
        const error = new Error(String(catalogFailure.message ?? `桩注入的失败：${String(catalogFailure.code)}`));
        error.code = String(catalogFailure.code);
        if (catalogFailure.detail !== undefined) {
          error.detail = catalogFailure.detail;
        }
        throw error;
      }
      // **目录与 id 分配都在 host**（协议 v0.2 S-1）：客户端这一侧没有目录。
      //
      // 目录项的键面照协议 P4 §2.4 面二 / §4.3 ②（**恰好 6 个键**）：会话 id 的
      // 键名是 **`id`**（旧形态那个 `sessionId` 键已经换掉了）⇒ ⇒ 客户端读
      // `item.id`，读旧键名会拿到**空串**（那是一条会「看起来正常」的坏链）。
      // ⚠️ **按站给不同的目录**（PM 2026-10-22 14:2x：切站之后候选组必须换成
      // 那站的会话）⇒ ⇒ 桩按 `payload.site` 分流 ⇒ ⇒ 切站前后**条数与条目 id
      // 都不同**（可读数）。
      const site = String(payload?.site ?? '');
      const preset = SITE_FIXTURES[site] ?? null;
      const items = preset === null
        ? Object.entries(FIXTURE_BODIES).map(([sessionId, body]) => ({
          id: sessionId,
          title: sessionId === 'g1-alpha' ? ALPHA : BETA,
          title_type: 'USER',
          model_type: 'deepseek-chat',
          pinned: false,
          updated_at: 0,
          body,
        }))
        : preset.items.map((item) => ({
          ...item,
          title_type: 'USER',
          model_type: 'deepseek-chat',
          pinned: false,
          updated_at: 0,
          body: FIXTURE_BODIES[item.id] ?? '',
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
          // **来源标识**（可注入）：默认**没有**（旧缓存页那样）⇒ 客户端退旧
          // 文案；注入了 ⇒ 客户端按它显示。
          //
          // ⚠️ **没注入时按站给**（照 host 侧那现实：`SITE_LABELS` 只有
          // `deepseek` 一条 ⇒ ⇒ 另两站的 `label` 是**小写短码**）⇒ ⇒ 客户端
          // 该自己给品牌名（**短码不直接上屏**）。
          ...(pageSource === undefined
            ? (preset === null ? {} : { source: preset.source })
            : { source: pageSource }),
        },
        allocations,
        cached: false,
      };
    }
    if (endpoint === 'capture') {
      const sessionId = String(payload?.sessionId ?? '');
      const snapshotId = String(payload?.snapshotId ?? '');
      // **记下这一次的载荷**（`site` 那一栏＝**这条引用当初那个站**，不是当前
      // 站——PM 2026-10-22 14:2x）。
      captureCalls.push({
        sessionId,
        snapshotId,
        site: payload?.site,
        hasSite: Object.prototype.hasOwnProperty.call(payload ?? {}, 'site'),
      });
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
      // **「只带出了部分正文」场景**（PM 2026-10-22 08:3x 裁定 · 档 D ①）：协议
      // P1 §1.1 的 `captureResult` ＝ `{status, omitted, unsupported}`；`omitted`
      // ＝ `[{kind, count}]` ⇒ ⇒ 桩照那个形态给，**不手抄文案**。
      const captureResult =
        capturePartial === null
          ? { status: 'ok', omitted: [], unsupported: [] }
          : {
            status: 'partial',
            omitted: capturePartial.omitted,
            unsupported: capturePartial.unsupported ?? [],
          };
      const snapshot = {
        schemaVersion: 1,
        snapshotId,
        providerId: PROVIDER_SHORT,
        accountScope: '',
        sessionId,
        branch: 'main',
        messages: [{ role: 'user', text: body }],
        contentDigest: 'stub-digest',
        captureResult,
        capturedAt: '',
        sourceUpdatedAt: '',
        sourceFingerprint: '',
      };
      snapshots.set(snapshotId, { snapshot, body });
      return { ok: true, snapshot };
    }
    if (endpoint === 'preview') {
      // **记下这一次的载荷**（口径一致：这一处也带 `site`）。
      previewCalls.push({
        snapshotId: String(payload?.snapshotId ?? ''),
        site: payload?.site,
        hasSite: Object.prototype.hasOwnProperty.call(payload ?? {}, 'site'),
      });
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
  return { api, ctx, registered, effects, bailCalls, notices, sandbox, snapshots, rpcCalls, catalogCalls, captureCalls, previewCalls };
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

/**
 * 取「会话」那些候选行（**排除「来源站」那几行**）。
 *
 * @param {object[]} rows - 候选行。
 * @returns {object[]}
 */
function sessionRows(rows) {
  return rows.filter((row) => row.section === 'Web Chat');
}

/**
 * 取「来源站」那几行（PM 2026-10-22 14:2x 派单新增的那个分组）。
 *
 * @param {object[]} rows - 候选行。
 * @returns {object[]}
 */
function siteRowsOf(rows) {
  return rows.filter((row) => row.section === '来源站');
}

/**
 * 取「为什么列不出」那一行说明（`unavailableRow`）。
 *
 * ⚠️ **它不在 `rows[0]`**：目录取数失败时**站行在前、说明行在后**（PM 2026-10-22
 * 15:2x 派单）⇒ ⇒ 站行不能把说明行挤掉，但说明行也不在第一行 ⇒ ⇒ 失败路径
 * 的用例一律**按它取**，不许按 `rows[0]` 取。
 *
 * @param {object[]} rows - 候选行。
 * @returns {object|undefined} 那一行说明（没有就是 undefined）。
 */
function noticeRow(rows) {
  return rows.find((row) => row.section === 'Web Chat');
}

/** 夹具正文表（host 侧的桩按官网会话 id 取正文）。 */
const FIXTURE_BODIES = { 'g1-alpha': BODY_ALPHA, 'g1-beta': BODY_BETA };

/**
 * **按站分的固定目录**（桩用；PM 2026-10-22 14:2x 派单）。
 *
 * **为什么要按站给不同的东西**：切站之后**候选组必须换成那站的会话** ⇒ ⇒ 桩
 * 若三个站给同一份目录，这条要求就**证不出来** ⇒ ⇒ 这里让两个站给**条数与
 * 条目 id 都不同**的目录。
 *
 * ⚠️ **`label` 故意给成小写短码**：照 host 侧那现实（`SITE_LABELS` 只有
 * `deepseek` 一条 ⇒ 另两站回退成短码）⇒ ⇒ 客户端该自己给品牌名（**短码不
 * 直接上屏**）。
 */
const SITE_FIXTURES = {
  minimax: {
    source: { kind: 'site', id: 'minimax', label: 'minimax' },
    items: [
      { id: 'mm-1', title: 'MM 会话一' },
      { id: 'mm-2', title: 'MM 会话二' },
      { id: 'mm-3', title: 'MM 会话三' },
    ],
  },
  chatgpt: {
    source: { kind: 'site', id: 'chatgpt', label: 'chatgpt' },
    items: [{ id: 'cg-1', title: 'CG 会话一' }],
  },
  // ⚠️ **DeepSeek 那一页**（真实 host 的默认站）：**真实源回包总有 `source`** ⇒ ⇒
  // 这一页**认得出** ⇒ ⇒ 按那页记的站就是 `deepseek`（PM 2026-10-22 14:35
  // 复核第 1 条）。
  deepseek: {
    source: { kind: 'site', id: 'deepseek', label: 'DeepSeek' },
    items: [
      { id: 'g1-alpha', title: ALPHA },
      { id: 'g1-beta', title: BETA },
    ],
  },
};

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
  // **落定后不主动弹**（PM 2026-10-22 08:4x 裁定：成功那句「已取到」**只在点开
  // 引用详情时**报 ⇒ ⇒ pick 成功时一句都不出）⇒ ⇒ 此刻 notices 里**仍然只有**
  // 上面那条「读取中」。
  assert.equal(
    notices.filter((n) => /读取中|尚未就绪/.test(String(n?.text ?? ''))).length,
    1,
    `读取落定后不该冒出成功提示：${JSON.stringify(notices)}`,
  );
  assert.equal(source.openReference({ sessionId: 's1' }, { ref, appearance: 'session' }), true);
  await new Promise((r) => setTimeout(r, 20));
  const texts = notices.map((n) => String(n?.text ?? ''));
  // **档 A 逐字**（PM 2026-10-22 08:4x 裁定的成功句）。
  assert.ok(
    texts.includes(`引用「${ref}」的正文已取到，发送时会一起带上。`),
    `详情那条必须是档 A 成品文案：${JSON.stringify(notices)}`,
  );
  // **不许说取全了**（协议 §9 **G4-23**：取到的正文是不是全部**尚未坐实**）⇒ ⇒
  // 「全部」「都取到」这类完整性说法一律不许出现。
  assert.ok(
    texts.every((t) => !/全部|都取到/.test(t)),
    `不许承诺取全了：${JSON.stringify(notices)}`,
  );
  // **不许说「已就绪」**（pending ≠ ready）。
  assert.ok(
    texts.every((t) => !/已就绪/.test(t)),
    `不许出现「已就绪」：${JSON.stringify(notices)}`,
  );
});

test('读取落定**不主动弹**（成功那句只在点开引用详情时报）', async () => {
  // **PM 2026-10-22 08:4x 裁定改了这一条的口径**：读取落定时**不再**弹提示。
  //
  // **为什么**：chip 出现本身就是信号（用户看得见引用被选进去了）⇒ ⇒ 再弹一句
  // 「已取到」是**重复同一个事实**；而「用户还没点开详情」时，界面里没有任何
  // 动作会因为这条提示变得可做 ⇒ ⇒ 不许拿它当「下一步」。
  //
  // **但「有内容没带进来」必须当场说**（那一档另有用例钉着：partial 才弹）。
  const { api, ctx, registered, notices } = loadClient({ config: { readDelayMs: 30 } });
  api.apply(ctx);
  const { ref } = pick(registered[0]);
  assert.equal(notices.length, 0, '读取还没落定时不该有提示');
  await new Promise((r) => setTimeout(r, 90));
  // **全取到时一条都不出**（不许拿成功当提示刷屏）。
  assert.equal(
    notices.length,
    0,
    `读取成功不该主动弹提示：${JSON.stringify(notices)}`,
  );
  // **点开引用详情才报成功那句**（这一条是用户自己点出来的，不是系统塞的）。
  registered[0].openReference({ sessionId: 's1' }, { ref, appearance: 'session' });
  await new Promise((r) => setTimeout(r, 40));
  assert.equal(
    notices.filter((n) => /已取到/.test(String(n?.text ?? ''))).length,
    1,
    `点开详情必须报一条成功句：${JSON.stringify(notices)}`,
  );
  assert.equal(notices[0].level, 'info');
  assert.equal(
    notices[0].text,
    `引用「${ref}」的正文已取到，发送时会一起带上。`,
    `成功句必须逐字等于档 A 定稿原文：${notices[0].text}`,
  );
});

test('读取失败也要让用户看到（error 级），不静默', async () => {
  const { api, ctx, registered, notices } = loadClient({ config: { readDelayMs: 10, failRead: true } });
  api.apply(ctx);
  const { ref } = pick(registered[0]);
  await new Promise((r) => setTimeout(r, 60));
  assert.equal(notices.length, 1, '读取失败必须立刻让用户看到');
  assert.equal(notices[0].level, 'error');
  // **档 B 兜底逐字**（这一档的失败**没有码** ⇒ 走兜底：只说事实、不指原因、
  // 不给动作）⇒ ⇒ 仍然**带 `ref`** ＋ **说清后果**（PM 09:0x／09:1x 裁定）。
  assert.equal(
    notices[0].text,
    `引用「${ref}」带不出正文。发送时会拒绝这次提交。`,
    `失败文案必须逐字等于档 B 兜底原文：${notices[0].text}`,
  );
});

test('目录行的描述按 host 给的**来源标识**显示（`page.source.label`）', async () => {
  // 真实源 ⇒ 显示站点名。
  const real = loadClient({
    pageSource: { kind: 'site', id: 'deepseek', label: 'DeepSeek' },
  });
  real.api.apply(real.ctx);
  const rows = await real.registered[0].candidates({ sessionId: 's1' }, { query: '' });
  assert.ok(rows.length > 0, '目录页得有候选行');
  // ⚠️ **断的是「会话」那几行**（PM 2026-10-22 14:2x 派单新增了「来源站」那个
  // 分组 ⇒ ⇒ 菜单里现在有两组行）。
  const first = sessionRows(rows)[0];
  assert.match(first.description, /DeepSeek/, `描述必须显示来源名：${first.description}`);
  // **逐字**：真实源**只显示来源名**（izzy 2026-10-21 口径：「DeepSeek 就显示
  // DeepSeek」）⇒ ⇒ **不加「· 官网对话」那半句**（那是同义反复：插件整个功能
  // 就是官网对话、分组名已经是「Web Chat」）⇒ 描述列只留**带新信息**的半句。
  assert.equal(
    first.description,
    'DeepSeek',
    `真实源的描述必须逐字等于来源名：${first.description}`,
  );

  // 夹具 ⇒ 来源名 ＋「G1 验证用」（**带新信息**：这是测试数据、别当真）。
  const fixture = loadClient({
    pageSource: { kind: 'fixture', id: 'fixture', label: '固定夹具' },
  });
  fixture.api.apply(fixture.ctx);
  const fixtureRows = await fixture.registered[0].candidates({ sessionId: 's1' }, { query: '' });
  assert.equal(
    sessionRows(fixtureRows)[0].description,
    '固定夹具 · G1 验证用',
    `夹具的描述必须逐字等于定稿原文：${sessionRows(fixtureRows)[0].description}`,
  );

  // 旧缓存页没有 `source` ⇒ 退旧文案（**不假装知道来源**）。
  const legacy = loadClient();
  legacy.api.apply(legacy.ctx);
  const legacyRows = await legacy.registered[0].candidates({ sessionId: 's1' }, { query: '' });
  assert.match(sessionRows(legacyRows)[0].description, /固定夹具/, '没有来源标识时退旧文案');
});

test('catalog 失败 ⇒ 按码给**成品文案**（不显示错误码、不露内部词）', async () => {
  const { api, ctx, registered } = loadClient({
    catalogFailure: { code: 'source_unsupported' },
  });
  api.apply(ctx);
  const rows = await registered[0].candidates({ sessionId: 's1' }, { query: '' });
  // **目录失败时也要给那一行说明**（不静默）⇒ ⇒ 它在**站行之后**（不是 `rows[0]`）。
  const notice = noticeRow(rows);
  assert.ok(notice, `目录失败时也要给一行说明（不静默）：${JSON.stringify(rows)}`);
  // **成品文案逐字照抄定稿原文**。
  assert.equal(
    notice.description,
    '这个来源现在列不出会话：请到官网窗口复制那段对话。',
    `目录失败的文案必须逐字等于定稿原文：${notice.description}`,
  );
  // **不许显示错误码**（`source_unsupported` 这种英文码用户看不懂、也没法照着做）。
  assert.doesNotMatch(
    notice.description,
    /source_unsupported|[a-z_]{6,}/,
    `文案不得显示错误码：${notice.description}`,
  );
  // **不许露内部词「host」**（那是插件与 host 半边之间的内部叫法）。
  assert.doesNotMatch(
    notice.description,
    /host/i,
    `文案不得露内部词：${notice.description}`,
  );
});

test('catalog 失败 ⇒ 不同码给**不同**文案（不串、不共用一句）', async () => {
  const cases = [
    ['not_logged_in', '还没在官网窗口登录，登录后才能列出会话。'],
    ['account_changed', '登录的账号变了，重新打开官网窗口再列一次。'],
    ['schema_changed', '官网的页面结构变了，升级插件后才能列出会话。'],
    ['timeout', '会话列表没在时间内读完，重新打开 @ 菜单再试。'],
    ['catalog_empty', '这个来源没有会话，换个关键词试试。'],
    ['catalog_failed', '会话列表这次没取到，重新打开 @ 菜单再试。'],
  ];
  for (const [code, expected] of cases) {
    const { api, ctx, registered } = loadClient({ catalogFailure: { code } });
    api.apply(ctx);
    const rows = await registered[0].candidates({ sessionId: 's1' }, { query: '' });
    const notice = noticeRow(rows);
    assert.ok(notice, `${code} 那一档也要给一行说明：${JSON.stringify(rows)}`);
    assert.equal(
      notice.description,
      expected,
      `${code} 的文案必须逐字等于定稿原文：${notice.description}`,
    );
  }
});

test('catalog 失败 ⇒ 认不出的码走**通用兜底**（不显示码、不猜它是什么）', async () => {
  const { api, ctx, registered } = loadClient({
    catalogFailure: { code: 'some_future_code' },
  });
  api.apply(ctx);
  const rows = await registered[0].candidates({ sessionId: 's1' }, { query: '' });
  const notice = noticeRow(rows);
  assert.ok(notice, `未知码那一档也要给一行说明：${JSON.stringify(rows)}`);
  assert.equal(
    notice.description,
    '会话列表这次没取到，重新打开 @ 菜单再试。',
    `未知码必须走通用兜底：${notice.description}`,
  );
  assert.doesNotMatch(
    notice.description,
    /some_future_code/,
    `未知码不得显示出来：${notice.description}`,
  );
});

test('**正文这一格还没接通** ⇒ 专属文案（说清「没接通」，不装成成功，也不说成「读取失败」）', async () => {
  const { api, ctx, registered, notices } = loadClient({
    config: { readDelayMs: 10 },
    captureFailure: {
      code: 'source_unsupported',
      // **夹具用 host 的真产出**，不手抄 —— 手抄的那串只证明「我抄对了」，
      // 证明不了**客户端吃得住真 host 的形状** ⇒ host 那边句首的机器标记一改
      // （`body-source-pending:`），手抄的那份就静默对不上 ⇒ ⇒ 专属文案退成
      // 通用失败文案，而端侧用例照样绿。
      detail: BODY_SOURCE_PENDING_DETAIL,
      message: '正文取得机制尚未坐实',
    },
  });
  api.apply(ctx);
  registered[0].onPick({
    candidate: candidate('g1-alpha', ALPHA),
    session: { sessionId: 's1' },
    span: { start: 0, end: 1, draftRev: 7 },
  });
  await new Promise((r) => setTimeout(r, 80));
  const texts = notices.map((n) => String(n?.text ?? ''));
  assert.ok(texts.length >= 1, `必须有提示：${JSON.stringify(notices)}`);
  assert.ok(
    texts.some((t) => /还没接通/.test(t)),
    `必须说清「这一格还没接通」：${JSON.stringify(notices)}`,
  );
  assert.ok(
    texts.every((t) => !/读取失败/.test(t)),
    `不许说成「读取失败」（那像 bug）：${JSON.stringify(notices)}`,
  );
  assert.ok(
    texts.every((t) => !/已取到|就绪/.test(t)),
    `不许装成成功：${JSON.stringify(notices)}`,
  );
  // **成品文案逐字照抄定稿原文**（UX 定稿 2026-10-21）⇒ 实现侧照抄。
  assert.ok(
    texts.includes('这条引用带不出正文：目录页能列出会话，但正文读取还没接通。'),
    `文案必须逐字等于定稿原文：${JSON.stringify(notices)}`,
  );
  // **不做时间承诺**：不写「暂时」——「暂时」暗示「等会儿会好」，而这一格在前置
  // （G4-20）坐实前**不会自己好** ⇒ ⇒ 只陈述现状。
  assert.ok(
    texts.every((t) => !/暂时/.test(t)),
    `不许做时间承诺：${JSON.stringify(notices)}`,
  );
  // **不拼 host 的 `detail` 原文**：那是给日志与排查看的线索（含协议节号与前置
  // 编号）⇒ ⇒ 界面文案自己写完整、自包含的事实。
  assert.ok(
    texts.every((t) => !/§4\.4|G4-20|协议/.test(t)),
    `界面文案不得出现内部术语与编号：${JSON.stringify(notices)}`,
  );
});

test('别的 `source_unsupported` 场景 ⇒ **不许**挂「正文还没接通」那条（不按码一刀切）', async () => {
  // **`source_unsupported` 在 host 侧有七个产出点**（目录里没有这个会话／真实源
  // 取数源没接上／pick 序号用尽／取数桥没起／捕获源明说不支持／未知端点兜底…）
  // ⇒ ⇒ 只看码就出「正文还没接通」，在别的场景里那是**假陈述**（用户会以为正文
  // 通道坏了，其实不是）⇒ ⇒ 客户端按 host 那句**标记**再断一道。
  const { api, ctx, registered, notices } = loadClient({
    config: { readDelayMs: 10 },
    captureFailure: {
      code: 'source_unsupported',
      detail: '目录里没有这个会话',
      message: '目录里没有这个会话',
    },
  });
  api.apply(ctx);
  const { ref } = pick(registered[0]);
  await new Promise((r) => setTimeout(r, 80));
  const texts = notices.map((n) => String(n?.text ?? ''));
  assert.ok(
    texts.every((t) => !/还没接通/.test(t)),
    `别的场景不许挂「正文还没接通」那条：${JSON.stringify(notices)}`,
  );
  // **失配是安全的** ⇒ 退回通用失败文案（现状），**不会说错**。
  //
  // **逐字＝档 B 兜底**（PM 2026-10-22 08:4x 裁定的档 B）：只说事实、不指原因、
  // 不给动作 ⇒ ⇒ **不是**旧那条固定串（「正文读取失败，发送时会拒绝这次提交」）。
  assert.ok(
    texts.includes(`引用「${ref}」带不出正文。发送时会拒绝这次提交。`),
    `标记对不上就退回通用失败文案：${JSON.stringify(notices)}`,
  );
});

test('读取失败 → openReference 报 error（不是假装还悬着）', async () => {
  const { api, ctx, registered, notices } = loadClient({ config: { readDelayMs: 10, failRead: true } });
  api.apply(ctx);
  const source = registered[0];
  const { ref } = pick(source);
  await new Promise((r) => setTimeout(r, 60));
  source.openReference({ sessionId: 's1' }, { ref, appearance: 'session' });
  // 落定那条 + 详情那条，**两条都必须是 error 级**——失败被降级成 info 或含糊
  // 措辞，用户会以为还能发。
  assert.ok(notices.length >= 2, `落定与详情都该有提示：${JSON.stringify(notices)}`);
  assert.ok(
    notices.every((n) => n.level === 'error'),
    `失败提示必须是 error 级：${JSON.stringify(notices)}`,
  );
  // **两条说的是两件事**（PM 2026-10-22 08:4x 裁定）：落定那条＝**这一档的成品
  // 文案**（档 B 兜底：只说事实）；详情那条＝**这个句柄的状态**（读取失败 ⇒
  // 发送会拒绝这次提交）⇒ ⇒ 逐字各钉一句。
  assert.equal(
    notices[0].text,
    `引用「${ref}」带不出正文。发送时会拒绝这次提交。`,
    `落定那条必须是档 B 兜底原文：${notices[0].text}`,
  );
  assert.equal(
    notices[1].text,
    `引用「${ref}」的正文读取失败，发送时会拒绝这次提交`,
    `详情那条必须逐字等于句柄状态文案：${notices[1].text}`,
  );
});

// ─────────────────────────────────────────────────────────────────────────────
// PM 2026-10-22 08:3x／08:4x 裁定的**单会话正文文案口径**（档 B／档 D ①）。
//
// **为什么逐字钉**：这些句子是 PM 裁定的定稿原文 ⇒ ⇒ 改字＝改口径 ⇒ ⇒ 用例
// 必须跟着红。**不许**用「包含某个片段」这种松断言 —— 那样换个说法照样绿。
// ─────────────────────────────────────────────────────────────────────────────

/**
 * 档 B「取不到」的**每一档**（PM 2026-10-22 08:5x 改动单 ＋ PM 09:0x 回改单）
 * —— 逐字期望。
 *
 * ## `text` 里的 `{ref}` 是**这条引用的句柄**
 *
 * 界面文案**每档都带 `ref`**（PM 09:0x 回改单：内核 notice 是**单槽** ⇒ ⇒ 用户
 * 同时引了多条、其中一条失败 ⇒ 他看到的是**哪一条**失败？⇒ ⇒ 必须带 `ref`）⇒ ⇒
 * 逐字期望里留一个 `{ref}` 占位 ⇒ ⇒ 用例里替换成真句柄再逐字比。
 *
 * ## 失败那半边现在是 **7 支**
 *
 * `schema_changed`｜`catalog_failed`｜`catalog_empty`｜`timeout`｜`body_too_large`
 * ｜`partial_content`｜兜底（认不出的码）。
 *
 * ⚠️ **PM 09:1x 裁定**：「回程失败」与「回程超时」**合成一句**（`timeout` 一支）
 * ⇒ ⇒ 客户端**不再**依赖实现侧那串中文措辞（那是给日志排查看的线索、不是稳定契
 * 约）⇒ ⇒ 那条「核上游措辞还在」的守卫用例也一并去掉。
 */
const BODY_FETCH_TIERS = [
  {
    name: '形状漂移',
    code: 'schema_changed',
    detail: '页面结构与协议 §4.4 不符：认不出站点的正文消息数组',
    text: '引用「{ref}」带不出正文：官网返回的内容读不懂，升级插件后才能再引用。发送时会拒绝这次提交。',
  },
  {
    name: '站内码',
    code: 'catalog_failed',
    detail: '站内码 40003 INVALID_TOKEN',
    text: '引用「{ref}」带不出正文：官网拒绝了这次读取。发送时会拒绝这次提交。',
  },
  {
    name: '站点返回空',
    code: 'catalog_empty',
    detail: '站点返回了空正文（接口正常，但这条会话没有消息）',
    text: '引用「{ref}」带不出正文：这条会话没有内容。发送时会拒绝这次提交。',
  },
  {
    name: '没取到（回程失败与回程超时合成一句）',
    code: 'timeout',
    detail: '回程失败：传输层状态码 500（协议 §4.2 的成功判据要求 200）',
    text: '引用「{ref}」带不出正文：官网没有返回这条会话的正文，可以再选一次这条会话。发送时会拒绝这次提交。',
  },
  {
    name: '正文太长',
    code: 'body_too_large',
    detail: '正文超限：300000 字节',
    text: '引用「{ref}」带不出正文：这条会话的正文太长，换一条短一些的会话再试。发送时会拒绝这次提交。',
  },
  {
    name: '内容被省略（若它以错误码出现）',
    code: 'partial_content',
    detail: '捕获失败原因：partial_content',
    text: '引用「{ref}」只带出了部分正文，另有内容没能带进来。发送时会拒绝这次提交。',
  },
  {
    name: '兜底（认不出的码）',
    code: 'some_unknown_code',
    detail: '捕获失败，原因未明',
    text: '引用「{ref}」带不出正文。发送时会拒绝这次提交。',
  },
];

test('档 B「取不到」按码给成品文案（每档逐字，且**都带 ref**）', async () => {
  // **码从哪来**：实现侧把正文那条链的失败归一到闭集码（`capture_code()`：回程
  // 失败／超时 ⇒ `timeout`｜站内码 ⇒ `catalog_failed`｜形状漂移 ⇒
  // `schema_changed`｜站点返回空 ⇒ `catalog_empty`）⇒ ⇒ **客户端只看得到码**，
  // 成因在 `detail` 里、**不进界面** ⇒ ⇒ 这一档必须**按码分流**。
  for (const tier of BODY_FETCH_TIERS) {
    const { api, ctx, registered, notices } = loadClient({
      config: { readDelayMs: 10 },
      captureFailure: { code: tier.code, detail: tier.detail },
    });
    api.apply(ctx);
    const { ref } = pick(registered[0]);
    await new Promise((r) => setTimeout(r, 80));
    assert.equal(
      notices.length,
      1,
      `「${tier.name}」应恰好一条提示：${JSON.stringify(notices)}`,
    );
    // **失败必须显眼**（正文没取到 ⇒ 发送会被拒）⇒ ⇒ `error` 档。
    assert.equal(notices[0].level, 'error', `「${tier.name}」必须是 error 档`);
    // **逐字**：整句（含 `ref` 与句末那句后果）都必须一字不差。
    assert.equal(
      notices[0].text,
      tier.text.replace('{ref}', ref),
      `「${tier.name}」的文案必须逐字等于定稿原文：${notices[0].text}`,
    );
    // **每档都要说清后果**（PM 09:0x 裁定 ＋ PM 09:1x 再确认：载体是 `error` 档，
    // 文案要自洽 ⇒ ⇒ 用户看完就知道发不出去）⇒ ⇒ 后果句**每档都真**、**不是一
    // 刀切的动作**。
    assert.ok(
      notices[0].text.includes('发送时会拒绝这次提交'),
      `「${tier.name}」必须说清「发送时会拒绝这次提交」：${notices[0].text}`,
    );
  }
});

test('档 B：`timeout` 那一档**不因线索措辞变化而变**（不再依赖那串中文）', async () => {
  // **为什么要有这条**：PM 09:1x 裁定「回程失败」与「回程超时」**合成一句**⇒ ⇒
  // 客户端**不再**看 `detail` 的措辞⇒ ⇒ 那串中文**改了也不会**影响界面⇒ ⇒ 这
  // 条把「不依赖」这件事钉住：换任何线索，出来的句子**一模一样**。
  const seen = new Set();
  for (const detail of [
    undefined,
    '回程失败：传输层状态码 500（协议 §4.2 的成功判据要求 200）',
    '还没抓到站点的会话正文（可重试）：等站点那条响应加载完没等到',
    '捕获失败，原因未明',
  ]) {
    const { api, ctx, registered, notices } = loadClient({
      config: { readDelayMs: 10 },
      captureFailure: { code: 'timeout', detail },
    });
    api.apply(ctx);
    pick(registered[0]);
    await new Promise((r) => setTimeout(r, 80));
    assert.equal(notices.length, 1, `应恰好一条提示：${JSON.stringify(notices)}`);
    seen.add(notices[0].text.replace(/引用「[^」]*」/, '引用「ref」'));
  }
  assert.equal(
    seen.size,
    1,
    `线索措辞变了也不许改文案（PM 09:1x：合成一句）：${JSON.stringify([...seen])}`,
  );
  // **那句话里不许出现「在时间内」**（那半句随 B5 一起撤了）⇒ ⇒ 也不许把断连
  // 说成超时。
  assert.ok(
    ![...seen][0].includes('在时间内'),
    `这一档不许说成超时：${[...seen][0]}`,
  );
});

test('档 B 的每一档都**不许**落一刀切的尾巴（动作逐档给）', async () => {
  // **为什么**：一刀切（「重新打开 @ 菜单再试」对每一档都套）几乎总是错的 ——
  // 站内码、目录空、正文超长这三档，用户做完那个动作**改不了结果**⇒ ⇒ 不许给。
  for (const tier of BODY_FETCH_TIERS) {
    const { api, ctx, registered, notices } = loadClient({
      config: { readDelayMs: 10 },
      captureFailure: { code: tier.code, detail: tier.detail },
    });
    api.apply(ctx);
    pick(registered[0]);
    await new Promise((r) => setTimeout(r, 80));
    const texts = notices.map((n) => String(n?.text ?? ''));
    assert.ok(
      texts.every((t) => !t.includes('重新打开 @ 菜单再试')),
      `「${tier.name}」不许套一刀切的尾巴：${JSON.stringify(notices)}`,
    );
    // **也不许复读详情那条的说法**（「引用「…」的正文读取失败」是**详情那条**说的
    // 句柄状态，不是这一档的原因）⇒ ⇒ 档 B 这一档说的是「**没取到**」。
    //
    // ⚠️ **注意**：句末那句「发送时会拒绝这次提交」是 **PM 09:0x 裁定要补的**
    // （事实后果、每档都真）⇒ ⇒ **不许**把它也当成「复读」禁掉。
    assert.ok(
      texts.every((t) => !t.includes('正文读取失败')),
      `「${tier.name}」不许复读详情那条的说法：${JSON.stringify(notices)}`,
    );
  }
});

test('档 D ①「只带出了部分正文」⇒ 当场说、带条数（partial 才弹）', async () => {
  const { api, ctx, registered, notices } = loadClient({
    config: { readDelayMs: 10 },
    capturePartial: { omitted: [{ kind: 'thinking', count: 3 }] },
  });
  api.apply(ctx);
  const { ref } = pick(registered[0]);
  await new Promise((r) => setTimeout(r, 80));
  assert.equal(notices.length, 1, `partial 应恰好一条提示：${JSON.stringify(notices)}`);
  // **不是失败**（正文还是带出去了、发送照样成功）⇒ ⇒ `info` 档的常驻条，
  // 不是 `error` 档的瞬态 toast。
  assert.equal(notices[0].level, 'info', 'partial 是「省略事实」告知，不是失败');
  // **逐字**（PM 09:0x 回改单：这档**也带 `ref`**——notice 是单槽 ⇒ ⇒ 用户要认得
  // 出是哪一条）⇒ ⇒ 但**不加**「发送时会拒绝这次提交」（这一档是**成功**档）。
  assert.equal(
    notices[0].text,
    `引用「${ref}」带出了这条会话能带的部分正文，另有 3 条内容没能带进来。`,
    `partial 文案必须逐字等于定稿原文：${notices[0].text}`,
  );
  assert.doesNotMatch(
    notices[0].text,
    /发送时会拒绝这次提交/,
    `这一档是成功档：不许说「发送时会拒绝这次提交」：${notices[0].text}`,
  );
  // **不许说取全了**（协议 §9 **G4-23**：取到的正文是不是全部**尚未坐实**）。
  assert.doesNotMatch(
    notices[0].text,
    /全部|都取到/,
    `partial 文案不许承诺取全了：${notices[0].text}`,
  );
});

test('档 D ① 的条数是 **`omitted` 的 count 求和**（不是条目个数）', async () => {
  // **为什么**：协议 P1 §1.1 的 `omitted` ＝ `[{kind, count}]` ⇒ ⇒ 一个条目
  // 可能代表**多条**内容 ⇒ ⇒ N 必须是 count **求和**，拿条目个数会把「缺了 5
  // 条」说成「缺了 2 条」。
  const { api, ctx, registered, notices } = loadClient({
    config: { readDelayMs: 10 },
    capturePartial: {
      omitted: [{ kind: 'thinking', count: 2 }, { kind: '别的类型', count: 3 }],
    },
  });
  api.apply(ctx);
  const { ref } = pick(registered[0]);
  await new Promise((r) => setTimeout(r, 80));
  assert.equal(notices.length, 1, `partial 应恰好一条提示：${JSON.stringify(notices)}`);
  assert.equal(
    notices[0].text,
    `引用「${ref}」带出了这条会话能带的部分正文，另有 5 条内容没能带进来。`,
    `条数必须是 count 求和：${notices[0].text}`,
  );
  // **不点明类型**：kind 是站点的内容类型 ⇒ ⇒ 搬进界面＝把站点概念泄给用户。
  assert.doesNotMatch(
    notices[0].text,
    /thinking|别的类型/,
    `界面文案不点明内容类型：${notices[0].text}`,
  );
});

test('**两条措辞纪律**：用户可见文案不许出现「全部／都取到」「已就绪」', async () => {
  // **纪律一（协议 §9 G4-23 ＋ §4.4 前置段那条同款口径）**：取到的正文是不是
  // **全部**，目前**没有依据** ⇒ ⇒ 用户可见文案里**不许**出现「全部」「都取到」这类
  // 完整性说法。
  //
  // （G4-23 的新措辞有**两档**：「`omitted` 非空」那档**现成可判**、界面**可以**
  // 说「另有 N 条」；「分页没取全」那档**判不出**、界面**不许**说「一部分」⇒ ⇒
  // 这条纪律只管**完整性**那半边，不许把那半边当成「连 N 条也不许说」。）
  //
  // **纪律二（文件头那条）**：pending ≠ ready ⇒ ⇒ **不许**说「已就绪」。
  //
  // **扫法**：把各档都跑一遍，把**所有**用户可见提示收集起来一起扫 ⇒ ⇒ 只扫
  // 某一档，等于让别的档偷偷长出这句话。
  //
  // ⚠️ **覆盖面要对着「实现的分支数」**（PM 09:0x 回改单 ＋ PM 09:1x 补充裁定向）：
  // 失败那半边现在 **7 支**（形状漂移／站内码／空／没取到／正文太长／内容被省略／
  // 兜底）⇒ ⇒ 少一档没出提示，那个档的纪律就**没人守**⇒ ⇒ 条数也**逐字钉住**。
  const collected = [];
  const failedTierTexts = [];

  // ① 成功档（点开引用详情才报）。
  const ok = loadClient({ config: { readDelayMs: 10 } });
  ok.api.apply(ok.ctx);
  const okRef = pick(ok.registered[0]).ref;
  await new Promise((r) => setTimeout(r, 60));
  ok.registered[0].openReference({ sessionId: 's1' }, { ref: okRef, appearance: 'session' });
  await new Promise((r) => setTimeout(r, 30));
  collected.push(...ok.notices);

  // ② 失败各档（**逐档都扫** ⇒ 含 `partial_content` 与兜底那两档）。
  for (const tier of BODY_FETCH_TIERS) {
    const bad = loadClient({
      config: { readDelayMs: 10 },
      captureFailure: { code: tier.code, detail: tier.detail },
    });
    bad.api.apply(bad.ctx);
    pick(bad.registered[0]);
    await new Promise((r) => setTimeout(r, 60));
    collected.push(...bad.notices);
    failedTierTexts.push(...bad.notices.map((n) => String(n?.text ?? '')));
  }

  // ③ partial 那一档（**成功档** ⇒ 不带「发送时会拒绝这次提交」）。
  const part = loadClient({
    config: { readDelayMs: 10 },
    capturePartial: { omitted: [{ kind: 'thinking', count: 1 }] },
  });
  part.api.apply(part.ctx);
  pick(part.registered[0]);
  await new Promise((r) => setTimeout(r, 60));
  collected.push(...part.notices);

  const texts = collected.map((n) => String(n?.text ?? ''));
  // ① 成功 1 条 ＋ ② 失败 7 条 ＋ ③ partial 1 条 ＝ 9 条。
  assert.equal(texts.length, 9, `这几档都该出提示：${JSON.stringify(collected)}`);
  assert.ok(
    texts.every((t) => !/全部|都取到/.test(t)),
    `用户可见文案不许承诺取全了：${JSON.stringify(collected)}`,
  );
  assert.ok(
    texts.every((t) => !/已就绪/.test(t)),
    `用户可见文案不许出现「已就绪」：${JSON.stringify(collected)}`,
  );
  // **每条失败档都要说清后果**（PM 09:0x 回改单）：载体是 `error` 档（显眼＝
  // 「这会拒」）⇒ ⇒ 文案要自洽 ⇒ ⇒ 用户看完就知道发不出去 ⇒ ⇒ **防以后被人
  // 删掉这半句**。
  assert.equal(
    failedTierTexts.length,
    7,
    `失败那半边应当逐档都扫到：${JSON.stringify(failedTierTexts)}`,
  );
  assert.ok(
    failedTierTexts.every((t) => t.includes('发送时会拒绝这次提交')),
    `每条失败档都必须说清「发送时会拒绝这次提交」：${JSON.stringify(failedTierTexts)}`,
  );
});

test('serialize 等的是 onPick 那次读取（同一条链，不是重读）', async () => {
  const { api, ctx, registered } = loadClient({ config: { readDelayMs: 40 } });
  api.apply(ctx);
  const source = registered[0];
  const { ref } = pick(source);
  const text = await source.codec.serialize(ref, new AbortController().signal);
  assert.equal(text, `\n${BODY_ALPHA}\n`);
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
  assert.equal(text, `\n${BODY_ALPHA}\n`);
});

test('读取失败 → serialize reject（内核据此拒绝发送，不静默降级）', async () => {
  const { api, ctx, registered } = loadClient({ config: { readDelayMs: 10, failRead: true } });
  api.apply(ctx);
  const source = registered[0];
  const { ref } = pick(source);
  await assert.rejects(() => source.codec.serialize(ref, new AbortController().signal), /固定读取失败/);
});

test('读取失败（host 侧 source_unsupported ＋ 正文未接通标记）→ reject 的文案是成品文案，不是裸码', async () => {
  // **裸码的产出链**：host 侧 `toRpcError` 把协议码放进 `message`
  // （`host/rpc.js:114-117`）⇒ ⇒ 客户端原样抛出时，内核顶部横幅显示的就是
  // `source_unsupported：换一个来源` 这种裸码 ⇒ ⇒ 客户端这一层要换成成品文案。
  const { api, ctx, registered } = loadClient({
    config: { readDelayMs: 10 },
    captureFailure: {
      code: 'source_unsupported',
      message: 'source_unsupported：换一个来源',
      detail: BODY_SOURCE_PENDING_DETAIL,
    },
  });
  api.apply(ctx);
  const source = registered[0];
  const { ref } = pick(source);
  await assert.rejects(
    () => source.codec.serialize(ref, new AbortController().signal),
    (error) => {
      assert.equal(
        error.message,
        '这条引用带不出正文：目录页能列出会话，但正文读取还没接通。',
        '命中「正文未接通」那个标记 ⇒ 用那句成品文案',
      );
      assert.ok(
        !error.message.includes('source_unsupported'),
        `不许把裸码上屏：${error.message}`,
      );
      assert.ok(
        !error.message.includes('body-source-pending'),
        `机器标记是内部串，不许上屏：${error.message}`,
      );
      return true;
    },
  );
});

test('读取失败但标记对不上 → reject 的文案是通用失败文案（不露裸码、不挂专属文案）', async () => {
  // **失配方向必须安全**：认不出的失败**不许**落到「正文还没接通」那句专属
  // 文案上（那会说错原因，比说错更坏）⇒ ⇒ 落通用失败文案。
  const { api, ctx, registered } = loadClient({
    config: { readDelayMs: 10 },
    captureFailure: {
      code: 'source_unsupported',
      message: 'source_unsupported：换一个来源',
      detail: '别的 detail（没有正文未接通那个机器标记）',
    },
  });
  api.apply(ctx);
  const source = registered[0];
  const { ref } = pick(source);
  await assert.rejects(
    () => source.codec.serialize(ref, new AbortController().signal),
    (error) => {
      assert.ok(
        !error.message.includes('source_unsupported'),
        `不许把裸码上屏：${error.message}`,
      );
      assert.ok(
        !error.message.includes('正文读取还没接通'),
        `失配不许落到那句专属文案上：${error.message}`,
      );
      assert.ok(
        !error.message.includes('body-source-pending'),
        `机器标记是内部串，不许上屏：${error.message}`,
      );
      assert.equal(
        error.message,
        '这条引用没读出正文：快照不存在，或读取过程失败。',
        '失配 ⇒ 通用失败文案',
      );
      return true;
    },
  );
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
  // **会话那部分仍是那两条**（PM 2026-10-22 14:2x 派单新增的「来源站」分组是
  // **另外**三行 ⇒ ⇒ 这里断的是会话那部分**没被带偏**）。
  const sessions = sessionRows(rows);
  assert.equal(sessions.length, 2);
  // **分组标题**（izzy 2026-10-21 当面点名）：「官方对话」⇒「Web Chat」⇒
  // **断言保留、只改值**（它就是钉这个字面量的，别删）。
  assert.equal(sessions[0].section, 'Web Chat');
  assert.ok(sessions.every((r) => JSON.parse(r.value).kind === 'official-chat'));
  // 目录项的会话 id 键名是 **`id`**（协议 P4 §2.4 面二 / §4.3 ②）⇒ ⇒ 候选行
  // 必须真的带上它。**读旧键名会拿到空串** —— 那是「看起来正常」的坏链，
  // 所以这条断言不能省。
  // ⚠️ **`Array.from`**：候选行是**沙箱 realm** 的数组（`node:vm` 里造的）⇒ ⇒
  // `assert/strict` 的 `deepEqual` 是 deepStrictEqual ⇒ ⇒ 跨 realm 的数组
  // **原型不同** ⇒ ⇒ 先转成宿主数组再比。
  const ids = Array.from(sessions, (r) => JSON.parse(r.value).id);
  assert.deepEqual(ids.sort(), ['g1-alpha', 'g1-beta']);
  // ⚠️ **「来源站」那几行是另一个 kind** ⇒ ⇒ 点它们是「换站」、**不是**「引
  // 会话」⇒ ⇒ 绝不会误插 chip（PM 2026-10-22 14:2x 派单第 ① 条）。
  const sites = siteRowsOf(rows);
  assert.equal(sites.length, 3);
  assert.ok(sites.every((r) => JSON.parse(r.value).kind === 'official-chat-site'));
});

test('目录查询**去抖** ＋ 「用户刚打开候选组」那一次带 `refresh`（协议 §4.6 修法 3）', async () => {
  const { api, ctx, registered, catalogCalls } = loadClient();
  api.apply(ctx);
  const source = registered[0];
  // ① 第一次调用：离「上一次」很久 ⇒ 判定为**新开候选组** ⇒ 带 `refresh`。
  await source.candidates({ sessionId: 's1' }, { query: '' });
  assert.equal(catalogCalls.length, 1);
  assert.equal(catalogCalls[0].refresh, true, '新开候选组那一次必须带 refresh');
  // ② 紧接着再来一次（间隔 < 2 秒）⇒ **不带** `refresh`（连续打字不许每键重取）。
  await source.candidates({ sessionId: 's1' }, { query: '具' });
  assert.equal(catalogCalls.length, 2);
  assert.equal(catalogCalls[1].refresh, false, '连续打字期间不该每键都带 refresh');
  assert.equal(catalogCalls[1].query, '具');
  // ③ 隔久了再来 ⇒ 又算「新开候选组」⇒ 带 `refresh`。
  await new Promise((r) => setTimeout(r, 2100));
  await source.candidates({ sessionId: 's1' }, { query: '具身' });
  assert.equal(catalogCalls.length, 3);
  assert.equal(catalogCalls[2].refresh, true, '隔久了就该重新取当刻的');
  assert.equal(catalogCalls[2].query, '具身');
});

test('目录查询**去抖**：连着按几次 ⇒ **只发一次**（被顶掉的那些一起拿最后一份结果）', async () => {
  const { api, ctx, registered, catalogCalls } = loadClient();
  api.apply(ctx);
  const source = registered[0];
  // 三个调用**不 await**（模拟连按三个键）⇒ ⇒ 去抖之后**只发一次**。
  const pending = [
    source.candidates({ sessionId: 's1' }, { query: '具' }),
    source.candidates({ sessionId: 's1' }, { query: '具身' }),
    source.candidates({ sessionId: 's1' }, { query: '具身A' }),
  ];
  const rows = await Promise.all(pending);
  assert.equal(catalogCalls.length, 1, '连按几次只该发一次');
  assert.equal(catalogCalls[0].query, '具身A', '发出去的是**最后一次**那个 query');
  // **被顶掉的那两个不许被吊着**（吊着 ⇒ 内核那次 await 永远不回来）。
  assert.equal(rows.length, 3);
  assert.ok(rows.every((r) => Array.isArray(r)));
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
  // **正文本体**必须与落盘内容逐字节相同；正文前后**各包一个换行分隔符**
  // （协议 P1 §S-4 · v0.16）—— 分隔符**不属于**正文本体。
  assert.equal(
    text,
    `\n${BODY_ALPHA}\n`,
    '模型输入的正文本体必须与落盘内容逐字节相同（外加首尾各一个换行分隔符）',
  );
  assert.equal(text, `\n${snapshots.get(ref).body}\n`, '模型输入的正文本体 = 落盘内容');
  const probe = sandbox.__DSH_XLINK_OFFICIAL_CHAT_PROBE__;
  const ok = probe.events.filter((e) => e.kind === 'serialize-ok').pop();
  assert.equal(ok.fromFile, true, 'serialize 必须是从文件取的正文');
});

test('引用正文与用户新写的那句分行（内核展开 chip 时不加分隔符 ⇒ 分隔符由 serialize 负责）', async () => {
  const { api, ctx, registered } = loadClient({ config: { readDelayMs: 20 } });
  api.apply(ctx);
  const source = registered[0];
  const { ref } = pick(source);
  await new Promise((r) => setTimeout(r, 120));
  const text = await source.codec.serialize(ref, new AbortController().signal);
  // **内核的展开规则**（`ui-conversation/client.js@0.2.1-rc.2:15518-15522`）：
  //   `out += draft.slice(cursor, part.offset) + part.text;` … `out += draft.slice(cursor);`
  //   ⇒ `defaultSink(out.trim(), …)`
  // ⇒ ⇒ **内核自己一个字分隔符都不加** ⇒ chip 前后的文本会直接贴上去。
  // chip 在草稿里占 `clipboardText.length` 个字符（`clipboardText` ＝ `ref` ＝ 快照 id）。
  const expand = (draft, chipOffset) => {
    const out = draft.slice(0, chipOffset) + text + draft.slice(chipOffset + ref.length);
    return out.trim();
  };
  // ① 先引用、后写新问题（2026-10-22 用户截图那种形态）
  assert.equal(
    expand(`${ref}成华区天气怎么样`, 0),
    `${BODY_ALPHA}\n成华区天气怎么样`,
    '引用正文与新问题必须分行（新问题不许接在引用正文最后一句后面）',
  );
  // ② 先写新问题、后引用（chip 落在句尾）
  assert.equal(
    expand(`成华区天气怎么样${ref}`, '成华区天气怎么样'.length),
    `成华区天气怎么样\n${BODY_ALPHA}`,
    '引用正文必须自己独占一行',
  );
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
  assert.equal(text, `\n${BODY_ALPHA}\n`, '核对失败不得影响正文照发');
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
// **为什么要专门一条**：其它码落的那条固定串（「引用「…」的正文读取失败，
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

test('其它码（如 body_too_large）⇒ 按码给档 B 成品文案（不再落那条固定串）', async () => {
  const { api, ctx, registered, notices } = loadClient({
    config: { readDelayMs: 10 },
    captureFailure: { code: 'body_too_large', detail: '正文超限：300000 字节' },
  });
  api.apply(ctx);
  const source = registered[0];
  const { ref } = pick(source);
  await new Promise((r) => setTimeout(r, 80));
  const shown = notices.filter((n) => /带不出正文/.test(String(n?.text ?? '')));
  assert.equal(shown.length, 1, `失败应恰好一条成品文案：${JSON.stringify(notices)}`);
  // **逐字＝档 B 那一档的定稿原文**（PM 2026-10-22 08:4x 裁定 ＋ PM 09:0x 回改单
  // 补 `ref` 与后果句 ＋ PM 09:1x 的最终形状）。
  //
  // **为什么这一档有动作**：「正文太长」⇒ ⇒ **换一条正文更短的会话**⇒ ⇒ 用户
  // 做完那个动作**真能改变这一档的结果**（那条会话就不再超限）⇒ ⇒ 过得了
  // P1 §3 第 ⑦ 条那道判据。
  assert.equal(
    shown[0].text,
    `引用「${ref}」带不出正文：这条会话的正文太长，换一条短一些的会话再试。发送时会拒绝这次提交。`,
    `body_too_large 的文案必须逐字等于档 B 定稿原文：${shown[0].text}`,
  );
  // **档位是 `error`**（失败 ⇒ 正文没取到 ⇒ 发送会被拒 ⇒ 必须显眼）。
  assert.equal(shown[0].level, 'error', '失败必须是 error 档');
  // **不许落第 14 码的专属文案**（那是另一个码的事）。
  assert.equal(
    notices.filter((n) => /这条引用太长/.test(String(n?.text ?? ''))).length,
    0,
    `其它码不得落第 14 码的文案：${JSON.stringify(notices)}`,
  );
  // **不许落旧那条固定串**（口径已改：按码给成品文案）。
  assert.equal(
    notices.filter((n) => /正文读取失败/.test(String(n?.text ?? ''))).length,
    0,
    `按码给文案后不得再落那条固定串：${JSON.stringify(notices)}`,
  );
});

test('用户可见文案里不许出现「G1：」等项目内代号（izzy 2026-10-21 当面点名）', async () => {
  // **`G1` 是项目内的里程碑代号**（「G1 可行性验证」）⇒ ⇒ 用户不认 ⇒ 用户可见
  // 的文案里不许有它。
  //
  // 覆盖三处：`showState` 的失败档／落定档 ＋ `showUnavailable`。
  const { api, ctx, registered, notices } = loadClient({
    config: { readDelayMs: 10, failRead: true },
  });
  api.apply(ctx);
  const source = registered[0];
  const { ref } = pick(source);
  await new Promise((r) => setTimeout(r, 60));
  source.openReference({ sessionId: 's1' }, { ref, appearance: 'session' });
  await new Promise((r) => setTimeout(r, 40));

  // **引用不成那一档**（`showUnavailable`）：先让目录失败拿到那行「不可用」，
  // 再点它 ⇒ 走 `showUnavailable`。
  const bad = loadClient({ catalogFailure: { code: 'catalog_failed' } });
  bad.api.apply(bad.ctx);
  const badRows = await bad.registered[0].candidates({ sessionId: 's1' }, { query: '' });
  // **点的是那行说明**（站行在前、说明行在后 ⇒ 不是 `badRows[0]`）。
  const badNotice = noticeRow(badRows);
  assert.ok(badNotice, `目录失败时也要给一行说明：${JSON.stringify(badRows)}`);
  bad.registered[0].onPick({
    candidate: badNotice,
    session: { sessionId: 's1' },
    span: { start: 0, end: 1, draftRev: 7 },
  });
  await new Promise((r) => setTimeout(r, 40));

  const shown = [...notices, ...bad.notices];
  assert.ok(shown.length >= 2, `那几处提示都该出现：${JSON.stringify(shown)}`);
  assert.ok(
    shown.every((n) => !/G1[:：]/.test(String(n?.text ?? ''))),
    `用户可见文案不得出现「G1：」：${JSON.stringify(shown)}`,
  );
  // **去掉前缀后句子仍然自包含**（说得清是哪一条、发生了什么）。
  assert.ok(
    shown.some((n) => /引用「/.test(String(n?.text ?? ''))),
    `去掉前缀后仍要说得清是哪一条：${JSON.stringify(shown)}`,
  );
  assert.ok(
    shown.some((n) => /官网引用这次没有生成/.test(String(n?.text ?? ''))),
    `引用不成那一档的文案必须仍在：${JSON.stringify(shown)}`,
  );
});

test('用户可见文案里不许出现「host」「快照 id」这类内部词（口径同 `catalogUnavailableText`）', async () => {
  // **内部词不是代号，但同样不进界面**：「host」是插件与 host 半边之间的叫法、
  // 「快照 id」是内部术语 ⇒ ⇒ 用户读不懂、也不认（口径见
  // `catalogUnavailableText` 的注释：内部词与错误码只进埋点）。
  //
  // **两档都造**：① 目录行没带上引用编号（`snapshotId` 为空）② 引用编号超长
  // （> 24 可见字符）⇒ 两档的 `reason` 都进界面。
  const { api, ctx, registered, notices } = loadClient();
  api.apply(ctx);
  const source = registered[0];

  const pickRow = (snapshotId) => source.onPick({
    candidate: {
      name: 'x',
      description: 'x',
      icon: 'session',
      section: 'Web Chat',
      value: JSON.stringify({ kind: 'official-chat', id: 'g1-alpha', title: 'x', snapshotId }),
    },
    session: { sessionId: 's1' },
    span: { start: 0, end: 1, draftRev: 7 },
  });

  pickRow('');
  pickRow('x'.repeat(30));
  await new Promise((r) => setTimeout(r, 40));

  const shown = notices.map((n) => String(n?.text ?? ''));
  assert.equal(shown.length, 2, `两档都该出提示：${JSON.stringify(notices)}`);
  // **内部词一个都不许有**。
  assert.ok(
    shown.every((t) => !/host/.test(t) && !/快照/.test(t)),
    `用户可见文案不得出现内部词：${JSON.stringify(shown)}`,
  );
  // **逐字钉住定稿文案**（要改先过 PM）。**三档的动作各按档给**（PM 23:5x
  // 裁定：下一步不许一刀切 ⇒ 每个动作都得过「做完真能改变这一档的结果吗」
  // 那道判据）。
  assert.equal(
    shown[0],
    '官网引用这次没有生成（没取到引用编号），换一条会话再试',
    `第一档文案必须逐字等于定稿原文：${shown[0]}`,
  );
  assert.equal(
    shown[1],
    '官网引用这次没有生成（引用编号超长）',
    `第二档文案必须逐字等于定稿原文：${shown[1]}`,
  );
  // **空括号不许出现**（成员 23:5x 提醒的那个洞）：外层那句话要么**整句不带
  // 括号**、要么括号里有**这一档独有的类别词** ⇒ ⇒ 绝不能是「（）」那种空位。
  assert.ok(
    shown.every((t) => !/（\s*）/.test(t)),
    `外层不许出现空括号：${JSON.stringify(shown)}`,
  );
  // **一刀切的尾巴不许回来**（PM 23:5x 裁定）：「重新打开 @ 菜单再试」**对每一
  // 档都套**⇒ 两档都站不住 ⇒ ⇒ 外层**一律不许**出现它。
  assert.ok(
    shown.every((t) => !t.includes('重新打开 @ 菜单再试')),
    `外层不许一刀切地给「重新打开 @ 菜单再试」：${JSON.stringify(shown)}`,
  );

  // **两层提示各管一件事**（PM 2026-10-21 23:5x 裁定）⇒ ⇒ ① **菜单那行**给
  // 「为什么」（catalog 的成品文案仍留在那一行）；② **点它之后的提示**只说
  // 「这次没生成」这一事实 ⇒ ⇒ **不把成品文案套进括号**、也不复读、**也不
  // 复读菜单那行的下一步**。
  const bad = loadClient({ catalogFailure: { code: 'not_logged_in' } });
  bad.api.apply(bad.ctx);
  const badRows = await bad.registered[0].candidates({ sessionId: 's1' }, { query: '' });
  // **点的是那行说明**（站行在前、说明行在后 ⇒ 不是 `badRows[0]`）。
  const badNotice = noticeRow(badRows);
  assert.ok(badNotice, `目录失败时也要给一行说明：${JSON.stringify(badRows)}`);
  bad.registered[0].onPick({
    candidate: badNotice,
    session: { sessionId: 's1' },
    span: { start: 0, end: 1, draftRev: 7 },
  });
  await new Promise((r) => setTimeout(r, 40));
  // **「为什么」留在菜单那行**（用户点之前就看得见）。
  assert.equal(
    badNotice.description,
    '还没在官网窗口登录，登录后才能列出会话。',
    '菜单那行必须仍带着「为什么」',
  );
  const badShown = bad.notices.map((n) => String(n?.text ?? ''));
  assert.equal(badShown.length, 1, `点「不可用」那行只出一条提示：${JSON.stringify(bad.notices)}`);
  // **这一档：菜单那行已说原因、也说了下一步 ⇒ 外层只说事实、不给动作**。
  assert.equal(
    badShown[0],
    '官网引用这次没有生成',
    `这一档只说事实、不给动作：${badShown[0]}`,
  );
  assert.ok(
    !badShown[0].includes('还没在官网窗口登录'),
    `外层不许复读菜单那行的原因：${badShown[0]}`,
  );
  assert.ok(
    !badShown[0].includes('重新打开 @ 菜单再试'),
    `外层不许复读菜单那行的下一步：${badShown[0]}`,
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

// ─────────────────── 来源站（PM 2026-10-22 14:2x 派单） ───────────────────

/**
 * 点「来源站」分组里的某一行（**切站**）。
 *
 * @param {object} source - 注册出来的来源。
 * @param {object[]} rows - 候选行。
 * @param {string} label - 站名。
 * @returns {unknown} `onPick` 的返回（切站 ⇒ 必须是 `undefined`）。
 */
function pickSite(source, rows, label) {
  const row = siteRowsOf(rows).find((one) => one.name === label);
  assert.ok(row, `菜单里得有「${label}」那一行`);
  return source.onPick({
    candidate: row,
    session: { sessionId: 's1' },
    span: { start: 0, end: 1, draftRev: 7 },
  });
}

/**
 * 点**会话**那一行（形成引用句柄）。
 *
 * @param {object} source - 注册出来的来源。
 * @param {object} row - 菜单里的会话行（**带 host 分配的 id**）。
 * @returns {unknown} `onPick` 的返回。
 */
function pickRow(source, row) {
  return source.onPick({
    candidate: row,
    session: { sessionId: 's1' },
    span: { start: 0, end: 1, draftRev: 7 },
  });
}

test('「来源站」的短码与**壳侧**那三站逐一对上（不许自造短码）', async () => {
  // ① **壳侧 Rust 的三个常量**（**生产段** —— `#[cfg(test)]` 那一段先截掉，
  //    否则测试里自己造的常量会被算进来）。
  const rust = readFileSync(
    new URL('../../../src-tauri/src/official_chat/session_structure.rs', import.meta.url),
    'utf8',
  );
  const production = rust.split('#[cfg(test)]')[0];
  const rustSites = [...production.matchAll(/const\s+\w*SITE\w*:\s*&str\s*=\s*"([^"]+)"/g)].map(
    (one) => one[1],
  );
  assert.deepEqual(
    [...new Set(rustSites)].sort(),
    ['chatgpt', 'deepseek', 'minimax'],
    `壳侧那三站变了 ⇒ 客户端的站表必须跟着改（读到的：${rustSites.join('、')}）`,
  );

  // ② **客户端的站表**（从**界面**读 ⇒ 断的是用户真能点到的那三行）。
  const { api, ctx, registered } = loadClient();
  api.apply(ctx);
  const rows = await registered[0].candidates({ sessionId: 's1' }, { query: '' });
  const clientSites = siteRowsOf(rows).map((row) => JSON.parse(row.value).code);
  assert.deepEqual(
    [...clientSites].sort(),
    ['chatgpt', 'deepseek', 'minimax'],
    `客户端的站表必须与壳侧一致（读到的：${clientSites.join('、')}）`,
  );

  // ③ **形态**：`parseSite` 认不出的短码 ⇒ 请求体**不带** `site` ⇒ ⇒ 客户端
  //    用的每一个都必须过它（否则请求会被**悄悄丢掉**）。
  for (const code of clientSites) {
    assert.notEqual(parseSite(code), '', `短码「${code}」过不了 parseSite（请求体会被悄悄丢掉）`);
  }
});

test('未选站 ⇒ 请求体里**没有** `site` 这个键（既有用户零变化）', async () => {
  const { api, ctx, registered, catalogCalls } = loadClient();
  api.apply(ctx);
  await registered[0].candidates({ sessionId: 's1' }, { query: '' });
  assert.equal(catalogCalls.length, 1);
  assert.equal(catalogCalls[0].hasSite, false, '没选站 ⇒ 这个键整个不出现');
  assert.equal(catalogCalls[0].site, undefined);
  // **host 那半边的真函数**：不带 ⇒ 请求体**逐字**是既有形态。
  assert.equal(
    buildRequestBody('sessionStructure', true, ''),
    '{"command":"sessionStructure","refresh":true}',
  );
});

test('选了 MiniMax ⇒ 目录请求带 `site`，候选组换成**那站**的会话', async () => {
  const { api, ctx, registered, catalogCalls, notices } = loadClient();
  api.apply(ctx);
  const source = registered[0];
  // ① 还没选站 ⇒ 目录是默认那份（两条固定夹具）。
  const before = await source.candidates({ sessionId: 's1' }, { query: '' });
  assert.equal(sessionRows(before).length, 2);
  // ② 点「来源站」分组里 MiniMax 那一行 ⇒ ⇒ **切站**、**不形成引用句柄**。
  const outcome = pickSite(source, before, 'MiniMax');
  assert.equal(outcome, undefined, '切站**不形成引用句柄**（不许插半个 chip）');
  // ③ 提示说清切到哪（自包含）。
  assert.ok(
    notices.some((one) => one.text === '已切到MiniMax，之后 @ 菜单列的是MiniMax的会话。'),
    `切站提示必须说清切到哪：${JSON.stringify(notices.map((one) => one.text))}`,
  );
  // ④ 再取一次目录 ⇒ ⇒ 这一次**带 `site`**。
  const after = await source.candidates({ sessionId: 's1' }, { query: '' });
  const last = catalogCalls[catalogCalls.length - 1];
  assert.equal(last.hasSite, true);
  assert.equal(last.site, 'minimax');
  // ⚠️ **切站不再靠强刷绕缓存**（PM 2026-10-22 14:5x 收尾单）：host 侧目录缓存
  // 键**已带站段** ⇒ ⇒ 切站后读的是另一把钥匙 ⇒ ⇒ 那一刀**不带** `refresh`
  // （`refresh` 只管新鲜度：距上次发起超过 2 秒 ⇒ 判为新开候选组）。
  assert.equal(last.refresh, false, '切站不靠强刷绕缓存（host 的缓存键已带站段）');
  // ⑤ **候选组换成那站的会话** ⇒ 条数与条目 id 都不同。
  const sessions = sessionRows(after);
  assert.equal(sessions.length, 3, 'MiniMax 那一站是三条');
  assert.deepEqual(
    Array.from(sessions, (row) => JSON.parse(row.value).id),
    ['mm-1', 'mm-2', 'mm-3'],
  );
  // ⑥ **描述列显示站名**（host 侧 `SITE_LABELS` 只有 `deepseek` 一条 ⇒ 桩故意
  //    给小写短码 `minimax` ⇒ ⇒ 界面这半要自己给品牌名，**短码不直接上屏**）。
  assert.equal(sessions[0].description, 'MiniMax');
  // ⑦ 当前站那一行标出来（用户一眼看出自己在哪）。
  assert.equal(siteRowsOf(after).find((row) => row.name === 'MiniMax').description, '当前来源站');
});

test('正文取数按**这条引用当初那个站**去取（不是当前站）', async () => {
  const { api, ctx, registered, captureCalls } = loadClient({ config: { readDelayMs: 10 } });
  api.apply(ctx);
  const source = registered[0];
  // ① 选 MiniMax，再引一条。
  const before = await source.candidates({ sessionId: 's1' }, { query: '' });
  pickSite(source, before, 'MiniMax');
  const mmRows = await source.candidates({ sessionId: 's1' }, { query: '' });
  pickRow(source, sessionRows(mmRows)[0]);
  await new Promise((r) => setTimeout(r, 60));
  // ② 切回 DeepSeek，再引一条。
  const after = await source.candidates({ sessionId: 's1' }, { query: '' });
  pickSite(source, after, 'DeepSeek');
  const dsRows = await source.candidates({ sessionId: 's1' }, { query: '' });
  pickRow(source, sessionRows(dsRows)[0]);
  await new Promise((r) => setTimeout(r, 60));
  // ③ 两次取正文带的站**各归各**（第一条仍按 MiniMax 取）。
  assert.equal(captureCalls.length, 2, `两次取正文都要有载荷：${JSON.stringify(captureCalls)}`);
  assert.equal(captureCalls[0].site, 'minimax');
  assert.equal(captureCalls[1].site, 'deepseek');
});

test('切了站之后，**已经选中的那条**仍按它自己的站取正文（不是当前站）', async () => {
  // **读取慢一点**（800 ms）⇒ ⇒ 切站能插在「选中」与「取正文」之间——这才是
  // 真正的风险时序（用户在正文取回来之前改了主意）。
  const { api, ctx, registered, captureCalls } = loadClient({ config: { readDelayMs: 800 } });
  api.apply(ctx);
  const source = registered[0];
  // ① 在 MiniMax 那一站选中一条（正文**还没取回来**）。
  const before = await source.candidates({ sessionId: 's1' }, { query: '' });
  pickSite(source, before, 'MiniMax');
  const mmRows = await source.candidates({ sessionId: 's1' }, { query: '' });
  pickRow(source, sessionRows(mmRows)[0]);
  // ② **这时候切到 DeepSeek**（用户改了主意）⇒ ⇒ 当前站已经是 deepseek。
  const after = await source.candidates({ sessionId: 's1' }, { query: '' });
  pickSite(source, after, 'DeepSeek');
  // ③ 正文这时才取 ⇒ ⇒ **必须按它当初那个站（MiniMax）取**。
  await new Promise((r) => setTimeout(r, 1100));
  assert.equal(captureCalls.length, 1, `取正文那次要有载荷：${JSON.stringify(captureCalls)}`);
  assert.equal(
    captureCalls[0].site,
    'minimax',
    '切站之后，那条引用仍按它自己的站取正文（当前站不是答案）',
  );
});

test('场景 A：先选 MiniMax → 切到 ChatGPT → 再点那条 MiniMax 的会话 ⇒ 取正文仍按 minimax', async () => {
  // **PM 2026-10-22 14:35 复核的场景 A**：候选组弹出来之后用户切了站、再点其中
  // 一行 ⇒ ⇒ 那一行**属于旧页** ⇒ ⇒ 取正文必须按**旧页的站**。
  const { api, ctx, registered, captureCalls } = loadClient({ config: { readDelayMs: 10 } });
  api.apply(ctx);
  const source = registered[0];
  // ① 选 MiniMax ⇒ 目录页回来（`page.source.id = minimax`）。
  const before = await source.candidates({ sessionId: 's1' }, { query: '' });
  pickSite(source, before, 'MiniMax');
  const mmRows = await source.candidates({ sessionId: 's1' }, { query: '' });
  const mmRow = sessionRows(mmRows)[0];
  assert.equal(JSON.parse(mmRow.value).site, 'minimax', '候选行要带着**那一页的站**');
  // ② **切到 ChatGPT**（用户改了主意）⇒ ⇒ 当前站已经是 chatgpt。
  pickSite(source, mmRows, 'ChatGPT');
  // ③ **再点手里那条 MiniMax 的会话**（它属于 MiniMax 那一页）。
  pickRow(source, mmRow);
  await new Promise((r) => setTimeout(r, 60));
  // ④ 取正文带的站 ＝ **minimax**，**不是** chatgpt。
  assert.equal(captureCalls.length, 1, `取正文那次要有载荷：${JSON.stringify(captureCalls)}`);
  assert.equal(
    captureCalls[0].site,
    'minimax',
    '那条会话属于 MiniMax 那一页 ⇒ 取正文必须按 minimax（不是当前站 chatgpt）',
  );
});

test('场景 B：那一页的站**认不出**（固定夹具页）⇒ 取正文**不带** `site` 键', async () => {
  const { api, ctx, registered, captureCalls } = loadClient({
    config: { readDelayMs: 10 },
    pageSource: { kind: 'fixture', id: 'fixture', label: '固定夹具' },
  });
  api.apply(ctx);
  const source = registered[0];
  const rows = await source.candidates({ sessionId: 's1' }, { query: '' });
  pickRow(source, sessionRows(rows)[0]);
  await new Promise((r) => setTimeout(r, 60));
  assert.equal(captureCalls.length, 1, `取正文那次要有载荷：${JSON.stringify(captureCalls)}`);
  assert.equal(captureCalls[0].hasSite, false, '认不出 ⇒ 这个键整个不出现（host 走默认站）');
  assert.equal(captureCalls[0].site, undefined);
});

test('场景 C：没选过站 ⇒ 取正文**不带** `site` 键（既有用户零变化）', async () => {
  const { api, ctx, registered, captureCalls } = loadClient({ config: { readDelayMs: 10 } });
  api.apply(ctx);
  const source = registered[0];
  const rows = await source.candidates({ sessionId: 's1' }, { query: '' });
  pickRow(source, sessionRows(rows)[0]);
  await new Promise((r) => setTimeout(r, 60));
  assert.equal(captureCalls.length, 1, `取正文那次要有载荷：${JSON.stringify(captureCalls)}`);
  assert.equal(captureCalls[0].hasSite, false, '没选过站 ⇒ 这个键整个不出现');
  assert.equal(captureCalls[0].site, undefined);
});

test('切站之后**不串缓存**（真 host 半边）：那一刀**不带** refresh，且列出来的是**新站**那一页', async () => {
  // ⚠️ **为什么必须用真 host 半边**：「切站后不串缓存」这件事**只有在真目录缓存
  // 逻辑下**才测得出来（假 host 桩没有缓存 ⇒ ⇒ 测了也测不到）⇒ ⇒ 这里把**真**
  // `createCommands` ＋ **真** `createCatalogCache` 接到客户端的 fetch 桩上。
  //
  // ⚠️ **反向自证怎么打**：把 host 侧 `catalogSiteSegment` 临时改成**不产站段**
  // ⇒ ⇒ 三个站共用一把钥匙 ⇒ ⇒ 切站后那一刀命中**上一个站**的缓存 ⇒ ⇒
  // **下面这条用例必红**（不红＝它没真的在测「切站后不串缓存」）。
  const rootDir = mkdtempSync(join(tmpdir(), 'dsh-catalog-cache-'));
  try {
    const transport = async ({ body }) => {
      const sent = JSON.parse(String(body));
      // ⚠️ **没给 `site` ⇒ 壳侧按默认站走**（`site_of`：默认 DeepSeek）⇒ ⇒ 回包
      // 信封上的 `site` 是**壳侧**给的站。
      const site = String(sent.site ?? '') === '' ? 'deepseek' : String(sent.site);
      const items = site === 'minimax'
        ? [
          { id: 'mm-1', title: 'MM 一', title_type: 'USER', model_type: 'minimax', pinned: false, updated_at: 1 },
          { id: 'mm-2', title: 'MM 二', title_type: 'USER', model_type: 'minimax', pinned: false, updated_at: 2 },
          { id: 'mm-3', title: 'MM 三', title_type: 'USER', model_type: 'minimax', pinned: false, updated_at: 3 },
        ]
        : [
          { id: 'ds-1', title: 'DS 一', title_type: 'USER', model_type: 'deepseek-chat', pinned: false, updated_at: 1 },
          { id: 'ds-2', title: 'DS 二', title_type: 'USER', model_type: 'deepseek-chat', pinned: false, updated_at: 2 },
        ];
      return {
        status: 200,
        body: JSON.stringify({
          ok: true,
          site,
          structure: {
            topKeys: ['sessions'],
            code: 0,
            msg: 'ok',
            dataKeys: ['sessions'],
            sessionsLength: items.length,
            firstItemKeys: Object.keys(items[0]),
            itemTypes: {},
            hasMore: false,
            items,
          },
        }),
      };
    };
    const commands = createCommands({
      store: { load: () => null, write: () => {} },
      catalogCache: createCatalogCache({ rootDir, ttlMs: 600000 }),
      captureSource: {
        list: async () => ({ ok: true, items: [] }),
        fetch: async () => ({ ok: true, messages: [], captureResult: { status: 'ok', omitted: [] } }),
      },
      siteSource: createSiteSource({
        env: {
          DSH_XLINK_BUILTIN_BRIDGE_URL: 'http://127.0.0.1:9',
          DSH_XLINK_BUILTIN_BRIDGE_TOKEN: 'token',
        },
        transport,
      }),
      seqAllocator: { reserve: () => ({ snapshotId: 'dsh~x~0001' }) },
      sweeper: null,
      providerId: 'official-chat',
      accountScope: 'acct-1',
    });
    const { api, ctx, registered, catalogCalls } = loadClient({
      // ⚠️ `createCommands` 返回的是**命令对象**（按端点取）⇒ ⇒ 这里按端点分发。
      realHost: (endpoint, payload) => commands[endpoint](payload),
    });
    api.apply(ctx);
    const source = registered[0];
    // ① 先取一次 DeepSeek 那一页（这一刀会**写**缓存）。
    const before = await source.candidates({ sessionId: 's1' }, { query: '' });
    assert.equal(sessionRows(before).length, 2, 'DeepSeek 那一页是两条');
    // ② 切到 MiniMax。
    pickSite(source, before, 'MiniMax');
    // ③ 再取一次目录 ⇒ ⇒ **不带 refresh**（撤掉那个绕行）⇒ ⇒ 且列出来的是
    //    **MiniMax 那一页**（host 的缓存键带站段 ⇒ ⇒ 拿不到 DeepSeek 那份缓存）。
    const after = await source.candidates({ sessionId: 's1' }, { query: '' });
    const last = catalogCalls[catalogCalls.length - 1];
    assert.equal(last.refresh, false, '切站不靠强刷绕缓存（host 的缓存键已带站段）');
    assert.equal(last.site, 'minimax');
    const sessions = sessionRows(after);
    assert.equal(sessions.length, 3, '列出来的必须是 MiniMax 那一页的三条（不串上一站的缓存）');
    assert.equal(sessions[0].description, 'MiniMax');
  } finally {
    rmSync(rootDir, { recursive: true, force: true });
  }
});

test('引用详情那条请求也带 `site`（三处口径一致）', async () => {
  const { api, ctx, registered, previewCalls } = loadClient({ config: { readDelayMs: 10 } });
  api.apply(ctx);
  const source = registered[0];
  const before = await source.candidates({ sessionId: 's1' }, { query: '' });
  pickSite(source, before, 'MiniMax');
  const mmRows = await source.candidates({ sessionId: 's1' }, { query: '' });
  const outcome = pickRow(source, sessionRows(mmRows)[0]);
  await new Promise((r) => setTimeout(r, 60));
  // **落盘之后**再点详情 ⇒ ⇒ 走「按固定 id 读盘」那条路（读取中那条路不发
  // 请求）。
  source.openReference({ sessionId: 's1' }, { ref: outcome?.insert?.ref, appearance: 'session' });
  await new Promise((r) => setTimeout(r, 30));
  assert.equal(previewCalls.length, 1, `读盘那次要有载荷：${JSON.stringify(previewCalls)}`);
  assert.equal(previewCalls[0].site, 'minimax');
  assert.equal(previewCalls[0].hasSite, true);
});

test('切到没登录的站 ⇒ 按 host 回包的真实码显示（不编「该站不可用」）', async () => {
  const { api, ctx, registered } = loadClient({ catalogFailure: { code: 'not_logged_in' } });
  api.apply(ctx);
  const rows = await registered[0].candidates({ sessionId: 's1' }, { query: '' });
  // **目录失败 ⇒ 「为什么列不出」那一行仍在**（站行在前、说明行在后 ⇒ 两组并存）。
  const notice = noticeRow(rows);
  assert.ok(notice, `目录失败时也要给一行说明：${JSON.stringify(rows)}`);
  // **逐字**：照 host 回包的真实码给（`not_logged_in` 那一档的定稿原文）。
  assert.equal(notice.description, '还没在官网窗口登录，登录后才能列出会话。');
  // **不许编「该站不可用」这种没依据的话**。
  assert.doesNotMatch(notice.description, /该站|这个站|不可用|不支持|已下线|未开放/);
});

test('目录取数失败 ⇒ **切站入口仍在**（三站行 ＋ 那行说明，两组并存）', async () => {
  // **协议 P4 v0.26 §4.7**：取数失败时，候选行 ＝ **各站入口行 ＋ 一行失败说明**
  // （站行在前、说明行在后）⇒ ⇒ 用户**看得见、点得着**三个站。
  //
  // ⚠️ **这一条只管「入口还在不在」** ⇒ ⇒ 它**不解决「取数为什么失败」**
  // （域名指向／登录态／挑战页是站点侧口径）。
  const { api, ctx, registered } = loadClient({ catalogFailure: { code: 'catalog_failed' } });
  api.apply(ctx);
  // ⚠️ **`Array.from`**：候选行是**沙箱 realm** 的数组（`node:vm` 里造的）⇒ ⇒
  // `assert/strict` 的 `deepEqual` 是 deepStrictEqual ⇒ ⇒ 跨 realm 的数组
  // **原型不同** ⇒ ⇒ 先转成宿主数组再比。
  const rows = Array.from(
    await registered[0].candidates({ sessionId: 's1' }, { query: '' }),
  );
  // ⚠️ **防假绿**：先确认**真的走进了失败分支**（那行说明在）⇒ ⇒ 否则这条
  // 用例测的根本不是失败路径。
  const notice = noticeRow(rows);
  assert.ok(notice, `必须真的走到失败分支（那行说明要在）：${JSON.stringify(rows)}`);
  assert.equal(notice.name, '官网引用不可用');
  // **三站行仍在**（用户看得见、点得着）。
  const sites = siteRowsOf(rows);
  assert.deepEqual(
    Array.from(sites, (row) => row.name),
    ['DeepSeek', 'MiniMax', 'ChatGPT'],
    `目录失败时三个站仍要列出来：${JSON.stringify(sites.map((row) => row.name))}`,
  );
  // **站行在前、说明行在后**（内核按数组顺序插分组标题 ⇒ ⇒ 两组并存）。
  assert.equal(rows[0].section, '来源站');
  assert.equal(rows[rows.length - 1].section, 'Web Chat');
  // **站行点了还是「换站」**（不误插引用 chip）。
  assert.ok(
    Array.from(sites, (row) => JSON.parse(row.value).kind).every(
      (kind) => kind === 'official-chat-site',
    ),
  );
});

test('host 通道不可达 ⇒ **切站入口仍在**（同样三站行 ＋ 那行说明）', async () => {
  // **另一条失败分支**（`hostClient` 取不到 ⇒ 页面里没有 fetch）⇒ ⇒ 形态口径
  // 与上面那条一致：**站行零网络依赖 ⇒ 取数失败也照样能换站**。
  const { api, ctx, registered } = loadClient({ withHostRpc: false });
  api.apply(ctx);
  const rows = Array.from(
    await registered[0].candidates({ sessionId: 's1' }, { query: '' }),
  );
  const notice = noticeRow(rows);
  assert.ok(notice, `host 不可达时也要给一行说明：${JSON.stringify(rows)}`);
  assert.deepEqual(
    Array.from(siteRowsOf(rows), (row) => row.name),
    ['DeepSeek', 'MiniMax', 'ChatGPT'],
    `host 不可达时三个站仍要列出来：${JSON.stringify(rows.map((row) => row.name))}`,
  );
  assert.equal(rows[0].section, '来源站');
  assert.equal(rows[rows.length - 1].section, 'Web Chat');
});

test('「来源站」分组：有关键词时**只列命中的**那几行', async () => {
  const { api, ctx, registered } = loadClient();
  api.apply(ctx);
  const rows = await registered[0].candidates({ sessionId: 's1' }, { query: 'mini' });
  const sites = siteRowsOf(rows);
  assert.equal(sites.length, 1);
  assert.equal(sites[0].name, 'MiniMax');
  // 命中不到 ⇒ **不显示这个分组**（不挂着无关的行）。
  const none = await registered[0].candidates({ sessionId: 's1' }, { query: 'zzz' });
  assert.equal(siteRowsOf(none).length, 0);
});

test('点「当前站」那一行 ⇒ 不假装切了', async () => {
  const { api, ctx, registered, notices } = loadClient();
  api.apply(ctx);
  const source = registered[0];
  const before = await source.candidates({ sessionId: 's1' }, { query: '' });
  pickSite(source, before, 'MiniMax');
  const after = await source.candidates({ sessionId: 's1' }, { query: '' });
  pickSite(source, after, 'MiniMax');
  assert.ok(
    notices.some((one) => one.text === '当前来源站已经是MiniMax。'),
    `点当前站那行要说清「已经是它」：${JSON.stringify(notices.map((one) => one.text))}`,
  );
});

test('「site」真的进了壳侧那条请求体（管道级读数）', () => {
  // **host 那半边的真函数**（不是桩）：带站 ⇒ 请求体带 `site` 那一格。
  assert.equal(
    buildRequestBody('sessionStructure', false, 'minimax'),
    '{"command":"sessionStructure","site":"minimax"}',
  );
  assert.equal(
    buildBodyRequestBody('sess-1', 'chatgpt'),
    '{"command":"sessionBody","sessionId":"sess-1","site":"chatgpt"}',
  );
  // **形态不认得 ⇒ 不带**（host 侧不猜）⇒ ⇒ 客户端必须自己保证短码在表里。
  assert.equal(
    buildRequestBody('sessionStructure', false, 'Not A Site'),
    '{"command":"sessionStructure"}',
  );
});
