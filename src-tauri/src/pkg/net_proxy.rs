//! 访问 GitHub（检查更新 / 下载更新）时该走哪条路。
//!
//! **为什么不能把这件事交给 HTTP 客户端自己判断**：tauri-plugin-updater 用的
//! reqwest 只认 `HTTP_PROXY` / `HTTPS_PROXY` / `ALL_PROXY` 环境变量。读 Windows
//! 注册表与 macOS 系统网络设置的那条路（hyper-util 的 `client-proxy-system`
//! 特性）在本项目没有开启——它会带进 `windows-registry` 与 `system-configuration`
//! 两个 crate，而它们不在依赖树里（也没有理由为此引入）。而壳是 GUI 程序、从
//! 资源管理器启动，继承不到用户为命令行特意设的那些变量。于是"系统里明明开着
//! 代理，更新检查却直连 GitHub 然后超时"（2026-09-30 用户实测，报错停在
//! `error sending request for url` 这一层）。
//!
//! 顺序是**先代理、失败再直连**：代理是用户显式配的那条路，它此刻自己不在
//! 运行时（软件没开 / 端口被占 / PAC 指向拿不到的内网）是常事，直连仍能救回
//! 一次检查。直连是**兜底**，不是与代理并列的第二个选项。
//!
//! **代理地址只有一个来源：当下读到的系统配置**（环境变量 → Windows 注册表
//! Internet Settings / macOS `scutil`）。**不落盘、不记忆、不复用上一次成功过
//! 的地址**——2026-10-20 实测把这条纪律补上：记忆代理（`last-proxy.txt`）是
//! 一份**快照**，它带来的问题比它解决的问题多：
//!
//!   ① **快照会过期**：本地代理客户端（Clash / Verge / v2rayN / Abysswalker）
//!      换端口、随机端口、重启后端口都变。过期地址要么让每次请求先白付一次
//!      必然失败的连接（本机实测：更新检查连着 3 次先去撞一个已经不存在的端口
//!      才回退直连），要么——更糟——**那个端口已经被别的进程占用了**，于是请求
//!      被送进一个用户根本没打算让它走的服务。
//!   ② **它盖过用户自己的开关**：`NO_PROXY=*`（"什么都别走代理"）与系统代理
//!      开关被关掉（"用户刚把代理关了"）在旧实现里都只是"探测不到"，于是记忆
//!      地址被插回路由表——**用户明确说不要走代理，请求还是走了代理**。
//!   ③ **它和这次请求无关**：记住地址的是 OpenAI / Codex 那条路（它证明的是
//!      "这个代理通 chatgpt.com"），复用的却是 GitHub 更新检查那条路。
//!
//! 原设计要解决的是"探测间歇失灵"（2026-10-09 记录：注册表开关抖动）。实测
//! **抖动期间代理客户端本身就在重启，端口根本不在线**——那正是记忆地址**帮不上
//! 忙**的时刻：它只多给一次注定失败的尝试。（2026-10-20 在本机连续 40 次采样
//! 间隔 250ms，`ProxyEnable` 40/40 稳定为 1，读失败是罕见事件。）真正要救
//! "直连被网关按出口拦（Cloudflare 403 挑战页）"的，是 `transport.rs` 里
//! "403 + HTML 挑战页视为路由依赖失败、换下一条路"那条规则，与记忆地址无关。
//!
//! 这一层只管「走哪条路」。壳里另外两条出网路径不归它管，也不要往这里凑：
//! WebView2 / WKWebView 本身就跟随系统代理，pnpm / npm 读的是 npm 自己的
//! proxy 配置。

use url::Url;

/// 一次请求要走的一条路。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// 走本机系统代理。`source` 记下它是从哪儿读到的，进事件日志与错误文案。
    Proxy { url: Url, source: &'static str },
    /// 直连：连环境变量里的代理也一并绕开。
    Direct,
}

impl Route {
    /// 人话描述，进事件日志与错误文案。
    pub fn describe(&self) -> String {
        match self {
            Route::Proxy { url, source } => format!("代理 {url}（{source}）"),
            Route::Direct => "直连（不走任何代理）".to_string(),
        }
    }
}

/// 按「先代理、失败再直连」给出本次要试的顺序。
///
/// 恒非空，末位恒为直连——回退的终点是「一定试过一次直连」。
///
/// **只认当下探测到的地址**：不落盘、不记忆（理由见文件头）。探测不到就是
/// 「此刻没有可用的代理」，直连——不是「翻出上次成功过的地址接着试」。
pub fn routes() -> Vec<Route> {
    routes_from(detect())
}

/// 纯函数部分：把「探测结果」变成「要试的顺序」。与探测分开是为了能直接测
/// 顺序本身，而不必在一台真的开着代理的机器上验证。
fn routes_from(detected: Option<(Url, &'static str)>) -> Vec<Route> {
    match detected {
        Some((url, source)) => vec![Route::Proxy { url, source }, Route::Direct],
        None => vec![Route::Direct],
    }
}

/// 探测本机系统代理：地址，以及它是从哪儿读到的。探测不到返回 `None`
/// （= 该直连），**不返回错误**：读不到注册表、`scutil` 不存在、地址写得
/// 不合法——这些都只是"没有可用的代理"，不该变成用户看得到的一条错误。
fn detect() -> Option<(Url, &'static str)> {
    if no_proxy_wildcard() {
        return None;
    }
    from_env().or_else(from_system)
}

/// 已退休的「记忆代理」文件（xlink home 下，一行 URL）。
///
/// 旧机制把上次成功过的代理地址落在这里；现在代理地址只从**当下**读到的系统
/// 配置来，这份快照没人读了。留着它有两个坏处：① 用户翻数据目录时会以为壳
/// "写死"了代理地址；② 里面存的还是那个改过端口的旧地址，看着像配置。
const RETIRED_PROXY_MEMORY_FILE: &str = "last-proxy.txt";

/// 清掉已退休的「记忆代理」文件。幂等：文件不在就算成功。
///
/// 失败只留 stderr 并记进事件日志——它是**残留**，不是错误，绝不能因此挡住
/// 启动。
pub(crate) fn retire_remembered_proxy_file() {
    let path = crate::shell::paths::xlink_home().join(RETIRED_PROXY_MEMORY_FILE);
    if !path.is_file() {
        return;
    }
    match std::fs::remove_file(&path) {
        Ok(()) => crate::shell::shell_events::record(
            "net-proxy",
            "已清理已退休的「记忆代理」文件：代理地址现在只从当下读到的系统配置来",
        ),
        Err(error) => eprintln!(
            "dsh-xlink: 清理已退休的记忆代理文件失败（{}）：{error}",
            path.display()
        ),
    }
}

/// 环境变量优先于系统设置：用户为绕开某个站点专门设的变量，比"系统里开着
/// 代理"更具体。`HTTPS_` 排在 `HTTP_` 前面——本项目访问的是 https。
fn from_env() -> Option<(Url, &'static str)> {
    const NAMES: [(&str, &str); 6] = [
        ("HTTPS_PROXY", "环境变量 HTTPS_PROXY"),
        ("https_proxy", "环境变量 https_proxy"),
        ("ALL_PROXY", "环境变量 ALL_PROXY"),
        ("all_proxy", "环境变量 all_proxy"),
        ("HTTP_PROXY", "环境变量 HTTP_PROXY"),
        ("http_proxy", "环境变量 http_proxy"),
    ];
    for (name, source) in NAMES {
        if let Some(url) = std::env::var(name).ok().as_deref().and_then(normalize) {
            return Some((url, source));
        }
    }
    None
}

/// `NO_PROXY=*` 表示"什么都别走代理"。
///
/// **只认这一种通配写法**：完整实现要匹配主机名、端口与 `<local>` 之类各方言，
/// 而壳只访问 GitHub 的两处域名，认不出方言反而会给出错误答案。读到别的写法
/// 时按"没有绕过项"处理，与客户端自身的做法一致。
fn no_proxy_wildcard() -> bool {
    ["NO_PROXY", "no_proxy"].iter().any(|name| {
        std::env::var(name).is_ok_and(|value| value.split(',').any(|item| item.trim() == "*"))
    })
}

/// `127.0.0.1:7890` → `http://127.0.0.1:7890`；已带 http/https 方案的照留。
///
/// 解析不出来（空串、`socks5://`、纯垃圾）返回 `None`：一条坏地址不该把更新
/// 检查带进一条必然失败的路由，让它顺延给直连即可。
fn normalize(value: &str) -> Option<Url> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let candidate = if value.contains("://") {
        value.to_string()
    } else {
        format!("http://{value}")
    };
    let url = Url::parse(&candidate).ok()?;
    (url.scheme() == "http" || url.scheme() == "https").then_some(url)
}

// --- Windows：注册表 ----------------------------------------------------------

/// Internet Settings 下用户能通过「设置 → 网络和 Internet → 代理」改的那一处。
#[cfg(windows)]
const INTERNET_SETTINGS_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Internet Settings";

/// 先 HKCU 后 HKLM：HKCU 是设置界面写的地方，而机器级代理常被装在 HKLM；
/// 两处都有时以用户那一份为准（用户能自己关掉它）。
#[cfg(windows)]
fn from_system() -> Option<(Url, &'static str)> {
    const SOURCE: &str = "Windows 系统代理设置";
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};

    windows_registry_proxy(HKEY_CURRENT_USER)
        .or_else(|| windows_registry_proxy(HKEY_LOCAL_MACHINE))
        .and_then(|(enabled, server)| {
            proxy_from_windows_settings(enabled, &server).map(|url| (url, SOURCE))
        })
}

#[cfg(windows)]
fn windows_registry_proxy(
    hive: windows_sys::Win32::System::Registry::HKEY,
) -> Option<(bool, String)> {
    use windows_sys::Win32::System::Registry::{RegCloseKey, RegOpenKeyExW, HKEY, KEY_READ};

    /// UTF-16 + NUL 结尾，供 Win32 的 `…W` 系列接口使用。
    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    let subkey = wide(INTERNET_SETTINGS_KEY);
    let mut key: HKEY = std::ptr::null_mut();
    let status = unsafe { RegOpenKeyExW(hive, subkey.as_ptr(), 0, KEY_READ, &mut key) };
    if status != 0 || key.is_null() {
        return None;
    }
    // 两个值必须**同一个 hive 里**成对读：拿 HKCU 的开关配 HKLM 的地址，
    // 或者反过来，会得到一个用户从没配过的组合。
    let enabled = read_reg_dword(key, "ProxyEnable") == Some(1);
    let server = read_reg_sz(key, "ProxyServer");
    unsafe { RegCloseKey(key) };
    Some((enabled, server?))
}

#[cfg(windows)]
fn read_reg_dword(key: windows_sys::Win32::System::Registry::HKEY, name: &str) -> Option<u32> {
    use windows_sys::Win32::System::Registry::{RegGetValueW, REG_DWORD, RRF_RT_REG_DWORD};

    let name: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    let mut kind = 0u32;
    let mut data = 0u32;
    let mut len = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            key,
            std::ptr::null(),
            name.as_ptr(),
            RRF_RT_REG_DWORD,
            &mut kind,
            &mut data as *mut u32 as *mut std::ffi::c_void,
            &mut len,
        )
    };
    (status == 0 && kind == REG_DWORD).then_some(data)
}

/// 读注册表字符串值：先用一次空缓冲问出长度，再按问到的长度读回来。
/// 猜一个固定缓冲区大小会在地址特别长时**静默截断**——截断出来的代理地址
/// 连得上别的服务，比读不到更难排查。
#[cfg(windows)]
fn read_reg_sz(key: windows_sys::Win32::System::Registry::HKEY, name: &str) -> Option<String> {
    use std::ffi::c_void;
    use windows_sys::Win32::System::Registry::{RegGetValueW, REG_SZ, RRF_RT_REG_SZ};

    /// 超过它就当读不到：合法的代理地址远小于这个量级，而没有上限的分配
    /// 会让一个坏值把内存要到手。
    const MAX_BYTES: u32 = 4096;

    let name: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    let mut kind = 0u32;
    let mut bytes = 0u32;
    let size_status = unsafe {
        RegGetValueW(
            key,
            std::ptr::null(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            &mut kind,
            std::ptr::null_mut(),
            &mut bytes,
        )
    };
    if size_status != 0 || kind != REG_SZ || bytes == 0 || bytes > MAX_BYTES {
        return None;
    }
    let mut buffer = vec![0u16; bytes as usize / 2 + 1];
    let status = unsafe {
        RegGetValueW(
            key,
            std::ptr::null(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            &mut kind,
            buffer.as_mut_ptr() as *mut c_void,
            &mut bytes,
        )
    };
    if status != 0 || kind != REG_SZ {
        return None;
    }
    let text: String = std::char::decode_utf16(buffer.iter().copied())
        .take_while(|unit| !matches!(unit, Ok('\0')))
        .collect::<Result<_, _>>()
        .ok()?;
    Some(text)
}

/// `ProxyEnable` + `ProxyServer` → 代理地址。
///
/// `ProxyServer` 有两种写法：单值 `host:port`（所有协议共用），以及分协议的
/// `http=…;https=…;socks=…`。本项目只访问 https，于是按 https → 单值 → http
/// 取第一个能用的：**只有 `http=` 也要用**（让 https 走 CONNECT 是 http 代理的
/// 常规用法），而 `socks=` 直接跳过——reqwest 没开 socks 特性，拿它当代理只会
/// 换来一次必然失败的请求。
#[cfg(any(test, windows))]
fn proxy_from_windows_settings(enabled: bool, server: &str) -> Option<Url> {
    if !enabled {
        return None;
    }
    let server = server.trim();
    if !server.contains('=') {
        return normalize(server);
    }
    let mut per_protocol = std::collections::BTreeMap::new();
    for entry in server.split(';') {
        if let Some((key, value)) = entry.split_once('=') {
            per_protocol.insert(key.trim().to_ascii_lowercase(), value.trim());
        }
    }
    ["https", "http"]
        .iter()
        .find_map(|key| per_protocol.get(*key).and_then(|value| normalize(value)))
}

// --- macOS：scutil -----------------------------------------------------------

/// `scutil --proxy` 是读系统网络设置的标准入口：没有等价的安全 API 能拿到
/// 同一份数据，而引一个新 crate 只为这一条不值得。输出解析在
/// `proxy_from_scutil`（纯函数，可直接测）。
#[cfg(target_os = "macos")]
fn from_system() -> Option<(Url, &'static str)> {
    const SOURCE: &str = "macOS 系统代理设置";
    let output = std::process::Command::new("/usr/sbin/scutil")
        .arg("--proxy")
        .output()
        .ok()?;
    proxy_from_scutil(&String::from_utf8_lossy(&output.stdout)).map(|url| (url, SOURCE))
}

/// 解析 `scutil --proxy` 那个字典转储（`key : value` 逐行）。
///
/// 优先 HTTPS 那份，没有再取 HTTP 那份：让 https 流量走 http 代理同样是常规
/// 用法（macOS 自己就是这么发的 CONNECT）。
#[cfg(any(test, target_os = "macos"))]
fn proxy_from_scutil(text: &str) -> Option<Url> {
    let mut fields = std::collections::BTreeMap::new();
    for line in text.lines() {
        // 嵌套的 ExceptionsList 里没有 " : "，会被这一条自然跳过。
        if let Some((key, value)) = line.split_once(" : ") {
            fields.insert(key.trim(), value.trim());
        }
    }
    for (enable_key, proxy_key, port_key) in [
        ("HTTPSEnable", "HTTPSProxy", "HTTPSPort"),
        ("HTTPEnable", "HTTPProxy", "HTTPPort"),
    ] {
        if fields.get(enable_key) != Some(&"1") {
            continue;
        }
        let (Some(host), Some(port)) = (fields.get(proxy_key), fields.get(port_key)) else {
            continue;
        };
        if let Some(url) = normalize(&format!("{host}:{port}")) {
            return Some(url);
        }
    }
    None
}

// --- 非 Windows / 非 macOS ---------------------------------------------------

/// Linux 上没有「系统代理」这一说（发行版各行其是，读 gsettings 只会得到一个
/// 与用户实际出口无关的答案）。该平台走直连，与改动前一致。
#[cfg(not(any(windows, target_os = "macos")))]
fn from_system() -> Option<(Url, &'static str)> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url_of(value: &str) -> Url {
        Url::parse(value).expect("测试里的地址应当是合法的 URL")
    }

    /// 路由表里**只可能有当下探测到的那个地址**。
    ///
    /// 旧实现还有一份"上次成功过的代理"记忆，会在探测不到时插回路由表——
    /// 那是把一份**快照**当成配置用，会盖过用户自己的开关（`NO_PROXY=*`、
    /// 把系统代理关掉），还会在客户端换端口后先白撞一次死端口。
    #[test]
    fn routes_only_ever_contain_the_freshly_detected_proxy() {
        let with_proxy = routes_from(Some((url_of("http://127.0.0.1:7890"), "测试来源")));
        assert_eq!(with_proxy.len(), 2, "探测代理 + 直连");
        assert_eq!(
            with_proxy[0].describe(),
            // `Url` 会给空路径补一个尾斜杠，展示的就是这个形态。
            "代理 http://127.0.0.1:7890/（测试来源）"
        );
        assert_eq!(with_proxy[1], Route::Direct);

        // 探测不到就是"此刻没有可用的代理"：直连，不翻旧账。
        assert_eq!(routes_from(None), vec![Route::Direct]);
    }

    /// 退休清理必须真的把那份快照删掉，而且**不许**碰用户数据目录里的别的文件。
    ///
    /// 用 `scoped_xlink_home` 把 `DSH_XLINK_HOME` 指到临时目录——测试永远不许碰
    /// 用户真实的数据目录（那里面是用户的内核、实例与会话）。
    #[test]
    fn the_retired_proxy_memory_file_is_removed_and_other_files_are_untouched() {
        let home = std::env::temp_dir().join(format!(
            "dsh-net-proxy-retire-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).expect("建临时 home");
        let _guard = crate::tests::scoped_xlink_home(&home);

        let stale = crate::shell::paths::xlink_home().join(RETIRED_PROXY_MEMORY_FILE);
        std::fs::write(&stale, b"http://127.0.0.1:33210/\n").expect("写残留文件");
        let keep = crate::shell::paths::xlink_home().join("keep.txt");
        std::fs::write(&keep, b"keep").expect("写对照文件");

        retire_remembered_proxy_file();
        assert!(!stale.exists(), "残留的「记忆代理」文件必须被清掉");
        assert!(keep.exists(), "别的文件不许动");

        // 幂等：文件已经不在时再调一次不报错。
        retire_remembered_proxy_file();
        let _ = std::fs::remove_dir_all(&home);
    }

    /// 代理地址**不许落盘**：记忆文件一旦存在，路由表就可能用上一个跟当前
    /// 配置无关的地址。判据直接盯生产代码里那个函数名与写文件的调用。
    #[test]
    fn no_proxy_address_is_persisted_anywhere() {
        let source = include_str!("net_proxy.rs");
        // 拼起来查：这个判据的源码本身就在被检查的文件里，字面量写全了会
        // 匹配到自己（"含自身"的那种假红）。
        let forbidden = ["remember", "_proxy("].concat();
        assert!(
            !source.contains(&forbidden),
            "记忆代理机制已退休：代理地址只从当下读到的系统配置来"
        );
        let write = ["atomic_write", "(&path"].concat();
        assert!(!source.contains(&write), "代理地址不许落盘");
    }

    /// `ProxyServer` 的两种写法都要认：`host:port` 与分协议的
    /// `http=…;https=…;socks=…`。后者必须挑 https 那份，而 socks 得跳过
    /// ——reqwest 没开 socks 特性，拿它当代理只会换来一次必然失败的请求。
    #[test]
    fn windows_proxy_settings_cover_both_spellings() {
        assert_eq!(
            proxy_from_windows_settings(true, "127.0.0.1:7890"),
            Some(url_of("http://127.0.0.1:7890"))
        );
        assert_eq!(
            proxy_from_windows_settings(
                true,
                "http=1.2.3.4:8080;https=1.2.3.4:8443;socks=1.2.3.4:1080"
            ),
            Some(url_of("http://1.2.3.4:8443"))
        );
        // 只有 http= 时仍然可用：让 https 走 CONNECT 是 http 代理的常规用法。
        assert_eq!(
            proxy_from_windows_settings(true, "http=1.2.3.4:8080"),
            Some(url_of("http://1.2.3.4:8080"))
        );
        // 只有 socks= 时没有可用路线，宁可直连。
        assert_eq!(
            proxy_from_windows_settings(true, "socks=1.2.3.4:1080"),
            None
        );
        // 用户把开关关掉就是没有代理，哪怕地址还留在值里。
        assert_eq!(proxy_from_windows_settings(false, "127.0.0.1:7890"), None);
        assert_eq!(proxy_from_windows_settings(true, "  "), None);
    }

    #[test]
    fn scutil_output_is_parsed_by_protocol() {
        let both = "<dictionary> {\n  HTTPEnable : 1\n  HTTPPort : 7890\n  HTTPProxy : 127.0.0.1\n  HTTPSEnable : 1\n  HTTPSPort : 8443\n  HTTPSProxy : 10.0.0.2\n  SOCKSEnable : 0\n}";
        assert_eq!(
            proxy_from_scutil(both),
            Some(url_of("http://10.0.0.2:8443"))
        );

        let http_only = "<dictionary> {\n  HTTPEnable : 1\n  HTTPPort : 7890\n  HTTPProxy : 127.0.0.1\n  HTTPSEnable : 0\n}";
        assert_eq!(
            proxy_from_scutil(http_only),
            Some(url_of("http://127.0.0.1:7890"))
        );

        assert_eq!(
            proxy_from_scutil("<dictionary> {\n  HTTPEnable : 0\n}"),
            None
        );
        assert_eq!(proxy_from_scutil(""), None);
    }

    /// 代理地址写成什么形状都要能落成一个 URL，而 socks5 之类 reqwest 用不了
    /// 的方案要在这里就被挡住——它进了 `Route` 就等于把一次必然失败的请求
    /// 排在了直连前面。
    #[test]
    fn normalize_accepts_bare_host_port_and_rejects_unusable_schemes() {
        assert_eq!(
            normalize("127.0.0.1:7890"),
            Some(url_of("http://127.0.0.1:7890"))
        );
        assert_eq!(
            normalize(" 127.0.0.1:7890 "),
            Some(url_of("http://127.0.0.1:7890"))
        );
        assert_eq!(
            normalize("https://proxy.internal:3128"),
            Some(url_of("https://proxy.internal:3128"))
        );
        assert_eq!(normalize("socks5://127.0.0.1:1080"), None);
        assert_eq!(normalize(""), None);
    }
}
