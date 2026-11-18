//! Hook 收件箱的 SQLite 存储：schema/迁移与全部仓库函数。
//!
//! P1 存元数据与纯文本正文，P2 增加附件占位、上传租约、配额与过期清理。
//! 所有函数只接收
//! `Connection`，不做网络、Tauri 或路径解析之外的副作用，便于用临时数据库
//! 直接测试。连接按调用打开（收件箱流量是每分钟几十条，不值得维护跨线程连接池），
//! 写并发由 WAL + `busy_timeout` 与 `IMMEDIATE` 事务串行化。

use std::path::Path;
use std::time::Duration;

use rusqlite::{params, types::Type, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// 数据库 schema 版本。只允许向前迁移；更高版本一律拒绝打开。
pub(crate) const DB_SCHEMA_VERSION: i64 = 5;

/// 上传租约：超过这个时间仍是 uploading 的附件可以被下一次上传接管。
pub(crate) const UPLOAD_LEASE_MS: i64 = 120 * 1000;
/// 墓碑窗口：过期事件清空内容后保留识别信息 30 天，用于幂等重试。
pub(crate) const TOMBSTONE_MS: i64 = 30 * 24 * 60 * 60 * 1000;
/// 默认存储预算 5 GiB。P2 固定值；P3 设置页再接可配置。
pub(crate) const STORAGE_BUDGET_BYTES: u64 = 5 * 1024 * 1024 * 1024;
/// 元数据、索引与小事件也占空间；行数上限限制空墓碑与碎片的持续增长。
const MAX_EVENT_ROWS: i64 = 10_000;
const EVENT_CONTENT_BYTES: &str = "content_bytes";

pub(crate) const ATTACHMENT_PENDING: &str = "pending";
pub(crate) const ATTACHMENT_UPLOADING: &str = "uploading";
pub(crate) const ATTACHMENT_STORED: &str = "stored";
pub(crate) const ATTACHMENT_FAILED: &str = "failed";

/// 未提交草稿的默认寿命：24 小时。
pub(crate) const RECEIVING_TTL_MS: i64 = 24 * 60 * 60 * 1000;
const SCHEMA_V1: &str = r#"
CREATE TABLE endpoints (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    adapter TEXT NOT NULL,
    enabled INTEGER NOT NULL,
    token_hash TEXT UNIQUE,
    allowed_kinds TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    last_event_at INTEGER,
    error_count INTEGER NOT NULL DEFAULT 0,
    deleted_at INTEGER
);

CREATE TABLE events (
    id TEXT PRIMARY KEY,
    endpoint_id TEXT NOT NULL REFERENCES endpoints(id),
    external_id TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    event_type TEXT NOT NULL,
    occurred_at INTEGER NOT NULL,
    received_at INTEGER NOT NULL,
    title TEXT NOT NULL,
    body_text TEXT NOT NULL,
    parameters TEXT NOT NULL,
    sender_policy TEXT,
    state TEXT NOT NULL,
    expires_at INTEGER,
    retention_until INTEGER,
    UNIQUE(endpoint_id, external_id)
);

CREATE INDEX events_received_at ON events(received_at DESC);
CREATE INDEX events_endpoint ON events(endpoint_id, received_at DESC);

CREATE TABLE request_audit (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    request_id TEXT NOT NULL,
    endpoint_id TEXT,
    event_id TEXT,
    code TEXT NOT NULL,
    at_ms INTEGER NOT NULL
);
"#;

/// v1 → v2：附件占位与上传状态。
const SCHEMA_V2: &str = r#"
CREATE TABLE attachments (
    id TEXT PRIMARY KEY,
    event_id TEXT NOT NULL REFERENCES events(id),
    client_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    filename TEXT NOT NULL,
    media_type TEXT NOT NULL,
    encoding TEXT,
    size_bytes INTEGER NOT NULL,
    state TEXT NOT NULL,
    sha256 TEXT,
    storage_rel TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(event_id, client_id)
);

CREATE INDEX attachments_event ON attachments(event_id);
CREATE INDEX attachments_state ON attachments(state);
"#;

/// v2 → v3：Jenkins Notification 适配器的独立 Basic 凭据（只存 SHA-256）。
const SCHEMA_V3: &str = r#"
ALTER TABLE endpoints ADD COLUMN notification_token_hash TEXT;
CREATE UNIQUE INDEX endpoints_notification_token
    ON endpoints(notification_token_hash)
    WHERE notification_token_hash IS NOT NULL;
"#;

/// v3 → v4：隔离并发上传租约，并一次性回填正文预算，避免每次接收扫描全文。
const SCHEMA_V4: &str = "ALTER TABLE attachments ADD COLUMN upload_owner TEXT;
ALTER TABLE events ADD COLUMN content_bytes INTEGER NOT NULL DEFAULT 1026;
UPDATE events SET content_bytes = 1024 + length(CAST(title AS BLOB)) + length(CAST(body_text AS BLOB)) + length(CAST(parameters AS BLOB)) + length(CAST(COALESCE(sender_policy, '') AS BLOB));";

/// v4 → v5：记录服务端实际观测到的投递方 IP（TCP 对端，不接受发送方自报）。
/// 升级前的历史事件为 NULL，面板如实显示「IP 未记录」。
const SCHEMA_V5: &str = "ALTER TABLE events ADD COLUMN source_ip TEXT;";

const MAX_AUDIT_ROWS: i64 = 2000;

#[derive(Debug)]
pub(crate) enum StoreError {
    /// 对象不存在或不属于该接入点；HTTP 映射 404，本地命令映射「不存在」。
    NotFound,
    /// 幂等键冲突或状态不允许；HTTP 映射 409。
    Conflict,
    /// 存储预算已满；HTTP 映射 413。
    QuotaExceeded(u64),
    /// 数据库打不开/读写出错；HTTP 映射 503。保留原始原因供日志与面板。
    Unavailable(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::NotFound => write!(f, "对象不存在"),
            StoreError::Conflict => write!(f, "幂等键或状态冲突"),
            StoreError::QuotaExceeded(limit) => {
                write!(f, "存储容量或事件数量已达上限，且没有可清理的历史事件（{limit} 字节 / {MAX_EVENT_ROWS} 条）；请完成或删除接收中的事件后重试")
            }
            StoreError::Unavailable(reason) => write!(f, "{reason}"),
        }
    }
}

impl From<rusqlite::Error> for StoreError {
    fn from(error: rusqlite::Error) -> Self {
        StoreError::Unavailable(error.to_string())
    }
}

impl From<std::io::Error> for StoreError {
    fn from(error: std::io::Error) -> Self {
        StoreError::Unavailable(error.to_string())
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct EndpointRow {
    pub id: String,
    pub name: String,
    pub adapter: String,
    pub enabled: bool,
    pub allowed_kinds: Vec<String>,
    pub created_at: i64,
    pub last_event_at: Option<i64>,
    pub error_count: i64,
    /// 该接入点的内容预算占用（正文、参数、元数据及附件预留）。
    pub bytes: u64,
    /// 是否已生成 Jenkins Notification 的独立 Basic 凭据。
    pub notification_enabled: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct EndpointAuth {
    pub id: String,
    pub enabled: bool,
    pub allowed_kinds: Vec<String>,
    pub deleted: bool,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct EventRow {
    pub id: String,
    pub endpoint_id: String,
    pub endpoint_name: String,
    pub external_id: String,
    pub event_type: String,
    pub title: String,
    pub body_text: String,
    pub parameters: serde_json::Value,
    pub sender_policy: Option<String>,
    /// 服务端观测到的投递方 IP；升级前的历史事件没有记录。
    pub source_ip: Option<String>,
    pub state: String,
    pub occurred_at: i64,
    pub received_at: i64,
    pub expires_at: Option<i64>,
    pub retention_until: Option<i64>,
    pub attachment_count: u32,
    /// 已归档的附件数：接收中事件用它显示「已到 / 总数」。
    pub stored_attachments: u32,
    /// 附件声明大小合计（与配额同一口径，含预留）。
    pub attachment_bytes: u64,
}

/// 列表只传摘要；正文和参数仅在打开详情时读取。
#[derive(Debug, Clone, Serialize)]
pub(crate) struct EventSummary {
    pub id: String,
    pub endpoint_id: String,
    pub endpoint_name: String,
    pub external_id: String,
    pub event_type: String,
    pub title: String,
    pub state: String,
    pub occurred_at: i64,
    pub received_at: i64,
    pub expires_at: Option<i64>,
    pub retention_until: Option<i64>,
    pub attachment_count: u32,
    pub stored_attachments: u32,
    pub attachment_bytes: u64,
}

/// 创建事件时登记的附件占位。文件字节随后经上传接口写入。
pub(crate) struct NewAttachment<'a> {
    pub client_id: &'a str,
    pub kind: &'a str,
    pub filename: &'a str,
    pub media_type: &'a str,
    pub encoding: Option<&'a str>,
    pub size_bytes: i64,
}

/// 已校验、待入库的事件。`fingerprint` 由 protocol 层按协议规范化后给出。
pub(crate) struct NewEvent<'a> {
    pub external_id: &'a str,
    pub fingerprint: &'a str,
    pub event_type: &'a str,
    pub occurred_at: i64,
    pub title: &'a str,
    pub text: &'a str,
    pub parameters: &'a serde_json::Value,
    pub sender_policy: Option<&'a str>,
    pub source_ip: Option<&'a str>,
    pub commit: bool,
    pub attachments: &'a [NewAttachment<'a>],
}

/// 附件行。`storage_rel` 是相对 Hook 数据目录的路径，由服务端生成，不接受外部输入。
#[derive(Debug, Clone, Serialize)]
pub(crate) struct AttachmentRow {
    pub id: String,
    pub event_id: String,
    pub client_id: String,
    pub kind: String,
    pub filename: String,
    pub media_type: String,
    pub encoding: Option<String>,
    pub size_bytes: i64,
    pub state: String,
    pub sha256: Option<String>,
    /// 内部相对路径；不进 IPC 响应（前端用 attachment_id 走受控预览命令）。
    #[serde(skip_serializing)]
    pub storage_rel: Option<String>,
}

/// 清理一个过期事件时返回的文件引用；先由数据库事务清空内容，再由调用方删文件。
#[derive(Debug, Clone)]
pub(crate) struct CleanupTarget {
    pub event_id: String,
    pub rels: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct EventFilter {
    pub endpoint_id: Option<String>,
    pub state: Option<String>,
    /// 按附件类型筛：`image` / `log`；组合内容不强行归成单一类型。
    pub attachment_kind: Option<String>,
    pub q: Option<String>,
    pub limit: usize,
    pub offset: usize,
}

/// 当前时间（毫秒）。测试通过显式传 `now_ms` 控制过期边界。
fn now_ms() -> i64 {
    crate::shell::process::epoch_millis() as i64
}

pub(crate) fn hash_token(token: &str) -> String {
    crate::shell::digest::hex(&Sha256::digest(token.as_bytes())[..])
}

/// 32 字节随机令牌的十六进制形式（256-bit）。只在创建/轮换时返回一次明文。
pub(crate) fn new_token() -> String {
    use rand::Rng;
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn new_id(prefix: &str) -> String {
    format!("{prefix}_{:016x}", rand::random::<u64>())
}

fn ensure_parent_dir(path: &Path) -> Result<(), StoreError> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    std::fs::create_dir_all(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
    }
    Ok(())
}

/// 打开并迁移数据库。数据库不存在时创建；更高版本的库拒绝打开，原文件不动。
pub(crate) fn open(path: &Path) -> Result<Connection, StoreError> {
    ensure_parent_dir(path)?;
    let mut connection = Connection::open(path)?;
    connection.busy_timeout(Duration::from_secs(2))?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    // journal_mode 会返回一行查询结果，用 query_row 接住，避免 pragma_update
    // 在部分 SQLite 版本上把「有返回值」当错误。
    let _: String = connection.query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    migrate(&mut connection)?;
    Ok(connection)
}

/// 只读场景（状态轮询、列表）用：库不存在时返回 `None`，不创建空文件。
pub(crate) fn open_if_exists(path: &Path) -> Result<Option<Connection>, StoreError> {
    if !path.is_file() {
        return Ok(None);
    }
    open(path).map(Some)
}

fn migrate(connection: &mut Connection) -> Result<(), StoreError> {
    let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    match version {
        0 => {
            let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            tx.execute_batch(SCHEMA_V1)?;
            tx.execute_batch(SCHEMA_V2)?;
            tx.execute_batch(SCHEMA_V3)?;
            tx.execute_batch(SCHEMA_V4)?;
            tx.execute_batch(SCHEMA_V5)?;
            tx.pragma_update(None, "user_version", DB_SCHEMA_VERSION)?;
            tx.commit()?;
            Ok(())
        }
        1 => {
            let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            tx.execute_batch(SCHEMA_V2)?;
            tx.execute_batch(SCHEMA_V3)?;
            tx.execute_batch(SCHEMA_V4)?;
            tx.execute_batch(SCHEMA_V5)?;
            tx.pragma_update(None, "user_version", DB_SCHEMA_VERSION)?;
            tx.commit()?;
            Ok(())
        }
        2 => {
            let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            tx.execute_batch(SCHEMA_V3)?;
            tx.execute_batch(SCHEMA_V4)?;
            tx.execute_batch(SCHEMA_V5)?;
            tx.pragma_update(None, "user_version", DB_SCHEMA_VERSION)?;
            tx.commit()?;
            Ok(())
        }
        3 => {
            let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            tx.execute_batch(SCHEMA_V4)?;
            tx.execute_batch(SCHEMA_V5)?;
            tx.pragma_update(None, "user_version", DB_SCHEMA_VERSION)?;
            tx.commit()?;
            Ok(())
        }
        4 => {
            let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            tx.execute_batch(SCHEMA_V5)?;
            tx.pragma_update(None, "user_version", DB_SCHEMA_VERSION)?;
            tx.commit()?;
            Ok(())
        }
        DB_SCHEMA_VERSION => Ok(()),
        newer => Err(StoreError::Unavailable(format!(
            "Hook 收件箱数据库版本 {newer} 高于本版本支持的 {DB_SCHEMA_VERSION}；             请升级应用或备份后移除该数据库（不会自动删除现有数据）"
        ))),
    }
}

fn endpoint_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<EndpointRow> {
    let allowed_kinds: String = row.get(4)?;
    let allowed_kinds = serde_json::from_str(&allowed_kinds).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(4, Type::Text, Box::new(error))
    })?;
    Ok(EndpointRow {
        id: row.get(0)?,
        name: row.get(1)?,
        adapter: row.get(2)?,
        enabled: row.get(3)?,
        allowed_kinds,
        created_at: row.get(5)?,
        last_event_at: row.get(6)?,
        error_count: row.get(7)?,
        bytes: row.get::<_, i64>(8)?.max(0) as u64,
        notification_enabled: row.get::<_, i64>(9)? != 0,
    })
}

const ENDPOINT_COLUMNS: &str =
    "id, name, adapter, enabled, allowed_kinds, created_at, last_event_at, error_count,
    (SELECT COALESCE(SUM(a.size_bytes), 0) FROM attachments a JOIN events e ON e.id = a.event_id
     WHERE e.endpoint_id = endpoints.id
       AND (a.state <> 'failed' OR a.storage_rel IS NOT NULL)) +
    (SELECT COALESCE(SUM(content_bytes), 0) FROM events WHERE endpoint_id = endpoints.id),
    (notification_token_hash IS NOT NULL)";

pub(crate) fn create_endpoint(
    connection: &Connection,
    name: &str,
    allowed_kinds: &[String],
) -> Result<(EndpointRow, String), StoreError> {
    let id = new_id("ep");
    let token = new_token();
    let kinds = serde_json::to_string(allowed_kinds)
        .map_err(|error| StoreError::Unavailable(error.to_string()))?;
    connection.execute(
        "INSERT INTO endpoints (id, name, adapter, enabled, token_hash, allowed_kinds, created_at, last_event_at, error_count, deleted_at)
         VALUES (?1, ?2, 'generic', 1, ?3, ?4, ?5, NULL, 0, NULL)",
        params![id, name, hash_token(&token), kinds, now_ms()],
    )?;
    let row = endpoint_by_id(connection, &id)?.ok_or(StoreError::NotFound)?;
    Ok((row, token))
}

pub(crate) fn list_endpoints(connection: &Connection) -> Result<Vec<EndpointRow>, StoreError> {
    let mut statement = connection.prepare(&format!(
        "SELECT {ENDPOINT_COLUMNS} FROM endpoints WHERE deleted_at IS NULL ORDER BY created_at ASC"
    ))?;
    let rows = statement
        .query_map([], endpoint_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub(crate) fn endpoint_by_id(
    connection: &Connection,
    id: &str,
) -> Result<Option<EndpointRow>, StoreError> {
    let mut statement = connection.prepare(&format!(
        "SELECT {ENDPOINT_COLUMNS} FROM endpoints WHERE id = ?1 AND deleted_at IS NULL"
    ))?;
    let row = statement
        .query_row(params![id], endpoint_from_row)
        .optional()?;
    Ok(row)
}

fn auth_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<EndpointAuth> {
    let kinds: String = row.get(2)?;
    let allowed_kinds = serde_json::from_str(&kinds).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(2, Type::Text, Box::new(error))
    })?;
    let deleted_at: Option<i64> = row.get(3)?;
    Ok(EndpointAuth {
        id: row.get(0)?,
        enabled: row.get(1)?,
        allowed_kinds,
        deleted: deleted_at.is_some(),
    })
}

/// 按凭据哈希列查接入点；列名只来自本模块常量，不接受外部输入。
fn auth_by_hash(
    connection: &Connection,
    column: &'static str,
    token_hash: &str,
) -> Result<Option<EndpointAuth>, StoreError> {
    let mut statement = connection.prepare(&format!(
        "SELECT id, enabled, allowed_kinds, deleted_at FROM endpoints WHERE {column} = ?1"
    ))?;
    Ok(statement
        .query_row(params![token_hash], auth_from_row)
        .optional()?)
}

pub(crate) fn auth_by_token_hash(
    connection: &Connection,
    token_hash: &str,
) -> Result<Option<EndpointAuth>, StoreError> {
    auth_by_hash(connection, "token_hash", token_hash)
}

pub(crate) fn set_endpoint_enabled(
    connection: &Connection,
    id: &str,
    enabled: bool,
) -> Result<EndpointRow, StoreError> {
    let changed = connection.execute(
        "UPDATE endpoints SET enabled = ?2 WHERE id = ?1 AND deleted_at IS NULL",
        params![id, enabled],
    )?;
    if changed == 0 {
        return Err(StoreError::NotFound);
    }
    endpoint_by_id(connection, id)?.ok_or(StoreError::NotFound)
}

pub(crate) fn rotate_endpoint_token(
    connection: &Connection,
    id: &str,
) -> Result<(EndpointRow, String), StoreError> {
    let token = new_token();
    let changed = connection.execute(
        "UPDATE endpoints SET token_hash = ?2 WHERE id = ?1 AND deleted_at IS NULL",
        params![id, hash_token(&token)],
    )?;
    if changed == 0 {
        return Err(StoreError::NotFound);
    }
    let row = endpoint_by_id(connection, id)?.ok_or(StoreError::NotFound)?;
    Ok((row, token))
}

/// 生成/轮换 Jenkins Notification 的独立 Basic 凭据；旧密码立即失效。
pub(crate) fn rotate_notification_token(
    connection: &Connection,
    id: &str,
) -> Result<(EndpointRow, String), StoreError> {
    let token = new_token();
    let changed = connection.execute(
        "UPDATE endpoints SET notification_token_hash = ?2 WHERE id = ?1 AND deleted_at IS NULL",
        params![id, hash_token(&token)],
    )?;
    if changed == 0 {
        return Err(StoreError::NotFound);
    }
    let row = endpoint_by_id(connection, id)?.ok_or(StoreError::NotFound)?;
    Ok((row, token))
}

/// Jenkins Notification 路由的 Basic 密码查找；与 Bearer 令牌分开，互不复用。
pub(crate) fn auth_by_notification_token_hash(
    connection: &Connection,
    token_hash: &str,
) -> Result<Option<EndpointAuth>, StoreError> {
    auth_by_hash(connection, "notification_token_hash", token_hash)
}

/// 软删除：立即作废凭据、停止接收，事件保留供本地查看。数据清理属于后续阶段。
pub(crate) fn delete_endpoint(connection: &Connection, id: &str) -> Result<(), StoreError> {
    let changed = connection.execute(
        "UPDATE endpoints SET enabled = 0, token_hash = NULL, notification_token_hash = NULL,
             deleted_at = ?2
         WHERE id = ?1 AND deleted_at IS NULL",
        params![id, now_ms()],
    )?;
    if changed == 0 {
        return Err(StoreError::NotFound);
    }
    Ok(())
}

fn expire_stale_on(
    connection: &Connection,
    endpoint_id: &str,
    at_ms: i64,
) -> Result<(), StoreError> {
    connection.execute(
        "UPDATE events SET state = 'expired'
         WHERE endpoint_id = ?1 AND state = 'receiving'
           AND expires_at IS NOT NULL AND expires_at <= ?2",
        params![endpoint_id, at_ms],
    )?;
    Ok(())
}

fn event_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<EventRow> {
    let parameters_text: String = row.get(7)?;
    let parameters = serde_json::from_str(&parameters_text).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(7, Type::Text, Box::new(error))
    })?;
    Ok(EventRow {
        id: row.get(0)?,
        endpoint_id: row.get(1)?,
        endpoint_name: row.get(2)?,
        external_id: row.get(3)?,
        event_type: row.get(4)?,
        title: row.get(5)?,
        body_text: row.get(6)?,
        parameters,
        sender_policy: row.get(8)?,
        source_ip: row.get(9)?,
        state: row.get(10)?,
        occurred_at: row.get(11)?,
        received_at: row.get(12)?,
        expires_at: row.get(13)?,
        retention_until: row.get(14)?,
        attachment_count: row.get::<_, i64>(15)?.max(0) as u32,
        stored_attachments: row.get::<_, i64>(16)?.max(0) as u32,
        attachment_bytes: row.get::<_, i64>(17)?.max(0) as u64,
    })
}

const EVENT_SELECT: &str = "SELECT e.id, e.endpoint_id, p.name, e.external_id, e.event_type,
        e.title, e.body_text, e.parameters, e.sender_policy, e.source_ip, e.state,
        e.occurred_at, e.received_at, e.expires_at, e.retention_until,
        (SELECT COUNT(*) FROM attachments a WHERE a.event_id = e.id),
        (SELECT COUNT(*) FROM attachments a WHERE a.event_id = e.id AND a.state = 'stored'),
        (SELECT COALESCE(SUM(a.size_bytes), 0) FROM attachments a
         WHERE a.event_id = e.id AND (a.state <> 'failed' OR a.storage_rel IS NOT NULL))
    FROM events e JOIN endpoints p ON p.id = e.endpoint_id";

const EVENT_SUMMARY_SELECT: &str =
    "SELECT e.id, e.endpoint_id, p.name, e.external_id, e.event_type,
        e.title, e.state, e.occurred_at, e.received_at, e.expires_at, e.retention_until,
        (SELECT COUNT(*) FROM attachments a WHERE a.event_id = e.id),
        (SELECT COUNT(*) FROM attachments a WHERE a.event_id = e.id AND a.state = 'stored'),
        (SELECT COALESCE(SUM(a.size_bytes), 0) FROM attachments a
         WHERE a.event_id = e.id AND (a.state <> 'failed' OR a.storage_rel IS NOT NULL))
    FROM events e JOIN endpoints p ON p.id = e.endpoint_id";

fn event_summary_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<EventSummary> {
    Ok(EventSummary {
        id: row.get(0)?,
        endpoint_id: row.get(1)?,
        endpoint_name: row.get(2)?,
        external_id: row.get(3)?,
        event_type: row.get(4)?,
        title: row.get(5)?,
        state: row.get(6)?,
        occurred_at: row.get(7)?,
        received_at: row.get(8)?,
        expires_at: row.get(9)?,
        retention_until: row.get(10)?,
        attachment_count: row.get::<_, i64>(11)?.max(0) as u32,
        stored_attachments: row.get::<_, i64>(12)?.max(0) as u32,
        attachment_bytes: row.get::<_, i64>(13)?.max(0) as u64,
    })
}

fn event_by_id(
    connection: &Connection,
    event_id: &str,
    endpoint_id: Option<&str>,
) -> Result<Option<EventRow>, StoreError> {
    let mut statement = connection.prepare(&format!(
        "{EVENT_SELECT} WHERE e.id = ?1 AND (?2 IS NULL OR e.endpoint_id = ?2)"
    ))?;
    let row = statement
        .query_row(params![event_id, endpoint_id], event_from_row)
        .optional()?;
    Ok(row)
}

pub(crate) fn event_detail(
    connection: &Connection,
    event_id: &str,
) -> Result<Option<EventRow>, StoreError> {
    event_by_id(connection, event_id, None)
}

/// 把一条事件的内容与附件行清空并标记为 `state`，返回需要删的文件路径。
fn clear_event_content(
    connection: &Connection,
    event_id: &str,
    state: &str,
) -> Result<Vec<String>, StoreError> {
    let rels = storage_rels(connection, event_id)?;
    connection.execute(
        "DELETE FROM attachments WHERE event_id = ?1",
        params![event_id],
    )?;
    connection.execute(
        "UPDATE events SET state = ?2, title = '', body_text = '', parameters = '{}',
             sender_policy = NULL, expires_at = NULL, retention_until = NULL, content_bytes = 1026
         WHERE id = ?1",
        params![event_id, state],
    )?;
    Ok(rels)
}

/// 清理最早的历史内容，纯文本与附件使用同一容量淘汰规则。
fn evict_oldest_event(connection: &Connection) -> Result<Option<(CleanupTarget, u64)>, StoreError> {
    let event_id: Option<String> = connection
        .query_row(
            "SELECT e.id FROM events e
             WHERE e.state IN ('ready', 'unavailable', 'expired')
               AND (e.state IN ('ready', 'unavailable') OR e.title <> '' OR e.body_text <> '' OR e.parameters <> '{}' OR e.sender_policy IS NOT NULL
                    OR EXISTS (SELECT 1 FROM attachments a WHERE a.event_id = e.id
                           AND (a.state <> ?1 OR a.storage_rel IS NOT NULL)))
             ORDER BY e.received_at ASC, e.rowid ASC
             LIMIT 1",
            params![ATTACHMENT_FAILED],
            |row| row.get(0),
        )
        .optional()?;
    let Some(event_id) = event_id else {
        return Ok(None);
    };
    let freed: i64 = connection.query_row(
        "SELECT COALESCE(SUM(size_bytes), 0) FROM attachments
         WHERE event_id = ?1 AND (state <> ?2 OR storage_rel IS NOT NULL)",
        params![event_id, ATTACHMENT_FAILED],
        |row| row.get(0),
    )?;
    let content: i64 = connection.query_row(
        &format!("SELECT {EVENT_CONTENT_BYTES} FROM events WHERE id = ?1"),
        params![event_id],
        |row| row.get(0),
    )?;
    // 清空后仍保留 1024 字节元数据预留和两个字节的空参数对象。
    let freed = freed.saturating_add(content.saturating_sub(1026));
    let rels = clear_event_content(connection, &event_id, crate::hooks::protocol::STATE_EVICTED)?;
    Ok(Some((
        CleanupTarget { event_id, rels },
        freed.max(0) as u64,
    )))
}

/// 测试专用：丢弃容量清理返回的文件引用。网络接收路径用
/// [`create_event_evicting`]，由调用方在事务提交后删除返回的附件文件。
#[cfg(test)]
pub(crate) fn create_event(
    connection: &mut Connection,
    endpoint_id: &str,
    event: &NewEvent<'_>,
    at_ms: i64,
) -> Result<(EventRow, bool), StoreError> {
    let (row, created, _) = create_event_evicting(connection, endpoint_id, event, at_ms)?;
    Ok((row, created))
}

/// 创建事件或返回已有事件。成功响应只能在事务提交后发出：`Ok` 返回时数据
/// 已经落盘（WAL 同步）。容量超过 5 GiB 时按 received_at 从旧到新清理
/// ready/unavailable/expired 事件的内容与附件；返回的路径由调用方在提交后删除。
pub(crate) fn create_event_evicting(
    connection: &mut Connection,
    endpoint_id: &str,
    event: &NewEvent<'_>,
    at_ms: i64,
) -> Result<(EventRow, bool, Vec<CleanupTarget>), StoreError> {
    let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    expire_stale_on(&tx, endpoint_id, at_ms)?;
    let existing: Option<(String, String)> = tx
        .query_row(
            "SELECT id, fingerprint FROM events WHERE endpoint_id = ?1 AND external_id = ?2",
            params![endpoint_id, event.external_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    if let Some((event_id, fingerprint)) = existing {
        if fingerprint != event.fingerprint {
            return Err(StoreError::Conflict);
        }
        let row = event_by_id(&tx, &event_id, Some(endpoint_id))?.ok_or(StoreError::NotFound)?;
        tx.commit()?;
        return Ok((row, false, Vec::new()));
    }

    let state = if event.commit {
        crate::hooks::protocol::STATE_READY
    } else {
        crate::hooks::protocol::STATE_RECEIVING
    };
    let expires_at = if event.commit {
        None
    } else {
        Some(at_ms + RECEIVING_TTL_MS)
    };
    let parameters = serde_json::to_string(event.parameters)
        .map_err(|error| StoreError::Unavailable(error.to_string()))?;
    let content_bytes = 1024
        + event.title.len()
        + event.text.len()
        + parameters.len()
        + event.sender_policy.map(str::len).unwrap_or(0);
    let declared = event
        .attachments
        .iter()
        .fold(content_bytes as u64, |sum, attachment| {
            sum.saturating_add(attachment.size_bytes.max(0) as u64)
        });
    let mut used = used_bytes(&tx)?;
    // 容量是全局的：顺手把任何接入点上已超时的接收中事件标成 expired，
    // 让它们进入容量清理候选，避免别的接入点的废弃上传卡住本次创建。
    tx.execute(
        "UPDATE events SET state = 'expired'
         WHERE state = 'receiving' AND expires_at IS NOT NULL AND expires_at <= ?1",
        params![at_ms],
    )?;
    let mut evicted = Vec::new();
    while used.saturating_add(declared) > STORAGE_BUDGET_BYTES {
        let Some((target, freed)) = evict_oldest_event(&tx)? else {
            return Err(StoreError::QuotaExceeded(STORAGE_BUDGET_BYTES));
        };
        evicted.push(target);
        used = used.saturating_sub(freed);
    }
    enforce_event_limit(&tx, MAX_EVENT_ROWS - 1, &mut evicted)?;
    let id = new_id("evt");
    tx.execute(
        "INSERT INTO events (id, endpoint_id, external_id, fingerprint, event_type,
             occurred_at, received_at, title, body_text, parameters, sender_policy,
             source_ip, state, expires_at, retention_until, content_bytes)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, NULL, ?15)",
        params![
            id,
            endpoint_id,
            event.external_id,
            event.fingerprint,
            event.event_type,
            event.occurred_at,
            at_ms,
            event.title,
            event.text,
            parameters,
            event.sender_policy,
            event.source_ip,
            state,
            expires_at,
            content_bytes as i64,
        ],
    )?;
    tx.execute(
        "UPDATE endpoints SET last_event_at = ?2 WHERE id = ?1",
        params![endpoint_id, at_ms],
    )?;
    for attachment in event.attachments {
        let attachment_id = new_id("att");
        tx.execute(
            "INSERT INTO attachments (id, event_id, client_id, kind, filename, media_type,
                 encoding, size_bytes, state, sha256, storage_rel, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL, NULL, ?10, ?10)",
            params![
                attachment_id,
                id,
                attachment.client_id,
                attachment.kind,
                attachment.filename,
                attachment.media_type,
                attachment.encoding,
                attachment.size_bytes,
                ATTACHMENT_PENDING,
                at_ms,
            ],
        )?;
    }
    let row = event_by_id(&tx, &id, Some(endpoint_id))?.ok_or(StoreError::NotFound)?;
    tx.commit()?;
    Ok((row, true, evicted))
}

pub(crate) fn commit_event(
    connection: &mut Connection,
    endpoint_id: &str,
    event_id: &str,
    at_ms: i64,
) -> Result<EventRow, StoreError> {
    let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    expire_stale_on(&tx, endpoint_id, at_ms)?;
    let row = event_by_id(&tx, event_id, Some(endpoint_id))?.ok_or(StoreError::NotFound)?;
    match row.state.as_str() {
        crate::hooks::protocol::STATE_READY => {
            tx.commit()?;
            Ok(row)
        }
        crate::hooks::protocol::STATE_RECEIVING => {
            let pending: i64 = tx.query_row(
                "SELECT COUNT(*) FROM attachments WHERE event_id = ?1 AND state <> ?2",
                params![event_id, ATTACHMENT_STORED],
                |row| row.get(0),
            )?;
            if pending > 0 {
                return Err(StoreError::Conflict);
            }
            tx.execute(
                "UPDATE events SET state = 'ready', expires_at = NULL
                 WHERE id = ?1",
                params![event_id],
            )?;
            let row = event_by_id(&tx, event_id, Some(endpoint_id))?.ok_or(StoreError::NotFound)?;
            tx.commit()?;
            Ok(row)
        }
        _ => Err(StoreError::Conflict),
    }
}

pub(crate) fn receipt_event(
    connection: &Connection,
    endpoint_id: &str,
    event_id: &str,
    at_ms: i64,
) -> Result<EventRow, StoreError> {
    expire_stale_on(connection, endpoint_id, at_ms)?;
    event_by_id(connection, event_id, Some(endpoint_id))?.ok_or(StoreError::NotFound)
}

const ATTACHMENT_COLUMNS: &str =
    "id, event_id, client_id, kind, filename, media_type, encoding, size_bytes, state, sha256, storage_rel";

fn attachment_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AttachmentRow> {
    Ok(AttachmentRow {
        id: row.get(0)?,
        event_id: row.get(1)?,
        client_id: row.get(2)?,
        kind: row.get(3)?,
        filename: row.get(4)?,
        media_type: row.get(5)?,
        encoding: row.get(6)?,
        size_bytes: row.get(7)?,
        state: row.get(8)?,
        sha256: row.get(9)?,
        storage_rel: row.get(10)?,
    })
}

pub(crate) fn event_attachments(
    connection: &Connection,
    event_id: &str,
) -> Result<Vec<AttachmentRow>, StoreError> {
    let mut statement = connection.prepare(&format!(
        "SELECT {ATTACHMENT_COLUMNS} FROM attachments WHERE event_id = ?1 ORDER BY client_id ASC"
    ))?;
    let rows = statement
        .query_map(params![event_id], attachment_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub(crate) fn attachment(
    connection: &Connection,
    event_id: &str,
    attachment_id: &str,
) -> Result<Option<AttachmentRow>, StoreError> {
    let mut statement = connection.prepare(&format!(
        "SELECT {ATTACHMENT_COLUMNS} FROM attachments WHERE id = ?1 AND event_id = ?2"
    ))?;
    let row = statement
        .query_row(params![attachment_id, event_id], attachment_from_row)
        .optional()?;
    Ok(row)
}

/// 取上传租约：pending 或过期 uploading 可被接管；返回附件行（含声明大小）。
pub(crate) fn begin_upload(
    connection: &Connection,
    event_id: &str,
    attachment_id: &str,
    at_ms: i64,
    owner: &str,
) -> Result<AttachmentRow, StoreError> {
    let changed = connection.execute(
        "UPDATE attachments SET state = ?3, updated_at = ?4, upload_owner = ?7
         WHERE id = ?1 AND event_id = ?2
           AND (state = ?5 OR (state = ?3 AND updated_at < ?6))",
        params![
            attachment_id,
            event_id,
            ATTACHMENT_UPLOADING,
            at_ms,
            ATTACHMENT_PENDING,
            at_ms - UPLOAD_LEASE_MS,
            owner,
        ],
    )?;
    if changed == 0 {
        return match attachment(connection, event_id, attachment_id)? {
            Some(_) => Err(StoreError::Conflict),
            None => Err(StoreError::NotFound),
        };
    }
    attachment(connection, event_id, attachment_id)?.ok_or(StoreError::NotFound)
}

/// 上传完成：只有 uploading 状态且实际大小等于声明大小才可置 stored。
#[allow(clippy::too_many_arguments)]
pub(crate) fn finish_upload(
    connection: &Connection,
    event_id: &str,
    attachment_id: &str,
    sha256: &str,
    size_bytes: i64,
    storage_rel: &str,
    at_ms: i64,
    owner: &str,
) -> Result<AttachmentRow, StoreError> {
    let changed = connection.execute(
        "UPDATE attachments SET state = ?3, sha256 = ?4, storage_rel = ?5, updated_at = ?6, upload_owner = NULL
         WHERE id = ?1 AND event_id = ?2 AND state = ?7 AND size_bytes = ?8 AND upload_owner = ?9",
        params![
            attachment_id,
            event_id,
            ATTACHMENT_STORED,
            sha256,
            storage_rel,
            at_ms,
            ATTACHMENT_UPLOADING,
            size_bytes,
            owner,
        ],
    )?;
    if changed == 0 {
        return Err(StoreError::Conflict);
    }
    attachment(connection, event_id, attachment_id)?.ok_or(StoreError::NotFound)
}

/// 上传失败后把租约放回 pending，允许发送方重试（例如校验失败或连接中断）。
pub(crate) fn abort_upload(
    connection: &Connection,
    event_id: &str,
    attachment_id: &str,
    at_ms: i64,
    owner: &str,
) -> Result<(), StoreError> {
    connection.execute(
        "UPDATE attachments SET state = ?3, updated_at = ?4, upload_owner = NULL
         WHERE id = ?1 AND event_id = ?2 AND state = ?5 AND upload_owner = ?6",
        params![
            attachment_id,
            event_id,
            ATTACHMENT_PENDING,
            at_ms,
            ATTACHMENT_UPLOADING,
            owner,
        ],
    )?;
    Ok(())
}

/// 恢复检查发现文件缺失时：附件失败、事件不可处理（不伪造 ready）。
pub(crate) fn mark_event_unavailable(
    connection: &Connection,
    event_id: &str,
) -> Result<bool, StoreError> {
    let changed = connection.execute(
        "UPDATE events SET state = 'unavailable', expires_at = NULL
         WHERE id = ?1 AND state <> 'unavailable' AND state <> 'expired'",
        params![event_id],
    )?;
    connection.execute(
        "UPDATE attachments SET state = ?2 WHERE event_id = ?1",
        params![event_id, ATTACHMENT_FAILED],
    )?;
    Ok(changed > 0)
}

/// 所有已落盘附件（启动恢复用）：(event_id, attachment_id, storage_rel, size)。
pub(crate) fn stored_attachments(
    connection: &Connection,
) -> Result<Vec<(String, String, String, i64)>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT event_id, id, storage_rel, size_bytes FROM attachments
         WHERE state = ?1 AND storage_rel IS NOT NULL",
    )?;
    let rows = statement
        .query_map(params![ATTACHMENT_STORED], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// 数据库中登记的全部文件路径（孤儿文件扫描的对照集）。
pub(crate) fn known_storage_rels(
    connection: &Connection,
) -> Result<std::collections::HashSet<String>, StoreError> {
    let mut statement =
        connection.prepare("SELECT storage_rel FROM attachments WHERE storage_rel IS NOT NULL")?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<std::collections::HashSet<_>>>()?;
    Ok(rows)
}

fn storage_rels(connection: &Connection, event_id: &str) -> Result<Vec<String>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT storage_rel FROM attachments
         WHERE event_id = ?1 AND storage_rel IS NOT NULL",
    )?;
    let rows = statement
        .query_map(params![event_id], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// 手动永久删除一个事件：返回需要删的附件文件。设计上只允许本地管理面调用，
/// 删除范围（事件 + 附件 + 文件）由调用方一次性展示给用户确认（P3）。
pub(crate) fn delete_event(
    connection: &mut Connection,
    event_id: &str,
) -> Result<Vec<String>, StoreError> {
    let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let exists: Option<i64> = tx
        .query_row(
            "SELECT 1 FROM events WHERE id = ?1",
            params![event_id],
            |row| row.get(0),
        )
        .optional()?;
    if exists.is_none() {
        return Err(StoreError::NotFound);
    }
    let rels = storage_rels(&tx, event_id)?;
    tx.execute(
        "DELETE FROM attachments WHERE event_id = ?1",
        params![event_id],
    )?;
    tx.execute("DELETE FROM events WHERE id = ?1", params![event_id])?;
    tx.commit()?;
    Ok(rels)
}

/// 内容预算占用，不等同于 SQLite 文件物理大小；删除后的页由 SQLite 复用。
pub(crate) fn used_bytes(connection: &Connection) -> Result<u64, StoreError> {
    let total: i64 = connection.query_row(
        &format!(
            "SELECT (SELECT COALESCE(SUM(size_bytes), 0) FROM attachments
         WHERE state <> ?1 OR storage_rel IS NOT NULL) +
         (SELECT COALESCE(SUM({EVENT_CONTENT_BYTES}), 0) FROM events)"
        ),
        params![ATTACHMENT_FAILED],
        |row| row.get(0),
    )?;
    Ok(total.max(0) as u64)
}

/// 容量压力下优先移除空墓碑；否则淘汰最旧历史内容，不删除活跃接收事件。
/// 因此幂等墓碑最多保留 30 天，同时受总行数限制。
fn enforce_event_limit(
    connection: &Connection,
    limit: i64,
    targets: &mut Vec<CleanupTarget>,
) -> Result<(), StoreError> {
    let mut count: i64 =
        connection.query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))?;
    while count > limit {
        let removed = connection.execute(
            "DELETE FROM events WHERE id = (SELECT e.id FROM events e
             WHERE e.state IN ('expired', 'evicted') AND title = '' AND body_text = ''
             AND parameters = '{}' AND NOT EXISTS (SELECT 1 FROM attachments a WHERE a.event_id = e.id)
             ORDER BY received_at, rowid LIMIT 1)", [],
        )?;
        if removed > 0 {
            count -= removed as i64;
        } else if let Some((target, _)) = evict_oldest_event(connection)? {
            targets.push(target);
        } else {
            return Err(StoreError::QuotaExceeded(STORAGE_BUDGET_BYTES));
        }
    }
    Ok(())
}

/// 后台清理：只处理未完成上传的超时事件，以及墓碑窗口外的空事件。
///
/// 已入箱内容不再按时间删除；容量超过 5 GiB 时由 `create_event_evicting`
/// 从最早的历史事件开始清理。文件删除由调用方做；失败时附件行已清、文件成为
/// 孤儿，下一轮按宽限期回收，不会再把内容当可处理事件提供。
pub(crate) fn cleanup_content(
    connection: &mut Connection,
    at_ms: i64,
) -> Result<Vec<CleanupTarget>, StoreError> {
    let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute(
        "UPDATE events SET state = 'expired'
         WHERE state = 'receiving' AND expires_at IS NOT NULL AND expires_at <= ?1",
        params![at_ms],
    )?;
    let event_ids: Vec<String> = {
        let mut statement = tx.prepare(
            "SELECT e.id FROM events e
             WHERE e.state = 'expired'
               AND (e.title <> '' OR e.body_text <> '' OR e.parameters <> '{}'
                    OR EXISTS (SELECT 1 FROM attachments a WHERE a.event_id = e.id))",
        )?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };
    let mut targets = Vec::with_capacity(event_ids.len());
    for event_id in event_ids {
        let rels = clear_event_content(&tx, &event_id, "expired")?;
        targets.push(CleanupTarget { event_id, rels });
    }
    // 容量是唯一的时间无关清理依据：即使没有新事件，维护线程也要把占用降回预算。
    let mut used = used_bytes(&tx)?;
    while used > STORAGE_BUDGET_BYTES {
        let Some((target, freed)) = evict_oldest_event(&tx)? else {
            break;
        };
        used = used.saturating_sub(freed);
        targets.push(target);
    }
    enforce_event_limit(&tx, MAX_EVENT_ROWS, &mut targets)?;
    // 墓碑窗口之外、已经没有内容的过期 / 容量清理事件直接删行；窗口内仍可用
    // external_id 命中并返回 expired / evicted，超出窗口不承诺去重。
    tx.execute(
        "DELETE FROM events
         WHERE state IN ('expired', 'evicted') AND received_at <= ?1
           AND title = '' AND body_text = '' AND parameters = '{}'
           AND NOT EXISTS (SELECT 1 FROM attachments a WHERE a.event_id = events.id)",
        params![at_ms - TOMBSTONE_MS],
    )?;
    tx.commit()?;
    Ok(targets)
}

fn like_pattern(query: &str) -> String {
    let mut escaped = String::with_capacity(query.len() + 2);
    for ch in query.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    format!("%{escaped}%")
}

pub(crate) fn list_events(
    connection: &Connection,
    filter: &EventFilter,
) -> Result<(Vec<EventSummary>, u64), StoreError> {
    let limit = filter.limit.clamp(1, 100) as i64;
    let offset = filter.offset as i64;
    let pattern = filter.q.as_deref().map(like_pattern);
    let where_sql = "WHERE (?1 IS NULL OR e.endpoint_id = ?1)
          AND (?2 IS NULL OR e.state = ?2)
          AND (?3 IS NULL OR e.title LIKE ?4 ESCAPE '\\' OR e.external_id LIKE ?4 ESCAPE '\\')
          AND (?5 IS NULL OR EXISTS (SELECT 1 FROM attachments a WHERE a.event_id = e.id AND a.kind = ?5))";
    let mut statement = connection.prepare(&format!(
        "{EVENT_SUMMARY_SELECT} {where_sql} ORDER BY e.received_at DESC, e.rowid DESC LIMIT ?6 OFFSET ?7"
    ))?;
    let items = statement
        .query_map(
            params![
                filter.endpoint_id.as_deref(),
                filter.state.as_deref(),
                filter.q.as_deref(),
                pattern.as_deref(),
                filter.attachment_kind.as_deref(),
                limit,
                offset
            ],
            event_summary_from_row,
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let total: i64 = connection.query_row(
        &format!("SELECT COUNT(*) FROM events e {where_sql}"),
        params![
            filter.endpoint_id.as_deref(),
            filter.state.as_deref(),
            filter.q.as_deref(),
            pattern.as_deref(),
            filter.attachment_kind.as_deref()
        ],
        |row| row.get(0),
    )?;
    Ok((items, total.max(0) as u64))
}

pub(crate) fn counts(connection: &Connection) -> Result<(u64, u64), StoreError> {
    let endpoints: i64 = connection.query_row(
        "SELECT COUNT(*) FROM endpoints WHERE deleted_at IS NULL",
        [],
        |row| row.get(0),
    )?;
    let events: i64 = connection.query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))?;
    Ok((endpoints.max(0) as u64, events.max(0) as u64))
}

/// 请求错误审计：只记 ID 与错误码，不记令牌或正文。审计写失败不影响请求。
pub(crate) fn record_error(
    connection: &Connection,
    request_id: &str,
    endpoint_id: Option<&str>,
    event_id: Option<&str>,
    code: &str,
) {
    let at = now_ms();
    let _ = connection.execute(
        "INSERT INTO request_audit (request_id, endpoint_id, event_id, code, at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![request_id, endpoint_id, event_id, code, at],
    );
    if let Some(endpoint_id) = endpoint_id {
        let _ = connection.execute(
            "UPDATE endpoints SET error_count = error_count + 1 WHERE id = ?1",
            params![endpoint_id],
        );
    }
    let _ = connection.execute(
        "DELETE FROM request_audit WHERE id NOT IN
            (SELECT id FROM request_audit ORDER BY id DESC LIMIT ?1)",
        params![MAX_AUDIT_ROWS],
    );
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct TempHome(PathBuf);

    impl TempHome {
        fn new(label: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "dsh-xlink-hooks-store-{label}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|value| value.as_nanos())
                    .unwrap_or(0)
            ));
            std::fs::create_dir_all(&dir).expect("创建临时目录");
            Self(dir)
        }

        fn db(&self) -> PathBuf {
            self.0.join("inbox.db")
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn open_db(home: &TempHome) -> Connection {
        super::open(&home.db()).expect("打开测试库")
    }

    fn endpoint(connection: &Connection) -> (String, String) {
        let (row, token) = create_endpoint(
            connection,
            "测试接入点",
            &["text".to_string(), "parameters".to_string()],
        )
        .expect("创建接入点");
        (row.id, token)
    }

    fn parameters() -> &'static serde_json::Value {
        static PARAMETERS: std::sync::OnceLock<serde_json::Value> = std::sync::OnceLock::new();
        PARAMETERS.get_or_init(|| serde_json::json!({"build": 128}))
    }

    fn new_event<'a>(
        external_id: &'a str,
        fingerprint: &'a str,
        text: &'a str,
        commit: bool,
    ) -> NewEvent<'a> {
        NewEvent {
            external_id,
            fingerprint,
            event_type: "build.materials",
            occurred_at: 1_763_172_000_000,
            title: "Project_Plague Win64 #128",
            text,
            parameters: parameters(),
            sender_policy: Some("sanitized"),
            source_ip: None,
            commit,
            attachments: &[],
        }
    }

    #[test]
    fn schema_is_created_and_newer_version_is_refused() {
        let home = TempHome::new("schema");
        let connection = open_db(&home);
        assert_eq!(counts(&connection).expect("计数"), (0, 0));
        drop(connection);

        let connection = Connection::open(home.db()).expect("直接打开");
        connection
            .pragma_update(None, "user_version", DB_SCHEMA_VERSION + 1)
            .expect("模拟更高版本");
        drop(connection);
        assert!(matches!(open(&home.db()), Err(StoreError::Unavailable(_))));
    }

    #[test]
    fn tokens_are_hashed_and_rotation_invalidates_old_token() {
        let home = TempHome::new("token");
        let connection = open_db(&home);
        let (endpoint_id, token) = endpoint(&connection);
        assert_eq!(token.len(), 64);
        let old_hash = hash_token(&token);
        let auth = auth_by_token_hash(&connection, &old_hash)
            .expect("查询")
            .expect("旧令牌应命中");
        assert_eq!(auth.id, endpoint_id);
        assert!(!auth.deleted);

        let (_, new_token) = rotate_endpoint_token(&connection, &endpoint_id).expect("轮换");
        assert_ne!(token, new_token);
        assert!(auth_by_token_hash(&connection, &old_hash)
            .expect("查询")
            .is_none());
        assert!(auth_by_token_hash(&connection, &hash_token(&new_token))
            .expect("查询")
            .is_some());
    }

    #[test]
    fn create_event_is_idempotent_and_detects_conflict() {
        let home = TempHome::new("idempotent");
        let mut connection = open_db(&home);
        let (endpoint_id, _) = endpoint(&connection);
        let first = new_event("build:1", "fingerprint-a", "第一次内容", true);
        let (row, created) =
            create_event(&mut connection, &endpoint_id, &first, 1_000).expect("创建");
        assert!(created);
        assert_eq!(row.state, "ready");

        let (replay, created) =
            create_event(&mut connection, &endpoint_id, &first, 2_000).expect("重放");
        assert!(!created);
        assert_eq!(replay.id, row.id);
        assert_eq!(replay.received_at, 1_000, "重放不得改写原事件时间");

        let changed = new_event("build:1", "fingerprint-b", "第二次内容", true);
        assert!(matches!(
            create_event(&mut connection, &endpoint_id, &changed, 3_000),
            Err(StoreError::Conflict)
        ));
        assert_eq!(counts(&connection).expect("计数").1, 1);
    }

    #[test]
    fn commit_and_expiry_transitions_are_enforced() {
        let home = TempHome::new("commit");
        let mut connection = open_db(&home);
        let (endpoint_id, _) = endpoint(&connection);
        let draft = new_event("build:draft", "fingerprint-draft", "草稿", false);
        let (row, _) =
            create_event(&mut connection, &endpoint_id, &draft, 1_000).expect("创建草稿");
        assert_eq!(row.state, "receiving");
        assert_eq!(row.expires_at, Some(1_000 + RECEIVING_TTL_MS));

        let committed = commit_event(&mut connection, &endpoint_id, &row.id, 2_000).expect("提交");
        assert_eq!(committed.state, "ready");
        assert_eq!(committed.expires_at, None);
        assert_eq!(committed.retention_until, None, "已入箱不再按固定时间过期");
        let again = commit_event(&mut connection, &endpoint_id, &row.id, 3_000).expect("重复提交");
        assert_eq!(again.state, "ready");

        let expired_draft = new_event("build:expired", "fingerprint-expired", "过期草稿", false);
        let (expired, _) =
            create_event(&mut connection, &endpoint_id, &expired_draft, 10_000).expect("创建");
        let receipt = receipt_event(
            &connection,
            &endpoint_id,
            &expired.id,
            10_000 + RECEIVING_TTL_MS + 1,
        )
        .expect("查询过期草稿");
        assert_eq!(receipt.state, "expired");
        assert!(matches!(
            commit_event(
                &mut connection,
                &endpoint_id,
                &expired.id,
                10_000 + RECEIVING_TTL_MS + 2
            ),
            Err(StoreError::Conflict)
        ));
    }

    #[test]
    fn cross_endpoint_access_is_not_found() {
        let home = TempHome::new("cross");
        let mut connection = open_db(&home);
        let (endpoint_a, _) = endpoint(&connection);
        let (endpoint_b, _) = endpoint(&connection);
        let event = new_event("build:cross", "fingerprint-cross", "跨端点", true);
        let (row, _) = create_event(&mut connection, &endpoint_a, &event, 1_000).expect("创建");
        assert!(matches!(
            receipt_event(&connection, &endpoint_b, &row.id, 2_000),
            Err(StoreError::NotFound)
        ));
        assert!(matches!(
            commit_event(&mut connection, &endpoint_b, &row.id, 2_000),
            Err(StoreError::NotFound)
        ));
        assert!(event_detail(&connection, &row.id)
            .expect("本地详情")
            .is_some());
    }

    #[test]
    fn soft_delete_invalidates_credentials_and_keeps_events() {
        let home = TempHome::new("delete");
        let mut connection = open_db(&home);
        let (endpoint_id, token) = endpoint(&connection);
        let event = new_event("build:deleted", "fingerprint-deleted", "保留事件", true);
        let (row, _) = create_event(&mut connection, &endpoint_id, &event, 1_000).expect("创建");
        delete_endpoint(&connection, &endpoint_id).expect("软删除");
        assert!(auth_by_token_hash(&connection, &hash_token(&token))
            .expect("查询")
            .is_none());
        assert!(list_endpoints(&connection).expect("列表").is_empty());
        assert!(event_detail(&connection, &row.id).expect("详情").is_some());
    }

    #[test]
    fn event_keeps_observed_source_ip() {
        let home = TempHome::new("source-ip");
        let mut connection = open_db(&home);
        let (endpoint_id, _) = endpoint(&connection);
        let mut event = new_event("build:source-ip", "fingerprint-source-ip", "来源", true);
        event.source_ip = Some("192.168.110.176");
        let (row, _) = create_event(&mut connection, &endpoint_id, &event, 1_000).expect("创建");
        assert_eq!(row.source_ip.as_deref(), Some("192.168.110.176"));
        let stored = event_detail(&connection, &row.id)
            .expect("详情")
            .expect("事件存在");
        assert_eq!(stored.source_ip.as_deref(), Some("192.168.110.176"));
    }

    #[test]
    fn list_events_filters_and_counts() {
        let home = TempHome::new("list");
        let mut connection = open_db(&home);
        let (endpoint_id, _) = endpoint(&connection);
        for (index, (external_id, state_text)) in
            [("build:ready", "就绪事件"), ("build:other", "另一条")]
                .into_iter()
                .enumerate()
        {
            let commit = index == 0;
            let event = new_event(external_id, external_id, state_text, commit);
            create_event(&mut connection, &endpoint_id, &event, 1_000 + index as i64)
                .expect("创建");
        }
        let (all, total) = list_events(
            &connection,
            &EventFilter {
                limit: 50,
                ..EventFilter::default()
            },
        )
        .expect("列表");
        assert_eq!(total, 2);
        assert_eq!(all.len(), 2);
        let summary = serde_json::to_value(&all[0]).expect("摘要序列化");
        assert!(summary.get("body_text").is_none());
        assert!(summary.get("parameters").is_none());
        // 即使历史参数损坏，列表仍可浏览；详情校验仍由详情入口负责。
        connection
            .execute("UPDATE events SET parameters = 'invalid json'", [])
            .expect("模拟损坏参数");

        let (ready, total) = list_events(
            &connection,
            &EventFilter {
                state: Some("ready".into()),
                limit: 50,
                ..EventFilter::default()
            },
        )
        .expect("按状态");
        assert_eq!(total, 1);
        assert_eq!(ready[0].external_id, "build:ready");

        let (searched, total) = list_events(
            &connection,
            &EventFilter {
                q: Some("build:other".into()),
                limit: 50,
                ..EventFilter::default()
            },
        )
        .expect("搜索");
        assert_eq!(total, 1);
        assert_eq!(searched[0].external_id, "build:other");
    }

    #[test]
    fn inbox_filters_attachment_kind_and_reports_stored_progress() {
        let home = TempHome::new("list-kind");
        let mut connection = open_db(&home);
        let (endpoint_id, _) = endpoint(&connection);
        let specs = [
            NewAttachment {
                client_id: "screen",
                kind: "image",
                filename: "failure.png",
                media_type: "image/png",
                encoding: None,
                size_bytes: 2048,
            },
            NewAttachment {
                client_id: "console",
                kind: "log",
                filename: "console.log",
                media_type: "text/plain",
                encoding: Some("utf-8"),
                size_bytes: 512,
            },
        ];
        let event = new_event_with_attachments("build:kinds", &specs, false);
        let (row, _) = create_event(&mut connection, &endpoint_id, &event, 1_000).expect("创建");
        assert_eq!(row.attachment_count, 2);
        assert_eq!(row.stored_attachments, 0);
        assert_eq!(row.attachment_bytes, 2560);

        let filter = |kind: &'static str| EventFilter {
            attachment_kind: Some(kind.into()),
            limit: 50,
            ..EventFilter::default()
        };
        let (logs, total) = list_events(&connection, &filter("log")).expect("按日志");
        assert_eq!(total, 1);
        assert_eq!(logs[0].external_id, "build:kinds");
        let (images, total) = list_events(&connection, &filter("image")).expect("按图片");
        assert_eq!(total, 1);
        assert_eq!(images[0].external_id, "build:kinds");
        let (other, total) = list_events(&connection, &filter("video")).expect("未知类型");
        assert_eq!(total, 0);
        assert!(other.is_empty());

        // 归档一个附件后，接收中进度与接入点占用都要跟着走（预留计入占用）。
        let attachments = event_attachments(&connection, &row.id).expect("附件");
        begin_upload(
            &connection,
            &row.id,
            &attachments[0].id,
            2_000,
            "test-attempt",
        )
        .expect("租约");
        assert_eq!(attachments[0].size_bytes, 512);
        finish_upload(
            &connection,
            &row.id,
            &attachments[0].id,
            "sha",
            512,
            "evt/att",
            2_000,
            "test-attempt",
        )
        .expect("归档");
        let (listed, _) = list_events(&connection, &filter("image")).expect("列表");
        assert_eq!(listed[0].stored_attachments, 1);
        assert_eq!(listed[0].attachment_bytes, 2560);
        let endpoint_row = endpoint_by_id(&connection, &endpoint_id)
            .expect("查询")
            .expect("存在");
        assert_eq!(endpoint_row.bytes, used_bytes(&connection).expect("总占用"));
        assert!(endpoint_row.bytes > 2560);
    }

    #[test]
    fn errors_are_audited_without_content() {
        let home = TempHome::new("audit");
        let connection = open_db(&home);
        let (endpoint_id, _) = endpoint(&connection);
        record_error(
            &connection,
            "req_test",
            Some(&endpoint_id),
            None,
            "invalid_request",
        );
        let endpoint_row = endpoint_by_id(&connection, &endpoint_id)
            .expect("查询")
            .expect("存在");
        assert_eq!(endpoint_row.error_count, 1);
        let audit_rows: i64 = connection
            .query_row("SELECT COUNT(*) FROM request_audit", [], |row| row.get(0))
            .expect("审计计数");
        assert_eq!(audit_rows, 1);
        let code: String = connection
            .query_row("SELECT code FROM request_audit", [], |row| row.get(0))
            .expect("审计码");
        assert_eq!(code, "invalid_request");
    }

    fn new_event_with_attachments<'a>(
        external_id: &'a str,
        attachments: &'a [NewAttachment<'a>],
        commit: bool,
    ) -> NewEvent<'a> {
        NewEvent {
            external_id,
            fingerprint: external_id,
            event_type: "build.materials",
            occurred_at: 1_763_172_000_000,
            title: "带附件事件",
            text: "混合素材",
            parameters: parameters(),
            sender_policy: None,
            source_ip: None,
            commit,
            attachments,
        }
    }

    #[test]
    fn attachments_are_pending_until_uploaded_and_required_by_commit() {
        let home = TempHome::new("attachments");
        let mut connection = open_db(&home);
        let (endpoint_id, _) = endpoint(&connection);
        let specs = [
            NewAttachment {
                client_id: "screen",
                kind: "image",
                filename: "failure.png",
                media_type: "image/png",
                encoding: None,
                size_bytes: 2048,
            },
            NewAttachment {
                client_id: "console",
                kind: "log",
                filename: "console.log",
                media_type: "text/plain",
                encoding: Some("utf-8"),
                size_bytes: 512,
            },
        ];
        let event = new_event_with_attachments("build:attachments", &specs, false);
        let (row, created) =
            create_event(&mut connection, &endpoint_id, &event, 1_000).expect("创建");
        assert!(created);
        assert_eq!(row.attachment_count, 2);
        assert_eq!(row.state, "receiving");

        let attachments = event_attachments(&connection, &row.id).expect("附件");
        assert_eq!(attachments.len(), 2);
        assert!(attachments
            .iter()
            .all(|attachment| attachment.state == ATTACHMENT_PENDING));
        let screen = attachments
            .iter()
            .find(|attachment| attachment.client_id == "screen")
            .expect("图片")
            .clone();
        assert_eq!(screen.kind, "image");
        assert_eq!(screen.size_bytes, 2048);

        assert!(
            matches!(
                commit_event(&mut connection, &endpoint_id, &row.id, 2_000),
                Err(StoreError::Conflict)
            ),
            "附件没上传完不能提交"
        );
        let lease =
            begin_upload(&connection, &row.id, &screen.id, 3_000, "test-attempt").expect("拿租约");
        assert_eq!(lease.state, ATTACHMENT_UPLOADING);
        assert!(
            matches!(
                begin_upload(&connection, &row.id, &screen.id, 3_100, "test-attempt"),
                Err(StoreError::Conflict)
            ),
            "同一附件同时只能有一个写者"
        );
        finish_upload(
            &connection,
            &row.id,
            &screen.id,
            "sha-image",
            2048,
            "evt-store/att-image",
            3_200,
            "test-attempt",
        )
        .expect("完成上传");
        assert!(
            matches!(
                begin_upload(&connection, &row.id, &screen.id, 3_300, "test-attempt"),
                Err(StoreError::Conflict)
            ),
            "stored 后不得替换"
        );

        let console = attachments
            .iter()
            .find(|attachment| attachment.client_id == "console")
            .expect("日志")
            .clone();
        begin_upload(&connection, &row.id, &console.id, 3_300, "test-attempt").expect("日志租约");
        finish_upload(
            &connection,
            &row.id,
            &console.id,
            "sha-log",
            512,
            "evt-store/att-log",
            3_400,
            "test-attempt",
        )
        .expect("日志完成");
        let ready = commit_event(&mut connection, &endpoint_id, &row.id, 3_500).expect("提交");
        assert_eq!(ready.state, "ready");
        assert_eq!(ready.expires_at, None);
        assert_eq!(ready.retention_until, None);
    }

    #[test]
    fn upload_lease_is_taken_over_after_timeout_and_abort_returns_to_pending() {
        let home = TempHome::new("lease");
        let mut connection = open_db(&home);
        let (endpoint_id, _) = endpoint(&connection);
        let specs = [NewAttachment {
            client_id: "log",
            kind: "log",
            filename: "build.log",
            media_type: "text/plain",
            encoding: Some("utf-8"),
            size_bytes: 10,
        }];
        let event = new_event_with_attachments("build:lease", &specs, false);
        let (row, _) = create_event(&mut connection, &endpoint_id, &event, 1_000).expect("创建");
        let lease_attachment = event_attachments(&connection, &row.id).expect("附件")[0].clone();
        begin_upload(
            &connection,
            &row.id,
            &lease_attachment.id,
            2_000,
            "test-attempt",
        )
        .expect("首租约");
        begin_upload(
            &connection,
            &row.id,
            &lease_attachment.id,
            2_000 + UPLOAD_LEASE_MS + 1,
            "test-attempt",
        )
        .expect("过期租约可接管");
        abort_upload(
            &connection,
            &row.id,
            &lease_attachment.id,
            3_000,
            "test-attempt",
        )
        .expect("放弃");
        let pending = attachment(&connection, &row.id, &lease_attachment.id)
            .expect("查询")
            .expect("存在");
        assert_eq!(pending.state, ATTACHMENT_PENDING);
    }

    #[test]
    fn stale_upload_cannot_finish_or_abort_a_new_lease_in_either_order() {
        let home = TempHome::new("lease-fencing");
        let mut connection = open_db(&home);
        let (endpoint_id, _) = endpoint(&connection);
        for newer_finishes_first in [false, true] {
            let specs = [NewAttachment {
                client_id: "log",
                kind: "log",
                filename: "build.log",
                media_type: "text/plain",
                encoding: Some("utf-8"),
                size_bytes: 10,
            }];
            let key = format!("lease:{newer_finishes_first}");
            let event = new_event_with_attachments(&key, &specs, false);
            let (row, _) = create_event(&mut connection, &endpoint_id, &event, 1_000).unwrap();
            let id = event_attachments(&connection, &row.id).unwrap()[0]
                .id
                .clone();
            begin_upload(&connection, &row.id, &id, 2_000, "old").unwrap();
            let now = 2_001 + UPLOAD_LEASE_MS;
            begin_upload(&connection, &row.id, &id, now, "new").unwrap();
            if newer_finishes_first {
                finish_upload(
                    &connection,
                    &row.id,
                    &id,
                    "new-digest",
                    10,
                    "evt/new",
                    now,
                    "new",
                )
                .unwrap();
            }
            assert!(matches!(
                finish_upload(
                    &connection,
                    &row.id,
                    &id,
                    "old-digest",
                    10,
                    "evt/old",
                    now,
                    "old"
                ),
                Err(StoreError::Conflict)
            ));
            abort_upload(&connection, &row.id, &id, now, "old").unwrap();
            if !newer_finishes_first {
                assert_eq!(
                    attachment(&connection, &row.id, &id)
                        .unwrap()
                        .unwrap()
                        .state,
                    ATTACHMENT_UPLOADING
                );
                finish_upload(
                    &connection,
                    &row.id,
                    &id,
                    "new-digest",
                    10,
                    "evt/new",
                    now,
                    "new",
                )
                .unwrap();
            }
            let final_row = attachment(&connection, &row.id, &id).unwrap().unwrap();
            assert_eq!(final_row.state, ATTACHMENT_STORED);
            assert_eq!(final_row.storage_rel.as_deref(), Some("evt/new"));
            assert_eq!(final_row.sha256.as_deref(), Some("new-digest"));
        }
    }

    #[test]
    fn used_bytes_counts_declared_attachments() {
        let home = TempHome::new("quota");
        let mut connection = open_db(&home);
        let (endpoint_id, _) = endpoint(&connection);
        let specs = [NewAttachment {
            client_id: "image",
            kind: "image",
            filename: "a.png",
            media_type: "image/png",
            encoding: None,
            size_bytes: 1234,
        }];
        let event = new_event_with_attachments("build:quota", &specs, false);
        create_event(&mut connection, &endpoint_id, &event, 1_000).expect("创建");
        let content = 1024
            + event.title.len()
            + event.text.len()
            + serde_json::to_string(event.parameters).unwrap().len()
            + event.sender_policy.unwrap_or("").len();
        assert_eq!(
            used_bytes(&connection).expect("占用"),
            1234 + content as u64
        );
    }

    #[test]
    fn legacy_text_and_parameters_count_utf8_bytes_and_can_be_evicted() {
        let home = TempHome::new("text-quota");
        let mut connection = open_db(&home);
        let (endpoint_id, _) = endpoint(&connection);
        let event = new_event("text", "text", "中文日志", true);
        let (row, _) = create_event(&mut connection, &endpoint_id, &event, 1_000).unwrap();
        let before = used_bytes(&connection).unwrap();
        // 回填使用 UTF-8 字节而不是字符数，与创建时的 Rust len() 口径一致。
        connection.execute("UPDATE events SET parameters = ?1, content_bytes = content_bytes - length(CAST(parameters AS BLOB)) + length(CAST(?1 AS BLOB))", params![r#"{"内容":"中文"}"#]).unwrap();
        let after = used_bytes(&connection).unwrap();
        assert_eq!(
            after as i64 - before as i64,
            r#"{"内容":"中文"}"#.len() as i64
                - serde_json::to_string(event.parameters).unwrap().len() as i64
        );
        let (target, freed) = evict_oldest_event(&connection).unwrap().unwrap();
        assert_eq!(target.event_id, row.id);
        assert!(target.rels.is_empty());
        assert_eq!(used_bytes(&connection).unwrap(), after - freed);
        assert_eq!(used_bytes(&connection).unwrap(), 1026);
        assert!(evict_oldest_event(&connection).unwrap().is_none());
    }

    #[test]
    fn v3_migration_backfills_existing_text_budget() {
        let home = TempHome::new("text-budget-v3");
        let connection = Connection::open(home.db()).unwrap();
        connection.execute_batch(SCHEMA_V1).unwrap();
        connection.execute_batch(SCHEMA_V2).unwrap();
        connection.execute_batch(SCHEMA_V3).unwrap();
        connection.execute_batch("PRAGMA user_version = 3;
            INSERT INTO endpoints (id,name,adapter,enabled,allowed_kinds,created_at) VALUES ('ep','test','generic',1,'[]',0);
            INSERT INTO events (id,endpoint_id,external_id,fingerprint,event_type,occurred_at,received_at,title,body_text,parameters,state)
            VALUES ('evt','ep','external','fingerprint','text',0,0,'标题','日志','{}','ready');").unwrap();
        drop(connection);
        let connection = open_db(&home);
        assert_eq!(
            used_bytes(&connection).unwrap(),
            1024 + "标题日志{}".len() as u64
        );
        assert_eq!(
            endpoint_by_id(&connection, "ep").unwrap().unwrap().bytes,
            used_bytes(&connection).unwrap()
        );
    }

    #[test]
    fn row_capacity_reclaims_tombstones_then_history_but_not_receiving() {
        let home = TempHome::new("row-quota");
        let mut connection = open_db(&home);
        let (endpoint_id, _) = endpoint(&connection);
        let old = create_event(
            &mut connection,
            &endpoint_id,
            &new_event("old", "old", "old", true),
            1,
        )
        .unwrap()
        .0;
        let pending = create_event(
            &mut connection,
            &endpoint_id,
            &new_event("pending", "pending", "pending", false),
            2,
        )
        .unwrap()
        .0;
        let recent = create_event(
            &mut connection,
            &endpoint_id,
            &new_event("recent", "recent", "recent", true),
            3,
        )
        .unwrap()
        .0;
        let mut targets = Vec::new();
        enforce_event_limit(&connection, 2, &mut targets).unwrap();
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].event_id, old.id);
        assert!(event_detail(&connection, &old.id).unwrap().is_none());
        assert!(event_detail(&connection, &pending.id).unwrap().is_some());
        assert!(event_detail(&connection, &recent.id).unwrap().is_some());
        enforce_event_limit(&connection, 1, &mut targets).unwrap();
        assert!(matches!(
            enforce_event_limit(&connection, 0, &mut targets),
            Err(StoreError::QuotaExceeded(_))
        ));
    }

    #[test]
    fn missing_attachment_marks_event_unavailable() {
        let home = TempHome::new("unavailable");
        let mut connection = open_db(&home);
        let (endpoint_id, _) = endpoint(&connection);
        let specs = [NewAttachment {
            client_id: "image",
            kind: "image",
            filename: "a.png",
            media_type: "image/png",
            encoding: None,
            size_bytes: 10,
        }];
        let event = new_event_with_attachments("build:missing", &specs, false);
        let (row, _) = create_event(&mut connection, &endpoint_id, &event, 1_000).expect("创建");
        let attachment_row = event_attachments(&connection, &row.id).expect("附件")[0].clone();
        begin_upload(
            &connection,
            &row.id,
            &attachment_row.id,
            1_100,
            "test-attempt",
        )
        .expect("租约");
        finish_upload(
            &connection,
            &row.id,
            &attachment_row.id,
            "sha",
            10,
            "evt/att",
            1_200,
            "test-attempt",
        )
        .expect("完成");
        commit_event(&mut connection, &endpoint_id, &row.id, 1_300).expect("提交");
        assert!(mark_event_unavailable(&connection, &row.id).expect("标记不可用"));
        let unavailable = event_detail(&connection, &row.id)
            .expect("查询")
            .expect("事件仍在");
        assert_eq!(unavailable.state, "unavailable");
        let failed = event_attachments(&connection, &row.id).expect("附件")[0].clone();
        assert_eq!(failed.state, ATTACHMENT_FAILED);
    }

    #[test]
    fn cleanup_clears_content_then_removes_tombstone() {
        let home = TempHome::new("cleanup");
        let mut connection = open_db(&home);
        let (endpoint_id, _) = endpoint(&connection);
        let specs = [NewAttachment {
            client_id: "image",
            kind: "image",
            filename: "a.png",
            media_type: "image/png",
            encoding: None,
            size_bytes: 10,
        }];
        let event = new_event_with_attachments("build:cleanup", &specs, false);
        let (row, _) = create_event(&mut connection, &endpoint_id, &event, 1_000).expect("创建");
        let attachment_row = event_attachments(&connection, &row.id).expect("附件")[0].clone();
        begin_upload(
            &connection,
            &row.id,
            &attachment_row.id,
            1_100,
            "test-attempt",
        )
        .expect("租约");
        finish_upload(
            &connection,
            &row.id,
            &attachment_row.id,
            "sha",
            10,
            "evt/att",
            1_200,
            "test-attempt",
        )
        .expect("完成");
        // 未提交的接收中事件超过 24h 才算过期；已入箱内容不按时间清理。
        let targets = cleanup_content(&mut connection, 1_000 + RECEIVING_TTL_MS + 1).expect("清理");
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].event_id, row.id);
        assert_eq!(targets[0].rels, vec!["evt/att".to_string()]);
        let expired = event_detail(&connection, &row.id)
            .expect("查询")
            .expect("墓碑仍在");
        assert_eq!(expired.state, "expired");
        assert_eq!(expired.title, "");
        assert_eq!(expired.body_text, "");
        assert_eq!(expired.attachment_count, 0);
        assert_eq!(used_bytes(&connection).expect("占用"), 1026);

        let targets =
            cleanup_content(&mut connection, 1_000 + TOMBSTONE_MS + 10_000).expect("墓碑清理");
        assert!(targets.is_empty());
        assert!(event_detail(&connection, &row.id).expect("查询").is_none());
    }

    #[test]
    fn ready_events_are_not_removed_by_time() {
        let home = TempHome::new("no-time-retention");
        let mut connection = open_db(&home);
        let (endpoint_id, _) = endpoint(&connection);
        let event = new_event("build:keep", "fingerprint-keep", "长期保留", true);
        let (row, _) = create_event(&mut connection, &endpoint_id, &event, 1_000).expect("创建");
        let targets =
            cleanup_content(&mut connection, 1_000 + 365 * 24 * 60 * 60 * 1000).expect("清理");
        assert!(targets.is_empty());
        let kept = event_detail(&connection, &row.id)
            .expect("查询")
            .expect("仍保留");
        assert_eq!(kept.state, "ready");
        assert_eq!(kept.body_text, "长期保留");
    }

    const CAPACITY_CHUNK: i64 = 100 * 1024 * 1024;

    fn capacity_attachment() -> NewAttachment<'static> {
        NewAttachment {
            client_id: "image",
            kind: "image",
            filename: "a.png",
            media_type: "image/png",
            encoding: None,
            size_bytes: CAPACITY_CHUNK,
        }
    }

    #[test]
    fn capacity_eviction_removes_the_oldest_ready_event_first() {
        let home = TempHome::new("capacity");
        let mut connection = open_db(&home);
        let (endpoint_id, _) = endpoint(&connection);
        let old_specs = [capacity_attachment()];
        let old_event = new_event_with_attachments("capacity:old", &old_specs, false);
        let (old, _, _) = create_event_evicting(&mut connection, &endpoint_id, &old_event, 1_000)
            .expect("旧事件");
        let attachment = event_attachments(&connection, &old.id).expect("附件")[0].clone();
        begin_upload(&connection, &old.id, &attachment.id, 1_100, "test-attempt").expect("租约");
        finish_upload(
            &connection,
            &old.id,
            &attachment.id,
            "sha",
            CAPACITY_CHUNK,
            "evt-old/att",
            1_200,
            "test-attempt",
        )
        .expect("完成");
        commit_event(&mut connection, &endpoint_id, &old.id, 1_300).expect("提交");

        let mut evicted = Vec::new();
        for index in 0..51 {
            let external = format!("capacity:{index}");
            let specs = [capacity_attachment()];
            let event = new_event_with_attachments(&external, &specs, false);
            let (_, _, paths) =
                create_event_evicting(&mut connection, &endpoint_id, &event, 2_000 + index)
                    .expect("容量内创建");
            evicted.extend(paths);
        }
        assert_eq!(evicted.len(), 1, "只清理最早的一条");
        assert_eq!(evicted[0].event_id, old.id);
        assert_eq!(evicted[0].rels, vec!["evt-old/att".to_string()]);
        let old_row = event_detail(&connection, &old.id)
            .expect("查询")
            .expect("墓碑仍在");
        assert_eq!(old_row.state, "evicted");
        assert_eq!(old_row.title, "");
        assert_eq!(old_row.attachment_count, 0);
        assert!(used_bytes(&connection).expect("占用") > 51 * CAPACITY_CHUNK as u64);
        assert!(used_bytes(&connection).expect("占用") <= STORAGE_BUDGET_BYTES);
    }

    #[test]
    fn maintenance_enforces_capacity_without_new_events() {
        let home = TempHome::new("capacity-maintenance");
        let mut connection = open_db(&home);
        let (endpoint_id, _) = endpoint(&connection);
        for index in 0..51 {
            let external = format!("maintenance:{index}");
            let specs = [capacity_attachment()];
            let event = new_event_with_attachments(&external, &specs, false);
            create_event_evicting(&mut connection, &endpoint_id, &event, 2_000 + index)
                .expect("容量内创建");
        }
        connection
            .execute(
                "UPDATE attachments SET state = 'stored', storage_rel = 'fake/' || id",
                [],
            )
            .expect("模拟已归档");
        connection
            .execute("UPDATE events SET state = 'ready'", [])
            .expect("模拟已入箱");
        connection
            .execute(
                "UPDATE attachments SET size_bytes = size_bytes + ?1
                 WHERE id = (SELECT id FROM attachments ORDER BY rowid LIMIT 1)",
                params![CAPACITY_CHUNK],
            )
            .expect("制造超出预算的占用");
        assert!(used_bytes(&connection).expect("占用") > STORAGE_BUDGET_BYTES);

        let targets = cleanup_content(&mut connection, 10_000).expect("维护清理");
        assert!(!targets.is_empty(), "维护应清理最早的历史事件");
        assert!(
            used_bytes(&connection).expect("占用") <= STORAGE_BUDGET_BYTES,
            "维护后占用必须回到预算内"
        );
        let evicted: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM events WHERE state = 'evicted'",
                [],
                |row| row.get(0),
            )
            .expect("统计");
        assert!(evicted > 0);
    }

    #[test]
    fn capacity_eviction_fails_when_only_incomplete_uploads_remain() {
        let home = TempHome::new("capacity-no-history");
        let mut connection = open_db(&home);
        let (endpoint_id, _) = endpoint(&connection);
        for index in 0..51 {
            let external = format!("pending:{index}");
            let specs = [capacity_attachment()];
            let event = new_event_with_attachments(&external, &specs, false);
            create_event_evicting(&mut connection, &endpoint_id, &event, 2_000 + index)
                .expect("容量内创建");
        }
        let external = "pending:overflow";
        let specs = [capacity_attachment()];
        let event = new_event_with_attachments(external, &specs, false);
        assert!(matches!(
            create_event_evicting(&mut connection, &endpoint_id, &event, 3_000),
            Err(StoreError::QuotaExceeded(_))
        ));
        assert_eq!(
            counts(&connection).expect("计数").1,
            51,
            "不能删除接收中的事件"
        );
    }

    #[test]
    fn manual_delete_removes_event_and_returns_blob_refs() {
        let home = TempHome::new("delete-event");
        let mut connection = open_db(&home);
        let (endpoint_id, _) = endpoint(&connection);
        let event = new_event("build:delete-event", "fingerprint-delete", "内容", true);
        let (row, _) = create_event(&mut connection, &endpoint_id, &event, 1_000).expect("创建");
        let rels = delete_event(&mut connection, &row.id).expect("删除");
        assert!(rels.is_empty());
        assert!(event_detail(&connection, &row.id).expect("查询").is_none());
        assert!(matches!(
            delete_event(&mut connection, &row.id),
            Err(StoreError::NotFound)
        ));
    }

    #[test]
    fn v1_database_migrates_to_current_schema() {
        let home = TempHome::new("migrate-v1");
        let connection = Connection::open(home.db()).expect("直接建库");
        connection.execute_batch(SCHEMA_V1).expect("v1 schema");
        connection
            .pragma_update(None, "user_version", 1)
            .expect("版本 1");
        drop(connection);
        let connection = open_db(&home);
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("版本");
        assert_eq!(version, DB_SCHEMA_VERSION);
        let table: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='attachments'",
                [],
                |row| row.get(0),
            )
            .expect("附件表");
        assert_eq!(table, 1);
        let notification_column: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('endpoints') WHERE name = 'notification_token_hash'",
                [],
                |row| row.get(0),
            )
            .expect("通知凭据列");
        assert_eq!(notification_column, 1);
    }

    #[test]
    fn v2_database_migrates_to_v3_without_losing_endpoints() {
        let home = TempHome::new("migrate-v2");
        let connection = Connection::open(home.db()).expect("直接建库");
        connection.execute_batch(SCHEMA_V1).expect("v1 schema");
        connection.execute_batch(SCHEMA_V2).expect("v2 schema");
        connection
            .execute(
                "INSERT INTO endpoints (id, name, adapter, enabled, token_hash, allowed_kinds,
                     created_at, last_event_at, error_count, deleted_at)
                 VALUES ('ep_old', '旧接入点', 'generic', 1, 'hash', '[\"text\"]', 1, NULL, 0, NULL)",
                [],
            )
            .expect("旧数据");
        connection
            .pragma_update(None, "user_version", 2)
            .expect("版本 2");
        drop(connection);

        let connection = open_db(&home);
        let row = endpoint_by_id(&connection, "ep_old")
            .expect("查询")
            .expect("旧接入点仍在");
        assert_eq!(row.name, "旧接入点");
        assert!(!row.notification_enabled);
        let (_, token) =
            rotate_notification_token(&connection, "ep_old").expect("迁移后可生成通知凭据");
        assert_eq!(token.len(), 64);
    }

    #[test]
    fn notification_credential_is_independent_and_rotation_invalidates_it() {
        let home = TempHome::new("notification-token");
        let connection = open_db(&home);
        let (endpoint_id, bearer) = endpoint(&connection);
        assert!(
            !endpoint_by_id(&connection, &endpoint_id)
                .expect("查询")
                .expect("存在")
                .notification_enabled
        );

        let (row, notification) =
            rotate_notification_token(&connection, &endpoint_id).expect("生成通知凭据");
        assert!(row.notification_enabled);
        assert_ne!(notification, bearer);
        assert!(
            auth_by_token_hash(&connection, &hash_token(&notification))
                .expect("Bearer 查询")
                .is_none(),
            "通知密码不能当 Bearer 令牌用"
        );
        assert!(
            auth_by_notification_token_hash(&connection, &hash_token(&bearer))
                .expect("通知查询")
                .is_none(),
            "Bearer 令牌不能当通知密码用"
        );
        let auth = auth_by_notification_token_hash(&connection, &hash_token(&notification))
            .expect("通知查询")
            .expect("命中");
        assert_eq!(auth.id, endpoint_id);

        let (_, rotated) =
            rotate_notification_token(&connection, &endpoint_id).expect("轮换通知凭据");
        assert_ne!(rotated, notification);
        assert!(
            auth_by_notification_token_hash(&connection, &hash_token(&notification))
                .expect("旧通知凭据")
                .is_none(),
            "旧通知密码必须立即失效"
        );
    }
}
