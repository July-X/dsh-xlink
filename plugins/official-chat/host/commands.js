/**
 * 客户端命令的**业务逻辑**（协议 §5：`capabilities` / `catalog` / `capture` /
 * `cancel` / `preview`）。
 *
 * ## 分层
 *
 * 本文件**不碰传输**（怎么从 client 到 host 是另一层的事），也不碰具体站点：
 * 取数由注入的 `captureSource` 提供。这样同一份逻辑可以被
 *  · 单元测试直接调（Node，无内核）；
 *  · 壳内回环路由直接复用；
 *  · 将来接真实站点时只换 `captureSource`。
 *
 * ## 目录页与快照是**两样东西**（协议 §1.2 / §5）
 *
 *   · 目录页（目录项 ＝ 协议 P4 §2.4 面二那 6 个键；游标在**顶层**）→
 *     `<实例 official-chat 根>/catalog-cache/`
 *     （**平铺**：文件名 = `<站段>__<provider>__<账号范围>__<查询词>.json`，
 *     见下面 `fileOf` —— **没有 provider 子目录**；**站段排第一** ⇒ ⇒ 三站
 *     不会共用一把钥匙）
 *   · 正文快照 → `<实例 official-chat 根>/snapshots/<provider>/`
 *
 * **目录页不含正文**（校验在 `protocol.js` 的 `validateCatalogPage`）。
 *
 * ## 本期不做什么（协议 §7 明确「不动的部分」）
 *
 * · **详情面板**属 P5 → `preview` 只回快照内容，不做面板；
 * · **正文去重**属 P2 → 同一正文引用两次 = 两个快照 id、**正文各送一份**（S-7）。
 *
 * ## 正文取数（协议 §4.4 · 已接通）
 *
 * · **目录页**（`catalog`）走**真实源**（`siteSource.structure`）；
 * · **正文**（`capture`）也走**真实源**（`siteSource.body`）：壳侧导航到那条会话的
 *   **深链** ⇒ ⇒ **站点自己**发那条 `history_messages` ⇒ ⇒ 壳侧取体并裁成
 *   「角色 ＋ 文本」⇒ ⇒ **壳侧零请求**、**页内零注入**；
 * · **真实源不在场时**（判据与单测环境）才退到**固定夹具**（`captureSource.fetch`）。
 *
 * @module xlink-official-chat/commands
 */

import { mkdirSync, readFileSync, writeFileSync, existsSync, readdirSync, unlinkSync, appendFileSync, chmodSync } from 'node:fs';
import { join } from 'node:path';
import { createHash } from 'node:crypto';

import {
  CONSTANTS,
  ERROR_CODES,
  ERROR_CODE_NAMES,
  SCHEMA_VERSION,
  STATES,
  buildCatalogSource,
  buildSnapshot,
  captureFailureCode,
  captureFailureDetail,
  errorPayload,
  groupKeyOf,
  makeProviderShort,
  makeSessionShort,
  parseSnapshotId,
  redactLogPath,
  resolveCase,
  validateCatalogPage,
} from './protocol.js';
import { createRefsStore } from './refs-store.js';
import { trimCatalogItem, parseSite } from './site-source.js';

/**
 * 把权限收成指定值（**失败就算了**）。
 *
 * **为什么要多这一步**：Node 的 `mode` **只在创建时生效** ⇒ 已存在的目录/文件
 * 不会被改 ⇒ **存量**落盘物收不进同一口径（协议 P3 §S-14 的目录缓存一条）。
 *
 * **失败不抛**：缓存写不进去不该把业务带崩 ⇒ 收不动就留着原样。
 *
 * @param {string} path - 目标路径。
 * @param {number} mode - 权限位（目录 `0o700`、文件 `0o600`）。
 */
function chmodQuiet(path, mode) {
  try {
    chmodSync(path, mode);
  } catch {
    /* 收不动就算了 */
  }
}

/**
 * 目录缓存键里的**站段**。
 *
 * ## 为什么键里必须有站
 *
 * 三站扩面以后，目录缓存**跨站共用一把钥匙**就是真缺陷：切站那一刻若那次取数
 * 失败、客户端没再带 `refresh`，下一次同 query 的目录请求会**命中上一个站的缓存页**
 * ⇒ ⇒ **用户明明选了 MiniMax，看到的却是 DeepSeek 的会话**。
 *
 * 客户端侧「站变了就强制重取」是**绕开**，不是根上修好 ⇒ ⇒ 根上要把键分开。
 *
 * ## 这一段长什么样
 *
 * **`<站段>__<provider>__<账号范围>__<查询词>.json`**，站段**排第一** ⇒ ⇒
 * `ls` 一下就分得开三站（「一眼看得出是哪个站」）。
 *
 * ⚠️ **站段带 `site-` 前缀，不裸写短码** —— 因为**紧挨着它的第二段是 provider
 * 短码 `ds`**，而 `ds` 恰恰**会被误读成 DeepSeek**（`ds` 是这个 provider 的短码，
 * **不是站短码**）⇒ ⇒ 加前缀 ⇒ **每一段自解释、不靠位置猜语义**。
 *
 * ## 没给站 / 认不出站 ⇒ 落**稳定的一段**
 *
 * `site-default` —— **绝不能让「空串」把两个站撞到同一个键上**。
 *
 * ⚠️ **为什么不把「没给站」归一成 `site-deepseek`**：「没给站」在壳侧意味着
 * 「**默认那个站点**」（DeepSeek），但**「默认站是哪个」只有内核知道**
 * （插件连本机回环桥，够不着站点）⇒ ⇒ 插件**不许**替内核假定 ⇒ ⇒ 只按
 * 「自己知道什么」分键（「没给站」＝ `site-default`）。
 * 代价：同一站可能被缓存两份（默认站与显式站各一份）—— **只是多一份缓存，
 * 不会串**。
 *
 * ⚠️ **形态认得出就逐字保留**（哪怕规格表里没有这个站）：壳侧对没登记的短码
 * 是**明确报错**（不猜、不回落）⇒ ⇒ 那种请求**取不到目录页**、也就不会落缓存
 * ⇒ ⇒ 但**读**发生在取之前 ⇒ ⇒ 若把它归一成 `site-default`，它就会**读到默认站
 * 那份缓存页** ⇒ ⇒ 逐字保留 ⇒ 键不同 ⇒ 读不到 ⇒ 干净。
 *
 * @param {unknown} site - 站点短码（客户端给的）。
 * @returns {string} 键里的站段（**永不为空**）。
 */
export function catalogSiteSegment(site) {
  const short = parseSite(site);
  return `site-${short === '' ? 'default' : short}`;
}

/**
 * 目录缓存（§1.2 的独立缓存物）。
 *
 * **换账号即失效**：缓存键里带 `accountScope`，账号变了就是另一个键。
 *
 * **换站即失效**：缓存键里带**站段**（`catalogSiteSegment`），站变了就是另一个键。
 *
 * @param {object} options
 * @param {string} options.rootDir - `<实例 official-chat 根>/catalog-cache`。
 * @param {number} [options.ttlMs] - 缓存期限（§4：5 分钟）。
 * @param {() => number} [options.now] - 取当前时间（测试注入）。
 * @returns {object} 缓存句柄。
 */
export function createCatalogCache({ rootDir, ttlMs = CONSTANTS.catalogCacheTtlMs, now = () => Date.now() }) {
  /**
   * 缓存文件名（**站段 ＋ provider ＋ 账号范围 ＋ 查询词**，全部编码进文件名）。
   *
   * ⚠️ **站段排第一**：站是**最外层**的维度（先选站，再选 provider／账号／查询词）
   * ⇒ ⇒ 键的顺序 ＝ 维度从外到内。
   */
  function fileOf(provider, site, accountScope, query) {
    const safe = (value) =>
      String(value ?? '')
        .replace(/[^0-9A-Za-z._-]/g, '_')
        .slice(0, 64);
    return join(
      rootDir,
      `${safe(catalogSiteSegment(site))}__${safe(provider)}__${safe(accountScope)}__${safe(query)}.json`,
    );
  }

  /**
   * 读缓存（过期即当作没有）。
   *
   * @returns {object|null}
   */
  function read(provider, site, accountScope, query) {
    const path = fileOf(provider, site, accountScope, query);
    if (!existsSync(path)) return null;
    let raw;
    try {
      raw = JSON.parse(readFileSync(path, 'utf8'));
    } catch {
      return null;
    }
    const fetchedAt = Date.parse(String(raw?.fetchedAt ?? ''));
    if (!Number.isFinite(fetchedAt) || now() - fetchedAt > ttlMs) return null;
    return raw.page ?? null;
  }

  /**
   * 写缓存。
   *
   * @param {object} page - 目录页。
   */
  function write(provider, site, accountScope, query, page) {
    // **目录缓存也要私有权限**（协议 P3 §S-14）：目录 `0o700`、文件 `0o600`
    // —— 与快照目录/文件（`host/index.js`）、认领目录/文件（`host/seq-store.js`）
    // 的既有口径一致。
    //
    // **Windows 上 `mode` 是 no-op**（Node 语义）⇒ 那边的实际保护来自**用户
    // profile 的默认 ACL**，**不声称跨平台加密**（§S-14 裁定 2）。
    mkdirSync(rootDir, { recursive: true, mode: 0o700 });
    // **`mode` 只在「创建时」生效** ⇒ 存量目录不会被改 ⇒ 显式收一次，让新旧
    // 落在同一口径（收不动就算了，见 `chmodQuiet`）。
    chmodQuiet(rootDir, 0o700);
    const path = fileOf(provider, site, accountScope, query);
    writeFileSync(
      path,
      `${JSON.stringify({ fetchedAt: new Date(now()).toISOString(), page }, null, 2)}\n`,
      { encoding: 'utf8', mode: 0o600 },
    );
    chmodQuiet(path, 0o600);
  }

  /**
   * 清缓存（换账号 / 显式失效）。
   *
   * @returns {number} 清掉几个文件。
   */
  function invalidate() {
    if (!existsSync(rootDir)) return 0;
    let count = 0;
    for (const name of readdirSync(rootDir)) {
      // 只删**缓存目录下的文件**（精确路径，不递归）。
      try {
        unlinkSync(join(rootDir, name));
        count += 1;
      } catch {
        /* 删不掉就留着 */
      }
    }
    return count;
  }

  return { read, write, invalidate, rootDir, ttlMs };
}

/**
 * 固定夹具取数源（本期用；真实站点属 P4，**不预写**）。
 *
 * 形态与协议一致：目录项（协议 P4 §2.4 面二那 6 个键）与可捕获的正文分开。
 *
 * @param {Array<{sessionId: string, title: string, body: string}>} items
 * @returns {object} `captureSource`。
 */
export function createFixtureSource(items) {
  const list = Array.isArray(items) ? items : [];
  return {
    /**
     * 列目录。
     *
     * **关键词在这里筛**（协议 §4 的搜索去抖 300 ms 是客户端的事；筛选本身是
     * 服务端的）：客户端把 `query` 传下来，服务端只回匹配的项。
     */
    async list({ accountScope, query }) {
      const keyword = String(query ?? '').trim();
      const rows = keyword === ''
        ? list
        : list.filter((item) => String(item?.title ?? '').includes(keyword));
      return {
        ok: true,
        // 目录项也是**那 6 个键**（协议 P4 §2.4 面二 / §4.3 ②）—— 夹具与真实源
        // **同形** ⇒ 命令层不必分叉。
        items: rows.map((item) => ({
          id: String(item?.sessionId ?? ''),
          title: String(item?.title ?? ''),
          title_type: 'USER',
          model_type: 'deepseek-chat',
          pinned: false,
          updated_at: 0,
        })),
        accountScope,
      };
    },
    /** 捕获一份正文。 */
    async fetch({ sessionId }) {
      const found = list.find((item) => item.sessionId === sessionId);
      if (found === undefined) {
        return { ok: false, code: 'source_unsupported', detail: '目录里没有这个会话' };
      }
      return {
        ok: true,
        messages: [{ role: 'user', text: found.body }],
        captureResult: { status: 'ok', omitted: [], unsupported: [] },
      };
    },
  };
}

/**
 * **账户维度的一致性校验**（协议 P3 §S-15）。
 *
 * 客户端传的 `accountScope` **只做校验、不构成授权**（P1 §5）：与服务端推导值
 * 不符 ⇒ 调用方按 `account_changed` 拒；**没传就放行**（客户端本来就不传这个
 * 字段——见 `client/client.js`：它不构造 `accountScope`）。
 *
 * **为什么不静默改写**：改写会让用户以为切了账户其实没切（§S-15 裁定 4）。
 *
 * @param {unknown} provided - 请求里的 `accountScope`（`undefined`/`null` ＝ 没传）。
 * @param {string} derived - **服务端推导**的账户范围。
 * @returns {string|null} 不符时的原因（可外发）；相符或没传 ⇒ `null`。
 */
function accountScopeMismatch(provided, derived) {
  if (provided === undefined || provided === null) return null;
  if (String(provided) === String(derived)) return null;
  return '账户范围与服务端推导值不一致';
}

/**
 * 目录项的**键与类型的唯一事实源**在 `protocol.js`（`CATALOG_ITEM_FIELDS`）。
 *
 * 这里只做**归一**：把来源（夹具／真实源）给的条目按那 6 个键裁一遍 ⇒ ⇒
 * 万一某一层多带了键（正文、凭据……），**它到不了目录页**。
 *
 * @param {unknown} item - 目录项面的一项。
 * @returns {{id: string, title: string, title_type: string, model_type: string,
 *   pinned: boolean, updated_at: number}}
 */
function catalogItem(item) {
  return trimCatalogItem(item);
}

/**
 * 建命令集。
 *
 * @param {object} options
 * @param {object} options.store - 快照存储（`createSnapshotStore` 的返回）。
 * @param {object} options.catalogCache - 目录缓存（`createCatalogCache` 的返回）。
 * @param {object} options.captureSource - 取数源（本期是固定夹具）。
 * @param {object} [options.siteSource] - **真实站点结构源**（P4：官网对话的会话目录
 *   **结构**，只回白名单；见 `host/site-source.js`）。**没接上时** `sessionStructure`
 *   命令按 `source_unsupported` 拒（**不静默降级**）。
 * @param {object} [options.seqAllocator] - 序号分配器（协议 v0.2 S-1「id 分配权
 *   归 host」）。**没有它就分配不出 id** ⇒ `catalog` 只能失败。
 * @param {object} [options.sweeper] - 保留期清理器（协议 §4.1）。**只在
 *   `catalog` 入口惰性触发**，至多一天一次。
 * @param {string} options.providerId - provider 稳定 id。
 * @param {string} options.accountScope - 账户范围（**后端推导**，见下）。
 * @param {() => number} [options.now]
 * @returns {object} 五个命令。
 */
export function createCommands({
  store,
  catalogCache,
  captureSource,
  siteSource = null,
  seqAllocator = null,
  sweeper = null,
  providerId = 'official-chat',
  accountScope = '',
  now = () => Date.now(),
}) {
  /** provider 的稳定短码（id 的第一段；与分配器同源）。 */
  const providerShort = makeProviderShort(providerId);

  /**
   * **占用记录存储**（协议 §2.4 / §S-11）。
   *
   * **根就是存储根**（`<实例根>` ＝ `store.root`）⇒ 调用方不必额外接线，
   * 也**不留可注入的口子**：占用关系**只有一个落点**
   * ⇒ 没有「换一个存储」的分支，也就没有「换一个存储 ⇒ 少写占用记录」这种
   * 只对测试有意义的入口。
   *
   * **根不可得时怎么办**（那一路是**单测里的假存储**，盘上没有实例根）
   * ⇒ **占用记录无处可写 ⇒ 跳过**（真实实现里根不可得时 `store.save`
   * **已经先失败**了，根本走不到这一步）。
   */
  const refs = createRefsStore({
    root: typeof store?.root === 'string' && store.root !== '' ? store.root : null,
    now,
  });

  /**
   * **脱敏的**错误日志路径（协议 §3：每个错误码都要带一条）。
   *
   * **它必须指向一个真存在的文件**——指向一个不存在的路径就是骗人。所以本插件
   * **自己写这份日志**：`<实例根>/official-chat/logs/official-chat.log`。
   *
   * **脱敏**（协议 §5：不在代码里展开真实用户 home）：对外一律是
   * `<instance>/…` 的占位形态，绝不把真实家目录拼进要外发的字符串。
   */
  const ERROR_LOG_RELATIVE = 'official-chat/logs/official-chat.log';
  const ERROR_LOG_PATH = redactLogPath(ERROR_LOG_RELATIVE);

  /**
   * 记一行失败原因到日志文件，**并**返回脱敏路径。
   *
   * **只写分类后的稳定事实**（码 + requestId + detail）：不写正文、不写异常串
   * ——协议 §3 说内部异常串不进给用户看的那一层；日志里也只留**可分类**的东西，
   * 否则日志本身就会变成泄露源。
   *
   * **记日志失败不许盖掉真正的失败原因**：那才是用户要看的。写不进去就跳过
   * （路径照样给出去，因为它指向的是「这类失败记在哪」的约定位置）。
   *
   * @param {string} code - 稳定码名。
   * @param {string} requestId - 关联请求 id。
   * @param {string|null} detail - 脱敏后的原因线索。
   * @returns {string} 脱敏日志路径。
   */
  const logFailure = (code, requestId, detail) => {
    try {
      const root = typeof store?.root === 'string' ? store.root : '';
      if (root !== '') {
        const dir = join(root, 'logs');
        mkdirSync(dir, { recursive: true, mode: 0o700 });
        const line = `${JSON.stringify({
          at: new Date(now()).toISOString(),
          code: String(code ?? ''),
          requestId: String(requestId ?? ''),
          detail: detail === null || detail === undefined ? null : String(detail),
        })}\n`;
        appendFileSync(join(dir, 'official-chat.log'), line, { mode: 0o600 });
      }
    } catch {
      // **写不进去就算了**：真正要报的是失败原因，不是日志。
    }
    return ERROR_LOG_PATH;
  };

  /**
   * 造一个错误负载（协议 §3 的形状 + **脱敏日志路径**）。
   *
   * **命令层所有失败都走这里**：命令层的失败是**真实发生**的失败，所以必须带
   * §3 要求的四个字段——可重试性、用户下一步、关联 `requestId`、脱敏日志路径。
   *
   * @param {string} code - 稳定码名。
   * @param {{requestId?: string, detail?: string|null}} [extra] - 关联信息。
   * @returns {object} 错误负载。
   */
  const fail = (code, extra = {}) => {
    const requestId = String(extra.requestId ?? '');
    const detail = extra.detail ?? null;
    return errorPayload(code, {
      requestId,
      detail,
      logPath: logFailure(code, requestId, detail),
    });
  };

  /**
   * **来源指纹**（协议 §8-6：指纹比对的校验点在 **host 命令层**）。
   *
   * 指纹回答的是「这份快照还是不是**同一个来源**取出来的」——账号换了、实例
   * 换了、来源变了，就该拒（`account_changed`），不得放行旧正文。
   *
   * **取值只来自服务端**：客户端传的会话标识与指纹**不构成授权**（协议 §5），
   * 所以请求里的 `sourceFingerprint` 一律**不采信**。
   *
   * **不可得 ⇒ 拒**：空指纹不是「一样」，是「不知道」——协议要求此时按
   * `account_changed` 拒，**不得放行**。
   *
   * **为什么取哈希**：`accountScope` 可能是实例 home 这类路径，而落盘物里
   * **不许**出现用户目录（仓库有专门判据），所以只留摘要。
   *
   * @param {string} scope - 服务端推导的账户/实例范围。
   * @returns {string} 指纹；空串表示**不可得**。
   */
  const fingerprintOf = (scope) => {
    const text = String(scope ?? '').trim();
    if (text === '') return '';
    return createHash('sha256').update(text, 'utf8').digest('hex').slice(0, 32);
  };

  /** 在途请求：`requestId` → 取消函数（S-2：取消后的迟到结果不得创建快照）。 */
  const inFlight = new Map();

  /**
   * 给一个目录项**预分配**一个快照 id（协议 v0.2 S-1：id 分配权归 host）。   *
   * **为什么分配发生在目录这一步**：内核的 `onPick` 是**同步**的（协议 S-1 的
   * id 必须在 pick 那一刻就成形），而 RPC 是 Promise——pick 那一刻问不到 host。
   * 于是分配挪到**更早的异步一步**：`catalog` 回目录页时顺带把 id 分配好，
   * 客户端把它放进候选行，pick 时直接取用。
   *
   * @param {string} sessionId - 官网会话 id。
   * @returns {string} 快照 id（同时就是 `ref`）。
   */
  function allocateFor(sessionId) {
    if (seqAllocator === null || typeof seqAllocator.reserve !== 'function') {
      throw new Error('序号分配器没接上：id 分配权归 host，host 必须自己分配');
    }
    return seqAllocator.reserve(sessionId).snapshotId;
  }

  /**
   * `capabilities`：把协议常量、错误码表、状态机、降级标记一次性交给客户端。
   *
   * **客户端据此不必再自带一份协议常量**——单一真相在 host 这边。
   */
  function capabilities() {
    return {
      ok: true,
      schemaVersion: SCHEMA_VERSION,
      providerId,
      constants: { ...CONSTANTS },
      errorCodes: Object.fromEntries(
        Object.entries(ERROR_CODES).map(([code, spec]) => [code, { ...spec }]),
      ),
      states: { ...STATES },
      /**
       * 降级标记：实现**不得假设** G1-1…G1-6 已解决（协议 §8-12）。
       *
       * **协议 §8-12 要求的六个都要登记**（原文是「G1-1…G1-6 缺口登记仍在」）：
       * G1-1 = `onPick` 异步化；G1-2 = 发送前的「引用未就绪」闸门；
       * G1-3 = fresh span 开放给插件；G1-4 = per-plugin 注册回执与可见诊断；
       * G1-5 = 插件重载后既有 chip 的 ref 恢复契约；
       * G1-6 = 内核无「chip 进入 `invalid` 态」的写入口。
       * 逐条说明见 `docs/features/official-chat/official-chat-feasibility.md` §3 缺口表。
       *
       * **G1-7 是本轮真机实测出的上游缺口，协议 §8-12 尚未收录（待 PM 改稿）**：
       * 在这里登记它，是为了让 `capabilities` **如实反映「通道在本内核上不通」**——
       * 否则客户端以为一切正常，直到真机调用才撞 HTTP 405。
       */
      degraded: {
        g1_1: 'onPick 仍是同步的 → 句柄是 pending 态（selected），不是「就绪」；落盘与就绪在发送路径上判定',
        g1_2: '发送前的「引用未就绪」闸门仍缺：无 chip 时内核把纯文本直接发出去（P1 靠 chip 在场保证不漏发）',
        g1_3: 'fresh span（InputActions.captureInsertion）仍未开放给客户端插件',
        g1_4: 'per-plugin 注册回执与「客户端半边注册失败时用户可见的诊断」仍缺',
        g1_5: '插件重载后既有 chip 的 ref 恢复契约仍缺（失效 chip 需用户重选）',
        g1_6: '内核无「chip 进入 invalid 态」的写入口（ReferenceInsert 无 invalid 字段、setInvalid 零调用方、DOM 无 data-invalid）→ 本阶段按「发送时拒 + 可见错误」验收，该缺口不因本阶段验收通过而关闭',
        g1_7: '**上游内核缺口（0.2.1-rc.2 真机实测）**：`dsh-client-connection@0.2.1-rc.2` 的 `owner.webServer.register(route)`（lib/index.js:871）是**属性访问**——cordis 的服务查找落在 connection 插件自己的 fiber scope（`ctx[symbols.shadow]`），webServer 在那里不可解析 ⇒ 第三方插件调 `rpc.handle` 抛 `cannot get property "webServer" without inject` ⇒ **host 半边不激活、RPC 通道整体不通**（浏览器侧表现为 HTTP 405 而非 404）。**插件侧无法自修**；内核改一行 `owner.get("webServer")`（与内核自己在 :745/:778 的写法一致）即可——隔离副本注入已验证有效。协议 §8-12 尚未收录本条，待 PM 改稿',
        // 以下三项不是 G1-* 缺口，同属「实现不得假设已解决」的降级项：
        capture_source:
          '**真实站点取数已接（P4）**：目录那条走 `sessionStructure`（现役回环桥 ' +
          '`POST /v1/official-chat/session-structure`，拿真实会话目录结构 ＋ 目录项面，' +
          '协议 §4.3 ＋ §2.4）；**正文**那条走 `sessionBody`（`POST /v1/official-chat/session-body`，' +
          '协议 §4.4：壳侧导航到那条会话的深链 ⇒ 站点自己发 `history_messages` ⇒ ' +
          '壳侧取体并裁成「角色 ＋ 文本」）。⚠️ **零写入口径**：**壳侧只做导航、一个写请求都不发**；' +
          '但**导航时站点自己**会跑初始化批次（含 `chat_session/create`）⇒ ⇒ 那是**站点落地即发、' +
          '不是深链特有** ⇒ ⇒ **一个字不许写成「零写入」**；⚠️ **界面不据此写任何一句**' +
          '（PM 2026-10-22 08:3x 裁：那一档已撤）',
        detail_panel: 'preview 只回内容；详情面板属 P5，本期不做',
        body_dedup: '同一正文引用两次各送一份（S-7）；正文去重属 P2',
      },
      paths: {
        catalogCache: redactLogPath('catalog-cache'),
        snapshots: redactLogPath('snapshots'),
      },
      /**
       * 本实例已用掉的 pick 序号（会话键 → 最大序号）。
       *
       * **两份取大**：序号**认领**（分配器，`seq-claims/`）与快照**落盘**
       * （`snapshots/`）是同一条「用掉」的两种记录，取大 = 不会因为其中一份
       * 缺了而让序号倒退。
       *
       * **客户端已经不用它了**（协议 v0.2 S-1：id 分配权归 host，client 不得
       * 自造 seq）——留在这里是为了**可观测**：对端能看到这个实例用到了第几号。
       */
      usedSeqs: (() => {
        const claimed = seqAllocator?.usedSeqs?.() ?? {};
        const stored = typeof store.listSessionSeqs === 'function' ? store.listSessionSeqs(providerId) : {};
        const merged = { ...claimed };
        for (const [key, seq] of Object.entries(stored)) {
          if (Number(seq) > (merged[key] ?? 0)) merged[key] = Number(seq);
        }
        return merged;
      })(),
    };
  }

  /**
   * `catalog`：列会话目录。**不含正文**（§1.2）。
   *
   * **顺带回预分配的快照 id**（协议 v0.2 S-1：id 分配权归 host）：客户端的
   * `onPick` 是**同步**的（内核不等 Promise），那一刻问不到 host ⇒ 分配必须
   * 发生在**更早的异步一步**，也就是这一次目录查询。
   *
   * **`allocations` 在 `page` 之外**：`page` 是 §1.2 的目录页（**可缓存**，
   * 5 分钟），而 id **每次都要新分配**（同一份缓存目录页不能带同一个 id
   * 反复用——那会让第二次 pick 撞上第一次的 id）。两者分开 = 缓存物与
   * 一次性分配物各归各的。
   *
   * @param {{query?: string, cursor?: string, accountScope?: string}} request
   */
  async function catalog(request = {}) {
    // **账户维度由服务端推导**（协议 P3 §S-15）：请求里的 `accountScope`
    // **只做一致性校验**、**不构成授权** ⇒ 不符即拒（`account_changed`）、
    // **不静默改写**（§S-15 裁定 4）。**没传就放行**。
    const scope = String(accountScope);
    const scopeMismatch = accountScopeMismatch(request.accountScope, scope);
    if (scopeMismatch !== null) {
      return {
        ok: false,
        ...fail('account_changed', { requestId: request.requestId, detail: scopeMismatch }),
      };
    }
    // **保留期主动清理**（协议 §4.1）：**惰性触发**，**只在 `catalog` 入口**，
    // 同实例至多一天一次（节流键 `<root>/sweeps/last-sweep-<日期>.json`）。
    //
    // **不阻塞业务**：清理失败只记日志（清理器自己兜），这里也再兜一层——
    // 清理是「 housekeeping」，**绝不能因为它把用户的目录查询搞挂**。
    try {
      sweeper?.maybeSweep?.();
    } catch (error) {
      // 吞掉：清理失败与「目录查不到」是两件事，不该混在一起报错。
    }
    const query = String(request.query ?? '');
    // **「当刻重取」的那一次**（协议 §4.6 修法 3 ③）：客户端判定「用户刚打开
    // 候选组」时带 `refresh` ⇒ ⇒ **绕开本页缓存**，并让**壳侧重取一轮**。
    //
    // ⚠️ **只有那一次带 `refresh`** ⇒ ⇒ 连续打字期间**不会**每键都重取
    // （壳侧重取一轮实测 ≈ 6 秒）。
    const refresh = request.refresh === true;
    // **站点短码**（客户端给的「我是哪个站」）—— **缓存键的一段**。
    //
    // ⚠️ **取数在缓存读之后** ⇒ ⇒ 键里必须有站 ⇒ ⇒ 否则切站那一下会命中上一个站
    // 的缓存页（用户看到别的站的会话）。认不出 ⇒ 空串 ⇒ 落 `site-default`
    // （**稳定的一段**，绝不让空串把两个站撞到同一个键上）。
    const site = String(request.site ?? '');
    const cached = refresh ? null : catalogCache.read(providerId, site, scope, query);
    let page;
    if (cached !== null) {
      page = cached;
    } else {
      let listed;
      // **来源标识**（`source`）：这一页的目录项**来自哪儿**。
      // 真实源 ⇒ `site` ＋ 站点短码（**内核给的**）；夹具 ⇒ `fixture`。
      let sourceKind = 'fixture';
      let sourceId = 'fixture';
      if (siteSource !== null && typeof siteSource.structure === 'function') {
        // **真实源**（P4）：目录页的会话目录**不再**用固定夹具。
        //
        // 走的是 `sessionStructure` 那条命令（协议 §2.4 面二：目录项面就挂在
        // **它**的回程载荷上 —— P1 协议 v0.12 §5）⇒ ⇒ 目录项由**站点自己**的
        // 会话目录产出，插件侧只做**逐字段映射**（`catalogPageItem`）。
        let result;
        try {
          result = await siteSource.structure({
            accountScope: scope,
            refresh,
            // ⚠️ **站点短码**（PM 2026-10-22 13:4x 裁）：告诉壳侧「这次取的是
            // 哪个站」⇒ ⇒ 认不出 ⇒ **不带**（壳侧会按默认站走）。
            site,
          });
        } catch {
          // **不把内部异常串带出去**（§3）：只给稳定码。
          return { ok: false, ...fail('catalog_failed', { requestId: request.requestId }) };
        }
        if (result?.ok !== true) {
          // 站内码**原样上报**（协议 §6.2）；闭集外的码一律归 `catalog_failed`。
          const raw = String(result?.code ?? '');
          const code = ERROR_CODE_NAMES.includes(raw) ? raw : 'catalog_failed';
          return {
            ok: false,
            ...fail(code, {
              requestId: request.requestId,
              detail: String(result?.detail ?? ''),
            }),
          };
        }
        const rawItems = result.structure?.items;
        listed = {
          ok: true,
          items: (Array.isArray(rawItems) ? rawItems : []).map(catalogItem),
          hasMore: Boolean(result.structure?.hasMore),
        };
        // **站点短码**（内核在桥回包信封上给的那个）⇒ 认不出就是空串（**不猜**）。
        sourceKind = 'site';
        sourceId = String(result.site ?? '');
      } else {
        // **没有真实源**（没装壳的测试环境）⇒ 用固定夹具。
        //
        // ⚠️ 产品装配里**永远有**真实源（`host/index.js` 注入 `siteSource`）⇒
        // ⇒ 这条分支**只**服务判据与单测（那里没有壳、没有登录态）。
        try {
          listed = await captureSource.list({ accountScope: scope, query, cursor: request.cursor });
        } catch {
          // **不把内部异常串带出去**（§3）：只给稳定码 + 分类信息。
          return { ok: false, ...fail('catalog_failed', { requestId: request.requestId }) };
        }
        if (listed?.ok === false) {
          return { ok: false, ...fail(listed.code ?? 'catalog_failed', { requestId: request.requestId }) };
        }
      }
      page = {
        accountScope: scope,
        providerId,
        items: listed.items ?? [],
        cursor: String(request.cursor ?? ''),
        hasMore: Boolean(listed.hasMore),
        fetchedAt: new Date(now()).toISOString(),
        // **来源标识**（顶层一个字段）：界面据此显示「DeepSeek」还是「固定夹具」。
        source: buildCatalogSource(sourceKind, sourceId),
      };
      const check = validateCatalogPage(page);
      if (!check.ok) {
        return {
          ok: false,
          ...fail('catalog_failed', { requestId: request.requestId, detail: check.errors.join('、') }),
        };
      }
      catalogCache.write(providerId, site, scope, query, page);
    }
    // **分配 id**：每个目录项一个。分配失败 ⇒ 整个 catalog 失败——不能给出
    // 「点了必定失败」的候选行。
    const allocations = {};
    for (const item of page.items ?? []) {
      // 目录项的**会话 id 键名是 `id`**（协议 P4 §2.4 面二 / §4.3 ② 的 6 个键）。
      const sessionId = String(item?.id ?? '');
      if (sessionId === '') continue;
      try {
        allocations[sessionId] = allocateFor(sessionId);
      } catch (error) {
        const raw = String(error?.code ?? '');
        // **内部码不许漏给对端**（协议 §3：14 个码是闭集）。这里归
        // **`catalog_failed`**（目录这一步没做完，**可重试**）——
        // **不归 `source_unsupported`**：那个码只表示「这个来源根本不支持
        // 该能力」，拿来兜「存储不可用」会让用户去「换一个来源」，
        // 而真正该做的是**重试**。
        const code = ERROR_CODE_NAMES.includes(raw) ? raw : 'catalog_failed';
        return {
          ok: false,
          ...fail(code, {
            requestId: request.requestId,
            detail: raw === '' ? '序号分配失败' : raw,
          }),
        };
      }
    }
    return { ok: true, page, cached: cached !== null, allocations };
  }

  /**
   * `sessionStructure`：**真实站点**的会话目录**结构**（P4 协议 §4.3 的字段面 ＋
   * §2.4 的白名单回程）。
   *
   * ## 它与 `catalog` 的分工
   *
   * · `catalog` ＝ **目录页**（目录项 ＝ 协议 P4 §2.4 面二那 6 个键；游标在顶层）
   *   ——P1 §1.2 的形状；
   * · `sessionStructure` ＝ **结构 ＋ 目录项面**（键名 / 条数 / 类型 / 布尔量 ＋
   *   目录项的值）——P4 §4.3 的形状。
   *
   * ⚠️ **P1 §1.2 里的「真实深链」与「游标」这两格，本轮目录页上仍是空的**：
   * · **`deepLink`** —— **PM 已裁**（协议 **v0.22 §2.4** 已解禁为面二的**可选第 7
   *   键**，类型 `string`）⇒ ⇒ 但**解禁是「许可」不是「义务」** ⇒ ⇒ **本轮不产出**
   *   （**没有消费方**：用户侧不上屏；正文那条链**自己造**深链）⇒ ⇒ 目录项里
   *   **仍然没有** `deepLink`；
   * · **`cursor`** —— 来源**仍未坐实**（缺口 G4-8）⇒ ⇒ 顶层 `cursor` 仍是空串。
   *
   * ⇒ ⇒ 两条**分开记**：一条已解禁但本轮不产出，一条还欠坐实 —— **不编造**。
   * ⚠️ **将来真有消费方时三面一起改**（契约表 ＋ Rust 侧目录项 ＋ 插件侧构造），
   * 形态按协议 v0.22：**不许带 query／fragment／别的段**。
   *
   * ## 目录页现在**已经**走真实来源了（v0.17 之后）
   *
   * v0.16 之前协议 §2.4 的白名单**禁止**回传会话 id 与标题 ⇒ 那时目录页只能
   * 用固定夹具，并在 `capabilities.degraded.capture_source` 里**如实标注**。
   * **v0.17 增了目录项面**（§2.4 面二 / §4.3 ②：每项恰好 6 个键）⇒ ⇒ 会话 id
   * 与标题**允许**出现在目录项里 ⇒ ⇒ **目录页改走这条真实路径**
   * （见上面 `catalog` 的取数分支：真实源失败就**如实失败**，不回退夹具）。
   *
   * ⚠️ 本命令**仍然**在真实源没接上时报 `source_unsupported`（**不静默降级**）——
   * 「目录页有数据」与「本命令能跑」是两回事。
   *
   * @param {{requestId?: string, accountScope?: string, site?: string}} request
   */
  async function sessionStructure(request = {}) {
    const requestId = String(request.requestId ?? '');
    // **账户维度由服务端推导**（协议 P3 §S-15）：同 `catalog` —— 客户端传的
    // `accountScope` **只做一致性校验、不构成授权**。
    const scope = String(accountScope);
    const scopeMismatch = accountScopeMismatch(request.accountScope, scope);
    if (scopeMismatch !== null) {
      return { ok: false, ...fail('account_changed', { requestId, detail: scopeMismatch }) };
    }
    if (siteSource === null || typeof siteSource.structure !== 'function') {
      return {
        ok: false,
        ...fail('source_unsupported', { requestId, detail: '真实站点取数源没接上' }),
      };
    }
    let result;
    try {
      result = await siteSource.structure({
        accountScope: scope,
        // ⚠️ **站点短码**（PM 2026-10-22 13:4x 裁）：认不出 ⇒ **不带**。
        site: request.site ?? '',
      });
    } catch {
      // **不把内部异常串带出去**（协议 §3）：只给稳定码。
      return { ok: false, ...fail('catalog_failed', { requestId }) };
    }
    if (result?.ok === false) {
      // 站内码**原样上报**（协议 §6.2：不解释语义）；闭集外的码一律归
      // `catalog_failed`（**不新增第 15 个码** —— 协议 §6.1），原码留在 detail 里。
      const raw = String(result.code ?? '');
      const code = ERROR_CODE_NAMES.includes(raw) ? raw : 'catalog_failed';
      const detail = String(result.detail ?? raw);
      return {
        ok: false,
        ...fail(code, { requestId, detail: detail === '' ? null : detail }),
      };
    }
    return { ok: true, structure: result.structure };
  }

  /**
   * 取一次**正文**。
   *
   * ## 真实源在场 ⇒ 走真实源（协议 §4.4）
   *
   * 真实源（`siteSource.body`）走**现役回环桥**（`POST /v1/official-chat/session-body`）
   * ⇒ 壳侧开一扇**自己家的不可见窗** → 订阅 `Network` 域 → 导航到那条会话的**深链**
   * ⇒ ⇒ **站点自己**发那条 `GET …/chat/history_messages?chat_session_id=<uuid>`
   * ⇒ ⇒ 壳侧取体并**裁成「角色 ＋ 文本」** ⇒ ⇒ **壳侧零请求**、**页内零注入**。
   *
   * ## 真实源不在场 ⇒ 固定夹具
   *
   * **只**服务判据与单测（那里没有壳、没有登录态）⇒ ⇒ 产品装配里**永远**走真实源
   * （`host/index.js` 注入 `siteSource`）。
   *
   * @param {string} sessionId - 官网会话 id。
   * @param {string} scope - 服务端推导的账户范围。
   * @param {string} [site] - 站点短码（**认不出 ⇒ 不带**；PM 2026-10-22 13:4x 裁）。
   * @returns {Promise<object>} 取数源的结果（`{ok:true, messages, captureResult}` 或
   *   `{ok:false, code, detail}`）。
   */
  async function fetchBody(sessionId, scope, site = '') {
    if (siteSource !== null && typeof siteSource.body === 'function') {
      return siteSource.body({ sessionId, accountScope: scope, site });
    }
    return captureSource.fetch({
      sessionId,
      accountScope: scope,
    });
  }

  /**
   * `capture`：捕获一份正文并**落盘**。
   *
   * 落盘成功后**句柄状态由快照文件派生**（S-3）——本函数不另存内存状态。
   *
   * **入参是 host 自己分配的 id**（协议 v0.2 S-1：id 分配权归 host，client
   * 不得自造 seq）：客户端把 `catalog` 拿到的 `snapshotId` 原样带回来，host
   * **重新核对**——形态、provider 段、会话段（必须等于该官网会话 id 的短码）、
   * 以及「这个序号是不是 host 认领过的」。**核对不过就拒**，绝不因为客户端
   * 报了个 id 就照单全收。
   *
   * @param {{sessionId: string, snapshotId: string, requestId?: string, accountScope?: string}} request
   */
  async function capture(request = {}) {
    const requestId = String(request.requestId ?? '');
    // **账户维度由服务端推导**（协议 P3 §S-15）：同 `catalog` —— 客户端传的
    // `accountScope` **只做一致性校验**，不符即拒（`account_changed`）、
    // **不静默改写**；**没传就放行**。
    const scope = String(accountScope);
    const scopeMismatch = accountScopeMismatch(request.accountScope, scope);
    if (scopeMismatch !== null) {
      return {
        ok: false,
        ...fail('account_changed', { requestId, detail: scopeMismatch }),
      };
    }
    const sessionId = String(request.sessionId ?? '');
    const requestedId = String(request.snapshotId ?? '');

    // **先核对 id，再取正文**：id 不合法就不该去打扰来源。
    const parsed = parseSnapshotId(requestedId);
    if (parsed === null) {
      return {
        ok: false,
        ...fail('snapshot_invalid', { requestId, detail: '快照 id 形态不合法' }),
      };
    }
    if (parsed.providerShort !== providerShort) {
      return {
        ok: false,
        ...fail('snapshot_invalid', { requestId, detail: 'provider 段与本来源不符' }),
      };
    }
    const sessionShort = makeSessionShort(sessionId);
    if (parsed.sessionShort !== sessionShort) {
      return {
        ok: false,
        ...fail('snapshot_invalid', { requestId, detail: '会话段与官网会话 id 不一致' }),
      };
    }
    if (seqAllocator === null || typeof seqAllocator.isClaimed !== 'function') {
      return {
        ok: false,
        ...fail('snapshot_invalid', { requestId, detail: '序号分配器没接上' }),
      };
    }
    if (!seqAllocator.isClaimed(sessionShort, parsed.seq)) {
      return {
        ok: false,
        ...fail('snapshot_invalid', { requestId, detail: '序号不是 host 分配的' }),
      };
    }
    // **当前指纹不可得 ⇒ 拒**（协议 §8-6）：不知道这份正文是不是同一个来源
    // 取出来的时，不得落盘、更不得放行。
    const fingerprint = fingerprintOf(accountScope);
    if (fingerprint === '') {
      return {
        ok: false,
        ...fail('account_changed', { requestId, detail: '当前来源指纹不可得' }),
      };
    }

    // **正文这一格现在接的是真实源**（协议 §4.4：深链 ⇒ 站点自己发那条
    // `history_messages` ⇒ 壳侧取体 ⇒ 裁成「角色 ＋ 文本」）。
    //
    // ## 为什么这里**不再**报「这一格还没接通」
    //
    // 那一格曾经被**故意挡着**（协议 §4.4 的前置 G4-20 未坐实）⇒ ⇒ 真实源在场时
    // 必失败 `source_unsupported` ＋ 一句「正文取得机制尚未坐实」。
    // **G4-19／G4-20 已坐实**（深链形态 ＋ 深链导航后站点确实发那条请求）⇒ ⇒
    // 这一格**换成真实取数** ⇒ ⇒ **那句机器标记不再挂**（挂错了就是骗用户：
    // 用户会以为「这功能还没做」，而它其实已经能取到正文）。
    //
    // ## 失败怎么落
    //
    // 真实源报什么就按**既有码**报（协议 v0.3 §3 的分流：超时 → `timeout`、
    // 结构变了 → `schema_changed`、站内码 → 原样上报）⇒ ⇒ **不新增第 15 个码**，
    // 也**不拿 `source_unsupported` 兜「捕获失败」**（那会让用户去「换一个来源」，
    // 而真正该做的是**重试**）。

    let cancelled = false;
    const cancel = () => {
      cancelled = true;
    };
    inFlight.set(requestId, cancel);
    try {
      let fetched;
      try {
        // ⚠️ **站点短码**（PM 2026-10-22 13:4x 裁）随请求体给壳侧 ⇒ ⇒ 认不出
        // ⇒ **不带**（壳侧按默认站走）。
        fetched = await fetchBody(sessionId, scope, request.site ?? '');
      } catch {
        return { ok: false, ...fail('timeout', { requestId }) };
      }
      if (cancelled) {
        // S-2：取消后的迟到结果**不得创建可引用快照**。
        return { ok: false, ...fail('cancelled', { requestId }) };
      }
      if (fetched?.ok === false) {
        // **捕获失败按原因分流**（协议 v0.3 §3「各码的使用边界」）：
        // 捕获源报了什么原因就用什么码——超时 → `timeout`（可重试）、
        // 官网页面结构变了 → `schema_changed`、用户取消 → `cancelled`、
        // 来源明说不支持该能力 → `source_unsupported`。
        // **原因不明 → `timeout` 兜**（可重试）。
        //
        // **不得用 `source_unsupported` 兜「捕获失败」**——那会让用户看到
        // 「换一个来源」而其实该**重试**，属于**掩盖真实原因**。
        const reason = String(fetched.reason ?? fetched.code ?? '').trim().toLowerCase();
        // ⚠️ **来源自己给的 `detail` 优先**（那是**已经分类好的事实** —— 壳侧
        // 给的「回程失败：…」「站内码 40003 INVALID_TOKEN」这类线索）⇒ ⇒
        // 界面拿到的就是**能行动的原因**，不是一句空泛的「捕获失败原因：…」。
        // **来源没给** ⇒ 才退到 `captureFailureDetail(reason)`（闭集内的原因词）。
        const sourceDetail = String(fetched.detail ?? '').trim();
        return {
          ok: false,
          ...fail(captureFailureCode(reason), {
            requestId,
            detail: sourceDetail === '' ? captureFailureDetail(reason) : sourceDetail,
          }),
        };
      }
      const resolved = resolveCase({
        kind: 'capture',
        schemaVersion: SCHEMA_VERSION,
        providerId,
        // ⚠️ **provider 段必须与分配器同源**（协议 v0.2 S-1）：分配器按
        // `makeProviderShort(providerId)` 造 id，落盘那份**也必须**按同一个短码
        // 造 ⇒ ⇒ **落盘的 id ＝ 分配出去的 id** ⇒ ⇒ 客户端拿句柄去 `preview`
        // 才读得到（`buildSnapshot` 默认的短码是协议常量 `ds`——provider id
        // 一旦不是它，两处就会**悄悄对不上**）。
        providerShort,
        accountScope: scope,
        sessionId,
        branch: 'main',
        seq: parsed.seq,
        messages: fetched.messages ?? [],
        captureResult: fetched.captureResult ?? { status: 'ok', omitted: [], unsupported: [] },
        capturedAt: new Date(now()).toISOString(),
        sourceUpdatedAt: new Date(now()).toISOString(),
        sourceFingerprint: fingerprint,
      });
      if (resolved.kind === 'error') {
        return { ok: false, ...fail(resolved.code, { requestId, detail: resolved.detail }) };
      }
      try {
        // **落盘**（协议 §2 S-3：单一事实源 = 快照文件）。存储不可用 / 同 id
        // 已存在 → 存储层抛带**稳定码**的错误，这里原样转成协议错误负载。
        store.save(resolved.snapshot);
      } catch (error) {
        // 协议 §3 的 14 个码是**闭集**：存储层的内部码（例如「同 id 已存在」用的
        // `snapshot_exists`）**不在表里** → 归到协议码，不许把协议外的码漏给对端。
        // 原始码留在 `detail`（它是稳定语义，不是异常串，可以外发）。
        const raw = String(error?.code ?? '');
        const isProtocolCode = ERROR_CODE_NAMES.includes(raw);
        const code = isProtocolCode ? raw : 'snapshot_invalid';
        // ⚠️ **区分两类**（PM 2026-10-21 裁）：
        //  · **`raw` 本身是协议码** ⇒ `detail` 保留 `error.detail` —— 那是**我们自己
        //    产出的分类事实**（第 14 码那处的三个数：正文字节／预算字节／超出量），
        //    **不是异常串** ⇒ ⇒ 界面文案要的就是它（丢了就只剩无数字的降级文案）；
        //  · **`raw` 是内部码**（如 `snapshot_exists`）⇒ `detail` **仍取 `raw`**
        //    （原始码保留在 `detail`，P1 §3 末条）⇒ ⇒ **那个异常即便带了
        //    `error.detail` 也不许用**（防修过头：内部异常串不许外发）。
        //  ⇒ 老码与其它码的 `detail` 行为**一个字不变**（没有 `detail` 的协议码
        //    仍退回「原始码」）。
        const rawDetail = error?.detail;
        const detail =
          isProtocolCode && typeof rawDetail === 'string' && rawDetail !== ''
            ? rawDetail
            : raw === ''
              ? null
              : raw;
        return { ok: false, ...fail(code, { requestId, detail }) };
      }
      // **写占用记录**（协议 §S-11 裁定 1）：**快照发布之后**立即写，
      // 且**先写占用记录、再对外宣告「已保存」**。
      //
      // **为什么必须在快照发布之后**：占用关系指向的是一份**已存在**的快照
      // ⇒ 快照没落成就不该有占用记录（判据 P3-4②：快照不存在时不应有占用记录）。
      // **为什么必须在宣告之前**：对外说「已保存」的那一刻，这份引用**要受保护**
      // ⇒ 占用关系得先在盘上。
      if (refs.available) {
        try {
          refs.write({
            snapshotId: resolved.snapshot.snapshotId,
            capturedAt: resolved.snapshot.capturedAt,
            createdAt: new Date(now()).toISOString(),
            sessionId: resolved.snapshot.sessionId,
          });
        } catch {
          // **写不进去 ⇒ 不宣告「已保存」**（§S-11 裁定 1 的顺序要求）：
          // 快照已经在盘上（发布点已过），但占用关系没落 ⇒ 这次引用**不受保护**
          // ⇒ 如实报失败（协议内的码），并留一行日志（`fail` 内部会记）。
          return {
            ok: false,
            ...fail('snapshot_invalid', { requestId, detail: '占用记录写入失败' }),
          };
        }
      }
      return { ok: true, snapshot: resolved.snapshot, notice: resolved.notice };
    } finally {
      inFlight.delete(requestId);
    }
  }

  /**
   * `cancel`：取消在途捕获。
   *
   * @param {{requestId: string}} request
   */
  function cancel(request = {}) {
    const requestId = String(request.requestId ?? '');
    const fn = inFlight.get(requestId);
    if (fn !== undefined) {
      fn();
      return { ok: true, cancelled: true };
    }
    return { ok: true, cancelled: false };
  }

  /**
   * **R1 显式释放**（协议 §S-11 裁定 2 第 1 条 · v0.5）：删掉某份快照的**占用记录**。
   *
   * ## 命令名写死为 `release`（协议 v0.5 §S-11）
   *
   * 协议把释放入口**写死成 host 命令 `release`**（入参 `{ snapshotId }`；成功返回
   * `{ ok: true, released: boolean }`；**幂等**；**不新增错误码**）⇒ 本函数就是
   * 那条命令的实现，**名字必须叫 `release`**（判据按这个名字调）。
   *
   * ## 为什么**不能**复用现成的 `cancel`
   *
   * `cancel` 的语义是「**取消在途捕获**」（按 `requestId`）⇒ 与「释放**已落盘
   * 快照**的占用记录」**不是同一件事** ⇒ 复用会让「取消一次抓取」意外删掉一份
   * 占用记录（协议 v0.5 明写）。
   *
   * ## 本阶段**只有这一条**可判定的释放形态
   *
   * R1 原写的触发条件（「用户显式移除 / 该次 pick 被取消」）在**当前内核契约下
   * 不可判定**——内核 source 契约里**没有**「引用被移除 / pick 被取消」的信号
   * （协议 §S-12 已逐条核过）⇒ 本阶段 R1 **只有「显式释放调用」这一条**。
   * ⇒ **client 侧本批不接线**（现役 client 也没有移除 chip 的入口；造那个 UI
   * 是 P4/UX 的活，超出本单）⇒ **也不进 rpc 的客户端端点表**（`host/rpc.js` 的
   * `ENDPOINTS` 保持 6 个）。
   *
   * ## 入参形态
   *
   * **两种都收**：`release({ snapshotId })`（协议形态）或 `release('ds~xxx~0001')`
   * ——省得调用方为「这个入口到底接字符串还是接请求对象」反复试。
   *
   * @param {string | {snapshotId?: string, requestId?: string}} input - 快照 id 或请求对象。
   * @returns {{ok: true, released: boolean} | object} 成功返回 `{ok:true, released}`。
   */
  function releaseOccupancy(input = {}) {
    const asObject = input !== null && typeof input === 'object';
    const requestId = asObject ? String(input.requestId ?? '') : '';
    const snapshotId = asObject ? String(input.snapshotId ?? '') : String(input ?? '');
    if (snapshotId === '') {
      // **不新增错误码**（协议 v0.5）：入参非法按**既有码**拒。
      return { ok: false, ...fail('snapshot_invalid', { requestId, detail: '快照 id 为空' }) };
    }
    // **删不掉就是没释放**（不假装）：`release` 只在真的删掉时才是 true。
    const released = refs.release(snapshotId);
    return { ok: true, released };
  }

  /**
   * `preview`：按**固定 id** 读快照（§5：host 回环路由只按固定 id 读快照）。
   *
   * **指纹比对在这一层**（协议 §8-6：校验点是 host 命令层，不是存储层）：
   * 当前来源指纹与快照里记的不一致 ⇒ 拒（`account_changed`）；当前指纹
   * **不可得** ⇒ 也拒（不知道是不是同一个来源，不得放行旧正文）。
   *
   * @param {{snapshotId: string, requestId?: string}} request
   */
  function preview(request = {}) {
    const id = String(request.snapshotId ?? '');
    const requestId = String(request.requestId ?? '');
    const fingerprint = fingerprintOf(accountScope);
    if (fingerprint === '') {
      return {
        ok: false,
        ...fail('account_changed', { requestId, detail: '当前来源指纹不可得' }),
      };
    }
    try {
      // **只按固定 id 读快照**（协议 §5）：路径由 id 决定，调用方不能另指目录。
      const loaded = store.load(id);
      // **指纹变了 ⇒ 拒**（协议 §8-6 / S-5）：这份正文不是当前来源取出来的。
      if (String(loaded.snapshot?.sourceFingerprint ?? '') !== fingerprint) {
        return {
          ok: false,
          ...fail('account_changed', { requestId, detail: '来源指纹已变' }),
        };
      }
      // **正文一起回**：模型输入的正文必须与盘上的内容逐字节相同（S-4），
      // 所以这里回的是 host **从文件读回来的那份**，不是另算出来的一份。
      return { ok: true, snapshot: loaded.snapshot, body: loaded.body, state: 'ready' };
    } catch (error) {
      return {
        ok: false,
        ...fail(String(error?.code ?? 'snapshot_invalid'), { requestId }),
      };
    }
  }

  /**
   * P2 协议 §2.2：`duplicates` —— **重复识别 ＋ 预算预检**（一次往返出两样）。
   *
   * **入参**：`{ sessionId, snapshotIds }`——`snapshotIds` 是 client 传来的
   * 「当前草稿里已选中的引用 id 列表」（协议 §2.2 明写：**host 不猜草稿**）。
   *
   * **出参**：协议 §2.2 的字段闭集（`ok` / `groups[]` / `budget{}`）。
   *
   * **失败口径**（协议 §2.3）四种情形 → 码。其中：
   * · `snapshot_store_unavailable`（盘上不可读）是**实现既有的内部码**
   *   （`host/index.js` 抛出），**不在** P1 §3 的 14 码闭集里 ⇒ 按 P1 §3 的
   *   规则**归入 `snapshot_invalid`**，原始码保留在 `detail`（`errorPayload`
   *   的未知码分支即此行为）。
   * · `invalid_request`（入参形态不对）**同样不在闭集里** ⇒ 同样归入
   *   `snapshot_invalid`、原码进 `detail`。**不新增码**（P2 §6.5）。
   * ⇒ 两处都**不引入闭集外的码**：闭集外码一律归 `snapshot_invalid`、原始码进
   * `detail`（P1 §3 `:167` 的既有规则；协议 v0.3 §2.3 已裁定「**不新增码**」，
   * §2.3「实现现状」已写明「**PM 认可，无需改代码**」）⇒ **调用点不再另做
   * 归一**：归一由 `errorPayload` 的未知码分支**统一兜底**（`detail` 形如
   * 「未知错误码 invalid_request」），判据 P2-6 断的就是这条留痕（`detail`
   * 必须带原始码，否则「入参形态错」与「快照过期」会压成不可区分的一坨）。
   *
   * **失败不阻塞发送**（协议 §2.3）：本命令失败只影响「提示」，正文照发。
   *
   * @param {{sessionId: string, snapshotIds: string[], requestId?: string}} request
   * @returns {Promise<object>} 协议 §2.2 的出参。
   */
  async function duplicates(request = {}) {
    const requestId = String(request.requestId ?? '');
    const sessionId = String(request.sessionId ?? '');
    const ids = request.snapshotIds;
    if (sessionId === '' || !Array.isArray(ids)) {
      return { ok: false, ...fail('invalid_request', { requestId }) };
    }
    // **指纹不可得 ⇒ 拒**（照 `preview` 的口径：不知道是不是同一个来源，
    // 不得拿旧正文当当前来源的内容）。
    const fingerprint = fingerprintOf(accountScope);
    if (fingerprint === '') {
      return { ok: false, ...fail('account_changed', { requestId, detail: '当前来源指纹不可得' }) };
    }
    const loaded = [];
    for (const raw of ids) {
      const id = String(raw ?? '');
      if (id === '') {
        return { ok: false, ...fail('invalid_request', { requestId }) };
      }
      try {
        // **只按固定 id 读快照**（协议 §5）：路径由 id 决定，调用方不能另指目录。
        const one = store.load(id);
        // **指纹变了 ⇒ 拒**（协议 §8-6 / S-5）。
        if (String(one?.snapshot?.sourceFingerprint ?? '') !== fingerprint) {
          return { ok: false, ...fail('account_changed', { requestId, detail: '来源指纹已变' }) };
        }
        loaded.push({
          id,
          snapshot: one.snapshot,
          bytes: Buffer.byteLength(String(one.body ?? ''), 'utf8'),
        });
      } catch (error) {
        return { ok: false, ...fail(String(error?.code ?? 'snapshot_invalid'), { requestId }) };
      }
    }
    // **分组**（协议 §1.1：五字段全等；§1.3：组只是事实分组，**成员不合并**）。
    const byKey = new Map();
    for (const one of loaded) {
      const key = groupKeyOf(one.snapshot);
      const bucket = byKey.get(key);
      if (bucket === undefined) byKey.set(key, [one]);
      else bucket.push(one);
    }
    const groups = [];
    for (const [groupKey, members] of byKey) {
      // **只报「重复组」（成员数 ≥ 2）**：单成员的组不是重复，报告它没有意义
      // （协议 §2.2 的 `redundantBytes` 对单成员恒为 0）。
      // **口径已定**（P2 协议 §2.2，PM 2026-10-20 23:5x 裁定）：`groups`
      // **只返回成员数 ≥ 2 的组** ⇒ 单成员组**不是重复组、不出现**
      // （否则 client 要自己过滤，「有没有重复」这个事实会被稀释）。
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
    // **组的顺序按 `groupKey` 升序**（协议 §3.2「多组形态」· PM 01:2x 裁定）：
    // 顺序抖动会让「后发那条」的文案不稳定 ⇒ 用户两次看到的文案可能不同 ⇒
    // 排成与**选中顺序无关**的定序。groupKey 是小写十六进制 ⇒ 按**字节序**比
    // （与 Rust 侧的 `sort_by` 口径一致）。
    groups.sort((a, b) => (a.groupKey < b.groupKey ? -1 : a.groupKey > b.groupKey ? 1 : 0));
    // **预算预检**（协议 §4）：`projectedBytes` = 各引用**各送一份**的总字节
    // （P1 S-7 口径）。**原生 `@session` 的占用本层不可观测**（host 没有草稿、
    // client 也拿不到草稿文本）⇒ 该项按 0 计 ⇒ 口径**偏保守**（可能少报一点）。
    const projectedBytes = loaded.reduce((sum, one) => sum + one.bytes, 0);
    const limitBytes = CONSTANTS.perMessageBudgetBytes;
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

  return {
    capabilities,
    catalog,
    sessionStructure,
    capture,
    cancel,
    preview,
    duplicates,
    buildSnapshot,
    /**
     * **R1 显式释放入口**（协议 §S-11 · v0.5）。
     *
     * **协议写死的名字是 `release`**（判据按这个名字调）⇒ `release` 是**正名**；
     * `releaseOccupancy` 是**同一个函数**的另一个名字（长名说清是什么）
     * ⇒ **不是两套逻辑**。
     */
    release: releaseOccupancy,
    releaseOccupancy,
  };
}
