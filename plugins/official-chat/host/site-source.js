/**
 * 官网对话「会话目录结构 ＋ 单会话正文」的**真实取数源**（P4 协议 §5 的接线侧）。
 *
 * ## 它是什么
 *
 * 壳侧那条路由（`POST /v1/official-chat/session-structure`）跑一轮真实取数，
 * 回来的**只有结构**：键名、条数、类型、布尔量（协议 §2.4 的白名单）。
 * 另一条路由（`POST /v1/official-chat/session-body`）跑一轮**正文**取数，
 * 回来的是**裁好的消息**（角色 ＋ 文本）—— **站点的响应体原文不出壳**。
 * 本文件是**内核侧**的客户端：把命令发给壳，把回包整理成协议形态。
 *
 * ## 三条纪律（与壳那一侧逐条对齐）
 *
 * · **只发一个命令名** —— 请求体是 `{"command":"sessionStructure"}`（正文那条是
 *   `{"command":"sessionBody","sessionId":"…"}`），**不接受任意 JS**
 *   （协议 §5.2 第 3 条：内核不许往壳里塞脚本）。
 * · **失败不抛** —— 统一回 `{ok:false, code, detail}`：取数失败不该让会话起不来。
 * · **不落凭据** —— 桥的令牌只在**请求头**里用一次（协议 §7.2）。
 *
 * ## 目录页（`catalog`）现在**用它**
 *
 * v0.16 之前 §2.4 的白名单**禁止**回传会话 id 与标题 ⇒ 目录页（那两样都要）
 * 只能用固定夹具。**v0.17 增了「目录项面」**（§2.4 面二 / §4.3 ②：每项恰好
 * 6 个键）⇒ ⇒ 会话 id 与标题**允许**出现在 `items` 的**元素内部**⇒ ⇒
 * 目录页**可以**由这条真实路径构造了。
 *
 * ⚠️ **本轮目录项里`不许`出现 `deepLink` 与 `cursor`**（协议 §2.4 面二：只回那 6
 * 个键）—— 但**两者的理由不同，要分开记**：
 * · **`deepLink`** —— **PM 已裁**（协议 **v0.22 §2.4** 已解禁为面二的**可选第 7 键**，
 *   类型 `string`）⇒ ⇒ **解禁是「许可」不是「义务」** ⇒ ⇒ **本轮不产出**
 *   （**没有消费方**：用户侧不上屏；正文那条链**自己造**深链）；
 * · **`cursor`** —— 来源**仍未坐实**（缺口 **G4-8**）⇒ ⇒ 顶层那个 `cursor` 仍是空串。
 *
 * @module xlink-official-chat/site-source
 */

import { request } from 'node:http';

import { CATALOG_ITEM_FIELDS, ERROR_CODE_NAMES } from './protocol.js';

/** 地址的环境变量名（壳在启动内核时注入）。 */
export const URL_ENV = 'DSH_XLINK_BUILTIN_BRIDGE_URL';
/** 令牌的环境变量名。 */
export const TOKEN_ENV = 'DSH_XLINK_BUILTIN_BRIDGE_TOKEN';

/** 桥上那条路由的路径。 */
export const ROUTE_PATH = '/v1/official-chat/session-structure';

/**
 * **正文**那条路由的路径（协议 §4.4）。
 *
 * ⚠️ **与目录那条分开**：目录＝结构（键名／条数／类型），正文＝消息（角色 ＋ 文本）
 * —— 两条路由**各收各的命令名**（协议 §5.2 的最小能力面）。
 */
export const BODY_PATH = '/v1/official-chat/session-body';

/** 白名单里的**唯一**命令名（与壳侧 `official_chat::session_structure::COMMAND` 一致）。 */
export const COMMAND = 'sessionStructure';

/** 正文那条路由的命令名（与壳侧 `official_chat::session_body::COMMAND` 一致）。 */
export const BODY_COMMAND = 'sessionBody';

/**
 * 单次请求的超时。
 *
 * **比记忆桥的 1500 ms 长得多**：这一轮要等**壳侧订阅到站点自己的那条目录响应**
 * （订阅必须**早于**站点首个请求 —— 协议 §8.5.1 硬约束 3 ⇒ 站点加载慢就得等得住）。
 */
export const REQUEST_TIMEOUT_MS = 45000;

/**
 * **正文**那条路由的超时（**比目录那条长**）。
 *
 * 目录那条：壳侧**复用**已经起好的取数窗 ⇒ 命中缓存就秒回。
 * 正文那条：壳侧**每次**建窗 → 导航到那条会话的深链 → 等站点自己发那条
 * `history_messages` → 取体 ⇒ ⇒ **壳侧那一轮的上限是 30 s**（取数超时）
 * ⇒ ⇒ 这边**必须**比它长，否则会先在这边超时（**报的是错的原因，不是真的失败**）。
 */
export const BODY_TIMEOUT_MS = 60000;

/** 请求体上限（字节）——请求体只有一个命令名，1 KB 足够。 */
export const MAX_BODY_BYTES = 1024;

/**
 * 读桥凭据。**两样缺一就当作没有桥**。
 *
 * @param {object} env - 进程环境（默认 `process.env`）。
 * @returns {{url: string, token: string} | null}
 */
export function readCredentials(env = process.env) {
  const url = env?.[URL_ENV];
  const token = env?.[TOKEN_ENV];
  if (typeof url !== 'string' || url.length === 0) return null;
  if (typeof token !== 'string' || token.length === 0) return null;
  return { url, token };
}

/**
 * 请求体：**只有一个命令名**（外加**可选**的一个布尔开关）。
 *
 * **为什么不让内核传任意 JS**：这条路由等于开了一个任意 JS 执行面
 * （协议 §5.2 第 3 条）⇒ 能传的只有**白名单里的命令名**。
 *
 * ## `refresh` 是干什么的（协议 §4.6 修法 2）
 *
 * `true` ＝「我要**当刻**的目录」⇒ 壳侧**重取一轮**（建一扇新窗 ＋ 导航一次，
 * 实测 ≈ 6 秒）；缺省／`false` ＝「有新鲜的就给新鲜的」⇒ 壳侧按自己的
 * **保鲜期**判定（60 秒）。
 *
 * ⚠️ **默认形态一个字没变**（`{"command":"sessionStructure"}`）—— 既有用例与判据
 * 都按它逐字比对 ⇒ ⇒ **只有显式 `refresh === true` 才多带那个键**。
 *
 * @param {string} [command] - 命令名。
 * @param {boolean} [refresh] - 是否强制当刻重取。
 * @returns {string} JSON 文本。
 */
export function buildRequestBody(command = COMMAND, refresh = false) {
  if (refresh === true) {
    return JSON.stringify({ command: String(command), refresh: true });
  }
  return JSON.stringify({ command: String(command) });
}

/**
 * 正文那条路由的请求体：**一个命令名 ＋ 一个会话 id**。
 *
 * ⚠️ **能力面就这两样**：不接受任意 JS、不接受路径、不接受别的参数
 * （协议 §5.2 第 3 条）。
 *
 * @param {string} sessionId - 官网会话 id（目录项面给的 `id`）。
 * @returns {string} JSON 文本。
 */
export function buildBodyRequestBody(sessionId) {
  return JSON.stringify({ command: BODY_COMMAND, sessionId: String(sessionId ?? '') });
}

/**
 * 默认传输：一次 POST，读完整个响应体。
 *
 * @param {{url: string, token: string, path: string, body: string, timeoutMs: number}} options
 * @returns {Promise<{status: number, body: string}>}
 */
function defaultTransport({ url, token, path, body, timeoutMs }) {
  return new Promise((resolve, reject) => {
    const target = new URL(path, url);
    const payload = Buffer.from(body, 'utf8');
    const req = request(
      {
        hostname: target.hostname,
        port: target.port,
        path: `${target.pathname}${target.search}`,
        method: 'POST',
        headers: {
          authorization: `Bearer ${token}`,
          'content-type': 'application/json',
          'content-length': payload.length,
        },
      },
      (res) => {
        let text = '';
        res.setEncoding('utf8');
        res.on('data', (chunk) => {
          text += chunk;
        });
        res.on('end', () => resolve({ status: res.statusCode ?? 0, body: text }));
      },
    );
    req.setTimeout(timeoutMs, () => {
      req.destroy(new Error(`取数桥在 ${timeoutMs}ms 内没有响应`));
    });
    req.on('error', reject);
    req.end(payload);
  });
}

/**
 * 把站点条目裁成**目录项面**的每一项（协议 §2.4 面二 / §4.3 ②）。
 *
 * **逐字段裁**：站点条目里别的键（**正文、凭据、整包**）**一个都不带**。
 *
 * ⚠️ **键与类型的唯一事实源是 `protocol.js` 的 `CATALOG_ITEM_FIELDS`** —— 本文件
 * **不许**再抄一份对象字面量（判据按**源码形态**抽这一层，抽到的必须是下面
 * 这一份真身；多一份同键集的对象字面量会把读数抢走）。
 *
 * @param {unknown} raw - 站点 `chat_sessions` 里的一项。
 * @returns {{id: string, title: string, title_type: string, model_type: string,
 *   pinned: boolean, updated_at: number}}
 */
export function trimCatalogItem(raw) {
  const item = raw !== null && typeof raw === 'object' ? raw : {};
  // items 元素：逐字 6 个键，类型照 §4.3 ②（`pinned` 是布尔、`updated_at` 是数）。
  return {
    id: String(item.id ?? ''),
    title: String(item.title ?? ''),
    title_type: String(item.title_type ?? ''),
    model_type: String(item.model_type ?? ''),
    pinned: Boolean(item.pinned),
    updated_at: Number(item.updated_at ?? 0),
  };
}

/**
 * 校验回包里的**结构**（协议 §4.3 的字段面 ＋ §2.4 的白名单）。
 *
 * **只查形状，不查值** —— 结构面里本来就不该有值（标题、会话 id 在**目录项面**
 * 的那 6 个键里，不在结构面里）。
 *
 * @param {unknown} structure - 回包里的结构。
 * @returns {{ok: boolean, errors: string[]}}
 */
export function validateStructure(structure) {
  const errors = [];
  if (structure === null || typeof structure !== 'object') {
    return { ok: false, errors: ['结构不是对象'] };
  }
  const value = /** @type {Record<string, unknown>} */ (structure);
  // **协议 §2.4 的白名单，逐字列举那几项**（§2.4「允许回传」是列举式的 ⇒
  // 白名单之外即红 ⇒ 壳侧裁好的载荷**一项不多**）。
  const keys = [
    'topKeys',
    'code',
    'msg',
    'dataKeys',
    'sessionsLength',
    'firstItemKeys',
    'itemTypes',
    'hasMore',
    'items',
  ];
  for (const key of keys) {
    if (!(key in value)) errors.push(`缺字段 ${key}`);
  }
  // **白名单之外即红**（协议 §2.4 的「允许回传」是列举式的）⇒ 纵深防御：
  // 载荷里出现白名单之外的字段 ⇒ 当成形状不对，不让它悄悄流下去。
  const extra = Object.keys(value).filter((key) => !keys.includes(key));
  if (extra.length > 0) errors.push(`出现白名单之外的字段：${extra.join('、')}`);
  if (keys.every((key) => key in value)) {
    if (!Array.isArray(value.topKeys)) errors.push('topKeys 不是数组');
    if (!Array.isArray(value.dataKeys)) errors.push('dataKeys 不是数组');
    if (!Array.isArray(value.firstItemKeys)) errors.push('firstItemKeys 不是数组');
    if (value.sessionsLength !== null && typeof value.sessionsLength !== 'number') {
      errors.push('sessionsLength 不是数字');
    }
    if (value.hasMore !== null && typeof value.hasMore !== 'boolean') {
      errors.push('hasMore 不是布尔量');
    }
    if (value.itemTypes === null || typeof value.itemTypes !== 'object') {
      errors.push('itemTypes 不是对象');
    }
    // **目录项面**（协议 §2.4 面二）：顶层 `items` 是数组，每项**恰好 6 个键**、
    // **类型**照 §4.3 ② ⇒ ⇒ 白名单外的一个键都不许有。
    if (!Array.isArray(value.items)) {
      errors.push('items 不是数组');
    } else {
      value.items.forEach((item, index) => {
        if (item === null || typeof item !== 'object') {
          errors.push(`items[${index}] 不是对象`);
          return;
        }
        for (const [field, kind] of Object.entries(CATALOG_ITEM_FIELDS)) {
          if (typeof item[field] !== kind) {
            errors.push(`items[${index}] 的 ${field} 不是 ${kind}`);
          }
        }
        const extraItem = Object.keys(item).filter(
          (field) => !(field in CATALOG_ITEM_FIELDS),
        );
        if (extraItem.length > 0) {
          errors.push(`items[${index}] 出现第 7 个键：${extraItem.join('、')}`);
        }
      });
    }
  }
  return { ok: errors.length === 0, errors };
}

/**
 * 校**站点短码**的形态。
 *
 * ## 口径
 *
 * 小写字母／数字／连字符，1–32 字符 ⇒ ⇒ 认得出就是短码（`deepseek`），
 * 认不出 ⇒ **空串**（**不猜**、不截断、不改写）。
 *
 * ⚠️ **为什么必须校验**：这个串会进目录页（**要落缓存、要给界面显示**）⇒ ⇒
 * 形态不受控的串会顺着它进缓存文件与界面 ⇒ ⇒ 只放行一个**窄形态**。
 *
 * @param {unknown} raw - 回包信封上的 `site`。
 * @returns {string} 短码；认不出 ⇒ 空串。
 */
export function parseSite(raw) {
  const text = String(raw ?? '').trim();
  return /^[a-z0-9][a-z0-9-]{0,31}$/.test(text) ? text : '';
}

/**
 * 跑一轮真实取数。
 *
 * @param {object} options
 * @param {{url: string, token: string} | null} options.credentials - 桥凭据。
 * @param {Function} [options.transport] - 传输实现（测试注入）。
 * @param {number} [options.timeoutMs] - 超时。
 * @returns {Promise<{ok: true, structure: object, site: string} | {ok: false, code: string, detail: string}>}
 */
export async function fetchSessionStructure({
  credentials,
  transport = defaultTransport,
  timeoutMs = REQUEST_TIMEOUT_MS,
  refresh = false,
} = {}) {
  if (!credentials) {
    return { ok: false, code: 'source_unsupported', detail: '本机壳没有为这个内核起取数桥' };
  }
  const body = buildRequestBody(COMMAND, refresh === true);
  if (Buffer.byteLength(body, 'utf8') > MAX_BODY_BYTES) {
    return { ok: false, code: 'catalog_failed', detail: '请求体超限' };
  }
  let response;
  try {
    response = await transport({
      url: credentials.url,
      token: credentials.token,
      path: ROUTE_PATH,
      body,
      timeoutMs,
    });
  } catch (error) {
    return {
      ok: false,
      code: 'catalog_failed',
      detail: `无法连接取数桥：${String(error?.message ?? error)}`,
    };
  }
  let parsed;
  try {
    parsed = JSON.parse(response.body || '{}');
  } catch {
    return {
      ok: false,
      code: 'catalog_failed',
      detail: `取数桥返回了无法解析的内容（HTTP ${response.status}）`,
    };
  }
  if (parsed?.ok === true) {
    const check = validateStructure(parsed.structure);
    if (!check.ok) {
      return { ok: false, code: 'schema_changed', detail: check.errors.join('、') };
    }
    // **站点短码**（信封上的 `site`）—— **只认内核给的那个**（内核是唯一知道
    // 「我是谁」的那侧：插件只连本机回环桥，够不着站点）。
    // 认不出 ⇒ **空串**（**不猜**）⇒ 上层据此说「站点未知」，**不假装知道**。
    return { ok: true, structure: parsed.structure, site: parseSite(parsed.site) };
  }
  const raw = String(parsed?.error?.code ?? '');
  const code = ERROR_CODE_NAMES.includes(raw) ? raw : 'catalog_failed';
  const detail = String(parsed?.error?.detail ?? '');
  return { ok: false, code, detail: detail === '' ? raw : detail };
}

/**
 * 跑一轮**单会话正文**取数（协议 §4.4）。
 *
 * ## 回包里有什么
 *
 * · **成功** ⇒ `{ok:true, messages, captureResult, pick}`——
 *   `messages` ＝ **裁好的消息**（角色 ＋ 文本，**站点的响应体原文不出壳**）；
 *   `captureResult` ＝ `{status, omitted, unsupported}`（协议 §1.1 的捕获结果面）；
 *   `pick` ＝ **命中判据**（命中几条、按哪一条取的 —— 协议 §4.4 要求写清）。
 * · **失败** ⇒ `{ok:false, code, detail}`——`code` **必在协议那 14 个码里**
 *   （闭集外的码一律归 `catalog_failed`）。
 *
 * @param {object} options
 * @param {{url: string, token: string} | null} options.credentials - 桥凭据。
 * @param {string} options.sessionId - 官网会话 id。
 * @param {Function} [options.transport] - 传输实现（测试注入）。
 * @param {number} [options.timeoutMs] - 超时。
 * @returns {Promise<{ok: true, messages: Array<object>, captureResult: object, pick: object|null}
 *   | {ok: false, code: string, detail: string}>}
 */
export async function fetchSessionBody({
  credentials,
  sessionId,
  transport = defaultTransport,
  timeoutMs = BODY_TIMEOUT_MS,
} = {}) {
  if (!credentials) {
    return { ok: false, code: 'source_unsupported', detail: '本机壳没有为这个内核起取数桥' };
  }
  const id = String(sessionId ?? '').trim();
  if (id === '') {
    return { ok: false, code: 'catalog_failed', detail: '没有会话 id' };
  }
  const body = buildBodyRequestBody(id);
  if (Buffer.byteLength(body, 'utf8') > MAX_BODY_BYTES) {
    return { ok: false, code: 'catalog_failed', detail: '请求体超限' };
  }
  let response;
  try {
    response = await transport({
      url: credentials.url,
      token: credentials.token,
      path: BODY_PATH,
      body,
      timeoutMs,
    });
  } catch (error) {
    return {
      ok: false,
      code: 'catalog_failed',
      detail: `无法连接取数桥：${String(error?.message ?? error)}`,
    };
  }
  let parsed;
  try {
    parsed = JSON.parse(response.body || '{}');
  } catch {
    return {
      ok: false,
      code: 'catalog_failed',
      detail: `取数桥返回了无法解析的内容（HTTP ${response.status}）`,
    };
  }
  if (parsed?.ok === true) {
    const check = validateBodyMessages(parsed.messages);
    if (!check.ok) {
      return { ok: false, code: 'schema_changed', detail: check.errors.join('、') };
    }
    return {
      ok: true,
      messages: parsed.messages,
      captureResult: parsed.captureResult ?? { status: 'ok', omitted: [], unsupported: [] },
      pick: parsed.pick ?? null,
    };
  }
  const raw = String(parsed?.error?.code ?? '');
  const code = ERROR_CODE_NAMES.includes(raw) ? raw : 'catalog_failed';
  const detail = String(parsed?.error?.detail ?? '');
  return { ok: false, code, detail: detail === '' ? raw : detail };
}

/**
 * 校**正文消息**的形态（角色 ＋ 文本）。
 *
 * **只查形状，不查值** —— 与 `validateStructure` 同一口径。
 *
 * @param {unknown} messages - 回包里的消息数组。
 * @returns {{ok: boolean, errors: string[]}}
 */
export function validateBodyMessages(messages) {
  const errors = [];
  if (!Array.isArray(messages)) {
    return { ok: false, errors: ['messages 不是数组'] };
  }
  messages.forEach((message, index) => {
    if (message === null || typeof message !== 'object') {
      errors.push(`messages[${index}] 不是对象`);
      return;
    }
    if (typeof message.role !== 'string' || message.role === '') {
      errors.push(`messages[${index}] 缺 role`);
    }
    if (typeof message.text !== 'string') {
      errors.push(`messages[${index}] 缺 text`);
    }
  });
  return { ok: errors.length === 0, errors };
}

/**
 * 造一个**真实取数源**（注入 `createCommands`）。
 *
 * 形态与夹具源一致（`{ok:true, …}` / `{ok:false, code, detail}`）⇒ 命令层不用
 * 知道自己拿的是夹具还是真站点。
 *
 * @param {object} [options]
 * @param {object} [options.env] - 环境变量表（测试注入）。
 * @param {Function} [options.transport] - 传输实现（测试注入）。
 * @returns {{structure: Function, body: Function}}
 */
export function createSiteSource({ env = process.env, transport = defaultTransport } = {}) {
  return {
    /**
     * 取一次会话目录**结构**。
     *
     * @param {{accountScope?: string, refresh?: boolean}} [params] - 调用参数。
     *   `refresh` ＝ **当刻重取**（协议 §4.6 修法 2）。
     * @returns {Promise<{ok: true, structure: object} | {ok: false, code: string, detail: string}>}
     */
    async structure(params = {}) {
      const credentials = readCredentials(env);
      return fetchSessionStructure({ credentials, transport, refresh: params?.refresh === true });
    },
    /**
     * 取一次**单会话正文**（协议 §4.4：深链 ⇒ 站点自己发那条请求 ⇒ 壳侧取体）。
     *
     * @param {{sessionId: string, accountScope?: string}} [params] - 调用参数。
     * @returns {Promise<{ok: true, messages: Array<object>, captureResult: object}
     *   | {ok: false, code: string, detail: string}>}
     */
    async body(params = {}) {
      const credentials = readCredentials(env);
      return fetchSessionBody({ credentials, sessionId: params?.sessionId, transport });
    },
  };
}

/**
 * 旧名（**保留**：判据与既有调用点按这个名字引**）。
 *
 * @deprecated 用 {@link createSiteSource}（它现在同时带 `structure` 与 `body`）。
 * @param {object} [options] - 同 `createSiteSource`。
 * @returns {{structure: Function, body: Function}}
 */
export function createSiteStructureSource(options = {}) {
  return createSiteSource(options);
}
