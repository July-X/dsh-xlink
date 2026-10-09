/**
 * Client 入口：在「设置 → 模型」页尾（`settings.models.footer` 插槽）挂
 * 本插件的账户卡。
 *
 * 束格式是内核客户端模块系统的既定方言（P0 调查 §6）：
 * `window.__ModuleLoader__.load({id, factory})` 以普通脚本供给，factory
 * 内 `require` 由浏览器模块图解析（react 等来自基础图）。因此本文件
 * **必须自包含**：不能出现顶层 ESM import；下方双语字典是
 * `locales/{zh,en}.js` 的内联孪生（构建步骤落地前手工保持同步）。
 *
 * 卡片挂 footer（list 插槽，注册 id = 插件 id）而不是 provider 行卡：
 * 行卡是内核渲染的，自带「编辑」按钮——本插件没有可编辑配置，空表单
 * 只会困惑（2026-10-09 用户反馈：移除编辑按钮，标题说明来源与移除入口）。
 * Host 侧因此也不再注册 configurable provider 目录行。
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
    providedBy:
      "本插件由 dsh-xlink 桌面端提供；如需移除插件，请到主面板的「插件」页操作。",
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
    providedBy:
      "This plugin is provided by the dsh-xlink desktop app; to remove it, use the Plugins page in the management panel.",
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
      // 后台授权流的失败原因（壳侧 lastError）：流程在后台线程跑，命令
      // 不抛错——不把 lastError 画出来，失败就只是「静默回到未登录」，
      // 用户永远不知道要开系统代理（2026-10-09 实测踩坑）。
      const flowError = authorizing ? null : (status?.lastError ?? "");
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
        { "data-xlink-openai-oauth": "card", style: { display: "grid", gap: 6 } },
        e("div", { style: { display: "flex", alignItems: "center", gap: 8 } }, [
          e(
            "span",
            { key: "s", style: { fontSize: 12, color: "var(--text-muted, #8a8f98)" } },
            `${L.statusPrefix}${statusText}`,
          ),
        ]),
        error ? e("div", { key: "err", style: { fontSize: 12, color: "var(--danger, #d4484a)" } }, error) : null,
        flowError
          ? e(
              "div",
              {
                key: "flow-err",
                style: {
                  fontSize: 12,
                  color: "var(--danger, #d4484a)",
                  overflowWrap: "anywhere",
                },
              },
              flowError,
            )
          : null,
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

    /** 页尾卡：标题（带来源 tooltip）+ 账户区。 */
    function FooterCard() {
      return e(
        "div",
        {
          "data-xlink-openai-oauth": "footer-card",
          style: {
            border: "1px solid var(--border, #d0d3d9)",
            borderRadius: 8,
            padding: "12px 14px",
            display: "grid",
            gap: 6,
          },
        },
        e(
          "div",
          { style: { display: "flex", alignItems: "center", gap: 8 } },
          e(
            "span",
            {
              key: "t",
              style: { fontWeight: 600, cursor: "help" },
              title: L.providedBy,
            },
            L.cardTitle,
          ),
        ),
        e(AccountCard),
      );
    }

    function apply(ctx) {
      ctx.slots.inject("settings.models.footer", () =>
        ctx.slots.register(
          {
            name: "settings.models.footer",
            id: "xlink-openai-oauth",
          },
          FooterCard,
        ),
      );
    }

    return { apply, inject: ["slots"] };
  },
});
