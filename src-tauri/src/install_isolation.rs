//! 检测一棵已安装的内核树是否仍与其他目录共享 inode。
//!
//! ## 为什么需要它（2026-09-30 定案的机制，见 AGENTS.md「装 / 删内核不许惊动
//! 另一个壳的工作台」）
//!
//! pnpm 的内容寻址 store 按**文件内容**去重：旧版安装（硬链接导入）的树与
//! store、另一个壳的树里内容相同的文件是**同一个 inode**，而 NTFS 上硬链接数
//! 增减会更新 ChangeTime——删除 / 重装这种树会短暂惊动对面正在服务的工作台
//! 页面。2026-09-30 起安装固定 `package-import-method=copy`，新树持有全新
//! inode，物理上碰不到任何别的文件；**存量旧树**则仍是共享的。
//!
//! 这个模块回答的就是「这棵树是哪一种」：任一采样文件的硬链接数 > 1 ⇒ 共享。
//! 由此版本页能给每个版本标「共享存储」、横幅能按**实际残余风险**决定是否
//! 出现（本壳全部版本独立 ⇒ 横幅消失），而不是无条件常驻吓唬用户。
//!
//! ## 采样面与误判方向
//!
//! 采样 = 内核入口 + `@deepseek-ai/dsh-client*` 的客户端 bundle + 若干第三方
//! 包的 `package.json`。**必须包含第三方包**：锁步重装可能只换官方子包、第三方
//! 沿用旧硬链接，只采 dsh 包会把这种混合树误判成「独立」。
//!
//! 误判方向的取舍：误报「共享」只是多显示一条横幅（烦），漏报则是横幅消失后
//! 用户删树惊动对面一次（有自愈兜底）。因此**读不出来一律按共享**（保守）。
//! 采样是穷举不了的，混合树存在理论上的漏报面——靠两侧门控 + 自愈兜底。
//!
//! ## 开销纪律
//!
//! `status()` 每 2.5s 轮询都会走到这里：每版本约 30 次文件打开 + 读链接数，
//! 纯元数据操作、不 spawn 任何进程（`fsutil hardlink list` 一次几十毫秒，
//! 轮询路径上用不起）。**只读不写**——打开文件读链接数不会改 ChangeTime，
//! 这一点本身就是这次事故的机制，不能自己踩。

use std::fs;
use std::path::{Path, PathBuf};

/// 采样上限（不含内核入口）。上限不是全量的替代，是把单次轮询的固定开销
/// 封住；覆盖面见模块文档。
const CLIENT_BUNDLE_CAP: usize = 12;
const THIRD_PARTY_CAP: usize = 20;

/// 这棵内核树是否仍与其他目录共享 inode（旧版硬链接安装）。
///
/// 读不出来、树不完整 ⇒ `true`（保守：宁可多一条横幅，不漏一次惊动）。
pub fn tree_shares_inodes(kernel_dir: &Path) -> bool {
    let probes = collect_probes(kernel_dir);
    if probes.is_empty() {
        return true;
    }
    let mut opened = 0usize;
    for probe in &probes {
        if let Some(links) = file_link_count(probe) {
            opened += 1;
            if links > 1 {
                return true;
            }
        }
    }
    // 一个文件都没打开成功：树在但读不了，按共享处理。
    opened == 0
}

/// 组装采样面：内核入口 + 官方客户端 bundle + 第三方包清单。
fn collect_probes(kernel_dir: &Path) -> Vec<PathBuf> {
    let mut probes = vec![kernel_dir.join(crate::kernel::KERNEL_BIN_REL)];
    let scope = kernel_dir.join("node_modules").join("@deepseek-ai");
    if let Ok(entries) = fs::read_dir(&scope) {
        for entry in entries.flatten() {
            if probes.len() > CLIENT_BUNDLE_CAP {
                break;
            }
            let name = entry.file_name();
            if name.to_string_lossy().starts_with("dsh-client") {
                probes.push(scope.join(&name).join("lib").join("client.js"));
            }
        }
    }
    // 第三方包（hoisted 直下、非 scope）：锁步重装换官方包时它们最可能保留
    // 旧硬链接，采样缺了它们混合树会漏判。
    let root = kernel_dir.join("node_modules");
    if let Ok(entries) = fs::read_dir(&root) {
        for entry in entries.flatten() {
            if probes.len() > CLIENT_BUNDLE_CAP + THIRD_PARTY_CAP {
                break;
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !name.starts_with('.') && !name.starts_with('@') {
                probes.push(root.join(name.as_ref()).join("package.json"));
            }
        }
    }
    probes
}

/// 一个文件的硬链接数；打不开（不存在 / 无权限）返回 `None`。
#[cfg(windows)]
fn file_link_count(path: &Path) -> Option<u32> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };
    let file = fs::File::open(path).ok()?;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY：句柄是自己刚打开的 `fs::File`（借用不转移），输出结构体按可变
    // 引用交给系统填充；`GetFileInformationByHandle` 不写句柄以外的任何东西。
    let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle() as HANDLE, &mut info) };
    (ok != 0).then_some(info.nNumberOfLinks)
}

/// Unix 上 `stat` 自带 `st_nlink`，无需开句柄。
#[cfg(unix)]
fn file_link_count(path: &Path) -> Option<u32> {
    use std::os::unix::fs::MetadataExt;
    fs::metadata(path).ok().map(|meta| meta.nlink() as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一棵最小内核树：内核入口 + 一个客户端 bundle + 一个第三方包。
    fn plant_tree(dir: &Path) {
        let bin = dir.join(crate::kernel::KERNEL_BIN_REL);
        fs::create_dir_all(bin.parent().unwrap()).unwrap();
        fs::write(&bin, "// bin").unwrap();
        let bundle = dir.join("node_modules/@deepseek-ai/dsh-client-fake/lib/client.js");
        fs::create_dir_all(bundle.parent().unwrap()).unwrap();
        fs::write(&bundle, "// bundle").unwrap();
        let pkg = dir.join("node_modules/protobuf-fake/package.json");
        fs::create_dir_all(pkg.parent().unwrap()).unwrap();
        fs::write(&pkg, "{}").unwrap();
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "install-iso-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// copy 落盘的树：每个文件只有自己一条链接 ⇒ 独立。
    #[test]
    fn a_copy_installed_tree_reads_as_isolated() {
        let dir = temp_dir("copy");
        plant_tree(&dir);
        assert!(
            !tree_shares_inodes(&dir),
            "全新 inode 的树必须读成独立——读错这条，版本页的横幅就永远消不掉"
        );
        fs::remove_dir_all(&dir).ok();
    }

    /// 旧版硬链接导入的树：任一文件多一条链接 ⇒ 共享。两个平台都能造硬链接
    /// （`std::fs::hard_link`），不依赖 Windows。
    #[test]
    fn a_hardlinked_tree_reads_as_shared() {
        let dir = temp_dir("hard");
        plant_tree(&dir);
        let bin = dir.join(crate::kernel::KERNEL_BIN_REL);
        let alias = dir.join("node_modules").join("link-alias.js");
        fs::hard_link(&bin, &alias).expect("hard link");
        assert!(
            tree_shares_inodes(&dir),
            "硬链接树必须读成共享——漏判这条，横幅会在真风险还在时消失"
        );
        fs::remove_dir_all(&dir).ok();
    }

    /// 读不了的树按共享处理：保守方向是「多一条横幅」而不是「漏一次惊动」。
    #[test]
    fn an_unreadable_tree_reads_as_shared() {
        let dir = temp_dir("empty");
        fs::create_dir_all(&dir).unwrap();
        assert!(tree_shares_inodes(&dir), "空 / 残缺树按共享处理");
        fs::remove_dir_all(&dir).ok();
    }
}
