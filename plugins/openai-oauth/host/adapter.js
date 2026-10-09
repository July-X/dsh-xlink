/**
 * `LlmAdapter` 实现：把 dsh 的模型能力查询转发到本地 Rust 桥接。
 *
 * 必须继承运行时 `LlmAdapter`（P0 调查 §5 实测：裸对象在
 * `registerAdapter` 内部就会因缺少默认方法被拒），可选方法（重试策略、
 * 图像计价）沿用类默认。
 *
 * 流式推理与图片附件经本地桥接转发；图片字节由内核附件服务读取并校验。
 */
import { LlmAdapter, LlmError, requestImageHandleText, offloadedImageText, resolveImageAttachmentAccess, requiredImageOffload } from "@deepseek-ai/dsh-llm";
import { PROVIDER_DISPLAY_NAME, PROVIDER_ID } from "./constants.js";
import { BridgeUnavailableError, fetchCatalog, handshake, streamInferenceLines } from "./bridge.js";
import { buildEnvelope, pumpStream, toProviderToolName } from "./request.js";
import { prepareImageParts } from "./images.js";

export class BridgeAdapter extends LlmAdapter {
  constructor(bridge, pluginVersion, context) {
    super();
    this.bridge = bridge;
    this.pluginVersion = pluginVersion;
    this.catalogRevision = undefined;
    this.modelsById = new Map();
    this.catalogView = undefined;
    this.context = context;
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
      inputModalities: ['text', 'image'],
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
      inputModalities: ['text', 'image'],
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
    const attachments = this.context?.get('attachments');
    const imageParts = await prepareImageParts(options.messages ?? [], {
      attachments, handleText: requestImageHandleText, offloadedText: offloadedImageText,
      resolveAccess: (ref) => resolveImageAttachmentAccess(attachments,
        (path) => this.context?.get('fs')?.processPathFromHostPath(path), ref),
      requiredOffload: requiredImageOffload,
    }, options.signal).catch((error) => {
      if (error.code === 'IMAGE_OFFLOAD_REQUIRED') {
        throw new LlmError(error.message, error.code, { offloadImages: error.offloadImages });
      }
      throw error;
    });
    const envelope = buildEnvelope(options, this.catalogView, imageParts);
    const toolMapping = new Map();
    for (const tool of options.tools ?? []) toProviderToolName(tool.name, toolMapping);
    const lines = streamInferenceLines(this.bridge, envelope, options.signal);
    yield* pumpStream(lines, toolMapping);
  }
}

/** 握手 + 适配器构造；桥接未配置时返回 undefined（由调用方决定注册策略）。 */
export async function connectBridge(bridge, pluginVersion, signal, context) {
  await handshake(bridge, pluginVersion, signal);
  return new BridgeAdapter(bridge, pluginVersion, context);
}

export { BridgeUnavailableError, PROVIDER_ID };
