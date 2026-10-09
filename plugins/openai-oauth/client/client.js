/**
 * Client 入口：在「设置 → 模型」的提供方卡上挂本插件的账户卡区域。
 *
 * 束格式是内核客户端模块系统的既定方言（P0 调查 §6）：
 * `window.__ModuleLoader__.load({id, factory})` 以普通脚本供给，factory
 * 内 `require` 由浏览器模块图解析（react 等来自基础图）。因此本文件
 * **必须自包含**：不能出现顶层 ESM import；下方双语字典是
 * `locales/{zh,en}.js` 的内联孪生（构建步骤落地前手工保持同步）。
 *
 * 插槽按 `settingsNs` keyed 派发——key 必须与 Host 侧接线行 id
 * （`xlink-openai-oauth`）一致。
 *
 * 账户动作走桌面壳的 Tauri 命令（`window.__TAURI__.core.invoke`，授权见
 * `capabilities/harness-remote.json` 的 allow-openai-account）：令牌与
 * 系统凭据库都在桌面壳侧，浏览器只看脱敏状态（设计 §3.2/§8）。
 * `__TAURI__` 在本束执行时未必就绪（harness-draft.js 同款问题）——
 * 惰性取用 + 组件挂载后重试。
 */
const STRINGS = {
  zh: {
    cardTitle: "OpenAI · ChatGPT 套餐",
    signIn: "登录 ChatGPT",
    cancel: "取消登录",
    signOut: "退出登录",
    refreshModels: "刷新模型列表",
    refreshFailed: "刷新模型目录失败",
    phaseAuthorizing: "登录进行中：系统浏览器即将打开授权页…",
    phaseAuthorized: "已登录",
    phaseSignedOut: "未登录",
    statusPrefix: "状态：",
    bridgeMissing: "桌面壳连接不可用（未注入 Tauri 桥）",
  },
  en: {
    cardTitle: "OpenAI · ChatGPT plan",
    signIn: "Sign in with ChatGPT",
    cancel: "Cancel sign-in",
    signOut: "Sign out",
    refreshModels: "Refresh model list",
    refreshFailed: "Failed to refresh model catalog",
    phaseAuthorizing: "Sign-in in progress: the system browser will open…",
    phaseAuthorized: "Signed in",
    phaseSignedOut: "Not signed in",
    statusPrefix: "Status: ",
    bridgeMissing: "Desktop shell bridge unavailable",
  },
};
const L = STRINGS[(globalThis.navigator?.language ?? "zh").toLowerCase().startsWith("zh") ? "zh" : "en"];

function invokeOrNull() {
  const api = globalThis.window?.__TAURI__?.core;
  return api && typeof api.invoke === "function" ? api.invoke : null;
}

window.__ModuleLoader__.load({
  id: "xlink-openai-oauth",
  factory: (require) => {
    const React = require("react");
    const e = React.createElement;

    function AccountCard() {
      const [status, setStatus] = React.useState(null);
      const [busy, setBusy] = React.useState(false);
      const [error, setError] = React.useState("");

      const refresh = React.useCallback(async () => {
        const invoke = invokeOrNull();
        if (invoke === null) {
          setStatus((prev) => ({ ...(prev ?? {}), phase: "signed-out", note: L.bridgeMissing }));
          return;
        }
        try {
          setStatus(await invoke("openai_account_status"));
        } catch (error) {
          setStatus((prev) => ({ ...(prev ?? {}), phase: prev?.phase ?? "signed-out", note: String(error) }));
        }
      }, []);

      React.useEffect(() => {
        // __TAURI__ 注入可能晚于本束执行：挂载后短重试两次再放弃。
        let attempts = 0;
        const tick = () => {
          attempts += 1;
          refresh();
          if (attempts < 3 && !window.__TAURI__) setTimeout(tick, 800);
        };
        tick();
      }, [refresh]);

      // 登录进行中每 2 秒刷新一次（flow 结束会落库，状态随即翻转）。
      React.useEffect(() => {
        if (status?.phase !== "authorizing") return undefined;
        const timer = setInterval(refresh, 2000);
        return () => clearInterval(timer);
      }, [status?.phase, refresh]);

      const run = async (command) => {
        setBusy(true);
        setError("");
        try {
          setStatus(await invokeOrNull()(command));
          if (command === "openai_logout") setStatus((prev) => ({ ...prev, phase: "signed-out" }));
        } catch (invokeError) {
          setError(String(invokeError));
        } finally {
          setBusy(false);
          refresh();
        }
      };

      const phase = status?.phase ?? "signed-out";
      const authorizing = phase === "authorizing";
      const statusText =
        status?.stateError ??
        status?.note ??
        (authorizing
          ? L.phaseAuthorizing
          : phase === "authorized"
            ? `${L.phaseAuthorized}${status?.email ? ` · ${status.email}` : ""}`
            : L.phaseSignedOut);

      return e(
        "div",
        { "data-xlink-openai-oauth": "card", style: { padding: "8px 0", display: "grid", gap: 6 } },
        e("div", { style: { display: "flex", alignItems: "center", gap: 8 } }, [
          e("span", { key: "t", style: { fontWeight: 600 } }, L.cardTitle),
          e(
            "span",
            { key: "s", style: { fontSize: 12, color: "var(--text-muted, #8a8f98)" } },
            `${L.statusPrefix}${statusText}`,
          ),
        ]),
        error ? e("div", { key: "err", style: { fontSize: 12, color: "var(--danger, #d4484a)" } }, error) : null,
        e("div", { key: "actions", style: { display: "flex", gap: 8 } }, [
          authorizing
            ? e(
                "button",
                {
                  key: "cancel",
                  onClick: () => run("openai_authorize_cancel"),
                  disabled: busy,
                  style: cardButtonStyle(),
                },
                L.cancel,
              )
            : e(
                "button",
                {
                  key: "login",
                  onClick: () => run("openai_authorize_start"),
                  disabled: busy || status?.pluginSourceAvailable === false || !!status?.stateError,
                  style: cardButtonStyle(),
                },
                L.signIn,
              ),
          phase === "authorized"
            ? [
                e(
                  "button",
                  {
                    key: "refresh",
                    onClick: () => run("openai_catalog_refresh"),
                    disabled: busy,
                    style: cardButtonStyle(),
                  },
                  L.refreshModels,
                ),
                e(
                  "button",
                  {
                    key: "logout",
                    onClick: () => run("openai_logout"),
                    disabled: busy,
                    style: cardButtonStyle(),
                  },
                  L.signOut,
                ),
              ]
            : null,
        ]),
      );
    }

    function cardButtonStyle() {
      return {
        padding: "4px 12px",
        borderRadius: 6,
        border: "1px solid var(--border, #d0d3d9)",
        background: "var(--surface, #fff)",
        color: "var(--text, #1f2328)",
        cursor: "pointer",
        fontSize: 12,
      };
    }

    function apply(ctx) {
      ctx.slots.inject("settings.models.provider-card", () =>
        ctx.slots.register(
          {
            name: "settings.models.provider-card",
            key: "xlink-openai-oauth",
          },
          ProviderCard,
        ),
      );
    }

    return { apply, inject: ["slots"] };
  },
});
