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
//!
//! ## 沙盒必须**先装上被测配置**，否则这个判据回答的是另一个问题
//!
//! `Sandbox::create` 建的是一个全新的空实例，`prepare_instance` 只落一份
//! 空 profile 占位——里面**一个插件都没有**。不把被测配置物化进去就启动，
//! 测的是"裸内核能不能起来"，与"这套配置能不能起来"无关：
//!
//! - 恢复自检会恒定得到"通过"，于是一次什么都没验的检查被说成"实测通过"；
//! - 二分每一轮试的都是同一个裸内核，恒定 `Pass`，分治照常推进，
//!   **收敛到一个从未被真正启动过的无辜插件**——比不做二分更糟。
//!
//! 所以 `start` 之前必须 `ensure_wiring_filtered`：被 `allow` 放行的插件会
//! 被物化进沙盒并进 profile 清单，被挡掉的既不物化也不进清单，内核因此
//! 真的在"缺它们"的状态下启动。

use std::path::Path;

use crate::plugins::WiringFilter;
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

/// 在一次性沙盒实例里装上**指定的那部分扩展**，起一次内核并判定。
///
/// `allow` 决定哪些插件被装进沙盒：恢复自检放行当前（已恢复）的那一套，
/// 二分放行本轮 trial 的那一半。被挡掉的插件不物化也不进 profile 清单，
/// 因此内核确实是在缺少它们的状态下启动的。
///
/// 不碰用户的真实工作台：临时实例走独立端口（3190-3290），起来即收摊。
/// 调用方持有的是一份**结论**，不是一个活着的进程。
///
/// 接线失败一律判 [`Verdict::Inconclusive`] 而不是 `Fail`：沙盒自己装不成
/// 被测配置，说明的是**我们没测成**，不是这套配置起不来。
#[allow(clippy::too_many_arguments)]
pub fn probe_once(
    data_dir: &Path,
    family: &str,
    settings: &settings::Settings,
    pnpm_exe: &Path,
    node_path: &Path,
    allow: &WiringFilter<'_>,
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

    // 关键一步：把被测配置装进沙盒。不装就启动等于在裸内核上做判定——
    // 那回答的是另一个问题，详见模块文档。
    on_progress("正在把待验证的扩展装进一次性沙盒 …");
    match crate::plugins::ensure_wiring_filtered(
        family,
        sandbox.instance_id(),
        data_dir,
        settings,
        pnpm_exe,
        allow,
        on_progress,
    ) {
        Ok((wired, _changed)) => on_progress(&format!("沙盒已装入 {wired} 个扩展")),
        Err(error) => {
            let evidence = sandbox.read_log_tail();
            return ProbeResult {
                ready: false,
                detail: format!("没能把待验证的扩展装进沙盒：{error}"),
                evidence,
                // 装不成是我们没测成，不是这套配置起不来。
                verdict: Verdict::Inconclusive,
            };
        }
    }

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
