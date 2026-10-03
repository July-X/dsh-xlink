//! 登录自启：让壳在用户登录后自动进后台。
//!
//! **只拉起壳，不拉起内核**（用户 2026-10-02 拍板）。理由是内核的代价
//! 摆在开机那一刻最难看：占端口、起 node 进程、订阅事件流，而用户可能
//! 开机后几小时内根本不开工作台。壳本身很轻，进后台不打扰；真要用时点
//! 菜单栏 / 托盘图标。要连内核一起起另设一个开关（`autostart_kernel`），
//! 它默认关闭。
//!
//! 自启拉起时**不显示面板**（同上拍板）：开机弹一个窗口挡在用户面前，
//! 正是自动启动最招人烦的地方。菜单栏 / 托盘图标就是全部可见痕迹。
//!
//! ## 平台机制
//!
//! 两端都只写**当前用户**的登录项，不需要管理员权限，也不需要任何外部
//! 工具：
//!
//! - macOS：`~/Library/LaunchAgents/<label>.plist`（`RunAtLoad`）。不用
//!   `SMAppService`——它要求应用是签名且装在 `/Applications` 下，开发期
//!   `npm run dev` 直接跑 `target/debug/dsh-xlink` 时注册会静默失败，而
//!   这恰恰是最需要验证的场景。LaunchAgent 只认「登录时跑这个可执行文件」，
//!   对可执行文件在哪儿没有要求。
//! - Windows：`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`（`windows-sys`
//!   已在依赖里，`tray.rs` 也已经用它读主题键，无新增依赖）。
//!
//! ## 写什么命令
//!
//! 两端都写 `<可执行文件路径> --autostart`。**路径不做任何假设**：dev 构建
//! 指向 `target/debug/`，release 指向 `.app/Contents/MacOS/` 或用户选的安装
//! 位置。用 `std::env::current_exe()` 现取，装到别处、移动二进制位置之后
//! 重新勾一次开关即可更新。
//!
//! 用户可能整目录搬家或换机器，此时旧条目指向已不存在的路径。系统不会
//! 报错，只会静默什么都不发生——所以 [`status`] 额外核对条目里的路径与
//! 当前可执行文件是否一致，不一致时报告 `stale` 而让面板提示重勾。

use std::path::{Path, PathBuf};

use crate::shell::resident::AUTOSTART_FLAG;

/// macOS 登录项的 plist 文件名。**dev 与 release 各自一份**——两个壳是
/// 独立应用（不同的可执行文件路径），共用一个文件名会让先启用的那个把
/// 后启用的条目覆盖掉，用户于是得到「开机启动的是另一个壳」。
fn agent_label() -> String {
    match crate::shell::paths::ShellMode::current() {
        crate::shell::paths::ShellMode::Release => "com.july-x.dsh-xlink".to_string(),
        crate::shell::paths::ShellMode::Dev => "com.july-x.dsh-xlink.dev".to_string(),
    }
}

/// 自启开关的当前状态。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutostartStatus {
    /// 系统登录项里有没有我们的条目。
    pub enabled: bool,
    /// 当前平台是否支持自启（两个发布平台都支持）。
    pub supported: bool,
    /// 条目存在但指向的不是当前可执行文件（搬过目录 / 换了构建）。
    /// 这种情况下 `enabled` 仍是 true，但系统实际拉不起来。
    pub stale: bool,
    /// 「登录时启动工作台」当前是否生效。它是壳自己的设置、不是系统登录项，
    /// 但两者总是一起被看，所以随同一个命令回给面板，省掉一次往返。
    pub kernel: bool,
    /// 给用户看的下一步。没有需要处理的事时为 `None`。
    pub note: Option<String>,
}

/// 自启是否生效（条目在，且指向当前可执行文件）。
pub fn enabled() -> bool {
    status()
        .map(|status| status.enabled && !status.stale)
        .unwrap_or(false)
}

/// 读当前状态。读失败按「未启用」处理并说明原因——不能把读不出来说成
/// 「没开」，那会让面板上的开关与系统实际状态对不上。
pub fn status() -> Result<AutostartStatus, String> {
    let expected = current_exe()?;
    let kernel = kernel_enabled();
    let mut status = match read_entry() {
        Some(stored) if stored == expected.display().to_string() => AutostartStatus {
            enabled: true,
            supported: true,
            stale: false,
            kernel,
            note: None,
        },
        Some(stored) => AutostartStatus {
            enabled: true,
            supported: true,
            stale: true,
            kernel,
            note: Some(format!(
                "自启条目仍指向旧位置（{stored}），系统登录时拉不起当前这个版本。\
                 关掉再打开开关即可更新。"
            )),
        },
        None => AutostartStatus {
            enabled: false,
            supported: true,
            stale: false,
            kernel,
            note: None,
        },
    };
    // 「开机启动工作台」是依附于「开机自启动」的：没开后者时它不生效，把话
    // 讲出来比让用户以为两个开关各管各的更省事。**引号里的名字必须与
    // `SettingsPanel.vue` 的 label 逐字一致**——写错一个���，用户在界面上就
    // 找不到这条提示在说哪个开关。
    if !status.enabled && status.kernel {
        status.note = Some("「开机启动工作台」需要先打开「开机自启动」才会生效".to_string());
    }
    Ok(status)
}

/// 打开 / 关闭登录自启。
///
/// 关闭时**只删我们自己的那一条**。Windows 上 `Run` 键里还有其它程序的值，
/// macOS 上 `LaunchAgents` 里还有别人的 plist，误删会让用户丢掉一个正在用的
/// 应用的自动启动。
pub fn set_enabled(enabled: bool) -> Result<(), String> {
    if enabled {
        write_entry(&current_exe()?)
    } else {
        remove_entry()
    }
}

/// 当前可执行文件路径。自启条目存的就是它（平台层再补上 `--autostart`）。
fn current_exe() -> Result<PathBuf, String> {
    std::env::current_exe()
        .map_err(|error| format!("无法确定当前程序的路径，自动启动未生效：{error}"))
}

/// 命令串的拼装。抽成函数是为了平台层与测试共用一份——Windows 的
/// 注册表值与 macOS 的 plist 数组是**同一串东西的两种存法**，拼装两处
/// 写必然会漂（漂了就是「自启命令里没有 `--autostart`」，于是开机弹面板）。
fn format_command(exe: &Path) -> String {
    format!("\"{}\" {}", exe.display(), AUTOSTART_FLAG)
}

#[cfg(target_os = "macos")]
mod platform {
    use super::PathBuf;
    use std::path::Path;

    /// `~/Library/LaunchAgents/<label>.plist`。
    ///
    /// 直接在 `~/Library/LaunchAgents` 上写，不问系统：这条路径由 launchd
    /// 约定，目录本身通常已存在（不存在就建）。`SMAppService` 那套 API 要
    /// 应用已签名且装在 `/Applications`，开发期直接跑 `target/debug/` 的
    /// 壳注册会静默失败——而那正是最需要验证的场景。
    fn agent_file() -> PathBuf {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .expect("HOME 一定存在：launchd 以用户身份拉起本进程");
        home.join("Library")
            .join("LaunchAgents")
            .join(format!("{}.plist", super::agent_label()))
    }

    /// 从 plist 里取出 `ProgramArguments` 的第一项（可执行文件路径）。
    ///
    /// 刻意**不引 XML 解析依赖**：plist 是我们自己写出去的，格式固定，
    /// 按行找 `<string>` 的那一行足够。反过来，引一个 plist 库会把
    /// 「写一个 12 行的文件」变成新增依赖与新增失败模式。
    pub(super) fn read_entry() -> Option<String> {
        let text = std::fs::read_to_string(agent_file()).ok()?;
        let mut in_args = false;
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("<key>ProgramArguments</key>") {
                in_args = true;
                continue;
            }
            if in_args {
                if trimmed.starts_with("</array>") {
                    return None;
                }
                if let Some(value) = trimmed
                    .strip_prefix("<string>")
                    .and_then(|rest| rest.strip_suffix("</string>"))
                {
                    return Some(value.to_string());
                }
            }
        }
        None
    }

    pub(super) fn write_entry(exe: &Path) -> Result<(), String> {
        let file = agent_file();
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("无法创建登录项目录 {}：{error}", parent.display()))?;
        }
        // `RunAtLoad` = 登录时拉起。我们不写 `KeepAlive`：壳退出后不该被
        // 再拉回来——「退出 dsh-xlink」必须是终态，被 launchd 复活一次就
        // 成了「退不掉」。
        let plist = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
             \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
             <plist version=\"1.0\">\n\
             <dict>\n\
             \x20 <key>Label</key>\n\
             \x20 <string>{}</string>\n\
             \x20 <key>ProgramArguments</key>\n\
             \x20 <array>\n\
             \x20  <string>{}</string>\n\
             \x20  <string>{}</string>\n\
             \x20 </array>\n\
             \x20 <key>RunAtLoad</key>\n\
             \x20 <true/>\n\
             \x20 <key>ProcessType</key>\n\
             \x20 <string>Interactive</string>\n\
             </dict>\n\
             </plist>\n",
            super::agent_label(),
            exe.display(),
            super::AUTOSTART_FLAG,
        );
        crate::shell::process::atomic_write(&file, plist.as_bytes())
            .map_err(|error| format!("写入登录项 {} 失败：{error}", file.display()))
    }

    pub(super) fn remove_entry() -> Result<(), String> {
        let file = agent_file();
        match std::fs::remove_file(&file) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("删除登录项 {} 失败：{error}", file.display())),
        }
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use std::path::Path;

    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW,
        RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, REG_SZ,
    };

    /// 当前用户的 `Run` 键。不碰 HKLM——那需要管理员权限，而每个用户
    /// 都能给自己开自启，没有理由要求提权。
    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    /// 我们的值名。与 plist 的 label 一样按壳模式分家，dev 与 release
    /// 各注册自己那一份。
    fn value_name() -> String {
        super::agent_label()
    }

    fn run_key() -> HKEY {
        use windows_sys::Win32::Foundation::ERROR_SUCCESS;
        let subkey: Vec<u16> = RUN_KEY.encode_utf16().chain(std::iter::once(0)).collect();
        let mut key = std::ptr::null_mut();
        // SAFETY: `RegOpenKeyExW` 只写 `key`，参数全部是我们自己持有的
        // 缓冲，长度由 NUL 结尾保证。
        let status =
            unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, subkey.as_ptr(), 0, KEY_READ, &mut key) };
        if status != ERROR_SUCCESS {
            return std::ptr::null_mut();
        }
        key
    }

    pub(super) fn read_entry() -> Option<String> {
        let key = run_key();
        if key.is_null() {
            return None;
        }
        let name: Vec<u16> = value_name()
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let mut kind: u32 = 0;
        // 先问大小再问内容：REG_SZ 的长度含结尾 NUL，值本身可能随路径长度变化。
        let mut len: u32 = 0;
        // SAFETY: 全部缓冲由本函数持有，`len` 是 in/out。
        let status = unsafe {
            RegQueryValueExW(
                key,
                name.as_ptr(),
                std::ptr::null(),
                &mut kind,
                std::ptr::null_mut(),
                &mut len,
            )
        };
        if status != 0 || kind != REG_SZ || len == 0 {
            unsafe { RegCloseKey(key) };
            return None;
        }
        let mut buf = vec![0u16; len.div_ceil(2) as usize + 1];
        let mut len = len;
        // SAFETY: `buf` 至少 `len` 字节，`len` 是 in/out。
        let status = unsafe {
            RegQueryValueExW(
                key,
                name.as_ptr(),
                std::ptr::null(),
                &mut kind,
                buf.as_mut_ptr() as *mut u8,
                &mut len,
            )
        };
        unsafe { RegCloseKey(key) };
        if status != 0 {
            return None;
        }
        let text = String::from_utf16_lossy(&buf);
        let text = text.trim_end_matches('\0').trim();
        (!text.is_empty()).then(|| exe_path_of(text)).flatten()
    }

    /// 从注册表值里取出可执行文件路径。
    ///
    /// 值是 `"<路径>" --autostart`，而 macOS 的 plist 里第一项就是路径。
    /// **两边都必须归一成裸路径再往外交**，否则同一个比较函数在两端得到
    /// 两种形态：macOS 那边是路径、Windows 这边是整串命令，stale 判定会
    /// 永远为真，用户每次进设置页都被告知「条目已失效」。
    fn exe_path_of(command: &str) -> Option<String> {
        let quoted = command.starts_with('"');
        let rest = if quoted {
            command.strip_prefix('"')?.split_once('"')?.0
        } else {
            command.split_whitespace().next()?
        };
        let rest = rest.trim();
        (!rest.is_empty()).then(|| rest.to_string())
    }

    pub(super) fn write_entry(exe: &Path) -> Result<(), String> {
        use windows_sys::Win32::Foundation::ERROR_SUCCESS;
        let subkey: Vec<u16> = RUN_KEY.encode_utf16().chain(std::iter::once(0)).collect();
        let name: Vec<u16> = value_name()
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let mut key = std::ptr::null_mut();
        // SAFETY: 同上；KEY_SET_VALUE 而非 KEY_ALL_ACCESS，最小权限。
        // 第 9 个参数 `lpdwdisposition`（这次调用是「是新建还是打开」的出参）
        // 2026-10-03 之前漏传，整个 Windows 构建直接编译不过：windows-sys 0.61
        // 起经 `windows_link::link!` 生成的签名是完整的 9 参数版本，只要它、
        // 传空指针表示「不关心」即可（winreg 的等价调用也是这么做的）。
        // **本地 macOS 的 `cargo check` 永远发现不了这一类错误**——整段代码在
        // `#[cfg(target_os = "windows")]` 里，见本文件顶部的门控说明。
        let status = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                subkey.as_ptr(),
                0,
                std::ptr::null(),
                0,
                KEY_SET_VALUE,
                std::ptr::null(),
                &mut key,
                std::ptr::null_mut(),
            )
        };
        if status != ERROR_SUCCESS || key.is_null() {
            return Err("无法打开当前用户的启动项注册表键，自动启动未生效".to_string());
        }
        // REG_SZ 的字节数含结尾 NUL。
        let wide: Vec<u16> = super::format_command(exe)
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let bytes = (wide.len() * std::mem::size_of::<u16>()) as u32;
        // SAFETY: `wide` 在调用期间存活，字节数按 REG_SZ 规则算。
        let status = unsafe {
            RegSetValueExW(
                key,
                name.as_ptr(),
                0,
                REG_SZ,
                wide.as_ptr() as *const u8,
                bytes,
            )
        };
        unsafe { RegCloseKey(key) };
        if status == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(format!("写入启动项注册表值失败（状态码 {status}）"))
        }
    }

    pub(super) fn remove_entry() -> Result<(), String> {
        use windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND;
        let subkey: Vec<u16> = RUN_KEY.encode_utf16().chain(std::iter::once(0)).collect();
        let name: Vec<u16> = value_name()
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let mut key = std::ptr::null_mut();
        // SAFETY: 同上；只开 KEY_READ 不足以删值，这里按需提权到 KEY_SET_VALUE。
        let status = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                subkey.as_ptr(),
                0,
                KEY_SET_VALUE,
                &mut key,
            )
        };
        if status != 0 || key.is_null() {
            return Ok(());
        }
        // SAFETY: 键句柄有效，值名以 NUL 结尾。
        let status = unsafe { RegDeleteValueW(key, name.as_ptr()) };
        unsafe { RegCloseKey(key) };
        if status == 0 || status == ERROR_FILE_NOT_FOUND {
            Ok(())
        } else {
            Err(format!("删除启动项注册表值失败（状态码 {status}）"))
        }
    }
}

/// 平台相关的读 / 写 / 删。macOS 与 Windows 之外（项目不发布）一律报
/// 「不支持」，而不是假装成功——面板上的开关必须反映真实能力。
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod platform {
    use std::path::Path;

    pub(super) fn read_entry() -> Option<String> {
        None
    }

    pub(super) fn write_entry(_exe: &Path) -> Result<(), String> {
        Err("当前平台不支持自动启动".to_string())
    }

    pub(super) fn remove_entry() -> Result<(), String> {
        Ok(())
    }
}

use platform::{read_entry, remove_entry, write_entry};

/// 读自启状态（面板进入设置页时调用）。
///
/// 纯读，不碰任何用户数据。
#[tauri::command]
pub fn autostart_status() -> Result<AutostartStatus, String> {
    status()
}

/// 打开 / 关闭登录自启，返回生效后的状态供 UI 回显。
///
/// 写的是**系统登录项**，失败必须如实回给用户——不能吞掉后让开关显示成
/// 已打开：那样用户以为开机自启了，实际每次都得手点。
#[tauri::command]
pub fn autostart_set(enabled: bool) -> Result<AutostartStatus, String> {
    set_enabled(enabled)?;
    status()
}

/// 打开 / 关闭「登录后自动拉起工作台」。返回生效后的取值。
///
/// 刻意与 [`autostart_set`] 分成两条命令：登录项本身归系统管（写在
/// `~/Library/LaunchAgents` 与注册表里，删应用时不残留），而这个开关只是
/// 一条壳自己的设置。合成一条会让「关掉自动启动」连带把「开机不自动起内核」
/// 这个独立偏好也一起改掉。
#[tauri::command]
pub fn autostart_set_kernel(enabled: bool) -> Result<bool, String> {
    let mode = crate::shell::settings::current_mode();
    let mut current = crate::shell::settings::load_for_shell(mode);
    current.autostart_kernel = Some(enabled);
    crate::shell::settings::save_for_shell(mode, &current).map_err(|e| e.to_string())?;
    Ok(enabled)
}

/// 「登录后自动拉起工作台」当前是否生效。随 [`autostart_status`] 一起回给
/// 面板，让两个开关能在一处初始化。
pub fn kernel_enabled() -> bool {
    crate::shell::settings::autostart_kernel_enabled(&crate::shell::settings::load_for_shell(
        crate::shell::settings::current_mode(),
    ))
}

/// 登录自启拉起时，要不要顺带把工作台也启动。
///
/// 判定是三重的，缺一个都会做出错误决定：
/// ① 用户显式开了「登录后自动启动工作台」；
/// ② 本次进程真的是登录项拉起来的（手动点图标启动时**不该**顺带起内核——
///    那等于用户想开面板，结果被动占了一个端口）；
/// ③ 目标内核版本存在。版本没装时启动只会失败并留下一条错误日志，而开机时
///    用户不在跟前，看不到也处理不了。
pub fn should_launch_kernel_on_autostart(active_version: Option<&str>) -> bool {
    should_launch_kernel_given(
        kernel_enabled(),
        crate::shell::resident::started_by_autostart(),
        active_version,
    )
}

/// [`should_launch_kernel_on_autostart`] 的纯逻辑部分：三个条件显式传进来。
///
/// 抽出它**不是为了好看，是因为不抽就测不了**：真实判据里第一个条件
/// `kernel_enabled()` 读的是磁盘设置，测试机上它多半是 `None`（默认关），
/// 于是无论「本次是否自启拉起」这一条判错多少，测试都因为短路在第一个条件
/// 而全绿。2026-10-02 首次反向验就抓到了这一点：把「是不是自启拉起」的判定
/// 整行删掉，`manual_launch_never_starts_the_kernel` 依然通过。
///
/// 现在三条件都显式传参，测试可以逐条打开——包括构造「开关已开」这个
/// 平时难造出来的状态。
pub fn should_launch_kernel_given(
    kernel_switch_on: bool,
    launched_by_autostart: bool,
    active_version: Option<&str>,
) -> bool {
    kernel_switch_on
        && launched_by_autostart
        && active_version.is_some_and(|version| !version.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 命令串必须带引号：路径里有空格时（`C:\Program Files\...`、
    /// macOS 上带空格的目录）不加引号，注册表与 launchd 都会在第一个空格
    /// 处截断，登录时拉起的是一段不存在的路径。
    #[test]
    fn command_quotes_the_executable_path() {
        let command = format_command(Path::new("/Applications/Dsh Xlink.app/Contents/MacOS/app"));
        assert_eq!(
            command,
            "\"/Applications/Dsh Xlink.app/Contents/MacOS/app\" --autostart"
        );
    }

    /// 每条自启命令都必须带 `--autostart`，否则壳会在登录时被当成一次
    /// 正常启动、**弹出面板挡在用户面前**——那正是自动启动最招人烦的地方。
    #[test]
    fn command_always_carries_the_autostart_flag() {
        let command = format_command(Path::new("/usr/local/bin/dsh-xlink"));
        assert!(
            crate::shell::resident::detect_autostart_arg(command.split(' ').map(str::to_string)),
            "自启命令里必须能识别出 --autostart：{command}"
        );
    }

    /// 同一份二进制必须算出同一条路径，否则 stale 判定永远为真，用户每次
    /// 进设置页都被告知「条目已失效」。
    #[test]
    fn same_executable_yields_same_path() {
        let exe = Path::new("/usr/local/bin/dsh-xlink");
        assert_eq!(exe.display().to_string(), exe.display().to_string());
        // 命令串也要可逆：剥掉引号与参数后必须拿回原路径。
        #[cfg(target_os = "windows")]
        assert_eq!(
            platform::exe_path_of(&format_command(exe)).as_deref(),
            Some("/usr/local/bin/dsh-xlink")
        );
    }

    /// 归一化必须扛住**路径里有空格**：不带引号的解析会在第一个空格处
    /// 截断，于是 `C:\Program Files\...` 被读成 `C:\Program`，stale 误报。
    #[test]
    #[cfg(target_os = "windows")]
    fn exe_path_keeps_spaces_inside_quotes() {
        let command = format_command(Path::new("C:\\Program Files\\dsh-xlink.exe"));
        assert_eq!(
            platform::exe_path_of(&command).as_deref(),
            Some("C:\\Program Files\\dsh-xlink.exe")
        );
    }

    /// 归一化对没有空格的裸路径也要成立——plist 里存的就是裸路径。
    #[test]
    #[cfg(target_os = "windows")]
    fn exe_path_accepts_unquoted_bare_path() {
        assert_eq!(
            platform::exe_path_of("/usr/local/bin/dsh-xlink --autostart").as_deref(),
            Some("/usr/local/bin/dsh-xlink")
        );
    }

    /// 搬过目录后必须判定为「不是同一条路径」，由上层报 stale 并提示重勾。
    #[test]
    fn moved_executable_is_detected_as_stale() {
        let stored = "/usr/local/bin/dsh-xlink";
        let expected = "/tmp/dsh-xlink-new";
        assert_ne!(stored, expected);
    }

    /// 自启的读 / 写 / 删会**碰到用户真实的登录项**（`~/Library/LaunchAgents`、
    /// `HKCU\...\Run`）。测试绝不能去动它——把条目删掉等于用户下次开机
    /// 少一个程序；改写它更糟。这里只验证「本模块能编译并给出可读的状态」，
    /// 真实的写路径由人工在装好的机器上验证。
    #[test]
    fn status_is_readable_without_touching_entries() {
        let status = status().expect("status 不应失败");
        assert!(status.supported, "两个发布平台都应支持自启");
        if status.stale {
            assert!(
                status.note.is_some(),
                "判定为 stale 时必须给用户一条可执行的下一步：{status:?}"
            );
        }
    }

    /// 三条件**逐一**构造，缺一个都不许通过。
    ///
    /// 逐条分开写（而不是只写一个「全开时为 true」）是必须的：把三个条件
    /// 用 `&&` 连起来写一个正例，把其中任意一条改坏都可能仍被其余两条掩盖
    /// ——2026-10-02 首次反向验正是这样翻车的：测试只调真实判据，而真实判据
    /// 第一个条件读磁盘（测试机上默认关），于是删掉第二条判据测试依然绿。
    #[test]
    fn all_three_conditions_are_required() {
        // 基准：三条都满足。
        assert!(
            should_launch_kernel_given(true, true, Some("0.2.0")),
            "开关开 + 自启拉起 + 版本已装 = 应该起内核"
        );

        // ① 手动启动（不是自启拉起）→ 不起。哪怕开关开着。
        assert!(
            !should_launch_kernel_given(true, false, Some("0.2.0")),
            "手动点图标启动不该顺带拉起内核：用户只想开面板，却被动占掉一个端口"
        );

        // ② 开关没开 → 不起。哪怕是自启拉起、版本也在。
        assert!(
            !should_launch_kernel_given(false, true, Some("0.2.0")),
            "没开「登录时启动工作台」就不该起内核"
        );

        // ③ 版本没装 / 是空白 → 不起。
        assert!(!should_launch_kernel_given(true, true, None));
        assert!(!should_launch_kernel_given(true, true, Some("")));
        assert!(!should_launch_kernel_given(true, true, Some("   ")));
    }

    /// 真实判据只做「读设置 + 读启动来源」两件事转发。**不测它本身**：
    /// 它的两个输入都来自进程外的真实状态（磁盘设置 / 本次启动来源），
    /// 在测试里改它们要么污染用户数据、要么是全局单例互相干扰。
    /// 纯逻辑由上一条覆盖，判据接线由 `setup` 里那一行调用保证。
    #[test]
    fn real_predicate_reads_current_state_without_side_effects() {
        // 手工点图标启动过一次之后，本进程不再是「自启拉起」。
        crate::shell::resident::mark_started_by_autostart(false);
        assert!(!crate::shell::resident::started_by_autostart());
        // 调它一次必须不崩、不改任何全局状态（这是纯查询）。
        let _ = should_launch_kernel_on_autostart(Some("0.2.0"));
        let _ = should_launch_kernel_on_autostart(None);
    }
}
