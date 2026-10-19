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
import * as dshLlm from "@deepseek-ai/dsh-llm";
import { PROVIDER_DISPLAY_NAME, PROVIDER_ID } from "./constants.js";
import { BridgeUnavailableError, fetchCatalog, handshake, streamInferenceLines } from "./bridge.js";
import { buildEnvelope, pumpStream, toProviderToolName } from "./request.js";
import { prepareImageParts } from "./images.js";

/**
 * 内核的额度码（`dsh-llm` 的 `QUOTA_EXCEEDED_CODE`）。
 *
 * 走命名空间 import 再取，而不是具名 import：这个插件要能装到比当前内核更
 * 老的版本上（README 记的三版本实测窗口），具名导入在链接期就要求导出存在，
 * 老内核缺它会让**整个插件加载失败**；取不到时退回字面量——`QUOTA` 是内核
 * 公布的稳定契约值，不会随版本改。
 */
const QUOTA_CODE = dshLlm.QUOTA_EXCEEDED_CODE ?? "QUOTA";

/**
 * 桥接/协议层的失败 → 内核错误类型。
 *
 * 内核只信任 `HarnessError` 的 code（`normalizeLlmFailure` 对其他 Error 一律
 * 记 `UNKNOWN`），所以额度类失败必须在这里换成 `LlmError`：否则 UI 拿不到
 * 「额度已用尽」的本地化提示与用量入口，重试策略也把它当未知错误。其余
 * 失败原样抛（内核记 UNKNOWN，与修复前一致）。
 */
function asLlmFailure(error) {
  if (error instanceof LlmError) return error;
  if (error?.code === "QUOTA") {
    return new LlmError(error.message, QUOTA_CODE, { cause: error });
  }
  return error;
}

export class BridgeAdapter extends LlmAdapter {
  constructor(bridge, pluginVersion, context) {
    super();
    this.bridge = bridge;
    this.pluginVersion = pluginVersion;
    this.catalogRevision = undefined;
    this.modelsById = new Map();
    this.catalogView = undefined;
    // 常用模型（设置页勾选的那一份）：**只用来过滤 listModels**。
    // undefined = 载荷没带（旧壳 / 冒烟桩）或集合为空 → 不过滤。
    this.selectedIds = undefined;
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
    // **完整目录**：发送前的 revision / 白名单校验按账号目录走，不受常用
    // 模型勾选影响。
    this.catalogView = { revisionCombined: this.catalogRevision, entries: catalog.models };
    this.selectedIds =
      Array.isArray(catalog.selected) && catalog.selected.length > 0
        ? new Set(catalog.selected.map(String))
        : undefined;
  }

  /**
   * 会话选择器里列出的模型 = 账号目录 ∩ 常用模型。
   *
   * `resolveModel` 刻意**不过滤**（见下）：取消勾选某个模型之后，已经用着
   * 它的会话仍要能解析出模型，否则报的是 `MODEL_NOT_IN_CATALOG`——那条
   * 会话的报错出现在会话里，而原因在设置页的勾选框上，查起来对不上号。
   */
  async listModels(provider, signal) {
    await this.#refreshCatalog(signal);
    const models = [...this.modelsById.values()].filter(
      (model) => this.selectedIds === undefined || this.selectedIds.has(String(model.id)),
    );
    return models.map((model) => ({
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
    try {
      yield* pumpStream(lines, toolMapping);
    } catch (error) {
      throw asLlmFailure(error);
    }
  }
}

/** 握手 + 适配器构造；桥接未配置时返回 undefined（由调用方决定注册策略）。 */
export async function connectBridge(bridge, pluginVersion, signal, context) {
  await handshake(bridge, pluginVersion, signal);
  return new BridgeAdapter(bridge, pluginVersion, context);
}

export { BridgeUnavailableError, PROVIDER_ID };
