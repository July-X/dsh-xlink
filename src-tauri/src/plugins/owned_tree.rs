//! 归属树：壳自己拥有的一棵目录树——归属标记 + 有上限的遍历与内容摘要。
//!
//! 记忆插件与 Ponytail 都要「认领一棵树、验证它没被改过」。这部分形状一样：
//! 树内一个 `.xlink-owner` 标记、一棵有上限的遍历（不跟链接、不吃特殊文件）、
//! 以及一份内容摘要。各自的文件名、文案与日志事务留在各自的模块里——
//! 那两套事务的恢复语义不同（一个认收据，一个认日志），硬合成一份只会多出
//! 一堆开关。

use std::fs;
use std::path::Path;

use sha2::{Digest, Sha256};

/// 树内归属标记的文件名。
pub(crate) const MARKER: &str = ".xlink-owner";

/// 遍历上限：层级、条目数、单文件大小、总大小。
const MAX_DEPTH: usize = 32;
const MAX_ENTRIES: usize = 2048;
const MAX_FILE: u64 = 16 * 1024 * 1024;
const MAX_TOTAL: u64 = 64 * 1024 * 1024;

/// 遍历结果里的一项：相对路径、类型（`d` 目录 / `f` 文件）、文件内容摘要。
pub(crate) struct Entry {
    pub path: String,
    pub kind: u8,
    pub digest: String,
}

/// 遍历一棵树：链接与特殊文件一律拒绝，条目与字节都有上限。
///
/// 返回按路径排序的条目（根目录自身不在里面）。文件的内容摘要在这里算好，
/// 免得调用方为了一个摘要再走一遍树。
pub(crate) fn scan(root: &Path) -> Result<Vec<Entry>, String> {
    fn visit(
        root: &Path,
        path: &Path,
        depth: usize,
        count: &mut usize,
        bytes: &mut u64,
        entries: &mut Vec<Entry>,
    ) -> Result<(), String> {
        *count += 1;
        if depth > MAX_DEPTH || *count > MAX_ENTRIES {
            return Err("归属树层级或文件数量过多".into());
        }
        let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink() || (!metadata.is_dir() && !metadata.is_file()) {
            return Err(format!("归属树包含链接或特殊文件：{}", path.display()));
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|error| error.to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        if metadata.is_file() {
            if metadata.len() > MAX_FILE {
                return Err("归属树文件过大".into());
            }
            *bytes = bytes.checked_add(metadata.len()).ok_or("归属树大小无效")?;
            if *bytes > MAX_TOTAL {
                return Err("归属树总大小过大".into());
            }
            let digest = crate::shell::digest::hex(
                &Sha256::digest(fs::read(path).map_err(|error| error.to_string())?)[..],
            );
            if depth > 0 {
                entries.push(Entry {
                    path: relative,
                    kind: b'f',
                    digest,
                });
            }
        } else {
            if depth > 0 {
                entries.push(Entry {
                    path: relative,
                    kind: b'd',
                    digest: String::new(),
                });
            }
            for item in fs::read_dir(path).map_err(|error| error.to_string())? {
                visit(
                    root,
                    &item.map_err(|error| error.to_string())?.path(),
                    depth + 1,
                    count,
                    bytes,
                    entries,
                )?;
            }
        }
        Ok(())
    }
    let mut entries = Vec::new();
    visit(root, root, 0, &mut 0, &mut 0, &mut entries)?;
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(entries)
}

/// 归属树摘要：路径 + 类型 + 内容摘要。
///
/// 空目录也算进摘要——多一个空目录是「内容变了」，不能靠摘要看不出来。
pub(crate) fn digest_entries(entries: &[Entry]) -> String {
    let mut hasher = Sha256::new();
    for entry in entries {
        hasher.update(entry.path.as_bytes());
        hasher.update([0, entry.kind, 0]);
        hasher.update(entry.digest.as_bytes());
        hasher.update([0]);
    }
    crate::shell::digest::hex(&hasher.finalize()[..])
}
