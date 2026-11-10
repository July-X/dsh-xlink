//! 从 `@deepseek-ai/dsh` npm 包获取官方 kernel 发布列表。
//!
//! 主数据源是公共 npm registry，这是用户在检查更新时进入的权威目的地
//! （`https://www.npmjs.com/package/@deepseek-ai/dsh`）。其返回的 JSON
//! 文档包含所有已发布的版本以及 `dist-tags`（latest、next、beta……），
//! 因此更新菜单能拿到准确的 prerelease 标记和时间戳。
//!
//! GitHub 作为兜底：当 registry 不可达时，外壳会回退到 GitHub REST API，
//! 再回退到其公共 Atom feed（不限速但同样会暴露这些 tag）。兜底警告
//! 会被回传到 UI，让用户知道自己看到的是哪一份来源。

use std::collections::{BTreeMap, HashMap};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::pkg::net_proxy::{self, Route};
use crate::shell::error::AppError;
use crate::shell::version::cmp_versions;

/// 官方发布版本上的 tag 前缀（例如 `dsh-v0.1.1-rc.2`）。
pub const TAG_PREFIX: &str = "dsh-v";

const USER_AGENT: &str = concat!("dsh-xlink/", env!("CARGO_PKG_VERSION"));
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
/// 可达性探测的短超时（只发一个小请求，别让用户为探测等满 30 秒）。
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_HTTP_BODY_BYTES: u64 = 10 * 1024 * 1024;
const MAX_TARBALL_BYTES: u64 = 256 * 1024 * 1024;

/// 文件下载（npm tarball / Node.js 包）的超时档：与 JSON API 请求分开。
///
/// 30s 的 `timeout_global` 覆盖到响应体读完：MiniMax tarball 有 57.8 MB，实测
/// 直连官方源要 28s、网络稍慢就必然超过 30s（2026-11-09 用户报
/// 「下载失败：timeout: global」）。下载不再给 body 设短总时限，只保留连接
/// 超时、单次阻塞读的空闲上限和 30 分钟硬顶。
#[derive(Clone, Copy)]
struct DownloadLimits {
    connect: Duration,
    idle: Duration,
    total: Duration,
}

const DOWNLOAD_LIMITS: DownloadLimits = DownloadLimits {
    // 连接仍要短：建连失败换源重试是另一条路。
    connect: HTTP_TIMEOUT,
    // 持续有数据就不算超时；完全静默 120s 才按断流处理。
    idle: Duration::from_secs(120),
    // 防止上游一直极慢地滴数据把安装线程留一整天。
    total: Duration::from_secs(30 * 60),
};

/// 路由 → 显式代理配置：与 `openai::transport::proxy_for` 同一条纪律，
/// `Route::Direct` 必须显式 `.proxy(None)`——ureq 的默认配置会自动捡环境
/// 变量里的代理，不关掉就等于「回退直连」仍被 `HTTPS_PROXY` 拉回代理。
fn proxy_for(route: &Route) -> Result<Option<ureq::Proxy>, HttpError> {
    match route {
        Route::Direct => Ok(None),
        Route::Proxy { url, .. } => ureq::Proxy::new(url.as_str()).map(Some).map_err(|error| {
            HttpError::Policy(format!(
                "系统代理地址不合法（{url}）：{error}；请检查系统代理设置后重试"
            ))
        }),
    }
}

/// 回环目标不走代理路由（与 `openai::transport::routes_for_url` 同一条）：
/// 代理不会替你访问本机。生产取源不会命中回环，测试夹具靠它不被本机代理接管。
fn routes_for_url(routes: &[Route], url: &str) -> Vec<Route> {
    let loopback = url.contains("://127.0.0.1:") || url.contains("://localhost:");
    if loopback {
        vec![Route::Direct]
    } else {
        routes.to_vec()
    }
}

/// 按路由表逐条试：**只有传输层失败才换下一条**。HTTP 状态、策略拒绝与
/// 解析失败换一条路只会同样失败，还会把第一个真正原因盖掉。
fn try_routes<T>(
    routes: &[Route],
    mut attempt: impl FnMut(&Route) -> Result<T, HttpError>,
) -> Result<T, HttpError> {
    let mut last = None;
    for route in routes {
        match attempt(route) {
            Ok(value) => return Ok(value),
            Err(error) => {
                let retryable = match &error {
                    HttpError::Ureq(error) => is_transport_error(error),
                    HttpError::Io(_) => true,
                    HttpError::Policy(_) => false,
                };
                if !retryable {
                    return Err(error);
                }
                last = Some(error);
            }
        }
    }
    Err(last.unwrap_or_else(|| HttpError::Policy("没有可用的网络路由；请检查系统代理设置".into())))
}

/// JSON API 用的 agent：30s 总时限（下载另有独立时限档，见 [`DownloadLimits`]）。
fn api_agent_for(route: &Route) -> Result<ureq::Agent, HttpError> {
    Ok(ureq::Agent::config_builder()
        .timeout_global(Some(HTTP_TIMEOUT))
        // 逐跳跟随与校验由 `follow_redirects` 手动完成（2026-11-01 OPT-02）。
        .max_redirects(0)
        .proxy(proxy_for(route)?)
        .build()
        .new_agent())
}

/// 可达性探测用的 agent：5 秒总时限、不跟随重定向（3xx 说明源本身可达）。
fn probe_agent_for(route: &Route) -> Result<ureq::Agent, HttpError> {
    Ok(ureq::Agent::config_builder()
        .timeout_global(Some(PROBE_TIMEOUT))
        .max_redirects(0)
        .proxy(proxy_for(route)?)
        .build()
        .new_agent())
}

/// 下载专用 agent：连接 / 空闲 / 总时限三档，不套 30s 的 API 总时限。
fn download_agent_for(route: &Route, limits: DownloadLimits) -> Result<ureq::Agent, HttpError> {
    Ok(ureq::Agent::config_builder()
        .timeout_connect(Some(limits.connect))
        .timeout_recv_body(Some(limits.idle))
        .timeout_global(Some(limits.total))
        .max_redirects(0)
        .proxy(proxy_for(route)?)
        .build()
        .new_agent())
}

/// ureq 3.4 在总时限到期后把剩余时间换成 1 秒的套接字超时：只要每秒还有
/// 字节到达，正文读取就不会报超时（与 `openai::transport::StreamDeadlineReader`
/// 同一条实测，046800c2）。下载的 30 分钟硬顶因此必须自己再核一次绝对截止
/// 时刻——否则上游一直慢速滴数据能把安装线程无限续命。
struct DownloadDeadlineReader<R> {
    inner: R,
    deadline: Instant,
}

fn check_download_deadline(deadline: Instant) -> io::Result<()> {
    if Instant::now() >= deadline {
        return Err(ureq::Error::Timeout(ureq::Timeout::Global).into_io());
    }
    Ok(())
}

impl<R: Read> Read for DownloadDeadlineReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        check_download_deadline(self.deadline)?;
        let read = self.inner.read(buf)?;
        // 阻塞读可能在截止前开始、截止后才拿到数据；不能再把它当成功。
        check_download_deadline(self.deadline)?;
        Ok(read)
    }
}

/// 探测「候选源是否可达」时查询的内核包名。与版本列表、内核安装走同一条链。
pub(crate) const DSH_NPM_PACKAGE: &str = "@deepseek-ai/dsh";

/// 内核安装使用的 registry 记录 key：只由内核版本列表写入。
pub(crate) const KERNEL_REGISTRY_KEY: &str = DSH_NPM_PACKAGE;
/// 插件 / profile / git 构建等非内核入口共用的 registry 记录 key。
pub(crate) const SHELL_REGISTRY_KEY: &str = "shell";

/// registry 记录的有效期：网络变化不会通知进程，记录太久会把安装钉在
/// 一个已经不可达的源上；超过这个时间就重新探测一次。
const REGISTRY_RECORD_TTL: Duration = Duration::from_secs(10 * 60);

/// 最近一次**真正成功**的 registry 取源 base（进程级记忆，按包隔离）。
///
/// 写入只发生在 [`http_get_registry_string`] 成功时——它返回的 base 就是这份
/// 内容真正来自的源。**按 key 隔离**：内核安装只看内核版本列表写下的源，
/// 插件 / 技能取源不会把它改掉（2026-11-01 评审：原先全局单值会让一次插件
/// 检查更新的镜像结果带进内核安装）。
///
/// **不落盘**：源是否可达是当下的网络事实，与 `net_proxy` 那条「不记忆代理
/// 地址」同一条纪律——持久化只会让下一次启动先去撞一个可能已经死掉的源。
static LAST_REGISTRY_BASES: Mutex<Option<HashMap<String, (String, Instant)>>> = Mutex::new(None);

/// 某个 key 最近一次成功取源的 base；`None` = 没记录或已过 TTL。
pub(crate) fn last_registry_base(key: &str) -> Option<String> {
    let mut guard = crate::lock(&LAST_REGISTRY_BASES);
    let map = guard.as_mut()?;
    match map.get(key) {
        Some((base, at)) if at.elapsed() <= REGISTRY_RECORD_TTL => Some(base.clone()),
        Some(_) => {
            map.remove(key);
            None
        }
        None => None,
    }
}

/// 记录一次成功取源的 base。覆盖旧值：以最近一次成功的为准。
fn remember_registry_base(key: &str, base: &str) {
    let mut guard = crate::lock(&LAST_REGISTRY_BASES);
    guard
        .get_or_insert_with(HashMap::new)
        .insert(key.to_string(), (base.to_string(), Instant::now()));
}

/// 一次取源允许跟随的重定向上限（与旧 `max_redirects(5)` 一致）。
const MAX_REDIRECTS: usize = 5;

/// 3xx 是否算需要跟随的重定向。与 ureq 的自动跟随判据一致：304 没有
/// Location，按普通响应返回。
fn is_redirect_status(status: u16) -> bool {
    (300..400).contains(&status) && status != 304
}

/// 把 `Location` 解析为下一跳的绝对 URL；相对 Location 按当前 URL 解析。
/// fragment 不属于请求目标，解析时丢掉。
fn resolve_location(current: &str, location: &str) -> Result<String, String> {
    let base = url::Url::parse(current)
        .map_err(|e| format!("当前取源地址不是合法 URL（{e}）；已在下载前中止"))?;
    let mut resolved = base
        .join(location.trim())
        .map_err(|e| format!("重定向目标的 Location 无法解析（{e}）；已在下载前中止"))?;
    resolved.set_fragment(None);
    Ok(resolved.to_string())
}

/// 发请求前的逐跳校验。
///
/// 首跳沿用 [`crate::pkg::registry::validate_https_url`] 的文案（「请求地址」），
/// 后续跳与 tarball 走同一份 [`crate::pkg::registry::validate_redirect_chain`]；
/// `allow_insecure_http` 只给本文件测试夹具（`http://127.0.0.1`）跳过 scheme
/// 校验，userinfo 与 host allowlist 照常生效。生产路径恒为 false。
fn validate_hop(
    url: &str,
    first: bool,
    allowed_hosts: Option<&[String]>,
    allow_insecure_http: bool,
) -> Result<(), String> {
    if allow_insecure_http {
        let parsed = url::Url::parse(url)
            .map_err(|e| format!("取源地址不是合法 URL（{e}）；已在下载前中止"))?;
        if !parsed.username().is_empty() || parsed.password().is_some() {
            return Err("取源地址不允许携带用户名或密码；已在下载前中止".to_string());
        }
        if let Some(hosts) = allowed_hosts {
            crate::pkg::registry::host_allowed(&parsed, hosts)?;
        }
        return Ok(());
    }
    if first {
        let parsed = crate::pkg::registry::validate_https_url(url, "请求地址")?;
        if let Some(hosts) = allowed_hosts {
            crate::pkg::registry::host_allowed(&parsed, hosts)?;
        }
        return Ok(());
    }
    let owned = url.to_string();
    crate::pkg::registry::validate_redirect_chain(std::slice::from_ref(&owned), allowed_hosts)
}

/// 手动逐跳跟随重定向：**每一跳发请求之前**先校验，被拒绝的目的地不会收到
/// 任何请求。`send` 返回 `(状态码, Location, 最终响应)`，测试可以注入一个
/// 只记录 URL 的假 transport 来证明禁止目标的请求计数为 0。
///
/// 错误分类保持原样：策略拒绝是 [`HttpError::Policy`]（不换源），传输失败是
/// [`HttpError::Ureq`]（可换源），HTTP 4xx/5xx 由 ureq 的
/// `http_status_as_error` 变成 `Ureq(StatusCode)`（源可达，不换源）。
fn follow_redirects<R>(
    url: &str,
    allowed_hosts: Option<&[String]>,
    allow_insecure_http: bool,
    mut send: impl FnMut(&str) -> Result<(u16, Option<String>, R), HttpError>,
) -> Result<R, HttpError> {
    let mut current = url.to_string();
    let mut hops = 0usize;
    loop {
        validate_hop(&current, hops == 0, allowed_hosts, allow_insecure_http)
            .map_err(HttpError::Policy)?;
        let (status, location, response) = send(&current)?;
        if !is_redirect_status(status) {
            return Ok(response);
        }
        if hops >= MAX_REDIRECTS {
            return Err(HttpError::Policy(format!(
                "重定向超过 {MAX_REDIRECTS} 跳上限；已在下载前中止，请检查取源地址是否在循环跳转"
            )));
        }
        let location = location
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                HttpError::Policy("重定向响应缺少可用的 Location；已在下载前中止".to_string())
            })?;
        current = resolve_location(&current, &location).map_err(HttpError::Policy)?;
        hops += 1;
    }
}

/// 用给定 agent 发一次 GET，并取出状态码与 Location；调用方按路由建 agent
/// （JSON / 探测 / 下载三档时限）。
fn send_get_with(
    agent: &ureq::Agent,
    url: &str,
    accept: Option<&str>,
) -> Result<(u16, Option<String>, ureq::http::Response<ureq::Body>), HttpError> {
    let mut request = agent.get(url).header("User-Agent", USER_AGENT);
    if let Some(value) = accept {
        request = request.header("Accept", value);
    }
    let response = request.call().map_err(HttpError::Ureq)?;
    let status = response.status().as_u16();
    let location = response
        .headers()
        .get(ureq::http::header::LOCATION)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    Ok((status, location, response))
}

/// 校验一个已下载文件的 npm 摘要（SRI 的 `sha512-<base64>` / `sha256-<base64>`，
/// 或老 packument 的 `dist.shasum` sha1 十六进制）。
///
/// 外壳自己下载的 tarball 必须校验一次：默认 registry 是第三方镜像，而解包之后
/// pnpm 会执行包里的 `prepare` 生命周期脚本——那等于执行未经验证的下载物。它能
/// 发现"元数据与 tarball 不同源"、传输损坏与缓存不一致（镜像若**同时**改写了
/// packument 与 tarball，任何同源校验都发现不了，那需要与上游 registry 的独立
/// 对账；这里不要把它说成更强的保证）。
///
/// **必须支持一次给出多个摘要**：SRI 规范允许空格分隔的摘要列表，npm 的 `ssri`
/// 也确实会产出这种形态。旧实现把 `sha512-A sha256-B` 整串送进 base64 解码，空格
/// 让它必然失败——无论内容对不对都装不上（P2-6）。现在按 token 解析，取其中
/// **最强且受支持**的算法校验；单个 token 形态不认识时跳过而不是整串失败。
///
/// **两条摘要都没有时拒绝安装**（fail-closed）：老 packument 常常只有 sha1 的
/// `dist.shasum`，所以先回退到它；两者都缺说明这份元数据根本没打算被校验，继续
/// 安装等于把「下载物未经校验」当成默认路径，而下游要执行包里的脚本。回退只在
/// **没有可用 integrity token 时**发生：integrity 里只要有一条受支持的摘要就按它
/// 裁决，不符即拒绝，绝不再拿更弱的 sha1 兜底（否则篡改者只要让 sha512 对不上、
/// sha1 恰好对得上就能通过）。
///
/// 返回 `Ok(Some(算法名))` 表示已校验。
pub fn verify_download_integrity(
    path: &Path,
    integrity: Option<&str>,
    shasum: Option<&str>,
) -> Result<Option<&'static str>, String> {
    use sha2::{Digest, Sha256, Sha512};

    let bytes = fs::read(path).map_err(|e| format!("无法读取 {}：{e}", path.display()))?;
    if let Some((algorithm, expected)) = strongest_integrity(integrity) {
        let actual = match algorithm {
            "sha512" => Sha512::digest(&bytes).to_vec(),
            _ => Sha256::digest(&bytes).to_vec(),
        };
        if actual != expected {
            return Err(format!(
                "下载内容与 registry 声明的 integrity 不符（算法 {algorithm}）：文件可能被镜像替换或传输损坏"
            ));
        }
        return Ok(Some(algorithm));
    }
    let Some(shasum) = shasum.map(str::trim).filter(|value| !value.is_empty()) else {
        return Err(String::from(
            "registry 的元数据里既没有 dist.integrity 也没有 dist.shasum，无法校验下载内容，已拒绝安装；\
             请改用提供摘要的 registry（DSH_NPM_REGISTRY）或联系镜像维护者",
        ));
    };
    let expected = shasum.to_ascii_lowercase();
    let actual = {
        use sha1::{Digest as _, Sha1};
        Sha1::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    if actual != expected {
        return Err(format!(
            "下载内容与 registry 声明的 shasum 不符（算法 sha1）：期望 {expected}，实际 {actual}；\
             文件可能被镜像替换或传输损坏"
        ));
    }
    Ok(Some("sha1"))
}

/// **不读文件**地报告「这次下载会用哪种摘要裁决」。
///
/// 诊断页要在安装**之前**就让用户看到完整性口径（设计 §6.5 要求首屏展示
/// sha512 / sha256 校验结果）。[`verify_download_integrity`] 要等下载完成才能
/// 调用，那时用户已经点过安装了——所以这里把选择逻辑单独暴露一份，与校验
/// 本身共用 [`strongest_integrity`]，保证「显示的」和「执行的」不会分叉。
///
/// 返回 `sha512` / `sha256` / `sha1`；三者都没有时返回 `None`——那种情况
/// [`verify_download_integrity`] 会 fail-closed 拒绝安装，UI 显示「未能校验」
/// 是如实陈述，不是警告一个不会发生的未来。
#[allow(dead_code, reason = "给 UI 报用了哪种摘要的查询口")]
pub fn integrity_algorithm(integrity: Option<&str>, shasum: Option<&str>) -> Option<&'static str> {
    if let Some((algorithm, _)) = strongest_integrity(integrity) {
        return Some(algorithm);
    }
    shasum
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|_| "sha1")
}

/// 从 SRI 字符串里挑出「最强且受支持」的那条摘要：sha512 > sha256。
///
/// 单独抽出来是因为 `verify_download_integrity` 要先知道"有没有可用的强摘要"，
/// 才能决定是否回退到 sha1 的 `shasum`。
fn strongest_integrity(integrity: Option<&str>) -> Option<(&'static str, Vec<u8>)> {
    let integrity = integrity.map(str::trim).filter(|value| !value.is_empty())?;
    let mut best: Option<(&'static str, Vec<u8>)> = None;
    for token in integrity.split_whitespace() {
        let Some((algorithm, encoded)) = token.split_once('-') else {
            continue;
        };
        let algorithm: &'static str = match algorithm {
            "sha512" => "sha512",
            "sha256" => "sha256",
            // 不支持的算法（例如未来出现的 sha3）不阻断安装，只要还有一条能用。
            _ => continue,
        };
        let Some(expected) = decode_base64(encoded.trim()) else {
            continue;
        };
        let replace = match &best {
            Some((current, _)) => *current == "sha256" && algorithm == "sha512",
            None => true,
        };
        if replace {
            best = Some((algorithm, expected));
        }
    }
    best
}

/// 解码标准 base64（SRI 使用的字符集，含 `+/` 与 `=` 填充）。
fn decode_base64(input: &str) -> Option<Vec<u8>> {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut lookup = [255u8; 256];
    for (index, byte) in TABLE.iter().enumerate() {
        lookup[*byte as usize] = index as u8;
    }
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let mut buffer = 0u32;
    let mut bits = 0u32;
    for byte in input.bytes() {
        if byte == b'=' {
            break;
        }
        let value = lookup[byte as usize];
        if value == 255 {
            return None;
        }
        buffer = (buffer << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
        }
    }
    Some(out)
}

/// 取源层错误：网络错误与策略错误分开，registry 路由只对前者换源。
#[derive(Debug)]
enum HttpError {
    Ureq(ureq::Error),
    /// 响应体读到一半的本机 IO 错误（传输中断一类，可换路）。单独一个变体
    /// 是为了错误文案与改动前逐字一致（`ureq::Error::Io` 会多一层前缀）。
    Io(io::Error),
    Policy(String),
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpError::Ureq(error) => write!(f, "{error}"),
            HttpError::Io(error) => write!(f, "{error}"),
            HttpError::Policy(reason) => write!(f, "{reason}"),
        }
    }
}

impl From<ureq::Error> for HttpError {
    fn from(error: ureq::Error) -> Self {
        HttpError::Ureq(error)
    }
}

/// GET `url` 并把 body 作为文本返回。桌面端每次外发请求都带上桌面
/// User-Agent，并对响应体大小做了上限。
pub(crate) fn http_get_string(url: &str, accept: Option<&str>) -> Result<String, String> {
    http_get_string_at(url, accept).map_err(|e| e.to_string())
}

/// 同 [`http_get_string`]，但保留错误分类——registry 路由要用它区分
/// 「源不可达（可换下一个）」「策略拒绝（不能换源）」与「源可达但拒绝了请求」。
fn http_get_string_at(url: &str, accept: Option<&str>) -> Result<String, HttpError> {
    http_get_string_at_with(url, accept, false)
}

/// http_get_string_at 的实现。allow_insecure_http 只给同文件测试用：本地夹具
/// 起的是 http://127.0.0.1:<随机端口>，生产路径永远传 false（2026-11-01 OPT-02）。
fn http_get_string_at_with(
    url: &str,
    accept: Option<&str>,
    allow_insecure_http: bool,
) -> Result<String, HttpError> {
    let routes = routes_for_url(&net_proxy::routes(), url);
    http_get_string_on_routes(&routes, url, accept, allow_insecure_http)
}

/// [`http_get_string_at_with`] 的路由可注入版本（用例直接摆路由，不碰本机代理）。
fn http_get_string_on_routes(
    routes: &[Route],
    url: &str,
    accept: Option<&str>,
    allow_insecure_http: bool,
) -> Result<String, HttpError> {
    try_routes(routes, |route| {
        let agent = api_agent_for(route)?;
        let mut response = follow_redirects(url, None, allow_insecure_http, |target| {
            send_get_with(&agent, target, accept)
        })?;
        response
            .body_mut()
            .with_config()
            .limit(MAX_HTTP_BODY_BYTES)
            .read_to_string()
            .map_err(HttpError::Ureq)
    })
}

/// 这个错误是否属于**传输层不可达**：连接失败、DNS 解析失败、超时、连接中断。
///
/// 只有这一类才值得换下一个 registry。HTTP 状态码错误、策略拒绝、JSON
/// 解析失败都说明**源是可达的**——另一个源不会给出不同的答案，换过去只会
/// 把「上游发布事故」说成「镜像延迟」。
fn registry_unreachable(error: &HttpError) -> bool {
    matches!(error, HttpError::Ureq(error) if is_transport_error(error))
}

/// 传输层不可达的 ureq 错误（连接失败 / DNS / 超时 / 中断）。probe 直接用
/// ureq 原始错误类型，因此单独抽一层。
fn is_transport_error(error: &ureq::Error) -> bool {
    matches!(error, ureq::Error::Io(_) | ureq::Error::Timeout(_))
}

/// 按 [`crate::pkg::registry::npm_registry_candidates`] 的顺序取一个 registry
/// 资源，返回 `(实际成功的 base, body)`。
///
/// 逐条尝试：传输层不可达才换下一个；显式 `DSH_NPM_REGISTRY` 时候选链只有
/// 一条，因此不会兜底。调用方拿到的 base 就是这份内容真正来自的源——内核
/// 安装用它去驱动 pnpm，版本列表用它渲染，两边因此看到同一份版本线。
///
/// 成功时把 base 记进进程级记忆（[`last_registry_base`]）：内核安装与
/// pnpm/npm 子进程据此「列表读到哪个源就沿用哪个源」，不再二次探测。
/// [`http_get_registry_string_from`] 是测试入口，**刻意不记账**——它不该有
/// 进程级副作用，用例也不必为全局值做串行安排。
pub(crate) fn http_get_registry_string(
    record_key: &str,
    path: &str,
    accept: Option<&str>,
) -> Result<(String, String), String> {
    let bases = crate::pkg::registry::npm_registry_candidates()
        .map_err(|e| format!("npm registry 配置无效：{e}"))?;
    let (base, body) = http_get_registry_string_from(path, accept, &bases, false)?;
    remember_registry_base(record_key, &base);
    Ok((base, body))
}

/// [`http_get_registry_string`] 的实现，候选链由调用方给出（测试用它替换
/// 真实的进程环境候选）。
fn http_get_registry_string_from(
    path: &str,
    accept: Option<&str>,
    bases: &[String],
    allow_insecure_http: bool,
) -> Result<(String, String), String> {
    let mut failures = Vec::new();
    for base in bases {
        let url = format!("{base}{path}");
        match http_get_string_at_with(&url, accept, allow_insecure_http) {
            Ok(body) => return Ok((base.clone(), body)),
            Err(error) => {
                let retryable = registry_unreachable(&error);
                failures.push(format!("{base} 不可用（{error}）"));
                if !retryable {
                    break;
                }
            }
        }
    }
    Err(format!("npm registry 取源失败：{}", failures.join("；")))
}

/// `-/package/<包名>/dist-tags` 的 registry 相对路径。包名按 URL 规则编码，
/// 与 `npm view` 读到的 dist-tag 端点一致。
fn dist_tags_path(package: &str) -> String {
    let encoded: String = url::form_urlencoded::byte_serialize(package.as_bytes()).collect();
    format!("-/package/{encoded}/dist-tags")
}

/// 探测当前实际可用的 registry，返回成功的 base。
///
/// 只用于**本进程还没有成功取源记录**时的兜底：安装入口优先复用
/// [`http_get_registry_string`] 记下的源（列表读到哪个源，安装就用哪个源，
/// 跨源因此不可能发生）；插件安装 / profile 安装 / 自动安装 pnpm 这些
/// 「先 spawn 子进程、没有经候选链取源」的入口也用它补一次记录
/// （[`ensure_registry_base`]）。
///
/// 探测只发一个小请求（dist-tags），并用**远短于安装的 5 秒超时**——正常
/// 网络下它只多花几十毫秒；官方源真的不可达时也不会让用户先等 30 秒。
///
/// 与 [`http_get_registry_string`] 同一条分类纪律：**只有传输层不可达
/// （连接失败 / DNS / 超时 / 中断）才换下一个源**。源可达但返回 4xx/5xx 时
/// 换源不会得到不同答案，反而会把官方源的瞬时故障说成「不可达」、把安装
/// 指向逐包同步滞后的镜像（2026-10-28 评审）。
///
/// 全部候选都不可达时返回 `None`：调用方继续用首选源，让 pnpm 报出真实的
/// 网络错误，而不是因为一次探测失败就拒绝安装（探测是优化，不是闸门）。
pub(crate) fn probe_registry_base(package: &str) -> Option<String> {
    // 配置不合法时不探测：真正的错误由 ensure_registry_base 报给用户。
    let bases = crate::pkg::registry::npm_registry_candidates().ok()?;
    probe_registry_base_from(package, &bases)
}

/// [`probe_registry_base`] 的实现：候选链由调用方给出（测试用它替换真实环境
/// 候选，不起公网请求）。
fn probe_registry_base_from(package: &str, bases: &[String]) -> Option<String> {
    let path = dist_tags_path(package);
    let routes = net_proxy::routes();
    for base in bases {
        let url = format!("{base}{path}");
        let reachable = try_routes(&routes_for_url(&routes, &url), |route| {
            let agent = probe_agent_for(route)?;
            agent
                .get(&url)
                .header("User-Agent", USER_AGENT)
                .call()
                .map(|_| ())
                .map_err(HttpError::Ureq)
        });
        match reachable {
            // 3xx 也算可达：探测只看「源在这一刻是否连得上」。
            Ok(()) => return Some(base.clone()),
            // 传输层不可达：换下一个源。
            Err(HttpError::Ureq(error)) if is_transport_error(&error) => continue,
            // 源可达但拒绝/报错：同样的问题换源不会消失，回落首选源失败更诚实。
            Err(_) => return None,
        }
    }
    None
}

/// 安装 / 子进程该用哪个 registry base：最近一次成功取源的源优先。
///
/// 顺序即契约：
/// 1. `recorded`——版本列表（或最近一次包元数据 / tarball）已经证明可用的源。
///    列表读到哪个源，安装就用哪个，跨源因此不可能发生。
/// 2. `probe`——本进程还没成功取过源时现场探测一次（请求小、5 秒超时）。
/// 3. `preferred`——探测也没结果就交给 pnpm 报真实网络错误；探测是优化，
///    不是闸门。
///
/// 抽成纯函数是为了能单测三条分支：真实现里的 `recorded` / `probe` 都带
/// 进程级副作用（全局记录 + 真实网络），在用例里摆出「recorded 有值」只能
/// 靠污染全局状态，会与并行用例互相闪烁。
pub(crate) fn choose_install_registry(
    recorded: Option<String>,
    preferred: &str,
    probe: impl FnOnce() -> Option<String>,
) -> String {
    match recorded {
        Some(base) => base,
        None => probe().unwrap_or_else(|| preferred.to_string()),
    }
}

/// 在真正 spawn npm/pnpm 之前补一次「最近成功源」记录，返回应使用的源。
///
/// 插件安装 / 内核版本列表会经候选链取源，`http_get_registry_string` 成功时
/// 自然写入记录；但 git 来源的插件构建、启动期 profile 依赖修复、首次自动
/// 安装 pnpm 这些入口是「先 spawn 子进程再说」，此时本进程可能还没有任何
/// 记录——子进程会落到首选源，官方源不可达时装不上。
///
/// 有记录时立即返回，**不重复探测**（每次探测最长 5 秒）。探测失败也不阻断
/// 安装：返回首选源，让 pnpm/npm 报出真实的网络错误——这是优化，不是闸门。
pub(crate) fn ensure_registry_base(
    record_key: &str,
    probe_package: &str,
) -> Result<String, String> {
    if let Some(base) = last_registry_base(record_key) {
        return Ok(base);
    }
    let preferred = crate::pkg::registry::npm_registry_base()
        .map_err(|e| format!("npm registry 配置无效：{e}"))?;
    Ok(probe_registry_base(probe_package)
        .inspect(|base| remember_registry_base(record_key, base))
        .unwrap_or(preferred))
}

/// GET `url` 并把 body 流式写入 `destination`，不把整包 tarball 缓冲到
/// 内存中。复制成功后目标文件才视为完整。
pub(crate) fn http_get_file(url: &str, destination: &Path) -> Result<(), String> {
    http_get_file_with_hosts(url, destination, None)
}

/// tarball 下载（2026-11-01 OPT-02）：在通用策略之外，逐跳限制允许的下载域。
///
/// 允许域由调用方给出（npm registry 主机 + DSH_NPM_TARBALL_HOSTS）。每一跳都在
/// **发请求之前**校验；任何一跳不合格都不会把请求发出去，并删除 .part、返回
/// 可操作原因。
pub(crate) fn http_get_file_with_hosts(
    url: &str,
    destination: &Path,
    allowed_hosts: Option<&[String]>,
) -> Result<(), String> {
    http_get_file_with_options(url, destination, allowed_hosts, false, DOWNLOAD_LIMITS)
}

/// [`http_get_file_with_hosts`] 的实现：测试注入明文回环与更短的超时档。
fn http_get_file_with_options(
    url: &str,
    destination: &Path,
    allowed_hosts: Option<&[String]>,
    allow_insecure_http: bool,
    limits: DownloadLimits,
) -> Result<(), String> {
    let routes = routes_for_url(&net_proxy::routes(), url);
    http_get_file_on_routes(
        &routes,
        url,
        destination,
        allowed_hosts,
        allow_insecure_http,
        limits,
    )
    .map_err(|error| error.to_string())
}

/// [`http_get_file_with_options`] 的路由可注入版本。
fn http_get_file_on_routes(
    routes: &[Route],
    url: &str,
    destination: &Path,
    allowed_hosts: Option<&[String]>,
    allow_insecure_http: bool,
    limits: DownloadLimits,
) -> Result<(), HttpError> {
    let Some(file_name) = destination.file_name() else {
        return Err(HttpError::Policy("下载目标不是文件路径".into()));
    };
    let mut partial_name = file_name.to_os_string();
    partial_name.push(".part");
    let partial = destination.with_file_name(partial_name);
    try_routes(routes, |route| {
        let agent = download_agent_for(route, limits)?;
        // 逐跳校验与跟随都在 `follow_redirects` 里：每一跳发请求之前校验
        // HTTPS、userinfo、下载域与跳数上限，禁止目标不会收到请求。
        let attempt = (|| {
            // 总时限从本次尝试开始算（与 ureq 自己的 `timeout_global` 同一
            // 起点；正文读取另由 `DownloadDeadlineReader` 核绝对截止时刻）。
            let deadline = Instant::now() + limits.total;
            let mut response =
                follow_redirects(url, allowed_hosts, allow_insecure_http, |target| {
                    send_get_with(&agent, target, None)
                })?;
            let mut reader = DownloadDeadlineReader {
                inner: response
                    .body_mut()
                    .with_config()
                    .limit(MAX_TARBALL_BYTES)
                    .reader(),
                deadline,
            };
            let mut file = File::create(&partial).map_err(|e| HttpError::Policy(e.to_string()))?;
            // 读侧失败是传输中断，可以换路；写侧（磁盘满 / 权限）换路没有意义，
            // 也不能把本机问题说成网络问题。
            let mut buffer = [0u8; 64 * 1024];
            loop {
                let read = reader.read(&mut buffer).map_err(HttpError::Io)?;
                if read == 0 {
                    break;
                }
                file.write_all(&buffer[..read])
                    .map_err(|e| HttpError::Policy(e.to_string()))?;
            }
            file.sync_all()
                .map_err(|e| HttpError::Policy(e.to_string()))?;
            fs::rename(&partial, destination).map_err(|e| HttpError::Policy(e.to_string()))?;
            Ok(())
        })();
        if attempt.is_err() {
            let _ = fs::remove_file(&partial);
        }
        attempt
    })
}

/// 仅获取 npm 的 `latest` dist-tag，用于更新检查。包的元数据与
/// tarball 都使用同一个可配置的 registry 候选链。
pub(crate) fn http_get_npm_latest(package: &str) -> Result<Option<String>, String> {
    let (_, body) = http_get_registry_string(package, &dist_tags_path(package), None)?;
    let tags: BTreeMap<String, String> =
        serde_json::from_str(&body).map_err(|e: serde_json::Error| e.to_string())?;
    Ok(tags.get("latest").cloned())
}

/// 在更新菜单「打开发布」链接中使用的人类可读 web URL。
pub const NPM_PACKAGE_URL: &str = "https://www.npmjs.com/package/@deepseek-ai/dsh";

const GITHUB_API_URL: &str =
    "https://api.github.com/repos/deepseek-ai/deepseek-harness/releases?per_page=30";
const GITHUB_ATOM_URL: &str = "https://github.com/deepseek-ai/deepseek-harness/releases.atom";

/// 一次官方 kernel 发布，按更新菜单中的展示形式呈现。
#[derive(Debug, Clone, Serialize)]
pub struct ReleaseInfo {
    /// 完整 tag，例如 `dsh-v0.1.1-rc.2`。
    pub tag: String,
    /// 从 tag 中提取出的版本号，例如 `0.1.1-rc.2`。
    pub version: String,
    pub prerelease: bool,
    pub name: String,
    pub published_at: Option<String>,
    pub html_url: String,
}

/// 去掉 `dsh-v` 前缀；与本项目无关的 tag 返回 `None`。
pub fn version_from_tag(tag: &str) -> Option<String> {
    let rest = tag.strip_prefix(TAG_PREFIX)?;
    (!rest.is_empty()).then(|| rest.to_string())
}

fn release_from_tag(tag: String, prerelease: bool) -> Option<ReleaseInfo> {
    let version = version_from_tag(&tag)?;
    // `prerelease` 由调用方传入：GitHub REST 会带真实的 prerelease 标志，而
    // Atom 兜底路径只能从 tag 名推断。两条路径都必须叠加上"版本号里带 `-`"
    // 这一条——否则 Atom 回退时 `0.1.2-rc.19` 会被当成稳定版：列表不打预发布
    // 标签，首次运行引导的「安装最新版本」还会优先选中它。
    let prerelease = prerelease || version.contains('-');
    // 版本号随后会被拼进 `kernels/<version>` 路径与 stub package.json，因此
    // 在这里（GitHub API 与 Atom 两条来源的共同出口）就过滤掉非法形态。
    // git tag 允许 `/`，历史上 GitHub 回退路径会把 `dsh-v1.0/hotfix` 变成
    // `1.0/hotfix` 并在 kernels/ 下建出嵌套目录。
    if !crate::shell::version::is_valid_kernel_version(&version) {
        eprintln!("dsh-xlink: 跳过形态非法的内核版本号：{version:?}");
        return None;
    }
    Some(ReleaseInfo {
        tag: tag.clone(),
        version,
        prerelease,
        // `name` 和 `html_url` 都会消费 `tag`；为另一个 clone 一次。
        name: tag.clone(),
        published_at: None,
        // 面向用户的发布 URL 现在指向 npm 包页面；GitHub 和 npm
        // 在同一个 tag 下发布，用户仍然能在那里看到期望的版本。
        html_url: NPM_PACKAGE_URL.to_string(),
    })
}

/// npm `versions` 对象中的一个条目——只反序列化渲染更新菜单实际需要的字段。
#[derive(Debug, Deserialize)]
struct NpmVersion {
    /// npm 的 `time[version]` 单独读取；这里仅在存在时作为回退
    /// （某些镜像只在 version 内暴露 `time`）。
    #[serde(default)]
    #[serde(rename = "date")]
    date: Option<String>,
    #[serde(default)]
    deprecated: Option<String>,
}

/// 单个包的 npm registry 响应顶层结构。
#[derive(Debug, Deserialize)]
struct NpmPackageDoc {
    /// 已发布版本的完整集合：`version -> 元数据`。
    #[serde(default)]
    versions: BTreeMap<String, NpmVersion>,
    /// 每个版本的发布时间戳（再加上 `created`/`modified` 这类非版本条目）。
    /// npm 实际上的发布时间都放在这里；`NpmVersion::date` 只是镜像的回退。
    #[serde(default)]
    time: BTreeMap<String, String>,
    /// `dist-tags`：`latest`、`next`、`beta` 等。仅当版本被打上这些 tag 时
    /// 才被当作 prerelease；其余都视为稳定版本。
    #[serde(rename = "dist-tags", default)]
    dist_tags: BTreeMap<String, String>,
}

impl NpmPackageDoc {
    /// 由 `dist-tags` 构造版本 → prerelease 的映射。除了 `latest`
    /// 之外的每个 dist-tag 都视为 prerelease 渠道；npm 本身只有一个
    /// `latest`，但维护者有时会发布 `next`、`beta`、`rc` 等。
    fn prerelease_versions(&self) -> std::collections::HashSet<String> {
        self.dist_tags
            .iter()
            .filter(|(tag, _)| tag.as_str() != "latest")
            .map(|(_, v)| v.clone())
            .collect()
    }
}

/// 从 npm registry 拉取 kernel 版本列表。
fn fetch_npm() -> Result<Vec<ReleaseInfo>, String> {
    // 与内核安装走同一条候选链：读到哪份版本记录，安装就尝试哪份
    // （成功取源会记进 `last_registry_base`，安装入口直接复用）。
    let (_, body) = http_get_registry_string(KERNEL_REGISTRY_KEY, DSH_NPM_PACKAGE, None)?;
    let pkg: NpmPackageDoc =
        serde_json::from_str(&body).map_err(|e: serde_json::Error| e.to_string())?;

    let prereleases = pkg.prerelease_versions();
    let times = pkg.time;
    let mut out: Vec<ReleaseInfo> = pkg
        .versions
        .into_iter()
        .filter(|(version, meta)| {
            // npm 有时会发布占位或被 yank 的条目；跳过它们，避免更新菜单
            // 推荐不可用的版本。
            // 版本号会变成目录名与 stub JSON 的一部分：registry 可被
            // `DSH_NPM_REGISTRY` 指到任意主机，被投毒时一个形状怪异的版本键
            // 就能越界写盘或注入 stub 字段。
            crate::shell::version::is_valid_kernel_version(version) && meta.deprecated.is_none()
        })
        .map(|(version, meta)| {
            let tag = format!("{TAG_PREFIX}{version}");
            let prerelease = prereleases.contains(&version) || version.contains('-');
            ReleaseInfo {
                tag: tag.clone(),
                version: version.clone(),
                prerelease,
                name: tag,
                published_at: times.get(&version).cloned().or(meta.date),
                html_url: format!("{NPM_PACKAGE_URL}/v/{version}"),
            }
        })
        .collect();
    out.sort_by(|a, b| cmp_versions(&a.version, &b.version).reverse());
    if out.is_empty() {
        return Err("npm registry 未返回任何 @deepseek-ai/dsh 版本".into());
    }
    Ok(out)
}

#[derive(serde::Deserialize)]
struct GhRelease {
    tag_name: String,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    html_url: Option<String>,
}

/// 从 GitHub API（兜底）拉取 `dsh-v*` 发布 tag。
fn fetch_api() -> Result<Vec<ReleaseInfo>, String> {
    let body = http_get_string(GITHUB_API_URL, Some("application/vnd.github+json"))?;
    let releases: Vec<GhRelease> =
        serde_json::from_str(&body).map_err(|e: serde_json::Error| e.to_string())?;
    let mut out: Vec<ReleaseInfo> = releases
        .into_iter()
        .filter_map(|r| {
            let mut info = release_from_tag(r.tag_name, r.prerelease)?;
            info.published_at = r.published_at;
            if let Some(url) = r.html_url {
                info.html_url = url;
            }
            Some(info)
        })
        .collect();
    out.sort_by(|a, b| cmp_versions(&a.version, &b.version).reverse());
    Ok(out)
}

/// 从发布版本的 Atom feed 中解析 `dsh-v*` 条目的标题。
///
/// feed 的形式大致是
/// `<entry><title>dsh-v0.1.1-rc.2</title>...</entry>`，但 feed 可以在
/// 标题里嵌入 HTML 实体（`&lt;`、`&gt;`、`&amp;`），并且正文开头还有一组
/// `<title>`/`<updated>` 头需要匹配。我们按字节下标切片，所以任何
/// 进入 `rest` 的偏移都必须基于原始缓冲区重新计算——如果在 `</title>`
/// 前后的两段切片共用同一个偏移（曾有版本这样实现），一旦标题中出现
/// 多字节 UTF-8 字符就会漂移，进而切断一个码点。
fn parse_atom(xml: &str) -> Vec<ReleaseInfo> {
    let mut out = Vec::new();
    let mut cursor = 0usize;
    while let Some(rel_start) = xml[cursor..].find("<title>") {
        let title_start = cursor + rel_start;
        let after_open = title_start + "<title>".len();
        let rel_end = xml[after_open..].find("</title>");
        let title_end = match rel_end {
            Some(r) => after_open + r,
            None => break,
        };
        let title = xml[after_open..title_end].trim();
        if let Some(info) = release_from_tag(title.to_string(), false) {
            out.push(info);
        }
        cursor = title_end + "</title>".len();
    }
    out.sort_by(|a, b| cmp_versions(&a.version, &b.version).reverse());
    out
}

/// 从 Atom feed（兜底）拉取 `dsh-v*` 发布 tag。
fn fetch_atom() -> Result<Vec<ReleaseInfo>, String> {
    let body = http_get_string(GITHUB_ATOM_URL, None)?;
    let out = parse_atom(&body);
    if out.is_empty() {
        return Err("Atom feed 未解析到 dsh-v* 标签".into());
    }
    Ok(out)
}

/// 列出发布版本的结果：数据本身加上任何兜底警告。
#[derive(Debug, Serialize, Clone)]
pub struct ReleaseList {
    pub releases: Vec<ReleaseInfo>,
    pub warning: Option<String>,
}

/// 交给前端的完整视图：发布列表 **加上** 本机能升级到哪一版。
///
/// **刻意不并进 [`ReleaseList`]**：`list_releases` 的返回值会进 60 秒进程缓存，
/// 而「可升级到哪一版」取决于本机已装了些什么。把按本机状态算出来的字段放进
/// 会被缓存的结构，缓存就会把上一次调用的答案端给下一个调用者——而那可能属于
/// 另一份已装集合。拆成两个类型，缓存里就只剩与本机无关的纯网络结果。
#[derive(Debug, Serialize, Clone)]
pub struct ReleaseOverview {
    pub releases: Vec<ReleaseInfo>,
    pub warning: Option<String>,
    /// 发布列表里比本机已装最新一版更新的那一版；已是最新则 `None`。
    pub upgrade: Option<String>,
}

/// 发布列表中比本机已装版本更新的那一版；没有更新的（或列表为空）则 `None`。
///
/// 基线取**已装最新**而不是活动版本：用户可能有意把活动内核停在某个旧版本上
/// （某个插件只在那一版验证过）。只要那版已经装好，就不该每次启动都催他升到
/// 自己明确不选的那一版。
///
/// 不依赖入参的排列顺序：各 `fetch_*` 末尾确实都按版本倒序排过，但判据不该
/// 建在「调用方拿到的这份恰好是最新的」上——多源合并、预发布剔除都可能打乱它。
/// 收 `&[String]` 而不是 `&[InstalledVersion]`，是为了让 `pkg/`（取源与出网）
/// 不必依赖 `kernel/`（内核安装树）这一侧的类型。
pub fn newest_upgrade(installed: &[String], releases: &[ReleaseInfo]) -> Option<String> {
    let newest_installed = installed.iter().max_by(|a, b| cmp_versions(a, b));
    releases
        .iter()
        .filter(|r| match newest_installed {
            Some(v) => cmp_versions(&r.version, v) == std::cmp::Ordering::Greater,
            None => true,
        })
        .max_by(|a, b| cmp_versions(&a.version, &b.version))
        .map(|r| r.version.clone())
}

/// 列出官方 kernel 发布版本，按从新到旧排序。
///
/// 数据源顺序：
/// 1. npm registry（`https://www.npmjs.com/package/@deepseek-ai/dsh`）——
///    用户在检查更新时看到的权威目的地。
/// 2. GitHub REST API——仅在 registry 不可达时使用。
/// 3. GitHub Atom feed——最后的兜底；不限速，但缺少 `prerelease` 标记。
///
/// 兜底时会设置 `warning`，UI 据此告知用户当前看到的是哪一份来源，
/// 并提示 npm 端的 prerelease 标记可能不完整。
/// 「检查更新」结果的进程级缓存：TTL 内重复调用直接复用。
///
/// `list_releases` 是纯网络操作，而首装引导会在启动时立刻再拉一次（引导流程
/// 自己调用一次 + 面板刷新调用一次），没有缓存就是两次完整的三级回退链
/// （P2-34）。只缓存成功结果：失败必须立刻可重试。
static RELEASES_CACHE: Mutex<Option<(Instant, ReleaseList)>> = Mutex::new(None);
const RELEASES_CACHE_TTL: Duration = Duration::from_secs(60);

fn cached_releases() -> Option<ReleaseList> {
    let guard = RELEASES_CACHE.lock().ok()?;
    let (stored_at, list) = guard.as_ref()?;
    if stored_at.elapsed() < RELEASES_CACHE_TTL {
        Some(list.clone())
    } else {
        None
    }
}

fn store_releases(list: &ReleaseList) {
    if let Ok(mut guard) = RELEASES_CACHE.lock() {
        *guard = Some((Instant::now(), list.clone()));
    }
}

#[cfg(test)]
static RELEASES_CACHE_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
fn clear_releases_cache() {
    if let Ok(mut guard) = RELEASES_CACHE.lock() {
        *guard = None;
    }
}

/// 一级源的**惰性**取源闭包：上一级命中时这一级不会被调用。
type Fetch = Box<dyn FnOnce() -> Result<Vec<ReleaseInfo>, String>>;

/// 回退链上的一级数据源。
struct Source {
    /// 在 warning / error 里的自称。
    name: &'static str,
    /// 「已回退到 …」后面的名字（括号里注明这一级的已知局限）。
    fallback: &'static str,
    /// 这一级返回空列表时给出的原因。
    empty_reason: &'static str,
    fetch: Fetch,
}

/// 把失败原因拼成「来源：原因；来源：原因」。
fn format_failures(failed: &[(&str, String)]) -> String {
    failed
        .iter()
        .map(|(name, reason)| format!("{name}：{reason}"))
        .collect::<Vec<_>>()
        .join("；")
}

/// 部分源失败时的回退警告：说明前面几级为什么被跳过、现在看到的是哪一级。
///
/// 只有一级失败时不重复写来源名——「npm registry 不可用（npm 超时）」已经
/// 说清楚是哪一级了，这条文案与旧实现逐字一致。
fn fallback_warning(failed: &[(&str, String)], winner: &str) -> String {
    if failed.len() == 1 {
        let (name, reason) = &failed[0];
        return format!("{name} 不可用（{reason}），已回退到 {winner}");
    }
    let skipped = failed
        .iter()
        .map(|(name, _)| *name)
        .collect::<Vec<_>>()
        .join(" 与 ");
    format!(
        "{skipped} 均不可用（{}），已回退到 {winner}",
        format_failures(failed)
    )
}

/// 全部源都失败：每条原因都要带上，用户和维护者才能判断是网络问题还是数据问题。
fn all_sources_failed(failed: &[(&str, String)]) -> AppError {
    AppError::GitHub(format!("全部源不可用 — {}", format_failures(failed)))
}

/// 三个数据源的优先级编排：npm registry → GitHub Releases API → GitHub Atom。
///
/// 顺序就是这张表的顺序：第一个给出非空结果的源胜出，它前面几级的失败原因
/// 汇总成**一条** warning（全失败时汇总成**一条** error）——旧实现把这套文案
/// 在两个分支里各写了一遍，改一处就会漏另一处。
///
/// 抽成接收三个**惰性**取源闭包的函数是为了能单元测试：源没有失败就不该被
/// 调用（否则一次成功的 npm 查询会白白多打两次网络），而三种失败组合各自
/// 应该产出什么 warning 也需要逐条钉住（P2-33）。
fn select_releases<F, G, H>(
    fetch_npm: F,
    fetch_api: G,
    fetch_atom: H,
) -> Result<ReleaseList, AppError>
where
    F: FnOnce() -> Result<Vec<ReleaseInfo>, String> + 'static,
    G: FnOnce() -> Result<Vec<ReleaseInfo>, String> + 'static,
    H: FnOnce() -> Result<Vec<ReleaseInfo>, String> + 'static,
{
    let sources = [
        Source {
            name: "npm registry",
            // 第一级不会是回退目标（只有排在它后面的源才可能胜出）。
            fallback: "npm registry",
            empty_reason: "npm registry 未返回任何 @deepseek-ai/dsh 版本",
            fetch: Box::new(fetch_npm),
        },
        Source {
            name: "GitHub API",
            fallback: "GitHub Releases API",
            empty_reason: "GitHub Releases API 返回空列表",
            fetch: Box::new(fetch_api),
        },
        Source {
            name: "GitHub Atom",
            fallback: "GitHub Atom feed（prerelease 标记可能不完整）",
            empty_reason: "Atom feed 未解析到 dsh-v* 标签",
            fetch: Box::new(fetch_atom),
        },
    ];

    // 空列表与"查询失败"等价：都说明这一级源这次给不出可用数据，应当继续回退。
    // 旧实现把空列表当成硬错误，于是镜像返回 200 但没有版本时用户直接看到报错，
    // 而 GitHub 回退其实完全可用（P2-33 顺带收口）。
    let mut failed: Vec<(&'static str, String)> = Vec::new();
    for source in sources {
        let reason = match (source.fetch)() {
            Ok(out) if !out.is_empty() => {
                let warning = if failed.is_empty() {
                    None
                } else {
                    Some(fallback_warning(&failed, source.fallback))
                };
                return Ok(ReleaseList {
                    releases: out,
                    warning,
                });
            }
            Ok(_) => source.empty_reason.to_string(),
            Err(error) => error,
        };
        failed.push((source.name, reason));
    }
    Err(all_sources_failed(&failed))
}

/// 带 TTL 缓存的取源：命中即复用，只缓存成功结果。
fn cached_or_fetch<F>(fetch: F) -> Result<ReleaseList, AppError>
where
    F: FnOnce() -> Result<ReleaseList, AppError>,
{
    if let Some(cached) = cached_releases() {
        return Ok(cached);
    }
    let result = fetch();
    if let Ok(list) = &result {
        store_releases(list);
    }
    result
}

/// 列出可安装的内核版本（npm 优先，GitHub 回退），带 60 秒缓存。
pub fn list_releases() -> Result<ReleaseList, AppError> {
    cached_or_fetch(|| select_releases(fetch_npm, fetch_api, fetch_atom))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// 起一个只回一次明文响应的本地 HTTP 服务器。
    fn serve_once(body: Vec<u8>) -> String {
        use std::io::Write;
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind local test server");
        let port = listener.local_addr().expect("test server address").port();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept local request");
            let mut head = [0u8; 1024];
            let _ = std::io::Read::read(&mut stream, &mut head);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.write_all(&body);
            let _ = stream.flush();
        });
        format!("http://127.0.0.1:{port}/")
    }

    /// 起一个只回指定 HTTP 状态码的本地服务器（空响应体）。用于探测的分类
    /// 用例：`500` 说明源**可达**，与连接失败是两回事。
    fn serve_status_once(status: u16) -> String {
        use std::io::Write;
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind local test server");
        let port = listener.local_addr().expect("test server address").port();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept local request");
            let mut head = [0u8; 1024];
            let _ = std::io::Read::read(&mut stream, &mut head);
            let response = format!(
                "HTTP/1.1 {status} ERROR\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        });
        format!("http://127.0.0.1:{port}/")
    }

    /// 起一个只回一次 302 重定向的本地 HTTP 服务器（Location 原样返回）。
    fn serve_redirect_once(location: &str) -> String {
        use std::io::Write;
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind local test server");
        let port = listener.local_addr().expect("test server address").port();
        let location = location.to_string();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept local request");
            let mut head = [0u8; 1024];
            let _ = std::io::Read::read(&mut stream, &mut head);
            let response = format!(
                "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        });
        format!("http://127.0.0.1:{port}/")
    }

    /// 拿一个当前没有监听者的回环地址：连接会被拒绝，属于**传输层不可达**。
    fn dead_registry() -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind dead port");
        let port = listener.local_addr().expect("dead port address").port();
        drop(listener);
        format!("http://127.0.0.1:{port}/")
    }

    /// **只有传输层失败才换源**：首选不可达时落到第二条，并返回真正响应的
    /// base——这个 base 会被内核安装拿去驱动 pnpm，返回错就等于列表与安装
    /// 又分叉了。
    #[test]
    fn registry_routing_falls_back_on_transport_failure() {
        let body = r#"{"ok":true}"#;
        let reachable = serve_once(body.as_bytes().to_vec());
        let bases = vec![dead_registry(), reachable.clone()];
        let (base, got) = http_get_registry_string_from("test-doc", None, &bases, true)
            .expect("首选源不可达时应换到第二条");
        assert_eq!(got, body);
        assert_eq!(base, reachable, "必须返回真正响应的那个 base");
    }

    /// 两个候选都不可达时，错误文本要列出**两个**源：否则用户看不出壳试过
    /// 镜像、只会以为「官方源挂了所以一定没救」。
    #[test]
    fn registry_routing_reports_every_unreachable_candidate() {
        let first = dead_registry();
        let second = dead_registry();
        let error =
            http_get_registry_string_from("test-doc", None, &[first.clone(), second.clone()], true)
                .expect_err("两条候选都不可达必须失败");
        assert!(error.contains(&first), "错误必须提到第一条候选：{error}");
        assert!(error.contains(&second), "错误必须提到第二条候选：{error}");
    }

    /// 分类必须只认传输层错误：`StatusCode` 说明源是可达的，换源不会得到
    /// 不同的答案。
    #[test]
    fn registry_unreachable_only_covers_transport_errors() {
        let refused = ureq::Error::Io(io::Error::new(
            io::ErrorKind::ConnectionRefused,
            "connection refused",
        ));
        assert!(is_transport_error(&refused));
        assert!(registry_unreachable(&HttpError::Ureq(refused)));
        assert!(
            !is_transport_error(&ureq::Error::StatusCode(500)),
            "源可达但返回 500 时换源毫无意义：另一个源只会同样失败"
        );
        assert!(
            !registry_unreachable(&HttpError::Policy("策略拒绝".into())),
            "策略拒绝不能换源：换一条源同样会拒绝"
        );
    }

    /// 探测的分类必须与取源一致：首选**不可达**才探测下一条，且返回真正
    /// 响应的 base（内核安装拿它去驱动 pnpm，返回错就等于列表与安装分叉）。
    #[test]
    fn probe_returns_the_reachable_candidate_after_transport_failure() {
        let reachable = serve_once(b"{}".to_vec());
        assert_eq!(
            probe_registry_base_from("test-doc", &[dead_registry(), reachable.clone()]),
            Some(reachable),
            "首选连接失败时应落到真正响应的那条候选"
        );
    }

    /// 首选**可达但返回 500** 时不得换源：换到镜像只会拿到一份逐包滞后的
    /// 目录，还会把一次瞬时 5xx 说成「官方 registry 不可达」（2026-10-28
    /// 评审的回归点）。
    #[test]
    fn probe_does_not_switch_on_reachable_http_error() {
        let reachable = serve_once(b"{}".to_vec());
        assert_eq!(
            probe_registry_base_from("test-doc", &[serve_status_once(500), reachable]),
            None,
            "源可达时探测必须返回 None（调用方回落首选源失败），不得指向镜像"
        );
    }

    /// 源记录按 key 隔离：插件检查更新写下镜像，不得改变内核安装读取的源。
    #[test]
    fn registry_records_are_scoped_by_key() {
        remember_registry_base("test-kernel-key", "https://kernel.example/");
        remember_registry_base("test-plugin-key", "https://mirror.example/");
        assert_eq!(
            last_registry_base("test-kernel-key").as_deref(),
            Some("https://kernel.example/"),
            "内核 key 只能读到自己写下的源"
        );
        assert_eq!(
            last_registry_base("test-plugin-key").as_deref(),
            Some("https://mirror.example/"),
            "插件 key 只影响自己"
        );
        assert_eq!(last_registry_base("test-missing-key"), None);
    }

    /// **已有成功取源记录时，安装必须直接复用，不得再探测**：列表读的是
    /// 镜像、安装却二次探测到官方源，就会重现「列表能列出、安装解析失败」
    /// 的跨源问题（2026-10-28 评审）。
    #[test]
    fn recorded_registry_wins_without_a_probe() {
        let chosen = choose_install_registry(
            Some(crate::pkg::registry::MIRROR_NPM_REGISTRY.to_string()),
            crate::pkg::registry::OFFICIAL_NPM_REGISTRY,
            || panic!("已有成功取源记录时不允许再探测"),
        );
        assert_eq!(chosen, crate::pkg::registry::MIRROR_NPM_REGISTRY);
    }

    /// 本进程还没成功取过源时才探测；探测命中的源就是安装要用的源。
    #[test]
    fn probe_is_used_only_without_a_recorded_base() {
        assert_eq!(
            choose_install_registry(None, crate::pkg::registry::OFFICIAL_NPM_REGISTRY, || Some(
                crate::pkg::registry::MIRROR_NPM_REGISTRY.to_string()
            )),
            crate::pkg::registry::MIRROR_NPM_REGISTRY
        );
    }

    /// 探测也没结果时回落首选源：探测是优化，不是安装闸门——不能让一次
    /// 探测失败把本来可能成功的安装拦在门外。
    #[test]
    fn preferred_registry_is_the_last_resort_when_probe_finds_nothing() {
        assert_eq!(
            choose_install_registry(None, crate::pkg::registry::OFFICIAL_NPM_REGISTRY, || None),
            crate::pkg::registry::OFFICIAL_NPM_REGISTRY
        );
    }

    // --- FIX-03：手动逐跳重定向 -------------------------------------------

    /// Location 的两种形态（相对 / 绝对）都要正确解析。
    #[test]
    fn redirect_locations_resolve_relative_and_absolute() {
        assert_eq!(
            resolve_location("https://r.example.com/a/b/c.tgz", "../d.tgz").unwrap(),
            "https://r.example.com/a/d.tgz",
            "相对 Location 按当前 URL 的目录解析"
        );
        assert_eq!(
            resolve_location("https://r.example.com/a/b", "//cdn.example.com/x").unwrap(),
            "https://cdn.example.com/x",
            "协议相对 Location 继承当前 scheme"
        );
        assert_eq!(
            resolve_location("https://r.example.com/a/b", "https://cdn.example.com/x").unwrap(),
            "https://cdn.example.com/x",
            "绝对 Location 原样使用"
        );
        assert_eq!(
            resolve_location("https://r.example.com/a/b", "?x=1").unwrap(),
            "https://r.example.com/a/b?x=1",
            "只带 query 的相对 Location 保留路径"
        );
        assert_eq!(
            resolve_location(
                "https://r.example.com/a/b",
                "https://cdn.example.com/x#frag"
            )
            .unwrap(),
            "https://cdn.example.com/x",
            "fragment 不参与请求目标"
        );
        assert!(
            resolve_location("https://r.example.com/a", "https://[bad").is_err(),
            "非法 Location 必须拒绝"
        );
    }

    /// 用手动跟随把真实本地夹具串起来：A 返回 302 → B 返回 body。
    /// 证明生产 transport 确实按 Location 走了第二跳。
    #[test]
    fn manual_redirect_following_reaches_the_allowed_target() {
        let body = b"followed".to_vec();
        let target = serve_once(body.clone());
        let base = serve_redirect_once(&format!("{target}payload"));
        let (used_base, got) =
            http_get_registry_string_from("doc", None, std::slice::from_ref(&base), true)
                .expect("允许的 https 跳转必须成功（夹具用 http 回环代跑）");
        assert_eq!(got, String::from_utf8(body).unwrap());
        assert_eq!(used_base, base, "返回的 base 必须是真正响应的那个源");
    }

    /// **被禁止的目标请求计数为 0**：假 transport 记录每一次真正发出去的
    /// 请求。策略拒绝发生在发请求之前，所以记录里只有第一跳。
    #[test]
    fn forbidden_redirect_target_never_receives_a_request() {
        let mut sent: Vec<String> = Vec::new();
        let mut responses: Vec<(u16, Option<String>, ())> = vec![
            (302, Some("https://evil.example.com/steal".to_string()), ()),
            // 若实现漏了逐跳校验/回退了自动跟随，这条会被取走并出现在 sent 里。
            (200, None, ()),
        ];
        let error = follow_redirects(
            "https://registry.example.com/pkg",
            Some(&["registry.example.com".to_string()]),
            false,
            |target| {
                sent.push(target.to_string());
                Ok(responses.remove(0))
            },
        )
        .expect_err("跳去未允许 host 必须拒绝");
        assert!(
            matches!(error, HttpError::Policy(_)),
            "应是策略拒绝：{error}"
        );
        assert_eq!(
            sent,
            vec!["https://registry.example.com/pkg".to_string()],
            "未允许的目标不得收到任何请求"
        );
    }

    /// HTTPS → HTTP、带 userinfo、缺 Location、非法 Location 都在发请求前拒绝。
    #[test]
    fn unsafe_redirect_targets_are_rejected_before_the_next_hop() {
        let cases = [
            ("http://registry.example.com/plain", "https"),
            ("https://user:pass@registry.example.com/x", "用户名或密码"),
            ("https://[bad", "无法解析"),
        ];
        for (location, expected) in cases {
            let mut sent: Vec<String> = Vec::new();
            let mut responses: Vec<(u16, Option<String>, ())> =
                vec![(302, Some(location.to_string()), ()), (200, None, ())];
            let error =
                follow_redirects("https://registry.example.com/pkg", None, false, |target| {
                    sent.push(target.to_string());
                    Ok(responses.remove(0))
                })
                .expect_err("危险重定向目标必须被拒绝");
            assert!(
                error.to_string().contains(expected),
                "{location} 的错误应提到 {expected}：{error}"
            );
            assert_eq!(sent.len(), 1, "{location} 被拒绝时不得再发请求");
        }

        // 302 却没有 Location：绝不猜一个目标继续。
        let mut sent: Vec<String> = Vec::new();
        let error = follow_redirects("https://registry.example.com/pkg", None, false, |target| {
            sent.push(target.to_string());
            Ok::<_, HttpError>((302, None, ()))
        })
        .expect_err("缺 Location 必须拒绝");
        assert!(error.to_string().contains("Location"), "{error}");
        assert_eq!(sent.len(), 1);
    }

    /// 相对 Location 在真实跟随循环里被解析成下一跳（不只是纯函数对）。
    #[test]
    fn relative_location_is_followed_as_resolved_absolute_url() {
        let mut sent: Vec<String> = Vec::new();
        let mut responses: Vec<(u16, Option<String>, ())> =
            vec![(302, Some("../other/pkg".to_string()), ()), (200, None, ())];
        follow_redirects(
            "https://registry.example.com/npm/a/pkg",
            None,
            false,
            |target| {
                sent.push(target.to_string());
                Ok(responses.remove(0))
            },
        )
        .expect("合法相对跳转必须跟随");
        assert_eq!(
            sent,
            vec![
                "https://registry.example.com/npm/a/pkg".to_string(),
                "https://registry.example.com/npm/other/pkg".to_string()
            ]
        );
    }

    /// 重定向环：在跳数上限处有界失败，不会无限请求。
    #[test]
    fn redirect_loop_stops_at_the_hop_limit() {
        let mut sent: Vec<String> = Vec::new();
        let error = follow_redirects("https://registry.example.com/loop", None, false, |target| {
            sent.push(target.to_string());
            Ok::<_, HttpError>((302, Some("/loop".to_string()), ()))
        })
        .expect_err("重定向环必须在跳数上限处失败");
        assert!(error.to_string().contains("跳上限"), "{error}");
        assert_eq!(
            sent.len(),
            MAX_REDIRECTS + 1,
            "首跳 + 最多 {MAX_REDIRECTS} 跳重定向，超限不再发请求"
        );
    }

    /// Atom 兜底路径只能从 tag 名推断预发布状态：版本号里出现 `-` 就必须标成
    /// 预发布，否则 `0.1.2-rc.19` 会被当成稳定版——列表不打预发布标签，首次
    /// 运行引导的「安装最新版本」还会优先选中它。
    fn sample_release(version: &str) -> ReleaseInfo {
        ReleaseInfo {
            tag: format!("dsh-v{version}"),
            version: version.to_string(),
            prerelease: false,
            name: format!("dsh-v{version}"),
            published_at: None,
            html_url: format!("https://example.invalid/dsh-v{version}"),
        }
    }

    fn sample_list(version: &str) -> ReleaseList {
        ReleaseList {
            releases: vec![sample_release(version)],
            warning: None,
        }
    }

    /// 按给定顺序构造发布列表（刻意不预先排序：判据不该依赖入参顺序）。
    fn releases_in_order(versions: &[&str]) -> Vec<ReleaseInfo> {
        versions.iter().map(|v| sample_release(v)).collect()
    }

    fn installed(versions: &[&str]) -> Vec<String> {
        versions.iter().map(|v| v.to_string()).collect()
    }

    #[test]
    fn upgrade_picks_the_newest_published_above_the_newest_installed() {
        let list = releases_in_order(&["0.2.0-rc.2", "0.1.7-rc.2"]);
        assert_eq!(
            newest_upgrade(&installed(&["0.1.7-rc.2"]), &list),
            Some("0.2.0-rc.2".to_string())
        );
    }

    #[test]
    fn no_upgrade_when_everything_published_is_already_installed() {
        let list = releases_in_order(&["0.2.0-rc.2", "0.1.7-rc.2"]);
        assert_eq!(
            newest_upgrade(&installed(&["0.1.7-rc.2", "0.2.0-rc.2"]), &list),
            None
        );
    }

    /// 基线取**已装最新**而不是活动版本：用户有意停在旧版时不该被反复催。
    #[test]
    fn baseline_is_the_newest_installed_not_the_active_one() {
        // 装着 rc.1 与 rc.2（rc.2 活动），npm 上只有 rc.2：没有可升的。
        let list = releases_in_order(&["0.2.0-rc.2"]);
        assert_eq!(
            newest_upgrade(&installed(&["0.2.0-rc.1", "0.2.0-rc.2"]), &list),
            None
        );
    }

    /// 预发布段必须参与比较：0.2.0-rc.1 高于 0.2.0-rc.0，0.2.0 正式版又高于
    /// 同一号的所有 rc。判据写错这一条会让每次启动都报「有新版本」。
    #[test]
    fn prerelease_ordering_is_respected() {
        let rc1 = releases_in_order(&["0.2.0-rc.1"]);
        assert_eq!(
            newest_upgrade(&installed(&["0.2.0-rc.0"]), &rc1),
            Some("0.2.0-rc.1".to_string())
        );
        let stable = releases_in_order(&["0.2.0"]);
        assert_eq!(
            newest_upgrade(&installed(&["0.2.0-rc.9"]), &stable),
            Some("0.2.0".to_string())
        );
        // 反向：正式版已在手，rc 不该再被说成「更新」。
        assert_eq!(
            newest_upgrade(&installed(&["0.2.0"]), &releases_in_order(&["0.2.0-rc.1"])),
            None
        );
    }

    /// 一切从简：没装任何内核时，列表里最新的那一版就是要装的那一版。
    #[test]
    fn empty_install_set_takes_the_newest_published() {
        let list = releases_in_order(&["0.1.7-rc.2", "0.2.0-rc.2", "0.2.0-rc.1"]);
        assert_eq!(newest_upgrade(&[], &list), Some("0.2.0-rc.2".to_string()));
    }

    #[test]
    fn empty_release_list_has_no_upgrade() {
        assert_eq!(newest_upgrade(&installed(&["0.1.0"]), &[]), None);
    }

    fn err(message: &str) -> Result<Vec<ReleaseInfo>, String> {
        Err(message.to_string())
    }

    fn ok(version: &str) -> Result<Vec<ReleaseInfo>, String> {
        Ok(vec![sample_release(version)])
    }

    /// 源没有失败就不该被调用：惰性闭包在这里直接 panic，谁被多调一次就会炸。
    fn forbidden() -> Result<Vec<ReleaseInfo>, String> {
        panic!("上一级源成功时不允许再打网络");
    }

    #[test]
    fn npm_wins_and_the_fallbacks_are_not_called() {
        let list = select_releases(|| ok("0.2.0"), forbidden, forbidden).expect("npm 命中");
        assert_eq!(list.releases[0].version, "0.2.0");
        assert!(list.warning.is_none(), "npm 成功时不该有回退警告");
    }

    #[test]
    fn empty_npm_result_falls_back_to_the_github_api() {
        // 空列表与失败等价：旧实现的 `Ok(_) => Err(...)` 分支就是这么处理的，
        // 这里把它钉住，避免以后有人把它当成功返回空的更新列表。
        let list = select_releases(|| Ok(Vec::new()), || ok("0.1.9"), forbidden)
            .expect("空 npm 结果应回退到 API");
        assert_eq!(list.releases[0].version, "0.1.9");
        assert!(
            list.warning
                .as_deref()
                .unwrap_or("")
                .contains("npm registry 未返回任何"),
            "警告要说明 npm 为什么被跳过：{:?}",
            list.warning
        );
    }

    #[test]
    fn api_failure_falls_back_to_the_atom_feed_with_a_warning() {
        let list = select_releases(|| err("npm 超时"), || err("api 502"), || ok("0.1.8"))
            .expect("三级回退应命中 Atom");
        let warning = list.warning.expect("回退到 Atom 必须带警告");
        assert!(
            warning.contains("npm 超时"),
            "警告要带上 npm 的原因：{warning}"
        );
        assert!(
            warning.contains("api 502"),
            "警告要带上 API 的原因：{warning}"
        );
        assert!(
            warning.contains("prerelease"),
            "Atom 的预发布标记不完整，必须写进警告：{warning}"
        );
    }

    #[test]
    fn all_sources_failing_reports_every_reason() {
        let error = select_releases(|| err("npm 超时"), || err("api 502"), || err("atom 403"))
            .expect_err("三个源都失败必须报错");
        let text = error.to_string();
        for reason in ["npm 超时", "api 502", "atom 403"] {
            assert!(
                text.contains(reason),
                "错误要带上每个源的原因（缺 {reason}）：{text}"
            );
        }
    }

    #[test]
    fn repeated_calls_within_the_ttl_reuse_the_cached_result() {
        // P2-34：首装引导会连着拉两次，没有缓存就是两次完整的三级回退链。
        // 缓存是进程级单例，所以这条用例与其它碰缓存的用例串行执行。
        let _guard = RELEASES_CACHE_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        clear_releases_cache();
        let calls = std::cell::Cell::new(0);
        let fetch = || {
            calls.set(calls.get() + 1);
            Ok(sample_list("0.3.0"))
        };

        let first = cached_or_fetch(fetch).expect("第一次");
        let second = cached_or_fetch(fetch).expect("第二次");
        assert_eq!(calls.get(), 1, "TTL 内的第二次调用必须复用缓存");
        assert_eq!(first.releases[0].version, second.releases[0].version);

        clear_releases_cache();
        let third = cached_or_fetch(fetch).expect("清缓存后");
        assert_eq!(calls.get(), 2, "显式清缓存后必须重新取源");
        assert_eq!(third.releases[0].version, "0.3.0");
    }

    #[test]
    fn failures_are_never_cached() {
        // 失败必须立刻可重试：用户点「检查更新」时网络刚恢复就该成功。
        let _guard = RELEASES_CACHE_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        clear_releases_cache();
        let calls = std::cell::Cell::new(0);
        let failing = || {
            calls.set(calls.get() + 1);
            Err::<ReleaseList, AppError>(AppError::GitHub("npm 不可用".into()))
        };

        assert!(cached_or_fetch(failing).is_err());
        assert!(cached_or_fetch(failing).is_err());
        assert_eq!(calls.get(), 2, "失败不允许进缓存");
    }

    #[test]
    fn atom_fallback_marks_dashed_versions_as_prerelease() {
        let stable = release_from_tag("dsh-v0.1.2".to_string(), false).expect("stable tag");
        assert!(!stable.prerelease);
        assert_eq!(stable.version, "0.1.2");

        let pre = release_from_tag("dsh-v0.1.2-rc.19".to_string(), false).expect("prerelease tag");
        assert!(pre.prerelease, "带 `-` 的版本号必须是预发布");

        // REST 路径已经带真实标志时保持原样。
        let flagged = release_from_tag("dsh-v0.1.2".to_string(), true).expect("flagged tag");
        assert!(flagged.prerelease);
    }

    /// SRI 校验必须真的比对内容。
    ///
    /// 外壳自己下载的 tarball 走的是与 packument 同一个第三方镜像：镜像可以在
    /// 元数据保持一致的前提下替换内容，而解包后 pnpm 会执行包里的 `prepare`
    /// 脚本。空文件的 sha512 是一个知名向量，用它同时验证 base64 解码与摘要比对。
    #[test]
    fn download_integrity_compares_content() {
        let root = std::env::temp_dir().join(format!(
            "dsh-sri-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&root).expect("create dir");
        let empty = root.join("empty.tgz");
        std::fs::write(&empty, b"").expect("write file");

        assert!(matches!(
            verify_download_integrity(
                &empty,
                Some("sha512-z4PhNX7vuL3xVChQ1m2AB9Yg5AULVxXcg/SpIdNs6c5H0NE8XYXysP+DGNKHfuwvY7kxvUdBeoGlODJ6+SfaPg=="),
                None
            ),
            Ok(Some("sha512"))
        ));
        // 多摘要（SRI 规范允许空格分隔，npm 的 ssri 会产出）：取最强且受支持的
        // 那一条校验，旧实现会被空格卡成"不是合法 base64"而拒绝安装（P2-6）。
        assert!(matches!(
            verify_download_integrity(
                &empty,
                Some(
                    "sha256-47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU= \
                     sha512-z4PhNX7vuL3xVChQ1m2AB9Yg5AULVxXcg/SpIdNs6c5H0NE8XYXysP+DGNKHfuwvY7kxvUdBeoGlODJ6+SfaPg=="
                ),
                None
            ),
            Ok(Some("sha512"))
        ));
        // 只有 sha256 时也能用。
        assert!(matches!(
            verify_download_integrity(
                &empty,
                Some("sha256-47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU="),
                None
            ),
            Ok(Some("sha256"))
        ));
        // 无法识别的算法 + 一条可用的：跳过前者而不是整串失败。
        assert!(matches!(
            verify_download_integrity(
                &empty,
                Some(
                    "sha3-AAAA sha512-z4PhNX7vuL3xVChQ1m2AB9Yg5AULVxXcg/SpIdNs6c5H0NE8XYXysP+DGNKHfuwvY7kxvUdBeoGlODJ6+SfaPg=="
                ),
                None
            ),
            Ok(Some("sha512"))
        ));

        let error = verify_download_integrity(&empty, Some("sha512-AAAA"), None)
            .expect_err("摘要不符必须被拒绝");
        assert!(error.contains("integrity"), "{error}");

        // 一条可用摘要、也没有 shasum 时必须显式拒绝，而不是放过（fail-closed）：
        // 空内容的 sha1 是 da39a3ee…，正好用来验证回退路径。
        let empty_sha1 = "da39a3ee5e6b4b0d3255bfef95601890afd80709";
        let error =
            verify_download_integrity(&empty, None, None).expect_err("两条摘要都缺必须拒绝安装");
        assert!(error.contains("dist.integrity"), "{error}");
        assert!(error.contains("dist.shasum"), "{error}");
        assert!(error.contains("拒绝安装"), "{error}");
        assert!(verify_download_integrity(&empty, Some("  "), Some("   ")).is_err());

        // 老 packument：没有 integrity，只有 sha1 的 shasum → 回退到它并标注算法。
        assert!(matches!(
            verify_download_integrity(&empty, None, Some(empty_sha1)),
            Ok(Some("sha1"))
        ));
        // 大小写与前后空白都要容忍（npm 上大小写不统一）。
        assert!(matches!(
            verify_download_integrity(
                &empty,
                None,
                Some(&format!("  {}  ", empty_sha1.to_uppercase()))
            ),
            Ok(Some("sha1"))
        ));
        let error = verify_download_integrity(&empty, None, Some("a".repeat(40).as_str()))
            .expect_err("shasum 不符必须拒绝");
        assert!(error.contains("shasum"), "{error}");

        // integrity 里只要有一条可用摘要就按它裁决：即使 shasum 是错的也不回退
        // （否则篡改者只要让强摘要对不上、弱摘要对得上就能通过）。
        assert!(matches!(
            verify_download_integrity(
                &empty,
                Some("sha256-47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU="),
                Some("a".repeat(40).as_str())
            ),
            Ok(Some("sha256"))
        ));
        // 反之：integrity 全是不认识的算法时，可用的 shasum 仍然兜住。
        assert!(matches!(
            verify_download_integrity(&empty, Some("md5-AAAA sha3-AAAA"), Some(empty_sha1)),
            Ok(Some("sha1"))
        ));
        // 摘要不符（多摘要里最强的那条不对）仍然拒绝，且不再退到 shasum。
        assert!(verify_download_integrity(
            &empty,
            Some("sha256-AAAA sha512-AAAA"),
            Some(empty_sha1)
        )
        .is_err());

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 形态非法的版本号在解析阶段就被丢掉（见 `version::is_valid_kernel_version`）。
    #[test]
    fn release_from_tag_rejects_non_semver_shapes() {
        assert!(release_from_tag("dsh-v1.0/hotfix".to_string(), false).is_none());
        assert!(release_from_tag("dsh-v..".to_string(), false).is_none());
    }

    /// 起一个按 `gap` 间隔逐块写 body 的本地服务器（明文，仅供测试）。
    fn serve_drip(chunks: usize, gap: std::time::Duration) -> String {
        use std::io::Write;
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind drip server");
        let port = listener.local_addr().expect("drip server address").port();
        std::thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut head = [0u8; 1024];
            let _ = std::io::Read::read(&mut stream, &mut head);
            let response =
                format!("HTTP/1.1 200 OK\r\nContent-Length: {chunks}\r\nConnection: close\r\n\r\n");
            if stream.write_all(response.as_bytes()).is_err() {
                return;
            }
            for _ in 0..chunks {
                if stream.write_all(b"x").is_err() || stream.flush().is_err() {
                    return;
                }
                std::thread::sleep(gap);
            }
        });
        format!("http://127.0.0.1:{port}/pkg.tgz")
    }

    /// 发完 `chunks` 块后静默 `silence`（Content-Length 比实际发送的多 1，
    /// 客户端会继续等）。
    fn serve_then_silence(chunks: usize, silence: std::time::Duration) -> String {
        use std::io::Write;
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind silent server");
        let port = listener.local_addr().expect("silent server address").port();
        std::thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut head = [0u8; 1024];
            let _ = std::io::Read::read(&mut stream, &mut head);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                chunks + 1
            );
            if stream.write_all(response.as_bytes()).is_err() {
                return;
            }
            for _ in 0..chunks {
                if stream.write_all(b"x").is_err() || stream.flush().is_err() {
                    return;
                }
            }
            std::thread::sleep(silence);
        });
        format!("http://127.0.0.1:{port}/pkg.tgz")
    }

    fn temp_download_path(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "xlink-download-{tag}-{:032x}.tgz",
            rand::random::<u128>()
        ))
    }

    /// 一条必然连不上的代理路由（端口刚被释放）：属于传输层失败。
    fn unreachable_proxy_route() -> Route {
        Route::Proxy {
            url: url::Url::parse(&dead_registry()).expect("dead proxy url"),
            source: "测试代理",
        }
    }

    /// 生产档位自检：下载总时限不能缩回 30s 的 API 时限（2026-11-09 的回退面）。
    /// 真实时钟回归见 `download_allows_a_body_slower_than_the_old_30s_global`
    /// （默认按 `#[ignore]` 跳过，改动这两档常量的关系时手工补跑）。
    #[test]
    fn download_limits_stay_clear_of_the_api_timeout() {
        assert!(DOWNLOAD_LIMITS.total > HTTP_TIMEOUT);
        assert!(DOWNLOAD_LIMITS.idle >= Duration::from_secs(60));
        assert_eq!(DOWNLOAD_LIMITS.connect, HTTP_TIMEOUT);
    }

    /// R03 同类回归：下载有 57.8 MB 的 tarball，持续有数据时不能被 30s 总时限
    /// 截断（2026-11-09 MiniMax 安装实测 `下载失败：timeout: global`）。
    /// 用生产默认值跑 31 秒：旧的 `HTTP_TIMEOUT` 全局时限会在第 30 秒报错。
    ///
    /// 31 秒真实时钟不进默认档（全量门禁只跑一次，没必要每次都等）；常量关系
    /// 由 `download_limits_stay_clear_of_the_api_timeout` 在快速门禁里守着。
    #[test]
    #[ignore = "31 秒真实时钟回归；完整补验：cargo test --locked --lib download_allows_a_body_slower_than_the_old_30s_global -- --ignored"]
    fn download_allows_a_body_slower_than_the_old_30s_global() {
        let url = serve_drip(32, Duration::from_secs(1));
        let dest = temp_download_path("slow");
        let started = Instant::now();
        http_get_file_with_options(&url, &dest, None, true, DOWNLOAD_LIMITS)
            .expect("持续有数据的下载不该被总时限截断");
        assert!(
            started.elapsed() > HTTP_TIMEOUT,
            "用例必须真的跨过旧的 30 秒线，实测 {:?}",
            started.elapsed()
        );
        assert_eq!(fs::read(&dest).unwrap(), vec![b'x'; 32]);
        let _ = fs::remove_file(&dest);
    }

    /// 下载空档超时要按断流处理，而不是一直等总时限。
    #[test]
    fn download_idle_timeout_ends_a_silent_stream() {
        let url = serve_then_silence(1, Duration::from_secs(2));
        let dest = temp_download_path("idle");
        let limits = DownloadLimits {
            connect: Duration::from_secs(5),
            idle: Duration::from_millis(200),
            total: Duration::from_secs(10),
        };
        let started = Instant::now();
        let error = http_get_file_with_options(&url, &dest, None, true, limits)
            .expect_err("静默超过空闲时限必须失败");
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "空闲时限不该退化成总时限：{error}"
        );
        let mut part = dest.clone().into_os_string();
        part.push(".part");
        assert!(
            !dest.exists() && !PathBuf::from(part).exists(),
            "失败要清掉未完成文件"
        );
    }

    /// 总时限仍要有：慢速滴数据不能把下载线程无限续命。
    #[test]
    fn download_total_cap_bounds_a_slow_drip() {
        let url = serve_drip(20, Duration::from_millis(100));
        let dest = temp_download_path("total");
        let limits = DownloadLimits {
            connect: Duration::from_secs(5),
            idle: Duration::from_secs(2),
            total: Duration::from_millis(400),
        };
        let started = Instant::now();
        let error = http_get_file_with_options(&url, &dest, None, true, limits)
            .expect_err("慢速滴数据必须在总时限到点时失败");
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "总时限没有生效：{error}"
        );
    }

    /// 路由顺序与分类（2026-11-10）：代理传输失败 → 换下一条；状态码 / 策略
    /// 拒绝不换（换了只会同样失败，还把第一个真正原因盖掉）。
    ///
    /// 不用真实代理夹具：ureq 对 `127.0.0.1` / `localhost` 目标会自动绕过代理
    /// （`Proxy::is_no_proxy`），回环用例根本走不到代理那条路。
    #[test]
    fn route_fallback_only_covers_transport_failures() {
        let routes = vec![unreachable_proxy_route(), Route::Direct];

        // 代理连不上：回退直连并成功。
        let mut tried = Vec::new();
        let value = try_routes(&routes, |route| {
            tried.push(route.describe());
            match route {
                Route::Proxy { .. } => Err(HttpError::Ureq(ureq::Error::Io(io::Error::new(
                    io::ErrorKind::ConnectionRefused,
                    "connection refused",
                )))),
                Route::Direct => Ok("direct"),
            }
        })
        .expect("传输层失败必须回退下一条路");
        assert_eq!(value, "direct");
        assert_eq!(tried.len(), 2);

        // 代理可达但返回 500：源可达，不换路。
        let mut tried = Vec::new();
        let error = try_routes(&routes, |route| -> Result<(), HttpError> {
            tried.push(route.describe());
            match route {
                Route::Proxy { .. } => Err(HttpError::Ureq(ureq::Error::StatusCode(500))),
                Route::Direct => panic!("状态码错误不该换路"),
            }
        })
        .expect_err("500 是源可达，不应回退");
        assert!(error.to_string().contains("500"), "{error}");
        assert_eq!(tried.len(), 1);
    }

    /// JSON 取源也走同一套路由注入入口：直连路由能读到响应，证明重构后的
    /// 请求/响应体路径完整。
    #[test]
    fn json_fetch_uses_the_injected_routes() {
        let url = serve_once(br#"{"ok":true}"#.to_vec());
        let body = http_get_string_on_routes(&[Route::Direct], &url, None, true)
            .expect("直连路由应读到响应");
        assert!(body.contains("\"ok\""));
    }

    /// 回环目标永远只走直连：本机代理不会替客户端访问 127.0.0.1。
    #[test]
    fn loopback_targets_never_take_the_proxy_route() {
        let routes = vec![unreachable_proxy_route(), Route::Direct];
        assert_eq!(
            routes_for_url(&routes, "http://127.0.0.1:4321/pkg.tgz"),
            vec![Route::Direct]
        );
        assert_eq!(
            routes_for_url(&routes, "https://registry.npmjs.org/pkg").len(),
            2
        );
    }
}
