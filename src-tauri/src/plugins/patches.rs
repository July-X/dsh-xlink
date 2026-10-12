//! 补丁的**只读状态缝**——曾经那套「应用 / 撤销补丁」机制已整体删除（2026-10-11）。
//!
//! ## 为什么删
//!
//! 唯一还内置的补丁 `dsh-file-perf` 在 `manifest.json` 里声明
//! `supersededSinceKernelVersion: 0.1.2-alpha.2`：该版本起修复已被官方内核采纳，
//! 补丁对它无效。实际算下来，它的适用窗口是 `[0.1.1-rc.2, 0.1.2-alpha.2)`——
//! npm 上**只有 `0.1.1-rc.1` 与 `0.1.1-rc.2` 两个版本**落在这个窗口里，而当前
//! 最新已是 `0.2.0-rc.2`。而 UI 侧的应用入口早在「设置 → 内核补丁」页移除时就
//! 没了（`ui/src/shell/SettingsPanel.vue` 顶部注释记着那次改动），留下的
//! `ui/src/plugins/patches.js` **零 importer**，`patch_status` / `patch_apply` /
//! `patch_revert` 三个 Tauri 命令注册着却无人调用。
//!
//! 保留这套机制的**唯一理由**原文是「便于旧内核撤销」——而那个窗口里只剩两个
//! 一年前的版本，用户几乎不可能还在跑，且他们一旦升级内核，补丁本来就会自动失效。
//!
//! ## 为什么还留这个文件
//!
//! **历史数据不能当作不存在**。删除机制 ≠ 用户的 `<data_dir>/patches/state.json`
//! 消失。已打过补丁的用户磁盘上仍有那份记录，而
//! [`crate::diagnostics::snapshot`] 要把「已应用补丁集」并进环境指纹——那决定
//! 「回到良好状态」认不认得当前这套配置。直接删掉这个读取面，症状是：
//! 打过分��的用户升级壳之后，环境指纹里「补丁 N」这一项凭空消失，回退点的
//! 判定基准被静默改动，而用户什么也没做。
//!
//! 所以这里只留 [`applied_patch_ids`] 一条**只读**缝，加它依赖的状态结构与
//! 容错读取。应用 / 撤销 / 状态查询 / 清单解析 / 备份与哈希校验，全部删掉——
//! 它们一个都没有生产调用方。
//!
//! ## 留下的四个单测各钉什么
//!
//! 这个文件小，但它守着的是**别人机器上的存量数据**，四个用例都是数据形态的
//! 回归（不是覆盖率）：
//! - 没有 `state.json` → 空列表（全新安装）
//! - 只取同一内核版本的记录（换版本即换集合）
//! - 老版 `state.json` 缺 `patchVersion` 仍能解析——**存量兼容的真正形态**
//! - 损坏降级为「读不到」而不 panic——这条读取缝在启动期的指纹采集路径上
//!
//! ## 对存量用户意味着什么
//!
//! 已应用补丁的用户此后**无法在 UI 上撤销**（撤销入口与命令都已删）。这与
//! `AGENTS.md` 记的那条一致：**从仓库里删掉补丁目录，并不会让已安装的用户机器上
//! 消失**——更新只覆盖与新增。所以删除补丁必须写进发布说明。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// 运行时状态的子目录（位于 `data_dir` 下）。
const PATCHES_SUBDIR: &str = "patches";

/// 应用记录文件。
const STATE_FILE: &str = "state.json";

/// 当前状态 schema 版本。
const STATE_SCHEMA_VERSION: u32 = 1;

/// 应用记录（`state.json` 中按「补丁 × 内核版本」组织）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchState {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u32,
    pub applied: Vec<AppliedPatch>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedPatch {
    pub id: String,
    #[serde(rename = "kernelVersion")]
    pub kernel_version: String,
    #[serde(rename = "appliedAt")]
    pub applied_at: String,
    /// 应用时使用的补丁定义版本。旧版 state.json 没有该字段时按过期记录处理。
    #[serde(rename = "patchVersion", default)]
    pub patch_version: Option<String>,
    /// 应用过程中跳过 / 警告的说明（非必需文件未命中、备份丢失等）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    pub files: Vec<AppliedFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedFile {
    pub to: String,
    /// 应用前目标是否已存在（存在时必有备份）。
    #[serde(rename = "hadOriginal")]
    pub had_original: bool,
    /// 应用后（当前应为）文件内容的 SHA-256。
    #[serde(rename = "patchedSha256")]
    pub patched_sha256: String,
    /// 备份文件相对 `<data_dir>/patches/backups/` 的路径。
    #[serde(rename = "backupRel", skip_serializing_if = "Option::is_none")]
    pub backup_rel: Option<String>,
    /// 应用前原文件内容的 SHA-256（应用时目标不存在则为 `None`）。
    #[serde(
        rename = "originalSha256",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub original_sha256: Option<String>,
}

impl Default for PatchState {
    fn default() -> Self {
        Self {
            schema_version: STATE_SCHEMA_VERSION,
            applied: Vec::new(),
        }
    }
}

fn state_dir(data_dir: &Path) -> PathBuf {
    data_dir.join(PATCHES_SUBDIR)
}

fn state_file(data_dir: &Path) -> PathBuf {
    state_dir(data_dir).join(STATE_FILE)
}

/// 展示路径用的容错读取：文件不存在或损坏都返回空状态。
///
/// 损坏当空在这里是**安全**的：唯一的读者是环境指纹的采集，它把补丁 id 折进
/// 摘要里，读不出只影响「回退点认为配置变了」，不会像「读到空清单就清扫链接」
/// 那样造成删除。
fn read_state(data_dir: &Path) -> PatchState {
    crate::shell::state::load_lossy(&state_file(data_dir))
}

/// 只读列出「当前内核上已应用的补丁 id」。
///
/// 快照模块（[`crate::diagnostics::snapshot`]）需要把已应用补丁集并进环境指纹，但不该
/// 因此拿到整个 [`PatchState`]——那里面还有备份路径、时间戳等与指纹无关
/// 的字段。给一条最小只读缝，比让它 `pub(crate)` 整个状态结构更窄。
pub fn applied_patch_ids(data_dir: &Path, kernel_version: &str) -> Vec<String> {
    read_state(data_dir)
        .applied
        .into_iter()
        .filter(|item| item.kernel_version == kernel_version)
        .map(|item| item.id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn data_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "dsh-patch-state-{label}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create data dir");
        dir
    }

    fn write_state(data_dir: &Path, body: &str) {
        let dir = state_dir(data_dir);
        fs::create_dir_all(&dir).expect("create state dir");
        fs::write(state_file(data_dir), body).expect("write state");
    }

    #[test]
    fn a_missing_state_file_yields_no_patch_ids() {
        let dir = data_dir("missing");
        assert!(
            applied_patch_ids(&dir, "0.2.0-rc.2").is_empty(),
            "没有 state.json 就是没打过补丁——全新安装的正常形态"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_ids_for_the_given_kernel_version_are_returned() {
        let dir = data_dir("versions");
        write_state(
            &dir,
            r#"{"schemaVersion":1,"applied":[
              {"id":"dsh-file-perf","kernelVersion":"0.1.1-rc.2","appliedAt":"2026-01-01T00:00:00Z","files":[]},
              {"id":"dsh-file-perf","kernelVersion":"0.2.0-rc.2","appliedAt":"2026-02-01T00:00:00Z","files":[]},
              {"id":"other","kernelVersion":"0.2.0-rc.2","appliedAt":"2026-02-01T00:00:00Z","files":[]}
            ]}"#,
        );
        let ids = applied_patch_ids(&dir, "0.2.0-rc.2");
        assert_eq!(
            ids,
            vec!["dsh-file-perf".to_string(), "other".to_string()],
            "只取同一内核版本的记录，且保持记录里的顺序"
        );
        assert!(
            applied_patch_ids(&dir, "0.1.1-rc.2") == vec!["dsh-file-perf".to_string()],
            "换个版本只拿该版本自己的记录"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// 存量兼容的真实形态：老版 state.json 里没有 `patchVersion` 字段。
    /// 这条红了就等于「删除机制顺带让老用户的补丁记录读不出来」——而那正是
    /// 环境指纹静默改变、用户什么也没做的那条路径。
    #[test]
    fn an_older_state_file_without_patch_version_still_parses() {
        let dir = data_dir("legacy");
        write_state(
            &dir,
            r#"{"schemaVersion":1,"applied":[
              {"id":"dsh-file-perf","kernelVersion":"0.2.0-rc.2","appliedAt":"2026-02-01T00:00:00Z",
               "files":[{"to":"a.js","hadOriginal":true,"patchedSha256":"ab","backupRel":"x"}]}
            ]}"#,
        );
        let ids = applied_patch_ids(&dir, "0.2.0-rc.2");
        assert_eq!(
            ids,
            vec!["dsh-file-perf".to_string()],
            "缺 patchVersion 是历史形态，不是损坏"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// 损坏必须**降级为「读不到」而不是崩**——这条读取缝在启动期的指纹采集路径上。
    #[test]
    fn a_corrupt_state_file_degrades_instead_of_panicking() {
        let dir = data_dir("corrupt");
        write_state(&dir, "{ this is not json");
        assert!(
            applied_patch_ids(&dir, "0.2.0-rc.2").is_empty(),
            "损坏的记录读不出 id，但绝不能 panic——它在启动路径上"
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
