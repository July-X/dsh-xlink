//! 系统通知通道的可用性判定：**这个进程事实上能不能把通知气泡投递出去**，以及
//! 不能的话用户下一步该做什么。
//!
//! 与 [`crate::notify::task`] 的分工：那边管状态机（谁完成了、记不记未读、弹不弹角标），
//! 这里只回答"平台这一关有没有卡住"。两端各卡一处，而且**都是静默失败**——不报
//! 任何错，用户看到的只是"角标变了、气泡没出现"：
//!
//! - **macOS**：系统通知按应用 bundle 归属。`tauri dev` / `cargo run` 跑的是
//!   `target/debug/dsh-xlink` 这个裸可执行文件，系统找不到 bundle——实测
//!   `usernoted` 会把请求挂到父进程（Terminal）名下、`NotificationCenter` 记
//!   `Unable to find valid bundle`，通知因此不会以 dsh-xlink 的名义出现。这是平台
//!   约束，壳无法绕过，只能如实告诉用户"用安装版验证"。
//! - **Windows**：系统通知总开关（「设置 → 系统 → 通知」）关着时，**所有**应用的
//!   toast 都被系统丢弃，而 `ToastNotifier::Show` 依然返回 `S_OK`。本机实测：总
//!   开关关闭时 `ToastNotifier::Setting()` 对任意 AUMID 都返回 `DisabledForUser`——
//!   NVIDIA、Defender、PowerShell、本应用无一例外，同时
//!   `HKCU\Software\Microsoft\Windows\CurrentVersion\PushNotifications\ToastEnabled`
//!   = 0。这里问的是**同一个问题的权威接口**，不是去猜注册表：它一次覆盖
//!   "用户关了总开关""用户单独关掉了本应用""组策略禁用"三种情形。
//!
//! **角标不受影响**：它走 `ITaskbarList3::SetOverlayIcon`（任务栏 API），与通知
//! 平台毫无关系。"只有数字红点、没有横幅"正是这个原因——两者从来不是同一套东西，
//! 排查时别把它们当成一个功能的两半。

/// macOS 上未打包构建的说明（`blocked()` 为 `true` 时展示）。
pub const MACOS_UNPACKAGED: &str = "当前是未打包的开发构建：macOS 按应用 bundle 归属系统通知，\
     找不到 bundle 时通知不会以 dsh-xlink 的名义投递（角标仍然正常）。要验证通知气泡，\
     请用安装版，或先执行 `npm run build -- --debug` 再运行 \
     `src-tauri/target/debug/bundle/macos/dsh-xlink.app`。";

/// Windows 上系统通知被关掉时的说明。
///
/// 特意点明"角标不受影响"：用户看到红点在变、却看不到横幅，第一反应是"通知坏了"，
/// 而真实原因是系统那一关没开——不说清楚就会去反复重装插件、重启内核。
pub const WINDOWS_DISABLED: &str = "Windows 的系统通知当前被系统关闭，所有应用的通知气泡都会被丢弃\
     （角标走的是任务栏，不受影响，所以只看到数字红点）。请打开「设置 → 系统 → 通知」里的\
     通知开关；打开后这条说明会自动消失。";

/// 本应用在 Windows 上的 AppUserModelID——toast 按它归属到本应用。
///
/// [`blocked`] 必须用**同一个** ID 去问：问的是"本应用的权限"，拿别的 AUMID
/// （例如 PowerShell 的）去问，答的就不是这件事。
#[cfg(target_os = "windows")]
pub fn app_id(app: &tauri::AppHandle) -> &str {
    app.config().identifier.as_str()
}

/// 系统通知当前事实上投递不出去时返回 `true`。
///
/// 语义是"**事实上**"，不是"权限状态"：桌面壳用 `notify-rust` 的
/// `NSUserNotificationCenter` backend，不经过 `UNUserNotificationCenter` 的授权，
/// 所以 macOS「系统设置 → 通知」里看到的 dsh-xlink 永远是无权限状态——在那里勾上
/// 也没用。把"没有授权"报成"投递不了"是误诊。
/// `app` 只有 Windows 分支用得上（要拿本应用的 AppUserModelID 去问系统），
/// macOS 上按 `current_exe()` 判 bundle 就够了——不 `allow` 的话，macOS 的
/// `cargo clippy -- -D warnings` 会以 `unused variable: app` 挡住发布
/// （desktop-v0.3.5-rc.1 第一次 Quality gates 就是这么挂的）。
pub fn blocked(
    #[cfg_attr(not(target_os = "windows"), allow(unused_variables))] app: &tauri::AppHandle,
) -> bool {
    #[cfg(target_os = "macos")]
    if !in_app_bundle() {
        return true;
    }
    #[cfg(target_os = "windows")]
    return windows_disabled(app);
    #[allow(unreachable_code)]
    false
}

/// 阻塞时的用户指引；`blocked()` 为 `false` 时返回 `None`。
pub fn note(app: &tauri::AppHandle) -> Option<String> {
    if !blocked(app) {
        return None;
    }
    #[cfg(target_os = "macos")]
    return Some(MACOS_UNPACKAGED.into());
    #[cfg(target_os = "windows")]
    return Some(WINDOWS_DISABLED.into());
    #[allow(unreachable_code)]
    None
}

/// macOS：跑的是不是 `.app` bundle 里的可执行文件。
#[cfg(target_os = "macos")]
fn in_app_bundle() -> bool {
    std::env::current_exe()
        .map(|path| path.to_string_lossy().contains(".app/Contents/MacOS/"))
        .unwrap_or(false)
}

/// Windows：`ToastNotifier::Setting()` 是不是「非放行」。
///
/// 问不到（老系统上 API 不可用、进程被策略挡住）一律按"没被关"处理：宁可少显示
/// 一条说明，也不谎报"你的通知被关了"——后者会让用户去翻一个根本关着的开关。
#[cfg(target_os = "windows")]
fn windows_disabled(app: &tauri::AppHandle) -> bool {
    use windows::UI::Notifications::ToastNotificationManager;

    let Ok(notifier) = ToastNotificationManager::CreateToastNotifierWithId(
        &windows::core::HSTRING::from(app_id(app)),
    ) else {
        return false;
    };
    match notifier.Setting() {
        Ok(setting) => blocked_by(setting),
        Err(_) => false,
    }
}

/// 设置值是否意味着"投递不出去"。**只把 [`NotificationSetting::Enabled`] 当放行**：
/// 其余取值（用户关了总开关 / 只关了本应用 / 组策略 / 应用清单）一律按阻塞处理，
/// 因为它们的效果一样——`Show` 返回成功而气泡不出现。
#[cfg(target_os = "windows")]
fn blocked_by(setting: windows::UI::Notifications::NotificationSetting) -> bool {
    use windows::UI::Notifications::NotificationSetting;
    setting != NotificationSetting::Enabled
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::blocked_by;
    use windows::UI::Notifications::NotificationSetting;

    /// 只有 `Enabled` 放行。本机实测（通知总开关关闭时）：任意 AUMID 都拿到
    /// `DisabledForUser`——包括本应用、`Microsoft.Windows.PowerShell`、
    /// `Windows.Defender.SecurityCenter` 与 `com.nvidia.nvapp`。
    #[test]
    fn only_enabled_setting_is_allowed() {
        assert!(!blocked_by(NotificationSetting::Enabled));
        assert!(blocked_by(NotificationSetting::DisabledForUser));
        assert!(blocked_by(NotificationSetting::DisabledForApplication));
        assert!(blocked_by(NotificationSetting::DisabledByGroupPolicy));
        assert!(blocked_by(NotificationSetting::DisabledByManifest));
    }
}
