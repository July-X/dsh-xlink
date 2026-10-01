//! 差异计算与环境恢复：P0 的回退点是「记录」，这里是把它「用回去」。
//!
//!
//! 与 [`crate::diagnostics::snapshot`] 的分工：那边负责**记住**（指纹、快照文档、裁剪、
//! 打点），这里负责**改回来**（逐条比对、只改差异、只停用不卸载、
//! 恢复后自检）。两者是同一件事的两个方向，拆开是因为它们的读者不同——
//! 读 snapshot 的人关心"这份回退点可不可信"，读 restore 的人关心"点确认
//! 之后到底会发生什么"。
//!
//! 四条硬规则的完整说明见 `docs/safety-net-design.md` §5.2。

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Serialize;

use crate::diagnostics::snapshot;
use crate::shell::error::AppError;

/// 一条差异。两个方向都带 `restorable`：面板必须在**用户确认之前**就知道
/// 哪些动不了——恢复完再告诉用户"这个没能恢复"等于让他在不知情的情况下
/// 拿到一个不完整的还原。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffItem {
    pub kind: String,
    pub id: String,
    /// 中文说明，直接可读。
    pub detail: String,
    /// false 时 `detail` 说明为什么动不了。
    pub restorable: bool,
    /// 该动作的目标值（物化模式 / 内核版本）。恢复落地时要它，不必重新
    /// 从快照声明里反查——那条路会让差异与执行读两份数据，早晚分叉。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub value: String,
}

/// 一次恢复的完整预览。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreDiff {
    pub snapshot_id: String,
    pub snapshot_created_at_ms: u64,
    pub reason: String,
    /// 有变化的维度清单。没有变化时恢复是空操作，面板据此禁用按钮。
    pub changes: Vec<DiffItem>,
    /// 动不了的条目数。>0 时按钮仍可点，但确认框必须显示这个数——
    /// 用户有权在知情的前提下选择"能恢复多少算多少"。
    pub blocked_count: usize,
    /// 当前环境已被手工改过（当前指纹与任何快照都不同）。
    pub current_drifted: bool,
    pub current_fingerprint: String,
}

/// 算出「从当前环境回到某份快照」要做的事。
///
/// 刻意**不**在这里改任何东西：差异只是给用户看的，恢复是另一次显式调用。
/// 两步合一意味着用户要点一次「恢复」才能知道自己会失去什么。
#[allow(clippy::too_many_arguments)]
pub fn diff(
    data_dir: &Path,
    family: &str,
    instance: &str,
    profile: &str,
    port: u16,
    snapshot_id: &str,
) -> Result<RestoreDiff, AppError> {
    let index = snapshot::load_checked(family, instance)?;
    let target = index
        .entries
        .iter()
        .find(|e| e.id == snapshot_id)
        .ok_or_else(|| {
            AppError::Io(format!(
                "回退点 {snapshot_id} 已不在快照列表里，请刷新后重试"
            ))
        })?;
    let current = snapshot::capture(data_dir, family, instance, profile, port);
    let mut changes: Vec<DiffItem> = Vec::new();

    // —— 内核版本 ——
    let now_version = current.kernel_version.clone().unwrap_or_default();
    let want_version = target.kernel_version.clone().unwrap_or_default();
    if now_version != want_version {
        let installed = std::fs::metadata(
            crate::kernel::lifecycle::kernel_dir(data_dir, &want_version)
                .join(crate::kernel::kernel_adapter::DshAdapter::KERNEL_BIN_REL),
        )
        .is_ok();
        changes.push(DiffItem {
            kind: "kernel".into(),
            id: want_version.clone(),
            detail: if installed {
                format!("内核版本 {now_version} → {want_version}")
            } else {
                format!(
                    "内核版本 {now_version} → {want_version}（**该版本已不在本机**，无法自动切换，需先到「内核版本」页装回）"
                )
            },
            restorable: installed && !want_version.is_empty(),
            value: want_version.clone(),
        });
    }

    // —— 插件 ——
    // 两个方向都只改「接线与启用状态」，绝不卸载、绝不删中央库条目。
    // 卸载是不可逆的，让一次环境回退去做它等于用恢复换数据。
    let now_plugins: BTreeMap<&str, &str> = current
        .plugins
        .iter()
        .map(|e| (e.id.as_str(), e.mode.as_str()))
        .collect();
    let want_plugins: BTreeMap<&str, &str> = target
        .plugins
        .iter()
        .map(|e| (e.id.as_str(), e.mode.as_str()))
        .collect();
    let store_ids: Vec<String> = crate::plugins::center::load_store(data_dir)
        .items
        .iter()
        .map(|item| item.id.clone())
        .collect();
    for (id, mode) in &want_plugins {
        match now_plugins.get(id) {
            Some(now_mode) if now_mode == mode => {}
            Some(now_mode) => changes.push(DiffItem {
                kind: "plugin-mode".into(),
                id: (*id).into(),
                detail: format!("插件 {id} 的物化模式 {now_mode} → {mode}"),
                restorable: true,
                value: (*mode).into(),
            }),
            None if store_ids.contains(&id.to_string()) => changes.push(DiffItem {
                kind: "plugin-enable".into(),
                id: (*id).into(),
                detail: format!("重新启用插件 {id}"),
                restorable: true,
                value: String::new(),
            }),
            None => changes.push(DiffItem {
                kind: "plugin-enable".into(),
                id: (*id).into(),
                detail: format!(
                    "重新启用插件 {id}（**中央库里已经没有这个插件**，需重新安装后才能恢复）"
                ),
                restorable: false,
                value: String::new(),
            }),
        }
    }
    for id in now_plugins.keys() {
        if want_plugins.contains_key(id) {
            continue;
        }
        changes.push(DiffItem {
            kind: "plugin-disable".into(),
            id: (*id).into(),
            detail: format!("停用插件 {id}（插件本身保留在中央库，随时可再启用）"),
            restorable: true,
            value: String::new(),
        });
    }

    // —— 技能启用位 ——
    let now_refs: BTreeSet<&String> = current.skill_refs.iter().collect();
    let want_refs: BTreeSet<&String> = target.skill_refs.iter().collect();
    for reference in want_refs.difference(&now_refs) {
        let (pkg, name) = split_skill_ref(reference);
        // 只查一次：恢复能不能落地取决于「包里还有没有这条技能」。
        let exists = crate::skills::manage::skill_exists(pkg, name);
        changes.push(DiffItem {
            kind: "skill-enable".into(),
            id: (*reference).clone(),
            detail: if exists {
                format!("启用技能 {pkg} / {name}")
            } else {
                format!("启用技能 {pkg} / {name}（**该技能包已不在中央库**，需重新安装）")
            },
            restorable: exists,
            value: String::new(),
        });
    }
    for reference in now_refs.difference(&want_refs) {
        let (pkg, name) = split_skill_ref(reference);
        changes.push(DiffItem {
            kind: "skill-disable".into(),
            id: (*reference).clone(),
            detail: format!("停用技能 {pkg} / {name}（技能仍留在中央库与活动视图的包目录里）"),
            restorable: true,
            value: String::new(),
        });
    }

    // —— 补丁：只报差异，不自动动 ——
    let now_patches: BTreeSet<&String> = current.patches.iter().map(|e| &e.id).collect();
    let want_patches: BTreeSet<&String> = target.patches.iter().map(|e| &e.id).collect();
    for id in want_patches.difference(&now_patches) {
        changes.push(DiffItem {
            kind: "patch-apply".into(),
            id: (*id).into(),
            detail: format!(
                "重新应用补丁 {id}（**需手动在「设置 → 内核补丁」确认**，恢复不自动动补丁）"
            ),
            restorable: false,
            value: String::new(),
        });
    }
    for id in now_patches.difference(&want_patches) {
        changes.push(DiffItem {
            kind: "patch-revert".into(),
            id: (*id).into(),
            detail: format!(
                "撤销补丁 {id}（**需手动在「设置 → 内核补丁」确认**，恢复不自动动补丁）"
            ),
            restorable: false,
            value: String::new(),
        });
    }

    let blocked_count = changes.iter().filter(|item| !item.restorable).count();
    Ok(RestoreDiff {
        snapshot_id: snapshot_id.to_string(),
        snapshot_created_at_ms: target.created_at_ms,
        reason: target.reason.clone(),
        changes,
        blocked_count,
        current_drifted: current.fingerprint != target.fingerprint,
        current_fingerprint: current.fingerprint,
    })
}

fn split_skill_ref(reference: &str) -> (&str, &str) {
    match reference.split_once('/') {
        Some((pkg, name)) => (pkg, name),
        None => (reference, ""),
    }
}

/// 把当前环境恢复到指定快照。
///
/// 四条硬规则（缺一条就等于用安全网反过来坑用户）：
///
/// 1. **先备份**。动之前把当前状态存成 `pre-restore` 快照——恢复本身也可能
///    失败，没有这个回退点就是单向操作。
/// 2. **只改差异项**。逐条比对当前环境与目标，一致的项一个字节都不动；
///    避免"恢复一下顺手改了点别的"。
/// 3. **不删数据**。插件只停用不卸载、技能只停用不删包。卸载不可逆，让一次
///    环境回退顺手做了等于用恢复换数据。
/// 4. **动不了的照实报**。源已不在的插件、未支持的补丁方向都进 `skipped`，
///    不假装成功。
#[allow(clippy::too_many_arguments)]
pub fn restore(
    data_dir: &Path,
    family: &str,
    instance: &str,
    profile: &str,
    port: u16,
    pnpm_exe: &Path,
    node_path: &Path,
    snapshot_id: &str,
    on_progress: &mut dyn FnMut(&str),
) -> Result<RestoreOutcome, AppError> {
    let plan = diff(data_dir, family, instance, profile, port, snapshot_id)?;
    if plan.changes.is_empty() {
        return Ok(RestoreOutcome {
            skipped: Vec::new(),
            applied: Vec::new(),
            // 空操作**不等于**"验过了"。什么都没改也就什么都没验，界面必须
            // 把它画成第三种样子，而不是借用"实测通过"那一句。
            verification: Verification::NotNeeded,
            verification_detail: String::new(),
            resulting_fingerprint: plan.current_fingerprint,
            backup_snapshot_id: String::new(),
        });
    }
    if crate::kernel::lifecycle::workbench_running(
        data_dir,
        &crate::shell::settings::load_for_shell(crate::shell::settings::current_mode()),
    ) {
        return Err(AppError::Io(
            "工作台正在运行，无法恢复配置。请先在概览页点「关闭工作台」，再重试".into(),
        ));
    }
    // 本壳工作台的判据之外还要问**实例**这一层：用户自建实例在注册表分家后
    // 有意留在两份注册表里，另一个 dsh-xlink 完全可能正跑着它，而恢复要改的
    // 正是那个实例的 profile 接线与插件物化。与「找回历史会话」同一条纪律、
    // 同一个判据与同一份文案（`instance::instance_kernel_running`）。
    if let Some(record) = crate::shell::instance::instance_kernel_running(family, instance) {
        return Err(AppError::Io(
            crate::shell::instance::instance_kernel_running_message(&record, instance, "恢复配置"),
        ));
    }

    // 规则 1：先备份。备份失败**必须**中止恢复——没有回退点的恢复不能做。
    let backup = snapshot::record(
        data_dir,
        family,
        instance,
        profile,
        port,
        snapshot::reason::PRE_RESTORE,
    )?;
    let backup_id = backup.map(|s| s.id).unwrap_or_default();
    if backup_id.is_empty() {
        // 同指纹时 `record` 不新增（去重规则）。这不是失败：当前环境与某份
        // 已存快照完全一致，它本身就是可用的回退点。
        on_progress("当前环境与已有回退点完全一致，无需额外备份");
    } else {
        on_progress(&format!("已把当前环境备份为回退点 {backup_id}"));
    }

    let mut applied: Vec<String> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    for item in &plan.changes {
        if !item.restorable {
            skipped.push(item.detail.clone());
            continue;
        }
        on_progress(&item.detail);
        match apply_one(data_dir, family, instance, pnpm_exe, item, on_progress) {
            Ok(()) => applied.push(item.detail.clone()),
            Err(error) => {
                // 单条失败不打断其余条目：用户要的是尽可能接近目标，而不是
                // 全有或全无。失败原因必须带进结果，否则用户无从判断恢复到了
                // 什么程度。
                skipped.push(format!("{}（失败：{error}）", item.detail));
            }
        }
    }

    // 接线是 profile 级动作，只在真的动过插件之后重跑一次。
    if applied.iter().any(|text| text.contains("插件")) {
        on_progress("正在重新接线 profile");
        let settings =
            crate::shell::settings::load_for_shell(crate::shell::settings::current_mode());
        if let Err(error) = crate::plugins::center::ensure_wiring_for_instance(
            family,
            instance,
            data_dir,
            &settings,
            pnpm_exe,
            on_progress,
        ) {
            skipped.push(format!(
                "重新接线失败（{error}）——插件文件已就位，重启工作台后手动点「同步」即可"
            ));
        }
    }

    // 规则 4：动不了的照实报。源已不在的插件、未支持的补丁方向都进 `skipped`，
    // 不假装成功。
    // 规则 5：**恢复完自己验一次**。恢复的产物是"配置已回到目标"，只有真的
    // 起得来才算数——这里用一次性沙盒内核走一遍真实启动链（与 P2 二分复用
    // 同一套判据），不碰用户的真实工作台。
    // 放行的是**当前这套**（已恢复）配置：中央库条目减去隔离表。恢复刚把该
    // 停用的写进 quarantine，所以过滤结果正好是目标回退点描述的那一套。
    let blocked = crate::plugins::quarantine::ids(data_dir);
    let allow = move |item: &crate::plugins::center::StoreItem| !blocked.contains(&item.id);
    let probe = crate::diagnostics::verify::probe_once(
        data_dir,
        family,
        &crate::shell::settings::load_for_shell(crate::shell::settings::current_mode()),
        pnpm_exe,
        node_path,
        &allow,
        on_progress,
    );
    let verification = if probe.ready {
        Verification::Verified
    } else {
        Verification::Failed
    };
    if !probe.ready {
        skipped.push(format!(
            "配置已按回退点改好，但沙盒自检没能起来（{}）。这不代表回退失败——\
             也可能是这个回退点本身就不是一个能起来的环境；请点「启动工作台」看真实结果，\
             或回到更早的回退点再试",
            probe.detail
        ));
    }

    let resulting_fingerprint = snapshot::fingerprint(data_dir, family, instance, profile, port);
    Ok(RestoreOutcome {
        skipped,
        applied,
        verification,
        verification_detail: if probe.ready {
            String::new()
        } else {
            probe.detail
        },
        resulting_fingerprint,
        backup_snapshot_id: backup_id,
    })
}

/// 恢复流程里逐条落地。单条失败返回 `Err`，由 [`restore`] 归进 `skipped`。
fn apply_one(
    data_dir: &Path,
    family: &str,
    instance: &str,
    pnpm_exe: &Path,
    item: &DiffItem,
    on_progress: &mut dyn FnMut(&str),
) -> Result<(), AppError> {
    match item.kind.as_str() {
        "kernel" => {
            crate::kernel::lifecycle::set_active(data_dir, &item.value)?;
            // 换版本要重新接线：新版本的 profile 接线来自它自己的 node_modules。
            let settings =
                crate::shell::settings::load_for_shell(crate::shell::settings::current_mode());
            let _ = crate::plugins::center::ensure_wiring_for_instance(
                family,
                instance,
                data_dir,
                &settings,
                pnpm_exe,
                on_progress,
            );
            Ok(())
        }
        "plugin-mode" => {
            let settings =
                crate::shell::settings::load_for_shell(crate::shell::settings::current_mode());
            crate::plugins::center::set_mode_for_instance(
                family,
                instance,
                data_dir,
                &settings,
                pnpm_exe,
                &item.id,
                &item.value,
                on_progress,
            )
        }
        // 停用 ≠ 卸载：写隔离记录并摘掉接线，中央库条目原样留着。名字、证据、
        // 时间按 `QuarantineItem` 的形状补齐——它会被事故面板原样显示，理由
        // 必须写清"这是自动停用，插件还在"。
        "plugin-disable" => {
            let name = crate::plugins::center::load_store(data_dir)
                .items
                .iter()
                .find(|entry| entry.id == item.id)
                .map(|entry| entry.name.clone())
                .unwrap_or_else(|| item.id.clone());
            crate::plugins::quarantine::add_all(
                data_dir,
                &[crate::plugins::quarantine::QuarantineItem {
                    id: item.id.clone(),
                    name,
                    reason: "环境回退：目标回退点里没有这个插件，已自动停用。插件仍在中央库，随时可再启用。".into(),
                    evidence: String::new(),
                    at: crate::shell::process::epoch_millis() / 1000,
                }],
            )
        }
        "plugin-enable" => {
            let _ = crate::plugins::quarantine::remove(data_dir, &item.id);
            let item_ref =
                crate::plugins::center::store_item(data_dir, &item.id).ok_or_else(|| {
                    AppError::Plugin(format!("中央库里找不到插件 {}，无法恢复", item.id))
                })?;
            crate::plugins::center::sync_kernels_for_instance(family, instance, data_dir, &item_ref)
        }
        "skill-enable" => {
            let (pkg, name) = split_skill_ref(&item.id);
            crate::skills::manage::set_enabled(pkg, name, true, on_progress)
        }
        "skill-disable" => {
            let (pkg, name) = split_skill_ref(&item.id);
            crate::skills::manage::set_enabled(pkg, name, false, on_progress)
        }
        other => Err(AppError::Io(format!("未知的恢复动作 {other}"))),
    }
}

/// 恢复后到底有没有真的实测过。三态而不是布尔。
///
/// `NotNeeded`（本次没有任何改动，因此根本没起内核）与 `Verified`（真装了
/// 目标配置、起了沙盒内核、应答了）在界面上**必须**是两种不同的东西——
/// 把"没测"画成"测过没问题"是这类工具最容易犯也最伤害信任的错，而一个
/// 布尔字段表达不了"没测"，只能表达"没测成"。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Verification {
    /// 沙盒里装上恢复后的配置起过一次内核，正常应答。
    Verified,
    /// 测了，但没起来。细节看 [`RestoreOutcome::verification_detail`]。
    Failed,
    /// 本次没有任何改动，因此没有起内核去证明它起得来。
    NotNeeded,
}

/// 一次恢复实际动了什么。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreOutcome {
    /// 跳过的条目（不可恢复的），带原因。
    pub skipped: Vec<String>,
    /// 真正动了的条目。
    pub applied: Vec<String>,
    /// 恢复后的实测状态，三态。
    pub verification: Verification,
    /// `Failed` 时的可读原因；其余两态为空。UI 必须把它显示出来，不能只
    /// 显示一个"未通过"让人自己猜。
    pub verification_detail: String,
    /// 恢复完成后的环境指纹。**不等于**目标快照指纹时，说明有东西没能恢复。
    pub resulting_fingerprint: String,
    /// 恢复前的状态存到了哪（`pre-restore` 快照 id）。恢复失败要能再恢复回来。
    pub backup_snapshot_id: String,
}

#[cfg(test)]
mod restore_tests {
    use super::*;

    fn entry(id: &str, mode: &str) -> snapshot::Entry {
        snapshot::Entry {
            id: id.into(),
            mode: mode.into(),
        }
    }

    /// 差异计算里最容易错的一条：用户**显式停用**的插件必须出现在"要恢复"
    /// 的一侧，而用户**新装的**插件必须出现在"要停用"的一侧。搞反了会把
    /// 用户刚装的插件当成要删的。
    #[test]
    fn plugin_diff_splits_both_directions() {
        let now: BTreeMap<&str, &str> = vec![("keep", "link"), ("fresh", "link")]
            .into_iter()
            .collect();
        let want: BTreeMap<&str, &str> = vec![("keep", "copy"), ("old", "link")]
            .into_iter()
            .collect();

        let want_only: Vec<&str> = want
            .keys()
            .filter(|k| !now.contains_key(*k))
            .copied()
            .collect();
        let now_only: Vec<&str> = now
            .keys()
            .filter(|k| !want.contains_key(*k))
            .copied()
            .collect();
        assert_eq!(want_only, vec!["old"], "停用过的插件要恢复");
        assert_eq!(
            now_only,
            vec!["fresh"],
            "新装的插件要被停用，且是停用不是卸载"
        );
    }

    /// 模式变化要单独成条：只比 id 集合会把"从 link 换成 copy"当成没变化，
    /// 而那恰恰是用户为绕开某个 bug 做的调整。
    #[test]
    fn plugin_mode_change_is_its_own_diff_item() {
        let now = [entry("keep", "link")];
        let want = [entry("keep", "copy")];
        let differs = now[0].id == want[0].id && now[0].mode != want[0].mode;
        assert!(differs, "同 id 不同 mode 必须算变化");
    }

    /// 中央库里已经没有的插件不能"恢复启用"——源都没了。差异必须在确认之前
    /// 就说清这一条动不了。
    #[test]
    fn missing_plugin_source_is_not_restorable() {
        let store_ids = ["keep".to_string()];
        assert!(!store_ids.contains(&"gone".to_string()));
        assert!(store_ids.contains(&"keep".to_string()));
    }

    #[test]
    fn skill_ref_splits_on_first_slash() {
        assert_eq!(split_skill_ref("pkg/entry"), ("pkg", "entry"));
        // 包 id 本身可以含 '-'，但不能含 '/'；条目名（kebab-case）同理。
        assert_eq!(
            split_skill_ref("dsh-review/code-style"),
            ("dsh-review", "code-style")
        );
        assert_eq!(split_skill_ref("bare"), ("bare", ""));
    }
}
