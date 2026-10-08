/**
 * `LlmAdapter` 实现：把 dsh 的模型能力查询转发到本地 Rust 桥接。
 *
 * 必须继承运行时 `LlmAdapter`（P0 调查 §5 实测：裸对象在
 * `registerAdapter` 内部就会因缺少默认方法被拒），可选方法（重试策略、
 * 图像计价）沿用类默认。
 *
 * 流式推理是 P2/P4 交付：`stream` 目前抛出带稳定 code 的
 * `LlmError`，不制造「看起来能对话」的假绿。
 */
import { LlmAdapter, LlmError } from "@deepseek-ai/dsh-llm";
import { PROVIDER_DISPLAY_NAME, PROVIDER_ID } from "./constants.js";
import { BridgeUnavailableError, fetchCatalog, handshake, streamInferenceLines } from "./bridge.js";
import { buildEnvelope, pumpStream } from "./request.js";

export class BridgeAdapter extends LlmAdapter {
  constructor(bridge, pluginVersion) {
    super();
    this.bridge = bridge;
    this.pluginVersion = pluginVersion;
    this.catalogRevision = undefined;
    this.modelsById = new Map();
    this.catalogView = undefined;
  }

  providerInfo(id) {
    return { id, name: PROVIDER_DISPLAY_NAME };
  }

  /** 拉取并缓存目录；账号代次由桥接侧保证（同一令牌一个代次）。 */
  async #refreshCatalog(signal) {
    const catalog = await fetchCatalog(this.bridge, signal);
    if (!Array.isArray(catalog?.models)) {
      throw new LlmError("桥接目录响应缺少 models 数组", "BRIDGE_BAD_CATALOG");
    }
    this.catalogRevision = String(catalog.revision ?? "");
    this.modelsById = new Map(catalog.models.map((model) => [String(model.id), model]));
    // 目录视图（buildEnvelope 用）：revision 由桥接拼好（目录|能力表双版本）。
    this.catalogView = { revisionCombined: this.catalogRevision, entries: catalog.models };
  }

  async listModels(provider, signal) {
    await this.#refreshCatalog(signal);
    return [...this.modelsById.values()].map((model) => ({
      provider,
      id: String(model.id),
      name: String(model.name ?? model.id),
      ...(model.description !== undefined ? { description: String(model.description) } : {}),
    }));
  }

  async resolveModel(provider, model, signal) {
    if (this.modelsById.size === 0 || signal?.aborted) await this.#refreshCatalog(signal);
    const entry = this.modelsById.get(model);
    if (entry === undefined) {
      throw new LlmError(`模型 ${model} 不在当前账号目录（revision ${this.catalogRevision ?? "?"}）`, "MODEL_NOT_IN_CATALOG");
    }
    return {
      provider,
      id: model,
      name: String(entry.name ?? model),
      ...(typeof entry.contextWindow === "number"
        ? { context: { contextWindow: entry.contextWindow } }
        : {}),
      ...(Array.isArray(entry.efforts) && entry.efforts.length > 0
        ? {
            reasoning: {
              efforts: entry.efforts.map((effort) => ({
                id: String(effort.id),
                name: String(effort.name ?? effort.id),
              })),
            },
          }
        : {}),
    };
  }

  async *stream(options) {
    // 目录视图缺失（未经 listModels）时先刷新一次——发送前校验双 revision
    // 的前提是 Host 手里有当前目录。
    if (this.catalogView === undefined) await this.#refreshCatalog(options.signal);
    const envelope = buildEnvelope(options, this.catalogView);
    const lines = streamInferenceLines(this.bridge, envelope, options.signal);
    yield* pumpStream(lines);
  }
}

/** 握手 + 适配器构造；桥接未配置时返回 undefined（由调用方决定注册策略）。 */
export async function connectBridge(bridge, pluginVersion, signal) {
  await handshake(bridge, pluginVersion, signal);
  return new BridgeAdapter(bridge, pluginVersion);
}

export { BridgeUnavailableError, PROVIDER_ID };
