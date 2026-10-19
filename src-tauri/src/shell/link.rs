//! 目录链接：把 `source` 链到 `target` 的跨平台原语。
//!
//! 插件物化（`plugins/center.rs`）、内置插件 peer 链接
//! （`plugins/builtin/materialize.rs`）与 `node_modules` 接线都经这里建目录链接。
//! 另一半是删除：[`remove_dir_link`]（同样对瞬时拒绝重试；删链接不动目标）。
//!
//! # 瞬时的 ERROR_ACCESS_DENIED 会挡住整个工作台
//!
//! 内置插件 `openai-oauth` 的 peer 链接建不出来时，`ensure_wired` 报错 →
//! `kernel_adapter` 的 `refresh_enabled` 把它变成 `AdapterError::Io` → 用户看到
//! 「工作台无法启动：内核进程没有被拉起来」，**整个内核都起不来**，而坏的只是一个
//! 插件的一条链接。日志：
//! `~/.dsh-xlink/shell/<mode>/logs/release-builtin-<id>-refresh-<日期>.log`。
//!
//! 实测（2026-10-19，这台机器）：同一个
//! 账户、同一个目标目录（`kernels/<版本>/node_modules/@deepseek-ai/dsh-llm`），
//! **时好时坏**——
//!
//! | 时间 | `symlink_dir` 结果 |
//! | --- | --- |
//! | 07:37 应用启动前刷新内嵌插件 | `ERROR_ACCESS_DENIED`（os error 5） |
// | 07:47 ~ 07:50 本模块之外的临时探针，连试 3 次 | `ERROR_ACCESS_DENIED`（os error 5） |
// | 07:53 之后同一个探针连试 40 次 | 全绿 |
//!
//! 这台机器装着 360安全卫士。**过滤器驱动在扫描目录时会把「在这个目录里建
//! reparse point」拒掉，返回的就是 ERROR_ACCESS_DENIED**——权限本身没问题，
//! `create_dir_all` 在同一个目录里刚成功过。所以这里对**瞬时**失败做有限重试：
//! 一次抖动不该让用户的整个内核起不来。
//!
//! ## `os error 5` 与 `os error 1314` 不是一回事
//!
//! 原文档写过「本机非管理员、**未开开发者模式**」，据此推出「建链不需要开发者模式」，
//! 还把这条结论写进了 README 与 `src-tauri/AGENTS.md`。**2026-10-19 复核证明那个
//! 前提是错的**：
//!
//! - `whoami /priv` 里没有 `SeCreateSymbolicLinkPrivilege`，进程是 Medium 完整性；
//! - 但 `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\AppModelUnlock` 的
//!   `AllowDevelopmentWithoutDevLicense = 1`，注册表键最后写入时间是 **2026-07-15**
//!   ——早于事故三个月，**开发者模式一直是开着的**。
//!
//! 也就是说：这台机器能建链，恰恰是因为它开着开发者模式。换成没开的机器，
//! `CreateSymbolicLinkW` 返回 `ERROR_PRIVILEGE_NOT_HELD`（**1314**，确定性失败），
//! 重试一万次也不会变。两者必须分开：
//!
//! | 错误码 | 性质 | 处置 |
//! | --- | --- | --- |
//! | 5 `ERROR_ACCESS_DENIED` | 瞬时（过滤器驱动扫描） | 重试；仍失败查安全软件 |
//! | 1314 `ERROR_PRIVILEGE_NOT_HELD` | 确定性（没开发者模式） | 一次就上抛；让用户开开发者模式或提权 |
//!
//! 所以 [`is_transient`] 只认 5 / 32 / 33，1314 立即上抛；文案由
//! `shell::error::io_error_guidance` 分别给出。**「不需要开发者模式」这条结论已撤回**，
//! 别再从它出发做设计。
//!
//! ## 不要绕开 std 去自己调 `CreateSymbolicLinkW`
//!
//! 试过，也失败得更隐蔽：`#[link(name = "kernel32")] extern "system"` 声明的
//! `CreateSymbolicLinkW` **返回 TRUE 但什么也没建**（探针里 `rc != 0` 而
//! `symlink_metadata()` 查不到落地物），`windows-sys` 那份则返回
//! `ERROR_PRIVILEGE_NOT_HELD`。std 的封装在本机 40/40 建得出来、且建出来的是
//! 真链接（`is_symlink()` 为真，插件侧判定所有权 / 健康度全靠它）。**换写法只会
//! 得到「看起来成功、实际没有链接」的静默故障**，比报错更糟。
//!
//! ## 也不要退回 junction（`skills/manage.rs` 那条路是另一回事）
//!
//! junction（mount point）不需要任何特权，普通用户即可创建，技能中央库用的就是
//! 它（`skills/manage.rs::make_entry_link`）。代价是 **junction 在
//! `std::fs::Metadata` 里不是符号链接**：`FileType::is_symlink()` 只认
//! `IO_REPARSE_TAG_SYMLINK`，而 junction 的 tag 是 `IO_REPARSE_TAG_MOUNT_POINT`，
//! 于是它 `is_dir()` 为真、`is_symlink()` 为假。插件侧大量判定按 `is_symlink()`
//! 分支（链接健康度、孤儿清扫、所有权），全都要跟着改；技能侧之所以能走 junction，
//! 是因为它已经用内容指纹把所有权判定补齐了。要的是**链接仍然被认成链接**。

use std::fs;
use std::io;
use std::path::Path;
use std::time::Duration;

/// 重试次数（不含首次）。3 次 × 递增等待 ≈ 1.5 秒上限。
const RETRIES: u32 = 3;
/// 首次重试前的等待；之后按次数递增。
const RETRY_DELAY: Duration = Duration::from_millis(250);

/// 建一个目录链接（`target` 指向 `source`）。
///
/// 失败时原样上抛调用方的 `io::Error`——路径与 OS 错误码都由调用方拼进用户可见的
/// 文案，这里不包装，免得把「哪一步失败」的信息吃掉。
pub(crate) fn make_dir_link(source: &Path, target: &Path) -> io::Result<()> {
    retry_transient(&mut || create_dir_link(source, target))
}

/// 删掉一条链接，**对同样的瞬时拒绝也重试**。
///
/// 删除与建链面对的是同一类拒绝：安全软件扫目录时连 `rmdir` / `unlink` 也一起拒。
/// 只给建链加重试、把删除留给调用方裸调 `remove_dir`，症状是「上一次抖动留下的
/// 旧链接删不掉」→ 下一次建链撞 `ERROR_ALREADY_EXISTS`（183，确定性失败、不重试）
/// → 用户看到的仍是「工作台起不来」，而根因是删除那一步。
///
/// 与 [`crate::pkg::fetch::remove_link`] 的分工：那个是**尽力而为**的清理
/// （目标不存在、权限不足都当没事，调用方不关心结果）；这个要**如实返回失败**，
/// 因为它前面就是一次「必须先摘掉旧产物」的重建。两者删法一致：先 `remove_file`
/// 再 `remove_dir`，覆盖文件与目录两种 reparse point。
pub(crate) fn remove_dir_link(path: &Path) -> io::Result<()> {
    retry_transient(&mut || match fs::remove_file(path) {
        Ok(()) => Ok(()),
        // 两条都试：Windows 上 reparse point 走 `remove_file` 就删掉了，Unix 上
        // 目录链接是目录形态、只有 `remove_dir` 认。上抛目录那一条的错误——
        // 目标按契约是目录链接，它的错误码才是语义正确的那一个。
        Err(_) => fs::remove_dir(path),
    })
}

/// 只对 [`is_transient`] 的失败重试，其余原样上抛。
///
/// 抽成独立函数是为了能**单测**：真正触发它的安全软件没法在测试里复现，
/// 「瞬时重试、确定性立即失败」这条判据本身就是必须被钉住的行为。
fn retry_transient(create: &mut dyn FnMut() -> io::Result<()>) -> io::Result<()> {
    let mut retried = 0;
    loop {
        match create() {
            Ok(()) => return Ok(()),
            Err(error) if retried < RETRIES && is_transient(&error) => {
                std::thread::sleep(RETRY_DELAY * (retried + 1));
                retried += 1;
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(unix)]
fn create_dir_link(source: &Path, target: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(source, target)
}

#[cfg(windows)]
fn create_dir_link(source: &Path, target: &Path) -> io::Result<()> {
    std::os::windows::fs::symlink_dir(source, target)
}

/// 这类失败值得再试一次吗。
///
/// **只认瞬时**：Windows 上是过滤器驱动 / 索引服务扫目录时给的拒绝
/// （`ERROR_ACCESS_DENIED` 5、`ERROR_SHARING_VIOLATION` 32、`ERROR_LOCK_VIOLATION`
/// 33）。`ERROR_ALREADY_EXISTS` 183 不在其中——那是调用方「先删旧链接」没删干净，
/// 重试只会把同一个确定性失败拖慢（调用方自己负责清旧产物）。
#[cfg(windows)]
fn is_transient(error: &io::Error) -> bool {
    matches!(error.raw_os_error(), Some(5 | 32 | 33))
}

/// Unix 侧只认「被信号打断」。
///
/// **`EACCES` / `EPERM`（`PermissionDenied`）不重试**：那边没有过滤器驱动那类
/// 瞬时拒绝，权限不足就是确定性的（父目录不可写、只读挂载），重试只会白等 1.5 秒
/// 再报同一个错。两个平台的判据必须口径一致：都只认「重试真的可能改变结果」的失败。
#[cfg(unix)]
fn is_transient(error: &io::Error) -> bool {
    matches!(error.kind(), io::ErrorKind::Interrupted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "xlink-link-{tag}-{}-{}",
            std::process::id(),
            crate::shell::process::epoch_millis()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 目录链接必须真的建得出来，且落地物**仍然是符号链接**——插件侧的链接健康度、
    /// 孤儿清扫、所有权判定全按 `is_symlink()` 分支走，junction 那类替代物会把它们
    /// 全部带歪（见模块文档）。
    #[test]
    fn dir_link_resolves_to_source_and_survives_removal() {
        let dir = scratch("roundtrip");
        let source = dir.join("source");
        let link = dir.join("link");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("marker.txt"), "payload").unwrap();

        make_dir_link(&source, &link).expect("建立目录链接");

        let md = fs::symlink_metadata(&link).unwrap();
        assert!(
            md.file_type().is_symlink(),
            "落地物必须是符号链接，否则插件侧按 is_symlink 判定的分支全走错"
        );
        // 顺着链接读得到源里的内容（Node 解析 import 依赖的正是这一条）。
        assert_eq!(
            fs::read_to_string(link.join("marker.txt")).unwrap(),
            "payload"
        );

        // 删链接不动目标：错删就是「把内核包删了」。
        //
        // **必须走本模块的 [`remove_dir_link`]，不能裸 `fs::remove_dir`**：Unix 上
        // `remove_dir` 落到 `rmdir()`，而 POSIX 规定 `rmdir` 对符号链接返回
        // `ENOTDIR`——本用例在 macOS 上会红在「删除失败」，与它要钉的语义无关
        // （2026-10-19 CI 实测）。Windows 的 `RemoveDirectoryW` 对 reparse point
        // 恰好成功，所以本机永远复现不出来。删法两条都试也是这个原因。
        remove_dir_link(&link).expect("删掉链接");
        assert!(link.symlink_metadata().is_err());
        assert!(source.join("marker.txt").is_file());
        let _ = fs::remove_dir_all(&dir);
    }

    /// 源的目录被删掉之后，链接自身仍然可以被识别并删除——重链时
    /// `ensure_one_peer_link` 走的就是这条路（旧链接悬空时重建）。
    #[test]
    fn dangling_link_is_still_removable() {
        let dir = scratch("dangling");
        let source = dir.join("source");
        let link = dir.join("link");
        fs::create_dir_all(&source).unwrap();
        make_dir_link(&source, &link).expect("建立目录链接");
        fs::remove_dir_all(&source).unwrap();

        assert!(link.symlink_metadata().is_ok(), "链接自身仍在");
        assert!(!link.exists(), "顺着链接看是悬空的");
        remove_dir_link(&link).expect("悬空链接也要删得掉");
        assert!(link.symlink_metadata().is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    /// 一个在本平台上**确实会被判成瞬时**的错误。
    ///
    /// 判据是分平台的，测试的素材也必须分平台：`from_raw_os_error(5)` 在 Windows 上
    /// 是 `ERROR_ACCESS_DENIED`，在 Unix 上却是 `EIO`——照抄 Windows 的错误码会让
    /// 这两条用例在 macOS 上必红，而红的原因与它们要钉的行为无关（2026-10-19 CI）。
    fn transient_error() -> io::Error {
        #[cfg(windows)]
        {
            io::Error::from_raw_os_error(5)
        }
        #[cfg(unix)]
        {
            io::Error::from(io::ErrorKind::Interrupted)
        }
    }

    /// 一个在本平台上**确实会被判成确定性**的错误。
    fn permanent_error() -> io::Error {
        #[cfg(windows)]
        {
            // ERROR_ALREADY_EXISTS：调用方「先删旧链接」没删干净。
            io::Error::from_raw_os_error(183)
        }
        #[cfg(unix)]
        {
            // EACCES：父目录不可写 / 只读挂载，重试不会改变结果。
            io::Error::from(io::ErrorKind::PermissionDenied)
        }
    }

    /// 分类判据本身：哪些错误会被重试，哪些必须立刻上抛。
    ///
    /// **反向验**：把 `ERROR_SHARING_VIOLATION`（32）从 Windows 的瞬时集合里去掉，
    /// 本用例在 Windows 上红——它就是判据不是凑数的。
    #[test]
    fn transient_classification_covers_filter_driver_denial() {
        assert!(is_transient(&transient_error()), "这一档必须重试");
        assert!(
            !is_transient(&permanent_error()),
            "确定性失败不该被重试：重试只会白等 1.5 秒再报同一个错"
        );
        #[cfg(windows)]
        {
            assert!(is_transient(&io::Error::from_raw_os_error(32)));
            assert!(is_transient(&io::Error::from_raw_os_error(33)));
            assert!(
                !is_transient(&io::Error::from_raw_os_error(183)),
                "已存在是确定性失败，重试只会拖慢"
            );
            assert!(
                !is_transient(&io::Error::from_raw_os_error(2)),
                "路径不存在重试没有意义"
            );
        }
        #[cfg(unix)]
        {
            assert!(
                !is_transient(&io::Error::from(io::ErrorKind::PermissionDenied)),
                "Unix 上没有过滤器驱动那类瞬时拒绝，EACCES 就是确定性失败"
            );
        }
    }

    /// 瞬时失败必须被重试到成功，确定性失败必须**一次就报**。
    ///
    /// 用计数闭包模拟「前两次被过滤器驱动拒、第三次放行」，断言 `create_dir_link`
    /// 被调了 3 次且最终成功。
    #[test]
    fn transient_failures_are_retried_and_eventually_succeed() {
        let mut attempts = 0;
        let outcome = retry_transient(&mut || {
            attempts += 1;
            if attempts < 3 {
                Err(transient_error())
            } else {
                Ok(())
            }
        });
        assert!(outcome.is_ok(), "第 3 次成功就该返回 Ok");
        assert_eq!(attempts, 3, "前两次必须真的重试过");
    }

    /// 确定性失败一次就上抛：peer 链接建不出来时用户的内核是起不来的，别拿重试
    /// 把它拖成十几秒。
    #[test]
    fn permanent_failures_surface_without_retrying() {
        let mut attempts = 0;
        let outcome: io::Result<()> = retry_transient(&mut || {
            attempts += 1;
            Err(permanent_error())
        });
        assert_eq!(
            outcome.unwrap_err().kind(),
            permanent_error().kind(),
            "上抛的必须是原来那个错误"
        );
        assert_eq!(attempts, 1, "确定性失败不该重试");
    }
}
