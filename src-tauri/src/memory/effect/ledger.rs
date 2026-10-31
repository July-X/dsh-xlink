//! 注入账本（P1）：插件每一次「真的注入 / 为什么没注入」的事实流水。
//!
//! ## 它回答什么问题
//!
//! 效果扫描（\`effect::scan\`）只能从内核会话日志里**倒推**注入：快照存在、请求带
//! usage、工具被调用。它答不了「这一回合为什么没有记忆」——预算用完、开关关着、零命中、
//! 桥失败在内核日志里长得一模一样（都是「什么都没发生」）。账本让插件在**当时**说一句
//! 原因，壳按 UTC 月份追加到 JSONL，面板再按窗口读。
//!
//! ## 布局
//!
//! 目录：\`<instance_memory_dir>/effect/ledger/\`（[\`dir_under\`]，\`paths::instance_memory_effect_dir\`
//! 的下级）；文件：\`notes-YYYY-MM.jsonl\`（按 **UTC** 月份分，[\`file_for\`]）。
//! 按 UTC 分月是刻意的：账本文件只是容器，**窗口过滤按本地日历日**（[\`read_in_dir\`]
//! 的 from/to 走 \`usage::local::date_string_of_ms\`，与效果扫描同一口径）——分月若跟本地
//! 时区走，跨时区换机器时同一批行会落到不同文件，却没有一条读得出来的规则。
//!
//! ## 三条硬约束
//!
//! 1. **绝不落正文/标题**：一行只有 id、字节数、计数、会话/工作区 id 与版本号（见
//!    [\`LedgerNote\`]）。标题在出报告时按 id 从 memory.json 现查，与效果账本同一条隐私红线。
//! 2. **写失败绝不打断 note 路由**：追加失败只写一行 \`shell_events\`（逻辑名
//!    \`memory-effect-ledger\`），调用方照常回 \`{"ok":true}\`——账本是观测数据，不是权威记录。
//! 3. **坏行跳过**：读取时逐行 \`serde_json\` 解析，任何一行坏了只丢那一行（不丢整份、
//!    也不报错）——半行 JSON 是进程被杀时最常见的残留。
//!
//! 保留 90 天（[\`RETENTION_DAYS\`]）：每次成功追加后顺手删掉窗口之前的**月份文件**
//! （best-effort；按整月删，窗口内的日行由读取侧再过滤）。

use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::shell::shell_events;
use crate::usage::local::date_string_of_ms;

/// 账本保留窗口（天，含今天），与效果账目（\`effect::RETENTION_DAYS\`）同一档。
pub const RETENTION_DAYS: u64 = 90;

/// 账本目录名：挂在 \`<instance_memory_dir>/effect/\` 下。
pub const DIR_NAME: &str = "ledger";

/// 一条 note 最多记多少个记忆 id。
///
/// 上限来自合同 §1.2（与桥的「最近注入」环形同档）：ids 会随查询串进来，没有上限时
/// 一条记录就能把 JSONL 行撑到与查询串同一量级，而账本是按月追加的流水。
pub const MAX_IDS: usize = 20;

/// \`kind\`：一次真的进了上下文的注入（含「把旧正文清掉」这种内容变化）。
pub const KIND_INJECT: &str = "inject";
/// \`kind\`：这一回合没有注入，\`reason\` 说明为什么。
pub const KIND_SKIP: &str = "skip";

const DAY_MS: u64 = 86_400_000;
const FILE_PREFIX: &str = "notes-";
const FILE_SUFFIX: &str = ".jsonl";

/// 账本行（也是 \`/v1/memory/note\` 落盘的唯一格式）。
///
/// 字段名与合同 §1.2 逐字一致（serde snake_case 即字段名，**不加 rename**）：
/// 插件、壳、报告三处跨进程/跨语言对齐，改名没有编译器管，只能靠测试与合同。
///
/// \`turn\` / \`step\` 用 \`i64\`：插件 pre-step 只有 turn，step 传 -1；旧插件不带这两个参数时
/// 也是 -1（「不知道」），报告里 turn<0 的关联按近似处理。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LedgerNote {
    /// 事件时刻（epoch 毫秒）。由壳在 \`note\` 路由里取当前时间，**不信插件传的时钟**。
    pub at_ms: u64,
    /// 内核会话 id（与 \`sessions/<ws>/session-<uuid>\` 目录同名）。
    pub session: String,
    /// 回合号（插件 pre-step 的 turn；旧插件 -1）。
    pub turn: i64,
    /// 步号（插件 pre-step 拿不到，恒 -1）。
    pub step: i64,
    /// \`inject\` / \`skip\`（见 [\`KIND_INJECT\`] / [\`KIND_SKIP\`]）。
    pub kind: String,
    /// inject 的触发路径（\`turn\` / \`library_change\`）或 skip 的原因桶
    /// （\`bridge_failed\` / \`budget\` / \`no_hit\` / \`no_change\` / \`disabled\`）。
    pub reason: String,
    /// 本回合开关是否开着（\`enabled=1\`）。
    pub enabled: bool,
    /// 本次变化涉及的记忆 id（32 位 hex）；skip 恒为空。最多 [\`MAX_IDS\`] 个。
    pub ids: Vec<String>,
    /// 本次变化段落（delta）的 UTF-8 字节数；skip 恒为 0。
    pub bytes: u64,
    /// 记忆库代次（\`result.value.generation\`）；拿不到为 0。
    pub generation: u64,
    /// 插件解析出的内核工作区 id（解析不到为空串）。
    pub workspace_id: String,
    /// 插件版本（插件读自身 \`package.json\`；读不到为 \`unknown\`）。
    pub plugin_version: String,
    /// 内核版本（壳按桥绑定的实例记录补；取不到为空串）。
    pub kernel_version: String,
}

impl LedgerNote {
    /// 这条记录是不是一次注入。报告与测试共用这一个判据，别各写一个 \`== "inject"\`。
    pub fn is_inject(&self) -> bool {
        self.kind == KIND_INJECT
    }
}

/// \`<effect_dir>/ledger/\`。账本目录只由 [\`append\`] 创建（best-effort），读取侧不建。
pub fn dir_under(effect_dir: &Path) -> PathBuf {
    effect_dir.join(DIR_NAME)
}

/// 账本文件：\`<ledger_dir>/notes-YYYY-MM.jsonl\`（UTC 月份）。
pub fn file_for(ledger_dir: &Path, at_ms: u64) -> PathBuf {
    ledger_dir.join(format!(
        "{FILE_PREFIX}{}{FILE_SUFFIX}",
        utc_month_of_ms(at_ms)
    ))
}

/// 把查询串里的 \`ids\` 归一成账本行：逗号分隔、去空白、丢空串、最多 [\`MAX_IDS\`] 个。
///
/// **只做形状归一，不查库**：账本记的是「插件声称本次变化涉及哪些 id」，标题与可见性
/// 由报告侧按 id 现查——这里多读一次库既慢，也会把「id 查不到」误当成「没注入」。
pub fn normalize_ids(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .take(MAX_IDS)
        .map(str::to_string)
        .collect()
}

/// 追加一条账本行。**永不返回错误、永不 panic 到调用方**。
///
/// 失败（目录建不出来、磁盘满、权限）只写一行 \`shell_events\`：note 路由的响应是插件的
/// 可见性信号，账本写不进去不该让插件以为「注入没发生」而去重试。
///
/// 成功之后顺手清理 90 天前的月份文件（best-effort，失败静默——删除失败不影响本行已落盘）。
pub fn append(ledger_dir: &Path, note: &LedgerNote) {
    let path = file_for(ledger_dir, note.at_ms);
    if let Err(error) = append_line(&path, note) {
        shell_events::record(
            "memory-effect-ledger",
            &format!(
                "注入账本追加失败（{}）：{error}；这一条只缺观测数据，不影响记忆本身",
                path.display()
            ),
        );
        return;
    }
    prune_old_months(ledger_dir, note.at_ms);
}

/// 读窗口内的账本行：\`[from_day, to_day]\` 是**本地日历日** \`YYYY-MM-DD\`（含两端）。
///
/// 逐行解析、坏行跳过；按 \`at_ms\` 升序返回（同毫秒保持文件内顺序）。目录不存在
/// 或读不动都返回空表——「账本未启用」是合法状态，不是错误。
pub fn read_in_dir(ledger_dir: &Path, from_day: &str, to_day: &str) -> Vec<LedgerNote> {
    let mut notes = Vec::new();
    let Ok(entries) = fs::read_dir(ledger_dir) else {
        return notes;
    };
    // 文件名排序 = 月份升序，读出来的行天然接近时间序；最后仍按 at_ms 排一次，
    // 因为一个月的文件里行是按到达顺序追加的（插件时钟与壳时钟可能交错）。
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .and_then(month_of_file_name)
                .is_some()
        })
        .collect();
    files.sort();
    for path in files {
        let Ok(file) = fs::File::open(&path) else {
            continue;
        };
        for line in BufReader::new(file).lines() {
            // 读错误（权限、I/O）只停这一个文件：已读出的行仍有效，其余文件照读。
            let Ok(line) = line else {
                break;
            };
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let Ok(note) = serde_json::from_str::<LedgerNote>(line) else {
                continue;
            };
            let day = date_string_of_ms(note.at_ms);
            if day.as_str() < from_day || day.as_str() > to_day {
                continue;
            }
            notes.push(note);
        }
    }
    notes.sort_by_key(|note| note.at_ms);
    notes
}

fn append_line(path: &Path, note: &LedgerNote) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut line = serde_json::to_string(note).map_err(std::io::Error::other)?;
    line.push('\n');
    let mut file = open_append(path)?;
    file.write_all(line.as_bytes())?;
    // 不 fsync：账本是可丢的观测数据，为它付一次落盘屏障不值得（同 effect 状态文件
    // 用 save_timed_light 的理由）。append 写完即可被同进程的读取方看到。
    file.flush()
}

fn open_append(path: &Path) -> std::io::Result<fs::File> {
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // 0600：账本里有会话/工作区 id 与版本号，不是公开数据。**创建时**就定死权限，
        // 不做事后 chmod（与 atomic_write_private 同一条纪律：事后再补总有一个窗口）。
        options.mode(0o600);
    }
    options.open(path)
}

/// 删掉窗口起点之前的**整月**文件。窗口内的日行由 [\`read_in_dir\`] 过滤。
///
/// 按整月删而不是按行重写：账本是追加流水，重写会引入「删一半」的中间态；而多留
/// 一个月的行只是读侧多过滤几行，代价远小于把在窗口内的行误删。
fn prune_old_months(ledger_dir: &Path, now_ms: u64) {
    let cutoff = utc_month_of_ms(now_ms.saturating_sub(RETENTION_DAYS * DAY_MS));
    let Ok(entries) = fs::read_dir(ledger_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        let Some(month) = month_of_file_name(&name) else {
            continue;
        };
        if month < cutoff.as_str() {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// 从账本文件名里取 \`YYYY-MM\`；不是账本文件就返回 \`None\`（别删用户别的文件）。
fn month_of_file_name(name: &str) -> Option<&str> {
    let month = name.strip_prefix(FILE_PREFIX)?.strip_suffix(FILE_SUFFIX)?;
    is_month_string(month).then_some(month)
}

fn is_month_string(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 7
        && bytes[4] == b'-'
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[5..].iter().all(u8::is_ascii_digit)
}

/// 毫秒时间戳 -> **UTC** 月份 \`YYYY-MM\`。取不到（纪元之前）退 \`1970-01\`。
fn utc_month_of_ms(ms: u64) -> String {
    let seconds = (ms / 1000) as i64;
    match time::OffsetDateTime::from_unix_timestamp(seconds) {
        Ok(datetime) => format!("{:04}-{:02}", datetime.year(), u8::from(datetime.month())),
        Err(_) => String::from("1970-01"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    const BASE_MS: u64 = 1_790_000_000_000;
    const DAY: u64 = 86_400_000;

    /// 与 \`effect::tests::scratch\` 同形：唯一目录，用例自己清。
    fn scratch(label: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let serial = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "memory-ledger-{label}-{}-{serial}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("建临时目录");
        dir
    }

    fn note(at_ms: u64, kind: &str, reason: &str, ids: Vec<&str>) -> LedgerNote {
        LedgerNote {
            at_ms,
            session: "session-00a67abf".to_string(),
            turn: 25,
            step: -1,
            kind: kind.to_string(),
            reason: reason.to_string(),
            enabled: true,
            ids: ids.into_iter().map(str::to_string).collect(),
            bytes: 758,
            generation: 34,
            workspace_id: "ws-1".to_string(),
            plugin_version: "0.2.0".to_string(),
            kernel_version: "0.2.2-alpha.2".to_string(),
        }
    }

    /// 落盘再读回：字段一个不丢，窗口按本地日历日、结果按 at_ms 升序。
    #[test]
    fn append_then_read_round_trips_fields_in_time_order() {
        let dir = scratch("round-trip");
        let today = date_string_of_ms(BASE_MS);
        let yesterday = date_string_of_ms(BASE_MS - DAY);
        let later = note(BASE_MS, KIND_INJECT, "turn", vec!["a".repeat(32).as_str()]);
        let earlier = note(BASE_MS - DAY, KIND_SKIP, "budget", Vec::new());
        // 先写晚的、再写早的：读取必须按 at_ms 升序，不按写入顺序。
        append(&dir, &later);
        append(&dir, &earlier);

        let notes = read_in_dir(&dir, &yesterday, &today);
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[0].at_ms, BASE_MS - DAY, "按 at_ms 升序");
        assert_eq!(notes[1].at_ms, BASE_MS);
        assert_eq!(notes[1], later, "字段必须原样回来");
        assert_eq!(notes[0], earlier);
        assert!(notes[1].is_inject());
        assert!(!notes[0].is_inject());
    }

    /// 坏行只丢那一行；窗口外的行按本地日历日过滤（含边界两端）。
    #[test]
    fn bad_lines_are_skipped_and_window_filters_by_local_day() {
        let dir = scratch("bad-lines");
        let today = date_string_of_ms(BASE_MS);
        let path = file_for(&dir, BASE_MS);
        fs::create_dir_all(&dir).expect("建账本目录");
        let inside = note(BASE_MS, KIND_INJECT, "turn", vec!["b".repeat(32).as_str()]);
        let outside = note(BASE_MS - 10 * DAY, KIND_INJECT, "turn", Vec::new());
        let mut content = String::new();
        content.push_str("{这不是 JSON\n");
        content.push_str(&serde_json::to_string(&inside).expect("序列化"));
        content.push('\n');
        content.push_str("{\"at_ms\": 123, 少半边\n");
        content.push_str(&serde_json::to_string(&outside).expect("序列化"));
        content.push('\n');
        content.push('\n');
        fs::write(&path, content).expect("写坏行夹具");

        let notes = read_in_dir(&dir, &today, &today);
        assert_eq!(notes.len(), 1, "坏行与窗口外都要跳过");
        assert_eq!(notes[0].session, "session-00a67abf");
        // 窗口包含 10 天前：outside 仍在保留期内（90 天），只是不在 7 天窗口里。
        let wide = read_in_dir(&dir, &date_string_of_ms(BASE_MS - 30 * DAY), &today);
        assert_eq!(wide.len(), 2);
    }

    /// 90 天保留：追加成功后删掉窗口起点之前的整月文件；窗口内的月份不动。
    #[test]
    fn old_month_files_are_pruned_after_append() {
        let dir = scratch("retention");
        fs::create_dir_all(&dir).expect("建账本目录");
        let old = dir.join("notes-2024-01.jsonl");
        let recent = dir.join(format!(
            "notes-{}.jsonl",
            utc_month_of_ms(BASE_MS - 10 * DAY)
        ));
        fs::write(&old, "").expect("写旧月份");
        fs::write(&recent, "").expect("写近月份");
        // 一个不匹配命名规则的文件必须留下：清理只认 notes-YYYY-MM.jsonl。
        let other = dir.join("state.json");
        fs::write(&other, "{}").expect("写别的文件");

        append(&dir, &note(BASE_MS, KIND_INJECT, "turn", Vec::new()));
        assert!(!old.exists(), "90 天前的月份文件要删掉");
        assert!(recent.exists(), "窗口内的月份文件要留下");
        assert!(other.exists(), "不匹配账本命名的文件不许动");
        assert!(file_for(&dir, BASE_MS).exists(), "本次追加的文件要在");
    }

    /// 隐私红线：一行里只有 id/字节/计数/会话坐标/版本，没有标题或正文。
    #[test]
    fn a_line_never_carries_titles_or_bodies() {
        let dir = scratch("privacy");
        append(
            &dir,
            &note(BASE_MS, KIND_INJECT, "turn", vec!["c".repeat(32).as_str()]),
        );
        let path = file_for(&dir, BASE_MS);
        let line = fs::read_to_string(&path).expect("读账本行");
        let value: serde_json::Value = serde_json::from_str(line.trim()).expect("一行是 JSON");
        let object = value.as_object().expect("一行是一个对象");
        let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "at_ms",
                "bytes",
                "enabled",
                "generation",
                "ids",
                "kernel_version",
                "kind",
                "plugin_version",
                "reason",
                "session",
                "step",
                "turn",
                "workspace_id",
            ],
            "账本行的字段集就是合同 §1.2 那一份；多一个 title/body 就是隐私回退"
        );
        for forbidden in ["title", "body", "text", "content", "query"] {
            assert!(
                !line.contains(&format!("\"{forbidden}\"")),
                "账本行不许出现 {forbidden}"
            );
        }
    }

    /// ids 归一：去空白、丢空串、封顶 [MAX_IDS]。
    #[test]
    fn ids_are_trimmed_deduplicated_by_shape_and_capped() {
        let many: Vec<String> = (0..MAX_IDS + 5)
            .map(|index| format!("{index:032x}"))
            .collect();
        let raw = many.join(",");
        let ids = normalize_ids(&raw);
        assert_eq!(ids.len(), MAX_IDS, "最多记 MAX_IDS 个");
        assert_eq!(ids[0], many[0]);
        assert_eq!(normalize_ids(" a ,, b ,"), vec!["a", "b"]);
        assert!(normalize_ids("").is_empty());
    }

    /// 目录不存在、或整个路径是个文件时：读取返回空表，追加不 panic（只留一行壳日志）。
    #[test]
    fn missing_or_unwritable_ledger_is_not_fatal() {
        let dir = scratch("unwritable");
        assert!(read_in_dir(&dir.join("nope"), "2026-01-01", "2026-12-31").is_empty());

        // 追加失败会走 shell_events，落盘路径必须指到临时 home。
        let home = scratch("unwritable-home");
        let _scoped_home = crate::tests::scoped_xlink_home(&home);
        let as_file = dir.join("not-a-dir");
        fs::write(&as_file, "x").expect("写占位文件");
        append(&as_file, &note(BASE_MS, KIND_SKIP, "disabled", Vec::new()));
        assert!(read_in_dir(&as_file, "2026-01-01", "2026-12-31").is_empty());
    }

    /// 账本文件 0600（unix）：里面有会话/工作区 id 与版本号，不是公开数据。
    #[cfg(unix)]
    #[test]
    fn ledger_files_are_private_to_the_user() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("perms");
        append(&dir, &note(BASE_MS, KIND_INJECT, "turn", Vec::new()));
        let mode = fs::metadata(file_for(&dir, BASE_MS))
            .expect("账本文件要存在")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "账本文件权限必须是 0600");
    }

    /// 目录布局：账本挂在 effect 目录下的 ledger/，文件名按 UTC 月份。
    #[test]
    fn layout_is_effect_ledger_notes_by_utc_month() {
        let effect = PathBuf::from("/tmp/effect");
        assert_eq!(dir_under(&effect), PathBuf::from("/tmp/effect/ledger"));
        assert_eq!(
            file_for(&dir_under(&effect), BASE_MS).file_name().unwrap(),
            "notes-2026-09.jsonl"
        );
        assert_eq!(utc_month_of_ms(0), "1970-01");
        assert_eq!(month_of_file_name("notes-2026-10.jsonl"), Some("2026-10"));
        assert_eq!(month_of_file_name("notes-2026-1.jsonl"), None);
        assert_eq!(month_of_file_name("state.json"), None);
    }
}
