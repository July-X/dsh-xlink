//! 只读拷贝一个浏览器 profile 目录，供验证用。
//!
//! # 为什么必须拷贝而不是直接指向
//!
//! 浏览器一启动就会往 user-data 目录里写 Cookies / Preferences / History /
//! IndexedDB —— 哪怕只是「打开页面看一眼」。所以**验证阶段绝不能**把探针
//! 指向用户真实的数据目录：写坏了回滚不了（Cookies 是活文件，没有版本可
//! 还原）。
//!
//! # 拷贝规则
//!
//! 1. **源只读**：只从源读，绝不写、绝不删。
//! 2. 写到**一次性临时目录**，跑完删掉；删的只能是**自己那块**临时目录。
//! 3. **容错**：源被运行中的浏览器独占时（Windows 上 Cookies 常见），
//!    单个文件拷不动就跳过并记账，其余照拷——浏览器照样能起来。
//!
//! Windows 上 Cookies 由 DPAPI 绑机器 + 用户、**不绑路径**，所以拷贝出来的
//! 登录态是可移植的（macOS 的 Keychain 绑定不同，不能这样搬）。

use std::path::{Path, PathBuf};

/// 一次拷贝的结果（写进证据里）。
#[derive(Debug, Clone, Default)]
pub(crate) struct CopyReport {
    /// 拷过去的文件数。
    pub files: usize,
    /// 拷贝的总字节。
    pub bytes: u64,
    /// 跳过的文件（源被独占等）。
    pub skipped: Vec<String>,
    /// 跳过的原因摘要。
    pub skip_reason: Option<String>,
}

/// 把 `src` 目录**只读**拷到 `dst`。
pub(crate) fn copy_dir_readonly(src: &Path, dst: &Path) -> std::io::Result<CopyReport> {
    let mut report = CopyReport::default();
    std::fs::create_dir_all(dst)?;
    copy_inner(src, dst, &mut report)?;
    Ok(report)
}

fn copy_inner(src: &Path, dst: &Path, report: &mut CopyReport) -> std::io::Result<()> {
    let entries = match std::fs::read_dir(src) {
        Ok(entries) => entries,
        Err(e) => {
            report.skipped.push(format!("{}: {e}", src.display()));
            report.skip_reason.get_or_insert_with(|| e.to_string());
            return Ok(());
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                report.skipped.push(format!("{src:?}: {e}"));
                continue;
            }
        };
        let path = entry.path();
        let name = entry.file_name();
        let target = dst.join(&name);
        let file_type = match entry.file_type() {
            Ok(t) => t,
            Err(e) => {
                report.skipped.push(format!("{}: {e}", path.display()));
                continue;
            }
        };
        if file_type.is_dir() {
            // 子目录必须**先建出来**，否则下面的文件拷贝会因为父目录不存在
            // 而整体失败（os error 3）。
            if let Err(e) = std::fs::create_dir_all(&target) {
                report.skipped.push(format!("{}: {e}", target.display()));
                report.skip_reason.get_or_insert_with(|| e.to_string());
                continue;
            }
            copy_inner(&path, &target, report)?;
        } else if file_type.is_file() {
            // 源被独占时拷贝会失败——跳过并记账，不让它毁掉整次拷贝。
            match std::fs::copy(&path, &target) {
                Ok(n) => {
                    report.files += 1;
                    report.bytes += n;
                }
                Err(e) => {
                    report.skipped.push(format!("{}: {e}", path.display()));
                    report.skip_reason.get_or_insert_with(|| e.to_string());
                }
            }
        }
    }
    Ok(())
}

/// 删掉**自己那块**临时拷贝目录。
///
/// 只删 `root` 之下的一切，且要求 `root` 名字里带本模块的标记，防止误删
/// 任何别的东西。**绝不删源**。
pub(crate) fn remove_temp_copy(root: &Path) -> std::io::Result<()> {
    let name = root
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    if !name.starts_with("dsh-official-chat-probe-") {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("拒绝删除不是本模块创建的临时目录：{name}"),
        ));
    }
    if root.exists() {
        std::fs::remove_dir_all(root)?;
    }
    Ok(())
}

/// 建一个一次性临时拷贝目录的根路径。
pub(crate) fn temp_copy_root() -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("dsh-official-chat-probe-{stamp}"))
}
