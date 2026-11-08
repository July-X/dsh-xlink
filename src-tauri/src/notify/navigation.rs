//! 通知标识映射到准确会话；只消费真实点击，工作台就绪后确认消费。
use serde::Serialize;
use std::{
    collections::VecDeque,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Target {
    pub sequence: u64,
    pub session_id: String,
    family: String,
    instance: String,
    mode: String,
}
struct Pending {
    target: Target,
    dispatched: bool,
    since: Instant,
}
#[derive(Default)]
struct Queue {
    routes: VecDeque<(String, Target)>,
    pending: Option<Pending>,
    sequence: u64,
}
impl Queue {
    fn select(&mut self, token: &str) -> Option<()> {
        let mut target = self
            .routes
            .iter()
            .find(|(id, _)| id == token)
            .map(|(_, target)| target.clone())?;
        self.sequence += 1;
        target.sequence = self.sequence;
        self.pending = Some(Pending {
            target,
            dispatched: false,
            since: Instant::now(),
        });
        Some(())
    }
    fn complete(&mut self, sequence: u64) -> bool {
        if self
            .pending
            .as_ref()
            .is_some_and(|p| p.target.sequence == sequence)
        {
            self.pending = None;
            true
        } else {
            false
        }
    }
}
fn queue() -> &'static Mutex<Queue> {
    static QUEUE: OnceLock<Mutex<Queue>> = OnceLock::new();
    QUEUE.get_or_init(|| Mutex::new(Queue::default()))
}

pub(crate) fn register(kernel: super::task::TaskKernel, session_id: &str) -> String {
    use super::task::TaskKernel;
    // MiniMax 没有可深链的会话路由：点击只把 MiniMax 工作台抬到前台，因此
    // family 记成 mcode，点击分支据此走「聚焦 MiniMax 窗口」而不是 DSH 导航。
    let (family, instance) = match kernel {
        TaskKernel::Dsh => {
            let (family, instance) = crate::shell::instance::resolve_default();
            (family.to_string(), instance.to_string())
        }
        TaskKernel::Mcode => (
            crate::shell::instance::KERNEL_FAMILY_MCODE.to_string(),
            crate::kernel::mcode_service::configured_instance_id()
                .ok()
                .flatten()
                .unwrap_or_default(),
        ),
    };
    let mut q = crate::lock(queue());
    q.sequence += 1;
    let target = Target {
        sequence: q.sequence,
        session_id: session_id.to_string(),
        family,
        instance,
        mode: crate::shell::settings::current_mode().as_str().into(),
    };
    let token = format!("xlink-{:032x}", rand::random::<u128>());
    q.routes.push_back((token.clone(), target));
    while q.routes.len() > 32 {
        q.routes.pop_front();
    }
    token
}
fn compatible(target: &Target) -> bool {
    let (family, instance) = crate::shell::instance::resolve_default();
    target.family == family
        && target.instance == instance
        && target.mode == crate::shell::settings::current_mode().as_str()
}
pub(crate) fn clicked(app: &AppHandle, token: &str) {
    let mut q = crate::lock(queue());
    if q.select(token).is_none() {
        drop(q);
        fail(app, "通知目标已失效，请从工作台打开会话".into());
        return;
    }
    let is_mcode = q
        .pending
        .as_ref()
        .is_some_and(|p| p.target.family == crate::shell::instance::KERNEL_FAMILY_MCODE);
    if is_mcode {
        // 没有可深链的会话路由：消费掉目标，只负责把它那个工作台抬出来。
        q.pending = None;
        drop(q);
        focus_mcode_workbench(app);
        return;
    }
    drop(q);
    resume(app);
}

/// 把 MiniMax 工作台抬到前台；窗口没开就退回管理面板（用户可以从概览再开）。
fn focus_mcode_workbench(app: &AppHandle) {
    let Some(window) = app.get_webview_window(crate::shell::appearance::MCODE_WORKBENCH_LABEL)
    else {
        crate::show_main_shell(app);
        return;
    };
    let _ = window.show();
    let _ = window.unminimize();
    let _ = window.set_focus();
}
pub(crate) fn resume(app: &AppHandle) {
    if !super::privacy::unlocked() {
        if let Some(pending) = crate::lock(queue()).pending.as_mut() {
            pending.dispatched = false;
        }
        return;
    }
    let target = {
        let mut q = crate::lock(queue());
        let Some(pending) = q.pending.as_mut() else {
            return;
        };
        if pending.since.elapsed() > Duration::from_secs(30) && pending.dispatched {
            q.pending = None;
            drop(q);
            fail(
                app,
                "工作台导航服务未就绪，请重启工作台后重试；日志中可查看 notification-navigation"
                    .into(),
            );
            return;
        }
        if pending.dispatched {
            return;
        }
        pending.dispatched = true;
        pending.since = Instant::now();
        pending.target.clone()
    };
    if !compatible(&target) || !crate::kernel_running(app) {
        crate::lock(queue()).pending = None;
        fail(
            app,
            "通知所属实例已变化或内核未运行，请启动对应工作台后打开会话".into(),
        );
        return;
    }
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = crate::commands::open_harness(handle.clone()).await {
            let mut q = crate::lock(queue());
            if q.complete(target.sequence) {
                drop(q);
                fail(&handle, error);
            }
            return;
        }
        let current = crate::lock(queue())
            .pending
            .as_ref()
            .map(|p| p.target.sequence);
        if current != Some(target.sequence) || !super::privacy::unlocked() {
            return;
        }
        if let Some(window) = handle.get_webview_window("harness") {
            super::activate::force_foreground(&window);
            match serde_json::to_string(&target) {
                Ok(json) => { if let Err(error) = window.eval(format!("window.dispatchEvent(new CustomEvent('dsh-xlink-notification',{{detail:{json}}}));")) { fail(&handle, error.to_string()); } }
                Err(error) => fail(&handle, error.to_string()),
            }
        }
    });
}
fn fail(app: &AppHandle, reason: String) {
    crate::shell::shell_events::record("notification-navigation", &reason);
    crate::show_main_shell(app);
    let _ = app.emit(super::activate::FAILED_EVENT, reason);
}
#[tauri::command]
pub(crate) fn notification_navigation_ready(
    window: WebviewWindow,
) -> Result<Option<Target>, String> {
    if window.label() != "harness" {
        return Err("只有工作台可以消费通知目标".into());
    }
    if !super::privacy::unlocked() {
        return Ok(None);
    }
    Ok(crate::lock(queue())
        .pending
        .as_ref()
        .filter(|p| compatible(&p.target))
        .map(|p| p.target.clone()))
}
#[tauri::command]
pub(crate) fn notification_navigation_result(
    app: AppHandle,
    window: WebviewWindow,
    sequence: u64,
    error: String,
) -> Result<(), String> {
    if window.label() != "harness" {
        return Err("只有工作台可以确认通知导航".into());
    }
    let mut q = crate::lock(queue());
    if q.complete(sequence) {
        drop(q);
        if !error.is_empty() {
            fail(&app, error.chars().take(512).collect());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn target(id: &str) -> Target {
        Target {
            sequence: 0,
            session_id: id.into(),
            family: "dsh".into(),
            instance: "work".into(),
            mode: "dev".into(),
        }
    }
    #[test]
    fn clicks_select_their_own_session_and_stale_ack_cannot_clear_a_new_click() {
        let mut q = Queue::default();
        q.routes.push_back(("A".into(), target("session-A")));
        q.routes.push_back(("B".into(), target("session-B")));
        q.select("B").unwrap();
        let old = q.pending.as_ref().unwrap().target.sequence;
        q.select("A").unwrap();
        assert_eq!(q.pending.as_ref().unwrap().target.session_id, "session-A");
        assert!(!q.complete(old));
        let sequence = q.pending.as_ref().unwrap().target.sequence;
        assert!(q.complete(sequence));
        assert!(q.pending.is_none());
        assert!(q.select("missing").is_none());
    }
}
