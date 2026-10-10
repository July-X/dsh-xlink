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
 * 样式用内核 webui 的 `--dsw-alias-*` 主题令牌（与 provider 行卡同源，
 * 明暗主题自动跟随）；按钮配方照抄内核 secondaryButton。令牌在 apply()
 * 时随一个 <style> 注入（内联样式做不了 :hover/:disabled）。
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
    refreshing: "刷新中…",
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
    refreshing: "Refreshing…",
    refreshFailed: "Failed to refresh model catalog",
    phaseAuthorizing: "Sign-in in progress: the system browser will open…",
    phaseAuthorized: "Signed in",
    phaseSignedOut: "Not signed in",
    statusPrefix: "Status: ",
    bridgeMissing: "Desktop shell bridge unavailable",
  },
};
const L = STRINGS[(globalThis.navigator?.language ?? "zh").toLowerCase().startsWith("zh") ? "zh" : "en"];

/** 主题样式：令牌与内核 webui 同源（--dsw-alias-*），明暗主题自动跟随。 */
const CARD_STYLE_ID = "xlink-openai-oauth-style";
const CARD_CSS = `
.xlink-oauth-card{border:.5px solid var(--dsw-alias-settings-card-stroke);background:var(--dsw-alias-settings-card-fill);border-radius:var(--dsw-radius-lg);padding:12px 14px;display:grid;gap:8px}
.xlink-oauth-title{font-weight:600;cursor:help}
.xlink-oauth-status{font-size:12px;color:var(--dsw-alias-label-secondary)}
.xlink-oauth-error{font-size:12px;color:var(--dsw-alias-state-error-primary);overflow-wrap:anywhere}
.xlink-oauth-actions{display:flex;gap:8px;flex-wrap:wrap}
.xlink-oauth-btn{box-sizing:border-box;border-radius:var(--dsw-radius-md);height:32px;font:inherit;cursor:pointer;border:.5px solid var(--dsw-alias-border-l3);color:var(--dsw-alias-label-primary);background:0 0;padding:0 14px;font-size:13px;display:inline-flex;align-items:center;justify-content:center;gap:4px}
.xlink-oauth-btn:hover:not(:disabled){background:var(--dsw-alias-interactive-bg-hover)}
.xlink-oauth-btn:disabled{opacity:.45;cursor:default}
`;

function invokeOrNull() {
  const api = globalThis.window?.__TAURI__?.core;
  return api && typeof api.invoke === "function" ? api.invoke : null;
}

/** 主题样式只注入一次（幂等：重复 apply 不重复追加 <style>）。 */
function ensureCardStyle(documentRef) {
  if (documentRef.getElementById(CARD_STYLE_ID)) return;
  const style = documentRef.createElement("style");
  style.id = CARD_STYLE_ID;
  style.textContent = CARD_CSS;
  documentRef.head.appendChild(style);
}

window.__ModuleLoader__.load({
  id: "xlink-openai-oauth",
  factory: (require) => {
    const React = require("react");
    const e = React.createElement;

    function AccountCard() {
      const [status, setStatus] = React.useState(null);
      const [busy, setBusy] = React.useState(false);
      const [lastCommand, setLastCommand] = React.useState("");
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

      // 主面板也能登录/退出：所有相位都回读同一份壳侧状态，避免另一端
      // 退出后这张卡永久停在已登录。授权中缩短间隔；卸载时清理。
      React.useEffect(() => {
        const authorizing = status?.phase === "authorizing";
        const timer = setInterval(refresh, authorizing ? 2000 : 5000);
        return () => clearInterval(timer);
      }, [status?.phase, refresh]);

      const run = async (command) => {
        setBusy(true);
        setLastCommand(command);
        setError("");
        try {
          setStatus(await invokeOrNull()(command));
        } catch (invokeError) {
          setError(String(invokeError));
        } finally {
          setBusy(false);
          refresh();
        }
      };

      const phase = status?.phase ?? "signed-out";
      const authorizing = phase === "authorizing";
      const authorized = phase === "authorized";
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

      // 按钮按相位出现（2026-10-09 用户反馈：已登录不应再显示登录按钮）：
      // 未登录 → 登录；授权中 → 取消；已登录 → 刷新 + 退出；需重新登录
      // → 登录 + 退出。「刷新模型列表」点击后挂加载态（文案切换 + 禁用）。
      const refreshing = busy && lastCommand === "openai_catalog_refresh";
      const actions = [];
      if (authorizing) {
        actions.push(
          e(
            "button",
            { key: "cancel", onClick: () => run("openai_authorize_cancel"), disabled: busy, className: "xlink-oauth-btn" },
            L.cancel,
          ),
        );
      } else {
        if (phase !== "authorized") {
          actions.push(
            e(
              "button",
              {
                key: "login",
                onClick: () => run("openai_authorize_start"),
                disabled: busy || status?.pluginSourceAvailable === false || !!status?.stateError,
                className: "xlink-oauth-btn",
              },
              L.signIn,
            ),
          );
        }
        if (authorized) {
          actions.push(
            e(
              "button",
              {
                key: "refresh",
                onClick: () => run("openai_catalog_refresh"),
                disabled: busy,
                className: "xlink-oauth-btn",
              },
              refreshing ? L.refreshing : L.refreshModels,
            ),
          );
        }
        if (authorized || phase === "reauth-required") {
          actions.push(
            e(
              "button",
              { key: "logout", onClick: () => run("openai_logout"), disabled: busy, className: "xlink-oauth-btn" },
              L.signOut,
            ),
          );
        }
      }

      return e(
        "div",
        { "data-xlink-openai-oauth": "card", style: { display: "grid", gap: 8 } },
        e("div", { className: "xlink-oauth-status" }, `${L.statusPrefix}${statusText}`),
        error ? e("div", { className: "xlink-oauth-error" }, error) : null,
        flowError ? e("div", { className: "xlink-oauth-error" }, flowError) : null,
        e("div", { className: "xlink-oauth-actions" }, actions),
      );
    }

    /** 页尾卡：标题（带来源 tooltip）+ 账户区。 */
    function FooterCard() {
      return e(
        "div",
        { className: "xlink-oauth-card", "data-xlink-openai-oauth": "footer-card" },
        e(
          "div",
          { style: { display: "flex", alignItems: "center", gap: 8 } },
          e("span", { className: "xlink-oauth-title", title: L.providedBy }, L.cardTitle),
        ),
        e(AccountCard),
      );
    }

    function apply(ctx) {
      // 样式注入只走浏览器 document——**不碰 ctx**：dsh 的插件 ctx 是服务
      // 代理，访问未注册的服务名会直接抛错，activation 即失败
      // （2026-10-09 实测：「1 entry did not activate」）。
      try {
        const documentRef = globalThis.window?.document;
        if (documentRef?.head) ensureCardStyle(documentRef);
      } catch (error) {
        // 样式注入失败不阻断激活：卡片退化为无 hover 的内联观感。
        (globalThis.console?.error ?? (() => {}))(`[xlink-openai-oauth] 样式注入失败：${error}`);
      }
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
