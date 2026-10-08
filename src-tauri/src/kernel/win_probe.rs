//! Windows 原生探测：进程命令行（读 PEB）与 TCP 监听表（GetExtendedTcpTable）。
//!
//! 为什么存在：状态轮询每 2.5s 要回答两个问题——「这个 pid 的命令行是什么」
//! （内核身份识别）与「这个端口谁在监听」（运行判定与端口归属）。2026-10-08
//! 的 perf 采样（`shell/logs/*-perf-status-*.log`）显示这两个问题在 Windows 上
//! 的既有答案各要派生一次重量级操作，把 `kernel_workbench_running` 段推到
//! p50 404ms / p95 690ms（内核运行期间该段累计占墙钟 12.6%）：
//!
//! - 命令行 → `powershell.exe Get-CimInstance`：本机实测单次 ~349ms。缓存
//!   TTL 3s 对 2.5s 的轮询只能隔次命中，等于内核运行期间每分钟派生约
//!   13 个 PowerShell 进程（每次都是一遍 .NET 运行时冷启）。
//! - 端口 → `TcpStream::connect_timeout(400ms)`：部分机器的防火墙对**无
//!   监听者**的回环端口静默丢包而不是立即 RST，于是「确认没在跑」每 2.5s
//!   白等满 400ms（实测 dev 壳 3 小时 564 次轮询次次卡在 403–415ms）。
//!
//! 两个原生 API 都是一次系统调用级别的查询，不派生进程、不发包。与 macOS
//! 侧换 `sysctl(KERN_PROCARGS2)` 是同一个决策——「别在轮询路径上派生子进程」
//! （见 `lifecycle.rs::process_command` 的平台分工说明）。
//!
//! 失败（进程已退出、权限不足、布局不认识）一律返回 `None`：快路径只许快、
//! 不许改变语义，调用方按既有路径回退。
//!
//! 与旧 `netstat -ano` 解析的一处**刻意差异**：旧解析会把 `[::1]:port` 的
//! IPv6 监听者也当成占用者，而本模块只查 v4 表（127.0.0.1 / 0.0.0.0）。
//! 内核绑的是 v4 回环，`[::1]` 上的监听并不占用它——旧 `port_open` 的
//! connect 判据本来也判不出 IPv6 监听，改表查询后两个判据从此一致。

use windows_sys::Wdk::System::Threading::{NtQueryInformationProcess, ProcessBasicInformation};
use windows_sys::Win32::Foundation::{CloseHandle, ERROR_INSUFFICIENT_BUFFER, HANDLE, NO_ERROR};
use windows_sys::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, MIB_TCPROW_OWNER_PID, MIB_TCPTABLE_OWNER_PID, MIB_TCP_STATE_LISTEN,
    TCP_TABLE_OWNER_PID_LISTENER,
};
use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_BASIC_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
};

/// 读进程命令行（原生路径）。查不到（进程已退出 / 权限不足 / 布局不认识）
/// 返回 `None`，由调用方回退既有实现——与 macOS `process_command_procargs`
/// 的约定相同。
pub(crate) fn process_command_line(pid: u32) -> Option<String> {
    // PEB 偏移只对 x64 核过（发布通道只有 x86_64）；其它架构直接交给回退。
    #[cfg(target_arch = "x86_64")]
    {
        process_command_line_peb(pid)
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        let _ = pid;
        None
    }
}

/// x64 布局偏移：`PEB.ProcessParameters` 在 PEB+0x20；
/// `RTL_USER_PROCESS_PARAMETERS.CommandLine`（UNICODE_STRING）在参数块+0x70，
/// 其 `Buffer` 指针在 UNICODE_STRING 内 +0x8。
#[cfg(target_arch = "x86_64")]
mod x64 {
    pub(crate) const PEB_PROCESS_PARAMETERS: usize = 0x20;
    pub(crate) const PARAMS_COMMAND_LINE: usize = 0x70;
    pub(crate) const UNICODE_STRING_BUFFER: usize = 0x8;
    /// 命令行长度的常识上界（字节，UTF-16）：真进程到不了，垃圾数据到得了。
    pub(crate) const MAX_COMMAND_LINE_BYTES: usize = 32 * 1024;
}

/// 关闭用毕的进程句柄。`OpenProcess` 的失败值是 null 句柄，成功值必须
/// `CloseHandle`，漏一个就占住目标进程的引用计数。
#[cfg(target_arch = "x86_64")]
struct HandleGuard(HANDLE);

#[cfg(target_arch = "x86_64")]
impl Drop for HandleGuard {
    fn drop(&mut self) {
        // Safety: self.0 一定来自 OpenProcess 的成功返回，且只关闭一次。
        unsafe { CloseHandle(self.0) };
    }
}

#[cfg(target_arch = "x86_64")]
fn process_command_line_peb(pid: u32) -> Option<String> {
    // Safety: 句柄由 OpenProcess 按 pid 打开、由 HandleGuard 保证关闭；
    // NtQueryInformationProcess / ReadProcessMemory 都是纯查询，不修改目标
    // 进程；所有读取的目标地址与长度先经校验，缓冲区由本函数分配。
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, 0, pid);
        if handle.is_null() {
            return None; // 进程已退出 / 权限不足
        }
        let _guard = HandleGuard(handle);

        // PROCESS_BASIC_INFORMATION 不实现 Default：作纯出参用 zeroed 初始化，
        // 只在 NtQueryInformationProcess 成功后才读它。
        let mut info: PROCESS_BASIC_INFORMATION = std::mem::zeroed();
        let mut returned = 0u32;
        let status = NtQueryInformationProcess(
            handle,
            ProcessBasicInformation,
            &mut info as *mut _ as *mut core::ffi::c_void,
            std::mem::size_of::<PROCESS_BASIC_INFORMATION>() as u32,
            &mut returned,
        );
        // NTSTATUS 成功（含 informational）≥ 0；PebBaseAddress 为空的只有
        // 已退出或系统闲进程这类没有用户态 PEB 的目标。
        if status < 0 || info.PebBaseAddress.is_null() {
            return None;
        }
        let peb = info.PebBaseAddress as usize;

        let params_ptr = read_usize(handle, peb + x64::PEB_PROCESS_PARAMETERS)?;
        // UNICODE_STRING { Length: u16, MaximumLength: u16, 对齐填充: u32, Buffer: *u16 }
        let unicode_string = read_bytes(handle, params_ptr + x64::PARAMS_COMMAND_LINE, 16)?;
        let length = u16::from_ne_bytes([unicode_string[0], unicode_string[1]]) as usize;
        let buffer = usize::from_ne_bytes(unicode_string[8..16].try_into().ok()?);
        if length == 0 || length % 2 != 0 || length > x64::MAX_COMMAND_LINE_BYTES || buffer == 0 {
            return None;
        }
        let raw = read_bytes(handle, buffer, length)?;
        let units: Vec<u16> = raw
            .chunks_exact(2)
            .map(|chunk| u16::from_ne_bytes([chunk[0], chunk[1]]))
            .collect();
        let command = String::from_utf16_lossy(&units);
        // 与 PowerShell 回退同一口径：trim 后仍要有内容，返回原文（未 trim）。
        (!command.trim().is_empty()).then_some(command)
    }
}

// Safety（两个 read helper）：只读目标进程内存；`read` 回报的字节数必须
// 等于请求量，否则视为查不到——部分读出的地址不可信。
#[cfg(target_arch = "x86_64")]
unsafe fn read_usize(handle: HANDLE, addr: usize) -> Option<usize> {
    let mut buf = [0u8; size_of_usize()];
    let mut read = 0usize;
    if ReadProcessMemory(
        handle,
        addr as *const core::ffi::c_void,
        buf.as_mut_ptr() as *mut core::ffi::c_void,
        buf.len(),
        &mut read,
    ) == 0
        || read != buf.len()
    {
        return None;
    }
    Some(usize::from_ne_bytes(buf))
}

#[cfg(target_arch = "x86_64")]
unsafe fn read_bytes(handle: HANDLE, addr: usize, len: usize) -> Option<Vec<u8>> {
    let mut buf = vec![0u8; len];
    let mut read = 0usize;
    if ReadProcessMemory(
        handle,
        addr as *const core::ffi::c_void,
        buf.as_mut_ptr() as *mut core::ffi::c_void,
        buf.len(),
        &mut read,
    ) == 0
        || read != buf.len()
    {
        return None;
    }
    Some(buf)
}

#[cfg(target_arch = "x86_64")]
const fn size_of_usize() -> usize {
    std::mem::size_of::<usize>()
}

/// 正在监听 `127.0.0.1:port`（或 `0.0.0.0:port`，全零绑定同样占用 v4 回环
/// 端口）的 TCP 进程 pid；无人监听或查询失败返回 `None`。
pub(crate) fn tcp_listener_pid(port: u16) -> Option<u32> {
    listener_rows()?
        .iter()
        .find(|row| row_matches(row, port))
        .map(|row| row.dwOwningPid)
}

/// TCP 监听表快照。`TCP_TABLE_OWNER_PID_LISTENER` 表类只回 LISTEN 行，查表
/// 无需任何权限（owner-pid 表对普通进程开放），失败几乎只可能是内存不足。
fn listener_rows() -> Option<Vec<MIB_TCPROW_OWNER_PID>> {
    // WinSock::AF_INET 的同值字面量：只为这一个常量开整个 WinSock 特性不值。
    const AF_INET: u32 = 2;
    // Safety: GetExtendedTcpTable 是纯查询；缓冲区按第一次调用回报的大小
    // 分配，第二次调用写入量在该容量内。
    unsafe {
        let mut size = 0u32;
        GetExtendedTcpTable(
            std::ptr::null_mut(),
            &mut size,
            0,
            AF_INET,
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        );
        if size == 0 {
            return None;
        }
        let mut buf = vec![0u8; size as usize];
        let rc = GetExtendedTcpTable(
            buf.as_mut_ptr() as *mut core::ffi::c_void,
            &mut size,
            0,
            AF_INET,
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        );
        if rc != NO_ERROR && rc != ERROR_INSUFFICIENT_BUFFER {
            return None;
        }
        let table = buf.as_ptr() as *const MIB_TCPTABLE_OWNER_PID;
        let count = (*table).dwNumEntries as usize;
        if count == 0 {
            return None;
        }
        // 变长尾数组：table 字段只是首元素，行区按条数展开。
        let rows = std::slice::from_raw_parts((*table).table.as_ptr(), count);
        Some(rows.to_vec())
    }
}

/// 行匹配：LISTEN 状态、本地端口等于 `port`、本地地址是 v4 回环或全零。
/// `dwLocalAddr` / `dwLocalPort` 都是**网络字节序**（端口在低 16 位），
/// 与旧 netstat 解析「本地地址列以 `:PORT` 结尾」在 v4 上等价；数值比较
/// 从根上没有 `:3090` 命中 `:30900` 那类子串歧义（P2-5）。
fn row_matches(row: &MIB_TCPROW_OWNER_PID, port: u16) -> bool {
    const LOOPBACK_V4: u32 = u32::from_le_bytes([127, 0, 0, 1]);
    const ANY_V4: u32 = 0;
    let local_port = u16::from_be((row.dwLocalPort & 0xFFFF) as u16);
    row.dwState == MIB_TCP_STATE_LISTEN as u32
        && local_port == port
        && (row.dwLocalAddr == LOOPBACK_V4 || row.dwLocalAddr == ANY_V4)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 端到端：读**自己**的命令行。测试二进制名必然出现在 PEB 的
    /// CommandLine 里——这条同时验证了三个偏移、UNICODE_STRING 布局与
    /// UTF-16 解码全程无白盒假设。
    #[test]
    fn process_command_line_reads_this_process() {
        let command = process_command_line(std::process::id()).expect("读自身命令行不该失败");
        let exe = std::env::current_exe().expect("测试进程有 exe 路径");
        let name = exe.file_name().unwrap().to_string_lossy().into_owned();
        assert!(
            command.contains(&name),
            "命令行 {command:?} 里应当含测试二进制名 {name:?}"
        );
    }

    /// 端到端：自建一个监听回环临时端口的 socket，表查询必须找到它并把
    /// 属主报成本进程——端口与地址的网络字节序匹配在这条里被真实地验过。
    #[test]
    fn tcp_listener_pid_finds_a_socket_we_own() {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind 回环临时端口");
        let port = listener.local_addr().unwrap().port();
        assert_eq!(tcp_listener_pid(port), Some(std::process::id()));
        drop(listener);
        // 关闭后端口可能被短暂保留，等它从表里消失（上限 2s）——
        // 残留期间属主仍应是本进程或空，绝不可能是别的监听者。
        for _ in 0..20 {
            if tcp_listener_pid(port).is_none() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        panic!("关闭监听 2s 后端口 {port} 仍在监听表中");
    }

    /// 行匹配的字节序与边界：端口按网络序存于低 16 位；ESTABLISHED 行与
    /// 非回环地址不算。旧解析靠 5 列形状区分的 UDP / 表头行，在类型化的
    /// 表行里天然不存在。
    #[test]
    fn row_matches_decodes_network_byte_order_port() {
        let row = |addr: [u8; 4], port: u16| MIB_TCPROW_OWNER_PID {
            dwState: MIB_TCP_STATE_LISTEN as u32,
            dwLocalAddr: u32::from_le_bytes(addr),
            dwLocalPort: port.to_be() as u32,
            dwRemoteAddr: 0,
            dwRemotePort: 0,
            dwOwningPid: 42,
        };
        assert!(row_matches(&row([127, 0, 0, 1], 3090), 3090));
        assert!(row_matches(&row([0, 0, 0, 0], 3090), 3090));
        // 数值比较没有 :3090 命中 :30900 的子串歧义。
        assert!(!row_matches(&row([127, 0, 0, 1], 30900), 3090));
        // 已连接不算监听。
        let mut established = row([127, 0, 0, 1], 3090);
        established.dwState = 5; // MIB_TCP_STATE_ESTAB
        assert!(!row_matches(&established, 3090));
        // 别人绑在非回环 / 非全零地址上的端口不算回环占用。
        assert!(!row_matches(&row([192, 168, 1, 5], 3090), 3090));
    }

    /// 已退出的进程查命令行必须是 `None`：这是「回退而不是报错」的语义
    /// 边界，内核刚停下的那几拍轮询会走到它。
    #[test]
    fn process_command_line_of_dead_process_is_none() {
        let mut child = std::process::Command::new("cmd")
            .args(["/c", "exit"])
            .spawn()
            .expect("spawn cmd");
        let pid = child.id();
        let _ = child.wait();
        // wait 之后进程对象仍在（Child 持有句柄），但 PEB 已不可读——
        // 预期 OpenProcess 可能成功、ReadProcessMemory 必然失败，两条路
        // 都汇聚到 None。
        assert_eq!(process_command_line(pid), None);
    }
}
