//! Resolving a usable `PATH` for child processes spawned by the shell.
//!
//! Tauri apps on Windows run as GUI-subsystem processes (`windows_subsystem
//! = "windows"`), and the path block CreateProcess inherits on launch is the
//! Window-Station system path. The user-level PATH that npm, pnpm, nvm and
//! similar tools add during install lives in
//! `HKEY_CURRENT_USER\Environment\Path` and is merged into the
//! system path by Explorer and other interactive hosts — but GUI apps
//! launched from a desktop shortcut, the Run dialog, or auto-start see
//! only the system path. The result: a shell that finds `node` (which
//! ships at a system path under `C:\Program Files\nodejs`) but cannot
//! find `pnpm.cmd` (which the user's npm prefix put under
//! `%AppData%\npm`), and silently fails to launch kernel installs.
//!
//! This module reads the user PATH once at process start and exposes a
//! merged `PATH` string that every `process::spawn` stamps onto the child
//! before it inherits any other env. Non-Windows platforms are a no-op:
//! `Command::env` is called with the same value the parent already has.

#[cfg(windows)]
use std::process::Command;
use std::sync::OnceLock;

/// Env var name Tauri-inherited Windows processes consult for the user
/// PATH; matches `HKEY_CURRENT_USER\Environment\Path` and the registry
/// value the User Environment Variable Editor exposes.
const PATH: &str = "PATH";
/// Windows registry path holding the user PATH (and friends).
#[cfg(windows)]
const REG_USER_ENV: &str = "HKCU\\Environment";
/// Registry value name for the user PATH.
#[cfg(windows)]
const REG_PATH_VALUE: &str = "Path";
/// `reg.exe` ships at a fixed Windows location and is always on the
/// system PATH; pinning it removes any chance of an attacker-planted shim
/// answering on PATH.
#[cfg(windows)]
const REG_EXE: &str = "C:\\Windows\\System32\\reg.exe";

/// One-shot cached merged PATH. Initialized lazily on first call from
/// `merged_path`. `OnceLock` is `Sync` and does not require `unsafe`, so
/// it is the right primitive here even though `std::env::set_var` would
/// not be safe in this multi-threaded Tauri runtime.
static MERGED: OnceLock<String> = OnceLock::new();

/// Effective `PATH` for child processes: process env on Unix, the
/// cached Windows merge on Windows. Returned as `&'static str` so
/// callers can pass it directly to `Command::env` without cloning.
pub fn merged_path() -> &'static str {
    MERGED.get_or_init(compute_merged_path).as_str()
}

#[cfg(not(windows))]
fn compute_merged_path() -> String {
    // On macOS / Linux the launched process inherits a usable PATH from
    // the parent shell. No merge is needed; mirror whatever is set so
    // `process::spawn` writes the same value back.
    std::env::var(PATH).unwrap_or_default()
}

#[cfg(windows)]
fn compute_merged_path() -> String {
    let system = std::env::var(PATH).unwrap_or_default();
    match read_user_path() {
        Some(user) if !user.is_empty() => merge_paths(&system, &user),
        // Either no registry entry or `reg.exe` refused to talk to us;
        // the system PATH is still better than nothing.
        _ => system,
    }
}

/// Read the user's `Path` value from `HKCU\Environment` via `reg.exe`.
/// Returns `None` on any failure (registry entry absent, permission
/// denied, shell error); callers fall back to the system PATH.
///
/// `reg.exe` is a GUI-subsystem binary so it never pops a console window
/// when spawned; we still need `quiet()` (CREATE_NO_WINDOW) to suppress
/// the brief flash some Windows builds produce for console programs, but
/// here it is belt-and-braces.
#[cfg(windows)]
fn read_user_path() -> Option<String> {
    use crate::process::quiet;
    use std::process::Stdio;
    let mut cmd = Command::new(REG_EXE);
    cmd.args(["query", REG_USER_ENV, "/v", REG_PATH_VALUE])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = quiet(&mut cmd).output().ok()?;
    if !output.status.success() {
        return None;
    }
    parse_reg_path(&String::from_utf8_lossy(&output.stdout))
}

/// Pull the value column out of `reg query`'s standard output:
/// `... Path    REG_SZ    C:\Users\...;C:\Program Files\...`
/// Tolerates the `Path    REG_EXPAND_SZ` variant the registry editor
/// writes when the value contains `%FOO%` references; both formats end
/// with `REG_<TYPE>    <value>` on the last non-empty line.
///
/// `split_whitespace` (not `splitn(3, char::is_whitespace)`) is the right
/// primitive here: `splitn` would cut at every individual whitespace
/// char, leaving the third element glued to `REG_SZ` because `splitn`
/// stops cutting after `n-1` matches regardless of how many separators
/// remain.
#[cfg(windows)]
fn parse_reg_path(out: &str) -> Option<String> {
    let last = out.lines().map(str::trim).rev().find(|l| !l.is_empty())?;
    let mut parts = last.split_whitespace();
    let _name = parts.next()?;
    let _ty = parts.next()?;
    let value = parts.collect::<Vec<_>>().join(" ");
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

/// Concatenate two `PATH` strings, preserving order, de-duplicating
/// entries (case-insensitive on Windows since the filesystem is),
/// skipping empty fields.
#[cfg(windows)]
fn merge_paths(system: &str, user: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let mut push = |entry: &str| {
        let trimmed = entry.trim();
        if trimmed.is_empty() {
            return;
        }
        let key = trimmed.to_ascii_lowercase();
        if seen.iter().any(|s| s == &key) {
            return;
        }
        seen.push(key);
        out.push(trimmed.to_string());
    };
    // User PATH first — anything the user explicitly put there wins over
    // whatever the system inherited, matching how Explorer concatenates
    // the two and how `cmd.exe` resolves bare names.
    for entry in user.split(';') {
        push(entry);
    }
    for entry in system.split(';') {
        push(entry);
    }
    out.join(";")
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn parse_reg_path_extracts_value() {
        let sample = "\
\r\nHKEY_CURRENT_USER\\Environment\r\n    Path    REG_SZ    C:\\Users\\zxx\\AppData\\Roaming\\npm;C:\\Program Files\\nodejs\r\n\r\n";
        assert_eq!(
            parse_reg_path(sample).as_deref(),
            Some("C:\\Users\\zxx\\AppData\\Roaming\\npm;C:\\Program Files\\nodejs"),
        );
    }

    #[test]
    fn parse_reg_path_handles_expand_sz() {
        let sample = "\
HKEY_CURRENT_USER\\Environment
    Path    REG_EXPAND_SZ    %USERPROFILE%\\bin;C:\\Windows
";
        assert_eq!(
            parse_reg_path(sample).as_deref(),
            Some("%USERPROFILE%\\bin;C:\\Windows"),
        );
    }

    #[test]
    fn parse_reg_path_rejects_empty_value() {
        let sample = "HKEY_CURRENT_USER\\Environment\n    Path    REG_SZ    \n";
        assert_eq!(parse_reg_path(sample), None);
    }

    #[test]
    fn merge_paths_user_wins_and_dedups() {
        let system = "C:\\Windows;C:\\Program Files\\nodejs";
        let user = "C:\\Users\\zxx\\AppData\\Roaming\\npm;c:\\program files\\nodejs";
        let merged = merge_paths(system, user);
        // User entry first; lowercase duplicate of `C:\Program Files\nodejs`
        // collapsed out; system `C:\Windows` retained at the end.
        assert_eq!(
            merged,
            "C:\\Users\\zxx\\AppData\\Roaming\\npm;c:\\program files\\nodejs;C:\\Windows",
        );
    }

    #[test]
    fn merge_paths_skips_empty_segments() {
        let merged = merge_paths(";;C:\\Windows;;", ";;C:\\Users\\bin;;");
        assert_eq!(merged, "C:\\Users\\bin;C:\\Windows");
    }
}
