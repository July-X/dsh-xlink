//! 旧布局 → 新布局的迁移向导骨架（开发计划 §P6 / 设计稿 §11）。
//!
//! 设计原则：先**只读扫描**，再让用户拍板；任何破坏性动作都必须由调用方
//! 显式发起，回滚必须保留旧源。
//!
//! 当前实现范围：
//! - [`LegacySource`] / [`LegacySourceInfo`]：识别旧布局来源（旧 DSH home
//!   下的 `plugins/`、`skills-store/`、`skills/`）。
//! - [`scan_legacy_sources`]：只读扫描，返回每个来源是否存在、文件数
//!   与字节数。**绝不**触碰旧目录——任何写入都要等用户拍板。
//! - [`MigrationPreview`] / [`preview_migration`]：对每条可迁移来源，列出
//!   源路径、目标路径、条目数和大小。复制 / 备份 / 回滚留到后续步骤
//!   （UI 形态 / 备份策略 / credentials 处理 等决策点尚未对齐）。
//!
//! 不在范围：
//! - 旧 `~/.dsh/desktop[-dev]/` 仍在旧 DSH home 下供 kernel 模块使用
//!   （含 `active.txt` / `kernel.pid` / 内核安装目录），由 kernel /
//!   `kernel_adapter` 自管理，**不**属于本向导迁移范围。
//!
//! 测试约束（开发计划 §P6 测试要求）：
//! - 预览阶段不写旧目录。
//! - `scan_legacy_sources` / `preview_migration` 在不存在的目录上返回空，
//!   不 panic、不创建目录。

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::paths::{
    legacy_dsh_home, legacy_dsh_plugins_root, legacy_dsh_skills_root, legacy_dsh_skills_store,
    plugins_store_root, skills_active_root, skills_store_root,
};

/// 旧布局来源——按目录定位，不依赖文件内容。
///
/// 设计稿 §11.1 描述的迁移映射（仅本向导范围；`desktop[-dev]/` 由 kernel
/// 模块自管，见本模块顶部说明）：
/// - `plugins/` → 新 Xlink home 的 `dsh-plugins/`
/// - `skills-store/` → 新 Xlink home 的 `skills/packages/`
/// - `skills/` → 新 Xlink home 的 `skills/active/`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LegacySource {
    /// `~/.dsh/plugins/`（旧版插件中央库）。
    Plugins,
    /// `~/.dsh/skills-store/`（旧版技能中央库）。
    SkillsStore,
    /// `~/.dsh/skills/`（旧版技能活动视图，官方内核读取路径之一）。
    SkillsActive,
}

impl LegacySource {
    /// 旧 DSH home 下的具体路径。**不**检查存在性。
    pub fn path(self) -> PathBuf {
        match self {
            LegacySource::Plugins => legacy_dsh_plugins_root(),
            LegacySource::SkillsStore => legacy_dsh_skills_store(),
            LegacySource::SkillsActive => legacy_dsh_skills_root(),
        }
    }

    /// 新布局下的目标路径。**不**检查存在性、不创建目录。
    pub fn target(self) -> PathBuf {
        match self {
            LegacySource::Plugins => plugins_store_root(),
            LegacySource::SkillsStore => skills_store_root(),
            LegacySource::SkillsActive => skills_active_root(),
        }
    }

    /// 全部来源枚举——`scan_legacy_sources` 与 `preview_migration`
    /// 按这个顺序扫，结果可重现。
    pub fn all() -> [LegacySource; 3] {
        [
            LegacySource::Plugins,
            LegacySource::SkillsStore,
            LegacySource::SkillsActive,
        ]
    }

    /// 用户可见的来源名称（简体中文）。
    pub fn display_name(self) -> &'static str {
        match self {
            LegacySource::Plugins => "旧插件中央库",
            LegacySource::SkillsStore => "旧技能中央库",
            LegacySource::SkillsActive => "旧技能活动视图",
        }
    }
}

/// 旧布局来源的扫描结果——只读、绝不创建目录。
#[derive(Debug, Clone, Serialize)]
pub struct LegacySourceInfo {
    pub source: LegacySource,
    pub path: PathBuf,
    pub target: PathBuf,
    pub exists: bool,
    pub file_count: usize,
    pub total_bytes: u64,
}

/// 扫描所有旧布局来源，返回它们是否存在、文件数和字节数。
///
/// **只读**：不创建 / 删除任何文件，也不写目标目录。`path` 不存在时
/// `exists = false`，`file_count = 0`，`total_bytes = 0`，便于调用方
/// 判定哪些来源需要处理。
///
/// 目录遍历在符号链接处**不**追链——避免把用户手放的目录误算进条目数。
pub fn scan_legacy_sources() -> Vec<LegacySourceInfo> {
    LegacySource::all()
        .into_iter()
        .map(|source| {
            let path = source.path();
            let target = source.target();
            let (exists, file_count, total_bytes) = scan_one(&path);
            LegacySourceInfo {
                source,
                path,
                target,
                exists,
                file_count,
                total_bytes,
            }
        })
        .collect()
}

fn scan_one(root: &Path) -> (bool, usize, u64) {
    if !root.exists() {
        return (false, 0, 0);
    }
    let metadata = match fs::symlink_metadata(root) {
        Ok(md) => md,
        Err(_) => return (true, 0, 0),
    };
    if !metadata.is_dir() {
        // 旧布局来源必须是目录；如果是文件 / 链接则跳过（不删）。
        return (true, 0, 0);
    }
    let mut count = 0usize;
    let mut bytes = 0u64;
    walk_count(root, &mut count, &mut bytes);
    (true, count, bytes)
}

fn walk_count(dir: &Path, count: &mut usize, bytes: &mut u64) {
    let entries = match fs::read_dir(dir) {
        Ok(it) => it,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let md = match fs::symlink_metadata(entry.path()) {
            Ok(md) => md,
            Err(_) => continue,
        };
        if md.is_dir() {
            walk_count(&entry.path(), count, bytes);
        } else {
            *count += 1;
            *bytes += md.len();
        }
    }
}

/// 单条迁移项的预览：源路径、目标路径、条目数、字节数。
#[derive(Debug, Clone, Serialize)]
pub struct MigrationItemPreview {
    pub source: LegacySource,
    pub display_name: String,
    pub source_path: PathBuf,
    pub target_path: PathBuf,
    pub file_count: usize,
    pub total_bytes: u64,
    pub target_exists: bool,
}

/// 完整迁移预览——包含 `DSH_HOME` 解析源、新 Xlink home 解析、每个
/// 旧来源的迁移项。
#[derive(Debug, Clone, Serialize)]
pub struct MigrationPreview {
    pub legacy_dsh_home: PathBuf,
    pub xlink_home: PathBuf,
    pub items: Vec<MigrationItemPreview>,
}

impl MigrationPreview {
    /// 至少有一条**实际可迁移**的来源（存在且非空）。
    pub fn has_migratable(&self) -> bool {
        self.items
            .iter()
            .any(|item| item.file_count > 0 || item.total_bytes > 0)
    }
}

/// 生成完整迁移预览。**只读**——不创建任何目录、不触碰源 / 目标。
pub fn preview_migration() -> MigrationPreview {
    let sources = scan_legacy_sources();
    let items = sources
        .into_iter()
        .map(|info| MigrationItemPreview {
            source: info.source,
            display_name: info.source.display_name().to_string(),
            source_path: info.path,
            target_path: info.target.clone(),
            file_count: info.file_count,
            total_bytes: info.total_bytes,
            target_exists: info.target.exists(),
        })
        .collect();
    MigrationPreview {
        legacy_dsh_home: legacy_dsh_home(),
        xlink_home: crate::paths::xlink_home(),
        items,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试 home：在临时目录下建一份唯一临时目录；drop 时自动清理。
    /// 设 `DSH_XLINK_HOME` 让 `paths::xlink_home()` 解析到本目录。
    /// 同时设 `DSH_HOME` 指向同一份临时目录的 `legacy/` 子目录，让
    /// `legacy_dsh_home()` 解析到那里——这样测试可以同时建一份"旧"
    /// 和一份"新"。
    struct TempHome {
        root: PathBuf,
        _xlink_guard: crate::tests::EnvGuard,
        _dsh_guard: crate::tests::EnvGuard,
    }

    impl TempHome {
        fn new() -> Self {
            let nano = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let seq = std::sync::atomic::AtomicUsize::fetch_add(
                &TEST_COUNTER,
                1,
                std::sync::atomic::Ordering::Relaxed,
            );
            let base = std::env::temp_dir().join(format!(
                "dsh-migration-test-{}-{nano}-{seq}",
                std::process::id()
            ));
            let legacy = base.join("legacy");
            fs::create_dir_all(&legacy).expect("create test home");
            let xlink_guard = crate::tests::scoped_xlink_home(&base);
            let dsh_guard = crate::tests::scoped_dsh_home(&legacy);
            TempHome {
                root: base,
                _xlink_guard: xlink_guard,
                _dsh_guard: dsh_guard,
            }
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    static TEST_COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    #[test]
    fn scan_legacy_sources_does_not_create_directories() {
        // 空 home 下扫所有旧来源——任何来源都不应被创建。
        let home = TempHome::new();
        let infos = scan_legacy_sources();
        assert_eq!(infos.len(), 3, "必须扫全 3 个旧来源");
        for info in &infos {
            assert!(!info.exists, "{} 不应存在", info.source.display_name());
            assert_eq!(info.file_count, 0);
            assert_eq!(info.total_bytes, 0);
            assert!(
                !info.target.exists(),
                "{} 目标也不应被创建",
                info.source.display_name()
            );
        }
    }

    #[test]
    fn scan_legacy_sources_counts_existing_files() {
        // 临时建一份"旧插件中央库"——里面放三个文件（含一个 nested），
        // 应该被算成 3 条 / 累计字节数；其余 2 个来源仍为不存在。
        let home = TempHome::new();
        let plugins_legacy = LegacySource::Plugins.path();
        fs::create_dir_all(&plugins_legacy).expect("legacy plugins");
        fs::write(plugins_legacy.join("store.json"), "{}\n").expect("store");
        fs::write(plugins_legacy.join("manifest.txt"), "x").expect("manifest");
        // nested dir also counted (recursive)
        fs::create_dir_all(plugins_legacy.join("pkg-a")).expect("pkg-a");
        fs::write(plugins_legacy.join("pkg-a/data.txt"), "abc").expect("pkg data");

        let infos = scan_legacy_sources();
        let plugins_info = infos
            .iter()
            .find(|i| i.source == LegacySource::Plugins)
            .expect("plugins info");
        assert!(plugins_info.exists);
        assert_eq!(plugins_info.file_count, 3);
        assert!(plugins_info.total_bytes > 0);
        for other in infos.iter().filter(|i| i.source != LegacySource::Plugins) {
            assert!(!other.exists, "{} 必须不存在", other.source.display_name());
        }
    }

    #[test]
    fn preview_migration_reports_target_paths_and_existing_flags() {
        // 一次性把 3 个旧来源都建上，preview 应该全部报告存在 + 字节数；
        // target_exists 取决于 xlink_home 下对应目录是否已被新布局占用。
        let home = TempHome::new();
        for source in LegacySource::all() {
            let path = source.path();
            fs::create_dir_all(&path).expect("legacy source");
            fs::write(path.join("seed.txt"), "x").expect("seed file");
        }
        let preview = preview_migration();
        assert!(preview.has_migratable(), "至少有一条可迁移来源");
        for item in &preview.items {
            assert!(item.file_count >= 1);
            assert!(item.total_bytes >= 1);
            // 3 个旧来源对应的目标路径**全部不应**存在——xlink_home 是
            // 全新临时目录，新布局不会冲突。
            assert!(
                !item.target_exists,
                "{} 的目标 {} 在全新 home 下不应存在",
                item.display_name,
                item.target_path.display()
            );
            assert_eq!(item.source_path, item.source.path());
            assert_eq!(item.target_path, item.source.target());
        }
    }

    #[test]
    fn legacy_source_target_matches_new_layout_paths() {
        // 直接锁定源 → 目标的映射——这是迁移向导的核心契约：preview 与
        // 复制步骤都按这个表来。任何改动都必须先改这里 + 改测试。
        use LegacySource::*;
        let cases = [
            (Plugins, plugins_store_root()),
            (SkillsStore, skills_store_root()),
            (SkillsActive, skills_active_root()),
        ];
        for (source, expected) in cases {
            assert_eq!(
                source.target(),
                expected,
                "{:?} 的目标路径必须与新布局一致",
                source
            );
        }
    }

    #[test]
    fn preview_reports_target_conflicts_when_new_layout_already_present() {
        // 当新布局已经写了部分目录（用户可能手动创建过、或先跑过迁移
        // 又回滚），preview 必须如实报告 target_exists 让 UI 弹冲突提示。
        let home = TempHome::new();
        // 先建一份旧 plugins 中央库。
        let plugins_legacy = LegacySource::Plugins.path();
        fs::create_dir_all(&plugins_legacy).expect("legacy");
        fs::write(plugins_legacy.join("store.json"), "x").expect("store");
        // 然后让新布局的 `dsh-plugins/` 已经存在（模拟"先跑过迁移"或"手放"）。
        let plugins_new = LegacySource::Plugins.target();
        fs::create_dir_all(&plugins_new).expect("new layout");
        fs::write(plugins_new.join("placeholder.txt"), "x").expect("placeholder");

        let preview = preview_migration();
        let plugins_item = preview
            .items
            .iter()
            .find(|i| i.source == LegacySource::Plugins)
            .expect("plugins item");
        assert!(
            plugins_item.target_exists,
            "新布局 dsh-plugins 已存在，preview 必须如实报告"
        );
    }
}
