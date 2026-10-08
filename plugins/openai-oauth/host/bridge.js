/**
 * Rust 本地桥接服务的客户端。
 *
 * 服务由桌面壳随内核启动拉起（开发计划 §5）：`127.0.0.1` 随机端口、
 * 每次启动的高熵令牌，经子进程环境传入。Host 插件只做协议转换，
 * 不持有 OAuth 令牌、不直连 OpenAI。
 *
 * P1 阶段实现握手与模型目录；推理流（`/v1/responses`）是 P2/P4 的
 * 交付物，适配器侧对它的调用会得到明确的 `BRIDGE_STREAM_UNIMPLEMENTED`。
 */
import { BRIDGE_PROTOCOL_VERSION, BRIDGE_TOKEN_ENV, BRIDGE_URL_ENV } from "./constants.js";

export class BridgeUnavailableError extends Error {
  constructor(message) {
    super(message);
    this.code = "BRIDGE_UNAVAILABLE";
  }
}

/** 从环境解析桥接地址与令牌；缺任一项即视为桥接未随本次启动提供。 */
export function bridgeFromEnv(env = process.env) {
  const url = env[BRIDGE_URL_ENV];
  const token = env[BRIDGE_TOKEN_ENV];
  if (!url || !token) return undefined;
  return { url: url.replace(/\/$/, ""), token };
}

async function bridgeFetch(bridge, path, init = {}) {
  let response;
  try {
    response = await fetch(`${bridge.url}${path}`, {
      ...init,
      headers: {
        authorization: `Bearer ${bridge.token}`,
        ...(init.headers ?? {}),
      },
      signal: init.signal,
    });
  } catch (error) {
    throw new BridgeUnavailableError(`桥接服务不可达（${bridge.url}）：${String(error?.cause ?? error)}`);
  }
  if (response.status === 401 || response.status === 403) {
    throw new BridgeUnavailableError("桥接服务拒绝了本次连接令牌");
  }
  if (!response.ok) {
    const body = await response.text().catch(() => "");
    throw new BridgeUnavailableError(`桥接服务返回 ${response.status}${body ? `：${body.slice(0, 200)}` : ""}`);
  }
  return response;
}

/**
 * 握手：核对协议版本。协议或插件版本不匹配按启动验证失败处理
 * （开发计划 §5），这里抛出带 `code` 的结构化错误。
 */
export async function handshake(bridge, pluginVersion, signal) {
  const response = await bridgeFetch(bridge, "/v1/handshake", { signal });
  const info = await response.json();
  if (info?.protocol !== BRIDGE_PROTOCOL_VERSION) {
    const error = new Error(`桥接协议不匹配：服务端 ${String(info?.protocol)}，插件期望 ${BRIDGE_PROTOCOL_VERSION}`);
    error.code = "BRIDGE_PROTOCOL_MISMATCH";
    throw error;
  }
  return info;
}

/**
 * 模型目录：返回 `{revision, models}`，模型含精确 ID、显示名与可选的
 * 上下文容量、强度档位（开发计划 §7 的能力记录）。
 */
export async function fetchCatalog(bridge, signal) {
  const response = await bridgeFetch(bridge, "/v1/models", { signal });
  return response.json();
}

/// 推理流（POST /v1/responses）：带鉴权与信封，返回**按行异步迭代器**
/// （桥接下发 NDJSON；连接关闭即迭代结束；终止包络由 pumpStream 解释）。
/// 非 2xx → BridgeUnavailableError（带响应体摘录，含 401 需重登 / 409
/// 目录过期 / 400 请求被拒的语义）。
export async function* streamInferenceLines(bridge, envelope, signal) {
  let response;
  try {
    response = await fetch(`${bridge.url}/v1/responses`, {
      method: "POST",
      headers: {
        authorization: `Bearer ${bridge.token}`,
        "content-type": "application/json",
      },
      body: JSON.stringify(envelope),
      signal,
    });
  } catch (error) {
    throw new BridgeUnavailableError(`桥接推理请求失败：${String(error?.cause ?? error)}`);
  }
  if (!response.ok) {
    const body = await response.text().catch(() => "");
    throw new BridgeUnavailableError(
      `桥接返回 ${response.status}${body ? `：${body.slice(0, 200)}` : ""}`,
    );
  }
  if (!response.body) {
    throw new BridgeUnavailableError("桥接推理响应没有流式 body");
  }
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let pending = "";
  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    pending += decoder.decode(value, { stream: true });
    let newline = pending.indexOf("\n");
    while (newline >= 0) {
      const line = pending.slice(0, newline);
      pending = pending.slice(newline + 1);
      if (line.trim()) yield line;
      newline = pending.indexOf("\n");
    }
  }
  pending += decoder.decode();
  if (pending.trim()) yield pending;
}
