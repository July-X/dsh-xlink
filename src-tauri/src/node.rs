//! 定位并校验用于运行 kernel 的 Node.js 运行时。

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

use crate::process::{run_command_capture, run_with_progress, LogSpec};
use crate::settings::Settings;

/// dsh 声明的 engines 范围（`^22.19.0 || >=24.0.0`）。
const MIN_COMPATIBLE: (u32, u32, u32) = (22, 19, 0);
const MAJOR_ALT_FLOOR: u32 = 24;

/// 外壳对某个 Node 候选探测到的信息。
#[derive(Debug, Clone, Serialize)]
pub struct NodeInfo {
    pub path: String,
    pub version: Option<String>,
    pub ok: bool,
    pub reason: String,
}

/// 将 `v22.19.0` 形式的输出解析为 (major, minor, patch)。
fn parse_version(output: &str) -> Option<(u32, u32, u32)> {
    let text = output.trim().strip_prefix('v')?;
    let mut parts = text.split('.');
    // 若缺少显式的 `parse::<u32>()` 类型标注，编译器会报 E0282
    // （type annotations needed），因为每个 `parse()` 都对目标整数类型
    // 泛型，本身没有对类型的约束。
    let major = parts.next()?.parse::<u32>().ok()?;
    let minor = parts.next()?.parse::<u32>().ok()?;
    let patch = parts
        .next()?
        .split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse::<u32>()
        .ok()?;
    Some((major, minor, patch))
}

/// 判断已解析的版本是否满足 dsh 的 engines 要求。
fn compatible((major, minor, _patch): (u32, u32, u32)) -> bool {
    (major == MIN_COMPATIBLE.0 && minor >= MIN_COMPATIBLE.1) || major >= MAJOR_ALT_FLOOR
}

/// 向某个 node 可执行文件询问其版本。
pub fn version_of(path: &Path) -> Option<String> {
    let mut cmd = Command::new(path);
    cmd.arg("--version");
    let (success, stdout, _) = run_command_capture(cmd, "node --version").ok()?;
    if !success {
        return None;
    }
    let version = stdout.trim().to_string();
    (parse_version(&stdout).is_some()).then_some(version)
}

/// 探测某个 node 候选可执行文件，并报告其可用性。
pub fn probe(path: &Path) -> NodeInfo {
    let path = path.to_string_lossy().into_owned();
    if !fs::metadata(&path).map(|m| m.is_file()).unwrap_or(false) {
        return NodeInfo {
            path,
            version: None,
            ok: false,
            reason: "路径不存在或不可读".into(),
        };
    }
    match version_of(Path::new(&path)) {
        None => NodeInfo {
            path,
            version: None,
            ok: false,
            reason: "无法读取版本输出（可能不是有效的 node 可执行文件）".into(),
        },
        Some(version) => {
            let parsed = parse_version(&version).unwrap_or_default();
            if compatible(parsed) {
                NodeInfo {
                    path,
                    version: Some(version),
                    ok: true,
                    reason: "可用".into(),
                }
            } else {
                let (maj, min, pat) = parsed;
                NodeInfo {
                    path,
                    version: Some(version),
                    ok: false,
                    reason: format!("版本 {maj}.{min}.{pat} 不满足 dsh 要求（^22.19 || >=24）"),
                }
            }
        }
    }
}

/// 为 Windows 的 PATH 查找补上结尾的 `.exe`。
fn exe_name(name: &str) -> String {
    if cfg!(windows) && !name.to_ascii_lowercase().ends_with(".exe") {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

/// 在 Windows 上查找工具时按顺序探测的可执行文件名候选。
/// Windows 的 PATH 查找遵循 `PATHEXT`（默认为 `.COM;.EXE;.BAT;.CMD;…`），
/// 并且 Node 周边工具绝大多数都以 `.cmd` shim 的形式安装到用户级 npm 前缀
/// （`%AppData%\npm\pnpm.cmd`）而非 `.exe`。先探测 `.cmd` 正好契合每次
/// `npm install -g` 产生的布局，再回退到 `.exe`（系统级安装和独立的
/// `pnpm` 安装），最后是裸名（PATH 项中已经包含扩展名）。在非
/// Windows 平台上只有裸名是合法的。
#[cfg(windows)]
const WINDOWS_EXE_CANDIDATES: &[&str] = &[".cmd", ".exe", ""];

#[cfg(windows)]
fn which_in_dir(name: &str, dir: &Path) -> Option<PathBuf> {
    for ext in WINDOWS_EXE_CANDIDATES {
        let candidate = dir.join(format!("{name}{ext}"));
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

#[cfg(not(windows))]
fn which_in_dir(name: &str, dir: &Path) -> Option<PathBuf> {
    let candidate = dir.join(name);
    candidate.is_file().then_some(candidate)
}

/// 在 PATH 上查找工具时需要扫描的目录。检测必须看到外壳注入到被启动
/// 子进程上的同一份 PATH（`crate::env::merged_path`）：GUI 启动的
/// Windows shell 只继承系统 PATH，因此即便用户在终端能跑出工具，装在
/// 用户级 npm 前缀（`%AppData%\npm`）里的工具对直接调用
/// `std::env::var_os("PATH")` 的扫描依然不可见。在其他平台上，
/// 合并后的 PATH 与进程 PATH 一致。
fn path_dirs() -> impl Iterator<Item = PathBuf> {
    std::env::split_paths(crate::env::merged_path())
}

/// 在 PATH 中查找 `node`。
fn from_path() -> Option<PathBuf> {
    for dir in path_dirs() {
        let candidate = dir.join(exe_name("node"));
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// 当 PATH 中找不到 `node` 时探测的常见安装位置。
fn common_locations() -> Vec<PathBuf> {
    let mut out = vec![
        PathBuf::from("/usr/local/bin/node"),
        PathBuf::from("/opt/homebrew/bin/node"),
        PathBuf::from("/usr/bin/node"),
    ];
    if cfg!(windows) {
        out.push(PathBuf::from(r"C:\Program Files\nodejs\node.exe"));
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            out.push(Path::new(&local).join(r"Programs\nodejs\node.exe"));
        }
    }
    out
}

// --- nvm 管理的安装 -------------------------------------------------------
//
// GUI shell 启动时拿到的 PATH 很有限（macOS 上是 launchd，Windows 上是
// 从 `HKCU\Environment` 合并的 Window Station 系统 PATH，详见
// `crate::env`），因此即便 nvm 管理的 Node 正是用户想要使用的运行时，
// 它对 PATH 扫描依然不可见。nvm 在磁盘上以可预测的布局保存每个已安装
// 的版本，外壳因此可以直接发现它们，而不依赖继承下来的环境：
//
// - nvm-sh（macOS/Linux）：根目录 `$NVM_DIR`（默认 `~/.nvm`），
//   版本位于 `versions/node/<vX.Y.Z>/bin/node`；首选版本记录在
//   `alias/default` 中，其内容本身可能再指向另一个 alias
//   （如 `lts/hydrogen`）或部分版本号（如 `22`）。
// - nvm-windows：根目录 `%NVM_HOME%`（默认 `%APPDATA%\nvm`），版本
//   位于 `vX.Y.Z\node.exe`；`nvm use` 所选的版本以
//   `%NVM_SYMLINK%` 命名的目录 junction 暴露出来。

/// [`resolve_alias`] 在放弃前所跟随的 alias 文件间接跳转次数。
/// `default` → `lts/hydrogen` → `20.9.0` 是常见深度；对重复的 alias
/// 解析会立即停止，上限则防御异常长的无环链，并最终回退到按版本倒序
/// 的扫描。
#[cfg(not(windows))]
const ALIAS_MAX_HOPS: usize = 5;

/// nvm-sh 的根目录（`$NVM_DIR`，默认为 `~/.nvm`），仅在其真实存在于磁盘上时返回。
#[cfg(not(windows))]
fn nvm_root() -> Option<PathBuf> {
    let candidate = std::env::var_os("NVM_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::kernel::dirs_home().join(".nvm"));
    candidate.is_dir().then_some(candidate)
}

/// nvm-windows 可能存放版本的目录：携带 `$NVM_HOME` 时使用它，
/// 否则回退到默认的 `%APPDATA%\nvm`。GUI 进程常常两个变量都拿不到
/// （参见 `crate::env`），默认路径确保在这种情况下检测仍能工作。
#[cfg(windows)]
fn nvm_roots() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    if let Some(dir) = std::env::var_os("NVM_HOME") {
        let p = PathBuf::from(dir);
        if p.is_dir() {
            out.push(p);
        }
    }
    if let Some(appdata) = std::env::var_os("APPDATA") {
        let p = PathBuf::from(appdata).join("nvm");
        if p.is_dir() && !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

/// 沿着 alias 文件链把一个 nvm-sh alias 解析为具体的版本说明：
/// `alias/<name>` 可能再指向另一个 alias，最终落到版本字符串
/// （可能只是部分版本，如 `22`）。返回最终的版本说明，但不检查是否有
/// 已安装版本与之匹配。
#[cfg(not(windows))]
fn resolve_alias(root: &Path, start: &str) -> Option<String> {
    let mut spec = start.to_string();
    let mut seen = HashSet::new();
    for _ in 0..ALIAS_MAX_HOPS {
        if !seen.insert(spec.clone()) {
            break;
        }
        match fs::read_to_string(root.join("alias").join(&spec)) {
            Ok(text) => {
                let next = text.trim();
                if next.is_empty() {
                    return None;
                }
                spec = next.to_string();
            }
            // `spec` 没有再指向其他 alias 文件，因此它就是最终的
            // （可能部分）版本说明。
            Err(_) => break,
        }
    }
    let trimmed = spec.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// 将 alias 解析出的版本说明去掉前后空白和开头的 `v`，
/// 以便与已安装目录名比较时使用统一格式。
fn normalize_spec(spec: Option<&str>) -> Option<String> {
    let s = spec?.trim();
    let s = s.strip_prefix('v').unwrap_or(s);
    (!s.is_empty()).then(|| s.to_string())
}

/// 把解析出的元组渲染回用于 alias 说明比较的点号形式
/// （`(22, 19, 0)` → `"22.19.0"`）。
fn format_version((major, minor, patch): (u32, u32, u32)) -> String {
    format!("{major}.{minor}.{patch}")
}

/// 对版本管理器发现到的安装进行排序，以供探测。
///
/// 与 `default_spec` 匹配的安装排在最前——这是用户通过
/// `nvm alias default` 锁定的版本。精确匹配优先，再按 spec 前缀
/// 匹配已安装的最高版本（`22` 解析为最新的 v22.x.y），镜像 nvm 自身
/// 对部分说明的解析方式。其余满足 engines 范围的安装按版本倒序排列，
/// 这样即便用户没有 pin，也能拿到可用的 Node。声明版本无法满足
/// engines 范围的安装会被直接丢弃：探测它们只会得到一个被调用方更好
/// 渲染的拒绝结果，与白白多起一个子进程相比并不划算。
fn order_nvm_versions(
    mut installed: Vec<((u32, u32, u32), PathBuf)>,
    default_spec: Option<&str>,
) -> Vec<PathBuf> {
    // 按版本从新到旧排序；相同的元组意味着相同的目录，
    // 不需要进一步区分。
    installed.sort_by_key(|a| std::cmp::Reverse(a.0));
    let mut head: Option<usize> = None;
    if let Some(spec) = normalize_spec(default_spec) {
        head = installed
            .iter()
            .position(|(v, _)| format_version(*v) == spec)
            .or_else(|| {
                installed
                    .iter()
                    .position(|(v, _)| format_version(*v).starts_with(&format!("{spec}.")))
            });
    }
    let mut out = Vec::with_capacity(installed.len());
    if let Some(i) = head {
        out.push(installed.remove(i).1);
    }
    out.extend(
        installed
            .into_iter()
            .filter(|(v, _)| compatible(*v))
            .map(|(_, path)| path),
    );
    out
}

/// nvm-sh（macOS/Linux）管理的 node 可执行文件，按探测顺序：
/// `default` alias 对应的安装排在最前，其余满足 engines 范围的安装
/// 按版本倒序排列。目录名无法解析的版本（例如手工放入的自定义构建）
/// 追加在末尾，让探测而非文件名启发来决定是否使用。
#[cfg(not(windows))]
pub(crate) fn nvm_candidates() -> Vec<PathBuf> {
    let Some(root) = nvm_root() else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(root.join("versions").join("node")) else {
        return Vec::new();
    };
    let mut installed = Vec::new();
    let mut unparsed = Vec::new();
    for entry in entries.flatten() {
        let bin_node = entry.path().join("bin").join("node");
        if !bin_node.is_file() {
            continue;
        }
        match parse_version(&entry.file_name().to_string_lossy()) {
            Some(v) => installed.push((v, bin_node)),
            None => unparsed.push(bin_node),
        }
    }
    let default_spec = resolve_alias(&root, "default");
    let mut out = order_nvm_versions(installed, default_spec.as_deref());
    out.extend(unparsed);
    out
}

/// nvm-windows 管理的 node 可执行文件，按探测顺序：活跃 junction
/// （`%NVM_SYMLINK%`，即 `nvm use` 选中的版本）排在最前，
/// 然后是 `%NVM_HOME%` / `%APPDATA%\nvm` 下每个满足 engines 范围的
/// 安装，按版本倒序排列。无法解析的目录名追加在末尾，留给探测。
#[cfg(windows)]
pub(crate) fn nvm_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(link) = std::env::var_os("NVM_SYMLINK") {
        let candidate = PathBuf::from(link).join(exe_name("node"));
        if candidate.is_file() {
            out.push(candidate);
        }
    }
    let mut installed = Vec::new();
    let mut unparsed = Vec::new();
    for root in nvm_roots() {
        let Ok(entries) = fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            let node_exe = entry.path().join(exe_name("node"));
            if !node_exe.is_file() {
                continue;
            }
            match parse_version(&entry.file_name().to_string_lossy()) {
                Some(v) => installed.push((v, node_exe)),
                None => unparsed.push(node_exe),
            }
        }
    }
    // nvm-windows 把所选版本记录在 junction 中而非 alias 文件，
    // 因此这里没有需要解析的默认 spec。
    out.extend(order_nvm_versions(installed, None));
    out.extend(unparsed);
    out
}

/// 自动检测顺序下的 node 可执行候选：PATH 命中（启动 shell 解析到的——
/// 终端里 `nvm use` 之后的 dev shell 会落在这里）优先，
/// 其次是 nvm 管理的安装，最后是常见的系统位置。
fn environment_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(found) = from_path() {
        out.push(found);
    }
    out.extend(nvm_candidates());
    out.extend(common_locations().into_iter().filter(|p| p.is_file()));
    out
}

/// 对有序的环境候选进行探测的结果。
struct EnvironmentScan {
    /// 第一个满足 engines 范围的候选（如果有）。
    usable: Option<NodeInfo>,
    /// 第一个能跑起来但不满足范围要求的候选——在没有合格候选时
    /// 暴露出来，以便消息能够明确指出「Node 太老」而不是装作没有 Node。
    near_miss: Option<NodeInfo>,
}

/// 按顺序探测候选，直到某个满足 engines 范围为止，并沿途记住最接近
/// 的失败结果。
fn probe_environment(candidates: &[PathBuf]) -> EnvironmentScan {
    let mut near_miss: Option<NodeInfo> = None;
    for candidate in candidates {
        let info = probe(candidate);
        if info.ok {
            return EnvironmentScan {
                usable: Some(info),
                near_miss,
            };
        }
        if info.version.is_some() && near_miss.is_none() {
            near_miss = Some(info);
        }
    }
    EnvironmentScan {
        usable: None,
        near_miss,
    }
}

/// 当自动检测在磁盘上找不到任何 Node.js 安装时给出的指引——常见于全新
/// 机器尚未完成任何初始化，或用户刚刚卸载了最后一个运行时。首选外壳
/// 自带的一键自动安装（下载官方二进制到数据目录，见 node_install.rs）；
/// 其余三种独立安装方式保留，是因为不同用户的偏好不同；他们选择自己
/// 本来就会用的方式，下一次状态刷新（或「检测 Node」按钮）会重新运行
/// 检测。
const NO_NODE_FOUND_GUIDANCE: &str =
    "请通过下列任一方式安装 Node.js 22.19+（或 >=24）：\n\
     • 自动安装（推荐）：在主面板点击「帮我安装」，外壳将自动下载官方 Node.js 到数据目录（需联网）。\n\
     • 版本管理器（推荐）：nvm 用户执行 `nvm install 24 && nvm alias default 24`；fnm 用户执行 `fnm install 24 && fnm default 24`；volta 用户执行 `volta install node@24`。\n\
     • 系统包管理器：macOS `brew install node@24`；Ubuntu/Debian 装 NodeSource 后 `apt install nodejs`；Windows `winget install OpenJS.NodeJS.LTS`。\n\
     • 官方安装包：从 https://nodejs.org/ 下载安装包，安装后重启本应用，或在「设置」中手动指定 node 可执行文件路径。";

/// 当自动检测找到了 Node.js 安装，但其声明的版本低于 engines 范围
/// （`^22.19 || >=24`）时的指引。区分这一点与「完全没有 Node」很关键，
/// 因为对应的操作是就地升级，而不是全新安装。
const NODE_TOO_OLD_GUIDANCE: &str =
    "请升级 Node 到 22.19+（或 >=24）：使用 nvm 的用户执行 `nvm install 24 && nvm alias default 24`；使用 fnm 的用户执行 `fnm install 24 && fnm default 24`；也可从 https://nodejs.org/ 下载新版安装包覆盖安装，或在「设置」中手动指定新版 node 可执行文件路径。";

/// 托管运行时（`data_dir/tools/node/`，由「帮我安装」自动下载）的探测结果：
/// 已安装且满足 engines 时优先于环境检测，保证自动安装之后立刻可用。
fn probe_managed(data_dir: &Path) -> Option<NodeInfo> {
    let exe = crate::node_install::managed_node_exe(data_dir)?;
    let info = probe(&exe);
    if info.ok {
        return Some(NodeInfo {
            path: info.path,
            version: info.version,
            ok: true,
            reason: "托管运行时（自动安装到数据目录）".into(),
        });
    }
    None
}

/// 解析 node 可执行文件，依次看：显式配置 → 托管运行时 → 环境。
/// 托管运行时是用户确认后自动安装到数据目录的官方二进制（node_install.rs），
/// 它排在环境检测之前，让「自动安装后无需再配置任何东西」的承诺成立。
/// 显式配置仍是最高优先级，可以覆盖托管运行时。
pub fn resolve(settings: &Settings, data_dir: &Path) -> NodeInfo {
    if let Some(path) = settings.node_path.as_ref() {
        let info = probe(Path::new(path));
        if info.ok {
            return info;
        }
        // 回退到托管运行时 / 自动检测；保留原因以便 UI 说明配置路径为何被拒绝。
        if let Some(mut detected) = probe_managed(data_dir) {
            detected.reason = format!(
                "配置的路径不可用（{}），已自动回退到：{}",
                info.reason, detected.path
            );
            return detected;
        }
        let scan = probe_environment(&environment_candidates());
        if let Some(mut detected) = scan.usable {
            detected.reason = format!(
                "配置的路径不可用（{}），已自动回退到：{}",
                info.reason, detected.path
            );
            return detected;
        }
        let detail = scan
            .near_miss
            .as_ref()
            .map(|n| format!("环境中只有 {}（{}）。", n.path, n.reason))
            .unwrap_or_default();
        let guidance = if scan.near_miss.is_some() {
            NODE_TOO_OLD_GUIDANCE
        } else {
            NO_NODE_FOUND_GUIDANCE
        };
        return NodeInfo {
            path: path.clone(),
            ok: false,
            version: None,
            reason: format!("配置路径不可用且未找到可用 node。{detail}{guidance}"),
        };
    }
    if let Some(info) = probe_managed(data_dir) {
        return info;
    }
    let scan = probe_environment(&environment_candidates());
    match scan.usable {
        Some(info) => info,
        None => {
            let detail = scan
                .near_miss
                .as_ref()
                .map(|n| format!("检测到 {}（{}）。", n.path, n.reason))
                .unwrap_or_default();
            let guidance = if scan.near_miss.is_some() {
                NODE_TOO_OLD_GUIDANCE
            } else {
                NO_NODE_FOUND_GUIDANCE
            };
            NodeInfo {
                path: scan.near_miss.clone().map(|n| n.path).unwrap_or_default(),
                version: None,
                ok: false,
                reason: format!(
                    "未检测到满足 dsh 要求（^22.19 || >=24）的 Node.js。{detail}{guidance}"
                ),
            }
        }
    }
}

/// 为安装 kernel 寻找可用的 pnpm 可执行文件。
///
/// 优先使用显式配置的路径，其次是与解析出的 `node` 同目录的版本，
/// 最后是 PATH。pnpm 是 kernel 版本的安装器，它不随 Node 一起提供，
/// 因此缺失的 pnpm 会以安装期错误形式暴露，并附带初始化指引。
/// Windows 上的探测同时兼容 `.cmd` shim（npm 前缀布局）和独立的
/// `.exe` 安装。
pub fn resolve_pnpm(settings: &Settings, node_dir: &Path) -> Option<PathBuf> {
    if let Some(path) = settings.pnpm_path.as_ref() {
        if Path::new(path).is_file() {
            return Some(PathBuf::from(path));
        }
    }
    if let Some(p) = which_in_dir("pnpm", node_dir) {
        return Some(p);
    }
    for dir in path_dirs() {
        if let Some(p) = which_in_dir("pnpm", &dir) {
            return Some(p);
        }
    }
    None
}

/// 寻找随解析出的 node 一同发布的 npm 可执行文件。npm 仅作为 pnpm 缺失
/// 时的备选安装器使用；在常见布局下，它位于 `node.exe` / `node` 旁
/// 并出现在 PATH 中。若存在显式的 `settings.npm_path`（拥有便携版 npm
/// 的高级用户），它优先被采用；随后按 node 同目录、PATH 顺序搜索，
/// 探测方式与 pnpm 相同，使用 `.cmd` / `.exe` / 裸名三种形式。
pub fn find_npm(settings: &Settings, node_dir: &Path) -> Option<PathBuf> {
    if let Some(path) = settings.npm_path.as_ref() {
        if Path::new(path).is_file() {
            return Some(PathBuf::from(path));
        }
    }
    if let Some(p) = which_in_dir("npm", node_dir) {
        return Some(p);
    }
    for dir in path_dirs() {
        if let Some(p) = which_in_dir("npm", &dir) {
            return Some(p);
        }
    }
    None
}

/// 解析 pnpm，仅当 Node 存在时按需安装。
///
/// 首先尝试三级查找（`settings.pnpm_path`、与 `node` 同目录、PATH）。
/// 若都未命中且 `npm` 可用，则运行一次 `npm install -g pnpm`，把每一行
/// 通过 `on_progress` 流式回传，并再次执行查找以返回刚装好的二进制。
/// 完整的 npm 输出会被追加到 `logs_dir` 下由 `log_spec` 标识的、按天滚动
/// 的 pnpm 安装日志；当日完整路径在调用时计算，这样用户可以立即从模态
/// 框的标签列表中打开日志。
pub fn ensure_pnpm(
    settings: &Settings,
    node_dir: &Path,
    logs_dir: &Path,
    log_spec: &LogSpec,
    mut on_progress: impl FnMut(&str),
) -> Result<PathBuf, String> {
    let log_path = log_spec.path_for(logs_dir, &crate::process::current_date_string());
    if let Some(p) = resolve_pnpm(settings, node_dir) {
        return Ok(p);
    }
    let npm = find_npm(settings, node_dir).ok_or_else(|| {
        format!(
            "未检测到 pnpm，也未找到可用的 npm（无法自动安装）。{node}\n\n可选操作：① 装好 Node 后在终端执行 `npm install -g pnpm`；② 从 https://pnpm.io/installation 按平台安装；③ 在「设置」中手动指定已下载的 pnpm 可执行文件路径。",
            node = NO_NODE_FOUND_GUIDANCE,
        )
    })?;
    on_progress("未检测到 pnpm，正在通过 npm 自动安装（首次需要联网，常见 30 秒~2 分钟）");
    let cwd = node_dir.to_path_buf();
    // `npm` 是一个带 `#!/usr/bin/env node` shebang 的 Node.js 脚本，
    // 并且在 `install -g` 期间还会运行 lifecycle 脚本。把已校验的 node
    // bin 目录与 `npm.parent()` 都放到 PATH 前部，让子进程即便在 GUI
    // 仅继承到 launchd PATH 时也能解析 `node`。
    let npm_dir = npm.parent().unwrap_or(std::path::Path::new("."));
    let status = run_with_progress(
        &npm,
        &["install", "-g", "pnpm"],
        &cwd,
        logs_dir,
        log_spec,
        &[node_dir, npm_dir],
        |line| on_progress(line),
    )
    .map_err(|e| {
        format!(
            "无法运行 npm 以自动安装 pnpm：{e}。请检查 Node.js 安装，或在「设置」中手动指定 pnpm 路径。完整日志：{log}",
            log = log_path.display()
        )
    })?;
    if !status.success() {
        let code = status
            .code()
            .map(|c| c.to_string())
            .unwrap_or_else(|| "?".into());
        return Err(format!(
            "自动安装 pnpm 失败（npm 退出码 {code}）。常见原因：网络受限、企业代理未配置 npm registry、npm prefix 权限不足（macOS/Linux 默认 prefix 需写 /usr/local，建议改用 nvm/fnm/volta 等用户级版本管理器），或 npm 本身不可用。完整日志：{log}\n\n可选操作：① 在终端执行 `npm install -g pnpm`；② 从 https://pnpm.io/installation 按平台安装；③ 在「设置」中指定已下载的 pnpm 可执行文件路径。",
            log = log_path.display()
        ));
    }
    // `npm install -g` 会把新脚本写到 npm 前缀的 bin 目录，
    // 在常见布局下该目录已经在 PATH 上——再次跑三级查找就能拿到刚装好
    // 的二进制。回退到显式配置的前缀是为了应对用户自定义 prefix
    // 不在 PATH 上的少见情况。
    if let Some(p) = resolve_pnpm(settings, node_dir) {
        on_progress("pnpm 已就绪");
        return Ok(p);
    }
    if let Ok(prefix) = npm_prefix(&npm, &cwd) {
        let candidate = prefix.join(if cfg!(windows) { "pnpm.cmd" } else { "pnpm" });
        if candidate.is_file() {
            on_progress("pnpm 已就绪");
            return Ok(candidate);
        }
    }
    Err(format!(
        "npm install -g pnpm 已完成但仍未在常见位置找到 pnpm 可执行文件。请检查 npm prefix 与 PATH 设置、确认 npm prefix/bin 在本应用 PATH 内，或在「设置」中手动指定 pnpm 路径。完整日志：{}",
        log_path.display()
    ))
}

/// 询问 npm 会把全局包安装到哪里——也就是我们即将查找的脚本 bin
/// 的父目录。在 Windows 上 npm 是 `.cmd` 批处理 shim，因此子进程通过
/// `process::script_output` 启动，它把批处理文件交给 `%ComSpec% /C`
/// 执行，并注入合并后的 PATH。
fn npm_prefix(npm: &Path, cwd: &Path) -> Result<PathBuf, String> {
    let npm_dir = npm.parent().unwrap_or(std::path::Path::new("."));
    let (success, stdout, _stderr) =
        crate::process::script_capture(npm, &["config", "get", "prefix"], cwd, &[npm_dir])
            .map_err(|e| format!("无法读取 npm prefix：{e}"))?;
    if !success {
        return Err("npm config get prefix 失败（请检查 npm 与 Node 环境）".into());
    }
    let prefix = stdout.trim();
    if prefix.is_empty() {
        return Err("npm prefix 为空".into());
    }
    Ok(PathBuf::from(prefix))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(not(windows))]
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// `temp_root` 的顺序编号，使并行测试不会在同一个
    /// `std::env::temp_dir()` 暂存空间上冲突。仅由 `cfg(not(windows))`
    /// 守卫下的 `resolve_alias` 测试使用。
    #[cfg(not(windows))]
    #[allow(dead_code)]
    static TEMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

    /// 在 `std::env::temp_dir()` 下创建一个全新的暂存目录。每次调用都返回
    /// 唯一路径，并清掉该路径上之前残留的内容，避免之前崩溃的运行留下的
    /// 旧数据混入本次测试。
    #[cfg(not(windows))]
    #[allow(dead_code)]
    fn temp_root(tag: &str) -> PathBuf {
        let seq = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "dsh-xlink-node-test-{tag}-{}-{seq}",
            std::process::id(),
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn vb(major: u32, minor: u32, patch: u32) -> PathBuf {
        // 排序只读取元组，路径内容对顺序没有意义。
        PathBuf::from(format!("/fake/{major}.{minor}.{patch}"))
    }

    #[test]
    fn parse_version_accepts_v_prefix_and_prerelease() {
        // 函数读取的是 `node --version` 输出，它始终带有开头的 `v`；
        // 裸版本号不在契约范围内。
        assert_eq!(parse_version("v22.19.0"), Some((22, 19, 0)));
        assert_eq!(parse_version("v24.0.0-rc.1"), Some((24, 0, 0)));
        assert_eq!(parse_version("not a version"), None);
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn normalize_spec_strips_v_and_whitespace() {
        assert_eq!(normalize_spec(Some("v22.19.0")), Some("22.19.0".into()));
        assert_eq!(normalize_spec(Some("  22  ")), Some("22".into()));
        assert_eq!(normalize_spec(Some("")), None);
        assert_eq!(normalize_spec(None), None);
    }

    #[test]
    fn format_version_round_trips() {
        assert_eq!(format_version((22, 19, 0)), "22.19.0");
        assert_eq!(format_version((24, 0, 0)), "24.0.0");
    }

    #[test]
    fn order_nvm_versions_picks_exact_default_first() {
        let installed = vec![
            ((24, 5, 0), vb(24, 5, 0)),
            ((22, 19, 0), vb(22, 19, 0)),
            ((22, 20, 0), vb(22, 20, 0)),
        ];
        let ordered = order_nvm_versions(installed, Some("v22.19.0"));
        // 精确匹配 `v22.19.0` 排在最前，其余兼容的安装按版本倒序跟随。
        assert_eq!(ordered, vec![vb(22, 19, 0), vb(24, 5, 0), vb(22, 20, 0)],);
    }

    #[test]
    fn order_nvm_versions_resolves_partial_spec_to_newest_match() {
        // `22` 解析为已安装的最高 v22.x.y——镜像 nvm 的行为。
        // v22.10.0 故意低于 `^22.19`，让测试聚焦在 spec 解析路径上；
        // engines 范围的丢弃逻辑另有专门测试覆盖。
        let installed = vec![
            ((22, 19, 0), vb(22, 19, 0)),
            ((22, 20, 0), vb(22, 20, 0)),
            ((24, 5, 0), vb(24, 5, 0)),
        ];
        let ordered = order_nvm_versions(installed, Some("22"));
        assert_eq!(ordered, vec![vb(22, 20, 0), vb(24, 5, 0), vb(22, 19, 0)]);
    }

    #[test]
    fn order_nvm_versions_drops_incompatible() {
        // 18.0.0 低于 `^22.19 || >=24`，必须从探测列表中排除——
        // 启动它只会白白消耗一个子进程，结果还是被 engines 检查拒掉。
        let installed = vec![
            ((18, 19, 0), vb(18, 19, 0)),
            ((22, 19, 0), vb(22, 19, 0)),
            ((24, 5, 0), vb(24, 5, 0)),
        ];
        let ordered = order_nvm_versions(installed, None);
        assert_eq!(ordered, vec![vb(24, 5, 0), vb(22, 19, 0)]);
    }

    #[test]
    fn order_nvm_versions_unknown_spec_falls_back_to_desc_scan() {
        // `lts/hydrogen` 链接到 `20.9.0`，而 20.9.0 在这里只以 alias 名
        // `default` 安装。没有匹配时队首留空，其余兼容版本仍然按版本倒序
        // 输出。
        let installed = vec![((22, 19, 0), vb(22, 19, 0)), ((24, 5, 0), vb(24, 5, 0))];
        let ordered = order_nvm_versions(installed, Some("lts/hydrogen"));
        assert_eq!(ordered, vec![vb(24, 5, 0), vb(22, 19, 0)]);
    }

    #[test]
    #[cfg(not(windows))]
    fn resolve_alias_follows_chain_to_concrete_version() {
        let root = temp_root("alias-chain");
        let alias_dir = root.join("alias");
        fs::create_dir_all(&alias_dir).unwrap();
        fs::write(alias_dir.join("default"), "lts/hydrogen\n").unwrap();
        fs::create_dir_all(alias_dir.join("lts")).unwrap();
        fs::write(alias_dir.join("lts").join("hydrogen"), "20.9.0").unwrap();
        assert_eq!(resolve_alias(&root, "default"), Some("20.9.0".into()));
    }

    #[test]
    #[cfg(not(windows))]
    fn resolve_alias_returns_partial_spec_when_not_a_file() {
        let root = temp_root("alias-partial");
        fs::create_dir_all(root.join("alias")).unwrap();
        fs::write(root.join("alias").join("default"), "22\n").unwrap();
        // `alias/22` 不存在——这是 nvm 部分 spec 的行为——因此链在裸 `22`
        // 处终止，由调用方拿它去匹配已安装的版本。
        assert_eq!(resolve_alias(&root, "default"), Some("22".into()));
    }

    #[test]
    #[cfg(not(windows))]
    fn resolve_alias_bounds_cycles() {
        let root = temp_root("alias-cycle");
        let alias_dir = root.join("alias");
        fs::create_dir_all(&alias_dir).unwrap();
        fs::write(alias_dir.join("a"), "b").unwrap();
        fs::write(alias_dir.join("b"), "a").unwrap();
        // 跳数上限会中止解析；最终 spec 就是循环停下的那个值，不会陷入死循环。
        // 它并不对应一个已安装的版本，因此调用方会回退到倒序扫描。
        assert_eq!(
            resolve_alias(&root, "a").as_deref(),
            Some("a"),
            "alias resolution must terminate even when the chain cycles",
        );
    }

    #[test]
    #[cfg(not(windows))]
    fn resolve_alias_reports_empty_alias_file_as_missing() {
        let root = temp_root("alias-empty");
        fs::create_dir_all(root.join("alias")).unwrap();
        fs::write(root.join("alias").join("default"), "").unwrap();
        assert_eq!(resolve_alias(&root, "default"), None);
    }

    /// 「设备上完全没有 Node」的消息是全新机器用户在管理面板里看到的合同
    /// 内容。它必须给出三种独立的安装方式（版本管理器 / 包管理器 /
    /// 官方安装包）以及手动路径的兜底入口，让用户可以照自己本来就会用
    /// 的方式去安装。
    #[test]
    fn no_node_found_guidance_lists_three_install_paths() {
        let msg = NO_NODE_FOUND_GUIDANCE;
        assert!(msg.contains("nvm"), "should mention nvm");
        assert!(
            msg.contains("fnm") || msg.contains("volta"),
            "should mention an alternative version manager",
        );
        assert!(msg.contains("brew"), "should mention Homebrew on macOS");
        assert!(msg.contains("winget"), "should mention winget on Windows");
        assert!(
            msg.contains("https://nodejs.org/"),
            "should point at the official installer",
        );
        assert!(
            msg.contains("「设置」"),
            "should remind the user about the manual-path setting",
        );
    }

    /// 「Node 太老」的消息是装有过时系统 Node 的用户看到的合同内容。
    /// 它必须指向升级命令（而非全新安装）以及手动路径的位置，使用户
    /// 即便想保留旧 Node，也能在「设置」中指向更新版本。
    #[test]
    fn node_too_old_guidance_points_at_upgrade_paths() {
        let msg = NODE_TOO_OLD_GUIDANCE;
        assert!(msg.contains("nvm install 24"), "should mention nvm upgrade");
        assert!(
            msg.contains("https://nodejs.org/"),
            "should point at the official installer",
        );
        assert!(msg.contains("「设置」"), "should mention the manual path");
        // 「太老」分支没必要推荐 apt/brew/winget，因为用户已经有 Node；
        // 这些全新安装命令会误导用户，并让消息显得冗长。
        assert!(
            !msg.contains("brew install"),
            "fresh-install commands do not belong in an upgrade message",
        );
    }
}
