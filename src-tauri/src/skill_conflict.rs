//! 技能「活动视图里被同名条目占住」的判据与出路，形状与 [`crate::skill_shadow`]
//! 一致：判据是纯函数，动手前重算一次，动手只改名。
//!
//! `skills::ensure_entry` 撞到「目标位置已被占用、且不属本商店所有」时报
//! `技能名冲突`，而这条错误过去唯一的出路是让用户自己去资源管理器里删。
//! 2026-09-30 真机复现：活动视图里躺着 humanizer **v3.0.0** 的旧副本（普通
//! 文件、无指纹），中央库当天已更新到 v3.1.0，点启用只弹出一个只有「关闭」
//! 的对话框——判据已经精确到具体文件，按钮却不存在。
//!
//! 三条纪律，任何一条都不能为了「让按钮好点」而放宽：
//!
//! - **判据就是 `ensure_entry` 的拒绝条件本身**，不另立标准：
//!   `entry_is_owned` 为假**且** `identical_unowned_copy` 为假。少一条判据，
//!   按钮就会出现在本来能启用的技能上；多一条，警告就会在启用根本不会失败
//!   的地方响。两条都是「精确到具体文件之后按钮才有用」的前提。
//! - **只改名，不删除**。`identical_unowned_copy` 那条路之所以敢直接删，是
//!   因为内容与中央库源逐字节相同（不可能是用户的工作成果）；而本模块处理
//!   的正是**内容不同**的那些——用户完全可能改过它，现场必须留。
//! - **版本证据只用于解释，不用于判定**。两份 frontmatter 的
//!   `metadata.version` 不同时，那份活动副本多半是升级前的旧版，但「多半」
//!   不是证据，所以文案只说「活动视图里是 vX、技能库是 vY」，绝不据此收编。
//!
//! 不需要停工作台：内核的 watcher 盯着活动根，移走之后壳管理的启停立刻可用。

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::ipc::Channel;
use tauri::AppHandle;

use crate::error::AppError;
use crate::skills::{self, SkillEntry, SkillStore};

/// 落盘用的逻辑名，用户在「查看日志」面板里按它检索。
const CONFLICT_LOG: &str = "skill-conflict";

/// 活动视图里占住位置、且不属技能库所有的一条同名条目。
///
/// 这是**给人读的告警文案**之外的第二份数据：面板据它决定要不要给按钮，
/// **不得**去解析 `warning` 字符串（与 `SkillStatus.shadowed` 同一纪律）。
#[derive(Debug, Clone, Serialize)]
pub struct ConflictEntry {
    /// 技能名（活动根中的条目名）。
    pub skill: String,
    /// 占住位置的那份文件或目录（绝对路径，由后端给出，前端不拼路径）。
    pub path: String,
    /// 这条冲突的来由，一句话。会尽量带上两边的版本号。
    pub detail: String,
}

/// 面板用的只读视图。判据为空时返回空列表——UI 据此决定要不要给按钮。
pub fn list() -> Vec<ConflictEntry> {
    let home = skills::resolve_home();
    list_for(
        &crate::paths::skills_active_root(),
        &skills::store_dir(&home),
        &skills::load_store(&home),
    )
}

/// 两个根都由入参给出：生产代码里 `skills_active_root` 与 `store_dir` 都绕开
/// `home` 直接读全局路径，所以判据要能在临时目录上跑（测试不许碰用户真实
/// 数据），根就必须显式传进来。
fn list_for(active: &Path, store_root: &Path, store: &SkillStore) -> Vec<ConflictEntry> {
    targets(active, store_root, store)
        .into_iter()
        .map(|(skill, target, source)| ConflictEntry {
            skill,
            path: target.display().to_string(),
            detail: detail_for(&target, &source),
        })
        .collect()
}

/// 判据的骨架：`(技能名, 占位的落点, 中央库源)`。`list` 与 `move_aside` 走
/// **同一条**——两份实现会让「按钮出现在哪里」与「动手动哪些文件」分叉，而
/// 那种分叉的后果是按钮点下去动了判据没点名的文件。
fn targets(
    active: &Path,
    store_root: &Path,
    store: &SkillStore,
) -> Vec<(String, PathBuf, PathBuf)> {
    let mut out = Vec::new();
    for item in &store.items {
        let pkg_dir = store_root.join(&item.id);
        for entry in &item.skills {
            if let Some((target, source)) = conflict_at(active, &pkg_dir, entry) {
                out.push((entry.name.clone(), target, source));
            }
        }
    }
    out
}

/// `Some((落点, 中央库源))` 当且仅当**现在启用这条技能会失败**。
///
/// 三步与 `ensure_entry` 逐条对齐：条目占着位置 → 不属本商店所有 → 也不是
/// 那份「内容与源逐字节一致、可安全收编」的迁移残留。
fn conflict_at(active: &Path, pkg_dir: &Path, entry: &SkillEntry) -> Option<(PathBuf, PathBuf)> {
    let target = skills::target_path_in(active, entry);
    let md = fs::symlink_metadata(&target).ok()?;
    let source = skills::resolved_source(&pkg_dir.join(&entry.path));
    if !source.exists()
        || skills::entry_is_owned(&target, &source, entry)
        || skills::identical_unowned_copy(&target, &md, &source)
    {
        return None;
    }
    Some((target, source))
}

/// 一句话说清这条冲突的来由。版本证据只用来解释，不参与判定。
fn detail_for(target: &Path, source: &Path) -> String {
    let at = frontmatter_version(target);
    let store = frontmatter_version(source);
    match (at.as_deref(), store.as_deref()) {
        (Some(a), Some(b)) if a != b => {
            format!("活动视图里这份是 v{a}，技能库里是 v{b}，是升级后留下的旧副本")
        }
        _ => "与技能库里的同名技能不是同一份，可能来自其他技能包或手动放置".to_string(),
    }
}

/// 读一份技能条目 frontmatter 里的 `metadata.version`（目录包读 `SKILL.md`）。
///
/// 解析在 `skill_frontmatter`：frontmatter 有两个消费者，各写一份解析器就会
/// 分叉，而分叉出来的那份会让「面板上显示的版本」与「文案里说的版本」对不上。
/// 读不出来返回 `None`，调用方据此退回通用说法——**判据不依赖它**。
fn frontmatter_version(path: &Path) -> Option<String> {
    let file = if path.is_dir() {
        path.join("SKILL.md")
    } else {
        path.to_path_buf()
    };
    crate::skill_frontmatter::version(&fs::read_to_string(file).ok()?)
}

/// 面板顶部告警的一整段。没有冲突时是 `None`。
pub fn warning(entries: &[ConflictEntry]) -> Option<String> {
    if entries.is_empty() {
        return None;
    }
    let listed = entries
        .iter()
        .map(|e| format!("{}（{}，{}）", e.skill, e.path, e.detail))
        .collect::<Vec<_>>()
        .join("、");
    Some(format!(
        "这些技能在活动视图里的位置被同名条目占住，而占位的那份不归技能库所有，\
         所以启用一定失败：{listed}。\
         点下面的「移走冲突条目」即可腾出位置（只改名，不删除，文件名加时间戳后缀）；\
         移走之后回到开关上点启用，改名后的那份内容仍可随时找回。"
    ))
}

/// 启用时那条报错的文案。判据给的是「是什么」，这里给的是「接下来做什么」——
/// 用户拿到的第一手信息不该停在「请先处理该条目」这种没有落点的祈使句上。
pub(crate) fn conflict_error(target: &Path, source: &Path) -> AppError {
    AppError::Skill(format!(
        "技能名冲突：活动视图里已有一份同名的 {}（{}），它不来自当前技能包，所以启用被拒绝。\
         可执行的出路：打开「技能」面板，点告警下方的「移走冲突条目」——它只改名不删除，\
         腾出位置后回到这个开关上点启用即可；也可以自己把那份条目改名或移走。",
        target.display(),
        detail_for(target, source)
    ))
}

/// 把占住位置的同名条目改名让路。判据为空时是空操作（不是错误）。
pub fn move_aside(on_progress: &mut dyn FnMut(&str)) -> Result<(), AppError> {
    let home = skills::resolve_home();
    move_aside_for(
        &crate::paths::skills_active_root(),
        &skills::store_dir(&home),
        &skills::load_store(&home),
        on_progress,
    )
}

fn move_aside_for(
    active: &Path,
    store_root: &Path,
    store: &SkillStore,
    on_progress: &mut dyn FnMut(&str),
) -> Result<(), AppError> {
    // 判据现算：面板渲染到用户点按钮之间，条目可能已经被别的动作启用、
    // 删除或换掉了。认不出来就不动，与 skill_shadow 同一纪律。
    let found = targets(active, store_root, store);
    if found.is_empty() {
        on_progress("活动视图里没有占位的同名条目，无需处理");
        return Ok(());
    }
    let mut moved = 0usize;
    for (skill, target, _source) in found {
        if !target.starts_with(active) {
            on_progress(&format!(
                "跳过 {skill}：{} 已不在活动视图内，判据可能已过期",
                target.display()
            ));
            continue;
        }
        let kept = skills::keep_aside(
            &target,
            "它占住了活动视图里的同名技能位置，且不是技能库放置的（可能被本地修改过）",
        )?;
        moved += 1;
        on_progress(&format!(
            "已把占住 {skill} 位置的 {} 改名为 {kept}（改名让路，不是删除；改回原名即可恢复）",
            target.display()
        ));
        crate::shell_events::record(
            CONFLICT_LOG,
            &format!("moved aside {skill}: {} -> {kept}", target.display()),
        );
    }
    on_progress(&format!(
        "共移走 {moved} 份，现在可以回到技能开关上点启用了"
    ));
    Ok(())
}

/// 面板「移走冲突条目」的命令壳。判据与动作都在本文件里，命令只递参数。
#[tauri::command]
pub async fn skill_move_aside_conflicts(
    app: AppHandle,
    on_event: Channel<String>,
) -> Result<(), String> {
    crate::commands::run_skill_command(app, on_event, move |progress| move_aside(progress)).await
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;

    static SEQ: AtomicU32 = AtomicU32::new(0);

    /// 临时目录 + Drop 时自清。**只清自己这一块**，绝不 `remove_dir_all` 一个
    /// 解析出来的家目录（开发规则里那条「测试永远不许碰用户的真实数据」）。
    struct TempTree(PathBuf);

    impl TempTree {
        fn new(tag: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "dsh-skill-conflict-{tag}-{}-{}",
                std::process::id(),
                SEQ.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            TempTree(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempTree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// 一份带 frontmatter 的技能文件，`version` 决定两份内容是否相同。
    fn skill_md(name: &str, version: &str, body: &str) -> String {
        format!("---\nname: {name}\ndescription: d\nmetadata:\n  version: \"{version}\"\n---\n\n{body}\n")
    }

    /// 造出中央库 + 活动视图。返回 (临时树, 中央库里的 SKILL.md 路径)。
    fn fixture(tag: &str, stem: &str, store_version: &str) -> (TempTree, PathBuf) {
        let tree = TempTree::new(tag);
        let store = tree.path().join("store").join("pkg");
        let active = tree.path().join("active");
        std::fs::create_dir_all(&store).unwrap();
        std::fs::create_dir_all(&active).unwrap();
        let source = store.join("SKILL.md");
        std::fs::write(&source, skill_md(stem, store_version, "new")).unwrap();
        (tree, source)
    }

    fn store_with(id: &str, entry: SkillEntry) -> SkillStore {
        SkillStore {
            schema_version: 1,
            items: vec![skills::SkillStoreItem {
                id: id.to_string(),
                name: id.to_string(),
                origin: String::from("git"),
                source: String::from("https://example.test/x.git"),
                repo_url: None,
                installed_version: String::from("v1.0.0"),
                latest_version: None,
                mode: String::from("link"),
                actual_mode: String::from("link"),
                pinned: false,
                installed_at: String::from("0"),
                updated_at: String::from("0"),
                description: None,
                skills: vec![entry],
            }],
            last_checked_at: None,
            warning: None,
        }
    }

    fn entry_named(stem: &str) -> SkillEntry {
        SkillEntry {
            name: stem.to_string(),
            description: String::from("d"),
            path: String::from("SKILL.md"),
            enabled: false,
            materialized_sha256: None,
        }
    }

    /// 判据的纯函数入口：在临时中央库与临时活动根上算一遍。
    fn conflicts(tree: &TempTree, stem: &str) -> Vec<ConflictEntry> {
        let store = store_with("pkg", entry_named(stem));
        list_for(
            &tree.path().join("active"),
            &tree.path().join("store"),
            &store,
        )
        .into_iter()
        .map(|mut e| {
            e.path = e.path.replace(&tree.path().display().to_string(), "<tmp>");
            e
        })
        .collect()
    }

    /// 本机这次的真实局面：活动视图里是 v3.0.0 的普通文件，中央库已到 v3.1.0，
    /// 清单里没有它的指纹 → 判据必须命中，并说清是哪两个版本。
    #[test]
    fn stale_copy_of_an_older_version_is_reported_with_both_versions() {
        let (tree, _source) = fixture("stale", "xlink-stale", "3.1.0");
        let active = tree.path().join("active");
        std::fs::write(
            active.join("xlink-stale.md"),
            skill_md("xlink-stale", "3.0.0", "old"),
        )
        .unwrap();
        let found = conflicts(&tree, "xlink-stale");
        assert_eq!(found.len(), 1, "占位的旧副本必须被命中：{found:?}");
        assert_eq!(found[0].skill, "xlink-stale");
        assert_eq!(found[0].path, "<tmp>/active/xlink-stale.md");
        assert!(
            found[0].detail.contains("v3.0.0") && found[0].detail.contains("v3.1.0"),
            "文案要带上两边的版本：{}",
            found[0].detail
        );
    }

    /// 内容与源逐字节一致的迁移残留由 `ensure_entry` 收编，**不是**冲突：
    /// 按钮出现在能正常启用的技能上，比没有按钮更糟。
    #[test]
    fn identical_migration_copy_is_not_a_conflict() {
        let (tree, source) = fixture("same", "xlink-same", "3.1.0");
        let active = tree.path().join("active");
        std::fs::write(
            active.join("xlink-same.md"),
            std::fs::read(&source).unwrap(),
        )
        .unwrap();
        assert!(conflicts(&tree, "xlink-same").is_empty());
    }

    /// 壳自己放置的条目（清单里有它的指纹）不是冲突——那是正常状态。
    #[test]
    fn store_owned_entry_is_not_a_conflict() {
        let (tree, source) = fixture("owned", "xlink-owned", "3.1.0");
        let active = tree.path().join("active");
        let text = std::fs::read_to_string(&source).unwrap();
        std::fs::write(active.join("xlink-owned.md"), &text).unwrap();
        let mut store = store_with("pkg", entry_named("xlink-owned"));
        store.items[0].skills[0].materialized_sha256 =
            skills::fingerprint_path(&active.join("xlink-owned.md"));
        assert!(list_for(&active, &tree.path().join("store"), &store).is_empty());
    }

    /// 活动视图里没有同名条目时什么也不报（空判据 → 面板不给按钮）。
    #[test]
    fn clean_active_root_reports_nothing() {
        let (tree, _source) = fixture("clean", "xlink-clean", "3.1.0");
        assert!(conflicts(&tree, "xlink-clean").is_empty());
        let store = store_with("pkg", entry_named("xlink-clean"));
        let mut log = Vec::new();
        let result = {
            let mut push = |line: &str| log.push(line.to_string());
            move_aside_for(
                &tree.path().join("active"),
                &tree.path().join("store"),
                &store,
                &mut push,
            )
        };
        result.unwrap();
        assert!(log.iter().any(|l| l.contains("无需处理")), "{log:?}");
    }

    /// 改名让路：内容必须原样留在旁边，位置腾空，且之后不再被判为冲突。
    #[test]
    fn move_aside_preserves_content_and_clears_the_conflict() {
        let (tree, _source) = fixture("move", "xlink-move", "3.1.0");
        let active = tree.path().join("active");
        let target = active.join("xlink-move.md");
        let old = skill_md("xlink-move", "3.0.0", "old");
        std::fs::write(&target, &old).unwrap();
        let store = store_with("pkg", entry_named("xlink-move"));
        let mut log = Vec::new();
        let result = {
            let mut push = |line: &str| log.push(line.to_string());
            move_aside_for(&active, &tree.path().join("store"), &store, &mut push)
        };
        result.unwrap();
        assert!(!target.exists(), "位置应已腾空");
        let kept: Vec<_> = std::fs::read_dir(&active)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(kept.len(), 1, "只该留下改名后的那一份：{kept:?}");
        assert!(!kept[0].ends_with(".md"), "落点仍以 .md 结尾：{}", kept[0]);
        assert_eq!(std::fs::read_to_string(active.join(&kept[0])).unwrap(), old);
        assert!(
            list_for(&active, &tree.path().join("store"), &store).is_empty(),
            "移走之后冲突必须消失"
        );
        assert!(
            log.iter().any(|l| l.contains("改名让路，不是删除")),
            "进度文案要说清这是改名不是删除：{log:?}"
        );
    }

    /// 判据到动手之间条目被启用掉了（位置已被壳接管）时不得乱动别的文件。
    #[test]
    fn move_aside_is_a_noop_when_the_conflict_already_resolved() {
        let (tree, _source) = fixture("stale", "xlink-gone", "3.1.0");
        let active = tree.path().join("active");
        std::fs::create_dir_all(&active).unwrap();
        let store = store_with("pkg", entry_named("xlink-gone"));
        let mut log = Vec::new();
        let result = {
            let mut push = |line: &str| log.push(line.to_string());
            move_aside_for(&active, &tree.path().join("store"), &store, &mut push)
        };
        result.unwrap();
        assert!(log.iter().any(|l| l.contains("无需处理")), "{log:?}");
        assert_eq!(std::fs::read_dir(&active).unwrap().count(), 0);
    }

    /// 判据与按钮的唯一依据是「启用会不会失败」，因此启用的拒绝文案必须与
    /// 判据说同一件事，且指向同一个出路。改一边不改另一边，文案就会指向一个
    /// 面板上并不存在的按钮。
    #[test]
    fn the_enable_error_names_the_panel_button() {
        let (tree, source) = fixture("copy", "xlink-copy", "3.1.0");
        let target = tree.path().join("active").join("xlink-copy.md");
        std::fs::write(&target, skill_md("xlink-copy", "3.0.0", "old")).unwrap();
        let text = conflict_error(&target, &source).to_string();
        assert!(
            text.contains("移走冲突条目"),
            "报错要指向面板上的按钮：{text}"
        );
        assert!(text.contains(&target.display().to_string()), "{text}");
        assert!(text.contains("只改名不删除"), "{text}");
    }

    /// `description: |` 这类块标量不该被读成版本号：判据不受影响，只影响文案。
    #[test]
    fn frontmatter_version_ignores_block_scalars_and_non_metadata_keys() {
        let (tree, _source) = fixture("fm", "xlink-fm", "3.1.0");
        let path = tree.path().join("weird.md");
        std::fs::write(
            &path,
            "---\nname: n\ndescription: |\n  line one\n  version: 9.9.9\nversion: 8.8.8\n---\n\nbody\n",
        )
        .unwrap();
        assert_eq!(
            frontmatter_version(&path),
            None,
            "顶层 version 不是 metadata 里的"
        );
        std::fs::write(&path, skill_md("n", "3.1.0", "b")).unwrap();
        assert_eq!(frontmatter_version(&path).as_deref(), Some("3.1.0"));
        // 目录包读 SKILL.md；`v` 前缀与引号都要能归一。
        let dir = tree.path().join("bundle");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("SKILL.md"),
            "---\nname: n\nmetadata:\n  version: 'v4.2.0'\n---\n",
        )
        .unwrap();
        assert_eq!(frontmatter_version(&dir).as_deref(), Some("4.2.0"));
    }
}
