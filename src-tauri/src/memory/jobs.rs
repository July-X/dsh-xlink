//! 自动提取（P2）的**任务表与租约**：谁在跑、跑到哪、过期了归谁。
//!
//! ## 为什么单独一份文件，而不是塞进 `memory.json`
//!
//! §4.1 给这件事留了口子（「那时可以给文档加一个只追加的 `jobs` 段，或换实现」）。选择换承载：
//! 租约是**运行态**——每次领取、续期、完成都要改写一遍，而权威记录那份文档的价值恰恰在于
//! 「小、可 CAS、代次稳定」。混在一起的话，一次租约续期就会重写整份记忆，并让 `generation`
//! 抖动，而 `generation` 是注入缓存的失效键（`plugins/memory/host/policy.js` 的
//! `shouldProbeLibrary`）——等于每次后台任务都让运行侧白刷一次上下文。
//!
//! 两条边界写死在这里：**不记正文**（提取结果走 `candidates`，这里只有键、状态与错误码）、
//! **进程内一把锁**（跨进程并发留给将来真需要时再评估，见 A16）。
//!
//! ## 时间为什么用 epoch 毫秒
//!
//! 租约要算 deadline（`now + 120s`）。`store` 那边的时间戳是给人看的 RFC3339，做算术得先解析、
//! 还会在夏令时/闰秒上绕一圈。这里所有判断都吃 `now_ms: u64`，纯函数、可测、不碰系统时钟——
//! **测试因此能精确构造「过期了」「还差 1 毫秒」这两种边界**，而用真实时钟只能靠 sleep 撞运气。

use serde::{Deserialize, Serialize};

use crate::shell::error::AppError;

/// 等待队列上限（§8 的建议初始预算）。到顶后**丢最老的待跑任务**而不是拒收新的：同一个会话
/// 的连续回合本来就要合并成「最新的那一个」，被丢掉的那些已经被更新的截止游标覆盖了。
pub const QUEUE_CAP: usize = 20;
/// 连续回合去抖窗口：这段时间内同一会话的新任务**合并**进还没开始的那个，而不是各排一个。
pub const DEBOUNCE_MS: u64 = 30_000;
/// 一次租约的时长。执行 deadline 120 秒，超时未续期即可被下一个领取者接管。
pub const LEASE_MS: u64 = 120_000;
/// 尝试次数上限：首次 + 瞬态失败重试两次（§8）。到顶后进 `failed`，不再自动重排。
pub const MAX_ATTEMPTS: u32 = 3;

/// 任务状态。
pub const JOB_PENDING: &str = "pending";
pub const JOB_RUNNING: &str = "running";
pub const JOB_DONE: &str = "done";
pub const JOB_FAILED: &str = "failed";

/// 任务键：**同一件事只能有一个任务**（§8-2）。
///
/// `cursor` 是来源截止序号（冻结输入的那一刀），`extractor` 是提取器版本——换了提取器等于换了
/// 任务，旧结果不许覆盖新结果（§8-6）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobKey {
    pub session: String,
    pub cursor: u64,
    pub extractor: String,
}

/// 一次租约：令牌 + 到期时刻。**只有当前令牌能提交**（§8-6）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lease {
    pub token: String,
    pub expires_at_ms: u64,
}

/// 一个提取任务。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub key: JobKey,
    pub state: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    /// 已经开始的次数（含当前这一次）。
    pub attempts: u32,
    /// 正在跑时的租约；`None` 表示还没被领取。
    pub lease: Option<Lease>,
    /// 失败原因（给「查看日志」与面板看，**不含模型输入**）。
    pub error: String,
}

impl Job {
    fn new(key: JobKey, now_ms: u64) -> Self {
        Self {
            key,
            state: JOB_PENDING.to_string(),
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
            attempts: 0,
            lease: None,
            error: String::new(),
        }
    }
}

/// 任务表。每命名空间一份（与 `memory.json` 同目录的 `jobs.json`）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct JobsDoc {
    pub schema_version: u32,
    /// 按插入顺序排列——「丢最老的」因此是 `remove(0)`，不需要再排序。
    #[serde(default)]
    pub jobs: Vec<Job>,
}

/// 本版 `jobs.json` 的结构版本。读不认识的版本时**拒绝写入**（与 `memory.json` 同一条纪律：
/// 降级会让新版本写下的字段静默丢失）。
pub const JOBS_SCHEMA_VERSION: u32 = 1;

impl JobsDoc {
    /// 空表（结构版本取本版）。
    pub fn new() -> Self {
        Self {
            schema_version: JOBS_SCHEMA_VERSION,
            jobs: Vec::new(),
        }
    }
}

/// 任务表文件名，与权威记录**同目录**。
const JOBS_FILE: &str = "jobs.json";

fn jobs_path(ns: &super::store::Namespace) -> std::path::PathBuf {
    ns.dir().join(JOBS_FILE)
}

/// 当前时刻（epoch 毫秒）。租约的 deadline 用毫秒算（见模块头：为什么不做 RFC3339 算术）。
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_millis() as u64)
        .unwrap_or(0)
}

/// 进程内的那一把锁。
///
/// `claim` 是「读-改-写」：两个线程同时领，就会各自读到同一份表、各自写回，后写的把先写的
/// 租约抹掉——症状是**两个任务同时在跑**（而设计要的是每命名空间一个），或者两个调用者
/// 拿到同一个令牌。跨**进程**的并发不在这一把锁的范围里（A16 留给 P2 之后评估）。
fn lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 一次事务：同一把锁里读表 → 改表 → 写回。**租约的每个操作都必须走它**。
pub fn mutate<T>(
    ns: &super::store::Namespace,
    action: impl FnOnce(&mut JobsDoc, u64) -> T,
) -> Result<T, AppError> {
    let _guard = lock();
    // 跨进程那层：租约也存在盘上，所以它保护不了跨进程的「读-改-写」（见 `memory::lock`）。
    let _cross = crate::memory::lock::WriteLock::new(&ns.dir())?;
    let mut doc = load(ns)?;
    let now = now_ms();
    let out = action(&mut doc, now);
    persist(ns, &doc)?;
    Ok(out)
}

/// 按令牌找**当前在跑**的那个任务。提交时用它判「这个令牌还算数吗」：
/// 任务已经被接管、已完成、已失败，或者令牌根本不对，都返回 `None`（§8-6 的迟到结果检查）。
pub fn running_job<'a>(doc: &'a JobsDoc, token: &str) -> Option<&'a Job> {
    if token.is_empty() {
        return None;
    }
    doc.jobs.iter().find(|job| {
        job.state == JOB_RUNNING && job.lease.as_ref().is_some_and(|lease| lease.token == token)
    })
}

/// 领取成功时的回执：令牌 + 这一轮要做什么。
#[derive(Debug, Clone, Serialize)]
pub struct Claimed {
    pub token: String,
    /// 键里的游标就是**冻结输入的截止序号**（§8-1）。
    pub key: JobKey,
    pub attempts: u32,
    pub expires_at_ms: u64,
}

/// 领一个任务。令牌**由这一侧生成**：调用方（插件）只需要带着它回来，不必自己保证唯一。
/// 领一个任务并回执；`due_only` 为真时**只领过了静默期的**（路由走这条）。
///
/// 为什么需要它（独立评审第 11 条）：去抖原先只作用于 `enqueue` 的**合并**，`claim` 没有任何
/// 时间判据——于是「先排、再领」在稳定态等于**每回合立刻一次提取**（一次全量日志读 + 一次模型
/// 调用），而注释里写的「静默期/相邻回合合并」并不成立。加上这一道之后，相邻的回合才真的会
/// 合并成一次提取：第一次 claim 发生在最后一个回合结束 **30 秒之后**。
///
/// 为什么是单独一个入口而不是改 `claim_for` 的语义：`claim_for` 是租约状态机的原语（十来个用例
/// 按绝对时间喂它），去抖是**调度策略**，混在一起会让那些用例为了凑静默期而改时间。
pub fn claim_any_due_for(
    doc: &mut JobsDoc,
    now_ms: u64,
    session: Option<&str>,
    due_only: bool,
) -> Option<Claimed> {
    if due_only && !has_due_job(doc, now_ms, session) {
        return None;
    }
    claim_any_inner(doc, now_ms, session)
}

/// 有没有**过了静默期**的待跑任务（`session` 为 `None` 时看全表）。
fn has_due_job(doc: &JobsDoc, now_ms: u64, session: Option<&str>) -> bool {
    doc.jobs.iter().any(|job| {
        job.state == JOB_PENDING
            && now_ms.saturating_sub(job.updated_at_ms) >= DEBOUNCE_MS
            && session.is_none_or(|wanted| job.key.session == wanted)
    })
}

/// 领一个任务并回执（内部实现，`session` 语义见 [`claim_for`]）。
fn claim_any_inner(doc: &mut JobsDoc, now_ms: u64, session: Option<&str>) -> Option<Claimed> {
    let token = super::store::new_id();
    let lease = claim_for(doc, now_ms, &token, session)?;
    let job = doc.jobs.iter().find(|job| {
        job.state == JOB_RUNNING && session.is_none_or(|wanted| job.key.session == wanted)
    })?;
    Some(Claimed {
        token,
        key: job.key.clone(),
        attempts: job.attempts,
        expires_at_ms: lease.expires_at_ms,
    })
}

/// 读任务表。
///
/// **读不出来不是「空表」**：退回空表会让下一次写入把用户的任务状态抹掉——崩在半路的任务
/// 于是永远留在 `running`，而新任务以为队列是空的。与 `memory.json` 同一条纪律：区分
/// 「不存在」与「损坏」，后者如实报错并让盘上那份保持原样。
pub fn load(ns: &super::store::Namespace) -> Result<JobsDoc, AppError> {
    use crate::shell::process::{read_state_file, StateRead};
    let path = jobs_path(ns);
    match read_state_file::<JobsDoc>(&path) {
        StateRead::Loaded(doc) => Ok(doc),
        StateRead::Missing => Ok(JobsDoc::new()),
        StateRead::Corrupt { reason } => Err(AppError::Memory(format!(
            "自动提取任务表读不出来：{reason}。磁盘上的文件保持原样（没有被覆盖），请先备份并检查 {}。",
            path.display()
        ))),
    }
}

/// 写任务表：0600 + 原子替换（与权威记录同一套纪律，说明见 `shell/atomic_write`）。
pub fn persist(ns: &super::store::Namespace, doc: &JobsDoc) -> Result<(), AppError> {
    ensure_writable(doc)?;
    // 目录由 `mutate` 拿的那把跨进程锁建好了（`WriteLock::new` 里 `create_dir_all`），
    // 这里不再重复一次——`jobs.rs` 在 320/320 的反棘轮线上，省下的这五行正好抵掉锁的开销。
    let path = jobs_path(ns);
    let text = serde_json::to_string_pretty(doc)
        .map_err(|error| AppError::Memory(format!("序列化任务表失败：{error}")))?;
    crate::shell::atomic_write::atomic_write_private(&path, format!("{text}\n").as_bytes())
        .map_err(|error| AppError::Memory(format!("无法写入任务表 {}：{error}", path.display())))
}

/// 入队结果，给调用方（与测试）说清发生了什么。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Enqueued {
    /// 新排了一个任务。
    Added,
    /// 合并进了还没开始的那个（去抖窗口内同一会话的连续回合）。
    Merged,
    /// 同一个键已经在跑了：什么都不做，等它出结果。
    AlreadyRunning,
    /// 同一个键已经做过（`done`/`failed`）：不重排。
    AlreadySettled,
    /// 队满，先把最老的待跑任务丢掉，再排这一个。
    Evicted,
}

impl Enqueued {
    /// 给路由与插件用的稳定字符串（`Added` / `Merged` / `AlreadyRunning` / `AlreadySettled` /
    /// `Evicted`）。用 snake_case 是为了让插件那侧不必认识 Rust 的枚举名。
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Merged => "merged",
            Self::AlreadyRunning => "already_running",
            Self::AlreadySettled => "already_settled",
            Self::Evicted => "evicted",
        }
    }
}

/// 排队一个任务。
///
/// 合并规则（§8-2「短时间内连续回合合并，只留尚未开始的最新任务，运行中的任务保留自己的冻结输入」）：
/// - 同一个键（会话 + 游标 + 提取器）**在跑** → `AlreadyRunning`（不重复排，也不打断它的冻结输入）；
/// - 同一个键已经 `done`/`failed` → `AlreadySettled`；
/// - 同一会话有**还没开始**的任务，且与现在差在 [`DEBOUNCE_MS`] 之内 → 把游标推进到新的那个
///   （`Merged`，只留最新）；
/// - 否则新排一个；队满时丢掉最老的待跑任务。
///
/// 已经 `done` 的任务**不算作可合并对象**：合并进一个跑完的任务会让它的结果永远不被重算。
pub fn enqueue(doc: &mut JobsDoc, key: JobKey, now_ms: u64) -> Enqueued {
    if let Some(job) = doc.jobs.iter_mut().find(|job| job.key == key) {
        return if job.state == JOB_RUNNING {
            Enqueued::AlreadyRunning
        } else if job.state == JOB_DONE {
            Enqueued::AlreadySettled
        } else {
            // `failed` 且还有尝试次数：重新排（`attempts` 留着，重试上限才有意义）。
            job.state = JOB_PENDING.to_string();
            job.lease = None;
            job.updated_at_ms = now_ms;
            Enqueued::Added
        };
    }

    let mergeable = doc.jobs.iter_mut().find(|job| {
        job.state == JOB_PENDING
            && job.key.session == key.session
            && job.key.extractor == key.extractor
            && now_ms.saturating_sub(job.updated_at_ms) <= DEBOUNCE_MS
    });
    if let Some(job) = mergeable {
        job.key.cursor = key.cursor;
        job.updated_at_ms = now_ms;
        return Enqueued::Merged;
    }

    let mut evicted = false;
    let pending: Vec<usize> = doc
        .jobs
        .iter()
        .enumerate()
        .filter(|(_, job)| job.state == JOB_PENDING)
        .map(|(index, _)| index)
        .collect();
    if pending.len() >= QUEUE_CAP {
        // 丢最老的**待跑**任务：它已经被更新的游标覆盖了（同一会话的连续回合）。
        doc.jobs.remove(pending[0]);
        evicted = true;
    }
    doc.jobs.push(Job::new(key, now_ms));
    if evicted {
        Enqueued::Evicted
    } else {
        Enqueued::Added
    }
}

/// 领取一个任务：每命名空间**同时只有一个在跑**（§8 的初始预算）。
///
/// 过期租约在这里被接管：`running` 且 `expires_at_ms <= now_ms` 的任务视为可领取（`attempts`
/// 已经记过，超过 [`MAX_ATTEMPTS`] 的直接判 `failed`，避免一个坏任务被无限接手）。
/// 返回 `None` 表示当前没有可领取的任务（没有待跑、或在跑的那个还没到期）。
/// 领一个任务：`session` 为 `None` 时从全表挑最早那条，带上就**只领这个会话的**。（`session` 为 `None` 时与它等价）。
///
/// 为什么需要：`/claim` 原先不带会话，于是从全表挑最早的那条 pending——而**提交时用的坐标
/// 取自发起领取的那个会话**。两个会话同跑、或某个会话留下 pending 时，A 的对话内容会带
/// **B 的项目坐标**落库（独立评审第 3 条，正是 G3 要防的形状）。
/// 领自己那条之后，「读谁的日志」与「记到哪个项目」就都对上了。
///
/// 「同一时刻只跑一个提取」这条**仍然是全局的**（上面那段在跑检查不看会话）——
/// 它保护的是模型预算，与「领谁的任务」是两件事。
pub fn claim_for(
    doc: &mut JobsDoc,
    now_ms: u64,
    token: &str,
    session: Option<&str>,
) -> Option<Lease> {
    if doc.jobs.iter().any(|job| {
        job.state == JOB_RUNNING && job.lease.as_ref().is_some_and(|l| l.expires_at_ms > now_ms)
    }) {
        return None;
    }

    // 先把过期的在跑任务处理掉：还有尝试次数就放回待跑，到顶就判失败。
    let mut released = Vec::new();
    for (index, job) in doc.jobs.iter_mut().enumerate() {
        let expired = job.state == JOB_RUNNING
            && job
                .lease
                .as_ref()
                .is_some_and(|l| l.expires_at_ms <= now_ms);
        if !expired {
            continue;
        }
        job.lease = None;
        if job.attempts >= MAX_ATTEMPTS {
            job.state = JOB_FAILED.to_string();
            job.error = format!("租约过期且已达尝试上限（{MAX_ATTEMPTS} 次）");
            job.updated_at_ms = now_ms;
        } else {
            job.state = JOB_PENDING.to_string();
            job.updated_at_ms = now_ms;
            released.push(index);
        }
    }

    let index = doc.jobs.iter().position(|job| {
        job.state == JOB_PENDING && session.is_none_or(|wanted| job.key.session == wanted)
    })?;
    let job = &mut doc.jobs[index];
    job.state = JOB_RUNNING.to_string();
    job.attempts = job.attempts.saturating_add(1);
    job.updated_at_ms = now_ms;
    let lease = Lease {
        token: token.to_string(),
        expires_at_ms: now_ms.saturating_add(LEASE_MS),
    };
    job.lease = Some(lease.clone());
    let _ = released;
    Some(lease)
}

/// 续期。只有**当前令牌**能续（过期的、被别人接管的都续不上）。
pub fn renew(doc: &mut JobsDoc, token: &str, now_ms: u64) -> bool {
    let Some(job) = doc.jobs.iter_mut().find(|job| {
        job.state == JOB_RUNNING && job.lease.as_ref().is_some_and(|lease| lease.token == token)
    }) else {
        return false;
    };
    job.lease = Some(Lease {
        token: token.to_string(),
        expires_at_ms: now_ms.saturating_add(LEASE_MS),
    });
    job.updated_at_ms = now_ms;
    true
}

/// 完成。**只有当前令牌能提交**（§8-6）：迟到的结果（别人的租约、过期的租约）一律拒绝。
pub fn finish(doc: &mut JobsDoc, token: &str, now_ms: u64) -> bool {
    let Some(job) = doc.jobs.iter_mut().find(|job| {
        job.state == JOB_RUNNING && job.lease.as_ref().is_some_and(|lease| lease.token == token)
    }) else {
        return false;
    };
    job.state = JOB_DONE.to_string();
    job.lease = None;
    job.updated_at_ms = now_ms;
    true
}

/// 失败：还有尝试次数就放回待跑（下一次领取才是重试），到顶进 `failed`。
///
/// 返回 `true` 表示**还可以重试**。错误文案里**不放模型输入**（§9：日志与状态都不含正文）。
pub fn fail(doc: &mut JobsDoc, token: &str, error: &str, now_ms: u64) -> Option<bool> {
    let job = doc.jobs.iter_mut().find(|job| {
        job.state == JOB_RUNNING && job.lease.as_ref().is_some_and(|lease| lease.token == token)
    })?;
    job.lease = None;
    job.updated_at_ms = now_ms;
    job.error = error.to_string();
    let retryable = job.attempts < MAX_ATTEMPTS;
    job.state = if retryable {
        JOB_PENDING.to_string()
    } else {
        JOB_FAILED.to_string()
    };
    Some(retryable)
}

/// 当前状态摘要（给面板与日志用）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct JobsSummary {
    pub pending: usize,
    pub running: usize,
    pub done: usize,
    pub failed: usize,
}

pub fn summary(doc: &JobsDoc) -> JobsSummary {
    let mut out = JobsSummary::default();
    for job in &doc.jobs {
        match job.state.as_str() {
            JOB_PENDING => out.pending += 1,
            JOB_RUNNING => out.running += 1,
            JOB_DONE => out.done += 1,
            JOB_FAILED => out.failed += 1,
            _ => {}
        }
    }
    out
}

/// 读不出来的结构版本：拒绝写入（同 `memory.json` 的纪律）。
pub fn ensure_writable(doc: &JobsDoc) -> Result<(), AppError> {
    if doc.schema_version > JOBS_SCHEMA_VERSION {
        return Err(AppError::Memory(format!(
            "自动提取任务表由更新版本的应用写入（jobs schema_version {}，本版只认到 \
             {JOBS_SCHEMA_VERSION}）：已拒绝写入，以免降级丢掉新字段。",
            doc.schema_version
        )));
    }
    Ok(())
}

/// **接线判据**：`jobs::mutate` 的临界区里那把跨进程锁真的拿着（摘掉接线即红）。
///
/// 任务表的写入同样要跨进程串行：租约存在盘上，它保护不了两个壳各自的「读-改-写」。
#[test]
fn jobs_mutate_holds_the_cross_process_lock() {
    let home = crate::memory::store::testkit::TempHome::new("jobs-lock-wired");
    let ns = crate::memory::store::testkit::namespace();
    let mut held = None;
    mutate(&ns, |_doc, _now| {
        held = Some(crate::memory::lock::lock_is_held(&ns.dir()));
    })
    .expect("写一次");
    assert_eq!(held, Some(true), "临界区里要拿着跨进程锁");
    assert!(
        !crate::memory::lock::lock_is_held(&ns.dir()),
        "出了临界区要释放"
    );
    drop(home);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(session: &str, cursor: u64) -> JobKey {
        JobKey {
            session: session.to_string(),
            cursor,
            extractor: "v1".to_string(),
        }
    }

    fn doc() -> JobsDoc {
        JobsDoc::new()
    }

    /// **入表后要过静默期才领得到**（独立评审第 11 条）：去抖原先只作用于 `enqueue` 的合并，
    /// `claim` 没有时间判据——于是「先排、再领」在稳定态等于**每回合立刻一次提取**，
    /// 相邻回合根本不会合并。
    #[test]
    fn a_fresh_job_is_not_claimable_until_the_debounce_has_passed() {
        let mut doc = doc();
        enqueue(&mut doc, key("s1", 10), 0);

        // 静默期内：到期才领的那条路领不到，任务仍是 pending。
        assert!(
            claim_any_due_for(&mut doc, 0, None, true).is_none(),
            "刚入表不该立刻被领走"
        );
        assert!(
            claim_any_due_for(&mut doc, DEBOUNCE_MS - 1, None, true).is_none(),
            "差 1 毫秒也不行"
        );
        assert_eq!(doc.jobs[0].state, JOB_PENDING);

        // 过了静默期：领得到，而且就是那一条。
        let claimed = claim_any_due_for(&mut doc, DEBOUNCE_MS, None, true).expect("到期该能领");
        assert_eq!(claimed.key.cursor, 10);
        finish(&mut doc, &claimed.token, DEBOUNCE_MS);

        // `due_only=false` 保留旧语义（租约状态机的用例按绝对时间喂它）。
        enqueue(&mut doc, key("s2", 20), DEBOUNCE_MS + 1);
        assert!(
            claim_any_due_for(&mut doc, DEBOUNCE_MS + 1, None, false).is_some(),
            "不带到期判据时仍可立刻领"
        );
    }

    /// `claim_any` 要带回**令牌与键**：调用方靠键里的游标去冻结输入，靠令牌回来提交。
    #[test]
    fn claiming_hands_back_a_token_and_the_frozen_cursor() {
        let mut doc = doc();
        enqueue(&mut doc, key("s1", 77), 0);
        let claimed = claim_any_due_for(&mut doc, 10, None, false).expect("claim");
        assert_eq!(claimed.key.cursor, 77, "游标就是冻结输入的截止序号");
        assert_eq!(claimed.attempts, 1);
        assert_eq!(claimed.token.len(), 32, "16 字节的随机令牌");
        assert!(finish(&mut doc, &claimed.token, 20), "拿回来的令牌能提交");
    }

    /// **只领自己会话的任务**（独立评审第 3 条）：不带会话时从全表挑最早那条，而提交用的坐标
    /// 取自发起领取的会话——两个会话同跑时，A 的对话内容会带 B 的项目坐标落库。
    #[test]
    fn claiming_scoped_to_a_session_never_takes_another_sessions_job() {
        let mut doc = doc();
        enqueue(&mut doc, key("a", 10), 0); // A 先排，所以「全表最早」是它
        enqueue(&mut doc, key("b", 20), 0);

        let claimed = claim_any_due_for(&mut doc, 10, Some("b"), false).expect("B 该领到自己那条");
        assert_eq!(claimed.key.session, "b", "领到的必须是自己的");
        assert_eq!(claimed.key.cursor, 20);

        // A 那条还等着（没被别人领走）。
        assert!(doc
            .jobs
            .iter()
            .any(|job| job.key.session == "a" && job.state == JOB_PENDING));

        finish(&mut doc, &claimed.token, 20);
        // 放掉之后 A 自己来领，拿到的才是 A 的。
        let next = claim_any_due_for(&mut doc, 30, Some("a"), false).expect("A 领自己那条");
        assert_eq!(next.key.session, "a");
        assert_eq!(next.key.cursor, 10);

        // 不带会话时仍是「全表最早」（旧行为保留给测试与旧调用点）。
        finish(&mut doc, &next.token, 40);
        enqueue(&mut doc, key("c", 30), 0);
        let any = claim_any_due_for(&mut doc, 50, None, false).expect("不带会话仍能领");
        assert_eq!(any.key.session, "c");
    }

    /// 事务：读-改-写一次完成，下一次读得到上一次的结果（崩溃恢复、跨调用都靠它）。
    #[test]
    fn a_transaction_reads_mutates_and_writes_back() {
        use crate::memory::store::testkit::{namespace, TempHome};
        let _home = TempHome::new("jobs-tx");
        let ns = namespace();
        let added = mutate(&ns, |doc, _| {
            enqueue(
                doc,
                JobKey {
                    session: "s1".to_string(),
                    cursor: 5,
                    extractor: "v1".to_string(),
                },
                0,
            )
        })
        .expect("mutate");
        assert_eq!(added, Enqueued::Added);
        let back = load(&ns).expect("load");
        assert_eq!(back.jobs.len(), 1, "事务要写回盘上");
        assert!(
            mutate(&ns, |doc, now| claim_any_due_for(doc, now, None, false))
                .expect("claim")
                .is_some()
        );
        let back = load(&ns).expect("load");
        assert_eq!(back.jobs[0].state, JOB_RUNNING);
    }

    /// 提交前的令牌检查：任务不在跑了就拿不到（迟到的结果不许落地，§8-6）。
    #[test]
    fn only_a_running_lease_can_be_looked_up_by_token() {
        let mut doc = doc();
        enqueue(&mut doc, key("s1", 10), 0);
        assert!(running_job(&doc, "nope").is_none(), "没领过就没有");
        assert!(running_job(&doc, "").is_none(), "空令牌不算");
        let claimed = claim_any_due_for(&mut doc, 10, None, false).expect("claim");
        assert!(running_job(&doc, &claimed.token).is_some());
        assert!(finish(&mut doc, &claimed.token, 20));
        assert!(
            running_job(&doc, &claimed.token).is_none(),
            "交过之后就不算数了"
        );
    }

    /// **崩溃恢复**（§10 的验收项之一，不需要模型）：壳崩在任务跑到一半，盘上留着一个 `running`
    /// 且租约已过期的任务；重启之后（新的一次事务）它必须**能被接管**，而**旧令牌交不上东西**。
    ///
    /// 这条与上面那些纯函数用例的区别在于它走**文件**：崩溃留下的东西只有盘上那一份，
    /// 用内存里的文档测不出「重启之后还认得它」。
    #[test]
    fn a_crashed_running_job_is_taken_over_after_a_restart() {
        use crate::memory::store::testkit::{namespace, TempHome};
        let _home = TempHome::new("jobs-recovery");
        let ns = namespace();

        // 手工造一份「崩在半路」的盘上状态：running + 租约早就过期。
        let crashed = JobsDoc {
            schema_version: JOBS_SCHEMA_VERSION,
            jobs: vec![Job {
                key: JobKey {
                    session: "s1".to_string(),
                    cursor: 5,
                    extractor: "v1".to_string(),
                },
                state: JOB_RUNNING.to_string(),
                created_at_ms: 0,
                updated_at_ms: 0,
                attempts: 1,
                lease: Some(Lease {
                    token: "old-token".to_string(),
                    expires_at_ms: now_ms().saturating_sub(1),
                }),
                error: String::new(),
            }],
        };
        persist(&ns, &crashed).expect("persist");

        // 重启之后第一次领取：**直接接手**，不必等满 120 秒（租约已经过期了）。
        let claimed = mutate(&ns, |doc, now| claim_any_due_for(doc, now, None, false))
            .expect("mutate")
            .expect("崩溃残留的任务要能被接管");
        assert_eq!(claimed.key.cursor, 5, "冻结的游标不变");
        assert_eq!(claimed.attempts, 2, "接手算第二次尝试");

        // 旧令牌作废：重启之后那个进程手里的令牌再也交不上东西（§8-6 的迟到结果检查）。
        let doc = load(&ns).expect("load");
        assert!(running_job(&doc, "old-token").is_none(), "旧令牌不算数了");
        let mut doc = load(&ns).expect("load");
        assert!(!finish(&mut doc, "old-token", now_ms()), "旧令牌交不上");
    }

    /// 落盘往返：崩溃恢复靠它（重启后 `running` 的任务还在，租约会过期、被别人接手）。
    #[test]
    fn a_jobs_document_round_trips_through_disk() {
        use crate::memory::store::testkit::{namespace, TempHome};
        let _home = TempHome::new("jobs-io");
        let ns = namespace();
        let mut doc = doc();
        enqueue(&mut doc, key("s1", 42), 1_000);
        assert!(claim_for(&mut doc, 1_000, "t1", None).is_some());
        persist(&ns, &doc).expect("persist");

        let back = load(&ns).expect("load");
        assert_eq!(back.jobs.len(), 1);
        assert_eq!(back.jobs[0].key.cursor, 42);
        assert_eq!(back.jobs[0].state, JOB_RUNNING);
        assert_eq!(back.jobs[0].attempts, 1);
        // 重启之后租约仍在，但要靠**到期**才能被接手（不是靠进程内状态）。
        assert!(claim_for(&mut back.clone(), 1_001, "t2", None).is_none());
        assert!(claim_for(&mut back.clone(), 1_000 + LEASE_MS, "t2", None).is_some());
    }

    /// 连续回合合并成「最新的那一个」，而不是各排一个（§8-2）。
    #[test]
    fn rapid_turns_merge_into_the_latest_cursor() {
        let mut doc = doc();
        assert_eq!(enqueue(&mut doc, key("s1", 10), 1_000), Enqueued::Added);
        assert_eq!(enqueue(&mut doc, key("s1", 20), 5_000), Enqueued::Merged);
        assert_eq!(doc.jobs.len(), 1, "合并之后只该有一个任务");
        assert_eq!(doc.jobs[0].key.cursor, 20, "留下的是最新的游标");

        // 出了去抖窗口就是新的一轮任务。
        assert_eq!(
            enqueue(&mut doc, key("s1", 30), 5_000 + DEBOUNCE_MS + 1),
            Enqueued::Added
        );
        assert_eq!(doc.jobs.len(), 2);
    }

    /// 同一个键在跑就别再排；跑完的也不重排。
    #[test]
    fn the_same_key_is_not_scheduled_twice() {
        let mut doc = doc();
        enqueue(&mut doc, key("s1", 10), 1_000);
        assert!(claim_for(&mut doc, 1_000, "t1", None).is_some());
        assert_eq!(
            enqueue(&mut doc, key("s1", 10), 2_000),
            Enqueued::AlreadyRunning
        );
        assert!(finish(&mut doc, "t1", 3_000));
        assert_eq!(
            enqueue(&mut doc, key("s1", 10), 4_000),
            Enqueued::AlreadySettled
        );
    }

    /// 每命名空间同时只有一个在跑：第二个领取拿不到东西。
    #[test]
    fn only_one_job_runs_per_namespace() {
        let mut doc = doc();
        enqueue(&mut doc, key("s1", 10), 1_000);
        enqueue(&mut doc, key("s2", 10), 1_000);
        assert!(claim_for(&mut doc, 1_000, "t1", None).is_some());
        assert!(
            claim_for(&mut doc, 1_000, "t2", None).is_none(),
            "已经有在跑的了"
        );
        assert_eq!(summary(&doc).running, 1);
    }

    /// 租约过期后被接管：**旧令牌再也交不上东西**（§8-6 的迟到结果检查）。
    #[test]
    fn an_expired_lease_is_taken_over_and_the_old_token_is_dead() {
        let mut doc = doc();
        enqueue(&mut doc, key("s1", 10), 0);
        let first = claim_for(&mut doc, 0, "t1", None).expect("first claim");
        assert_eq!(first.expires_at_ms, LEASE_MS);

        // 还没到期：别人领不到。
        assert!(claim_for(&mut doc, LEASE_MS - 1, "t2", None).is_none());
        // 到期之后可以接管。
        let second = claim_for(&mut doc, LEASE_MS, "t2", None).expect("takeover");
        assert_eq!(doc.jobs[0].attempts, 2, "接手算一次尝试");
        assert!(!renew(&mut doc, "t1", LEASE_MS + 1), "旧令牌续不上");
        assert!(!finish(&mut doc, "t1", LEASE_MS + 1), "旧令牌交不上");
        assert!(renew(&mut doc, "t2", LEASE_MS + 1));
        assert!(finish(&mut doc, "t2", LEASE_MS + 2));
        assert_eq!(second.token, "t2");
    }

    /// 过期且到尝试上限 → 判失败，不再无限被接手（否则一个坏任务会永远占着那一个名额）。
    #[test]
    fn an_expired_job_gives_up_after_the_attempt_limit() {
        let mut doc = doc();
        enqueue(&mut doc, key("s1", 10), 0);
        let mut now = 0;
        for _ in 0..MAX_ATTEMPTS {
            assert!(claim_for(&mut doc, now, &format!("t{now}"), None).is_some());
            now += LEASE_MS;
        }
        assert!(
            claim_for(&mut doc, now, "last", None).is_none(),
            "到顶之后不再被领取"
        );
        assert_eq!(doc.jobs[0].state, JOB_FAILED);
        assert!(
            doc.jobs[0].error.contains("尝试上限"),
            "{}",
            doc.jobs[0].error
        );
    }

    /// 瞬态失败可以重试，到上限就停（§8 的「重试两次」）。
    #[test]
    fn transient_failures_retry_then_give_up() {
        let mut doc = doc();
        enqueue(&mut doc, key("s1", 10), 0);
        assert!(claim_for(&mut doc, 0, "t1", None).is_some());
        assert_eq!(fail(&mut doc, "t1", "桥超时", 1), Some(true));
        assert_eq!(doc.jobs[0].state, JOB_PENDING);

        assert!(claim_for(&mut doc, 2, "t2", None).is_some());
        assert_eq!(fail(&mut doc, "t2", "桥超时", 3), Some(true));
        assert!(claim_for(&mut doc, 4, "t3", None).is_some());
        assert_eq!(fail(&mut doc, "t3", "桥超时", 5), Some(false), "第三次到顶");
        assert_eq!(doc.jobs[0].state, JOB_FAILED);
        assert_eq!(
            fail(&mut doc, "t3", "再来一次", 6),
            None,
            "已经没有在跑的了"
        );
    }

    /// 队满丢最老的**待跑**任务，而不是拒收新的（新游标覆盖旧游标，这正是合并的语义）。
    #[test]
    fn a_full_queue_drops_the_oldest_pending_job() {
        let mut doc = doc();
        for index in 0..QUEUE_CAP {
            // 每个会话一个任务，且间隔超过去抖窗口，避免被合并。
            let at = (index as u64) * (DEBOUNCE_MS + 1);
            assert_eq!(
                enqueue(&mut doc, key(&format!("s{index}"), 1), at),
                Enqueued::Added
            );
        }
        assert_eq!(doc.jobs.len(), QUEUE_CAP);
        let at = (QUEUE_CAP as u64) * (DEBOUNCE_MS + 1);
        assert_eq!(enqueue(&mut doc, key("new", 1), at), Enqueued::Evicted);
        assert_eq!(doc.jobs.len(), QUEUE_CAP, "队还是满的");
        assert!(
            !doc.jobs.iter().any(|job| job.key.session == "s0"),
            "最老的那个被丢掉了"
        );
        assert!(doc.jobs.iter().any(|job| job.key.session == "new"));
    }

    /// 换了提取器版本 = 换了一个任务（旧结果不许覆盖新结果，§8-6）。
    #[test]
    fn a_new_extractor_version_is_a_different_job() {
        let mut doc = doc();
        enqueue(&mut doc, key("s1", 10), 0);
        let mut other = key("s1", 10);
        other.extractor = "v2".to_string();
        assert_eq!(enqueue(&mut doc, other, 0), Enqueued::Added);
        assert_eq!(doc.jobs.len(), 2);
    }

    /// 结构版本比本版新时拒绝写入（同 `memory.json` 的纪律）。
    #[test]
    fn a_newer_schema_refuses_writes() {
        let doc = JobsDoc {
            schema_version: JOBS_SCHEMA_VERSION + 1,
            jobs: Vec::new(),
        };
        assert!(ensure_writable(&doc).is_err());
    }
}
