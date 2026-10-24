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

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io;
use std::io::Read;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::shell::error::AppError;
use crate::shell::version::cmp_versions;

/// 官方发布版本上的 tag 前缀（例如 `dsh-v0.1.1-rc.2`）。
pub const TAG_PREFIX: &str = "dsh-v";

const USER_AGENT: &str = concat!("dsh-xlink/", env!("CARGO_PKG_VERSION"));
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_HTTP_BODY_BYTES: u64 = 10 * 1024 * 1024;
const MAX_TARBALL_BYTES: u64 = 256 * 1024 * 1024;

fn http_agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::Agent::config_builder()
            .timeout_global(Some(HTTP_TIMEOUT))
            .build()
            .new_agent()
    })
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

/// GET `url` 并把 body 作为文本返回。桌面端每次外发请求都带上桌面
/// User-Agent，并对响应体大小做了上限。
pub(crate) fn http_get_string(url: &str, accept: Option<&str>) -> Result<String, String> {
    let request = http_agent().get(url).header("User-Agent", USER_AGENT);
    let request = match accept {
        Some(value) => request.header("Accept", value),
        None => request,
    };
    let mut response = request.call().map_err(|e: ureq::Error| e.to_string())?;
    response
        .body_mut()
        .with_config()
        .limit(MAX_HTTP_BODY_BYTES)
        .read_to_string()
        .map_err(|e: ureq::Error| e.to_string())
}

/// GET `url` 并把 body 作为文本返回，**解压后**的字节数不超过 `max_bytes`。
///
/// ## 为什么需要这一条
///
/// [`http_get_string`] 的 10 MiB 上限是给**小 JSON**（npm registry 文档、
/// GitHub release 列表）准备的，社区插件目录是另一类东西：它是一份随站点
/// 增长而变大的全量清单（2026-10-24 实测 12712365 字节 / 21421 条），已经
/// 越过那条上限——两条取源路径（dshfind 与参考市场）**同时**越线，于是
/// 「目录获取失败」把整个插件中心打成空白，而 UI 那句「请检查网络或代理」
/// 把用户指向完全错误的方向（网络是好的，是我们的上限太小）。
///
/// ## 为什么用 gzip
///
/// 同一个端点在 `Accept-Encoding: gzip` 下只回 2583034 字节（实测），是
/// 明文的 1/5。压缩传输让「目录还在长大」这件事在带宽与耗时上都不再是问题。
///
/// **解压后的字节数仍然有上限**，而且这个上限是**解压之后**量的——这正是
/// ureq 自己的 `gzip` 特性做不到的：它的 `LimitReader` 位于解压器**内层**
/// （见 ureq 源码 `BodyReader`：`ContentDecoder<LimitReader<…>>`），所以开
/// 启 ureq 的 gzip 特性会让上限只约束**压缩后**的字节，一个 9.9 MiB 的 gzip
/// 炸弹能解出数十 GB 并触发 OOM abort（2026-09-10 的 P1-6 就是为此关掉
/// ureq gzip 特性的）。这里不用 ureq 的 gzip 特性，而是自己带
/// `Accept-Encoding: gzip` 头并用 flate2 解压，**上限加在解压后的输出上**：
/// 炸弹解不开，同时保留了压缩传输的收益。
///
/// ## 上限管两笔账，用同一个数
///
/// `max_bytes` 同时管**线上字节**与**解压后字节**。两笔账的原因是不同的
/// （一个是防无限流，一个是防 gzip 炸弹），但**上限必须是同一个数**——
///
/// · **明文路径也要能过**：服务端不压缩时线上就是明文（今天的目录是
///   12.7 MB）。若沿用通用取源那条 10 MiB 上限，「服务端哪天不压缩了」就会
///   让插件中心**再次**整体空白，而那天的原因还是同一个（体积），不是网络。
/// · **超限时只能说同一种话**：两条路径超限时都给带 [`BODY_TOO_LARGE_MARKER`]
///   的原因，调用方才能把「体积超限」与「网络故障」分开措辞。
///
/// ## 失败分类
///
/// 上限与网络故障是两类完全不同的原因，用户该做的事也不同（前者等目录
/// 变小或升级壳，后者查代理）。因此这里把「超过上限」单独识别出来，返回
/// 带 [`BODY_TOO_LARGE_MARKER`] 标记的原因文本，由调用方决定怎么措辞。
pub(crate) fn http_get_string_gzip(
    url: &str,
    accept: Option<&str>,
    max_bytes: u64,
) -> Result<String, String> {
    let request = http_agent()
        .get(url)
        .header("User-Agent", USER_AGENT)
        // 显式声明接受 gzip：ureq 自身关掉了 gzip 特性（见上文），它不会
        // 自动带上这个头，但服务端仍然会按我们声明的方式压缩。
        .header("Accept-Encoding", "gzip");
    let request = match accept {
        Some(value) => request.header("Accept", value),
        None => request,
    };
    let mut response = request.call().map_err(|e: ureq::Error| e.to_string())?;
    let is_gzip = response
        .headers()
        .get("content-encoding")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.eq_ignore_ascii_case("gzip"))
        .unwrap_or(false);

    // **线上字节**（压缩后的量）也要有上限，否则「一个自称 gzip 的无限流」能
    // 直接吃光内存。这一层与解压后的上限是**两笔账**，但**用同一个数**
    // `max_bytes`。
    //
    // 明文响应（服务端不压缩）走的就是这一层，目录明文 12.7 MB 必须能过——
    // 沿用通用取源那条 10 MiB 上限会把它挡在门外，于是「服务端哪天不压缩了」
    // 就会让插件中心**再次**整体空白，而那天的原因还是同一个（体积）。
    //
    // **不用 ureq 的 `limit`**：它超限时抛的是英文原文「response body is
    // larger than request limit: N」。那条文案随依赖版本变，而调用方
    // （[`crate::plugins::center::catalog_error`]）靠**标记**分类「体积超限」
    // 与「网络故障」——去匹配依赖的英文原文，依赖一升级，提示就悄悄退化成
    // 「请检查网络或代理设置」，而网络是好的。自己用 `take(max_bytes + 1)`
    // 读：多读到的那**一个字节**就是「超了」的证据，明文与 gzip 两条路径因此
    // 得到**同一条**带标记的原因。
    let mut raw = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(max_bytes.saturating_add(1))
        .read_to_end(&mut raw)
        .map_err(|e: io::Error| e.to_string())?;
    if raw.len() as u64 > max_bytes {
        return Err(body_too_large(max_bytes));
    }

    let plain = if is_gzip {
        let mut decoder = flate2::read::MultiGzDecoder::new(&raw[..]);
        // 上限加在**解压后的输出**上：这是与 ureq 内置 gzip 相反的关键差别。
        let mut out = Vec::new();
        decoder
            .by_ref()
            .take(max_bytes.saturating_add(1))
            .read_to_end(&mut out)
            .map_err(|e| format!("解压 gzip 响应失败：{e}"))?;
        if out.len() as u64 > max_bytes {
            return Err(body_too_large(max_bytes));
        }
        out
    } else {
        // 明文路径的长度上限刚才已经判过（`raw.len() > max_bytes` 已返回）。
        raw
    };

    String::from_utf8(plain).map_err(|e| format!("响应体不是合法的 UTF-8 文本：{e}"))
}

/// 「响应体超过上限」的原因文本里带的标记。调用方靠它把这一类失败与
/// 网络故障分开——两者的下一步完全不同，不能混成一句「请检查网络」。
pub(crate) const BODY_TOO_LARGE_MARKER: &str = "响应体超过上限";

/// 统一措辞：把字节上限换算成 MiB 报出来，并带上标记。
fn body_too_large(max_bytes: u64) -> String {
    format!(
        "{BODY_TOO_LARGE_MARKER}（允许 {} MiB）",
        max_bytes / (1024 * 1024)
    )
}

/// GET `url` 并把 body 流式写入 `destination`，不把整包 tarball 缓冲到
/// 内存中。复制成功后目标文件才视为完整。
pub(crate) fn http_get_file(url: &str, destination: &Path) -> Result<(), String> {
    let Some(file_name) = destination.file_name() else {
        return Err("下载目标不是文件路径".into());
    };
    let mut partial_name = file_name.to_os_string();
    partial_name.push(".part");
    let partial = destination.with_file_name(partial_name);
    let result = (|| {
        let mut response = http_agent()
            .get(url)
            .header("User-Agent", USER_AGENT)
            .call()
            .map_err(|e: ureq::Error| e.to_string())?;
        let mut reader = response
            .body_mut()
            .with_config()
            .limit(MAX_TARBALL_BYTES)
            .reader();
        let mut file = File::create(&partial).map_err(|e: io::Error| e.to_string())?;
        io::copy(&mut reader, &mut file).map_err(|e: io::Error| e.to_string())?;
        file.sync_all().map_err(|e: io::Error| e.to_string())?;
        fs::rename(&partial, destination).map_err(|e: io::Error| e.to_string())?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&partial);
    }
    result
}

/// 仅获取 npm 的 `latest` dist-tag，用于更新检查。包的元数据与
/// tarball 都使用同一个可配置的 registry 前缀。
pub(crate) fn http_get_npm_latest(package: &str) -> Result<Option<String>, String> {
    let encoded: String = url::form_urlencoded::byte_serialize(package.as_bytes()).collect();
    let url = format!(
        "{}-/package/{encoded}/dist-tags",
        crate::pkg::registry::npm_registry_base()
    );
    let body = http_get_string(&url, None)?;
    let tags: BTreeMap<String, String> =
        serde_json::from_str(&body).map_err(|e: serde_json::Error| e.to_string())?;
    Ok(tags.get("latest").cloned())
}

/// `@deepseek-ai/dsh` 包对应的 npm registry 端点。npm registry 返回
/// 的 JSON 文档包含所有已发布版本、`dist-tags`、脚本和元数据；
/// 这就是 `npm view` 读取的内容。通过 `registry::npm_registry_base()`
/// 解析，使外壳的镜像选择同样作用于更新菜单消费的发布列表。
fn npm_registry_url() -> String {
    format!(
        "{}@deepseek-ai/dsh",
        crate::pkg::registry::npm_registry_base()
    )
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
    let url = npm_registry_url();
    let body = http_get_string(&url, None)?;
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
            // 版本号会变成目录名与 stub JSON 的一部分：默认 registry 是第三方
            // 镜像，镜像被投毒时一个形状怪异的版本键就能越界写盘或注入 stub 字段。
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

    /// 起一个只回一次响应的本地 HTTP 服务器。
    ///
    /// `content_encoding` 是**服务端**的行为：只有当请求头里带了
    /// `Accept-Encoding: gzip` 时才回压缩体并声明 `Content-Encoding: gzip`，
    /// 否则回**明文**。这一点是判据的关键：服务器无视请求头、总是回 gzip
    /// 的话，「客户端有没有声明 gzip」这条就完全没被测到——反向验实测过：
    /// 去掉 `Accept-Encoding: gzip` 头时用例照样绿（真实服务端会改成回明文，
    /// 于是壳会拿到 12.7 MB 明文而不是 2.58 MB 压缩体，正是这次要修的那件事）。
    fn serve_once(body: Vec<u8>, gzip_body: Option<Vec<u8>>) -> String {
        use std::io::Write;
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind local test server");
        let port = listener.local_addr().expect("test server address").port();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept local request");
            let mut head = [0u8; 1024];
            let read = std::io::Read::read(&mut stream, &mut head).unwrap_or(0);
            let request = String::from_utf8_lossy(&head[..read]);
            // 真实服务端的行为：请求声明了 gzip 才压缩。
            let wants_gzip = request
                .to_ascii_lowercase()
                .contains("accept-encoding: gzip");
            let (payload, encoding) = match gzip_body {
                Some(compressed) if wants_gzip => (compressed, Some("gzip")),
                _ => (body, None),
            };
            let mut response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n",
                payload.len()
            );
            if let Some(encoding) = encoding {
                response.push_str(&format!("Content-Encoding: {encoding}\r\n"));
            }
            response.push_str("\r\n");
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.write_all(&payload);
            let _ = stream.flush();
        });
        format!("http://127.0.0.1:{port}/")
    }

    /// gzip 压缩一段内容。
    fn gzip(data: &[u8]) -> Vec<u8> {
        use flate2::write::GzEncoder;
        use flate2::Compression;
        let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
        std::io::Write::write_all(&mut encoder, data).expect("gzip write");
        encoder.finish().expect("gzip finish")
    }

    /// **客户端必须声明接受 gzip**，服务端才会给压缩体。
    ///
    /// 这条钉的是「压缩传输」这件事本身：ureq 自身关掉了 gzip 特性
    /// （见 `Cargo.toml` 的说明），所以**壳必须自己带 `Accept-Encoding: gzip`
    /// 头**，否则服务端会回 12.7 MB 明文而不是 2.58 MB 压缩体——目录还在
    /// 长大，那条明文路径迟早又会撑破上限。
    ///
    /// 反向验实测过：判据写成「服务器无视请求头总是回 gzip」时，去掉这个头
    /// 用例照样绿（判据没造出它要杀的行为）。现在服务器按请求头决定回压缩体
    /// 还是回明文，去掉头就会拿到明文 ⇒ 用例红。
    #[test]
    fn the_client_asks_for_gzip_so_the_server_compresses() {
        let payload = "目录正文";
        let url = serve_once(payload.as_bytes().to_vec(), Some(gzip(payload.as_bytes())));
        let body = http_get_string_gzip(&url, None, 1024 * 1024).expect("应拿到压缩体并解码");
        assert_eq!(body, payload);
    }

    /// **gzip 响应必须被解压**：服务端声明了 gzip 时，壳拿到的是文本而不是
    /// 二进制。这正是目录取源能扛住 12.7 MB 的原因（压缩后只有 2.58 MB）。
    ///
    /// 夹具的明文与压缩体**内容不同**（压缩体里放的是另一个字符串），
    /// 这样「拿到的是压缩体」与「拿到的是明文」两种结果可区分：
    /// 反向验实测过，第一版夹具两边内容相同，于是去掉 `Accept-Encoding`
    /// 头（服务端改回明文）时断言照样成立 ⇒ 判据没造出它要杀的行为。
    #[test]
    fn gzip_response_is_decoded_into_text() {
        let plain = "服务端回明文时的内容";
        let compressed_plain = "服务端回 gzip 时解出来的内容";
        let url = serve_once(
            plain.as_bytes().to_vec(),
            Some(gzip(compressed_plain.as_bytes())),
        );
        let body = http_get_string_gzip(&url, None, 1024 * 1024).expect("gzip 响应应解码成功");
        assert_eq!(
            body, compressed_plain,
            "必须拿到**解压后**的内容，而不是压缩体的二进制，也不是服务端回退成的明文"
        );
    }

    /// **未压缩的响应照常读**：服务端不压缩时不能因此失败。
    #[test]
    fn plain_response_is_read_as_is() {
        let url = serve_once(b"plain body".to_vec(), None);
        let body = http_get_string_gzip(&url, None, 1024 * 1024).expect("普通响应应读取成功");
        assert_eq!(body, "plain body");
    }

    /// **上限加在解压之后**：一个压缩体很小、解压后却超过上限的响应必须被拒。
    ///
    /// 这是本条修复的核心判据，也是 ureq 内置 gzip 特性做不到的那一半
    /// （它的 `LimitReader` 在解压器**内层**，只约束压缩后的字节，gzip 炸弹
    /// 能解出数十 GB）。2026-10-24 的故障是「目录涨到 12.7 MB 越过了 10 MiB
    /// 上限」，所以「上限必须作用在解压后的量上」正是这里要钉住的行为。
    #[test]
    fn the_limit_applies_to_the_decompressed_size() {
        // 100 KB 的重复内容压到几百字节：压缩体远小于上限，解压后远超。
        let payload = "x".repeat(100 * 1024);
        let compressed = gzip(payload.as_bytes());
        assert!(
            compressed.len() < 4096,
            "夹具必须真的压缩过（压缩后 {} 字节）",
            compressed.len()
        );
        let url = serve_once(payload.clone().into_bytes(), Some(compressed));
        let error = http_get_string_gzip(&url, None, 1024).expect_err("解压后超过上限必须失败");
        assert!(
            error.contains(BODY_TOO_LARGE_MARKER),
            "错误必须标明是体积超限（这样前端才能说「网络是正常的」），实际：{error}"
        );
    }

    /// **压缩体本身也要有上限**：一个自称 gzip 的无限流不能直接吃光内存。
    /// 这一层拦的是**线上字节**，与解压后的上限是两笔账（但用同一个数）。
    ///
    /// 夹具用**不可压缩**的字节（伪随机数）：若用重复字符，gzip 之后会缩到
    /// 几千字节，于是「压缩体超上限」这条路径根本没被走到——那正是反向验
    /// 实测出来的问题（第一版夹具用重复的 `y`，10 MiB 明文压后只剩 10 KB，
    /// 于是这条用例测的是另一条路径，断言与夹具对不上）。伪随机数据压不动，
    /// 压缩体才真的越线。
    ///
    /// 断言只看**自己的标记**：早先这里也接受 ureq 的英文原文（"response body
    /// is larger than request limit"），而调用方靠标记分类「体积超限」与
    /// 「网络故障」——去匹配依赖的英文原文，依赖一换版本，提示就悄悄退化成
    /// 「请检查网络」，而网络是好的。
    #[test]
    fn an_oversized_compressed_body_is_rejected_too() {
        // 伪随机字节：gzip 压不动，压缩体与明文同量级。
        let limit = 1024 * 1024u64;
        let mut state = 0x2545_f491_4f6c_dd1du64;
        let mut raw = Vec::with_capacity(limit as usize + 4096);
        while raw.len() < limit as usize + 4096 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            raw.extend_from_slice(&state.to_le_bytes());
        }
        let compressed = gzip(&raw);
        assert!(
            compressed.len() as u64 > limit,
            "夹具必须压不动（压缩后 {} 字节 vs 上限 {}）",
            compressed.len(),
            limit
        );
        let url = serve_once(raw, Some(compressed));
        let error = http_get_string_gzip(&url, None, limit).expect_err("压缩体超过上限必须失败");
        assert!(
            error.contains(BODY_TOO_LARGE_MARKER),
            "错误必须带自己的标记（调用方靠它说「网络是正常的」），实际：{error}"
        );
    }

    /// **明文响应不能沿用通用取源那条 10 MiB 上限**：服务端不压缩时，线上
    /// 就是明文（今天的目录是 12.7 MB）。
    ///
    /// 这条钉的是「服务端哪天不压缩了」：**旧实现把线上字节交给 ureq 的
    /// `limit(MAX_HTTP_BODY_BYTES)`（10 MiB）**，明文目录于是在 10 MiB 处被
    /// 挡下，报的还是 ureq 的英文原文——分类器把它当成「网络故障」，于是
    /// 插件中心再次整体空白，提示还说「请检查网络」。
    ///
    /// 反向验实测过：把线上上限改回 10 MiB，本用例红（拿到 `Err`）。
    #[test]
    fn a_plain_body_larger_than_the_generic_limit_is_still_accepted() {
        // 11 MiB：在通用那条 10 MiB 之上，在目录自己的上限之下。
        let payload = vec![b'x'; (MAX_HTTP_BODY_BYTES + 1024 * 1024) as usize];
        let url = serve_once(payload.clone(), None);
        let body = http_get_string_gzip(&url, None, 48 * 1024 * 1024)
            .expect("明文响应超过通用那条 10 MiB 上限也必须能取回");
        assert_eq!(body.len(), payload.len());
    }

    /// **明文响应超上限时也说「体积超限」**：不能把依赖的英文原文透出去。
    ///
    /// 调用方（`catalog_error`）靠标记把「体积超限」与「网络故障」分开措辞；
    /// 早先明文路径超限时抛的是 ureq 的「response body is larger than request
    /// limit」，标记不在里面 ⇒ 前端说「请检查网络或代理设置」，而网络是好的。
    #[test]
    fn an_oversized_plain_body_is_reported_as_too_large() {
        let payload = vec![b'x'; 2 * 1024 * 1024];
        let url = serve_once(payload, None);
        let error =
            http_get_string_gzip(&url, None, 1024 * 1024).expect_err("明文响应超过上限必须失败");
        assert!(
            error.contains(BODY_TOO_LARGE_MARKER),
            "错误必须带自己的标记（这样前端才能说「网络是正常的」），实际：{error}"
        );
        assert!(
            !error.contains("response body is larger than request limit"),
            "不能把依赖的英文原文透出去（它会随依赖版本变），实际：{error}"
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
}
