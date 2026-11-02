//! 启动时的内核热文件预热与工作台后台预启动（B1 + B2）。
//!
//! ## 为什么
//!
//! 2026-10-27 真机启动：壳 07:39:39 起来 → 内核 07:39:45 派生 → 端口就绪
//! 07:40:37，冷启动 52.1s、全程约 61s。隔离复现（同一份 home + 内核树）：
//! 冷跑 54.1s wall / 10.4s CPU / 7,682 reads / 124–138k metadata ops，紧接着
//! 热跑 10.9s（CPU 反而 11.9s），**两次 IO 计数完全一致**——代码与 CPU 都没
//! 变慢，多出来的 43 秒是杀毒软件对**首次打开的文件**的扫描延迟。
//!
//! 2026-11-02 本机复测（360安全卫士 + Defender 实时保护同时在跑）把代价的
//! 落点钉死了：
//!
//! - 冷文件 open+close **7.7ms**，同一个文件再 open **0.07ms**；**stat 冷态
//!   也只有 0.07ms**。⇒ 扫描代价付在 **open** 上，不在 read 上，也几乎不在
//!   stat 上。**只 stat 等于没付钱**——上一版预热在这台机器上一分钱没省。
//! - 扫描通道是**共享吞吐**：单线程 7.7ms/文件，8 线程 2.1ms/文件（16 线程
//!   不再变快）；但旁边有别的线程在开冷文件时，本线程从 3.1ms/文件劣化到
//!   13.1ms（最坏一次 1898ms）。⇒ 预热与内核启动**不能同时跑**，否则内核
//!   比不做预热还惨。
//!
//! 分摊的形状因此是：**先把内核这次真会加载的文件 open 一遍（8 线程并行、
//! 有死线），再起内核**。内核随后开同一批文件就是热态（0.07ms）：内核自己
//! 冷启动要开约 7,700 个文件，串行 7.7ms 就是约 59s，预热全量约 32s，
//! 合计 60s → 约 39s。
//!
//! ## 预热哪些文件
//!
//! 不是整棵内核树（41,092 个文件 / 594MB，扫完要 5 分钟），而是**内核这次
//! 真会加载的那部分**：
//!
//! - profile 清单（`<实例>/home/profiles/<profile>/package.json`）里的
//!   `dsh.profile.bundles`；
//! - 加上 CLI 本体包 `@deepseek-ai/dsh`；
//! - 从这些包出发，按各自 `package.json` 的 `dependencies` 在内核安装树的
//!   `node_modules/`（pnpm 扁平布局，包名直接对应 `node_modules/<name>`）里
//!   递归走闭包；
//! - 再加上实例 profile 自己的 `node_modules`（内核从那儿解析 profile 的依赖）。
//!
//! 实测（dsh 0.2.2-alpha.2 + web profile）：内核树闭包 **550 个包**，加上
//! profile 侧共 **约 1.3 万个 .js/.json 文件、约 130MB**。只取
//! `.js / .mjs / .cjs / .json`——Node 的 ESM 加载器实际会读的那类。
//!
//! ## 怎么分摊（用户拍板的方向）
//!
//! - **B1 预热内核热文件**：只 **open** 不读内容（扫描代价在 open 上，读内容
//!   只是多花带宽），8 线程并行、上限 [`PREWARM_DEADLINE`]（40s）。撞线就带着
//!   「已预热 N 个文件」照常起内核——它只能缩短等待，不能把后台就绪往后拖太久。
//!   顺序按 profile 的 bundle 顺序 + 依赖深度优先，尽量贴近内核自己的加载
//!   顺序：撞线时先热过的那批正是内核最先要读的那批。
//! - **B2 后台预启动**：管理面板画出第一帧之后（延迟 [`PRESTART_DELAY`]）
//!   按设置决定要不要顺手把工作台拉起来。默认**开启**：用户点「启动工作台」
//!   时内核通常已经在跑，交互从「等一分钟」变成「已经好了」。
//!
//! 判定集中在 [`should_prestart`]：设置开关、登录自启（那条路径自己决定起
//! 不起内核，不重复起）、已安装版本（没装就什么都不做）、端口占用（已经在
//! 跑就不重复起）。任一条不满足都静默返回。
//!
//! ## 失败处理
//!
//! 后台动作没有人在跟前看进度，所以它**绝不弹事故、不弹 toast**：失败只记
//! [`shell_events::record`]（落进「查看日志」），进程照常。用户随后手动点
//! 「启动工作台」会走同一条 `start_kernel_blocking`，由它给出正常的错误面。
//!
//! 壳开始退出时**不再派生内核**：退出标记与派生闸门统一在
//! [`crate::shell::shutdown`]，退出清理跑完之后由后台线程补起一个内核会
//! 留下占端口的孤儿。预热本身也让位：它在退出流程开始后立刻收工。
//!
//! ## 与登录自启的关系
//!
//! 登录自启拉起时**不预启动**：那条路径有自己的 `autostart_kernel` 开关与
//! `dsh-autostart-kernel` 线程（见 `lib.rs` 的 setup），这里再起一次就是
//! 两个线程抢同一个启动动作。两处都经过 `start_kernel_blocking` 的 lifecycle
//! 锁，但「一个动作只有一个发起方」比「靠锁串行」更容易讲清楚。

use std::collections::HashSet;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use tauri::Manager;

use crate::commands::AppState;
use crate::kernel::lifecycle;
use crate::shell::{instance, paths, resident, settings, shell_events};

/// 等面板先画出第一帧再动手：用户双击图标后要先看到界面，而不是窗口还没
/// 画出来、背后已经有一堆 node 进程在起。2s 足够面板加载骨架（实测窗口
/// 可见 0.14–0.27s，首条状态轮询紧随其后）。
const PRESTART_DELAY: Duration = Duration::from_secs(2);

/// 预热死线。闭包全量约 1.3 万个文件，8 线程实测 2.1ms/文件 ≈ 32s；取 40s
/// 留出余量（文件清单计算 + 慢盘），又不至于让「用户点开时已经好了」被拖成
/// 两分钟。撞线就带着已预热的那部分照常起内核。
const PREWARM_DEADLINE: Duration = Duration::from_secs(40);

/// 并行 open 的线程数。实测 8 线程 2.1ms/文件（单线程 7.7ms），16 线程不再
/// 变快——杀软那条扫描通道的吞吐就这么宽。
const PREWARM_WORKERS: usize = 8;

/// 内核 CLI 本体包。它是内核启动的入口，必须进闭包。
const CLI_PACKAGE: &str = "@deepseek-ai/dsh";

/// 事件日志的逻辑名。落在 `<shell_logs_dir>/<kind>-kernel-prestart-<date>.log`，
/// 自动出现在「查看日志」面板——后台动作没有别的地方可查。
const EVENT_LOG: &str = "kernel-prestart";

/// 一次预热的统计。只用于事件日志与测试，不参与任何启动判定。
#[derive(Debug, Default)]
pub(crate) struct PrewarmStats {
    /// 闭包命中的包数。
    pub(crate) packages: usize,
    /// 成功打开过的文件数。
    pub(crate) files: usize,
    /// 整次预热耗时（含闭包计算）。
    pub(crate) elapsed_ms: u64,
    /// 是否撞上 [`PREWARM_DEADLINE`]（true = 剩下的文件没热到）。
    pub(crate) timed_out: bool,
    /// 是否因为「壳在退出」或「内核已经开始启动」提前收工。
    pub(crate) stopped_early: bool,
}

/// 是否需要后台预启动。四个条件缺一不可：
/// ① 用户没关掉这个开关（默认开，见 [`settings::prestart_kernel_enabled`]）；
/// ② 本次不是登录自启拉起的——那条路径有独立的 `autostart_kernel` 开关和
///    自己的后台线程，同一个启动动作只该有一个发起方；
/// ③ 有已安装且已激活的内核版本——没装时起了也白起，只会失败；
/// ④ 端口空闲——已经在服务的话没有第二份要起。
///
/// 抽成纯函数是为了能逐条测：真实调用点里前三个输入都来自进程外状态
/// （磁盘设置 / 本次启动来源 / data dir），在测试里造「全部满足」的现场
/// 要么污染用户数据、要么需要真起一个内核；2026-10-02 的同类判据就吃过
/// 「只测真实函数、结果被第一个条件短路成永远绿」的亏（见
/// [`crate::shell::autostart::should_launch_kernel_given`] 的注释）。
pub(crate) fn should_prestart(
    enabled: bool,
    launched_by_autostart: bool,
    active_version: Option<&str>,
    port_busy: bool,
) -> bool {
    enabled
        && !launched_by_autostart
        && active_version.is_some_and(|version| !version.trim().is_empty())
        && !port_busy
}

/// 派一个后台线程做预启动，**立刻返回**（`setup()` 不能被阻塞）。
///
/// 线程名固定为 `dsh-kernel-prestart`：任务管理器 / 调试器里一眼能认出
/// 这条线程在干什么。线程起不来只写 stderr、不阻断启动——退化回用户手动
/// 点「启动工作台」的旧路径。
pub(crate) fn spawn(app: &tauri::AppHandle) {
    let handle = app.clone();
    let spawned = std::thread::Builder::new()
        .name("dsh-kernel-prestart".to_string())
        .spawn(move || run(&handle));
    if let Err(error) = spawned {
        eprintln!(
            "dsh-xlink: 后台预启动线程未启动（{error}）；\
             工作台需要手动开启，冷启动耗时不会提前付掉"
        );
    }
}

/// 线程主体：延迟 → 判据 → 预热内核热文件 → 试着把工作台拉起来。
///
/// 预期内的跳过（用户关了开关、登录自启拉起、没装内核、端口已占用）
/// **静默返回**：后台动作没人在跟前，这些不是异常，刷日志只会让「查看日志」
/// 变吵。只有真正做了什么（开始预热、启动成功 / 失败）才落
/// [`shell_events::record`]。
fn run(app: &tauri::AppHandle) {
    std::thread::sleep(PRESTART_DELAY);

    let settings = settings::load_for_shell(settings::current_mode());
    let enabled = settings::prestart_kernel_enabled(&settings);
    let launched_by_autostart = resident::started_by_autostart();
    let Some(data_dir) = app
        .try_state::<AppState>()
        .map(|state| state.data_dir.clone())
    else {
        return;
    };
    let active_version = lifecycle::read_active(&data_dir);
    // 端口探测只在其余条件都满足时才做：macOS 上无监听者的回环端口可能被
    // 防火墙静默丢包、等满 400ms（见 [`lifecycle::port_open`] 的注释），
    // 没必要为一个已经关掉的开关付这笔钱。
    let port_busy = if enabled
        && !launched_by_autostart
        && active_version
            .as_deref()
            .is_some_and(|version| !version.trim().is_empty())
    {
        lifecycle::port_open(settings.port)
    } else {
        false
    };
    if !should_prestart(
        enabled,
        launched_by_autostart,
        active_version.as_deref(),
        port_busy,
    ) {
        return;
    }
    // `should_prestart` 已经保证版本非空，这里只是把 `Option` 收窄成 `&str`。
    let Some(version) = active_version.as_deref() else {
        return;
    };

    // B1：把内核这次真会加载的那批文件 open 一遍（8 线程并行），把杀软的
    // 「首次打开」扫描代价付掉。失败 / 超时只影响这次缓解本身。
    //
    // 预热**排在起内核之前**：杀软那条扫描通道是共享吞吐，预热与内核同时
    // 开冷文件会把内核的 open 从 3.1ms 拖到 13.1ms——比不做预热还惨。
    let kernel_root = lifecycle::kernel_dir(&data_dir, version);
    let (family, instance_id) = instance::resolve_default();
    let profile_dir = paths::instance_profile_dir(family, instance_id, &settings.profile);
    shell_events::record(
        EVENT_LOG,
        &format!(
            "后台预启动：开始预热内核热文件（内核树 {}，profile {}）",
            kernel_root.display(),
            profile_dir.display()
        ),
    );
    let stats = prewarm_tree(app, &kernel_root, &profile_dir);
    let note = if stats.timed_out {
        format!(
            "，已到 {}s 上限，剩余文件未预热",
            PREWARM_DEADLINE.as_secs()
        )
    } else if stats.stopped_early {
        "，提前收工（壳在退出或内核已开始启动）".to_string()
    } else {
        String::new()
    };
    shell_events::record(
        EVENT_LOG,
        &format!(
            "后台预启动：内核热文件预热完成（打开 {} 个文件，命中 {} 个包，耗时 {}ms{}）",
            stats.files, stats.packages, stats.elapsed_ms, note
        ),
    );

    // 壳在这段时间里开始退出（用户在预热期间点了退出）：不再派生内核，
    // 否则退出清理跑完之后起一个占端口的孤儿进程。标记与派生闸门由
    // `shell::shutdown` 统一提供，看护路径与本线程共用同一份真值。
    if crate::shell::shutdown::is_shutting_down() {
        shell_events::record(EVENT_LOG, "后台预启动：壳正在退出，已跳过启动内核");
        return;
    }

    // B2：启动本体。`start_kernel_blocking` 内部会拿 lifecycle 锁，因此与
    // 用户手点「启动工作台」/「关闭工作台」串行：先到的那个先做，后到的
    // 那个走 `guarded_start` 的幂等判据（已在跑就返回 no-op 报告）。
    // 进度消息落 stderr：这条路径没有 Channel 可推，dev 终端里看得到。
    let started = Instant::now();
    let result = crate::commands::start_kernel_blocking(app, &data_dir, &mut |message| {
        eprintln!("dsh-xlink: [后台预启动] {message}");
    });
    let elapsed = started.elapsed();
    match result {
        Ok(report) if report.running => {
            let mut line = format!(
                "后台预启动完成：工作台已在后台运行（端口 {}，耗时 {:.1}s）",
                report.port,
                elapsed.as_secs_f64()
            );
            if let Some(warning) = report.warning {
                line.push_str(&format!("；{warning}"));
            }
            shell_events::record(EVENT_LOG, &line);
        }
        // 看护把「没起来」折进 `Ok(StartReport { running: false, incident })`：
        // 事故的归因与处置归面板，用户手动点一次「启动工作台」就会看到。
        // 后台路径不许把它变成弹窗，只留一行可查的日志。
        Ok(report) => {
            // 壳在启动途中开始退出：这次不是「没起来」，是取消了。不能写成
            // 「请在面板点启动工作台重试」——壳正在关闭，用户没有重试的机会。
            if crate::shell::shutdown::is_shutting_down() {
                shell_events::record(EVENT_LOG, "后台预启动：壳正在退出，已取消本次启动");
                return;
            }
            shell_events::record(
                EVENT_LOG,
                &format!(
                    "后台预启动未完成：工作台没有在服务（端口 {}，耗时 {:.1}s）；\
                     请在面板点「启动工作台」重试",
                    report.port,
                    elapsed.as_secs_f64()
                ),
            );
        }
        Err(error) => {
            shell_events::record(EVENT_LOG, &format!("后台预启动失败：{error}"));
        }
    }
}

/// 预热内核这次真会加载的那批文件（bundle 依赖闭包 + 实例 profile 的依赖）。
///
/// 只 **open** 不读内容：扫描代价付在 open 上（冷态 open+close 7.7ms，之后
/// read 同一文件只要 0.7–1.0ms），读内容只是多花带宽。
///
/// 「该停就停」的两个信号：壳在退出，或**内核已经开始启动**（用户手点
/// 「启动工作台」）——后者必须让位：杀软那条扫描通道是共享吞吐，预热与内核
/// 同时开冷文件会把内核的 open 拖慢 4 倍。判据用 lifecycle 锁：它在
/// `start_kernel_blocking` 整个启动过程中都被占着，而它平时只有用户发起的
/// 装卸 / 启停才会拿。
pub(crate) fn prewarm_tree(
    app: &tauri::AppHandle,
    kernel_root: &Path,
    profile_dir: &Path,
) -> PrewarmStats {
    let state = app.try_state::<AppState>();
    prewarm_tree_with_deadline(kernel_root, profile_dir, PREWARM_DEADLINE, &|| {
        crate::shell::shutdown::is_shutting_down()
            || state
                .as_ref()
                .is_some_and(|state| state.lifecycle.try_lock().is_err())
    })
}

/// [`prewarm_tree`] 的可测参数版：死线与「要不要停」都由调用方给。
fn prewarm_tree_with_deadline(
    kernel_root: &Path,
    profile_dir: &Path,
    deadline: Duration,
    stop: &(dyn Fn() -> bool + Sync),
) -> PrewarmStats {
    let started = Instant::now();
    let mut stats = PrewarmStats::default();
    if started.elapsed() >= deadline {
        stats.timed_out = true;
        return stats;
    }

    // ① 目标包：profile 的 bundle 闭包 + CLI 本体 + profile 自己的依赖。
    let packages = hot_packages(kernel_root, profile_dir);
    stats.packages = packages.len();

    // ② 目标文件：每个包目录里的 .js/.mjs/.cjs/.json。
    let files = hot_files(&packages);

    // ③ 8 线程并行 open。
    let (opened, timed_out, stopped) = open_files(&files, started, deadline, stop);
    stats.files = opened;
    stats.timed_out = timed_out;
    stats.stopped_early = stopped;
    stats.elapsed_ms = started.elapsed().as_millis() as u64;
    stats
}

/// 这次真会被加载的包目录（内核树与 profile 树两处）。
///
/// 顺序 = profile 清单里的 bundle 顺序 + 依赖深度优先（依赖在前）：内核的
/// ESM 加载器也是先加载依赖再执行包体，撞死线时先热过的那批正是内核最先
/// 要读的那批。
fn hot_packages(kernel_root: &Path, profile_dir: &Path) -> Vec<PathBuf> {
    let manifest = read_profile_manifest(profile_dir);
    let mut roots = manifest.bundles;
    roots.push(String::from(CLI_PACKAGE));
    let mut packages = package_closure(&kernel_root.join("node_modules"), &roots);
    packages.extend(package_closure(
        &profile_dir.join("node_modules"),
        &manifest.dependencies,
    ));
    packages
}

/// profile 清单里预热要用的两样东西：`dsh.profile.bundles` 与 `dependencies`。
struct ProfileManifest {
    /// 内核这次要加载的 bundle。
    bundles: Vec<String>,
    /// profile 自己的依赖（内核从 profile 的 `node_modules` 里解析它们）。
    dependencies: Vec<String>,
}

/// 读 profile 清单。读不出来就当没有——实例还没准备好时清单可能还不存在，
/// 那时预热只剩 CLI 本体那一份，仍然值得做。
fn read_profile_manifest(profile_dir: &Path) -> ProfileManifest {
    let empty = ProfileManifest {
        bundles: Vec::new(),
        dependencies: Vec::new(),
    };
    let Ok(text) = fs::read_to_string(profile_dir.join("package.json")) else {
        return empty;
    };
    let Ok(root) = serde_json::from_str::<serde_json::Value>(&text) else {
        return empty;
    };
    let bundles = root
        .get("dsh")
        .and_then(|dsh| dsh.get("profile"))
        .and_then(|profile| profile.get("bundles"))
        .and_then(|bundles| bundles.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let dependencies = root
        .get("dependencies")
        .and_then(|dependencies| dependencies.as_object())
        .map(|map| map.keys().cloned().collect())
        .unwrap_or_default();
    ProfileManifest {
        bundles,
        dependencies,
    }
}

/// 从 `roots` 出发，按各包 `package.json` 的 `dependencies` 递归走闭包。
///
/// `node_modules` 是 pnpm 扁平布局：包名直接对应 `node_modules/<name>`，它
/// 可能是指向 `.pnpm/` 的符号链接——`read_dir` / `read_to_string` 跟随它，
/// 不需要特殊处理。深度优先且**依赖在前**。
fn package_closure(node_modules: &Path, roots: &[String]) -> Vec<PathBuf> {
    let mut visited: HashSet<String> = HashSet::new();
    let mut packages = Vec::new();
    for root in roots {
        visit_package(node_modules, root, &mut visited, &mut packages);
    }
    packages
}

/// 递归收一个包：先收它的依赖（深度优先），再收它自己。
fn visit_package(
    node_modules: &Path,
    name: &str,
    visited: &mut HashSet<String>,
    packages: &mut Vec<PathBuf>,
) {
    if name.is_empty() || !visited.insert(name.to_string()) {
        return;
    }
    let dir = node_modules.join(name);
    for dependency in read_dependencies(&dir) {
        visit_package(node_modules, &dependency, visited, packages);
    }
    // 没装出来的包（optional 依赖没装、版本被钉掉）跳过，不报错。
    if dir.is_dir() {
        packages.push(dir);
    }
}

/// 一个包的 `dependencies` 名单。读不到就当没有依赖。
fn read_dependencies(dir: &Path) -> Vec<String> {
    let Ok(text) = fs::read_to_string(dir.join("package.json")) else {
        return Vec::new();
    };
    let Ok(root) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Vec::new();
    };
    root.get("dependencies")
        .and_then(|dependencies| dependencies.as_object())
        .map(|map| map.keys().cloned().collect())
        .unwrap_or_default()
}

/// 每个包目录里的 `.js / .mjs / .cjs / .json`——Node 的 ESM 加载器实际会读
/// 的那类文件。
///
/// 不进嵌套的 `node_modules`：那里面是 pnpm 版本冲突留下的另一份副本，闭包
/// 已经按包名覆盖过；进去只会把文件数翻倍。
fn hot_files(packages: &[PathBuf]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for package in packages {
        let mut pending = vec![package.clone()];
        while let Some(dir) = pending.pop() {
            let Ok(entries) = fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let Ok(file_type) = entry.file_type() else {
                    continue;
                };
                if file_type.is_dir() {
                    if path.file_name().is_some_and(|name| name == "node_modules") {
                        continue;
                    }
                    pending.push(path);
                    continue;
                }
                if is_hot_file(&path) {
                    files.push(path);
                }
            }
        }
    }
    files
}

/// Node 会真的读的文件类型。
fn is_hot_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("js") | Some("mjs") | Some("cjs") | Some("json")
    )
}

/// 8 线程并行 open：每个文件 open 成功就立刻 drop。
///
/// 返回 `(打开成功的文件数, 是否撞上死线, 是否被叫停)`。任何 IO 错误跳过，
/// 不 panic——预热失败只是没省下时间，不该挡住启动。
fn open_files(
    files: &[PathBuf],
    started: Instant,
    deadline: Duration,
    stop: &(dyn Fn() -> bool + Sync),
) -> (usize, bool, bool) {
    let next = AtomicUsize::new(0);
    let opened = AtomicUsize::new(0);
    let timed_out = AtomicBool::new(false);
    let stopped = AtomicBool::new(false);
    std::thread::scope(|scope| {
        for _ in 0..PREWARM_WORKERS {
            scope.spawn(|| {
                loop {
                    if started.elapsed() >= deadline {
                        timed_out.store(true, Ordering::Relaxed);
                        return;
                    }
                    if stop() {
                        stopped.store(true, Ordering::Relaxed);
                        return;
                    }
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    if index >= files.len() {
                        return;
                    }
                    // open 成功即丢：只付扫描的代价，不读内容。
                    if File::open(&files[index]).is_ok() {
                        opened.fetch_add(1, Ordering::Relaxed);
                    }
                }
            });
        }
    });
    (
        opened.load(Ordering::Relaxed),
        timed_out.load(Ordering::Relaxed),
        stopped.load(Ordering::Relaxed),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_temp_dir(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "dsh-xlink-prestart-{}-{}-{}",
            label,
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    /// 造一个假包：`package.json` + 一个入口文件。
    fn fake_package(kernel_root: &Path, name: &str, dependencies: &[&str]) -> PathBuf {
        let dir = kernel_root.join("node_modules").join(name);
        fs::create_dir_all(dir.join("lib")).expect("create package dir");
        let deps = dependencies
            .iter()
            .map(|dependency| format!("\"{dependency}\":\"1.0.0\""))
            .collect::<Vec<_>>()
            .join(",");
        fs::write(
            dir.join("package.json"),
            format!("{{\"name\":\"{name}\",\"dependencies\":{{{deps}}}}}"),
        )
        .expect("write package.json");
        fs::write(dir.join("lib").join("index.js"), "// fixture").expect("write entry");
        dir
    }

    /// 造一份 profile 清单（bundle 列表 + 自己的依赖）。
    fn fake_profile(profile_dir: &Path, bundles: &[&str]) {
        fs::create_dir_all(profile_dir).expect("create profile dir");
        let bundles = bundles
            .iter()
            .map(|bundle| format!("\"{bundle}\""))
            .collect::<Vec<_>>()
            .join(",");
        fs::write(
            profile_dir.join("package.json"),
            format!("{{\"dsh\":{{\"profile\":{{\"bundles\":[{bundles}]}}}}}}"),
        )
        .expect("write profile manifest");
    }

    /// 四个条件逐一打开：任何一条不满足都不许预启动。
    ///
    /// 逐条分开断言（而不是只写一个「全开时为 true」）是必须的：把任意一条
    /// 改坏都可能被其余三条掩盖，而这个判据在生产里读的三个输入都来自进程外
    /// 状态（磁盘设置 / 启动来源 / data dir）——单测造不出「全部满足」的现场，
    /// 一旦漏判就是启动时多起一个内核或白读一遍树。
    #[test]
    fn should_prestart_requires_every_condition() {
        // 基准：开关开 + 非自启拉起 + 版本已装 + 端口空闲。
        assert!(should_prestart(true, false, Some("0.2.1-rc.3"), false));

        // ① 用户关掉开关。
        assert!(
            !should_prestart(false, false, Some("0.2.1-rc.3"), false),
            "开关关闭时不该预启动"
        );
        // ② 登录自启拉起：那条路径有自己的开关与线程。
        assert!(
            !should_prestart(true, true, Some("0.2.1-rc.3"), false),
            "登录自启拉起时不该由这里再起一个"
        );
        // ③ 没装版本 / 版本串是空白。
        assert!(!should_prestart(true, false, None, false));
        assert!(!should_prestart(true, false, Some(""), false));
        assert!(!should_prestart(true, false, Some("   "), false));
        // ④ 端口已被占用（工作台已经在服务）。
        assert!(
            !should_prestart(true, false, Some("0.2.1-rc.3"), true),
            "端口已占用时工作台已经在跑，不该再起一份"
        );
    }

    /// 闭包只含「从 bundle 走得到」的包，且只取 Node 真会读的文件类型。
    #[test]
    fn closure_covers_only_reachable_packages_and_js_json_files() {
        let root = unique_temp_dir("closure");
        let kernel = root.join("kernel");
        fake_package(&kernel, CLI_PACKAGE, &[]);
        fake_package(&kernel, "@deepseek-ai/bundle", &["dep-a"]);
        fake_package(&kernel, "dep-a", &[]);
        let unrelated = fake_package(&kernel, "unrelated", &[]);
        fs::write(unrelated.join("readme.md"), "x").expect("write md");
        let profile = root.join("profile");
        fake_profile(&profile, &["@deepseek-ai/bundle"]);

        let packages = hot_packages(&kernel, &profile);
        let names: Vec<String> = packages
            .iter()
            .filter_map(|package| package.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .collect();
        assert!(names.contains(&String::from("bundle")), "{names:?}");
        assert!(names.contains(&String::from("dep-a")), "{names:?}");
        assert!(names.contains(&String::from("dsh")), "{names:?}");
        assert!(
            !names.contains(&String::from("unrelated")),
            "走不到的包不该进闭包：{names:?}"
        );
        // 依赖在前：内核的加载顺序也是先依赖后包体。
        let at = |name: &str| {
            names
                .iter()
                .position(|item| item == name)
                .expect("in closure")
        };
        assert!(at("dep-a") < at("bundle"), "{names:?}");

        let files = hot_files(&packages);
        assert!(
            files.iter().all(|file| is_hot_file(file)),
            "只该取 .js/.mjs/.cjs/.json"
        );
        assert!(
            !files.iter().any(|file| file.starts_with(&unrelated)),
            "无关包的文件不该被打开"
        );
        assert!(!files.iter().any(|file| file.ends_with("readme.md")));

        fs::remove_dir_all(&root).ok();
    }

    /// 并行 open 跑通，计数与热文件集一致。
    #[test]
    fn prewarm_opens_every_hot_file_and_counts_them() {
        let root = unique_temp_dir("open");
        let kernel = root.join("kernel");
        fake_package(&kernel, CLI_PACKAGE, &[]);
        fake_package(&kernel, "@deepseek-ai/bundle", &["dep-a"]);
        fake_package(&kernel, "dep-a", &[]);
        let profile = root.join("profile");
        fake_profile(&profile, &["@deepseek-ai/bundle"]);

        let stats =
            prewarm_tree_with_deadline(&kernel, &profile, Duration::from_secs(30), &|| false);
        assert_eq!(stats.packages, 3, "CLI + bundle + 它的依赖");
        // 3 个包 ×（package.json + lib/index.js）= 6 个文件。
        assert_eq!(stats.files, 6);
        assert!(!stats.timed_out);
        assert!(!stats.stopped_early);

        fs::remove_dir_all(&root).ok();
    }

    /// 超时分支：0 死线必须立刻停下、什么都不开。生产用 40s 死线，真跑一遍
    /// 这个分支不现实，只能从死线一侧验证。
    #[test]
    fn prewarm_reports_timeout_without_opening_anything() {
        let root = unique_temp_dir("timeout");
        let kernel = root.join("kernel");
        fake_package(&kernel, CLI_PACKAGE, &[]);
        let profile = root.join("profile");
        fake_profile(&profile, &[]);

        let stats = prewarm_tree_with_deadline(&kernel, &profile, Duration::ZERO, &|| false);
        assert!(stats.timed_out, "0 死线必须立刻判定超时");
        assert_eq!(stats.files, 0, "超时后不该再打开任何文件");

        fs::remove_dir_all(&root).ok();
    }

    /// 被叫停（壳在退出 / 内核已经开始启动）时立刻收工，一个文件都不多开。
    #[test]
    fn prewarm_stops_when_asked_to() {
        let root = unique_temp_dir("stop");
        let kernel = root.join("kernel");
        fake_package(&kernel, CLI_PACKAGE, &[]);
        let profile = root.join("profile");
        fake_profile(&profile, &[]);

        let stats =
            prewarm_tree_with_deadline(&kernel, &profile, Duration::from_secs(30), &|| true);
        assert!(stats.stopped_early, "叫停必须被记下来");
        assert_eq!(stats.files, 0, "叫停后不该再打开任何文件");

        fs::remove_dir_all(&root).ok();
    }

    /// 树不存在时静默返回零值（版本目录在启动前被卸载也走这条），不能 panic。
    #[test]
    fn prewarm_tolerates_a_missing_tree() {
        let root = unique_temp_dir("missing");
        let stats = prewarm_tree_with_deadline(
            &root.join("no-kernel"),
            &root.join("no-profile"),
            Duration::from_secs(30),
            &|| false,
        );
        assert_eq!((stats.files, stats.packages), (0, 0));
        assert!(!stats.timed_out);
        fs::remove_dir_all(&root).ok();
    }
}
