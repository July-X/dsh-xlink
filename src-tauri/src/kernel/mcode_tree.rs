//! Unix（macOS / Linux）下的 MiniMax 子进程树跟踪与停止确认。
//!
//! Windows 侧用 Job Object 覆盖整棵树（`kernel/mcode_job.rs`）；Unix 没有
//! 等价物，而 MiniMax 的候选包会再派生「任务 shell / hook / 终端」——这些
//! 进程可以 `setsid()` 脱离根进程所在的进程组，父进程退出后还会被
//! reparent 到 launchd。只按「根 pid + 端口 + 原进程组」无法证明整棵树已
//! 退出，这正是 macOS 侧此前拒绝启动的原因。
//!
//! 做法是**周期性进程表快照**：每隔一拍读一次系统进程表，以「身份仍匹配的
//! 已记录进程」为种子把当前仍挂在它们下面的后代记下来（第一拍只有根）。
//! `setsid()` 只换会话与进程组，不改父子关系，
//! 所以脱离进程组的孙进程照样能沿 ppid 链找到；那些父进程先死、随后被
//! reparent 的后代，只要在父进程还活着时被扫到过一次就仍然在册。
//!
//! 进程表来源：macOS 走原生快路径（`libc::proc_listallpids` +
//! `libc::proc_pidinfo(PROC_PIDTBSDINFO)`）——两次系统调用就能拿到
//! 全部进程的 pid / ppid / 启动时间，不必派生 `ps`（每次派生约 40ms
//! CPU，后台常驻跑不起），而且启动时间是**微秒级**，比 `ps` 的 `lstart`
//! （秒级）更能挡住 pid 复用。其余 Unix 退回系统自带的 `ps`：
//! `ps -axo pid=,ppid=,stat=,lstart=`（macOS 的 BSD ps 与 Linux 的 procps
//! 都支持这套 BSD 风格参数）。这是仓库既有的做法：`lifecycle.rs` 的
//! macOS `sysctl` 快路径也是 `ps` 兜底。
//!
//! 防 pid 复用：记 pid 时同时记下它的启动时间。判定存活时身份对不上
//! 就当已退出，**绝不向无关进程发信号**。
//!
//! shortcut: 非 macOS 的 `ps` 路径只有秒级精度（`lstart`），极端情况下
//! 「同一秒内 pid 被复用」无法区分；macOS 的原生路径是微秒级，没有这个问题。

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle, Thread};
use std::time::{Duration, Instant};

use crate::shell::process::GroupStopOutcome;

/// 快照间隔。脱离进程组只改会话、不改父子关系，后代只要还挂在父进程下，
/// 下一拍就会被发现；间隔只影响「父进程立刻死掉」那个窗口的大小。
///
/// macOS 的原生快路径没有进程派生开销，跑得密；其它 Unix 退回 `ps`，
/// 每次派生一个进程（约 40ms CPU），间隔必须放大。
#[cfg(target_os = "macos")]
const POLL_INTERVAL: Duration = Duration::from_millis(250);
#[cfg(not(target_os = "macos"))]
const POLL_INTERVAL: Duration = Duration::from_secs(1);

/// TERM 之后等待后代退出的上限。
const TERM_GRACE: Duration = Duration::from_secs(1);
/// KILL 之后等待后代退出的上限。
const KILL_GRACE: Duration = Duration::from_secs(1);
/// 等待期间的复查间隔。
const VERIFY_INTERVAL: Duration = Duration::from_millis(100);

/// 进程表来自哪条路。
///
/// 两条路的「启动时间」表示法不同（原生是 `秒.微秒`，`ps` 是 `lstart`
/// 字符串），跨来源的身份**无法核对**。名册记住来源，判定时只在同一
/// 来源内比较：来源对不上就按「还活着」处理，而不是按「已退出」——
/// 查不到结论时不能假装树是空的（否则壳会报「已停止」并删掉唯一能
/// 追溯那个进程的记录，把活着的后代变成孤儿）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProcSource {
    /// macOS 原生快路径。
    Native,
    /// `ps` 兜底。
    Ps,
}

/// 进程表里的一行。
#[derive(Debug, Clone)]
struct ProcEntry {
    pid: u32,
    ppid: u32,
    /// 僵尸：已退出但父进程还没收尸，不算「还在运行」。
    zombie: bool,
    /// 进程启动时间，用作 pid 复用核对的身份。含义由 [`ProcSource`] 决定。
    start: String,
    /// 这一行的来源。
    source: ProcSource,
}

/// 读一次系统进程表。
fn snapshot() -> Result<Vec<ProcEntry>, String> {
    // 原生路径失败就退回 ps：查不到结论时不能假装「树是空的」。
    #[cfg(target_os = "macos")]
    if let Ok(entries) = snapshot_native() {
        return Ok(entries);
    }
    snapshot_ps()
}

/// macOS：原生进程表（不派生进程）。
///
/// `proc_listallpids` 列出全部 pid，`proc_pidinfo(PROC_PIDTBSDINFO)` 逐个
/// 取 ppid 与启动时间。启动时间是开机以来的秒 + 微秒，进程一生不变，
/// 因此可以作为 pid 复用的身份判据。
#[cfg(target_os = "macos")]
fn snapshot_native() -> Result<Vec<ProcEntry>, String> {
    let mut capacity = 4096usize;
    let pids = loop {
        let mut buffer = vec![0 as libc::c_int; capacity];
        // SAFETY: 缓冲区容量与传入的长度一致。
        let count = unsafe {
            libc::proc_listallpids(buffer.as_mut_ptr().cast(), (capacity * 4) as libc::c_int)
        };
        if count <= 0 {
            return Err(format!("proc_listallpids 失败（{count}）"));
        }
        let count = count as usize;
        if count < capacity {
            buffer.truncate(count);
            break buffer;
        }
        // 缓冲区可能装不下（返回值也可能直接是进程总数）：翻倍再来一次。
        capacity *= 2;
        if capacity > 1 << 20 {
            return Err("proc_listallpids 的缓冲区始终不够".to_string());
        }
    };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    let mut entries = Vec::with_capacity(pids.len());
    for pid in pids {
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        // SAFETY: info 已清零，缓冲区与长度都是该 flavor 的文档大小。
        let written = unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDTBSDINFO,
                0,
                (&mut info as *mut libc::proc_bsdinfo).cast(),
                size,
            )
        };
        if written != size {
            // 进程刚退出：这一拍看不到它，下次再看。
            continue;
        }
        entries.push(ProcEntry {
            pid: info.pbi_pid,
            ppid: info.pbi_ppid,
            zombie: info.pbi_status == libc::SZOMB,
            start: format!("{}.{}", info.pbi_start_tvsec, info.pbi_start_tvusec),
            source: ProcSource::Native,
        });
    }
    Ok(entries)
}

/// 用系统自带的 `ps` 读进程表（非 macOS 的路径，也是 macOS 原生路径失败
/// 时的兜底）。
fn snapshot_ps() -> Result<Vec<ProcEntry>, String> {
    let mut command = crate::shell::process::command_with_path("ps");
    // 固定 locale：`lstart` 的月份/星期名随 locale 变，混用会让身份核对失效。
    command.env("LC_ALL", "C");
    command.args(["-axo", "pid=,ppid=,stat=,lstart="]);
    let (success, stdout, stderr) = crate::shell::process::run_command_capture(command, "ps")
        .map_err(|error| format!("无法读取进程表：{error}"))?;
    if !success {
        return Err(format!("ps 读取进程表失败：{}", stderr.trim()));
    }
    Ok(parse_ps(&stdout))
}

/// 解析 `ps -axo pid=,ppid=,stat=,lstart=` 的输出。
///
/// `lstart` 是最后一段且本身含空格（`Thu Nov  7 12:22:34 2026`），
/// 所以按「前三个字段 + 剩下的全部」切，不按空格数切。
fn parse_ps(stdout: &str) -> Vec<ProcEntry> {
    let mut entries = Vec::new();
    for line in stdout.lines() {
        let mut parts = line.split_whitespace();
        let (Some(pid), Some(ppid), Some(stat)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        let start: Vec<&str> = parts.collect();
        let (Ok(pid), Ok(ppid)) = (pid.parse::<u32>(), ppid.parse::<u32>()) else {
            continue;
        };
        if start.is_empty() {
            continue;
        }
        entries.push(ProcEntry {
            pid,
            ppid,
            zombie: stat.starts_with('Z'),
            start: start.join(" "),
            source: ProcSource::Ps,
        });
    }
    entries
}

/// 进程表里当前仍是 `roots` 后代的 pid（沿 ppid 链走）。
fn descendants(roots: &[u32], table: &[ProcEntry]) -> Vec<u32> {
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for entry in table {
        children.entry(entry.ppid).or_default().push(entry.pid);
    }
    let mut seen: HashSet<u32> = roots.iter().copied().collect();
    let mut found = Vec::new();
    let mut stack = roots.to_vec();
    while let Some(pid) = stack.pop() {
        for child in children.get(&pid).into_iter().flatten() {
            if seen.insert(*child) {
                found.push(*child);
                stack.push(*child);
            }
        }
    }
    found
}

/// 根进程 + 已观察到的全部后代。
///
/// 进程组的收尾（TERM → 等待 → KILL）由调用方用
/// `crate::shell::process::terminate_process_group` 做；本结构负责的是
/// **脱离进程组的后代**：它们只在这里有名字。
pub(crate) struct McodeTree {
    root: u32,
    /// pid -> (来源, 启动时间)。身份对不上的 pid 一律当作已退出；
    /// **来源**不同的身份同样无法核对，按「还活着」处理。
    known: Arc<Mutex<HashMap<u32, (ProcSource, String)>>>,
    halt: Arc<AtomicBool>,
    worker: Mutex<Option<JoinHandle<()>>>,
    wake: Mutex<Option<Thread>>,
}

impl McodeTree {
    /// 开始跟踪 `root` 的后代，后台线程持续扫进程表。
    pub(crate) fn start(root: u32) -> Self {
        let known = Arc::new(Mutex::new(HashMap::new()));
        let halt = Arc::new(AtomicBool::new(false));
        let worker = {
            let known = Arc::clone(&known);
            let halt = Arc::clone(&halt);
            thread::Builder::new()
                .name("mcode-tree".to_string())
                .spawn(move || {
                    while !halt.load(Ordering::Relaxed) {
                        if let Ok(table) = snapshot() {
                            remember(&known, root, &table);
                        }
                        // park_timeout 醒来即复查，freeze() 会立刻唤醒它。
                        thread::park_timeout(POLL_INTERVAL);
                    }
                })
                .ok()
        };
        let wake = worker.as_ref().map(|worker| worker.thread().clone());
        Self {
            root,
            known,
            halt,
            worker: Mutex::new(worker),
            wake: Mutex::new(wake),
        }
    }

    /// 读一拍进程表，并吸收所有身份仍匹配的已知进程的新后代。
    fn refresh(&self) -> Result<Vec<ProcEntry>, String> {
        let table = snapshot()?;
        remember(&self.known, self.root, &table);
        Ok(table)
    }

    /// 停止前或核验监听归属时同步补拍，不等后台下一拍。
    pub(crate) fn capture(&self) -> Result<(), String> {
        self.refresh().map(|_| ())
    }

    /// 监听者可以是启动器的后代；必须同时核对已记录的启动身份。
    pub(crate) fn contains_pid(&self, pid: u32) -> Result<bool, String> {
        Ok(self.alive_pids(&self.refresh()?).contains(&pid))
    }

    /// 已记录的身份（pid -> 启动时间）。测试用。
    #[cfg(test)]
    pub(crate) fn known_start(&self, pid: u32) -> Option<String> {
        self.lock_known().get(&pid).map(|(_, start)| start.clone())
    }

    /// 已观察到的 pid（根 + 全部后代），升序。测试用。
    #[cfg(test)]
    pub(crate) fn known_pids(&self) -> Vec<u32> {
        let map = self.lock_known();
        let mut pids: Vec<u32> = map.keys().copied().collect();
        pids.sort_unstable();
        pids
    }

    /// 进程树是否已空：已知的 pid 全部不在（身份对不上即视为已退出），
    /// 且当前进程表里已经没有根进程的后代。
    pub(crate) fn empty(&self) -> Result<bool, String> {
        let table = self.refresh()?;
        Ok(self.empty_in(&table))
    }

    /// 收尾：先请已知的残留后代优雅退出（TERM），仍不退出再升级 KILL，
    /// 最后做一次终局扫描确认进程树已空。
    ///
    /// 只动**身份匹配**的 pid：启动时间对不上说明 pid 已被复用，那是别人的
    /// 进程，不能发信号。
    pub(crate) fn stop(&self) -> GroupStopOutcome {
        self.freeze();
        let mut table = match self.refresh() {
            Ok(table) => table,
            Err(reason) => return GroupStopOutcome::Unknown(reason),
        };
        let mut alive = self.alive_pids(&table);
        if !alive.is_empty() {
            for pid in &alive {
                signal(*pid, libc::SIGTERM);
            }
            self.wait_gone(&alive, TERM_GRACE);
        }
        // 停止期间后代还会派生出新进程（TERM 处理器 / hook 里 fork）：每一轮 KILL
        // 前都重新吸收一次，两轮足够覆盖「收到 TERM 后又 fork 了一个」这种一跳延迟。
        for _ in 0..2 {
            // 重新读表再升级 KILL：这一轮里 pid 可能已经被复用。
            table = match self.refresh() {
                Ok(table) => table,
                Err(reason) => return GroupStopOutcome::Unknown(reason),
            };
            alive = self.alive_pids(&table);
            if alive.is_empty() {
                break;
            }
            for pid in &alive {
                signal(*pid, libc::SIGKILL);
            }
            self.wait_gone(&alive, KILL_GRACE);
        }
        // 终局扫描：已知 pid 全部不在，且当前进程表里没有根的后代。
        let table = match self.refresh() {
            Ok(table) => table,
            Err(reason) => return GroupStopOutcome::Unknown(reason),
        };
        if self.empty_in(&table) {
            GroupStopOutcome::Stopped
        } else {
            GroupStopOutcome::StillAlive
        }
    }

    /// 已知 pid 里仍然活着的（**来源与身份都**匹配且不是僵尸）。
    ///
    /// 来源不同的那一拍不认：身份无法核对就可能发信号给别人家的进程。
    fn alive_pids(&self, table: &[ProcEntry]) -> Vec<u32> {
        let map = self.lock_known();
        let mut alive: Vec<u32> = map
            .iter()
            .filter(|(pid, (source, start))| {
                table.iter().any(|entry| {
                    entry.pid == **pid
                        && !entry.zombie
                        && entry.source == *source
                        && entry.start == **start
                })
            })
            .map(|(pid, _)| *pid)
            .collect();
        alive.sort_unstable();
        alive
    }

    /// 进程树是否已空（给定进程表）。
    ///
    /// 「来源不同」按**还活着**处理：这一拍换了一条进程表路径，身份就没法
    /// 核对了，而按「已退出」处理会让壳报「已停止」并删掉唯一能追溯那个
    /// 进程的记录——脱离进程组的 hook / 终端会变成没人认领的孤儿。
    fn empty_in(&self, table: &[ProcEntry]) -> bool {
        let map = self.lock_known();
        for (pid, (source, start)) in map.iter() {
            if let Some(entry) = table.iter().find(|entry| entry.pid == *pid) {
                if entry.zombie {
                    continue;
                }
                if entry.source == *source {
                    // 同一来源：身份能核对。对不上说明 pid 被复用，按「已退出」
                    // 处理（发信号那份判据在 `alive_pids`，它更严）。
                    if entry.start == *start {
                        return false;
                    }
                    continue;
                }
                // 来源对不上：身份无法核对，按「还活着」处理。
                return false;
            }
        }
        // 根进程即使没记到身份，只要还在跑也不算空。
        match (
            table.iter().find(|entry| entry.pid == self.root),
            map.get(&self.root),
        ) {
            (Some(entry), Some((source, start)))
                if !entry.zombie && entry.source == *source && entry.start == *start =>
            {
                return false
            }
            (Some(entry), Some((source, _))) if !entry.zombie && entry.source != *source => {
                // 来源换了一条：身份无法核对，按「还活着」处理。
                return false;
            }
            (Some(entry), None) if !entry.zombie => return false,
            _ => {}
        }
        // 当前进程表里仍有根的后代 ⇒ 有我们没记住的进程。
        descendants(&[self.root], table).is_empty()
    }

    /// 有界等待这批 pid 退出（按身份核对，不看 pid 存不存在）。
    fn wait_gone(&self, pids: &[u32], grace: Duration) {
        let deadline = Instant::now() + grace;
        loop {
            let table = match snapshot() {
                Ok(table) => table,
                // 读不到进程表就放弃等待，如实交给终局扫描判定。
                Err(_) => return,
            };
            let map = self.lock_known();
            let still_alive = pids.iter().any(|pid| {
                let Some((source, start)) = map.get(pid) else {
                    return false;
                };
                // 与 empty_in 同一口径：来源对不上时身份无法核对，按还活着处理。
                table.iter().any(|entry| {
                    entry.pid == *pid
                        && !entry.zombie
                        && (entry.source != *source || entry.start == *start)
                })
            });
            drop(map);
            if !still_alive {
                return;
            }
            if Instant::now() >= deadline {
                return;
            }
            thread::sleep(VERIFY_INTERVAL);
        }
    }

    /// 冻结后台快照：不再新增后代，线程干净退出。
    fn freeze(&self) {
        self.halt.store(true, Ordering::Relaxed);
        if let Some(wake) = self.lock_wake().as_ref() {
            wake.unpark();
        }
        if let Some(worker) = self.lock_worker().take() {
            let _ = worker.join();
        }
    }

    fn lock_known(&self) -> std::sync::MutexGuard<'_, HashMap<u32, (ProcSource, String)>> {
        self.known
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn lock_worker(&self) -> std::sync::MutexGuard<'_, Option<JoinHandle<()>>> {
        self.worker
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn lock_wake(&self) -> std::sync::MutexGuard<'_, Option<Thread>> {
        self.wake
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl Drop for McodeTree {
    fn drop(&mut self) {
        self.freeze();
    }
}

/// 把这一轮看到的、仍属于本树的进程记进名册。已有记录不覆盖：pid 被复用后，
/// 旧身份才是我们该认的那个（对不上 ⇒ 视为已退出）。
///
/// 种子是**所有身份仍匹配的已记录 pid**，不只是原始 root：后代被 init/launchd
/// 收养后已不在 root 的父链下，只从 root 追溯会漏掉它后来派生的子进程。
fn remember(known: &Mutex<HashMap<u32, (ProcSource, String)>>, root: u32, table: &[ProcEntry]) {
    let mut map = known
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut seeds: Vec<u32> = map
        .iter()
        .filter(|(pid, (source, start))| {
            table.iter().any(|entry| {
                entry.pid == **pid
                    && !entry.zombie
                    && entry.source == *source
                    && entry.start == **start
            })
        })
        .map(|(pid, _)| *pid)
        .collect();
    // 根身份已改变时不能认领当前父子链（那是别人的新进程），但其余身份仍匹配的
    // 已知进程还要继续吸收后代——它们才是被收养后仍在树里的那些。
    if !map.contains_key(&root) {
        // 第一次采样：根还没在册，按当前进程表认领它。
        if table.iter().any(|entry| entry.pid == root && !entry.zombie) {
            seeds.push(root);
        }
    }
    let mut claims = descendants(&seeds, table);
    claims.extend(seeds);
    for pid in claims {
        if let Some(entry) = table.iter().find(|entry| entry.pid == pid) {
            map.entry(pid)
                .or_insert_with(|| (entry.source, entry.start.clone()));
        }
    }
}

/// 给 pid 发信号。信号失败（进程已退出）不影响结论，终局扫描会核实。
fn signal(pid: u32, number: libc::c_int) {
    // SAFETY: pid 来自本进程自己的名册，number 是 SIGTERM / SIGKILL。
    unsafe {
        libc::kill(pid as libc::pid_t, number);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// 夹具：根进程 + 一个**脱离进程组**的孙进程。
    ///
    /// 用 `fork` 而不是 shell：`sh` 的后台任务默认还在同一个进程组里，
    /// 而这里要测的正是「进程组收尾杀不到的那一个」。孙进程 `setsid()`
    /// 自成会话，根进程退出后它会被 reparent 到 launchd。
    struct Fixture {
        root: u32,
        detached: u32,
        detached_start: String,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            // SAFETY: 都是夹具自己的 pid。
            unsafe {
                libc::kill(-(self.root as libc::pid_t), libc::SIGKILL);
            }
            // 补刀前先核对身份：pid 被复用时不能误杀别人的进程。
            if let Ok(table) = snapshot() {
                let same = table
                    .iter()
                    .any(|entry| entry.pid == self.detached && entry.start == self.detached_start);
                if same {
                    // SAFETY: 身份核对过，是夹具自己的进程。
                    unsafe {
                        libc::kill(self.detached as libc::pid_t, libc::SIGKILL);
                    }
                }
            }
            let mut status = 0;
            // SAFETY: 根进程是本测试进程的直接子进程。
            unsafe {
                libc::waitpid(self.root as libc::pid_t, &mut status, libc::WNOHANG);
            }
        }
    }

    /// 夹具：根进程派生子进程 D，D 自成会话；根进程退出后 D 被 init/launchd
    /// 收养，再从「已不在根父链下」的位置派生同样自成会话的孙进程 E。
    ///
    /// 这正是 R04 的形状：只从原始 root 追溯父链的实现会把 E 漏在名册外。
    struct ReparentFixture {
        root: u32,
        parent: u32,
        parent_start: String,
        /// E 只在根退出后出现，因此由用例读到 pid 后回填。
        child: Option<(u32, String)>,
        dir: std::path::PathBuf,
    }

    impl Drop for ReparentFixture {
        fn drop(&mut self) {
            // SAFETY: 根进程是夹具自己的会话组长。
            unsafe {
                libc::kill(-(self.root as libc::pid_t), libc::SIGKILL);
            }
            let table = snapshot().unwrap_or_default();
            // 补刀前核对身份：pid 被复用时不能误杀别人的进程。
            if table
                .iter()
                .any(|entry| entry.pid == self.parent && entry.start == self.parent_start)
            {
                // SAFETY: 身份核对过，是夹具自己的进程。
                unsafe {
                    libc::kill(self.parent as libc::pid_t, libc::SIGKILL);
                }
            }
            let child = self.child.clone().or_else(|| {
                // 用例在根退出后、回填 E 的 pid 之前失败：pid 文件是刚写下的，按文件
                // 补刀，免得留下一个睡 3600 秒的进程。
                std::fs::read_to_string(self.dir.join("child.pid"))
                    .ok()?
                    .trim()
                    .parse::<u32>()
                    .ok()
                    .map(|pid| (pid, String::new()))
            });
            if let Some((child, start)) = child {
                let owned = start.is_empty()
                    || table
                        .iter()
                        .any(|entry| entry.pid == child && entry.start == start);
                if owned {
                    // SAFETY: 身份核对过（或 pid 刚由本夹具写下），是夹具自己的进程。
                    unsafe {
                        libc::kill(child as libc::pid_t, libc::SIGKILL);
                    }
                }
            }
            let mut status = 0;
            // SAFETY: 根进程是本测试进程的直接子进程。
            unsafe {
                libc::waitpid(self.root as libc::pid_t, &mut status, libc::WNOHANG);
            }
        }
    }

    /// 进程表换了来源（原生快路径失败退回 `ps`）时，身份就没法核对了：
    /// 那时候**不能**得出「树已空」的结论——壳会据此报「已停止」并删掉唯一
    /// 能追溯那个进程的记录，脱离进程组的 hook / 终端就变成没人认领的孤儿。
    #[test]
    fn a_source_switch_never_claims_the_tree_is_empty() {
        let tree = McodeTree {
            root: 4242,
            known: Arc::new(Mutex::new(HashMap::new())),
            halt: Arc::new(AtomicBool::new(false)),
            worker: Mutex::new(None),
            wake: Mutex::new(None),
        };
        tree.lock_known()
            .insert(4242, (ProcSource::Native, "1700000000.123456".to_string()));
        // 同一拍换成了 `ps`：pid 还在，身份无法核对。
        let ps_table = vec![ProcEntry {
            pid: 4242,
            ppid: 1,
            zombie: false,
            start: "Thu Nov  7 12:22:34 2026".to_string(),
            source: ProcSource::Ps,
        }];
        assert!(
            !tree.empty_in(&ps_table),
            "来源不同时不能得出「树已空」的结论"
        );
        // 来源与身份都一致 → 确实还活着。
        let native_table = vec![ProcEntry {
            pid: 4242,
            ppid: 1,
            zombie: false,
            start: "1700000000.123456".to_string(),
            source: ProcSource::Native,
        }];
        assert!(!tree.empty_in(&native_table));
        // pid 确实不在表里 → 树空。
        assert!(tree.empty_in(&[]));
    }

    /// pid 被复用（同一来源、启动时间对不上）仍按「已退出」处理：那不是我们的
    /// 进程，但也不能因此说树不空。
    #[test]
    fn a_reused_pid_still_counts_as_gone() {
        let tree = McodeTree {
            root: 4242,
            known: Arc::new(Mutex::new(HashMap::new())),
            halt: Arc::new(AtomicBool::new(false)),
            worker: Mutex::new(None),
            wake: Mutex::new(None),
        };
        tree.lock_known()
            .insert(4242, (ProcSource::Native, "1700000000.123456".to_string()));
        let reused = vec![ProcEntry {
            pid: 4242,
            ppid: 1,
            zombie: false,
            start: "1700000999.999999".to_string(),
            source: ProcSource::Native,
        }];
        assert!(
            tree.empty_in(&reused),
            "pid 被复用后它不是我们的进程，树应当算空"
        );
    }

    #[test]
    fn reused_root_never_adopts_an_unrelated_listener() {
        let known = Mutex::new(HashMap::from([(4242, (ProcSource::Ps, "old".to_string()))]));
        let table = vec![
            ProcEntry {
                pid: 4242,
                ppid: 1,
                zombie: false,
                start: "new".into(),
                source: ProcSource::Ps,
            },
            ProcEntry {
                pid: 4243,
                ppid: 4242,
                zombie: false,
                start: "child".into(),
                source: ProcSource::Ps,
            },
        ];
        remember(&known, 4242, &table);
        assert_eq!(known.lock().unwrap().len(), 1, "不能认领复用 PID 的新后代");
    }

    /// 只认身份仍匹配的已知进程为新种子：复用 root pid 的无关进程及其子进程不能
    /// 被认领，而被收养的已知 D 的新后代必须继续入册。
    #[test]
    fn remember_reseeds_from_matching_known_pids_but_not_from_a_reused_root() {
        let known = Mutex::new(HashMap::from([
            (4242, (ProcSource::Ps, "old-root".to_string())),
            (4300, (ProcSource::Ps, "kept".to_string())),
        ]));
        let entry = |pid: u32, ppid: u32, start: &str| ProcEntry {
            pid,
            ppid,
            zombie: false,
            start: start.to_string(),
            source: ProcSource::Ps,
        };
        let table = vec![
            entry(4242, 1, "reused-root"),
            entry(4243, 4242, "unrelated-child"),
            entry(4300, 1, "kept"),
            entry(4301, 4300, "kept-child"),
        ];
        remember(&known, 4242, &table);
        let map = known.lock().unwrap();
        assert!(
            map.contains_key(&4301),
            "身份匹配的已知进程的新后代必须被吸收"
        );
        assert!(
            !map.contains_key(&4243),
            "不能认领复用 pid 的无关进程的后代"
        );
    }

    #[test]
    fn capture_records_the_listener_before_the_next_background_poll() {
        let dir = std::env::temp_dir().join(format!(
            "xlink-mcode-capture-test-{:032x}",
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let _home = crate::tests::scoped_xlink_home(&dir);
        let fixture = spawn_fixture(&dir);
        // 不起后台线程，证明归属核验不依赖下一次轮询。
        let tree = McodeTree {
            root: fixture.root,
            known: Arc::new(Mutex::new(HashMap::new())),
            halt: Arc::new(AtomicBool::new(false)),
            worker: Mutex::new(None),
            wake: Mutex::new(None),
        };
        assert!(tree.contains_pid(fixture.detached).unwrap());
        assert!(!tree.contains_pid(std::process::id()).unwrap());
        tree.capture().unwrap();
        assert!(tree.known_pids().contains(&fixture.root));
        drop(fixture);
        drop(_home);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn detached_grandchild_is_tracked_stopped_and_unrelated_processes_survive() {
        let dir = std::env::temp_dir().join(format!(
            "xlink-mcode-tree-test-{:032x}",
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&dir).expect("temporary fixture directory");
        let _home = crate::tests::scoped_xlink_home(&dir);
        let fixture = spawn_fixture(&dir);
        // 无关进程：一个独立的 sleep，停止之后必须还活着。
        let mut unrelated = crate::shell::process::command_with_path("sleep")
            .arg("300")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn the unrelated process");

        let tree = McodeTree::start(fixture.root);
        // 跟踪器必须记住那个脱离进程组的孙进程。
        let deadline = Instant::now() + Duration::from_secs(10);
        while !tree.known_pids().contains(&fixture.detached) {
            assert!(
                Instant::now() < deadline,
                "the tracker did not record the detached grandchild"
            );
            thread::sleep(Duration::from_millis(50));
        }
        // 原生快路径与 ps 必须看到同一件事：孙进程的父进程是根进程。
        let native = snapshot().expect("read the process table");
        let listed = snapshot_ps().expect("read the process table with ps");
        for table in [&native, &listed] {
            let entry = table
                .iter()
                .find(|entry| entry.pid == fixture.detached)
                .expect("the detached grandchild is missing from the process table");
            assert_eq!(entry.ppid, fixture.root);
        }
        // 身份核对：名册里的启动时间必须与系统进程表当下报的一致。
        let observed = native
            .iter()
            .find(|entry| entry.pid == fixture.detached)
            .map(|entry| entry.start.clone());
        assert_eq!(observed.as_deref(), Some(fixture.detached_start.as_str()));
        assert_eq!(tree.known_start(fixture.detached), observed);
        // 无关进程不在名册里。
        assert!(!tree.known_pids().contains(&unrelated.id()));

        // 进程组收尾：杀得到根进程，杀不到脱离进程组的孙进程。
        // SAFETY: 根进程是夹具自己的会话组长。
        unsafe {
            libc::kill(-(fixture.root as libc::pid_t), libc::SIGTERM);
        }
        thread::sleep(Duration::from_millis(200));
        // SAFETY: 同上。
        unsafe {
            libc::kill(-(fixture.root as libc::pid_t), libc::SIGKILL);
        }
        let mut status = 0;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            // SAFETY: 根进程是本测试进程的直接子进程。
            let reaped =
                unsafe { libc::waitpid(fixture.root as libc::pid_t, &mut status, libc::WNOHANG) };
            if reaped == fixture.root as libc::pid_t {
                break;
            }
            assert!(Instant::now() < deadline, "the fixture root did not exit");
            thread::sleep(Duration::from_millis(20));
        }
        assert!(
            process_alive(fixture.detached),
            "the detached grandchild must survive the process group stop"
        );

        // 跟踪器收尾：残留后代逐个清掉，并如实给出结论。
        assert_eq!(tree.stop(), GroupStopOutcome::Stopped);
        let deadline = Instant::now() + Duration::from_secs(5);
        while process_alive(fixture.detached) {
            assert!(
                Instant::now() < deadline,
                "the detached grandchild is still alive after the stop"
            );
            thread::sleep(Duration::from_millis(50));
        }
        // 无关进程不许被牵连。
        assert!(
            unrelated
                .try_wait()
                .expect("query the unrelated process")
                .is_none(),
            "an unrelated process was terminated"
        );
        let _ = unrelated.kill();
        let _ = unrelated.wait();

        drop(_home);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// R04：已被跟踪、随后被 init/launchd 收养的 D 再派生 E 时，新采样必须
    /// 以 D 为种子继续吸收后代；停止要真的带走 E，无关进程不受影响。
    #[test]
    fn a_reparented_known_descendant_keeps_tracking_its_new_children() {
        let dir = std::env::temp_dir().join(format!(
            "xlink-mcode-reparent-test-{:032x}",
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&dir).expect("temporary fixture directory");
        let _home = crate::tests::scoped_xlink_home(&dir);
        let mut fixture = spawn_reparent_fixture(&dir);
        // 无关进程：一个独立的 sleep，停止之后必须还活着。
        let mut unrelated = crate::shell::process::command_with_path("sleep")
            .arg("300")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn the unrelated process");
        let tree = McodeTree::start(fixture.root);

        // 缺陷前提：D 已经在名册里。
        let deadline = Instant::now() + Duration::from_secs(10);
        while !tree.known_pids().contains(&fixture.parent) {
            assert!(
                Instant::now() < deadline,
                "the tracker did not record the reparented descendant"
            );
            thread::sleep(Duration::from_millis(50));
        }
        // 根退出：D 从此不在根的父链下，随后派生 E。
        // SAFETY: 根进程是夹具自己的直接子进程。
        unsafe {
            libc::kill(fixture.root as libc::pid_t, libc::SIGKILL);
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let mut status = 0;
            // SAFETY: 同上。
            let reaped =
                unsafe { libc::waitpid(fixture.root as libc::pid_t, &mut status, libc::WNOHANG) };
            if reaped == fixture.root as libc::pid_t {
                break;
            }
            assert!(Instant::now() < deadline, "the fixture root did not exit");
            thread::sleep(Duration::from_millis(20));
        }
        let child = wait_for_pid(&dir.join("child.pid"));
        let start = snapshot()
            .expect("read the process table")
            .into_iter()
            .find(|entry| entry.pid == child)
            .map(|entry| entry.start)
            .expect("the reparented descendant's child is missing from the process table");
        fixture.child = Some((child, start));
        // 等若干采样：只从原始 root 追溯的实现永远看不到 E。
        let deadline = Instant::now() + Duration::from_secs(5);
        while !tree.known_pids().contains(&child) {
            assert!(
                Instant::now() < deadline,
                "the child spawned after reparenting was never tracked"
            );
            thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(tree.stop(), GroupStopOutcome::Stopped);
        let deadline = Instant::now() + Duration::from_secs(5);
        while process_alive(child) {
            assert!(
                Instant::now() < deadline,
                "the newly spawned descendant survived the stop"
            );
            thread::sleep(Duration::from_millis(50));
        }
        // 无关进程不许被牵连。
        assert!(
            unrelated
                .try_wait()
                .expect("query the unrelated process")
                .is_none(),
            "an unrelated process was terminated"
        );
        let _ = unrelated.kill();
        let _ = unrelated.wait();

        drop(_home);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 起一个根进程；它的子进程 D 自成会话，等根退出（被 reparent）后再派生
    /// 一个同样自成会话的孙进程 E。
    fn spawn_reparent_fixture(dir: &Path) -> ReparentFixture {
        let parent_file = dir.join("parent.pid");
        let child_file = dir.join("child.pid");
        let parent_fd = open_pid_file(&parent_file);
        let child_fd = open_pid_file(&child_file);
        // SAFETY: fork 之后子进程只调用异步信号安全的函数。
        let root = unsafe { libc::fork() };
        assert!(root >= 0, "cannot fork the fixture");
        if root == 0 {
            redirect_to_null();
            // SAFETY: setsid 只影响自己的会话。
            unsafe {
                libc::setsid();
            }
            // SAFETY: 同上。
            let parent = unsafe { libc::fork() };
            if parent == 0 {
                // SAFETY: 同上。
                unsafe {
                    libc::setsid();
                }
                // SAFETY: 读自己的 pid 无副作用。
                let (parent_pid, root_pid) = unsafe { (libc::getpid(), libc::getppid()) };
                write_pid(parent_fd, parent_pid);
                // 等原始父进程退出：之后本进程被 init/launchd 收养。
                while unsafe { libc::getppid() } == root_pid {
                    // SAFETY: 纯等待。
                    unsafe {
                        libc::sleep(1);
                    }
                }
                // SAFETY: 同上。
                let child = unsafe { libc::fork() };
                if child == 0 {
                    // SAFETY: 自身不是组长，setsid 会成功。
                    unsafe {
                        libc::setsid();
                    }
                    // SAFETY: 读自己的 pid 无副作用。
                    let pid = unsafe { libc::getpid() };
                    write_pid(child_fd, pid);
                    loop {
                        // SAFETY: 纯等待。
                        unsafe {
                            libc::sleep(3600);
                        }
                    }
                }
                loop {
                    // SAFETY: 纯等待。
                    unsafe {
                        libc::sleep(3600);
                    }
                }
            }
            loop {
                // SAFETY: 纯等待。
                unsafe {
                    libc::sleep(3600);
                }
            }
        }
        // SAFETY: 父进程用不到这两个 fd。
        unsafe {
            libc::close(parent_fd);
            libc::close(child_fd);
        }
        let parent = wait_for_pid(&parent_file);
        let parent_start = snapshot()
            .expect("read the process table")
            .into_iter()
            .find(|entry| entry.pid == parent)
            .map(|entry| entry.start)
            .expect("the reparented descendant is missing from the process table");
        ReparentFixture {
            root: root as u32,
            parent,
            parent_start,
            child: None,
            dir: dir.to_path_buf(),
        }
    }

    /// 打开夹具的 pid 文件。不用 O_CLOEXEC：fork 出来的子进程要继承它。
    fn open_pid_file(path: &Path) -> libc::c_int {
        let path =
            std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).expect("fixture path");
        // SAFETY: 打开自己的临时目录下的文件。
        let fd = unsafe {
            libc::open(
                path.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC,
                0o600,
            )
        };
        assert!(fd >= 0, "cannot open the fixture pid file");
        fd
    }

    /// 起一个根进程，它派生一个 `setsid()` 自成会话的孙进程。
    fn spawn_fixture(dir: &Path) -> Fixture {
        let detached_file = dir.join("detached.pid");
        let fd = open_pid_file(&detached_file);
        // SAFETY: fork 之后子进程只调用异步信号安全的函数。
        let root = unsafe { libc::fork() };
        assert!(root >= 0, "cannot fork the fixture");
        if root == 0 {
            redirect_to_null();
            // SAFETY: setsid 只影响自己的会话。
            unsafe {
                libc::setsid();
            }
            // SAFETY: 同上。
            let detached = unsafe { libc::fork() };
            if detached == 0 {
                // SAFETY: 自身不是组长，setsid 会成功。
                unsafe {
                    libc::setsid();
                }
                // SAFETY: 读自己的 pid 无副作用。
                let pid = unsafe { libc::getpid() };
                write_pid(fd, pid);
                loop {
                    // SAFETY: 纯等待。
                    unsafe {
                        libc::sleep(3600);
                    }
                }
            }
            // 根进程：只负责活着，并给孙进程当父进程。
            loop {
                // SAFETY: 纯等待。
                unsafe {
                    libc::sleep(3600);
                }
            }
        }
        // SAFETY: 父进程用不到这个 fd。
        unsafe {
            libc::close(fd);
        }
        let detached = wait_for_pid(&detached_file);
        let detached_start = snapshot()
            .expect("read the process table")
            .into_iter()
            .find(|entry| entry.pid == detached)
            .map(|entry| entry.start)
            .expect("the detached grandchild is missing from the process table");
        Fixture {
            root: root as u32,
            detached,
            detached_start,
        }
    }

    /// 等夹具把孙进程的 pid 写进文件（有界）。
    fn wait_for_pid(path: &Path) -> u32 {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Ok(text) = std::fs::read_to_string(path) {
                if let Some(digits) = text.strip_suffix('\n') {
                    if let Ok(pid) = digits.parse::<u32>() {
                        return pid;
                    }
                }
            }
            assert!(
                Instant::now() < deadline,
                "the fixture did not report its child"
            );
            thread::sleep(Duration::from_millis(20));
        }
    }

    /// 写 pid 到 fd。不分配内存：`fork` 之后调用分配函数可能撞上别的线程
    /// 持有的 malloc 锁。
    fn write_pid(fd: libc::c_int, pid: libc::pid_t) {
        let mut digits = [0u8; 12];
        let mut value = pid as u32;
        let mut index = digits.len();
        loop {
            index -= 1;
            digits[index] = b'0' + (value % 10) as u8;
            value /= 10;
            if value == 0 {
                break;
            }
        }
        let mut line = [0u8; 13];
        let length = digits.len() - index;
        line[..length].copy_from_slice(&digits[index..]);
        line[length] = b'\n';
        // SAFETY: fd 是自己打开的，缓冲区在调用期间有效。
        unsafe {
            libc::write(fd, line.as_ptr().cast(), length + 1);
        }
    }

    /// 把标准输入输出接到 /dev/null：夹具进程不该持有 cargo 的输出管道。
    fn redirect_to_null() {
        // SAFETY: 打开 /dev/null 并复制到标准 fd。
        unsafe {
            let null = libc::open(c"/dev/null".as_ptr(), libc::O_RDWR);
            if null >= 0 {
                libc::dup2(null, 0);
                libc::dup2(null, 1);
                libc::dup2(null, 2);
                if null > 2 {
                    libc::close(null);
                }
            }
        }
    }

    /// pid 是否还在（`kill(pid, 0)`）。
    fn process_alive(pid: u32) -> bool {
        // SAFETY: 只做存在性探测。
        unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
    }
}
