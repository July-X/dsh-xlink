//! 工作台窗口的加载看门狗与生命周期。
//!
//! **为什么需要它**：工作台页面里那个 `harness-health.js` 是注入脚本，与页面
//! 同生共死——页面没加载出来时它同样没跑，于是「白屏 / 直接黑屏」这类故障在
//! `last-incident.json` 里一个字都留不下。2026-09-29 实测两次黑屏，两个壳都
//! 没有事故记录，因为页面里一行 JS 都没执行。能观测到它的只有壳这一侧：
//! webview 自己的加载事件。
//!
//! Tauri 2.11 那版的 `PageLoadEvent` 只有 `Started` / `Finished`，**没有失败变体**
//! （当前栈：Tauri 2.12.3 / wry 0.57.0）
//! ——导航失败时两个都不触发。所以判据只能是超时：开始加载之后一段时间内没
//! 等到 `Finished`，就是页面没起来。
//!
//! 这个看门狗最大的风险不是「该重载时不重载」，而是反过来——把用户的窗口变成
//! 一个自己跟自己打架的东西。三条自律写在 [`should_reload`] 上。
//!
//! ## 为什么还多了一级「重建窗口」
//!
//! [`should_reload`] 与页面内那次自愈都只会 `reload()`。而 2026-09-30 实测的
//! 黑屏里有一种 reload 救不回来：页面被内核 `client-hmr` 推的 `rebuilt` 帧换掉
//! 模块后死在装配窗口里，或 WebView2 渲染进程被打崩——`reload()` 落在一块死掉
//! 的文档上，用户看到的仍是一片黑（用户原话：「reload 后，还是会黑屏」）。
//! **重建窗口是壳侧唯一能换掉渲染进程的动作**：它建出一个全新的 webview。
//! 因此 [`recreate`] 是自愈链条的最后一级。
//!
//! ## 重建的两条纪律（2026-09-30 真机数据改出来的）
//!
//! - **别落进风暴里**：对面包活动（装 / 删内核）进行期间，页面随时可能再死一次
//!   ——15:36:32 的自动重建落在对面卸载风暴中间，新窗口 4 秒后又撞死。自动重建
//!   走 [`recreate_when_quiet`]：先等信标平息（有界），再动手。
//! - **预算是「有限次 + 冷却」，不是「一次」**：一次性闸门在那次事故里把用户晾
//!   在黑屏上 25 秒，直到手动刷新。[`MAX_REBUILDS`] + [`REBUILD_COOLDOWN`] 才是
//!   「窗口别自己跟自己打架」的正确闸——风暴过后额度还在。
//!
//! **可观测性**：触发条件、重载与重建结果都落 `shell_events`（`harness-window.log`，
//! 「查看日志」面板里可见），不靠 `eprintln!`——GUI 应用的 stderr 在 Windows
//! 上没有任何去处，而"页面闪了一下"这种只有后果没有原因的现象，没有这条
//! 日志就只能靠时间戳对猜。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use tauri::webview::{NewWindowFeatures, NewWindowResponse, PageLoadEvent};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};
use url::Url;

use crate::commands::AppState;

/// `Started` 之后多久还没 `Finished` 就判定页面没起来。
///
/// **实测依据**（2026-09-29，本机、内核 0.2.0-rc.1、隔离 DSH_HOME 实跑）：
/// 内核冷启动到可访问 2.4s，首屏 index + 三批组合 bundle（1 + 58 + 6 = 65 个
/// 模块）合计 0.1s。**这些 bundle 由 127.0.0.1 本地提供，不走网络**，所以首屏
/// 耗时只有 CPU 与磁盘——把门槛设成分钟级是没有依据的，那只会让一个已经死掉的
/// 窗口白白多亮 40s。
///
/// 取 15s = 实测基线的 6 倍：机器正忙（例如同时在装内核、磁盘被写满）时留够
/// 余量，但一个真正卡死的窗口也能在 15s 内被救回来，而不是让用户干等。
pub const LOAD_TIMEOUT: Duration = Duration::from_secs(15);

/// 单个窗口生命周期内最多自动重载几次（上限 3，再多就不是「兜底」而是「闪」）。
///
/// 上限存在的理由和页面内那次自愈一样：刷新救不回来的故障，重试一万次也还是
/// 救不回来，只会让人看着窗口反复闪。超限后停手，让用户看到真实错误。
pub const MAX_RELOADS: u8 = 2;

/// 整个进程内最多自动重建几次窗口。
///
/// 曾经是「每进程只重建一次」。2026-09-30 真机数据把它证伪了：那次自动重建
/// 落在对面卸载内核的风暴中间（15:36:32 重建 → 15:36:40 新窗口又撞死 → 额度
/// 已尽），黑屏一直挂到 15:37:05 用户手动刷新——把用户晾在黑屏上的不是故障
/// 本身，是那道一次性闸门。「窗口别自己跟自己打架」的闸改由 [`REBUILD_COOLDOWN`]
/// 承担：有限次 + 冷却，风暴过后额度还在。
pub const MAX_REBUILDS: u8 = 3;

/// 两次自动重建之间的最小间隔。冷却防的是「窗口自己跟自己打架」；期间到达的
/// 故障报告由 [`recreate_when_quiet`] 的在途等待者兜住（它落地时会再过一次预算）。
pub const REBUILD_COOLDOWN: Duration = Duration::from_secs(20);

/// 自动重建等风停的上限：信标可能被一次卡死的装包拖着（TTL 10 分钟），无条件
/// 等下去等于把自愈关掉。到点按原计划重建——那时还有页面自愈、手动「刷新工作台」
/// 两层兜底。
pub const QUIET_WAIT_CAP: Duration = Duration::from_secs(240);

/// 等待期间的轮询间隔。
pub const QUIET_POLL: Duration = Duration::from_secs(2);

/// 工作台 webview 的加载观测量。
#[derive(Default)]
pub struct HarnessPage {
    /// 最近一次 `Started` 的时刻；收到 `Finished` 就清空。
    pub pending_since: Option<Instant>,
    /// 本次窗口生命周期内已自动重载的次数。
    pub reloads: u8,
    /// 本进程内已自动重建的次数与最近一次时刻（[`MAX_REBUILDS`] /
    /// [`REBUILD_COOLDOWN`] 的账本）。用户手动刷新先走 [`reset_budget`] 清账，
    /// 所以那一路永远有额度。
    pub rebuilds: u8,
    pub last_rebuild: Option<Instant>,
    /// 是否已有一个「等风停再重建」的在途等待者。故障报告连环到达时后来的
    /// 直接放弃——在等的那次落地时会重新过预算与判据。
    pub quiet_waiter: bool,
}

/// 记一次加载事件。挂在工作台窗口的 `on_page_load` 上。
///
/// 两侧都落一条事件日志：只有重载时留痕的话，事后看到的是"某时刻页面闪了
/// 一下"，而看不到它**卡了多久**、**卡从什么时候开始**——那才是判断是内核
/// 慢、是机器忙、还是看门狗本身误判的依据。
pub fn observe(state: &AppState, started: bool) {
    let mut page = crate::lock(&state.harness_page);
    if started {
        page.pending_since = Some(Instant::now());
        drop(page);
        crate::shell::shell_events::record(HARNESS_WINDOW_LOG, "工作台页面开始加载");
    } else {
        page.pending_since = None;
    }
}

/// 是否该因为「页面长时间没加载出来」而重载一次工作台窗口。
///
/// 抽成纯函数是因为它是这里唯一有分支的地方，而真正难测的恰恰是**不该重载的
/// 那几条**：真正干活的那段要 `AppHandle` 和真实 webview，测不起。
///
/// `timeout` 由调用方按 [`crate::kernel::package_activity::load_timeout`] 算好后传进来
/// ——判据本身不该知道装包信标的事，阈值策略住在那个模块里。
pub fn should_reload(
    pending_since: Option<Instant>,
    kernel_running: bool,
    reloads: u8,
    timeout: Duration,
) -> bool {
    let Some(since) = pending_since else {
        return false;
    };
    // 内核真挂了的时候重载窗口没有意义：那种情况该显示启动失败，掩盖它比
    // 不重载更糟。
    if !kernel_running || reloads >= MAX_RELOADS {
        return false;
    }
    since.elapsed() >= timeout
}

/// 重载额度已经用完、页面**仍然**没加载完时，是否该换一整个窗口。
///
/// 这是 [`should_reload`] 的下一级，同样抽成纯函数：不该动手的情形（预算用尽、
/// 冷却未过、内核没跑、重载额度未用完）都在这里判掉，其余交给 [`recreate`]。
pub fn should_recreate(
    pending_since: Option<Instant>,
    kernel_running: bool,
    reloads: u8,
    rebuilds: u8,
    cooldown_ready: bool,
    timeout: Duration,
) -> bool {
    if rebuilds >= MAX_REBUILDS || !cooldown_ready || !kernel_running || reloads < MAX_RELOADS {
        return false;
    }
    pending_since.is_some_and(|since| since.elapsed() >= timeout)
}

/// 冷却是否已过：没重建过（第一次）或距上次自动重建已过 [`REBUILD_COOLDOWN`]。
pub fn cooldown_ready(last_rebuild: Option<Instant>) -> bool {
    // MSRV 曾是 1.77 时这里刻意用 `map_or`（`is_none_or` 要 1.82）；
    // tauri 2.12 把 rust-version 抬到 1.90 后约束解除，回归 clippy 的标准写法。
    last_rebuild.is_none_or(|at| at.elapsed() >= REBUILD_COOLDOWN)
}

/// 页面报上来的故障，是不是「刷新已经救不回来」的那一种。
///
/// 判据借用**页面自己的**额度，而不是壳这边的猜测：`harness-health.js` 第一次
/// 撞上槽位装配不变量时上报 `slot-assembly` 并在 3 秒后自己刷新一次；额度用掉
/// 之后同一句错误改用 `runtime-error` 上报（见该脚本的
/// `handleSlotAssemblyFailure`）。于是「槽位不变量 + runtime-error」精确等价于
/// 「页面已经自己刷过一次、没刷好」——这正是壳必须换窗口的唯一时机。
///
/// 槽位短语取 [`crate::kernel::kernel_evidence::SLOT_PHRASES`]（与 `guard.rs` 归因同一份
/// 列表），不在这里另抄一份。
pub fn fault_needs_new_window(kind: &str, message: &str, stack: &str) -> bool {
    // 证据格式**必须**与 `guard.rs` 合并前端证据时的那一份一致（`工作台前端错误：`
    // / `前端堆栈：` 两个前缀）：`kernel_evidence::is_slot_failure` 只读带前缀的
    // 行，直接把裸 message 拼进去会让这个判据永远为假——一个永远不成立的
    // 「该换窗口了」比没有这个判据更坏，它看起来在工作其实永远不会响。
    kind.trim() == "runtime-error"
        && crate::kernel::kernel_evidence::is_slot_failure(&format!(
            "工作台前端错误：{}\n前端堆栈：{}",
            message.trim(),
            stack.trim()
        ))
}

/// 工作台窗口长时间没加载出来时重载一次；额度用完仍未加载完则重建窗口。
/// 由状态轮询顺带驱动，不另起定时器。
pub fn reload_stalled(app: &AppHandle, kernel_running: bool) {
    let (pending, reloads, rebuilds, last_rebuild) = {
        // `app.state::<T>()` 返回的是**值**，guard 借用的是它内部的 Mutex——
        // 写成 `lock(&app.state::<T>().field)` 会让 guard 活得比那个临时值久
        // （E0716）。先把 State 绑成局部量，生命周期才够。
        let state = app.state::<AppState>();
        let page = crate::lock(&state.harness_page);
        (
            page.pending_since,
            page.reloads,
            page.rebuilds,
            page.last_rebuild,
        )
    };
    // 装包活动期间放宽阈值：对面在装/删内核时，本壳页面加载慢是应该的，
    // 不是故障。见 package_activity::load_timeout 的文档注释。
    let timeout = crate::kernel::package_activity::load_timeout();
    if !should_reload(pending, kernel_running, reloads, timeout) {
        if should_recreate(
            pending,
            kernel_running,
            reloads,
            rebuilds,
            cooldown_ready(last_rebuild),
            timeout,
        ) {
            recreate_when_quiet(app, "刷新后仍未加载出来");
        }
        return;
    }
    let Some(window) = app.get_webview_window("harness") else {
        return;
    };
    {
        let state = app.state::<AppState>();
        let mut page = crate::lock(&state.harness_page);
        // 抢在导航之前落账：`reload()` 可能同步触发新的 `Started`，顺序反了
        // 会让这一次重载不计数，等于把上限变成无限。
        page.reloads += 1;
        page.pending_since = None;
        // **落盘而不是 eprintln**：壳是 GUI 应用，stderr 在 Windows 上没有
        // 任何去处，而"页面闪了一下"这种只有后果、没有原因的动作恰恰只能靠
        // 这里留痕——用户报障时我们要能回答"壳当时做过什么"。
        let message = format!(
            "工作台窗口超过 {}s 没有加载完成，已自动重载（第 {} 次，上限 {}）",
            LOAD_TIMEOUT.as_secs(),
            page.reloads,
            MAX_RELOADS
        );
        eprintln!("dsh-xlink: {message}");
        crate::shell::shell_events::record(HARNESS_WINDOW_LOG, &message);
    }
    if let Err(error) = reload_window(&window) {
        eprintln!("dsh-xlink: 工作台窗口重载失败：{error}");
        crate::shell::shell_events::record(
            HARNESS_WINDOW_LOG,
            &format!("工作台窗口重载失败：{error}"),
        );
    }
}

/// 页面报上来「刷新已经救不回来」之后，壳要不要换一整个窗口。
///
/// 这是**黑屏自愈的最后一环**，也是 2026-09-30 那次用户看得见的最后一步：此前
/// [`fault_needs_new_window`] 算得出这个答案却**没有任何生产调用方**——它只在测试
/// 里被断言。于是那条链的末端是：页面自己刷过一次、没刷好、壳弹一个事故面板，
/// 然后**窗口就停在黑屏上**。而手动「刷新工作台」（同一个 [`recreate`]）已经证明
/// 换窗口能救回来——自动链没有走这一步，是漏接而不是判断为「不该」。
///
/// `kernel_running` 与 [`should_recreate`] 同一条纪律：内核真的没在跑时换窗口只是
/// 把同一个连接失败重新渲染一遍，掩盖它比不换更糟。「每进程只重建一次」那道闸在
/// [`recreate`] 里面，不在这里重复。
pub fn should_recreate_after_fault(
    kind: &str,
    message: &str,
    stack: &str,
    kernel_running: bool,
) -> bool {
    kernel_running && fault_needs_new_window(kind, message, stack)
}

/// 上面那个判据的**动作**，与判据放在一处：命令层只该递证据，不该知道「黑屏之后
/// 要换一个窗口」这件事是怎么做的——`commands.rs` 是反棘轮文件，而这里才是自愈
/// 链条该待的地方。
///
/// 动作走 [`recreate_when_quiet`]：对面包活动还在进行时先等风停（有界）。等风
/// 期间**不持生命周期锁**——一等可能是几分钟，锁住它会把「关闭工作台」也冻住；
/// 等待线程落地前会自己再核一遍内核是否仍在服务。
pub fn recreate_after_fault(app: &AppHandle, health: &crate::diagnostics::guard::HealthReport) {
    // 内核是否在服务由这一层自己问：命令层不该为了调一个自愈动作而先去读设置与
    // 探端口（`commands.rs` 是反棘轮文件，那里每多一行都要从别处省一行）。
    let settings = crate::shell::settings::load_for_shell(crate::shell::settings::current_mode());
    let serving = crate::kernel::lifecycle::port_open(settings.port);
    if !should_recreate_after_fault(&health.kind, &health.message, &health.stack, serving) {
        return;
    }
    recreate_when_quiet(app, "刷新后页面仍报同一处槽位故障");
}

/// 自动重建的「等风停」外壳：对面包活动还在进行时先把重建挂起，等活动结束
/// （或 [`QUIET_WAIT_CAP`] 到点）再动手。
///
/// 2026-09-30 实测（`release-harness-window.log` 15:36 那段）：自动重建落在对面
/// 卸载内核的风暴中间，新窗口 4 秒后撞上同一处槽位故障再死一次——**在风暴里
/// 重建等于再掷一次骰子**。信标（[`crate::kernel::package_activity`]）正是让恢复动作
/// 知道「现在别落子」的那只表。
///
/// 等待在后台线程上进行：调用方（状态轮询 / 故障报告的命令线程）不该被一个
/// 可能长达数分钟的等待卡住。同一时刻只派一个等待者
/// （[`HarnessPage::quiet_waiter`]），故障报告连环到达时后来的直接放弃——在途
/// 的那一次落地时会重新过预算、冷却与「内核仍在服务」三道闸。
pub fn recreate_when_quiet(app: &AppHandle, reason: &str) {
    {
        let state = app.state::<AppState>();
        let mut page = crate::lock(&state.harness_page);
        if page.quiet_waiter {
            return;
        }
        page.quiet_waiter = true;
    }
    let handle = app.clone();
    let why = reason.to_string();
    let spawned = std::thread::Builder::new()
        .name("dsh-recreate-when-quiet".into())
        .spawn(move || {
            let deadline = Instant::now() + QUIET_WAIT_CAP;
            let mut announced = false;
            while let Some(left) = crate::kernel::package_activity::remaining() {
                if Instant::now() >= deadline {
                    crate::shell::shell_events::record(
                        HARNESS_WINDOW_LOG,
                        &format!(
                            "对面包活动仍未结束（还剩约 {}s），等待上限已到，按原计划重建工作台窗口（{why}）",
                            left.as_secs()
                        ),
                    );
                    break;
                }
                if !announced {
                    announced = true;
                    crate::shell::shell_events::record(
                        HARNESS_WINDOW_LOG,
                        &format!(
                            "对面包活动进行中（还剩约 {}s），自动重建暂缓，等风停再动手（{why}）",
                            left.as_secs()
                        ),
                    );
                }
                std::thread::sleep(QUIET_POLL);
            }
            // 等待期间内核可能被停掉：换窗口只会把同一个连接失败重新渲染一遍。
            let settings = crate::shell::settings::load_for_shell(crate::shell::settings::current_mode());
            if !crate::kernel::lifecycle::port_open(settings.port) {
                crate::shell::shell_events::record(
                    HARNESS_WINDOW_LOG,
                    "等待期间内核已停止，取消自动重建（重新启动工作台时会建新窗口）",
                );
            } else {
                let _ = recreate(&handle, &why);
            }
            let state = handle.state::<AppState>();
            crate::lock(&state.harness_page).quiet_waiter = false;
        });
    if let Err(error) = spawned {
        // 线程都起不来时退回立即重建：等风停是优化，不是前提。
        eprintln!("dsh-xlink: 等风停线程创建失败，直接重建：{error}");
        {
            let state = app.state::<AppState>();
            crate::lock(&state.harness_page).quiet_waiter = false;
        }
        let _ = recreate(app, reason);
    }
}

/// 拆掉工作台窗口再原样建一个。自愈链条的**最后一级**。
///
/// `reason` 是这次重建的**成因**，它会原样进事件日志：自动重建与用户手动
/// 「刷新工作台」走的是同一个动作，但原因不同——日志里写错成因（把手动说成
/// 「自动」）会让事后排查顺着一件没发生过的事去找。
///
/// 预算是「有限次 + 冷却」（[`MAX_REBUILDS`] / [`REBUILD_COOLDOWN`]，2026-09-30
/// 之前是每进程一次，那次真机事故里它把用户晾在黑屏上 25 秒）。用户手动按的那次
/// 由 [`reset_budget`] 先清账再进来，所以手动出路永远有额度。
///
/// 用 [`AppState::harness_url`] 里记录的**本次加载地址**重建，而不是重新解析
/// 入口 URL：重建要的是"同一个页面再来一次"，重新解析会在内核刚重启、还没写出
/// launch token 时拿到别的地址，把一种故障换成另一种。
///
/// 预算耗尽或冷却未过时静默返回 `Ok`：调用方（等待线程 / 状态轮询）把这次
/// 当作「已处理」，用户仍有手动那条路（本函数本身就是它）。
///
/// **成败都必须落盘，且成功那条只能在 `open()` 返回之后写。** 2026-10-18 实机：
/// 日志写下「已拆掉并重建」之后 20 分钟没有出现过任何一次页面加载，而建窗失败
/// 只进了 `eprintln!`——GUI 应用在 Windows 上没有 stderr 去处，事后既查不到
/// 「有没有重建」也查不到「为什么没建出来」。旧窗口此刻已经拆掉，这条失败是
/// 用户面前没有工作台的直接原因，必须能在「查看日志」里看到。
pub fn recreate(app: &AppHandle, reason: &str) -> Result<(), String> {
    let Some(url) = crate::lock(&app.state::<AppState>().harness_url).clone() else {
        return Ok(());
    };
    let attempt = {
        let state = app.state::<AppState>();
        let mut page = crate::lock(&state.harness_page);
        if page.rebuilds >= MAX_REBUILDS || !cooldown_ready(page.last_rebuild) {
            return Ok(());
        }
        page.rebuilds += 1;
        page.last_rebuild = Some(Instant::now());
        page.reloads = 0;
        page.pending_since = None;
        page.rebuilds
    };
    if let Some(window) = app.get_webview_window("harness") {
        let _ = window.destroy();
    }
    let parsed = match Url::parse(&url) {
        Ok(parsed) => parsed,
        Err(error) => {
            let message = recreate_failure_message(reason, &format!("工作台地址无法解析：{error}"));
            crate::shell::shell_events::record(HARNESS_WINDOW_LOG, &message);
            return Err(message);
        }
    };
    match open(app, parsed) {
        Ok(()) => {
            crate::shell::shell_events::record(
                HARNESS_WINDOW_LOG,
                &recreate_success_message(reason, attempt),
            );
            Ok(())
        }
        Err(error) => {
            let message = recreate_failure_message(reason, &error);
            crate::shell::shell_events::record(HARNESS_WINDOW_LOG, &message);
            Err(message)
        }
    }
}

/// 重建成功的日志行。只允许在 `open()` 返回 `Ok` 之后调用——写早了就是
/// 「日志说重建了、用户面前却没有窗口」。
fn recreate_success_message(reason: &str, attempt: u8) -> String {
    format!("工作台窗口{reason}，已拆掉并重建（本进程第 {attempt} 次，上限 {MAX_REBUILDS} 次）")
}

/// 重建失败的日志行：结果、原因、用户的下一步都要在。
fn recreate_failure_message(reason: &str, error: &str) -> String {
    format!(
        "工作台窗口{reason}，重建失败：{error}。请关闭工作台窗口后，到概览页点「工作台窗口」重新打开；\
         仍失败请重启应用"
    )
}

/// 清空看门狗的额度，让下一次 [`recreate`] 真的动手。
///
/// 预算是给**自动**自愈的闸：它防的是窗口自己跟自己打架。用户手动按「刷新
/// 工作台」时那道闸恰好挡住了唯一能救他出路的那次动作——所以手动路径必须先把
/// 账清零（重建次数与冷却一起清，手动出路永远有额度）。清零之后 [`recreate`]
/// 会重新记账，自动链条在这次手动重建之后仍受冷却与预算约束：用户按完还能再按，
/// 壳不会自己在背后接着重建。
pub fn reset_budget(state: &AppState) {
    let mut page = crate::lock(&state.harness_page);
    page.pending_since = None;
    page.reloads = 0;
    page.rebuilds = 0;
    page.last_rebuild = None;
}

/// 建出工作台窗口。
///
/// **必须开一条 OS 线程**：在 Tauri 命令线程里同步构造 webview 在 Windows 上会
/// 死锁（与 `open_log_window` 同一理由），所以这里 spawn 之后立刻取回结果。
pub fn open(app: &AppHandle, url: Url) -> Result<(), String> {
    let handle = app.clone();
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("dsh-open-harness".into())
        .spawn(move || {
            let result = build(&handle, &url);
            if let Err(ref e) = result {
                eprintln!("dsh-xlink: failed to open harness window: {e}");
            }
            #[cfg(debug_assertions)]
            if result.is_ok() {
                if let Some(window) = handle.get_webview_window("harness") {
                    window.open_devtools();
                }
            }
            let _ = tx.send(result);
        })
        .map_err(|e| e.to_string())?;
    rx.recv()
        .map_err(|_| "工作台窗口创建线程已结束，未返回结果".to_string())?
}

/// 文档标题同步时的窗口标题：内核用 `document.title` 表达「会话名 — 产品名」，
/// 官方 Electron 壳默认让窗口标题跟着走；壳用 Tauri 的
/// `on_document_title_changed` 做同一件事（零注入），标题仍走
/// [`crate::shell::window::window_title`] 的「名字 — 应用名」约定。
pub(crate) fn title_from_document(document_title: &str, fallback: &str) -> String {
    let trimmed = document_title.trim();
    let name = if trimmed.is_empty() {
        fallback
    } else {
        trimmed
    };
    crate::shell::window::window_title(name)
}

/// 一条 `window.open` 请求的处置策略。
///
/// 三条**互相独立**的东西，凑在一处是因为它们都决定「这条请求归谁」：
///
/// 1. **`keep_login_in_shell`**：认证域（Google / Apple / Microsoft / GitHub
///    的账号主机）**留在壳内**。理由：认证链跨出壳 ⇒ 会话 cookie 落在别的
///    浏览器里 ⇒ 壳内的 webview 永远拿不到登录态 ⇒ 用户看到「登录做完了，
///    页签还是未登录」，**壳侧一行日志都没有**。
/// 2. **`profile_dir` / `browser_args`**：托管弹窗要用**哪一份浏览器档案**。
///    弹窗必须与发起它的那扇 webview **同一份**档案，否则 cookie 不通：
///    Windows 上 WebView2 的 user-data 目录不同就是两套 cookie jar，而且
///    同一目录的环境配置必须完全一致（`additional_browser_args` 也要跟着
///    一起给）。
/// 3. **`data_store_identifier`**：macOS 上的同一件事**换个开关**。
///    WKWebView **不认** `data_directory`（tauri 的方法文档原话：WKWebView 里
///    `data_directory` 不可用时用它替代），认的是这个标识符。不显式绑**同一个**
///    标识符，弹窗就落到**默认**数据存储 ⇒ 里面没有发起方那份 Google session
///    ⇒ 认证弹窗里出不来账号选择器（用户在壳内看到的仍然是「只能输邮箱」）。
#[derive(Clone, Default)]
pub(crate) struct PopupPolicy {
    /// 认证域留在壳内（不交系统浏览器）。
    pub keep_login_in_shell: bool,
    /// 托管弹窗的 user-data 目录（Windows）。`None` = 用默认目录。
    pub profile_dir: Option<std::path::PathBuf>,
    /// 托管弹窗的浏览器参数（Windows；必须与发起方同一份）。
    pub browser_args: Option<String>,
    /// 托管弹窗要绑定的 **WKWebsiteDataStore 标识符**（macOS 专有）。
    ///
    /// 与发起方**同一个**标识符 ⇒ 同一份 cookie jar。`None` = 用默认数据存储。
    pub data_store_identifier: Option<[u8; 16]>,
    /// 托管弹窗是否注入官方对话那份指纹脚本（`chat-fingerprint.js`）。
    ///
    /// 弹窗与内容页签是同一族（都是「别人的站点」）：页面探针看到的
    /// `navigator.webdriver` / `__TAURI_*` 在两处必须一致。
    pub fingerprint: bool,
}

/// 内核 `window.open` 弹窗的递增序号（Tauri 的窗口 label 必须唯一）。
static POPUP_SEQ: AtomicU64 = AtomicU64::new(0);

/// 内核 `window.open` 请求的统一策略（**两条**工作台建窗链共用）。
///
/// 内核自己有 `window.open` 的流程：账号登录先 `window.open("about:blank")` 预开
/// 一个标签页、拿到授权地址后再导航它（被拒就退化成「复制链接」）；终端里的链接
/// 也是 `window.open()` 后写 `location`。此前一律 `Deny`，把这两条**内核能力**
/// 削掉了（2026-10-18 分析）。
///
/// 现在：`about:blank` 与内核本机页面 ⇒ 建一扇**壳托管**的临时窗口并返回
/// `NewWindowResponse::Create`，内核的 `popup.location = …` 照常工作；其它
/// `http(s)` 外链维持「交系统浏览器」；其它 scheme 仍拒绝——不在壳里长出无主窗口。
///
/// 托管窗口不注入工作台那套脚本（草稿 / 自愈 / 台灯 / 外观镜像都是工作台专用），
/// 只继承「别人的站点 ⇒ 右键一律取消」那一族策略与标题同步。
pub(crate) fn new_window_response(
    app: &AppHandle,
    link: Url,
    features: NewWindowFeatures,
) -> NewWindowResponse<tauri::Wry> {
    new_window_response_with(app, link, features, PopupPolicy::default())
}

/// 同 [`new_window_response`]，但带一条**处置策略**。
///
/// 官方对话的页签用它：认证域留在壳内 + 托管弹窗与页签共享同一份档案。
pub(crate) fn new_window_response_with(
    app: &AppHandle,
    link: Url,
    features: NewWindowFeatures,
    policy: PopupPolicy,
) -> NewWindowResponse<tauri::Wry> {
    match popup_route(&link, &policy) {
        PopupRoute::Managed => create_popup_window(app, &link, features, &policy),
        PopupRoute::Browser => {
            use tauri_plugin_opener::OpenerExt;
            if let Err(error) = app.opener().open_url(link.to_string(), None::<&str>) {
                eprintln!("dsh-xlink: 无法用系统浏览器打开链接 {link}：{error}");
            }
            NewWindowResponse::Deny
        }
        PopupRoute::Reject => NewWindowResponse::Deny,
    }
}

/// 一条 `window.open` 请求走哪条路。建窗那一步要 `AppHandle`，这条路的选择
/// 是纯函数，单测直接钉住（策略写反的症状是内核登录退化成复制链接 / 终端
/// 链接点了没反应，或者壳里冒出无主窗口）。
#[derive(Debug, PartialEq, Eq)]
enum PopupRoute {
    /// `about:blank` 或内核本机页面：建一扇壳托管的临时窗口。
    Managed,
    /// 外部 `http(s)`：交系统浏览器。
    Browser,
    /// 其它 scheme：拒绝。
    Reject,
}

fn popup_route(link: &Url, policy: &PopupPolicy) -> PopupRoute {
    if link.scheme() == "about" || is_loopback_http(link) {
        PopupRoute::Managed
    } else if policy.keep_login_in_shell && super::official_chat::is_login_domain(link) {
        // 认证域留在壳内：会话必须落回壳自己的浏览器档案。
        PopupRoute::Managed
    } else if matches!(link.scheme(), "http" | "https") {
        PopupRoute::Browser
    } else {
        PopupRoute::Reject
    }
}

fn is_loopback_http(link: &Url) -> bool {
    matches!(link.scheme(), "http" | "https")
        && matches!(link.host_str(), Some("127.0.0.1" | "localhost" | "::1"))
}

fn create_popup_window(
    app: &AppHandle,
    link: &Url,
    features: NewWindowFeatures,
    policy: &PopupPolicy,
) -> NewWindowResponse<tauri::Wry> {
    let label = format!(
        "{}-{}",
        crate::shell::appearance::POPUP_LABEL_PREFIX,
        POPUP_SEQ.fetch_add(1, Ordering::Relaxed) + 1
    );
    let nested = app.clone();
    let nested_policy = policy.clone();
    let mut builder = WebviewWindowBuilder::new(app, &label, WebviewUrl::External(link.clone()))
        .title(crate::shell::window::window_title("工作台弹窗"))
        // 带上 opener 的窗口尺寸 / macOS 同一 webview 配置 / Windows 同一环境
        // （`window_features` 三个平台各自补齐，漏掉 macOS 那一项窗口直接建不出来）。
        .window_features(features)
        // 页面是授权站点 / 内核回调页：外观由页面与系统决定，壳不钉
        // （`initial_theme` 对 `workbench-popup-*` 恒返回 `None`）。
        .theme(crate::shell::appearance::initial_theme(&label))
        // 别人的站点：右键一律取消，与官方对话内容 webview 同族。
        .initialization_script(include_str!("no-context-menu.js"))
        .on_new_window(move |inner, inner_features| {
            new_window_response_with(&nested, inner, inner_features, nested_policy.clone())
        })
        .on_document_title_changed(|window, title| {
            let _ = window.set_title(&title_from_document(&title, "工作台弹窗"));
        });
    // **与发起方同一份浏览器档案**：弹窗里的会话必须落回发起方那一份，
    // 否则认证链在弹窗里跑完、发起方拿不到登录态（Windows 上 user-data
    // 目录不同 = 两套 cookie jar；同一目录的环境配置还必须完全一致，
    // 所以 `additional_browser_args` 要跟着一起给）。
    if let Some(dir) = &policy.profile_dir {
        builder = builder.data_directory(dir.clone());
    }
    if let Some(args) = &policy.browser_args {
        builder = builder.additional_browser_args(args);
    }
    // 指纹脚本：弹窗与内容页签同一族，页面探针看到的两处必须一致。
    if policy.fingerprint {
        builder = builder.initialization_script(include_str!("chat-fingerprint.js"));
    }
    // **macOS：显式绑定同一个 WKWebsiteDataStore 标识符。**
    //
    // 这一条是「弹窗继承发起方档案」在 macOS 上的**唯一**实现方式：
    // WKWebView 不认 `data_directory`（上面那个只对 Windows 的 WebView2 有
    // 效），认的是这个标识符。不绑 ⇒ 弹窗落到默认数据存储 ⇒ 里面**没有**
    // 发起方那份 Google session ⇒ 认证弹窗里照样出不来账号选择器。
    //
    // 另两个平台没有这个概念（Windows 靠上面的 user-data 目录），那边是空操作。
    #[cfg(target_os = "macos")]
    if let Some(identifier) = policy.data_store_identifier {
        builder = builder.data_store_identifier(identifier);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = policy.data_store_identifier;
    let built = builder.build();
    match built {
        Ok(window) => NewWindowResponse::Create { window },
        Err(error) => {
            let line = format!("无法创建内核弹窗（{link}）：{error}");
            eprintln!("dsh-xlink: {line}");
            crate::shell::shell_events::record(HARNESS_WINDOW_LOG, &line);
            NewWindowResponse::Deny
        }
    }
}

/// 把原生窗口的全屏状态同步成 `<html data-fullscreen>`。
///
/// 内核的布局契约里，全屏时浮层起点 / 面板内缩按 `data-fullscreen` 调整（官方
/// Electron 壳由 desktop preload 设置）。它属于「桌面壳负责的窗口状态」，所以由
/// 壳补：监听窗口 `Resized` 事件（macOS 进出全屏会触发）+ 页面加载完成时补一次
/// （重载会清掉文档上的属性），只走 Tauri 的 `eval`，不往页面里再塞脚本。
pub(crate) fn attach_fullscreen_bridge(window: &WebviewWindow) {
    let target = window.clone();
    let last = Arc::new(AtomicBool::new(window.is_fullscreen().unwrap_or(false)));
    window.on_window_event(move |event| {
        if !matches!(event, WindowEvent::Resized(_)) {
            return;
        }
        let fullscreen = target.is_fullscreen().unwrap_or(false);
        if last.swap(fullscreen, Ordering::Relaxed) == fullscreen {
            return;
        }
        let _ = target.eval(fullscreen_script(fullscreen));
    });
}

fn fullscreen_script(fullscreen: bool) -> String {
    format!("document.documentElement.toggleAttribute('data-fullscreen', {fullscreen});")
}

/// 页加载完成时调用：重载会清掉文档上的 `data-fullscreen`，按窗口当前状态补一次。
pub(crate) fn sync_fullscreen_on_load(webview: &WebviewWindow) {
    if let Ok(fullscreen) = webview.is_fullscreen() {
        let _ = webview.eval(fullscreen_script(fullscreen));
    }
}

/// 工作台窗口的 webview 配置。**壳拥有的纪律都在这里**：
///
/// - `on_new_window`：内核自己的 `window.open` 流程（账号登录预开授权标签页、
///   终端链接）走 [`new_window_response`]：`about:blank` 与内核本机页面建壳托管
///   的临时弹窗（`NewWindowResponse::Create`），其它 http(s) 外链交系统浏览器，
///   其余 scheme 拒绝——既保住内核能力，也不在壳内长出无主窗口。
/// - `on_document_title_changed`：内核写 `document.title`（会话名 — 产品名）时
///   同步本窗原生标题栏（官方 Electron 壳的默认行为）。
/// - 七条 `initialization_script`：标题栏品牌条带 / 拉绳小台灯 / 健康自愈探针 /
///   未发送草稿 / 历史会话保护 / 右键兜底（`context-menu-fallback.js`——挂在
///   `window` 的**冒泡**阶段，只在页面没处理时取消默认行为：引擎原生菜单被压掉，
///   内核自绘的菜单照常；左键选词与 Ctrl/⌘+C 复制照旧）/ 外观镜像
///   （`workbench-appearance.js`——按内核声明的 `ui-theme` 偏好设置本窗原生外观，
///   `system` 一档**不覆盖**，见该脚本头）。
/// - `on_page_load`：把加载事件喂给 [`observe`]；加载完成后补一次
///   `data-fullscreen`（重载会清掉文档上的属性）。
fn build(app: &AppHandle, url: &Url) -> Result<(), String> {
    use crate::shell::window::{window_title, CHROME_BACKDROP};
    let popup_app = app.clone();
    let window = WebviewWindowBuilder::new(app, "harness", WebviewUrl::External(url.clone()))
        .title(window_title("工作台"))
        .inner_size(1280.0, 840.0)
        .background_color(CHROME_BACKDROP)
        // 这扇窗**不钉外观**：页面是内核 Web UI，明暗由内核的 `ui-theme` 设置
        // 决定（`system` 一档是内核在页内查 `prefers-color-scheme` 解析的）。
        // 2026-10-07 曾为统一「深色内容 + 浅色标题栏」把它钉成深色，代价是内核
        // 设置里的「跟随系统」永远跟随不了（2026-10-18 用户实测）。
        // `initial_theme(WORKBENCH_LABEL)` 两个平台都返回 `None`；写出来是为了
        // 让这条链自己说清它属于谁。同一条链的另一半在 `commands::open_harness`，
        // 别只改一处。
        .theme(crate::shell::appearance::initial_theme(
            crate::shell::appearance::WORKBENCH_LABEL,
        ))
        .on_new_window(move |link, features| new_window_response(&popup_app, link, features))
        .on_document_title_changed(|window, title| {
            let _ = window.set_title(&title_from_document(&title, "工作台"));
        })
        .on_navigation(move |url| {
            // 观测，不拦截（恒放行）。2026-09-30 的黑屏里页面「无声死掉又自己
            // 重新加载」，壳侧只有 `Started` 一行时刻，看不出它**为什么**加载、
            // 去了哪里。每个导航留一行地址 + 当时的跨壳装包上下文，下次事故
            // 「查看日志」面板里就能把「页面自己动了」和「对面在装包」钉在
            // 同一行上。
            let context = crate::kernel::package_activity::remaining()
                .map(|left| format!("（跨壳装包活动进行中，还剩约 {}s）", left.as_secs()))
                .unwrap_or_default();
            let address: String = url.as_str().chars().take(140).collect();
            crate::shell::shell_events::record(
                HARNESS_WINDOW_LOG,
                &format!("工作台页面导航：{address}{context}"),
            );
            true
        })
        .initialization_script(include_str!("titlebar-pulse.js"))
        .initialization_script(include_str!("pullstring-launcher.js"))
        .initialization_script(include_str!("harness-health.js"))
        .initialization_script(include_str!("harness-draft.js"))
        .initialization_script(include_str!("workbench-history-guard.js"))
        // 把内核声明的外观偏好镜像到本窗的原生外观（`system` 时不覆盖，见脚本头）。
        .initialization_script(include_str!("workbench-appearance.js"))
        // 工作台只压引擎原生菜单、放行内核自绘的菜单；同一条工作台建窗链的
        // 另一半在 `commands.rs::open_harness`，别只改一处。
        .initialization_script(include_str!("context-menu-fallback.js"))
        .on_page_load({
            let handle = app.clone();
            move |webview, payload| {
                let started = payload.event() == PageLoadEvent::Started;
                let state = handle.state::<AppState>();
                observe(&state, started);
                if !started {
                    sync_fullscreen_on_load(&webview);
                }
            }
        })
        .build()
        .map_err(|e| format!("无法创建工作台窗口：{e}"))?;
    attach_fullscreen_bridge(&window);
    Ok(())
}

/// 本模块事件日志的逻辑名（落 `<kind>-harness-window-<date>.log`，出现在
/// 「查看日志」面板的列表里）。
const HARNESS_WINDOW_LOG: &str = "harness-window";

fn reload_window(window: &WebviewWindow) -> Result<(), String> {
    window.reload().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 四条自律逐条钉住。这个看门狗坏掉时的表现是「窗口自己跟自己打架」，
    /// 比「该重载时不重载」严重得多，所以「不该重载」那几条才是重点。
    #[test]
    fn a_loaded_page_is_never_reloaded() {
        // 已经 Finished ⇒ pending_since 是 None ⇒ 永不重载。
        assert!(!should_reload(None, true, 0, LOAD_TIMEOUT));
    }

    #[test]
    fn a_page_still_within_the_timeout_is_left_alone() {
        let start = Instant::now();
        assert!(!should_reload(Some(start), true, 0, LOAD_TIMEOUT));
    }

    #[test]
    fn a_stalled_page_is_reloaded_once_when_the_kernel_is_serving() {
        let long_ago = Instant::now() - LOAD_TIMEOUT - Duration::from_secs(1);
        assert!(should_reload(Some(long_ago), true, 0, LOAD_TIMEOUT));
        // 额度用完就停手：刷新救不回来的故障，重试一万次还是救不回来。
        assert!(!should_reload(
            Some(long_ago),
            true,
            MAX_RELOADS,
            LOAD_TIMEOUT
        ));
    }

    #[test]
    fn a_dead_kernel_is_never_papered_over_by_reloading() {
        let long_ago = Instant::now() - LOAD_TIMEOUT - Duration::from_secs(1);
        assert!(!should_reload(Some(long_ago), false, 0, LOAD_TIMEOUT));
    }

    /// 门槛必须是「慢但正常」与「卡死」之间的分界，不能贴着任何一边。实测首屏
    /// 约 2.5s（bundle 走本地），所以下界取 10s；上界不许太大，否则一个死掉的
    /// 窗口要让用户白等——15s 之后就该动手。
    #[test]
    fn the_timeout_brackets_the_measured_baseline() {
        let measured = Duration::from_millis(2500);
        assert!(
            LOAD_TIMEOUT >= measured * 4,
            "门槛离实测基线太近，机器稍忙就会误判成失败，触发一次毫无必要的重载"
        );
        assert!(
            LOAD_TIMEOUT <= Duration::from_secs(30),
            "门槛太大就失去了兜底的意义：死掉的窗口要让用户白等这么久"
        );
    }

    /// 重建只在「重载额度已用尽 + 页面仍未加载完」时发生，且受「有限次 + 冷却」
    /// 约束。这两条是「窗口自己跟自己打架」的闸：早一步是多余的重建，
    /// 晚一步就是用户眼前那片黑。
    #[test]
    fn a_new_window_comes_only_after_the_reload_budget_is_spent() {
        let long_ago = Instant::now() - LOAD_TIMEOUT - Duration::from_secs(1);
        let fresh = || Instant::now() - REBUILD_COOLDOWN - Duration::from_secs(1);
        // 额度没用完 ⇒ 还在走 reload 那条路，不许重建。
        assert!(!should_recreate(
            Some(long_ago),
            true,
            0,
            0,
            cooldown_ready(None),
            LOAD_TIMEOUT
        ));
        // 额度用完 + 页面仍未加载完 + 冷却已过 ⇒ 换窗口。
        assert!(should_recreate(
            Some(long_ago),
            true,
            MAX_RELOADS,
            0,
            cooldown_ready(None),
            LOAD_TIMEOUT
        ));
        assert!(should_recreate(
            Some(long_ago),
            true,
            MAX_RELOADS,
            MAX_REBUILDS - 1,
            cooldown_ready(Some(fresh())),
            LOAD_TIMEOUT
        ));
        // 预算用尽 ⇒ 绝不再建。
        assert!(!should_recreate(
            Some(long_ago),
            true,
            MAX_RELOADS,
            MAX_REBUILDS,
            cooldown_ready(Some(fresh())),
            LOAD_TIMEOUT
        ));
        // 冷却未过 ⇒ 不建（这是「别自己跟自己打架」的闸）。
        assert!(!should_recreate(
            Some(long_ago),
            true,
            MAX_RELOADS,
            1,
            cooldown_ready(Some(Instant::now())),
            LOAD_TIMEOUT
        ));
        // 页面已经加载完（pending_since 为空）⇒ 没什么要救的。
        assert!(!should_recreate(
            None,
            true,
            MAX_RELOADS,
            0,
            cooldown_ready(None),
            LOAD_TIMEOUT
        ));
        // 内核挂了 ⇒ 该重启内核，重建窗口只会掩盖它。
        assert!(!should_recreate(
            Some(long_ago),
            false,
            MAX_RELOADS,
            0,
            cooldown_ready(None),
            LOAD_TIMEOUT
        ));
    }

    /// 冷却的纯函数钉住：没建过 ⇒ 就绪；刚建过 ⇒ 未就绪；过完冷却 ⇒ 又就绪。
    /// 2026-09-30 之前这里是「每进程一次」，那次真机事故里它把用户晾在黑屏上
    /// 25 秒——「一次」换来的安全感是假的，风暴过后额度必须还在。
    #[test]
    fn the_rebuild_cooldown_elapses_and_then_the_budget_is_available_again() {
        assert!(cooldown_ready(None), "第一次重建没有冷却问题");
        assert!(!cooldown_ready(Some(Instant::now())), "刚建完必须冷却");
        let cooled = Instant::now() - REBUILD_COOLDOWN - Duration::from_secs(1);
        assert!(cooldown_ready(Some(cooled)), "冷却过了就该再有额度");
    }

    /// 判据必须与页面自愈的额度对齐：`slot-assembly` 是第一次（页面会自己刷新，
    /// 壳不许插手），`runtime-error` + 槽位短语才是「已经刷过一次、没刷好」。
    /// 自己抄一份短语表只会与 `harness-health.js`、`kernel_evidence` 三处漂移。
    #[test]
    fn only_a_repeated_slot_failure_asks_for_a_new_window() {
        let phrase = crate::kernel::kernel_evidence::SLOT_PHRASES[0];
        let message = format!("Uncaught Error: scope 'session-maybe' {phrase}");
        // 第一次：页面自己会刷新，壳不重建。
        assert!(!fault_needs_new_window("slot-assembly", &message, ""));
        // 额度用掉之后：正是要换窗口的时候。
        assert!(fault_needs_new_window("runtime-error", &message, ""));
        // 与槽位无关的前端异常不该触发换窗口（换掉也救不回来，只会闪）。
        assert!(!fault_needs_new_window(
            "runtime-error",
            "Uncaught TypeError: x is not a function",
            ""
        ));
        // 空白页有自己的归因与处置，不走这一级。
        assert!(!fault_needs_new_window("blank", &message, ""));
    }

    /// 这一层曾经**算了却没人调**——`fault_needs_new_window` 只有测试引用它，
    /// 生产代码里一处都没有。于是自动链的末端是「弹个事故面板，然后停在黑屏上」，
    /// 而手动「刷新工作台」（同一个 `recreate`）证明换窗口能救回来。**判据是对的，
    /// 是没接上**——所以这里钉的是「什么时候该换」，以及内核没在跑时**不该**换。
    #[test]
    fn a_repeated_slot_failure_while_serving_asks_for_a_new_window() {
        let phrase = crate::kernel::kernel_evidence::SLOT_PHRASES[0];
        let message = format!("Uncaught Error: scope 'session-maybe' {phrase}");
        assert!(should_recreate_after_fault(
            "runtime-error",
            &message,
            "",
            true
        ));
        // 内核真的没在跑时换窗口，只是把同一个连接失败重新渲染一遍。
        assert!(!should_recreate_after_fault(
            "runtime-error",
            &message,
            "",
            false
        ));
        // 第一次撞上（页面会自己刷新）仍然不换——否则壳与页面各刷一次。
        assert!(!should_recreate_after_fault(
            "slot-assembly",
            &message,
            "",
            true
        ));
    }

    /// 重建的**成败都要留痕**，而且成功那条只能在建窗成功之后写。
    ///
    /// 2026-10-18 实机：`harness-window.log` 写着「已拆掉并重建」，此后 20 分钟
    /// 没有任何一次页面加载；建窗失败只进了 Windows 上没有去处的 stderr。用户
    /// 手里没有工作台，日志里却查不到原因——这条失败必须在「查看日志」里看得见。
    #[test]
    fn recreate_success_and_failure_both_carry_the_facts() {
        let success = recreate_success_message("刷新后页面仍报同一处槽位故障", 2);
        assert!(success.contains("已拆掉并重建"), "成功要说结果：{success}");
        assert!(success.contains("第 2 次"), "成功要带次数：{success}");
        assert!(
            success.contains(&format!("上限 {MAX_REBUILDS} 次")),
            "成功要带预算，用户才知道还剩几次：{success}"
        );

        let failure = recreate_failure_message(
            "刷新后页面仍报同一处槽位故障",
            "无法创建工作台窗口：窗口已存在",
        );
        assert!(failure.contains("重建失败"), "失败必须明说结果：{failure}");
        assert!(failure.contains("窗口已存在"), "失败必须带原因：{failure}");
        assert!(
            failure.contains("「工作台窗口」重新打开"),
            "失败必须给下一步：{failure}"
        );
    }

    /// 内核 `window.open` 的三条路（2026-10-18 恢复内核能力时定的策略）：
    /// 登录预开窗 / 终端链接必须走壳托管的窗口，外部链接交浏览器，
    /// 其它 scheme 不许在壳里长出窗口。
    #[test]
    fn window_open_requests_are_routed_by_scheme_and_host() {
        let parse = |raw: &str| Url::parse(raw).expect("测试 URL 必须合法");
        let default_policy = PopupPolicy::default();
        assert_eq!(
            popup_route(&parse("about:blank"), &default_policy),
            PopupRoute::Managed
        );
        assert_eq!(
            popup_route(&parse("http://127.0.0.1:3091/?token=abc"), &default_policy),
            PopupRoute::Managed
        );
        assert_eq!(
            popup_route(&parse("http://localhost:3091/callback"), &default_policy),
            PopupRoute::Managed
        );
        assert_eq!(
            popup_route(&parse("https://chat.deepseek.com/"), &default_policy),
            PopupRoute::Browser
        );
        assert_eq!(
            popup_route(&parse("http://127.0.0.1.evil.example/"), &default_policy),
            PopupRoute::Browser
        );
        assert_eq!(
            popup_route(&parse("file:///etc/passwd"), &default_policy),
            PopupRoute::Reject
        );
        assert_eq!(
            popup_route(&parse("data:text/html,<b>x</b>"), &default_policy),
            PopupRoute::Reject
        );
        assert_eq!(
            popup_route(&parse("javascript:void(0)"), &default_policy),
            PopupRoute::Reject
        );
    }

    /// **认证域留在壳内**（官方对话页签的处置策略）。
    ///
    /// 写反的症状是「静默」的：认证链在系统浏览器里跑完，壳内的 webview
    /// 拿不到登录态 ⇒ 用户看到「登录做完了但页签还是未登录」，壳侧无日志。
    /// 所以两头都要钉：认证域**必须**留在壳内；普通外链**仍然**交系统
    /// 浏览器（不能因为加了这条就把所有外链都劫持进壳内弹窗）。
    #[test]
    fn login_domains_stay_in_shell_while_ordinary_links_go_to_the_browser() {
        let parse = |raw: &str| Url::parse(raw).expect("测试 URL 必须合法");
        let policy = PopupPolicy {
            keep_login_in_shell: true,
            ..PopupPolicy::default()
        };

        // 认证域：留在壳内（会话必须落回壳自己的浏览器档案）
        for url in [
            "https://accounts.google.com/v3/signin/identifier?client_id=x",
            "https://auth.openai.com/api/accounts/callback/google",
            "https://appleid.apple.com/auth/authorize",
            "https://login.microsoftonline.com/common/oauth2/authorize",
        ] {
            assert_eq!(
                popup_route(&parse(url), &policy),
                PopupRoute::Managed,
                "{url} 是认证域，必须留在壳内"
            );
        }

        // 普通外链：仍然交系统浏览器（现状不变）
        for url in [
            "https://chat.deepseek.com/",
            "https://help.openai.com/zh-hans-cn/",
            "https://example.com/",
        ] {
            assert_eq!(
                popup_route(&parse(url), &policy),
                PopupRoute::Browser,
                "{url} 是普通外链，不该被劫持进壳内弹窗"
            );
        }

        // 其它 scheme 仍然拒绝（认证域豁免不许放宽到非 http(s)）
        assert_eq!(
            popup_route(&parse("file:///etc/passwd"), &policy),
            PopupRoute::Reject
        );
    }

    /// 默认策略（工作台那条链）**不**把认证域留在壳内。
    ///
    /// 工作台的 `window.open` 是内核自己的流程（预开 `about:blank` 再导航），
    /// 它的外链处置是另一回事；判据要钉住「官方对话那条链开了豁免，
    /// 不等于所有链都开了」。
    #[test]
    fn the_default_policy_does_not_keep_login_domains_in_shell() {
        let parse = |raw: &str| Url::parse(raw).expect("测试 URL 必须合法");
        assert_eq!(
            popup_route(
                &parse("https://accounts.google.com/v3/signin/identifier"),
                &PopupPolicy::default()
            ),
            PopupRoute::Browser
        );
    }

    /// 内核文档标题同步：空标题退回壳的名字，非空按「名字 — 应用名」约定。
    #[test]
    fn document_titles_are_synced_with_the_shell_naming_convention() {
        assert_eq!(
            title_from_document("   ", "工作台"),
            crate::shell::window::window_title("工作台")
        );
        assert_eq!(
            title_from_document(" 会话 — DeepSeek Harness ", "工作台"),
            crate::shell::window::window_title("会话 — DeepSeek Harness")
        );
    }
}
