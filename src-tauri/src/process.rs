//! Suppressing the console window Windows allocates for child processes,
//! plus a shared helper that streams long-running child output to both a
//! log file and an in-process progress callback.
//!
//! The shell is a GUI-subsystem app: every `Command` spawned without
//! `CREATE_NO_WINDOW` makes Windows briefly allocate a console window, which
//! the user sees as a flashing terminal. All helper-process spawns in this
//! crate go through `quiet`.

use std::ffi::OsStr;
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Hide the console window Windows would otherwise flash for the child.
/// No-op on other platforms.
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
pub fn script_output(
    exe: &Path,
    args: &[&str],
    cwd: &Path,
    extra_path_dirs: &[&Path],
) -> io::Result<std::process::Output> {
    let path = merge_extra_path(crate::env::merged_path(), extra_path_dirs);
    #[cfg(windows)]
    {
        let comspec = std::env::var("ComSpec").unwrap_or_else(|_| "cmd.exe".into());
        let mut cmd = Command::new(comspec);
        cmd.arg("/C").arg(exe).args(args);
        cmd.current_dir(cwd);
        cmd.env("PATH", path);
        cmd.stdin(Stdio::null());
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        quiet(&mut cmd).output()
    }
    #[cfg(not(windows))]
    {
        let mut cmd = Command::new(exe);
        cmd.args(args);
        cmd.current_dir(cwd);
        cmd.env("PATH", path);
        cmd.stdin(Stdio::null());
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        cmd.output()
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
pub fn run_with_progress(
    exe: &Path,
    args: &[&str],
    cwd: &Path,
    log_path: &Path,
    extra_path_dirs: &[&Path],
    mut on_progress: impl FnMut(&str),
) -> io::Result<ExitStatus> {
    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut log = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(log_path)?;

    let mut child = spawn(exe, args, cwd, extra_path_dirs)?;
    let stdout = child.stdout.take().expect("child stdout was piped");
    let stderr = child.stderr.take().expect("child stderr was piped");

    let (tx, rx) = mpsc::channel::<String>();
    let tx_err = tx.clone();
    let drain_stdout = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let drain_stderr = std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines() {
            let Ok(line) = line else { break };
            if tx_err.send(line).is_err() {
                break;
            }
        }
    });

    const HEARTBEAT_SECS: u64 = 10;
    let started = Instant::now();
    loop {
        match rx.recv_timeout(Duration::from_secs(HEARTBEAT_SECS)) {
            Ok(line) => {
                on_progress(line.trim_end());
                let _ = writeln!(log, "{line}");
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let secs = started.elapsed().as_secs();
                on_progress(&format!("… 子进程仍在运行（已进行 {secs} 秒）"));
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    let _ = drain_stdout.join();
    let _ = drain_stderr.join();

    reap(child)
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
        cmd.spawn()
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
    use std::path::PathBuf;

    /// First-run installs reach `run_with_progress` before anything else
    /// has created the data dir's logs folder; the log open must create
    /// missing parents instead of failing with NotFound (Windows: `os
    /// error 3`), which previously surfaced as the misleading "无法运行
    /// npm" before npm was even spawned.
    #[test]
    fn run_with_progress_creates_missing_log_directory() {
        let root =
            std::env::temp_dir().join(format!("dsh-desktop-process-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let log_path = root.join("a").join("b").join("run.log");
        let cwd = std::env::temp_dir();
        let status = if cfg!(windows) {
            run_with_progress(
                Path::new("cmd.exe"),
                &["/C", "echo", "hi"],
                &cwd,
                &log_path,
                &[],
                |_| {},
            )
        } else {
            run_with_progress(
                Path::new("/bin/echo"),
                &["hi"],
                &cwd,
                &log_path,
                &[],
                |_| {},
            )
        }
        .expect("spawn child");
        assert!(status.success());
        assert!(log_path.is_file(), "log file must be created: {log_path:?}");
        let _ = fs::remove_dir_all(&root);
    }
    /// Separator the helper actually uses on the host platform.
    /// `merge_extra_path` follows `cfg(windows)` (see its body), so the
    /// expected strings here track that choice instead of hard-coding a
    /// Unix-style `:` that would fail on a Windows test runner.
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
}
