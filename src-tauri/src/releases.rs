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
use std::sync::OnceLock;
use std::time::Duration;

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
            !version.is_empty()
                && meta.deprecated.is_none()
                && !version.contains(' ')
                && !version.contains('/')
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
#[derive(Serialize)]
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
pub fn list_releases() -> Result<ReleaseList, AppError> {
    match fetch_npm() {
        Ok(out) if !out.is_empty() => Ok(ReleaseList { releases: out, warning: None }),
        Ok(_) => Err(AppError::GitHub("npm registry 未返回任何 @deepseek-ai/dsh 版本".into())),
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
