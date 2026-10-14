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
/// 同源抑制指纹的保留条数。
pub const MAX_SUPPRESSED: usize = 500;
/// 候选的保留条数，超出丢最老的**已决**候选。
pub const MAX_CANDIDATES: usize = 200;

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
    let outcome = apply(&mut doc)?;
    doc.meta.generation = doc.meta.generation.saturating_add(1);
    persist(ns, &doc)?;
    Ok(outcome)
}

/// 状态与计数。展示路径**容错读**：文档坏了面板也要能打开，但必须把原因说出来。
pub fn status(ns: &Namespace) -> MemoryStatus {
    let path = ns.file();
    let total_bytes = fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
    let (doc, warning) = load_for_display(ns);
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
    let mut view = MemoryListView {
        status: status(ns),
        memories,
        candidates,
    };
    view.status.warning = warning;
    view
}

/// 新建或更新一条记忆。更新走 compare-and-swap：`expected_revision` 与库里不一致
/// 就拒绝，旧窗口不会覆盖新修改。
pub fn save(ns: &Namespace, input: SaveInput) -> Result<MemoryRecord, AppError> {
    let title = input.title.trim().to_string();
    let body = input.body.trim().to_string();
    if title.is_empty() && body.is_empty() {
        return Err(AppError::Memory(
            "标题和正文都是空的，没有可保存的内容。至少写一句。".to_string(),
        ));
    }
    if body.len() > MAX_BODY_BYTES {
        return Err(AppError::Memory(format!(
            "正文 {} 字节，超过单条上限 {MAX_BODY_BYTES} 字节。请拆成几条更短的记忆。",
            body.len()
        )));
    }
    let scope = if input.scope == SCOPE_GENERAL {
        SCOPE_GENERAL
    } else {
        SCOPE_PROJECT
    };
    if scope == SCOPE_PROJECT
        && input.workspace_id.trim().is_empty()
        && input.workspace_path.trim().is_empty()
    {
        return Err(AppError::Memory(
            "项目记忆需要绑定一个已登记的工作区；没有工作区时请改存「通用偏好」。".to_string(),
        ));
    }
    let keywords: Vec<String> = input
        .keywords
        .iter()
        .map(|keyword| keyword.trim().to_string())
        .filter(|keyword| !keyword.is_empty())
        .collect();
    let verification = if input.verification.is_empty() {
        VERIFICATION_VERIFIED.to_string()
    } else {
        input.verification.clone()
    };
    let (workspace_id, workspace_path) = if scope == SCOPE_GENERAL {
        (String::new(), String::new())
    } else {
        (
            input.workspace_id.trim().to_string(),
            input.workspace_path.trim().to_string(),
        )
    };

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
                scope: scope.to_string(),
                workspace_id,
                workspace_path,
                title,
                body,
                keywords,
                pinned: input.pinned,
                source: input
                    .source
                    .clone()
                    .unwrap_or_else(|| MemorySource::manual(&now)),
                verification,
                state: STATE_ACCEPTED.to_string(),
                revision: 1,
                created_at: now.clone(),
                updated_at: now,
                ..MemoryRecord::default()
            };
            doc.memories.push(record.clone());
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
        record.scope = scope.to_string();
        record.workspace_id = workspace_id;
        record.workspace_path = workspace_path;
        record.title = title;
        record.body = body;
        record.keywords = keywords;
        record.pinned = input.pinned;
        record.verification = verification;
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

/// 开关：使用已批准的记忆。
pub fn set_use_approved(ns: &Namespace, enabled: bool) -> Result<(), AppError> {
    mutate(ns, |doc| {
        doc.meta.use_approved = enabled;
        Ok(())
    })
}

/// 开关：自动提取（P2 才真正接线，这里只落状态）。
#[allow(
    dead_code,
    reason = "P2 的自动提取开关：首版界面只显示「不可用」，不提供写入入口"
)]
pub fn set_auto_extract(ns: &Namespace, enabled: bool) -> Result<(), AppError> {
    mutate(ns, |doc| {
        doc.meta.auto_extract = enabled;
        Ok(())
    })
}

/// 提交一条待确认候选。模型只能走到这里——候选必须由用户批准才进权威记录。
pub fn submit_candidate(
    ns: &Namespace,
    input: CandidateInput,
) -> Result<MemoryCandidate, AppError> {
    let title = input.title.trim().to_string();
    let body = input.body.trim().to_string();
    if title.is_empty() && body.is_empty() {
        return Err(AppError::Memory(
            "候选内容为空，没有可提交的东西。".to_string(),
        ));
    }
    if body.len() > MAX_BODY_BYTES {
        return Err(AppError::Memory(format!(
            "候选正文 {} 字节，超过单条上限 {MAX_BODY_BYTES} 字节。",
            body.len()
        )));
    }
    let scope = if input.scope == SCOPE_GENERAL {
        SCOPE_GENERAL
    } else {
        SCOPE_PROJECT
    };
    let keywords: Vec<String> = input
        .keywords
        .iter()
        .map(|keyword| keyword.trim().to_string())
        .filter(|keyword| !keyword.is_empty())
        .collect();
    let verification = if input.verification.is_empty() {
        VERIFICATION_UNVERIFIED.to_string()
    } else {
        input.verification.clone()
    };
    let (workspace_id, workspace_path) = if scope == SCOPE_GENERAL {
        (String::new(), String::new())
    } else {
        (
            input.workspace_id.trim().to_string(),
            input.workspace_path.trim().to_string(),
        )
    };
    let source = input.source.clone();
    mutate(ns, move |doc| {
        let now = now_iso();
        let candidate = MemoryCandidate {
            id: new_id(),
            scope: scope.to_string(),
            workspace_id,
            workspace_path,
            title,
            body,
            keywords,
            source,
            verification,
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
pub fn decide_candidate(
    ns: &Namespace,
    id: &str,
    accept: bool,
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
            scope: candidate.scope.clone(),
            workspace_id: candidate.workspace_id.clone(),
            workspace_path: candidate.workspace_path.clone(),
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
        Ok(Some(record))
    })
}

/// 读一条记录并记一次「显式使用」（只有面板点开才算，检索展示不算）。
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

    pub(crate) fn read_doc(ns: &Namespace) -> MemoryDoc {
        serde_json::from_str(&fs::read_to_string(ns.file()).expect("doc")).expect("json")
    }
}

#[cfg(test)]
mod tests {
    use super::testkit::{general_input, namespace, project_input, read_doc, TempHome};
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

        let record = decide_candidate(&ns, &candidate.id, true)
            .expect("decide")
            .expect("accepted record");
        assert_eq!(record.state, STATE_ACCEPTED);
        assert_eq!(record.source.session, "s-1");
        assert_eq!(record.source.start_seq, 3);
        assert!(
            decide_candidate(&ns, &candidate.id, true).is_err(),
            "不能重复批准"
        );
        assert_eq!(status(&ns).accepted, 1);
    }

    #[test]
    fn rejecting_a_candidate_creates_nothing() {
        let _home = TempHome::new("reject");
        let ns = namespace();
        let candidate = submit_candidate(
            &ns,
            CandidateInput {
                title: "看起来不对的结论".to_string(),
                body: "正文".to_string(),
                ..CandidateInput::default()
            },
        )
        .expect("submit");
        assert!(decide_candidate(&ns, &candidate.id, false)
            .expect("decide")
            .is_none());
        assert!(status(&ns).accepted == 0);
        assert_eq!(status(&ns).pending_candidates, 0);
    }
}
