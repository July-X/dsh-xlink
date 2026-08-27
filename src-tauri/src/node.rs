//! Locating and validating the Node.js runtime that runs the kernel.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

use crate::process::{quiet, run_with_progress};
use crate::settings::Settings;

/// The engine range dsh declares (`^22.19.0 || >=24.0.0`).
const MIN_COMPATIBLE: (u32, u32, u32) = (22, 19, 0);
const MAJOR_ALT_FLOOR: u32 = 24;

/// What the shell found out about a Node candidate.
#[derive(Debug, Clone, Serialize)]
pub struct NodeInfo {
    pub path: String,
    pub version: Option<String>,
    pub ok: bool,
    pub reason: String,
}

/// Parse `v22.19.0`-style output into (major, minor, patch).
fn parse_version(output: &str) -> Option<(u32, u32, u32)> {
    let text = output.trim().strip_prefix('v')?;
    let mut parts = text.split('.');
    // Without the explicit `parse::<u32>()` annotations the compiler reports
    // E0282 (`type annotations needed`) because each `parse()` is generic
    // over the target integer type and has no constraint on its own.
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

/// Whether a parsed version satisfies the dsh engine requirement.
fn compatible((major, minor, _patch): (u32, u32, u32)) -> bool {
    (major == MIN_COMPATIBLE.0 && minor >= MIN_COMPATIBLE.1) || major >= MAJOR_ALT_FLOOR
}

/// Ask a node executable for its version.
pub fn version_of(path: &Path) -> Option<String> {
    let mut cmd = Command::new(path);
    cmd.arg("--version");
    let output = quiet(&mut cmd).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let version = text.trim().to_string();
    (parse_version(&text).is_some()).then_some(version)
}

/// Probe a candidate node executable and report how usable it is.
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

/// Drop the trailing `.exe` for PATH lookup on Windows.
fn exe_name(name: &str) -> String {
    if cfg!(windows) && !name.to_ascii_lowercase().ends_with(".exe") {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

/// Candidate executable names probed in order when looking up a tool on
/// Windows. Windows PATH lookups honour `PATHEXT` (default
/// `.COM;.EXE;.BAT;.CMD;…`), and Node-adjacent tools are overwhelmingly
/// shipped as `.cmd` shims into the user-level npm prefix
/// (`%AppData%\npm\pnpm.cmd`) instead of `.exe`. Probing `.cmd` first
/// matches the layout every npm `install -g` produces, then falls through
/// to `.exe` (system-wide installs and `pnpm` standalone) and finally the
/// bare name (PATH entries that already include an extension). Outside
/// Windows only the bare name is valid.
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

/// Directories to scan when looking up a tool on PATH. Detection must see
/// the same PATH the shell stamps onto spawned children
/// (`crate::env::merged_path`): a GUI-launched Windows shell inherits only
/// the system PATH, so tools installed into the user-level npm prefix
/// (`%AppData%\npm`) are invisible to a raw `std::env::var_os("PATH")`
/// scan even though the user can run them from any terminal. On other
/// platforms the merged PATH mirrors the process PATH.
fn path_dirs() -> impl Iterator<Item = PathBuf> {
    std::env::split_paths(crate::env::merged_path())
}

/// Find `node` on the PATH.
fn from_path() -> Option<PathBuf> {
    for dir in path_dirs() {
        let candidate = dir.join(exe_name("node"));
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Well-known install locations probed when `node` is not on the PATH.
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

// --- nvm-managed installations ----------------------------------------------
//
// GUI shells launch with a minimal PATH (launchd on macOS, the Window
// Station system PATH merged from `HKCU\Environment` on Windows — see
// `crate::env`), so a Node managed by nvm is invisible to the PATH walk
// even though it is exactly the runtime the user intends to use. nvm
// keeps every installed version on disk in a predictable layout, so the
// shell discovers them directly instead of depending on the inherited
// environment:
//
// - nvm-sh (macOS/Linux): `$NVM_DIR` (default `~/.nvm`) with versions
//   under `versions/node/<vX.Y.Z>/bin/node`; the preferred one recorded
//   in `alias/default`, whose content may itself name another alias
//   (`lts/hydrogen`) or a partial version (`22`).
// - nvm-windows: `%NVM_HOME%` (default `%APPDATA%\nvm`) with versions
//   under `vX.Y.Z\node.exe`; the version selected by `nvm use` is exposed
//   as a directory junction named by `%NVM_SYMLINK%`.

/// How many alias-file indirections [`resolve_alias`] follows before
/// giving up. `default` → `lts/hydrogen` → `20.9.0` is the common depth;
/// repeated alias specs stop immediately, while the cap protects against
/// unusually long acyclic chains and falls through to the newest-first scan.
#[cfg(not(windows))]
const ALIAS_MAX_HOPS: usize = 5;

/// The nvm-sh root directory (`$NVM_DIR`, defaulting to `~/.nvm`) when it
/// exists on disk.
#[cfg(not(windows))]
fn nvm_root() -> Option<PathBuf> {
    let candidate = std::env::var_os("NVM_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::kernel::dirs_home().join(".nvm"));
    candidate.is_dir().then_some(candidate)
}

/// Directories nvm-windows may keep versions in: `$NVM_HOME` when the
/// environment carries it, plus the default `%APPDATA%\nvm` location. The
/// GUI process frequently inherits neither variable (see `crate::env`),
/// so the default keeps detection working without it.
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

/// Resolve an nvm-sh alias to a concrete version spec by following the
/// alias-file chain: `alias/<name>` may name another alias and ends at a
/// version string, possibly partial (`22`). Returns the final spec without
/// checking that any installed version matches it.
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
            // `spec` does not name another alias file, so it already is
            // the concrete (possibly partial) version spec.
            Err(_) => break,
        }
    }
    let trimmed = spec.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// Trim and strip the leading `v` from an alias-resolved version spec so
/// comparisons against installed directory names share one form.
fn normalize_spec(spec: Option<&str>) -> Option<String> {
    let s = spec?.trim();
    let s = s.strip_prefix('v').unwrap_or(s);
    (!s.is_empty()).then(|| s.to_string())
}

/// Render a parsed tuple back to the dotted form used for alias-spec
/// comparison (`(22, 19, 0)` → `"22.19.0"`).
fn format_version((major, minor, patch): (u32, u32, u32)) -> String {
    format!("{major}.{minor}.{patch}")
}

/// Order discovered version-manager installations for probing.
///
/// The installation matching `default_spec` comes first — that is the
/// version the user pinned with `nvm alias default`. Exact match wins,
/// then the highest installed version the spec prefixes (`22` resolves to
/// the newest v22.x.y), mirroring how nvm itself interprets partial specs.
/// Remaining engine-compatible installations follow newest-first, so an
/// unpinned machine still gets a usable Node. Installations whose declared
/// version cannot satisfy the engines range are dropped entirely: probing
/// them would only produce a rejection the caller renders better than a
/// wasted child spawn.
fn order_nvm_versions(
    mut installed: Vec<((u32, u32, u32), PathBuf)>,
    default_spec: Option<&str>,
) -> Vec<PathBuf> {
    // Newest first; equal tuples imply equal directories so no further
    // tiebreak exists.
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

/// Node executables managed by nvm-sh (macOS/Linux), in probe order: the
/// `default`-alias installation first, then every other engine-compatible
/// installation newest-first. Version directories whose names do not parse
/// (custom builds dropped in by hand) are appended last so the probe, not
/// the filename heuristic, decides.
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

/// Node executables managed by nvm-windows, in probe order: the active
/// junction (`%NVM_SYMLINK%`, what `nvm use` selected) first, then every
/// engine-compatible installation under `%NVM_HOME%` / `%APPDATA%\nvm`
/// newest-first. Unparseable directory names are appended last and left to
/// the probe.
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
    // nvm-windows records its selection in the junction rather than an
    // alias file, so there is no default spec to resolve here.
    out.extend(order_nvm_versions(installed, None));
    out.extend(unparsed);
    out
}

/// Candidate node executables in auto-detection order: the PATH hit first
/// (what the launching shell resolved — a terminal-run dev shell after
/// `nvm use` lands here), then nvm-managed installations, then well-known
/// system locations.
fn environment_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(found) = from_path() {
        out.push(found);
    }
    out.extend(nvm_candidates());
    out.extend(common_locations().into_iter().filter(|p| p.is_file()));
    out
}

/// Result of probing the ordered environment candidates.
struct EnvironmentScan {
    /// First candidate that satisfies the engines range, if any.
    usable: Option<NodeInfo>,
    /// First candidate that runs but fails the range — surfaced when
    /// nothing qualifies so the message can say "your Node is too old"
    /// instead of pretending no Node exists.
    near_miss: Option<NodeInfo>,
}

/// Probe ordered candidates until one satisfies the engines range,
/// remembering the closest failure along the way.
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

/// Guidance when auto-detection finds no Node.js installation anywhere on
/// disk — common on a fresh machine before any setup runs, or when the
/// user has only just uninstalled the last runtime. Three independent
/// install paths are listed because each user has a different preference;
/// they pick whichever they would have used anyway, and the next status
/// refresh (or the「检测 Node」button) re-runs detection.
const NO_NODE_FOUND_GUIDANCE: &str =
    "请通过下列任一方式安装 Node.js 22.19+（或 >=24）：\n\
     • 版本管理器（推荐）：nvm 用户执行 `nvm install 24 && nvm alias default 24`；fnm 用户执行 `fnm install 24 && fnm default 24`；volta 用户执行 `volta install node@24`。\n\
     • 系统包管理器：macOS `brew install node@24`；Ubuntu/Debian 装 NodeSource 后 `apt install nodejs`；Windows `winget install OpenJS.NodeJS.LTS`。\n\
     • 官方安装包：从 https://nodejs.org/ 下载安装包，安装后重启本应用，或在「设置」中手动指定 node 可执行文件路径。";

/// Guidance when auto-detection finds a Node.js installation but its
/// declared version falls short of the engines range (`^22.19 || >=24`).
/// Distinguishing this from "no Node at all" matters because the action
/// is upgrade-in-place, not fresh-install.
const NODE_TOO_OLD_GUIDANCE: &str =
    "请升级 Node 到 22.19+（或 >=24）：使用 nvm 的用户执行 `nvm install 24 && nvm alias default 24`；使用 fnm 的用户执行 `fnm install 24 && fnm default 24`；也可从 https://nodejs.org/ 下载新版安装包覆盖安装，或在「设置」中手动指定新版 node 可执行文件路径。";

/// Resolve the node executable from explicit config, then the environment.
pub fn resolve(settings: &Settings) -> NodeInfo {
    if let Some(path) = settings.node_path.as_ref() {
        let info = probe(Path::new(path));
        if info.ok {
            return info;
        }
        // Fall through to detection; keep the reason so the UI can explain
        // why the configured path was rejected.
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

/// Find a usable pnpm executable for installing kernels.
///
/// Prefer an explicit config path, then the folder next to the resolved
/// `node`, then the PATH. pnpm is the installer for kernel versions; it is
/// not bundled with Node, so a missing pnpm surfaces as an install-time
/// error with setup guidance. The Windows probe tolerates both `.cmd`
/// shims (the npm-prefix layout) and standalone `.exe` installs.
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

/// Find an npm executable that ships with the resolved node. npm is needed
/// only as a fallback installer when pnpm is missing; on the common layout
/// it sits next to `node.exe` / `node` and on PATH. An explicit
/// `settings.npm_path` wins when present (advanced users with a portable
/// npm), then the node-sibling and PATH searches use the same `.cmd` /
/// `.exe` / bare-name probe as pnpm.
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

/// Resolve pnpm, installing it on demand when only Node is present.
///
/// The three-tier lookup (`settings.pnpm_path`, alongside `node`, PATH) is
/// tried first. When none of them hit and `npm` is reachable, run
/// `npm install -g pnpm` once, stream every line back through `on_progress`,
/// and re-run the lookup so the just-installed binary is returned. The full
/// npm transcript is written to `log_path` so the user can inspect failures
/// without rerunning the install.
pub fn ensure_pnpm(
    settings: &Settings,
    node_dir: &Path,
    log_path: &Path,
    mut on_progress: impl FnMut(&str),
) -> Result<PathBuf, String> {
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
    // `npm` is a Node.js script with a `#!/usr/bin/env node` shebang and
    // also runs lifecycle scripts during `install -g`. Prepend both the
    // validated node bin dir and `npm.parent()` so the child can resolve
    // `node` even when the GUI inherited a launchd-only PATH.
    let npm_dir = npm.parent().unwrap_or(std::path::Path::new("."));
    let status = run_with_progress(
        &npm,
        &["install", "-g", "pnpm"],
        &cwd,
        log_path,
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
    // `npm install -g` writes the new script into the npm prefix bin dir,
    // which on the common layout is already on PATH — re-running the
    // three-tier resolver picks the just-installed binary up. Falling back
    // to the explicitly-configured prefix handles the unusual case where
    // the user has a custom prefix that PATH does not see.
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

/// Ask npm where it would install global packages — the parent dir of the
/// script bin we are about to look in. npm is a `.cmd` batch shim on
/// Windows, so the spawn goes through `process::script_output`, which
/// routes batch files through `%ComSpec% /C` and stamps the merged PATH.
fn npm_prefix(npm: &Path, cwd: &Path) -> Result<PathBuf, String> {
    let npm_dir = npm.parent().unwrap_or(std::path::Path::new("."));
    let output = crate::process::script_output(npm, &["config", "get", "prefix"], cwd, &[npm_dir])
        .map_err(|e| format!("无法读取 npm prefix：{e}"))?;
    if !output.status.success() {
        return Err(format!(
            "npm config get prefix 失败（退出码 {:?}）",
            output.status.code()
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let prefix = text.trim();
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

    /// Sequential tag for `temp_root`, so parallel test runs do not
    /// collide on the same `std::env::temp_dir()` scratch space. Used
    /// only from the `cfg(not(windows))`-gated `resolve_alias` tests.
    #[cfg(not(windows))]
    #[allow(dead_code)]
    static TEMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

    /// Build a fresh scratch directory under `std::env::temp_dir()`. Each
    /// call returns a unique path and removes any previous contents at that
    /// path so a stale scratch from a crashed earlier run cannot leak in.
    #[cfg(not(windows))]
    #[allow(dead_code)]
    fn temp_root(tag: &str) -> PathBuf {
        let seq = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "dsh-desktop-node-test-{tag}-{}-{seq}",
            std::process::id(),
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn vb(major: u32, minor: u32, patch: u32) -> PathBuf {
        // Path content is irrelevant for ordering — only the tuple is read.
        PathBuf::from(format!("/fake/{major}.{minor}.{patch}"))
    }

    #[test]
    fn parse_version_accepts_v_prefix_and_prerelease() {
        // The function reads `node --version` output, which always carries
        // the leading `v`; the bare form is not a contract.
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
        // exact match `v22.19.0` heads the list, the remaining compatible
        // installations follow newest-first.
        assert_eq!(ordered, vec![vb(22, 19, 0), vb(24, 5, 0), vb(22, 20, 0)],);
    }

    #[test]
    fn order_nvm_versions_resolves_partial_spec_to_newest_match() {
        // `22` resolves to the highest installed v22.x.y — mirroring nvm.
        // v22.10.0 is intentionally below `^22.19` so the test stays
        // focused on the spec-resolution path; the engines-range drop is
        // covered separately.
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
        // 18.0.0 falls short of `^22.19 || >=24` and must be omitted from
        // the probe list — spawning it would only burn a child process
        // for a result the engine-range check rejects anyway.
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
        // `lts/hydrogen` chains to `20.9.0`, which is installed only under
        // the alias name `default` here. With no installed match the head
        // stays empty and the compatible ones still come through newest-
        // first.
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
        // `alias/22` does not exist — nvm partial-spec behavior — so the
        // chain terminates on the bare `22` and the caller matches it
        // against installed versions.
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
        // The hop cap stops the resolution; the final spec is whatever the
        // loop ended on, never a hang. It does not name an installed
        // version, so callers fall back to the desc scan.
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

    /// The "no Node anywhere" message is the contract a user on a fresh
    /// machine reads in the management panel. It must give three
    /// independent install paths (version manager / package manager /
    /// official installer) and the manual-path escape hatch, so the user
    /// can act on whichever they would have used anyway.
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

    /// The "Node too old" message is the contract a user with an
    /// outdated system install reads. It must point at upgrade commands
    /// (not fresh-install), and at the manual-path slot so they can keep
    /// their old install and still point the shell at a newer one if
    /// they prefer.
    #[test]
    fn node_too_old_guidance_points_at_upgrade_paths() {
        let msg = NODE_TOO_OLD_GUIDANCE;
        assert!(msg.contains("nvm install 24"), "should mention nvm upgrade");
        assert!(
            msg.contains("https://nodejs.org/"),
            "should point at the official installer",
        );
        assert!(msg.contains("「设置」"), "should mention the manual path");
        // The "too old" branch never has to recommend apt/brew/winget
        // because the user already has a Node; those fresh-install
        // commands are misleading and would clutter the message.
        assert!(
            !msg.contains("brew install"),
            "fresh-install commands do not belong in an upgrade message",
        );
    }
}
