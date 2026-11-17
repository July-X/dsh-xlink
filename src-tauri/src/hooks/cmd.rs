//! Hook 模块的本地管理命令（P3 面板的前端契约）。
//!
//! 管理面只走本地 IPC；接收端凭据不能读取其他事件、不能修改设置。明文的
//! 接入点令牌只在创建/轮换的返回值里出现一次，列表与日志永不含明文。

use std::io::{Read, Seek, SeekFrom};

use base64::Engine as _;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::hooks::config::{HooksConfig, CONFIG_SCHEMA_VERSION};
use crate::hooks::service::{self, HooksStatus};
use crate::hooks::store::{
    self, AttachmentRow, EndpointRow, EventFilter, EventRow, EventSummary, StoreError,
};

const SUPPORTED_KINDS: [&str; 2] = ["text", "parameters"];
/// 日志预览每页字节数：50 MiB 日志不能整块搬进 IPC 与渲染进程。
const PREVIEW_PAGE_BYTES: u64 = 64 * 1024;
/// 图片预览上限与 P2 单图限额一致；超过就拒绝，不整块读入。
const MAX_IMAGE_PREVIEW_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct CreatedEndpoint {
    pub endpoint: EndpointRow,
    /// 明文令牌，只在此响应中出现一次；不落盘、不写日志。
    pub token: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct CreatedNotification {
    pub endpoint: EndpointRow,
    /// Basic 密码，只在此响应中出现一次；与 Bearer 令牌分开。
    pub token: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct EventList {
    pub items: Vec<EventSummary>,
    pub total: u64,
}

/// 详情：事件不可变内容 + 附件状态（P3 预览用 attachment_id 再取受控内容）。
#[derive(Debug, Clone, Serialize)]
pub(crate) struct EventDetail {
    pub event: EventRow,
    pub attachments: Vec<AttachmentRow>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct InboxFilter {
    pub endpoint_id: Option<String>,
    pub state: Option<String>,
    /// `image` / `log`；空表示不按附件类型筛。
    pub attachment_kind: Option<String>,
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
        StoreError::QuotaExceeded(limit) => {
            format!(
                "{action}：Hook 存储预算已满（上限 {limit} 字节），且没有可清理的历史事件；请等接收完成或手动删除"
            )
        }
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
pub async fn hooks_save_config(
    enabled: bool,
    port: u16,
    interface_name: Option<String>,
    bind_ip: Option<String>,
    netmask: Option<String>,
    allowed_subnets: Option<Vec<String>>,
    disable_only: Option<bool>,
) -> Result<HooksStatus, String> {
    crate::commands::blocking(move || {
        if !enabled && disable_only.unwrap_or(false) {
            return Ok(service::disable());
        }
        service::apply_config(HooksConfig {
            schema_version: CONFIG_SCHEMA_VERSION,
            enabled,
            port,
            interface_name: interface_name.unwrap_or_default(),
            bind_ip: bind_ip.unwrap_or_default(),
            netmask: netmask.unwrap_or_default(),
            allowed_subnets: allowed_subnets.unwrap_or_default(),
            confirmation_id: String::new(),
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
pub async fn hooks_rotate_endpoint_notification_token(
    id: String,
) -> Result<CreatedNotification, String> {
    crate::commands::blocking(move || -> Result<CreatedNotification, String> {
        let connection = open_for_write()?;
        let (endpoint, token) = store::rotate_notification_token(&connection, &id)
            .map_err(|error| store_error("生成通知凭据", error))?;
        Ok(CreatedNotification { endpoint, token })
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
            attachment_kind: filter.attachment_kind,
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

/// 手动永久删除一个事件与其附件文件。P3 面板必须先展示删除范围再调用。
#[tauri::command]
pub async fn hooks_delete_event(event_id: String) -> Result<(), String> {
    crate::commands::blocking(move || -> Result<(), String> {
        let mut connection = open_for_write()?;
        let rels = store::delete_event(&mut connection, &event_id)
            .map_err(|error| store_error("删除事件", error))?;
        for rel in rels {
            crate::hooks::blob::remove_rel(&rel)
                .map_err(|error| format!("事件已删除，但附件文件清理失败：{error}"))?;
        }
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn hooks_event_detail(event_id: String) -> Result<EventDetail, String> {
    crate::commands::blocking(move || match open_for_read()? {
        Some(connection) => {
            let event = store::event_detail(&connection, &event_id)
                .map_err(|error| store_error("读取事件", error))?
                .ok_or_else(|| "事件不存在（可能已被清理）".to_string())?;
            let attachments = store::event_attachments(&connection, &event.id)
                .map_err(|error| store_error("读取附件", error))?;
            Ok(EventDetail { event, attachments })
        }
        None => Err("收件箱还没有任何事件".to_string()),
    })
    .await
}

/// 受控本地预览：只按事件 + 附件 ID 读已归档文件，图片整块转 base64，
/// 日志按 64 KiB 分页。前端不接触磁盘路径，也不经网络下载。
#[derive(Debug, Clone, Serialize)]
pub(crate) struct AttachmentPreview {
    pub kind: String,
    pub media_type: String,
    pub size_bytes: i64,
    pub offset: u64,
    pub text: Option<String>,
    pub data_base64: Option<String>,
    /// 本次只是文件的一段，不是完整内容。
    pub truncated: bool,
    pub prev_offset: Option<u64>,
    pub next_offset: Option<u64>,
    /// 这一页按 UTF-8 解不出来（编码声明不符或跨页切断多字节字符）。
    pub invalid_utf8: bool,
}

fn preview_attachment(
    event_id: &str,
    attachment_id: &str,
    offset: u64,
) -> Result<AttachmentPreview, String> {
    let connection = open_for_read()?.ok_or_else(|| "收件箱还没有任何事件".to_string())?;
    let attachment = store::attachment(&connection, event_id, attachment_id)
        .map_err(|error| store_error("读取附件", error))?
        .ok_or_else(|| "附件不存在（可能已被清理）".to_string())?;
    if attachment.state != store::ATTACHMENT_STORED {
        return Err(format!(
            "附件尚未归档（当前状态：{}），等发送方上传完成后再预览",
            attachment.state
        ));
    }
    let rel = attachment
        .storage_rel
        .as_deref()
        .ok_or_else(|| "附件没有归档路径，请刷新收件箱后重试".to_string())?;
    let path = crate::hooks::blob::path_for_rel(rel)?;
    let meta = std::fs::symlink_metadata(&path)
        .map_err(|error| format!("无法读取附件文件 {}：{error}", path.display()))?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err("附件路径不是普通文件，已拒绝读取".into());
    }
    let size = meta.len();
    if attachment.size_bytes >= 0 && attachment.size_bytes as u64 != size {
        return Err("附件文件大小与记录不一致，可能已被外部修改；请刷新收件箱后重试".into());
    }
    let mut preview = AttachmentPreview {
        kind: attachment.kind.clone(),
        media_type: attachment.media_type.clone(),
        size_bytes: size as i64,
        offset: 0,
        text: None,
        data_base64: None,
        truncated: false,
        prev_offset: None,
        next_offset: None,
        invalid_utf8: false,
    };
    match attachment.kind.as_str() {
        crate::hooks::protocol::KIND_IMAGE => {
            if size > MAX_IMAGE_PREVIEW_BYTES {
                return Err(format!(
                    "图片大于 {MAX_IMAGE_PREVIEW_BYTES} 字节，面板不整块读取；原文件仍保留在收件箱"
                ));
            }
            let bytes = std::fs::read(&path)
                .map_err(|error| format!("无法读取图片 {}：{error}", path.display()))?;
            preview.data_base64 = Some(base64::engine::general_purpose::STANDARD.encode(&bytes));
        }
        crate::hooks::protocol::KIND_LOG => {
            let start = offset.min(size);
            let mut file = std::fs::File::open(&path)
                .map_err(|error| format!("无法读取日志 {}：{error}", path.display()))?;
            file.seek(SeekFrom::Start(start))
                .map_err(|error| format!("无法定位日志 {}：{error}", path.display()))?;
            let mut buffer = Vec::with_capacity(PREVIEW_PAGE_BYTES as usize);
            file.take(PREVIEW_PAGE_BYTES)
                .read_to_end(&mut buffer)
                .map_err(|error| format!("无法读取日志 {}：{error}", path.display()))?;
            let read = buffer.len() as u64;
            preview.offset = start;
            preview.invalid_utf8 = std::str::from_utf8(&buffer).is_err();
            preview.text = Some(String::from_utf8_lossy(&buffer).into_owned());
            preview.prev_offset = (start > 0).then(|| start.saturating_sub(PREVIEW_PAGE_BYTES));
            preview.next_offset = (start.saturating_add(read) < size).then_some(start + read);
            preview.truncated = preview.next_offset.is_some() || start > 0;
        }
        other => return Err(format!("内容类型 {other:?} 暂不支持预览")),
    }
    Ok(preview)
}

#[tauri::command]
pub async fn hooks_attachment_preview(
    event_id: String,
    attachment_id: String,
    offset: Option<u64>,
) -> Result<AttachmentPreview, String> {
    crate::commands::blocking(move || {
        preview_attachment(&event_id, &attachment_id, offset.unwrap_or(0))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::blob;
    use crate::hooks::store::{NewAttachment, NewEvent};
    use crate::tests::{scoped_xlink_home, EnvGuard};
    use std::io::Write;
    use std::path::PathBuf;

    struct PreviewCase {
        home: PathBuf,
        event_id: String,
        attachment_id: String,
        rel: Option<String>,
    }

    impl Drop for PreviewCase {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.home);
        }
    }

    /// 临时 home + 一个带单附件的事件；upload 为假时只登记占位。
    fn seed(
        label: &str,
        kind: &str,
        media_type: &str,
        encoding: Option<&str>,
        bytes: &[u8],
        upload: bool,
    ) -> (PreviewCase, EnvGuard) {
        let home = std::env::temp_dir().join(format!(
            "dsh-xlink-hooks-preview-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|value| value.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&home).expect("创建临时 home");
        let guard = scoped_xlink_home(&home);
        let mut connection = store::open(&service::db_path()).expect("打开测试库");
        let (endpoint, _) =
            store::create_endpoint(&connection, "预览", &["text".to_string()]).expect("接入点");
        let spec = NewAttachment {
            client_id: "a",
            kind,
            filename: "file",
            media_type,
            encoding,
            size_bytes: bytes.len() as i64,
        };
        let parameters = serde_json::json!({});
        let event = NewEvent {
            external_id: "preview",
            fingerprint: "preview",
            event_type: "test",
            occurred_at: 0,
            title: "",
            text: "",
            parameters: &parameters,
            sender_policy: None,
            commit: false,
            attachments: std::slice::from_ref(&spec),
        };
        let (event, _) =
            store::create_event(&mut connection, &endpoint.id, &event, 1_000).expect("创建事件");
        let attachment = store::event_attachments(&connection, &event.id)
            .expect("附件")
            .remove(0);
        let mut rel = None;
        if upload {
            store::begin_upload(
                &connection,
                &event.id,
                &attachment.id,
                2_000,
                "test-attempt",
            )
            .expect("租约");
            let (mut file, temp) = blob::create_temp().expect("临时文件");
            file.write_all(bytes).expect("写临时文件");
            file.sync_all().expect("刷盘");
            drop(file);
            let placed = blob::place(&temp, &event.id, &attachment.id).expect("归档");
            store::finish_upload(
                &connection,
                &event.id,
                &attachment.id,
                "sha",
                bytes.len() as i64,
                &placed,
                2_000,
                "test-attempt",
            )
            .expect("完成上传");
            rel = Some(placed);
        }
        drop(connection);
        (
            PreviewCase {
                home,
                event_id: event.id,
                attachment_id: attachment.id,
                rel,
            },
            guard,
        )
    }

    #[test]
    fn log_preview_pages_and_never_claims_more_than_one_window() {
        let content: String = (0..12000)
            .map(|index| format!("line {index} ok\n"))
            .collect();
        let bytes = content.as_bytes();
        assert!(bytes.len() as u64 > PREVIEW_PAGE_BYTES);
        let (case, _guard) = seed("log", "log", "text/plain", Some("utf-8"), bytes, true);

        let first = preview_attachment(&case.event_id, &case.attachment_id, 0).expect("第一页");
        assert_eq!(first.kind, "log");
        assert_eq!(first.offset, 0);
        assert!(first.truncated);
        assert!(!first.invalid_utf8);
        let next = first.next_offset.expect("还有下一页");
        assert_eq!(next, PREVIEW_PAGE_BYTES);

        let second = preview_attachment(&case.event_id, &case.attachment_id, next).expect("第二页");
        assert_eq!(second.offset, next);
        assert_eq!(second.prev_offset, Some(0));
        let mut joined = first.text.expect("第一页文本");
        joined.push_str(&second.text.expect("第二页文本"));
        assert!(content.starts_with(&joined), "两页应首尾相接且不丢字节");
    }

    #[test]
    fn image_preview_returns_exact_bytes() {
        let bytes: &[u8] = &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, b'x'];
        let (case, _guard) = seed("image", "image", "image/png", None, bytes, true);
        let preview = preview_attachment(&case.event_id, &case.attachment_id, 0).expect("图片");
        let encoded = preview.data_base64.expect("base64");
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .expect("解码");
        assert_eq!(decoded, bytes);
        assert!(!preview.truncated);
        assert!(preview.text.is_none());
    }

    #[test]
    fn preview_refuses_pending_attachment() {
        let (case, _guard) = seed("pending", "log", "text/plain", Some("utf-8"), b"abc", false);
        let error = preview_attachment(&case.event_id, &case.attachment_id, 0)
            .expect_err("未归档的附件不能预览");
        assert!(error.contains("尚未归档"), "错误要说清状态：{error}");
    }

    #[test]
    fn preview_reports_missing_archived_file() {
        let (case, _guard) = seed("missing", "log", "text/plain", Some("utf-8"), b"abc", true);
        let rel = case.rel.clone().expect("归档路径");
        let path = blob::path_for_rel(&rel).expect("路径");
        std::fs::remove_file(path).expect("制造缺失文件");
        let error =
            preview_attachment(&case.event_id, &case.attachment_id, 0).expect_err("文件缺失");
        assert!(
            error.contains("无法读取附件文件"),
            "错误要指到文件：{error}"
        );
    }

    #[test]
    fn non_utf8_log_window_is_flagged_instead_of_silently_replaced() {
        let (case, _guard) = seed(
            "gb18030",
            "log",
            "text/plain",
            Some("gb18030"),
            &[0xff, 0xfe, 0x41],
            true,
        );
        let preview = preview_attachment(&case.event_id, &case.attachment_id, 0).expect("日志");
        assert!(preview.invalid_utf8, "解不出来的字节必须让面板知道");
        assert!(preview.text.expect("文本").contains('A'));
    }
}
