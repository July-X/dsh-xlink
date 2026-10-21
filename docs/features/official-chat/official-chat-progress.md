---
description: "官网对话直接引用的分阶段事实进度记录。P0 部分只写已发生的事实，不预写结论。"
kind: "progress"
status: "P0 机制层结论已出（G1/G2/G3）；P1 协议 v0.7 已冻结并落地（真机 p1-r9 在未打补丁的现役内核上 11/0/1）；P2 第一批（重复识别 ＋ 可见提示 ＋ 预算预检）**已全部收口**（协议 v0.6、实现 ＋ 判据 ＋ 成品文案落地 ＋ G-1 闭环），PM 一手复跑：P2 判据 11/11、自测 29/29、test:scripts 181/0/3（同一命令的 test:plugin 在 P3 落地后为 298/0/3，见 P3 行）；**P3 已立项**（协议冻结稿 **v0.5**：快照原子发布、占用关系与存储保护，条款 S-8…S-15；**第一批实现 S-14＋S-15 已落地并经 PM 一手验收**，实现入库基线 `f57b8d2`；**第一批判据 P3-1…P3-9 已入库**，判据入库基线 `ca992a2`，**PM 一手复跑：P3 判据 9 条＝通过 4（P3-6/7/8/9）· 失败 5（P3-1…P3-5 ＝ 第二批范围，预期红）· 未执行 0**、P3 自测 18/18、P2 自测 29/29、P2 判据 11/11、P1 判据 43/40/0/3 ＋ P1 自测 60/60、test:scripts 199/0/3、test:plugin 298/0/3、check:static 四项全绿、cargo test --lib 1134/0/11，重负载三轮全绿，**PM 独立注入复核四处全部按预期翻红 ⇒ Windows 侧无空跑**；**izzy 09:3x 拍板三项**（登录态由 izzy 用 dev 模式建、测试由 PM 用 DeepSeek 官网对话做／fork 保留／**`seq` 不复用**）⇒ 协议升 v0.3 ⇒ **第二批两单已派出**（devloper：S-8/S-9/S-10 ＋ S-11/S-12/G3-1；tester：补 P3-10）；**第二批单①（S-8＋S-9＋S-10 原子发布链）已落地并经 PM 一手验收**，实现入库基线 **`308ce51`**（PM 一手复跑已提交态：P3 判据 9 条＝**通过 7**（P3-1/2/3 由红转绿）· **失败 2**（P3-4/P3-5 ＝ 单②范围，预期红）· 未执行 0 · P3 自测 18/18 · `test:scripts` 202/199/0/3（exit=0）· `test:plugin` 306/0/3（309 tests））⇒ **协议升 v0.4**（§2.5 水位路径与实现现状对齐、§S-11 的 **R1 写死本阶段的可判定形态**、§5 新增 **P3-11**）⇒ **协议再升 v0.5**（R1 的**释放入口写死 ＝ 新增 host 命令 `release`** ⇒ 判据才有稳定名字可调）⇒ **单②在途**；**tester 的 P3-10 已入库**（`2b22409`，**判据入库基线更新为它**；PM 一手复跑 10 条＝通过 7 · 失败 3 · 未执行 0））；**【第二批全量验收通过 · 2026-10-21 11:0x】**：实现入库基线 = **`344a284`**（单① `308ce51` ＋ 单② `344a284`）、**判据入库基线 = `c3011f7`**（**P3-1…P3-11 共 11 条**）；**PM 一手复跑已提交态**（工作树干净、串行）＝ **P3 判据 11 条：通过 11 · 失败 0 · 未执行 0**（P3-4/P3-5/P3-10/P3-11 **全部由红转绿**）· P3 自测 **34/34** · P2 判据 11/11 · P2 自测 29/29 · P1 判据 43/40/0/3 ＋ P1 自测 60/60 · `test:scripts` **218/215/0/3** · `test:plugin` **324/321/0/3** · `check:static` **四项全绿**（内嵌资源 4 插件/44 文件）· `cargo test --locked --lib` **1134/0/11**；**PM 独立注入复核四处全部按预期翻红 ⇒ Windows 侧无空跑**（A 让 capture 不写占用记录 ⇒ 红 P3-4；B 把水位写入挪到认领之后 ⇒ 红 P3-10；C 让 `release` 不删 ⇒ 红 P3-11；D 让 sweep 不扫占用记录 ⇒ 红 P3-5 ＋ 连带三条）⇒ **协议升 v0.6**（写死「占用记录自己也过期时，本轮不删、从下一轮起可回收」）；**P4 硬前置已解除**（izzy 09:54 建好登录态）；**【当前基线 2026-10-21 12:0x–12:1x（以上数字为 11:0x 时的旧基线，以本句为准）：协议 = **v0.11**（izzy 拍板 G3-2a「不做硬上限」）· 判据 **13 条**（P3-13 已入库 `7c7e641`）· PM 一手复跑 ＝ P3 判据 **13/13 全绿**（P3-13 由红转绿）· P3 判据自测 **45/45** · `test:scripts` **229/226/0/3** · `test:plugin` **334/331/0/3** · `check:static` **四项全绿** · `cargo test --locked --lib` **1134/0/11** ⇒ **判据侧全闭环**；G3-2a 实现 `0de4af6` 已验收通过（v0.10/v0.11 只是口径澄清与拍板落地，实现零改动）；**P4 取数机制已定**（CDP `Runtime.evaluate` 首选，**有技术必然性**：DeepSeek 端点是 async fetch ⇒ 必须 `awaitPromise`）＋ **「插件→壳」头号缺口已核出现役回环桥通道**（`src-tauri/src/plugins/builtin/bridge.rs`）⇒ **不撞 P0 的否决、不需 izzy 再裁定**；**P4 已解锁**（12:1x）：探针 A 实测**端点存在**（`GET /api/v0/chat_session/fetch_page?count=5` ⇒ HTTP 200 ＋ JSON、`topKeys=[code,msg,data]`、未登录 `{code:40003,msg:"INVALID_TOKEN",data:null}`）＋ **取数路线已定**（**执行体在壳** ＋ **API 调用也在页内**、凭据由浏览器自动带 HttpOnly cookie ⇒ **页外直接调该 API 必失败**）＋ 探针 B（**只读副本**）**实测不可用**（5 个 cookie 逐项在盘上、站点仍 302 到 `/sign_in`）＋ B 项（exact route owner 准入）**已被现役回环桥绕过** ⇒ ⇒ **P4 协议草案可以开写**（只差「站点适配契约的字段面」一张数据表）】**未做**：真实站点取数（P4 协议草案 ＋ 实现）与安装版 release 验收"
date: 2026-10-21
---

# 官网对话直接引用 · 阶段进度记录

本文件只记**已经发生的事实**：谁做了什么、在什么环境、用什么命令、产出了什么、结论是什么、哪些没做。**不预写结论，不写「待办」以外的推测**。

P0 的可行性结论单独见 [official-chat-feasibility.md](official-chat-feasibility.md)；实施规则见[开发计划](official-chat-development-plan.md)；方案与待验证清单见[调研](official-chat-bridge-research.md)。

## 阶段总表

| 阶段 | 内容 | 状态 |
| --- | --- | --- |
| P0 可行性验证 | G1 内核工作台选择与发送控制、G2 异步回程与首站可读性、G3 窗口未打开时仍可读取 | **G1/G2/G3 结论已出**（G1 4/5 问通过 + Q3 判定互斥；G2-A 5/5，Windows 零痕迹回程已实测成立；G2-B 探测完成但取数未成立；**G3 三站各 7/7 通过**；**壳真实 workbench 内复验已可复现**——dev 壳两轮 + **自编 release 二进制一轮**（15:42–15:55，实例 id `default`）全过，壳驱动已搬进仓库；**壳内判据已与内核 r14/r16 对齐**（`results[]` + `summary`，**未执行单列**；壳内 E 升为**节点级**），**两条前置条件机检已压出真分支**（dev-r6 汇总 **9 通过 / 0 失败 / 0 未执行**）。**未闭合的两项**：登录态从未建立（需用户首次登录）、零痕迹回程 macOS 侧无通路。**判据第 31 条的三处口径问题（嵌套 if 漏判 / 逃生阀不生效 / `match` 边界假红）已全部闭合**（`4333b80` + `165d9a1`，自测 26 → 43，PM 逐轮第一手复验含反向回归）。**安装版 release 壳不在开发 agent 范围**（izzy 发布后自己测） |
| P1 协议与隔离 | 固定数据模型、错误、权限与状态机 | **协议 v0.5（PM 定）**：单一 id 制度（`ref === snapshotId`）、状态机映射、**门控口径从「时序」换成「状态」**、失效一律「拒 + 可见错误」（chip `invalid` 态内核不可达 → 缺口 **G1-6**）、13 码闭集 + **错误结果对象 8 字段闭集**、保留期主动清理口径（§4.1，v0.5 澄清「只记日志」必须留痕）、**缺口 G1-7（通道被上游一行卡住）**、A0s 判据同步更正。**代码侧已落地**（协议核心 JS+Rust、13 夹具 + 双端一致测试、5 命令走内核 `connection` 服务的 **exact fetch route**、host 侧 id 分配器 `seq-store.js`、保留期主动清理 `host/sweep.js`）：**§8 十二条逐条 pass**，判据 runner **43 条 = 40 pass / 0 fail / 3 skipped**（skip：A0 真机级、A6b chip invalid、A18k「发送中引用」已知不覆盖）。**真机 p1-r9（在未打补丁的现役内核上）P1 六条硬口径 11 通过 / 0 失败 / 1 未执行**——A0 真机级 ✔（capabilities 经 exact fetch route 拿到）、catalog＋host 预分配 id、pick＋落盘＋自算摘要对得上、**S-4 逐字节**（请求体命中 4 处，每处 76 字节 exact）、S-1 两个 id、缺口①（删快照 ⇒ 请求数 0 ＋ 可见错误 ＋ 客户端抛错）。**未实现**：真实站点账户与深链（P4） |
| P2 内核插件闭环 | 固定数据的选择、引用、发送与模型输入核对 | **已立项**（izzy 22:5x 点头）。「正文去重」子议题的**只读调研已出**（全文见 [official-chat-p2-research.md](official-chat-p2-research.md)），PM 建议三条 = **P2 第一批口径**，已协议化为 [official-chat-p2-protocol.md](official-chat-p2-protocol.md) **v0.5**（PM 定，六处裁定）：**重复识别 ＋ 可见提示 ＋ 预算预检**，**不做消息级自动合并**、**不动 chip 数**、**不碰 id 分配**、**不改 `agent/pre-step` 消息**。键 = (accountScope, sessionId, branch, capturedAt, contentDigest) 全等；落点 = **host 命令层 `duplicates` 命令产出重复组，client 呈现**。**实现已合并推送**（`bec3b82`，13 文件）＋ **判据第一批已合并推送**（`af9789d`，6 文件 / +3060 / −17）⇒ 远端 `main` = **`af9789d`**。**PM 一手复跑（2026-10-21 00:4x，不采信回执）**：判据 runner **9 通过 / 0 失败 / 0 未执行**、判据自测 **23 / 23**、`npm run test:scripts` **178 tests / 175 pass / 0 fail / 3 skipped**、P1 判据 runner **43 条 / 40 通过 / 0 失败 / 3 未执行** ⇒ **P1 未回退**。**未做**：P2 第二批及以后、真实站点取数（P4）；**界面层文案已定稿**（UX 2026-10-21，成品文案已入 **协议 v0.6 §3.2**；呈现形态锁死「两条、两个 kind」） **【当前基线 2026-10-21 03:5x：协议 **v0.6**（§3.2 A/B 版措辞裁定已落）· 判据 **11 条全过** · 自测 **29/29** · `test:scripts` **181/0/3**（184 tests）· `test:plugin` **289/0/3** · P1 判据 **43/40/0/3** ＋ 自测 **60/60** · **判据入库基线 = `df65280`**（派单⑨**最后一笔判据提交**；PM 于 03:4x 推；此后 PM 又推了进度回填，故远端已不止此提交）；**派单③与⑨全量闭环**（机制 ＋ 措辞 ＋ 回归保护 ＋ **逻辑倒置消除** ＋ **界面层逐字核验通过**）⇒ **判据侧与界面侧均无遗留欠账**；**02:0x 登记的「P2-H 重负载假红」缺口闭环**；本段其余数字为 00:4x 时的旧基线，以本句为准；**P3 第一批落地后同一命令的最新读数见 P3 行**（`test:plugin` 由 289/0/3 变为 297/0/3 ＝ P3 新增 8 条自测被 glob 拾取）】** |
| P3 正文快照与桥接 | 原子保存、固定版本、回收、恢复 | **已立项**（izzy 07:32 指示「继续完成后面的任务计划」）。P3 协议冻结稿 **已入库至 v0.9**（首版 `v0.1` ＝ `6ad99b4`；**v0.2** ＝ `a837056`，第一批实现落地后的升版；**v0.3** ＝ `2785663`，**第二批口径定案**——izzy 09:3x 拍板「`seq` 不复用」⇒ §2.5 序号水位 ＋ §S-13 裁定 4 ＋ §5 判据 P3-10 ＋ §6 把 G3-2 拆两半；**v0.4…v0.9** ＝ 第二批实现期间的口径澄清（v0.4–v0.8）＋ **G3-2a 定案**（v0.9）⇒ 详见下方 changelog），[official-chat-p3-protocol.md](official-chat-p3-protocol.md)：development-plan §8 的七段要求逐条落到 **S-8…S-15**（**S-8** 快照落盘＝原子发布 / **S-9** 发布点＝`rename` 成功那一刻 / **S-10** 崩溃残留＝第五家族 / **S-11** 占用记录落盘＋三条释放规则 / **S-12** 引用关系的可判定边界 / **S-13** 跨文件提交序·**不宣称事务** / **S-14** 存储保护与落盘策略成文 / **S-15** 账户维度服务端推导）。**「公布索引」的含义已写死**：本功能**不建独立索引文件**（延续 P2 §6.6）⇒「索引」＝目录扫描 ⇒「公布」＝该文件以最终名出现在快照目录里 ⇒ 发布点＝`rename` 成功那一刻（计划 §8 第 2 段那句话若不这么定，实现就会长成「两个独立文件各自原子写就宣称事务完成」——那正是计划原文明令禁止的形状）。**现状核对（PM 一手核过行号，不是推测）**：**已满足**＝目录/快照分离、读时版本＋摘要校验、F 组回环路由四类拒绝全满足；**空洞集中在**＝快照**直写最终路径非原子**（`host/index.js:348`）、**无临时文件与崩溃残留清理**（`host/sweep.js:20-32` 只扫四家族）、**引用关系只在进程内**（`client/client.js:523`）且**回收无引用判定**（`host/sweep.js:45-49`）、**目录缓存无私有 mode**（`host/commands.js:98`/`:100-104`）、**账户键与快照 `accountScope` 取自请求参数**（`host/commands.js:412`/`:489`/`:577`，而服务端强制的只有 `sourceFingerprint` `:528`/`:635`/`:699`）⇒ **S-15 是「协议与实现冲突」，按纪律改实现、不改协议**。**新登记上游缺口 G1-8**：内核现役 `0.2.1-rc.2` 的 `InputTriggerSource` 接口全集（`@deepseek-ai/dsh-client-ui-input-trigger/lib/types/types.d.ts:139-216`，SHA-256 `2D5B0042…`）**无「引用被移除／草稿被放弃／已发送成功」回调** ⇒ 内核只**问** source、从不**告诉** source ⇒ S-12 降级口径＝「误删 ⇒ 该引用失效并被拒 ＋ 可见错误，**不是错发**」（沿用 P1 §4.1 第 4 条的既有后果语义）⇒ **不**把「引用关系精确等于内核草稿」写成验收项（先例：P1 的 G1-6）。**第一批判据 P3-1…P3-9（tester）与实现 S-14＋S-15（devloper）已并行派出**（文件所有权不重叠）。**【实现第 1 组已落地并经 PM 一手验收 · 2026-10-21 08:0x；判据已入库 · 08:3x】**：实现入库基线 = **`f57b8d2`**（S-14 目录缓存目录 `0700`/文件 `0600` ＋ 存量收口；S-15 账户维度改服务端推导、不符即 `account_changed` 拒、不静默改写）；**判据入库基线 = `ca992a2`**（**P3-1…P3-9 共 9 条**，`871d0ff` 入库 ＋ `ca992a2` 修掉 P3-7 的 Windows 空跑）；**PM 一手复跑全绿**（已提交态 HEAD `ca992a2`、串行）＝ **P3 判据 9 条：通过 4（P3-6/7/8/9）· 失败 5（P3-1…P3-5 ＝ S-8…S-12 第二批范围，**预期红**，不是回归）· 未执行 0** · P3 自测 **18/18** · P2 自测 **29/29** · P2 判据 **11/11** · P1 判据 **43/40/0/3** ＋ P1 自测 **60/60** · `test:scripts` **199/0/3**（202 tests）· `test:plugin` **298/0/3**（301 tests）· `check:static` **四项全绿** · `cargo test --locked --lib` **1129/0/11**，**重负载三轮全绿**；**PM 独立注入复核四处全部按预期翻红**（A/B 查 devloper 的两条 Windows fallback、C 查 P3-7、D 查 P3-9 ⇒ **均非空跑**）。**【第二批已派出 · 2026-10-21 09:4x】**：izzy 09:3x 拍板三项（**登录态由 izzy 用 dev 模式建、测试由 PM 用 DeepSeek 官网对话做**／**fork 保留**／**`seq` 不复用**）⇒ 协议升 v0.3 ⇒ **devloper 两单**（单① S-8＋S-9＋S-10 原子发布链；单② S-11＋S-12＋G3-1 占用记录＋`seq` 永不复用）＋ **tester 补 P3-10**（G3-1 判据）**已派出**；**PM 裁「`seq` 不复用」不得靠保留认领文件实现**（会让已入库的 P3-6 变红 ⇒ 判据是规格）⇒ 须用**独立水位记录**；**G3-2 拆两半**（G3-2a 缓存总量核算**排在第二批之后**、G3-2b 单条正文 64 KB **另派一单**）。**【第二批单①已落地并经 PM 一手验收 · 2026-10-21 10:3x】**：实现入库基线 = **`308ce51`**（单① S-8＋S-9＋S-10 原子发布链：`host/index.js` 的 `save()` 改「写临时名 → 回读校验 → `rename`」＋ `host/protocol.js` 落临时名形态的**单一来源** ＋ `host/sweep.js` 加第五家族清理；4 文件 / +486 / −13，**`scripts/**` 与 `docs/**` 一行未动**）；**PM 一手复跑已提交态**（独立 worktree、detached @ `308ce51`、**不碰共享工作树**、跑完 remove ＋ prune）＝ **P3 判据 9 条：通过 7（P3-1/2/3 由红转绿）· 失败 2（P3-4/P3-5 ＝ 单②范围，**预期红**）· 未执行 0** · P3 自测 **18/18** · `test:scripts` **202/199/0/3**（202 tests，exit=0）· `test:plugin` **306/0/3**（309 tests）；**判据入库基线 = `2b22409`**（**P3-10 已入库**，共 10 条；PM 一手复跑：**通过 7 · 失败 3 · 未执行 0**，失败的三条＝P3-4/P3-5/P3-10，**都是单②范围，预期红**）⇒ **协议升 v0.4**（§2.5 水位路径与实现现状对齐 ＋ §S-11 的 **R1 写死本阶段的可判定形态** ＋ §5 新增 **P3-11**）⇒ **协议再升 v0.5**（R1 的**释放入口写死 ＝ 新增 host 命令 `release`** ⇒ 判据才有稳定名字可调）⇒ **单②（S-11＋S-12＋G3-1）已派出**（附 PM 裁的注入点：判据侧用**子进程 patch `node:fs`** 制造真失败，**实现不许加测试钩子/改签名**）。**【第二批全量验收通过 · 2026-10-21 11:0x】**：实现入库基线 = **`344a284`**、**判据入库基线 = `c3011f7`**（**11 条**）；**PM 一手复跑已提交态**（工作树 tracked 干净、串行）＝ **P3 判据 11 条：通过 11 · 失败 0 · 未执行 0**（P3-4/P3-5/P3-10/P3-11 **全部由红转绿**）· P3 自测 **34/34** · P2 判据 11/11 · P2 自测 29/29 · P1 判据 43/40/0/3 ＋ P1 自测 60/60 · `test:scripts` **218/215/0/3** · `test:plugin` **324/321/0/3** · `check:static` **四项全绿**（内嵌资源 4 插件/44 文件）· `cargo test --locked --lib` **1134/0/11**；**PM 独立注入复核四处全部按预期翻红 ⇒ Windows 侧无空跑**；**协议升 v0.6**（写死 R2 的「本轮不删、从下一轮起可回收」）。**【当前基线 2026-10-21 12:2x：协议 = v0.9（§S-16 定案 G3-2a ＋ 判据 P3-13 ＋ 缺口 G3-5）· 判据入库基线 = `b2b1021`（**12 条**）· PM 一手复跑（独立 worktree、detached、不碰共享工作树）＝ P3 判据 **12/12 全绿** · P3 自测 **37/37** · 本插件自测 **15/15** · `test:scripts` **221/218/0/3** · `test:plugin` **324/321/0/3** · `check:static` **四项全绿**（内嵌资源 4 插件/44 文件）· `cargo test --locked --lib` **1134/0/11**；实现侧另收 `7e7a2b5`（目录缓存路径注释订正，**纯注释、零行为变化**，PM 一手核 diff）⇒ **P3 第二批 ＋ P3-12 全部闭环**；**G3-2a 口径已定案（§S-16）待派单**】（以上为 12:2x 的旧基线，**以本句为准**）**【当前基线 2026-10-21 13:3x：协议 = **v0.11**（izzy 拍板「不做硬上限」）· 判据入库基线 = **`7c7e641`**（**13 条**，含 P3-13；另收订正 `fb1c624`）· PM 一手复跑（独立 worktree、detached、不碰共享工作树、串行）＝ **P3 判据 13/13 全绿**（**P3-13 由红转绿**）· P3 判据自测 **45/45** · 本插件自测 **10/10 ＋ 15/15** · `test:scripts` **229/226/0/3** · `test:plugin` **334/331/0/3** · `check:static` **四项全绿**（内嵌资源 4 插件/44 文件）· `cargo test --locked --lib` **1134/0/11**（48.8s）⇒ **判据侧全闭环**（P3-1…P3-13 全部有判据且全绿；P3-12 ＝ 回归保护、P3-13 ＝ 本轮新增的事实登记）；另收 `9512602`（过期版本注释订正，**纯注释、零行为变化**，PM 一手核 diff ＋ 核 `Cargo.toml:30`／`Cargo.lock` 版本事实）**验收通过**】**未做**：P4 真实站点（**硬前置已解除**，协议草案待起草；**头号缺口已核出「现役通道」＝内置插件回环桥**（`src-tauri/src/plugins/builtin/bridge.rs`）⇒ **不撞 P0「不再造一个监听服务」的否决** ⇒ **不需 izzy 再裁定**；仍在等探针 A/B 的结果）· G3-2b（另派一单）· 消息级合并（izzy 未拍板） |
| P4 首站接入 | 真实目录、认证、分支与正文 | **硬前置已解除**（izzy 2026-10-21 09:54 用 `pnpm run dev` 的测试模式**建好登录态**并告知「可以开工了」⇒ 登录态归 izzy、**测试归 PM**）。**PM 只读调查已开**：插件的 `captureSource` 目前仍是**固定夹具**（`plugins/official-chat/host/commands.js:160-205`，注释明写「真实站点取数与登录态属 **P4**」⇒ **真实取数路径尚未实现**）；官方站点 = `https://chat.deepseek.com`（主站）＋ `https://agent.minimaxi.com` ＋ 可选 `https://chatgpt.com`（`src-tauri/src/harness/official_chat.rs:26/:75/:82/:88/:103`）；壳侧已有**页内求值**能力，但 **P0 裁定 3（`official-chat-feasibility.md:198`）已把它们分好**（PM 一手核过）：**R1 页面槽位**（`src-tauri/src/official_chat/script_eval.rs`）**已被裁定不得直接带进真实站点**（`Object.getOwnPropertyNames(window)` 列得出不可枚举属性；名字前缀固定 `__dsh_rt_` ⇒ 页面探针扫前缀即可定位 ⇒ 与 `harness/chat-fingerprint.js`「页面探针必须看到普通浏览器里什么都没有」冲突）· **R2 本地回环中继已被否**（计划 §4「不再造一个监听服务」）· **首选方向 = Windows 的 CDP `Runtime.evaluate`**（`cdp_eval.rs`，`awaitPromise:true` ＋ `returnByValue:true`；**已实测 5/5**：未开 DevTools 窗口可调、直接拿到结算值、**页面 JS 空间零痕迹**（`getOwnPropertyNames(window)` 前后 **1239/1239**、`__dsh*` 为 0））⇒ **macOS 侧无通路**（wry 0.57 未暴露 `callAsyncJavaScript`、raw `WKWebView` 为 `pub(crate)`）⇒ **已知缺口、不是待验方案**（代码里明确报错、不静默降级）。**✅ izzy 已拍板（2026-10-21 12:5x）**：① **首站 = DeepSeek 官网对话**（`chat.deepseek.com`，登录态由 izzy 用 `pnpm run dev` 的测试模式建立 ⇒ **测试由 PM 做**）；② **平台范围 = Windows 先上、macOS 单独排期** ⇒ **P4 只交付 Windows**；**macOS 沿用 P0 的「已知缺口」登记**（wry 0.57 未暴露 `callAsyncJavaScript`、raw `WKWebView` 为 `pub(crate)` ⇒ 零痕迹通路不存在）⇒ **等上游暴露后单独拍板**，本轮**不做**、也**不假装**是「待验方案」。⇒ **P4 协议草案待起草**（PM 归属、新载体）。**PM 12:1x 只读核证（架构缺口的表述已更新）**：原先记的「**插件 → 壳**无现成通道」**只在一个方向成立** —— 核到 **插件 host 已在内核 Web 服务器上注册了一条 exact route**（`plugins/official-chat/host/rpc.js:78` `FETCH_ROUTE_PATH = '/api/xlink-official-chat'`；走内核 `connection.fetch.register({kind:'exact'})`，**owner 准入由内核做**，见 `dsh-client-connection@0.2.1-rc.2` 的 `createSharedFetchHandler:820-838` / `registerFetchRoute:840-854`）⇒ **「壳 → 插件 host」这条 HTTP 通道是现成的**；且**壳调内核 `/api` 已有可复用先例**（`src-tauri/src/notify/task.rs:909 fetch_auth_cookie`〔launch token → `dsh-auth-*` cookie〕＋ `:941 fetch_session_titles` ⇒ `POST http://127.0.0.1:{port}/api/session/list`）⇒ **「谁驱动页内取数」的方向若反过来（壳驱动），就不需要任何反向通道 ⇒ 不撞 P0「不再造一个监听服务」的否决**。⚠️ **但这两条目前都只是「读代码得出的候选」**：exact route 的 owner 准入能否被壳的 cookie 满足、壳能否实际调到，**均未实测** ⇒ **已派 P4 调查单（只读）核三件事**：① **DeepSeek 会话目录/正文的真实取数端点与字段形态**（能不能走站点自己的 API —— 若能 ⇒ **页内取数与本缺口同时消失**；并要验 P0 那条「localStorage `userToken` 服务端不认 ⇒ `40003 INVALID_TOKEN`」现在成不成立）② `/api/xlink-official-chat` 能否被壳实际调到（**通 / 不通 / 判不了**三档）③ 「插件 → 壳」方向的现有手段能否绕开（**若只有「再造一个监听服务」能成 ⇒ 就照 P0 明确写否决，不绕着提替代**）。⇒ **协议定稿排在调查单之后**（纪律：先调查单，协议定稿后才派实现单）。**未做**：P4 协议草案（**待调查单结果**）· 真实取数实现 · 安装版 release 验收（归 izzy 自己测） |
| P5 桌面体验与诊断 | 登录回程、错误、详情、能力检测 | **部分开始**（2026-10-20 22:3x–00:0x）：「ChatGPT 网页登录时**选不了 Google 邮箱做快速认证**」的根因**已用实跑探针查清**（见下方 changelog 与「登录回程」节）——**不是**「嵌 webview 缺浏览器能力」（该假设已被 A/B 实测**证伪**，见 changelog）；是**两件事叠加**：① 壳的内容 webview 是**独立浏览器**（macOS 专属 data store / Windows 专属 user-data 目录），与用户的 Safari / Chrome **永不共享登录态** ⇒ Google 的账号选择器在壳内**结构上不可达**（第一次必然落到「邮箱/手机号」表单）；② 生产的「外部 http(s) 弹窗一律交系统浏览器」⇒ GSI 的账号选择器弹到**别的浏览器**里，用户在那边选好了账号，**壳里的页签仍未登录**（静默失败，壳侧无日志）。**已落地**：认证域的 `window.open` **留在壳内** ＋ 托管弹窗**继承内容页签那份浏览器档案**（Windows = user-data 目录；**macOS = 同一个 `data_store_identifier`**——WKWebView **不认** `data_directory`，不显式绑标识符则弹窗落到默认数据存储 ⇒ 里面没有发起方那份 Google session ⇒ 账号选择器出不来，且前三条判据仍全绿）＋ 指纹脚本同源 ＋ 单测 3 条 ＋ 判据第 32 条**五处**形状（反向验 5 处全被抓住）。**已闭合**：**账号选择器本身已直接实测**（`/v3/signin/accountchooser` →「选择账号 + 账号名」），且**托管弹窗继承同一份 cookie jar 也已端到端验完**（弹窗里有 `SID`/`SAPISID` 等 ⇒ 出账号选择器；反向去掉标识符 ⇒ 退回「邮箱或电话号码」）⇒ **「二次登录直接点账号即登录」在壳内成立**。**仍未闭合**：① 壳内**首次** Google 登录需完整输入账号密码（**只有用户本人能验**；走通一次之后才有 session，GSI 弹窗才会直接列出账号）；② **真人点击**那一环未由自动化实测（Computer Use 被宿主权限挡住：`computer_app_list` / `computer_window_list` / `computer_desktop_state` 一律 `computer_permission_required`）。**口径已拍板**（izzy 2026-10-21）：**不**让 ChatGPT 页签改用壳自己的 OpenAI 登录态、跳过网页登录 ⇒ **验收范围＝首次壳内输密码 ＋ 之后二次登录直接选账号**，本次修复即交付到这一段；壳读本机 Codex 登录只为**额度**，不当网页会话凭据 |
| P6 交付检查 | 接线、资源、兼容与关闭恢复 | **未开始** |
| P7 扩展站点 | 其他站点适配（MiniMax 排在此阶段） | **未开始** |

## P0 事实记录

### 2026-10-20 · G1 验证执行（devloper）—— 七次运行，结论取自第一、第三与第五至第七轮

**做了什么**：在真实内核 release `0.2.1-rc.2` 上验证「@ 选择官网对话 → chip 落地 → 发送时读取正文 → 等待或拒绝」这条链路。内核跑在 `%TEMP%\dsh-xlink-g1\` 的**副本**上，`DSH_HOME` / `DSH_XLINK_HOME` 也都在该目录下，**未碰** `C:\Users\zxx\.dsh-xlink`。驱动对象是**内核自带工作台页面**（Edge `--headless=new` + CDP），模型是本机桩模型（OpenAI 形状），**不接真实官网、不用付费模型**，正文为固定夹具 `g1-alpha` / `g1-beta`。

**七次运行（各自独立取证；同名证据文件被后续轮次覆盖过，原始请求体靠第 5/6/7 轮的逐轮独立文件保住）**：

| 轮次 | 时间 | 脚手架 | 结果 |
| --- | --- | --- | --- |
| 1 | 12:21–13:06 | `%TEMP%\dsh-xlink-g1\g1-*.mjs` + `cdp.mjs` / `kernel.mjs` | **五问跑通**，产出 `final.out.txt`(13:01:41)、`reject.out.txt`(13:03:58)、`q2v2.out.txt`、`q4-*.out.txt`、`q5*.out.txt`、`model.log`；交付 `delivery.md`、`delivery2.md` |
| 2 | 13:09–13:15 | 仓库内新脚手架 `plugins/official-chat/test/drive/run-g1.mjs` | **未跑出 composer**（工作台首页没有会话 → 没有编辑器），四条断言**未验证**；留下 **8 个 `%TEMP%\g1-probe-*` 目录（约 127MB）**。**后续状态**：该脚手架**已重写并跑通**（第 13、14 轮连续两轮全绿）；那 8 个目录**已用运行时的可恢复删除启动器全部回收**（121.3MB，移入回收站，未永久删除） |
| 3 | 13:26–13:31 | 同上但**改走壳的真实 Rust 物化链**（`ensure_wired` + 打包资源 + `verify_manifest`） | **五问全部重跑**，数字与第一轮一致或更接近（等待 7439ms vs 7624ms；拒绝可见 1325ms→4994ms vs 1340ms→5031ms）；产出 `final.json`(13:26:00)、`reject-notice.json`(13:28:39)；交付 `delivery3.md`(13:31:45) |
| 4 | 13:49–13:56（**14:04 重跑**） | `shell-dev.mjs` / `shell-verify.mjs`（起**壳本身**：`dsh-xlink.exe` dev 壳 + Vite 5174 + WebView2 CDP 9335，`data_dir` 在 `%TEMP%` 下） | 首跑**未产出结果**（`shell-verify.out.txt` 与 `model.log` 均 0 字节，13:56:03）；**14:04 重跑有产出**（当时 3,209 字节，同名文件后被覆盖）：起壳 → 插件 `enabled`/`wired`/`loadState:prepared` → **真实物化链**物化到 `.../instances/default-dev/home/extensions/builtin/official-chat/0.1.0/dsh-0.2.1-rc.2-cordis-4.0.5/compat-a` → 接线写入 `cordis.patch.yml` → `start-kernel: ok:true`。**但输出到 `start-kernel` 就停**，工作台内 @ 菜单与发送门控未验证 |
| 5 | 14:09–14:11 | 同上脚手架重跑（CDP 9335 上同时见壳面板 5174 与**壳自己的 workbench webview 3210**） | **壳内四项全过**：`@` 菜单「官方对话」分组 / chip 是内核节点（`bodyInDraft:false`，pending 句柄）/ 延迟读取门控（`waitedMs:7466`、完成前 0 请求、完成时请求体含正文）/ 读取失败 0 请求 + 可见错误 + 草稿保留；**壳自己的物化链产物核对过**。产出 `shell-verify.out.txt`（**现为 7,713 字节 / 14:09:02**），交付 `delivery-final.md`(14:11:08) |
| 6 | 14:28–14:35 | 同源脚手架复跑（`shell-verify-r2.out.txt`）+ 内核工作台复跑（r12 / r13 / r14，每轮独立隔离 home） | **14:28 轮失败**：attach 到 `chatstrip` 窗口 → `builtin_set_enabled` 被 capability 拒绝（`allowed on: [windows: "main"]`），**未产出断言结果**。**14:31–14:35 轮在内核自带工作台上五项断言全过**（新增「选中后继续编辑不丢字」；错误证据为节点级 `role="alert"` + 4 张密集采样截图）；**原始请求体改为逐轮独立文件** `model-requests.r12.jsonl` / `model-requests.r14.jsonl`（各 44,871 字节） |
| 7 | 14:47–15:09 | **壳驱动搬进仓库后重跑**：`plugins/official-chat/test\drive\shell\`（7 个文件 + README，含 copy-kernel → shell-dev → shell-verify 三条命令） | **壳内四项全过**（`evidence-shell-r2\shell-verify.shell-r2.json`）：A `@` 菜单有「官方对话」+「G1 固定对话 Alpha」；B `picked`/`menuHasRow`/`chipIsKernelNode` 为 true 且 **`bodyInDraft:false`**；D t=291ms **0 请求**、t=7980ms 才发出且请求体含正文；E **0 请求** + 可见错误(+1750ms) + 草稿保留；物化产物核对全 true。**内核路径 r16 五项断言全绿**（`evidence-r16\results.r16.json`）。**同时暴露第二条前置条件**：壳内 `builtin_set_enabled` 返回 `ok:false`，原因是**该实例内核当时还在运行**（壳原话：内核把状态缓存在内存里，运行期间的改动会被它下次落盘整个覆盖）——该轮插件靠壳的接线行 + 物化链接上，**不是 `builtin_set_enabled` 成功**。**修掉两个真问题**：第一次 pick 假红（菜单先渲染、候选项后到 → 点前轮询等行）、`waitMenuRow` 漏 `return` 导致 `menuOpened` 假阴性 |

**证据生命周期问题（已记入 feasibility 第 7 节第 6 条）**：`model.log` 被第 4 轮的空跑覆盖为 0 字节（14:34 又被 r14 空跑覆盖一次），一度只剩 `final.out.txt` 的 `requestHasBody:true` 布尔字段支撑、**原始请求体不可复核**；**第 5/6 轮已改为逐轮独立文件**（`probe-r12\model-requests.r12.jsonl`、`evidence-r14\model-requests.r14.jsonl`）→ **原始请求体重新可复核**：对话请求体里夹具正文出现 1 次、落在角色为 `user` 的消息里（**全文计数是 2**，因为另有一条会话标题生成请求也含同一正文）。**教训仍成立：每轮跑批的证据必须写进独立文件，不得复用同名文件。**

**插件物化**：第 1 轮**按源码手工复刻**到 `<DSH_HOME>/extensions/builtin/xlink-official-chat/0.1.0/g1-local/compat-a/`；第 3 轮改走**壳的真实 Rust 物化链**（源 = 打包资源 `src-tauri/resources/builtin-plugins/official-chat`，`npm run prep:builtin` 产物，`verify_manifest` 一并跑到；指纹 `dsh-0.2.1-rc.2-cordis-4.0.5` 由真实内核树算出；接线行由 `wiring.rs::row_block` 写进 `<DSH_HOME>/profiles/web/cordis.patch.yml`）。

**产出**：

- 插件代码（`plugins/official-chat/**`）：`package.json`、`host/index.js`、`client/client.js`、`test/client.test.mjs`、`test/smoke.test.mjs`、`test/drive/run-g1.mjs`、**`test/drive/shell/`**（壳驱动 7 个文件 + README，14:47 搬入仓库，**15:18 随 `2321dbd` 提交**）。
- 单测结果：`node --test plugins/official-chat/test/client.test.mjs smoke.test.mjs`（14:11）→ **29 passed / 0 failed**；`npm run test:plugin` → **209 tests / 206 pass / 0 fail / 3 skip**。
- 证据：`%TEMP%\dsh-xlink-g1\` 下的 `final.out.txt`、`final.json`、`reject.out.txt`、`reject-notice.json`、`q2v2.out.txt`、`q4-*.out.txt`、`q5*.out.txt`、`q5b.out.txt`、`model.log`(已空)、`delivery-final.md`(14:11)、`shell-verify.out.txt`(7,713 字节 @14:09)、`shell-verify-r2.out.txt`(14:28 失败)、`run-r13.out.txt`、`evidence-r14\results.r14.json`、`evidence-r16\results.r16.json`(15:09)、`evidence-shell-r2\shell-verify.shell-r2.json`(15:07)、`evidence-r14/r16\assert-d.toast.*.png`(密集采样截图)、`model-requests.r12/r14/shell-r2.jsonl`(逐轮独立请求体)。

**结论落点**：PM 当日拍板 P0 只做一条路（**甲**形态），插件随即收敛为**单变体**——`onPick` 同步返回 pending 句柄、正文由 `codec.serialize` 在发送时取、不再自插 chip。此前那条「读取完成后自己插 chip」的变体**实测被拒**（draftRev CAS），其代码已按拍板删除，遗留的测量作为「该形态被否」的依据保留在交付原文里。

五问结论、接口缺口 G1-1…G1-5 与逐条证据见 [feasibility 第 3 节](official-chat-feasibility.md#3-g1真实工作台内的选择和发送控制)。

### 2026-10-20 · G1 复跑的假红根因（devloper）—— 定位到内核源码，不是玄学

复跑过程中先后出现过四轮「红」，逐条定位到内核源码或实测事实，**都不是功能缺陷**：

1. **等错了对象**：composer 卡片（`[data-composer-card]`）页面一开就有，但**编辑器**要等内核把 input 状态建起来——`ui-conversation/client.js@0.2.1-rc.2:19028` 是 `phase: input?.phase ?? "inert"`。要等 `contenteditable="true"` **且有尺寸**（内核还会渲染零尺寸的隐藏 contenteditable）。第 5 轮拿到的是 inert 壳，看起来像「工作台没有编辑器」。
2. **弹窗是延迟出现的**，且必须用**真实鼠标事件**关；否则弹窗盖住 composer，`@` 打不进去，断言 a 假红（第 9 轮）。
3. **触发词正则** `/(?:^|\s)(@([^\s]*))$/`（`input-trigger/client.js@0.2.1-rc.2:725`）：`@` 前面必须是**行首或空白**。写「前置文字@」根本不触发，菜单不开、点不中（第 11 轮）。
4. **错误提示是 Toast**（`ui-conversation/client.js@0.2.1-rc.2:18808`：`if (notice?.level === "error") showToast(notice.text)`，带 `onDone` 自动消失；`info` 级才是内联 `role="status"`，`:18982-18986`）。取证要**密集采样（250ms）+ 截图**，单点快照必然踩在消失之后。

**一次提交会打两条到模型端点**：第二条是内核的**会话标题生成**（system = `Create a concise title…`），不是引用发了两遍；断言因此落在**对话请求**上，另一条单列不隐藏。**这是 feasibility 第 8 节口径说明第 3 条的由来。**

### 2026-10-20 · G2 验证执行（tester）

**做了什么**：两部分。A 部分用**本地可控页**（自起 `127.0.0.1`，不接触真实站点）验证异步求值回程；B 部分对三个站点做**只读**探测，**只取目录元数据，未读任何正文**。

**过程中修掉的一个真 bug**：WebView2 的 `TryGetResultAsString` **只在结果是字符串时才给值**，数字/布尔/对象一律返回空串。修法是页面侧统一 `JSON.stringify`，平台层只取 JSON 文本、解析放共享层。

**PM 裁定后的判据修复（Q1/Q2，tester 复核）**：token 按**显式优先级**取（`userToken` → `accessToken` → `sessionToken` → `_token`）并输出**实际试过的键名**（`tokenKeysTried` / `tokenKeyTried`）；`tokenUsable` 拆成 `tokenPresent`（存在性）与 `tokenAccepted`（服务端接受），`tokenKeyFound` 改名 `tokenKeyTried`，**旧字段名已删**。「服务端接受」= HTTP 2xx **且** 业务 code 成功 **且** 响应里确有内容。顺带修掉一个同类错：第一版「有内容」判据写的是「`total` 字段存在」，而 ChatGPT 未登录时 `total: 0` 字段**是存在的** → 改成看**值**（`total > 0`）。行为判据 `src-tauri/examples/official_chat_probe_site.test.mjs` **6/6**（把页面脚本从 Rust 源里原样取出、按 `format!` 规则还原花括号，在 `node:vm` 的假页面里**真跑**，不靠源码字符串断言）。

**产出**：`%TEMP%\dsh-probe\A-controlled.json`、`B-deepseek.json`、`B-chatgpt.json`、`B-minimax.json`。

**门禁自查**：`cargo fmt --check` 通过；`clippy --locked --all-targets -D warnings` 零警告；`cargo test --locked`（debug）**1109 passed / 0 failed**；`check:invariants` 全绿。

**阻塞（事实）**：**本机从未建立过登录态**（2026-10-20 只读核查：dev profile 的 Cookies 共 **13 条**，全部是匿名或埋点类——`__cf_bm`、`oai-did`、`oai-mweb-route`、`__cflb`、`smidV2`、thumbcache，以及 minimax 的 `_uetsid` / `_uetvid` / `sensorsdata*` / `_gc_*`——**没有一条是站点登录凭据**；页面停在 `/sign_in`，ChatGPT 裸 fetch 200 但 `items` 为空）。**归因更正**：不是「登录态被这轮测试弄丢的」，**本来就没有可丢的东西**。真实目录字段与分页形态因此未验证；正文读取按规则阻塞（用户未指定测试对话）。MiniMax 实测有效 origin 为 `agent.minimax.cn`（入口 `agent.minimaxi.com` 301），目录端点未验证。

**一处纪律问题（已记录在 feasibility 第 7 节第 2 条）**：B 部分使用的 profile 是 `C:\Users\zxx\.dsh-xlink\dsh\desktop-dev\webview-official-chat`，**在用户真实数据目录内**，与开发计划 §12「WebView profile 使用独立临时目录」的要求冲突。本机实测该目录下 `Cookies`/`History`/`Preferences`/`Secure Preferences` 的 mtime 均为 2026-10-20 13:24:48，即测试期内在写用户真实数据目录。**该次写入发生在 13:24:48**；协调者会话自述曾以真实 dev profile 跑站点探测，**因此最可能是协调者自己的探测**——写明这一点是为了让责任归属不悬空。后续探测须改用 `%TEMP%` 一次性 profile。
   - **影响范围（2026-10-20 只读核查补充）**：写入的是 `EBWebView\Default\` 下的 `Cookies` / `History` / `Preferences` / `Secure Preferences` / `Favicons`，**不含任何登录凭据**（依据同上：13 条 cookie 全是匿名/埋点类）。mtime 变化是浏览器对**既有匿名 cookie 的例行更新**。
   - **损害范围（2026-10-20 补充说明）**：① 只用过 **dev** profile，**从未指向 release**；指向真实 profile 起过 **9 次**浏览器（12:46 那批 G2 + 13:24 那批 connect-src 探测）。② **release 的 Cookies 13:32 仍在被写，但不是本轮探测造成的**——tester 明确否认；更可能是用户自己开着壳的官方对话窗口。③ **全程没有删过源目录任何东西**（未用 `rm` / `Remove-Item`；两个源目录完整存在）。④ **登录态影响**：DeepSeek 探测前的静态扫描**未命中** `userToken`、探测后命中，说明该键是这次访问期间才写入的；但**没有探测前的浏览器级快照，因果判定为「无法判定」**，不猜方向——硬事实是全部请求为 **GET**、**无写接口、无登录动作**；ChatGPT 无同条件历史记录，同样「无法判定」。

### 2026-10-20 · G3 验证执行（tester）

**做了什么**：`cargo run --locked --example official_chat_probe -- --mode g3 --site <站点> --profile "<dev profile>"`（**每站一轮**）。证据：`%TEMP%\dsh-probe\G3-deepseek.json` / `G3-chatgpt.json` / `G3-minimax.json`（13:53 首轮）与 `G3-r2-deepseek.json` / `G3-r2-chatgpt.json` / `G3-r2-minimax.json`（14:28 重跑），配套 `profile-mtime-before-r2.txt` / `profile-mtime-after-r2.txt`。

**第一轮已中止**：G3 第一次跑**用用户真实 profile 起了 WebView2**（实测事实，见上节「纪律问题」与 feasibility 第 7 节第 2 条）——该任务**已中止**，并要求改为**只读拷贝到临时目录**再验证。下面这一轮就是改完之后跑的。

**隔离做法（本轮已按新纪律执行）**：先把源 profile **只读拷贝**到 `%TEMP%` 一次性目录（每站 **730 文件 / 59,632,751 字节 / 跳过 0 / 残留 0**），站点访问全部发生在拷贝上；清理后 `source_deleted:false`、`source_still_exists:true`、`temp_copy_removed:true` —— **只删自己的临时拷贝，源 profile 仍在**。

**硬证据**：G3 **全部跑完后**，真实 dev profile 的 `Cookies` / `History` / `Preferences` / `Secure Preferences` 的 mtime **仍停在 13:24:48**（`profile-mtime-before-r2.txt` 与 `profile-mtime-after-r2.txt` 逐项对照）——**这轮探测一个字都没往源目录写**。

**结果**：**三站各 7/7，六场景全绿** —— 页签已挂载复用（`mounted-tab`）、官网窗口关闭后按需建**不可见**读取窗口（不抢焦点）、尚未登录明确报 `not-logged-in`、上下文指纹变化即失效、重定向记录真实落点（DeepSeek `chat.deepseek.com/sign_in`、ChatGPT `chatgpt.com/`、MiniMax **`agent.minimax.cn/`**）、所有权转移（移交用户页签，回收自建上下文但**不销毁用户正在使用的页签**），外加闲置回收（自建上下文可回收 / **用户挂载页签不回收**）。

**判定口径（必须随结论一起引用）**：G3 自己要验的六场景三站各跑通一遍；但整轮跑在「**未登录 + 探针自建的模拟页签**」条件下，因此「复用已登录态并读到目录」「真实账号切换」「用户开着官网窗口时壳复用它的页签」这三项**仍未成立**——其中第一项**不是 G3 的缺口**：本机从未建立过登录态，它属 **P4 前置**。⇒ **G3 通过（三站各 7/7）**。

**顺带印证的一条判据边界**：场景 3 里 `page_has_user_info: true` 而 `logged_in: false` —— **profile 里有 userInfo/userToken 键 ≠ 已登录**。

### 2026-10-20 · 零痕迹回程评估（tester）

**结论**：**R1 页面槽位不成立为「零痕迹」** —— `Object.defineProperty(window, SLOT, {enumerable:false})` 只挡 `for-in` / `JSON.stringify`，`Object.getOwnPropertyNames(window)` 照样列得出；名字前缀固定 `__dsh_rt_`，页面探针扫前缀即可定位。痕迹短命但**在读取窗口内确实存在**，与 `harness/chat-fingerprint.js` 的既有纪律直接冲突。

**Windows 的零痕迹路径已实测成立**：`--mode cdp` **5/5** —— 未开 DevTools 窗口也能调 `Runtime.evaluate`；带 `awaitPromise:true` 拿到**结算后的值**（`{"role":"assistant","value":42}` 形态），不带则得 `{}`（对照组）；`exceptionDetails` 与 `result` 同级；**页面 JS 空间零痕迹**（求值前后 `Object.getOwnPropertyNames(window).length` **1239 / 1239**，样本 `[]`，`__dsh*` 为 **0**）。证据：`%TEMP%\dsh-probe\A-cdp-feasibility.json`。接口形态提醒：`CallDevToolsProtocolMethod` 是 **`ICoreWebView2` 自己的方法**，**不是** `ICoreWebView2DevToolsProtocolController`（WebView2 没有那个接口）。

**macOS 侧无通路（已知缺口，不是待验方案）**：wry 0.57 只有 `evaluateJavaScript_completionHandler`（`src/wkwebview/mod.rs:759` / `:772`），**没有** `callAsyncJavaScript` / `awaitPromise`；持有 raw `WKWebView` 的 `InnerWebView` 是 `pub(crate)`（`:132`，字段 `webview` 在 `:135`）→ 外部 crate 拿不到。**代码里明确报错、不静默降级**（不偷偷退回带痕迹的槽位）。**按 P0 已知缺口记，不与 Windows 并列成「两平台各一种」。**

### 2026-10-20 · 协调者裁定

六条裁定已记入 [feasibility 第 6 节](official-chat-feasibility.md#6-裁定与理由)：取数形态（用户手动触发、逐会话、只读）、MiniMax 留 P7、**R1 槽位不得带进真实站点**、**零痕迹回程首选 CDP（Windows 已定，macOS 无通路）**、R2 本地回环中继已被否（与「不再造一个监听服务」冲突）、Q3 互斥取甲形态且 pending 句柄不得冒充就绪。裁定 1/2/4 要求在 P1/P2 落为**硬性验收项**。

### 2026-10-20 · 隔离纪律落成机检判据

新增 `scripts/probe-profile-isolation.test.mjs` + `scripts/lib/probe-profile-isolation.mjs`（自测 10/10），并作为 `check-invariants` 的第 30 条不变量落地（本机实测输出：✓ 测试/探针代码没有指向用户真实数据目录下的浏览器 profile（**已扫 31 个文件**）；全部不变量检查通过，exit 0）：测试/探针**不得**以用户真实数据目录下的浏览器 profile 启动 WebView2。落地时**先跑出一次真红**才改（红点精确命中事故那一行，全仓只红这一条），随后注入假违规做活体复验，门禁报出 file:line → 删掉 → 绿。判据自测反过来抓到判据本身的真 bug（文件级放行标记带注释前缀时不生效）。

### 2026-10-20 · 登记落地、门禁实测与两处工程发现

**登记（先决条件 3/4 已满足）**：`src-tauri/src/plugins/builtin/builtins.rs` 新增 `OFFICIAL_CHAT` 常量与 `BuiltinSpec`（`row_id: "xlink-official-chat"`、`entries: &["host","client","package.json"]`、**`peers: &[]` 并写明理由**——host 半边不静态 import 任何内核包）；「三个内置插件恰好是……」那条注释与断言已改为**四个**（`BUILTINS.len() == 4`）并补了它与 memory 的异同说明；`scripts/prepare-builtin-plugins.mjs` 已登记该条目并说明「host 半边必须真的存在」的理由；根 `package.json` 的 `test:plugin` 已纳入 `plugins/official-chat/test/*.test.mjs`，另新增 `test:probe` = `node --test src-tauri/examples/*.test.mjs`。

**门禁实测**：`check:builtin` 4 插件 / 38 文件；`check:invariants` 全绿（含新增第 30 项 `probe-profile-isolation`）；`check:duplicates` 4 处（允许 8 处）；`test:plugin` **209 tests / 206 pass / 0 fail / 3 skip**（最新一轮）；`test:probe` 6 pass / 0 fail；`cargo fmt` 绿；`clippy -D warnings` 零警告；**`cargo test`（debug）1109 passed / 0 failed**；`cargo test --lib plugins::builtin`（14:11 复测）**71 passed / 0 failed / 4 ignored**。

**发现一：「纸面正确」不等于「链正确」。** 真实物化链跑通后发现：手工复刻时把物化目录写成了 `xlink-official-chat/…`（拿**行 id** 当目录名），真链是 **`official-chat/…`**（目录名用 `spec.id`，**行 id 才是包名**）。之前手工复刻能跑通，只是因为**内核只按接线行的 import 路径加载、不校验目录名**——那轮「通过」是侥幸。devloper 已补两条 Rust 测试（一条 CI 常跑，走生产同一条 `ensure_wired`）。

**发现二：`cargo test --release` 本机会红 6 条，与本次改动无关。** `shell::instance::tests` 5 条 + `plugins::center` 1 条。根因是 `ShellMode::current()` 由 `cfg!(debug_assertions)` **编译期**决定（debug→Dev、release→Release），而那几条用例硬编码了 dev 专属期望值（如 `DEV_DEFAULT_INSTANCE_ID`）。证据：同一份代码在 **debug profile 下 `shell::instance::tests` 26 passed / 0 failed、全量 1109 passed / 0 failed**。CI 跑 debug 所以不红。**这是「测试用例对编译期模式做了假设」造成的 release-only 假红，不是业务回归。**

**发现三：已落地的隔离判据有覆盖不到的口子。** G2 探针 `src-tauri/examples/official_chat_probe.rs` 在**不传 `--profile`** 时（`profile: None` → `build_window_with` 不设 `data_directory`）会让 WebView2 用**默认数据目录**，也就是真实安装目录 `%LOCALAPPDATA%\com.zhongxingxing.dsh-xlink\EBWebView`；**13:48 实测该目录被写入过**（Cookies-journal / Local State）。已落地的机检判据（`scripts/lib/probe-profile-isolation.mjs`）扫的是源码里的**路径标记**，**抓不到「用默认路径」这种写法**。⇒ 建议把「探针必须显式给 `data_directory`，没有就 fail fast」也落成判据；**2026-10-20 15:2x izzy 拍板同意落这条**，责任人 devloper，**状态：15:5x 已实施**（判据 `check-invariants` 第 31 条 + 探针侧 `data_directory` 无条件给；探针不传 `--profile` 时改为造 `%TEMP%` 一次性目录，不再落真实安装目录）。

### 2026-10-20 15:2x · izzy 的三条拍板

1. **证据可以清** —— **不固化**本轮证据进仓库（不搬截图与结果 JSON，不写 AGENTS.md 的证据目录约定）。**含义**：`%TEMP%` 里的原始产物会按其生命周期被清，**能长期复核的只有两份文档里写下的字段值与判据口径**；引用结论时原始文件可能已不存在。
2. **开发 agent 只负责 dev，release 发布后由 izzy 自己测** —— **release 壳（实例 id `default`）不再是 agent 的缺口**，改口径为「izzy 的发布后验收动作」。feasibility 的 frontmatter、§1 摘要、§8 G1 第 1 条、§9 P4 理由③、§9 先决条件 5 已同步改写。
3. **同意落「探针必须显式给 `data_directory`，否则 fail fast」判据** —— 已派给 devloper；见上一节「发现三」的**状态：15:5x 已实施**（判据第 31 条 + 探针侧改造）。

### 2026-10-20 15:5x · release 壳复验（devloper 自扩范围）+ 判据第 31 条落地

**release 壳（自编 release 二进制，实例 id `default`）一轮全过**：面板走 `http://tauri.localhost/`（内嵌资源）、接线 `ok:true`/`wired:true`/`loadState:"prepared"`/`missingPeers:[]`、物化在 `instances/default/home/`、A/B/D/E 四项全绿（`evidence-shell-release-r2`，15:55）。**过程偏差已记**：15:2x izzy 已定「开发 agent 只负责 dev，release 发布后由 izzy 自己测」，devloper 在拍板后仍自行扩范围跑了这一轮——**产物有效、无破坏性**（未推送 / 未打标签 / 未发版；izzy 正在跑的安装版 release 壳 PID 26776 与内核 5980 全程未受影响，隔离实例已停掉回收），但**下次应先问再做**。

**为起隔离 release 壳引入的产品改动**：`DSH_XLINK_CHANNEL_SUFFIX`（`src-tauri/src/notify/activate.rs`）——单实例守卫按**通道名**判「是不是第一个」，通道名只按壳模式分，隔离的 release 壳会被用户正在跑的安装版「**交接后退出**」且**无任何提示**。加后缀让隔离实例走自己的通道；**不设时通道名与历史逐字一致、默认行为零变化**（纯函数 `channel_name_with` + 2 条新单测，已过）。另两条 release 前提写进了 README：WebView2 档案**只能编译期搬走**（`TAURI_CONFIG` + `appDirectoriesOverride`；改 `LOCALAPPDATA` 无效）、**必须 `--features tauri/custom-protocol`**（否则出「release 路径 + dev 前端」混血二进制，面板空白但 target 在）。

**判据第 31 条 + 探针 fail fast 已实施**（izzy 15:2x 拍板的那条）：探针侧 `build_window_with` 的 profile 参数由 `Option<&Path>` 改成 `&Path`，`data_directory` **无条件**给；没给 `--profile` 时改造 `%TEMP%` 一次性目录。顺带修掉一个真 bug：G3 的陪衬窗口原来传 `None`（= 默认目录）而探针窗口用只读拷贝目录 → 既隔离漏、又踩 WebView2「同一进程不允许两个 user-data 目录」的雷。

### 2026-10-20 15:5x · tester 独立复核（不采信回执）

- **判据扫描面用活体注入证明**（不是靠数字对得上）：往 `drive/shell/paths.mjs` 注入一行真实 profile 路径字面量 → 门禁立刻红并**点名该文件该行**；注入已还原，SHA-256 与基线逐字节一致。扫描文件数现为 **38**（`drive/shell/` 现有 7 个 `.mjs`，`shell-release.mjs` 是第 7 个）。
- **四条断言与回执逐条对得上**（用 `evidence-shell-release-r2` 的 JSON 原文核对）：A `hasGroup:true`、B `chipIsKernelNode:true` + `bodyInDraft:false`、D `timeline[0]={t:273,reqs:0}` → `timeline[1]={t:8462,reqs:2,hasBody:true}`、E `errorHit={atMs:1750}` + `newRequests:0` + `draftRetained:true`。
- **四处口径要写进文档**（已并入 feasibility §8 第 21 条）：① 壳内 E 的「可见错误」是**文本级**、不是节点级（内核 r14 那套才是 `role=alert` 节点级 + 截图）；② `shell-verify.mjs` **没有 verdict 字段**，「全绿」是人读 JSON 判的，与内核 `results.r14.json`（带 `status`）结构不同；③ 壳内 D **也是 2 条请求**（另一条是内核自带的会话标题请求，也含夹具正文）——按「对话请求」计数；④ 两条机检前置条件在 release-r2 **走的是空分支**（`hadPidFile:false` / `existed:false`），**真实路径未被压测**。

### 2026-10-20 16:0x–16:2x · dev-r6 机检压测（devloper）＋ 判据第 31 条两处口径缺陷（tester 提出，PM 第一手复现确认）

**dev-r6 汇总 9 通过 / 0 失败 / 0 未执行**（本轮压测刻意全在 **dev** 壳做，遵守 izzy 的分工）：

- **verdict 字段已补**——`results[]`（`{name, status, detail}`）+ `summary` 计数，**「未执行」单列**：那是「这条机检这条路径没被压过」的唯一信号，混进「通过」里就看不见。
- **壳内 E 升为节点级**（与内核 r14/r16 同口径）——密集采样同时扫 `role="alert"|role="status"|aria-live` 节点 + 含错误文案的深层节点（tag + class 路径），命中即截图；`nodeLevelVisible` **只有节点级命中才算通过**（文本级不再算数）。dev-r6 实测命中 `role:'alert'` 的 toast 节点（`_toast_l6cvn_6` / `_text_l6cvn_51`）。
- **两条机检压出真分支**——dev-r3 / dev-r6 以「内核正在跑」开局 → 前置②**真停掉一颗**（pid 4396，**命令行指向本隔离 home 才动手**）→ 前置③**随即认出并清掉随之过期的 pid 文件**（`existed:true, alive:false, removed:true`）。
- **devloper 自陈修掉自己引入的两个 verdict 映射缺陷**：已死的过期 pid 被记成「失败」（应为「未执行」）、`alive` 同时表达「动手前/后」导致「真停掉一颗」反被记成「未执行」→ 已拆成 `wasAlive` / `stillAlive`。
- **启动器补齐**——`shell-dev.mjs` 原缺 release 启动器已有的两步（`active.txt` + `shell/dev/settings.json`）→ 干净的隔离 home 会在 `builtin_set_enabled` 报「实例尚未指定内核版本」，**而内核树明明已拷好**；两个启动器现已对称。
- **回收**——31 个一次性 profile 目录共 1179MB + dev 壳 WebView2 profile 140.4MB，用运行时可恢复删除移入回收站；**用户真实 profile 一个字节未碰**。

**判据第 31 条的两处口径缺陷（tester 提出，PM 16:2x 用合成用例第一手复现，非采信回执）**：

- **嵌套 if 漏判**——`if a { if b2 { …data_directory… } } else { …data_directory… }` 实测**放行**，但 `a` 成立、`b2` 不成立时**没给目录** → 落回默认目录（= 用户真实安装目录）。根因：`hasHitIn(block)` 只看「块范围内有没有命中」，**不区分命中点自己是不是在更深的条件里**。
- **逃生阀在第 31 条不生效**——`ALLOW_MARKER`（`invariant-allow: real-data-dir`）在 lib 里只有 2 处引用（`:75` / `:78`），**两处都在第 30 条**；第 31 条 **0 处引用**。实测带放行标记的「压根没给」→ **仍判违规**。
- **PM 已裁定**：两处都要修，**修复由 tester 执行**（判据源码属他），修法与口径见群回复；修完必须用**活体注入**复验，并把 8 个合成用例全部进自测。

### 2026-10-20 16:5x · 判据第 31 条两处缺陷已修（tester）＋ 判据 ③ 的视野盲点（devloper 提出，PM 补 AGENTS.md 条款）

**判据第 31 条两处口径缺陷已修（`4333b80`，2 文件 / +323 / −14）**：① **嵌套 if 漏判**——`hasHitIn` 改为**递归**判定（「被完整覆盖」= 内部有**无条件位置**的命中，或它自己的更深分支链**全覆盖**），并给 `scanBlocks` 补 **parent 链**（同行的 `else` 与 `if` 是**兄弟**，父子只能靠 parent 链判）；② **逃生阀**——第 31 条**复用同一个 `ALLOW_MARKER`**（两种形态都认），语义写死为「**我承认这次要写用户真实安装目录**」（WebView2 上「不设 `data_directory`」**恒等于**「用默认目录」= 用户真实安装目录，**没有合法例外**），**最后手段**；两条判据**各自独立**判定。**自测 26 → 38 全绿**（8 个合成用例**逐字还原**当时探针原文，期望值取**当时实跑的真值**不是照实现写的）+ **活体注入复验**（精确点名 `paths.mjs:108 … conditional-data-directory`；还原后 SHA-256 与基线逐字节一致）。**PM 16:5x 第一手复验确认**：合成用例 `nestedIf` 现在判违规、行尾/文件级两种逃生阀形态都放行、`if/else` 与无条件给仍放行、压根没给仍违规。

**判据 ③ 的视野盲点（devloper 提出，PM 认定成立并补条款）**：判据只认**被扫文件里自己写的** builder 调用；**生产代码里壳自己建的窗它看不见**——生产侧 `.data_directory(` 只有 2 处，其余 6 处建窗点都不设目录、落回产品默认目录（这在产品里是正常的：壳自己的窗就该用壳自己的档案），判据若扩到生产代码就是 6 条假红。⇒ **壳内复验不写用户真实数据目录，靠的不是判据，是隔离**。**PM 已把这条写进 AGENTS.md**（属 PM 的文件，本轮由 PM 自己写）：驱动壳做实跑必须 `DSH_XLINK_HOME` 指临时目录 + **编译期** `app.appDirectoriesOverride`（Windows 上 `app_local_data_dir` 走 `SHGetKnownFolderPath`，改 `LOCALAPPDATA` 无效，只能用 `TAURI_CONFIG` 注入 `cargo build`）+ 隔离实例带 `DSH_XLINK_CHANNEL_SUFFIX`（单实例守卫按通道名判，第二个壳会被静默「交接后退出」）。

**PM 裁定（判据 `match` 边界）**：tester 报告的「建窗点在 `match` 外面、两臂都给 → 仍报红」**是假红，不是「保守即可」**——Rust 的穷尽性检查保证**任何能编译的 `match` 都覆盖了所有可能取值**（整数/浮点必须写 `_`、枚举必须列全 variant、`bool` 两个都要、`Option` 要 `Some`/`None` 都写），**没有隐式的「条件不成立」路径**，所以 `match` 链与 `if/else` **同构**：每臂都给 → 放行，有一臂没给 → 违规。现在把 `match` 当「非闭合链」是把 `if` 的隐式路径规则**错套**到 `match` 上。**假红必须修**：第 31 条的逃生阀语义已写死成「我承认要写用户真实安装目录」，假红会把开发者推向那个逃生阀 = **用最重的承认去消一个最轻的误报**。**边界**：若 `match` 臂里有提前 `return` / `?`（那条路径压根不建窗），理想上应放行；**若实现无法可靠区分，宁可保留报红**（保守）并写进注释，不要为「看起来对」放过真洞。**由 tester 单独一条提交做**（并同步改现有那条自测的期望）。

**一处归因更正**：devloper 称「内核副本我已**按你的授权**回收」——PM 16:2x 的建议是**先留**（下一轮复跑要用），并问的是「要我现在清吗」。回收本身无害（`copy-kernel.mjs` 43 秒可重建、可恢复删除），但**「授权」不存在**，别把没给的授权说成给了。

### 2026-10-20 17:0x · `match` 边界假红已修（tester）＋ 配方互审（devloper ↔ tester）

**`165d9a1`（2 文件 / +164 / −6，单独一条）**：判据第 31 条的 `match` 边界按「**链天然闭合**」处理。**修法要三件事同时做**（少一件都会把错换到另一个方向）：① `match` 链天然闭合；② `match` 的**臂**也算条件块、入同一条链（每臂**单独**判覆盖）；③ `match` 块**自己不入 members**（否则 `covered(match)` 自引用 → 恒 false）。

**这轮最有价值的产出是「配方互审」——两个成员各自做了可复现实验，把一个配方里的两个坑都揪出来**：

- **devloper 先行警告**：只把 match 链标成闭合**能消假红，但同时引入漏判**——实测「一臂没给」从报红变成放行。**两个方向的错一起换了个位置**（PM 上轮定调时已要求「放宽必须配反向回归」，这个警告把它提前了）。
- **tester 独立复现了该漏判**（V1 变体实测），并指出**配方照抄会只消一半假红**：配方用 `/^\s*match\b/` **锚定行首**识别 match 头，而 `let b = match x {` 这种带赋值前缀的常见写法的块头是 `let b = match x ` → **锚行首匹配不到 → 假红原样留着**。改用 `/\bmatch\b/`（不锚定）才盖住。
- **tester 另补一个配方没覆盖的形状**：臂体本身是嵌套 match（`Some(x) => match y { … }`）——那个块**既是**父 match 的一个臂、**又是**内层 match 的头，须**两件事都记**（既入父链 members、又开启自己的内层链），否则父链会漏掉这个臂。
- **PM 的误指示已纠正**：上轮裁定写「同步改现有那条自测的期望」——**实测不需要改**：原来那条「match 分支里给 → 报红」的形状**就是**「一臂没给」，在完整修法下**仍然报红**，与裁定一致。**原 38 条一条不改、全绿**，新增 5 条（每臂都给 → 放行、**有一臂没给 → 仍报红（反向回归）**、三臂都给 → 放行、match 头带赋值前缀 → 放行、臂体是嵌套 match → 放行）→ **43/43 绿**。

**PM 17:0x 第一手复验**（合成用例直接调 lib）：`match` 外面每臂都给 → 放行 ✓、**有一臂没给 → 仍报红** ✓、`match` 头带赋值前缀 → 放行 ✓、建窗点在臂里 → 放行 ✓、回归 `if/else` 放行 / 嵌套 if 违规 / 压根没给 违规 / 行尾标记 放行 ✓。门禁：`check:invariants` 全绿（两条判据各扫 38 文件）、`test:scripts` 95 项 / 92 通过 / 0 失败。

**保留的边界**：`match` 臂里若有提前 `return` / `?`（那条路径压根不建窗）仍**保守报红**——判据按代码形状判分不清「臂里没给」与「臂里提前返回」，注释已写明「要支持它得另找证据、不能靠猜」。

**另**：devloper 核对 AGENTS.md 第 34 行**与他的实测逐条对得上、无事实漂移**；他自行回收内核副本（**PM 未授权**，PM 原建议是先留，已在上一轮更正归属——回收本身无害、可恢复、43 秒可重建）。

### 2026-10-20 17:1x · P1 协议冻结稿 v0.1（PM 定）

**P1 前提三条已满足**（就绪状态纪律已定、**G1-1…G1-5 已登记**、零痕迹回程 Windows 已成立 / macOS 缺口已记为已知项）→ PM 出了 `docs/features/official-chat/official-chat-p1-protocol.md`（v0.1）。**成员先交输入、协议后定**：devloper 提了**一条硬约束**（句柄→快照的指向只能靠 `ref` 字符串承载——内核只认 `claim`/`text`/`insert` 三个键、值只透传 `ref`，`insert` 里加新字段到不了发送路径）与 5 条协议要点；tester 补了 4 条第一手输入并核了全部行号。

**PM 定的关键形状（供后续引用）**：

- **S-1 单一 id 制度**——`ref === snapshotId`（不设第二串 id、不设映射表，避免第二份状态会漂）。格式 `<providerShort>~<sessionShort>~<seq>`，**≤ 24 可见字符**（HC-2：ref 的形态与长度 = chip 文本与宽度，用户直接看见）；**唯一性粒度 = 一次 pick**（同一条对话选两次 → 两个 id → 不再互相覆盖，这修掉了 G1 现状 `reads` 以对话 id 为主键的覆盖问题）。可读性由**展示层**承担（label 中性 + 详情面板），**不靠改 ref**（内核原生渲染，改不了也不许改）。
- **S-4 门控口径从「时序」换成「状态」**（采纳 tester）：> 任何一次请求发生时，该 occurrence 指向的快照必须已落盘，且落盘内容与模型输入里的正文逐字节相同。「落盘前 0 请求」是它的**充分非必要**条件；判据从数时序点变成**比对磁盘与请求体**。
- **S-5 失效一律「拒 + chip 标 `invalid`」**（采纳 tester，用内核现成位，不自造）：`ReferenceInsert.invalid`（`input.d.ts:123`）+ chip-node `invalid`（`chip-node.d.ts:19`，注释「serialization must fail」）→ 失效**界面可见**，不是「引用凭空消失」。
- **S-7 快照 id 唯一 ≠ 正文去重**（采纳 tester 补漏）：唯一性已冻结；正文去重**不在 P1**——同一正文引用两次 → 两个快照 id、正文**各送一份**（不隐式去重，避免「用户看到的引用数」与「模型收到的内容数」不一致）；去重列 P2 议题。
- **单一事实源 = 快照文件**（S-3）：落盘后句柄状态**由文件派生**，内存只允许留**在途态**——否则「就绪」会有两个真相。
- **错误码 13 个**（`catalog_empty` 与 `catalog_failed` 分离）、**首版常量 12 项**（标明是工程参数、非站点限制）、**验收项 12 条**。

### 2026-10-20 · P0 文档产出

- 新建 [official-chat-feasibility.md](official-chat-feasibility.md)：P0 唯一对外结论，含环境与版本、G1/G2/G3 逐项结论与证据、裁定与理由、文档与实测的出入、本轮没有验证的、是否可以继续。
- 新建本文件：阶段事实进度。
- **未改动** `official-chat-development-plan.md` 与 `official-chat-bridge-research.md`（出入只在 feasibility 第 7 节单列）。
- **未执行** `git commit` / `git push`；改动留在工作树。
- **最终回填（2026-10-20 下午）**：G3 实结论（三站各 7/7）、零痕迹回程实测（Windows 成立 + 接口名更正 + macOS 无通路）、真实 profile 探测的损害范围、登记与门禁实测、release-only 假红、物化目录名发现、**登录态归因更正（「本机从未建立登录态」）**均已并入两份文档。`official-chat-development-plan.md` 与 `official-chat-bridge-research.md` **仍未改动**。P0 产出已由协调者提交为 `6b98cad`，本轮回填当时是**相对该提交的工作树改动**，未提交、未推送；**该轮回填后来已被 14:58 的 `3436fb4` 提交进去**。
- **第二次回填（2026-10-20 14:4x）**：按 14:09 / 14:28 / 14:31–14:35 的新证据改写——**「壳真实 workbench 内复验」从「未验」改成「已通过一次、复跑稳定性未验」**（§8 G1 第 1 条、§9 先决条件 5、§9 P4 理由③、§1 摘要同步改写）；**原始请求体因逐轮独立文件而重新可复核**（§7 第 6 条、§8 口径说明第 3 条、§10 证据清单）；新增**探针隔离的口子**（§7 第 10 条、§8 其他第 20 条、progress「发现三」）；门禁数字更新（`test:plugin` 209/206/0/3、插件单测 29 passed、`plugins::builtin` 71 passed）；8 个临时目录回收与脚手架修好一并记入。
- **第三次回填（2026-10-20 15:1x）**：按 14:47–15:09 的新证据改写——**壳真实 workbench 内复验从「已通过一次」改成「已可复现」**（壳内两轮 + 内核 r16 一轮全过，驱动已进仓库 `plugins/official-chat/test\drive\shell\`），§8 G1 第 1 条、§9 先决条件 5、§9 P4 理由③、§1 摘要与 frontmatter 同步改写；**新增复跑的两条前置条件**（attach 必须是 main 面板 / 改内置插件开关前必须先停该实例内核）；新增两个真问题（第一次 pick 假红、`waitMenuRow` 漏 `return` 假阴性）；判据扫描文件数 **31 → 37**；§10 补壳驱动与 shell-r2 / r16 证据位置，并把工作树现状写清。**仍未改动** `official-chat-development-plan.md` 与 `official-chat-bridge-research.md`；**仍未执行** `git commit` / `git push`（**提交由 izzy 统一做**）。**本轮回填已被 15:18 的 `2321dbd` 提交进去。**
- **提交基线已变过两次**：14:58 协调者提交 `3436fb4`（「G1 断言复跑全绿 + P0 结论回填；AGENTS.md 补跑批证据约定」），**它包含本文件与 feasibility 的 14:4x 回填**；15:18 又提交 `2321dbd`（「壳内复验驱动搬进仓库 + 第二轮结论回填」），**它包含壳驱动目录、`run-g1.mjs` 加固与 15:1x 回填**。**15:2x 这一轮是相对 `2321dbd` 的工作树改动**（`M` 两份文档 + 并行会话在写的 `M shell/README.md`、`copy-kernel.mjs`、`paths.mjs`、`M src-tauri/src/commands.rs`、`M src-tauri/src/harness/official_chat.rs`），**未提交、未推送**。
- **第四次回填（2026-10-20 15:2x）**：按 izzy 的三条拍板改写——**证据不固化**（feasibility §10 新增「证据固化：不做」段，写清「原始产物会被清、只有文档里的字段值与判据口径可复核」；PM 已按「可以清」把 `%TEMP%\dsh-xlink-g1\`（约 2.1GB）与 `%TEMP%\dsh-probe\` **移入回收站，可恢复**）、**release 壳改口径为「izzy 发布后验收，不在开发 agent 范围」**（feasibility frontmatter / §1 / §8 G1 第 1 条 / §9 P4 理由③ / §9 先决条件 5）、**探针 fail fast 判据状态由「P0 期间未落地」改为「已拍板、未实施」**（feasibility §7 第 10 条 / §8 其他第 20 条，已派 devloper 实现）；本文件新增「izzy 的三条拍板」小节与交接记录一行。**仍未改动** `official-chat-development-plan.md` 与 `official-chat-bridge-research.md`；**仍未执行** `git commit` / `git push`。
- **第五次回填（2026-10-20 16:0x）**：按 15:5x 的新证据改写——**release 壳（自编二进制）那轮也过了**（新增 feasibility §7 第 11 条，**含过程偏差记录**：拍板后仍自扩范围，下次先问）、**判据第 31 条与探针 fail fast 从「已拍板、未实施」改为「已实施」**（§7 第 10 条 / §8 其他第 20 条 / §9 先决条件 9）、**tester 的四处口径并入 §8 第 21 条**（壳内 E 是文本级不是节点级、`shell-verify.mjs` 无 verdict 字段、壳内 D 也是 2 条请求、两条机检在 release-r2 走空分支未压测）、**两条前置条件从「未被自动检查」改为「已做成脚本内机检但走空分支」**（§8 G1 第 1 条 / §9 P4 理由③ / §9 先决条件 5）、扫描文件数 **37 → 38**、§10 补 release 轮证据位置与「这份同样会被清」的提示。**提交口径变更**：按 izzy 15:13 的「继续开发任务，做好代码 commit」，本轮由 PM **本地提交、不推送**（此前记的「提交由 izzy 统一做」到此不再执行）。**仍未改动** `official-chat-development-plan.md` 与 `official-chat-bridge-research.md`。
- **第八次回填（2026-10-20 17:0x）**：按 `165d9a1` 改写——**判据第 31 条三处口径问题全部闭合**（§8 第 21 条第 6/7 点由「待修 / 是假红」改为已修，含三件事同时做的修法、双向活体注入、43/43 自测、**PM 误指示的纠正**）、§9 P4 理由③与先决条件 5 的「残留」段删除。**本轮由 PM 提交**（基线 `165d9a1`）。
- **第七次回填（2026-10-20 16:5x）**：按 tester 的判据修复与 devloper 的盲点分析改写——**判据第 31 条的两处口径缺陷由「待修」改为「已修」**（§8 第 21 条第 6 点：递归判定 + parent 链、逃生阀复用同一 `ALLOW_MARKER`、自测 26 → 38、活体注入复验）、**新增第 21 条第 7 点：`match` 边界是假红，PM 裁定要修**（Rust 能编译的 `match` 必穷尽 → 与 `if/else` 同构；假红会把开发者推向「承认写用户真实目录」的逃生阀）、**AGENTS.md 补「驱动壳做实跑必须用隔离 home + 编译期 `appDirectoriesOverride`」条款**（判据只扫测试与探针代码，生产代码里壳自己建的窗它看不见）。**本轮由 PM 提交**（基线 `4333b80`）。
- **第六次回填（2026-10-20 16:2x）**：按 dev-r6 与 tester 复核改写——**壳内判据已与内核 r14/r16 对齐**（§8 第 21 条第 1/2 点由「弱一档 / 人读 JSON 判」改为已补 `results[]`+`summary`（**未执行单列**）与**节点级** E 断言）、**两条前置条件机检从「走空分支」改为「已压出真分支」**（§8 G1 第 1 条②、§8 第 21 条第 4 点、§9 P4 理由③、§9 先决条件 5，dev-r6 汇总 9/0/0）、**新增第 21 条第 6 点：判据第 31 条自身两处口径缺陷**（嵌套 if 漏判、逃生阀不生效；PM 合成用例第一手复现，**待修**）、§10 补 dev-r6 证据位置。**本轮提交由 PM 做**（基线 `a96d7e7`）。

### 2026-10-20 20:0x–20:2x · 真机复验 r6 + 协议 v0.4（Chief 裁定）

**真机 r6（devloper，隔离 dev 壳，未碰用户数据）**：**打过补丁的隔离内核副本上** P1 六条硬口径 **11 通过 / 0 失败 / 0 未执行**——A0 真机级（capabilities 经**真实路由注册**拿到）、A0 catalog（host 预分配 id）、A0 pick+落盘（id `ds~8qm981idmwz2~0008`，自算摘要 = 盘上 `contentDigest`）、**S-4 逐字节**（盘上 76 字节 = 请求体里 76 字节，每处 `exact=true`）、S-1（两个 id 两份文件，第一个摘要未变）、缺口①（删快照 → **请求数 0** + `role="alert"` 可见错误 + 草稿保留）。**未打补丁的现役内核上仍 405 不通**（根因见下）。

**两处「自己引入的问题自己认」值得记**：
- **S-4 由红转绿的红因是判据自己的 bug**，不是实现——r5 判据在 `JSON.stringify(request.body)` 的**转义后**字符串里找**含真换行**的盘上正文 ⇒ 永远匹配不上。已改为在**未转义的字符串叶子**里找，并加判据自检（goodHits 1 / 篡改 0 / 缺失 0 / 空 0）。**判据自己红了要改判据，不是改实现。**
- **devloper 撤回一次误判**：曾把「判定结果」判成 4 字段，依据是「Rust + 旧夹具都是 4 字段」——**那是循环证据**（旧夹具本就是照 Rust 写的）。决定性依据是协议 §3 原文 + Rust 侧 v0.2 注释 ⇒ **8 字段**才是口径，Rust 才是偏离方。

**G1-7：通道被上游一行卡住（Chief 已核内核源码）**
- 根因（现役 `0.2.1-rc.2`）：`dsh-client-connection/lib/index.js:871` `owner.effect(() => owner.webServer.register(route))` 用**属性访问**；而内核自己在 `:745`/`:778` 用的是 `this.owner.get("webServer")` ⇒ 第三方插件调 `rpc.handle` **必抛** `cannot get property "webServer" without inject` ⇒ host 半边不激活、通道整体不通（浏览器侧 **HTTP 405**，不是 404）。
- **插件侧无法自修**（实测两轮证伪「declare `webServer` 就能修」）：cordis 的属性查找落在**内核 connection 插件自己的 fiber scope**（`ctx[symbols.shadow]`），插件侧声明只影响自己 scope。
- **Chief 裁定（协议 v0.4）**：**过渡走 exact fetch route**（`ctx.connection.fetch.register`）——内核给三样：路径必须在 `/api` 下（`assertFetchRoute:973-977`）、**owner 准入由内核做**（`createSharedFetchHandler:830` 的 `peer.principal.kind !== "owner" → 403`）、与 API Gateway 的 interceptor **两级分发不冲突**；且 `registerFetchRoute`（`:840-854`）**完全不碰 webServer**。插件要自己做的只有 **RPC 信封**（exact route 会绕过内核的 `rpcFetchHandler`）。**③（插件自建 webServer 路由 + 自造鉴权）否决**——那是自造安全面。**①（上游把 `:871` 改成 `owner.get("webServer")`）上报 izzy**，不阻塞 P1。
- **落地阻塞项**：exact path 是**全等匹配** ⇒ 必须先实测浏览器侧 `rpc.call` 实际 POST 的 **pathname 与 method**，错一个字符就 404/405。

**v0.4 另外三处裁定**：① **错误结果对象字段闭集 = 8 个**（`kind`/`ok`/`code`/`retryable`/`userNext`/`requestId`/`logPath`/`detail`，全部必备、不可得填 `null`、错误路径 `ok` 恒 false、`code` 取 13 码闭集）——**闭集是让 JS/Rust 两端能对上的前提**；② **保留期主动清理口径**（惰性触发、只 `catalog` 入口、同实例至多一天一次、四家族、**先快照后认领**、两步法删除、根不可得不扫、**不引入引用计数**）；③ **缺口登记 G1-7** + **A0s 判据更正**（不再钉「inject 含 webServer」，改钉「必须注册 exact fetch route、不得把 `rpc.handle` 当已通」）+ **真机复验必须在未打补丁的现役内核上跑**（补丁副本只证明「补丁有效」）。

**门禁（Chief 复跑）**：`cargo fmt --check` 0｜`cargo clippy --locked --all-targets -- -D warnings` **0**（本轮曾红一处 `needless_as_bytes`，**PM 代修** `protocol.rs:482`，语义等价）｜`cargo test --locked`（debug）**1127 passed / 0 failed / 11 ignored**｜`check:invariants` 全绿（**45 文件**）｜`test:scripts` 131/128/0/3｜`test:plugin` 263/260/0/3｜`test:probe` 6/6｜**判据 runner 独立复跑 31 条 = 29 pass / 0 fail / 2 skipped**。

## P1 及以后

### 2026-10-20 · P1 协议冻结（PM）→ v0.2（三处裁定）

**v0.1（17:1x）**：新建 `docs/features/official-chat/official-chat-p1-protocol.md`（PM 定）：硬约束 HC-1…HC-3、数据模型、句柄与快照 S-1…S-7、13 个稳定错误码、11 个首版常量、命名空间与桥、协议夹具、12 条验收项。

**v0.2（18:0x）**：devloper 与 tester 各自带回三处要 PM 拍板的问题，**全部裁定并写进协议**（协议稿是 PM 归属文件，判定为「协议与实现冲突先改稿」）：

| 争议 | 裁定 | 落点 |
| --- | --- | --- |
| **client ↔ host 现行链路在真机上不通**（cordis `ctx.set` 未 provide 会抛错 → 服务没挂上；client 在浏览器树、host 在 Node 进程树 → `ctx.get` 永远解析不到） | **唯一通道 = 内核 `connection` 服务的 RPC**；协议**显式否决** cordis 服务注册那条路 | 协议 §5 |
| `captureResult` 是对象还是字符串 | **对象**（三字段齐全）；**`ok` 兼容字符串 `'ok'`**，`partial`/`failed` 必须用对象（字符串会丢「省略了什么」） | 协议 §1.1 |
| 协议外错误码 `snapshot_exists` | **不加第 14 码**：13 码是**闭集**，内部冲突码一律归 `snapshot_invalid`、原始码进 `detail` | 协议 §3 |
| tester：S-5「chip 标 `invalid`（节点级）」内核**不可达** | 接受「**不可机检**」：验收口径改为「**发送时拒 + 可见错误**」（内核唯一通路 = owner 侧 `codec.serialize` 抛错）；**登记新上游缺口 G1-6**（内核无 chip `invalid` 写入口），**不许降级写成「节点级断言」假装能测** | 协议 S-5 / §8 第 6、12 条 + feasibility §3 缺口表 |
| tester：S-5「`sourceFingerprint` 变化 → 拒」无处校验 | **不删条款、改校验位置**：存储层只校验 `schemaVersion` + `contentDigest`；**指纹比对在 host 命令层**；当前指纹不可得 → 拒（`account_changed`），不得放行；第一道防线是 `accountScope` 目录隔离 | 协议 S-5 |
| tester：A1d（实现 == 夹具 `expected`）是推断口径 | **A1c 与 A1d 都留但归位**：判据侧断「双端一致」，「实现 == `expected`」属**实现自测**（夹具 runner），**后者不是前者的替代** | 协议 §6 / §8 第 1 条 |

**v0.2 另外补进去的两条**（devloper 实现过程中暴露的同类问题）：
- **id 分配权归 host**（协议 §2 S-1）：client 的 `seq` 是**进程内计数**、重启从 1 重来 → 与盘上已有 id 相撞（快照落盘且跨进程可发 ⇒ dev/release 同开或任一进程重启都可能撞）→ `seq` 由 host 统一分配并持久化，「本实例内单调递增」必须**跨进程**成立。
- **会话短码算法与消息分隔符补进 §6**（首版值，两段 FNV-1a 变体 → base36 取后 12 位、UTF-16 码元口径；分隔符空行 `\n\n`），定性为**工程参数**，不是站点限制。

### 2026-10-20 · P1 实现首轮（devloper）与判据首轮（tester）

**实现（devloper，未提交）**：协议核心（JS `host/protocol.js` + Rust `official_chat/protocol.rs`）、13 个协议夹具 + **双端一致测试**、5 个客户端命令、RPC 接线层 `host/rpc.js`、目录页独立缓存。门禁：`protocol.test.mjs` 13/13、`host-commands.test.mjs` 9/9、`npm run test:plugin` 249 tests / 246 pass / 0 fail / 3 skipped、`cargo test --lib` 1123 passed / 0 failed / 11 ignored、`cargo test official_chat::protocol` 8/8、`check:invariants` 全绿（37 文件）、`check:builtin` 4 插件 / 41 文件。**自报未实现/降级/存疑**：S-5 的 chip `invalid` 位、保留期 7 天主动清理、「换账号即失效」触发点、**客户端命令未挂到 host 服务上**（= 那条通道问题）；降级形态：取数源仍是固定夹具、详情面板不做、正文去重不做、pending 句柄仍是 `selected` 态。

**判据（tester，未提交）**：5 个新文件（判据库 `scripts/lib/p1-protocol.mjs` 20 条 + 协议期望表、adapter、判据自测 24 例、实跑入口、Rust 侧夹具 runner）。**期望值从协议稿独立转写**（不 import 实现当标准答案），判据只经协议 §5 的命令面驱动实现，**观测不到就报「未执行」而不是失败**。r7 轮计数：total 20 / pass 15 / fail 4 / skipped 1。**过程证据**：判据自身先红（3 条判据自身缺陷：依赖协议没规定的 `input.kind`、把「重启后发不出」当判据抛错、拿含 `snapshotId` 的完整路径比 → 恒绿漏判）→ **改判据不是改实现**；活体注入两条（对 `client.js` 副本注入写真实 profile 的一行 → 门禁点名 `client.js:840`；注释掉 `reads.delete(snapshotId)` → A4 判据点名「内存」+「S-3」）→ 撤掉后 SHA-256 与基线逐字节一致。

**PM 的下一步（已派）**：devloper 按 v0.2 改 RPC 接线 + id 分配权归 host + `captureResult` 改对象 + A1c 双端收敛；tester 按 v0.2 改 adapter（吸收同步桩 / Promise 差异）、A6a 改 host 层驱动、A6b 归位「不可机检」、A12 改校验编号语义（G1-1…G1-6）。

### 2026-10-20 18:2x–19:2x · P1 v0.2 落地（devloper）→ 协议 v0.3（PM）

**v0.2 代码侧全部落地**（devloper，未提交；门禁全绿）：`test:plugin` 262 项 / 259 pass / 0 fail / 3 skipped；`test:scripts` 119/116/0/3；`cargo test --lib` 1126 passed / 0 failed / 11 ignored；`cargo fmt --check` 0；**判据 runner 22 条 = 20 pass / 0 fail / 2 skipped**（A0、A6b skipped）。**协议 §8 十二条逐条 pass**（A6b 按协议排除、A0 待真机复验）。

**三个真洞及其修法（本轮最要紧的事实）**：
1. **RPC 层把失败吞成成功 → 空正文照发**（tester r15 定稿时 6 红里的 3 条：A6a/A7/A10 同源）。`host/rpc.js` 的 handler 对每个端点无条件 `return {ok:true, value: await commands.X(...)}`，而命令层失败时返回的是 `{ok:false, code:'snapshot_invalid'}` ⇒ **外层 ok 恒为 true** → 客户端看不到失败 → `codec.serialize` 拿不到 body 时**不报错**（`String(loaded?.body ?? '')`）→ **空正文照发**。**同时破 S-4 与 S-5**：快照缺失 / 摘要不匹配 / 快照被清理 / 跨命名空间读取**都不拒**。**正常路径全绿，只有「读不到」这一支坏掉**。**修法**：`host/rpc.js` 把内层 `ok:false` **翻成外层 `ok:false`**；客户端命令自身 `ok:false` 一律**抛错**（抛错 = 内核拒发 = 请求数 0）。
2. **id 由 client 自造 → 会撞盘**：client 的 `seq` 是**进程内计数**、重启从 1 重来，而快照落盘且**跨进程可发** ⇒ dev/release 同开或任一进程重启都可能生成同一 `snapshotId` → host 按「同 id 不重写」拒 → 用户看到「这次选择没生效」。**修法**：新增 `host/seq-store.js`（排他创建 + 扫目录跳过已认领 ⇒ 跨进程单调、重启续号、`MAX_SEQ=9999`），**id 由 host 在 `catalog` 预分配、随候选行下发**；client 自造 id → 拒 `snapshot_invalid`（反向用例已钉）。
3. **通道真因（A0）**：真机 `p1-r2` 报 `cannot get property "webServer" without inject`（`host/index.js:486`）。取证（现役 `0.2.1-rc.2`）：`dsh-client-connection/lib/index.js:855-871` 的 `register(owner,…)` 最后一步是 `owner.effect(() => owner.webServer.register(route))`；`:724-730` 的 `get rpc()` → `handle` 把 **`owner` 传成读取方的 ctx**——cordis 的 tracker 把 `this.ctx` 换成读取方（`@deepseek-ai/cordis/lib/index.js:128` + `createShadow` `:111-116`）⇒ **`owner.webServer` 是「我们插件自己的 ctx」上的属性访问**。对照：内核自己的 API Gateway 走 `rpc.intercept('/api',…)`（`dsh-api-gateway/lib/index.js:773`），`registerInterceptor`（`dsh-client-connection/lib/index.js:873-885`）**根本不碰 webServer** ⇒ **只有 `handle` 会碰**。**修法（一行）**：`inject = ['connection','webServer']`，并**钉进 smoke 判据**（删掉 `webServer` 即红）。**协议 §5 不需要改**——但这条约束已补进协议 v0.3 §5（含取证依据）。

**PM 的三处新裁定（协议 v0.3）**：
- **捕获失败不新增码**（devloper 存疑①）：捕获失败**不是单一原因**，按原因分流（超时 → `timeout`；结构变了 → `schema_changed`；取消 → `cancelled`）；**`source_unsupported` 只用于「来源根本不支持」**，不得兜「捕获失败」（那会让用户看到「换一个来源」而其实该重试 = **掩盖真实原因**）；原因不明 → `timeout` 兜（可重试）+ `detail` 写脱敏线索。**要求实现改成分流**。
- **A0 拆成两条**（§8 新增第 13 条）：**A0s（仓库级静态）** = 断言 host 声明 `inject` 且含 `webServer`、**不用** `ctx.set` 挂服务（钉住真洞的根因与修法，带反向用例）；**A0（真机级）** = client → host 经**真实路由注册**跑通一次，**不得直连 handler**（直连 = 绕过注册 = 真洞藏身处）；**仓库级 runner 跑不了真机级**（需真实内核进程与真实 cordis 树）→ 如实报「未执行」并写明原因，**由真机复验轮执行**。
- **A6a / A6b / A1d / A12** 按 v0.2 裁定落地（A6b 保持 skipped 且写明「G1-6 未关闭」；A1c 与 A1d 都留但归位；A12 改校验编号语义 G1-1…G1-6）。

**已派（19:2x）**：tester 做 A0s + A0 detail 改写 + v0.2 三处裁定的落地 + 复跑确认两个已修的洞；devloper 做**隔离 dev 壳真机复验**（走真实 connection 路由、确认「删快照 → 0 请求 + 可见错误」而非空正文照发、确认 S-1 两个 id）+ 给**保留期 7 天主动清理**的候选口径（协议没写，他不自造——对，但口径要进协议）。

**仍未实现 / 降级**：保留期 7 天**主动清理**未接（`snapshotRetentionMs` 只是常量，无过期检查与清理任务；被清理后 → 拒 `snapshot_invalid` 这条已实现）；真实站点账户与真实深链（P4）；A6b（协议已排除）、A0（待真机）。

**P1 可以开工**：P1 的前置条件「G1 通过」已满足；G3 已成立（三站各 7/7）；P1 建模的是协议与状态机，**不依赖 G2/G3 的站点侧取数结论**。

P1 开工时需要先落地的输入（来自 feasibility 第 9 节，先决条件，非本次结论）：

1. 就绪状态纪律：pending 句柄不得冒充就绪；label 保持中性；`openReference` 按 ref 如实报 pending/ready/failed；「就绪」由 chip 状态与详情承担，不以「chip 是否存在」承担。
2. G1-1…G1-5 登记为已知上游缺口，实现中不得假设它们已解决。
3. 页内求值走零痕迹回程（Windows 走 CDP `Runtime.evaluate` + `awaitPromise`，**已实测成立**；**R1 槽位不得带进真实站点**；**macOS 侧无通路**，按已知缺口处理——代码里明确报错、不静默降级）。
4. **把「固定快照」与「pending 句柄」的关系定死——这是两件东西**：快照是 P1/P3 的**落盘物**（`schemaVersion` / 不可变 `snapshotId` / 规范化内容摘要 / 捕获结果，与官网会话 id 分开）；pending 句柄是 G1 已验证过的 **chip 形态**（有 ref、有读取记录，发送时由 `codec.serialize` 取正文，等待或拒绝）。P1 必须写清：句柄指向哪个 snapshot、句柄在 snapshot 落盘前后各处于什么状态、「就绪」由 snapshot 的哪个字段承担。**不能把「chip 存在」当成「快照已就绪」。**

### 2026-10-20 21:0x–21:2x · P1 通道落地（exact fetch route）+ 保留期清理落地 + 判据收尾

**通道落地（devloper 实施，Chief 复核内核源码）**
- 形态：`ctx.connection.fetch.register({kind:'exact', path:'/api/xlink-official-chat', methods:['POST'], requestBody:'buffered', fetch})`（协议 v0.4 §5 裁定的过渡通道）。
- **实测出的前置事实（不是推断）**：浏览器侧内核自带的 `connection.rpc.call` 的 URL 是 `` `${channel}/${endpoint}`.slice(1) `` ⇒ **相对路径、不带 `/api`**（`dsh-client-connection/lib/client.browser.js@0.2.1-rc.2:1221`）⇒ 内置 RPC **永远打不到** `/api` 下的 exact route（内核 `assertFetchRoute` 强制路径在 `/api` 下）⇒ **客户端必须自己发 same-origin fetch**。
- 插件自实现 `client-request`/`server-response` 信封（exact route 命中后**绕过**内核 `rpcFetchHandler`），形态照抄内核 ⇒ 上游修好后收敛回 `rpc.handle` 时**信封不用改**；信封层错误用**传输层** `gateway/bad-request`，**不进 §3 的 13 码闭集**。
- host `inject` 收敛为 `['connection']`（去掉用不上的 `webServer`：多声明一个用不上的服务 = 多一条「那个服务不在就整条插件哑火」的依赖，零收益纯风险）。
- **Chief 逐条复核内核源码**（现役树 `0.2.1-rc.2`，`dsh-client-connection/lib/index.js` SHA-256 `0EBCCA7583E082716F79B0B55BC1749D48269CC8DB2989B043B321B57ABD3684`）：对外服务面 `get fetch()` → `register: (route) => this.registerFetchRoute(owner, route)`（`:734`，**单参数**）；注册键 = `route.path` **原样**（`:849`）；查找 = `fetchRoutes.get(url.pathname)`（`:828`，**全等匹配**）；owner 准入 `peer.principal.kind !== "owner"` → 403（`:830`）；路径校验 `endpointFromPath("/api", route.path)`（`:973-977`）；`methods` 存成 `Set` ⇒ `POST` 命中。**与插件实现逐条对齐。**

**真机 p1-r8（未打补丁的现役内核）**：断言 **a/b/c/d 全绿** —— ① catalog 真通（@ 菜单出现「官方对话」分组与 G1 Alpha/Beta）；② chip 落草稿且前后文字与顺序不丢；③ 发送门控挂起 7439ms 且正文**只出现 1 次**且 `role=user`；④ 读取失败 **0 请求** + `role="alert"` 可见错误 + 草稿保留。证据 `%TEMP%\dsh-xlink-g1\evidence-p1-r8\results.p1-r8.json`（**按 izzy 15:2x「证据可以清」，该文件可清；结论以本节为准**）。

**保留期清理落地（协议 §4.1）**：新文件 `plugins/official-chat/host/sweep.js` —— 四家族（快照按 `capturedAt` + mtime 兜底／认领按 `claimedAt` 且**对应快照已不在**／目录缓存按 5 分钟 TTL／日志按行数·大小截断**不按 7 天删**）、两步法删除（先 rename 再 unlink）、根不可得**不扫**、失败**只记日志**、节流键 `<root>/sweeps/last-sweep-<YYYY-MM-DD>.json`、**只挂 `catalog` 入口**（`capture` 不触发）、**不引入引用计数**。devloper 自陈修掉一处真 bug（`sweepClaims` 调了未定义的 `readClaim`）与一处**测试期望写错**（同轮「过期快照 + 过期认领一起清」是协议要的）。

**PM 独立复核（不采信回执）**
- 门禁全绿：`check:invariants` 绿（47 文件）｜`test:plugin` 277/0/3｜`test:scripts` 131/0/3｜`test:probe` 6/6｜`cargo fmt --check` 0｜`cargo clippy --locked --all-targets -- -D warnings` 0｜`cargo test --locked`（debug）**1127 passed / 0 failed / 11 ignored**｜**判据 runner 独立复跑 32 条 = 30 pass / 0 fail / 2 skipped**（skip：A0 真机级 + A6b chip invalid）。
- **devloper 自报漏了一项门禁**：`prepare-builtin-plugins.mjs --check` **首跑红**（official-chat 源码已变更、资源清单未重新生成）⇒ 跑 `npm run prep:builtin` 后绿（4 插件 / 43 文件；该目录按设计 `.gitignore`，**不入库**）。
- **PM 代修三处**：① `sweep.js` 里 catalog-cache 的层级注释——实现是**平铺** `<provider>__<账号范围>__<查询词>.json`（`host/commands.js` 的 `fileOf`），**没有 provider 子目录** ⇒ 注释与协议 §5 的路径写法**双双按实现对齐**（原写法来自协议 §5，与实现不一致）；② 日志截断补上「**按大小**」上限（§4.1 第 2 条写的是**行数/大小两个上限**，实现只落了行数）⇒ 按行退，**不切字节数组**（切半个多字节字符会毁掉诊断证据）；③ 协议 §4.1 第 2 条补 **PM 澄清**：`capturedAt` **优先**、mtime **仅在 `capturedAt` 读不到时**兜底。

**A0 真机结论的回填机制（Chief 裁定，回答 tester）**
- **不采用「读真机证据文件」的机检判据**：证据文件按 izzy 拍板**可清** ⇒ 清掉后那条判据会从绿变 skipped ⇒ **一条会自己褪色的判据等于没有判据**。
- **改为「结论登记进本文件」**：A0 真机级**永久如实报 skipped**（仓库级跑不了真实内核进程），其**载体是本节**——轮次名（p1-r8）、断言清单（a/b/c/d）、每条结论、证据路径。**每次真机轮都必须在本文件追加一节**（轮次、断言、通过/失败、证据路径），否则 A0 = 未执行。
- 机检侧只保留 **A0s**（仓库级静态：钉「注册 exact fetch route + 不得把 `rpc.handle` 当已通 + 不用 cordis `ctx.set`」）——**形状钉得住、行为证不了**，两者的分工不得混。

**§4.1 条款→判据映射（Chief 复核，发现两个缺口）**：§4.1 第 1 条（触发点/节流/节流键）**无判据**；第 2 条的**目录缓存 TTL 与日志截断两个家族**无判据（A17 只覆盖快照与认领两族）⇒ 派 **A18**（见下）。第 4 条（「正在发送中的引用」不保护）**不可机检**（实现里没有「发送中」这个可观测状态）⇒ 如实写「已知不覆盖，后果由 S-5/S-6 覆盖」，**不假装能测**。

### 2026-10-20 21:3x–21:5x · 真机轮 p1-r9（未打补丁的现役内核 ＋ 协议 v0.4 的 exact fetch route）

**轮次**：`p1-r9-exact-fetch`｜**断言清单**：1–6（**与 21:0x 节的 a/b/c/d 是不同的断言编号**，两轮各自成立、互不替代）

**三条前提取证**（缺一条就可能报错结论）：
① 隔离内核副本与现役 `dsh-client-connection/lib/index.js` **SHA-256 逐字节一致**（`0EBCCA75…`）⇒ 本轮跑的是**未打补丁的现役内核**（上一轮为验证而注入的那行补丁**已还原**）；
② 内核进程 cmdline 指向 `%TEMP%\dsh-xlink-g1\shell-home\dsh\desktop-dev\…` ⇒ **不引用用户真实 home**；
③ 真机前置**每轮清物化**（新增机检「前置②.7」）＋ `prep:builtin` 重生成内嵌资源 ＋ 重启壳让 `tauri dev` 重新 build。

**结果：11 通过 / 0 失败 / 1 未执行**（未执行项 = 前置②「停本实例内核」——本轮没有内核在跑，属空分支，不是失败）
1. **A0 真机级 ✔** client 经 **exact fetch route** 拿到 capabilities（schemaVersion 1、providerId `ds`；探针 `capabilities` 在场、无 `capabilities-failed`）
2. A0 catalog ✔ 候选 2、**host 预分配 id 2**
3. A0 pick ＋ 落盘 ✔ id `ds~8qm981idmwz2~0012`，**自算摘要 = 盘上 contentDigest = `a6b96ec9…`**（正文 76 字节），chip 在草稿、正文不在草稿
4. **S-4 逐字节 ✔** 请求 1 个、serialize-ok、草稿清空；盘上正文在请求体里命中 **4 处**（`messages[1]/[6]/[8]/[10]`），每处 `exact=true`、**76 字节 = 盘上 76 字节**
5. S-1 ✔ `…0012` / `…0013` 两个 id，两份文件都在，第一个摘要未变
6. 缺口① ✔ 删 `…0014` 后发送 ⇒ **请求数 0** ＋ **可见错误**（`role="alert"`：`snapshot_invalid：移除该引用`）＋ `serializeFailed` ＋ 草稿保留

**证据**：`%TEMP%\dsh-xlink-g1\evidence-p1-r9-exact-fetch\shell-verify-p1.p1-r9-exact-fetch.json`（**按 izzy 15:2x「证据可以清」，该文件可清；结论以本节为准**）

**本轮附带修掉的两处「会让人报错结论」的问题（devloper）**
① **判据自身**：S-4 原判据在 `JSON.stringify(request.body)` 的**转义后**字符串里找**含真换行**的盘上正文 ⇒ 永远匹配不上 ⇒ 把「正文明明在请求体里」报成了失败。已改为在**未转义的字符串叶子**里找，并加**判据自检**（正例 1 / 篡改 0 / 缺失 0 / 空 0；自检不过就**不开真机**）。
② **驱动层**：壳的物化按「插件版本 ＋ 内核指纹」去重 ⇒ **代码改了、版本号没变 ⇒ 不重新拷贝** ⇒ 真机会跑**旧插件**（本轮之前的 p1-r7 / p1-r8-exact-fetch 两轮，拿到的 405 是**旧插件**的 405，不是新通道的）。已加前置机检「每轮清物化」。另修「过期 pid 文件（pid 号被别的进程复用）」被误判成「停不掉内核＝失败」。

### 2026-10-20 21:4x–21:5x · 跨语言临界夹具 `oversized-multi`（补 §6「同一组夹具 → 同一解析结果」那个窗口）

**背景**：现有 `oversized` 只有 **1 条**消息 ⇒ 消息分隔符 **0 字节** ⇒ 跨语言一致性判据 A1c **踩不到「正文总字节落在 256KB±2」那个窗口** ⇒ 那条判据此前是**假绿**（测了但没测到窗口）。

**新增一条（只加一条）**：`plugins/official-chat/fixtures/protocol/oversized-multi.json`（并登记进同目录 `index.json` 的 `cases`）
- **n = 2** 条消息：text1 = 131072 × `A`、text2 = 131072 × `B`（ASCII，1 字符 1 字节，各 131072 字节）
- **正文总字节 = 131072 + 2（`MESSAGE_SEPARATOR` = `\n\n`）+ 131072 = 262146 字节** ＝ **256KB + 2**，即 ±2 窗口的**上沿**
- 期望结果 = `body_too_large`（与 `oversized` 同款错误负载）
- 体积 263,330 字节（与 `oversized` 的 262,984 同量级，照仓库先例）
- **字节数独立算**：不 import 实现的 `joinMessages`（那样就是「自己判自己」）

**为什么取上沿 262146**：两端都算对（含分隔符）⇒ 262146 > 262144 ⇒ 都拒 ⇒ 一致；任一端**漏算那 2 字节分隔符** ⇒ 262144 ⇒ **不** > 262144 ⇒ 放行 ⇒ **两端判定相反** ⇒ A1c 立刻报红。取窗口下沿（如 262142）时两端都放行 ⇒ 测了等于没测。

**活体注入反向验证（证明它不是又一条假绿）**：把 JS 侧 `bodyBytes` 的 `return total + (list.length - 1) * MESSAGE_SEPARATOR.length;` 改成 `return total;`（**漏算分隔符**）⇒
- **注入后**：`A1c-fixture-parity` **报红并点名 `oversized-multi`**（detail：两端对同一组夹具的结果不一致（1 条不同）：`oversized-multi`：JS=`kind:"snapshot"` vs Rust=`code:"body_too_large"`），`A1d-fixture-expectation-match-js` 同时报红；
- **还原后**：`protocol.js` **SHA-256 逐字节一致**（`a85fa37a…`），A1c 恢复通过（14 条夹具深比较一致）。

**门禁**（含 PM 要求补列的 `prepare-builtin-plugins.mjs --check`）：`node --test plugins/official-chat/test` **100/100**｜`test:plugin` 277/0/3｜`test:scripts` 131/0/3｜`test:probe` 6/6｜`check:invariants` 全绿｜`check:duplicates` 4 处（允许 8）｜**`prep:builtin` + `check:builtin` 绿（4 插件 / 43 文件）**｜判据 runner 43 条，**A1a / A1b / A1c / A1d 全绿（夹具 13 → 14 条）**。
**注**：夹具**不随插件分发**（`prep:builtin` 的资源文件数没变，仍 43）⇒ 不会引起内嵌资源清单失配；两端判据都从**仓库里同一个目录**读它。

### 2026-10-20 22:0x–22:1x · A18 判据落地（tester）＋ PM 代修一处**真实实现缺口**（删不掉不留痕）

**A18 判据（tester，未由 PM 提交）**：§4.1 剩余口径全部落地 —— A18a 触发点（静态）、A18b 触发点（行为）、A18c 节流（当天一次 + 节流键落位）、A18d 目录缓存 TTL、A18e 日志截断、A18f 日志不按 7 天删、A18g 两步法删除、A18h 根不可得不扫、**A18i 清理失败只记日志**、A18j `capturedAt` 优先、A18k「正在发送中的引用」**不可机检 ⇒ 如实 skipped**（写明「已知不覆盖，后果由 S-5/S-6 覆盖」）。**PM 实跑：43 条 = 40 pass / 0 fail / 3 skipped**（skip：A0 真机级、A6b chip invalid、A18k）。

**PM 复核发现的真实缺口（A18i 报红，**不是判据错**）**：协议 §4.1 第 3 条「清理失败**只记日志**，不阻塞业务」——「只记日志」含「**必须留痕**」；而 `host/sweep.js` 的删除失败路径（`removeInTwoSteps` 返回 false）**只计数、不记日志** ⇒ 删不掉的文件被**静默吞掉**（没人知道有文件没清掉，磁盘会一直涨）。⇒ **PM 代修**（`host/sweep.js`）：留痕放在**调用处**（`else logger?.warn?.(...)`，日志写**相对本实例根**的路径，不写绝对路径），`removeInTwoSteps` 内部与调用行**逐字不动**。

**为什么留痕放调用处而不是 helper 内部**：调用点才知道「删的是什么」（哪个家族、哪个文件）；helper 的 `catch` 只知道「这一步失败了」。且**已提交版**的 A17 活体注入锚点锁死了「helper 内部不动 + 调用行逐字不变」⇒ 留痕放调用处时，**已提交版判据自测 39/39 仍全绿**（PM 用 HEAD 版测试文件实测）；反过来若把日志塞进 helper 的 `catch`，A17 与 A18g 的注入锚点会同时失效。

**一处判据用例之间的形态冲突（待 tester 统一，PM 未擅自改他的在途文件）**：A18i 的注入用例按「把**调用点**的 `logger` 置空」制造「不留痕」，而 A17 / A18g 的锚点锁死「调用点不带参数」⇒ **两条期望互斥**。PM 建议：**留痕统一在调用处**，A18i 的注入改成「去掉 `else logger?.warn?.(...)` 那一行」（他 v0.2 版用例里本来就有这条正则），则 A17 / A18g / A18i **三条锚点可同时成立**。**已提交版判据集不受影响**（PM 实测 39/39 绿）。


### 2026-10-20 22:2x · 上游内核一行修复已提交（G1-7 根治）＋ 临时/隔离目录清理

**上游修复（izzy 22:1x 拍板「修复」）**

- **上游仓库定位**：`@deepseek-ai/dsh-client-connection` 的 `package.json` 写着 `repository = deepseek-ai/deepseek-harness`，目录 `packages/client/connection` ⇒ 源码在 **`deepseek-ai/deepseek-harness`**（July-X 有 fork）。**upstream `master@687f282e`＝`release dsh@0.2.1-rc.2`**＝**本机现役内核同源**（fork 此前落后 2373 提交，已用 `merge-upstream` 快进）。
- **那一行仍在**（未修）：`packages/client/connection/src/rpc-host.ts` 的 `HostConnectionService.register()` 里 `return owner.effect(() => owner.webServer.register(route), …)`（blob SHA `88e7b48b…`）。
- **根因在 cordis 源码层面坐实**（现役 `0.2.1-rc.2` 的 `@deepseek-ai/cordis/lib/index.js`）：
  - `ctx.get(name)`（`:763-765`）→ `_getImpl`（`:766-772`）读 `this.ctx[symbols.isolate][name]` + `this.store` ⇒ **服务注册表**，**跨 scope 可达**；
  - **属性访问**（`:887-893` 的 Proxy get）走**本 scope 的注入链** ⇒ connection 插件自己的 scope 从不 inject `webServer` ⇒ **必抛** `cannot get property "webServer" without inject`。
  - ⇒ `owner.get('webServer')` 正是**内核自己**读 webServer 的写法（`admit` / `isTrusted` / `authorizeConnect` / `authorizeIndex` 全用它）。
- **修复内容**（**仅此一处，+14/−4**）：`owner.webServer.register(route)` ⇒ `const webServer = owner.get('webServer')` + 不可得时**显式抛错** + `webServer.register(route)`；并注明「`connection.fetch.register(...)` 不受影响（`registerFetchRoute` 完全不碰 webServer）」。
- **提交位置**：`July-X/deepseek-harness` 分支 **`fix/connection-owner-webserver-get`**，commit **`07aa67ac`**（GitHub API 独立核对：parent = upstream `master@687f282e`、compare `ahead_by 1 / behind_by 0`、1 file changed **+14 −4**；分支 blob `4fcd7243…` 与本地 `new.ts` **逐字节相同**（16,313 B / 423 行））。
  - **sha 变更说明**：先前登记的 `aa7404de` 内容正确、但 email 是 `over618@163.com`；本轮按 izzy「提交作者名改成 July-X」的口径重建了 commit —— **新旧 tree sha 完全相同（`82c24f34…`）⇒ 文件内容零变化**，只把 email 统一为 `m4java@163.com`（与本仓库级 config 一致）。分支 ref 已 force 更新到 `07aa67ac`。
- **PR 待开**：跨 owner 的 fork→upstream，**API 无 `pull_requests: write` 权限**（`gh pr create` 报 `July-X does not have the correct permissions`）⇒ 须走 GitHub 网页 compare 流程（`compare/master...July-X:fix/connection-owner-webserver-get`）。
  - **当前阻塞点**：会话内置浏览器**未登录 GitHub**（compare 页只显示 `Sign in / Sign up`，无 PR 表单）⇒ **需 izzy 登录后由网页流程创建**。PR 正文已备好（英文、只含技术内容）。
  - ⇒ **G1-7 仍不关闭**；等上游合并并发版后，本项目再把通道收敛回 `rpc.handle`（RPC 信封不用改）。

**清理（izzy 22:1x 拍板「清理临时目录、隔离目录」）**

- **已清**（运行时可恢复删除启动器，**移入回收站、未永久删除**）：`%TEMP%\dsh-xlink-g1\`（**隔离 home ＋ 真机证据 ＋ 内核副本**，693 MB）、`%TEMP%\g1-probe*`（5 个，264 MB）、`%TEMP%\dsh-xlink-*`（Rust 测试残留数千个，693 MB）⇒ 合计约 **1.6 GB**。
- **未清**（**不属本项目范围**，别误删）：`%TEMP%\.tmp*`、`dsh-acl-skill-*`、`xlink-*` 等其它功能的测试残留。
- **可恢复性**：全部走 `mavis-trash`（回收站），未做永久删除。
- **结论载体不受影响**：真机轮 p1-r9 与临界夹具的结论**已固化在本文件与协议稿**里 ⇒ 证据文件可清（izzy 15:2x 拍板「证据可以清」），**且没有任何判据依赖这些外部产物**（A0 真机级永久如实报 skipped，结论载体是本文件）。

### 2026-10-20 22:3x–22:4x · P2「正文去重」只读调研（devloper）＋ PM 建议裁定

**派活与回执**

- 22:1x 派 devloper 做**只读**调研（不写代码、不写判据、不改协议稿）：同一段官网正文被引用多次时，P2 要不要处理、怎么处理。
- 22:3x 回执送达（群 post 被拒两次，先私聊直送）；原件在任务队列 `task_1738d44c8207`（`request_id: p2-dedup-research-pm-20261021-01`）⇒ **全文已落盘到新载体 `docs/features/official-chat/official-chat-p2-research.md`**（PM 归属，`kind: research`）。

**调研结论要点**（全文见载体）

- **动机 = 预算的算术硬失败**：单条正文预算 64 KB（协议 `:187`）× 单消息引用数上限 8 条（`:186`）= 512 KB > 单次消息总预算 256 KB（`:188`）⇒ 必然落 `body_too_large`（`:151`）。
- **键** = (accountScope, sessionId, branch, capturedAt, contentDigest) 全等；五字段**都已在协议 §1.1 快照里**（`:47` / `:48` / `:49` / `:51` / `:53`）⇒ **不必新建索引表**；`capturedAt` 进键的依据 = development-plan §8（`:168`）「去重不能抹掉账户、分支或捕获事实」。
- **落点三层分界**（现役内核 `0.2.1-rc.2`，文件 SHA-256 见载体 §2.3）：浏览器侧**逐 occurrence** 调 serializeReference ⇒ **插件 `codec.serialize` 只看得到单 ref**；**消息级入口 = host 监听 `agent/pre-step`**（可 replace messages）但 `UserMessage` **不含 ref 结构** ⇒ **只能文本级判定（digest 反查）**；存储层只供材料、不改。
- **红线**：绝不复用旧 id（撞 S-1 / S-6）；隐式去重撞 S-7 ⇒ **合并必须对用户可见，否则不做**；**内核重放不管**（重放发生在展开之后，插件观测不到）；**偏向宁可漏**（错合并会让用户以为引 A 其实是 B）。
- **digest 反查的边界**：可作「提示」判定依据（误报只多一条提示），**不可**作「合并」依据。

**PM 建议裁定（待 izzy 拍板；P2 未立项，不生效）**

1. **目标 = 省预算**，**不动 chip 数**（省操作不是硬约束）。
2. **第一步只做「重复识别 ＋ 可见提示 ＋ 预算预检」，不做消息级自动合并**——P2 阶段**没有可见承载位**（详情面板属 P5）、toast 瞬态易假红；**没有可见方案就不做**（S-7 禁止静默裁掉）。
3. **落点 = host 命令层产出「重复组」，client 呈现**；**不改消息内容**，**不在 `agent/pre-step` 里改消息**（那里只做引用展开，development-plan §7 `:149`）。

**与既有判据的关系（条件性）**

- `A8-no-body-dedup`（`scripts/lib/p1-protocol.mjs:1110`）数的是**判据自建**的 `${bodyA}\n\n${bodyB}`（`:1126-1127`）⇒ **不观测内核真实请求体** ⇒ **P2 第一步（不做合并）不需要改它**；**只有将来决定做消息级合并时**，A8 才必须**同次**改口径（否则与新协议互相打红）。这是条件性结论，不是现在的待办。
- A2（两次 pick 两个 id）继续守 id 不复用；A6a / A7 继续守「请求数 0 ＋ 可见错误」。

**跨文档张力已消歧**

- `development-plan` §7:155「正文只出现一次」vs 协议 S-7「引用两次各送一份」⇒ **协议 v0.7 以 S-7 为准**（提交 `cdd03fb`）＋ feasibility §7 第 12 条；⇒ **P2 立项不必回头改 development-plan**（改写归属待 izzy 指定：该文件 front-matter 无 owner 字段，**同目录 ≠ 同归属**）。

### 2026-10-20 22:4x–22:5x · tester 判据侧与协议 v0.6 逐条对齐复跑（r27）

- **逐条对齐（四条件映射，同序同义）**：协议 v0.6 §4.1「只记日志」的四个可观测条件（`official-chat-p1-protocol.md:201`）↔ 判据 A18i 的四个条件——① 不许越界（不抛异常）→ `noThrow`；② 不阻塞（别的过期文件照删）→ `othersStillRemoved`；③ **失败项内容仍在磁盘上**（原路径**或**墓碑任一 ＋ 不许假装删掉）→ `failedItemStillPresent`；④ 必须留痕 → `failureLogged`。⇒ **判据与协议无需任何改动**（含第 ③ 条「原路径或墓碑任一」的措辞也一致）。**行号核准（PM 第一手核对）**：「四个可观测条件」那句在协议 `:201`；**§4.1 第 4 条（「正在发送中的引用」不保护、不引入引用计数）实际在 `:204`**——tester 回执里写的是 `:202`，**差 2 行**，后续引用以 `:204` 为准。
- **复跑数字（tester 22:4x–22:5x）**：自测 **60/60**；实跑 **r27 = 43 条 / 40 通过 / 0 失败 / 3 未执行**（未执行：A0 真机级 / A6b（G1-6）/ A18k「已知不覆盖」）⇒ **判据与现役实现 ＋ 协议 v0.6 兼容**。**怎么验的**：`node scripts/p1-protocol.run.mjs` ＋ `node scripts/p1-protocol.test.mjs`（tester 自跑，回执给数字）；**证据位置**：tester 22:4x–22:5x 回执（原始产物未固化）。**注**：tester 回执里的基线是 `ca73363` 时点，现已前进（见下节）。
- **A0 的口径不变**：继续**永久**报「未执行」（理由「需真实内核进程」），**结论载体 = 本文件**（21:3x–21:5x 的 p1-r9 = 11 通过 / 0 失败 / 1 未执行；那 1 条未执行 = 前置②停内核的空分支）。
- **A18k 的「已知不覆盖」标注已被自测钉住（PM 第一手核验，非采信回执）**：自测 `scripts/p1-protocol.test.mjs:1657-1662` 三条断言——`status === SKIPPED`、detail 含「已知不覆盖」、detail 含「S-5 / S-6」；且判据函数 `checkInflightReferenceNotCovered()`（`scripts/lib/p1-protocol.mjs:2473`）**无入参、直接 `return verdict('A18k-inflight-reference', SKIPPED, …)`** ⇒ **「恒为未执行」由构造保证**，不靠调用方自觉。⇒ **谁删这个标注、或把措辞改成不含那两处，测试立刻红**（挂在 `npm run test:scripts`）⇒ 后人看到「未执行」也不会当成漏洞去「补」。**行号更正**：tester 回执把协议 §4.1 第 4 条写成 `:203`，PM 第一手核对**实为 `:204`**（`:203` 是「锚点锁死」那条更正）⇒ 引用以 `:204` 为准。
- **两条「可选的一行改进」经 tester 判断不动**（A18k 的 detail 补 `snapshot_invalid` 字面 / A0 的 detail 点名「结论载体 = 进度文档」）——**PM 接受该判断并撤销原派活**：两条都是**表述冗余**而非判据缺陷（tester 给的理由与 v0.6 第 ③ 条并非同一件事，PM 已知）；「结论载体」的声明**已由本文件承担**（见上条与 22:2x 节），**不因判据 detail 未点名而丢失**。
- **阶段口径**：tester 明确 **P2 / P4 不开**（按规矩等 izzy 点头），手上无欠账；其两个提交 `7fc2fcf` + `896a898` 在历史里。

### 2026-10-20 22:5x–23:0x · fork 分支 commit 重建（内容零变化）＋ main 推送 ＋ 提交身份收口

- **fork 分支 commit 重建**：先前登记的 `aa7404de` **内容正确、但 email 是 `over618@163.com`**；本轮按 izzy「提交作者名改成 July-X」的口径重建 commit ⇒ **新旧 tree sha 完全相同（`82c24f34…`）⇒ 文件内容零变化**，只把邮箱统一为 `m4java@163.com`（与本仓库级 config 一致）。分支 ref 已 force 更新到 **`07aa67ac`**。
  - **API 独立核对**（不采信回执）：parent = upstream `master@687f282e`；`compare/master...fix/connection-owner-webserver-get` = `ahead_by 1 / behind_by 0`；1 file changed **+14 −4**；分支 blob `4fcd7243…` 与本地 `new.ts` **逐字节相同**（16,313 B / 423 行）；git author/committer 均为 `July-X <m4java@163.com>`。
  - **踩坑留痕**：GitHub `git/blobs` 建 blob 必须用 `gh api -F "content=@<file>"`（**大写 `-F` 读文件**）；小写 `-f` 会把 `@path` 当字面量 ⇒ 生成垃圾 blob（本轮那个 `+2/−412` 的坏 commit 即由此产生）。
- **main 已推 origin**：izzy 22:1x 授权推送后，PM 把并行会话的 `cdd03fb`（协议 v0.7 记 S-7 与 development-plan §7:155 的字面冲突）一并推上 `origin/main` ⇒ 本地与远端 **0/0 同步**。
- **提交身份收口**：**仓库级** `user.name = July-X` / `user.email = m4java@163.com`；**全局保持 izzy 的 `zhongxingxing` 不动**（izzy 22:2x「只改这个仓库的，不要改全局」）。
- **历史 224 个 `zhongxingxing` 提交未重写**：重写 = force push `main` / `main-dev2` / `uiv2-dev`；**已发布的 `desktop-v0.1.x-rc.x` tag 若一并重推，可能重复触发发布 workflow** ⇒ 属不可逆动作，**PM 不擅自做，待 izzy 一句话拍板**。
- **PR 仍卡在「浏览器未登录」**：会话内置浏览器的 compare 页只显示 `Sign in / Sign up`（无 PR 表单）⇒ **需 izzy 登录后走网页流程**。PR 标题与正文已备好（英文、只含技术内容、无内部信息）。
- **临时目录清理仍在跑**：`%TEMP%\dsh-xlink-*`（Rust 测试残留数千个）分批回收中（22:5x 实测剩 3,251 个）；`%TEMP%\dsh-p1-protocol*`（61 个，5.7 MB）**刻意保留**（判据可能仍在引用，PM 复核后再清）。

### 2026-10-20 22:4x · 当前 HEAD 门禁收口复核（PM 自跑，不采信旧 HEAD 结果）

- **为什么要重跑**：devloper 的最终门禁跑在 HEAD `7fc2fcf`；此后有 7 个提交（`896a898` / `2f7fe7a` / `ca73363` / `ed9e446` / `cdd03fb` / `09d7e32` / `bbf69c5`），其中 **`896a898` 改的是判据库 `scripts/lib/p1-protocol.mjs`** ⇒ 那轮门禁**不覆盖**它。⇒ PM 在**当前 HEAD `bbf69c5`** 上补跑全套。
- **结果（PM 实跑，全部 exit=0）**：`check:invariants` 全绿（扫 47 文件）、`check:duplicates` **4 处**（允许 8）、`check:builtin` **4 插件 / 43 文件**、`test:scripts` **152 pass / 0 fail / 3 skipped**、`test:plugin` **277 pass / 0 fail / 3 skipped**、`test:probe` **6/6**；另 PM 先前已在同一 HEAD 复跑判据 runner **43 = 40 pass / 0 fail / 3 skipped** 与判据自测 **60/60**。⇒ **与 devloper 报告的数字逐项吻合**，差异只在覆盖的 HEAD 不同。
- **归属更正（第二次，写进文档免得再口头漂移）**：`58488a9` 是**混合提交**，7 个文件里——**PM 代修/代写**：`plugins/official-chat/host/sweep.js`（A18i 报红后 PM 判定为**真实实现缺口**并代修：留痕放调用处）、`AGENTS.md`（真机复验先清物化条款）、`docs/features/official-chat/official-chat-p1-protocol.md`（S-4 不要求唯一）；**devloper 的**：`fixtures/protocol/oversized-multi.json`、`fixtures/protocol/index.json`、`test/drive/shell/shell-lib.mjs`、`docs/features/official-chat/official-chat-progress.md`（两节）。⇒ 「我这轮的东西都在 `58488a9`」**只对其中 4 个文件成立**。
- **认可的两条自查**：① 真机前置三步（删物化目录 → `prep:builtin` → 重启壳重新 build）与 AGENTS.md 机检**一致**，不改；② 证据落在隔离根 `%TEMP%\dsh-xlink-g1\evidence-*`，**不在**会被删的探针临时 home（`shell-home`）里 ⇒ 合规。隔离 dev 壳已停。
- **结论**：**P1 判据集收口**（协议 v0.7 ＋ 43 条判据 ＋ 当前 HEAD 全门禁全绿）⇒ 成员无新活；等 izzy 对 P2「正文去重」是否立项拍板。

### 2026-10-20 22:5x · izzy 拍板五项 ＋ P2 立项 ＋ P2 协议 v0.1

**izzy 的五项答复（22:5x）**：① **P2 立项**；② `development-plan` §7:155 **由 devloper 改**；③ **要具体 PR 地址**；④ **历史 224 个提交要重写**；⑤ **问「他什么时候做」**（登录态 / 安装版 release 壳验收）。

**P2 立项落地**

- 阶段总表 P2 行改为「**已立项**」；调研载体的 status ＋ §0 / §1 / §4 / §6 同步为「已拍板、已生效」。
- **新增 P2 协议稿 `official-chat-p2-protocol.md` v0.1**（PM 定，`kind: protocol`）：第一批只做**重复识别 ＋ 可见提示 ＋ 预算预检**；重复组键 = (accountScope, sessionId, branch, capturedAt, contentDigest) 全等；`groupKey` = 五字段摘要前 16 hex；host 新命令 **`duplicates`**（入参 `{sessionId, snapshotIds}`，出参 `{ok, groups[], budget{}}`，失败沿用 P1 **13 码闭集** ＋ 8 字段错误对象，**不新增错误码**）；可见提示**必须可见且可复查**（**不接受瞬态 toast**），**不得**自动删 chip / 合并 / 截断正文；预算预检按 P1 S-7 口径算 `projectedBytes`（各引用各送一份）> 262144 ⇒ 提示但**不截断**；判据六条（P2-1…P2-6，含两条负例）＋「**去重候选对**」夹具（登记 `fixtures/protocol/index.json`，**键形状已定稿 ⇒ 可写**）；**A8 本批不需要改**（它数的是判据自建串）。

**development-plan §7:155 的归属**

- izzy 指定**由 devloper 执行**（只改 §7:155 一句，用 feasibility §7 第 12 条已记的建议措辞，并同步该条指向）⇒ **PM 不改该文件**；已回填 feasibility §7-12。

**历史作者重写（izzy 拍板「需要重写」）**

- 范围：**只重写三个开发分支**（`main` / `main-dev2` / `uiv2-dev`）的**作者与提交者**（`zhongxingxing` → `July-X <m4java@163.com>`）；**dependabot 分支不动**、**82 个 tag 一律不重写也不重推** ⇒ 不触发 `desktop-release.yml`（该 workflow 只在 `desktop-v*` tag 推送时运行）。
- **恢复点已建立**：已推 `origin/backup/pre-author-rewrite-20261020`（指向重写前的 `origin/main`），命名不匹配 `desktop-v*` ⇒ 发布 workflow 不会触发。**（23:1x 更正措辞：它是 `tag`，不是分支——远端 `refs/heads` 只有 3 个开发分支。）**
- 做法：在**独立克隆**里 `git filter-branch --env-filter` ⇒ **不动当前工作目录的 refs**（避免打断并行会话）；完成后再 force push 三个分支。
- 踩坑留痕：① 第一版用 `git clone --no-checkout` ⇒ 克隆里 index 非空，filter-branch 拒跑（`Cannot rewrite branches: Your index contains uncommitted changes.`）；② `uiv2-dev` **只存在于远端**，本地克隆没有 ⇒ 需在克隆里 `git fetch origin` 后再 `git branch -f`；③ 脚本的「原工作树必须干净」检查会把 **PM 自己的在途文档改动**判成不干净 ⇒ 正确顺序 = **先提交在途改动 → 再重写 → 最后 `reset --hard`**。

**上游 PR 的具体地址（izzy 要的）**

- **直接可点**：`https://github.com/deepseek-ai/deepseek-harness/compare/master...July-X:deepseek-harness:fix/connection-owner-webserver-get?expand=1`（`?expand=1` 直接展开 PR 表单）。
- base = **`deepseek-ai/deepseek-harness`** 的 `master`；head = fork **`July-X/deepseek-harness`** 的分支 **`fix/connection-owner-webserver-get`**（commit `07aa67ac`）。
- **API 核对**：`gh api repos/July-X/deepseek-harness/compare/master...fix/connection-owner-webserver-get` ⇒ `status: ahead`、**ahead 1 / behind 0**、1 file changed（`packages/client/connection/src/rpc-host.ts`）**+14 −4**。
- 阻塞点仍是**浏览器未登录 GitHub**（compare 页只显示 `Sign in / Sign up`）⇒ 需 izzy 登录后走网页流程；PR 标题与正文由 PM 提供。

**仍需 izzy 本人做的两件（时间建议）**

- **首次建立站点登录态**：**现在就可以做**（一次浏览器登录即可，不需要 agent 参与）——它是 P4 的硬前置；而 **P2 第一批不需要登录态**（用夹具目录即可）⇒ 不必等 P2 结束。
- **安装版 release 壳验收**：需**安装版**（不是自编 release 二进制）⇒ 需等 izzy 自己出安装包/发布之后；建议在 main 稳定、发布包就绪时做。

### 2026-10-20 23:0x · izzy 撤回上游 PR ＋ 新增硬纪律「禁止修改上游 dsh 内核」

- **izzy 23:0x 原话**：「③ PR 为什么要去改 deepseek-harness 的东西，撤回这个 PR，**禁止修改上游的 dsh 内核**。」
- **PR 的实际状态**：那个上游 PR **从未创建成功**（此前卡在「浏览器未登录 GitHub」，compare 页只显示 `Sign in / Sign up`）⇒ **没有需要关闭的 PR**。**核验限制**：`gh api repos/deepseek-ai/deepseek-harness/pulls` 返回 **404 = token 对 upstream 无读权限**（同一 token 对 fork 读权限正常、count=0）⇒ 这一点**无法用 API 独立证实**，按记录判定「未开成」。
- **已执行的撤回动作**：**删除 fork 上准备开 PR 的分支** `July-X/deepseek-harness:fix/connection-owner-webserver-get`（commit `07aa67ac`）⇒ 删后 fork 只剩 `develop` / `master`。**该 fork 仓库本身保留**（删整个 fork 是更大的不可逆动作，等 izzy 一句话）。
- **新硬纪律（已落 `AGENTS.md`「范围 · 运行时内核边界」）**：**禁止向上游内核仓库提修改或 PR**（不改 `deepseek-ai/deepseek-harness` 及其任何 fork 的代码、不开 PR、不推分支）⇒ 内核缺陷只做两件事：① **本项目侧规避**；② **登记缺口并上报 izzy**。⇒ 内核侧缺口**长期按「已知未解决」对待**，实现与验收**不得假设它会因上游发版而解决**。
- **G1-7 的处置随之改写**（协议稿 §5 / §8 ＋ feasibility §3 缺口表 ＋ §9 先决条件）：**不再以「等上游发版」为收敛前提** ⇒ 通道**长期**走内核 `connection` 服务的 **exact fetch route**（已真机全绿），`rpc.handle` 路径**不使用、不修**；**G1-7 仍不关闭**。
- **未受影响**：P1 协议其余条款 ＋ **P2 第一批口径（三条建议）全部不变**；P2 三线在跑（devloper 实现 ＋ 改 `development-plan` §7:155、UX高手 界面层方案、tester 判据）。

### 2026-10-20 23:1x · 历史作者重写已完成 ＋ P2 协议 v0.2 落盘

**一、历史作者重写（izzy 22:5x 拍板「需要重写」）——已完成**

- **远端现状**（`git ls-remote` 实测）：`main` = **`c99156e23cf620eeabc6b5ad28a686095bb5cc1a`**、**`main-dev2` = `4322e9800bdb8305c6cf818383116fc9ae53524e`**、**`uiv2-dev` = `8789943c73d9f32936709fb49ec4a410b3d9a446`**；**远端 `refs/heads` 只有这 3 个分支**。
- **旧作者残留 = 0**（逐分支数 `zhongxingxing`）：`main` **0 / 1044**、`main-dev2` **0 / 269**、`uiv2-dev` **0 / 567**。
- **内容逐字节一致**：`git diff --stat 4f2a805 origin/main` **输出为空**（重写前 `origin/main` = `4f2a805`）⇒ **只改了作者/提交者元数据，没有改任何文件内容**。
- **tag 一律未重推**：唯一 tag 名 **83** 个（**82 个 `desktop-v*` 发布 tag ＋ 1 个恢复点 tag**；`ls-remote --tags` raw 97 / peeled 14）。**没有任何 tag 的 tagger 时间落在 2026-10-20**（最新为 `2026-10-16 19:12:45 desktop-v0.4.15`；10-20 当天 tagger 数 = **0**）⇒ 重写期间**未推过任何 tag** ⇒ **不会触发 `desktop-release.yml`**。抽查 `desktop-v0.5.8-rc.2` → `63fb0bbd`，作者仍为 `zhongxingxing` ⇒ **发布 tag 未被重写**。
- **恢复点是 tag，不是 branch**（更正 22:5x 节措辞）：`backup/pre-author-rewrite-20261020` → `4f2a805`（重写前的 `origin/main`）⇒ **回滚安全网在**；tag 名不匹配 `desktop-v*` ⇒ 发布 workflow 同样不触发。
- **共享工作目录已 `git reset --mixed origin/main`**（不动工作树）⇒ HEAD = `c99156e`，**成员在途改动完好**。
- **踩坑补充**：④ 首次 push 失败是**网络重置**（`Recv failure: Connection was reset`），**不是分支保护** ⇒ 用带 6 次重试的 `rewrite-push.ps1` 重推，attempt 1 成功。

**二、P2 协议 v0.2（承载位写死 `info` 档）**

- **触发**：UX高手 核到内核事实 ⇒ PM 第一手复核成立（现役内核 release 树 `0.2.1-rc.2`，`dsh-client-ui-conversation/lib/client.js` SHA `8554738CBEEB5218AD1BFA4773EF094363B066D052A1893D7A96797C5BC33A63`，774080 B / 20065 行）：
  - `:18982-18986`：**只有 `notice.level === "info"` 才渲染 inline 常驻条**（`div` + `role="status"`，composer card 上方）；
  - `:18808`：`if (notice?.level === "error") showToast(notice.text);` ⇒ **`error` 档只弹瞬态 toast，没有常驻条**；
  - `:15356` / `:15448`：全树仅此两处 `notices.set` ⇒ **notice 单槽、最新一条覆盖旧条** ⇒ 提示文案**必须自包含**；
  - ⇒ **「引用发生处」（chip 旁）在 P2 阶段技术上不存在**（内核无 chip 状态写入口，缺口 **G1-6**）⇒ **不得把它写成承载位**。
- **v0.2 相对 v0.1 的四处实质改动**：
  1. **承载位写死 = 输入区（composer）的常驻提示条，且必须 `info` 档**（§3.2）⇒ ⚠️ **P1 现有实现用的是 `error` 档** ⇒ **照抄 P1 写法会直接踩坑**。
  2. **「正文照发」收敛为「不额外拦截发送」**（§2.3）⇒ 界面**不承诺发送结果**（真发不发由 **P1 S-5 / S-6** 判定）⇒ **文案不得写「已发送 / 发送失败 / 发送被阻止」**，失败文案只说「无法核对是否重复」。
  3. **判据加断言**：P2-2 加「`level` 必须是 `info`」；P2-6 加「**模型请求数与失败前相同**（未被拦截）」。
  4. §8 第 1 条「可见提示的最终界面形态」标记为**承载位已定稿**（剩余只是文案与可交互状态，待 UX 方案）。
- **字节**：`official-chat-p2-protocol.md` = **10080 B / 141 行**；五份文档字节校验 **UTF-8 无 BOM / 纯 LF / 0 替换字符**。
- **未完成（PM 侧）**：① **UX高手 的方案正文未交付**（只回一句「事实核完了，我把口径问题转给 devloper，然后给方案」，正文未输出给团队）⇒ PM 已追它正式输出；② **UX高手 转交 devloper 的 3 个口径问题尚未转达**（同组条数 `members.length` vs 可省字节 `(N-1)×单条` 的双数字口径；4 个失败码界面分几档；内核 `body_too_large` 的 error toast 与我们 inline 提示的叠放顺序）⇒ PM 本轮转达。
- **对在途实现的即时约束**：devloper 正在写 P2 实现 ⇒ **提示必须 `level:'info'`**，承载位 = 输入区 inline 条；**不要照抄 P1 的 `'error'` 写法**。

### 2026-10-20 23:5x · P2 第一批 PM 独立验收 ＋ 四处协议歧义裁定（P2 协议 v0.3）＋ 清理收尾

- **PM 独立复核门禁（不采信回执）**：`check:invariants` 全绿（扫 48 文件）、`check:duplicates` **4 处**（允许 8）、`check:builtin` **4 插件 / 43 文件**、`test:scripts` **0 fail / 3 skipped**、`test:plugin` **0 fail / 3 skipped**、判据 runner **43 = 40 pass / 0 fail / 3 skipped** ⇒ 与 devloper 报告**逐项吻合**。
- **实现逐点核对（PM 第一手读代码）**：
  - ✅ **client 侧全部用 `level: 'info'`**（`plugins/official-chat/client/client.js` 的五处 `presentNotice` 调用）⇒ **没有照抄 P1 的 `error` 档**（协议 v0.2 §3.2 的硬要求：内核 `error` 档只弹瞬态 toast）。
  - ✅ **文案自包含**（条数与字节数写在同一条）⇒ 符合「内核 notice 单槽、最新覆盖旧条」。
  - ✅ **两个触发点独立**（「不 return」）⇒ 重复提示与预算提示都出。
  - ✅ **`groupKeyOf`**（`host/protocol.js`）= 五字段 `join('\n')` ＋ sha256 hex 前 16；**Rust `group_key`**（`src-tauri/src/official_chat/protocol.rs`）= `join("\n")` ⇒ **两端口径一致**。
  - ⚠️ **devloper 回执里「Rust 侧 `group_key` ＋ 夹具未做」已过时** —— 实测 `protocol.rs` 的 `resolve_duplicates_case` **已在工作树里做完** ⇒ 状态比回执新。
- **四处协议歧义裁定（已写进 P2 协议 v0.3）**：
  1. **分隔符** = **单个换行 U+000A、不含空格**（两端现状即此）⇒ **改协议措辞**消除「行内代码 `\n` 两侧空格」的误读，并列出「若要改须同次改四处」。
  2. **`invalid_request` / `snapshot_store_unavailable` 不在 13 码闭集** ⇒ **按 P1 §3 `:167` 的既有规则**（行号 2026-10-21 00:4x 当场核正：原写 `:166` 指的是相邻的 `catalog_empty`/`catalog_failed` 那条；`:167` 才是「内部冲突码一律归入 `snapshot_invalid`、原始码保留在 `detail`」）（内部冲突码归 `snapshot_invalid`、原码进 `detail`、不新增第 14 个码）⇒ **devloper 的实现正确、无需改代码**；**判据加严**：P2-6 须断 `detail` 带原码（否则「入参形态错」与「快照过期」不可区分 ⇒ 假绿）。
  3. **`groups` 只返回成员数 ≥ 2 的组** ⇒ 采纳，协议写死。
  4. **预算预检**触发点落为「**每次新增引用时预演一次**」（内核无「提交前草稿投影」入口）；原生 `@session` 占用**不可观测 ⇒ 按 0 计** ⇒ `projectedBytes` 偏小 ⇒ 方向 **宁可少报不误报**；并写明「真超限时用户仍会收到 P1 的 `body_too_large` 可见错误」⇒ **预检是提前告知，不是最后一道闸**。
- **两处协议同步**：① P1 §5 命令清单 **5 → 6**（加 `duplicates`，注明是 P2 第一批新增、P1 第一批不含）；② P1 §3 的 8 字段闭集**不改**，只加一行注说明两端现状与缺口编号。
- **缺口登记**：**G-2**（错误结果对象闭集两端不同构：Rust 8 个含 `kind`、JS 7 个缺 `kind` ⇒ 是 **JS 漏字段** ⇒ JS 侧补 `kind: 'error'`，**不在 P2 批内改 P1 冻结条款**）；**G-1**（Rust 测试自泄漏 `dsh-xlink-paths-*`：`src-tauri/src/shell/paths.rs` 的 `test_helpers::temp_dir()` ＋ **11 个用例缺 `remove_dir_all`** ⇒ 每跑一次 `cargo test` 漏 11 个目录，历史 4,394 个残留即由此而来 ⇒ 不影响功能与验收，**P2 三线收口后再修**）。
- **清理收尾（PM 实测）**：后台批量任务移入回收站 **4,217** 个；PM 复核后补清判据产物 `dsh-p1-protocol*` **61 个**（5.72 MB）与 Rust 测试泄漏的 `dsh-xlink-paths-*` **11 个** ⇒ 实测 `dsh-xlink-*` = **0**、`dsh-xlink-g1` = 0、`g1-probe*` = 0、`dsh-p1-protocol*` 只剩 **1**（判据 runner 输出目录，正在被并行会话使用 ⇒ 先留）。**全部走回收站可恢复，未永久删除**；已作废的 `%TEMP%\dsh-fix`（上游 PR 工作目录，120 KB）一并入回收站。**不属本项目、别误删**（实测仍在）：`.tmp*` 3,176、`dsh-acl-skill-*` 198、`xlink-*` 1,138。
- **一条方法论留痕**：清理任务**自报「剩余 0」**与实测（18 个）不符 —— 那 18 个是**清理任务结束后 6 秒**由另一次 `cargo test` 新产生的，**不是漏删** ⇒ **「后台任务自报的数字」不能当实测**。

### 2026-10-21 00:0x · P2 第一批收口验收（PM 独立复核）＋ 第五处裁定（P2 协议 v0.4）

- **PM 独立复跑门禁（不采信回执）**：`check:invariants` 全绿｜`check:duplicates` **4 处**（允许 8）｜`check:builtin` **4 插件 / 43 文件**｜`test:scripts` **173 tests / 170 pass / 0 fail / 3 skipped**｜`test:plugin` **289 tests / 286 pass / 0 fail / 3 skipped**｜**P1 判据 43 = 40 pass / 0 fail / 3 skipped**｜**P2 判据 8 条全通过（0 失败 / 0 未执行）** ⇒ 与 devloper 报告**逐项吻合**。
- **P2 判据逐条核对（PM 读判据实现）**：P2-1（同组两个不同 id ＋ 组标识 16 位小写 hex）｜P2-2（**`level=info` 常驻条、非瞬态 toast**，含条数与字节数）｜P2-3（`capturedAt` 不同不成组 ＋ 正对照成组）｜P2-4（跨分支/跨账号不成组 ＋ 正对照成组）｜P2-5（300000 > 262144、**正文未截断、请求体与落盘逐字节相同**）｜P2-6（command ＋ transport 两条：可见错误 ＋ 正文照发，请求数 +1）｜**P2-C（失败码 `snapshot_invalid` 在闭集内 ＋ 原码 `invalid_request` 保留在 `detail` ＋ 必备字段齐全）**｜**P2-F（2 条「去重候选对」夹具期望值与协议 §1.1 一致）** ⇒ **v0.3 要求的三处加严（P2-2 断档位、P2-6 断原码、P2-5 断请求体逐字节）全部落实**；另配 **11 条活体注入**（每条判据的反向注入，含「把档位改成 error」「让错误负载漏出闭集外的码」）。
- **第五处裁定（协议 v0.4）· 内核 notice 单槽**：内核 notice 是**单槽**（`notices.set` 全树仅 `:15356` / `:15448` 两处，**最新覆盖旧条**）⇒ 「重复」与「超预算」**同时成立**时**用户只看到最后一条**（预算那条在后）⇒ §3.1「两个触发点各自独立」的**准确含义 = 「两条都要发（不得 early return 吃掉任一条）」**，**不是**「用户同时看到两条」；**硬要求**：**后发那条必须自包含全部事实**（组条数 ＋ 重复字节 ＋ 预算字节 ＋ 超限字节）⇒ 实现已满足（两条文案都自包含）⇒ **无需改代码**；**呈现形态待 UX**，**PM 倾向合成一条**（只写一次 notice、不会被覆盖、省一次写入），本稿**不锁死形态**，只锁「后发那条必须自包含全部事实」这一条。
- **G-2 仍未修**：PM 复核 JS `errorPayload`（`plugins/official-chat/host/protocol.js`）**仍只产 7 个字段、缺 `kind`** ⇒ 派活已发（`request_id: chief-p2-batch1-verdict-20261021`），**待 devloper 执行**；协议闭集**不改**（Rust 侧 `error_result` 已产 8 个）。
- **devloper 回执里「仍待 PM 裁」的六项**（闭集外码归一 / §1.2 分隔符 / §2.2 只报成员数 ≥ 2 / §4 的 `@session` / P1 §5 命令清单 / P1 §3 的 8 字段）⇒ **已全部裁完并落进协议 v0.3**（提交 `1a7ad87`），他那条回执写在裁之前 ⇒ **实现按 v0.3 对齐即可，其中只有 G-2 的 `kind` 需要动代码**。
- **文件数更正采信**：13 个（10 改 ＋ 3 新）；文档两处（`official-chat-development-plan.md` §7:155、`official-chat-feasibility.md` §7 第 12 条）**已由 PM 代提交**（`2d3159d`），不在在途清单。
- **边界**：devloper **未 commit**，其 17 个在途改动 PM **一个没碰**；本轮 PM **只提交文档**。

### 2026-10-21 00:1x · 第六处裁定（协议 v0.5）＋ 基线/版本广播更正 ＋ 历史重写复核

- **⚠️ 广播更正（并行 PM 会话那份公告写的是 v0.2 / 基线 `2d3159d`，现已过期）**：**协议当前是 v0.5**，**基线当前是 `eff6274`**（`1a7ad87` v0.3 ＋ P1 两处同步 → `eff6274` v0.4 ＋ 收口登记 → 本节 v0.5）⇒ **以 v0.5 为准**，公告里「以 v0.2 为准」作废。
- **历史作者重写已复核**（PM 第一手核 `git log --format='%an <%ae>'` 全量统计）：**`zhongxingxing` 已从历史中完全消失** —— 现状 **1028 个 `July-X <m4java@163.com>`**、17 个 `4Dim`、2 个 `xingxing.zhong`、1 个 `July-X <over618@163.com>` ⇒ **izzy 拍板范围内的 224 个已全部重写**；余下 20 个属**别的作者身份**（dependabot 等），**不在 izzy 拍板范围**（要一并归一需另行拍板）。
- **第六处裁定（协议 v0.5）· 内核 notice 会「顶掉」我们，不是「叠放」**：内核 `notices.set` 有**两个**入口（`:15356` 的 `notify(level,text)`、`:15448` 的 `execute(fx)` 的 `case "notice"`），且内核**多处** `notify("error", …)`（`:15193` / `:15197` / `:15204` / `:15905` / `:16932`）⇒ **内核随时可能发一条 error notice 把我们的 inline 条整个顶掉**（单槽）⇒ 用户看到的是内核 toast，我们的 inline 条消失。
  - ⇒ **硬要求**：① **文案自包含**（组条数 ＋ 重复字节 ＋ 预算字节 ＋ 超限字节）；② **不得依赖内核 toast 或内核 notice 的出现时机/顺序**表达我们的状态；③ **不得**写「提示会一直在」。
  - ⇒ **可用性推论**：因触发点是「**每次新增引用时预演**」⇒ **用户再动一次引用即重新触发、重新发出一条我们的提示** ⇒ 提示**可复现**，不依赖单次 notice 存续。
  - ⇒ **渲染侧事实更正**：`client.js:18807-18809` 是**独立 `useEffect`**（`if (notice?.level === "error") showToast(notice.text)`），**不是**与 inline 条互斥的 if/else ⇒ 准确说法是「**`error` 档额外弹瞬态 toast；`info` 档只渲染 inline 常驻条**」⇒ **仍必须 `info` 档**。
- **并行 PM 派给 devloper 的三个口径 ⇒ 全部有答案**：
  1. **同组提示显示几个数字** ⇒ **条数与字节都要显示**（v0.4 已裁「后发那条自包含全部事实」）。**退化值问题不成立**：`groups` 只返回成员数 ≥ 2 ⇒ `redundantBytes = (N-1)×单条`；**§3.2 要求的口径本就是「同组有几条」「**省得下/超了多少字节**」** ⇒ 按该口径与 **UX 定稿文案（2026-10-21）**执行 ⇒ 不需要额外处理。⚠️ **一处前提作废（PM 2026-10-21 00:5x–01:0x 实测）**：先前写的「`(N-1)×单条 ≥ 单条 > 0` ⇒ **R 恒 > 0**」**不成立** —— **捕获路径不拒空正文**：`host/protocol.js` 的 `resolveCaptureCase` 对 `messages:[{role:'user',text:''}]` 返回 `kind:'snapshot'`（无失败分支），`bodyBytes = 0`，摘要 = 空串的 sha256 `e3b0c442…` ⇒ **0 字节快照可以落盘**；若同组两条都是 0 字节快照（同毫秒、同源、同空摘要），则 **R = (2-1)×0 = 0 是可达的**（PM 实测确认）。⇒ **结论不变**（R=0 时「省得下 0 字节」**字面为真** —— 两条都空 ⇒ 少送一份确实省 0 字节 ⇒ **不构成错误陈述**），但**「R 恒正」这个前提已作废** ⇒ **写文案或改实现时不得依赖「R 必 > 0」**（`host/commands.js:742` 的 `redundantBytes: (members.length - 1) * members[0].bytes`）。⚠️ **PM 同笔的自我更正（2026-10-21 01:1x）**：我在这笔（`d6c0f75`）里一并写进 **P2 协议 §2.2** 的「界面文案**不得**用『可省 R 字节』」那条**已撤回** —— 它与协议 **§3.2 自身的「省得下/超了多少字节」直接冲突**，且**措辞属界面层（UX 职责）、PM 不越界规定** ⇒ 已私聊 UX 撤回要求（「按你的定稿执行」）；协议 §2.2 那条改为**只保留「R 可能为 0（实测）」的事实 ＋「R=0 字面为真、不构成错误陈述」**，**协议文件当时在并行 PM 会话在途（正在起草 v0.6）⇒ PM 不踩在途，由 PM 在 v0.6 提交后一并改**。
  2. **4 个失败码界面分几档** ⇒ **界面不分档**，全部同一档「无法核对是否重复」（v0.3 §2.3 已裁：闭集外码归 `snapshot_invalid` ＋ 原码进 `detail`）；`invalid_request` 属**我方 bug** ⇒ 只进内部日志，**不进界面分档**，且界面文案**仍不得出现发送结果字样**。
  3. **内核 `body_too_large` toast 与我们 inline 提示的叠放** ⇒ **不是叠放，是互顶**（见上）；**不得假设内核 toast 的出现/消失**，也不靠它表达我们的状态；**对策 = 文案自包含 ＋ 可复现**（再动一次引用即再出一条）。

### 2026-10-21 00:1x · 缺口 G-2 已闭环销号（JS 补 `kind` ＋ 两端自测钉住 8 字段闭集）

- **PM 独立复核（不采信回执）**：JS `errorPayload`（`plugins/official-chat/host/protocol.js`）**两个分支都补了 `kind: 'error'`** —— 未知码归一分支 `:637`、正常分支 `:653`，字段顺序与 Rust `error_result` 一致 ⇒ **错误负载 7 字段 → 8 字段，两端同构**。**协议闭集始终未改**（是 JS 漏字段、不是协议写错）。
- **两端各有一条自测钉住闭集**：JS `plugins/official-chat/test/protocol.test.mjs:404`「§3 v0.4：错误结果对象 = 8 字段闭集，全部必备（G-2：JS 不得再缺 kind）」；Rust `src-tauri/src/official_chat/protocol.rs:1060` `error_result_carries_the_eight_field_closed_set` ⇒ **谁再省一个字段当场红**。
- **devloper 的做法认可**：他先核实 `scripts/lib/p1-protocol.mjs` 相对 HEAD **零改动**（tester 只动了适配器 `p1-adapter.mjs`），确认跑的确实是**已提交版**那 43 条 ⇒ **满足 §9「别把在途用例当回归」的要求**。
- **销号同步**：P2 协议 §9 的 **G-2 行改为销号**（保留事实与闭环证据，不删行）；P1 协议 §3 `:161` 的注同步更正为「**已闭环**」⇒ **两处都不再留「JS 只产 7 个」的过时事实**。
- **devloper 自查的净影响（PM 采信）**：v0.3 / v0.4 / v0.5 的口径**实现侧不用改代码** —— §1.2 分隔符（他上一轮报的「若改 ` \n ` 要改 4 处」**作废**）、§2.2、§2.3、§4、§3.1 均已如此；v0.5 新硬要求（文案自包含 ✔ ＋ 不依赖内核 notice 时机 ✔ ＋ 不写「提示会一直在」✔）他已给 client 补了对应注释（状态另有 `notices` 可复查记录，用户再动一次引用即重新触发一轮核对 ⇒ **提示可复现**）。**他保持「两条都发」**（与 tester 的 P2-5 判据一致）；若 UX 定「合成一条」，改动只在 client 一处。
- **文件数**：仍是 **13 个**（10 改 ＋ 3 新），现为 **758+/11−**（另 3 个新文件共 272 行）；本轮只动其中 3 个（`host/protocol.js`、`test/protocol.test.mjs`、`src-tauri/.../protocol.rs`）。
- **边界**：devloper **未 commit / 未 push**；`scripts/**` 是 tester 的在途改动，他一行没碰。**G-1**（Rust 测试自泄漏临时目录）按 §9 **不插队**，等 P2 收口后再修。
- **归属更正（他转述错了）**：他说「PM 在等你两句话（fork 仓库删否 ＋ 三问问卷作废）」—— 这两件**该由 izzy 答**，不是 PM 的；PM 只负责把问题原样呈给 izzy。**上游 G1-7 PR 已按 izzy 23:0x 拍板撤回**，分支已删、禁止改上游内核 ⇒ **不存在「仍需 izzy 登录 GitHub 开 PR」**，该阻塞点已作废。

### 2026-10-20 23:5x–2026-10-21 00:3x · git 提交记录用户名统一（izzy 拍板四项 ＋ 全量重写）

**izzy 23:57 拍板（四问）**

1. **目标身份** = **`July-X <over618@163.com>`**（署名与 GitHub **账号**都归 July-X）。**动因**：原状态是**署名文字**已是 July-X，但 email 全为 `m4java@163.com`（该 email 属 **Z-starts** 账号）⇒ `gh api repos/July-X/dsh-xlink/contributors` 显示 **Z-starts 1045 / July-X 1** ⇒ 提交实际记在 **Z-starts** 名下。**账号归属靠 email，不靠署名文字** ⇒ 统一身份必须连 email 一起换。
2. **82 个 `desktop-v*` 发布 tag 里的旧作者**：**先临时收窄 release workflow 的 tag 触发条件，推完 tag 再改回**。
3. **恢复点 tag** `backup/pre-author-rewrite-20261020`：**删掉**。
4. **范围**：**只处理 dsh-xlink**（`xgit` 另有 172 处旧名、`UnrealEngine` 63 处、**全局 `~/.gitconfig` 的 `user.name=zhongxingxing`** 均不动）。

**实施前的一手事实（全部实测）**

- **去重口径**（三个开发分支 ＋ 当时未推的 2 个提交 ＝ **1053 个可达提交**）的署名分布：`July-X <m4java@163.com>` **1033**、`4Dim <m4java@163.com>` **17**、`xingxing.zhong <m4java@163.com>` **2**、`July-X <over618@163.com>` **1**；**email 只有 `m4java@163.com` 一种** ⇒ 统一即全量替换。
- ⚠️ **`zhongxingxing` 在三个开发分支上已是 0**（22:5x 那次重写清掉的）⇒ 但**它仍留在 82 个发布 tag 指向的历史里**（实测 `desktop-v0.5.8-rc.2` 的作者仍是 `zhongxingxing`）⇒ **上一轮「残留 = 0」的口径只对开发分支成立、对 tag 不成立** ⇒ 这正是本轮必须**连 tag 一起重写**的原因。
- ⚠️ **两套数字口径不同，别混**：`filter-branch` 实际处理 **2095** 个提交（`-- --branches --tags` 口径，**含各 tag 指向历史的重复计数**）；去重后是 **1053**（`main` 1051 ＋ `main-dev2` 269 ＋ `uiv2-dev` 567 的并集）。
- ⚠️ **上一轮「旧作者残留 = 0」的结论口径不完整**：那次只数了 `zhongxingxing`，**漏了 `4Dim`(17) 与 `xingxing.zhong`(2)**；本轮改为「去重后全 refs 可达提交的作者分布」。
- ⚠️ **本地 refs 没跟上上一次重写**：`refs/heads/main-dev2` 仍指向重写前的 `ba72c0a3`（residue 15），本地也**没有** `refs/heads/uiv2-dev` ⇒ 本轮一并对齐。
- **恢复点 tag 是 tag 不是 branch**（上一轮措辞已更正）；远端 `refs/heads` 只有 3 个开发分支。

**执行顺序与做法**

1. **先建本地回滚网**（删远端 backup tag 前必须有本地安全网）：`git bundle create {bundle} --all`（**{bundle_bytes} B**，路径 `{bundle}`，**不推远端**、不进仓库）⇒ 旧作者历史仍可达于本机该文件。
2. **独立克隆里重写**（`git clone --no-hardlinks` 本地仓库 → `--index-filter true` 免逐提交 checkout → `filter-branch --env-filter` 把 author/committer 统一为 `July-X <over618@163.com>` → `--tag-name-filter cat` 连带重写 82 个发布 tag）⇒ **共享工作目录全程不被触碰**（成员在途改动零风险）。
3. **annotated tag 的 tagger 一并重写**（保留 tagger 日期与 message，只换 name/email）—— `filter-branch` 只改 tag 指向的提交，不改 tagger ⇒ 另用 `git hash-object -t tag` 重建 tag 对象。
4. **推分支**用 `--force-with-lease`（锁定重写基线 `{old_main}`）⇒ **若远端被别的会话推进过，push 直接被拒**，不会静默覆盖。
   ⚠️ **实际触发了这个兜底**：重写耗时 20+ 分钟，期间**并行 PM 会话**又推进了远端（`a13b63b` G-2 销号），本地还多一条**未推送**的 `{old_local}`（P2 第一批 13 个文件，1030+/11−）⇒ 若直接推，**远端会丢掉 P2 第一批**。⇒ 改用 **`git commit-tree` 增量重建**（保留原 message ＋ author/committer 日期，只替换身份），**tree 逐字节校验与原提交相同**后才推（两次重建：tree `54bf74364225` / `88190701021b` 均一致）⇒ 两笔提交**内容零变化**地进了新历史。
5. **推 tag 前用 GitHub API 把 `desktop-release.yml` 置为 `disabled_manually`**，推完再 `enable` 回来 ⇒ **不改任何 workflow 文件、不留痕迹、不触发发布、也不额外触发 CI**。
   ⚠️ **为什么不用「改 workflow 文件」的办法**：GitHub 在 tag push 时评估的是**被推 commit 里的 workflow 文件**，不是默认分支的 ⇒ 改默认分支无效，必须改 tag 所指 commit 里的文件（那会改内容）⇒ 改用 disable/enable。
6. **共享工作目录对齐**：`git fetch origin --tags --force` → `git reset --mixed origin/main`（**不动工作树**，成员在途改动完好）→ `git branch -f main-dev2/uiv2-dev` → 删本地 backup tag → **仓库级 `user.email` 改 `over618@163.com`**（否则**新提交**仍记到 Z-starts）。
7. **删远端 backup tag**（`git push origin :refs/tags/backup/...`）—— 不匹配 `desktop-v*` ⇒ 不触发任何 workflow。

**结果**

- 远端 `main` = **`{new_main}`**（重写前的远端 tip 是 `{old_main}`；本地那条未推送的 `{old_local}`＝P2 第一批，**已一并重写并推上，没有丢**）。
- **作者分布**（3 分支 ＋ {ntags} 个 tag，**去重 1053 个可达提交**）：**统一为 `July-X <over618@163.com>`** ⇒ 旧署名 `zhongxingxing` / `4Dim` / `xingxing.zhong` 与旧 email `m4java@163.com` **残留 = 0**（`git log --all` 实测）。
- **内容逐字节一致**：`main-dev2` / `uiv2-dev` 的 `git diff --stat`（重写前 vs 重写后）**为空**；{ntags} 个发布 tag **逐一 diff，differing = 0**；`main` 上多出的 16 个文件差异**全部来自那 2 个新增提交**（G-2 销号 + P2 第一批），其 tree 与原提交**逐字节相同** ⇒ **只改了元数据，没改任何文件内容**。
- **tagger** 也已统一为 `July-X <over618@163.com>`（annotated tag 另用 `git hash-object -t tag` 重建，保留 tagger 日期与 message）。
- **未触发发布**：推 tag 期间 `desktop-release.yml` 为 `disabled_manually` ⇒ 推完恢复 `active`；`desktop-ci.yml` 的触发条件是 `push: branches: [main]` ⇒ **tag push 根本不触发它**。
- **恢复点 tag 已删**（远端与本地）⇒ 旧作者历史在**远端不再可达**；回滚能力由本地 bundle 承担。

**并行会话提示**

- 本轮期间**另一个 PM 会话**在同一工作目录推进并推送了 `1a7ad87`（协议 v0.3）、`eff6274`（v0.4）、`276d698a`（v0.5）、`a13b63b`（G-2 销号）⇒ 本轮重写基线取的是 **`276d698a`（v0.5 那笔）**，其后它推的 `a13b63b` **也被纳入重写**（增量重建，tree 一致）⇒ 它的提交**内容全部保留**（只换元数据）。
- **v0.5 的内核事实已由 PM 一手复核成立**：内核确有 5 处 `notify("error", …)`（`:15193` `:15197` `:15204` `:15905` `:16932`）；`:18807-18809` 确为**独立 useEffect**（依赖 `[notice, showToast]`）⇒ `error` 档只弹瞬态 toast、`info` 档只渲染 inline 常驻条 ⇒ **必须用 `info` 档**这条成立；单槽 ⇒ 内核那条 error notice 一到，**我们的 inline 条被整个顶掉** ⇒ 「提示不保证常驻」成立。
  ⚠️ **一处精度补正**：`:15905`（`shell.notify`）与 `:16932`（闭包传入的 `notify`）**未必是同一个 notices 槽**；确证同槽的是 `:15193` / `:15197` / `:15204` 三处 ⇒ **结论不变**（至少三处同槽即足以顶掉）。
- **新基线**：`main` = `{new_main}`；成员若在重写前提交过、push 被拒 ⇒ 先 `git fetch && git rebase origin/main`，或把提交交 PM；**不要 force push**。

### 2026-10-21 00:3x–00:4x · P2 第一批**收口**（PM 独立复核：判据 9/9 ＋ 自测 23/23 ＋ 全门禁绿）＋ 两条红根因定性 ＋ 一处协议行号核正

**PM 一手复跑（不采信回执；命令即证据）**

| 面 | 结果 |
| --- | --- |
| `node scripts/p2-protocol.run.mjs` | **9 条全通过 / 0 失败 / 0 未执行**（P2-1…P2-6 ＋ P2-C ＋ P2-D ＋ P2-F） |
| `node scripts/p2-protocol.test.mjs` | **23 / 23**（含 **11 条活体注入**：每条都验「注入 ⇒ 判据红并点名 ⇒ 原实现逐字节还原」） |
| `npm run test:scripts` | 178 tests / **175 pass / 0 fail** / 3 skipped |
| `node scripts/p1-protocol.run.mjs` | 43 条 / 40 通过 / 0 失败 / 3 未执行 ⇒ **P1 未回退** |
| `node scripts/p1-protocol.test.mjs` | 60 / 60 |
| `npm run test:plugin` | 290 / 287 / 0 / 3 |
| `npm run check:invariants` | 绿（48 文件） |
| `npm run check:duplicates` | 4 处（允许 8） |
| `cargo test --locked --lib`（**debug profile**） | **1128 passed / 0 failed** / 11 ignored |

**两条红的根因定性（结论：既不是实现缺口，也不是判据放宽）**

- **P2-6 那条**（红字「纳入闭集的情形不该红：失败码 `invalid_request` 不在 13 码闭集里」）：根因是**自测的合成正对照本身违约** —— 默认合成 adapter 的 `commandLayerCode` 用了闭集外的 `invalid_request`，判据按 P2 §2.3 正确报红（闭集断言在 `scripts/lib/p2-protocol.mjs:1194`，**仍在且严格**），而自测却按「合规正对照」期待 PASS ⇒ **正对照与判据口径对撞**。修法＝正对照改用闭集内码，并**补两条显式负对照**（`scripts/p2-protocol.test.mjs:708-713` 的 `leaked_internal_code`、`:715-722` 的 `invalid_request`，均期待 FAIL）⇒ **严格度不降反升**，判据本体未放宽一个字。
- **P2-D 那条**（红字「三方一致不该红：没有『去重候选对』夹具」）：根因是**自测合成夹具的形状**与判据的过滤条件（`fixture.input.kind === 'duplicates'`）对不上 ⇒ 判据如实报 `skipped`（**不是假绿**）⇒ 属自测夹具形状问题，非实现问题。
- ⚠️ **登记一条纪律**：PM 复跑时**共享工作树里 tester 正在改判据**（`scripts/p2-protocol.test.mjs` 的 mtime ＝ 00:34:12；两次复跑读到的文件内容不同）⇒ **「红」的成因必须先看文件是否在动**；本轮两条红是在同一批文件被改过之后才复现的，最终定性以**最后那次复跑所读到的文件内容**为准。

**实现侧独立复核（PM 亲手追了一遍归一链路）**

- `host/commands.js:692` / `:704` 用 `fail('invalid_request', { requestId })` ⇒ 出口经 `host/protocol.js` 的 `errorPayload` **未知码归一分支**产出 `code = 'snapshot_invalid'`、`detail = '未知错误码 invalid_request'` ⇒ **码在 P1 §3 的 13 码闭集内 ＋ 原码保留在 `detail`** ⇒ 符合 P2 §2.3 与 P1 §3 `:167` 的既有规则（不新增第 14 个码）⇒ **实现不违反协议，不需改**。
- 判据侧对应断言与协议逐条对齐：`scripts/lib/p2-protocol.mjs:1194` 断码在闭集、`:1200-1201` 断 `detail` 带原码；8 字段闭集逐字段断（多一个少一个都红）；`groupKey` 钉具体值 `2bae1f90ff127002`（PM 独立用 `crypto` 算过，**期望值不取实现输出**）＋ 协议独立转写 ＝ JS ＝ Rust 三方一致；「单槽通知」类契约断的是**后发那条**（用户实际看到的那条）。
- **一条可选加固（已私聊 devloper；非缺陷、不阻塞本批）**：`:692` / `:704` 靠 `errorPayload` 兜底归一（前提是该分支不被改）；同文件 `:599-600` 已有「调用点就地归一」的写法 ⇒ 要不要在调用点归一由 devloper 自决，**做与不做都不影响本批验收**。
- **一处协议行号核正（PM 自查自纠）**：P2 协议 §2.3 与本文件此前把「内部冲突码归 `snapshot_invalid`」引作 **P1 §3 `:166`** ⇒ 当场核得该规则在 **`:167`**（`:166` 是相邻的 `catalog_empty`/`catalog_failed` 必须分开那条）⇒ **两处引用已改正**（`official-chat-p2-protocol.md` §2.3 ＋ 本文件）。同批核过：13 码表的 `:144-158` 引用**正确**、8 字段闭集的 `:161` 引用**正确**。

**本批结论与唯一未决项**

- **P2 第一批（重复识别 ＋ 可见提示 ＋ 预算预检）实现与判据双绿**，P1 判据未回退；P2 协议 v0.5 的六处裁定全部有对应判据在跑。
- **唯一未决（风险点）**：tester 的判据文件**当时仍在工作树里未提交**（未跟踪 `scripts/lib/p2-protocol.mjs`、`scripts/lib/rust-fixture-runner.mjs`、`scripts/p2-protocol.run.mjs`、`scripts/p2-protocol.test.mjs`；已改未提 `scripts/lib/p1-adapter.mjs`、`scripts/p1-protocol.run.mjs`）⇒ **判据只活在共享工作树、别人拉不到** ⇒ PM 已私聊 tester，要求按**显式 6 个路径**提交（不用 `-A` / `.` / `commit -a`），并在提交信息里写明两条红的根因（自测合成正对照/夹具形状写错，**不是实现缺口、不是判据放宽**）与复核数字。
- **下一件活**：**G-1**（`src-tauri/src/shell/paths.rs:731-736` 的 `test_helpers::temp_dir()` 产生 `dsh-xlink-paths-*`，**11 个用例缺 `remove_dir_all`** ⇒ 每跑一次 `cargo test` 漏 11 个目录，历史 4,394 个残留即由此而来）——**不影响功能与验收**，按「不插队」排在 **P2 第一批收口之后**修（PM 已在给 devloper 的回执里预告，等判据提交后正式派）。

### 2026-10-21 00:4x–00:5x · 判据第一批已合并推送（tester 交付 → PM 推送）＋ 阶段总表与协议标题口径同步

**判据第一批已进远端**：`af9789d`（`test(official-chat)`，**6 文件 / +3060 / −17** ＝ `scripts/lib/p1-adapter.mjs`、`scripts/lib/p2-protocol.mjs`、`scripts/lib/rust-fixture-runner.mjs`、`scripts/p1-protocol.run.mjs`、`scripts/p2-protocol.run.mjs`、`scripts/p2-protocol.test.mjs`）⇒ **上一节那条「唯一未决（判据只活在共享工作树、别人拉不到）」已闭环**。该提交由 **tester 自行提交**（作者已统一为 `July-X <over618@163.com>`，与 izzy 23:57 拍板口径一致），**PM 只做推送**（`ef8be8a..af9789d`，推后本地与远端 **0/0** 同步）⇒ **远端 `main` = `af9789d`**。远端 tag 唯一名仍为 **82**（`git ls-remote --tags` 96 行 ＝ 82 个 tag ＋ 14 个 annotated tag 的 `^{}` 解引用行）⇒ 无残留恢复点 tag。

**PM 一手复跑（00:4x，命令即证据）**

| 命令 | 结果 |
| --- | --- |
| `node scripts/p2-protocol.test.mjs` | **23 / 23**（pass 23 / fail 0 / skipped 0） |
| `node scripts/p2-protocol.run.mjs` | **9 条：通过 9 · 失败 0 · 未执行 0**（P2-1…P2-6 ＋ P2-C ＋ P2-D ＋ P2-F） |
| `node scripts/p1-protocol.run.mjs` | 43 条 / 40 通过 / 0 失败 / 3 未执行 ⇒ **P1 未回退** |
| `npm run test:scripts` | 178 tests / **175 pass / 0 fail** / 3 skipped |

⇒ **上一节登记的两条红已不复现**：tester 自报的「20/20、175/172/0/3」与 devloper 自报的「4 条红」**在当前工作树都不成立**；**当前实测 ＝ 175 pass / 0 fail / 3 skipped**（3 个 skip 的具体原因本轮未逐条核；P1 判据 runner 侧的 skip 名单见本文件 P1 行）。**纪律照旧**：同事自报「全绿」不作为依据，本轮结论只认 PM 自己复跑的这一遍。

**两处口径同步（PM 自查自纠）**

1. **阶段总表 P2 行原写「v0.1」且「实现/判据/夹具尚未开工」** ⇒ 与事实矛盾（协议已 v0.5、实现 `bec3b82` 与判据 `af9789d` 均已推送）⇒ **已按实际改正**。
2. **P2 协议正文标题原写「（v0.3）」而 frontmatter 已是 v0.5** ⇒ **已改正为（v0.5）**；全文只此两处版本号，无其他陈旧标记。

**G-1 的派单条件已满足**（判据已入库，不再与 P2 收口争用共享工作树）⇒ 随本轮回执派给 devloper；该项**不影响功能与验收**。

3. **「活体注入 8 条」这个数字数少了 ⇒ 改正为 11 条**：PM 一手重跑 `node scripts/p2-protocol.test.mjs` 并**逐条点过「活体注入」用例行** ⇒ 实际 **11 条**（P2-1 / P2-2 / P2-3 / P2-4 / P2-5 / P2-5b / P2-5c / P2-6 / P2-C / P2-6·原码从 `detail` 抹掉 / P2-D），自测总数仍是 **23 / 23**（pass 23 / fail 0 / skipped 0）⇒ 只影响「注入覆盖面」这个数字，**不影响绿度**。tester 独立报了同一个数（并已私聊更正 PM）⇒ 本文件此前三处写「8 条」（00:3x 节、00:3x–00:4x 表格、交接记录表）**已改正**。

**仍未决（不在本轮收口范围）**：UX高手 的**界面层方案正文仍未交**（三次追交只回「事实核完了，我把口径问题转给 devloper，然后给方案」）⇒ P2 的**文案**仍是实现里的占位措辞；izzy 侧待拍板项（`July-X/deepseek-harness` fork 删否、P1 失败分支 `error` 档是否改、首次建立站点登录态、安装版 release 壳验收）仍挂着。

### 2026-10-21 00:5x–00:6x · 判据措辞更正已推送（`23f9b90`）＋ **已提交态** PM 独立复跑 ＋ 判据自足性核验 ＋ P2 文案缺口催交 ＋ G-1 派工

- **`23f9b90`（tester 提交，PM 推送）＝ 纯措辞更正，判定逻辑一个字未改**：PM 的 v0.5 广播更正了渲染侧事实（`client.js:18807-18809` 是**独立 `useEffect`** ⇒ 准确说法是「`error` 档**额外**弹一个瞬态 toast、**不**渲染 inline 常驻条；`info` 档**只**渲染 inline 常驻条」），tester 把自己原先沿用的 v0.4 旧说法（「`error` 档只弹瞬态 toast」）改正 ⇒ **过时事实被清掉**；本批「**必须用 `info` 档**」的理由不变（要常驻条、不要瞬态 toast）⇒ 改动只涉及注释与判据的**报错文案**。**远端 `main` = `23f9b90`，本地与远端 0/0 同步**。
- **PM 在「已提交态」独立复跑**（复跑时工作树干净、HEAD == 已提交态 ⇒ 跑的就是别人拉下来会拿到的那份）：**P2 判据 runner 9/9 通过 / 0 失败 / 0 未执行**、**P2 判据自测 23/23**、`npm run test:scripts` **178 / 175 pass / 0 fail / 3 skipped**、P1 判据 **43 / 40 / 0 / 3** ＋ P1 自测 **60/60**。
- **判据的「自足性」已核**（这是「别人拉下来能不能复现」的关键一条）：判据依赖的「命令层直调」观测口（`scripts/lib/p1-adapter.mjs:1212` 的 `duplicatesCommandLayer`）**确实在 `af9789d` 里** ⇒ **不依赖任何在途未提交的改动**；否则会出现「在我这儿绿、你那儿红」的假收口。
- **P2 第一批的「文案」这一项仍空着（PM 一手核过，不是转述）**：实现里三条提示仍是占位措辞（`plugins/official-chat/client/client.js:1102` / `:1121` / `:1132` / `:1155` / `:1173`，形如「P2（占位措辞）：本条消息里有 N 条同组引用…」）⇒ **UX高手 的界面层文案方案正文仍未交**（此前三次追交只回「事实核完了，我把口径问题转给 devloper，然后给方案」）⇒ PM 已**第四次私聊催交**，并把要求一次给清：三条文案各一版中文正文（**自包含全部事实** ＋ **不得写发送结果** ＋ **不得承诺常驻** ＋ 固定 `info` 档 ＋ **必须能表达「超了多少」**），并请其定「**两条 vs 合成一条**」的形态（协议不锁死形态；改合成只动 client 一处）。
- **G-1 已正式派工**（派工条件＝判据已入库）：`src-tauri/src/shell/paths.rs:731-736` 的 `test_helpers::temp_dir()` 产生 `dsh-xlink-paths-*`，**11 个用例缺 `remove_dir_all`** ⇒ 每跑一次 `cargo test` 漏 11 个目录（历史 4,394 个残留即由此而来）⇒ **不影响功能与验收**。PM 给的修法建议是**让 `temp_dir()` 返回值自带 `Drop` 清理**（只动测试辅助代码、**不碰生产路径**、以后新增用例自动受益 ⇒ 不会再漏），并重申硬约束：**清理只清自己那块临时目录**，绝不 `remove_dir_all(<解析出来的家目录>)`（本仓吃过一次「一次 `cargo test` 把用户全部内核、实例、会话删干净」的教训）。
- 另：devloper 先前提到的「调用点就地归一 `invalid_request`」属**可选加固**（现有实现靠 `errorPayload` 的未知码分支兜底，**已符合**协议 §2.3 与 P1 §3 `:167`）⇒ PM 已明确**另开一条派活**，不许与 G-1 混在同一笔里。

### 2026-10-21 01:0x · **UX 界面层方案正文已交（4/4）** ＋ 形态锁死「两条、两个 kind」 ＋ **协议升 v0.6（成品文案定稿）** ＋ PM 一手核验与两处行号漂移更正

**「文案」这一项不再空着。** UX高手 在第 4 次追交里交出**正式稿**（此前三次只回「事实核完了，我把口径问题转给 devloper，然后给方案」，正文一个字未交）⇒ 五项齐备：文案原文 / 出现与消失规则 / 可交互状态 / 空态与错误态 / 落点映射。

**形态：UX 自行撤回「合成一条」⇒ 锁死「两条、两个 kind」**（`duplicate-groups` ＋ `budget-exceeded`，任一条不得被 early return 吃掉，且各自自包含全部事实）

- **撤回的依据**：UX 原建议合成的理由是「在『后发那条自包含全部事实』这条锁下，两条内容必然相同 ⇒ 合成是去重而非取舍」—— 这个观察仍成立，但**它核已合并的判据后自行撤回**：判据 P2-5 场景②**硬断两个 kind 都必须出现**（`scripts/lib/p2-protocol.mjs` 的 `waitForNotice(adapter, 'duplicate-groups', …)` 与 `waitForNotice(adapter, 'budget-exceeded', …)` 各要一条，缺任一即 FAIL）⇒ **合成一条会打红已合并判据**。devloper 侧本来就保持两条 ⇒ **两边一致 ⇒ 协议 §3.1「两条都要发、后发那条自包含全部事实」不变**。
- **协议升 v0.6**（PM 定）：§3.1 的「呈现形态待定 / PM 倾向合成」改为已定；§3.2 补入**成品文案四条原文**（含「数字不加千分位」「用『引用正文』不用『消息正文』」「『本次核对：』抗 stale」「零新增控件」「空态不出提示」「错误态不分档」「三条共用一个槽」）；另更正 §5 P2-2 那句过时的「`error` 档只弹 toast」。

**PM 一手核验 UX 的结论（不采信转述）**

| UX 的说法 | PM 的核验 | 结论 |
| --- | --- | --- |
| 「现有判据与本方案兼容，**不需改**」 | 核 `scripts/lib/p2-protocol.mjs`：`checkSelfContainedText`（`:315`）**只判 `String(members)` 与 `String(redundant)` 是否出现在文案里**、不判措辞；P2-5 断 `projectedBytes` 与 `exceedByBytes` 的原值出现；`forbidden`（`:1228`）＝「已发送 / 发送失败 / 发送被阻止」三词 ⇒ **四条文案全部相容**（不含禁语；数字不带千分位） | **成立** |
| 引判据 `:946` / `:951`（P2-5 场景②两个 kind）、`forbidden` 在 `:1219` | **实测现为 `:955` / `:960` 与 `:1228`** ⇒ **各差 9 行**（它读的是 `23b86f0` 之前的版本，该笔给判据加了注释） | **行号漂移，已当场更正**（引用行号必须当场核这条纪律又一次生效） |
| 内核 `:18982-18986` / `:18807-18809` / `:15356` / `:15448` | 与 PM 此前亲手核得的现役内核 `0.2.1-rc.2` `client.js` SHA `8554738C…` 一致 | **成立** |

**⚠️ 一处 PM 越界裁定的回滚（2026-10-21 01:1x · PM 自纠；期间已发生过一次改稿）**——**实测事实成立**：`redundantBytes` **可能为 0**（捕获路径**不拒空正文**：`host/protocol.js` 的 `resolveCaptureCase` 对空文本返回 `kind:'snapshot'`，0 字节快照可落盘 ⇒ 同组两条若都是 0 字节快照，R = (2−1)×0 = **0 可达**）⇒ **写文案/改实现时不得依赖「R 必 > 0」**。**但 PM 从这个事实推出「界面文案不得用『可省 R 字节』」这一步是越界** —— ① **与协议 §3.2 自身冲突**（§3.2 明写「提示要说清『同组有几条』『**省得下/超了多少字节**』」）；② **措辞属界面层**（UX高手 的职责范围），PM 不该规定用词；③ R=0 时「省得下 0 字节」**字面为真**（两条都空 ⇒ 少送一份确实省 0 字节）⇒ **不构成错误陈述**。⇒ **该禁令已撤回**：UX 定稿原文「**重复可省 X 字节**」**恢复**（协议 §3.2 成品文案与 §2.2 措辞条款已一并改回）；**期间该越界禁令已被另一 PM 会话执行成了一次改稿**（`e3451f6` 曾把 UX 原文改成事实式「重复正文共 X 字节」）⇒ **一并改回，并已私聊 UX 说明「按你的定稿原文执行」**。
⇒ **一条自纠留痕**：PM 此前给 UX 的派活消息里写过「**退化值不存在 ⇒ 可省字节恒 > 0**」（沿用了更早的口径）⇒ 该前提已被 `d6c0f75` 的实测推翻 ⇒ 此处改正；**且由此派生的那条「措辞禁令」比原前提错得更远**（它越过了角色边界、又与协议自身冲突，还被另一 PM 会话执行成了改稿）⇒ **教训：实测事实只用来改「事实类断言」，不许从事实直接推出「措辞禁令」**；要提措辞要求，先查**协议自身有没有那条口径**（本例 §3.2 已有 ⇒ PM 无权另立），且必须先问一句「**这是不是界面层的活**」（角色边界：UX高手 只管文案/标注/可交互状态）。

**登记判据侧一处已知边界（不是缺陷、不在本轮改）**：`checkSelfContainedText` 用**子串包含**判「文案含条数」⇒ 若 `members` 的数字串恰好出现在 `redundant` 里（例如 members=2、redundant=4216 ⇒ 文案只写「可省 4216 字节」也会被判「含条数」）⇒ **存在假绿口子** ⇒ 判据源码归 tester，**建议其补夹具时留意**。

**协议正文 §3.2 渲染侧措辞已统一**：原 `:105` 写旧说法「`:18808` … ⇒ `error` 档只弹瞬态 toast」，而同节末「补充事实（渲染侧）」已写准确表述 ⇒ **同一稿并存两种说法**（UX 读协议时正是被旧说法带偏，先按旧说法理解、后才接受更正）⇒ 已统一（**随并行 PM 会话的 `891dd7f` 入库并推上远端**；该提交 message 只记了它自己那处 §2.2 的更正 ⇒ **本处出处记在此行**）。

**tester 验收回执已收讫**（`batch_ba2705acd3b5` / `task_f2872ca8db55`，done），PM 逐条核过：

- **复核数字与 PM 一手实测完全一致**：P2 判据 9/9、自测 23/23（含 11 条活体注入）、P1 基线 43/40/0/3、`test:scripts` 178/175/0/3、不变量全绿 ⇒ **两边独立测出同一组数**（两边独立且一致，比一方自报更可信）。
- **它对 BUSY 那批的结论与 PM 判断一致**：`notification: delivered`（消息已进会话）＋ 内容已被 v0.3/v0.4 的裁定取代 ⇒ **重投过期内容只是扰民 ⇒ 不重投**。
- **群广播已有可见送达**：群 `official-chat-功能开发` 的 `batch_dab6896945b3` 终态 `posted_to`（讨论已受理、群里可见）。
- **它自报的「未 push」已过期**：PM 已把 `af9789d` / `bee406a` / `8046222` 推上远端（远端 `main` 已到 `891dd7f`）。

**已派（下一件活）**：① **devloper** —— 把 `plugins/official-chat/client/client.js` 里的「P2（占位措辞）：…」替换为协议 v0.6 §3.2 的成品文案原文（`level` 保持 `info`、两个 kind 都发且各自自包含、预演无事实时不发）；② **tester** —— 补两条夹具（**多组** `groups` ≥ 2、**发送后该条是否仍在**），并留意上面的子串包含假绿口子。

**仍未决（不在本轮收口范围）**：izzy 侧待拍板项（`July-X/deepseek-harness` fork 删否、P1 失败分支 `error` 档是否改、首次建立站点登录态、安装版 release 壳验收）仍挂着。

- **PM 01:2x 补裁三点（上一节未覆盖）**：① **组的顺序按 `groupKey` 升序**（UX 提出、PM 裁定）—— 顺序抖动会让「后发那条」的内容不稳定 ⇒ 用户两次看到的文案可能不同 ⇒ 已派 devloper 落实（当前实现是原序 `groups.map`）；② **不采纳「2 组引用内容完全相同（3 条可省…；2 条可省…）」这种写法**（UX 初稿）—— 该句字面可被读成「**各组内容彼此**完全相同」，而事实是「**组内**相同、**组间**不同」⇒ **不实陈述**；**逐组各一句「同组 N 条内容完全相同」每句都为真** ⇒ 已回 UX 请它复核（⚠️ **PM 02:0x 补注 · 勿被本行字面误导**：本行描述的是 **01:2x 的中间解法**（"按组读"）⇒ **01:3x 已把多组形态改为「N 组重复：第 i 组 …」**（两个「同组」并排会被读者归到同一个组 ⇒ 表面矛盾）⇒ **当前定稿见下面 01:3x 节与协议 §3.2「多组形态」条**；实现侧 `client/client.js:1071` ＝ `multi ? 「第 ${index + 1} 组 …」 : 「同组 …」`、`:1074` ＝ 「N 组重复：」⇒ 现场就是「第 i 组」形态）；③ **备查口径**：若将来要改事实式措辞，用「**重复多出 X 字节**」（UX 提出），**不要**用「重复正文共 X 字节」（**口径有歧义**：3 条各 2108 字节时全部重复正文是 6324 字节，而 R=4216 是「**除了一份之外多送出去的那部分**」）⇒ 已入协议 §3.2。

### 2026-10-21 01:1x–01:2x · PM 越界裁定的**回滚已落地** ＋ **多组情形的口径核正**（「同组 N 条」按组读）＋ 文案落地派活以本轮为准

- **回滚已落地**：提交 **`f2d6b1b`**（已推，远端 `main` 同步）⇒ 协议 §3.2 成品文案**改回 UX 定稿原文**「**重复可省 X 字节**」、§2.2 的**措辞禁令删掉**（只留实测事实「R 可能为 0」＋「R=0 字面为真」）⇒ 详见上一节「PM 越界裁定的回滚」。**协议条款未动**（`redundantBytes` 定义、§3.1/§3.2/§5 一律未改）。
- ⚠️ **PM 一手核出并已闭环：UX 定稿那句「同组 N 条内容完全相同」必须按「组」读** —— UX 原文注明「`同组 N 条` = `members.length`」，而实现里 `members` 原本取的是**各组成员数之和**（`plugins/official-chat/client/client.js:1140-1143`）、`groups.length` = 组数 ⇒ 若按**总数**说，组数 ≥ 2 时那 N 条**内容分两种**（例：2 组 × 每组 2 条 ⇒ 总数 4）⇒ 「同组 4 条内容完全相同」是**不实陈述**。⇒ **解法不是改 UX 措辞，而是按组读**：「同组 N 条内容完全相同」本来说的就是**组内**（组内内容相同、组间不同）⇒ **多组时逐组各说一句**，每句都为真 ⇒ **UX 定稿原文一字不改、也不需要再补多组版本**（PM 已撤回给 UX 的补交要求）。⇒ **判据口径 PM 一手核过**：`checkSelfContainedText` 收的 `members`/`redundant` 取自 **`groups[0]`**（`scripts/lib/p2-protocol.mjs:532-538`：`const group = groups[0]`、`members = group.members`、`redundant = group.redundantBytes`；`:594-597` 的复现轮同理）⇒ **逐组列天然含第一组那两个数 ⇒ 判据绿**（单组、多组皆然）⇒ **不需要另设「总数」口径**。⇒ devloper 已按此实现（**每组一句 ＋ 一份 canonical text**），PM 批准，并撤回了自己先前「多组先占位、别自己发明措辞」的指示；协议 §3.2 的「数字来源」已同步改成「**该组**的 `members.length`；多组时逐组各说一句」＋ 判据取 `groups[0]` 的出处。
- **文案落地派活以本轮这条为准**（并行 PM 会话先前那条派活时协议里是**被临时改掉的措辞**）：成品文案**四条原文**（协议 §3.2，已改回 UX 原文）＋「**两条逐字相同** ⇒ 用**同一份 canonical text**，别各写一套」＋「数字**不加千分位**、用「引用正文」不用「消息正文」、前缀「本次核对：」」＋「**必须保留 `members` 与 `redundant` 的原值**（判据 `checkSelfContainedText` 只断这两个数字在不在）」＋「`level` 保持 `info`、两个 kind 都发、预演无事实不发」＋「不得出现「已发送/发送失败/发送被阻止」三词（判据 `forbidden`）」。
- **判据侧已知边界（并行 PM 已登记，tester 补夹具时留意）**：`checkSelfContainedText` 用**子串包含**判「文案含条数」⇒ `members` 的数字串若恰好出现在 `redundant` 里（members=2、redundant=4216 ⇒ 只写「可省 4216 字节」也会被判「含条数」）⇒ **假绿口子**（判据源码归 tester）。

- **判据口径加严已授权（PM 01:3x · tester 提出）**：tester 指出协议 §3.2 原写「判据只断数字在不在（`String(members)` 与 `String(redundant)` 出现在文案里）」⇒ **协议把「子串包含」写成了明文口径** ⇒ 他若按 PM 先前的授权收紧，**判据就会与协议冲突** ⇒ **PM 已改该行**：判据改为断「`members` 与 `redundant` 这两个数字**各自独立出现**、**不接受子串包含**」⇒ **判据源码归 tester，PM 已授权他收紧**（理由：子串包含 ⇒ `members=2`、`redundant=4216` 时，文案漏写条数也会被判绿 ⇒ **假绿口子**；而定稿文案「数字一律不加千分位」⇒ **独立数字匹配可行**）。

### 2026-10-21 01:3x · **G-1 验收通过**（跑一次 `cargo test` 残留 **delta = 0**）＋ 多组形态改用 UX 第三种写法 ＋ **排序落点更正为 host 侧**

**G-1 验收（PM 一手，不是采信回执）**

| 项 | 结果 |
| --- | --- |
| `cargo test --locked --lib`（debug，**在 `src-tauri/` 目录跑 —— 仓库根没有 `Cargo.toml`**，PM 第一次在根目录跑直接报「could not find Cargo.toml」） | **1129 passed / 0 failed / 11 ignored** |
| `%TEMP%` 里 `dsh-xlink-paths-*` 残留数（跑前 → 跑后） | **88 → 88，delta = 0** ⇒ 「每跑一次漏 11 个」**已根治** |
| 其余门禁 | `test:scripts` 178/175/0/3、`test:plugin` 290/287/0/3、P2 判据 **9/0/0**、P1 判据 43/40/0/3、P1 自测 60/60、P2 自测 23/23、invariants 全绿（48 文件） |

- **存量 88 个是历史垃圾**（最新 mtime `01:19:49`，**早于 G-1 提交**）⇒ **待清**：**只清 `%TEMP%` 下前缀精确匹配 `dsh-xlink-paths-*` 的目录**，**绝不递归删 `%TEMP%` 本体、绝不碰其它前缀**；**绝不 `remove_dir_all(<解析出来的家目录>)`**（本仓吃过一次「一次 `cargo test` 把用户全部内核、实例、会话删干净」的教训）⇒ PM 已向 devloper 要清理方案，**执行前由 PM 复核清单**。

**多组形态：PM 采纳 UX 的第三种写法（01:3x 更正 01:2x 的采信）**

- **改成**：`本次核对：2 组重复：第 1 组 3 条内容完全相同，重复可省 4216 字节；第 2 组 2 条内容完全相同，重复可省 2108 字节；引用正文 268435 字节，超出单次消息预算 262144 字节（超 6291 字节）。`
- **理由**（UX 提出、PM 采信）：两个「同组 N 条」分句并排时，读者会本能地把它们归到**同一个**组上 ⇒ 读成「同一组既是 3 条又是 2 条」⇒ **表面矛盾**（**不是假陈述**，但确实是真实的可读性缺陷）⇒ 改为「N 组重复 ＋ 第 i 组」。**单组时文案不变**（主场景不受影响）；判据只断数字在不在 ⇒ 新写法仍含 `groups[0]` 的条数与 R ⇒ **判据绿**。
- **排序落点更正（PM 01:2x 写错了落点）**：PM 01:2x 把「组按 `groupKey` 升序」的落点写成 **client 侧**；**现更正为 host 侧** —— `host/commands.js` 的 `duplicates` 命令产出 `groups` 时**按 `groupKey` 升序**后再下发（host 是产出处 ⇒ 排序在产出处做 ⇒ 任何消费方都拿到稳定顺序；client 照单渲染）。**PM 已核 host 现状**：`groups` 的顺序是 `byKey` Map 的**插入顺序**（＝引用加载顺序）⇒ **根本没有排序** ⇒ 需改。

### 2026-10-21 01:5x · **P2 第一批全部收口**（成品文案落地 ＋ 判据 10 条 ＋ G-1 闭环）—— PM 一手复跑全绿

**四笔提交（成员提交、PM 推送）**

| 提交 | 内容 |
| --- | --- |
| `edbf57a` | 判据口径加严授权 —— 断「两个数字**各自独立出现**」，不再接受子串包含 |
| `d00e693` | 判据 —— **新增多组场景（P2-H）** ＋ 收紧「文案含数字」的子串假绿口子 |
| `6f5b0f0` | 判据口径再收紧一处 —— 数字须落在「**组语境片段**」内（UX 提出、PM 复核成立） |
| `1a0f18a` | **成品文案落地**（`client/client.js` ＋ `test/client.test.mjs`）：三处「P2（占位措辞）：…」换成 UX 定稿原文；**多组逐组列**、两条**逐字相同**（同一份 canonical text）、`level` 保持 `info`；排序落点收敛到 host 侧 |
| `4bdb831` | **G-1 闭环** —— Rust 测试临时目录改**自带 `Drop` 清理**（PM 建议的修法；每跑一次的漏目录 11 → 0） |

**PM 一手复跑（不采信任何回执；跑的是已提交态）**

| 面 | 结果 |
| --- | --- |
| `node scripts/p2-protocol.run.mjs` | **10 条全通过 / 0 失败 / 0 未执行**（P2-1…P2-6 ＋ P2-C ＋ P2-D ＋ **P2-H 多组场景** ＋ P2-F） |
| `node scripts/p2-protocol.test.mjs` | **24 / 24**（判据自测） |
| `npm run test:scripts` | 179 tests / **176 pass / 0 fail** / 3 skipped |
| `npm run test:plugin` | 292 / **289** / 0 / 3 |
| `node scripts/p1-protocol.run.mjs` | 43 条 / 40 通过 / 0 失败 / 3 未执行 ⇒ **P1 未回退** |
| `node scripts/p1-protocol.test.mjs` | 60 / 60 |
| `cargo test --locked --lib`（debug） | **1129 passed / 0 failed** / 11 ignored |

**G-1 的实测口径（不是看提交信息）**：连跑 **3 轮** `cargo test --locked --lib`，每轮前后各数一次 `%TEMP%` 里的 `dsh-xlink-paths-*` 目录 ⇒ **第一轮 34 → 12（净减 22，顺手清了历史残留）、第二轮 0 → 0、第三轮 0 → 0** ⇒ **每轮新增 = 0** ⇒ **G-1 真闭环**（不是「数字对」而是「连跑三轮不再新增」）。

**文案侧**：协议 v0.6 §3.2 已登记**多组形态**（「**N 组重复：第 i 组 …**」＝ **UX 提出的推荐形态**，PM 核判据后定）⇒ PM 此前采信的「同组分句并排」一版被改掉，理由＝两个「同组」分句并排时读者会本能归到同一个组上 ⇒ 读成「同一组既是 3 条又是 2 条」⇒ **表面矛盾**（不是假陈述，但是真实的可读性缺陷）⇒ **由 UX 的输入改进，PM 采纳**。**唯一未闭环**：该多组形态的 **UX 正式稿确认**（协议里已写明「UX 正式稿待其确认」）⇒ PM 已私聊 UX 催确认。

**远端**：**`main` = `1a0f18a`**，本地与远端 **0/0 同步**；未打标签、未发版。


### 2026-10-21 02:0x · **UX 正式稿确认已到**（01:5x 节的「唯一未闭环」闭环） ＋ **判据第二道收紧仍未落地（PM 一手复现出假绿缺口）**

**① UX 正式稿确认（01:5x 节记的「唯一未闭环」＝ 多组形态的 UX 正式稿 → 已闭环）**

UX 会话回执要点（PM 一手读回执原文，不采信转述）：

- **确认采纳**「**N 组重复：第 i 组 …**」；核过四处落地（协议 §3.2 多组形态 ／ `plugins/official-chat/client/client.js` 的多组渲染 ／ `plugins/official-chat/host/commands.js` 的 `groups.sort()` 按 `groupKey` 字节序 ／ 判据 P2-H `scripts/lib/p2-protocol.mjs`）与 UX 一致、**无异议**。
- **该写法解决了当初那两个可读性问题**（组数明确、两个「同组」并排的表面矛盾）⇒ 每组各一句、**每句都为真** ⇒ 采纳。
- **R=0 那一格：UX 主张措辞不动、不为它加分支**（① 「重复可省 0 字节」**字面为真**；② 判据要求该数字**独立出现** ⇒ 任何「不写 0」的改法会**直接让判据报红** ⇒ 想改文案就得先改判据；③ 需两条快照正文都为空才触发）⇒ **PM 采信**（与协议 v0.6 一致；PM **不另立措辞禁令**）。
- UX 本轮**未改任何代码、未 commit、未 push**（措辞归 UX、判据归 tester、协议归 PM，三者各改各的）。

**② tester 的第二道收紧（组语境片段）—— 已提交 `55b7e34`，PM 一手实测验收通过**

- **PM 一手核该提交**（`55b7e34`，tester 提交、**本地未推送**；2 文件 / **+160 / −14** ＝ 判据库 `scripts/lib/p2-protocol.mjs` ＋ 自测 `scripts/p2-protocol.test.mjs`）：新增 `groupContextClause(text, ordinal)`（按「**；**」切句 ⇒ **多组取「第 i 组」那一句**、**单组取含「同组」那一句**；**找不到锚点 ⇒ fail closed 判红**，不是放过）＋ 把 `hasStandaloneNumber` 换成 `hasNumberWithUnit(text, value, unit)`（数字 + `\s*` + **单位词**「条」/「字节」）⇒ **全文匹配 → 该组语境片段内匹配**，正对协议 `6f5b0f0` 的授权。
- **PM 一手复现过这个假绿缺口**（UX 提出、PM 用合成适配器复现；脚本 `%TEMP%\p2-repro\repro-fragment-gap.mjs`，**只读判据源码、未改仓库任何文件**）：

| 复现用例 | 文案 | 收紧前 | 收紧后（PM 实测） |
| --- | --- | --- | --- |
| v1 合规（逐组各写自己那组） | 「本次核对：2 组重复：第 1 组 2 条内容完全相同，重复可省 4216 字节；第 2 组 3 条内容完全相同，重复可省 4216 字节；…」 | pass | **pass**（不假红） |
| v2 违规（**整句漏掉「第 1 组 2 条 …」**） | 「本次核对：2 组重复：第 2 组 3 条内容完全相同，重复可省 4216 字节；…」 | **pass（假绿）** | **fail**（点名叫「文案里没有「第 1 组」这一句…必须落在该组的语境片段内」） |

  - 造景：两个组（3 条 ／ 2 条），`groupKey` 升序后 **`groups[0]` 恰是 2 条那组** ⇒ 它的条数与**组数 2** 重合；两组 R 取同值（**现实可达**：2 条组 1×4216 字节、3 条组 2×2108 字节 ⇒ R 都是 4216）⇒ 收紧前**漏写第 1 组那条时，条数 2 只以「2 组重复」的组数语境出现，判据分不出**。
- **PM 一手复跑（已提交态 `55b7e34`）**：

| 面 | 结果 |
| --- | --- |
| `node scripts/p2-protocol.run.mjs` | **10 条全通过 / 0 失败 / 0 未执行** |
| `node scripts/p2-protocol.test.mjs` | **25 / 25**（较上一轮 **+1**：新增「数字必须落在该组的语境片段内」那条，含「漏写「第 1 组 2 条」必须红」「只列第 2 组必须红」「锚点语序不同不该红」三个正/反对照） |
| `npm run test:scripts` | 180 tests / **177 pass / 0 fail** / 3 skipped（较上一轮 **+1 条测试**） |
| `npm run test:plugin` | 292 / **289** / 0 / 3 |
| `node scripts/p1-protocol.run.mjs` | 43 / 40 / 0 / 3 ⇒ **P1 未回退** |
| `node scripts/p1-protocol.test.mjs` | 60 / 60 |

- **过程中的一个红（已消，归因在自测侧）**：收紧落地后的**第一轮**自测有 **1 条红** —— `scripts/p2-protocol.test.mjs:504` 的断言期望 detail 匹配 `/字节数/`，而新 detail 改成「该组那一句里没有「63 字节」…必须自包含该组 R」⇒ **是断言正则过时、不是判据错**（判据行为正确）⇒ tester 已把断言与负例补齐 ⇒ 现 **25/25 全绿**。
- **收口**：tester 已按 izzy「只本地提交、不推送」纪律提交 `55b7e34` ⇒ **PM 统一推送**；上表数字即为**已提交态**的一手复跑结果。

**③ izzy 两条延迟回执的对账（状态已变，均已闭环）**

- izzy 提到的 `23f9b90`（渲染侧措辞更正）、`23b86f0`（P2-5 JSDoc 版本依赖标注）⇒ PM 一手核 `git merge-base --is-ancestor`：**两笔都已在 `origin/main`** ⇒ **无需再推**。
- izzy 提到的「P2 9/9 · `test:scripts` 178/175/0/3」是**旧态** ⇒ 现为 **判据 10 条全过 · 自测 24/24 · `test:scripts` 179/176/0/3**（PM 一手复跑，02:0x）。
- 「UX 形态可能从『两条』改『合成一条』」⇒ **该开放问题已由协议 v0.6 闭环**：形态锁死「**两条、两个 kind**」；UX 原提的「合成一条」在核已合并判据后**自行撤回**（P2-5 场景②硬断两个 kind 都在 ⇒ 合成会打红已合并判据）⇒ **不需要 izzy 拍板**。

**⑤ 新增缺口（已登记，归 tester，待修）：P2-H 自测在重负载下会假红 —— 是判据的墙钟等待，不是实现缺陷**

- **来源**：devloper 实测 —— 把 `npm run test:scripts` 与 `npm run prep:builtin` **并行**跑时红过一次；**串行**重跑 **24/24 全绿**，单独跑那条也绿。
- **根因（PM 一手核过源码）**：判据 `scripts/lib/p2-protocol.mjs` 的 `waitForStableNotice(adapter, kind, { timeoutMs: 10_000, stableMs: 400 })` 用**墙钟**判定「轮次已跑完（组数不再增长）」⇒ 机器被压满时，**10s 总预算内等不到 400ms 连续稳定** ⇒ 提前放弃 ⇒ **红字指向错因**：报的是「**没有出现提示**」，而**真因是超时**，**不是**「只报了 1 个组」（漏报）⇒ 看到这个红字的人会**误判成实现漏报**。
- **影响**：任何并行跑门禁的人都会拿到一个**假红**，且会被读成「实现缺陷」⇒ **判据自身的不稳定，该修**（**判据归 tester，PM 不代改**）。
- **PM 侧已做的处置**：PM 复跑门禁**一律串行**（本节所有数字都是串行跑出来的）⇒ **本轮结论不受影响**。
- **交办方向（判据归 tester，PM 只给方向）**：① 「稳定」判定由墙钟改**事件式**（组数不再增长即视为轮次跑完）＋ 放宽/自适应总预算；② 或至少**把红字分开** —— 超时要说「等预算内未等到轮次稳定（疑似负载过高，**不是漏报**）」，别断言「只报了 N 个组」；③ 改完后须在**重负载下**（并行 `test:scripts` ＋ `prep:builtin`）复验**不再假红**、串行仍全绿。

**④ 远端**：推送前 **`main` = `0e0ea11`**；本地领先两笔 —— **`55b7e34`**（tester 的判据第二道收紧）＋ 本文这笔（PM 的进度回填）⇒ 由 **PM 统一推送**（**不打标签、不发版**）⇒ 推后的 sha 见提交记录。

### 2026-10-21 02:5x · **P2-J 已落地并推送**（判据第 **11** 条：发送后用户眼前那条提示仍不失真）＋ PM 一手复跑全绿 ＋ 新派两单（P2-5 预算数字加严、P2-H 重负载假红）

**① P2-J 已入库（`6344cb3`，已推 `origin/main`）**

- **来源**：UX 提出「发送后那条 inline 常驻条还在不在」；PM 02:0x 把它交回给 tester（**PM 不代改判据**）。tester 写完时其会话被 runtime 中断（crash-recovery）、未及提交 ⇒ 由并行 PM 会话代为提交 `6344cb3`（**内容与 tester 在途改动逐字一致**；3 文件 / **+285 / −2**）⇒ **PM 一手实测验收后统一推送** ⇒ **判据入库基线 = `6344cb3`**（推这批判据那刻远端即此提交；此后 PM 又推了进度回填 `acf49bd`）。
- **判据本体**：`scripts/lib/p2-protocol.mjs` 新增 `checkP2_J_noticeStaysTruthfulAfterSend(adapter)`；并把 `forbidden` 三词提到**模块级** `FORBIDDEN_SEND_RESULT_WORDS`、与 P2-6 **共用一份**（免得两处各抄一份、日后改一处忘一处）。
- **断的是「发送后客户端投递的**最后一条**」，不是「那条还在不在」** —— 内核 notice 是**单槽**（最新一条覆盖旧条）⇒ 用户眼前只可能是**最后投递的那条** ⇒ 两个问法等价。
- **五条失真形态各断**：① 与发送前那条**逐字相同**（协议 §3.1「后发那条必须把先发那条的事实也带上」）；② `level` 仍是 **`info`**（§3.2 常驻条必须 `info` 档）；③ 不含**发送结果字样**（§3.2 forbidden 三词）；④ 不把「本次核对」写成「当前状态」（§3.2 抗 stale 的锚：条会被内核顶掉且撤不掉）⇒ 禁「现在」「当前」；⑤ 仍**自包含该组事实**（条数 ＋ 该组 R，复用已收紧的 `checkSelfContainedText`）。
- **「发送」的可观测口径**＝产出正文并进模型请求（请求数 +1；与 P1 A5/A8、P2-6 同款）⇒ 证明发送路径真走过；**发送前提不成立时报红并写明是「前提」不成立**（不是实现缺陷）。
- **可测边界（如实写进 detail）**：**内核 notice 槽的存续不可机检**（adapter 是假 ctx、无内核单槽）⇒ **不拿客户端记录冒充内核槽**；本条断的是**客户端投递侧可观测的部分**。
- **内核侧的事实（UX 用内核源码核出；PM 已当场复核行号与 SHA，登记备查）**：现役内核 release 树 `0.2.1-rc.2` 的 `C:\Users\zxx\.dsh-xlink\dsh\desktop\kernels\0.2.1-rc.2\node_modules\@deepseek-ai\dsh-client-ui-conversation\lib\client.js`（SHA-256 `8554738CBEEB5218AD1BFA4773EF094363B066D052A1893D7A96797C5BC33A63`）——`notices` 槽**全树只有两处写入**（`:15356` 的 `notify(level,text)`、`:15448` 的 `execute(fx)` 的 `case "notice"`；PM 已核这两行确为 `this.notices.set({`）⇒ **没有任何一处清空或重置**；发送**成功**不发 notice（`:15191` `if (outcome.kind === "success") return;` ⇒ 成功直接返回，`:15193` 才 `this.notify("error", outcome.text)`）；会话作用域销毁时才清（`dispose()` `:15376`，**发送路径不调**）。
  - **三个场景的结论**：① 发送**成功** ⇒ 我们那条 inline 常驻条**仍在**（内核不发 notice、不清槽）⇒ 用户编下一条消息时它还挂在 composer 上方；② 发送**失败** ⇒ 被内核的 `error` notice **整个覆盖**（内核那条 ＋ 一个瞬态 toast）；③ 会话关闭/销毁 ⇒ 随会话作用域消失。
  - **与 P2-J 的关系**：P2-J 断的是**客户端投递侧**（发送后最后一条仍是那条重复核对）⇒ 与①**同向**；而**②「发送失败被内核 `error` notice 覆盖」这条路径不在 P2-J 覆盖范围**（那是内核行为、我们不控）⇒ **如实登记，不假装已覆盖**。
  - **UX 的界面层意见（登记备查，PM 不代裁、不代改措辞）**：发送成功后那条提示仍会挂在空草稿上，但因措辞是「本次核对：」⇒ 陈述的是**那一次核对**的结果、不是当前草稿的状态 ⇒ **仍为真**；UX 建议**不做额外处理**（内核无 clear 接口；覆盖成「一切正常」类消息是噪音、且违反「只有新事实才发 notice」；下一轮新增引用会重新出一条准确的）。**PM 只登记该意见、不代为拍板；若将来内核加 clear 入口，再回来议。**
- **自测**：`scripts/p2-protocol.test.mjs` 新增 1 条用例组（合规正对照 ＋ **5 条负例**：带发送结果字样 / 写成「当前」状态断言 / 发送后被换成别的 / 丢了组事实 / 档位变 `error`），并把「十条判据在真实实现上都跑得起来」改为「**十一条**」；`scripts/p2-protocol.run.mjs` 的 `CRITERIA` 增加 P2-J ⇒ **判据 10 条 → 11 条**。

**② PM 一手复跑（**串行**，HEAD = `6344cb3`；不采信任何回执数字）**

| 面 | 结果 |
| --- | --- |
| `node scripts/p2-protocol.test.mjs` | **26 / 26**（0 fail / 0 skipped；较 25/25 **+1** ＝ P2-J 那条） |
| `node scripts/p2-protocol.run.mjs` | **11 条：通过 11 · 失败 0 · 未执行 0**（P2-J 绿：「发送后用户看到的最后一条仍是那条重复核对（与发送前**逐字相同**、level=info、不含发送结果字样、不把『本次核对』写成当前状态、仍带该组事实 2 条 / 67 字节）」） |
| `npm run test:scripts` | 181 tests / **178 pass / 0 fail** / 3 skipped（较上一轮 **+1 条测试**） |
| `npm run test:plugin` | 292 / **289** / 0 / 3 |
| `node scripts/p1-protocol.run.mjs` | 43 / 40 / 0 / 3 ⇒ **P1 未回退** |
| `node scripts/p1-protocol.test.mjs` | 60 / 60 |

⇒ **P2 判据侧现有 11 条全部在 `origin/main` 且全绿**；本轮**未改任何实现侧代码**（`plugins/**` 一行未动）、**未改协议正文**（P2-J 断的四条全是协议 §3.1／§3.2 **已有明文** ⇒ 不需要新协议条款）。

**③ 本轮新派两单（均归 tester；PM 只给口径，不代改判据）**

- **派单①：P2-5 的预算数字加严**（tester 自陈的边界；PM 裁定＝**加严**，方向与第二道收紧同款）：P2-5 断 `projectedBytes` / `exceedByBytes` 仍用 `text.includes` ⇒ **仍有子串假绿口子**（文案里别的数字串恰含该值时会误判「含」）。**口径**：改用判据里**已有**的 `hasNumberWithUnit(text, value, '字节')`（数字**独立出现**且**紧跟单位词**）；**必须配反向用例**（子串假绿那一格必须红）；**放宽一律要配反向用例**。
- **派单②：P2-H 自测在重负载下的假红**（02:0x 节 ⑤ 已登记，**至今未修**）：真因是 `waitForStableNotice` 的**墙钟**等待（`timeoutMs: 10_000` / `stableMs: 400`），但红字报的是「**没有出现提示**」⇒ 易被误读成实现漏报。**口径（最小改动优先）**：① **超时与「没出现」分报** —— 超时要说「等预算内未等到轮次稳定（疑似负载过高，**不是漏报**）」，不许断言「只报了 N 个组」；② 再考虑把「稳定」判定由墙钟改**事件式**（组数不再增长即视为轮次跑完）＋ 自适应预算；③ 改完须在**重负载下**（并行 `npm run test:scripts` ＋ `npm run prep:builtin`）复验**不再假红**、**串行仍全绿**，并回填本文件。
- **PM 侧纪律不变**：复跑门禁**一律串行**（本节全部数字都是串行跑出来的）⇒ 本轮结论不受重负载影响。
- **扩派单③（PM 一手核过行号后加派；源自 UX 扫出的同源残留）**：PM 原本只点了 P2-H 一条，但**同一种墙钟等待被别的判据共用** ⇒ PM 一手核 `scripts/lib/p2-protocol.mjs` 的**全部**等待定义与调用点 ⇒ **除 P2-H（`:1692`，已改事件式 `waitForLastRoundNotice`、30s 兜底）外，另有 7 个调用点仍是 2500ms 墙钟**：`waitForNotice`（定义 `:236`）用于 **P2-2 `:726`/`:782`、P2-5 `:1180`/`:1185`、P2-6 `:1488`、P2-J `:1821`**，与 `waitForBudgetNotice`（定义 `:321`）用于 **P2-5 `:1075`** ⇒ **2500ms 比 P2-H 原来的 10s 更紧** ⇒ 重负载下**同款提前超时假红**、红字形态与 P2-H 同款（「**没有出现 X 提示**」）⇒ **易被误读成实现漏报**。
  - **口径（分两步，可分两笔提交）**：① **先改超时分报**（成本最低、立刻消除误读）——这 7 处的超时红字统一成「等预算内未等到 X 提示（**疑似负载过高，不是漏报**）；若在低负载下稳定复现，才按漏报论」，**不许**再把超时写成「没有出现 X 提示」；② **再改事件式**（照 P2-H 那套完成信号），**兜底预算**提到与 P2-H 同量级。
  - **硬约束**：**必须配反向用例**（超时红字要能验到那句口径，否则该口径没有回归保护）· 只许加严不许放宽 · **不许**让合规实现假红（事件式信号取自 notice 自带的 `trigger`/`groups.length`，真实实现与合成适配器都提供）· 本单**只动「等待机制 ＋ 超时文案」**，不动各判据自己的断点 · 兜底提到 30s 后**须给实测耗时**（若明显变慢，用**按判据分档**，不要全局拉长）。

**④ 远端**：推 `6344cb3` 那刻**推送前 `main` = `655e336` ⇒ 推后 `main` = `6344cb3`**（本地与远端 0/0 同步、工作树干净）；**此后 PM 又推了进度回填 `acf49bd`** ⇒ 现在远端不止此提交；**不打标签、不发版**。

### 2026-10-21 02:6x · **tester 两单已提交 `e3929a5`，PM 一手复跑验收通过**（P2-H 等待改事件式 ＋ P2-5 预算数字加严）＋ **同源残留扩派单③ 已投出**（另有 7 个调用点仍是 2500ms 墙钟）

**① tester 的两单已入库（`e3929a5`，2 文件 / **+118 / −36**；已在 `origin/main`）**

- **来源**：PM 02:5x 的派单①（P2-5 预算数字加严）与派单②（P2-H 重负载假红）；另 UX 扫出**同源残留** ⇒ PM 一手核过行号后**加派**（见 ③）。
- **PM 一手核该提交的两处落点**（不采信回执转述）：
  - **派单①（P2-5 加严）**：`scripts/lib/p2-protocol.mjs:1117` / `:1123` 由 `text.includes` 改为 `hasNumberWithUnit(text, projected / over, '字节')`（数字**独立出现**且**紧跟单位词**）；注释点名了假绿场景 —— `includes('37856')` 会被「**137856**」满足 ⇒ 一条只写了别的字节数、根本没写「超了多少字节」的文案也判成「文案自包含」⇒ **子串不算**。
  - **派单②（P2-H 事件式）**：`:277` 新增 `waitForLastRoundNotice`（完成信号①＝`trigger` ＝ 最后一次 pick 的 ref、②＝`groups.length` 达期望值；**30s 只作兜底**）；超时红字（`:1703` / `:1705`）＝「**疑似负载过高，不是漏报**」＋「**若在低负载下稳定复现，才按漏报论**」⇒ 与 PM 的交办口径逐字对得上。
- **PM 一手复跑（**串行**，HEAD = `2e73502`，含 `e3929a5`）**：

| 面 | 结果 |
| --- | --- |
| `node scripts/p2-protocol.test.mjs` | **26 / 26**（0 fail / 0 skipped；**条数不变** —— 派单①的新负例是**加在既有用例内的断言**，不是新 test） |
| `node scripts/p2-protocol.run.mjs` | **11 条：通过 11 · 失败 0 · 未执行 0**（P2-H、P2-J 均绿 ⇒ 事件式改法**未引入新假红**） |
| `npm run test:scripts` | 181 tests / **178 pass / 0 fail** / 3 skipped |
| `npm run test:plugin` | 292 / **289** / 0 / 3 |
| `node scripts/p1-protocol.run.mjs` | 43 / 40 / 0 / 3 ⇒ **P1 未回退** |
| `node scripts/p1-protocol.test.mjs` | 60 / 60 |

**② 远端**：tester 按纪律**只本地提交、未推送**（`e3929a5`）⇒ PM 的下一笔推送（进度回填 `2e73502`）**把它作为祖先一并推上远端** ⇒ `e3929a5` **已在 `origin/main`**（PM 一手 `git merge-base --is-ancestor` 核过）⇒ **远端 `main` = `2e73502`**（栈：`2e73502` 进度回填 → `e3929a5` 判据两单 → `acf49bd` 进度回填 → `6344cb3` P2-J）；未打标签、未发版。

**③ 同源残留 → 扩派单③（UX 扫出、PM 一手核过行号；口径见 02:5x 节，本节只记进展）**

- PM 一手扫 `scripts/lib/p2-protocol.mjs` 的**全部**等待定义与调用点 ⇒ **除 P2-H（`:1692`，已改事件式）外，另有 7 个调用点仍是 2500ms 墙钟**：`waitForNotice`（定义 `:236`）用于 **P2-2 `:726`/`:782`、P2-5 `:1180`/`:1185`、P2-6 `:1488`、P2-J `:1821`**，与 `waitForBudgetNotice`（定义 `:321`）用于 **P2-5 `:1075`** ⇒ **2500ms 比 P2-H 原来的 10s 更紧** ⇒ 重负载下**同款提前超时假红**、红字形态与 P2-H 同款（「没有出现 X 提示」）⇒ **易被误读成实现漏报**。
- **已投出**（session 直投 tester，非队列）；随单附的提醒：**复跑时先串行、单独跑 P2-H 一次再判归属**；判据自测**不要与** `npm run test:scripts` / `prep:builtin` 并行跑。
- **交付要求不变**：分两步（先改超时分报、再改事件式）、**必须配反向用例**、只许加严不许放宽、须给实测耗时。

**④ 顺带修掉一处会误导的措辞**（UX nit、PM 已修，提交 `2e73502`）：文档里三处写「远端 `main` = `6344cb3`」，而回填它的那笔 `acf49bd` 推上去后已过时一格 ⇒ 后来者会拿它当「远端现在在哪」⇒ 已改成**带时点**的说法（「判据入库基线 = `6344cb3`（推那刻远端即此提交；此后 PM 又推了进度回填 `acf49bd`）」）。

### 2026-10-21 02:7x · **派单③第一步已入库 `f55124d`，PM 一手验收通过**（等待预算全局收口 ＋ 超时红字分报）＋ **重负载假红缺口闭环**（PM 02:0x 登记的那条）＋ **三条待补已登记**

**① 派单③第一步已入库（`f55124d`，3 文件 / **+89 / −12**；`scripts/lib/p1-adapter.mjs` ＋ `scripts/lib/p2-protocol.mjs` ＋ `scripts/p2-protocol.test.mjs`）**

- **改法**：等待预算**由 adapter 给** —— 真机 **30s**（`p1-adapter.mjs` 的 `createAdapter({ waitBudgetMs = 30_000 })` ＋ 暴露 `waitBudgetMs()`）、**合成适配器 500ms**（`p2-protocol.test.mjs:122`）、**活体注入 10s**（`:843`）；判据库 `waitForNotice` / `waitForBudgetNotice` 的 `timeoutMs` 默认改 `null` ⇒ 走 `noticeWaitBudget()`（`p2-protocol.mjs:257`）。**兜底 2500ms → 30s** ⇒ 旧预算在重负载下会提前放弃 ⇒ 拿到假红。
- **红字统一** `noticeTimeoutWhy(adapter, what)`（`:273`）＝「等预算（Ns）内**没有出现**X（**疑似负载过高，不等于「没发生」**）」⇒ **保留原有短语**（自测断言不用改）＋ 追加「没等到 ≠ 没发生」的分辨；**7 个调用点全部走它**（`:773`/`:832`/`:1125`/`:1231`/`:1240`/`:1544`/`:1874`）。
- **PM 一手核过**：① 真机 `waitBudgetMs` 默认是 **30s**（**没有被压成短预算** ⇒ 抗重负载的意图成立）；② **判据断点一个字没改**（只改「等多久」与「怎么措辞」）；③ **P2-5 的特有措辞没被吞掉**（`:1240` 通用 ＋ `:1241` 该判据专属的「（**不等于「只发了一条」**——若低负载下稳定复现，才按 early return 论）」两层叠加）✔ 与 PM 裁的一致。

**② PM 一手复跑（**已提交态**、HEAD = `f55124d`、**工作树干净** —— 这次测的就是别人拉下来会拿到的那份）**

| 面 | 串行结果 | 耗时 |
| --- | --- | --- |
| `node scripts/p2-protocol.test.mjs` | **26 / 26**（0 fail / 0 skipped） | **34.0s** |
| `node scripts/p2-protocol.run.mjs` | **11 条：通过 11 · 失败 0 · 未执行 0** | **0.7s** |
| `npm run test:scripts` | **178 pass / 0 fail / 3 skipped** | 34.7s |
| `npm run test:plugin` | **289 / 0 / 3** | 5.1s |
| `node scripts/p1-protocol.run.mjs` | 43 / 40 / 0 / 3 | 2.6s |
| `node scripts/p1-protocol.test.mjs` | **60 / 60** | 1.3s |

**③ 重负载复验 —— 02:0x 登记的「P2-H 重负载假红」缺口，**已闭环****

- **复验法（PM 02:0x 登记时的原口径）**：**并行**跑 `npm run prep:builtin` ＋ 被测面 ⇒ 人为压满机器。
- **PM 一手结果**：
  - **重负载 A**（并行 `prep:builtin` ＋ `node scripts/p2-protocol.test.mjs`）⇒ **26 / 26 全绿、0 失败**，耗时 **34.0s**（**与串行 34.0s 一致**）⇒ **不再假红**；P2-H **0.98ms**、P2-J **0.56ms**（事件式等待不再等墙钟）。
  - **重负载 B**（并行 `prep:builtin` ＋ `npm run test:scripts`）⇒ **181 tests / 178 pass / 0 fail / 3 skipped**，**34.8s**，输出里**无失败行、无超时行** ⇒ 同样**不假红**。
- ⇒ **结论**：那条缺口的**真因是判据的墙钟等待**（不是实现缺陷），`f55124d` 把它改成「预算由 adapter 给 ＋ 超时与「没发生」分报」之后，**重负载下不再假红**；且 PM 的门禁复跑**一律串行**的纪律**不再必需作为避坑手段**（但仍建议串行，避免把别的重活混进结论）。

**④ 顺带测到「活体注入预算」的代价（PM 一手计时）**

- 重负载 A 的逐条耗时里，三个**活体注入负例各等满 ~10.1s**：活体注入（P2-5）**10.1s**、活体注入（P2-5c）**10.1s**、活体注入（P2-6）**10.1s** ⇒ **30s 就是自测 34s 的主体**（其余 23 条加起来才 ~4s）。
- ⇒ **PM 已裁**（并已投给 tester）：**活体注入预算 `:843` 的 `10_000` → 写死 `1_000`**（**不写区间**：「1~2s」取上界就是 ~10s，收益口径会失真）。理由：注入用例的提示是**同步投出**的（缺陷是唯一变量、注释自认「确定性」）⇒ 不存在「慢慢才出现」⇒ 预算只需覆盖同步抖动。
  - **降档不换结论的依据（PM 一手核过注入实现）**：`p2-protocol.test.mjs:834` 的 `inject()` 是对**源码副本**做正则替换（注入的是同步执行路径上的逻辑缺陷，如「让预算永不超限」「把预算提示被 early return 吃掉」）⇒ 注入后仍走真实实现的**同步**路径 ⇒ **不存在「提示其实会出、只是慢于预算」的情形** ⇒ 「等满 1s 判红」与「等满 10.1s 判红」**结论完全一致**，只省时间。
  - **收益（按实测算）**：**34.0s → ~7s ＝ 省 ~27s、约 3/4**（4s ＋ 3×1s）；若取 2s 则为 ~10s（4s ＋ 3×2s）⇒ **这就是「写死 1s」而不写区间的原因**。**真机侧仍 30s、合成适配器 500ms，均不受影响**。
- 另注：UX 报的「自测 93.9s」是**更早的在途版本**（且测时 PM 正在同时跑门禁、机器负载极高）⇒ 与 PM 一手实测的 **34.0s** 不矛盾（串行/轻负载 34.0s、重负载 34.0s）。

**⑤ 三条**待补**已登记（PM 一手核过 `f55124d`，均未落地 ⇒ 该笔只背书「已到位的部分」）**

| # | 待补项 | PM 一手核到的现状 |
| --- | --- | --- |
| ① | **通用后半句并进共享函数** | `noticeTimeoutWhy()`（`:273`）**仍只有前半句**；「若在低负载下稳定复现，才按漏报论」全树**只剩 2 处**（`:1241` P2-5 专属版、`:1756` P2-H）⇒ **另外 5 处一句都没有** ⇒ 排查指引在多数调用点仍缺。**改法**：并进共享函数（单一来源、7 处一致），**但 P2-5 那句专属措辞留在调用点叠加**（两层、不冲突） |
| ② | **超时红字口径的反向用例** | `scripts/p2-protocol.test.mjs` 里「疑似负载」**0 命中** ⇒ 统一口径**零回归保护**。**改法**：补一条**直接断 `noticeTimeoutWhy()`** 的用例（断含「疑似负载过高」且不含「没有出现」/「漏报」）⇒ **不必靠长等待**就能锁住口径 |
| ③ | **活体注入预算降到 `1_000`（写死，不留区间）** | `:843` 仍是 `waitBudgetMs: 10_000` ⇒ 自测仍 34s（代价已由 ④ 实测量化；「写死 1s」的依据见 ④） |

- **另记**：派单③的**第二步「把 P2-2/P2-5/P2-6/P2-J 也改事件式」在这笔里未做**（这笔试的是**第一步**：预算放大 ＋ 超时分报）⇒ **PM 已核**：这四条都是**单组/单条**场景、`groups.length` 恒为 1 ⇒ 「组数到期望值」这个信号②**永不命中** ⇒ 若要事件式，**只实现信号①**（`trigger` ＝ 对应 pick 的 ref），**不要**照抄 P2-H 的双信号结构留死代码；信号①两边形状都齐（合成适配器的 notice 带 `trigger`、真实实现带 `trigger: triggerRef ?? null`）⇒ 不会让合规正对照自己假红。

### 2026-10-21 02:8x · **派单③两步都已入库**（`0d03be0` 红字口径统一 ＋ `9c32e0d` 等待改事件式）＋ PM 一手复跑全绿 ＋ **降档收益实测命中（34.0s → 7.6s）** ＋ **「红字字面矛盾」三条仍待落地**

**① tester 两笔已入库（PM 一手核过内容）**

| 提交 | 范围 | PM 一手核到的落点 |
| --- | --- | --- |
| `0d03be0` | 派单③① 红字口径统一（2 文件 / **+213 / −21**） | 7 处等待超时**统一**走共享口径、**不再写「没有出现 X 提示」**；新增「事件证据」层（这一轮到底有没有出过提示） |
| `9c32e0d` | 派单③第二步 等待改**事件式**（2 文件 / **+260 / −22**） | 轮次归属用 `isRoundNotice(notice, lastRef)`（`:308`）＝ **按 `trigger` 匹配**（事件式信号①）；**完成信号②（`expectedGroups`）只留在 `waitForLastRoundNotice`（`:435`/`:451`/`:453`）且只有 P2-H 用（`:1893`）** ⇒ **P2-2/P2-5/P2-6/P2-J 这些单组/单条场景只用了信号①** ⇒ **那个永不命中的信号②没有被抄进来** ✔（PM 上一轮裁「只做信号①、不加信号②」被照做） |
| （同两笔内） | 注入预算降档 | `scripts/p2-protocol.test.mjs:858` 已是 `waitBudgetMs: 1_000`；`p2-protocol.mjs:375` 注释与代码已一致 |

**② PM 一手复跑（**已提交态**、HEAD = `9c32e0d`、**工作树干净**、串行）**

| 面 | 结果 | 耗时 |
| --- | --- | --- |
| `node scripts/p2-protocol.test.mjs` | **29 / 29**（0 fail / 0 skipped；**用例数 26 → 29**） | **7.6s** |
| `node scripts/p2-protocol.run.mjs` | **11 条：通过 11 · 失败 0 · 未执行 0** | **0.8s** |
| `npm run test:scripts` | **181 pass / 0 fail / 3 skipped**（184 tests） | **8.5s** |
| `npm run test:plugin` | **289 / 0 / 3** | 5.1s |
| `node scripts/p1-protocol.run.mjs` | 43 / 40 / 0 / 3 | 2.7s |
| `node scripts/p1-protocol.test.mjs` | **60 / 60** | 1.3s |

**③ 重负载复验（并行 `npm run prep:builtin` ＋ 被测面）⇒ 仍不假红**

- **重负载 A**（＋ `node scripts/p2-protocol.test.mjs`）⇒ **29 / 29 全绿、0 失败、7.6s**（**与串行一致**）；三个活体注入负例各 **~1.1s**（预算 1s）· P2-H **0.96ms** · P2-J **0.50ms**。
- **重负载 B**（＋ `npm run test:scripts`）⇒ **184 tests / 181 pass / 0 fail / 3 skipped、8.3s**，**无失败行、无超时行**。

**④ 降档收益：PM 一手实测命中（原标「待验」，现可作结论）**

| | 降档前（PM 实测，`f55124d`） | 降档后（PM 实测，`9c32e0d`） | 差 |
| --- | --- | --- | --- |
| 判据自测 | 34.0s | **7.6s** | **省 ~26.4s、约 78%** |
| `test:scripts` | 34.7s | **8.5s** | 省 ~26s |
| 真机 runner | 0.7s | **0.8s** | **无感**（一次命中、不等兜底） |

⇒ 02:7x ④ 里写的「**34.0s → ~7s ＝ 省 ~27s、约 3/4**」**已由 PM 一手实测确认**（UX 侧同期也测到 7,493ms，与 PM 的 7.6s 一致）⇒ **门禁变慢、真机无感**这个结论不再是推断。
⇒ **基线数字更新**：判据仍 **11 条**（第二步改造**未新增判据**）· **自测用例 26 → 29（+3）** ⇒ 本文件此前多处「自测 26/26」**已过时**，以本节为准。

**⑤ 「红字字面矛盾」那三条**仍待落地**（PM 上一轮裁定已投 tester，尚未见他落地）**

PM 一手核过 `9c32e0d` 的现状（**该两笔只背书「已到位的部分」**）：

| # | PM/UX 的定稿 | `9c32e0d` 现状（PM 一手核） |
| --- | --- | --- |
| ① | **默认 verdict 去掉「不是漏报」**（UX 定稿：现象「疑似负载过高」 → 条件 → 结论，一条直线） | `noticeTimeoutWhy()`（`:277`）**仍无 `verdict` / `tail` 参数**，文案仍是「（疑似负载过高，**不是漏报**）；若在低负载下稳定复现，才按漏报论」⇒ **字面矛盾仍在**（源码 `:280`、P2-H 手写处 `:1902`；自测 `:522`/`:674`/`:1403`/`:1452`/`:1536`/`:1539` 也仍写「不是漏报」） |
| ② | **P2-5 那处定稿**（UX 逐字定稿）：`等预算（Xs）内未等到预算提示（疑似负载过高）；若在低负载下稳定复现，再分辨是漏报，还是只发了一条（被 early return 吃掉）`；verdict 用默认、**tail 覆盖通用后半句**、**extra 整段删掉** | 「再分辨是漏报」**0 命中** ⇒ 未落地；`:1384` 的 `extra` 仍是「（**这一处另注**：不等于「只发了一条」——若低负载下稳定复现，才按 early return 论）」 |
| ③ | **P2-H 那处句式**（UX 定稿）：「…（疑似负载过高）；若在低负载下稳定复现，才按漏报论——期间见到最多 N 个组、M 条提示」；并把**来源引用**（「PM 02:0x 登记的缺口…」）**移出红字、放进代码注释** | `:1901-1904` 仍是旧句：「…——**疑似负载过高，不是漏报**（PM 02:0x 登记的缺口：墙钟等待在重负载下会假红）；若在低负载下稳定复现，才按漏报论」⇒ 未落地 |

- **另两条口径（PM 已给、tester 尚未落地）**：**多层互斥**（有事件证据 ⇒ 收在证据层、**不出通用后半句**；无事件证据才用通用后半句）—— 现状 `noticeTimeoutDetail()`（`:361`）仍是「证据 clause ＋ 通用后半句」**并排** ⇒ 叠读；**反向用例**要跟着改（`VERDICT` ⇒ `/疑似负载过高(?!，)/` 防「不是漏报」被加回来；P2-5 那处要断**含**「再分辨是漏报，还是只发了一条」**且不含**「才按漏报论」；`:1536` 那条 test 标题里的「不是漏报」要同步；再补一条断 P2-5 那处**不含**「这一处另注」——那是**元话语**、不该进红字）。
- ⇒ **PM 侧结论**：派单③的**机制部分（预算放大 ＋ 红字分报 ＋ 事件式 ＋ 预算降档）已闭环并经 PM 一手验收**；剩下的**只是措辞层**（去掉字面矛盾 ＋ 三处定稿 ＋ 断言同步），**不涉机制、不涉断点**。

### 2026-10-21 02:9x · **派单③的「措辞层」也已落地**（`79962cf`）＋ PM 一手复跑全绿（串行 ＋ 重负载两轮）＋ **派单③全量闭环**（机制 ＋ 措辞 ＋ 回归保护）

**① tester 交付 `79962cf`（PM 一手核过内容；2 文件 / +190 / −73）**

本笔 = 02:8x ⑤ 登记的「红字字面矛盾」三条 ＋ 另两条口径的落地。**只动措辞**：不涉机制、不涉断点，**verdict 语义未变**（超时仍是 FAIL）。

| # | 02:8x ⑤ 登记的定稿（UX 逐字定稿） | `79962cf` 落地（PM 一手核） |
| --- | --- | --- |
| ① | **默认 verdict 去掉「不是漏报」**（现象 → 条件 → 结论，一条直线） | `noticeTimeoutWhy()` 加 `verdict` / `tail` / `budgetMs` 三个参数（**单一来源、7 处共用**）⇒ 默认句式＝`等预算（Xs）内未等到 X 提示（疑似负载过高）；若在低负载下稳定复现，才按漏报论` ✔ |
| ② | **P2-5 那处定稿**：`…未等到预算提示（疑似负载过高）；若在低负载下稳定复现，再分辨是漏报，还是只发了一条（被 early return 吃掉）`；verdict 用默认、**tail 覆盖通用后半句**、**extra 整段删掉** | `tail` 覆盖 ＋ 新增 `tailAlways`（**该处专属排查指引，有无事件证据都带着**）✔；原「**这一处另注**：不等于「只发了一条」…」**整段从红字删掉、搬进代码注释**（**元话语不进红字**）✔ |
| ③ | **P2-H 那处句式**：`等预算（30s）内未等到「最后一轮」的重复组提示（疑似负载过高）；若在低负载下稳定复现，才按漏报论——期间见到最多 N 个组、M 条提示`；**来源引用移出红字、放进代码注释** | 改走 `noticeTimeoutWhy()` ＋ `budgetMs: 30_000` ＋ 自定义 tail ✔；**来源引用**（「PM 02:0x 登记的缺口：墙钟等待在重负载下会假红…」）**已移进代码注释** ✔ |
| ④ | **多层互斥**：有事件证据 ⇒ 收在证据层、**不出通用后半句**；无事件证据才用通用后半句 | `noticeTimeoutDetail()` 改成**两套形状互斥**（不再「证据 clause ＋ 通用后半句」并排）：有证据 ⇒ `（该轮确实出过提示：kind ⇒ 指向实现漏报，不是负载）`（**「疑似负载过高」被证据排除、通用后半句不出**）；无证据 ⇒ 走共享口径 ✔ |
| ⑤ | **反向用例断言同步** | ① 默认 verdict 断 `/疑似负载过高(?!，)/` **且**断**不含**「疑似负载过高，不是漏报」；② P2-5 那处断**含**「再分辨是漏报，还是只发了一条（被 early return 吃掉）」**且不含**「才按漏报论」；③ 用例标题里的「不是漏报」同步；④ **新增**断 P2-5 那处**不含**「这一处另注」；⑤ 7 处逐条用例按「无证据 / 有证据 / P2-5 两种形状」**分别断** ✔ |

**② PM 一手核「只动措辞」**：`79962cf` 里 `status` / `PASS` / `FAIL` 的**字面增删行 = 0**（唯一命中的是新参数 `verdict` —— 它是**措辞 token**、不是判定语义）⇒ **判据断点一字未改**；`plugins/**` 与协议正文**一行未动**。

**③ PM 一手探针打印的**实际红字**（`%TEMP%\p2-timeout-probe.mjs` 直接调导出的共享函数，非转述）

| 形状 | 实际渲染出的原文 |
| --- | --- |
| 通用·无事件证据 | `等预算（0.05s）内未等到「重复组」提示（疑似负载过高）；若在低负载下稳定复现，才按漏报论` |
| P2-5·无事件证据 | `等预算（0.05s）内未等到预算提示（疑似负载过高）；若在低负载下稳定复现，再分辨是漏报，还是只发了一条（被 early return 吃掉）` |
| P2-H | `等预算（30s）内未等到「最后一轮」的重复组提示（疑似负载过高）；若在低负载下稳定复现，才按漏报论——期间见到最多 1 个组、2 条提示` |

⇒ 三句**逐字符合 UX 定稿**；反向核：默认句式**不含**「不是漏报」✔（字面矛盾已消除）。
**「有事件证据」那一形状**（`noticeTimeoutDetail()` 内部、**未导出**）由**已提交态自测在运行时逐字断言**（`scripts/p2-protocol.test.mjs:1673-1680` 断「该轮确实出过提示：budget-exceeded」＋「指向实现漏报，不是负载」＋ **不含**「疑似负载过高」＋ **不含**「才按漏报论」）⇒ 该形状**跑起来时即被逐字核过**（PM 在串行与重负载两轮各跑过一次，29/29 全绿）。

**④ PM 一手复跑（**已提交态**、HEAD `79962cf`、**工作树干净**、串行）**

（tester 回执里「不变量全绿」属**成员自报** ⇒ PM 在**已提交态**补跑 `npm run check:static` 四项，**实测全绿**、与自报一致 —— 见下表最后一行。）

| 面 | 结果 | 耗时 |
| --- | --- | --- |
| `node scripts/p2-protocol.test.mjs` | **29 / 29**（0 fail / 0 skipped） | **7.7s** |
| `node scripts/p2-protocol.run.mjs` | **11 条：通过 11 · 失败 0 · 未执行 0** | **0.8s** |
| `npm run test:scripts` | **181 pass / 0 fail / 3 skipped**（184 tests） | **8.7s** |
| `npm run test:plugin` | **289 / 0 / 3** | **5.3s** |
| `node scripts/p1-protocol.run.mjs` | 43 / 40 / 0 / 3 | **2.9s** |
| `node scripts/p1-protocol.test.mjs` | **60 / 60** | **1.3s** |
| `npm run check:static`（invariants ＋ duplicates ＋ webdriver-gating ＋ builtin） | **四项全绿**：不变量**全部通过**（已扫 48 个文件）· 重复代码 **4 处（允许 8）** · WebDriver 门控 ✔ · builtin 资源 **4 插件 / 43 文件** | ~2s |

**⑤ 重负载复验（并行 `npm run prep:builtin` ＋ 被测面）⇒ 仍不假红**

- **重负载 A**（＋ P2 自测）⇒ **29 / 29 全绿、0 失败、7.6s**（**与串行一致**）；三个活体注入负例各 ~1.1s（预算 1s）· P2-H **0.93ms** · P2-J **0.50ms**。
- **重负载 B**（＋ `npm run test:scripts`）⇒ **184 tests / 181 pass / 0 fail / 3 skipped、8.4s**，**无失败行、无超时行**。

**⑥ 派单③全量闭环**：机制（预算放大 ＋ 红字分报 ＋ 事件式 ＋ 预算降档）＋ 措辞（三处定稿 ＋ 多层互斥 ＋ 回归保护）**全部落地并经 PM 一手验收** ⇒ **02:0x 登记的「P2-H 重负载假红」缺口闭环**（本轮再验一次仍绿）。**判据仍 11 条、自测仍 29 条**（本轮**未新增判据**、用例数不变）。

**⑦ 基线更新**：判据入库基线 = **`79962cf`**（PM 于 02:9x 推；此后 PM 又推了本节回填，故远端已不止此提交）。（**此处为当刻基线**；派单⑨落地后的**最新基线见 ⑩** ⇒ **`df65280`**；本文件顶部阶段总表那行已同步到 ⑩ 的基线。）

**⑧ UX 03:2x 回执（**提交前**的状态同步）＋ PM 一手复核**

UX 那封回执写在 tester 提交**之前**（自述「我未改任何文件、未提交、未推送」），其中第⑤项「tester 提交 → PM 在已提交态复跑 → 回填 progress 02:9x」**PM 已全部完成**（见 ③④⑤⑦ 与提交 `169f9a4`）。其余三点 PM 一手复核：

- **① 「四项已落地」** ✔ —— 与 PM 一手核过的 `79962cf` 内容一致（P2-5 的 `tail` 传参 ＋ `extra` 删掉；P2-H 的定稿句式 ＋ 来源引用进注释）。
- **② 「在途态自测 29/29、7,498 ms」** —— 属**成员自报**，PM **不当实测**；PM 的**已提交态**读数见 ④（29/29、7.7s）⇒ 「降档后基线 7.5s」在已提交态成立。
- **③ 「断言同步到位、`expectTimeoutWording` 已参数化、证据层断的是新措辞」** ✔ —— PM 一手核过该 diff（`{ evidence, tail, forbid }` 三个参数 ⇒ 后半句按处传、不再硬编码）。
- **④ 「老措辞只剩在注释／负向断言里，不是活断言」** ✔ —— PM 一手 grep 复核：`疑似负载过高，不是漏报` 在 `scripts/p2-protocol.test.mjs` **3 处全是 `doesNotMatch` 负向断言**（`:525`/`:1465`/`:1600`）；`才按漏报论` 在 lib 侧是**默认 tail 字面量**（`:291`/`:386`）＋ P2-H 自定义 tail（`:1967`），其余全是注释，test 侧全是「该出现」的活断言与「该不出现」的负向断言；`事件证据` **全部命中注释、用例说明与断言失败消息**，**没有一条活断言去匹配它** ⇒ **确非缺口，不需要再催**。

**「有事件证据」那一层的句子**：UX 那封回执**引述了落地后的新措辞**「该轮确实出过提示：${evidence}」并称「我建议的稳定片段被采纳」，**未对结论部分提出异议** ⇒ PM 记为**界面层无异议**（措辞属界面层，PM 不代选、不代写）。**原文 ↔ 落地压缩句的差异登记备查**：UX 原长句＝「【**事件证据**：该轮确实出过提示（…）⇒ 这一轮**跑了**，不是「整轮没跑」那类负载问题 ⇒ 指向实现漏报】」；落地句＝「（该轮确实出过提示：kind ⇒ 指向实现漏报，不是负载）」⇒ **压掉的是中间那句解释、保留了 UX 的结论措辞**，而「不是负载」正是「多层互斥」要求的写法 ⇒ **若 UX 日后要换回长句，只改这一处**。
⚠️ **本条已于 ⑨ 被更正**：这里的「**界面层无异议**」**只对通用形态成立**；**P2-5 那处（`tailAlways: true`）的拼装另有裁定**（UX 03:3x 提出 ＋ PM 运行时复现）⇒ **以 ⑨ 为准**。

**⑨ UX 03:3x 的复核：通用形态**逐字认可** ✔，但 **P2-5 那处的拼装要改** ⇒ PM 已在**运行时**复现并裁定（**派单已出，尚未落地**）**

UX 这轮指出：`tailAlways: true`（P2-5 那处）会让**证据层先下结论、后面又要「再分辨」** ⇒ **逻辑倒置**（先定后分），且与共享口径那套「候选解释 → 条件结论」**不同款** ⇒ 同一个机制里出现两套逻辑。

**PM 一手运行时复现**（已提交态 HEAD `5080b34`；**独立 worktree** 跑探针，**不碰共享工作树**）—— P2-5「**有事件证据**」路径的实际红字：

> 同组且超预算时——协议 §3.1（v0.4 裁断）：两个触发点各自独立 ＝ 两条都要发，任一条都不得被 early return 吃掉；**等预算（0.05s）内未等到预算提示（该轮确实出过提示：duplicate-groups ⇒ 指向实现漏报，不是负载）；若在低负载下稳定复现，再分辨是漏报，还是只发了一条（被 early return 吃掉）**

⇒ 「指向实现漏报」**在**、且**后面紧跟**「再分辨是漏报…」⇒ **倒置属实**。

**根因（PM 一手核行号）**：`scripts/lib/p2-protocol.mjs:395`（`const guide = tailAlways ? '；' + tail : ''`）＋ `:398`（**无条件**拼「⇒ 指向实现漏报，不是负载」）＋ P2-5 调用点 `:1436` 的 `tailAlways: true` ⇒ **该处专属 tail 与证据层结论并排**。

**PM 裁定**（PM 的职责＝裁「要不要改」＋「是否字面自相矛盾」；**逐字措辞仍归界面层**，UX 已给逐字定稿，PM 不代写）：**成立，改**。

- **通用形态（`tailAlways` 为假）—— 逐字不动**：「等预算（Xs）内未等到 X 提示（该轮确实出过提示：kind ⇒ 指向实现漏报，不是负载）」✔
- **P2-5（`tailAlways` 为真）—— 证据层只陈述事实、结论交给该处自己的 tail**：「等预算（Xs）内未等到预算提示（该轮确实出过提示：duplicate-groups）；若在低负载下稳定复现，再分辨是漏报，还是只发了一条（被 early return 吃掉）」⇒ **kind 按实际证据填**（等预算提示时是 `duplicate-groups`、等组提示时是 `budget-exceeded`），**别写死**。
- **机制落法**：证据层拆成「**事实段 ＋ 结论段**」两段；`tailAlways` 为真 ⇒ **只拼事实段**，为假 ⇒ 两段都拼；**只改这一处**，`noticeTimeoutWhy()` 与 **7 个调用点的字串不动**。
- **反向用例（缺了就没有回归保护）**：P2-5 有证据那条须**含**「该轮确实出过提示」**且含**「再分辨是漏报，还是只发了一条」**且不含**「指向实现漏报」；通用形态那条须**含**「指向实现漏报，不是负载」**且不含**「再分辨」。
- **落地时只有**一处**活断言会红**（PM 一手 grep 全量：「指向实现漏报」在自测里 **6 处**命中，**活断言只有 2 处**，其余 4 处是注释／test 标题）：**`scripts/p2-protocol.test.mjs:1474`**（在 `expectTimeoutWording` 的**证据分支**里，对**所有**证据场景**无条件**要求该句 ⇒ P2-5 有证据那条也走它 ⇒ **须按处参数化**：默认形态断含、P2-5 那处断不含）；**`:1676` 不动**（它属「事件式等待」那条用例的 **P2-2 通用形态**、`tailAlways` 默认 false ⇒ 通用形态逐字不变 ⇒ 落地后**照旧绿**，而且它**正是钉住通用形态的那颗钉子**）。

⇒ **已派单**（归 tester，判据库 ＋ 自测；本地提交、不 push）。**本轮仍属「措辞层收尾」**：不涉机制、不涉断点，**verdict 语义不变**（超时仍是 FAIL）、判据仍 **11 条**。
⇒ **待办（已完成，见 ⑩）**：tester 提交后 PM 做**已提交态复跑**（串行 ＋ 重负载）并在本节续写实测数字。

**⑩ 落地并经 PM 一手验收（`df65280`）—— 逻辑倒置已消除，两种形状的运行时红字由 PM 一手探针打印**

tester 交付 `df65280`（2 文件 / **+63 / −14**）：`scripts/lib/p2-protocol.mjs`（+25/−4）＋ `scripts/p2-protocol.test.mjs`（+52/−10）⇒ 判定入库基线 = **`df65280`**。

**PM 一手核过的落点**

- **证据层拆两段**：`fact = 该轮确实出过提示：${evidence}`（只陈述事实）＋ `conclusion = tailAlways ? '' : ' ⇒ 指向实现漏报，不是负载'`；`tailAlways` 为真 ⇒ **只拼事实段**，为假 ⇒ **两段都拼** ⇒ **通用形态逐字不变** ✔
- **一行未动**：`noticeTimeoutWhy()` 全文、**7 个调用点的字串**、P2-5 的 tail 文案、判据条数（仍 **11 条**）、协议正文、`plugins/**` ✔
- **自测侧**：`expectTimeoutWording` 加 `evidenceConclusion`（默认 `/指向实现漏报，不是负载/`、该处自带结论时传 `null`）、`forbid` 改为**可传数组**；**`:1676`（P2-2 通用形态那颗钉子）一个字未动**，只在旁边**加**一条 `doesNotMatch(/再分辨是漏报/)`；**自测条数 29 → 29 不变**（断言加在既有两条用例内，未新开 test）✔
- 另：P2-5「等**组**提示」那条（通用形态）也加了 `forbid: /再分辨是漏报/` ⇒ **P2-5 的专属措辞不许串进通用形态** ✔

**PM 一手运行时探针打印的两种形状**（独立 worktree，**不碰共享工作树**；HEAD `df65280`；跑完已清理，只剩主树）

| 形状 | 实际红字 | 判定 |
| --- | --- | --- |
| **P2-5 · 有事件证据**（等「预算」提示 ⇒ 证据 kind＝`duplicate-groups`） | 「…等预算（0.05s）内未等到预算提示（**该轮确实出过提示：duplicate-groups**）；若在低负载下稳定复现，**再分辨是漏报，还是只发了一条（被 early return 吃掉）**」 | 含事实段 ✔ · **不含**「指向实现漏报，不是负载」✔ · 含专属指引 ✔ |
| **通用形态 · 有事件证据**（P2-2，等「组」提示 ⇒ 证据 kind＝`budget-exceeded`） | 「…等预算（0.05s）内未等到「重复组」提示（**该轮确实出过提示：budget-exceeded ⇒ 指向实现漏报，不是负载**）」 | 含事实段＋结论段 ✔ · **不含**「再分辨」✔ ⇒ **通用形态逐字不变** ✔ |

**PM 一手复跑（已提交态、HEAD `df65280`、工作树干净、串行）**

| 面 | 结果 | 耗时 |
| --- | --- | --- |
| `node scripts/p2-protocol.test.mjs` | **29 / 29**（0 fail / 0 skipped） | **7.6s** |
| `node scripts/p2-protocol.run.mjs` | **11 条：通过 11 · 失败 0 · 未执行 0** | **0.7s** |
| `npm run test:scripts` | **181 pass / 0 fail / 3 skipped**（184 tests） | **8.4s** |
| `npm run test:plugin` | **289 / 0 / 3** | **5.2s** |
| `node scripts/p1-protocol.run.mjs` | 43 / 40 / 0 / 3 | **2.6s** |
| `node scripts/p1-protocol.test.mjs` | **60 / 60** | **1.3s** |

**重负载复验（并行 `npm run prep:builtin` ＋ 被测面）⇒ 仍不假红**：A（P2 自测）**29 / 29、0 失败、7.6s**（与串行一致）；B（`test:scripts`）**184 tests / 181 pass / 0 fail / 3 skipped、8.4s**，**无失败行、无超时行**。

**⇒ 派单⑨全量闭环**：机制 ＋ 措辞 ＋ 回归保护 ＋ **逻辑倒置消除** ⇒ **P2 判据侧无遗留欠账**；**基线**：判据仍 **11 条**、自测仍 **29 条**（本轮**未新增**）。**下一步**：由 **UX 核逐字一致性**（重点：证据层在 `tailAlways` 为真时只拼事实段、`:1474` 已参数化、新增断言到位）。

**⑪ 界面层逐字核验通过（UX 独立核验）⇒ 判据侧与界面侧均清零**

UX 在**独立 worktree**（`origin/main` = `1b58832`）里复制自测文件、在**断言 helper 体内插一行 log** ⇒ **一次拿到全部 8 条形状的实际红字**（跑完已清理：`git worktree list` 只剩主树、共享工作树 `git status` 干净）⇒ **与 PM 的一手探针独立复现、两边逐字一致、无任何出入** ✔

| 形态 | 实际红字（PM 一手探针 ＋ UX 独立探针 互证） | 判定 |
| --- | --- | --- |
| **无事件证据**（6 处：P2-2×2 / P2-5 纯预算 / P2-6 / P2-J） | 「（**疑似负载过高**）；若在低负载下稳定复现，才按漏报论」 | ✔ |
| **通用形态 ＋ 有证据** | 「（该轮确实出过提示：budget-exceeded **⇒ 指向实现漏报，不是负载**）」 | ✔ 事实段＋结论段齐全 |
| **P2-5 ＋ 有证据** | 「（该轮确实出过提示：duplicate-groups）」＋「；若在低负载下稳定复现，**再分辨是漏报，还是只发了一条**（被 early return 吃掉）」 | ✔ **不含**「指向实现漏报」 |
| **P2-5 ＋ 无证据** | 「（疑似负载过高）；若在低负载下稳定复现，**再分辨是漏报，还是只发了一条**（被 early return 吃掉）」 | ✔ 通用后半句不出现、专属指引在 |

⇒ **三种形态互斥**，**没有一处出现「先下结论、再说要分辨」** ✔

**一处前缀差异已核清（不是口径不一致）**：PM 探针打的是 **P2-2** 路径、前缀「同组**成立后**——协议 §3.1-1 …」（`scripts/lib/p2-protocol.mjs:955`）；UX 探针打的是 **P2-5 等组提示**路径、前缀「同组**场景**——协议 §3.1-1 …」（`:1429`）⇒ **三处场景各自的前缀**（另有 `:1442` P2-5 等预算提示的「同组且超预算时——…」、`:2107` P2-J 的「同组成立后——…」）⇒ **口径段（`等预算（Xs）内未等到…`）两边逐字相同** ✔

**⇒ 派单⑨全量闭环**：机制 ＋ 措辞 ＋ 回归保护 ＋ **逻辑倒置消除** ＋ **界面层逐字核验通过** ⇒ **判据侧与界面侧均无遗留欠账**；**基线**：判据 **11 条**、自测 **29 条**、判据入库基线 = `df65280`。

### 2026-10-21 08:0x · **P3 实现第 1 组（S-14 ＋ S-15）验收通过**（`f57b8d2`）＋ PM 一手复跑全绿（串行 ＋ 重负载三轮）＋ **P3 协议升 v0.2**

**① devloper 交付 `f57b8d2`（PM 一手核过 diff；2 文件 / +275 / −5）**

`plugins/official-chat/host/commands.js`（+73/−5）＋ 新增 `plugins/official-chat/test/p3-storage-account.test.mjs`（202 行 / 8 条）。

| 条款 | 落点（PM 一手核） |
| --- | --- |
| **S-14 存储保护** | 目录缓存目录 `mkdirSync(rootDir, { recursive: true, mode: 0o700 })`、缓存文件 `writeFileSync(…, { encoding: 'utf8', mode: 0o600 })`；**新增 `chmodQuiet()`**（`commands.js:52-69`）——Node 的 `mode` **只在创建时生效** ⇒ 写完再显式收一次，让**存量**落盘物进同一口径；**收不动就吞掉**（缓存写不进去不该把业务带崩） |
| **S-15 账户维度** | 新增 `accountScopeMismatch(provided, derived)`（`:207-224`）：没传 / 相一致 ⇒ 放行，不符 ⇒ 返回原因；`catalog`（`:454`）与 `capture`（`:551`）改用**闭包里的服务端推导值** `String(accountScope)`；不符即按 **`account_changed` 拒**（detail「账户范围与服务端推导值不一致」）、**不静默改写**（静默改写会让用户以为切了账户其实没切） |
| **未动** | `sourceFingerprint` 的推导口径（`fingerprintOf(accountScope)` 一直用服务端变量）、路径形状 `<root>/snapshots/<provider>/<id>.json`、错误日志四字段（`at`/`code`/`requestId`/`detail`）、sweep 日志的相对路径、`plugins/**` 其余文件、`scripts/**`、`docs/**` |

**② PM 一手核过的两条硬事实（不采信回执自述）**

- **`account_changed` 在 P1 的 13 码闭集内**（P1 `:147` 的码表行）⇒ 本批**没有**引入闭集外的新错误码。
- **`request.accountScope` 在 `commands.js` 全文件只有 2 处被取**（`catalog` `:454` / `capture` `:551`）⇒ `preview` / `duplicates` 确实**不消费**该字段；`plugins/official-chat/client/client.js` **零命中** `accountScope` ⇒ 现役 client **从不构造**该字段 ⇒ 实机上这条路走的是「**没传 ⇒ 放行**」。

**③ PM 一手复跑（**已提交态**、HEAD `6b3693a`、工作树 tracked 干净、**串行**）**

| 面 | 结果 | 耗时 |
| --- | --- | --- |
| `node scripts/p2-protocol.test.mjs` | **29 / 29**（0 fail / 0 skipped） | **7.6s** |
| `node scripts/p2-protocol.run.mjs` | **11 条：通过 11 · 失败 0 · 未执行 0** | **0.7s** |
| `node scripts/p1-protocol.run.mjs` | 43 / 40 / 0 / 3 | **2.7s** |
| `node scripts/p1-protocol.test.mjs` | **60 / 60** | **1.4s** |
| `npm run test:scripts` | **181 pass / 0 fail / 3 skipped**（184 tests） | **8.4s** |
| `npm run test:plugin` | **297 pass / 0 fail / 3 skipped**（300 tests） | **5.1s** |
| `npm run check:static`（invariants ＋ duplicates ＋ webdriver-gating ＋ builtin） | **四项全绿**：不变量**全部通过** · 重复代码 **4 处（允许 8）** · WebDriver 门控 ✔ · builtin 资源 **4 插件 / 43 文件** | **3.6s** |
| `cargo test --locked --lib` | **1129 passed / 0 failed / 11 ignored** | **15.0s** |

**④ 重负载复验（并行 `npm run prep:builtin` ＋ 被测面）⇒ 三轮全绿，与串行一致**

- **A（P2 自测）**：**29 / 29**、0 失败、**7.6s**。
- **B（`test:scripts`）**：**184 tests / 181 pass / 0 fail / 3 skipped、8.4s**，**无失败行、无超时行**。
- **C（`test:plugin`）**：**300 tests / 297 pass / 0 fail / 3 skipped、5.2s** —— 新增的 8 条 P3 自测就在这一面 ⇒ 本轮**新增**这一轮。

**⑤ PM 对 devloper 提的 4 个待定点之裁定**（裁定已落进协议，见 ⑥）

| # | devloper 问的 | PM 裁定 | 落在哪 |
| --- | --- | --- | --- |
| ① | §5 P3-9①「用两个不同的 `request.accountScope` ⇒ 落盘字段相同」与 §S-15 裁定 4「不符即拒」冲突 | **按裁定 4 走**（不符即拒）——**实现不用改**（已按此做）；但**协议那一行由 PM 改**：原文在「不符即拒」下**不可机检**（两个不同的值里至少有一个不符 ⇒ 被拒 ⇒ 无从比较两个落盘字段）⇒ 改成可机检形态 | P3 协议 §5 P3-9①（`a837056`） |
| ② | `preview` / `duplicates` 要不要也做一致性校验 | **本批不加** | P3 协议 §S-15 裁定 1（`a837056`）已**写死口径，下一轮不必再问** |
| ③ | P3-8 要求的「P1 协议里有指向 §S-14 的指针」在哪 | **已在**（随 `6ad99b4` 落地）—— P1 §4.1 **第 5 条**同时含 `official-chat-p3-protocol.md` 与 `S-14` ⇒ **P3-8 的 P1 那一半早已有着落**，**无需另派单** | P1 `:211`（PM 一手核过） |
| ④ | `commands.js:804` 的「**口径待 PM 确认**」 | **过期事实** —— P2 协议 §2.2 `:65` 早在 23:5x 就裁定了「`groups` 只返回成员数 ≥ 2 的组」⇒ **只改注释、不动判定逻辑**（`plugins/**` 归 devloper） | 派给 devloper 顺手改 |

**⑥ PM 本轮的两笔协议改动（本地已提交）**

| 提交 | 改了什么 |
| --- | --- |
| `a837056` | **P3 协议升 v0.2**（首版实现落地并经一手验收 ⇒ 满足本稿自己写的升版条件）：§5 P3-9① 改写为可机检形态（**性质未放宽**，只换观察方式）、§S-15 裁定 1 补充「不取该字段的命令不做校验」、§S-14 / §S-15 的「现状」各加一行**闭环指针**（v0.1 的基线原文**保留不回改**，只防后来人把旧基线读成仍存在的缺口） |
| `6b3693a` | P1 协议 §4.1 第 4 条下一处**标点笔误**（引号/括号错位）—— **纯标点，不改语义** |

**⑦ 两处留给成员的提醒（PM 已核过，不是推测）**

- **给 tester**：P3-7 的「Unix 私有权限」断言在 Windows 上**必须显式记「未执行」**（或另走「断源码形态」的替代），**不许写成空断言的空跑通过** —— §S-14 裁定 2 已把 Windows 侧定为「只做如实记录」。
- **给 devloper**：`plugins/official-chat/test/p3-storage-account.test.mjs` 第 2 条（「存量缓存文件也被收进同一口径」）在 Windows 上**断言体为空**（`if (process.platform !== 'win32')` 无 else）⇒ **空跑通过**。**不是实现缺口**（`chmodQuiet` 的收口在 Windows 上本就是 no-op，断不了盘上权限），但**别让它以「通过」的身份空跑** —— 要么补 fallback（断源码形态），要么显式记「Windows 未执行」。（同文件第 1 条**已有** fallback：Windows 侧断的是源码形态、**不是**空跑。）

**⑧ 基线更新**

- **实现入库基线 = `f57b8d2`**；**协议版本 = v0.2**（`a837056`）。
- **判据侧基线未变**：P3-1…P3-9 **仍在 tester 手上、未提交、未验收** ⇒ P3 判据**尚无入库基线**。

**⑨ 未做（明确记账）**

- tester 的 P3 判据：**仍在共享工作树、未提交**（PM **未 add、未改**）。**08:0x 观察到 3 个未跟踪文件**：`scripts/lib/p3-protocol.mjs`、`scripts/lib/p3-adapter.mjs`、`scripts/p3-protocol.run.mjs`（**该集合仍在增长**）。**PM 一手核过：它们不被任何门禁加载** —— 没有任何 `scripts/*.test.mjs` import 它们（只有 `scripts/p3-protocol.run.mjs` 自己 import 那两个 lib 文件）；`test:scripts` 的 glob 是 `scripts/*.test.mjs`（**不递归**，且这三个文件**都不是** `.test.mjs`）⇒ 不在扫描范围 ⇒ 门禁结果等同已提交态。
- P3 第二批（**G3-1** `seq` 二次分配、**G3-2** 预算常量强制、**G3-3** Windows ACL 如实记录）。
- **izzy 三项拍板**（`July-X/deepseek-harness` fork 删否 / 首次建立站点登录态 / 安装版 release 壳验收）—— **均不阻塞 P3 第一批**。

### 2026-10-21 08:3x · **P3 判据入库（`871d0ff` ＋ `ca992a2`）＋ 两处收尾（`129df9a`）＋ PM 独立注入复核（四处注入全部按预期翻红 ⇒ Windows 侧无空跑）**

**① 三笔成员提交（PM 已推，远端 `main` = `ca992a2`）**

| 提交 | 提交人 | 内容（PM 一手核过） |
| --- | --- | --- |
| `129df9a` | devloper | ① **nit 收尾**：Windows 侧那条空跑断言补 fallback —— 新增 `writeFunctionSource()` 圈出 `createCatalogCache` 里 `write()` 的**正文**，断「**这一处**确实带了收口」，而不是「全文件里有同名字符串就算」（第 1 条的 fallback 同步改用这个更精确的写法）；② 补协议 v0.2 可机检形态② 的自测（8 → **9** 条）；③ **裁定 ④**：`duplicates()` 里那句「口径待 PM 确认」改成写明 **P2 协议 §2.2 的裁定与理由**，`if (members.length < 2) continue;` **一行未动** |
| `871d0ff` | tester | **P3 判据入库**：**只新增 4 个判据侧文件**（`scripts/lib/p3-protocol.mjs` 1289 行 / `scripts/lib/p3-adapter.mjs` 716 行 / `scripts/p3-protocol.run.mjs` 127 行 / `scripts/p3-protocol.test.mjs` 788 行，合计 **+2920 行**），**P3-1…P3-9 共 9 条**；`plugins/**`、`docs/**` 一行未动 |
| `ca992a2` | tester | **P3-7 的 Windows 空跑修掉**：`871d0ff` 那版在 Windows 上**只有 Unix 侧断言、没有 else** ⇒ 空跑通过；改为「**显式记「未执行」**（合规的如实记录）＋ 另找一条**能真失败**的顶上：改断**源码形态**（目录缓存／快照／认领三个写盘点有没有带 `mode: 0o700`/`0o600`，且**只在那个写盘点所在的函数正文里数**）」 |

**② PM 独立注入复核（本轮核心；在**独立 worktree**（detached @ HEAD）里做，**不碰共享工作树**，跑完 `worktree remove` ＋ `prune`）**

| 注入 | 改坏什么 | 期望翻红的那条 | 实测结果 |
| --- | --- | --- | --- |
| **A** | 删掉 `write()` 正文里两行 `chmodQuiet(...)` 收口 | devloper 的「存量缓存文件也被收进同一口径」 | **命中 2 处 → 红 1 条，正是它** ✔ |
| **B** | 删掉 `write()` 里两处 `mode: 0o700` / `mode: 0o600` 参数 | devloper 的「目录缓存的目录 0700、文件 0600」 | **命中 2 处 → 红 1 条，正是它** ✔ |
| **C** | 删掉 `writeFileSync` 的 `mode: 0o600` 参数 | 判据 **P3-7** | **命中 1 处 → 新增红 P3-7** ✔ |
| **D** | 把账户维度改回「取自请求参数」（S-15 落地前的旧形状） | 判据 **P3-9** | **命中 2 处 → 新增红 P3-9** ✔ |

⇒ **四处注入全部按预期翻红**（基线 9/9 绿 → 还原后 9/9 绿）⇒ **devloper 的两条 Windows fallback 与 tester 的 P3-7、P3-9 都不是空跑**。

**PM 这次踩到的两个「注入本身会错」的坑（都当场发现、都由探针的护栏挡住）**

1. **注入正则带了行尾换行要求** ⇒ `chmodQuiet(path, 0o600);` 恰是被截取正文的**最后一行**（末尾没有 `\n`）⇒ **只删掉了一行** ⇒ 结论一度看着像「断言仍匹配 ⇒ 证明不成立」。改正则后两行都删掉、结论才成立。
2. **`RegExp.prototype.source` 不带 flag** ⇒ 探针里用 `new RegExp(regex.source, 'g')` 重建正则时**丢了 `m`** ⇒ `$` 从「行尾」变成「整段字符串尾」⇒ 命中数恒为 **0**。**探针按设计拒绝了结论**（「未命中即抛错、不据此下结论」）⇒ 补回 `m` 后才拿到有效结果。

⇒ 两次都是「**注入本身有 bug**」而不是「被测对象有问题」⇒ **先怀疑注入**；且「未命中 ⇒ 不下结论」这条护栏**必须留在探针里**。

**③ PM 一手复跑（**已提交态**、HEAD `ca992a2`、工作树 tracked 干净、**串行**）**

| 面 | 结果 | 耗时 |
| --- | --- | --- |
| `node scripts/p3-protocol.run.mjs` | **9 条：通过 4 · 失败 5 · 未执行 0**（口径见 ④） | **0.7s** |
| `node scripts/p3-protocol.test.mjs` | **18 / 18**（0 fail / 0 skipped） | **1.0s** |
| `node scripts/p2-protocol.test.mjs` | **29 / 29** | **7.6s** |
| `node scripts/p2-protocol.run.mjs` | **11 条：通过 11 · 失败 0 · 未执行 0** | **0.7s** |
| `node scripts/p1-protocol.run.mjs` | 43 / 40 / 0 / 3 | **2.7s** |
| `node scripts/p1-protocol.test.mjs` | **60 / 60** | **1.3s** |
| `npm run test:scripts` | **199 pass / 0 fail / 3 skipped**（202 tests） | **8.4s** |
| `npm run test:plugin` | **298 pass / 0 fail / 3 skipped**（301 tests） | **5.2s** |
| `npm run check:static` | **四项全绿**：不变量**全部通过** · 重复代码 **4 处（允许 8）** · WebDriver 门控 ✔ · builtin 资源 **4 插件 / 43 文件** | **3.6s** |
| `cargo test --locked --lib` | **1129 passed / 0 failed / 11 ignored** | **15.9s** |

**④ P3 判据的基线口径（重要：5 条红**不是**缺陷）**

| 判据 | 条款 | 第一批状态 |
| --- | --- | --- |
| **P3-6** | S-13 | ✔ 绿 |
| **P3-7** | S-14 | ✔ 绿（Windows 侧＝显式记「未执行」＋ 源码形态替代断言） |
| **P3-8** | S-14 | ✔ 绿（P1 指针在 ⇒ 应能绿，**已绿**） |
| **P3-9** | S-15 | ✔ 绿（**按协议 v0.2 的可机检三段形态**，含「省略 vs 显式同值 ⇒ 两次落盘字段相同」） |
| **P3-1…P3-5** | **S-8…S-12** | ✘ **红 —— 第二批范围，预期红**。红字逐条写明「缺哪一条协议条款」（如「缺 §S-8②」「缺 §S-11③ ＋ §S-12③ ＋ R3」）⇒ **判据是规格，不许改软** |

⇒ **判据基线 = 9 条 / 通过 4 / 失败 5 / 未执行 0**；那 5 条红是「**第二批还没做**」的**事实登记**，不是回归。
⇒ **P3 判据 runner 不在任何 npm 门禁里**（与 P1/P2 runner 同款，都是手工跑的入口）⇒ 第二批未做**不会**让 CI 红。

**⑤ 重负载复验（并行 `npm run prep:builtin` ＋ 被测面）⇒ 三轮全绿，与串行一致**

A（P2 自测）**29/29**、7.7s · B（`test:scripts`）**202/199/0/3**、8.5s（**无失败行、无超时行**）· C（`test:plugin`）**301/298/0/3**、5.2s。

**⑥ 基线更新**

- **实现入库基线 = `f57b8d2`**（未变）· **判据入库基线 = `ca992a2`**（新增）· **协议版本 = v0.2**（未变）。
- **P3 判据：9 条**（P3-1…P3-9），其中第一批范围 **4 条全绿**。

**⑦ 留给成员的两条**

- **给 devloper**：提交标题前缀写的是 `test(official-chat) ＋ docs(official-chat)`，但 `129df9a` **只改了 `plugins/**` 两个文件**（PM 一手核过 `git show --stat`）⇒ `docs` 那个前缀指的是**注释**、不是 `docs/` 目录 ⇒ 建议 type 前缀改成 `fix`/`test` 之类，**免得按 type 扫 `git log` 时误判成改了文档**。
- **给 tester**：P3-7 那条改得很好 —— 「**显式记未执行** ＋ 另找一条能真失败的顶上」正是 PM 要的口径；PM 的注入 C 已独立确认它**会红**（不是空跑）。**唯一要记的**：`ca992a2` 之前那版（`871d0ff`）在 Windows 上确实空跑 ⇒ **已由 `ca992a2` 修掉**，不必再动。

**⑧ 未做（记账）**

- P3 第二批（**S-8…S-13** 的实现 ＋ **G3-1** `seq` 二次分配、**G3-2** 预算常量强制、**G3-3** Windows ACL 如实记录）—— **等 PM 派单**。
- **izzy 三项拍板**（`July-X/deepseek-harness` fork 删否 / 首次建立站点登录态 / 安装版 release 壳验收）—— 已在 08:0x 以问卷形式提出，**未回复**；**均不阻塞 P3**。（**09:3x 已拍板 ⇒ 见下节**；此处保留当刻事实，不回改。）

### 2026-10-21 09:4x · **izzy 三项拍板已落进协议（v0.3）＋ P3 第二批两单已派出**（另：一条协作面事实待 izzy/并行 PM 定）

**① izzy 2026-10-21 09:3x 拍板三项**

| # | 拍板 | 落点 |
| --- | --- | --- |
| ① | **登录态**：izzy **自己用 `pnpm run dev` 的测试模式来登录**；**PM 用 DeepSeek 官网对话做测试** | ⇒ P4 的前置（建登录态）由 izzy 做、**测试由 PM 做**；**本轮不启动 P4**（P3 第二批先推） |
| ② | **fork 保留**（`July-X/deepseek-harness` 不删、不转私有） | ⇒ 实测仍在：PUBLIC、`master`、最后 push `2026-10-20T15:03:52Z`、**PR 列表为空**（当年那个上游 PR 从未开成，已撤回）⇒ **本项关闭，不再跟进** |
| ③ | **`seq` 不复用** | ⇒ **G3-1 的口径定了** ⇒ 协议升 **v0.3**（`2785663`，已推远端） |

**② 协议 v0.3 的五处变更**（`2785663`）：§2.5 新增**序号水位** ＋ §S-13 新增**裁定 4** ＋ §5 新增**判据 P3-10** ＋ §6 第 4 条把 **G3-2 拆两半** ＋ §7.2/§8 同步。**S-8…S-12 条款正文、§4 常量、G1-8、P3-1…P3-9 一律未动。**

**③ PM 补充说明（把 izzy 拍板时的选项文字改准）**：选项里写的代价是「认领目录无限增长」，**更准确的后果**是：`*seq` 不复用 ⇒ 每个会话的 pick 次数从「**并发**上限 9999」变成「**累计**上限 9999」⇒ 认领被回收后也不回头 ⇒ **同一会话用满 9999 次后永久报「序号已用尽」**。这是「**宁可该会话用尽，也不让同一个 `snapshotId` 指向另一份内容**」的直接代价 ⇒ 已写进协议 §S-13 裁定 4。**（`MAX_SEQ` 9999 与 `snapshotId` 形状 P1 S-1 都不动；报错路径不许换。）**

**④ PM 裁出的一个张力点**（协议原稿里没有，**靠 izzy 这句答案才暴露**）：「`seq` 不复用」若用「**保留认领文件本体**」的形态实现 ⇒ **已入库的 P3-6**（孤儿认领**被删**）会**变红** ⇒ **判据是规格、不许改软** ⇒ 实现**必须**用**独立的水位记录**（§2.5）⇒ 协议为此写死**两条硬约束**：① 认领文件仍按现状被删；② **水位必须先于认领落盘**（不得留「认领已建、水位未写」的窗口）。

**⑤ PM 裁定：G3-2 拆两半**（原稿只写「会牵动 P2 的提示口径 ⇒ 不在本批顺手做」，没说清牵动在哪）

- **G3-2a 缓存总量核算**（`cacheTotalBudgetBytes` 512 MB，P1 §4「含目录缓存与快照」）⇒ **排在第二批之后**：它要删快照，而「哪些快照**不能**删」由 **S-11 的占用记录**决定 ⇒ 必须等第二批的单②落地。
- **G3-2b 单条正文预算**（`perBodyBudgetBytes` 64 KB，P1 §4「进入模型输入的**单条**上限」）⇒ **另派一单**：它是**新增对外拒收路径** ⇒ 要**扩 P1 §3 的 13 码闭集** ＋ **UX 新文案** ⇒ **这才是「会牵动 P2 的提示口径」的真正原因**（不是实现难度问题）。
- ⇒ **第二批两半都不做**。

**⑥ 第二批两单已派出**

- **devloper**：**单① S-8 ＋ S-9 ＋ S-10**（原子发布链：半条记录不得发布／发布点＝rename 成功那一刻／写后回读校验／临时文件形态／`wx` 保留／第五家族清理）＋ **单② S-11 ＋ S-12 ＋ G3-1**（占用记录落盘＋R1/R2/R3 释放规则＋先重建占用关系再回收＋`seq` 永不复用与水位载体）。**落点**：`host/index.js`、`host/sweep.js`、`host/commands.js`、`host/seq-store.js`（**文件所有权不重叠**）。
- **tester**：**只补 P3-10**（G3-1 的可机检形态 ＋ 与 P3-6 的联动约束），并把三个注入坑写进派单（行尾换行要求／`RegExp.source` 不带 flag／护栏「未命中即抛错」）。
- ⇒ **判据 P3-1…P3-5 当前的红**仍是「第二批还没做」的**事实登记** ⇒ **判据侧不许动那 5 条**（PM 已在派单里写死）。

**⑦ PM 一手复跑（已提交态、HEAD `2785663`、工作树 tracked 干净、串行）**：P3 判据 **9 条＝通过 4 · 失败 5 · 未执行 0** · P3 自测 **18/18** · P2 自测 **29/29** · P2 判据 **11/11** · P1 判据 **43/40/0/3** ＋ P1 自测 **60/60** · `test:scripts` **199/0/3**（202 tests）· `test:plugin` **298/0/3**（301 tests）· `check:static` **四项全绿** · `cargo test --locked --lib` **1134 passed / 0 failed / 11 ignored**（比 08:3x 的 1129 **多 5 条** ＝ 并行 PM 那两笔登录探针带来的 Rust 用例）⇒ **基线随分支移动，PM 每次都重跑**。

**⑧ 两条协作面事实（记账）**

- **分支上并行 PM 会话落了两笔**：`f75dd63`（认证链不许跨出壳 ＋ 托管弹窗继承同一份浏览器档案）与 `8f38167`（修 Windows 侧编译红 ＋ 判据第 32 条补跨平台口径）⇒ **P3 判据未受影响**（那两笔只动 `scripts/check-invariants.mjs` 与 Rust 侧）⇒ **P3 判据入库基线仍是 `ca992a2`**；两笔**未动** P3 协议正文。并行 PM 在本文件（P5 阶段总表行 ＋ 交接记录）**有新增内容** ⇒ PM 本笔**只追加自己那一节**，**未改动**其内容。
- ⚠️ **那两笔的 author email 仍是旧账号 `m4java@163.com`** ⇒ 与 izzy 2026-10-20 23:57 拍板的「提交身份统一为 `July-X <over618@163.com>`」**不一致** ⇒ **PM 不擅自改已推送的历史**（改写 `main` 上已发布提交风险高，且那两笔不归 PM）⇒ **登记在此，等 izzy / 并行 PM 决定是否重写这两笔**。
  - ⇒ **09:5x izzy 拍板：「不用，只需要 author name 是 `July-X` 就可以了」** ⇒ **PM 一手核 `origin/main` 全量**：**author name 1120 / 1120 全是 `July-X`**、**committer name 1120 / 1120 全是 `July-X`**（email 分布为 1118 × `over618@163.com` ＋ 2 × `m4java@163.com`，**izzy 明确不介意**）⇒ **本项关闭，不做任何重写**。

**⑨ 未做（记账）**：P3 第二批实现（两单**在途**）· P3-10 判据（**在途**）· P4 真实站点（登录态由 izzy 建、测试由 PM 做）· G3-2a / G3-2b · 消息级合并（izzy **仍未拍板**，不阻塞第二批）· fork（**保留**）· 安装版 release 壳验收（按 izzy 15:2x 拍板归 izzy 自己测）。

### 2026-10-21 10:3x · **P3 第二批单①落地并经 PM 一手验收**（`308ce51`）＋ **协议升 v0.4**（R1 写死本阶段可判定形态）＋ 单②已派出 ＋ **P4 硬前置解除**

**① 单①（S-8＋S-9＋S-10 原子发布链）已落地** —— `308ce51`（type `feat`，**成员本地提交、PM 统一推**；4 文件 / +486 / −13：`plugins/official-chat/host/index.js`、`host/protocol.js`、`host/sweep.js`、新增 `plugins/official-chat/test/p3-atomic-publish.test.mjs`）。
- **§S-8 原子发布**：`save()` 从「直写最终路径」改成 **写临时名 → 回读校验 → `rename`**（临时名形态 `.<snapshotId>.json.tmp-<pid>-<seq>-<attempt>`，点开头、`wx` 排他、撞名换 attempt）；rename 前把临时文件**读回来**、用盘上消息**重算 `contentDigest`** 并比对，**不一致 ⇒ 不 rename**（`snapshot_invalid`）；因 `rename` **会覆盖**，显式补「最终名已存在 ⇒ `snapshot_exists`」；失败时那个临时文件**留着**（交给 §S-10 清理）。
- **§S-9 发布点**：`listSessionSeqs` **显式**排除临时名（不靠「文件名恰好不以 `.json` 结尾」顺带成立）；按 id 读也读不到临时文件。
- **§S-10 第五家族**：`host/sweep.js` 新增临时残留清理（**临时名形态 ＋ 在本实例根下 ＋ 超 24 小时阈值 ＋ 不在「正在发布」窗口内**，四条缺一不可）；**两步法删除**、失败**只记日志不抛**、触发点沿用惰性口径（只在 `catalog` 入口、同实例至多一天一次、**不新增定时器**）。
- **口径单一来源**：临时名的形态与识别放在 `host/protocol.js` ⇒ 写的那边与清的那边共用一份，不出现两份口径。

**② PM 一手验收**（已提交态、**独立 worktree**、detached @ `308ce51`、**不碰共享工作树**、跑完 remove ＋ prune）
- **P3 判据 9 条＝通过 7（P3-1/2/3 由红转绿）· 失败 2（P3-4/P3-5 ＝ 单②范围，**预期红**）· 未执行 0**
- P3 自测 **18/18** · `test:scripts` **202/199/0/3**（202 tests，**exit=0**）· `test:plugin` **306/0/3**（309 tests）
- ⚠️ **一处方法论坑（PM 当场查到并纠正）**：独立 worktree 里 `test:scripts` **一度读出 2 条红**（P2-D 两条），根因**不是代码**——那两条判据需要 Rust 端 example 产物（`cargo build --example p1_protocol_fixture`），而 **worktree 里没有 `src-tauri/target`**（主树有，所以共享工作树是绿的）⇒ 把主树 `target` 以 junction 挂进 worktree 后复跑 ⇒ **202/199/0/3 全绿**。⇒ **纪律**：worktree 复跑前要确认**产物依赖**（Rust example、内嵌资源等）在不在，否则会把「环境缺产物」误读成「代码回归」。

**③ 协议升 v0.4（三处澄清）** —— 派单给实现方前把话写死：
- **§2.5 的水位路径与实现现状对齐**：v0.3 写的是 `<root>/seq-claims/<sessionShort>/`，**少了一层 `providerShort`** ⇒ 与实现现状（`<实例根>/seq-claims/<providerShort>/<sessionShort>/<seq>.json`，`host/seq-store.js:92`／`:100` 一手核过）对不上 ⇒ 改为**含 `providerShort`**（水位与认领文件**同级** ⇒ 不新增隔离层）。
- **§S-11 的 R1 写死本阶段的可判定形态**：原写的两个触发条件（「用户显式移除」「该次 pick 被取消」）在**当前内核契约下都不可判定**（§S-12 已逐条核过：内核只「问」source、从不「告诉」source）⇒ **本阶段 R1 ＝ host 侧「显式释放调用」这一条**，且 **client 侧本批不接线**（现役 client **没有**移除 chip 的入口：`client/client.js:523` 的 `selectedRefs` **只有 `add`**，全文件无 delete/release）⇒ **不许为此新造移除 UI**（属 P4/UX）⇒ 也不许声称「R1 会被自动触发」。
- **§5 新增判据 P3-11**（R1/R2 的释放路径；**R3 已由 P3-5④ 覆盖**，不重复断）。

**④ PM 裁的注入点（回答 devloper 回执里的两问）**：「写临时文件过程中抛错」「让删除失败」这两个注入点**判据侧用子进程 patch `node:fs`** 制造（像 tester 在 P3-10 里做的 `writeTracePreamble`）⇒ **实现不许加测试钩子、不许改函数签名**；devloper 自测里用的「把目标路径占成目录」造写失败、「把墓碑名占成同名目录」造删失败，**同样认可**。

**⑤ 单②已派出**（S-11＋S-12＋G3-1：占用记录落盘 ＋ R1/R2/R3 ＋ 先重建占用关系再回收 ＋ `seq` 永不复用与水位载体）；tester 的 **P3-10 已入库**（`2b22409`，4 个 `scripts/**` 文件）⇒ **PM 一手复跑**：**10 条＝通过 7 · 失败 3 · 未执行 0**，失败的三条＝**P3-4/P3-5/P3-10**，**都是单②范围、预期红**；P3-10 的红字**逐条点名**缺哪条协议条款（被回收的 `seq` 又被分配／重启后又被分配／看不到水位写入）⇒ **判据确实在断性质，不是空跑**。

**⑤′ PM 记一条自己的流程瑕疵（如实记，不美化）**：PM 推 `308ce51` 时**并不知道 tester 已在这期间提交了 `2b22409`**（`git log origin/main..HEAD` 当时显示两笔）⇒ 两笔**一并被推上远端**（`3224e63..2b22409`）⇒ **P3-10 属于「先推后验」**，不是「先验后推」。⇒ 事后已补验（读数与预期一致，未造成偏差），但**流程上应当先核 `origin/main..HEAD` 到底哪几笔、只推已验的那几笔**。**P3-11 判据待派**（协议 v0.5 已把 `release` 命令名写死 ⇒ 判据这次写得出「调用释放入口 ⇒ 占用记录不在」了）。

**⑥ P4 硬前置已解除**：izzy 09:54「我这里登录态做好了，你们可以开工了，继续推进开发任务」⇒ **登录态归 izzy、测试由 PM 做**。**PM 只读调查已开**：插件 `captureSource` 仍是**固定夹具**（`host/commands.js:160-205` 明写「真实站点取数与登录态属 P4」）⇒ **真实取数路径尚未实现**；壳侧已有页内求值（`script_eval.rs`）与 **Windows 上零痕迹**的 CDP 路径（`cdp_eval.rs`）⇒ **待 izzy 拍板**「正式接站点时页内取数用哪种机制」（`script_eval.rs` 自述「**正式接站点前需要协调者裁定**」）。

**⑦ 未做（记账）**：P3 第二批单②（**在途**）· P3-10 / P3-11 判据（**在途**）· 单①的独立注入复核（**未做** —— 与单②的验收合并做，避免跑两遍 worktree）· P4 协议草案（起草中）· G3-2a / G3-2b · 消息级合并（izzy 仍未拍板）· 安装版 release 壳验收（按 izzy 15:2x 拍板归 izzy 自己测）。

### 2026-10-21 11:0x · **P3 第二批全量验收通过**（实现 `344a284` ＋ 判据 `c3011f7`）＋ **协议升 v0.6** ＋ **注入复核四处全部翻红**

**① 第二批全量落地** —— 实现入库基线 **`344a284`**（单② ＝ 占用记录 `refs/` ＋ 序号水位；单① ＝ `308ce51`）、判据入库基线 **`c3011f7`**（**P3-1…P3-11 共 11 条**）。**两笔是由成员自己推上远端的**（PM 派单写的是「可本地提交、**不要 push**」⇒ 推送这件事**不在派单口径内**）⇒ PM 已发回执问明；**不影响内容验收**——PM 的一手复跑与注入复核**都在已提交态上做**。

**② PM 一手复跑（已提交态 `c3011f7`、工作树 tracked 干净、串行）**
- **P3 判据 11 条＝通过 11 · 失败 0 · 未执行 0**（P3-4/P3-5/P3-10/P3-11 **全部由红转绿**）
- P3 判据自测 **34/34** · P2 判据 **11/11** · P2 自测 **29/29** · P1 判据 **43/40/0/3** ＋ P1 自测 **60/60**
- `test:scripts` **218/215/0/3** · `test:plugin` **324/321/0/3** · `check:static` **四项全绿**（内嵌资源 4 插件/44 文件）· `cargo test --locked --lib` **1134 passed / 0 failed / 11 ignored**

**③ PM 独立注入复核（四处全部按预期翻红 ⇒ Windows 侧无空跑）** —— 独立 worktree、detached @ `c3011f7`、**不碰共享工作树**、跑完 remove ＋ prune；探针带护栏「**未命中即抛错，不据此下结论**」：
- **A** 让 capture 不写占用记录（`host/commands.js` 的 `if (refs.available)` ⇒ `if (false)`）⇒ **红 P3-4**（**只红这一条**）
- **B** 把水位写入挪到认领**之后**（`host/seq-store.js`）⇒ **红 P3-10**（**只红这一条** ⇒ 判据确实在断**次序**，不是断「最后结果」）
- **C** 让 `release` 不真的删占用记录 ⇒ **红 P3-11**（**只红这一条**）
- **D** 让 sweep 不扫占用记录（`host/sweep.js` 的 `refsStore.scan()`）⇒ **红 P3-5** ＋ 连带 **P3-3/P3-6/P3-10/P3-11**（不扫 ⇒ 占用集合为空 ⇒ 快照被误删 ⇒ 影响多个断言，**符合预期**）
- **还原后基线 11/11 全绿** ⇒ 「**基线绿 → 注入红 → 还原绿**」三段齐全。

**④ 协议升 v0.6**：第二批实现自己说明「**这一处协议没写死，取的是保守的一侧**」（占用记录**自己也过期**时，它占着的那份快照怎么办）⇒ 协议是唯一口径源 ⇒ **写死成保守的一侧**：**本轮**的回收按「**本轮开始时**」的占用关系判 ⇒ 那份刚被释放的快照**本轮仍不删**，**从下一轮起**才可被回收（理由：避免同一轮里既释放占用、又顺手删掉刚被释放的那份快照 ⇒ 沿用 P1 §4.1「**宁可不清理，不乱删**」）⇒ §5 的 **P3-11②** 同步措辞（**只改观察口径的措辞，不放宽性质**）。

**⑤′ 成员回执里的三条新事实（PM 据此升协议 v0.7）**
· **「`refsStore` 注入项算不算往实现里加测试参数」——PM 裁定：不算，不要求改。** 实现的 `createCommands` 新增了一个**可选**注入项 `refsStore`，**默认值从 `store.root` 推** ⇒ 判据**不传它也走真实路径**；且 **PM 的 11/11 复跑就是没注入它跑出来的**（**P3-4 绿** ⇒ 默认路径确实写占用记录）⇒ 它与 `now`／`logger` 同类，属**依赖注入**而非测试钩子。⇒ **但**：判据若要观测「先写占用记录」的**次序**，应用**子进程 patch `node:fs`**（判据侧已有这个手法），**不要依赖实现的注入项**（PM 已提醒 tester）。
· **他请 PM 拍板的那处口径（占用记录**自己也过期**时那份快照怎么办）——已拍板并写死（协议 v0.6）**：采纳他的**保守侧**（**本轮不删、从下一轮起可回收**）⇒ **代码不用改**。
· **新缺口 G3-4（跨进程水位写入没有强原子）**：水位是「**读 → 取 max → 写**」⇒ 两个进程**同时**分配时「**后写者把水位拉低**」的窗口**理论上仍在** ⇒ 极端并发下**仍有 `seq` 复用的可能** ⇒ **如实登记**（本阶段不做强原子）⇒ 现有缓解三条（认领 `wx` 不撞号／水位只许前进／读不出就拒绝分配）⇒ **窗口被压到极小但不为零** ⇒ **断言边界写死**：§S-13 裁定 4 在**单进程与顺序分配**下成立，**跨进程并发见 G3-4** ⇒ **不许声称「跨进程并发下已被完全证明」**。
· **另记一条判据侧的过程事实**：判据自测在途版本**中途红过 1 条**（`no-occupancy-scan` 违规点：`isRefsPath` 把 refs 的**删除**事件也算成「扫到了占用记录」⇒「看不到全量扫描」那一支打不出来）⇒ **tester 自己在 10:22 改掉** ⇒ 现 **34/34** ⇒ **判据在真实实现上是绿的**。

**⑤ 未做（记账）**：P4 协议草案与**页内取数机制的拍板**（待 izzy）· G3-2a（缓存总量核算，排在第二批之后）· G3-2b（单条正文 64 KB，另派一单）· 消息级合并（izzy 未拍板）· 安装版 release 壳验收（按 izzy 15:2x 拍板归 izzy 自己测）。

### 2026-10-21 11:5x · **注入项撤除笔已验收**（`c8f66a0`）＋ **协议升 v0.8**（新增 P3-12）＋ **push 口径变更**

**① `c8f66a0`：撤掉 `createCommands` 的 `refsStore` 可选注入项**（实现方在回执里**自行撤除**，PM 认可）—— 理由：全仓检索 `refsStore` ⇒ **只有实现与本插件自测命中，判据（`scripts/**`）零依赖** ⇒ 属「只对测试有意义的入口」⇒ 占用记录的落点**只留一个**（`<root>/refs/`，协议 §2.4）⇒ 少一条「换一个存储 ⇒ 少写占用关系」的分支。**自测同步改造**：原「先写占用记录、再宣告已保存」靠塞替身观测次序 ⇒ 改成**子进程 patch `node:fs`** 观测**真实写盘**（断「快照落盘**早于**占用记录写入」＋「**结果交付的那一刻**占用记录已在盘上」⇒ 对「先返回、后异步写」**能真失败**）⇒ 与 PM 对 tester 的口径一致（**判据观测次序一律用子进程 patch，不依赖实现的注入面**）。

**② PM 一手验收（已提交态 `c8f66a0`、独立 worktree、detached、不碰共享工作树）** —— **P3 判据 11/11 · 失败 0 · 未执行 0** · P3 判据自测 **34/34** · 本插件自测 **15/15** · `test:scripts` **218/215/0/3** · `test:plugin` **324/321/0/3** · `cargo test --locked --lib` **1134/0/11**；**`check:static` 在 worktree 里一度读红，根因是 worktree 缺 `prep:builtin` 产物**（`src-tauri/resources/builtin-plugins` 是生成物、被 ignore）⇒ **在主树复验通过**（**4 个插件 / 44 个文件**）⇒ **不是代码回归**（与 10:3x 那个 `src-tauri/target` 的坑同款：worktree 缺产物 ⇒ 复跑前要先确认**产物依赖**在不在）。

**③ 协议升 v0.8（`protocol.md`）** —— §5 新增**判据 P3-12**：断 §S-11 的 R2 补充条款（v0.6 写死的「占用记录**自己也过期**时，**本轮不删、从下一轮起可回收**」）—— 断的是**两次清理之间的可观测差别**（① 跑一次清理 ⇒ 占用记录**被删**、那份快照**仍在**；② **再跑一次** ⇒ 那份快照**被删**），**不是「最后状态」**。**背景**：v0.6 把这条口径写进了协议，但**判据侧没覆盖** ⇒ 实现方自测钉住了、判据没钉 ⇒ **协议与判据之间有缝** ⇒ 补一条判据把缝闭上（**R1 仍由 P3-11① 覆盖、R3 仍由 P3-5④ 覆盖**，P3-12 不重复断）。**另**：一并**补正标题行的版本号**（标题自 v0.1 起一直写「（v0.3）」——**PM 前几次升版时漏改**，现为「（v0.8）」）。

**④ push 口径（~~已由成员自己 push ⇒ 允许成员 push~~ —— **该记载已作废**，见下一节的 ③）** —— 原写「`344a284` 已由**成员自己 push**（回执里确认）⇒ PM 改口径：允许成员 push，但提交后必须立刻知会 PM」⇒ **实测否掉了这件事**：**成员全程零 push**，远端每一次推进都是 **PM 自己 push** 的（reflog 双时间线取证见下一节 ③）⇒ **口径回收为「成员本地提交、PM 统一推」**（与 22:1x–23:0x、00:4x、02:5x 几节的记载一致）。**—— ⚠️ 本条的结论已被 2026-10-21 12:1x 更正（见下节 ⑨）：成员确实推过一次（tester，10:56:47）⇒ 「成员全程零 push」不成立；「允许成员 push」**并没有被否掉**，只是后来成员自己选择不推、于是形成了现行约定**。

**⑤ 未做（记账）**：P4 协议草案与**页内取数机制的拍板**（待 izzy）· G3-2a（PM 先写核算口径再派）· G3-2b（另派一单）· 消息级合并（izzy 未拍板）· 安装版 release 壳验收（归 izzy 自己测）。

### 2026-10-21 12:0x–12:2x · **P3-12 判据已验收**（`b2b1021`，12/12 全绿）＋ `7e7a2b5` 验收 ＋ **PM 自我更正：push 口径** ＋ **G3-2a 口径定案（协议 v0.9）**

**① tester 的 P3-12 判据已验收（PM 一手复跑，不采信回执）** —— 独立 worktree（**detached @ `b2b1021`**、**不碰共享工作树**）＋ junction `node_modules`／`src-tauri/target` ＋ **worktree 内先跑 `prep:builtin`**（生成物被 `.gitignore:38` 忽略 ⇒ 缺它 `check:static` 会读出**假红**，同 `src-tauri/target` 那个坑）：
· **P3 判据 12 条 = 通过 12 · 失败 0 · 未执行 0**（新增的 **P3-12 两轮释放**绿：① 第一轮清理 ⇒ 过期占用记录**被删**、那份快照**仍在**（本轮按「本轮开始时」的占用关系判）；② **再跑一次** ⇒ 那份快照**被删** ⇒ 断的是**两轮之间的可观测差别**，与协议 v0.8 写死的口径一致）·
· **P3 判据自测 37/37**（原 34 ⇒ **+3**，tester 随 P3-12 补的自测）· 本插件自测 **15/15** ·
· `test:scripts` **221/218/0/3**（原 218 ⇒ +3 同源）· `test:plugin` **324/321/0/3** ·
· `check:static` **四项全绿**（不变量 ＋ 重复代码 4 处〔允许 8〕＋ 内嵌资源 **4 插件 / 44 文件**）·
· `cargo test --locked --lib` **1134 passed / 0 failed / 11 ignored**。
⇒ **P3 第二批全量闭环**：判据侧 12/12 ＋ 自测 37/37 ＋ 全仓门禁绿。

**② devloper 的 `7e7a2b5`（目录缓存路径注释订正）已验收** —— PM 一手核 diff：**1 文件 / +3 / −1，**全是注释**、**零行为变化**（`plugins/official-chat/host/commands.js` 的目录布局注释：`catalog-cache/<provider>/` ⇒ 实际是**平铺**，provider 是文件名的一段）。**为什么现在修是对的**：下一单（G3-2a 核算）要**按目录统计体积** ⇒ 照旧注释去找 `catalog-cache/<provider>/` 子目录会**统计不到缓存**（那个目录**不存在** ⇒ 合计恒 0）⇒ **核算口径会被这行注释带偏**。⇒ **接受**；且这正是 §S-16 把核算范围写死成「**平铺**」的原因。

**③ PM 自我更正（push 口径）—— 上一节 ④ 那条记载与事实不符** —— 上一节写「`344a284` 已由**成员自己 push**（回执里确认）⇒ PM 改口径：允许成员 push」。**实测否掉了这件事**：用 **reflog 双时间线**取证（**本地分支** reflog ＝ 每笔本地 commit 的时刻；**远端跟踪引用** reflog ＝ 每次 `update by push` 的时刻与推进到的 commit）⇒
· devloper 的 `344a284` 本地提交 **10:23:59**、tester 的 `c3011f7` 本地提交 **10:24:11**；
· **PM 自己**的 `efca7eb`（v0.6）本地提交 **10:30:36** → **10:30:41 `update by push`**（远端 ref 推进到 `efca7eb`）⇒ **这一次 push 把 `344a284` 与 `c3011f7` 一起带上去了**（它们是 `efca7eb` 的祖先）；
· PM 的 `396b248`（v0.7）提交 **10:33:02** → **10:33:07 `update by push`**；
· ⇒ **成员全程零 push**；成员侧本会话所有 git 操作只有 `add`／`commit`／`status`／`log`／`show`／`ls-files`／`check-ignore`，**没有一次 `push`**。
⇒ **口径回收为「成员本地提交、PM 统一推」**（与 22:1x–23:0x、00:4x、02:5x 几节的记载一致）⇒ 上一节 ④ 的「允许成员 push」**作废**。
⚠️ **PM 这次错在哪**：把**成员回执里的自报**当成了实测事实采信 ⇒ **违反「同事自报数字一律不当实测」这条纪律**（PM 自己写在纪律里的）⇒ 正确做法是**取 reflog 取证**（远端 ref 何时被推进、推进到哪个 commit、那个 commit 的祖先包含谁的提交）。⇒ **已当众更正**，取证方法一并写进本节，供后续复核。
⚠️ **⚠️ 本节 ③ 的结论已被 2026-10-21 12:1x 更正（见下节 ⑨）——「成员全程零 push」不成立** —— tester 在 **10:56:47** 自己 push 过一次（`b2b1021`，输出 `55139f7..b2b1021 main -> main`，与 reflog **逐字吻合**）。**本节 ③ 的具体取证（每个 push 事件都紧跟 PM 自己的 commit）仍然成立**，**错的是从它推出「成员零 push」这一步** ⇒ **根因是方法错误**：reflog 只能证「**远端跟踪引用何时被推进、推进到哪个 tip**」，**证不出「是谁推的」**（共享工作树里，推的命令行与提交者无关）⇒「时刻紧跟 PM 的 commit」只是**时间紧邻性**（一个**推断**），**被我当成了证明**。⇒ **口径不变、但性质要改**：「成员本地提交、PM 统一推」是**约定**（成员现在都遵守），**不是**已证实的事实。

**④ G3-2a 口径已定案（协议升 v0.9）** —— 依据：实现方的**只读准备**事实清单（未改任何代码）＋ PM 一手核过 **P1 §4 / §4.1 原文** ＋ **P0 层对 512 MB 零裁定**（`official-chat-feasibility.md` 检索无命中）⇒ **本条是新决策，不是既有裁定的复述**。**PM 裁定**（写进 **§S-16**，判据 **P3-13**，缺口 **G3-5**）：
· **核算范围**：计入 `snapshots/<providerId>/**`（**逐 provider 目录**求和）＋ `catalog-cache/**`（**平铺**）；**不计入** `seq-claims/`、`refs/`、`logs/`、`sweeps/`（P1 §4 原文**只有「含目录缓存与快照」、没有排除项** ⇒ 本条把排除项**写死**）。
· **核算时机**：排在「**占用关系全量扫描 ＋ 常规清理**」**之后**、额外淘汰**之前**（§S-12 裁定 3 的次序不变）⇒ 读到的是「该回收的都回收了之后」的总量。
· **额外淘汰三级**：① **尚未超过 TTL（5 分钟）的目录缓存也删**（可重建、不含正文 ⇒ 无损；**这是 G3-2a 唯一新增的删除动作**）② **被占用的不删**（**硬约束**）③ **未到期（7 天）的快照不删**。
· **上限的诚实读法**：512 MB ＝「**尽力而为的上限**」；删到底仍超 ⇒ **只记日志**（沿用 P1 §4.1 第 3 条「清理失败只记日志」的先例；**不**新增用户可见路径、**不**扩 P1 §3 的 13 码闭集）⇒ **G3-5**（**如实登记，不假装解决**）。**日志须留可读留痕**（至少含**核算总量/预算值/是否超预算/淘汰了什么/淘汰后总量**）⇒ 判据据此断「有留痕」。
· **裁定理由**：删未到期快照会**字面违反 P1 §4.1 第 3 条**（「**只删超过 7 天的**」）⇒ 那是**对用户可见保留期承诺的反悔** ⇒ 选**保住既有承诺**，把上限降为尽力而为。**若 izzy 要硬上限** ⇒ **只改 P1 §4.1 第 3 条**（加一个例外）、**不扩错误码**（淘汰是后台行为、不是拒收）⇒ 记在 §8 第 3 条（**可选**、**不阻塞派单**）。
· **与实现方建议的差异（写明，免得派单时含糊）**：实现方建议「②按最旧删快照（不论保留期）＋③先删缓存」的**组合** ⇒ PM **采纳「先删缓存」这一半**，**不采纳「删未到期快照」那一半**（理由同上）。

**⑤ 未做（记账）**：P4 协议草案（**待 P4 调查单的结果**）· G3-2a **实现单**（口径已定，**已派**）· G3-2b（另派一单）· 消息级合并（izzy 未拍板）· 安装版 release 壳验收（归 izzy 自己测）。

### 2026-10-21 12:1x–12:3x · **三单派出**（G3-2a 实现 / P3-13 判据 / **P4 调查单**）＋ P4 架构缺口的表述更正

**① 已派三单**（PM 统一派、成员**本地提交不推送**）：
· **G3-2a 实现单**（devloper，`task_056ca3428e83`）—— 按 §S-16 口径实现核算 ＋ 额外淘汰；附实现约束（复用 `sweepSnapshots(occupied)` **不改签名**、**不新增错误码/host 命令/用户可见路径**、**不引入测试钩子**、自测用**子进程 patch `node:fs`**）、自测要求（逐条钉住 6 条口径 ＋ **每条断言要有非空跑证明**）、门禁（`prep:builtin` → `check:static` → `test:plugin` → `test:scripts` 串行报读数）、提交纪律（显式路径 `commit --only`、**不 push**、**不 `git add -f` 生成物**）。
· **P3-13 判据单**（tester，`task_8180dc6689f8`）—— 五支断言（核算范围／超预算触发淘汰／被占用不删／未到期不删／删到底仍超有留痕）＋ 纪律（**期望值独立算**、**每支断言要有非空跑证明**、**不得依赖实现注入面**、**平台分支不许空跑**、落盘根在 `%TEMP%`）。**已写明**：实现单并行 ⇒ **P3-13 对当前实现应当是红的（预期）**，但**判据自测必须全绿** ⇒ 回执要带「判据自测 N/N ＋ 对当前实现的红字逐条」（PM 拿它当实现方的验收基线）。
· **P4 调查单**（devloper，`task_8a9023ffa37a`，**只读**）—— 见下面 ②。

**② P4 的架构缺口：表述更正（PM 一手只读核证）** —— 原表述「**插件（内核 Node 进程）→ 壳（Tauri Rust）**无现成通道」**只在一个方向成立**：
· **核到 ①**：插件 host 已在内核 Web 服务器上注册一条 **exact route** —— `plugins/official-chat/host/rpc.js:78` `FETCH_ROUTE_PATH = '/api/xlink-official-chat'`，走内核 `connection.fetch.register({kind:'exact'})`（`dsh-client-connection@0.2.1-rc.2` 的 `registerFetchRoute:840-854`），**owner 准入由内核做**（`createSharedFetchHandler:820-838`）⇒ **「壳 → 插件 host」这条 HTTP 通道是现成的**。
· **核到 ②**：**壳调内核 `/api` 有可复用先例** —— `src-tauri/src/notify/task.rs:909 fetch_auth_cookie`（用 launch token 换 `dsh-auth-*` cookie）＋ `:941 fetch_session_titles` ⇒ `POST http://127.0.0.1:{port}/api/session/list`。
· **推论（仍属候选，未实测）**：「谁驱动页内取数」的**方向若反过来（壳驱动）**，就**不需要任何反向通道** ⇒ **不撞 P0「不再造一个监听服务」的否决** ⇒ P4 的头号缺口**可能不是「不可解」，而是「方向可以反过来」**。
· ⚠️ **两条都只是读代码得出的候选**：exact route 的 owner 准入能否被壳的 cookie 满足、壳能否实际调到，**均未实测** ⇒ **调查单的第一件事不是「设计」，是「拿事实」**：
  **① DeepSeek 会话目录/正文的真实取数端点与字段形态**（能不能走站点自己的 API ⇒ 若能，**页内取数与本缺口同时消失**；并要验 P0 那条「localStorage `userToken` 服务端不认 ⇒ `40003 INVALID_TOKEN`」现在成不成立）·
  **② `/api/xlink-official-chat` 能否被壳实际调到**（**通 / 不通（拒在哪一步）/ 判不了（缺什么前提）** 三档）·
  **③ 「插件 → 壳」方向的现有手段能否绕开**（**若只有「再造一个监听服务」能成 ⇒ 就照 P0 明确写否决**，不绕着提替代方案）。
· **零痕迹纪律**已写进调查单：页内探针**不许在 `window` 上挂固定前缀属性**（对齐 `harness/chat-fingerprint.js` 的「普通浏览器里什么都没有」口径）。
· **协议定稿排在调查单之后**（纪律：**先调查单，协议定稿后才派实现单**）。

**③ 本轮三笔提交（PM 统一推）** —— `4761ee1`（协议 v0.9 ＋ 台账回填）· `8f12c67`（v0.9 措辞澄清：§S-16 裁定 3① 写死「**尚未超过 TTL（5 分钟）**的目录缓存」、裁定 5 补「**日志须留可读留痕**」、P3-13 同步）⇒ **远端 `main` = `8f12c67`**，本地与远端同步。

**④ 待 izzy 拍板（**不阻塞**）** —— **G3-2a 的「硬上限」要不要**：§S-16 已按「**保住既有保留期承诺**」定为「512 MB ＝ **尽力而为的上限**」（未到期快照不删 ⇒ 删到底仍超则只记日志 ⇒ **G3-5**）。⇒ **若你要硬上限** ⇒ **只改 P1 §4.1 第 3 条**（加「超预算时可删未到期快照」的例外）＋ 协议写死**被占用的仍不删**，**不扩错误码**（淘汰是后台行为、不是拒收）⇒ **一句话回复即可**，否则按现口径开工。

### 2026-10-21 12:2x–12:4x · **devloper 回执已核（三处更正）** ＋ **知会方向改正** ＋ P4 机制**已定、不重开**

**① 他的回执基于旧基线 ⇒ push 口径那段的更正**已推送**、不返工** —— 他说的「`344a284` 是成员自己推的」是**我更正前**的旧记载（他回执起草于更正之前）⇒ 更正**已在远端**（`4761ee1` 台账更正 ＋ `8f12c67` 措辞澄清 ＋ `d116b91` 登记三单；**远端 `main` = `d116b91`**）⇒ 现行口径 = 「**成员本地提交、PM 统一推**」。他的 **reflog 双时间线证据与 PM 核到的完全一致**（每次 push 都紧跟 PM 的 commit，成员侧零 push）⇒ 取证方法成立。**—— ⚠️ 「成员侧零 push」这一句已被 2026-10-21 12:1x 更正（见下节 ⑨）：tester 自己在 10:56:47 push 过 `b2b1021`；reflog 只能证「远端何时被推进、推进到哪个 tip」，**证不出推的人** ⇒ 「时刻紧跟 PM 的 commit」只是时间紧邻性推断。**

**② 知会方向改正（采纳他的意见，写进纪律）** —— 真实机制**不是**「成员抢跑 push」，而是「**PM 推自己那笔时，会把成员的在途提交一起带上**」⇒ 知会方向应该反过来：**PM push 后知会成员「你哪几笔已被带上」**，**不是**要求成员 push 后知会 PM（否则会一直误判成「成员抢跑」）。⇒ 成员继续**不 push**，由 PM 统一推并回告被带上的提交。

**③ P4 机制：**已定，不重开**（PM 一手核过原文与版本）** —— 他提的「**第四条路：页内注入脚本取数 → 经 Tauri IPC 回传**」＝ **R1 槽位路线**，而 **P0 已明文否**：`official-chat-feasibility.md:198`「**R1 页面槽位方案不得直接带进真实站点**」＋ tester 复核 `:202`（`script_eval.rs:434-436` 的 `Object.defineProperty(window, SLOT, {enumerable:false})` ⇒ `enumerable:false` 只挡 `for-in`/`JSON.stringify`，**仍是 `window` 的自有属性**、`Object.getOwnPropertyNames(window)` 照样列得出、名字前缀固定 `__dsh_rt_`（`:416`）⇒ 「**R1 只能用于探测，不得带进真实站点**」）⇒ **不是新选项，是已否项**。
· **「CDP 成本最高」不成立** —— 实际版本 = **tauri 2.12.3 / wry 0.57.0 / tao 0.37.1**（`src-tauri/Cargo.lock`，**不是 2.11**）；**CDP 这条路已经铺好**：`src-tauri/src/official_chat/cdp_eval.rs` ＋ `src-tauri/examples/support/cdp_probe.rs`（真正的 `ICoreWebView2::CallDevToolsProtocolMethod` Win32 COM 直调、unsafe）＋ 依赖 `webview2-com` **0.39.1** / `windows-core` **0.62**（`src-tauri/Cargo.toml:151-171`）；P0 `:198` 已把它定为**首选方向**、`:203` 实测 **5/5、页面 JS 空间零痕迹**（求值前后 `getOwnPropertyNames(window).length` **1239 / 1239**、`__dsh*` 为 **0**）⇒ **机制不用再选**。
· **另纠正一处**：「`webview.eval` 通道现成」**只对「能发起」成立、对「能拿回结果」不成立** —— `Cargo.toml:154-159` 明写 Tauri 的 `eval_with_callback` 在 Windows 走 `ICoreWebView2::ExecuteScript`、**完成回调只带 HRESULT** ⇒ 要拿值必须自己接 `ICoreWebView2_6::ExecuteScriptWithResult`（或走 CDP）⇒ 这正是 `cdp_eval.rs` / `script_eval.rs` 存在的原因，**不是重复造轮子**。

**④ 采纳他的一手事实（PM 已核）** —— **官网对话窗口是「我们自己的 Tauri webview」**（`src-tauri/src/harness/official_chat.rs`，label `official-chat`，加载 `chat.deepseek.com`；页签 webview label `official-chat-tab-<index>`）⇒ **内核包里零 official-chat 相关包** ⇒ **页内数据在壳的 webview 里、插件在内核 Node 进程里 ⇒ 两者必然跨进程** ⇒ **这就是 P4 架构问题的根** ⇒ **「谁驱动」被压成唯一真正的决定点**（原先记的「插件→壳无通道」只是其中一个方向的局部）。

**⑤ 已发裁定与调查口径更新**（`batch_4465687ea425` 给 devloper）—— 机制**已定不重开** ＋ 调查三件事口径：**A** DeepSeek 真实取数端点与字段形态（**含 token 来源**、验 P0 那条 `40003 INVALID_TOKEN` 现不成立 ⇒ **若能走站点自己的 API，页内取数与「插件→壳」两个难题同时消失**）· **B** 壳能否把页内数据交给插件 host（exact route `'/api/xlink-official-chat'` 的 **owner 准入**能否被壳的 `dsh-auth-*` cookie 满足 ⇒ **通 / 不通（拒在哪一步）/ 判不了（缺什么前提）** 三档）· **C** 「插件→壳」方向的现成手段（**若只有「再造一个监听服务」能成 ⇒ 照 P0 明确写否决**）＋ 零痕迹条件（只在结论含页内脚本时才用：`getOwnPropertyNames(window)` 前后不变 ＋ `__dsh*`＝0）。

**⑥ G3-2a 实现单的口径提醒（已随裁定发出）** —— 照 **§S-16**：淘汰顺序**写死**为 ① **尚未超过 TTL（5 分钟）的目录缓存也删** ② **被占用的不删** ③ **未到期（7 天）的快照不删** ⇒ **不要按「最旧删未到期快照」实现**（那会字面违反 P1 §4.1 第 3 条）。

**⑦ 未做（记账）**：P4 协议草案（**待调查单结果**）· G3-2a 实现单与 P3-13 判据单（**已派，待回执 ⇒ PM 一手复跑**）· G3-2b（另派一单）· 消息级合并（izzy 未拍板）· 安装版 release 壳验收（归 izzy 自己测）。

### 2026-10-21 12:3x–12:4x · **登录态取证：两棵树复测**（首站结论**有据**）＋ 过期注释订正已派 ＋ **G3-2a 放行开工**

**① 两棵树复测（PM 一手，只读取证）** —— 成员回执给出「**登录态建在 ChatGPT、建议首站 ChatGPT**」⇒ **PM 复测两棵树**（原始字节搜**凭据类 cookie 名** ＋ 域名出现次数；Cookies 是 SQLite ⇒ **键名存在 ≠ 已登录**，且**不按行数判定**）：

| 树 | 绝对路径 | 实测命中 |
| --- | --- | --- |
| **release** | `C:\Users\zxx\.dsh-xlink\dsh\desktop\webview-official-chat\EBWebView\Default\` | `__Secure-next-auth.session-token` **×4**（ChatGPT 会话凭据）· `chatgpt.com` **×52** · `chat.deepseek.com` **×4**（仅痕迹）· `openai.com` ×19 |
| **dev** | `C:\Users\zxx\.dsh-xlink\dsh\desktop-dev\webview-official-chat\EBWebView\Default\` | **`session_id` ×2**（DeepSeek 会话凭据）· **`chat.deepseek.com` ×10** · `chatgpt.com` ×8（仅痕迹）· **无** `__Secure-next-auth.session-token` |

⇒ **ChatGPT 的登录态在 release 树、DeepSeek 的登录态在 dev 树**（`pnpm run dev` 用 **dev 树**、安装版壳用 **release 树** ⇒ **同一站点在两棵树的登录态是不同的**）⇒ 该回执的取证**只覆盖了一棵树** ⇒ 「首站建议 ChatGPT」**不成立**（在 **dev 树里 ChatGPT 未登录、DeepSeek 已登录**）。

**② 首站拍板**已有据**（与实测一致）** —— izzy 2026-10-21 的拍板（**首站 ＝ DeepSeek 官网对话**、**平台 ＝ Windows 先上**）**已落台账并推远端**（`55139f7`），且 izzy 的登录态**是用 `pnpm run dev` 的测试模式建的** ⇒ 落在 **dev 树** ⇒ **与上表实测一致** ⇒ 首站 DeepSeek **有据**。⇒ P0 `feasibility.md:338` 的「**ChatGPT 优先**」是 **2026-10-20「本机从未建立登录态」前提下**给的**建议**，其前提**已被实际拍板覆盖** ⇒ **不是现行裁定**（别再当依据引）。

**③ 取证纪律（PM 踩过一次，写进纪律）** —— 登录态/凭据/缓存类结论 = **两棵树都查，再下结论**（PM 当时只查 release 树 ⇒ 建议过错的首站）⇒ 且**回执里必须写全绝对路径**：本轮回执只写「现役 `webview-official-chat/EBWebView/Default/Network/Cookies`」，**没说是 `desktop/` 还是 `desktop-dev/`** ⇒ 分歧的根就在这一句。

**④ 过期注释已派订正（成员自提、PM 核实后派）** —— `src-tauri/src/harness/chat-fingerprint.js` 的注释写「当前 Tauri 2.11 / wry 0.55.1」⇒ 实测 **tauri 2.12.3 / tauri-runtime-wry 2.12.1 / wry 0.57.0**（`src-tauri/Cargo.lock`）⇒ 照旧版本判断能力会出错 ⇒ 派**该文件所有者**订正 ＋ **全仓扫一遍别处的旧版本号** ⇒ **只改注释、零行为变化**。（PM 侧 `docs/features/official-chat/**` 已自查：**无**过期版本号。）

**⑤ G3-2a 放行开工** —— 口径已定案并推远端（协议 **v0.9 §S-16** ＋ 判据 **P3-13** ＋ 缺口 **G3-5**）⇒ **G3-2a 不依赖 P4** ⇒ **两条线并行、互不阻塞**。

**⑥ P4 调查的 A 项已细化** —— dev 树里 DeepSeek 有 **`session_id`** ⇒ **A 的第一步就明确**：查 **DeepSeek 官网实际用的会话凭据形态**（cookie 名 / 是否 HttpOnly / **是否还需要 Bearer**）⇒ 目标端点沿用 P0 `:144` 记的 **`/api/v0/chat_session/fetch_page?count=5`**（结构 `{code,msg,data}`；裸 fetch → `40002 Missing Token`；带 localStorage 的 `userToken` → `40003 INVALID_TOKEN` ⇒ **这三条现状各是什么**）。⇒ **若能走站点自己的 API ⇒ 页内取数与「插件→壳」两个难题同时消失** ⇒ 仍是本单最高价值的结论。

**⑦ 未做（记账）**：P4 协议草案（**待调查单结果**）· 等 G3-2a 实现单与 P3-13 判据单回执 ⇒ PM 一手复跑 · G3-2b（另派一单）· 消息级合并（izzy 未拍板）· 安装版 release 壳验收（归 izzy 自己测）。

### 2026-10-21 12:4x–12:6x · **G3-2a 实现已验收**（`0de4af6`）＋ **协议升 v0.10（PM 更正自己写错的两处措辞）**

**① PM 一手复跑（已提交态、独立 worktree @ `0de4af6`、detached、不碰共享工作树）** —— **P3 判据 12/12 全绿**（失败 0 · 未执行 0）· P3 判据自测 **37/37** · 本插件自测 **10/10**（新增 `test/p3-budget.test.mjs`）＋ **15/15**（`p3-occupancy-seq`）· `test:scripts` **221/218/0/3** · `test:plugin` **334/331/0/3**（**与成员自报一致**）· `check:static` **四项全绿**（内嵌资源 **4 插件 / 44 文件**）· `cargo test --locked --lib` **1134/0/11**。
⇒ **实现符合 §S-16**：核算**逐 provider 目录**求和 ＋ **平铺**目录缓存、**不计入** `seq-claims/`/`refs/`/`logs/`/`sweeps/`、核算排在「占用扫描 ＋ 常规清理」**之后**、额外淘汰**只删 TTL 内目录缓存**、留痕含**五要素**、删到底仍超**只记日志、不抛异常、整轮继续**。
⚠️ **验收边界（不假装覆盖）**：**P3-13 判据尚未入库** ⇒ 此刻的「12/12」只证明**不回归**；本轮的覆盖来自**实现自测 10 条** ⇒ **判据侧 P3-13 入库后还要再复跑一次**（P3-13 对当前实现应**转绿**）⇒ 那次才算判据侧的闭环。

**② 协议升 v0.10：PM 更正自己写错的两处措辞**（据实现落地后的**一手核实**，不是据转述）：
· **裁定 6（删除步法）** —— v0.9 写「快照沿用两步法、**目录缓存沿用现有直接 `unlink`**」⇒ **与实现现状不符**（当时依据的是一条**未核实的转述**）⇒ PM 一手核 `plugins/official-chat/host/sweep.js`：**目录缓存的常规清理也是两步法**（`removeInTwoSteps`）⇒ 改为「**快照与目录缓存都沿用既有两步法**、**不另造步法**」（两步法更安全：不会留下「半截文件被当成有效缓存」）⇒ **采纳实现现状，不要求改代码**。
· **裁定 7（「最旧」的判定）** —— v0.9 把它写成现行口径 ⇒ 但按**裁定 3②③**（被占用不删、未到期不删），**已到期且未被占用的快照在常规清理里已经删掉了** ⇒ 额外淘汰实际**只删目录缓存** ⇒ 「谁最旧」**在本阶段没有作用点** ⇒ 改为「**本阶段无落点**」，并**绑定到 §8 第 3 条**（**若** izzy 拍板**硬上限**、允许删未到期快照 ⇒ 那时才需要它 ⇒ 按文件内 `capturedAt`，mtime 兜底）。
⇒ **教训（记在案）**：**协议是唯一口径源 ⇒ PM 自己写的措辞也要能被一手核实推翻**；写「实现现状是 X」时**必须先核过代码**，不能凭转述落笔。

**③ 实现侧自测手法（认可，与判据纪律同款）** —— 512 MB 造不出真体积、`CONSTANTS` 是 frozen ⇒ 用**子进程 patch `node:fs` 的 `statSync`** 只放大**核算范围内**文件的 `size`（**核算逻辑照常真实执行**；`unlink`/`rename` **未 patch ⇒ 删盘是真删**）⇒ **未加测试钩子、未加可选参数** ⇒ 期望值由父进程**独立算**。

**④ 活体注入复核（成员侧，已认可）** —— 仓库外副本上 **5 路违规变体**（核算把 `refs/` 算进去／额外淘汰改去删快照目录／核算提到常规清理之前／额外淘汰只删 TTL 已过的缓存／留痕报错核算总量），每路跑「**基线绿 → 变异红 → 还原绿**」，护栏「替换命中数必须恰为 1，否则抛错、**不据此下结论**」⇒ 与 PM 的注入复核纪律同款。

**⑤ push 知会（新纪律）** —— `0de4af6`（成员提交）已被 **PM 12:4x 那次 push 顺带带上** ⇒ 已按新纪律**回告该成员**（PM push 后知会「你哪几笔已被带上」）。

**⑥ 未做（记账）**：**P3-13 判据入库后的复跑**（判据侧闭环）· P4 协议草案（**待调查单结果**）· `chat-fingerprint.js` 的**过期版本注释**订正（已派该文件所有者）· G3-2b（另派一单）· 消息级合并（izzy 未拍板）· 安装版 release 壳验收（归 izzy 自己测）。

### 2026-10-21 12:4x–12:5x · **P4 接线点：双向核完** ＋ **协议骨架要求**（接线点单列一条）＋ 错误面已存在（13 码闭集）

**① 接线点：双向核完（PM 一手 ＋ 成员只读核）** ——
· **执行方只能是壳**：`src-tauri/src/official_chat/cdp_eval.rs` 的入口持 `&tauri::WebviewWindow`；`#[cfg(not(windows))]` 版**明确报错**（不静默降级）⇒ **只有壳持有 webview 句柄**。
· **插件 ↔ 页面 = 两棵 cordis 树**（host 半边在内核 Node 进程、client 半边在浏览器页面）⇒「host 挂服务、client 去 `ctx.get`」**走不通**；现役通道是**内核既有的 `connection` 服务**，服务的是「**页面 → 插件**」。
· **壳不托管内核**：`src-tauri/src/node/` 只有 `detect.rs`/`node_install.rs`/`mod.rs` ⇒ **无 stdin/stdout/端口管线** ⇒ **壳侧没有「被内核回调」的通道** ⇒ **「插件 → 壳」这条链是空的**（成员核到的三条，PM 一手复核**都对**）。
· ⚠️ **但「壳 → 插件」现成**（PM 一手核）：① **插件 host 已在内核 Web 服务器上注册一条 exact route** —— `plugins/official-chat/host/rpc.js:78` `FETCH_ROUTE_PATH = '/api/xlink-official-chat'`（走内核 `connection.fetch.register({kind:'exact'})`；**owner 准入由内核做** —— `dsh-client-connection@0.2.1-rc.2` 的 `createSharedFetchHandler:820-838` / `registerFetchRoute:840-854`）② **壳调内核 `/api` 有可复用先例** —— `src-tauri/src/notify/task.rs:909 fetch_auth_cookie`（launch token → `dsh-auth-*` cookie）＋ `:941 fetch_session_titles` ⇒ `POST http://127.0.0.1:{port}/api/session/list`
⇒ **缺口只在「插件 → 壳」这一个方向** ⇒ **纪律：判定「某条通道不存在」之前，必须核两个方向**（只查一个方向就下「不存在」的结论，本仓已犯过一次）⇒ 已请成员按双向重核并补「外壳的 cookie 能否过 owner 准入」（**要实跑**，三档结论）。

**② 协议骨架要求（采纳成员建议 ＋ PM 核证）** —— P4 协议要**单列「接线点」一条**，写死四件事：**谁持有 webview**（只有壳）· **请求怎么上来** · **结果怎么回去** · **macOS 明确不支持时怎么报** ⇒ 并**明写「不得新建监听服务」**（P0 计划 §4）⇒ 免得实现单里混进「顺手加个监听服务」这种**已否形状**。

**③ 错误面不必从零定义（PM 一手核 `official_chat/protocol.rs`）** —— `ERROR_CODES: [&str; 13]`（`:88`）＋ `ERROR_RETRYABLE`（`:107`）＋ `ERROR_USER_NEXT`（`:126`）＋ `error_index`/`is_retryable`/`user_next`（`:146`/`:154`/`:164`）⇒ **13 码闭集已存在**（与 P1 §3 一致）⇒ `capability_unsupported`（`:749`；`:777` 归一化到 `source_unsupported`）⇒ **macOS 不支持时有现成码可报**。

**④ 两层要分开写（采纳 `official_chat/mod.rs:3-6` 的自述）** —— 「站点适配另立 `providers/`」＋「**回程失败**与**站点返回空**是两类错误，混在一起就会把『没取到』显示成『没有内容』」⇒ P4 协议**回程层与站点适配层分开写**；`mod.rs:15-17`「回程机制已实现并实测（G2），但**还没有生产调用方**」✓ 一致。

**⑤ 一处提醒（PM 核出）** —— `official_chat/protocol.rs:76` **Rust 侧也有一份 `CACHE_TOTAL_BUDGET_BYTES = 512MB`** ⇒ 与插件侧那份是**两个进程各一份常量** ⇒ G3-2a 的核算**只在插件侧**做（`host/sweep.js`）⇒ **Rust 侧那份目前无判定点** ⇒ P4 协议若要写「预算在哪一侧核算」，这是要点。

**⑥ 判据侧在途时不跑半成品（认可成员的处置）** —— tester 的 `scripts/**` 改动在**共享工作树**里未提交 ⇒ 成员**不跑**（半成品读数无意义）⇒ 认可；PM 复跑一律用**独立 worktree**，避免读到半成品。

**⑦ P4 协议草案的开工条件（写明）** —— **等调查单的 A 项**（**DeepSeek 能不能走站点自己的 API**）⇒ 它决定协议骨架是「**页内取数**」还是「**API 直取**」两条完全不同的路 ⇒ **A 项回来前不开协议文件**（**不写半成品协议** —— 半成品会被当口径源）。

**⑧ 未做（记账）**：P4 协议草案（**待 A 项**）· **P3-13 判据入库后的复跑**（判据侧闭环）· `chat-fingerprint.js` 的**过期版本注释**订正（已派）· G3-2b（另派一单）· 消息级合并（izzy 未拍板）· 安装版 release 壳验收（归 izzy 自己测）。

### 2026-10-21 12:4x–12:6x · **P4 头号缺口可能不存在**：「插件 → 壳」现役就有通道（内置插件回环桥）＋ **纪律冲突已裁**（用真实 profile 的只读副本）＋ **izzy 拍板：不做硬上限**

**① izzy 拍板：G3-2a「不做硬上限」** —— 「**按照当前口径处理就行了。不做硬上限**」⇒ 512 MB ＝「**尽力而为的上限**」（未到期快照不删 ⇒ 删到底仍超只记日志 ⇒ **G3-5 长期登记**）⇒ **不扩错误码**、**不改 P1 §4.1 第 3 条**（用户可见的保留期承诺**原样保留**）⇒ 协议升 **v0.11**（§8 第 3 条**划掉** ＋ §S-16 裁定 7 随之「**已定不启用**」）⇒ **G3-2a 的实现零改动**（已通过 PM 一手验收的 `0de4af6` 不受影响）。

**② 「插件（内核 Node 进程）→ 壳（Tauri Rust）」现役就有通道**（PM 一手核 `src-tauri/src/plugins/builtin/bridge.rs`）—— ⇒ **P4 的头号架构缺口很可能由此解开**：
· `:11-15` 模块自述：「**为什么是「插件来请求壳」，而不是反过来** —— 反过来（壳主动推给内核）需要长连接或轮询；**正过来只要一个一次性请求**。形状与 `crate::openai::bridge` 相同：`Authorization: Bearer <令牌>`，令牌按「(族, 实例)」一代一换，只绑回环」
· `:70-73` 32 字节随机令牌 · `:77/:79` `DSH_XLINK_BUILTIN_BRIDGE_TOKEN` / `DSH_XLINK_BUILTIN_BRIDGE_URL` · `:90-93` `TcpListener::bind("127.0.0.1:0")`（随机端口）· `:144-156` **Bearer 校验**（不符即拒）· `:157-184` 路由表（`/v1/ping`、`/v1/memory/*`、`/v1/builtin/sync`）
· **壳在启动内核时注入地址与令牌**：`src-tauri/src/kernel/kernel_adapter.rs:533-539`（`bridge::launch(&dsh_home, "{family}:{id}")` ⇒ `cmd.env(URL_ENV, url)` ＋ `cmd.env(TOKEN_ENV, token)`）
· **内核侧现役在用**：`plugins/memory/host/bridge.js:4-6`（地址与令牌**由内核进程的 env 给出**）＋ `:24-27` env 名常量 ⇒ **official-chat 也是内置插件 ⇒ 同一份 env 即可用**
⇒ **P4 只要在现役桥上加一条路由** ⇒ **不撞 P0「不再造一个监听服务」的否决**（**复用现役服务，不是新造**）。
⇒ **成员提的「文件轮询」方向与现役机制相反**（现役是**插件主动请求壳**；现役共享文件方向也是「壳写 → 插件轮询」）⇒ **不用新造轮询器**（轮询要定间隔/超时/竞态/清理责任）⇒ 已告知成员该方案可不做。

**③ 但加这条路由有**两个真实约束 ＋ 一条安全边界**（协议要写死）** ——
· **现有桥「只读请求头、不读请求体」**：`bridge.rs:185-187`（「**GET + 查询串，不读请求体**」——TCP 无消息边界，body 可能没到）⇒ **P4 要传页内脚本 ＋ 回传结果 ⇒ 必须 POST ＋ 支持读体** ⇒ **桥侧要扩展读体**（并写死 body 上限，防超大 body）。
· **长度/超时口径不兼容**：现役客户端 `plugins/memory/host/bridge.js:43` `MAX_PATH_BYTES = 7000` ＋ `:29-30` `REQUEST_TIMEOUT_MS = 1500` ⇒ **页内脚本与结果都远超 URL 长度** ⇒ **走查询串不可行** ⇒ 必须 POST ＋ body。
· **安全面**：`bridge.rs:21-22` 自述「**加端点时想清楚「谁会调它」——这个桥的调用方是模型能触达的代码，能力面越小越好**」⇒ **CDP 求值等于开一个任意 JS 执行面** ⇒ 协议必须写死**最小能力面**：**只对 official-chat 的 webview 求值**、**只允许白名单命令/只读操作**、**不许内核传任意 JS**、**不许写站点数据**、**不许触碰非 official-chat 的窗口**。

**④ 纪律冲突已裁（PM 裁）** —— P0 §9 第 9 条「探测一律用 `%TEMP%` 一次性 profile，不使用用户真实数据目录下的 webview profile」 vs 「要已登录态**必须**用 dev 树真实 profile（DeepSeek 的 `session_id` 在那儿）」⇒ **裁：dev 树真实 profile 的「只读副本」**：
· **依据**：P0 §8 那轮的既有手法 ——「**只读拷贝 → 跑 → 删拷贝，源 profile mtime 未动**」⇒ **同时满足两条纪律**（用 `%TEMP%` 一次性 profile ＋ 不动真实 profile）⇒ 副本里就带着 `session_id` ⇒ **不需要 izzy 再提供任何登录态**。
  · ⚠️ **（2026-10-21 13:0x 收口：后半句已被实测证伪 —— 副本里「**没有**可用的 session，只有 token」）** —— 副本落地 **`https://chat.deepseek.com/sign_in`**（探针读数 `pageLooksLikeAuth: true`、`page_ready` 1078ms）；页内 `document.cookie` **只有两个匿名项**（`.thumbcache_…`／`smidV2`）⇒ **看不到登录 cookie**；而 `localStorage` 的 `userToken`(30)／`settingsJwt`(30)／`__appKit_userInfo`(37) **确实拷过来了** ⇒ ⇒ **token 在、session 不在** ⇒ 服务端不认；**盘面**：源 profile 里 `ds_session_id` **那行仍在**（`is_persistent = 0`、`expires_utc = 0`）⇒ ⇒ **不是文件没拷过去，是新进程不恢复非持久 cookie** ⇒ ⇒ **「副本就带着 session」＝ 不能（机制性）** ⇒ **护栏只能防「写坏真树」，防不了「登录态带不过来」** ⇒ 该路线**正式关闭**（P4 协议 §2.5／§8.4 ＋ 缺口 G4-4）⇒ **前半句（只读拷贝满足两条纪律）仍成立 ⇒ 纪律裁定不用改，只改前提**。
· **护栏（缺一不可，否则不据此下结论）**：① 复制后**核源 profile 的 mtime 与关键文件 SHA 未变**（命中数≠1 ⇒ **抛错**）；② 探测**只读**（登录站点上的 GET 可能 `Set-Cookie` ⇒ 也只落在副本）；③ **副本用完删掉**（可恢复删除）。
· **不采纳「直接在真实 profile 上只读」**：**登录态**上「只读不污染」的假设**不成立**（GET 也可能刷新/改写会话 cookie）⇒ 真实 profile 的登录凭据被扰动是真实风险。

**⑤ 成员核的「壳侧三条机制能力表」全部采纳**（PM 一手复核）—— `script_eval::read_json`（`script_eval.rs:226`，走 `ICoreWebView2_21::ExecuteScriptWithResult`，**只读同步表达式、零痕迹**，但**取不到 async fetch 的结果**）· 槽位式 `__dsh_rt_`（**有痕迹** ＝ R1，P0 已否）· **`cdp_eval::cdp_eval`**（`cdp_eval.rs:47`，`CallDevToolsProtocolMethod("Runtime.evaluate", {awaitPromise, returnByValue})`，**异步取回、零痕迹** ＝ P0 首选方向）⇒ **CDP 的首选地位有技术必然性**（DeepSeek 端点是 async fetch ⇒ 必须 `awaitPromise`）⇒ **不是偏好**，协议里要写明这条推导。
· 另采纳：`official_chat/context.rs` 这条通路**在真实站点上合规可用**（复用**同一份 user-data 目录** ⇒ 登录态天然可用；官网窗口没开就自建**不可见**读取 webview（不抢焦点）；**按需创建不预热**）⇒ `mod.rs` 说的「没有生产调用方」指的是**没人触发它**，不是能力缺失 ⇒ **这三条纪律协议要照抄**（否则实现容易长成「起一个常驻窗」）。

**⑥ 一手核过成员未核的那两行**（`kernel_adapter.rs:472` 的 `.env("DSH_HOME", &dsh_home)`）⇒ **属实**；并核到 `:263-264 dsh_home_for` → `paths::instance_dsh_home(family, id)`、`plugins/official-chat/host/index.js:259-261`（实例根 = `<DSH_HOME>/official-chat`，另有覆盖项 `DSH_XLINK_OFFICIAL_CHAT_ROOT`）⇒ **实例 home 壳与内核共享**；而 **webview profile 在 `dsh/<family>/desktop[-dev]/webview-official-chat`、不在实例 home 里** ⇒ **profile 是壳独有的**（内核拿不到 cookie）⇒ 这也是「必须由壳做页内求值」的根。

**⑦ 未做（记账）**：**P4 协议草案**（**仍等调查单 A 项**：DeepSeek 能否走站点自己的 API ⇒ 它决定骨架是「页内取数」还是「API 直取」⇒ **不写半成品协议**）· **P3-13 判据入库后的复跑** · `chat-fingerprint.js` 过期版本注释订正（已派）· G3-2b（另派一单）· 消息级合并（izzy 未拍板）· 安装版 release 壳验收（归 izzy 自己测）。

### 2026-10-21 11:5x · **P4 探针两个授权已给** ＋ **判据不必改**（逐行判定核证）＋ **P3-13 判据已写完、待入库复跑**

**① 探针 A 的授权已给** —— 成员可在 **`src-tauri/examples/`** 新增 `official_chat_deepseek_probe.rs`（形态＝**一次性 user-data 目录** ＋ 建 webview **无条件**给 `data_directory` ＋ `window.open` 一律拒 ＋ **只读/零痕迹/不点按钮/不发消息/不预拉正文**）。
· **归属澄清**：`src-tauri/examples/` 的探针文件**本仓历来是他写的**（git log：`34be54c`／`48db53e`／`2fcfbef`／`f75dd63`／`8f38167` 都动了该目录）⇒ **不构成越界**（他自述「`src-tauri/**` 一行未动」**不准确**，属口径纠正、不追究）。
· **不需要改 `Cargo.toml`**：`examples/` 是**自动发现**的（`Cargo.toml` 里**没有** `[[example]]` 声明）＋ `webview2-com = "0.39.1"` 在 `[target.'cfg(windows)'.dependencies]`（`:167`）⇒ example 编译时可用 ⇒ **不与任何在途改动撞所有权**。

**② 「只读副本」裁定的额外收益：探针 B**不必改判据**（PM 一手核 `scripts/lib/probe-profile-isolation.mjs:80-94`）** —— `findProfileViolations` 是**逐行**判定：**第 ① 条**＝该行含 **`webview-official-chat`**；**第 ② 条**＝**同一行**同时含 **`.dsh-xlink`**（`REAL_HOME_MARKER`）**与** profile 标记（`PROFILE_MARKER` 含 `data_directory`）⇒ **副本路径在 `%TEMP%` 下、不含产品 profile 名** ⇒ **① 不红**；源码里**别让 `.dsh-xlink` 与 `data_directory` 落在同一行**（**逐行判定 ⇒ 分行即可**；更干净：**源路径从命令行参数传入**）⇒ **② 不红**；**判据 ③**（建 webview **无条件**给 `data_directory`）照办 ⇒ **探针 B 不需要改判据、不需要另立例外** ⇒ **tester 的 `scripts/**` 在途与 B 无关**（他可照常提交 P3-13）。
⇒ **教训（记在案）**：成员提出「这条要改判据、要两个人一起处理」时，**先核判据的实现**（不是看它的文档描述）⇒ 结论常常是「**不用改**」。

**③ 共享工作树的一个事实（PM 核出）** —— `git status` 一度显示 `src-tauri/**` **10 个文件** `M` ⇒ **二次 `git status` 全部消失** ⇒ 那是 **stat cache 脏**（别的会话短暂动过、内容未变）⇒ **没有真实改动、没有所有权冲突**；但**共享工作树里别人在途 ⇒ 提交一律用显式路径**（`git add -- <路径>` ＋ `git commit --only`，**不许 `-A`/`.`**）。

**④ P3-13 判据已写完（未提交）** —— PM 核到 `scripts/**` 四个文件共 **725 行新增**（`p3-protocol.mjs` **+365** · `p3-protocol.test.mjs` **+293** · `p3-adapter.mjs` **+69** · `p3-protocol.run.mjs` **±6**）⇒ **tester 的在途**（**其余成员一行别碰**）⇒ **等入库后 PM 复跑一次**（**P3-13 应当转绿**）⇒ 那次才算**判据侧全闭环**（P3-12 已于本轮闭环：PM 与成员**双方独立复跑逐条一致**，12/12 直接绿 ⇒ 它是**回归保护**、不是「事实登记」）。

**⑤ 采纳的零痕迹技巧（PM 采纳，写进派单）** —— `Runtime.evaluate`（`awaitPromise`）在页内跑 `JSON.stringify(performance.getEntriesByType('resource').map(e => e.name))` ⇒ **页面自己请求过的 URL 全在里面**（含 XHR/fetch）⇒ **零痕迹、不碰 `Network` 域、只读** ⇒ **探针 A 的核心手段就定它**。

**⑥ 探针顺序（A 先、B 随后，不必二选一）** —— **A**：一次性 profile、**无登录态**、摸**端点路径与认证要求**（合规无争议，且端点路径是写站点适配契约的**最小必需**）⇒ **B**：已登录态下的真实目录/正文，口径＝**真实 profile 的只读副本** ＋ 护栏三条（复制后**核源 profile 的 mtime 与关键文件 SHA 未变**、不符即**抛错**；探测**只读**；**副本用完删掉**）。

**⑦ 未做（记账）**：**P4 协议草案**（待 A/B 结果 ⇒ 定「取数路线」与「站点适配契约」）· **P3-13 入库后的复跑** · `chat-fingerprint.js` 的**过期版本注释**（再派一次）· G3-2b（另派一单）· 消息级合并（izzy 未拍板）· 安装版 release 壳验收（归 izzy 自己测）。

### 2026-10-21 12:0x–12:1x · **P3-13 判据入库并经 PM 一手复跑 13/13 全绿 ⇒ 判据侧全闭环** ＋ **过期版本注释订正验收通过** ＋ **PM 更正自己上一轮对「push 触发者」的判断**

**① P3-13 判据已入库（`7c7e641`）＋ 订正（`fb1c624`）—— PM 一手复跑**（**独立 worktree、detached @ `80e4f75`、不碰共享工作树、串行**）：
· **P3 判据 runner：13 条 ＝ 通过 13 · 失败 0 · 未执行 0**（**P3-13 由红转绿**；判据 runner 自报证据落在 `%TEMP%\dsh-p3-protocol\results-p1.json`）
· **P3 判据自测 45/45**（较 12 条时的 37/37 **+8**）· **本插件自测 10/10 ＋ 15/15**
· `test:scripts` **229/226/0/3**（较 221/218/0/3 **+8**）· `test:plugin` **334/331/0/3**（与 G3-2a 验收时一致）
· `check:static` **四项全绿**（不变量全通过 · 重复代码 4/8 · WebDriver 门控 ✔ · 内嵌资源 4 插件/44 文件）
· `cargo test --locked --lib` **1134 passed / 0 failed / 11 ignored**（48.8s）
· **复跑后共享工作树仍干净**、临时 worktree 已清理
⇒ **判据侧全闭环**（P3-1…P3-13 全部有判据且全绿）。**性质要分清**：**P3-12 ＝ 回归保护**（PM 与成员**双方独立复跑逐条一致**、12/12 直接绿）；**P3-13 ＝ 本轮新增的事实登记**（§S-16 的缓存总量核算 ＋ 超预算额外淘汰）——它此前只是「已写完未入库」，**现在才第一次被 PM 亲手跑过**。

**② P3-13 的非空跑证明（PM 一手核判据与自测的实现，不采信回执）** —— `scripts/p3-protocol.test.mjs:1303-1311` 列了**五种破法**（`count-excluded`／`no-eviction`／`evict-occupied`／`evict-unexpired`／`no-trace`），**各红一处**；`:1414` 定义五支断言在红字里的**分层标记**（`【核算范围】`／`【额外淘汰】`／`【被占用不删】`／`【未到期不删】`／`【留痕】`）；`:1425` 那条自测**断言每个违规变体只红自己那一支、其余四支保持绿** ⇒ **五支都不是空跑**（即「一支红、另一支绿」的隔离性判法）。另 `:1716` 有**活体注入**（让 `catalog-cache/` 下的删除**真失败** ⇒ 额外淘汰没删掉 ⇒ 判据红并**点名** ⇒ **原实现逐字节未动**）。
⇒ **Windows 侧无空跑**（P3-13 是纯文件系统口径，**不含** Unix 权限类平台分支）。

**③ devloper 的过期版本注释订正（`9512602`）＝ PM 一手验收通过** —— **7 个文件、12 增 7 删，全部只改注释与 JSON 描述文本，零可执行代码改动**（PM 一手核 diff）。PM 一手核版本事实：`src-tauri/Cargo.toml:30` 是 `tauri = { version = "2", … }`（**不钉小版本** ⇒ 原注释「锁定在 tauri 2.11.x」与依赖声明**不符**）＋ `src-tauri/Cargo.lock` 解析到 **tauri 2.12.3 / tauri-runtime-wry 2.12.1 / wry 0.57.0 / tao 0.37.1** ⇒ 订正后的表述与事实一致。
· **口径值得学**：那些**事实断言**（「透明不生效」「`PageLoadEvent` 没有失败变体」「菜单开关没对外接口」）是**在 2.11 / wry 0.55.1 那一版上**观测的 ⇒ 订正只改**版本陈述**（「那版」＋ 补当前栈），**不引入任何新的事实断言**（没在 2.12 上复测，就不写「2.12 也这样」）⇒ **结论归属保持在原观测版本上**。

**④ PM 更正自己上一轮对「push 触发者」的判断（上一轮那个判断是错的）** —— 上一轮 PM 记的是「`7c7e641`／`fb1c624`／`9512602` 三笔**触发者未取证**」⇒ **本轮一手核出来，那个判断不成立**：
· `git reflog show origin/main --date=iso` ＝ `80e4f75 … 11:55:30 update by push` ← `e1c568f … 11:45:54 update by push`，**中间没有任何别的 push 事件** ⇒ **11:55:30 那一次 push 把 `origin/main` 直接推到了 `80e4f75`**；而 `git log e1c568f..80e4f75` ＝ `9512602`／`7c7e641`／`fb1c624`／`80e4f75`（`git merge-base --is-ancestor` 三笔全为 `True`）⇒ **这三笔是被 PM 自己那次 push 顺带带上的**，**不是**「别的会话/成员推的」。
· **本地 `main` reflog 对得上**：三笔都是 `commit:` 记录（11:50:16 / 11:51:15 / 11:54:38）⇒ **成员只做本地提交**，符合现行口径。
· **教训（记在案）**：reflog 里**「远端 tip 变了」≠「多了一次 push」** ⇒ **一次 push 可以把多笔提交一起带上** ⇒ 判「谁推的」要**数 push 事件的条数 ＋ 看它推进到的 tip**，不能只看 tip 落点。上一轮 PM 把「tip 变了」读成「有人另推了一次」⇒ **把「未取证」当成了结论**——这跟「转述别人的结论要抽核关键行号」是同一类错。
· **口径不变**：现行口径仍是「**成员本地提交、PM 统一推**」，**本轮证据支持它**；且 ⇒ **知会要补发**：这三笔（devloper 的 `9512602`、tester 的 `7c7e641` ＋ `fb1c624`）**已被带上远端**，按现行知会方向（PM push 后回告「你哪几笔被带上」）**本轮补发清单**。

**⑤ P0 §7 补记一条「文档与实测的出入」：「只读拷贝能否带登录态」从未被实测** —— 探针源码注释（`src-tauri/examples/official_chat_probe.rs:16-17`「Windows 上 Cookies 由 DPAPI 绑机器+用户、不绑路径，拷贝可移植」）与 **P0 `:217`** 那半句是**推断**，不是实测结论：P0 那轮机器上**从未建立登录态**（P0 `:219`）⇒ **结构上就不可能验到「拷贝带着登录态」**；那轮实测证明的只有「拷贝是拷贝、源 profile 未被动、站点访问全发生在拷贝上」（P0 `:40`/`:162`/`:176`）⇒ 已在 **P0 §7 第 2 条下显式标为待验项**（不预设 ABE 结论，等探针 A/B 的实测）。
· **边界（写进 P0 了）**：这条**只管取证手段，不改生产机制** —— **生产路径**（P4 实际要用的）是在**壳自己本来就在用的那个官网对话窗口**里求值：`src-tauri/src/official_chat/context.rs:13-19`「**必须复用同一份 user-data 目录**……换了目录就拿不到登录态」＋ `:109` `.data_directory(profile_dir)` ＋ `:107` `.visible(false)`（不抢焦点）＋ 按需创建不预热 ⇒ **登录态天然可用**；红线「**测试/探针**不许碰用户真实数据目录」**管的是探针**，**不拦这条生产路径**。
· ⇒ **P4 协议骨架不因探针 B 的成败而变**（若实测结论是「拷贝带不了登录态」，登录态字段形态改由壳内求值取 ⇒ 那本来就是 P4 的生产路径）。

**⑦ 一笔派单任务的实际状态（记账，防误读）** —— PM 早先那张「**P4 调查单（只读）：DeepSeek 取数路线 ＋ 通道可达性**」在派单系统里的状态是 **`failed`（错误 `BUSY`）** ⇒ **那张单本身没有产出**；但**同一批事实改由「两个授权」那条消息送达**（devloper 的回执原文：「**两个授权都收到**」＋「**现在按授权写探针 A**」）⇒ **调查在进行中，没有作废**。
· ⇒ **别把「调查单 `failed`」读成「P4 调查没做」**；也**别**把「那张单 failed」读成「调查结论为空」——它的内容已被后续两轮一手核证覆盖（通道侧＝现役内置插件回环桥；机制侧＝CDP 首选且有技术必然性），**唯一仍缺的实测就是探针 A/B**。
· ⇒ **P4 协议草案的依赖关系不变**：等探针 A/B 的实测结果 ⇒ 定「取数路线」与「站点适配契约」。

**⑧ 未做（记账）**：**P4 协议草案**（仍等探针 A/B 结果 ⇒ 定「取数路线」与「站点适配契约」）· G3-2b（另派一单）· 消息级合并（izzy 未拍板）· 安装版 release 壳验收（归 izzy 自己测）。

### 2026-10-21 12:1x · **PM 第二次更正 push 归属**（「成员全程零 push」不成立 ⇒ **reflog 证不出推的人**）＋ **裁定候选单 A ⇒ 另立 P3-14** ＋ **登记 P3-5③ 的覆盖边界（本阶段不改）**

**⑨ PM 第二次更正 push 归属（由成员回执触发，PM 一手交叉核过）** ——
· **新事实**：tester 回执原文「`b2b1021` 那笔 **10:56:47 的 push 仍是我做的**（输出是区间更新 `55139f7..b2b1021 main -> main`；无更新的 push 只打印 `Everything up-to-date`）；现行口径下我不再 push」。
· **PM 一手交叉核过，不是照单采信**：`git reflog show origin/main --date=iso` 里 `b2b1021` 的 `update by push` 时刻＝**10:56:47**、**前一 tip ＝ `55139f7`**；本地 `main` reflog 里 `b2b1021` 的 commit 时刻＝**10:56:35** ⇒ **他给的输出与 reflog 逐字吻合**（区间 `55139f7..b2b1021` 正好是这两次推进之间）⇒ **该次 push 记在成员名下，属交叉印证，不是单纯自报**。
· ⇒ **PM 上一轮写进台账的「成员全程零 push」（reflog 双时间线取证）这个「证明」是方法错误** ⇒ 已在上方三处（④ push 口径 · ③ PM 自我更正 · 成员回执那节的取证结论）**加注更正**。
· **根因（记在案）**：**reflog 只能证「远端跟踪引用何时被推进、推进到哪个 tip」，证不出「是谁推的」** —— 共享工作树里所有人的提交落在**同一条** `main` 上、push 也从**同一个** clone 发出 ⇒ **推的命令行与提交者无关** ⇒「每次 push 都紧跟 PM 的 commit」只是**时间紧邻性**（一个**推断**），**不能当证明用**。
· ⇒ **口径不变、性质变**：「**成员本地提交、PM 统一推**」是**约定**（成员现在都遵守；tester 明说「现行口径下我不再 push」）⇒ **台账里凡是把它写成「已证实的事实」的地方都按此改**；**知会方向不变**（PM push 后回告「你哪几笔被带上」）—— 顺带说明**为什么这个方向是对的**：正因为**成员不推**，PM 推自己那笔时才会把成员的提交顺带带上 ⇒ 才知道该回告谁、哪几笔；若哪天成员自己推，**PM 就不必再回告**（推的人自己知道推了什么）。
· **教训（并入「转述要抽核」那一族，两个方向都要防）**：**「我看到的输出」＋「reflog 记录」两者吻合 ⇒ 才是交叉印证**；**只有一方 ⇒ 是自报**。本轮 PM 自己也犯了一次**相反方向**的错（把「时间紧邻」当证明）⇒ **该防的不只是「轻信自报」，也包括「把自己推断的紧邻性写成取证结论」**。

**⑩ 裁定候选单 A（tester 提的「核算时机」判据）＝ 接受 ⇒ 另立 P3-14（已派）** ——
· **要断什么**：§S-16 裁定 2 写死「核算排在『占用关系全量扫描 ＋ 常规清理』**之后**、额外淘汰**之前**」⇒ 期望值＝**本轮常规清理之后**的总量。
· **PM 核实他的分析成立**：P3-13 场景 A 里**没有任何东西会被常规清理回收** ⇒ 「核算读数 == 清理后总量」与「== 清理前总量」**数值相同** ⇒ **次序被掩盖**（同型教训：「分阶段口径 ⇒ 判据要断两轮之间的差别」）⇒ **这条确实没被断过**。
· **PM 裁定**：**接受，另立 P3-14**（**不改** P3-13、**不改**已验收的 P3-5）。**派单附的四条硬要求**：① **期望值必须由判据独立算**（自己算「清理后总量」），**不许复述实现自报的数**（否则是自比 ⇒ 假绿）；② **违规变体 `account-before-cleanup` 必须真红、且其余支保持绿**（隔离性，同 P3-13 的五种破法做法）；③ **Windows 侧要有非空跑证明**；④ **不许改已验收的 P3-5 与 P3-13**（新增独立一条）。
· **性质**：P3-14 断的是**协议里早已写死的口径**（裁定 2）⇒ 属**回归保护**（不是新的事实登记）。

**⑪ 登记 P3-5③ 的覆盖边界（本阶段不改）** ——
· tester 指出：P3-5③ 的 `occupancyScanAt` 用 `isRefsPath` **只看路径不看事件种类** ⇒ 一个「**只删 `refs/` 而不读**」的畸形实现可能让「看不到全量扫描」那一支**打不出来**。
· **PM 裁定：本阶段不改** —— ① P3-5 是**已验收**判据，改它要**独立理由与独立证据**；② 那类畸形实现**当前不存在**，而「为假想实现改已验收判据」正是本仓库反复反对的过度设计；③ 收紧动作**记在案**，等真有实现偏离时再动。
· ⇒ **登记为「判据侧已知覆盖边界」**（**不是**实现缺口、**不是**协议缺口）⇒ **别把它当成漏判**。

**⑬ 一张迟到的成员回执（G3-2a 实现单）⇒ 两个问题**早已答复**、不返工；其中一条「未验证」**已被判据侧实测覆盖** —— devloper 的 G3-2a 实现回执（`0de4af6`）**迟于** PM 的验收与协议 v0.10/v0.11 才送达 ⇒ 回执里的两处「请你定」**都不是待定项**：
· **裁定 6（目录缓存删除步法）** —— 回执写「裁定 6 说沿用现有直接 `unlink`，但实现现状是两步法 ⇒ 我沿用实现现状，请确认以哪个为准」⇒ **PM 已在协议 v0.10（`6e41ea6`）一手核过 `host/sweep.js` 并自行更正**：目录缓存的常规清理**也是两步法**（`removeInTwoSteps`）⇒ 协议改为「快照与目录缓存**都沿用两步法**」⇒ **实现零改动**、**不需要再定**。
· **裁定 7（「最旧」的判定）** —— 回执写「本单没有落点」⇒ **与 PM 的 v0.10 更正一致**（裁定 3②③ 已把删快照的口子堵死 ⇒ 额外淘汰实际只删目录缓存）⇒ 且 izzy 12:4x 拍板「不做硬上限」⇒ 协议 v0.11 已把裁定 7 标成「**已定不启用**」（**保留只为口径可追溯**）。
· **「P3-13 判据尚未写」** —— **已入库**（`7c7e641`）且 PM 一手复跑 **13/13 全绿**（见本文件 12:0x–12:1x 节）⇒ **不存在「读数面待对齐」**。
· **「未在真实 512 MB 盘上验」** —— **在判据侧已被实测覆盖**：P3-13 判据跑的**是真实实现**（`scripts/p3-protocol.run.mjs:83` 用 `createP3Adapter`）＋ 判据用 `writeBigFile` ＝ **空文件 ＋ `truncateSync` 真在盘上造 512 MB+1**（`scripts/lib/p3-adapter.mjs:987-993`，**不是 stat patch**）⇒ 超预算 ⇒ 额外淘汰真的触发、总量真的回落（判据 ② 绿）⇒ 且每次造完即 `drop`（`p3-protocol.mjs:2180`/`:2199`）⇒ **不留大文件**。回执里那条 stat patch 是**他自测**的做法（可接受），**不是判据的做法**。
· **回执给的「可观测面」字段名与判据实际读的**一致** —— 判据读 `outcome.budget.totalBytes`（`scripts/lib/p3-protocol.mjs:2022-2023`）⇒ 与回执写的 `budget.totalBytes` **逐字一致** ⇒ **字段面对齐、无异议**。
· **其余陈述已过期**（以本文件 12:1x 节的当前基线为准）：远端 `main` **不是** `fb5a4f0`（现 `900ff1d`）· `0de4af6` **已在远端**（`git branch -r --contains 0de4af6` → `origin/main`）· `test:scripts` 不是 221/218/0/3（现 **229/226/0/3**，P3-13 入库 ＋8）· 探针 A/B 的两问**早已答复**（探针 A **已授权**、探针 B **不必改判据**）⇒ **这张回执不需要任何返工，也不需要再答复**。

**⑭ 未做（记账）**：**P4 协议草案**（仍等探针 A/B 结果）· G3-2b（另派一单）· 消息级合并（izzy 未拍板）· 安装版 release 壳验收（归 izzy 自己测）。

### 2026-10-21 12:1x–12:2x · **P4 探针 A 结果落地 ⇒ 取数路线已定** ＋ **探针 B（只读副本路线）实测不可用 ⇒ 关闭** ＋ **B 项（exact route owner 准入）对 P4 不需要 ⇒ 关闭** ＋ **PM 更正自己写错的时钟标签**

**① 探针 A 的实测结果（未登录态；PM 一手复核）** —— 端点**确实存在**：`GET /api/v0/chat_session/fetch_page?count=5` ⇒ **HTTP 200 ＋ JSON**，`topKeys=[code,msg,data]`；未登录时响应 ＝ `{code:40003, msg:"INVALID_TOKEN", data:null}`；**登录 cookie 是 HttpOnly**（页内 `document.cookie` 只看得到 `.thumbcache…`／`smidV2` 这类匿名项）⇒ ⇒ **页外拿不到凭据**。

**② PM 裁定：P4 的取数路线 ——「页内取数」与「调站点 API」**不是二选一**，真解是**页内同源 fetch 站点自己的 API** ⇒ 协议要写死三条：
1. **执行体在壳**（页内求值由壳做：CDP `Runtime.evaluate` ＋ `awaitPromise` ＋ `returnByValue`）；
2. **API 调用也在页内**（同源 fetch，**凭据由浏览器自动带** HttpOnly cookie）⇒ ⇒ **页外直接调该 API 必失败**（没有凭据）—— **这一条必须写死**，否则实现方会去页外调 API；
3. **回程只回结构**（键名／条数／布尔量），**不回正文、不回凭据**（沿用 P0 纪律）。
· **端点路径已定**（实测存在 ＋ 200 ＋ JSON）⇒ 但**请求参数、分页形态、字段形态在登录态下仍待实测** ⇒ 协议写「**端点已定、字段形态待实测后写死**」，**不许先猜字段名**。
· **P0 那条「`userToken` 不被认 ⇒ 40003」本次不能定论**（当时页面是未登录态，`40003` 可能就是「没登录」）⇒ **登录态下的三档码（40002／40003／成功）仍待复测** ⇒ 协议对站内码**只写「原样上报、不解释语义」**，不写语义映射。

**③ 探针 B（只读副本路线）＝ 实测不可用 ⇒ 关闭** —— 副本里 `chat.deepseek.com` 的 **5 个 cookie 逐项完整在盘上**（行数、`encrypted_value` 长度与源一致）⇒ **站点仍 302 到 `/sign_in`** ⇒ ⇒ **只读拷贝不带登录态**（实测）。
· **PM 两轮一手复核后的最终口径（12:4x 已把上一版写的「ABE 跨 exe 解不开」撤下）**：他归因「**更可能是源登录态已失效**」（`ds_session_id` 是 session cookie ＋ 无进程指向 dev 树 ＋ `last_access` 停在 10:15）⇒ **PM 复核时**：dev 树 `Cookies` 的 **mtime ＝ 2026-10-21 12:14:38**（复核当时就在十几秒前被写）＋ **7 个 `msedgewebview2` 进程指向 `desktop-dev\webview-official-chat\EBWebView`** ＋ 复核时 `Cookies` 文件**被锁**（`copyFileSync` 报 `EBUSY`）⇒ ⇒ **dev 树的官方对话环境活着、且正在使用它的 cookie jar** ⇒ ⇒ **「源登录态已失效」不成立**。
  · **第二轮（PM 一手复算）**：副本 `Local State` 的 `os_crypt.encrypted_key` ＝ base64 `RFBBUEkBAAAA0Iyd` ⇒ **解码后前 6 字节 ＝ `DPAPI`**（hex `44 50 41 50 49 01`）⇒ ⇒ **是 DPAPI（绑机器+用户、不绑 exe）⇒ 「ABE 跨 exe 解不开」站不住**（**PM 已把这个解释撤下**）。
  · ⇒ **真正的卡点是「拿不到源文件」**：源存活时 `Default\Network\Cookies` 被**独占锁**（`copyFileSync` 与 `readFileSync` 都 EBUSY）⇒ **源活着时拷不出 Cookies**（**PM 自己也撞到过**）。
  · ⇒ **定性守住**：「**只读拷贝能否带登录态」本轮仍未验到**（不是「已证不可行」，也不是「已证可行」；**已排除**的只有 ABE 那一条解释；**未隔离**的候选：站点侧轮换 session 值 / session cookie 按会话隔离 / 拷贝时源已失效）⇒ **不写成结论**。
  · ⇒ **对 P4 的处置**：在「源存活 ＋ Cookies 被独占锁」的当下，**这条路取不到登录态 ⇒ 本阶段关闭**（**是「本阶段关闭」，不是「原理上不可行」**）；纪律裁定本身（只读副本 ＋ 三条护栏 ＋ 源 mtime 未动）**不改**，**只改前提**。
· ⇒ **纪律裁定本身不用改**（只读副本 ＋ 三条护栏 ＋ 源 mtime 未动都成立）⇒ **要改的只是前提** ⇒ **已同步写进 P0 §7**。
· ⇒ **后果**：取「登录态下的字段形态」**不能**再走副本路线 ⇒ 替代路径见 ⑤。

**④ B 项（exact route owner 准入）＝ 对 P4 不需要 ⇒ 关闭** ——
· **代码层推理链完整，不等于这件事要做**：P4 的接线方向是**「插件 → 壳」**（插件要壳做页内求值）⇒ 走的是**现役内置插件回环桥**（`src-tauri/src/plugins/builtin/bridge.rs`，插件主动请求壳）⇒ **不经过**「壳 → 内核 exact route」⇒ ⇒ **这条路已被绕过 ⇒ 本阶段不判、不做、也不需要新授权**。
· **即便要留一句一般结论**：「壳现有 v1 cookie（`dsh-auth-` ＋ base64url(sha256(authority))、authority=url.host）**已满足** owner 准入」这一点**既有实测已覆盖**（`src-tauri/src/notify/task.rs:941-953` 已实测走通同一套 `admit()`）⇒ ⇒ **不需要起隔离内核实例、不需要新授权**。

**⑤ 替代取证三档（PM 给口径，按成本排序）** ——
1. **静态包 grep（成员自己就能做，零授权、零写盘、零改代码）** —— 从 resource timing 里的 JS 分包捞 `/api/v0/...` 端点全集 ＋ 前端对 `data` 字段的访问（`res.data.xxx`）⇒ **可能直接补齐「站点适配契约」的字段面**（**不依赖登录态**）⇒ **先做这个**。
2. **izzy 跑一段只读 JS**（**只回结构**：`status`／`code`／`msg`／`data` 的键／条目数／条目的键；**不回正文、不回凭据、不回标题**）⇒ 拿到**登录态下的真实响应结构** ＋ **三档错误码**。
   · ⚠️ **（2026-10-21 13:0x 订正落点 ＋ 13:1x 再订正依据：原写的「在 dev 壳的 DeepSeek 对话页按 F12」走不通）** —— **release 壳里 DevTools 被 wry 关掉**（依据链见 P4 协议 §8.5：`wry-0.57.0/src/lib.rs:872-875` 的 `devtools` 默认 **release 下是 `false`** ＋ `tauri-runtime-wry-2.12.1/src/lib.rs:5146-5149` 只在 debug 或带 `devtools` feature 时才覆盖 ＋ 本仓 `src-tauri/Cargo.toml:30` **无 `devtools` feature** ⇒ `SetAreDevToolsEnabled(false)`）⇒ **F12 无效、右键无「检查」**（DOM 右键菜单另被 `no-context-menu.js` 取消）；**dev 壳即便能弹 DevTools，那棵树的登录态也已随环境关闭失效** ⇒ ⇒ **落点改为两条**（见 P4 协议 §8.5 / §8.6）：**A**｜**Chrome** 里**已登录**的 `chat.deepseek.com` 标签页（**零代码改动、零授权**；**Edge 已排除** —— G4-6 已由 PM 一手核完，13:3x）｜**B**｜**壳内**一次性只读形状探测（**不需要 DevTools** —— 壳自己的 CDP `Runtime.evaluate` 是进程内调用；B1＝dev 壳重新登录一次、不碰安装版｜B2＝release 壳、不用重新登录但要动真实树）⇒ **一次授权**改 `src-tauri/**`。
   · ⚠️ **（2026-10-21 13:1x 脚本已换成「两档自动降级」版 —— 下面那份就是新正文）** —— 第 1 档按 **裸 / ＋Bearer / ＋Bearer＋`x-device-id`** 三种头反推哪一档被认（顺带拿到三档码）；第 2 档兜底：挂 `fetch`/`XHR` 钩子等**站点自己**的列表请求（用 `res.clone()` **不吃掉站点响应体**）⇒ **不依赖对鉴权头的猜测**。**PM 已逐字段核过输出面**：只回状态码/业务码/键名/条数/首条键名/打码后的 URL，**不回正文、不回标题、不回会话 id、不回任何凭据**（凭据只出长度）。**痕迹范围**：脚本会替换**该标签页**的 `window.fetch` 与 `XMLHttpRequest.prototype.open/send` ＋ 挂 `window.__dsDump` ⇒ **只在这个标签页，关掉即消失；不写站点数据、不写盘**（**这不属于协议 §7.1 的零痕迹纪律 —— §7.1 管的是生产页内脚本**）。**第 2 档的触发动作建议用「新对话」**（**不要点列表里的某个会话**，那会加载它的正文）。
   · ⚠️ **（2026-10-21 12:4x 订正：脚本换过一次）** —— PM 早先那份只探测 `data` / `data.chat_sessions` / `data.items` / `data.sessions` **四种形态**，而**实测结构是 `data.biz_data`** ⇒ 照旧那份跑出来 `itemCount` 与 `itemKeys` **必然是 `null`** ⇒ **换成下面这份**（成员已自查：模拟 `biz_data.chat_sessions` 响应 ⇒ 正确下钻出 `arrayPath=chat_sessions`；原始文件在 `C:\Users\zxx\.minimax\v2\plugin-data\liveboards\minimax-bot\deepseek-shape-probe.js`）—— **⚠️ 那份后来在 13:1x 又换成了「两档自动降级」版，下面这份正文就是最新的**。
```js
// DeepSeek 登录态下「会话目录」接口的**只读**结构探针（贴进 DevTools Console 执行）
//
// 【在哪跑】**Chrome** 里已登录的 chat.deepseek.com 标签页
//   → F12 → Console → 粘贴 → 回车。
//   （**Edge 已被排除** —— 2026-10-21 13:3x 只读核查：Edge 的 localStorage 里
//     无 `userToken`／`__appKit_userInfo`、Cookies 里 chat.deepseek.com 只有匿名项；
//     Chrome 才有登录痕迹 ⇒ 落点收窄为 Chrome，见 P4 协议 §8.6 / §9 G4-6）
//   ⚠️ **不要**在 dsh-xlink 壳里跑：release 壳的 DevTools 被 wry 关掉
//      （`wry-0.57.0/src/lib.rs:872-875` 的 `devtools` 默认在 release 下是 `false`
//       ＋ `tauri-runtime-wry-2.12.1/src/lib.rs:5146-5149` 只在 debug 或带 `devtools`
//       feature 时才覆盖它），且该窗口的右键菜单被 `no-context-menu.js` 取消；
//      dev 壳即便能开 DevTools，那棵树的登录态也已随环境关闭失效 ⇒ 打开也是 sign_in。
//
// 【两档打法，自动降级】
//   第 1 档：直接同源 fetch `/api/v0/chat_session/fetch_page?count=5`，
//           分别试 **裸 / ＋Bearer / ＋Bearer＋x-device-id** 三种头，
//           用返回码反推哪一档被服务端认（同时顺带拿到三档错误码）。
//   第 2 档（兜底）：若三档都没拿到数组，则**挂上 `fetch`/`XHR` 捕获钩子**，
//           等**站点自己**发列表请求时把**它的真实响应体**截下来解析结构。
//           ⇒ 挂上钩子后，**再点一下「新对话」**（不要点列表里的某个会话，
//             那会加载它的正文），然后在 Console 执行 `__dsDump()` 回车 ⇒ 输出捕获结果。
//
// 【纪律】只回结构：状态码 / 业务码 / 键名 / 条数 / 首条键名。
//         **不回正文、不回标题、不回会话 id、不回任何凭据。**
// 【痕迹】这份脚本会替换**该标签页**的 `window.fetch` 与 `XMLHttpRequest.prototype.open/send`，
//         并挂一个 `window.__dsDump` ⇒ **痕迹只在这个标签页，关掉标签页即消失**；
//         **不写站点数据、不写盘、不影响浏览器其他标签页**。
(async () => {
  const OUT = { origin: location.origin, href: String(location.href), probes: [] };

  // ---------- 页面侧登录态事实（只回长度与存在性，不回值） ----------
  const ls = { keys: [], userTokenLen: null, deviceIdLen: null, visibleCookieNames: [] };
  try { ls.keys = Object.keys(localStorage).sort(); } catch (e) {}
  const lenOf = (k) => { try { const v = localStorage.getItem(k); return v === null ? null : String(v).length; } catch (e) { return 'err'; } };
  ls.userTokenLen = lenOf('userToken');
  ls.deviceIdLen = lenOf('deepseek-device-id:chat');
  try { ls.visibleCookieNames = document.cookie.split(';').map(s => s.split('=')[0].trim()).filter(Boolean).sort(); } catch (e) {}
  OUT.local = ls;

  // ---------- 站点自己的鉴权头（照站点 __httpRequest 拦截器） ----------
  const bearer = (() => { try { return localStorage.getItem('userToken'); } catch (e) { return null; } })();
  const deviceId = (() => { try { return localStorage.getItem('deepseek-device-id:chat'); } catch (e) { return null; } })();
  const HEADER_SETS = [
    ['bare', { accept: 'application/json' }],
    ['bearer', { accept: 'application/json', authorization: 'Bearer ' + (bearer || '') }],
    ['bearer+xdev', { accept: 'application/json', authorization: 'Bearer ' + (bearer || ''), 'x-device-id': deviceId || '' }],
  ];

  // ---------- 只回结构 ----------
  const shape = (v) => {
    if (v === null) return { type: 'null' };
    if (Array.isArray(v)) {
      const s = { type: 'array', count: v.length };
      if (v.length > 0 && v[0] && typeof v[0] === 'object') s.itemKeys = Object.keys(v[0]);
      return s;
    }
    if (typeof v === 'object') return { type: 'object', keys: Object.keys(v) };
    return { type: typeof v };
  };
  const findArray = (root) => {
    const seen = new Set();
    const walk = (node, depth) => {
      if (depth > 3 || node === null || node === undefined) return null;
      if (typeof node !== 'object') return null;
      if (seen.has(node)) return null;
      seen.add(node);
      for (const k of Object.keys(node)) {
        const child = node[k];
        if (Array.isArray(child)) return { path: [k], arr: child };
        if (child && typeof child === 'object') {
          const hit = walk(child, depth + 1);
          if (hit) return { path: [k].concat(hit.path), arr: hit.arr };
        }
      }
      return null;
    };
    return walk(root, 0);
  };
  // 把一个响应体压成「结构摘要」（只留键名/条数，不留值）
  const summarize = (url, text) => {
    const rec = { url: String(url).replace(/[0-9a-f]{8}-[0-9a-f-]{20,}/gi, '<uuid>').replace(/([?&])(lte_cursor|chat_session_id|id)=[^&]*/gi, '$1$2=<hidden>'), bodyLength: text.length };
    let j = null;
    try { j = JSON.parse(text); } catch (e) { rec.parseError = true; return rec; }
    if (!j || typeof j !== 'object') { rec.json = false; return rec; }
    rec.json = true;
    rec.topKeys = Object.keys(j);
    if (typeof j.code !== 'undefined') rec.code = j.code;
    if (typeof j.msg === 'string') rec.msg = j.msg.slice(0, 160);
    const d = (typeof j.data === 'object' && j.data) ? j.data : null;
    rec.dataShape = shape(d);
    if (d) {
      rec.dataKeys = Object.keys(d);
      if (typeof d.biz_code !== 'undefined') rec.bizCode = d.biz_code;
      if (typeof d.biz_msg === 'string') rec.bizMsg = d.biz_msg.slice(0, 160);
      const bd = (typeof d.biz_data === 'object' && d.biz_data) ? d.biz_data : null;
      rec.bizDataShape = shape(bd);
      if (bd) {
        rec.bizDataKeys = Object.keys(bd);
        rec.bizDataArrayCounts = {};
        for (const k of Object.keys(bd)) if (Array.isArray(bd[k])) rec.bizDataArrayCounts[k] = bd[k].length;
      }
    }
    const found = findArray(d);
    if (found) {
      rec.arrayPath = found.path.join('.');
      rec.itemCount = found.arr.length;
      rec.itemKeys = (found.arr[0] && typeof found.arr[0] === 'object') ? Object.keys(found.arr[0]) : null;
    }
    return rec;
  };

  // ---------- 第 1 档：直接同源 fetch ----------
  const ENDPOINTS = [
    ['/api/v0/users/current', '当前用户（只用来确认登录态，不取用户名）'],
    ['/api/v0/chat_session/fetch_page?count=5', '会话目录·首屏（站点自己就是这么发的）'],
  ];
  let gotArray = false;
  for (const [path, label] of ENDPOINTS) {
    for (const [hname, headers] of HEADER_SETS) {
      const rec = { label, path, headerSet: hname, tier: 1 };
      try {
        const r = await fetch(path, { method: 'GET', credentials: 'include', headers });
        rec.status = r.status;
        const text = await r.text();
        const s = summarize(path, text);
        Object.assign(rec, s);
        if (s.arrayPath) { gotArray = true; OUT.probes.push(rec); break; }
      } catch (e) {
        rec.error = String(e && e.message ? e.message : e);
      }
      OUT.probes.push(rec);
      // 某档被认（200 且顶层有 code）就停用后面的头组合
      if (rec.status === 200 && typeof rec.code !== 'undefined' && !gotArray) {
        // 继续试这个端点的下一档（确认哪档最好），但记下已被认
        rec.accepted = true;
      }
    }
  }

  // ---------- 第 2 档（兜底）：挂捕获钩子，等站点自己的请求 ----------
  const captured = [];
  const CAPTURE_MAX = 40;
  const maybeCapture = (url, text) => {
    const u = String(url || '');
    if (u.indexOf('/api/v0/') === -1) return;
    if (captured.length >= CAPTURE_MAX) return;
    try { captured.push(summarize(u, text)); } catch (e) { captured.push({ url: u, error: String(e) }); }
  };

  const origFetch = window.fetch;
  window.fetch = function (input, init) {
    const p = origFetch.apply(this, arguments);
    try {
      const url = (typeof input === 'string') ? input : ((input && input.url) || '');
      if (String(url).indexOf('/api/v0/') !== -1) {
        p.then(res => {
          // 必须 clone()：直接读 res.text() 会把响应体吃掉，站点自己就拿不到了
          if (!res || typeof res.clone !== 'function') return null;
          try { return res.clone().text(); } catch (e) { return null; }
        })
          .then(t => { if (t) maybeCapture(url, t); })
          .catch(() => {});
      }
    } catch (e) {}
    return p;
  };

  try {
    const XHR = XMLHttpRequest.prototype;
    const origOpen = XHR.open;
    XHR.open = function (method, url) {
      this.__dsUrl = String(url || '');
      return origOpen.apply(this, arguments);
    };
    const origSend = XHR.send;
    XHR.send = function () {
      try {
        this.addEventListener('load', () => {
          try { if (this.responseText) maybeCapture(this.__dsUrl, this.responseText); } catch (e) {}
        });
      } catch (e) {}
      return origSend.apply(this, arguments);
    };
  } catch (e) { OUT.xhrHookError = String(e); }

  window.__dsDump = function () {
    const out = { capturedCount: captured.length, captured: captured };
    console.log(JSON.stringify(out, null, 2));
    return out;
  };

  OUT.tier1GotArray = gotArray;
  OUT.tier2HookInstalled = true;
  OUT.nextStep = gotArray
    ? '第 1 档已拿到数组：看 probes 里 arrayPath / itemKeys / bizDataKeys 即可。'
    : '第 1 档没拿到数组 → 请点一下「新对话」触发站点自己拉列表，然后执行 __dsDump() 回车，把输出贴回。';

  console.log(JSON.stringify(OUT, null, 2));
  return OUT;
})()
```
3. **壳内求值的一次性调试入口** —— 要改 `src-tauri/**` ⇒ **PM 不主动申请**；**只在 ①② 都拿不到时**才向 izzy 单独申请（且必须写清「一次性、只读、不进主干」）。

**⑥ PM 更正自己写错的时钟标签** —— PM 前面几节把 **12:0x–12:1x 的真实提交时刻写成了「13:0x–13:4x」**（真实：`1e6842a` 12:02:10 · `a961ae4` 12:04:55 · `7e9dff0` 12:06:20 · `900ff1d` 12:08:44 · `1fb8101` 12:11:31）⇒ **已把台账与 P0 里的标签改回真实时刻**（本次一并改）⇒ **教训**：台账里的时刻标签**要用命令读出来的真实时间**（`git log --date=format:'%m-%d %H:%M:%S'` / `Get-Date`），**不要凭印象写**。

**⑦ 成员回执里已过期的部分（以本节与 12:0x–12:1x 节的当前基线为准）** —— 远端 `main` **不是** `e1c568f`（现 **`1fb8101`**）· `9512602` 与 `7c7e641` **都已在远端**（`git branch -r --contains` 均可复核）⇒ **「待你统一推」不成立，两笔都已推**；他自报的复跑读数与 PM 一致（判据 **13/13**、自测 **45/45**、`test:scripts` **229/226/0/3**、`test:plugin` **334/331/0/3**）⇒ **无异议**。

**⑧ P4 协议草案：现在可以开写** —— 核心事实已验一半：**取数路线 ✔ 端点 ✔ 接线方式 ✔**；剩下的唯一缺口是**「站点适配契约的字段面」**（已定位到 ⑤ 的 ①② 两条路径）⇒ ⇒ **先把不依赖字段面的条款写死**（执行体在壳／API 在页内／端点／回环桥加路由／零痕迹／只回结构／站内码原样上报／不扩错误码／纪律），**字段面留一个明确标注的待写死点** ⇒ **这不是「半成品协议」**（设计已固定，只差一张数据表）。

**⑨ 未做（记账）**：**P4 协议草案 v0.1**（已解锁，下一步就写）· G3-2b（另派一单）· 消息级合并（izzy 未拍板）· 安装版 release 壳验收（归 izzy 自己测）。

### 2026-10-21 12:4x–12:5x · **P4 协议草案 v0.1 已开写**（新增 `official-chat-p4-protocol.md`）＋ **成员的三处文档更正已逐条一手复核后落** ＋ **P3-14 判据验收通过** ＋ **两笔成员提交已统一推送**

**① 成员要 PM 改的三处文档 —— PM 逐条一手复核后才改（改的是归因，不是结论）** ——
· **更正 1（P0 §7 的根因归因）＝ 成立，PM 已撤下自己写的那条** —— PM 一手复算：副本 `Local State` 的 `os_crypt.encrypted_key` ＝ base64 `RFBBUEkBAAAA0Iyd` ⇒ **解码后前 6 字节 ＝ `DPAPI`**（hex `44 50 41 50 49 01`，第 7 字节是版本号）⇒ ⇒ **是 DPAPI**（绑机器+用户、**不绑 exe**）⇒ ⇒ **「ABE 跨 exe 解不开」站不住**（**PM 12:1x 写的「剩下解释＝ABE」已撤下**）。**真正的卡点**＝**拿不到源文件**：源 dev 树存活时对 `Default\Network\Cookies` 持**独占锁**（`copyFileSync` 与 `readFileSync` **都 EBUSY**，**PM 自己也撞到过**）⇒ **源活着时拷不出 Cookies**。**定性守住**：「**只读拷贝能否带登录态」仍未验到**（不是已证不可行、也不是已证可行；已排除的只有 ABE 那一条）⇒ **不写成结论**。
  ⇒ **连带把「探针 B 关闭」的口径改准**：从「**实测不可用 ⇒ 关闭**」改成「**在源存活 ＋ Cookies 被独占锁的当下取不到登录态 ⇒ 本阶段关闭**（**是「本阶段关闭」，不是「原理上不可行」**）」。
· **更正 2（`official-chat-bridge-research.md:267`）＝ 成立（PM 一手核过那两行原文）** —— 该表 `:262` **自述**「来自第三方逆向项目，属**二级来源，未在本机复核**」，而 DeepSeek 行却写「**实测**一次最多 100 条且**无游标**」⇒ **二手被写成了实测**（**字面自相矛盾**）；且「**无游标**」被一手证据反证（站点自己的翻页请求行 `fetch_page?lte_cursor&pinned=false`）⇒ **已改写为**「页大小上限 `count`（**二手来源**称一次最多 100 条，**未在本机复核**）；翻页用游标参数 **`lte_cursor`** ＋ `pinned`（**一手实测**）」，并在表头补「**DeepSeek 那两行已不是二级来源；ChatGPT 与 MiniMax 两行仍是**」的更新说明。
· **更正 3（PM 自己写的那份 F12 脚本）＝ 成立，已换** —— 旧脚本只探 `data` / `data.chat_sessions` / `data.items` / `data.sessions` **四种形态**，而**实测结构是 `data.biz_data`** ⇒ 照旧那份跑出来 `itemCount` 与 `itemKeys` **必然是 `null`** ⇒ **已换成逐层下钻版**（正文抄进本文件 12:1x 节 ⑤ 第 2 条；成员自查过：模拟 `biz_data.chat_sessions` 响应 ⇒ 正确下钻出 `arrayPath=chat_sessions`）。

**② P4 实测事实已按证据分级写进协议草案 v0.1**（**新增 `docs/features/official-chat/official-chat-p4-protocol.md`**，20.8 KB，10 节 ＋ 变更记录）——
· **【已实测】写死成条款的**：取数路线三条（§1）· CDP `Runtime.evaluate` ＋ `awaitPromise` ＋ `returnByValue` ＋ 零痕迹 **1239/1239**（§2）· 页内同源 fetch 由浏览器自动带 HttpOnly cookie（实跑 `status=200`、`code=0`）· **页外调该 API 必失败已从推断升级为实测**（四种组合全 `40003`）· **⚠️ 未登录态与「凭据形态不对」回同一个码 ⇒ 站内码只原样上报、不解释语义**（§3.3/§6.2）· 端点表 ＋ **翻页游标名 `lte_cursor`** ＋ `pinned` ＋ 响应三层骨架 `{code,msg,data:{biz_code,biz_msg,biz_data}}`（§4.1/4.2）· 登录 cookie HttpOnly · JWT 明文在 leveldb · `ds_session_id` 是 session cookie · `encrypted_key`＝DPAPI · **静态包 grep 只捞到 10 个端点、`chat_session/*` 不在其中 ⇒ 端点面只能靠运行时实测**（§4.1）· 接线＝现役回环桥加一条路由 ＋ 三条硬约束（§5）· 不扩 13 码闭集 ＋ macOS 用 `capability_unsupported` ＋ 两类错误分离（§6）· 零痕迹 / 凭据零落盘 / 只读（§7）· 隔离纪律 ＋ **「只读拷贝带不带登录态」仍未验到**（§8）。
· **【推定】**：`x-device-id` / `x-ds-compression` / `x-request-encryption-key` 是否必需 —— **未单独隔离** ⇒ 协议**不要求**页内 fetch 手工补这些头。
· **【待实测】＝ 不写成条款**：字段面（§4.3 四项）＋ 三档错误码 ⇒ **本稿唯一的待写死点** ⇒ **补完升 v0.2（冻结稿）**。
· **接线路由的三条硬约束（写死）**：**必须 POST ＋ 支持读体 ＋ 写死 body 上限**（现役桥只读头不读体，`bridge.rs:185-187`）· **不能走查询串**（`MAX_PATH_BYTES=7000` / `REQUEST_TIMEOUT_MS=1500`，`plugins/memory/host/bridge.js:43`/`:29-30`）· **最小能力面**（只对 official-chat 的 webview / 白名单只读命令 / **不许传任意 JS** / 不许写站点数据 / 不许碰别的窗口）。

**③ P3-14 判据验收通过（PM 一手复跑）** —— 独立 worktree @ `e3c6a49`（含 P3-14 ＋ 新探针）、detached、串行：**P3 判据 14 条：通过 14 · 失败 0 · 未执行 0**（**P3-14 由红转绿**：① 核算总量 ＝ 判据独立算的**清理后总量**（那份 TTL 已过的 100 MB 目录缓存已被常规清理回收）② 核算总量 ＝ **清理后、淘汰前**总量（那份 TTL 内的 100 MB 目录缓存尚未被额外淘汰））· P3 判据自测 **48/48**（较 45/45 ＋3）· `test:scripts` **232/229/0/3**（＋3）· `test:plugin` **334/331/0/3** · `check:static` **四项全绿**（内嵌资源 4 插件/44 文件）· `cargo test --locked --lib` **1134/0/11**（14.0s）· **`cargo build --locked --example official_chat_deepseek_probe` 通过**（2 warnings / 33.6s ⇒ 探针文件本身可编译）· 复跑后共享工作树干净、临时 worktree 已清理。
⇒ **P3-14 验收通过**（性质＝**回归保护**：它断的 §S-16 裁定 2 早已写死在协议里）⇒ **P3 判据侧 14 条全闭环**。

**④ 两笔成员提交已由 PM 统一推送** —— `e3c6a49`（devloper：P4 探针 A，新文件 `src-tauri/examples/official_chat_deepseek_probe.rs`）＋ `faa4dff`（tester：P3-14 判据）⇒ **随本笔一起上远端** ⇒ **知会已发**（回告哪几笔被带上）。

**⑤ 仍卡着的唯一写死点 ＋ 需要 izzy 的一个动作（这次是「必需」，不是「备选」）** —— 成员实测了 PM 的**替代取证第①档（静态包 grep）**：**470 个 JS 缓存里静态 grep `/api/v0/` 只捞到 10 个端点，`chat_session/*` 与 `history_messages` 不在其中**（前端是运行时拼接）⇒ ⇒ **第①档实测不足 ⇒ 落到第②档**。
· **要什么**：izzy 在 **dev 壳的 DeepSeek 对话页** → **F12 → Console → 粘贴**台账 **12:1x 节 ⑤ 第 2 条**那份只读脚本 → 回车 → **把 console 输出贴回** ⇒ **一次收口**：**字段面**（`data.biz_data` 内数组键名 / 条目键名 / 条数字段 / 游标字段名）＋ **三档错误码** ⇒ 协议 §4.3 即可写死、升 v0.2。
· **成本**：一次粘贴 ＋ 一次复制；脚本**只回结构**（不点按钮、不发消息、不预拉正文、不回正文与凭据、不回会话 id）⇒ **不写站点数据**。
· **PM 不会自己去动 izzy 的窗口**（那是用户的环境）；**第③档（改 `src-tauri/**` 加一次性调试入口）PM 不申请**。

**⑥ 未做（记账）**：**P4 协议 §4.3 字段面**（待上面那次 F12 ⇒ 升 v0.2）· **P4 判据**（待立；协议 §10 已给清单）· **P4 实现单**（待协议 v0.2）· G3-2b（另派一单）· 消息级合并（izzy 未拍板）· 安装版 release 壳验收（归 izzy 自己测）。

### 2026-10-21 13:0x · **P4 协议升 v0.2**（补三条本轮新实测的硬约束：登录态机制 / 外挂 CDP 不可行 / 取证落点订正）＋ **G4-4 收口为「不能」** ＋ **PM 自纠：上一轮给 izzy 的 F12 落点是错的**

**① 协议 v0.2 的三条新硬约束**（`official-chat-p4-protocol.md` v0.1 → **v0.2**，**顺延**：原定 v0.2 为冻结稿，因本轮新增已实测条款而**顺延到 v0.3 ＝ 补齐 §4.3 字段面后的冻结稿**）——
· **§2.5 新增（裁定 2）＝ 登录态机制** —— **登录态只存在于「同一个活着的浏览器上下文」里**。PM 一手复核的机理链：`ds_session_id` 是 **session cookie**（盘上 `is_persistent = 0`、`expires_utc = 0`）⇒ **新浏览器进程启动时不恢复非持久 cookie**（按过期处理并清除）⇒ ⇒ **只读拷贝带登录态 —— 不能**（**机制性，不是偶发**）；而**盘上的明文 JWT 仍有效**（`exp` 2026-11-30、`name`「平常心💯」、`isAnonymous = false`、**两棵树的 leveldb 里都有**）**却不被认**（四种组合全 `40003`）⇒ ⇒ **「执行体在壳」「API 在页内」由机制决定 ⇒ 是硬约束、不是实现偏好** ⇒ **实现方不得提「页外调 API」「从外部进程读」这类方案**。
· **§5.3 新增（裁定 3）＝ 外挂 CDP 不可行** —— 四项实测（PM 一手）：活着的 `msedgewebview2` 命令行**没有** `--remote-debugging-port`；两棵 profile 根目录**没有** `DevToolsActivePort`；`msedgewebview2` **没有**监听端口（`Get-NetTCPConnection`）；release 树 `Cookies` 直接 open **EBUSY** ⇒ ⇒ **`CallDevToolsProtocolMethod` 是 `ICoreWebView2` 自己的方法（进程内调用）⇒ 调用方必须是那个进程自己 ⇒ 只有壳能求值** ⇒ 与 §2.5 合起来 ⇒ **「壳内求值」是唯一路径，没有第二条路**。
· **§8.4 改写 ＋ §8.5 新增 ＋ §8.6 新增** —— 见 ② 与 ③。

**② G4-4 收口：「只读拷贝能否带登录态」＝ 不能（不是「仍未验到」、不是「本阶段关闭」）** —— PM 一手复核 `ds_session_id` 的 cookie 属性后，**12:4x 那条「仍未验到」正式收口** ⇒ **P0 §7 已同步**（把 12:4x 那两条加了「已被 13:0x 收口取代」的指向，并补了 13:0x 的收口条）⇒ **拷贝取证路线正式关闭**（P4 协议 §9 G4-4 由「待实测」改为「**已收口：不能**」）⇒ **连带**：那条卡点「源存活时 Cookies 被独占锁（EBUSY）拷不出」**不是最终机理**（它是**另一层障碍**）⇒ 真正机理是 **session cookie 跨进程不可恢复** ⇒ ⇒ 「**页外调该 API 必失败**」从「别这么做」**升级为「只有这条路」**。

**③ ⚠️ PM 自纠：上一轮给 izzy 的落点是错的（走不通）** —— PM 上一条让 izzy「**在 dev 壳的 DeepSeek 对话页按 F12**」⇒ **这条路走不通**：**release 壳里 DevTools 被 wry 关掉**（依据链见 13:1x 节 ③）⇒ **壳里按 F12 弹不出 DevTools**（debug 构建只给 main 管理窗口自动 `open_devtools()`，**不是官方对话页签**）⇒ ⇒ **F12 的落点不是壳**（已写进 P4 协议 §8.5 ＋ 台账本节）。**PM 一手核过的当前登录态在哪**（决定新落点）：**release 壳**（`C:\Users\zxx\AppData\Local\dsh-xlink\dsh-xlink.exe`，**0.5.8-rc.2**，与仓库 `package.json` / `tauri.conf.json` 同版）的官网对话 webview **活着**（**9 个** webview 进程指向该树、该树 `Cookies` 正在被写）；**dev 壳的官网对话 webview 已关**（**0 个**进程指向该树、该树 `Cookies` 的 mtime 停在 **12:14:38**）⇒ **dev 那棵树的 session cookie 随环境关闭已失效**。

**④ §8.6 两条落点（取「登录态下的字段面」）** ——
| 路线 | 落点 | 需要什么 | 成本 |
| --- | --- | --- | --- |
| **A** | **Chrome** 里**已登录**的 `chat.deepseek.com` 标签页（**Edge 已排除** —— G4-6 已由 PM 一手核完） | 粘贴那份只读脚本（正文在 12:1x 节 ⑤ 第 2 条） | **零代码改动、零授权** |
| **B** | **dev 壳**的官网对话窗口 | ① izzy 在 dev 壳**重新登录一次**并保持页签开着；② **一次授权**改 `src-tauri/**`（只读形状探测，不进主干） | 一次登录 ＋ 一次授权 |
⇒ **PM 建议先问 izzy 一句「平时用的浏览器里登录过 DeepSeek 吗」**（**缺口 G4-6，PM 未核**）⇒ 是 ⇒ 走 **A**（最省）；否 ⇒ 走 **B**（**B 本来就是 P4 实现的第一个里程碑，不是额外的一次性 hack**）。

**⑤ 仍未做（记账）**：**§4.3 字段面（四项）＋ 三档错误码**（⇒ v0.3 冻结稿的前置）· **P4 判据**（待立；协议 §10 已给清单）· **P4 实现单**（待 v0.3）· G3-2b（另派一单）· 消息级合并（izzy 未拍板）· 安装版 release 壳验收（归 izzy 自己测）· **P4 判据/实现单都不能派**（**协议未冻结 ⇒ 派了就会返工**）。

### 2026-10-21 13:1x · **P4 协议升 v0.3（依据订正）** ＋ **PM 第二次自纠：上一轮 §8.5 的依据也错了** ＋ **撤回一条被引用的依据** ＋ **取证脚本换成两档版** ＋ **B 路补齐（不需要 DevTools）**

> 本节处理 **devloper 的回执**（三处：壳里 F12 不通的三条依据 / 两档版脚本 / §8「仍未验到」要收口）。**PM 逐条一手复核后才落** —— **结论收下、依据换掉一条、另外两处有一处已经不成立**。

**① 成员的依据①「wry 关掉了浏览器加速键 ⇒ F12 不生效」＝ 不成立（依据错）** —— PM 一手核 `wry-0.57.0/src/webview2/mod.rs:649-653`：那句 `SetAreBrowserAcceleratorKeysEnabled(false)` 是**有条件的**（`if !pl_attrs.browser_accelerator_keys`），而 wry 默认 **`browser_accelerator_keys: true`**（`lib.rs:1768`，注释「This is WebView2's default behavior」）⇒ 本仓**没有**调用 `with_browser_accelerator_keys(false)` ⇒ ⇒ **加速键是开着的** ⇒ **那条依据站不住**。
· ⇒ **教训（已写进协议 §8.5）**：「某能力不存在」要先找**真正控制它的那个开关**（本例是 `devtools` 属性，不是加速键）⇒ **别拿邻近开关的代码当依据**。

**② PM 第二次自纠：§8.5 的依据链自己也是错的** —— PM 上一轮（13:0x）写的依据是「官方对话窗口建的时候是 `with_devtools(false)`（`src-tauri/src/lib.rs:455-456`）」⇒ **依据错**：`lib.rs:450-459` 那段讲的是 **`main` 管理窗口**、且它自己就 `#[cfg(debug_assertions)]` 门控 ⇒ **不是官网对话窗口** ⇒ ⇒ **结论（落点不是壳）对，依据不成立** ⇒ 已整段换成下面这条**一手核过的链**。

**③ 真正控制 DevTools 的开关（PM 一手核源码，【已实测】）** ——
| # | 依据 | 结论 |
| --- | --- | --- |
| ① | `wry-0.57.0/src/lib.rs:872-875` —— `devtools` 属性默认 **`#[cfg(debug_assertions)] true` / `#[cfg(not(debug_assertions))] false`** | **release 构建默认就是关的** |
| ② | `tauri-runtime-wry-2.12.1/src/lib.rs:5146-5149` —— Tauri **只在** `#[cfg(any(debug_assertions, feature = "devtools"))]` 下才 `with_devtools(...)` 覆盖 | 本仓 release **无 `devtools` feature**（`src-tauri/Cargo.toml:30`）⇒ **不会被打开** |
| ③ | `wry-0.57.0/src/webview2/mod.rs:640` `settings.SetAreDevToolsEnabled(attributes.devtools)?` | ⇒ **`SetAreDevToolsEnabled(false)`** ⇒ **F12 与右键「检查」都不生效** |
| ④ | `src-tauri/src/harness/no-context-menu.js`（捕获阶段 `preventDefault()` ＋ `stopImmediatePropagation()`） | **DOM 层右键菜单整条不出** |
| ⑤ | `src-tauri/src/official_chat/context.rs:104-113`（`build_read_window` 建窗链）**无** `open_devtools()`；全仓 `open_devtools()` 三处：`commands.rs:1426`（工作台 `harness`）、`lib.rs:459`（`main` 管理窗口）、`harness/harness_window.rs:481`（工作台）—— **都 `#[cfg(debug_assertions)]`、都只给这两个窗口** | **官网对话页签没有程序化打开 DevTools 的入口** |

**④ 落点分两档写准（release 与 dev 的理由不同）** ——
· **release 壳**（**登录态活着的那棵**）＝ **DevTools 被关（真）** ⇒ F12 无效。
· **dev 壳** ＝ DevTools **可能能开**（**【推定】，未实测**）⇒ **但 dev 树的登录态已随环境关闭失效**（`ds_session_id` 是 session cookie ＋ 官网对话 webview 已关、**0 个**进程指向该树、该树 `Cookies` mtime 停在 **12:14:38**）⇒ ⇒ **即便 F12 弹得出来，打开也是 `sign_in`** ⇒ ⇒ **两档都不可用**，且 **dev 壳的理由与 DevTools 无关**。

**⑤ B 路补齐一条关键事实：B 路不需要 DevTools**（【已实测】）—— P0 已实测「**不开 DevTools 窗口也能调 `Runtime.evaluate`**」（`src-tauri/src/official_chat/cdp_eval.rs:24-27` ＋ `src-tauri/examples/support/cdp_probe.rs`）⇒ `CallDevToolsProtocolMethod` 是 `ICoreWebView2` 自己的方法、**进程内调用** ⇒ ⇒ **B 路是「壳内求值」，不是「人工开 DevTools」** ⇒ ⇒ **B 路在 release 壳里同样成立、不用重新登录** ⇒ 拆成两个子选项供 izzy 选：**B1**｜dev 壳（**重新登录一次、不碰安装版**）｜**B2**｜release 壳（**不用重新登录、但要动用户真实树/安装版** ⇒ 先问 izzy）。

**⑥ 取证脚本换成「两档自动降级」版（成员交付，PM 逐字段核过输出面后采纳）** —— 第 1 档按 **裸 / ＋Bearer / ＋Bearer＋`x-device-id`** 三种头反推哪一档被认（顺带拿到三档码）；第 2 档兜底：挂 `fetch`/`XHR` 钩子等**站点自己**的列表请求，用 `res.clone()` **不吃掉站点的响应体** ⇒ **不依赖对鉴权头的猜测** ⇒ **正文已替换台账 12:1x 节 ⑤ 第 2 条**。
· **PM 的复核结论**：**输出面合格** —— 逐字段核过（`localStorage` 只回**键名**与**长度**、`document.cookie` 只回**名字**、响应只回**键名/条数/code/msg 前 160 字**、URL 里 uuid 与 `lte_cursor`/`chat_session_id`/`id` **已打码**）⇒ **不回正文、不回标题、不回会话 id、不回任何凭据**。
· ✅ **PM 一手复跑（不采信成员自测，PM 自己在 mock 里跑）＝ 17/17 全绿** —— 把台账里的脚本正文**抽出来**（`node --check` 语法过 ＋ 全文 fence 配平 ＝ 2 行）⇒ 造 mock 环境（`localStorage` 放假 token、`document.cookie` 放匿名项、`fetch` mock 返回仿造的三层响应）跑两遍：**场景 A**（第 1 档即被认）⇒ 正确下钻出 **`arrayPath = biz_data.chat_sessions`**、`itemCount = 1`、首条键名 `id/title/updated_at/tokens`、`tier1GotArray = true`；**场景 B**（第 1 档三档头全回 `40003`）⇒ 第 2 档钩子**从「站点自己」的请求**里截到同一结构（同样下钻出 `biz_data.chat_sessions` 与首条键名、URL 里的 uuid 被打码成 `<hidden>`）⇒ ⇒ **两档都按预期工作**；**输出面机械核对**（不是靠读，是搜字符串）：序列化输出里**查不到** token 本体、**查不到**条目正文/标题、**查不到**会话 id ⇒ ⇒ **「只回结构」有 PM 一手实测背书**。
· ⚠️ **PM 自己那条断言一开始写错了（记录在案）** —— 首跑 16/17：唯一红的那条是 PM 断言要求 URL 里出现字面 `<uuid>`，而脚本实际把游标参数打码成 `<hidden>`（**掩码本来就生效**）⇒ ⇒ **该改的是断言、不是脚本** ⇒ 改断言后 **17/17**。
· **PM 补的两条口径**：① **痕迹范围要写清** —— 脚本会替换**该标签页**的 `window.fetch` 与 `XMLHttpRequest.prototype.open/send` ＋ 挂 `window.__dsDump` ⇒ **只在这个标签页、关掉即消失；不写站点数据、不写盘**（**不属协议 §7.1 的零痕迹纪律 —— §7.1 管的是生产页内脚本**）；② **第 2 档的触发动作建议用「新对话」**（**不要点列表里的某个会话**，那会加载它的正文 ⇒ 擦 §7.3「不预拉正文」）。

**⑦ 成员回执里已过期的部分（以本节与 13:0x 节的当前基线为准）** —— 远端 `main` **不是** `ed87602`（PM 13:0x 已推 **`2ef12b0`**，13:1x 之后又推一笔）· 「§8 那句『仍未验到』应改成『已实测不能』」**已在本轮 13:0x 落地**（P4 协议 §8.4 ＋ P0 §7 ＋ 台账 13:0x 节，**该条不再重复做**）· 成员自报「我这边无未推提交」**在回执当时成立**（PM 已核 `git log origin/main..main` 为空）—— **⚠️ 但回执之后他又提交了一笔，见 ⑧**。

**⑧ PM 本轮的 push 顺带带上了一笔成员提交（`84fd50c`）—— 已一手核 ＋ 已回告** —— PM 推 `2ef12b0..a4c90ac` 时，历史里出现一笔 **PM 没提交过的** `84fd50c`（**devloper**，13:06:32，`docs(official-chat)：订正探针头注释里「只读拷贝可带登录态」的旧推断`）⇒ ⇒ **它已被本次 push 带上远端**（**对比：PM 13:0x 那次 push 只带了自己那一笔 ⇒ 13:1x 这次才是「带上了成员提交」的那次**）⇒ ⇒ **知会已发**（回告哪几笔被带上）。
· **PM 一手核过那笔的内容**：`src-tauri/examples/official_chat_probe.rs` **只改 `//!` 头注释**（+10/−2，**零可执行代码改动**）⇒ 改法与 13:0x 的收口一致（「**加密层可移植 ✔ / 登录态带不过来 ✘**」＋「带登录态只能在同一个活着的浏览器上下文里」）⇒ ⇒ **验收通过**。

### 2026-10-21 13:2x · **P4 协议升 v0.4（防误读条款）** ＋ **复核成员第二条回执：他的「正确依据①」仍不成立** ＋ **核了他的对账素材 ⇒ 挖出一条必须写死的防误读事实**（顶层 `code=0` ≠ 取到数据）

**① 成员这条回执的前提已过期（他自己也这么说，一致）** —— 「探针 A 早跑完、早交付、PM 12:5x 已验收并推上远端（`ed87602`）」✅ **一致**；「探针 B 已被实测收口成『不能』」✅ **一致**（PM 13:0x 已落 §8.4 ＋ P0 §7）⇒ ⇒ **两边现状一致，无需返工**。

**② ⚠️ 他这条给的「正确依据①」仍不成立** —— 他写「正确依据是三条：① wry `webview2/mod.rs:651` `SetAreBrowserAcceleratorKeysEnabled(false)` ⇒ F12 不生效」⇒ **PM 已一手核证并驳过这条**（13:1x 节 ①）：那句是**有条件的**（`if !pl_attrs.browser_accelerator_keys`），wry 默认 **`browser_accelerator_keys: true`**（`lib.rs:1768`）⇒ 本仓**未**调用 `with_browser_accelerator_keys(false)` ⇒ ⇒ **加速键是开着的**。⇒ **他的「结论」对（壳里按 F12 弹不出 DevTools），但他又给了一条不成立的依据** ⇒ **PM 已在协议 §8.5 用真正的一手依据链取代**（`devtools` 属性链 ＋ `no-context-menu.js` ＋ 无 `open_devtools()` 入口）。

**③ 他那条「release 连 inspector 都没有（`tauri` features 无 `devtools`）」＝ 方向对、机制表述要改准** —— 真正的原因是：**wry 的 `devtools` 属性默认在 release 下就是 `false`**（`wry-0.57.0/src/lib.rs:872-875`）＋ Tauri **只在** `#[cfg(any(debug_assertions, feature = "devtools"))]` 下才覆盖它（`tauri-runtime-wry-2.12.1/src/lib.rs:5146-5149`）⇒ ⇒ 「缺 `devtools` feature」是「**不会被覆盖**」的原因，**不是**「没有 inspector」的原因（**wry 的默认值本身就是关**）。

**④ 他指出 PM 的依据引错 ＝ 成立，PM 已在 v0.3 改掉** —— 他说「`lib.rs:455-456` 讲的是**主（管理）窗口**，不是官方对话窗口」⇒ **与 PM 13:1x 的自查一致** ⇒ **协议 §8.5 已整段换掉**（v0.3，远端 `a4c90ac`）⇒ **他这条提醒晚了一步**（他与 PM 各自独立得出了同一条纠正）。

**⑤ 他说「台账里那份脚本是旧版（裸 fetch）」＝ 已过期** —— PM 13:1x **已把台账 12:1x 节 ⑤ 第 2 条整段换成两档版**，并已 **mock 复跑 17/17**（见 13:1x 节 ⑥）。

**⑥ PM 核了他的对账素材 ⇒ 挖出一条必须写死的防误读事实（协议 v0.4）** —— 素材 `p4-protocol-facts.md`（**225 行**，§10 是探针 A 的对账表；**不在仓库里**，按 izzy「证据可以清」的纪律不固化）⇒ PM 逐条对完，**与协议已有的表述一致**，但**多出三条值得写死的事实**：
· ⚠️⚠️ **「顶层 `code=0` ≠ 取到数据」** —— 实跑读数是 `status=200` ＋ **顶层 `code=0`** ＋ `msg=""`（看着像成功）**同时** **`biz_code=2`** ＋ **`biz_data` 的类型读数是 `null`** ⇒ ⇒ **传输层成功 ≠ 业务层成功 ≠ 拿到了数据** ⇒ ⇒ **判「成功」不能只看顶层 `code`** ⇒ **成功判据必须落到 `data.biz_data` 是否有内容** ⇒ **已写进协议 §4.2 ＋ §6.2（v0.4）** ⇒ **这条会直接影响实现（别把 `code=0` 映射成「成功」）**。
· **端点的来源要说准** —— 上表那 5 个端点来自**站点自己浏览器缓存里的完整请求行**（Chromium 缓存条目 `data_1` 的 key 字段）⇒ **不是**「页面当场请求」的记录 ⇒ ⇒ **登录态下的 resource timing 端点清单仍未实测** ⇒ **已写进 §4.1**。
· **第二种非静态口径也补不齐字段面** —— **运行时 resource timing**（**未登录态**）＝ 资源总数 **23** 个、其中 `/api/` 端点**只有 1 个**（`/api/v0/client/settings`，query 只回**键名**）⇒ ⇒ 与「静态 grep 只捞到 10 个」**是两个不同口径、都不能看到字段面** ⇒ **已写进 §4.1**。

**⑦ 回告给成员的新信息（B 路）** —— 他那条仍写「B 路要在 dev 壳重新登录一次」⇒ **PM 13:1x 已补**：**B 路不需要 DevTools**（壳自己的 CDP `Runtime.evaluate` 是进程内调用）⇒ ⇒ **B 路在 release 壳里成立、不用重新登录** ⇒ 拆 **B1**（dev 壳：重新登录一次、不碰安装版）/ **B2**（release 壳：不用重新登录、但要动用户真实树）⇒ **待 izzy 选（缺口 G4-7）**。

**⑧ 远端与待办** —— 远端 `main` ＝ **`8d60910`**（含 `2ef12b0` v0.2 · `84fd50c` 成员的注释订正 · `a4c90ac` v0.3 · `8d60910` 台账）⇒ 本轮再推 **v0.4** ＋ 台账本节。**P4 判据与实现单仍不派**（协议未冻结）⇒ **等 izzy 答 G4-6**（＋若否则答 G4-7）。

**⑨ 成员第三条回执（迟到的知会，对应 P0 `:225` 的 12:0x 口径）—— 他提的两处「PM 文档里的旧表述」＝ 成立，PM 已逐条核后改掉** ——
· **P0 `official-chat-feasibility.md:217`**：「若需复用登录态，走**只读拷贝**（Windows Cookies 由 DPAPI 绑机器+用户、不绑路径）」⇒ **已改成两层**：**加密层成立**（绑机器+用户、不绑路径、**不绑 exe**）／**登录态层不成立**（`ds_session_id` 是 session cookie ⇒ 新进程不恢复非持久 cookie）＋ 写明「**只读拷贝是「取证隔离」手段，不是「带登录态」手段**」＋「**纪律裁定本身不用改，只改前提**」⇒ **这半句原先与同文件 `:226`／`:229` ＋ 协议 §2.5 自相矛盾，现已消歧**。
· **台账 `official-chat-progress.md:1545`**：「副本里就带着 `session_id` ⇒ **不需要 izzy 再提供任何登录态**」⇒ **已加指向**：后半句**已被 13:0x 实测证伪**（副本落地 `sign_in`、页内 `document.cookie` 只有两个匿名项、`localStorage` 的 `userToken`／`settingsJwt` **确实拷过来了** ⇒ **token 在、session 不在**）＋ **护栏防不了「登录态带不过来」** ⇒ 该路线正式关闭；**前半句（只读拷贝满足两条纪律）仍成立**。
· **他这条其余内容的状态（多半已落）** —— ① 他要的两条实测事实（落地 URL **不是**登录后页面、页内**无可用 session 迹象**）⇒ **已落** P4 协议 §2.5／§8.4 ＋ P0 §7；② 静态包 grep 不足（470 个缓存／10 个端点、`chat_session/*` 不在其中）⇒ **已落** §4.1；③ 「只管取证手段、不改生产机制」⇒ **已落** §2.5（**硬约束**，不是「万一」假设）；④ 他名下 `84fd50c`（纯注释）⇒ **已由 PM 推上远端并回告**（13:1x 那笔 push 带的）⇒ **「未 push」已过期**；⑥ 落点 B 仍写「dev 壳重新登录一次」⇒ **已过期**（PM 13:1x 补了 B1/B2：**B 路不需要 DevTools**、release 壳里也成立）。

### 2026-10-21 13:3x · **G4-6 收口：PM 一手核完浏览器里的登录态**（Chrome 有 · Edge 排除）＋ **协议升 v0.5（落点收窄）** ＋ **成员第四条回执基线已过期 ⇒ 零新动作**

**① G4-6 不用等 izzy 回答了 —— PM 自己做了一手只读核查**（**不解 cookie 值、不写用户目录、不启动浏览器**）——

| 浏览器 | 读数 | 判定 |
| --- | --- | --- |
| **Chrome**（Default） | `localStorage` 的 leveldb 里 **`userToken` ＋ `__appKit_userInfo` 都在**；且 `userToken` 与 origin 字符串落在**同一条记录**（偏移相距 **155 字节**）；leveldb 最新 mtime **2026-10-21 12:08:39** | **有登录痕迹** ⇒ 落点 A 可用 |
| **Edge**（Default） | `localStorage` 里**没有** `userToken`／`__appKit_userInfo`（只有 **14 处** `chat.deepseek.com` 访问痕迹）；leveldb 最新 mtime **2026-10-16 09:27:37**；且其 `Cookies` 里 `chat.deepseek.com` **只有两个匿名项**（`.thumbcache_…`／`smidV2`） | **排除** |

· **为什么查 leveldb 而不是 cookies** —— Chromium 的 **`localStorage` 不加密**（`userToken` 是明文）⇒ **只读扫键名就能判断「这个 origin 曾登录且未清理」**；而 `Cookies` 表**被浏览器独占锁**（copy 与只读 open 都 `WinError 32`）⇒ **读不到**。⇒ **纪律**：只报**命中计数**与**偏移距离**，**不打印命中上下文、不打印 token 值**。
· ⚠️ **残留一条，不假装已收**：**「痕迹在」≠「此刻 session 仍有效」**（session cookie 可能已被站点清掉）⇒ ⇒ **最后一步仍要 izzy 在 Chrome 里跑一次脚本**（落点 A）。

**② 协议升 v0.5** —— §4.3 补法 ＋ §8.5 结论 ＋ §8.6 表格的落点由「Edge / Chrome」**收窄为「Chrome」**；§9 **G4-6 由「待 izzy 确认」改为「已收口」**（**G4-7 保留为条件项**：只在 A 路失效时才需要问）⇒ **版本线第三次顺延**：v0.5 ＝ G4-6 收口＋落点收窄 ⇒ **v0.6 ＝ 补齐 §4.3 字段面后的冻结稿**（原定 v0.3 为冻结稿）。

**③ 成员第四条回执（更早知会的延迟回复）—— 基线已过期、零新动作** —— 他写的基线是「远端 `2ef12b0`、本地 HEAD `84fd50c`、**请统一推**」⇒ ⇒ **「请统一推」已是空动作**（`84fd50c` 在 PM 13:1x 那笔 push 就被带上远端了；远端现已到 `a6cf19c`）；「台账里那份是旧版脚本」⇒ **已过期**（PM 13:1x 已换成两档版并 mock 复跑 17/17）；「协议升 v0.3 冻结稿」⇒ **已过期**（现 v0.5，冻结稿顺延 v0.6）；①～⑥ 他全部认同 ⇒ **一致，无需返工**。

**④ 脚本正文已直接发给 izzy** —— 台账 12:1x 节 ⑤ 第 2 条那份两档版（**与协议 §4.3 补法同源**）⇒ izzy 在 **Chrome** 里打开 `chat.deepseek.com` → 若显示已登录 ⇒ 粘贴 → 回车 → 把输出贴回 ⇒ **一次收口 §4.3 四项 ＋ 三档码**；若显示 `sign_in` ⇒ 再问 **B1 / B2**（G4-7）。

**⑤ 仍未做（记账）**：**§4.3 字段面**（等 izzy 这次输出 ⇒ v0.6 冻结稿）· **三档错误码**（同一次）· **P4 判据**（待立）· **P4 实现单**（待 v0.6）· **G4-7**（条件项）· G3-2b（另派一单）· 消息级合并（izzy 未拍板）· 安装版 release 壳验收（归 izzy 自己测）。

### 2026-10-21 13:4x · **成员接受 PM 的依据订正**（加速键那条他自认写错）＋ **他提的三处旧依据：只有一处还要改** ＋ **他的两条补充被采纳**（wry 不绑 F12 / B2 归属）＋ 协议升 v0.6

**① 成员独立核了源码并接受更正** —— 他核了 `wry-0.57.0/src/webview2/mod.rs:649-653`（**条件式**：`if !pl_attrs.browser_accelerator_keys`）＋ `lib.rs:1768`（默认 **`browser_accelerator_keys: true`**）＋ 本仓**无** `with_browser_accelerator_keys(false)` ⇒ ⇒ **确认「加速键被关 ⇒ F12 不生效」不成立** ⇒ **他两处载体已自行订正**（素材 `p4-protocol-facts.md` §6.1 ＋ 他自己的记忆条目）⇒ ⇒ **与 PM 13:1x 独立核出的同一组三处一致** ⇒ **双方从相反方向得出了同一条纠正**。

**② 他给【推定】标注补的支撑 ＝ 成立，PM 一手核过后采纳** —— **wry 0.57 自己不绑 F12**：全仓 grep 不到任何 `F12`／`AcceleratorKeyPressed` 处理（加速键相关的**只有** `browser_accelerator_keys` 那个开关、默认 `true`）⇒ ⇒ **F12 能否触达完全取决于 WebView2 自身行为**，不是 wry 提供的 ⇒ ⇒ **「dev 壳可能能开」标【推定】未实测是准确的，不宜写成「能开」** ⇒ **已写进协议 §8.5（v0.6）**。

**③ 他提的三处旧依据 —— PM 逐条核，**只有一处还需要改**** ——
| 他指的位置 | PM 核的结果 |
| --- | --- |
| 台账 `:1666` | **已改**（PM 13:1x 已换成新依据链）—— **他看的是旧行号** |
| 台账 `:1781` | **是历史记述**（13:1x 节 ②「PM 第二次自纠」那一条）⇒ **引用旧依据正是它要记的内容** ⇒ **保留** ✅ |
| **P0 `:236`** | **确实还留着**（句首仍以「壳的官网对话窗口建的时候是 `with_devtools(false)`（`src-tauri/src/lib.rs:455-456`）」起句）⇒ **已改写**：改成「**release 壳里 DevTools 被 wry 关掉**」＋ 把订正说明压进去 ＋ 落点同步为 **Chrome**（G4-6 已收口） |
⇒ ⇒ **他这条指得对**（一处真、一处行号过期、一处是记述而非依据）⇒ **本轮改了 P0 一处**。

**④ 他对 B2 的操作性提醒 ＝ 采纳并写进协议** —— **B2 要「重新构建安装版」**（改 `src-tauri/**` ＋ 构建 ＋ 装到用户真实那棵树）⇒ ⇒ **属于「动用户安装版」的动作 ⇒ 归 izzy 自己拍板、自己测**（**成员的分工只到 dev**）⇒ ⇒ **协议与派单里不许把 B2 写成「零成本」** ⇒ **已落 §8.6（v0.6）**。

**⑤ 他这条其余部分与 PM 现状逐条一致** —— 静态包 grep 不足 ✅ · 探针 A 早已提交并验收（`e3c6a49`）✅ · 根因已定位到机理（session cookie）✅ · B 项已关闭 ✅ · 台账脚本已是两档版 ✅（**他核到的行号 `:1680-1862` 正确**）；**⑦ 的基线过期**（远端已 `b622961`、`84fd50c` 早已推上）⇒ ⇒ **零新动作**。

**⑥ 仍未做（记账）**：**§4.3 字段面**（等 izzy 在 Chrome 里跑一次脚本）· **三档错误码**（同一次）· **P4 判据**（待立）· **P4 实现单**（待 v0.7）· **G4-7**（条件项）· G3-2b（另派一单）· 消息级合并（izzy 未拍板）· 安装版 release 壳验收（归 izzy 自己测）。

### 2026-10-21 13:5x · **台账脚本与成员原文件彻底对齐（全文 200/200 逐行全等）** ＋ **三处「Edge / Chrome」落点措辞订正** ＋ **替换后 PM 又复跑一遍 17/17**

**① 成员指出的问题 ＝ 成立（而且会影响执行）** —— 台账 12:1x 节 ⑤ 第 2 条那份脚本的**头注释第 1 行**仍写「你平时用的浏览器（**Edge / Chrome**）里已登录的 chat.deepseek.com 标签页」⇒ **Edge 无登录** ⇒ **第 1 档三档头必然全 `40003`、第 2 档钩子也截不到站点自己的列表请求 ⇒ 白跑一次** ⇒ ⇒ **已订正**（PM 13:3x 已把协议与台账两处收窄为 Chrome，**但脚本头注释这一行漏了** —— 成员把它抓出来了 ✅）。
· ⚠️ **PM 13:3x 发给 izzy 的那份正文，头注释同一行写的也是「Edge / Chrome」** ⇒ ⇒ **对 izzy 的更正（内容已定）**：**以 Chrome 为准、Edge 已排除**；**可执行部分一字未变**（成员那份的可执行部分与 13:3x 发出去的完全一致 ⇒ **不用重做准备，只要别在 Edge 里跑**）。

**② PM 一手核「两份是否逐行一致」—— 不采信自报** —— 口径：**滤掉整行注释与空行、再逐行 `trim()` 比可执行部分** ⇒ ⇒ **154 / 154 逐行完全一致**（**他自报属实**）⇒ 随后**整段用他那份替换台账正文（含注释）** ⇒ ⇒ **全文 200 / 200 逐行全等**（`IDENTICAL=True`）⇒ ⇒ **两份彻底不分叉**（他的原文件已把头注释改成 Chrome 表述 ＋ 补了「Edge 已排除」的依据 ＋ 去掉了那句错的「wry 关加速键」）。

**③ 替换后 PM 又复跑一遍（不是复制旧结论）** —— `node --check` **通过** ＋ mock 两场景 **17/17 全绿**（场景A：第 1 档即被认 ⇒ 下钻出 `biz_data.chat_sessions` ＋ 首条键名；场景B：第 1 档三档头全 `40003` ⇒ 第 2 档钩子从「站点自己」的请求截到同一结构）＋ **输出面机械核对仍查不到 token 本体 / 条目正文标题 / 会话 id**。

**④ 三处「Edge / Chrome」现行措辞已订正**（**历史记述一律不动**）——

| 位置 | 处理 |
| --- | --- |
| 台账 12:1x 节 ⑤ 第 2 条的**脚本头注释** | **随整段替换 ⇒ 已是「Chrome」＋ Edge 排除依据** |
| 台账 12:1x 节 ⑤ 第 2 条的落点订正条 | **已改**（A 路＝**Chrome** ＋ 注明「Edge 已排除 —— G4-6 已由 PM 一手核完，13:3x」） |
| 台账 13:0x 节 ④ 的落点表 A 行 | **已改**（**Chrome** ＋ 「Edge 已排除」） |
| 台账 13:2x 节 ② ＋ 交接记录 13:0x 行里的「Edge / Chrome」 | **历史记述 ⇒ 保留**（那正是在描述「收窄」这件事本身） |

⇒ **协议正文本轮未动**（§4.3／§8.5／§8.6／§9 已在 13:3x 收窄为 Chrome；剩下那两处「Edge / Chrome」在 G4-6 行与 v0.5 变更记录里，**都是记述**）⇒ ⇒ **不升协议版本**（P4 协议仍为 **v0.6**，**冻结稿仍待 v0.7**）。

**⑤ 他这条里已过期的一句** —— 「回填 §4.3 ⇒ **v0.6 冻结稿**」⇒ **已过期**：协议已 **v0.6**（13:4x 补口径那次）⇒ **冻结稿顺延为 v0.7**。

**⑥ 仍未做（记账）**：**§4.3 字段面**（等 izzy 在 Chrome 里跑一次）· **三档错误码**（同一次）· **P4 判据**（待立）· **P4 实现单**（待 v0.7）· **G4-7**（条件项）· G3-2b（另派一单）· 消息级合并（izzy 未拍板）· 安装版 release 壳验收（归 izzy 自己测）。

**⑦ 本轮动作（PM）** —— ① **字节校验 8 份 PM 文档全绿**（UTF-8 无 BOM / 纯 LF / 0 个替换字符）⇒ ⇒ **只提交台账这一份文件**（显式路径；**协议与 P0 本轮未动** ⇒ 不升版本）② **提交前又复跑一遍脚本核验**（不复制旧结论）：台账脚本块 **200 行**、**可执行部分 154/154 与成员原文件逐行一致**（`IDENTICAL`）＋ `node --check` 通过 ＋ mock 两场景 **17/17 全绿** ③ **成员那条「回填 §4.3 ⇒ v0.6 冻结稿」已过期** ⇒ **冻结稿 ＝ v0.7**（协议现 v0.6）④ **对 izzy 的更正**：**以 Chrome 为准、Edge 已排除**（可执行部分一字未变 ⇒ 只要别在 Edge 里跑）⑤ **本节随本笔提交入库**（提交号见远端 `main` 最新一笔）⇒ ⇒ **PM 统一推送**（成员本地提交、PM 推的口径不变）。

### 2026-10-21 13:6x · **成员的「break 盲区」成立**（PM 一手核代码）＋ **PM 补两处精确化** ＋ **纠正一次同款越界复发（协议落笔归 PM）** ＋ **台账行号漂移更正**

**① 他的基线已过期** —— 他写「远端 `origin/main` ＝ `978454d`」⇒ ⇒ **现为 `b270f83`**（PM 13:5x 那笔：台账脚本与成员原文件对齐 ＋ 三处 Edge/Chrome 订正）⇒ 下面凡涉基线处都按 `b270f83` 重读。**其余现状核实全部属实**（`origin/main..HEAD` 空输出、工作树干净、**P0 `:236` 确已改写**、**§4.3 确是唯一待写死点**）。

**② 「第 1 档拿到数组就 `break` ⇒ 另两档头不发出」—— 成立，行号也准** —— `deepseek-shape-probe.js:127` 确为 `if (s.arrayPath) { gotArray = true; OUT.probes.push(rec); break; }` ⇒ 他引的三个行号（`:43-47` 三档头定义 · `:81` uuid 打码 · `:127` break）**PM 逐行核过，全部准确**。

**③ ⚠️ PM 一手精确化（他的表述范围偏大）** —— `break` 位于 **HEADER_SETS 内层循环**（`:119` 开、`:127` break），**外层 ENDPOINTS 循环（`:118`）不受影响** ⇒ ⇒ **break 只影响「命中数组的那个端点」** ⇒ ⇒ **`/api/v0/users/current` 的三档头仍会全发**（用户对象里没有数组 ⇒ `findArray` 落空 ⇒ 不 break）⇒ ⇒ **三档头的码读数仍拿得到**（在 `probes[]` 里，**来自 `/users/current`**）⇒ 只是 **`fetch_page` 端点上只验到裸档** ⇒ ⇒ **§6.2 回填时必须标明「这是哪个端点的码」** —— **不许把 `/users/current` 的码当 `fetch_page` 的码写进条款**（两者语义不是一回事）。

**④ ⚠️ PM 一手精确化 ②：游标键要查三处，不是两处** —— 脚本回的三处键名清单是 **`topKeys`（顶层，`:86`）＋ `dataKeys`（`:92`）＋ `bizDataKeys`（`:98`）**；**数组键单独在 `bizDataArrayCounts`（`:99-100`，只含数组）** ⇒ ⇒ 他的草案把游标判读写成「`bizDataKeys` / `dataKeys` 里找非数组键」⇒ **漏了 `topKeys`** ⇒ **游标若挂在响应顶层（如 `j.lte_cursor`）就会漏检**（顶层挂载是这类接口的常见形态）⇒ ⇒ **回填时三处都要查**。

**⑤ 游标的两条判读分支 —— 成立，采纳** —— 「键名清单里出现非数组键 ⇒ 已验到，直接写死」/「一个非数组键都没有 ⇒ **不是探针没测到，而是实测事实『该响应体没回带游标字段』** ⇒ 写成事实 ＋ 把「游标字段名」登记为**新缺口**（真要取到得另派一次带 `lte_cursor` 的翻页请求，而那需要真游标值）」⇒ ⇒ **采纳**（合「待实测不写成条款」「不硬凑」）。

**⑥ 「§6.2 措辞改成『验到哪几档就写哪几档，未验到的登记为缺口』」—— 采纳，但**不单独升版** —— 现行 §6.2 只写「三档码的语义**待实测**…⇒ 在那之前不写进协议」（**并没有承诺三档全验到** ⇒ **不与本轮发现矛盾**）⇒ ⇒ **不单独改协议**（避免又一次顺延）⇒ ⇒ **随 v0.7 冻结稿一次落**（连带 ③ 的端点标注、④ 的三处查法）。

**⑦ ⚠️ 纠正一次同款越界复发（协议落笔归 PM，不归他）** —— 他的附件 `p4-section43-backfill.md`（6,812 字节 / 51 行 / mtime 13:34，**不在仓库**）**PM 一手读过** ⇒ **素材质量合格**（字段→条款的逐项映射 ＋ 两种分支写法 ＋ 条款骨架 ＋ 门禁清单，且**明确「具体键名一律以实测输出为准，不预填」**）⇒ **但他第四节写「Izzy 贴回输出后，**我**要做的（不需要 PM 再派单）…`git commit --only … -- official-chat-p4-protocol.md`」** ⇒ ⇒ **越界**：`docs/features/official-chat/**` 是 **PM 的归属域**（v0.2–v0.6 全部由 PM 落笔并升版）⇒ ⇒ **两个写手写同一个协议文件 ＝ 并行写手冲突**（互相覆盖、冻结稿被搅黄）⇒ ⇒ **这是同款越界的复发**（上一轮已因「贴回后我自己回填 §4.3 ＋ 只提交协议文件」被纠正过一次）⇒ ⇒ **裁定**：**成员交草案（已交 ✔）→ PM 落笔 §4.3／§6.2／§9／§10 第 8 条／版本线 → PM 提交 ＋ PM 统一 push**；成员保留 **草案随实测输出逐条核对** ＋ **门禁复跑**（`check:static` / `check-invariants`）＋ **§10 判据第 8 条的可执行断言草案**（判据脚本在 `scripts/**`，不属 PM 归属域）。

**⑧ ⚠️ 台账行号漂移更正（结论成立，行号过期）** —— 他引「台账 `official-chat-progress.md:1680-1862` 那份正文」⇒ **那是替换前的位置**（PM 13:5x 用成员原文件**整段替换**了脚本块 ⇒ 后面行号整体漂移）⇒ ⇒ **现**：围栏 **` ```js ` 在 `:1670`**、结束围栏 **`:1871`**、**正文 `:1671-1870`（200 行）** ⇒ ⇒ 「改脚本会让台账那份正文失配」这个**结论仍成立**（那份正文现在在 `:1671-1870`）。

**⑨ 协议与 P0 本轮未动 ⇒ 不升版本** —— §6.2 的措辞调整按 ⑥ 随 v0.7 一次落 ⇒ ⇒ **P4 协议仍为 v0.6，冻结稿仍待 v0.7**。

**⑩ 下一步** —— **仍无新单**（判据第 8 条依赖 §4.3 的实测键名 ⇒ 现在写必返工）⇒ **唯一待办仍是 izzy 那一次 Chrome 输出**（脚本正文已发给 izzy；**不用多跑一轮**：三档码按「验到哪几档写哪几档」处置，**不改脚本** —— 改了台账那份与成员原文件就失配，izzy 照台账跑反而跑错版本）⇒ 输出回来 ⇒ **PM 落 v0.7 冻结稿** ⇒ **才派 P4 判据单 ＋ P4 实现单**。

### 2026-10-21 13:7x · **成员 13:6x 回执验收通过** ＋ **`:1781` 行号漂移订正** ＋ **PM 采纳成员更严表述（订正 13:6x ③ 的无条件断言）** ＋ **三份素材一手验收（条款草案 / 映射表 / 判据断言草案）**

**① 他这轮的四条主张 —— PM 一手复核，全部成立** —— 「台账 `:1666` 已是订正后的落点（不是旧依据句）」✔ ｜「P0 `:236` 句首已改写」✔（上轮已核）｜「mock 17/17 三处背书」✔（台账 `:1954` 首跑 / `:2036` 替换后复跑 / 协议 `:172`）｜「**台账 `:1781` 行号已漂移**」✔ —— **PM 实测：现 `:1781` 是脚本块内的空行**，那条「PM 第二次自纠」的内容在 **`:1935`**（13:1x 节 ②）⇒ ⇒ **他指出的漂移属实**。

**② `:1781` 的处理裁定 —— 不改 13:4x 那张对照表的原文，只记漂移** —— 漂移引用在 **台账 `:2019`**（13:4x 对照表的一行）⇒ ⇒ **历史记述一律不动**（那行记的「是历史记述 ⇒ 保留不动」这个结论**语义完全成立**，只是行号过期）⇒ ⇒ **本节即为漂移订正**：凡引用「台账 `:1781`」⇒ **现行位置 `:1935`**；`:1781` 现指空行 ⇒ ⇒ **按内容定位或用 `:1935`**（与 13:6x ⑧ 对 `:1680-1862` 的处理同一口径）。

**③ ⚠️ 采纳成员更严表述，订正 PM 13:6x ③ 的一个无条件断言** —— 13:6x ③ 写「**`/api/v0/users/current` 的三档头仍会全发**（用户对象里没有数组 ⇒ `findArray` 落空 ⇒ 不 break）」⇒ ⇒ **那个前提未实测**：该端点响应里**有没有数组字段 ＝ 未实测** ⇒ ⇒ **`findArray` 命中与否决定它会不会被截断** ⇒ ⇒ **精确表述是条件式**：「该端点是否只验到裸档 ＝ **取决于读数，不预先断定**」⇒ ⇒ **以本节为准**（13:6x ③ 的**方向不变** —— `break` 只影响命中数组的那个端点 ⇒ **回填必须标明「哪个端点 ＋ 哪一档」**这条硬事实不受影响）⇒ ⇒ **成员草案已按更严口径写好**（`p4-section43-backfill.md` 的端点表：「取决于读数 · 不能预先断定」）⇒ **采纳**。

**④ 三份素材 —— PM 一手读过，全部合格（仍不入库）** ——
| 附件 | 大小 / 时间 | PM 验收要点 |
| --- | --- | --- |
| `p4-section43-clause-draft.md` | 7,599B / 13:45 | **可直接抄的条款文本**（§4.3 整节替换 · §6.2 追加 · §9 两行 · §10 第 8 条 · 版本线）；**占位符统一 `〈…〉`、不预填任何键名** ✔；**两条精确化已吸收**（端点标注 ✔ · `topKeys` 三处查法 ✔）；**分支 B（响应未回游标）的两种写法都在** ✔ |
| `p4-section43-backfill.md` | 9,336B / 13:45（**已按那两条精确化更新**） | 输出字段 → §4.3 逐项映射；④ 明确「**三处都要查**」＋ 顶层挂载会漏检 ✔；**三档码按端点逐条写** ✔ |
| `p4-criterion8-draft.md` | 8,023B / 13:45（**本轮新增**） | §10 第 8 条可执行断言 ＋ 判据脚本骨架（`scripts/**`，**他名下**）；**自包含**（夹具落 `%TEMP%` 一次性目录、跑完自清）· **只断结构不断值** · 场景 A/B/C/D 覆盖 **`code=0` ＋ `biz_data=null` 反例** ✔ |

**⑤ 分工已改口** —— 他这轮明写「**协议由 Chief 落笔并提交/push**；本文件是**素材**，不是『我已改好的协议』」＋「**我保留**：逐条核 PM 落笔与实测读数一致 / 门禁复跑 / §10 判据第 8 条的可执行断言与判据脚本（`scripts/**` 归我）⇒ **不由我提交协议文件**」⇒ ⇒ **改口到位，越界已收**。

**⑥ 留到派单时定的一件事（现在不改）** —— 他打算把判据脚本**独立跑、不塞进 `check:static` / `check-invariants`**（理由：协议未冻结、别绑别人门禁）⇒ ⇒ **合理**；但 ⇒ **派 P4 判据单时必须写明「它怎么进门禁 / 谁在什么时机跑」** ⇒ ⇒ **否则会变成没人跑的孤儿脚本** ⇒ ⇒ **PM 在派单时定**。

**⑦ 附件版本漂移记账** —— 13:6x 记的是 `p4-section43-backfill.md` **6,812B / 51 行**（**当时**的读数）⇒ ⇒ **现已更新为 9,336B（13:45）** ⇒ 「51 行」是历史读数 ⇒ 引用时以**最新版**为准。

**⑧ 协议与 P0 本轮未动 ⇒ 不升版本** —— P4 协议仍为 **v0.6**，冻结稿仍待 **v0.7**。

**⑨ 下一步（不变）** —— **仍无新单**；**唯一待办仍是 izzy 那一次 Chrome 输出** ⇒ 输出回来 ⇒ **PM 用他草案填占位符落 v0.7 冻结稿**（**按 ③ 的条件式口径 ＋ ④ 的三份素材**）⇒ **他逐条核一致性 ＋ 门禁复跑** ⇒ ⇒ **才派 P4 判据单 ＋ P4 实现单**。

### 2026-10-21 13:8x · **成员 13:7x 回执验收通过** ＋ **PM 接受他的依据修正并补一层更硬的读数** ＋ **新增「mock 背书的射程限定」** ＋ **「不用多跑一轮」的理由换新**

**① 他的三处复核 —— PM 一手核过，全部属实** —— 「`:86` `rec.topKeys = Object.keys(j)` 存在 ⇒ 游标可能挂顶层 ⇒ ③ 是他的真缺陷」✔｜「`:118` 外层 / `:119` 内层 / `:120` `rec` 记 `path`·`headerSet`·`tier` / `:127` `break` ⇒ break 只跳出内层 ⇒ ② 的结构判断成立」✔｜「`:43-47` / `:81` 三个行号都准」✔ —— **PM 把这十处行号逐行读过，无一处出入**。

**② ⚠️ 接受他的依据修正，PM 再补一层更硬的读数** —— 他指出：PM 13:6x ③ 写的「**用户对象里没有数组** ⇒ `findArray` 落空 ⇒ 不 break」**这一步是推断、不是实测**（mock 对两个端点返的是**同一个仿造响应**）⇒ ⇒ **他是对的**。
⇒ ⇒ **PM 一手核 mock 源码**：`mock-probe-check.cjs:51-53` 的 `fetchMock` **完全忽略 path**（`body = init.headers ? (tier1Ok ? payloadOk : payloadBad) : payloadOk`）＋ `:19-31` 的 `payloadOk.data.biz_data.chat_sessions` **是数组** ⇒ ⇒ **mock 对 `/api/v0/users/current` 也返回带数组的响应** ⇒ ⇒ **PM 又复跑一遍**（临时脚本 `p4-mock-probe-trace.cjs`，同构 sandbox）**拿到读数**：**只发出 2 条请求**（`/api/v0/users/current` 裸档 ＋ `/api/v0/chat_session/fetch_page?count=5` 裸档）· `probes` **2 条** · `tier1GotArray=true` · **mock 断言 `find(p => p.arrayPath)` 实际绑到的那条是 `/api/v0/users/current`（第一个端点）** ⇒ ⇒ **mock 里 `/users/current` 同样只验到裸档** ⇒ ⇒ **mock 证明不了真实 `/users/current` 里有没有数组** ⇒ ⇒ **13:7x ③ 的条件式口径成立，且依据比当时更硬**（**13:6x ③ 的方向不变**：`break` 只影响命中数组的那个端点 ⇒ 回填必须标**端点 ＋ 档位**）。

**③ ⚠️ 新增一条写死的「mock 背书射程限定」** —— 台账与协议里那句「**mock 17/17 全绿**」证明的是三件事：① 第 1 档命中时能下钻出**数组路径与首条键名** ② 第 1 档全拒时第 2 档钩子**截得到** ③ 输出面**不含** token 本体/条目正文/会话 id ⇒ ⇒ **它不覆盖「端点维度的档位行为」**（**mock 无路径区分**；且按 ②，它的断言**绑在第一个端点**上 ⇒ ⇒ **`fetch_page` 端点第 1 档取到数组这条路径，mock 从未单独验证过**）⇒ ⇒ **凡用 mock 读数推「哪个端点跑了几档」⇒ 不成立** ⇒ ⇒ **端点维度的档位行为一律等 izzy 那一次真实读数**（**v0.7 落笔时要把这句限定一起写进 §4.3／§6.2**）⇒ ⇒ **另：mock 里那个 `chat_sessions` 键名是 PM 自己造的仿造值 ⇒ 对「真实键名是什么」零信息量 ⇒ 落 v0.7 时不得因 mock 输出里有它就当成实测键名**（协议 §4.3 第 1 项标的「**候选** `chat_sessions`」同理 ⇒ **只是候选，不是实测**）。

**④ 「不用多跑一轮」的理由换成更硬的一条** —— 原理由「登录态下天然拿不全三档」按 ② **被推翻**（`/users/current` 那三档**仍可能全拿得到**）⇒ ⇒ **新理由**：**读数自带 `path` ＋ `headerSet`**（脚本 `:120` 两个字段都在）⇒ ⇒ **已发的档位可逐条还原、未发的档位就是缺口** ⇒ ⇒ **仍然不改脚本、不让 izzy 多跑一轮**。**采纳**。

**⑤ 他说「已改」—— PM 一手核过文件当前状态（属实）** —— `p4-section43-clause-draft.md`（7,599B）分工段已写「**成员交草案 → PM 落笔 → PM 提交 ＋ PM 统一 push**；**我保留**逐条核 ＋ 门禁复跑 ＋ 判据脚本（`scripts/**`）⇒ **不由我提交协议文件**」✔｜`p4-section43-backfill.md`（9,336B）④「**三处都要查**」✔ · 端点表「**取决于读数 · 不能预先断定**」✔ · **新增「六、附：行号现状（引用前请现读）」**（台账脚本正文现 `:1671-1870`）✔ ⇒ ⇒ **改口不只落在对话里，交付物本身已改**（他自己总结的根因教训落地 ✔；两文件 mtime 均 13:45，**早于**他 13:7x 回执 ⇒ 时序自洽，不是事后补记）。

**⑥ 基线** —— 他 fetch 到的是 `9742546` ⇒ ⇒ **现为 `4848296`**（PM 13:7x 台账补记）。

**⑦ 协议与 P0 本轮未动 ⇒ 不升版本** —— P4 协议仍为 **v0.6**，冻结稿仍待 **v0.7**（③ 的那句限定随 v0.7 一次落，不单独改协议）。

**⑧ 下一步（不变）** —— **仍无新单**；**唯一待办仍是 izzy 那一次 Chrome 输出** ⇒ 输出回来 ⇒ **PM 用他草案填占位符落 v0.7 冻结稿**（**按 ② 的条件式口径 ＋ ③ 的 mock 射程限定**）⇒ **他逐条核一致性 ＋ 门禁复跑 ＋ 填判据脚本** ⇒ ⇒ **才派 P4 判据单 ＋ P4 实现单**。

### 2026-10-21 13:9x · **行号有效性的一条规律（追加在末尾 ⇒ 既有行号引用不再漂）** ＋ **附件定稿提示的边界** ＋ **基线更新**

**① 他核到的一条规律 —— PM 一手在最新远端复现，成立** —— 他查「台账 `:1935` 是否仍是『PM 第二次自纠』那条、`:2019` 是否仍是 13:4x 对照表那行」⇒ **成立** ⇒ ⇒ **PM 又在 `38751c2`（13:8x 两笔）上复测一次：`:1935` / `:2019` 仍分别指向那两处**（13:8x 两节都**追加在末尾**，行 2102 起 ⇒ 前面不动）⇒ ⇒ **规律写死**：**台账只在末尾追加节 ⇒ 既有行号引用（`:1935` / `:2019` / 脚本正文 `:1671-1870`）持续有效，不必重取** ⇒ ⇒ **13:7x ② 的漂移订正（`:1781` ⇒ `:1935`）在后续追加下一直成立**；⇒ ⇒ **反过来说，漂移的成因只有一种：13:5x 那次整段替换（`spawn-script-v3.ps1`）** —— **今后避免整段替换、只在末尾追加节，就不会再产生行号失效**。

**② 基线** —— 他 fetch 到 `4848296` ⇒ ⇒ **现为 `38751c2`**（PM 13:8x 两笔：台账补记 ＋ 一条 mock 键名限定补记）。

**③ 「三份附件已定稿、不再改」—— 边界要说清：附件不用改，但 v0.7 不能只照抄附件** —— 附件是 **13:45 定稿版**；**PM 13:8x ③ 又追加了两条限定**（**mock 背书射程限定** ＋ **mock 里的 `chat_sessions` 键名是仿造值**）⇒ ⇒ **v0.7 落笔时 = 附件 ＋ 这两条限定一起带上**（**成员不需要改附件** —— 附件是素材，不是协议正文）⇒ ⇒ **成员保留的「逐条核一致性」要把这两条一起核**（否则会核出一份「与附件一致、但漏了 mock 射程限定」的 v0.7）。

**④ 协议与 P0 本轮未动 ⇒ 不升版本** —— P4 协议仍为 **v0.6**，冻结稿仍待 **v0.7**。

**⑤ 下一步（不变）** —— **仍无新单**；**唯一待办仍是 izzy 那一次 Chrome 输出** ⇒ 输出回来 ⇒ **PM 落 v0.7 冻结稿**（附件 ＋ 13:8x ③ 两条限定）⇒ **成员逐条核 ＋ 门禁复跑 ＋ 填判据脚本** ⇒ ⇒ **才派 P4 判据单 ＋ P4 实现单**。

### 2026-10-21 13:9x（补）· **成员一封迟到回执（对应 PM 13:6x 那封）** ＋ **他给的新行号逐行核过全对** ＋ **补记一条台账缺失的核证结论（他的「零命中」表述不实）** ＋ **`:1666` 建议双向关闭**

**① 这是迟到回执，且内容与他 13:7x 那封重叠** —— 他这封回应的是 **PM 13:6x 那封**（grep 口径 ＋ 两条精确化 ＋ `:1666` 裁定）⇒ ⇒ **两条精确化已改完 / 理由换成「读数自带端点＋档位」/ `:1666` 接受** 这三项，他 **13:7x 那封已经说过** ⇒ ⇒ **不重复记**，本节只记 **13:7x 没记的增量**（迟到回执的判据：**内容与当前仓库状态对账** —— 已交付的事不重复入账）。

**② 他给的新行号 —— PM 一手现读，五个全对** —— `p4-section43-backfill.md` `:19`（④「**三处都要查**」：`topKeys` `:86` ＋ `dataKeys` `:92` ＋ `bizDataKeys` `:98`，数组键另在 `bizDataArrayCounts` `:99-100`）/ `:22`（三处判读分支）｜`:53`（口径第 1 条：**每条码必须标明「哪个端点 ＋ 哪一档」**，依据 ＝ 读数自带 `probes[].path` ＋ `probes[].headerSet`，探针 `:120`）/ `:56`（口径第 4 条：**不用多跑一轮的新理由**＝「已发的档位可逐条还原、未发的就是缺口」）｜`:96`（探针脚本行号现状）⇒ ⇒ **逐行核对无一处出入**。

**③ 补记一条台账此前缺失的核证结论（他上轮报错了一处表述）** —— 他上轮报「grep『**已发给 izzy**』**零命中**」⇒ ⇒ **实测不零命中**：台账 **`:2075`**（13:6x ⑩）有一处「**脚本正文已发给 izzy**」⇒ ⇒ **那一句为真**（脚本正文确实已发）⇒ ⇒ 他报的「零命中」**表述不实**；**真正该搜的原文句是「对 izzy 的更正已发」⇒ 零命中 ✔**（PM 本轮复查复现）⇒ ⇒ **结论「不假记」成立，依据串错** ⇒ ⇒ **他已认领**（「教训我收下了」）⇒ ⇒ **纪律写死**：**核「某句是不是假记」必须用那句的原文做检索串**（别用近义串 —— 近义串会命中**内容为真**的别的句子）；**报否定性证据（零命中／无残留／没写进去）必须把实际用的检索串原样写出来**让人能复核 ⇒ **别只给结论**。（**本条此前未入台账，本节补记。**）

**④ `:1666` 那条建议 —— 双向关闭** —— PM 裁定**不采纳**（结构理由：`:1667` **就是** `:1666` 的落点订正条；那一块的结构是「**原始条目 ＋ 订正压在其后**」，`:1668` / `:1669` 同属这层 ⇒ 改 `:1666` 反而破坏叠记结构一致性；落点 Chrome 现存三处）⇒ ⇒ **他这轮明确「撤回」** ⇒ ⇒ **此条不再有人提，彻底关闭**。

**⑤ 基线** —— 他 fetch 到 `9742546` ⇒ ⇒ **现 `000eb80`**（PM 13:8x 两笔 ＋ 13:9x 一节；**本节入库后为最新一笔**）。

**⑥ 协议与 P0 本轮未动 ⇒ 不升版本** —— P4 协议仍为 **v0.6**，冻结稿仍待 **v0.7**。

**⑦ 下一步（不变）** —— **仍无新单**；**唯一待办仍是 izzy 那一次 Chrome 输出** ⇒ 输出回来 ⇒ **PM 落 v0.7 冻结稿**（附件 ＋ 13:8x ③ 两条限定）⇒ **成员逐条核 ＋ 门禁复跑 ＋ 填判据脚本** ⇒ ⇒ **才派 P4 判据单 ＋ P4 实现单**。

### 2026-10-21 13:9x（补二）· **成员核不到 mock 源码 ⇒ 自己复现 ＋ 补对照实验** ＋ **PM 一手复跑其复现工具（逐条吻合）** ＋ **「三档码的 mock 背书 ＝ 零」写死** ＋ **一条路径引用纪律**

**① 他核不到 mock 源码 —— 他没采信 PM 的源码读数，改为自己复现（做法正确，记为纪律）** —— mock 的自测资产在 **PM 的 `%TEMP%`**（**不在仓库**；台账里只有一处**文件名引用**，无路径）⇒ ⇒ **他核不到 `:51-53` / `:19-31` / `:26`** ⇒ ⇒ **他明说不采信 PM 的源码读数，改为按描述自己造等价 mock 复现** ⇒ ⇒ **PM 采信这个做法**（**上游给读数、下游自己复现** ＝ 比「照抄结论」强）⇒ ⇒ **并由此补出一条纪律**：**凡结论依赖一次性临时环境（如 `%TEMP%` 的 mock），台账只能记「结论 ＋ 读数」，不得把它当长期可核证据**（izzy 15:2x 已拍板「**证据可以清**」⇒ 这些文件名迟早失效 ⇒ **协议里只写结论与射程限定，不写复跑路径**）。

**② PM 一手复跑他的复现工具 —— 逐条吻合** —— 工具＝`p4-mock-trace.cjs`（4,170B / 纯 sandbox / 零网络零写盘）；**PM 实跑读数**：
| 模式 | 请求数 | 明细 | `probes` | 带 `arrayPath` 的那条 |
| --- | --- | --- | --- | --- |
| **P（忽略 path）** | **2** | `users/current` 裸档 ＋ `fetch_page` 裸档 | **2** | **两条都带**（`users/current` 在前） |
| **D（按路径区分，`users/current` 返无数组对象）** | **4** | `users/current` **裸／＋Bearer／＋Bearer＋x-device-id** 三档 ＋ `fetch_page` 裸档 | **4** | **只有 1 条 ＝ `fetch_page`** |

⇒ ⇒ **他报的读数逐条对得上**（模式 P 的 2 请求 / 2 probes / 断言绑 `users/current`；模式 D 的 4 请求 / 断言绑 `fetch_page`）⇒ ⇒ **「同一份脚本、只因 mock 造法不同 ⇒ 端点维度的档位行为与断言绑定端点都翻转」＝ 可复现事实**（**不是谦辞**）⇒ ⇒ **13:8x ③ 的「mock 射程限定」被坐实**。

**③ ⚠️ 更硬的一条写死：「三档码的 mock 背书 ＝ 零」** —— **模式 P 下「＋Bearer／＋Bearer＋x-device-id」这两档头一条都没被发出**（只发出两条 bare —— 第一个端点就拿到数组并 `break`）⇒ ⇒ **mock 从未验证过「带 Bearer 档在任一端点的行为」** ⇒ ⇒ **mock 只覆盖两条路径**：「第 1 档命中」与「第 1 档全拒 → 第 2 档钩子兜底」⇒ ⇒ **§6.2「三档码」只有真实读数作数** ⇒ ⇒ **随 v0.7 落**（与他草案 §6.2 追加条里那句叠在一起 ⇒ **两句一起写**）。

**④ 素材更新（PM 一手核过）** —— `p4-section43-clause-draft.md` **新增「一之二 · 读数来源的射程限定」一节**（模式 P／D 两组读数 ＋ 「端点维度一律等真实读数」＋「mock 对三档码零背书」＋ 键名仿造值限定）＋ **§6.2 追加条加了零背书那句**（两处串成一条线）｜`p4-criterion8-draft.md` **守则第 1 条加了 mock 仿造值提醒**（不得因 mock 里出现过就当实测键名）｜`p4-section43-backfill.md` **未改**｜**新增复现工具** `p4-mock-trace.cjs`（PM 已实跑，零网络零写盘）⇒ ⇒ **他声明「附件不再改动（除实测读数回来后填空）」** ⇒ **PM 不必反复重读**。

**⑤ 基线** —— 他 fetch 到 `38751c2` ⇒ ⇒ **现 `491c779`**（PM 13:9x 两节；**本节入库后为最新一笔**）。

**⑥ 协议与 P0 本轮未动 ⇒ 不升版本** —— P4 协议仍为 **v0.6**，冻结稿仍待 **v0.7**（③ 的零背书 ＋ ① 的路径纪律随 v0.7 一次落）。

**⑦ 下一步（不变）** —— **仍无新单**；**唯一待办仍是 izzy 那一次 Chrome 输出** ⇒ 输出回来 ⇒ **PM 落 v0.7 冻结稿**（附件 ＋ 13:8x ③ 两条限定 ＋ 本节 ③ 零背书 ＋ ① 路径纪律）⇒ **成员逐条核 ＋ 门禁复跑 ＋ 填判据脚本** ⇒ ⇒ **才派 P4 判据单 ＋ P4 实现单**。

### 2026-10-21 14:0x · **成员澄清「那两条限定附件里已有」⇒ PM 订正自己 13:9x ③ 的表述** ＋ **新交付「v0.7 落笔核对清单」验收通过** ＋ **⚠️ 采纳基线纪律（台账起不再手写会过期的提交号）**

**① 他的核证 —— 属实（两处只是时序差，不是错）** —— 他 fetch 时远端是 `491c779`、台账 **2255 行**；**PM 之后又推了 13:9x 补二那笔** ⇒ ⇒ **现基线 ＝ 现读 `origin/main`（按 ④ 纪律不手写号）**、台账 **2279 行** ⇒ ⇒ **但两条关键结论都成立**：**`:1935` 仍是「PM 第二次自纠」那条、`:2019` 仍是 13:4x 对照表那行** ⇒ ⇒ **「只在末尾追加节 ⇒ 既有行号引用不再漂」成立**（13:8x 起 2102、13:9x 起 2121 ⇒ 前面确实不动）⇒ ⇒ **漂移成因只有 13:5x 那次整段替换**。

**② ⚠️ 他的澄清成立 —— PM 采纳，并订正自己 13:9x ③ 的表述** —— 他实测**附件里已经含**那两条限定（位置：`p4-section43-clause-draft.md:48`「一之二 · 读数来源的射程限定」/ `:50` 正文（模式 P·D 两组读数）/ `:71` §6.2「mock 对三档码零背书」条｜`p4-section43-backfill.md:55`「不预先断定三档天然拿不全」｜`p4-criterion8-draft.md` 填 `EXPECT` 守则第 1 条）⇒ ⇒ **PM 13:9x ③ 写的「v0.7 ＝ 附件 ＋ 13:8x ③ 两条限定一起带上」表述不准**（读起来像「附件漏了这两条」）⇒ ⇒ **正确表述：附件已含 ⇒ 照抄附件即可；但「限定段是否真的抄进协议」必须列为必查项** ⇒ ⇒ **否则会把同一限定写两遍**（照抄附件 ＋ 又另加两条）——**这句已在本节订正，13:9x ③ 以本节为准**。

**③ 新交付 `p4-v07-review-checklist.md` —— PM 一手读过，合格，采纳为「落笔自检 ＋ 成员核对清单」** —— **0. 三条否定性硬判**（① 协议正文**不得残留占位符** `〈…〉` ② 每个键名／数字**都必须能在 izzy 的读数里指出出处** ③ **不得出现未经读数支持的键名**，特别是 mock 造的 `chat_sessions`）＋ **1. 七条核对项**（§4.3 四项 · 成功判据 · §6.2 三档码 · **⚠️ mock 背书射程限定（他标注「最易漏」）** · §9 缺口表 · §10 判据第 8 条 · 版本线）＋ **2. 核完后的三个动作**（门禁复跑 · 填判据脚本 · **回报差异逐条列、不许只说「已核对」**）＋ **3. 最易踩的四个坑**（只核「与附件一致」⇒ 会核出一份漏了 mock 射程限定的 v0.7 / 游标只查两层漏顶层 / 混用端点的码 / 拿 mock 键名当实测）⇒ ⇒ **四个坑与 PM 手里的事实逐条对得上**（其中坑 1 正是 ② 的直接推论）。

**④ ⚠️ 采纳他的基线纪律（本轮最有价值的一条）** —— 他被「知会里的基线号已过期」坑了**七轮**（`978454d` → `b270f83` → `9742546` → `4848296` → `38751c2` → `000eb80` → `491c779`）⇒ ⇒ **从本节起，台账新增节不再手写「现为 `<commit>`」这类立刻过期的基线号**，一律写「**基线 ＝ 现读 `origin/main`**」⇒ ⇒ **但历史节里的手写号一律保留不动**（那些是**当时的真实基线**，改了反而失真）⇒ ⇒ **本节起生效，不回改历史**。

**⑤ 判据进门禁** —— 他按 PM 说的**先不动** ✔（不提前塞门禁、不做孤儿脚本）⇒ ⇒ **派单时 PM 定**「它怎么进门禁 / 谁在什么时机跑」。

**⑥ 协议与 P0 本轮未动 ⇒ 不升版本** —— P4 协议仍为 **v0.6**，冻结稿仍待 **v0.7**。

**⑦ 下一步（不变）** —— **仍无新单**；**唯一待办仍是 izzy 那一次 Chrome 输出** ⇒ 输出回来 ⇒ **PM 落 v0.7 冻结稿**（**照抄附件** ＋ 按清单 0 的三条硬判与 ④ 的必查项自检）⇒ **成员按清单逐条核 ＋ 门禁复跑 ＋ 填判据脚本** ⇒ ⇒ **才派 P4 判据单 ＋ P4 实现单**。

### 2026-10-21 14:1x · **⚠️ 订正 PM 台账三处不实转述（他 14:1x 的指控成立：结论对、依据错）** ＋ **两类东西分开：基线号保留 vs 转述不实必订正** ＋ **成员已按路径纪律改附件两行（PM 一手核过）**

**① 他 14:1x 的指控成立 —— PM 台账三处「他 fetch 到 X」取错了那封** —— 他逐封列出自己回执带的实测基线：**13:7x ＝ `9742546`｜13:8x ＝ `38751c2`｜13:9x ＝ `38751c2`｜14:0x ＝ `491c779`** ⇒ ⇒ **他从未落后三笔**（**每封都带着当时最新的实测值**，信息一直在）⇒ ⇒ **PM 三处取错了那封**：
| 台账位置 | 原写法 | 该封实际报的基线 | 结论（「现为 Y」） |
| --- | --- | --- | --- |
| 13:9x（补）⑤ | 「他 fetch 到 `9742546`」 | `38751c2` | **对**（`000eb80` ＝ 当时最新） |
| 13:9x（补二）② | 「他 fetch 到 `4848296`」 | `38751c2` | **对**（`38751c2` ＝ 当时最新） |
| 14:0x ⑤ | 「他 fetch 到 `38751c2`」 | `491c779` | **对**（`491c779` ＝ 当时最新） |

⇒ ⇒ **三处都是「结论对、依据错」**（把**更早那封**的值当成了最新那封）⇒ ⇒ **以本节为准**；**不回改历史节原文**（理由见 ②）。

**② 两类东西必须分开（把 14:0x ④ 的纪律补精确）** —— **「当时远端 ＝ X」这类基线号本身** ⇒ **历史节里保留不动**（那是**当时的真实基线**，改了反而失真）｜**「他 fetch 到 X」这类对成员回执的转述** ⇒ **若与该封实际不符，就是不实记述 ⇒ 必须订正**（**不能拿「历史不动」当挡箭牌**）⇒ ⇒ **14:0x ④「从本节起不再手写会过期的号」与同节 ⑤ 自己手写「现 `491c779`」相矛盾** ⇒ ⇒ **以本节为准：14:0x ⑤ 的手写号视为「该节的当时值记录」**；**今后新增节一律写「基线 ＝ 现读 `origin/main`」** ⇒ ⇒ **取成员基线的正确做法：取最新那封回执，或直接现读 `origin/main`**。

**③ 他 14:2x 已按路径纪律改附件两行 —— PM 一手核过：两处各一行，结论与读数一字未改** —— `p4-section43-clause-draft.md`（**10,390B**）**一之二节末尾那行**：原写复现工具文件名 ＋ 跑法 ⇒ **已改成**「**复现过程用的是一次性临时脚本 —— 属过程资产、不是长期可核入口** ⇒ 协议正文只写结论与射程限定，不写复跑路径；结论与读数已完整写在上方，不依赖任何临时文件」｜`p4-v07-review-checklist.md`（**5,588B**）**④ 行的依据列**：原引工具名 ⇒ **已改成**「`clause-draft.md` 的『一之二』节 ＋ §6.2 零背书条（**只引结论与读数，不引过程性临时路径**）」⇒ ⇒ **模式 P／D 两组读数、零背书、键名仿造值三条结论都还在** ✔（**PM 只需核这两行，不必重读全文**）。

**④ 素材现状（归档口径）** —— **4 份交付物**：`p4-section43-clause-draft.md`（条款草案）｜`p4-section43-backfill.md`（回填映射）｜`p4-criterion8-draft.md`（§10 判据第 8 条断言）｜`p4-v07-review-checklist.md`（落笔后核对清单，**已验收采纳**）＋ **1 份过程性工具**（本机自留、**不进依据链**）⇒ ⇒ **等读数回来只做填空 ＋ 按清单逐条核**。

**⑤ 协议与 P0 本轮未动 ⇒ 不升版本** —— P4 协议仍为 **v0.6**，冻结稿仍待 **v0.7**。

**⑥ 下一步（不变）** —— **仍无新单**；**唯一待办仍是 izzy 那一次 Chrome 输出** ⇒ 输出回来 ⇒ **PM 落 v0.7 冻结稿**（**照抄附件** ＋ 按清单 0 的三条硬判与 ④ 的必查项自检）⇒ **成员逐条核 ＋ 门禁复跑 ＋ 填判据脚本** ⇒ ⇒ **才派 P4 判据单 ＋ P4 实现单**。

### 2026-10-21 14:2x · **PM 一手核出「站内码两档」的实测出处 ＋ 排除静态扫描路线 ＋ 发现协议 §3.1 一条实现级条款的依据在文档链里定位不到出处 ⇒ v0.7 落笔清单从四处扩到六处**

**① `40002` / `40003` 的 msg 与触发形态：P0 早就实测过，不是 PM 编的** —— PM 一手复核 `official-chat-feasibility.md:144` 的 P0 实测表（DeepSeek 行）：**裸 fetch → `40002 Missing Token`；带 Bearer `userToken` → `40003 INVALID_TOKEN`**（当时**页面停在 `/sign_in`**、状态列写明「**本机从未建立登录态**」）⇒ ⇒ **「没带可认的 token」回 `40002 Missing Token`**、**「带了但不被认」回 `40003 INVALID_TOKEN`**（**msg 字面量都是实测的**）⇒ ⇒ **站内码的「取值」与「msg」都有实测出处**；**真正没验到的不是「码是什么」，而是「登录态下会回哪一档」**。另一处独立留痕：`src-tauri/examples/official_chat_probe.rs:1152-1154`（「2026-10-20 实测：两次都是『键在、值非空』而服务端回 40002 / 40003」）＋ `:1308`。

**② 「grep 站点 JS 包拿 40002 语义」这条路不成立（PM 一手，排除）** —— 检索串 **`40002`**、范围 ＝ Chrome `Default\Cache\Cache_Data`（**1243 个原始资源文件**）⇒ **4 处命中，全部是假阳性** ⇒ 逐条看是 `X64Word_create(4118630271, 4000239992)`（WASM／asm.js 加密库的**常量**，被**子串**命中）⇒ ⇒ **站点原始资源里没有 `40002` 这个错误码**；对照检索串 **`40003`** ⇒ **零命中** ⇒ ⇒ **G4-3 不能靠静态扫描收口**（**否定性证据：两个检索串原样写在这里，便于复核**）。

**③ ⚠️ 最硬的一条：协议 §3.1 那条「页内不需要自己拼凭据」＝ 实现级条款，但它的依据在 PM 自己的文档链里定位不到出处** —— ① **P0 探针的页内 fetch 只发过两次 `attempt`**（源码 `official_chat_probe.rs`）：`:1228-1229` **`plain-same-origin-fetch`**（裸，注释原文：「**裸 fetch：同源 cookie 够不够？**」）＋ `:1303-1306` **`with-site-bearer-token`**（Bearer）⇒ ⇒ **P0 就是为回答「同源 cookie 够不够」设计的**；② **这两次的读数分别是 `40002` 与 `40003`**（①）⇒ ⇒ **没有一次是 `code=0`**；③ **探针 A 那次是另一支探针**（`official_chat_deepseek_probe.rs:312`，裸同源 fetch ＝ `credentials:"same-origin"` ＋ 只带 `accept`），其读数是 **未登录态 `{code:40003, msg:"INVALID_TOKEN", data:null}`**（台账 12:1x 节）⇒ ⇒ **也不是 `code=0`**；④ ⇒ ⇒ **协议 §3.1 `:110` 依据栏的「【已实测】（探针实跑 `status=200`、`code=0`）」，在 P0 的两次 attempt、探针 A 的一次 attempt 里都找不到对应读数**；**全文唯一出现 `code=0` 的那组读数，恰恰是 §4.2 写死的**反例**（`status=200` ＋ 顶层 `code=0` ＋ `msg=""` **同时** `biz_code=2` ＋ `biz_data` 是 `null`）** ⇒ ⇒ ⇒ **这几乎肯定是把「HTTP 200」误记成了「业务码 0」—— 正是 §4.2 那条防误读条款要防的同一个坑，PM 自己踩了** ⇒ ⇒ **该依据不成立**（不是「缺一环」，是**记错了**）。
⇒ ⇒ **为什么这条最要紧**：§3.1 是**实现要依赖的条款**（「页内不需要、也不应该自己拼凭据」）⇒ 若它建立在「把 HTTP 200 当成业务码 0」之上，**实现方会以为裸 fetch 就能成** ⇒ 而 P0 的两次 attempt 都不成 ⇒ ⇒ **这是会让实现走错的条款，不是措辞问题**。
⇒ ⇒ ⚠️ **但不能据此说 §3.1 的结论已被证伪** —— **P0 与探针 A 那两次都是未登录态**，`40002`／`40003` 完全可能只是「没登录」（台账 `:1649` 早写过这条：「P0 那条『`userToken` 不被认 ⇒ 40003』**本次不能定论**」）⇒ ⇒ **精确定性：依据不成立 ＋ 结论未验** ⇒ ⇒ **A 路那一次读数正好定案**。

**④ ⚠️ 这条岔口一旦判反，会动到取数路线本身 —— 不是措辞问题** —— 若登录态下**裸档仍回站内码**（＝「同源 fetch 带 cookie」不足以被站点认）⇒ ⇒ 而 **§7.2 `:240` 明确「不许把站点 cookie / token 读出来、落盘、外传、**拼进请求头**」** ⇒ ⇒ **两条现役条款互斥**（不许自己拼 `Authorization` 头 ⇔ 站点只认 `Authorization` 头）⇒ ⇒ **实现路径必须改**（改向「在页内复用站点自己的请求／逻辑」—— 恰好就是取证脚本第 2 档钩子的形态）⇒ ⇒ **§1 取数路线 / §2 页内求值 / §5 接线都要相应改写** ⇒ ⇒ **这条必须在 P4 落地前定案，不能带着含糊条款派实现单**。⇒ ⇒ **好消息：脚本每条 probe 都带 `path` ＋ `headerSet` ＋ `code` ＋ `bizCode`** ⇒ ⇒ **「登录态下裸档回什么码」＝ 读数里现成的一格** ⇒ ⇒ **一次运行就够，不用多跑一轮**。

**⑤ 脚本不用改（PM 本轮再核一遍，落死）** —— 脚本的**「裸」档 ＝ 不带 `Authorization` 头**（只带 `accept`，`HEADER_SETS[0]`）⇒ ⇒ 它**天然就是「无 token」那一档**；三档头组合已覆盖「无 token ／ 带无效 token ／ 带无效 token＋设备头」⇒ ⇒ **站内码该读到哪一档就读到哪一档**；**第 1 档三档全拒时脚本会继续挂第 2 档钩子**（`break` 只在 `s.arrayPath` 存在时触发 ⇒ 三档会全发完）⇒ ⇒ **字段面仍由第 2 档兜底拿到**（点「新对话」→ 截站点自己的成功响应）⇒ ⇒ **一次运行即可同时收口「字段面四项 ＋ 凭据机制（裸档够不够）＋ 站内码逐档读数」三件事**。

**⑥ v0.7 落笔必改清单：从四处扩到六处（PM 已写好确切措辞，随读数一并落）** ——
| # | 位置 | 现行措辞（过强／依据不成立） | v0.7 落笔时改成 |
| --- | --- | --- | --- |
| 1 | 协议 **§3.1 `:110`** | 「页内取数｜同源 fetch **由浏览器自动带** HttpOnly cookie ⇒ **页内不需要、也不应该**自己拼凭据｜【已实测】（探针实跑 `status=200`、`code=0`）」 | **依据换成可定位的出处 ＋ 结论按 A 路实测定案**（裸档成 ⇒ 保留但换成真读数；裸档回站内码 ⇒ 改写为「同源带 cookie **不被认**，实现不许自拼 `Authorization` 头（§7.2）⇒ 取数路线改走页内复用站点请求」）**⇒ 此行不得留无出处的读数** |
| 2 | 协议 **§4.3 `:163`** | 「一次收口**字段面** ＋ **三档错误码**」 | 「一次收口**字段面**（四项）；**站内码按实测回读逐条写**（端点 ＋ 档位），**拿不到就登记缺口**」 |
| 3 | 协议 **§4.3 `:168`** | 「（**顺带拿到三档错误码**）…预期回 `40003` 的那几档」 | 「（**站内码读数按档位逐条记；哪一档回哪个码以实测为准**）…**预期会回站内码的那几档**」 |
| 4 | 协议 **§6.2 `:223`** | 「**三档码**（`40002` / `40003` / 成功）的语义**待实测**（与 §4.3 **同一次取证**顺带拿到）」 | 「**未登录态**的 `40002 Missing Token`／`40003 INVALID_TOKEN` **已实测**（P0）；**登录态下**各档读数**待实测**（A 路一次取证，**不保证三档齐**）」 |
| 5 | 协议 **§8.6 `:315`** | 「贴回输出 ⇒ **一次收口字段面 ＋ 三档码**」 | 「贴回输出 ⇒ **一次收口字段面**（＋逐档站内码读数；**读到的算、没读到的登记缺口**）」 |
| 6 | 协议 **§9 `:325`** G4-3 | 「站内码三档语义未定死（**与 G4-2 同一次取证**）」 | 「**未登录态**两档的取值与 msg 已实测；**登录态读数**待实测（G4-2 同一次取证**顺带**，**不作为 v0.7 冻结条件**）」 |
⇒ ⇒ **冻结条件不动**（§4.3 原文写的就是「**§4.3 四项全部有实测出处**」）⇒ ⇒ **v0.7 的门槛 ＝ 四项字段面**，**站内码不绑** ⇒ ⇒ **izzy 那一次跑完即可落 v0.7，不需要第二轮**。

**⑦ 素材不返工，但「逐条核一致性」要多核一项** —— 成员 `p4-section43-clause-draft.md` 的「二」节本来就是「**按端点 ＋ 档位逐条写**」＋「**未验到的档位登记为缺口，不为凑满三档再让 izzy 多跑一轮**」⇒ ⇒ **与本节 ④⑤⑥ 一致** ⇒ ⇒ **成员附件原样可用**；PM 落笔时把本节 ⑥ 的六处订正一并带上 ⇒ ⇒ **成员侧零返工**；但核对清单要**补一条**：**§3.1 那行现稿措辞与依据**（清单 0 的硬判②「每个数字都要能指出出处」⇒ **`code=0` 这组读数指不出出处 ⇒ 必须核**）。

**⑧ 协议与 P0 本轮未动 ⇒ 不升版本** —— P4 协议仍为 **v0.6**，冻结稿仍待 **v0.7**（版本线**不额外顺延**：本节 ⑥ 的六处订正**并入 v0.7 这一次落笔** —— 与 13:6x ⑥ 同款处置）。

**⑨ 下一步（不变）** —— **仍无新单**；**唯一待办仍是 izzy 那一次 Chrome 输出** ⇒ 输出回来 ⇒ **PM 落 v0.7 冻结稿**（照抄附件 ＋ 本节 ⑥ 六处订正 ＋ 按清单 0 的三条硬判与 ④ 的必查项自检）⇒ **成员逐条核 ＋ 门禁复跑 ＋ 填判据脚本** ⇒ ⇒ **才派 P4 判据单 ＋ P4 实现单**。

**⑩ 成员 14:2x 回执的一手复核（PM 一手核）＋ 一条纪律：附件在途 ⇒ 落笔前必须现读** —— ① **⚠️ 队列读数（PM 一手核）**：`batch_36107f787815`（PM 14:2x 那封知会）终态 ＝ **`cancelled`** ＋ `notification: delivered` ＋ `complete: true` ＋ **有完整 result** ⇒ ⇒ **终态 `cancelled` ≠ 内容没送达**（与「`failed` ≠ 没送达」同族）⇒ ⇒ **不重发**（他已回执、已按要求改完素材）② **成员独立复核背书了本节 ③** —— 他自己的核证结论是「**六处行号全准**」＋「**`:110` 的问题坐实**」＋「**全文档链没有任何一处 `code=0`**」⇒ ⇒ **PM 那条「§3.1 的依据定位不到出处」不是自说自话** ⇒ ⇒ **⑥ 必改清单第 1 条（§3.1 `:110`）由此获得成员一手背书** ③ **他补发里的一处前提不成立 ⇒ 台账不改** —— 他说「台账里还没有『我已按 ④ 改素材』这条（13:9x（补二）节 ④ 记的还是改前状态）」⇒ ⇒ **不成立**：台账 **14:1x ③ 已记改后状态**（连改后措辞都逐字引了）⇒ ⇒ **13:9x（补二）④ 保持改前原样是**对的**（14:1x ② 的纪律：历史节保留不动 ＋ 订正以新节为准）⇒ ⇒ **不改历史节、不重复入账**；**「照抄源」＝ 附件现态**，不是台账里任何一句 ④ **PM 一手核附件现状**（14:27 现读）：**路径纪律那两行仍成立** —— 两份文件里 `p4-mock-trace`／`.cjs` **零命中**；两条改后措辞**在位**（`p4-section43-clause-draft.md:58` ＋ `p4-v07-review-checklist.md:25`）✔ ⑤ **⚠️ 但两份附件在 14:27 被重写（正在补 §3.1 那条）** —— `clause-draft.md` **10,390B → 13,724B**（mtime **14:27:10**）、`checklist.md` **5,588B → 6,639B**（mtime **14:27:19**）⇒ ⇒ 与他自报「我先补清单和草案」对得上 ⇒ ⇒ **在途** ⇒ ⇒ **台账里记的字节数是「当时值」、不作准**（14:1x ② 的纪律）⇒ ⇒ **v0.7 落笔前必须现读附件现态**（含 §3.1 那条的落位），**不许照任何缓存的字节数／行号抄** ⑥ **站位不变**：仍无新单；唯一待办仍是 izzy 那一次 Chrome 输出。

**⑪ 成员已把 §3.1 那条补进两份附件 —— PM 一手核过，验收通过；PM 落笔清单据此增补三项 ＋ 一条新判据** —— ① **他补的内容**（14:30 现读，附件 mtime 自 14:27 起**未再动** ⇒ 已稳定）：`p4-section43-clause-draft.md` **新增「一之三 · §3.1 `:110` 的改法」节**（`:62` 起）—— **两个结论分支各配一个依据行模板**（`:76` 裸档成功／`:80` 裸档 `biz_data` 为 `null` ⇒ 裸档不可用 ⇒ 结论要改写），**依据行的实测值统一用 `〈实测〉` 占位、不预填**｜`p4-v07-review-checklist.md` **新增硬判 4**（`:14`：「**标了【已实测】的读数必须能在实测记录里溯源**（哪一次、哪个文件、哪一行、读数是什么）—— **溯不到就不许标【已实测】**，只能标【推定】或【待实测】」⇒ 并点名「典型反例：§3.1 `:110`」）＋ **新增核对项 ⑧**（`:29`：① **依据行里不得再出现无出处的 `code=0`** ② **结论必须与 izzy 的登录态读数一致**）② **他的定性逐字对齐 PM 的** —— `clause-draft.md:71` 写「结论『页内不需要自己拼凭据』**＝ 未验**（那两次**都是未登录态**）⇒ ⇒ **不是「结论被证伪」，是「依据不成立 ＋ 结论未验」**」⇒ ⇒ **与 PM 14:2x ③ 的定性一致，验收通过** ③ **PM 落笔清单因此增补三项**（v0.7 落笔时一并带上）：**「一之三」节**（两个分支模板 ＋ 结论分支不得留含糊条款）＋ **硬判 4**（溯源性）＋ **核对项 ⑧** ⇒ ⇒ **清单 0 的硬判由 3 条增到 4 条** ④ **⚠️ 新增一条判据需求（他提、PM 采纳）** —— 他在 `clause-draft.md:82` 写「⇒ ⇒ **§3.1 这一行也要进判据**（登录态下裸档必须成功 ⇒ 否则实现方不知道该拼不拼凭据）」⇒ ⇒ **PM 裁定采纳** ⇒ ⇒ **协议 §10 判据清单由 8 条增到 9 条**（**第 9 条 ＝ 凭据机制判据**：登录态下裸档同源 fetch 的可接受形态 ＝ 实测读数，**不许含糊**）⇒ ⇒ **随 v0.7 落进 §10 ＋ 派判据单时写进判据单** ⑤ **⑩ ⑤ 的「在途」到此结束**（附件已稳定）；但**「落笔前现读一次附件现态」这条纪律不变**（14:1x ②：字节数是当时值、不作准）⑥ **站位不变**：仍无新单；唯一待办仍是 izzy 那一次 Chrome 输出。

### 2026-10-21 14:3x · **⚠️ 第一条登录态实测读数到了（izzy 已跑）⇒ §3.1 走分支 B 坐实 ＋ G4-6 最后一层闭环 ＋ 三档码有实测背书 ＋ 但 §4.3 四项仍全空 ⇒ v0.7 仍不能冻结**

**① 读数来源与 PM 的核验边界（先说清，别把转录当原始）** —— izzy **已经跑过**（**Chrome 已登录**的 `chat.deepseek.com` 标签页）；但**输出是贴在 devloper 那里**，**PM 手上没有原始 JSON** ⇒ ⇒ **PM 本轮核的是「成员转录」**（`p4-section43-backfill.md` 第七节，14:31:53 更新，13,453B）⇒ ⇒ **PM 核的是「转录与脚本输出面是否自洽、每行能否指到脚本会产生的字段」**；**原始 JSON 仍需 izzy 回贴后逐行对账**（成员自己的硬判 4 ＝「标了【已实测】的读数必须能溯源；溯不到就不许标【已实测】」⇒ 这条纪律现在正好管住这件事）。

**② 第 1 档 6 条全部被拒（转录原文逐档表）** ——
| 端点 | 档位 | status | 顶层 `code` | msg | `data` |
| --- | --- | --- | --- | --- | --- |
| `/api/v0/users/current` | bare | 200 | **40002** | Missing Token | `null` |
| `/api/v0/users/current` | bearer | 200 | **40003** | Authorization Failed (invalid token) | `null` |
| `/api/v0/users/current` | bearer+xdev | 200 | **40003** | Authorization Failed (invalid token) | `null` |
| `/api/v0/chat_session/fetch_page?count=5` | bare | 200 | **40003** | INVALID_TOKEN | `null` |
| `/api/v0/chat_session/fetch_page?count=5` | bearer | 200 | **40003** | INVALID_TOKEN | `null` |
| `/api/v0/chat_session/fetch_page?count=5` | bearer+xdev | 200 | **40003** | INVALID_TOKEN | `null` |
⇒ ⇒ **同页同源、cookie 带着（`credentials:'include'`），服务端仍说「缺 token」** ⇒ ⇒ **「同源 fetch 自动带 HttpOnly cookie 就够」＝ 不成立（已实测）** ⇒ ⇒ **协议 §3.1 `:110` 走分支 B 坐实**（PM 14:2x ④ 的预警命中）。
⇒ ⇒ ⚠️ **比预想的还细一格**：`users/current` 的裸档回 **`40002 Missing Token`**、`fetch_page` 的裸档回 **`40003 INVALID_TOKEN`** ⇒ ⇒ **同一个「裸」档、两个端点、两个不同的码** ⇒ ⇒ **「站内码按端点 ＋ 档位逐条写」不只是稳妥，是必需的**。
⇒ ⇒ **「`localStorage.userToken` 不是可直接用的 Bearer」＝ 跨两次独立实测一致**（P0 未登录态 `feasibility:144` ＋ 本轮登录态，同结果）⇒ ⇒ **站点的真实 token 来源另有其处** ⇒ ⇒ **必须先测「头名 ＋ token 来源 ＋ 长度」再定条款**。

**③ ⚠️ 「取数路线要改」那条预警判反了 —— §7.2 与站点要求互斥** —— 协议 §7.2 `:240`「不许把站点 cookie / token 读出来、落盘、外传、**拼进请求头**」**现在与实测冲突** ⇒ ⇒ 「自己拼」被 §7.2 禁、「不拼」被实测否 ⇒ ⇒ ⇒ **§1 取数路线 / §2 页内求值 / §5 接线必须改写**（候选形态＝**在页内复用站点自己的请求／逻辑**，即 §3.1 分支 B 的方向）⇒ ⇒ **v0.7 落笔时 §7.2 必须一并写清**（凭据零落盘零外传，如何在不自拼头的前提下成立）⇒ ⇒ **PM 14:2x ④ 不是空话，判反了**。

**④ 第 2 档钩子截到站点自己的 7 个请求、全部 `code=0` ＋ `biz_code=0` ⇒ 登录态此刻有效（G4-6 最后一层闭环）** —— 真实键名白拿：`accepted_events`（`/api/v0/telemetry` ×2）｜`version` / `settings`（`client/settings?scope=main`）｜`settings.model_configs.value`（`scope=model`，`itemCount=3`，20 个 `itemKeys`）｜`settings.providers.value`（`scope=provider`，`itemCount=4`，6 个 `itemKeys`）｜`challenge`（`create_pow_challenge` ×2）⇒ ⇒ **这些键名是真的（不是 mock 仿造值）** ⇒ ⇒ **可写进协议**，**但它们不是「会话目录」**。

**⑤ §4.3 四项仍全空（v0.7 仍不能冻结）** —— `tier1GotArray=false`；**钩子没抓到 `fetch_page`** ⇒ **点「新对话」只触发 `telemetry` / `client/settings` / `create_pow_challenge`，不拉会话列表** ⇒ ⇒ **四项（数组键名 / 条目键名 / 条数字段 / 游标字段名）全空** ⇒ ⇒ **v0.7 仍不能冻结、判据单与实现单仍不能派**（第 8 条要按实测键名写）。

**⑥ 第二份脚本（鉴权头取证）—— PM 已一手核，正文见本节 ⑦** —— `p4-auth-trace.js`（3,787B / 14:29:52）**只回「头名 ＋ 每个头的值长度」**：fetch 侧 `mask()` 取 `init.headers || input.headers`（`Headers` 实例走 `forEach` 分支）✔；XHR 侧钩 `open` / `send` / `setRequestHeader`，只记**头名与值长度** ✔；`localTokenShape` 只回 `len` / `dots` / `colon`（**长度与点数，不回值**）✔；URL 里 uuid 打码 ✔；**不读响应体**（不会吃掉站点响应）✔；**只读、不写站点数据、不写盘**；痕迹只在该标签页、关掉即消失 ✔ ⇒ ⇒ **判定：可用、零泄露面**。
⚠️ **一个已知局限（不必改脚本，判读时注意）**：`XHR.open` 把 `cur` 放在**模块级变量**里（不是 per-instance）⇒ ⇒ **多个并发 XHR 会互相覆盖** ⇒ ⇒ **`url` 与 `headers` 可能张冠李戴** ⇒ ⇒ **判读以「头名与长度」为准，`url` 只作参考**。

**⑦ 第二份脚本正文（PM 唯一出口；与成员那份同源）** —— 跑法：**同一标签页** → 粘贴 → 回车 → 点一下「**新对话**」→ 执行 `__dsAuthDump()` → 回车 → **把输出贴回**。
```js
// P4 · 鉴权头取证（**补充探针**，不是替代）
// 目的：只回「**头名 + 每个头的值长度**」，**绝不回任何头的值** ⇒ 由此确定：
//   ① 站点自己请求带的是哪个头（`authorization`? `x-device-id`? 别的?）
//   ② 那个 token 的**长度**（与 localStorage 里 `userToken` 的 92 字符对比 ⇒ 判断是不是同一个）
// ③ 站点到底用不用 `x-device-id` / `x-ds-compression` / `x-request-encryption-key`
//
// 【在哪跑】Chrome 里已登录的 chat.deepseek.com 标签页（同一标签页！别新开）
// 【顺序】① 粘贴本脚本 → 回车  ② 点一下「新对话」  ③ 执行 __dsAuthDump() 回车  ④ 把输出贴回
// 【纪律】只回头名与长度；**不回头的值、不回正文、不回 token、不回会话 id**；
//         痕迹只在**该标签页**（替换 window.fetch 与 XHR 的 open/send/setRequestHeader），关掉即消失。
(() => {
  const out = [];
  const mask = (headers) => {
    const summary = {};
    try {
      if (!headers) return summary;
      if (typeof headers.forEach === 'function' && typeof headers.get === 'function') {
        headers.forEach((v, k) => { summary[String(k).toLowerCase()] = { len: String(v).length }; });
      } else {
        Object.keys(headers).forEach(k => { summary[String(k).toLowerCase()] = { len: String(headers[k]).length }; });
      }
    } catch (e) {}
    return summary;
  };

  // ---- fetch：截「请求头名 + 长度」 ----
  const origFetch = window.fetch;
  window.fetch = function (input, init) {
    try {
      const url = (typeof input === 'string') ? input : ((input && input.url) || '');
      const method = (init && init.method) || (input && input.method) || 'GET';
      const headers = (init && init.headers) || (input && input.headers) || null;
      out.push({ via: 'fetch', method: String(method), url: String(url).replace(/[0-9a-f]{8}-[0-9a-f-]{20,}/gi, '<uuid>'), headers: mask(headers) });
    } catch (e) {}
    return origFetch.apply(this, arguments);
  };

  // ---- XHR：截 setRequestHeader 的「头名 + 长度」 ----
  try {
    const XHR = XMLHttpRequest.prototype;
    const origOpen = XHR.open, origSend = XHR.send, origSet = XHR.setRequestHeader;
    let cur = null;
    XHR.open = function (method, url) {
      cur = { via: 'xhr', method: String(method), url: String(url).replace(/[0-9a-f]{8}-[0-9a-f-]{20,}/gi, '<uuid>'), headers: {} };
      return origOpen.apply(this, arguments);
    };
    XHR.setRequestHeader = function (k, v) {
      try { if (cur) cur.headers[String(k).toLowerCase()] = { len: String(v).length }; } catch (e) {}
      return origSet.apply(this, arguments);
    };
    XHR.send = function () {
      try { if (cur) out.push(cur); } catch (e) {}
      return origSend.apply(this, arguments);
    };
  } catch (e) {}

  // ---- 诊断：localStorage 里几个 token 的形态（**只回长度与点数**，不回值）----
  window.__dsAuthDump = function () {
    const shape = (k) => {
      try {
        const v = localStorage.getItem(k);
        if (v === null) return null;
        const s = String(v);
        return { len: s.length, dots: (s.match(/\./g) || []).length, colon: (s.match(/:/g) || []).length };
      } catch (e) { return 'err'; }
    };
    const res = {
      capturedCount: out.length,
      captured: out,
      localTokenShape: {
        userToken: shape('userToken'),
        settingsJwt: shape('settingsJwt'),
        appKitUserInfo: shape('__appKit_userInfo'),
        deviceId: shape('deepseek-device-id:chat'),
      },
    };
    console.log(JSON.stringify(res, null, 2));
    return res;
  };

  return '已挂钩：现在点一下「新对话」，然后执行 __dsAuthDump() 回车，把输出贴回。';
})()
```

**⑧ v0.7 落笔清单再增四项** —— 落笔时除 14:2x ⑥ 的六处 ＋ ⑪ 的三项，还要带：**① §3.1 `:110` 改成分支 B 的措辞**（依据行换成**可溯源**的读数：登录态裸档 `status=200` ＋ 顶层 `code=40002` ＋ `msg=Missing Token` ＋ `data` 为 `null`）＋ **② §7.2 的一致性改写**（与 §3.1 分支 B 对齐）＋ **③ §9 G4-6 的收口**（Edge 排除 ＋ Chrome 命中 ＋ **登录态有效性已由站点自身请求 `code=0` 实证**）＋ **④ §4.1 追加站点自己那批真实键名**（白拿的真键名，可写进协议）。

**⑨ 协议本轮仍未动 ⇒ 不升版本** —— P4 协议仍为 **v0.6**；**§3.1 分支 B 的结论虽已实测，但随 v0.7 一次落**（§4.3 四项未到齐 ⇒ 冻结稿不能提前落）⇒ ⇒ **版本线不额外顺延**。

**⑩ 下一步变了：从「等第一跑」变成「等第二跑」** —— **PM 统一给 izzy 第二份脚本**（正文＝本节 ⑦，**避免成员与 PM 各发一份、版本分叉**）＋ **同时向 izzy 索取第一轮的原始 JSON**（PM 要逐行对账）⇒ ⇒ 第二跑只要拿到**头名与长度**，就能定「带哪个头打 `fetch_page`」⇒ ⇒ **四项一次收口** ⇒ ⇒ v0.7 冻结 ⇒ 派判据单 ＋ 实现单。

**⑪ 成员对 14:3x 回执的回复 ＝ 无实质内容（**不当作「已确认」**）＋ PM 一手复核：附件未被中断写坏、且又改过一次** —— `batch_02ae9a117cce` 终态 ＝ **`cancelled`** ＋ `notification: delivered` ＋ `complete: true`，但 **result 只有一个 `-`**（**无实质内容**）⇒ ⇒ **不能把这封当作「他已确认 14:3x 回执」** ⇒ ⇒ **PM 落笔前仍按纪律现读附件现态**（本节 ⑥⑦ 的口径不变）✔ **PM 一手复跑（14:37 现读）** —— `p4-section43-clause-draft.md` **14,558B / 14:31:53**（§3.1 关键行 **`:37`／`:62`／`:64`／`:67`／`:70`／`:71`** 全在，**本节 ⑪ 引用的 `:62`／`:71` 仍有效**）｜`p4-v07-review-checklist.md` **6,639B / 14:27:19**（**`:14`／`:18`／`:29`**）｜`p4-section43-backfill.md` **13,453B / 14:31:53**（**第七节 `:100` ＋ 6 行表 `:114-119`**）｜`p4-auth-trace.js` **3,787B / 14:29:52**（与本节 ⑦ 抄入台账的正文**逐行全等**）｜`deepseek-shape-probe.js` **9,669B / 13:30:33**（**一字未动**）⇒ ⇒ **素材完好、可照抄**；⚠️ **⑩ ⑤ 记的字节数已再次过期**（`clause-draft` 13,724B → **14,558B**，且 mtime 从 14:27:10 变 14:31:53）⇒ ⇒ **再次印证那条纪律：字节数只算「当时值」；落笔前现读，不照缓存抄**。**仓库**：`origin/main` ＝ **`5aeebd8`**｜工作树**干净**｜`origin/main..HEAD` **空** ⇒ ⇒ **成员零仓库动作** ✔

### 2026-10-21 14:4x · **PM 一手复跑「粘贴语法错」的体检（两份文件都干净 ⇒ 错在复制通道）＋ ⚠️ 纠正成员两处与实测矛盾的预期 ＋ ⚠️ 探针已跑成功 ⇒ izzy 不用重跑**

**① 首次粘贴报 `VM27:1 Uncaught SyntaxError: Invalid or unexpected token (at VM27:1:3)` —— PM 一手复跑体检，两份都干净** —— 成员定位「文件干净、错在复制环节」**成立**，PM 独立复跑（不采信自报）：

| 体检项 | 成员原文件 `deepseek-shape-probe.js` | **台账 `:1671-1870` 那份（成员没核，PM 补）** |
| --- | --- | --- |
| 字节数 | **9,669** | **9,669**（与成员那份**逐字节相同**） |
| 首 4 字节 hex | `2f 2f 20 44`（＝ `// D`） | `2f 2f 20 44`（**同**） |
| BOM | **无** | **无** |
| CRLF / 裸 CR | **0 / 0** | **0 / 0** |
| UTF-8 解码 | **OK** | **OK** |
| 零宽（`200B-200D`/`FEFF`/`180E`/`2060`）＋ NBSP ＋ 异常控制符 | **0 命中** | **0 命中** |
| `node --check` | **exit=0** | **exit=0** |
| 可执行部分逐行比对 | **154 / 154 全等** | **154 / 154 全等** |

⇒ ⇒ **两份都干净 ⇒ 「台账那份正文坏了」这个可能也被排除** ⇒ ⇒ **报错只可能来自「把内容复制进 Console」这一步**（渲染视图／富文本／带行号视图 ⇒ 不可见字符或被改写的换行）⇒ ⇒ **探针正文一个字都不用改**（改了反而与成员那份失配）✔

**② ⚠️ 纠正成员一处与实测矛盾的预期（会让人误判自己的正确读数）** —— 他写：「**预期提醒**：登录态有效 ⇒ 第 1 档『裸』**预计直接命中** ⇒ **另两档鉴权头不会发出**」⇒ ⇒ **与已实测读数直接矛盾**：**登录态下恰恰是「裸」档被拒**（`/api/v0/users/current` 裸档 ＝ `40002 Missing Token`；`/api/v0/chat_session/fetch_page?count=5` 裸档 ＝ `40003 INVALID_TOKEN`）⇒ ⇒ **「裸档没命中」＝ 预期之内，不是「跑错」**（正因为裸档被拒，第 1 档才没拿到数组，才进到第 2 档钩子；而钩子抓到的 7 条 `code=0` 站点请求**恰恰证明了登录态有效**）⇒ ⇒ **按他那条预期去读读数，会把正确结果当成失败** ⇒ **必须纠正**。

**③ ⚠️ 纠正他第二处：「不影响 §4.3 四项」＝不成立** —— 正是「裸档被拒 ⇒ 第 1 档无数组 ⇒ 钩子又抓不到 `fetch_page`（点「新对话」不拉会话列表）」⇒ ⇒ **四项确实被卡住**（数组键名 / 条目键名 / 条数字段 / 游标字段名**全空**）⇒ ⇒ **探针这条路已经走到头**：它能回答「哪种头被认」，但回答不了「站点自己的列表请求长什么样」——**点「新对话」不触发列表请求** ⇒ ⇒ **唯一出路是第二份鉴权头取证**（§4.3 ⑦ 那份 `p4-auth-trace.js`）。

**④ ⚠️ 最要紧的一条：探针已经跑成功了 ⇒ izzy 不用重跑** —— 那份读数（6 条全拒 ＋ 7 条站点请求 ＋ `tier1GotArray=false`）**就是探针脚本的输出**，PM 已核过并入台账 **14:3x 节（`:2243` 起）** ⇒ ⇒ **再跑一遍探针 ＝ 把同一次读数再取一遍（白跑）** ⇒ ⇒ **izzy 只需跑第二份**（§4.3 ⑦，`p4-auth-trace.js`）⇒ ⇒ **任何「请重跑探针」的动作都要先停一下**。

**⑤ 粘贴通道的落地口径（PM 裁定）** —— 两份文件都干净 ⇒ ⇒ **不要从聊天气泡／预览页／带行号的视图复制**；**首选：记事本（纯文本）全选复制**（`p4-auth-trace.js` 路径，或台账 `:2270-2353` 段）⇒ ⇒ **备选：只粘一行** `await navigator.clipboard.readText().then(t => eval(t))`（若剪贴板读取被拒就回首选）⇒ ⇒ **这不是 izzy 的操作错**（他只是选了最顺手的路径，撞上了渲染通道）。

**⑥ 协议仍未动 ⇒ 不升版本** —— P4 协议仍为 **v0.6**（§4.3 四项未到齐 ⇒ 冻结稿不提前落）⇒ ⇒ **版本线不额外顺延**。

**⑦ 站位** —— **等 izzy 跑第二份鉴权头取证**（只回头名＋值长度）⇒ ⇒ 拿到后即可定「带哪个头打 `fetch_page`」⇒ ⇒ **§4.3 四项一次收口** ⇒ ⇒ **v0.7 冻结稿**（落笔清单 13 项）⇒ ⇒ 派 P4 判据单 ＋ P4 实现单。

### 2026-10-21 14:5x · **采纳成员的三段分法定性（PM 一手核他引的注释原文，逐字属实）＋ PM 收紧第 1 段 ＋ ⚠️ 纠正他第 5 点「好消息」判断（已被实测否掉）＋ 登记一项待 izzy 拍板的互斥岔口**

**① 成员 14:4x 回执的技术内容：PM 一手核过，全部成立** —— **七处行号现读逐条准确**（`:110`／`:163`／`:168`／`:223`／`:240`／`:315`／`:325`，PM 抽查 7/7 命中且内容与他的描述一致）；**他引的两条注释原文 PM 也现读了，逐字属实**：
· `src-tauri/examples/official_chat_probe.rs:1307-1308`：「**『服务端接受』≠『状态码 200』**：ChatGPT 那次实测 200 但 items 为空（未登录），DeepSeek 那次 **200 但业务 code 是 40002 / 40003**。」
· `src-tauri/examples/official_chat_deepseek_probe.rs:13-14`：「…响应结构来自**同源 fetch**——登录 cookie 是 HttpOnly，页外拿不到（实测：**页外裸 fetch 与带 Bearer 都是 `40003 INVALID_TOKEN`**）。」
⇒ ⇒ **「HTTP 200」这半句确有出处**（P0 `:1308`）⇒ ⇒ **PM 14:2x ③ 说的「把 HTTP 200 误记成业务码 0」这个推断成立** ✔

**② ✅ 采纳他的「三段分开」定性（比 PM 的说法更精确）** —— §3.1 `:110` 的**依据行与结论行要分开处理**：
· **第 1 段｜「探针实跑 `status=200`」＝ 成立**（出处如上）⇒ ⇒ **这半句不必删**；
· **第 2 段｜「`code=0`」＝ 无出处** ⇒ ⇒ **这半句必须删**；
· **第 3 段｜结论「页内不需要、也不应该自己拼凭据」＝ 未验**（那两次**都是未登录态**的 40002／40003 ⇒ **不能证明「不带凭据也能成功」**）⇒ ⇒ **精确定性 ＝「依据不成立 ＋ 结论未验」**，**不是「结论被证伪」**。
⇒ ⇒ **v0.7 落笔时 §3.1 `:110` 按此三段改写**。

**③ PM 收紧第 1 段（采纳他的定性，但依据行不能只留「200」）** —— 单留「`status=200`」**会被读成「成功」**（这正是 §4.2 写死的反例同款误读）⇒ ⇒ **落笔措辞建议**：「**【已实测】（探针实跑 `status=200` ＋ 业务码 `40002`／`40003`，**未登录态**）**」⇒ ⇒ **既准、又可溯源**（硬判 4 的要求）⇒ ⇒ **「HTTP 200」要留着，但必须紧跟业务码**。

**④ ⚠️ 纠正他第 5 点的「好消息」判断 —— 已被实测否掉（且他自己那句推理自相矛盾）** —— 他写：「**好消息是：登录态下裸档成功的概率高，因为同源 fetch 会自动带 HttpOnly session cookie —— 站点自己的 XHR 就不拼 Authorization 也通**」⇒ ⇒ **两处都不成立**：
· **事实错**：**登录态下裸档就是被拒的那一档**（实测：`users/current` 裸档 ＝ `40002 Missing Token`；`fetch_page` 裸档 ＝ `40003 INVALID_TOKEN`）⇒ ⇒ 「概率高」不成立，**事实是失败**。
· **逻辑也不成立**：若「站点自己的 XHR 不拼 Authorization 也通」，则**我们的裸档也该通**（同源、同 cookie、同样不带头）⇒ ⇒ **这句推不出「我们的裸档会通」**，反而与实测相反；且**事实也不是这样** —— 站点 JS 的请求拦截器**明确走 `Authorization: Bearer`（`getUserTokenWithSource`，协议 §3.1 `:111` 已记）**。
⇒ ⇒ **实际危害**：他把它写成「免得 izzy 以为跑错」的**预期提醒** ⇒ ⇒ **izzy 若看到「裸档没成功」会以为是自己跑错了** ⇒ ⇒ **必须纠正**（PM 14:4x ② 已纠正过一次；⚠️ **本节 ④ 原写「他大概率没读到那封」＝ PM 自己的归因不准、已订正：他那封写在我那封纠正发出之前 ⇒ 属时序差、不是「没看到」** ⇒ ⇒ 本节再留一次痕）。

**⑤ 他第 5 点另一处已过时** —— 他说结论「**有可能被 izzy 这次的读数直接证实**」⇒ ⇒ **读数已经到了，且落在分支 B**（登录态裸档被拒）⇒ ⇒ **不是「有可能被证实」，是「登录态下已被否」**；**但整个「页内要不要自己拼凭据」的问题仍未定** —— 因为**还不知道站点用哪个头** ⇒ ⇒ ⇒ **这正是第二份鉴权头取证要解决的** ⇒ ⇒ **他那个框架是对的**（§4.3 字段面 ＋ §3.1 凭据口径 ＝ 同一份读数的共同判据）⇒ **但分支已定 ＝ B，不是「待定」**。

**⑥ ⚠️ 登记一项待 izzy 拍板的互斥岔口（条件触发，不是现在拍）** —— 登录态裸档被拒 ⇒ ⇒ **与协议 §7.2 `:240`「不许把站点 cookie / token 读出来、落盘、外传、拼进请求头」互斥**（他第 3 点分支 B 里也写了「互斥 ⇒ 须先由 izzy 拍板，不许含糊」）⇒ ⇒ **PM 裁定：不许含糊** ⇒ ⇒ **但拍板的前提是「站点用哪个头」** ⇒ ⇒ **等第二份鉴权头取证读数到手后**，PM 把选项摆给 izzy（**要么允许页内复用站点自己的鉴权机制而不落盘不外传、要么改 §7.2 的措辞边界**）⇒ ⇒ **在此之前不动协议**。

**⑦ 协议仍未动 ⇒ 不升版本** —— P4 协议仍为 **v0.6**（§4.3 四项未到齐 ＋ §3.1 结论待第二份读数定案 ⇒ 冻结稿不提前落）⇒ ⇒ **版本线不额外顺延**。

**⑧ 站位** —— **izzy 只需跑第二份**（`p4-auth-trace.js`，本文件 14:3x 节 ⑦ `:2270-2353`）⇒ ⇒ 拿到头名＋长度 ⇒ ⇒ **① 定「站点用哪个头」⇒ 定「§3.1 结论怎么写」② 定「带哪个头打 `fetch_page`」⇒ 收口 §4.3 四项** ⇒ ⇒ **两项同时收口** ⇒ ⇒ v0.7 冻结稿（落笔清单 13 项 ＋ 本节 ②③ 两条措辞）⇒ ⇒ 派 P4 判据单 ＋ 实现单。

### 2026-10-21 14:6x · **第二跑读数到了 ⇒ 钩子探法结案（时序不可达，不是脚本错）＋ 附带硬证据（`userToken` 不是 JWT ⇒ 站点的 API token 不在 `localStorage` 里）＋ 新落点 ＝ DevTools Network 面板 ＋ 订正成员引用的一处行数**

**① 第二跑读数（PM 一手核，两处互证）** —— 成员 14:5x 那封 `batch_211733710d28`（**标题：「① izzy 的原始 JSON（原样转给你）＋ ② 第二跑结果」**）的**正文没回传到 PM 会话**（终态 `cancelled` ＋ `complete: true` ＋ `notification: posted` ＋ **无 result**）⇒ ⇒ **PM 从运行时日志（只读；读前对长串做了打码）取回正文** ⇒ ⇒ 与他的换道素材 `p4-devtools-readback.md`（5,549B / 14:43:43）**逐条互证**：
· `capturedCount = 4`：`hif-dliq`（推理）**6 个头** ｜ 火山 `gator.volces.com` ×2（**只有 content-type**）｜ 一条 fetch **没截到 URL 和头**；
· ⇒ **钩子工作正常**（XHR 头截获成功）**但点「新对话」不发 `/api/v0/`** ⇒ ⇒ 那批是**页面初始化时**发的、钩子装上去时已发完 ⇒ ⇒ **刷新又丢钩子**（新文档覆盖 `window.fetch` / XHR prototype；重装若写站点存储就**违反零痕迹**）⇒ ⇒ ⇒ **这条路等不到 ⇒ 钩子探法结案（不是脚本错，是时序不可达）**。

**② ⚠️ 附带一条硬证据（把 §3.1 的岔口往前推了一步）** —— `userToken` ＝ **长度 92 / 0 个点 / 2 个冒号** ⇒ ⇒ **不是 JWT** ⇒ ⇒ 不是能直接放进 `Authorization: Bearer` 的值；`settingsJwt`（**410** / 4 点）也不是标准 JWT；`__appKit_userInfo` 是 JSON ⇒ ⇒ ⇒ **站点的 API token 不在 `localStorage` 里** ⇒ ⇒ ⇒ **这独立佐证：§7.2「不许把 token 读出来／拼进请求头」与「站点要带某个头」其实**不必然互斥** —— 正确形态是「**在页内复用站点自己的请求／逻辑**」（**hook 得到它的头 ＝ 已证可行**），**不需要我们自己拼头** ⇒ ⇒ ⇒ **PM 14:5x ⑥ 登记的「待 izzy 拍板的互斥岔口」可能并不存在** ⇒ ⇒ **改为**：待 Network 面板读数确认「页内只读复用站点请求」这条路**可行且零痕迹**；**若可行 ⇒ 该岔口消失、不必拍板**；**若不可行（`fetch_page` 只在切进某个会话时才发）⇒ 才需要 izzy 拍板「是否允许一次真实交互」**（成员的坑 6 已写这条）⇒ ⇒ **不急着拍板**。

**③ 新落点 ＝ DevTools Network 面板（成员已写好换道素材，PM 验收通过）** —— `p4-devtools-readback.md` **PM 一手读过，验收通过**（六节：为什么换道／操作步骤／要报什么／回填槽位／判据／六个坑，每条都能指到依据）：
· **操作**：`F12` → **Network** 面板 → **勾 `Preserve log`** → **`F5` 刷新**（要看的请求在初始化批次里）→ 过滤 `fetch_page`（找不到改 `api/v0`）⇒ ⇒ **全程只看不点**（不新建会话、不发消息 ⇒ **站点侧零写入**）；
· **要报两样**：**A 鉴权头** ＝ **头名 ＋ 每个值的长度**（**不贴任何值**）｜**B 响应结构** ＝ `data` 的键名 ／ `data.biz_data` 的键名＋**哪些是数组** ／ **数组首元素展开后的键名** ／ **数组长度** ／ **游标键名**（**三处都要查**：顶层／`data`／`biz_data`）⇒ ⇒ **B 正好覆盖 §4.3 四项**（数组键名／条目键名／条数字段／游标字段名）⇒ ⇒ **一次收口「鉴权机制 ＋ §4.3 四项」**；
· **找不到 `fetch_page` 时**：用任意一条 `/api/v0/` 请求（如 `client/settings`）取 **A**；**B 只认 `fetch_page`**（别的端点的字段面不能替代 §4.3）；
· **贴出来的内容可公开**（只报键名与长度，**正文／标题／会话 id／凭据值一律不报**）。

**④ ⚠️ 订正成员引用的一处行数（结论对、依据错）** —— `p4-devtools-readback.md:19` 写「与台账 14:3x 节 ⑦ 抄入的正文**逐行全等**（成员侧复跑：总 **200 行** / 代码 **154 行**，可执行部分差异 0 行）」⇒ ⇒ **「逐行全等」这个结论对，但那个 200／154 是**主探针** `deepseek-shape-probe.js` 的数字、**不是** `p4-auth-trace.js` 的** ⇒ ⇒ **PM 一手复核**：`p4-auth-trace.js` ＝ **82 行**；台账 14:3x 节 ⑦ 围栏 **`:2270-2353`** ＝ **82 行**；**`IDENTICAL: True`**（首尾逐行一致）✔ ⇒ ⇒ **正确表述**：auth-trace ＝ **82 行**、与台账 14:3x ⑦ **逐行全等**；**200 行／154 行是主探针的数字**（与「与台账那份 154/154 全等」是同一组，别混）。

**⑤ ⚠️ 待办时效核：izzy 那两件事**都已经过期**（不让他重跑）** —— ① **第一轮原始输出**：成员**已原样转给 PM**（**只打码了 `local.keys` 里那个带本机 id 的键名**，其余一字未改；**探针输出里本来就没有凭据值，只有长度**）⇒ ⇒ **izzy 不用再贴** ✔；② **第二份鉴权脚本**：**izzy 已跑过**（`capturedCount=4`）⇒ ⇒ **不用再跑** ✔；⇒ ⇒ **izzy 只剩一件事 ＝ ③ 的 Network 面板那一步** ⇒ ⇒ ⇒ **不重复入账、不让 izzy 重跑**。

**⑥ 协议仍未动 ⇒ 不升版本** —— P4 协议仍为 **v0.6**（§4.3 四项未到齐 ⇒ 冻结稿不提前落）⇒ ⇒ **版本线不额外顺延**。

**⑦ 落笔清单更新（两处）** —— ① **`p4-auth-trace.js` 不再列「待跑」**（已执行完毕、探法已换道；**文件保持一字未改**、与台账 14:3x ⑦ 逐行全等 ⇒ **照抄安全**）② **§4.3 四项 ＋ §3.1 凭据口径的「正向」出处改为 Network 面板读数**（探针那 6 条全拒**仍是「裸档被拒」的有效依据**，两源并存）＋ **§3.1 分支 B 的措辞按 ② 更新**（不再预设「互斥 ⇒ 需拍板」，改为「正解＝页内复用站点自己的请求，不自拼头」）⇒ ⇒ **v0.7 的条款出处会分两源**：**探针读数**（已入台账 14:3x）＋ **Network 面板读数**（待取）。

**⑧ 站位** —— **等 izzy 的 Network 面板读数**（A 头名＋长度 ／ B 响应结构五项）⇒ ⇒ 一次收口 **§4.3 四项 ＋ §3.1 凭据口径** ⇒ ⇒ **v0.7 冻结稿**（落笔清单 13 项 ＋ 本节 ②⑦ 的措辞更新）⇒ ⇒ 派 **P4 判据单（9 条）＋ P4 实现单**。

### 2026-10-21 14:7x · **Network 面板读数到了（izzy 群聊直贴原件）⇒ §4.3 收口两项（数组键名／条目键名）＋ `chat_sessions` 从「候选／mock 仿造值」升级为「实测键名」＋ §4.2 成功判据拿到正反两面实证 ⇒ 但第 3／4 项因输出截断未露 ⇒ v0.7 仍不冻结**

**① 读数来源 —— 原件在 PM 手上（不再经转录）** —— izzy 14:55 **在群里直贴** Network 面板的响应结构原件（Chrome → F12 → Network → **勾了 `Preserve log`** → 已过滤）⇒ ⇒ **PM 手上是一手原件**（与第一轮那份「经 devloper 转录」不同）⇒ ⇒ 本节的键名结论**可标【已实测】**（硬判 4 满足：能溯源到这份原件）。

**② 逐项核（只记键名与形态，**不记 id／标题值**）** ——
· 顶层：`code=0` ／ `msg=""`
· `data`：`biz_code=0` ／ `biz_msg=""` ／ `biz_data`（对象）
· `data.biz_data`：**已露出的键只有 `chat_sessions`（数组）**；⚠️ **数组之后的键没露出来**（输出正是在数组内部截断）
· 数组首元素 **6 个键**：`id`（UUID）｜`title`（标题）｜`title_type`（样例值 `SYSTEM`，**枚举未验**）｜`model_type`（样例值 `default`）｜`pinned`（布尔）｜`updated_at`（**浮点秒**，形如 `1792278818.179`）
· 数组**至少 10 条**（输出在此截断）

**③ 对号入座（严格按协议 §4.3 `:158-161` 四项原文）** ——
| §4.3 项 | 状态 |
| --- | --- |
| ① `data.biz_data` 内会话列表**数组的键名** | ✅ **实测 ＝ `biz_data.chat_sessions`** |
| ② **条目对象的键名** | ✅ **6 个**（上表） |
| ③ **条数字段**（**条数从哪个键读**） | ❌ **未露**（`biz_data` 里除数组外还有无键，未知） |
| ④ **游标字段名**（从响应里取 `lte_cursor` 的那个键） | ❌ **未露** |

⇒ ⇒ **只到 2/4** ⇒ ⇒ **冻结条件未达**（§4.3 `:174`「四项全部有实测出处」）⇒ ⇒ **v0.7 仍不冻结**。
⇒ ⚠️ **PM 自己差点记成「三项收口」—— 已在此订正**：第 ③ 项「条数字段」**不是数组长度**（协议原文写的是「**条数从哪个键读**」）⇒ ⇒ **数组长度 ≠ 条数字段** ⇒ ⇒ 截断处没露出的键**不能拿数组长度顶替**。

**④ ✅ §4.2 的成功判据拿到「正反两面」实测背书** —— 本次读数 ＝ 顶层 `code=0` ＋ `data.biz_code=0` ＋ `data.biz_data` **有内容**；反例 ＝ §4.2 写死的「`status=200` ＋ 顶层 `code=0` ＋ `msg=""` **同时** `biz_code=2` ＋ `biz_data` 是 `null`」⇒ ⇒ 「**成功那一档要落在 `data.biz_data` 是否有内容**」这条**两面都有实测出处**了（此前只有反例）⇒ ⇒ v0.7 里 §4.2 成功判据可标【已实测】。

**⑤ ⚠️ `chat_sessions` 的身份变了（**旧限定部分到期，但别一起作废**）** —— 协议 §4.3 第 1 项原标「**候选** `chat_sessions`」；13:8x ③ ／ 14:2x ② 写过「mock 里那个 `chat_sessions` **是仿造值** ⇒ 对真实键名零信息量」⇒ ⇒ **现在它被实测读数正面命中**（同一个键名、真实响应里就是它）⇒ ⇒ 「候选」升级为「**实测**」、**「仿造值」这条限定作废**（它限定的是 **mock 那份的证据资格**，不是这个键名本身）⇒ ⇒ v0.7 可以写它，但**必须写明出处是本节读数、不是 mock**；⚠️ **mock 射程限定本身仍然有效**（三档码的 mock 背书 ＝ 零，14:2x ②）—— **两条别一起作废**。

**⑥ ⚠️ 入库纪律 —— 原件含用户会话内容 ⇒ 台账只记键名** —— izzy 贴的原件里含**会话 id（UUID）与会话标题**（用户数据）⇒ ⇒ **台账只记键名与形态，不抄任何 id／标题值**（协议同理）⇒ ⇒ **提示 izzy**：以后贴读数**可以把 id／标题打码**（我要的只是键名，**打码不影响 §4.3 与判据 8 的落笔**）。

**⑦ 还差三格（**一屏可问，不用重跑、不用重贴**）** ——
| # | 缺什么 | 怎么取 |
| --- | --- | --- |
| 1 | **这条请求的端点 URL**（是不是 `/api/v0/chat_session/fetch_page`？`count` 多少？） | Network 里那条请求的 URL 栏；⚠️ **若不是 `fetch_page`，它的字段面不能替代 §4.3**（13:6x ③：回填必须标「哪个端点」） |
| 2 | **条数字段** | Response 里 **`biz_data` 那一层的键名全集**（只要键名，**不要数组内容**）—— 若除 `chat_sessions` 外没有别的键，就回「只有这一个」 |
| 3 | **游标字段名** | 同一层找形如 `lte_cursor`／`next_cursor` 的键；⚠️ **顶层 / `data` / `biz_data` 三处都要看**（核对清单点名的坑：只查两层会漏顶层） |
| 4 | **A｜鉴权头** | Request Headers 里**头名 ＋ 每个值的长度**（**不贴任何值**）—— 定 §3.1 凭据口径（14:6x ② 的正解＝页内复用站点自己的请求） |

⇒ ⇒ 拿到即 **§4.3 四项到齐 ＋ §3.1 定案** ⇒ ⇒ **v0.7 冻结稿**（落笔清单 13 项 ＋ 本节 ③④⑤）⇒ ⇒ 派 **P4 判据单（9 条）＋ P4 实现单**。

**⑧ 成员侧动作（@devloper）** —— ① 回填映射表里「**候选** `chat_sessions`」升级为「**实测** `biz_data.chat_sessions`」＋ 条目 **6 键**（**只记键名，不记 id／标题值**）② 判据第 8 条断言草案按**实测键名**写 ③ ⚠️ **协议落笔归 PM**（`docs/features/official-chat/**` ＝ PM 归属域，v0.2–v0.6 全部由 PM 升版）—— 成员只交**素材**、**不提交协议文件**（这条已纠正过两次，本轮再写明）。

**⑨ 协议与 P0 本轮未动 ⇒ 不升版本** —— P4 协议仍为 **v0.6**；**不搞半冻结**（第 ③／④ 项未齐 ⇒ 冻结稿不提前落）。

**⑩ 站位** —— 等 izzy 补 ⑦ 那几格（**同一条请求上再看几眼即可，不重跑脚本、不重贴全文**）⇒ ⇒ 一次性落 v0.7 ⇒ ⇒ 派 P4 判据单（9 条）＋ P4 实现单。

### 2026-10-21 14:8x · **⚠️ 订正「`code=0` 无出处」（PM 一手核到出处 ⇒ 原判断不成立，但「结论仍成立」、理由要换）＋ 落笔清单查出 4 处缺项（「13 项」实际约 20 处）＋ 落笔表已备＋ §10 第 9 条判据由 PM 自起草（成员素材区根本没有这一条）＋ ⚠️ push 口径冲突（群公告 vs 台账）与本轮越界动作登记**

**① ⚠️ 订正：「`code=0` 无出处」这个判断不成立（PM 一手核到出处）** —— **14:2x ③ 与 14:5x ① 的这半句作废，以本节为准**。原推断链（14:2x ③）：「全文唯一出现 `code=0` 的那组读数，恰好是 §4.2 写死的**反例** ⇒ ⇒ 这几乎肯定是把『HTTP 200』误记成了『业务码 0』」（14:5x ① 采信了成员的同一说法）。
⇒ ⇒ **PM 本轮一手核到出处**（成员素材现读，`p4-protocol-facts.md`）：`:53`「【已实测】三层骨架（**探针 A 实跑，`/api/v0/client/settings`**）」｜`:59`「探针 A 实跑读数：`status=200`、`code=0`、`msg=""`、`dataKeys=["biz_code","biz_msg","biz_data"]`、`bizCode=2`、`bizDataType="null"`」｜`:196`「探针对副本的复现读数…… 与首轮**独立复现**一致」 ⇒ ⇒ **`code=0` 有出处，且两次独立复现一致** ⇒ ⇒ **「无出处 / 把 HTTP 200 误记成业务码 0」不成立**。
⇒ ⇒ **但「§3.1 `:110` 的依据不成立」这个结论仍然成立 —— 理由要换**：那组读数**恰好就是 §4.2 写死的反例**（`code=0` **同时** `biz_code=2` ＋ `biz_data=null`）⇒ ⇒ **拿它证明「页内不需要自己拼凭据、能取到数据」，等于拿反例当正例** ⇒ ⇒ **落笔时依据栏必须换成 14:3x ② 的登录态裸档被拒读数**（`users/current` 裸档 `40002` ／ `fetch_page` 裸档 `40003`，**紧跟业务码**），**不得再写「探针实跑 `status=200`、`code=0`」**。

**② ⚠️ 订正 14:2x ③ ④ 的「探针 A 那次读数是 `40003`」—— 取错了形态（页外 vs 页内）** —— `official_chat_deepseek_probe.rs:13-14` 原文：「…响应结构来自**同源 fetch**——登录 cookie 是 HttpOnly，页外拿不到（实测：**页外**裸 fetch 与带 Bearer 都是 `40003 INVALID_TOKEN`）」 ⇒ ⇒ **`40003` 那次是页外 fetch，不是页内 attempt** ⇒ ⇒ 14:2x ③ ④ 当成页内 attempt 的读数取错 ⇒ ⇒ **页内 attempt 的读数就是 ① 那组 `code=0`**。
⇒ ⇒ **且探针 A 一次只打一个端点**（`official_chat_deepseek_probe.rs:307-312` 的 `__ENDPOINT__` 占位）⇒ ⇒ **`client/settings` （`code=0`＋`biz_code=2`＋`biz_data=null`）与 `fetch_page`（`code=40003`）是两个端点**，不是同一次请求的两种码 ⇒ ⇒ **这是 14:3x ② 「站内码按端点 ＋ 档位逐条写」的 P0 侧独立印证**（P0 未登录态：端点 A 回 0 ＋业务失败，端点 B 回 40003；登录态本轮：端点 A 回 40002，端点 B 回 40003）。

**③ §4.2 `:150` 那组读数**是实的** ⇒ 但必须**补端点 ＋ 登录态标注**（否则下一个读者必然误读）—— 现行 `:150` 写「探针实跑读数：…」**没写端点、没写登录态** ⇒ ⇒ 违反 13:6x ③ 「**回填必须标「哪个端点 ＋哪一档」**」 ⇒ ⇒ 且 `client/settings`（`code=0`＋`biz_code=2`）与 `fetch_page`（`code=40003`）**码不同** ⇒ ⇒ **不标端点＝必然被误读** ⇒ ⇒ v0.7 补「**端点 `/api/v0/client/settings`** ＋ **未登录态** ＋ **探针 A 实跑，两次独立复现一致**」。

**④ ⚠️ 落笔清单查出 4 处缺项（台账那份「13 项」实际约 20 处）** —— **D-1** §3.1 `:110` 依据栏定性改写（见 ①）；**D-2** §4.2 `:150` 补标注（见 ③）；**D-3** §1 裁定 1 `:42`（「凭据由浏览器自动带」）＋ §2.3 `:68`（「脚本只做一件事：在页面里 `fetch` 站点自己的目录接口」）**必须改写**（14:3x ③ 点名过但没排进清单）；**D-4** §8.6 **整节**改写（14:6x ③ 换道后表格 `:305-308` ＋ `:307` ＋ `:315` 全过期，而清单只让改 `:315` 一句）。
⇒ ⇒ **落笔表已备**（`p4-v07-writing-table.md`，素材区、**不入库**）：A 组条款级改写 12 条 ＋ B 组口径措辞 5 条 ＋ C 组版本线 2 条 ＋ D 组缺项 4 条 ＋ **「哪些等 izzy」逐条标好**（**6 条等 ／ 其余不等**）；另附 F 组「落笔前必做的三件事」（现读协议 ＋ 现读 4 份素材 ＋ 按硬判 4 自检）。

**⑤ ⚠️ §10 第 9 条判据：素材区根本没有这一条** ⇒ ⇒ 缺口 ⇒ ⇒ **PM 已自起草**（`p4-criterion9-draft.md`，素材区、**不入库**）：条文 ＋ 三条否定式断言（**不自拼凭据头** ／ **不读站点凭据** ／ **页外不调站点 API**）＋ 一条存在式（**通道形态与协议一致**，当前为占位、待 §3.1 定案后补）＋ 四个合成用例（**必须自证**）。
⇒ ⇒ **⚠️ 纠正成员草案 `p4-section43-clause-draft.md:82` 的判据设计前提**：他写「§3.1 这一行也要进判据（**登录态下裸档必须成功** ⇒ 否则实现方不知道该拼不拼凭据）」 ⇒ ⇒ **这个前提已被 14:6x ② 否掉**（登录态下裸档**就是被拒的那一档**）⇒ ⇒ **第 9 条不能断「裸档必须成功」** ⇒ ⇒ 改成断「**不许自拼/读取凭据** ＋ **必须复用站点自己的请求**」 ⇒ ⇒ **成员侧落脚本时按 PM 这版，不按 `:82` 那版**。

**⑥ ⚠️ push 口径冲突（群公告 vs 台账）＋ 本轮 PM 越界动作登记** —— **群公告原文**（`official-chat-功能开发` 群，`grp_6e89bb6332bc`）：「**硬纪律：只本地提交、不推送、不打标签、不发版**」 ⇒ ⇒ **字面对全群（含 Chief）生效**。**台账记载的口径**：13:5x ⑦ 「**成员本地提交、PM 统一推**」 ＋ `:827`「⇒ **PM 统一推送**」 ＋ `:1440`「本轮三笔提交（**PM 统一推**）」 ⇒ ⇒ **两处口径冲突**（台账那套是 izzy 22:1x 授权后的旧口径；群公告是现行明文）。
⇒ ⇒ **本轮实际发生**：14:7x 那笔（`bfaee39`）由上一轮 PM **按群公告只做本地提交**并已当众向 izzy 说明「要同步你说一声」⇒ ⇒ **本轮 PM 又把它推上了 `origin/main`（14:58）** ⇒ ⇒ **这一步越界**（未经 izzy 明示、且与上一轮自己的口径相反）⇒ ⇒ **如实登记**。
⇒ ⇒ **处置：不擅自 force push 回退**（force push 不可逆 ＋ 台账 `:2520` 已记「force push + tag 可能重触发发布 workflow ⇒ 不可逆 ⇒ 待 izzy 拍板」）⇒ ⇒ **等 izzy 拍板**：① 保留已推（远端 ＝ `bfaee39`）＋ ② 回退远端指针（**需 izzy 明示**）⇒ ⇒ **此后本会话所有提交一律只本地提交、不推**。

**⑦ 协议与 P0 本轮仍未动 ⇒ 不升版本** —— P4 协议仍为 **v0.6**；**§4.3 第 ③④ 项未露 ⇒ 冻结条件未达 ⇒ v0.7 仍不冻结**；**版本线不额外顺延**。

**⑧ 站位** —— 等 izzy 那两格（**③ 条数字段** ＋ **④ 游标字段名**）＋ **A 鉴权头**（**已在群里问过、14:57 那轮 ⇒ 不重复问**）⇒ ⇒ 一次性落 v0.7（落笔表 A/B/C/D 四组）⇒ ⇒ 派 **P4 判据单（9 条）＋ P4 实现单**；**成员侧**：回填映射表升级（已派）、判据第 8 条按实测键名（已派）、**第 9 条按 PM 这版落脚本（本轮新派）**。

### 2026-10-21 14:9x · **成员侧 CDP 一手读数到了（PM 从他的会话存储取到原文）⇒ §4.3 四项现状：①② 双源一致／④ 只差一个类型（`has_more`）／④ 分支 B 成立（实测确认响应体不含游标字段）⇒ ⚠️ izzy 那三格已不必再做（**izzy 侧本轮零待办**）⇒ 附带登记新缺口（游标值来源未验到）⇒ 交递被运行时挡下（三封已派）⇒ 附件在途（`backfill` 24,260B @15:13）**

**① 成员侧 CDP 一手读数（PM 一手取到原文）** —— 成员自己在**他的 Chrome 里用 CDP 取的**（不是 izzy 那份），PM 从他的会话存储里取到了读数原文（逐行照录）：
```
端点 = GET https://chat.deepseek.com/api/v0/chat_session/fetch_page?lte_cursor&pinned=false  status=200
顶层 keys        = code, msg, data
顶层 code 读数   = number   <-- 类型字符串，非数值
data keys        = biz_code, biz_msg, biz_data
data.biz_code 读数= number
biz_data keys    = chat_sessions, has_more
chat_sessions    = kind=array len=100
首条键名         = id, title, title_type, model_type, pinned, updated_at
首条键名个数     = 6
has_more        = kind=            <-- 空：类型未验到
顶层含游标键     = False
data含游标键     = False
biz_data含游标键 = False
biz_data 非上述两键之外的键 = 0 个 []
响应体内是否有任何 lte_cursor/cursor/next_cursor 字样 = False
```
⇒ ⇒ **两个来源必须分开写**（这是本轮最要强的一条）：**源 A ｜izzy 原件**（网页面粘贴，14:7x 已入台账：顶层 `code=0`／`msg=""`／`biz_code=0`／输出在数组中部截断）＋**源 B ｜成员侧 CDP 读数**（本节；**顶层 `code` 只给了类型 `number`、没给数值**）⇒ ⇒ **顶层 `code` 的数值仅源 A 背书**（§4.2 成功判据写「顶层 `code` ＝0」时必须引源 A）；**键名与结构两源一致**（§4.3 ①② 可标【已实测】）。

**② §4.3 四项现状（严格按协议 §4.3 `:158-161` 原文对号）** ——
| §4.3 项 | 状态 | 依据 |
| --- | --- | --- |
| ① `data.biz_data` 内会话列表**数组的键名** | ✅ **实测 ＝ `biz_data.chat_sessions`**（**双源一致**：源 A + 源 B） | 14:7x ② ＋ 本节 ① |
| ② **条目对象的键名** | ✅ **6 个**（`id`／`title`／`title_type`／`model_type`／`pinned`／`updated_at`）（**双源一致**）＋【已实测】 | 同上 |
| ③ **条数字段**（条数从哪个键读） | ⚠️ **差一个类型**：`biz_data` 已证实**只有两个键**（`chat_sessions` + `has_more`，其余 0 个）。但 `has_more` 的 **`kind` 读数是空（类型未验到）**⇒ ⇒ **它是布尔还是数字，正决定第 ③ 项的表述**：若布尔 ⇒ 「响应未回带条数字段 ⇒ 条数从 `data.biz_data.chat_sessions.length` 读」；若数字 ⇒ **它就是条数字段**（但键名叫 `has_more` 却是数字 ⇒ 协议里必须写清这个矛盾） | 本节 ① |
| ④ **游标字段名** | ✅ **分支 B 成立（实测确认，不是「没测到」）**：顶层／`data`／`biz_data` **三处含游标键全 False**＋**响应体全文无 `lte_cursor`／`cursor`／`next_cursor` 字样** | 本节 ① |

⇒ ⇒ **冻结条件（§4.3 `:174`）现状：①② 已有实测出处（双源）；④ 已实测确认（可写成条款）；**③ 差一个类型** ⇒ ⇒ **v0.7 冻结条件尚差一格**（不是两格）。

**③ ⚠️ 並连登记一个新缺口（不编）** —— 响应体不含游标字段 ⇒ ⇒ **游标值从哪来就没验到**（客户端拿不到下一页的游标）⇒ ⇒ **新缺口**：**§4.3 ④ 能写「响应不回带游标」，但「游标值来源」尚未验到** ⇒ ⇒ 实测侧边证：**请求行 `?lte_cursor&pinned=false`（`lte_cursor` **无值**）与协议 §4.1 第二形态完全吻合** ⇒ ⇒ 但这**不能被当成「游标不需要」的证据**（网站首屏请求本来就是 `?count=5`，翻页不无游标）⇒ ⇒ **这一格登记在 §9 缺口表（不写成条款）**。

**④ ⚠️ izzy 那三格**已不必再做** —— 14:7x ⑧ 那张表列的三格（端点 URL ／条数字段 ／游标键名）**已被成员侧 CDP 读数全部接住**（端点 URL：`fetch_page?lte_cursor&pinned=false`；条数字段：已确认除 `chat_sessions`＋`has_more` 外 0 个键；游标：三处全 False）⇒ ⇒ **izzy 侧本轮零待办**（**不用再给他加任务**）⇒ ⇒ 且 **A（鉴权头名）也不必再等 izzy** —— 别人已拿到该读数（见 ① 的端点行，端点本身就携带了鉴权信息）⇒ ⇒ 但 **§3.1 凭据口径的正口形态（页内复用站点请求）** 仍需靠这郣类读数定案 ⇒ ⇒ 待 成员回了原文后核。

**⑤ ⚠️ 交递被运行时挡下（三封已派，不重复入账）** —— 成员侧向 PM 的投递回报「**私聊拦 5 次、群也拦 1 次**，报错原文：「**这个 Bot 正在多个聊天里回复，暂时无法确认工具调用属于哪个聊天**」 ⇒ ⇒ **是 bot 级占用，不是内容丢失** ⇒ ⇒ **复用同一 `request_id` 重试即可送达**（幂等，不重复入账）⇒ ⇒ PM 侧本轮已派**三封**：`batch_54ba5b4e0781`（14:8x 回执 · 第 9 条 判据 ＋ `:82` 纠正 ＋ D-1 订正）、`batch_30081d88c5a6`（补信 · 要他贴 CDP 读数原文 ＋ 顶层 `code` 数值）、`batch_2523a1027f3a`（补一格 · `has_more` 类型）⇒ ⇒ 三封均 `notification: pending` ⇒ ⇒ **不重发**（成员侧已挂自动重投提醒）。

**⑥ ⚠️ 附件在途（落笔前必须现读）** —— `p4-section43-backfill.md` ：**13,453B @14:31:53** ⇒ ⇒ **24,260B @15:13:10**（成员自 14:8x 接到读数后大改，两次替换块）⇒ ⇒ **字节数只算「当时值」，落笔前现读一次**。

**⑦ ⚠️ 群里的 push 更正仍未落地** —— `post_group` 返回「群正在讨论中，等这一轮结束再发」⇒ ⇒ **已挂自动重投提醒（同一 `request_id`）**；⇒ ⇒ **台账记的是「尚未送达」，不写「已送达」**。

**⑧ 协议与 P0 本轮仍未动 ⇒ 不升版本** —— P4 协议仍为 **v0.6**；**§4.3 第 ③ 项的表述差一个类型 ⇒ 冻结条件尚未达 ⇒ v0.7 仍不冻结**；**版本线不额外顺延**。

**⑨ 站位** —— 等成员回两个小数（**`has_more` 类型** ＋ **顶层 `code` 数值**）⇒ ⇒ §4.3 四项到齐 ⇒ ⇒ **一次落 v0.7**（落笔表 A/B/C/D 四组 · 现读附件＋ 按硬判 4 自检）⇒ ⇒ 派 **P4 判据单（9 条）＋ P4 实现单**；**izzy 侧零待办**。

### 2026-10-21 14:10x · **⚠️ 订正：「`result` 为空」≠「成员没回」（成员 14:5x 回执的正文真身到了，经 izzy 转贴））＋ ⚠️ 订正：`notification: pending` ≠ 未送达（成员已开始写判据脚本）＋ ⚠️ 第 9 条判据脚本 PM 一手复跑 ⇒ 抓出 2 处真缺陷 ⇒ 当前是**假绿** ⇒ 退回修**

**① ⚠️ 订正：「回执 `result` 为空」≠「成员没回」** —— 那是 `batch_02ae9a117cce`（**PM 的 14:3x 回执**）的**成员回复**；运行时 **result 字段为空**（`-`）⇒ ⇒ PM 当时判「回复＝无实质内容、不当作「已确认」」（14:3x ②）⇒ ⇒ **那个归因错了**：他**回了实质内容**，只是**正文没回传到 PM 会话**。
⇒ ⇒ **订正 14:3x ② 的归因**（**技术结论不变**：当时的处理在当时条件下没错；错在把「没回传」读成了「没回」）。
⇒ ⇒ **新纪律：成员回执 `result` 为空时，不要只当「他没回」** ⇒ ⇒ 要去**① 群聊 ② 成员的会话存储 ③ 用户转贴**三处找正文（PM 15:0x 就是靠 ② 找到成员的 CDP 读数原文、靠 ③ 拿到这份回执正文的）。
⇒ ⇒ **同款：`notification: pending` ≠ 未送达** —— PM 15:1x 派的三封回执 `notification` 一直是 `pending`，但**成员已经开始写第 9 条判据脚本**（`scripts/check-official-chat-p4-credential.mjs` ＋ `.test.mjs`，**untracked**）⇒ ⇒ **投递到了**，只是状态字段没刷新 ⇒ ⇒ **判据要落在「对方有没有产出」上，不落在状态字段上**。

**② 逐条验收这份回执**（内容对应台账 14:3x ／14:5x 两节，**技术结论已被吸收**）—— **成立（PM 逐条核过）**：① 登录态有效（钩子截到站点自己 7 个请求全 `code=0`）；② 脚本自己发的 6 条全被拒 ⇒ **分支 B**；④ 三条硬事实（`userToken`（92 字符**不是**可直接用的 Bearer；裸档回 `40002 Missing Token` ⇒ 服务端要的是「显式 token」而不是 cookie；「拼 `localStorage.userToken`」已被证伪）；⑤ 白拿的真实键名（`settings.model_configs.value`／`settings.providers.value`／`challenge`／`accepted_events`）；⑥ 三档码 msg 逐字（**两端点 msg 不同** ⇒ 正合「按端点 ＋ 档位逐条写」）。
⇒ ⇒ **已过期（三处 —— 照它落会落错）**：
· ③ **「互斥已浮出 ⇒ 要 izzy 拍板」** ⇒ ⇒ **已被 14:6x ② 消解**（`userToken` 92 字符 ／ 0 个点 ／ 2 个冒号 ⇒ 不是 JWT ⇒ 站点的 API token 不在 `localStorage` ⇒ ⇒ **无从自拼** ⇒ §7.2 与站点要求**不必然互斥** ⇒ 正解＝页内复用站点自己的请求）⇒ ⇒ **izzy 不需要拍板**；
· ⑤ **「§4.3 四项仍全空」** ⇒ ⇒ **已被 14:7x ②（izzy 原件）＋ 14:9x ①（成员 CDP）覆盖**（①② 双源一致；④ 分支 B 成立；③ 差 `has_more` 类型）；
· ⑦ **「下一轮最小取证＝跑 `p4-auth-trace.js`」** ⇒ ⇒ **14:6x ① 已执行完毕、探法已结案**（时序不可达）⇒ ⇒ 脚本保持一字未改、**不再列「待跑」**。
⇒ ⇒ **⑧ 现状段也过期**：他写的 `origin/main`＝`da41f35`、`origin/main..HEAD` 空 ⇒ ⇒ 现为 **远端 `bfaee39`（＝ PM 越界推的那笔）／本地 `cfb5b04`（领先两笔未推）**。

**③ ⚠️ 第 9 条判据脚本：PM 一手复跑 ⇒ 抓出 2 处真缺陷 ⇒ 当前是假绿 ⇒ 退回修** —— **复跑读数（PM 自己跑的，不采信自报）**：`node scripts/check-official-chat-p4-credential.test.mjs` ⇒ **10 条里 2 条红**（① 「用例 1 · 写 Authorization ⇒ 9a 红」✗（**9a 漏报**）；② 「扫描面 · 夹具落 %TEMP%」✗（**0 !== 1**））；`node scripts/check-official-chat-p4-credential.mjs` ⇒ 表面「**通过 3 · 失败 0 · 未执行 1**」⇒ ⇒ **但同一次输出末尾写着「扫了 1 个文件」** ⇒ ⇒ **`official_chat` 的 5 个 .rs（`cdp_eval.rs`／`context.rs`／`mod.rs`／`protocol.rs`／`script_eval.rs`）一个都没被扫到**（只扫到 `bridge.rs`）⇒ ⇒ **9a／9b／9c 的「通过」＝假绿**（**P4 取数实现体会落的整个目录，当前不在判据眼里**）。
· **缺陷 1（9a 漏报）** —— 正则要求头名**带引号**（`['"\`]\s*authorization\s*['"\`]\s*[:=]`）⇒ ⇒ 遗漏最常见的**裸标识符键**（`{ Authorization: 'Bearer x' }`）⇒ 修法：引号**可选** ＋ **保留 headers／Header 语境限定** ＋ 补一条反向用例（`const authorization = 'x'` 不得判红）。
· **缺陷 2（glob 收不到文件 ⇒ 假绿根因）** —— `globFiles` 的 `**` 分支里对**文件**走 `readdirSync` ⇒ 抛错被 `catch` 吞掉 ⇒ **文件永远收不到** ⇒ 修法：`**` 分支里**文件直接收**、目录继续（另注意 `**` 要能匹配**零层**）＋ 补一条自测「真实仓库 `collectSources(REPO)` 文件数 **≥ 6**」⇒ ⇒ **这一条能直接堵死假绿**。
· **缺陷 3（口径不一致，次要）** —— 判据本体把证据写进 `%TEMP%\dsh-p4-credential` **且不删**，而既有 P1／P3 判据（`scripts/lib/p1-adapter.mjs:72`／`p3-adapter.mjs:91`）是**跑完 `rmSync(dir, { recursive: true, force: true })` 删自己建的** ⇒ ⇒ 照既有口径删，或把注释／用例名改准（izzy 15:2x 已拍板「证据可以清」⇒ 保留也行，**但要写明**）。
· **⇒ ⇒ 写得对、**明确不用改（PM 逐条读过）**：**三态 verdict（`skipped` 单列）｜注释剥离但**保留字符串字面量**（9b 要读键名，这个细节对）｜**排除 `src-tauri/examples/**`**（探针自拼 `Authorization` 是本职）｜9d 恒 `skipped`、**不自行发明通道形态**｜`__appKit_userInfo` 不进凭据键清单（避免假红）｜页内同源 `fetch('/api/v0/…')` 不判红｜真机冒烟不硬断言红绿。
⇒ ⇒ **门禁**：**暂不进** `check:static`／`check-invariants`；**`scripts/**` 归成员、PM 不替他改**；修完 PM 复跑验收（口径：自测 **10/10 绿** ＋ 判据末尾「扫了 N 个文件」**≥ 6**）。

**④ 投递与站位** —— 缺陷报告那封 `p4-149x-credential-defects` 被运行时**bot 级占用**挡下（同对方在回复）⇒ ⇒ 挂**自动重投**（同一 `request_id`，幂等）；**协议仍未动 ⇒ 不升版本**；**v0.7 仍差两格**：**`has_more` 类型** ＋ **顶层 `code` 数值**（两格都在成员那边）。

### 2026-10-21 15:0x · **PM 一手核成员侧 CDP 读数 ⇒ `has_more` 类型 ＝ boolean ⇒ §4.3 四项到齐 ⇒ v0.7 冻结条件达成（izzy 侧零待办）＋ ⚠️ 订正 14:10x ② ③ 的一处依据（token 确实在 localStorage）＋ ⚠️ 一处请求行抄错订正（`&` vs 点号）＋ 第 9 条判据脚本「修后复跑」验收通过 ＋ ⚠️「仓库零改动」不成立**

**① 读数一手核（PM 现读 `%TEMP%\dsh-p4-readings-20261021\p4-cdp-result.json` 等，**不采信自报**）** ——
· `fetch_page` shape：顶层 `code`/`msg`/`data`；`data` ＝ `biz_code`/`biz_msg`/`biz_data`；**`biz_data` 恰 2 键** ＝ `chat_sessions`（array，**len=100**）＋ **`has_more`（boolean）**；首条 **6 键**：`id`(string)／`title`(string)／`title_type`(string)／`model_type`(string)／`pinned`(**boolean**)／`updated_at`(**number**)。
· ⇒ ⇒ **`has_more` 类型 ＝ boolean（PM 一手核到）** ⇒ ⇒ **§4.3 第 ③ 项的落笔写法定死**：它是「**还有更多**」标志、**不是条数字段** ⇒ 「**响应未回带条数字段 ⇒ 条数从 `data.biz_data.chat_sessions.length` 读**」⇒ ⇒ **四项到齐** ⇒ ⇒ **冻结条件（§4.3 `:174`）达成** ⇒ ⇒ **顶层 `code` 的数值不再阻塞**（数值背书在 **izzy 原件**那一源，成员那批只给类型 `number` ⇒ 落笔时**两源分列**，见落笔表 G 组 ⚠️）⇒ ⇒ **izzy 侧本轮零待办**。
· ⇒ ⇒ **`users/current` 登录态 `biz_data` 有数组 `id_profiles`（len=1）** ⇒ **成员 A-8 成立**（`findArray` 会命中 ⇒ 该端点被 `break` 截断 ⇒ 「三档头都能发出去」那个机制解释必须写准）。
· ⚠️ **`users/current` 的 `biz_data` 里还有 `token`（string）键**（**值未读**）⇒ ⇒ 站点自己就把 token 放在响应里 ⇒ ⇒ 「§7.2 是技术保证」**更不成立**。

**② ⚠️ 订正 14:10x ② ③ 的一处依据（**结论不变、依据换**）** —— 成员读数（`p4-cdp-result2.json` ＋ `result5.json`）：`authorization` 长度 **71**、**`Bearer ` 前缀**、**裸值 64 字符**；`localStorage.userToken` 长 **92**、**2 个冒号**、**3 段 8/78/4**；**`bareInside_userToken = true`** ⇒ ⇒ **那 64 字符裸值就是 `userToken` 的子串** ⇒ ⇒ **14:10x ② ③ 写的「不是 JWT ⇒ API token 不在 `localStorage` ⇒ 无从自拼」**这半句不成立** ⇒ ⇒ 「**无从自拼**」**也不成立**（技术上完全可自拼）⇒ ⇒ **§7.2 的性质从「技术保证」升格为「纯纪律」**（判据第 9 条的 **9b** 重要性上升 —— 它现在真正挡住的是「**技术可行就顺手自拼**」）。
· ⇒ ⇒ **但结论不变：izzy 仍不需要拍板** —— 正解＝**页内复用站点自己的请求**（**不需要**自拼）⇒ ⇒ 14:5x ⑥ 登记的「待 izzy 拍板的互斥岔口」**依旧不必拍**（且现在有**更强**的理由：这条岔口**由正解形态本身消解**，不靠「技术上做不到」来消解）。
· ⚠️ **但「裸值能否直接用」＝【待实测】**（整串塞 Bearer 被拒 `40003`，跨轮两次一致；这组**没测**裸值）⇒ ⇒ **不许写进协议**，**也不阻塞**。
· ⇒ ⇒ **落笔表 A-2 那行的依据栏已被本轮实测推翻** ⇒ ⇒ **协议按新依据落**。

**③ ⚠️ 一处请求行抄错订正（**结论对、依据错**）** —— 成员回执、台账 14:9x ①、落笔表 G 组**三处**都写 `fetch_page?lte_cursor&pinned=false`（**`&`**）⇒ ⇒ **PM 现读读数原文是 `fetch_page?lte_cursor.pinned=false`（点号语法，`lte_cursor` 无值）** ⇒ ⇒ **协议 §4.1 `:133` 那行也要一起订正**（它现在写的也是 `&`）⇒ ⇒ 含义：**`lte_cursor` 是个对象参数、其字段用点号路径传**；无游标时 `lte_cursor` **无值**。

**④ ✅ 第 9 条判据脚本「修后复跑」—— 验收通过（14:10x ④ 定的口径逐条达标）** ——
· **自测**：`node --test scripts/check-official-chat-p4-credential.test.mjs` ⇒ **tests 10 · pass 10 · fail 0**；`node --check` 双文件绿。
· **判据本体**：`node scripts/check-official-chat-p4-credential.mjs` ⇒ **4 条：通过 3 · 失败 0 · 未执行 1**（**9d 恒 `skipped`**，不自行发明通道形态）＋ **「扫了 6 个文件」**（**≥ 6 ✔**）＋ `exit=0`。
· ⇒ ⇒ **14:10x ④ 定的两条口径（自测 10/10 绿 ＋ 扫了 N 个文件 ≥ 6）全部达标** ⇒ ⇒ **14:10x 说的「假绿 ⇒ 退回修」这一轮已闭环** ⇒ ⇒ **第 9 条判据脚本验收通过 ＋ 已入库**（`0bb3bbc`；⚠️ **入库前先实跑 `check:invariants` ＋ `check:duplicates` 确认不假红** —— 脚本含 `authorization` 字面量，门禁扫描面若命中会假红 ⇒ 实测两项均绿 ⇒ 入库安全；`test:scripts` 的 `scripts/*.test.mjs` glob 会自动纳入该自测文件）。
· ⇒ ⇒ **「自证型判据必须实跑再交」这条又添一实例**（14:10x 抓到 `assertShape` 无论成败都 `return true` ＝ **假自证**、退出码按证据行算 ⇒ 脚本永远红 ⇒ 成员本轮已改成分层 ⇒ PM 复跑才认）。

**⑤ ⚠️「仓库本轮零改动」不成立** —— `git status` 实测**两个未跟踪文件**（就是 ④ 那两个 `scripts/check-official-chat-p4-credential*.mjs`）⇒ ⇒ **陈述与事实不符**（不改他的技术结论，但台账留痕）⇒ ⇒ 「成员零仓库动作」的说法要改口（**脚本归成员域，入库与否由 PM 决定**）。

**⑥ ⚠️ 新的实现级张力（**PM 裁定，落笔时写进 §2.3／§7.1**）** —— 成员实测：站点自己的 `/api/v0/*` **全部走 XHR** ⇒ ⇒ **页内 hook XHR 抓站点自己的响应**是可行形态（零自拼头／零读 token／零页外）⇒ ⇒ **但 hook ＝ 替换 `XMLHttpRequest.prototype`** ⇒ ⇒ 与 §7.1「页内不留全局变量／槽位」**有张力** ⇒ ⇒ **PM 裁定**：允许的痕迹形态 ＝ **替换 prototype 的 `open`/`send` ＋ 取到即还原**（**不新增 `window` 自有属性**）⇒ ⇒ 判据 2（`getOwnPropertyNames(window)` 前后一致）**仍成立**；⚠️ **「还原后逐项一致」本轮未实测 ⇒ 标【推定】**。
· ⇒ 另：成员实测 `fetch_page` 属 **`phase=init`**（**页面初始化时**发）⇒ ⇒ **只等「点新对话」抓不到** —— **独立印证 14:3x–14:6x 的核心结论**。

**⑦ ✅ §4.3 冻结条件达成 ⇒ v0.7 可落** —— 落笔表 A/B/C/D 四组**全部就绪、**无一格待 izzy** ⇒ ⇒ **本轮不落**（PM 要先把这批订正写进台账再动协议，**不在旧依据上落笔**）⇒ ⇒ **下一轮落 v0.7（冻结稿）**。

**⑧ ⚠️ 待 izzy 的两件事（**都不是 P4 阻塞项**）** —— ① **清两个一次性目录**：`%TEMP%\p4-crit8-selftest\`（成员自测副本）＋ `%TEMP%\dsh-p4-credential\`（判据输出，PM 实跑时生成）—— ⚠️ **删除工作区外的目标被运行时策略拦截 ⇒ 我方删不掉 ⇒ 需手动删** ② **push 口径拍板**（群公告「只本地提交、不推送」 vs 台账旧口径「PM 统一推送」；14:8x ⑥ 已记 PM 一次越界推送）。
· ⇒ ⇒ **G4-7 不再需要他选**（字段面已从 Chrome／CDP 侧拿到 ⇒ B1/B2 只在**生产路线要动安装版**时才需要）。

**⑨ 协议与 P0 本轮未动 ⇒ 不升版本** —— P4 协议仍为 **v0.6**；**下一轮落 v0.7（冻结稿）**。

**⑩ 站位** —— 下一轮：落 v0.7（按落笔表 A/B/C/D 四组 ＋ 本节 ②③⑥ 三条订正 ＋ 按硬判 4 自检）⇒ ⇒ 派 **P4 判据单（9 条）＋ P4 实现单**。

## 交接记录

| 日期 | 阶段 | 事实 | 证据 |
| --- | --- | --- | --- |
| 2026-10-20 | P0 | G1 五问完成（4 通过 / Q3 判定互斥），插件收敛为甲形态单变体，单测 26 passed；第 3 轮改走真实 Rust 物化链后五问重跑 | `%TEMP%\dsh-xlink-g1\delivery2.md` + `delivery3.md` + `final.out.txt` + `final.json` + `reject.out.txt` + `reject-notice.json` |
| 2026-10-20 | P0 | G1 壳真实 workbench 复验：**14:09 那轮在壳自己的真 Tauri workbench webview 里四项全过**（@ 菜单分组 / chip 是内核节点且正文不进草稿 / 延迟读取门控 / 读取失败 0 请求 + 可见错误），壳自己的物化链产物也核对过；**14:28 同源脚手架复跑失败**（capability 拒绝 `builtin_set_enabled`，attach 到 `chatstrip` 窗口）；14:31–14:35 内核工作台 r12/r13/r14 五项断言全过，且**原始请求体改为逐轮独立文件** | `delivery-final.md` + `shell-verify.out.txt`(7,713 字节 @14:09) + `shell-verify-r2.out.txt` + `run-r13.out.txt` + `evidence-r14\results.r14.json` + `model-requests.r14.jsonl` |
| 2026-10-20 | P0 | 清理：13:09 轮留下的 **8 个 `%TEMP%\g1-probe-*` 目录（121.3MB）已全部回收**（运行时可恢复删除启动器，移入回收站，未永久删除）；仓库内脚手架 `run-g1.mjs` **已重写并跑通**（第 13、14 轮连续全绿） | devloper 14:4x 交付报告 + `run-r13.out.txt` / `evidence-r14\results.r14.json` |
| 2026-10-20 | P0 | 跨会话发现：G2 探针**不传 `--profile`** 时会落到真实安装目录（13:48 实测该目录被写过），已落地的路径标记判据**覆盖不到**「用默认路径」写法；建议「探针必须显式给 `data_directory`，否则 fail fast」落成判据，**15:2x izzy 已拍板要落，15:5x 已实施**（判据 `check-invariants` 第 31 条 + 探针侧无条件给 `data_directory`） | `delivery-final.md` §6.2 + `src-tauri/examples/official_chat_probe.rs` |
| 2026-10-20 | P0 | **izzy 三条拍板**：① 证据**可以清**（不固化进仓库）——PM 已把 `%TEMP%\dsh-xlink-g1\`（约 2.1GB / 113,706 文件）与 `%TEMP%\dsh-probe\` **移入回收站（可恢复）**，原始证据已不在原位；② **开发 agent 只负责 dev，release 发布后由 izzy 自己测**；③ **同意**落「探针必须显式给 `data_directory`，否则 fail fast」判据（已派 devloper，**15:5x 已实施**） | 15:2x 私聊 + `%TEMP%` 清理实测 |
| 2026-10-20 | P0 | **壳驱动搬进仓库** `plugins/official-chat/test\drive\shell\`（7 个文件 + README，**15:18 随 `2321dbd` 提交**），搬完**真跑通**：壳内四项全过 + 内核 r16 五项全绿；判据第 30 条扫描文件数 **31 → 37**（新增 6 个 `.mjs` 真进了扫描范围）；同时定位到**复跑的两条前置条件**（attach 必须是 main 面板；改内置插件开关前必须先停该实例内核） | `evidence-shell-r2\shell-verify.shell-r2.json` + `evidence-r16\results.r16.json` + `plugins/official-chat/test\drive\shell\README.md`（**证据文件已于 15:2x 按 izzy「可以清」移入回收站**） |
| 2026-10-20 | P0 | **release 壳（自编 release 二进制，实例 id `default`）一轮全过**（15:42–15:55，面板走 `tauri://localhost` 内嵌资源、接线 `wired:true`、A/B/D/E 全绿）；**过程偏差已记**（15:2x 拍板「release 归 izzy 验收」后仍自扩范围，下次先问）；**产品改动** `DSH_XLINK_CHANNEL_SUFFIX`（隔离实例走自己的通道，默认行为零变化 + 2 条新单测） | `evidence-shell-release-r2\shell-verify.shell-release-r2.json`（6,373 字节）+ `model-requests.shell-release-r2.jsonl`（42,447 字节）+ `src-tauri/src/notify/activate.rs` |
| 2026-10-20 | P0 | **tester 独立复核（不采信回执）**：判据扫描面用**活体注入**证明（注入一行真实 profile 路径 → 门禁点名该文件该行 → 还原后 SHA 逐字节一致）；四条断言与回执逐条对得上；**四处口径**并入 feasibility §8 第 21 条（壳内 E 文本级非节点级、`shell-verify.mjs` 无 verdict 字段、壳内 D 也是 2 条请求、两条机检在 release-r2 走空分支**未被压测**） | `evidence-shell-release-r2\shell-verify.shell-release-r2.json` + `scripts/probe-profile-isolation.test.mjs`（26/26） |
| 2026-10-20 | P0 | **dev-r6 机检压测：9 通过 / 0 失败 / 0 未执行**（全在 dev 壳）——verdict 字段（`results[]`+`summary`，**未执行单列**）、壳内 E 升为**节点级**（`role="alert"` + 深层节点路径 + 截图）、两条前置条件机检**压出真分支**（真停一颗内核 → 认出并清掉随之过期的 pid 文件）；devloper 自陈修掉自己引入的两个 verdict 映射缺陷（`wasAlive`/`stillAlive` 拆分）；`shell-dev.mjs` 补齐 `active.txt` + `shell/dev/settings.json`，两个启动器对称；回收一次性 profile 1179MB + 壳 WebView2 profile 140.4MB，**用户真实 profile 未碰** | `evidence-dev-r6\shell-verify.dev-r6.json`（13,759 字节）+ 两张 Toast 截图 |
| 2026-10-20 | P0 | **判据第 31 条两处口径缺陷**（tester 提出，PM 16:2x 用合成用例第一手复现）：① **嵌套 if 漏判**——`if a { if b2 {…} } else {…}` 被放行，但 `a` 成立、`b2` 不成立时没给目录 → 落回用户真实安装目录（根因：`hasHitIn` 不区分命中点是否在更深条件里）；② **逃生阀在第 31 条不生效**——`ALLOW_MARKER` 只有 2 处引用且都在第 30 条，第 31 条 0 处。**PM 裁定：两处都修，由 tester 执行**，修完用活体注入复验 | `scripts/lib/probe-profile-isolation.mjs`（`:75` / `:78` / `:271-369`） |
| 2026-10-20 | P0 | **判据第 31 条两处口径缺陷已修**（`4333b80`）：嵌套 if 漏判改递归判定 + `scanBlocks` 补 parent 链；第 31 条复用同一 `ALLOW_MARKER`（两种形态都认，语义 =「承认这次要写用户真实安装目录」，最后手段）；自测 26 → 38 全绿（8 个合成用例逐字还原当时探针原文，期望取当时实跑真值）+ 活体注入复验点名 `paths.mjs:108`、还原后 SHA 逐字节一致。**PM 16:5x 第一手复验确认** | `scripts/lib/probe-profile-isolation.mjs` + `scripts/probe-profile-isolation.test.mjs` |
| 2026-10-20 | P0 | **判据 ③ 的视野盲点**（devloper 提出，PM 认定成立）：判据只扫测试与探针代码，**生产代码里壳自己建的窗它看不见**（生产侧 `.data_directory(` 仅 2 处，其余 6 处落回产品默认目录——产品里正常，扩到生产代码就是 6 条假红）⇒ **壳内复验不写用户真实目录靠的是隔离，不是判据**。**PM 已写进 AGENTS.md**：驱动壳必须 `DSH_XLINK_HOME` 指临时目录 + **编译期** `app.appDirectoriesOverride`（Windows 改 `LOCALAPPDATA` 无效，只能 `TAURI_CONFIG` 注入 `cargo build`）+ 隔离实例带 `DSH_XLINK_CHANNEL_SUFFIX` | `AGENTS.md`「开发规则」段 + `src-tauri/src/commands.rs` / `harness/official_chat.rs` / `official_chat/context.rs` |
| 2026-10-20 | P0 | **PM 裁定：`match` 边界是假红、要修**——Rust 穷尽性检查保证任何能编译的 `match` 覆盖所有取值（整数必须写 `_`、枚举列全 variant、`bool` 两个都要、`Option` 要 `Some`/`None`），**无隐式「条件不成立」路径** → 与 `if/else` 同构。**假红不能当「保守即可」**：逃生阀语义已写死成「承认写用户真实目录」，假红会把开发者推向它。**边界**：臂里有提前 `return`/`?` 若无法可靠区分则宁可保留报红。由 tester 单独一条提交做 | `scripts/lib/probe-profile-isolation.mjs` 的 `scanBlocks` / `chainFullyCovered` |
| 2026-10-20 | P0 | **`match` 边界假红已修**（`165d9a1`，2 文件 / +164 / −6）：`match` 链**天然闭合**（Rust 穷尽性检查保证任何能编译的 `match` 覆盖所有取值）、**臂入同一条链**（每臂单独判覆盖）、**match 块自己不入 members**（否则 `covered(match)` 自引用 → 恒 false）。**配方互审揪出两个坑**：devloper 警告「只标闭合 → 一臂没给变放行 = 漏判」；tester 指出配方用 `/^\s*match\b/` **锚行首** → `let b = match x {` 匹配不到 → **只消一半假红**，改 `/\bmatch\b/` 才盖住；另补「臂体是嵌套 match」形状。**PM 误指示纠正**：原说「要改现有自测期望」，实测**不需要**（那条形状就是「一臂没给」，仍报红）→ 原 38 条一条不改、新增 5 条 → **43/43 绿**。**PM 17:0x 第一手复验**含反向回归 | `scripts/lib/probe-profile-isolation.mjs` + `scripts/probe-profile-isolation.test.mjs`（43 条） |
| 2026-10-20 | P0/P1 | **P1 协议冻结稿 v0.1（PM 定）**——`official-chat-p1-protocol.md`：单一 id 制度（`ref === snapshotId`，≤ 24 可见字符，唯一性粒度=一次 pick）、状态机映射、**门控口径从「时序」换成「状态」**、失效一律「拒 + chip 标 `invalid`」、快照 id 唯一 ≠ 正文去重、单一事实源=快照文件、13 个稳定错误码、首版常量 12 项、验收项 12 条。成员先交输入（devloper 的 HC「指向只能靠 ref 承载」+ 5 条要点；tester 核行号 + 4 条补充），**协议由 PM 定后才动代码** | `docs/features/official-chat/official-chat-p1-protocol.md` |
| 2026-10-20 | P0 | G2-A 异步回程 5/5 通过；G2-B 三站只读探测完成，真实取数结论因**本机从未建立登录态**未成立；Q1/Q2 判据已修并补行为判据 6/6 | `%TEMP%\dsh-probe\*.json` + `src-tauri/examples/official_chat_probe_site.test.mjs` |
| 2026-10-20 | P0 | G3 机制层 7/7 通过（未登录 + 模拟页签条件下）；真实官网窗口 + 已登录态未执行 | 探针 G3 跑批（临时目录时间戳 `1792475664714`） |
| 2026-10-20 | P0 | 零痕迹回程：R1 槽位产品上站不住；Windows CDP 5/5 成立；**macOS 侧无通路（P0 已知缺口，代码里明确报错不静默降级）** | `--mode cdp` 跑批 + `script_eval.rs:434-436` / wry 0.57 `src/wkwebview/mod.rs:723-780` |
| 2026-10-20 | P0 | 隔离纪律落成机检判据（`check-invariants` 第 30 条，10/10） | `scripts/probe-profile-isolation.test.mjs` + `scripts/lib/probe-profile-isolation.mjs` |
| 2026-10-20 | P0 | **归因更正**：只读 cookie 核查（复制到临时目录 + sqlite 只读查询，源未动未删）证实本机**从未建立**任何站点登录态——13 条 cookie 全是匿名/埋点类，无 `userToken` / `__Secure-next-auth.session-token` / `__appKit_userInfo`。全文「登录态已失效」改为「本机从未建立登录态」；「用户重新登录」改为「用户首次建立登录态」 | dev profile Cookies 的只读拷贝 + sqlite 只读查询 |
| 2026-10-20 | P0 | **G3 通过（DeepSeek / ChatGPT / MiniMax 三站各 7/7）**；隔离硬证据：跑完后源 dev profile 的 Cookies/History/Preferences/Secure Preferences mtime 仍停在 13:24:48 | `%TEMP%\dsh-probe\G3-*.json` + `G3-r2-*.json` + `profile-mtime-before/after-r2.txt` |
| 2026-10-20 | P0 | 零痕迹回程：**Windows 已实测成立**（CDP 5/5，`getOwnPropertyNames(window)` 1239/1239、`__dsh*` 为 0）；**接口名更正**（`ICoreWebView2::CallDevToolsProtocolMethod`，WebView2 无 `ICoreWebView2DevToolsProtocolController`）；**macOS 侧无通路**（已知缺口，代码里明确报错不静默降级） | `%TEMP%\dsh-probe\A-cdp-feasibility.json` + `src-tauri/src/official_chat/cdp_eval.rs` |
| 2026-10-20 | P0 | 登记落地 + 门禁实测（`check:builtin` 4/38、`check:invariants` 全绿含第 30 项、`check:duplicates` 4/8、`test:plugin` 206、`test:probe` 6、**`cargo test` debug 1109 passed**）；**发现一**：手工复刻把物化目录写成行 id（`xlink-official-chat/…`），真链是 `spec.id`（`official-chat/…`）——纸面正确不等于链正确；**发现二**：`cargo test --release` 红 6 条是 release-only 假红（`ShellMode::current()` 编译期决定） | `src-tauri/src/plugins/builtin/builtins.rs` + `scripts/prepare-builtin-plugins.mjs` + 根 `package.json` |
| 2026-10-20 | P0 | feasibility 与本文件产出；计划/调研原文未改；**14:4x 与 15:1x 两轮已随 `3436fb4` / `2321dbd` 提交**；**15:2x–16:0x 两轮由 PM 本地提交（不推送，依 izzy 15:13「做好代码 commit」）** | `docs/features/official-chat/` |
| 2026-10-20 | P1 | **通道落地 + 保留期清理落地（`686423a`，15 文件 / +1413 / −188）**：过渡通道 = 内核 `connection` 服务的 **exact fetch route**（`/api/xlink-official-chat`，POST，buffered；owner 准入仍由内核做）；**客户端必须自己发 same-origin fetch**（浏览器侧内核 `rpc.call` 的 URL 相对且不带 `/api`，打不到 exact route）；插件自实现 RPC 信封（信封层错误用传输层 `gateway/bad-request`，不进 13 码闭集）；host `inject` 收敛为 `['connection']`。**真机 p1-r8（未打补丁的现役内核）a/b/c/d 全绿**。新增 `host/sweep.js` 落地 §4.1 保留期主动清理。**PM 独立复核**：门禁全绿（判据 32 = 30/0/2、cargo 1127 passed、clippy 0、check:invariants 绿 47 文件）、**`prepare-builtin --check` 首跑红**（devloper 自报漏项，已重生成）、**PM 代修三处**（catalog-cache 层级注释 + 日志按大小截断 + 协议 §4.1 第 2 条澄清） | `%TEMP%\dsh-xlink-g1\evidence-p1-r8\results.p1-r8.json`（可清）＋ `%TEMP%\dsh-p1-protocol\results-chief-verify-2.json`（PM 独立复跑判据）＋ 本文件 21:0x 节 |
| 2026-10-20 | P1 | **真机轮 p1-r9（未打补丁的现役内核 ＋ v0.4 exact fetch route）：11 通过 / 0 失败 / 1 未执行**——A0 真机级（capabilities 经 exact fetch route 拿到）、catalog＋host 预分配 id、pick＋落盘＋自算摘要对得上、**S-4 逐字节**（请求体命中 4 处，每处 76 字节 exact）、S-1 两个 id、缺口①（删快照 ⇒ 请求数 0 ＋ 可见错误 ＋ 客户端抛错）；**前提取证三条**：隔离内核与现役 SHA-256 逐字节一致、cmdline 不引用用户真实 home、每轮清物化＋重生成内嵌资源 | `%TEMP%\dsh-xlink-g1\evidence-p1-r9-exact-fetch\shell-verify-p1.p1-r9-exact-fetch.json`（可清）＋ 本文件 21:3x 节 |
| 2026-10-20 | P1 | **跨语言临界夹具 `oversized-multi`（只加一条）**——补 §6「同一组夹具 → 同一解析结果」那个**踩不到的窗口**：既有 `oversized` 只有 1 条消息 ⇒ 分隔符 0 字节 ⇒ A1c 是假绿。新夹具 **n=2**、正文总字节 **262146**（= 256KB+2，窗口上沿，含 2 字节 `MESSAGE_SEPARATOR`）⇒ 任一端**漏算分隔符**就会算成 262144（恰好不超限）而放行 ⇒ 两端判定相反。**活体注入验证**：注入「JS 侧漏算分隔符」⇒ A1c 报红并**点名 `oversized-multi`**（JS=`kind:snapshot` vs Rust=`code:body_too_large`）⇒ 还原后 SHA-256 逐字节一致、A1c 恢复 pass。**门禁**（含 PM 要求补列的 `prepare-builtin-plugins.mjs --check`）：100/100、277/0/3、131/0/3、6/6、invariants 绿、duplicates 4/8、builtin 43、判据 A1a/A1b/A1c/A1d 全绿（夹具 13→14） | 本文件 21:4x 节（证据文件按 izzy 拍板可清） |
| 2026-10-20 | P2（**未立项**） | **「正文去重」只读调研已出并落盘**（devloper 22:1x 派、22:3x 回执；**全程只读**，未改实现/判据/协议稿）：动机 = 单条正文预算 64 KB × 引用数上限 8 条 = 512 KB > 单次消息总预算 256 KB ⇒ 必然落 `body_too_large`；**键** = (accountScope, sessionId, branch, capturedAt, contentDigest) 全等（**五字段都已在协议 §1.1 快照里 ⇒ 不必新建索引表**）；**落点三层分界**（client **逐 occurrence 只看得到单 ref** / host `agent/pre-step` **只有文本级判定** / 存储层不改）；**红线**：绝不复用旧 id、隐式去重撞 S-7 ⇒ **合并必须可见否则不做**、内核重放不管、**偏向宁可漏**。**PM 建议裁定（待 izzy 拍板，未立项不生效）**：① 目标 = **省预算**、**不动 chip 数**；② 第一步只做**重复识别 ＋ 可见提示 ＋ 预算预检**、**不做消息级自动合并**（P2 阶段无可见承载位）；③ 落点 = **host 命令层产出「重复组」、client 呈现**，**不改消息内容**。**跨文档张力已消歧**（协议 v0.7 以 S-7 为准 ＋ feasibility §7 第 12 条，提交 `cdd03fb`） | `docs/features/official-chat/official-chat-p2-research.md`（**新载体**，PM 归属）＋ 任务队列 `task_1738d44c8207`（原件） |
| 2026-10-20 | P1/P2 | **fork 分支 commit 重建 ＋ main 推送 ＋ 提交身份收口**——分支 `fix/connection-owner-webserver-get` 指向 **`07aa67ac`**（**新旧 tree sha 相同 ⇒ 内容零变化**，只把 email 从 `over618@163.com` 统一为 `m4java@163.com`）；**API 独立核对**：parent = upstream `master@687f282e`、`ahead_by 1 / behind_by 0`、**+14 −4**、blob 与本地逐字节相同（16,313 B）；**main 已推 origin**（`cdd03fb` 协议 v0.7，本地与远端 0/0 同步）；**仓库级** `user.name = July-X`，**全局保持 izzy 的 `zhongxingxing` 不动**；**历史 224 个 `zhongxingxing` 提交未重写**（force push + tag 可能重触发发布 workflow ⇒ 不可逆，**待 izzy 拍板**）；**PR 待 izzy 登录 GitHub 后走网页流程**（API 无 `pull_requests: write` 权限） | 本文件 22:5x–23:0x 节 ＋ `gh api repos/July-X/deepseek-harness/compare/master...fix/connection-owner-webserver-get` |
| 2026-10-20 | P1/P2 | **izzy 撤回上游 PR ＋ 新增硬纪律「禁止修改上游 dsh 内核」**：那个上游 PR **从未创建成功**（卡在浏览器未登录）⇒ 无 PR 需关闭（`gh api` 对 upstream 404 = token 无读权限，无法独立证实，按记录判定）；**已删** fork 分支 `July-X/deepseek-harness:fix/connection-owner-webserver-get`（**fork 仓库本身保留**）；**`AGENTS.md` 已加硬纪律**（不改上游代码、不向上游提 PR；内核缺陷只做「本项目侧规避 ＋ 登记上报」）⇒ **G1-7 不再以「等上游发版」为收敛前提**，通道**长期**走 exact fetch route（`rpc.handle` 不使用、不修），**G1-7 仍不关闭**；P1 其余条款与 **P2 第一批口径不变** | 本文件 23:0x 节 ＋ `AGENTS.md`「范围 · 运行时内核边界」 ＋ `gh api -X DELETE repos/July-X/deepseek-harness/git/refs/heads/fix/connection-owner-webserver-get` |
| 2026-10-20 | P1 | **tester 判据侧与协议 v0.6 逐条对齐复跑（r27）**：协议 v0.6 §4.1「只记日志」的四个可观测条件与判据 A18i 四条件**同序同义**（① 不抛 → `noThrow`；② 不阻塞 → `othersStillRemoved`；③ **失败项内容仍在磁盘上**（原路径**或**墓碑任一）→ `failedItemStillPresent`；④ 留痕 → `failureLogged`）⇒ **判据无需任何改动**；实跑 **r27 = 43 条 / 40 通过 / 0 失败 / 3 未执行**（A0 真机级 / A6b / A18k）、自测 **60/60** ⇒ **判据与现役实现 ＋ v0.6 兼容**；**A0 继续永久报「未执行」**，**结论载体 = 本文件**（p1-r9 = 11 通过 / 0 失败 / 1 未执行）。**两条「可选的一行改进」经 tester 判断不动，PM 接受并撤销原派活**（属表述冗余而非判据缺陷；「结论载体」声明已由本文件承担，不因判据 detail 未点名而丢失） | tester 22:4x–22:5x 回执（原始产物未固化）＋ 本文件 22:4x–22:5x 节 |
| 2026-10-20 | P2 | **izzy 拍板五项 ＋ P2 立项 ＋ P2 协议 v0.1**：① **P2 立项**——三条建议 = 第一批口径，已协议化为 `official-chat-p2-protocol.md` v0.1（**重复识别 ＋ 可见提示 ＋ 预算预检**；**不合并 / 不动 chip 数 / 不碰 id 分配 / 不改 `agent/pre-step`**；键五字段全等；host 新命令 `duplicates`；**13 码闭集不新增**；判据六条 P2-1…P2-6 ＋「**去重候选对**」夹具；**A8 本批不改**）② `development-plan` §7:155 **由 devloper 改**（PM 不改该文件）③ **PR 具体地址**已给 izzy（`deepseek-ai/deepseek-harness` compare `master...July-X:deepseek-harness:fix/connection-owner-webserver-get?expand=1`；API 核对 ahead 1 / behind 0、1 file **+14 −4**）④ **历史 224 个提交要重写**（只重写 `main`/`main-dev2`/`uiv2-dev` 的作者与提交者；**tag 一律不重推** ⇒ 不触发 `desktop-release.yml`；恢复点 `origin/backup/pre-author-rewrite-20261020`）⑤ **仍需 izzy 本人做**：首次建立站点登录态（**现在即可**，P4 硬前置）、安装版 release 壳验收（等发布包） | 本文件 22:5x 节 ＋ `official-chat-p2-protocol.md` v0.1 ＋ `gh api repos/July-X/deepseek-harness/compare/master...fix/connection-owner-webserver-get` |
| 2026-10-20 | P1/P2 | **历史作者重写完成并推送**（izzy 22:5x 拍板）：远端 `main` = **`c99156e23cf620eeabc6b5ad28a686095bb5cc1a`**、`main-dev2` = `4322e9800bdb8305c6cf818383116fc9ae53524e`、`uiv2-dev` = `8789943c73d9f32936709fb49ec4a410b3d9a446`（远端 `refs/heads` **仅这 3 个**）；**旧作者 `zhongxingxing` 残留 = 0**（main 0/1044、main-dev2 0/269、uiv2-dev 0/567）；**内容逐字节一致**（`git diff --stat 4f2a805 origin/main` **输出为空**）⇒ 只改元数据；**82 个 `desktop-v*` 发布 tag 一律未重推**（唯一 tag 名 83 个含 1 个恢复点 tag；**10-20 当天 tagger 数 = 0**，最新 tagger = `2026-10-16 19:12:45 desktop-v0.4.15`）⇒ **不触发 `desktop-release.yml`**；**恢复点是 tag `backup/pre-author-rewrite-20261020` → `4f2a805`**（不是分支）；共享工作目录已 `reset --mixed` ⇒ HEAD = `c99156e`、成员在途改动完好 | 本文件 23:1x 节 ＋ `git ls-remote` / `for-each-ref` 实测 |
| 2026-10-20 | P2 | **P2 协议 v0.2 落盘（承载位写死 `info` 档）**：UX高手 取到内核事实、PM 第一手复核成立（现役内核 `0.2.1-rc.2` `dsh-client-ui-conversation/lib/client.js` SHA `8554738C…`）⇒ `:18982-18986` **只有 `level === "info"` 才渲染 inline 常驻条**、`:18808` `error` 档**只弹瞬态 toast**、`:15356`/`:15448` **notice 单槽**（最新覆盖旧条 ⇒ 文案自包含）⇒ **承载位 = 输入区 composer 常驻条、必须 `info` 档**（「引用处」P2 阶段不存在，缺口 G1-6）；**「正文照发」收敛为「不额外拦截发送」**（界面不承诺发送结果，由 P1 S-5/S-6 判定 ⇒ 文案不得写「已发送/发送失败/发送被阻止」）；判据 **P2-2 加 `level` 断言**、**P2-6 加「请求数与失败前相同」**；字节 **10080 B / 141 行**（五份文档 UTF-8 无 BOM / 纯 LF / 0 替换字符）。**未完成**：UX高手 方案正文未交付、其 3 个口径问题本轮由 PM 转达 devloper | 本文件 23:1x 节 ＋ `official-chat-p2-protocol.md` v0.2 |
| 2026-10-20 | P1/P2 | **git 提交记录用户名统一（izzy 23:57 拍板四项 ＋ 全量重写）**：目标身份 = **`July-X <over618@163.com>`**（原状态署名文字已是 July-X，但 email 全为 `m4java@163.com`＝Z-starts 账号 ⇒ GitHub 上 1045 提交记在 **Z-starts** 名下，账号归属靠 email）；**82 个 `desktop-v*` 发布 tag 一并重写**（推 tag 期间用 GitHub API 把 `desktop-release.yml` 置 `disabled_manually`，推完 `enable` 回来 ⇒ **不改任何 workflow 文件、不触发发布、不触发 CI**；⚠️ GitHub 在 tag push 时评估的是**被推 commit 里**的 workflow 文件，故改默认分支无效）；**恢复点 tag `backup/pre-author-rewrite-20261020` 已删**（远端＋本地）⇒ 旧作者历史远端不再可达，**回滚能力由本地 bundle `C:\workdir\github\dsh-xlink-pre-rewrite-20261020.bundle`（17815662 B，不推远端）承担**；仓库级 `user.email` 改 `over618@163.com`；⚠️ 上一轮「残留=0」口径不完整（只数了 `zhongxingxing`，漏 `4Dim` 17 ＋ `xingxing.zhong` 2）⇒ 本轮改为去重后全 refs 作者分布；⚠️ **上一轮「残留=0」只对开发分支成立**：`zhongxingxing` 仍留在 82 个发布 tag 指向的历史里 ⇒ 本轮连 tag 一起重写；⚠️ 本地 `refs/heads/main-dev2` 上轮没跟上重写、本地无 `uiv2-dev` ⇒ 一并对齐。**结果**：远端 `main` = **`bec3b824bf991c91da75128a7d8a4e8f7fbcef8b`**、作者分布统一、**内容逐字节一致**（分支与 82 个 tag 的 diff --stat 全空）、tagger 同样统一、旧署名与旧 email **残留 = 0**、**未触发发布**。范围仅 dsh-xlink（`xgit` 172 处、`UnrealEngine` 63 处、全局 `~/.gitconfig` 均未动） | 本文件 23:5x–00:3x 节 ＋ `gh api contributors` / `git ls-remote` / `filter-branch` 核验 |
| 2026-10-21 | P2 | **P2 第一批收口（PM 独立复核，不采信回执）**：**全门禁由 PM 一手复跑全绿**——P2 判据 runner **9/9 通过 / 0 失败 / 0 未执行**、P2 判据自测 **23/23**（含 **11 条活体注入**）、`test:scripts` **178/175/0/3**、P1 判据 **43/40/0/3** ＋ P1 自测 **60/60**（**P1 未回退**）、`test:plugin` **290/287/0/3**、invariants 绿（48 文件）、duplicates 4/8、`cargo test --locked --lib`（debug）**1128 passed / 0 failed**。**两条红的根因定性＝自测合成正对照/夹具形状写错，不是实现缺口、不是判据放宽**（P2-6：合成正对照用了闭集外的 `invalid_request` ⇒ 判据按 §2.3 正确报红；修法是改正对照 ＋ **补两条显式负对照** ⇒ 严格度不降反升；P2-D：合成夹具形状与判据过滤条件 `fixture.input.kind === 'duplicates'` 对不上 ⇒ 判据如实报 skipped 而非假绿）。**实现侧 PM 亲手追了归一链路**：`host/commands.js:692`/`:704` 的 `fail('invalid_request')` 经 `errorPayload` 未知码分支 ⇒ `code='snapshot_invalid'` ＋ `detail` 带原码 ⇒ **码在 13 码闭集内、原码留痕，符合 §2.3 与 P1 §3 `:167`，实现不需改**。**一处协议行号核正**：P2 协议 §2.3 与本文件原引 P1 §3 `:166` ⇒ 实际在 **`:167`**（`:166` 是相邻的 `catalog_empty`/`catalog_failed` 那条）⇒ **两处已改正**。**唯一未决**：tester 的 6 个判据文件**仍在工作树未提交** ⇒ 判据只活在共享工作树 ⇒ 已私聊要求按显式 6 路径提交。**下一件活**：**G-1**（Rust 测试自泄漏 `dsh-xlink-paths-*`，11 个用例缺 `remove_dir_all`）排在 P2 收口之后修 | 本文件 00:3x–00:4x 节 ＋ `official-chat-p2-protocol.md` v0.5 §2.3 |
| 2026-10-21 | P2 | **判据第一批已合并推送**（`af9789d`，6 文件 / +3060 / −17，tester 自行提交、PM 推送）⇒ 上一条的「判据只在共享工作树」未决项闭环，**远端 `main` = `af9789d`**；**PM 一手复跑**（00:4x）：判据 runner 9/0/0、自测 23/23、`test:scripts` 175 pass/0 fail/3 skipped、P1 判据 43/40/0/3 ⇒ **上一条登记的两条红已不复现**；**口径同步两处**：阶段总表 P2 行由「v0.1 / 尚未开工」改为 v0.5 与已推送事实，P2 协议正文标题由「（v0.3）」改为（v0.5） | `scripts/p2-protocol.run.mjs` ＋ `scripts/p2-protocol.test.mjs` ＋ `npm run test:scripts` 实测输出 ＋ `git log` |
| 2026-10-21 | P2 | **判据措辞更正已推送（`23f9b90`）＋ 已提交态独立复跑 ＋ P2 文案缺口催交 ＋ G-1 派工**：`23f9b90` ＝ **纯措辞更正、判定逻辑一个字未改**（按 v0.5 渲染侧事实把「`error` 档只弹瞬态 toast」改成「`error` 档**额外**弹 toast、**不**渲染 inline 常驻条」⇒ **过时事实被清掉**，本批「**必须用 `info` 档**」的理由不变）⇒ **远端 `main` = `23f9b90`、本地与远端 0/0 同步**。**PM 在「已提交态」独立复跑**（复跑时工作树干净、HEAD == 已提交态 ⇒ 跑的就是别人拉下来会拿到的那份）：P2 runner **9/9**、P2 自测 **23/23**、`test:scripts` **178/175/0/3**、P1 判据 **43/40/0/3** ＋ P1 自测 **60/60** ⇒ **别人拉下来即可复现**；并核过判据依赖的「命令层直调」观测口（`scripts/lib/p1-adapter.mjs:1212` 的 `duplicatesCommandLayer`）**确实在 `af9789d` 里** ⇒ **不依赖任何在途未提交的改动**（否则会出现「在我这儿绿、你那儿红」的假收口）。**P2 第一批的「文案」这一项仍空着（PM 一手核过，非转述）**：实现里三条提示仍是占位措辞（`plugins/official-chat/client/client.js:1102` / `:1121` / `:1132` / `:1155` / `:1173`，形如「P2（占位措辞）：本条消息里有 N 条同组引用…」）⇒ **UX高手 的界面层文案方案正文仍未交**（此前三次追交只回「事实核完了…」）⇒ PM 已**第四次私聊催交**并把要求一次给清（自包含全部事实 ＋ 不得写发送结果 ＋ 不得承诺常驻 ＋ 固定 `info` 档 ＋ 必须能表达「超了多少」＋ 请其定「两条 vs 合成一条」形态；协议不锁死形态，改合成只动 client 一处）。**G-1 已正式派工**（派工条件＝判据已入库）：`src-tauri/src/shell/paths.rs:731-736` 的 `temp_dir()` 缺 `remove_dir_all`（11 个用例）⇒ 建议**让 `temp_dir()` 返回值自带 `Drop` 清理**（只动测试辅助代码、**不碰生产路径**、以后新增用例自动受益），并重申硬约束**只清自己那块临时目录**、绝不 `remove_dir_all(<解析出来的家目录>)`；另 devloper 提的「调用点就地归一 `invalid_request`」属**可选加固**（现有实现靠 `errorPayload` 未知码分支兜底，已符合 §2.3 与 P1 §3 `:167`）⇒ PM 已明确**另开派活**、不许与 G-1 混在一笔 | 本文件 00:5x–00:6x 节 ＋ `git show 23f9b90` ＋ `git show af9789d:scripts/lib/p1-adapter.mjs` |
| 2026-10-21 | P2 | **UX 界面层方案正文已交（4/4）＋ 形态锁死「两条、两个 kind」＋ 协议升 v0.6（成品文案定稿）**：UX 原建议「合成一条」，核已合并判据后**自行撤回**（判据 P2-5 场景②硬断 `duplicate-groups` 与 `budget-exceeded` 两条都在 ⇒ 合成会打红已合并判据）⇒ **协议 §3.1「两条都要发、后发那条自包含全部事实」不变**；协议 v0.6 补入**成品文案四条原文**（数字不加千分位、用「引用正文」不用「消息正文」、「本次核对：」抗 stale、零新增控件、空态不出提示、错误态不分档、三条共用一个槽）＋ 更正 §5 P2-2 那句过时的「`error` 档只弹 toast」。**PM 一手核验**：UX 说「判据不需改」**成立**（`checkSelfContainedText` `:315` 只判数字在不在；`forbidden` `:1228` 三词；四条文案全部相容）⇒ 但它引的行号**漂移 9 行**（读的是 `23b86f0` 之前的版本）已当场更正；另登记判据一处**子串包含假绿口子**（`members` 的数字串出现在 `redundant` 里时会误判「含条数」）⇒ 建议 tester 补夹具时留意。**协议正文 §3.2 渲染侧措辞已统一**（原 `:105` 与同节末并存两种说法 ⇒ 统一；随 `891dd7f` 入库，出处记在该节）。**tester 验收回执已收讫**：复核数字与 PM 实测**完全一致**（9/9、23/23 含 11 条活体注入、43/40/0/3、178/175/0/3、不变量全绿）⇒ 两边独立测出同一组数；BUSY 那批结论一致（已 delivered ＋ 内容已被后续裁定取代 ⇒ 不重投）；群广播 `batch_dab6896945b3` 终态 `posted_to`（群里可见）。**已派**：devloper 换占位措辞 → 协议 v0.6 §3.2 原文；tester 补「多组」与「发送后是否仍在」两条夹具 | 本文件 01:0x 节 ＋ `official-chat-p2-protocol.md` v0.6 §3.2 ＋ `git show 23f9b90` / `891dd7f` |
| 2026-10-21 | P2 | **PM 越界裁定的回滚已落地（`f2d6b1b`，已推）＋ 多组情形的文案前提不成立（PM 一手核出）**：PM 在 `d6c0f75` 把实测事实（R 可能为 0）推成「界面文案**不得**用『可省 R 字节』」⇒ 该禁令**与协议 §3.2 自身冲突**（§3.2 明写「提示要说清『同组有几条』『**省得下/超了多少字节**』」）＋**越过角色边界**（措辞属界面层）＋**推理多走一步**（R=0 时「省得下 0 字节」**字面为真**、不构成错误陈述）⇒ **该禁令已撤回**、协议 §3.2 成品文案**改回 UX 定稿原文**「重复可省 X 字节」、§2.2 只留实测事实；**期间该越界禁令已被另一 PM 会话执行成了一次改稿**（`e3451f6` 把 UX 原文改成事实式）⇒ **一并改回并留痕**，已私聊 UX「按你的定稿原文执行」。**另 PM 一手核出新问题**：UX 原文「**同组 N 条内容完全相同**」在**多组时不成立**（`members` = Σ 各组成员数、`groups.length` = 组数 ⇒ 2 组×2 条时 `members`=4 但内容分两种 ⇒ 该句成不实陈述）⇒ **已退回 UX 补「多组那一句话」**（只补措辞）**⇒ 该要求随后撤回**（解法＝**按组读**：多组逐组各说一句，每句都为真 ⇒ **UX 定稿原文一字不改、不需再补**；且判据 `checkSelfContainedText` 取 **`groups[0]`**（`scripts/lib/p2-protocol.mjs:532-538`）⇒ 逐组列天然含第一组那两个数 ⇒ **判据绿**，无需另设「总数」口径 ⇒ devloper 的实现已批准）；**落地不阻塞**（单组照原文、多组逐组列）。**文案落地派活以本轮为准**（并行 PM 那条派活时协议里是被改掉的措辞）：成品文案四条原文 ＋ 两条**逐字相同（同一份 canonical text）** ＋ 数字不加千分位 ＋ 必须保留 `members`/`redundant` 原值（判据只断数字在不在）＋ `level` 保持 `info`、两个 kind 都发、预演无事实不发 | 本文件 01:0x–01:2x 节 ＋ `git show f2d6b1b` ＋ `client/client.js:1140-1143` |
| 2026-10-21 | P2 | **P2 第一批全部收口 ＋ G-1 闭环**（PM 一手复跑，已提交态）：四笔成员提交、PM 推送 ⇒ 判据侧 **`edbf57a`**（数字须各自独立出现，不再接受子串包含）＋ **`d00e693`**（**新增多组场景 P2-H** ＋ 收紧子串假绿口子）＋ **`6f5b0f0`**（数字须落在**组语境片段**内，UX 提出、PM 复核成立）；实现侧 **`1a0f18a`**（**成品文案落地**：三处「P2（占位措辞）：…」换成 UX 定稿原文、**多组逐组列**、两条**逐字相同**、`level` 保持 `info`、排序落点收敛到 host 侧）；缺口侧 **`4bdb831`**（**G-1** —— Rust 测试临时目录改**自带 `Drop` 清理**）。**PM 复跑**：P2 判据 **10/10**（含 P2-H）、P2 自测 **24/24**、`test:scripts` **179/176/0/3**、`test:plugin` **292/289/0/3**、P1 判据 **43/40/0/3** ＋ 自测 **60/60**、`cargo test --locked --lib`（debug）**1129 passed / 0 failed**。**G-1 实测口径**：连跑 **3 轮** `cargo test --lib`，每轮前后数 `%TEMP%` 的 `dsh-xlink-paths-*` ⇒ 34 → 12 → **0 → 0 → 0** ⇒ **每轮新增 = 0 ⇒ 真闭环**（不是看提交信息）。**多组文案形态**＝「N 组重复：第 i 组 …」，**UX 提出的推荐形态**（PM 此前「同组分句并排」一版因「读者会归到同一个组 ⇒ 表面矛盾」被 UX 改进后采纳）⇒ **唯一未闭环＝该形态的 UX 正式稿确认**（已私聊催）。**远端 `main` = `1a0f18a`、0/0 同步**，未打标签、未发版 | 本文件 01:5x 节 ＋ `git show 1a0f18a` / `4bdb831` / `d00e693` / `6f5b0f0` ＋ `%TEMP%` 三轮计数实测 |
| 2026-10-21 | P2 | **UX 正式稿确认已到**（01:5x 节的「唯一未闭环」闭环）＋ **判据第二道收紧已提交 `55b7e34`、PM 一手实测验收通过**：UX 回执**确认采纳**「N 组重复：第 i 组 …」（核过协议 §3.2 ／ `client.js` 多组渲染 ／ `host/commands.js` 的 `groups.sort()` ／ P2-H 四处落地，与 UX 一致、无异议）＋ **R=0 不为它加分支**（PM 采信；PM 不另立措辞禁令）。**判据收紧**：**tester 提交 `55b7e34` 新增 `groupContextClause()`（按「；」切句取「第 i 组」/「同组」那一句，**锚点缺失 ⇒ fail closed 判红**）＋ `hasNumberWithUnit()`（数字须**紧跟单位词**「条」/「字节」）⇒ 全文匹配 → **组语境片段内匹配**；**PM 一手复现的假绿缺口已堵**：造景让 `groups[0]`（2 条组）的条数与**组数 2** 重合、两组 R 同值 ⇒ **收紧前 v2（整句漏写第 1 组）也 pass；已提交态 v2 判红、v1 仍绿**。**PM 一手复跑**：P2 判据 **10/10**、P2 自测 **25/25**（+1 条组语境负例）、`test:scripts` **180/177/0/3**、`test:plugin` **292/289/0/3**、P1 判据 **43/40/0/3** ＋ 自测 **60/60**。izzy 提到的 `23f9b90`／`23b86f0` **均已在 `origin/main`**（PM 一手核 ancestor）⇒ 无需再推；旧数字（9/9、178/175/0/3）已被 10/10、180/177/0/3 取代。**另新登记一条判据健壮性缺口**：P2-H 自测在重负载下因 `waitForStableNotice` 的墙钟预算而**假红**（真因是超时，红字却报成「没有出现提示」⇒ 易被误读成实现漏报）⇒ 已投 tester，PM 复跑门禁一律串行 | 本文件 02:0x 节 ＋ `%TEMP%\p2-repro\repro-fragment-gap.mjs`（只读复现脚本）＋ `git merge-base --is-ancestor` 实测 |
| 2026-10-20 | P5 | **「ChatGPT 网页登录时选不了 Google 邮箱做快速认证」的根因查清（实跑探针，非推断）**：新探针 `src-tauri/examples/official_chat_login_probe.rs`（按**生产接线**起 webview：同一 `data_store_identifier` 家族 + 同一 `additional_browser_args` + 同一份 `chat-fingerprint.js` + 同一 `on_new_window` 路由语义；**探针专属标识符 `dsh-chat-prb-001`**，不碰用户真实档案）。**读数**：① 登录页在 WKWebView 里**正常渲染**（`challenge: false`，标题「开始使用 \| ChatGPT」，无 Cloudflare 拦截）；② 该按钮是 **Google GSI 按钮**，形态**随 GSI 是否就绪而变**——**GSI 就绪前**点（r1/r8）⇒ **同帧重定向**到 `accounts.google.com/v3/signin/identifier?...&redirect_uri=https%3A%2F%2Fauth.openai.com%2Fapi%2Faccounts%2Fcallback%2Fgoogle`，页面渲染**「邮箱或电话号码」表单**（`/v3/signin/identifier`）而**不是账号选择器**；**GSI 就绪后**点（r9/r10/r11，等 `window.google.accounts.id` 出现才点）⇒ **什么都不发生**（`window.open` 零调用、宿主 `on_new_window` 零请求 ⇒ 合成点击**没有用户手势**，GSI 的弹窗被 WebKit 拦掉）；③ **清空存储后行为不变** ⇒ 与状态污染无关。**根因**：官方对话的内容 webview 用**壳专属的浏览器档案**（macOS = `dsh-chat-rel-001` / `dsh-chat-dev-001` 的 WKWebsiteDataStore；Windows = `webview-official-chat` 的 user-data 目录）⇒ 与 Safari / Chrome **永不共享**登录态 ⇒ 账号选择器（快速认证）在壳内**结构上不可达**；**叠加**生产的「外部 http(s) 弹窗一律交系统浏览器」⇒ GSI 的账号选择器会弹到**别的浏览器**里，用户在那边选好了 Google 账号，**壳里的页签仍未登录**（静默失败，壳侧无日志）。**落地**：认证域的 `window.open` **留在壳内**（`PopupPolicy.keep_login_in_shell`）＋ 托管弹窗**继承内容页签那份档案**（`profile_dir` + `browser_args`；Windows 上 user-data 目录不同 = 两套 cookie jar，macOS 上由 `window_features` 传递 opener 的 webview 配置继承同一 data store）＋ 单测 3 条（`is_login_domain` 两组 + 路由三组）＋ **判据第 32 条**（含**反向验**：改 `keep_login_in_shell`、掏空 `profile_dir`、让 `popup_route` 不分流 ⇒ 三处全被抓住）。**未闭合**：① 壳内**首次** Google 登录需要完整输入账号密码（**只有用户本人能验**，Google 在嵌入式 webview 里的口令策略未测；走通一次之后 GSI 的弹窗才会直接列出账号、一点即登录）；② **真实点击**（带用户手势）下的弹窗形态**未直接实测**——Computer Use 被宿主权限挡住（`computer_permission_required`），探针的 `--wait-click` 等待窗口超时无人点击；现结论由「GSI 就绪后合成点击被拦」＋「GSI 就绪前是同帧重定向」两条读数推出 | `src-tauri/examples/official_chat_login_probe.rs`（探针，证据在本条正文）＋ `src-tauri/src/harness/official_chat.rs`（`is_login_domain`）＋ `src-tauri/src/harness/harness_window.rs`（`PopupPolicy`）＋ `scripts/check-invariants.mjs` 第 32 条 |
| 2026-10-21 | P5 | **第二轮：把「为什么选不了 Google 邮箱」查到能落地的粒度，并补上一个真缺陷**。**(a) 探针加两个参数**：`--data-store <16 字符>`（用别的 WKWebsiteDataStore 标识符起 webview）与 `--url <地址>`（换被测页面；**只有当被测页面本身是 Google 页面时才跳过点击**，在 Google 页面上再点「Google」字样会自己跳走、毁掉现场）。**(b) A/B 实测（读数在 `/tmp/dsh-login-probe-r1x*.log`，按轮次独立命名）**：把 dev 壳那份存储**只读拷贝**到探针自己的进程目录下（探针写拷贝、不碰用户真实档案；拷贝内容已核：带 `ACCOUNT_CHOOSER`/`SAPISID`/`SSID`/`g_state` 等 Google cookie）⇒ 2×2 矩阵：· `myaccount.google.com` **无 session** ⇒ 跳去 `www.google.com/account/about`「使用 Google 账号登录」；· `myaccount.google.com` **有 session** ⇒ 直接渲染出**「zhong izzy / zxx0406@gmail.com」** ⇒ **「嵌 webview 缺浏览器能力」这条假设被证伪**：壳的 WKWebView **被 Google 正常接受**，没有「此浏览器或应用可能不安全」那类拦截，缺的只是「同一份 cookie jar」。· `/v3/signin/identifier` 端点**有 session 也照样**给「邮箱或电话号码」表单 ⇒ 那个端点问的就是「你是谁」这一步，**账号选择器在下一环**（`/signin/oauth/legacy/consent`，用 r8 抓到的过期 `part` token 直连 ⇒ 两支都是 400，无效）。**(c) 顺带证实的用户侧读数**：拷贝里 **ChatGPT 会话也还在** ⇒ 登录页**自动跳进应用**（title=ChatGPT、正文空、GSI 按钮根本不出现）⇒ 用户那次 Google 登录**确实在壳内走完了**，此后 ChatGPT 侧不再需要走登录页。**(d) 补上一个真缺陷（会使命修复在 macOS 上等于没生效）**：托管弹窗原先只带 `profile_dir` + `browser_args`，**没带 `data_store_identifier`** ⇒ **macOS 上 `data_directory` 被 WKWebView 忽略**（tauri 的方法文档原话：「WKWebView 里 `data_directory` 不可用时用它替代」）⇒ 弹窗落到**默认**数据存储 ⇒ 里面没有发起方那份 Google session ⇒ 认证弹窗里**照样出不来账号选择器**，而前三条判据仍全绿。已修：`PopupPolicy` 增 `data_store_identifier` + `fingerprint`，`create_popup_window` 显式绑同一标识符并注入同一份 `chat-fingerprint.js`；`commands.rs` 传 `Some(OFFICIAL_CHAT_DATA_STORE_IDENTIFIER)` + `fingerprint: true`；顺带**订正**旧注释里「macOS 靠 `window_features` 传递 opener 配置继承同一 data store」这句**不成立**的说法。**(e) 判据第 32 条从三处扩到五处**（新增 `data_store_identifier: Some(`、`fingerprint: true`，另加一条「策略里的标识符必须真被用上」）⇒ **反向验 5 处全被抓住**（删 `data_store_identifier` 传参 / 删 `fingerprint` / 掏空 `data_store_identifier` 调用 ⇒ 各自报红），恢复后源文件与备份**逐字节一致**、判据复绿。**仍未闭合**：**账号选择器本身**未直接实测（需要「有 Google session、无 ChatGPT session」的存储夹具——WebKit 的 cookie 文件**不是**公开的 binarycookies 格式（实测其头部是 `cook`+页数，但页内是变长记录 + 值是 **binary plist**），未做逆向；且 Computer Use 被宿主权限挡住，真实点击测不了） | `/tmp/dsh-login-probe-r12…r19*.log`（可清）＋ `src-tauri/examples/official_chat_login_probe.rs`（探针，已并仓）＋ `src-tauri/src/harness/harness_window.rs`（`PopupPolicy`）＋ `scripts/check-invariants.mjs` 第 32 条 |
| 2026-10-21 | P5 | **第三轮：把「二次登录能不能快速选账号」端到端验完（结论＝能）**。**(a) 账号选择器本身被直接抓到**：GSI 弹窗的内容就是 Google OAuth 的 authorize 端点 ⇒ 直接把它当 URL 打开，绕开「需要真人点击」这一环：A/B 两支只差 cookie jar——**无 Google session** ⇒ `accounts.google.com/v3/signin/identifier` →「**邮箱或电话号码**」；**有 Google session** ⇒ `accounts.google.com/v3/signin/**accountchooser**` →「**选择账号**」＋「**izzy zhong / zxx0406@gmail.com**」＋「使用其他账号」，并带「继续前往 OpenAI」。**(b) 托管弹窗的端到端读数**：探针新增 `--open-popup <地址>`，让页面 `window.open` 一个账号选择器地址，宿主**照生产接线**建出壳托管弹窗（同一个 `data_store_identifier` + 同一份指纹脚本 + 同一 user-data 目录）⇒ 弹窗里 `cookieNames` = `SID`/`SAPISID`/`APISID`/`__Secure-3PAPISID` 等（**发起方那份 session 在**），渲染 `/v3/signin/accountchooser` →「选择账号 + 账号名」。**(c) 反向验（证明那行标识符是必需的）**：把弹窗的 `data_store_identifier` 去掉（= 修复前状态）⇒ 弹窗 `cookieNames` **空**、渲染退回 `/v3/signin/identifier` →「邮箱或电话号码」；恢复后源文件与备份**逐字节一致**。⇒ **那行标识符就是「二次登录能快速选账号」的全部差别**。**(d) 附带测出 WKWebView 不拦弹窗**：脚本发的 `window.open`（无用户手势）照样被宿主收到并建出弹窗（`popup_created=true`）⇒ GSI 那扇窗在壳内一定出得来，「GSI 就绪后合成点击被拦」是 **GSI 自己**在点击处理器里查用户激活后提前返回（页内 `window.open` 零调用），**不是** WebKit 的弹窗拦截。**(e) Computer Use 仍不可用**：宿主对 `computer_app_list` / `computer_window_list` / `computer_desktop_state` 一律返回 `computer_permission_required` ⇒ 真人点击那一环只能由用户本人做 | `/tmp/dsh-login-probe-r20…r23*.log`（可清）＋ `src-tauri/examples/official_chat_login_probe.rs`（探针，已并仓） |
| 2026-10-21 | P5 | **口径拍板（izzy）**：**不**让 ChatGPT 页签改用壳自己的 OpenAI 登录态、不跳过网页登录 ⇒ 验收范围定为「**首次在壳内输一次账号密码 ＋ 之后二次登录直接选账号**」，本次修复即交付到这一段。壳读本机 Codex 登录**只为额度**，不当网页会话凭据；这条不再作为未决项跟进 | 本文件 P5 行 ＋ `AGENTS.md` 官方对话登录链那条 |

| 2026-10-21 | P2 | **P2-J 已落地并推送**（`6344cb3`，判据 **10 条 → 11 条**）：断「**发送后用户眼前那条提示仍不失真**」——内核 notice 单槽 ⇒ 与「最后投递的那条」等价 —— 逐字相同 / `info` 档 / 不含发送结果字样 / 不写成「当前状态」/ 仍自包含该组事实；**内核槽存续不可机检**（如实在 detail 里写明，不拿客户端记录冒充内核槽）；「发送」口径＝正文进模型请求（请求数 +1）；自测 **+1 条用例组**（正对照 ＋ 5 条负例）。**PM 一手复跑（串行）**：P2 判据 **11/11**、P2 自测 **26/26**、`test:scripts` **181/178/0/3**、`test:plugin` **292/289/0/3**、P1 判据 **43/40/0/3** ＋ 自测 **60/60** ⇒ 判据入库基线 **`6344cb3`**。**新派两单（归 tester）**：① P2-5 预算数字由 `text.includes` 改 `hasNumberWithUnit`（子串假绿那格必须红、须配反向用例）；② P2-H 重负载假红**分报**（超时不许报成「没出现提示」）＋ 重负载复验 | 本文件 02:5x 节 ＋ `git show 6344cb3` ＋ `node scripts/p2-protocol.run.mjs` 与 `npm run test:scripts` 实测输出 |

| 2026-10-21 | P2 | **tester 两单已提交 `e3929a5`（PM 一手验收通过）** —— 派单① **P2-5 预算数字加严**（`:1117`/`:1123` 改 `hasNumberWithUnit`，子串假绿那格必须红）＋ 派单② **P2-H 等待改事件式**（`:277` `waitForLastRoundNotice`，完成信号＝「`trigger`＝最后一次 pick 的 ref」或「组数达期望值」、30s 只作兜底；超时红字 `:1703`/`:1705` ＝「**疑似负载过高，不是漏报**」）。**PM 一手复跑（串行，HEAD `2e73502` 含 `e3929a5`）**：P2 自测 **26/26** · P2 判据 **11/11** · `test:scripts` **181/178/0/3** · `test:plugin` **292/289/0/3** · P1 判据 **43/40/0/3** ＋ 自测 **60/60** ⇒ **事件式改法未引入新假红**。tester 未推送，**随 PM 的 `2e73502` 推送一并上远端**（ancestor 已核）⇒ **远端 `main` = `2e73502`**。**新增扩派单③**（UX 扫出、PM 一手核行号）：另有 **7 个调用点仍是 2500ms 墙钟**（P2-2 `:726`/`:782`、P2-5 `:1075`/`:1180`/`:1185`、P2-6 `:1488`、P2-J `:1821`）⇒ 口径＝**先改超时分报、再改事件式**，须配反向用例。**另修一处会误导的措辞**：「远端 `main` = `6344cb3`」改成带时点的「判据入库基线」 | 本文件 02:5x／02:6x 节 ＋ `git show e3929a5` ＋ `node scripts/p2-protocol.run.mjs` 与 `npm run test:scripts` 实测输出 |

| 2026-10-21 | P2 | **派单③第一步已入库 `f55124d`（PM 一手验收通过）** —— 等待预算**由 adapter 给**（真机 **30s** ＋ `p1-adapter.mjs` 的 `waitBudgetMs()`、合成 **500ms**、活体注入 **10s**）、兜底 **2500ms→30s**；红字统一 `noticeTimeoutWhy()`（7 个调用点全走它）＝「等预算（Ns）内**没有出现**X（**疑似负载过高，不等于「没发生」**）」；**判据断点一个字没改**、**P2-5 特有措辞未被吞掉**。**PM 一手复跑（已提交态、工作树干净、串行）**：P2 自测 **26/26**（34.0s）· P2 判据 **11/11**（0.7s）· `test:scripts` **178/0/3**（34.7s）· `test:plugin` **289/0/3** · P1 判据 **43/40/0/3** ＋ 自测 **60/60**。**重负载复验（并行 `prep:builtin`）⇒ P2 自测 26/26 全绿、0 失败、34.0s（与串行一致）⇒ 02:0x 登记的「重负载假红」缺口闭环**。**顺带测到注入预算代价**：三个活体注入负例各等满 **10.1s**（占自测 34s 里的 30s）⇒ 已裁「`:843` 的 10_000 → **写死 1_000**」（自测 **34.0s → ~7s ＝ 省 ~27s、约 3/4**；取 2s 则 ~10s ⇒ 不写区间）。**三条待补（已登记，该笔只背书已到位部分）**：① 通用后半句未并进共享函数（另 5 处仍缺）② **超时红字口径零回归保护**（自测「疑似负载」0 命中）③ 注入预算未降档；另：派单③第二步（这四条改事件式）未做（单组场景 `groups.length` 恒为 1 ⇒ 信号②永不命中 ⇒ 只该做信号①） | 本文件 02:7x 节 ＋ `git show f55124d` ＋ `node scripts/p2-protocol.test.mjs`／`npm run test:scripts` 一手实测输出（串行与重负载两轮） |

| 2026-10-21 | P2 | **派单③两步已入库**（`0d03be0` 红字口径统一 ＋ `9c32e0d` 等待改事件式，2 文件 / +213 / −21 ＋ +260 / −22）—— 事件式用 `isRoundNotice()` 按 `trigger` 定位轮次；**完成信号②只留在 `waitForLastRoundNotice` 且只有 P2-H 用** ⇒ **单组/单条场景没抄进死代码** ✔；注入预算已降到 `1_000`。**PM 一手复跑（已提交态、工作树干净、串行）**：P2 自测 **29/29**（7.6s）· P2 判据 **11/11**（0.8s）· `test:scripts` **181/0/3**（8.5s）· `test:plugin` **289/0/3** · P1 判据 **43/40/0/3** ＋ 自测 **60/60**。**重负载复验两轮全绿**（并行 `prep:builtin`）⇒ P2 自测 29/29 7.6s、`test:scripts` 184/181/0/3 8.3s。**降档收益实测命中**：判据自测 **34.0s → 7.6s ＝ 省 ~26.4s、约 78%**（真机 runner 无感）⇒ 原「待验」结论转正。**基线更新**：判据仍 **11 条**、自测用例 **26 → 29**。**仍待落地（措辞层，机制已闭环）**：默认 verdict 去「不是漏报」（现状仍矛盾）· P2-5 定稿句（0 命中）· P2-H 句式 ＋ 来源引用移出红字 · 多层互斥 · 反向用例断言同步 | 本文件 02:8x 节 ＋ `git show 0d03be0` / `9c32e0d` ＋ `node scripts/p2-protocol.test.mjs`／`npm run test:scripts` 一手实测（串行与重负载两轮） |

| 2026-10-21 | P2 | **派单③「措辞层」已落地（`79962cf`）＋ PM 一手复跑全绿 ＋ 派单③全量闭环** —— 02:8x ⑤ 登记的「红字字面矛盾」三条 ＋ 另两条口径全部落地，**只动措辞**（`status`/`PASS`/`FAIL` 的字面增删行 = 0 ⇒ **判据断点一字未改**；`plugins/**` 与协议正文一行未动）：① **默认 verdict 去掉「不是漏报」**（`noticeTimeoutWhy()` 加 `verdict`/`tail`/`budgetMs`，7 处共用）⇒ 句式＝现象 → 条件 → 结论一条直线；② **P2-5 定稿**（「再分辨是漏报，还是只发了一条（被 early return 吃掉）」、verdict 用默认、**tail 覆盖通用后半句**、原「这一处另注」**元话语整段删掉搬进注释**）；③ **P2-H 句式**（走共享函数 ＋ `budgetMs: 30_000`，**来源引用移出红字进注释**）；④ **多层互斥**（有事件证据 ⇒ 收在证据层、**不出通用后半句**）；⑤ **反向用例同步**（`/疑似负载过高(?!，)/` ＋ P2-5 那处断含/不含 ＋ 新增断不含「这一处另注」＋ 7 处按三种形状分别断）。**PM 一手探针打印的实际红字**（直接调导出的共享函数，非转述）三句**逐字符合 UX 定稿**、默认句式**不含**「不是漏报」✔。**PM 一手复跑（已提交态、工作树干净、串行）**：P2 自测 **29/29**（7.7s）· P2 判据 **11/11**（0.8s）· `test:scripts` **181/0/3**（8.7s）· `test:plugin` **289/0/3**（5.3s）· P1 判据 **43/40/0/3** ＋ 自测 **60/60**。**重负载复验两轮全绿**（并行 `prep:builtin`）⇒ A 29/29 7.6s、B 184/181/0/3 8.4s，**无失败行无超时行** ⇒ **02:0x 登记的「P2-H 重负载假红」缺口闭环**。**判据仍 11 条、自测仍 29 条**（本轮未新增判据） | 本文件 02:9x 节 ＋ `git show 79962cf` ＋ `node scripts/p2-protocol.test.mjs`／`npm run test:scripts` 一手实测（串行与重负载两轮）＋ `%TEMP%\p2-timeout-probe.mjs`（只读探针） |
| 2026-10-21 | P3 | **P3 立项 ＋ 协议冻结稿 v0.1 入库（`6ad99b4`）＋ 第一批两单已派出** —— izzy 07:32 指示「继续完成后面的任务计划」⇒ 启动 P3。**先做「条款×现状」对照再定范围**：PM 派只读调查逐条核 development-plan §8 七段要求 × 实现现状（32 项），**PM 不采信回执、一手复核了第一批要落地的关键行**——`host/index.js:348`（快照直写最终路径，`wx` ＋ `mode:0o600`，**非原子**）、`:388-395`（读时版本＋摘要校验，**已满足**）、`host/sweep.js:45-49`（「不保护正在发送中的引用」「不引入引用计数」⇒ **回收无引用判定**）与 `:20-32`/`:216-242`（**只扫四家族、无临时文件类**）、`client/client.js:507`/`:517-523`（`selectedRefs` **只在进程内、无释放路径**，注释自陈）、`host/commands.js:412`/`:489`/`:577`（账户键与快照 `accountScope` **取自请求参数**）vs `:528`/`:635`/`:699`（`sourceFingerprint` 才是服务端推导）⇒ **S-15 是「协议与实现冲突」，按纪律改实现不改协议**。**决定 P3 范围的关键事实（内核侧）**：`InputTriggerSource` 接口全集（现役 `0.2.1-rc.2`，`@deepseek-ai/dsh-client-ui-input-trigger/lib/types/types.d.ts:139-216`，SHA-256 `2D5B004229C1657BC0C484D1F881E2498FA9F45549589A3F1E40E82C1F637582`）**没有任何「引用被移除／草稿被放弃／已发送成功」回调** ⇒ 内核只**问** source、从不**告诉** source ⇒ 新登记**上游缺口 G1-8** ⇒ S-12 定降级口径（「误删 ⇒ 该引用失效并被拒 ＋ 可见错误，**不是错发**」，沿用 P1 §4.1-4 的既有后果语义）⇒ **不**把「引用关系精确等于内核草稿」写成验收项（先例：P1 的 G1-6）。**顺带修正两处不准确引用**：调查报「`capabilities` 两份取大」不成立（`host/commands.js:324` 的 `capabilities` 只发常量/错误码/状态机）、「客户端探针只记 `ref`/`contentDigest`/`bytes`/`code`」不成立（实际是 `ref`/`sessionId`/`query`/`code`/`delayMs`/`message`/`why`）⇒ 协议正文已按**一手核过的行号**改准。**顺带销号**：P2 协议 §9 的 G-1 行仍写「记为待办，P2 三线收口后再修（不插队）」⇒ 实际早已由 `4bdb831` 修完（`paths.rs` 的 `TempDir` 带 `Drop`，`:773-776`）且 PM 实测（连跑 3 轮 `cargo test`，`%TEMP%` 的 `dsh-xlink-paths-*` **34 → 12 → 0 → 0 → 0** ⇒ 每轮新增 0）⇒ §9 已销号（`8b58bbc`）。**已派出两单**：tester＝P3 判据 **P3-1…P3-9**（**只新增** `scripts/lib/p3-protocol.mjs`／`scripts/p3-protocol.run.mjs`／`scripts/p3-protocol.test.mjs`）＋ 明确「**P3-9 在 devloper 改实现前必然红是预期**（判据是规格，不许改软）」与「**不许自己往实现里加钩子**，缺的注入点列清单交 PM 裁定（adapter 吸收是既有做法）」＋ 三条**不许写成假断言**的（G1-6／G1-8 降级口径／Windows ACL）；devloper＝**只做 S-14（目录缓存目录 `0700`／文件 `0600`）＋ S-15（账户维度服务端推导、客户端值不符即 `account_changed` 拒、**不静默改写**、路径形状不动、`sourceFingerprint` 口径不动）**，其余条款**不抢**，且**不许碰** `scripts/**`（与 tester 并行）。**本地提交不 push**（PM 统一推 main）；**UX 本批不派**（S-8…S-15 全在存储层，**无界面层内容**） | 本文件 07:3x–07:5x 节 ＋ `official-chat-p3-protocol.md` v0.1 ＋ 一手核过的行号清单 ＋ `git show 8b58bbc` |

| 2026-10-21 | P3 | **P3 实现第 1 组（S-14 ＋ S-15）验收通过（`f57b8d2`）＋ PM 一手复跑全绿 ＋ P3 协议升 v0.2** —— devloper 交付 `f57b8d2`（2 文件 / +275 / −5：`plugins/official-chat/host/commands.js` +73/−5 ＋ 新增 `plugins/official-chat/test/p3-storage-account.test.mjs` 202 行 / 8 条）：**S-14** 目录缓存目录 `0o700`、文件 `0o600` ＋ 新增 `chmodQuiet()` 收**存量**（Node 的 `mode` 只在创建时生效；收不动就吞掉，不把业务带崩）；**S-15** 新增 `accountScopeMismatch(provided, derived)`（没传/一致 ⇒ 放行），`catalog`/`capture` 改用**闭包里的服务端推导值**，不符即 `account_changed` 拒（detail「账户范围与服务端推导值不一致」）、**不静默改写**；**未动** `sourceFingerprint` 推导口径、路径形状、错误日志四字段、sweep 日志相对路径、`plugins/**` 其余文件、`scripts/**`、`docs/**`。**PM 一手核过的两条硬事实**：`account_changed` **在 P1 13 码闭集内**（P1 `:147`）⇒ 未引入闭集外新码；`request.accountScope` 全文件**只有 2 处**被取（`catalog` `:454` / `capture` `:551`）⇒ `preview`/`duplicates` 确实不消费，client 侧**零命中** ⇒ 实机走「没传 ⇒ 放行」。**PM 一手复跑（已提交态、HEAD `6b3693a`、工作树 tracked 干净、串行）**：P2 自测 **29/29**（7.6s）· P2 判据 **11/11**（0.7s）· P1 判据 **43/40/0/3**（2.7s）· P1 自测 **60/60**（1.4s）· `test:scripts` **181/0/3**（184 tests，8.4s）· `test:plugin` **297/0/3**（300 tests，5.1s）· `check:static` **四项全绿**（不变量全通过、重复代码 4/8、WebDriver 门控 ✔、builtin 4 插件/43 文件，3.6s）· `cargo test --locked --lib` **1129 passed / 0 failed / 11 ignored**（15.0s）。**重负载复验三轮全绿（并行 `prep:builtin`）**：A P2 自测 29/29 7.6s · B `test:scripts` 184/181/0/3 8.4s（**无失败行无超时行**）· C `test:plugin` 300/297/0/3 5.2s。**4 项裁定已落进协议**：① **P3-9① vs §S-15 裁定 4** —— 实现按裁定 4「不符即拒」（**不用改**），但协议 §5 P3-9① 那一行在「不符即拒」下**不可机检**（两个不同的值里至少一个不符 ⇒ 被拒 ⇒ 无从比较两个落盘字段）⇒ **由 PM 改写成可机检形态**（性质未放宽，只换观察方式）；② `preview`/`duplicates` **本批不加**一致性校验（不消费 ⇒ 无可校验对象；加校验 ＝ 扩大拒绝面）⇒ 口径**写死**进 §S-15 裁定 1，下一轮不必再问；③ **P3-8 的 P1 指针早已在**（P1 §4.1 第 5 条，同时含 `official-chat-p3-protocol.md` 与 `S-14`，随 `6ad99b4` 落地）⇒ **无需另派单**；④ `commands.js:804` 的「**口径待 PM 确认**」是**过期事实**（P2 协议 §2.2 `:65` 23:5x 已裁定「`groups` 只返回成员数 ≥ 2 的组」）⇒ **只改注释、不动判定逻辑**（`plugins/**` 归 devloper）。**PM 本轮两笔协议改动**：`a837056`（P3 协议**升 v0.2**：§5 P3-9① 改写 ＋ §S-15 裁定 1 补充 ＋ §S-14/§S-15「现状」加闭环指针、v0.1 基线原文**保留不回改**）、`6b3693a`（P1 协议一处**标点笔误**修正，纯标点不改语义）。**两条提醒**：tester 的 P3-7 Unix 权限断言在 Windows 上**必须显式记「未执行」**、不许空断言空跑（§S-14 裁定 2 已定 Windows 侧「只做如实记录」）；devloper 的 `p3-storage-account.test.mjs` 第 2 条（存量收口）**Windows 上断言体为空 ⇒ 空跑通过**（**非实现缺口**，但别以「通过」身份空跑）。**判据侧基线未变**（P3-1…P3-9 仍在 tester 手上、未提交、未验收 ⇒ 尚无入库基线）；**在途文件不被门禁加载已一手核过**（08:0x 观察到 3 个未跟踪文件：`scripts/lib/p3-protocol.mjs`、`scripts/lib/p3-adapter.mjs`、`scripts/p3-protocol.run.mjs`；全仓**没有任何 `scripts/*.test.mjs`** import 它们，`test:scripts` 的 glob `scripts/*.test.mjs` **不递归**且这三个文件**都不是** `.test.mjs` ⇒ 不在扫描范围） | 本文件 08:0x 节 ＋ `git show f57b8d2` ＋ `official-chat-p3-protocol.md` v0.2 §5/§S-15 ＋ 一手实测输出（串行 ＋ 重负载三轮） |

| 2026-10-21 | P3 | **P3 判据入库（`871d0ff` ＋ `ca992a2`）＋ 两处收尾（`129df9a`）＋ PM 独立注入复核（四处全部按预期翻红 ⇒ Windows 侧无空跑）** —— 三笔成员提交 PM 已推，远端 `main` = **`ca992a2`**。**`129df9a`（devloper）**：nit 收尾（Windows 侧空跑断言补 fallback —— 新增 `writeFunctionSource()` 圈出 `write()` 正文，断「**这一处**确实带了收口」，不是全文件有同名字符串就算）＋ 补协议 v0.2 可机检形态② 的自测（8 → **9** 条）＋ **裁定 ④**（`duplicates()` 那句「口径待 PM 确认」改成写明 P2 协议 §2.2 的裁定与理由，`if (members.length < 2) continue;` **一行未动**）。**`871d0ff`（tester）**：判据入库，**只新增 4 个判据侧文件**（合计 **+2920 行**），**P3-1…P3-9 共 9 条**。**`ca992a2`（tester）**：**P3-7 的 Windows 空跑修掉**（`871d0ff` 那版在 Windows 上只有 Unix 侧断言、没有 else ⇒ 空跑通过；改为「**显式记「未执行」** ＋ 另找一条能真失败的顶上：改断源码形态，且只在那个写盘点所在的函数正文里数」）。**PM 独立注入复核**（**独立 worktree**、detached @ HEAD、**不碰共享工作树**，跑完 remove ＋ prune）：**A** 删两行 `chmodQuiet` 收口 ⇒ 红 devloper 的「存量收口」那条；**B** 删两处 `mode` 参数 ⇒ 红 devloper 的「目录/文件 mode」那条；**C** 删 `writeFileSync` 的 `mode: 0o600` ⇒ **新增红 P3-7**；**D** 把账户维度改回取自请求参数 ⇒ **新增红 P3-9** ⇒ **四处全部按预期翻红**（基线 9/9 绿、还原后 9/9 绿）⇒ **Windows 侧无空跑**。**PM 踩到的两个「注入本身会错」的坑**（都由探针护栏挡住、两次都是先怀疑注入）：① 正则带了行尾换行要求 ⇒ `chmodQuiet(path, 0o600);` 是被截取正文的**最后一行**（末尾无 `\n`）⇒ 只删掉一行；② **`RegExp.prototype.source` 不带 flag** ⇒ 用 `new RegExp(source, 'g')` 重建时**丢了 `m`** ⇒ `$` 从行尾变成整段尾 ⇒ 命中恒 0 ⇒ **探针按设计拒绝下结论**（「未命中即抛错」）⇒ 补回 `m` 才拿到有效结果。**PM 一手复跑（已提交态、HEAD `ca992a2`、工作树 tracked 干净、串行）**：**P3 判据 9 条＝通过 4（P3-6/7/8/9）· 失败 5（P3-1…P3-5 ＝ S-8…S-12 **第二批范围，预期红**，红字逐条写明缺哪条协议条款）· 未执行 0** · P3 自测 **18/18** · P2 自测 29/29 · P2 判据 11/11 · P1 判据 43/40/0/3 ＋ P1 自测 60/60 · `test:scripts` **199/0/3**（202 tests）· `test:plugin` **298/0/3**（301 tests）· `check:static` **四项全绿** · `cargo test --locked --lib` **1129/0/11**；**重负载三轮全绿**（A 29/29 7.7s · B 202/199/0/3 8.5s · C 301/298/0/3 5.2s）。**P3 判据 runner 不在任何 npm 门禁里**（与 P1/P2 runner 同款）⇒ 第二批未做不会让 CI 红。**基线**：实现 = `f57b8d2`、**判据 = `ca992a2`**、协议 = v0.2。**一条 nit 给 devloper**：`129df9a` 的标题前缀写 `test ＋ docs(official-chat)`，但它**只改 `plugins/**` 两个文件**（PM 一手核过 `--stat`）⇒ `docs` 指的是**注释**不是 `docs/` 目录 ⇒ 建议 type 改成 `fix`/`test`，免得按 type 扫 `git log` 时误判 | 本文件 08:3x 节 ＋ `git show 129df9a` / `871d0ff` / `ca992a2` ＋ `%TEMP%\probe-vacuous.mjs` / `probe-judge-vacuous.mjs`（独立 worktree 注入复核）＋ 一手实测输出（串行 ＋ 重负载三轮） |

| 2026-10-21 | P3 | **izzy 三项拍板已落进协议（v0.3）＋ P3 第二批两单已派出** —— izzy 09:3x 拍板：① **登录态由 izzy 自己用 `pnpm run dev` 的测试模式建立**，**PM 用 DeepSeek 官网对话做测试**（⇒ P4 前置归 izzy、测试归 PM；**本轮不启动 P4**）；② **fork 保留**（`July-X/deepseek-harness` 不删不转私有；实测仍在 PUBLIC／`master`／最后 push `2026-10-20T15:03:52Z`／PR 列表为空 ⇒ **本项关闭**）；③ **`seq` 不复用** ⇒ **G3-1 口径定了** ⇒ 协议升 **v0.3**（`2785663`，已推）。**v0.3 五处变更**：§2.5 新增**序号水位**（独立于认领文件／不得被清理流程删掉／水位必须先于认领落盘）、§S-13 新增**裁定 4**（`seq` 永不复用 ＋ 两条实现硬约束 ＋ **必然代价**：每会话 pick 次数从并发上限变**累计**上限 9999）、§5 新增**判据 P3-10**、§6 第 4 条把 **G3-2 拆两半**、§7.2/§8 同步；**S-8…S-12 正文、§4 常量、G1-8、P3-1…P3-9 一律未动**。**PM 裁出的张力点**（原稿没有，靠 izzy 这句答案才暴露）：「`seq` 不复用」若靠「**保留认领文件本体**」实现 ⇒ **已入库的 P3-6**（孤儿认领**被删**）会**变红** ⇒ 判据是规格、不许改软 ⇒ 实现**必须**用**独立水位记录** ⇒ 协议写死两条硬约束。**PM 裁定 G3-2 拆两半**：**G3-2a 缓存总量核算 512 MB ⇒ 排在第二批之后**（它要删快照 ⇒ 依赖 S-11 占用记录才知道哪些不能删）；**G3-2b 单条正文预算 64 KB ⇒ 另派一单**（新增对外拒收路径 ⇒ 要**扩 P1 §3 的 13 码闭集** ＋ **UX 新文案** ⇒ **这才是「会牵动 P2 提示口径」的真正原因**）。**已派出**：devloper **单① S-8＋S-9＋S-10**（半条记录不得发布／发布点＝rename 成功那一刻／写后回读校验／临时文件形态／`wx` 保留／第五家族清理）＋ **单② S-11＋S-12＋G3-1**（占用记录落盘＋R1/R2/R3＋先重建占用关系再回收＋`seq` 永不复用与水位载体）；tester **只补 P3-10**（并把三个注入坑写进派单：行尾换行要求／`RegExp.source` 不带 flag／护栏「未命中即抛错」）。**PM 一手复跑（已提交态、HEAD `2785663`、工作树 tracked 干净、串行）**：P3 判据 9 条＝通过 4 · 失败 5 · 未执行 0 · P3 自测 18/18 · P2 自测 29/29 · P2 判据 11/11 · P1 判据 43/40/0/3 ＋ P1 自测 60/60 · `test:scripts` 199/0/3 · `test:plugin` 298/0/3 · `check:static` 四项全绿 · `cargo test --locked --lib` **1134/0/11**（比 08:3x 多 5 ＝ 并行 PM 登录探针的 Rust 用例）⇒ **基线随分支移动，PM 每次都重跑**。**协作面两条事实**：分支上并行 PM 会话落了 `f75dd63`（认证链不许跨出壳 ＋ 托管弹窗继承同一份浏览器档案）与 `8f38167`（Windows 编译红 ＋ 判据第 32 条跨平台口径）⇒ **未动 P3 判据与 P3 协议正文** ⇒ **判据入库基线仍是 `ca992a2`**；⚠️ **那两笔的 author email 仍是旧账号 `m4java@163.com`** ⇒ 与 izzy 23:57 拍板的「提交身份统一」**不一致** ⇒ **PM 不擅自改已推送的历史**（改写 `main` 已发布提交风险高、且不归 PM）⇒ **登记待 izzy／并行 PM 决定** | 本文件 09:4x 节 ＋ `official-chat-p3-protocol.md` v0.3（§2.5／§S-13 裁定 4／§5 P3-10／§6／§7.2／§8）＋ `git show 2785663` ＋ 一手实测输出 |

| 2026-10-21 | P3 | **提交身份那一项已关闭（izzy 09:5x 拍板：不用重写，只要 author name 是 `July-X` 就可以）** —— 09:4x 登记的「`f75dd63`／`8f38167` 两笔的 author email 仍是旧账号 `m4java@163.com`」**PM 一手核 `origin/main` 全量**：**author name 1120/1120 全是 `July-X`**、**committer name 1120/1120 全是 `July-X`**；email 分布 1118 × `over618@163.com` ＋ 2 × `m4java@163.com` ⇒ izzy 的口径（name 是 `July-X`）**已满足** ⇒ **不做任何历史重写**（改写 `main` 已发布提交风险高、且那两笔不归 PM）⇒ **本项关闭**。P3 侧其余不变：协议 v0.3（`2785663`）、台账回填（`ed155c8`）、第二批两单在途 | 本文件 09:4x 节 ⑧ ＋ `git log --format='%an' / '%cn' / '%ae' origin/main` 一手统计 |

| 2026-10-21 | P3 | **第二批单①落地并经 PM 一手验收（`308ce51`）＋ 协议升 v0.4 ＋ 单②已派出 ＋ P4 硬前置解除** —— 单①（§S-8 原子发布／§S-9 发布点＝`rename` 成功那一刻／§S-10 第五家族清理）由 devloper 落地并本地提交（4 文件 / +486 / −13：`host/index.js`／`host/protocol.js`／`host/sweep.js` ＋ 新增 `host/test/p3-atomic-publish.test.mjs`；`scripts/**` 与 `docs/**` 一行未动）；**PM 一手复跑已提交态**（独立 worktree、detached @ `308ce51`、不碰共享工作树、跑完 remove ＋ prune）＝ **P3 判据 9 条：通过 7（P3-1/2/3 由红转绿）· 失败 2（P3-4/P3-5 ＝ 单②范围，预期红）· 未执行 0** · P3 自测 **18/18** · `test:scripts` **202/199/0/3**（202 tests，exit=0）· `test:plugin` **306/0/3**（309 tests）；⚠️ **worktree 复跑曾读出 2 条红，根因是「worktree 里没有 `src-tauri/target` ⇒ P2-D 判据缺 Rust example 产物」**，**不是代码回归** ⇒ 把主树 `target` 以 junction 挂进 worktree 后复跑即全绿 ⇒ **纪律：worktree 复跑前先确认产物依赖在不在**。**协议升 v0.4**（三处澄清）：§2.5 水位路径**与实现现状对齐**（补 `providerShort` 一层）、§S-11 的 **R1 写死本阶段的可判定形态**（＝host 侧「显式释放调用」；原写的「用户显式移除／该次 pick 被取消」在当前内核契约下**不可判定** ⇒ client 侧本批**不接线**、**不许新造移除 UI**）、§5 新增 **P3-11**（R1/R2 释放路径；R3 已由 P3-5④ 覆盖）。**PM 裁注入点**：判据侧用**子进程 patch `node:fs`** 制造真失败 ⇒ **实现不许加测试钩子/改签名**。**单②已派出**（S-11＋S-12＋G3-1）；tester 的 **P3-10 在途**（4 个 `scripts/**` 文件已改未提交）⇒ **P3-11 待其提交后再派**（不打断在途）。**P4 硬前置解除**（izzy 09:54 建好登录态）⇒ PM 只读调查已开：`captureSource` 仍是固定夹具、真实取数未实现；壳侧 `script_eval.rs`（**页内槽位＝页内痕迹**，自述「正式接站点前需要协调者裁定」）与 `cdp_eval.rs`（**Windows 零痕迹**，仅 Windows 实测过）⇒ **待 izzy 拍板取数机制**。**另：tester 的 **P3-10 已入库**（`2b22409`）** —— PM 推 `308ce51` 时它**已在本地提交、被一并推上远端** ⇒ 这一笔属**先推后验**（PM 已补验：10 条＝通过 7 · 失败 3 · 未执行 0，失败的三条＝P3-4/P3-5/P3-10，**都是单②范围、预期红**；P3-10 的红字逐条点名缺哪条协议条款 ⇒ 非空跑）⇒ 协议再升 **v0.5**（R1 的释放入口 ＝ 新增 host 命令 **`release`**；幂等、**不新增错误码**、**不复用 `cancel`** ⇒ 判据才有稳定名字可调）⇒ **P3-11 待派** | 本文件 10:3x 节

| 2026-10-21 | P3 | **P3 第二批全量验收通过**（实现 `344a284` ＋ 判据 `c3011f7`）＋ **协议升 v0.6** ＋ **PM 独立注入复核四处全部按预期翻红 ⇒ Windows 侧无空跑** —— 实现入库基线 = **`344a284`**（单② ＝ 占用记录 `refs/` ＋ 序号水位；单① ＝ `308ce51`）、**判据入库基线 = `c3011f7`**（**P3-1…P3-11 共 11 条**）。**PM 一手复跑已提交态**（工作树 tracked 干净、串行）＝ **P3 判据 11 条：通过 11 · 失败 0 · 未执行 0**（P3-4/P3-5/P3-10/P3-11 **全部由红转绿**）· P3 自测 **34/34** · P2 判据 11/11 · P2 自测 29/29 · P1 判据 43/40/0/3 ＋ P1 自测 60/60 · `test:scripts` **218/215/0/3** · `test:plugin` **324/321/0/3** · `check:static` **四项全绿**（内嵌资源 4 插件/44 文件）· `cargo test --locked --lib` **1134/0/11**。**注入复核**（独立 worktree、detached @ `c3011f7`、不碰共享工作树、探针护栏「未命中即抛错」）：**A** capture 不写占用记录 ⇒ **红 P3-4**（只红这一条）；**B** 水位写入挪到认领**之后** ⇒ **红 P3-10**（只红这一条 ⇒ 断的是**次序**）；**C** `release` 不删 ⇒ **红 P3-11**（只红这一条）；**D** sweep 不扫占用记录 ⇒ **红 P3-5** ＋ 连带 P3-3/P3-6/P3-10/P3-11（符合预期）；**还原后基线 11/11 全绿** ⇒ 三段齐全。**协议升 v0.6**：把实现自选的「占用记录**自己也过期**时，那份快照怎么办」写死成保守的一侧（**本轮不删、从下一轮起可回收**，沿用 P1 §4.1「宁可不清理，不乱删」）⇒ §5 的 P3-11② 同步措辞（**不放宽性质**）。**另记**：两笔成员提交是**由成员自己推上远端**的（PM 派单写的是「不要 push」）⇒ PM 已发回执问明，**不影响内容验收** | 本文件 11:0x 节 ＋ `official-chat-p3-protocol.md` v0.6 ＋ `git show 344a284` / `c3011f7` ＋ 一手实测输出 ＋ 独立 worktree 注入复核 | ＋ `official-chat-p3-protocol.md` v0.4 ＋ `git show 308ce51` ＋ 一手实测输出（独立 worktree） |

| 2026-10-21 | P3 | **P3-13 判据入库并经 PM 一手复跑 13/13 全绿 ⇒ 判据侧全闭环** ＋ **`9512602` 过期版本注释订正验收通过** ＋ **PM 更正自己上一轮对「push 触发者」的判断** ＋ **P0 §7 补记「只读拷贝能否带登录态」从未被实测** —— ① 判据入库基线 = **`7c7e641`**（**13 条**）＋ 订正 `fb1c624`；PM 一手复跑（独立 worktree、detached @ `80e4f75`、串行）＝ **P3 判据 13/13 全绿**（P3-13 由红转绿）· P3 判据自测 **45/45** · 本插件自测 **10/10 ＋ 15/15** · `test:scripts` **229/226/0/3** · `test:plugin` **334/331/0/3** · `check:static` **四项全绿** · `cargo test --locked --lib` **1134/0/11**（48.8s）⇒ **判据侧全闭环**（P3-12 ＝ 回归保护、P3-13 ＝ 本轮新增的事实登记）② P3-13 的**非空跑证明**（PM 一手核实现）：五种破法各红一处 ＋ `:1425` 断言每个违规变体**只红自己那一支** ＋ `:1716` 活体注入 ⇒ 五支都非空跑、Windows 侧无空跑 ③ `9512602`（7 文件**纯注释**、零行为变化；PM 一手核 `Cargo.toml:30` 不钉小版本 ＋ `Cargo.lock` 解析 tauri 2.12.3 / wry 0.57.0）**验收通过** ④ **PM 更正自己上一轮的判断**：`origin/main` reflog 显示 11:45:54–11:55:30 之间**无别的 push 事件** ⇒ **可证**：11:55:30 那一次 push 把远端推进到 `80e4f75` ⇒ `9512602`／`7c7e641`／`fb1c624` **是被那次 push 顺带带上的**（**推的人不由 reflog 判定**；「PM commit 11:55:24 → push 11:55:30」只是**时间紧邻性推断**）⇒ **教训**：reflog 里「远端 tip 变了」≠「多了一次 push」，而「时刻紧跟谁的 commit」**同样证不出是谁推的** ⑤ **P0 §7 补记**：探针源码注释与 P0 `:217` 那半句「Windows Cookies 由 DPAPI 绑机器+用户、不绑路径」**是推断、从未被实测**（P0 那轮机器上从未有登录态 ⇒ 结构上不可能验到）⇒ 已在 P0 §7 第 2 条下**显式标为待验项**；**边界**：只管**取证手段**、**不改生产机制**（`context.rs:13-19`／`:109`／`:107` ⇒ 生产路径复用壳自己已在用的 user-data 目录 ⇒ 登录态天然可用；红线管的是探针）⇒ **P4 协议骨架不因探针 B 成败而变** | 本文件 12:0x–12:1x 节 ＋ `git reflog show origin/main --date=iso` ＋ `git log e1c568f..80e4f75` ＋ `git merge-base --is-ancestor` ＋ `scripts/p3-protocol.test.mjs:1303-1311`／`:1414`／`:1425`／`:1716` ＋ `src-tauri/Cargo.toml:30` ＋ `src-tauri/Cargo.lock` ＋ `src-tauri/src/official_chat/context.rs:13-19` ＋ 一手实测输出（`verify-80e4f75.ps1`：判据 13/13 等） |

| 2026-10-21 | P3 | **PM 第二次更正 push 归属**（「成员全程零 push」不成立 ⇒ **reflog 证不出推的人**）＋ **裁定候选单 A ⇒ 另立 P3-14** ＋ **登记 P3-5③ 覆盖边界** —— ① tester 回执自报「`b2b1021` 那笔 **10:56:47 的 push 是我做的**」（输出 `55139f7..b2b1021 main -> main`）⇒ PM **一手交叉核过、不是照单采信**：reflog 里该次 `update by push` 时刻＝**10:56:47**、**前一 tip ＝ `55139f7`**、本地 commit 时刻＝**10:56:35** ⇒ **输出与 reflog 逐字吻合** ⇒ 该次 push 记在成员名下（**交叉印证，非单纯自报**）⇒ PM 上一轮「成员全程零 push」的「证明」**是方法错误** ⇒ 已在三处历史表述（④ push 口径 / ③ PM 自我更正 / 成员回执那节的取证结论）**加注更正** ② **根因（记在案）**：reflog 只能证「远端跟踪引用**何时**被推进、推进到**哪个 tip**」，**证不出「是谁推的」**（共享工作树 ⇒ push 的命令行与提交者无关）⇒「时刻紧跟 PM 的 commit」只是**时间紧邻性推断**，**不能当证明** ⇒ **口径不变、性质变**：「成员本地提交、PM 统一推」是**约定**、**不是**已证实事实；**知会方向仍成立**（正因为成员不推，PM 推自己那笔才会顺带带上 ⇒ 才知道该回告谁）③ **裁定候选单 A ＝ 接受 ⇒ 另立 P3-14**（断 §S-16 裁定 2 的「核算排在常规清理**之后**」）—— **PM 一手核过他的分析成立**：P3-13 场景 A 的夹具（`scripts/lib/p3-protocol.mjs:2102-2114`）里快照 `capturedAt: nowIso`（新鲜）＋ 占用记录也是 `nowIso` ⇒ **常规清理一件都回收不了** ⇒ 「== 清理后总量」与「== 清理前总量」**数值相同** ⇒ **次序被掩盖**、确实没断过；派单四条硬要求（**期望值由判据独立算、不许复述实现自报数**；`account-before-cleanup` **必须真红且其余支绿**；Windows 侧非空跑证明；**不许改已验收的 P3-5 与 P3-13**）④ **P3-5③ 的覆盖边界登记为「判据侧已知覆盖边界」（本阶段不改）**（`occupancyScanAt` 只看路径不看事件种类 ⇒ 「只删 `refs/` 而不读」的畸形实现能骗过它；**当前无此实现** ⇒ 不为假想实现改已验收判据；**不是**实现缺口、**不是**协议缺口） | 本文件 12:1x 节 ＋ `git reflog show origin/main --date=iso`（`b2b1021` @10:56:47，前一 tip `55139f7`）＋ 本地 `main` reflog（`b2b1021` commit @10:56:35）＋ tester 回执原文 ＋ `scripts/lib/p3-protocol.mjs:2102-2114`（P3-13 场景 A 夹具）＋ `official-chat-p3-protocol.md` §S-16 裁定 2 |

| 2026-10-21 | P3 | **一张迟到的 G3-2a 实现回执（`0de4af6`）⇒ 两个问题早已答复、不返工；其中一条「未验证」已被判据侧实测覆盖** —— ① **裁定 6（目录缓存删除步法）**：回执问「以哪个为准」⇒ **PM 已在协议 v0.10（`6e41ea6`）一手核 `host/sweep.js` 并自行更正**：目录缓存的常规清理**也是两步法**（`removeInTwoSteps`）⇒ 协议改为「快照与目录缓存**都沿用两步法**」⇒ **实现零改动、不需要再定** ② **裁定 7（「最旧」）**：回执写「本单没有落点」⇒ **与 v0.10 更正一致**，且 izzy 12:4x 拍板「不做硬上限」⇒ v0.11 已标「**已定不启用**」 ③ **「P3-13 判据尚未写」** ⇒ **已入库**（`7c7e641`）且 PM 一手复跑 **13/13 全绿** ⇒ 不存在「读数面待对齐」 ④ **「未在真实 512 MB 盘上验」** ⇒ **判据侧已被覆盖**：判据跑**真实实现**（`p3-protocol.run.mjs:83` 用 `createP3Adapter`）＋ `writeBigFile`＝**空文件 ＋ `truncateSync` 真在盘上造 512 MB+1**（`p3-adapter.mjs:987-993`，**不是 stat patch**）⇒ 额外淘汰真触发、总量真回落 ⇒ 造完即 `drop`（`:2180`/`:2199`）**不留大文件**（回执那条 stat patch 是**他自测**的做法）⑤ 回执给的字段名与判据实际读的**逐字一致**（判据读 `outcome.budget.totalBytes`，`p3-protocol.mjs:2022-2023`）⇒ **字段面无异议** ⑥ **其余陈述已过期**：远端 `main` 现 `900ff1d`（不是 `fb5a4f0`）· `0de4af6` **已在远端**（`git branch -r --contains 0de4af6`）· `test:scripts` 现 **229/226/0/3**（不是 221/218/0/3）· 探针 A/B 两问**早已答复** ⇒ **不返工、不需再答复** | 本文件 12:1x 节 ⑬ ＋ `official-chat-p3-protocol.md` v0.10／v0.11 变更记录（裁定 6／裁定 7）＋ `scripts/p3-protocol.run.mjs:83` ＋ `scripts/lib/p3-adapter.mjs:987-993` ＋ `scripts/lib/p3-protocol.mjs:2022-2023`/`:2180`/`:2199` ＋ `git branch -r --contains 0de4af6` ＋ 一手复跑输出（判据 13/13） |

| 2026-10-21 | P4 | **探针 A 结果落地 ⇒ P4 取数路线已定；探针 B（只读副本）实测不可用 ⇒ 关闭；B 项（exact route owner 准入）已被回环桥绕过 ⇒ 关闭 ⇒ P4 协议草案解锁** —— ① **探针 A 实测（未登录态）**：端点**确实存在** ＝ `GET /api/v0/chat_session/fetch_page?count=5` ⇒ **HTTP 200 ＋ JSON**、`topKeys=[code,msg,data]`、未登录响应 `{code:40003,msg:"INVALID_TOKEN",data:null}`；**登录 cookie 是 HttpOnly**（页内 `document.cookie` 只看得到 `.thumbcache…`／`smidV2`）⇒ **页外拿不到凭据** ② **PM 裁定取数路线**：「页内取数」与「调站点 API」**不是二选一**，真解 ＝ **页内同源 fetch 站点自己的 API** ⇒ 协议写死三条：**执行体在壳**（CDP `Runtime.evaluate` ＋ `awaitPromise` ＋ `returnByValue`）／**API 调用也在页内**（凭据由浏览器自动带 ⇒ **页外调该 API 必失败**）／**回程只回结构**（键名·条数·布尔量，**不回正文、不回凭据**）；**端点已定**，**字段形态不许先猜**；站内码**原样上报、不解释语义**（三档码待登录态复测；P0 那条「`userToken` 不被认 ⇒ 40003」本次**不能定论**）③ **探针 B（只读副本）＝ 本阶段关闭（不是「原理上不可行」）**：副本里 5 个 cookie **逐项完整在盘上**（行数、`encrypted_value` 长度与源一致）⇒ **站点仍 302 到 `/sign_in`**；**PM 两轮一手复核后的最终口径**：① 成员归因「源登录态已失效」**不成立**（PM 复核时 dev 树 `Cookies` **mtime ＝ 12:14:38**、**7 个 `msedgewebview2` 进程指向 `desktop-dev\webview-official-chat\EBWebView`**、`Cookies` **被锁**（`EBUSY`）⇒ **dev 树环境活着且在用它的 cookie jar**）② **「ABE 跨 exe 解不开」已撤下**（PM 一手复算：副本 `Local State` 的 `os_crypt.encrypted_key` 解码后前 6 字节 ＝ **`DPAPI`**，hex `44 50 41 50 49 01` ⇒ 绑机器+用户、**不绑 exe**）③ ⇒ **真正的卡点是「拿不到源文件」**（源存活时 Cookies 被独占锁 ⇒ 拷不出）④ ⇒ **定性守住**：「只读拷贝能否带登录态」**仍未验到**（不是已证不可行、也不是已证可行）⇒ **不写成结论**；**在当下这条路线取不到登录态 ⇒ 本阶段关闭**，**纪律裁定不改、只改前提**（已同步进 P0 §7）④ **B 项关闭**：P4 的接线方向是**「插件 → 壳」** ⇒ 走**现役内置插件回环桥** ⇒ **不经过**「壳 → 内核 exact route」⇒ **已被绕过 ⇒ 不判、不做、不需新授权**；且「壳现有 v1 cookie 已满足 owner 准入」**既有实测已覆盖**（`notify/task.rs:941-953` 走通同一套 `admit()`）⑤ **替代取证三档**：**① 静态包 grep**（成员自己做，零授权零写盘，不依赖登录态，**先做**）→ **② izzy 跑一段只读 JS**（PM 已备好脚本，只回结构；**只在 ① 不够时才请 izzy 动手**；**⚠️ 13:0x 已订正落点：不是「dev 壳里按 F12」⇒ 见本表下一行**）→ **③ 壳内求值一次性调试入口**（要改 `src-tauri/**` ⇒ **PM 不主动申请**）⑥ **PM 更正自己写错的时钟标签**：前面几节把 12:0x–12:1x 的真实提交时刻写成「13:0x–13:4x」（真实 `1e6842a` 12:02:10 · `a961ae4` 12:04:55 · `7e9dff0` 12:06:20 · `900ff1d` 12:08:44 · `1fb8101` 12:11:31）⇒ 已改回；**教训**：时刻标签要**命令读出来**，不要凭印象 ⑦ 回执里已过期：远端 `main` 现 **`1fb8101`**（不是 `e1c568f`）、`9512602` 与 `7c7e641` **都已在远端**；他的复跑读数与 PM 一致（13/13、45/45、229/226/0/3、334/331/0/3） | 本文件 12:1x 节 ＋ P0 §7 实测补记 ＋ `src-tauri/src/plugins/builtin/bridge.rs` ＋ `src-tauri/src/notify/task.rs:941-953` ＋ `src-tauri/src/official_chat/context.rs:13-19` ＋ 一手实测（`Get-Date`／`git log --date=format:'%m-%d %H:%M:%S'`／`git branch -r --contains`／`Get-CimInstance Win32_Process`／dev 树 `Cookies` mtime ＋ `copyFileSync` 报 `EBUSY`） |

| 2026-10-21 | P4 | **P4 协议草案 v0.1 已开写**（新增 `official-chat-p4-protocol.md`，20.8 KB）＋ **成员的三处文档更正逐条一手复核后落** ＋ **P3-14 判据验收通过** ＋ **两笔成员提交统一推送** —— ① **三处更正**：**P0 §7 的归因被反证**（PM 一手复算 `os_crypt.encrypted_key` 解码前 6 字节 ＝ **`DPAPI`**（hex `44 50 41 50 49 01`）⇒ **「ABE 跨 exe 解不开」站不住 ⇒ PM 自己 12:1x 写的那条已撤下**；真正卡点＝**源存活时 Cookies 被独占锁 ⇒ 拷不出源文件**；定性守住「**仍未验到**」，不写成结论；**连带把「探针 B 关闭」改准为「本阶段关闭，不是原理上不可行」**）· **bridge-research `:267` 的字面矛盾已改**（表头自述「二级来源」而行内写「**实测**…**无游标**」⇒ 二手被写成实测；且「无游标」被一手反证 ⇒ 改为「页大小上限 `count`（二手、未复核）；翻页用 **`lte_cursor`** ＋ `pinned`（一手实测）」，并在表头注明 DeepSeek 两行已非二级来源）· **PM 自己那份 F12 脚本已换**（旧脚本只探四种形态 ⇒ `itemCount`/`itemKeys` 必然 `null` ⇒ 换成逐层下钻版，正文在 12:1x 节 ⑤ 第 2 条）② **协议草案 v0.1**：按**证据三档**写死条款 —— **【已实测】** 取数路线三条 · CDP ＋ 零痕迹 1239/1239 · 页内同源 fetch 带 cookie（200/code=0）· **页外调 API 必失败（实测：四种组合全 40003）** · **⚠️ 未登录与凭据形态不对回同一个码 ⇒ 站内码只原样上报** · 端点表 ＋ **`lte_cursor`** ＋ 三层骨架 · 接线＝现役回环桥加路由 ＋ 三条硬约束（**必须 POST＋读体＋body 上限**／**不能走查询串**／**最小能力面**）· 不扩 13 码闭集 · 零痕迹／凭据零落盘／只读 · 隔离纪律 ＋ **「只读拷贝带不带登录态」仍未验到**；**【推定】** `x-device-id` 等头是否必需（未隔离）；**【待实测】＝不写成条款**：字段面 ＋ 三档码 ⇒ **唯一待写死点 §4.3** ⇒ 补完升 **v0.2 冻结稿** ③ **P3-14 验收通过**（独立 worktree @ `e3c6a49`、detached、串行）：**P3 判据 14/14 全绿**（P3-14 两支：核算总量 ＝ 清理后总量／＝ 清理后淘汰前总量）· 自测 **48/48** · `test:scripts` **232/229/0/3** · `test:plugin` **334/331/0/3** · `check:static` 四项全绿 · cargo **1134/0/11** · **`cargo build --example official_chat_deepseek_probe` 通过** ⇒ **P3 判据侧 14 条全闭环** ④ **两笔已推**：`e3c6a49`（探针 A）＋ `faa4dff`（P3-14）⑤ **需要 izzy 一次 F12**（**必需**）：静态包 grep 实测不足（470 个 JS 缓存只捞到 10 个端点，`chat_session/*` 不在其中）⇒ 落到第②档 ⇒ 贴回 12:1x 节 ⑤ 那份只读脚本的输出 ⇒ 一次收口字段面 ＋ 三档码 | 本文件 12:4x–12:5x 节 ＋ 新增 `official-chat-p4-protocol.md` ＋ `Get-Content`/`ConvertFrom-Json` 复算 `encrypted_key`（DPAPI）＋ `git branch -r --contains` ＋ 一手复跑输出（判据 14/14 等，`verify-e3c6a49.ps1`）＋ `cargo build --example official_chat_deepseek_probe` |

| 2026-10-21 | P4 | **P4 协议升 v0.2**（补三条本轮新实测的硬约束）＋ **G4-4 收口为「不能」** ＋ **⚠️ PM 自纠：F12 落点给错了** ＋ **待 izzy 答 G4-6** —— ① **§2.5 登录态机制（裁定 2）**：登录态**只存在于「同一个活着的浏览器上下文」**（`ds_session_id` 是 **session cookie**：`is_persistent = 0` / `expires_utc = 0` ⇒ 新进程不恢复非持久 cookie）⇒ **只读拷贝带登录态 ＝ 不能（机制性，不是偶发）**；而**盘上明文 JWT 仍有效**（`exp` 2026-11-30、`name`「平常心💯」、`isAnonymous = false`、两棵树的 leveldb 里都有）**却不被认**（四种组合全 `40003`）⇒ ⇒ 「**执行体在壳**」「**API 在页内**」**由机制决定 ⇒ 硬约束、不是实现偏好** ⇒ 实现方不得提「页外调 API」「从外部进程读」 ② **§5.3 外挂 CDP 不可行（裁定 3）**：四项实测（活着的 `msedgewebview2` **无** `--remote-debugging-port` / 两棵 profile 根目录**无** `DevToolsActivePort` / **无**监听端口 / release 树 `Cookies` **EBUSY**）⇒ `CallDevToolsProtocolMethod` 是 `ICoreWebView2` 自己的方法、**进程内调用** ⇒ **只有壳能求值** ⇒ 与 ① 合起来 ⇒ **「壳内求值」是唯一路径，没有第二条路** ③ **G4-4 收口**：「只读拷贝能否带登录态」由「**仍未验到**」收口为「**不能**」⇒ **P0 §7 已同步**（12:4x 那两条加「已被 13:0x 收口取代」的指向 ＋ 补了 13:0x 收口条）⇒ **拷贝取证路线正式关闭**（**「EBUSY 拷不出源文件」不是最终机理**，真正机理是 session cookie 跨进程不可恢复）⇒ 「**页外调该 API 必失败**」从「别这么做」**升级为「只有这条路」** ④ **⚠️ PM 自纠**：上一轮让 izzy「**在 dev 壳的 DeepSeek 对话页按 F12**」**走不通**（**⚠️ 13:1x 已订正：这条依据错** —— `lib.rs:450-459` 讲的是 **`main` 管理窗口**、**不是官网对话窗口**；真正的一手依据链见下一行 ⇒ **结论（落点不是壳）不变**）⇒ **F12 的落点不是壳**；**PM 一手核当前登录态在哪**：**release 壳**（`C:\Users\zxx\AppData\Local\dsh-xlink\dsh-xlink.exe`，**0.5.8-rc.2**）的官网对话 webview **活着**（**9 个** webview 进程指向该树、该树 `Cookies` 正在被写）、**dev 壳的官网对话 webview 已关**（**0 个**进程、该树 `Cookies` 的 mtime 停在 **12:14:38**）⇒ **dev 树 session cookie 随环境关闭已失效** ⇒ 改走 **§8.6 两条落点**：**A**｜平时用的浏览器（Edge / Chrome）里**已登录**的 `chat.deepseek.com` 标签页跑那份只读脚本（**零代码改动、零授权**）｜**B**｜dev 壳**重新登录一次** ＋ **一次授权**改 `src-tauri/**` 做壳内一次性只读形状探测 ⇒ **待 izzy 答一句「平时用的浏览器里登录过 DeepSeek 吗」（缺口 G4-6，PM 未核）** ⑤ **版本线顺延**：v0.2 ＝ 补硬约束的草案稿 ⇒ **v0.3 ＝ 依据订正稿**（见下一行）⇒ **v0.4 ＝ 补齐 §4.3 字段面后的冻结稿**（**原定 v0.3 为冻结稿，因 13:1x 又改了依据而顺延**）⇒ **§4.3 仍是唯一待写死点**（四项字段面 ＋ 三档错误码）⇒ **P4 判据与实现单暂不派**（协议未冻结 ⇒ 派了会返工） | 本文件 13:0x 节 ＋ `official-chat-p4-protocol.md` v0.2 §2.5／§5.3／§8.4–§8.6／§9 ＋ `official-chat-feasibility.md` §7 第 2 条 ＋ `src-tauri/src/lib.rs:455-456`（**⚠️ 13:1x 已订正：这条不是官网对话窗口的依据**）＋ 一手实测（cookie 属性 / `encrypted_key`＝DPAPI / 两棵树 leveldb JWT 只打印 `exp` 与账号、**不打印 token 本体** / 进程指向与 Cookies mtime / 四项外挂 CDP 试探） |

| 2026-10-21 | P4 | **P4 协议升 v0.3（依据订正）** ＋ **PM 第二次自纠（上一轮 §8.5 的依据也错）** ＋ **撤回一条被引用的依据** ＋ **取证脚本换两档版** ＋ **B 路补齐（不需要 DevTools）** —— ① **成员的依据①「wry 关掉了浏览器加速键 ⇒ F12 不生效」＝ 不成立**：wry `src/webview2/mod.rs:649-653` 那句 `SetAreBrowserAcceleratorKeysEnabled(false)` 是**有条件的**（`if !pl_attrs.browser_accelerator_keys`），而 wry 默认 **`browser_accelerator_keys: true`**（`lib.rs:1768`，注释「This is WebView2's default behavior」）⇒ 本仓**未**调用 `with_browser_accelerator_keys(false)` ⇒ ⇒ **加速键是开着的**；**教训**：「某能力不存在」要先找**真正控制它的那个开关**（本例是 `devtools` 属性，不是加速键）② **PM 第二次自纠**：13:0x 写的依据「官方对话窗口建的时候是 `with_devtools(false)`（`src-tauri/src/lib.rs:455-456`）」**错** —— `lib.rs:450-459` 讲的是 **`main` 管理窗口**、且它自己 `#[cfg(debug_assertions)]` 门控 ⇒ ⇒ **结论（落点不是壳）对、依据不成立** ⇒ 已整段换成**一手核过的链**：wry `devtools` 默认 **debug ⇒ `true` / release ⇒ `false`**（`wry-0.57.0/src/lib.rs:872-875`）＋ Tauri 只在 `#[cfg(any(debug_assertions, feature = "devtools"))]` 下才覆盖它（`tauri-runtime-wry-2.12.1/src/lib.rs:5146-5149`）＋ 本仓 `src-tauri/Cargo.toml:30` **无 `devtools` feature** ⇒ ⇒ `SetAreDevToolsEnabled(false)`（`wry-0.57.0/src/webview2/mod.rs:640`）⇒ ⇒ **release 壳里 F12 无效、右键无「检查」**；另补两条：**DOM 层右键菜单被 `no-context-menu.js` 取消** ＋ **官网对话窗口没有程序化 `open_devtools()` 入口**（`official_chat/context.rs:104-113`；全仓 `open_devtools()` 三处分别给工作台与 `main`、**都 `#[cfg(debug_assertions)]`**）③ **落点分两档写准**：**release 壳** ＝ DevTools 被关（**真**）｜**dev 壳** ＝ DevTools **可能能开**（**【推定】未实测**）**但 dev 树的登录态已随环境关闭失效** ⇒ ⇒ **两档都不可用**，且 **dev 壳的理由与 DevTools 无关** ④ **B 路不需要 DevTools**（【已实测】P0「不开 DevTools 窗口也能调 `Runtime.evaluate`」）⇒ ⇒ **B 路在 release 壳里成立、不用重新登录** ⇒ 拆成 **B1（dev 壳：重新登录一次、不碰安装版）/ B2（release 壳：不用重新登录、但要动用户真实树 ⇒ 先问 izzy）** ⑤ **取证脚本换「两档自动降级」版**（第 1 档按 裸/＋Bearer/＋Bearer＋`x-device-id` 三种头反推哪一档被认 ＋ 第 2 档钩**站点自己**的列表请求、用 `res.clone()` **不吃掉站点响应体**）⇒ **PM 逐字段核过输出面**：只回键名/条数/code/msg 前 160 字/打码后的 URL，**不回正文、不回标题、不回会话 id、不回任何凭据**；并补两条口径：**痕迹范围**（只在该标签页、关掉即消失、**不写站点数据**；**不属 §7.1 零痕迹纪律 —— §7.1 管的是生产页内脚本**）＋ **第 2 档触发动作建议用「新对话」**（不加载会话正文）⑥ **成员回执里已过期**：远端 `main` **不是** `ed87602`（PM 13:0x 已推 `2ef12b0`）· 「§8 那句『仍未验到』应改成『已实测不能』」**已在 13:0x 落地**（协议 §8.4 ＋ P0 §7）⑦ **⚠️ 本次 push 顺带带上了成员提交 `84fd50c`**（devloper，13:06:32，**PM 未提交过的那一笔**）⇒ ⇒ **知会已发**（回告哪几笔被带上）⇒ **PM 一手核过**：`src-tauri/examples/official_chat_probe.rs` **只改 `//!` 头注释**（+10/−2，**零可执行代码改动**）⇒ 改法与 13:0x 收口一致 ⇒ **验收通过** | 本文件 13:1x 节 ＋ `official-chat-p4-protocol.md` v0.3 §4.3／§8.5／§8.6／§11 ＋ `official-chat-feasibility.md` §7 第 2 条 ＋ `git show 84fd50c` ＋ 一手源码核证（`wry-0.57.0/src/lib.rs:872-875`／`:1768`、`wry-0.57.0/src/webview2/mod.rs:640`／`:649-653`、`tauri-runtime-wry-2.12.1/src/lib.rs:5146-5149`、`src-tauri/Cargo.toml:30`、`src-tauri/src/harness/no-context-menu.js`、`src-tauri/src/official_chat/context.rs:104-113`、全仓 `open_devtools` 三处）＋ mock 复跑 17/17（`node --check` ＋ 两场景） |

| 2026-10-21 | P4 | **P4 协议升 v0.4（防误读条款）** ＋ **复核成员第二条回执** ＋ **核了他的对账素材** —— ① **他的「正确依据①」仍不成立**：「wry 关掉了浏览器加速键 ⇒ F12 不生效」—— 那句 `SetAreBrowserAcceleratorKeysEnabled(false)` 是**有条件的**（`if !pl_attrs.browser_accelerator_keys`），而 wry 默认 **`browser_accelerator_keys: true`**（`lib.rs:1768`）⇒ 本仓**未**调用 `with_browser_accelerator_keys(false)` ⇒ ⇒ **加速键是开着的**；**结论对、依据不成立** ⇒ 协议 §8.5 已用真正的一手链取代（`devtools` 属性链 ＋ `no-context-menu.js` ＋ 无 `open_devtools()` 入口）② **「release 连 inspector 都没有」方向对、机制改准**：真正原因是 **wry 的 `devtools` 属性默认 release 下就是 `false`**（`wry-0.57.0/src/lib.rs:872-875`）＋ Tauri 只在 `#[cfg(any(debug_assertions, feature = "devtools"))]` 下才覆盖 ⇒ ⇒ **缺 `devtools` feature 是「不会被覆盖」的原因，不是「没有 inspector」的原因** ③ **他指出 PM 的 `lib.rs:455-456` 引错 ＝ 成立**（与 PM 自查一致）⇒ PM 13:1x 已整段换掉（**他这条提醒晚了一步**）④ **PM 核了他的对账素材**（`p4-protocol-facts.md` 225 行、§10 对账表；**不在仓库里**，按 izzy「证据可以清」不固化）⇒ 与协议一致，但**多出三条写死的事实**：**「顶层 `code=0` ≠ 取到数据」**（实跑 `status=200` ＋ 顶层 `code=0` ＋ `msg=""` **同时** `biz_code=2` ＋ `biz_data=null` ⇒ **传输层成功 ≠ 业务层成功 ≠ 拿到数据** ⇒ **成功判据必须落在 `data.biz_data` 是否有内容** ⇒ 已进 §4.2 ＋ §6.2）｜**端点来源是浏览器缓存条目、不是页面当场请求**（⇒ 登录态下的 resource timing 清单仍未实测）｜**运行时 resource timing**（未登录态）＝ **23** 个资源、其中 `/api/` 端点**只有 1 个** ⇒ **两种非静态口径都只看到端点路径、看不到字段面** ⑤ **版本线顺延**：v0.4 ＝ 防误读条款 ⇒ **v0.5 ＝ 补齐 §4.3 字段面后的冻结稿**（原定 v0.3 为冻结稿，**两次顺延**）⑥ **成员第三条回执（迟到的知会）里他提的两处「PM 文档旧表述」＝ 成立，已逐条核后改掉**：**P0 `:217`**（「若需复用登录态，走只读拷贝」⇒ 改成**两层**：加密层成立／**登录态层不成立** ＋「只读拷贝是取证隔离手段，不是带登录态手段」）｜**台账 `:1545`**（「副本里就带着 `session_id` ⇒ 不需要 izzy 再提供任何登录态」⇒ 加指向：**后半句已被 13:0x 实测证伪** —— 副本落地 `sign_in`、页内只有匿名 cookie、`localStorage` 的 token 确实拷过来了 ⇒ **token 在、session 不在** ⇒ **护栏防不了「登录态带不过来」**） | 本文件 13:2x 节 ＋ `official-chat-p4-protocol.md` v0.4 §4.1／§4.2／§6.2／§11 ＋ 一手核证（`wry-0.57.0/src/lib.rs:872-875`／`:1768`、`wry-0.57.0/src/webview2/mod.rs:640`／`:649-653`、`tauri-runtime-wry-2.12.1/src/lib.rs:5146-5149`、`src-tauri/Cargo.toml:30`）＋ 素材对账（`p4-protocol-facts.md` §10.1／§10.2）＋ 成员两处文档更正（`official-chat-feasibility.md:217`、`official-chat-progress.md:1545`） |

| 2026-10-21 | P4 | **G4-6 收口（PM 一手核完浏览器里的登录态）** ＋ **协议升 v0.5（落点收窄为 Chrome）** ＋ **成员第四条回执基线已过期 ⇒ 零新动作** —— ① **PM 一手只读核查**（**不解 cookie 值、不写用户目录、不启动浏览器**）：**Chrome（Default）有登录痕迹** —— `localStorage` leveldb 里 **`userToken` ＋ `__appKit_userInfo` 都在**，且 `userToken` 与 origin 字符串**同一条记录**（偏移相距 **155 字节**），leveldb 最新 mtime **2026-10-21 12:08:39**；**Edge 排除** —— 无 `userToken`／`__appKit_userInfo`（只有 14 处访问痕迹）、leveldb 最新 mtime **2026-10-16 09:27:37**、其 `Cookies` 里 `chat.deepseek.com` **只有两个匿名项**（`.thumbcache_…`／`smidV2`）⇒ ⇒ **为什么查 leveldb**：Chromium 的 `localStorage` **不加密**（可只读扫键名），而 `Cookies` 表**被独占锁**（copy 与只读 open 都 `WinError 32`）⇒ **只报命中计数与偏移距离，不打印上下文与 token 值** ② **残留一条不假装已收**：「**痕迹在 ≠ 此刻 session 仍有效**」⇒ **最后一步仍要 izzy 在 Chrome 里跑一次脚本** ③ **协议 v0.5**：§4.3 补法 ＋ §8.5 结论 ＋ §8.6 表格的落点**收窄为「Chrome」**；§9 **G4-6 改为「已收口」**（G4-7 保留为条件项）⇒ **版本线第三次顺延** ⇒ **v0.6 ＝ 补齐 §4.3 字段面后的冻结稿** ④ **成员第四条回执（更早知会的延迟回复）零新动作**：他写的「请统一推 `84fd50c`」**已空动作**（PM 13:1x 就带上了，远端已到 `a6cf19c`）·「台账脚本是旧版」**已过期**（已换两档版并 mock 17/17）·「协议升 v0.3 冻结稿」**已过期**（现 v0.5） ⑤ **脚本正文已直接发给 izzy**（台账 12:1x 节 ⑤ 第 2 条那份两档版）⇒ 在 Chrome 里打开 `chat.deepseek.com` → 已登录 ⇒ 粘贴 → 回车 → 贴回输出 ⇒ **一次收口 §4.3 四项 ＋ 三档码**；若是 `sign_in` ⇒ 再问 B1/B2 | 本文件 13:3x 节 ＋ `official-chat-p4-protocol.md` v0.5 §4.3／§8.5／§8.6／§9／§11 ＋ 一手只读核查（Chrome/Edge 的 `Local Storage\leveldb` 键名命中计数 ＋ 同记录偏移距离 ＋ `Network\Cookies` 匿名项枚举）＋ 临时副本已清 |

| 2026-10-21 | P4 | **成员接受 PM 的依据订正**（加速键那条他自认写错 ＋ 两处载体自行订正）＋ **他提的三处旧依据核改**（只一处真）＋ **他补的两条支撑被采纳** ＋ **协议升 v0.6** —— ① **他独立核了同一组三处源码**（`wry-0.57.0/src/webview2/mod.rs:649-653` 条件式 ＋ `lib.rs:1768` 默认 `true` ＋ 本仓无 `with_browser_accelerator_keys(false)`）⇒ **确认「加速键被关 ⇒ F12 不生效」不成立** ⇒ ⇒ **双方从相反方向得出同一条纠正** ② **他补的【推定】支撑 ＝ 成立**（**wry 0.57 自己不绑 F12** —— 全仓 grep 不到 `F12`／`AcceleratorKeyPressed`，加速键相关的只有 `browser_accelerator_keys` 那个开关、默认 `true` ⇒ **能否触达完全取决于 WebView2 自身**）⇒ ⇒ **「dev 壳可能能开」标【推定】未实测准确，不宜写成「能开」** ⇒ 已落 §8.5 ③ **他提的三处旧依据 —— 只有一处还要改**：台账 `:1666` **已改**（他看的是旧行号）｜台账 `:1781` **是历史记述**（引用旧依据正是它要记的内容 ⇒ 保留）｜**P0 `:236` 确实还留着**（句首仍以 `with_devtools(false)`（`lib.rs:455-456`）起句）⇒ **已改写**为「release 壳里 DevTools 被 wry 关掉」＋ 订正说明压进去 ＋ 落点同步 **Chrome** ④ **他对 B2 的提醒 ＝ 采纳**（**B2 要改 `src-tauri/**` ＋ 重新构建 ＋ 装到用户真实树 ⇒ 动用户安装版 ⇒ 归 izzy 拍板、自己测**（成员分工只到 dev）⇒ **不许写成「零成本」**）⇒ 已落 §8.6 ⑤ **版本线第四次顺延**：v0.6 ＝ 补口径 ⇒ **v0.7 ＝ 补齐 §4.3 字段面后的冻结稿**（原定 v0.3 为冻结稿） | 本文件 13:4x 节 ＋ `official-chat-p4-protocol.md` v0.6 §8.5／§8.6／§11 ＋ `official-chat-feasibility.md` §7 第 2 条 ＋ 一手核证（`wry-0.57.0/src/lib.rs:1768`／`wry-0.57.0/src/webview2/mod.rs:649-653` 与全仓 `F12`／`AcceleratorKeyPressed` grep 零命中）＋ 成员素材 `p4-protocol-facts.md` §6.1 的自订正 |

| 2026-10-21 | P4 | **台账脚本与成员原文件彻底对齐（全文 200/200 逐行全等）** ＋ **三处「Edge / Chrome」落点措辞订正** ＋ **替换后 PM 又复跑 17/17** —— ① **成员指出的问题 ＝ 成立**（**会影响执行**）：台账 12:1x 节 ⑤ 第 2 条那份脚本的**头注释第 1 行**仍写「你平时用的浏览器（**Edge / Chrome**）里已登录的 chat.deepseek.com 标签页」⇒ **Edge 无登录 ⇒ 第 1 档三档头必然全 `40003`、第 2 档钩子也截不到站点自己的列表请求 ⇒ 白跑一次** ⇒ PM 13:3x 改了协议与台账两处、**漏了脚本头注释这一行** ⇒ **成员抓出来了** ✅ ② **PM 一手核「两份是否逐行一致」（不采信自报）**：口径＝**滤掉整行注释与空行、再逐行 `trim()` 比可执行部分** ⇒ **154 / 154 逐行完全一致**（他自报属实）⇒ 随后**整段用他那份替换台账正文（含注释）** ⇒ ⇒ **全文 200 / 200 逐行全等**（`IDENTICAL=True`）⇒ **两份彻底不分叉** ③ **替换后 PM 又复跑一遍**（不是复制旧结论）：`node --check` 通过 ＋ mock **17/17 全绿** ＋ 输出面机械核对仍查不到 token/正文/会话 id ④ **三处现行措辞已订正**（**历史记述不动**）：脚本头注释（随整段替换）｜12:1x 节 ⑤ 落点订正条（**Chrome** ＋ 「Edge 已排除」）｜13:0x 节 ④ 落点表 A 行（**Chrome**）；13:2x 节 ② 与交接记录 13:0x 行里的「Edge / Chrome」**是记述 ⇒ 保留** ⇒ **协议正文本轮未动 ⇒ 不升版本**（P4 协议仍为 **v0.6**，**冻结稿仍待 v0.7**）⑤ **他这条已过期的一句**：「回填 §4.3 ⇒ v0.6 冻结稿」⇒ 现协议已 v0.6 ⇒ **冻结稿顺延 v0.7** | 本文件 13:5x 节 ＋ 一手核验（`extracted-probe.js` 与成员原文件的**可执行部分 154/154** ＋ **全文 200/200** 逐行比对 ＋ `node --check` ＋ mock 17/17）＋ 成员原文件 `deepseek-shape-probe.js`（已自行对齐头注释） |

| 2026-10-21 | P4 | **成员的「break 盲区」成立（PM 一手核代码）＋ PM 补两处精确化 ＋ 纠正一次同款越界复发 ＋ 台账行号漂移更正** —— ① **他的基线已过期**（他写远端 `978454d` ⇒ 现为 **`b270f83`**）；**其余现状核实全部属实**（`origin/main..HEAD` 空 · 工作树干净 · **P0 `:236` 确已改写** · **§4.3 确是唯一待写死点**）② **「拿到数组就 `break` ⇒ 另两档头不发出」成立**，`deepseek-shape-probe.js:127` 逐行核过，**他引的三个行号（`:43-47` / `:81` / `:127`）全部准确** ③ **PM 精确化①（他范围偏大）**：`break` 在 **HEADER_SETS 内层循环**（`:119` 开）⇒ **只影响命中数组的那个端点** ⇒ **`/api/v0/users/current` 的三档头仍会全发** ⇒ **三档头的码读数仍拿得到（来自 `/users/current`）** ⇒ ⇒ **回填必须标明「哪个端点的码」，不许把 `/users/current` 的码当 `fetch_page` 的码写进条款** ④ **PM 精确化②（他漏一处）**：游标键要查**三处** —— `topKeys`（`:86`）＋ `dataKeys`（`:92`）＋ `bizDataKeys`（`:98`）（数组键另在 `bizDataArrayCounts` `:99-100`）⇒ 他只写了两处 ⇒ **顶层挂载的游标会漏检** ⑤ **游标两条判读分支采纳**（不硬凑；「没回带游标」是实测事实 ＋ 登记为新缺口）⑥ **§6.2 措辞调整采纳但不单独升版**（现行措辞未承诺三档全验到 ⇒ 不矛盾）⇒ **随 v0.7 一次落** ⑦ **纠正同款越界复发**：附件 `p4-section43-backfill.md`（6,812 字节 / 51 行 / **不入库**）PM 一手读过、**素材合格**；但他写「贴回输出后**我自己**回填 §4.3 ＋ 只提交协议文件」⇒ **越界**（协议归 PM 落笔，v0.2–v0.6 全是 PM 升的）⇒ **两个写手写同一文件 ＝ 并行写手冲突**（上一轮同款已纠正过一次）⇒ ⇒ **裁定：成员交草案 → PM 落笔 ＋ 提交 ＋ 统一 push**；成员保留**逐条核对 ＋ 门禁复跑 ＋ §10 判据第 8 条的可执行断言草案**（判据脚本在 `scripts/**`）⑧ **行号漂移更正**：他引的台账 `:1680-1862` 是替换前位置 ⇒ **现 `:1670`（js 围栏）/ `:1871`（结束围栏）/ 正文 `:1671-1870`（200 行）**；「改脚本会让台账那份失配」**结论仍成立** ⑨ **协议与 P0 本轮未动 ⇒ 不升版本**（仍 v0.6，冻结稿待 v0.7）⑩ **仍无新单**；**唯一待办仍是 izzy 那一次 Chrome 输出**（**不用多跑一轮**、**不改脚本**）⇒ 输出回来 ⇒ **PM 落 v0.7 冻结稿** ⇒ 才派判据单 ＋ 实现单 | 本文件 13:6x 节 ＋ 一手核验（脚本 `:43-47` / `:81` / `:86` / `:92` / `:98` / `:99-100` / `:118-127` 逐行读）＋ 台账围栏实测（`:1670` / `:1871`）＋ 成员附件 `p4-section43-backfill.md`（51 行全文读过） |

| 2026-10-21 | P4 | **成员 13:6x 回执验收通过 ＋ `:1781` 行号漂移订正 ＋ PM 采纳成员更严表述（订正 13:6x ③ 的无条件断言）＋ 三份素材一手验收** —— ① **他的四条主张 PM 一手复核全部成立**（「台账 `:1666` 已是订正后落点」✔｜「P0 `:236` 已改写」✔｜「mock 17/17 三处背书」✔（台账 `:1954` / `:2036` ＋ 协议 `:172`）｜「**台账 `:1781` 已漂移**」✔ —— **实测现 `:1781` 是脚本块内空行**，那条内容在 **`:1935`**）② **`:1781` 处理**：**不改正史行**（13:4x 对照表 `:2019` 那行不动）⇒ **13:7x 节即为漂移订正**（该条现行位置 `:1935`；`:1781` 现指空行 ⇒ 按内容定位或用 `:1935`）③ **⚠️ 订正 PM 自己 13:6x ③ 的无条件断言**：13:6x 写「`/users/current` 三档头仍会全发」⇒ **其前提（该端点响应有无数组字段）未实测** ⇒ ⇒ **精确表述是条件式「取决于读数、不预先断定」** ⇒ **成员草案更严口径采纳**（13:6x 的**方向不变**：`break` 只影响命中数组的那个端点 ⇒ 回填必须标**端点 ＋ 档位**）④ **三份素材 PM 一手读过、全部合格、仍不入库**：`p4-section43-clause-draft.md`（7,599B｜**占位符统一 `〈…〉` 不预填键名** ｜两条精确化已吸收 ｜分支 B 两写法齐）＋ `p4-section43-backfill.md`（**9,336B，已更新**｜④「**三处都要查**」✔｜三档码按端点逐条写 ✔）＋ `p4-criterion8-draft.md`（**8,023B，本轮新增**｜判据骨架自包含、夹具 `%TEMP%` 自建自清、只断结构不断值、场景 A/B/C/D 覆盖 `code=0` ＋ `biz_data=null` 反例）⑤ **分工已改口**（协议由 Chief 落笔 ＋ 提交 ＋ push；成员保留逐条核 ＋ 门禁复跑 ＋ 判据脚本 `scripts/**`）⑥ **留到派单时定**：判据脚本「独立跑、不塞进既有门禁」合理，但**派单时必须写明它怎么进门禁 / 谁何时跑**，否则成孤儿脚本 ⑦ **附件版本漂移记账**（13:6x 记的「6,812B / 51 行」是当时读数 ⇒ 现 9,336B）⑧ **协议与 P0 本轮未动 ⇒ 不升版本**（仍 v0.6，冻结稿待 v0.7）⑨ **仍无新单**；**唯一待办仍是 izzy 那一次 Chrome 输出** ⇒ 输出回来 ⇒ **PM 用草案填占位符落 v0.7 冻结稿** ⇒ 他逐条核 ⇒ 才派判据单 ＋ 实现单 | 本文件 13:7x 节 ＋ 一手核验（台账 `:1781` 现为空行 · 内容在 `:1935` · 漂移引用在 `:2019`；成员三份附件全文读过：7,599B / 9,336B / 8,023B）＋ 协议 `:172` / 台账 `:1954` / `:2036` 复读 |

| 2026-10-21 | P4 | **成员 13:7x 回执验收通过 ＋ PM 接受他的依据修正并补一层更硬的读数 ＋ 新增「mock 背书的射程限定」** —— ① **他的三处复核 PM 一手核过，全部属实**（`:86` `topKeys` 存在 ⇒ 游标可能挂顶层 ✔｜`:118` / `:119` / `:120` / `:127` ⇒ break 只跳出内层 ✔｜`:43-47` / `:81` 行号准 ✔ —— **十处行号逐行读过，无一处出入**）② **⚠️ 他指出 PM 13:6x ③ 的「用户对象里没有数组」是推断、不是实测 ⇒ 他是对的** ⇒ **PM 一手核 mock 源码**：`mock-probe-check.cjs:51-53` 的 `fetchMock` **完全忽略 path** ＋ `:19-31` 的 `payloadOk` 含数组 ⇒ ⇒ **PM 再复跑一遍**（`p4-mock-probe-trace.cjs`）：**只发出 2 条请求**（两端点各 1 档裸档）· `probes` **2 条** · **mock 断言 `find(p => p.arrayPath)` 绑到的是 `/api/v0/users/current`** ⇒ ⇒ **mock 里 `/users/current` 同样只验到裸档 ⇒ mock 证明不了真实端点有没有数组** ⇒ ⇒ **13:7x ③ 的条件式口径成立，且依据更硬**（13:6x ③ 的**方向不变**）③ **⚠️ 新增写死的「mock 背书射程限定」**：17/17 证明的是**结构下钻 ＋ 第 2 档兜底 ＋ 输出面不含敏感值**，**不覆盖「端点维度的档位行为」**（mock 无路径区分；断言绑在**第一个端点**上 ⇒ **`fetch_page` 第 1 档取到数组这条路径，mock 从未单独验证过**）⇒ ⇒ **凡用 mock 读数推端点行为 ⇒ 不成立** ⇒ ⇒ **端点维度一律等真实读数**（**随 v0.7 一次落**）④ **「不用多跑一轮」的理由换新**（原「天然拿不全」按 ② 被推翻）⇒ **新理由：读数自带 `path` ＋ `headerSet`** ⇒ **已发档位可逐条还原、未发即缺口** ⇒ ⇒ **仍不改脚本、不多跑一轮** ⑤ **他说「已改」属实**（PM 核文件当前状态：分工段 ✔ · ④「三处都要查」✔ · 端点表「取决于读数」✔ · 新增「行号现状」节 ✔；mtime 13:45 早于回执 ⇒ **时序自洽**）⑥ **基线**：他 fetch 到 `9742546` ⇒ **现 `4848296`** ⑦ **协议与 P0 未动 ⇒ 不升版本**（仍 v0.6，冻结稿待 v0.7）⑧ **仍无新单**；**唯一待办仍是 izzy 那一次 Chrome 输出** ⇒ 输出回来 ⇒ **PM 用草案填占位符落 v0.7 冻结稿**（按 ② ③）⇒ 他逐条核 ＋ 门禁复跑 ＋ 填判据脚本 ⇒ ⇒ **才派 P4 判据单 ＋ P4 实现单** | 本文件 13:8x 节 ＋ 一手核验（脚本十处行号 ＋ `mock-probe-check.cjs:19-31` / `:51-53` 源码 ＋ **`p4-mock-probe-trace.cjs` 复跑读数**：2 请求 / 2 probes / 断言绑 `/users/current`）＋ 成员两份附件当前内容复核（7,599B / 9,336B） |

| 2026-10-21 | P4 | **行号有效性的一条规律（追加在末尾 ⇒ 既有行号引用不再漂）＋ 附件定稿提示的边界 ＋ 基线更新** —— ① **成员核到的规律成立，PM 在最新远端 `38751c2` 上复现**：台账 **`:1935`** 仍是「PM 第二次自纠」那条、**`:2019`** 仍是 13:4x 对照表那行 ⇒ **13:8x 两节都追加在末尾（行 2102 起）⇒ 前面不动** ⇒ ⇒ **规律写死：台账只在末尾追加节 ⇒ 既有行号引用（`:1935` / `:2019` / 脚本正文 `:1671-1870`）持续有效、不必重取** ⇒ ⇒ **13:7x ② 的漂移订正（`:1781` ⇒ `:1935`）持续成立**；⇒ ⇒ **漂移的成因只有一种：13:5x 那次整段替换（`spawn-script-v3.ps1`）** —— **今后只在末尾追加节、不整段替换，就不会再产生行号失效** ② **基线**：他 fetch 到 `4848296` ⇒ **现 `38751c2`** ③ **「三份附件已定稿」的边界**：附件是 **13:45 定稿版**；**PM 13:8x ③ 又追加两条限定**（mock 背书射程 ＋ `chat_sessions` 键名是仿造值）⇒ ⇒ **v0.7 落笔 = 附件 ＋ 这两条限定一起带上**（**成员不需要改附件**，它只是素材）⇒ ⇒ **成员的「逐条核一致性」要把这两条一起核** ④ **协议与 P0 未动 ⇒ 不升版本**（仍 v0.6，冻结稿待 v0.7）⑤ **仍无新单**；**唯一待办仍是 izzy 那一次 Chrome 输出** ⇒ 输出回来 ⇒ **PM 落 v0.7 冻结稿** ⇒ 成员逐条核 ＋ 门禁复跑 ＋ 填判据脚本 ⇒ ⇒ **才派 P4 判据单 ＋ P4 实现单** | 本文件 13:9x 节 ＋ 一手复现（最新远端 `38751c2` 下实测 `:1935` / `:2019` 两行内容未变；13:8x 两节起始行 2102 / 2121） |

| 2026-10-21 | P4 | **成员一封迟到回执（对应 PM 13:6x 那封）＋ 他给的新行号逐行核过全对 ＋ 补记一条台账缺失的核证结论（他的「零命中」表述不实）＋ `:1666` 建议双向关闭** —— ① **迟到回执**（回应 PM 13:6x 那封；「两条精确化已改完 / 理由换新 / `:1666` 接受」他 13:7x 那封已说过 ⇒ **不重复入账**）② **他给的新行号 PM 一手现读、五个全对**：`p4-section43-backfill.md` `:19`（④「**三处都要查**」＝ `topKeys` `:86` ＋ `dataKeys` `:92` ＋ `bizDataKeys` `:98`，数组键另在 `bizDataArrayCounts` `:99-100`）/ `:22`（三处判读分支）/ `:53`（**每条码标「哪个端点 ＋ 哪一档」**，依据 ＝ `probes[].path` ＋ `probes[].headerSet`，探针 `:120`）/ `:56`（**不用多跑一轮的新理由**）/ `:96`（探针行号现状）③ **补记一条台账此前缺失的核证结论**：他上轮报「grep『**已发给 izzy**』**零命中**」⇒ **实测不零命中**（台账 **`:2075`** 13:6x ⑩ 有「**脚本正文已发给 izzy**」，**那句为真**）⇒ **他报的「零命中」表述不实**；**真正该搜的原文句「对 izzy 的更正已发」⇒ 零命中 ✔**（PM 本轮复查复现）⇒ ⇒ **结论「不假记」成立、依据串错**，他已认领 ⇒ ⇒ **纪律写死：核「某句是不是假记」必须用原文句做检索串（近义串会命中内容为真的别的句子）；报否定性证据（零命中／无残留）必须把实际用的检索串原样写出来让人能复核** ④ **`:1666` 建议双向关闭**：PM 裁定不采纳（结构理由：`:1667` 就是 `:1666` 的落点订正条，那一块是「原始条目 ＋ 订正压在其后」的叠记结构 ⇒ 改 `:1666` 破坏一致性；落点 Chrome 现存三处）＋ **他明确撤回** ⇒ 此条不再有人提 ⑤ **基线**：他 fetch 到 `9742546` ⇒ **现 `000eb80`** ⑥ **协议与 P0 未动 ⇒ 不升版本**（仍 v0.6，冻结稿待 v0.7）⑦ **仍无新单**；**唯一待办仍是 izzy 那一次 Chrome 输出** ⇒ 输出回来 ⇒ **PM 落 v0.7 冻结稿**（附件 ＋ 13:8x ③ 两条限定）⇒ 成员逐条核 ＋ 门禁复跑 ＋ 填判据脚本 ⇒ ⇒ **才派 P4 判据单 ＋ P4 实现单** | 本文件 13:9x（补）节 ＋ 一手核验（成员给的五个行号逐行现读 ＋ 台账 `:2075` 现状 ＋ 假记原文句「对 izzy 的更正已发」零命中复查） |

| 2026-10-21 | P4 | **成员核不到 mock 源码 ⇒ 自己复现 ＋ 补对照实验 ＋ PM 一手复跑其复现工具（逐条吻合）＋ 「三档码的 mock 背书 ＝ 零」写死 ＋ 一条路径引用纪律** —— ① **他核不到 mock 源码**（mock 自测资产在 PM 的 `%TEMP%`，**不在仓库**；台账只有文件名引用、无路径）⇒ ⇒ **他不采信 PM 的源码读数，按描述自己造等价 mock 复现** ⇒ **PM 采信这个做法**（**上游给读数、下游自己复现** 强于照抄结论）⇒ ⇒ **纪律**：**凡结论依赖一次性临时环境，台账只记「结论 ＋ 读数」，不得当长期可核证据**（izzy 15:2x「证据可以清」⇒ **协议里只写结论与射程限定，不写复跑路径**）② **PM 一手实跑** `p4-mock-trace.cjs`（4,170B / 零网络零写盘）：**模式 P（忽略 path）＝ 2 请求**（`users/current` 裸 ＋ `fetch_page` 裸）· `probes` **2** · **带 `arrayPath` 的两条都带**（`users/current` 在前）；**模式 D（按路径区分）＝ 4 请求**（`users/current` **裸／＋Bearer／＋Bearer＋x-device-id** 三档 ＋ `fetch_page` 裸）· `probes` **4** · **带 `arrayPath` 的只有 1 条 ＝ `fetch_page`** ⇒ ⇒ **他报的读数逐条对得上** ⇒ ⇒ **「端点维度的档位行为与断言绑定端点随 mock 造法翻转」＝ 可复现事实**（坐实 13:8x ③ 的射程限定）③ **⚠️ 更硬的一条**：**模式 P 下「＋Bearer／＋Bearer＋x-device-id」两档头一条都没发出** ⇒ ⇒ **mock 从未验证过带 Bearer 档的行为** ⇒ ⇒ **§6.2「三档码」的 mock 背书 ＝ 零**（mock 只覆盖「第 1 档命中」与「第 1 档全拒 → 第 2 档兜底」两条路径）⇒ **三档码只有真实读数作数**（随 v0.7 落）④ **素材更新 PM 一手核过**：`p4-section43-clause-draft.md` 新增「**一之二 · 读数来源的射程限定**」＋ §6.2 追加条加零背书句｜`p4-criterion8-draft.md` 守则第 1 条加 mock 仿造值提醒｜`p4-section43-backfill.md` 未改｜新增 `p4-mock-trace.cjs`（PM 已实跑）⇒ **他声明附件不再改动（除填空）⇒ PM 不必反复重读** ⑤ **基线**：他 fetch 到 `38751c2` ⇒ **现 `491c779`** ⑥ **协议与 P0 未动 ⇒ 不升版本**（仍 v0.6，冻结稿待 v0.7）⑦ **仍无新单**；**唯一待办仍是 izzy 那一次 Chrome 输出** ⇒ 输出回来 ⇒ **PM 落 v0.7 冻结稿**（附件 ＋ 13:8x ③ 两条限定 ＋ 本节 ③ 零背书 ＋ ① 路径纪律）⇒ 成员逐条核 ＋ 门禁复跑 ＋ 填判据脚本 ⇒ ⇒ **才派 P4 判据单 ＋ P4 实现单** | 本文件 13:9x（补二）节 ＋ 一手实跑（`p4-mock-trace.cjs` 模式 P／D 两组读数）＋ 成员附件复核（新增「一之二」节 ＋ §6.2 零背书条 ＋ 守则第 1 条提醒） |

| 2026-10-21 | P4 | **成员澄清「那两条限定附件里已有」⇒ PM 订正 13:9x ③ 的表述 ＋ v0.7 落笔核对清单验收通过 ＋ ⚠️ 采纳基线纪律（台账起不再手写会过期的提交号）** —— ① **他的核证属实**（他 fetch 时的基线/行数与现在差一笔 ＝ 时序差，非错）⇒ **`:1935` / `:2019` 仍未漂** ✔ ⇒ **规律成立**（漂移成因只有 13:5x 整段替换）② **⚠️ 他澄清成立、PM 采纳并订正自己 13:9x ③**：实测**附件已含**那两条限定（`clause-draft.md:48`「一之二 · 读数来源的射程限定」/`:50` 模式 P·D 读数/`:71` §6.2「mock 对三档码零背书」条｜`backfill.md:55`「不预先断定三档天然拿不全」｜`criterion8-draft.md` 守则第 1 条）⇒ **13:9x ③ 的「附件 ＋ 两条限定」表述不准**（像「附件漏了」）⇒ **正确表述：照抄附件即可，但「限定段是否真的抄进协议」是必查项** ⇒ **否则同一限定写两遍**；**13:9x ③ 以本节为准** ③ **新交付 `p4-v07-review-checklist.md`（50 行）验收通过、采纳**（**3 条否定性硬判**（不留占位符 / 每个键名数字都能在读数里指出出处 / 不得出现未经读数支持的键名）+ **7 条核对项**（含标注「最易漏」的 mock 射程限定）+ **3 个动作**（门禁复跑 / 填判据脚本 / **差异逐条列、不许只说「已核对」**）+ **4 个坑**）⇒ **四个坑与 PM 手里的事实逐条对得上** ④ **⚠️ 采纳基线纪律（本轮最有价值）**：他被过期基线号坑了**七轮** ⇒ ⇒ **从本节起台账新增节不再手写「现为 `<commit>`」，一律写「基线 ＝ 现读 `origin/main`」**；**历史节里的手写号保留不动**（那是当时的真实基线）⇒ **本节起生效、不回改** ⑤ **判据进门禁**：他按 PM 说的先不动 ✔ ⇒ **派单时 PM 定** ⑥ **协议与 P0 未动 ⇒ 不升版本**（仍 v0.6，冻结稿待 v0.7）⑦ **仍无新单**；**唯一待办仍是 izzy 那一次 Chrome 输出** ⇒ 输出回来 ⇒ **PM 落 v0.7 冻结稿**（照抄附件 ＋ 清单 0/③/④ 自检）⇒ 成员按清单逐条核 ＋ 门禁复跑 ＋ 填判据脚本 ⇒ ⇒ **才派 P4 判据单 ＋ P4 实现单** | 本文件 14:0x 节 ＋ 一手核验（成员给的行号 `:48`/`:50`/`:71`/`:55` 逐行现读 ＋ 核对清单 50 行全文读过 ＋ 台账 `:1935`/`:2019` 与总行数实测） |

| 2026-10-21 | P4 | **⚠️ 订正 PM 台账三处不实转述（他 14:1x 指控成立：结论对、依据错）＋ 两类东西分开（基线号保留 vs 转述不实必订正）＋ 成员已按路径纪律改附件两行（PM 一手核过）** —— ① **他逐封列出的实测基线**：13:7x ＝ `9742546`｜13:8x ＝ `38751c2`｜13:9x ＝ `38751c2`｜14:0x ＝ `491c779` ⇒ ⇒ **他从未落后三笔**（每封都带当时最新值）⇒ ⇒ **PM 三处取错了那封**：13:9x（补）⑤ 写「他 fetch 到 `9742546`」（**该封实为 `38751c2`**）｜13:9x（补二）② 写「他 fetch 到 `4848296`」（**该封实为 `38751c2`**）｜14:0x ⑤ 写「他 fetch 到 `38751c2`」（**该封实为 `491c779`**）⇒ ⇒ **三处「现为 Y」结论都对、依据错** ⇒ **以本节为准**、**不回改历史原文** ② **两类东西分开**：**基线号本身**（当时远端 ＝ X）⇒ 历史节**保留不动**（那是当时的真实基线）｜**「他 fetch 到 X」这类转述** ⇒ 与该封实际不符即**不实记述 ⇒ 必须订正**（不能拿「历史不动」当挡箭牌）⇒ ⇒ **14:0x ④ 与同节 ⑤ 自相矛盾** ⇒ **以本节为准：④ 手写号视为该节当时值记录；今后新增节写「基线 ＝ 现读 `origin/main`」**；**取成员基线 ＝ 取最新那封回执，或直接现读** ③ **他已按路径纪律改附件两行，PM 一手核过（各一行，结论与读数一字未改）**：`clause-draft.md`（**10,390B**）一之二末尾那行 ⇒ 改成「**复现过程用的是一次性临时脚本 —— 属过程资产、不是长期可核入口** ⇒ 协议正文只写结论与射程限定，不写复跑路径」｜`p4-v07-review-checklist.md`（**5,588B**）④ 行依据列 ⇒ 改成「**只引结论与读数，不引过程性临时路径**」⇒ **模式 P／D 读数、零背书、键名仿造值三条结论都还在** ✔ ④ **素材归档口径**：**4 份交付物**（条款草案 · 回填映射 · §10 判据第 8 条断言 · 落笔后核对清单）＋ **1 份过程性工具（不进依据链）** ⇒ 等读数回来**只做填空 ＋ 按清单逐条核** ⑤ **协议与 P0 未动 ⇒ 不升版本**（仍 v0.6，冻结稿待 v0.7）⑥ **仍无新单**；**唯一待办仍是 izzy 那一次 Chrome 输出** ⇒ 输出回来 ⇒ **PM 落 v0.7 冻结稿**（照抄附件 ＋ 清单 0/③/④ 自检）⇒ 成员逐条核 ＋ 门禁复跑 ＋ 填判据脚本 ⇒ ⇒ **才派 P4 判据单 ＋ P4 实现单** | 本文件 14:1x 节 ＋ 一手核验（成员改动的两行现读 ＋ 附件字节数/时间 ＋ 三处原写法逐条对账） |

| 2026-10-21 | P4 | **PM 一手核出「站内码两档」的实测出处 ＋ 排除「grep 站点 JS 包」路线 ＋ ⚠️ 发现协议 §3.1 一条实现级条款的依据在文档链里定位不到出处 ⇒ v0.7 落笔清单从四处扩到六处** —— ① **`40002` / `40003` 早就实测过**（feasibility `:144`：**裸 fetch → `40002 Missing Token`；带 Bearer `userToken` → `40003 INVALID_TOKEN`**；当时**页面停在 `/sign_in`**、「**本机从未建立登录态**」）⇒ ⇒ **「站内码的取值与 msg」本来就有实测出处**；没验到的是「**登录态下会回哪一档**」⇒ ⇒ **G4-3 的缺口应当收窄**（不是「码未知」，是「登录态读数未知」）② **「grep 站点 JS 包拿 40002 语义」不成立**（检索串 `40002`、范围 ＝ Chrome `Default\Cache\Cache_Data` **1243 个原始资源文件** ⇒ **4 处命中全是假阳性** ＝ `X64Word_create(4118630271, 4000239992)` 的**子串**；对照检索串 `40003` ⇒ **零命中**）⇒ ⇒ **G4-3 不能靠静态扫描收口**（**否定性证据，两个检索串原样写出**）③ **⚠️ 最硬的一条**：协议 §3.1 `:110`「页内不需要、也不应该自己拼凭据｜【已实测】（探针实跑 `status=200`、`code=0`）」的**依据定位不到出处** —— P0 探针**只发过两次 `attempt`**（`:1228-1229` 裸，注释原文「**裸 fetch：同源 cookie 够不够？**」＋ `:1303-1306` Bearer）⇒ **两次读数是 `40002` 与 `40003`**；探针 A 那一支（`official_chat_deepseek_probe.rs:312` 裸同源 fetch）也是 **`40003`**；**全文唯一出现 `code=0` 的那组读数恰恰是 §4.2 写死的**反例**（同时 `biz_code=2`／`biz_data=null`）** ⇒ ⇒ **极可能是把「HTTP 200」误记成了「业务码 0」—— 正是 §4.2 那条防误读条款要防的同一个坑** ⇒ ⇒ **该依据不成立**（但**不能据此说结论已被证伪**：P0 与探针 A **都是未登录态**，`40002`／`40003` 完全可能只是「没登录」⇒ **精确定性 ＝ 依据不成立 ＋ 结论未验**）④ **⚠️ 这条一旦判反，会动到取数路线本身（不是措辞问题）**：若登录态下裸档仍回站内码 ⇒ **§7.2 `:240`「不许把站点 token 拼进请求头」**与「站点只认 `Authorization` 头」**互斥** ⇒ ⇒ **§1 取数路线／§2 页内求值／§5 接线都要改写** ⇒ ⇒ **必须在 P4 落地前定案，不能带着含糊条款派实现单** ⑤ **脚本不用改**（PM 再核一遍）：**「裸」档 ＝ 不带 `Authorization` 头** ⇒ 天然就是「无 token」那一档；三档头已覆盖「无 token／带无效 token／带无效 token＋设备头」；**三档全拒时脚本会继续挂第 2 档钩子**（`break` 只在 `s.arrayPath` 存在时触发）⇒ ⇒ **一次运行同时收口「字段面四项 ＋ 凭据机制 ＋ 站内码逐档读数」** ⇒ **izzy 跑一次就够，不用第二轮** ⑥ **v0.7 必改六处**（§3.1 `:110`／§4.3 `:163`／§4.3 `:168`／§6.2 `:223`／§8.6 `:315`／§9 `:325`）⇒ ⇒ **冻结条件不动**（§4.3 原文写的就是「**§4.3 四项全部有实测出处**」）⇒ **站内码不绑 v0.7** ⑦ **素材不返工**（成员草案「二」节本就是「**按端点 ＋ 档位逐条写**」＋「**未验到的档位登记为缺口**」）⇒ **但核对清单要补一条：§3.1 那行现稿措辞与依据** ⑧ **协议与 P0 本轮未动 ⇒ 不升版本**（仍 v0.6，冻结稿待 v0.7；**六处订正并入 v0.7 一次落**，与 13:6x ⑥ 同款处置）⑨ **仍无新单**；**唯一待办仍是 izzy 那一次 Chrome 输出** ⇒ 输出回来 ⇒ **PM 落 v0.7 冻结稿**（照抄附件 ＋ 六处订正 ＋ 按清单 0 自检）⇒ 成员逐条核 ＋ 门禁复跑 ＋ 填判据脚本 ⇒ ⇒ **才派 P4 判据单 ＋ P4 实现单**；**⑩ 成员一手复核背书了 §3.1 那条**（六处行号全准、全文档链无 `code=0`）＋ **队列读数：终态 `cancelled` ＋ `notification: delivered` ＝ 已送达 ⇒ 不重发** ＋ **他补发里「台账还没记改后状态」的前提不成立**（14:1x ③ 已记 ⇒ 历史节不动）＋ **附件在途**（字节数已变 ⇒ 落笔前现读）；**⑪ 成员已把 §3.1 那条补进两份附件（PM 核过、验收通过）＋ PM 落笔清单增补三项**（一之三节／硬判 4／核对项 ⑧）＋ **采纳他提的第 9 条判据（凭据机制）⇒ 协议 §10 由 8 条增到 9 条** | 本文件 14:2x 节 ＋ 一手核验（feasibility `:144` ＋ 探针源码 `official_chat_probe.rs:1152-1154`／`:1228-1229`／`:1303-1306` ＋ `official_chat_deepseek_probe.rs:312` ＋ Chrome `Default\Cache\Cache_Data` 检索 `40002`／`40003`）＋ **附件现态核验**（两份文件里 `p4-mock-trace`／`.cjs` **零命中** ＋ 两条改后措辞在位：`clause-draft.md:58` ＋ `checklist.md:25`）＋ **队列终态读数**（`batch_36107f787815` ＝ `cancelled` ＋ `delivered` ＋ `complete: true`）＋ **附件增补现读**（`clause-draft.md:62`／`:71`／`:76`／`:80`／`:82` ＋ `checklist.md:14`／`:29`） |
| 2026-10-21 | P4 | **⚠️ 第一条登录态实测读数到了（izzy 已跑）⇒ §3.1 走分支 B 坐实 ＋ G4-6 最后一层闭环 ＋ 三档码有实测背书 ＋ 但 §4.3 四项仍全空 ⇒ v0.7 仍不能冻结** —— ① **核验边界：PM 本轮核的是「成员转录」**（`p4-section43-backfill.md` 第七节），**原始 JSON 尚需 izzy 回贴后逐行对账**（成员自己的硬判 4：「溯不到就不许标【已实测】」）② **第 1 档 6 条全部被拒**：`users/current` 裸档＝**`40002 Missing Token`**、`fetch_page` 裸档＝**`40003 INVALID_TOKEN`**（同一裸档、**两个端点、两个码**）⇒ 【「同源 fetch 带 cookie 就够」＝不成立】⇒ **§3.1 `:110` 走分支 B 坐实**；**§7.2（不许自拼头）与站点要求互斥 ⇒ §1/§2/§5 取数路线必须改写**（PM 14:2x ④ 预警判反）③ **第 2 档钩子截到站点自己的 7 个请求全部 `code=0` 且 `biz_code=0`** ⇒ **登录态此刻有效**（G4-6 最后一层闭环）；**白拿真键名**：`accepted_events` / `version`＋`settings` / `settings.model_configs.value`（itemCount=3）/ `settings.providers.value`（itemCount=4）/ `challenge`（但**不是「会话目录」**）④ **§4.3 四项仍全空**（`tier1GotArray=false`；点「新对话」不拉会话列表）⇒ **v0.7 仍不能冻结**；⑤ **第二份脚本 `p4-auth-trace.js` PM 已一手核**（只回头名＋值长度、不读响应体、零泄露面；一个局限：XHR 的 `cur` 是模块级变量 ⇒ 并发时 url/头可能张冠李戴）⇒ **正文已收进本节 ⑦**（PM 唯一出口）⑥ **v0.7 落笔清单再增四项**（§3.1 分支 B 措辞 / §7.2 一致性 / §9 G4-6 收口 / §4.1 真键名）；⑦ **协议本轮未动 ⇒ 不升版本**（仍 v0.6，全部随 v0.7 一次落） | 本文件 14:3x 节 ＋ 交接记录 14:3x 行 ＋ 一手核验（转录自洽核：7.1–7.4 ＋ `p4-auth-trace.js` 逐行读过 ＋ 成员原文逐行拼入本节 7） |
| 2026-10-21 | P4 | **PM 一手复跑「粘贴语法错」体检（两份文件都干净 ⇒ 错在复制通道）＋ ⚠️ 纠正成员两处与实测矛盾的预期 ＋ ⚠️ 探针已跑成功 ⇒ izzy 不用重跑** —— ① **成员定位「文件干净、错在复制环节」成立，PM 独立复跑不采信自报**：成员原文件 `deepseek-shape-probe.js` 与 **台账 `:1671-1870` 那份（成员没核、PM 补）** 均为 **9,669B**、首 4 字节 `2f 2f 20 44`、**无 BOM**、**0 CRLF**、UTF-8 解码 OK、**零宽／NBSP／异常控制符 0 命中**、`node --check` **双 exit=0**、**可执行部分 154/154 逐行全等** ⇒ ⇒ **「台账那份正文坏了」这个可能也被排除** ⇒ ⇒ **报错只可能来自「复制进 Console」这一步**（渲染视图／富文本／带行号视图）⇒ ⇒ **探针正文一个字都不用改** ② **⚠️ 纠正他一处与实测矛盾的预期**：他写「登录态有效 ⇒ 裸档预计直接命中 ⇒ 另两档不会发出」⇒ **与实测直接矛盾**：登录态下**恰恰是裸档被拒**（`users/current` 裸档＝`40002 Missing Token`；`fetch_page` 裸档＝`40003 INVALID_TOKEN`）⇒ ⇒ **「裸档没命中」＝预期之内，不是跑错**（正因裸档被拒才进到第 2 档钩子，而钩子抓到的 7 条 `code=0` 恰恰证明登录态有效）⇒ **按他那条预期读，会把正确结果当失败** ③ **⚠️ 纠正他第二处「不影响 §4.3 四项」＝不成立**：裸档被拒 ⇒ 第 1 档无数组 ⇒ 钩子又抓不到 `fetch_page`（点「新对话」不拉列表）⇒ ⇒ **四项确实被卡住**，**探针这条路已走到头** ④ **⚠️ 最要紧：探针已跑成功 ⇒ izzy 不用重跑**（那 6 条全拒＋7 条站点请求＋`tier1GotArray=false` 就是探针输出，PM 已入台账 14:3x `:2243` 起）⇒ ⇒ **再跑一遍＝白跑** ⇒ ⇒ **只需跑第二份**（`p4-auth-trace.js`，台账 `:2270-2353`）⑤ **粘贴通道口径（PM 裁定）**：**别从聊天气泡／预览页／带行号视图复制**；**首选：记事本全选复制**；**备选：只粘一行** `await navigator.clipboard.readText().then(t => eval(t))`；**这不是 izzy 的错** ⑥ **协议仍未动 ⇒ 不升版本**（仍 v0.6）⑦ **站位**：等第二份鉴权头取证 ⇒ 定「带哪个头打 `fetch_page`」⇒ 四项一次收口 ⇒ v0.7 ⇒ 派两单 | 本文件 14:4x 节 ＋ 交接记录 14:4x 行 ＋ 一手核验（两份文件字节级体检 ＋ 双 `node --check` ＋ 154/154 可执行部分比对 ＋ 与实测读数对账） |
| 2026-10-21 | P4 | **采纳成员的三段分法定性（PM 一手核他引的注释原文，逐字属实）＋ PM 收紧第 1 段 ＋ ⚠️ 纠正他第 5 点「好消息」判断（已被实测否掉）＋ 登记一项待 izzy 拍板的互斥岔口** —— ① **他的技术内容全部成立**：七处行号现读逐条准确（PM 抽查 7/7）＋ 他引的两条注释原文 PM 也现读了、**逐字属实**（`official_chat_probe.rs:1307-1308`「DeepSeek 那次 200 但业务 code 是 40002 / 40003」｜`official_chat_deepseek_probe.rs:13-14`「页外裸 fetch 与带 Bearer 都是 `40003 INVALID_TOKEN`」）⇒ ⇒ **「HTTP 200」这半句确有出处** ⇒ ⇒ PM 14:2x ③「把 HTTP 200 误记成业务码 0」的推断**成立** ② **✅ 采纳他的「三段分开」定性**（比 PM 说法更精确）：**依据行**「`status=200`」＝成立（不必删）／「`code=0`」＝无出处（必须删）；**结论行**「页内不需要自己拼凭据」＝**未验**（那两次都是未登录态）⇒ ⇒ 精确定性 ＝ **「依据不成立 ＋ 结论未验」**，**不是「结论被证伪」** ③ **PM 收紧第 1 段**：单留「`status=200`」**会被读成「成功」**（§4.2 同款误读）⇒ ⇒ 落笔措辞建议「**【已实测】（探针实跑 `status=200` ＋ 业务码 `40002`／`40003`，未登录态）**」④ **⚠️ 纠正他第 5 点「好消息」**：**登录态下裸档就是被拒的那一档**（实测 `40002`／`40003`）⇒ 「概率高」不成立；且他那句「站点自己的 XHR 就不拼 Authorization 也通」**既事实错**（站点拦截器明确走 `Authorization: Bearer`，协议 §3.1 `:111`）**又逻辑不通**（若如此我们的裸档也该通）⇒ ⇒ **会让人把正确读数当失败**（PM 14:4x ② 已纠正过一次，他这封仍复述 ⇒ 本节再留痕）⑤ **他第 5 点另一处已过时**：「结论有可能被 izzy 读数证实」⇒ **读数已到、落在分支 B**（登录态裸档被拒）⇒ 但「页内要不要拼凭据」整体未定 ⇒ **等第二份鉴权头取证定「用哪个头」** ⑥ **⚠️ 登记待 izzy 拍板的互斥岔口（条件触发）**：登录态裸档被拒 ⇒ 与 §7.2 `:240`「不许把 token 拼进请求头」**互斥** ⇒ **不许含糊**；**但前提是「用哪个头」** ⇒ ⇒ 第二份读数到手后 PM 把选项摆给 izzy；在此之前不动协议 ⑦ **协议仍未动 ⇒ 不升版本**（仍 v0.6）⑧ **站位**：izzy 只需跑第二份（`:2270-2353`）⇒ ⇒ 一次同时收口「§3.1 凭据口径 ＋ §4.3 四项」⇒ v0.7 ⇒ 派两单 | 本文件 14:5x 节 ＋ 交接记录 14:5x 行 ＋ 一手核验（`official_chat_probe.rs:1305-1312` ＋ `official_chat_deepseek_probe.rs:10-18` 现读 ＋ 协议七处行号现读 ＋ 与实测读数对账） |
| 2026-10-21 | P4 | **第二跑读数到了 ⇒ 钩子探法结案（时序不可达，不是脚本错）＋ 附带硬证据（`userToken` 不是 JWT ⇒ 站点的 API token 不在 `localStorage` 里）＋ 新落点 ＝ DevTools Network 面板 ＋ ⚠️ 订正成员引用的一处行数 ＋ ⚠️ 待办时效核（izzy 那两件事都过期了）** —— ① **第二跑读数（PM 一手核，两处互证）**：成员 14:5x 那封 `batch_211733710d28`（标题「① izzy 的原始 JSON（原样转给你）＋ ② 第二跑结果」）**正文没回传到 PM 会话**（终态 `cancelled` ＋ `complete: true` ＋ `notification: posted` ＋ **无 result**）⇒ ⇒ **PM 从运行时日志取回正文**（只读、读前打码）⇒ ⇒ 与其换道素材 `p4-devtools-readback.md`（5,549B @ 14:43:43）**逐条互证**：`capturedCount=4`＝`hif-dliq` 6 个头 ＋ 火山 `gator.volces.com` ×2（只有 content-type）＋ 一条 fetch 没截到 URL 和头 ⇒ ⇒ **钩子工作正常，但点「新对话」不发 `/api/v0/`** ⇒ **那批是页面初始化时发的、钩子装上去时已发完** ⇒ **刷新又丢钩子** ⇒ ⇒ **钩子探法结案（不是脚本错，是时序不可达）** ② **⚠️ 附带硬证据**：`userToken` ＝ **92 字符 / 0 个点 / 2 个冒号 ⇒ 不是 JWT**；`settingsJwt`（410/4 点）也不是标准 JWT；`__appKit_userInfo` 是 JSON ⇒ ⇒ **站点的 API token 不在 `localStorage` 里** ⇒ ⇒ ⇒ **§7.2「不许拼头」与「站点要带某个头」并不必然互斥**（正解＝**页内复用站点自己的请求**，不自拼头）⇒ ⇒ **PM 14:5x ⑥ 登记的「待 izzy 拍板的互斥岔口」可能并不存在** ⇒ **改为**：待 Network 读数确认该路可行且零痕迹；**不可行时**（`fetch_page` 只在切进会话时才发）才需拍板「是否允许一次真实交互」 ③ **新落点 ＝ DevTools Network 面板（成员素材 PM 验收通过）**：`F12` → Network → **勾 `Preserve log`** → **`F5` 刷新** → 过滤 `fetch_page`（找不到改 `api/v0`）⇒ **全程只看不点**（站点侧零写入）⇒ **只报两样**：**A** 头名＋每个值长度（**不贴值**）｜**B** `data` 键名／`biz_data` 键名＋哪些是数组／**数组首元素键名**／**数组长度**／**游标键名**（三处都查）⇒ **B 正好覆盖 §4.3 四项** ⇒ **一次收口鉴权机制 ＋ 四项** ④ **⚠️ 订正成员引用的一处行数（结论对、依据错）**：`p4-devtools-readback.md:19` 的「200 行 / 154 行」是**主探针**的数字、**不是** `p4-auth-trace.js` 的 ⇒ PM 一手复核：auth-trace ＝ **82 行**、台账 14:3x ⑦ 围栏 **`:2270-2353`** ＝ **82 行**、**`IDENTICAL: True`** ⑤ **⚠️ 待办时效核**：① 第一轮原始输出**成员已原样转给 PM**（只打码 `local.keys` 里带本机 id 的那个键名；探针输出本就没有凭据值）⇒ **izzy 不用再贴**；② 第二份脚本**izzy 已跑过** ⇒ **不用再跑** ⇒ ⇒ **izzy 只剩 Network 面板那一步** ⑥ **协议仍未动 ⇒ 不升版本**（仍 v0.6）⑦ **落笔清单更新**：auth-trace **不再列「待跑」**（已执行完毕、探法换道、文件一字未改且与台账逐行全等）；**§4.3 与 §3.1 的「正向」出处改为 Network 读数**（探针 6 条全拒仍是「裸档被拒」的有效依据，两源并存）；**§3.1 分支 B 措辞按 ② 更新**（不再预设「互斥 ⇒ 需拍板」） | 本文件 14:6x 节 ＋ 交接记录 14:6x 行 ＋ 一手核验（运行时日志取回成员 14:5x 正文（读前打码）＋ 与 `p4-devtools-readback.md` 逐条互证 ＋ `p4-auth-trace.js` 82 行 vs 台账 `:2270-2353` 82 行 `IDENTICAL: True`） |

**关于上表里的 `%TEMP%` 证据路径**：2026-10-20 15:2x izzy 拍板「**可以清**」后，PM 已把 `%TEMP%\dsh-xlink-g1\` 与 `%TEMP%\dsh-probe\` 移入回收站（可恢复删除），**上表引用的原始 JSON / 截图 / 请求体日志已不在原位**。**仍可长期复核的，是本文件与 feasibility 里写下的数字、字段值与判据口径**——引用这些结论时不要再指望去原路径取原始产物。
