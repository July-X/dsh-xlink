# AGENTS.md — `src-tauri/`（Tauri Rust 进程）

根 [AGENTS.md](../AGENTS.md) 的后端分支。根文件里的「范围」「数据目录」「发布」各章对后端同样适用，这里只收改 `src-tauri/**` 之前要知道的约束。前端见 [ui/AGENTS.md](../ui/AGENTS.md)。

提交前跑 `cargo clippy --all-targets`（零警告基线）与 `cargo fmt`。

## 目录约定

`src-tauri/src/` **按功能分目录**，不要往根目录平铺。2026-10-01 重组过一次：当时 60 多个 `.rs` 加 6 个注入脚本 `.js` 全堆在根目录，找一个模块得先把整个目录扫一遍。分组与 `ui/src/` 对齐，一层目录说清一件事。

```
src-tauri/src/
├── lib.rs / main.rs / commands.rs        # 入口、setup、面板命令层，就这三个留根
├── shell/          # 壳自身：进程、路径、实例、状态、设置、窗口、托盘、错误
├── kernel/         # 内核生命周期：安装、钉版、适配器、看护退避（lifecycle.rs）
├── harness/        # 工作台窗口：建窗 / 自愈 / 草稿与附件 / 官方对话 + 注入脚本
├── plugins/        # 插件中央库、补丁、安装预检、沙盒
├── skills/         # 技能中央库、物化、同名冲突
├── diagnostics/    # 安全网：启动看护、环境回退点、恢复后自检、二分定位
├── migration/      # 迁移向导与找回历史会话
├── pkg/            # 取源与出网：npm/GitHub、归档、发布列表、自身更新
├── node/           # Node 检测与托管安装
├── notify/         # 任务通知与「点通知回到工作台」
└── usage/          # 用量：本地 token 账目、云端套餐、凭据只读解析
```

三条约定：

- **子模块声明为 `pub(crate) mod x;`，不做 `pub use x::*` 重导出。** glob 重导出在两个子模块导出同名符号时会变成「歧义」，而显式路径 `crate::<组>::<模块>::X` 既无歧义，也保留了「这个符号来自哪个模块」的信息——这在 `shell/`（14 个模块）和 `pkg/` 这种大组里是刚需。
- **`lib.rs` 只声明组，不声明叶子。** 加一个模块要改两处（组内 `mod.rs` + 该组自己的调用点），这是有意的：组目录的存在就是为了让「有哪些模块」这件事在一个文件里看得全。
- **搬文件时会跟着失效的东西，已经全部改成按 basename 认了**——`scripts/lib/shell-source.mjs`（门禁脚本与 UI 测试共用的一份解析器）、`check-invariants.mjs` 的 `baseName()` / `inFile()`、`check-code-budget.mjs` 的 `isKnownBlob()` / `moduleId()`。**新增判据时照这个来**：判据要问的是「哪个模块」，不是「文件在哪一层」。2026-10-01 那次重组的实测代价：9 项不变量一次性转红、5 个测试 ENOENT 挂掉、`check-invariants` 自己在启动阶段就崩——红的原因与要检查的东西全都无关。反过来说，`FILE_BUDGETS` 里登记的**路径**仍要跟着改（那是给人看的），但**反棘轮比对**按模块标识走，所以搬过的文件不会被当成新文件。

本文下面各章沿用**不带目录的文件名**（`kernel.rs`、`paths.rs`、`harness_draft.rs`…）指代模块，指的是同名文件，上表给出它现在在哪。

## 错误信息

- **错误信息必须包含可操作的下一步与相关日志路径。** `AppError::Skill(...)` / `AppError::Io(...)` 那些字符串是用户**唯一**能拿到的东西（GUI 应用里 `eprintln!` 在 Windows 上没有去处），写成「操作失败」等于让用户自己猜。写不出来就先问一句「出事了他查什么」，查不到说明这个设计还没完成。

## 进程、出网与落盘

- 所有 GUI 子进程使用 `process.rs` 的 PATH 合并、静默窗口和进程组回收策略；涉及进程、网络或目录树的 Tauri 命令必须异步执行并使用 `spawn_blocking`。
- **Rust 侧出网必须走 `net_proxy::routes()`，不要自己造客户端**。壳访问 GitHub 的唯一路径是更新检查 / 下载更新（2026-09-30 用户实测：系统里明明开着代理，检查更新却直连 GitHub 然后报 `error sending request for url`）。根因是 reqwest 只认 `HTTP_PROXY` / `HTTPS_PROXY` / `ALL_PROXY` 环境变量，**不读系统设置**——读注册表 / `scutil` 的那半边（hyper-util 的 `client-proxy-system`）没开，而壳是 GUI 程序、从资源管理器启动，继承不到用户为命令行设的变量。于是约定三条：① **新增 GitHub 出网路径一律复用 `routes()`**（先系统代理、失败再直连，末位恒为直连），复制一份探测逻辑就会重新长出「只认环境变量」的同一个 bug；② **直连必须显式 `no_proxy()`**——否则回退到直连仍被 `HTTPS_PROXY` 拉回代理，等于同一条路试两遍；③ **只有传输层失败才回退**（`should_try_next`），签名 / 清单 / 版本号解析失败换一条路只会同样地失败，重试还会让「第二次也这样」盖住第一次的真正原因。前两条单测抓不到（路由表是纯函数，测的是形状），由 `check-invariants` 第 14 项钉住：`updater_builder()` 在生产代码里只许出现一次，且那处必须同时接上 `Route::Direct => no_proxy()` 与 `Route::Proxy => .proxy()`。另两条出网路径不归它管：WebView2 / WKWebView 本身就跟随系统代理，pnpm / npm 读的是 npm 自己的 proxy 配置。
- **用户看得见后果的后台动作必须落盘，不能只 `eprintln!`**。壳是 GUI 应用，Windows 上 `eprintln!` 没有任何去处（没有控制台可接，dev 模式从终端起才看得到），而**恰恰是那些只有后果、没有原因的动作用了它**：工作台窗口自动重载、给 pnpm 降优先级、自愈与降级。2026-09-29 的代价：dev 壳装内核的 8 秒里 release 壳的工作台被重载并撞上内核的启动顺序竞态，壳这边**一行日志都留不下**，用户与维护者只能靠 `last-incident.json` 的时间戳对猜。凡是用户看得见现象的动作都走 `shell_events::record(<逻辑名>, <行>)`——它按日轮转落进 `shell_logs_dir`，因此**自动出现在「查看日志」面板**。写失败只落 stderr，绝不阻断调用方。**新增后台动作时先问一句「出事后怎么查」**，查不到就说明这个设计还没完成。
- **依赖钉版可以降级，但不得跨版本线，也不得静默**。内核 monorepo 锁步发布的前提是每个子包都发了，漏发时 pnpm 会在解析阶段拒绝整棵依赖树（2026-09-29 实测 `0.2.0-rc.2` 缺 `dsh-client-ui-settings-account@0.2.0-rc.2`，**官方与镜像 registry 都没有**，换 registry 救不了）。壳的处理在 `kernel_deps.rs`：失败后解析断边，查当前 registry，给每条断边选**同一 `major.minor` 版本线内、语义化更低**的已发布版本钉进 `overrides` 再重试。三条纪律：① **只退到更低、只退到同一条版本线、只退到同一稳定性层级**——更高的版本与内核其余子包的配套关系未经测试，跨 minor 的差异可能已经是不兼容，壳无权替用户决定，那种情况宁可不装并如实说「上游发布不完整，换个内核版本」；**跨稳定性层级同理**：语义化里 `0.2.0-rc.2 < 0.2.0` 成立，所以只判「严格更低」会把正式版的需求悄悄换成 RC，而提示文案只说「已改钉到同一版本线上已发布的较低版本」，用户看不出装出来的是个 RC——`is_pre_release` 那道判断不能省；② **降级必须说出口**，每条进进度面板并汇总进安装成功提示，让用户知道这份内核不是原样的、差在哪；③ **降级钉版要活过下一轮**——锁步重装写 overrides 时必须与它合并，覆盖写会把包钉回那个不存在的版本，pnpm 立刻二次失败。预检放在失败之后而非安装之前，正常安装不多付网络往返。

## 窗口与注入脚本

- **每个窗口都要有底色，包括 `tauri.conf.json` 里声明的主面板**（2026-10-01）。没有它时 WKWebView 在文档首帧之前一律绘制纯白，`background_color` 在 macOS 上完全看不到效果（窗口底色被 webview 盖住）——症状是启动时闪一屏白、加载慢的观感，而它其实一秒就好了。运行时创建的 8 处 webview 一直传着 `background_color(backdrop)`，唯独主面板是 conf 里声明的、启动时用户看到的那一个，一直漏着。补的是 `"backgroundColor": "#0b1020"`，取自 `theme.css` 的 `--bg`（面板是**纯深色**：`:root { color-scheme: dark }`，没有浅色变体，也没有 JS 切类名，所以写死深色不会在切主题时闪错）。**注意别照抄工作台那个 `chrome_backdrop()` 的 `#16171a`**——那是给工作台 / 官方对话这些加载**第三方页面**的窗口用的中性底色，面板自己的底是蓝调的 `#0b1020`，两者不是一个东西。改窗口时顺手问一句「这个窗口首帧之前是什么颜色」。
- **窗口里一律禁右键菜单，但一个字都不许碰「选中」**（2026-09-30）。三类窗口各有一处落点：壳自己的五个窗口（`main` / 日志 / 用量 / 套餐 / 官方对话页签栏）共用 SPA 入口，走 `ui/src/shell/noContextMenu.js`（`main.js` 在 `app.mount` 之前调一次）；工作台与官方对话三个内容 webview 加载的是**别人的**页面，走 Rust 注入的 `src-tauri/src/harness/no-context-menu.js`（`window` 捕获阶段 `preventDefault` + `stopImmediatePropagation`，页面自绘的菜单才一并拦掉）。**左键拖选与 Ctrl/⌘+C 复制必须照旧可用**：写 `user-select: none` 或拦下 `copy` 不会报任何错，症状只是用户再也复制不出东西。`check:invariants` 第 16 项按「每条 `WebviewUrl::External` 建窗链」逐条查——工作台有**两条**建窗链（`commands.rs::open_harness` 走用户点击那条，`harness_window::build` 走自愈 / 手动刷新那条），历史上 `harness-draft.js` 就是因为要接两处才留下这条隐患，**新增远程窗口或改这两条链时别只改一处**。引擎层的菜单（Wry `with_default_context_menus` / WebView2 `AreDefaultContextMenusEnabled`）在 Tauri 2.11 上没有对外接口，这一层只能靠 DOM 事件取消。
- 工作台草稿的注入脚本 `harness-draft.js` 与编解码 `harness_media.rs` 也归本侧：它们注入进**别人的**页面，只能经 `capabilities/harness-remote.json` 授权后 invoke，**命令声明与 capability 两处都漏就是真机上 ACL 静默拒、`.catch` 吞掉、功能完全不工作**。

## 内核安装与跨壳

- **装 / 删内核不许惊动另一个壳的工作台（2026-09-30 定案：pnpm store 的 inode 侧信道，不是资源争用）**。两棵内核安装树物理不相交，但 pnpm 的内容寻址 store（`~/AppData/Local/pnpm/store/v11`）**按文件内容寻址**——两个壳的树与 store 里内容相同的文件是**同一个 inode**（`fsutil hardlink list` 实证：release rc.2 的 `dsh-client-ui-session/lib/client.js` 被 store、release rc.1 树、dev rc.2 树四条路径共享）。而 **NTFS 上硬链接数增减会更新 ChangeTime**（实测：加/删一个链接，`stat.ctimeMs` 变、`mtime` 不变），内核 `dsh-client-hmr` 的 node 侧**每 500ms stat 轮询每个客户端 bundle**（`sameBundleStat` 比较 mtime/ctime/size），把这种噪声当成「bundle 重建」推 SSE `rebuilt` 帧给**活页面**，页面换模块的窗口里 `dsh-client-ui-session` 被销毁、session 作用域被摘，`ScopeProvider` 重渲染即抛 `scope 'session-maybe' rendered without an installed adapter` 死掉。**当天五次装 / 删全部在 4~6 秒内打死对面的工作台页面，与 CPU / 磁盘无关**（触发安装只跑 9.2s、537 个包全部从 store 复用、BELOW_NORMAL 优先级；机器 14 核 / 64GB / NVMe——「资源争用」「看门狗误重载」「事件风暴惊动监视器」三个旧说法全部不成立：当天看门狗重载计数为 0，内核日志一行都没有）。**完整证据链、三个被证伪的理论、排查工具箱与方法论见 [docs/case-2026-09-30-inode-side-channel-blackscreen.md](../docs/case-2026-09-30-inode-side-channel-blackscreen.md)**——遇到「分了家仍互相惊动」的一类问题先照它来。
  - **⓪ 根治：内核安装用 `--config.package-import-method=copy`**（`kernel.rs` 安装参数，锁步重装复用同一份 args）。树持有全新 inode，装 / 删从此**物理上碰不到对面的任何文件**——连用户自己的 pnpm 项目也不再能惊动正在服务的工作台（修之前它们走同一侧信道也能）。代价：每个版本真实占盘约 450 MB（不再与 store 硬链接共享）、安装慢几秒；这是「已安装的内核是一棵独立的、不可变的树」这一语义的正确实现。**存量硬链接树不迁移，但被看见**：`install_isolation.rs` 采样读硬链接数判定每棵树的实况（任一采样文件链接数 >1 即共享；读不出来按共享——保守方向），`InstalledVersion.shared_storage` 落到版本页行上的「共享存储」标记；把带标记的版本**卸载后重装一次**即隔离、标记消失。覆盖重装不保证重新导入全部文件，别当迁移手段。`check-invariants.mjs` 有机械检查钉住这个参数（被删时编译不报、测试不红，只有它会响）。
  - **① 信标（`package_activity.rs`）的真实职责是「恢复退避」，看门狗放宽只是防御**。真机数据：看门狗「把慢判成死」从未触发（当天重载计数 0）；反而是**恢复动作落进风暴**各死了一次（页面 3s 自愈刷新、壳自动重建）。因此 `recovery_backoff()`（封顶 120s）是主职——页面自愈刷新前经 `harness_reload_backoff` 命令问它（**注意 ACL：harness 窗口是 remote origin，命令必须授进 `capabilities/harness-remote.json`**），壳的 `recreate_when_quiet` 等它；`load_timeout()`（clamp 15s→120s，只放宽不收紧，硬顶必须有——放宽到 10 分钟等于把关门狗关掉 10 分钟）保留为防御。
  - **跨壳判据只提示，不阻断**（`kernel::warn_other_shell_workbench` → `instance::workbench_running_in_other_shell`）。曾经它是硬拦（2026-09-30 上午），理由是「装包会打死对面工作台」；⓪ 落地后**装**的路径已根治，**删旧版硬链接树**仍可能惊动对面一次且会自愈——为这一次可自愈的惊动硬拦，代价是废掉双壳并行，比例不对。现在它**按实际残余风险出现**：横幅 = 本壳还有共享 inode 的版本（采样判定）× 对面正在服务的树也共享——两侧任一独立它就消失，不再无条件常驻吓唬已隔离的用户；`warn_other_shell_workbench` 同理只在删 / 重装共享树时说（全新安装安静即正确）。文案把后果与出路说清（删共享版本会惊动一次、会自愈、点「刷新工作台」能回来）。`warn_other_shell_workbench` 的**返回类型刻意是 `()`**——返回 `Result` / `Option` 就能被调用方接成一次拒绝，而那条路等于把双壳并行关掉。
  - **本壳那条守卫（`ensure_own_shell_stopped`）三条动作都要留**：装 / 删改的是自己脚下那棵树，运行中的内核就在里面。「切换版本也不拦跨壳」是另一层理由——它写的是本壳树里的 `active.txt` 一个文件，对方够不着。
  - **分家与跨壳提示不矛盾**：分家分的是**路径**（树、注册表、插件中央库、端口、实例 id），此前没分到的是 **pnpm store 的 inode**（内容寻址按文件去重，跨树跨壳共享）——⓪ 补的就是这一层。别再把「机器资源（同一块盘、同一个 WebView2 进程池）」当作跨壳惊动的解释：2026-09-30 已证伪。
  - `check-invariants.mjs` 第 12 项把这套范围钉成机械检查：三条动作都必须过本壳守卫、旧的跨壳硬拦入口不得复活、跨壳调用点恰好 2 处、**跨壳提示的返回类型必须是无**。最后一条是唯一挡得住「把提示接成阻断」的判据——**只按函数名查是不够的**，第一版检查就栽在这里：它一路绿着，而把提示接成 `Err` 的改法畅通无阻。
  - **恢复链按层兜底，各管一段，不要指望某一层单独解决问题**。原始事故（2026-09-30 上午的版本）：dev 壳装内核，release 壳的工作台黑屏，**用户正在输入的内容全丢**。当时把它归因于「看门狗把慢判成死」，下午的完整日志推翻了这个链条——页面是被内核 `client-hmr` 的 `rebuilt` 帧换模块换死的，看门狗全程没响。
    - **⓪ 让触发不发生**：`--config.package-import-method=copy`（见上）。**装新版本从此根治**；删旧版硬链接树与依赖 `dsh-client-*` 的插件安装仍走同一侧信道，靠下面几层兜底。
    - **① 万一被惊动了，别把恢复落进风暴里**：`package_activity.rs` 信标 + `recovery_backoff()`。页面自愈刷新前问 `harness_reload_backoff`（≤5s 一轮、上限 30 轮、IPC 失败按老行为 3s 照刷）；壳自动重建走 `recreate_when_quiet`（后台线程等风停、上限 240s、落地前重核内核在服务、单等待者）。看门狗阈值放宽（clamp 15s→120s，只放宽不收紧、有硬顶）保留为防御。
    - **② 万一还是被换掉了，输入不丢**：`harness_draft.rs` + 注入脚本 `harness-draft.js`。**必须经壳落盘**，因为 `recreate` 会换掉整个 webview，sessionStorage 直接没了（自愈额度 flag 就是这么丢的）。输入框是 **Lexical 富文本编辑器**不是 textarea，所以读用 `innerText`、写用 `execCommand('insertText')` + 读回来核对——直接改 `textContent` 只会「看起来有字」，一发送就没了。**恢复必须先在页面上找到能写的地方，再去壳里取**（`take` 是读走即删的，顺序反了会在页面还没装配好时把草稿吞掉）。记草稿的时机是**停止输入 600ms 之后**，不是页面卸载时——卸载那一刻再发起 IPC 多半等不到回程。
      - **草稿有两半，「没发出去的存」之外还有「发出去的作废」**（2026-09-30 用户明说：「已经发送的消息，下次打开工作台不应该再次填充在输入区域」）。发送时 Lexical 是**程序化**清空编辑器的，**不派发 `input`**，所以「空了」光靠监听收不到；只写不清的话那句已送达的话就静静躺在盘上，等着下次打开工作台时自己坐回输入框，而恢复那侧看到的是一段「新鲜的草稿」，无从知道它已在会话里。两侧各补一道：页面侧 `watchComposer` 每秒看一眼（**只在内容真的变了时才动作**，还在打字时让位给 600ms 停顿判据），本页存过且现在空了就走 `clear_harness_draft`（新命令，权限在 `allow-clear-harness-draft` + `capabilities/harness-remote.json`，两处都漏就是真机上 ACL 静默拒、`.catch` 吞掉、功能完全不工作）；数据侧 `stash` 收到空文本**或超长文本**即删除而非原样返回（存不下现在这段时，交回一段更旧的话比不交更糟）。
      - **清盘有两个刻意的不作为，改这条时别顺手去掉**：**页面上一个可见的可编辑元素都没有时不清**——黑屏那一刻很可能正是 composer 消失的时候，这时的「空」不是「发出去了」，清盘等于亲手删掉用户唯一没发出去的那段；**本页没存过东西时不清**——恢复失败放回盘上的那一份是唯一一份。判据刻意用「可编辑元素里的字没了」而**不是**「点发送 / 按 Enter」：内核换一版就可能换掉 class 名与文案，而前者与内核版本无关。残留窗口：发送后一秒内页面被换掉且那次 IPC 没落地，下一次打开仍会回填。
      - **「已发送」的判据只能读 composer，不能读「页面上的某个输入框」**（2026-09-30 用户复报，0.3.7-rc.1 仍复现）。`currentText()` 在焦点不落在可编辑元素上时曾退回「所有可见可编辑元素里**文字最长的**那个」，而 `stashNow` 判断「这句话是不是已经发出去了」用的正是它是否为空——页面上任何别的输入框（搜索框里残留的旧查询最常见）都让它恒为非空，于是 `clear_harness_draft` 永远不触发，盘上那份**已经发出去的**草稿留到下次重启才被填回输入框，同时那份还被覆盖成别的框里的字。实测证据：`stash_harness_draft`「已发送」→ 紧接着 `stash_harness_draft`「上一次的搜索词」，中间没有 clear。内核侧不是假设：0.2.0-rc.2 的客户端里有 **5 个包**渲染 `contenteditable`（composer 只是 `dsh-client-ui-conversation` 一个）、十余个包渲染 `textarea`/`input`（命令面板、侧栏、目录选择…）。修法是新增 `composerEditable()`（先取 `[data-composer-card]` 里的，卡片不在就取**最后一个**可见可编辑元素，理由同原 `emptyEditable`），`currentText` 与 `emptyEditable` **都只认它**。`emptyEditable` 那一侧是同一根因的另一面：它取「最后一个空的可编辑元素」，composer 里有用户新敲的字而上方有个空搜索框时，倒着找会撞上搜索框，把草稿写进搜索框。两条都钉了单测并逐条反向验过。**残留窗口（如实记）**：清盘判据挂在 1s 轮询（`WATCH_INTERVAL_MS`）上，发送后 1s 内工作台窗口被换掉（用户手动重开或自愈重建）则这一拍来不及跑。缩小它只是把轮询调密，不是修复；真要关掉得让恢复侧拿会话的最后一条消息去比对，那是另一件事的量级。
      **草稿还要连「还没发出去的图」一起保管**（2026-09-30 用户追加）。图在核心里**不是编辑器的一部分**，而是 composer 卡片上一排 `blob:` 缩略图（`URL.createObjectURL(file)`），字节只活在那一个页面里，**页面上没有任何服务端副本**——所以只能在页面还活着时 `fetch(blobUrl)` 读出原始字节交给壳（**不能用 `img.src`**，那是浏览器按视口转码过的显示图），页面上过不来的东西就别假装保管：**非图片附件走的是立即上传**（`beginFileUpload`），DOM 上只剩一张文件卡片，页面上拿不到字节。四个要点：
        - **判据用 `[data-composer-card]` + `img[src^="blob:"]`**，都是内核自己渲染的语义钩子，不是哈希 class 名（真机实测 0.2.0-rc.2 的 DOM 上确实有）。会话历史里的图走 http(s) 媒体地址，所以 `blob:` 精确地就是「还没发出去的那几张」。
        - **恢复走 paste，不走文件选择框——这是实测出来的，不是猜的**（2026-09-30，无头 Chromium 连着正在跑的 0.2.0-rc.2 工作台做的）：往 `<input type="file">` 塞 `DataTransfer.files` 再派发 `change`，`input.files` 确实被填上（值是 `C:\fakepath\…`），但内核**一张也没收下**——React 的 value tracker 把合成 change 判成「值没变」而丢掉整个事件。往编辑器派发带 `clipboardData` 的 `ClipboardEvent('paste')` 则一次就中：那正是**用户自己粘贴截图时内核走的同一条路**（Lexical `PASTE_COMMAND` → `intakeFiles` → `createDrafts`）。**改这条前请重测，不要凭读代码下结论。**
        - **注入成功 ≠ 恢复成功**：派发之后数一遍轨道上的图有没有变多，最多等 3 秒。没变多就是假恢复（用户以为图还在，一发送就没），按「文字成没成」分两种处置：都没成 ⇒ 整份放回（输入框还空着，是干净的重试）；**文字已回填而图没进去 ⇒ 清盘不放回**（再放回只会让下一次**空的**输入框收到一份重复文字），并把「只成了一半」写进控制台与 `__DSH_HARNESS_DRAFT_PARTIAL__`。另一个真踩到的坑：`fill()` 写完字之后 `emptyEditable()` 就返回 null 了，所以**编辑器元素必须在填字之前取好传进去**。
        - **上限比内核更紧并如实报数**：8 张 / 单张 4MB / 合计 12MB（内核自己是 20 张 / 20MB / 200MB，而这些字节还要以 base64 过一次 IPC）。存不下的按**先到先留**丢弃，张数一路带回页面（`dropped`）并 `shell_events::record` 落进「查看日志」——**悄悄少给几张比说清楚更糟**。编解码在 `harness_media.rs`（自己写的，不引依赖），`harness_draft.rs` 只管生命周期。
      - **作废判据是「既没字也没图」而不是「文字为空」**——只发图不发字是最容易漏的那一种，而它恰恰是最常见的一种（甩张截图就发）。
    - **③ 万一黑屏了，自己好，且别把额度一把梭**：`report_harness_fault` 接上 `recreate_after_fault` → `recreate_when_quiet` → `recreate`（预算 `MAX_REBUILDS=3` + `REBUILD_COOLDOWN=20s`；曾经是「每进程一次」，2026-09-30 下午的真机数据把它证伪：自动重建落在卸载风暴中间，新窗口 4 秒后又死、额度已尽，用户被晾在黑屏上 25 秒直到手动刷新。防打架的闸是冷却，不是一次性）。手动「刷新工作台」先 `reset_budget` 清账再进 `recreate`，手动出路永远有额度。判据与动作都放在 `harness_window`（自愈链条该待的地方），命令层只递两行证据；`commands.rs` 是反棘轮文件，在那里多写一行就要从别处省一行。
    - **只有 ⓪ 能让事故不发生，① ② ③ 都是兜底**。内核侧的两个缺陷（`client-hmr` 把 stat 噪声当重建推给活页面；页面换模块窗口里槽位装配不变量会崩）归内核仓库，值得把本条的实证链反馈过去——壳不改内核代码。
    - **这一类改造最容易被单测骗过去**：③ 的判据是纯函数，测得再准，把调用摘掉照样全绿（实测把 `port_open(settings.port)` 改成 `false`，判据测试依然 ok）。所以机械检查钉的是**接线形状**（命令层必须调 `recreate_after_fault`，而那个动作里必须问 `should_recreate_after_fault` 且走 `recreate_when_quiet`）而不是行为。写这类检查还要防另一件事：**note 跟着别的变量走会永远打印「通过」，把失败盖住**——判据本身在骗人比没判据更坏。
    - **残留缺口（如实记在 `docs/troubleshooting.md` 末尾，别让人重新踩一遍）**：① **存量硬链接树**——版本页带「共享存储」标记的就是；删除 / 重装它会惊动对面页面一次（自愈兜住），卸载后重装一次即根治、标记消失；本壳全部版本隔离后跨壳横幅自动消失（采样有理论漏判面：锁步重装只换部分包的混合树可能被误判独立，靠两侧门控 + 自愈兜底）；**依赖 `@deepseek-ai/dsh-client-*` 的插件安装**理论上仍能经同一 store 触发（插件装包不走内核安装参数），同靠自愈兜底。② **草稿有 600ms 窗口**（同前）。③ **内核侧两缺陷仍在**（见上）。④ **插件安装 / 更新不打信标**——恢复退避只在内核装 / 删场景踩过刹车，插件场景尚未观察到需要（`plugins.rs` 是反棘轮文件，真需要时再补）。

## 配置回退与恢复

- **变更配置前必须留下回退点**。装 / 卸 / 更新插件、切物化模式、切内核版本之前调 `snapshot::record(..., reason::PRE_CHANGE)`；只有**真正起来并应答过、且没有事故**的启动才算 `startup-ok`（带事故启动的环境不算"良好"——看护停用两个插件才起来的状态，记成良好会让恢复把"被降级过的样子"当成用户原本的样子）。改动走 `run_plugin_mutation_command` 而不是 `run_plugin_command`：**读类命令（检查更新、拉目录）打「变更前」是假的**，会把真有价值的回退点挤掉。打点**绝不阻断用户操作**——快照写不进去只写 stderr。裁剪时 last-known-good 进保护区，有测试钉死（`snapshot::tests::prune_never_drops_the_last_known_good`）。P0 只有只读面，恢复属 P1，见 [docs/safety-net-design.md](../docs/safety-net-design.md)。
- **二分定位的结论不得叫「根因」**。`bisect.rs` 的 `Conclusion` 只有 `minimal-bad-set` / `not-in-set` / `aborted` 三种取值，**没有"找到根因"**——组合效应（两个扩展单独都正常、一起就炸）会让二分停在一个不可修的答案上，把它说成根因会让用户去卸一个无辜的插件。试探的判据走 `verify::probe_once`，与恢复后自检**同一条**：判据一旦有两份实现就会分叉，而分叉出来的那个会让二分**静默收敛到错误答案**（把"没试成"当成"起来了"，坏的那半边被记成已排除）。`Inconclusive` **绝不等于** `Pass`。
- **恢复必须先看差异，且只改差异项**。`restore::diff` 与 `restore::restore` 是**两条命令**，不要合成一条：用户必须先看见将要失去什么再点确认，合一意味着要点一次「恢复」才知道后果。`restore::restore` 遵守四条硬规则：动手前先 `pre-restore` 备份（**备份失败必须中止**，没有回退点的恢复是单向操作）、逐条比对只改不一致的项、**只停用不卸载**（插件写 quarantine 记录，技能走 `set_enabled`，两者都不删中央库条目——卸载不可逆，让一次回退顺手做了等于用恢复换数据）、动不了的条目进 `skipped` 照实报。`Restorable: false` 的条目必须在**用户确认之前**就标注出来；事后才说等于让用户在一个不完整的承诺上点了确认。恢复后用 `verify::probe_once` 实测一遍，`verified` 必须如实反映——让"没验"看起来像"验过没问题"是最伤信任的错。恢复要求工作台已停止，且那条停止判据是**实例级**的（`instance::instance_kernel_running`，与「找回历史会话」同一份判据与文案）：用户自建实例在注册表分家后有意留在两份注册表里，只查本壳工作台会漏掉「另一个壳正跑着它」，而恢复改的正是那个实例的 profile 接线与插件物化。

## 预检

- **预检类功能必须先跑基线再判失败**。装了候选扩展起不来，不能直接判"扩展坏了"——可能是环境本来就坏了。`guard.rs` 早就为同一个问题付过代价（宁可放弃插件归因，也不肯因为环境问题停用无辜插件），预检沿用同一条纪律：不装任何东西先起一次作为基线，只有基线正常、装了候选才失败，才判 `Fail`。判定必须三态（`pass` / `fail` / `inconclusive`），`Verdict::as_str()` 是与前端 `PrecheckDialog.vue` 的**契约边界**——改一边不改另一边会把"未通过"画成"通过"，有测试钉死（`precheck::tests::verdict_strings_match_the_ui_contract`）。前半半条见 [ui/AGENTS.md §跨前后端的契约](../ui/AGENTS.md)。
