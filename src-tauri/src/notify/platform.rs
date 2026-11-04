//! 原生通知的唯一投递点；通知自身标识用于导航和移除敏感历史。
use std::sync::OnceLock;
use tauri::AppHandle;
static APP: OnceLock<AppHandle> = OnceLock::new();
pub(crate) fn serve(app: &AppHandle) {
    let _ = APP.set(app.clone());
    native::serve(app);
    super::privacy::serve(app);
}
pub(crate) fn show(app: &AppHandle, token: &str, normal: &str, sound: bool) -> Result<(), String> {
    let unlocked = super::privacy::can_disclose();
    let body = super::privacy::body(normal, Some(unlocked));
    native::show(app, token, body, sound, unlocked)?;
    // 覆盖状态查询与投递间发生锁屏的常见竞态；解锁不重投正文。
    if unlocked && !super::privacy::unlocked() {
        remove_sensitive(app)?;
    }
    Ok(())
}
pub(crate) fn remove_sensitive(app: &AppHandle) -> Result<(), String> {
    native::remove_sensitive(app)
}

#[cfg(target_os = "macos")]
#[allow(deprecated)]
mod native {
    use super::*;
    use objc2::{
        define_class, msg_send, rc::Retained, runtime::ProtocolObject, ClassType, MainThreadOnly,
    };
    use objc2_foundation::{
        MainThreadMarker, NSObject, NSObjectProtocol, NSString, NSUserNotification,
        NSUserNotificationCenter, NSUserNotificationCenterDelegate,
    };
    use std::cell::RefCell;
    define_class!(
        #[unsafe(super = NSObject)]
        #[name = "DshXlinkNotificationDelegate"]
        #[thread_kind = MainThreadOnly]
        struct NotificationDelegate;
        unsafe impl NSObjectProtocol for NotificationDelegate {}
        unsafe impl NSUserNotificationCenterDelegate for NotificationDelegate {
            #[unsafe(method(userNotificationCenter:didActivateNotification:))]
            fn activated(
                &self,
                center: &NSUserNotificationCenter,
                notification: &NSUserNotification,
            ) {
                if let (Some(app), Some(id)) = (APP.get(), notification.identifier()) {
                    let token = id.to_string();
                    if let Some(token) = token
                        .strip_prefix(&format!(
                            "xlink-sensitive-{}-",
                            crate::shell::settings::current_mode().as_str()
                        ))
                        .or_else(|| {
                            token.strip_prefix(&format!(
                                "xlink-private-{}-",
                                crate::shell::settings::current_mode().as_str()
                            ))
                        })
                    {
                        super::super::navigation::clicked(app, token);
                    }
                }
                center.removeDeliveredNotification(notification);
            }
            #[unsafe(method(userNotificationCenter:shouldPresentNotification:))]
            fn present(
                &self,
                _center: &NSUserNotificationCenter,
                _notification: &NSUserNotification,
            ) -> bool {
                true
            }
        }
    );
    thread_local! { static DELEGATE: RefCell<Option<Retained<NotificationDelegate>>> = const { RefCell::new(None) }; }
    fn notification_center() -> Option<Retained<NSUserNotificationCenter>> {
        // 裸开发二进制会实际返回 nil，SDK 的非空声明会触发 Rust panic。
        // SAFETY: Objective-C 签名与 SDK 一致，仅将返回值按可空指针接收。
        unsafe {
            msg_send![
                NSUserNotificationCenter::class(),
                defaultUserNotificationCenter
            ]
        }
    }
    pub(super) fn serve(app: &AppHandle) {
        let result = app.run_on_main_thread(|| {
            let Some(center) = notification_center() else {
                crate::shell::shell_events::record(
                    "notification-native",
                    "macOS通知中心不可用；请使用安装产物验证通知",
                );
                return;
            };
            let Some(mtm) = MainThreadMarker::new() else {
                return;
            };
            // SAFETY: NSObject 的 init 与协议签名一致；delegate 由主线程槽持有。
            let delegate: Retained<NotificationDelegate> =
                unsafe { msg_send![NotificationDelegate::alloc(mtm), init] };
            unsafe {
                center.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
            }
            DELEGATE.with(|slot| *slot.borrow_mut() = Some(delegate));
        });
        if let Err(error) = result {
            crate::shell::shell_events::record("notification-native", &error.to_string());
        }
    }
    pub(super) fn show(
        app: &AppHandle,
        token: &str,
        body: &str,
        sound: bool,
        sensitive: bool,
    ) -> Result<(), String> {
        let token = token.to_string();
        let body = body.to_string();
        app.run_on_main_thread(move || {
            let Some(center) = notification_center() else {
                if let Some(app) = APP.get() {
                    super::super::task::remember_toast_error(
                        app,
                        Err("macOS通知中心不可用；请使用安装产物验证通知".into()),
                    );
                }
                return;
            };
            // 在主线程真正投递时重查锁屏，不能沿用排队时的状态。
            let sensitive = sensitive && super::super::privacy::can_disclose();
            let notification = payload(&token, &body, sound, Some(sensitive));
            center.deliverNotification(&notification);
        })
        .map_err(|e| e.to_string())
    }
    fn payload(
        token: &str,
        body: &str,
        sound: bool,
        state: Option<bool>,
    ) -> Retained<NSUserNotification> {
        let sensitive = state == Some(true);
        let prefix = if sensitive {
            "xlink-sensitive-"
        } else {
            "xlink-private-"
        };
        let notification = NSUserNotification::new();
        notification.setIdentifier(Some(&NSString::from_str(&format!(
            "{prefix}{}-{token}",
            crate::shell::settings::current_mode().as_str()
        ))));
        notification.setTitle(Some(&NSString::from_str("任务已完成")));
        notification.setInformativeText(Some(&NSString::from_str(super::super::privacy::body(
            body, state,
        ))));
        if sound {
            notification.setSoundName(Some(&NSString::from_str(
                "NSUserNotificationDefaultSoundName",
            )));
        }
        notification
    }
    pub(super) fn remove_sensitive(app: &AppHandle) -> Result<(), String> {
        app.run_on_main_thread(|| {
            let Some(center) = notification_center() else {
                return;
            };
            for notification in center.deliveredNotifications() {
                if notification.identifier().is_some_and(|id| {
                    let id = id.to_string();
                    id.starts_with(&format!(
                        "xlink-sensitive-{}-",
                        crate::shell::settings::current_mode().as_str()
                    )) || (crate::shell::settings::current_mode()
                        == crate::shell::paths::ShellMode::Release
                        && !id.starts_with("xlink-"))
                }) {
                    center.removeDeliveredNotification(&notification);
                }
            }
        })
        .map_err(|e| e.to_string())
    }
    #[cfg(test)]
    mod tests {
        #[test]
        fn native_private_notification_contains_no_normal_body() {
            for state in [Some(false), None] {
                let note =
                    super::payload("opaque-id", "secret title / prompt / project", true, state);
                assert_eq!(note.title().unwrap().to_string(), "任务已完成");
                assert_eq!(note.informativeText().unwrap().to_string(), "任务已完成");
                assert!(note
                    .identifier()
                    .unwrap()
                    .to_string()
                    .starts_with("xlink-private-"));
            }
            let note = super::payload("opaque-id", "normal", false, Some(true));
            assert_eq!(note.informativeText().unwrap().to_string(), "normal");
        }
    }
}

#[cfg(target_os = "windows")]
mod native {
    use super::*;
    use std::sync::Mutex;
    use windows::{
        core::HSTRING,
        Data::Xml::Dom::XmlDocument,
        Foundation::TypedEventHandler,
        UI::Notifications::{ToastNotification, ToastNotificationManager},
    };
    static TOASTS: Mutex<Vec<ToastNotification>> = Mutex::new(Vec::new());
    fn escape(text: &str) -> String {
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&apos;")
    }
    pub(super) fn serve(_app: &AppHandle) {}
    pub(super) fn show(
        app: &AppHandle,
        token: &str,
        body: &str,
        sound: bool,
        sensitive: bool,
    ) -> Result<(), String> {
        let send = || -> windows::core::Result<()> {
            let xml = XmlDocument::new()?;
            let audio = if sound {
                ""
            } else {
                "<audio silent=\"true\"/>"
            };
            xml.LoadXml(&HSTRING::from(format!("<toast launch=\"{}\"><visual><binding template=\"ToastGeneric\"><text>任务已完成</text><text>{}</text></binding></visual>{audio}</toast>", escape(token), escape(body))))?;
            let toast = ToastNotification::CreateToastNotification(&xml)?;
            // tag 最长16字符；完整点击标识在 launch 和回调闭包中。
            let tag = token.chars().rev().take(16).collect::<String>();
            toast.SetTag(&HSTRING::from(tag))?;
            let group = format!(
                "xlink-{}-{}",
                if sensitive { "s" } else { "p" },
                crate::shell::settings::current_mode().as_str()
            );
            toast.SetGroup(&HSTRING::from(group))?;
            let token = token.to_string();
            let app_for_click = app.clone();
            toast.Activated(&TypedEventHandler::new(move |_, _| {
                super::super::navigation::clicked(&app_for_click, &token);
                Ok(())
            }))?;
            let notifier = ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(
                super::super::notify_gate::app_id(app),
            ))?;
            notifier.Show(&toast)?;
            let mut toasts = crate::lock(&TOASTS);
            toasts.push(toast);
            while toasts.len() > 32 {
                toasts.remove(0);
            }
            Ok(())
        };
        send().map_err(|e| e.to_string())
    }
    pub(super) fn remove_sensitive(app: &AppHandle) -> Result<(), String> {
        let remove = || -> windows::core::Result<()> {
            let app_id = HSTRING::from(super::super::notify_gate::app_id(app));
            let history = ToastNotificationManager::History()?;
            let notifier = ToastNotificationManager::CreateToastNotifierWithId(&app_id)?;
            for toast in history.GetHistoryWithId(&app_id)? {
                if toast.Group()?.to_string()
                    == format!(
                        "xlink-s-{}",
                        crate::shell::settings::current_mode().as_str()
                    )
                    || (toast.Group()?.is_empty()
                        && crate::shell::settings::current_mode()
                            == crate::shell::paths::ShellMode::Release)
                {
                    notifier.Hide(&toast)?;
                    history.RemoveGroupedTagWithId(&toast.Tag()?, &toast.Group()?, &app_id)?;
                }
            }
            Ok(())
        };
        remove().map_err(|e| e.to_string())
    }
}
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod native {
    use super::*;
    pub(super) fn serve(_: &AppHandle) {}
    pub(super) fn show(_: &AppHandle, _: &str, _: &str, _: bool, _: bool) -> Result<(), String> {
        Err("当前平台不支持系统通知".into())
    }
    pub(super) fn remove_sensitive(_: &AppHandle) -> Result<(), String> {
        Ok(())
    }
}
