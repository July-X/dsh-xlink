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
pub(crate) fn permission_denied() -> bool {
    native::permission_denied()
}

#[cfg(target_os = "macos")]
mod native {
    use super::*;
    use block2::{DynBlock, RcBlock};
    use objc2::runtime::Bool;
    use objc2::{
        define_class, msg_send, rc::Retained, runtime::ProtocolObject, AnyThread, ClassType,
    };
    use objc2_foundation::{NSArray, NSError, NSObject, NSObjectProtocol, NSString};
    use objc2_user_notifications::*;
    use std::{
        cell::RefCell,
        ptr::NonNull,
        sync::atomic::{AtomicBool, Ordering},
    };
    use tauri::Emitter;
    static PERMISSION_DENIED: AtomicBool = AtomicBool::new(false);
    define_class!(
        #[unsafe(super = NSObject)]
        #[name = "DshXlinkNotificationDelegate"]
        #[thread_kind = AnyThread]
        struct NotificationDelegate;
        unsafe impl NSObjectProtocol for NotificationDelegate {}
        unsafe impl UNUserNotificationCenterDelegate for NotificationDelegate {
            #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
            fn activated(
                &self,
                center: &UNUserNotificationCenter,
                response: &UNNotificationResponse,
                completed: &DynBlock<dyn Fn()>,
            ) {
                let id = response.notification().request().identifier();
                // SAFETY: 框架常量由系统提供；只有打开通知的默认动作可以发起导航。
                let open = response
                    .actionIdentifier()
                    .isEqualToString(unsafe { UNNotificationDefaultActionIdentifier });
                if let (Some(app), Some(token)) = (APP.get(), clicked_token(&id.to_string(), open))
                {
                    super::super::navigation::clicked(app, &token);
                    center
                        .removeDeliveredNotificationsWithIdentifiers(&NSArray::from_slice(&[&*id]));
                }
                completed.call(());
            }
            #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
            fn present(
                &self,
                _center: &UNUserNotificationCenter,
                _notification: &UNNotification,
                completed: &DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
            ) {
                completed.call((UNNotificationPresentationOptions::Banner
                    | UNNotificationPresentationOptions::List
                    | UNNotificationPresentationOptions::Sound,));
            }
        }
    );
    thread_local! { static DELEGATE: RefCell<Option<Retained<NotificationDelegate>>> = const { RefCell::new(None) }; }
    fn prefix(sensitive: bool) -> String {
        format!(
            "xlink-{}-{}-",
            if sensitive { "sensitive" } else { "private" },
            crate::shell::settings::current_mode().as_str()
        )
    }
    fn clicked_token(identifier: &str, open: bool) -> Option<String> {
        if !open {
            return None;
        }
        identifier
            .strip_prefix(&prefix(true))
            .or_else(|| identifier.strip_prefix(&prefix(false)))
            .filter(|token| {
                token.len() == 38
                    && token.starts_with("xlink-")
                    && token[6..].bytes().all(|b| b.is_ascii_hexdigit())
            })
            .map(str::to_string)
    }
    fn failed(message: String) {
        crate::shell::shell_events::record("notification-native", &message);
        if let Some(app) = APP.get() {
            super::super::task::remember_toast_error(app, Err(message));
            let _ = app.emit("notification-status", super::super::task::status(app));
        }
    }
    fn refresh_permission(center: &UNUserNotificationCenter) {
        center.getNotificationSettingsWithCompletionHandler(&RcBlock::new(
            |settings: NonNull<UNNotificationSettings>| {
                // SAFETY: 系统在回调执行期间持有 settings。
                let status = unsafe { settings.as_ref() }.authorizationStatus();
                PERMISSION_DENIED.store(status == UNAuthorizationStatus::Denied, Ordering::Release);
            },
        ));
    }
    pub(super) fn permission_denied() -> bool {
        if super::super::notify_gate::in_app_bundle() {
            refresh_permission(&UNUserNotificationCenter::currentNotificationCenter());
        }
        PERMISSION_DENIED.load(Ordering::Acquire)
    }
    pub(super) fn serve(app: &AppHandle) {
        // UserNotifications 在没有 bundle 的裸开发二进制中不能初始化。
        if !super::super::notify_gate::in_app_bundle() {
            return;
        }
        if let Err(error) = app.run_on_main_thread(|| {
            let center = UNUserNotificationCenter::currentNotificationCenter();
            // SAFETY: NSObject init 与协议签名一致；主线程槽持有弱 delegate 的原件。
            let delegate: Retained<NotificationDelegate> =
                unsafe { msg_send![NotificationDelegate::alloc(), init] };
            center.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
            DELEGATE.with(|slot| *slot.borrow_mut() = Some(delegate));
            refresh_permission(&center);
        }) {
            failed(format!("注册通知回执失败：{error}"));
        }
    }
    fn payload(
        token: &str,
        body: &str,
        sound: bool,
        state: Option<bool>,
    ) -> Retained<UNNotificationRequest> {
        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str("任务已完成"));
        content.setBody(&NSString::from_str(super::super::privacy::body(
            body, state,
        )));
        if sound {
            content.setSound(Some(&UNNotificationSound::defaultSound()));
        }
        UNNotificationRequest::requestWithIdentifier_content_trigger(
            &NSString::from_str(&format!("{}{token}", prefix(state == Some(true)))),
            &content,
            None,
        )
    }
    pub(super) fn show(
        _app: &AppHandle,
        token: &str,
        body: &str,
        sound: bool,
        sensitive: bool,
    ) -> Result<(), String> {
        let token = token.to_owned();
        let body = body.to_owned();
        let center = UNUserNotificationCenter::currentNotificationCenter();
        center.requestAuthorizationWithOptions_completionHandler(
            UNAuthorizationOptions::Alert
                | UNAuthorizationOptions::Sound
                | UNAuthorizationOptions::Badge,
            &RcBlock::new(move |allowed: Bool, error: *mut NSError| {
                if !error.is_null() {
                    // SAFETY: NSError 在授权回调内有效。
                    failed(format!(
                        "请求通知权限失败：{}",
                        unsafe { &*error }.localizedDescription()
                    ));
                    return;
                }
                PERMISSION_DENIED.store(!allowed.as_bool(), Ordering::Release);
                if !allowed.as_bool() {
                    failed("系统未允许通知，请在 macOS 系统设置 → 通知中允许 dsh-xlink".into());
                    return;
                }
                // 授权可能等了很久，发送时重新裁剪；原本私密的通知不会升级为敏感正文。
                let state = Some(sensitive && super::super::privacy::can_disclose());
                let request = payload(&token, &body, sound, state);
                UNUserNotificationCenter::currentNotificationCenter()
                    .addNotificationRequest_withCompletionHandler(
                        &request,
                        Some(&RcBlock::new(|error: *mut NSError| {
                            if !error.is_null() {
                                // SAFETY: NSError 在投递回调内有效。
                                failed(format!(
                                    "通知投递失败：{}",
                                    unsafe { &*error }.localizedDescription()
                                ));
                            }
                        })),
                    );
            }),
        );
        Ok(())
    }
    pub(super) fn remove_sensitive(app: &AppHandle) -> Result<(), String> {
        if !super::super::notify_gate::in_app_bundle() {
            return Ok(());
        }
        let center = UNUserNotificationCenter::currentNotificationCenter();
        center.getDeliveredNotificationsWithCompletionHandler(&RcBlock::new(
            |notifications: NonNull<NSArray<UNNotification>>| {
                // SAFETY: 系统在回调执行期间持有列表。
                let ids: Vec<_> = unsafe { notifications.as_ref() }
                    .iter()
                    .map(|n| n.request().identifier())
                    .filter(|id| id.to_string().starts_with(&prefix(true)))
                    .collect();
                UNUserNotificationCenter::currentNotificationCenter()
                    .removeDeliveredNotificationsWithIdentifiers(&NSArray::from_retained_slice(
                        &ids,
                    ));
            },
        ));
        center.getPendingNotificationRequestsWithCompletionHandler(&RcBlock::new(
            |requests: NonNull<NSArray<UNNotificationRequest>>| {
                // SAFETY: 系统在回调执行期间持有列表。
                let ids: Vec<_> = unsafe { requests.as_ref() }
                    .iter()
                    .map(|n| n.identifier())
                    .filter(|id| id.to_string().starts_with(&prefix(true)))
                    .collect();
                UNUserNotificationCenter::currentNotificationCenter()
                    .removePendingNotificationRequestsWithIdentifiers(
                        &NSArray::from_retained_slice(&ids),
                    );
            },
        ));
        // 只作旧版通知清理；激活与新投递都不再依赖 Foundation 的已弃用接口。
        #[allow(deprecated)]
        app.run_on_main_thread(|| {
            use objc2_foundation::NSUserNotificationCenter;
            // SAFETY: 裸二进制可能返回 nil，按可空声明读取。
            let center: Option<Retained<NSUserNotificationCenter>> = unsafe {
                msg_send![
                    NSUserNotificationCenter::class(),
                    defaultUserNotificationCenter
                ]
            };
            if let Some(center) = center {
                for n in center.deliveredNotifications() {
                    if n.identifier().is_some_and(|id| {
                        id.to_string().starts_with(&prefix(true))
                            || (crate::shell::settings::current_mode()
                                == crate::shell::paths::ShellMode::Release
                                && !id.to_string().starts_with("xlink-"))
                    }) {
                        center.removeDeliveredNotification(&n);
                    }
                }
            }
        })
        .map_err(|e| e.to_string())
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn native_payload_is_private_and_receipts_keep_the_exact_token() {
            for state in [Some(false), None] {
                let request = payload(
                    "xlink-0123456789abcdef0123456789abcdef",
                    "private title / question / project",
                    true,
                    state,
                );
                assert_eq!(request.content().title().to_string(), "任务已完成");
                assert_eq!(request.content().body().to_string(), "任务已完成");
            }
            let a = payload(
                "xlink-0123456789abcdef0123456789abcdef",
                "normal A",
                false,
                Some(true),
            );
            let b = payload(
                "xlink-fedcba9876543210fedcba9876543210",
                "normal B",
                false,
                Some(true),
            );
            assert_eq!(a.content().body().to_string(), "normal A");
            assert_eq!(
                clicked_token(&a.identifier().to_string(), true).as_deref(),
                Some("xlink-0123456789abcdef0123456789abcdef")
            );
            assert_eq!(
                clicked_token(&b.identifier().to_string(), true).as_deref(),
                Some("xlink-fedcba9876543210fedcba9876543210")
            );
            assert!(clicked_token(&a.identifier().to_string(), false).is_none());
            assert!(clicked_token("foreign-notification", true).is_none());
            assert!(a.content().sound().is_none());
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
