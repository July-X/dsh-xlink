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
 */
const STRINGS = {
  zh: {
    cardTitle: "OpenAI · ChatGPT 套餐",
    cardConfigured: "已在工作台启用，登录与用量管理随后续版本提供。",
    cardNotConfigured: "尚未配置：等待桌面壳的登录服务（P2 起）。",
    keyConfigured: "凭据已就绪",
  },
  en: {
    cardTitle: "OpenAI · ChatGPT plan",
    cardConfigured: "Enabled in the workbench; sign-in and usage management arrive in a later release.",
    cardNotConfigured: "Not configured yet: waiting for the desktop shell sign-in service (from P2).",
    keyConfigured: "Credential ready",
  },
};
const L = STRINGS[(globalThis.navigator?.language ?? "zh").toLowerCase().startsWith("zh") ? "zh" : "en"];

window.__ModuleLoader__.load({
  id: "xlink-openai-oauth",
  factory: (require) => {
    const React = require("react");
    const e = React.createElement;

    function ProviderCard(props) {
      const { provider, configured, keyConfigured } = props ?? {};
      return e("div", { "data-xlink-openai-oauth": "card", style: { padding: "8px 0" } },
        e("div", { style: { fontWeight: 600 } }, L.cardTitle),
        e("div", { style: { fontSize: 12, opacity: 0.75 } },
          configured ? L.cardConfigured : L.cardNotConfigured),
        keyConfigured ? e("div", { style: { fontSize: 12 } }, L.keyConfigured) : null,
        provider ? e("div", { style: { fontSize: 11, opacity: 0.55 } }, provider.displayName) : null,
      );
    }

    function apply(ctx) {
      ctx.slots.inject("settings.models.provider-card", () => ctx.slots.register({
        name: "settings.models.provider-card",
        key: "xlink-openai-oauth",
      }, ProviderCard));
    }

    return { apply, inject: ["slots"] };
  },
});
