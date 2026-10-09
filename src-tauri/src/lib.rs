//! dsh-xlink：围绕 DeepSeek Harness 内核的 Tauri 壳。
//!
//! 壳负责管理固定版本的内核（通过 pnpm 从官方 `dsh-v*` 发布版本安装）、
//! 运行当前激活内核的 `dsh web` 服务器，并在专属 webview 窗口中打开其
//! UI。所有管理操作都通过 [`commands`] 中的命令，由本地 `ui/` 前端
//! 发起。
//!
//! ## 死代码策略
//!
//! `tauri dev` 默认会打开所有编译期 warning。多内核改造（P0–P8 共 9 个
//! stage）有意保留了一批「预留 API」——例如 [`commands::plugin_install_instance`]
//! / [`commands::plugin_sync_instance`] 等实例范围命令、
//! [`shell::instance::allocate_port`] / [`shell::instance::InstanceLock`] 等端口与锁原语、
//! [`kernel::kernel_adapter::AdapterCapability`] / [`kernel::kernel_adapter::resolve_install_root`]
//! 等适配器 trait 骨架——它们是后续 PR 启用 per-instance 写动作、第二内核
//! 接入的入口，删了就找不回来。下面这条 `allow(dead_code)` 让
//! `cargo check` / `cargo test` 都静默这一类预留 API 的 dead_code 警告；
//! 只关 dead_code，不关 unused_imports / unused_variables——那两类是真
//! 报警（未使用的导入 / 变量通常是上一笔 commit 的失误），不允许用
//! allow 抹掉。
//!
//! 当某个预留 API 在后续 commit 真正被接入时，**移除对应 item 上方的
//! allow 标注**（或整体移除本 allow 然后逐步加 item 级 allow）。
#![allow(
    dead_code,
    reason = "多内核改造预留 API（实例范围命令 / 端口分配 / 适配器骨架）"
)]

// 按功能分模块（2026-09-30）。每个目录一个 mod.rs，子模块声明为 `pub(crate)`，
// 于是外部写 `crate::<组>::<模块>::X`——分目录改的是文件的摆放，不是调用方
// 要记的路径形状。
mod commands;
mod diagnostics;
mod diskusage;
mod harness;
mod kernel;
mod migration;
mod node;
mod notify;
mod openai;
mod pkg;
mod plugins;
mod shell;
mod skills;
mod usage;

use std::sync::Mutex;

use commands::AppState;
use tauri::{Manager, WindowEvent};

/// 锁定一个互斥锁；当另一个线程 panic 时取回内部值。
pub fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// macOS 自检：管理面板窗口必须是「可最小化」的，否则标题栏那盏黄灯是个死按钮。
///
/// 这是「点黄灯没反应」那个 bug 的回归哨兵。根因在 AppKit 的
/// `NSWindowStyleMask`：tao 的 `set_decorations(false)` 会把样式位重算成
/// `Borderless | Resizable`（tao 0.35 `platform_impl/macos/window.rs`），
/// 其中**不含** `Miniaturizable`，而 `miniaturize:` 在窗口不带该位时静默失败
/// ——`isMiniaturized` 永远是 false、不报错，Tauri 的 `minimize()` 也把这次
/// 失败当成功返回 `Ok(())`，前端连 toast 都触发不了。修复办法是让窗口**出生
/// 即无边框**（`tauri.conf.json` 的 `decorations: false`，见那里的注释），
/// 而不是建窗后再改一次装饰：后者那次样式重算会把默认的 `Miniaturizable`
/// 一起抹掉，而且它由 tao 异步排到主线程队列，在 `setup()` 里补设也会被覆盖。
///
/// `is_minimizable()` 读的就是 AppKit 的 `isMiniaturizable`，所以这里能真实
/// 反映黄灯是否可用。只记录、不中断启动：窗口仍可用关闭按钮与 `Cmd+M` 操作，
/// 不值得为一个按钮拒绝启动。走 `spawn_blocking` 是因为该查询要同步回主线程，
/// 直接在调用线程上等待会把它占住。
#[cfg(target_os = "macos")]
fn check_main_window_minimizable(app: &tauri::App) {
    let Some(window) = app.get_webview_window("main") else {
        eprintln!(
            "dsh-xlink: 找不到管理窗口（label: main），无法确认标题栏黄灯是否可用。\
             请重启应用；若问题持续，请用 `npm run dev` 在终端启动，\
             连同上面的完整输出一起反馈。"
        );
        return;
    };
    // 该查询要同步回主线程，直接在调用线程上等待会把它占住。
    tauri::async_runtime::spawn_blocking(move || {
        if window.is_minimizable().unwrap_or(false) {
            return;
        }
        eprintln!(
            "dsh-xlink: 管理窗口不是「可最小化」的，标题栏黄灯点了不会有反应。\
             请改用系统快捷键 Cmd+M；重启应用可重试这次设置。\
             若问题持续，请用 `npm run dev` 在终端启动，连同上面的完整输出一起反馈。"
        );
    });
}

/// 启动期把 Node 运行时缓存焐热（在后台线程里跑一次 `cached_node`）。
///
/// 探测要派生 `node --version`，壳进程里一次派生 ~250ms（fork 要逐区域复制
/// WKWebView 的 ~4900 个 VM 区域，见 lifecycle.rs 的 `process_command` 注释），
/// 冷探测几百毫秒。首条 `get_status` 在面板 webview 加载完成后才会来（晚于
/// setup 数百毫秒到数秒），后台线程大概率已经把缓存填上，这笔钱不再记进第一
/// 条状态刷新——2026-10-01 perf 实测首条 refresh 的 `node` 段 497ms（同一
/// 二进制被探测两遍，另见 detect.rs 的候选去重）。
///
/// 竞态无害：`cached_node` 在探测前先释放锁，与首条轮询撞车只是多一次幂等的
/// 探测（P2-27）。预热线程起不来也不阻断启动——退化成今天的同步探测。
pub(crate) fn warm_node_cache(app: &tauri::AppHandle) {
    let handle = app.clone();
    let spawned = std::thread::Builder::new()
        .name("dsh-node-warm".to_string())
        .spawn(move || {
            let settings = shell::settings::load_for_shell(shell::settings::current_mode());
            let state = handle.state::<commands::AppState>();
            let _ = commands::cached_node(&state, &settings);
        });
    if spawned.is_err() {
        eprintln!("dsh-xlink: Node 缓存预热线程未启动；首次状态刷新将同步探测");
    }
}

/// 应用入口；由 `main.rs` 调用。
pub fn run() {
    // 单实例守卫必须排在**一切**之前：Windows 上点系统通知横幅会让系统再
    // 拉起一个本 exe（未打包应用的默认激活行为）。第二个进程绝不能走到
    // `setup()`——那里会 `reap_orphans` 回收内核，等于用户点一下通知就把
    // 自己正在跑的内核杀了。抢不到唯一实例时，意图转交给在跑的那个就退出。
    if notify::activate::claim_or_handoff() == notify::activate::Startup::HandedOff {
        return;
    }
    // 登录自启标记要在建窗**之前**判定：`setup` 里要据此决定面板是否可见，
    // 而那时窗口已经建好、马上要显示。早判一帧都不会有「闪一下再消失」。
    //
    // 排在单实例守卫之后是有意的：被交接走的第二个进程（用户点了通知横幅
    // 或又双击了一次图标）不是登录项拉起的，它不参与这个判断——它连
    // `setup` 都走不到。
    shell::resident::mark_started_by_autostart(shell::resident::detect_autostart_arg(
        std::env::args(),
    ));
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            plugins::builtin::runtime::configure(app.handle());
            // 激活通道监听：接收「点通知横幅回到工作台」的交接请求。排在
            // setup 最前面，让它与 `claim_or_handoff` 之间只隔一个建窗过程。
            notify::activate::serve(app.handle());

            // 管理面板在 macOS / Windows 上使用本地 Vue 标题栏绘制交通灯和品牌
            // 背景：这两个平台的「无边框」由 `tauri.conf.json` 的 `decorations:
            // false` 在**建窗时**给定，这里刻意不再调 `set_decorations(false)`。
            //
            // 运行时改装饰会顺手关掉 macOS 窗口的「可最小化」样式位，黄灯随即
            // 变成点了没反应的死按钮（`miniaturize:` 静默失败，Tauri 的
            // `minimize()` 还返回 `Ok(())`，前端连提示都弹不出来）；而且这次
            // 样式重算由 tao 异步排到主线程队列，在 `setup()` 里紧接着补设
            // `set_minimizable(true)` 也会被它覆盖（已实测）。声明式配置没有
            // 这个问题：窗口出生就是无边框的，交通灯/标题栏按钮的语义位全部
            // 保留。细节见下面 `check_main_window_minimizable` 的文档注释。
            #[cfg(target_os = "macos")]
            check_main_window_minimizable(app);

            // 副窗吸附跟随：模型用量窗口 + 日志查看器共用同一套 dock /
            // 移动跟随逻辑（详见 `shell::window::attach_dock_listener`）。主窗拖动时
            // 副窗按合帧窗口（~60fps）持续 set_position，保持贴在主窗右侧。
            // 两个监听器都挂在 main 的 on_window_event 上，事件源单一不
            // 会形成两窗互拉的回环；未开的窗口由 `get_webview_window`
            // 短路掉，不影响 setup 流程。
            shell::window::attach_dock_listener(app.handle(), shell::window::USAGE_VIEWER_LABEL);
            shell::window::attach_dock_listener(app.handle(), shell::window::LOG_VIEWER_LABEL);
            shell::window::attach_dock_listener(
                app.handle(),
                shell::window::SUBSCRIPTION_VIEWER_LABEL,
            );

            // 家族命名空间：data_dir 按（注册表里）默认实例的内核族解析到
            // `<xlink_home>/<family>/desktop[-dev]/`，并把 v0.2.x 的平铺
            // 目录（`<xlink_home>/desktop[-dev]/`）一次性搬迁进去。
            let family = crate::shell::instance::default_family();
            let data_dir = kernel::lifecycle::data_dir(&family);
            // 把解析出的 data dir 打到 stderr，让同时运行 `tauri dev` 与
            // 已安装 release 壳的开发者能一眼看出到底是哪一个
            // （release → `~/.dsh-xlink/<family>/desktop/`，debug →
            // `~/.dsh-xlink/<family>/desktop-dev/`）真正拥有这个进程。
            // 这种廉价的保险能避免经典的「我在 dev 壳里改了设置，
            // release 壳却看不到」踩坑。
            eprintln!(
                "dsh-xlink: data_dir = {} (family: {family}, build: {})",
                data_dir.display(),
                if cfg!(debug_assertions) {
                    "dev"
                } else {
                    "release"
                }
            );
            // 必须在任何状态管理之前回收属于本 data dir 的孤儿 dsh web
            // 内核：崩溃 / 被杀的壳会让它的内核以 cwd == data_dir 继续
            // 运行，同一项目目录上两个内核会向同一会话日志追加内容，
            // 导致日志损坏（seq gap）。必须早于 start_kernel，否则
            // start_kernel 会观察到「端口已被占用」，把孤儿当作健康实例。
            kernel::lifecycle::reap_orphans(&data_dir);
            // 实例系统：把现有用户从旧 active.txt + settings 迁过来的状态
            // 灌进实例注册表。旧版壳首次启动或升级到 P8 之后的 dev 跑，
            // 实例系统都是空——不主动跑这一步，顶部 dropdown 会一直显示
            // 「加载中」、PluginsPanel「所有实例」tab 会显示「实例注册表
            // 加载失败」。这里是**唯一**入口：曾经还有一个前端从不调用的同名
            // Tauri 命令，它把 id 写死成 `DEFAULT_INSTANCE_ID`，dev 壳走到它就会
            // 往共享注册表里塞一条名为 `default`、端口取 dev 的 3091 的幽灵记录。
            //
            // 必须**早于** `app.manage(AppState { data_dir, ... })`——后者
            // 会 move 走 data_dir，之后再借用就拿不到了。
            // 注册表按壳模式分文件（`state/instances.json` / `instances-dev.json`）：
            // 两个壳各持一份，跨进程的读-改-写从「同一个文件两把进程内锁」变成
            // 「两个互不相干的文件」，跨壳竞态从根上消失。必须在下面注册默认实例
            // **之前**跑——认领要读本壳那份文件，dev 壳第一次启动时那份还不存在。
            if let Err(error) =
                crate::shell::registry_split::ensure_scoped(crate::shell::settings::current_mode())
            {
                eprintln!("dsh-xlink: 实例注册表分家失败（{error}）；本壳将退回共享的旧文件");
            }
            if let Err(error) = crate::shell::instance::ensure_default_registered(&data_dir) {
                eprintln!("dsh-xlink: 默认实例注册失败（{error}）");
            }
            // 回收上一次预检崩溃留下的沙盒实例目录。这些目录对用户没有任何
            // 价值，却会让实例目录越攒越多，也会让人以为系统里真有一个叫
            // 「沙盒预检」的实例。放在实例注册之后，保证随后 `load_registry`
            // 看到的是干净状态。族列表取自适配器注册表——接入新内核只需在
            // `adapters()` 里追加实现，这里自动跟上。
            for adapter in crate::kernel::kernel_adapter::adapters() {
                let family = adapter.family();
                let removed = crate::plugins::sandbox::sweep_stale(family);
                if removed > 0 {
                    eprintln!("dsh-xlink: 回收了 {removed} 个残留的预检沙盒目录（{family}）");
                }
            }
            // 内核 home 一次性搬迁：多内核改造前内核一直以默认 `~/.dsh` 运行
            // （旧启动路径从不注入 `DSH_HOME`），会话 / 凭据 / profile 都在
            // 那里；改造后内核经 `DSH_HOME` 指向实例目录，不搬迁等于让用户
            // 面对一个空工作台。失败不阻塞启动：数据原封留在 `~/.dsh`，下次
            // 启动自动重试（逐项并入，可安全续跑）。
            // 历史数据只搬进 release 实例（`legacy_migration_target`）：dev 壳与
            // release 壳各有各的默认实例，谁先跑谁搬走的话 release 的历史就丢了。
            let (legacy_family, legacy_instance) =
                crate::shell::instance::legacy_migration_target();
            if let Err(error) = crate::shell::instance::migrate_legacy_dsh_home_if_needed(
                legacy_family,
                legacy_instance,
                &crate::shell::paths::dirs_home().join(".dsh"),
            ) {
                eprintln!(
                    "dsh-xlink: 内核数据搬迁 ~/.dsh 未完成（{error}）；\
                     遗留数据原样保留，修复后重启桌面端会自动重试"
                );
            }
            app.manage(AppState {
                data_dir,
                running: Mutex::new(None),
                lifecycle: Mutex::new(()),
                node_cache: Mutex::new(None),
                harness_url: Mutex::new(None),
                harness_page: Mutex::new(harness::harness_window::HarnessPage::default()),
            });
            // 历史数据迁移（plugin 中央库搬迁 / skill store 整合）不再在
            // setup() 里自动跑——主窗口 mount 后由 [`migration_prompt`]
            // 检测到遗留数据时通过 [`migration_run`] / [`migration_skip_set`]
            // 让用户主动决定。setup 期只做孤儿内核回收（`reap_orphans`）
            // 与日志目录的轻量修复；plugin store / skill store 的恢复
            // 推迟到用户授权之后跑。

            // 日志目录的启动期修复：把旧命名（`X.log.1`，扩展名是 `1`）的
            // 轮转备份改名为 `X.1.log`，它们此前永远不出现在日志面板里。
            // 纯改名、失败即跳过，不影响启动。
            let logs_dir = kernel::lifecycle::logs_dir(&app.state::<AppState>().data_dir);
            crate::shell::process::migrate_legacy_rotated_logs(&logs_dir);
            // 过期日志清理：每个「日期 × kind」三代 × 8 MiB 且日期只增不减，
            // 不裁剪会长期累积到 GB 级（P2-62）。
            let removed = crate::shell::process::prune_old_logs(&logs_dir);
            if removed > 0 {
                eprintln!("dsh-xlink: 已清理 {removed} 个过期日志文件（保留 30 天 / 200 MiB）");
            }
            // 诊断记录的孤儿详情清扫。`prune` 只清「被 20 条上限挤掉」的
            // 那些；详情文件先落地、索引后写入，壳在两者之间被强杀就会留下
            // 索引里查不到的 `run-*.json`——列表读索引看不到它们，它们却带着
            // 整份事件流一直占盘。按本壳的默认实例扫一次，与 prune_old_logs
            // 同一个位置：都属于「不需要用户授权的清理」。写失败不阻塞启动。
            let (diag_family, diag_instance) = crate::shell::instance::resolve_default();
            let orphaned = crate::diagnostics::run::sweep_orphans(diag_family, diag_instance);
            if orphaned > 0 {
                eprintln!("dsh-xlink: 已清理 {orphaned} 个索引里已不存在的诊断详情文件");
            }
            pkg::updater::spawn_background_check(app.handle());
            // Node 运行时缓存预热：探测要派生 `node --version`，壳进程里
            // 一次派生 ~250ms（fork 逐区域复制 WKWebView 的 ~4900 个 VM
            // 区域），冷探测几百毫秒。放到后台线程与 WebView 加载并行——
            // 首条 `get_status` 在面板加载完成后才会来（晚于 setup 数百
            // 毫秒到数秒），大概率已经命中热缓存，这笔钱不再记进第一条
            // 状态刷新（2026-10-01 perf 实测首条 refresh 的 node 段 497ms，
            // 修复后应归零）。竞态无害：cached_node 在探测前释放锁，
            // 撞车只是多一次幂等探测（P2-27）。
            warm_node_cache(app.handle());
            // 建立常驻入口图标：Windows 是通知区域托盘，macOS 是菜单栏。
            // 2026-10-02 起两平台语义统一——关闭与最小化都只是收起窗口，
            // 程序继续在后台运行，重开与退出都在图标菜单里，所以它必须在
            // 任何窗口可能被收起之前就绪。失败不阻断启动：没有图标时窗口
            // 仍可正常使用，只是「收起后只能靠重新启动找回」这一退化行为，
            // 代价写进日志供排查。
            if let Err(error) = shell::resident::setup(app.handle()) {
                eprintln!(
                    "dsh-xlink: 无法建立常驻入口图标（{error}）；\
                     关闭按钮仍会把窗口收进后台，但届时只能通过重新启动应用找回界面。\
                     若界面显示异常，重启应用重试；仍失败请用 `npm run dev` 在终端启动，\
                     连同上面的完整输出一起反馈。"
                );
            }
            // 登录自启拉起时不显示面板：开机弹一个窗口挡在用户面前，正是
            // 自动启动最招人烦的地方。必须在**任何窗口可见之前**判定并收起，
            // 否则用户会看到面板闪一下再消失。图标已经建好（上面），所以
            // 收起之后程序仍在后台，菜单栏 / 托盘图标是全部可见痕迹。
            //
            // 收起必须走 [`shell::resident::hide_to_shell`] 这一份实现，不能
            // 裸调 `window.hide()`：那份实现还会在 macOS 上把激活等级降到
            // Accessory（Windows 上补 skip-taskbar）。只 hide 的话进程带着
            // Regular 等级、零可见窗口地留在 Dock 里——图标上挂着「在运行」
            // 的小点，点它又什么都不会发生（`RunEvent::Reopen` 原先只抬工作
            // 台）。2026-10-05 重启实测：登录项拉起后 `lsappinfo` 报该进程
            // `type="Foreground"`，Dock 常驻图标 + 无窗口，正是「Dock 有启动
            // 状态、看不到主界面」报告的直接成因。接线由 check:invariants
            // 第 17 项钉住。
            if shell::resident::started_by_autostart() {
                shell::resident::hide_to_shell(app.handle());
                eprintln!(
                    "dsh-xlink: 由系统登录项拉起，管理面板已收进后台；\
                     点菜单栏 / 托盘图标可打开界面。"
                );
                // 用户显式开了「登录后自动启动工作台」才顺带起内核（默认关）。
                // 判定与失败处理都在 `should_launch_kernel_on_autostart` /
                // `start_kernel_blocking` 里，这里只负责发起。
                //
                // **必须派后台线程**：启动要派生 pnpm / node / 内核进程并阻塞
                // 等端口就绪，最坏几分钟（受防护的重试会重装依赖）。而
                // `setup` 跑在 Tauri 主线程上、后面还跟着事件循环启动——
                // 在这儿等几分钟等于开机后几分钟界面无响应。
                let autostart_handle = app.handle().clone();
                let spawned = std::thread::Builder::new()
                    .name("dsh-autostart-kernel".to_string())
                    .spawn(move || {
                        let data_dir = autostart_handle
                            .try_state::<AppState>()
                            .map(|state| state.data_dir.clone());
                        let Some(data_dir) = data_dir else {
                            return;
                        };
                        // 版本没装时不起（`None` → 不做），而不是起了再失败：
                        // 开机那一刻用户不在跟前，失败日志没人看得见。
                        let active = kernel::lifecycle::read_active(&data_dir);
                        if !shell::autostart::should_launch_kernel_on_autostart(active.as_deref()) {
                            eprintln!("dsh-xlink: 登录自启未拉起工作台（开关关闭或内核未安装）");
                            return;
                        }
                        let mut log = |message: &str| eprintln!("dsh-xlink: [自启] {message}");
                        match commands::start_kernel_blocking(
                            &autostart_handle,
                            &data_dir,
                            &mut log,
                        ) {
                            Ok(report) if report.running => {
                                eprintln!("dsh-xlink: [自启] 工作台已在后台启动");
                            }
                            Ok(report) => {
                                eprintln!(
                                    "dsh-xlink: [自启] 工作台未能启动：{}",
                                    report
                                        .warning
                                        .clone()
                                        .unwrap_or_else(|| "未知原因".to_string())
                                );
                            }
                            Err(error) => {
                                eprintln!("dsh-xlink: [自启] 启动工作台失败：{error}");
                            }
                        }
                    });
                if spawned.is_err() {
                    eprintln!("dsh-xlink: 登录自启线程未启动；工作台需要手动开启");
                }
            }
            // 在 debug 构建中自动打开管理窗口的 DevTools。
            // Tauri 的 webview 快捷键（`Cmd+Option+I`、`Cmd+Shift+I`、
            // F12）在 macOS 上不一定能触达 WKWebView，因此调试入口
            // 必须从嵌入端主动打开。`setup` 在已配置窗口创建之后
            // 才触发，所以这里的 `main` webview 已经可取；
            // `#[cfg(debug_assertions)]` 门控让 release 构建
            // （其本身就 `with_devtools(false)`）免于这次调用。
            #[cfg(debug_assertions)]
            if let Some(window) = app.get_webview_window("main") {
                window.open_devtools();
            }
            Ok(())
        })
        .on_page_load(|webview, payload| {
            if payload.event() == tauri::webview::PageLoadEvent::Started {
                shell::appearance::initialize(webview.window());
            }
        })
        .invoke_handler(tauri::generate_handler![
            shell::appearance::set_window_appearance,
            commands::get_status,
            commands::detect_node,
            commands::install_node,
            commands::save_settings,
            commands::list_log_files,
            commands::read_log_file,
            commands::open_data_dir,
            commands::check_shell_update,
            commands::install_shell_update,
            commands::confirm_shell_ready,
            commands::fetch_releases,
            commands::install_kernel,
            commands::activate_version,
            commands::remove_version,
            commands::start_kernel,
            commands::stop_kernel,
            commands::open_harness,
            harness::harness_cmd::harness_force_reload,
            harness::harness_cmd::harness_reload_backoff,
            harness::harness_cmd::stash_harness_draft,
            harness::harness_cmd::take_harness_draft,
            harness::harness_cmd::clear_harness_draft,
            commands::report_harness_fault,
            commands::open_log_window,
            commands::open_official_chat,
            commands::close_official_chat,
            commands::official_chat_tabs,
            commands::switch_official_chat_tab,
            commands::focus_main_shell,
            commands::minimize_shell,
            // 磁盘占用报表（只读，无删除入口——见 diskusage.rs 的模块文档）。
            diskusage::disk_usage,
            // 后台常驻与登录自启（2026-10-02）。三条命令都在 shell::autostart
            // 里，不进 commands.rs——那里只剩调度与端口 / 插件那几族。
            shell::autostart::autostart_status,
            shell::autostart::autostart_set,
            shell::autostart::autostart_set_kernel,
            commands::plugin_status,
            commands::plugin_install,
            plugins::precheck_cmd::plugin_precheck_install,
            // 内嵌 openai-oauth 插件（只读状态 + 启停开关）。
            plugins::builtin::cmd::builtin_openai_status,
            plugins::builtin::cmd::builtin_openai_set_enabled,
            // 内嵌 openai-oauth 的授权命令面（P2）。
            openai::cmd::openai_account_status,
            openai::cmd::openai_authorize_start,
            openai::cmd::openai_authorize_cancel,
            openai::cmd::openai_logout,
            openai::cmd::openai_catalog_refresh,
            commands::plugin_set_precheck,
            plugins::precheck_cmd::plugin_precheck_apply,
            diagnostics::snapshot_cmd::snapshot_list,
            diagnostics::bisect_cmd::bisect_view,
            diagnostics::bisect_cmd::bisect_start,
            diagnostics::bisect_cmd::bisect_probe,
            diagnostics::bisect_cmd::bisect_abort,
            diagnostics::run_cmd::diagnostic_run_list,
            diagnostics::run_cmd::diagnostic_run_get,
            diagnostics::run_cmd::diagnostic_run_latest,
            diagnostics::run_cmd::diagnostic_run_clear,
            diagnostics::snapshot_cmd::snapshot_preview_restore,
            diagnostics::snapshot_cmd::snapshot_restore,
            commands::plugin_update,
            commands::plugin_uninstall,
            commands::plugin_sync,
            commands::plugin_set_mode,
            commands::plugin_check_updates,
            // 插件目录检索：关键词 / 分类 / 排序由面板作为一个整体发来。
            // 不进 commands.rs——它和这三个控件的筛选规则同生共死，见
            // plugins/catalog.rs 的模块文档。
            plugins::catalog::plugin_catalog_search,
            commands::plugin_resolve,
            commands::patch_status,
            commands::patch_apply,
            commands::patch_revert,
            commands::skill_status,
            commands::skill_install,
            commands::skill_update,
            commands::skill_uninstall,
            commands::skill_set_enabled,
            commands::skill_move_aside_shadowed,
            skills::skill_conflict::skill_move_aside_conflicts,
            commands::skill_check_updates,
            commands::notification_status,
            commands::notification_mark_read,
            commands::notification_save_settings,
            commands::notification_test,
            commands::notification_test_sound,
            commands::confirm_close_shell,
            // P2：实例管理命令。
            commands::list_instances,
            commands::create_instance,
            commands::delete_instance,
            commands::set_default_instance,
            commands::start_instance,
            commands::stop_instance,
            commands::restart_instance,
            // P6：迁移向导命令。
            commands::migration_preview,
            commands::migration_run,
            commands::migration_rollback,
            commands::migration_list,
            migration::home_recovery_cmd::scan_misplaced_home,
            migration::home_recovery_cmd::recover_misplaced_home,
            commands::migration_skip_get,
            commands::migration_skip_set,
            commands::migration_skip_clear,
            // 模型用量统计：扫描与聚合都住在 usage.rs，不经 commands.rs。
            usage::local::get_model_usage,
            usage::local::open_usage_window,
            // 云端套餐用量：查询、缓存与建窗都住在 subscription.rs。
            usage::subscription::get_subscription_usage,
            usage::subscription::open_subscription_window,
        ])
        .build(tauri::generate_context!())
        .unwrap_or_else(|error| {
            // 走到这里说明 Tauri 连应用实例都没建起来（窗口/插件/运行时初始化
            // 失败）。`generate_context!` 已在编译期把配置与前端资源清单烘焙进来，
            // 所以运行期失败基本是环境问题而非代码缺陷 —— 给出可操作的下一步，
            // 而不是把英文 panic 甩给用户。
            eprintln!(
                "dsh-xlink: 启动失败，无法创建应用窗口：{error}\n\
                 请先确认管理面板产物存在（在项目根目录执行 `npm run build:ui` 生成 ui/dist），\
                 然后重新启动应用；\n\
                 若仍失败，用 `npm run dev` 在终端启动可看到完整输出，请连同上面的信息一起反馈。"
            );
            std::process::exit(1);
        });

    // 管理窗口的关闭请求**一律**只把窗口收进后台，内核与工作台继续运行；
    // 真正退出只由常驻入口图标菜单里的「退出 dsh-xlink」发起。
    //
    // 2026-10-02 之前这里按平台分叉：Windows 收进托盘，macOS 在内核运行
    // 时弹「完全退出？」。同一份产品两种心智模型，用户换平台只能靠试。现在
    // 两端统一，实现在 `shell::resident`（托盘 / 菜单栏只提供图标与菜单，
    // 行为这一层只有一份）。
    //
    // 「内核在跑要不要问一句」这个诉求没有丢，只是挪了位置：托盘 / 菜单栏的
    // 「退出」走 `resident::request_quit`，它把窗口叫回前台并广播
    // `request-quit-confirm`，前端照旧弹确认、再依次 `stop_kernel` 与
    // `confirm_close_shell`。问的是**真要结束时**，而不是每次收起。
    //
    // 提示路径不能依赖 `RunEvent::Exit` 来拆窗口：`confirm_close_shell`
    // 会销毁主窗口，但事件循环只在最后一个窗口消失时才结束（macOS 上
    // 即便如此也不结束——需要显式 exit），所以一个仍打开的 `official-chat`
    // 窗口会让循环（以及 app）继续存活，却无人关闭它。因此下面的 Exit
    // 分支只是绕过提示的那些退出（Cmd+Q、操作系统关机、无需警告时最后
    // 窗口的自动关闭）的回退路径。
    app.run(|handle, event| {
        if let tauri::RunEvent::WindowEvent {
            label,
            event: WindowEvent::CloseRequested { api, .. },
            ..
        } = &event
        {
            // 主窗口：收进后台而不是退出。官方对话等副窗不在此列——它由
            // 面板驱动（面板退出时一并关闭），单独点它的 X 属于真关闭。
            if shell::resident::intercept_close(handle, label, api) {
                return;
            }
        }
        // 工作台窗口的前台状态：用户切回工作台即视为"看过结果了"，未读角标
        // 清零。只认 `harness`——用户盯着管理面板时并不算看到了对话结果，
        // 那种情况下仍然应该收到通知（见 `notify::task::set_workbench_focused`）。
        if let tauri::RunEvent::WindowEvent {
            label,
            event: WindowEvent::Focused(focused),
            ..
        } = &event
        {
            if label == "harness" {
                notify::task::set_workbench_focused(handle, *focused);
            }
        }
        // 窗口被销毁（用户点系统的关闭按钮、或 `stop_kernel` 的 `destroy()`）
        // 时必须显式清掉前台标记：销毁不一定伴随 `Focused(false)`，而标记一旦
        // 卡在 true，之后所有"离开时完成"的任务都会被当成"用户正在看"而永不
        // 提醒——通知功能就此静默失效。
        if let tauri::RunEvent::WindowEvent {
            label,
            event: WindowEvent::Destroyed,
            ..
        } = &event
        {
            if label == "harness" {
                notify::task::set_workbench_focused(handle, false);
            }
        }
        // 显示缩放变化（窗口被拖到另一块 DPI 不同的屏幕，或系统缩放被改）后按新的
        // `SM_CXSMICON` 重选托盘帧。托盘图标是 shell 侧的一张 HICON 快照，系统不会
        // 替我们按新 DPI 重取：不重设的话，125%/150% 缩放下会一直用 100% 那一档
        // 被 shell 拉大（发虚），或反过来被缩小（丢掉 16px 帧的清晰度）。
        #[cfg(target_os = "windows")]
        if let tauri::RunEvent::WindowEvent {
            label,
            event: WindowEvent::ScaleFactorChanged { .. },
            ..
        } = &event
        {
            if label == shell::tray::MAIN_WINDOW {
                shell::tray::refresh_icon(handle);
            }
        }
        // macOS：应用被重新打开（点 Dock 图标、点通知横幅）。工作台开着就把它
        // 抬到台前——通知横幅点一下就该看到任务结果，而不是停在管理面板上。
        // 工作台没开时**必须自己把管理面板叫回来**：macOS 的激活只把进程带到
        // 前台，不会替我们显示 `hide()` 掉的窗口；缺了这步兜底，Dock 上那枚
        // 「在运行」的图标点了没反应（2026-10-05 重启实测）。接线由
        // check:invariants 第 17 项钉住。
        #[cfg(target_os = "macos")]
        if let tauri::RunEvent::Reopen { .. } = &event {
            if !notify::activate::raise_workbench_if_open(handle) {
                shell::resident::show_main_shell(handle);
            }
        }
        if let tauri::RunEvent::Exit = event {
            // 回收内核，使 app 退出后不会留下仍在服务的 dsh web 进程。
            // 内存中的 child 覆盖本会话启动的内核；pid 文件覆盖上一次壳
            // 运行（例如崩溃后）留下的孤儿，由 `kill_pid` 的内核检查把关。
            //
            // **「后台常驻」不改变这一段**：常驻指的是关窗之后内核继续跑
            // （2026-10-02 拍板，内核随壳一起停），所以退出时该收的照样收。
            // 换句话说这里不需要跨进程认领协议，也就不会出现「壳退出了
            // 端口还被谁占着」「下次启动认不认得这个孤儿」那类问题。
            //
            // 在绕过退出提示的那些退出路径（macOS 上的 Cmd+Q、操作系统
            // 关机、无需警告时的最后窗口自动关闭）上级联关闭 official-chat
            // 这个由面板驱动的窗口。确认退出路径已经在退出循环前通过
            // `confirm_close_shell` 销毁了所有窗口——这里的 Exit 只是
            // 回退兜底，不是主要的拆窗路径。理由与上面的 WindowEvent
            // 处理相同：official-chat 是一个由面板驱动的临时窗口，
            // 关闭面板就意味着关闭它。
            if let Some(oc) = handle.get_window("official-chat") {
                let _ = oc.destroy();
            }
            if let Some(state) = handle.try_state::<AppState>() {
                {
                    let mut guard = lock(&state.running);
                    if let Some(mut child) = guard.take() {
                        let _ = kernel::lifecycle::stop(&mut child);
                    }
                }
                let data_dir = state.data_dir.clone();
                let current = shell::settings::load_for_shell(shell::settings::current_mode());
                // 判据同样不看配置端口：内核可能绑在用户改端口之前的那个
                // 端口上，用当前配置端口探测会漏掉它，让它继续占着端口活到
                // 下一次启动。
                if let Some(pid) = kernel::lifecycle::workbench_pid(&data_dir, &current) {
                    // 同 stop_kernel：带上记录里的启动端口（P2-1）。
                    kernel::lifecycle::kill_pid(
                        pid,
                        kernel::lifecycle::recorded_kernel_port(&data_dir),
                    );
                }
                kernel::lifecycle::clear_pid(&data_dir);
            }
            // 事件订阅线程是壳自己的：退出前显式停掉并等在途的重连/读循环
            // 收尾，避免进程退出时留下一个还在往已销毁的 AppHandle 上发事件
            // 的线程。
            notify::task::stop_watcher();
        }
    });
}

/// 把管理面板主窗口恢复到前台（`show` + 取消最小化 + 前台聚焦）。
///
/// 把管理面板主窗口恢复到前台（`show` + 取消最小化 + 前台聚焦）。
///
/// 2026-10-02 起实现**只有一份**，在 [`shell::resident::show_main_shell`]：
/// 常驻语义既然已经两平台统一，这里就没有可分发的差异了。曾经这个函数按
/// 平台各写一份，而 `shell::tray::show_main_shell` 又回调它——Windows 上点
/// 托盘图标直接 `thread 'main' has overflowed its stack`（首版即如此）。
/// **不要把动作挪回本函数**：托盘 / 菜单栏图标、工作台拉绳
/// （`focus_main_shell`）与系统 Dock 激活都调它，抄一份就会漂。
///
/// Windows 上必须做一次 always-on-top 往返：焦点经 IPC 到达时
/// `SetForegroundWindow` 会被系统静默忽略，先置顶再解除才能让窗口真正浮到
/// 最前；其它平台是无害的 no-op。
pub(crate) fn show_main_shell(handle: &tauri::AppHandle) {
    shell::resident::show_main_shell(handle);
}

/// 内核当前是否在对外服务。
///
/// 内存中的句柄只有在内核**确实还活着**时才算数：内核自行退出或被外部杀掉
/// 之后句柄仍留在槽位里，只看 `is_some()` 会让这里一直撒谎（关窗时弹出一个
/// 与实际状态矛盾的确认为）。句柄失效后回落到 [`kernel::lifecycle::workbench_running`]，
/// 它同样不以配置端口为判据——内核可能绑在用户改端口之前的那个端口上。
///
/// 对 crate 内公开：托盘菜单的「退出」要用它判断是否需要先弹确认。
pub(crate) fn kernel_running(handle: &tauri::AppHandle) -> bool {
    let Some(state) = handle.try_state::<AppState>() else {
        return false;
    };
    {
        let mut guard = lock(&state.running);
        let alive = guard
            .as_mut()
            .map(|child| child.try_wait().is_ok_and(|status| status.is_none()))
            .unwrap_or(false);
        if alive {
            return true;
        }
        if guard.is_some() {
            // 句柄还在但进程已经退出（或查询失败）：清掉僵尸句柄，
            // 避免它继续影响状态判断。
            *guard = None;
        }
    }
    kernel::lifecycle::workbench_running(
        &state.data_dir,
        &shell::settings::load_for_shell(shell::settings::current_mode()),
    )
}

/// 测试专用 RAII：把 `DSH_XLINK_HOME` 临时指向给定路径，离开作用域时还原。
/// 进程内串行化（`OnceLock<Mutex>`），防止并行测试相互覆盖 env。
///
/// 仅在 `cfg(test)` 下编译；生产代码看不到，避免误把测试逻辑拖进发布版。
#[cfg(test)]
pub(crate) mod tests {
    use std::path::Path;
    use std::sync::{Mutex, MutexGuard, OnceLock};

    /// Xlink / DSH 各持一把进程级锁——两把 env 互不踩，同一进程内可同时
    /// 持有一对 `EnvGuard`（如迁移向导测试需要既 mock `DSH_XLINK_HOME`
    /// 又 mock `DSH_HOME`）。
    static XLINK_ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    static DSH_ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    /// 持有进程级互斥锁 + 旧 env 值；drop 时按 RAII 释放锁并还原 env。
    /// 调用方只需 `let _guard = scoped_xlink_home(&root);`。
    ///
    /// `var` 决定改 `DSH_XLINK_HOME`（Xlink 路径解析）还是 `DSH_HOME`
    ///（旧布局 + kernel 解析），也决定用哪把锁。
    pub(crate) struct EnvGuard {
        _lock: MutexGuard<'static, ()>,
        var: EnvVar,
        previous: Option<std::ffi::OsString>,
    }

    enum EnvVar {
        XlinkHome,
        DshHome,
    }

    impl EnvVar {
        fn name(&self) -> &'static str {
            match self {
                EnvVar::XlinkHome => crate::shell::paths::DSH_XLINK_HOME_ENV,
                EnvVar::DshHome => "DSH_HOME",
            }
        }
        fn lock(&self) -> &'static Mutex<()> {
            match self {
                EnvVar::XlinkHome => XLINK_ENV_LOCK.get_or_init(|| Mutex::new(())),
                EnvVar::DshHome => DSH_ENV_LOCK.get_or_init(|| Mutex::new(())),
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var(self.var.name(), value),
                None => std::env::remove_var(self.var.name()),
            }
        }
    }

    fn acquire_env_guard(home: &Path, var: EnvVar) -> EnvGuard {
        let lock = var
            .lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous = std::env::var_os(var.name());
        std::env::set_var(var.name(), home);
        EnvGuard {
            _lock: lock,
            var,
            previous,
        }
    }

    /// 进入作用域时拿锁、把 `DSH_XLINK_HOME` 指向 `home`；drop 时还原 env
    /// 并释放锁。返回 [`EnvGuard`] 是 RAII 写法。
    pub(crate) fn scoped_xlink_home(home: &Path) -> EnvGuard {
        acquire_env_guard(home, EnvVar::XlinkHome)
    }

    /// 进入作用域时拿锁、**移除** `DSH_XLINK_HOME`；drop 时还原 env 并释放
    /// 锁。测「缺省解析到 `~/.dsh-xlink`」这类默认路径时必须走这里，不能
    /// 裸调 `shell::env::remove_var`——后者只在本地锁里串行，拦不住别的模块正持
    /// 着 [`scoped_xlink_home`] 往临时目录写文件，env 一被摘掉那些写入就
    /// 全部落到用户真实的 `~/.dsh-xlink`（夹具泄漏进真实数据的根因）。
    pub(crate) fn scoped_xlink_home_unset() -> EnvGuard {
        let lock = EnvVar::XlinkHome
            .lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous = std::env::var_os(EnvVar::XlinkHome.name());
        std::env::remove_var(EnvVar::XlinkHome.name());
        EnvGuard {
            _lock: lock,
            var: EnvVar::XlinkHome,
            previous,
        }
    }

    /// 进入作用域时拿锁、把 `DSH_HOME` 指向 `home`；drop 时还原 env
    /// 并释放锁。迁移向导的测试需要它——旧布局的路径解析由 `DSH_HOME`
    /// 决定，与 `DSH_XLINK_HOME` 正交。
    pub(crate) fn scoped_dsh_home(home: &Path) -> EnvGuard {
        acquire_env_guard(home, EnvVar::DshHome)
    }
}
