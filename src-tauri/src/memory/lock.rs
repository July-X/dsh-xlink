//! 跨**进程**的写锁：两个壳指向同一个实例时，进程内那把 Mutex 拦不住对方。
//!
//! ## 为什么需要它
//!
//! 记忆库与任务表的写入都是「读 → 改 → 原子写回」（`store::mutate` / `jobs::mutate`），
//! 两者各自有一把进程内的 Mutex——它只串行化**本进程**。设计 §4.1 末尾把这件事留给 P2 评估，
//! 现在 P2 的租约已经落地，可以把结论写下来：**租约本身也存在盘上，所以它保护不了跨进程的
//! 「读-改-写」**——两个进程各自读到同一份表、各自改、后写的覆盖先写的，症状是
//! 「我明明存了两条，只剩一条」而且不报任何错。
//!
//! ## 为什么是文件锁而不是「加一段 jobs 状态」
//!
//! 设计给的两条路是「加 `jobs` 段或换承载」，两条都要动数据形状或承载方式。文件锁是第三条：
//! **语义完全不变**（还是那两个文件、还是那份 JSON），只是把「读-改-写」放进一个跨进程的
//! 临界区里。代价是一次 `flock` 系统调用，收益是把静默丢数据变成正确串行。
//!
//! ## 为什么是 `try_lock` 加重试，而不是阻塞等
//!
//! 阻塞等意味着**另一个壳卡住时这边也一起卡住**（面板点一下就没反应了，还不说为什么）。
//! 这里只等一小会儿，等不到就**如实报错**——文案里说清「另一个壳正在写这个实例」，
//! 用户知道该关掉哪一个。宁可失败得清楚，也不要挂得莫名。
//!
//! ## 用守卫而不是闭包
//!
//! `WriteLock::new` 构造即加锁、`Drop` 即释放，调用点只要一行；闭包版本要把整段「读-改-写」
//! 缩进一层，而 `jobs.rs` 在 320/320 的反棘轮线上，加不了那两行。
//!
//! ## 不可重入
//!
//! `flock` 是**按打开的文件描述**算的，同一进程里再开一次也拿不到——所以**不许嵌套**。
//! 当前调用点都在「一次读-改-写」的最外层（`store::mutate` / `jobs::mutate`），没有嵌套；
//! 新增写入路径时要留意这一点。

use std::cell::Cell;
use std::fs::{self, File, OpenOptions};
use std::marker::PhantomData;
use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant};

use crate::shell::error::AppError;

/// 锁文件的名字。放在实例的记忆目录里，与它保护的两个文件同处一地。
pub(crate) const LOCK_FILE: &str = ".write.lock";

/// 等锁的总时长：够让另一个进程写完一次（几百字节的 JSON），又不至于让界面看起来卡死。
const WAIT_TOTAL: Duration = Duration::from_millis(500);
const WAIT_STEP: Duration = Duration::from_millis(25);

thread_local! {
    /// 本线程已经把这把锁拿了几层（`0` = 没拿）。
    ///
    /// 为什么需要它：`/submit` 要**跨两次写**（先按令牌判租约、再写候选）才关得上「判完到写之间
    /// 被接管」的窗口，而它中间调用的 `store::submit_candidate` 会再拿一次锁——`flock` 按**文件
    /// 描述**算，同进程再开一个描述符也拿不到，于是自己把自己拒了（独立评审第 10 条）。
    /// 有这层计数之后，**同一线程**嵌套拿锁变成空操作；另一个线程照旧要排队（那是我们想要的：
    /// 跨文件的写本来就该串行）。
    static DEPTH: Cell<u32> = const { Cell::new(0) };
}

/// 跨进程写锁：**构造即加锁**，`Drop` 即释放。**同线程可重入**（见 [`DEPTH`]）。
///
/// `Debug` 是给测试用的：`WriteLock::new(...).unwrap_err()` 要求 `Ok` 那侧能打印。
///
/// `PhantomData<*const ()>` 让它**不是 `Send`**：可重入计数是**线程局部**的，guard 被搬到别的线程
/// 去 drop 会让原来那个线程的计数永远停在 1，之后它在同一线程里的嵌套拿锁就再也不加锁了——
/// 那是静默失去互斥，比编译错误难查得多。
#[derive(Debug)]
pub(crate) struct WriteLock {
    /// `None` = 重入的那一层：不重复加锁，`Drop` 时也不释放（释放是外层的事）。
    file: Option<File>,
    _not_send: PhantomData<*const ()>,
}

impl WriteLock {
    /// 拿到锁，或等一小会儿之后回一条说得清的错误。**顺带把目录建出来**（写之前总要建，
    /// 所以 `jobs::persist` 里那段 `create_dir_all` 可以省掉）。
    pub(crate) fn new(dir: &Path) -> Result<Self, AppError> {
        fs::create_dir_all(dir).map_err(|error| {
            AppError::Memory(format!("无法创建记忆目录 {}：{error}", dir.display()))
        })?;
        if DEPTH.with(|depth| depth.get()) > 0 {
            DEPTH.with(|depth| depth.set(depth.get() + 1));
            return Ok(Self {
                file: None,
                _not_send: PhantomData,
            });
        }
        let path = dir.join(LOCK_FILE);
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .map_err(|error| {
                AppError::Memory(format!("无法打开写锁 {}：{error}", path.display()))
            })?;
        restrict_permissions(&file, &path)?;
        let deadline = Instant::now() + WAIT_TOTAL;
        loop {
            match file.try_lock() {
                Ok(()) => {
                    DEPTH.with(|depth| depth.set(1));
                    return Ok(Self {
                        file: Some(file),
                        _not_send: PhantomData,
                    });
                }
                Err(_) if Instant::now() < deadline => sleep(WAIT_STEP),
                Err(error) => {
                    return Err(AppError::Memory(format!(
                        "另一个壳正在写这个实例的记忆（锁 {} 拿不到：{error}）。\
                         等它写完再试，或者先关掉另一个窗口。",
                        path.display()
                    )))
                }
            }
        }
    }
}

impl Drop for WriteLock {
    fn drop(&mut self) {
        DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
        // 重入那一层没有自己的文件描述符，释放是外层的事。
        // 释放是幂等的；drop 里不能抛，失败就让它随进程结束一起消失。
        if let Some(file) = &self.file {
            let _ = file.unlock();
        }
    }
}

/// 锁文件按 0600 建：它不含内容，但目录里不该多出一个别人可写的文件。
#[cfg(unix)]
fn restrict_permissions(file: &File, path: &Path) -> Result<(), AppError> {
    use std::os::unix::fs::PermissionsExt;
    file.set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(|error| AppError::Memory(format!("无法设置写锁权限 {}：{error}", path.display())))
}

#[cfg(not(unix))]
fn restrict_permissions(_file: &File, _path: &Path) -> Result<(), AppError> {
    Ok(())
}

/// **接线判据用**：此刻这把锁被别人拿着吗（含同一进程里的另一个描述符）。
///
/// 有测试而没接线是本仓反复栽的形状，所以 `store::mutate` / `jobs::mutate` 里各有一条
/// 用例在临界区**内部**调它——摘掉接线立刻红。
#[cfg(test)]
pub(crate) fn lock_is_held(dir: &Path) -> bool {
    let path = dir.join(LOCK_FILE);
    let Ok(probe) = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
    else {
        return false;
    };
    probe.try_lock().is_err()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "dsh-memory-lock-{tag}-{}-{}",
            std::process::id(),
            crate::memory::store::new_id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn open_lock_file(dir: &Path) -> File {
        OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(dir.join(LOCK_FILE))
            .unwrap()
    }

    /// **同一线程可以重入**（`/submit` 要跨两次写才关得上「判完到写之间被接管」的窗口，
    /// 而中间那次 `store::submit_candidate` 会再拿一次锁），**别的线程仍然被挡住**。
    #[test]
    fn the_same_thread_may_reenter_but_another_thread_still_waits() {
        let dir = temp_dir("reentrant");
        let outer = WriteLock::new(&dir).unwrap();

        // 同线程再拿一次：立刻成功（不阻塞、不报「另一个壳在写」），而且锁还在手上。
        let started = Instant::now();
        let inner = WriteLock::new(&dir).expect("同线程嵌套该直接成功");
        assert!(
            started.elapsed() < WAIT_TOTAL / 2,
            "嵌套拿锁不该等（等了 {:?}，那说明它去抢文件锁了）",
            started.elapsed()
        );
        assert!(lock_is_held(&dir), "重入的那一层不该把锁放掉");

        // 另一线程：照样拿不到（这才是互斥真正的形状）。
        let probe_dir = dir.clone();
        let other = std::thread::spawn(move || {
            assert!(WriteLock::new(&probe_dir).is_err(), "别的线程不该拿到");
        });
        other.join().unwrap();

        drop(inner);
        assert!(lock_is_held(&dir), "内层 drop 之后外层还拿着");
        drop(outer);
        assert!(!lock_is_held(&dir), "外层 drop 之后才真的放掉");
    }

    #[test]
    fn a_second_handle_cannot_take_the_lock_while_the_first_holds_it() {
        let dir = temp_dir("exclusive");
        let first = open_lock_file(&dir);
        first.lock().unwrap();

        // 同一进程里再开一个描述符——`flock` 按**描述**算，所以这里也拿不到。
        let second = open_lock_file(&dir);
        assert!(
            second.try_lock().is_err(),
            "第一个还持着锁时第二个不该拿到——跨进程互斥就是靠这一点"
        );

        first.unlock().unwrap();
        assert!(second.try_lock().is_ok(), "放开之后要能拿到");
    }

    #[test]
    fn the_guard_really_holds_the_lock_and_releases_it_on_drop() {
        let dir = temp_dir("critical");
        let held_inside;
        {
            let _lock = WriteLock::new(&dir).unwrap();
            held_inside = lock_is_held(&dir);
        }
        assert!(held_inside, "临界区里别人拿不到锁");
        // 出来之后锁要放掉，否则下一次写入会白白等 500 毫秒再报错。
        assert!(!lock_is_held(&dir), "守卫 drop 要释放");
    }

    #[test]
    fn an_unreachable_directory_fails_loudly_instead_of_writing_without_a_lock() {
        // 拿锁的路径上一个**文件**挡着：建目录会失败，这时宁可报错也不要无锁写入。
        let dir = temp_dir("blocked");
        let blocker = dir.join("not-a-dir");
        fs::write(&blocker, b"x").unwrap();
        let error = WriteLock::new(&blocker.join("nested")).unwrap_err();
        assert!(error.to_string().contains("无法创建记忆目录"), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn the_lock_file_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_dir("perms");
        let _lock = WriteLock::new(&dir).unwrap();
        let mode = fs::metadata(dir.join(LOCK_FILE))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "锁文件按 0600 建");
    }
}
