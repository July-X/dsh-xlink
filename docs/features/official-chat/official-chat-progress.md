---
description: "官网对话直接引用的分阶段事实进度记录。P0 部分只写已发生的事实，不预写结论。"
kind: "progress"
status: "P0 机制层结论已出（G1/G2/G3）；P1 协议已冻结（v0.5）且代码侧落地、真机 p1-r9 在未打补丁的现役内核上 11/0/1；P2 第一批（重复识别 ＋ 可见提示 ＋ 预算预检）协议 v0.5、实现与判据均已合并推送（远端 main = af9789d），PM 一手复跑：判据 9 通过 / 0 失败 / 0 未执行、自测 23/23、test:scripts 175 pass / 0 fail / 3 skipped；真实站点取数（P4）与安装版 release 验收未做"
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
| P2 内核插件闭环 | 固定数据的选择、引用、发送与模型输入核对 | **已立项**（izzy 22:5x 点头）。「正文去重」子议题的**只读调研已出**（全文见 [official-chat-p2-research.md](official-chat-p2-research.md)），PM 建议三条 = **P2 第一批口径**，已协议化为 [official-chat-p2-protocol.md](official-chat-p2-protocol.md) **v0.5**（PM 定，六处裁定）：**重复识别 ＋ 可见提示 ＋ 预算预检**，**不做消息级自动合并**、**不动 chip 数**、**不碰 id 分配**、**不改 `agent/pre-step` 消息**。键 = (accountScope, sessionId, branch, capturedAt, contentDigest) 全等；落点 = **host 命令层 `duplicates` 命令产出重复组，client 呈现**。**实现已合并推送**（`bec3b82`，13 文件）＋ **判据第一批已合并推送**（`af9789d`，6 文件 / +3060 / −17）⇒ 远端 `main` = **`af9789d`**。**PM 一手复跑（2026-10-21 00:4x，不采信回执）**：判据 runner **9 通过 / 0 失败 / 0 未执行**、判据自测 **23 / 23**、`npm run test:scripts` **178 tests / 175 pass / 0 fail / 3 skipped**、P1 判据 runner **43 条 / 40 通过 / 0 失败 / 3 未执行** ⇒ **P1 未回退**。**未做**：P2 第二批及以后、真实站点取数（P4）；**界面层文案已定稿**（UX 2026-10-21，成品文案已入 **协议 v0.6 §3.2**；呈现形态锁死「两条、两个 kind」） |
| P3 正文快照与桥接 | 原子保存、固定版本、回收、恢复 | **未开始** |
| P4 首站接入 | 真实目录、认证、分支与正文 | **未开始**（前置条件未满足） |
| P5 桌面体验与诊断 | 登录回程、错误、详情、能力检测 | **未开始** |
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

**关于上表里的 `%TEMP%` 证据路径**：2026-10-20 15:2x izzy 拍板「**可以清**」后，PM 已把 `%TEMP%\dsh-xlink-g1\` 与 `%TEMP%\dsh-probe\` 移入回收站（可恢复删除），**上表引用的原始 JSON / 截图 / 请求体日志已不在原位**。**仍可长期复核的，是本文件与 feasibility 里写下的数字、字段值与判据口径**——引用这些结论时不要再指望去原路径取原始产物。
