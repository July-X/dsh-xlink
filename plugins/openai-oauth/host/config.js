/**
 * 插件配置 schema——设置命名空间的实体。
 *
 * 设置镜像的命名空间视图由各 profile entry 的 Config schema 派生
 * （`dsh-settings`：「Derive editable forms from plugin Config schemas」；
 * `forms()` 对 `schema === undefined` 的 entry 直接跳过）。host 不导出
 * Config 时，`llm.registerConfigurableProviders` 声明的 provider 行在
 * 「设置 → 模型」永远 not-configured：不出卡、进不了添加列表，client
 * 的账户卡无处渲染（2026-10-09 用户实测）。本插件无需用户配置字段，
 * 空对象即最小合法 schema——行卡渲染时 provider-card 插槽照样按
 * settingsNs 派发，账户卡挂在卡片的扩展区。
 *
 * 依赖经物化目录的 peer 链接解析（`@deepseek-ai/schemastery`，见
 * `materialize.rs` 的 PEER_PACKAGES），与 `@deepseek-ai/dsh-llm` 同一机制。
 */
import z from "@deepseek-ai/schemastery";

export const Config = z.object({});
