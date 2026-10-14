# 本地记忆系统：开发清单与进度

**这份文件是记忆系统的开发清单。** 设计稿 [dsh-xlink-memory-design.md](dsh-xlink-memory-design.md)
回答「为什么这么设计」，本文件回答「现在做到哪了、还差什么、拿什么验的」。设计稿 §12 是时间线
（做过什么），本文件是**当前状态**（还差什么）——两份都要看，职责不重叠。

> ## 维护规则（改记忆系统的人都要读）
>
> 1. **改行为 = 同一提交里改本文件**。动 `src-tauri/src/memory/**`、`plugins/memory/**`、
>    `ui/src/memory/**` 里任何影响行为的东西，就在对应区段把那一条的状态、判据、剩余差距
>    改成当下事实。只改代码不改这一份，等于把「进度表」变成一份没人敢信的文档。
> 2. **状态必须带判据**。`✅` 要写清是哪个用例/哪次真机验证；拿不出判据的写 `⬜ 未验证`
>    而不是 `✅`。判据失效了（用例删了、验证的版本升了）同样要改。
> 3. **条目编号稳定**。加条目往后排（`A15`…），不要重排已有编号——别的文档和提交信息会引用它。
> 4. **门禁**：`npm run check`（全量）；真机：`npm run e2e -- --spec ./e2e/specs/memory.spec.mjs`。
>    新增模块时 `check-invariants` 的「记忆进度表覆盖所有模块」会红——那是提醒在这里补一行。

状态图例：`✅` 已落地且有判据 · `🟡` 部分落地（写明差什么）· `⬜` 未开始 · `🚫` 设计稿明确不做

最后同步：2026-10-14（对应设计稿修订 2026-10-14，内核 `0.2.1-alpha.2`）。

## 总览

| 阶段 | 出处 | 状态 | 一句话 |
| --- | --- | --- | --- |
| P0 存储选型与真实接缝验证 | §4.1、§10 | ✅ | 单文档 JSON 定案；内核挂载插件、首请求带记忆、物化与接线都真机验过 |
| P1 人工保存 + 候选确认 + 检索 + 上下文与工具 + 管理页 + 导出 | §2、§3.1、§6、§7、§10 | 🟡 | 主链路可用；缺「工作台显示本次用了 N 条」、冲突/合并/过期标记、清空、Windows 验收 |
| P2 本地模型自动提取（游标、租约、取消、预算） | §8、§10 | ⬜ | 只有 `auto_extract` 字段与界面占位，代码零实现 |

## A. 存储与数据模型（§4.1、§5、§6.1）

- ✅ **A1 单文档 JSON 为权威记录，不引入 SQLite**（§4.1）—— `src-tauri/src/memory/store.rs`；
  `Cargo.lock` 无 sqlite（含间接）；选型理由见 §4.1 定案
- ✅ **A2 落盘路径 `<DSH_XLINK_HOME>/kernels/<family>/instances/<id>/memory/<profile>/`**（§5）——
  `shell/paths.rs::instance_memory_dir` 派生 + `validate_id_component`；用例 `store_lives_under_the_instance_directory`
- ✅ **A3 命名空间 `(family, instance_id, profile)`，不加 mode 维**（§5）—— `store.rs::Namespace`；
  面板走 `default_instance_key()`，桥走绑定
- ✅ **A4 `memory.json` 0600 + 原子替换**（§5）—— `shell/atomic_write_private`；`persist`
- ✅ **A5 损坏文档：读-改-写路径拒绝写入，展示路径容错读并明说**（§6.1）—— `load` 对 `load_for_display`；
  用例 `corrupt_file_is_an_error_not_a_reset`、`find_by_id_reports_a_corrupt_document`
- ✅ **A6 `schema_version` 比代码新时拒绝写入，不就地降级**（§6.1）—— `check_writable`；用例 `newer_schema_refuses_writes`
- ✅ **A7 `meta`：schema 版本 / 数据代次 / 两个开关 / 抑制指纹**（§6.1）—— `MemoryMeta`
- ✅ **A8 记录字段：id、范围、正文、主题/关键词、来源、验证、revision、状态、使用时间**（§6.1）—— `MemoryRecord`
- ✅ **A9 标题/正文长度上限**（§3.1 的预算推论）—— `MAX_TITLE_BYTES=512`、`MAX_BODY_BYTES=8 KiB`；
  超出即拒绝（不静默截断）
- 🟡 **A10 候选的来源细节：事件起止序号、源 revision/hash、提取器版本**（§6.1）—— 现在候选只有
  `source.session`（哪个会话提的）。**还差**：这些字段随 P2 的游标一起加（见 H2/H3）
- ✅ **A11 来源种类 `user-manual` / `session` / `extract`**（§6.1）—— `SOURCE_*`；`extract` 只有常量，P2 才写入
- ✅ **A12 验证范围 `verified` / `unverified` / `stale`，不用置信分数**（§6.1）—— `VERIFICATION_*`；
  排序里参与加权
- 🟡 **A13 「过期」要有入口**（§6.1、§7.2）—— `VERIFICATION_STALE` 参与排序，但**面板没有把它置为
  stale 的操作**（只能编辑正文或停用）。**还差**：面板加「标为过期」或按时间自动过期
- 🟡 **A14 个人偏好不能从项目候选自动提升**（§6.1）—— 没有自动提升这条路径（✅），但「提升前展示
  正文与来源、去掉路径/客户名/私有信息、由用户明确选通用范围」只有一半：批准时用的是候选自带的范围，
  改范围要批准后再编辑。**还差**：批准对话框里直接选范围 + 一句去私有信息的提醒
- ✅ **A15 整库体积上限 1 MiB**（实现补充，设计稿之外）—— `input::MAX_DOC_BYTES` 在唯一写入口判
  「变大且越界」，缩小永远放行；用例 `the_document_size_cap_is_wired_and_deleting_still_works_after_it_trips`
- ⬜ **A16 跨进程并发（两个壳指向同一实例）**（§4.1 末）—— 设计稿把它留给 P2 的租约，当前只有
  进程内 Mutex。**还差**：P2 时评估（加 `jobs` 段或换承载，语义不变）
- ⬜ **A17 磁盘满的明确状态**（§10）—— 错误文案提到了写不进去，但没有真的模拟磁盘满的用例。
  **还差**：一条只读/满盘的用例

## B. 检索与注入（§3.1、§6.2）

- ✅ **B1 `agent/created` 里按 agent 作用域注册 `xlink.memory`**（§3.1-1）—— `plugins/memory/host/index.js`；
  真机 `dsh headless` 验证（挂载 + 注册 + 首请求带记忆）
- ✅ **B2 注册监听器内一切失败都吞掉，降级为「本次无记忆」**（§3.1-1）—— try/catch + 面探测；
  真机验证（没有 `agent.ctx.systemPrompt.context` 时只 warn）
- ✅ **B3 `name`/`order`(300)/`text` + `interpolate:false`，注册普通 context**（§3.1-2）—— `CONTEXT_NAME`/`CONTEXT_ORDER`
- ✅ **B4 `agent/pre-step` 先 `next()`、保留决定、返回原决定**（§3.1-3）—— `index.js` 的 pre-step
- ✅ **B5 项目身份取有效工作目录 + `resolveByPath`，不读冻结的 `session.header.cwd`**（§3.1-5）——
  `workingDirectory(ctx, agent)` + `policy::resolveWorkspaceId`（解析不到只带路径，按路径兜底）
- ✅ **B6 目录一变就清空缓存，且**立刻**生效（含预算用满的情况）**（§3.1-5）—— `policy::retarget` +
  `beginStep`；用例 `预算用满之后换目录：缓存也要立刻清空（接线，不只是纯函数）`（反向验过）
- ✅ **B7 换目录不重置注入预算**（§3.1-6）—— `retarget` 保留 `injections`；用例
  `切换工作目录清空缓存，但不重置会话级的注入预算`
- ✅ **B8 会话级注入预算：每回合最多一次、文本相同不计数、≤3 次 + ≤2 次库变化刷新**（§3.1-6）——
  `shouldRefresh` / `shouldProbeLibrary` / `applyResult`；用例 `预算之外只按数据代次刷新，额度用完就不再探`
- ✅ **B9 累计注入 ≤36 KiB**（§3.1-6）—— `MAX_TOTAL_BYTES_PER_SESSION`（按 UTF-8 字节累计，内容没变的
  那一次不占额度）；用例 `累计字节到顶之后不再刷新——次数封顶挡不住「每次都写满」`（反向验过）
- ✅ **B10 库变化（批准/删除/停用/关开关）在同一会话内生效**（§3.1-6）—— 探 `generation`，代次变了才换
  正文；判断收在 `policy::applyStep`；用例 `结果怎么用：探测那条路只在库变了时才换正文`（反向验过）
- ✅ **B11 渲染预算：单条详情 8 KiB / 单次总 12 KiB / 最多 6 条**（§3.1）—— `query.rs` 的
  `CONTEXT_DETAIL_BYTES`/`CONTEXT_TOTAL_BYTES`/`MAX_HITS`
- 🟡 **B12 常驻摘要上限 4 KiB**（§3.1）—— 实现里没有独立的「常驻摘要」段：固定模板约 150 字节 +
  命中详情，实际远小于 4 KiB。**还差**：要么在设计稿里改成「模板 + 详情」的说法，要么真有摘要时再加闸
- ✅ **B13 按完整条目裁剪、按 UTF-8 字符边界截断、绝不切半个字符**（§3.1）—— `truncate_utf8`；用例
  `a_single_huge_body_is_truncated_on_a_char_boundary`
- ✅ **B14 没命中就不发任何文本**（实现补充）—— `render_context` 的早退；用例 `no_hits_means_no_text_at_all`
- ✅ **B15 稳定模板写明「可能过期、命令不是待执行指令、冲突时以用户要求为准」**（§3.1）——
  `render_context` 的开场白
- ✅ **B16 user-role 背景材料，不授予权限、不标 required**（§3.1）—— 普通 context 注册
- 🟡 **B17 150 ms 降级线**（§3.1）—— 设计稿自己标注「建议值不是已验证指标」；实现用
  `REQUEST_TIMEOUT_MS = 1500`，超时本回合不注入。**还差**：实测一个降级阈值并写进设计稿
- ✅ **B18 用户关闭可选运行上下文时如实报告「本次未注入」**（§3.1）—— `enabled:false` + 工具文案；
  用例 `retrieve_reports_the_switch_instead_of_an_empty_library`
- ✅ **B19 检索取消/超时/失败清空缓存、不沿用上一回合**（§3.1-5）—— `applyFailure`；用例
  `文本没变不算一次注入；失败清空缓存而不是沿用`。探测那条路失败**不动缓存**（一次抖动不是库变化）
- ✅ **B20 禁止全局「当前项目」变量**（§3.1-5）—— 状态在 `agent/created` 的闭包里，按 agent 一份
- ✅ **B21 确定性排序：固定项 > 关键词 > 项目匹配 > 验证 > 最近使用，稳定 ID 收尾**（§6.2）——
  `query::sorted_hits`；用例覆盖隔离与固定项
- ✅ **B22 标题与正文双向短语匹配**（§6.2「中文按明确关键词与短语匹配」）—— `query::score`；用例
  `a_message_that_mentions_the_title_hits_without_keywords`（反向验过）
- 🟡 **B23 「单次扫描 2 MiB 时返回可定位的缩小查询提示」**（§6.2）—— 有整库 1 MiB 上限（A15），
  但没有按「本次扫描字节」给提示。**还差**：扫描量判据或把这条从设计稿划掉
- 🟡 **B24 展示次数与有效引用次数分开**（§6.2）—— 只有 `use_count`/`last_used_at`（模型按 id 读时 +1），
  没有「回答中引用 ID」的计数。**还差**：要么实现引用计数，要么在设计稿里收窄这条
- ✅ **B25 三个工具 `memory_search` / `memory_read` / `memory_propose`**（§6.2）—— `index.js::registerTools`
- ✅ **B26 工具从 agent/session 推导项目，不接受调用者传实例/目录/路径**（§6.2）—— 工具里没有这些参数，
  坐标由 `workingDirectory` 与桥绑定给出
- ✅ **B27 无 agent 的工具调用被拒绝**（§6.2、§10）—— 三个工具都 `if (!exec.agent) throw`
- 🟡 **B28 输出模式、输入大小、取消信号都要校验**（§6.2）—— 输出 schema ✅、输入大小 ✅
  （`MAX_PATH_BYTES` 7000 + `MAX_QUERY_CHARS` 500）、**取消信号 ⬜**（工具随 agent 取消，插件没有
  自己的取消控制器）。**还差**：P2 需要（见 H6）

## C. 写入路径（§7）

- ✅ **C1 保存/接受候选校验预期 revision（CAS）**（§7.1、§7.2）—— `store::save` 的 `expected_revision`；
  用例 `update_uses_compare_and_swap`
- ✅ **C2 写入正文与来源、数据代次 +1**（§7.1）—— `mutate`；`status.generation` 面板可见
- ✅ **C3 模型不改权威记录**（§7.1）—— 桥只有 `propose` → `submit_candidate`，批准只存在于 Tauri 命令面
- ⬜ **C4 相同规范化正文可合并来源**（§7.2）—— 未实现。`fingerprint` 只用于删除抑制。
  **还差**：写入时按指纹合并 `source` 列表 + 用例
- ⬜ **C5 意义冲突标「待处理冲突」，不由模型选择胜者**（§7.2）—— 未实现（没有冲突字段，也没有检测）。
  **还差**：先定「怎么判冲突」（同 scope 同关键词正文相似？），再谈字段与界面
- 🟡 **C6 用户纠正优先于旧候选**（§7.2）—— 用户手动保存一条即可覆盖语义，但旧候选不会被自动标过期
  或标注「已被用户纠正」。**还差**：批准/保存时把同主题候选标过期
- 🟡 **C7 过期的技术事实要重新验证后再用**（§7.2）—— 见 A13（没有置 stale 的入口）
- ✅ **C8 删除：移除记录 + 记正文指纹 + 代次 +1**（§7.3）—— `store::delete`；用例覆盖
- ✅ **C9 删除后立即阻止再次检索与按 id 读**（§7.3）—— 用例 `disabling_a_record_removes_it_from_retrieval`、`find_by_id_hides_disabled_records_unless_asked`、桥的 `memory_routes_work_over_the_bridge`
- ✅ **C10 删除前显示影响范围、需明确确认**（§7.3）—— 面板 `confirmDialog`（含「已导出的文件与旧请求
  不会被撤回」）
- ✅ **C11 相同来源不能自动再生（抑制指纹）**（§7.3）—— `submit_candidate` 查 `meta.suppressed`；用例 `a_deleted_body_is_not_accepted_back_from_the_model`
- ✅ **C12 界面说明会话历史/已导出文件的边界，并引导新开会话**（§7.3）—— 删除确认文案
- ⬜ **C13 清空（全部删除）**（§2 的写操作清单）—— 面板没有「清空全部」入口。**还差**：按 §7.3
  「清空前列出记录与候选范围、需用户明确确认」实现
- ✅ **C14 跨项目推广（项目记忆改通用）**（§2）—— 面板编辑里可改 scope

## D. 管理面板（§2）

- ✅ **D1 侧栏「记忆」入口 + 面板可打开**（§2）—— `ui/src/memory/MemoryPanel.vue`；e2e 用例
- ✅ **D2 新建/编辑一条记忆，选项目或通用**（§2）—— 编辑器 + `canSaveDraft`（项目必须填目录）
- ✅ **D3 「待确认」视图**（§2）—— 候选卡片，正文与来源都摆出来，只有点保存才成为记忆
- 🟡 **D4 「此实例的通用偏好」独立视图**（§2）—— 现在是一个混合列表 + scope 标签，没有按范围分视图。
  **还差**：范围筛选（或两个 tab）
- 🟡 **D5 列表显示最后修改时间、来源和验证状态**（§2）—— 时间 ✅、`已用 N 次` ✅、固定/已停用 ✅；
  **来源与验证状态只在候选卡片里显示**。**还差**：行里补来源 kind 与验证状态
- ⬜ **D6 工作台显示「本次使用了 N 条记忆」，可展开看内容与来源**（§2）—— 未实现。现在只有面板的
  「检索预览」。**还差**：内核侧没有壳能渲染的地方，要么做成插件在工具结果/上下文里的计数，要么
  从壳侧读会话事件反推（先定方案再动手）
- ✅ **D7 停用后新回合不再新增贡献**（§2）—— 注入只取 `accepted`；用例
- ✅ **D8 删除后阻止再次检索**（§2）—— 同 C9
- ✅ **D9 导出 Markdown 一次性快照**（§2）—— `memory/export.rs`；用例 `export_writes_a_private_snapshot_with_everything_in_it`、`unreadable_document_is_reported_inside_the_snapshot`
- ✅ **D10 导出不参与运行时与代次、手工修改不反向入库**（§2）—— 文件头写明；导出不碰 `memory.json`
- ✅ **D11 两个开关默认关闭；自动提取显示不可用**（§2）—— `use_approved=false`、`auto_extract=false`；
  面板把自动提取标为不可用
- 🟡 **D12 「开关生效需重启工作台」的提示**（§2）—— 后端在内核跑着时拒绝改插件开关并给出原因 ✅，
  但面板文案没有「重启后生效」这句。**还差**：插件页/记忆页补一句
- ⬜ **D13 启用页说明「本地保存 ≠ 上下文永不出网」**（§1）—— 未实现文案。**还差**：启用插件时的提示
- ⬜ **D14 与 `AGENTS.md` 指令链的分工指引**（§1.2）—— 未实现文案（「始终生效写 AGENTS.md，可检索
  带来源的写记忆」）。**还差**：面板描述里补一段
- 🚫 **D15 页面标明「设计实施后才可用」**（§2）—— 已上线，这句不再适用（设计稿该句已过期）

## E. 内核接线、桥与诊断（§3、§4、§9）

- ✅ **E1 内置插件四处登记齐全**（§4）—— `builtins.rs::BUILTINS`、`prepare-builtin-plugins.mjs`、
  `plugins/memory/package.json`、`package.json` 的 `test:plugin`；`dev-builtin.test.mjs` 钉住
- ✅ **E2 物化到 `<DSH_HOME>/extensions/builtin/xlink-memory/.../compat-a/`**（§4）—— 真机验证
  （`builtin_set_enabled` 后 `loadState: prepared`、`wired: true`、`missingPeers: []`）
- ✅ **E3 不向已安装内核 `node_modules/**` 写任何东西**（§4）—— 未触碰；物化与接线都在 DSH home 内
- ✅ **E4 桥三条路由 `context` / `read` / `propose`**（§9）—— `plugins/builtin/bridge.rs`；路由用例
- ✅ **E5 命名空间在签发与校验两侧完整**（§9）—— 令牌绑 `(family, instance)`，profile 由壳设置补齐，
  两者合成 `Namespace`；真机验证面板与插件写同一份 `memory.json`
- 🟡 **E6 「停用即时生效」靠运行中插件的失效确认，未确认时标为处理中**（§9）—— 失效靠 B10 的代次探测
  （本会话内 ≤2 次）✅；**没有「处理中」状态**。**还差**：额度用尽后的语义要说清（面板提示「新开会话生效」）
- ✅ **E7 新增路由写清「谁能调、最坏能造成什么」**（§9）—— `bridge.rs` 各路由注释
- ✅ **E8 诊断落壳侧：`shell_events` 逻辑名 `memory-bridge`，只记路由与计数**（§9）—— `note_memory_call`
- ✅ **E9 状态区分 disabled/unsupported/ready/degraded，物化只能叫 prepared**（§9）—— 内置插件状态机；
  真机验证过 `prepared` → `ready` 的区分
- ✅ **E10 失败日志不含记忆正文/模型输入/凭据**（§9）—— 事件日志只有路由与计数
- ✅ **E11 壳桥不可用时普通对话继续，工具返回明确不可用原因**（§9）—— `applyFailure` + 工具文案；
  `agent/created` 不抛
- ✅ **E12 越权、删除、许可撤销、命名空间不匹配一律清空旧缓存**（§9）—— `applyFailure` + 代次探测
- ✅ **E13 拒绝过期令牌、跨命名空间请求与任意路径参数**（§9）—— Bearer 校验 + 坐标由绑定决定 +
  参数白名单（`q`/`id`/`workspace*`/`title`/`body`/`scope`/`session`）

## F. 交付与门禁（§4、§10）

- ✅ **F1 新文件登记 `FILE_BUDGETS` 且 ≤800 行硬顶**（§10 前置）—— `scripts/check-code-budget.mjs`
- ✅ **F2 `TOTAL_BUDGET` 同提交上调并写明理由**（§10 前置）—— 现为 51646；每次上调都有提交信息
- ✅ **F3 `builtins.rs` 预算随条目调整**（§10 前置）—— 已登记
- ✅ **F4 UI 产物预算实测**（§10 前置）—— `check-ui-bundle-budget`：JS 698,642 / 720,000、CSS 245,288 / 256,000
- ✅ **F5 资源生成步骤挂进所有会跑 Rust 构建的 workflow**（§10 前置）—— `scripts/dev-builtin.test.mjs` 钉住
- 🟡 **F6 macOS Intel 与 Windows 分别验收**（§10）—— macOS 本机全量跑过（946 Rust + 517 UI + e2e）；
  **Windows 只跑 `cargo check`，测试从未在那台机器上执行过**。**还差**：在 Windows 上跑一次
  `npm run check`（那条 `\\?\` 前缀的路径归一用例就是为此写的）
- ✅ **F7 测试持 `scoped_xlink_home`/`TempHome`，只清自己的目录**（§10）—— `memory::store::testkit`

## G. 行为测试清单（§10）

- ✅ **G1 A 项目保存的记忆不进入 B 项目** —— `retrieve_keeps_projects_apart`
- ✅ **G2 会话中途切目录后新目录的项目记忆不再注入** —— `beginStep` 用例 + `retarget` 用例
- 🟡 **G3 候选不记到旧项目**（§10）—— 提候选时每次重新取有效工作目录 ✅（代码路径），**没有专门用例**
- 🟡 **G4 无工作区、路径改名与大小写差异不扩大范围** —— 无工作区 ✅（只允许通用偏好）、路径改名 ✅
  （id 优先、路径兜底）；**Windows 大小写差异 ⬜ 未测**
- ✅ **G5 第一个真实请求包含记忆** —— 真机 `dsh headless` + 桩模型：请求体里带着注入正文
- ⬜ **G6 `agent/pre-step` 拒绝或取消时不提交缓存**（§10）—— 没有 `next()` reject 的用例。
  **还差**：插件侧用例（`decision` 为拒绝时不得改动 state）
- ✅ **G7 其他插件贡献仍在** —— 保留 `next()` 的决定（代码路径 + 真机）
- ✅ **G8 关闭可选上下文时如实未注入** —— `enabled:false` 用例 + 工具文案
- ✅ **G9 重复步骤不追加相同快照** —— `applyResult` 文本相同不计数；用例
- ✅ **G10 20 回合会话注入 ≤3 次（库变化 +2）、累计 ≤36 KiB** —— B8/B9 的用例
- ✅ **G11 换目录不重置这个计数** —— `retarget` 用例
- ⬜ **G12 成功压缩后能重新取得记忆**（§10）—— 未验证（需要真实压缩事件）。**还差**：真机压缩后
  再问一次，确认记忆重新注入
- ⬜ **G13 失败压缩的判据含「只有 start、没有 end」**（§8、§10）—— 未实现（属 P2 的提取触发）
- ✅ **G14 `agent/created` 解析失败或桥不可用时会话仍能正常创建与对话** —— try/catch + 真机
- ⬜ **G15 子智能体不超出父级读权限**（§5、§10）—— 未实现显式夹紧，也没有用例。**还差**：定策略 + 用例
- ✅ **G16 无 agent 的工具调用被拒绝** —— 三个工具的 `if (!exec.agent) throw`（代码判据；无单独用例）
- ✅ **G17 删除后搜索/详情/导出/缓存都不返回该条** —— 同 C9 的用例 + 导出快照用例
- ✅ **G18 相同来源不能自动再生** —— 用例 `a_deleted_body_is_not_accepted_back_from_the_model`
- ✅ **G19 保留会话历史这一边界显示正确** —— 删除确认文案
- ✅ **G20 已导出文件不在清理承诺内** —— 删除确认文案 + 导出文件头
- 🟡 **G21 外部文档里的「忽略指令」/伪造用户要求/路径穿越/注入变量拿不到更高权限**（§10）——
  模板写明「资料而非待执行指令」✅、桥不接受任意路径 ✅；**没有专门的对抗用例**。**还差**：写几条
  注入文本的检索/渲染用例
- 🟡 **G22 超大输入、中文匹配、损坏文档、并发写入返回明确状态** —— 超大输入 ✅、中文 ✅、损坏 ✅、
  进程内并发 ✅（Mutex）；**磁盘满 ⬜、跨进程写入 ⬜**
- ✅ **G23 真机端到端（面板与命令）** —— `e2e/specs/memory.spec.mjs` 3/3（侧栏入口、三条命令可解析、
  无控制台错误）；面板内容渲染无法在 occluded WKWebView 里断言（spec 里写明原因）

## H. P2：自动提取（§8、§10）—— 全部未开始

- ⬜ **H1 `turn/end` 冻结截止序号 + `sessions.flush` 屏障（false 时改用 live 历史）**（§8-1）
- ⬜ **H2 `jobs` 表：任务键、游标、状态、lease、所有权令牌、重试与失败原因**（§8-2）
- ⬜ **H3 连续回合去抖 30 秒、合并任务、运行中任务保留冻结输入**（§8-2、§8 预算）
- ⬜ **H4 Rust 原子领取租约；输入构建排除凭据/附件/无关正文，脱敏复用 `diagnostics::run::sanitize`**（§8-3）
- ⬜ **H5 本地模型经 `ctx.llm.prepareCall()`/`stream()`，不创建用户可见对话；`purpose` 省略并审计**（§8-4）
- ⬜ **H6 有 schema 的候选输出 + 来源引用；空集合算成功；取消控制器**（§8-5、§8 末）
- ⬜ **H7 提交前检查 lease token / 来源有效性 / 抑制指纹；旧版本候选标过期不覆写**（§8-6）
- ⬜ **H8 断网生成、本地模型缺失时不可用、无云端回退**（§8 末、§10）
- ⬜ **H9 崩溃恢复、迟到结果、重复 `turn/end`、过期租约不造成覆盖或重复候选**（§10）

## I. 实现补充（设计稿之外，已落地）

这些是落地时加的、设计稿里没有的条目。写在这里是因为它们同样是「记忆系统的行为」，改它们也要同步本文件。

- ✅ **I1 输入规范化的共享层 `memory/input.rs`** —— 面板保存与候选提交共用一份判断（原先各抄一份）
- ✅ **I2 整库体积上限 1 MiB** —— 见 A15
- ✅ **I3 库变化在同一会话内生效（≤2 次）** —— 见 B10
- ✅ **I4 没命中就不发开场白** —— 见 B14
- ✅ **I5 会话中记录 + 会话内生效的两处引导** —— `memory_propose` 描述四类触发 + 注入块那句提醒
- ✅ **I6 两条没人调的命令已删**（`memory_read` / `memory_submit_candidate`）—— 面板从不 invoke；
  模型那条路走桥的 `read`/`propose`。`memory_status` 保留（e2e 在用）
- ✅ **I7 注入插件的「未启用」提示不误报** —— `injectionDisabled`：状态没读回来时不说
- ✅ **I8 导出不覆盖已有文件、文档读不出来时在文件头照实说** —— `export.rs`；用例覆盖

## J. 模块地图（哪个文件负责什么）

`check-invariants` 的「记忆进度表覆盖所有模块」钉住这一节：新增或改名模块时它会红。

| 模块 | 职责 | 相关条目 |
| --- | --- | --- |
| `src-tauri/src/memory/mod.rs` | 模块声明（4 个生产模块 + `cmd`） | — |
| `src-tauri/src/memory/store.rs` | 落盘事务：读-改-写、损坏与降级、记录/候选、删除抑制、代次 | A1–A17、C1–C11 |
| `src-tauri/src/memory/query.rs` | 有界检索与注入文本渲染（纯函数） | B11–B25 |
| `src-tauri/src/memory/input.rs` | 面板保存与候选提交共用的输入规范化；整库体积上限 | A9、A15、I1 |
| `src-tauri/src/memory/workspaces.rs` | 只读内核工作区注册表（面板下拉、路径归一 realpath） | B5 |
| `src-tauri/src/memory/export.rs` | Markdown 一次性快照导出 | D9、D10、I8 |
| `src-tauri/src/memory/cmd.rs` | 面板命令层（11 条 Tauri 命令，`blocking`） | D1–D14 |
| `plugins/memory/host/index.js` | 装配层：`agent/created` 注册上下文、`pre-step`、三个工具 | B1–B10、B25–B27、I5 |
| `plugins/memory/host/policy.js` | 纯策略：注入预算、代次探测、「结果怎么用」 | B6–B10 |
| `plugins/memory/host/bridge.js` | 桥客户端：凭据读取、路径拼装、超时与通道上限 | B28、E4 |
| `plugins/memory/test/smoke.test.mjs` | 策略层与桥客户端用例（19 条，含 2 条接线判据） | 各条的判据 |
| `ui/src/memory/memory.js` | 面板状态与动作（含 `injectionDisabled`） | D1–D14、I7 |
| `ui/src/memory/MemoryPanel.vue` | 面板视图（列表、候选、编辑器、预览、导出） | D1–D14 |

## 待办总表（按建议顺序）

| # | 条目 | 为什么排这个位置 |
| --- | --- | --- |
| 1 | **D6 工作台显示「本次使用了 N 条记忆」**（§2）—— 需先定方案（内核侧没有壳能渲染的位置） | 设计稿对用户的核心承诺，也是「记忆到底有没有生效」唯一的即时反馈 |
| 2 | **G6 `pre-step` 拒绝时不提交缓存** | 一条用例就能钉住的正确性缺口，成本最低 |
| 3 | **D12/D13/D14 三处文案**（重启生效、出网说明、与 AGENTS.md 的分工） | 用户看得见的承诺，纯文案 |
| 4 | **C4 合并来源 + C5 冲突标记 + C6 旧候选过期 + A13/C7 stale 入口** | 一组「同一件事的多份记录怎么收敛」，要一起定语义 |
| 5 | **C13 清空（全部删除）** | §2 的写操作清单里唯一没入口的 |
| 6 | **F6 Windows 跑一次 `npm run check`** | 路径归一的用例就是为它写的，至今没在 Windows 上执行过 |
| 7 | **G12 压缩后记忆恢复、G21 对抗用例、G22 磁盘满/跨进程** | 补行为测试的空白 |
| 8 | **D4/D5 面板的范围视图与来源/验证显示** | 体验补齐，不影响正确性 |
| 9 | **H1–H9 P2 自动提取** | 设计稿 §8 的完整流程；开工前先确认本地模型与断网验收 |

## 怎么验（判据在哪跑）

```sh
npm run check                                       # 全量门禁（含 946 条 Rust、517 条 UI、插件用例）
node --test plugins/memory/test/*.test.mjs          # 插件策略层（注入预算、代次探测、桥客户端）
cargo test --lib memory::                           # Rust 记忆模块
npm run e2e -- --spec ./e2e/specs/memory.spec.mjs   # 真机：面板入口与命令可用
```

真机运行链路的验证记录（桩桥 + 桩模型，零模型额度）见设计稿 §12「真实运行链路验收」一行。
