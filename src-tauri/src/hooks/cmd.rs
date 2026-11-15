//! Hook 模块的本地管理命令（P3 面板的前端契约）。
//!
//! 管理面只走本地 IPC；接收端凭据不能读取其他事件、不能修改设置。明文的
//! 接入点令牌只在创建/轮换的返回值里出现一次，列表与日志永不含明文。

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::hooks::config::{HooksConfig, CONFIG_SCHEMA_VERSION};
use crate::hooks::service::{self, HooksStatus};
use crate::hooks::store::{self, EndpointRow, EventFilter, EventRow, StoreError};

const SUPPORTED_KINDS: [&str; 2] = ["text", "parameters"];

#[derive(Debug, Clone, Serialize)]
pub(crate) struct CreatedEndpoint {
    pub endpoint: EndpointRow,
    /// 明文令牌，只在此响应中出现一次；不落盘、不写日志。
    pub token: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct EventList {
    pub items: Vec<EventRow>,
    pub total: u64,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct InboxFilter {
    pub endpoint_id: Option<String>,
    pub state: Option<String>,
    pub q: Option<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

fn open_for_write() -> Result<Connection, String> {
    store::open(&service::db_path()).map_err(|error| {
        format!(
            "无法打开 Hook 收件箱 {}：{error}",
            service::db_path().display()
        )
    })
}

fn open_for_read() -> Result<Option<Connection>, String> {
    store::open_if_exists(&service::db_path()).map_err(|error| {
        format!(
            "无法打开 Hook 收件箱 {}：{error}",
            service::db_path().display()
        )
    })
}

fn store_error(action: &str, error: StoreError) -> String {
    match error {
        StoreError::NotFound => format!("{action}：对象不存在"),
        StoreError::Conflict => format!("{action}：对象状态冲突，请刷新后重试"),
        StoreError::Unavailable(reason) => format!("{action} 失败：{reason}"),
    }
}

fn validate_endpoint_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("接入点名称不能为空".into());
    }
    if name.chars().count() > 64 || name.chars().any(char::is_control) {
        return Err("接入点名称最多 64 个字符，且不能包含控制字符".into());
    }
    Ok(name.to_string())
}

fn normalize_kinds(kinds: Option<Vec<String>>) -> Result<Vec<String>, String> {
    let mut normalized = Vec::new();
    for kind in kinds.unwrap_or_else(|| {
        SUPPORTED_KINDS
            .iter()
            .map(|kind| kind.to_string())
            .collect()
    }) {
        if !SUPPORTED_KINDS.contains(&kind.as_str()) {
            return Err(format!(
                "内容类型 {kind:?} 尚未支持；当前只允许 text、parameters"
            ));
        }
        if !normalized.contains(&kind) {
            normalized.push(kind);
        }
    }
    if normalized.is_empty() {
        return Err("接入点至少要允许一种内容类型".into());
    }
    Ok(normalized)
}

#[tauri::command]
pub async fn hooks_status() -> Result<HooksStatus, String> {
    crate::commands::blocking(|| Ok::<HooksStatus, String>(service::status())).await
}

#[tauri::command]
pub async fn hooks_save_config(enabled: bool, port: u16) -> Result<HooksStatus, String> {
    crate::commands::blocking(move || {
        service::apply_config(HooksConfig {
            schema_version: CONFIG_SCHEMA_VERSION,
            enabled,
            port,
        })
    })
    .await
}

#[tauri::command]
pub async fn hooks_list_endpoints() -> Result<Vec<EndpointRow>, String> {
    crate::commands::blocking(move || -> Result<Vec<EndpointRow>, String> {
        match open_for_read()? {
            Some(connection) => {
                store::list_endpoints(&connection).map_err(|error| store_error("读取接入点", error))
            }
            None => Ok(Vec::new()),
        }
    })
    .await
}

#[tauri::command]
pub async fn hooks_create_endpoint(
    name: String,
    allowed_kinds: Option<Vec<String>>,
) -> Result<CreatedEndpoint, String> {
    crate::commands::blocking(move || -> Result<CreatedEndpoint, String> {
        let name = validate_endpoint_name(&name)?;
        let kinds = normalize_kinds(allowed_kinds)?;
        let connection = open_for_write()?;
        let (endpoint, token) = store::create_endpoint(&connection, &name, &kinds)
            .map_err(|error| store_error("创建接入点", error))?;
        Ok(CreatedEndpoint { endpoint, token })
    })
    .await
}

#[tauri::command]
pub async fn hooks_set_endpoint_enabled(id: String, enabled: bool) -> Result<EndpointRow, String> {
    crate::commands::blocking(move || {
        let connection = open_for_write()?;
        store::set_endpoint_enabled(&connection, &id, enabled)
            .map_err(|error| store_error("更新接入点状态", error))
    })
    .await
}

#[tauri::command]
pub async fn hooks_rotate_endpoint_token(id: String) -> Result<CreatedEndpoint, String> {
    crate::commands::blocking(move || -> Result<CreatedEndpoint, String> {
        let connection = open_for_write()?;
        let (endpoint, token) = store::rotate_endpoint_token(&connection, &id)
            .map_err(|error| store_error("轮换接入点令牌", error))?;
        Ok(CreatedEndpoint { endpoint, token })
    })
    .await
}

#[tauri::command]
pub async fn hooks_delete_endpoint(id: String) -> Result<(), String> {
    crate::commands::blocking(move || {
        let connection = open_for_write()?;
        store::delete_endpoint(&connection, &id).map_err(|error| store_error("删除接入点", error))
    })
    .await
}

#[tauri::command]
pub async fn hooks_inbox_list(filter: Option<InboxFilter>) -> Result<EventList, String> {
    crate::commands::blocking(move || -> Result<EventList, String> {
        let filter = filter.unwrap_or_default();
        let endpoint_id = filter.endpoint_id;
        let state = filter.state;
        let query = filter.q;
        let event_filter = EventFilter {
            endpoint_id,
            state,
            q: query,
            limit: filter.limit.unwrap_or(50).clamp(1, 100),
            offset: filter.offset.unwrap_or(0),
        };
        match open_for_read()? {
            Some(connection) => {
                let (items, total) = store::list_events(&connection, &event_filter)
                    .map_err(|error| store_error("读取收件箱", error))?;
                Ok(EventList { items, total })
            }
            None => Ok(EventList {
                items: Vec::new(),
                total: 0,
            }),
        }
    })
    .await
}

#[tauri::command]
pub async fn hooks_event_detail(event_id: String) -> Result<EventRow, String> {
    crate::commands::blocking(move || match open_for_read()? {
        Some(connection) => store::event_detail(&connection, &event_id)
            .map_err(|error| store_error("读取事件", error))?
            .ok_or_else(|| "事件不存在（可能已被清理）".to_string()),
        None => Err("收件箱还没有任何事件".to_string()),
    })
    .await
}
