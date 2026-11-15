//! Hook 收件箱的 SQLite 存储：schema/迁移与全部仓库函数。
//!
//! P1 只存元数据与纯文本正文；文件类素材（附件）属于 P2。所有函数只接收
//! `Connection`，不做网络、Tauri 或路径解析之外的副作用，便于用临时数据库
//! 直接测试。连接按调用打开（收件箱流量是每分钟几十条，不值得维护跨线程连接池），
//! 写并发由 WAL + `busy_timeout` 与 `IMMEDIATE` 事务串行化。

use std::path::Path;
use std::time::Duration;

use rusqlite::{params, types::Type, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// 数据库 schema 版本。只允许向前迁移；更高版本一律拒绝打开。
pub(crate) const DB_SCHEMA_VERSION: i64 = 1;

/// 未提交草稿的默认寿命：24 小时。
pub(crate) const RECEIVING_TTL_MS: i64 = 24 * 60 * 60 * 1000;
/// 已入箱事件的默认保留期：7 天（P1 只记录，不删除；清理在 P2）。
pub(crate) const RETENTION_MS: i64 = 7 * 24 * 60 * 60 * 1000;

const SCHEMA: &str = r#"
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

const MAX_AUDIT_ROWS: i64 = 2000;

#[derive(Debug)]
pub(crate) enum StoreError {
    /// 对象不存在或不属于该接入点；HTTP 映射 404，本地命令映射「不存在」。
    NotFound,
    /// 幂等键冲突或状态不允许；HTTP 映射 409。
    Conflict,
    /// 数据库打不开/读写出错；HTTP 映射 503。保留原始原因供日志与面板。
    Unavailable(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::NotFound => write!(f, "对象不存在"),
            StoreError::Conflict => write!(f, "幂等键或状态冲突"),
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
    pub state: String,
    pub occurred_at: i64,
    pub received_at: i64,
    pub expires_at: Option<i64>,
    pub retention_until: Option<i64>,
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
    pub commit: bool,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct EventFilter {
    pub endpoint_id: Option<String>,
    pub state: Option<String>,
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
            tx.execute_batch(SCHEMA)?;
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
    })
}

const ENDPOINT_COLUMNS: &str =
    "id, name, adapter, enabled, allowed_kinds, created_at, last_event_at, error_count";

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

pub(crate) fn auth_by_token_hash(
    connection: &Connection,
    token_hash: &str,
) -> Result<Option<EndpointAuth>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT id, enabled, allowed_kinds, deleted_at FROM endpoints WHERE token_hash = ?1",
    )?;
    let row = statement
        .query_row(params![token_hash], |row| {
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
        })
        .optional()?;
    Ok(row)
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

/// 软删除：立即作废凭据、停止接收，事件保留供本地查看。数据清理属于后续阶段。
pub(crate) fn delete_endpoint(connection: &Connection, id: &str) -> Result<(), StoreError> {
    let changed = connection.execute(
        "UPDATE endpoints SET enabled = 0, token_hash = NULL, deleted_at = ?2
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
        state: row.get(9)?,
        occurred_at: row.get(10)?,
        received_at: row.get(11)?,
        expires_at: row.get(12)?,
        retention_until: row.get(13)?,
    })
}

const EVENT_SELECT: &str = "SELECT e.id, e.endpoint_id, p.name, e.external_id, e.event_type,
        e.title, e.body_text, e.parameters, e.sender_policy, e.state,
        e.occurred_at, e.received_at, e.expires_at, e.retention_until
    FROM events e JOIN endpoints p ON p.id = e.endpoint_id";

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

/// 创建事件或返回已有事件。成功响应只能在事务提交后发出：`Ok` 返回时数据
/// 已经落盘（WAL 同步）。
pub(crate) fn create_event(
    connection: &mut Connection,
    endpoint_id: &str,
    event: &NewEvent<'_>,
    at_ms: i64,
) -> Result<(EventRow, bool), StoreError> {
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
        return Ok((row, false));
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
    let retention_until = if event.commit {
        Some(at_ms + RETENTION_MS)
    } else {
        None
    };
    let id = new_id("evt");
    let parameters = serde_json::to_string(event.parameters)
        .map_err(|error| StoreError::Unavailable(error.to_string()))?;
    tx.execute(
        "INSERT INTO events (id, endpoint_id, external_id, fingerprint, event_type,
             occurred_at, received_at, title, body_text, parameters, sender_policy,
             state, expires_at, retention_until)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
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
            state,
            expires_at,
            retention_until,
        ],
    )?;
    tx.execute(
        "UPDATE endpoints SET last_event_at = ?2 WHERE id = ?1",
        params![endpoint_id, at_ms],
    )?;
    let row = event_by_id(&tx, &id, Some(endpoint_id))?.ok_or(StoreError::NotFound)?;
    tx.commit()?;
    Ok((row, true))
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
            tx.execute(
                "UPDATE events SET state = 'ready', expires_at = NULL, retention_until = ?2
                 WHERE id = ?1",
                params![event_id, at_ms + RETENTION_MS],
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
) -> Result<(Vec<EventRow>, u64), StoreError> {
    let limit = filter.limit.clamp(1, 100) as i64;
    let offset = filter.offset as i64;
    let pattern = filter.q.as_deref().map(like_pattern);
    let where_sql = "WHERE (?1 IS NULL OR e.endpoint_id = ?1)
          AND (?2 IS NULL OR e.state = ?2)
          AND (?3 IS NULL OR e.title LIKE ?4 ESCAPE '\\' OR e.external_id LIKE ?4 ESCAPE '\\')";
    let mut statement = connection.prepare(&format!(
        "{EVENT_SELECT} {where_sql} ORDER BY e.received_at DESC, e.rowid DESC LIMIT ?5 OFFSET ?6"
    ))?;
    let items = statement
        .query_map(
            params![
                filter.endpoint_id.as_deref(),
                filter.state.as_deref(),
                filter.q.as_deref(),
                pattern.as_deref(),
                limit,
                offset
            ],
            event_from_row,
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let total: i64 = connection.query_row(
        &format!("SELECT COUNT(*) FROM events e {where_sql}"),
        params![
            filter.endpoint_id.as_deref(),
            filter.state.as_deref(),
            filter.q.as_deref(),
            pattern.as_deref()
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
            commit,
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
        assert_eq!(committed.retention_until, Some(2_000 + RETENTION_MS));
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
}
