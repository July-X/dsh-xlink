//! 官网对话**读取上下文**：官网窗口没打开时，工作台里 `@` 选中官网对话
//! 仍要能读到内容。
//!
//! # 为什么需要它
//!
//! 官网对话的内容 webview 是**官网窗口的子 webview**
//! （`commands.rs` 的 `add_official_chat_tab`）。官网窗口没开时，
//! `ensure_official_chat_tab` 直接返回「官网网页版窗口未打开」——读取链路
//! 因此断掉。本模块负责在这种情况下**按需**建立一条读取上下文。
//!
//! # 三条硬纪律
//!
//! 1. **必须复用同一份 user-data 目录**。Windows 上是
//!    `<data_dir>/webview-official-chat`（cookies / localStorage 都在里面），
//!    macOS 上是 `harness::official_chat::OFFICIAL_CHAT_DATA_STORE_IDENTIFIER`
//!    对应的 WKWebsiteDataStore。换了目录就拿不到登录态——所以这里的
//!    `profile_dir` **只能**从统一的取目录函数来，调用方不得自造路径。
//!    WebView2 还要求同一 user-data 目录下的所有环境**选项完全一致**，所以
//!    自建的读取 webview 必须传**同一套** `additional_browser_args`。
//! 2. **不抢焦点**。自建的读取 webview 放在**不可见**窗口里，既不
//!    `set_focus` 也不 `show`。
//! 3. **不启动所有站点作长期预热**，只按需创建**用到的那个**站点；用户
//!    正在使用的页签**不得**被销毁。
//!
//! # 归属
//!
//! - 官网窗口开着、内容页签已挂载 → **复用**那个页签（不新建 webview）。
//! - 官网窗口关着 → 自建一个隐藏读取 webview（`owned = true`），闲置后可回收。
//! - 官网窗口之后又开了 → 读取上下文**移交**给已挂载页签，自建的隐藏
//!   webview 回收（不碰用户的页签）。
//!
//! # 账号切换
//!
//! 上下文记一个**账号指纹**（由页内算出的、**不含凭据**的摘要）。指纹变了
//! 就说明换了账号：旧的在途请求结果与目录缓存一律作废，不许把上一账号的
//! 候选套到新账号。

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::Value;
use tauri::Manager;

use super::script_eval::{read_json, RoundTripError};

/// 读取上下文的来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContextSource {
    /// 复用官网窗口里已挂载的内容页签。
    MountedTab,
    /// 官网窗口没开，按需自建的隐藏读取 webview。
    LazyHidden,
}

impl ContextSource {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            ContextSource::MountedTab => "mounted-tab",
            ContextSource::LazyHidden => "lazy-hidden",
        }
    }
}

/// 一次读取上下文。
pub(crate) struct ReadingContext {
    /// 站点 key（`deepseek` / `chatgpt` / `minimax`）。
    pub(crate) site: &'static str,
    /// 实际使用的 user-data 目录。**必须**与官网对话窗口那份一致。
    pub(crate) profile_dir: PathBuf,
    /// 上下文来源。
    pub(crate) source: ContextSource,
    /// 承载读取的窗口（已挂载页签，或自建的隐藏读取窗口）。
    pub(crate) window: tauri::WebviewWindow,
    /// 是否是**我们自己**建的窗口。只有自建的才允许回收——用户挂载的页签
    /// 绝不能被销毁。
    pub(crate) owned: bool,
    /// 建立时刻，用于闲置回收。
    pub(crate) created_at: Instant,
    /// 最近一次使用时刻。
    pub(crate) last_used: Instant,
    /// 账号指纹（不含凭据）。
    pub(crate) account: Option<String>,
    /// 实际导航落点（origin）。
    pub(crate) landed_origin: Option<String>,
}

/// 建立一个**不可见**的官网对话读取窗口。
///
/// **为什么是顶格自由函数**：仓库判据 `check-invariants.mjs` 的「远程建窗链
/// 认族」按**顶格 `fn `** 切链（窗口身份词常在 `WebviewUrl::External(` 的
/// 上一行 `format!` 里，起点必须退到函数签名才认得出族）。放在 `impl` 的
/// 方法里会让它落在切片之外，判据会把这条链报成「认不出属于哪一族」。
///
/// 建窗接线按官方对话族：注入 `no-context-menu.js`（别人的站点不许自绘右键
/// 菜单），标题走 `window_title()`，主题走 `appearance::initial_theme`。
fn build_read_window(
    app: &tauri::AppHandle,
    site: &str,
    url: tauri::Url,
    profile_dir: &Path,
    browser_args: &str,
) -> Result<tauri::WebviewWindow, String> {
    let label = format!("official-chat-read-{site}");
    tauri::WebviewWindowBuilder::new(app, &label, tauri::WebviewUrl::External(url))
        .title(crate::shell::window::window_title("官网对话读取上下文"))
        .inner_size(800.0, 600.0)
        .visible(false)
        .theme(crate::shell::appearance::initial_theme(&label))
        .data_directory(profile_dir.to_path_buf())
        .additional_browser_args(browser_args)
        .initialization_script(include_str!("../harness/no-context-menu.js"))
        .initialization_script(include_str!("../harness/chat-fingerprint.js"))
        .build()
        .map_err(|e| format!("无法建立读取上下文：{e}"))
}

impl ReadingContext {
    /// 取得（或按需建立）某个站点的读取上下文。
    ///
    /// `mounted_label` 给出官网对话内容页签的 label（`official-chat-tab-{i}`）；
    /// 存在就复用，否则按需建一个**不可见**的读取窗口。
    ///
    /// 建窗接线按仓库判据的口径：窗口属于**官方对话族**（label 带
    /// `official-chat`），所以注入 `no-context-menu.js`（别人的站点不许自绘
    /// 右键菜单）；标题走 `window_title()`，主题走 `appearance::initial_theme`。
    pub fn resolve(
        app: &tauri::AppHandle,
        site: &'static str,
        site_url: &str,
        profile_dir: &Path,
        mounted_label: &str,
        browser_args: &str,
    ) -> Result<Self, String> {
        let profile_dir = profile_dir.to_path_buf();

        // 1) 官网窗口开着、内容页签已挂载 → 复用。
        if let Some(window) = app.get_webview_window(mounted_label) {
            return Ok(Self {
                site,
                profile_dir,
                source: ContextSource::MountedTab,
                window,
                owned: false,
                created_at: Instant::now(),
                last_used: Instant::now(),
                account: None,
                landed_origin: None,
            });
        }

        // 2) 官网窗口没开 → 按需建一个**不可见**的读取窗口。
        //    不可见 + 从不 set_focus ⇒ 不抢焦点。
        let url: tauri::Url = site_url
            .parse()
            .map_err(|e| format!("读取上下文的站点地址非法：{e}"))?;
        std::fs::create_dir_all(&profile_dir)
            .map_err(|e| format!("无法创建官网对话数据目录：{e}"))?;
        let window = build_read_window(app, site, url, &profile_dir, browser_args)?;

        Ok(Self {
            site,
            profile_dir,
            source: ContextSource::LazyHidden,
            window,
            owned: true,
            created_at: Instant::now(),
            last_used: Instant::now(),
            account: None,
            landed_origin: None,
        })
    }

    /// 同步读一个表达式的值。
    pub(crate) async fn read(
        &mut self,
        expression: &str,
        timeout: Duration,
    ) -> Result<Value, RoundTripError> {
        self.last_used = Instant::now();
        read_json(&self.window, expression, timeout).await
    }

    /// 读**实际导航落点**（origin + path，不含 query）。
    ///
    /// 站点可能把入口域名跳到另一个域名（实测 `agent.minimaxi.com` →
    /// `agent.minimax.cn`），所以「有效 origin」只能测、不能假设。
    pub(crate) async fn probe_landing(
        &mut self,
        timeout: Duration,
    ) -> Result<(String, String), RoundTripError> {
        let raw = self
            .read(
                "JSON.stringify({ origin: String(location.origin), path: String(location.pathname) })",
                timeout,
            )
            .await?;
        let text =
            raw.as_str()
                .map(|s| s.to_string())
                .ok_or_else(|| RoundTripError::Transport {
                    detail: "页面没有返回导航落点".to_string(),
                })?;
        let parsed: Value = serde_json::from_str(&text).map_err(|e| RoundTripError::Transport {
            detail: format!("导航落点不是合法 JSON：{e}"),
        })?;
        let origin = parsed["origin"].as_str().unwrap_or("").to_string();
        let path = parsed["path"].as_str().unwrap_or("").to_string();
        self.landed_origin = Some(origin.clone());
        Ok((origin, path))
    }

    /// 算**账号指纹**（不含凭据）。
    ///
    /// 页内只回**键名与长度**，宿主再做摘要——值不离开页面，摘要也不可逆推出
    /// 凭据。账号相关的键各站不同，所以按「键名 + 长度」取集合。
    pub(crate) async fn account_fingerprint(
        &mut self,
        timeout: Duration,
    ) -> Result<String, RoundTripError> {
        let raw = self
            .read(
                r#"JSON.stringify((function () {
  var keys = Object.keys(localStorage);
  var picked = {};
  var re = /user|account|session|token|jwt|profile|uid/i;
  for (var i = 0; i < keys.length; i++) {
    if (!re.test(keys[i])) continue;
    var v = null;
    try { v = localStorage.getItem(keys[i]); } catch (e) {}
    picked[keys[i]] = v === null ? -1 : String(v).length;
  }
  return picked;
})())"#,
                timeout,
            )
            .await?;
        let text = raw.as_str().ok_or_else(|| RoundTripError::Transport {
            detail: "页面没有返回账号指纹素材".to_string(),
        })?;
        // 摘要：只看长度与键名，值不参与。
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for b in text.as_bytes() {
            hash ^= *b as u64;
            hash = hash.wrapping_mul(0x100_0000_01b3);
        }
        Ok(format!("{hash:016x}"))
    }

    /// 账号是否变了。变了就把旧上下文标记为失效。
    ///
    /// 返回 `true` 表示**已切换**，调用方必须丢掉旧的在途结果与目录缓存。
    ///
    /// **第一次调用是建立基线**：那时 `account` 还是 `None`，返回 `false`
    /// （不算切换），但基线必须**记下来**——否则第二次比较永远拿不到
    /// 「上一个账号是谁」。
    pub(crate) fn note_account(&mut self, fingerprint: &str) -> bool {
        let changed = match &self.account {
            Some(prev) => prev != fingerprint,
            None => false,
        };
        // 无论是否变化都更新为最新指纹：第一次调用是在建立基线。
        self.account = Some(fingerprint.to_string());
        changed
    }

    /// 当前账号指纹（从未算过则为 `None`）。
    pub(crate) fn account(&self) -> Option<&str> {
        self.account.as_deref()
    }

    /// 闲置够久了没有？**只对自建的读取窗口**回答 `true`**。
    ///
    /// 用户挂载的页签由用户自己管，壳不该替它回收。
    pub(crate) fn is_idle(&self, idle: Duration) -> bool {
        self.owned && self.last_used.elapsed() >= idle
    }

    /// 回收自建的读取窗口。返回是否真的回收了。
    ///
    /// 绝不碰用户挂载的页签。
    pub(crate) fn release(&mut self) -> bool {
        if !self.owned {
            return false;
        }
        let _ = self.window.close();
        true
    }

    /// 上下文是否仍然有效（窗口还在、没被用户销毁）。
    pub(crate) fn is_alive(&self) -> bool {
        self.window.is_visible().is_ok() || self.window.url().is_ok()
    }
}
