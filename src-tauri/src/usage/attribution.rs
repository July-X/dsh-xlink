//! 日归属口径：一条记录算到哪个本地日；口径变了以后怎么把历史桶算对。
//!
//! 两份账目（内核自己的那份 [`super::local`]、外部 agent 工具那份
//! [`super::external`]）都把原始记录按**本地日历日**分桶，而归到哪一天取
//! 决于「那一刻用哪一版时区规则换算」。规则一改，同一批记录会落到不同的日
//! 子——已经落盘的桶因此停在旧口径上。
//!
//! 这里放三样东西：
//!
//! 1. **口径版本**（[`ATTRIBUTION_VERSION`]）：桶必须记着自己是被哪一版算出
//!    来的，否则没人能判断哪些桶要重算。
//! 2. **受影响的那几天**（[`attribution_shift`]）：判据只看两版口径的**本地
//!    午夜**挪没挪——挪了，界线两侧的记录换了日子；没挪，一个字节都不必重算。
//! 3. **旧口径的偏移**（`shell::localtime::legacy_offset_at`）：算「挪没挪」
//!    的前提是能算出旧口径的结果。
//!
//! 判据注入合成规则（[`OffsetOf`]），因此不依赖本机时区。

use std::collections::BTreeSet;

use time::macros::format_description;
use time::{Date, OffsetDateTime, UtcOffset};

/// 日归属口径 v1：套用**当前生效**的时区规则。
///
/// 这是 2026-11-21 之前的行为（`time` 在 Windows 上走
/// `SystemTimeToTzSpecificLocalTime(NULL, …)`，按当前规则换算任何时刻）。
pub(crate) const ATTRIBUTION_LEGACY: u32 = 1;

/// 日归属口径 v2：按记录发生那一刻**所在年份**的规则（d8b44356 之后）。
pub(crate) const ATTRIBUTION_OWN_YEAR: u32 = 2;

/// 当前生效的口径版本。
pub(crate) const ATTRIBUTION_VERSION: u32 = ATTRIBUTION_OWN_YEAR;

/// 桶是按哪一版口径算出来的（0 = 旧状态文件没记这个字段 ⇒ 按 v1 处理）。
///
/// 状态里的桶是**派生聚合**：它是「按某一版偏移规则把原始记录分到日子里」
/// 的结果。规则一改，同一批记录会落到不同的日子——没有这个标记，状态就
/// 分不清「这些天是旧口径算的」与「这些天是新口径算的」，口径一变就只能
/// 整份重扫或者等用户手工清缓存。
pub(crate) fn stored_attribution(value: u32) -> u32 {
    match value {
        0 => ATTRIBUTION_LEGACY,
        other => other,
    }
}

/// 取某一版口径下某一刻的本地偏移。
///
/// 判据注入合成规则（不依赖本机时区），生产走 [`platform_offset`]。
pub(crate) type OffsetOf = fn(u32, OffsetDateTime) -> UtcOffset;

/// 生产用的取法：v1 走系统那套「当前规则套所有时刻」，v2 走逐年规则。
pub(crate) fn platform_offset(version: u32, utc: OffsetDateTime) -> UtcOffset {
    match version {
        ATTRIBUTION_LEGACY => crate::shell::localtime::legacy_offset_at(utc),
        _ => crate::shell::localtime::local_offset_at(utc),
    }
}

/// 某一版口径下，某一刻（epoch 毫秒）归属到哪个本地日。
pub(crate) fn day_under(version: u32, ts_ms: u64, offset_of: OffsetOf) -> String {
    let utc = OffsetDateTime::from_unix_timestamp_nanos(i128::from(ts_ms) * 1_000_000)
        .unwrap_or(OffsetDateTime::UNIX_EPOCH);
    utc.to_offset(offset_of(version, utc))
        .date()
        .format(&format_description!("[year]-[month]-[day]"))
        .unwrap_or_default()
}

/// 某个本地日在某一版口径下的**本地午夜**对应的 UTC 时刻（秒）。
///
/// 「午夜落在哪一刻」是归属会不会变的唯一判据：两版口径的午夜差了几秒，
/// 两个午夜之间的那些记录就换了日子；午夜没动，那天的归属没变，不必重算。
fn local_midnight_instant(version: u32, day: Date, offset_of: OffsetOf) -> Option<i64> {
    let midnight_utc = day
        .midnight()
        .assume_offset(UtcOffset::UTC)
        .unix_timestamp();
    // 本地午夜 = 该日 00:00 按偏移回退到 UTC。偏移随夏令时变，迭代几次收敛。
    let mut guess = midnight_utc;
    for _ in 0..4 {
        let at = OffsetDateTime::from_unix_timestamp(guess).ok()?;
        guess = midnight_utc - i64::from(offset_of(version, at).whole_seconds());
    }
    let at = OffsetDateTime::from_unix_timestamp(guess).ok()?;
    (at.to_offset(offset_of(version, at)).date() == day).then_some(guess)
}

/// 某个本地日在两版口径下的午夜时刻（秒）：(最早, 最晚)。
fn midnight_pair(from: u32, to: u32, day: Date, offset_of: OffsetOf) -> Option<(i64, i64)> {
    match (
        local_midnight_instant(from, day, offset_of),
        local_midnight_instant(to, day, offset_of),
    ) {
        (Some(a), Some(b)) => Some((a.min(b), a.max(b))),
        (Some(a), None) => Some((a, a)),
        (None, Some(b)) => Some((b, b)),
        (None, None) => None,
    }
}

/// 口径变了之后要重算的时段。
#[derive(Debug, Default)]
pub(crate) struct AttributionShift {
    /// 归属会变的那些本地日（两版口径下都列出来）。
    pub days: Vec<String>,
    /// 要回来源重读的 UTC 区间（epoch 毫秒，闭开；相邻的已合并）。
    pub spans: Vec<(i64, i64)>,
}

/// 两版口径之间，窗口内归属会变的那几天 + 要回读哪些时段。
///
/// **只看本地午夜挪没挪**：午夜挪了，界线两侧的记录就换了日子；没挪，
/// 那天的归属没变，一个字节都不必重算。
pub(crate) fn attribution_shift(
    from: u32,
    to: u32,
    first: &str,
    last: &str,
    offset_of: OffsetOf,
) -> AttributionShift {
    let (Some(first), Some(last)) = (parse_day(first), parse_day(last)) else {
        // 日期都解析不出来 ⇒ 没有可信的窗口，也就没什么可重算的。
        return AttributionShift::default();
    };
    let mut days: BTreeSet<String> = BTreeSet::new();
    let mut spans: Vec<(i64, i64)> = Vec::new();
    let mut day = first;
    loop {
        let moved = match (
            local_midnight_instant(from, day, offset_of),
            local_midnight_instant(to, day, offset_of),
        ) {
            (Some(legacy), Some(current)) => legacy != current,
            _ => false,
        };
        if moved {
            // 午夜挪了 → 这一天和它的前一天都受影响（界线两侧换日子）。
            let prev = day.previous_day();
            let next = day.next_day();
            for affected in [prev, Some(day)].into_iter().flatten() {
                days.insert(day_string(affected));
            }
            // 回读区间：把这两天在两版口径下的区间并起来。
            let lo = [prev, Some(day)]
                .into_iter()
                .flatten()
                .filter_map(|d| midnight_pair(from, to, d, offset_of).map(|(a, _)| a))
                .min();
            let hi = [Some(day), next]
                .into_iter()
                .flatten()
                .filter_map(|d| midnight_pair(from, to, d, offset_of).map(|(_, b)| b))
                .max();
            if let (Some(lo), Some(hi)) = (lo, hi) {
                spans.push((lo * 1000, hi * 1000));
            }
        }
        if day >= last {
            break;
        }
        match day.next_day() {
            Some(next) => day = next,
            None => break,
        }
    }
    // 相邻的区间合并成一段：连续的受影响时段只回读一次。
    spans.sort_unstable();
    let mut merged: Vec<(i64, i64)> = Vec::new();
    for (lo, hi) in spans {
        match merged.last_mut() {
            Some(last) if lo <= last.1 => last.1 = last.1.max(hi),
            _ => merged.push((lo, hi)),
        }
    }
    AttributionShift {
        days: days.into_iter().collect(),
        spans: merged,
    }
}

/// `YYYY-MM-DD` → 日期。
fn parse_day(text: &str) -> Option<Date> {
    Date::parse(text, &format_description!("[year]-[month]-[day]")).ok()
}

/// 日期 → `YYYY-MM-DD`。
fn day_string(day: Date) -> String {
    day.format(&format_description!("[year]-[month]-[day]"))
        .unwrap_or_default()
}

/// 合成一套偏移规则（判据用）：**2024 年用 -4h，其余年份用 -5h**。
///
/// 这正是「同一时区在某个年份改过基准差」的形状：旧口径（套当前生效的规则）
/// 把 2024 年的每一天都按 -5h 算，新口径（按记录当年的规则）按 -4h 算。
///
/// 判据不依赖本机时区——本机是 UTC 的话两版口径根本一样，测不出「哪几天
/// 变了」这件事。
#[cfg(test)]
pub(crate) fn shifted_zone_offset(version: u32, utc: OffsetDateTime) -> UtcOffset {
    let own_year = if utc.year() == 2024 {
        -4 * 3600
    } else {
        -5 * 3600
    };
    match version {
        ATTRIBUTION_LEGACY => UtcOffset::from_whole_seconds(-5 * 3600).unwrap_or(UtcOffset::UTC),
        _ => UtcOffset::from_whole_seconds(own_year).unwrap_or(UtcOffset::UTC),
    }
}

/// 合成一套偏移规则（判据用）：**只有 2024-06-15 那一天的偏移从 -5h 变成 -4h**。
///
/// 与 [`shifted_zone_offset`]（整年改基准差）互补：这一套只让**一天**的归属
/// 变掉，正好用来钉「只重算受影响的那几天，没受影响的文件一个字节都不动」。
#[cfg(test)]
pub(crate) fn one_day_shifted_offset(version: u32, utc: OffsetDateTime) -> UtcOffset {
    let shifted = utc.year() == 2024 && utc.month() == time::Month::June && utc.day() == 15;
    let seconds = match version {
        ATTRIBUTION_LEGACY => -5 * 3600,
        _ if shifted => -4 * 3600,
        _ => -5 * 3600,
    };
    UtcOffset::from_whole_seconds(seconds).unwrap_or(UtcOffset::UTC)
}
