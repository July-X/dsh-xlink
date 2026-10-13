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
 * 这里让用户勾出常用的那几条。**勾选只改界面上的草稿，点「保存」才落盘**
 * （2026-10-13 用户要求：不要每勾一下写一次）——壳把集合落进
 * `shell/<mode>/openai-oauth/model-selection.json`，桥接的 `/v1/models`
 * 载荷带 `selected`，Host 适配器只过滤 `listModels`（`resolveModel` 仍走
 * 完整目录，见 host/adapter.js）。保存后不用刷新页面：Host 那边盯着这只
 * 文件，一变就重播一次适配器注册，内核随即让已打开的页面重拉模型目录
 * （host/index.js 的 watchSelectionFile）。
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
    modelsHint: "未勾选的模型不再出现在会话的模型选择器里，正在用它的会话需要另选一个。",
    modelsSummary: (names) => `已选 ${names.length} 项：${names.join("、")}`,
    modelsExpand: "选择模型",
    modelsCollapse: "收起",
    modelsLoading: "读取模型中…",
    modelsFailed: "读取常用模型失败",
    modelSaveFailed: "保存常用模型失败",
    modelsSave: "保存",
    modelsSaving: "保存中…",
    modelsDirty: "未保存的更改",
    modelsDiscard: "放弃改动",
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
      "Unchecked models no longer appear in the session model picker; a session using one must pick another.",
    modelsSummary: (names) => `${names.length} selected: ${names.join(", ")}`,
    modelsExpand: "Choose models",
    modelsCollapse: "Collapse",
    modelsLoading: "Loading models…",
    modelsFailed: "Failed to load the model list",
    modelSaveFailed: "Failed to save the model list",
    modelsSave: "Save",
    modelsSaving: "Saving…",
    modelsDirty: "Unsaved changes",
    modelsDiscard: "Discard",
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
.xlink-oauth-models-titlebar{display:flex;align-items:baseline;gap:8px;min-width:0}
.xlink-oauth-models-actions{display:flex;align-items:center;gap:10px;flex:none}
.xlink-oauth-models-title{font-weight:600;font-size:13px;flex:none}
.xlink-oauth-models-summary{font-size:12px;color:var(--dsw-alias-label-secondary);min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.xlink-oauth-dirty{color:var(--dsw-alias-label-secondary);font-size:12px;flex:none}
.xlink-oauth-save{height:28px;padding:0 12px;font-size:12px}
.xlink-oauth-link{border:0;background:0 0;color:var(--dsw-alias-label-secondary);font:inherit;font-size:12px;cursor:pointer;padding:0}
.xlink-oauth-link:hover:not(:disabled){color:var(--dsw-alias-label-primary)}
.xlink-oauth-link:disabled{opacity:.45;cursor:default}
.xlink-oauth-list{display:grid;gap:1px;max-height:168px;overflow-y:auto;border:.5px solid var(--dsw-alias-border-l3);border-radius:var(--dsw-radius-md);padding:6px 8px}
.xlink-oauth-item{display:flex;align-items:center;gap:8px;font-size:13px;cursor:pointer;padding:1px 0}
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
     * 条目加勾选状态。勾选框只改**界面上的草稿**，点「保存」才把整份选中集合
     * 交回 `..._set`（而不是增量），壳按目录过滤后落盘。
     *
     * 为什么不是每勾一下写一次（2026-10-13 用户要求）：
     * · 勾选框是"挑选"动作，挑选过程中每一下都落盘会在盘上留下一串中间状态，
     *   用户改主意时那几次写都已经发生了（Host 还会为每次写重播一次适配器
     *   注册，让所有打开的页面重拉模型目录）；
     * · 传整份而不是增量：增量接口一旦漏掉一条，界面上看不出任何异常，而会话
     *   里会多出或少掉一个模型。
     *
     * 草稿与已保存集合不一致时显示「未保存」，保存按钮才可点；`恢复默认` 也只
     * 改草稿（同样要点保存），所以两条路径的语义是同一条。刷新（`nonce` 变化）
     * 会用服务端状态重置草稿——那是用户主动的"重新读取"，勾选会跟着回到盘上
     * 那份（未保存的改动被丢弃，界面上看得见）。
     *
     * 默认**收起成一行**（2026-10-13 用户反馈「占用过大」）：标题 + 已选摘要 +
     * 「选择模型」按钮。账号目录有几十条，常驻一份两三百像素高的勾选列表会把
     * 设置页压掉半屏，而这张卡片平时只需要回答"现在哪些模型会出现在会话里"。
     * 点「选择模型」才展开列表；保存成功后自动收起（一次编辑结束就把地方还回去）。
     * 收起时**不隐藏任何状态**：未保存的改动、读取失败、目录空态照常显示在那一行
     * 下面，只有"可操作的列表"被折起来，且未保存时不允许收起（切换按钮在
     * 「展开 + 有改动」时禁用）——收起来就看不见勾选，等于让用户以为改丢了；
     * 想放弃没有保存的改动就用那一排里的「放弃改动」（没有它这条禁用规则就成
     * 了一条没有出口的规则）。
     *
     * 未登录时不画这一区：那种情况下按不出任何模型，而卡片上方已经在说
     * 「未登录」了，多一行空列表只是噪声。
     */
    function ModelSection({ phase, nonce }) {
      const [view, setView] = React.useState(null);
      // 草稿：正在勾的集合（目录顺序）。null = 还没读到目录。
      const [draft, setDraft] = React.useState(null);
      // 展开态：默认收起（见上方注释）。
      const [expanded, setExpanded] = React.useState(false);
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
            setDraft(idsOf(next));
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
      const saved = idsOf(view);
      const pending = draft ?? saved;
      const dirty = !sameIds(pending, saved);
      const pendingNames = models.filter((model) => pending.includes(model.id)).map((model) => model.name);
      const save = async () => {
        if (draft === null) return;
        setBusy(true);
        setError("");
        try {
          const next = await invokeOrNull()("openai_models_selection_set", { ids: pending });
          setView(next);
          setDraft(idsOf(next));
          // 保存成功就收起：这一次编辑结束了，把版面还回去（摘要行里能看到结果）。
          setExpanded(false);
        } catch (writeError) {
          setError(`${L.modelSaveFailed}：${String(writeError)}`);
        } finally {
          setBusy(false);
        }
      };
      const toggle = (model, checked) => {
        if (busy || view === null) return;
        const ids = models
          .filter((entry) => (entry.id === model.id ? checked : pending.includes(entry.id)))
          .map((entry) => entry.id);
        // 空集会让这个提供方从会话选择器里整组消失：最后一枚在 UI 上是禁用的，
        // 这里再挡一次（钩子里的栈多半来自键盘 / 程序化触发）。
        if (ids.length === 0) return;
        setDraft(ids);
      };
      // 「恢复默认」只改草稿：与勾选框同一条路径（不点保存就不落盘）。
      const restoreDefaults = () => {
        if (busy) return;
        setDraft([...(view?.recommended ?? [])]);
      };
      // 撤销未保存的改动（回到盘上那份）。没有它，「未保存时不许收起」就成了
      // 一条没有出口的规则：改错了只能一条条勾回去，或者把错的存下去。
      const discard = () => {
        if (busy) return;
        setDraft(idsOf(view));
      };

      // 收起时仍然要说的那几件事（加载中 / 读失败 / 目录空态）。
      const statusLine = error
        ? e("div", { className: "xlink-oauth-error" }, error)
        : view === null
          ? e("div", { className: "xlink-oauth-status" }, L.modelsLoading)
          : view.note
            ? e("div", { className: "xlink-oauth-status" }, view.note)
            : null;

      return e(
        "div",
        { className: "xlink-oauth-models", "data-xlink-openai-oauth": "models" },
        e(
          "div",
          { className: "xlink-oauth-models-head" },
          e(
            "div",
            { className: "xlink-oauth-models-titlebar" },
            e("span", { className: "xlink-oauth-models-title" }, L.modelsTitle),
            models.length > 0
              ? e(
                  "span",
                  { className: "xlink-oauth-models-summary", title: L.modelsSummary(pendingNames) },
                  L.modelsSummary(pendingNames),
                )
              : null,
          ),
          e(
            "div",
            { className: "xlink-oauth-models-actions" },
            dirty ? e("span", { className: "xlink-oauth-dirty" }, L.modelsDirty) : null,
            dirty && expanded
              ? e(
                  "button",
                  {
                    key: "discard",
                    type: "button",
                    className: "xlink-oauth-link xlink-oauth-discard",
                    onClick: discard,
                    disabled: busy,
                  },
                  L.modelsDiscard,
                )
              : null,
            expanded && (view?.recommended?.length ?? 0) > 0
              ? e(
                  "button",
                  {
                    key: "restore",
                    type: "button",
                    className: "xlink-oauth-link xlink-oauth-restore",
                    onClick: restoreDefaults,
                    disabled: busy,
                  },
                  L.restoreDefaults,
                )
              : null,
            e(
              "button",
              {
                key: "toggle",
                type: "button",
                className: "xlink-oauth-link xlink-oauth-toggle",
                onClick: () => setExpanded((value) => !value),
                // 未保存时不许收起：列表一折起来就看不出改了什么，而草稿还在。
                disabled: busy || (expanded && dirty) || models.length === 0,
              },
              expanded ? L.modelsCollapse : L.modelsExpand,
            ),
            // 保存只在"能保存"或"正在编辑"时出现：收起且无改动时它永远灰着，
            // 留在那一行里只是噪声。
            dirty || expanded
              ? e(
                  "button",
                  {
                    key: "save",
                    type: "button",
                    className: "xlink-oauth-btn xlink-oauth-save",
                    onClick: save,
                    disabled: busy || !dirty,
                  },
                  busy ? L.modelsSaving : L.modelsSave,
                )
              : null,
          ),
        ),
        statusLine,
        expanded ? e("div", { className: "xlink-oauth-status" }, L.modelsHint) : null,
        expanded && models.length > 0
          ? e(
              "div",
              { className: "xlink-oauth-list" },
              models.map((model) =>
                e(
                  "label",
                  { key: model.id, className: "xlink-oauth-item" },
                  e("input", {
                    type: "checkbox",
                    // 勾的是**草稿**，不是服务端状态：点保存才落盘。
                    checked: pending.includes(model.id),
                    // 至少留一条：取消勾选最后一条会让提供方整组从选择器里消失。
                    disabled: busy || (pending.includes(model.id) && pending.length <= 1),
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

    /** 服务端视图里的选中 id（目录顺序；视图缺失时当空集）。 */
    function idsOf(view) {
      return (view?.models ?? []).filter((model) => model.selected).map((model) => model.id);
    }

    /** 两份 id 列表是否逐位相同（两边都按目录顺序生成，比集合弱、比排序便宜）。 */
    function sameIds(left, right) {
      return left.length === right.length && left.every((id, index) => id === right[index]);
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
