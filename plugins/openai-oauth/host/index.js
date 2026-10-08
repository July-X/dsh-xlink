/**
 * Host 入口：声明提供方目录 + 注册桥接适配器。
 *
 * 接线形状（P0 调查 §5/§6 实测）：profile `cordis.patch.yml` 的
 * `insert:` 行以相对路径指向本文件（必须直指入口 JS，ESM 无目录导入），
 * 行 id 即 settingsNs；peer 符号链接把 `@deepseek-ai/dsh-llm` 解析到
 * 目标内核树的同一份实现。
 *
 * 桥接未随启动提供时（P1 阶段的常态）：仍然注册目录行与适配器——
 * 适配器的目录/解析调用会得到 `BRIDGE_UNAVAILABLE`，工作台其余功能
 * 不受影响。这对应设计文档「模型连接状态不能由『有一份令牌』推导」。
 */
import { writeFileSync } from "node:fs";
import { PACKAGE_ID, PROVIDER_DISPLAY_NAME, PROVIDER_ID, TEST_MARKER_ENV } from "./constants.js";
import { bridgeFromEnv } from "./bridge.js";
import { BridgeAdapter, connectBridge } from "./adapter.js";

const inject = ["llm"];
const name = PACKAGE_ID;

async function apply(ctx, config = {}) {
  const settingsNs = ctx.fiber.entry?.options.id ?? PACKAGE_ID;
  const pluginVersion = ctx.fiber.entry?.options.version ?? "0.1.0";

  ctx.llm.registerConfigurableProviders([{
    provider: PROVIDER_ID,
    displayName: PROVIDER_DISPLAY_NAME,
    settingsNs,
    settingsPath: [],
  }]);

  const bridge = bridgeFromEnv();
  let adapter;
  let bridgeError;
  if (bridge !== undefined) {
    try {
      adapter = await connectBridge(bridge, pluginVersion);
    } catch (error) {
      bridgeError = { code: error?.code ?? "BRIDGE_HANDSHAKE_FAILED", message: String(error?.message ?? error) };
    }
  }
  const registered = ctx.llm.registerAdapter(
    [PROVIDER_ID],
    adapter ?? new BridgeAdapter(bridge, pluginVersion),
  );
  if (bridge === undefined) {
    ctx.logger?.warn?.(`${name}: 桥接服务未随本次启动提供（${PROVIDER_ID} 的目录与推理不可用）`);
  } else if (bridgeError !== undefined) {
    ctx.logger?.warn?.(`${name}: 桥接握手失败：${bridgeError.message}`);
  }

  const marker = process.env[TEST_MARKER_ENV];
  if (marker !== undefined && marker !== "") {
    // 冒烟走真实路径：经注册的适配器拉目录并解析一次模型，把能力
    // （contextWindow / efforts）带进标记文件。生产启动不设此变量。
    let catalog = { error: "bridge-unavailable" };
    if (adapter !== undefined) {
      try {
        const models = await adapter.listModels(PROVIDER_ID);
        const resolved = models.length > 0 ? await adapter.resolveModel(PROVIDER_ID, models[0].id) : undefined;
        catalog = {
          count: models.length,
          first: models[0]?.id,
          resolvedContext: resolved?.context?.contextWindow,
          resolvedEfforts: resolved?.reasoning?.efforts?.map((effort) => effort.id),
        };
      } catch (error) {
        catalog = { error: String(error?.message ?? error), code: error?.code ?? error?.failure?.code };
      }
    }
    writeFileSync(marker, JSON.stringify({
      ok: true,
      provider: PROVIDER_ID,
      settingsNs,
      bridgeProvided: bridge !== undefined,
      bridgeHandshakeOk: adapter !== undefined,
      bridgeError,
      catalog,
      replaceIsFunction: typeof registered?.replace === "function",
    }, null, 2));
    ctx.logger?.info?.(`${name}: test marker written`);
  }
}

export { apply, inject, name, PROVIDER_ID };
