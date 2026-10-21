/**
 * 官网对话「会话目录结构」的**真实取数源**（P4 协议 §5 的接线侧）。
 *
 * ## 它是什么
 *
 * 壳侧那条路由（`POST /v1/official-chat/session-structure`）跑一轮真实取数，
 * 回来的**只有结构**：键名、条数、类型、布尔量（协议 §2.4 的白名单）。
 * 本文件是**内核侧**的客户端：把命令发给壳，把回包整理成协议形态。
 *
 * ## 三条纪律（与壳那一侧逐条对齐）
 *
 * · **只发一个命令名** —— 请求体是 `{"command":"sessionStructure"}`，
 *   **不接受任意 JS**（协议 §5.2 第 3 条：内核不许往壳里塞脚本）。
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
 * ⚠️ **本轮不回 `deepLink`、不回 `cursor`**（缺口 G4-19 / G4-8）⇒ ⇒ 目录项
 * 里**不许**出现这两个键（协议 §2.4 面二：只回那 6 个键）。
 *
 * @module xlink-official-chat/site-source
 */

import { request } from 'node:http';

import { ERROR_CODE_NAMES } from './protocol.js';

/** 地址的环境变量名（壳在启动内核时注入）。 */
export const URL_ENV = 'DSH_XLINK_BUILTIN_BRIDGE_URL';
/** 令牌的环境变量名。 */
export const TOKEN_ENV = 'DSH_XLINK_BUILTIN_BRIDGE_TOKEN';

/** 桥上那条路由的路径。 */
export const ROUTE_PATH = '/v1/official-chat/session-structure';

/** 白名单里的**唯一**命令名（与壳侧 `official_chat::session_structure::COMMAND` 一致）。 */
export const COMMAND = 'sessionStructure';

/**
 * 单次请求的超时。
 *
 * **比记忆桥的 1500 ms 长得多**：这一轮要等**壳侧订阅到站点自己的那条目录响应**
 * （订阅必须**早于**站点首个请求 —— 协议 §8.5.1 硬约束 3 ⇒ 站点加载慢就得等得住）。
 */
export const REQUEST_TIMEOUT_MS = 45000;

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
 * 请求体：**只有一个命令名**。
 *
 * **为什么不让内核传任意 JS**：这条路由等于开了一个任意 JS 执行面
 * （协议 §5.2 第 3 条）⇒ 能传的只有**白名单里的命令名**。
 *
 * @param {string} [command] - 命令名。
 * @returns {string} JSON 文本。
 */
export function buildRequestBody(command = COMMAND) {
  return JSON.stringify({ command: String(command) });
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
 * 目录项面**每一项**的 6 个键与它们的类型（协议 §2.4 面二 / §4.3 ②）。
 *
 * ⚠️ **穷举列举**：少一个键、多一个键、类型不符 ⇒ 都是漂移（判据第 3／8 条
 * 断的就是这一面）。
 *
 * ⚠️ **用「键／类型」成对数组，不用对象**：判据按**源码形态**抽这一层的键面
 * （`scripts/lib/p4-source-shape.mjs` 的 `jsItemsElementFields`）⇒ ⇒ 若这里写成
 * **对象字面量**，它会被当成「目录项的那个字面量」抽走 ⇒ ⇒ 类型全是字符串
 * 字面量（`unknown`）⇒ 键面会假红。真正的目录项字面量在
 * {@link trimCatalogItem}，**那一份才是判据要读的那一份**。
 */
export const CATALOG_ITEM_TYPES = Object.freeze([
  Object.freeze(['id', 'string']),
  Object.freeze(['title', 'string']),
  Object.freeze(['title_type', 'string']),
  Object.freeze(['model_type', 'string']),
  Object.freeze(['pinned', 'boolean']),
  Object.freeze(['updated_at', 'number']),
]);

/**
 * 把站点条目裁成**目录项面**的每一项（协议 §2.4 面二 / §4.3 ②）。
 *
 * **逐字段裁**：站点条目里别的键（**正文、凭据、整包**）**一个都不带**。
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
        for (const [field, kind] of CATALOG_ITEM_TYPES) {
          if (typeof item[field] !== kind) {
            errors.push(`items[${index}] 的 ${field} 不是 ${kind}`);
          }
        }
        const extraItem = Object.keys(item).filter(
          (field) => !CATALOG_ITEM_TYPES.some(([known]) => known === field),
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
 * 跑一轮真实取数。
 *
 * @param {object} options
 * @param {{url: string, token: string} | null} options.credentials - 桥凭据。
 * @param {Function} [options.transport] - 传输实现（测试注入）。
 * @param {number} [options.timeoutMs] - 超时。
 * @returns {Promise<{ok: true, structure: object} | {ok: false, code: string, detail: string}>}
 */
export async function fetchSessionStructure({
  credentials,
  transport = defaultTransport,
  timeoutMs = REQUEST_TIMEOUT_MS,
} = {}) {
  if (!credentials) {
    return { ok: false, code: 'source_unsupported', detail: '本机壳没有为这个内核起取数桥' };
  }
  const body = buildRequestBody();
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
    return { ok: true, structure: parsed.structure };
  }
  const raw = String(parsed?.error?.code ?? '');
  const code = ERROR_CODE_NAMES.includes(raw) ? raw : 'catalog_failed';
  const detail = String(parsed?.error?.detail ?? '');
  return { ok: false, code, detail: detail === '' ? raw : detail };
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
 * @returns {{structure: Function}}
 */
export function createSiteStructureSource({ env = process.env, transport = defaultTransport } = {}) {
  return {
    /**
     * 取一次会话目录**结构**。
     *
     * @param {{accountScope?: string}} [params] - 调用参数。
     * @returns {Promise<{ok: true, structure: object} | {ok: false, code: string, detail: string}>}
     */
    async structure(params = {}) {
      const credentials = readCredentials(env);
      return fetchSessionStructure({ credentials, transport });
    },
  };
}
