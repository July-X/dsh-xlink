# 记忆效果快照：给 agent 的可复核读数

日期：2026-11-01。对象：本机记忆模块的「效果」读数。配套：[记忆效果可视化设计](effectiveness-dashboard-design.md)（口径与指标来源）、[开发清单](memory-progress.md)。

本文写给**后续要分析、修复、增强记忆模块的 agent 与维护者**：效果数据在哪里、哪份是契约、怎么读、哪些结论不能下。

## 1. 为什么要有这份快照

面板上的效果报告（`EffectReport`）每次由 `effect::report()` 现算，只在 Tauri IPC 里活一次。管理面板之外没有出口：本机 agent 既没有面板通道，也拿不到那一次返回。能直接读的只有 `state.json`（派生缓存，且 schema 会变）和注入账本；而最值得分析的 L2 配对、L3 实验、提取质量、健康度全在 Rust 里派生——让 agent 重写一遍 `scan.rs` 的口径，两份实现必然漂。

所以：**同一次报告落两份文件**，agent 直接读，口径不复制。

## 2. 产物与刷新时机

实例根定位：会话环境里 `DSH_HOME` 指向 `<实例>/home`，效果目录是 `$DSH_HOME/../memory/$DSH_PROFILE/effect/`。bash 直接读即可（读操作不受内核文件沙箱限制）。

没有 `DSH_HOME` 时（例如在仓库里直接跑的 Codex / 脚本），按 `~/.dsh-xlink/kernels/dsh/instances/<实例>/memory/<profile>/effect/` 定位：release 实例是 `default`、dev 是 `default-dev`，profile 当前恒为 `web`。下面的示例按 `DSH_HOME` 写，外部 agent 换成绝对路径即可。

| 文件 | 是什么 | 刷新时机 | 权限 |
| --- | --- | --- | --- |
| `effect/report.json` | 完整 `EffectReport` + 信封（schema / kind / generated_at / range_days / refresh） | 每次 `memory_effect_report` 成功时重写 | 0600 |
| `effect/report.md` | 几 KB 摘要：可信度、核心读数、未注入原因、L2/L3、提取质量、榜单、口径纪律、自动线索 | 同上 | 0600 |
| `effect/ledger/notes-YYYY-MM.jsonl` | 插件注入 / 跳过事件的权威流水（JSONL，按 UTC 月分文件） | 每次真实注入 / 跳过时追加 | 0600 |
| `effect/state.json` | 扫描内核会话日志后的派生缓存（日 × 记忆聚合） | 每次生成报告时增量重写 | 跟随 umask（实测 0644） |
| `effect/experiment.json` | L3 开关（开始 / 停止 / 比例 / 种子） | 用户在面板开始 / 停止实验时 | 0600 |

**刷新边界**：快照只在 `memory_effect_report` 真正执行时重写——首次打开某个时间窗、切换 7 / 30 / 90、点工具栏「刷新」，或开始 / 停止实验后的强制刷新。**同一时间窗再次进入效果页会吃前端缓存**（`ui/src/memory/effect.js` 的 `canReuseReport`），不触发命令；切页签也不触发。**没有后台定时器**：冷扫要解几百 MB 会话日志，放进启动路径不划算。需要更近的时点就点一次「刷新」；注入原因本身以 `ledger/` 为准，它一直是实时的。

`state.json` 是内部缓存，**不是契约**：schema 会随扫描逻辑升级，字段也不保证面向外部。分析时优先读 `report.json` / `report.md`，需要逐日、逐记忆细账再读 `state.json`。

## 3. report.json 的契约

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
| `ledger` | 没注入的原因（开关 / 零命中 / 预算 / 桥失败 / 内容未变） | 账本是插件上报的权威事件流；不可用时会明确标 `available=false` |
| `comparison` | 同会话暴露 vs 未暴露请求的差（L2） | 只能写「伴随」；配对会话少于 10 个只给计数与所需样本 |
| `experiment` | 会话级随机对照读数（L3） | 配置过且每组样本够才给指标（停止 / 到期后窗口内历史分组仍会出读数）；别混用 L2 的因果语言 |
| `extraction` | 自动提取来源的记忆质量 | 来自 `memory.json` / `jobs.json` 既有事实，无新埋点 |
| `leaderboard` | 哪些记忆在干活、哪些在占位 | 标题按 id 从 `memory.json` 现查；已删除的只给状态文案 |
| `cost` / `store` | 成本与库存 | 提取 token 只有上界估算，不是精确账 |

三个口径**不能相加**：扫描的「注入快照次数」、账本的「inject 事件」、扫描的「携带记忆请求」是三种单位。面板也从不相加，分析时同样别加。

## 4. report.md 怎么读

固定九节，建议按这个顺序：

1. **数据可信度**：会话覆盖、解析健康、样本门槛、账本是否可用。数字不对劲先停在这里。
2. **核心读数**：L1 事实。
3. **未注入原因**：`skip` 的原因分解、`inject` 触发路径与版本表——回答「为什么这回合没有记忆」。
4. **L2**：伴随差异 + 置信区间；样本不足会直接写出来。
5. **L3**：实验状态与两组读数。
6. **提取质量**：自动提取的记忆有没有被用、提取器版本表现。
7. **记忆榜单**：按注入快照排序的前 10 条。
8. **读法**：口径纪律（每次生成都带，防止 agent 拿到数字就过度解读）。
9. **可复查的线索**：按当前数据条件生成的提示（不是结论），直接对应可能的下一步排查。

## 5. 一次分析的推荐路径

```bash
E="${DSH_HOME:?}/../memory/${DSH_PROFILE:?}/effect"

# ① 先排除数据问题
jq '.report.coverage, .report.health' "$E/report.json"

# ② 记忆被用了多少：注意样本门槛
jq '.report.kpi' "$E/report.json"

# ③ 没注入的原因分布与版本差异
jq '[.report.ledger.reasons[] | select(.kind=="skip")], .report.ledger.versions' "$E/report.json"

# ④ 现在能不能谈差异：L2 / L3 的样本够不够
jq '.report.comparison.sessions_with_both, .report.comparison.required_sessions' "$E/report.json"
jq '.report.experiment' "$E/report.json"

# ⑤ 哪条记忆在干活
jq '.report.leaderboard[:5]' "$E/report.json"

# ⑥ 直接读摘要（更适合塞进模型上下文）
sed -n '1,120p' "$E/report.md"
```

拿到线索后按这个顺序动手：先修数据（解析 / 账本可用性），再修检索与预算（`query.rs` / `policy.js`），最后才讨论「记忆有没有用」。改完回看同一窗口的 L1/L2；只有实验开启时才允许谈 L3 因果。

## 6. 已知缺口（别假装能算）

- **逐请求 join 不存在**：账本缺设计稿 §4.2 承诺的 `request_seq` / `event_seq` / `epoch`，`step` 恒为 -1；注入与请求只能按会话 + turn / 日期近似。设计稿里「每条请求一行落 state」从未落地，所以「注入后这一回合省了多少」只能看 L2 / L3 的聚合差。
- **没有后台刷新**：快照只在面板生成报告时更新，见 §2。
- **只留最新一份，没有历史归档**：要比较「改动前 / 改动后」，在改代码前把当时的 `report.json` 拷到别处，或依赖窗口内的 L2 / L3 样本；不要指望快照自己保留历史。
- **两份文件不是事务**：`report.json` / `report.md` 各自原子替换，第二次写失败或进程在中途被杀时可能一新一旧；两边的头都带 `generated_at`，对不上就刷新一次，以 `report.json` 为准。
- **`state.json` 权限是 0644**（设计稿要求 0600）：它是内部缓存，不含标题正文，但分析时别把它当契约。
- **提取 token 只有上界估算**：`jobs.json` 不记 usage。
- **设计稿与实现有若干漂移**：`memory_effect_notes` 命令、报告的 `force` / 90s TTL 都只存在于设计稿。以本文件、代码与 `memory-progress.md` 为准。

如果后续需要「agent 随时主动问、不依赖面板刷新」，最小补法是加一条只读桥路由（如 `GET /v1/memory/effect?range_days=`）与一个薄工具，返回摘要而不是整份报告；**不要**在 JS / 脚本里重写 scan 聚合。

## 7. 隐私边界

- 快照与 `memory.json` 同目录、0600；不含记忆正文、关键词、来源字段。排行榜条目会带记忆 id、标题、scope、verification——都是面板本来就展示的字段，标题来自 `memory.json`，只落在本机这份快照里。
- 分析可以引用计数、原因、样本量与标题；不要把快照内容、会话日志原文粘进远端模型的上下文——那和把记忆正文发出去没有区别。

## 8. 判据与验证

- `cd src-tauri && cargo test --locked memory::effect_export`：信封与 schema、空窗口「样本不足」、`skip` / `inject` 分列、标题保留而正文绝不落盘、写失败的可操作文案、0600；另有一条 scoped 临时 home 下直接跑 `report_and_write` 的生产入口用例，断言两个文件真的落在实例 `effect/` 下（判据名见 `memory-progress.md` 的 2026-11-01 条目）。
- 生产接线：`memory_effect_report` 是快照的唯一写入点（`src-tauri/src/memory/cmd.rs`）。现有 e2e 用例会真实调用这条命令、但**没有**断言文件存在；路径解析与落盘由上面的生产入口单测覆盖，接线本身仍靠唯一调用点与代码审查。别把这条读成端到端验过文件。
- 写快照是 best-effort：失败只记 `memory-effect` 壳日志，面板仍返回报告。
