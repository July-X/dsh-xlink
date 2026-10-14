//! 本地记忆的权威记录：每命名空间一份 JSON 文档，原子落盘。
//!
//! ## 为什么不是 SQLite
//!
//! 设计稿 §4.1 把「引入 SQLite」列为 P0 放行项，结论是**不引入**：本仓没有任何
//! SQLite 依赖（`Cargo.lock` 里连间接命中都没有），而 `Cargo.toml` 两处注释记着
//! 「CI 的 Intel macOS / Windows runner 上免 C 工具链构建」这条既有取舍；
//! `rusqlite` 的 `bundled` 要 C 工具链，系统库在 Windows 上还要随包带一份。
//! P1 的真实需求只有三件：单写者、事务性替换、损坏不改语义——`read_state_file`
//! + `atomic_write_private` 已经给了，加一个数据库换来的只是编译与打包风险。
//!
//! ## 单写者
//!
//! 所有写入都在壳进程内发生（面板命令走 Tauri；内核插件将来经回环桥回来，仍然
//! 落到这个进程），所以一把进程内 Mutex 串行「读-改-写」就够。写盘走
//! [`atomic_write_private`]：记忆正文是用户私有内容，临时文件从一开始就不能被
//! 同机其它用户读到。跨进程并发留给 P2 的任务租约，那时再评估。
//!
//! ## 损坏与降级
//!
//! 读不出来**不是**「空库」：把损坏退化成空文档，下一次写入就会把用户的全部记忆
//! 覆盖成空。`meta.schema_version` 高于本版时同样拒绝写入——降级迁移会让新版本写下的
//! 字段静默丢失。
//!
//! 检索与注入文本在 [`super::query`]：那是纯函数，与这里的落盘事务分开测。

use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::plugins::center::default_instance_key;
use crate::shell::atomic_write::atomic_write_private;
use crate::shell::error::AppError;
use crate::shell::paths;
use crate::shell::process::{read_state_file, StateRead};
use crate::shell::settings;

/// 文档 schema 版本。库里记的版本**高于**本值时拒绝写入（见 [`check_writable`]）。
pub const SCHEMA_VERSION: u32 = 1;

/// 记忆范围：绑定某个已登记工作区，或本实例通用的用户偏好。
pub const SCOPE_PROJECT: &str = "project";
pub const SCOPE_GENERAL: &str = "general";

/// 记录状态。删除不留墓碑：记录被移除，正文指纹进 `meta.suppressed`。
pub const STATE_ACCEPTED: &str = "accepted";
pub const STATE_DISABLED: &str = "disabled";

/// 候选状态。
pub const CANDIDATE_PENDING: &str = "pending";
pub const CANDIDATE_ACCEPTED: &str = "accepted";
pub const CANDIDATE_REJECTED: &str = "rejected";

/// 验证范围。**不是**置信分数：只区分「验证过」「没验证」「已过期」。
pub const VERIFICATION_VERIFIED: &str = "verified";
pub const VERIFICATION_UNVERIFIED: &str = "unverified";
pub const VERIFICATION_STALE: &str = "stale";

/// 来源种类。
pub const SOURCE_MANUAL: &str = "user-manual";
/// 会话来源：桥接层（`memory_propose` 工具）用它标注候选从哪个会话来。
pub const SOURCE_SESSION: &str = "session";
/// 自动提取来源（P2）。首版没有任何写入方，留着是为了让 P2 不引入新的字面量。
#[allow(dead_code, reason = "P2 自动提取的来源标记，首版不写")]
pub const SOURCE_EXTRACT: &str = "extract";

/// 每命名空间的记录上限。到顶后新建被拒绝（宁可不收，也不静默丢最老的）。
pub const MAX_MEMORIES: usize = 1000;
/// 单条正文上限（字节）。超出即拒绝——静默截断会让用户以为存下了全文。
pub const MAX_BODY_BYTES: usize = 8 * 1024;
/// 单条标题上限（字节）。**正文限了而标题不限**是一个洞：注入时表头里装着完整
/// 标题，标题一长 [`crate::memory::query`] 的正文预算就饱和到 0、整条被跳过，
/// 症状是「检索里排第一的那条就是不出现在注入里」。
pub const MAX_TITLE_BYTES: usize = 512;
/// 同源抑制指纹的保留条数。
pub const MAX_SUPPRESSED: usize = 500;
/// 候选的保留条数，超出丢最老的**已决**候选。
pub const MAX_CANDIDATES: usize = 200;
/// **待确认**候选的条数上限。到顶后拒收新的，而不是丢旧的——待确认的那些是用户
/// 还没看见的东西，静默丢掉等于模型提过就当没提过。
///
/// 与 [`MAX_MEMORIES`] 同一个理由：宁可不收，也不让人以为收下了。
pub const MAX_PENDING_CANDIDATES: usize = 200;

/// 记忆的命名空间：内核族 + 实例 + profile。
///
/// 项目不是命名空间的一部分：项目是命名空间**内部**的一个 `workspace_id`，
/// 因为「哪些项目存在」由内核的工作区登记决定，而不是由记忆库自己发明。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Namespace {
    pub family: String,
    pub instance_id: String,
    pub profile: String,
}

impl Namespace {
    /// 本壳默认实例的命名空间。**不读 `settings.current_instance_id` 做决策**
    /// （根 AGENTS.md 的纪律），走 `plugins::center::default_instance_key()`。
    pub fn current() -> Self {
        let (family, instance_id) = default_instance_key();
        let profile = settings::load_for_shell(settings::current_mode()).profile;
        Self {
            family,
            instance_id,
            profile,
        }
    }

    /// 内核侧插件用的命名空间：内核族与实例由**回环桥的绑定**给出（桥知道自己
    /// 在替哪个实例服务），profile 仍取壳设置——与面板同一份来源。插件传来的
    /// 任何坐标都不参与决定，所以模型无法把记忆写到别的实例或别的 profile。
    pub fn for_bridge(family: &str, instance_id: &str) -> Self {
        let profile = settings::load_for_shell(settings::current_mode()).profile;
        Self {
            family: family.to_string(),
            instance_id: instance_id.to_string(),
            profile,
        }
    }

    /// 展示与日志用的稳定键。
    pub fn key(&self) -> String {
        format!("{}/{}/{}", self.family, self.instance_id, self.profile)
    }

    /// `<instance_dir>/memory/<profile>/`。
    pub fn dir(&self) -> PathBuf {
        paths::instance_memory_dir(&self.family, &self.instance_id, &self.profile)
    }

    /// 权威记录文件。
    pub fn file(&self) -> PathBuf {
        paths::instance_memory_file(&self.family, &self.instance_id, &self.profile)
    }
}

/// 记忆来源。平铺结构而非带标签枚举：这是落盘格式，少一层嵌套就少一种
/// 跨版本解析失败的方式；未知字段按 serde 默认忽略。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct MemorySource {
    /// `user-manual` / `session` / `extract`。
    pub kind: String,
    /// 会话来源才有：来源会话的 id、事件起止序号与来源 revision。
    ///
    /// 字段名刻意不叫 `session_id`：前端有一条 IPC 判据按**字段名**工作
    /// （`check-invariants` 的 `ipc-fields`），而 `notify/task.rs` 里有个
    /// camelCase 改名的 `session_id`——同名会让「前端读 source.session_id」
    /// 被判成读错了别人的字段。改叫 `session` 两边都干净。
    pub session: String,
    pub start_seq: u64,
    pub end_seq: u64,
    pub revision: String,
    /// 这条来源成立的时间（RFC3339，UTC）。
    pub at: String,
}

impl MemorySource {
    /// 面板手工新建。
    pub fn manual(at: &str) -> Self {
        Self {
            kind: SOURCE_MANUAL.to_string(),
            at: at.to_string(),
            ..Self::default()
        }
    }
}

/// 一条已接受的记忆。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct MemoryRecord {
    pub id: String,
    /// `project` / `general`。
    pub scope: String,
    pub workspace_id: String,
    /// 工作区路径：`workspace_id` 缺失或换实例时的兜底匹配，也是面板要显示的东西。
    pub workspace_path: String,
    pub title: String,
    pub body: String,
    pub keywords: Vec<String>,
    /// 用户固定项：排序最优先，且不受关键词匹配影响。
    pub pinned: bool,
    pub source: MemorySource,
    /// `verified` / `unverified` / `stale`。
    pub verification: String,
    /// `accepted` / `disabled`。
    pub state: String,
    /// 比较版本后写入（CAS）用的单调版本号。
    pub revision: u64,
    pub created_at: String,
    pub updated_at: String,
    /// 最后一次**显式读取**的时间（展示不等于使用，见模块文档）。
    pub last_used_at: String,
    pub use_count: u64,
    pub recall_count: u64,
}

/// 一条待确认候选。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct MemoryCandidate {
    pub id: String,
    pub scope: String,
    pub workspace_id: String,
    pub workspace_path: String,
    pub title: String,
    pub body: String,
    pub keywords: Vec<String>,
    pub source: MemorySource,
    pub verification: String,
    /// `pending` / `accepted` / `rejected`。
    pub state: String,
    pub created_at: String,
    pub decided_at: String,
}

/// `meta` 段：schema 版本、数据代次、开关与同源抑制指纹。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct MemoryMeta {
    pub schema_version: u32,
    /// 每次成功写入 +1。运行侧缓存按它判断「读到的还是不是同一代」。
    pub generation: u64,
    /// 使用已批准的记忆（默认关闭）。
    pub use_approved: bool,
    /// 自动提取（P2，默认关闭）。
    pub auto_extract: bool,
    /// 被删除条目的正文指纹：P2 的提取任务据此拒绝「刚删掉又长出来」。
    pub suppressed: Vec<String>,
}

impl Default for MemoryMeta {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            generation: 0,
            use_approved: false,
            auto_extract: false,
            suppressed: Vec::new(),
        }
    }
}

/// 一份命名空间文档。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct MemoryDoc {
    pub meta: MemoryMeta,
    pub memories: Vec<MemoryRecord>,
    pub candidates: Vec<MemoryCandidate>,
}

/// 面板新建/编辑一条记忆的输入。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SaveInput {
    /// 空 = 新建；非空 = 按 id 更新。
    pub id: String,
    pub scope: String,
    pub workspace_id: String,
    pub workspace_path: String,
    pub title: String,
    pub body: String,
    pub keywords: Vec<String>,
    pub pinned: bool,
    /// 留空按 `verified`（这是用户自己写下的）。
    pub verification: String,
    /// 更新时的期望版本：与库里不一致就拒绝，避免旧窗口覆盖新修改。
    pub expected_revision: Option<u64>,
    pub source: Option<MemorySource>,
}

/// 插件（将来的会话提取）提交候选的输入。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct CandidateInput {
    pub scope: String,
    pub workspace_id: String,
    pub workspace_path: String,
    pub title: String,
    pub body: String,
    pub keywords: Vec<String>,
    pub source: MemorySource,
    pub verification: String,
}

/// 列表筛选。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ListFilter {
    /// 空 = 全部；否则 `project` / `general`。
    pub scope: String,
    pub include_disabled: bool,
    pub include_candidates: bool,
}

/// 面板要的状态摘要（内核没跑也能读）。
#[derive(Debug, Clone, Serialize)]
pub struct MemoryStatus {
    pub namespace: String,
    pub dir: String,
    pub file: String,
    pub schema_version: u32,
    pub generation: u64,
    pub use_approved: bool,
    pub auto_extract: bool,
    pub accepted: usize,
    pub disabled: usize,
    pub pending_candidates: usize,
    pub total_bytes: u64,
    /// 文档读不出来时的说明。**不打折**：展示的是空列表，磁盘上的文件没被动过。
    pub warning: Option<String>,
}

/// 列表视图。
/// 一次清空的结果（面板据此说清「清掉了什么」，而不是只说「已清空」）。
#[derive(Debug, Clone, Default, Serialize)]
pub struct ClearOutcome {
    pub memories: usize,
    pub candidates: usize,
    /// 这次新留下的抑制指纹条数（已有的不重复记）。
    pub suppressed: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryListView {
    pub status: MemoryStatus,
    pub memories: Vec<MemoryRecord>,
    pub candidates: Vec<MemoryCandidate>,
}

fn write_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// 中毒的锁不放弃：写入是「读-改-写」，而中毒只说明**另一个**线程 panic 过，
/// 与本文件的状态无关；把中毒当致命错误只会让记忆功能永久卡死。
fn lock() -> MutexGuard<'static, ()> {
    write_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// RFC3339（UTC）时间戳。取不到系统时间就退到纪元——时间戳不该让写入失败。
pub(crate) fn now_iso() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    OffsetDateTime::from_unix_timestamp(seconds)
        .unwrap_or(OffsetDateTime::UNIX_EPOCH)
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

pub(crate) fn new_id() -> String {
    let mut bytes = [0u8; 16];
    rand::Rng::fill_bytes(&mut rand::rng(), &mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// 正文指纹：先去空白差异，再 SHA-256。删除时记下它，P2 的提取据此拒绝再生。
pub fn fingerprint(body: &str) -> String {
    let normalized: String = body.split_whitespace().collect::<Vec<_>>().join(" ");
    let digest = Sha256::digest(normalized.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(crate) fn load(ns: &Namespace) -> Result<MemoryDoc, AppError> {
    match read_state_file::<MemoryDoc>(&ns.file()) {
        StateRead::Loaded(doc) => Ok(doc),
        StateRead::Missing => Ok(MemoryDoc::default()),
        StateRead::Corrupt { reason } => Err(AppError::Memory(format!(
            "记忆库读不出来：{reason}。磁盘上的文件保持原样（没有被覆盖），请先备份并检查 {} 再重试。",
            ns.file().display()
        ))),
    }
}

/// 展示路径的容错读：坏了也让面板打开，但把原因带出去。
fn load_for_display(ns: &Namespace) -> (MemoryDoc, Option<String>) {
    let path = ns.file();
    match read_state_file::<MemoryDoc>(&path) {
        StateRead::Loaded(doc) => (doc, None),
        StateRead::Missing => (MemoryDoc::default(), None),
        StateRead::Corrupt { reason } => (
            MemoryDoc::default(),
            Some(format!(
                "记忆库读不出来：{reason}。下面显示的是空列表，磁盘上的文件**没有**被覆盖；请备份并检查 {} 后重试。",
                path.display()
            )),
        ),
    }
}

/// 版本高于本版时拒绝写入。`0` 是「早于版本化」的写法，按当前版本对待。
fn check_writable(doc: &MemoryDoc, ns: &Namespace) -> Result<(), AppError> {
    if doc.meta.schema_version > SCHEMA_VERSION {
        return Err(AppError::Memory(format!(
            "记忆库由更新版本的应用写入（schema_version {}，本版只认到 {SCHEMA_VERSION}）：已拒绝写入，\
             以免降级丢掉新字段。请升级应用，或改用另一个实例。文件：{}",
            doc.meta.schema_version,
            ns.file().display()
        )));
    }
    Ok(())
}

fn persist(ns: &Namespace, doc: &MemoryDoc) -> Result<(), AppError> {
    let dir = ns.dir();
    fs::create_dir_all(&dir).map_err(|error| {
        AppError::Memory(format!("无法创建记忆目录 {}：{error}", dir.display()))
    })?;
    let text = serde_json::to_string_pretty(doc)
        .map_err(|error| AppError::Memory(format!("序列化记忆失败：{error}")))?;
    atomic_write_private(&ns.file(), format!("{text}\n").as_bytes()).map_err(|error| {
        AppError::Memory(format!("无法写入记忆库 {}：{error}", ns.file().display()))
    })
}

/// 「读-改-写」的唯一入口：拿锁 → 读 → 校验可写 → 改 → 代次 +1 → 原子写。
fn mutate<T>(
    ns: &Namespace,
    apply: impl FnOnce(&mut MemoryDoc) -> Result<T, AppError>,
) -> Result<T, AppError> {
    let _guard = lock();
    let mut doc = load(ns)?;
    check_writable(&doc, ns)?;
    // 整库体积的上限在这里判，不在各个写入函数里：这是唯一的写入口，而「变大且越过
    // 上限」这条判据需要改动前后的两个数（缩小永远放行——库已经超标时，删一条是
    // 唯一的出路）。每回合检索都要解析整份文档，见 `input::MAX_DOC_BYTES`。
    let before = crate::memory::input::doc_bytes(&doc);
    let outcome = apply(&mut doc)?;
    crate::memory::input::ensure_doc_growth(before, crate::memory::input::doc_bytes(&doc))?;
    doc.meta.generation = doc.meta.generation.saturating_add(1);
    persist(ns, &doc)?;
    Ok(outcome)
}

/// 由**已经读出来**的文档算状态摘要。
///
/// 单独拆出来是因为 [`list`] 自己已经读过一次了：原先 `list` 把 `status(ns)` 整个
/// 塞进 `MemoryListView`，于是面板每刷新一次、回环桥每 `memory_read` 一次，整份
/// JSON 都被多读一遍再解析一遍（满库 1000 条时是好几 MB 的重复解析）。
fn status_from(ns: &Namespace, doc: &MemoryDoc, warning: Option<String>) -> MemoryStatus {
    let path = ns.file();
    let total_bytes = fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
    MemoryStatus {
        namespace: ns.key(),
        dir: ns.dir().display().to_string(),
        file: path.display().to_string(),
        schema_version: doc.meta.schema_version,
        generation: doc.meta.generation,
        use_approved: doc.meta.use_approved,
        auto_extract: doc.meta.auto_extract,
        accepted: doc
            .memories
            .iter()
            .filter(|record| record.state == STATE_ACCEPTED)
            .count(),
        disabled: doc
            .memories
            .iter()
            .filter(|record| record.state != STATE_ACCEPTED)
            .count(),
        pending_candidates: doc
            .candidates
            .iter()
            .filter(|candidate| candidate.state == CANDIDATE_PENDING)
            .count(),
        total_bytes,
        warning,
    }
}

/// 状态与计数。展示路径**容错读**：文档坏了面板也要能打开，但必须把原因说出来。
pub fn status(ns: &Namespace) -> MemoryStatus {
    let (doc, warning) = load_for_display(ns);
    status_from(ns, &doc, warning)
}

/// 按 id 取一条记录，**不记使用**。
///
/// `include_disabled` 交给调用方定，因为两条调用方的语义相反：回环桥的 `read`
/// 路由传 `false`——用户在面板上点了「停用」，模型就不该再按 id 读到正文；
/// 面板自己看一条已停用的记录是另一条命令（`read_and_note_use`），不受这里管。
///
/// 走 [`load`] 而不是容错读：文档读不出来时**如实报错**。退成空列表会让回环桥
/// 回一句「这条记忆不属于当前项目」，把「记忆库坏了」说成「你要的那条不存在」。
pub fn find_by_id(
    ns: &Namespace,
    id: &str,
    include_disabled: bool,
) -> Result<Option<MemoryRecord>, AppError> {
    Ok(load(ns)?
        .memories
        .into_iter()
        .find(|record| record.id == id && (include_disabled || record.state == STATE_ACCEPTED)))
}

/// 列表：固定项优先，其余按最近修改倒序。
pub fn list(ns: &Namespace, filter: &ListFilter) -> MemoryListView {
    let (doc, warning) = load_for_display(ns);
    let mut memories: Vec<MemoryRecord> = doc
        .memories
        .iter()
        .filter(|record| filter.scope.is_empty() || record.scope == filter.scope)
        .filter(|record| filter.include_disabled || record.state == STATE_ACCEPTED)
        .cloned()
        .collect();
    memories.sort_by(|left, right| {
        right
            .pinned
            .cmp(&left.pinned)
            .then_with(|| right.updated_at.cmp(&left.updated_at))
            .then_with(|| left.id.cmp(&right.id))
    });
    let candidates = if filter.include_candidates {
        let mut pending: Vec<MemoryCandidate> = doc
            .candidates
            .iter()
            .filter(|candidate| candidate.state == CANDIDATE_PENDING)
            .cloned()
            .collect();
        pending.sort_by(|left, right| right.created_at.cmp(&left.created_at));
        pending
    } else {
        Vec::new()
    };
    MemoryListView {
        status: status_from(ns, &doc, warning),
        memories,
        candidates,
    }
}

/// 新建或更新一条记忆。更新走 compare-and-swap：`expected_revision` 与库里不一致
/// 就拒绝，旧窗口不会覆盖新修改。
pub fn save(ns: &Namespace, input: SaveInput) -> Result<MemoryRecord, AppError> {
    // 输入规范化走共享层：面板与候选两条写入路径必须对「什么算合法」给同一个答案。
    let clean = crate::memory::input::clean(
        crate::memory::input::Raw::from(&input),
        VERIFICATION_VERIFIED,
        "记忆",
    )?;

    mutate(ns, move |doc| {
        let now = now_iso();
        if input.id.trim().is_empty() {
            if doc.memories.len() >= MAX_MEMORIES {
                return Err(AppError::Memory(format!(
                    "这个实例的记忆已达上限 {MAX_MEMORIES} 条。请先删除不再需要的条目——\
                     这里宁可不收，也不静默丢掉更早的记录。"
                )));
            }
            let record = MemoryRecord {
                id: new_id(),
                scope: clean.scope,
                workspace_id: clean.workspace_id,
                workspace_path: clean.workspace_path,
                title: clean.title,
                body: clean.body,
                keywords: clean.keywords,
                pinned: input.pinned,
                source: input
                    .source
                    .clone()
                    .unwrap_or_else(|| MemorySource::manual(&now)),
                verification: clean.verification,
                state: STATE_ACCEPTED.to_string(),
                revision: 1,
                created_at: now.clone(),
                updated_at: now,
                ..MemoryRecord::default()
            };
            doc.memories.push(record.clone());
            // 用户自己写下了同一条，待确认里的重复候选就没有意义了（§7.2 用户优先）。
            crate::memory::duplicates::supersede_pending_duplicates(doc, &record.body, &now_iso());
            return Ok(record);
        }

        let record = doc
            .memories
            .iter_mut()
            .find(|record| record.id == input.id)
            .ok_or_else(|| {
                AppError::Memory(format!(
                    "找不到记忆 {}——它可能已被删除。请刷新列表后重试。",
                    input.id
                ))
            })?;
        if let Some(expected) = input.expected_revision {
            if expected != record.revision {
                return Err(AppError::Memory(format!(
                    "这条记忆已被改动过（当前版本 {}，你手上的是 {expected}）。\
                     请刷新后重新编辑，避免覆盖更新的内容。",
                    record.revision
                )));
            }
        }
        record.scope = clean.scope;
        record.workspace_id = clean.workspace_id;
        record.workspace_path = clean.workspace_path;
        record.title = clean.title;
        record.body = clean.body;
        record.keywords = clean.keywords;
        record.pinned = input.pinned;
        record.verification = clean.verification;
        record.updated_at = now;
        record.revision = record.revision.saturating_add(1);
        Ok(record.clone())
    })
}

/// 停用/启用一条记忆（不删除，保留来源与版本）。
pub fn set_state(ns: &Namespace, id: &str, state: &str) -> Result<MemoryRecord, AppError> {
    if state != STATE_ACCEPTED && state != STATE_DISABLED {
        return Err(AppError::Memory(format!("不认识的状态「{state}」。")));
    }
    mutate(ns, |doc| {
        let record = doc
            .memories
            .iter_mut()
            .find(|record| record.id == id)
            .ok_or_else(|| AppError::Memory(format!("找不到记忆 {id}——它可能已被删除。")))?;
        record.state = state.to_string();
        record.updated_at = now_iso();
        record.revision = record.revision.saturating_add(1);
        Ok(record.clone())
    })
}

/// 删除一条记忆：移除记录、记下正文指纹（防 P2 自动再生）、代次 +1。
pub fn delete(ns: &Namespace, id: &str) -> Result<(), AppError> {
    mutate(ns, |doc| {
        let Some(index) = doc.memories.iter().position(|record| record.id == id) else {
            return Err(AppError::Memory(format!(
                "找不到要删除的记忆 {id}——它可能已经被删过了。"
            )));
        };
        let removed = doc.memories.remove(index);
        let value = fingerprint(&removed.body);
        if !doc.meta.suppressed.contains(&value) {
            doc.meta.suppressed.push(value);
            if doc.meta.suppressed.len() > MAX_SUPPRESSED {
                doc.meta.suppressed.remove(0);
            }
        }
        Ok(())
    })
}

/// 清空一个命名空间：移除全部记录与候选，并逐条留下正文指纹。
///
/// 与循环调 [`delete`] 的差别只有「一次事务」：中间不会留下删了一半的状态，代次也只加
/// 一次（面板的缓存失效只看代次）。**指纹仍然逐条留下**——清空之后模型不该把刚被清掉的
/// 东西原样提回来（§7.3 的防再生），这条不能因为「反正都清了」而省。
///
/// 不碰导出目录：导出的 Markdown 是用户自己拿走的文件，清空不承诺清理它（面板文案要
/// 说清这一点，同 §7.3）。
pub fn clear(ns: &Namespace) -> Result<ClearOutcome, AppError> {
    mutate(ns, |doc| {
        let mut outcome = ClearOutcome {
            memories: doc.memories.len(),
            candidates: doc.candidates.len(),
            suppressed: 0,
        };
        for record in doc.memories.drain(..) {
            let value = fingerprint(&record.body);
            if !doc.meta.suppressed.contains(&value) {
                doc.meta.suppressed.push(value);
                outcome.suppressed += 1;
            }
        }
        doc.candidates.clear();
        while doc.meta.suppressed.len() > MAX_SUPPRESSED {
            doc.meta.suppressed.remove(0);
        }
        Ok(outcome)
    })
}

/// 开关：使用已批准的记忆。
pub fn set_use_approved(ns: &Namespace, enabled: bool) -> Result<(), AppError> {
    mutate(ns, |doc| {
        doc.meta.use_approved = enabled;
        Ok(())
    })
}

/// 提交一条待确认候选。模型只能走到这里——候选必须由用户批准才进权威记录。
pub fn submit_candidate(
    ns: &Namespace,
    input: CandidateInput,
) -> Result<MemoryCandidate, AppError> {
    let clean = crate::memory::input::clean(
        crate::memory::input::Raw::from(&input),
        VERIFICATION_UNVERIFIED,
        "候选",
    )?;
    let source = input.source.clone();
    mutate(ns, move |doc| {
        let now = now_iso();
        // 用户刚删掉的东西不许原样长回来（设计 §7.3）。**模型这一侧查**：
        // `memory_propose` 是首版唯一一个「内容由模型说了算」的写入方，删了再提回来
        // 就是它干的。面板那条 `save` 不查——用户自己重新写一条是恢复，不是再生。
        let value = fingerprint(&clean.body);
        if doc.meta.suppressed.contains(&value) {
            return Err(AppError::Memory(
                "这条内容刚被删除过，不再重新收它。若用户确实想恢复，请让他在管理面板里手动新建。"
                    .to_string(),
            ));
        }
        // **同一正文不重复收**（§7.2「相同规范化正文可合并来源」的首版做法）：正文指纹
        // 一样就是同一件事，再收一条只会让待确认清单与检索结果里出现两份。判据在
        // `duplicates`（那个模块管「同一件事别长成两条」）。
        if let Some(reason) = crate::memory::duplicates::duplicate_reason(doc, &clean.body) {
            return Err(AppError::Memory(reason));
        }
        // 待确认的候选到顶后**拒收新的**，而不是丢旧的（见 `MAX_PENDING_CANDIDATES`）。
        // 少了这一道，`memory_propose` 就是一条无上限的写入路径：模型在循环里调一千次，
        // 文档就长一千条，而每次调用都要把整份文档重写一遍。
        let pending = doc
            .candidates
            .iter()
            .filter(|candidate| candidate.state == CANDIDATE_PENDING)
            .count();
        if pending >= MAX_PENDING_CANDIDATES {
            return Err(AppError::Memory(format!(
                "这个实例已有 {MAX_PENDING_CANDIDATES} 条待确认候选。再提交会让用户面对一个很长的清单——\
                 请先在管理面板的「待确认」里处理掉一些。"
            )));
        }
        let candidate = MemoryCandidate {
            id: new_id(),
            scope: clean.scope,
            workspace_id: clean.workspace_id,
            workspace_path: clean.workspace_path,
            title: clean.title,
            body: clean.body,
            keywords: clean.keywords,
            source,
            verification: clean.verification,
            state: CANDIDATE_PENDING.to_string(),
            created_at: now,
            decided_at: String::new(),
        };
        doc.candidates.push(candidate.clone());
        // 只裁**已决**候选；待确认的一条都不丢（那是用户还没看见的东西）。
        let mut decided: Vec<usize> = doc
            .candidates
            .iter()
            .enumerate()
            .filter(|(_, candidate)| candidate.state != CANDIDATE_PENDING)
            .map(|(index, _)| index)
            .collect();
        while doc.candidates.len() > MAX_CANDIDATES && !decided.is_empty() {
            let index = decided.remove(0);
            doc.candidates.remove(index);
            for remaining in decided.iter_mut() {
                *remaining -= 1;
            }
        }
        Ok(candidate)
    })
}

/// 批准或拒绝一条候选。批准会转成一条权威记录（版本 1）。
/// `scope_override` 是用户在批准时**当场选的范围**（§6.1「提升前须展示正文与来源、去掉路径与
/// 客户名等私有信息，并由用户明确选择通用范围」）：`None` 就沿用候选自带的范围。传
/// [`SCOPE_GENERAL`] 时**项目坐标会被清掉**——留着它们会让这条记录带着一个永远匹配不上的
/// `workspace_id`，而 `scope` 又说它是通用的，两边对不上。
pub fn decide_candidate(
    ns: &Namespace,
    id: &str,
    accept: bool,
    scope_override: Option<&str>,
) -> Result<Option<MemoryRecord>, AppError> {
    mutate(ns, |doc| {
        let now = now_iso();
        let Some(index) = doc
            .candidates
            .iter()
            .position(|candidate| candidate.id == id)
        else {
            return Err(AppError::Memory(format!(
                "找不到候选 {id}——它可能已经处理过了。"
            )));
        };
        if doc.candidates[index].state != CANDIDATE_PENDING {
            return Err(AppError::Memory(format!(
                "候选 {id} 已经处理过了（{}）。",
                doc.candidates[index].state
            )));
        }
        let candidate = doc.candidates[index].clone();
        let chosen = scope_override.unwrap_or(candidate.scope.as_str());
        let (scope, workspace_id, workspace_path) = if chosen == SCOPE_GENERAL {
            (SCOPE_GENERAL.to_string(), String::new(), String::new())
        } else {
            (
                SCOPE_PROJECT.to_string(),
                candidate.workspace_id.clone(),
                candidate.workspace_path.clone(),
            )
        };
        if scope == SCOPE_PROJECT && workspace_id.is_empty() && workspace_path.is_empty() {
            return Err(AppError::Memory(
                "这条候选没有项目坐标，只能存成「通用偏好」（或让它在原项目里重新提一次）。"
                    .to_string(),
            ));
        }
        if !accept {
            doc.candidates[index].state = CANDIDATE_REJECTED.to_string();
            doc.candidates[index].decided_at = now;
            return Ok(None);
        }
        if doc.memories.len() >= MAX_MEMORIES {
            return Err(AppError::Memory(format!(
                "记忆已达上限 {MAX_MEMORIES} 条，无法批准更多候选。请先删除不再需要的条目。"
            )));
        }
        let record = MemoryRecord {
            id: new_id(),
            scope,
            workspace_id,
            workspace_path,
            title: candidate.title.clone(),
            body: candidate.body.clone(),
            keywords: candidate.keywords.clone(),
            source: candidate.source.clone(),
            // 用户看过正文与来源后点的「保存」——这是一次显式确认，标为已验证。
            verification: VERIFICATION_VERIFIED.to_string(),
            state: STATE_ACCEPTED.to_string(),
            revision: 1,
            created_at: now.clone(),
            updated_at: now.clone(),
            ..MemoryRecord::default()
        };
        doc.memories.push(record.clone());
        doc.candidates[index].state = CANDIDATE_ACCEPTED.to_string();
        doc.candidates[index].decided_at = now;
        // 批准之后，待确认里**同一正文**的那些重复候选一并收掉：用户刚刚说「这条要」，
        // 再让他为同一个内容点一次保存是多余的（§7.2 用户优先）。
        crate::memory::duplicates::supersede_pending_duplicates(doc, &record.body, &now_iso());
        Ok(Some(record))
    })
}

/// 读一条记录并记一次「显式使用」。
///
/// **调用方只有回环桥的 `read` 路由**（模型调 `memory_read` 工具）。检索展示不算
/// 使用；面板的列表也不计数——它在列表里就把正文画出来了，没有「点开」这个动作。
/// 面板上那个「已用 N 次」因此是「模型读过几次」。
pub fn read_and_note_use(ns: &Namespace, id: &str) -> Result<MemoryRecord, AppError> {
    mutate(ns, |doc| {
        let now = now_iso();
        let record = doc
            .memories
            .iter_mut()
            .find(|record| record.id == id)
            .ok_or_else(|| AppError::Memory(format!("找不到记忆 {id}——它可能已被删除。")))?;
        record.use_count = record.use_count.saturating_add(1);
        record.last_used_at = now;
        Ok(record.clone())
    })
}

#[cfg(test)]
pub(crate) mod testkit {
    use super::*;

    /// 每个用例一个独立临时目录 + `DSH_XLINK_HOME` 守卫，`Drop` 时删掉。
    pub(crate) struct TempHome {
        pub(crate) dir: PathBuf,
        _guard: crate::tests::EnvGuard,
    }

    impl TempHome {
        pub(crate) fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "dsh-memory-{tag}-{}-{}",
                std::process::id(),
                new_id()
            ));
            let guard = crate::tests::scoped_xlink_home(&dir);
            Self { dir, _guard: guard }
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }

    pub(crate) fn namespace() -> Namespace {
        Namespace {
            family: "dsh".to_string(),
            instance_id: "default-dev".to_string(),
            profile: "web".to_string(),
        }
    }

    pub(crate) fn project_input(workspace: &str, title: &str, body: &str) -> SaveInput {
        SaveInput {
            scope: SCOPE_PROJECT.to_string(),
            workspace_id: workspace.to_string(),
            workspace_path: format!("/tmp/{workspace}"),
            title: title.to_string(),
            body: body.to_string(),
            ..SaveInput::default()
        }
    }

    pub(crate) fn general_input(title: &str, body: &str) -> SaveInput {
        SaveInput {
            scope: SCOPE_GENERAL.to_string(),
            title: title.to_string(),
            body: body.to_string(),
            ..SaveInput::default()
        }
    }

    /// 项目候选。**必须带工作区坐标**：没有坐标的项目候选谁都检索不到
    /// （`query::scope_matches` 两项都空一律不通过），收下来只是躺在「待确认」里。
    pub(crate) fn candidate_input(title: &str, body: &str) -> CandidateInput {
        CandidateInput {
            scope: SCOPE_PROJECT.to_string(),
            workspace_id: "ws-a".to_string(),
            workspace_path: "/tmp/ws-a".to_string(),
            title: title.to_string(),
            body: body.to_string(),
            ..CandidateInput::default()
        }
    }

    pub(crate) fn read_doc(ns: &Namespace) -> MemoryDoc {
        serde_json::from_str(&fs::read_to_string(ns.file()).expect("doc")).expect("json")
    }
}

#[cfg(test)]
mod tests {
    use super::testkit::{
        candidate_input, general_input, namespace, project_input, read_doc, TempHome,
    };
    use super::*;
    use std::path::Path;

    #[test]
    fn store_lives_under_the_instance_directory() {
        let home = TempHome::new("path");
        let ns = namespace();
        assert_eq!(
            ns.file(),
            home.dir
                .join("kernels/dsh/instances/default-dev/memory/web/memory.json")
        );
        assert_eq!(ns.key(), "dsh/default-dev/web");
        assert!(!Path::new(&ns.dir()).exists(), "读路径不该创建目录");
    }

    /// 整库体积上限接在**写入口**上，而且超标之后删除仍然走得通。
    ///
    /// 上限（1 MiB）是按「每回合解析整份文档」定的。判据必须走真的 `mutate`，不能只
    /// 测那个纯函数——否则接线摘掉照样绿。库是**直接铺出来再 `persist`** 的（比循环
    /// 存一百多条快得多），模拟的是「老版本写下的」或「上限被调小」这两种真实来路。
    #[test]
    fn the_document_size_cap_is_wired_and_deleting_still_works_after_it_trips() {
        let _home = TempHome::new("doc-cap");
        let ns = namespace();
        let body = "关".repeat(2700); // 中文 3 字节/字：8100 字节，压在单条上限之内
        let mut doc = MemoryDoc::default();
        while crate::memory::input::doc_bytes(&doc) <= crate::memory::input::MAX_DOC_BYTES {
            let index = doc.memories.len();
            doc.memories.push(MemoryRecord {
                id: new_id(),
                scope: SCOPE_PROJECT.to_string(),
                workspace_id: "ws-a".to_string(),
                workspace_path: "/tmp/ws-a".to_string(),
                title: format!("第 {index} 条"),
                body: body.clone(),
                ..MemoryRecord::default()
            });
        }
        let victim = doc.memories[0].id.clone();
        persist(&ns, &doc).expect("persist oversized");

        // 1) 继续长大：拒，而且说清是总量。
        let refused = save(&ns, project_input("ws-a", "再来一条", &body)).unwrap_err();
        assert!(refused.to_string().contains("总量"), "{refused}");

        // 2) 缩小：放行——库已经超标时，删一条是用户唯一的出路。
        delete(&ns, &victim).expect("超标之后要删得掉");

        // 3) 删到上限之下之后，再存一条就该成——超标不是「只读」状态。
        while crate::memory::input::doc_bytes(&load(&ns).expect("load"))
            > crate::memory::input::MAX_DOC_BYTES
        {
            let next = load(&ns).expect("load").memories[0].id.clone();
            delete(&ns, &next).expect("delete");
        }
        save(&ns, project_input("ws-a", "删够了就能存", "短正文")).expect("删到上限之下应该又能存");
    }

    /// 手填的项目路径在**写入口**就被规范化成内核那种 realpath。
    ///
    /// 钉的是调用点而不是那个纯函数（`workspaces` 里另有它自己的用例）：规范化写好
    /// 了却没人调，是这个仓库反复栽过的一类形状——helper 有测试、接线没有，于是
    /// 「带符号链接的路径永远匹配不上」照样上线。
    #[test]
    fn a_hand_typed_path_is_normalized_at_the_write_entry() {
        let _home = TempHome::new("normalize");
        let ns = namespace();
        let real = std::env::temp_dir().join(format!("dsh-memory-norm-{}", std::process::id()));
        let _ = fs::remove_dir_all(&real);
        fs::create_dir_all(&real).expect("mkdir");
        let link = real.with_extension("link");
        let _ = fs::remove_file(&link);

        // 期望值走**同一个**规范化函数，而不是 `fs::canonicalize` 的原始结果：
        // Windows 上后者带 `\\?\` 前缀而 `normalize_path` 会剥掉它，拿它当期望就
        // 让这条用例只在 macOS 上成立（Windows CI 那个 job 只跑 `cargo check`、
        // 从不执行测试，于是它一直绿着——直到有人真的在 Windows 上跑一遍）。
        let canonical = crate::memory::workspaces::normalize_path(&real.display().to_string());

        // 两个平台都能验的那一半：已存在的目录 + 结尾斜杠 + 首尾空白 → 归到 realpath。
        let direct = save(
            &ns,
            SaveInput {
                scope: SCOPE_PROJECT.to_string(),
                workspace_path: format!("  {}//  ", real.display()),
                title: "构建命令".to_string(),
                body: "用 npm run check".to_string(),
                ..SaveInput::default()
            },
        )
        .expect("save");
        assert_eq!(direct.workspace_path, canonical, "存下来的必须是 realpath");

        // 符号链接那一半只在 unix 上成立：Windows 上建符号链接要管理员权限或开发者
        // 模式，而这条断言在那边的形状是「路径不存在时退回去尾分隔符」——那是
        // `normalize_path` 另一条分支（`workspaces` 里另有用例钉它）。
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&real, &link).expect("symlink");
            let via_link = save(
                &ns,
                SaveInput {
                    scope: SCOPE_PROJECT.to_string(),
                    workspace_path: format!("{}//", link.display()),
                    title: "构建命令".to_string(),
                    body: "用 npm run check".to_string(),
                    ..SaveInput::default()
                },
            )
            .expect("save");
            assert_eq!(
                via_link.workspace_path, canonical,
                "符号链接也要归到 realpath"
            );
            // 内核那侧按 realpath 查得到它。**这里存了两条**（直写一条、走符号链接
            // 一条），两条的 `workspace_path` 都是同一个 realpath，所以都该命中——
            // 原先断言的是 1，而它从写下那天起就是红的（两条记录、一条的期望）。
            set_use_approved(&ns, true).expect("enable");
            let hits =
                crate::memory::query::retrieve(&ns, "构建命令", "", &canonical).expect("retrieve");
            assert_eq!(
                hits.hits.len(),
                2,
                "两条记录都落在同一个 realpath 下，都该命中：{:?}",
                hits.hits.iter().map(|hit| &hit.id).collect::<Vec<_>>()
            );
            assert!(
                hits.hits
                    .iter()
                    .all(|hit| hit.body.contains("npm run check")),
                "{:?}",
                hits.hits
            );
            let _ = fs::remove_file(&link);
        }
        let _ = fs::remove_dir_all(&real);
    }

    #[test]
    fn save_then_list_round_trips() {
        let _home = TempHome::new("roundtrip");
        let ns = namespace();
        let saved = save(&ns, project_input("ws-1", "构建命令", "用 npm run check")).expect("save");
        assert_eq!(saved.revision, 1);
        assert_eq!(saved.state, STATE_ACCEPTED);
        assert_eq!(saved.source.kind, SOURCE_MANUAL);
        assert_eq!(saved.verification, VERIFICATION_VERIFIED);

        let view = list(
            &ns,
            &ListFilter {
                include_candidates: true,
                ..ListFilter::default()
            },
        );
        assert_eq!(view.memories.len(), 1);
        assert_eq!(view.memories[0].body, "用 npm run check");
        assert_eq!(view.status.accepted, 1);
        assert_eq!(view.status.generation, 1);
        assert!(view.status.warning.is_none());
        assert!(ns.file().exists());
    }

    #[test]
    fn project_scope_requires_a_workspace() {
        let _home = TempHome::new("scope-required");
        let ns = namespace();
        let error = save(
            &ns,
            SaveInput {
                scope: SCOPE_PROJECT.to_string(),
                title: "没有工作区".to_string(),
                ..SaveInput::default()
            },
        )
        .expect_err("must reject");
        assert!(error.to_string().contains("工作区"), "{error}");
    }

    #[test]
    fn general_scope_drops_workspace_fields() {
        let _home = TempHome::new("general-scope");
        let ns = namespace();
        let saved = save(&ns, general_input("偏好", "回答用中文")).expect("save");
        assert_eq!(saved.scope, SCOPE_GENERAL);
        assert!(saved.workspace_id.is_empty());
        assert!(saved.workspace_path.is_empty());
    }

    #[test]
    fn update_uses_compare_and_swap() {
        let _home = TempHome::new("cas");
        let ns = namespace();
        let saved = save(&ns, project_input("ws-1", "标题", "第一版")).expect("save");

        let stale = save(
            &ns,
            SaveInput {
                id: saved.id.clone(),
                expected_revision: Some(99),
                ..project_input("ws-1", "标题", "改坏了")
            },
        )
        .expect_err("stale revision must be rejected");
        assert!(stale.to_string().contains("已被改动过"), "{stale}");

        let updated = save(
            &ns,
            SaveInput {
                id: saved.id.clone(),
                expected_revision: Some(saved.revision),
                ..project_input("ws-1", "标题", "第二版")
            },
        )
        .expect("update");
        assert_eq!(updated.revision, 2);
        assert_eq!(read_doc(&ns).memories[0].body, "第二版");
    }

    #[test]
    fn delete_removes_the_record_and_keeps_a_fingerprint() {
        let _home = TempHome::new("delete");
        let ns = namespace();
        let saved = save(&ns, project_input("ws-1", "标题", "会被删掉")).expect("save");
        delete(&ns, &saved.id).expect("delete");

        let doc = read_doc(&ns);
        assert!(doc.memories.is_empty());
        assert_eq!(doc.meta.suppressed, vec![fingerprint("会被删掉")]);
        assert_eq!(doc.meta.generation, 2);
        assert!(delete(&ns, &saved.id).is_err(), "重复删除要如实报错");
    }

    /// 写不进去时**说得清、且不丢已有内容**（§10 的「磁盘满」那一格）。
    ///
    /// 只读目录是最接近磁盘满的模拟：`persist` 要在同一个目录里建临时文件，建不出来就是
    /// 一次写入失败。要钉两头——错误里带得上路径（用户据此去查权限/空间），以及**旧文档
    /// 一个字节没变**（原子替换的意义就在这里；「写失败把库写没了」是这类系统最坏的失败）。
    #[cfg(unix)]
    #[test]
    fn a_read_only_directory_fails_loudly_and_keeps_the_old_document() {
        use std::os::unix::fs::PermissionsExt;

        let _home = TempHome::new("read-only");
        let ns = namespace();
        let kept = save(&ns, project_input("ws-a", "写得进去的时候", "正文")).expect("save");
        let dir = ns.dir();
        let before = fs::read_to_string(ns.file()).expect("read doc");

        fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).expect("chmod");
        let failed = save(&ns, project_input("ws-a", "写不进去的时候", "正文二")).unwrap_err();
        let message = failed.to_string();
        assert!(
            message.contains(&ns.file().display().to_string()),
            "错误里要有路径，用户才知道去哪查：{message}"
        );

        // 旧文档原样还在（原子替换 + 拒绝写入，不是「先清空再写」）。
        let after = fs::read_to_string(ns.file()).expect("read doc again");
        assert_eq!(after, before);
        let view = list(&ns, &ListFilter::default());
        assert_eq!(view.memories.len(), 1);
        assert_eq!(view.memories[0].id, kept.id);

        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).expect("chmod back");
    }

    /// 并发写入不丢更新：进程内那把 Mutex 是「读-改-写」唯一的串行点（§4.1 的单写者）。
    ///
    /// 没有它时两个线程各自读到同一份文档、各自加一条、后写的覆盖先写的——症状是
    /// 「我明明存了两条，只剩一条」，而且不报任何错。跨**进程**并发是另一件事，设计上
    /// 留给 P2 的租约（清单 A16），这里只钉进程内。
    #[test]
    fn concurrent_writes_do_not_lose_each_others_update() {
        let _home = TempHome::new("concurrent");
        let ns = namespace();
        let threads = 8;
        std::thread::scope(|scope| {
            for index in 0..threads {
                let ns = ns.clone();
                scope.spawn(move || {
                    save(
                        &ns,
                        project_input(
                            "ws-a",
                            &format!("并发第 {index} 条"),
                            &format!("正文 {index}"),
                        ),
                    )
                    .expect("save");
                });
            }
        });
        let view = list(&ns, &ListFilter::default());
        assert_eq!(view.memories.len(), threads, "每一次写入都要留下");
        let mut titles: Vec<&str> = view
            .memories
            .iter()
            .map(|record| record.title.as_str())
            .collect();
        titles.sort_unstable();
        for index in 0..threads {
            let expected = format!("并发第 {index} 条");
            assert!(
                titles.contains(&expected.as_str()),
                "少了 {expected}：{titles:?}"
            );
        }
    }

    /// 清空：一次事务删干净、逐条留下抑制指纹、代次只加一次，导出目录不碰。
    #[test]
    fn clear_removes_everything_and_still_suppresses_the_bodies() {
        let _home = TempHome::new("clear");
        let ns = namespace();
        let kept = save(&ns, project_input("ws-a", "留下的", "正文一")).expect("save");
        save(&ns, general_input("清掉的偏好", "正文二")).expect("save general");
        submit_candidate(
            &ns,
            CandidateInput {
                scope: SCOPE_PROJECT.to_string(),
                workspace_path: "/tmp/ws-a".to_string(),
                title: "待确认的候选".to_string(),
                body: "正文三".to_string(),
                ..CandidateInput::default()
            },
        )
        .expect("candidate");
        let before = load(&ns).expect("load").meta.generation;

        let outcome = clear(&ns).expect("clear");
        assert_eq!(outcome.memories, 2);
        assert_eq!(outcome.candidates, 1);
        assert_eq!(outcome.suppressed, 2, "两条正文各留一枚指纹");

        let after = load(&ns).expect("load");
        assert!(after.memories.is_empty());
        assert!(after.candidates.is_empty());
        assert_eq!(after.meta.generation, before + 1, "一次事务只加一次代次");
        assert_eq!(after.meta.suppressed.len(), 2);

        // 清空之后模型不许把刚清掉的东西原样提回来（§7.3 的防再生）。
        let refused = submit_candidate(
            &ns,
            CandidateInput {
                scope: SCOPE_PROJECT.to_string(),
                workspace_path: "/tmp/ws-a".to_string(),
                title: "再提一次".to_string(),
                body: "正文二".to_string(),
                ..CandidateInput::default()
            },
        )
        .unwrap_err();
        assert!(refused.to_string().contains("删除过"), "{refused}");

        // 清空之后面板仍能用：库还是可读可写的（不是「损坏」）。
        save(&ns, project_input("ws-a", "清空之后新建", "正文四")).expect("save after clear");
        let _ = kept;
    }

    #[test]
    fn corrupt_file_is_an_error_not_a_reset() {
        let _home = TempHome::new("corrupt");
        let ns = namespace();
        fs::create_dir_all(ns.dir()).expect("dir");
        fs::write(ns.file(), b"{ this is not json").expect("write");

        let error = save(&ns, project_input("ws-a", "标题", "正文")).expect_err("must refuse");
        assert!(error.to_string().contains("记忆库读不出来"), "{error}");
        assert_eq!(
            fs::read(ns.file()).expect("read"),
            b"{ this is not json",
            "损坏文件必须原样保留"
        );

        let view = list(&ns, &ListFilter::default());
        assert!(view.memories.is_empty());
        assert!(view.status.warning.is_some(), "展示路径要把损坏说出来");
    }

    #[test]
    fn newer_schema_refuses_writes() {
        let _home = TempHome::new("schema");
        let ns = namespace();
        fs::create_dir_all(ns.dir()).expect("dir");
        let doc = serde_json::json!({
            "meta": { "schema_version": SCHEMA_VERSION + 1, "generation": 3 },
            "memories": [],
        });
        fs::write(ns.file(), serde_json::to_string_pretty(&doc).unwrap()).expect("write");

        let error = save(&ns, project_input("ws-a", "标题", "正文")).expect_err("must refuse");
        assert!(error.to_string().contains("更新版本"), "{error}");
    }

    #[test]
    fn candidate_needs_an_explicit_decision() {
        let _home = TempHome::new("candidate");
        let ns = namespace();
        let candidate = submit_candidate(
            &ns,
            CandidateInput {
                scope: SCOPE_PROJECT.to_string(),
                workspace_id: "ws-a".to_string(),
                workspace_path: "/tmp/ws-a".to_string(),
                title: "候选".to_string(),
                body: "模型提出的内容".to_string(),
                source: MemorySource {
                    kind: SOURCE_SESSION.to_string(),
                    session: "s-1".to_string(),
                    start_seq: 3,
                    end_seq: 9,
                    revision: "r1".to_string(),
                    at: now_iso(),
                },
                ..CandidateInput::default()
            },
        )
        .expect("submit");

        let view = list(
            &ns,
            &ListFilter {
                include_candidates: true,
                ..ListFilter::default()
            },
        );
        assert_eq!(view.candidates.len(), 1, "候选不能自己变成记录");
        assert!(view.memories.is_empty());
        assert_eq!(view.status.pending_candidates, 1);

        let record = decide_candidate(&ns, &candidate.id, true, None)
            .expect("decide")
            .expect("accepted record");
        assert_eq!(record.state, STATE_ACCEPTED);
        assert_eq!(record.source.session, "s-1");
        assert_eq!(record.source.start_seq, 3);
        assert!(
            decide_candidate(&ns, &candidate.id, true, None).is_err(),
            "不能重复批准"
        );
        assert_eq!(status(&ns).accepted, 1);
    }

    /// 批准时用户当场选范围（§6.1）：选「通用偏好」必须**清掉项目坐标**——留着它们会让这条
    /// 记录带着一个永远匹配不上的 `workspace_id`，而 `scope` 又说它是通用的，两边对不上。
    #[test]
    fn accepting_with_a_scope_override_moves_the_record_to_general() {
        let _home = TempHome::new("promote");
        let ns = namespace();
        let candidate = submit_candidate(
            &ns,
            CandidateInput {
                scope: SCOPE_PROJECT.to_string(),
                workspace_id: "ws-a".to_string(),
                workspace_path: "/tmp/ws-a".to_string(),
                title: "通用一点的写法".to_string(),
                body: "提交前跑检查".to_string(),
                ..CandidateInput::default()
            },
        )
        .expect("candidate");

        let record = decide_candidate(&ns, &candidate.id, true, Some(SCOPE_GENERAL))
            .expect("accept")
            .expect("record");
        assert_eq!(record.scope, SCOPE_GENERAL);
        assert!(record.workspace_id.is_empty(), "通用偏好不该带项目 id");
        assert!(record.workspace_path.is_empty(), "通用偏好不该带项目路径");
        // 通用偏好对哪个项目都算（`scope_matches` 的第一条分支）。
        let hits =
            crate::memory::query::retrieve(&ns, "提交前跑检查", "", "/anywhere").expect("retrieve");
        assert_eq!(hits.hits.len(), 0, "开关还没开");
        set_use_approved(&ns, true).expect("enable");
        let hits =
            crate::memory::query::retrieve(&ns, "提交前跑检查", "", "/anywhere").expect("retrieve");
        assert_eq!(hits.hits.len(), 1, "{:?}", hits.hits);
    }

    #[test]
    fn rejecting_a_candidate_creates_nothing() {
        let _home = TempHome::new("reject");
        let ns = namespace();
        let candidate =
            submit_candidate(&ns, candidate_input("看起来不对的结论", "正文")).expect("submit");
        assert!(decide_candidate(&ns, &candidate.id, false, None)
            .expect("decide")
            .is_none());
        assert!(status(&ns).accepted == 0);
        assert_eq!(status(&ns).pending_candidates, 0);
    }

    #[test]
    fn an_over_long_title_is_refused_like_an_over_long_body() {
        let _home = TempHome::new("title-limit");
        let ns = namespace();
        // 正文限了而标题不限是一个洞：注入表头里装着完整标题，标题一长正文预算
        // 饱和到 0、整条被跳过，症状是「排第一的那条就是不注入」。
        let long = "标".repeat(MAX_TITLE_BYTES);
        let error = save(
            &ns,
            SaveInput {
                scope: SCOPE_PROJECT.to_string(),
                workspace_id: "ws-a".to_string(),
                title: long.clone(),
                body: "正文".to_string(),
                ..SaveInput::default()
            },
        )
        .expect_err("must refuse");
        assert!(error.to_string().contains("标题"), "{error}");

        let candidate_error =
            submit_candidate(&ns, candidate_input(&long, "正文")).expect_err("候选也必须拒");
        assert!(
            candidate_error.to_string().contains("标题"),
            "{candidate_error}"
        );
        // 边界内照常放行。
        assert!(save(
            &ns,
            project_input("ws-a", &"标".repeat(MAX_TITLE_BYTES / 3), "正文")
        )
        .is_ok());
    }

    #[test]
    fn a_deleted_body_is_not_accepted_back_from_the_model() {
        let _home = TempHome::new("suppress");
        let ns = namespace();
        let saved =
            save(&ns, project_input("ws-a", "约定", "提交前跑 npm run check")).expect("save");
        delete(&ns, &saved.id).expect("delete");

        // 模型这一侧拒：设计 §7.3「相同来源不能自动再生」。
        let refused = submit_candidate(
            &ns,
            CandidateInput {
                scope: SCOPE_PROJECT.to_string(),
                workspace_id: "ws-a".to_string(),
                title: "又来了".to_string(),
                body: "提交前跑   npm run check".to_string(),
                ..CandidateInput::default()
            },
        )
        .expect_err("刚删掉的内容不许原样回来");
        assert!(refused.to_string().contains("刚被删除"), "{refused}");
        assert_eq!(status(&ns).pending_candidates, 0);

        // 面板那一侧不查：用户自己重新写一条是**恢复**，不是再生。
        assert!(save(&ns, project_input("ws-a", "约定", "提交前跑 npm run check")).is_ok());
    }

    /// 待确认候选到顶后**拒收新的**，而不是丢旧的。
    ///
    /// `memory_propose` 是模型能触达的写入路径，循环调一千次就长一千条，而每次
    /// 提交都要把整份文档重写一遍——这一道是那条路径唯一的长度闸。
    /// 同一正文不重复收：**已有记录**与**已在待确认**两条路都要挡住（§7.2 的首版做法）。
    /// 只挡精确重复——「意思相近但写法不同」需要模型比对，那是 P2（清单里的 C5）。
    #[test]
    fn the_same_body_is_not_collected_twice() {
        let _home = TempHome::new("dedup");
        let ns = namespace();
        save(
            &ns,
            project_input("ws-a", "构建前先跑检查", "提交前跑 npm run check"),
        )
        .expect("save");

        let dup_record = submit_candidate(
            &ns,
            CandidateInput {
                scope: SCOPE_PROJECT.to_string(),
                workspace_path: "/tmp/ws-a".to_string(),
                title: "换个标题也一样".to_string(),
                body: "提交前跑 npm run check".to_string(),
                ..CandidateInput::default()
            },
        )
        .unwrap_err();
        assert!(
            dup_record.to_string().contains("已经在记忆里"),
            "{dup_record}"
        );

        submit_candidate(
            &ns,
            CandidateInput {
                scope: SCOPE_PROJECT.to_string(),
                workspace_path: "/tmp/ws-a".to_string(),
                title: "部署约定".to_string(),
                body: "部署走 tag".to_string(),
                ..CandidateInput::default()
            },
        )
        .expect("first candidate");
        let dup_pending = submit_candidate(
            &ns,
            CandidateInput {
                scope: SCOPE_PROJECT.to_string(),
                workspace_path: "/tmp/ws-a".to_string(),
                title: "又说了一遍".to_string(),
                body: "部署走 tag".to_string(),
                ..CandidateInput::default()
            },
        )
        .unwrap_err();
        assert!(
            dup_pending.to_string().contains("已经在待确认里"),
            "{dup_pending}"
        );

        // 正文不同就不挡（首版没有语义判据，宁可多收一条让人自己看，也不要拿近似当重复）。
        submit_candidate(
            &ns,
            CandidateInput {
                scope: SCOPE_PROJECT.to_string(),
                workspace_path: "/tmp/ws-a".to_string(),
                title: "更细的部署约定".to_string(),
                body: "部署走 tag，之后补 changelog".to_string(),
                ..CandidateInput::default()
            },
        )
        .expect("different body");
    }

    /// 用户自己写下同一条之后，待确认里的重复候选被收掉（§7.2「用户纠正优先于旧候选」）。
    #[test]
    fn writing_the_same_body_yourself_retires_the_pending_duplicate() {
        let _home = TempHome::new("supersede");
        let ns = namespace();
        let candidate = submit_candidate(
            &ns,
            CandidateInput {
                scope: SCOPE_PROJECT.to_string(),
                workspace_path: "/tmp/ws-a".to_string(),
                title: "会话提的".to_string(),
                body: "提交前跑 npm run check".to_string(),
                ..CandidateInput::default()
            },
        )
        .expect("candidate");

        save(
            &ns,
            project_input("ws-a", "我自己写的", "提交前跑 npm run check"),
        )
        .expect("save");

        let view = list(
            &ns,
            &ListFilter {
                include_candidates: true,
                ..ListFilter::default()
            },
        );
        assert!(
            view.candidates.is_empty(),
            "重复候选该被收掉：{:?}",
            view.candidates
        );
        // **状态改成已拒绝，不是删掉**：文档里留痕，用户问「我提的那条呢」时有据可查。
        let doc = load(&ns).expect("load");
        let row = doc
            .candidates
            .iter()
            .find(|row| row.id == candidate.id)
            .expect("候选要留着");
        assert_eq!(row.state, CANDIDATE_REJECTED);
        assert!(!row.decided_at.is_empty());
    }

    #[test]
    fn pending_candidates_are_capped_by_refusing_rather_than_dropping() {
        let _home = TempHome::new("cap");
        let ns = namespace();
        for index in 0..MAX_PENDING_CANDIDATES {
            submit_candidate(
                &ns,
                candidate_input(&format!("候选 {index}"), &format!("正文 {index}")),
            )
            .expect("submit");
        }
        assert_eq!(status(&ns).pending_candidates, MAX_PENDING_CANDIDATES);

        let refused =
            submit_candidate(&ns, candidate_input("再加一条", "正文")).expect_err("到顶后拒收");
        assert!(refused.to_string().contains("待确认"), "{refused}");
        let view = list(
            &ns,
            &ListFilter {
                include_candidates: true,
                ..ListFilter::default()
            },
        );
        assert_eq!(
            view.candidates.len(),
            MAX_PENDING_CANDIDATES,
            "拒收不许顺手裁掉一条"
        );
    }

    /// 已决候选的裁剪**只动已决的那些**，一条待确认都不能少。
    ///
    /// 钉的是那段下标算术：删掉一条之后剩下的下标要整体前移一位，漏了这一步会静默
    /// 删掉一条**待确认**候选——那是用户还没看见的东西。全文件最容易写错的一段。
    ///
    /// 走正常提交路径永远到不了「一次要删两条」那个分支（每次提交都会把总数裁回
    /// 上限），所以这里直接落一份**超上限**的文档——那正是降版本或换实现之后会有的形状。
    #[test]
    fn trimming_decided_candidates_never_drops_a_pending_one() {
        let _home = TempHome::new("trim");
        let ns = namespace();
        fs::create_dir_all(ns.dir()).expect("dir");
        let total = MAX_CANDIDATES + 2;
        // 前 5 条是**已决**的：它们才是允许被裁掉的那些。留够 5 条是为了让一次提交
        // 之后要连删三条——那条「剩下的下标要整体前移」的路径只有连删时才走得到。
        let decided: usize = 5;
        let candidates: Vec<MemoryCandidate> = (0..total)
            .map(|index| MemoryCandidate {
                id: format!("c{index}"),
                title: format!("候选 {index}"),
                body: format!("正文 {index}"),
                state: if index < decided {
                    CANDIDATE_REJECTED.to_string()
                } else {
                    CANDIDATE_PENDING.to_string()
                },
                ..MemoryCandidate::default()
            })
            .collect();
        let mut doc = MemoryDoc::default();
        doc.meta.schema_version = SCHEMA_VERSION;
        doc.candidates = candidates;
        fs::write(ns.file(), serde_json::to_string_pretty(&doc).expect("json"))
            .expect("seed over-cap doc");

        submit_candidate(&ns, candidate_input("新来的", "正文")).expect("submit");

        let after = read_doc(&ns);
        assert!(
            after.candidates.len() <= MAX_CANDIDATES,
            "总数要回到上限内：{}",
            after.candidates.len()
        );
        // 连删三条、按升序删：c0/c1/c2 走，下标前移算对了的话它们正好是这三条。
        for index in 0..(decided - 2) {
            let id = format!("c{index}");
            assert!(
                !after.candidates.iter().any(|row| row.id == id),
                "已决候选 {id} 应当被裁掉"
            );
        }
        // 还剩下的已决候选一条不许少。
        for index in (decided - 2)..decided {
            let id = format!("c{index}");
            assert!(
                after.candidates.iter().any(|row| row.id == id),
                "已决候选 {id} 不该在这轮被裁"
            );
        }
        // 待确认的一条都不能少——下标没前移到位就会误删它们中的某一条。
        for index in decided..total {
            let id = format!("c{index}");
            assert!(
                after.candidates.iter().any(|row| row.id == id),
                "待确认候选 {id} 被误删了"
            );
        }
        assert!(after.candidates.iter().any(|row| row.title == "新来的"));
    }

    /// `find_by_id` 的启用状态过滤：面板要能看已停用的，模型不许。
    #[test]
    fn find_by_id_hides_disabled_records_unless_asked() {
        let _home = TempHome::new("find");
        let ns = namespace();
        let saved = save(&ns, project_input("ws-a", "条目", "正文")).expect("save");
        set_state(&ns, &saved.id, STATE_DISABLED).expect("disable");

        assert!(
            find_by_id(&ns, &saved.id, false)
                .expect("readable")
                .is_none(),
            "模型这一侧看不见已停用的"
        );
        assert!(
            find_by_id(&ns, &saved.id, true)
                .expect("readable")
                .is_some(),
            "面板自己看得到"
        );
        assert!(find_by_id(&ns, "nope", true).expect("readable").is_none());
    }

    /// 文档读不出来时 `find_by_id` **如实报错**，而不是退成空列表。
    ///
    /// 退成空的话，回环桥会回一句「这条记忆不属于当前项目」，用户去查自己的项目 id，
    /// 而真因是盘上那份文件坏了。
    #[test]
    fn find_by_id_reports_a_corrupt_document() {
        let _home = TempHome::new("find-corrupt");
        let ns = namespace();
        fs::create_dir_all(ns.dir()).expect("dir");
        fs::write(ns.file(), "{ 坏的").expect("write");
        let error = find_by_id(&ns, "任何", true).expect_err("must refuse");
        assert!(error.to_string().contains("记忆库读不出来"), "{error}");
    }
}
