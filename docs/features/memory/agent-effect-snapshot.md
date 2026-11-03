# 记忆效果快照：给 agent 的可复核读数

日期：2026-11-01（当日两轮：先落快照，再补「主动取数 + 历史归档 + turn/step 关联」）。对象：本机记忆模块的「效果」读数。配套：[记忆效果可视化设计](effectiveness-dashboard-design.md)（口径与指标来源）、[开发清单](memory-progress.md)。

本文写给**后续要分析、修复、增强记忆模块的 agent 与维护者**：效果数据在哪里、哪份是契约、怎么读、哪些结论不能下。

## 1. 为什么要这份快照

面板上的效果报告（`EffectReport`）每次现算，原先只在 Tauri IPC 里活一次；本机 agent 没有面板通道，只能硬读 `state.json`（派生缓存，schema 会变）与注入账本，而最值得分析的 L2 配对、L3 实验、提取质量、健康度全在 Rust 里派生。让 agent 重写一遍 `scan.rs` 的口径，两份实现必然漂。

所以现在有三层读面：**落盘快照**（`report.json` / `report.md`）、**历史归档**（`history/`，可比较改动前后）、**会话内只读工具**（`memory_effect`，随时取最新摘要）。三层都只排版同一份 `EffectReport`，不复制聚合逻辑。

## 2. 产物与刷新时机

实例根定位：会话环境里 `DSH_HOME` 指向 `<实例>/home`，效果目录是 `$DSH_HOME/../memory/$DSH_PROFILE/effect/`。bash 直接读即可（读操作不受内核文件沙箱限制）。

没有 `DSH_HOME` 时（例如在仓库里直接跑的 Codex / 脚本），按 `~/.dsh-xlink/kernels/dsh/instances/<实例>/memory/<profile>/effect/` 定位：release 实例是 `default`、dev 是 `default-dev`，profile 当前恒为 `web`。下面的示例按 `DSH_HOME` 写，外部 agent 换成绝对路径即可。

| 文件 | 是什么 | 刷新时机 | 权限 |
| --- | --- | --- | --- |
| `effect/report.json` | 完整 `EffectReport` + 信封（schema / kind / generated_at / range_days / refresh） | 每次生成报告时重写（面板命令或只读桥） | 0600 |
| `effect/report.md` | 几 KB 摘要：可信度、核心读数、未注入原因、L2/L3、提取质量、榜单、口径纪律、自动线索 | 同上 | 0600 |
| `effect/history/report-*.json` | 与 `report.json` 同一份信封的历史归档，按生成时间命名（同一秒多份用 `~2` / `~3` 后缀，字典序即写入序） | 同上，每次归档一份，滚动保留最近 60 份 | 0600 |
| `effect/ledger/notes-YYYY-MM.jsonl` | 插件注入 / 跳过事件的权威流水（JSONL，按 UTC 月分文件） | 每次真实注入 / 跳过时追加 | 0600 |
| `effect/state.json` | 扫描内核会话日志后的派生缓存（日 × 记忆聚合） | 每次生成报告时增量重写 | 跟随 umask（实测 0644） |
| `effect/experiment.json` | L3 开关（开始 / 停止 / 比例 / 种子） | 用户在面板开始 / 停止实验时 | 0600 |

**刷新边界**：快照只在报告真正生成时重写，生成入口有三个——面板「记忆 → 效果」页首次拉某个时间窗 / 切 7·30·90 / 点「刷新」/ 开始·停止实验；插件经只读桥 `GET /v1/memory/effect` 取数；会话里调用 `memory_effect` 工具（内部走同一条桥）。**同一时间窗再次进入效果页会吃前端缓存**（`ui/src/memory/effect.js` 的 `canReuseReport`），不触发命令；切页签也不触发。**没有后台定时器**：冷扫要解几百 MB 会话日志，不该塞进启动路径。需要新数据就调工具或点一次「刷新」；注入原因本身以 `ledger/` 为准，它一直是实时的。

面板与只读桥共用**进程内一把单飞锁**：两边同时取数会串行，不会并发扫同一批日志，也不会把历史归档写重。

`state.json` 是内部缓存，**不是契约**：schema 会随扫描逻辑升级，字段也不保证面向外部。分析时优先读 `report.json` / `report.md` / `history/`，需要逐日、逐记忆细账再读 `state.json`。

## 3. 会话里主动取数：memory_effect 工具

模型在会话里可以调 `memory_effect`（第 5 个记忆工具，插件 v0.2.1 起）：

- 参数：`range_days`，取值 7 / 30 / 90，默认 7。
- 返回：`range_days` / `generated_at` / `history_files` / `latest_history` / `digest`；`digest` 与 `report.md` 同一套段落，但**榜单标题换成记忆 id**——它会进入模型上下文，而实例级榜单可能含别的项目的标题；标题只写在本机 `report.md`。
- 行为：每次调用都会现算并重写快照与历史（与面板同一口径），**不是读旧文件**；桥超时设 20 秒（冷扫可能秒级）。
- 限制：对照实验的 control 会话拒绝调用（避免把记忆信息带进对照组）；桥不可用时抛「读取记忆效果失败」，不会编一份空摘要。

壳侧对应路由：`GET /v1/memory/effect?range_days=7`（只读回环桥，只有拿到 Bearer 的插件能调），返回同样的五个字段（`digest` 同样不含标题），失败回 500 `{"error": "..."}`。路由只写 `effect/` 下的派生快照，**不碰权威记忆库**；命名空间取自桥绑定，不是默认实例。

## 4. report.json 的契约

外层六个键固定（schema 1）：

```json
{
  "schema": 1,
  "kind": "xlink.memory.effect-report",
  "generated_at": "2026-11-01T03:22:11.123Z",
  "range_days": 7,
  "refresh": "……",
  "report": { "range_days": 7, "window": {}, "coverage": {}, "kpi": {}, "…": "…" }
}
```

`report` 就是面板拿到的那一份，段落与含义：

| 段落 | 回答什么 | 关键纪律 |
| --- | --- | --- |
| `window` / `coverage` / `health` | 数据本身可不可信 | 先看这组；解码失败、坏行、未知事件占比高时，后面的比率都要打折 |
| `kpi` / `funnel` / `trend` | 记忆到底被用了多少 | 比率是 0–100 百分数；请求 / 注入样本不足 30 时只给计数 |
| `ledger` | 没注入的原因（开关 / 零命中 / **同回合已查过** / **预算用尽** / 桥失败 / 内容未变；旧的 `budget` 行保留成「旧桶」，不并进新桶） | 权威事件流；`available=false` 表示未启用。`link.linked` 是总关联数，`link.step_linked` 是其中按 `turn+step` 精确命中的条数（旧账目没带 step 时为 0，`rate` 分母不变） |
| `comparison` | 同会话暴露 vs 未暴露请求的差（L2） | 只能写「伴随」；配对会话少于 10 个只给计数与所需样本 |
| `experiment` | 会话级随机对照读数（L3） | 配置过且每组样本够才给指标（停止 / 到期后窗口内历史分组仍会出读数）；别混用 L2 的因果语言 |
| `extraction` | 自动提取来源的记忆质量 | 来自 `memory.json` / `jobs.json` 既有事实；2026-11-03 起 `failures[]`（按 stage×kind 聚合 failed 任务）与 `store.jobs_pending_oldest_age_ms`（最老待跑任务的年龄）是**新埋点**（任务表多了 `stage` / `error_kind` 两列） |
| `leaderboard` | 哪些记忆在干活、哪些在占位 | 标题按 id 从 `memory.json` 现查；已删除的只给状态文案 |
| `cost` / `store` | 成本与库存 | 提取 token 只有上界估算，不是精确账 |

三个口径**不能相加**：扫描的「注入快照次数」、账本的「inject 事件」、扫描的「携带记忆请求」是三种单位。面板也从不相加，分析时同样别加。

## 5. report.md 怎么读

固定九节，建议按这个顺序：

1. **数据可信度**：会话覆盖、解析健康、样本门槛、账本是否可用与 step 精确命中数。数字不对劲先停在这里。
2. **核心读数**：L1 事实。
3. **未注入原因**：`skip` 的原因分解、`inject` 触发路径与版本表——回答「为什么这回合没有记忆」。
4. **L2**：伴随差异 + 置信区间；样本不足会直接写出来。
5. **L3**：实验状态与两组读数。
6. **提取质量**：自动提取的记忆有没有被用、提取器版本表现、**失败卡在哪一步（阶段×类别）与队列健康**（待跑数 / 最老待跑任务的年龄）。
7. **记忆榜单**：按注入快照排序的前 10 条。
8. **读法**：口径纪律（每次生成都带，防止 agent 拿到数字就过度解读）。
9. **可复查的线索**：按当前数据条件生成的提示（不是结论），直接对应可能的下一步排查。

## 6. 一次分析的推荐路径

```bash
E="${DSH_HOME:?}/../memory/${DSH_PROFILE:?}/effect"

# ① 先排除数据问题
jq '.report.coverage, .report.health' "$E/report.json"

# ② 记忆被用了多少：注意样本门槛
jq '.report.kpi' "$E/report.json"

# ③ 没注入的原因分布与版本差异；关联质量看 linked / step_linked / rate
jq '[.report.ledger.reasons[] | select(.kind=="skip")], .report.ledger.versions' "$E/report.json"
jq '.report.ledger.link' "$E/report.json"

# ④ 现在能不能谈差异：L2 / L3 的样本够不够
jq '.report.comparison.sessions_with_both, .report.comparison.required_sessions' "$E/report.json"
jq '.report.experiment' "$E/report.json"

# ⑤ 哪条记忆在干活
jq '.report.leaderboard[:5]' "$E/report.json"

# ⑥ 直接读摘要（本机分析用；要放进模型上下文就用 memory_effect 工具，它不给标题）
sed -n '1,120p' "$E/report.md"

# ⑦ 比较改动前后：挑改动前后的两份历史快照（同名窗口）
ls -1t "$E/history" | head
diff <(jq -S '.report' "$E/history/<改前>.json") <(jq -S '.report' "$E/report.json")
```

会话里也可以直接让模型调 `memory_effect(range_days=7|30|90)`，拿到的是同一份摘要的最新版本；比读文件少了「快照可能是旧的」这一步。

拿到线索后按这个顺序动手：先修数据（解析 / 账本可用性），再修检索与预算（`query.rs` / `policy.js`），最后才讨论「记忆有没有用」。改完回看同一窗口的 L1/L2；只有实验开启时才允许谈 L3 因果。

## 7. 已知缺口（别假装能算）

- **逐请求结果不落盘**：账本与请求的关联已能按 `turn+step` 精确匹配（插件 v0.2.1 起上报真实 step，报告在 `link.step_linked` 里给条数），但「每一次注入对应哪次请求、结果如何」的逐请求行仍然没有落盘；单请求因果别算，用 L2 / L3 的聚合差。
- **step 关联依赖插件版本**：旧账目（step=-1）只能按 turn / 日期近似；压缩后 turn 重复时仍可能串到别的周期，看 `link.rate` 与 `step_linked` 判断可信度。
- **没有后台定时器**：快照在报告生成时更新；要新数据就调 `memory_effect` 或点面板「刷新」。
- **历史归档只有 JSON**：`history/` 里不存 md；同一秒多份用 `~2` / `~3` 后缀，`~` 的字典序在 `.` 之后，所以按文件名排序的最后一份就是最新（真实使用每轮只会写一份）。历史目录可整目录删，下次生成会重建。
- **两份文件不是事务**：`report.json` / `report.md` 各自原子替换，第二次写失败或进程在中途被杀时可能一新一旧；两边的头都带 `generated_at`，对不上就刷新一次，以 `report.json` 为准。
- **`state.json` 权限是 0644**（设计稿要求 0600）：它是内部缓存，不含标题正文，但分析时别把它当契约。
- **提取 token 只有上界估算**：`jobs.json` 不记 usage。
- **设计稿与实现有若干漂移**：`memory_effect_notes` 命令、报告的 `force` / 90s TTL 都只存在于设计稿。以本文件、代码与 `memory-progress.md` 为准。

## 8. 隐私边界

- 快照 / 历史归档与 `memory.json` 同目录、0600；不含记忆正文、关键词、来源字段。排行榜条目会带记忆 id、标题、scope、verification——都是面板本来就展示的字段，标题来自 `memory.json`，只落在本机这些派生文件里。
- `memory_effect` 工具会把摘要带进模型上下文，因此**它不含记忆标题**：榜单只给记忆 id 与计数；实例级聚合可能跨项目，标题不随工具出本机。工具与 `memory_search` / `memory_read` 是同一条隐私边界——选远端模型时这些内容一样出网。
- 本机 `report.json` / `report.md` / `history/` 有标题（面板本来就展示），只在该实例目录、0600。分析可以引用计数、原因、样本量与标题（本机语境）；不要把手读到的文件内容或会话日志原文手工粘进远端模型的上下文。

## 9. 判据与验证

- `cd src-tauri && cargo test --locked memory::effect_export`：信封与 schema、空窗口「样本不足」、`skip` / `inject` 分列、标题保留而正文绝不落盘、**模型版摘要不含标题**、写失败的可操作文案、0600，以及历史归档（同信封 / schema、同秒 `~N` 后缀、摘要份数与盘上一致、滚动保留上限）与 scoped 临时 home 下走生产入口落盘的用例。判据名见 `memory-progress.md` 的 2026-11-01 条目。
- `cargo test --locked memory::effect`：扫描侧 `snapshot_steps` 回填与报告侧 `turn+step` 优先 / turn 回退的关联用例。
- 桥路由：`memory_routes_work_over_the_bridge` 端到端走生产分发臂（200 / range / generated_at / history_files / digest 头 / 调用留痕），并用**非默认实例 id** 的桥绑定断言快照落在该实例目录——`Namespace::current()` 被误用时这条才红。
- 插件：`npm run test:plugin`（工具清单五件、效果工具调桥与失败路径、control 拒绝、note 上报真实 step）。
- 面板路径：现有 e2e 用例会真实调用 `memory_effect_report`，但没有断言文件存在；路径解析与落盘由生产入口单测覆盖，接线靠唯一调用点与代码审查。别把这条读成端到端验过文件。
