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
//! 4. **可选整棵排除**（[`copy_dir_readonly_excluding`]）：取证时常常需要
//!    「**登录态留着、浏览器自己的站点缓存摘掉**」那份 profile ⇒ ⇒ 源目录
//!    一个字都不动（仍然只读）⇒ ⇒ 只在**副本**里少几棵目录。
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
    /// **按名字整棵排除掉的目录**（取证用：把浏览器的站点缓存摘掉、只留 cookies）。
    pub excluded_dirs: Vec<String>,
}

/// 把 `src` 目录**只读**拷到 `dst`。
pub(crate) fn copy_dir_readonly(src: &Path, dst: &Path) -> std::io::Result<CopyReport> {
    copy_dir_readonly_excluding(src, dst, &[])
}

/// 同上，但**按目录名**排除若干整棵子树（`skip_dir_names` 命中即整棵不拷）。
///
/// **为什么要有这个**：站点的「本地缓存」（IndexedDB / Local Storage）里可能
/// 存着它自己那份**消息缓存** ⇒ ⇒ 拿它去测「站点会不会把消息放上网」时，
/// 必须有一份「**登录态在、缓存不在**」的 profile ⇒ ⇒ 而**源目录一个字都不
/// 能动**（只读）⇒ ⇒ 只在副本里少几棵目录。
pub(crate) fn copy_dir_readonly_excluding(
    src: &Path,
    dst: &Path,
    skip_dir_names: &[String],
) -> std::io::Result<CopyReport> {
    let mut report = CopyReport::default();
    std::fs::create_dir_all(dst)?;
    copy_inner(src, dst, &mut report, skip_dir_names)?;
    Ok(report)
}

fn copy_inner(
    src: &Path,
    dst: &Path,
    report: &mut CopyReport,
    skip_dir_names: &[String],
) -> std::io::Result<()> {
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
            // **按名字整棵排除**（取证用）：源一个字都不动，只在副本里少这棵。
            let name_str = name.to_string_lossy().to_string();
            if skip_dir_names.iter().any(|n| n == &name_str) {
                report.excluded_dirs.push(name_str);
                continue;
            }
            // 子目录必须**先建出来**，否则下面的文件拷贝会因为父目录不存在
            // 而整体失败（os error 3）。
            if let Err(e) = std::fs::create_dir_all(&target) {
                report.skipped.push(format!("{}: {e}", target.display()));
                report.skip_reason.get_or_insert_with(|| e.to_string());
                continue;
            }
            copy_inner(&path, &target, report, skip_dir_names)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excludes_nested_cache_directories_without_changing_the_source() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "dsh-official-chat-probe-copy-test-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir(&root).unwrap();
        let src = root.join("source");
        let dst = root.join("filtered");
        let complete = root.join("complete");
        let cache = "Default/IndexedDB/messages";
        let cookie = "Default/Network/Cookies";
        std::fs::create_dir_all(src.join("Default/IndexedDB")).unwrap();
        std::fs::create_dir_all(src.join("Default/Network")).unwrap();
        std::fs::write(src.join(cache), b"cached messages").unwrap();
        std::fs::write(src.join(cookie), b"synthetic login").unwrap();

        let report = copy_dir_readonly_excluding(&src, &dst, &["IndexedDB".into()]).unwrap();
        assert_eq!(report.excluded_dirs, ["IndexedDB"]);
        assert_eq!(report.files, 1);
        assert_eq!(report.bytes, 15);
        assert!(report.skipped.is_empty());
        assert!(!dst.join("Default/IndexedDB").exists());
        assert_eq!(std::fs::read(dst.join(cookie)).unwrap(), b"synthetic login");
        assert_eq!(std::fs::read(src.join(cache)).unwrap(), b"cached messages");
        assert_eq!(std::fs::read(src.join(cookie)).unwrap(), b"synthetic login");

        let report = copy_dir_readonly(&src, &complete).unwrap();
        assert_eq!(report.files, 2);
        assert!(report.excluded_dirs.is_empty());
        assert_eq!(
            std::fs::read(complete.join(cache)).unwrap(),
            b"cached messages"
        );
        for dir in [&src, &dst, &complete] {
            std::fs::remove_file(dir.join(cookie)).unwrap();
            std::fs::remove_dir(dir.join("Default/Network")).unwrap();
            if dir.join(cache).exists() {
                std::fs::remove_file(dir.join(cache)).unwrap();
                std::fs::remove_dir(dir.join("Default/IndexedDB")).unwrap();
            }
            std::fs::remove_dir(dir.join("Default")).unwrap();
            std::fs::remove_dir(dir).unwrap();
        }
        std::fs::remove_dir(root).unwrap();
    }
}
