# 记忆系统 P2 链的独立评审（2026-10-15）

**评审者**：独立 subagent（只评审、未改代码），按本机安装的 `0.2.1-alpha.2` 内核源码逐条核对。
**范围**：插件侧（`plugins/memory/host/`：`index.js` / `run.js` / `extract.js` / `sessionlog.js` /
`bridge.js` / `policy.js` / `mask.js`）与壳侧（`src-tauri/src/memory/` 的 `jobs.rs` / `routes.rs` /
`lock.rs` / `store.rs::submit_candidate`，以及 `plugins/builtin/bridge.rs` 的两条手动夹具）。

**为什么值得单独留一份**：到 2026-10-15 为止**已修 13 条**（含两条「高」）、**2 条修了一半**；**其余 3 条是待办**（全在「低」档），
散在对话里会丢。清单（[memory-progress.md](../features/memory/memory-progress.md)）指向本文件。

**评审快照**（行号对应这些版本；评审期间工作区被另一会话改过一次）：
`index.js` md5 `27ae268e…`、`extract.js` `f786624d…`、`policy.js` `2fca8401…`、`run.js` `2fdb1501…`、
`sessionlog.js` `818e41cc…`、`routes.rs` `e547d15e…`、`jobs.rs` `0cda2adf…`、`bridge.rs` `ee9a0aec…`。

---

## 高

### 1. 切片里一条助手的话都没有 —— ✅ 已修（2026-10-15）

`extract.js::renderEvent` 取 `event.data.content`，而内核里 `assistant/message` 的正文在
`data.message.content`（依据：`dsh-session-query` 的 `extractSessionEventText`；`dsh-agent-loop`
append 的是 `{turn, step, message, usage}`）。只有 `user/message` 是 `data.content`。

**后果**：每次提取静默少一半内容、不报错、`dropped` 也不计；提示词写着「每行是 用户/助手」，
而 §8-5「未验证方案不能写成成功经验」因此失去可判据。

**修法**：按事件类型分支取正文；夹具改成**内核真实形状**，并加一条「不认识的形状就当没有正文，
别硬猜」的判据。（**Lead 复核过内核源码**，评审结论成立。）

### 2. 主路（落盘屏障 → 读句柄）永远走不到 —— ⬜ 待办

插件交给 `sessions.flush` 的是自拼的 `{ id }` 字面量，而内核 `SessionStore.flush` 用 `WeakMap`
按**对象身份**取 live entry（`dsh-session/lib/index.js`），非 live 对象必抛
`session "…" is not live in this store` → 每次都退到 live 兜底。

**后果**：`sessionPersistence.open/read/close` 这条被文档与 6 组用例钉住的主路在生产里是死代码；
§8-1 的「落盘屏障」实际未生效。文档里「真机验过 `persisted` 17/28」只可能是探针**直接**调
`readSessionRange`（拿真会话对象）得来的，插件这条接线拿不到。

**修法方向**：把**真会话对象**带进来（`session/event` 的首参，或 `services.sessions.get(sessionId)`），
id 只给桥用。附带：`index.js` 里 `{ session: { id: key.session }, ...key }` 的内层被 `...key`
立即覆盖，是死表达式。

### 3. 跨会话领取把候选记到**别人的项目** —— ⬜ 待办

`/claim` 不带 session，`claim_any` 从全表挑最早 pending；提交时 `workspace_id` / `workspace` /
`scope` 取自**发起领取的那个会话**的目录。两个会话同跑，或某会话留下 pending（上一任务还在跑 →
本回合领不到 → 下一个 `turn/end` 来自别的会话就会领走它）时，**A 的对话内容会带 B 的项目坐标落库**。

这正是 G3 要防的形状，而 G3 的用例只钉了「现解析」，钉不到「解析的是**谁**的目录」。

**修法方向**：任务键里冻结 workspace，或 `/claim?session=` 只领自己的。

---

## 中

4. **失败归因两侧各丢一半** —— ✅ 已修：`/jobs/fail` 现在报 `stage：reason`；
   `run.js` 的兜底 catch 也不再一律记成 `model`（用「当前步」变量）。
5. **部分提交失败被静默吞** —— ✅ 已修（2026-10-15）：只有「一条都没成」才 `ok:false`，`refusals` 直接丢；
   成功日志里的「被判据拒 M 条」用的是解析阶段那份，不含提交阶段。**修法**：refusals 的计数与
   首因进日志与结果。

   **修法（已落）**：`submit` 回 `{ submitted, refused, refusedReason }`，编排层带进结果，
   日志改成三个数分开报（**落库 N 条 / 解析阶段拒 M 条 / 提交阶段拒 K 条：首因**）。
   判据：一条「10 条里落 3 条、7 条被业务闸拒」形状的编排用例，断言 `refused` 与首因都回来了。
6. **逐字段限额与通道限额不合成** —— ✅ 已修（2026-10-15）：`MAX_ENCODED_BODY_BYTES` 只给标题/关键词/路径留了
   1000 字节，而中文标题（≤512 B ≈ 1530 编码）+ 12 条关键词（≈2300）就能吃光。评审实测：
   逐项合法的候选合计 **10181 B > 7000** → 被桥拒 → 整批提交不上 → 重试 3 次 → FAILED，
   而文案是「这条记忆太长了…请拆成更短的几条」（模型给的结果拆不了）。**修法**：按整条 path 预算。
   **修法（已落）**：`bridge.js` 新增 `encodedPathBytes(route, params)`——与 `buildPath` 同一个
   `URLSearchParams` 算法，所以「算出来的」就是「发出去的」字节数。提交循环在**每条候选**上真算一次，
   超了就把**这一条**记进 `refusals`（理由带真实数字：「编码后 N 字节，超过通道上限 7000 字节」），
   其余候选照常提交——不再整批打回。判据：一组复现评审那组数的参数（255 字标题 + 12 条关键词 +
   2000 字正文）必须被判超，且算出的字节数等于 `buildPath` 的长度；另钉住装配层真的在提交前算。

7. **每次读整份会话日志，且 `evidence_seq` 判据覆盖整个会话** —— 🟡 **判据那一半已修**（2026-10-15）：`from: 0` + `humanSeqs`
   取自全集，而不是喂给模型的那 64 KiB 切片 → 模型幻觉一个历史人类 seq 就能过；工具输出里的
   注入只要碰上任意历史人类 seq 就可升级成「用户偏好」。**修法**：把上一游标或切片首 seq 冻进
   任务键，作为 `from` 与 `humanSeqs` 的下界。

   **已落的那一半**：`evidence_seq` 的判据收窄到**模型实际看到的那一段**——
   `runExtraction` 把切片的 seq 集合（`buildSlice` 本来就返回 `seqs`）传给校验，
   `parseExtractionOutput` 只认这一段里的人类消息。这条把「工具输出里的指令不能升级成用户偏好」
   重新变成可判据的（原先覆盖整个会话，历史里总能找到一个人类序号）。
   判据：一条「切片外的人类 seq 必须被拒」的用例，**反向验过**。
   **还差**：读范围仍是 `from: 0`（每次读整份日志）——那要把上一游标冻进任务键，
   属字段级 schema 变更，与 C4/C5/H7 一起攒在「schema 变更窗口」里。
8. **`EXTRACTOR_VERSION` 两份** —— ✅ 已修：`policy.js` 改为从 `extract.js` 再导出（只留一份）。
9. **从不 renew，租约长度等于自己的时限** —— ✅ 已修（2026-10-15）：`LEASE_MS`（从壳发令牌起算）==
   `EXTRACT_TIMEOUT_MS`（从收到回执起算）→ 慢模型跑到 119 s 再提交时租约已过期，旧令牌的
   submit 与 fail 全部无效（`/jobs/fail` 返回 `retryable:null`，插件不读）→ 结果与失败原因一起消失。
   `/jobs/renew` 目前只有测试在调。

   **修法（已落）**：插件按 **45 秒**续一次租（壳的租约 120 秒，45 秒留足余量），
   `finally` 里停掉计时器；**续不到（`renewed === false`，说明已被接管）就提前 abort**——
   继续跑只是白烧一次模型调用。判据是装配层那几条（要真的调 `/jobs/renew`、要 `clearInterval`、
   要认 `renewed === false`）。`/jobs/renew` 从「只有测试在调」变成生产在调。
10. **`/submit` 的令牌判定与写入不在同一个临界区** —— ✅ 已修（2026-10-15）：`jobs::load`（不拿锁）→ 判 → 再
    `store::submit_candidate`（另一次 `store::mutate`）→ 读完、写之前租约到期并被接管时，
    迟到结果照样落库。**修法**：把判定搬进同一次 mutate。

   **修法（已落）**：把跨进程锁**提前拿到**整个 `/submit` 分支结束——判定与写入于是同处一个临界区。
   为此给 `WriteLock` 加了**同线程可重入**（`flock` 按文件描述符算，同进程再开一个描述符也拿不到，
   所以原先路由持锁时中间那次 `store::submit_candidate` 会把自己拒掉）：一个线程局部的层数计数，
   重入那一层不加锁也不释放；**别的线程照旧排队**（跨文件的写本来就该串行）。
   guard 用 `PhantomData<*const ()>` 钉成**不是 `Send`**——可重入计数是线程局部的，guard 被搬到
   别的线程去 drop 会让原线程的计数永远停在 1，之后它嵌套拿锁就再也不加锁了（静默失去互斥）。
   判据 `the_same_thread_may_reenter_but_another_thread_still_waits`（同线程嵌套立刻成功、
   内层 drop 后外层仍持有、**另一线程仍被拒**），而真桥那 15 条用例照常全过——
   那本身就是「重入真的让生产那条路走通了」的证据。
11. **去抖只作用于「合并」，不作用于「领取」** —— ✅ 已修（2026-10-15）：`claim` 无时间判据，任务入表即可领 →
    「先排、再领」在稳定态等于**每回合立刻一次提取**（一次全量日志读 + 一次模型调用）；
    代码注释里的「静默期/相邻回合合并」不成立。**修法**：`claim` 加
    `now - updated_at_ms < DEBOUNCE_MS` 跳过。（评审自注：机制从代码确证，但「4 回合 4 次」
    也可解释为各自回合领走，要实测就 30 秒内连发两回合看 claim 次数。）

   **修法（已落）**：新增 `claim_any_due_for(doc, now, session, due_only)`，**路由走它**
   （`due_only = true`）——入表后先过 `DEBOUNCE_MS` 静默期才领得到。**不改 `claim_for` 的语义**：
   它是租约状态机的原语（十来个用例按绝对时间喂它），去抖是调度策略，混在一起会逼那些用例
   为了凑静默期而改时间。判据 `a_fresh_job_is_not_claimable_until_the_debounce_has_passed`
   （静默期内领不到、差 1 毫秒也不行、到 `DEBOUNCE_MS` 领得到、`due_only=false` 保留旧语义），
   **反向验过**（去掉到期判据即红）。
12. **任务表没有任何可见面** —— 🟡 **日志那一半已修**（2026-10-15）：`/jobs/status` 与 `jobs::summary` 只有测试在调，
    UI/命令层无入口；`routes::jobs` 刻意不写 `shell_events`，插件 logger 又不进 stdout，
    stage 以前也不上报 → 用户问「这个会话为什么一条记忆都没提取出来」时，唯一可查处是手动打开
    `jobs.json`。**修法**：面板加「提取任务」卡片，或至少把 enqueue/claim/fail 计数落 `shell_events`。
13. **手动夹具的「基础设施失败」判据漏词** —— ✅ 已修（改**按 stage 判** + 一条逐字的环境允许项，
    并证明该允许项是必要的：加它之前夹具是红的）。

---
   **修法（已落一半）**：`routes::jobs` 现在用**另一个标签**（`memory-jobs`）留痕——
   `enqueue`（结局 added/merged/…）、`claim`（领到没领到、游标与尝试次数）、
   `finish`（ok / **stale-token**）、`fail`（**stage** 与 retryable）。
   进「查看日志」面板，**不进「最近注入」**（那是原来刻意不记的理由，仍然成立）；
   **只记路由与结局，不记正文、不记令牌**。`finish` 的 `stale-token` 与 `fail` 的 stage
   正是「为什么这个会话一条记忆都没提取出来」要看的两行。
   **还差**：面板上的「提取任务」卡片（把 `jobs::summary` 接到 UI）。

## 低

14. **`policy.js` 的 `buildInput` 是没人调用的重复实现** —— ✅ 已修（2026-10-15）：生产走 `extract.js::buildSlice`，
    `buildInput` 只有冒烟测试在调。两份形状不同（事件形状、截断标记、按字节 vs 按字符），
    而设计 §8-3 的「不静默截断」只实现在**死的那份**里；活的那份连 `dropped` 都不告诉模型。

   **修法（已落）**：把「不静默截断」**搬进活着的那份**（`extract.js::buildSlice` 现在在被丢弃整条
   对话时，会在切片最前面放一行「这段范围按字节上限截断了，前面还有 N 条对话没包含进来」），
   然后**删掉死的那一整簇**（`buildInput` / `cutBytes` / `eventText` / `MAX_INPUT_BYTES` /
   `MAX_EVENT_BYTES` / `TRUNCATION_MARKER` / `EVENT_TRUNCATION_MARKER`）——`policy.js` 从 568 行
   降到 492 行。原来那几组判据**改测活着的那条路**（只取对话、掩码在削短之前、截断留标记），
   反向验过（去掉标记即红）。
   附带修掉一个**共享夹具的形状错误**：测试里的 `human()` 用 `data.message.content` 造
   `user/message`，而内核里那一路的正文在 `data.content`（`data.message.content` 是**消息列表**
   那一路的形状）——正是第 1 条的形状，只不过这次出现在夹具里。
15. **`MAX_EVENT_BYTES` 名义字节、实际当字符数用** —— ✅ 已修（真的按字节削 + 判据）。
16. **指纹去重跨项目全局，且业务拒绝被当成提取失败** —— ⬜：A 项目已有逐字相同的一条时，
    B 项目模型提同一句 → 被拒 → 插件按 `ok:false` 处理 → 重试 3 次 → FAILED。
    **修法**：提取链把业务拒绝与基础设施失败分开（前者记账收尾、不重试）。
17. **每条候选的 `evidence_seq` 根本没传给壳** —— ⬜：壳按任务键写
    `source{start_seq:0, end_seq:cursor}` → 10 条候选来源范围一模一样，精确到句的引用丢失。
    §8-5①「来源引用」在库里没有落点（UI 暂时不显示这两个字段，所以看不出来）。
18. **关键词在壳侧完全不限长/限条** —— ⬜：插件的 `MAX_KEYWORDS`/`MAX_KEYWORD_BYTES` 只在自己
    那侧；`doc_bytes` 只统计标题+正文 → 关键词既不受单条限额也不进整库体积闸。属纵深缺口。

---

## 评审确认没问题的部分

- **跨进程写锁覆盖所有写路径**：两个 `mutate` 都在临界区内拿 `WriteLock`，各有「临界区里断言锁
  在手上」的接线用例；memory 模块的盘上写入只有那两条路径，无旁路。
- **租约状态机纯逻辑自洽**：过期接管、attempts 上限、旧令牌交不上、队满丢最老待跑、同键不重排、
  schema 更高拒写，边界用例齐（`LEASE_MS-1` / `LEASE_MS`）。
- **`bridge.js`**：路径长度预检、非 2xx → `{ok:false,reason}`、不跟随重定向、编码正确。
- **`parseExtractionOutput` 的机械判据本身正确**；`extractJson` 的配平扫描不会被字符串里的括号骗到。
- **`mask.js`**：三类形状与「空格不是分隔符」实现正确，与 Rust 侧键名表逐项对齐的判据存在。
- **失败不抛/降级**：`run.js` 不抛、空切片与空数组合法、关句柄失败不掩盖读结果、
  `resolveWorkspaceId` / `workingDirectory` 失败静默降级。
- **`memory.json` / `jobs.json` 的读纪律**（损坏不清空、0600 原子替换）没问题。

**评审没查**：真机跑（真模型/真桥/真内核会话）、`/tmp/p2probe` 的探针脚本与 sdk 会话原始证据、
UI 的其余部分、`query.rs` / `recent.rs` / `export.rs` / `workspaces.rs`。
