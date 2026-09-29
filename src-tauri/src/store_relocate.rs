//! 插件中央库的一次性目录搬迁：已发布布局 `<xlink_home>/dsh-plugins/` →
//! 当前布局 `<xlink_home>/plugins/dsh[-dev]/`（见 [`crate::paths::plugins_store_root`]）。
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

use crate::paths::ShellMode;

/// 按壳模式把中央库准备到位。`root` 是本壳的中央库根，由
/// [`crate::plugins::store_dir`] 在每次解析中央库时调用——目标已存在时立刻
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
    let legacy = crate::paths::legacy_shared_plugins_store_root();
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
        Err(rename_error) => match crate::plugins::copy_dir_recursive(&legacy, root) {
            Ok(()) => {
                eprintln!(
                    "plugins: 中央库跨卷已复制到 {}（rename 失败：{rename_error}）",
                    root.display()
                );
                let _ = std::fs::remove_dir_all(&legacy);
            }
            Err(copy_error) => eprintln!(
                "plugins: 中央库从 {} 迁移失败（{rename_error}；复制也失败：{copy_error}）；\
                 插件列表可能为空，不影响 release 内核与工作台",
                legacy.display()
            ),
        },
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
    let release_root = crate::paths::plugins_store_root_for(ShellMode::Release);
    let legacy = crate::paths::legacy_shared_plugins_store_root();
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
    if let Err(error) = crate::plugins::copy_dir_recursive(&source, root) {
        eprintln!(
            "plugins: dev 中央库种子复制失败（{} -> {}）：{error}；dev 壳的插件列表将从空开始，不影响 release",
            source.display(),
            root.display()
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
        let legacy = crate::paths::legacy_shared_plugins_store_root();
        let release = crate::paths::plugins_store_root_for(ShellMode::Release);
        let dev = crate::paths::plugins_store_root_for(ShellMode::Dev);

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
        let release = crate::paths::plugins_store_root_for(ShellMode::Release);
        let dev = crate::paths::plugins_store_root_for(ShellMode::Dev);
        ensure_ready(ShellMode::Release, &release);
        ensure_ready(ShellMode::Dev, &dev);
        assert!(!release.exists(), "无旧目录时不得创建 release 中央库");
        assert!(!dev.exists(), "无旧目录时不得创建 dev 中央库");
        assert!(
            !home.root.join("plugins").exists(),
            "连命名空间父目录都不该被建出来"
        );
    }
}
