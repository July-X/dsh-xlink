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
        written_recently(&entry)
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

/// 该目录最近是否被改动过。
///
/// 读不出 mtime 时返回 false：同理，宁可漏判。Windows 上目录 mtime 的更新
/// 时机与 Unix 不完全一致（受目录项缓存影响），偏保守正好落在这里。
fn written_recently(entry: &std::fs::DirEntry) -> bool {
    entry
        .metadata()
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .is_some_and(|age| age < RECENT_WRITE)
}

/// `pid` 是否仍是一个活着的进程。
///
/// Unix 用 `kill(pid, 0)`：它不投递信号，只问「这个 pid 在不在」，且**不会**回收
/// 僵尸——`waitpid` 会把观测动作本身变成破坏，所以这里绝不能用它。返回 `EPERM`
/// 表示进程在、只是没权限，同样算活着；只有 `ESRCH`（pid 从进程表消失）才算没了。
#[cfg(unix)]
fn process_alive(pid: u32) -> bool {
    if unsafe { libc::kill(pid as i32, 0) } == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// Windows 上没有信号 0 的等价物，走 `OpenProcess` 探一个查询句柄：能打开就是
/// 进程还在。查询句柄用完立刻关，不留句柄泄漏（`windows-sys` 的
/// `Win32_System_Threading` 特性已经因 Job Object 与 `win_probe.rs` 在依赖里）。
#[cfg(windows)]
fn process_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return false;
    }
    unsafe { CloseHandle(handle) };
    true
}

#[cfg(test)]
mod tests {
    use super::*;
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

    /// 造一个暂存目录名，带可控的 pid 与年龄。
    fn staging(store: &Path, prefix: &str, pid: u32, age: Duration) -> std::path::PathBuf {
        let name = format!("{prefix}{pid}-1234567890");
        let dir = store.join(name);
        fs::create_dir_all(&dir).expect("create staging");
        // mtime 回拨只求「看起来很久以前」；某些文件系统上精度受限，
        // 因此断言只对「明显够老」的方向成立（见用例）。
        let when = SystemTime::now() - age;
        if age.as_secs() > 5 {
            filetime_shift(&dir, when);
        }
        dir
    }

    /// 把目录 mtime 改到过去。macOS / Linux 可用 `utimensat`，Windows 上退化为
    /// 不改（相关用例在 `#[cfg(unix)]` 上）。
    #[cfg(unix)]
    fn filetime_shift(dir: &Path, when: SystemTime) {
        let secs = when
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("时间在 epoch 之后")
            .as_secs() as i64;
        let times = [
            libc::timespec {
                tv_sec: secs,
                tv_nsec: 0,
            },
            libc::timespec {
                tv_sec: secs,
                tv_nsec: 0,
            },
        ];
        let c = std::ffi::CString::new(dir.to_string_lossy().as_bytes()).expect("路径无 NUL");
        unsafe {
            libc::utimensat(libc::AT_FDCWD, c.as_ptr(), times.as_ptr(), 0);
        }
    }

    #[cfg(not(unix))]
    fn filetime_shift(_dir: &Path, _when: SystemTime) {}

    #[test]
    fn a_live_foreign_owner_counts_as_busy() {
        let store = store("live");
        // 本进程一定活着，但它被显式排除，所以这里用一个"别的活进程"：
        // 起一个 sleep 子进程当 owner，它的 pid 必然存活且不是我们。
        let mut child = std::process::Command::new("sh")
            .args(["-c", "sleep 30"])
            .spawn()
            .expect("spawn owner");
        staging(&store, ".tmp-", child.id(), Duration::from_secs(0));
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
        staging(&store, ".tmp-", std::process::id(), Duration::from_secs(0));
        assert!(
            !another_writer_active(&store),
            "本进程的暂存目录不算「别人在写」——那会让安装自己跳过对账"
        );
        let _ = fs::remove_dir_all(&store);
    }

    #[test]
    fn a_dead_owner_is_reclaimable() {
        let store = store("dead");
        // 起一个立刻退出的进程拿一个几乎肯定已回收的 pid。
        let mut child = std::process::Command::new("sh")
            .args(["-c", "exit 0"])
            .spawn()
            .expect("spawn");
        let pid = child.id();
        let _ = child.wait();
        staging(&store, ".backup-", pid, Duration::from_secs(0));
        assert!(
            !another_writer_active(&store),
            "创建者已退出的暂存目录必须被留给回收，否则崩溃残留永远清不掉"
        );
        let _ = fs::remove_dir_all(&store);
    }

    #[test]
    fn a_live_owner_with_an_ancient_mtime_is_reclaimable() {
        // pid 复用兜底：名字折着一个活着的 pid，但目录早就不再被写。
        let store = store("recycled");
        let mut child = std::process::Command::new("sh")
            .args(["-c", "sleep 30"])
            .spawn()
            .expect("spawn owner");
        staging(&store, ".new-", child.id(), Duration::from_secs(3600));
        assert!(
            !another_writer_active(&store),
            "久未改动的暂存目录即便 pid 活着也不算忙，否则 pid 复用会让回收永久停摆"
        );
        let _ = child.kill();
        let _ = child.wait();
        let _ = fs::remove_dir_all(&store);
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
