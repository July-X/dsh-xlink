//! 技能「被更高优先级的根盖住」的处置：把盖住的那几份**改名让路**。
//!
//! 内核的 `dsh-skill-filesystem` 把 `<projectRoot>/.dsh/skills`（rank 100）与
//! `<projectRoot>/.agents/skills`（rank 200）排在壳的 custom 根（rank 300）
//! **前面**，而 `projectRoot` 是从会话工作目录向上第一个带 `.git` 的目录。
//! 家目录带 `.git` 时（2026-09-30 本机实测确实有 `~/.git`），这两处实际扮演
//! 「项目级」根，与那里同名的技能由内核读另一份，壳的启停与更新对它不生效。
//! 判据与 rank 算术在 [`paths::shadowing_skill_roots`]，本模块只负责执行。
//!
//! 三条纪律，任何一条都不能为了「让按钮好点」而放宽：
//!
//! - **只改名，不删除**。落点带时间戳且不再以 `.md` 结尾（内核的发现只认
//!   `*.md` 与目录包），所以移走之后既不会被当成技能、也不会再被判据命中；
//!   回退就是把文件名改回去。
//! - **只动判据点名的那几条**。列表来自 `paths::shadowing_skill_entries`，
//!   动手前再确认落点仍在这两个高优先级根内——判据算完到动手之间文件可能
//!   已经变了（换了链接、被别的进程挪走），认不出来就不动。
//! - **家目录没有 `.git` 时什么也不做**。那时 `~/.dsh/skills` 根本不会被扫，
//!   `~/.agents/skills` 只是 rank 500 的 user-agents，排在壳的**之后**；
//!   把它当「盖住」就会去动一份正在生效的文件。
//!
//! 这是壳唯一一处**故意**写用户技能目录的路径（`~/.dsh/skills`），
//! 出发点是：判据已经精确到具体文件，而此前面板只给一句话、让用户自己去
//! 资源管理器里删——可执行的下一步不该只存在于文档里。
//!
//! 不需要停工作台：内核的 chokidar watcher 盯着这两个根，改名会让它重新
//! 发现技能，壳管理的那一份当场接管。

use std::path::Path;

use serde::Serialize;

use crate::error::AppError;
use crate::paths;

/// 落盘用的逻辑名，用户在「查看日志」面板里按它检索。
const SHADOW_LOG: &str = "skill-shadow";

/// 活动视图里被更高优先级根盖住的一条条目。
#[derive(Debug, Clone, Serialize)]
pub struct ShadowedEntry {
    /// 活动视图里的技能名（被盖住的那一份也用它做 key）。
    pub skill: String,
    /// 盖住它的那份文件或目录（绝对路径，由后端给出，前端不拼路径）。
    pub path: String,
}

/// 面板用的只读视图。判据为空时返回空列表——UI 据此决定要不要给按钮。
pub fn list() -> Vec<ShadowedEntry> {
    list_for(&paths::skills_active_root(), &paths::dirs_home())
}

fn list_for(active: &Path, home: &Path) -> Vec<ShadowedEntry> {
    paths::shadowing_skill_entries(active, home)
        .into_iter()
        .map(|(skill, path)| ShadowedEntry {
            skill,
            path: path.display().to_string(),
        })
        .collect()
}

/// 把盖住活动视图的那些条目改名让路。判据为空时是空操作（不是错误）。
pub fn move_aside_shadowed(on_progress: &mut dyn FnMut(&str)) -> Result<(), AppError> {
    move_aside_for(
        &paths::skills_active_root(),
        &paths::dirs_home(),
        on_progress,
    )
}

fn move_aside_for(
    active: &Path,
    home: &Path,
    on_progress: &mut dyn FnMut(&str),
) -> Result<(), AppError> {
    let roots = paths::shadowing_skill_roots(home);
    let shadowed = paths::shadowing_skill_entries(active, home);
    if shadowed.is_empty() {
        on_progress("没有被同名条目盖住的技能，无需处理");
        return Ok(());
    }
    let mut moved = 0usize;
    for (skill, path) in shadowed {
        if !roots.iter().any(|root| path.starts_with(root)) {
            on_progress(&format!(
                "跳过 {skill}：{} 已不在高优先级根内，判据可能已过期",
                path.display()
            ));
            continue;
        }
        let new_path = crate::skills::keep_aside(
            &path,
            "它盖住了活动视图里的同名技能，壳管理的启停与更新对它不生效",
        )?;
        moved += 1;
        on_progress(&format!(
            "已把盖住 {skill} 的 {} 改名为 {new_path}（改名让路，不是删除；改回原名即可恢复）",
            path.display()
        ));
        crate::shell_events::record(
            SHADOW_LOG,
            &format!("moved aside {skill}: {} -> {new_path}", path.display()),
        );
    }
    on_progress(&format!("共移走 {moved} 份，壳管理的条目现在接管这些技能"));
    Ok(())
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
                "dsh-skill-shadow-{tag}-{}-{}",
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

    /// 造出「家目录是 git 仓库 + 活动视图与高优先级根同名」的局面。
    /// 返回 (临时树, 活动根, 盖住的那一份)。
    fn fixture(tag: &str, stem: &str) -> (TempTree, PathBuf, PathBuf) {
        let tree = TempTree::new(tag);
        let home = tree.path().join("home");
        let active = tree.path().join("active");
        std::fs::create_dir_all(home.join(".git")).unwrap();
        std::fs::create_dir_all(home.join(".dsh").join("skills")).unwrap();
        std::fs::create_dir_all(&active).unwrap();
        std::fs::write(active.join(format!("{stem}.md")), "managed").unwrap();
        let shadowing = home.join(".dsh").join("skills").join(format!("{stem}.md"));
        std::fs::write(&shadowing, "old").unwrap();
        (tree, active, shadowing)
    }

    fn run(active: &Path, home: &Path) -> (Result<(), AppError>, Vec<String>) {
        let mut log = Vec::new();
        let result = {
            let mut push = |line: &str| log.push(line.to_string());
            move_aside_for(active, home, &mut push)
        };
        (result, log)
    }

    /// 命中的那一份被改名让路，活动视图那份原地不动，且改名后不再被判据命中。
    #[test]
    fn move_aside_renames_the_shadowing_copy_and_keeps_the_managed_one() {
        let (tree, active, shadowing) = fixture("rename", "xlink-shadow");
        let home = tree.path().join("home");
        let (result, log) = run(&active, &home);
        result.unwrap();
        assert!(!shadowing.exists(), "盖住的那一份不该还在原处");
        assert!(
            active.join("xlink-shadow.md").exists(),
            "活动视图那份不能动"
        );
        assert_eq!(
            std::fs::read_to_string(active.join("xlink-shadow.md")).unwrap(),
            "managed"
        );
        assert!(
            list_for(&active, &home).is_empty(),
            "改名之后不该再被判为盖住（否则告警永远消不掉）"
        );
        // 落点必须不再以 .md 结尾，否则内核仍会把它当技能扫进来。
        let kept: Vec<_> = std::fs::read_dir(home.join(".dsh").join("skills"))
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(kept.len(), 1, "只该留下改名后的那一份：{kept:?}");
        assert!(!kept[0].ends_with(".md"), "落点仍以 .md 结尾：{}", kept[0]);
        assert!(
            log.iter().any(|l| l.contains("改名让路，不是删除")),
            "进度文案要说清这是改名不是删除：{log:?}"
        );
    }

    /// 家目录不是项目根时不得动任何东西：那两个目录要么不被扫，要么排在壳
    /// 之后，动它们就是动一份正在生效的文件。
    #[test]
    fn move_aside_is_a_noop_without_a_project_root() {
        let (tree, active, shadowing) = fixture("nogit", "xlink-nogit");
        let home = tree.path().join("home");
        std::fs::remove_dir_all(home.join(".git")).unwrap();
        let (result, log) = run(&active, &home);
        result.unwrap();
        assert!(shadowing.exists(), "没有 .git 时不得动 ~/.dsh/skills");
        assert!(log.iter().any(|l| l.contains("无需处理")), "{log:?}");
    }

    /// 目录包形态（`~/.dsh/skills/<name>/`）也要能移走，且活动视图那份不受影响。
    #[test]
    fn move_aside_handles_directory_bundles() {
        let tree = TempTree::new("bundle");
        let home = tree.path().join("home");
        let active = tree.path().join("active");
        std::fs::create_dir_all(home.join(".git")).unwrap();
        std::fs::create_dir_all(home.join(".dsh").join("skills").join("xlink-bundle")).unwrap();
        std::fs::create_dir_all(&active).unwrap();
        std::fs::write(active.join("xlink-bundle.md"), "managed").unwrap();
        let (result, _) = run(&active, &home);
        result.unwrap();
        assert!(!home
            .join(".dsh")
            .join("skills")
            .join("xlink-bundle")
            .exists());
        assert!(
            list_for(&active, &home).is_empty(),
            "{:?}",
            list_for(&active, &home)
        );
    }

    /// 只动判据点名的那几条：同一个高优先级根里、**没有**同名活动条目的文件
    /// （用户自己放的）必须原地不动——判据是「盖住活动视图」，不是「清空根目录」。
    #[test]
    fn move_aside_leaves_unrelated_entries_alone() {
        let tree = TempTree::new("scoped");
        let home = tree.path().join("home");
        let active = tree.path().join("active");
        let project = home.join(".dsh").join("skills");
        std::fs::create_dir_all(home.join(".git")).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&active).unwrap();
        std::fs::write(active.join("xlink-hit.md"), "managed").unwrap();
        std::fs::write(project.join("xlink-hit.md"), "old").unwrap();
        // 同根下的另一份：活动视图里没有同名条目，因此它没盖住任何东西。
        let orphan = project.join("xlink-orphan.md");
        std::fs::write(&orphan, "mine").unwrap();
        let (result, _) = run(&active, &home);
        result.unwrap();
        assert!(!project.join("xlink-hit.md").exists(), "命中的应被移走");
        assert!(orphan.exists(), "没盖住任何东西的那份必须原地不动");
        assert_eq!(std::fs::read_to_string(&orphan).unwrap(), "mine");
        assert!(list_for(&active, &home).is_empty());
    }
}
