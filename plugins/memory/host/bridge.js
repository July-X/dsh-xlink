/**
 * 记忆插件与桌面壳之间的最小客户端。
 *
 * 通道是外壳的**内置插件回环桥**（`src-tauri/src/plugins/builtin/bridge.rs`）：
 * `Authorization: Bearer <令牌>`，地址与令牌由内核进程的环境变量给出
 * （`DSH_XLINK_BUILTIN_BRIDGE_URL` / `_TOKEN`，壳在启动内核时注入）。
 *
 * 三条纪律与桥那一侧逐条对齐：
 * · **GET + 查询串，不发请求体**——桥的请求读取原语只保证读到请求头，body 可能
 *   在同一次 read 里到达也可能没有（TCP 无消息边界），拿半截缓冲当请求体会得到
 *   一个查不出原因的空串；
 * · **参数一律 `URLSearchParams` 编码**：标题与正文基本都是中文，不编码就会被
 *   桥那边的解码器原样还原成 `%E4%B8%AD…` 存进记忆库；
 * · **失败不抛**，统一回 `{ ok: false, reason }`：记忆不可用不该让会话起不来。
 *
 * 本文件**不 import 任何 `@deepseek-ai/*` 包**，所以 `npm run check` 的冒烟测试
 * 能直接 import 它并把编解码、超时与错误分支真的跑一遍（与 ponytail 的
 * `bridge-credential.js` 同一个理由：能脱离内核测的那一半单独放）。
 *
 * @module xlink-memory/bridge
 */
import { request } from 'node:http';

/** 地址的环境变量名（壳在启动内核时注入）。 */
export const URL_ENV = 'DSH_XLINK_BUILTIN_BRIDGE_URL';
/** 令牌的环境变量名。 */
export const TOKEN_ENV = 'DSH_XLINK_BUILTIN_BRIDGE_TOKEN';

/** 单次请求的超时。本地回环，1.5 秒还没回就是那边卡住了——宁可这回合不带记忆。 */
export const REQUEST_TIMEOUT_MS = 1500;

/**
 * 拼出来的路径（含查询串）字节上限。
 *
 * 壳那边 `openai::http::read_request` 的缓冲是**固定 8 KB**，而 `GET` 的查询串与
 * 请求头共用这一个额度。越界之后它返回「请求头不完整」，**一个字节的响应都不写**
 * 就关掉连接——插件侧只看到 `socket hang up`，被包成「无法连接记忆桥」，而模型会
 * 把这句话原样讲给用户听：用户会去查壳的进程，而不是知道自己写得太长。
 *
 * 所以在发出去之前拦一道，把「太长」这件事说清楚。留 1 KB 余量给 `Host` 与
 * `Authorization` 两行请求头。
 */
export const MAX_PATH_BYTES = 7000;

/**
 * 读桥凭据。**两样缺一就当作没有桥**：只有地址没有令牌只会白跑一趟并拿回 401。
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
 * 拼一条记忆路由的路径。空串按「没有这个参数」处理——桥那边两者等价，
 * 而短 URL 更好读、也少一次无意义的解码。
 *
 * @param {string} route - `context` / `read` / `propose`。
 * @param {object} params - 查询参数。
 * @returns {string} 以 `/v1/memory/` 开头的路径。
 */
export function buildPath(route, params = {}) {
  const search = new URLSearchParams();
  for (const [key, value] of Object.entries(params)) {
    if (value === undefined || value === null) continue;
    const text = String(value);
    if (text.length === 0) continue;
    search.set(key, text);
  }
  const query = search.toString();
  return `/v1/memory/${route}${query ? `?${query}` : ''}`;
}

/**
 * 默认传输：一次 GET，读完整个响应体。**不跟随重定向**（回环桥不会发重定向，
 * 跟随只会把令牌送去别处）。
 *
 * @param {{url: string, token: string, path: string, timeoutMs: number}} options
 * @returns {Promise<{status: number, body: string}>}
 */
function defaultTransport({ url, token, path, timeoutMs }) {
  return new Promise((resolve, reject) => {
    const target = new URL(path, url);
    const req = request(
      {
        hostname: target.hostname,
        port: target.port,
        path: `${target.pathname}${target.search}`,
        method: 'GET',
        headers: { authorization: `Bearer ${token}` },
      },
      (res) => {
        let body = '';
        res.setEncoding('utf8');
        res.on('data', (chunk) => {
          body += chunk;
        });
        res.on('end', () => resolve({ status: res.statusCode ?? 0, body }));
      }
    );
    req.setTimeout(timeoutMs, () => {
      req.destroy(new Error(`桥在 ${timeoutMs}ms 内没有响应`));
    });
    req.on('error', reject);
    req.end();
  });
}

/**
 * 调一条记忆路由并把结果收敛成 `{ ok }`。
 *
 * 非 2xx 也走 `{ ok: false }`：401（令牌过期）、404（条目已删除）、400（正文
 * 超限）都是**可预期的业务结果**，调用方要能原样把原因讲给模型与用户听，
 * 而不是抛一个异常让整步失败。
 *
 * @param {object} options - 凭据、路由、参数与超时。
 * @param {Function} [transport] - 传输实现（测试注入）。
 * @returns {Promise<{ok: true, value: any} | {ok: false, reason: string}>}
 */
export async function callMemory(
  { credentials, route, params, timeoutMs = REQUEST_TIMEOUT_MS },
  transport = defaultTransport
) {
  if (!credentials) return { ok: false, reason: '本机壳没有为这个内核起记忆桥' };
  const path = buildPath(route, params);
  // 先按**字节**量一遍大小再发：中文编码后是 3 倍膨胀，按字符数判断会漏。
  const bytes = Buffer.byteLength(path, 'utf8');
  if (bytes > MAX_PATH_BYTES) {
    return {
      ok: false,
      reason:
        `这条记忆太长了（编码后约 ${bytes} 字节，通道上限 ${MAX_PATH_BYTES} 字节）。` +
        '请拆成更短的几条分别提交——一次发不过去的内容，拆开就能过。',
    };
  }
  let response;
  try {
    response = await transport({ url: credentials.url, token: credentials.token, path, timeoutMs });
  } catch (error) {
    return { ok: false, reason: `无法连接记忆桥：${String(error?.message ?? error)}` };
  }
  let parsed;
  try {
    parsed = JSON.parse(response.body || '{}');
  } catch {
    return { ok: false, reason: `记忆桥返回了无法解析的内容（HTTP ${response.status}）` };
  }
  if (response.status < 200 || response.status >= 300) {
    return {
      ok: false,
      reason: typeof parsed.error === 'string' && parsed.error ? parsed.error : `记忆桥返回 HTTP ${response.status}`,
    };
  }
  return { ok: true, value: parsed };
}
