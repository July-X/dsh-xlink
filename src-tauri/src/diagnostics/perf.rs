//! 状态快照的低扰动性能观测编排。
//!
//! 采样默认关闭；开启后把 `get_status` 的文件、进程、窗口与状态组装分段写入
//! 壳日志。把这段编排放在 diagnostics 而不是命令层，避免状态命令继续膨胀，
//! 也让后续按日志数据优化时不必改 IPC 契约。

use std::path::Path;
use std::sync::OnceLock;
use std::time::Instant;

use tauri::{AppHandle, Manager};

use crate::commands::{cached_node, AppState, StatusView};
use crate::harness::harness_window;
use crate::harness::official_chat::OFFICIAL_CHAT_WINDOW_LABEL;
use crate::kernel;
use crate::plugins::quarantine;
use crate::shell::settings;
use crate::shell::shell_events;

/// 可选的低扰动性能采样。
///
/// `v0.5.0` 之前的版本默认记录，便于在性能方案定型前持续收集真实数据；
/// `v0.5.0` 及之后默认关闭。`DSH_XLINK_PERF=1`（或 `true` / `on`）可显式开启，
/// `0`（或 `false` / `off`）可显式关闭。采样结果使用 `key=value` 单行格式写入壳日志，
/// 便于按 `source`、p50、p95 和 max 聚合；关闭时只做一次 `OnceLock` 读取，
/// 不创建计时器和字段数组。
pub(crate) struct PerfSample {
    enabled: bool,
    name: &'static str,
    source: &'static str,
    started: Option<Instant>,
    fields: Vec<(&'static str, FieldValue)>,
}

/// 一个采样字段。耗时字段输出带 `_us` 后缀，计数字段按原样输出。
enum FieldValue {
    Micros(u128),
    Count(u64),
}

static PERF_ENABLED: OnceLock<bool> = OnceLock::new();
const PERF_CUTOFF_VERSION: &str = "0.5.0";

impl PerfSample {
    pub(crate) fn new(name: &'static str, source: Option<&str>) -> Self {
        let enabled = perf_enabled();
        let source = match source {
            Some("poll") => "poll",
            Some("refresh") => "refresh",
            _ => "unknown",
        };
        Self {
            enabled,
            name,
            source,
            started: enabled.then(Instant::now),
            fields: Vec::new(),
        }
    }

    pub(crate) fn disabled() -> Self {
        Self {
            enabled: false,
            name: "disabled",
            source: "disabled",
            started: None,
            fields: Vec::new(),
        }
    }

    pub(crate) fn start(&self) -> Option<Instant> {
        self.enabled.then(Instant::now)
    }

    pub(crate) fn end(&mut self, field: &'static str, started: Option<Instant>) {
        if let Some(started) = started {
            self.fields
                .push((field, FieldValue::Micros(started.elapsed().as_micros())));
        }
    }

    /// 记一个计数字段（如「本次扫描了几个文件」）。计时不适合表达的量用这个：
    /// 输出为 `<field>=<value>`，不带 `_us` 后缀。
    pub(crate) fn count(&mut self, field: &'static str, value: u64) {
        if self.enabled {
            self.fields.push((field, FieldValue::Count(value)));
        }
    }

    pub(crate) fn measure<T>(&mut self, field: &'static str, run: impl FnOnce() -> T) -> T {
        let started = self.start();
        let value = run();
        self.end(field, started);
        value
    }

    /// 记一个**已经掐好**的耗时（单位微秒）。给那些掐表在别处、跨一整个循环
    /// 没法包成 [`Self::measure`] 闭包的阶段用：分段计时、以及 `atomic_write`
    /// 那三段自己量出来的 fsync。
    pub(crate) fn record_micros(&mut self, field: &'static str, micros: u128) {
        if self.enabled {
            self.fields.push((field, FieldValue::Micros(micros)));
        }
    }

    /// 一次记一整组掐好的耗时。由被观测方（[`crate::shell::atomic_write::WriteTimings`]）
    /// 自己吐出 `(字段名, 微秒)` 对——调用点一行写完，不必把四段拆成四行 `if`。
    pub(crate) fn record_all(&mut self, fields: [(&'static str, u128); 4]) {
        if self.enabled {
            for (field, micros) in fields {
                self.fields.push((field, FieldValue::Micros(micros)));
            }
        }
    }

    /// 采样关闭时构造一个「强制开启」的采样器，供单测断言记了哪些字段。
    /// 单测里 `new()` 会去读 `DSH_XLINK_PERF` 与版本默认值——那让「本轮记了
    /// 哪些分段」这条断言没法在测试里成立，而**字段名悄悄少一个**正是这套
    /// 埋点最可能的腐化方式（改了调用点，忘了同步日志解析的人）。
    #[cfg(test)]
    pub(crate) fn for_tests() -> Self {
        Self {
            enabled: true,
            name: "test",
            source: "test",
            started: Some(Instant::now()),
            fields: Vec::new(),
        }
    }

    /// 本轮记下的字段名（按记录顺序）。供上面那条单测断言覆盖面。
    #[cfg(test)]
    pub(crate) fn captured_fields(&self) -> Vec<&'static str> {
        self.fields.iter().map(|(name, _)| *name).collect()
    }

    pub(crate) fn finish(self) {
        let Some(started) = self.started else {
            return;
        };
        let mut line = format!(
            "perf={} source={} shell={} build={} total_us={}",
            self.name,
            self.source,
            // 每条采样自带版本与构建类型，否则「这条 39ms 是修复前还是修复后」
            // 只能靠文件名日期猜。`CARGO_PKG_VERSION` 是编译期常量、`cfg!` 同理，
            // 两者的运行期成本都是零。
            env!("CARGO_PKG_VERSION"),
            if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            },
            started.elapsed().as_micros()
        );
        for (field, value) in self.fields {
            match value {
                FieldValue::Micros(elapsed) => line.push_str(&format!(" {field}_us={elapsed}")),
                FieldValue::Count(count) => line.push_str(&format!(" {field}={count}")),
            }
        }
        shell_events::record("perf-status", &line);
    }
}

fn perf_enabled() -> bool {
    *PERF_ENABLED.get_or_init(|| {
        let default = default_perf_enabled(env!("CARGO_PKG_VERSION"));
        let value = std::env::var("DSH_XLINK_PERF").ok();
        perf_flag(value.as_deref(), default)
    })
}

fn default_perf_enabled(version: &str) -> bool {
    crate::shell::version::cmp_versions(version, PERF_CUTOFF_VERSION) == std::cmp::Ordering::Less
}

fn perf_flag(value: Option<&str>, default: bool) -> bool {
    match value {
        None => default,
        Some("1") | Some("true") | Some("on") => true,
        Some("0") | Some("false") | Some("off") | Some(_) => false,
    }
}

/// 在阻塞线程上收集管理面板的完整状态，并在需要时记录分段耗时。
pub(crate) fn collect_status(app: &AppHandle, data_dir: &Path, source: Option<&str>) -> StatusView {
    let mut perf = PerfSample::new("get_status", source);
    // 一次读盘同时拿 settings 与 warning。此前 settings 在这里和
    // `status_with_perf` 里各读一遍 settings.json（perf 采样里 settings_us 与
    // kernel_settings_warning_us 之和约 5%）——settings.json 每轮 poll 读两次
    // 纯属浪费，2026-10-09 起合并为这一读，`kernel_settings_warning` 分段随
    // 之从日志里消失。
    let (settings, settings_warning) = perf.measure("settings", || {
        settings::load_checked_for_shell(settings::current_mode())
    });
    let kernel_status =
        kernel::lifecycle::status_with_perf(data_dir, &settings, settings_warning, &mut perf);
    perf.measure("reload_stalled", || {
        harness_window::reload_stalled(app, kernel_status.running)
    });
    let quarantine_doc = perf.measure("quarantine", || quarantine::load(data_dir));
    let state = app.state::<AppState>();
    let node_info = perf.measure("node", || cached_node(&state, &settings));
    let official_chat_open = app.get_window(OFFICIAL_CHAT_WINDOW_LABEL).is_some();
    let last_incident = perf.measure("last_incident", || {
        crate::diagnostics::startup_run::load_incident(data_dir)
    });
    let view = StatusView {
        shell_version: app.package_info().version.to_string(),
        dev_build: cfg!(debug_assertions),
        shell_mode: crate::shell::paths::ShellMode::current(),
        kernel: kernel_status,
        node: node_info,
        quarantined: quarantine_doc.items,
        last_incident,
        settings,
        official_chat_open,
    };
    perf.finish();
    view
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perf_defaults_before_v050_and_env_can_override_it() {
        assert!(default_perf_enabled("0.3.7-rc.1"));
        assert!(default_perf_enabled("0.4.9"));
        assert!(default_perf_enabled("0.5.0-rc.1"));
        assert!(!default_perf_enabled("0.5.0"));
        assert!(!default_perf_enabled("0.5.1"));

        assert!(perf_flag(None, true));
        assert!(!perf_flag(None, false));
        assert!(perf_flag(Some("1"), false));
        assert!(perf_flag(Some("true"), false));
        assert!(perf_flag(Some("on"), false));
        assert!(!perf_flag(Some("0"), true));
        assert!(!perf_flag(Some("false"), true));
        assert!(!perf_flag(Some("off"), true));
        assert!(!perf_flag(Some("yes"), true));
        assert!(PerfSample::disabled().start().is_none());
    }
}
