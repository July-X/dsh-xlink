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
import { unwatchFile, watchFile, writeFileSync } from "node:fs";
import {
  PACKAGE_ID,
  PROVIDER_ID,
  SELECTION_FILE_ENV,
  SELECTION_WATCH_DEBOUNCE_MS,
  SELECTION_WATCH_INTERVAL_MS,
  TEST_MARKER_ENV,
} from "./constants.js";
import { bridgeFromEnv } from "./bridge.js";
import { BridgeAdapter, connectBridge } from "./adapter.js";

// Config 必须从入口导出：设置服务按 profile entry 的 Config schema 派生
// 命名空间视图，没有它 provider 行进不了设置页（见 config.js 头注）。
import { Config } from "./config.js";

const inject = ["llm"];
const name = PACKAGE_ID;

/**
 * 常用模型偏好文件的变更通知。
 *
 * 用户在设置页勾完模型，会话的模型选择器必须按**新**列表列——而内核 webui
 * 的模型目录是按「Host 代次」缓存的（`ModelCatalogDirectory` 只在连接重建、
 * `llm/adapters-updated`、设置 / 凭据变更这几处重拉）。壳写文件时没法直接
 * 通知内核，所以这里盯着那只文件：stat 一变就用注册句柄 `replace` 一次，
 * 把这枚事件广播出去（`dsh-llm` 的 `commitRoutes` 每次替换都会发）。
 *
 * 观察失败一律降级（返回 undefined，只写日志、**不抛**）：拿不到实时刷新时
 * 用户刷新一次工作台页面照样能看到新列表，不值得为此让插件激活失败。
 *
 * `interval` 可注入（默认 2s）：单测拿它把 2 秒的 stat 节拍压到毫秒级。
 */
function watchSelectionFile(registered, file, warn, interval = SELECTION_WATCH_INTERVAL_MS) {
  if (!file || typeof registered?.replace !== "function") return undefined;
  let pending;
  const listener = (current, previous) => {
    if (current.mtimeMs === previous.mtimeMs && current.size === previous.size) return;
    clearTimeout(pending);
    // 一次原子写会先落临时文件再改名，stat 可能连跳两拍：合并成一拍再通知。
    pending = setTimeout(() => {
      try {
        registered.replace([PROVIDER_ID]);
      } catch (error) {
        warn?.(
          `常用模型变更通知失败（偏好已保存，刷新工作台页面即可生效）：${String(error?.message ?? error)}`,
        );
      }
    }, SELECTION_WATCH_DEBOUNCE_MS);
  };
  try {
    watchFile(file, { interval }, listener)?.unref?.();
  } catch (error) {
    warn?.(`无法观察常用模型偏好文件（勾选仍生效，改完刷新页面即可）：${String(error)}`);
    return undefined;
  }
  return () => {
    clearTimeout(pending);
    unwatchFile(file, listener);
  };
}

async function apply(ctx, config = {}) {
  const settingsNs = ctx.fiber.entry?.options.id ?? PACKAGE_ID;
  const pluginVersion = ctx.fiber.entry?.options.version ?? "0.1.0";

  // **不注册 configurable provider 目录行**：行卡是内核渲染的，自带一个
  // 「编辑」按钮——而本插件没有任何可编辑配置（Config 是空 schema），
  // 那个按钮只会打开一个空表单（2026-10-09 用户反馈：移除编辑按钮）。
  // 账户卡因此挂在 `settings.models.footer` 插槽（client.js），标题与
  // 「如何移除」说明完全由插件自绘。模型与推理走 `registerAdapter`，
  // 与目录行无关，会话模型选择器不受影响。

  const bridge = bridgeFromEnv();
  let adapter;
  let bridgeError;
  if (bridge !== undefined) {
    try {
      adapter = await connectBridge(bridge, pluginVersion, undefined, ctx);
    } catch (error) {
      bridgeError = { code: error?.code ?? "BRIDGE_HANDSHAKE_FAILED", message: String(error?.message ?? error) };
    }
  }
  const registered = ctx.llm.registerAdapter(
    [PROVIDER_ID],
    adapter ?? new BridgeAdapter(bridge, pluginVersion, ctx),
  );
  if (bridge === undefined) {
    ctx.logger?.warn?.(`${name}: 桥接服务未随本次启动提供（${PROVIDER_ID} 的目录与推理不可用）`);
  } else if (bridgeError !== undefined) {
    ctx.logger?.warn?.(`${name}: 桥接握手失败：${bridgeError.message}`);
  }

  // 常用模型的变更通知：偏好文件一变就重播一次适配器注册（见上方函数头注）。
  // 文件路径由壳经环境传入；没有它（旧壳 / 未接线）就不观察，勾选照样在下次
  // 拉目录时生效。
  const stopWatchingSelection = watchSelectionFile(
    registered,
    process.env[SELECTION_FILE_ENV],
    (message) => ctx.logger?.warn?.(`${name}: ${message}`),
  );
  if (stopWatchingSelection !== undefined) {
    try {
      // 停用时收掉 stat 轮询：内核进程里不该留下没人再用的观察者。
      ctx.on?.("dispose", stopWatchingSelection);
    } catch (error) {
      stopWatchingSelection();
      ctx.logger?.warn?.(`${name}: 观察器收尾登记失败：${String(error?.message ?? error)}`);
    }
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
        // 未勾选（不在 `selected` 里）的模型仍要能解析：取消勾选不该让正在用
        // 它的会话报 MODEL_NOT_IN_CATALOG。取「适配器手里的完整目录」减掉
        // listModels 给出的那些——这条只在冒烟标记里跑，生产不执行。
        const unlisted = [...adapter.modelsById.keys()].find(
          (id) => !models.some((model) => model.id === id),
        );
        const resolvedUnlisted =
          unlisted === undefined ? undefined : await adapter.resolveModel(PROVIDER_ID, unlisted);
        catalog = {
          count: models.length,
          first: models[0]?.id,
          unlisted,
          unlistedResolvable: resolvedUnlisted !== undefined,
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

export { apply, inject, name, Config, PROVIDER_ID };
// 测试缝：变更通知的节拍可注入（生产调用点不传第四个参数）。
export { watchSelectionFile };
