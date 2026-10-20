/**
 * client ↔ host 的**桥接线层**（协议 §5「命名空间与桥」）。
 *
 * ## 为什么是 RPC，而不是「host 挂个服务、client 去 get」
 *
 * 取证结论（内核现役 release 树 `0.2.1-rc.2`）：
 *
 * 1. **client 半边与 host 半边不在同一棵 cordis 树**。client 半边是浏览器页面里
 *    的脚本（`window.__ModuleLoader__.load`），host 半边是内核 Node 进程里的
 *    cordis 插件。浏览器树里 `ctx.get('xlink-official-chat')` **永远解析不到**。
 * 2. **`ctx.set(name, service)` 在 cordis 里会抛错**：cordis 要求该属性已被
 *    `provide` 过，否则 `cannot set property "..." without provide`
 *    （`@deepseek-ai/cordis/lib/index.js:782-789`）。所以「host `ctx.set` 一个
 *    服务、client `ctx.get` 同一个名字」这条路**两头都不通**。
 * 3. **正式通道是 `connection` 服务的 RPC**：host 侧
 *    `ctx.connection.rpc.handle(channel, handler)`，client 侧
 *    `ctx.get('connection').rpc.call(channel, endpoint, payload, signal)`
 *    （浏览器侧 `client.browser.js:1497` 提供 `connection`；
 *    host 侧 `dsh-client-connection/lib/index.js:710`）。
 * 4. **`/api` 共享通道的 interceptor 全局只允许一个**（`lib/index.js:880`
 *    `already has an interceptor`，API Gateway 已占用）→ 插件必须用自己的
 *    通道前缀。
 *
 * ## 本文件的职责
 *
 * 只做**协议层 ↔ 传输层**的翻译：
 *
 *   · 把 5 个客户端命令（协议 §5：`capabilities` / `catalog` / `capture` /
 *     `cancel` / `preview`）映射成 RPC 端点；
 *   · 把协议错误负载（`{code, retryable, userNext, …}`）翻译成内核 RPC 要的
 *     `{ok:false, error:{code, message, details}}` 形态。
 *
 * **业务逻辑不在这里**（在 `commands.js`），**传输细节也不在这里**（内核的
 * `connection` 服务负责）。
 *
 * ## 授权
 *
 * 浏览器同源带签名 cookie；host 侧 `admit()` 先过 Host/Origin 再验认证，
 * 非 owner Peer 直接 403（`dsh-client-connection/lib/index.js:742-750`）。
 * **所以本层不自己实现鉴权**——那会绕过内核的准入检查。
 *
 * @module xlink-official-chat/rpc
 */

/** 插件自用的 RPC 通道前缀（**不能**用 `/api`：那个通道归 API Gateway 独占）。 */
export const RPC_CHANNEL = '/xlink-official-chat';

/** 5 个客户端命令的端点名（协议 §5「客户端命令职责」）。 */
export const ENDPOINTS = Object.freeze([
  'capabilities',
  'catalog',
  'capture',
  'cancel',
  'preview',
]);

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
      // （`dsh-client-connection/lib/index.js:614`：`z.record(z.string(), z.unknown())`）。
      // **不许放进去**：内部异常串——`detail` 只承载**已分类**的事实（稳定码）。
      detail: payload?.detail ?? null,
    },
  };
}

/**
 * 建 RPC handler。
 *
 * @param {{commands: object, logger?: object}} options
 *   - `commands`：`createCommands(...)` 的返回（业务逻辑）。
 * @returns {(endpoint: string, payload: object, signal?: AbortSignal) => Promise<object>}
 *   内核 RPC 要的 handler 形态：`{ok:true, value}` 或 `{ok:false, error}`。
 */
export function createRpcHandler({ commands, logger } = {}) {
  /**
   * 把命令层的返回值**转成 RPC 层的形态**。
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
   * @param {object} value - 命令层的返回值。
   * @returns {object} `{ok:true, value}` 或 `{ok:false, error}`。
   */
  const wrap = (value) => {
    if (value?.ok === false) {
      return { ok: false, error: toRpcError(value) };
    }
    return { ok: true, value };
  };

  return async function handleRpc(endpoint, payload, signal) {
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
        `official-chat: RPC 端点 ${name} 失败：${String(error?.message ?? error)}`,
      );
      return {
        ok: false,
        error: toRpcError({ code: 'source_unsupported', userNext: '换一个来源' }),
      };
    }
  };
}
