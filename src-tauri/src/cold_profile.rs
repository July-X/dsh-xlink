//! **冷档案**：给取数窗一份「**登录态在、站点缓存不在**」的**一次性**浏览器档案副本。
//!
//! # 为什么需要它（**根因**）
//!
//! DeepSeek 的单会话正文端点 `GET /api/v0/chat/history_messages` 是**增量
//! （MERGE）协议**：站点先问客户端「你缓存到第几版了」（`cache_version`），
//! 再**只回增量**。⇒ ⇒ 取数窗复用**用户自己那份档案**时，客户端缓存已经是
//! 最新的 ⇒ ⇒ 站点回**空增量**（`chat_messages` 长度 0）⇒ ⇒ 壳侧裁出 0 条
//! ⇒ ⇒ 界面落 `catalog_empty` 档（「这条会话没有内容」）。
//!
//! 【已实测】同一棵源档案的四臂对照（2026-10-22 11:1x，DeepSeek 首站）：
//!
//! | 臂 | `Default\IndexedDB` | 请求 query | `cache_control` | 正文条数 |
//! | --- | --- | --- | --- | --- |
//! | 带缓存 | 在 | 带 `cache_version` | `MERGE` | 增量（22 条） |
//! | **冷** | **删掉** | **只有 `chat_session_id`** | **`REPLACE`** | **全量（32 条）** |
//!
//! ⇒ ⇒ **「冷」＝ 客户端那份增量缓存不在** ⇒ ⇒ **服务端才会回全量**。
//!
//! # 形态：拷贝一份**白名单子集**，不是整棵
//!
//! · **只拷**：`Local State`／`Preferences`／`Secure Preferences`／
//!   `Default\Local Storage`／`Default\Network`（Cookies）。
//! · **不拷**：`Default\IndexedDB` —— **那棵就是「冷」的定义**（站点自己那份
//!   `history-message` 增量缓存）。
//! · **源档案只读**：只从源读、绝不写、绝不删（协议 §7.3 只读）。
//! · **副本是一次性的**：用完整棵删掉（凭据不留在盘上，协议 §7.2）。
//!
//! ⚠️ **为什么白名单必须带上 `Local Storage`** —— 【已实测】站点把登录态放在
//! **Local Storage**：删掉它 ⇒ 立刻跳微信扫码登录页；删掉 `IndexedDB`
//! ⇒ 仍然登录 ⇒ ⇒ 登录态在 `Local Storage`、增量缓存在 `IndexedDB`，**两者
//! 分开落** ⇒ ⇒ 这就是「能只摘掉缓存、不摘掉登录态」的原因。
//!
//! ⚠️ **Windows 上 Cookies 的可搬性** —— Cookies 由 DPAPI 绑机器 ＋ 用户、
//! **不绑路径**（macOS 的 Keychain 绑不同，搬不动）⇒ ⇒ Windows 上这份副本
//! 带得上登录态。

// macOS 上这条通路整块不存在（`arm_capture_window` 直接报不支持）⇒ ⇒ 这份
// 档案逻辑在那边用不上 ⇒ ⇒ 免掉那边的 dead_code 噪声。
#![cfg_attr(not(windows), allow(dead_code))]

use std::path::{Path, PathBuf};

/// 副本根目录的**标记**（删的时候只删带这个前缀的目录，绝不误删别的东西）。
const MARK: &str = "dsh-xlink-cold-profile-";

/// 白名单里的**单文件**（相对 `EBWebView`）。
///
/// ⚠️ **`Default\IndexedDB` 不在里面** —— 那就是「冷」的定义。
const WHITELIST_FILES: &[&str] = &[
    "Local State",
    "Default/Preferences",
    "Default/Secure Preferences",
];

/// 白名单里的**整棵目录**（相对 `EBWebView`）。
const WHITELIST_DIRS: &[&str] = &["Default/Local Storage", "Default/Network"];

/// 一份**冷档案**（临时目录 ＋ 自动清理）。
///
/// ## 生命周期
///
/// `dir` 交给 WebView2 的 `.data_directory()` ⇒ ⇒ **窗还开着时这个值必须
/// 活着** ⇒ ⇒ 取数窗**建好、用完、关掉之后**才 drop（`Drop` 里删目录）。
///
/// ## 清理
///
/// `Drop` 删整棵副本目录；进程被强杀留下的残留在**下一次 `prepare`** 时
/// 按标记清掉（只清带本模块标记的目录）。
#[derive(Debug)]
pub struct ColdProfile {
    /// 交给 `.data_directory()` 的**父目录**（WebView2 会在它下面自己建
    /// `EBWebView`）⇒ ⇒ 这份副本的形态与生产档案**完全一致**。
    ///
    /// ⚠️ 这**就是**临时根本身（`Drop` 删的就是它）。
    pub dir: PathBuf,
}

impl ColdProfile {
    /// 备一份冷档案（**源只读**）。
    ///
    /// `source` ＝ **生产档案的父目录**（`<data_dir>/webview-official-chat`
    /// —— 与官网对话窗口那份**同一个**目录，见协议 §2.1 纪律 1）。
    ///
    /// 失败只回**一句原因**（调用方按 §6 的既有码归类）—— **不回凭据、
    /// 不回路径里的私密部分**。
    pub fn prepare(source: &Path) -> Result<Self, String> {
        let source_webview = source.join("EBWebView");
        if !source_webview.is_dir() {
            return Err(format!(
                "官网对话档案还没有浏览器数据目录（{}），先打开一次官网对话页签再试",
                source_webview.display()
            ));
        }
        let root = temp_root()?;
        cleanup_stale(&root);
        let target_webview = root.join("EBWebView");
        let mut copied = 0usize;
        let mut skipped: Vec<String> = Vec::new();

        for rel in WHITELIST_FILES {
            let from = source_webview.join(rel);
            let to = target_webview.join(rel);
            match copy_file(&from, &to) {
                Ok(true) => copied += 1,
                Ok(false) => {}
                Err(error) => skipped.push(format!("{rel}: {error}")),
            }
        }
        for rel in WHITELIST_DIRS {
            let from = source_webview.join(rel);
            let to = target_webview.join(rel);
            match copy_dir(&from, &to) {
                Ok((files, misses)) => {
                    copied += files;
                    skipped.extend(misses);
                }
                Err(error) => skipped.push(format!("{rel}: {error}")),
            }
        }

        if copied == 0 {
            let _ = std::fs::remove_dir_all(&root);
            return Err(format!(
                "冷档案一个文件都没拷到（源 {}）：{}",
                source_webview.display(),
                if skipped.is_empty() {
                    "源目录是空的".to_string()
                } else {
                    skipped.join("；")
                }
            ));
        }

        // **登录态在不在** —— 只能事后由站点说了算（壳不读凭据）⇒ ⇒ 这里
        // 只登记**拷了多少、跳过了什么**（**不含任何凭据内容**）。
        eprintln!(
            "dsh-xlink: 冷档案已备好（拷了 {copied} 个文件，跳过 {} 项）",
            skipped.len()
        );
        for item in skipped.iter().take(8) {
            eprintln!("dsh-xlink: 冷档案跳过：{item}");
        }

        Ok(ColdProfile { dir: root })
    }
}

impl Drop for ColdProfile {
    fn drop(&mut self) {
        // 只删**自己那块**（名字带标记）—— 绝不碰生产档案。
        let name = self
            .dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if !name.starts_with(MARK) {
            return;
        }
        // ⚠️ **WebView2 的浏览器进程退出有延迟** ⇒ ⇒ 关窗之后立刻删，常常撞上
        // 「文件被占用」⇒ ⇒ 有限次重试（**凭据不留在盘上**是硬要求，
        // 协议 §7.2）；实在删不掉就登记，**下一次 `prepare` 会清残留**。
        let mut last = String::new();
        for _ in 0..6 {
            match std::fs::remove_dir_all(&self.dir) {
                Ok(()) => return,
                Err(error) => {
                    last = error.to_string();
                    std::thread::sleep(std::time::Duration::from_millis(80));
                }
            }
        }
        eprintln!("dsh-xlink: 冷档案临时目录没删干净（下次取数时会清）：{last}");
    }
}

/// 一次性的临时根目录（名字带标记 ⇒ 删的时候只删自己那块）。
fn temp_root() -> Result<PathBuf, String> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let root = std::env::temp_dir().join(format!("{MARK}{stamp}-{}", std::process::id()));
    std::fs::create_dir_all(&root).map_err(|e| format!("临时目录建不出来：{e}"))?;
    Ok(root)
}

/// 清掉**上一次**留下的残骸（进程被强杀 ⇒ `Drop` 没跑）。
///
/// **只清带本模块标记、且明显是陈旧的**目录 —— ⚠️ **不许删「还在用」的**
/// （上一份副本的 WebView2 进程可能还没退出；WebView2 正在写它 ⇒ mtime 是
/// 「刚刚」⇒ ⇒ 靠这条躲掉）。
fn cleanup_stale(current: &Path) {
    let temp = std::env::temp_dir();
    let entries = match std::fs::read_dir(&temp) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    let now = std::time::SystemTime::now();
    for entry in entries.flatten() {
        let path = entry.path();
        if path == current || !path.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with(MARK) {
            continue;
        }
        // **陈旧判定**：最后修改时间距今超过 10 分钟 ⇒ ⇒ 那一轮早就结束了。
        let modified = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| now.duration_since(t).ok());
        let stale = modified.map(|d| d.as_secs() > 10 * 60).unwrap_or(false);
        if !stale {
            continue;
        }
        let _ = std::fs::remove_dir_all(&path);
    }
}

/// 拷一个文件（**源只读**）。
///
/// 源被独占时（Windows 上 Cookies 常见）拷不动 ⇒ ⇒ **跳过并记账**，
/// 不让一次文件失败毁掉整份副本。
fn copy_file(from: &Path, to: &Path) -> Result<bool, String> {
    if !from.is_file() {
        return Ok(false);
    }
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("建目录失败：{e}"))?;
    }
    match std::fs::copy(from, to) {
        Ok(_) => Ok(true),
        Err(error) => Err(error.to_string()),
    }
}

/// 拷一整棵目录（**源只读**）⇒ ⇒ 回「拷了几个文件、跳过了几项」。
fn copy_dir(from: &Path, to: &Path) -> Result<(usize, Vec<String>), String> {
    let mut files = 0usize;
    let mut skipped: Vec<String> = Vec::new();
    if !from.is_dir() {
        return Ok((0, skipped));
    }
    std::fs::create_dir_all(to).map_err(|e| format!("建目录失败：{e}"))?;
    let entries = match std::fs::read_dir(from) {
        Ok(entries) => entries,
        Err(error) => return Err(error.to_string()),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let target = to.join(entry.file_name());
        let file_type = match entry.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        if file_type.is_dir() {
            match copy_dir(&path, &target) {
                Ok((sub_files, sub_skipped)) => {
                    files += sub_files;
                    skipped.extend(sub_skipped);
                }
                Err(error) => skipped.push(format!("{}: {error}", path.display())),
            }
        } else if file_type.is_file() {
            match std::fs::copy(&path, &target) {
                Ok(_) => files += 1,
                Err(error) => skipped.push(format!("{}: {error}", path.display())),
            }
        }
    }
    Ok((files, skipped))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一份**假源档案**（形态与生产那份一致：`<root>/EBWebView/...`）。
    ///
    /// ⚠️ **只碰临时目录** —— 测试永远不许碰用户的真实数据目录（仓库纪律）。
    fn fake_source(tag: &str) -> PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let root = std::env::temp_dir().join(format!("dsh-xlink-cold-test-{tag}-{stamp}"));
        let webview = root.join("EBWebView");
        // 登录态那一份（`Local Storage`）＋ 站点增量缓存那一份（`IndexedDB`）。
        for rel in [
            "Local State",
            "Default/Preferences",
            "Default/Secure Preferences",
            "Default/Local Storage/leveldb/000001.log",
            "Default/Network/Cookies",
            "Default/IndexedDB/https_chat.deepseek.com_0.indexeddb.leveldb",
        ] {
            let path = webview.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, b"x").unwrap();
        }
        root
    }

    #[test]
    fn 白名单里没有_indexeddb() {
        // 「冷」的定义就是这一条：站点那份增量缓存**不许**进副本。
        assert!(!WHITELIST_FILES.iter().any(|p| p.contains("IndexedDB")));
        assert!(!WHITELIST_DIRS.iter().any(|p| p.contains("IndexedDB")));
    }

    #[test]
    fn 副本里有登录态那几样_没有站点那份缓存() {
        let source = fake_source("ok");
        let cold = ColdProfile::prepare(&source).expect("冷档案应当备得出来");
        let webview = cold.dir.join("EBWebView");
        // 登录态（`Local Storage`）与 Cookies（`Network`）**在**。
        assert!(webview
            .join("Default/Local Storage/leveldb/000001.log")
            .is_file());
        assert!(webview.join("Default/Network/Cookies").is_file());
        assert!(webview.join("Local State").is_file());
        // 站点那份增量缓存（`IndexedDB`）**不在** ⇒ ⇒ 站点会回全量而不是增量。
        assert!(!webview.join("Default/IndexedDB").exists());
        // **源一个字节都没动**（§7.3 只读）。
        assert!(source
            .join("EBWebView/Default/IndexedDB/https_chat.deepseek.com_0.indexeddb.leveldb")
            .is_file());
        let _ = std::fs::remove_dir_all(&source);
    }

    #[test]
    fn 源还没有浏览器数据目录时报错而不是崩() {
        let missing = std::env::temp_dir().join("dsh-xlink-cold-test-does-not-exist-20261022");
        let error = ColdProfile::prepare(&missing).expect_err("缺源目录必须报错");
        assert!(error.contains("还没有浏览器数据目录"), "实际错误：{error}");
    }
}
