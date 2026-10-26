//! 托盘退出的应答与原生兜底；错误页面和未挂载的 SPA 也必须能退出。

use std::sync::Mutex;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, WebviewWindow};

use super::{resident::MAIN_WINDOW, shell_events};

const RESPONSE_TIMEOUT: Duration = Duration::from_secs(2);
static REQUESTS: Mutex<QuitRequests> = Mutex::new(QuitRequests::new());

struct QuitRequests {
    next_id: u64,
    pending: Option<(u64, bool)>, // id, 是否已交给原生确认
}

impl QuitRequests {
    const fn new() -> Self {
        Self {
            next_id: 0,
            pending: None,
        }
    }

    fn begin(&mut self) -> Option<u64> {
        if self.pending.is_some() {
            return None;
        }
        self.next_id += 1;
        self.pending = Some((self.next_id, false));
        Some(self.next_id)
    }

    fn acknowledge(&mut self, id: u64) -> bool {
        if self.pending != Some((id, false)) {
            return false;
        }
        self.pending = None;
        true
    }

    fn claim_fallback(&mut self, id: u64) -> bool {
        if self.pending != Some((id, false)) {
            return false;
        }
        self.pending = Some((id, true));
        true
    }

    fn finish(&mut self, id: u64) {
        if self.pending.is_some_and(|(pending, _)| pending == id) {
            self.pending = None;
        }
    }
}

/// 只接受主面板对当前请求的应答；超时后到达的应答不能再弹第二个确认框。
#[tauri::command]
pub fn acknowledge_quit_request(window: WebviewWindow, request_id: u64) -> bool {
    window.label() == MAIN_WINDOW && crate::lock(&REQUESTS).acknowledge(request_id)
}

pub fn request(app: &AppHandle) {
    let Some(id) = crate::lock(&REQUESTS).begin() else {
        return;
    };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        // 活体检测会读进程与磁盘，不能占用托盘所在的主线程。
        let handle = app.clone();
        let sent = tauri::async_runtime::spawn_blocking(move || {
            handle.emit_to(
                MAIN_WINDOW,
                "request-quit-confirm",
                serde_json::json!({
                    "request_id": id,
                    "kernel_running": crate::kernel_running(&handle),
                    "official_chat_open": handle.get_window("official-chat").is_some(),
                    "from_background": true,
                }),
            )
        })
        .await;
        if matches!(sent, Ok(Ok(()))) {
            tokio::time::sleep(RESPONSE_TIMEOUT).await;
        }
        if !crate::lock(&REQUESTS).claim_fallback(id) {
            return;
        }

        let result = fallback(app).await;
        if let Err(error) = result {
            shell_events::record("shell-quit", &format!("原生退出失败：{error}"));
            let message = format!(
                "退出失败：{error}\n请再次从托盘选择退出。日志：{}（shell-quit）。",
                super::process::shell_logs_dir().display()
            );
            let _ =
                tauri::async_runtime::spawn_blocking(move || native_dialog(&message, false)).await;
        }
        crate::lock(&REQUESTS).finish(id);
    });
}

async fn fallback(app: AppHandle) -> Result<(), String> {
    shell_events::record("shell-quit", "管理面板未应答退出请求，使用原生确认框");
    let confirmed = tauri::async_runtime::spawn_blocking(|| native_dialog(
        "管理面板未响应退出请求。\n退出将停止内核与进行中的任务，并关闭工作台、官网网页版和常驻图标。\n是否退出 dsh-xlink？", true
    )).await.map_err(|e| e.to_string())??;
    if !confirmed {
        return Ok(());
    }
    // 与正常路径复用同一套停止及拆窗逻辑；停止失败绝不继续销毁管理面板。
    crate::commands::stop_kernel(app.clone()).await?;
    crate::commands::confirm_close_shell(app).await
}

#[cfg(target_os = "windows")]
fn native_dialog(message: &str, confirm: bool) -> Result<bool, String> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, IDYES, MB_DEFBUTTON2, MB_ICONWARNING, MB_OK, MB_SETFOREGROUND, MB_TASKMODAL,
        MB_YESNO,
    };
    let text: Vec<u16> = message.encode_utf16().chain(Some(0)).collect();
    let title: Vec<u16> = "退出 dsh-xlink".encode_utf16().chain(Some(0)).collect();
    // 无需 WebView/HWND；默认选“否”，避免错误页加载时意外停止任务。
    let result = unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            title.as_ptr(),
            (if confirm {
                MB_YESNO | MB_DEFBUTTON2
            } else {
                MB_OK
            }) | MB_ICONWARNING
                | MB_SETFOREGROUND
                | MB_TASKMODAL,
        )
    };
    if result == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(result == IDYES)
}

#[cfg(target_os = "macos")]
fn native_dialog(message: &str, confirm: bool) -> Result<bool, String> {
    // 文案经 argv 传入，不插进 AppleScript 源码。无需 WebView 或新依赖。
    let output = super::process::command_with_path("/usr/bin/osascript")
        .args(["-e", "on run argv\ntry\nif item 2 of argv is \"confirm\" then\ndisplay dialog (item 1 of argv) with title \"退出 dsh-xlink\" buttons {\"取消\", \"退出\"} default button \"取消\" cancel button \"取消\" with icon caution\nreturn \"quit\"\nelse\ndisplay dialog (item 1 of argv) with title \"退出 dsh-xlink\" buttons {\"好\"} default button \"好\" with icon caution\nreturn \"ok\"\nend if\non error number -128\nreturn \"cancel\"\nend try\nend run", message, if confirm { "confirm" } else { "error" }])
        .output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim() == "quit")
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn native_dialog(_: &str, _: bool) -> Result<bool, String> {
    Err("当前平台没有原生退出确认框".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn responsive_panel_cancels_only_its_own_fallback() {
        let mut requests = QuitRequests::new();
        let first = requests.begin().unwrap();
        assert!(requests.begin().is_none());
        assert!(!requests.acknowledge(first + 1));
        assert!(requests.acknowledge(first));
        assert!(!requests.claim_fallback(first));
        let second = requests.begin().unwrap();
        assert!(!requests.acknowledge(first));
        assert!(!requests.claim_fallback(first));
        assert!(requests.claim_fallback(second));
    }

    #[test]
    fn unresponsive_panel_falls_back_once_and_rejects_late_ack() {
        let mut requests = QuitRequests::new();
        let id = requests.begin().unwrap();
        assert!(requests.claim_fallback(id));
        assert!(!requests.claim_fallback(id));
        assert!(!requests.acknowledge(id));
        assert!(requests.begin().is_none());
        requests.finish(id + 1);
        assert!(requests.begin().is_none());
        requests.finish(id); // 原生取消 / 失败后允许再次尝试退出。
        assert!(requests.begin().is_some());
    }
}
