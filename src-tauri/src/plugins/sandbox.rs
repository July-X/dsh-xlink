//! 沙盒预检：在一次性临时实例里装候选扩展、启动内核、探测健康。
//!
//! ## 为什么需要这一层
//!
//! 装插件 / 装技能是本壳**唯一**能主动破坏用户既有环境的动作，而破坏总是
//! 延迟暴露：装完当下一切正常，直到下一次启动工作台才崩。既有
//! [`crate::diagnostics::guard`] 的看门狗只在用户**主动启动**时兜底，那时坏包已经进了
//! 中央库、已经接了线、已经出现在面板上——用户要自己判断该卸哪一个。
//!
//! 沙盒把「会不会坏」这个判断**前移到安装那一刻**：在一个用完即弃的实例
//! 里真的把内核跑起来，坏包当场暴露，中央库与所有真实实例保持原样。
//!
//! ## 沙盒的边界
//!
//! - **不入注册表**。沙盒只有内存里的 [`InstanceRecord`] 与一个磁盘目录，
//!   不写 `registry.json`，因此实例列表、实例切换器、默认实例全部看不见它。
//! - **目录沿用常规实例布局**（`paths::instance_dir`）。这样
//!   `paths::instance_dsh_home` 等一整套派生函数零改动即可复用——适配器
//!   注入的 `DSH_HOME` 天然落在沙盒目录内，不会回落到用户的 `~/.dsh`。
//! - **端口自己说了算**。见下节为什么不能用 `kernel::start_instance`。
//! - **只在沙盒里物化与接线**。真实实例的 `extensions/` 与 `wiring.json`
//!   在预检期间一个字节都不会变。
//!
//! ## 为什么不复用 `kernel::start_instance`
//!
//! `kernel::resolve_instance_record` 会把壳侧权威状态强行写进实例记录
//! （`record.port = settings.port` 是它的第一件事）。沙盒走这条路径会去
//! 抢真实工作台的端口，于是「用户正在用工作台时做预检」必然失败。因此
//! 这里直接调适配器的 [`KernelAdapter::start`]，端口由沙盒自己分配。
//!
//! ## 预检能看见什么、看不见什么
//!
//! 能看见：进程起不来、就绪前崩溃、端口永不监听、内核 HTTP 不应答、
//! 启动日志里的致命错误标记。
//!
//! **看不见**：工作台页面加载之后的运行时 JS 异常与白屏。那需要真正的
//! webview 才能采集，属于 `harness-health.js` 与事故面板的职责。预检
//! 通过**不等于**页面一定不白屏，报告里必须这样表述，不能反过来暗示
//! 「预检过了就一定没事」。

use crate::kernel;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, ExitStatus};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::kernel::kernel_adapter::AdapterCapability;
use crate::shell::instance::InstanceRecord;
use crate::{kernel::kernel_adapter, shell::instance, shell::paths, shell::process};

/// 沙盒实例 id 前缀。既是路径合法字符集内的标识，也让任何遗留目录一眼
/// 可辨（[`sweep_stale`] 按此前缀回收）。
pub const SANDBOX_ID_PREFIX: &str = "sbx-";

/// 沙盒端口区段。刻意与壳的默认 3090 / 3091 错开：真实工作台正在运行
/// 时预检也必须能起一个并行内核，撞端口会让预检彻底失去意义。
const SANDBOX_PORT_BASE: u16 = 3190;
const SANDBOX_PORT_CEILING: u16 = 3290;

/// 判定沙盒内核「起来了」的最长等待。比 [`crate::diagnostics::guard`] 的 30 秒略长：
/// 预检跑在用户点按钮之后，多等几秒的成本远低于误判。
const READY_TIMEOUT_SECS: u64 = 40;
/// 监听轮询间隔。
const WATCH_POLL_MILLIS: u64 = 400;
/// 预检报告里携带的日志证据上限（字节）。够看清一段堆栈，又不至于把
/// 面板撑爆。
pub const EVIDENCE_MAX_BYTES: u64 = 8 * 1024;

/// 预检结论。三态而不是两态是刻意的：把「沙盒自己没跑起来」（端口占用、
/// 内核版本缺失、node 异常）与「候选包把内核搞崩了」混成同一个
/// `fail`，用户会去卸一个无辜的插件。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Verdict {
    /// 沙盒内核在带着候选包的情况下正常应答 HTTP。
    Pass,
    /// 候选包让沙盒内核起不来。**中央库与真实实例未被改动。**
    Fail,
    /// 预检自身没跑成（沙盒起不来、探测不了），候选包尚未被证明有问题。
    Inconclusive,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Pass => "pass",
            Verdict::Fail => "fail",
            Verdict::Inconclusive => "inconclusive",
        }
    }
}

/// 一次预检的结论。字段刻意全部可选 / 有默认值：报告要能被持久化到
/// 事故记录里，结构演进时旧记录必须仍能读出来。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrecheckReport {
    /// `pass` / `fail` / `inconclusive`。
    pub verdict: String,
    /// 候选包的中央库 id。取源阶段就失败时为空。
    pub plugin_id: String,
    /// 候选包的显示名。
    pub plugin_name: String,
    /// 预检通过后是否已把包安装到目标实例。只有 `pass` 会真正安装；
    /// `inconclusive` 也会安装（预检不许因为自身故障拦住用户），但这个
    /// 字段让 UI 能如实说明安装是在**没有预检背书**的情况下发生的。
    pub installed: bool,
    /// 一句话结论，直接给用户看。
    pub summary: String,
    /// 启动日志末尾的证据摘录。
    pub evidence: String,
    /// 证据日志的完整落盘路径，供「查看日志」动作使用。预检通过时为空。
    pub evidence_path: String,
    /// 可操作的下一步（简体中文）。
    pub hint: String,
    /// 即使判定为通过也照实列出的告警（例如启动日志里出现了致命错误
    /// 标记，但内核仍然起来了）。空数组表示没有。
    pub warnings: Vec<String>,
    /// 预检耗时（毫秒）。
    pub duration_ms: u64,
    /// 本次预检的运行记录 id（`diagnostics::run`）。插件安全诊断页靠它
    /// 拉那条时间线；空串表示本次预检没有落记录。
    #[serde(default)]
    pub run_id: String,
    // —— 候选插件的来源与影响范围（设计 §6.5 首屏必须展示的五项）——
    //
    // 这些是**用户判断「要不要信任这个包」的依据**。不给它们，用户只能
    // 看到「预检通过」四个字，却不知道装的是哪个地址的什么版本。
    /// 候选插件的来源类型：`npm` / `github` / `path`。
    #[serde(default)]
    pub source_kind: String,
    /// 来源的短名称（npm 包名或 `owner/repo`），**不含**完整 URL 与凭据。
    #[serde(default)]
    pub source_label: String,
    /// 钉住的版本或 tag；空表示跟随最新。
    #[serde(default)]
    pub pin: String,
    /// 下载物是否通过完整性校验，以及用的哪种摘要。
    ///
    /// 取值见 [`integrity`]：`sha512` / `sha256` / `sha1` / `none`。
    /// `none` 表示**没能校验**——这比 `sha1` 弱得多，UI 必须区别显示，
    /// 不能笼统显示成「已校验」。
    #[serde(default)]
    pub integrity: String,
    /// 物化方式：`link` 或 `copy`。
    #[serde(default)]
    pub materialize: String,
    /// 会不会影响默认实例。
    #[serde(default)]
    pub affects_default_instance: bool,
    /// 目标实例 id（预检实际装到的那一个）。
    #[serde(default)]
    pub target_instance: String,
}

/// 完整性摘要的种类 → UI 文案与强弱分级。
///
/// **强度分三档而不是「有 / 无」两档**：`sha1` 存在但早已不是抗碰撞摘要，
/// 而 `none` 意味着根本没校验。两者都叫「已校验」会让用户以为拿到了
/// 和 npm 官方同样的保证。
pub mod integrity {
    /// sha512（npm 默认，SRI 最强）。
    pub const SHA512: &str = "sha512";
    /// sha256。
    pub const SHA256: &str = "sha256";
    /// sha1（老 packument 的 `dist.shasum`），存在但抗碰撞已破。
    pub const SHA1: &str = "sha1";
    /// 没有可用摘要，**未校验**。
    pub const NONE: &str = "none";

    /// 摘要种类 → 用户能读懂的一句话。
    pub fn label(kind: &str) -> &'static str {
        match kind {
            SHA512 => "已通过 sha512 校验",
            SHA256 => "已通过 sha256 校验",
            SHA1 => "已通过 sha1 校验（摘要算法较弱）",
            NONE => "未能校验完整性",
            // 未知取值原样透出而不是假装成 `none`——后者会把「后端换了
            // 算法」说成「压根没校验」。
            _ => "未知的完整性状态",
        }
    }
}

impl PrecheckReport {
    pub fn new(plugin_id: &str, plugin_name: &str, verdict: Verdict) -> Self {
        PrecheckReport {
            verdict: verdict.as_str().to_string(),
            plugin_id: plugin_id.to_string(),
            plugin_name: plugin_name.to_string(),
            installed: false,
            summary: String::new(),
            evidence: String::new(),
            evidence_path: String::new(),
            hint: String::new(),
            warnings: Vec::new(),
            duration_ms: 0,
            run_id: String::new(),
            source_kind: String::new(),
            source_label: String::new(),
            pin: String::new(),
            integrity: integrity::NONE.to_string(),
            materialize: String::new(),
            affects_default_instance: false,
            target_instance: String::new(),
        }
    }
}

/// 启动日志里视为「可疑」的标记。这些标记**不**直接判失败——内核在
/// 插件报错的场合仍可能正常起来——只作为通过项的告警列给用户。判失败
/// 只认「进程没起来 / 端口不监听 / HTTP 不应答」这三个硬事实。
const LOG_MARKERS: &[&str] = &[
    "uncaught exception",
    "Cannot find module",
    "ERR_MODULE_NOT_FOUND",
    "fatal:",
];

/// 扫描启动日志，列出命中的可疑标记。返回去重后的可读片段。
pub fn scan_log_markers(log: &str) -> Vec<String> {
    let mut hits: Vec<String> = Vec::new();
    for line in log.lines() {
        for marker in LOG_MARKERS {
            if !line.contains(marker) {
                continue;
            }
            let excerpt = line.trim();
            if excerpt.is_empty() {
                continue;
            }
            let entry = format!("{marker}：{}", truncate(excerpt, 160));
            if !hits.contains(&entry) {
                hits.push(entry);
            }
            break; // 一行只报一次，避免同一条错误刷屏
        }
    }
    hits.truncate(4); // 面板上一屏放得下的量
    hits
}

fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let head: String = text.chars().take(max_chars).collect();
    format!("{head}…")
}

// ─── 沙盒实例 ──────────────────────────────────────────────────────────

/// 一次性的预检实例。持有自己派生的内核子进程，`Drop` 时保证它连同
/// 整个实例目录一起消失——预检跑完必须不留任何痕迹，哪怕调用方 panic
/// 或提前 return。
pub struct Sandbox {
    family: String,
    instance_id: String,
    record: InstanceRecord,
    dir: PathBuf,
    logs_dir: PathBuf,
    child: Option<Child>,
}

impl Sandbox {
    /// 准备一个沙盒实例：分配 id 与端口、落目录骨架、按内核族惯例写好
    /// profile 模板。此处**不**启动内核。
    ///
    /// `used_ports` 是注册表里已被真实实例占用的端口。
    pub fn create(
        family: &str,
        version: &str,
        profile: &str,
        used_ports: &[u16],
        on_progress: &mut dyn FnMut(&str),
    ) -> Result<Self, String> {
        let adapter = kernel_adapter::lookup(family)
            .ok_or_else(|| format!("内核族 {family} 还没有适配器实现，无法对它做安装预检"))?;
        if !adapter
            .capabilities()
            .contains(AdapterCapability::ProfileWiring)
        {
            return Err(format!(
                "内核族 {family} 不支持 profile 接线，装完也不会被加载——对它做预检没有意义，请直接安装"
            ));
        }
        let instance_id = new_id();
        let dir = paths::instance_dir(family, &instance_id);
        let port = pick_port(used_ports).ok_or_else(|| {
            format!(
                "端口区间 {SANDBOX_PORT_BASE}-{SANDBOX_PORT_CEILING} 内没有空闲端口，无法为预检启动临时内核。请关闭一些占用端口的程序后重试"
            )
        })?;
        // `InstanceRecord::new` 已把 workspace 定为 `instance_dir/<id>/workspace`，
        // 与 `paths::instance_dsh_home` 落在同一棵树下——适配器注入的
        // `DSH_HOME` 因此天然隔离在沙盒内。
        let mut record = InstanceRecord::new(&instance_id, family, port, process::epoch_millis());
        record.kernel_version = Some(version.to_string());
        record.profile = profile.to_string();
        record.label = Some(String::from("沙盒预检"));

        adapter
            .prepare_instance(&record)
            .map_err(|e| format!("无法准备沙盒实例目录：{e}"))?;

        on_progress(&format!("沙盒实例 {instance_id} 已就绪（临时端口 {port}）"));
        let logs_dir = dir.join("logs");
        Ok(Sandbox {
            family: family.to_string(),
            instance_id,
            record,
            dir,
            logs_dir,
            child: None,
        })
    }

    /// 实例 id（形如 `sbx-…`），用于定位日志与残留目录。
    pub fn instance_id(&self) -> &str {
        &self.instance_id
    }

    /// 沙盒实例目录的绝对路径。
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// 本次预检使用的端口。
    pub fn port(&self) -> u16 {
        self.record.port
    }

    /// 派生内核日志的精确路径。日志写在沙盒目录内（随目录一起删除，
    /// 不污染用户日志列表），需要长期留证时由调用方另存。
    pub fn kernel_log_path(&self) -> PathBuf {
        let today = process::current_date_string();
        kernel::lifecycle::kernel_log_spec(&self.family, &self.instance_id)
            .path_for(&self.logs_dir, &today)
    }

    /// 启动内核并等它就绪。失败时子进程已被本方法消费（终止并回收），
    /// 不会留下孤儿。
    pub fn start(&mut self, install_root: &Path, node: &Path) -> Result<(), String> {
        let adapter = kernel_adapter::lookup(&self.family)
            .ok_or_else(|| format!("内核族 {} 还没有适配器实现", self.family))?;
        std::fs::create_dir_all(&self.logs_dir)
            .map_err(|e| format!("无法准备沙盒日志目录：{e}"))?;
        let mut child = adapter
            .start(&self.record, install_root, node, &self.logs_dir)
            .map_err(|e| format!("沙盒内核启动失败：{e}"))?;
        match watch(&mut child, self.record.port) {
            WatchVerdict::Ready => {
                self.child = Some(child);
                Ok(())
            }
            WatchVerdict::Exited(status) => {
                let _ = child.wait();
                Err(format!("沙盒内核在就绪前退出（{status}）"))
            }
            WatchVerdict::Hung => {
                let _ = child.wait();
                Err(format!(
                    "沙盒内核没有在 {READY_TIMEOUT_SECS} 秒内开始监听端口 {}",
                    self.record.port
                ))
            }
        }
    }

    /// 对沙盒内核做一次 HTTP 存活确认。返回状态码。
    pub fn probe(&self) -> Result<u16, String> {
        http_probe(self.record.port, "/")
    }

    /// 读启动日志末尾，供报告取证据。
    pub fn read_log_tail(&self) -> String {
        let path = self.kernel_log_path();
        process::read_tail(&path, EVIDENCE_MAX_BYTES)
    }

    /// 显式停止内核。`Drop` 还会再兜一次，因此提前调用是安全的。
    pub fn shutdown(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = kernel::lifecycle::stop(&mut child);
            let _ = child.wait();
        }
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        // 顺序要紧：先停进程，再删目录。目录里可能还压着内核写的
        // session / log 文件，进程没停就删会在 Windows 上直接失败。
        self.shutdown();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// 同一毫秒内连开两次预检也必须拿到不同目录：光靠「毫秒 + pid」不够
/// （同进程同毫秒会撞），所以再叠一个进程内单调计数器。
fn new_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    format!(
        "{SANDBOX_ID_PREFIX}{:x}-{:x}-{:x}",
        process::epoch_millis(),
        std::process::id(),
        seq
    )
}

/// 选一个空闲的沙盒端口。注册表里已被实例占用的端口与**当前真的有人
/// 在监听**的端口都要避开：后者覆盖「用户手工起了个进程」以及「上一次
/// 预检崩溃留下的孤儿内核」。
fn pick_port(used: &[u16]) -> Option<u16> {
    (SANDBOX_PORT_BASE..SANDBOX_PORT_CEILING)
        .find(|p| !used.contains(p) && !kernel::lifecycle::port_open(*p))
}

enum WatchVerdict {
    Ready,
    Exited(ExitStatus),
    Hung,
}

fn watch(child: &mut Child, port: u16) -> WatchVerdict {
    let deadline = Instant::now() + Duration::from_secs(READY_TIMEOUT_SECS);
    loop {
        if kernel::lifecycle::port_open(port) {
            return WatchVerdict::Ready;
        }
        if let Ok(Some(status)) = child.try_wait() {
            return WatchVerdict::Exited(status);
        }
        if Instant::now() >= deadline {
            let _ = kernel::lifecycle::stop(child);
            return WatchVerdict::Hung;
        }
        std::thread::sleep(Duration::from_millis(WATCH_POLL_MILLIS));
    }
}

/// 极简 HTTP GET，只回答「内核是否在正常应答」这一个问题。刻意不引入
/// HTTP 客户端依赖：预检只需要状态行，而完整响应校验需要真正的
/// webview，那是 `harness-health.js` 的职责。
fn http_probe(port: u16, path: &str) -> Result<u16, String> {
    let addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port));
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(1500))
        .map_err(|e| format!("连接沙盒内核 {port} 失败：{e}"))?;
    stream
        .set_read_timeout(Some(Duration::from_millis(3000)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_millis(1500)))
        .map_err(|e| e.to_string())?;
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    )
    .map_err(|e| format!("向沙盒内核发送探测请求失败：{e}"))?;
    let mut buf = [0u8; 1024];
    let read = stream
        .read(&mut buf)
        .map_err(|e| format!("读取沙盒内核响应失败：{e}"))?;
    let head = String::from_utf8_lossy(&buf[..read]);
    head.split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| format!("沙盒内核返回的不是合法 HTTP 响应：{head}"))
}

/// 回收上一次预检崩溃留下的沙盒目录。启动时调一次：这些目录对用户
/// 没有任何价值，却会让实例目录列表越攒越多，也会让人以为系统里存在
/// 一个叫「沙盒预检」的实例。
pub fn sweep_stale(family: &str) -> usize {
    let root = paths::kernel_instances_dir(family);
    let Ok(entries) = std::fs::read_dir(&root) else {
        return 0;
    };
    let mut removed = 0usize;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !name.starts_with(SANDBOX_ID_PREFIX) {
            continue;
        }
        if std::fs::remove_dir_all(entry.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// 注册表里已被占用的端口，沙盒挑端口时要避开。
///
/// 注册表读不出来（schema 不兼容、文件损坏）时返回空列表而不是报错：
/// 端口冲突的后果是预检失败并如实报出来，**不应该**让整个预检在还没
/// 开始前就崩掉。`pick_port` 本身还会用 `port_open` 复查一遍真实的
/// 监听状态，所以少几个已占用端口不会导致撞车。
pub fn used_ports(family: &str) -> Vec<u16> {
    match instance::load_registry() {
        Ok(registry) => registry
            .instances
            .iter()
            .filter(|r| r.kernel_family == family)
            .map(|r| r.port)
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// 沙盒失败时把证据日志另存到壳的日志目录，让用户在沙盒目录被删除
/// 之后仍能打开完整输出。返回落盘路径。
pub fn preserve_evidence(data_dir: &Path, sandbox: &Sandbox, plugin_id: &str) -> Option<PathBuf> {
    let today = process::current_date_string();
    let target = kernel::lifecycle::current_kernel_log_path(data_dir, &sandbox.family, "precheck")
        .parent()
        .map(|dir| dir.join(format!("precheck-{}-{today}.log", sanitize(plugin_id))))?;
    let source = sandbox.kernel_log_path();
    if !source.is_file() {
        return None;
    }
    std::fs::copy(&source, &target).ok()?;
    Some(target)
}

/// 证据文件名里的插件 id 可能含斜杠等路径字符。
fn sanitize(raw: &str) -> String {
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sandbox_id_satisfies_path_component_rules() {
        let id = new_id();
        paths::validate_id_component(&id).expect("沙盒 id 必须能安全用作路径片段");
        assert!(id.starts_with(SANDBOX_ID_PREFIX));
    }

    #[test]
    fn two_sandbox_ids_do_not_collide() {
        // 同一毫秒内连开两次预检也必须拿到不同目录。
        let a = new_id();
        let b = new_id();
        assert_ne!(
            a, b,
            "沙盒 id 必须在同一毫秒内也唯一（pid + 毫秒已是唯一组合）"
        );
    }

    #[test]
    fn pick_port_avoids_used_and_listening_ports() {
        // 3190 是区段起点；把它标成已占用后应当顺延，而不是重复给出。
        let chosen = pick_port(&[SANDBOX_PORT_BASE]).expect("区段内应有可用端口");
        assert_ne!(chosen, SANDBOX_PORT_BASE);
        assert!((SANDBOX_PORT_BASE..SANDBOX_PORT_CEILING).contains(&chosen));
    }

    #[test]
    fn scan_marks_suspicious_lines_once_per_line() {
        let log = "\
loading plugin foo
fatal: uncaught exception in plugin bar
Cannot find module 'x'
fatal: uncaught exception in plugin bar
";
        let hits = scan_log_markers(log);
        // 重复行去重成一条，两个不同标记各一条
        assert_eq!(hits.len(), 2, "重复错误行不应重复上报：{hits:?}");
        assert!(hits.iter().any(|h| h.contains("uncaught exception")));
        assert!(hits.iter().any(|h| h.contains("Cannot find module")));
    }

    #[test]
    fn scan_markers_ignores_clean_log() {
        assert!(scan_log_markers("loading plugin a\nready on 3190\n").is_empty());
    }

    #[test]
    fn sanitize_scrubs_path_separators_from_evidence_names() {
        assert_eq!(sanitize("@scope/pkg"), "_scope_pkg");
        assert_eq!(sanitize("a/b\\c"), "a_b_c");
    }

    #[test]
    fn truncate_marks_elision() {
        assert_eq!(truncate("abc", 8), "abc");
        assert_eq!(truncate("abcdef", 3), "abc…");
    }
}
