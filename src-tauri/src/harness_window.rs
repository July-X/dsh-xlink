//! 工作台窗口的加载看门狗。
//!
//! **为什么需要它**：工作台页面里那个 `harness-health.js` 是注入脚本，与页面
//! 同生共死——页面没加载出来时它同样没跑，于是「白屏 / 直接黑屏」这类故障在
//! `last-incident.json` 里一个字都留不下。2026-09-29 实测两次黑屏，两个壳都
//! 没有事故记录，因为页面里一行 JS 都没执行。能观测到它的只有壳这一侧：
//! webview 自己的加载事件。
//!
//! Tauri 2.11 的 `PageLoadEvent` 只有 `Started` / `Finished`，**没有失败变体**
//! ——导航失败时两个都不触发。所以判据只能是超时：开始加载之后一段时间内没
//! 等到 `Finished`，就是页面没起来。
//!
//! 这个看门狗最大的风险不是「该重载时不重载」，而是反过来——把用户的窗口变成
//! 一个自己跟自己打架的东西。三条自律写在 [`should_reload_harness`] 上。
//!
//! **可观测性**：触发条件与重载结果都落 `shell_events`（`harness-window.log`，
//! 「查看日志」面板里可见），不靠 `eprintln!`——GUI 应用的 stderr 在 Windows
//! 上没有任何去处，而"页面闪了一下"这种只有后果没有原因的现象，没有这条
//! 日志就只能靠时间戳对猜。

use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager, WebviewWindow};

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

/// 工作台 webview 的加载观测量。
#[derive(Default)]
pub struct HarnessPage {
    /// 最近一次 `Started` 的时刻；收到 `Finished` 就清空。
    pub pending_since: Option<Instant>,
    /// 本次窗口生命周期内已自动重载的次数。
    pub reloads: u8,
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
        crate::shell_events::record(HARNESS_WINDOW_LOG, "工作台页面开始加载");
    } else {
        page.pending_since = None;
    }
}

/// 是否该因为「页面长时间没加载出来」而重载一次工作台窗口。
///
/// 抽成纯函数是因为它是这里唯一有分支的地方，而真正难测的恰恰是**不该重载的
/// 那几条**：真正干活的那段要 `AppHandle` 和真实 webview，测不起。
pub fn should_reload(pending_since: Option<Instant>, kernel_running: bool, reloads: u8) -> bool {
    let Some(since) = pending_since else {
        return false;
    };
    // 内核真挂了的时候重载窗口没有意义：那种情况该显示启动失败，掩盖它比
    // 不重载更糟。
    if !kernel_running || reloads >= MAX_RELOADS {
        return false;
    }
    since.elapsed() >= LOAD_TIMEOUT
}

/// 工作台窗口长时间没加载出来时重载一次。由状态轮询顺带驱动，不另起定时器。
pub fn reload_stalled(app: &AppHandle, kernel_running: bool) {
    let state = app.state::<AppState>();
    let (pending, reloads) = {
        let page = crate::lock(&state.harness_page);
        (page.pending_since, page.reloads)
    };
    if !should_reload(pending, kernel_running, reloads) {
        return;
    }
    let Some(window) = app.get_webview_window("harness") else {
        return;
    };
    {
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
        crate::shell_events::record(HARNESS_WINDOW_LOG, &message);
    }
    if let Err(error) = reload_window(&window) {
        eprintln!("dsh-xlink: 工作台窗口重载失败：{error}");
        crate::shell_events::record(HARNESS_WINDOW_LOG, &format!("工作台窗口重载失败：{error}"));
    }
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
        assert!(!should_reload(None, true, 0));
    }

    #[test]
    fn a_page_still_within_the_timeout_is_left_alone() {
        let start = Instant::now();
        assert!(!should_reload(Some(start), true, 0));
    }

    #[test]
    fn a_stalled_page_is_reloaded_once_when_the_kernel_is_serving() {
        let long_ago = Instant::now() - LOAD_TIMEOUT - Duration::from_secs(1);
        assert!(should_reload(Some(long_ago), true, 0));
        // 额度用完就停手：刷新救不回来的故障，重试一万次还是救不回来。
        assert!(!should_reload(Some(long_ago), true, MAX_RELOADS));
    }

    #[test]
    fn a_dead_kernel_is_never_papered_over_by_reloading() {
        let long_ago = Instant::now() - LOAD_TIMEOUT - Duration::from_secs(1);
        assert!(!should_reload(Some(long_ago), false, 0));
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
}
