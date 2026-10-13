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
 * 卡片下半部分是**常用模型勾选**（`openai_models_selection_get/_set`）：
 * 账号目录里的模型默认全都进会话的模型选择器，几十条里混着各种实验模型；
 * 这里让用户勾出常用的那几条，壳把集合落进 `shell/<mode>/openai-oauth/
 * model-selection.json`，桥接的 `/v1/models` 载荷带 `selected`，Host 适配器
 * 只过滤 `listModels`（`resolveModel` 仍走完整目录，见 host/adapter.js）。
 * 勾完不用刷新页面：Host 那边盯着这只文件，一变就重播一次适配器注册，
 * 内核随即让已打开的页面重拉模型目录（host/index.js 的 watchSelectionFile）。
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
    modelsTitle: "常用模型",
    modelsHint:
      "勾选的模型会出现在会话的模型选择器里；取消勾选的模型不再列出，正在用它的会话需要另选一个。",
    modelsLoading: "读取模型中…",
    modelsFailed: "读取常用模型失败",
    modelSaveFailed: "保存常用模型失败",
    restoreDefaults: "恢复默认",
    capabilityUnknown: "能力待验证",
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
    modelsTitle: "Commonly used models",
    modelsHint:
      "Checked models appear in the session model picker; unchecked models are no longer listed, and a session using one must pick another.",
    modelsLoading: "Loading models…",
    modelsFailed: "Failed to load the model list",
    modelSaveFailed: "Failed to save the model list",
    restoreDefaults: "Restore defaults",
    capabilityUnknown: "unverified",
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
.xlink-oauth-models{display:grid;gap:6px;border-top:.5px solid var(--dsw-alias-settings-card-stroke);padding-top:10px}
.xlink-oauth-models-head{display:flex;align-items:center;justify-content:space-between;gap:8px}
.xlink-oauth-models-title{font-weight:600;font-size:13px}
.xlink-oauth-link{border:0;background:0 0;color:var(--dsw-alias-label-secondary);font:inherit;font-size:12px;cursor:pointer;padding:0}
.xlink-oauth-link:hover:not(:disabled){color:var(--dsw-alias-label-primary)}
.xlink-oauth-link:disabled{opacity:.45;cursor:default}
.xlink-oauth-list{display:grid;gap:2px;max-height:220px;overflow-y:auto;border:.5px solid var(--dsw-alias-border-l3);border-radius:var(--dsw-radius-md);padding:6px 8px}
.xlink-oauth-item{display:flex;align-items:center;gap:8px;font-size:13px;cursor:pointer;padding:2px 0}
.xlink-oauth-item input{flex:none;margin:0}
.xlink-oauth-item-name{flex:none}
.xlink-oauth-item-id{color:var(--dsw-alias-label-secondary);font-size:11px;overflow-wrap:anywhere}
.xlink-oauth-item-note{color:var(--dsw-alias-label-secondary);font-size:11px;flex:none}
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
      // 「刷新模型列表」成功后让常用模型区重读一次（目录可能多 / 少了几条）。
      const [catalogNonce, setCatalogNonce] = React.useState(0);

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
        // 授权中 2s，其余 60s（与 ui/src/plugins/openaiAccount.js 同口径）。
        // 每次 refresh 都要经壳读一次系统凭据库，而那要 spawn 一个
        // `security` / PowerShell 子进程；固定 5s 意味着这个页签常开一天就是
        // 一万七千多次进程创建。「另一端退出后状态不同步」仍然有事件兜底，
        // 而这里的轮询本就只作兜底（注释上一条 effect 已经这么定位了）。
        const timer = setInterval(refresh, authorizing ? 2000 : 60000);
        return () => clearInterval(timer);
      }, [status?.phase, refresh]);

      const run = async (command) => {
        setBusy(true);
        setLastCommand(command);
        setError("");
        try {
          setStatus(await invokeOrNull()(command));
          if (command === "openai_catalog_refresh") setCatalogNonce((value) => value + 1);
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
        e(ModelSection, { phase, nonce: catalogNonce }),
      );
    }

    /**
     * 常用模型勾选区。
     *
     * 列表来自壳：`openai_models_selection_get` 给的是当前账号目录的**全部**
     * 条目加勾选状态；每次勾选把**整份**选中集合交回 `..._set`（而不是增量），
     * 壳按目录过滤后落盘。列表只有几十条、动作是用户点一下，整份提交换来的
     * 是「界面状态与磁盘状态永远一致」——增量接口一旦漏掉一条，界面上看不出
     * 任何异常，而会话里会多出或少掉一个模型。
     *
     * 未登录时不画这一区：那种情况下按不出任何模型，而卡片上方已经在说
     * 「未登录」了，多一行空列表只是噪声。
     */
    function ModelSection({ phase, nonce }) {
      const [view, setView] = React.useState(null);
      const [busy, setBusy] = React.useState(false);
      const [error, setError] = React.useState("");

      React.useEffect(() => {
        if (phase !== "authorized") return undefined;
        const invoke = invokeOrNull();
        if (invoke === null) {
          setError(L.bridgeMissing);
          return undefined;
        }
        // 卸载 / 换相位后到达的回程不许再写状态（否则已登录的旧列表会盖回
        // 一屏刚退出登录的卡片）。
        let cancelled = false;
        invoke("openai_models_selection_get")
          .then((next) => {
            if (cancelled) return;
            setView(next);
            setError("");
          })
          .catch((readError) => {
            if (!cancelled) setError(`${L.modelsFailed}：${String(readError)}`);
          });
        return () => {
          cancelled = true;
        };
      }, [phase, nonce]);

      if (phase !== "authorized") return null;

      const models = view?.models ?? [];
      const selectedCount = models.filter((model) => model.selected).length;
      const write = async (ids) => {
        setBusy(true);
        setError("");
        try {
          setView(await invokeOrNull()("openai_models_selection_set", { ids }));
        } catch (writeError) {
          setError(`${L.modelSaveFailed}：${String(writeError)}`);
        } finally {
          setBusy(false);
        }
      };
      const toggle = (model, checked) => {
        if (busy || view === null) return;
        const ids = models
          .filter((entry) => (entry.id === model.id ? checked : entry.selected))
          .map((entry) => entry.id);
        // 空集会让这个提供方从会话选择器里整组消失：最后一枚在 UI 上是禁用的，
        // 这里再挡一次（钩子里的栈多半来自键盘 / 程序化触发）。
        if (ids.length === 0) return;
        write(ids);
      };

      return e(
        "div",
        { className: "xlink-oauth-models", "data-xlink-openai-oauth": "models" },
        e(
          "div",
          { className: "xlink-oauth-models-head" },
          e("span", { className: "xlink-oauth-models-title" }, L.modelsTitle),
          (view?.recommended?.length ?? 0) > 0
            ? e(
                "button",
                {
                  key: "restore",
                  type: "button",
                  className: "xlink-oauth-link",
                  onClick: () => write(view.recommended),
                  disabled: busy,
                },
                L.restoreDefaults,
              )
            : null,
        ),
        e("div", { className: "xlink-oauth-status" }, L.modelsHint),
        error ? e("div", { className: "xlink-oauth-error" }, error) : null,
        view === null && !error ? e("div", { className: "xlink-oauth-status" }, L.modelsLoading) : null,
        view?.note ? e("div", { className: "xlink-oauth-status" }, view.note) : null,
        models.length > 0
          ? e(
              "div",
              { className: "xlink-oauth-list" },
              models.map((model) =>
                e(
                  "label",
                  { key: model.id, className: "xlink-oauth-item" },
                  e("input", {
                    type: "checkbox",
                    checked: model.selected,
                    // 至少留一条：取消勾选最后一条会让提供方整组从选择器里消失。
                    disabled: busy || (model.selected && selectedCount <= 1),
                    onChange: (event) => toggle(model, event.target.checked),
                  }),
                  e("span", { className: "xlink-oauth-item-name" }, model.name),
                  e("span", { className: "xlink-oauth-item-id" }, model.id),
                  model.capability === "unknown"
                    ? e("span", { className: "xlink-oauth-item-note" }, L.capabilityUnknown)
                    : null,
                ),
              ),
            )
          : null,
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
