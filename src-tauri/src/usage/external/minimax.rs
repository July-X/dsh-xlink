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
//! **同名文件不等于同一份库**：`~/.minimax/sqlite.db` 的用途随 MiniMax 版本变过
//! （2026-10 实测本机它只剩一张 `agents` 表）。按路径去找表、找不到就**整库跳过**，
//! 不把 `no such table` 当读错误报给用户——见 [`table_exists`]。
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
//! - **`ts` 是毫秒**（2026-10-24 实测本机：当前库首行 `1783782027226` =
//!   2026-07-11 23:00:27 本地，老库同）。SQL 的时间下界必须同单位——详见
//!   [`cutoff_epoch_ms`]。
//!
//! ## `model` 列有一段是空的，模型名从消息侧回补
//!
//! 当前库的 `model` 列**不是一直有值**（2026-10-25 实测本机 56,421 行；
//! **这个库在持续写入**，所以「还在增长的那一段」的行数会随时间变）：
//!
//! | 区间 | 写法 |
//! | --- | --- |
//! | 2026-07-11 ~ 08-06 | `minimax/MiniMax-M3`（6,836 行） |
//! | 2026-08-11 ~ 10-23 | **空**（47,955 行，占 85.0%；这段已封口，行数不再变） |
//! | 2026-10-23 ~ 至今 | `minimax/MiniMax-M3.1-Flash`（快照 1,630 行） |
//!
//! 那 47,955 行**不是某个模型**，而是 MiniMax 有两个多月没往这一列里写模型名
//! （期间用户换过 7 个模型）。**模型名并没有丢**：同一个库的
//! `local_runtime_message_rows.data_json` 里，`$.context_usage_telemetry.model`
//! 记着每一轮实际用的模型——按 `(session_id, turn_id)` 对齐就能把模型名补回去。
//!
//! 这条解析链**有对照校验**，不是猜的：
//!
//! - 显式写了模型名的 1,630 行 `minimax/MiniMax-M3.1-Flash`，按同样的键对齐后
//!   **1,630 行逐行相等、0 行不等**——**100%** 落回 `MiniMax-M3.1-Flash`；
//! - 每个轮次**恰好**对应一个模型（1,667 个轮次 = 1,667 个映射，没有一对多）；
//! - 补出来的模型与显式段**首尾相接**：`MiniMax-M3`（显式段 08-06 结束）→
//!   `MiniMax-M3`（空列段 08-11 起）→ `MiniMax-M3.1-Flash-Preview`（09-27 起）
//!   → `MiniMax-M3.1-Flash`（10-19 起）→ `MiniMax-M3.1-Flash`（显式段 10-23 起）。
//!
//! 补不回来的（实测 7 行）仍标 `minimax/未标注`——**宁可标「未标注」，不编一个**。
//!
//! ## 两个库有多大（2026-10-24 实测本机）
//!
//! | 库 | 体积 | 全表行数 | 落在 90 天窗口内 |
//! | --- | --- | --- | --- |
//! | 当前库 `v2/sqlite/runtime-state.sqlite` | 2.0 GB | 56,199 | 全部 |
//! | 老库 `sqlite.db` | 367 MB | 25,016 | 0（最后一行 2026-06-30，落在窗口外） |
//!
//! ⇒ **SQL 的时间下界必须真的生效**：下界算错单位时（秒 vs 毫秒）那一句
//! `ts >= ?1` 恒真，每次扫描都要把上面两整份读出来，再靠 Rust 侧剪掉窗口外
//! 的部分。
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

use rusqlite::{Connection, OpenFlags, Row};

use super::{derive_view, ExternalToolUsage};

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
/// 当前库里记着「每一轮实际用了哪个模型」的会话消息表。
const LIVE_MESSAGE_TABLE: &str = "local_runtime_message_rows";

/// 扫 MiniMax desktop 的用量，产出视图 + 新的增量账。
///
/// `cached` 是上一轮这个来源的状态：拿它的**逐表高水位**做增量——只读
/// `ts > 高水位` 的行，不重扫整张表。
pub(crate) fn scan(
    cutoff: &str,
    today: &str,
    cached: Option<&super::ExternalSourceState>,
) -> super::SourceScan {
    let Some(root) = super::minimax_root() else {
        return super::SourceScan {
            view: note("未找到 MiniMax desktop 的本机数据目录（~/.minimax）"),
            state: super::ExternalSourceState::default(),
            complete: false,
        };
    };
    scan_root(&root, cutoff, today, cached)
}

/// 扫一个 MiniMax 根目录，返回视图与新的增量账。
///
/// 与 [`scan`] 分开是为了**可测**：根目录由调用方给出，测试就能在临时目录里
/// 造一个 SQLite 库，不碰用户真实数据。
fn scan_root(
    root: &Path,
    cutoff: &str,
    today: &str,
    cached: Option<&super::ExternalSourceState>,
) -> super::SourceScan {
    let live = root.join(LIVE_DB);
    let legacy = root.join(LEGACY_DB);
    let cutoff_ms = cutoff_epoch_ms(cutoff);

    let mut tables: BTreeMap<String, super::MinimaxTableState> = BTreeMap::new();
    let mut notes: Vec<String> = Vec::new();
    let mut complete = true;
    // 「库文件在、用量表不在」是**版本差异**，不是读失败（见 [`table_exists`]）。
    // 这两枚计数只用来把「一个用量表都没扫到」时的提示说准。
    let mut skipped_tables = 0usize;
    let mut scanned_tables = 0usize;

    // 根目录变了 → 旧账作废（换机器 / 换 HOME）。
    let previous_tables = cached
        .filter(|state| state.root == root.to_string_lossy())
        .map(|state| &state.tables);

    for (path, table) in [(&live, LIVE_TABLE), (&legacy, LEGACY_TABLE)] {
        if !path.is_file() {
            continue;
        }
        let (conn, degraded) = match open_read_only(path) {
            Ok(pair) => pair,
            Err(reason) => {
                notes.push(reason);
                complete = false;
                continue;
            }
        };
        if degraded {
            // 忽略 WAL 读到的可能是**旧数据**：这一轮不算读全，缓存不作新鲜用
            // （否则「最近几分钟的记录不在账里」会被缓存住整个窗口）。
            complete = false;
            notes.push(format!(
                "{} 只能按「忽略 WAL」的方式读取（MiniMax 没在运行时），最近几分钟的记录可能不在账里",
                path.display()
            ));
        }
        // **「文件在但表不在」必须跳过，不能当错误报出来**（2026-10-26 用户实机）：
        // MiniMax 更新后把 `~/.minimax/sqlite.db` 复用成了另一份库（本机实测只剩
        // 一张 `agents` 表），老代码在这里 `prepare` 失败，面板上就挂一条英文的
        // 「读取 token_usage 的身份失败：no such table: token_usage」。
        // 跳过时**不动 `complete`**：这份账仍然算读全了——我们没读到的不是
        // 「这份库里的数据」，而是「这个文件本来就不是那份数据」。
        match table_exists(&conn, path, table) {
            Ok(true) => {}
            Ok(false) => {
                skipped_tables += 1;
                continue;
            }
            Err(reason) => {
                notes.push(reason);
                complete = false;
                continue;
            }
        }
        let previous = previous_tables.and_then(|tables| tables.get(table));
        match scan_table(&conn, path, table, cutoff, cutoff_ms, previous) {
            Ok((state, _rows)) => {
                tables.insert(table.to_string(), state);
                scanned_tables += 1;
            }
            Err(reason) => {
                notes.push(reason);
                complete = false;
            }
        }
    }

    let days = merge_tables(&tables);
    let state = super::ExternalSourceState {
        root: root.to_string_lossy().into_owned(),
        tables,
        ..Default::default()
    };
    if days.is_empty() && notes.is_empty() {
        // 一个用量表都没扫到、而库文件明明是有的：多半是 MiniMax 换了库/表名，
        // 提示要说「结构对不上」，不能退回「没有找到用量库」（那会让用户以为
        // 自己没装 MiniMax）。
        let reason = if scanned_tables == 0 && skipped_tables > 0 {
            "MiniMax desktop 的本地库结构已变：没有找到用量表，读不到历史用量"
        } else {
            "没有找到 MiniMax desktop 的用量库"
        };
        return super::SourceScan {
            view: note(reason),
            state,
            complete,
        };
    }
    let mut view = derive_view(
        LABEL,
        SOURCE_ID,
        &root.to_string_lossy(),
        // **会话文件数恒为 0**：MiniMax 是一份 SQLite，没有「文件」这个概念。
        // 此前这里塞的是数据库行数，界面却按「个会话文件」渲染——而行数与请求
        // 次数本来就是同一个数，等于把同一个数字换个名字再显示一遍
        // （2026-10-24 评审发现）。前端据此不显示这一项。
        0,
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
    super::SourceScan {
        view,
        state,
        complete,
    }
}

/// 由缓存里的账直接建视图（新鲜窗口内用：**不碰磁盘**）。
pub(crate) fn view_from_state(
    state: &super::ExternalSourceState,
    cutoff: &str,
    today: &str,
) -> ExternalToolUsage {
    let days = merge_tables(&state.tables);
    let mut view = derive_view(LABEL, SOURCE_ID, &state.root, 0, &days, cutoff, today);
    if view.tokens == 0 && view.requests == 0 {
        view.note = Some("近 90 天内没有读到 MiniMax desktop 的会话用量记录".to_string());
    }
    view
}

/// 把各张表的账并成一份（日期 × 模型）。
fn merge_tables(
    tables: &BTreeMap<String, super::MinimaxTableState>,
) -> BTreeMap<String, BTreeMap<String, super::ExternalModelDay>> {
    let mut days: BTreeMap<String, BTreeMap<String, super::ExternalModelDay>> = BTreeMap::new();
    for state in tables.values() {
        for (day, per_model) in &state.days {
            let bucket = days.entry(day.clone()).or_default();
            for (key, model_day) in per_model {
                let slot = bucket.entry(key.clone()).or_default();
                slot.requests += model_day.requests;
                slot.tokens += model_day.tokens;
                slot.input += model_day.input;
                slot.output += model_day.output;
                slot.cache_read += model_day.cache_read;
                slot.cache_write += model_day.cache_write;
            }
        }
    }
    days
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

/// 扫一张用量表：增量读 + 身份对账。
///
/// 返回 `(新的增量账, 这一轮新读到的行数)`。
///
/// ## 增量
///
/// 上一轮的**高水位**（已计入账目的最大 ts）是下界：只读 `ts > 高水位` 的行。
/// 实测本机那张 56,700 行的表：全量窗口查询约 200ms，而模型名回补那段
/// （47,757 行的 JSON 提取）约 1.1s——**回补才是大头**，增量之后它只查新增
/// 的那几轮，近乎为零。
///
/// ## 身份对账
///
/// 表是**只增不改**的日志，所以总行数只会增加，且增加的正好是这一轮新读到的
/// 行数。对不上（或者表里最新的那行早于我们已计入的高水位）说明**库被换掉 /
/// 被重写**——这时整表重扫。宁可多扫一次，不可少算。
fn scan_table(
    conn: &Connection,
    path: &Path,
    table: &str,
    cutoff: &str,
    cutoff_ms: i64,
    previous: Option<&super::MinimaxTableState>,
) -> Result<(super::MinimaxTableState, u64), String> {
    let (rows_total, max_ts) = table_identity(conn, table)?;
    let lower = previous.map(|state| state.high_water_ts);

    let mut state = super::MinimaxTableState {
        high_water_ts: lower.unwrap_or(0),
        rows_total,
        days: previous.map(|state| state.days.clone()).unwrap_or_default(),
        ..Default::default()
    };
    let mut scanned = 0u64;
    let mut max_seen = lower.unwrap_or(0);
    read_range(
        conn,
        path,
        table,
        cutoff_ms,
        lower,
        &mut state,
        &mut scanned,
        &mut max_seen,
    )?;

    let replaced = match previous {
        Some(previous) => {
            rows_total != previous.rows_total + scanned || max_ts < previous.high_water_ts
        }
        None => false,
    };
    if replaced {
        // 库被换掉 / 被重写：整表重扫。
        state = super::MinimaxTableState {
            high_water_ts: 0,
            rows_total,
            days: BTreeMap::new(),
            ..Default::default()
        };
        scanned = 0;
        max_seen = 0;
        read_range(
            conn,
            path,
            table,
            cutoff_ms,
            None,
            &mut state,
            &mut scanned,
            &mut max_seen,
        )?;
    }
    state.high_water_ts = max_seen;
    // 这一轮实际读了多少行：增量成功时等于「新增的那几行」。留着它是为了让
    // 「退化成全量重扫」这件事看得见（账仍然对，只是慢）。
    state.scanned_rows = scanned;
    // 窗口往前挪了：把窗口外的日期从账里剔掉（账本只留窗口内的）。
    state.days.retain(|day, _| day.as_str() >= cutoff);
    Ok((state, scanned))
}

/// 库里有没有这张表。
///
/// **「文件在、表不在」是合法的版本差异，不是读失败**：`~/.minimax/sqlite.db`
/// 的用途随 MiniMax 版本变过——老版本它是 `token_usage` 用量库，2026-10 的新版本
/// 把它复用成了另一份库（本机实测只剩一张 `agents` 表）。这类文件应**整库跳过**：
/// 让 `scan_table` 去 `prepare` 只会得到一句 `no such table`，而那句话会被当成
/// 读错误挂到面板上（2026-10-26 用户实机）。
///
/// 只有**读 `sqlite_master` 本身失败**才算错误——那才说明这个文件不是我们能读的库。
fn table_exists(conn: &Connection, path: &Path, table: &str) -> Result<bool, String> {
    let mut statement = conn
        .prepare("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1")
        .map_err(|e| format!("读取 {} 的表结构失败：{e}", path.display()))?;
    let mut rows = statement
        .query([table])
        .map_err(|e| format!("查询 {} 的表结构失败：{e}", path.display()))?;
    match rows
        .next()
        .map_err(|e| format!("遍历 {} 的表结构失败：{e}", path.display()))?
    {
        Some(_) => Ok(true),
        None => Ok(false),
    }
}

/// 表的身份：总行数 + 最大 ts。
///
/// 两条都走索引（`count(*)` 与 `max(ts)` 在本机那张 56,700 行的表上各约 0ms），
/// 所以拿它们当「库被换掉没有」的判据不花钱。
fn table_identity(conn: &Connection, table: &str) -> Result<(u64, u64), String> {
    let sql = format!("SELECT count(*), max(ts) FROM {table}");
    let mut statement = conn
        .prepare(&sql)
        .map_err(|e| format!("读取 {table} 的身份失败：{e}"))?;
    let mut rows = statement
        .query(())
        .map_err(|e| format!("查询 {table} 的身份失败：{e}"))?;
    let Some(row) = rows
        .next()
        .map_err(|e| format!("遍历 {table} 的身份失败：{e}"))?
    else {
        return Ok((0, 0));
    };
    let rows_total = row.get::<_, i64>(0).unwrap_or(0).max(0) as u64;
    let max_ts = row
        .get::<_, Option<i64>>(1)
        .unwrap_or(None)
        .unwrap_or(0)
        .max(0) as u64;
    Ok((rows_total, max_ts))
}

/// 读一段 ts 区间，把账并进 `state`。
///
/// `lower = Some(高水位)` 时只读 `ts > 高水位` 的行（增量）；`None` 时读
/// `ts >= 窗口下界` 的行（首扫 / 重扫）。
#[allow(clippy::too_many_arguments)]
fn read_range(
    conn: &Connection,
    path: &Path,
    table: &str,
    cutoff_ms: i64,
    lower: Option<u64>,
    state: &mut super::MinimaxTableState,
    scanned: &mut u64,
    max_seen: &mut u64,
) -> Result<(), String> {
    // **只有当前库能从消息侧回补模型名**：老库没有那张消息表（实测老库的
    // `session_messages.data` 里也没有 telemetry 字段），而老库的 `model`
    // 列本来就从未为空（25,016 行全有值），回补对它没有意义。
    let turn_models = if table == LIVE_TABLE {
        load_turn_models(conn, table, cutoff_ms, lower)
    } else {
        TurnModels::new()
    };
    let sql = match lower {
        Some(_) => format!(
            "SELECT session_id, turn_id, ts, model, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens \
             FROM {table} WHERE ts > ?1 AND ts >= ?2"
        ),
        None => format!(
            "SELECT session_id, turn_id, ts, model, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens \
             FROM {table} WHERE ts >= ?1"
        ),
    };
    let mut statement = conn
        .prepare(&sql)
        .map_err(|e| format!("读取 {} 失败：{e}", path.display()))?;
    let mut rows = match lower {
        Some(high) => statement.query([high as i64, cutoff_ms]),
        None => statement.query([cutoff_ms]),
    }
    .map_err(|e| format!("查询 {} 失败：{e}", path.display()))?;
    while let Some(row) = rows
        .next()
        .map_err(|e| format!("遍历 {} 失败：{e}", path.display()))?
    {
        absorb(state, scanned, max_seen, row, &turn_models);
    }
    Ok(())
}

/// 把一行用量并进账本。
fn absorb(
    state: &mut super::MinimaxTableState,
    scanned: &mut u64,
    max_seen: &mut u64,
    row: &Row<'_>,
    turn_models: &TurnModels,
) {
    let session_id: String = row.get(0).unwrap_or_default();
    let turn_id: String = row.get(1).unwrap_or_default();
    let ts: i64 = row.get(2).unwrap_or(0);
    let model: Option<String> = row.get(3).unwrap_or(None);
    let input: u64 = row.get::<_, i64>(4).unwrap_or(0).max(0) as u64;
    let output: u64 = row.get::<_, i64>(5).unwrap_or(0).max(0) as u64;
    let cache_read: u64 = row.get::<_, i64>(6).unwrap_or(0).max(0) as u64;
    let cache_write: u64 = row.get::<_, i64>(7).unwrap_or(0).max(0) as u64;
    let day = super::local_date_string_of_ms(ts.max(0) as u64);
    let key = model_key(
        model.as_deref(),
        turn_models.get(&(session_id, turn_id)).map(String::as_str),
    );
    let slot = state.days.entry(day).or_default().entry(key).or_default();
    slot.requests += 1;
    slot.tokens += input + output + cache_read + cache_write;
    slot.input += input;
    slot.output += output;
    slot.cache_read += cache_read;
    slot.cache_write += cache_write;
    *scanned += 1;
    if ts.max(0) as u64 > *max_seen {
        *max_seen = ts.max(0) as u64;
    }
}

/// `(session_id, turn_id)` → 那一轮实际用的模型名。
type TurnModels = BTreeMap<(String, String), String>;

/// 从当前库的会话消息里取出「每一轮实际用了哪个模型」。
///
/// MiniMax 在 2026-08-11 ~ 10-23 之间不再往用量表的 `model` 列写模型名
/// （实测本机那段时间 47,955 行全是 NULL），**模型名并没有丢**：同一个库的
/// `local_runtime_message_rows.data_json` 里，`$.context_usage_telemetry.model`
/// 记着每一轮实际用的模型（实测 52,379 条消息带这个字段、1,667 个不同的轮次）。
///
/// **只查真正缺模型名的那几轮**（`model` 为空的那批 `(session_id, turn_id)`），
/// 走 `(session_id, turn_id)` 索引 ⇒ 实测 0.63s，比全表扫（1.03s）快。
/// 这样也**不需要按时间裁剪消息**：查的就是要的那一轮，不存在「消息还没落库
/// 所以查不到」的时间差问题（实测用量行与消息落库时间的偏移最大 −20.7 分钟）。
///
/// 取不到的场合一律退化成空表 ⇒ 那批行仍标 `minimax/未标注`：
///
/// - 老版本的 MiniMax 没有这张表；
/// - 老的 SQLite 没编 JSON 函数（`json_extract` 用不了）；
/// - 消息行被清掉了（实测只有 7 行属于这种）。
fn load_turn_models(
    conn: &Connection,
    table: &str,
    cutoff_ms: i64,
    lower: Option<u64>,
) -> TurnModels {
    // 增量时只查**新增的那几轮**（`ts > 高水位`）——这段查询是整条链上最贵的
    // 地方（实测全量 47,757 行的 JSON 提取约 1.1s，增量近乎为零）。
    let sql = match lower {
        Some(_) => format!(
            "SELECT m.session_id, m.turn_id, \
                    json_extract(m.data_json, '$.context_usage_telemetry.model') \
             FROM (SELECT DISTINCT session_id, turn_id FROM {table} \
                   WHERE ts > ?1 AND ts >= ?2 AND (model IS NULL OR TRIM(model) = '')) u \
             JOIN {LIVE_MESSAGE_TABLE} m \
               ON m.session_id = u.session_id AND m.turn_id = u.turn_id \
             WHERE json_extract(m.data_json, '$.context_usage_telemetry.model') IS NOT NULL"
        ),
        None => format!(
            "SELECT m.session_id, m.turn_id, \
                    json_extract(m.data_json, '$.context_usage_telemetry.model') \
             FROM (SELECT DISTINCT session_id, turn_id FROM {table} \
                   WHERE ts >= ?1 AND (model IS NULL OR TRIM(model) = '')) u \
             JOIN {LIVE_MESSAGE_TABLE} m \
               ON m.session_id = u.session_id AND m.turn_id = u.turn_id \
             WHERE json_extract(m.data_json, '$.context_usage_telemetry.model') IS NOT NULL"
        ),
    };
    let Ok(mut statement) = conn.prepare(&sql) else {
        return TurnModels::new();
    };
    let Ok(mut rows) = (match lower {
        Some(high) => statement.query([high as i64, cutoff_ms]),
        None => statement.query([cutoff_ms]),
    }) else {
        return TurnModels::new();
    };
    let mut map = TurnModels::new();
    while let Ok(Some(row)) = rows.next() {
        let (Ok(session), Ok(turn), Ok(model)) = (
            row.get::<_, String>(0),
            row.get::<_, String>(1),
            row.get::<_, String>(2),
        ) else {
            continue;
        };
        map.insert((session, turn), model);
    }
    map
}

/// 模型 → `provider/model`。
///
/// `explicit` 是用量表 `model` 列的值（MiniMax 自己写的，带 `minimax/` 前缀）；
/// `resolved` 是从消息侧回补出来的、那一轮**实际用的**模型名（没有 provider
/// 前缀）。**显式值优先**——它本来就是 MiniMax 写的，最权威。
///
/// 两个都没有才落 `minimax/未标注`——**宁可标「未标注」，不编一个**。
fn model_key(explicit: Option<&str>, resolved: Option<&str>) -> String {
    let named = explicit
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .or_else(|| resolved.map(str::trim).filter(|m| !m.is_empty()));
    match named {
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

/// 本地日历日 `YYYY-MM-DD` → 该日**本地** 00:00 的 epoch **毫秒**。
///
/// **单位必须是毫秒**：MiniMax 两个库的 `ts` 列存的就是毫秒（2026-10-24 实测
/// 本机：当前库首行 `1783782027226` = 2026-07-11 23:00:27 本地，老库同）。
/// 此前这里算的是**秒**——于是 `WHERE ts >= ?1` 拿毫秒比秒，**恒真**，SQL
/// 那一层等于没过滤：每次扫描都把 2 GB 的当前库（56,199 行）与 367 MB 的老库
/// （25,016 行）整份读出来，再靠 Rust 侧按本地日历日剪掉窗口外的那部分
/// （2026-10-24 评审发现）。
///
/// **按本地时区算午夜**：下界取「截止日的本地午夜」时，窗口首日的每一行才
/// 落得到。取 UTC 午夜的话，在 UTC+8 这样的正偏移下它比本地午夜**晚 8 小时**，
/// 首日 00:00~08:00 的行会被 SQL 滤掉（那正是本机时区）。
///
/// 再往前留**一天**余量：窗口两端可能跨过夏令时切换，偏移未必相同；多读的
/// 一天由 `derive_view` 按本地日历日剪掉，不进账。
fn cutoff_epoch_ms(cutoff: &str) -> i64 {
    let Some(date) = time::Date::parse(
        cutoff,
        &time::macros::format_description!("[year]-[month]-[day]"),
    )
    .ok() else {
        return 0;
    };
    let offset = time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC);
    let local_midnight = date.midnight().assume_offset(offset);
    local_midnight.unix_timestamp_nanos().div_euclid(1_000_000) as i64 - 86_400_000
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

    /// 建出用量表。
    ///
    /// **列名与真实库一致**（`turn_id` 真实库里就有——模型名回补要靠它对齐）。
    fn create_usage_table(conn: &Connection, table: &str) {
        conn.execute(
            &format!(
                "CREATE TABLE {table} (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    session_id TEXT NOT NULL,
                    agent_name TEXT NOT NULL DEFAULT 'mavis',
                    framework_type TEXT NOT NULL DEFAULT 'pi-agent',
                    turn_id TEXT,
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
    }

    /// 造一个只含用量表的 SQLite 库（行不带轮次）。
    fn write_db(path: &Path, table: &str, rows: &[(Option<&str>, i64, i64, i64, i64)]) {
        let conn = Connection::open(path).expect("临时库应可写");
        create_usage_table(&conn, table);
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

    /// 一行**带轮次**的用量（测试用）。
    struct TurnRow<'a> {
        model: Option<&'a str>,
        session: &'a str,
        turn: &'a str,
        ts: i64,
        input: i64,
        output: i64,
        cache_read: i64,
    }

    /// 造一个**带轮次**的用量表：`turn_id` 有值，用来测「模型名从消息侧回补」。
    fn write_db_with_turns(path: &Path, table: &str, rows: &[TurnRow<'_>]) {
        let conn = Connection::open(path).expect("临时库应可写");
        create_usage_table(&conn, table);
        for row in rows {
            let model = row.model.map(|m| m.to_string());
            conn.execute(
                &format!(
                    "INSERT INTO {table}
                     (session_id, turn_id, model, ts, input_tokens, output_tokens, cache_read_tokens)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"
                ),
                rusqlite::params![
                    row.session,
                    row.turn,
                    model,
                    row.ts,
                    row.input,
                    row.output,
                    row.cache_read
                ],
            )
            .expect("写行应成功");
        }
    }

    /// 造「会话消息表」：每一行带 `context_usage_telemetry.model`。
    ///
    /// **表名与真实库一致**（`load_turn_models` 按名字找它）。
    fn write_message_table(path: &Path, rows: &[(&str, &str, &str, i64)]) {
        let conn = Connection::open(path).expect("临时库应可写");
        conn.execute(
            "CREATE TABLE local_runtime_message_rows (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                msg_id TEXT NOT NULL,
                role TEXT,
                turn_id TEXT,
                created_at_ms INTEGER NOT NULL,
                data_json TEXT NOT NULL
            )",
            [],
        )
        .expect("建表应成功");
        for (session, turn, model, created_at_ms) in rows {
            let data = format!(
                r#"{{"msg_id":"m-{turn}","turn_id":"{turn}","role":"assistant","context_usage_telemetry":{{"model":"{model}"}}}}"#
            );
            conn.execute(
                "INSERT INTO local_runtime_message_rows
                 (session_id, msg_id, role, turn_id, created_at_ms, data_json)
                 VALUES (?1, ?2, 'assistant', ?3, ?4, ?5)",
                rusqlite::params![session, format!("m-{turn}"), turn, created_at_ms, data],
            )
            .expect("写行应成功");
        }
    }

    /// 2026-10-24 12:00:00 本地的 epoch 毫秒。
    fn ts_2026_10_24() -> i64 {
        1_792_876_800_000
    }

    /// 某个本地日历日的**本地** 00:00 的 epoch **毫秒**。
    fn local_midnight_ms(date: &str) -> i64 {
        let offset = time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC);
        time::Date::parse(
            date,
            &time::macros::format_description!("[year]-[month]-[day]"),
        )
        .expect("日期应可解析")
        .midnight()
        .assume_offset(offset)
        .unix_timestamp_nanos()
        .div_euclid(1_000_000) as i64
    }

    /// 往已存在的库里**追加**行（增量扫描的第二轮用）。
    fn append_rows(path: &Path, table: &str, rows: &[(Option<&str>, i64, i64, i64, i64)]) {
        let conn = Connection::open(path).expect("临时库应可写");
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

    /// **增量**：第二轮只读新增的行，旧行**不重复计数**。
    ///
    /// 这是「避免反复扫描」的核心：账目是累加的，所以增量必须精确到「只加新
    /// 的那几行」——多算一次就把用量翻倍了。
    #[test]
    fn an_incremental_scan_adds_new_rows_without_counting_old_ones_twice() {
        let dir = TempDir::new("minimax-incremental");
        let db_dir = dir.path().join("v2/sqlite");
        std::fs::create_dir_all(&db_dir).unwrap();
        let db = db_dir.join("runtime-state.sqlite");
        write_db(
            &db,
            LIVE_TABLE,
            &[(Some("minimax/MiniMax-M3"), ts_2026_10_24(), 100, 20, 300)],
        );

        let first = scan_root(dir.path(), "2026-10-01", "2026-10-31", None);
        assert_eq!(first.view.requests, 1);
        assert_eq!(first.view.tokens, 420);

        // 追加一行（ts 更晚）。
        append_rows(
            &db,
            LIVE_TABLE,
            &[(
                Some("minimax/MiniMax-M3"),
                ts_2026_10_24() + 60_000,
                10,
                5,
                0,
            )],
        );
        let second = scan_root(dir.path(), "2026-10-01", "2026-10-31", Some(&first.state));

        assert_eq!(
            second.view.requests, 2,
            "两行就该是两次请求——旧行被重复计数了"
        );
        assert_eq!(second.view.tokens, 435, "总量 = 420 + 15");
        let state = second
            .state
            .tables
            .get(LIVE_TABLE)
            .expect("应有这张表的增量账");
        assert_eq!(
            state.high_water_ts,
            (ts_2026_10_24() + 60_000) as u64,
            "高水位推进到最新那行"
        );
        assert_eq!(state.rows_total, 2, "表里一共两行");
        assert_eq!(
            state.scanned_rows, 1,
            "增量这一轮只该读新增的那一行——读了 2 行说明退化成全量重扫了（账对，但慢）"
        );
    }

    /// 库被换掉 / 被重写时：**整表重扫**，不沿用旧账。
    ///
    /// 表是只增不改的日志，所以总行数只会增加、且增量正好等于这一轮新读到的
    /// 行数。对不上（这里用「行数变少」模拟老行被清掉）就是异常——宁可多扫
    /// 一次，不可少算。
    #[test]
    fn a_shrinking_table_triggers_a_full_rescan() {
        let dir = TempDir::new("minimax-replaced");
        let db_dir = dir.path().join("v2/sqlite");
        std::fs::create_dir_all(&db_dir).unwrap();
        let db = db_dir.join("runtime-state.sqlite");
        write_db(
            &db,
            LIVE_TABLE,
            &[
                (Some("minimax/MiniMax-M3"), ts_2026_10_24(), 100, 20, 300),
                (Some("minimax/MiniMax-M3"), ts_2026_10_24() + 1_000, 7, 7, 7),
            ],
        );
        let first = scan_root(dir.path(), "2026-10-01", "2026-10-31", None);
        assert_eq!(first.view.requests, 2);

        // 换成一份行数更少的库（老行被清掉了）。
        std::fs::remove_file(&db).unwrap();
        write_db(
            &db,
            LIVE_TABLE,
            &[(Some("minimax/MiniMax-M3"), ts_2026_10_24(), 1, 1, 1)],
        );

        let second = scan_root(dir.path(), "2026-10-01", "2026-10-31", Some(&first.state));
        assert_eq!(
            second.view.requests, 1,
            "库被换掉后必须按新库重算，不能沿用旧账"
        );
        assert_eq!(second.view.tokens, 3);
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
        let view = scan_root(dir.path(), "2026-10-01", "2026-10-31", None).view;
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
        let view = scan_root(dir.path(), "2026-10-01", "2026-10-31", None).view;
        assert_eq!(view.requests, 1, "老版本的库也要算（它覆盖更早的历史）");
        assert_eq!(view.tokens, 15);
    }

    /// 老库文件被 MiniMax 新版本**复用成别的库**（没有用量表）时：整库跳过、不报错。
    ///
    /// 2026-10-26 用户实机：`~/.minimax/sqlite.db` 只剩一张 `agents` 表（更新后
    /// 复用了这个文件名），面板上却挂着「读取 token_usage 的身份失败：no such
    /// table: token_usage」——把**版本差异**报成了读故障，还让整份账每轮都按
    /// 「没读全」重扫一次。
    #[test]
    fn a_database_without_the_usage_table_is_skipped_silently() {
        let dir = TempDir::new("legacy-repurposed");
        let db_dir = dir.path().join("v2/sqlite");
        std::fs::create_dir_all(&db_dir).unwrap();
        write_db(
            &db_dir.join("runtime-state.sqlite"),
            LIVE_TABLE,
            &[(Some("minimax/MiniMax-M3"), ts_2026_10_24(), 100, 20, 0)],
        );
        // 同名的老库：只有一张跟用量无关的表（与用户机器上的形状一致）。
        let legacy = Connection::open(dir.path().join("sqlite.db")).expect("临时库应可写");
        legacy
            .execute("CREATE TABLE agents (id INTEGER PRIMARY KEY)", [])
            .expect("建表应成功");
        drop(legacy);

        let scan = scan_root(dir.path(), "2026-10-01", "2026-10-31", None);
        assert_eq!(scan.view.requests, 1, "当前库的账照常算");
        assert_eq!(scan.view.tokens, 120);
        assert!(
            scan.view.note.is_none(),
            "老库没有用量表是版本差异，不该挂读错误：{:?}",
            scan.view.note
        );
        assert!(
            scan.complete,
            "跳过一个「本来就不是用量库」的文件，不算没读全"
        );
        assert!(
            !scan.state.tables.contains_key(LEGACY_TABLE),
            "没有这张表，就不该给它建一份增量账"
        );
    }

    /// 一个用量表都没有、但库文件明明在：提示「结构已变」，不是那句英文 SQL 错误。
    #[test]
    fn a_repurposed_database_says_the_schema_changed() {
        let dir = TempDir::new("schema-changed");
        let legacy = Connection::open(dir.path().join("sqlite.db")).expect("临时库应可写");
        legacy
            .execute("CREATE TABLE agents (id INTEGER PRIMARY KEY)", [])
            .expect("建表应成功");
        drop(legacy);

        let view = scan_root(dir.path(), "2026-10-01", "2026-10-31", None).view;
        let note = view.note.as_deref().unwrap_or("");
        assert!(note.contains("结构已变"), "要说是库的结构变了：{note}");
        assert!(
            !note.contains("no such table"),
            "不能把 SQLite 的英文报错原样挂给用户：{note}"
        );
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
        let view = scan_root(dir.path(), "2026-10-01", "2026-10-31", None).view;
        assert_eq!(view.requests, 1);
        assert_eq!(view.tokens, 15, "窗口外的行不进账");
    }

    /// `model` 列为空、消息侧也查不到那一轮时，**如实标「未标注」**。
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
        let view = scan_root(dir.path(), "2026-10-01", "2026-10-31", None).view;
        assert_eq!(view.models.len(), 1);
        assert_eq!(
            view.models[0].key, "minimax/未标注",
            "模型名缺失时如实标注，不编一个"
        );
    }

    /// `model` 列为空时，**模型名从会话消息侧回补**（同一个库）。
    ///
    /// 实测本机：MiniMax 在 2026-08-11 ~ 10-23 之间不往 `model` 列写模型名，
    /// 那 47,955 行全是 NULL；模型名本身在 `local_runtime_message_rows` 的
    /// `$.context_usage_telemetry.model` 里，按 `(session_id, turn_id)` 能对齐。
    #[test]
    fn a_blank_model_is_recovered_from_the_session_messages() {
        let dir = TempDir::new("recover");
        let db_dir = dir.path().join("v2/sqlite");
        std::fs::create_dir_all(&db_dir).unwrap();
        let db = db_dir.join("runtime-state.sqlite");
        write_db_with_turns(
            &db,
            LIVE_TABLE,
            &[TurnRow {
                model: None,
                session: "mvs_a",
                turn: "turn-1",
                ts: ts_2026_10_24(),
                input: 100,
                output: 20,
                cache_read: 300,
            }],
        );
        write_message_table(
            &db,
            &[(
                "mvs_a",
                "turn-1",
                "MiniMax-M3.1-Flash-Preview",
                ts_2026_10_24(),
            )],
        );
        let view = scan_root(dir.path(), "2026-10-01", "2026-10-31", None).view;
        assert_eq!(view.requests, 1);
        assert_eq!(view.models.len(), 1);
        assert_eq!(
            view.models[0].key, "minimax/MiniMax-M3.1-Flash-Preview",
            "列里的模型名为空时，用消息侧那一轮实际用的模型名补回去"
        );
        assert_eq!(view.models[0].model, "MiniMax-M3.1-Flash-Preview");
    }

    /// **显式值优先**：列里已经有模型名时，消息侧不覆盖它。
    #[test]
    fn an_explicit_model_is_never_overwritten_by_the_message_side() {
        let dir = TempDir::new("explicit");
        let db_dir = dir.path().join("v2/sqlite");
        std::fs::create_dir_all(&db_dir).unwrap();
        let db = db_dir.join("runtime-state.sqlite");
        write_db_with_turns(
            &db,
            LIVE_TABLE,
            &[TurnRow {
                model: Some("minimax/MiniMax-M3.1-Flash"),
                session: "mvs_a",
                turn: "turn-1",
                ts: ts_2026_10_24(),
                input: 100,
                output: 20,
                cache_read: 300,
            }],
        );
        write_message_table(&db, &[("mvs_a", "turn-1", "MiniMax-M3", ts_2026_10_24())]);
        let view = scan_root(dir.path(), "2026-10-01", "2026-10-31", None).view;
        assert_eq!(view.models.len(), 1);
        assert_eq!(
            view.models[0].key, "minimax/MiniMax-M3.1-Flash",
            "列里写着的模型名最权威，不被消息侧覆盖"
        );
    }

    /// 回补**按轮次**对齐：别的轮次的模型名不能串过来。
    #[test]
    fn the_recovered_model_is_scoped_to_its_own_turn() {
        let dir = TempDir::new("scoped");
        let db_dir = dir.path().join("v2/sqlite");
        std::fs::create_dir_all(&db_dir).unwrap();
        let db = db_dir.join("runtime-state.sqlite");
        write_db_with_turns(
            &db,
            LIVE_TABLE,
            &[
                TurnRow {
                    model: None,
                    session: "mvs_a",
                    turn: "turn-1",
                    ts: ts_2026_10_24(),
                    input: 10,
                    output: 5,
                    cache_read: 0,
                },
                TurnRow {
                    model: None,
                    session: "mvs_a",
                    turn: "turn-2",
                    ts: ts_2026_10_24(),
                    input: 20,
                    output: 10,
                    cache_read: 0,
                },
                TurnRow {
                    model: None,
                    session: "mvs_b",
                    turn: "turn-3",
                    ts: ts_2026_10_24(),
                    input: 30,
                    output: 15,
                    cache_read: 0,
                },
            ],
        );
        write_message_table(
            &db,
            &[
                ("mvs_a", "turn-1", "MiniMax-M3", ts_2026_10_24()),
                (
                    "mvs_a",
                    "turn-2",
                    "MiniMax-M3.1-Flash-Preview",
                    ts_2026_10_24(),
                ),
                ("mvs_b", "turn-3", "MiniMax-M3.1-Flash", ts_2026_10_24()),
            ],
        );
        let view = scan_root(dir.path(), "2026-10-01", "2026-10-31", None).view;
        let keys: Vec<&str> = view.models.iter().map(|m| m.key.as_str()).collect();
        // 按 tokens 降序：turn-3(45) > turn-2(30) > turn-1(15)。
        assert_eq!(
            keys,
            vec![
                "minimax/MiniMax-M3.1-Flash",
                "minimax/MiniMax-M3.1-Flash-Preview",
                "minimax/MiniMax-M3",
            ],
            "每个轮次回补自己的模型名，不串"
        );
        assert_eq!(view.models[0].tokens, 45);
    }

    /// 消息表里有那一轮、但**用量行对不上任何一轮**时，仍标「未标注」。
    #[test]
    fn an_unmatched_turn_stays_unlabelled() {
        let dir = TempDir::new("unmatched");
        let db_dir = dir.path().join("v2/sqlite");
        std::fs::create_dir_all(&db_dir).unwrap();
        let db = db_dir.join("runtime-state.sqlite");
        write_db_with_turns(
            &db,
            LIVE_TABLE,
            &[TurnRow {
                model: None,
                session: "mvs_a",
                turn: "turn-x",
                ts: ts_2026_10_24(),
                input: 10,
                output: 5,
                cache_read: 0,
            }],
        );
        write_message_table(
            &db,
            &[("mvs_a", "turn-other", "MiniMax-M3", ts_2026_10_24())],
        );
        let view = scan_root(dir.path(), "2026-10-01", "2026-10-31", None).view;
        assert_eq!(
            view.models[0].key, "minimax/未标注",
            "对不上轮次就不回补，如实标「未标注」"
        );
    }

    #[test]
    fn a_missing_source_says_so_instead_of_reporting_zero() {
        let dir = TempDir::new("empty");
        let view = scan_root(dir.path(), "2026-10-01", "2026-10-31", None).view;
        assert!(
            view.note.is_some(),
            "没有库时要说清为什么，而不是显示 0 用量"
        );
    }

    /// SQL 的下界有两个坑，两个都钉住。
    ///
    /// ① **单位**：MiniMax 库里 `ts` 是**毫秒**，下界此前算的是**秒** ⇒
    /// `ts >= ?1` 恒真，SQL 那一层等于没过滤，每次扫描都把 2 GB 的当前库
    ///    与 367 MB 的老库整份读出来（2026-10-24 评审发现）。
    /// ② **时区**：下界取「截止日的本地午夜」，否则正偏移下（UTC+8 就是
    ///    本机时区）窗口首日 00:00~08:00 的行会被滤掉。
    #[test]
    fn the_cutoff_bound_never_cuts_the_first_local_day() {
        let cutoff = "2026-10-24";
        let bound = cutoff_epoch_ms(cutoff);
        let midnight = local_midnight_ms(cutoff);
        assert!(
            bound > 1_000_000_000_000,
            "下界必须是毫秒（与库里 ts 同单位），拿秒去比毫秒等于没过滤：bound={bound}"
        );
        assert!(
            bound <= midnight,
            "下界必须不晚于窗口首日的本地午夜（否则首日的行被 SQL 滤掉）：bound={bound} midnight={midnight}"
        );
        assert!(
            bound >= midnight - 86_400_000,
            "下界最多留一天余量（多读的行由 derive_view 按本地日历日剪掉）"
        );
    }

    /// 窗口首日**本地**凌晨的行必须进账。
    #[test]
    fn the_first_local_day_is_not_lost_to_the_sql_bound() {
        let dir = TempDir::new("firstday");
        let db_dir = dir.path().join("v2/sqlite");
        std::fs::create_dir_all(&db_dir).unwrap();
        let cutoff = "2026-10-24";
        // 一行落在窗口首日的本地 00:30。
        let ts = local_midnight_ms(cutoff) + 30 * 60 * 1000;
        write_db(
            &db_dir.join("runtime-state.sqlite"),
            LIVE_TABLE,
            &[(Some("minimax/MiniMax-M3"), ts, 100, 20, 0)],
        );
        let view = scan_root(dir.path(), cutoff, "2026-10-31", None).view;
        assert_eq!(view.requests, 1, "窗口首日的行不能被 SQL 下界滤掉");
        assert_eq!(view.tokens, 120);
    }

    /// MiniMax 是一份 SQLite，**没有「会话文件」这个概念** ⇒ 该项恒为 0。
    ///
    /// 此前这里塞的是数据库行数，界面却按「个会话文件」渲染——而行数与请求
    /// 次数本来就是同一个数（2026-10-24 评审发现）。
    #[test]
    fn there_is_no_session_file_count_for_a_sqlite_source() {
        let dir = TempDir::new("nofiles");
        let db_dir = dir.path().join("v2/sqlite");
        std::fs::create_dir_all(&db_dir).unwrap();
        write_db(
            &db_dir.join("runtime-state.sqlite"),
            LIVE_TABLE,
            &[
                (Some("minimax/MiniMax-M3"), ts_2026_10_24(), 10, 5, 0),
                (Some("minimax/MiniMax-M3"), ts_2026_10_24(), 10, 5, 0),
            ],
        );
        let view = scan_root(dir.path(), "2026-10-01", "2026-10-31", None).view;
        assert_eq!(view.tracked_files, 0, "SQLite 来源没有会话文件数");
        assert_eq!(view.requests, 2, "行数就是请求次数，不另设一个数字");
    }
}
