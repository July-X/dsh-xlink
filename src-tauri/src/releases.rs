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
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::version::cmp_versions;

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

/// 校验一个已下载文件的 npm SRI 摘要（`sha512-<base64>` / `sha256-<base64>`）。
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
/// 返回 `Ok(Some(算法名))` 表示已校验，`Ok(None)` 表示 registry 没给摘要（老
/// packument 只给 sha1 的 `shasum`，本函数不消费它），由调用方决定如何提示。
pub fn verify_download_integrity(
    path: &Path,
    integrity: Option<&str>,
) -> Result<Option<&'static str>, String> {
    use sha2::{Digest, Sha256, Sha512};

    let Some(integrity) = integrity.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    // 只挑"最强且受支持"的那一条：sha512 > sha256。
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
    let Some((algorithm, expected)) = best else {
        return Err(format!(
            "registry 返回的 integrity 里没有本外壳支持的摘要（sha512/sha256）：{integrity}，拒绝安装"
        ));
    };
    let bytes = fs::read(path).map_err(|e| format!("无法读取 {}：{e}", path.display()))?;
    let actual = match algorithm {
        "sha512" => Sha512::digest(&bytes).to_vec(),
        _ => Sha256::digest(&bytes).to_vec(),
    };
    if actual != expected {
        return Err(format!(
            "下载内容与 registry 声明的 integrity 不符（算法 {algorithm}）：文件可能被镜像替换或传输损坏"
        ));
    }
    Ok(Some(algorithm))
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
        crate::registry::npm_registry_base()
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
    format!("{}@deepseek-ai/dsh", crate::registry::npm_registry_base())
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
    if !crate::version::is_valid_kernel_version(&version) {
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
            crate::version::is_valid_kernel_version(version) && meta.deprecated.is_none()
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

/// 三个数据源的优先级编排：npm registry → GitHub Releases API → GitHub Atom。
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
    F: FnOnce() -> Result<Vec<ReleaseInfo>, String>,
    G: FnOnce() -> Result<Vec<ReleaseInfo>, String>,
    H: FnOnce() -> Result<Vec<ReleaseInfo>, String>,
{
    // 空列表与"查询失败"等价：都说明这一级源这次给不出可用数据，应当继续回退。
    // 旧实现把空列表当成硬错误，于是镜像返回 200 但没有版本时用户直接看到报错，
    // 而 GitHub 回退其实完全可用（P2-33 顺带收口）。
    let npm_source = fetch_npm().and_then(|out| {
        if out.is_empty() {
            Err("npm registry 未返回任何 @deepseek-ai/dsh 版本".to_string())
        } else {
            Ok(out)
        }
    });
    match npm_source {
        Ok(out) => Ok(ReleaseList { releases: out, warning: None }),
        Err(npm_err) => match fetch_api() {
            Ok(out) if !out.is_empty() => Ok(ReleaseList {
                releases: out,
                warning: Some(format!(
                    "npm registry 不可用（{npm_err}），已回退到 GitHub Releases API"
                )),
            }),
            Ok(_) => {
                let api_err = "GitHub Releases API 返回空列表".to_string();
                match fetch_atom() {
                    Ok(out) => Ok(ReleaseList {
                        releases: out,
                        warning: Some(format!(
                            "npm registry 与 GitHub API 均不可用（npm：{npm_err}；api：{api_err}），已回退到 GitHub Atom feed（prerelease 标记可能不完整）"
                        )),
                    }),
                    Err(atom_err) => Err(AppError::GitHub(format!(
                        "全部源不可用 — npm：{npm_err}；GitHub API：{api_err}；GitHub Atom：{atom_err}"
                    ))),
                }
            }
            Err(api_err) => match fetch_atom() {
                Ok(out) => Ok(ReleaseList {
                    releases: out,
                    warning: Some(format!(
                        "npm registry 与 GitHub API 均不可用（npm：{npm_err}；api：{api_err}），已回退到 GitHub Atom feed（prerelease 标记可能不完整）"
                    )),
                }),
                Err(atom_err) => Err(AppError::GitHub(format!(
                    "全部源不可用 — npm：{npm_err}；GitHub API：{api_err}；GitHub Atom：{atom_err}"
                ))),
            },
        },
    }
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
                Some("sha512-z4PhNX7vuL3xVChQ1m2AB9Yg5AULVxXcg/SpIdNs6c5H0NE8XYXysP+DGNKHfuwvY7kxvUdBeoGlODJ6+SfaPg==")
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
                )
            ),
            Ok(Some("sha512"))
        ));
        // 只有 sha256 时也能用。
        assert!(matches!(
            verify_download_integrity(
                &empty,
                Some("sha256-47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU=")
            ),
            Ok(Some("sha256"))
        ));
        // 无法识别的算法 + 一条可用的：跳过前者而不是整串失败。
        assert!(matches!(
            verify_download_integrity(
                &empty,
                Some(
                    "sha3-AAAA sha512-z4PhNX7vuL3xVChQ1m2AB9Yg5AULVxXcg/SpIdNs6c5H0NE8XYXysP+DGNKHfuwvY7kxvUdBeoGlODJ6+SfaPg=="
                )
            ),
            Ok(Some("sha512"))
        ));

        let error =
            verify_download_integrity(&empty, Some("sha512-AAAA")).expect_err("摘要不符必须被拒绝");
        assert!(error.contains("integrity"), "{error}");

        // 老 packument 只有 sha1 的 shasum、没有 integrity：跳过而不是误判。
        assert!(matches!(verify_download_integrity(&empty, None), Ok(None)));
        assert!(matches!(
            verify_download_integrity(&empty, Some("  ")),
            Ok(None)
        ));

        // 一条可用摘要都没有时必须显式拒绝，而不是放过。
        assert!(verify_download_integrity(&empty, Some("md5-AAAA")).is_err());
        assert!(verify_download_integrity(&empty, Some("sha3-AAAA")).is_err());
        // 摘要不符（多摘要里最强的那条不对）仍然拒绝。
        assert!(verify_download_integrity(&empty, Some("sha256-AAAA sha512-AAAA")).is_err());

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 形态非法的版本号在解析阶段就被丢掉（见 `version::is_valid_kernel_version`）。
    #[test]
    fn release_from_tag_rejects_non_semver_shapes() {
        assert!(release_from_tag("dsh-v1.0/hotfix".to_string(), false).is_none());
        assert!(release_from_tag("dsh-v..".to_string(), false).is_none());
    }
}
