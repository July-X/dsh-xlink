//! 外部 agent 工具的对话 token 用量：Codex（CLI / Codex Desktop）与
//! MiniMax Desktop。
//!
//! ## 这一份与内核那份账目是两回事
//!
//! [`crate::usage::local`] 记的是**内核自己写**的会话文件，按实例存；而这一份
//! 只读**别家 agent 工具**留在本机用户目录里的账。两者的 provider 命名空间不同、
//! **总量口径也不同**（Codex 报的 `cached_input_tokens` 是 `input_tokens` 的
//! **子集**，内核那套是各分量并列相加），所以它们**不混进同一份日序列**——
//! 详情页里是两块独立展示，各自按自己的口径算总量。
//!
//! ## 默认不扫
//!
//! 用户要在「模型用量」窗口勾选来源，勾选状态按壳模式持久化
//! （`shell/<mode>/usage-external-sources.json`）。这与「Codex 额度」那条
//! 「显式许可」的纪律同源：读的是别家工具留在本机的数据，**不是**模型登录的
//! 一部分，勾选之前一律不碰。
//!
//! ## 目录约定
//!
//! | 来源 | 读哪儿 | 形态 |
//! | --- | --- | --- |
//! | Codex | `$CODEX_HOME`（默认 `~/.codex`）的 `sessions/**/*.jsonl.zst` 与 `archived_sessions/*.jsonl.zst` | zstd 压的 JSONL，每行一个 JSON |
//! | MiniMax Desktop | `~/.minimax/v2/sqlite/runtime-state.sqlite` 的 `local_runtime_token_usage` 表；老版本另有 `~/.minimax/sqlite.db` 的 `token_usage` 表 | SQLite |
//!
//! 两个来源的**扫描实现各自独立**（`codex.rs` / `minimax.rs`）：文件格式、增量
//! 策略、失败语义都不同，硬塞进一个模块只会互相迁就。

pub(crate) mod codex;
pub(crate) mod minimax;

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use time::format_description::FormatItem;
use time::macros::format_description;

use crate::shell::error::AppError;
use crate::shell::{paths, settings, state};

/// 统计窗口（天，含今天）。与内核那份账目同口径：超过的记录自动丢弃。
pub const RETENTION_DAYS: u64 = 90;

/// 缓存文档的 schema 版本。结构变了就 +1，旧文档按「整体重扫」处理。
///
/// **2026-10-25 没动它**：这一轮只**新增**了字段（`tables` / `freshness`），
/// 新字段都有 `#[serde(default)]`，旧文档读出来时它们是「空」——而「空」在这里
/// 的含义恰好是「没扫过 / 不新鲜」，于是升级后的第一轮必然重扫一次。没有
/// 「半旧半新」的语义被默默继承，旧的 Codex 账（每个文件的账）也照常复用。
const STATE_SCHEMA: u32 = 1;

/// 外部来源的**新鲜度窗口**（毫秒）。
///
/// 与内核那份账目同档（`usage::local::FRESH_SCAN_INTERVAL_MS` = 90s）：窗口内
/// 的重复调用直接用缓存里的账建视图，**一个字节都不碰磁盘**。
///
/// 为什么需要它：外部来源的扫描不是免费的——MiniMax 那边即使有增量缓存，
/// 打开那份 2 GB 的库也要几百毫秒，而「模型用量」窗口的打开、刷新、切勾选框
/// 都会触发一次扫描。没有窗口时，连着开两次窗就是两次真扫。
///
/// 窗口内**不新鲜**的两种情形都要作废：跨了本地日历日（「今日用量」是按日历
/// 日算的，昨天的缓存里的「今天」是昨天的），以及上一轮**没读全**（空账被当成
/// 「已读过」缓存下来，那个来源的用量会从此一直是 0）。
pub const FRESH_SCAN_INTERVAL_MS: u64 = 90_000;

/// 用户勾选了哪些外部来源。
///
/// 这既是**持久化的偏好**，也是**单次扫描的请求**：前端勾上哪个就传哪个，
/// 后端只扫传了的那些（`false` 的来源一个字节都不读）。
#[derive(Serialize, Deserialize, Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct ExternalSources {
    /// Codex（CLI / Desktop）的会话用量。
    #[serde(default)]
    pub codex: bool,
    /// MiniMax Desktop 的会话用量。
    #[serde(default)]
    pub minimax: bool,
}

impl ExternalSources {
    /// 有没有勾选任何来源。
    pub fn any(self) -> bool {
        self.codex || self.minimax
    }
}

/// 一个外部来源的用量汇总（详情页里一张卡）。
#[derive(Serialize, Clone, Default)]
pub struct ExternalToolUsage {
    /// 来源 id：`codex` / `minimax`。前端拿它做 key，别拿界面名。
    pub source: String,
    /// 界面名：「Codex」/「MiniMax Desktop」。
    pub label: String,
    /// 读的是哪儿（目录或文件）。**空串** = 没找到这个来源的数据。
    pub source_path: String,
    /// 扫到的**会话文件**数。
    ///
    /// **MiniMax Desktop 恒为 0**：它是一份 SQLite，没有「文件」这个概念
    /// （2026-10-24 评审发现此前这里塞的是数据库行数，界面却按「个会话文件」
    /// 渲染——行数与请求次数本来就是同一个数，等于把同一个数字换个名字再显示
    /// 一遍）。前端据此**不显示这一项**。
    pub tracked_files: usize,
    /// 窗口内的总 token。**各来源按自己的口径算**（见模块文档）。
    pub tokens: u64,
    /// 窗口内的请求次数（一次模型调用 = 一次）。
    pub requests: u64,
    pub input: u64,
    pub output: u64,
    /// 缓存命中 token。**Codex 的这个数是 `input` 的子集**（别再相加）。
    pub cache_read: u64,
    pub cache_write: u64,
    /// 窗口内有用量记录的天数。
    pub active_days: u64,
    /// 按 tokens 降序。
    pub models: Vec<ExternalModelUsage>,
    /// 升序，恒覆盖整个窗口（零日补零）——与内核那份的日序列同构。
    pub days: Vec<ExternalDayUsage>,
    /// 要告诉用户的一句话（没装 / 读失败 / 数据可能不是最新的 / 窗口内没有
    /// 记录）。`None` = 一切正常。
    ///
    /// **不是「有没有数据」的开关**：数据在不在看 `tokens` 与 `requests`。
    /// 这里是「有什么话要说」。
    pub note: Option<String>,
}

/// 单个模型的合计。
#[derive(Serialize, Clone, Default)]
pub struct ExternalModelUsage {
    /// `provider/model`（与内核那份同构；provider 缺失时整串当模型名）。
    pub key: String,
    pub provider: String,
    pub model: String,
    pub requests: u64,
    pub tokens: u64,
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

/// 单日用量。与 [`crate::usage::local::DayUsageView`] 同构，好让前端复用同一
/// 套切片 / 堆叠 / 热力图函数。
#[derive(Serialize, Clone, Default)]
pub struct ExternalDayUsage {
    /// 本地日历日 `YYYY-MM-DD`。
    pub date: String,
    pub tokens: u64,
    pub requests: u64,
    pub models: BTreeMap<String, u64>,
}

/// 「一天 × 一个模型」在外部来源上的账。
///
/// **与内核那份 [`crate::usage::local::ModelDay`] 分开**：内核那份的总量是
/// 各分量**并列相加**（`input + output + cache_read + cache_write`），而 Codex
/// 报的 `cached_input_tokens` 是 `input_tokens` 的**子集**、它自己报的
/// `total_tokens` 也不等于各分量之和（`crs` provider 尤其如此）。所以这里
/// **多存一个 `tokens`**：那是该来源**自己报**的总量，读的时候直接取，
/// 绝不拿分量重算。
#[derive(Serialize, Deserialize, Clone, Default, PartialEq, Debug)]
pub struct ExternalModelDay {
    #[serde(default)]
    pub requests: u64,
    /// 该来源自己报的总量。
    #[serde(default)]
    pub tokens: u64,
    #[serde(default)]
    pub input: u64,
    #[serde(default)]
    pub output: u64,
    #[serde(default)]
    pub cache_read: u64,
    #[serde(default)]
    pub cache_write: u64,
}

/// 一个文件在账目里的位置（Codex 侧的增量单元）。
///
/// Codex 的会话文件是**每次保存整体重写**的（单帧 zstd，压缩字节每次全变），
/// 压缩层做不了增量——能用的只有「文件变了就整个重扫」。于是这里存
/// **大小 + mtime**：两者都与账目记录的一致就复用，不一致就重扫这个文件并
/// **整条替换**它的账（不做加法，所以重扫不会把同一段用量数两遍）。
#[derive(Serialize, Deserialize, Clone, Default, PartialEq, Debug)]
pub struct ExternalFileEntry {
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub mtime_ms: u64,
    /// 该文件贡献的「天 × 模型」账。
    #[serde(default)]
    pub days: BTreeMap<String, BTreeMap<String, ExternalModelDay>>,
    /// **这一份账没读全**（读失败 / 解压中途坏帧）。
    ///
    /// 标记为真的条目**下一轮一定会重扫**：它带着上一次读到的账（别让用户
    /// 看到数字凭空少一块），但**不享受「大小与 mtime 没变就跳过」的优待**——
    /// 那正是「读失败一次就永久归零」的成因（2026-10-24 评审发现）。
    #[serde(default)]
    pub stale: bool,
}

/// Codex 侧的缓存：源根目录 + 每个文件的账。
#[derive(Serialize, Deserialize, Clone, Default, PartialEq)]
pub struct ExternalSourceState {
    /// 源根目录。变了就整体重扫（换机器 / 换 `CODEX_HOME`）。
    #[serde(default)]
    pub root: String,
    /// 相对路径 → 该文件的账（**Codex 专用**）。
    #[serde(default)]
    pub files: BTreeMap<String, ExternalFileEntry>,
    /// 表名 → 该表的增量账（**MiniMax 专用**）。
    ///
    /// 与 `files` 并存而不合并成一个结构：两个来源的增量单元形状不同（Codex
    /// 是「文件」，MiniMax 是「表」），硬塞进同一个字段只会互相迁就。
    #[serde(default)]
    pub tables: BTreeMap<String, MinimaxTableState>,
    /// 新鲜度凭据：这份状态什么时候扫出来的、扫全了没有。
    #[serde(default)]
    pub freshness: SourceFreshness,
}

/// 一个来源的**新鲜度凭据**。
///
/// 新鲜度窗口按毫秒算，但「今日用量」是按**日历日**算的——跨零点时同一份缓存
/// 里的「今天」已经是昨天，这份缓存必须作废（所以除了时刻还要记日历日）。
#[derive(Serialize, Deserialize, Clone, Default, PartialEq, Debug)]
pub struct SourceFreshness {
    /// 上次扫描的时刻（毫秒）。
    #[serde(default)]
    pub scanned_at_ms: u64,
    /// 上次扫描时的本地日历日 `YYYY-MM-DD`。
    #[serde(default)]
    pub scanned_date: String,
    /// 这一轮**读全了没有**。
    ///
    /// 没读全就不许把这份状态当新鲜缓存用：空账一旦被当成「已读过」缓存下来，
    /// 那个来源的用量会从此一直是 0（2026-10-24 评审在 Codex 侧踩过同一个坑，
    /// 那边的解法是逐文件标 `stale`；这里是整源级的同一道闸）。
    #[serde(default)]
    pub complete: bool,
}

impl SourceFreshness {
    /// 这份缓存现在能不能直接用（**不碰磁盘**）。
    pub fn is_fresh(&self, now_ms: u64, today: &str) -> bool {
        self.complete
            && self.scanned_at_ms > 0
            && self.scanned_date == today
            && now_ms.saturating_sub(self.scanned_at_ms) < FRESH_SCAN_INTERVAL_MS
    }
}

/// MiniMax 一张用量表的增量账。
#[derive(Serialize, Deserialize, Clone, Default, PartialEq, Debug)]
pub struct MinimaxTableState {
    /// **已经计入账目的最大 ts**（毫秒，含）。
    ///
    /// 下一轮只读 `ts > 它` 的行——这就是增量扫描的下界。
    #[serde(default)]
    pub high_water_ts: u64,
    /// 表里的**总行数**（不限于统计窗口内）。
    ///
    /// 用来发现「库被换掉 / 被重写」：正常情况下这个数只会**增加**，且增加的
    /// 正好是这一轮新读到的行数。对不上就整表重扫——宁可多扫一次，不可少算。
    #[serde(default)]
    pub rows_total: u64,
    /// 上一轮**实际从库里读了多少行**。
    ///
    /// 增量成功时它等于「新增的那几行」；一旦等于整表行数，说明这一轮退化成
    /// 了全量重扫——账虽然对，但慢。留着它是为了让这件事**看得见**（诊断与判据
    /// 都靠它），而不是只能靠猜。
    #[serde(default)]
    pub scanned_rows: u64,
    /// 账目（日期 × 模型）。
    #[serde(default)]
    pub days: BTreeMap<String, BTreeMap<String, ExternalModelDay>>,
}

/// 一个来源扫完的产物。
pub(crate) struct SourceScan {
    /// 给界面看的那张卡。
    pub view: ExternalToolUsage,
    /// 写回缓存的状态。
    pub state: ExternalSourceState,
    /// 这一轮**读全了没有**（没读全 ⇒ 这份状态不许当新鲜缓存用）。
    pub complete: bool,
}

/// 外部来源的扫描缓存（**派生数据**，丢了可以重算）。
#[derive(Serialize, Deserialize, Clone, Default, PartialEq)]
pub struct ExternalStateDoc {
    #[serde(default)]
    pub schema: u32,
    #[serde(default)]
    pub retention_days: u64,
    #[serde(default)]
    pub last_scanned_at_ms: u64,
    /// 来源 id → 该来源的账。
    #[serde(default)]
    pub sources: BTreeMap<String, ExternalSourceState>,
}

fn state_ctx() -> state::StateCtx {
    state::StateCtx {
        corrupt: |reason| format!("外部工具用量缓存损坏（{reason}）。已丢弃缓存并重新扫描"),
        kind: AppError::Usage,
    }
}

/// 用户勾选的来源存哪儿：按壳模式隔离（与「Codex 额度」那条许可同一形状）。
fn sources_path() -> PathBuf {
    paths::shell_dir(settings::current_mode()).join("usage-external-sources.json")
}

/// 扫描缓存存哪儿：同样按壳模式隔离——它跟内核那份账目**不按实例**分家
/// （外部工具的数据是**本机用户级**的，与跑的是哪个实例无关）。
fn state_path() -> PathBuf {
    paths::shell_dir(settings::current_mode()).join("usage-external-state.json")
}

/// 读回用户勾选的来源。文件不存在 = 一个都没勾（默认不扫）。
pub(crate) fn saved_sources() -> ExternalSources {
    state::load_lossy::<ExternalSources>(&sources_path())
}

/// 持久化用户勾选的来源。
pub(crate) fn save_sources(sources: ExternalSources) -> Result<ExternalSources, String> {
    state::save(&sources_path(), &sources, state_ctx()).map_err(|e| e.to_string())?;
    Ok(sources)
}

/// 缓存文档的读写。损坏时**整体丢弃重扫**——这份文档整份都能从会话文件重算，
/// 拒绝读写只会让用户的统计凭空消失。
///
/// **schema 变了也一样整体重扫**（2026-10-24 补上：这条此前只写在注释里，
/// 没实现——旧文档的字段名变了以后，`#[serde(default)]` 会把缺的字段默默补成
/// 0，于是拿一份**半旧半新**的账继续用，比全量重扫更坏）。作废时留一条
/// shell 事件，说清「为什么这轮数字变了」。
fn load_state() -> ExternalStateDoc {
    let (doc, corrupted): (ExternalStateDoc, _) = state::load_rebuildable(&state_path());
    if let Some(reason) = corrupted {
        crate::shell::shell_events::record(
            "usage-external-state",
            &format!(
                "外部工具用量缓存损坏，已从零重扫（{reason}）：{}",
                state_path().display()
            ),
        );
        return ExternalStateDoc::default();
    }
    if doc.schema != STATE_SCHEMA {
        crate::shell::shell_events::record(
            "usage-external-state",
            &format!(
                "外部工具用量缓存版本变了（{} → {STATE_SCHEMA}），已从零重扫：{}",
                doc.schema,
                state_path().display()
            ),
        );
        return ExternalStateDoc::default();
    }
    doc
}

/// Codex 的本机数据目录：遵循 `CODEX_HOME`，未设置时用 `<home>/.codex`。
///
/// 刻意**不**搜索其它目录——Codex 自己在别处放副本（如 `~/.mavis/codex-auth.json`
/// 那种）不代表那是它的会话目录，认错地方等于读到另一份数据。
pub(crate) fn codex_root() -> Option<PathBuf> {
    codex_root_from(std::env::var_os("CODEX_HOME"), paths::dirs_home())
}

/// [`codex_root`] 的可测形态：判断全在这里，环境变量由调用方读出传入。
fn codex_root_from(codex_home: Option<OsString>, home: PathBuf) -> Option<PathBuf> {
    let root = match codex_home.filter(|value| !value.is_empty()) {
        Some(value) => PathBuf::from(value),
        None => home.join(".codex"),
    };
    if root.is_absolute() {
        Some(root)
    } else {
        None
    }
}

/// MiniMax desktop 的本机数据目录。
///
/// `~/.minimax` 是当前的位置；`~/.mavis` 是老名字（这台机器上它是
/// `~/.minimax` 的软链接），认两个是为了不把「换过目录的用户」判成没装。
pub(crate) fn minimax_root() -> Option<PathBuf> {
    let home = paths::dirs_home();
    for name in [".minimax", ".mavis"] {
        let candidate = home.join(name);
        if candidate.is_dir() {
            return Some(candidate);
        }
    }
    None
}

/// 窗口的第一天（本地日历日）。
pub(crate) fn cutoff_date_string(now_ms: u64) -> String {
    local_date_string_of_ms(now_ms.saturating_sub((RETENTION_DAYS - 1) * 86_400_000))
}

/// epoch 毫秒 → 本地日历日 `YYYY-MM-DD`。
pub(crate) fn local_date_string_of_ms(ms: u64) -> String {
    crate::shell::localtime::local_date_string(
        std::time::UNIX_EPOCH + std::time::Duration::from_millis(ms),
    )
}

/// 窗口内的每一天（升序，含首尾）。窗口外的日期不产出。
pub(crate) fn window_dates(cutoff: &str, today: &str) -> Vec<String> {
    const DAY_FORMAT: &[FormatItem<'static>] = format_description!("[year]-[month]-[day]");
    let mut dates = Vec::new();
    let mut cursor = time::Date::parse(cutoff, DAY_FORMAT).ok();
    let end = time::Date::parse(today, DAY_FORMAT).ok();
    while let Some(date) = cursor {
        dates.push(date.format(DAY_FORMAT).unwrap_or_default());
        if Some(date) == end {
            break;
        }
        match date.next_day() {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }
    dates
}

/// 把「天 × 模型」的账求和成视图。
///
/// 与 [`crate::usage::local::derive_view`] 同一形状：日序列恒覆盖整个窗口
/// （零日补零），模型按 tokens 降序。
pub(crate) fn derive_view(
    label: &str,
    source: &str,
    source_path: &str,
    tracked_files: usize,
    days_by_model: &BTreeMap<String, BTreeMap<String, ExternalModelDay>>,
    cutoff: &str,
    today: &str,
) -> ExternalToolUsage {
    let mut models: BTreeMap<String, ExternalModelUsage> = BTreeMap::new();
    // 日期 → (总量, 请求数, 按模型的量)
    let mut by_day: BTreeMap<String, (u64, u64, BTreeMap<String, u64>)> = BTreeMap::new();
    let mut totals = ExternalModelUsage::default();
    for (day, per_model) in days_by_model {
        if day.as_str() < cutoff {
            continue;
        }
        let day_bucket = by_day.entry(day.clone()).or_insert((0, 0, BTreeMap::new()));
        for (key, model_day) in per_model {
            let model = models.entry(key.clone()).or_insert_with(|| {
                let (provider, model) = key.split_once('/').unwrap_or((key.as_str(), ""));
                ExternalModelUsage {
                    key: key.clone(),
                    provider: provider.to_string(),
                    model: model.to_string(),
                    ..Default::default()
                }
            });
            model.requests += model_day.requests;
            model.tokens += model_day.tokens;
            model.input += model_day.input;
            model.output += model_day.output;
            model.cache_read += model_day.cache_read;
            model.cache_write += model_day.cache_write;
            totals.requests += model_day.requests;
            totals.tokens += model_day.tokens;
            totals.input += model_day.input;
            totals.output += model_day.output;
            totals.cache_read += model_day.cache_read;
            totals.cache_write += model_day.cache_write;
            day_bucket.0 += model_day.tokens;
            day_bucket.1 += model_day.requests;
            *day_bucket.2.entry(key.clone()).or_insert(0) += model_day.tokens;
        }
    }
    let days: Vec<ExternalDayUsage> = window_dates(cutoff, today)
        .into_iter()
        .map(|date| {
            let (tokens, requests, models) =
                by_day.remove(&date).unwrap_or((0, 0, BTreeMap::new()));
            ExternalDayUsage {
                date,
                tokens,
                requests,
                models,
            }
        })
        .collect();
    let mut models: Vec<ExternalModelUsage> = models.into_values().collect();
    models.sort_by(|a, b| b.tokens.cmp(&a.tokens).then_with(|| a.key.cmp(&b.key)));
    let active_days = days.iter().filter(|d| d.requests > 0).count() as u64;
    ExternalToolUsage {
        source: source.to_string(),
        label: label.to_string(),
        source_path: source_path.to_string(),
        tracked_files,
        tokens: totals.tokens,
        requests: totals.requests,
        input: totals.input,
        output: totals.output,
        cache_read: totals.cache_read,
        cache_write: totals.cache_write,
        active_days,
        models,
        days,
        note: None,
    }
}

/// ISO 8601 时刻（`2026-10-13T14:49:08.080Z`）→ 本地日历日 `YYYY-MM-DD`。
///
/// Codex 的行内 `timestamp` 是 **UTC** 的 ISO 8601 串；归属到哪个日历日必须
/// 换算到用户本地时区再取日期——Codex 的目录名（`sessions/2026/10/13/`）是
/// **会话开始**那天，一个会话跨午夜时会横跨多个日历日。
pub(crate) fn local_day_from_iso(timestamp: &str) -> Option<String> {
    let parsed =
        time::OffsetDateTime::parse(timestamp, &time::format_description::well_known::Rfc3339)
            .ok()?;
    let offset = time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC);
    parsed
        .to_offset(offset)
        .format(&format_description!("[year]-[month]-[day]"))
        .ok()
}

/// 扫描用户勾选的来源，返回每个来源一张卡。
///
/// `sources` 里的每一项都会被扫——**没勾的一个字节都不读**。
///
/// ## 新鲜度窗口
///
/// 窗口内（[`FRESH_SCAN_INTERVAL_MS`]，且还是同一个本地日历日、且上一轮读全了）
/// 的重复调用**不碰磁盘**：直接由缓存里的账建视图。`force`（用户点了「刷新」）
/// 无视窗口——显式刷新必须真的重扫。
pub(crate) fn scan(sources: ExternalSources, now_ms: u64, force: bool) -> Vec<ExternalToolUsage> {
    let cutoff = cutoff_date_string(now_ms);
    let today = local_date_string_of_ms(now_ms);
    let mut doc = load_state();
    let mut out = Vec::new();
    let mut dirty = false;

    if sources.codex {
        let cached = doc.sources.get(codex::SOURCE_ID).cloned();
        match cached_view(&doc, codex::SOURCE_ID, &cutoff, &today, now_ms, force) {
            Some(view) => out.push(view),
            None => {
                let scanned = codex::scan(&cutoff, &today, now_ms, cached.as_ref());
                out.push(scanned.view);
                doc.sources.insert(
                    codex::SOURCE_ID.to_string(),
                    stamp(scanned.state, now_ms, &today, scanned.complete),
                );
                dirty = true;
            }
        }
    }
    if sources.minimax {
        let cached = doc.sources.get(minimax::SOURCE_ID).cloned();
        match cached_view(&doc, minimax::SOURCE_ID, &cutoff, &today, now_ms, force) {
            Some(view) => out.push(view),
            None => {
                let scanned = minimax::scan(&cutoff, &today, cached.as_ref());
                out.push(scanned.view);
                doc.sources.insert(
                    minimax::SOURCE_ID.to_string(),
                    stamp(scanned.state, now_ms, &today, scanned.complete),
                );
                dirty = true;
            }
        }
    }

    if dirty {
        doc.schema = STATE_SCHEMA;
        doc.retention_days = RETENTION_DAYS;
        doc.last_scanned_at_ms = now_ms;
        // 缓存是派生数据：写失败只丢一次全量重扫，不该阻断调用方。
        state::save_best_effort(&state_path(), &doc);
    }
    out
}

/// 这一轮能不能直接用缓存里的账建视图（**不碰磁盘**）。
///
/// `force`（用户点了「刷新」）无视窗口——显式刷新必须真的重扫。
///
/// 窗口内还要满足两件事：还是**同一个本地日历日**（「今日用量」按日历日算，
/// 昨天的缓存里的「今天」是昨天的），以及上一轮**读全了**（没读全就不许把
/// 空账当成「已读过」缓存下来）。
fn cached_view(
    doc: &ExternalStateDoc,
    source: &str,
    cutoff: &str,
    today: &str,
    now_ms: u64,
    force: bool,
) -> Option<ExternalToolUsage> {
    if force {
        return None;
    }
    let state = doc.sources.get(source)?;
    if !state.freshness.is_fresh(now_ms, today) {
        return None;
    }
    match source {
        codex::SOURCE_ID => Some(codex::view_from_state(state, cutoff, today)),
        minimax::SOURCE_ID => Some(minimax::view_from_state(state, cutoff, today)),
        _ => None,
    }
}

/// 给一次扫描的结果盖上新鲜度凭据。
fn stamp(
    mut state: ExternalSourceState,
    now_ms: u64,
    today: &str,
    complete: bool,
) -> ExternalSourceState {
    state.freshness = SourceFreshness {
        scanned_at_ms: now_ms,
        scanned_date: today.to_string(),
        complete,
    };
    state
}

/// 设置用户勾选的外部来源。
///
/// 权限在 `allow-usage-viewer`（模型用量窗口）。这是**唯一**的写入口：
/// 前端勾上 / 取消勾选都走它，勾选状态按壳模式持久化。
#[tauri::command]
pub async fn set_usage_external_sources(
    sources: ExternalSources,
) -> Result<ExternalSources, String> {
    crate::commands::blocking(move || save_sources(sources)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_root_follows_codex_home_then_home_dot_codex() {
        let home = PathBuf::from("/Users/tester");
        assert_eq!(
            codex_root_from(None, home.clone()),
            Some(PathBuf::from("/Users/tester/.codex"))
        );
        assert_eq!(
            codex_root_from(Some(OsString::from("/opt/codex")), home.clone()),
            Some(PathBuf::from("/opt/codex"))
        );
        // 空串视作没设——否则 `CODEX_HOME=` 会把根目录解析成相对路径。
        assert_eq!(
            codex_root_from(Some(OsString::from("")), home.clone()),
            Some(PathBuf::from("/Users/tester/.codex"))
        );
        // 相对路径直接拒绝：读别人产品的目录，猜错比读不到更糟。
        assert_eq!(
            codex_root_from(Some(OsString::from("relative")), home),
            None
        );
    }

    /// 造一份「刚扫过、读全了」的缓存文档。
    fn fresh_doc(now_ms: u64, today: &str) -> ExternalStateDoc {
        let mut days: BTreeMap<String, BTreeMap<String, ExternalModelDay>> = BTreeMap::new();
        days.entry("2026-10-24".to_string()).or_default().insert(
            "codex/gpt".to_string(),
            ExternalModelDay {
                requests: 2,
                tokens: 300,
                input: 100,
                output: 50,
                cache_read: 150,
                cache_write: 0,
            },
        );
        ExternalStateDoc {
            schema: STATE_SCHEMA,
            retention_days: RETENTION_DAYS,
            last_scanned_at_ms: now_ms,
            sources: BTreeMap::from([(
                codex::SOURCE_ID.to_string(),
                ExternalSourceState {
                    root: "/tmp/codex".to_string(),
                    files: BTreeMap::from([(
                        "sessions/a.jsonl.zst".to_string(),
                        ExternalFileEntry {
                            size: 1,
                            mtime_ms: 1,
                            days,
                            stale: false,
                        },
                    )]),
                    tables: BTreeMap::new(),
                    freshness: SourceFreshness {
                        scanned_at_ms: now_ms,
                        scanned_date: today.to_string(),
                        complete: true,
                    },
                },
            )]),
        }
    }

    const NOW: u64 = 1_792_876_800_000;

    /// 窗口内：**吃缓存建视图，一个字节都不碰磁盘**。
    ///
    /// 这条钉的是「连着开两次窗 = 两次真扫」那个缺口：外部来源的扫描不是免费
    /// 的（MiniMax 那张表全量扫要 1s 上下），窗口内必须直接用缓存。
    #[test]
    fn a_fresh_cache_serves_the_view_without_touching_the_disk() {
        let doc = fresh_doc(NOW, "2026-10-24");
        let view = cached_view(
            &doc,
            codex::SOURCE_ID,
            "2026-10-01",
            "2026-10-24",
            NOW,
            false,
        )
        .expect("窗口内的缓存应当直接建视图");
        assert_eq!(view.tokens, 300);
        assert_eq!(view.requests, 2);
    }

    /// 用户点了「刷新」：**无视窗口**，必须真扫。
    #[test]
    fn an_explicit_refresh_ignores_the_freshness_window() {
        let doc = fresh_doc(NOW, "2026-10-24");
        assert!(
            cached_view(
                &doc,
                codex::SOURCE_ID,
                "2026-10-01",
                "2026-10-24",
                NOW,
                true
            )
            .is_none(),
            "显式刷新必须无视新鲜度窗口"
        );
    }

    /// 出了窗口：必须真扫。
    #[test]
    fn a_cache_older_than_the_window_is_not_served() {
        let doc = fresh_doc(NOW, "2026-10-24");
        let later = NOW + FRESH_SCAN_INTERVAL_MS;
        assert!(
            cached_view(
                &doc,
                codex::SOURCE_ID,
                "2026-10-01",
                "2026-10-24",
                later,
                false
            )
            .is_none(),
            "窗口外必须重扫"
        );
    }

    /// **跨了本地日历日**：缓存里的「今天」已经是昨天，必须作废。
    ///
    /// 新鲜度窗口按毫秒算（90s），而「今日用量」按日历日算——23:59:50 扫的
    /// 缓存，00:00:10 拿来用，「今日」就整整差了一天。
    #[test]
    fn a_cache_from_another_day_is_not_served() {
        let doc = fresh_doc(NOW, "2026-10-24");
        assert!(
            cached_view(
                &doc,
                codex::SOURCE_ID,
                "2026-10-01",
                "2026-10-25",
                NOW,
                false
            )
            .is_none(),
            "跨了日历日的缓存必须作废"
        );
    }

    /// 上一轮**没读全**：不许当新鲜缓存用。
    ///
    /// 空账一旦被当成「已读过」缓存下来，那个来源的用量会从此一直是 0
    /// （2026-10-24 评审在 Codex 侧踩过同一个坑，那边是逐文件标 `stale`）。
    #[test]
    fn an_incomplete_scan_is_never_served_as_fresh() {
        let mut doc = fresh_doc(NOW, "2026-10-24");
        if let Some(state) = doc.sources.get_mut(codex::SOURCE_ID) {
            state.freshness.complete = false;
        }
        assert!(
            cached_view(
                &doc,
                codex::SOURCE_ID,
                "2026-10-01",
                "2026-10-24",
                NOW,
                false
            )
            .is_none(),
            "没读全的一轮不许被当成新鲜缓存"
        );
    }

    /// 没有缓存的那个来源：不能拿别的来源的缓存凑数。
    #[test]
    fn a_source_without_a_cache_is_never_served() {
        let doc = fresh_doc(NOW, "2026-10-24");
        assert!(
            cached_view(
                &doc,
                minimax::SOURCE_ID,
                "2026-10-01",
                "2026-10-24",
                NOW,
                false
            )
            .is_none(),
            "没扫过的来源必须真扫"
        );
    }

    #[test]
    fn window_dates_covers_the_whole_window_including_today() {
        let dates = window_dates("2026-07-27", "2026-10-24");
        assert_eq!(dates.len(), 90);
        assert_eq!(dates.first().map(String::as_str), Some("2026-07-27"));
        assert_eq!(dates.last().map(String::as_str), Some("2026-10-24"));
    }

    #[test]
    fn derive_view_sums_models_and_zero_fills_the_window() {
        let mut days: BTreeMap<String, BTreeMap<String, ExternalModelDay>> = BTreeMap::new();
        days.entry("2026-10-23".to_string()).or_default().insert(
            "minimax/MiniMax-M3".to_string(),
            ExternalModelDay {
                requests: 2,
                tokens: 300,
                input: 100,
                output: 50,
                cache_read: 150,
                cache_write: 0,
            },
        );
        let view = derive_view(
            "MiniMax Desktop",
            "minimax",
            "/tmp/x",
            0,
            &days,
            "2026-10-20",
            "2026-10-24",
        );
        assert_eq!(view.tokens, 300);
        assert_eq!(view.requests, 2);
        assert_eq!(view.active_days, 1);
        assert_eq!(view.days.len(), 5, "窗口内的每一天都要有一格");
        assert_eq!(view.days[0].date, "2026-10-20");
        assert_eq!(view.days[0].tokens, 0, "零日补零");
        assert_eq!(view.days[3].tokens, 300);
        assert_eq!(view.models.len(), 1);
        assert_eq!(view.models[0].provider, "minimax");
        assert_eq!(view.models[0].model, "MiniMax-M3");
    }

    #[test]
    fn derive_view_drops_days_outside_the_window() {
        let mut days: BTreeMap<String, BTreeMap<String, ExternalModelDay>> = BTreeMap::new();
        days.entry("2026-01-01".to_string()).or_default().insert(
            "codex/gpt".to_string(),
            ExternalModelDay {
                requests: 1,
                tokens: 999,
                ..Default::default()
            },
        );
        let view = derive_view(
            "Codex",
            "codex",
            "/tmp/y",
            1,
            &days,
            "2026-10-20",
            "2026-10-24",
        );
        assert_eq!(view.tokens, 0, "窗口外的账不进汇总");
        assert_eq!(view.days.len(), 5);
    }

    /// 序列化出来的字段名是**跨语言契约**：前端模板按这些名字读
    /// （`src.tokens` / `m.provider` / `row.date`…）。改名时前端不会报任何错——
    /// `{{ formatTokens(undefined) }}` 渲染成 "0"，界面看着「用量是 0」，实则字段
    /// 对不上。前端那侧有对应的判据（`ui/test/usageExternal.test.js`），这里是
    /// 后端这侧把字段清单钉住，两边各钉一头。
    ///
    /// **改名时两处一起改**：这里钉的字段清单，和 `UsageWindow.vue` 里的读法。
    #[test]
    fn serialized_field_names_are_the_cross_language_contract() {
        let tool = serde_json::to_value(ExternalToolUsage {
            source: "codex".into(),
            label: "Codex".into(),
            source_path: "/tmp/y".into(),
            tracked_files: 1,
            tokens: 2,
            requests: 3,
            input: 4,
            output: 5,
            cache_read: 6,
            cache_write: 7,
            active_days: 8,
            models: Vec::new(),
            days: Vec::new(),
            note: None,
        })
        .expect("序列化成 JSON");
        let mut tool_keys: Vec<&str> = tool
            .as_object()
            .expect("是个对象")
            .keys()
            .map(|k| k.as_str())
            .collect();
        tool_keys.sort_unstable();
        assert_eq!(
            tool_keys,
            vec![
                "active_days",
                "cache_read",
                "cache_write",
                "days",
                "input",
                "label",
                "models",
                "note",
                "output",
                "requests",
                "source",
                "source_path",
                "tokens",
                "tracked_files",
            ],
            "ExternalToolUsage 的字段名是对外契约，改名要连前端一起改"
        );

        let model = serde_json::to_value(ExternalModelUsage {
            key: "openai/gpt".into(),
            provider: "openai".into(),
            model: "gpt".into(),
            requests: 1,
            tokens: 2,
            input: 3,
            output: 4,
            cache_read: 5,
            cache_write: 6,
        })
        .expect("序列化成 JSON");
        let mut model_keys: Vec<&str> = model
            .as_object()
            .expect("是个对象")
            .keys()
            .map(|k| k.as_str())
            .collect();
        model_keys.sort_unstable();
        assert_eq!(
            model_keys,
            vec![
                "cache_read",
                "cache_write",
                "input",
                "key",
                "model",
                "output",
                "provider",
                "requests",
                "tokens",
            ],
            "ExternalModelUsage 的字段名是对外契约"
        );

        let day = serde_json::to_value(ExternalDayUsage {
            date: "2026-10-24".into(),
            tokens: 1,
            requests: 2,
            models: BTreeMap::new(),
        })
        .expect("序列化成 JSON");
        let mut day_keys: Vec<&str> = day
            .as_object()
            .expect("是个对象")
            .keys()
            .map(|k| k.as_str())
            .collect();
        day_keys.sort_unstable();
        assert_eq!(
            day_keys,
            vec!["date", "models", "requests", "tokens"],
            "ExternalDayUsage 的字段名是对外契约"
        );
    }
}
