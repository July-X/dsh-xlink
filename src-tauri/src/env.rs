//! 为 Shell 派生的子进程解析一个可用的 `PATH`。
//!
//! Windows 上的 Tauri 应用作为 GUI 子系统进程（`windows_subsystem =
//! "windows"`）运行，CreateProcess 启动时继承的路径段是 Window Station
//! 的系统路径。npm、pnpm、nvm 等工具在安装时追加到用户级 PATH 的内容
//! 保存在 `HKEY_CURRENT_USER\Environment\Path` 中，由 Explorer 以及
//! 其他交互式宿主程序合并进系统路径——但是从桌面快捷方式、“运行”对话
//! 框或开机自启动启动的 GUI 应用只能看到系统路径。结果是：Shell 能找
//! 到 `node`（它位于 `C:\Program Files\nodejs` 下的系统路径），却找不
//! 到 `pnpm.cmd`（用户的 npm prefix 把它放到 `%AppData%\npm` 下），因
//! 此静默地无法启动内核安装。
//!
//! 本模块在进程启动时读取一次用户 PATH，并暴露一个合并后的 `PATH` 字
//! 符串，所有 `process::spawn` 在子进程继承其它环境变量之前都会把它写
//! 入子进程。 非 Windows 平台为 no-op：`Command::env` 直接传入父进程
//! 已有的那个值。

#[cfg(windows)]
use std::process::Command;
use std::sync::OnceLock;

/// Tauri 继承的 Windows 进程所查询的用户 PATH 环境变量名；与
/// `HKEY_CURRENT_USER\Environment\Path` 以及“用户环境变量编辑器”曝
/// 出的注册表值一致。
const PATH: &str = "PATH";
/// Windows 中保存用户 PATH（及同类项）的注册表路径。
#[cfg(windows)]
const REG_USER_ENV: &str = "HKCU\\Environment";
/// 注册表中用户 PATH 的值名。
#[cfg(windows)]
const REG_PATH_VALUE: &str = "Path";
/// `reg.exe` 位于固定的 Windows 路径下，并始终在系统 PATH 中；将其
/// 固定下来可以排除任何攻击者植入的同名 shim 在 PATH 上响应的可能。
#[cfg(windows)]
const REG_EXE: &str = "C:\\Windows\\System32\\reg.exe";

/// 一次性缓存的合并后 PATH。在首次从 `merged_path` 调用时惰性初始化。
/// `OnceLock` 是 `Sync` 的，且不需要 `unsafe`，因此即使
/// `std::env::set_var` 在这个多线程的 Tauri 运行时下并不安全，它仍
/// 然是正确的原语。
static MERGED: OnceLock<String> = OnceLock::new();

/// 子进程实际使用的 `PATH`：在 Unix 上直接使用进程环境变量，在
/// Windows 上使用缓存好的合并值。以 `&'static str` 返回，以便调用方
/// 直接传给 `Command::env` 而无需克隆。
pub fn merged_path() -> &'static str {
    MERGED.get_or_init(compute_merged_path).as_str()
}

#[cfg(not(windows))]
fn compute_merged_path() -> String {
    // 在 macOS / Linux 上，启动的进程从父 Shell 继承一个可用的 PATH。
    // 不需要合并；原样镜像已设置的值，让 `process::spawn` 把同样的值
    // 写回子进程。
    std::env::var(PATH).unwrap_or_default()
}

#[cfg(windows)]
fn compute_merged_path() -> String {
    let system = std::env::var(PATH).unwrap_or_default();
    match read_user_path() {
        Some(user) if !user.is_empty() => merge_paths(&system, &user),
        // 要么没有注册表项，要么 `reg.exe` 拒绝与我们通信；系统 PATH
        // 总归聊胜于无。
        _ => system,
    }
}

/// 通过 `reg.exe` 从 `HKCU\Environment` 读取用户的 `Path` 值。
/// 任何失败（注册表项缺失、权限被拒、Shell 出错）时返回 `None`；
/// 调用方回退到系统 PATH。
///
/// `reg.exe` 本身是 GUI 子系统二进制，启动时不会弹出控制台窗口；但
/// 在某些 Windows 版本中控制台程序仍可能短暂闪烁，因此这里仍然需要
/// `quiet()`（CREATE_NO_WINDOW），算作有备无患。
#[cfg(windows)]
fn read_user_path() -> Option<String> {
    let mut cmd = Command::new(REG_EXE);
    cmd.args(["query", REG_USER_ENV, "/v", REG_PATH_VALUE]);
    let (success, stdout, _) = crate::process::run_command_capture(cmd, "reg query PATH").ok()?;
    if !success {
        return None;
    }
    parse_reg_path(&stdout)
}

/// 从 `reg query` 的标准输出中提取值列：
/// `... Path    REG_SZ    C:\Users\...;C:\Program Files\...`
/// 同时兼容注册表编辑器在值中包含 `%FOO%` 引用时写出的
/// `Path    REG_EXPAND_SZ` 变体；两种格式都在最后一个非空行以
/// `REG_<TYPE>    <value>` 结尾。
///
/// 这里应使用 `split_whitespace`（而不是
/// `splitn(3, char::is_whitespace)`）：`splitn` 会在每一个独立的空白
/// 字符处切分，而 `splitn` 在匹配够 `n-1` 次后即停止，不再继续切
/// 分，结果会把第三个元素和 `REG_SZ` 粘在一起。
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

/// 拼接两个 `PATH` 字符串，保留顺序，对条目去重（Windows 文件系统不
/// 区分大小写，因此比较也不区分大小写），并跳过空字段。
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
    // 用户 PATH 优先——用户显式放置的路径胜过来自系统继承的任何条目，
    // 这与 Explorer 拼接两者以及 `cmd.exe` 解析裸名时的行为一致。
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
        // 用户条目优先；与 `C:\Program Files\nodejs` 大小写不同的重复条目
        // 被合并掉；系统 `C:\Windows` 保留在末尾。
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
