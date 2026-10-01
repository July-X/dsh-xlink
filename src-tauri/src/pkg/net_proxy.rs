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
//! 不回退的是别的东西：签名校验失败、清单解析失败、URL 无效——换一条路只会
//! 同样地失败，见 `updater::should_try_next`。
//!
//! 代价可控：代理没在运行时，多半是连本机端口被拒（毫秒级），因此多出来的那
//! 一次尝试通常比一次失败的直连还短。
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

    /// 只有直连时**不能**凭空造一条代理路由：那是把"用户没配代理"改成
    /// "用户配了个不存在的代理"。
    #[test]
    fn routes_end_with_direct_and_start_with_the_detected_proxy() {
        let with_proxy = routes_from(Some((url_of("http://127.0.0.1:7890"), "测试来源")));
        assert_eq!(with_proxy.len(), 2);
        assert_eq!(
            with_proxy[0].describe(),
            // `Url` 会给空路径补一个尾斜杠，展示的就是这个形态。
            "代理 http://127.0.0.1:7890/（测试来源）"
        );
        assert_eq!(with_proxy[1], Route::Direct);

        let without_proxy = routes_from(None);
        assert_eq!(without_proxy, vec![Route::Direct]);
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
