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
    atomic_write, attach_log_drainers, build_log_kind, quiet, run_with_progress,
    run_with_progress_at, LogSpec,
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
    /// 设置文件损坏 / 读不出来时的说明。非空时 UI 必须显示它：此时端口已经
    /// 无声回退到默认值，不提示的话用户只会看到"工作台跑到别的端口去了"。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settings_warning: Option<String>,
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
        Some(v) => atomic_write(&target, format!("{v}\n").as_bytes())
            .map_err(|e| AppError::Io(e.to_string())),
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
        installed,
        active,
        active_installed,
        // 不以配置端口判活：内核可能绑在用户改端口之前的那个端口上，
        // 详见 [`workbench_pid`]。
        running: workbench_running(data_dir, settings),
        port: settings.port,
        data_dir: display_short(data_dir),
        // 与 `settings` 参数同样的读取路径，但保留诊断：调用方传进来的
        // settings 可能已经是"回退后的默认值"。
        settings_warning: crate::settings::load_checked(data_dir).1,
    }
}

/// 检查工作台是否已经停止。活动版本切换会改变下一次启动使用的内核；
/// 工作台启动或运行期间必须先停止，避免当前服务与 active 指针指向不同版本。
fn ensure_workbench_stopped(data_dir: &Path) -> Result<(), AppError> {
    let settings = settings::load(data_dir);
    if workbench_running(data_dir, &settings) {
        return Err(AppError::Kernel(format!(
            "工作台正在启动或运行（端口 {}），请先点击「关闭工作台」停止工作台后再切换内核",
            settings.port
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
/// `node_exe` 是已校验的 `node` 可执行文件路径。其**所在目录**被前置到
/// 子进程的 PATH，这样 pnpm 的 `#!/usr/bin/env node` shebang（以及任何
/// shell out 调用 `node` 的生命周期脚本）都能解析到它。若没有这一 stamp，
/// 通过 launchd-only PATH（macOS .app bundle）或仅有系统 PATH 的 Windows
/// PATH 启动的 GUI 进程，即便父进程能定位到二进制来拉起 pnpm，也会以
/// `env: node: No such file or directory` 退出。nvm 管理的 Node 尤为
/// 常见——`node` 位于 `~/.nvm/versions/node/<v>/bin`，根本不在继承的
/// PATH 中。`node_exe` 同时也是安装结束后 `smoke_load_native_modules`
/// 启动 Node 探针的入口——见该函数的 doc 注释。
///
/// `on_progress` 会收到人类可读的阶段消息以及每一条原始安装日志行，
/// 让 UI 在安装运行期间可以实时展示输出。
pub fn install_version(
    data_dir: &Path,
    node_exe: &Path,
    pnpm_exe: &Path,
    version: &str,
    on_progress: impl FnMut(&str),
) -> Result<(), AppError> {
    let dir = kernel_dir(data_dir, version);
    // 全新安装失败时把目录整个删掉。
    //
    // 半成品目录（pnpm 跑到一半、原生模块没编译出来、smoke 探针失败）会被
    // `list_installed` 当成一个**已安装版本**列出来，而且允许用户切换过去；
    // 真正撞上问题是在启动内核时——报"原生模块缺失"，然后被启动看护当成
    // 疑似插件问题处理几分钟，用户完全看不出根因是这个版本压根没装完。
    // 重装（目录本来就存在）时保留残骸，让用户能对比或手动处理。
    let existed_before = dir.exists();
    let outcome = install_version_into(data_dir, node_exe, pnpm_exe, version, on_progress);
    if outcome.is_err() && !existed_before {
        let _ = fs::remove_dir_all(&dir);
    }
    outcome
}

fn install_version_into(
    data_dir: &Path,
    node_exe: &Path,
    pnpm_exe: &Path,
    version: &str,
    mut on_progress: impl FnMut(&str),
) -> Result<(), AppError> {
    let dir = kernel_dir(data_dir, version);
    fs::create_dir_all(&dir).map_err(|e| AppError::Io(e.to_string()))?;
    let stub = dir.join("package.json");
    // 用 serde_json 构造而不是手写 format!：版本号一旦含有引号，手写模板会被
    // 闭合、注入任意字段（pnpm 之后会执行 stub 里的生命周期脚本）。这里由
    // 序列化器负责转义；命令边界的 is_valid_kernel_version 是更早的一道闸。
    let stub_text = format!(
        "{}\n",
        serde_json::json!({
            "name": format!("dsh-kernel-{}", version.replace('.', "_")),
            "private": true,
            "version": "1.0.0",
        })
    );
    atomic_write(&stub, stub_text.as_bytes()).map_err(|e| AppError::Io(e.to_string()))?;

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
    // 同时打开 `PNPM_NO_STRICT_DEP_BUILDS`（不把 ERR_PNPM_IGNORED_BUILDS
    // 视作错误，仅警告）与 `PNPM_ALLOW_ALL_BUILDS`（真正运行原生模块的
    // install / postinstall 脚本，由壳对 @deepseek-ai 命名空间的信任背书）；
    // 二者缺一则 fs-ext / node-pty 等模块只下载 JS 不编译 .node，
    // 内核启动时报 `Cannot find module './build/Release/fs_ext.node'`。
    let args = [
        "add",
        "--prefix",
        prefix,
        "--ignore-workspace",
        "--config.node-linker=hoisted",
        PNPM_NO_STRICT_DEP_BUILDS,
        PNPM_ALLOW_ALL_BUILDS,
        PNPM_REPORTER,
        spec.as_str(),
    ];
    // `node_exe` 的目录排在最前，使任何 shebang 或生命周期子进程看到的都是
    // 父进程使用的同一个 node，即便 pnpm 本身位于别处（例如设置里
    // 固定的 `pnpm` shim）。参见上文的 doc 注释。
    let node_dir = node_exe.parent().unwrap_or_else(|| Path::new("."));
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
            "注意：pnpm 以退出码 {exit_code} 结束（多为依赖构建脚本未完全成功所致），已校验后续步骤"
        ));
    }

    // 真正决定内核是否能跑的是 `*.node` 二进制是否就位：仅凭 `bin.js`
    // 与 pnpm 退出码无法判定 fs-ext / node-pty / koffi 是否真的产出了
    // 原生模块。pnpm 11 的 ignored-builds 机制会让 `.node` 静默缺失，
    // 必须显式遍历 `NATIVE_MODULE_CHECKS` 检查 build 产物；缺则直接失败，
    // 不让工作台后续启动时再撞上「Cannot find module './build/...'」一类的
    // 难以定位的错误。
    if let Err(missing) = verify_native_modules(&dir) {
        return Err(AppError::Kernel(format_native_modules_error(
            &missing, &log_path,
        )));
    }

    // 静态检查通过后再跑一次「真的把原生模块加载起来」的可执行性探针。
    // 文件存在 ≠ 可加载：fs-ext / node-addon-system 等在 Node 启动时
    // 解析不到 `.node` 会以同步异常形式抛出，pnpm 阶段却已经退出 0——
    // 这一步把那条路径在「安装完成」与「首次启动」之间提前引爆，并把
    // 真实报错（NODE_MODULE_VERSION 不匹配、optionalDependencies 没拉
    // 对平台、prebuild 文件缺失等）落到安装日志里，由 UI 立刻显示给
    // 用户，而不是留给 last-incident 的二次归因。详见
    // `smoke_load_native_modules` 的 doc 注释。
    if let Err(reason) =
        smoke_load_native_modules(node_exe, &dir, &logs_root, &log_spec, &mut on_progress)
    {
        // 探针已经把根因写进 `reason`（含它自己捕获的输出），直接放进
        // 文案；只有「怎么处理」这类通用建议需要在这里补。文案里保留日志
        // 路径，让用户能把完整 require 堆栈交给支持。
        return Err(AppError::Kernel(format!(
            "内核依赖的可加载性校验失败：{reason}。若探针输出里出现 `Cannot find module` 或 `NODE_MODULE_VERSION`，说明当前平台的 prebuild 未随 optionalDependencies 下载或与本机 Node 版本不匹配（重试安装或切换到其他内核版本）；若只有退出码而没有探针输出，说明 Node 探针进程本身没能启动，日志与「设置 → 运行时」里显示的 Node 路径可用于定位。完整日志：{}",
            log_path.display()
        )));
    }
    on_progress("内核安装成功");
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

/// `--config.dangerously-allow-all-builds=true`：pnpm 10+ 默认白名单机制下，
/// 不在 `onlyBuiltDependencies` 的包的 `install` / `postinstall` 构建脚本会被
/// 静默跳过。内核依赖链里 `fs-ext`、`node-pty`、`koffi` 等原生模块需要
/// 真正执行 `node-gyp` / `cnoke` 才能产出 `*.node` 二进制，否则运行时
/// 直接报 `Cannot find module './build/Release/fs_ext.node'` 一类错误；
/// 仅仅 `strict-dep-builds=false` 只会让 pnpm 不再报错，**不会**实际运行
/// 这些脚本。这个开关在 `install_version` 内部使用，由壳对 `@deepseek-ai`
/// 命名空间的信任背书——任何被装入内核目录的依赖都已经过上游审查，再叠加
/// `--config.strict-dep-builds=false` 让 pnpm 把退出码降级为可恢复警告，
/// 后续 `verify_native_modules` 步骤会真正决定这次安装是否成功。
pub(crate) const PNPM_ALLOW_ALL_BUILDS: &str = "--config.dangerously-allow-all-builds=true";

/// 一项原生模块的就位检查：包名 + 相对 `node_modules/<pkg>` 的 `.node` 路径。
/// 安装结束后核对每一项是否真的产出了二进制——这是 pnpm 静默跳过构建脚本
/// 后唯一可靠的就位判据（`pnpm` 的退出码不再可信，`node_modules/<pkg>` 的
/// 存在只说明 JS 已就位、不说明原生模块能加载）。
///
/// 注意：该列表同时容纳「老内核的 `fs-ext`」和「新内核的
/// `@deepseek-ai/node-addon-system-*`」两个家族的入口。`verify_native_modules`
/// 在运行时只对**实际安装**的包做校验：内核依赖里没出现的包会被自动跳过，
/// 避免「0.1.5-alpha 起不再引入 fs-ext、但校验器仍要 fs_ext.node」一类
/// 的虚假缺失。两个家族分别承担不同的根因：
///
/// - `fs-ext 2.x`：dsh 0.1.3-alpha 系列直接依赖，需要 `node-gyp` 现场编译，
///   它没有预编译二进制（不像 `koffi` / `node-pty` 用 per-platform prebuilds），
///   缺 `.node` 时运行时直接报 `Cannot find module './build/Release/fs_ext.node'`，
///   是「无法确认内核工作台地址」的根因之一。
/// - `@deepseek-ai/node-addon-system-<plat>`：dsh 0.1.5-alpha 起
///   `dsh-session-persistence-jsonl` 切换到的新平台原语，靠 per-platform
///   子包（`…-darwin-x64` / `…-linux-x64` 等）携带预编译的 `bin/system.node`，
///   完全替代了 `fs-ext` 的 `flock` 角色。它没有现场编译脚本，所以 Node 版本
///   与本地构建工具链一般不会让它缺失；如果真的缺失，多半是 pnpm 的
///   `onlyBuiltDependencies` 之外的网络/磁盘问题，重装即可。
///
/// 后续如果再出现需要现场编译的新增原生依赖，请把 `(package, relative_path)`
/// 追加到该列表——但请优先确认它是否真的没有预编译产物；能 prebuild 就不要
/// 让用户的 Node 版本影响安装。
const NATIVE_MODULE_CHECKS: &[NativeCheck] = &[
    // fs-ext：0.1.3-alpha 系列内核唯一需要 node-gyp 现场编译的入口。
    NativeCheck::new("fs-ext", "build/Release/fs_ext.node"),
    // node-addon-system：0.1.5-alpha 系列内核的平台原语入口。
    // 子包以 `optionalDependencies` 形式被主包按当前 OS/arch 引入，
    // 这里把已知的 6 个目标平台全部列出，`verify_native_modules` 会跳过
    // 未安装的子包。
    NativeCheck::new(
        "@deepseek-ai/node-addon-system-darwin-x64",
        "bin/system.node",
    ),
    NativeCheck::new(
        "@deepseek-ai/node-addon-system-darwin-arm64",
        "bin/system.node",
    ),
    NativeCheck::new(
        "@deepseek-ai/node-addon-system-linux-x64",
        "bin/system.node",
    ),
    NativeCheck::new(
        "@deepseek-ai/node-addon-system-linux-arm64",
        "bin/system.node",
    ),
    NativeCheck::new(
        "@deepseek-ai/node-addon-system-win32-x64",
        "bin/system.node",
    ),
    NativeCheck::new(
        "@deepseek-ai/node-addon-system-win32-arm64",
        "bin/system.node",
    ),
];

/// 单项原生模块检查。包名用 `&'static str` 是因为整张表是静态字面量；
/// 错误文案要把 pkg/rel 拼回用户可见的字符串，所以用 owned `String` 出口。
#[derive(Clone, Copy)]
struct NativeCheck {
    pkg: &'static str,
    rel: &'static str,
}

impl NativeCheck {
    const fn new(pkg: &'static str, rel: &'static str) -> Self {
        Self { pkg, rel }
    }
}

/// 内核 install 后核对 `NATIVE_MODULE_CHECKS`：只对**实际安装**的包
/// 校验其原生二进制（`<kernel>/node_modules/<pkg>/<rel>` 必须存在）。
/// 包本身没出现在依赖树里就直接跳过——`fs-ext` 与 `node-addon-system`
/// 分属不同时代的内核，校验器不应替内核版本"二选一"。
/// 全数到齐返回 `Ok(())`；缺失包名 + 期望路径用于构造可操作的错误文案。
fn verify_native_modules(kernel_root: &Path) -> Result<(), Vec<(String, &'static str)>> {
    let mut missing: Vec<(String, &'static str)> = Vec::new();
    let nm = kernel_root.join("node_modules");
    for check in NATIVE_MODULE_CHECKS {
        // 包目录缺席意味着这条 entry 对当前内核版本根本不适用——
        // 老内核没有 node-addon-system，新内核没有 fs-ext，校验器
        // 不该为它们凭空补一条缺失记录。
        if !nm.join(check.pkg).is_dir() {
            continue;
        }
        let path = nm.join(check.pkg).join(check.rel);
        if !path.is_file() {
            missing.push((check.pkg.to_string(), check.rel));
        }
    }
    if missing.is_empty() {
        Ok(())
    } else {
        Err(missing)
    }
}

/// 把「原生模块二进制缺失」翻译成 UI 可展示的可操作文案：列出缺失包、
/// 按缺失家族给出针对性指引（`fs-ext` 走"安装 Node 24"那条已验证有效的
/// 路径；`node-addon-system-*` 因为携带 prebuild，缺二进制一般是 pnpm
/// 没把 optionalDependencies 拉下来，建议重试或回退版本），并附日志路径
/// 供用户回查。日志路径通过 `log_path` 注入，调用方负责传当日的真实路径。
fn format_native_modules_error(missing: &[(String, &'static str)], log_path: &Path) -> String {
    let list = missing
        .iter()
        .map(|(pkg, rel)| format!("- {pkg}（缺 {rel}）"))
        .collect::<Vec<_>>()
        .join("、");
    // 按缺失的家族分发建议：fs-ext 是真正的 node-gyp 现场编译依赖，
    // Node 25+ 旧 NAN C++ 头文件不兼容会导致构建失败，所以仍推荐
    // 「设置 → 运行时 → 安装 Node.js」让外壳使用内置 Node 24 LTS；
    // node-addon-system-* 自带 prebuild，缺失意味着 optionalDependency
    // 没被拉下来，重试或回退内核版本更对症。两条建议同时出现时一并给出。
    let has_fs_ext = missing.iter().any(|(pkg, _)| pkg == "fs-ext");
    let has_node_addon_system = missing
        .iter()
        .any(|(pkg, _)| pkg.starts_with("@deepseek-ai/node-addon-system-"));
    let mut hint = String::new();
    if has_fs_ext {
        hint.push_str("推荐在「设置 → 运行时」点击「安装 Node.js」，让外壳使用内置的 Node 24 LTS（fs-ext 2.x 的旧 NAN C++ 头文件与 Node 25 的 V8 ABI 不兼容，系统 Node 过新会导致构建脚本即使运行也会失败）");
    }
    if has_node_addon_system {
        if !hint.is_empty() {
            hint.push('；');
        }
        hint.push_str("node-addon-system 子包以 optionalDependencies 形式随平台拉入，缺失通常意味着 pnpm 没有把当前平台的 prebuild 下载下来，请重试安装或回退到其他内核版本");
    }
    if !hint.is_empty() {
        hint.push('。');
    }
    format!(
        "内核依赖的原生模块未构建完成：{list}。{hint}完整日志：{log}",
        log = log_path.display(),
    )
}

/// smoke-load 探针：要逐一 `require()` 的内核包名列表。挑选原则是
/// 「凡是会拉起原生模块的内核入口包」——把它们写成静态字面量是为了让
/// 单元测试可以直接 grep 字符串、验证列表不为空；不是说这些包名
/// 永远不变（实际上 `fs-ext` 在 0.1.5-alpha 起已被
/// `@deepseek-ai/node-addon-system-*` 取代，但 `dsh-fs-local` /
/// `dsh-subprocess-local` 等直接调用者无论上游怎么换底层库都会留着）。
/// `node-addon-require-builtin`（@deepseek-ai/dsh 的隐式依赖）用来
/// 验证 `@deepseek-ai/node-addon-system-darwin-x64` 这类 platform
/// 子包真的把 `bin/system.node` 解析成功，因为 require 才会触发原生
/// 模块加载——`require.resolve` 单独只能确认 JS 路径在位。
const SMOKE_LOAD_TARGETS: &[&str] = &[
    "@deepseek-ai/dsh-fs-local",
    "@deepseek-ai/dsh-subprocess-local",
    "@deepseek-ai/dsh-bash-local",
    "@deepseek-ai/dsh-pwsh-local",
    "@deepseek-ai/dsh-session-persistence-jsonl",
    "@deepseek-ai/dsh-sandbox-local",
    "@deepseek-ai/dsh-credentials-local",
    "@deepseek-ai/dsh-attachment-local",
    "@deepseek-ai/dsh-file-reference-local",
    "@deepseek-ai/dsh-spill-local",
    "node-addon-require-builtin",
];

/// 构造 smoke-load 探针的 JS 源码。设计要点：
///
/// 1. **同时跑 `require.resolve` + `require`**：前者确认 JS 入口在位，
///    后者才会真正加载原生 `.node`。`require.resolve` 通过但 `require`
///    抛 `Cannot find module './build/...'` 正是 fs-ext / node-addon-system
///    这类问题在 pnpm 安装后最普遍的表现形态。
/// 2. **不引入任何 dsh-xlink 私有的包名硬编码**：探针逻辑只问
///    「这些模块能不能 require」，「要测哪些模块」由 `SMOKE_LOAD_TARGETS`
///    这一行常量驱动，新增原生依赖时只追加常量即可，不必再改探针代码。
/// 3. **错误用结构化形式输出**：`PROBE-FAIL <pkg> <code> <message>`，
///    日志解析器（和单元测试）可以从大量堆栈里 grep 出真正失败的那一行。
///    `console.error` 而不是 `console.log`，让 pnpm / vite 风格的 reporter
///    不会吞掉它。
/// 4. **`require.resolve` 失败时立即跳到 `require` 的 catch**：某些
///    平台子包只作为 `optionalDependencies` 出现，`require.resolve` 在
///    当前平台不需要时本来就不该解析成功，探针忽略这类缺失而非报错——
///    这是平台无关的；真要测的是当前**确实应该可加载**的入口包。
fn smoke_load_probe_script() -> String {
    let targets_json = serde_json::to_string(SMOKE_LOAD_TARGETS).unwrap_or_else(|_| "[]".into());
    format!(
        r#"
const targets = {targets_json};
const fail = [];
for (const t of targets) {{
  let entry;
  try {{ entry = require.resolve(t); }}
  catch (e) {{
    // 入口不可解析等同于「内核根本没把这个包拉下来」——是安装事故，
    // 必须报错让壳知道，不允许静默跳过。
    fail.push({{ pkg: t, code: 'UNRESOLVED', message: String(e && e.message || e) }});
    console.error('PROBE-FAIL ' + t + ' UNRESOLVED ' + (e && e.code || ''));
    continue;
  }}
  try {{
    // require 是真正加载原生模块的入口；fs-ext / node-addon-system 等
    // 在这一步会同步抛 'Cannot find module ./build/Release/...'。
    require(t);
  }} catch (e) {{
    fail.push({{ pkg: t, code: 'LOAD_FAILED', message: String(e && e.message || e) }});
    console.error('PROBE-FAIL ' + t + ' LOAD_FAILED ' + (e && e.code || '') + ' ' + (e && e.message || e));
  }}
}}
if (fail.length === 0) {{
  console.log('PROBE-OK ' + targets.length);
  process.exit(0);
}}
console.error('PROBE-FAIL-COUNT ' + fail.length + '/' + targets.length);
process.exit(2);
"#
    )
}

/// 安装结束后跑一次 Node 探针：把所有「必装 + 必能 require」的内核包
/// 真实加载一次，把任何 `Cannot find module './build/Release/...'` 或
/// `NODE_MODULE_VERSION mismatch` 一类错误在「安装完成 → 首次启动」
/// 这条时间线上提前引爆。`verify_native_modules` 只看二进制**文件**
/// 是否就位，`smoke_load_native_modules` 看的是原生模块**能不能加载**
/// ——二者互补：前者快、能给精确的「哪个文件缺失」信息，但漏判
/// `*.node` 存在但 ABI 不兼容的罕见情况；后者慢一点（最多一秒级），
/// 但能把所有 require-time 问题一次性炸出来。
///
/// 失败的退出码 / 输出会经由 `run_with_progress` 落到 `log_path`，
/// 调用方把它转译为用户可见的错误文案（见 `install_version` 中
/// 调用点的 fallback 字符串）。
fn smoke_load_native_modules(
    node_exe: &Path,
    kernel_dir: &Path,
    logs_dir: &Path,
    log_spec: &LogSpec,
    on_progress: &mut impl FnMut(&str),
) -> Result<(), String> {
    on_progress("正在校验内核原生模块的可加载性");
    let script = smoke_load_probe_script();
    // hoisted node-linker 下 `kernel_dir/node_modules` 直接平铺了所有
    // 顶层包；NODE_PATH 把这个目录暴露给 require 解析器，让探针里的
    // `@deepseek-ai/dsh-foo` 等名称能解析到正确路径。这是 npm/yarn
    // 风格的解析，pnpm 的 `--config.node-linker=hoisted` 已经把目录
    // 布局对齐到了 npm。
    let node_dir = node_exe.parent().unwrap_or_else(|| Path::new("."));
    // 探针的失败证据必须留在内存里，而不是只依赖日志文件：日志是排障时
    // 的第二手材料，而错误文案会原样展示给用户。旧的 `PROBE-FAIL` 行以前
    // 只经由 `run_with_progress` 落到日志，UI 上只剩下「退出码 1」——探针
    // 自己的失败约定是退出码 2，所以 1 恰恰**不是**探针报出的失败，
    // 而是 node 根本没跑起来（见 `process::command_through_shell_if_needed`
    // 里那条「含空格路径被 cmd.exe 切碎」的记录）。这类事故只看退出码
    // 无从区分，必须把真实输出带出来。
    //
    // 行数上限避免把 require 失败的整段堆栈灌进进度面板；节点包名与
    // 错误首行足够定位问题，完整输出仍在日志里。
    const MAX_PROBE_DETAIL_LINES: usize = 6;
    let mut probe_detail: Vec<String> = Vec::new();
    let status = run_with_progress(
        node_exe,
        &["--no-warnings", "-e", &script],
        kernel_dir,
        logs_dir,
        log_spec,
        &[node_dir],
        |line| {
            // 把探针的关键信号往前传到 UI（PROBE-OK / PROBE-FAIL），其余
            // 噪音（Node 的 deprecation 提示等）由日志兜底。
            if line.contains("PROBE-") {
                on_progress(line);
                if probe_detail.len() < MAX_PROBE_DETAIL_LINES {
                    probe_detail.push(line.to_string());
                }
            }
        },
    )
    .map_err(|e| format!("无法启动 Node 探针：{e}"))?;
    if status.success() {
        return Ok(());
    }
    // exit code 2 是探针自己的失败约定（见脚本末尾）。其他非零退出既可能
    // 是探针没能启动（shell 分词、ABI 崩溃），也可能是被信号/看护杀掉，
    // 文案必须把两者分开，否则用户只会看到「退出码 1（详见当日日志）」
    // 这种既没有原因也没有下一步的报错。
    let code = status.code();
    let mut reason = match code {
        Some(2) => "Node 探针报告内核依赖无法加载".to_string(),
        Some(other) => format!(
            "Node 探针以退出码 {other} 结束（探针自身的失败约定是退出码 2，因此这通常意味着 node 可执行文件没能真正跑起来，例如路径中含空格被命令解释器切碎、或该 node 与内核的 prebuild ABI 不匹配）"
        ),
        None => "Node 探针被信号终止".to_string(),
    };
    if !probe_detail.is_empty() {
        reason.push_str("；探针输出：");
        reason.push_str(&probe_detail.join(" | "));
    }
    Err(reason)
}

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
    // 把内核自己那个 node 的目录前置到子进程 PATH：`node` 可能是托管安装
    // （`<data_dir>/tools/node/<ver>/bin/node`）或 nvm 的绝对路径，这两种情况下
    // 它都不在继承来的 PATH 上，内核派生的任何 `#!/usr/bin/env node` 子进程都
    // 会找不到解释器。详见 process::command_with_path_dirs。
    let node_dir = node.parent().unwrap_or_else(|| Path::new("."));
    let mut cmd = crate::process::command_with_path_dirs(node, &[node_dir]);
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
    // 派生后立刻纳入「随壳终止」的保护：Windows 上入 Job Object，壳崩溃 / 被
    // 强杀时内核被系统一并收走，不会留下占端口的孤儿（P2-2）。
    crate::process::adopt_kernel_process(&child);
    if let Err(error) = attach_log_drainers(&mut child, &logs_dir(data_dir), &kernel_log_spec()) {
        crate::process::terminate_process_tree(&mut child);
        return Err(AppError::Io(format!("无法接管内核日志：{error}")));
    }
    Ok(child)
}

/// 除非端口已被占用，否则启动当前激活的内核。
///
/// 当端口已有响应、**且监听者确实是本 data dir 的内核**时返回 `Ok(None)`
/// （幂等的启动）；本调用真正拉起进程时返回 `Ok(Some(child))`。
///
/// 端口上有响应但不是本 data dir 的内核时返回错误，而不是静默的 `Ok(None)`：
/// 后者会让「启动工作台」报告成功，而工作台窗口打开的其实是别人的服务——
/// 用户完全看不出内核根本没起来。
pub fn start_maybe(data_dir: &Path, node: &Path) -> Result<Option<Child>, AppError> {
    let s = settings::load(data_dir);
    let port = s.port;
    if port_open(port) {
        if workbench_running(data_dir, &s) {
            return Ok(None);
        }
        let owner = port_listen_pid(port)
            .map(|pid| format!("，占用者 pid {pid}"))
            .unwrap_or_default();
        return Err(AppError::Kernel(format!(
            "端口 {port} 已被其它进程占用{owner}，无法启动工作台。请在设置页改用其它端口，或先释放该端口"
        )));
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
/// Windows 端不在这里扫描：内核在派生时就被放进 `KILL_ON_JOB_CLOSE` 的 Job
/// Object（见 `process::adopt_kernel_process`），壳一退出内核就被系统终止，
/// 不存在需要事后回收的孤儿。参数在 Unix 分支里被 `data_dir == cwd` 比较使用，
/// Windows 编译时整个 #[cfg(unix)] 块被跳过，所以该参数属于平台特定的未使用项。
#[cfg_attr(not(unix), allow(unused_variables))]
pub fn reap_orphans(data_dir: &Path) {
    #[cfg(unix)]
    {
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
                // cwd 等于本 data dir 是比端口更强的身份证据，因此这里不再
                // 传端口：内核可能是用户改端口之前启动的，用当前配置端口去
                // 校验 `--port` 会让它恰好逃过回收。
                kill_pid(pid, None);
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
    // 如实复查：旧实现无论结果都返回 `Ok`，于是 UI 一律提示「已关闭工作台」，
    // 而 taskkill 被拒或进程处于不可中断状态时内核仍在服务——用户既看不到提示
    // 也没有下一步（P2-2）。判据只在**有正面证据**（仍能查到该 pid）时报告失败：
    // `Unknown`（查询工具跑不起来）时保持原有的乐观语义，否则受限环境会把每一次
    // 正常停止都报成失败。
    if process_state(child.id()) == ProcessState::Alive {
        return Err(AppError::Kernel(format!(
            "无法停止内核进程 {}：已发送终止信号但进程仍然存在（可能被系统或安全软件保护）。             请在任务管理器里结束它后重试；不确定时先用「查看日志」确认它是否仍在服务",
            child.id()
        )));
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

/// 从 `netstat -ano` 的输出里取出正在监听 `port` 的 TCP pid。
///
/// 必须按列解析，不能对整行做 `:PORT` 子串匹配 —— 子串会命中三类别的东西：
/// 另一个端口的监听者（`:3090` 命中 `:30900`）、外部地址列里的端口、以及
/// IPv6 地址中的数字尾巴。选中错的 pid 之后 `pid_is_kernel` 恒为 false，
/// 于是 `workbench_pid` 宣称「没有内核在跑」，而内核其实正服务着端口。
///
/// 规则：协议为 `TCP`、恰好 5 列（`TCP 本地地址 外部地址 状态 PID`）、本地
/// 地址以 `:PORT` 结尾、状态为 `LISTENING`、末列可解析为 pid。
#[cfg(any(windows, test))]
fn parse_netstat_listener_pid(output: &str, port: u16) -> Option<u32> {
    let suffix = format!(":{port}");
    for line in output.lines() {
        let columns: Vec<&str> = line.split_whitespace().collect();
        // UDP 行只有 4 列（没有状态列），表头行的末列不是数字；两者都在
        // 下面的条件里被自然排除。
        if columns.len() != 5 {
            continue;
        }
        if !columns[0].eq_ignore_ascii_case("TCP") || columns[3] != "LISTENING" {
            continue;
        }
        // 只比较本地地址列的**结尾**：`127.0.0.1:3090`、`[::1]:3090` 都算，
        // `0.0.0.0:30900` 不算。
        if !columns[1].ends_with(&suffix) {
            continue;
        }
        if let Ok(pid) = columns[4].parse() {
            return Some(pid);
        }
    }
    None
}

#[cfg(windows)]
pub(crate) fn port_listen_pid(port: u16) -> Option<u32> {
    // `netstat -ano` 每个 TCP/UDP 端点输出一行；解析交给
    // `parse_netstat_listener_pid`（按列匹配，见那里的说明）。
    let (success, stdout, _) = crate::process::run_capture_output("netstat", &["-ano"]).ok()?;
    if !success {
        return None;
    }
    parse_netstat_listener_pid(&stdout, port)
}

/// 上次壳启动的内核的 PID 文件：`<data_dir>/kernel.pid`。
fn pid_path(data_dir: &Path) -> PathBuf {
    data_dir.join("kernel.pid")
}

/// 记录已启动内核的 pid 与它绑定的端口（best-effort）。
///
/// 端口必须一起记：只记 pid 时"这个 pid 还是我们那个内核"与"OS 把同一个 pid
/// 复用给了另一个 dsh 内核"无法区分，而后者会让「关闭工作台」对另一个实例的
/// 内核（dev/release 双开、另一个 data dir、CLI 直跑的 `dsh web`）发
/// SIGTERM/SIGKILL（P2-1）。
pub fn write_pid(data_dir: &Path, pid: u32, port: u16) {
    let _ = atomic_write(&pid_path(data_dir), format!("{pid} {port}\n").as_bytes());
}

/// 内核身份记录：pid + 启动时绑定的端口。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PidRecord {
    pub pid: u32,
    /// 旧格式（rc.20 及更早只写 pid）为 `None`；下一次启动会重写成带端口的形态。
    pub port: Option<u16>,
}

/// 读取 pid 记录（兼容只写 pid 的旧格式）。
pub fn read_pid_record(data_dir: &Path) -> Option<PidRecord> {
    let text = fs::read_to_string(pid_path(data_dir)).ok()?;
    let mut parts = text.split_whitespace();
    let pid = parts.next()?.parse().ok()?;
    let port = parts.next().and_then(|value| value.parse::<u16>().ok());
    Some(PidRecord { pid, port })
}

/// 记录里那个内核启动时绑定的端口（若有）。用于在按 pid 终止它之前把完整的
/// 身份证据交给 [`kill_pid`]。
pub fn recorded_kernel_port(data_dir: &Path) -> Option<u16> {
    read_pid_record(data_dir).and_then(|record| record.port)
}

/// 在成功停止后清除 pid 记录。
pub fn clear_pid(data_dir: &Path) {
    let _ = fs::remove_file(pid_path(data_dir));
}

/// 进程命令行查询缓存：Windows 上每次查询都要派生一次 PowerShell + CIM 查询
/// （经验耗时 0.3–1 秒 CPU），而状态轮询每 2.5 秒就会问一次——只要 pid 文件还在
/// （哪怕已经陈旧），就是每分钟约 24 次子进程与 WMI 查询，常驻托盘时是持续性
/// 后台负载（P2-9）。
///
/// 缓存 3 秒（略大于一个轮询周期）：身份判据里其余部分仍然是实时查询——端口
/// 监听活体（`port_listen_pid`）与进程存活（`process_state`）都直接问 OS，因此
/// "内核刚启动/刚退出"最多被延迟一个周期，不会改变启停判定。查询失败（命令跑不
/// 起来）不进缓存，避免把一次偶发失败固化 3 秒。
#[cfg(windows)]
mod command_cache {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    /// 缓存有效期。取值权衡：越大越省 PowerShell，越大越可能用到过期的身份。
    const TTL: Duration = Duration::from_secs(3);

    static CACHE: Mutex<Option<(u32, Instant, Option<String>)>> = Mutex::new(None);

    pub(super) fn get(pid: u32) -> Option<Option<String>> {
        let guard = CACHE.lock().ok()?;
        let (cached_pid, at, value) = guard.as_ref()?;
        (*cached_pid == pid && at.elapsed() < TTL).then(|| value.clone())
    }

    pub(super) fn put(pid: u32, value: Option<String>) {
        if value.is_none() {
            return;
        }
        if let Ok(mut guard) = CACHE.lock() {
            *guard = Some((pid, Instant::now(), value));
        }
    }
}

/// 返回某个进程的命令行，避免不受限的助手命令把 stop 路径挂住。
///
/// **空输出一律当作"查不到"返回 `None`**：Windows 的 PowerShell 对不存在的 pid
/// 不报错，只是输出空串，若把空串当成 `Some("")` 返回，调用方就会得出"这个 pid
/// 存在但不是内核"的结论（`ListenerIdentity::NotKernel`），于是「打开工作台」被
/// 误拒、并让用户去结束一个根本不存在的进程。空输出同样意味着"无法据此判断身份"。
fn process_command(pid: u32) -> Option<String> {
    #[cfg(unix)]
    {
        crate::process::run_capture("ps", &["-p", &pid.to_string(), "-o", "command="])
            .ok()
            .and_then(|(ok, output)| (ok && !output.trim().is_empty()).then_some(output))
    }
    #[cfg(windows)]
    {
        // 轮询路径上复用几秒内的结果，别每 2.5 秒都派生一次 PowerShell（P2-9）。
        if let Some(cached) = command_cache::get(pid) {
            return cached;
        }
        let filter =
            format!("(Get-CimInstance Win32_Process -Filter 'ProcessId = {pid}').CommandLine");
        let value = crate::process::run_capture(
            "powershell.exe",
            &["-NoProfile", "-NonInteractive", "-Command", &filter],
        )
        .ok()
        .and_then(|(ok, output)| (ok && !output.trim().is_empty()).then_some(output));
        command_cache::put(pid, value.clone());
        value
    }
}

/// 进程存活探测的三态结果。
///
/// 必须区分 [`ProcessState::Gone`] 与 [`ProcessState::Unknown`]：`ps` 明确报告
/// "没有这个 pid"（退出码非 0）说明进程确实没了；而 `ps` 自己跑不起来（被沙盒
/// 挡住、二进制缺失）时我们**什么都不知道**。把后者当成"还活着"会让 [`stop`]
/// 在一切正常时报告失败，反过来当成"已死"又会让杀死失败的场景静默通过。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProcessState {
    Alive,
    Gone,
    Unknown,
}

/// 查询进程存活状态（尽力而为，不猜）。
fn process_state(pid: u32) -> ProcessState {
    #[cfg(unix)]
    let captured = crate::process::run_capture("ps", &["-p", &pid.to_string(), "-o", "command="]);
    #[cfg(windows)]
    let captured = {
        let filter =
            format!("(Get-CimInstance Win32_Process -Filter 'ProcessId = {pid}').CommandLine");
        crate::process::run_capture(
            "powershell.exe",
            &["-NoProfile", "-NonInteractive", "-Command", &filter],
        )
    };
    match captured {
        // 查到了进程：查询成功且输出非空。
        Ok((true, output)) if !output.trim().is_empty() => ProcessState::Alive,
        // 查询本身成功但什么都没查到：Unix 上 `ps` 以非 0 退出（没有这个 pid），
        // Windows 上 PowerShell 对不存在的 pid 返回空串且退出码为 0。
        Ok(_) => ProcessState::Gone,
        Err(_) => ProcessState::Unknown,
    }
}

/// 命令行是否属于 dsh 内核（`@deepseek-ai/dsh/lib/bin.js`）。大小写与
/// Windows 的反斜杠路径都要能命中。
fn command_is_kernel(command: &str) -> bool {
    command
        .to_ascii_lowercase()
        .replace('\\', "/")
        .contains("@deepseek-ai/dsh/lib/bin.js")
}

/// 监听某个端口的进程是什么身份。区分 [`ListenerIdentity::Unknown`] 是有必要
/// 的：读不到命令行（权限不足、进程刚退出）不等于"它不是内核"，调用方在
/// Unknown 时应保持原来的宽松行为，否则会把正常场景挡在门外。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ListenerIdentity {
    Kernel,
    NotKernel,
    Unknown,
}

/// 反查监听 `port` 的进程身份。端口空闲、平台查询失败或命令行不可读时
/// 返回 [`ListenerIdentity::Unknown`]。
pub(crate) fn port_listener_identity(port: u16) -> ListenerIdentity {
    let Some(pid) = port_listen_pid(port) else {
        return ListenerIdentity::Unknown;
    };
    match process_command(pid) {
        Some(command) if command_is_kernel(&command) => ListenerIdentity::Kernel,
        Some(_) => ListenerIdentity::NotKernel,
        None => ListenerIdentity::Unknown,
    }
}

/// 判断 `pid` 是否是服务于指定端口的 dsh 内核。三层防护：
/// 1. 命令行必须含 `@deepseek-ai/dsh/lib/bin.js`，挡住被复用 pid 的无关进程；
/// 2. 命令行 `--port` 必须等于给定端口，挡住跨 profile（dev 3091 / release 3090）
///    的壳误把对方的内核认作自己；
/// 3. 给定端口时再向 OS 反查一次监听该端口的 pid，必须等于本 pid，挡住 pid 文件
///    陈旧、内核早已退出但 OS 把 pid 复用给另一个进程的情况——单看命令行残留
///    无法区分这一类，端口活体验证是唯一可信的"这还是不是同一个内核"判据。
///
/// 传 `None` 时跳过第 2、3 层，只保留"这个 pid 现在是否仍是一个 dsh web 内核"
/// 的身份与存活校验。调用方在已经持有更强证据时（例如 pid 文件属于本 data dir、
/// 或已用 cwd 精确匹配过）应当传 `None`——端口会随用户的设置变化，把它当成
/// 身份的一部分会让改过端口的内核彻底失去追踪，见 [`workbench_pid`]。
pub(crate) fn pid_is_kernel(pid: u32, port: Option<u16>) -> bool {
    let Some(command) = process_command(pid) else {
        return false;
    };
    if !command_is_kernel(&command) {
        return false;
    }
    let command = command.to_ascii_lowercase().replace('\\', "/");
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

/// 本 data dir 的内核当前是否真的在运行，并返回它的 pid。
///
/// **判据与配置端口解耦**，这是这个函数存在的理由：用户可以在内核运行期间
/// 把端口从 3090 改成 3091，此后配置端口空闲而内核仍在服务。若把「配置端口
/// 有监听者」当作"在工作台在跑"的唯一判据，状态页会读成「未运行」，用户点
/// 一次「启动工作台」就会在同一 data dir 上拉起第二个内核——两个内核写同一
/// 份会话日志，正是 [`reap_orphans`] 注释里描述的 `seq gap` 损坏。
///
/// 证据优先级：
/// 1. `kernel.pid`（本壳或上一次壳启动内核时写入）+ [`pid_is_kernel`] 的实时
///    身份校验。后者会重新查询进程是否存在，所以内核已退出、pid 被复用给别
///    的进程时这里同样返回 `None`——僵尸检测与存活检测是同一件事。
///    记录里带着启动端口时做完整三层校验；只有 pid 的旧格式退回宽松判据。
/// 2. 配置端口上的监听者，仅在 pid 文件缺失或失效时兜底（例如内核由上一个壳
///    启动，而那个壳因端口已被占用而跳过了启动、从未写出 pid 文件）。兜底同样
///    要求监听者通过身份校验，因此无关进程占用端口不会再被误报成「工作台在
///    运行」。
pub fn workbench_pid(data_dir: &Path, settings: &Settings) -> Option<u32> {
    if let Some(record) = read_pid_record(data_dir) {
        // 带端口的记录走完整判据（身份 + 命令行里的 `--port` 一致 + 该端口的
        // 监听者就是本 pid）。pid 被复用给另一个 dsh 内核时，第 2 层会挡住它：
        // 那是另一个实例的内核，不属于本 data dir，更不能被我们杀掉（P2-1）。
        if pid_is_kernel(record.pid, record.port) {
            return Some(record.pid);
        }
    }
    if port_open(settings.port) {
        if let Some(pid) = port_listen_pid(settings.port) {
            // 这里已知监听端口就是 `settings.port`，把端口一起传下去让判据完整。
            if pid_is_kernel(pid, Some(settings.port)) {
                return Some(pid);
            }
        }
    }
    None
}

/// [`workbench_pid`] 的布尔形式：工作台是否正在运行。
pub fn workbench_running(data_dir: &Path, settings: &Settings) -> bool {
    workbench_pid(data_dir, settings).is_some()
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

    /// `netstat -ano` 的真实形状（保留中文 Windows 的本地化表头，因为表头
    /// 行也在被解析的输入里）。
    const NETSTAT_SAMPLE: &str = "\
活动连接

  协议  本地地址          外部地址        状态           PID
  TCP    127.0.0.1:30900        0.0.0.0:0              LISTENING       9001
  TCP    127.0.0.1:3090         0.0.0.0:0              LISTENING       4242
  TCP    127.0.0.1:3090         127.0.0.1:51000        ESTABLISHED     4242
  TCP    127.0.0.1:51000        127.0.0.1:3090         ESTABLISHED     7777
  TCP    [::1]:3090             [::]:0                 LISTENING       4242
  UDP    127.0.0.1:3090         *:*                                    5555
";

    #[test]
    fn kernel_command_identity_accepts_real_forms_and_rejects_near_misses() {
        // `port_listener_identity` 只有在这条判据成立时才敢说"监听者是内核"。
        assert!(command_is_kernel(
            "/usr/local/bin/node /Users/u/.dsh/desktop/kernels/0.1.5/lib/node_modules/@deepseek-ai/dsh/lib/bin.js --port 3090"
        ));
        // Windows 命令行用反斜杠，且大小写不固定。
        assert!(command_is_kernel(
            r"C:\nodejs\node.exe C:\Users\u\.dsh\desktop\kernels\0.1.5\node_modules\@DeepSeek-AI\dsh\lib\bin.js --port 3090"
        ));
        // 近失：同命名空间下的另一个包、以及占用了端口的无关进程。
        assert!(!command_is_kernel(
            "/usr/bin/node /p/node_modules/@deepseek-ai/dsh-tools/lib/bin.js"
        ));
        assert!(!command_is_kernel("/usr/bin/python3 -m http.server 3090"));
        assert!(!command_is_kernel(""));
    }

    #[test]
    fn netstat_parser_does_not_confuse_a_longer_port() {
        // P2-5：`:3090` 的子串匹配会先命中 `:30900` 那一行并返回 pid 9001，
        // 于是 pid_is_kernel 失败、workbench_pid 报「没有内核在跑」。
        assert_eq!(parse_netstat_listener_pid(NETSTAT_SAMPLE, 3090), Some(4242));
        assert_eq!(
            parse_netstat_listener_pid(NETSTAT_SAMPLE, 30900),
            Some(9001)
        );
    }

    #[test]
    fn netstat_parser_ignores_non_listening_and_foreign_address_matches() {
        // 外部地址列里出现该端口、或状态不是 LISTENING 的行都不算证据：
        // 只有 `LISTENING` 的那一行才提供 pid。
        let only_established = "\
  协议  本地地址          外部地址        状态           PID
  TCP    127.0.0.1:51000        127.0.0.1:3090         ESTABLISHED     7777
";
        assert_eq!(parse_netstat_listener_pid(only_established, 3090), None);

        // UDP 行（4 列、无状态）不是 TCP 监听者，不能拿来当内核。
        let only_udp = "\
  协议  本地地址          外部地址        状态           PID
  UDP    127.0.0.1:3090         *:*                                    5555
";
        assert_eq!(parse_netstat_listener_pid(only_udp, 3090), None);
    }

    #[test]
    fn netstat_parser_accepts_ipv6_loopback_and_empty_output() {
        let ipv6 = "\
  TCP    [::1]:3091             [::]:0                 LISTENING       321
";
        assert_eq!(parse_netstat_listener_pid(ipv6, 3091), Some(321));
        // 端口空闲 / netstat 只给出表头时必须是 None，而不是误报某个 pid。
        assert_eq!(parse_netstat_listener_pid("", 3090), None);
        assert_eq!(
            parse_netstat_listener_pid("  协议  本地地址  外部地址  状态  PID\n", 3090),
            None
        );
    }

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
        // 端口留空（0）：这个用例只验证 pid 记录这条证据链，不依赖端口。
        let settings = Settings {
            port: 0,
            ..Settings::default()
        };
        settings::save(&root, &settings).expect("save test settings");

        // 「工作台在运行」现在由内核身份决定（命令行含内核入口），而不是
        // "某个进程恰好占着配置端口"。因此这里派生一个命令行里带该标识的
        // 占位进程，不需要真的能跑内核；`; :` 用来阻止 sh 把自身优化成对
        // sleep 的 exec（那样会丢掉我们塞进 argv 的标识）。
        let mut placeholder = std::process::Command::new("sh")
            .arg("-c")
            .arg("sleep 30; :")
            .arg("@deepseek-ai/dsh/lib/bin.js")
            .arg("web")
            .arg("--port")
            .arg("3090")
            .spawn()
            .expect("spawn placeholder kernel");
        // 记录里带端口，占位进程的命令行也必须带同一个端口：真实内核由
        // `kernel::start` 以 `--port <port>` 启动，`workbench_pid` 的第 2 层正是
        // 靠这个参数把"另一个实例的内核"排除在外（P2-1）。
        write_pid(&root, placeholder.id(), 3090);

        let error = set_active(&root, "0.1.2").expect_err("running workbench must block switch");

        assert!(error
            .to_string()
            .contains("请先点击「关闭工作台」停止工作台后再切换内核"));

        let _ = placeholder.kill();
        let _ = placeholder.wait();
        clear_pid(&root);
        fs::remove_dir_all(&root).expect("remove test data");
    }

    /// 反向断言：端口上的**无关**监听者不再被当作"工作台在运行"，因此
    /// 不会错误地阻止切换内核版本。
    #[test]
    fn switching_active_version_is_allowed_when_only_an_unrelated_listener_holds_the_port() {
        let root = std::env::temp_dir().join(format!(
            "dsh-active-version-unrelated-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind port");
        let port = listener.local_addr().expect("listener addr").port();
        settings::save(
            &root,
            &Settings {
                port,
                ..Settings::default()
            },
        )
        .expect("save test settings");

        // 没有内核在跑，所以守卫放行；随后因为版本未安装而失败——
        // 错误信息应当是"未安装"，而不是"工作台正在运行"。
        let error = set_active(&root, "0.1.2").expect_err("missing version must still fail");
        let text = error.to_string();
        assert!(
            !text.contains("工作台正在启动或运行"),
            "无关监听者不应阻止切换内核，实际：{text}"
        );
        assert!(
            text.contains("未安装"),
            "应当因为版本未安装而失败，实际：{text}"
        );

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

    /// 当内核目录缺少 `fs-ext/build/Release/fs_ext.node`（pnpm 跳过构建
    /// 脚本、Node 版本过新无法编译等情形都会导致此状态），`verify_native_modules`
    /// 必须显式报告缺失，不能让 install 静默成功——后续启动会撞上
    /// `Cannot find module './build/Release/fs_ext.node'`，UI 端表现为
    /// 「无法确认内核工作台地址」，根因却被吞掉。
    #[test]
    fn verify_native_modules_flags_missing_fs_ext_binary() {
        let root = std::env::temp_dir().join(format!(
            "dsh-native-check-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let pkg_root = root.join("node_modules").join("fs-ext");
        fs::create_dir_all(pkg_root.join("build/Release")).unwrap();
        // 不写 .node 文件——模拟原生模块未构建的状态。
        let missing = verify_native_modules(&root).expect_err("应报告缺失");
        assert!(
            missing.iter().any(|(pkg, _)| pkg == "fs-ext"),
            "missing list 必须包含 fs-ext：{:?}",
            missing
        );
        // 错误文案应当把缺失的相对路径与「安装 Node.js」指引同时给出，
        // 让用户有可操作的下一步，而不是仅仅说"内核依赖未完整安装"。
        let log = root.join("install.log");
        let rendered = format_native_modules_error(&missing, &log);
        assert!(rendered.contains("fs-ext"), "渲染文案：{rendered}");
        assert!(
            rendered.contains("设置"),
            "应提示到设置页安装运行时：{rendered}"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    /// 全部原生模块就位时 `verify_native_modules` 必须返回 `Ok`，
    /// 即便内核目录本身只是个最小可启动 stub——验证逻辑只看 `*.node`
    /// 是否存在，不依赖其它安装产物。
    #[test]
    fn verify_native_modules_passes_when_binary_exists() {
        let root = std::env::temp_dir().join(format!(
            "dsh-native-check-pass-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let release = root
            .join("node_modules")
            .join("fs-ext")
            .join("build/Release");
        fs::create_dir_all(&release).unwrap();
        fs::write(release.join("fs_ext.node"), b"stub").unwrap();
        verify_native_modules(&root).expect("fs_ext.node 在位时必须通过");
        fs::remove_dir_all(&root).unwrap();
    }

    /// 0.1.5-alpha 系列内核（fs-ext 已不再被依赖，改为
    /// `@deepseek-ai/node-addon-system-<plat>`）：校验器必须接受「fs-ext
    /// 不在依赖树里、node-addon-system 子包的 prebuild 已就位」的状态，
    /// 否则 0.1.5-alpha 的全新安装永远过不了关——而实际上 fs_ext.node 本来
    /// 就不该被期望存在。这是 commit 4c8da01 引入 strict 校验后唯一漏掉的
    /// 路径。
    #[test]
    fn verify_native_modules_passes_when_node_addon_system_present_and_fs_ext_absent() {
        let root = std::env::temp_dir().join(format!(
            "dsh-native-check-nas-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        // 模拟新内核：只装 node-addon-system-darwin-x64，并写好其 prebuild。
        // 注意：刻意不创建 fs-ext 目录——它在新内核里就不该存在。
        let bin = root
            .join("node_modules")
            .join("@deepseek-ai/node-addon-system-darwin-x64")
            .join("bin");
        fs::create_dir_all(&bin).unwrap();
        fs::write(bin.join("system.node"), b"stub").unwrap();
        verify_native_modules(&root)
            .expect("fs-ext 缺席但 node-addon-system prebuild 就位时必须通过");
        fs::remove_dir_all(&root).unwrap();
    }

    /// 新内核的 node-addon-system 子包目录虽然存在，但 prebuild 文件缺失：
    /// 校验器应当把这一项报为缺失，让 UI 引导用户重试，而不是放过错误状态。
    #[test]
    fn verify_native_modules_flags_missing_node_addon_system_binary() {
        let root = std::env::temp_dir().join(format!(
            "dsh-native-check-nas-missing-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let pkg = root
            .join("node_modules")
            .join("@deepseek-ai/node-addon-system-darwin-x64");
        fs::create_dir_all(pkg.join("bin")).unwrap();
        // 不写 system.node——模拟 prebuild 下载失败。
        let missing = verify_native_modules(&root).expect_err("应报告缺失");
        assert!(
            missing
                .iter()
                .any(|(pkg, _)| pkg == "@deepseek-ai/node-addon-system-darwin-x64"),
            "missing list 必须包含 node-addon-system-darwin-x64：{:?}",
            missing
        );
        // 错误文案应当对 node-addon-system 给出针对性的重试建议，
        // 而不是把「安装 Node 24」套到它头上。
        let log = root.join("install.log");
        let rendered = format_native_modules_error(&missing, &log);
        assert!(
            rendered.contains("node-addon-system"),
            "渲染文案：{rendered}"
        );
        assert!(
            !rendered.contains("Node 25"),
            "不应把 Node 版本建议强加给 node-addon-system 缺失：{rendered}"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    /// 文案分支：fs-ext 缺失与 node-addon-system 缺失同时出现时，
    /// 两条建议都要出现，且按 fs-ext 在前、node-addon-system 在后的
    /// 固定顺序串联（错误文案要给「重试」类用户与「换 Node」类用户
    /// 都能用上的指引）。
    #[test]
    fn format_native_modules_error_combines_hints_for_both_families() {
        let log = Path::new("/tmp/dsh-install-test.log");
        let missing = vec![
            ("fs-ext".to_string(), "build/Release/fs_ext.node"),
            (
                "@deepseek-ai/node-addon-system-linux-x64".to_string(),
                "bin/system.node",
            ),
        ];
        let rendered = format_native_modules_error(&missing, log);
        assert!(rendered.contains("设置"), "应给出 Node 24 建议：{rendered}");
        assert!(
            rendered.contains("optionalDependencies"),
            "应给出 node-addon-system 重试建议：{rendered}"
        );
        let settings_pos = rendered.find("设置").unwrap();
        let optional_pos = rendered.find("optionalDependencies").unwrap();
        assert!(
            settings_pos < optional_pos,
            "Node 建议应排在 node-addon-system 建议之前：{rendered}"
        );
    }

    /// smoke-load 探针脚本必须包含至少一个会拉起原生模块的入口包，
    /// 且必须同时调用 `require.resolve` 与 `require`——前者只确认
    /// JS 路径在位，后者才真正加载 `.node`。如果未来有人把 `require`
    /// 替换成 `require.resolve`，fs-ext / node-addon-system 缺失这种
    /// 形态会被这条测试抓住，立刻挂掉。
    #[test]
    fn smoke_load_probe_script_requires_real_native_loading() {
        let script = smoke_load_probe_script();
        assert!(
            script.contains("require.resolve"),
            "探针必须先 require.resolve 再 require：{script}"
        );
        assert!(
            script.contains("\n    require(t);") || script.contains("\n    require("),
            "探针必须对每个目标真实 require 一次（不是仅解析）：{script}"
        );
        // SMOKE_LOAD_TARGETS 必须非空、且至少包含一个能拉起原生模块的
        // 入口；上一版 0.1.3-alpha 的 fs-ext 守护已经被
        // node-addon-system 替代，但 session-persistence-jsonl 在两个
        // 版本里都必装——是回归测试的稳定锚点。
        assert!(
            !SMOKE_LOAD_TARGETS.is_empty(),
            "SMOKE_LOAD_TARGETS 不能为空"
        );
        assert!(
            SMOKE_LOAD_TARGETS.contains(&"@deepseek-ai/dsh-session-persistence-jsonl"),
            "探针目标里必须包含 dsh-session-persistence-jsonl：{:?}",
            SMOKE_LOAD_TARGETS
        );
        // 失败约定：PROBE-FAIL 行 + exit 2。字符串直接 grep，避免重构时
        // 漏改约定。
        assert!(
            script.contains("PROBE-FAIL"),
            "缺少 PROBE-FAIL 约定：{script}"
        );
        assert!(
            script.contains("process.exit(2)"),
            "缺少 exit(2) 约定：{script}"
        );
    }

    /// smoke-load 真实跑通路径：构造一个**空的**内核目录（连 dsh 包都
    /// 没装），让探针发现所有目标都不可解析，应当以非零退出码失败。
    /// 这是把 smoke-load 接入 install_version 后的端到端防线——避免
    /// `verify_native_modules` 通过但运行时 require 失败的旧坑再次
    /// 漏过。
    #[test]
    fn smoke_load_native_modules_fails_when_target_unresolved() {
        // 用 rustc 自带的 rust-script 跑不起 Node，但 `node` 在 CI 上一般
        // 存在；本地 macOS 上没装 Node 的环境，跳过该测试。`#[ignore]` 给
        // 默认运行兜底，CI 可以单独跑。
        let Some(node) = find_system_node() else {
            eprintln!(
                "smoke_load_native_modules_fails_when_target_unresolved: 跳过（未找到 node）"
            );
            return;
        };
        let root = std::env::temp_dir().join(format!(
            "dsh-smoke-load-fail-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        fs::create_dir_all(root.join("node_modules")).unwrap();
        let logs_dir = root.join("logs");
        fs::create_dir_all(&logs_dir).unwrap();
        let log_spec = install_log_spec("smoke-fail");
        let mut captured = Vec::<String>::new();
        let result = smoke_load_native_modules(&node, &root, &logs_dir, &log_spec, &mut |line| {
            captured.push(line.to_string())
        });
        fs::remove_dir_all(&root).unwrap();
        assert!(
            result.is_err(),
            "空内核目录必须被探针报错：captured={captured:?}"
        );
        let err = result.unwrap_err();
        assert!(err.contains("探针"), "错误文案应当提到探针：{err}");
    }

    /// smoke-load 通过路径：构造一个让探针「至少不报 UNRESOLVED」的场景，
    /// 即提供一个空 module 让 `@deepseek-ai/dsh-session-persistence-jsonl`
    /// 解析到但 `require` 抛错（这里用一个空目录 + 占位 package.json
    /// 模拟）。这个测试只验证探针**真正启动 Node 并执行**，完整的
    /// 成功路径需要在真实内核上验证——这里覆盖的是「探针进程能起来」
    /// 这一最基本的前提。
    #[test]
    fn smoke_load_native_modules_invokes_node() {
        let Some(node) = find_system_node() else {
            eprintln!("smoke_load_native_modules_invokes_node: 跳过（未找到 node）");
            return;
        };
        let root = std::env::temp_dir().join(format!(
            "dsh-smoke-load-invoke-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        fs::create_dir_all(root.join("node_modules")).unwrap();
        let logs_dir = root.join("logs");
        fs::create_dir_all(&logs_dir).unwrap();
        let log_spec = install_log_spec("smoke-invoke");
        let mut captured = Vec::<String>::new();
        // 探针应当被调用并以非零退出码结束（因为内核里啥都没装）。
        let _ = smoke_load_native_modules(&node, &root, &logs_dir, &log_spec, &mut |line| {
            captured.push(line.to_string())
        });
        // 关键信号必须出现在 on_progress 通道里（PROBE-FAIL-COUNT）。
        assert!(
            captured.iter().any(|l| l.contains("PROBE-")),
            "on_progress 必须收到 PROBE-* 标记：{captured:?}"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    /// 在 PATH / `command_with_path` 的合并路径下找一个可用的 `node`。
    /// 测试环境若没装 Node 就跳过（不阻塞常规 CI）。
    fn find_system_node() -> Option<PathBuf> {
        let candidates: &[&str] = if cfg!(windows) {
            &["node.exe"]
        } else {
            &["node"]
        };
        for c in candidates {
            if let Ok(p) = which_first(c) {
                return Some(p);
            }
        }
        None
    }

    fn which_first(name: &str) -> std::io::Result<PathBuf> {
        // 简化版 which：从 PATH 与 crate::env::merged_path() 之外的常用
        // 位置查找。Windows 上 PATH 由 command_with_path 合并过；这里
        // 直接 std::env::var 后 split，不必再走 merged_path。
        let path_var = std::env::var_os("PATH")
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "PATH not set"))?;
        for entry in std::env::split_paths(&path_var) {
            let full = entry.join(name);
            if full.is_file() {
                return Ok(full);
            }
        }
        // 兜底：典型 macOS Homebrew / Linux 系统 Node 路径。
        for hint in [
            "/usr/local/bin/node",
            "/opt/homebrew/bin/node",
            "/usr/bin/node",
        ] {
            let p = PathBuf::from(hint);
            if p.is_file() {
                return Ok(p);
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("node not found on PATH: {name}"),
        ))
    }

    /// 一个隔离的 data dir，供"工作台是否在运行"的判据测试使用。
    fn workbench_test_dir(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "dsh-workbench-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        fs::create_dir_all(&root).expect("create data dir");
        root
    }

    /// 端口被**无关进程**占用时，不能报「工作台在运行」，启动路径也必须明确
    /// 报出端口冲突。
    ///
    /// 修复前这两个判据都以"配置端口上有监听者"为准：面板显示「运行中」，
    /// `start_maybe` 静默返回 `Ok(None)`（当作"已在运行"），而内核根本没起来
    /// ——用户接着点「打开工作台」，打开的其实是那个无关进程。
    #[test]
    fn unrelated_listener_on_the_port_is_not_a_running_workbench() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind port");
        let port = listener.local_addr().expect("listener addr").port();
        let root = workbench_test_dir("unrelated-listener");
        settings::save(
            &root,
            &Settings {
                port,
                ..Settings::default()
            },
        )
        .expect("save settings");
        let current = settings::load(&root);

        // 监听者是本测试进程，命令行不含内核标识。
        assert!(
            !workbench_running(&root, &current),
            "无关进程占用的端口不能被判成正在运行的工作台"
        );
        assert!(
            !status(&root, &current).running,
            "状态快照同样不能把无关监听者报成运行中"
        );

        let error = start_maybe(&root, Path::new("/nonexistent/node"))
            .expect_err("端口被无关进程占用时必须报错，而不是静默认为已在运行");
        assert!(
            error.to_string().contains("已被其它进程占用"),
            "错误信息应说明端口冲突，实际：{error}"
        );

        drop(listener);
        let _ = fs::remove_dir_all(&root);
    }

    /// 全新安装失败后不得留下半成品目录。
    ///
    /// `list_installed` 会把 `kernels/` 下任何目录都当成已安装版本，用户能切换
    /// 过去，真正的失败推迟到启动内核时才发生（原生模块缺失），随后被启动看护
    /// 当成疑似插件问题处理几分钟——用户完全看不出根因是这个版本没装完。
    #[test]
    fn failed_fresh_install_leaves_no_half_built_version() {
        let root = workbench_test_dir("failed-install");
        let version = "0.9.9";

        let error = install_version(
            &root,
            Path::new("/nonexistent/node"),
            Path::new("/nonexistent/pnpm"),
            version,
            |_| {},
        )
        .expect_err("pnpm 不存在时必须失败");
        assert!(!error.to_string().is_empty());

        assert!(
            !kernel_dir(&root, version).exists(),
            "失败的全新安装不得留下半成品目录"
        );
        assert!(
            !list_installed(&root).iter().any(|v| v.version == version),
            "半成品不得出现在已安装列表里"
        );

        let _ = fs::remove_dir_all(&root);
    }

    /// `kernel.pid` 指向一个已经不存在（或已被复用给无关进程）的 pid 时，
    /// 判据必须回落到「没有内核在运行」。
    ///
    /// 这也是"僵尸句柄"检测：内核自行退出后 pid 记录会留在磁盘上，只看
    /// "记录存在"会让壳一直认为工作台活着。
    #[test]
    fn stale_pid_record_is_not_a_running_workbench() {
        let root = workbench_test_dir("stale-pid");
        // 端口留空（0）使这个用例只走 pid 文件分支，不触发端口兜底——
        // 否则本机恰好在默认端口上跑着内核时这个断言会失真。
        settings::save(
            &root,
            &Settings {
                port: 0,
                ..Settings::default()
            },
        )
        .expect("save settings");
        let current = settings::load(&root);

        assert!(
            !workbench_running(&root, &current),
            "没有 pid 记录时不应报告工作台在运行"
        );

        write_pid(&root, u32::MAX, 3090);
        assert!(
            !workbench_running(&root, &current),
            "陈旧的 pid 记录不能被判成正在运行的工作台"
        );

        let _ = fs::remove_dir_all(&root);
    }

    /// P2-1：`kernel.pid` 里的身份记录必须带启动端口，并两种格式都能读。
    #[test]
    fn pid_record_carries_the_start_port_and_accepts_the_legacy_format() {
        let root = workbench_test_dir("pid-record");
        write_pid(&root, 4321, 3091);
        assert_eq!(
            read_pid_record(&root),
            Some(PidRecord {
                pid: 4321,
                port: Some(3091)
            })
        );
        assert_eq!(recorded_kernel_port(&root), Some(3091));

        // 旧格式（rc.20 及更早只写一个数字）继续可读，端口为 None。
        fs::write(pid_path(&root), "4321\n").expect("legacy pid file");
        assert_eq!(
            read_pid_record(&root),
            Some(PidRecord {
                pid: 4321,
                port: None
            })
        );
        assert_eq!(recorded_kernel_port(&root), None);

        // 坏内容不算记录。
        fs::write(pid_path(&root), "not-a-pid\n").expect("garbage pid file");
        assert_eq!(read_pid_record(&root), None);

        let _ = fs::remove_dir_all(&root);
    }

    /// P2-2：存活探测必须区分"查到了""确实没了""查不了"。
    ///
    /// `stop()` 只在有正面证据（仍能查到该 pid）时才报告停止失败；把"查不了"
    /// 当成"还活着"会让受限环境里每一次正常停止都报错。
    #[cfg(unix)]
    #[test]
    fn process_state_distinguishes_gone_from_unknown() {
        let mut child = std::process::Command::new("/bin/sh")
            .args(["-c", "sleep 5"])
            .spawn()
            .expect("spawn probe child");
        let pid = child.id();
        assert_eq!(process_state(pid), ProcessState::Alive, "活着的子进程");

        let _ = child.kill();
        let _ = child.wait();
        // 已被回收：`ps -p <pid>` 退出码非 0（进程不存在），必须是 Gone 而不是 Unknown。
        assert_eq!(process_state(pid), ProcessState::Gone, "已回收的 pid");

        // 一个几乎不可能存在的 pid 同样是 Gone。
        assert_eq!(process_state(u32::MAX), ProcessState::Gone);
    }

    /// P2-1：记录端口与进程实际端口不一致时不得认领。
    ///
    /// 用一个命令行里同时含内核标记与 `--port` 的真实进程模拟"另一个实例的内核"：
    /// 它的 pid 落在本 data dir 的记录里（端口写成另一个值）时，旧判据
    /// （`pid_is_kernel(pid, None)`，只看"这个 pid 现在是不是某个 dsh 内核"）会
    /// 认领它，于是「关闭工作台」对它发 SIGTERM/SIGKILL——杀掉的是另一个实例正在
    /// 服务的会话。带端口的完整判据必须拒绝，同时在端口一致时仍然认领。
    #[cfg(unix)]
    #[test]
    fn recorded_port_mismatch_is_not_our_workbench() {
        let root = workbench_test_dir("pid-port-mismatch");
        // 端口留空（0）让这个用例只走 pid 文件分支，避免本机恰好在默认端口上有
        // 内核时干扰断言。
        settings::save(
            &root,
            &Settings {
                port: 0,
                ..Settings::default()
            },
        )
        .expect("save settings");
        let current = settings::load(&root);

        // 诱饵必须让内核标记与 `--port` 留在**自己**的命令行里：`sh -c 'sleep 30'`
        // 会被 shell 优化成 exec sleep，多余参数随之消失（第一版诱饵就是这么失真的），
        // 所以这里用两条命令的脚本来保证 shell 一直活着；端口参数还必须放在脚本
        // 末尾——`--port 45231;` 会被分号粘成一个 token，`pid_is_kernel` 的第 2 层
        // 是按空格切分后逐 token 比对的。
        let mut child = std::process::Command::new("/bin/sh")
            .args([
                "-c",
                "sleep 5; true @deepseek-ai/dsh/lib/bin.js --port 45231",
            ])
            .spawn()
            .expect("spawn decoy kernel");
        let pid = child.id();
        let mut ready = false;
        for _ in 0..40 {
            if pid_is_kernel(pid, None) {
                ready = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(
            ready,
            "诱饵进程的命令行应当含内核标记：{:?}",
            process_command(pid)
        );
        assert!(
            !pid_is_kernel(pid, Some(45232)),
            "端口不一致必须被拒绝（旧判据在这里返回 true）"
        );

        write_pid(&root, pid, 45232);
        assert!(
            !workbench_running(&root, &current),
            "记录端口与内核实际端口不一致时不得认领（P2-1）"
        );

        // 端口一致时仍然认领——修复不能做成"永远不认"。
        write_pid(&root, pid, 45231);
        assert_eq!(workbench_pid(&root, &current), Some(pid));

        let _ = child.kill();
        let _ = child.wait();
        let _ = fs::remove_dir_all(&root);
    }
}
