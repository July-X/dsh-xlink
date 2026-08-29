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
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use time::format_description::FormatItem;
use time::macros::format_description;
use time::OffsetDateTime;

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

/// Build a `Command` for a one-shot external tool (`git`, `tar`, …) that
/// inherits the merged PATH, so children of the GUI shell can resolve
/// tools the user installed under their own user PATH. `process::spawn`
/// covers long-running helpers (pnpm/npm) and threads the same PATH in;
/// this helper is the single-shot sibling — every direct `Command::new`
/// in the shell that does not already live in `process::spawn` should
/// build through here so a Windows install from `tauri build`'s GUI
/// subsystem sees the same toolchain the user can see from `cmd.exe`.
///
/// Without the merge, `Command::new("git")` from a Windows GUI subsystem
/// process looks up `git.exe` only on the system PATH. Git for Windows
/// and most Windows installers register in `HKCU\Environment\Path`
/// (user PATH), not the system PATH, so the lookup misses and the user
/// sees the error wrapped as `未找到 git（git 来源的插件需要 git；请先
/// 安装 git）`. The same shape would affect any other user-PATH-only
/// tool — `tar` ships at `C:\Windows\System32\tar.exe` and works
/// without the merge on Windows 10+, but the explicit stamping keeps
/// macOS/Linux consistent and removes a future surprise when a new tool
/// stops being system-installed.
pub fn command_with_path<S: AsRef<OsStr>>(program: S) -> Command {
    let mut cmd = Command::new(program);
    cmd.env("PATH", crate::env::merged_path());
    cmd
}

/// Collect the output of a one-shot script tool (`npm config …`, …) whose
/// executable may be a `.cmd` batch shim. CreateProcess cannot execute
/// batch files directly on Windows, so the spawn routes through
/// `%ComSpec% /C` there, mirroring `spawn`; elsewhere the executable runs
/// directly. The child inherits the merged PATH with `extra_path_dirs`
/// prepended, so the script's `#!/usr/bin/env node` resolution finds the
/// node the caller validated even from a GUI shell with a system-only
/// PATH.
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
        let comspec = std::env::var("ComSpec").unwrap_or_else(|_| "cmd.exe".into());
        let mut cmd = Command::new(comspec);
        cmd.arg("/C").arg(exe).args(args);
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

/// Spawn a long-running child (`pnpm`, `npm`, …) and stream each stdout and
/// stderr line to both `log_path` and `on_progress`, returning once the
/// process exits.
///
/// `.cmd` files cannot be spawned directly on Windows, so they are routed
/// through the command shell there; everywhere else the executable is run
/// directly. Each output stream is drained on its own thread so a full OS
/// pipe buffer can never deadlock the other stream; the lines travel over a
/// channel back to this thread, which is the only caller of `on_progress`.
/// A heartbeat keeps the caller informed when the child stays silent for
/// tens of seconds while resolving the dependency graph or talking to the
/// npm registry.
///
/// `extra_path_dirs` prepends the listed directories to the inherited
/// `PATH` on the child before it runs anything. macOS `.app` bundles launch
/// from a launchd environment whose `PATH` is just `/usr/bin:/bin:/usr/sbin:
/// /sbin`, so a user who installed Node and pnpm via Homebrew or nvm lives
/// outside that path; a child that invokes a Node-shebanged script
/// (`tsdown`, `tsc`, `node ./foo.js`, …) then dies with `env: node: No
/// such file or directory` even though the parent could find both binaries
/// to spawn them. Prepending `pnpm_exe.parent()` (and `node_dir` when the
/// caller has it) makes the child see the same `node` the parent used.
///
/// The log file's parent directory is created when missing: the first
/// install on a fresh data dir reaches this helper before anything else
/// has created the log directory, and a bare `open` would otherwise fail
/// with `NotFound` (Windows: `系统找不到指定的路径 (os error 3)`).
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

fn rotated_log_path(path: &Path, index: u8) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(format!(".{index}"));
    PathBuf::from(name)
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
struct RotatingLog {
    mode: LogMode,
    current_date: String,
    current_path: PathBuf,
    writer: Option<fs::File>,
    bytes: u64,
}

impl RotatingLog {
    fn new(logs_dir: &Path, spec: LogSpec) -> io::Result<Self> {
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

    fn write_line(&mut self, line: &str) -> io::Result<()> {
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

fn spawn_log_drain<R: Read + Send + 'static>(stream: R, logger: Arc<Mutex<RotatingLog>>) {
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stream);
        let mut buffer = Vec::with_capacity(MAX_OUTPUT_LINE_BYTES);
        while let Ok(Some(line)) = read_capped_line(&mut reader, &mut buffer) {
            let Ok(mut log) = logger.lock() else { break };
            if log.write_line(&line).is_err() {
                break;
            }
        }
        if let Ok(mut log) = logger.lock() {
            let _ = log.flush();
        }
    });
}

const RUN_CAPTURE_TIMEOUT: Duration = Duration::from_secs(30);
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

fn run_capture_command_bytes(
    mut cmd: Command,
    label: &str,
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
    let deadline = started + RUN_CAPTURE_TIMEOUT;
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
                        format!(
                            "{label} timed out after {} seconds",
                            RUN_CAPTURE_TIMEOUT.as_secs()
                        ),
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

/// Run a short-lived external tool and capture bounded stdout/stderr.
///
/// The child is isolated into its own process group on Unix and its output
/// pipes are drained concurrently. A timeout kills the group and never waits
/// indefinitely for a reader whose pipe may still be held by a descendant.
pub fn run_capture_output(program: &str, args: &[&str]) -> io::Result<(bool, String, String)> {
    let (success, stdout, stderr) = run_capture_bytes(program, args)?;
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
    let started = Instant::now();
    let deadline = started + RUN_PROGRESS_TIMEOUT;
    let mut child_exited = false;
    let mut output_closed = false;
    let mut timed_out = false;
    loop {
        if !child_exited {
            match child.try_wait() {
                Ok(Some(_)) => child_exited = true,
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
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            timed_out = true;
            break;
        }
        match rx.recv_timeout(remaining.min(Duration::from_secs(HEARTBEAT_SECS))) {
            Ok(line) => {
                on_progress(line.trim_end());
                let _ = log.write_line(&line);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let secs = started.elapsed().as_secs();
                on_progress(&format!("… 子进程仍在运行（已进行 {secs} 秒）"));
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
        // The receiver is dropped before termination so a noisy reader exits
        // on send failure. Do not join here: a process outside the group may
        // still hold an inherited pipe, and the command must honor its
        // deadline rather than wait forever for that external process.
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

    let _ = drain_stdout.join();
    let _ = drain_stderr.join();

    let status = reap(child)?;
    log.flush()?;
    Ok(status)
}

fn spawn(exe: &Path, args: &[&str], cwd: &Path, extra_path_dirs: &[&Path]) -> io::Result<Child> {
    // Pin every child we spawn (pnpm/npm and friends) at the shell's
    // configured npm registry so the mirror choice is enforceable even when
    // the user's global .npmrc points elsewhere or a project-local .npmrc
    // is missing. `npm_config_registry` is the env var pnpm and npm both
    // consult as the highest-priority source.
    let registry = crate::registry::npm_registry_base();
    // Tauri ships as a Windows GUI-subsystem app and inherits only the
    // system PATH on launch; the user PATH (where `npm` and `pnpm` shims
    // live after `npm install -g`) is dropped unless we re-stamp it.
    // `env::merged_path` reads `HKCU\Environment\Path` once and joins it
    // onto whatever the process already has.
    //
    // `extra_path_dirs` is layered on top of that: caller-supplied
    // directories (the validated `node` bin dir, `pnpm_exe.parent()` so
    // pnpm's own shim family is reachable, …) are prepended in order so
    // any Node-shebanged child can resolve `node` even on macOS .app
    // bundles, whose launchd PATH is system-only.
    let path = merge_extra_path(crate::env::merged_path(), extra_path_dirs);
    #[cfg(windows)]
    {
        let comspec = std::env::var("ComSpec").unwrap_or_else(|_| "cmd.exe".into());
        let mut cmd = Command::new(comspec);
        cmd.arg("/C").arg(exe).args(args);
        // GUI shells start with an arbitrary cwd; the child must inherit an
        // explicit one or it resolves the nearest package.json upward and
        // installs into the wrong directory.
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
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        isolate_process(&mut cmd);
        quiet(&mut cmd).spawn()
    }
}

/// Prepend `extra` entries to `base`. Empty / non-existent entries are
/// skipped so a caller that has no extra directories pays no cost. Path
/// separators follow the host (`;` on Windows, `:` elsewhere).
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

/// Read a bounded tail of a text file for display. Missing or unreadable
/// files yield an empty string — callers render tails next to live state
/// and must not turn a vanished log into an error dialog.
pub(crate) fn read_tail(path: &Path, max_bytes: u64) -> String {
    let Ok(meta) = fs::metadata(path) else {
        return String::new();
    };
    let Ok(file) = fs::File::open(path) else {
        return String::new();
    };
    use std::io::{Read, Seek};
    let mut reader = file;
    let offset = meta.len().saturating_sub(max_bytes);
    // `Vec::with_capacity` cannot infer its element type until something
    // pins it; without the annotation `reader.read_to_end(&mut buf)` later
    // in this function needs the explicit hint.
    let mut buf: Vec<u8> = Vec::with_capacity(max_bytes as usize);
    if offset > 0 {
        let _ = reader.seek(io::SeekFrom::Start(offset));
    }
    let _ = reader.read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).into_owned()
}

fn reap(mut child: Child) -> io::Result<ExitStatus> {
    // On Windows `ComSpec /C` makes cmd.exe the direct child and the real
    // program its grandchild; waiting on cmd only returns after the
    // grandchild has already exited, so a plain wait is right everywhere.
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
        assert_eq!(stderr, if cfg!(windows) { "out \r\n" } else { "err" });
    }

    #[cfg(unix)]
    #[test]
    fn command_capture_stops_when_output_limit_is_exceeded() {
        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", "yes x | head -c 4194305"]);
        let error = run_command_capture(cmd, "noisy capture").expect_err("capture must be bounded");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
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
        let status = if cfg!(windows) {
            run_with_progress(
                Path::new("cmd.exe"),
                &["/C", "echo", "hi"],
                &cwd,
                &logs_dir,
                &log_spec,
                &[],
                |_| {},
            )
        } else {
            run_with_progress(
                Path::new("/bin/echo"),
                &["hi"],
                &cwd,
                &logs_dir,
                &log_spec,
                &[],
                |_| {},
            )
        }
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
        // A slice of only whitespace/empty entries must leave the base
        // untouched — the helper should never panic on missing entries.
        let empty = PathBuf::from("");
        let base = format!("/usr/bin{SEP}/bin");
        let merged = merge_extra_path(&base, &[empty.as_path()]);
        assert_eq!(merged, base);
    }

    /// `command_with_path` is the single-shot sibling of `spawn`: every
    /// direct `Command::new` for an external tool (`git`, `tar`, …) should
    /// route through here so the GUI shell's children inherit the merged
    /// PATH instead of the system-only PATH they would otherwise see.
    /// The Debug formatter of `Command` only reports the program and args
    /// (env entries live in an opaque internal table on every Rust
    /// version we care about), so we exercise the actual spawn path:
    /// the test runs a tiny shell-less child on every supported host
    /// and checks the child's `$PATH` echoes the merged value, not the
    /// bare inherited PATH that would surface as the Windows bug.
    #[test]
    fn command_with_path_stamps_merged_path_on_child() {
        use std::process::Stdio;

        let mut cmd = command_with_path(if cfg!(windows) { "cmd.exe" } else { "/bin/sh" });
        // `cmd.exe /C "echo %PATH%"` and `/bin/sh -c 'echo "$PATH"'` both
        // round-trip the inherited PATH through the child untouched, so
        // any divergence from `env::merged_path()` is the helper's fault.
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
        // PATH line is everything before the marker line; trim the
        // trailing newline so we compare against the helper's exact output.
        let stamped = stdout[..marker_idx].trim_end().to_string();
        assert_eq!(
            stamped,
            crate::env::merged_path(),
            "child must inherit the merged PATH stamped by the helper"
        );
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
}
