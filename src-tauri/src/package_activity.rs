//! 跨壳装包活动信标：让**另一个壳的恢复动作**知道「风还没停」。
//!
//! ## 它到底解决什么（2026-09-30 定案，历史结论有两处要更正）
//!
//! 这个模块最初的动机是「装包把机器打满 → 对面页面加载被拖过看门狗 15s 阈值 →
//! 看门狗把慢判成死并重载」。**那条路径在真机上从未触发过**：当天五次装/删
//! 事故里，对面壳的看门狗重载计数为 0——页面不是被看门狗杀的，是被**内核自己**
//! 换模块换死的（机制见 [`crate::kernel::install_version`] 的文档注释：pnpm store
//! 的 inode 共享 + NTFS ChangeTime + `dsh-client-hmr` 每 500ms 的 stat 轮询把
//! 链接数噪声当成 bundle 重建推给活页面）。打满机器的说法（14 核 / 64GB / NVMe
//! 被一次 9 秒的纯硬链接安装打满）从硬件上就不成立。
//!
//! 真正需要这只表的是**恢复侧**：15:36:3x 的页面自愈刷新与自动重建先后落进
//! 卸载风暴，几秒内又死一次。所以信标如今的职责排序是：
//!
//! 1. [`recovery_backoff`]——恢复动作（页面自愈刷新 / 壳重建）动手前先问它，
//!    风没停就别落子。**主职**。
//! 2. [`load_timeout`]——看门狗阈值放宽。留着当防御（万一真有页面被拖慢的
//!    场景），但它不再是这套信标存在的理由。
//!
//! 它**不阻止**装包，也**不能**让内核的槽位装配竞态消失——那件事归内核仓库。
//!
//! ## 为什么是一个文件、为什么放在 xlink_home 根上
//!
//! 两个壳的树、注册表、插件中央库、端口都分家了，但「对面包活动」这条消息
//! 没法只放在一边。这是 AGENTS.md 那张表之外唯一一处有意的跨壳可变数据，三条
//! 自律把它限制成一个信标而不是状态：
//!
//! 1. **内容与用户数据无关**：只有「谁在装、装到什么时候为止」。删掉它，唯一后果是
//!    对面的恢复动作落点变差（更容易落进风暴）。
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

/// 恢复退避的上限。页面不能无限期黑着等——信标可能被一次卡死的装包拖着
/// （TTL 10 分钟），到点就照常动手，后面还有壳侧重建与手动「刷新工作台」两层
/// 兜底（两侧上限不同：页面 [`crate::harness_window::QUIET_WAIT_CAP`] 更长）。
pub const RECOVERY_BACKOFF_CAP: Duration = Duration::from_secs(120);

/// 自动恢复动作（页面自愈刷新 / 壳重建窗口）动手前应退避多久；`ZERO` 表示风已停。
///
/// 这是信标如今的**主职**：2026-09-30 真机数据里，看门狗「把慢判成死」那条路径
/// 一次都没触发过，反而是**恢复动作落进风暴**各死了一次（页面 3s 自愈刷新、
/// 壳自动重建）。有界退避让刷新落点等到风停——那时一次就能成。
pub fn recovery_backoff() -> Duration {
    remaining()
        .map(|left| left.min(RECOVERY_BACKOFF_CAP))
        .unwrap_or(Duration::ZERO)
}

/// 本次页面加载的有效阈值。看门狗每轮问一次。
///
/// 这是信标的**防御性**用途（主职是 [`recovery_backoff`]）：装包期间对面壳的
/// 页面加载可能真的变慢（下载缓存被挤、杀毒扫描变多），放宽阈值让看门狗别在
/// 那种时刻把「慢」判成「死」。这条路径真机至今没有触发过（2026-09-30 五次
/// 事故里看门狗重载计数为 0——页面是被内核换模块换死的，不是被看门狗杀的），
/// 留着它是因为代价为零、方向正确。
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

    /// 装包活动期间**不重载**——看门狗的防御性放宽（真机至今未触发过这条路径，
    /// 2026-09-30 五次事故里页面都是被内核换模块换死的，不是被看门狗杀的；
    /// 留着它是因为方向正确、代价为零）。
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
            "装包期间把「慢」判成「死」是这套防御要挡的方向"
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

    /// 恢复退避是信标如今的**主职**：风没停给正数、风停给 0、且有上限——页面
    /// 不能无限期黑着等一个可能卡死的装包。2026-09-30 实测：落在风暴中的自愈
    /// 刷新与自动重建都在几秒内又死一次，落点比次数重要。
    #[test]
    fn recovery_backoff_tracks_the_beacon_but_never_exceeds_the_cap() {
        let home = std::env::temp_dir().join(format!("pkg-backoff-{}", std::process::id()));
        let _xlink = scoped_xlink_home(&home);
        std::fs::create_dir_all(&home).expect("create home");
        let file = crate::paths::package_activity_file();
        std::fs::create_dir_all(file.parent().unwrap()).expect("create dir");
        let beacon_with = |ms: u64| {
            write_beacon(&Beacon {
                shell: "dev".into(),
                action: "删除内核版本".into(),
                pid: std::process::id(),
                until_ms: now_ms() + ms,
            })
        };

        // 没有活动 ⇒ 立刻可以动手。
        assert_eq!(recovery_backoff(), Duration::ZERO);

        // 活动还剩约 30s ⇒ 退避接近剩余时间（在 Cap 内）。
        beacon_with(30_000);
        let backoff = recovery_backoff();
        assert!(
            backoff > Duration::from_secs(20) && backoff <= Duration::from_secs(30),
            "退避应接近剩余时间：{backoff:?}"
        );

        // 活动还剩 9 分钟 ⇒ 退避封顶在 RECOVERY_BACKOFF_CAP，而不是跟着 TTL 走。
        beacon_with(540_000);
        assert_eq!(
            recovery_backoff(),
            RECOVERY_BACKOFF_CAP,
            "页面不能无限期黑着等——到点照常动手，后面还有壳侧重建与手动兜底"
        );

        end();
        assert_eq!(recovery_backoff(), Duration::ZERO, "风停了就该立刻放行");
        std::fs::remove_dir_all(&home).ok();
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
