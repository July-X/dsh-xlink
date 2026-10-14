# 记忆内核 + 面板的独立评审（2026-10-15，第二次）

**评审者**：独立 subagent（只读，未改任何代码）。
**范围**：`src-tauri/src/memory/`（`store` / `query` / `cmd` / `export` / `recent` / `workspaces` /
`input` / `duplicates` / `lock`，以及 `jobs` / `jobs_routes` 的相关处）、`ui/src/memory/**`、
`ui/src/shell` 里与记忆相关的部分、设计文档一致性。

**为什么有第二次**：第一次评审（[memory-p2-review-2026-10-15.md](memory-p2-review-2026-10-15.md)，
18 条已处理）**明确没覆盖这些文件**——它们至今没有被独立看过。

**结论**：**没有「高」严重度问题**；10 条发现（6 中 / 4 低）。评审另外逐项确认了写路径、
注入路径、面板契约、导出与锁（见文末「我确认没问题的部分」）。

**当场修了 7 条**（#1 #2 #3 #4 #5 #8 #10），**其余 3 条是待办**（#6 #7 #9）。

---

## 中

### 1. 收掉重复候选时不判范围 —— ✅ 已修

`duplicates.rs::supersede_pending_duplicates` 不判范围：保存/批准一条记忆会把**别的项目**里同正文的
待确认候选静默改成 `rejected`。而 `duplicate_reason`（写入闸）在上一轮评审里刚做成「带范围判重」——
**同一件事的另一半**仍按全库指纹扫。

**触发**：A、B 两个仓库各自提了一条同样的约定，用户在 A 点保存 → B 的那条从「待确认」消失；
面板与导出都只列 pending，**任何界面上都看不到它**。

**修法（已落）**：把范围判据提成**一份**（`duplicates::in_scope`），两个函数共用；调用点改成传整条
记录（`&MemoryRecord`）而不是只有正文。判据
`saving_in_one_project_does_not_supersede_another_projects_candidate`（A 保存后 B 的候选仍是 pending），
**反向验过**（去掉范围判定 → B 的候选当场消失，报的正是评审描述的场景）。
**还差的**：评审建议的「面板给已忽略候选一个查看入口」——那是一个独立的小功能，记在清单里。

### 2. 导出 / 清空 / 空态用的是**筛选后**的列表 —— ✅ 已修

三处都读 `list`（`visibleMemories(view, listScope)`），而动作自己的判据是未筛选的 `memoryStore.view`
（同一件事两份判据，方向还不一致）。

**触发**：库里只有通用偏好时点「本项目」→ 卡片说「还没有记忆」（假话，右栏状态行还写着
`accepted: N`），「导出 Markdown」与「清空」同时变灰点不动。

**修法（已落）**：三处改读新的 `allMemories`（未筛选的那份），空态文案分成两句——
「还没有记忆」（库是空的）与「当前筛选下没有记忆（库里有 N 条，切到『全部』能看到）」。
判据读模板原文，钉住这两句与「不许再读 `list`」。

### 3. `injectionDisabled` 只判启用意图，不判接线事实 —— ✅ 已修

内核读的是 `cordis.patch.yml` 的**接线行**（`wired`），`requestedEnabled` 只是壳里那份 `state.json`。
只看后者两个方向都会错：① 切内核版本后 `loadState='incompatible'`，插件页明写「重新拨一次开关修复」，
记忆页一声不吭；② 启用是「先写接线行、后写 `state.json`」，中间失败留下
`wired=true, requestedEnabled=false`——内核照旧加载插件，记忆页却报「没启用」并给一个启用按钮。

**修法（已落）**：判据换成 `wired === true && loadState === 'prepared'`；`wired` **字段缺失**
（旧壳）时**也不说**——「不知道」不能当成「没接线」，那正是这条判据第一版的老毛病。
判据覆盖四种组合（接线且就绪 / 没接线 / 接线行在但内核已切换 / 两步之间失败）。

### 4. 正文输入框按字符数限长，而后端按字节闸 —— ✅ 已修

`maxlength="4000"` 数的是 UTF-16 码元，后端闸的是 8192 **字节**：中文 4000 字 ≈ 12000 字节。
用户按面板自己允许的长度写完（面板没有任何计数器提示），保存得到「超过单条上限 8192 字节」。
标题那栏 80 字（≈240 B）反而比后端的 512 B 紧，所以只有正文这处骗人。

**修法（已落）**：去掉 `maxlength`，加一个按**字节**算的计数器（「正文 N / 8192 字节」），
`canSaveDraft` 超限即拦住；`MAX_BODY_BYTES` 与 `byteLength` 放在 `memory.js` 里，
注释写明与后端 `store::MAX_BODY_BYTES` 逐字对齐。判据：4000 个中文字不许保存、2000 个放行、
模板里不许再出现 `maxlength="4000"`。

### 5. `MAX_SUPPRESSED(500) < MAX_MEMORIES(1000)` —— ✅ 已修

删除/清空累计超过 500 条时最早的正文指纹被挤掉，被删内容可再被模型提回来。**触发**：清空一个有
600+ 条小记录的库（1000 条小记录确实放得进 1 MiB），`clear` 只留后 500 枚指纹；而删除与清空的
确认文案都承诺「记下正文指纹防止模型把同一条重新提回来」。

**修法（已落）**：上限提到 **1000**（与 `MAX_MEMORIES` 同量级），注释写明 1000 枚指纹约 64 KB、
离 1 MiB 的文档上限还远，没有理由省这一半。

### 6. 固定项超过 6 条后静默截断，而面板写的是「每次都带上」 —— ⬜ 待办

`take(MAX_HITS)` 的溢出既不计入 `omitted`（那行注记只统计**体积**跳过的），面板也不体现。
**触发**：pin 了 7 条 → 只有 6 条进上下文，丢哪条由 `verification_rank → last_used_at → updated_at → id`
决定，用户看不出规律。**次一级**：`pinned` 只是 +1000 分而不是排序键，一条 10 个以上关键词全命中的
记录（每词 +100）得 1080 会压过固定项，把 pin 挤出前 6。

**方向**：pin 改成排序键（或面板限制/警告 pin 数量），条数溢出也写进行内说明。
**评审自注**：pin 与关键词打分的交互是按代码推的，没有真机跑满 7 条 pin。

---

## 低

7. **整库体积闸漏算 `workspace_path` / `source.session`** —— ⬜ 待办：`doc_bytes` 只数标题、正文、
   关键词。项目路径是手填的（下拉 `allow-create`，`normalize_path` 对不存在的路径只去空白与尾分隔符，
   **无长度上限**），粘一大段文本就能在 `doc_bytes` 报「已到 1 MiB」的同时把 `memory.json` 撑到几 MB，
   而每次检索都要解析整份文档。**方向**：计入 `doc_bytes` 或单独设上限，并在写入口拒绝。
8. **「库读不出来」被当成「开关关着」** —— ✅ 已修：`/enqueue` 原先
   `store::load(..).unwrap_or(false)` → 库损坏时回 `disabled`，插件与日志都不留真因。
   现在读失败**如实报错**（`failed(error)`），与「用户没开这个功能」分开答复。
9. **`note` 路由的两个纵深缺口** —— ⬜ 待办：① 这是唯一一条**不过** `scope_matches` 的按 id 读取路径
   （`find_by_id(.., true)` 连停用都不挡）：一个越权/传错的 id 会让别的项目的**标题**出现在面板
   「最近注入」里；② `ids` 没有条数上限（8 KiB 请求头里最坏 ~250 个），而每个 id 都整库解析一遍。
   正常路径只有 ≤6 个 id。**方向**：限条数（对齐 `MAX_HITS`）并带坐标做一次范围校验。
10. **死代码三处** —— ✅ 已修：① `memoryStore.failure` 只写不读（面板用的是 `status.warning`）→ 删；
    ② `jobs_routes.rs` 写死字面量 `"extract"` 而不用 `store::SOURCE_EXTRACT`，且那个常量的
    `#[allow(dead_code, reason = "P2…首版不写")]` 已过期 → 改用常量并删掉过期的 allow；
    ③ `export.rs` 的 `CANDIDATE_REJECTED` 分支到不了（`state_text` 只服务 `MemoryRecord`）→ 删，
    并写明候选的状态在导出里另有写法。

---

## 我确认没问题的部分（评审的覆盖面）

- **写路径**：`mutate` 是唯一写入口（`persist` 无外部调用方）；进程内 Mutex + 跨进程 `WriteLock`
  覆盖整段读-改-写；`check_writable`（库比代码新即拒）与 `ensure_doc_growth`（缩小永远放行，
  删除/开关/清空不会被卡死）都挂在入口上；临时文件 0600 + fsync + rename + 父目录 fsync；
  损坏 ≠ 空库（写路径 Err、展示路径容错读并带 warning）。
- **注入路径**：`retrieve` 与按 id 读都过 `scope_matches`，「没有坐标就没有访问权」；路径兜底是
  realpath **相等**而不是前缀/包含匹配；`MAX_HITS` / 单条 8 KiB / 总 12 KiB 都按字节真算、
  截断落在 UTF-8 边界、表头永远完整；模板里「资料而非指令」那句还在；一条没命中就一个字节都不发。
- **面板契约**：14 条命令的字段名与结构体逐个核对过（结构体 snake_case、命令参数 camelCase）；
  14 条都同时登记在 `app-commands.json` 与 `generate_handler`。
- **导出**：0600、不覆盖、目录固定派生（无用户输入拼路径）、文档读不出来时在文件头照实说。
- **锁**：同线程重入计数与 `Drop` 递减配对、guard 非 `Send`、锁文件 0600、拿不到锁是有界失败
  而不是静默无锁写入。

## 评审不确定、需要实测的三处

1. **Windows 上 `atomic_write` 的替换语义**：按 Rust std，`fs::rename` 在 Windows 上带
   `MOVEFILE_REPLACE_EXISTING`，`replace_file` 的 remove+rename 分支应当到不了——评审没有 Windows
   实测，故未作为发现上报。**这一条与清单里的 F6（Windows 门禁）是同一件事。**
2. 第 6 条的 pin 与关键词打分交互（按代码推的，没有真机跑满 7 条 pin）。
3. `el-select` + `allow-create` 是否有隐藏的输入长度限制（评审判断没有，没在真壳上试过超长路径）。
