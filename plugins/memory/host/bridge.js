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
/**
 * 这条请求**编码后**有多长——与 [`buildPath`] 用同一个 `URLSearchParams` 算法，
 * 所以它就是通道真正会看到的字节数。
 *
 * 为什么要有它（独立评审第 6 条）：正文的上限（8 KiB）是按**原文**定的，而通道上限（7000）
 * 是按**编码后**算的，中文一字 3 字节、百分号编码再乘 3——逐项都合法的候选合起来可以远超通道。
 * 评审实测：510 B 标题 + 12 条关键词 + 编码后 5994 B 正文 = **10181 B > 7000**。
 * 光靠「给标题和关键词留 1000 字节」这种估法挡不住（那 1000 要同时装标题、关键词、工作区路径
 * 与令牌），所以**提交前按整条路径真算一次**。
 *
 * @param {string} route - 路由（`jobs/submit` 等）。
 * @param {object} params - 参数。
 * @returns {number} 字节数。
 */
export function encodedPathBytes(route, params = {}) {
  return Buffer.byteLength(buildPath(route, params), 'utf8');
}

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
 * 把领回来的任务键翻译成插件内部用的那份：**补上 live 会话对象，其余字段原样带过去**。
 *
 * **为什么必须是「原样带过去」而不是手挑几个字段**：手挑必然漏，而漏掉的那一个
 * **症状是静默的**。2026-10-16 真机上逮到过：`index.js` 那时写的是
 * `const key = { session: liveSession, cursor: endSeq, extractor }`——壳那边明明把
 * `from_cursor` 冻进任务键并落盘了（`memory/jobs.rs` 的 `enqueue`），领回来的响应里也有，
 * 而这里只挑了三个字段，`from_cursor` 落在外面，于是读取那行
 * `from: Number(key.from_cursor ?? 0)` **恒取 0**。
 *
 * 后果有两层，第二层比第一层严重：
 *   ① 每次提取都从会话开头重读整份日志——正是那句注释声称要避开的成本，长会话下白读；
 *   ② `content_hash` 是对**整段切片**算的，于是它覆盖 `0..endSeq`：会话只要长了指纹就变，
 *      H7「同会话 + 区间重叠 + 指纹不同」于是**每次都成立**，退化成清单里被否掉的
 *      (b) 口径（「几乎必然触发」）。① 只是慢，② 让一条拍板过的判据失去意义。
 *
 * **判据为什么以前没逮住**：`smoke.test.mjs` 里那条
 * `assert.match(code, /from: Number\(key\.from_cursor \?\? 0\)/)` **绿着**——它判的是源码
 * 字面，而字面是对的；值到不到它一点没查。**判据必须在两端之间对数据，不能只判代码长得
 * 像不像**（本仓栽过好几次，见记忆里「判据骗过自己的四种方式」）。
 *
 * @param {object} claimedKey - 桥回来的 `key`（`{session, cursor, extractor, from_cursor, …}`）。
 * @param {object|null} liveSession - 按 id 取到的 live 会话对象；取不到就是 `null`。
 * @param {string} [defaultExtractor] - 缺 `extractor` 时用的兜底：插件自己的提取器版本号。
 *   **由调用点传，不在本文件写死**——那个常量住在 `index.js`（`extract.js` 的 `EXTRACTOR_VERSION`），
 *   而本函数只 import `node:http`、本来就不该知道它。早先这里写死空串，等于把「缺了就用本版提取器」
 *   静默降级成「空版本」（2026-10-16 从 `index.js` 搬过来时丢的）。
 * @returns {object} 内部用的任务键。
 */
export function claimKeyOf(claimedKey, liveSession, defaultExtractor) {
  const carried = claimedKey && typeof claimedKey === 'object' ? { ...claimedKey } : {};
  return {
    ...carried,
    // **会话要换成 live 对象**：`SessionStore.flush` 按**对象身份**（WeakMap）取 entry，
    // 自拼的 `{id}` 字面量永远不是它认识的那个，主路于是每次抛「not live in this store」
    // 并静默退到 live 兜底。取不到就留 `null`——读取那一步会如实说「没有会话对象」。
    session: liveSession ?? null,
    cursor: Number(carried.cursor ?? 0),
    extractor: String(carried.extractor ?? defaultExtractor ?? ''),
  };
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
