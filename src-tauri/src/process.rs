//! 抑制 Windows 为子进程分配的 console 窗口，以及一个共享助手，把长时间运行
//! 的子进程输出同时流式输出到日志文件和进程内的进度回调。
//!
//! 壳是 GUI 子系统应用：每个未指定 `CREATE_NO_WINDOW` 的 `Command`
//! 都会让 Windows 短暂分配一个 console 窗口，用户会看到一个闪烁的终端。
//! 本 crate 中所有助手进程的 spawn 都通过 `quiet`。

use std::ffi::OsStr;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    mpsc, Arc, Mutex,
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use time::format_description::FormatItem;
use time::macros::format_description;
use time::OffsetDateTime;

static ATOMIC_WRITE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// 隐藏 Windows 否则会为子进程闪烁的 console 窗口。其他平台上为 no-op。
pub fn quiet(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// 将一个小型持久化文件写入同目录临时文件，再替换正式文件。写入文件
/// 和替换动作之间不会暴露截断的 JSON；同目录临时文件也确保 rename 不会
/// 跨文件系统。Unix 直接使用 rename 的原子替换语义，Windows 在目标已存
/// 在时先移除旧文件再 rename，至少不会让读者看到半写入内容。
pub fn atomic_write(path: &Path, contents: &[u8]) -> io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let file_name = path
        .file_name()
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "atomic write path has no file name",
            )
        })?
        .to_string_lossy();

    for attempt in 0..100u32 {
        let sequence = ATOMIC_WRITE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(
            ".{file_name}.tmp-{}-{sequence}-{attempt}",
            std::process::id()
        ));
        let mut file = match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };

        let result = (|| -> io::Result<()> {
            file.write_all(contents)?;
            file.sync_all()?;
            drop(file);
            replace_file(&temporary, path)?;
            sync_parent_directory(parent)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        return result;
    }

    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a unique temporary file for atomic write",
    ))
}

fn replace_file(temporary: &Path, destination: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        match fs::rename(temporary, destination) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                fs::remove_file(destination)?;
                fs::rename(temporary, destination)
            }
            Err(error) => Err(error),
        }
    }
    #[cfg(not(windows))]
    {
        fs::rename(temporary, destination)
    }
}

#[cfg(unix)]
fn sync_parent_directory(parent: &Path) -> io::Result<()> {
    fs::File::open(parent)?.sync_all()
}

#[cfg(not(unix))]
fn sync_parent_directory(_parent: &Path) -> io::Result<()> {
    Ok(())
}

/// 读取一个小型 JSON 状态文件的结果。
///
/// **必须区分「文件不存在」与「读取/解析失败」**。把后者退化成"空清单"是
/// 静默数据丢失的通用形态：同一次启动流程会据此清退接线、删除物化产物，并用
/// 空内容覆盖掉用户的真实数据——Windows 上杀毒软件短暂锁一下文件、或一次手工
/// 编辑留下的语法错误，就足以触发。`store.json` / `state.json` / `settings.json`
/// 全都经这里读取。
pub enum StateRead<T> {
    Loaded(T),
    /// 文件不存在：正常的首次运行。
    Missing,
    /// 读取或解析失败。调用方**不得**把它当成空状态继续：写路径要报错退出，
    /// 清扫路径要跳过，展示路径要把它暴露成警告。原文件保持在原处不动，
    /// 用户仍可人工恢复。
    Corrupt {
        reason: String,
    },
}

/// 读取并解析 `path`，区分"不存在"与"损坏"（见 [`StateRead`]）。
pub fn read_state_file<T: serde::de::DeserializeOwned>(path: &Path) -> StateRead<T> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return StateRead::Missing,
        Err(error) => {
            return StateRead::Corrupt {
                reason: format!("无法读取 {}：{error}", path.display()),
            }
        }
    };
    match serde_json::from_str(&text) {
        Ok(value) => StateRead::Loaded(value),
        Err(error) => StateRead::Corrupt {
            reason: format!("无法解析 {}：{error}", path.display()),
        },
    }
}

/// 为一次性外部工具（`git`、`tar` 等）构造 `Command`，让它继承合并后的 PATH，
/// 这样 GUI 壳的子进程能解析到用户安装在自己 user PATH 下的工具。
/// `process::spawn` 覆盖了长时间运行的助手（pnpm/npm）并把同样的 PATH
/// 透传过去；本助手是单次执行的兄弟——壳里任何不处于 `process::spawn`
/// 中的直接 `Command::new`，都应该通过这里构造，使 `tauri build` 的
/// Windows GUI 子系统也能看到用户从 `cmd.exe` 看到的同一套工具链。
///
/// 若不做合并，Windows GUI 子系统进程里的 `Command::new("git")` 只能
/// 在系统 PATH 上查找 `git.exe`。Git for Windows 以及大多数 Windows
/// 安装器都注册在 `HKCU\Environment\Path`（user PATH），而非系统 PATH，
/// 因此查找会失败，用户看到的是被包装为「未找到 git（git 来源的插件
/// 需要 git；请先安装 git）」的错误。同类问题也会影响任何仅 user PATH
/// 的工具——`tar` 位于 `C:\Windows\System32\tar.exe`，在 Windows 10+
/// 上即使不合并也能工作，但显式 stamp 让 macOS/Linux 保持一致，并
/// 避免未来某款工具不再随系统安装时出现意外。
pub fn command_with_path<S: AsRef<OsStr>>(program: S) -> Command {
    let mut cmd = Command::new(program);
    cmd.env("PATH", crate::env::merged_path());
    cmd
}

/// 与 [`command_with_path`] 相同，但把 `extra_path_dirs` 前置到子进程的 PATH。
///
/// 用于**长驻**进程（内核）与任何可能派生出 `#!/usr/bin/env node` 子进程的
/// 场景：调用方已经解析出唯一可信的 node 路径，把它所在目录前置之后，子进程
/// 里的 shebang 才会解析到同一个 node。缺少这一步时，走托管安装（或 nvm
/// 绝对路径探测）的用户，其内核 PATH 里根本没有那个 node 目录——插件 CLI、
/// `npm`/`npx`、工作台里的终端任务一律以 `env: node: No such file or directory`
/// 失败；若用户 PATH 上恰好还有另一条 Node 线，加载同一批原生模块还会撞
/// `NODE_MODULE_VERSION` 不一致。
pub fn command_with_path_dirs<S: AsRef<OsStr>>(program: S, extra_path_dirs: &[&Path]) -> Command {
    let mut cmd = Command::new(program);
    cmd.env(
        "PATH",
        merge_extra_path(crate::env::merged_path(), extra_path_dirs),
    );
    cmd
}

/// Windows 上该不该把可执行文件交给 `%ComSpec% /C` 执行。
///
/// 判据是「CreateProcess 能不能直接拉起它」，因此只有**已知的真实二进制
/// 后缀**（`.exe` / `.com`）走直接执行；`.cmd` / `.bat` 是批处理，必须
/// 经命令解释器，其余一切形态（空后缀也算）沿用旧行为走 `%ComSpec% /C`。
/// 这样修的是「`.exe` 被误交给 cmd 拼命令行」这一个明确缺陷，不额外改变
/// 别的形态此前侥幸能跑的路径。非 Windows 平台上没有这种区分，
/// 一律直接执行。
fn needs_command_shell(exe: &Path) -> bool {
    if !cfg!(windows) {
        return false;
    }
    !exe.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|ext| ext.eq_ignore_ascii_case("exe") || ext.eq_ignore_ascii_case("com"))
}

/// 构造执行 `exe` + `args` 的 [`Command`]：批处理文件走 `%ComSpec% /C`，
/// 其余直接执行。
///
/// **绝不能**把真实二进制也交给 `cmd.exe /C` 拼接命令行。`cmd.exe` 不按
/// MSVCRT 规则重新解析 argv，而是直接对 `/C` 之后的整串做分词，只有整串
/// 以 `"` 开头时才会剥掉首尾引号。父进程（Rust 的 `Command`）只在参数
/// 含空格时加引号，于是 `C:\Program Files\nodejs\node.exe --version`
/// 这样的命令行被切成「命令 `C:\Program` + 参数 `Files\nodejs\node.exe`
/// --version」，cmd 报 `'C:\Program' is not recognized as an internal or
/// external command` 并以退出码 1 结束，目标程序从未运行。Windows 上 Node
/// 默认装在 `C:\Program Files\nodejs`，因此「node 装在默认位置」这一最
/// 常见的情况下，凡是经此路径启动的 node 都会静默失败：pnpm 安装内核正常
/// 完成，随后安装后的原生模块探针（`kernel::smoke_load_native_modules`）
/// 以退出码 1 告败且日志里没有任何 `PROBE-` 行——因为 node 根本没跑。
pub(crate) fn command_through_shell_if_needed(exe: &Path, args: &[&str]) -> Command {
    #[cfg(windows)]
    if needs_command_shell(exe) {
        let comspec = std::env::var("ComSpec").unwrap_or_else(|_| "cmd.exe".into());
        let mut cmd = Command::new(comspec);
        cmd.arg("/C").arg(exe).args(args);
        return cmd;
    }
    let mut cmd = Command::new(exe);
    cmd.args(args);
    cmd
}

/// 收集一次性脚本工具（`npm config …` 等）的输出，这类工具的可执行文件
/// 可能是 `.cmd` 批处理 shim。Windows 上 CreateProcess 无法直接执行批处理
/// 文件，所以这类 shim 走 `%ComSpec% /C`；真实二进制（`.exe`）一律直接
/// 执行（原因见 [`command_through_shell_if_needed`]）。子进程继承合并后的
/// PATH，并将 `extra_path_dirs` 前置，让脚本的 `#!/usr/bin/env node` 解析
/// 能找到调用方已校验的 node，即便是在只有系统 PATH 的 GUI 壳中。
pub fn script_capture(
    exe: &Path,
    args: &[&str],
    cwd: &Path,
    extra_path_dirs: &[&Path],
) -> io::Result<(bool, String, String)> {
    let path = merge_extra_path(crate::env::merged_path(), extra_path_dirs);
    let label = exe.to_string_lossy().into_owned();
    #[cfg(windows)]
    {
        let mut cmd = command_through_shell_if_needed(exe, args);
        cmd.current_dir(cwd);
        cmd.env("PATH", path);
        run_command_capture(cmd, &label)
    }
    #[cfg(not(windows))]
    {
        let mut cmd = Command::new(exe);
        cmd.args(args);
        cmd.current_dir(cwd);
        cmd.env("PATH", path);
        run_command_capture(cmd, &label)
    }
}

/// 启动一个长时间运行的子进程（`pnpm`、`npm` 等），把每一条 stdout 与
/// stderr 行同时流式输出到 `log_path` 和 `on_progress`，进程退出后返回。
///
/// Windows 上无法直接 spawn `.cmd` 文件，因此会走 command shell；
/// 其他平台直接运行可执行文件。每个输出流都在独立线程上 drain，
/// 使任何一边的 OS 管道缓冲区满都不会死锁另一边；行数据通过 channel
/// 回传到这个线程，由它独占地调用 `on_progress`。当子进程在解析
/// 依赖图或与 npm registry 通信时静默数十秒，心跳会让调用方随时知道
/// 进度。
///
/// `extra_path_dirs` 在子进程运行任何东西之前，把列出的目录前置到
/// 继承的 `PATH`。macOS `.app` bundle 从 launchd 环境启动，其 `PATH`
/// 仅 `/usr/bin:/bin:/usr/sbin:/sbin`；因此通过 Homebrew 或 nvm
/// 安装 Node 和 pnpm 的用户，这些工具都在 PATH 之外；调用 Node
/// shebang 脚本（`tsdown`、`tsc`、`node ./foo.js` 等）的子进程会
/// 因此以 `env: node: No such file or directory` 退出，即便父进程
/// 自己能找到这两个可执行文件。前置 `pnpm_exe.parent()`（以及
/// 调用方持有的 `node_dir`）能让子进程看到父进程使用的同一个 `node`。
///
/// 日志文件的父目录在缺失时会创建：全新 data dir 上的首次安装会在
/// 其他任何东西创建日志目录之前就进入本助手，直接 `open` 会以
/// `NotFound` 失败（Windows 上为 `系统找不到指定的路径 (os error 3)`）。
const MAX_OUTPUT_LINE_BYTES: usize = 64 * 1024;
const OUTPUT_QUEUE_CAPACITY: usize = 256;

fn read_capped_line<R: BufRead>(
    reader: &mut R,
    buffer: &mut Vec<u8>,
) -> io::Result<Option<String>> {
    buffer.clear();
    let mut truncated = false;
    loop {
        let chunk = reader.fill_buf()?;
        if chunk.is_empty() {
            if buffer.is_empty() {
                return Ok(None);
            }
            break;
        }
        let newline = chunk.iter().position(|byte| *byte == b'\n');
        let consumed = newline.map_or(chunk.len(), |index| index + 1);
        if buffer.len() < MAX_OUTPUT_LINE_BYTES {
            let available = MAX_OUTPUT_LINE_BYTES - buffer.len();
            let copied = consumed.min(available);
            buffer.extend_from_slice(&chunk[..copied]);
            truncated |= copied < consumed;
        } else {
            truncated = true;
        }
        reader.consume(consumed);
        if newline.is_some() {
            break;
        }
    }
    let mut line = String::from_utf8_lossy(buffer).into_owned();
    if line.ends_with('\n') {
        line.pop();
        if line.ends_with('\r') {
            line.pop();
        }
    }
    if truncated {
        line.push_str("… [输出行已截断]");
    }
    Ok(Some(line))
}

const KERNEL_LOG_MAX_BYTES: u64 = 8 * 1024 * 1024;
const KERNEL_LOG_BACKUPS: u8 = 2;
/// 按日轮转路径中故意不使用 `BufWriter`。带缓冲的写入器在两次 flush 之间
/// 会保留最多 8 KiB 在内存里，这会妨碍事件排查期间的实时 tail，并在内核
/// panic 时可能丢失最近的日志行。OS 文件写入本身就内部批量化了少量写入，
/// 因此按行 `write_all` + `flush_all` 既能让磁盘上的视图保持最新，又
/// 不会带来可观的成本。
const NO_BUF_FLUSH: bool = true;

/// 写入每个日志文件名的构建类型前缀。release 与 dev 构建的壳已经位于
/// 不同数据目录（`desktop/` 与 `desktop-dev/`），但同时在文件名上也
/// 打上前缀，意味着——无论是文件管理器列出的 logs 目录，还是从
/// `~/.dsh/desktop*/logs/` 取出并发送给支持的 tar 包——即使不参考
/// 父路径也能一目了然。
pub const LOG_KIND_RELEASE: &str = "release";
pub const LOG_KIND_DEV: &str = "dev";

/// 解析用于日志文件名 stamp 的构建类型。与 `kernel::data_dir` 中的
/// `SHELL_SUBDIR_*` 划分相对应，保证目录布局与文件名 stamp 在「来自
/// 哪个构建」上保持一致。
pub fn build_log_kind() -> &'static str {
    if cfg!(debug_assertions) {
        LOG_KIND_DEV
    } else {
        LOG_KIND_RELEASE
    }
}

/// 将 `SystemTime` 格式化为日志文件名中使用的本地日期戳 `YYYY-MM-DD`。
/// `time` crate 的默认 features 包含 `local-offset`，转换使用用户所在时区——
/// UTC 日期会在用户感知的本地时间的不同时刻翻转日志，把同一个用户日
/// 拆到两个文件里。
pub fn current_date_string() -> String {
    local_date_string(SystemTime::now())
}

fn local_date_string(time: SystemTime) -> String {
    let Ok(duration) = time.duration_since(UNIX_EPOCH) else {
        return String::from("1970-01-01");
    };
    let Ok(datetime) = OffsetDateTime::from_unix_timestamp(duration.as_secs() as i64) else {
        return String::from("1970-01-01");
    };
    let local =
        datetime.to_offset(time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC));
    const DATE_FORMAT: &[FormatItem<'static>] = format_description!("[year]-[month]-[day]");
    local
        .format(&DATE_FORMAT)
        .unwrap_or_else(|_| String::from("1970-01-01"))
}

/// 为指定构建类型与本地日期下的具名日志拼装日志文件名。集中在此，
/// 让所有调用方（内核日志、安装日志、插件日志……）都遵循同一格式，
/// 这也是 `list_log_files` 与弹窗标签列表对用户保持稳定的根本。
pub fn log_file_name(kind: &str, name: &str, date: &str) -> String {
    format!("{}-{}-{}.log", kind, name, date)
}

/// 日志目录的保留窗口：超过 `LOG_RETENTION_DAYS` 天、或总量超过
/// `LOG_RETENTION_BYTES` 的日志会在启动时清掉（从最旧的开始）。
///
/// 每个「日期 × kind」最多留 `KERNEL_LOG_BACKUPS + 1` 代 × 8 MiB，但**日期
/// 只增不减**：每天最多新增 24 MiB，长期使用会累积到 GB 级（P2-62）。这里按
/// "先看天数、再看总量"的顺序裁剪，`LOG_RETENTION_GRACE` 内的文件永不删除
/// —— 那可能是正在写入的当次会话日志。
const LOG_RETENTION_DAYS: u64 = 30;
const LOG_RETENTION_BYTES: u64 = 200 * 1024 * 1024;
const LOG_RETENTION_GRACE: Duration = Duration::from_secs(600);

/// 启动时裁剪过期的 Shell 日志，返回删除的文件数。
pub fn prune_old_logs(logs_dir: &Path) -> usize {
    let Ok(entries) = fs::read_dir(logs_dir) else {
        return 0;
    };
    let mut candidates: Vec<(PathBuf, SystemTime, u64)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("log") {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        candidates.push((path, modified, meta.len()));
    }
    // 新的在前：裁剪时从尾部（最旧）开始丢。
    candidates.sort_by_key(|(_, modified, _)| std::cmp::Reverse(*modified));

    let now = SystemTime::now();
    let mut kept_bytes = 0u64;
    let mut removed = 0usize;
    for (path, modified, size) in candidates {
        let age = now.duration_since(modified).unwrap_or_default();
        if age < LOG_RETENTION_GRACE {
            // 刚写过的文件（多半是当前会话的日志）不参与裁剪，但仍计入总量。
            kept_bytes = kept_bytes.saturating_add(size);
            continue;
        }
        let expired = age > Duration::from_secs(LOG_RETENTION_DAYS * 24 * 60 * 60);
        let over_budget = kept_bytes.saturating_add(size) > LOG_RETENTION_BYTES;
        if expired || over_budget {
            if fs::remove_file(&path).is_ok() {
                removed += 1;
            }
            continue;
        }
        kept_bytes = kept_bytes.saturating_add(size);
    }
    removed
}

/// 把旧命名的轮转备份（`<base>.log.<n>`）改名为新命名（`<base>.<n>.log`）。
///
/// 旧实现往文件名末尾追加代次，扩展名因此变成 `1`，`list_log_files` 的扩展名
/// 过滤会把它们全部挡在面板之外 —— 也就是说 0.1.2-rc.18 及更早的壳轮转出去的
/// 历史日志永远看不到。启动时做一次性改名即可把那部分内容找回来。
///
/// 只在目标不存在时改名：升级后又发生过一次轮转的情况里，新命名的那份才是
/// 更新的内容，不能覆盖。所有错误静默跳过 —— 日志归档整理失败不该阻止启动。
pub fn migrate_legacy_rotated_logs(logs_dir: &Path) {
    let Ok(entries) = fs::read_dir(logs_dir) else {
        return;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let name = entry.file_name().to_string_lossy().into_owned();
        // 形如 `release-kernel-2026-09-10.log.1`：以 `.log.<数字>` 结尾。
        let Some((base, generation)) = name.rsplit_once(".log.") else {
            continue;
        };
        if base.is_empty()
            || generation.is_empty()
            || !generation.bytes().all(|b| b.is_ascii_digit())
        {
            continue;
        }
        let target = logs_dir.join(format!("{base}.{generation}.log"));
        if target.exists() {
            continue;
        }
        let _ = fs::rename(entry.path(), target);
    }
}

/// 轮转备份的路径：`<kind>-<name>-<date>.log` → `<kind>-<name>-<date>.<index>.log`。
///
/// 代次**必须**插在扩展名之前：`list_log_files` 按 `.log` 扩展名收文件，
/// 而日志面板读取也要求 `read_log_file` 拿到一个纯文件名。旧实现直接往
/// 末尾追加（`X.log.1`）会把扩展名变成 `1`，于是被轮转出去的历史日志
/// 在面板里完全不可见 —— 而它们恰好是事故排查时最需要的那几 MB。
fn rotated_log_path(path: &Path, index: u8) -> PathBuf {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "log".to_string());
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("log")
        .to_string();
    path.with_file_name(format!("{stem}.{index}.{extension}"))
}

fn rotate_existing_log(path: &Path) -> io::Result<()> {
    for index in (1..=KERNEL_LOG_BACKUPS).rev() {
        let source = if index == 1 {
            path.to_path_buf()
        } else {
            rotated_log_path(path, index - 1)
        };
        let destination = rotated_log_path(path, index);
        if source.exists() {
            match fs::remove_file(&destination) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
            fs::rename(source, destination)?;
        }
    }
    Ok(())
}

/// 轮转写入器消费的具名日志 spec。`name` 是逻辑标识（`kernel`、`install`、
/// `plugin-wiring` 等），与构建类型和日期一起嵌入文件名。构建类型由
/// 调用方预先计算（通常是 `build_log_kind()`），便于单一测试或 CLI 工具
/// 以另一种构建类型打 stamp。
#[derive(Debug, Clone)]
pub struct LogSpec {
    pub kind: String,
    pub name: String,
}

impl LogSpec {
    pub fn new(kind: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            name: name.into(),
        }
    }

    /// 把该 spec 在给定本地日期、给定日志目录下应当写入的文件路径解析出来。
    /// `list_log_files` 用它来命名标签页，需要向用户展示路径的调用方
    /// （如 `read_log_file` 的 tail 选择器）也会用到。
    pub fn path_for(&self, logs_dir: &Path, date: &str) -> PathBuf {
        logs_dir.join(log_file_name(&self.kind, &self.name, date))
    }
}

/// 日志写入器模式：dated（构建类型 + 名称 + 日期戳，本地午夜滚动），
/// 或 fixed-path（调用方指定的精确路径，例如位于插件目录内、卸载
/// 时会被清理的 per-plugin 构建日志）。
#[derive(Debug, Clone)]
enum LogMode {
    Dated { logs_dir: PathBuf, spec: LogSpec },
    Fixed { path: PathBuf },
}

/// 按日轮转、实时 flush 的日志写入器。每个具名日志（`kernel`、
/// `install-<version>`、`plugin-<id>` 等）一个实例；多个 `RotatingLog`
/// 可以共享同一个 `logs_dir`，因为每个文件名都带唯一的构建类型、名称、
/// 日期戳。
///
/// 轮转策略：
/// - 当天始终写入 `<logs_dir>/<kind>-<name>-<date>.log`。
/// - 跨日写入时，写入器关闭昨天的文件并打开新文件（昨天的文件保留
///   在原位；列表视图与弹窗标签列表按需轮转）。
/// - 当当天文件超过 `KERNEL_LOG_MAX_BYTES` 时，原文件重命名为 `<...>.1`
///   并打开新文件，每个日期最多保留 `KERNEL_LOG_BACKUPS + 1` 代。
/// - `Fixed` 模式完全跳过日期与构建类型轮转，使用调用方的精确路径；
///   大小上限仍会触发轮转。
pub(crate) struct RotatingLog {
    mode: LogMode,
    current_date: String,
    current_path: PathBuf,
    writer: Option<fs::File>,
    bytes: u64,
}

impl RotatingLog {
    pub(crate) fn new(logs_dir: &Path, spec: LogSpec) -> io::Result<Self> {
        fs::create_dir_all(logs_dir)?;
        let mut log = Self {
            mode: LogMode::Dated {
                logs_dir: logs_dir.to_path_buf(),
                spec,
            },
            current_date: String::new(),
            current_path: PathBuf::new(),
            writer: None,
            bytes: 0,
        };
        log.open_for_today()?;
        Ok(log)
    }

    /// 构造一个固定到特定路径的写入器。不进行日期或构建类型轮转；
    /// 大小轮转仍然生效，防止失控的日志超出磁盘配额。供调用方完全拥有
    /// 的一次性脚本（如 per-plugin 构建日志）使用。
    fn new_at_path(path: &Path) -> io::Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut log = Self {
            mode: LogMode::Fixed {
                path: path.to_path_buf(),
            },
            current_date: local_date_string(SystemTime::now()),
            current_path: path.to_path_buf(),
            writer: None,
            bytes: 0,
        };
        log.open_for_today()?;
        Ok(log)
    }

    /// 为当天的日志文件打开（或重新打开）写入器。幂等：相同日期且
    /// 未达大小上限的调用是廉价的 no-op。新文件以追加（而非截断）方式
    /// 打开，让一个新的实例接入已经打开的当天日志时能保留历史。
    fn open_for_today(&mut self) -> io::Result<()> {
        let path = self.resolve_path();
        let same_path = self.current_path == path;
        if same_path && self.bytes < KERNEL_LOG_MAX_BYTES && self.writer.is_some() {
            return Ok(());
        }
        // 先释放上一个写入器；Drop 会关闭 FD，下一次 open 即可使用
        // 同一路径而不会在 Unix 上出现 inotify 式的「text file busy」
        // 小故障。
        if let Some(_writer) = self.writer.take() {
            // 在循环下一次迭代中显式 drop
        }
        // 大小上限：重新打开之前先把昨日同一天的同名文件轮转掉，
        // 让新文件以空开始。日期变化时由于 `path` 与 `self.current_path`
        // 不同，仍会落到全新文件。
        if same_path {
            if let Ok(meta) = fs::metadata(&path) {
                if meta.len() >= KERNEL_LOG_MAX_BYTES {
                    rotate_existing_log(&path)?;
                }
            }
        }
        let file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        // 捕获 open 之后的大小，使已经开了数小时的长时间写入器看到
        // 一个真实的字节数。
        let size = file.metadata().map(|m| m.len()).unwrap_or(0);
        self.current_path = path;
        self.current_date = local_date_string(SystemTime::now());
        self.writer = Some(file);
        self.bytes = size;
        Ok(())
    }

    fn resolve_path(&self) -> PathBuf {
        match &self.mode {
            LogMode::Dated { logs_dir, spec } => {
                let today = local_date_string(SystemTime::now());
                spec.path_for(logs_dir, &today)
            }
            LogMode::Fixed { path } => path.clone(),
        }
    }

    pub(crate) fn write_line(&mut self, line: &str) -> io::Result<()> {
        // 先处理日期滚动（仅 dated 模式）：跨午夜运行的内核必须
        // 先进入新一天的文件，再做大小检查，以保证午夜前的数据
        // 落在正确的文件里。Fixed 模式从不按日期轮转。
        let needed = line.len() as u64 + 1;
        let date_rolled = matches!(self.mode, LogMode::Dated { .. })
            && self.current_date != local_date_string(SystemTime::now());
        if date_rolled || self.bytes.saturating_add(needed) > KERNEL_LOG_MAX_BYTES {
            self.open_for_today()?;
        }
        let writer = self.writer.as_mut().expect("rotating log writer missing");
        writer.write_all(line.as_bytes())?;
        writer.write_all(b"\n")?;
        // 实时 flush：每一行在下一个文件系统调用时就已经落盘。
        // 加缓冲只会徒增成本，并可能在内核 panic / SIGKILL 时丢失
        // 最近几行——而这恰恰是用户报告需要在日志里立刻看到的情形。
        if NO_BUF_FLUSH {
            writer.flush()?;
        }
        self.bytes = self.bytes.saturating_add(needed);
        Ok(())
    }

    fn flush(&mut self) -> io::Result<()> {
        if let Some(writer) = self.writer.as_mut() {
            writer.flush()?;
        }
        Ok(())
    }
}

/// 最近一次日志写入失败的说明（读走即清），供诊断/事故面板引用。
static LOG_WRITE_ERROR: Mutex<Option<String>> = Mutex::new(None);

/// 取走"最近一次日志写入失败"的说明。`None` 表示当前没有已知的写入故障。
pub fn take_log_write_error() -> Option<String> {
    LOG_WRITE_ERROR.lock().ok().and_then(|mut slot| slot.take())
}

/// 记录一次日志写入失败。只保留第一条，避免同类错误刷屏，同时把诊断打到
/// stderr —— 面板里看不到"日志本身坏了"，终端是唯一能立刻看到的地方。
fn record_log_write_error(error: &io::Error) {
    let message = format!("日志写入失败：{error}");
    match LOG_WRITE_ERROR.lock() {
        Ok(mut slot) => {
            if slot.is_none() {
                eprintln!("dsh-xlink: {message}（仍会继续排空内核输出，但后续内容不会落盘）");
                *slot = Some(message);
            }
        }
        Err(_) => eprintln!("dsh-xlink: {message}"),
    }
}

/// 排空一个流，把每一行交给 `write`。
///
/// **写入失败绝不能中断排空**：一旦停止读取，子进程的管道会被填满，内核就
/// 卡在写日志上（结果比丢日志严重得多）。旧实现在第一次写入出错时 `break`，
/// 于是会话中途日志静默死亡，而事故面板仍然引用那个日志路径（P2-3）。
///
/// 返回实际读到的行数，便于诊断与测试。
fn drain_stream<R: Read, F: FnMut(&str) -> io::Result<()>>(
    stream: R,
    mut write: F,
) -> io::Result<u64> {
    let mut reader = BufReader::new(stream);
    let mut buffer = Vec::with_capacity(MAX_OUTPUT_LINE_BYTES);
    let mut lines = 0u64;
    let mut failing = false;
    loop {
        match read_capped_line(&mut reader, &mut buffer) {
            Ok(Some(line)) => {
                lines += 1;
                match write(&line) {
                    Ok(()) => failing = false,
                    Err(error) => {
                        if !failing {
                            record_log_write_error(&error);
                            failing = true;
                        }
                    }
                }
            }
            Ok(None) => return Ok(lines),
            Err(error) => return Err(error),
        }
    }
}

fn spawn_log_drain<R: Read + Send + 'static>(stream: R, logger: Arc<Mutex<RotatingLog>>) {
    std::thread::spawn(move || {
        let _ = drain_stream(stream, |line| match logger.lock() {
            Ok(mut log) => log.write_line(line),
            Err(_) => Err(io::Error::other("日志写入器锁被毒化")),
        });
        if let Ok(mut log) = logger.lock() {
            let _ = log.flush();
        }
    });
}

/// 短工具（`git --version`、`taskkill`、`lsof`…）的默认上限。网络操作要显式
/// 传更长的值，见 `run_command_capture_with_timeout`。
const RUN_CAPTURE_TIMEOUT: Duration = Duration::from_secs(30);
/// `git clone` 的上限：浅克隆在慢网络或大仓库下远超 30 秒，而它恰恰是很多
/// 插件（GitHub Release 不可用时）的主安装路径。
pub const GIT_CLONE_TIMEOUT: Duration = Duration::from_secs(600);
const RUN_CAPTURE_MAX_BYTES: usize = 4 * 1024 * 1024;
const RUN_CAPTURE_READER_GRACE: Duration = Duration::from_millis(500);

fn isolate_process(cmd: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
}

pub(crate) fn terminate_process_tree(child: &mut Child) {
    #[cfg(unix)]
    {
        let pgid = child.id() as i32;
        unsafe {
            libc::kill(-pgid, libc::SIGTERM);
        }
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            if child.try_wait().ok().flatten().is_some() || Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        unsafe {
            libc::kill(-pgid, libc::SIGKILL);
        }
        let _ = child.wait();
    }
    #[cfg(windows)]
    {
        let mut cmd = command_with_path("taskkill");
        cmd.args(["/PID", &child.id().to_string(), "/T", "/F"]);
        let _ = quiet(&mut cmd).status();
        let _ = child.wait();
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = child.kill();
        let _ = child.wait();
    }
}

fn spawn_capture_reader<R: Read + Send + 'static>(
    stream: R,
    max_bytes: usize,
) -> mpsc::Receiver<io::Result<Vec<u8>>> {
    let (tx, rx) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let _ = tx.send(read_bounded_bytes(stream, max_bytes));
    });
    rx
}

fn abandon_capture_reader(rx: &mpsc::Receiver<io::Result<Vec<u8>>>) {
    let _ = rx.recv_timeout(RUN_CAPTURE_READER_GRACE);
}

fn wait_capture_reader(
    rx: &mpsc::Receiver<io::Result<Vec<u8>>>,
    deadline: Instant,
) -> io::Result<Vec<u8>> {
    match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "captured output pipe did not close before the deadline",
        )),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err(io::Error::other("capture reader disconnected"))
        }
    }
}

fn run_capture_command_bytes(cmd: Command, label: &str) -> io::Result<(bool, Vec<u8>, Vec<u8>)> {
    run_capture_command_bytes_with_timeout(cmd, label, RUN_CAPTURE_TIMEOUT)
}

/// 与 [`run_capture_command_bytes`] 相同，但使用调用方给定的超时。
///
/// 30 秒的默认值是为 `git --version`、`taskkill`、`lsof` 这类短工具准备的；
/// `git clone` 是**网络**操作，慢网络或大仓库下 30 秒必然超时——而很多 dsh
/// 插件的 GitHub Release 不可用，clone 恰恰是主路径。传一个长超时可以避免把
/// 一次正常的浅克隆变成"无法运行 git"。
fn run_capture_command_bytes_with_timeout(
    mut cmd: Command,
    label: &str,
    timeout: Duration,
) -> io::Result<(bool, Vec<u8>, Vec<u8>)> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    isolate_process(&mut cmd);
    let mut child = quiet(&mut cmd).spawn()?;
    let stdout = child.stdout.take().expect("capture stdout was piped");
    let stderr = child.stderr.take().expect("capture stderr was piped");
    let stdout_reader = spawn_capture_reader(stdout, RUN_CAPTURE_MAX_BYTES);
    let stderr_reader = spawn_capture_reader(stderr, RUN_CAPTURE_MAX_BYTES);

    let started = Instant::now();
    let deadline = started + timeout;
    let mut stdout_capture = None;
    let mut stderr_capture = None;
    let status = loop {
        if stdout_capture.is_none() {
            match stdout_reader.try_recv() {
                Ok(result) => {
                    if let Err(error) = &result {
                        terminate_process_tree(&mut child);
                        abandon_capture_reader(&stderr_reader);
                        return Err(io::Error::new(error.kind(), error.to_string()));
                    }
                    stdout_capture = Some(result);
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {}
            }
        }
        if stderr_capture.is_none() {
            match stderr_reader.try_recv() {
                Ok(result) => {
                    if let Err(error) = &result {
                        terminate_process_tree(&mut child);
                        abandon_capture_reader(&stdout_reader);
                        return Err(io::Error::new(error.kind(), error.to_string()));
                    }
                    stderr_capture = Some(result);
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {}
            }
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    terminate_process_tree(&mut child);
                    abandon_capture_reader(&stdout_reader);
                    abandon_capture_reader(&stderr_reader);
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        format!("{label} timed out after {} seconds", timeout.as_secs()),
                    ));
                }
                std::thread::sleep(remaining.min(Duration::from_millis(50)));
            }
            Err(error) => {
                terminate_process_tree(&mut child);
                abandon_capture_reader(&stdout_reader);
                abandon_capture_reader(&stderr_reader);
                return Err(error);
            }
        }
    };

    let stdout = match match stdout_capture {
        Some(result) => result,
        None => wait_capture_reader(&stdout_reader, deadline),
    } {
        Ok(output) => output,
        Err(error) => {
            terminate_process_tree(&mut child);
            abandon_capture_reader(&stderr_reader);
            return Err(error);
        }
    };
    let stderr = match match stderr_capture {
        Some(result) => result,
        None => wait_capture_reader(&stderr_reader, deadline),
    } {
        Ok(output) => output,
        Err(error) => {
            terminate_process_tree(&mut child);
            return Err(error);
        }
    };
    Ok((status.success(), stdout, stderr))
}

fn run_capture_bytes(program: &str, args: &[&str]) -> io::Result<(bool, Vec<u8>, Vec<u8>)> {
    let mut cmd = command_with_path(program);
    cmd.args(args);
    run_capture_command_bytes(cmd, program)
}

/// 运行一个短生命周期的外部工具，捕获限定大小的 stdout/stderr。
///
/// 子进程在 Unix 上被隔离到独立进程组，输出管道并发 drain。超时
/// 杀掉整个进程组，永远不会因某个读者持有的管道可能被后代占用而
/// 无限等待。
pub fn run_capture_output(program: &str, args: &[&str]) -> io::Result<(bool, String, String)> {
    let (success, stdout, stderr) = run_capture_bytes(program, args)?;
    Ok((
        success,
        String::from_utf8_lossy(&stdout).into_owned(),
        String::from_utf8_lossy(&stderr).into_owned(),
    ))
}

/// 与 [`run_command_capture`] 相同，但使用调用方给定的超时。
///
/// 供 `git clone` 这类网络操作使用：默认的 30 秒对它们来说必然不够。
pub fn run_command_capture_with_timeout(
    cmd: Command,
    label: &str,
    timeout: Duration,
) -> io::Result<(bool, String, String)> {
    let (success, stdout, stderr) = run_capture_command_bytes_with_timeout(cmd, label, timeout)?;
    Ok((
        success,
        String::from_utf8_lossy(&stdout).into_owned(),
        String::from_utf8_lossy(&stderr).into_owned(),
    ))
}

pub fn run_command_capture(cmd: Command, label: &str) -> io::Result<(bool, String, String)> {
    let (success, stdout, stderr) = run_capture_command_bytes(cmd, label)?;
    Ok((
        success,
        String::from_utf8_lossy(&stdout).into_owned(),
        String::from_utf8_lossy(&stderr).into_owned(),
    ))
}

pub fn run_capture(program: &str, args: &[&str]) -> io::Result<(bool, String)> {
    let (success, stdout, _) = run_capture_output(program, args)?;
    Ok((success, stdout))
}

fn read_bounded_bytes<R: Read>(mut reader: R, max_bytes: usize) -> io::Result<Vec<u8>> {
    let mut output = Vec::with_capacity(max_bytes.min(8192));
    let mut buffer = [0u8; 8192];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        if output.len() >= max_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("captured output exceeded {max_bytes} bytes"),
            ));
        }
        let copied = count.min(max_bytes - output.len());
        output.extend_from_slice(&buffer[..copied]);
        if copied < count {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("captured output exceeded {max_bytes} bytes"),
            ));
        }
    }
    Ok(output)
}

/// 给长时间运行的子进程附加有界的后台日志 drain。调用方保留子进程
/// 的所有权，由分离的 reader 维持其管道流动并轮转日志，内存中不
/// 滞留输出。
///
/// drain 出的输出通过按日轮转的 `RotatingLog` 写入，append 到当天的
/// `<kind>-<name>-<date>.log`，本地午夜滚动到新文件，因此 tail 中的
/// `read_log_file` 与手动的 `tail -F` 都能看到正在进行的启动过程，
/// 又不会丢掉午夜前的历史。`log_spec` 携带逻辑名（`kernel`、
/// `install-<version>` 等）以及构建类型戳；具体文件路径由写入器
/// 内部解析。
pub fn attach_log_drainers(
    child: &mut Child,
    logs_dir: &Path,
    log_spec: &LogSpec,
) -> io::Result<()> {
    let logger = Arc::new(Mutex::new(RotatingLog::new(logs_dir, log_spec.clone())?));
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "child stdout was not piped"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "child stderr was not piped"))?;
    spawn_log_drain(stdout, Arc::clone(&logger));
    spawn_log_drain(stderr, logger);
    Ok(())
}

/// 把长时间运行子进程合并后的 stdout+stderr 同时流式输出到 `on_progress`
/// 与 `logs_dir` 下按日轮转的日志文件，进程退出后返回。drain 出的输出
/// 走与 `attach_log_drainers` 相同的、按日期 + 构建类型戳的写入器，
/// 因此跨午夜的安装流程会落到两个文件，不需要额外代码路径。每一行
/// 在下一行被接受前都已 flush，IPC 通道上的进度与磁盘上的进度始终
/// 一致。
pub fn run_with_progress(
    exe: &Path,
    args: &[&str],
    cwd: &Path,
    logs_dir: &Path,
    log_spec: &LogSpec,
    extra_path_dirs: &[&Path],
    on_progress: impl FnMut(&str),
) -> io::Result<ExitStatus> {
    let log = RotatingLog::new(logs_dir, log_spec.clone())?;
    run_with_progress_log(exe, args, cwd, log, extra_path_dirs, on_progress)
}

/// `run_with_progress` 的路径固定版本。完整输出原样 append 到 `log_path`——
/// 不打构建类型戳，也不按日轮转。用于调用方完全拥有的一次性脚本
/// （per-plugin 构建日志等）。每一行在下一行被接受前也已 flush，
/// IPC 通道上的进度与磁盘上的进度始终一致。
pub fn run_with_progress_at(
    exe: &Path,
    args: &[&str],
    cwd: &Path,
    log_path: &Path,
    extra_path_dirs: &[&Path],
    on_progress: impl FnMut(&str),
) -> io::Result<ExitStatus> {
    let log = RotatingLog::new_at_path(log_path)?;
    run_with_progress_log(exe, args, cwd, log, extra_path_dirs, on_progress)
}

/// `run_with_progress` 与 `run_with_progress_at` 的共享实现。`RotatingLog`
/// 参数决定输出是落到 dated 文件（带构建类型戳）还是调用方固定的精确路径。
fn run_with_progress_log(
    exe: &Path,
    args: &[&str],
    cwd: &Path,
    mut log: RotatingLog,
    extra_path_dirs: &[&Path],
    mut on_progress: impl FnMut(&str),
) -> io::Result<ExitStatus> {
    let mut child = spawn(exe, args, cwd, extra_path_dirs)?;
    let stdout = child.stdout.take().expect("child stdout was piped");
    let stderr = child.stderr.take().expect("child stderr was piped");

    let (tx, rx) = mpsc::sync_channel::<String>(OUTPUT_QUEUE_CAPACITY);
    let tx_err = tx.clone();
    let drain_stdout = std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut buffer = Vec::with_capacity(MAX_OUTPUT_LINE_BYTES);
        while let Ok(Some(line)) = read_capped_line(&mut reader, &mut buffer) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let drain_stderr = std::thread::spawn(move || {
        let mut reader = BufReader::new(stderr);
        let mut buffer = Vec::with_capacity(MAX_OUTPUT_LINE_BYTES);
        while let Ok(Some(line)) = read_capped_line(&mut reader, &mut buffer) {
            if tx_err.send(line).is_err() {
                break;
            }
        }
    });

    const HEARTBEAT_SECS: u64 = 10;
    const RUN_PROGRESS_TIMEOUT: Duration = Duration::from_secs(30 * 60);
    // 子进程退出后等 drain 线程收尾的宽限。孙进程可能继承了 stdout/stderr 并
    // 一直持有管道（pnpm 的生命周期脚本留下后台进程、Windows 上 `cmd /C` 包一层
    // 时尤其常见）：那时 `rx` 永远不会 Disconnected，如果一直等到全局 deadline，
    // 一次**已经成功**的安装会被报成"运行超过 30 分钟"的失败。
    const DRAIN_GRACE: Duration = Duration::from_secs(5);
    // 轮询周期：只用来及时发现"子进程已经退出"。心跳消息另按
    // `HEARTBEAT_SECS` 节流，因此缩短轮询不会让 UI 收到更密的进度。
    const POLL_INTERVAL: Duration = Duration::from_millis(500);
    let started = Instant::now();
    let mut last_heartbeat = Instant::now();
    let deadline = started + RUN_PROGRESS_TIMEOUT;
    let mut child_exited = false;
    let mut output_closed = false;
    let mut timed_out = false;
    let mut output_truncated = false;
    let mut drain_deadline: Option<Instant> = None;
    loop {
        if !child_exited {
            match child.try_wait() {
                Ok(Some(_)) => {
                    child_exited = true;
                    drain_deadline = Some(Instant::now() + DRAIN_GRACE);
                }
                Ok(None) => {}
                Err(error) => {
                    drop(rx);
                    terminate_process_tree(&mut child);
                    return Err(error);
                }
            }
        }
        if child_exited && output_closed {
            break;
        }
        if let Some(grace) = drain_deadline {
            if Instant::now() >= grace {
                output_truncated = true;
                break;
            }
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            timed_out = true;
            break;
        }
        // 在"子进程已退出、只等管道关闭"的阶段只睡到宽限点，不做无意义的心跳。
        let wait = match drain_deadline {
            Some(grace) => grace.saturating_duration_since(Instant::now()),
            None => POLL_INTERVAL,
        };
        match rx.recv_timeout(wait.min(remaining)) {
            Ok(line) => {
                on_progress(line.trim_end());
                let _ = log.write_line(&line);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // 心跳按固定间隔节流：轮询变密是为了尽快发现子进程退出，
                // 而不是往进度面板刷屏。
                if last_heartbeat.elapsed() >= Duration::from_secs(HEARTBEAT_SECS) {
                    last_heartbeat = Instant::now();
                    let secs = started.elapsed().as_secs();
                    on_progress(&format!("… 子进程仍在运行（已进行 {secs} 秒）"));
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                output_closed = true;
            }
        }
    }

    if timed_out {
        drop(rx);
        terminate_process_tree(&mut child);
        let _ = log.flush();
        // 在终止之前就 drop 接收端，使嘈杂的 reader 因 send 失败而退出。
        // 这里不要 join：组外的某个进程可能仍持有继承的管道，命令必须
        // 遵守其 deadline，而不是为那个外部进程永远等待下去。
        drop(drain_stdout);
        drop(drain_stderr);
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            format!(
                "子进程运行超过 {} 分钟",
                RUN_PROGRESS_TIMEOUT.as_secs() / 60
            ),
        ));
    }

    if output_truncated {
        // 进程本身已经正常结束，只是仍有别的进程持有它的输出管道。此时不能
        // join：那会把刚设的宽限又变成无限等待。drain 线程会在管道最终关闭后
        // 自行退出。
        on_progress("子进程已退出；仍有其它进程持有它的输出管道，剩余输出未收录（进程本身已结束）");
    } else {
        let _ = drain_stdout.join();
        let _ = drain_stderr.join();
    }

    let status = reap(child)?;
    log.flush()?;
    Ok(status)
}

fn spawn(exe: &Path, args: &[&str], cwd: &Path, extra_path_dirs: &[&Path]) -> io::Result<Child> {
    // 把每个 spawn 的子进程（pnpm/npm 等等）固定到壳配置的 npm registry，
    // 让镜像选择即便在用户全局 .npmrc 指向别处或项目级 .npmrc 缺失时
    // 仍然可强制生效。`npm_config_registry` 是 pnpm 与 npm 共同视为
    // 最高优先级来源的环境变量。
    let registry = crate::registry::npm_registry_base();
    // Tauri 以 Windows GUI 子系统应用发布，启动时仅继承系统 PATH；
    // user PATH（`npm install -g` 后 `npm` 与 `pnpm` shim 所在）会被丢弃，
    // 除非我们重新 stamp。`env::merged_path` 读取一次 `HKCU\Environment\Path`
    // 并拼接到进程已有的 PATH 上。
    //
    // `extra_path_dirs` 在此之上再叠加一层：调用方提供的目录（已校验的
    // `node` bin 目录、`pnpm_exe.parent()` 使 pnpm 自身的 shim 系列可达……）
    // 按顺序前置，保证任何 Node shebang 子进程都能解析 `node`，即便在
    // macOS .app bundle 上 launchd PATH 只有系统路径。
    let path = merge_extra_path(crate::env::merged_path(), extra_path_dirs);
    #[cfg(windows)]
    {
        let mut cmd = command_through_shell_if_needed(exe, args);
        // GUI 壳以任意的 cwd 启动；子进程必须显式继承一个 cwd，
        // 否则会向上解析最近的 package.json 并装到错误目录。
        cmd.current_dir(cwd);
        cmd.env("PATH", path);
        cmd.env("npm_config_registry", registry);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        isolate_process(&mut cmd);
        quiet(&mut cmd).spawn()
    }
    #[cfg(not(windows))]
    {
        let mut cmd = Command::new(exe);
        cmd.args(args);
        cmd.current_dir(cwd);
        cmd.env("PATH", path);
        cmd.env("npm_config_registry", registry);
        // macOS 上 Apple 命令行工具的 clang 默认只能找到 v1 下的零散 libc++
        // 头文件，缺失 `memory` / `string` 等关键头——fs-ext / node-pty 等
        // 依赖 node-gyp 的原生模块在「pnpm 真正跑构建脚本」路径上会以
        // `fatal error: 'memory' file not found` 失败。CommandLineTools 的
        // `xcrun --show-sdk-path` 在每个 macOS 升级 / Xcode 更新后会切换
        // 版本，强行 stamp 某个具体路径会随 SDK 轮换失效；统一从 `xcrun`
        // 取最新 SDK，并把 libc++ 的 v1 头目录与 SDK 系统头目录拼到
        // `CPLUS_INCLUDE_PATH` / `C_INCLUDE_PATH`。父进程已经设置过的话不
        // 覆盖，让高级用户的特殊配置（如指向完整 Xcode.app 的 SDK）优先。
        apply_macos_toolchain_env(&mut cmd);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        isolate_process(&mut cmd);
        quiet(&mut cmd).spawn()
    }
}

/// 在 macOS 上为子进程补足 node-gyp / clang 所需的 SDK 头目录。仅当父进程
/// 没有显式覆盖相关变量时才填默认，让用户自定义优先。
#[cfg(target_os = "macos")]
fn apply_macos_toolchain_env(cmd: &mut Command) {
    let Some(sdk_root) = detect_macos_sdk_root() else {
        return;
    };
    let libcxx_include = format!("{sdk_root}/usr/include/c++/v1");
    let sys_include = format!("{sdk_root}/usr/include");
    let sdk_root_value: std::ffi::OsString = std::ffi::OsString::from(&sdk_root);
    // 检查父进程是否显式设过；只要 `Command::env` 没在子命令上覆盖，
    // std::env::var_os 看到的就是子进程实际继承到的值，避免给 GUI
    // shell 已经手动 stamp 的高级用户环境硬塞 SDKROOT。
    if std::env::var_os("SDKROOT").is_none() {
        cmd.env("SDKROOT", &sdk_root_value);
    }
    let mut cxx_path = std::env::var("CPLUS_INCLUDE_PATH").unwrap_or_default();
    if !cxx_path.split(':').any(|p| p == libcxx_include) {
        if !cxx_path.is_empty() {
            cxx_path.push(':');
        }
        cxx_path.push_str(&libcxx_include);
        cmd.env("CPLUS_INCLUDE_PATH", &cxx_path);
    }
    let mut c_path = std::env::var("C_INCLUDE_PATH").unwrap_or_default();
    if !c_path.split(':').any(|p| p == sys_include) {
        if !c_path.is_empty() {
            c_path.push(':');
        }
        c_path.push_str(&sys_include);
        cmd.env("C_INCLUDE_PATH", &c_path);
    }
}

#[cfg(target_os = "macos")]
fn detect_macos_sdk_root() -> Option<String> {
    if let Ok(value) = std::env::var("SDKROOT") {
        if !value.is_empty() && std::path::Path::new(&value).is_dir() {
            return Some(value);
        }
    }
    let output = std::process::Command::new("xcrun")
        .args(["--show-sdk-path", "--sdk", "macosx"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8(output.stdout).ok()?.trim().to_string();
    if path.is_empty() {
        None
    } else {
        Some(path)
    }
}

/// 把 `extra` 中的条目前置到 `base` 前。空 / 不存在的条目会被跳过，
/// 这样调用方没有额外目录时没有任何代价。路径分隔符遵循宿主平台
/// （Windows 上为 `;`，其他平台为 `:`）。
fn merge_extra_path(base: &str, extra: &[&Path]) -> String {
    if extra.is_empty() {
        return base.to_string();
    }
    #[cfg(windows)]
    const SEP: char = ';';
    #[cfg(not(windows))]
    const SEP: char = ':';
    let mut out = String::new();
    let mut first = true;
    for dir in extra {
        let Some(text) = dir.to_str() else { continue };
        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !first {
            out.push(SEP);
        }
        out.push_str(trimmed);
        first = false;
    }
    if !first {
        if !base.is_empty() {
            out.push(SEP);
            out.push_str(base);
        }
    } else {
        out.push_str(base);
    }
    out
}

/// 读到空内容后的短重试次数。撞上轮转的瞬间（旧文件被改名、同名新文件刚
/// 建立）会读到 0 字节，而调用方把「空 tail」当作「没有日志证据」——启动
/// 防护在 P1-3 之后仍会用这个证据决定要不要归因。一次 20 ms 后的重读足以
/// 覆盖这个窗口，又不会让「日志确实是空的」多等太久。
const READ_TAIL_ATTEMPTS: u8 = 2;
const READ_TAIL_RETRY_DELAY: Duration = Duration::from_millis(20);

/// 读取文本文件的有界尾部用于展示。缺失或不可读的文件返回空字符串——
/// 调用方在实时状态旁渲染 tail，不应把消失的日志变成错误对话框。
pub(crate) fn read_tail(path: &Path, max_bytes: u64) -> String {
    for attempt in 1..=READ_TAIL_ATTEMPTS {
        let Ok(mut file) = fs::File::open(path) else {
            return String::new();
        };
        // 长度必须取自**已打开的 fd**，而不是路径上的 `fs::metadata`。
        // 先 metadata 再 open 会跨越一次日志轮转（rename + 新建同名文件）：
        // 拿旧长度去 seek 一个更短的新文件，`seek` 到 EOF 之后是合法操作、
        // 读回 0 字节，于是返回空 tail —— 面板显示「日志是空的」，启动防护
        // 也会把「无内容」当成「无归因证据」，正是 P2-8 描述的路径。
        let len = file.metadata().map(|m| m.len()).unwrap_or(0);
        let text = read_tail_from(&mut file, len, max_bytes);
        if !text.is_empty() || attempt == READ_TAIL_ATTEMPTS {
            return text;
        }
        std::thread::sleep(READ_TAIL_RETRY_DELAY);
    }
    String::new()
}

/// 从已打开的句柄读取尾部。`len` 是调用方观察到的长度；若按它算出的起点
/// 一个字节都读不到（文件在我们观察之后被截断或被替换成了更短的文件），
/// 就退回从头读 —— 保证「文件里确实有内容」时不会返回空字符串。
fn read_tail_from(file: &mut fs::File, len: u64, max_bytes: u64) -> String {
    use std::io::{Read, Seek};
    let start = len.saturating_sub(max_bytes);
    // `Vec::with_capacity` 在没有东西把元素类型钉住之前无法推断；
    // 没有这里的类型标注，后续的 `read_to_end(&mut buf)` 需要这条显式提示。
    let mut buf: Vec<u8> = Vec::with_capacity(max_bytes as usize);
    if start > 0 {
        let _ = file.seek(io::SeekFrom::Start(start));
    }
    let _ = file.read_to_end(&mut buf);
    if buf.is_empty() && start > 0 {
        let _ = file.seek(io::SeekFrom::Start(0));
        let _ = file.read_to_end(&mut buf);
    }
    String::from_utf8_lossy(&buf).into_owned()
}

/// Windows：把内核进程收进一个 `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` 的
/// Job Object。
///
/// 壳崩溃、被任务管理器强杀、或用户注销时，Job 句柄随进程一起关闭，系统会把
/// Job 里的所有进程（内核及其派生的 node 子进程）一并终止 —— 因此不会留下
/// 占着端口的孤儿内核（P2-2）。Unix 侧靠 `reap_orphans` 的「cwd == data_dir」
/// 扫描兜底；Windows 没有等价且不需要读 PEB / 不需要管理员权限的手段。
///
/// 句柄必须活到壳退出，所以放进进程级 `OnceLock`：**一旦它被 drop，Job 就会
/// 立刻关闭并杀掉正在运行的内核**。
#[cfg(windows)]
mod job_object {
    // 子模块**不继承**父模块的 `use`：本模块用到的符号必须逐个引入，否则
    // Windows 目标下会报 E0433/E0425（desktop-v0.1.2-rc.19 连续两次 Windows
    // 打包失败都是这一类：先是 `io` / `Child`，后是 `OnceLock`）。
    // `use super::*` 只能覆盖父模块顶层确实导入过的名字 —— process.rs 顶层没有
    // `OnceLock`，所以它必须单独写。
    use super::*;
    use std::os::windows::io::AsRawHandle;
    use std::sync::OnceLock;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    /// `HANDLE` 是裸指针，而 `OnceLock` 要求 `Sync`；Job 句柄只被主线程创建、
    /// 之后只读使用，跨线程共享是安全的。
    struct JobHandle(HANDLE);
    unsafe impl Send for JobHandle {}
    unsafe impl Sync for JobHandle {}

    static KERNEL_JOB: OnceLock<JobHandle> = OnceLock::new();

    fn job_handle() -> io::Result<HANDLE> {
        if let Some(handle) = KERNEL_JOB.get() {
            return Ok(handle.0);
        }
        // SAFETY: 两个空指针分别表示「默认安全属性」与「匿名 Job」，都是
        // CreateJobObjectW 允许的取值；句柄所有权在成功后被 OnceLock 持有，
        // 失败路径就地 CloseHandle，不会泄漏。
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return Err(io::Error::last_os_error());
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let ok = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const core::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if ok == 0 {
                let error = io::Error::last_os_error();
                CloseHandle(job);
                return Err(error);
            }
            let _ = KERNEL_JOB.set(JobHandle(job));
            Ok(job)
        }
    }

    /// 把刚派生出的内核进程加入 Job。失败只记录不致命：孤儿回收退回到「下次
    /// 启动时按 pid 文件 / 端口反查」的既有路径，比让内核起不来强。
    pub(super) fn adopt(child: &Child) {
        let job = match job_handle() {
            Ok(job) => job,
            Err(error) => {
                eprintln!(
                    "dsh-xlink: 无法创建内核 Job Object（{error}）；壳异常退出时可能留下孤儿内核"
                );
                return;
            }
        };
        // SAFETY: `child` 是 std 刚从 CreateProcess 拿到的进程句柄，带有
        // PROCESS_SET_QUOTA | PROCESS_TERMINATE；Job 句柄来自本模块的
        // OnceLock，两者在调用期间都有效。
        let assigned = unsafe { AssignProcessToJobObject(job, child.as_raw_handle() as HANDLE) };
        if assigned == 0 {
            eprintln!(
                "dsh-xlink: 无法把内核进程加入 Job Object（{}）；壳异常退出时可能留下孤儿内核",
                io::Error::last_os_error()
            );
        }
    }
}

/// 把内核进程纳入「随壳一起终止」的机制（目前只有 Windows 的 Job Object 需
/// 要显式动作；Unix 靠 `kernel::reap_orphans` 在下次启动时回收）。
pub(crate) fn adopt_kernel_process(child: &Child) {
    #[cfg(windows)]
    job_object::adopt(child);
    #[cfg(not(windows))]
    let _ = child;
}

fn reap(mut child: Child) -> io::Result<ExitStatus> {
    // Windows 上 `ComSpec /C` 把 cmd.exe 作为直接子进程，真正的程序是它的
    // 孙子进程；在 cmd 上 wait 要等到孙子进程退出才返回，因此各处都使用
    // 朴素的 wait 即可。
    child.wait()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static PROCESS_TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn rotated_backups_keep_the_log_extension() {
        // 备份名必须仍以 `.log` 结尾：`list_log_files` 按扩展名收文件，
        // `read_log_file` 也要求纯文件名。旧实现追加成 `X.log.1` 会让扩展名
        // 变成 `1`，于是轮转出去的历史日志在面板里完全不可见（P2-4）。
        let path = PathBuf::from("/tmp/logs/release-kernel-2026-09-10.log");
        assert_eq!(
            rotated_log_path(&path, 1),
            PathBuf::from("/tmp/logs/release-kernel-2026-09-10.1.log")
        );
        assert_eq!(
            rotated_log_path(&path, 2),
            PathBuf::from("/tmp/logs/release-kernel-2026-09-10.2.log")
        );
        assert_eq!(
            rotated_log_path(&path, 2)
                .extension()
                .and_then(|e| e.to_str()),
            Some("log"),
        );
    }

    #[test]
    fn rotation_keeps_the_two_newest_generations_readable() {
        let dir = temp_dir("rotation");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("release-kernel-2026-09-10.log");

        // 依次写满三代：当前文件应变成 .1.log，原 .1.log 应变成 .2.log。
        fs::write(&path, b"third").unwrap();
        fs::write(rotated_log_path(&path, 1), b"second").unwrap();
        fs::write(rotated_log_path(&path, 2), b"first").unwrap();
        rotate_existing_log(&path).unwrap();

        assert!(!path.exists(), "当前文件应被轮转走");
        assert_eq!(
            fs::read(rotated_log_path(&path, 1)).unwrap(),
            b"third",
            "最新的内容应落在第 1 代备份"
        );
        assert_eq!(
            fs::read(rotated_log_path(&path, 2)).unwrap(),
            b"second",
            "上一代应下移一位"
        );

        // 每个备份都必须仍能被日志面板当普通 `.log` 文件读取。
        for index in 1..=KERNEL_LOG_BACKUPS {
            let backup = rotated_log_path(&path, index);
            assert_eq!(
                backup.extension().and_then(|e| e.to_str()),
                Some("log"),
                "{backup:?} 必须保留 .log 扩展名"
            );
        }
        fs::remove_dir_all(&dir).ok();
    }

    fn temp_dir(label: &str) -> PathBuf {
        let unique = PROCESS_TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!(
            "dsh-xlink-process-{}-{}-{}",
            label,
            std::process::id(),
            unique
        ))
    }

    #[test]
    fn legacy_rotated_logs_are_migrated_without_clobbering_new_ones() {
        let dir = temp_dir("log-migration");
        fs::create_dir_all(&dir).unwrap();

        // 旧命名（扩展名是 `1`，面板看不到）→ 应改名为 `X.1.log`。
        fs::write(dir.join("release-kernel-2026-09-10.log.1"), b"legacy").unwrap();
        fs::write(dir.join("release-kernel-2026-09-09.log.2"), b"legacy2").unwrap();
        // 已经存在新命名的那份：说明升级后又轮转过一次，新命名才是更新的内容。
        fs::write(dir.join("release-kernel-2026-09-10.1.log"), b"new").unwrap();
        // 干扰项：不是轮转备份，不能被改名。
        fs::write(dir.join("release-kernel-2026-09-10.log"), b"current").unwrap();
        fs::write(dir.join("notes.log.bak"), b"other").unwrap();

        migrate_legacy_rotated_logs(&dir);

        assert_eq!(
            fs::read(dir.join("release-kernel-2026-09-10.1.log")).unwrap(),
            b"new",
            "新命名的备份不能被旧文件覆盖"
        );
        // 目标已存在时明知有冲突也不能覆盖，因此旧文件被留在原地（内容不丢，
        // 只是仍不进面板）——这是有意的取舍，不是遗漏。
        assert_eq!(
            fs::read(dir.join("release-kernel-2026-09-10.log.1")).unwrap(),
            b"legacy",
            "冲突时旧文件应原样保留"
        );
        assert_eq!(
            fs::read(dir.join("release-kernel-2026-09-09.2.log")).unwrap(),
            b"legacy2",
            "没有冲突的旧备份应完成改名"
        );
        assert_eq!(
            fs::read(dir.join("release-kernel-2026-09-10.log")).unwrap(),
            b"current",
            "当期日志不受影响"
        );
        assert!(dir.join("notes.log.bak").exists(), "非轮转备份不动");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn legacy_migration_tolerates_a_missing_directory() {
        // 首次启动时 logs 目录可能还不存在：迁移必须静默返回而不是 panic。
        migrate_legacy_rotated_logs(&temp_dir("logs-absent"));
    }

    #[test]
    fn tail_returns_the_bounded_end_of_a_large_file() {
        let dir = temp_dir("tail-large");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("release-kernel-2026-09-10.log");
        let mut content = "x".repeat(200);
        content.push_str("TAIL");
        fs::write(&path, content.as_bytes()).unwrap();

        let tail = read_tail(&path, 4);
        assert_eq!(tail, "TAIL");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn tail_returns_whole_content_for_small_files_and_empty_for_missing() {
        let dir = temp_dir("tail-small");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("release-kernel-2026-09-10.log");
        fs::write(&path, b"short").unwrap();
        assert_eq!(read_tail(&path, 4096), "short");
        assert_eq!(read_tail(&dir.join("gone.log"), 4096), "");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn tail_falls_back_to_start_when_the_file_shrank_after_the_length_was_taken() {
        // P2-8 的竞速，确定性复现：长度先按更大的旧文件取好（轮转前的
        // `metadata().len()`），随后同名路径上已经是一个更短的新文件。
        // 旧实现 seek 到越界位置后读回 0 字节并就此返回 —— 空 tail 会被
        // 启动防护当成「无归因证据」。新实现在读空时退回从头读。
        let dir = temp_dir("tail-race");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("release-kernel-2026-09-10.log");
        fs::write(&path, b"rotation-leftovers").unwrap();

        let mut file = fs::File::open(&path).unwrap();
        // 1000 字节是"旧文件"的长度，远超当前文件实际的 18 字节。
        let text = read_tail_from(&mut file, 1000, 16);
        assert!(
            !text.is_empty(),
            "文件里有内容时不允许返回空 tail（正是 P2-8 的失败形态）"
        );
        assert!(
            text.contains("rotation-leftovers"),
            "应退回从头读，实际拿到：{text:?}"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn tail_from_handle_respects_the_bound_when_the_length_is_current() {
        // 长度可信时仍然只读尾部：越界兜底不能退化成"总是读全文"。
        let dir = temp_dir("tail-bound");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("install.log");
        fs::write(&path, b"0123456789").unwrap();

        let mut file = fs::File::open(&path).unwrap();
        let len = file.metadata().unwrap().len();
        assert_eq!(read_tail_from(&mut file, len, 4), "6789");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn capped_line_consumes_overlong_line_without_growing_buffer() {
        let input = format!("{}\nnext\n", "x".repeat(MAX_OUTPUT_LINE_BYTES * 2));
        let mut reader = BufReader::new(Cursor::new(input));
        let mut buffer = Vec::new();
        let first = read_capped_line(&mut reader, &mut buffer).expect("read first line");
        assert!(first
            .as_deref()
            .is_some_and(|line| line.contains("输出行已截断")));
        assert!(buffer.len() <= MAX_OUTPUT_LINE_BYTES);
        assert_eq!(
            read_capped_line(&mut reader, &mut buffer)
                .unwrap()
                .as_deref(),
            Some("next")
        );
    }

    #[test]
    fn bounded_capture_rejects_excess_immediately() {
        let input = vec![b'x'; 32];
        let error = read_bounded_bytes(Cursor::new(input), 8).expect_err("capture must be bounded");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn atomic_write_replaces_file_without_leaving_staging_files() {
        let seq = PROCESS_TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "dsh-atomic-write-test-{}-{seq}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create test directory");
        let path = root.join("store.json");
        fs::write(&path, b"old\n").expect("seed destination");

        atomic_write(&path, b"new\n").expect("atomic replacement");

        assert_eq!(
            fs::read_to_string(&path).expect("read destination"),
            "new\n"
        );
        let leftovers: Vec<_> = fs::read_dir(&root)
            .expect("read test directory")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().contains(".tmp-"))
            .collect();
        assert!(leftovers.is_empty(), "temporary files must be cleaned up");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn command_capture_returns_bounded_stdout_and_stderr() {
        let cmd = if cfg!(windows) {
            let mut cmd = Command::new("cmd.exe");
            cmd.args(["/C", "echo out 1>&2 & echo ok"]);
            cmd
        } else {
            let mut cmd = Command::new("/bin/sh");
            cmd.args(["-c", "printf ok; printf err >&2"]);
            cmd
        };
        let (success, stdout, stderr) = run_command_capture(cmd, "capture test").unwrap();
        assert!(success);
        let line_end = if cfg!(windows) { "\r\n" } else { "" };
        assert_eq!(stdout, format!("ok{line_end}"));
        // `echo out 1>&2` 重定向的是 `echo` 的**参数分隔空格**之后的输出：
        // 部分 cmd.exe 版本在重定向到管道时会把分隔空格一并写出（实测
        // `"out  \r\n"`，两个空格），另一些只有一个。这里只钉住语义部分，
        // 不把一个与本次改动无关的 cmd.exe 版本差异变成红灯。
        assert_eq!(stderr.trim_end(), "out");
    }

    #[cfg(unix)]
    #[test]
    fn command_capture_stops_when_output_limit_is_exceeded() {
        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", "yes x | head -c 4194305"]);
        let error = run_command_capture(cmd, "noisy capture").expect_err("capture must be bounded");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    /// 宿主平台上必然存在、且以**真实可执行文件**形式存在（非批处理
    /// shim）的一个程序及其参数，运行时必然写出一行 stdout。用于让 spawn
    /// 类测试覆盖「直接执行」这条路径——真机上的 pnpm 是 `.cmd`、node 是
    /// `.exe`，两条分支都必须被真正跑过一次。返回 `None` 表示宿主上找不到
    /// 这样的程序（此时测试自行跳过）。
    fn direct_executable() -> Option<(&'static str, &'static [&'static str])> {
        // 必须是「无参数也产生 stdout」的程序：日志写入器要以「有行可写」
        // 为前提才会创建并 flush 日志文件。
        const NO_ARGS: &[&str] = &[];
        const SH: &[&str] = &["-c", "printf hi"];
        let (exe, args) = if cfg!(windows) {
            ("C:\\Windows\\System32\\hostname.exe", NO_ARGS)
        } else {
            ("/bin/sh", SH)
        };
        Path::new(exe).is_file().then_some((exe, args))
    }

    /// `command_through_shell_if_needed` 必须把 `.exe` 直接交给
    /// CreateProcess，而不是经 `%ComSpec% /C` 拼命令行。后者在
    /// `C:\Program Files\nodejs\node.exe` 这类含空格路径上会把命令行
    /// 切成「`C:\Program` + 其余」，cmd 以退出码 1 报
    /// `'C:\Program' is not recognized...`，目标程序根本没跑——内核安装后
    /// 的原生模块探针正是这样在默认 Node 安装上全平台失败，且日志里连一行
    /// `PROBE-` 都没有（P0 回归的根因）。
    #[cfg(windows)]
    #[test]
    fn windows_executable_is_spawned_directly_not_through_command_shell() {
        let spaced = Path::new("C:\\Program Files\\nodejs\\node.exe");
        assert!(!needs_command_shell(spaced));
        // 直接执行：`Command` 的程序就是那个含空格的路径本身，参数单独
        // 成项；Debug 输出即最终命令行（`"C:\Program Files\nodejs\node.exe"
        // "--version"`）。旧实现渲染成 `cmd.exe /C C:\Program
        // Files\nodejs\node.exe --version`，正是被 cmd 切碎的那种形态。
        let cmd = command_through_shell_if_needed(spaced, &["--version"]);
        let rendered = format!("{cmd:?}");
        assert!(
            rendered.contains(r#""C:\\Program Files\\nodejs\\node.exe" "--version""#),
            "含空格的 .exe 必须以自身为程序直接执行，实际：{rendered}"
        );
        assert!(
            !rendered.contains("cmd.exe"),
            "真实可执行文件不得绕经 cmd.exe，实际：{rendered}"
        );
        // 批处理 shim 仍然必须经 cmd.exe —— CreateProcess 跑不了 .cmd。
        assert!(needs_command_shell(Path::new(
            "C:\\Users\\me\\AppData\\Roaming\\npm\\pnpm.CMD"
        )));
        assert!(needs_command_shell(Path::new(
            "C:\\Program Files\\nodejs\\npm.cmd"
        )));
        assert!(needs_command_shell(Path::new("C:\\tools\\build.bat")));
        // 空后缀沿用旧行为，不在此顺带改变。
        assert!(needs_command_shell(Path::new("C:\\tools\\mytool")));
        assert!(!needs_command_shell(Path::new(
            "C:\\Users\\me\\AppData\\Local\\pnpm\\pnpm.exe"
        )));
        assert!(!needs_command_shell(Path::new(
            "C:\\Windows\\System32\\cmd.com"
        )));
    }

    /// 真机回归：默认安装位置的 node 必须能被 spawn 出真实输出。这是
    /// 「node 路径含空格」这条 P0 的最小可执行复现——旧实现下它恒以退出
    /// 码 1 结束且没有任何 stdout/stderr。末尾顺带断言进度回调确实看到了
    /// 子进程输出，因为内核安装后的原生模块探针正是靠「日志/进度里有没有
    /// `PROBE-` 行」来区分「node 没跑起来」与「原生模块加载失败」的。
    #[cfg(windows)]
    #[test]
    fn node_in_program_files_spawns_and_reports_version() {
        let node = Path::new("C:\\Program Files\\nodejs\\node.exe");
        if !node.is_file() {
            return;
        }
        let mut cmd = command_through_shell_if_needed(node, &["--version"]);
        let output = quiet(&mut cmd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .expect("spawn node");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "node --version 必须成功，stderr={stderr:?}"
        );
        assert!(
            stdout.trim().starts_with('v'),
            "必须拿到真实版本输出，stdout={stdout:?}"
        );

        let seq = PROCESS_TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("dsh-xlink-probe-test-{}-{seq}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let logs_dir = root.join("logs");
        let node_dir = node.parent().expect("node has a parent directory");
        let mut captured: Vec<String> = Vec::new();
        let status = run_with_progress(
            node,
            &["--no-warnings", "-e", "console.log('PROBE-OK 1')"],
            &std::env::temp_dir(),
            &logs_dir,
            &LogSpec::new("test", "probe-spawn"),
            &[node_dir],
            |line| captured.push(line.to_string()),
        )
        .expect("run node through run_with_progress");
        assert!(
            status.success(),
            "node 必须以 0 退出，captured={captured:?}"
        );
        assert!(
            captured.iter().any(|line| line.contains("PROBE-OK")),
            "进度回调必须看到 node 的输出，captured={captured:?}"
        );
        let log_path =
            LogSpec::new("test", "probe-spawn").path_for(&logs_dir, &current_date_string());
        let logged = fs::read_to_string(&log_path).expect("read probe log");
        assert!(
            logged.contains("PROBE-OK"),
            "探针输出必须落到日志，实际：{logged:?}"
        );
        let _ = fs::remove_dir_all(&root);
    }

    /// 创建了 data dir 的 logs 目录；日志 open 必须创建缺失的父目录，
    /// 而不是以 NotFound 失败（Windows 上为 `os error 3`），那种情况
    /// 之前会在 npm 尚未 spawn 时就以误导性的「无法运行 npm」表面化。
    #[test]
    fn run_with_progress_creates_missing_log_directory() {
        let seq = PROCESS_TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "dsh-xlink-process-test-{}-{seq}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let logs_dir = root.join("a").join("b");
        let log_spec = LogSpec::new("test", "create-missing");
        let cwd = std::env::temp_dir();
        let Some((exe, args)) = direct_executable() else {
            return;
        };
        let status = run_with_progress(
            Path::new(exe),
            args,
            &cwd,
            &logs_dir,
            &log_spec,
            &[],
            |_| {},
        )
        .expect("spawn child");
        assert!(status.success());
        // dated 文件是文件名携带 spec 的 kind 与 name 的那一个；
        // 父目录必须存在才能写入。这里不去钉死确切文件名（路径里有日期），
        // 因为测试可能在 CI 上跨午夜运行。
        let today = current_date_string();
        let log_path = log_spec.path_for(&logs_dir, &today);
        assert!(log_path.is_file(), "log file must be created: {log_path:?}");
        let _ = fs::remove_dir_all(&root);
    }
    /// 助手在宿主平台上实际使用的分隔符。
    /// `merge_extra_path` 跟随 `cfg(windows)`（参见其函数体），
    /// 因此这里期望字符串也跟随这一选择，而不是硬编码 Unix 风格的 `:`，
    /// 否则在 Windows 测试运行器上会失败。
    const SEP: char = if cfg!(windows) { ';' } else { ':' };

    #[test]
    fn merge_extra_path_no_extras_returns_base() {
        let base = format!("/usr/bin{SEP}/bin");
        assert_eq!(merge_extra_path(&base, &[]), base);
    }

    #[test]
    fn merge_extra_path_prepends_single_dir() {
        let dir = PathBuf::from("/usr/local/bin");
        let merged = merge_extra_path(&format!("/usr/bin{SEP}/bin"), &[dir.as_path()]);
        assert_eq!(merged, format!("/usr/local/bin{SEP}/usr/bin{SEP}/bin"));
    }

    #[test]
    fn merge_extra_path_preserves_order() {
        let first = PathBuf::from("/opt/homebrew/bin");
        let second = PathBuf::from("/usr/local/bin");
        let merged = merge_extra_path("/usr/bin", &[first.as_path(), second.as_path()]);
        assert_eq!(
            merged,
            format!("/opt/homebrew/bin{SEP}/usr/local/bin{SEP}/usr/bin")
        );
    }

    #[test]
    fn merge_extra_path_skips_empty_segments() {
        let empty = PathBuf::from("");
        let blank = PathBuf::from("   ");
        let real = PathBuf::from("/usr/local/bin");
        let merged = merge_extra_path(
            "/usr/bin",
            &[empty.as_path(), blank.as_path(), real.as_path()],
        );
        assert_eq!(merged, format!("/usr/local/bin{SEP}/usr/bin"));
    }

    #[test]
    fn merge_extra_path_empty_base_still_includes_extras() {
        let dir = PathBuf::from("/usr/local/bin");
        let merged = merge_extra_path("", &[dir.as_path()]);
        assert_eq!(merged, "/usr/local/bin");
    }

    #[test]
    fn merge_extra_path_all_extras_blank_falls_back_to_base() {
        // 仅包含空白 / 空条目的切片必须保持 base 不变——助手不应在
        // 条目缺失时 panic。
        let empty = PathBuf::from("");
        let base = format!("/usr/bin{SEP}/bin");
        let merged = merge_extra_path(&base, &[empty.as_path()]);
        assert_eq!(merged, base);
    }

    /// `command_with_path` 是 `spawn` 的单次执行兄弟：每个针对外部工具
    /// （`git`、`tar` 等）的直接 `Command::new` 都应该走这里，让 GUI 壳
    /// 的子进程继承合并后的 PATH，而不是只能看到系统 PATH。`Command`
    /// 的 Debug 格式化器只输出 program 与 args（环境项在我们关心的所有
    /// Rust 版本上都存放在不透明的内部表中），因此这里走真实的 spawn 路径：
    /// 测试在每个支持的宿主上运行一个小的无 shell 子进程，检查子进程的
    /// `$PATH` 回显的是合并后的值，而非会触发 Windows bug 的裸继承 PATH。
    #[test]
    fn command_with_path_stamps_merged_path_on_child() {
        use std::process::Stdio;

        let mut cmd = command_with_path(if cfg!(windows) { "cmd.exe" } else { "/bin/sh" });
        // `cmd.exe /C "echo %PATH%"` 与 `/bin/sh -c 'echo "$PATH"'` 都会
        // 原封不动地把继承的 PATH 回传到子进程；任何与 `env::merged_path()`
        // 不一致都是助手的错。
        let child_path: String = if cfg!(windows) {
            "%PATH%".to_string()
        } else {
            "$PATH".to_string()
        };
        let marker = "__DSH_TEST_PATH_MARKER__";
        if cfg!(windows) {
            cmd.arg("/C")
                .arg(format!("echo {child_path} & echo {marker}"));
        } else {
            cmd.arg("-c")
                .arg(format!("echo {child_path}; echo {marker}"));
        }
        cmd.stdin(Stdio::null());
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        let child = quiet(&mut cmd).spawn().expect("spawn child");
        let output = child.wait_with_output().expect("collect output");
        assert!(
            output.status.success(),
            "child must exit cleanly, stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        let marker_idx = stdout
            .find(marker)
            .unwrap_or_else(|| panic!("child never printed marker: {stdout:?}"));
        // PATH 一行是 marker 行之前的所有内容；去掉末尾换行以便与
        // 助手的精确输出比较。
        let stamped = stdout[..marker_idx].trim_end().to_string();
        assert_eq!(
            stamped,
            crate::env::merged_path(),
            "child must inherit the merged PATH stamped by the helper"
        );
    }

    /// 子进程退出后，若孙进程仍持有输出管道，命令必须很快返回，而不是一直等到
    /// 30 分钟的总超时——那会把一次**已经成功**的安装报成"运行超过 30 分钟"的失败。
    ///
    /// Unix-only：Windows 上要用 `start /b` 才能造出同样的"孙进程继承管道"形态。
    #[cfg(unix)]
    #[test]
    fn run_with_progress_returns_soon_when_a_grandchild_holds_the_pipe() {
        let root = std::env::temp_dir().join(format!(
            "dsh-drain-grace-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        fs::create_dir_all(&root).expect("create work dir");

        let started = Instant::now();
        let status = run_with_progress_at(
            Path::new("/bin/sh"),
            // `&` 让 sleep 继承 stdout 后 sh 立刻退出：子进程没了，管道还开着。
            &["-c", "sleep 30 & exit 0"],
            &root,
            &root.join("drain.log"),
            &[],
            |_| {},
        )
        .expect("命令本身必须成功返回");
        let elapsed = started.elapsed();

        assert!(status.success(), "子进程应正常退出");
        assert!(
            elapsed < Duration::from_secs(20),
            "子进程退出后应当在宽限内返回，实际耗时 {elapsed:?}"
        );
        let _ = fs::remove_dir_all(&root);
    }

    /// `command_with_path_dirs` 必须把调用方给出的目录**前置**到子进程 PATH。
    ///
    /// 这是"内核里 `#!/usr/bin/env node` 能工作"的前提：走托管安装（或 nvm
    /// 绝对路径探测）时，那个 node 根本不在继承来的 PATH 上，内核对它派生的
    /// 一切 node 子进程（插件 CLI、npm/npx、工作台终端任务）都只能靠这层前置
    /// 找到解释器——少了它，用户看到的是 `env: node: No such file or directory`。
    #[test]
    fn command_with_path_dirs_prepends_extra_directories() {
        use std::process::Stdio;

        let extra = std::env::temp_dir().join(format!(
            "dsh-path-dirs-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        fs::create_dir_all(&extra).expect("create extra dir");

        let mut cmd = command_with_path_dirs(
            if cfg!(windows) { "cmd.exe" } else { "/bin/sh" },
            &[extra.as_path()],
        );
        let marker = "__DSH_TEST_PATH_DIRS_MARKER__";
        if cfg!(windows) {
            cmd.arg("/C").arg(format!("echo %PATH% & echo {marker}"));
        } else {
            cmd.arg("-c").arg(format!("echo \"$PATH\"; echo {marker}"));
        }
        cmd.stdin(Stdio::null());
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        let child = quiet(&mut cmd).spawn().expect("spawn child");
        let output = child.wait_with_output().expect("collect output");
        assert!(
            output.status.success(),
            "child must exit cleanly, stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        let marker_idx = stdout
            .find(marker)
            .unwrap_or_else(|| panic!("child never printed marker: {stdout:?}"));
        let stamped = stdout[..marker_idx].trim_end();
        let expected = merge_extra_path(crate::env::merged_path(), &[extra.as_path()]);
        assert_eq!(
            stamped, expected,
            "child PATH must be the merged path with the extra directory prepended"
        );
        let _ = fs::remove_dir_all(&extra);
    }

    /// `local_date_string` 是按日轮转的心跳：在今天构造的 `RotatingLog`
    /// 在请求当前路径时，必须报告一个带今天日期戳的文件。这里不去断言
    /// 具体年份，让测试能扛过慢速 CI 时钟；唯一不变的形态约定是 spec
    /// 名称之后的 `YYYY-MM-DD` 后缀。
    #[test]
    fn log_spec_path_for_stamps_local_date() {
        let spec = LogSpec::new("test", "kernel");
        let path = spec.path_for(Path::new("/tmp/logs"), &current_date_string());
        let name = path.file_name().and_then(|n| n.to_str()).expect("name");
        assert!(
            name.starts_with("test-kernel-"),
            "filename should carry the build kind and logical name: {name}"
        );
        // 日期后缀必须为 10 字符：YYYY-MM-DD。
        let suffix = name.trim_end_matches(".log");
        let date = suffix.rsplit('-').take(3).collect::<Vec<_>>();
        let mut reconstructed = String::new();
        for (i, part) in date.iter().rev().enumerate() {
            if i > 0 {
                reconstructed.push('-');
            }
            reconstructed.push_str(part);
        }
        assert_eq!(
            reconstructed.len(),
            10,
            "date suffix must be YYYY-MM-DD: {reconstructed}"
        );
    }

    /// `build_log_kind` 是区分 release 与 dev 日志的关键。它必须与
    /// 目录划分（`desktop/` 与 `desktop-dev/`）保持一致，使从 data dir
    /// 取出的 tar 包及其中的文件名总能就「来自哪个构建」达成一致。
    #[test]
    fn build_log_kind_matches_data_dir_split() {
        let kind = build_log_kind();
        if cfg!(debug_assertions) {
            assert_eq!(kind, LOG_KIND_DEV);
        } else {
            assert_eq!(kind, LOG_KIND_RELEASE);
        }
    }

    /// 今天打开的 `RotatingLog` 被赋予一个未来日期后，必须在下一次写入
    /// 时迁移到新一天的文件。我们通过把写入器的 `current_date` 与
    /// `current_path` 改成另一个日历日来覆盖 date-rolled 分支；
    /// 下一次写入必须重新解析路径并落到新文件。
    #[test]
    fn rotating_log_rolls_on_date_change() {
        let seq = PROCESS_TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let logs_dir = std::env::temp_dir().join(format!("dsh-rot-test-{seq}"));
        let _ = fs::remove_dir_all(&logs_dir);
        let spec = LogSpec::new("test", "kernel");
        // 1. 用针对固定路径打开的写入器写入，模拟「昨天」。这与
        //    `run_with_progress_at` 针对一个恰好匹配 dated 方案的脚本
        //    会产生的形状相同。
        let path_yesterday = spec.path_for(&logs_dir, "2000-01-01");
        let mut yesterday_log = RotatingLog::new_at_path(&path_yesterday).expect("open yesterday");
        yesterday_log.write_line("day-one").expect("write day one");
        // 2. 打开一个全新的 dated 写入器（这代表壳在新一天启动的瞬间）。
        //    之前写入的昨天文件必须保持原样。
        let mut today_log = RotatingLog::new(&logs_dir, spec.clone()).expect("open today");
        today_log.write_line("day-two").expect("write day two");
        // 3. dated 写入器不能踩坏被固定的昨天文件：day-one 在 2000-01-01
        //    文件里，day-two 在今天的文件里，两条路径不同。
        let path_today = spec.path_for(&logs_dir, &current_date_string());
        let yesterday_text = std::fs::read_to_string(&path_yesterday).expect("read yesterday");
        let today_text = std::fs::read_to_string(&path_today).expect("read today");
        assert!(yesterday_text.contains("day-one"));
        assert!(today_text.contains("day-two"));
        assert_ne!(path_yesterday, path_today);
        let _ = fs::remove_dir_all(&logs_dir);
    }

    /// `write_line` 在 `current_date` 已经不再匹配本地时钟时必须走
    /// date-rolled 分支——不只在构造时走。我们通过给私有字段塞一个
    /// 陈旧日期来覆盖这一分支，然后写入一行：它应当落到今天的文件
    /// （即写入器从实时时钟解析出的那条路径）。
    #[test]
    fn rotating_log_takes_date_rolled_branch_on_stale_state() {
        let seq = PROCESS_TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let logs_dir = std::env::temp_dir().join(format!("dsh-rot-stale-test-{seq}"));
        let _ = fs::remove_dir_all(&logs_dir);
        let spec = LogSpec::new("test", "kernel");
        let mut log = RotatingLog::new(&logs_dir, spec.clone()).expect("open");
        log.write_line("first").expect("write first");
        // 模拟写入器跨午夜一直开着：保存的 `current_date` 是昨天，
        // 但实时时钟已经走到今天。下一次写入必须观察到这一不一致，
        // 在今天的路径上重新打开（`resolve_path` 从实时时钟派生出该路径）。
        let yesterday = spec.path_for(&logs_dir, "2000-01-01");
        log.current_date = String::from("2000-01-01");
        log.current_path = yesterday.clone();
        log.write_line("second").expect("write second");
        let today = spec.path_for(&logs_dir, &current_date_string());
        let today_text = std::fs::read_to_string(&today).expect("read today");
        assert!(
            today_text.contains("second"),
            "stale-state write must reach the live today's file: {today_text:?}"
        );
        // 「first」一行仍位于测试开始时打开的那个文件；日滚重置会
        // 打开今天的文件，不会回溯修改原始文件。
        let _ = fs::remove_dir_all(&logs_dir);
    }

    /// `RotatingLog::new_at_path` 是用于 per-plugin 构建日志的固定路径
    /// 变体：它不能打日期或构建类型戳，连续写入必须追加到同一个文件
    /// （使一个长构建的历史留在同一份脚本里）。
    #[test]
    fn rotating_log_at_path_appends_without_stamp() {
        let seq = PROCESS_TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("dsh-rot-fixed-test-{seq}"));
        let _ = fs::remove_dir_all(&root);
        let path = root.join(".dsh-build.log");
        let mut log = RotatingLog::new_at_path(&path).expect("open at path");
        log.write_line("first").expect("write first");
        log.write_line("second").expect("write second");
        let text = std::fs::read_to_string(&path).expect("read");
        assert_eq!(text, "first\nsecond\n");
        // 文件名不携带构建类型 / 日期戳。
        let name = path.file_name().and_then(|n| n.to_str()).expect("name");
        assert_eq!(name, ".dsh-build.log");
        let _ = fs::remove_dir_all(&root);
    }

    /// `write_line` 在每一行之后 flush OS 文件，使得壳的 panic / SIGKILL
    /// 也能在磁盘上保留一份可供排查的最新日志。我们通过单次写入后用
    /// 一个独立的 `File` 句柄重新打开来验证：不刷新的 `BufWriter` 在
    /// Drop 后仍能给出数据，而不刷新的 `File` 不能（OS 自身也有缓冲，
    /// 所以这里真正校验的是：在 `RotatingLog` 层面我们不会丢失那一行）。
    #[test]
    fn rotating_log_writes_are_durable_after_write() {
        let seq = PROCESS_TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let logs_dir = std::env::temp_dir().join(format!("dsh-rot-flush-test-{seq}"));
        let _ = fs::remove_dir_all(&logs_dir);
        let spec = LogSpec::new("test", "kernel");
        let mut log = RotatingLog::new(&logs_dir, spec.clone()).expect("open");
        log.write_line("durable").expect("write");
        // 通过全新的文件句柄读取（无共享状态），确认字节已经走过
        // 写入器，到达 OS 页缓存。
        let path = spec.path_for(&logs_dir, &current_date_string());
        let text = std::fs::read_to_string(&path).expect("read after write");
        assert!(text.contains("durable"));
        let _ = fs::remove_dir_all(&logs_dir);
    }

    /// macOS 工具链补丁：CommandLineTools 自带的 clang 找不到完整的
    /// libc++ 头目录。`detect_macos_sdk_root` 必须能拿到某个 SDK 路径，
    /// `apply_macos_toolchain_env` 必须把 libc++ 的 v1 与 SDK 系统头
    /// 拼进 `CPLUS_INCLUDE_PATH` / `C_INCLUDE_PATH`。这是 `fs-ext`、
    /// `node-pty` 等原生模块在「pnpm 真正跑构建脚本」路径下能否成功
    /// 编译的关键；少了这一步，`fatal error: 'memory' file not found`
    /// 会让所有需要 node-gyp 的依赖全部失败。
    #[cfg(target_os = "macos")]
    #[test]
    fn apply_macos_toolchain_env_sets_include_paths_when_sdk_available() {
        let sdk_root = match detect_macos_sdk_root() {
            Some(path) if std::path::Path::new(&path).is_dir() => path,
            _ => return, // 没装 CommandLineTools 的 CI 环境允许直接跳过
        };
        let libcxx_include = format!("{sdk_root}/usr/include/c++/v1");
        let sys_include = format!("{sdk_root}/usr/include");

        // 父进程没有 SDK 路径的占位 CPLUS_INCLUDE_PATH 时，函数必须
        // 把 libcxx_include 与 sys_include 都加进来。
        let mut cmd = std::process::Command::new("/bin/true");
        apply_macos_toolchain_env(&mut cmd);
        let cxx = cmd
            .get_envs()
            .find(|(k, _)| *k == std::ffi::OsStr::new("CPLUS_INCLUDE_PATH"))
            .and_then(|(_, v)| v.map(|s| s.to_string_lossy().into_owned()));
        let c_path = cmd
            .get_envs()
            .find(|(k, _)| *k == std::ffi::OsStr::new("C_INCLUDE_PATH"))
            .and_then(|(_, v)| v.map(|s| s.to_string_lossy().into_owned()));
        if let Some(cxx) = cxx {
            assert!(
                cxx.split(':').any(|p| p == libcxx_include),
                "CPLUS_INCLUDE_PATH 应包含 {libcxx_include}，实际：{cxx}"
            );
        }
        if let Some(c_path) = c_path {
            assert!(
                c_path.split(':').any(|p| p == sys_include),
                "C_INCLUDE_PATH 应包含 {sys_include}，实际：{c_path}"
            );
        }
    }

    /// `apply_macos_toolchain_env` 不修改父进程的环境——它只在传入的
    /// `Command` 上 `env` 调用；这条单测用于防止将来误用 `std::env::set_var`
    /// 把 SDKROOT / CPLUS_INCLUDE_PATH 写进父进程，污染后续所有 spawn。
    #[cfg(target_os = "macos")]
    #[test]
    fn apply_macos_toolchain_env_does_not_mutate_parent_env() {
        let before_sdkroot = std::env::var_os("SDKROOT");
        let before_cxx = std::env::var_os("CPLUS_INCLUDE_PATH");
        let mut cmd = std::process::Command::new("/bin/true");
        apply_macos_toolchain_env(&mut cmd);
        assert_eq!(
            std::env::var_os("SDKROOT"),
            before_sdkroot,
            "SDKROOT 不应被写入父进程环境"
        );
        assert_eq!(
            std::env::var_os("CPLUS_INCLUDE_PATH"),
            before_cxx,
            "CPLUS_INCLUDE_PATH 不应被写入父进程环境"
        );
    }
}

#[cfg(test)]
mod drain_stream_tests {
    use super::*;
    use std::io::Cursor;
    use std::sync::Mutex as StdMutex;

    /// `LOG_WRITE_ERROR` 是进程级单例，而 libtest 默认并发跑用例：碰它的用例
    /// 必须串行，否则一个用例的 `take_log_write_error()` 会把另一个用例刚写进去
    /// 的诊断读走（这正是第一版并发下偶发失败的原因）。
    static GLOBAL_SLOT_LOCK: StdMutex<()> = StdMutex::new(());

    fn lock_slot() -> std::sync::MutexGuard<'static, ()> {
        GLOBAL_SLOT_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn drain_keeps_reading_after_a_write_failure() {
        // P2-3：写入失败后旧实现立即 break，子进程管道会被填满，内核卡在写
        // 日志上；而且日志静默死亡、事故面板仍指向该文件。
        let _guard = lock_slot();
        let input = "one\ntwo\nthree\n";
        let mut seen: Vec<String> = Vec::new();
        let lines = drain_stream(Cursor::new(input.as_bytes().to_vec()), |line| {
            seen.push(line.to_string());
            Err(io::Error::other("disk full"))
        })
        .expect("读取本身不该失败");

        assert_eq!(lines, 3, "必须把流排空，否则子进程会阻塞");
        assert_eq!(seen, vec!["one", "two", "three"], "每一行都要交给写入器");
        assert!(
            take_log_write_error().is_some(),
            "写入失败必须留下可读的诊断"
        );
    }

    #[test]
    fn drain_reports_recovery_without_duplicating_diagnostics() {
        let _guard = lock_slot();
        let input = "a\nb\nc\nd\n";
        let mut calls = 0usize;
        let lines = drain_stream(Cursor::new(input.as_bytes().to_vec()), |_| {
            calls += 1;
            // 第 2 行失败一次，之后恢复。
            if calls == 2 {
                Err(io::Error::other("transient"))
            } else {
                Ok(())
            }
        })
        .expect("读取本身不该失败");
        assert_eq!(lines, 4);
        assert!(take_log_write_error().is_some(), "瞬时失败也要留痕");
        assert!(
            take_log_write_error().is_none(),
            "读走一次即清，避免同一条诊断反复出现"
        );
    }

    #[test]
    fn drain_returns_cleanly_for_empty_input() {
        let lines = drain_stream(Cursor::new(Vec::new()), |_| Ok(())).expect("ok");
        assert_eq!(lines, 0);
    }
}

#[cfg(test)]
mod log_retention_tests {
    use super::*;
    use std::fs::File;

    static RETENTION_COUNTER: std::sync::atomic::AtomicUsize =
        std::sync::atomic::AtomicUsize::new(0);

    fn temp_logs(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "dsh-xlink-retention-{}-{}-{}",
            label,
            std::process::id(),
            RETENTION_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_log(dir: &Path, name: &str, bytes: usize, age_secs: u64) {
        let path = dir.join(name);
        fs::write(&path, vec![b'x'; bytes]).unwrap();
        let when = SystemTime::now() - Duration::from_secs(age_secs);
        File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(when)
            .unwrap();
    }

    #[test]
    fn logs_older_than_the_retention_window_are_removed() {
        // P2-62：日期只增不减，不裁剪会长期累积到 GB 级。
        let dir = temp_logs("age");
        write_log(&dir, "release-kernel-old.log", 16, 40 * 24 * 60 * 60);
        write_log(&dir, "release-kernel-recent.log", 16, 20 * 24 * 60 * 60);
        write_log(&dir, "release-kernel-today.log", 16, 5);

        let removed = prune_old_logs(&dir);

        assert_eq!(removed, 1, "只应删掉超过 30 天的那一份");
        assert!(!dir.join("release-kernel-old.log").exists());
        assert!(dir.join("release-kernel-recent.log").exists());
        assert!(dir.join("release-kernel-today.log").exists());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn total_size_budget_drops_the_oldest_first() {
        let dir = temp_logs("size");
        // 每份 100 MiB、都在保留期内：总量 300 MiB > 200 MiB，必须从最旧的开始丢。
        let hundred_mib = 100 * 1024 * 1024;
        write_log(
            &dir,
            "release-install-oldest.log",
            hundred_mib,
            3 * 24 * 60 * 60,
        );
        write_log(
            &dir,
            "release-install-middle.log",
            hundred_mib,
            2 * 24 * 60 * 60,
        );
        write_log(
            &dir,
            "release-install-newest.log",
            hundred_mib,
            24 * 60 * 60,
        );

        let removed = prune_old_logs(&dir);

        assert_eq!(removed, 1, "超预算时从最旧的开始删，删到不再超预算为止");
        assert!(!dir.join("release-install-oldest.log").exists());
        assert!(dir.join("release-install-middle.log").exists());
        assert!(dir.join("release-install-newest.log").exists());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn freshly_written_logs_are_never_deleted() {
        // 刚写过的文件很可能是当前会话正在追加的日志；即使超预算也不能动。
        let dir = temp_logs("grace");
        let huge = LOG_RETENTION_BYTES + 8 * 1024 * 1024;
        write_log(&dir, "release-kernel-live.log", huge as usize, 1);

        let removed = prune_old_logs(&dir);

        assert_eq!(removed, 0, "宽限期内的文件绝不删除");
        assert!(dir.join("release-kernel-live.log").exists());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn non_log_files_and_missing_dirs_are_left_alone() {
        let dir = temp_logs("mixed");
        write_log(&dir, "keep.txt", 16, 400 * 24 * 60 * 60);
        fs::write(dir.join("notes.md"), b"hello").unwrap();

        assert_eq!(prune_old_logs(&dir), 0, "只处理 *.log");
        assert!(dir.join("keep.txt").exists());
        assert_eq!(prune_old_logs(&dir.join("absent")), 0, "目录缺失时静默返回");
        fs::remove_dir_all(&dir).ok();
    }
}
