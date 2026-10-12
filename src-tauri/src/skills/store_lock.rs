//! 「技能中央库」的**跨进程**互斥锁。
//!
//! `manage::lock_store()` 那把 `OnceLock<Mutex<()>>` 只到进程级：它挡得住本进程内
//! 的并发，挡不住 dev 壳与 release 壳同时改同一个 `skills/packages/` 与
//! `skills/active/`（共享是有意选的，见 AGENTS.md）。于是两个壳各装各的技能时，
//! 各自读到同一份旧 `store.json`、各自写回，后写者把先写者**整条覆盖**——面板上
//! 那个技能查不到、卸载报「技能包不在中央库中」，而它的目录与链接都还在、工作台
//! 也照常加载。用户无从自查。
//!
//! ## 为什么不上 `fs2` / `fs4` / `fs-ext`
//!
//! 三条理由，按重要性：
//!
//! 1. **`libc` 与 `windows-sys` 都已在依赖里**，用它们做平台锁不新增任何 crate
//!    （`windows-sys` 那几个特性已因 Job Object 与 `win_probe.rs` 打开）。而
//!    `fs2` / `fs4` 会**新增依赖并改动 `Cargo.lock`**——CI 全线带 `--locked`，
//!    锁文件与依赖声明不同步会在两个平台上同时炸，且报错离真正原因隔好几层。
//! 2. 锁文件本身要落在用户数据目录里，得处理"目录不存在"、"路径被占用"、
//!    "进程崩了留下僵尸锁"——这些用 `create_new` 一条路径就能覆盖。
//! 3. 本仓库的 `Cargo.toml` 对每个依赖都写了"为什么加它 / 为什么不加它"的注释，
//!    新增一个纯锁依赖要交代的理由，不如直接用平台原生。
//!
//! ## 锁的形状
//!
//! 一个 `<store>/.store-lock` 文件，**内容是持有者的 pid**。获取 = `create_new`
//! 独占创建；释放 = 删除。拿不到就按退避重试，超时则**强制抢**（见
//! [`StealPolicy`]）。
//!
//! **僵尸锁**是这套形状唯一的真风险：持有者进程被 `kill -9` 时文件留下，后续所有
//! 获取者会一直等下去，直到超时。超时后读文件里的 pid 判活——活着就是真有人在
//! 持有（可能是个跑很久的大下载），死了就抢。所以 pid 必须写进文件，不能写空。
//!
//! **只做互斥，不做公平**：多等待者同时超时会有几个一起抢，靠 `create_new` 的
//! 原子性决定唯一赢家（`AlreadyExists` 的人重新排队）。抢不到的下轮再试。

use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::store_owner::process_alive;

/// 锁文件名。放在中央库根（`skills/packages/`）下，与包目录同层。
///
/// 选这里而不是 `<xlink_home>/`：锁必须和它保护的数据**同卷**才有意义，而
/// `store.json` 就在这个目录。放在更上层会引入"锁和数据不在一个卷"的假设。
const LOCK_FILE: &str = ".store-lock";

/// 获取锁时的退避步长与上限。
///
/// 与 `shell/process.rs` 的 `RUN_CAPTURE_POLL_*` 同一形状：小步先走，避免两个壳
/// 同时抢时反复空转；上限 50ms 让长等待不至于变成忙等。
const POLL_MIN: Duration = Duration::from_millis(2);
const POLL_MAX: Duration = Duration::from_millis(50);

/// 默认等待预算。取 60 秒。
///
/// 定这个数的依据：锁覆盖的临界区里最慢的是 `fetch_into_store` 的下载解包
/// （AGENTS.md 记的 `GIT_CLONE_TIMEOUT` 是 300 秒，但那是 git 克隆的上限，典型
/// 远小于此）。60 秒是"绝大多数安装在等待期间就结束了"与"超时抢锁也不会
/// 抢到一个还在正常下载的进程"之间的折中——后者由 pid 判活兜底，所以这个值
/// 偏保守一些更安全。
const DEFAULT_WAIT: Duration = Duration::from_secs(60);

/// 持有期间发现锁文件已被别人抢走时的行为。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StealPolicy {
    /// 抢锁：先删掉对方的锁文件再独占创建。用于**只读/可推迟**的路径
    /// （如启动对账）。
    ///
    /// 对方被抢时会在释放阶段发现文件已不属于自己——那是对的，它本来就不该
    /// 让一个只读路径永久阻塞。
    Steal,
    /// 不抢：拿不到就返回错误。用于**写路径**——写路径抢锁等于两个进程同时
    /// 改 `store.json`，那正是这把锁要防的事。
    Fail,
}

/// 已持有的跨进程锁。Drop 时释放。
///
/// **不要手工提前 drop**：提前释放会让后面拿不到锁的进程以为没人持有，于是
/// 与本进程并发改同一份数据。需要缩短临界区时，正确做法是把临界区拆细，让锁
/// 自然落在一个更小的范围内。
#[derive(Debug)]
pub(crate) struct StoreLock {
    path: PathBuf,
}

impl StoreLock {
    /// 释放锁文件。
    ///
    /// 幂等：文件不在了也返回 Ok（可能已被超时方抢走，此时**不该**报���——它已经
    /// 不是我们的了，删别人的锁会让那个进程以为自己还持有）。
    fn release(&self) -> io::Result<()> {
        match std::fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }
}

impl Drop for StoreLock {
    fn drop(&mut self) {
        if let Err(error) = self.release() {
            eprintln!(
                "dsh-xlink: 技能库跨进程锁未能释放（{}）：{}",
                self.path.display(),
                error
            );
        }
    }
}

/// 获取技能中央库的跨进程锁。
///
/// `store` 是中央库根（`store_dir(home)`）。`wait` 超过后按 `steal` 处理。
pub(crate) fn acquire(store: &Path, steal: StealPolicy) -> Result<StoreLock, String> {
    acquire_within(store, steal, DEFAULT_WAIT)
}

/// 同 [`acquire`]，但等待预算可调（测试与调用方自定义用）。
pub(crate) fn acquire_within(
    store: &Path,
    steal: StealPolicy,
    wait: Duration,
) -> Result<StoreLock, String> {
    let path = store.join(LOCK_FILE);
    let deadline = Instant::now() + wait;
    let mut step = POLL_MIN;

    loop {
        match try_create(&path) {
            TryOutcome::Acquired(lock) => return Ok(lock),
            TryOutcome::Broken(error) => return Err(format!("技能库锁不可用：{error}")),
            TryOutcome::Taken => {}
        }

        if Instant::now() >= deadline {
            match steal {
                StealPolicy::Fail => {
                    return Err(format!(
                        "技能库正被另一个进程占用（等了 {}），本次操作未执行；\
                         若确认没有别的壳在装技能，可重试",
                        secs(wait)
                    ));
                }
                StealPolicy::Steal => {
                    if owner_is_dead(&path) {
                        let _ = std::fs::remove_file(&path);
                        continue;
                    }
                    // 持有者还活着：它是只读路径，等着比抢掉安全，但预算已尽——
                    // 放弃这一轮（不删它的锁），下轮再有机会。
                    return Err(format!(
                        "技能库正被另一个进程写入（等了 {}），本次对账推迟",
                        secs(wait)
                    ));
                }
            }
        }

        std::thread::sleep(step);
        step = (step * 2).min(POLL_MAX);
    }
}

/// 一次独占创建的尝试。
///
/// 拆成独立的枚举是因为 `io::ErrorKind::AlreadyExists` 既是"被别人占了"也是
/// "父目录不存在"之外的各种情况，需要与"真的坏了"分开处理。
#[derive(Debug)]
enum TryOutcome {
    Acquired(StoreLock),
    Taken,
    Broken(io::Error),
}

fn try_create(path: &Path) -> TryOutcome {
    let Some(parent) = path.parent() else {
        return TryOutcome::Broken(io::Error::new(
            io::ErrorKind::InvalidInput,
            "lock path has no parent",
        ));
    };
    if std::fs::create_dir_all(parent).is_err() {
        return TryOutcome::Broken(io::Error::other("cannot create lock parent dir"));
    }
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut file) => {
            use std::io::Write;
            // pid 写不进去不算失败：锁文件本身已经是独占的，判活退化成
            // "永远认为持有者活着"（超时后不抢），保守方向。
            let _ = write!(file, "{}", std::process::id());
            let _ = file.sync_all();
            TryOutcome::Acquired(StoreLock {
                path: path.to_path_buf(),
            })
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => TryOutcome::Taken,
        Err(error) => TryOutcome::Broken(error),
    }
}

/// 锁文件里的 pid 是否已经死了。读不出 pid 时保守地回答"活着"。
fn owner_is_dead(path: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(path) else {
        // 读不出来（被抢的瞬间、或权限问题）——不抢。
        return false;
    };
    let Ok(pid) = text.trim().parse::<u32>() else {
        return false;
    };
    !process_alive(pid)
}

fn secs(duration: Duration) -> String {
    let value = duration.as_secs_f32();
    if value.fract() == 0.0 {
        format!("{value} 秒")
    } else {
        format!("{value:.1} 秒")
    }
}

/// 技能中央库的**完整**互斥：进程内 Mutex + 跨进程文件锁，一起持有。
///
/// 两把锁**必须成对出现**，所以它们由这个函数一手包办——外部拿不到"只有文件锁"
/// 或"只有 Mutex"的组合，那正是两种撞车：
///
/// - **只有进程内锁** → 两个壳各读各改各写同一份 `store.json`，后写的整条覆盖
///   先写的（面板上那个技能查不到、卸载报「不在中央库中」，而目录与链接都还在）。
/// - **只有跨进程锁** → 本进程内两个线程各拿一把**不同的**文件锁（`create_new`
///   让它们互相看不见），于是自己和自己撞，而 Mutex 明明就是防这个的。
///
/// 获取顺序固定为「先 Mutex、后文件锁」，全局一致因此无死锁可能：每个进程最多
/// 持一把 Mutex，拿到它之后最多等一个文件锁，别的进程持有的 Mutex 与本进程
/// 无关，等不到就超时退让。
///
/// Drop 顺序由字段声明决定（文件锁先放），两者之间没有窗口——期间若有别的进程在
/// 等，它拿到锁时本进程已不再改数据。
///
/// `steal`：写路径一律 [`StealPolicy::Fail`]（抢锁等于并发写，正是这把锁要防的
/// 事），只有启动对账那种可推迟的只读路径才用 [`StealPolicy::Steal`]。
pub(crate) fn lock_both(store: &Path, steal: StealPolicy) -> Result<StoreGuard, String> {
    let process_guard = crate::lock(process_mutex());
    let file_guard = acquire(store, steal)?;
    Ok(StoreGuard {
        _process: process_guard,
        _file: file_guard,
    })
}

/// 中央库的进程内互斥。**不单独暴露**——它只在 [`lock_both`] 内部出现，
/// 以免有人拿它当"完整互斥"用（那正是丢更新的成因）。
fn process_mutex() -> &'static std::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
}

/// [`lock_both`] 返回的守卫。字段顺序即释放顺序：文件锁先放。
#[derive(Debug)]
pub(crate) struct StoreGuard {
    _process: std::sync::MutexGuard<'static, ()>,
    _file: StoreLock,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn store(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "dsh-store-lock-{label}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create store");
        dir
    }

    #[test]
    fn acquires_and_releases() {
        let store = store("basic");
        {
            let _lock = acquire(&store, StealPolicy::Fail).expect("first acquire");
            assert!(
                store.join(LOCK_FILE).exists(),
                "持有期间锁文件必须在——否则别的进程以为没人持有"
            );
        }
        assert!(
            !store.join(LOCK_FILE).exists(),
            "Drop 必须释放锁，否则第二次获取必然超时"
        );
        // 释放后能再拿。
        let _again = acquire(&store, StealPolicy::Fail).expect("re-acquire after release");
        let _ = fs::remove_dir_all(&store);
    }

    #[test]
    fn a_held_lock_blocks_the_second_acquirer() {
        let store = store("held");
        let _held = acquire(&store, StealPolicy::Fail).expect("first");
        let err = acquire_within(&store, StealPolicy::Fail, Duration::from_millis(30))
            .expect_err("第二个必须拿不到");
        assert!(
            err.contains("另一个进程占用"),
            "错误要说清是被占用而不是泛化失败：{err}"
        );
        let _ = fs::remove_dir_all(&store);
    }

    #[test]
    fn a_live_owner_is_never_stolen() {
        // 写路径绝不能抢：本进程的 pid 一定活着，Steal 也必须失败。
        let store = store("live-owner");
        fs::write(store.join(LOCK_FILE), format!("{}", std::process::id())).expect("plant");
        let err = acquire_within(&store, StealPolicy::Steal, Duration::from_millis(30))
            .expect_err("活着的持有者不得被抢");
        assert!(
            err.contains("正被另一个进程写入"),
            "超时且持有者活着时要放弃这一轮：{err}"
        );
        assert!(
            store.join(LOCK_FILE).exists(),
            "放弃时不得删掉持有者的锁——那是它的"
        );
        let _ = fs::remove_dir_all(&store);
    }

    #[test]
    fn a_dead_owner_is_stolen() {
        // 崩溃留下的僵尸锁必须能被清掉，否则这把锁会永久堵死技能安装。
        let store = store("dead-owner");
        let mut child = crate::skills::store_owner::spawn_exiting();
        fs::write(store.join(LOCK_FILE), format!("{}", child.id())).expect("plant");
        let _lock = acquire_within(&store, StealPolicy::Steal, Duration::from_millis(30))
            .expect("死掉的持有者必须被抢");
        // 再 wait 一次是为了让 clippy 看得见「这个 Child 被收过了」；真正起作用的
        // 是它在整个断言期间**持有句柄**这件事——Windows 上进程对象只要还有句柄
        // 就仍能 OpenProcess 成功，判活必须再问退出码（见 store_owner）。
        let _ = child.wait();
        let _ = fs::remove_dir_all(&store);
    }

    #[test]
    fn an_unreadable_owner_is_treated_as_alive() {
        // pid 写不进去时退化成"永远活着"，这是保守方向：宁可不抢，也别两个
        // 进程同时改 store.json。
        let store = store("garbage-owner");
        fs::write(store.join(LOCK_FILE), "not-a-pid").expect("plant");
        let err = acquire_within(&store, StealPolicy::Steal, Duration::from_millis(30))
            .expect_err("读不出 pid 时不得抢");
        assert!(err.contains("正被另一个进程写入"), "{err}");
        let _ = fs::remove_dir_all(&store);
    }

    #[test]
    fn creates_the_store_dir_when_missing() {
        // 全新安装：中央库目录还不存在。锁不能因为"父目录没有"就失败。
        let base = store("fresh");
        let nested = base.join("packages");
        let _lock = acquire(&nested, StealPolicy::Fail).expect("fresh store");
        assert!(nested.join(LOCK_FILE).exists());
        let _ = fs::remove_dir_all(&base);
    }
}

#[cfg(test)]
mod cross_process_tests {
    use super::*;
    use std::fs;

    fn store(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "dsh-store-lock-xp-{label}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create store");
        dir
    }

    /// 子进程正文：**报到 → 取锁 → 读 → 改 → 整份写回 → 放锁**。
    ///
    /// 形状必须与真实丢更新一致：读整份清单、追加自己的标记、再**整份写回**。
    /// 这里踩过两个坑，都记在这里因为它们都不是"测试写错了"那么单纯：
    ///
    /// ① 第一版用「往文件尾追加一行」——无锁时那也两行都在（O_APPEND 的小写入
    ///    本身原子）。反向验时它对着一个被彻底去掉的跨进程锁依然绿：断言看着在
    ///    验互斥，实际验的是内核的 append 语义。
    /// ② 第二版把形状改对了，却忘了**两个子进程可能根本不重叠**——起得晚的那个
    ///    在先起的写完之后才读，于是读到的是已经带对方标记的内容，照样两行都在。
    ///
    /// 所以报到点必须放在**取锁之前**：每个子进程先落一个 `ready-<tag>`，等到两个
    /// 报到文件都在了才去抢锁。这样两个进程必然在同一瞬间起跑——
    /// 有锁时一个进临界区、另一个在外面等（后者读到前者写完的内容 → 两行都在）；
    /// 无锁时两个同时在临界区、各自读到 `""`、后写者整份覆盖 → 只剩一行（红）。
    ///
    /// 报到点不能放在锁**之内**：有锁时那会变成死等（第二个永远进不来）。
    #[test]
    fn child_holds_lock() {
        let Ok(store) = std::env::var("DSH_STORE_LOCK_TEST_STORE") else {
            // 不是被当作 worker 调起的普通 test binary 运行——直接返回。
            return;
        };
        let tag = std::env::var("DSH_STORE_LOCK_TEST_TAG").expect("tag");
        let store = PathBuf::from(store);
        let manifest = store.join("manifest.json");

        // 报到 + 等待同伴报到（10 秒只是防死等，不是本用例要报告的东西）。
        fs::write(store.join(format!("ready-{tag}")), "ready").expect("report ready");
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let both = store.join("ready-alpha").exists() && store.join("ready-beta").exists();
            if both || Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }

        let guard = lock_both(&store, StealPolicy::Fail).expect("child must acquire the lock");

        // 临界区：读 → 拉开窗口 → 改 → 整份写回。
        // 窗口必须大于「拿到锁的时刻之差」与调度抖动，否则持锁的那个写完了第二个
        // 才读，丢更新压根不会发生（就是 ② 那个坑）。
        let current = fs::read_to_string(&manifest).unwrap_or_default();
        std::thread::sleep(Duration::from_millis(300));
        fs::write(&manifest, format!("{current}{tag}\n")).expect("write whole manifest back");

        drop(guard);
    }

    /// 拉起一个 worker 子进程（真进程，不是线程——线程之间 `create_new` 的文件锁
    /// 互相看不见，用线程验等于没验）。
    fn worker(store: &Path, tag: &str) -> std::process::Child {
        let exe = std::env::current_exe().expect("test exe path");
        std::process::Command::new(exe)
            .args([
                "--exact",
                "skills::store_lock::cross_process_tests::child_holds_lock",
            ])
            .env("DSH_STORE_LOCK_TEST_STORE", store)
            .env("DSH_STORE_LOCK_TEST_TAG", tag)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("spawn worker")
    }

    /// 两个真进程并发对同一份清单做「读 → 改 → 整份写回」：两个标记都必须留下。
    ///
    /// 这就是「dev 壳装技能 A、release 壳同时装技能 B」那条丢更新的最小复现。
    /// 报到栅栏保证竞态**确定**发生：无跨进程锁时后写者整份覆盖，用例必须红。
    #[test]
    fn two_processes_do_not_lose_each_others_update() {
        let store = store("two-procs");
        let manifest = store.join("manifest.json");
        fs::write(&manifest, "").expect("seed manifest");

        let mut a = worker(&store, "alpha");
        let mut b = worker(&store, "beta");
        // 父进程不参与开闸——两个子进程自己等对方报到（见 child_holds_lock）。
        // 这里只等它们都退场。
        let _ = a.wait();
        let _ = b.wait();

        let text = fs::read_to_string(&manifest).expect("read manifest back");
        let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
        assert_eq!(
            lines.len(),
            2,
            "两个进程的写入必须都在（少一个 = 整份写回把另一个覆盖了，正是丢更新）：实际 {:?}",
            lines
        );
        assert!(
            lines.contains(&"alpha") && lines.contains(&"beta"),
            "两行必须分别是两个进程写的：{:?}",
            lines
        );
        let _ = fs::remove_dir_all(&store);
    }
}
