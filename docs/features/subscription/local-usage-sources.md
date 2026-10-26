# 本机用量来源契约

本文描述本机 Codex 登录和外部 agent 用量的读取许可、统计与缓存规则，承接根 `AGENTS.md` 的跨前后端约束。云端套餐接口见[套餐用量设计](subscription-usage-design.md)，调查过程与测量数据见[复盘 §22–23、§33、§35–36](../../reviews/agent-lessons.md)。

## Codex 额度与登录许可

`usage/subscription_codex.rs` 只在用户显式开启后读取本机 Codex 登录文件；账号不一致、过期或身份无法确认时拒绝。不得刷新、修改或复制凭据。许可按壳模式隔离；前端共用 `CodexUsageConsent.vue`，通过 `codex-usage-consent-changed` 同步。切换来源先清除旧额度，显示名称为「Codex 额度」，不能称为 SIWC 应用用量。

新增读取本机 Codex 登录的消费方统一使用 `subscription_codex::chatgpt_web_available()`，同时要求 `enabled()` 与 `active_usage()`，不能各自拼许可条件。官网 ChatGPT 条件页签表 `OFFICIAL_CHAT_OPTIONAL_TABS` 在开窗时求值并快照到 `AppState::official_chat_tabs`：下标用于内容 webview label，不能随令牌轮换改变。

本机 Codex 登录不作为网页会话凭据；官网页签仍通过壳内网页登录，见[官网对话指南](../official-chat/official-chat-agent-guide.md#34-认证弹窗与浏览器档案)。

## 外部工具来源与展示

Codex CLI / Codex Desktop、MiniMax Desktop 默认均不勾选；未勾选就不扫描对应目录。所有外部数据只读，`set_usage_external_sources` 只写本壳偏好。勾选与缓存按 dev / release 壳模式隔离，不按实例分家，因为外部工具数据属于本机用户。

后端持久化偏好是唯一真值，前端每次拉取后以返回值为准。外部来源和内核用量独立展示，不能混合序列：

| 来源 | 统计口径 |
| --- | --- |
| Codex | `cached_input_tokens` 是 `input_tokens` 的子集，总量取来源自报 `total_tokens` |
| MiniMax | `input` 不含 `cache_read`，总量为四个分量相加 |

外部卡片的汇总数字与图表共同跟随窗口时间范围，用同一套 `sliceDays` + `summarizeDays`，不能将 90 天总数配在 7 天图表旁。

## 文件收集与失败语义

Codex 会话收集同时支持 `.jsonl` 和 `.jsonl.zst`；同一逻辑会话两种形态并存时只读普通文件。不能只扫压缩文件，否则会漏掉近期会话。

读失败的文件标为 `stale` / 没读全，下轮必重扫；不能写成空账后缓存为已完成。整源读取不完整时也不能将缓存标为新鲜。

## 新鲜度与增量

开窗先用缓存，用户显式刷新或切换勾选才强制扫描。前端 `refreshUsage(external, externalForce)` 的第二个参数与 Rust `external_force` 对应。

`external::FRESH_SCAN_INTERVAL_MS` 为 90 秒。只有窗口内、同一本地日历日、上一轮读全三个条件同时满足，才直接从缓存账目建视图，不接触来源磁盘；`external_force` 无视该窗口。

- Codex 按文件 `size` + `mtime` 复用；没读全的文件下轮重扫。
- MiniMax 按 `ts` 高水位只取新增行，同时核对日志总行数与新增行数；不符合只增不改的身份条件（换库、重写等）时整表重扫，不能只依赖时间戳。

## MiniMax 模型名回补

显式 `model` 优先；缺失时从同库 `local_runtime_message_rows.data_json` 的 `$.context_usage_telemetry.model` 按 `(session_id, turn_id)` 回补；两边都没有才标「未标注」，不猜测。

只查缺名字的轮次并走索引，不全表提取 JSON，不按消息时间裁剪。用量行可能早于消息落库，时间窗口无法替代轮次关联。历史空值集中在哪段时间、回补可信度和性能测量保留在[复盘 §35–36](../../reviews/agent-lessons.md)，不作为固定性能承诺。

## 前缀缓存诊断

使用 `npm run analyze:prompt-cache`（`scripts/analyze-prompt-cache.mjs`）核对命中率、键稳定性和前缀变化。内核 assistant 消息的 `replayState.response` 保存上游原始响应，是该路线取证来源，只读访问。

该套餐路线没有 `prompt_cache_diagnostics`；分字段缓存量来自 `usage.attribution.request_fields`。整体前缀命中时，`instructions` / `tools` 的 `cached_tokens` 等于各自 token 数。参考实现的缓存亲和契约同时涉及请求体 `prompt_cache_key` 和同值 `session-id` 请求头，复核时须检查完整请求。案例和源码依据见[复盘 §22–23](../../reviews/agent-lessons.md)。
