//! 「技能中央库正被**别的进程**写入」的判据。
//!
//! 存在的原因是一个已知的跨壳缺口：技能中央库 `skills/packages/` 与活动视图
//! `skills/active/` 是**两个壳共享**的（`AGENTS.md` 记录了这是维护者有意选的），
//! 而 `manage::lock_store()` 那把锁只到**进程级**——它挡得住本进程内的并发，
//! 挡不住 dev 壳与 release 壳同时动同一个目录。
//!
//! 这个文件只回答一件事：**此刻中央库里那些暂存目录，是不是正被一个活着的
//! 别的进程使用着**。调用方（`manage::reconcile`）据此整轮跳过启动对账。
//!
//! 为什么「跳过」而不是「加锁等它」：暂存目录的存活期覆盖整个下载解包过程，
//! 从几百毫秒到几分钟都可能。等着就是让启动卡几分钟；而对账这件事本身**没有
//! 时效要求**——推迟一轮，下次启动补上即可。破坏性操作（回收别人的暂存目录、
//! 删「清单里没记录」的孤儿链接）与「等一下」的代价比起来不对等，所以选跳过。
//!
//! 判据是「pid 还活着 **且** 目录刚被写过」而不是单看 pid：暂存目录名折了创建者
//! 的 pid（见 `pkg::fetch::new_staging_dir` 的 `.tmp-<pid>-<nanos>`），但 pid 会
//! 被系统复用，一个上次崩下的残留很可能带着一个**如今活着的**别的进程的 pid；
//! 单看 pid 会让回收永久停摆。反过来单看 mtime 也不够——慢速下载中途目录长时间
//! 不会被改动，会被当成残留删掉。两个条件合起来才既不打断活人、也不放过死鬼。

use std::path::Path;
use std::time::{Duration, SystemTime};

use super::store_owner::process_alive;

/// 暂存目录名的三个前缀，与 `manage` 里的同名常量一致（`pkg::fetch::new_staging_dir`
/// 拼出来的形状就是 `{prefix}{pid}-{nanos}`）。
const STAGING_PREFIXES: [&str; 3] = [".tmp-", ".new-", ".backup-"];

/// 暂存目录的 mtime 超过这个年龄时，不再认为有活人在写它。
///
/// 取 10 分钟：远大于任何一次正常的下载解包，而远小于「用户半夜崩溃了第二天早上
/// 才开一次壳」这种间隔。它不是正确性的保证——正确性由 pid 判活保证——而是
/// 给 pid 复用兜一个底，避免上一条说的「残留带着活 pid 导致永久停摆」。
const RECENT_WRITE: Duration = Duration::from_secs(10 * 60);

/// `store`（中央库根）里是否有**别的进程**正在使用中的暂存目录。
///
/// 本进程自己写的暂存目录不算——同一个进程里 `lock_store()` 已经串行化了，
/// 而把「自己正在装技能」判成「别人在写」会让安装流程跳过对账，白丢一次修复。
/// 目录读不出来时返回 false：那是「没有证据有人在写」，交给调用方按常态处理，
/// 而不是「有证据有人在写」——后者会平白吞掉一轮对账。
pub(crate) fn another_writer_active(store: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(store) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            return false;
        };
        let Some(owner_pid) = staging_owner_pid(&name) else {
            return false;
        };
        if owner_pid == std::process::id() {
            return false;
        }
        if !process_alive(owner_pid) {
            return false;
        }
        looks_in_use(true, entry_age(&entry))
    })
}

/// 从暂存目录名里解出创建者的 pid：`.tmp-<pid>-<nanos>`。
///
/// 名字不是这三个前缀开头的、或 pid 段解析不出数字，都返回 None——判据宁可漏判
/// （那一轮不做对账，下次启动还会再来）也不误判。
fn staging_owner_pid(name: &str) -> Option<u32> {
    let rest = STAGING_PREFIXES
        .iter()
        .find_map(|prefix| name.strip_prefix(prefix))?;
    rest.split('-').next()?.parse().ok()
}

/// 单个暂存目录「看起来正被使用」的判据（**纯函数**）。
///
/// 拆出来不是为了好看，是为了让「久未改动」这条兜底在**两个平台**上都有覆盖：
/// 原来的用例靠把目录 mtime 回拨一年来构造场景，而 std 在 Windows 上打不开
/// 目录句柄（`File::open` 对目录直接失败），于是那条 `filetime_shift` 在
/// Windows 上是个空实现——**注释写着「相关用例在 `#[cfg(unix)]` 上」，可用例
/// 本身并没有 gate**，于是在 Windows 上它测的是「刚建的目录 + 活 pid」，判据
/// 说它忙，而用例断言它不忙。pid 复用兜底恰恰是这条判据存在的理由，不能只
/// 在一个平台上被验过。
///
/// `owner_alive` 与 `age` 分别是「创建者的 pid 还活着吗」和「这个目录多久没被
/// 动过了（读不出为 None）」。
fn looks_in_use(owner_alive: bool, age: Option<Duration>) -> bool {
    owner_alive && age.is_some_and(|age| age < RECENT_WRITE)
}

/// 该目录距今多久没被改动过；读不出 mtime 时返回 `None`。
///
/// Windows 上目录 mtime 的更新时机与 Unix 不完全一致（受目录项缓存影响），
/// 读不出时按「没有证据有人在写」处理，正好落在这条保守方向上。
fn entry_age(entry: &std::fs::DirEntry) -> Option<Duration> {
    let modified = entry.metadata().ok()?.modified().ok()?;
    SystemTime::now().duration_since(modified).ok()
}

// 判活与「多久没动过」都在 [`store_owner::process_alive`] 与 [`looks_in_use`] 里，
// 本文件只负责「扫目录、把暂存目录名折成 pid」这一层。

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::store_owner::{spawn_exiting, spawn_sleeper};
    use std::fs;

    /// 造一个中央库根，返回它的路径。
    fn store(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "dsh-store-busy-{label}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create store");
        dir
    }

    /// 造一个暂存目录名，带可控的 pid。
    fn staging(store: &Path, prefix: &str, pid: u32) -> std::path::PathBuf {
        let name = format!("{prefix}{pid}-1234567890");
        let dir = store.join(name);
        fs::create_dir_all(&dir).expect("create staging");
        dir
    }

    #[test]
    fn a_live_foreign_owner_counts_as_busy() {
        let store = store("live");
        // 本进程一定活着，但它被显式排除，所以这里用一个"别的活进程"：
        // 起一个子进程当 owner，它的 pid 必然存活且不是我们。
        let mut child = spawn_sleeper();
        staging(&store, ".tmp-", child.id());
        assert!(
            another_writer_active(&store),
            "另一个活进程正在使用的暂存目录必须被判为「中央库正忙」"
        );
        let _ = child.kill();
        let _ = child.wait();
        let _ = fs::remove_dir_all(&store);
    }

    #[test]
    fn our_own_staging_dir_is_not_busy() {
        let store = store("self");
        staging(&store, ".tmp-", std::process::id());
        assert!(
            !another_writer_active(&store),
            "本进程的暂存目录不算「别人在写」——那会让安装自己跳过对账"
        );
        let _ = fs::remove_dir_all(&store);
    }

    #[test]
    fn a_dead_owner_is_reclaimable() {
        let store = store("dead");
        // 起一个立刻退出的进程拿它的 pid（句柄仍开着，正是 Windows 上最容易
        // 判错的那一种「已死」）。
        let mut child = spawn_exiting();
        staging(&store, ".backup-", child.id());
        assert!(
            !another_writer_active(&store),
            "创建者已退出的暂存目录必须被留给回收，否则崩溃残留永远清不掉"
        );
        let _ = child.wait();
        let _ = fs::remove_dir_all(&store);
    }

    /// pid 复用兜底：名字折着一个活着的 pid，但目录早就不再被写。
    ///
    /// 这一条只判**规则**（[`looks_in_use`]），不再去回拨目录 mtime 造假场景：
    /// 改目录 mtime 在 Windows 上做不到（std 打不开目录句柄），原来的
    /// `filetime_shift` 在那里是空实现，于是这个用例在 Windows 上实测测的是
    /// 「刚建的目录 + 活 pid」，与断言相反。规则本身与平台无关，判它就够；
    /// 真的从文件系统那头走一遍由上面三条（新建目录都是新鲜的 mtime）盖住。
    #[test]
    fn an_ancient_staging_dir_does_not_count_as_in_use() {
        assert!(
            !looks_in_use(true, Some(Duration::from_secs(3600))),
            "久未改动的暂存目录即便 pid 活着也不算忙，否则 pid 复用会让回收永久停摆"
        );
        assert!(
            looks_in_use(true, Some(Duration::from_secs(60))),
            "刚被写过的暂存目录且主人活着 = 正在用，不能误判成残留"
        );
        assert!(
            !looks_in_use(false, Some(Duration::from_secs(0))),
            "主人已死时与 mtime 无关，一律不算在用"
        );
        assert!(
            !looks_in_use(true, None),
            "读不出 mtime 按「没有证据有人在写」处理，不能反过来当成占用"
        );
    }

    #[test]
    fn non_staging_entries_are_ignored() {
        let store = store("noise");
        // 一个正式包目录 + 一个无关文件，都不该被当成暂存。
        fs::create_dir_all(store.join("some-skill")).expect("pkg dir");
        fs::write(store.join("store.json"), b"{}").expect("store file");
        assert!(
            !another_writer_active(&store),
            "只有三种暂存前缀算暂存，正式目录与清单文件不算"
        );
        let _ = fs::remove_dir_all(&store);
    }

    #[test]
    fn owner_pid_is_parsed_from_the_staging_name() {
        assert_eq!(staging_owner_pid(".tmp-4242-99"), Some(4242));
        assert_eq!(staging_owner_pid(".new-7-1"), Some(7));
        assert_eq!(staging_owner_pid(".backup-1-2"), Some(1));
        assert_eq!(staging_owner_pid(".tmp-nope-1"), None, "pid 段不是数字");
        assert_eq!(staging_owner_pid("some-skill"), None, "不是暂存前缀");
        assert_eq!(staging_owner_pid(".tmp-"), None, "缺 pid 段");
    }
}
