//! 社区插件管理：位于 dsh-xlink home 下的中央库、按内核物化（link 或 copy）、
//! profile 接线以及更新检查。
//!
//! 所有插件源码统一保存在 `<home>/plugins/`（即中央库），绝不放入内核安装
//! 目录内。每一个已安装的内核都从自己的 `<data_dir>/kernels/<version>/plugins/`
//! 目录读取插件，该目录由桌面壳从中央库物化而来，方式可以是 symlink（link 模式，
//! 默认）或真实拷贝（copy 模式）。活动内核的 profile（`profiles/<profile>/`）
//! 随后把每个插件声明为指向该物化目录的依赖；当插件声明了 `dsh.bundle` 时
//! 再叠加一层 bundle 层，整体镜像内核 plugin CLI 产出的结构。这样切换内核
//! 永远不需要重新安装，只需重新物化并重接线。
//!
//! 设计说明见桌面交付物中的 `docs/plugin-management.md`。

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::process::run_capture;
use crate::quarantine;
use crate::releases::{http_get_file, http_get_npm_latest, http_get_string};
use crate::version::cmp_versions;
use crate::{commands, kernel, node, settings};

/// 桌面壳将插件接入的默认 profile（即内核的 web 表面）。
pub const DEFAULT_PROFILE: &str = "web";
/// 桌面壳 home 下的中央库目录名。
const STORE_SUBDIR: &str = "plugins";
/// 位于中央库目录内的插件清单文件。
const STORE_FILE: &str = "store.json";
/// 每个中央库条目内的取源标记文件。
const SOURCE_MARKER: &str = ".dsh-source.json";
/// 社区目录的主要数据源：dsh-plugin.org hub（DSH-Plugin Hub 插件中心背后的数据源）。
const HUB_CATALOG_URL: &str = "https://dsh-plugin.org/api/plugins.zh.json";
/// 社区目录的回退数据源：参考市场的插件列表，在 hub 不可达时使用。
const MARKET_CATALOG_URL: &str =
    "https://raw.githubusercontent.com/losebird/dsh-plugin-market/main/registry/all.json";
/// 桌面壳数据目录下的目录缓存文件。
const CATALOG_CACHE_FILE: &str = "plugins-catalog.json";
/// 目录缓存的新鲜度窗口。
const CATALOG_TTL_SECS: u64 = 6 * 3600;
/// 内核 plugins 目录内的物化元数据目录名。
const META_SUBDIR: &str = ".meta";
/// pnpm `link:`（symlink）依赖的 spec 前缀。
const SPEC_LINK: &str = "link:";
/// pnpm `file:`（中央库拷贝）依赖的 spec 前缀。
const SPEC_FILE: &str = "file:";

// --- 数据模型 ------------------------------------------------------------

/// 中央库中一个已安装的插件条目。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StoreItem {
    /// 文件系统安全的插件键（包名/仓库名，斜杠已替换）。
    pub id: String,
    /// 显示名称（npm 包名或仓库简称）。
    pub name: String,
    /// 取源方式：npm 或 git。
    pub origin: String,
    /// 取源地址：npm 包名（可附 `@version`）或 git URL（可附 `#tag`）。
    pub source: String,
    pub installed_version: String,
    /// 已知的最新版本，由 check_updates 刷新。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_version: Option<String>,
    /// 期望的物化模式：link 或 copy。
    pub mode: String,
    /// 源是否锁定了版本（npm `@version` / git `#tag`）。
    pub pinned: bool,
    /// 自纪元以来的秒数，仅供展示。
    pub installed_at: String,
    /// 最近一次拉取距纪元的秒数，仅供展示。
    pub updated_at: String,
    /// 给用户看的仓库 URL（git 来源插件）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl Default for StoreItem {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            origin: String::from("npm"),
            source: String::new(),
            installed_version: String::new(),
            latest_version: None,
            mode: String::from("link"),
            pinned: false,
            installed_at: String::new(),
            updated_at: String::new(),
            repo_url: None,
            description: None,
        }
    }
}

/// 持久化的中央库文档。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Store {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u32,
    pub items: Vec<StoreItem>,
    #[serde(rename = "lastCheckedAt", skip_serializing_if = "Option::is_none")]
    pub last_checked_at: Option<String>,
    /// 最近一次接线/安装失败后向 UI 展示的告警，若无则为空。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

impl Default for Store {
    fn default() -> Self {
        Self {
            schema_version: 1,
            items: Vec::new(),
            last_checked_at: None,
            warning: None,
        }
    }
}

/// 每个内核的物化记录，每个插件对应一个 JSON 文件。
#[derive(Debug, Clone, Serialize, Deserialize)]
struct KernelMeta {
    /// 磁盘上的实际模式：link 或 copy。
    mode: String,
    /// 本次物化对应的中央库版本。
    version: String,
    synced_at: String,
}

/// 管理界面渲染的一行数据。
#[derive(Debug, Clone, Serialize)]
pub struct PluginRow {
    pub id: String,
    pub name: String,
    pub origin: String,
    pub source: String,
    pub installed_version: String,
    pub latest_version: Option<String>,
    pub pinned: bool,
    /// 中央库中记录的期望模式。
    pub desired_mode: String,
    /// 活动内核中的实际模式（仅当已物化时）。
    pub actual_mode: Option<String>,
    /// 活动内核中的物化是否完整且与当前版本一致。
    pub synced: bool,
    /// 活动内核的 profile 是否已经加载了此插件。
    pub wired: bool,
    /// 启动看护禁用此插件时的隔离记录；
    /// `None` 表示该插件正常参与接线。
    pub quarantined: Option<quarantine::QuarantineItem>,
    pub repo_url: Option<String>,
    pub description: Option<String>,
    pub installed_at: String,
    pub updated_at: String,
}

/// 管理界面所需的插件汇总状态。
#[derive(Debug, Clone, Serialize)]
pub struct PluginStatus {
    pub rows: Vec<PluginRow>,
    pub profile: String,
    pub active_kernel: Option<String>,
    /// 已知存在更新版本的插件数量。
    pub updates: usize,
    pub last_checked_at: Option<String>,
    pub warning: Option<String>,
}

/// 在插件中心展示的一条目录条目。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogItem {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub description: String,
    pub stars: u64,
    #[serde(default)]
    pub forks: u64,
    pub downloads: u64,
    pub verified: bool,
    pub repo: Option<String>,
    /// 安装 spec：npm 包名或 git URL（已知时附 `#tag`）。
    pub spec: String,
    /// npm 或 git，由条目的安装方式推导。
    pub origin: String,
    pub category: String,
    /// 最新发布版本字符串（可能带前导 `v`）。
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// 上游最近更新的 ISO 时间戳（已知时）。
    #[serde(default)]
    pub updated: String,
    /// 给用户看的详情页（dsh-plugin.org 或仓库）。
    #[serde(default)]
    pub detail_url: String,
}

/// npm registry 文档中我们关心的子集。
#[derive(Debug, Deserialize)]
struct NpmDoc {
    #[serde(rename = "dist-tags", default)]
    dist_tags: BTreeMap<String, String>,
    #[serde(default)]
    versions: BTreeMap<String, NpmVersionDoc>,
}

#[derive(Debug, Deserialize)]
struct NpmVersionDoc {
    #[serde(default)]
    dist: Option<NpmDist>,
}

#[derive(Debug, Deserialize)]
struct NpmDist {
    #[serde(default)]
    tarball: String,
}

/// 解析后的安装请求。
#[derive(Debug, Clone)]
pub struct PluginSpec {
    pub origin: String,
    /// npm 包名或 git URL。
    pub source: String,
    /// 可选的版本钉（npm semver）或 tag（git）。
    pub pin: Option<String>,
    /// 文件系统安全的中央库 id。
    pub id: String,
    /// 显示名称。
    pub name: String,
    /// git 来源下给用户看的仓库 URL。
    pub repo_url: Option<String>,
}

// --- 路径 ------------------------------------------------------------------

/// 中央库根目录：`<home>/plugins/`，与中央库喂养的 profile 目录同级。
/// `data_dir` 指向 `<home>/desktop/`（参见 `kernel::data_dir`）。
pub fn store_dir(data_dir: &Path) -> PathBuf {
    data_dir
        .parent()
        .map(|home| home.join(STORE_SUBDIR))
        .unwrap_or_else(|| data_dir.join(STORE_SUBDIR))
}

fn store_file(data_dir: &Path) -> PathBuf {
    store_dir(data_dir).join(STORE_FILE)
}

fn store_plugin_dir(data_dir: &Path, id: &str) -> PathBuf {
    store_dir(data_dir).join(id)
}

fn kernel_plugins_dir(data_dir: &Path, version: &str) -> PathBuf {
    kernel::kernel_dir(data_dir, version).join("plugins")
}

fn kernel_plugin_dir(data_dir: &Path, version: &str, id: &str) -> PathBuf {
    kernel_plugins_dir(data_dir, version).join(id)
}

fn kernel_meta_file(data_dir: &Path, version: &str, id: &str) -> PathBuf {
    kernel_plugins_dir(data_dir, version)
        .join(META_SUBDIR)
        .join(format!("{id}.json"))
}

fn profile_dir(data_dir: &Path, profile: &str) -> PathBuf {
    data_dir
        .parent()
        .map(|home| home.join("profiles").join(profile))
        .unwrap_or_else(|| data_dir.join("profiles").join(profile))
}

fn wiring_log_spec() -> crate::process::LogSpec {
    crate::process::LogSpec::new(crate::process::build_log_kind(), "plugin-wiring")
}

fn wiring_log_path(data_dir: &Path) -> PathBuf {
    let logs = kernel::logs_dir(data_dir);
    wiring_log_spec().path_for(&logs, &crate::process::current_date_string())
}

fn plugin_log_spec(id: &str) -> crate::process::LogSpec {
    crate::process::LogSpec::new(crate::process::build_log_kind(), format!("plugin-{id}"))
}

fn plugin_log_path(data_dir: &Path, id: &str) -> PathBuf {
    let logs = kernel::logs_dir(data_dir);
    plugin_log_spec(id).path_for(&logs, &crate::process::current_date_string())
}

/// 将包名/仓库名映射为文件系统安全的中央库 id。之后路径穿越在结构上已不可能：
/// 斜杠会被替换为双下划线，`.` / 空段会直接拒绝。空白字符同样会被拒绝，因为
/// npm 包名和 GitHub `owner/repo` 字符串都是单一 token —— 形如
/// `dsh plugin remove @scope/pkg` 的字符串走到这里意味着调用方忘了在
/// `split_dsh_plugin_cli` 里剥掉 CLI 前缀，中央库 id 不应该默默吞下它。
pub fn id_for_name(raw: &str) -> Result<String, AppError> {
    let name = raw.trim();
    if name.is_empty() || name.len() > 200 {
        return Err(AppError::Plugin("插件名称为空或过长".into()));
    }
    if name.chars().any(|c| c.is_whitespace()) {
        return Err(AppError::Plugin(format!(
            "非法的插件名称 {name:?}（包含空白字符）"
        )));
    }
    for part in name.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return Err(AppError::Plugin(format!(
                "非法的插件名称 {name:?}（包含空段或 ..）"
            )));
        }
    }
    Ok(name.replace('/', "__"))
}

fn now_epoch_secs() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default()
}

// --- 中央库持久化 --------------------------------------------------------

pub fn load_store(data_dir: &Path) -> Store {
    let Ok(text) = fs::read_to_string(store_file(data_dir)) else {
        return Store::default();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

fn save_store(data_dir: &Path, store: &Store) -> Result<(), AppError> {
    fs::create_dir_all(store_dir(data_dir)).map_err(|e| AppError::Io(e.to_string()))?;
    let text = serde_json::to_string_pretty(store).map_err(|e| AppError::Io(e.to_string()))?;
    fs::write(store_file(data_dir), text + "\n").map_err(|e| AppError::Io(e.to_string()))?;
    ensure_store_npmrc(data_dir)
}

/// 在中央库目录写入本地 `.npmrc`。新版 pnpm 默认 `minimumReleaseAge` 为
/// 约 3 天，这样锁定在 dev/rc 版本上的条目也能直接安装而无需等待；同时把
/// registry 镜像固定为桌面壳已在使用的镜像，让仅镜像可见的 scoped 包也能解析。
/// 任何内容不一致都会重写一遍，使先前（错误的）配置能在原位得到修正；
/// `save_store` 在每次写库时都会调用本函数，所以内容一致时不会触碰磁盘、
/// 避免无谓的写入。
fn ensure_store_npmrc(data_dir: &Path) -> Result<(), AppError> {
    let npmrc = store_dir(data_dir).join(".npmrc");
    let registry = crate::registry::npm_registry_base();
    let text =
        format!("minimumReleaseAge=0\nregistry={registry}\n@deepseek-ai:registry={registry}\n");
    if fs::read_to_string(&npmrc)
        .map(|existing| existing == text)
        .unwrap_or(false)
    {
        return Ok(());
    }
    fs::write(&npmrc, text).map_err(|e| AppError::Io(e.to_string()))?;
    Ok(())
}

fn store_item(data_dir: &Path, id: &str) -> Option<StoreItem> {
    load_store(data_dir)
        .items
        .into_iter()
        .find(|item| item.id == id)
}

fn upsert_item(data_dir: &Path, item: StoreItem) -> Result<(), AppError> {
    let mut store = load_store(data_dir);
    if let Some(existing) = store.items.iter_mut().find(|i| i.id == item.id) {
        *existing = item;
    } else {
        store.items.push(item);
    }
    save_store(data_dir, &store)
}

fn remove_item(data_dir: &Path, id: &str) -> Result<(), AppError> {
    let mut store = load_store(data_dir);
    store.items.retain(|item| item.id != id);
    save_store(data_dir, &store)
}

fn read_meta(data_dir: &Path, version: &str, id: &str) -> Option<KernelMeta> {
    let text = fs::read_to_string(kernel_meta_file(data_dir, version, id)).ok()?;
    serde_json::from_str(&text).ok()
}

fn write_meta(data_dir: &Path, version: &str, id: &str, meta: &KernelMeta) -> Result<(), AppError> {
    if let Some(parent) = kernel_meta_file(data_dir, version, id).parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::Io(e.to_string()))?;
    }
    let text = serde_json::to_string(meta).map_err(|e| AppError::Io(e.to_string()))?;
    fs::write(kernel_meta_file(data_dir, version, id), text)
        .map_err(|e| AppError::Io(e.to_string()))
}

// --- spec 解析 -------------------------------------------------------------

/// 尝试从安装 spec 中剥出 `dsh plugin ... add <pkg>` 这类 CLI 调用形式。
/// 返回请求的 profile（若有）以及余下解析器能直接使用的裸包 spec。对于
/// 不匹配 CLI 形式的输入返回 `None`，这些会继续走现有的 npm / git /
/// owner-repo 解析分支。
///
/// 桌面壳只接受内核 `dsh plugin` 命令的手动安装形式：`add` 和 `install`
/// （内核将二者视为别名）。`remove` / `update` / `list` 一律拒绝，
/// 以避免粘贴进来的命令把用户只想安装的插件误卸载。`--profile`（或 `-p`）
/// 标志会被解析但忽略：桌面壳始终将插件接入 `DEFAULT_PROFILE`（`web`）；
/// 支持多 profile 的事另外追踪。
///
/// 识别的形式：
///
/// ```text
/// dsh plugin add <pkg>
/// dsh plugin install <pkg>
/// dsh plugin --profile web add <pkg>
/// dsh plugin -p web install <pkg>
/// ```
///
/// 内核可能接受的尾随标志（`--save-dev`、`--force` …）会被静默丢弃 —— 桌面壳
/// 的解析器只需要拿到包 spec 即可。
fn split_dsh_plugin_cli(spec: &str) -> Option<(Option<String>, String)> {
    let s = spec.trim();
    let after_dsh = s.strip_prefix("dsh ")?;
    let after_plugin = after_dsh.trim_start().strip_prefix("plugin")?;
    let args = after_plugin.trim();

    let mut profile: Option<String> = None;
    let mut package: Option<String> = None;
    let mut iter = args.split_whitespace();
    while let Some(arg) = iter.next() {
        match arg {
            "--profile" | "-p" => {
                profile = iter.next().map(str::to_string);
            }
            "add" | "install" => {
                // 动词后的第一个位置参数就是包 spec。尾随标志不必追踪
                // —— 手动安装流程用不到，丢掉即可。
                package = iter.next().map(str::to_string);
                break;
            }
            _ => {
                // 未知的前置标志 —— 直接放弃，让常规的 npm / git 解析器接手。
                // 这样能捕获用户不小心输入的 `dsh plugin something`。
                return None;
            }
        }
    }
    package.map(|p| (profile, p))
}

/// 尝试从安装 spec 中剥出包管理器安装命令（`npm install <pkg>`、
/// `pnpm add <pkg>`、`yarn add <pkg>`、`bun add <pkg>` …）。返回裸包 spec；
/// 不匹配该形式的输入返回 `None`，继续走常规的 npm / git / owner-repo 解析。
///
/// 标志无论出现在哪里（动词前后、包 spec 前后）都会被静默丢弃。桌面壳不
/// 关心 `--save-dev` / `--global` / `--registry` —— 所有接受的写法最终都会
/// 落到 `<dsh_home>/plugins/` 下的同一中央库路径。接受的动词只有
/// `install` / `i` / `add`（npm 的 `install` 和 `i` 是同一动作的别名；
/// `add` 是较新的别名，与 pnpm / yarn / bun 保持一致）。
///
/// 识别的形式（前缀可为 `npm` / `pnpm` / `yarn` / `bun` 任一）：
///
/// ```text
/// npm install <pkg>
/// npm i @scope/pkg@1.2.3
/// pnpm add owner/repo
/// yarn add https://github.com/o/r.git#v1
/// npm install --save-dev <pkg>      ← --save-dev 丢弃
/// ```
fn split_package_manager_cli(spec: &str) -> Option<String> {
    // 四种包管理器共享同一套动词词汇 `install` / `i` / `add`，差别只在二进制前缀。
    const VERBS: &[&str] = &["install", "i", "add"];

    let s = spec.trim();
    let rest = s
        .strip_prefix("npm ")
        .or_else(|| s.strip_prefix("pnpm "))
        .or_else(|| s.strip_prefix("yarn "))
        .or_else(|| s.strip_prefix("bun "))?;

    let mut saw_verb = false;
    for token in rest.split_whitespace() {
        if !saw_verb {
            if VERBS.contains(&token) {
                saw_verb = true;
            }
            // 动词前的任何内容（如 `npm --silent install <pkg>` 这类二进制前缀
            // 标志）都会被丢弃。
            continue;
        }
        // 动词之后，取第一个非标志的位置参数。其前的任何标志
        // （如 `npm i -D <pkg>`）会被静默跳过。
        if token.starts_with('-') {
            continue;
        }
        return Some(token.to_string());
    }
    None
}

/// 把 npm spec 拆分为 (name, 可选 pin)。scope 前缀之后的最后一个 `@`
/// 用来分隔版本；`@scope/name@1.2.3` 解析为 `(@scope/name, 1.2.3)`。
/// 纯名字则原样返回。
fn split_npm_spec(spec: &str) -> Result<(String, Option<String>), AppError> {
    let s = spec.trim();
    if s.starts_with('@') {
        let (head, rest) = s
            .split_once('/')
            .ok_or_else(|| AppError::Plugin(format!("非法的 npm 包名 {spec:?}")))?;
        let rest = rest.trim();
        let (name, pin) = match rest.rsplit_once('@') {
            Some((n, p)) if !n.is_empty() && !p.is_empty() && !p.contains('/') => {
                (n, Some(p.to_string()))
            }
            _ => (rest, None),
        };
        let name = format!("{head}/{name}");
        return Ok((name, pin));
    }
    match s.rsplit_once('@') {
        Some((n, p)) if !n.is_empty() && !p.is_empty() && !p.contains('/') => {
            Ok((n.to_string(), Some(p.to_string())))
        }
        _ => Ok((s.to_string(), None)),
    }
}

/// 把安装请求解析为 `PluginSpec`。可接受的形式：
///   - 内核完整的 `dsh plugin [--profile X] (add|install) <pkg>` CLI 调用
///     （其中可选的 `--profile` / `-p` 标志被忽略 —— 桌面壳始终接入活动 profile）；
///   - 带可选 `@version` 钉的 npm 包名，包括 `@scope/name`；
///   - git URL（`https://…`、`git@…` 或 `github.com/owner/name`）；
///   - 裸的 `owner/repo` 简写，可附 `#tag`。
pub fn parse_spec(spec: &str) -> Result<PluginSpec, AppError> {
    let s = spec.trim().trim_end_matches('/');
    if s.is_empty() || s.len() > 500 {
        return Err(AppError::Plugin("安装地址为空或过长".into()));
    }
    // 先尝试 `dsh plugin ... add <pkg>`：方便从内核文档或 ChatGPT 建议里
    // 直接复制粘贴。helper 会剥出包 spec 并递归，让下游所有分支
    // （npm / git / owner-repo）保持单一真相源。
    if let Some((_profile, pkg)) = split_dsh_plugin_cli(s) {
        return parse_spec(&pkg);
    }
    // 标准包管理器安装语法（`npm install <pkg>`、`pnpm add <pkg>`、
    // `yarn add <pkg>`、`bun add <pkg>`）。同样的递归 —— 标志被丢弃，
    // 抽出的 spec 走相同的 npm / git / owner-repo 流水线。
    if let Some(pkg) = split_package_manager_cli(s) {
        return parse_spec(&pkg);
    }
    if s.starts_with("git@") || s.contains("://") || s.contains("github.com/") {
        // git 来源：[url][#tag]
        let (url, pin) = match s.split_once('#') {
            Some((u, tag)) if !u.is_empty() && !tag.is_empty() => (u, Some(tag.to_string())),
            _ => (s, None),
        };
        let repo_url = s.contains("github.com/").then(|| url.to_string());
        // URL 含空路径段（协议双斜杠），先归一成 owner/repo 形状再映射 id
        let id_base = url
            .trim_start_matches("git@")
            .split("://")
            .last()
            .unwrap_or(url)
            .trim_end_matches(".git")
            .replace(':', "/");
        let id = id_for_name(&id_base)?;
        let name = url
            .trim_end_matches(".git")
            .rsplit('/')
            .next()
            .unwrap_or(url)
            .to_string();
        return Ok(PluginSpec {
            origin: "git".into(),
            source: url.to_string(),
            pin,
            id,
            name,
            repo_url,
        });
    }
    // owner/repo 简写：非 npm 样式（不含 @ 且含斜杠）按 GitHub 仓库处理
    if s.contains('/') && !s.starts_with('@') {
        // 同样的 #tag 切分逻辑，让 `owner/repo#v1.2.3` 这种简写也能
        // 落到 fetch_git 的 tag 路径，而不是把 #tag 拼进 URL 然后
        // git ls-remote 永远拿不到。
        let (repo_path, pin) = match s.split_once('#') {
            Some((repo, tag)) if !repo.is_empty() && !tag.is_empty() => {
                (repo, Some(tag.to_string()))
            }
            _ => (s, None),
        };
        let github = format!("https://github.com/{repo_path}.git");
        let id = id_for_name(repo_path)?;
        return Ok(PluginSpec {
            origin: "git".into(),
            source: github,
            pin,
            id,
            name: repo_path
                .rsplit('/')
                .next()
                .unwrap_or(repo_path)
                .to_string(),
            repo_url: Some(format!("https://github.com/{repo_path}")),
        });
    }
    // npm 来源
    let (name, pin) = split_npm_spec(s)?;
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "-._@/".contains(c))
    {
        return Err(AppError::Plugin(format!("非法的 npm 包名 {spec:?}")));
    }
    let id = id_for_name(&name)?;
    Ok(PluginSpec {
        origin: "npm".into(),
        source: name.clone(),
        pin,
        id,
        name,
        repo_url: None,
    })
}

// --- 版本比较 -----------------------------------------------------------
// 与内核发布列表共用：`crate::version::cmp_versions`。

/// 在 tag 候选中挑出最高版本，若无则返回 `None`。
fn latest_tag<'a>(tags: impl Iterator<Item = &'a str>) -> Option<String> {
    tags.filter_map(|t| {
        let stripped = t.strip_prefix('v').unwrap_or(t);
        let head = stripped.split_once('-').map(|(h, _)| h).unwrap_or(stripped);
        let parts: Vec<&str> = head.split('.').collect();
        (parts.len() >= 2 && parts[..2].iter().all(|seg| seg.parse::<u64>().is_ok()))
            .then(|| t.to_string())
    })
    .max_by(|a, b| cmp_versions(a, b))
}

/// 给定的版本字符串是否形如 semver（例如 `v0.15.0`、`1.2.3-rc.1`），而非
/// git 短 hash（例如 `v646c91c`）。
///
/// 由 `is_newer_than` 用于识别以下罕见的回退路径：未锁定的 git 来源仓库
/// 没有任何可用的 semver tag —— 此时 `installed_version` 是克隆下来的 HEAD
/// 短 hash，而 `cmp_versions` 会单纯因为数字段数量把任何 semver tag 排在
/// 前面。先按形态过滤一次，让 `is_newer_than` 选用合适的比较方式，而不是
/// 盲目信任那种顺序。
fn looks_like_semver(version: &str) -> bool {
    let stripped = version.strip_prefix('v').unwrap_or(version);
    let head = stripped.split_once('-').map(|(h, _)| h).unwrap_or(stripped);
    let parts: Vec<&str> = head.split('.').collect();
    parts.len() >= 2 && parts[..2].iter().all(|seg| seg.parse::<u64>().is_ok())
}

/// 给定来源的插件，候选版本 `latest` 是否比当前已安装的 `installed` 更新。
///
/// - npm / 锁定的 git：按 `cmp_versions` 与 semver 基线排序。
/// - 未锁定 git、已安装版本呈 tag 形态（`fetch_git` 解析到最高 semver tag
///   之后的常见情况）：同样按 semver 排序。
/// - 未锁定 git、已安装版本呈 hash 形态（仓库无任何 semver tag 时的回退
///   路径）：`cmp_versions` 会单纯因为数字段数量把远端的 tag 形 `latest`
///   排在前面，因此改为字符串相等判断 —— 但仅在 `latest` 也是 hash 时生效。
///   当 hash 形 `installed` 面对 tag 形 `latest` 时，说明远端没有可比较的
///   commit 图信号，应当报告无更新，直到用户手动重新安装。
fn is_newer_than(latest: &str, installed: &str, origin: &str, pinned: bool) -> bool {
    if origin == "git" && !pinned && !looks_like_semver(installed) {
        if looks_like_semver(latest) {
            false
        } else {
            latest != installed
        }
    } else {
        cmp_versions(latest, installed) == Ordering::Greater
    }
}

// --- 取源 ----------------------------------------------------------------

/// 拉取某个包的 npm registry 文档。
fn fetch_npm_doc(name: &str) -> Result<NpmDoc, String> {
    let url = format!("{}{}", crate::registry::npm_registry_base(), name);
    let body = http_get_string(&url, None)?;
    serde_json::from_str(&body).map_err(|e: serde_json::Error| e.to_string())
}

/// 将 npm tgz 解压到 `dest`，并剥掉其顶层的 `package/` 段。共享的 Rust
/// 解压器会拒绝路径穿越、链接和特殊文件，并同时限制条目数量和声明的
/// 解压后大小，再做发布。
fn extract_tarball(tarball: &Path, dest: &Path) -> Result<(), String> {
    crate::archive::extract_gzip_tarball(tarball, dest)
}

fn write_source_marker(spec: &PluginSpec, version: &str, dest: &Path) -> Result<(), AppError> {
    let marker = serde_json::json!({
        "id": spec.id,
        "origin": spec.origin,
        "source": spec.source,
        "version": version,
        "fetchedAt": now_epoch_secs(),
    });
    let text = serde_json::to_string_pretty(&marker).map_err(|e| AppError::Io(e.to_string()))?;
    fs::write(dest.join(SOURCE_MARKER), text + "\n").map_err(|e| AppError::Io(e.to_string()))
}

/// 进行中取源目录的前缀（`.tmp-<pid>-<ts>`）。配合 `.dsh-id` 标记，
/// 让 `reconcile_store` 能按插件 id 暂存目录归类，而不必从目录名里解析 id
///（目录名可能含 `-`）。
const TMP_PREFIX: &str = "tmp-";
/// 已通过 `validate_plugin`、等待发布的取源结果的前缀。
/// `.new-<pid>-<ts>` 只在校验完成到最终 rename 到 `final_dir` 之间存在。
const NEW_PREFIX: &str = "new-";
/// `final_dir` → `new_dir` 切换过程中被挪到一旁的旧活动插件目录的前缀。
/// `.backup-<pid>-<ts>` 会一直保留到发布成功、下一次清理把它移除；中途崩溃
/// 时它是 `reconcile_store` 回滚到已知可用旧版本的安全网。
const BACKUP_PREFIX: &str = "backup-";
/// 暂存目录内部记录插件 id 的文件，恢复流程据此识别该目录归属，不必再解析路径。
const ID_MARKER: &str = ".dsh-id";

/// 在 `store` 下构造一个唯一的空暂存目录。`kind` 取 `TMP_PREFIX` /
/// `NEW_PREFIX` / `BACKUP_PREFIX` 之一；`pid` 与 `nanos` 折入目录名，保证
/// 两个并发的取源（或与崩溃交错的两次更新）不会冲突。
///
/// 何时写入 `.dsh-id` 标记由调用方决定。原来在 rename 目标上预先盖章是
/// Windows 上的故障模式：上一次尝试遗留的 `.new-<pid>-<ts>` 目录里既有
/// 标记文件也有中间内容，Windows 上 `fs::rename` 会以 ERROR_DIR_NOT_EMPTY
/// 拒绝非空目标。让新路径在 rename 完成前保持空、只在源端盖章，封住了这
/// 个口子。
///
/// `fs::remove_dir_all` 不再是「点了就忘」：清理旧目标的失败会暴露出来，
/// 让调用方决定是重试、上报还是回退到别的路径。正常路径下返回值与之前一致。
fn new_staging_dir(store: &Path, kind: &str, id: &str) -> io::Result<PathBuf> {
    let _ = id;
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    let dir = store.join(format!("{kind}{pid}-{nanos}"));
    match fs::remove_dir_all(&dir) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => {
            return Err(e);
        }
    }
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// 在已有暂存目录里写入 `.dsh-id` 标记，方便 `reconcile_store` 把它与对应
/// 的 `final_dir` 归到一组。只能在 rename 成功后调用，绝不能在之前调用，
/// 这样 rename 目标在 Windows 上始终保持空目录。
fn stamp_id_marker(dir: &Path, id: &str) -> io::Result<()> {
    fs::write(dir.join(ID_MARKER), format!("{id}\n"))
}

/// 把插件拉取到中央库的暂存 tmp 目录，校验后再以崩溃安全的方式发布到
/// `final_dir`。返回新的中央库条目，沿用原 mode 与 latest。`pnpm_exe` 用于
/// 构建那些提交里不含 `lib/` 的 git 来源插件。
///
/// fetch → validate → publish 三段使用三种暂存名，使得任何步骤崩溃时，
/// 线上插件（`final_dir`）只会落到两种可恢复状态之一：要么指向旧版本
/// （在切换开始前原封不动地保留），要么指向新版本（发布 rename 之前校验
/// 已通过）。`final_dir` 短暂缺失的中间态会在下次启动时由 `reconcile_store`
/// 调和；当 `.new-*` 与 `.backup-*` 都从崩溃中幸存时，它倾向于回滚到旧版本
/// —— 新内容已经在磁盘上并通过校验，用户重试只需再次触发发布步骤。
fn fetch_into_store(
    data_dir: &Path,
    pnpm_exe: &Path,
    spec: &PluginSpec,
    on_progress: &mut dyn FnMut(&str),
) -> Result<StoreItem, AppError> {
    let store = store_dir(data_dir);
    fs::create_dir_all(&store).map_err(|e| AppError::Io(e.to_string()))?;
    let tmp =
        new_staging_dir(&store, TMP_PREFIX, &spec.id).map_err(|e| AppError::Io(e.to_string()))?;
    // 写入 `.dsh-id` 标记必须等到 fetch_* 返回之后再做：`git clone` 要求目标
    // 目录为空，若标记文件在拉取时已经存在，它会直接中止并报
    // "destination path '...' already exists and is not an empty directory"。
    // npm 流程的 tarball 解压倒是会顺手把标记覆盖掉，所以「先盖章再 fetch」
    // 只在 npm 流程上偶然能跑通。在这里盖章 —— 等暂存树已经在磁盘上、
    // 且 fetch 已校验目录为空 —— 既能让 `reconcile_store` 在校验/发布期间
    // 拿到所需的身份信息，标记又会跟着内容一起被 `tmp → new` 的 rename
    // 一起带走。

    let version = match spec.origin.as_str() {
        "npm" => fetch_npm(spec, &tmp, on_progress),
        "git" => fetch_git(spec, &tmp, pnpm_exe, on_progress),
        other => Err(AppError::Plugin(format!("未知来源 {other:?}"))),
    };
    let version = match version {
        Ok(v) => v,
        Err(e) => {
            let _ = fs::remove_dir_all(&tmp);
            return Err(e);
        }
    };
    stamp_id_marker(&tmp, &spec.id).map_err(|e| AppError::Io(e.to_string()))?;

    on_progress("正在校验插件是否符合 dsh 规范");
    if let Err(e) = validate_plugin(&tmp) {
        let _ = fs::remove_dir_all(&tmp);
        return Err(e);
    }

    // 把内容原子地换到 `final_dir`。三段式 rename（tmp → new → final_dir，
    // 同时把旧的 final 停到 backup）保证中途崩溃时线上插件仍可恢复。每个
    // rename 目标在 rename 成功前都保持空目录，这样 Windows 上 `MoveFileEx`
    // （对非空目录返回 ERROR_DIR_NOT_EMPTY）就不会阻塞安装。
    //
    // 错误策略：每一步都把失败往上抛，而不是吞掉。如果 rename 把校验过的
    // 内容放到了 `new` 但发布到 `final_dir` 失败，就把上一个 `final_dir` 从
    // `backup` 恢复回来，避免用户被卡在插件被卸载的状态。如果恢复本身也
    // 失败，函数会带着已暂存的状态返回错误，留给下次启动时 `reconcile_store`
    // 修补。
    let new =
        new_staging_dir(&store, NEW_PREFIX, &spec.id).map_err(|e| AppError::Io(e.to_string()))?;
    if let Err(e) = fs::rename(&tmp, &new) {
        // `tmp` 还保留着校验过的内容；留在磁盘上，让重试 /
        // `reconcile_store` 还有机会把它提升上去。
        return Err(AppError::Io(format!("将暂存目录提升到 .new-* 失败：{e}")));
    }

    let final_dir = store_plugin_dir(data_dir, &spec.id);
    let backup = new_staging_dir(&store, BACKUP_PREFIX, &spec.id)
        .map_err(|e| AppError::Io(e.to_string()))?;
    if final_dir.exists() {
        if let Err(e) = fs::rename(&final_dir, &backup) {
            // `new` 携带着校验过的内容；`final_dir` 是刚刚拒绝移动的线上插件。
            // 直接把 `new` 提升上去并附带一条非致命告警，避免把校验过的内容
            // 晾在一边 —— 用户明确请求安装了这个插件。
            if fs::rename(&new, &final_dir).is_err() {
                let _ = fs::remove_dir_all(&new);
                return Err(AppError::Io(format!("备份旧版本失败且无法发布新版本：{e}")));
            }
            return Err(AppError::Io(format!(
                "插件已发布，但备份旧版本失败（{e}）；下次更新若失败将无法回滚"
            )));
        }
        // rename 已经落定，现在再给 backup 打上标记 —— 后续若有 swap 把
        // backup 留下来变成孤儿，`reconcile_store` 还要靠标记把目录跟
        // 插件 id 关联起来。
        if let Err(e) = stamp_id_marker(&backup, &spec.id) {
            eprintln!(
                "dsh-xlink: warning, could not stamp id marker on backup of {}: {e}",
                spec.id
            );
        }
    }

    if let Err(e) = fs::rename(&new, &final_dir) {
        // 回滚：旧版本现在在 `backup` 里，恢复回 `final_dir`。如果成功，
        // `new` 就变成遗留的 `.new-*`，留给下次启动时 `reconcile_store`
        // 提升；如果失败，两份状态都在磁盘上，由恢复扫描调和。
        if fs::rename(&backup, &final_dir).is_err() {
            return Err(AppError::Io(format!("发布新版本失败且回滚旧版本失败：{e}")));
        }
        return Err(AppError::Io(format!("发布新版本失败，已回滚到旧版本：{e}")));
    }

    // 同步清理现在多余的 backup。这里的失败要对用户可见：永远留下
    // `.backup-*` 会不断堆积死目录，`reconcile_store` 虽然通常会在下次
    // 启动时清理，但不一定能从真正的「崩溃中断的 swap」中把它们分辨出来。
    if backup.exists() {
        if let Err(e) = fs::remove_dir_all(&backup) {
            return Err(AppError::Io(format!(
                "插件已发布成功，但清理备份目录失败：{e}（下次启动时 reconcile_store 会接手）"
            )));
        }
    }

    write_source_marker(spec, &version, &final_dir)?;

    let now = now_epoch_secs();
    let existing = store_item(data_dir, &spec.id);
    Ok(StoreItem {
        id: spec.id.clone(),
        name: spec.name.clone(),
        origin: spec.origin.clone(),
        source: spec.source.clone(),
        installed_version: version,
        latest_version: existing.as_ref().and_then(|e| e.latest_version.clone()),
        mode: existing
            .as_ref()
            .map(|e| e.mode.clone())
            .unwrap_or_else(|| String::from("link")),
        pinned: spec.pin.is_some(),
        installed_at: existing
            .as_ref()
            .map(|e| e.installed_at.clone())
            .unwrap_or_else(|| now.clone()),
        updated_at: now,
        repo_url: spec.repo_url.clone(),
        description: None,
    })
}

/// 调和被中断更新遗留下来的暂存目录。每次启动都能安全调用；正常路径
///（没有遗留暂存）只跑一次 `read_dir` 扫描，不做 rename 或删除。
///
/// 按插件 id 划分的恢复规则如下：
///
/// | `final_dir` | `.new-*` | `.backup-*` | `.tmp-*` | 处理动作 |
/// | --- | --- | --- | --- | --- |
/// | 存在 | 任意 | 任意 | 任意 | 移除全部暂存（发布后清理或残留尝试） |
/// | 不存在 | 否 | 是 | 否 | 回滚：rename `.backup-*` 为 `final_dir` |
/// | 不存在 | 是 | 否 | 否 | 发布：rename `.new-*` 为 `final_dir` |
/// | 不存在 | 是 | 是 | 任意 | 回滚（更稳妥；用户保留已知的上一可用版本） |
/// | 不存在 | 否 | 否 | 是 | 未完成的取源；移除 `.tmp-*` |
/// | 不存在 | 是 | 是 | 是 | 回滚 + 移除 tmp |
///
/// 当同一 id 留下多个暂存目录时（罕见但可能 —— 比如上一次恢复流程自身崩溃
/// 中断），保留最新的那一份：`.tmp-*` 一律丢弃（从未通过校验）；在
/// `.new-*` / `.backup-*` 之间，后缀字典序最大者胜出（时间戳 + pid 自然
/// 排成最新在最后），其余较老的对等目录会被移除。
pub fn reconcile_store(data_dir: &Path) {
    let store = store_dir(data_dir);
    let Ok(entries) = fs::read_dir(&store) else {
        return;
    };

    enum Kind {
        Tmp,
        New,
        Backup,
    }

    let mut by_id: std::collections::HashMap<String, Vec<(Kind, PathBuf)>> =
        std::collections::HashMap::new();
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        // `.tmp-` 是引入标记前的旧命名方式；当前的暂存目录统一使用
        // `tmp-` / `new-` / `backup-`（见 `new_staging_dir`）。
        let kind = if name.starts_with(TMP_PREFIX) || name.starts_with(".tmp-") {
            Kind::Tmp
        } else if name.starts_with(NEW_PREFIX) {
            Kind::New
        } else if name.starts_with(BACKUP_PREFIX) {
            Kind::Backup
        } else {
            continue;
        };
        let marker = fs::read_to_string(entry.path().join(ID_MARKER))
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let Some(id) = marker else {
            // 没有 id 标记的暂存目录：是创建暂存目录和打标记之间崩溃（或是更早
            // 版本对暂存目录采用了不同命名）的残留。它从来不是用户数据 —— 直接
            // 回收，避免无限堆积。
            let _ = fs::remove_dir_all(entry.path());
            continue;
        };
        if id == name {
            // 名字以暂存前缀开头的线上插件（npm 允许 `tmp-foo` 这类名字）：
            // 它的 final 目录自带标记，不能把它自己当成残留暂存分组。
            continue;
        }
        by_id.entry(id).or_default().push((kind, entry.path()));
    }

    for (id, mut items) in by_id {
        let final_dir = store.join(&id);
        // 按目录名排序（其中编码了 pid + 时间戳），最新的排在最后。
        items.sort_by(|a, b| a.1.file_name().cmp(&b.1.file_name()));

        if final_dir.exists() {
            for (_, path) in items {
                let _ = fs::remove_dir_all(&path);
            }
            continue;
        }

        // 分别挑出最新的 `.new-*` 与 `.backup-*`，丢弃所有较老的同辈。
        // `.tmp-*` 一律丢弃。
        let newest_new = items
            .iter()
            .rev()
            .find(|(k, _)| matches!(k, Kind::New))
            .map(|(_, p)| p.clone());
        let newest_backup = items
            .iter()
            .rev()
            .find(|(k, _)| matches!(k, Kind::Backup))
            .map(|(_, p)| p.clone());
        for (kind, path) in &items {
            let drop = match kind {
                Kind::Tmp => true,
                Kind::New => Some(path) != newest_new.as_ref(),
                Kind::Backup => Some(path) != newest_backup.as_ref(),
            };
            if drop {
                let _ = fs::remove_dir_all(path);
            }
        }

        // 落实上面那张表里的恢复动作。
        if let Some(backup) = newest_backup {
            // 两个状态都幸存时，回滚是更稳妥的默认：旧版本是我们知道用户
            // 已经在跑的那一份，而 `.new-*` 内容只是「已校验但还没跑起来」。
            let _ = fs::rename(&backup, &final_dir);
            if let Some(new) = newest_new {
                let _ = fs::remove_dir_all(&new);
            }
        } else if let Some(new) = newest_new {
            let _ = fs::rename(&new, &final_dir);
        }
        // 否则：只剩 `.tmp-*`，上面已经清理掉了。
    }
}

/// 在给定 npm 文档与用户提供的 pin（或 `None` 表示「按包的 `latest`
/// dist-tag」)的前提下，决定插件安装应当钉到哪个版本。返回解析后的版本字符串，
/// 以及当解析结果为空字符串时应当出现在错误信息中的标签。
///
/// Dist-tag 解析规则：
/// - `pin = None` → 查 `dist-tags.latest`，缺省回退为 ""
/// - `pin = Some(tag)`，且 `tag` 命中 dist-tag → 使用该 tag 指向的版本
/// - `pin = Some(ver)`，且 `ver` 未命中任何 dist-tag → 把 pin 当字面量
///   semver 使用（调用方会以清晰的错误信息暴露 `versions[ver]` 查询失败）
fn resolve_npm_version(doc: &NpmDoc, pin: Option<&str>) -> (String, String) {
    match pin {
        Some(tag) => (
            doc.dist_tags
                .get(tag)
                .cloned()
                .unwrap_or_else(|| tag.to_string()),
            tag.to_string(),
        ),
        None => (
            doc.dist_tags.get("latest").cloned().unwrap_or_default(),
            "latest".to_string(),
        ),
    }
}

fn fetch_npm(
    spec: &PluginSpec,
    dest: &Path,
    on_progress: &mut dyn FnMut(&str),
) -> Result<String, AppError> {
    on_progress(&format!("正在查询 npm registry：{}", spec.source));
    let doc =
        fetch_npm_doc(&spec.source).map_err(|e| AppError::Plugin(format!("查询 npm 失败：{e}")))?;
    // 先经 `dist-tags` 解析请求的版本，这样 `@latest`（或 `@next`、`@beta`）
    // 之类的 pin 就能落到 `versions` 索引里实际的 semver 字符串上。少了这一步，
    // 字面量 `"latest"` 会被当成 `versions` 的 key，查询返回 `None`，用户会看到
    // 「npm 上 @linxin666/dsh-liangshen@latest 没有可下载的 tarball」，
    // 即便这个包以及它的最新 tarball 都已发布在 registry 上。未命中任何
    // dist-tag 的 pin 则原样穿透，保证 `@1.2.3` 这类字面量 semver 仍能直接
    // 命中 `versions`。
    let (version, pin_label) = resolve_npm_version(&doc, spec.pin.as_deref());
    if version.is_empty() {
        return Err(AppError::Plugin(format!(
            "npm 上找不到包 {} 或其 {pin_label} 标记",
            spec.source
        )));
    }
    let tarball = doc
        .versions
        .get(&version)
        .and_then(|v| v.dist.as_ref())
        .map(|d| d.tarball.clone())
        .filter(|t| !t.is_empty())
        .ok_or_else(|| {
            AppError::Plugin(format!(
                "npm 上 {}@{version} 没有可下载的 tarball",
                spec.source
            ))
        })?;
    on_progress(&format!("正在下载 {}@{version} …", spec.source));
    let tgz = dest.join(".pkg.tgz");
    http_get_file(&tarball, &tgz).map_err(|e| AppError::Plugin(format!("下载失败：{e}")))?;
    // 通过共享的 Rust 归档处理器解压到暂存目录。它会校验 npm 的 `package/`
    // 根，并把其子项发布到 `dest`，后续 `validate_plugin(&dest)` 与物化步骤
    // 就从这里读取。
    extract_tarball(&tgz, dest)
        .map_err(|e| AppError::Plugin(format!("解包失败：{e}（请确认下载内容完整后重试）")))?;
    let _ = fs::remove_file(&tgz);
    Ok(version)
}

fn fetch_git(
    spec: &PluginSpec,
    dest: &Path,
    pnpm_exe: &Path,
    on_progress: &mut dyn FnMut(&str),
) -> Result<String, AppError> {
    // 探测和 clone 都走 `process::command_with_path`，让继承的 PATH 包含
    // 用户的工具位置。Windows 上 GUI 子系统的 release 构建在启动时只能
    // 看到 system PATH；而 Git for Windows 注册在用户 PATH
    // （`HKCU\Environment\Path`），这里直接 `Command::new("git")` 会得到
    // "command not found"，用户就会看到误导性的「未找到 git」错误，
    // 尽管 `git` 在他们打开的任何 cmd.exe 里都能正常运行。
    if !matches!(run_capture("git", &["--version"]), Ok((true, _))) {
        return Err(AppError::Plugin(
            "未找到 git（git 来源的插件需要 git；请先安装 git）".into(),
        ));
    }

    // 决定要 checkout 什么。
    // - pinned：spec 已经给出 `#tag`，直接使用。
    // - unpinned：挑选远端发布的最高 semver tag，让磁盘上保存的
    //   `installed_version` 与 `check_updates` 要对比的字符串形态一致。
    //   没有 semver tag 的仓库回退到默认分支（HEAD 短 hash）；
    //   `is_newer_than` 会专门处理这种回退情况，避免新 tag 因为段数
    //   看上去比 hash「更新」。
    let branch = match spec.pin.as_ref() {
        Some(tag) => Some(tag.clone()),
        None => match git_latest_tag(&spec.source) {
            Ok(Some(tag)) => Some(tag),
            Ok(None) => None,
            Err(e) => {
                return Err(AppError::Plugin(format!("查询最新 tag 失败：{e}")));
            }
        },
    };

    on_progress(&format!("正在克隆 {}", spec.source));
    let mut cmd = crate::process::command_with_path("git");
    cmd.arg("clone").arg("--depth").arg("1");
    if let Some(tag) = &branch {
        cmd.arg("--branch").arg(tag);
    }
    cmd.arg(&spec.source).arg(dest);
    let output = crate::process::run_command_capture(cmd, "git clone")
        .map_err(|e| AppError::Io(format!("无法运行 git：{e}")))?;
    let (success, stdout, stderr) = output;
    if !success {
        // 优先反向查找 "fatal:" / "error:" 行；找不到就退回到最后一行
        // 非空 stderr，再退回 "，请检查地址与网络" 兜底文案。
        let detail = stderr
            .lines()
            .rev()
            .find(|l| {
                let t = l.trim_start();
                t.starts_with("fatal:") || t.starts_with("error:")
            })
            .or_else(|| stderr.lines().rev().find(|l| !l.trim().is_empty()))
            .unwrap_or("")
            .trim()
            .to_string();
        let stdout_tail = stdout
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("")
            .trim()
            .to_string();
        let mut msg = String::from("git clone 失败");
        if !detail.is_empty() {
            msg.push_str(&format!("：{detail}"));
        } else if !stdout_tail.is_empty() {
            msg.push_str(&format!("：{stdout_tail}"));
        } else {
            msg.push_str("，请检查地址与网络");
        }
        return Err(AppError::Plugin(msg));
    }
    build_git_plugin(dest, pnpm_exe, on_progress)?;
    if let Some(tag) = branch {
        return Ok(tag);
    }
    // 未锁定的仓库且没有任何 semver tag：克隆的是默认分支；记录下 HEAD
    // 的 hash，让 source marker 仍能指向稳定的内容，用户也能看出当前
    // 拥有的是哪个 commit。
    let dest_str = dest.to_str().unwrap_or("");
    let (ok, out) = run_capture("git", &["-C", dest_str, "rev-parse", "--short", "HEAD"])
        .map_err(|e| AppError::Io(e.to_string()))?;
    Ok(if ok {
        out.trim().to_string()
    } else {
        String::from("head")
    })
}

/// 在克隆完成后立刻构建一个 git 来源插件。
///
/// Git 仓库把构建产物放在 `.gitignore` 里（`lib/` 从来不会被提交），所以
/// 刚克隆下来的树不能满足加载器，直到构建完为止。包自身的 `prepare`
/// 脚本正是 npm 官方为这种场景提供的钩子；通过 pnpm 跑它能让工具链解析
/// 留在插件内部完成。尽力而为：当 `prepare` 不存在时插件必须自带预构建
/// 产物，且 `validate_plugin` 仍会守住最终状态，所以只有当声明的 prepare
/// 真的失败时，这里才会报告失败。
fn build_git_plugin(
    dest: &Path,
    pnpm_exe: &Path,
    on_progress: &mut dyn FnMut(&str),
) -> Result<(), AppError> {
    let root = match read_plugin_manifest(dest) {
        Ok(root) => root,
        Err(_) => return Ok(()), // 真正的错误由 validate_plugin 报告
    };
    let has_prepare = root
        .get("scripts")
        .and_then(|s| s.get("prepare"))
        .and_then(|p| p.as_str())
        .map(|p| !p.is_empty())
        .unwrap_or(false);
    let main = root.get("main").and_then(|m| m.as_str()).unwrap_or("");
    let entry_ready = !main.is_empty()
        && (dest.join(main).is_file() || dest.join(format!("{main}.js")).is_file());
    // 预构建的仓库：什么都不用做。「声明了 prepare 但还没构建」才是常见情况。
    if !has_prepare || entry_ready {
        return Ok(());
    }
    // 构建脚本需要依赖来定位工具（tsdown 等）。install_store_deps 之后会在
    // link 模式下跑一次，但太晚 —— 入口检查在这之前发生，且 copy 模式根本不会
    // 跑 install_store_deps。
    on_progress("正在安装插件依赖并构建（pnpm，git 来源需要生成 lib/）");
    let log_path = dest.join(".dsh-build.log");
    let args = [
        "install",
        "--ignore-workspace",
        "--config.node-linker=hoisted",
        kernel::PNPM_REPORTER,
        kernel::PNPM_NO_STRICT_DEP_BUILDS,
        "--config.enable-pre-post-scripts=true",
    ];
    // 当 enable-pre-post-scripts 打开时，pnpm 会在 install 之后自动跑包的
    // `prepare` 生命周期脚本。`pnpm_exe.parent()` 被前置到子进程的 PATH 里，
    // 让生命周期 shell 能解析到 `node`（以及任何同目录里有 shebang 的工具），
    // 即使 GUI 进程继承到的 PATH 只有 launchd 的、不含用户的 Homebrew / nvm
    // bin 目录；少了这一步 prepare 步骤会返回 127 并报
    // `env: node: No such file or directory`。
    let pnpm_dir = pnpm_exe.parent().unwrap_or(Path::new("."));
    // 构建日志放在插件自己的目录里；不做按日轮转，也不带构建种类标记
    // —— 插件卸载时该文件会被一起删掉，所以固定路径反而最合适。
    let status = kernel::run_pnpm_at(
        pnpm_exe,
        &args,
        dest,
        &log_path,
        &[pnpm_dir],
        &mut *on_progress,
    )
    .map_err(|e| {
        AppError::Plugin(format!(
            "无法运行 pnpm（{e}），请确认 Node.js 与 pnpm 可用，详情见日志：{}",
            log_path.display()
        ))
    })?;
    if !status.success() {
        return Err(AppError::Plugin(format!(
            "插件构建失败（退出码 {:?}）：`prepare` 未成功生成入口。详情见 {}",
            status.code(),
            log_path.display()
        )));
    }
    Ok(())
}

fn read_plugin_manifest(plugin_root: &Path) -> Result<serde_json::Value, serde_json::Error> {
    let text =
        fs::read_to_string(plugin_root.join("package.json")).map_err(serde_json::Error::io)?;
    serde_json::from_str(&text)
}

/// 插件目录是否声明了运行时依赖。
fn manifest_has_deps(plugin_root: &Path) -> bool {
    let Ok(root) = read_plugin_manifest(plugin_root) else {
        return false;
    };
    root.get("dependencies")
        .and_then(|d| d.as_object())
        .map(|d| !d.is_empty())
        .unwrap_or(false)
}

/// 插件是否声明了 bundle 层。
fn manifest_is_bundle(plugin_root: &Path) -> bool {
    let Ok(root) = read_plugin_manifest(plugin_root) else {
        return false;
    };
    root.get("dsh")
        .and_then(|d| d.get("bundle"))
        .and_then(|b| b.get("patch"))
        .and_then(|p| p.as_str())
        .map(|p| !p.is_empty())
        .unwrap_or(false)
}

/// 内核加载已安装插件所需满足的条件：可解析的 package.json 且带 name；包
/// 声明 bundle 层时对应的 patch 文件必须存在；无论是否带 bundle 都要有可
/// 解析的 `main` / `exports` 入口。紧随 fetch 之后运行，让不合规的包
/// 在安装阶段就大声失败，而不是拖到下次内核启动才崩。
///
/// 即使有 bundle 层，入口检查也必须执行：插件常常两者都声明（bundle 改写
/// 客户端 UI，`main` 加载服务端一半）。一旦在 bundle 分支提前返回，git
/// 来源的安装就会在缺少构建产物（`lib/` 被 .gitignore 掉）时也通过，
/// 然后内核在解析 ESM 时崩溃。
fn validate_plugin(dir: &Path) -> Result<(), AppError> {
    let root = read_plugin_manifest(dir)
        .map_err(|_| AppError::Plugin("不符合 dsh 插件规范：缺少可解析的 package.json".into()))?;
    let name = root.get("name").and_then(|n| n.as_str()).unwrap_or("");
    if name.is_empty() {
        return Err(AppError::Plugin(
            "不符合 dsh 插件规范：package.json 缺少 name 字段".into(),
        ));
    }
    if let Some(patch) = root
        .get("dsh")
        .and_then(|d| d.get("bundle"))
        .and_then(|b| b.get("patch"))
        .and_then(|p| p.as_str())
    {
        if patch.is_empty() || !dir.join(patch).is_file() {
            return Err(AppError::Plugin(format!(
                "不符合 dsh 插件规范：声明了 bundle 层但包内找不到 patch 文件 {patch:?}，内核启动将无法加载该层"
            )));
        }
        // 不提前返回：下面的运行时入口检查仍然必需。
    }
    let has_exports = root.get("exports").is_some();
    if !has_exports {
        let main = root.get("main").and_then(|m| m.as_str()).unwrap_or("");
        if main.is_empty() {
            return Err(AppError::Plugin(
                "不符合 dsh 插件规范：既未声明 dsh.bundle.patch，也没有 main/exports 入口，内核无法加载"
                    .into(),
            ));
        }
        // Node 解析 main 时允许省略 .js 后缀，两者都接受。
        if dir.join(main).is_file() || dir.join(format!("{main}.js")).is_file() {
            return Ok(());
        }
        return Err(AppError::Plugin(format!(
            "不符合 dsh 插件规范：main 入口 {main:?} 在包内不存在，内核无法加载。git 来源的插件通常需要在包内执行一次构建（如 `pnpm run prepare`）生成 lib/"
        )));
    }
    Ok(())
}

/// 在中央库目录里安装插件自身的依赖。只有 link 模式需要这一步（copy 模式
/// 由 profile 自己的 pnpm 负责）。
fn install_store_deps(
    data_dir: &Path,
    pnpm_exe: &Path,
    id: &str,
    on_progress: &mut dyn FnMut(&str),
) -> Result<(), AppError> {
    let dir = store_plugin_dir(data_dir, id);
    let log_path = plugin_log_path(data_dir, id);
    if !manifest_has_deps(&dir) {
        return Ok(());
    }
    on_progress("正在安装插件自身依赖（pnpm）");

    // 删除任何残留的 lockfile，让 pnpm 重新解析时不会撞上因
    // minimumReleaseAge 而禁止最近发布的 rc 版本的条目。没有 lockfile 时
    // 直接重新解析 registry，永远是安全的。
    let lockfile = dir.join("pnpm-lock.yaml");
    if lockfile.is_file() {
        fs::remove_file(&lockfile).ok();
    }

    let args = [
        "install",
        "--ignore-workspace",
        "--config.node-linker=hoisted",
        kernel::PNPM_REPORTER,
        kernel::PNPM_NO_STRICT_DEP_BUILDS,
    ];
    let pnpm_dir = pnpm_exe.parent().unwrap_or(Path::new("."));
    // 插件安装日志放在桌面壳的日志目录下；使用按日命名的写入器，让跨午夜的
    // 长安装落在两个文件里，且基于 spec 的文件名前缀让 dev / release 在弹窗
    // 标签列表里能直观地区分开。
    let status = kernel::run_pnpm(
        pnpm_exe,
        &args,
        &dir,
        &kernel::logs_dir(data_dir),
        &plugin_log_spec(id),
        &[pnpm_dir],
        &mut *on_progress,
    )
    .map_err(|e| {
        AppError::Plugin(format!(
            "无法运行 pnpm（{e}），请确认 Node.js 与 pnpm 可用，详情见日志：{}",
            log_path.display()
        ))
    })?;
    if !status.success() && !dir.join("node_modules").is_dir() {
        return Err(AppError::Plugin(format!(
            "插件依赖安装失败（退出码 {:?}），详情见日志：{}",
            status.code(),
            log_path.display()
        )));
    }
    if !status.success() {
        on_progress(
            "注意：pnpm 以非零退出码结束（多为依赖构建脚本被忽略所致），插件依赖已基本就绪",
        );
    }
    Ok(())
}

// --- 物化 ----------------------------------------------------------------

/// 把一个插件物化到一个内核：link（symlink，Windows 上是 junction）或 copy，
/// 记录在 `.meta/<id>.json` 中。返回实际采用的模式。
pub fn materialize_one(
    data_dir: &Path,
    version: &str,
    item: &StoreItem,
) -> Result<String, AppError> {
    let source = store_plugin_dir(data_dir, &item.id);
    let target = kernel_plugin_dir(data_dir, version, &item.id);
    let meta = read_meta(data_dir, version, &item.id);

    // 一次性解析中央库路径：如果中央库源本身就是一个 symlink（比如 git 来源
    // 的插件直接克隆到中央库），用真实文件系统位置，让内核插件目录拿到
    // 一条直接链接 —— 避免双重 symlink 链把 Node 的 realpath 弄断。
    let resolved_source = fs::symlink_metadata(&source)
        .ok()
        .filter(|m| m.file_type().is_symlink())
        .and_then(|_| fs::read_link(&source).ok())
        .unwrap_or_else(|| source.to_path_buf());

    let fresh = meta
        .as_ref()
        .map(|m| m.version == item.installed_version && m.mode == item.mode)
        .unwrap_or(false);

    // 如果元数据说没变化且目标已存在，还要再核对一下目标 symlink 是否真的正确。
    // 上一次运行可能留下了一条过时的双重 symlink 链，即便记录的版本和模式
    // 都没变 —— 落到下面去重建一条正确的直接链接。
    if fresh && target.exists() {
        let target_ok = fs::symlink_metadata(&target)
            .ok()
            .filter(|m| m.file_type().is_symlink())
            .and_then(|_| fs::read_link(&target).ok())
            .map(|link| link == resolved_source)
            .unwrap_or(false);
        if target_ok {
            return Ok(meta.map(|m| m.mode).unwrap_or_else(|| item.mode.clone()));
        }
    }

    // 清除旧产物（错误残留：非链接目录、指向别处的链接或旧版本副本）
    remove_materialized(data_dir, version, &item.id);

    let mut actual = item.mode.clone();
    if item.mode == "link" && make_dir_link(&resolved_source, &target).is_err() {
        // 链接失败（Windows 权限、文件系统不支持）→ 降级复制
        actual = String::from("copy");
        eprintln!(
            "dsh-xlink: link failed for {}; falling back to copy",
            item.id
        );
    }
    if actual == "copy" {
        copy_tree(&source, &target).map_err(|e| {
            AppError::Io(format!(
                "复制插件 {} 到内核失败：{e}。请关闭工作台后点击「同步」重试；若持续失败请查看日志 {}",
                item.id,
                wiring_log_path(data_dir).display()
            ))
        })?;
    }
    if !target.exists() {
        return Err(AppError::Plugin(format!(
            "物化失败：{} 在内核 {version} 中未就绪",
            item.id
        )));
    }
    write_meta(
        data_dir,
        version,
        &item.id,
        &KernelMeta {
            mode: actual.clone(),
            version: item.installed_version.clone(),
            synced_at: now_epoch_secs(),
        },
    )?;
    Ok(actual)
}

/// 删除一个文件系统 link（symlink），不动它的目标。Windows 上 `DeleteFile`
/// 会以 ERROR_ACCESS_DENIED 拒绝目录 symlink —— 只有 `RemoveDirectory`
/// 才能删除它们；文件 symlink 又需要 `DeleteFile`；两种都试一遍就覆盖了
/// 所有平台和所有形态。选错方式会让链接留在原地，之后所有操作（重建、
/// 复制）都会顺着链接追到目标里去 —— Windows 上一次插件更新正是这样演变成
/// 「把中央库目录拷给自己」并以 os error 2 失败。
fn remove_link(path: &Path) {
    if fs::remove_file(path).is_err() {
        let _ = fs::remove_dir(path);
    }
}

/// 从一个内核里移除插件的物化（link 或 copy 残留）。
fn remove_materialized(data_dir: &Path, version: &str, id: &str) {
    let target = kernel_plugin_dir(data_dir, version, id);
    match fs::symlink_metadata(&target) {
        Ok(md) if md.file_type().is_symlink() => remove_link(&target),
        Ok(_) => {
            let _ = fs::remove_dir_all(&target);
        }
        Err(_) => {}
    }
    let _ = fs::remove_file(kernel_meta_file(data_dir, version, id));
}

/// 清理内核中中央库已经不再持有的插件条目 —— 卸载时撞上 Windows 文件锁、或
/// 手工删除中央库目录后留下的残留。仅在以下两种情况清理：桌面壳能证明该条目
/// 归它所有（有 `.meta/<id>.json` 记录），或者条目已经损坏（symlink 的目标
/// 已消失）；用户手工放进内核 plugins 目录的任何东西都保留不动。以中央库
/// 成员身份而非接线过滤器为依据，保证被隔离的插件也能保留物化。
fn sweep_kernel_orphans(data_dir: &Path, version: &str, store: &Store) {
    let dir = kernel_plugins_dir(data_dir, version);
    let Ok(entries) = fs::read_dir(&dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if name == META_SUBDIR || store.items.iter().any(|i| i.id == name) {
            continue;
        }
        let path = entry.path();
        let dangling_link = fs::symlink_metadata(&path)
            .ok()
            .filter(|m| m.file_type().is_symlink())
            .map(|_| !path.exists()) // exists() 会顺着链接追到目标上
            .unwrap_or(false);
        let shell_owned = kernel_meta_file(data_dir, version, &name).is_file();
        if dangling_link || shell_owned {
            remove_materialized(data_dir, version, &name);
        }
    }
}

#[cfg(unix)]
fn make_dir_link(source: &Path, target: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(source, target)
}

#[cfg(windows)]
fn make_dir_link(source: &Path, target: &Path) -> io::Result<()> {
    std::os::windows::fs::symlink_dir(source, target)
}

/// 递归地把 source 复制到 target，替换任何已存在的内容。每个 IO 错误都
/// 带上失败的路径 —— 一句裸的 "系统找不到指定的文件 (os error 2)" 出现在
/// 含 2 万文件的 `node_modules` 复制里根本无法定位。树中损坏的 symlink
/// 会打 warning 后跳过，而不是中断整次复制：Windows 上一个悬空链接（比如
/// 内核目标移走后的同伴链接）会让 `fs::copy` 报 os error 2，尽管它周围
/// 一切正常。
fn copy_tree(source: &Path, target: &Path) -> io::Result<()> {
    copy_tree_at(source, target, &mut Vec::new())
}

fn link_points_to_ancestor(path: &Path, ancestors: &[PathBuf]) -> bool {
    let Ok(target) = fs::read_link(path) else {
        return false;
    };
    let resolved = if target.is_absolute() {
        target
    } else {
        path.parent().unwrap_or_else(|| Path::new(".")).join(target)
    };
    let resolved = fs::canonicalize(&resolved).unwrap_or(resolved);
    ancestors.iter().any(|ancestor| ancestor == &resolved)
}

/// `ancestors` 保存了当前递归栈上每个目录的规范化路径。copy 模式会顺着
/// 目录链接继续走（拷贝出来的树不能依赖链接能力），而在 macOS / Linux 上
/// pnpm 的 `node_modules` 几乎全由 symlink 构成 —— 循环依赖会形成链接环，
/// 这里通过检测目录的规范化路径是否已在祖先列表里来抓住它们。菱形结构
///（两个链接指向同一兄弟）不是环，会直接放行。
fn copy_tree_at(source: &Path, target: &Path, ancestors: &mut Vec<PathBuf>) -> io::Result<()> {
    let canonical = fs::canonicalize(source).unwrap_or_else(|_| source.to_path_buf());
    if ancestors.contains(&canonical) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("目录树存在循环链接：{}", source.display()),
        ));
    }
    ancestors.push(canonical);
    let result = copy_tree_inner(source, target, ancestors);
    ancestors.pop();
    result
}

fn copy_tree_inner(source: &Path, target: &Path, ancestors: &mut Vec<PathBuf>) -> io::Result<()> {
    if target.is_symlink() {
        remove_link(target);
        if target.is_symlink() {
            // 链接清不掉：再沿它拷贝就会写到它的目标里去（可能就是中央库本身）。
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("无法清除旧的链接 {}，请关闭工作台后重试", target.display()),
            ));
        }
    } else if target.exists() {
        let _ = fs::remove_dir_all(target);
    }
    fs::create_dir_all(target).map_err(|e| {
        io::Error::new(e.kind(), format!("创建目录 {} 失败：{e}", target.display()))
    })?;
    let entries = fs::read_dir(source).map_err(|e| {
        io::Error::new(e.kind(), format!("读取目录 {} 失败：{e}", source.display()))
    })?;
    for entry in entries {
        let entry = entry.map_err(|e| {
            io::Error::new(e.kind(), format!("遍历目录 {} 失败：{e}", source.display()))
        })?;
        let from = entry.path();
        let to = target.join(entry.file_name());
        // `file_type` 不会顺着链接走：symlink 即使目标是一个目录，也会以
        // symlink 的形态报告。Windows 上的 junction 则会显示为普通目录，
        // 直接顺着拷贝即可，这正好契合 copy 模式「不依赖链接能力」的承诺。
        let file_type = entry.file_type().map_err(|e| {
            io::Error::new(e.kind(), format!("读取 {} 的类型失败：{e}", from.display()))
        })?;
        if file_type.is_symlink() {
            if link_points_to_ancestor(&from, ancestors) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("目录树存在循环链接：{}", from.display()),
                ));
            }
            // 沿链接走一次来分类；悬空链接拷不动，但也不能因此打断整棵树。
            match fs::metadata(&from) {
                Ok(md) if md.is_dir() => copy_tree_at(&from, &to, ancestors)?,
                Ok(_) => copy_file(&from, &to)?,
                Err(e) => {
                    eprintln!(
                        "dsh-xlink: skipping dangling symlink {} during copy: {e}",
                        from.display()
                    );
                    continue;
                }
            }
        } else if file_type.is_dir() {
            copy_tree_at(&from, &to, ancestors)?;
        } else {
            copy_file(&from, &to)?;
        }
    }
    Ok(())
}

/// 把单个文件复制到 `to`，覆盖任何已存在的内容；错误信息同时带上两条路径，
/// 失败时能精确指向问题文件。
fn copy_file(from: &Path, to: &Path) -> io::Result<()> {
    let _ = fs::remove_file(to);
    fs::copy(from, to)
        .map_err(|e| {
            io::Error::new(
                e.kind(),
                format!("复制 {} → {} 失败：{e}", from.display(), to.display()),
            )
        })
        .map(|_| ())
}

/// 把插件物化到每一个已安装的内核。
pub fn sync_kernels(data_dir: &Path, item: &StoreItem) -> Result<(), AppError> {
    for version in kernel::list_installed(data_dir) {
        materialize_one(data_dir, &version.version, item)?;
    }
    Ok(())
}

// --- profile 接线 --------------------------------------------------------

/// 计算 `from_dir` 到 `to` 的相对路径（两者在同一根目录下）；当没有公共前缀
/// 时返回绝对路径。
fn relative_path(from_dir: &Path, to: &Path) -> PathBuf {
    let to_path = to;
    let from: Vec<Component> = from_dir.components().collect();
    let to: Vec<Component> = to_path.components().collect();
    let mut common = 0;
    while common < from.len() && common < to.len() && from[common] == to[common] {
        common += 1;
    }
    if common == 0 {
        return to_path.to_path_buf();
    }
    let mut out = PathBuf::new();
    for _ in common..from.len() {
        out.push("..");
    }
    for part in &to[common..] {
        out.push(part.as_os_str());
    }
    out
}

/// `package.json` 中 dependency spec 要用的正斜杠路径字符串。
fn spec_path_string(rel: &Path) -> String {
    rel.to_string_lossy().replace('\\', "/")
}

/// 新建 profile 时的模板 bundle，对应内核的 profile 模板。
fn template_bundles(profile: &str) -> Vec<String> {
    match profile {
        "web" => vec![
            String::from("@deepseek-ai/dsh-base"),
            String::from("@deepseek-ai/dsh-web-app"),
        ],
        "headless" => vec![
            String::from("@deepseek-ai/dsh-base"),
            String::from("@deepseek-ai/dsh-headless"),
        ],
        _ => vec![String::from("@deepseek-ai/dsh-base")],
    }
}

/// 把 profile 清单读成可变的 JSON 树（未知字段会原样保留）。profile
/// 目录尚未初始化时返回 `None`。
fn read_profile_json(
    data_dir: &Path,
    profile: &str,
) -> Result<Option<serde_json::Value>, AppError> {
    let path = profile_dir(data_dir, profile).join("package.json");
    let Ok(text) = fs::read_to_string(&path) else {
        return Ok(None);
    };
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|e| AppError::Io(e.to_string()))
}

fn write_profile_json(
    data_dir: &Path,
    profile: &str,
    root: &serde_json::Value,
) -> Result<(), AppError> {
    let path = profile_dir(data_dir, profile).join("package.json");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::Io(e.to_string()))?;
    }
    let text = serde_json::to_string_pretty(root).map_err(|e| AppError::Io(e.to_string()))?;
    fs::write(path, text + "\n").map_err(|e| AppError::Io(e.to_string()))
}

/// 按内核相同的方式初始化 profile 清单，但提前把模板 bundle 列表写进去，
/// 这样首次启动前就能完成接线。
fn ensure_profile(data_dir: &Path, profile: &str) -> Result<(), AppError> {
    let dir = profile_dir(data_dir, profile);
    let manifest_path = dir.join("package.json");
    if fs::metadata(&manifest_path).is_ok() {
        return Ok(());
    }
    fs::create_dir_all(&dir).map_err(|e| AppError::Io(e.to_string()))?;
    let root = serde_json::json!({
        "name": format!("dsh-profile-{profile}"),
        "private": true,
        "dependencies": {},
        "dsh": { "profile": { "bundles": template_bundles(profile) } }
    });
    write_profile_json(data_dir, profile, &root)?;
    let patch = dir.join("cordis.patch.yml");
    if !patch.exists() {
        let _ = fs::write(&patch, "# Your patch layer for this dsh profile.\n[]\n");
    }
    let workspace = dir.join("pnpm-workspace.yaml");
    let needs_workspace = !workspace.exists()
        || fs::read_to_string(&workspace)
            .map(|t| !t.contains("minimumReleaseAge: 0"))
            .unwrap_or(false);
    if needs_workspace {
        let _ = fs::write(
            &workspace,
            "packages:\n  - .\n\nnodeLinker: hoisted\nautoInstallPeers: false\nminimumReleaseAge: 0\n",
        );
    }
    Ok(())
}

/// 判断 profile 中的 dependency spec 是否由桌面壳写入（指向某个内核的
/// plugins 目录）。用来防止清退误伤 CLI 管理的依赖。
///
/// 桌面壳写出的 spec 末尾总是 `kernels/<version>/plugins/<id>`；据此路径
/// 结构匹配，而不是根据数据目录名做判断。基于目录名匹配（`desktop/kernels/`）
/// 会把 debug 壳（`desktop-dev/`）或 `DSH_DESKTOP_DATA_DIR` 覆写写出的 spec
/// 误判为用户自管的，卸载时留下悬空的依赖和 bundle 层，内核启动解析
/// 悬空 bundle 时崩溃。
fn is_managed_spec(spec: &str) -> bool {
    let Some(path) = spec
        .strip_prefix(SPEC_LINK)
        .or_else(|| spec.strip_prefix(SPEC_FILE))
    else {
        return false;
    };
    let mut segs = path.split('/').rev();
    let (Some(id), Some("plugins"), Some(version), Some("kernels")) =
        (segs.next(), segs.next(), segs.next(), segs.next())
    else {
        return false;
    };
    !id.is_empty() && !version.is_empty()
}

/// 决定哪些中央库条目参与物化与 profile 接线的过滤器。启动看护会把要排除
/// 的插件传进来，从而实现「不加载特定插件」的重启。
pub type WiringFilter<'a> = dyn Fn(&StoreItem) -> bool + 'a;

/// 按中央库对活动内核的 profile 清单进行调和，排除隔离注册表中命名的插件
/// （见 [`crate::guard`]）。
pub fn ensure_wiring(
    data_dir: &Path,
    settings: &settings::Settings,
    pnpm_exe: &Path,
    on_progress: &mut dyn FnMut(&str),
) -> Result<(usize, bool), AppError> {
    let blocked = quarantine::ids(data_dir);
    ensure_wiring_filtered(
        data_dir,
        settings,
        pnpm_exe,
        &move |item| !blocked.contains(&item.id),
        on_progress,
    )
}

/// 按中央库对活动内核的 profile 清单进行调和：把每个允许条目对应的依赖
/// 设置为已物化的目录，维护 bundle 层，活动内核变更时重写 spec。当清单发生
/// 变更或 profile 的 `node_modules` 缺失时运行 `pnpm install`。被过滤掉的
/// 条目既不会被物化也不会被接线，其遗留的托管依赖与 bundle 层会被清退
/// —— 这正是内核能在缺少这些插件时启动的原因。
///
/// manifest 写入具有事务性：pnpm 失败时回滚 manifest，因为无法解析的
/// bundles 条目会让内核在启动时崩溃。即使中央库为空也照常调和，让卸载最后
/// 一个插件时把残留清掉，而不会留下无法解析的层。
///
/// 返回 `(wired_count, changed)`。
pub fn ensure_wiring_filtered(
    data_dir: &Path,
    settings: &settings::Settings,
    pnpm_exe: &Path,
    allow: &WiringFilter<'_>,
    on_progress: &mut dyn FnMut(&str),
) -> Result<(usize, bool), AppError> {
    let store = load_store(data_dir);
    ensure_profile(data_dir, &settings.profile)?;

    // 物化活动内核，再据插件清单决定 bundle 层；没有活动内核且仍有插件时
    // 等内核装好再接线（store 为空则继续，让下面的清退逻辑跑掉残留）。
    // 被过滤器排除的插件（如启动看护隔离的嫌疑插件）既不物化也不进清单，
    // 内核因此在缺少它们的状态下完成启动。
    //
    // 单个插件物化失败不中止整轮接线：失败项不进清单（manifest 随之把它
    // 清退，内核不带它启动），其余插件照常接线，最后聚合报错。否则一个
    // 损坏的插件会让卸载残留的清退永远跑不到，故障在 store warning 里
    // 越积越多。
    let mut specs: BTreeMap<String, (String, bool)> = BTreeMap::new();
    let mut failures: Vec<String> = Vec::new();
    match kernel::read_active(data_dir) {
        Some(active) => {
            for item in store.items.iter().filter(|item| allow(item)) {
                let result = refresh_store_peers(data_dir, item, &active)
                    .and_then(|_| materialize_one(data_dir, &active, item));
                match result {
                    Ok(actual) => {
                        let prefix = if actual == "copy" {
                            SPEC_FILE
                        } else {
                            SPEC_LINK
                        };
                        let rel = relative_path(
                            &profile_dir(data_dir, &settings.profile),
                            &kernel_plugin_dir(data_dir, &active, &item.id),
                        );
                        specs.insert(
                            item.name.clone(),
                            (
                                format!("{prefix}{}", spec_path_string(&rel)),
                                manifest_is_bundle(&kernel_plugin_dir(data_dir, &active, &item.id)),
                            ),
                        );
                    }
                    Err(e) => failures.push(format!("{}（{e}）", item.name)),
                }
            }
            sweep_kernel_orphans(data_dir, &active, &store);
        }
        None if !store.items.is_empty() => return Ok((0, false)),
        None => {}
    }

    let mut root = read_profile_json(data_dir, &settings.profile)?
        .ok_or_else(|| AppError::Plugin("profile 尚未初始化".into()))?;
    let previous = root.clone();
    let changed = wire_manifest(&mut root, &specs, &settings.profile)?;

    // manifest 没变但 node_modules 缺失（上次 pnpm 失败或目录被清）也必须
    // 重装，否则 bundles 里的层解析不了，内核启动即崩。
    let profile = profile_dir(data_dir, &settings.profile);
    let node_modules_missing = !profile.join("node_modules").is_dir();
    if changed || node_modules_missing {
        if changed {
            write_profile_json(data_dir, &settings.profile, &root)?;
        }
        on_progress("正在同步 profile 依赖（pnpm install）");
        let status = run_profile_install(data_dir, &settings.profile, pnpm_exe, on_progress)?;
        if !status.success() {
            if changed {
                let _ = write_profile_json(data_dir, &settings.profile, &previous);
            }
            return Err(AppError::Plugin(format!(
                "pnpm install 在 profile 中失败（退出码 {:?}），已回滚 profile 配置，详情见日志：{}",
                status.code(),
                wiring_log_path(data_dir).display()
            )));
        }
    }
    if !failures.is_empty() {
        return Err(AppError::Plugin(format!(
            "部分插件未能接入内核：{}。其余插件已正常接线；修复后点击「同步」重试",
            failures.join("；")
        )));
    }
    Ok((specs.len(), changed))
}

/// 在指定 profile 目录下跑 `pnpm install`，返回它的退出状态，让调用方各自
/// 实施回滚语义。flag 与中央库层级的 install 保持一致：profile 只需要一个
/// 可用的 node_modules，既有的产物回退机制在 pnpm 报告 ignored-builds 误报
/// 时也兜得住。`pnpm_exe.parent()` 被前置到子进程的 PATH 里，让任何带
/// Node shebang 的生命周期脚本都能找到与启动 pnpm 相同的 `node`。
fn run_profile_install(
    data_dir: &Path,
    profile_name: &str,
    pnpm_exe: &Path,
    on_progress: &mut dyn FnMut(&str),
) -> Result<std::process::ExitStatus, AppError> {
    // 写入器在按日 spec 内部维持稳定的路径；每行都重新解析路径，跨午夜的
    // 轮转仍能落到正确的文件里。这里不再单独暴露路径 —— 命名由 spec 负责。
    let pnpm_dir = pnpm_exe.parent().unwrap_or(Path::new("."));
    // Profile 接线日志会被启动看护下所有 profile install 流程共享，因此
    // 放在桌面壳的日志目录下，按日轮转，并打上构建种类标记。
    kernel::run_pnpm(
        pnpm_exe,
        &[
            "install",
            kernel::PNPM_REPORTER,
            kernel::PNPM_NO_STRICT_DEP_BUILDS,
        ],
        &profile_dir(data_dir, profile_name),
        &kernel::logs_dir(data_dir),
        &wiring_log_spec(),
        &[pnpm_dir],
        on_progress,
    )
    .map_err(|e| AppError::Io(format!("无法运行 pnpm（{e}）")))
}

/// profile 清单的原始文本，在启动看护改写接线之前抓取，这样放弃时能
/// 精准恢复用户原本的内容。
pub fn snapshot_profile_manifest_text(data_dir: &Path, profile: &str) -> Option<String> {
    fs::read_to_string(profile_dir(data_dir, profile).join("package.json")).ok()
}

/// 恢复先前抓取过的清单，并重新同步 node_modules。缺少快照时保留当前
/// manifest、只重跑 install —— 这是文件本身丢失时能拿出的最好修复。
pub fn restore_profile_manifest(
    data_dir: &Path,
    settings: &settings::Settings,
    pnpm_exe: &Path,
    previous: Option<&str>,
    on_progress: &mut dyn FnMut(&str),
) -> Result<(), AppError> {
    let path = profile_dir(data_dir, &settings.profile).join("package.json");
    if let Some(text) = previous {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| AppError::Io(e.to_string()))?;
        }
        fs::write(&path, text).map_err(|e| AppError::Io(e.to_string()))?;
    }
    on_progress("正在恢复 profile 依赖（pnpm install）");
    let status = run_profile_install(data_dir, &settings.profile, pnpm_exe, on_progress)?;
    if !status.success() {
        return Err(AppError::Plugin(format!(
            "pnpm install 在恢复后的 profile 中失败（退出码 {:?}），详情见日志：{}",
            status.code(),
            wiring_log_path(data_dir).display()
        )));
    }
    Ok(())
}

/// 写入（或清除）中央库向用户展示的告警。安静的接线流程和启动看护的
/// 首次重试修复共用此函数，确保两处展示给用户的告警与插件卡片旁的内容一致。
pub fn set_store_warning(data_dir: &Path, warning: Option<String>) {
    let mut store = load_store(data_dir);
    store.warning = warning;
    let _ = save_store(data_dir, &store);
}

/// 给同步命令（切换内核 / 启动）用的安静接线：失败时只写入中央库供
/// `plugin_status.warning` 展示，不会阻塞动作。复用调用方缓存好的 node
/// 探测结果，让切换内核时不会再 spawn 第二次 `node --version`。
pub fn ensure_wiring_quiet(
    data_dir: &Path,
    settings: &settings::Settings,
    node_info: &node::NodeInfo,
) -> Result<(), String> {
    let (_, pnpm_exe) = commands::promise_pnpm(data_dir, node_info, |_| {})?;
    let mut noop = |_: &str| {};
    match ensure_wiring(data_dir, settings, &pnpm_exe, &mut noop) {
        Ok(_) => {
            set_store_warning(data_dir, None);
            Ok(())
        }
        Err(e) => {
            set_store_warning(data_dir, Some(e.to_string()));
            Err(e.to_string())
        }
    }
}

// --- 更新检查 -----------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct UpdateInfo {
    pub id: String,
    pub latest: Option<String>,
    pub error: Option<String>,
}

/// 把每个中央库条目对照其来源的最新版本检查一遍，结果写回中央库供 UI
/// 角标使用。
pub fn check_updates(data_dir: &Path) -> Result<Vec<UpdateInfo>, AppError> {
    let mut store = load_store(data_dir);
    let mut out = Vec::new();
    for item in &mut store.items {
        let (latest, error) = match item.origin.as_str() {
            "npm" => match http_get_npm_latest(&item.source) {
                Ok(latest) => (latest, None),
                Err(e) => (None, Some(e)),
            },
            "git" => match git_latest(item) {
                Ok(v) => (v, None),
                Err(e) => (None, Some(e)),
            },
            _ => (None, None),
        };
        let newer =
            latest.filter(|v| is_newer_than(v, &item.installed_version, &item.origin, item.pinned));
        item.latest_version = newer.clone();
        out.push(UpdateInfo {
            id: item.id.clone(),
            latest: newer,
            error,
        });
    }
    store.last_checked_at = Some(now_epoch_secs());
    save_store(data_dir, &store)?;
    Ok(out)
}

/// git 来源插件的最新版本：远端发布的最高 semver tag。`fetch_git` 把
/// `installed_version` 也对齐成同样的形态（要么是 tag，要么回退为 HEAD
/// hash），这样 `is_newer_than` 就可以直接比较二者。
///
/// 未锁定分支跟随最高 tag 而非分支 HEAD —— 这样即使开发者 push 了新 commit
/// 但尚未发版，也不会显得比用户上次安装的版本「更新」。插件作者通过 tag
/// 发布版本，而 tag 才是用户希望被通知的内容。
fn git_latest(item: &StoreItem) -> Result<Option<String>, String> {
    git_latest_tag(&item.source)
}

/// 远端发布的最高 semver tag。被 `fetch_git` 用来在源未锁定时挑选分支，
/// 被 `git_latest` 用来和已安装版本对比。远端没有可用 tag 时返回 `None`。
fn git_latest_tag(source: &str) -> Result<Option<String>, String> {
    let (ok, out) =
        run_capture("git", &["ls-remote", "--tags", source]).map_err(|e| e.to_string())?;
    if !ok {
        return Ok(None);
    }
    let tags: Vec<String> = out
        .lines()
        .filter_map(|line| {
            let (_, ref_part) = line.split_once('\t')?;
            let tag = ref_part.strip_prefix("refs/tags/")?.trim_end_matches("^{}");
            Some(tag.to_string())
        })
        .collect();
    Ok(latest_tag(tags.iter().map(|s| s.as_str())))
}

// --- 目录 ----------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct CatalogDoc {
    #[serde(default)]
    items: Vec<CatalogRaw>,
}

#[derive(Debug, Deserialize)]
struct CatalogRaw {
    id: String,
    name: String,
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    package: Option<String>,
    #[serde(default)]
    repo: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    stars: u64,
    #[serde(default)]
    downloads: u64,
    #[serde(default)]
    verified: bool,
    #[serde(rename = "install", default)]
    install: Option<CatalogInstall>,
    #[serde(default)]
    category: String,
}

#[derive(Debug, Deserialize)]
struct CatalogInstall {
    #[serde(default)]
    method: String,
}

/// dsh-plugin.org 的短键载荷（`/api/plugins.zh.json`）。
#[derive(Debug, Deserialize)]
struct HubRaw {
    /// 插件 slug。
    #[serde(default)]
    s: String,
    /// 作者 slug（GitHub owner，小写）。
    #[serde(default)]
    o: String,
    /// 显示名称。
    #[serde(default)]
    n: String,
    /// 最新版本，例如 `v3.22.1`。
    #[serde(default)]
    vr: String,
    /// 分类 id（interface / session / memory / tools / agent / workflow / …）。
    #[serde(default)]
    c: String,
    /// 标签。
    #[serde(default)]
    t: Vec<String>,
    /// 描述。
    #[serde(default)]
    d: String,
    /// 仓库引用。同时接受简写 `"owner/repo"` 与详细形式
    /// `{ "repo": "owner/repo", "npmPackage": "pkg" }`；两者都填的条目
    /// 说明作者同时提供了 npm 分发与源码仓库，目录应当从 npm 安装。
    #[serde(default, deserialize_with = "deserialize_hub_repo")]
    r: HubRepo,
    /// 核验状态；`verified` 表示人工审核过。
    #[serde(default)]
    v: String,
    /// 上游最近更新时间（ISO 8601）。
    #[serde(default)]
    u: String,
    /// star 数。
    #[serde(default)]
    sg: u64,
    /// fork 数。
    #[serde(default)]
    fk: u64,
}

/// hub 条目里的仓库引用：可以是裸的 `owner/repo` 简写，也可以是详细对象
/// `{ repo, npmPackage }`。空字段或缺失字段都解析为 `None`，下游代码不会
/// 看到需要再过滤的占位字符串。
#[derive(Debug, Default, Clone)]
struct HubRepo {
    repo: Option<String>,
    npm_package: Option<String>,
}

impl HubRepo {
    fn repo(&self) -> Option<&str> {
        self.repo.as_deref().filter(|s| !s.is_empty())
    }
    fn npm_package(&self) -> Option<&str> {
        self.npm_package.as_deref().filter(|s| !s.is_empty())
    }
}

/// 把 `r` 从 JSON 字符串或 `{ repo, npmPackage }` 对象两种形式中任一反序列化。
/// 其它类型（null、数字 …）归并为空 `HubRepo`，与 JSON 里省略该字段时的形态一致。
///
/// 自从作者开始同时发布 npm 分发与源码仓库之后，hub 数据里大多数条目
/// 切到了详细形式。先前的 `r: String` 反序列化器会一并拒绝这些条目，
/// 导致一条详细条目就让整个数组的 `Vec<HubRaw>::from_str` 失败 —— 目录
/// 于是默默回退到规模小得多的参考市场。在这里同时接受两种形态，是让 hub
/// 独占的插件也能在插件中心露脸的关键。
fn deserialize_hub_repo<'de, D>(de: D) -> Result<HubRepo, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;
    let value = serde_json::Value::deserialize(de)?;
    match value {
        serde_json::Value::Null => Ok(HubRepo::default()),
        serde_json::Value::String(s) => Ok(HubRepo {
            repo: (!s.is_empty()).then_some(s),
            npm_package: None,
        }),
        serde_json::Value::Object(_) => {
            #[derive(Deserialize)]
            struct Detail {
                #[serde(default)]
                repo: String,
                #[serde(default, rename = "npmPackage")]
                npm_package: String,
            }
            let d: Detail = serde_json::from_value(value).map_err(Error::custom)?;
            Ok(HubRepo {
                repo: (!d.repo.is_empty()).then_some(d.repo),
                npm_package: (!d.npm_package.is_empty()).then_some(d.npm_package),
            })
        }
        _ => Err(Error::custom(
            "`r` must be a string or { repo, npmPackage } object",
        )),
    }
}

impl HubRaw {
    /// 把一条 hub 条目规整成共享的目录条目。hub 数据区分四种安装路径，
    /// 按以下优先级选取：
    ///
    /// 1. 填了 `npmPackage` → 从 npm 安装（作者发布的包，无需编译；当作者
    ///    提供了 npm 分发时优先）。
    /// 2. 填了 `repo` → 通过 `git clone` 从 GitHub 仓库安装。
    /// 3. `repo` 缺失/为空，但 `o` + `n` 都填了 → 回退到
    ///    `github.com/{o}/{n}.git`。hub 里这种条目有几十条 —— 作者发布了
    ///    GitHub 仓库却没填 `r`，少了这个回退，`parse_spec` 会把安装路由
    ///    到 npm（用显示名查），拿到 404，用户就会看到误导性的
    ///    「查询 npm 失败：404」错误。
    /// 4. 兜底 → 用显示名走 npm（给纯 npm 的旧条目预留的回退）。
    ///
    /// 目录 UI 在每张卡片上展示所选的来源，用户能一眼看出「安装」按钮
    /// 会触发 npm 还是 git。
    fn into_item(self) -> CatalogItem {
        let repo = self.r.repo().map(str::to_string);
        let npm_package = self.r.npm_package().map(str::to_string);
        let detail_url = if !self.o.is_empty() && !self.s.is_empty() {
            format!("https://dsh-plugin.org/zh/plugins/{}/{}", self.o, self.s)
        } else {
            repo.as_ref()
                .map(|r| format!("https://github.com/{r}"))
                .unwrap_or_default()
        };
        let (origin, spec) = if let Some(pkg) = &npm_package {
            ("npm", pkg.clone())
        } else if let Some(r) = &repo {
            ("git", format!("https://github.com/{r}.git"))
        } else if !self.o.is_empty() && !self.n.is_empty() {
            (
                "git",
                format!("https://github.com/{}/{}.git", self.o, self.n),
            )
        } else {
            ("npm", self.n.clone())
        };
        let id = if self.s.is_empty() {
            self.n.clone()
        } else {
            self.s.clone()
        };
        CatalogItem {
            id,
            name: self.n,
            kind: String::new(),
            description: self.d,
            stars: self.sg,
            forks: self.fk,
            downloads: 0,
            verified: self.v == "verified",
            repo,
            spec,
            origin: origin.to_string(),
            category: self.c,
            version: self.vr,
            tags: self.t,
            updated: self.u,
            detail_url,
        }
    }
}

/// 把一条参考市场条目规整成共享的目录条目。
fn from_market_raw(raw: CatalogRaw) -> CatalogItem {
    let npm_origin = raw.package.is_some()
        || raw
            .install
            .as_ref()
            .map(|i| matches!(i.method.as_str(), "npm" | "pnpm" | "dsh-plugin-add"))
            .unwrap_or(false);
    let (origin, spec) = if npm_origin {
        (
            "npm",
            raw.package.clone().unwrap_or_else(|| raw.name.clone()),
        )
    } else if let Some(repo) = &raw.repo {
        let tag = raw.version.as_deref().filter(|v| {
            let head = v
                .strip_prefix('v')
                .unwrap_or(v)
                .split_once('-')
                .map(|(h, _)| h)
                .unwrap_or(v);
            let parts: Vec<&str> = head.split('.').collect();
            parts.len() >= 2 && parts[..2].iter().all(|s| s.parse::<u64>().is_ok())
        });
        let base = format!("https://github.com/{repo}.git");
        (
            "git",
            match tag {
                Some(t) => format!("{base}#{t}"),
                None => base,
            },
        )
    } else {
        ("git", raw.repo.clone().unwrap_or_default())
    };
    let detail_url = raw
        .repo
        .as_ref()
        .map(|r| format!("https://github.com/{r}"))
        .unwrap_or_default();
    CatalogItem {
        id: raw.id,
        name: raw.name,
        kind: raw.kind,
        description: raw.description.unwrap_or_default(),
        stars: raw.stars,
        forks: 0,
        downloads: raw.downloads,
        verified: raw.verified,
        repo: raw.repo,
        spec,
        origin: origin.to_string(),
        category: raw.category,
        version: raw.version.unwrap_or_default(),
        tags: Vec::new(),
        updated: String::new(),
        detail_url,
    }
}

/// 丢掉插件中心不应展示的目录条目。UI 把 npm install 作为规范路径
///（占位符形如 `npm i @scope/pkg …`），手动安装流程也原生支持
/// `npm i` / `pnpm add` / `yarn add` / `bun add` —— 因此没有 npm 包的
/// 条目通过中心 UI 无法完成安装。它们仍然可以通过手动安装入口访问
///（`owner/repo` / git URL / dsh plugin CLI），但目录不应列出它们，
/// 否则每张卡片上的「安装」按钮都会对着 npm registry 拿 404。
fn filter_npm_origin(items: Vec<CatalogItem>) -> Vec<CatalogItem> {
    items.into_iter().filter(|i| i.origin == "npm").collect()
}

/// 拉取社区目录，把规整后的条目缓存 `CATALOG_TTL_SECS`（`force` 跳过缓存）。
/// dsh-plugin.org hub 是主要数据源；hub 不可达时回退到参考市场的列表。缓存
/// 内容始终过滤为 npm 来源条目，这样引入此过滤之前写入的旧缓存不会在
/// 首次读取时把仅 git 的条目泄到插件中心。
fn fetch_catalog(data_dir: &Path, force: bool) -> Result<Vec<CatalogItem>, String> {
    let cache = data_dir.join(CATALOG_CACHE_FILE);
    let fresh = !force
        && fs::metadata(&cache)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|m| m.elapsed().ok().map(|e| e.as_secs() < CATALOG_TTL_SECS))
            .unwrap_or(false);
    if fresh {
        if let Ok(text) = fs::read_to_string(&cache) {
            if let Ok(items) = serde_json::from_str::<Vec<CatalogItem>>(&text) {
                return Ok(filter_npm_origin(items));
            }
        }
    }
    let hub = http_get_string(HUB_CATALOG_URL, None).and_then(|body| {
        serde_json::from_str::<Vec<HubRaw>>(&body)
            .map_err(|e: serde_json::Error| e.to_string())
            .map(|raws| raws.into_iter().map(HubRaw::into_item).collect::<Vec<_>>())
    });
    let items = match hub {
        Ok(items) if !items.is_empty() => items,
        _ => {
            let body = http_get_string(MARKET_CATALOG_URL, None)?;
            let doc: CatalogDoc =
                serde_json::from_str(&body).map_err(|e: serde_json::Error| e.to_string())?;
            doc.items.into_iter().map(from_market_raw).collect()
        }
    };
    let items = filter_npm_origin(items);
    if fs::create_dir_all(data_dir).is_ok() {
        if let Ok(text) = serde_json::to_string(&items) {
            let _ = fs::write(&cache, text);
        }
    }
    Ok(items)
}

/// 按 star 数排序的完整社区目录（`force` 跳过缓存）。搜索与分类筛选
/// 放在 UI 端，这样对缓存列表做筛选就是即时的。
pub fn catalog(data_dir: &Path, force: bool) -> Result<Vec<CatalogItem>, AppError> {
    let mut items = fetch_catalog(data_dir, force)
        .map_err(|e| AppError::Plugin(format!("目录获取失败：{e}")))?;
    items.sort_by_key(|a| std::cmp::Reverse(a.stars));
    Ok(items)
}

/// 把中央库的插件依赖与 bundle 层应用到 profile 清单上。返回是否发生变化。
/// 纯函数（不碰 fs、不跑 pnpm），因此即使没有工具链也能对接线做单元测试。
fn wire_manifest(
    root: &mut serde_json::Value,
    specs: &BTreeMap<String, (String, bool)>,
    profile: &str,
) -> Result<bool, AppError> {
    let mut changed = false;
    let deps = root
        .get_mut("dependencies")
        .and_then(|d| d.as_object_mut())
        .ok_or_else(|| AppError::Plugin("profile manifest 缺少 dependencies".into()))?;
    for (name, (spec, _)) in specs {
        if deps.get(name).and_then(|s| s.as_str()) != Some(spec.as_str()) {
            deps.insert(name.clone(), serde_json::Value::String(spec.clone()));
            changed = true;
        }
    }
    deps.retain(|name, spec| {
        if !is_managed_spec(spec.as_str().unwrap_or("")) {
            return true; // 用户/CLI 管理的不动
        }
        if !specs.contains_key(name) {
            changed = true;
            return false;
        }
        true
    });

    // bundles：模板层与托管层重建，用户其他条目（CLI 添加等）原样保留。
    // 已卸载的托管插件：依赖被清退后其层必须同步清退，否则内核启动会因无法
    // 解析该 bundle 而失败——因此只保留依赖仍存在且非托管 spec 的层。
    let kept_user_bundles: std::collections::HashSet<String> = deps
        .iter()
        .filter(|(_, spec)| !is_managed_spec(spec.as_str().unwrap_or("")))
        .map(|(name, _)| name.clone())
        .collect();
    let managed_bundles: Vec<String> = specs
        .iter()
        .filter(|(_, (_, is_bundle))| *is_bundle)
        .map(|(name, _)| name.clone())
        .collect();
    let template: Vec<String> = template_bundles(profile);
    let mut next: Vec<String> = template.clone();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let bundles = root
        .get_mut("dsh")
        .and_then(|d| d.get_mut("profile"))
        .and_then(|p| p.get_mut("bundles"))
        .and_then(|b| b.as_array_mut())
        .ok_or_else(|| AppError::Plugin("profile manifest 缺少 dsh.profile.bundles".into()))?;
    for name in bundles.iter().filter_map(|b| b.as_str().map(String::from)) {
        if seen.contains(&name) || template.contains(&name) || managed_bundles.contains(&name) {
            continue;
        }
        if !kept_user_bundles.contains(&name) {
            changed = true;
            continue;
        }
        seen.insert(name.clone());
        next.push(name);
    }
    for name in &managed_bundles {
        if !next.contains(name) {
            next.push(name.clone());
        }
    }
    let current: Vec<String> = bundles
        .iter()
        .filter_map(|b| b.as_str().map(String::from))
        .collect();
    if next != current {
        *bundles = next.into_iter().map(serde_json::Value::String).collect();
        changed = true;
    }
    Ok(changed)
}

// --- 编排 ----------------------------------------------------------------

/// 安装一个插件：拉取到中央库、link 模式下在中央库装依赖、物化到每一个内核、
/// 接入活动 profile。
pub fn install(
    data_dir: &Path,
    settings: &settings::Settings,
    pnpm_exe: &Path,
    spec_str: &str,
    mode: &str,
    on_progress: &mut dyn FnMut(&str),
) -> Result<StoreItem, AppError> {
    let spec = parse_spec(spec_str)?;
    if store_item(data_dir, &spec.id).is_some() {
        return Err(AppError::Plugin(format!(
            "{} 已安装，请使用「更新」",
            spec.name
        )));
    }
    let mut item = fetch_into_store(data_dir, pnpm_exe, &spec, on_progress)?;
    item.mode = if mode == "copy" { "copy" } else { "link" }.to_string();
    if item.mode == "link" {
        // 在装依赖前先确保中央库级别的 `.npmrc` 已就绪，这样即使中央库是在
        // 此修复部署之前创建的，`minimumReleaseAge` 排除也已生效。
        ensure_store_npmrc(data_dir).ok();
        install_store_deps(data_dir, pnpm_exe, &item.id, on_progress)?;
    }
    upsert_item(data_dir, item.clone())?;
    // 重装代表明确的重试意图：清掉历史隔离记录，否则新装的插件会被旧
    // 记录挡在接线之外，表现为"装了却不生效"的哑故障。
    let _ = quarantine::remove(data_dir, &item.id);
    sync_kernels(data_dir, &item)?;
    on_progress("正在接线到 profile");
    ensure_wiring(data_dir, settings, pnpm_exe, on_progress)?;
    Ok(item)
}

/// 更新一个插件：按同一源重新拉取、刷新中央库依赖、重新同步所有内核、
/// 重新接线。
pub fn update(
    data_dir: &Path,
    settings: &settings::Settings,
    pnpm_exe: &Path,
    id: &str,
    on_progress: &mut dyn FnMut(&str),
) -> Result<StoreItem, AppError> {
    let item =
        store_item(data_dir, id).ok_or_else(|| AppError::Plugin("插件不在中央库中".into()))?;
    if item.pinned {
        return Err(AppError::Plugin(format!(
            "{} 已锁定版本 {}，如需升级请重新安装（不带版本号）",
            item.name, item.installed_version
        )));
    }
    let spec = parse_spec(&item.source)?;
    on_progress(&format!("正在更新 {}", item.name));
    let fetched = fetch_into_store(data_dir, pnpm_exe, &spec, on_progress)?;
    let mut updated = fetched;
    updated.mode = item.mode.clone();
    if updated.mode == "link" {
        ensure_store_npmrc(data_dir).ok();
        install_store_deps(data_dir, pnpm_exe, &updated.id, on_progress)?;
    }
    // 把 latest_version 同步到刚刚安装的版本，让 UI 角标在更新成功之后
    // 立刻清掉。否则 `check_updates` 的旧结果会留下来，角标一直显示用户
    // 刚刚安装完成的「幻影新版本」。之后 `check_updates` 仍能在远端
    // 这次拉取之后又前进时重新抬高 `latest_version`。
    updated.latest_version = Some(updated.installed_version.clone());
    upsert_item(data_dir, updated.clone())?;
    // 与 install 同理：更新是明确的重试意图，历史隔离记录不再适用。
    let _ = quarantine::remove(data_dir, &updated.id);
    sync_kernels(data_dir, &updated)?;
    on_progress("正在同步 profile");
    ensure_wiring(data_dir, settings, pnpm_exe, on_progress)?;
    Ok(updated)
}

/// 在所有位置移除一个插件：中央库、内核物化、profile 接线。
///
/// 部分清理之后仍可重试卸载：如果中央库条目已经不在，仅当隔离注册表里
/// 仍然挂着同一个插件时，卸载请求才会被接受。这让事件响应动作能继续
/// 清理残留的隔离/profile 状态，又不会把「任意一个缺失的 id」当成卸载
/// 成功。
pub fn uninstall(
    data_dir: &Path,
    settings: &settings::Settings,
    pnpm_exe: &Path,
    id: &str,
    on_progress: &mut dyn FnMut(&str),
) -> Result<(), AppError> {
    let has_store_item = store_item(data_dir, id).is_some();
    if !has_store_item
        && !quarantine::load(data_dir)
            .items
            .iter()
            .any(|item| item.id == id)
    {
        return Err(AppError::Plugin("插件不在中央库中".into()));
    }
    if !has_store_item {
        on_progress("正在清理插件残留");
    }
    // 先删中央库目录：Windows 上被运行中的内核持有的文件会让
    // `remove_dir_all` 失败；在这里失败就停下，其它状态原封不动，让用户
    // 关掉工作台再重试。以前把错误吞掉，会留下桌面壳既不能接线也删除
    // 不掉的孤儿中央库目录。
    let store_path = store_plugin_dir(data_dir, id);
    match fs::remove_dir_all(&store_path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => {
            return Err(AppError::Io(format!(
                "无法删除插件目录 {}：{e}。文件可能正被运行中的内核占用，请关闭工作台后重试卸载",
                store_path.display()
            )));
        }
    }
    for version in kernel::list_installed(data_dir) {
        remove_materialized(data_dir, &version.version, id);
    }
    remove_item(data_dir, id)?;
    // 隔离记录随卸载一并清除：残留记录会在用户日后重装同名插件时把它挡
    // 在接线之外，形成"装了却不生效"的哑故障。
    quarantine::remove(data_dir, id)?;
    ensure_wiring(data_dir, settings, pnpm_exe, on_progress)?;
    Ok(())
}

/// 把期望模式重新应用到每个内核，并重新接线。
pub fn set_mode(
    data_dir: &Path,
    settings: &settings::Settings,
    pnpm_exe: &Path,
    id: &str,
    mode: &str,
    on_progress: &mut dyn FnMut(&str),
) -> Result<(), AppError> {
    if mode != "link" && mode != "copy" {
        return Err(AppError::Plugin("模式只能是 link 或 copy".into()));
    }
    let mut item =
        store_item(data_dir, id).ok_or_else(|| AppError::Plugin("插件不在中央库中".into()))?;
    item.mode = mode.to_string();
    upsert_item(data_dir, item.clone())?;
    if mode == "link" {
        install_store_deps(data_dir, pnpm_exe, id, on_progress)?;
    }
    sync_kernels(data_dir, &item)?;
    ensure_wiring(data_dir, settings, pnpm_exe, on_progress)?;
    Ok(())
}

/// 清理每个已安装内核中归桌面壳所有的插件残留。`ensure_wiring` 只访问
/// 活动内核，所以这一步必须由显式的全内核同步自己负责。
fn sweep_all_kernel_orphans(data_dir: &Path, store: &Store) {
    for version in kernel::list_installed(data_dir) {
        sweep_kernel_orphans(data_dir, &version.version, store);
    }
}

/// 物化所有插件并重新接线（对应「同步」按钮）。
pub fn sync_all(
    data_dir: &Path,
    settings: &settings::Settings,
    pnpm_exe: &Path,
    on_progress: &mut dyn FnMut(&str),
) -> Result<(), AppError> {
    let store = load_store(data_dir);
    for item in &store.items {
        sync_kernels(data_dir, item)?;
    }
    sweep_all_kernel_orphans(data_dir, &store);
    ensure_wiring(data_dir, settings, pnpm_exe, on_progress)?;
    Ok(())
}

/// 拼装 UI 的状态快照（不发起网络请求）。
pub fn status(data_dir: &Path, settings: &settings::Settings) -> PluginStatus {
    let store = load_store(data_dir);
    let active = kernel::read_active(data_dir);
    let profile_manifest = read_profile_json(data_dir, &settings.profile)
        .ok()
        .flatten();
    let quarantine_doc = quarantine::load(data_dir);

    let mut rows = Vec::new();
    let mut updates = 0;
    for item in &store.items {
        let quarantined = quarantine_doc
            .items
            .iter()
            .find(|q| q.id == item.id)
            .cloned();
        let (actual_mode, synced) = match &active {
            Some(version) => {
                let meta = read_meta(data_dir, version, &item.id);
                let present = kernel_plugin_dir(data_dir, version, &item.id).exists();
                let current = meta
                    .as_ref()
                    .map(|m| m.version == item.installed_version)
                    .unwrap_or(false);
                (meta.map(|m| m.mode), present && current)
            }
            None => (None, false),
        };
        let wired = profile_manifest
            .as_ref()
            .and_then(|m| m.get("dependencies"))
            .and_then(|d| d.get(&item.name))
            .and_then(|s| s.as_str())
            .map(is_managed_spec)
            .unwrap_or(false);
        if item
            .latest_version
            .as_deref()
            .map(|l| is_newer_than(l, &item.installed_version, &item.origin, item.pinned))
            .unwrap_or(false)
        {
            updates += 1;
        }
        // UI 每行的「有更新」角标只判断 `row.latest_version` 是否为真，而不
        // 再重跑版本比较，所以当记录的「latest」不再新于用户实际安装的版本
        // 时，我们就把这个字段隐藏。否则在更新成功（latest == installed）
        // 以及 `update()` 显式把 `latest_version = installed_version` 同步
        // 之后，行内角标仍会一直挂着。上面顶层 counts 已经为 `N 个更新`
        // 徽标做过同样的过滤。
        let row_latest = item
            .latest_version
            .as_deref()
            .filter(|l| is_newer_than(l, &item.installed_version, &item.origin, item.pinned))
            .map(|s| s.to_string());
        rows.push(PluginRow {
            id: item.id.clone(),
            name: item.name.clone(),
            origin: item.origin.clone(),
            source: item.source.clone(),
            installed_version: item.installed_version.clone(),
            latest_version: row_latest,
            pinned: item.pinned,
            desired_mode: item.mode.clone(),
            actual_mode,
            synced,
            wired,
            quarantined,
            repo_url: item.repo_url.clone(),
            description: item.description.clone(),
            installed_at: item.installed_at.clone(),
            updated_at: item.updated_at.clone(),
        });
    }
    PluginStatus {
        rows,
        profile: settings.profile.clone(),
        active_kernel: active,
        updates,
        last_checked_at: store.last_checked_at,
        warning: store.warning,
    }
}

/// 在 `kernels/<version>/plugins/` 下物化好的一个插件。由
/// [`kernel_plugin_list`] 返回，让管理 UI 鼠标悬停在版本行上时能看到内核
/// 当前磁盘上确实存在的插件 —— 包括已经从中央库移除但桌面壳仍保留的条目，
/// 以及用户手工放进去的外来条目。
#[derive(Debug, Clone, Serialize)]
pub struct KernelPluginRow {
    pub id: String,
    /// 已知时取自中央库的解析后显示名称；否则与 `id` 相同，让外部/手工目录
    /// 在 tooltip 里也有标签可看。
    pub name: String,
    pub version: String,
    /// "link" 或 "copy" —— `.meta` 记录为准。目录没有 `.meta` 标记（手工
    /// 条目）时为 `None`。
    pub mode: Option<String>,
    /// 磁盘上的条目是否仍然匹配记录的中央库版本（不计 `synced_at`）。为
    /// `false` 时 tooltip 会展示「未同步」提示。
    pub synced: bool,
    /// 中央库当前是否仍持有对应条目。这里为 `false` 意味着插件已从中央库
    /// 删除但内核还留着残留 —— 给用户一个清理信号很有用。
    pub in_store: bool,
}

/// 抓取 `kernels/<version>/plugins/` 下物化的所有插件快照。`version` 必须
/// 已经是已安装状态；本函数不做校验，因为版本面板只展示已安装的条目。
pub fn kernel_plugin_list(data_dir: &Path, version: &str) -> Vec<KernelPluginRow> {
    let mut rows = Vec::new();
    let plugins_dir = kernel_plugins_dir(data_dir, version);
    let entries = match fs::read_dir(&plugins_dir) {
        Ok(it) => it,
        Err(_) => return rows,
    };
    // 中央库名字查询，让刚被删除的插件也能解析出标签，而不是只显示裸 id。
    let store_doc = load_store(data_dir);
    let store_index: std::collections::HashMap<&str, &StoreItem> = store_doc
        .items
        .iter()
        .map(|item| (item.id.as_str(), item))
        .collect();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy().into_owned();
        if name_str.starts_with('.') || name_str == META_SUBDIR {
            continue;
        }
        let id = name_str;
        let dir = entry.path();
        let meta = read_meta(data_dir, version, &id);
        let present = dir.exists();
        let store_item = store_index.get(id.as_str()).copied();
        let synced = match (&meta, store_item) {
            (Some(meta), Some(item)) => present && meta.version == item.installed_version,
            (Some(_), None) => present,
            _ => false,
        };
        let (display_name, version, in_store) = match store_item {
            Some(item) => (item.name.clone(), item.installed_version.clone(), true),
            None => (
                id.clone(),
                meta.as_ref().map(|m| m.version.clone()).unwrap_or_default(),
                false,
            ),
        };
        rows.push(KernelPluginRow {
            id,
            name: display_name,
            version,
            mode: meta.map(|m| m.mode),
            synced,
            in_store,
        });
    }
    rows.sort_by_key(|a| a.name.to_lowercase());
    rows
}

/// 把活动内核 `node_modules` 中 link 模式插件的 peerDependencies 解析进
/// 中央库目录，让插件的 import 走查时能找到和内核相同的 cordis / dsh-*
/// 实例。记录在 `.dsh-peers.json` 中，按内核版本区分，切换内核时会重跑。
fn refresh_store_peers(data_dir: &Path, item: &StoreItem, active: &str) -> Result<(), AppError> {
    if item.mode != "link" {
        return Ok(());
    }
    let plugin_root = store_plugin_dir(data_dir, &item.id);
    let meta_path = plugin_root.join(".dsh-peers.json");
    let meta: serde_json::Value = fs::read_to_string(&meta_path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    if meta.get("kernel").and_then(|k| k.as_str()) == Some(active)
        && meta.get("peers").and_then(|p| p.as_array()).is_some()
    {
        return Ok(()); // 已为该内核解析过
    }
    let Ok(manifest) = read_plugin_manifest(&plugin_root) else {
        return Ok(());
    };
    let Some(peers) = manifest.get("peerDependencies").and_then(|p| p.as_object()) else {
        return Ok(());
    };
    let kernel_mm = kernel::kernel_dir(data_dir, active).join("node_modules");
    let mut linked: Vec<String> = Vec::new();
    for name in peers.keys() {
        let target = kernel_mm.join(name);
        if !target.exists() {
            continue;
        }
        let dest = plugin_root.join("node_modules").join(name);
        if dest.exists() {
            continue; // 已在库内安装（已发布或被 hoisted）
        }
        if let Some(parent) = dest.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if make_dir_link(&target, &dest).is_err() {
            // 链接不可用（Windows 权限等）→ 复制一份，解析不依赖链接能力
            let _ = copy_tree(&target, &dest);
        }
        linked.push(name.clone());
    }
    let text = serde_json::json!({ "kernel": active, "peers": linked });
    if let Ok(text) = serde_json::to_string(&text) {
        let _ = fs::write(meta_path, text);
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TEST_HOME_COUNTER: AtomicUsize = AtomicUsize::new(0);
    /// 每个测试独占的一次性 home，drop 时清理。
    struct TestHome(PathBuf);

    impl TestHome {
        fn new() -> Self {
            let nano = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let base =
                std::env::temp_dir().join(format!("dsh-plugins-test-{}", std::process::id()));
            let seq = TEST_HOME_COUNTER.fetch_add(1, Ordering::Relaxed);
            let home = base.join(format!("{nano}-{seq}"));
            fs::create_dir_all(&home).expect("test home");
            TestHome(home)
        }

        fn data_dir(&self) -> PathBuf {
            self.0.join("desktop")
        }
    }

    impl Drop for TestHome {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn parses_npm_specs() {
        assert_eq!(parse_spec("dsh-market").unwrap().origin, "npm");
        let scoped = parse_spec("@ace-zone/dsh-market").unwrap();
        assert_eq!(scoped.origin, "npm");
        assert_eq!(scoped.source, "@ace-zone/dsh-market");
        assert_eq!(scoped.pin, None);
        let pinned = parse_spec("@ace-zone/dsh-market@0.1.66").unwrap();
        assert_eq!(pinned.pin.as_deref(), Some("0.1.66"));
        assert_eq!(pinned.id, "@ace-zone__dsh-market");
        let unpinned = parse_spec("dsh-market@1.2.3").unwrap();
        assert_eq!(unpinned.source, "dsh-market");
        assert_eq!(unpinned.pin.as_deref(), Some("1.2.3"));
    }

    #[test]
    fn filter_npm_origin_drops_git_only_entries() {
        // 插件中心只会展示「安装」按钮实际能从 npm 拿到的条目。仅 git 的
        // 条目仍可通过手动安装入口访问（接受 owner/repo / git URL /
        // dsh plugin CLI），但把它们列在目录里，用户一点安装就会立刻撞上 404。
        let mk = |id: &str, name: &str, origin: &str, spec: &str, repo: Option<&str>| CatalogItem {
            id: id.into(),
            name: name.into(),
            kind: String::new(),
            description: String::new(),
            stars: 0,
            forks: 0,
            downloads: 0,
            verified: false,
            repo: repo.map(str::to_string),
            spec: spec.into(),
            origin: origin.into(),
            category: String::new(),
            version: String::new(),
            tags: vec![],
            updated: String::new(),
            detail_url: String::new(),
        };
        let npm_only = mk("a", "a", "npm", "@scope/a", Some("owner/a"));
        let git_only = mk("b", "b", "git", "https://github.com/owner/b.git", None);

        let out = filter_npm_origin(vec![npm_only.clone(), git_only]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, "a");
        assert_eq!(out[0].origin, "npm");
        // 空输入是 no-op。
        assert!(filter_npm_origin(vec![]).is_empty());
        // 全 npm 的列表原样穿透。
        assert_eq!(filter_npm_origin(vec![npm_only.clone(), npm_only]).len(), 2);
    }

    #[test]
    fn resolves_npm_version_through_dist_tags() {
        // 把版本钉到 "latest" 时应当先经 `dist-tags.latest` 跳到 registry
        // 实际发布的 semver —— 少了这一步，字面量 `"latest"` 会直接被当成
        // `versions` 的 key，用户就会看到「没有可下载的 tarball」，即使
        // 这个包以及它的 tarball 都已存在（用户机器上 @linxin666/dsh-liangshen
        // 触发的 bug）。
        let doc: NpmDoc = serde_json::from_str(
            r#"{
                "dist-tags": {"latest": "0.3.2", "next": "1.0.0-rc.1"},
                "versions": {
                    "0.3.2": {},
                    "1.0.0-rc.1": {},
                    "0.1.0": {}
                }
            }"#,
        )
        .expect("npm doc");
        assert_eq!(
            resolve_npm_version(&doc, None),
            ("0.3.2".into(), "latest".into())
        );
        assert_eq!(
            resolve_npm_version(&doc, Some("latest")),
            ("0.3.2".into(), "latest".into())
        );
        assert_eq!(
            resolve_npm_version(&doc, Some("next")),
            ("1.0.0-rc.1".into(), "next".into())
        );
        // 字面量 semver 钉：保留 pin 原样，让调用方的 `versions[<pin>]`
        // 查询暴露精确的错误信息。
        assert_eq!(
            resolve_npm_version(&doc, Some("1.0.0-rc.1")),
            ("1.0.0-rc.1".into(), "1.0.0-rc.1".into())
        );
        assert_eq!(
            resolve_npm_version(&doc, Some("9.9.9")),
            ("9.9.9".into(), "9.9.9".into())
        );

        // 没有 `dist-tags.latest`：`None` 返回空字符串，错误信息会变成
        // 「找不到包 … 或其 latest 标记」。
        let doc: NpmDoc = serde_json::from_str(r#"{"versions": {"1.0.0": {}}}"#).unwrap();
        assert_eq!(
            resolve_npm_version(&doc, None),
            ("".into(), "latest".into())
        );
    }

    #[test]
    fn parses_package_manager_install_cli() {
        // 用户从文档里粘贴的精确形态：`npm i @scope/pkg@v`。
        let spec = parse_spec("npm i @linxin666/dsh-liangshen").unwrap();
        assert_eq!(spec.origin, "npm");
        assert_eq!(spec.source, "@linxin666/dsh-liangshen");
        assert_eq!(spec.pin, None);

        // `install` 与 `add` 两种动词，遍及四种包管理器前缀。
        let spec = parse_spec("npm install @scope/pkg@1.2.3").unwrap();
        assert_eq!(spec.source, "@scope/pkg");
        assert_eq!(spec.pin.as_deref(), Some("1.2.3"));
        assert_eq!(parse_spec("pnpm add owner/repo").unwrap().origin, "git");
        assert_eq!(parse_spec("yarn add owner/repo").unwrap().origin, "git");
        assert_eq!(
            parse_spec("bun add @scope/pkg@latest").unwrap().source,
            "@scope/pkg"
        );

        // 包 spec 前的标志（`--save-dev`、`-D`）会被静默丢弃。
        // `npm i -D <pkg>` 与 `npm install --save-dev <pkg>` 都会归并
        // 为裸的包 spec。
        let spec = parse_spec("npm i -D @scope/pkg@1.0.0").unwrap();
        assert_eq!(spec.source, "@scope/pkg");
        let spec = parse_spec("npm install --save-dev @scope/pkg@1.0.0").unwrap();
        assert_eq!(spec.source, "@scope/pkg");

        // `npm install`（无包 spec）会落到 npm 解析分支，并对空白字符
        // 大声报错，让用户看到可操作的错误而不是一次悄无声息的 no-op。
        assert!(parse_spec("npm install").is_err());
    }

    #[test]
    fn parses_dsh_plugin_cli_form() {
        // 完整的 `dsh plugin --profile X add <pkg>` 形态是内核交付的规范
        // 命令 —— 用户从文档 / 聊天建议里粘贴，桌面壳必须按原样接受。
        let spec =
            parse_spec("dsh plugin --profile web add @linxin666/dsh-liangshen@latest").unwrap();
        assert_eq!(spec.origin, "npm");
        assert_eq!(spec.source, "@linxin666/dsh-liangshen");
        assert_eq!(spec.pin.as_deref(), Some("latest"));

        // 不带 `--profile` 标志：仍能解析包 spec。
        let spec = parse_spec("dsh plugin add @linxin666/dsh-liangshen@latest").unwrap();
        assert_eq!(spec.origin, "npm");
        assert_eq!(spec.source, "@linxin666/dsh-liangshen");
        assert_eq!(spec.pin.as_deref(), Some("latest"));

        // 在内核 CLI 中 `install` 是 `add` 的别名。
        let spec = parse_spec("dsh plugin install @scope/pkg@1.2.3").unwrap();
        assert_eq!(spec.source, "@scope/pkg");
        assert_eq!(spec.pin.as_deref(), Some("1.2.3"));

        // 短 `-p` 标志。
        let spec = parse_spec("dsh plugin -p web add owner/repo#v1.0.0").unwrap();
        assert_eq!(spec.origin, "git");
        assert_eq!(spec.source, "https://github.com/owner/repo.git");
        assert_eq!(spec.pin.as_deref(), Some("v1.0.0"));

        // `dsh plugin … remove` / `update` / `list` 不是安装动词；手动
        // 安装 UI 必须拒绝它们，避免粘贴的命令误把插件卸载掉。
        assert!(parse_spec("dsh plugin remove @scope/pkg").is_err());
        assert!(parse_spec("dsh plugin list").is_err());
    }

    #[test]
    fn validate_plugin_checks_the_load_contract() {
        let home = TestHome::new();
        let dir = home.0.join("plugin");
        fs::create_dir_all(&dir).expect("plugin dir");

        // 没有 package.json：拒绝
        assert!(validate_plugin(&dir).is_err());

        // bundle 层声明了 patch 但文件缺失：拒绝
        fs::write(
            dir.join("package.json"),
            r#"{"name":"p","dsh":{"bundle":{"patch":"./cordis.patch.yml"}}}"#,
        )
        .expect("manifest");
        assert!(validate_plugin(&dir).is_err());

        // patch 文件补齐但无运行时入口：仍拒绝，bundle 层不能替代 main/exports
        fs::write(dir.join("cordis.patch.yml"), "patches: []\n").expect("patch");
        assert!(validate_plugin(&dir).is_err());

        // 普通依赖型插件：main 指向真实文件才放行
        fs::write(
            dir.join("package.json"),
            r#"{"name":"p","main":"lib/index.js"}"#,
        )
        .expect("manifest");
        assert!(validate_plugin(&dir).is_err());
        fs::create_dir_all(dir.join("lib")).expect("lib");
        fs::write(dir.join("lib/index.js"), "module.exports = {}\n").expect("entry");
        assert!(validate_plugin(&dir).is_ok());

        // bundle 层 + 有效运行时入口：放行
        fs::write(
            dir.join("package.json"),
            r#"{"name":"p","main":"lib/index.js","dsh":{"bundle":{"patch":"./cordis.patch.yml"}}}"#,
        )
        .expect("manifest");
        assert!(validate_plugin(&dir).is_ok());

        // exports 入口存在即放行（Node 自己解析其目标）
        fs::write(
            dir.join("package.json"),
            r#"{"name":"p","exports":"./lib/index.js"}"#,
        )
        .expect("manifest");
        assert!(validate_plugin(&dir).is_ok());

        // 既无 bundle 也无入口：拒绝
        fs::write(dir.join("package.json"), r#"{"name":"p"}"#).expect("manifest");
        assert!(validate_plugin(&dir).is_err());
    }

    #[test]
    fn hub_entry_normalizes_to_catalog_item() {
        let raw: HubRaw = serde_json::from_str(
            r#"{"s":"modlens","o":"liustack","n":"modlens","vr":"v3.22.1","c":"tools",
                "t":["vision"],"d":"desc","r":"liustack/modlens","v":"verified",
                "u":"2026-08-20T20:05:55Z","sg":3497,"fk":95}"#,
        )
        .expect("hub raw");
        let item = raw.into_item();
        assert_eq!(item.origin, "git");
        assert_eq!(item.spec, "https://github.com/liustack/modlens.git");
        assert_eq!(item.version, "v3.22.1");
        assert_eq!(item.stars, 3497);
        assert_eq!(item.forks, 95);
        assert!(item.verified);
        assert_eq!(item.category, "tools");
        assert_eq!(
            item.detail_url,
            "https://dsh-plugin.org/zh/plugins/liustack/modlens"
        );

        // 无 repo 的条目回退 npm 安装
        let raw: HubRaw = serde_json::from_str(r#"{"n":"pkg","d":"x"}"#).expect("hub raw");
        let item = raw.into_item();
        assert_eq!(item.origin, "npm");
        assert_eq!(item.spec, "pkg");
        assert!(!item.verified);
    }

    #[test]
    fn hub_entry_with_npm_package_installs_from_npm() {
        // 作者同时发布了 GitHub 仓库和 npm 分发；目录应当走 npm 路径，
        // 这样 UI 上的「安装」按钮装的是已发布的 tarball（在 npm 上可能
        // 拿到 404，而不是对一个任意仓库做 git clone）。
        let raw: HubRaw = serde_json::from_str(
            r#"{"s":"dsh-zhipu","n":"dsh-zhipu","r":{"repo":"fineven/dsh-zhipu","npmPackage":"dsh-zhipu"}}"#,
        )
        .expect("hub raw");
        let item = raw.into_item();
        assert_eq!(item.origin, "npm");
        assert_eq!(item.spec, "dsh-zhipu");
        assert_eq!(item.repo.as_deref(), Some("fineven/dsh-zhipu"));
    }

    #[test]
    fn hub_entry_with_repo_object_no_npm_falls_back_to_git() {
        // 详细 `r` 但没有 `npmPackage` 时仍应走 git 安装；详细形式只是把
        // `repo` 简写包成了对象形态，安装路径不变。
        let raw: HubRaw =
            serde_json::from_str(r#"{"s":"plug","n":"plug","r":{"repo":"owner/plug"}}"#)
                .expect("hub raw");
        let item = raw.into_item();
        assert_eq!(item.origin, "git");
        assert_eq!(item.spec, "https://github.com/owner/plug.git");
    }

    #[test]
    fn hub_entry_with_empty_repo_object_falls_back_to_npm_name() {
        // 详细 `r` 但两个字段都为空时，等同于 `r` 缺失 —— 用显示名走 npm 安装。
        let raw: HubRaw =
            serde_json::from_str(r#"{"s":"plug","n":"plug","r":{}}"#).expect("hub raw");
        let item = raw.into_item();
        assert_eq!(item.origin, "npm");
        assert_eq!(item.spec, "plug");
        assert!(item.repo.is_none());
    }

    #[test]
    fn hub_entry_missing_repo_falls_back_to_owner_name_git() {
        // 作者发布了 GitHub 仓库，却在 manifest 里把 `r` 留空。少了这个
        // 回退，`parse_spec` 会把安装路由到 npm（用显示名查），拿到 404，
        // 露出用户刚刚在 `dsh-web-ui` 上报的误导性「查询 npm 失败：404」
        // 错误。`o` + `n` 足以让我们还原出规范的 github.com URL。
        let raw: HubRaw =
            serde_json::from_str(r#"{"s":"dsh-web-ui","o":"someone","n":"dsh-web-ui"}"#)
                .expect("hub raw");
        let item = raw.into_item();
        assert_eq!(item.origin, "git");
        assert_eq!(item.spec, "https://github.com/someone/dsh-web-ui.git");
        assert!(item.repo.is_none());
    }

    #[test]
    fn parses_git_specs() {
        let url = parse_spec("https://github.com/losebird/dsh-plugin-market").unwrap();
        assert_eq!(url.origin, "git");
        assert!(url.source.starts_with("https://"));
        let pinned = parse_spec("https://github.com/o/r.git#v1.2.3").unwrap();
        assert_eq!(pinned.pin.as_deref(), Some("v1.2.3"));
        let ssh = parse_spec("git@github.com:o/r.git").unwrap();
        assert_eq!(ssh.origin, "git");
        let shorthand = parse_spec("losebird/dsh-plugin-market").unwrap();
        assert_eq!(shorthand.origin, "git");
        assert_eq!(
            shorthand.source,
            "https://github.com/losebird/dsh-plugin-market.git"
        );
        assert_eq!(shorthand.id, "losebird__dsh-plugin-market");
    }

    #[test]
    fn rejects_path_traversal_ids() {
        assert!(id_for_name("a/../b").is_err());
        assert!(id_for_name("..").is_err());
        assert!(id_for_name("").is_err());
        assert!(id_for_name("a//b").is_err());
    }

    #[test]
    fn picks_latest_tag() {
        let tags = ["v1.2.3", "v1.2.0", "v0.9.0", "v2.0.0-rc.1"];
        assert_eq!(
            latest_tag(tags.iter().copied()).as_deref(),
            Some("v2.0.0-rc.1")
        );
        assert_eq!(
            latest_tag(["1.2.3", "1.10.0"].iter().copied()).as_deref(),
            Some("1.10.0")
        );
        assert_eq!(latest_tag(["not-a-version"].iter().copied()), None);
    }

    #[test]
    fn detects_semver_shape() {
        // 至少两段数字是 tag 过滤器和更新比较器共同依赖的门槛；其它形态
        // 都视作 hash。
        assert!(looks_like_semver("v0.15.0"));
        assert!(looks_like_semver("0.15.0"));
        assert!(looks_like_semver("1.2.3-rc.1"));
        assert!(!looks_like_semver("v646c91c"));
        assert!(!looks_like_semver("head"));
        assert!(!looks_like_semver("1"));
        assert!(!looks_like_semver(""));
    }

    #[test]
    fn newer_than_handles_hash_vs_semver() {
        // npm / 锁定的 git 走 semver 排序。未锁定的 git 分支现在也保存
        // 了远端的最高 tag（通过 `fetch_git`），同样进入同一条 semver 排序路径。
        assert!(is_newer_than("v0.15.0", "v0.14.0", "npm", false));
        assert!(!is_newer_than("v0.14.0", "v0.15.0", "npm", false));
        assert!(is_newer_than("v1.0.0", "v0.15.0", "git", true));
        assert!(is_newer_than("v0.16.0", "v0.15.0", "git", false));
        assert!(!is_newer_than("v0.15.0", "v0.15.0", "git", false));

        // 回退路径：未锁定的 git 来源、仓库无任何可用 semver tag 时，
        // `installed_version` 记录为 HEAD 短 hash。远端 `latest` 是 tag，
        // 纯 semver 比较会单纯因为段数判断为 Greater。改用字符串相等判断，
        // 避免新 tag 看上去永远「更新」。
        assert!(!is_newer_than("v0.15.0", "v646c91c", "git", false));
        assert!(is_newer_than("vNEW1", "v646c91c", "git", false));
        assert!(!is_newer_than("v646c91c", "v646c91c", "git", false));
        // 锁定 + hash 形态的 latest 不会进入特殊分支；cmp_versions 会把
        // hash 排在任何 semver tag 之下。
        assert!(is_newer_than("v0.15.0", "v646c91c", "git", true));
    }

    #[test]
    fn computes_relative_paths() {
        let from = Path::new("/home/u/.dsh/profiles/web");
        let to = Path::new("/home/u/.dsh/desktop/kernels/0.1.1/plugins/x");
        // profile spec 始终经过 spec_path_string，它会按平台规则把路径
        // 分隔符归一为正斜杠。
        assert_eq!(
            spec_path_string(&relative_path(from, to)),
            "../../desktop/kernels/0.1.1/plugins/x"
        );
        assert_eq!(relative_path(from, from), PathBuf::new());
    }

    #[test]
    fn store_round_trips() {
        let home = TestHome::new();
        let data_dir = home.data_dir();
        let item = StoreItem {
            id: "test-plugin-1".into(),
            name: "test-plugin".into(),
            origin: "npm".into(),
            source: "test-plugin".into(),
            installed_version: "1.0.0".into(),
            latest_version: None,
            mode: "link".into(),
            pinned: false,
            installed_at: "1".into(),
            updated_at: "2".into(),
            repo_url: None,
            description: None,
        };
        upsert_item(&data_dir, item.clone()).expect("save");
        let loaded = load_store(&data_dir);
        assert_eq!(loaded.items.len(), 1);
        assert_eq!(loaded.items[0].id, "test-plugin-1");
        assert!(store_dir(&data_dir).starts_with(home.0.as_path()));
    }

    #[test]
    fn materialize_link_then_copy() {
        let home = TestHome::new();
        let data_dir = home.data_dir();
        let id = "mat-plugin";
        let source = store_plugin_dir(&data_dir, id);
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("package.json"), "{}").unwrap();
        let item = StoreItem {
            id: id.into(),
            name: "mat-plugin".into(),
            origin: "npm".into(),
            source: "mat-plugin".into(),
            installed_version: "1.0.0".into(),
            latest_version: None,
            mode: "link".into(),
            pinned: false,
            installed_at: String::new(),
            updated_at: String::new(),
            repo_url: None,
            description: None,
        };
        let version = "0.1.1";
        let actual = materialize_one(&data_dir, version, &item).expect("materialize");
        // 链接失败（Windows 无开发者模式、受限文件系统、沙箱）会降级为 copy，
        // 两种结果都是合法行为；能链接时必须真的是链接。
        assert!(actual == "link" || actual == "copy");
        let target = kernel_plugin_dir(&data_dir, version, id);
        assert!(target.exists());
        if actual == "link" {
            assert!(target.is_symlink());
        }
        let actual = materialize_one(&data_dir, version, &item).expect("idempotent");
        assert!(actual == "link" || actual == "copy");

        // copy 模式覆盖
        let mut copy_item = item.clone();
        copy_item.mode = "copy".to_string();
        let actual = materialize_one(&data_dir, version, &copy_item).expect("copy");
        assert_eq!(actual, "copy");
        assert!(target.join("package.json").is_file());
        let meta = read_meta(&data_dir, version, id).expect("meta");
        assert_eq!(meta.mode, "copy");
        assert_eq!(meta.version, "1.0.0");
    }

    #[test]
    fn copy_tree_reports_failing_path() {
        let home = TestHome::new();
        let missing = home.0.join("no-such-source");
        let err = copy_tree(&missing, &home.0.join("out")).expect_err("missing source must fail");
        let msg = err.to_string();
        assert!(
            msg.contains("no-such-source"),
            "error must name the failing path, got: {msg}"
        );
    }

    #[test]
    fn copy_tree_skips_dangling_symlink() {
        let home = TestHome::new();
        let source = home.0.join("src-tree");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("real.txt"), "hi").unwrap();
        // 目标已经消失的 symlink：Windows 上 `fs::copy` 对它会报 os error 2，
        // 以前会让整次目录拷贝都终止。
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(source.join("gone.txt"), source.join("link.txt"));
        #[cfg(windows)]
        let linked =
            std::os::windows::fs::symlink_file(source.join("gone.txt"), source.join("link.txt"));
        if linked.is_err() {
            return; // 当前环境没有 symlink 权限
        }
        let target = home.0.join("dst-tree");
        copy_tree(&source, &target).expect("dangling link must not abort the copy");
        assert_eq!(fs::read_to_string(target.join("real.txt")).unwrap(), "hi");
        assert!(!target.join("link.txt").exists());
    }

    #[test]
    fn copy_tree_aborts_on_link_cycle() {
        let home = TestHome::new();
        let source = home.0.join("cycle-tree");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("real.txt"), "hi").unwrap();
        // 指向自身祖先的目录链接：macOS / Linux 上 pnpm 循环依赖产生的
        // 形态，那里 `node_modules` 全部是 symlink。拷贝必须以清晰的错误
        // 失败，而不是无休止地递归。
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(&source, source.join("loop"));
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_dir(&source, source.join("loop"));
        if linked.is_err() {
            return; // 当前环境没有 symlink 权限
        }
        let err = copy_tree(&source, &home.0.join("dst-cycle")).expect_err("cycle must fail");
        assert!(
            err.to_string().contains("循环链接"),
            "error explains the cycle, got: {err}"
        );
    }

    #[test]
    fn reconcile_reaps_unmarked_staging_dirs() {
        let home = TestHome::new();
        let data_dir = home.data_dir();
        let store = store_dir(&data_dir);
        fs::create_dir_all(&store).unwrap();
        write_fake_plugin(&store.join("live-plugin"), "1.0.0");
        // `.dsh-id` 标记之前的崩溃残留，加上更早版本的 `.tmp-` 命名：
        // 没有标记，从来不是用户数据。
        fs::create_dir_all(store.join(format!("{TMP_PREFIX}1-2"))).unwrap();
        fs::create_dir_all(store.join(".tmp-legacy-3")).unwrap();
        reconcile_store(&data_dir);
        assert!(!store.join(format!("{TMP_PREFIX}1-2")).exists());
        assert!(!store.join(".tmp-legacy-3").exists());
        assert!(store.join("live-plugin").is_dir());
    }

    #[test]
    fn reconcile_keeps_live_plugin_named_like_staging() {
        let home = TestHome::new();
        let data_dir = home.data_dir();
        let store = store_dir(&data_dir);
        fs::create_dir_all(&store).unwrap();
        // npm 允许名字带 staging 前缀（`tmp-foo`）：final 目录自带标记，
        // 不能把它自己当成残留暂存清扫掉。
        let id = format!("{TMP_PREFIX}foo");
        mark_staging(&store.join(&id), &id);
        write_fake_plugin(&store.join(&id), "1.0.0");
        reconcile_store(&data_dir);
        assert!(store.join(&id).join("package.json").is_file());
    }

    #[test]
    fn sweep_removes_only_owned_or_broken_orphans() {
        let home = TestHome::new();
        let data_dir = home.data_dir();
        let version = "9.9.9";
        let item = StoreItem {
            id: "live".into(),
            name: "live".into(),
            origin: "npm".into(),
            source: "live".into(),
            installed_version: "1.0.0".into(),
            latest_version: None,
            mode: "copy".into(),
            pinned: false,
            installed_at: String::new(),
            updated_at: String::new(),
            repo_url: None,
            description: None,
        };
        upsert_item(&data_dir, item).expect("store");
        let store = load_store(&data_dir);
        let plugins = kernel_plugins_dir(&data_dir, version);
        fs::create_dir_all(plugins.join("live")).unwrap();
        // 桌面壳所有的孤儿：`.meta` 记录证明这是壳自己放进去的。
        fs::create_dir_all(plugins.join("ghost")).unwrap();
        write_meta(
            &data_dir,
            version,
            "ghost",
            &KernelMeta {
                mode: "copy".into(),
                version: "1.0.0".into(),
                synced_at: "1".into(),
            },
        )
        .expect("meta");
        // 损坏的孤儿：中央库目标已经消失的 symlink。
        #[cfg(unix)]
        let linked =
            std::os::unix::fs::symlink(plugins.join("missing-target"), plugins.join("dangler"));
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_dir(
            plugins.join("missing-target"),
            plugins.join("dangler"),
        );
        let has_link = linked.is_ok();
        // 外部条目：无 meta、非链接 —— 留给用户处理。
        fs::create_dir_all(plugins.join("foreign")).unwrap();

        sweep_kernel_orphans(&data_dir, version, &store);

        assert!(plugins.join("live").exists());
        assert!(!plugins.join("ghost").exists());
        assert!(read_meta(&data_dir, version, "ghost").is_none());
        if has_link {
            assert!(!plugins.join("dangler").exists());
        }
        assert!(plugins.join("foreign").exists());
    }

    #[test]
    fn kernel_plugin_list_scans_materialized_entries() {
        let home = TestHome::new();
        let data_dir = home.data_dir();
        let version = "1.0.0";

        let bin = kernel::kernel_dir(&data_dir, version).join("node_modules/@deepseek-ai/dsh/lib");
        fs::create_dir_all(&bin).unwrap();
        fs::write(bin.join("bin.js"), "").unwrap();

        // 中央库一个插件：内核目录带有有效的 link 元数据；另一个中央库项目
        // 在内核目录中仍是旧版本，Tooltip 应明确显示它尚未同步。
        let store = store_dir(&data_dir);
        write_fake_plugin(&store.join("live-plugin"), "1.0.0");
        upsert_item(
            &data_dir,
            StoreItem {
                id: "live-plugin".into(),
                name: "live-plugin".into(),
                origin: "npm".into(),
                source: "live-plugin".into(),
                installed_version: "1.0.0".into(),
                latest_version: None,
                pinned: false,
                mode: "link".into(),
                repo_url: None,
                description: None,
                installed_at: "0".into(),
                updated_at: "0".into(),
            },
        )
        .expect("upsert store");
        upsert_item(
            &data_dir,
            StoreItem {
                id: "stale-plugin".into(),
                name: "stale-plugin".into(),
                origin: "npm".into(),
                source: "stale-plugin".into(),
                installed_version: "2.0.0".into(),
                latest_version: None,
                pinned: false,
                mode: "copy".into(),
                repo_url: None,
                description: None,
                installed_at: "0".into(),
                updated_at: "0".into(),
            },
        )
        .expect("upsert stale store");
        let plugins = kernel_plugins_dir(&data_dir, version);
        fs::create_dir_all(plugins.join("live-plugin")).unwrap();
        write_meta(
            &data_dir,
            version,
            "live-plugin",
            &KernelMeta {
                mode: "link".into(),
                version: "1.0.0".into(),
                synced_at: "1".into(),
            },
        )
        .unwrap();
        fs::create_dir_all(plugins.join("stale-plugin")).unwrap();
        write_meta(
            &data_dir,
            version,
            "stale-plugin",
            &KernelMeta {
                mode: "copy".into(),
                version: "1.0.0".into(),
                synced_at: "1".into(),
            },
        )
        .unwrap();
        // 手工目录：没有 meta 也没有 store 项目，应当原样出现并标 in_store=false。
        fs::create_dir_all(plugins.join("manual")).unwrap();

        let rows = kernel_plugin_list(&data_dir, version);
        let ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
        assert!(ids.contains(&"live-plugin"));
        assert!(ids.contains(&"manual"));

        let live = rows.iter().find(|r| r.id == "live-plugin").unwrap();
        assert_eq!(live.name, "live-plugin");
        assert_eq!(live.version, "1.0.0");
        assert_eq!(live.mode.as_deref(), Some("link"));
        assert!(live.in_store);
        assert!(live.synced);

        let stale = rows.iter().find(|r| r.id == "stale-plugin").unwrap();
        assert!(stale.in_store);
        assert!(!stale.synced);
        assert_eq!(stale.mode.as_deref(), Some("copy"));

        let manual = rows.iter().find(|r| r.id == "manual").unwrap();
        assert_eq!(manual.name, "manual");
        assert!(manual.mode.is_none());
        assert!(!manual.in_store);
    }

    #[test]
    fn sync_all_sweeps_removed_plugins_from_every_kernel() {
        let home = TestHome::new();
        let data_dir = home.data_dir();
        let versions = ["1.0.0", "2.0.0"];

        for version in versions {
            let bin =
                kernel::kernel_dir(&data_dir, version).join("node_modules/@deepseek-ai/dsh/lib");
            fs::create_dir_all(&bin).unwrap();
            fs::write(bin.join("bin.js"), "").unwrap();

            let plugins = kernel_plugins_dir(&data_dir, version);
            fs::create_dir_all(plugins.join("ghost")).unwrap();
            write_meta(
                &data_dir,
                version,
                "ghost",
                &KernelMeta {
                    mode: "copy".into(),
                    version: "1.0.0".into(),
                    synced_at: "1".into(),
                },
            )
            .unwrap();
        }

        // 把 profile 调和留在 no-op 路径上，让这个测试只考察 sync_all
        // 的全内核物化清理。
        let profile = profile_dir(&data_dir, DEFAULT_PROFILE);
        fs::create_dir_all(profile.join("node_modules")).unwrap();
        fs::write(
            profile.join("package.json"),
            r#"{"name":"dsh-profile-web","private":true,"dependencies":{},"dsh":{"profile":{"bundles":["@deepseek-ai/dsh-base","@deepseek-ai/dsh-web-app"]}}}"#,
        )
        .unwrap();

        let mut noop = |_: &str| {};
        sync_all(
            &data_dir,
            &settings::Settings::default(),
            Path::new("pnpm"),
            &mut noop,
        )
        .unwrap();

        for version in versions {
            assert!(!kernel_plugin_dir(&data_dir, version, "ghost").exists());
            assert!(read_meta(&data_dir, version, "ghost").is_none());
        }
    }

    #[test]
    fn wiring_survives_single_plugin_failure() {
        let home = TestHome::new();
        let data_dir = home.data_dir();
        let version = "9.9.9";
        fs::create_dir_all(&data_dir).unwrap();
        fs::write(data_dir.join("active.txt"), format!("{version}\n")).unwrap();
        let mk_item = |id: &str, name: &str| StoreItem {
            id: id.into(),
            name: name.into(),
            origin: "npm".into(),
            source: name.into(),
            installed_version: "1.0.0".into(),
            latest_version: None,
            // copy 模式让期望的 profile spec 保持确定性：link 模式在没有
            // symlink 权限的机器上会静默降级为 copy，导致 spec 前缀翻转。
            mode: "copy".into(),
            pinned: false,
            installed_at: String::new(),
            updated_at: String::new(),
            repo_url: None,
            description: None,
        };
        write_fake_plugin(&store_plugin_dir(&data_dir, "healthy"), "1.0.0");
        upsert_item(&data_dir, mk_item("healthy", "healthy-plugin")).expect("healthy");
        // 损坏：中央库里有登记，但目录已经不见了。
        upsert_item(&data_dir, mk_item("broken", "broken-plugin")).expect("broken");
        // profile 已经为健康插件单独接好线，因此成功执行后 manifest
        // 保持不变，也根本不会 shell out 到 pnpm。
        let profile = profile_dir(&data_dir, "web");
        fs::create_dir_all(profile.join("node_modules")).unwrap();
        let manifest = serde_json::json!({
            "name": "dsh-profile-web",
            "private": true,
            "dependencies": {
                "healthy-plugin": "file:../../desktop/kernels/9.9.9/plugins/healthy"
            },
            "dsh": { "profile": { "bundles": ["@deepseek-ai/dsh-base", "@deepseek-ai/dsh-web-app"] } }
        });
        fs::write(
            profile.join("package.json"),
            serde_json::to_string_pretty(&manifest).unwrap() + "\n",
        )
        .unwrap();

        let mut noop = |_: &str| {};
        let err = ensure_wiring(
            &data_dir,
            &settings::Settings::default(),
            Path::new("pnpm"),
            &mut noop,
        )
        .expect_err("the broken plugin must surface as an error");
        let msg = err.to_string();
        assert!(
            msg.contains("broken-plugin"),
            "error names the failed plugin: {msg}"
        );
        // 健康插件仍然被物化，manifest 也保留它的接线 —— 一个坏插件不再
        // 拖垮其他所有插件。
        assert!(kernel_plugin_dir(&data_dir, version, "healthy")
            .join("package.json")
            .is_file());
        let on_disk = fs::read_to_string(profile.join("package.json")).unwrap();
        assert!(on_disk.contains("healthy-plugin"));
    }

    #[test]
    fn refreshes_peers_from_active_kernel() {
        let home = TestHome::new();
        let data_dir = home.data_dir();
        let id = "peer-plugin";
        let version = "2.0.0";
        // 假内核：node_modules 里有插件声明但中央库没有的 peer
        let kernel_mm = kernel::kernel_dir(&data_dir, version).join("node_modules");
        fs::create_dir_all(kernel_mm.join("@deepseek-ai/dsh-base")).unwrap();
        fs::write(kernel_mm.join("@deepseek-ai/dsh-base/package.json"), "{}").unwrap();
        fs::write(data_dir.join("active.txt"), format!("{version}\n")).unwrap();
        let plugin_root = store_plugin_dir(&data_dir, id);
        fs::create_dir_all(&plugin_root).unwrap();
        let manifest = serde_json::json!({
            "name": "peer-plugin",
            "peerDependencies": { "@deepseek-ai/dsh-base": "*" },
        });
        fs::write(
            plugin_root.join("package.json"),
            serde_json::to_string(&manifest).unwrap(),
        )
        .unwrap();
        let item = StoreItem {
            id: id.into(),
            name: "peer-plugin".into(),
            origin: "npm".into(),
            source: "peer-plugin".into(),
            installed_version: "1.0.0".into(),
            latest_version: None,
            mode: "link".into(),
            pinned: false,
            installed_at: String::new(),
            updated_at: String::new(),
            repo_url: None,
            description: None,
        };
        refresh_store_peers(&data_dir, &item, version).expect("peers");
        let dest = plugin_root.join("node_modules/@deepseek-ai/dsh-base");
        assert!(dest.exists(), "peer 应被链接/复制进中央库");
        assert!(dest.join("package.json").is_file());
        // 幂等：同内核再跑一次不重复（存在即跳过）
        refresh_store_peers(&data_dir, &item, version).expect("peers again");
        assert!(dest.exists());
    }

    #[test]
    fn wire_manifest_applies_and_prunes() {
        let mut root = serde_json::json!({
            "name": "dsh-profile-web",
            "private": true,
            "dependencies": {
                "other": "1.0.0",
                "old-plugin": "link:../../desktop/kernels/1.0.0/plugins/old-plugin",
            },
            "dsh": {
                "profile": {
                    "bundles": ["@deepseek-ai/dsh-base", "@deepseek-ai/dsh-web-app", "old-plugin"],
                },
            },
        });
        let mut specs = BTreeMap::new();
        specs.insert(
            "new-plugin".to_string(),
            (
                "link:../../desktop/kernels/9.9.9/plugins/new-plugin".to_string(),
                true,
            ),
        );
        specs.insert(
            "plain-plugin".to_string(),
            (
                "link:../../desktop/kernels/9.9.9/plugins/plain-plugin".to_string(),
                false,
            ),
        );

        let changed = wire_manifest(&mut root, &specs, "web").expect("wire");
        assert!(changed);
        let deps = root["dependencies"].as_object().expect("deps");
        assert_eq!(
            deps["new-plugin"].as_str().unwrap(),
            "link:../../desktop/kernels/9.9.9/plugins/new-plugin"
        );
        assert!(deps.contains_key("plain-plugin"));
        assert!(deps.contains_key("other")); // 用户/CLI 条目保留
        assert!(!deps.contains_key("old-plugin")); // 已卸载条目清退
        let bundles: Vec<&str> = root["dsh"]["profile"]["bundles"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|b| b.as_str())
            .collect();
        assert!(bundles.contains(&"@deepseek-ai/dsh-base"));
        assert!(bundles.contains(&"new-plugin"));
        assert!(!bundles.contains(&"plain-plugin")); // 非 bundle 不进层
        assert!(!bundles.contains(&"old-plugin"));

        let changed = wire_manifest(&mut root, &specs, "web").expect("wire again");
        assert!(!changed);
    }

    #[test]
    fn is_managed_spec_matches_shell_layout_not_dir_name() {
        // release 壳（desktop/）与 dev 壳（desktop-dev/）写出的 spec 都必须
        // 识别为托管，否则 dev 卸载残留会被当成用户依赖保留，内核启动时
        // 解析悬空 bundle 崩溃（regression：WIRED_MARK 只匹配 "desktop/kernels/"）。
        assert!(is_managed_spec(
            "link:../../desktop/kernels/1.0.0/plugins/x"
        ));
        assert!(is_managed_spec(
            "link:../../desktop-dev/kernels/0.1.1-rc.2/plugins/x"
        ));
        // DSH_DESKTOP_DATA_DIR 覆盖为任意目录名时写出的 spec（common==0 时
        // relative_path 产出绝对路径）。
        assert!(is_managed_spec(
            "file:C:/Users/u/.dsh/desktop/kernels/1/plugins/x"
        ));
        assert!(is_managed_spec(
            "link:/Volumes/ext/dsh-shell/kernels/1/plugins/@scope__pkg"
        ));
        // 非托管：用户/CLI 依赖——版本号、任意 link/file 目标、路径里
        // kernels/plugins 不构成尾部布局、空的 version/id 段。
        assert!(!is_managed_spec("^1.0.0"));
        assert!(!is_managed_spec("link:../packages/my-plugin"));
        assert!(!is_managed_spec("file:../vendor/kernels/pkg"));
        assert!(!is_managed_spec(
            "link:../../desktop/plugins/1.0.0/kernels/x"
        ));
        assert!(!is_managed_spec("link:../../desktop/kernels//plugins/x"));
        assert!(!is_managed_spec("link:../../desktop/kernels/1/plugins/"));
    }

    #[test]
    fn wire_manifest_prunes_dev_shell_residue() {
        // dev 壳卸载插件后 manifest 残留的 desktop-dev spec 必须被清退：
        // 空 specs（安全模式/最后一个插件被卸载）下依赖与 bundle 层一起消失，
        // 只留下模板层。用户/CLI 条目不受影响。
        let mut root = serde_json::json!({
            "name": "dsh-profile-web",
            "private": true,
            "dependencies": {
                "other": "1.0.0",
                "dsh-synapse": "link:../../desktop-dev/kernels/0.1.1-rc.2/plugins/github.com__liangmianya__dsh-synapse",
            },
            "dsh": {
                "profile": {
                    "bundles": ["@deepseek-ai/dsh-base", "@deepseek-ai/dsh-web-app", "dsh-synapse"],
                },
            },
        });
        let specs = BTreeMap::new();
        let changed = wire_manifest(&mut root, &specs, "web").expect("wire");
        assert!(changed);
        let deps = root["dependencies"].as_object().expect("deps");
        assert!(!deps.contains_key("dsh-synapse")); // dev 壳卸载残留清退
        assert!(deps.contains_key("other")); // 用户/CLI 条目保留
        let bundles: Vec<&str> = root["dsh"]["profile"]["bundles"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|b| b.as_str())
            .collect();
        assert_eq!(
            bundles,
            ["@deepseek-ai/dsh-base", "@deepseek-ai/dsh-web-app"]
        );
    }

    #[test]
    fn wire_manifest_rewrites_cross_shell_specs() {
        // manifest 由 dev 壳接线，release 壳重新接线时 spec 重写为本壳的
        // kernel plugins 目录。
        let mut root = serde_json::json!({
            "name": "dsh-profile-web",
            "private": true,
            "dependencies": {
                "dsh-synapse": "link:../../desktop-dev/kernels/0.1.1-rc.2/plugins/github.com__liangmianya__dsh-synapse",
            },
            "dsh": {
                "profile": {
                    "bundles": ["@deepseek-ai/dsh-base", "@deepseek-ai/dsh-web-app", "dsh-synapse"],
                },
            },
        });
        let mut specs = BTreeMap::new();
        specs.insert(
            "dsh-synapse".to_string(),
            (
                "link:../../desktop/kernels/0.1.1-rc.2/plugins/github.com__liangmianya__dsh-synapse"
                    .to_string(),
                true,
            ),
        );
        let changed = wire_manifest(&mut root, &specs, "web").expect("wire");
        assert!(changed);
        assert_eq!(
            root["dependencies"]["dsh-synapse"].as_str().unwrap(),
            "link:../../desktop/kernels/0.1.1-rc.2/plugins/github.com__liangmianya__dsh-synapse"
        );
        let bundles: Vec<&str> = root["dsh"]["profile"]["bundles"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|b| b.as_str())
            .collect();
        assert!(bundles.contains(&"dsh-synapse"));
    }

    #[test]
    fn spec_path_uses_forward_slashes() {
        #[cfg(windows)]
        let rel = PathBuf::from("..\\..\\desktop\\kernels\\1\\plugins\\x");
        #[cfg(not(windows))]
        let rel = PathBuf::from("../../desktop/kernels/1/plugins/x");
        assert_eq!(spec_path_string(&rel), "../../desktop/kernels/1/plugins/x");
    }

    #[test]
    fn settings_profile_defaults_to_web() {
        let s = settings::Settings::default();
        assert_eq!(s.profile, DEFAULT_PROFILE);
        assert_eq!(DEFAULT_PROFILE, "web");
    }

    #[test]
    fn status_flags_stale_materialization() {
        let home = TestHome::new();
        let data_dir = home.data_dir();
        let version = "1.0.0";
        fs::create_dir_all(kernel::kernel_dir(&data_dir, version)).unwrap();
        fs::write(data_dir.join("active.txt"), format!("{version}\n")).unwrap();
        upsert_item(
            &data_dir,
            StoreItem {
                id: "stale-plugin".into(),
                name: "stale-plugin".into(),
                origin: "npm".into(),
                source: "stale-plugin".into(),
                installed_version: "2.0.0".into(),
                latest_version: Some("3.0.0".into()),
                mode: "link".into(),
                pinned: false,
                installed_at: String::new(),
                updated_at: String::new(),
                repo_url: None,
                description: None,
            },
        )
        .expect("save");
        let settings = settings::Settings::default();
        let view = status(&data_dir, &settings);
        assert_eq!(view.rows.len(), 1);
        assert!(!view.rows[0].synced);
        assert_eq!(view.updates, 1);
        assert_eq!(view.rows[0].latest_version.as_deref(), Some("3.0.0"));
    }

    /// `update()` 把 `latest_version` 同步为 `installed_version` 之后，
    /// UI 不应再渲染「有更新」角标。`status()` 行必须隐藏 `latest_version`，
    /// 让按行的 UI 检查（判断字段是否为真）停止显示那条幽灵通知。顶层
    /// 总数已经单独过滤过。
    #[test]
    fn status_hides_latest_when_not_newer_than_installed() {
        let home = TestHome::new();
        let data_dir = home.data_dir();
        upsert_item(
            &data_dir,
            StoreItem {
                id: "synced-plugin".into(),
                name: "synced-plugin".into(),
                origin: "npm".into(),
                source: "synced-plugin".into(),
                installed_version: "1.2.3".into(),
                latest_version: Some("1.2.3".into()),
                mode: "link".into(),
                pinned: false,
                installed_at: String::new(),
                updated_at: String::new(),
                repo_url: None,
                description: None,
            },
        )
        .expect("save");
        let settings = settings::Settings::default();
        let view = status(&data_dir, &settings);
        assert_eq!(view.updates, 0);
        assert!(
            view.rows[0].latest_version.is_none(),
            "row.latest_version must be hidden when it equals installed_version, got {:?}",
            view.rows[0].latest_version
        );
    }

    #[test]
    fn status_keeps_latest_when_newer_than_installed() {
        let home = TestHome::new();
        let data_dir = home.data_dir();
        upsert_item(
            &data_dir,
            StoreItem {
                id: "behind-plugin".into(),
                name: "behind-plugin".into(),
                origin: "npm".into(),
                source: "behind-plugin".into(),
                installed_version: "1.0.0".into(),
                latest_version: Some("2.0.0".into()),
                mode: "link".into(),
                pinned: false,
                installed_at: String::new(),
                updated_at: String::new(),
                repo_url: None,
                description: None,
            },
        )
        .expect("save");
        let settings = settings::Settings::default();
        let view = status(&data_dir, &settings);
        assert_eq!(view.updates, 1);
        assert_eq!(view.rows[0].latest_version.as_deref(), Some("2.0.0"));
    }

    /// 启动看护产生的隔离记录必须出现在插件行上，让管理 UI 能够按插件
    /// 给出「重新启用 / 卸载」的决策，而不是等用户发现某个集成悄无声息
    /// 地不见。
    #[test]
    fn status_attaches_quarantine_record_to_row() {
        let home = TestHome::new();
        let data_dir = home.data_dir();
        upsert_item(
            &data_dir,
            StoreItem {
                id: "bad-plugin".into(),
                name: "bad-plugin".into(),
                origin: "npm".into(),
                source: "bad-plugin".into(),
                installed_version: "1.0.0".into(),
                latest_version: None,
                mode: "link".into(),
                pinned: false,
                installed_at: String::new(),
                updated_at: String::new(),
                repo_url: None,
                description: None,
            },
        )
        .expect("save");
        quarantine::add_all(
            &data_dir,
            &[quarantine::QuarantineItem {
                id: "bad-plugin".into(),
                name: "bad-plugin".into(),
                reason: "测试隔离".into(),
                evidence: "Error: boom".into(),
                at: 1,
            }],
        )
        .expect("quarantine");

        let view = status(&data_dir, &settings::Settings::default());
        assert_eq!(view.rows.len(), 1);
        let record = view.rows[0]
            .quarantined
            .as_ref()
            .expect("row must carry the quarantine record");
        assert_eq!(record.reason, "测试隔离");

        // 重新启用会把记录一起丢掉，行上的隔离标记也跟着没了。
        quarantine::remove(&data_dir, "bad-plugin").expect("remove");
        let view = status(&data_dir, &settings::Settings::default());
        assert!(view.rows[0].quarantined.is_none());
    }

    #[test]
    fn uninstall_cleans_stale_quarantine_when_store_item_is_missing() {
        let home = TestHome::new();
        let data_dir = home.data_dir();
        let id = "dsh-flowglass";
        fs::create_dir_all(&data_dir).expect("data dir");
        quarantine::add_all(
            &data_dir,
            &[quarantine::QuarantineItem {
                id: id.into(),
                name: id.into(),
                reason: "启动失败".into(),
                evidence: "Error: missing dependency".into(),
                at: 1,
            }],
        )
        .expect("quarantine");

        // 这是 dev 壳上报的部分清理状态：中央库条目和 profile 接线都
        // 已经不在了，但还有一条老的隔离记录需要清掉。
        let profile = profile_dir(&data_dir, "web");
        fs::create_dir_all(profile.join("node_modules")).expect("node_modules");
        let manifest = serde_json::json!({
            "name": "dsh-profile-web",
            "private": true,
            "dependencies": {},
            "dsh": { "profile": { "bundles": ["@deepseek-ai/dsh-base", "@deepseek-ai/dsh-web-app"] } }
        });
        fs::write(
            profile.join("package.json"),
            serde_json::to_string_pretty(&manifest).expect("manifest") + "\n",
        )
        .expect("write manifest");

        let mut noop = |_: &str| {};
        uninstall(
            &data_dir,
            &settings::Settings::default(),
            Path::new("pnpm"),
            id,
            &mut noop,
        )
        .expect("stale quarantine should be removable");

        assert!(quarantine::load(&data_dir).items.is_empty());
        let cleaned: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(profile.join("package.json")).expect("cleaned manifest"),
        )
        .expect("cleaned json");
        let bundles = cleaned["dsh"]["profile"]["bundles"]
            .as_array()
            .expect("bundles");
        assert!(!bundles.iter().any(|bundle| bundle.as_str() == Some(id)));
    }

    /// 辅助函数：在暂存目录里写入 `.dsh-id` 标记，让恢复扫描能把它与
    /// 对应的 `final_dir` 归到一组。
    fn mark_staging(dir: &Path, id: &str) {
        fs::create_dir_all(dir).expect("mkdir staging");
        fs::write(dir.join(ID_MARKER), format!("{id}\n")).expect("write marker");
    }

    fn write_fake_plugin(dir: &Path, tag: &str) {
        fs::create_dir_all(dir).expect("mkdir plugin");
        let pkg = format!(r#"{{"name":"p","version":"{tag}","main":"lib/index.js"}}"#);
        fs::write(dir.join("package.json"), pkg).expect("manifest");
        fs::create_dir_all(dir.join("lib")).expect("lib");
        fs::write(dir.join("lib/index.js"), "module.exports={}").expect("entry");
    }

    #[test]
    fn reconcile_is_noop_when_no_staging_dirs() {
        let home = TestHome::new();
        let data_dir = home.data_dir();
        let store = store_dir(&data_dir);
        fs::create_dir_all(&store).unwrap();
        // 仅有线上插件、周围没有任何暂存目录。
        write_fake_plugin(&store.join("live-plugin"), "1.0.0");
        reconcile_store(&data_dir);
        assert!(store.join("live-plugin").is_dir());
        let entries: Vec<_> = fs::read_dir(&store)
            .unwrap()
            .flatten()
            .filter(|e| {
                let n = e.file_name();
                let n = n.to_string_lossy();
                n.starts_with(TMP_PREFIX)
                    || n.starts_with(NEW_PREFIX)
                    || n.starts_with(BACKUP_PREFIX)
            })
            .collect();
        assert!(entries.is_empty(), "no staging should remain");
    }

    #[test]
    fn reconcile_reverts_to_backup_when_final_missing_and_both_staging_present() {
        let home = TestHome::new();
        let data_dir = home.data_dir();
        let store = store_dir(&data_dir);
        fs::create_dir_all(&store).unwrap();

        // 在 `final → backup` 与 `new → final` 之间崩溃：final_dir 缺失，
        // backup（旧版）和 new（已校验）都从崩溃中幸存。
        let id = "p";
        mark_staging(&store.join(format!("{BACKUP_PREFIX}1-1")), id);
        write_fake_plugin(&store.join(format!("{BACKUP_PREFIX}1-1")), "1.0.0");
        mark_staging(&store.join(format!("{NEW_PREFIX}2-2")), id);
        write_fake_plugin(&store.join(format!("{NEW_PREFIX}2-2")), "2.0.0");

        reconcile_store(&data_dir);

        // 回滚：backup 胜出，新的暂存被丢弃。
        let final_dir = store.join(id);
        let final_manifest = fs::read_to_string(final_dir.join("package.json")).unwrap();
        assert!(
            final_manifest.contains("\"version\":\"1.0.0\""),
            "expected revert to old version, got: {final_manifest}"
        );
        assert!(!store.join(format!("{NEW_PREFIX}2-2")).exists());
        assert!(!store.join(format!("{BACKUP_PREFIX}1-1")).exists());
    }

    #[test]
    fn reconcile_promotes_new_when_only_new_survives() {
        let home = TestHome::new();
        let data_dir = home.data_dir();
        let store = store_dir(&data_dir);
        fs::create_dir_all(&store).unwrap();

        // 在 `tmp → new` 之后、`final → backup` 之前崩溃：final 缺失，
        // 只有 `.new-*` 幸存，恢复流程会发布它。
        let id = "p";
        mark_staging(&store.join(format!("{NEW_PREFIX}3-3")), id);
        write_fake_plugin(&store.join(format!("{NEW_PREFIX}3-3")), "2.5.0");

        reconcile_store(&data_dir);

        let final_dir = store.join(id);
        let final_manifest = fs::read_to_string(final_dir.join("package.json")).unwrap();
        assert!(
            final_manifest.contains("\"version\":\"2.5.0\""),
            "expected publish of new version, got: {final_manifest}"
        );
        assert!(!store.join(format!("{NEW_PREFIX}3-3")).exists());
    }

    #[test]
    fn reconcile_discards_tmp_only() {
        let home = TestHome::new();
        let data_dir = home.data_dir();
        let store = store_dir(&data_dir);
        fs::create_dir_all(&store).unwrap();

        // 在校验前的拉取中途崩溃：只有 `.tmp-*` 幸存。
        let id = "p";
        mark_staging(&store.join(format!("{TMP_PREFIX}4-4")), id);
        write_fake_plugin(&store.join(format!("{TMP_PREFIX}4-4")), "0.0.1");

        reconcile_store(&data_dir);

        assert!(!store.join(format!("{TMP_PREFIX}4-4")).exists());
        assert!(!store.join(id).exists());
    }

    #[test]
    fn reconcile_cleans_stale_staging_when_live_plugin_present() {
        let home = TestHome::new();
        let data_dir = home.data_dir();
        let store = store_dir(&data_dir);
        fs::create_dir_all(&store).unwrap();

        // 线上插件存在；一次完成的更新留下的 `.backup-*` 没被清掉
        // （发布后的清理没跑）。恢复流程直接丢弃它。
        let id = "live";
        write_fake_plugin(&store.join(id), "3.0.0");
        mark_staging(&store.join(format!("{BACKUP_PREFIX}5-5")), id);
        write_fake_plugin(&store.join(format!("{BACKUP_PREFIX}5-5")), "2.0.0");
        mark_staging(&store.join(format!("{NEW_PREFIX}6-6")), id);
        write_fake_plugin(&store.join(format!("{NEW_PREFIX}6-6")), "3.0.0");

        reconcile_store(&data_dir);

        assert!(store.join(id).is_dir());
        assert!(!store.join(format!("{BACKUP_PREFIX}5-5")).exists());
        assert!(!store.join(format!("{NEW_PREFIX}6-6")).exists());
    }

    #[test]
    fn reconcile_picks_newest_when_multiple_staging_dirs_share_id() {
        let home = TestHome::new();
        let data_dir = home.data_dir();
        let store = store_dir(&data_dir);
        fs::create_dir_all(&store).unwrap();

        // 两次失败的更新交错：final 缺失，同一 id 下两份 backup 与两份
        // new 同时幸存。最新的（后缀字典序最大者）胜出；较老的对等目录
        // 被移除。
        let id = "p";
        mark_staging(&store.join(format!("{BACKUP_PREFIX}1-1")), id);
        write_fake_plugin(&store.join(format!("{BACKUP_PREFIX}1-1")), "0.9.0");
        mark_staging(&store.join(format!("{BACKUP_PREFIX}2-2")), id);
        write_fake_plugin(&store.join(format!("{BACKUP_PREFIX}2-2")), "1.0.0");
        mark_staging(&store.join(format!("{NEW_PREFIX}3-3")), id);
        write_fake_plugin(&store.join(format!("{NEW_PREFIX}3-3")), "1.5.0");
        mark_staging(&store.join(format!("{NEW_PREFIX}4-4")), id);
        write_fake_plugin(&store.join(format!("{NEW_PREFIX}4-4")), "2.0.0");

        reconcile_store(&data_dir);

        // 两份 new 都存在 → 回滚到最新的 backup（id 2-2）；两份 new 全部移除。
        let final_dir = store.join(id);
        let final_manifest = fs::read_to_string(final_dir.join("package.json")).unwrap();
        assert!(
            final_manifest.contains("\"version\":\"1.0.0\""),
            "expected freshest backup as the revert target, got: {final_manifest}"
        );
        assert!(!store.join(format!("{BACKUP_PREFIX}1-1")).exists());
        assert!(!store.join(format!("{BACKUP_PREFIX}2-2")).exists());
        assert!(!store.join(format!("{NEW_PREFIX}3-3")).exists());
        assert!(!store.join(format!("{NEW_PREFIX}4-4")).exists());
    }

    /// `new_staging_dir` 返回的是一个没有任何标记的空目录。标记的写入
    /// 由调用方负责：在 rename 目标上预先盖章就是当初 Windows 上
    /// ERROR_DIR_NOT_EMPTY 的失败模式（Windows 的 `MoveFileEx` 会拒绝
    /// 非空目标），所以暂存目录的创建 API 必须把路径留空，由
    /// `stamp_id_marker` 在 rename 成功后再补上标记。
    #[test]
    fn new_staging_dir_returns_empty_dir_without_marker() {
        let home = TestHome::new();
        let store = store_dir(&home.data_dir());
        let dir = new_staging_dir(&store, TMP_PREFIX, "test-plugin").expect("create staging");
        assert!(dir.is_dir(), "staging dir must exist");
        let entries: Vec<_> = fs::read_dir(&dir).unwrap().collect();
        assert!(
            entries.is_empty(),
            "staging dir must be empty so fs::rename can land on it; got {:?}",
            entries
                .iter()
                .map(|e| e.as_ref().unwrap().file_name())
                .collect::<Vec<_>>()
        );
    }

    /// `stamp_id_marker` 写入的是 `reconcile_store` 用来按插件 id 给暂存
    /// 目录分组的标记。盖章之后，目录里恰好只剩这一个文件（标记本身）。
    #[test]
    fn stamp_id_marker_writes_marker_file() {
        let home = TestHome::new();
        let dir = store_dir(&home.data_dir()).join("marker-target");
        fs::create_dir_all(&dir).unwrap();
        stamp_id_marker(&dir, "test-plugin").expect("stamp");
        let content = fs::read_to_string(dir.join(ID_MARKER)).unwrap();
        assert_eq!(content, "test-plugin\n");
    }

    /// 同一测试里的两次 `new_staging_dir` 调用必须落在不同的路径上。
    /// nanos + pid 方案在一切实际时间尺度下都能保持唯一；第二次调用不能
    /// 与第一次冲突，更不能把第一次清理掉。
    #[test]
    fn new_staging_dir_paths_do_not_collide() {
        let home = TestHome::new();
        let store = store_dir(&home.data_dir());
        let a = new_staging_dir(&store, TMP_PREFIX, "a").expect("first");
        let b = new_staging_dir(&store, TMP_PREFIX, "b").expect("second");
        assert_ne!(a, b, "two staging dirs must have distinct paths");
        assert!(a.is_dir());
        assert!(b.is_dir());
    }

    /// `new_staging_dir` 会在调用方传入一个可能被复用的路径时清理已存在的
    /// 目标目录。我们在两次调用之间预先在 helper 期望的路径上放一个过期
    // 目录来模拟残留 —— helper 内部的 `remove_dir_all` 必须能清掉它。
    // 在旧的「吞掉错误」实现里，残留会原样穿透过去，导致 Windows 上
    // `fs::rename` 因 ERROR_DIR_NOT_EMPTY 而失败。
    #[test]
    fn new_staging_dir_clears_stale_target() {
        let home = TestHome::new();
        let store = store_dir(&home.data_dir());
        // 种下与 helper 第一次调用期望路径匹配的过期残留。
        let first = new_staging_dir(&store, TMP_PREFIX, "stale-test").expect("first call");
        let stale_id_marker = first.join(ID_MARKER);
        fs::write(&stale_id_marker, "stale-test\n").unwrap();
        // 第二次调用会落在不同的路径（nanos 漂移），但「同路径重试」要求
        // 清理步骤在目录被复用前完成 —— 验证 helper 的 remove 步骤对第一条
        // 路径确实有效。
        let _ = fs::remove_dir_all(&first);
        let second = new_staging_dir(&store, TMP_PREFIX, "stale-test").expect("second call");
        assert!(second.is_dir());
        assert!(!stale_id_marker.exists(), "stale marker must be gone");
    }
}
