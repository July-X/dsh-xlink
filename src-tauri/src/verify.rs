//! 「起一次沙盒内核，看它起不起来」——恢复自检与二分试探共用的那一次判定。
//!
//! ## 为什么抽出来
//!
//! 两件事需要同一条判据，但都不该各自实现一遍：
//! - 环境恢复完成后确认「配置真的回到一个能起来的状态」；
//! - 二分定位每轮试探「启用这一半，内核起不起来」。
//!
//! 判据一旦有两份实现，很快就会分叉——而分叉出来的那个会让二分静默收敛到
//! 错误的答案：它把"没试成"当成"起来了"，坏的那一半就被记成已排除。
//!
//! ## 判据
//!
//! 与 `guard::watch_child` 同源：端口应答即 `Ready`，进程提前退出即失败，
//! 超时未应答即挂起。**不做**白屏 / 运行时异常判定——那需要真正的 webview，
//! 属于 `harness-health.js` 的职责；这里只回答"内核起没起来"。

use std::path::Path;

use crate::sandbox::{Sandbox, Verdict};
use crate::settings;

/// 一次沙盒启动的判定结果。
pub struct ProbeResult {
    /// 内核是否正常应答。
    pub ready: bool,
    /// 卡在哪一步的可读说明（失败 / 挂起 / HTTP 不应答时才有内容）。
    pub detail: String,
    /// 启动日志末尾，供面板展示证据。
    pub evidence: String,
    /// 三态判定。`Inconclusive` **不是** `Pass`——把"没试成"当成"起来了"
    /// 是二分里最致命的一种错。
    pub verdict: Verdict,
}

/// 在一次性沙盒实例里起一次内核并判定。
///
/// 不碰用户的真实工作台：临时实例走独立端口（3190-3290），起来即收摊。
/// 调用方持有的是一份**结论**，不是一个活着的进程。
#[allow(clippy::too_many_arguments)]
pub fn probe_once(
    data_dir: &Path,
    family: &str,
    _instance: &str,
    settings: &settings::Settings,
    node_path: &Path,
    on_progress: &mut dyn FnMut(&str),
) -> ProbeResult {
    let version = match crate::kernel::read_active(data_dir) {
        Some(version) => version,
        None => {
            return ProbeResult {
                ready: false,
                detail: "本机没有启用任何内核版本，无法自检".into(),
                evidence: String::new(),
                verdict: Verdict::Inconclusive,
            }
        }
    };
    let install_root = crate::kernel_adapter::lookup(family)
        .and_then(|adapter| adapter.resolve_install_dir(&version))
        .unwrap_or_else(|| crate::kernel::kernel_dir(data_dir, &version));

    let mut sandbox = match Sandbox::create(
        family,
        &version,
        &settings.profile,
        &crate::sandbox::used_ports(family),
        on_progress,
    ) {
        Ok(sandbox) => sandbox,
        Err(reason) => {
            return ProbeResult {
                ready: false,
                detail: format!("沙盒实例建不起来：{reason}"),
                evidence: String::new(),
                verdict: Verdict::Inconclusive,
            }
        }
    };

    if let Err(detail) = sandbox.start(&install_root, node_path) {
        let evidence = sandbox.read_log_tail();
        return ProbeResult {
            ready: false,
            detail,
            evidence,
            verdict: Verdict::Fail,
        };
    }
    let probe = sandbox.probe();
    let evidence = sandbox.read_log_tail();
    sandbox.shutdown();

    match probe {
        Ok(code) if (200..400).contains(&code) => ProbeResult {
            ready: true,
            detail: String::new(),
            evidence,
            verdict: Verdict::Pass,
        },
        Ok(code) => ProbeResult {
            ready: false,
            detail: format!("内核返回 HTTP {code}"),
            evidence,
            verdict: Verdict::Fail,
        },
        Err(detail) => ProbeResult {
            ready: false,
            detail,
            evidence,
            verdict: Verdict::Fail,
        },
    }
}

/// 判定字符串 ↔ [`crate::bisect::Outcome`] 的单向映射。
///
/// 只有 `ProbeResult` 需要，二分会话的类型在 `bisect.rs` 里；把转换放在
/// 调用方，避免两个模块互相依赖。
pub fn outcome_label(verdict: Verdict) -> &'static str {
    verdict.as_str()
}
