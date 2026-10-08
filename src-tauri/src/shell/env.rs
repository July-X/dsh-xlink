//! 为 Shell 派生的子进程解析一个可用的 `PATH`。
//!
//! Windows 上的 Tauri 应用作为 GUI 子系统进程（`windows_subsystem =
//! "windows"`）运行，CreateProcess 启动时继承的路径段是 Window Station
//! 的系统路径。npm、pnpm、nvm 等工具在安装时追加到用户级 PATH 的内容
//! 保存在 `HKEY_CURRENT_USER\Environment\Path` 中，由 Explorer 以及
//! 其他交互式宿主程序合并进系统路径——但是从桌面快捷方式、“运行”对话
//! 框或开机自启动启动的 GUI 应用只能看到系统路径。结果是：Shell 能找
//! 到 `node`（它位于 `C:\Program Files\nodejs` 下的系统路径），却找不
//! 到 `pnpm.cmd`（用户的 npm prefix 把它放到 `%AppData%\npm` 下），因
//! 此静默地无法启动内核安装。
//!
//! 本模块在进程启动时读取一次用户 PATH，并暴露一个合并后的 `PATH` 字
//! 符串，所有 `process::spawn` 在子进程继承其它环境变量之前都会把它写
//! 入子进程。 非 Windows 平台为 no-op：`Command::env` 直接传入父进程
//! 已有的那个值。

#[cfg(windows)]
use std::process::Command;
use std::sync::OnceLock;

/// Tauri 继承的 Windows 进程所查询的用户 PATH 环境变量名；与
/// `HKEY_CURRENT_USER\Environment\Path` 以及“用户环境变量编辑器”曝
/// 出的注册表值一致。
const PATH: &str = "PATH";
/// Windows 中保存用户 PATH（及同类项）的注册表路径。
#[cfg(windows)]
const REG_USER_ENV: &str = "HKCU\\Environment";
/// 注册表中用户 PATH 的值名。
#[cfg(windows)]
const REG_PATH_VALUE: &str = "Path";
/// `reg.exe` 位于固定的 Windows 路径下，并始终在系统 PATH 中；将其
/// 固定下来可以排除任何攻击者植入的同名 shim 在 PATH 上响应的可能。
#[cfg(windows)]
const REG_EXE: &str = "C:\\Windows\\System32\\reg.exe";

/// 一次性缓存的合并后 PATH。在首次从 `merged_path` 调用时惰性初始化。
/// `OnceLock` 是 `Sync` 的，且不需要 `unsafe`，因此即使
/// `std::env::set_var` 在这个多线程的 Tauri 运行时下并不安全，它仍
/// 然是正确的原语。
static MERGED: OnceLock<String> = OnceLock::new();

/// 子进程实际使用的 `PATH`：在 Unix 上直接使用进程环境变量，在
/// Windows 上使用缓存好的合并值。以 `&'static str` 返回，以便调用方
/// 直接传给 `Command::env` 而无需克隆。
pub fn merged_path() -> &'static str {
    MERGED.get_or_init(compute_merged_path).as_str()
}

#[cfg(not(windows))]
fn compute_merged_path() -> String {
    // 在 macOS / Linux 上，启动的进程从父 Shell 继承一个可用的 PATH。
    // 不需要合并；原样镜像已设置的值，让 `process::spawn` 把同样的值
    // 写回子进程。
    std::env::var(PATH).unwrap_or_default()
}

#[cfg(windows)]
fn compute_merged_path() -> String {
    let system = std::env::var(PATH).unwrap_or_default();
    match read_user_path() {
        Some(user) if !user.is_empty() => merge_paths(&system, &user),
        // 要么没有注册表项，要么 `reg.exe` 拒绝与我们通信；系统 PATH
        // 总归聊胜于无。
        _ => system,
    }
}

/// 通过 `reg.exe` 从 `HKCU\Environment` 读取用户的 `Path` 值。
/// 任何失败（注册表项缺失、权限被拒、Shell 出错）时返回 `None`；
/// 调用方回退到系统 PATH。
///
/// `reg.exe` 本身是 GUI 子系统二进制，启动时不会弹出控制台窗口；但
/// 在某些 Windows 版本中控制台程序仍可能短暂闪烁，因此这里仍然需要
/// `quiet()`（CREATE_NO_WINDOW），算作有备无患。
#[cfg(windows)]
fn read_user_path() -> Option<String> {
    let mut cmd = Command::new(REG_EXE);
    cmd.args(["query", REG_USER_ENV, "/v", REG_PATH_VALUE]);
    let (success, stdout, _) =
        crate::shell::process::run_command_capture(cmd, "reg query PATH").ok()?;
    if !success {
        return None;
    }
    parse_reg_path(&stdout)
}

/// 从 `reg query` 的标准输出中提取值列：
/// `... Path    REG_SZ    C:\Users\...;C:\Program Files\...`
/// 同时兼容注册表编辑器在值中包含 `%FOO%` 引用时写出的
/// `Path    REG_EXPAND_SZ` 变体；两种格式都在最后一个非空行以
/// `REG_<TYPE>    <value>` 结尾。
///
/// 这里应使用 `split_whitespace`（而不是
/// `splitn(3, char::is_whitespace)`）：`splitn` 会在每一个独立的空白
/// 字符处切分，而 `splitn` 在匹配够 `n-1` 次后即停止，不再继续切
/// 分，结果会把第三个元素和 `REG_SZ` 粘在一起。
#[cfg(windows)]
fn parse_reg_path(out: &str) -> Option<String> {
    let last = out.lines().map(str::trim).rev().find(|l| !l.is_empty())?;
    let mut parts = last.split_whitespace();
    let _name = parts.next()?;
    let _ty = parts.next()?;
    let value = parts.collect::<Vec<_>>().join(" ");
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

/// 拼接两个 `PATH` 字符串，保留顺序，对条目去重（Windows 文件系统不
/// 区分大小写，因此比较也不区分大小写），并跳过空字段。
#[cfg(windows)]
fn merge_paths(system: &str, user: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let mut push = |entry: &str| {
        let trimmed = entry.trim();
        if trimmed.is_empty() {
            return;
        }
        let key = trimmed.to_ascii_lowercase();
        if seen.iter().any(|s| s == &key) {
            return;
        }
        seen.push(key);
        out.push(trimmed.to_string());
    };
    // 用户 PATH 优先——用户显式放置的路径胜过来自系统继承的任何条目，
    // 这与 Explorer 拼接两者以及 `cmd.exe` 解析裸名时的行为一致。
    for entry in user.split(';') {
        push(entry);
    }
    for entry in system.split(';') {
        push(entry);
    }
    out.join(";")
}

// 名字带 `windows_`：文件末尾还有一个跨平台的 `mod tests`（`parse_scutil_proxy`
// 那一族），同名会撞成 `error[E0428]: the name tests is defined multiple times`。
// **只在 `cargo test` / `cargo clippy --all-targets` 下编译**，所以平时
// `cargo build` 都发现不了——发布 CI 的 quality job 跑 `cargo test` 才会现形。
#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    #[test]
    fn parse_reg_path_extracts_value() {
        let sample = "\
\r\nHKEY_CURRENT_USER\\Environment\r\n    Path    REG_SZ    C:\\Users\\zxx\\AppData\\Roaming\\npm;C:\\Program Files\\nodejs\r\n\r\n";
        assert_eq!(
            parse_reg_path(sample).as_deref(),
            Some("C:\\Users\\zxx\\AppData\\Roaming\\npm;C:\\Program Files\\nodejs"),
        );
    }

    #[test]
    fn parse_reg_path_handles_expand_sz() {
        let sample = "\
HKEY_CURRENT_USER\\Environment
    Path    REG_EXPAND_SZ    %USERPROFILE%\\bin;C:\\Windows
";
        assert_eq!(
            parse_reg_path(sample).as_deref(),
            Some("%USERPROFILE%\\bin;C:\\Windows"),
        );
    }

    #[test]
    fn parse_reg_path_rejects_empty_value() {
        let sample = "HKEY_CURRENT_USER\\Environment\n    Path    REG_SZ    \n";
        assert_eq!(parse_reg_path(sample), None);
    }

    #[test]
    fn merge_paths_user_wins_and_dedups() {
        let system = "C:\\Windows;C:\\Program Files\\nodejs";
        let user = "C:\\Users\\zxx\\AppData\\Roaming\\npm;c:\\program files\\nodejs";
        let merged = merge_paths(system, user);
        // 用户条目优先；与 `C:\Program Files\nodejs` 大小写不同的重复条目
        // 被合并掉；系统 `C:\Windows` 保留在末尾。
        assert_eq!(
            merged,
            "C:\\Users\\zxx\\AppData\\Roaming\\npm;c:\\program files\\nodejs;C:\\Windows",
        );
    }

    #[test]
    fn merge_paths_skips_empty_segments() {
        let merged = merge_paths(";;C:\\Windows;;", ";;C:\\Users\\bin;;");
        assert_eq!(merged, "C:\\Users\\bin;C:\\Windows");
    }
}

// ─── 系统网络代理 ────────────────────────────────────────────────────────

/// 操作系统层面配置的代理（macOS「系统设置 → 网络 → 代理」、Windows
/// Internet Settings）。只用于**注入子进程环境变量**。
#[derive(Default, Debug, PartialEq, Eq)]
pub(crate) struct SystemProxy {
    pub(crate) http: Option<String>,
    pub(crate) https: Option<String>,
    /// SOCKS 走 `socks5h://`（h = 让代理端解析域名），否则本地 DNS 仍要
    /// 直连，域名解析慢的那一段照样慢。
    pub(crate) socks: Option<String>,
}

/// 把 `scutil --proxy` 的输出解析成代理设置。**纯函数**：macOS 上那段输出
/// 的形状由系统决定，但内容是文本，解析就该能脱离 `scutil` 单测。
///
/// 认不出来就返回空——**宁可不注入，也不要注入一个错的**。错的代理地址
/// 比没有代理更糟：git 会对着一个不存在的端口重试，最后报一个比「直连慢」
/// 更难懂的错误。
///
/// PAC（`ProxyAutoConfigURLString`）刻意不解析：它是一段需要求值的 JS，
/// git 的 `http_proxy` 表达不了。忽略了它，用户至少还是直连。
fn parse_scutil_proxy(text: &str) -> SystemProxy {
    fn field(text: &str, key: &str) -> Option<String> {
        let line = format!("{key} : ");
        let mut lines = text.lines().filter_map(|l| l.trim().strip_prefix(&line));
        let value = lines.next()?.trim();
        // 值可能是嵌套字典（如带例外列表的 HTTPProxy），那不是我们要的形状。
        if value.is_empty() || value.starts_with('<') {
            return None;
        }
        Some(value.to_string())
    }
    fn enabled(text: &str, key: &str) -> bool {
        field(text, key).is_some_and(|v| v == "1")
    }
    fn url_of(text: &str, host_key: &str, port_key: &str, scheme: &str) -> Option<String> {
        let host = field(text, host_key)?;
        let port = field(text, port_key)?;
        Some(format!("{scheme}://{host}:{port}"))
    }

    SystemProxy {
        http: enabled(text, "HTTPEnable")
            .then(|| url_of(text, "HTTPProxy", "HTTPPort", "http"))
            .flatten(),
        https: enabled(text, "HTTPSEnable")
            .then(|| url_of(text, "HTTPSProxy", "HTTPSPort", "http"))
            .flatten(),
        socks: enabled(text, "SOCKSEnable")
            .then(|| url_of(text, "SOCKSProxy", "SOCKSPort", "socks5h"))
            .flatten(),
    }
}

/// 读操作系统配置的代理。读不到（没有开代理、命令不存在、权限不足）返回空。
#[cfg(target_os = "macos")]
fn read_system_proxy() -> SystemProxy {
    let Ok(output) = std::process::Command::new("/usr/sbin/scutil")
        .arg("--proxy")
        .output()
    else {
        return SystemProxy::default();
    };
    parse_scutil_proxy(&String::from_utf8_lossy(&output.stdout))
}

/// Windows：Internet Settings 的 `ProxyEnable` + `ProxyServer`。
/// `ProxyServer` 允许写成 `host:port` 或 `http=…;https=…;socks=…`，后者
/// 按协议拆开。
#[cfg(windows)]
fn read_system_proxy() -> SystemProxy {
    fn query(name: &str) -> Option<String> {
        let output = std::process::Command::new("reg")
            .args([
                "query",
                r"HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Internet Settings",
                "/v",
                name,
            ])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&output.stdout);
        // `reg query` 的输出是定宽表格，最后一列才是值。
        text.lines()
            .filter_map(|line| line.split_whitespace().nth(3))
            .find(|value| *value != "REG_DWORD_BIG_ENDIAN")
            .map(|value| value.to_string())
    }
    if query("ProxyEnable").as_deref() != Some("0x1") {
        return SystemProxy::default();
    }
    let Some(server) = query("ProxyServer") else {
        return SystemProxy::default();
    };
    if server.contains('=') {
        let mut proxy = SystemProxy::default();
        for part in server.split(';') {
            let Some((scheme, rest)) = part.split_once('=') else {
                continue;
            };
            let value = format!("http://{rest}");
            match scheme.trim() {
                "http" => proxy.http = Some(value),
                "https" => proxy.https = Some(value),
                "socks" => proxy.socks = Some(format!("socks5h://{rest}")),
                _ => {}
            }
        }
        proxy
    } else {
        SystemProxy {
            http: Some(format!("http://{server}")),
            https: Some(format!("http://{server}")),
            socks: None,
        }
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
fn read_system_proxy() -> SystemProxy {
    SystemProxy::default()
}

/// 读一次并缓存：系统代理在一次运行里不会变，而 `scutil --proxy` 要起子进程。
fn system_proxy() -> &'static SystemProxy {
    static CACHE: OnceLock<SystemProxy> = OnceLock::new();
    CACHE.get_or_init(read_system_proxy)
}

/// 注入给 git 子进程的代理变量，**只包含当前环境里还没有的那些**。
///
/// **优先级：已有环境变量 > 系统设置。** 用户在 `.zshrc` 里显式写的
/// `https_proxy`（可能指向公司内网、或指向另一个工具）不该被系统设置盖掉；
/// git 自己也会优先读环境变量，我们若强行覆盖等于替他改主意。
///
/// 一个都没探测到就返回空——**没有代理就走直连**，这本来就是 git 的默认行为。
pub fn proxy_env_for_children() -> Vec<(&'static str, String)> {
    let proxy = system_proxy();
    let mut out = Vec::new();
    for (name, value) in [
        ("http_proxy", proxy.http.clone()),
        ("https_proxy", proxy.https.clone()),
        ("all_proxy", proxy.socks.clone()),
    ] {
        let Some(value) = value else { continue };
        if std::env::var_os(name).is_none() && std::env::var_os(name.to_uppercase()).is_none() {
            out.push((name, value));
        }
    }
    out
}

/// 把探测到的代理写进一个子进程的命令。空结果时不碰 `PATH` 之外的东西。
pub(crate) fn apply_proxy_env(cmd: &mut std::process::Command) {
    for (name, value) in proxy_env_for_children() {
        cmd.env(name, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_typical_scutil_block() {
        let text = "<dictionary> {\n  HTTPEnable : 1\n  HTTPPort : 7897\n  HTTPProxy : 127.0.0.1\n  HTTPSEnable : 1\n  HTTPSPort : 7897\n  HTTPSProxy : 127.0.0.1\n  SOCKSEnable : 0\n}\n";
        let proxy = parse_scutil_proxy(text);
        assert_eq!(proxy.http.as_deref(), Some("http://127.0.0.1:7897"));
        assert_eq!(proxy.https.as_deref(), Some("http://127.0.0.1:7897"));
        assert_eq!(proxy.socks, None);
    }

    #[test]
    fn disabled_entries_are_not_picked_up() {
        // 开着开关但没填地址，或填了地址但开关是 0，都不算配了代理。
        let off = parse_scutil_proxy(
            "<dictionary> {\n  HTTPEnable : 0\n  HTTPPort : 7897\n  HTTPProxy : 127.0.0.1\n}\n",
        );
        assert_eq!(off.http, None);
        let dangling =
            parse_scutil_proxy("<dictionary> {\n  HTTPSEnable : 1\n  HTTPSPort : 7897\n}\n");
        assert_eq!(dangling.https, None);
        assert_eq!(
            parse_scutil_proxy("<dictionary> {\n}\n"),
            SystemProxy::default()
        );
    }

    #[test]
    fn a_nested_dictionary_value_is_not_a_host() {
        // PAC / 例外列表会让某个键的值是嵌套字典，那不是地址。
        let text = "<dictionary> {\n  HTTPEnable : 1\n  HTTPProxy : <dictionary> {\n    ExceptionsList : <array> {\n    }\n    ExcludeSimpleHostnames : 1\n  }\n  HTTPPort : 7897\n}\n";
        assert_eq!(parse_scutil_proxy(text).http, None);
    }

    #[test]
    fn socks_gets_the_socks5h_scheme() {
        let text =
            "<dictionary> {\n  SOCKSEnable : 1\n  SOCKSPort : 1080\n  SOCKSProxy : 10.0.0.1\n}\n";
        assert_eq!(
            parse_scutil_proxy(text).socks.as_deref(),
            Some("socks5h://10.0.0.1:1080")
        );
    }
}
