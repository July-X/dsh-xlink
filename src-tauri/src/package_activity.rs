//! 跨壳装包活动信标：让**服务中的那个壳**知道「对面正在把机器打满」。
//!
//! ## 它解决的是一次误判，不是一次数据事故
//!
//! 2026-09-30 实测：dev 壳装内核的 10 秒里，release 壳的工作台在 10:32:47 重载、
//! 10:32:48 撞上内核的启动顺序竞态后黑屏。**重载不是原因，是放大器**——看门狗的
//! 判据是「`Started` 之后 15s 没等到 `Finished`」，而装包时页面不是坏了，是**慢**：
//! pnpm 硬链接数万文件加 node-gyp 编译把 CPU 与磁盘打满，15s 很容易超。看门狗分不清
//! 「慢」和「死」，于是把一次正常的慢加载判成故障，重载又恰好落在机器最忙的那一刻。
//!
//! 所以这里做的事很窄：**装包期间把对面看门狗的耐心放宽**（[`crate::harness_window`]
//! 的 [`effective_load_timeout`](crate::harness_window::effective_load_timeout)），
//! 让那 10 秒里根本不会触发重载。它**不阻止**装包，也**不能**让内核的启动顺序竞态
//! 消失——那件事归内核仓库。
//!
//! ## 为什么是一个文件、为什么放在 xlink_home 根上
//!
//! 两个壳的树、注册表、插件中央库、端口都分家了，但**机器资源是共享的**，而共享
//! 的东西没法只放在一边。这是 AGENTS.md 那张表之外唯一一处有意的跨壳可变数据，三条
//! 自律把它限制成一个信标而不是状态：
//!
//! 1. **内容与用户数据无关**：只有「谁在装、装到什么时候为止」。删掉它，唯一后果是
//!    对面更容易把自己的看门狗误判成故障。
//! 2. **自过期**：内容带到期时刻，读方发现过期一律当没有。壳崩了也不会把对面的
//!    看门狗永久放宽。
//! 3. **原子写 + 唯一写者语义**：两个壳都可能写，谁在装谁写，结束时各自删。冲突的
//!    后果只是「读到一个仍然有效的活动时间」，不会造成误判。
//!
//! 放在 `xlink_home()` 根上（[`crate::paths::package_activity_file`]）而不是某个壳的
//! `desktop[-dev]/` 里：写进另一棵安装树正是 2026-09-29 插件中央库踩过的坑。

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// 信标的存活时长上限。一次内核安装实测 10s~数分钟（0.1.7-rc.2 的一次删除 34.2s，
/// 安装还要跑 node-gyp），给到 10 分钟是「覆盖得住」的量级；再长就只是在延长
/// 误伤的可能。
const ACTIVITY_TTL: Duration = Duration::from_secs(600);

/// 信标内容。字段名随 `KernelStatus` 那批一样走 snake_case（无 `rename_all`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Beacon {
    shell: String,
    action: String,
    /// 写入方的 pid。用于**提前**结束：壳崩了之后不用干等到期。
    pid: u32,
    /// 到期时刻（Unix 毫秒）。
    until_ms: u64,
}

/// 装包活动期间，加载阈值最多放宽到多少。
///
/// 放宽不是无限的：信标理论上可能留下一个陈旧的（壳崩在装包中途、写到一半的
/// 文件），而**看门狗的职责就是把真正卡死的窗口救回来**。给它一个硬顶，超出之后
/// 照常按 [`crate::harness_window::LOAD_TIMEOUT`] 走——那时要么装包真的还在跑（页面
/// 慢但没死，多等几十秒无害），要么信标已经陈旧（按原判据处理才是对的）。
pub const ACTIVITY_LOAD_CAP: Duration = Duration::from_secs(120);

/// 本次页面加载的有效阈值。看门狗每轮问一次。
///
/// **为什么要有这一层**：2026-09-30 实测 dev 壳装内核的 10 秒里，release 壳的
/// 工作台在第 5 秒被自己的看门狗判成故障并重载，重载又恰好落在机器最忙的那一刻，
/// 撞上内核的启动顺序竞态黑屏。**重载不是原因，是放大器**——看门狗原来只有「慢」
/// 与「死」两个概念，而装包时页面确实只是慢。这一层做的事就是让那 10 秒里它分得清。
pub fn load_timeout() -> Duration {
    effective_load_timeout(remaining())
}

/// [`load_timeout`] 的纯函数内核：没有活动就是平时那个门槛；有活动就放宽到「还剩
/// 多久」，但不超过 [`ACTIVITY_LOAD_CAP`]。
///
/// 取 `clamp` 而不是 `max`：信标里写着一个 10 分钟的到期时刻（覆盖得住一次慢安装），
/// 而**放宽到 10 分钟等于把看门狗关掉 10 分钟**。上限必须在这里，不能指望信标写窄。
pub fn effective_load_timeout(activity_remaining: Option<Duration>) -> Duration {
    activity_remaining
        .map(|left| left.clamp(crate::harness_window::LOAD_TIMEOUT, ACTIVITY_LOAD_CAP))
        .unwrap_or(crate::harness_window::LOAD_TIMEOUT)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 装包开始前打信标。写失败只落 stderr——**绝不阻断**调用方的装包动作。
pub fn begin(action: &str) {
    let beacon = Beacon {
        shell: crate::settings::current_mode().as_str().to_string(),
        action: action.to_string(),
        pid: std::process::id(),
        until_ms: now_ms() + ACTIVITY_TTL.as_millis() as u64,
    };
    write_beacon(&beacon);
}

/// 装包结束后撤信标。**不检查归属**：对面此刻也在装包时，我们撤掉的是自己那份，
/// 而两个壳的装包极少真正重叠；真重叠时最坏结果是「提前放宽结束」，不是误判。
pub fn end() {
    let _ = std::fs::remove_file(crate::paths::package_activity_file());
}

fn write_beacon(beacon: &Beacon) {
    let file = crate::paths::package_activity_file();
    if let Some(parent) = file.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
    }
    match serde_json::to_vec(beacon) {
        Ok(bytes) => {
            if let Err(error) = crate::process::atomic_write(&file, &bytes) {
                eprintln!("package-activity: 写信标失败（不阻断装包）：{error}");
            }
        }
        Err(error) => eprintln!("package-activity: 信标序列化失败：{error}"),
    }
}

/// 当前是否还有装包活动；是的话给出**还剩多久**。
///
/// 读不到、解析不了、已过期、写入方已经不在了——四种情况一律 `None`。这里**只放宽
/// 看门狗、从不收紧**，所以误判的方向是「少放宽一次」，代价是一次不必要的重载；
/// 反过来（把没有活动当成有活动）会让一个真正卡死的窗口迟迟不被救回来。
pub fn remaining() -> Option<Duration> {
    let file = crate::paths::package_activity_file();
    let raw = std::fs::read(&file).ok()?;
    let beacon: Beacon = serde_json::from_slice(&raw).ok()?;
    let until = beacon.until_ms.saturating_sub(now_ms());
    if until == 0 {
        return None;
    }
    // 写入方已经退出：提前作废，不必干等到期。查不出来时**当作还在**——到期时刻
    // 仍然兜着底，而「少放宽一次」的代价远小于「把还在装的包当成结束了」。
    if crate::kernel::process_is_definitely_gone(beacon.pid) {
        return None;
    }
    Some(Duration::from_millis(until).min(ACTIVITY_TTL))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::scoped_xlink_home;

    /// 信标是**自过期**的：到期之后读方一律当没有活动。壳崩了 / 进程被杀之后对面
    /// 的看门狗不该被永久放宽——这是这条设计最容易出事的地方。
    #[test]
    fn an_expired_beacon_reads_as_no_activity() {
        let home = std::env::temp_dir().join(format!("pkg-activity-{}", std::process::id()));
        let _xlink = scoped_xlink_home(&home);
        std::fs::create_dir_all(&home).expect("create home");

        let file = crate::paths::package_activity_file();
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).expect("create dir");
        }
        write_beacon(&Beacon {
            shell: "dev".into(),
            action: "安装内核版本".into(),
            pid: std::process::id(),
            until_ms: now_ms() - 1,
        });
        assert!(
            remaining().is_none(),
            "已过期的信标必须读成「没有活动」，否则壳崩一次就把对面看门狗永久放宽"
        );

        // 活着的信标（pid 是本进程，肯定还在）读得到，且不超过 TTL。
        write_beacon(&Beacon {
            shell: "dev".into(),
            action: "安装内核版本".into(),
            pid: std::process::id(),
            until_ms: now_ms() + ACTIVITY_TTL.as_millis() as u64,
        });
        let left = remaining().expect("活着的信标必须读得到");
        assert!(left <= ACTIVITY_TTL, "剩余时间不该超过 TTL：{left:?}");
        assert!(
            left > Duration::from_secs(300),
            "刚写的信标不该只剩一点点：{left:?}"
        );

        // 撤掉之后立刻读成没有。
        end();
        assert!(remaining().is_none(), "撤掉后不该还读得到活动");

        std::fs::remove_dir_all(&home).ok();
    }

    /// 端到端把三段接起来：**装包开始 → 看门狗真的变宽 → 装包结束 → 恢复原样**。
    ///
    /// 前面几组测试各测一段：信标读出来对不对、阈值算得对不对。**中间那一段没人
    /// 测**——而它正是最可能悄悄断掉的地方（`begin` 写了别的文件、`load_timeout`
    /// 读的另一个键、两个函数各测各的都绿，串起来却不通）。看门狗每轮问的正是
    /// `load_timeout()`，所以这里也从它问起。
    #[test]
    fn the_watchdog_widens_while_a_package_op_is_in_flight_and_narrows_after() {
        let home = std::env::temp_dir().join(format!("pkg-activity-e2e-{}", std::process::id()));
        let _xlink = scoped_xlink_home(&home);
        std::fs::create_dir_all(&home).expect("create home");

        let idle = load_timeout();
        assert_eq!(
            idle,
            crate::harness_window::LOAD_TIMEOUT,
            "平时就是那个门槛"
        );

        begin("安装内核版本");
        let busy = load_timeout();
        assert!(
            busy > idle,
            "装包期间看门狗必须变宽——不变宽就等于这一层没接上：{idle:?} -> {busy:?}"
        );
        assert!(busy <= ACTIVITY_LOAD_CAP, "但必须有硬顶：{busy:?}");

        end();
        assert_eq!(
            load_timeout(),
            idle,
            "装包结束必须立刻恢复原样，否则这一次装包会永久放宽对面的看门狗"
        );
        std::fs::remove_dir_all(&home).ok();
    }

    /// 装包活动期间**不重载**——这一层存在的全部理由。
    ///
    /// 2026-09-30 实测：dev 壳装内核的 10 秒里，release 壳的工作台在第 5 秒被
    /// 自己的看门狗判成故障并重载，重载恰好落在机器最忙的那一刻，撞上内核的启动
    /// 顺序竞态黑屏。**重载不是原因，是放大器**。这里钉的就是「那一刻不重载」。
    #[test]
    fn a_slow_load_during_a_package_install_is_not_treated_as_a_stall() {
        let base = crate::harness_window::LOAD_TIMEOUT;
        // 已经 30s —— 按平时的门槛早该重载了。
        let since = std::time::Instant::now() - Duration::from_secs(30);
        // 但对面正在装包、还剩 80s：阈值被抬到 80s，于是这一次**不**重载。
        let timeout = effective_load_timeout(Some(Duration::from_secs(80)));
        assert_eq!(timeout, Duration::from_secs(80));
        assert!(
            !crate::harness_window::should_reload(Some(since), true, 0, timeout),
            "装包期间把「慢」判成「死」，重载恰好落在机器最忙的那一刻——这正是 2026-09-30 黑屏的成因"
        );
        // 同一个 30s，装包结束后就是该重载的。放宽只对活动窗口生效。
        assert!(crate::harness_window::should_reload(
            Some(since),
            true,
            0,
            effective_load_timeout(None)
        ));
        let _ = base;
    }

    /// 放宽必须有硬顶：信标里写着 10 分钟的到期时刻，照它走等于把关掉看门狗
    /// 10 分钟——而看门狗的职责恰恰是把**真正卡死**的窗口救回来。
    #[test]
    fn the_widening_is_capped_so_a_stuck_page_is_still_rescued() {
        let long_beacon = effective_load_timeout(Some(Duration::from_secs(600)));
        assert_eq!(long_beacon, ACTIVITY_LOAD_CAP);
        let since = std::time::Instant::now() - (ACTIVITY_LOAD_CAP + Duration::from_secs(1));
        assert!(crate::harness_window::should_reload(
            Some(since),
            true,
            0,
            long_beacon
        ));
    }

    /// 活动剩余时间**短于**平时门槛时不许把阈值往下压：那是「放宽」，不是「收紧」。
    /// 一个只剩 3s 的活动不该让阈值变成 3s。
    #[test]
    fn a_short_activity_never_narrows_the_timeout() {
        assert_eq!(
            effective_load_timeout(Some(Duration::from_secs(3))),
            crate::harness_window::LOAD_TIMEOUT,
            "放宽只能向上，不能把门槛压到比平时更紧"
        );
    }
    /// 坏文件不能让对面崩溃，也不能被当成「有活动」——后者会把一个真正卡死的
    /// 窗口一直放过去。
    #[test]
    fn a_corrupt_beacon_reads_as_no_activity() {
        let home = std::env::temp_dir().join(format!("pkg-activity-bad-{}", std::process::id()));
        let _xlink = scoped_xlink_home(&home);
        std::fs::create_dir_all(&home).expect("create home");

        let file = crate::paths::package_activity_file();
        std::fs::create_dir_all(file.parent().unwrap()).expect("create dir");
        std::fs::write(&file, b"{ not json").expect("write");
        assert!(remaining().is_none(), "坏信标必须读成「没有活动」");

        std::fs::remove_dir_all(&home).ok();
    }
}
