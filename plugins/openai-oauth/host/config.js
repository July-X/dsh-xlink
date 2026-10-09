/**
 * 插件配置 schema——设置命名空间的实体。
 *
 * 设置镜像的命名空间视图由各 profile entry 的 Config schema 派生，但有
 * 两道闸（`dsh-settings`）：
 * ① `schema(entry)`：entry 必须导出 Config——没有它，`llm.
 *    registerConfigurableProviders` 声明的行在「设置 → 模型」永远
 *    not-configured（不出卡、进不了添加列表）；
 * ② `volatileForm(schema)`：object schema 逐字段只保留 volatile 字段，
 *    **空 dict 返回 undefined → 该命名空间照样不存在**。整个 schema
 *    标 `.volatile()` 则整体作为 form（meta.volatile 短路）。
 * 所以空对象也必须 `.volatile()`——本插件无需用户配置字段，但行卡要
 * 渲染、provider-card 插槽要派发，命名空间必须存在（2026-10-09 用户
 * 实测：只补 z.object({}) 时模型选择器能看到 provider，设置页仍无行）。
 *
 * 依赖经物化目录的 peer 链接解析（`@deepseek-ai/schemastery`，见
 * `materialize.rs` 的 PEER_PACKAGES），与 `@deepseek-ai/dsh-llm` 同一机制。
 */
import z from "@deepseek-ai/schemastery";

export const Config = z.object({}).volatile();
