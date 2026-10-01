//! 插件中央库的一次性目录搬迁：已发布布局 `<xlink_home>/dsh-plugins/` →
//! 当前布局 `<xlink_home>/plugins/dsh[-dev]/`（见 [`crate::shell::paths::plugins_store_root`]）。
//!
//! 为什么要独立成文件，而不是塞进 `plugins.rs`：`plugins.rs` 是代码预算门禁里
//! 的「大文件，只许下调」，而本模块关心的**只有一件事**——旧目录还在不在、
//! 要不要搬、搬失败怎么办。它不读 `store.json`、不碰物化指纹、不参与安装 /
//! 卸载 / 同步；那些是中央库的日常读写，留在 `plugins.rs`。搬迁是一次性的目录级
//! 动作，与条目逻辑混在一起只会互相拖长，也让「中央库条目」与「旧布局兼容」
//! 各自可读。
//!
//! 三条自律（失败处理与 `plugins.rs` 同口径）：
//! - **只做一次**：目标目录已存在就直接返回，绝不覆盖任何已有内容；
//! - **失败不阻塞启动**：只留 stderr——中央库为空不该让整个壳起不来；
//! - **release 搬、dev 复制**：搬完之后原目录不复存在，而两个壳可能在同一台
//!   机器上先后启动——dev 需要的是自己那份副本，不该去动 release 的。

use std::path::Path;

use crate::shell::paths::ShellMode;

/// 按壳模式把中央库准备到位。`root` 是本壳的中央库根，由
/// [`crate::plugins::center::store_dir`] 在每次解析中央库时调用——目标已存在时立刻
/// 返回，日常调用零成本。
pub fn ensure_ready(mode: ShellMode, root: &Path) {
    match mode {
        ShellMode::Release => move_legacy_store_once(root),
        ShellMode::Dev => seed_dev_store_once(root),
    }
}

/// 已发布版本把中央库放在 `<xlink_home>/dsh-plugins/`，两个壳共用。新布局改成
/// `plugins/dsh`（release）/ `plugins/dsh-dev`（dev）之后，**release 是持有者**：
/// 存量目录整体搬过去，用户的插件列表不会凭空清空。
///
/// 为什么 release 用「搬」而 dev 用「复制」：搬完之后原目录不再存在，而两个壳
/// 可能在同一台机器上先后启动——dev 需要的是自己那份副本，不该去动 release 的。
fn move_legacy_store_once(root: &Path) {
    let legacy = crate::shell::paths::legacy_shared_plugins_store_root();
    if legacy == *root || root.exists() || !legacy.is_dir() {
        return;
    }
    if let Some(parent) = root.parent() {
        if let Err(error) = std::fs::create_dir_all(parent) {
            eprintln!(
                "plugins: 建中央库父目录失败（{}）：{error}",
                parent.display()
            );
            return;
        }
    }
    match std::fs::rename(&legacy, root) {
        Ok(()) => eprintln!(
            "plugins: 中央库已从 {} 迁到 {}",
            legacy.display(),
            root.display()
        ),
        // 跨卷时 rename 不可用，退回复制；失败只留 stderr，不阻塞启动——
        // 迁移是体验优化，中央库为空不该让整个壳起不来。
        Err(rename_error) => copy_legacy_store(&legacy, root, &rename_error),
    }
}

/// 跨卷回退：rename 已失败，把整棵中央库**复制**过来，成功后删源。
///
/// 抽出来（而不是内联在 [`move_legacy_store_once`] 里）只为能直接测「中途
/// 失败要清掉半截目标」——rename 失败本身只在跨卷时发生，测试造不出来，
/// 而复制中途失败可以（mode-000 的子目录）。
///
/// 失败时**清掉半截目标**再报错：`copy_dir_recursive` 先 `create_dir_all(to)`
/// 才读源，失败时会留下半个 `plugins/dsh/`；而 `move_legacy_store_once` 的
/// 入场条件是 `root.exists()`，半截目录一旦留下就永久短路重试——全量数据
/// 还在 `dsh-plugins/` 里，用户看到的却是一份残缺的插件列表。目标始终是
/// 源的一份副本，删了不丢东西，下次启动自然重试（与 `seed_dev_store_once`
/// 的清理同一纪律）。
fn copy_legacy_store(legacy: &Path, root: &Path, rename_error: &std::io::Error) {
    match crate::plugins::center::copy_dir_recursive(legacy, root) {
        Ok(()) => {
            eprintln!(
                "plugins: 中央库跨卷已复制到 {}（rename 失败：{rename_error}）",
                root.display()
            );
            let _ = std::fs::remove_dir_all(legacy);
        }
        Err(copy_error) => {
            let leftover = std::fs::remove_dir_all(root);
            eprintln!(
                "plugins: 中央库从 {} 迁移到 {} 失败（{rename_error}；复制也失败：{copy_error}）；\
                 插件列表可能为空，不影响 release 内核与工作台{}",
                legacy.display(),
                root.display(),
                match leftover {
                    Ok(()) => "（已清掉未完成的半截目录，下次启动会重试）".to_string(),
                    Err(clean) => format!("（清理未完成目录也失败：{clean}，需手动删除）"),
                }
            );
        }
    }
}

/// dev 壳首次解析中央库时，从别处复制一份种子。
///
/// 两个壳曾经共用一份中央库，于是 dev 壳里已装的插件都在那个共享目录里。切成
/// 独立的 `plugins/dsh-dev/` 之后它会是空的，dev 壳的插件列表会**凭空清空**——
/// 数据没丢，但用户看到的是一个空列表。这里一次性复制过去，避免那种体验断裂。
///
/// 种子来源优先 release 那份（`plugins/dsh`）——它才是新布局下的持有者；只有
/// release 还没迁移（dev 先启动）时才回退到历史的 `dsh-plugins/`。
///
/// 三条自律：
/// - **release 永远不触发**（由 [`ensure_ready`] 的分发保证）；
/// - **只做一次**（目标目录已存在就直接返回），不覆盖 dev 自己的后续改动；
/// - **失败不阻塞启动**，只留 stderr——种子是体验优化，不是功能前提。
fn seed_dev_store_once(root: &Path) {
    if root.exists() {
        return;
    }
    let release_root = crate::shell::paths::plugins_store_root_for(ShellMode::Release);
    let legacy = crate::shell::paths::legacy_shared_plugins_store_root();
    let source = if release_root.is_dir() && release_root != *root {
        release_root
    } else if legacy.is_dir() && legacy != *root {
        legacy
    } else {
        return;
    };
    if let Some(parent) = root.parent() {
        if let Err(error) = std::fs::create_dir_all(parent) {
            eprintln!(
                "plugins: 建中央库父目录失败（{}）：{error}",
                parent.display()
            );
            return;
        }
    }
    if let Err(error) = crate::plugins::center::copy_dir_recursive(&source, root) {
        // **清掉半截目标**再报错：`copy_dir_recursive` 先 `create_dir_all(to)`
        // 才读源，失败时会留下一个空目录；而 `seed_dev_store_once` 的入场条件是
        // `root.exists()`，空目录一旦留下就再也补不上种子，dev 的插件列表会
        // 永久停在空状态。两个壳同时启动、release 抢先 rename 走时就会走到这里。
        let leftover = std::fs::remove_dir_all(root);
        eprintln!(
            "plugins: dev 中央库种子复制失败（{} -> {}）：{error}；dev 壳的插件列表将从空开始，不影响 release{}",
            source.display(),
            root.display(),
            match leftover {
                Ok(()) => "（已清掉未完成的空目录，下次启动会重试）".to_string(),
                Err(clean) => format!("（清理未完成目录也失败：{clean}，需手动删除）"),
            }
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::EnvGuard;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static SEQ: AtomicUsize = AtomicUsize::new(0);

    /// 每个测试独占的临时 Xlink home：建目录 + 把 `DSH_XLINK_HOME` 指过去
    /// （顺带持住进程级 env 锁），drop 时清目录并还原 env。
    ///
    /// **必须**走 [`crate::tests::scoped_xlink_home`] 而不是裸 `set_var`：本
    /// 模块的函数全按 `paths::*` 解析路径，env 一旦漏出去，写的就是用户真实
    /// 的 `~/.dsh-xlink`（2026-09-29 的初版测试就是这么写的，结尾还直接
    /// `remove_dir_all(xlink_home())`——一次 `cargo test` 能把用户全部内核、
    /// 实例与会话删干净）。`paths.rs` 里 `scoped_xlink_home_unset` 的注释记
    /// 的正是同一个事故根因。
    struct TempXlink {
        root: std::path::PathBuf,
        _guard: EnvGuard,
    }

    impl TempXlink {
        fn new() -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let seq = SEQ.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "dsh-store-relocate-{}-{nanos}-{seq}",
                std::process::id()
            ));
            std::fs::create_dir_all(&root).expect("create temp xlink home");
            let guard = crate::tests::scoped_xlink_home(&root);
            TempXlink {
                root,
                _guard: guard,
            }
        }
    }

    impl Drop for TempXlink {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    /// 存量用户的数据不能凭空消失。`dsh-plugins/` 是已发布版本在用的目录，
    /// release 必须把它整体搬进 `plugins/dsh/`；dev 则复制一份种子到
    /// `plugins/dsh-dev/`。四个最容易错的地方各钉一条：搬错了、覆盖了、
    /// 搬完把源删了、没源时凭空造目录。
    #[test]
    fn the_legacy_store_is_carried_into_the_new_layout() {
        let _home = TempXlink::new();
        let legacy = crate::shell::paths::legacy_shared_plugins_store_root();
        let release = crate::shell::paths::plugins_store_root_for(ShellMode::Release);
        let dev = crate::shell::paths::plugins_store_root_for(ShellMode::Dev);

        std::fs::create_dir_all(legacy.join("some-plugin")).unwrap();
        std::fs::write(legacy.join("some-plugin").join("package.json"), "{}").unwrap();
        std::fs::write(legacy.join("store.json"), "{\"items\":[]}").unwrap();

        // release：整体搬过去，内容一字不少。
        ensure_ready(ShellMode::Release, &release);
        assert_eq!(
            std::fs::read_to_string(release.join("some-plugin").join("package.json")).unwrap(),
            "{}",
            "存量插件源码必须原样搬过去"
        );
        assert!(
            release.join("store.json").is_file(),
            "中央库根下的文档也要搬"
        );
        assert!(!legacy.exists(), "搬完原目录不该还在（否则下次又会搬一遍）");

        // dev：复制一份种子，release 那份不能被它动。
        ensure_ready(ShellMode::Dev, &dev);
        assert!(
            dev.join("some-plugin").join("package.json").is_file(),
            "dev 必须拿到自己的副本"
        );
        assert!(release.exists(), "dev 取种子不得动 release 的目录");

        // 幂等：再来一次什么都不做。
        ensure_ready(ShellMode::Release, &release);
        ensure_ready(ShellMode::Dev, &dev);
        assert!(release.join("some-plugin").is_dir() && dev.join("some-plugin").is_dir());

        // 目标已存在时不得覆盖——dev 自己后来的改动不能被种子冲掉。
        std::fs::write(dev.join("some-plugin").join("marker.txt"), "mine").unwrap();
        ensure_ready(ShellMode::Dev, &dev);
        assert_eq!(
            std::fs::read_to_string(dev.join("some-plugin").join("marker.txt")).unwrap(),
            "mine",
            "目标已存在时不得覆盖"
        );
    }

    /// 没有旧目录时**什么也不做**——`store_dir` 在解析中央库时被每次调用，
    /// 凭空造一个空中央库会让「装过插件」的判据失真（store.json 缺失 ≠ 没装过）。
    #[test]
    fn nothing_happens_when_there_is_no_legacy_store() {
        let home = TempXlink::new();
        let release = crate::shell::paths::plugins_store_root_for(ShellMode::Release);
        let dev = crate::shell::paths::plugins_store_root_for(ShellMode::Dev);
        ensure_ready(ShellMode::Release, &release);
        ensure_ready(ShellMode::Dev, &dev);
        assert!(!release.exists(), "无旧目录时不得创建 release 中央库");
        assert!(!dev.exists(), "无旧目录时不得创建 dev 中央库");
        assert!(
            !home.root.join("plugins").exists(),
            "连命名空间父目录都不该被建出来"
        );
    }

    /// 跨卷复制**中途**失败时必须清掉半截目标：`root.exists()` 是迁移的入场
    /// 条件，半截 `plugins/dsh/` 一旦留下就永久短路重试——全量数据还在
    /// `dsh-plugins/` 里，用户看到的却是一份残缺的插件列表。rename 失败只在
    /// 跨卷时发生、测试造不出来，因此直接测回退函数；复制中途失败用一个
    /// mode-000 的子目录制造（读到它时 EACCES，此时 `first.txt` 已拷过去）。
    /// 与 `seed_dev_store_once` 的清理同一纪律，那边修的是 dev 侧的对称形态。
    #[test]
    #[cfg(unix)]
    fn a_partial_cross_volume_copy_is_cleaned_up_so_retry_stays_possible() {
        use std::os::unix::fs::PermissionsExt;

        // root 不受 mode-000 约束（读目录照样成功），夹具会失效——那种环境
        // 下跳过而不是让断言以「半截目标没被清掉」的假象失败。
        if unsafe { libc::geteuid() } == 0 {
            eprintln!("store_relocate: 以 root 运行，mode-000 夹具无效，跳过本用例");
            return;
        }
        let _home = TempXlink::new();
        let legacy = crate::shell::paths::legacy_shared_plugins_store_root();
        let release = crate::shell::paths::plugins_store_root_for(ShellMode::Release);

        std::fs::create_dir_all(legacy.join("some-plugin").join("sub")).unwrap();
        std::fs::write(legacy.join("store.json"), "{}").unwrap();
        std::fs::write(legacy.join("some-plugin").join("first.txt"), "1").unwrap();
        std::fs::write(
            legacy.join("some-plugin").join("sub").join("second.txt"),
            "2",
        )
        .unwrap();
        std::fs::set_permissions(
            legacy.join("some-plugin").join("sub"),
            std::fs::Permissions::from_mode(0o000),
        )
        .unwrap();

        copy_legacy_store(&legacy, &release, &std::io::Error::other("cross-volume"));

        assert!(
            !release.exists(),
            "半截目标必须被清掉，否则下次启动 root.exists() 直接短路，迁移永不重试"
        );
        assert!(
            legacy.join("store.json").is_file()
                && legacy.join("some-plugin").join("first.txt").is_file(),
            "源必须原封不动——它是全量数据的所在地"
        );
        assert!(
            legacy.join("some-plugin").is_dir(),
            "读不进去的子目录也仍在源里（断言它本身只需父目录的 r+x）"
        );

        // 收尾前恢复权限，TempXlink 的 Drop 才能删掉这块临时目录。
        std::fs::set_permissions(
            legacy.join("some-plugin").join("sub"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
}
