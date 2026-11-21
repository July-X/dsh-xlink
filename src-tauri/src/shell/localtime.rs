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
//! 2026-10-06 审查（docs/features/diagnostics/runtime-diagnostics-review-2026-10-06.md P2-04）抓到
//! 的就是运行记录这一处；根因是同一件事写了两遍。
//!
//! 也因为 `process.rs` 在代码预算的**反棘轮**上（只许越来越小），把本地时间
//! 从「一个进程的杂项」里抬出来是唯一能加东西而不违反那条规则的地方。
//!
//! ## 本地偏移：Windows 与 Unix 不是一回事
//!
//! `time` 0.3.55 在 Unix 上走 `localtime_r`（tzdata），拿到的是**逐年历史
//! 规则**；在 Windows 上走 `SystemTimeToTzSpecificLocalTime(nullptr, …)`，
//! 按**当前生效**的时区规则换算——它不是历史表。后果是跨年时间戳被归到上一年、
//! 夏令时切换错位那一周差一小时（2026-11-21 代码 review 查实：MSDN 明写该
//! 函数在这两种情况下**可能算错**）。
//!
//! 因此 Windows 侧自己算：走 `GetTimeZoneInformationForYear`，它把「当前规则
//! 形状 + 历年偏移变更」按**给定年份**折算成一份 `TIME_ZONE_INFORMATION`。
//! 规则换算本身是纯函数（[`ZoneRules`]），判据不依赖本机时区——任何平台都跑，
//! Windows 上因此也有护栏（此前的用例挂着 `#[cfg(unix)]`，Windows 侧零覆盖）。

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
    let offset = local_offset_at(datetime);
    datetime.to_offset(offset).format(format).ok()
}

/// 某一刻的本地 UTC 偏移。
///
/// **Windows 上必须自己按年算**：`time` 的 Windows 实现按当前生效的规则
/// 换算（见模块文档），跨年会算错。Windows 侧先走本模块的逐年规则
/// （[`offset_at_with`]），取不到才退回 `time` 的实现——退回即退回成旧行为，
/// 不会比升级前更差。
pub(crate) fn local_offset_at(utc: OffsetDateTime) -> UtcOffset {
    #[cfg(windows)]
    let per_year = windows_offset_at(utc);
    #[cfg(not(windows))]
    let per_year: Option<UtcOffset> = None;

    per_year
        .or_else(|| UtcOffset::local_offset_at(utc).ok())
        .unwrap_or(UtcOffset::UTC)
}

/// 某一刻的本地 UTC 偏移——**旧口径**（套用当前生效的规则）。
///
/// 这是 2026-11-21 之前的行为，也是**外部用量缓存里那批历史桶的算法**：
/// Windows 上 `time` 走 `SystemTimeToTzSpecificLocalTime(NULL, …)`，按**当前
/// 生效**的规则换算任何时刻（跨年会算错）；Unix 上 `time` 走 tzdata，本来
/// 就是逐年的，与 [`local_offset_at`] 同源。
///
/// 留着它不是为了继续用它算账，而是为了**能算出「哪几天的归属变了」**：
/// 缓存里的桶记着「这些天是按旧口径算的」，没有旧口径就没人能判断它们
/// 要不要重算（见 `usage::external` 的口径版本机制）。
#[cfg(windows)]
pub(crate) fn legacy_offset_at(utc: OffsetDateTime) -> UtcOffset {
    use windows_sys::Win32::Foundation::SYSTEMTIME;
    use windows_sys::Win32::System::Time::SystemTimeToTzSpecificLocalTime;

    let utc = utc.to_offset(UtcOffset::UTC);
    let utc_st = SYSTEMTIME {
        wYear: utc.year() as u16,
        wMonth: utc.month() as u16,
        wDay: utc.day() as u16,
        wDayOfWeek: 0, // 系统不读这个字段
        wHour: utc.hour() as u16,
        wMinute: utc.minute() as u16,
        wSecond: utc.second() as u16,
        wMilliseconds: utc.millisecond(),
    };
    let mut local_st: SYSTEMTIME = unsafe { std::mem::zeroed() };
    // Safety: 两个 SYSTEMTIME 都是我们自己填的；系统 API 只写 local_st。
    let ok = unsafe { SystemTimeToTzSpecificLocalTime(std::ptr::null(), &utc_st, &mut local_st) };
    if ok == 0 {
        return UtcOffset::UTC;
    }
    match systemtime_epoch_secs(local_st) {
        // 墙钟减 UTC 就是偏移（旧口径算出来的那个偏移）。
        Some(local) => {
            UtcOffset::from_whole_seconds(i32::try_from(local - utc.unix_timestamp()).unwrap_or(0))
                .unwrap_or(UtcOffset::UTC)
        }
        None => UtcOffset::UTC,
    }
}

/// 系统给的**墙钟**（SYSTEMTIME）→ epoch 秒。
#[cfg(windows)]
fn systemtime_epoch_secs(st: windows_sys::Win32::Foundation::SYSTEMTIME) -> Option<i64> {
    let date = time::Date::from_calendar_date(
        st.wYear as i32,
        time::Month::try_from(st.wMonth as u8).ok()?,
        st.wDay as u8,
    )
    .ok()?;
    let clock = time::Time::from_hms(st.wHour as u8, st.wMinute as u8, st.wSecond as u8).ok()?;
    Some(
        date.with_time(clock)
            .assume_offset(UtcOffset::UTC)
            .unix_timestamp(),
    )
}

/// Unix 上旧口径与新口径同源（tzdata 本来就是逐年的），因此直接用现行实现。
#[cfg(not(windows))]
pub(crate) fn legacy_offset_at(utc: OffsetDateTime) -> UtcOffset {
    local_offset_at(utc)
}

// ─── 时区规则（纯函数，判据在任何平台都跑）──────────────────────────────

/// 一年的时区规则：换算真正用得到的那几个字段。
///
/// 独立成自己的类型而不是直接用 Win32 的 `TIME_ZONE_INFORMATION`：判据要
/// 拿**合成规则**在任意平台上验「历史时刻用当年的规则」，因此这里不依赖
/// 任何平台类型。
#[cfg(any(windows, test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ZoneRules {
    /// 与 UTC 的基准差（分钟）。本地 = UTC - (bias + 当期 bias)。
    bias: i32,
    /// 标准时相对基准的额外差（分钟）。
    standard_bias: i32,
    /// 夏令时相对基准的额外差（分钟）。
    daylight_bias: i32,
    /// 转回标准时的规则；`month == 0` 表示不转回标准时。
    standard: TransitionRule,
    /// 转入夏令时的规则；`month == 0` 表示全年没有夏令时。
    daylight: TransitionRule,
}

/// 一次切换的规则（Win32 `SYSTEMTIME` 里的日期字段）。
#[cfg(any(windows, test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TransitionRule {
    /// 0 = 不切换；1..=12 = 月份。
    month: u8,
    /// 星期几，0 = 周日（与 `SYSTEMTIME` 一致）。
    day_of_week: u8,
    /// 月内第几个该星期几：1..=5 = 第 n 个，6 = 最后一个。
    day: u8,
    hour: u8,
    minute: u8,
    second: u8,
}

#[cfg(any(windows, test))]
impl TransitionRule {
    /// 从 Win32 的 `SYSTEMTIME` 取日期与时刻字段。
    #[cfg(windows)]
    fn from_systemtime(st: &windows_sys::Win32::Foundation::SYSTEMTIME) -> Self {
        Self {
            month: st.wMonth as u8,
            day_of_week: st.wDayOfWeek as u8,
            day: st.wDay as u8,
            hour: st.wHour as u8,
            minute: st.wMinute as u8,
            second: st.wSecond as u8,
        }
    }
}

/// 取某个时刻的本地偏移：逐年试规则，并用「换算回去的本地年」校验。
///
/// **为什么要有校验**：本地年可能与 UTC 年不同（跨年那几天）。用哪一年的
/// 规则，就得换算出那一年的本地日期——否则 1 月初的记录会被套上 12 月的
/// 规则（偏移可能不同）。
///
/// `rules_of` 按给定年份给出那一年的规则：Windows 上是
/// `GetTimeZoneInformationForYear`，判据里给的是合成规则。
#[cfg(any(windows, test))]
fn offset_at_with<F>(utc: OffsetDateTime, rules_of: F) -> Option<UtcOffset>
where
    F: Fn(i32) -> Option<ZoneRules>,
{
    for year in [utc.year(), utc.year() - 1, utc.year() + 1] {
        let Some(rules) = rules_of(year) else {
            continue;
        };
        let Ok(candidate) = UtcOffset::from_whole_seconds(offset_seconds_at(&rules, utc)) else {
            continue;
        };
        if utc.to_offset(candidate).year() == year {
            return Some(candidate);
        }
    }
    None
}

/// 按这一年的规则算某个 UTC 时刻的本地偏移（秒）。
#[cfg(any(windows, test))]
fn offset_seconds_at(rules: &ZoneRules, utc: OffsetDateTime) -> i32 {
    let standard_offset = -(rules.bias + rules.standard_bias) * 60;
    let daylight_offset = -(rules.bias + rules.daylight_bias) * 60;
    let year = utc.year();
    let daylight_start = transition_instant(&rules.daylight, year, standard_offset);
    let standard_start = transition_instant(&rules.standard, year, daylight_offset);
    let daylight = match (daylight_start, standard_start) {
        // 全年没有夏令时。
        (None, _) => false,
        // 不转回标准时：切换之后都是夏令时（切换之前仍是标准时）。
        (Some(start), None) => utc >= start,
        // 两次切换都在本年：夏令时是中间那一段。
        (Some(start), Some(end)) if start <= end => utc >= start && utc < end,
        // 夏令时跨年（南半球形态）：切换之后、回到标准时之前。
        (Some(start), Some(end)) => utc >= start || utc < end,
    };
    if daylight {
        daylight_offset
    } else {
        standard_offset
    }
}

/// 一次切换发生的 UTC 时刻。
///
/// 切换时刻是**墙钟**（本地时间），并按**切换前**的偏移解释：转入夏令时的
/// 那一刻墙钟还是标准时，转回标准时的那一刻墙钟还是夏令时（Win32 的
/// `TIME_ZONE_INFORMATION` 就是这个约定）。
#[cfg(any(windows, test))]
fn transition_instant(
    rule: &TransitionRule,
    year: i32,
    offset_before_seconds: i32,
) -> Option<OffsetDateTime> {
    let month = time::Month::try_from(rule.month).ok()?;
    let date = transition_date(year, month, rule)?;
    let clock = time::Time::from_hms(rule.hour, rule.minute, rule.second).ok()?;
    let wall = OffsetDateTime::new_utc(date, clock);
    Some(wall - time::Duration::seconds(offset_before_seconds as i64))
}

/// 规则指向的那一天（按「月内第几个星期几」或「最后一个星期几」）。
#[cfg(any(windows, test))]
fn transition_date(year: i32, month: time::Month, rule: &TransitionRule) -> Option<time::Date> {
    let first = time::Date::from_calendar_date(year, month, 1).ok()?;
    match rule.day {
        1..=5 => {
            let delta = (rule.day_of_week + 7 - first.weekday().number_days_from_sunday()) % 7;
            let offset = (rule.day as i64 - 1) * 7 + delta as i64;
            Some(first + time::Duration::days(offset))
        }
        6 => {
            let last = time::Date::from_calendar_date(year, month, month.length(year)).ok()?;
            let back = (last.weekday().number_days_from_sunday() + 7 - rule.day_of_week) % 7;
            Some(last - time::Duration::days(back as i64))
        }
        _ => None,
    }
}

// ─── Windows：按年取规则 ───────────────────────────────────────────────

/// Windows 上按**指定年份**取当年的时区规则与偏移。
///
/// `GetTimeZoneInformationForYear` 是这里的关键：它把动态时区信息（「当前
/// 规则形状 + 历年偏移变更」）按给定年份折算成一份 `TIME_ZONE_INFORMATION`,
/// 因此历史时刻落在**当年**的偏移上，而不是当前生效的那一套。
#[cfg(windows)]
fn zone_rules_for_year(year: i32) -> Option<ZoneRules> {
    use windows_sys::Win32::System::Time::{
        GetDynamicTimeZoneInformation, GetTimeZoneInformationForYear,
        DYNAMIC_TIME_ZONE_INFORMATION, TIME_ZONE_INFORMATION,
    };
    // Safety: 两个调用都只往我们给的定长缓冲区里写；两个结构体都是零初始化的，
    // 读不到未初始化字节。
    let mut dynamic: DYNAMIC_TIME_ZONE_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe { GetDynamicTimeZoneInformation(&mut dynamic) } == 0 {
        return None;
    }
    let mut info: TIME_ZONE_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe { GetTimeZoneInformationForYear(year as u16, &dynamic, &mut info) } == 0 {
        return None;
    }
    Some(ZoneRules {
        bias: info.Bias,
        standard_bias: info.StandardBias,
        daylight_bias: info.DaylightBias,
        standard: TransitionRule::from_systemtime(&info.StandardDate),
        daylight: TransitionRule::from_systemtime(&info.DaylightDate),
    })
}

/// Windows 上的本地偏移：按年取规则（见 [`zone_rules_for_year`]）。
#[cfg(windows)]
fn windows_offset_at(utc: OffsetDateTime) -> Option<UtcOffset> {
    offset_at_with(utc, zone_rules_for_year)
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

    #[cfg(unix)]
    #[test]
    fn historical_timestamps_use_their_own_dst_offset() {
        const CHILD: &str = "DSH_TEST_LOCALTIME_TZ";
        if std::env::var_os(CHILD).is_none() {
            let output = crate::shell::process::command_with_path(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "shell::localtime::tests::historical_timestamps_use_their_own_dst_offset",
                ])
                .env(CHILD, "1")
                .env("TZ", "America/New_York")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        for (iso, date, clock) in [
            ("2026-01-15T04:30:00Z", "2026-01-14", "23:30:00"),
            ("2026-07-15T04:30:00Z", "2026-07-15", "00:30:00"),
        ] {
            let at =
                OffsetDateTime::parse(iso, &time::format_description::well_known::Rfc3339).unwrap();
            let time = UNIX_EPOCH + Duration::from_secs(at.unix_timestamp() as u64);
            assert_eq!(local_date_string(time), date);
            assert_eq!(local_clock_string(time), clock);
            assert_eq!(
                crate::usage::external::local_day_from_iso(
                    iso,
                    crate::usage::external::platform_offset,
                )
                .as_deref(),
                Some(date)
            );
        }
    }

    /// Unix 纪元之前取不到时间——回退值必须能被下游解析，而不是抛。
    #[test]
    fn a_clock_before_the_epoch_falls_back_instead_of_panicking() {
        let before = UNIX_EPOCH - Duration::from_secs(1);
        assert_eq!(local_date_string(before), "1970-01-01");
        assert_eq!(local_clock_string(before), "--:--:--");
        assert_eq!(local_hms_string(before), "000000");
    }

    // ─── 历史归属的判据（任何平台都跑）──────────────────────────────

    /// 合成一份「美东式」规则：标准时 UTC-5（bias 300、standard_bias 0）、
    /// 夏令时 UTC-4（daylight_bias -60）；三月第二个周日 02:00 转入夏令时、
    /// 十一月第一个周日 02:00 转回标准时。形状与 Win32 的
    /// `TIME_ZONE_INFORMATION` 一致（bias 单位分钟、切换时刻按切换前的
    /// 偏移解释）。
    #[cfg(any(windows, test))]
    fn eastern_rules() -> ZoneRules {
        ZoneRules {
            bias: 300,
            standard_bias: 0,
            daylight_bias: -60,
            standard: TransitionRule {
                month: 11,
                day_of_week: 0,
                day: 1,
                hour: 2,
                minute: 0,
                second: 0,
            },
            daylight: TransitionRule {
                month: 3,
                day_of_week: 0,
                day: 2,
                hour: 2,
                minute: 0,
                second: 0,
            },
        }
    }

    fn utc_at(iso: &str) -> OffsetDateTime {
        OffsetDateTime::parse(iso, &time::format_description::well_known::Rfc3339)
            .expect("时间戳应可解析")
    }

    /// 本地日历日 + 时刻（换算结果的人类可读形式）。
    fn local_day_clock(utc: OffsetDateTime, offset: UtcOffset) -> (String, String) {
        let local = utc.to_offset(offset);
        (
            local
                .format(&format_description!("[year]-[month]-[day]"))
                .expect("日期应可格式化"),
            local
                .format(&format_description!("[hour]:[minute]:[second]"))
                .expect("时刻应可格式化"),
        )
    }

    /// 历史时刻按**当年**的规则归属：夏令时前后差一小时，跨年时刻落在
    /// 正确的本地年与本地日。
    ///
    /// 这条判据不依赖本机时区、不切 `TZ`、不靠子进程，因此在 Windows 上
    /// 也真跑——挂 `#[cfg(unix)]` 的那条（`time` 的 Unix 路径）此前在
    /// Windows 上零覆盖。
    #[test]
    fn historical_timestamps_use_the_offset_of_their_own_year() {
        let offset = |utc| offset_at_with(utc, |_| Some(eastern_rules())).expect("应能算出偏移");

        // 冬天按标准时（UTC-5），夏天按夏令时（UTC-4）。
        let winter = utc_at("2026-01-15T04:30:00Z");
        let winter_offset = offset(winter);
        assert_eq!(winter_offset.whole_seconds(), -5 * 3600);
        assert_eq!(
            local_day_clock(winter, winter_offset),
            ("2026-01-14".to_string(), "23:30:00".to_string())
        );

        let summer = utc_at("2026-07-15T04:30:00Z");
        let summer_offset = offset(summer);
        assert_eq!(summer_offset.whole_seconds(), -4 * 3600);
        assert_eq!(
            local_day_clock(summer, summer_offset),
            ("2026-07-15".to_string(), "00:30:00".to_string())
        );

        // 夏令时切换那一刻（2026-03-08 07:00Z = 切换前的 02:00 标准时）：
        // 前一分钟还是 UTC-5，切换后是 UTC-4。
        let before = utc_at("2026-03-08T06:59:00Z");
        assert_eq!(offset(before).whole_seconds(), -5 * 3600);
        let after = utc_at("2026-03-08T07:00:00Z");
        assert_eq!(offset(after).whole_seconds(), -4 * 3600);

        // 跨年：2026-01-01T02:00Z 在美东是 2025-12-31 21:00（标准时）。
        let cross = utc_at("2026-01-01T02:00:00Z");
        let cross_offset = offset(cross);
        assert_eq!(cross_offset.whole_seconds(), -5 * 3600);
        assert_eq!(
            local_day_clock(cross, cross_offset),
            ("2025-12-31".to_string(), "21:00:00".to_string())
        );
    }

    /// 跨年那一刻用的是**上一年自己的偏移**，不是当前年的。
    ///
    /// 合成规则里 2025 与 2026 的基准差不同（240 / 300）：套错年份会把
    /// 本地时刻整整差一小时。这正是「当前规则套历史记录」那类错法的形状，
    /// 而它在跨年窗口里每天都会发生一次。
    #[test]
    fn cross_year_instant_uses_the_previous_years_own_bias() {
        let rules = |year: i32| {
            let mut rules = eastern_rules();
            if year == 2025 {
                rules.bias = 240;
            }
            Some(rules)
        };
        let cross = utc_at("2026-01-01T02:00:00Z");
        let offset = offset_at_with(cross, rules).expect("应能算出偏移");
        assert_eq!(offset.whole_seconds(), -4 * 3600);
        assert_eq!(
            local_day_clock(cross, offset),
            ("2025-12-31".to_string(), "22:00:00".to_string())
        );
    }

    /// Windows 上自己算的偏移必须与系统当面的换算一致——**限本年**。
    ///
    /// 系统那套 `SystemTimeToTzSpecificLocalTime(nullptr, …)` 用的是当前规则，
    /// 跨年与规则变更过的年份它自己就会算错，所以这条只在本年比：这一段
    /// 里系统的答案是对的，用它当基准才有效。
    ///
    /// 这条判据是 Windows 侧 FFI 接线是否正确的唯一护栏：纯函数那条验的是
    /// 规则换算，验不到「API 取到的规则是不是当年那套」。
    #[cfg(windows)]
    #[test]
    fn windows_offset_matches_the_system_conversion_within_the_current_year() {
        use windows_sys::Win32::Foundation::SYSTEMTIME;
        use windows_sys::Win32::System::Time::SystemTimeToTzSpecificLocalTime;

        let year = OffsetDateTime::now_utc().year();
        for month in 1..=12u8 {
            for day in [1u8, 15, 28] {
                let Ok(date) = time::Date::from_calendar_date(
                    year,
                    time::Month::try_from(month).unwrap(),
                    day,
                ) else {
                    continue;
                };
                let utc = OffsetDateTime::new_utc(date, time::Time::from_hms(12, 0, 0).unwrap());
                let offset = local_offset_at(utc);
                let ours = utc.to_offset(offset);

                let utc_st = SYSTEMTIME {
                    wYear: utc.year() as u16,
                    wMonth: u8::from(utc.month()) as u16,
                    wDayOfWeek: utc.weekday().number_days_from_sunday() as u16,
                    wDay: utc.day() as u16,
                    wHour: utc.hour() as u16,
                    wMinute: utc.minute() as u16,
                    wSecond: utc.second() as u16,
                    wMilliseconds: 0,
                };
                let mut local_st: SYSTEMTIME = unsafe { std::mem::zeroed() };
                // Safety: 输入与输出都是我们自己的定长结构；时区信息传 NULL
                // 表示「用当前时区」，与 `time` 的 Windows 实现同一条路径。
                let ok = unsafe {
                    SystemTimeToTzSpecificLocalTime(std::ptr::null(), &utc_st, &mut local_st)
                };
                assert_ne!(ok, 0, "系统换算应当成功（{year}-{month}-{day}）");
                let theirs = OffsetDateTime::new_utc(
                    time::Date::from_calendar_date(
                        local_st.wYear as i32,
                        time::Month::try_from(local_st.wMonth as u8).unwrap(),
                        local_st.wDay as u8,
                    )
                    .unwrap(),
                    time::Time::from_hms(
                        local_st.wHour as u8,
                        local_st.wMinute as u8,
                        local_st.wSecond as u8,
                    )
                    .unwrap(),
                );
                // 比的是**墙钟**（本地年月日时分秒），不是带偏移的时间戳：
                // 系统换算给的是本地墙上时间，两者的偏移表示本就不同。
                let wall = format_description!("[year]-[month]-[day] [hour]:[minute]:[second]");
                assert_eq!(
                    ours.format(&wall).unwrap(),
                    theirs.format(&wall).unwrap(),
                    "{year}-{month}-{day} 的本地时间与系统换算不一致"
                );
            }
        }
    }
}
