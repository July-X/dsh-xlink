//! 启动时的内核树预热与工作台后台预启动（B1 + B2）。
//!
//! ## 为什么
//!
//! 2026-10-27 真机启动：壳 07:39:39 起来 → 内核 07:39:45 派生 → 端口就绪
//! 07:40:37，冷启动 52.1s、全程约 61s。隔离复现（同一份 home + 内核树）：
//! 冷跑 54.1s wall / 10.4s CPU / 7,682 reads / 124–138k metadata ops，紧接着
//! 热跑 10.9s（CPU 反而 11.9s），**两次 IO 计数完全一致**——也就是说代码与
//! CPU 都没变慢，多出来的 43 秒是杀毒软件对**首次访问**的扫描延迟（Defender
//! 实时保护：首次打开 17–18ms，stat 0.28ms vs 热态 0.07ms），乘以内核在
//! 约 35k 文件的树上做元数据密集的模块解析。壳自己的启动很快（窗口
//! 0.14–0.27s），所以这笔钱省不掉，只能挪到用户点按钮之前。
//!
//! ## 怎么分摊（用户拍板的方向）
//!
//! - **B1 预热内核树**：只读元数据地遍历一遍已安装内核树（`read_dir` +
//!   `file_type` + `symlink_metadata`，不打开任何文件内容），先把 AV 的首次
//!   扫描代价付掉；上限 [`PREWARM_DEADLINE`]（20s，冷态 35k 文件约 10–15s），
//!   走不完也照常往下启动——它只能缩短等待，不能把后台就绪往后拖太久。
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
//! 壳开始退出时**不再派生内核**（[`SHELL_SHUTTING_DOWN`]）：退出清理跑完
//! 之后由后台线程补起一个内核会留下占端口的孤儿。
//!
//! ## 与登录自启的关系
//!
//! 登录自启拉起时**不预启动**：那条路径有自己的 `autostart_kernel` 开关与
//! `dsh-autostart-kernel` 线程（见 `lib.rs` 的 setup），这里再起一次就是
//! 两个线程抢同一个启动动作。两处都经过 `start_kernel_blocking` 的 lifecycle
//! 锁，但「一个动作只有一个发起方」比「靠锁串行」更容易讲清楚。

use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use tauri::Manager;

use crate::commands::AppState;
use crate::kernel::lifecycle;
use crate::shell::{resident, settings, shell_events};

/// 等面板先画出第一帧再动手：用户双击图标后要先看到界面，而不是窗口还没
/// 画出来、背后已经有一堆 node 进程在起。2s 足够面板加载骨架（实测窗口
/// 可见 0.14–0.27s，首条状态轮询紧随其后）。
const PRESTART_DELAY: Duration = Duration::from_secs(2);

/// 预热上限。热态遍历这棵树是秒级；冷态单次首次 stat 实测 0.28ms（vs 热态
/// 0.07ms），35k 文件约 10–15s。上限取 20s 而不是更久，是因为**预热只是把
/// 后续启动的等待前移**：它每多花 1s，后台工作台就晚 1s 就绪。20s 足够覆盖
/// 正常冷态，极端慢盘/扫描风暴下宁可少预热一部分，也不能把「用户点开时已经
/// 好了」拖成两分钟。
const PREWARM_DEADLINE: Duration = Duration::from_secs(20);

/// 事件日志的逻辑名。落在 `<shell_logs_dir>/<kind>-kernel-prestart-<date>.log`，
/// 自动出现在「查看日志」面板——后台动作没有别的地方可查。
const EVENT_LOG: &str = "kernel-prestart";

/// 壳是否正在退出。`RunEvent::Exit` 先置位，后台线程在预热结束、真正派生
/// 内核**之前**再看一眼：用户在「已启动壳、但还没打开工作台」的窗口里退出
/// 时，不能被后台线程在退出清理之后又派生出一个内核，留下占着端口的孤儿。
///
/// 为什么不只靠 lifecycle 锁：退出路径为了不卡住用户并不拿那把锁（见
/// `lib.rs` 的 `RunEvent::Exit`），锁只能串行化「启动 vs 停止」，不能让一个
/// 还没开始的启动自己让路。
static SHELL_SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);

/// 由 `lib.rs` 的 `RunEvent::Exit` 调用：进程退出后不再发起后台内核启动。
pub(crate) fn mark_shutting_down() {
    SHELL_SHUTTING_DOWN.store(true, Ordering::SeqCst);
}

/// 一次内核树预热的统计。只用于事件日志与测试，不参与任何启动判定。
#[derive(Debug, Default)]
pub(crate) struct PrewarmStats {
    /// 触发过 stat 的普通文件数。
    pub(crate) files: usize,
    /// 递归进入的子目录数（不含根目录本身）。
    pub(crate) dirs: usize,
    /// 整次遍历耗时（含超时判定与出错跳过的部分）。
    pub(crate) elapsed_ms: u64,
    /// 是否撞上 [`PREWARM_DEADLINE`]（true = 树没走完）。
    pub(crate) timed_out: bool,
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

/// 线程主体：延迟 → 判据 → 预热内核树 → 试着把工作台拉起来。
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

    // B1：只读元数据地遍历内核树，把 AV「首次访问」的扫描代价提前付掉。
    // 失败 / 超时只影响这次缓解本身，不影响随后启动。
    let kernel_root = lifecycle::kernel_dir(&data_dir, version);
    shell_events::record(
        EVENT_LOG,
        &format!("后台预启动：开始预热内核树 {}", kernel_root.display()),
    );
    let stats = prewarm_tree(&kernel_root);
    let timeout_note = if stats.timed_out {
        format!(
            "，已到 {}s 上限，剩余目录未预热",
            PREWARM_DEADLINE.as_secs()
        )
    } else {
        String::new()
    };
    shell_events::record(
        EVENT_LOG,
        &format!(
            "后台预启动：内核树预热完成（{} 个文件，{} 个目录，耗时 {}ms{}）",
            stats.files, stats.dirs, stats.elapsed_ms, timeout_note
        ),
    );

    // 壳在这段时间里开始退出（用户在预热期间点了退出）：不再派生内核，
    // 否则退出清理跑完之后起一个占端口的孤儿进程。
    if SHELL_SHUTTING_DOWN.load(Ordering::SeqCst) {
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

/// 只读元数据地遍历 `root`，把每个目录项都 stat 一遍（**不打开任何文件**）。
///
/// 为什么只 stat：冷启动慢的不是读文件内容，而是杀毒软件对**每个首次访问
/// 的路径**做扫描（实测首次打开 17–18ms，stat 也有 0.28ms vs 热态 0.07ms），
/// 而内核启动做的是元数据密集的模块解析——先把这棵树的目录项都摸一遍，
/// 真正启动时这些路径对过滤驱动来说已经是「见过」的。
///
/// 用迭代而不是递归：内核树又深又宽，递归既可能撞栈，超时路径也难写清楚。
/// 任何 IO 错误都跳过（树可能正在被卸载 / 某个子目录权限受限——那是启动
/// 路径自己要报的错，不是预热该管的事）。
pub(crate) fn prewarm_tree(root: &Path) -> PrewarmStats {
    prewarm_tree_with_deadline(root, PREWARM_DEADLINE)
}

/// [`prewarm_tree`] 的 deadline 参数版：生产用 [`PREWARM_DEADLINE`]，测试用
/// 0 验证超时分支（真跑 20s 的用例没人愿意留，而不测这条分支它就是死代码）。
fn prewarm_tree_with_deadline(root: &Path, deadline: Duration) -> PrewarmStats {
    let started = Instant::now();
    let mut stats = PrewarmStats::default();
    let mut pending = vec![root.to_path_buf()];
    'walk: while let Some(dir) = pending.pop() {
        if started.elapsed() >= deadline {
            stats.timed_out = true;
            break;
        }
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            if started.elapsed() >= deadline {
                stats.timed_out = true;
                break 'walk;
            }
            // `file_type()` 在 Unix 上用目录项自带的 `d_type`、Windows 上读
            // 目录项自带的属性，不额外打开文件；真正的 stat 是下面那句。
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() {
                // 不跟随链接：预热不该走出内核树，更不该被环状链接拖死。
                continue;
            }
            let path = entry.path();
            let Ok(metadata) = fs::symlink_metadata(&path) else {
                continue;
            };
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                stats.dirs += 1;
                pending.push(path);
            } else if metadata.is_file() {
                stats.files += 1;
            }
        }
    }
    stats.elapsed_ms = started.elapsed().as_millis() as u64;
    stats
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

    /// 遍历只数元数据、不读内容；文件 / 目录计数与树的形状一致。
    #[test]
    fn prewarm_tree_counts_a_nested_tree() {
        let root = unique_temp_dir("nested");
        fs::create_dir_all(root.join("a").join("nested").join("deep"))
            .expect("create a/nested/deep");
        fs::create_dir_all(root.join("b")).expect("create b");
        for (relative, bytes) in [
            ("f1.txt", &b"one"[..]),
            ("a/f2.txt", b"two"),
            ("a/nested/f3.txt", b"three"),
            ("a/nested/deep/f4.txt", b"four"),
            ("b/f5.txt", b"five"),
        ] {
            fs::write(root.join(relative), bytes).expect("write test file");
        }

        let stats = prewarm_tree(&root);
        assert_eq!(stats.files, 5, "5 个文件应各触发一次 stat");
        assert_eq!(
            stats.dirs, 4,
            "a / a/nested / a/nested/deep / b 共 4 个子目录（根目录不计）"
        );
        assert!(!stats.timed_out, "小树不该超时");
        assert!(
            stats.elapsed_ms < PREWARM_DEADLINE.as_millis() as u64,
            "小树耗时不该达到 60s 上限：{}ms",
            stats.elapsed_ms
        );

        fs::remove_dir_all(&root).ok();
    }

    /// 树不存在时静默返回零值（版本目录在启动前被卸载也走这条），不能 panic。
    #[test]
    fn prewarm_tree_tolerates_a_missing_root() {
        let root = unique_temp_dir("missing").join("not-there");
        let stats = prewarm_tree(&root);
        assert_eq!((stats.files, stats.dirs), (0, 0));
        assert!(!stats.timed_out);
    }

    /// 超时分支：0 上限必须立刻停下并置 `timed_out`。生产用 60s 上限，真跑
    /// 一遍这个分支不现实，只能从 deadline 一侧验证。
    #[test]
    fn prewarm_tree_reports_timeout() {
        let root = unique_temp_dir("timeout");
        fs::write(root.join("f.txt"), b"x").expect("write test file");
        let stats = prewarm_tree_with_deadline(&root, Duration::ZERO);
        assert!(stats.timed_out, "0 上限必须立刻判定超时");
        assert_eq!(
            (stats.files, stats.dirs),
            (0, 0),
            "超时后不该再碰任何目录项"
        );
        fs::remove_dir_all(&root).ok();
    }
}
