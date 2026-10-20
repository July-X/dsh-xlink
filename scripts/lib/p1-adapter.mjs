// P1 协议判据的**实跑适配器**：把真实实现（`plugins/official-chat/**`）接到判据库的
// adapter 契约上。
//
// ## 边界
//
// 适配器只做三件事：
//  1. **造上下文**（内核 ctx、host 侧真落盘 store）——**不替实现做协议判断**；
//  2. **把实现的可观测口翻译成判据要的形状**（句柄、正文、状态、落盘文件、计数）；
//  3. **如实报告观测不到的东西**（返回 `undefined` / `null`），让判据报「未执行」
//     而不是假绿或假红。
//
// 观测口全部是**协议稿与内核既有契约**规定的：
//  · `source.onPick(...)` → 句柄（协议 S-1：ref === snapshotId）
//  · `source.codec.serialize(ref, signal)` → 进入模型输入的正文（协议 S-4）
//  · `source.stateOf(ref)` → 内存里的在途态（协议 S-3）
//  · 客户端探针事件 `open-reference` → 状态的结构化记录（不是文案）
//  · host 侧 `createSnapshotStore` → 真正的落盘物（协议 §5 的落盘路径）
//
// ## 隔离
//
// 落盘根目录一律在 `%TEMP%` 下的一次性目录里（`DSH_HOME` 由适配器注入），
// **绝不指向用户真实数据目录**。每次 `createAdapter` 用一个新的一次性目录。
//
// @module scripts/lib/p1-adapter

import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { tmpdir } from 'node:os';
import { createContext, runInContext } from 'node:vm';

const PLUGIN_ROOT = new URL('../../plugins/official-chat/', import.meta.url);
const CLIENT_SOURCE_URL = new URL('../../plugins/official-chat/client/client.js', import.meta.url);
/** host 半边目录（默认仓库里那份）。 */
const HOST_DIR_URL = new URL('../../plugins/official-chat/host/', import.meta.url);
/** 序号分配器的模块 URL（子进程要 import 同一份实现）。 */
const SEQ_STORE_URL = new URL('../../plugins/official-chat/host/seq-store.js', import.meta.url).href;

/** 客户端探针键（客户端自己写进 globalThis 的事件表）。 */
const PROBE_KEY = '__DSH_XLINK_OFFICIAL_CHAT_PROBE__';

/** 客户端配置键（读延迟等）。 */
const CONFIG_KEY = '__DSH_G1_CONFIG__';

/**
 * 一次性临时目录（判据专用，绝不指向用户真实数据目录）。
 *
 * @param {string} tag - 目录名后缀，便于事后辨认。
 * @returns {string}
 */
export function makeTempRoot(tag = 'p1') {
  const dir = join(
    tmpdir(),
    `dsh-p1-protocol-${tag}-${Date.now()}-${Math.floor(Math.random() * 1e6)}`,
  );
  mkdirSync(dir, { recursive: true });
  return dir;
}

/**
 * 删掉一次性临时目录（只删自己创建的）。
 *
 * @param {string} dir
 */
export function removeTempRoot(dir) {
  if (typeof dir !== 'string' || dir === '') return;
  if (!dir.includes('dsh-p1-protocol-')) {
    throw new Error(`拒绝删除不是判据自己建的目录：${dir}`);
  }
  rmSync(dir, { recursive: true, force: true });
}

/**
 * 载入客户端束并造一个假的内核 ctx（**与内核加载的是同一份源码**）。
 *
 * @param {object} options
 * @param {object} options.store - host 侧真落盘 store（`saveSnapshot` / `loadSnapshot`）。
 * @param {object} [options.config] - 客户端配置。
 * @param {Function} options.fetchImpl - 客户端用的 **same-origin fetch** 桩
 *   （v0.4：客户端自己发 fetch 打 exact route）。
 * @returns {object}
 */
function loadClient({ config = {}, sourcePath, fetchImpl }) {
  const source = readFileSync(sourcePath ?? CLIENT_SOURCE_URL, 'utf8');
  const registered = [];
  const notices = [];
  const effects = [];
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
    [CONFIG_KEY]: config,
    // **客户端自己发 same-origin fetch**（协议 v0.4 §5）：桩要复刻内核的
    // **全等 pathname 匹配 + method 校验**，不是「怎么调都能通」。
    fetch: fetchImpl,
  };
  sandbox.window = sandbox;
  sandbox.globalThis = sandbox;
  sandbox.__ModuleLoader__ = { load: (entry) => { module = entry; } };
  sandbox.window.__ModuleLoader__ = sandbox.__ModuleLoader__;

  createContext(sandbox);
  runInContext(source, sandbox, { filename: 'plugins/official-chat/client/client.js' });

  const actx = {
    bail: () => true,
    get: (name) => (name === 'conversation'
      ? { input: { for: () => ({ notify: (level, text) => notices.push({ level, text }) }) } }
      : undefined),
  };

  const services = {
    inputTriggers: { registerSource: (s) => registered.push(s) },
    sessions: { scope: () => actx },
    // **注意：`connection` 服务不再给客户端**——v0.4 起客户端**自己发 fetch**
    // 打 exact route，不再读 `ctx.get('connection')`。
  };

  const ctx = {
    get: (name) => services[name],
    effect: (fn) => { effects.push(fn); fn(); },
  };

  const api = module.factory();
  const probe = sandbox[PROBE_KEY] ?? { events: [], counters: {} };
  // **用 getter**：`registered[0]` 要等 `apply(ctx)` 之后才有值，
  // 在这里直接取值会永远拿到 undefined。
  return {
    api,
    ctx,
    probe,
    notices,
    registered,
    get source() {
      return registered[0];
    },
    sandbox,
  };
}

/**
 * 等到某个 ref 的读取落定（落盘成功或失败）。
 *
 * @param {object} probe
 * @param {string} ref
 * @param {number} timeoutMs
 * @returns {Promise<string>} 'snapshot-saved' | 'snapshot-failed' | 'timeout'
 */
async function waitSettled(probe, ref, timeoutMs = 15000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const events = probe?.events ?? [];
    for (let i = events.length - 1; i >= 0; i -= 1) {
      const event = events[i];
      if ((event.kind === 'snapshot-saved' || event.kind === 'snapshot-failed') && event.ref === ref) {
        return event.kind;
      }
    }
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
  return 'timeout';
}

/**
 * 建一个判据适配器。
 *
 * @param {object} [options]
 * @param {string} [options.home] - 落盘根（`DSH_HOME`）。不给就用一次性临时目录。
 * @param {string} [options.tag] - 临时目录后缀。
 * @param {object} [options.host] - host 侧模块覆盖（测试时注入替身）。
 * @param {string} [options.clientSourcePath] - 客户端源码路径（活体注入时指向**副本**）。
 * @param {string} [options.hostDir] - **host 半边目录**（活体注入时指向**副本**）。
 *   目录里要有 `index.js` / `commands.js` / `rpc.js` / `seq-store.js` / `protocol.js`
 *   ——它们之间的相对 import 在副本里同样成立，所以「对真实实现副本注入」能跑通。
 * @param {number} [options.now] - **冻结时钟**（毫秒）。不给就是**实时**
 *   （P1 判据的口径一个字不变）。P2 §1.1 的键含 `capturedAt`（毫秒精度）⇒
 *   两次独立捕获几乎必然不同 ⇒ 「同组」场景必须冻结时间才造得出来。
 * @param {number} [options.bodyBytes] - **每条正文的字节数**（造大正文用；
 *   不给就是默认正文）。P2 §4 的预算预检要 `projectedBytes > 262144`。
 * @param {'transport'|'command'} [options.failDuplicates] - 让 `duplicates`
 *   端点失败（P2 §2.3 的失败口径）。不给就不注入。
 * @returns {Promise<object>} adapter。
 */
export async function createAdapter({
  home,
  tag = 'adapter',
  host,
  clientSourcePath,
  hostDir,
  now,
  bodyBytes,
  failDuplicates,
} = {}) {
  // **host 半边的模块来源**：默认仓库里那份；`hostDir` 给了就用那份目录
  // （活体注入用——注入打在**副本**上，原实现一个字节都不碰）。
  const hostRoot = hostDir === undefined ? HOST_DIR_URL : pathToFileURL(`${hostDir}/`);
  const hostModule = host ?? (await import(new URL('index.js', hostRoot).href));
  const commandsModule = await import(new URL('commands.js', hostRoot).href);

  const ownsRoot = home === undefined;
  const root = home ?? makeTempRoot(tag);
  mkdirSync(root, { recursive: true });

  const rpcModule = await import(new URL('rpc.js', hostRoot).href);
  // **序号分配器**（协议 v0.2 S-1：id 分配权归 host，client 不得自造 seq）。
  const seqModule = await import(new URL('seq-store.js', hostRoot).href);
  // **协议层模块**（P2 §1.2 的 `groupKeyOf` 在这里）：与 host 半边同一份副本 ⇒
  // 活体注入打在副本上时，被测的组标识函数也跟着变。
  const protocolModule = await import(new URL('protocol.js', hostRoot).href);

  /** 当前这一轮实例的状态。 */
  let client = null;
  let store = null;
  let commands = null;
  let seqAllocator = null;
  let scope = { accountScope: 'desktop-dev/default/site-A', instance: 'default-dev', mode: 'dev' };
  let serializeOkCount = 0;
  let captureCount = 0;
  let fetchCount = 0;
  let throwInternalError = false;
  let failCatalog = false;

  /**
   * **时钟**（P2 判据用）。
   *
   * 默认 `null` = **实时**（P1 判据的口径一个字都不变）；给了 `now` 就**冻结**
   * （`setNow()` 可改）——P2 §1.1 的判定键含 `capturedAt`（毫秒精度）⇒
   * 两次独立捕获几乎必然不同 ⇒ 「同组成立」这个场景**必须**冻结时间才造得出来。
   */
  let frozenNow = typeof now === 'number' ? Number(now) : null;
  const clock = () => (frozenNow === null ? Date.now() : frozenNow);

  /**
   * **`duplicates` 的失败注入模式**（P2 §2.3）。
   *
   * `null` = 不注入；`'transport'` = 送不到（HTTP 500）；`'command'` = 命令
   * 自己说失败（`{ok:false, code, …}` 的错误负载）。
   */
  let failMode = typeof failDuplicates === 'string' ? failDuplicates : null;

  /**
   * **清理器替身**（协议 §4.1 第 1 条的**触发点**判据要用）。
   *
   * 真实装配里 `host/index.js` 传的是**真** sweeper；这里传一个
   * **只记调用、不真扫**的替身——它把「谁调了清理器」变成**可观测**，
   * 而判据要判的正是**触发点**（§4.1：只在 `catalog` 入口，`capture` 不触发）。
   *
   * ## 为什么不用真的
   *
   * 真 sweeper 会在别的判据跑的时候删盘上的文件 ⇒ **判据之间互相污染**
   * （A17 / A18 各自自建一次性夹具根，不受这里影响）。
   *
   * ## 为什么替身不能自己抛
   *
   * `commands.js` 的 `catalog` 把清理调用包在 try/catch 里（协议 §4.1：
   * 清理失败不阻塞业务）——替身抛错会被那个 catch 吞掉，**判据就看不到
   * 「触发过」**。所以替身只记录、不抛。
   */
  const sweeperCallsLog = [];
  const sweeperSpy = {
    /** 惰性入口（`catalog` 走的就是这个）。 */
    maybeSweep() {
      sweeperCallsLog.push({ method: 'maybeSweep' });
      return { ran: false, skipped: true, reason: '判据替身：不真的扫' };
    },
    /** 全量扫。 */
    sweep() {
      sweeperCallsLog.push({ method: 'sweep' });
      return { scanned: 0, removed: 0 };
    },
    /** 今天是否已扫过。 */
    alreadySweptToday() {
      sweeperCallsLog.push({ method: 'alreadySweptToday' });
      return false;
    },
  };

  const env = { ...process.env, DSH_HOME: root };

  function buildStore() {
    return hostModule.createSnapshotStore(env);
  }

  /** 客户端按会话取到的正文（host 半边取数，正文不经过客户端 → 没有第二份真相）。 */
  function bodyFor(sessionId) {
    const base = `【判据正文】${String(sessionId ?? '')}\n第二行固定正文。`;
    // **P2 §4 的预算预检要「正文大到超预算」**（`bodyBytes` 给了才造）。
    // 补的是 ASCII ⇒ 字节数 = 字符数 ⇒ 目标字节数**精确**。
    if (!Number.isInteger(bodyBytes) || bodyBytes <= 0) return base;
    const need = bodyBytes - Buffer.byteLength(base, 'utf8');
    if (need <= 0) return base;
    return `${base}${'A'.repeat(need)}`;
  }

  /**
   * **假 `connection` 服务**：只实现 exact fetch route 的**注册**
   * （协议 v0.4 §5 的通道形态）。
   *
   * ## 这里模拟的是内核 `connection` 服务的**路由注册**语义（不是直连 handler）
   *
   * 协议 v0.4 §8 第 13 条（A0 / A0s）：「client → host 经内核 `connection` 服务的
   * **真实路由注册**跑通一次（**不得直连 handler 绕过注册**）」。
   *
   * 真机 p1-r2 / p1-r6 的根因就是**没注册上**：host 半边调 `rpc.handle` 时内核抛
   * `cannot get property "webServer" without inject` ⇒ 路由压根没注册 ⇒ 菜单空、
   * 正文没进模型。**若本文件直连 handler，那个洞就会被判据自己盖住**（判据全绿、
   * 真机全红）——所以这里**必须**走「注册 exact route + 客户端自己 fetch」：
   * **host 半边不注册，客户端就调不通**（拿到 404）。
   *
   * **故意不实现 `rpc.handle`**：v0.4 已证伪那条路（插件侧必抛）。若有人把 host
   * 半边改回 `rpc.handle`，这里就**注册不上** ⇒ 客户端 404 ⇒ **判据红**。
   *
   * **仍然不等于真机**：内核那边还有 owner 准入、webServer 路由与 fiber scope
   * 的那套（协议 §5 的取证）。**真机级那条（A0）仓库级跑不了**，如实报「未执行」。
   */
  const routes = new Map();
  const connection = {
    fetch: {
      /** host 半边注册 exact route（与内核 `connection.fetch.register` 同名同形）。 */
      register: (route) => {
        const path = String(route?.path ?? '');
        if (path === '') throw new Error('exact route 缺 path');
        if (routes.has(path)) throw new Error(`exact route 重复注册：${path}`);
        routes.set(path, route);
        return () => routes.delete(path);
      },
    },
  };

  /**
   * 客户端用的 **same-origin fetch** 桩：复刻内核 shared fetch handler 的
   * **全等 pathname 匹配**与 **method 校验**（内核 `createSharedFetchHandler`，
   * `dsh-client-connection/lib/index.js@0.2.1-rc.2:820-838`：先查 exact route，
   * path 不在表里 ⇒ 404；method 不在集合里 ⇒ 405）。
   *
   * @param {string|Request} input - URL。
   * @param {object} [init] - fetch init。
   * @returns {Promise<Response>}
   */
  const fetchImpl = async (input, init = {}) => {
    const url = new URL(String(input), 'http://127.0.0.1');
    const route = routes.get(url.pathname);
    if (route === undefined) {
      // **没注册就调不通**——这正是真机那个洞的形状（路由没注册上）。
      return new Response('not found', { status: 404 });
    }
    const method = String(init?.method ?? 'GET').toUpperCase();
    if (!route.methods.includes(method)) {
      return new Response('method not allowed', { status: 405 });
    }
    // **P2 §2.3 的失败注入**（默认 `failMode === null` ⇒ **完全不影响**既有行为）。
    const injected = injectDuplicatesFailure(init);
    if (injected !== undefined) return injected;
    const request = new Request(url.toString(), { ...init, method });
    return await route.fetch(request);
  };

  /**
   * `duplicates` 端点的**失败注入**（P2 §2.3 的失败口径）。
   *
   * · `'transport'` —— **送不到**（HTTP 500）：客户端抛 `host_unreachable`。
   * · `'command'` —— **命令自己说失败**：`{ok:false, code, …}` 的错误负载
   *   （字段照 P1 §3 的闭集；客户端在 `value.ok === false` 分支抛，码进
   *   `failure.code`）。
   *
   * **两种形态都按真实协议负载构造**：注入的是**对端真会收到的那种东西**，
   * 不是「随便一个异常」——否则测的就不是「客户端怎么处理失败」了。
   *
   * @param {object} init - fetch init（`body` 是 RPC 信封的 JSON 串）。
   * @returns {Response|undefined} 注入的响应；不注入时返回 `undefined`。
   */
  function injectDuplicatesFailure(init) {
    if (failMode === null) return undefined;
    let envelope = null;
    try {
      envelope = JSON.parse(String(init?.body ?? ''));
    } catch {
      return undefined;
    }
    if (envelope?.method !== 'duplicates') return undefined;
    if (failMode === 'transport') {
      return new Response('判据注入：duplicates 端点故障', { status: 500 });
    }
    const rpcId = String(envelope?.rpcId ?? '');
    const value = {
      ok: false,
      code: 'snapshot_invalid',
      retryable: false,
      userNext: '移除该引用',
      requestId: rpcId,
      logPath: null,
      detail: '判据注入：duplicates 命令失败',
    };
    return new Response(
      JSON.stringify({ type: 'server-response', rpcId, result: { ok: true, value } }),
      { status: 200, headers: { 'content-type': 'application/json' } },
    );
  }

  /**
   * 经**注册好的那条 exact route**调一个命令。
   *
   * **为什么走 fetch 而不是直连 handler**：判据要看的必须是「对端真正拿到的东西」
   * ——包括**信封翻译后**的形态（`{ok:false, error:{code, message, details}}`），
   * 不是命令层的内部返回。走 fetch = 走**客户端那条路**（全等 pathname 匹配 +
   * method 校验 + 信封），所以「host 半边没注册 ⇒ 404」这条也会如实暴露。
   *
   * @param {string} endpoint - 端点名（协议 §5 的五个）。
   * @param {object} payload - 请求负载。
   * @returns {Promise<object>} `{ok:true,value}` 或 `{ok:false,error}`。
   */
  async function callThroughRoute(endpoint, payload) {
    const rpcId = `p1-${endpoint}-${Math.random().toString(36).slice(2, 10)}`;
    const response = await fetchImpl(rpcModule.FETCH_ROUTE_PATH, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ type: 'client-request', rpcId, method: endpoint, payload }),
    });
    if (!response.ok) {
      throw new Error(`exact route 调用失败：HTTP ${response.status}`);
    }
    const full = await response.json();
    return full.result;
  }

  function buildClient() {
    const loaded = loadClient({ config: { readDelayMs: 0 }, sourcePath: clientSourcePath, fetchImpl });
    // **必须调 `apply(ctx)`**：来源是在 apply 里经 `ctx.effect` 注册的，
    // 不调就一个 source 都拿不到（`registered` 是空的）。
    loaded.api.apply(loaded.ctx);
    return loaded;
  }

  const captureSource = {
    /**
     * 列目录：**按查询词回一条**目录项。
     *
     * **为什么目录里必须有东西**：协议 v0.2 S-1 的 id 由 host 在 `catalog` 那一步
     * 分配（回包里的 `allocations`），目录空 ⇒ 没有会话 ⇒ **分配不出 id** ⇒
     * 客户端的 `onPick` 拿不到 `snapshotId` ⇒ 判据什么也观察不到（表现为
     * 「skipped」而不是红绿）。所以取数源按查询词回一条，让 host 的 `catalog`
     * 走**真实分配路径**。
     *
     * @param {{accountScope?: string, query?: string}} request
     * @returns {Promise<object>}
     */
    async list({ query } = {}) {
      fetchCount += 1;
      if (failCatalog) {
        // 目录**读取失败**（协议 §3：`catalog_failed`）——与「目录没有结果」
        // （`catalog_empty`）是**两回事**，判据要能分开验它们。
        throw new Error('目录读取失败（判据注入）');
      }
      const keyword = String(query ?? '').trim();
      const items =
        keyword === ''
          ? []
          : [
              {
                sessionId: keyword,
                title: `判据会话 ${keyword}`,
                updatedAt: new Date(0).toISOString(),
                deepLink: `https://example.invalid/chat/${keyword}`,
              },
            ];
      return { ok: true, items, hasMore: false };
    },
    async fetch({ sessionId } = {}) {
      fetchCount += 1;
      if (throwInternalError) {
        // 内部异常的**特征片段**：判据要确认它们**没有**被外发。
        throw new Error('ENOENT: /srv/secret/path.json (permission denied) at Object.fetch');
      }
      return { ok: true, messages: [{ role: 'user', text: bodyFor(sessionId) }] };
    },
  };

  /** 重建「进程」：新的 store + 新的 host 命令面 + 新的客户端实例（磁盘不动）。 */
  function boot() {
    store = buildStore();
    // **序号分配器**（协议 v0.2 S-1：id 分配权归 host）：`catalog` 认它、
    // `capture` 核对它，判据侧的 pick 也从它这里拿 id。
    //
    // **根目录与真实装配一致**：`host/index.js:519` 用的是 `store.root`
    // （= `<DSH_HOME>/official-chat`），不是 `DSH_HOME` 本身——判据若用另一个
    // 根，认领文件与快照文件就落在两棵不同的树里，跨进程那条判据测的就不是
    // 真实布局了。
    seqAllocator = seqModule.createSeqAllocator({ root: store.root, providerId: 'ds' });
    commands = commandsModule.createCommands({
      store,
      catalogCache: commandsModule.createCatalogCache({ rootDir: join(root, 'catalog-cache') }),
      captureSource,
      // provider 短码用**协议 §S-1 的口径**（`ds`/`gpt`/`mm`，≤4 字符），
      // 也与真实装配一致（`host/index.js` 的 `PROVIDER_ID = 'ds'`）。
      // 传别的值会让「落盘目录段」与「id 首段」不一致 → 落盘后读不回来，
      // 那会变成**判据自己制造的假红**，不是实现的缺口。
      providerId: 'ds',
      accountScope: scope.accountScope,
      // **序号分配器必须接上**（协议 v0.2 S-1：id 分配权归 host）——没接上时
      // `catalog` 分配不出 id，判据就什么也观察不到（表现为「skipped」而不是
      // 真的红绿）。
      seqAllocator,
      // **清理器替身**（协议 §4.1 第 1 条的触发点判据要用）：
      // 真实装配里 `host/index.js` 传的是**真** sweeper；判据这里传一个
      // **只记调用、不真扫**的替身——它把「谁调了清理器」变成**可观测**，
      // 而判据要判的正是**触发点**（§4.1：只在 `catalog` 入口，`capture` 不触发）。
      //
      // **为什么不用真的**：真 sweeper 会在别的判据跑的时候删盘上的文件 ⇒
      // 判据之间互相污染（A17 / A18 各自自建独立实例，不受这里影响）。
      sweeper: sweeperSpy,
      // **时钟**（P2 判据冻结时间用；默认实时 ⇒ P1 口径不变）。
      now: clock,
    });
    // 命令层的 capture 计数（用于「失效后是否静默重读」）。
    const originalCapture = commands.capture;
    commands.capture = async (request) => {
      captureCount += 1;
      return originalCapture(request);
    };
    // **注册 exact route**（协议 v0.4 §8-13：不得直连 handler 绕过注册）。
    //
    // **走 host 半边自己的注册代码**（`host/index.js` 的 `registerExactRoute`）——
    // 这样「host 半边改回 `rpc.handle`」这件事在**仓库级就会红**：假的
    // `connection` 服务**没有** `rpc.handle` ⇒ 注册不上 ⇒ 客户端拿到 404。
    //
    // **为什么不能自己拼一条「反正能通」的通道**：那正是「桩骗过自己」——
    // 真机全红、判据不报。
    routes.clear();
    if (typeof hostModule.registerExactRoute === 'function') {
      hostModule.registerExactRoute(
        {
          connection,
          effect: (fn) => fn(),
          logger: { warn() {}, info() {}, error() {} },
        },
        { commands, logger: { warn() {}, info() {}, error() {} } },
      );
    } else {
      // 注入的替身 host 模块没有这段代码 ⇒ 退回「同一条 exact route」的手工注册
      // （**仍然是注册语义**：没注册就调不通）。
      const dispatch = rpcModule.createDispatcher({ commands });
      const routeHandler = rpcModule.createFetchRouteHandler({ dispatch });
      connection.fetch.register({
        kind: 'exact',
        path: rpcModule.FETCH_ROUTE_PATH,
        methods: ['POST'],
        requestBody: 'buffered',
        fetch: routeHandler,
      });
    }
    client = buildClient();
  }

  boot();

  /** 快照 id 的 provider 段 = 落盘目录名（协议 §5：`snapshots/<provider>/<id>.json`）。 */
  function providerOf(id) {
    const text = String(id ?? '');
    const head = text.split('~')[0];
    return head === '' ? '' : head;
  }

  /**
   * 快照文件在磁盘上的位置。
   *
   * 根目录**取 store 自己报的** `root`（`<DSH_HOME>/official-chat`）——
   * 自己拼 `join(root, ...)` 会漏掉后端解析出来的那一层（协议 §5：路径跟随后端解析）。
   *
   * provider 段**不猜**：先按 id 首段试，miss 就在 `snapshots/` 下按文件名找。
   * （落盘用 `snapshot.providerId`、读取用 id 首段——这两段若不一致，**正是**
   * 本判据要暴露的实现缺口，判据自己不能先把它抹平。）
   */
  function snapshotPath(ref) {
    const id = String(ref ?? '');
    const base = store?.root ?? join(root, 'official-chat');
    const direct = join(base, 'snapshots', providerOf(id), `${id}.json`);
    if (existsSync(direct)) return direct;
    const snapsDir = join(base, 'snapshots');
    if (!existsSync(snapsDir)) return direct;
    for (const entry of readdirSync(snapsDir, { withFileTypes: true })) {
      if (!entry.isDirectory()) continue;
      const candidate = join(snapsDir, entry.name, `${id}.json`);
      if (existsSync(candidate)) return candidate;
    }
    return direct;
  }

  const adapter = {
    /** 落盘根（判据自用）。 */
    root,

    /** 当前命名空间。 */
    scope: () => ({ ...scope }),

    /** 协议 §5 的 `capabilities`。 */
    capabilities() {
      return commands.capabilities();
    },

    /**
     * 一次 pick（协议 S-1：唯一性粒度 = 一次 pick）。
     *
     * ## **真实装配口径**（协议 v0.2 S-1：id 分配权归 host）
     *
     * 走的是**真实那条链**：
     *
     * ```text
     * client.candidates()  →  host.catalog()  →  allocations（host 分配 id）
     *        →  候选行带 snapshotId  →  client.onPick()（同步取用）
     * ```
     *
     * **为什么不自己造候选行**：自己造 = 绕过 host 的目录与分配 ⇒ 判据测的
     * 就不再是「真实装配」——这与 A0 那条「桩把本该跨进程的调用接上了」是
     * 同一个坑：**桩会骗过自己**。v0.1 时 client 自造 seq，造候选行没问题；
     * v0.2 之后 id 由 host 分配，造候选行就等于**假装 id 是 host 分配的**。
     *
     * 官网会话 id = 判据给的 `sessionId`（协议 §1.1 的 `sessionId` 就是它）；
     * `fixtureId` 只作标题标签，**不参与 id 计算**。
     *
     * @param {{sessionId: string, fixtureId?: string}} args
     * @returns {Promise<{ref: string, insert: object}>}
     */
    async pick({ sessionId, fixtureId = 'g1-alpha' }) {
      const sourceSessionId = String(sessionId ?? '');
      let rows;
      try {
        rows = await client.source.candidates(
          { sessionId: sourceSessionId },
          { query: sourceSessionId, quoted: false, drilled: false },
        );
      } catch (error) {
        return { ref: '', insert: {}, reason: `目录命令抛错：${String(error?.message ?? error)}` };
      }
      const row = (Array.isArray(rows) ? rows : []).find((item) => {
        try {
          const value = JSON.parse(String(item?.value ?? ''));
          return String(value?.id ?? '') === sourceSessionId;
        } catch {
          return false;
        }
      });
      if (row === undefined) {
        // 目录里没有这一行（host 不可达 / 目录失败 / 目录空）→ 如实报「没取到」。
        return { ref: '', insert: {}, reason: `目录里没有 ${sourceSessionId} 这一行（标签 ${fixtureId}）` };
      }
      const outcome = client.source.onPick({
        candidate: row,
        session: { sessionId: sourceSessionId },
        span: { start: 0, end: 1, draftRev: 1 },
      });
      const insert = outcome?.insert ?? {};
      // **官网会话 id 一并回给判据**：判据要自己调 host 命令时需要它
      // （协议 §1.1 的 `sessionId` = 官网会话 id；dsh 的会话 id 是另一回事）。
      return { ref: String(insert.ref ?? ''), insert, sessionId: sourceSessionId };
    },

    /**
     * 等读取落定。
     *
     * @param {string} ref
     */
    async settle(ref) {
      return waitSettled(client.probe, ref);
    },

    /**
     * 序列化：进入模型输入的正文（协议 S-4）。
     *
     * @param {string} ref
     * @returns {Promise<string>}
     */
    async serialize(ref) {
      const body = await client.source.codec.serialize(String(ref));
      serializeOkCount += 1;
      return String(body ?? '');
    },

    /**
     * 内存里的在途态。
     *
     * 客户端**没有**把 `stateOf` 挂在 source 上（它在 factory 闭包里），
     * 所以这里如实返回 `undefined`——观测不到就是观测不到，不许编。
     *
     * @param {string} ref
     * @returns {string|undefined}
     */
    stateOf(ref) {
      const state = client.source?.stateOf?.(String(ref));
      return state === undefined ? undefined : String(state);
    },

    /**
     * 打开引用详情后的**结构化**观测（协议 S-3 的判据口径）。
     *
     * · `from` —— `'inflight'` = 状态来自内存里的在途记录；
     *           `'snapshot-file'` = 状态来自**快照文件**。
     * · `level` —— 通知级别：`'info'` = 派生出来是就绪态，`'error'` = 失败态。
     *   （**判 level 枚举，不判文案**——文案是措辞，不是契约。）
     *
     * @param {string} ref
     * @returns {Promise<{from: string, state: string, level?: string}|undefined>}
     */
    async openReferenceObservation(ref) {
      const before = client.probe.events.length;
      const noticeBefore = client.notices.length;
      try {
        client.source.openReference({ sessionId: 'p1-check' }, { ref: String(ref) });
      } catch (error) {
        return { from: 'error', state: undefined, level: undefined, error: String(error?.message ?? error) };
      }
      const deadline = Date.now() + 3000;
      let event = null;
      while (Date.now() < deadline && event === null) {
        const events = client.probe.events ?? [];
        for (let i = events.length - 1; i >= before; i -= 1) {
          if (events[i].kind === 'open-reference') {
            event = events[i];
            break;
          }
          if (events[i].kind === 'open-reference-failed') {
            event = { from: 'load-failed', state: 'failed' };
            break;
          }
        }
        if (event === null) await new Promise((resolve) => setTimeout(resolve, 20));
      }
      if (event === null) return undefined;
      // 通知可能是异步来的（读文件那一步），再等一下。
      while (Date.now() < deadline) {
        const notice = client.notices[noticeBefore];
        if (notice !== undefined) {
          return { from: String(event.from ?? ''), state: String(event.state ?? ''), level: notice.level };
        }
        await new Promise((resolve) => setTimeout(resolve, 20));
      }
      return { from: String(event.from ?? ''), state: String(event.state ?? ''), level: undefined };
    },

    /**
     * 磁盘上的快照（协议 S-4 的「落盘物」）。
     *
     * @param {string} ref
     * @returns {{exists: boolean, path: string, body: string, raw: string}}
     */
    snapshotFile(ref) {
      const id = String(ref ?? '');
      if (id === '') return { exists: false, path: '', body: '', raw: '' };
      const path = snapshotPath(id);
      if (!existsSync(path)) return { exists: false, path, body: '', raw: '' };
      const raw = readFileSync(path, 'utf8');
      let parsed = null;
      try {
        parsed = JSON.parse(raw);
      } catch {
        parsed = null;
      }
      const messages = Array.isArray(parsed?.messages) ? parsed.messages : [];
      const body = messages.map((message) => String(message?.text ?? '')).join('\n\n');
      return { exists: true, path, body, raw, snapshot: parsed };
    },

    /**
     * 删掉一份快照（模拟保留期到期被清理）。
     *
     * @param {string} ref
     */
    removeSnapshot(ref) {
      const id = String(ref ?? '');
      const path = snapshotPath(id);
      if (existsSync(path)) rmSync(path, { force: true });
    },

    /**
     * 制造失效场景。
     *
     * @param {string} ref
     * @param {'missing'|'fingerprint'|'digest'} scenario
     */
    async invalidate(ref, scenario) {
      const id = String(ref ?? '');
      const path = snapshotPath(id);
      if (scenario === 'missing') {
        if (existsSync(path)) rmSync(path, { force: true });
        return;
      }
      if (!existsSync(path)) return;
      const raw = readFileSync(path, 'utf8');
      const parsed = JSON.parse(raw);
      if (scenario === 'digest') {
        parsed.contentDigest = 'f'.repeat(64);
      } else if (scenario === 'fingerprint') {
        // 指纹变了：改来源指纹，并把摘要按**新正文**重算（否则会被当成摘要不匹配，
        // 测的就不是「指纹变化」这一条了）。
        parsed.sourceFingerprint = 'fp-changed-0000';
        const body = (Array.isArray(parsed.messages) ? parsed.messages : [])
          .map((message) => String(message?.text ?? ''))
          .join('\n\n');
        parsed.contentDigest = createHash('sha256')
          .update(body.replace(/\r\n/g, '\n').replace(/\r/g, '\n'), 'utf8')
          .digest('hex');
      }
      writeFileSync(path, `${JSON.stringify(parsed, null, 2)}\n`, 'utf8');
    },

    /**
     * chip 的**节点级**状态。
     *
     * 当前实现没有把这个状态暴露给驱动侧（内核 chip 是内核自己的草稿节点，
     * 插件拿不到编辑器）——所以这里如实返回 `undefined`，让判据报「未执行」。
     *
     * @param {string} ref
     * @returns {object|undefined}
     */
    chipNode(ref) {
      const events = client.probe?.events ?? [];
      const hit = [...events].reverse().find(
        (event) => event.kind === 'chip-invalid' && event.ref === String(ref),
      );
      return hit === undefined ? undefined : { invalid: hit.invalid === true };
    },

    /** 成功产出正文（= 真的会进模型请求）的次数。 */
    modelRequestCount() {
      return serializeOkCount;
    },

    /** 命令层 capture 被调用的次数。 */
    captureCalls() {
      return captureCount;
    },

    /** 取数次数（重新取数 = 静默重读）。 */
    readCalls() {
      return fetchCount;
    },

    /**
     * **清理器替身的调用记录**（协议 §4.1 第 1 条的触发点观测口）。
     *
     * 判据要判的是「**谁**调了清理器」：`catalog` 入口调、`capture` 入口不调。
     * 真实 sweeper 把这个信号吞掉了（它只管扫），所以这里用**替身**把它变成可观测。
     *
     * @returns {{count: number, calls: Array<{method: string}>}}
     */
    sweeperCalls() {
      return { count: sweeperCallsLog.length, calls: [...sweeperCallsLog] };
    },

    /**
     * 让取数侧**真的抛一个带内部细节的异常**，返回外发的错误负载。
     *
     * 协议 §3：内部异常串只能进日志，不能进给用户看的那一层。
     *
     * @returns {Promise<object|undefined>}
     */
    async captureWithInternalError() {
      throwInternalError = true;
      try {
        const result = await commands.capture({
          sessionId: 'p1-check-leak',
          requestId: 'p1-check-leak-req',
        });
        return result;
      } finally {
        throwInternalError = false;
      }
    },

    /**
     * 模拟进程重启：**内存全空**，磁盘不动。
     */
    async reload() {
      boot();
    },

    /**
     * 换命名空间（换账号 / 换实例 / dev-release 同开）。
     *
     * @param {object} next
     * @param {Function} run
     */
    async withScope(next, run) {
      const previous = scope;
      scope = { ...scope, ...next };
      const previousRoot = adapter.root;
      // 不同命名空间 → 不同落盘根（协议 §5：路径跟随后端路径解析，按实例/账户隔离）。
      const scopedRoot = join(previousRoot, 'scope', `${scope.mode}-${scope.instance}-${scope.accountScope}`);
      mkdirSync(scopedRoot, { recursive: true });
      const savedEnv = env.DSH_HOME;
      env.DSH_HOME = scopedRoot;
      const savedStore = store;
      const savedClient = client;
      try {
        boot();
        const result = await run();
        return result;
      } finally {
        env.DSH_HOME = savedEnv;
        store = savedStore;
        client = savedClient;
        scope = previous;
      }
    },

    /**
     * 让**目录读取失败**（协议 §3 的 `catalog_failed`），跑一段逻辑。
     *
     * **为什么要有它**：§8-9 要求「`catalog_empty` 与 `catalog_failed`
     * **分离**」。只在码表里分开（可重试性/用户下一步不同）不够——还得验
     * **目录读失败时给的是 `catalog_failed`**，而不是 `catalog_empty` 或别的码。
     *
     * @param {() => Promise<unknown>} run
     * @returns {Promise<unknown>}
     */
    async withFailingCatalog(run) {
      const saved = failCatalog;
      failCatalog = true;
      try {
        return await run();
      } finally {
        failCatalog = saved;
      }
    },

    /**
     * 走**真实 RPC 通道**调一个 host 命令，返回**原始信封**。
     *
     * 判据要判「对端真正看到的错误负载长什么样」——经 RPC 层翻译后的形态
     * （`{ok:false, error:{code, message, details}}`）才是对端拿到的东西，
     * 不是命令层的内部返回。
     *
     * @param {string} endpoint - 端点名（协议 §5 的五个）。
     * @param {object} payload - 请求负载。
     * @returns {Promise<object>}
     */
    async callHost(endpoint, payload) {
      return callThroughRoute(endpoint, payload);
    },

    /**
     * **当前来源指纹不可得**时的 host 命令层行为。
     *
     * 协议 §S-5（v0.2）：「当前指纹不可得时 → 拒（`account_changed`），
     * **不得放行**」。指纹不可得 = 服务端推导出的账户范围为空串
     * （`commands.js` 的 `fingerprintOf('')` → 空串）。
     *
     * @param {() => Promise<unknown>} run - 在「指纹不可得」这一状态下跑。
     * @returns {Promise<unknown>}
     */
    async withUnavailableFingerprint(run) {
      const saved = scope;
      scope = { ...scope, accountScope: '' };
      try {
        boot();
        return await run();
      } finally {
        scope = saved;
        boot();
      }
    },

    /**
     * **存储层**读一份快照（协议 §S-5 v0.2：存储层**只**校验 `schemaVersion`
     * 与 `contentDigest`，**不比对指纹**——指纹的校验点是 host 命令层）。
     *
     * @param {string} ref
     * @returns {{loaded: boolean, error: string, fingerprint: string}}
     */
    storeLoad(ref) {
      try {
        const loaded = store.load(String(ref));
        return {
          loaded: true,
          error: '',
          fingerprint: String(loaded?.snapshot?.sourceFingerprint ?? ''),
        };
      } catch (error) {
        return {
          loaded: false,
          error: String(error?.code ?? error?.message ?? error),
          fingerprint: '',
        };
      }
    },

    /**
     * 用一个 **host 从未认领过的 id** 调 `capture`（协议 v0.2 S-1：
     * 「`seq` 由 host 侧统一分配……**client 不得自造**」）。
     *
     * 做法：先让 host 正常分配一个 id（拿真实的会话段），再把**序号**换成一个
     * 没被认领过的（`9999` = 分配器的上限，正常路径不会走到）。
     *
     * @param {string} sessionId - 官网会话 id。
     * @returns {Promise<{id: string, claimed: boolean, result: object}>}
     */
    async captureUnclaimedId(sessionId) {
      const listed = await commands.catalog({ query: String(sessionId), accountScope: scope.accountScope });
      const allocated = String(listed?.allocations?.[String(sessionId)] ?? '');
      if (allocated === '') {
        return { id: '', claimed: false, result: { ok: false, error: { code: 'catalog_failed' } } };
      }
      const parts = allocated.split('~');
      const unclaimed = `${parts[0]}~${parts[1]}~9999`;
      const claimed = seqAllocator.isClaimed(parts[1], 9999);
      const result = await callThroughRoute('capture', {
        sessionId: String(sessionId),
        snapshotId: unclaimed,
        requestId: 'p1-unclaimed',
      });
      return { id: unclaimed, claimed, result };
    },

    /**
     * **另一个进程**（真的 `node` 子进程）在同一个落盘根上认领一个序号。
     *
     * 协议 §2 S-1（v0.2）：「**「本实例内单调递增」必须跨进程成立**」。
     *
     * **为什么用真的子进程**：分配器靠**排他创建**（`wx`）认领，跨进程与
     * 「同进程两个实例」的差别只在「内存不共享」——而分配器**没有内存状态**
     * （每次都从目录读）。用真子进程验，才不是近似。
     *
     * @param {string} sessionId - 官网会话 id。
     * @returns {Promise<{seq: number, snapshotId: string}|undefined>}
     */
    async reserveInChildProcess(sessionId) {
      const script = [
        `import { createSeqAllocator } from ${JSON.stringify(SEQ_STORE_URL)};`,
        `const allocator = createSeqAllocator({ root: process.env.DSH_P1_SEQ_ROOT, providerId: 'ds' });`,
        `const reserved = allocator.reserve(process.env.DSH_P1_SEQ_SESSION);`,
        `process.stdout.write(JSON.stringify(reserved));`,
      ].join('\n');
      const stdout = execFileSync(process.execPath, ['--input-type=module', '-e', script], {
        env: {
          ...process.env,
          DSH_P1_SEQ_ROOT: String(store.root ?? ''),
          DSH_P1_SEQ_SESSION: String(sessionId ?? ''),
        },
        encoding: 'utf8',
        maxBuffer: 8 * 1024 * 1024,
      });
      const parsed = JSON.parse(String(stdout).trim());
      return { seq: Number(parsed?.seq ?? 0), snapshotId: String(parsed?.snapshotId ?? '') };
    },

    // ---------------------------------------------------------------------
    // P2 协议（§1.1 / §1.2 / §2.2 / §2.3 / §4）的观测口
    //
    // **这一段全部是可选的**：不给选项时行为与 P1 判据完全一致。
    // ---------------------------------------------------------------------

    /**
     * 客户端探针事件（客户端自己写进 globalThis 的事件表）。
     *
     * @returns {Array<object>}
     */
    probeEvents() {
      return [...(client.probe?.events ?? [])];
    },

    /**
     * 客户端的**可见提示**（P2 §3.2）。
     *
     * 每条 = `{kind, level, text, groups, budget, trigger, delivered}`：
     *
     * · `delivered === true` ⇒ 这条提示**经输入区通道投出去了**
     *   （内核 `conversation.input.for(actx).notify(level, text)`）
     *   ⇒ **承载位 = 输入区常驻条**（协议 §3.2；按 **v0.5 的渲染侧更正**：`error` 档
     *   **额外**弹一个瞬态 toast、**不**渲染 inline 常驻条 ⇒ 只有 `info` 档才有常驻条）。
     * · `groups` / `budget` = 提示里的**结构化数据**——判据判**数据**，
     *   不判文案措辞（UX 文案未定稿）。
     *
     * **为什么用「同一条文案」把两处记录对上**：结构化提示与承载位投递是
     * 客户端**两次记录**（一次 `record('notice', …)`、一次 `notify(…)`），
     * 两处的 `text` 是同一个字符串 ⇒ 用它当连接键。
     *
     * @returns {Array<object>}
     */
    visibleNotices() {
      const delivered = new Set(client.notices.map((n) => String(n?.text ?? '')));
      const out = [];
      for (const event of client.probe?.events ?? []) {
        if (typeof event?.text !== 'string' || typeof event?.level !== 'string') continue;
        // 「承载位投递」那条记录（`kind === 'notify'`）单独算，不重复列。
        if (event.kind === 'notify') continue;
        // **只认「提示」**：结构化提示带 `groups` / `budget` / `trigger` 之一。
        if (event.groups === undefined && event.budget === undefined && event.trigger === undefined) {
          continue;
        }
        out.push({
          kind: String(event.kind === 'notice' ? (event.noticeKind ?? '') : (event.kind ?? '')),
          level: String(event.level ?? ''),
          text: String(event.text ?? ''),
          groups: event.groups ?? null,
          budget: event.budget ?? null,
          trigger: event.trigger ?? null,
          delivered: delivered.has(String(event.text ?? '')),
        });
      }
      return out;
    },

    /**
     * 冻结时钟（毫秒）。P2 §1.1 的键含 `capturedAt` ⇒ 造「同组」必须冻结。
     *
     * @param {number} ms
     */
    setNow(ms) {
      frozenNow = Number(ms);
    },

    /**
     * **本轮造的正文目标字节数**（`bodyBytes` 选项；没给就是 `null`）。
     *
     * P2 §3.3「不得截断正文来绕过超限」这条要断「落盘正文 == 完整原文」，
     * 而「完整原文有多长」是**造输入时定的** ⇒ 判据从这儿取，不许自己猜。
     *
     * @returns {number|null}
     */
    configuredBodyBytes() {
      return Number.isInteger(bodyBytes) && bodyBytes > 0 ? bodyBytes : null;
    },

    /** 恢复实时时钟。 */
    releaseNow() {
      frozenNow = null;
    },

    /**
     * 在「`duplicates` 端点失败」的状态下跑一段逻辑（P2 §2.3）。
     *
     * @param {'transport'|'command'} mode - 失败形态。
     * @param {() => Promise<unknown>} run
     * @returns {Promise<unknown>}
     */
    async withFailingDuplicates(mode, run) {
      const saved = failMode;
      failMode = String(mode ?? '');
      try {
        return await run();
      } finally {
        failMode = saved;
      }
    },

    /**
     * 命令层的 `duplicates`（经**注册好的 exact route**，不走直连）。
     *
     * 返回值已归一：`{ok, groups, budget, code, error}`——
     * · `ok === true` ⇒ `groups` / `budget` 是协议 §2.2 的出参；
     * · `ok === false` ⇒ `code` 是**闭集内**的码（P1 §3；闭集外的码在实现侧
     *   已归入 `snapshot_invalid`、原码进 `detail`），`error` 是 RPC 层翻译后的
     *   错误负载（`{code, message, details}`，`details` 里带 `detail` / `retryable`
     *   / `userNext` / `requestId` / `logPath`）。
     *
     * @param {{sessionId: string, snapshotIds: string[]}} request
     * @returns {Promise<{ok: boolean, groups: Array<object>, budget: object|null,
     *   code: string|null, error: object|null}>}
     */
    async duplicatesFor({ sessionId, snapshotIds } = {}) {
      const result = await callThroughRoute('duplicates', {
        sessionId: String(sessionId ?? ''),
        snapshotIds: Array.isArray(snapshotIds) ? snapshotIds : [],
        requestId: 'p2-check',
      });
      if (result?.ok === true) {
        const value = result.value ?? {};
        return {
          ok: value?.ok === true,
          groups: Array.isArray(value?.groups) ? value.groups : [],
          budget: value?.budget ?? null,
          code: null,
          error: null,
        };
      }
      return {
        ok: false,
        groups: [],
        budget: null,
        code: String(result?.error?.code ?? ''),
        error: result?.error ?? null,
      };
    },

    /**
     * **命令层**直调 `duplicates`（**不经 RPC 翻译**）⇒ 拿到**协议 §2.2 的原始错误负载**。
     *
     * ## 为什么要直连命令层
     *
     * RPC 层（`host/rpc.js` 的 `toRpcError`）会把错误负载翻译成
     * `{code, message, details}` ⇒ **`kind` 这一字段穿不过去** ⇒ 要断 P1 §3 `:161`
     * 的 **8 字段闭集**（`kind`/`ok`/`code`/`retryable`/`userNext`/`requestId`/
     * `logPath`/`detail`），只能在**命令层**断。
     *
     * **这不是「绕过 exact route」**：A0s 禁的是**客户端→host 的传输通道**直连
     * （要证明内核那条路真的注册上了）；这里断的是**命令层自己的返回形态**，
     * 与 `host-commands.test.mjs` 同款。
     *
     * @param {object} request - `duplicates` 的入参。
     * @returns {Promise<object>} 命令层的原始返回。
     */
    async duplicatesCommandLayer(request = {}) {
      return commands.duplicates(request);
    },

    /**
     * 造一份**五字段可控**的快照（P2 §1.1 的键）。
     *
     * **这是造输入，不是造标准答案**：期望的「成组 / 不成组」由判据**自己**
     * 按协议 §1.1 算（五字段全等 ⇒ 成组），不 import 实现的分组逻辑。
     *
     * `sourceFingerprint` 按 host 的口径（`sha256(accountScope)` 前 32 位）算——
     * 那是**输入**（存储层的指纹比对），不是期望值。
     *
     * @param {object} [fields]
     * @returns {object} 快照对象。
     */
    makeSnapshot({
      seq = 1,
      sessionId = 'p2-craft-session',
      branch = 'main',
      accountScope = scope.accountScope,
      capturedAt = '2026-10-20T00:00:00.000Z',
      text = 'P2 判据正文。',
    } = {}) {
      const scopeText = String(accountScope ?? '').trim();
      const fingerprint =
        scopeText === ''
          ? ''
          : createHash('sha256').update(scopeText, 'utf8').digest('hex').slice(0, 32);
      return commands.buildSnapshot({
        providerId: 'ds',
        accountScope: String(accountScope ?? ''),
        sessionId: String(sessionId ?? ''),
        branch: String(branch ?? 'main'),
        seq: Number(seq),
        messages: [{ role: 'user', text: String(text ?? '') }],
        captureResult: { status: 'ok', omitted: [], unsupported: [] },
        capturedAt: String(capturedAt ?? ''),
        sourceUpdatedAt: String(capturedAt ?? ''),
        sourceFingerprint: fingerprint,
      });
    },

    /**
     * 把一份快照**落盘**（命令层场景用：造「盘上真有这条」）。
     *
     * @param {object} snapshot
     * @returns {object} store 的返回值。
     */
    saveSnapshot(snapshot) {
      return store.save(snapshot);
    },

    /**
     * 实现侧的**组标识函数**（协议 §1.2）。
     *
     * **是被测对象，不是标准答案**：判据只用它断「同组成员的标识一致 /
     * 不同键的标识不同 / 标识形状」，**以及**（协议 v0.3 把分隔符裁定为单个
     * 换行 U+000A 之后）**与判据自己按 §1.2 独立算出的值是否相等**。
     *
     * @param {object} snapshot
     * @returns {string}
     */
    groupKeyOf(snapshot) {
      return String(protocolModule.groupKeyOf(snapshot));
    },

    /** 收尾：删掉一次性临时目录。 */
    cleanup() {
      if (ownsRoot) removeTempRoot(root);
    },
  };

  return adapter;
}
