# 内嵌 OpenAI OAuth 插件 P0 发布包调查

日期：2026-10-08。状态：静态接口调查完成，离线接线原型未做（见 §4）。输入：[设计文档](openai-oauth-design.md)、[开发计划](openai-oauth-development-plan.md) §9 P0。

## 1. 调查对象与方法

- 窗口：`0.2.1-alpha.1` / `0.2.0-rc.2` / `0.2.0-rc.1`，与[开发计划 §1.1](openai-oauth-development-plan.md) 快照一致。
- `0.2.1-alpha.1` 取自本机安装树（只读检查）；rc.1/rc.2 从 registry.npmjs.org 拉官方 tarball 解包比对。
- `@deepseek-ai/dsh` 元包是薄元包（23–24 KB），实际代码在子包。按接口面选六个关键子包逐版本 diff：`dsh-llm`、`dsh-client-ui-settings-models`、`dsh-client-ui-slots`、`dsh-plugin-manager`、`dsh-client-modules`、`dsh-app-boot`。
- 静态接口调查不等于兼容验收；逐版本运行验收按开发计划 §10 在 P6 执行。

## 2. 逐版本接口表

| 接口面 | 证据文件 | rc.1 | rc.2 | alpha.1 | 结论 |
| --- | --- | --- | --- | --- | --- |
| 适配器注册 `ctx.llm.registerAdapter(providers, adapter)`；`LlmAdapter`（`providerInfo` / `listModels` / `resolveModel` / 抽象 `stream`）；注册句柄含 `replace(providers)` | `dsh-llm/lib/types/index.d.ts` | ✅ | ✅ | ✅ | 三版本逐字节一致 |
| 强度类型 `LlmModelReasoningInfo{efforts[], defaultEffort?}`、`LlmReasoningEffortInfo{id,name}`、`GenerateOptions.reasoningEffort` | `dsh-llm/lib/types/types.d.ts` | ✅ | ✅ | ✅ | 逐字节一致；开发计划 §7 的投影有真实落点 |
| 回放 `ReplayEnvelope{response, per-block}`：finish 块携带、存于 assistant 消息 model source，两半对内核不透明 | 同上 | ✅ | ✅ | ✅ | 开发计划 §8.2 的公开持久化接口在场 |
| 模型设置插槽：`settings.models.provider-card`（按 `settingsNs` 键控，owner 含 `provider/configured/keyConfigured`）、`settings.models.sign-in`、`settings.models.footer` | `dsh-client-ui-settings-models/lib/types/client/slot-contract.d.ts` | ✅ | ✅ | ✅ | 逐字节一致；账户卡片可挂自有命名空间 |
| 提供方目录 `ProviderDirectoryEntry{provider, displayName, settingsNs, settingsPath, active, declared?}`；`LlmConfigurableProvider` | settings-models `store.d.ts`、dsh-llm `types.d.ts` | ✅ | ✅ | ✅ | 提供方行进设置页的类型面在场 |
| Host 插件加载：profile `cordis.patch.yml` 为 YAML 序列；`insert:` 行锚定为 file URL 交 `ctx.loader`；`dsh.bundle` 声明 overlay patch | `dsh-plugin-manager/lib/index.js`、`dsh-app-boot/lib/index.js` | ✅ | ✅ | ✅（另新增 `dependencySpec` 处理 `file:`/`link:`/git 规格） | 免 pnpm 接线存在公开机制 |
| 客户端声明 `dsh.client{platform, inject[]}`；浏览器运行时经 `internal.import` 动态加载（client.js 为零 node 依赖契约面） | `dsh-client-modules/lib/index.js` + `client.js` | ✅ | ✅ | ✅ | 客户端插件运行时发现，不改 webui 构建 |
| Cordis 版本钉版 | 元包 `package.json` | `~4.0.4` | `~4.0.4` | `~4.0.5-alpha.1` | 宿主依赖必须按内核指纹解析（开发计划 §6.3） |

rc.1 与 rc.2 之间六个包共约 199 行差异，未触及上表接口；alpha.1 相对 rc 在 plugin-manager 的差异（约 365 行）主要新增非 registry 协议的依赖规格处理，`insert`/overlay 机制未变。

## 3. 对设计的关键结论

1. 适配器注册面、强度类型、回放信封、设置插槽在窗口内三版本逐字节一致，单一兼容层预计覆盖模型侧接口；物化目录仍按指纹区分，但首个 compat 层可共用。
2. 免 pnpm 接线路径成立：profile patch 的 `insert:` 行以本地路径加载插件目录（锚定为 file URL），不经 pnpm。插件目录内对 `@deepseek-ai/*` 宿主依赖的解析必须指向目标内核的同一份，这是 P1 原型第一优先验证项。
3. 客户端插件在浏览器运行时动态加载，`dsh.client` 声明三版本校验一致，账户卡片不需要改 webui 构建。
4. `LlmAdapter` 契约要求提供方 HTTP 请求带 `attributionHeaders()`；本路线的实际 HTTP 由 Rust 服务发出，归属头是否随桥接透传上游在实现时按 dsh-llm 归属模块细节落定。
5. 静态未定位的入口：`LlmConfigurableProvider` 的声明来源（settings schema 或配置节）在 `dsh-settings` / `dsh-client-ui-settings` 侧，P1 以 `--dump-config` 实测——判断内核认不认某个注入方式必须实测，不读接口名推断。

## 4. 缺口与实现路径（对照 P0 退出准则）

| 缺口 | 实现路径 | 归属 |
| --- | --- | --- |
| `insert:` 行 + 宿主依赖解析未实测 | P1 原型：临时实例 insert 指向插件目录，验证 `@deepseek-ai/*` 解析到内核树 | P1 第一优先 |
| `LlmConfigurableProvider` 声明入口未定位 | 读 `dsh-settings`/`dsh-client-ui-settings` + `--dump-config` 实测 | P1 |
| `dsh.client` 的 inject 目标运行时发现链未端到端验证 | P1 原型：最小 client 插件注册 `provider-card` 插槽 | P1 |
| Cordis `~4.0.4` 与 `~4.0.5-alpha.1` 的宿主 API 差异面 | 按指纹物化目录天然隔离；原型各跑一次 | 低 |

结论：窗口内未发现缺失必要能力的版本，所有缺口有明确实现路径——满足开发计划 §9 P0 退出准则的「所有缺口有明确实现路径」；「离线接线原型」移交 P1 作为第一项交付，P0 在原型完成前不宣告结束。

调查证据：本机安装树 `~/.dsh-xlink/dsh/desktop/kernels/0.2.1-alpha.1/node_modules/@deepseek-ai/`（只读）与 registry.npmjs.org 官方 tarball（解包于本地临时目录）。
