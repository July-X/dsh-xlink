//! 本地日历时间的换算与格式化。
//!
//! ## 为什么单独一个模块
//!
//! 全仓有**三处**各自算「本地时间」：`process::local_date_string`（日志文件名
//! 的日期戳）、`shell_events::clock`（事件日志的时刻）、以及本轮新增的
//! `local_hms_string`（运行记录 id 的时刻）。三份各写一遍的后果不是"多了几行"
//! ——而是**三处会各走各的时区**：`OffsetDateTime::now_local()` 与
//! `to_offset(current_local_offset())` 在时区信息不可用时的回退不同，日期
//! 走一份、时刻走另一份时，跨零点的记录会显示成"昨天的日期配今天的时间"。
//! 2026-10-06 审查（docs/runtime-diagnostics-review-2026-10-06.md P2-04）抓到
//! 的就是运行记录这一处；根因是同一件事写了两遍。
//!
//! 也因为 `process.rs` 在代码预算的**反棘轮**上（只许越来越小），把本地时间
//! 从「一个进程的杂项」里抬出来是唯一能加东西而不违反那条规则的地方。

use std::time::{SystemTime, UNIX_EPOCH};

use time::format_description::FormatItem;
use time::macros::format_description;
use time::{OffsetDateTime, UtcOffset};

/// 本地 `YYYY-MM-DD`。
///
/// `time` crate 的默认 features 包含 `local-offset`，转换使用用户所在时区——
/// UTC 日期会在用户感知的本地时间的不同时刻翻转日志，把同一个用户日拆到
/// 两个文件里。
pub(crate) fn local_date_string(time: SystemTime) -> String {
    match local(time, DATE_FORMAT) {
        Some(text) => text,
        None => String::from("1970-01-01"),
    }
}

/// 今天的本地日期（`local_date_string` 的即时版本）。
pub fn current_date_string() -> String {
    local_date_string(SystemTime::now())
}

/// 本地 `HH:MM:SS`（带冒号）。事件日志与任何给人看的时间戳用这个。
pub(crate) fn local_clock_string(time: SystemTime) -> String {
    match local(time, CLOCK_FORMAT) {
        Some(text) => text,
        None => String::from("--:--:--"),
    }
}

/// 本地 `HHMMSS`（不带冒号）。运行记录 id 的时刻段用这个。
///
/// 与 [`local_date_string`] 用**同一个**偏移换算——日期按本地走而时刻按 UTC
/// 走，runId 里就会出现「昨天的日期配明天的时间」（审查 P2-04）。
pub(crate) fn local_hms_string(time: SystemTime) -> String {
    match local(time, HMS_FORMAT) {
        Some(text) => text,
        None => String::from("000000"),
    }
}

const DATE_FORMAT: &[FormatItem<'static>] = format_description!("[year]-[month]-[day]");
const CLOCK_FORMAT: &[FormatItem<'static>] = format_description!("[hour]:[minute]:[second]");
const HMS_FORMAT: &[FormatItem<'static>] = format_description!("[hour][minute][second]");

/// 换算到本地时区后按 `format` 渲染。取不到时区就用 UTC——**不编一个假的
/// 本地时间**：显示成 UTC 至少是确定的，编出来的时间会让日志对齐变得没意义。
fn local(time: SystemTime, format: &[FormatItem<'static>]) -> Option<String> {
    let duration = time.duration_since(UNIX_EPOCH).ok()?;
    let datetime = OffsetDateTime::from_unix_timestamp(duration.as_secs() as i64).ok()?;
    let offset = UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC);
    datetime.to_offset(offset).format(format).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// 三个格式必须来自**同一次**换算：同一时刻下，日期与时刻对得上。
    ///
    /// 断言的是自洽而非某个时区的具体值——后者会让测试在 CI 换时区后假红。
    #[test]
    fn date_and_clock_come_from_the_same_conversion() {
        let at = SystemTime::now();
        let date = local_date_string(at);
        let clock = local_clock_string(at);
        assert_eq!(date.len(), 10, "{date}");
        assert_eq!(clock.len(), 8, "{clock}");
        let hour: u32 = clock[..2].parse().expect("小时");
        assert!(hour < 24, "{clock}");
        let minute: u32 = clock[3..5].parse().expect("分钟");
        assert!(minute < 60, "{clock}");
        assert_eq!(local_hms_string(at).len(), 6, "HHMMSS 不带冒号");
    }

    /// Unix 纪元之前取不到时间——回退值必须能被下游解析，而不是抛。
    #[test]
    fn a_clock_before_the_epoch_falls_back_instead_of_panicking() {
        let before = UNIX_EPOCH - Duration::from_secs(1);
        assert_eq!(local_date_string(before), "1970-01-01");
        assert_eq!(local_clock_string(before), "--:--:--");
        assert_eq!(local_hms_string(before), "000000");
    }
}
