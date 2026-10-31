//! 会话级随机对照实验（P2，L3）：显式开启、默认关闭、可随时停止、只影响开始后的新会话。
//!
//! ## 协议（必须先读再调参数）
//!
//! - **单元是会话**：实验开始后**新创建的**会话在 agent/created 时向壳领取一次分组；
//!   已存在的会话不参与——不在用户正在进行的对话中途改变行为。
//! - **比例**：ratio_bp 是万分比（1000 = 10%）；bucket = fnv1a(session + ":" + seed) % 10000，
//!   bucket < ratio_bp 进 control（对照组，不注入记忆、不排自动提取），其余 treatment。
//! - **时长**：1–14 天，缺省 7 天；到期按 ends_at_ms 判（不需要后台定时器）。
//! - **停止**：写 stopped_at_ms 立即生效；control 会话最迟下一步重新领取分组后恢复注入。
//! - **默认关闭**：没有配置文件 = 未配置；读取容错（坏 JSON / 未知 schema 一律按未配置）。
//!
//! ## 安全边界
//!
//! 配置文件 0600 原子写（用户实验意图是私密数据）；只有面板命令面（Tauri command）
//! 能开始/停止，模型工具不可达；桥上的 /v1/memory/experiment 只读、不写任何数据、
//! 不返回记忆内容；对照组会话不得写入记忆库（插件侧 control 工具拒绝 + 不注入 + 不排提取）。
//! 实验文件与账本都不落正文/标题。

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::shell::atomic_write::atomic_write_private;
use crate::shell::error::AppError;

/// 配置文件 schema。不认识的一律按「未配置」——新版本写的开关不能由旧版本猜着执行。
pub const SCHEMA: u32 = 1;

const DAY_MS: u64 = 86_400_000;

/// 实验时长边界（天）。
pub const MIN_DAYS: u32 = 1;
pub const MAX_DAYS: u32 = 14;
pub const DEFAULT_DAYS: u32 = 7;

/// 对照组比例边界（万分比）：5%–30%。
pub const MIN_RATIO_BP: u16 = 500;
pub const MAX_RATIO_BP: u16 = 3000;
pub const DEFAULT_RATIO_BP: u16 = 1000;

/// 分母：万分之一。
const BUCKET_SCALE: u32 = 10_000;

/// 落盘配置（experiment.json）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExperimentConfig {
    pub schema: u32,
    pub started_at_ms: u64,
    pub ends_at_ms: u64,
    /// 万分比：1000 = 10% 会话进对照组。
    pub ratio_bp: u16,
    /// 分组种子的来源（开始时刻）；同一实验内固定，重启/重读不重新分组。
    pub seed: u64,
    /// 0 = 进行中；非 0 = 用户停止时刻。
    pub stopped_at_ms: u64,
}

/// 面板/命令拿到的状态（Serialize；不含任何记忆内容）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ExperimentState {
    pub configured: bool,
    pub active: bool,
    pub started_at_ms: u64,
    pub ends_at_ms: u64,
    pub stopped_at_ms: u64,
    /// 对照组比例，百分数 0-100（与报告其它 ratio 一致）。
    pub ratio_percent: f64,
    pub seed: u64,
}

/// 读配置文件：不存在 / 坏 JSON / schema 不认识 → 未配置（不报错、不刷日志）。
///
/// **为什么坏 JSON 也吞掉**：这是一份「用户开关」而不是权威记录，坏掉时最安全的行为是
/// 回到默认关闭（实验不跑），而不是让效果页整页报错。文件本身不会被覆盖，直到用户
/// 下一次显式开始实验。
pub fn load(path: &Path) -> ExperimentConfig {
    let Ok(text) = fs::read_to_string(path) else {
        return ExperimentConfig::default();
    };
    let Ok(config) = serde_json::from_str::<ExperimentConfig>(&text) else {
        return ExperimentConfig::default();
    };
    if config.schema != SCHEMA {
        return ExperimentConfig::default();
    }
    config
}

/// 实验此刻是否进行中：已开始、未停止、未到期。
pub fn is_active(config: &ExperimentConfig, now_ms: u64) -> bool {
    config.schema == SCHEMA
        && config.started_at_ms > 0
        && config.stopped_at_ms == 0
        && now_ms >= config.started_at_ms
        && now_ms < config.ends_at_ms
}

/// 配置 -> 面板状态。
pub fn state_of(config: &ExperimentConfig, now_ms: u64) -> ExperimentState {
    let configured = config.schema == SCHEMA && config.started_at_ms > 0;
    ExperimentState {
        configured,
        active: is_active(config, now_ms),
        started_at_ms: config.started_at_ms,
        ends_at_ms: config.ends_at_ms,
        stopped_at_ms: config.stopped_at_ms,
        ratio_percent: if configured {
            ratio_percent(config.ratio_bp)
        } else {
            0.0
        },
        seed: config.seed,
    }
}

/// 万分比 -> 百分数（1000 -> 10.0）。
pub fn ratio_percent(ratio_bp: u16) -> f64 {
    (ratio_bp as f64) / 100.0
}

/// 开始一次实验：校验参数、写配置、返回状态。
///
/// **越界报错而不是静默 clamp**：这两个值会改变正在跑的行为，悄悄裁剪会让用户以为
/// 自己选的是 50%，实际跑的是 30%。
pub fn start(
    path: &Path,
    days: Option<u32>,
    ratio_bp: Option<u16>,
    now_ms: u64,
) -> Result<ExperimentState, AppError> {
    let days = days.unwrap_or(DEFAULT_DAYS);
    if !(MIN_DAYS..=MAX_DAYS).contains(&days) {
        return Err(AppError::Memory(format!(
            "对照实验天数必须在 {MIN_DAYS}–{MAX_DAYS} 天之间（收到 {days}）"
        )));
    }
    let ratio_bp = ratio_bp.unwrap_or(DEFAULT_RATIO_BP);
    if !(MIN_RATIO_BP..=MAX_RATIO_BP).contains(&ratio_bp) {
        return Err(AppError::Memory(format!(
            "对照组比例必须在 {MIN_RATIO_BP}–{MAX_RATIO_BP} 万分比之间（5%–30%，收到 {ratio_bp}）"
        )));
    }
    let existing = load(path);
    if is_active(&existing, now_ms) {
        return Err(AppError::Memory(
            "已有进行中的对照实验，请先停止".to_string(),
        ));
    }
    let config = ExperimentConfig {
        schema: SCHEMA,
        started_at_ms: now_ms,
        ends_at_ms: now_ms.saturating_add((days as u64) * DAY_MS),
        ratio_bp,
        seed: now_ms,
        stopped_at_ms: 0,
    };
    save(path, &config)?;
    Ok(state_of(&config, now_ms))
}

/// 停止实验：没有配置报错；已停止/已到期幂等返回当前状态（active=false）。
pub fn stop(path: &Path, now_ms: u64) -> Result<ExperimentState, AppError> {
    let mut config = load(path);
    if config.schema != SCHEMA || config.started_at_ms == 0 {
        return Err(AppError::Memory(
            "没有可停止的对照实验（尚未开始过）".to_string(),
        ));
    }
    // 已停止 / 已到期：幂等返回，不报错（按钮可能被连点，或页面停在旧状态上）。
    if config.stopped_at_ms == 0 && is_active(&config, now_ms) {
        config.stopped_at_ms = now_ms;
        save(path, &config)?;
    }
    Ok(state_of(&config, now_ms))
}

/// 确定性分组：同一 session + seed 永远同一个 cohort。
///
/// 用 FNV-1a 而不是随机数：分组必须在壳与插件两侧、重启前后都一致，也不需要存
/// 「会话 -> 组」的映射表。种子来自开始时刻，所以两次实验的分配互不相关。
pub fn bucket_of(session: &str, seed: u64) -> u32 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in session.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    for byte in seed.to_string().as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    (hash % BUCKET_SCALE as u64) as u32
}

/// 会话落在哪一组：control（不注入）/ treatment（正常）。
pub fn cohort_of(session: &str, seed: u64, ratio_bp: u16) -> &'static str {
    if bucket_of(session, seed) < ratio_bp as u32 {
        "control"
    } else {
        "treatment"
    }
}

/// 桥路由响应：只读、不含记忆内容。未配置/已停止/已到期时 cohort 为空串。
pub fn bridge_response(config: &ExperimentConfig, now_ms: u64, session: &str) -> serde_json::Value {
    let active = is_active(config, now_ms);
    let configured = config.schema == SCHEMA && config.started_at_ms > 0;
    let cohort = if active && !session.is_empty() {
        cohort_of(session, config.seed, config.ratio_bp)
    } else {
        ""
    };
    serde_json::json!({
        "configured": configured,
        "active": active,
        "cohort": cohort,
        "ends_at_ms": config.ends_at_ms,
    })
}

fn save(path: &Path, config: &ExperimentConfig) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            AppError::Memory(format!(
                "无法创建实验配置目录 {}：{error}",
                parent.display()
            ))
        })?;
    }
    let text = serde_json::to_string_pretty(config)
        .map_err(|error| AppError::Memory(format!("序列化对照实验配置失败：{error}")))?;
    // 0600：用户实验意图是私密数据；写入口只有一个（面板命令），不能跟随 umask。
    atomic_write_private(path, format!("{text}\n").as_bytes()).map_err(|error| {
        AppError::Memory(format!(
            "无法写入对照实验配置 {}：{error}。请检查该目录的写入权限。",
            path.display()
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    const NOW: u64 = 1_790_000_000_000;
    const DAY: u64 = 86_400_000;

    fn scratch(label: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let serial = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "effect-experiment-{label}-{}-{serial}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("建临时目录");
        dir.join("experiment.json")
    }

    #[test]
    fn default_is_unconfigured_and_inactive() {
        let path = scratch("default");
        let config = load(&path);
        assert_eq!(config, ExperimentConfig::default());
        let state = state_of(&config, NOW);
        assert!(!state.configured);
        assert!(!state.active);
        assert_eq!(state.ratio_percent, 0.0);
        assert!(!is_active(&config, NOW));
    }

    #[test]
    fn start_writes_config_and_is_active_until_ends_at() {
        let path = scratch("start");
        let state = start(&path, Some(7), Some(1000), NOW).expect("应能开始");
        assert!(state.configured && state.active);
        assert_eq!(state.ratio_percent, 10.0);
        assert_eq!(state.ends_at_ms, NOW + 7 * DAY);
        assert!(path.exists(), "配置必须落盘");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).expect("文件").permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "实验配置是私密数据");
        }
        // 到期后自动失效（不需要后台定时器）。
        assert!(!is_active(&load(&path), NOW + 7 * DAY));
        assert!(state_of(&load(&path), NOW + 7 * DAY).configured);
    }

    #[test]
    fn second_start_while_active_is_rejected_and_stop_is_idempotent() {
        let path = scratch("stop");
        start(&path, None, None, NOW).expect("首个实验");
        let error = start(&path, None, None, NOW + 1).expect_err("进行中不许再开");
        assert!(error.to_string().contains("已有进行中的对照实验"));
        let stopped = stop(&path, NOW + 1000).expect("停止");
        assert!(!stopped.active);
        assert_eq!(stopped.stopped_at_ms, NOW + 1000);
        // 连点/重复停止：幂等，不报错、不覆盖原停止时刻。
        let again = stop(&path, NOW + 9000).expect("重复停止幂等");
        assert_eq!(again.stopped_at_ms, NOW + 1000);
        assert!(!again.active);
        // 停止后可以开新实验。
        assert!(start(&path, Some(1), Some(3000), NOW + 9000).is_ok());
    }

    #[test]
    fn stop_without_config_reports_actionable_error() {
        let path = scratch("stop-missing");
        let error = stop(&path, NOW).expect_err("没有配置");
        assert!(error.to_string().contains("没有可停止的对照实验"));
    }

    #[test]
    fn corrupt_or_unknown_schema_reads_as_unconfigured() {
        let path = scratch("corrupt");
        fs::write(&path, b"{\"schema\": 1, \"started").expect("写坏 JSON");
        assert_eq!(load(&path), ExperimentConfig::default());
        fs::write(&path, b"{\"schema\": 99, \"started_at_ms\": 1}").expect("写未知 schema");
        assert_eq!(load(&path), ExperimentConfig::default());
        assert!(!state_of(&load(&path), NOW).configured);
    }

    #[test]
    fn parameter_bounds_are_errors_not_silent_clamps() {
        let path = scratch("bounds");
        for days in [0u32, 15] {
            let error = start(&path, Some(days), None, NOW).expect_err("天数越界");
            assert!(error.to_string().contains("天数"));
        }
        for ratio in [499u16, 3001] {
            let error = start(&path, None, Some(ratio), NOW).expect_err("比例越界");
            assert!(error.to_string().contains("比例"));
        }
        // 边界内的值必须能开始（越界在上面的 start 调用里已经断言过报错）。
        assert!(start(&path, Some(1), Some(500), NOW + 20_000).is_ok());
    }

    #[test]
    fn cohort_is_stable_per_seed_and_moves_with_seed_and_ratio() {
        let session = "session-00a67abf-6433-4d96-8e78-c2a2b7979cd7";
        assert_eq!(
            cohort_of(session, 12345, 1000),
            cohort_of(session, 12345, 1000),
            "同 session + seed 必须稳定"
        );
        // ratio=0 全 treatment；ratio=10000 全 control（纯函数边界）。
        assert_eq!(cohort_of(session, 777, 0), "treatment");
        assert_eq!(cohort_of(session, 777, BUCKET_SCALE as u16), "control");
        let bucket = bucket_of(session, 424242);
        assert!(bucket < BUCKET_SCALE);
        assert_eq!(bucket, bucket_of(session, 424242));
        assert!(matches!(
            cohort_of("session-x", 1, 500),
            "control" | "treatment"
        ));
        assert_ne!(bucket_of("session-x", 1), bucket_of("session-x", 2));
    }

    #[test]
    fn bridge_response_is_read_only_shape_and_empty_when_inactive() {
        let path = scratch("bridge");
        let config = load(&path);
        let response = bridge_response(&config, NOW, "session-a");
        assert_eq!(response["configured"], serde_json::json!(false));
        assert_eq!(response["active"], serde_json::json!(false));
        assert_eq!(response["cohort"], serde_json::json!(""));
        start(&path, Some(7), Some(1000), NOW).expect("开始");
        let config = load(&path);
        let response = bridge_response(&config, NOW + 1, "session-a");
        assert_eq!(response["configured"], serde_json::json!(true));
        assert_eq!(response["active"], serde_json::json!(true));
        assert!(matches!(
            response["cohort"].as_str(),
            Some("control") | Some("treatment")
        ));
        assert_eq!(response["ends_at_ms"], serde_json::json!(NOW + 7 * DAY));
    }
}
