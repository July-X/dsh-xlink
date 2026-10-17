# 记忆注入对模型上下文缓存的影响（2026-10-17 实测）

调研问题：记忆系统会不会拉低模型的**前缀缓存命中率**（DeepSeek / OpenAI 兼容接口的
automatic prefix caching，命中价差约 10 倍）？会不会是「缓存命中下降」的元凶？

**结论：不会。** 注入块出现在 10-16 / 10-17 两天，合计 84 KiB ≈ 2.8 万 tokens；这两天
prompt 总量约 2.0 亿，记忆块占 **0.014%**（放进 6 周总量 56.3 亿里是 0.0005%）。控制变量后
（同一模型、两次请求之间没有压缩）有记忆会话 96.73%、无记忆会话 96.83%，差 0.1pp，在噪声里。
真正吃掉 miss 的是**压缩**与**上游缓存过期**，不是记忆。

## 一、机制：记忆块根本进不了 system prompt

| 环节 | 事实 | 出处 |
| --- | --- | --- |
| 注册面 | 插件在 `agent/created` 里注册一条**运行上下文贡献**（`systemPrompt.context`，`order: 300`，`interpolate: false`），正文由 `text: () => box.state.text` 现取 | `plugins/memory/host/index.js` |
| 内核装配 | 内核把这条贡献与 cwd / 文件策略 / 审批策略一起 `joinContextSections`，拼成 **runtime-context 快照**，作为一条 **user 消息**追加到会话末尾（`RuntimeContextProjection.project`） | 内核包 `@deepseek-ai/dsh-system-prompt`、`@deepseek-ai/dsh-agent-loop` |
| 追加时机 | **只有拼出来的整段文本与上一次不同才追加**（`if (this.retained?.text === snapshot) return;`）；旧快照留在历史里直到压缩 | 同上 |
| 与 system prompt 的关系 | system prompt 走 `renderPrompt(assembly.sections)`，快照走 `joinContextSections(assembly.contexts)`——**两个不同的数组**，记忆块从不进入前缀最前面的系统提示 | 内核包 `@deepseek-ai/dsh-system-prompt` |

也就是说：**改记忆块只会在对话末尾追加一条新消息，不会重写前缀**。这正是前缀缓存最友好的形状。

## 二、插件自己的三道闸（只数「内容真的变了」）

- `MAX_INJECTIONS_PER_SESSION = 3`：每会话最多 3 次**内容变化**；同一回合只检索一次
  （`shouldRefresh` 按 `turn` 去重），`applyResult` 只在 `next !== state.text` 时才 +1。
- `MAX_LIBRARY_REFRESHES_PER_SESSION = 2`：预算用完之后只按**数据代次**探测（批准 / 删除 /
  编辑才 +1），探到没变就一个字节都不动。
- `MAX_TOTAL_BYTES_PER_SESSION = 36 KiB`：换过几次才算几次，重复渲染不占额度。

出处：`plugins/memory/host/policy.js`。压缩成功后预算清零（`afterCompaction`），压缩失败不清零。

## 三、实测（本机 release 实例，两天窗口）

数据源：`~/.dsh-xlink/kernels/dsh/instances/default/home/sessions/**/session*.jsonl.zstd`
（`zstd -dc` 解码）。336 个会话文件里 170 个是 v3/v4（带 runtime-context 投影与
`usage` 字段），其中 159 个有可分析的请求，时间跨度 **2026-09-05 → 10-17**，
共 **25,981 次请求 / 56.3 亿 prompt tokens / 整体命中率 97.97%**
（`assistant/message.usage`：`cacheReadTokens` 命中、`inputTokens` 未命中）。

记忆注入只出现在其中的 10-16 / 10-17 两天（14 个会话）。这两天的命中率是 6 周里最低的
（10-16 **77.53%**、10-17 **91.33%**，其余各天 95~99.8%）——所以「最近命中率掉了」的观感
是真的，下面第 3.4 节说明是谁吃掉的。

### 3.1 miss token 归因

| 桶 | 请求数 | 命中率 | miss | 占全部 miss |
| --- | ---: | ---: | ---: | ---: |
| ① 会话首个请求（冷启动） | 159 | 9.80% | 2.64M | 2.3% |
| ② 间隔内有压缩（历史被重写） | 916 | **57.57%** | **30.35M** | **26.5%** |
| ③ 间隔超过 5 分钟（上游缓存 TTL） | 286 | 71.11% | 18.16M | 15.9% |
| ④ 间隔内只追加了记忆快照 | **1** | 0.32% | 0.35M | **0.3%** |
| ⑤ 正常续写 | 24,619 | 98.86% | 62.88M | 55.0% |

### 3.2 记忆块自身的体量

- 91 份含记忆正文的快照（分布在 14 个会话）：**15 份首发**（首个请求之前）、**61 份压缩后原样
  重发**（内容逐字节相同，是内核在快照被压缩掉之后重新追加的）、**10 份内容有变化**——
  逐份 diff 过，10 份全部来自库被真的改过（新增 / 删除 / `updated_at` 变更），
  **没有一次是「只是顺序变了」**。
- 记忆段（不含 cwd / 策略那几段）p50 **780 B**、max 1297 B，合计 84 KiB ≈ 2.8 万 tokens。
- 单份快照总量 p50 1725 B、max 2606 B。

### 3.3 控制变量：有记忆 vs 无记忆

| 分组（只取 `opencode/deepseek-v4.1-flash`，后续请求） | 请求 | 命中率 |
| --- | ---: | ---: |
| 普通会话·无压缩 | 1114 | 96.83% |
| 记忆会话·无压缩 | 593 | **96.73%** |
| 记忆会话·重压缩·无压缩 | 465 | 93.22% |
| 普通会话·有压缩 | 77 | 31.76% |
| 记忆会话·有压缩 | 35 | 22.29% |
| 记忆会话·重压缩·有压缩 | 275 | 35.60% |

不分组时「有记忆注入」的会话命中率（89.7%）确实低于其余（98.3%），**但那两个会话正是
各自发生 132 / 200 次压缩的长会话**——差距来自压缩，不是记忆。同模型、把压缩间隔去掉后，
有记忆与无记忆的差距消失（0.1pp）。

### 3.4 最近两天命中率下滑是谁吃掉的

| 会话 | 请求 | 压缩启动次数 | miss | 其中发生在「压缩后的请求」上 |
| --- | ---: | ---: | ---: | ---: |
| `session-fbb5df89`（10-16 20:40 起） | 509 | **200** | 8.45M | **83.7%** |
| `session-b049253b`（10-17 11:32 起） | 231 | **132** | 4.26M | **86.6%** |
| `766ada2c`（10-16） | 29 | 9 | 0.46M | 79.3% |
| `session-6cebb93b`（本会话，对照） | 371 | 1 | 4.56M | 3.5% |

10-16 当天 miss 的 **88.2%** 来自 `session-fbb5df89` 与 `766ada2c` 两个压缩会话；10-17 当天
前三个会话占 57.8%。这两个重压缩会话同时也是「含记忆快照最多」的两个——**相关性来自它们又长、
又反复压缩，不是记忆**：把它们的记忆段整段删掉，也只省下各自 miss 的 0.03% 不到。

## 四、优化方案

### ① 压缩是唯一值得动手的大头（不是记忆）

916 次「间隔内有压缩」的请求（3.5%）吃掉 26.5% 的 miss；两条长会话里压缩平均每 2~3 次
请求就来一次，每次都是一次摘要 LLM 调用 + 历史重写。方案：

1. **报上游内核**：这份数据（压缩次数、压缩前后 token、命中率）足以支撑一个 issue。
2. **本地可调**：内核 `@deepseek-ai/dsh-compaction-basic` 暴露 `thresholdRatio`、`retention`、
   `maxTokens`、`modelPolicies`，当前走默认值；换模型（1M 窗口 × `maxTokens` 384K）与
   200K 窗口混用时阈值是否还合理，要单独做一次实验。壳侧的 `cordis.patch.yml` 目前只允许
   追加技能 loader 行（见 `AGENTS.md`），要动压缩配置得先扩大那条接线约定。
3. 使用侧：把 500 步的单回合拆成多回合/多会话，压缩风暴会显著减少。

### ② 空闲间隔（miss 的 15.9%）

上游缓存有 TTL，间隔 > 5 分钟的请求命中率掉到 71%，> 数小时的基本全 miss。这条没有可优化项，
只能解释给用户听（「隔夜回来第一条必然贵」）。

### ③ 记忆侧：注入不用改，要改就改自动提取

- **注入块不建议再压**：收益不到 0.02%（两天口径），而块小到看不见会让「记忆是否生效」变得不可验证。
- **不要把它挪进 system prompt**：那才会真的每次改记忆就废掉整段缓存。这条是给后来人的禁令。
- **自动提取（`auto_extract`，当前关闭）是唯一「大头 miss」的记忆功能**：一次调用最多带
  64 KiB 切片（`MAX_SLICE_BYTES`）+ 固定 system prompt，切片每次都不同 ⇒ 除系统提示那段前缀
  外基本全 miss；而且用的是会话当前模型（`agentDefaultModel`）。开启时的建议：
  切片上限收到 16~24 KiB、把「解析失败」的重试从 3 次降下来（37 个任务里 16 个失败于
  「模型没有返回任何内容」，每次失败都是白烧一次调用）、或指到便宜模型。

## 五、复现方法

```sh
# 1. 解出一个会话，看请求用量与运行上下文快照
f=~/.dsh-xlink/kernels/dsh/instances/default/home/sessions/*/*/session.v4.jsonl.zstd
zstd -dc $f | rg -o '"usage":\{[^}]*\}' | head
# 2. 快照 = user/message 且 data.source.kind == "runtime-context"
#    含记忆 = 正文里有「以下是本机保存的」（src-tauri/src/memory/query.rs::render_context 的开场白）
```

判据是内核归一化后的 `usage.cacheReadTokens / inputTokens`；不同 provider（MiniMax / GLM /
DeepSeek）各自的口径可能不同，跨 provider 比较时只比趋势不比绝对值。
