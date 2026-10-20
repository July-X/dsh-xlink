/**
 * client ↔ host 的**桥接线层**（协议 §5「命名空间与桥」）。
 *
 * ## 通道形态：**内核 `connection` 服务的 exact fetch route**（协议 v0.4 裁定）
 *
 * 取证结论（内核现役 release 树 `0.2.1-rc.2`，
 * `C:\Users\zxx\.dsh-xlink\dsh\desktop\kernels\0.2.1-rc.2`）：
 *
 * 1. **client 半边与 host 半边不在同一棵 cordis 树**。client 半边是浏览器页面里的
 *    脚本（`window.__ModuleLoader__.load`），host 半边是内核 Node 进程里的
 *    cordis 插件。浏览器树里 `ctx.get('xlink-official-chat')` **永远解析不到**。
 * 2. **`ctx.set(name, service)` 在 cordis 里会抛错**：cordis 要求该属性已被
 *    `provide` 过，否则 `cannot set property "..." without provide`
 *    （`@deepseek-ai/cordis/lib/index.js@0.2.1-rc.2:782-789`）。所以「host `ctx.set` 一个
 *    服务、client `ctx.get` 同一个名字」这条路**两头都不通**。
 * 3. **`connection.rpc.handle(channel, handler)` 插件侧用不了**（v0.4 已证伪）：
 *    `register()` 的最后一步是 `owner.effect(() => owner.webServer.register(route), …)`
 *    （`dsh-client-connection/lib/index.js@0.2.1-rc.2:871`）——**属性访问**；
 *    而属性查找落在**内核 connection 插件自己的 fiber scope**
 *    （`(ctx[symbols.shadow] ?? ctx).fiber`，
 *    `@deepseek-ai/cordis/lib/index.js@0.2.1-rc.2:683` + `createShadow` `:111-116`），
 *    那个 scope **从不 inject `webServer`**（内核一律安全读
 *    `this.owner.get("webServer")`，`dsh-client-connection/lib/index.js@0.2.1-rc.2:745`
 *    与 `:778`）⇒ 第三方插件调 `rpc.handle` 必抛
 *    `cannot get property "webServer" without inject`。**插件侧声明 `webServer`
 *    也修不了**（实测两轮无效）。
 * 4. **可行通道 = `ctx.connection.fetch.register({kind:'exact', path, …})`**
 *    （`dsh-client-connection/lib/index.js@0.2.1-rc.2:840-854`
 *    `registerFetchRoute`）——**完全不碰 webServer** ⇒ 不会触发上面那个抛错。
 *    内核给三样：
 *    ① **路径必须在 `/api` 命名空间下**——`assertFetchRoute`
 *       （`dsh-client-connection/lib/index.js@0.2.1-rc.2:973-977`）用
 *       `endpointFromPath("/api", route.path)` 校验，不过就抛；
 *    ② **owner 准入由内核做**——`createSharedFetchHandler`
 *       （`dsh-client-connection/lib/index.js@0.2.1-rc.2:820-838`）命中 exact route
 *       时先判 `peer.principal.kind !== "owner"` → 403；
 *    ③ 与 API Gateway 的 shared interceptor **两级分发不冲突**（exact route 先查、
 *       miss 才落 interceptor）。
 * 5. **exact path 是全等匹配**（`fetchRoutes.get(url.pathname)`）——所以
 *    **客户端必须自己发 same-origin fetch**到**同一个 pathname**：浏览器侧内核自带的
 *    `connection.rpc.call` 打不到它（`dsh-client-connection/lib/client.browser.js@0.2.1-rc.2:1221`
 *    的 URL 是 `` `${channel}/${endpoint}`.slice(1) ``——**相对路径、不带 `/api`**，
 *    且 `channel` 的形状是 `^\/[A-Za-z0-9._~-]+$`，装不下多段路径）。
 *
 * ## 本文件的职责
 *
 * 只做**协议层 ↔ 传输层**的翻译，**分两层**：
 *
 *   · `createDispatcher`——**派发层**：把 5 个客户端命令（协议 §5：`capabilities` /
 *     `catalog` / `capture` / `cancel` / `preview`）映射成结果，并把协议错误负载
 *     （`{code, retryable, userNext, …}`）翻译成 `{ok:false, error:{code, message,
 *     details}}`。**它不认识 HTTP**。
 *   · `createFetchRouteHandler`——**信封层**：exact fetch route 的 `fetch` 实现。
 *     解析内核那套 `client-request` 信封、校验、调派发层、把结果包回
 *     `server-response` 信封。**它不认识协议**。
 *
 * **业务逻辑不在这里**（在 `commands.js`），**传输细节也不在这里**（内核的
 * `connection` 服务负责路由与准入）。
 *
 * ## 授权
 *
 * 浏览器同源带签名 cookie；**owner 准入由内核做**（`createSharedFetchHandler` 命中
 * exact route 时判 `peer.principal.kind`），**本层不自己实现鉴权**——那会绕过内核
 * 的准入检查，自造安全面（协议 §5 已显式否决「插件自建 webServer 路由 + 自实现
 * 鉴权」那条路）。
 *
 * @module xlink-official-chat/rpc
 */

/**
 * exact fetch route 的**路径**（协议 v0.4 §5 写死）。
 *
 * **必须在 `/api` 命名空间下**——`assertFetchRoute` 用
 * `endpointFromPath("/api", route.path)` 校验（`dsh-client-connection/lib/index.js@0.2.1-rc.2:973-977`）。
 *
 * **客户端必须用同一个 pathname**（全等匹配：`fetchRoutes.get(url.pathname)`）。
 */
export const FETCH_ROUTE_PATH = '/api/xlink-official-chat';

/**
 * 客户端命令的端点名（协议 §5「客户端命令职责」＋ **P2 协议 §2.2 的
 * `duplicates`**——重复识别 ＋ 预算预检）。
 *
 * **P1 §5 的清单仍写 5 个命令**（`capabilities` / `catalog` / `capture` /
 * `cancel` / `preview`）⇒ **P2 协议未同步 P1 §5**，此处按 P2 协议加了第 6 个
 * ⇒ **P1 §5 需 PM 同步**（协议侧归属，不在本层改）。
 */
export const ENDPOINTS = Object.freeze([
  'capabilities',
  'catalog',
  'capture',
  'cancel',
  'preview',
  'duplicates',
]);

/** 信封里 `method` 字段的形状（照抄内核 `ENDPOINT_SEGMENT_PATTERN`）。 */
const ENDPOINT_PATTERN = /^[A-Za-z0-9_$.-]+$/;

/**
 * 把协议错误负载翻成内核 RPC 的错误形态。
 *
 * **内部异常串不许进去**（协议 §3）：只有稳定码与「用户下一步」这类
 * **已经分类好**的信息可以外发。
 *
 * @param {{code: string, retryable?: boolean, userNext?: string, detail?: string|null}} payload
 * @returns {{code: string, message: string, details: object}}
 */
export function toRpcError(payload) {
  const code = String(payload?.code ?? 'source_unsupported');
  const userNext = String(payload?.userNext ?? '');
  const message = userNext === '' ? code : `${code}：${userNext}`;
  return {
    code,
    message,
    details: {
      retryable: Boolean(payload?.retryable),
      userNext,
      requestId: payload?.requestId ?? null,
      logPath: payload?.logPath ?? null,
      // **原始码要穿过这道边界**（协议 §3 末条：内部冲突码归入 `snapshot_invalid`，
      // 「原始码保留在 `detail`」）。不带走它，对端就只看到一个笼统的
      // `snapshot_invalid`，排查时拿不到「其实是同 id 重复落盘」。
      //
      // **能放进去**：内核的 RPC 错误 schema 的 `details` 是自由 record
      // （`dsh-client-connection/lib/index.js@0.2.1-rc.2:614`：
      // `z.record(z.string(), z.unknown())`）。
      // **不许放进去**：内部异常串——`detail` 只承载**已分类**的事实（稳定码）。
      detail: payload?.detail ?? null,
    },
  };
}

/**
 * **派发层**：把端点名映射到命令，并把命令的返回值整成传输层的结果形态。
 *
 * **内层的 `ok:false` 必须变成外层的 `ok:false`**——这一条是 P1 判据
 * A6a / A7 / A10 的根因：命令层失败时回的是
 * `{ok:false, code:'snapshot_invalid', …}`，若这里无条件包成
 * `{ok:true, value: …}`，调用方只看外层 ⇒ **失败被吞成成功** ⇒ 客户端拿到
 * 一个**没有正文的「成功值」**，又把**空正文发给模型**。
 *
 * 那样会同时破掉两条协议：S-4（请求体必须与落盘逐字节相同）与
 * S-5（失效必须拒），而**正常路径全绿**——只有「读不到」这一支坏掉，
 * 极难在正常使用时发现。
 *
 * **判失败必须看内层**；包装层负责把内层失败转成外层失败。
 *
 * @param {{commands: object, logger?: object}} options
 *   - `commands`：`createCommands(...)` 的返回（业务逻辑）。
 * @returns {(endpoint: string, payload: object, signal?: AbortSignal) => Promise<object>}
 *   `{ok:true, value}` 或 `{ok:false, error}`。
 */
export function createDispatcher({ commands, logger } = {}) {
  /**
   * 把命令层的返回值**转成传输层的形态**。
   *
   * @param {object} value - 命令层的返回值。
   * @returns {object} `{ok:true, value}` 或 `{ok:false, error}`。
   */
  const wrap = (value) => {
    if (value?.ok === false) {
      return { ok: false, error: toRpcError(value) };
    }
    return { ok: true, value };
  };

  return async function dispatch(endpoint, payload, signal) {
    const name = String(endpoint ?? '');
    try {
      switch (name) {
        case 'capabilities':
          return wrap(commands.capabilities());
        case 'catalog':
          return wrap(await commands.catalog(payload ?? {}));
        case 'capture':
          return wrap(await commands.capture(payload ?? {}));
        case 'cancel':
          return wrap(commands.cancel(payload ?? {}));
        case 'preview':
          return wrap(commands.preview(payload ?? {}));
        case 'duplicates':
          // **P2 协议 §2.2**：重复识别 ＋ 预算预检（与 `groups` 同一次往返出）。
          return wrap(await commands.duplicates(payload ?? {}));
        default:
          // 未知端点不是「成功值」——它必须是个明确的失败。
          return {
            ok: false,
            error: toRpcError({ code: 'source_unsupported', userNext: '换一个来源' }),
          };
      }
    } catch (error) {
      // **异常串不进返回值**（协议 §3）：只回稳定码，细节进日志。
      logger?.warn?.(
        `official-chat: 端点 ${name} 失败：${String(error?.message ?? error)}`,
      );
      return {
        ok: false,
        error: toRpcError({ code: 'source_unsupported', userNext: '换一个来源' }),
      };
    }
  };
}

/**
 * **信封层**：exact fetch route 的 `fetch` 实现。
 *
 * **为什么信封要自己写**：exact route 命中后**绕过**内核的 `rpcFetchHandler`
 * （`dsh-client-connection/lib/index.js@0.2.1-rc.2:888-917`），所以
 * `client-request` / `server-response` 这套信封由本文件实现——**但形态照抄内核**，
 * 这样将来上游修好 `:871` 后收敛回 `rpc.handle` 时，信封不用改。
 *
 * **信封形状**（照抄内核 `lib/types/rpc-schema.js`）：
 * · 请求：`{type:'client-request', rpcId, method, payload}`
 * · 响应：`{type:'server-response', rpcId, result}`，`result` 是
 *   `{ok:true, value}` 或 `{ok:false, error:{code, message, details}}`
 *   （内核 `fullResponse`，`dsh-client-connection/lib/index.js@0.2.1-rc.2:938-946`）。
 *
 * **协议 §3 的 13 个码是闭集**：本层**不新增协议码**。信封本身不合法时用的是
 * **传输层**的 `gateway/bad-request`（照抄内核 `badRequest`，
 * `dsh-client-connection/lib/index.js@0.2.1-rc.2:915-925`）——它是传输层事实，
 * 不是协议 §3 的命令结果码，**不进协议码表**。
 *
 * @param {{dispatch: Function, logger?: object}} options
 *   - `dispatch`：`createDispatcher(...)` 的返回。
 * @returns {(request: Request) => Promise<Response>} exact route 的 `fetch`。
 */
export function createFetchRouteHandler({ dispatch, logger } = {}) {
  /**
   * 回一个 `server-response` 信封。
   *
   * @param {string} rpcId - 关联 id。
   * @param {object} result - `{ok:true,value}` 或 `{ok:false,error}`。
   * @returns {Response}
   */
  const envelope = (rpcId, result) =>
    Response.json({ type: 'server-response', rpcId, result });

  /**
   * 信封不合法 ⇒ 400 + `gateway/bad-request`（形状照抄内核）。
   *
   * @param {string[]} issues - 具体问题。
   * @returns {Response}
   */
  const badRequest = (issues) =>
    new Response(
      JSON.stringify({
        type: 'server-response',
        rpcId: 'invalid-request',
        result: {
          ok: false,
          error: {
            code: 'gateway/bad-request',
            message: 'invalid client-request message',
            details: { issues },
          },
        },
      }),
      { status: 400, headers: { 'content-type': 'application/json' } },
    );

  return async function handleFetchRoute(request) {
    let message = null;
    try {
      message = await request.json();
    } catch {
      return badRequest(['body 不是合法 JSON']);
    }
    const issues = [];
    if (message === null || typeof message !== 'object' || Array.isArray(message)) {
      issues.push('body 不是对象');
      return badRequest(issues);
    }
    if (message.type !== 'client-request') {
      issues.push(`type 不是 client-request（实为 ${String(message.type ?? '')}）`);
    }
    const rpcId = typeof message.rpcId === 'string' ? message.rpcId : '';
    if (rpcId === '') {
      issues.push('rpcId 缺失或不是字符串');
    }
    const method = typeof message.method === 'string' ? message.method : '';
    if (!ENDPOINT_PATTERN.test(method)) {
      issues.push(`method 形状不合法（实为 ${String(message.method ?? '')}）`);
    }
    if (issues.length > 0) {
      return badRequest(issues);
    }

    let result;
    try {
      result = await dispatch(method, message.payload ?? {}, request.signal);
    } catch (error) {
      // 派发层自己已经兜住了异常；这里只是不让异常冒到 HTTP 层。
      logger?.warn?.(
        `official-chat: exact route 派发 ${method} 失败：${String(error?.message ?? error)}`,
      );
      result = {
        ok: false,
        error: toRpcError({ code: 'source_unsupported', userNext: '换一个来源' }),
      };
    }
    return envelope(rpcId, result);
  };
}
