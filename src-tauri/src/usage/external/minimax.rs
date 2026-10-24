//! MiniMax desktop 的会话 token 用量。
//!
//! ## 两个库，两段历史
//!
//! | 库 | 表 | 覆盖 |
//! | --- | --- | --- |
//! | `~/.minimax/v2/sqlite/runtime-state.sqlite`（当前） | `local_runtime_token_usage` | 2026-07-11 起，一直到今天 |
//! | `~/.minimax/sqlite.db`（老版本） | `token_usage` | 2026-06-07 ~ 06-30 |
//!
//! 两张表**列结构完全相同**（老的那张只是没加 `local_runtime_` 前缀），只是
//! **时间段不重叠**——所以两个都读、并集就是完整历史。**只读老库会得到三个
//! 月前的数据**，对「最近 90 天」窗口贡献 0 行。
//!
//! ## 口径
//!
//! - **一次模型调用一行**：`input_tokens` / `output_tokens` / `cache_read_tokens`
//!   / `cache_write_tokens` 是**每次请求**的增量（实测同一会话里 `input`
//!   非单调抖动、`cache_read` 随上下文增长——累计值不会这样）。
//! - **总量 = `input + output + cache_read + cache_write`**（56,103 行全量
//!   恒等式成立）。`reasoning_tokens` 两库里**恒为 0**，不参与计算。
//! - **`input` 不含 `cache_read`**——与 Codex 那套（cached ⊂ input）正好相反。
//! - `raw` 列在两个库里**键名不同**（`totalTokens`/`cacheRead` vs
//!   `total`/`cache.read`），所以**只用列**，不解析 `raw`。
//!
//! ## 不读的那些
//!
//! - `v2/sqlite/backups/runtime-state-before-v2-migration-*.sqlite` 是当前库的
//!   **子集**副本，读它就是重复计数。
//! - `local_runtime_v2_goals` / `local_runtime_thread_goals` 是 goal 级聚合，
//!   `local_runtime_v2_memory_execution_calls` 是 memory 子系统的**独立**子账
//!   ——加了就是重复计数。
//!
//! ## 读法
//!
//! 用 `rusqlite` 以**只读**方式打开。**不能用 `immutable=1`**：那个模式忽略
//! WAL，而当前库的 `-wal` 里有尚未 checkpoint 的帧（实测会丢掉最近几分钟
//! 的数据）。只读连接要能建 / 读 `-shm` 索引，所以 MiniMax 没在运行时可能
//! 打开失败——那时退回 `immutable=1` 并**如实告诉用户数据可能不是最新的**。

use std::collections::BTreeMap;
use std::path::Path;

use rusqlite::{Connection, OpenFlags};

use super::{derive_view, ExternalModelDay, ExternalToolUsage};

/// 来源 id（缓存文档里的键）。
pub const SOURCE_ID: &str = "minimax";

/// 界面名。
const LABEL: &str = "MiniMax Desktop";

/// 当前版本的用量库（相对 MiniMax 根目录）。
const LIVE_DB: &str = "v2/sqlite/runtime-state.sqlite";
/// 老版本的用量库。
const LEGACY_DB: &str = "sqlite.db";

/// 当前库的用量表。
const LIVE_TABLE: &str = "local_runtime_token_usage";
/// 老库的用量表。
const LEGACY_TABLE: &str = "token_usage";

/// 扫 MiniMax desktop 的用量，产出视图。
pub(crate) fn scan(cutoff: &str, today: &str) -> ExternalToolUsage {
    let Some(root) = super::minimax_root() else {
        return note("未找到 MiniMax desktop 的本机数据目录（~/.minimax）");
    };
    scan_root(&root, cutoff, today)
}

/// 扫一个 MiniMax 根目录。
///
/// 与 [`scan`] 分开是为了**可测**：根目录由调用方给出，测试就能在临时目录里
/// 造一个 SQLite 库，不碰用户真实数据。
pub(crate) fn scan_root(root: &Path, cutoff: &str, today: &str) -> ExternalToolUsage {
    let live = root.join(LIVE_DB);
    let legacy = root.join(LEGACY_DB);

    let mut days: BTreeMap<String, BTreeMap<String, ExternalModelDay>> = BTreeMap::new();
    let mut tracked = 0usize;
    let mut notes: Vec<String> = Vec::new();
    let mut any = false;

    for (path, table) in [(&live, LIVE_TABLE), (&legacy, LEGACY_TABLE)] {
        if !path.is_file() {
            continue;
        }
        match read_table(path, table, cutoff) {
            Ok(scan) => {
                any = any || scan.rows > 0;
                tracked += scan.rows;
                if scan.degraded {
                    notes.push(format!(
                        "{} 只能按「忽略 WAL」的方式读取（MiniMax 没在运行时），最近几分钟的记录可能不在账里",
                        path.display()
                    ));
                }
                for (day, key, row) in scan.entries {
                    let slot = days.entry(day).or_default().entry(key).or_default();
                    slot.requests += row.requests;
                    slot.tokens += row.tokens;
                    slot.input += row.input;
                    slot.output += row.output;
                    slot.cache_read += row.cache_read;
                    slot.cache_write += row.cache_write;
                }
            }
            Err(reason) => notes.push(reason),
        }
    }

    if !any && notes.is_empty() {
        return note("没有找到 MiniMax desktop 的用量库");
    }
    let mut view = derive_view(
        LABEL,
        SOURCE_ID,
        &root.to_string_lossy(),
        tracked,
        &days,
        cutoff,
        today,
    );
    if view.tokens == 0 && view.requests == 0 {
        view.note = Some(if notes.is_empty() {
            "近 90 天内没有读到 MiniMax desktop 的会话用量记录".to_string()
        } else {
            notes.join("；")
        });
    } else if !notes.is_empty() {
        // 有数据但也有一处读不到：把这一半说清楚，别让用户以为那是全部。
        view.note = Some(notes.join("；"));
    }
    view
}

/// 该来源不可用时的占位视图。
fn note(reason: &str) -> ExternalToolUsage {
    ExternalToolUsage {
        source: SOURCE_ID.to_string(),
        label: LABEL.to_string(),
        note: Some(reason.to_string()),
        ..Default::default()
    }
}

/// 一行用量账：`(日期, 模型键, 分量)`。
type UsageRow = (String, String, ModelRow);

/// 一次表扫描的结果：行数、是否降级、逐行的账。
struct TableScan {
    rows: usize,
    degraded: bool,
    entries: Vec<UsageRow>,
}

/// 读一个库的一张用量表。
///
/// 返回 `(行数, 是否降级, 逐行的账)`。降级指「只能按忽略 WAL 的方式读到」
/// ——数据可能不是最新的，文案里要说。
fn read_table(path: &Path, table: &str, cutoff: &str) -> Result<TableScan, String> {
    let (conn, degraded) = open_read_only(path)?;
    let sql = format!(
        "SELECT ts, model, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens \
         FROM {table} WHERE ts >= ?1"
    );
    let cutoff_ms = cutoff_epoch_ms(cutoff);
    let mut statement = conn
        .prepare(&sql)
        .map_err(|e| format!("读取 {} 失败：{e}", path.display()))?;
    let mut rows = statement
        .query([cutoff_ms])
        .map_err(|e| format!("查询 {} 失败：{e}", path.display()))?;
    let mut out: Vec<UsageRow> = Vec::new();
    while let Some(row) = rows
        .next()
        .map_err(|e| format!("遍历 {} 失败：{e}", path.display()))?
    {
        let ts: i64 = row.get(0).unwrap_or(0);
        let model: Option<String> = row.get(1).unwrap_or(None);
        let input: u64 = row.get::<_, i64>(2).unwrap_or(0).max(0) as u64;
        let output: u64 = row.get::<_, i64>(3).unwrap_or(0).max(0) as u64;
        let cache_read: u64 = row.get::<_, i64>(4).unwrap_or(0).max(0) as u64;
        let cache_write: u64 = row.get::<_, i64>(5).unwrap_or(0).max(0) as u64;
        let day = super::local_date_string_of_ms(ts.max(0) as u64);
        let key = model_key(model.as_deref());
        out.push((
            day,
            key,
            ModelRow {
                requests: 1,
                tokens: input + output + cache_read + cache_write,
                input,
                output,
                cache_read,
                cache_write,
            },
        ));
    }
    Ok(TableScan {
        rows: out.len(),
        degraded,
        entries: out,
    })
}

/// 一行用量。
struct ModelRow {
    requests: u64,
    tokens: u64,
    input: u64,
    output: u64,
    cache_read: u64,
    cache_write: u64,
}

/// 模型 → `provider/model`。
///
/// MiniMax 的 `model` 列本来就带 `minimax/` 前缀；**85% 的行是 NULL**
/// （2026-08-11 起 MiniMax 不再往这个列里写模型名），那些行归到
/// `minimax/未标注`——如实标出来，而不是编一个模型名。
fn model_key(model: Option<&str>) -> String {
    match model.map(str::trim).filter(|m| !m.is_empty()) {
        Some(model) if model.contains('/') => model.to_string(),
        Some(model) => format!("minimax/{model}"),
        None => "minimax/未标注".to_string(),
    }
}

/// 只读打开一个 SQLite 库。
///
/// 先试常规只读（WAL 正确）；失败时退回 `immutable=1`——那个模式忽略 WAL，
/// 数据可能落后几分钟，所以返回 `degraded = true` 让上层如实说清。
fn open_read_only(path: &Path) -> Result<(Connection, bool), String> {
    match Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY) {
        Ok(conn) => Ok((conn, false)),
        Err(first) => {
            let uri = format!("file:{}?immutable=1", path.display());
            match Connection::open_with_flags(&uri, OpenFlags::SQLITE_OPEN_READ_ONLY) {
                Ok(conn) => Ok((conn, true)),
                Err(second) => Err(format!(
                    "读取 {} 失败（{first}；退回只读快照也失败：{second}）",
                    path.display()
                )),
            }
        }
    }
}

/// 本地日历日 `YYYY-MM-DD` → 该日起点的 epoch 毫秒（UTC 口径的近似下限）。
///
/// 只用作 SQL 的**下界**：取当天 00:00 的 UTC 时刻比取本地 00:00 早最多
/// 一天，落在窗口外的那部分行在读出来之后还会按本地日历日被剪掉。
fn cutoff_epoch_ms(cutoff: &str) -> i64 {
    let Some(date) = time::Date::parse(
        cutoff,
        &time::macros::format_description!("[year]-[month]-[day]"),
    )
    .ok() else {
        return 0;
    };
    date.midnight()
        .assume_utc()
        .unix_timestamp_nanos()
        .div_euclid(1_000_000_000) as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    /// 造一个临时目录（用完删掉）。**不碰用户真实数据目录**。
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "dsh-xlink-usage-minimax-{tag}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("临时目录应可创建");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// 造一个只含用量表的 SQLite 库。
    fn write_db(path: &Path, table: &str, rows: &[(Option<&str>, i64, i64, i64, i64)]) {
        let conn = Connection::open(path).expect("临时库应可写");
        conn.execute(
            &format!(
                "CREATE TABLE {table} (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    session_id TEXT NOT NULL,
                    model TEXT,
                    ts INTEGER NOT NULL,
                    input_tokens INTEGER NOT NULL DEFAULT 0,
                    output_tokens INTEGER NOT NULL DEFAULT 0,
                    cache_read_tokens INTEGER NOT NULL DEFAULT 0,
                    cache_write_tokens INTEGER NOT NULL DEFAULT 0
                )"
            ),
            [],
        )
        .expect("建表应成功");
        for (model, ts, input, output, cache_read) in rows {
            let model = model.map(|m| m.to_string());
            conn.execute(
                &format!(
                    "INSERT INTO {table} (session_id, model, ts, input_tokens, output_tokens, cache_read_tokens)
                     VALUES ('mvs_test', ?1, ?2, ?3, ?4, ?5)"
                ),
                rusqlite::params![model, ts, input, output, cache_read],
            )
            .expect("写行应成功");
        }
    }

    /// 2026-10-24 12:00:00 本地的 epoch 毫秒。
    fn ts_2026_10_24() -> i64 {
        1_792_876_800_000
    }

    #[test]
    fn live_database_is_the_source_of_truth() {
        let dir = TempDir::new("live");
        let db_dir = dir.path().join("v2/sqlite");
        std::fs::create_dir_all(&db_dir).unwrap();
        write_db(
            &db_dir.join("runtime-state.sqlite"),
            LIVE_TABLE,
            &[(Some("minimax/MiniMax-M3"), ts_2026_10_24(), 100, 20, 300)],
        );
        let view = scan_root(dir.path(), "2026-10-01", "2026-10-31");
        assert_eq!(view.requests, 1);
        // 总量 = input + output + cache_read（cache_write 为 0）。
        assert_eq!(view.tokens, 420);
        assert_eq!(view.models.len(), 1);
        assert_eq!(view.models[0].key, "minimax/MiniMax-M3");
        assert_eq!(view.models[0].provider, "minimax");
        assert_eq!(view.models[0].model, "MiniMax-M3");
    }

    #[test]
    fn legacy_database_is_read_too() {
        let dir = TempDir::new("legacy");
        write_db(
            &dir.path().join("sqlite.db"),
            LEGACY_TABLE,
            &[(Some("minimax/MiniMax-M3"), ts_2026_10_24(), 10, 5, 0)],
        );
        let view = scan_root(dir.path(), "2026-10-01", "2026-10-31");
        assert_eq!(view.requests, 1, "老版本的库也要算（它覆盖更早的历史）");
        assert_eq!(view.tokens, 15);
    }

    #[test]
    fn rows_outside_the_window_are_not_counted() {
        let dir = TempDir::new("window");
        let db_dir = dir.path().join("v2/sqlite");
        std::fs::create_dir_all(&db_dir).unwrap();
        write_db(
            &db_dir.join("runtime-state.sqlite"),
            LIVE_TABLE,
            &[
                (Some("minimax/MiniMax-M3"), ts_2026_10_24(), 10, 5, 0),
                (Some("minimax/MiniMax-M3"), 1_000_000_000_000, 999, 999, 999),
            ],
        );
        let view = scan_root(dir.path(), "2026-10-01", "2026-10-31");
        assert_eq!(view.requests, 1);
        assert_eq!(view.tokens, 15, "窗口外的行不进账");
    }

    #[test]
    fn missing_model_is_labelled_instead_of_invented() {
        let dir = TempDir::new("nomodel");
        let db_dir = dir.path().join("v2/sqlite");
        std::fs::create_dir_all(&db_dir).unwrap();
        write_db(
            &db_dir.join("runtime-state.sqlite"),
            LIVE_TABLE,
            &[(None, ts_2026_10_24(), 10, 5, 0)],
        );
        let view = scan_root(dir.path(), "2026-10-01", "2026-10-31");
        assert_eq!(view.models.len(), 1);
        assert_eq!(
            view.models[0].key, "minimax/未标注",
            "模型名缺失时如实标注，不编一个"
        );
    }

    #[test]
    fn a_missing_source_says_so_instead_of_reporting_zero() {
        let dir = TempDir::new("empty");
        let view = scan_root(dir.path(), "2026-10-01", "2026-10-31");
        assert!(
            view.note.is_some(),
            "没有库时要说清为什么，而不是显示 0 用量"
        );
    }
}
