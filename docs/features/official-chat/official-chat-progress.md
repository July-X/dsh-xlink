---
description: "官网对话直接引用的分阶段事实进度记录。P0 部分只写已发生的事实，不预写结论。"
kind: "progress"
status: "P0 机制层结论已出（G1/G2/G3）；真实链路与首站取数未闭合"
date: 2026-10-20
---

# 官网对话直接引用 · 阶段进度记录

本文件只记**已经发生的事实**：谁做了什么、在什么环境、用什么命令、产出了什么、结论是什么、哪些没做。**不预写结论，不写「待办」以外的推测**。

P0 的可行性结论单独见 [official-chat-feasibility.md](official-chat-feasibility.md)；实施规则见[开发计划](official-chat-development-plan.md)；方案与待验证清单见[调研](official-chat-bridge-research.md)。

## 阶段总表

| 阶段 | 内容 | 状态 |
| --- | --- | --- |
| P0 可行性验证 | G1 内核工作台选择与发送控制、G2 异步回程与首站可读性、G3 窗口未打开时仍可读取 | **机制层已出结果**（G1 4/5 问通过 + Q3 判定互斥；G2-A 5/5；G2-B 探测完成但取数未成立；G3 机制层 7/7）。**真实链路未闭合**：壳真实 workbench 复验输出 0 字节、G3 真实官网窗口 + 已登录态未跑、首站正文未读 |
| P1 协议与隔离 | 固定数据模型、错误、权限与状态机 | **未开始**（P0 的就绪状态纪律可并行开工，见 feasibility 第 9 节） |
| P2 内核插件闭环 | 固定数据的选择、引用、发送与模型输入核对 | **未开始** |
| P3 正文快照与桥接 | 原子保存、固定版本、回收、恢复 | **未开始** |
| P4 首站接入 | 真实目录、认证、分支与正文 | **未开始**（前置条件未满足） |
| P5 桌面体验与诊断 | 登录回程、错误、详情、能力检测 | **未开始** |
| P6 交付检查 | 接线、资源、兼容与关闭恢复 | **未开始** |
| P7 扩展站点 | 其他站点适配（MiniMax 排在此阶段） | **未开始** |

## P0 事实记录

### 2026-10-20 · G1 验证执行（devloper）—— 四次运行，结论取自第一与第三次

**做了什么**：在真实内核 release `0.2.1-rc.2` 上验证「@ 选择官网对话 → chip 落地 → 发送时读取正文 → 等待或拒绝」这条链路。内核跑在 `%TEMP%\dsh-xlink-g1\` 的**副本**上，`DSH_HOME` / `DSH_XLINK_HOME` 也都在该目录下，**未碰** `C:\Users\zxx\.dsh-xlink`。驱动对象是**内核自带工作台页面**（Edge `--headless=new` + CDP），模型是本机桩模型（OpenAI 形状），**不接真实官网、不用付费模型**，正文为固定夹具 `g1-alpha` / `g1-beta`。

**四次运行（各自独立取证，互不覆盖的只有第三次的 `final.json` / `reject-notice.json`）**：

| 轮次 | 时间 | 脚手架 | 结果 |
| --- | --- | --- | --- |
| 1 | 12:21–13:06 | `%TEMP%\dsh-xlink-g1\g1-*.mjs` + `cdp.mjs` / `kernel.mjs` | **五问跑通**，产出 `final.out.txt`(13:01:41)、`reject.out.txt`(13:03:58)、`q2v2.out.txt`、`q4-*.out.txt`、`q5*.out.txt`、`model.log`；交付 `delivery.md`、`delivery2.md` |
| 2 | 13:09–13:15 | 仓库内新脚手架 `plugins/official-chat/test/drive/run-g1.mjs` | **未跑出 composer**（工作台首页没有会话 → 没有编辑器），四条断言**未验证**；留下 **8 个 `%TEMP%\g1-probe-*` 目录（约 127MB）未清理** |
| 3 | 13:26–13:31 | 同上但**改走壳的真实 Rust 物化链**（`ensure_wired` + 打包资源 + `verify_manifest`） | **五问全部重跑**，数字与第一轮一致或更接近（等待 7439ms vs 7624ms；拒绝可见 1325ms→4994ms vs 1340ms→5031ms）；产出 `final.json`(13:26:00)、`reject-notice.json`(13:28:39)；交付 `delivery3.md`(13:31:45) |
| 4 | 13:49–13:56 | `shell-dev.mjs` / `shell-verify.mjs`（起**壳本身**：`dsh-xlink.exe` dev 壳 + Vite 5174 + WebView2 CDP 9335，`data_dir` 在 `%TEMP%` 下） | **未产出结果**：`shell-verify.out.txt` 与 `model.log` 均为 **0 字节**(13:56:03) |

**证据生命周期问题（已记入 feasibility 第 7 节第 6 条）**：`model.log` 被第 4 轮的空跑覆盖为 0 字节，「请求体含夹具正文」目前只剩 `final.out.txt` 的 `requestHasBody:true` 布尔字段支撑，**原始请求体不可复核**。

**插件物化**：第 1 轮**按源码手工复刻**到 `<DSH_HOME>/extensions/builtin/xlink-official-chat/0.1.0/g1-local/compat-a/`；第 3 轮改走**壳的真实 Rust 物化链**（源 = 打包资源 `src-tauri/resources/builtin-plugins/official-chat`，`npm run prep:builtin` 产物，`verify_manifest` 一并跑到；指纹 `dsh-0.2.1-rc.2-cordis-4.0.5` 由真实内核树算出；接线行由 `wiring.rs::row_block` 写进 `<DSH_HOME>/profiles/web/cordis.patch.yml`）。

**产出**：

- 插件代码（只在 `plugins/official-chat/**`，**未提交**）：`package.json`、`host/index.js`、`client/client.js`、`test/client.test.mjs`、`test/smoke.test.mjs`、`test/drive/run-g1.mjs`。
- 单测结果：`node --test plugins/official-chat/test/*.test.mjs` → **26 passed / 0 failed**；`npm run test:plugin` → 206 tests / 203 pass / 0 fail / 3 skip。
- 证据：`%TEMP%\dsh-xlink-g1\` 下的 `final.out.txt`、`final.json`、`reject.out.txt`、`reject-notice.json`、`q2v2.out.txt`、`q4-*.out.txt`、`q5*.out.txt`、`q5b.out.txt`、`model.log`(已空)。

**结论落点**：PM 当日拍板 P0 只做一条路（**甲**形态），插件随即收敛为**单变体**——`onPick` 同步返回 pending 句柄、正文由 `codec.serialize` 在发送时取、不再自插 chip。此前那条「读取完成后自己插 chip」的变体**实测被拒**（draftRev CAS），其代码已按拍板删除，遗留的测量作为「该形态被否」的依据保留在交付原文里。

五问结论、接口缺口 G1-1…G1-5 与逐条证据见 [feasibility 第 3 节](official-chat-feasibility.md#3-g1真实工作台内的选择和发送控制)。

### 2026-10-20 · G2 验证执行（tester）

**做了什么**：两部分。A 部分用**本地可控页**（自起 `127.0.0.1`，不接触真实站点）验证异步求值回程；B 部分对三个站点做**只读**探测，**只取目录元数据，未读任何正文**。

**过程中修掉的一个真 bug**：WebView2 的 `TryGetResultAsString` **只在结果是字符串时才给值**，数字/布尔/对象一律返回空串。修法是页面侧统一 `JSON.stringify`，平台层只取 JSON 文本、解析放共享层。

**PM 裁定后的判据修复（Q1/Q2，tester 复核）**：token 按**显式优先级**取（`userToken` → `accessToken` → `sessionToken` → `_token`）并输出**实际试过的键名**（`tokenKeysTried` / `tokenKeyTried`）；`tokenUsable` 拆成 `tokenPresent`（存在性）与 `tokenAccepted`（服务端接受），`tokenKeyFound` 改名 `tokenKeyTried`，**旧字段名已删**。「服务端接受」= HTTP 2xx **且** 业务 code 成功 **且** 响应里确有内容。顺带修掉一个同类错：第一版「有内容」判据写的是「`total` 字段存在」，而 ChatGPT 未登录时 `total: 0` 字段**是存在的** → 改成看**值**（`total > 0`）。行为判据 `src-tauri/examples/official_chat_probe_site.test.mjs` **6/6**（把页面脚本从 Rust 源里原样取出、按 `format!` 规则还原花括号，在 `node:vm` 的假页面里**真跑**，不靠源码字符串断言）。

**产出**：`%TEMP%\dsh-probe\A-controlled.json`、`B-deepseek.json`、`B-chatgpt.json`、`B-minimax.json`。

**门禁自查**：`cargo fmt --check` 通过；`clippy --locked --all-targets -D warnings` 零警告；`cargo test --locked` **1108 passed / 0 failed**；`check:invariants` 全绿。

**阻塞（事实）**：DeepSeek 与 ChatGPT 的**登录态已失效**（页面停在 `/sign_in`；ChatGPT 裸 fetch 200 但 `items` 为空），真实目录字段与分页形态因此未验证；正文读取按规则阻塞（用户未指定测试对话）。MiniMax 实测有效 origin 为 `agent.minimax.cn`（入口 `agent.minimaxi.com` 301），目录端点未验证。

**一处纪律问题（已记录在 feasibility 第 7 节第 2 条）**：B 部分使用的 profile 是 `C:\Users\zxx\.dsh-xlink\dsh\desktop-dev\webview-official-chat`，**在用户真实数据目录内**，与开发计划 §12「WebView profile 使用独立临时目录」的要求冲突。本机实测该目录下 `Cookies`/`History`/`Preferences`/`Secure Preferences` 的 mtime 均为 2026-10-20 13:24:48，即测试期内在写用户真实数据目录。**该次写入发生在 13:24:48**；协调者会话自述曾以真实 dev profile 跑站点探测，**因此最可能是协调者自己的探测**——写明这一点是为了让责任归属不悬空。后续探测须改用 `%TEMP%` 一次性 profile。

### 2026-10-20 · G3 验证执行（tester）

**做了什么**：`cargo run --locked --example official_chat_probe -- --mode g3 --site deepseek --profile "<dev profile>"`。

**隔离做法（本轮已按新纪律执行）**：先把源 profile **只读拷贝**到 `%TEMP%` 一次性目录（**730 文件 / 59,632,751 字节 / 跳过 0**），站点访问全部发生在拷贝上；清理后 `source_deleted:false`、`source_still_exists:true`、`temp_copy_removed:true` —— **只删自己的临时拷贝，源 profile 仍在**。跑完实测源 profile 的 `Cookies` / `Preferences` mtime 仍为 **13:24:48**（早于本轮跑批 30 分钟，不是本轮写的）。

**结果**：7 个场景的机制**全通** —— 页签已挂载复用、官网窗口关闭后按需建不抢焦点的上下文、尚未登录明确报 `not-logged-in`、上下文指纹变化即失效、重定向记录实际落点、所有权转移（`user_tab_still_alive:true`）、闲置回收（自建上下文可回收 / **用户挂载页签不回收**）。

**判定口径（必须随结论一起引用）**：本轮整轮跑在「**未登录 + 探针自建的模拟页签**」条件下，因此「复用已登录态并读到目录」「真实账号切换」「用户开着官网窗口时壳复用它的页签」**三项未验证**。⇒ **G3 机制层通过（7/7，条件受限）；真实官网窗口 + 已登录态 = 未执行。**

**顺带印证的一条判据边界**：场景 3 里 `page_has_user_info: true` 而 `logged_in: false` —— **profile 里有 userInfo/userToken 键 ≠ 已登录**。

### 2026-10-20 · 零痕迹回程评估（tester）

**结论**：**R1 页面槽位不成立为「零痕迹」** —— `Object.defineProperty(window, SLOT, {enumerable:false})` 只挡 `for-in` / `JSON.stringify`，`Object.getOwnPropertyNames(window)` 照样列得出；名字前缀固定 `__dsh_rt_`，页面探针扫前缀即可定位。痕迹短命但**在读取窗口内确实存在**，与 `harness/chat-fingerprint.js` 的既有纪律直接冲突。

**Windows 的零痕迹路径成立**：`--mode cdp` **5/5** —— 未开 DevTools 窗口也能调 `Runtime.evaluate`；带 `awaitPromise:true` 拿到**结算后的值**（`{"role":"assistant","value":42}` 形态），不带则得 `{}`（对照组）；`exceptionDetails` 与 `result` 同级；**页面 JS 空间零痕迹**（求值前后 `Object.getOwnPropertyNames(window).length` 1239 / 1239，样本 `[]`）。接口形态提醒：`CallDevToolsProtocolMethod` 是 **`ICoreWebView2` 自己的方法**，**不是** `ICoreWebView2DevToolsProtocolController`（WebView2 没有那个接口）。

**macOS 当前无通路**：wry 0.57 只有 `evaluateJavaScript_completionHandler`（`src/wkwebview/mod.rs:759` / `:772`），**没有** `callAsyncJavaScript` / `awaitPromise`；持有 raw `WKWebView` 的 `InnerWebView` 是 `pub(crate)`（`:132`，字段 `webview` 在 `:135`）→ 外部 crate 拿不到。**按 P0 未闭合项记，不与 Windows 并列成「两平台各一种」。**

### 2026-10-20 · 协调者裁定

六条裁定已记入 [feasibility 第 6 节](official-chat-feasibility.md#6-裁定与理由)：取数形态（用户手动触发、逐会话、只读）、MiniMax 留 P7、**R1 槽位不得带进真实站点**、**零痕迹回程首选 CDP（Windows 已定，macOS 无通路）**、R2 本地回环中继已被否（与「不再造一个监听服务」冲突）、Q3 互斥取甲形态且 pending 句柄不得冒充就绪。裁定 1/2/4 要求在 P1/P2 落为**硬性验收项**。

### 2026-10-20 · 隔离纪律落成机检判据

新增 `scripts/probe-profile-isolation.test.mjs` + `scripts/lib/probe-profile-isolation.mjs`（10/10），并作为 `check-invariants` 的第 30 条不变量落地：测试/探针**不得**以用户真实数据目录下的浏览器 profile 启动 WebView2。落地时**先跑出一次真红**才改（红点精确命中事故那一行，全仓只红这一条），随后注入假违规做活体复验，门禁报出 file:line → 删掉 → 绿。判据自测反过来抓到判据本身的真 bug（文件级放行标记带注释前缀时不生效）。

### 2026-10-20 · P0 文档产出

- 新建 [official-chat-feasibility.md](official-chat-feasibility.md)：P0 唯一对外结论，含环境与版本、G1/G2/G3 逐项结论与证据、裁定与理由、文档与实测的出入、本轮没有验证的、是否可以继续。
- 新建本文件：阶段事实进度。
- **未改动** `official-chat-development-plan.md` 与 `official-chat-bridge-research.md`（出入只在 feasibility 第 7 节单列）。
- **未执行** `git commit` / `git push`；改动留在工作树。

## P1 及以后

尚无事实记录。

P1 开工时需要先落地的输入（来自 feasibility 第 9 节，先决条件，非本次结论）：

1. 就绪状态纪律：pending 句柄不得冒充就绪；label 保持中性；`openReference` 按 ref 如实报 pending/ready/failed；「就绪」由 chip 状态与详情承担，不以「chip 是否存在」承担。
2. G1-1…G1-5 登记为已知上游缺口，实现中不得假设它们已解决。
3. 页内求值走零痕迹回程（Windows 走 CDP `Runtime.evaluate` + `awaitPromise`；**R1 槽位不得带进真实站点**；macOS 当前无通路，按未闭合项处理）。

## 交接记录

| 日期 | 阶段 | 事实 | 证据 |
| --- | --- | --- | --- |
| 2026-10-20 | P0 | G1 五问完成（4 通过 / Q3 判定互斥），插件收敛为甲形态单变体，单测 26 passed；第 3 轮改走真实 Rust 物化链后五问重跑 | `%TEMP%\dsh-xlink-g1\delivery2.md` + `delivery3.md` + `final.out.txt` + `final.json` + `reject.out.txt` + `reject-notice.json` |
| 2026-10-20 | P0 | G1 第 4 轮「壳真实 workbench 复验」起壳成功但**输出 0 字节，未产出结果** | `%TEMP%\dsh-xlink-g1\shell-dev.log` + `shell-verify.out.txt`(0 字节) |
| 2026-10-20 | P0 | G2-A 异步回程 5/5 通过；G2-B 三站只读探测完成，真实取数结论因登录态失效未成立；Q1/Q2 判据已修并补行为判据 6/6 | `%TEMP%\dsh-probe\*.json` + `src-tauri/examples/official_chat_probe_site.test.mjs` |
| 2026-10-20 | P0 | G3 机制层 7/7 通过（未登录 + 模拟页签条件下）；真实官网窗口 + 已登录态未执行 | 探针 G3 跑批（临时目录时间戳 `1792475664714`） |
| 2026-10-20 | P0 | 零痕迹回程：R1 槽位产品上站不住；Windows CDP 5/5 成立；**macOS 当前无通路（P0 未闭合项）** | `--mode cdp` 跑批 + `script_eval.rs:434-436` / wry 0.57 `src/wkwebview/mod.rs:723-780` |
| 2026-10-20 | P0 | 隔离纪律落成机检判据（`check-invariants` 第 30 条，10/10） | `scripts/probe-profile-isolation.test.mjs` + `scripts/lib/probe-profile-isolation.mjs` |
| 2026-10-20 | P0 | feasibility 与本文件产出；计划/调研原文未改；未提交 | `docs/features/official-chat/` |
