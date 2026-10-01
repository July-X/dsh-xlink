//! 工作台窗口的**手动**逃生口（`harness_window.rs` 的 Tauri 面）。
//!
//! ## 为什么自动自愈还不够
//!
//! `harness_window` 那条链条（卡住 15s → reload → 换窗口）只在**加载事件**
//! 上做判据：开始加载之后没等到 `Finished` 才算没起来。2026-09-30 实测的黑屏
//! 有一类整个不在这个判据的覆盖范围内——WebView2 渲染进程被装包事件风暴打崩，
//! 页面**早就 Finished 过了**，之后渲染进程死掉、文档变黑。此时 `pending_since`
//! 是 `None`，看门狗永远不会动手，而用户看得见的现象与「页面没加载出来」一模一样。
//!
//! 自动链条还有两道闸是为「不要自己跟自己打架」设的：重载上限（[`MAX_RELOADS`]）
//! 与「每进程只重建一次」。这两道闸在自动场景里是对的，在**用户手动**按下按钮的
//! 场景里恰好挡住了唯一能救他的动作——所以手动路径先 [`reset_budget`] 再重建。
//!
//! ## 为什么不放 commands.rs
//!
//! `commands.rs` 是代码预算里的反棘轮文件（只许下调），而这条命令与它其余 70 多条
//! 命令没有任何共享逻辑：它把「该不该动手」的判据、额度清零、重建三件事串起来，
//! 关心的只有工作台窗口。留在 `commands.rs` 只能靠上调数字过门禁，而调数字是
//! AGENTS.md 明确禁止的反应。`bisect_cmd.rs` / `home_recovery_cmd.rs` 是同一处理由。
//!
//! 判据与文案抽成纯函数 [`refuse_reason`]：三条拒绝路径的**说法**是用户唯一能
//! 拿到的东西（窗口没了就是没了，不给下一步等于让用户猜），值得用测试钉住。

use tauri::{AppHandle, Manager};

use crate::commands::AppState;

/// 「现在该不该动手」以及不该动手时**怎么跟用户说**。抽成纯函数是为了让三条
/// 拒绝路径的说法可测——真正建窗那部分要 `AppHandle`，测不起。
///
/// 两条拒绝各有各的理由，别混：
///
/// - **内核没在跑**：`reload` 与重建都只是把同一个连接失败重新渲染一遍。掩盖它
///   比不重载更糟（`should_reload` 也是这么拒的），该显示的是启动失败。
/// - **窗口从没打开过**：`harness_url` 是壳为「同一个页面再来一次」记下的地址，
///   没有它就没有可重建的对象。这时候该让用户去按「工作台窗口」，而不是静默
///   建出一个地址未知的窗口。
fn refuse_reason(port: u16, port_open: bool, loaded_url: Option<&str>) -> Option<String> {
    if !port_open {
        return Some(format!(
            "内核未在运行（端口 {port}），刷新只会重新加载同一个连接失败的页面。\
             请先点击「工作台」启动内核，再点「刷新工作台」"
        ));
    }
    if loaded_url.is_none() {
        return Some(
            "工作台窗口还没有打开过，没有可重载的对象。请先点击「工作台窗口」把它打开".into(),
        );
    }
    None
}

/// 用户点「刷新工作台」：换掉整个工作台窗口（**新 webview = 新渲染进程**）。
///
/// 按钮文案是「刷新工作台」，机制是强制重载——两者说的是两件事：文案说用户得到
/// 什么（一个重新加载好的工作台），这里说壳到底做了什么（换掉整个窗口）。
///
/// 为什么不是 `reload()`：2026-09-30 的黑屏实测里，渲染进程被打崩后 `reload()`
/// 落在同一块死掉的文档上，用户看到的仍是一片黑（用户原话：「reload 后，还是会
/// 黑屏」）。换掉整个窗口是壳侧唯一能换掉渲染进程的动作，所以手动这一档直接
/// 上到重建，不做「先试刷新」——按钮必须可预期：按下去就是换一个新的。
///
/// 代价要说清：工作台窗口会关掉再打开，页面里的滚动位置、侧栏面板、终端标签等
/// 前端运行时状态会丢（会话在服务端，不受影响）。按钮的 title 里写了这一点。
#[tauri::command]
pub async fn harness_force_reload(app: AppHandle) -> Result<(), String> {
    crate::commands::blocking(move || {
        let state = app.state::<AppState>();
        // 与 `open_harness` 同一把生命周期锁：建窗 / 停内核 / 换实例并发时，
        // 两个线程各自建出一个 "harness" 窗口是更糟的结果。
        let _lifecycle_guard = crate::lock(&state.lifecycle);
        let settings =
            crate::shell::settings::load_for_shell(crate::shell::settings::current_mode());
        let loaded = crate::lock(&state.harness_url).clone();
        if let Some(reason) = refuse_reason(
            settings.port,
            crate::kernel::lifecycle::port_open(settings.port),
            loaded.as_deref(),
        ) {
            return Err(reason);
        }
        crate::harness::harness_window::reset_budget(&state);
        crate::harness::harness_window::recreate(&app, "被用户手动刷新")
    })
    .await
}

/// 页面自愈刷新前问一句「现在该不该等」：返回建议退避的毫秒数（0 = 立刻刷）。
///
/// 调它的是 `harness-health.js` 的槽位自愈路径。2026-09-30 实测：那次 3 秒自愈
/// 刷新落在对面卸载内核的风暴中间，新加载的页面几秒后又撞死一次——刷新本身
/// 没错，错在**落点**。信标让页面知道「风还没停」；等风停再刷，一次就能成。
#[tauri::command]
pub fn harness_reload_backoff() -> u64 {
    crate::kernel::package_activity::recovery_backoff().as_millis() as u64
}

/// 页面把「输入框里还没发出去的东西」交给壳：一段文字，外加还没发出去的那几张图。
///
/// 调它的是注入脚本 `harness-draft.js`，时机是**用户停止输入一会儿之后**而不是
/// 页面卸载时——卸载那一刻再发起 IPC，多半等不到回程（页面已经没了）。见
/// [`crate::harness::harness_draft`] 模块开头对「为什么经过壳而不是只放 sessionStorage」的
/// 说明。
#[tauri::command]
pub fn stash_harness_draft(
    href: String,
    text: String,
    images: Option<Vec<crate::harness::harness_media::ImageInput>>,
) {
    let (family, id) = crate::shell::instance::resolve_default();
    crate::harness::harness_draft::stash(
        family,
        id,
        &href,
        &text,
        images.as_deref().unwrap_or(&[]),
    );
}

/// 页面报告「输入框空了」：把盘上那份草稿删掉。
///
/// 绝大多数时候那句话是**已经发出去**的（发送时编辑器被程序化清空，不派发
/// `input`，页面自己派 `clear` 不现实）。不清的后果用户已经说过一次：下次打开
/// 工作台，那条已发送的消息会自己坐回输入框。取值形态与「没在输入」时调用
/// `stash_harness_draft` 相同——但**必须独立成一条命令**：清盘的理由不在文本里，
/// 而在「本页曾经存过、现在没了」这个状态上。
#[tauri::command]
pub fn clear_harness_draft() {
    let (family, id) = crate::shell::instance::resolve_default();
    crate::harness::harness_draft::clear(family, id);
}

/// 页面起来后取走草稿（**读走即删**；图片以 base64 内联在返回值里）。
///
/// 调用方必须**先确认页面上有可写的输入框**再调它：取走即删是「草稿不变成垃圾」
/// 与「草稿不变成惊吓」的分界，而页面侧找不到输入框的那次调用会把草稿吞掉。
#[tauri::command]
pub fn take_harness_draft() -> Option<crate::harness::harness_draft::RestoredDraft> {
    let (family, id) = crate::shell::instance::resolve_default();
    crate::harness::harness_draft::take(family, id)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 两条拒绝路径各自说清「为什么不做」与「接下来按哪里」：「刷新工作台」是用户
    /// 在故障里唯一的手动出路，把它变成一句「操作失败」等于没有出路。
    #[test]
    fn a_force_reload_is_refused_with_a_next_step() {
        let dead = refuse_reason(3090, false, Some("http://127.0.0.1:3090/?token=x")).unwrap();
        assert!(dead.contains("3090"), "要说清是哪个端口：{dead}");
        assert!(dead.contains("「工作台」"), "要给出去向：{dead}");

        let never = refuse_reason(3090, true, None).unwrap();
        assert!(never.contains("「工作台窗口」"), "要给出去向：{never}");

        // 内核在跑 + 窗口开过 ⇒ 放行。
        assert!(refuse_reason(3090, true, Some("http://127.0.0.1:3090/?token=x")).is_none());
    }

    /// 内核没在跑时**先**说内核：两个拒绝同时成立时，端口那句才是用户当下唯一
    /// 能解决的那一条（窗口没开是因为它压根没开过）。
    #[test]
    fn a_dead_kernel_outranks_the_missing_window() {
        let reason = refuse_reason(3091, false, None).unwrap();
        assert!(reason.contains("内核未在运行"), "顺序不能反：{reason}");
    }
}
