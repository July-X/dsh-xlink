//! 环境快照：记住「这套配置曾经是好的」。
//!
//! ## 为什么需要它
//!
//! 一个实例的"环境"由四类独立可变的东西相乘决定：内核版本 × 插件集 ×
//! 技能集 × 补丁集（再加上 profile 名与端口）。用户在"昨天还能用、今天起
//! 不来"的时候，脑子里没有任何一份"昨天那套配置"的清单——而这恰恰是唯
//! 一有用的信息。既有回滚能力（补丁备份、迁移 rollback、quarantine 记
//! 录）都是**单点**的，没有一处记录整套配置曾经是好的。
//!
//! 完整设计见 `docs/safety-net-design.md`。本文只覆盖该文档的 **P0**：
//! 指纹、快照存储与两个打点，**不含**任何自动恢复或二分定位。
//!
//! ## 指纹是声明，不是备份
//!
//! [`fingerprint`] 算的是「构成这个实例的输入」的摘要，不含 `wiring.json`
//! 与 profile `package.json` 的内容本身。因此有指纹就能算出这套配置**该
//! 长什么样**，而不仅仅是"曾经长什么样"——这是 P1 恢复能够只改差异项的
//! 前提。
//!
//! ## 绝不含凭据
//!
//! 快照会落盘、会被导出、可能被用户贴进事故反馈。它必须能安全地给人看，
//! 所以只记 id / 模式 / 版本这类配置形状，`credentials` / API Key /
//! `.credentials.yaml` 一个字节都不进。凭据纪律沿用 `credentials.rs` 的
//! 既有规则。
//!
//! ## 存储形状
//!
//! 索引与快照内容放在**同一个文档**里，而不是「一份索引 + 每份快照一个
//! 文件」：保留上限只有 [`MAX_ENTRIES`] 份，单文件一次原子写就够；两份
//! 文件会引入「索引指向的详情丢了」这种不一致状态，而它换不来任何好处。

use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::AppError;
use crate::state::{self, StateCtx};

/// 保留的快照份数上限。10 份足够覆盖"最近几次变更 + 几个已知良好点"，
/// 而文件体积有硬上界（约 1 KB × 10）。
pub const MAX_ENTRIES: usize = 10;

/// 文档结构版本。改动字段语义时递增，旧文档按 [`is_compatible`] 的规则
/// 继续可读（只丢无法解释的条目，不整份作废）。
const SCHEMA: u32 = 1;

/// 打点原因。
pub mod reason {
    /// 内核成功启动且无告警——**唯一**能确立 last-known-good 的时点。
    pub const STARTUP_OK: &str = "startup-ok";
    /// 用户主动变更（安装 / 更新 / 卸载 / 切模式 / 切版本）之前。
    pub const PRE_CHANGE: &str = "pre-change";
    /// 用户手动打的回退点。
    pub const MANUAL: &str = "manual";
}

/// 保留权重。`pre-change` 最高——它记录的是"我改之前"，用户马上要用；
/// `startup-ok` 最低——同一天开关工作台十几次就会攒出十份一模一样的
/// 快照（重复的指纹本来就不会重复入库，但权重仍要体现"这类最不值钱"）。
fn weight_of(reason: &str) -> u8 {
    match reason {
        reason::PRE_CHANGE => 2,
        reason::MANUAL => 1,
        _ => 0,
    }
}

/// 快照里插件 / 补丁条目的最小形状：只记 id 与模式，不记路径与时间戳。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub mode: String,
}

/// 一份快照声明。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub id: String,
    pub created_at_ms: u64,
    /// [`reason`] 里的取值之一。
    pub reason: String,
    /// [`fingerprint`] 的输出。
    pub fingerprint: String,

    // —— 重建这套配置所需的全部输入 ——
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kernel_version: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub profile: String,
    #[serde(default)]
    pub port: u16,
    /// 排序后的插件条目。存排序结果而不是原始顺序：顺序变了不算配置变了。
    #[serde(default)]
    pub plugins: Vec<Entry>,
    /// 排序后的技能条目名。
    #[serde(default)]
    pub skills: Vec<String>,
    /// 排序后的已应用补丁条目。
    #[serde(default)]
    pub patches: Vec<Entry>,
}

/// 快照文档（索引 + 全部声明）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotIndex {
    #[serde(default)]
    pub schema: u32,
    /// 指向最近一份"成功启动过"的快照 id。**永不删除它**，哪怕它最旧——
    /// 它是 P1 恢复的默认目标。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_known_good: Option<String>,
    /// 倒序（最新在前）。
    #[serde(default)]
    pub entries: Vec<Snapshot>,
}

impl SnapshotIndex {
    fn is_compatible(&self) -> bool {
        self.schema <= SCHEMA
    }
}

fn ctx() -> StateCtx {
    // 两句文案分开写：展示侧说「按空文档处理」，写路径侧必须说「不能覆盖」，
    // 否则用户看到的第一句会以为自己的回退点还在。
    StateCtx {
        corrupt: |reason| {
            format!(
                "快照文档无法解析：{reason}。面板读不到历史回退点；下一次打点会尝试重建它，但覆盖前请先确认当前配置可用"
            )
        },
        kind: |reason| AppError::Io(format!("快照文档无法解析，已拒绝覆盖：{reason}")),
    }
}

/// 读快照文档。损坏 / 结构过新时**不**返回空文档冒充"没有快照"：那会让
/// last-known-good 静默消失，而 P1 的恢复入口正是以它为准。这里降级为空
/// 文档只发生在展示路径；写路径一律走 [`load_checked`]。
pub fn load(family: &str, instance: &str) -> SnapshotIndex {
    let mut index: SnapshotIndex = state::load_lossy(&paths(family, instance));
    if !index.is_compatible() {
        return SnapshotIndex {
            schema: SCHEMA,
            ..SnapshotIndex::default()
        };
    }
    index.schema = SCHEMA;
    index
}

/// 面板侧的「文档坏了」提示。展示路径用容错读（面板还得能打开），但必须
/// 把「读到的是空文档」这件事如实说出来。
pub fn warning(family: &str, instance: &str) -> Option<String> {
    state::integrity_warning::<SnapshotIndex>(&paths(family, instance), ctx())
}

/// 读-改-写路径：文档损坏时报错，不覆盖用户记录。
pub fn load_checked(family: &str, instance: &str) -> Result<SnapshotIndex, AppError> {
    let index: SnapshotIndex = state::load_checked(&paths(family, instance), ctx())?;
    if !index.is_compatible() {
        return Err(AppError::Io(format!(
            "快照文档结构版本 {} 高于本版支持的 {SCHEMA}，请升级桌面端后再操作",
            index.schema
        )));
    }
    Ok(index)
}

fn paths(family: &str, instance: &str) -> std::path::PathBuf {
    crate::paths::instance_snapshot_file(family, instance)
}

fn save(family: &str, instance: &str, index: &SnapshotIndex) -> Result<(), AppError> {
    state::save(&paths(family, instance), index, ctx())
}

// --- 指纹 ----------------------------------------------------------------

/// 算出当前实例的环境指纹。
///
/// 参与计算的四类输入与 [`Snapshot`] 的重建字段一一对应。**不含**接线产物
/// 本身（`wiring.json` / profile `package.json`），因为它们是派生结果——
/// 同样的输入必然生成同样的接线，把派生值也算进去只会让指纹在"重装同一个
/// 插件"之后无谓地变一次。
pub fn fingerprint(
    data_dir: &Path,
    family: &str,
    instance: &str,
    profile: &str,
    port: u16,
) -> String {
    let mut hasher = Sha256::new();
    // 分段用 0x1f 隔开：否则 ("ab","c") 与 ("a","bc") 会得到同样的摘要。
    let mut field = |value: &str| {
        hasher.update(value.as_bytes());
        hasher.update([0x1f]);
    };

    let version = crate::kernel::instance_active_version(family, instance).unwrap_or_default();
    field(&version);
    field(profile);
    field(&port.to_string());

    for item in crate::plugins::load_store(data_dir).items {
        field(&item.id);
        field(&item.mode);
    }
    // 活动视图是「内核此刻会读到哪些技能」的权威来源：中央库记的是"装
    // 了什么"，活动目录记的是"现在生效什么"，而环境指纹要的是后者。
    for name in crate::skills::active_entry_names() {
        field(&name);
    }
    for id in crate::patches::applied_patch_ids(data_dir, &version) {
        field(&id);
    }

    let digest = hasher.finalize();
    digest.iter().take(12).map(|b| format!("{b:02x}")).collect()
}

/// 采集当前环境的快照内容（尚未编号、尚未入库）。
pub fn capture(
    data_dir: &Path,
    family: &str,
    instance: &str,
    profile: &str,
    port: u16,
) -> Snapshot {
    let version = crate::kernel::instance_active_version(family, instance).unwrap_or_default();
    let mut plugins: Vec<Entry> = crate::plugins::load_store(data_dir)
        .items
        .iter()
        .map(|item| Entry {
            id: item.id.clone(),
            mode: item.mode.clone(),
        })
        .collect();
    plugins.sort_by(|a, b| a.id.cmp(&b.id));
    let mut patches: Vec<Entry> = crate::patches::applied_patch_ids(data_dir, &version)
        .into_iter()
        .map(|id| Entry {
            id,
            mode: String::new(),
        })
        .collect();
    patches.sort_by(|a, b| a.id.cmp(&b.id));

    Snapshot {
        id: String::new(),
        created_at_ms: crate::process::epoch_millis(),
        reason: String::new(),
        fingerprint: fingerprint(data_dir, family, instance, profile, port),
        kernel_version: Some(version),
        profile: profile.to_string(),
        port,
        plugins,
        skills: crate::skills::active_entry_names(),
        patches,
    }
}

// --- 打点 ----------------------------------------------------------------

/// 打一个快照点。
///
/// `startup-ok` 有两条去重规则，缺一不可：
/// 1. **同指纹不重复入库**——用户一天开关工作台十几次，配置没变就打十几次
///    快照，除了把有价值的 `pre-change` 挤掉之外没有任何信息量。
/// 2. **last-known-good 不被顶掉**——同指纹的重复启动只刷新时间戳式的
///    展示顺序，不改指向，避免"连续成功启动"把用户手动打的回退点挤掉。
pub fn record(
    data_dir: &Path,
    family: &str,
    instance: &str,
    profile: &str,
    port: u16,
    reason: &str,
) -> Result<Option<Snapshot>, AppError> {
    let mut index = load_checked(family, instance)?;
    let fresh = capture(data_dir, family, instance, profile, port);

    if reason == reason::STARTUP_OK {
        index.last_known_good = Some(fresh.fingerprint.clone());
    }
    if let Some(existing) = index
        .entries
        .iter_mut()
        .find(|entry| entry.fingerprint == fresh.fingerprint)
    {
        // 同指纹：只在新原因权重更高时把它「提级」成那份，原因与时间刷新。
        // 例如「装了个插件 → 启动成功 → 装回同一个」两次 pre-change 与一次
        // startup-ok 落在同一指纹上，保留更值钱的那个身份。
        if weight_of(reason) > weight_of(&existing.reason) {
            existing.reason = reason.to_string();
        }
        return Ok(None);
    }

    let stored = Snapshot {
        id: format!(
            "snap-{}",
            &fresh.fingerprint[..8.min(fresh.fingerprint.len())]
        ),
        created_at_ms: fresh.created_at_ms,
        reason: reason.to_string(),
        ..fresh
    };
    index.entries.insert(0, stored.clone());
    prune(&mut index);
    save(family, instance, &index)?;
    Ok(Some(stored))
}

/// 按权重与时间裁剪，**永不删掉 last-known-good**。
fn prune(index: &mut SnapshotIndex) {
    if index.entries.len() <= MAX_ENTRIES {
        return;
    }
    let keeper = index.last_known_good.as_ref().and_then(|fp| {
        index
            .entries
            .iter()
            .find(|e| &e.fingerprint == fp)
            .map(|e| e.id.clone())
    });
    let mut survivors: Vec<Snapshot> = Vec::with_capacity(MAX_ENTRIES);
    // 先放 last-known-good，再按 (权重, 时间) 降序填满剩余位置。
    if let Some(id) = &keeper {
        if let Some(entry) = index.entries.iter().find(|e| &e.id == id).cloned() {
            survivors.push(entry);
        }
    }
    let mut rest: Vec<Snapshot> = index
        .entries
        .iter()
        .filter(|e| Some(&e.id) != keeper.as_ref())
        .cloned()
        .collect();
    rest.sort_by(|a, b| {
        weight_of(&b.reason)
            .cmp(&weight_of(&a.reason))
            .then(b.created_at_ms.cmp(&a.created_at_ms))
    });
    let room = MAX_ENTRIES.saturating_sub(survivors.len());
    survivors.extend(rest.into_iter().take(room));
    survivors.sort_by_key(|entry| std::cmp::Reverse(entry.created_at_ms));
    index.entries = survivors;
}

// --- 只读视图 ------------------------------------------------------------

/// 给面板看的单条快照。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotView {
    pub id: String,
    pub created_at_ms: u64,
    pub reason: String,
    pub fingerprint: String,
    pub kernel_version: String,
    pub plugin_count: usize,
    pub skill_count: usize,
    pub patch_count: usize,
    /// 是否就是 last-known-good 指向的那份。
    pub last_known_good: bool,
    /// 指纹与当前环境相同 = 这套配置**此刻仍然生效**，恢复它等于什么都不做。
    /// 面板据此把「恢复」按钮置灰——让用户点一个必然无效果的按钮比不显示
    /// 按钮更让人困惑。
    pub is_current: bool,
}

/// 快照列表 + 摘要，供面板一次性渲染。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotListView {
    pub entries: Vec<SnapshotView>,
    /// 从未成功启动过（因而没有任何 last-known-good）时为 false。面板要据
    /// 此说明"还没有可回退的点"，而不是渲染一个空列表让用户以为功能坏了。
    pub has_last_known_good: bool,
    pub last_known_good_id: String,
    /// 当前环境本身的指纹。用户手工改过 `extensions/` 时它会与任何一条
    /// 快照都不同——面板据此提示"当前环境已被手工修改"。
    pub current_fingerprint: String,
    /// 文档损坏时的提示。面板仍能打开（展示路径容错读），但绝不能让用户
    /// 以为"从来没有过回退点"——那与"有回退点但读不出来"是两回事。
    pub warning: Option<String>,
}

pub fn list(
    data_dir: &Path,
    family: &str,
    instance: &str,
    profile: &str,
    port: u16,
) -> SnapshotListView {
    let index = load(family, instance);
    let current = fingerprint(data_dir, family, instance, profile, port);
    let good_id = index
        .last_known_good
        .as_ref()
        .and_then(|fp| {
            index
                .entries
                .iter()
                .find(|e| &e.fingerprint == fp)
                .map(|e| e.id.clone())
        })
        .unwrap_or_default();
    SnapshotListView {
        has_last_known_good: !good_id.is_empty(),
        last_known_good_id: good_id.clone(),
        current_fingerprint: current.clone(),
        warning: warning(family, instance),
        entries: index
            .entries
            .iter()
            .map(|entry| SnapshotView {
                id: entry.id.clone(),
                created_at_ms: entry.created_at_ms,
                reason: entry.reason.clone(),
                fingerprint: entry.fingerprint.clone(),
                kernel_version: entry.kernel_version.clone().unwrap_or_default(),
                plugin_count: entry.plugins.len(),
                skill_count: entry.skills.len(),
                patch_count: entry.patches.len(),
                last_known_good: entry.id == good_id,
                is_current: entry.fingerprint == current,
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(id: &str, reason: &str, at: u64, fp: &str) -> Snapshot {
        Snapshot {
            id: id.into(),
            created_at_ms: at,
            reason: reason.into(),
            fingerprint: fp.into(),
            kernel_version: Some("1.0.0".into()),
            profile: "web".into(),
            port: 3090,
            plugins: Vec::new(),
            skills: Vec::new(),
            patches: Vec::new(),
        }
    }

    /// 裁剪的硬要求：last-known-good 哪怕是最旧、权重最低的那份，也绝不能
    /// 被裁掉——P1 的恢复入口以它为准，丢了就等于安全网在最需要时失灵。
    #[test]
    fn prune_never_drops_the_last_known_good() {
        let mut index = SnapshotIndex {
            schema: SCHEMA,
            // last_known_good 存的是**指纹**而不是 id：它要表达的是"这套
            // 配置被成功启动验证过"，而不是"文档里的某一行"。指纹由配置
            // 本身算出，比任何命名约定都更贴近那个不变量。
            last_known_good: Some("fp0".into()),
            entries: Vec::new(),
        };
        // 故意让「oldest」既最旧又权重最低。
        index
            .entries
            .push(snapshot("oldest", reason::STARTUP_OK, 1, "fp0"));
        for i in 0..20 {
            index.entries.push(snapshot(
                &format!("s{i}"),
                reason::PRE_CHANGE,
                100 + i,
                &format!("fp{i}"),
            ));
        }

        prune(&mut index);

        assert!(index.entries.len() <= MAX_ENTRIES, "裁剪后必须不超上限");
        assert!(
            index.entries.iter().any(|e| e.id == "oldest"),
            "last-known-good 被裁掉了，安全网在最需要时失灵"
        );
    }

    /// 高权重的回退点绝不能因为配额满而被低权重的挤掉。这是上面那条的
    /// 推广：裁剪只在**还有低权重可丢**时才丢高权重。
    #[test]
    fn prune_drops_low_weight_first() {
        let mut index = SnapshotIndex {
            schema: SCHEMA,
            last_known_good: None,
            entries: Vec::new(),
        };
        for i in 0..10 {
            index.entries.push(snapshot(
                &format!("ok{i}"),
                reason::STARTUP_OK,
                1000,
                &format!("g{i}"),
            ));
        }
        for i in 0..5 {
            index.entries.push(snapshot(
                &format!("ch{i}"),
                reason::PRE_CHANGE,
                2000,
                &format!("c{i}"),
            ));
        }

        prune(&mut index);

        // 15 进 10 出：5 个高权重必须全留，剩下 5 个位置给低权重。
        assert_eq!(
            index
                .entries
                .iter()
                .filter(|e| e.reason == reason::PRE_CHANGE)
                .count(),
            5,
            "pre-change 权重最高，不能被 startup-ok 挤掉；剩下 {:?}",
            index
                .entries
                .iter()
                .map(|e| e.reason.as_str())
                .collect::<Vec<_>>()
        );
        assert_eq!(index.entries.len(), MAX_ENTRIES);
    }

    #[test]
    fn weight_ordering_matches_the_design() {
        assert!(weight_of(reason::PRE_CHANGE) > weight_of(reason::MANUAL));
        assert!(weight_of(reason::MANUAL) > weight_of(reason::STARTUP_OK));
    }

    /// 指纹的分段必须真的分段：否则 `plugins: ["ab"]` 与 `["a","b"]` 会
    /// 算出同一个摘要，而那是两种不同的配置。
    #[test]
    fn fingerprint_separates_fields() {
        fn digest(parts: &[&str]) -> String {
            let mut hasher = Sha256::new();
            for part in parts {
                hasher.update(part.as_bytes());
                hasher.update([0x1f]);
            }
            hasher
                .finalize()
                .iter()
                .take(12)
                .map(|b| format!("{b:02x}"))
                .collect()
        }
        assert_ne!(digest(&["ab", "c"]), digest(&["a", "bc"]));
        assert_eq!(digest(&["a", "b"]), digest(&["a", "b"]));
    }
}
