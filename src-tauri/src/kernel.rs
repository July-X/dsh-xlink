//! 内核生命周期：安装固定版本的内核、管理当前激活版本，并启动 / 停止壳所内嵌的
//! `dsh web` 进程。
//!
//! 壳的元数据与内核自身的数据并列存放，统一位于 harness home 目录下：
//! `<dsh_home>/desktop/`（默认即 `~/.dsh/desktop/`）。安装一个内核版本时，
//! 在专属目录中运行 pnpm：
//!
//! ```text
//! <dsh_home>/desktop/kernels/<version>/
//!   package.json                     # pnpm 安装的最小桩包
//!   node_modules/@deepseek-ai/dsh/   # 固定版本的内核
//! ```
//!
//! 安装使用 `hoisted` node-linker，使 `node_modules` 保持平铺——与 npm 生成的
//! 布局一致——内核入口路径可直接以普通路径解析，无需依赖支持符号链接的文件系统。
//! pnpm 的全局内容寻址存储让重复安装其他版本比冷启动的 npm 安装快得多。
//! `append-only` reporter 会把每个生命周期事件以一行日志输出到 stdout，
//! 实时流式推送给 UI。
//!
//! 当前激活版本记录在 `<dsh_home>/desktop/active.txt` 中。

use std::fs;
use std::io::{self, Write};
use std::net::TcpStream;
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Stdio};
use std::time::Duration;

use crate::process::{
    attach_log_drainers, build_log_kind, quiet, run_with_progress, run_with_progress_at, LogSpec,
};

use serde::Serialize;
use tauri::Manager;

use crate::error::AppError;
use crate::settings::{self, Settings};

/// dsh 自身的 home 目录名（参见 `@deepseek-ai/dsh-home-paths`）。
pub const DSH_HOME_DIR_NAME: &str = ".dsh";
/// *release* 构建下壳的元数据根目录，位于 dsh home 下：`<dsh_home>/desktop/`。
const SHELL_SUBDIR_RELEASE: &str = "desktop";
/// *debug* 构建（`tauri dev`）下壳的元数据根目录。该路径与 release 路径并列
/// 存在但名称不同，以便开发者在同一台机器上同时运行 `tauri dev` 和已安装的
/// release 壳，二者的 settings.json / kernels / active.txt / kernel.pid / port
/// 不会互相覆盖。两个构建读取各自的 data dir，因此 dev 壳看到的是自己安装的
/// 内核集合和自己正在运行的内核 pid，release 壳看到的也是自己的。
const SHELL_SUBDIR_DEV: &str = "desktop-dev";
/// 本构建实际使用的子目录，在编译期选定。
const SHELL_SUBDIR: &str = if cfg!(debug_assertions) {
    SHELL_SUBDIR_DEV
} else {
    SHELL_SUBDIR_RELEASE
};
/// 内核 web 服务器的默认端口。debug 构建默认为 3091（比 release 的 3090 多一），
/// 这样 `tauri dev` 与已安装的 release 壳可以在同一台机器上运行而不会在 loopback 上
/// 冲突。该值仅在 settings.json 缺失或没有 `port` 字段时生效；用户一旦持久化保存了
/// 某个值，就会原样读回使用。
pub const DEFAULT_PORT: u16 = if cfg!(debug_assertions) { 3091 } else { 3090 };
/// 内核前端静态包的名称。npm 发布包会排除 dist 下的 source map，导致
/// WebKit Inspector 在 debug 壳中对每个带引用的脚本报告 404。
const WEB_FRONTEND_PACKAGE: &str = "@deepseek-ai/dsh-web-frontend";
/// 不修改内核 JS，只为已存在 sourceMappingURL 的缺失目标生成这个最小
/// v3 map；它让 DevTools 结束请求，不会伪造任何源码映射。
pub(crate) const EMPTY_SOURCE_MAP: &str = r#"{"version":3,"sources":[],"names":[],"mappings":""}"#;

/// 已安装包中内核 CLI 入口的相对路径。
const KERNEL_BIN_REL: &str = "node_modules/@deepseek-ai/dsh/lib/bin.js";
const MAX_ORPHAN_CANDIDATES: usize = 256;

/// 磁盘上已安装的一个内核版本。
#[derive(Debug, Clone, Serialize)]
pub struct InstalledVersion {
    pub version: String,
    pub active: bool,
    /// 仅内核入口文件（`KERNEL_BIN_REL`）的大小——一种廉价的完整性信号，
    /// 而非整个安装的占用体积。
    pub size_bytes: u64,
}

/// UI 在每次状态刷新时渲染的快照。
#[derive(Debug, Clone, Serialize)]
pub struct KernelStatus {
    pub installed: Vec<InstalledVersion>,
    pub active: Option<String>,
    pub active_installed: bool,
    pub running: bool,
    pub port: u16,
    /// 壳元数据根目录的展示形式，当路径位于用户 home 下时把 home 前缀
    /// 缩短为 `~`。UI 把该值显示在「打开」按钮旁边，按钮点击后也打开
    /// 同一路径——这样标签和操作指向的是同一个目录。
    pub data_dir: String,
    pub ever_installed: bool,
}

/// 壳的元数据根目录，按以下优先级解析：
///
/// 1. `DSH_DESKTOP_DATA_DIR`——完整覆盖。允许高级用户把壳指向任意目录
///    （例如在外置磁盘上测试），同时短路掉下文的 dsh-home 与构建类型
///    子目录逻辑。
/// 2. `<dsh_home>/<SHELL_SUBDIR>/`，其中 `<dsh_home>` 来自 `DSH_HOME`
///    或 `~/.dsh`，`<SHELL_SUBDIR>` 在 release 构建下是 `desktop/`，
///    在 debug 构建（`tauri dev`）下是 `desktop-dev/`。两个名称避免
///    开发运行的壳与已安装的 release 壳在同一台机器上共用
///    settings.json / active.txt / kernel.pid / port。
/// 3. 当 dsh home 只读时，回退到 Tauri 的操作系统 app-data 目录；
///    宁愿在某个地方启动也不愿在启动阶段直接失败。
///
/// 壳的所有状态（内核、设置、日志、活动指针）都存放在这一根目录中，
/// 与内核自身的数据并列。
pub fn data_dir(app: &tauri::AppHandle) -> PathBuf {
    if let Some(override_dir) = std::env::var_os("DSH_DESKTOP_DATA_DIR").map(PathBuf::from) {
        let _ = fs::create_dir_all(&override_dir);
        return override_dir;
    }
    let home = std::env::var_os("DSH_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs_home().join(DSH_HOME_DIR_NAME));
    let dir = home.join(SHELL_SUBDIR);
    if fs::create_dir_all(&dir).is_ok() {
        return dir;
    }
    // dsh home 只读：回退到 OS 的 app-data 目录，使壳至少能启动，
    // 而不是启动阶段直接失败。
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
}

/// 用户的操作系统 home 目录（Unix 下为 `$HOME`，Windows 下为 `%USERPROFILE%`）。
/// 与 `node.rs` 共用，由其在 home 下定位 nvm 管理的 Node 安装。
pub(crate) fn dirs_home() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// 把 `path` 渲染为展示形式：把 home 前缀缩短为 `~`。当路径不在用户
/// 操作系统 home 下时（例如 `DSH_HOME` 被重定向到自定义位置）回退为
/// 完整字符串；这种情况下用户已经知道路径非标准，需要原样查看。
/// Windows 上的反斜杠会规范化为正斜杠，使展示形式与文档中
/// `~/.dsh/...` 的写法一致。
fn display_short(path: &Path) -> String {
    let home = dirs_home();
    let display = if let Ok(rel) = path.strip_prefix(&home) {
        let mut out = String::from("~");
        if !rel.as_os_str().is_empty() {
            out.push('/');
            out.push_str(
                &rel.to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "/"),
            );
        }
        out
    } else {
        path.display().to_string()
    };
    // 较长的 data-dir 路径（自定义 DSH_HOME、深层 app-data 回退）会撑爆
    // 概览卡片里的 kv 值列。省略路径的中间部分，保留可读的头部
    // （`~` 前缀及其后两个段）与尾部（最末 2~3 个段，这些承载目录的标识）。
    // 省略号让值仍带线索，无需展示完整字符串。
    ellipsize_middle(&display, 38)
}

/// 把 `s` 中间折叠为 `…`（当长度超过 `max_chars` 时），保留头部
/// （约前 40% 的预算）与尾部（其余）。优先以完整路径段作为切点，
/// 让结果读起来仍是真实路径，而非被截断的字符串。
fn ellipsize_middle(s: &str, max_chars: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max_chars {
        return s.to_string();
    }
    let head_budget = max_chars * 2 / 5;
    let tail_budget = max_chars - head_budget;
    // 优先切在 '/' 边界处，避免任一侧停在段中间。
    // 在 head 预算范围内倒序查找最后一个 '/'，在 tail 预算范围内
    // 正序查找第一个 '/'。
    let head_cut = (0..head_budget).rev().find(|&i| chars[i] == '/');
    let tail_start = chars.len() - tail_budget;
    let tail_cut = (tail_start..chars.len()).find(|&i| chars[i] == '/');
    let head_end = match head_cut {
        // 把 '/' 留在头部一侧（路径段读起来完整）。
        Some(i) => i + 1,
        None => head_budget,
    };
    let tail_begin = match tail_cut {
        Some(i) => i,
        None => tail_start,
    };
    let mut out: String = chars[..head_end].iter().collect();
    out.push('…');
    out.extend(chars[tail_begin..].iter());
    out
}

pub fn kernels_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("kernels")
}

pub fn kernel_dir(data_dir: &Path, version: &str) -> PathBuf {
    kernels_dir(data_dir).join(version)
}

/// 定位内核实际解析到的前端 dist。pnpm 的 hoisted 布局可能直接暴露
/// 包目录，也可能把它放在 `.pnpm` 的带 peer 后缀目录中；两种布局都
/// 由安装器合法地产生。
fn frontend_dist_dir(kernel_root: &Path, version: &str) -> Option<PathBuf> {
    let direct = kernel_root
        .join("node_modules")
        .join("@deepseek-ai")
        .join(WEB_FRONTEND_PACKAGE.rsplit('/').next()?)
        .join("dist");
    if direct.is_dir() {
        return Some(direct);
    }

    let package_version = version.strip_prefix('v').unwrap_or(version);
    let prefix = format!("@deepseek-ai+dsh-web-frontend@{package_version}");
    let pnpm_root = kernel_root.join("node_modules").join(".pnpm");
    let entries = fs::read_dir(pnpm_root).ok()?;
    entries.flatten().find_map(|entry| {
        let name = entry.file_name();
        let name = name.to_str()?;
        if !name.starts_with(&prefix) {
            return None;
        }
        let dist = entry
            .path()
            .join("node_modules")
            .join("@deepseek-ai")
            .join("dsh-web-frontend")
            .join("dist");
        dist.is_dir().then_some(dist)
    })
}

/// 从 JS 文件末尾读取 source map 指令。发布包中的指令是普通的
/// `//# sourceMappingURL=...` 注释；只接受 `.map` 文件，跳过 inline map
/// 和其它 URL，避免把任意脚本文本转成文件路径。
fn source_map_reference(source: &str) -> Option<&str> {
    source.lines().rev().find_map(|line| {
        let value = line.split_once("sourceMappingURL=")?.1.trim();
        let value = value.strip_suffix("*/").map(str::trim).unwrap_or(value);
        let end = value.find(['?', '#']).unwrap_or(value.len());
        let value = &value[..end];
        (value.ends_with(".map") && !value.starts_with("data:")).then_some(value)
    })
}

/// 把相对 source map URL 解析到 `root` 内；绝对 URL、协议 URL 和越界
/// 的 `..` 引用不应触发壳对任意路径的写入。
fn source_map_path(script: &Path, reference: &str, root: &Path) -> Option<PathBuf> {
    if reference.is_empty() || reference.contains("://") {
        return None;
    }
    let mut path = script.parent()?.to_path_buf();
    for component in Path::new(reference).components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => path.push(part),
            Component::ParentDir => {
                if !path.pop() {
                    return None;
                }
            }
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    (path.starts_with(root) && path != root).then_some(path)
}

fn collect_javascript_files(dir: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_javascript_files(&entry.path(), files)?;
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        let is_javascript = matches!(
            entry.path().extension().and_then(|value| value.to_str()),
            Some("js" | "mjs" | "cjs")
        );
        if is_javascript {
            files.push(entry.path());
        }
    }
    Ok(())
}

/// 为前端 dist 中已有 source map 声明而缺失的目标创建最小 v3 map。
/// 返回本次新建的文件数；已有文件（包括符号链接）保持不变。
fn materialize_missing_source_maps(dist_root: &Path) -> io::Result<usize> {
    let root = fs::canonicalize(dist_root)?;
    let mut scripts = Vec::new();
    collect_javascript_files(&root, &mut scripts)?;
    let mut created = 0;
    for script in scripts {
        let source = match fs::read_to_string(&script) {
            Ok(source) => source,
            Err(error) if error.kind() == io::ErrorKind::InvalidData => continue,
            Err(error) => return Err(error),
        };
        let Some(reference) = source_map_reference(&source) else {
            continue;
        };
        let Some(map_path) = source_map_path(&script, reference, &root) else {
            continue;
        };
        // symlink_metadata 也能识别 dangling symlink；壳不触碰包目录中
        // 已存在的任何条目。
        if fs::symlink_metadata(&map_path).is_ok() {
            continue;
        }
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&map_path)
        {
            Ok(mut map) => {
                if let Err(error) = map.write_all(EMPTY_SOURCE_MAP.as_bytes()) {
                    let _ = fs::remove_file(&map_path);
                    return Err(error);
                }
                created += 1;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Ok(created)
}

/// 在工作台窗口创建前准备缺失的前端 source map。该操作是 best-effort：
/// source map 仅用于 DevTools 调试，包目录只读时不应阻断工作台本身启动。
pub(crate) fn prepare_workbench_source_maps(data_dir: &Path) {
    let Some(version) = read_active(data_dir) else {
        return;
    };
    let root = kernel_dir(data_dir, &version);
    let Some(dist) = frontend_dist_dir(&root, &version) else {
        return;
    };
    if let Err(error) = materialize_missing_source_maps(&dist) {
        eprintln!(
            "dsh-xlink: unable to prepare frontend source maps in {}: {error}",
            dist.display()
        );
    }
}

pub fn active_file(data_dir: &Path) -> PathBuf {
    data_dir.join("active.txt")
}

pub fn logs_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("logs")
}

/// 读取看起来像已安装内核版本的目录名。
pub fn list_installed(data_dir: &Path) -> Vec<InstalledVersion> {
    let dir = kernels_dir(data_dir);
    let mut out = Vec::new();
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            if !entry.metadata().map(|m| m.is_dir()).unwrap_or(false) {
                continue;
            }
            let size = fs::metadata(kernel_dir(data_dir, &name).join(KERNEL_BIN_REL))
                .ok()
                .map(|m| m.len())
                .unwrap_or(0);
            out.push(InstalledVersion {
                version: name,
                active: false,
                size_bytes: size,
            });
        }
    }
    out
}

/// 当前激活的版本（若有）。
pub fn read_active(data_dir: &Path) -> Option<String> {
    fs::read_to_string(active_file(data_dir))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// 记录当前激活的版本。以纯文本形式持久化，便于 CLI 工具与 app 双方都能
/// 简单地检查格式。
///
/// 写入采用 temp 文件 + rename 的方式，避免写入中途崩溃留下被截断的
/// `active.txt`——读取方把空文件当作「没有激活版本」，这会静默地解除
/// 内核固定。
pub fn write_active(data_dir: &Path, version: Option<&str>) -> Result<(), AppError> {
    fs::create_dir_all(data_dir).map_err(|e| AppError::Io(e.to_string()))?;
    let target = active_file(data_dir);
    match version {
        Some(v) => {
            let tmp = data_dir.join("active.txt.tmp");
            fs::write(&tmp, format!("{v}\n")).map_err(|e| AppError::Io(e.to_string()))?;
            fs::rename(&tmp, &target).map_err(|e| AppError::Io(e.to_string()))
        }
        None => match fs::remove_file(&target) {
            Ok(()) => Ok(()),
            // 删除一个不存在的文件已达到请求状态；卸载路径在部分清理时
            // 依赖此行为。
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(AppError::Io(e.to_string())),
        },
    }
}

/// 刷新每个已安装版本上的 `active` 标记。
pub fn with_active(installed: &mut [InstalledVersion], active: Option<&str>) {
    for item in installed.iter_mut() {
        item.active = Some(item.version.as_str()) == active;
    }
}

/// 组装完整的状态快照。
pub fn status(data_dir: &Path, settings: &Settings) -> KernelStatus {
    let mut installed = list_installed(data_dir);
    let active = read_active(data_dir);
    with_active(&mut installed, active.as_deref());
    let active_installed = active
        .as_ref()
        .map(|v| kernel_dir(data_dir, v).join(KERNEL_BIN_REL).is_file())
        .unwrap_or(false);
    KernelStatus {
        ever_installed: !installed.is_empty(),
        installed,
        active,
        active_installed,
        running: port_open(settings.port),
        port: settings.port,
        data_dir: display_short(data_dir),
    }
}

/// 检查工作台是否已经停止。活动版本切换会改变下一次启动使用的内核；
/// 工作台启动或运行期间必须先停止，避免当前服务与 active 指针指向不同版本。
fn ensure_workbench_stopped(data_dir: &Path) -> Result<(), AppError> {
    let port = settings::load(data_dir).port;
    if port_open(port) {
        return Err(AppError::Kernel(format!(
            "工作台正在启动或运行（端口 {port}），请先点击「关闭工作台」停止工作台后再切换内核"
        )));
    }
    Ok(())
}

/// 切换 `start` 将运行的已安装版本。只有工作台已停止时才能切换，避免
/// 运行中的服务与 `active.txt` 指向不同版本。
pub fn set_active(data_dir: &Path, version: &str) -> Result<(), AppError> {
    ensure_workbench_stopped(data_dir)?;
    if !kernel_dir(data_dir, version).join(KERNEL_BIN_REL).is_file() {
        return Err(AppError::Kernel(format!(
            "版本 {version} 未安装或安装不完整"
        )));
    }
    write_active(data_dir, Some(version))
}

/// 删除一个已安装的版本。若该版本是当前激活版本，调用方需先停止内核。
pub fn uninstall(data_dir: &Path, version: &str) -> Result<(), AppError> {
    if read_active(data_dir).as_deref() == Some(version) {
        return Err(AppError::Kernel(format!(
            "正在使用版本 {version}，请先停止并切换到其他版本"
        )));
    }
    let dir = kernel_dir(data_dir, version);
    if !dir.exists() {
        return Err(AppError::Kernel(format!("版本 {version} 未安装")));
    }
    fs::remove_dir_all(&dir).map_err(|e| AppError::Io(e.to_string()))
}

/// 仅保留最新的 `KEEP` 条内核安装日志（按修改时间）以及即将写入的那一条，
/// 防止长期使用让日志目录无限膨胀。在新命名规则下，每个文件是
/// `<kind>-install-<version>-<date>.log`；过滤器同时接受这种格式与
/// 旧式的 `install-*.log` 名称，让从老版本壳升级上来的用户首次使用时就
/// 把旧安装日志清理掉。
///
/// Pnpm 自动安装日志（`<kind>-pnpm-install-<epoch>-<date>.log`）被排除：
/// 它们不是安装脚本，且 `epoch` 已经让它们在每个会话中唯一，可通过
/// 每日轮转自行清理。
///
/// Best-effort：单条删除失败会被忽略。
fn rotate_install_logs(logs: &Path, keep: &Path) {
    const KEEP: usize = 9;
    let Ok(entries) = fs::read_dir(logs) else {
        return;
    };
    let mut logs: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .filter(|p| {
            let Some(name) = p.file_name().and_then(|n| n.to_str()) else {
                return false;
            };
            // 新命名：<kind>-install-<version>-<date>.log
            // 旧命名：install-<version>.log
            // 两者均含子串 `install-`，排除 pnpm 自动安装。
            let has_install = name.contains("install-");
            let is_pnpm_auto = name.contains("pnpm-install-");
            has_install && !is_pnpm_auto
        })
        .filter(|p| p != keep)
        .filter_map(|p| {
            let modified = p.metadata().ok()?.modified().ok()?;
            Some((modified, p))
        })
        .collect();
    if logs.len() < KEEP {
        return;
    }
    logs.sort_by_key(|a| std::cmp::Reverse(a.0)); // 最新在前
    for (_, path) in logs.iter().skip(KEEP - 1) {
        let _ = fs::remove_file(path);
    }
}

/// 让 pnpm 把 `@deepseek-ai/dsh@<version>` 安装到对应目录。
///
/// `node_dir` 是已校验 `node` 可执行文件所在的目录，并被前置到子进程的 PATH，
/// 这样 pnpm 的 `#!/usr/bin/env node` shebang（以及任何 shell out 调用 `node`
/// 的生命周期脚本）都能解析到它。若没有这一 stamp，通过 launchd-only PATH
/// （macOS .app bundle）或仅有系统 PATH 的 Windows PATH 启动的 GUI 进程，
/// 即便父进程能定位到二进制来拉起 pnpm，也会以 `env: node: No such file or
/// directory` 退出。nvm 管理的 Node 尤为常见——`node` 位于
/// `~/.nvm/versions/node/<v>/bin`，根本不在继承的 PATH 中。
///
/// `on_progress` 会收到人类可读的阶段消息以及每一条原始安装日志行，
/// 让 UI 在安装运行期间可以实时展示输出。
pub fn install_version(
    data_dir: &Path,
    node_dir: &Path,
    pnpm_exe: &Path,
    version: &str,
    mut on_progress: impl FnMut(&str),
) -> Result<(), AppError> {
    let dir = kernel_dir(data_dir, version);
    fs::create_dir_all(&dir).map_err(|e| AppError::Io(e.to_string()))?;
    let stub = dir.join("package.json");
    let stub_text = format!(
        "{{\"name\":\"dsh-kernel-{}\",\"private\":true,\"version\":\"1.0.0\"}}\n",
        version.replace('.', "_")
    );
    fs::write(&stub, stub_text).map_err(|e| AppError::Io(e.to_string()))?;

    // 新命名规则下按日轮转的安装日志：实时脚本写入
    // `<kind>-install-<version>-<date>.log`。用户在日志弹窗中打开的就是
    // 当天的路径；跨日轮转防止长时间重试把单个文件撑满。这里保留旧的
    // `install-<version>.log` 轮转调用，作为一次性清理——把重命名前残留的
    // 旧文件扫掉，因为它们的旧路径已经无法通过 `list_log_files` 触达。
    let logs_root = logs_dir(data_dir);
    let log_spec = install_log_spec(version);
    let log_path = log_spec.path_for(&logs_root, &crate::process::current_date_string());
    rotate_install_logs(&logs_root, &log_path);

    on_progress("正在通过 pnpm 安装内核（首次通常需要 1~3 分钟，下方为实时日志）");
    let spec = format!("@deepseek-ai/dsh@{version}");
    let prefix = dir.to_str().unwrap_or_default();
    // `--ignore-workspace` 让安装脱离用户环境可能暴露的任何 workspace；
    // 内核目录是独立的 package 根目录。
    let args = [
        "add",
        "--prefix",
        prefix,
        "--ignore-workspace",
        "--config.node-linker=hoisted",
        PNPM_REPORTER,
        spec.as_str(),
    ];
    // `node_dir` 排在最前，使任何 shebang 或生命周期子进程看到的都是
    // 父进程使用的同一个 node，即便 pnpm 本身位于别处（例如设置里
    // 固定的 `pnpm` shim）。参见上文的 doc 注释。
    let pnpm_dir = pnpm_exe.parent().unwrap_or(Path::new("."));
    let status = run_pnpm(
        pnpm_exe,
        &args,
        &dir,
        &logs_root,
        &log_spec,
        &[node_dir, pnpm_dir],
        &mut on_progress,
    )
    .map_err(|e| {
        AppError::Kernel(format!(
            "无法运行 pnpm（{e}）。请确认已安装 Node.js 与 pnpm，详情见日志：{}",
            log_path.display()
        ))
    })?;
    on_progress("pnpm 已退出，正在校验安装结果");

    // pnpm ≥ 10 在存在被忽略的构建脚本（见 `pnpm approve-builds`）时会打印
    // `[ERR_PNPM_IGNORED_BUILDS]` 并以非零退出码结束，尽管安装产物已经就绪，
    // 退出码因此不能作为安装成功判据。以内核入口文件是否就位为准：
    // 退出码非零且产物缺失 → 失败；退出码非零且产物完整 → 降级为警告。
    let exit_code = status
        .code()
        .map(|c| c.to_string())
        .unwrap_or_else(|| "? (信号)".into());
    let bin_ready = dir.join(KERNEL_BIN_REL).is_file();
    if !status.success() && !bin_ready {
        return Err(AppError::Kernel(format!(
            "pnpm 安装失败（退出码 {exit_code}），请检查网络或 pnpm 配置后重试，详情见日志：{}",
            log_path.display()
        )));
    }
    if !bin_ready {
        return Err(AppError::Kernel(format!(
            "安装未产生预期的内核入口（{KERNEL_BIN_REL}），请查看日志：{}",
            log_path.display()
        )));
    }
    if !status.success() {
        on_progress(&format!(
            "注意：pnpm 以退出码 {exit_code} 结束（多为依赖构建脚本被忽略所致，可以在该内核目录运行 pnpm approve-builds 允许），内核文件已安装完成"
        ));
    }
    Ok(())
}

/// `--reporter=append-only`：pnpm 把每个生命周期事件以一行日志输出到 stdout，
/// 由 `run_with_progress` 流式转发到 UI 和日志文件。
pub(crate) const PNPM_REPORTER: &str = "--reporter=append-only";

/// `--config.strict-dep-builds=false`：pnpm 11+ 默认不再静默跳过
/// 间接依赖的构建脚本，会把 `ERR_PNPM_IGNORED_BUILDS` 转化为非零退出码，
/// 即便生成的依赖树本身没问题（插件经常会拉入类似 `node-pty` 这类依赖，
/// 其原生编译壳根本不需要）。传入该选项的调用方会自行校验产物
/// （内核入口、`node_modules`），而非仅依赖退出码。
pub(crate) const PNPM_NO_STRICT_DEP_BUILDS: &str = "--config.strict-dep-builds=false";

/// 内核日志文件的逻辑名（不含构建类型前缀和日期戳）。完整的文件名在
/// 写入时按 `<kind>-KERNEL_LOG_NAME-<date>.log` 拼装，从而在本地
/// 午夜自动滚动到新文件。
pub const KERNEL_LOG_NAME: &str = "kernel";

/// 为运行中的内核构造按日轮转的日志 spec。进程内的每个轮转槽位
/// （start、run_pnpm、ensure_pnpm）都使用同一 spec，这样在某个标签页
/// tail 时始终跟踪同一个内核会话。
pub fn kernel_log_spec() -> LogSpec {
    LogSpec::new(build_log_kind(), KERNEL_LOG_NAME)
}

/// 为内核安装构造按日轮转的日志 spec。版本嵌入逻辑名中，因此同一版本的
/// 多次安装尝试会落到同一个每日文件里（重试之间以追加方式累积）。
pub fn install_log_spec(version: &str) -> LogSpec {
    LogSpec::new(build_log_kind(), format!("install-{version}"))
}

/// 便捷函数：获取给定日志目录下当天的内核日志路径。由读取路径
/// （`get_kernel_log`、guard attribution）使用，它们总是需要最近一天的日志。
pub fn current_kernel_log_path(data_dir: &Path) -> PathBuf {
    let logs = logs_dir(data_dir);
    let today = crate::process::current_date_string();
    kernel_log_spec().path_for(&logs, &today)
}

/// 用给定参数启动 pnpm 一次，将合并的 stdout+stderr 按行同时管道到
/// 滚动日志和 `on_progress`。这是共享助手 `run_with_progress` 的轻量包装，
/// 后者已经处理 Windows 上 `.cmd` 路由、双流 drain 以及静默期心跳——这些
/// 正是 pnpm 安装需要透传到 UI 的能力。`extra_path_dirs` 透传进去，使
/// 子进程能在其 PATH 上找到已校验的 `node`——原因参见
/// `process::run_with_progress` 中关于 macOS 启动的 `.app` bundle 上
/// pnpm spawn 环境为空的说明。
pub(crate) fn run_pnpm(
    pnpm_exe: &Path,
    args: &[&str],
    cwd: &Path,
    logs_dir: &Path,
    log_spec: &LogSpec,
    extra_path_dirs: &[&Path],
    on_progress: impl FnMut(&str),
) -> io::Result<std::process::ExitStatus> {
    run_with_progress(
        pnpm_exe,
        args,
        cwd,
        logs_dir,
        log_spec,
        extra_path_dirs,
        on_progress,
    )
}

/// `run_pnpm` 的路径固定版本。用于一次性脚本（例如插件自身目录下的
/// per-plugin 构建日志等），由调用方完全拥有；输出原样写入 `log_path`，
/// 不打构建类型戳，也不按日轮转。基于大小的轮转仍然生效，防止失控的
/// 构建超出磁盘配额。
pub(crate) fn run_pnpm_at(
    pnpm_exe: &Path,
    args: &[&str],
    cwd: &Path,
    log_path: &Path,
    extra_path_dirs: &[&Path],
    on_progress: impl FnMut(&str),
) -> io::Result<std::process::ExitStatus> {
    run_with_progress_at(pnpm_exe, args, cwd, log_path, extra_path_dirs, on_progress)
}

/// 检查 `127.0.0.1:port` 上是否已有进程在监听。
pub fn port_open(port: u16) -> bool {
    use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
    let addr: SocketAddr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), port));
    TcpStream::connect_timeout(&addr, Duration::from_millis(400)).is_ok()
}

/// 为当前激活版本启动 `dsh web --no-open`，输出重定向到内核日志。
/// 在 Unix 上，子进程会被放到独立的进程组中，停止时即可回收整个组。
pub fn start(data_dir: &Path, node: &Path, version: &str, port: u16) -> Result<Child, AppError> {
    let dir = kernel_dir(data_dir, version);
    let bin = dir.join(KERNEL_BIN_REL);
    if !bin.is_file() {
        return Err(AppError::Kernel(format!(
            "版本 {version} 未安装或安装不完整"
        )));
    }
    if port_open(port) {
        return Err(AppError::Kernel(format!(
            "端口 {port} 已被占用，可能已有内核在运行"
        )));
    }
    let mut cmd = crate::process::command_with_path(node);
    let port_arg: String = port.to_string();
    cmd.arg(&bin)
        .arg("web")
        .arg("--no-open")
        .arg("--port")
        .arg(port_arg)
        .current_dir(data_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(|| {
                // 进入新会话，使 `kill -pid` 能回收整个进程组。
                if libc::setsid() == -1 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }

    // quiet() 在这里同样关键：内核是一个长时间运行的 console 应用，
    // 否则它会在整个生命周期内一直占用一个可见的终端窗口。
    let mut child = quiet(&mut cmd)
        .spawn()
        .map_err(|e| AppError::Io(format!("无法启动内核：{e}")))?;
    if let Err(error) = attach_log_drainers(&mut child, &logs_dir(data_dir), &kernel_log_spec()) {
        crate::process::terminate_process_tree(&mut child);
        return Err(AppError::Io(format!("无法接管内核日志：{error}")));
    }
    Ok(child)
}

/// 除非端口已被占用，否则启动当前激活的内核。
///
/// 当端口已有响应时返回 `Ok(None)`（幂等的启动），本调用真正拉起
/// 进程时返回 `Ok(Some(child))`。
pub fn start_maybe(data_dir: &Path, node: &Path) -> Result<Option<Child>, AppError> {
    let s = settings::load(data_dir);
    let port = s.port;
    if port_open(port) {
        return Ok(None);
    }
    let active = read_active(data_dir).ok_or_else(|| {
        AppError::Kernel("尚未选择内核版本，请先在“更新”页安装并切换到某一版本".into())
    })?;
    start(data_dir, node, &active, port).map(Some)
}

/// 回收工作目录等于 `data_dir` 的孤儿 dsh web 内核。
///
/// 壳发生崩溃，或壳窗口未走「关闭工作台」就被杀掉，都会把内核子进程留在
/// 身后（setsid 已经把它从壳的进程组里分离出去）。下一次壳启动会发现
/// 端口已被占用，`start_maybe` 报告「已在运行」——但那个孤儿会像第二个实例
/// 那样继续向同一项目目录写会话日志，这正是历史上出现 `corrupt session
/// log: seq gap in committed region` 失败的根因。回收流程：扫描所有
/// `@deepseek-ai/dsh/bin.js web` 进程，把它们的 cwd 与 `data_dir` 比较，
/// 对匹配且不是当前壳内存中子进程的项做 SIGTERM+SIGKILL（通过 kill_pid，
/// 它带有相同的 pid-is-kernel 守卫）。
///
/// 对同一 data dir 上故意运行的第二个壳实例安全吗？并不完全安全：使用同一
/// data dir 的第二个壳也会以 cwd == data_dir 运行其内核，所以本次扫描也会
/// 把那个内核回收掉。但这正是期望的结果——桌面壳在每个 data dir 上是
/// 单实例的（dev / release 划分让每个构建拥有自己的目录），而同一目录上
/// 两个内核恰恰是本函数存在的目的所要防止的损坏场景。
/// 在 windows_subsystem 下，Windows 端仅保留接口、没有 orphan-reap 实现
/// （见函数体注释）。参数在 Unix 分支里被 `data_dir == cwd` 比较使用，
/// Windows 编译时整个 #[cfg(unix)] 块被跳过，所以该参数属于平台特定的未使用项。
#[cfg_attr(not(unix), allow(unused_variables))]
pub fn reap_orphans(data_dir: &Path) {
    #[cfg(unix)]
    {
        let port = crate::settings::load(data_dir).port;
        let (success, text, _) =
            match crate::process::run_capture_output("ps", &["-eo", "pid,command"]) {
                Ok(output) => output,
                Err(_) => return,
            };
        if !success {
            return;
        }
        // 每个候选项都可能触发一次有界的 lsof/ps 探测，因此不能让
        // 启动期清理与不可信的进程列表规模成正比。
        let mut candidates = 0usize;
        for line in text.lines() {
            if candidates >= MAX_ORPHAN_CANDIDATES {
                break;
            }
            if !line.contains("@deepseek-ai/dsh/lib/bin.js") || !line.contains(" web ") {
                continue;
            }
            candidates += 1;
            let pid: u32 = match line.split_whitespace().next().and_then(|p| p.parse().ok()) {
                Some(p) => p,
                None => continue,
            };
            if pid == std::process::id() {
                continue;
            }
            // 解析进程的 cwd：Linux 上读 /proc/{pid}/cwd，macOS 上用 lsof。
            // 只有 cwd 与 OUR data dir 匹配的实体才是我们要回收的。
            let cwd_matches = std::fs::read_link(format!("/proc/{pid}/cwd"))
                .map(|p| p == data_dir)
                .unwrap_or_else(|_| {
                    let pid_arg = pid.to_string();
                    crate::process::run_capture_output(
                        "lsof",
                        &["-a", "-p", &pid_arg, "-d", "cwd", "-Fn"],
                    )
                    .ok()
                    .and_then(|(success, stdout, _)| {
                        success.then(|| {
                            stdout
                                .lines()
                                .find(|l| l.starts_with('n'))
                                .map(|l| std::path::PathBuf::from(&l[1..]))
                        })
                    })
                    .flatten()
                    .map(|p| p == data_dir)
                    .unwrap_or(false)
                });
            if cwd_matches {
                kill_pid(pid, Some(port));
            }
        }
    }
    #[cfg(windows)]
    {
        // Windows 上没有 /proc；新版 PowerShell 已经禁用 wmic/wmic；
        // stop_kernel 中的端口回退路径能覆盖常见情形，而 PowerShell 的
        // Get-CimInstance 每次启动都跑太慢。Windows 上暂时保持 no-op——
        // 那里的孤儿回收是后续任务，pid 文件 / 端口回退仍然允许用户停止。
    }
}

/// 停止正在运行的内核子进程，在支持的平台上回收整个进程组。
pub fn stop(child: &mut Child) -> Result<(), AppError> {
    #[cfg(unix)]
    {
        let pid = child.id() as i32;
        // 先请求整个组终止，再强制 kill 任何仍然存活的进程。
        unsafe {
            libc::kill(-pid, libc::SIGTERM);
        }
        // 用 try_wait 轮询而非阻塞 wait()：忽略 SIGTERM 的子进程会
        // 永远阻塞 stop()，导致后面的 SIGKILL 无法执行。
        // 与 `kill_pid` 同样的 1 秒预算。
        let mut exited = false;
        for _ in 0..10 {
            // try_wait 仅在 OS 级错误时失败；继续轮询，无论如何
            // 让后面的 SIGKILL 把子进程收尾。
            if child.try_wait().is_ok_and(|status| status.is_some()) {
                exited = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        if !exited {
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
            }
            // kill 后再回收；这里的 wait 报错意味着子进程已经消失，
            // 而这正是我们想要的状态。
            let _ = child.wait();
        }
    }
    #[cfg(windows)]
    {
        let pid = child.id().to_string();
        let mut cmd = crate::process::command_with_path("taskkill");
        cmd.args(["/PID", &pid, "/T", "/F"]);
        let _ = quiet(&mut cmd).status();
        let _ = child.wait();
    }
    Ok(())
}

// --- pid 跟踪 ---------------------------------------------------------------
//
// 壳自身的内存中 `running` 子进程在壳重启时会丢失。pid 文件让稍后的
// 「停止内核」操作仍然能够回收内核。

// --- 基于端口的 pid 查询 -----------------------------------------------------
//
// 当 dev 与 release 壳并列运行时（参见 `data_dir` + `DEFAULT_PORT` 中的
// data dir 隔离），release 壳的 `kernel.pid` 与 dev 壳无关，反之亦然。
// dev 壳还可能在它启动的内核仍在运行时被重启——此时内存中 `state.running`
// 句柄已经消失，`start_maybe` 调用因为端口已被占用而跳过启动，因此 dev 壳
// 永远不会写出自己的 pid 文件。此时 Stop 没有可 kill 的 pid；内核继续存活，
// UI 把端口读作「运行中」。通过监听端口反查 pid 可以恢复这一场景下的
// pid——dev 壳、release 壳，以及任何想要接管一个非自己启动的内核的后续
// 壳，都可以用这种方式回收运行中的进程。

/// 返回当前正在监听 `127.0.0.1:port` 的进程 pid；若端口空闲或
/// 平台特定的查询失败，返回 `None`。作为内核 pid 文件缺失或指向
/// 陈旧进程时的回退。
#[cfg(unix)]
pub(crate) fn port_listen_pid(port: u16) -> Option<u32> {
    // lsof 最具可移植性：macOS 默认自带，绝大多数 Linux 发行版的
    // base 包中也包含。
    // `-nP` 跳过 DNS 与服务名解析（更快、输出更确定）；
    // `-iTCP:PORT -sTCP:LISTEN -t` 过滤出我们想要的那一个 pid——
    // 首行就是监听者的 pid。
    if let Some(pid) = port_listen_pid_lsof(port) {
        return Some(pid);
    }
    // 没有 lsof 的 Linux 系统回退到 `ss`。
    port_listen_pid_ss(port)
}

#[cfg(unix)]
pub(crate) fn port_listen_pid_lsof(port: u16) -> Option<u32> {
    let port_arg = port.to_string();
    let (success, stdout, _) = crate::process::run_capture_output(
        "lsof",
        &["-nP", "-iTCP", &port_arg, "-sTCP:LISTEN", "-t"],
    )
    .ok()?;
    if !success {
        return None;
    }
    stdout.lines().next().and_then(|s| s.trim().parse().ok())
}

#[cfg(unix)]
pub(crate) fn port_listen_pid_ss(port: u16) -> Option<u32> {
    let filter = format!("sport = :{port}");
    let (success, stdout, _) =
        crate::process::run_capture_output("ss", &["-lntp", &filter]).ok()?;
    if !success {
        return None;
    }
    // ss 行格式：
    //   LISTEN 0 128  127.0.0.1:3091  127.0.0.1:*  users:(("node",pid=1762,fd=22))
    // pid 位于 users:(("…",pid=NUMBER,fd=NUMBER)) 元组中；
    // 无需解析周围文本——只需取第一段 "pid=NUMBER"。
    stdout
        .lines()
        .filter_map(|line| {
            let idx = line.find("pid=")?;
            let after = &line[idx + 4..];
            let end = after
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(after.len());
            after[..end].parse().ok()
        })
        .next()
}

#[cfg(windows)]
pub(crate) fn port_listen_pid(port: u16) -> Option<u32> {
    // netstat -ano 每个 TCP/UDP 端点输出一行；用 `:PORT` 与 `LISTENING`
    // 过滤，找到绑定在 dev/release 端口上的 pid。
    let (success, stdout, _) = crate::process::run_capture_output("netstat", &["-ano"]).ok()?;
    if !success {
        return None;
    }
    let port_str = port.to_string();
    let needle = format!(":{}", port_str);
    for line in stdout.lines() {
        if line.contains(&needle) && line.contains("LISTENING") {
            if let Some(pid) = line.split_whitespace().last() {
                if let Ok(pid) = pid.parse() {
                    return Some(pid);
                }
            }
        }
    }
    None
}

/// 上次壳启动的内核的 PID 文件：`<data_dir>/kernel.pid`。
fn pid_path(data_dir: &Path) -> PathBuf {
    data_dir.join("kernel.pid")
}

/// 记录已启动内核的 pid（best-effort）。
pub fn write_pid(data_dir: &Path, pid: u32) {
    let _ = fs::write(pid_path(data_dir), pid.to_string());
}

/// 读取已记录的内核 pid（若存在且可解析）。
pub fn read_pid(data_dir: &Path) -> Option<u32> {
    fs::read_to_string(pid_path(data_dir))
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// 在成功停止后清除 pid 记录。
pub fn clear_pid(data_dir: &Path) {
    let _ = fs::remove_file(pid_path(data_dir));
}

/// 返回某个进程的命令行，避免不受限的助手命令把 stop 路径挂住。
fn process_command(pid: u32) -> Option<String> {
    #[cfg(unix)]
    {
        crate::process::run_capture("ps", &["-p", &pid.to_string(), "-o", "command="])
            .ok()
            .and_then(|(ok, output)| ok.then_some(output))
    }
    #[cfg(windows)]
    {
        let filter =
            format!("(Get-CimInstance Win32_Process -Filter 'ProcessId = {pid}').CommandLine");
        crate::process::run_capture(
            "powershell.exe",
            &["-NoProfile", "-NonInteractive", "-Command", &filter],
        )
        .ok()
        .and_then(|(ok, output)| ok.then_some(output))
    }
}

/// 判断 `pid` 是否是服务于指定端口的 dsh 内核。三层防护：
/// 1. 命令行必须含 `@deepseek-ai/dsh/lib/bin.js`，挡住被复用 pid 的无关进程；
/// 2. 命令行 `--port` 必须等于给定端口，挡住跨 profile（dev 3091 / release 3090）
///    的壳误把对方的内核认作自己；
/// 3. 给定端口时再向 OS 反查一次监听该端口的 pid，必须等于本 pid，挡住 pid 文件
///    陈旧、内核早已退出但 OS 把 pid 复用给另一个进程的情况——单看命令行残留
///    无法区分这一类，端口活体验证是唯一可信的"这还是不是同一个内核"判据。
pub(crate) fn pid_is_kernel(pid: u32, port: Option<u16>) -> bool {
    let Some(command) = process_command(pid) else {
        return false;
    };
    let command = command.to_ascii_lowercase().replace('\\', "/");
    if !command.contains("@deepseek-ai/dsh/lib/bin.js") {
        return false;
    }
    let Some(port) = port else {
        return true;
    };
    let port_str = port.to_string();
    let mut args = command.split_whitespace();
    let mut port_arg_matches = false;
    while let Some(arg) = args.next() {
        let arg = arg.trim_matches('"');
        if arg == "--port" {
            if args
                .next()
                .map(|value| value.trim_matches('"') == port_str)
                .unwrap_or(false)
            {
                port_arg_matches = true;
            }
            break;
        }
        if let Some(value) = arg.strip_prefix("--port=") {
            if value == port_str {
                port_arg_matches = true;
            }
            break;
        }
    }
    if !port_arg_matches {
        return false;
    }
    // 端口活体验证：OS 反查"当前谁在监听该端口"，必须等于本 pid。
    // 查询失败（lsof / ss / netstat 缺失或沙盒阻断）时不要把已
    // 经命令行验证过的内核误判为不可信——让 stop_kernel 的端口反查
    // 兜底接手。
    match port_listen_pid(port) {
        Some(listener_pid) => listener_pid == pid,
        None => true,
    }
}

/// 按 pid 杀掉被追踪出的内核：先给进程组发 TERM，再 KILL 任何幸存者。
/// 当 pid 已不存在或与本壳记录的内核命令及可选端口不匹配时为 no-op。
pub fn kill_pid(pid: u32, port: Option<u16>) {
    if !pid_is_kernel(pid, port) {
        return;
    }
    #[cfg(unix)]
    {
        let pgid = pid as i32; // start() 会调用 setsid()，因此子进程是其进程组的领头
        unsafe {
            libc::kill(-pgid, libc::SIGTERM);
        }
        for _ in 0..10 {
            std::thread::sleep(Duration::from_millis(100));
            let alive = unsafe { libc::kill(-pgid, 0) } == 0;
            if !alive {
                return;
            }
        }
        unsafe {
            libc::kill(-pgid, libc::SIGKILL);
        }
    }
    #[cfg(windows)]
    {
        let mut cmd = crate::process::command_with_path("taskkill");
        cmd.args(["/PID", &pid.to_string(), "/T", "/F"]);
        let _ = quiet(&mut cmd).status();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_empty_maps_only_for_missing_javascript_source_map_references() {
        let root = std::env::temp_dir().join(format!(
            "dsh-source-map-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let assets = root.join("assets");
        fs::create_dir_all(&assets).expect("create assets");
        fs::write(
            assets.join("index.js"),
            "console.log('workbench');\n//# sourceMappingURL=index.js.map\n",
        )
        .expect("write missing-map javascript");
        fs::write(
            assets.join("already.js"),
            "//# sourceMappingURL=already.js.map\n",
        )
        .expect("write existing-map javascript");
        fs::write(assets.join("already.js.map"), b"original map").expect("write existing map");
        fs::write(
            assets.join("inline.js"),
            "//# sourceMappingURL=data:application/json;base64,AAAA\n",
        )
        .expect("write inline-map javascript");
        fs::write(
            assets.join("escape.js"),
            "//# sourceMappingURL=../../outside.js.map\n",
        )
        .expect("write outside-map javascript");

        let created = materialize_missing_source_maps(&root).expect("materialize maps");

        assert_eq!(created, 1);
        assert_eq!(
            fs::read_to_string(assets.join("index.js.map")).expect("read generated map"),
            EMPTY_SOURCE_MAP
        );
        assert_eq!(
            fs::read_to_string(assets.join("already.js.map")).expect("read existing map"),
            "original map"
        );
        assert!(!root
            .parent()
            .expect("temp parent")
            .join("outside.js.map")
            .exists());
        fs::remove_dir_all(&root).expect("remove test files");
    }

    #[test]
    fn finds_frontend_dist_in_pnpm_layout_and_strips_version_prefix() {
        let root = std::env::temp_dir().join(format!(
            "dsh-frontend-dist-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let dist = root
            .join("node_modules")
            .join(".pnpm")
            .join("@deepseek-ai+dsh-web-frontend@0.1.2-alpha.1_peerhash")
            .join("node_modules")
            .join("@deepseek-ai")
            .join("dsh-web-frontend")
            .join("dist");
        fs::create_dir_all(&dist).expect("create pnpm dist");

        assert_eq!(
            frontend_dist_dir(&root, "v0.1.2-alpha.1"),
            Some(dist.clone())
        );
        fs::remove_dir_all(&root).expect("remove test files");
    }

    #[test]
    fn refuses_to_change_active_version_while_workbench_is_serving() {
        let root = std::env::temp_dir().join(format!(
            "dsh-active-version-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind workbench port");
        let port = listener.local_addr().expect("workbench address").port();
        let mut settings = Settings::default();
        settings.port = port;
        settings::save(&root, &settings).expect("save test settings");

        let error = set_active(&root, "0.1.2").expect_err("running workbench must block switch");

        assert!(error
            .to_string()
            .contains("请先点击「关闭工作台」停止工作台后再切换内核"));
        drop(listener);
        fs::remove_dir_all(&root).expect("remove test data");
    }

    /// `display_short` 是 UI 显示在「打开」按钮旁的文本；
    /// 按钮必须打开与标签同名的目录，否则用户会落到下一级而疑惑
    /// 为何路径对不上。home 前缀的替换在 Windows 上还要在正斜杠 /
    /// 反斜杠边界处保持一致。
    #[test]
    fn display_short_substitutes_home_with_tilde() {
        let home = dirs_home();
        let nested = home.join(".dsh").join("desktop");
        assert_eq!(display_short(&nested), "~/.dsh/desktop");
    }

    #[test]
    fn display_short_falls_back_to_full_path_outside_home() {
        // 自定义 DSH_HOME 目标位于 $HOME 之外；原样展示，
        // 让布局非标准的用户能核对自己的壳数据实际写到哪。
        let outside = PathBuf::from("/custom/redirect/.dsh/desktop");
        assert_eq!(display_short(&outside), outside.display().to_string());
    }

    #[test]
    fn display_short_keeps_tilde_only_when_path_equals_home() {
        // 边界情形：data_dir 解析到 home 本身（没有 `.dsh/desktop`
        // 后缀）。输出仍应为单个 `~`，而不是 `~/`。
        let home = dirs_home();
        assert_eq!(display_short(&home), "~");
    }
}
