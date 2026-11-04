//! 锁屏查询失败也按私密处理；应用焦点不能代表屏幕锁定状态。
use std::{
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::Duration,
};
static MONITORING: AtomicBool = AtomicBool::new(false);
use tauri::AppHandle;
pub(crate) fn unlocked() -> bool {
    screen_unlocked() == Some(true)
}
pub(crate) fn can_disclose() -> bool {
    MONITORING.load(Ordering::Acquire) && unlocked()
}
pub(crate) fn body(normal: &str, state: Option<bool>) -> &str {
    if state == Some(true) {
        normal
    } else {
        "任务已完成"
    }
}
pub(crate) fn serve(app: &AppHandle) {
    let app = app.clone();
    let result = thread::Builder::new()
        .name("dsh-notification-privacy".into())
        .spawn(move || {
            let mut previous_error = None;
            loop {
                if !unlocked() {
                    let error = super::platform::remove_sensitive(&app).err();
                    if error != previous_error {
                        if let Some(error) = &error {
                            crate::shell::shell_events::record("notification-privacy", error);
                            super::task::remember_toast_error(
                                &app,
                                Err(format!(
                                    "锁屏通知清理失败：{error}；请检查系统通知权限或查看通知日志"
                                )),
                            );
                        }
                        previous_error = error;
                    }
                }
                super::navigation::resume(&app);
                thread::sleep(Duration::from_millis(500));
            }
        });
    MONITORING.store(result.is_ok(), Ordering::Release);
    if let Err(error) = result {
        crate::shell::shell_events::record(
            "notification-privacy",
            &format!("无法启动隐私监视器：{error}"),
        );
    }
}
#[cfg(target_os = "macos")]
fn screen_unlocked() -> Option<bool> {
    use objc2_foundation::NSString;
    use std::ffi::c_void;
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGSessionCopyCurrentDictionary() -> *const c_void;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFDictionaryGetValue(dict: *const c_void, key: *const c_void) -> *const c_void;
        fn CFBooleanGetValue(value: *const c_void) -> u8;
        fn CFBooleanGetTypeID() -> usize;
        fn CFGetTypeID(value: *const c_void) -> usize;
        fn CFRelease(value: *const c_void);
    }
    // SAFETY: Copy 返回自有 CFDictionary；仅在类型确为 CFBoolean 时读值，结束时释放。
    unsafe {
        let dict = CGSessionCopyCurrentDictionary();
        if dict.is_null() {
            return None;
        }
        let key = NSString::from_str("CGSSessionScreenIsLocked");
        let value = CFDictionaryGetValue(dict, objc2::rc::Retained::as_ptr(&key).cast());
        let result = if !value.is_null() && CFGetTypeID(value) == CFBooleanGetTypeID() {
            Some(CFBooleanGetValue(value) == 0)
        } else if value.is_null() {
            // macOS 解锁会话不含锁定位；只在有效的本机已登录会话中接受该形态。
            let mut flags = Vec::new();
            for name in ["kCGSSessionOnConsoleKey", "kCGSessionLoginDoneKey"] {
                let key = NSString::from_str(name);
                let flag = CFDictionaryGetValue(dict, objc2::rc::Retained::as_ptr(&key).cast());
                flags.push(
                    if !flag.is_null() && CFGetTypeID(flag) == CFBooleanGetTypeID() {
                        Some(CFBooleanGetValue(flag) != 0)
                    } else {
                        None
                    },
                );
            }
            if flags.iter().all(|flag| *flag == Some(true)) {
                Some(true)
            } else {
                None
            }
        } else {
            None
        };
        CFRelease(dict);
        result
    }
}
#[cfg(target_os = "windows")]
fn screen_unlocked() -> Option<bool> {
    use windows_sys::Win32::System::RemoteDesktop::*;
    // SAFETY: WTS 分配的只读缓冲区先检查长度与层级，再按官方布局读取；始终释放。
    unsafe {
        let mut buffer = std::ptr::null_mut();
        let mut bytes = 0;
        if WTSQuerySessionInformationW(
            WTS_CURRENT_SERVER_HANDLE,
            WTS_CURRENT_SESSION,
            WTSSessionInfoEx,
            &mut buffer,
            &mut bytes,
        ) == 0
        {
            return None;
        }
        let result = if !buffer.is_null() && bytes as usize >= std::mem::size_of::<WTSINFOEXW>() {
            let info = &*buffer.cast::<WTSINFOEXW>();
            if info.Level == 1 {
                let state = info.Data.WTSInfoExLevel1;
                match state.SessionFlags as u32 {
                    WTS_SESSIONSTATE_UNLOCK => Some(state.SessionState == WTSActive),
                    WTS_SESSIONSTATE_LOCK => Some(false),
                    _ => None,
                }
            } else {
                None
            }
        } else {
            None
        };
        WTSFreeMemory(buffer.cast());
        result
    }
}
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn screen_unlocked() -> Option<bool> {
    None
}
#[cfg(test)]
mod tests {
    #[test]
    fn locked_or_unknown_payload_never_contains_sensitive_content() {
        for state in [Some(false), None] {
            assert_eq!(
                super::body("private title / question / project", state),
                "任务已完成"
            );
        }
        assert_eq!(super::body("normal", Some(true)), "normal");
    }
}
