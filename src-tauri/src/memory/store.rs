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

use super::sources;
use crate::plugins::center::default_instance_key;
use crate::shell::atomic_write::atomic_write_private;
use crate::shell::error::AppError;
use crate::shell::paths;
use crate::shell::process::{read_state_file, StateRead};
use crate::shell::settings;

/// 文档 schema 版本。库里记的版本**高于**本值时拒绝写入（见 [`check_writable`]）。
///
/// **2 = 来源由单对象改成列表**（2026-10-15 的 schema 变更窗口，C4）：三条字段级变更攒在
/// 一次抬版里付账。抬版不是形式——写侧会把 `meta.schema_version` 盖成本值，于是旧应用读到
/// 新库会按「库比它新」拒写，而不是把没见过的 `sources` 数组当未知字段丢掉、再把自己那份
/// 单对象来源写回去（那样每一次编辑都会悄悄抹掉全部来源，而界面上看不出任何异常）。
pub const SCHEMA_VERSION: u32 = 6;

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
/// 自动提取来源（P2）。**已经有写入方**（`jobs_routes` 的 `/submit`），
/// 所以那句 `#[allow(dead_code, …首版不写)]` 已经过期——留着它会掩盖真正的死代码。
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
/// 删除留下的正文指纹上限。**与 [`MAX_MEMORIES`] 同量级**（记忆内核那次评审第 5 条）：
/// 原先是 500，比记录上限少一半——清空一个有 600+ 条小记录的库时，**最早的 100 枚指纹当场
/// 被挤掉**，而删除与清空的确认文案都承诺「记下正文指纹防止模型把同一条重新提回来」。
/// 1000 枚指纹约 64 KB，离 1 MiB 的文档上限还远，没有理由省这一半。
pub const MAX_SUPPRESSED: usize = 1000;
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
    /// **那一段来源内容的指纹**（会话来源才有，H7）：插件侧算出来的 64 位内容指纹，
    /// 写成 16 位十六进制。
    ///
    /// **为什么 `revision` 不够用**：`revision` 存的是**提取器版本**，而同一版提取器读同一段
    /// 会话永远得到同一个 `revision`——拿它判「来源变了」会得到「永远没变」这个假阴性。
    /// 而「来源变了」在真实世界里的形状是**会话被压缩/编辑过之后重提同一段**：
    /// 序号还在、内容不同，于是只有内容指纹认得出来。
    ///
    /// **它不是安全边界，判据也照这个前提写**：插件跑在内核沙箱里，而 `host/*.js` 到今天
    /// **零个 node 内置模块**——为一个「变没变」的判据去引 `node:crypto`，等于给沙箱面加
    /// 一条依赖换取一个没人需要的性质。伪造一个指纹的后果最多是「一条过期候选没被标」，
    /// 而候选本来就要用户点保存才生效。64 位碰撞概率约 1e-19，够判「同一段内容」。
    ///
    /// **空 = 没算出来**（面板手工新建的来源、以及旧版本写的候选）：空的一律不参与判定，
    /// 见 `sources::retire_stale_candidates`。
    #[serde(default)]
    pub content_hash: String,
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
    /// 这条记忆从哪些地方来（面板手动 / 会话里提出 / 自动提取，可能好几处）。
    /// 读回兼容旧的 `source` 单对象，见 [`crate::memory::sources`]。
    #[serde(default, alias = "source", deserialize_with = "sources::one_or_many")]
    pub sources: Vec<MemorySource>,
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
    pub kind: String,
    #[serde(flatten)]
    pub experience: Box<super::experience::ExperienceFields>,
    #[serde(flatten)]
    pub handoff: Box<super::handoff::HandoffFields>,
    pub id: String,
    pub scope: String,
    pub workspace_id: String,
    pub workspace_path: String,
    pub title: String,
    pub body: String,
    pub keywords: Vec<String>,
    /// 候选只有**一个**来源：它是「某次提交里的一件事」，还没有被批准——并入记录之后
    /// 才谈得上一条记忆从几个地方来。来源列表是批准那一刻才出现的（`decide_candidate`）。
    pub source: MemorySource,
    pub verification: String,
    /// `pending` / `accepted` / `rejected`。
    pub state: String,
    /// 模型标的「与这些既有记忆意义冲突」——**只标记、不裁决**（设计 §7-2）。
    /// 胜者由用户在面板里自己删一条；形状与限额见 `memory/marks.rs`。
    pub conflicts_with: Vec<String>,
    /// 模型标的「本条取代了这些待确认候选」（C6「用户纠正优先于旧候选」）——**同样只标记、
    /// 不执行**：壳不会自动拒收那条候选，用户在面板上看到提示后自己决定。
    ///
    /// **为什么不自动执行**（这一条容易被当成省事）：面板只列 pending，被拒的候选界面上就
    /// 不出现——于是「照做」等于模型标错一条就有一条用户提过的东西静默消失，而他连它存在
    /// 过都不知道。代价对照着看：C5 的纪律（丢标记不丢候选）在这一条上原样成立。
    #[serde(default)]
    pub supersedes: Vec<String>,
    pub created_at: String,
    pub decided_at: String,
}

/// `submit_candidate` 的两种结局：收成一条待确认候选，或并进一条已有的记录。
///
/// 返回一个枚举而不是只回候选 id，是因为「模型又提了一次同一件事」有两种都算成功的结局，
/// 调用方（回环桥 `/submit`、`memory_propose` 路由）必须能分别如实报出来——把它们都报成
/// 「已提交」或都报成「已拒绝」都会让用户在面板上看到与库里对不上的清单。
#[derive(Debug)]
pub enum SubmitOutcome {
    Candidate(MemoryCandidate),
    Merged(MemoryRecord),
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
    pub handoffs: Vec<super::handoff::Handoff>,
    pub experiences: Vec<super::experience::Experience>,
}

/// 面板新建/编辑一条记忆的输入。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SaveInput {
    pub kind: String,
    #[serde(flatten)]
    pub experience: super::experience::ExperienceFields,
    #[serde(flatten)]
    pub handoff: super::handoff::HandoffFields,
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
    pub kind: String,
    #[serde(flatten)]
    pub experience: super::experience::ExperienceFields,
    #[serde(flatten)]
    pub handoff: super::handoff::HandoffFields,
    pub conflicts_with: Vec<String>,
    pub supersedes: Vec<String>,
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
    /// 一次注入最多几条（`query::MAX_HITS` 的真值，面板直接显示，不自己抄一个数）。
    pub max_hits: usize,
    pub total_bytes: u64,
    /// 文档读不出来时的说明。**不打折**：展示的是空列表，磁盘上的文件没被动过。
    pub warning: Option<String>,
    /// **开关改过、而插件还没再调用过壳**（E6 的「处理中」）。
    ///
    /// 由壳侧进程内的两个时刻算出来（见 `recent::effect_pending`），**不在文档里**——
    /// 它描述的是「运行中的工作台认没认」，重启之后这个问题就不存在了。
    /// 面板据此把「已生效」与「处理中」分开说，而不是笼统写一句「下次生效」。
    pub effect_pending: bool,
}

/// 列表视图。
/// 一次清空的结果（面板据此说清「清掉了什么」，而不是只说「已清空」）。
#[derive(Debug, Clone, Default, Serialize)]
pub struct ClearOutcome {
    pub handoffs: usize,
    pub experiences: usize,
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
    pub handoffs: Vec<super::handoff::Handoff>,
    pub experiences: Vec<super::experience::Experience>,
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
    crate::shell::digest::hex(&digest[..])
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
pub(crate) fn mutate<T>(
    ns: &Namespace,
    apply: impl FnOnce(&mut MemoryDoc) -> Result<T, AppError>,
) -> Result<T, AppError> {
    let _guard = lock();
    // 跨进程那层：两个壳指向同一个实例时，进程内这把 Mutex 拦不住对方（见 `memory::lock`）。
    // 顺序是「先本进程、再跨进程」——同进程的竞争不该去碰文件锁。
    let _cross = crate::memory::lock::WriteLock::new(&ns.dir())?;
    let mut doc = load(ns)?;
    check_writable(&doc, ns)?;
    // 整库体积的上限在这里判，不在各个写入函数里：这是唯一的写入口，而「变大且越过
    // 上限」这条判据需要改动前后的两个数（缩小永远放行——库已经超标时，删一条是
    // 唯一的出路）。每回合检索都要解析整份文档，见 `input::MAX_DOC_BYTES`。
    let before = crate::memory::input::doc_bytes(&doc);
    let outcome = apply(&mut doc)?;
    crate::memory::input::ensure_doc_growth(before, crate::memory::input::doc_bytes(&doc))?;
    doc.meta.generation = doc.meta.generation.saturating_add(1);
    // 写入即抬版本：**读**时按库里记的版本判能不能写，这里才是把版本改成「本版写的」的那一步。
    // 少了它，一次抬版之后库里永远记着旧版本，直到下一次抬版才会盖上——中间那段时间旧应用
    // 读到新字段照样当未知字段丢掉，再把自己的单对象来源写回去。
    doc.meta.schema_version = SCHEMA_VERSION;
    persist(ns, &doc)?;
    Ok(outcome)
}

/// 状态为 `state` 的记录条数（状态摘要要 accepted 与非 accepted 两档）。
fn count_state(records: &[MemoryRecord], state: &str) -> usize {
    records.iter().filter(|row| row.state == state).count()
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
        accepted: count_state(&doc.memories, STATE_ACCEPTED),
        // 「已停用」是同一份判据的另一面，写成减法而不是把 `!=` 再抄一遍：两档数字来自同一处，
        // 日后改了一处也不会出现对不上的面板。
        disabled: doc.memories.len() - count_state(&doc.memories, STATE_ACCEPTED),
        pending_candidates: doc
            .candidates
            .iter()
            .filter(|candidate| candidate.state == CANDIDATE_PENDING)
            .count(),
        // **一次注入最多几条要下发**，不能由面板写死：那个常量在 `query.rs`，面板照着抄一个
        // 数就会在下次调它时静默说谎（同一个形状刚在插件的注入预算上发生过一次——3 改成 8，
        // 而面板里没有任何判据会因此变红）。这里给真值，面板只负责显示。
        max_hits: crate::memory::query::MAX_HITS,
        total_bytes,
        warning,
        effect_pending: crate::memory::recent::effect_pending(),
    }
}

/// 状态与计数。展示路径**容错读**：文档坏了面板也要能打开，但必须把原因说出来。
pub fn status(ns: &Namespace) -> MemoryStatus {
    let (doc, warning) = load_for_display(ns);
    status_from(ns, &doc, warning)
}

/// 待确认候选的条数（工作台会话头的待确认徽标用）。
///
/// **只数数**：徽标要说的只是「有 N 条在等用户审阅」，标题与正文一个字节都
/// 不出壳——比 `memory_recent_injections` 的「标题 + id」还要窄一档。读不出来
/// 就回 0：徽标消失比显示一个错的数好。
pub fn pending_count(ns: &Namespace) -> usize {
    load(ns)
        .map(|doc| {
            doc.candidates
                .iter()
                .filter(|candidate| candidate.state == CANDIDATE_PENDING)
                .count()
        })
        .unwrap_or(0)
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
        handoffs: doc.handoffs,
        experiences: doc.experiences,
    }
}

/// 新建或更新一条记忆。更新走 compare-and-swap：`expected_revision` 与库里不一致
/// 就拒绝，旧窗口不会覆盖新修改。
pub fn save(ns: &Namespace, input: SaveInput) -> Result<MemoryRecord, AppError> {
    match input.kind.as_str() {
        "handoff" => return super::handoff::save(ns, input),
        "experience" => return super::experience::save(ns, input),
        "" | "memory" => {}
        _ => return Err(AppError::Memory("不认识的记录类型，请刷新后重试。".into())),
    }
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
                sources: sources::one(input.source.clone(), &now),
                verification: clean.verification,
                state: STATE_ACCEPTED.to_string(),
                revision: 1,
                created_at: now.clone(),
                updated_at: now,
                ..MemoryRecord::default()
            };
            doc.memories.push(record.clone());
            // 用户自己写下了同一条，待确认里的重复候选就没有意义了（§7.2 用户优先）。
            crate::memory::duplicates::supersede_pending_duplicates(doc, &record, &now_iso());
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
        // **编辑不动 `sources`**：来源是「这件事从哪儿来」的历史，改正文不追加一条来源。
        // 正文改成与另一条记录相同时这里也不合并——那是「两件同样的事」，面板上看得见，
        // 而自动提取那条路上的合并由 `duplicates::merge_source` 负责（见 C4）。
        record.verification = clean.verification;
        record.updated_at = now;
        record.revision = record.revision.saturating_add(1);
        Ok(record.clone())
    })
}

/// 停用/启用一条记忆（不删除，保留来源与版本）。
pub fn set_state(ns: &Namespace, id: &str, state: &str) -> Result<MemoryRecord, AppError> {
    if state == "archived" {
        return super::handoff::archive(ns, id);
    }
    if state != STATE_ACCEPTED && state != STATE_DISABLED {
        return Err(AppError::Memory(format!("不认识的状态「{state}」。")));
    }
    mutate(ns, |doc| {
        if let Some(item) = doc.experiences.iter_mut().find(|item| item.record.id == id) {
            item.record.state = state.to_string();
            item.record.revision = item.record.revision.saturating_add(1);
            item.record.updated_at = now_iso();
            return Ok(item.record.clone());
        }
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
        if let Some(index) = doc.handoffs.iter().position(|item| item.record.id == id) {
            doc.handoffs.remove(index);
            return Ok(());
        }
        if let Some(index) = doc.experiences.iter().position(|item| item.record.id == id) {
            let removed = doc.experiences.remove(index);
            super::experience::suppress(doc, &removed.record.body);
            return Ok(());
        }
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
            handoffs: doc.handoffs.len(),
            experiences: doc.experiences.len(),
            memories: doc.memories.len(),
            candidates: doc.candidates.len(),
            suppressed: 0,
        };
        doc.handoffs.clear();
        for experience in std::mem::take(&mut doc.experiences) {
            if !doc
                .meta
                .suppressed
                .contains(&fingerprint(&experience.record.body))
            {
                outcome.suppressed += 1;
            }
            super::experience::suppress(doc, &experience.record.body);
        }
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
///
/// 命中一条**已接受**的同正文记录时不会生成候选，而是把这次来源并进那条记录（§7.2
/// 「相同规范化正文可合并来源」）：跨会话再提一次同一件事是自动提取最常见的形状，原先的做法
/// 只有「拒收」，而那会把「这件事在会话 B 也发生过」这条信息一起丢掉。
pub fn submit_candidate(ns: &Namespace, input: CandidateInput) -> Result<SubmitOutcome, AppError> {
    match input.kind.as_str() {
        "experience" => return super::experience::propose(ns, input),
        // 交接候选（2026-10-24）：模型可以**起草**交接，但与经验同等待遇——只能进
        // 待确认，批准权在面板（`auto_accept_candidate` 同样跳过它）。
        "handoff" => return super::handoff::propose(ns, input),
        "" | "memory" => {}
        _ => {
            return Err(AppError::Memory(format!(
                "不认识的候选类型「{}」；可提的类型是记忆、经验与交接。",
                input.kind
            )))
        }
    }
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
        // 同一正文**并入已有记录的来源**，并入不了才轮到拒收（`duplicate_reason`）。合并不成
        // 候选，所以它排在待确认条数上限**之前**：待确认满了不该挡住「给已有的一条补个来源」。
        if let Some(record) = crate::memory::duplicates::merge_source(doc, &clean, &source, &now) {
            return Ok(SubmitOutcome::Merged(record));
        }
        if let Some(reason) = crate::memory::duplicates::duplicate_reason(doc, &clean) {
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
        // **两个标记在同一个入口规范化**（`marks::from_input`），不是一个一个算。
        let marks = crate::memory::marks::from_input(&input.conflicts_with, &input.supersedes);
        // **来源变了就把先前那条标过期**（H7，设计 §8-6「来源变化时保留旧版本候选并标过期」）：
        // 旧候选**留着**（用户还要能看见并决定），只是 `verification` 变成 stale，于是面板上
        // 有一句「可能已过期，看一眼再决定」。判定在 `sources` 那边——`store.rs` 只剩几行余量，
        // 而它本来就贴着反棘轮线（基线预算 665，只许下调）。**要在 `source`/`now` 被 move 之前**。
        crate::memory::sources::retire_stale_candidates(doc, &source);
        let candidate = MemoryCandidate {
            kind: input.kind,
            experience: Default::default(),
            handoff: Default::default(),
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
            conflicts_with: marks.conflicts,
            supersedes: marks.supersedes,
            created_at: now,
            decided_at: String::new(),
        };
        doc.candidates.push(candidate.clone());
        crate::memory::prune::trim_decided_candidates(doc);
        Ok(SubmitOutcome::Candidate(candidate))
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
    decide_candidate_with(ns, id, accept, scope_override, true)
}

/// 自动提取的结果**直接成为记忆**（2026-10-19 用户要求：自动提取不该再让人逐条点保存）。
///
/// 它与用户点保存走**同一条**决策路径，唯一的差别是验证范围：没有经过用户确认的条目保持
/// **未验证**——不能顺手标成「已验证」，那一档在面板上的意思是「用户看过并确认过」。
///
/// 返回 `None` 时什么也不写：候选刚被处理过，或模型标了 `conflicts_with` / `supersedes`。
/// 后两种**必须留给用户裁决**（C5「不由模型选择胜者」、C6「取代只是标记」）——自动落地
/// 等于把那条裁决权悄悄吞掉，页面上一个字都不会说。
pub fn auto_accept_candidate(ns: &Namespace, id: &str) -> Result<Option<MemoryRecord>, AppError> {
    mutate(ns, |doc| {
        let ready = match doc.candidates.iter().find(|candidate| candidate.id == id) {
            Some(candidate) => {
                candidate.state == CANDIDATE_PENDING
                    && candidate.kind != "experience"
                    // 交接是「这个项目进行到哪儿」的权威快照，落成即顶掉旧交接——
                    // 那是用户的决定，与经验同等待遇，必须留给人审。
                    && candidate.kind != "handoff"
                    && candidate.conflicts_with.is_empty()
                    && candidate.supersedes.is_empty()
            }
            None => false,
        };
        if !ready {
            return Ok(None);
        }
        decide_in_doc(doc, id, true, None, false)
    })
}

/// 两条决策入口的公共实现。`confirmed` 只有「用户真的点过一次」才是 true。
fn decide_candidate_with(
    ns: &Namespace,
    id: &str,
    accept: bool,
    scope_override: Option<&str>,
    confirmed: bool,
) -> Result<Option<MemoryRecord>, AppError> {
    mutate(ns, |doc| {
        decide_in_doc(doc, id, accept, scope_override, confirmed)
    })
}

fn decide_in_doc(
    doc: &mut MemoryDoc,
    id: &str,
    accept: bool,
    scope_override: Option<&str>,
    confirmed: bool,
) -> Result<Option<MemoryRecord>, AppError> {
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
    if candidate.kind == "experience" {
        return super::experience::decide(doc, index, accept, scope_override);
    }
    if candidate.kind == "handoff" {
        return super::handoff::decide(doc, index, accept, scope_override);
    }
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
        // 候选自带的来源成为这条记录的第一个来源。
        sources: vec![candidate.source.clone()],
        // 用户看过正文与来源后点的「保存」——这是一次显式确认，标为已验证；自动接受
        // 没有这一层确认，沿用候选自己的验证范围（提取路径给的是「未验证」）。
        verification: if confirmed {
            VERIFICATION_VERIFIED.to_string()
        } else {
            candidate.verification.clone()
        },
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
    crate::memory::duplicates::supersede_pending_duplicates(doc, &record, &now_iso());
    Ok(Some(record))
}

/// 读一条记录并记一次「显式使用」。
///
/// **调用方只有回环桥的 `read` 路由**（模型调 `memory_read` 工具）。检索展示不算
/// 使用；面板的列表也不计数——它在列表里就把正文画出来了，没有「点开」这个动作。
/// 面板上那个「模型读过 N 次」因此是「模型读过几次」。
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

    /// **真的磁盘满**（A17）：往一个快满的小卷里一直写，直到 ENOSPC。
    ///
    /// 只读目录那条（`a_read_only_directory_fails_loudly_and_keeps_the_old_document`）验的是
    /// **EACCES**，代码路径虽然一样，但「磁盘满」还多一层：写到一半空间没了。
    /// 跑法（清单「怎么验」那一节记了完整命令）：
    ///
    /// ```sh
    /// hdiutil create -size 1m -fs HFS+ -volname xlinktiny -ov /tmp/xlink-tiny.dmg
    /// hdiutil attach /tmp/xlink-tiny.dmg -nobrowse -quiet
    /// XLINK_TINY_VOLUME=/Volumes/xlinktiny \
    ///   cargo test --lib manual_disk_full -- --ignored --nocapture
    /// hdiutil detach /Volumes/xlinktiny -quiet
    /// ```
    #[test]
    #[ignore = "手动夹具：要一个真的快满的卷（见清单「怎么验」）"]
    fn manual_disk_full_reports_clearly_and_keeps_what_is_written() {
        let volume = std::env::var("XLINK_TINY_VOLUME")
            .expect("要 XLINK_TINY_VOLUME 指向一个小卷（见这条用例的文档注释）");
        let home = PathBuf::from(&volume).join("xlink-home");
        let _guard = crate::tests::scoped_xlink_home(&home);
        let ns = namespace();

        // 每条 6 KB 左右，够快地把它填满（每写一次都要重写整份文档，所以条数别太多）。
        let body = "正文".repeat(1000);
        let mut written = 0usize;
        let error = loop {
            let title = format!("第 {written} 条");
            match save(&ns, general_input(&title, &body)) {
                Ok(_) => {
                    written += 1;
                    assert!(written < 2000, "写了 2000 条还没满，这个卷不够小");
                }
                Err(error) => break error,
            }
        };
        let message = error.to_string();
        println!("[disk-full] 写进 {written} 条之后失败：{message}");
        assert!(
            message.contains("无法写入记忆库") || message.contains("无法创建记忆目录"),
            "文案要说清是写不进去：{message}"
        );
        assert!(
            message.contains(&ns.file().display().to_string()),
            "要带上路径，用户才知道去哪查：{message}"
        );
        // **写失败的这一次没动到已有的东西**：文档还读得出来，条数正好是写成功那些。
        let doc = load(&ns).expect("满了也要读得出来");
        assert_eq!(doc.memories.len(), written, "写失败的这次不该留下半份文档");
        println!("[disk-full] 已有的 {written} 条完好");
    }

    /// **接线判据**：`mutate` 的临界区里那把跨进程锁真的拿着（摘掉接线即红）。
    #[test]
    fn mutate_holds_the_cross_process_lock() {
        let _home = TempHome::new("lock-wired");
        let ns = namespace();
        let mut held = None;
        mutate(&ns, |_doc| {
            held = Some(crate::memory::lock::lock_is_held(&ns.dir()));
            Ok(())
        })
        .expect("写一次");
        assert_eq!(held, Some(true), "临界区里要拿着跨进程锁");
        // 出来之后要放掉：否则下一次写入会白等 500 毫秒再报错。
        assert!(
            !crate::memory::lock::lock_is_held(&ns.dir()),
            "出了临界区要释放"
        );
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

    /// 提交候选并取那条候选。**并入来源那种结局在这里直接判红**——写这些用例时预期的就是
    /// 「收成待确认」，而两种结局各有各的用例（并入那条见 `merging_the_same_body_from_another_session`）。
    fn submit_ok(ns: &Namespace, input: CandidateInput) -> MemoryCandidate {
        match submit_candidate(ns, input).expect("submit candidate") {
            SubmitOutcome::Candidate(candidate) => candidate,
            SubmitOutcome::Merged(record) => panic!("本该收成候选，却并进了 {}", record.title),
        }
    }

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
        // 让这条用例只在 macOS 上成立——它当时能一直绿着，是因为 Windows CI 那个
        // job 只跑 `cargo check --all-targets`、从不执行测试。2026-10-15 起那个 job
        // 跑 `cargo test --locked`（当天先在真机上跑过一遍全量门禁，全绿），
        // 同形状的期望值错误不会再溜过去。
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
        assert_eq!(saved.sources.len(), 1);
        assert_eq!(saved.sources[0].kind, SOURCE_MANUAL);
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
        let candidate = submit_ok(
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
                    content_hash: "h-1".to_string(),
                    revision: "r1".to_string(),
                    at: now_iso(),
                },
                ..CandidateInput::default()
            },
        );

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
        assert_eq!(record.sources.len(), 1);
        assert_eq!(record.sources[0].session, "s-1");
        assert_eq!(record.sources[0].start_seq, 3);
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
        let candidate = submit_ok(
            &ns,
            CandidateInput {
                scope: SCOPE_PROJECT.to_string(),
                workspace_id: "ws-a".to_string(),
                workspace_path: "/tmp/ws-a".to_string(),
                title: "通用一点的写法".to_string(),
                body: "提交前跑检查".to_string(),
                ..CandidateInput::default()
            },
        );

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

    /// 自动提取的结果**直接成为记忆**，不需要用户再点一次保存（2026-10-19 用户要求）。
    /// 但它没有经过用户确认，所以验证范围必须保持「未验证」——标成「已验证」是在替用户作证。
    #[test]
    fn an_extracted_candidate_is_saved_directly_and_stays_unverified() {
        let _home = TempHome::new("auto-accept");
        let ns = namespace();
        let candidate = submit_ok(
            &ns,
            candidate_input("提交前先跑检查", "提交前跑 npm run check"),
        );
        assert_eq!(candidate.verification, VERIFICATION_UNVERIFIED);

        let record = auto_accept_candidate(&ns, &candidate.id)
            .expect("auto accept")
            .expect("record");
        assert_eq!(record.state, STATE_ACCEPTED);
        assert_eq!(
            record.verification, VERIFICATION_UNVERIFIED,
            "没经过用户确认就不能标成已验证"
        );
        assert_eq!(record.sources.len(), 1, "候选的来源落在记录的第一个来源上");

        let doc = read_doc(&ns);
        assert_eq!(doc.memories.len(), 1);
        assert_eq!(
            doc.candidates[0].state, CANDIDATE_ACCEPTED,
            "候选本身要留下已接受的结局（用户问「我那条候选呢」时有据可查）"
        );
        let view = list(
            &ns,
            &ListFilter {
                include_candidates: true,
                ..ListFilter::default()
            },
        );
        assert_eq!(view.memories.len(), 1);
        assert_eq!(view.candidates.len(), 0, "已决候选不再出现在待确认里");
        assert_eq!(view.status.accepted, 1);
    }

    /// 模型标了冲突或取代的候选**不能被自动接受**：C5/C6 的裁决权在用户手里，自动落地
    /// 等于把「留哪一条」悄悄替用户决定了。两种标记都要挡住。
    #[test]
    fn a_marked_candidate_waits_for_the_user_instead_of_being_auto_accepted() {
        let _home = TempHome::new("auto-accept-marked");
        let ns = namespace();

        let conflicted = submit_ok(
            &ns,
            CandidateInput {
                conflicts_with: vec!["m-existing".to_string()],
                ..candidate_input("新的说法", "提交前不要跑检查")
            },
        );
        assert!(
            auto_accept_candidate(&ns, &conflicted.id)
                .expect("auto accept")
                .is_none(),
            "带冲突标记的不能自动落地"
        );

        let superseding = submit_ok(
            &ns,
            CandidateInput {
                supersedes: vec!["c-old".to_string()],
                ..candidate_input("改口之后", "提交前跑 npm run check")
            },
        );
        assert!(
            auto_accept_candidate(&ns, &superseding.id)
                .expect("auto accept")
                .is_none(),
            "带取代标记的不能自动落地"
        );

        assert_eq!(status(&ns).accepted, 0, "两条都该留在待确认里");
        assert_eq!(status(&ns).pending_candidates, 2);
    }

    #[test]
    fn rejecting_a_candidate_creates_nothing() {
        let _home = TempHome::new("reject");
        let ns = namespace();
        let candidate = submit_ok(&ns, candidate_input("看起来不对的结论", "正文"));
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

    /// **同正文 + 新来源 = 并入来源，而不是新收一条**（C4，§7.2「相同规范化正文可合并来源」）。
    ///
    /// 这条判据的两半都要钉住：**记录数不变**（用户看到的清单里不能因为模型换了个说法就多一条），
    /// **来源多了一条**（「这件事在会话 B 也说过」这条信息原先整个丢掉）。
    #[test]
    fn merging_the_same_body_from_another_session_adds_a_source() {
        let _home = TempHome::new("merge-source");
        let ns = namespace();
        let saved = save(
            &ns,
            project_input("ws-a", "构建前先跑检查", "提交前跑 npm run check"),
        )
        .expect("save");

        let extract = |session: &str| CandidateInput {
            scope: SCOPE_PROJECT.to_string(),
            workspace_id: "ws-a".to_string(),
            workspace_path: "/tmp/ws-a".to_string(),
            title: "换个标题也一样".to_string(),
            body: "提交前跑 npm run check".to_string(),
            source: MemorySource {
                kind: SOURCE_EXTRACT.to_string(),
                session: session.to_string(),
                start_seq: 12,
                end_seq: 12,
                ..MemorySource::default()
            },
            ..CandidateInput::default()
        };

        let merged = match submit_candidate(&ns, extract("s-1")).expect("submit from s-1") {
            SubmitOutcome::Merged(record) => record,
            SubmitOutcome::Candidate(_) => panic!("同正文不该再收一条候选"),
        };
        assert_eq!(merged.id, saved.id, "并入的是原来那条");
        assert_eq!(merged.sources.len(), 2, "手动保存 + 会话 B 的自动提取");
        assert_eq!(merged.sources[1].session, "s-1");
        assert_eq!(merged.revision, saved.revision + 1, "并入也是一次改动");

        // **同一条来源不重复记**，而且这次仍算「重复」——并不进去就退回拒收，不静默截断。
        let refused = submit_candidate(&ns, extract("s-1"))
            .expect_err("同一条来源没有新东西可记")
            .to_string();
        assert!(refused.contains("已经在记忆里"), "{refused}");

        // 换一个会话就是新来源：应当并进去，来源列表按序增长。
        let third = match submit_candidate(&ns, extract("s-2")).expect("submit from s-2") {
            SubmitOutcome::Merged(record) => record,
            SubmitOutcome::Candidate(_) => panic!("同正文不该再收一条候选"),
        };
        assert_eq!(third.sources.len(), 3);

        let doc = read_doc(&ns);
        assert_eq!(doc.memories.len(), 1, "并入来源不新建记录");
        assert!(
            doc.candidates.is_empty(),
            "并入来源不占用户的待确认清单：{:?}",
            doc.candidates
        );
    }

    /// **已停用的记录不给合并**：用户明确停用过的东西，模型不该顺手把它带回来。这条与
    /// 「并入来源」只差一个状态，而那一个状态就是「用户自己的决定」，不能被模型绕过。
    #[test]
    fn a_disabled_record_is_never_revived_by_a_merge() {
        let _home = TempHome::new("merge-disabled");
        let ns = namespace();
        let saved = save(
            &ns,
            project_input("ws-a", "构建前先跑检查", "提交前跑 npm run check"),
        )
        .expect("save");
        set_state(&ns, &saved.id, STATE_DISABLED).expect("disable");

        let refused = submit_candidate(
            &ns,
            CandidateInput {
                scope: SCOPE_PROJECT.to_string(),
                workspace_id: "ws-a".to_string(),
                workspace_path: "/tmp/ws-a".to_string(),
                title: "换个标题也一样".to_string(),
                body: "提交前跑 npm run check".to_string(),
                source: MemorySource {
                    kind: SOURCE_EXTRACT.to_string(),
                    session: "s-1".to_string(),
                    ..MemorySource::default()
                },
                ..CandidateInput::default()
            },
        )
        .expect_err("停用过的记忆不许被并活")
        .to_string();
        assert!(refused.contains("已经在记忆里"), "{refused}");
        assert!(
            refused.contains(STATE_DISABLED),
            "要说清它现在是停用：{refused}"
        );
        assert_eq!(read_doc(&ns).memories[0].state, STATE_DISABLED);
    }

    /// **写过的文件立刻盖上本版版本号**（schema 变更窗口）。少了这一句，一次抬版之后库里
    /// 永远记着旧版本，直到下一次抬版才盖上——中间那段时间旧应用读到新字段照样当未知字段
    /// 丢掉，再把它那份单对象来源写回去，用户下一次编辑就抹掉了全部来源。
    #[test]
    fn writing_stamps_the_current_schema_version() {
        let _home = TempHome::new("schema-stamp");
        let ns = namespace();
        let doc = MemoryDoc {
            meta: MemoryMeta {
                schema_version: 1,
                generation: 4,
                ..MemoryMeta::default()
            },
            ..MemoryDoc::default()
        };
        fs::create_dir_all(ns.dir()).expect("mkdir");
        fs::write(ns.file(), serde_json::to_string_pretty(&doc).expect("json"))
            .expect("write v1 doc");

        save(
            &ns,
            project_input("ws-a", "构建前先跑检查", "提交前跑 npm run check"),
        )
        .expect("save on a v1 library");

        let after = read_doc(&ns);
        assert_eq!(after.meta.schema_version, SCHEMA_VERSION);
        assert_eq!(after.memories.len(), 1, "旧库读得出来、也写得回去");
        assert_eq!(after.memories[0].sources[0].kind, SOURCE_MANUAL);
    }

    /// **抬版之后旧库照样读得出来**：旧文件里是单对象 `source`，读进来是列表（`sources`）。
    /// 没有这一句，这次抬版就会把老用户的记忆库变成「下次打开就报错」。
    #[test]
    fn a_v1_document_with_a_single_source_object_still_loads() {
        let _home = TempHome::new("v1-read");
        let ns = namespace();
        fs::create_dir_all(ns.dir()).expect("mkdir");
        fs::write(
            ns.file(),
            r#"{"meta":{"schema_version":1,"generation":7,"use_approved":true,"auto_extract":false,"suppressed":[]},"memories":[{"id":"m-1","scope":"general","workspace_id":"","workspace_path":"","title":"回答用中文","body":"默认用中文回答。","keywords":[],"pinned":false,"source":{"kind":"user-manual","session":"","start_seq":0,"end_seq":0,"revision":"","at":"2026-10-01T00:00:00Z"},"verification":"verified","state":"accepted","revision":1,"created_at":"2026-10-01T00:00:00Z","updated_at":"2026-10-01T00:00:00Z","last_used_at":"","use_count":0,"recall_count":0}],"candidates":[]}"#,
        )
        .expect("write v1 doc");

        let view = list(&ns, &ListFilter::default());
        assert_eq!(view.memories.len(), 1);
        assert_eq!(view.memories[0].sources.len(), 1, "旧的单对象读成一条来源");
        assert_eq!(view.memories[0].sources[0].kind, SOURCE_MANUAL);
        assert_eq!(view.status.schema_version, 1, "读不改版本，只在写的时候盖");

        // 再写一次：文件里就换成数组形状了。
        save(&ns, general_input("回答用中文", "默认用中文回答。")).expect("save on a v1 library");
        let text = fs::read_to_string(ns.file()).expect("read back");
        // 落盘走的是 `to_string_pretty`，数组与对象之间有换行——判形状不能拿紧凑写法去比。
        assert!(text.contains("\"sources\": ["), "写出去是新形状：{text}");
        assert!(
            !text.contains("\"source\":"),
            "旧的那个键不再写出去：{text}"
        );
    }

    /// C5：冲突标记要**真的落进文件**。`conflicts.rs` 的用例只管形状（去重、限额），
    /// 而「提交那一刻的规范化结果真的进了 `memory.json`」是另一件事——形状对了、没接上，
    /// 面板上就永远是「没有冲突」。
    #[test]
    fn submitting_a_candidate_carries_its_conflict_markers_through_to_the_file() {
        let _home = TempHome::new("conflict-markers");
        let ns = namespace();
        let existing =
            save(&ns, general_input("回答用中文", "默认用中文回答。")).expect("save one");
        let over_long = "x".repeat(crate::memory::marks::MAX_MARK_ID_BYTES + 1);
        let outcome = submit_candidate(
            &ns,
            CandidateInput {
                conflicts_with: vec![
                    existing.id.clone(),
                    existing.id.clone(),
                    "  ".to_string(),
                    over_long.clone(),
                    "mem-other".to_string(),
                ],
                ..candidate_input("提交前先跑检查", "提交前必须跑 npm run check。")
            },
        )
        .expect("submit");
        let SubmitOutcome::Candidate(candidate) = outcome else {
            panic!("这条正文是新的，应该收成候选");
        };
        // 留下的两条：去重保序（模型先标的那条在前）、超长与空串被丢。
        assert_eq!(
            candidate.conflicts_with,
            vec![existing.id.clone(), "mem-other".to_string()]
        );
        let doc = read_doc(&ns);
        assert_eq!(
            doc.candidates[0].conflicts_with,
            vec![existing.id, "mem-other".to_string()],
            "落盘之后要还是那两条——面板读的是文件，不是内存里那份"
        );
    }

    /// C6：取代标记与冲突标记**同一套形状、不同的限额**。落库那一半在这里——
    /// 面板只列 pending，所以壳只存标记、不据此拒收任何候选（拒收与否是用户按一次的事）。
    #[test]
    fn supersedes_is_stored_but_nothing_is_rejected_by_it() {
        let _home = TempHome::new("supersedes-store");
        let ns = namespace();
        set_use_approved(&ns, true).expect("打开记忆开关");
        // **一条真的、还在待确认里的候选**：判据要盯的是「即使它真的存在，壳也不许动它」。
        // 只用一个库里不存在的 id 去验，验到的只是「id 不存在时照样收」——
        // 而「只标记、不执行」真正防的那件事是「照着标记把旧候选拒掉」，那条路根本没被走到。
        let SubmitOutcome::Candidate(pending) = submit_candidate(
            &ns,
            crate::memory::store::testkit::candidate_input("个人项目一律用 pnpm", "都用 pnpm。"),
        )
        .expect("预置一条待确认候选") else {
            panic!("新正文应该收成候选");
        };

        let outcome = submit_candidate(
            &ns,
            CandidateInput {
                supersedes: vec![
                    pending.id.clone(),
                    pending.id.clone(),
                    "  ".to_string(),
                    "x".repeat(crate::memory::marks::MAX_MARK_ID_BYTES + 1),
                ],
                ..candidate_input("这个仓库用 npm install", "这个仓库用 npm install。")
            },
        )
        .expect("submit");
        let SubmitOutcome::Candidate(candidate) = outcome else {
            panic!("新正文应该收成候选");
        };
        // 去重保序、超长被丢：留下的只有那一条。
        assert_eq!(candidate.supersedes, vec![pending.id.clone()]);
        let doc = read_doc(&ns);
        assert_eq!(doc.candidates.len(), 2, "两条待确认候选都在");
        let older = doc
            .candidates
            .iter()
            .find(|row| row.id == pending.id)
            .expect("被标的那条");
        assert_eq!(
            older.state, CANDIDATE_PENDING,
            "**它真的存在、也真的被标了，而它仍然是待确认**——这就是「只标记、不执行」"
        );
        // 上限由调用方给：传 1 就只留 1 条（两个标记各是各的，理由写在 marks.rs 的常量上）。
        let capped = crate::memory::marks::normalize(&["a".to_string(), "b".to_string()], 1);
        assert_eq!(capped.kept, vec!["a".to_string()]);
        let _ = pending;
    }

    /// 带会话来源的候选输入——H7 的三个字段（会话 / 序号区间 / 指纹）都在参数里，
    /// **不要在用例里手写 `MemorySource` 字面量**：写一遍就会漏一个字段，而漏掉的那次
    /// 会以「判据红在一条不相干的地方」的形式冒出来（2026-10-16 真踩过 `candidate_needs_an
    /// _explicit_decision` 少字段直接编译不过）。
    fn session_candidate(
        title: &str,
        body: &str,
        session: &str,
        start_seq: u64,
        end_seq: u64,
        content_hash: &str,
    ) -> CandidateInput {
        CandidateInput {
            source: MemorySource {
                kind: SOURCE_EXTRACT.to_string(),
                session: session.to_string(),
                start_seq,
                end_seq,
                content_hash: content_hash.to_string(),
                revision: "xlink-memory-extract/4".to_string(),
                at: now_iso(),
            },
            ..candidate_input(title, body)
        }
    }

    /// **H7 的落库主判据**：同会话 + 序号区间重叠 + 指纹不同 → 先前那条待确认候选标 stale。
    ///
    /// 走的是真的 `submit_candidate`，不是 `retire_stale_candidates` 那个纯函数——
    /// 纯函数那三条在 `sources.rs` 里各自验过，这里要验的是**它真的被接在写路径上**，
    /// 而「函数有判据、调用点漏了」正是 2026-10-11 那两条判据（`doc 承诺下次启动会接手`
    /// 不等于接上了）要防的同一个形状。
    ///
    /// **不成立的那几半也走同一个入口**：只验「该标的标了」的话，把判据改成「凡是新候选都标
    /// 过期」照样全绿，而那恰恰是这条纪律要防的方向（误标比漏标坏）。
    #[test]
    fn re_extracting_the_same_range_retires_only_the_earlier_candidate() {
        let _home = TempHome::new("h7-retire");
        let ns = namespace();
        set_use_approved(&ns, true).expect("打开记忆开关");

        let first = submit_ok(
            &ns,
            session_candidate("第一版", "第一版正文。", "s-1", 8, 84, "aaaa"),
        );
        // 同一段重提，指纹不同 → 先前那条标过期，**而且它仍然是待确认**（只标不删）。
        let second = submit_ok(
            &ns,
            session_candidate("第二版", "第二版正文。", "s-1", 8, 84, "bbbb"),
        );
        let doc = read_doc(&ns);
        let by_id = |id: &str| {
            doc.candidates
                .iter()
                .find(|row| row.id == id)
                .unwrap_or_else(|| panic!("候选 {id} 不在库里"))
                .clone()
        };
        let older = by_id(&first.id);
        assert_eq!(
            older.verification, VERIFICATION_STALE,
            "来源变了的那条被标过期"
        );
        assert_eq!(
            older.state, CANDIDATE_PENDING,
            "**标过期不许顺手把它处理掉**——用户还要能看见并自己决定（纪律：只标不删不拒收）"
        );
        assert_eq!(
            by_id(&second.id).verification,
            VERIFICATION_UNVERIFIED,
            "刚提交的那条自己不该被标"
        );

        // 指纹相同 → 不标（内容没变，说它过期是假的）。
        submit_ok(
            &ns,
            session_candidate("第三版", "第三版正文。", "s-1", 8, 84, "bbbb"),
        );
        // 序号不重叠 → 不标（那是接着往后说，不是重提）。
        submit_ok(
            &ns,
            session_candidate("第四版", "第四版正文。", "s-1", 200, 300, "cccc"),
        );
        // 别的会话 → 不标（那是另一件事）。
        submit_ok(
            &ns,
            session_candidate("第五版", "第五版正文。", "s-2", 8, 84, "dddd"),
        );
        // 没有会话的来源（面板手工保存那条 `save`）→ 不标。**这一条最要紧**：它带指纹也
        // 不带会话，而判据要求两个都在——只查指纹的话，面板每存一条都会把用户所有待确认
        // 候选标成过期。
        submit_ok(
            &ns,
            session_candidate("第六版", "第六版正文。", "", 8, 84, "eeee"),
        );
        let doc = read_doc(&ns);
        let still_fresh = ["第五版", "第六版"].map(|title| {
            doc.candidates
                .iter()
                .find(|row| row.title == title)
                .unwrap_or_else(|| panic!("{title} 不在库里"))
                .verification
                .clone()
        });
        assert_eq!(
            still_fresh,
            [
                VERIFICATION_UNVERIFIED.to_string(),
                VERIFICATION_UNVERIFIED.to_string()
            ],
            "别的会话与无会话的来源都不该被标"
        );
        assert_eq!(
            doc.candidates
                .iter()
                .filter(|row| row.verification == VERIFICATION_STALE)
                .count(),
            1,
            "**整份库里只该有那一条 stale**（指纹相同 / 不重叠 / 别的会话 / 无会话，四种不成立）"
        );
    }

    /// 抬到 schema 5 这一批**不许要求用户手工改自己的数据文件**：v4 的来源既没有
    /// `content_hash`，而读回要照样成功（`#[serde(default)]` 兜住）。
    ///
    /// **这条判据真正要杀的行为是「新机制把老数据全标成过期」**：磁盘上每一份 v4 库里的
    /// 候选都没有指纹，而拿空值去比字符串「全都不同」——于是用户装上新版之后，第一次有
    /// 指纹的候选一进来，他攒的所有待确认候选全部变成「可能已过期」。所以这里不只验
    /// 「读得进来」，还要验**读进来之后不会被新机制动**。
    #[test]
    fn a_v4_document_without_the_content_hash_still_loads_and_is_never_retired() {
        let _home = TempHome::new("v4-read");
        let ns = namespace();
        set_use_approved(&ns, true).expect("打开记忆开关");
        fs::create_dir_all(ns.dir()).expect("mkdir");
        fs::write(
            ns.file(),
            r#"{"meta":{"schema_version":4,"generation":3,"use_approved":true,"auto_extract":false,"suppressed":[]},"memories":[],"candidates":[{"id":"c-old","scope":"general","workspace_id":"","workspace_path":"","title":"提交前先跑检查","body":"提交前跑 npm run check。","keywords":["提交"],"source":{"kind":"extract","session":"s1","start_seq":8,"end_seq":84,"revision":"xlink-memory-extract/3","at":""},"verification":"unverified","state":"pending","created_at":"2026-10-16T00:00:00Z","decided_at":""}]}"#,
        )
        .expect("write v4 doc");

        let doc = read_doc(&ns);
        assert_eq!(doc.candidates.len(), 1);
        assert!(
            doc.candidates[0].source.content_hash.is_empty(),
            "v4 的来源没有这个键，读出来必须是空的而不是报错"
        );

        // **真来一条带指纹的新候选**（同会话 + 序号区间完全重叠）——老那条仍然不许被标。
        submit_ok(
            &ns,
            session_candidate("新来的", "新来的正文。", "s1", 8, 84, "ffff"),
        );
        let doc = read_doc(&ns);
        let old = doc
            .candidates
            .iter()
            .find(|row| row.id == "c-old")
            .expect("老候选还在库里");
        assert_eq!(
            old.verification, VERIFICATION_UNVERIFIED,
            "**没有指纹的老候选宁可漏标**：标成过期会让用户攒的东西全部变成「可能不准」"
        );
        assert_eq!(old.state, CANDIDATE_PENDING);
    }

    /// 抬到 schema 4 这一批**不许要求用户手工改自己的数据文件**：v2/v3 的候选既没有
    /// `conflicts_with` 也没有 `supersedes`，而读回要照样成功（结构级 `#[serde(default)]`
    /// 兜住）。这条判据从 v2→v3 时就在，抬到 4 之后**同一个形状继续成立**——改数据文件
    /// 版本号而不加新判据，就会以为「上次验过了」。
    #[test]
    fn a_v2_document_without_the_conflict_field_still_loads() {
        let _home = TempHome::new("v2-read");
        let ns = namespace();
        fs::create_dir_all(ns.dir()).expect("mkdir");
        fs::write(
            ns.file(),
            r#"{"meta":{"schema_version":2,"generation":3,"use_approved":true,"auto_extract":false,"suppressed":[]},"memories":[],"candidates":[{"id":"c-1","scope":"general","workspace_id":"","workspace_path":"","title":"提交前先跑检查","body":"提交前跑 npm run check。","keywords":["提交"],"source":{"kind":"extract","session":"s1","start_seq":8,"end_seq":84,"revision":"xlink-memory-extract/1","at":""},"verification":"unverified","state":"pending","created_at":"2026-10-16T00:00:00Z","decided_at":""}]}"#,
        )
        .expect("write v2 doc");

        // 直接读文档而不是走 `list`（它的默认筛选**不含**候选）：这一条要验的是
        // 「反序列化一个没有该键的 v2 候选」这件事本身。
        let doc = read_doc(&ns);
        assert_eq!(doc.candidates.len(), 1);
        assert!(
            doc.candidates[0].conflicts_with.is_empty(),
            "v2 的候选没有这个键，读出来必须是空的而不是报错"
        );
        assert!(
            doc.candidates[0].supersedes.is_empty(),
            "v2/v3 的候选同样没有 `supersedes`，读出来也必须是空的"
        );
        // 写一次就把版本盖到当前（读不改版本，只在写的时候盖）。
        submit_candidate(&ns, candidate_input("另一件事", "另一件与上面不同的事。"))
            .expect("submit");
        let text = fs::read_to_string(ns.file()).expect("read back");
        assert!(
            text.contains(&format!("\"schema_version\": {SCHEMA_VERSION}")),
            "{text}"
        );
    }

    /// **收掉重复候选也要带范围**（记忆内核那次评审第 1 条）：`duplicate_reason`（写入闸）按范围判了，
    /// 而 `supersede_pending_duplicates` 是同一件事的另一半，原先仍按**全库**指纹扫——A、B 两个仓库
    /// 各自提了同一条约定时，用户在 A 点保存会让 **B 的那条从待确认里静默消失**，而面板与导出都只列
    /// pending，任何界面上都看不到它。
    #[test]
    fn saving_in_one_project_does_not_supersede_another_projects_candidate() {
        let _home = TempHome::new("supersede-scope");
        let ns = namespace();
        // 两个项目各提一条同正文的候选（注意：写入闸现在按范围判，所以两条都收得下）。
        for workspace in ["ws-a", "ws-b"] {
            submit_candidate(
                &ns,
                CandidateInput {
                    scope: SCOPE_PROJECT.to_string(),
                    workspace_id: workspace.to_string(),
                    workspace_path: format!("/tmp/{workspace}"),
                    title: format!("{workspace} 的约定"),
                    body: "提交前跑 npm run check".to_string(),
                    ..CandidateInput::default()
                },
            )
            .expect("两个项目各收一条");
        }

        // 在 A 里保存同正文的一条：只该收掉 A 的那条候选。
        save(
            &ns,
            project_input("ws-a", "A 写下的", "提交前跑 npm run check"),
        )
        .expect("save");

        let view = list(
            &ns,
            &ListFilter {
                include_candidates: true,
                ..ListFilter::default()
            },
        );
        let pending: Vec<&str> = view
            .candidates
            .iter()
            .filter(|candidate| candidate.state == CANDIDATE_PENDING)
            .map(|candidate| candidate.workspace_id.as_str())
            .collect();
        assert_eq!(
            pending,
            vec!["ws-b"],
            "B 的待确认不该被 A 的保存收掉：{pending:?}"
        );
    }

    /// **重复判据带范围**（独立评审第 16 条）：指纹原先在全库比，于是 A 项目里已有的那句
    /// 「提交前跑 npm run check」会让 B 项目提同一句时被拒——两个项目各自定下同一条约定完全正常，
    /// 而 B 项目的用户看到的是「已经在记忆里了」却在自己的项目里找不到它。
    #[test]
    fn the_same_body_in_another_project_is_not_a_duplicate() {
        let _home = TempHome::new("dedup-scope");
        let ns = namespace();
        save(
            &ns,
            project_input("ws-a", "A 项目的约定", "提交前跑 npm run check"),
        )
        .expect("save");

        // 同一个正文、**另一个项目**：不算重复，收下。
        submit_candidate(
            &ns,
            CandidateInput {
                scope: SCOPE_PROJECT.to_string(),
                workspace_id: "ws-b".to_string(),
                workspace_path: "/tmp/ws-b".to_string(),
                title: "B 项目也这么定".to_string(),
                body: "提交前跑 npm run check".to_string(),
                ..CandidateInput::default()
            },
        )
        .expect("另一个项目的同一条不算重复");

        // 通用记录仍然挡住通用候选：通用记录处处可见，再存一份才是真的重复。
        save(&ns, general_input("通用约定", "先看日志再看代码")).expect("save general");
        let general_dup = submit_candidate(
            &ns,
            CandidateInput {
                scope: SCOPE_GENERAL.to_string(),
                title: "又说一遍".to_string(),
                body: "先看日志再看代码".to_string(),
                ..CandidateInput::default()
            },
        )
        .unwrap_err();
        assert!(
            general_dup.to_string().contains("已经在记忆里"),
            "{general_dup}"
        );
    }

    /// 用户自己写下同一条之后，待确认里的重复候选被收掉（§7.2「用户纠正优先于旧候选」）。
    #[test]
    fn writing_the_same_body_yourself_retires_the_pending_duplicate() {
        let _home = TempHome::new("supersede");
        let ns = namespace();
        let candidate = submit_ok(
            &ns,
            CandidateInput {
                scope: SCOPE_PROJECT.to_string(),
                workspace_path: "/tmp/ws-a".to_string(),
                title: "会话提的".to_string(),
                body: "提交前跑 npm run check".to_string(),
                ..CandidateInput::default()
            },
        );

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
        let total = crate::memory::prune::MAX_CANDIDATES + 2;
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
            after.candidates.len() <= crate::memory::prune::MAX_CANDIDATES,
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
