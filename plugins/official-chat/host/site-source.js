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
 * ## 为什么目录页（`catalog`）还不用它
 *
 * 协议 §2.4 的白名单**禁止**回传**会话 id 与标题**，而 P1 §1.2 的目录页
 * 恰恰要这两样 ⇒ ⇒ **目录页不能由这条真实路径构造** ⇒ 目录页仍用固定夹具，
 * 并在 `capabilities.degraded` 里如实标注（**不静默降级**）。
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
 * 校验回包里的**结构**（协议 §4.3 的字段面 ＋ §2.4 的白名单）。
 *
 * **只查形状，不查值** —— 值本来就不该过来（标题、会话 id、凭据）。
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
