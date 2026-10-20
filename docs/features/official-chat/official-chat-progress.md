---
description: "官网对话直接引用的分阶段事实进度记录。P0 部分只写已发生的事实，不预写结论。"
kind: "progress"
status: "P0 机制层结论已出（G1/G2/G3）；壳真实 workbench 内复验已通过一次；真实链路与首站取数未闭合"
date: 2026-10-20
---

# 官网对话直接引用 · 阶段进度记录

本文件只记**已经发生的事实**：谁做了什么、在什么环境、用什么命令、产出了什么、结论是什么、哪些没做。**不预写结论，不写「待办」以外的推测**。

P0 的可行性结论单独见 [official-chat-feasibility.md](official-chat-feasibility.md)；实施规则见[开发计划](official-chat-development-plan.md)；方案与待验证清单见[调研](official-chat-bridge-research.md)。

## 阶段总表

| 阶段 | 内容 | 状态 |
| --- | --- | --- |
| P0 可行性验证 | G1 内核工作台选择与发送控制、G2 异步回程与首站可读性、G3 窗口未打开时仍可读取 | **G1/G2/G3 结论已出**（G1 4/5 问通过 + Q3 判定互斥；G2-A 5/5，Windows 零痕迹回程已实测成立；G2-B 探测完成但取数未成立；**G3 三站各 7/7 通过**；**壳真实 workbench 内复验已可复现**，壳驱动已搬进仓库）。**未闭合的三项**：登录态从未建立（需用户首次登录）、零痕迹回程 macOS 侧无通路、**release 壳**未验（复跑只跑过 dev 壳） |
| P1 协议与隔离 | 固定数据模型、错误、权限与状态机 | **未开始**（P0 的就绪状态纪律可并行开工，见 feasibility 第 9 节） |
| P2 内核插件闭环 | 固定数据的选择、引用、发送与模型输入核对 | **未开始** |
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

- 插件代码（只在 `plugins/official-chat/**`，**未提交**）：`package.json`、`host/index.js`、`client/client.js`、`test/client.test.mjs`、`test/smoke.test.mjs`、`test/drive/run-g1.mjs`、**`test/drive/shell/`**（壳驱动 7 个文件 + README，14:47 搬入仓库，**未提交**）。
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

**发现三：已落地的隔离判据有覆盖不到的口子。** G2 探针 `src-tauri/examples/official_chat_probe.rs` 在**不传 `--profile`** 时（`profile: None` → `build_window_with` 不设 `data_directory`）会让 WebView2 用**默认数据目录**，也就是真实安装目录 `%LOCALAPPDATA%\com.zhongxingxing.dsh-xlink\EBWebView`；**13:48 实测该目录被写入过**（Cookies-journal / Local State）。已落地的机检判据（`scripts/lib/probe-profile-isolation.mjs`）扫的是源码里的**路径标记**，**抓不到「用默认路径」这种写法**。⇒ 建议把「探针必须显式给 `data_directory`，没有就 fail fast」也落成判据；**该建议在 P0 期间未落地**。

### 2026-10-20 · P0 文档产出

- 新建 [official-chat-feasibility.md](official-chat-feasibility.md)：P0 唯一对外结论，含环境与版本、G1/G2/G3 逐项结论与证据、裁定与理由、文档与实测的出入、本轮没有验证的、是否可以继续。
- 新建本文件：阶段事实进度。
- **未改动** `official-chat-development-plan.md` 与 `official-chat-bridge-research.md`（出入只在 feasibility 第 7 节单列）。
- **未执行** `git commit` / `git push`；改动留在工作树。
- **最终回填（2026-10-20 下午）**：G3 实结论（三站各 7/7）、零痕迹回程实测（Windows 成立 + 接口名更正 + macOS 无通路）、真实 profile 探测的损害范围、登记与门禁实测、release-only 假红、物化目录名发现、**登录态归因更正（「本机从未建立登录态」）**均已并入两份文档。`official-chat-development-plan.md` 与 `official-chat-bridge-research.md` **仍未改动**。P0 产出已由协调者提交为 `6b98cad`，本轮回填当时是**相对该提交的工作树改动**，未提交、未推送；**该轮回填后来已被 14:58 的 `3436fb4` 提交进去**。
- **第二次回填（2026-10-20 14:4x）**：按 14:09 / 14:28 / 14:31–14:35 的新证据改写——**「壳真实 workbench 内复验」从「未验」改成「已通过一次、复跑稳定性未验」**（§8 G1 第 1 条、§9 先决条件 5、§9 P4 理由③、§1 摘要同步改写）；**原始请求体因逐轮独立文件而重新可复核**（§7 第 6 条、§8 口径说明第 3 条、§10 证据清单）；新增**探针隔离的口子**（§7 第 10 条、§8 其他第 20 条、progress「发现三」）；门禁数字更新（`test:plugin` 209/206/0/3、插件单测 29 passed、`plugins::builtin` 71 passed）；8 个临时目录回收与脚手架修好一并记入。
- **第三次回填（2026-10-20 15:1x）**：按 14:47–15:09 的新证据改写——**壳真实 workbench 内复验从「已通过一次」改成「已可复现」**（壳内两轮 + 内核 r16 一轮全过，驱动已进仓库 `plugins/official-chat/test\drive\shell\`，**未提交**），§8 G1 第 1 条、§9 先决条件 5、§9 P4 理由③、§1 摘要与 frontmatter 同步改写；**新增复跑的两条前置条件**（attach 必须是 main 面板 / 改内置插件开关前必须先停该实例内核）；新增两个真问题（第一次 pick 假红、`waitMenuRow` 漏 `return` 假阴性）；判据扫描文件数 **31 → 37**；§10 补壳驱动与 shell-r2 / r16 证据位置，并把工作树现状写清（多出 `?? plugins/official-chat/test/drive/shell/`）。**仍未改动** `official-chat-development-plan.md` 与 `official-chat-bridge-research.md`；**仍未执行** `git commit` / `git push`（**提交由 izzy 统一做**）。
- **提交基线已变**：14:58 协调者提交了 `3436fb4`（「G1 断言复跑全绿 + P0 结论回填；AGENTS.md 补跑批证据约定」），**它已经包含本文件与 feasibility 的 14:4x 回填**、`run-g1.mjs` 重写与 AGENTS.md 一行。**15:1x 这一轮是相对 `3436fb4` 的工作树改动**（`M` 两份文档 + `M run-g1.mjs` + `?? drive/shell/`），**未提交、未推送**。

## P1 及以后

尚无事实记录。

**P1 可以开工**：P1 的前置条件「G1 通过」已满足；G3 已成立（三站各 7/7）；P1 建模的是协议与状态机，**不依赖 G2/G3 的站点侧取数结论**。

P1 开工时需要先落地的输入（来自 feasibility 第 9 节，先决条件，非本次结论）：

1. 就绪状态纪律：pending 句柄不得冒充就绪；label 保持中性；`openReference` 按 ref 如实报 pending/ready/failed；「就绪」由 chip 状态与详情承担，不以「chip 是否存在」承担。
2. G1-1…G1-5 登记为已知上游缺口，实现中不得假设它们已解决。
3. 页内求值走零痕迹回程（Windows 走 CDP `Runtime.evaluate` + `awaitPromise`，**已实测成立**；**R1 槽位不得带进真实站点**；**macOS 侧无通路**，按已知缺口处理——代码里明确报错、不静默降级）。
4. **把「固定快照」与「pending 句柄」的关系定死——这是两件东西**：快照是 P1/P3 的**落盘物**（`schemaVersion` / 不可变 `snapshotId` / 规范化内容摘要 / 捕获结果，与官网会话 id 分开）；pending 句柄是 G1 已验证过的 **chip 形态**（有 ref、有读取记录，发送时由 `codec.serialize` 取正文，等待或拒绝）。P1 必须写清：句柄指向哪个 snapshot、句柄在 snapshot 落盘前后各处于什么状态、「就绪」由 snapshot 的哪个字段承担。**不能把「chip 存在」当成「快照已就绪」。**

## 交接记录

| 日期 | 阶段 | 事实 | 证据 |
| --- | --- | --- | --- |
| 2026-10-20 | P0 | G1 五问完成（4 通过 / Q3 判定互斥），插件收敛为甲形态单变体，单测 26 passed；第 3 轮改走真实 Rust 物化链后五问重跑 | `%TEMP%\dsh-xlink-g1\delivery2.md` + `delivery3.md` + `final.out.txt` + `final.json` + `reject.out.txt` + `reject-notice.json` |
| 2026-10-20 | P0 | G1 壳真实 workbench 复验：**14:09 那轮在壳自己的真 Tauri workbench webview 里四项全过**（@ 菜单分组 / chip 是内核节点且正文不进草稿 / 延迟读取门控 / 读取失败 0 请求 + 可见错误），壳自己的物化链产物也核对过；**14:28 同源脚手架复跑失败**（capability 拒绝 `builtin_set_enabled`，attach 到 `chatstrip` 窗口）；14:31–14:35 内核工作台 r12/r13/r14 五项断言全过，且**原始请求体改为逐轮独立文件** | `delivery-final.md` + `shell-verify.out.txt`(7,713 字节 @14:09) + `shell-verify-r2.out.txt` + `run-r13.out.txt` + `evidence-r14\results.r14.json` + `model-requests.r14.jsonl` |
| 2026-10-20 | P0 | 清理：13:09 轮留下的 **8 个 `%TEMP%\g1-probe-*` 目录（121.3MB）已全部回收**（运行时可恢复删除启动器，移入回收站，未永久删除）；仓库内脚手架 `run-g1.mjs` **已重写并跑通**（第 13、14 轮连续全绿） | devloper 14:4x 交付报告 + `run-r13.out.txt` / `evidence-r14\results.r14.json` |
| 2026-10-20 | P0 | 跨会话发现：G2 探针**不传 `--profile`** 时会落到真实安装目录（13:48 实测该目录被写过），已落地的路径标记判据**覆盖不到**「用默认路径」写法；建议「探针必须显式给 `data_directory`，否则 fail fast」落成判据，**P0 期间未落地** | `delivery-final.md` §6.2 + `src-tauri/examples/official_chat_probe.rs` |
| 2026-10-20 | P0 | **壳驱动搬进仓库** `plugins/official-chat/test\drive\shell\`（7 个文件 + README，**未提交**），搬完**真跑通**：壳内四项全过 + 内核 r16 五项全绿；判据第 30 条扫描文件数 **31 → 37**（新增 6 个 `.mjs` 真进了扫描范围）；同时定位到**复跑的两条前置条件**（attach 必须是 main 面板；改内置插件开关前必须先停该实例内核） | `evidence-shell-r2\shell-verify.shell-r2.json` + `evidence-r16\results.r16.json` + `plugins/official-chat/test\drive\shell\README.md` |
| 2026-10-20 | P0 | G2-A 异步回程 5/5 通过；G2-B 三站只读探测完成，真实取数结论因**本机从未建立登录态**未成立；Q1/Q2 判据已修并补行为判据 6/6 | `%TEMP%\dsh-probe\*.json` + `src-tauri/examples/official_chat_probe_site.test.mjs` |
| 2026-10-20 | P0 | G3 机制层 7/7 通过（未登录 + 模拟页签条件下）；真实官网窗口 + 已登录态未执行 | 探针 G3 跑批（临时目录时间戳 `1792475664714`） |
| 2026-10-20 | P0 | 零痕迹回程：R1 槽位产品上站不住；Windows CDP 5/5 成立；**macOS 侧无通路（P0 已知缺口，代码里明确报错不静默降级）** | `--mode cdp` 跑批 + `script_eval.rs:434-436` / wry 0.57 `src/wkwebview/mod.rs:723-780` |
| 2026-10-20 | P0 | 隔离纪律落成机检判据（`check-invariants` 第 30 条，10/10） | `scripts/probe-profile-isolation.test.mjs` + `scripts/lib/probe-profile-isolation.mjs` |
| 2026-10-20 | P0 | **归因更正**：只读 cookie 核查（复制到临时目录 + sqlite 只读查询，源未动未删）证实本机**从未建立**任何站点登录态——13 条 cookie 全是匿名/埋点类，无 `userToken` / `__Secure-next-auth.session-token` / `__appKit_userInfo`。全文「登录态已失效」改为「本机从未建立登录态」；「用户重新登录」改为「用户首次建立登录态」 | dev profile Cookies 的只读拷贝 + sqlite 只读查询 |
| 2026-10-20 | P0 | **G3 通过（DeepSeek / ChatGPT / MiniMax 三站各 7/7）**；隔离硬证据：跑完后源 dev profile 的 Cookies/History/Preferences/Secure Preferences mtime 仍停在 13:24:48 | `%TEMP%\dsh-probe\G3-*.json` + `G3-r2-*.json` + `profile-mtime-before/after-r2.txt` |
| 2026-10-20 | P0 | 零痕迹回程：**Windows 已实测成立**（CDP 5/5，`getOwnPropertyNames(window)` 1239/1239、`__dsh*` 为 0）；**接口名更正**（`ICoreWebView2::CallDevToolsProtocolMethod`，WebView2 无 `ICoreWebView2DevToolsProtocolController`）；**macOS 侧无通路**（已知缺口，代码里明确报错不静默降级） | `%TEMP%\dsh-probe\A-cdp-feasibility.json` + `src-tauri/src/official_chat/cdp_eval.rs` |
| 2026-10-20 | P0 | 登记落地 + 门禁实测（`check:builtin` 4/38、`check:invariants` 全绿含第 30 项、`check:duplicates` 4/8、`test:plugin` 206、`test:probe` 6、**`cargo test` debug 1109 passed**）；**发现一**：手工复刻把物化目录写成行 id（`xlink-official-chat/…`），真链是 `spec.id`（`official-chat/…`）——纸面正确不等于链正确；**发现二**：`cargo test --release` 红 6 条是 release-only 假红（`ShellMode::current()` 编译期决定） | `src-tauri/src/plugins/builtin/builtins.rs` + `scripts/prepare-builtin-plugins.mjs` + 根 `package.json` |
| 2026-10-20 | P0 | feasibility 与本文件产出；计划/调研原文未改；未提交 | `docs/features/official-chat/` |
