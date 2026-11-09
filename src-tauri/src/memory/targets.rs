//! 记忆模块的每内核开关：一次确认公共池来源范围，并控制候选提取和记忆引用。
//!
//! 新开关与来源快照写在同一个文件；关闭或来源变化时停止使用，不能退回旧逐条许可。
//! 尚未迁移的实例保留旧行为，旧 `use_approved` 不会被静默解释成跨来源授权。
//! 控制文件不可读时拒绝使用，不覆盖已有数据。
use super::module;
use super::store::{self, Namespace};
use crate::shell::{error::AppError, instance, paths, process, settings};
use serde::{Deserialize, Serialize};

pub(crate) const TARGET_DSH: &str = "dsh";
pub(crate) const TARGET_MCODE: &str = "mcode";

#[derive(Default, Deserialize, Serialize)]
struct Targets {
    version: u32,
    dsh: Option<bool>,
    mcode: Option<bool>,
    #[serde(default)]
    dsh_pool_pair: Option<String>,
    #[serde(default)]
    mcode_pool_pair: Option<String>,
}

/// 一个接收目标的使用状态。界面据此画开关，并区分「用户选过」与「跟随旧设置」。
#[derive(Debug, Serialize)]
pub(crate) struct TargetUseView {
    target: &'static str,
    /// 用户显式选择；`None` = 还没选过，跟随旧设置。
    preference: Option<bool>,
    /// 现在实际是否允许使用记忆。
    effective: bool,
    pool_enabled: bool,
    needs_consent: bool,
    /// 显式选择缺失、当前效值来自旧设置（界面显示「跟随原设置」而不是猜一个值）。
    follows_legacy: bool,
    warning: Option<String>,
}

fn file() -> std::path::PathBuf {
    paths::shell_dir(settings::current_mode()).join("memory-sharing/target-use.json")
}

fn fail(reason: impl std::fmt::Display) -> AppError {
    AppError::Memory(format!(
        "记忆使用开关不可用：{reason}；请刷新记忆页面并检查 {}",
        file().display()
    ))
}

fn read() -> Result<Targets, AppError> {
    match process::read_state_file::<Targets>(&file()) {
        process::StateRead::Missing => Ok(Targets {
            version: 1,
            ..Default::default()
        }),
        process::StateRead::Loaded(value) if value.version == 1 => Ok(value),
        process::StateRead::Loaded(_) => Err(fail("开关版本不受支持")),
        process::StateRead::Corrupt { reason } => Err(fail(reason)),
    }
}

fn target_key(family: &str) -> &'static str {
    if family == instance::KERNEL_FAMILY_MCODE {
        TARGET_MCODE
    } else {
        TARGET_DSH
    }
}

fn label(target: &str) -> &'static str {
    if target == TARGET_MCODE {
        "MiniMax"
    } else {
        "DSH"
    }
}

/// None 代表尚未迁移，空串代表用户显式关闭公共池。
fn pool_pair<'a>(targets: &'a Targets, family: &str) -> Option<&'a str> {
    if family == instance::KERNEL_FAMILY_MCODE {
        targets.mcode_pool_pair.as_deref()
    } else {
        targets.dsh_pool_pair.as_deref()
    }
}

pub(crate) fn pool_configured(family: &str) -> Result<bool, AppError> {
    Ok(pool_pair(&read()?, family).is_some())
}

pub(crate) fn pool_enabled_for(ns: &Namespace) -> Result<Option<bool>, AppError> {
    let targets = read()?;
    let Some(expected) = pool_pair(&targets, &ns.family) else {
        return Ok(None);
    };
    let preference = if ns.family == instance::KERNEL_FAMILY_MCODE {
        targets.mcode
    } else {
        targets.dsh
    };
    if expected.is_empty() || preference == Some(false) {
        return Ok(Some(false));
    }
    let pair = module::pair()?;
    Ok(Some(expected == module::key(&pair) && pair.contains(ns)))
}

/// 候选提取与引用共用一个内核开关；未迁移时维持旧授权范围。
pub(crate) fn extraction_enabled(ns: &Namespace) -> Result<bool, AppError> {
    if blocked(&ns.family) {
        return Ok(false);
    }
    if let Some(enabled) = pool_enabled_for(ns)? {
        return Ok(enabled);
    }
    Ok(store::load(ns)?.meta.use_approved)
}

/// 读失败留痕的节流：`blocked()` 在每次检索前都会跑，坏文件不该刷屏日志。
static LAST_READ_FAILURE_NOTE: std::sync::Mutex<Option<std::time::Instant>> =
    std::sync::Mutex::new(None);

/// 门禁 fail-closed 是**静默停用记忆**，必须留下可查的一条（带文件路径与下一步）。
fn note_read_failure(error: &AppError) {
    let Ok(mut last) = LAST_READ_FAILURE_NOTE.lock() else {
        return;
    };
    let now = std::time::Instant::now();
    if last
        .is_some_and(|previous| now.duration_since(previous) < std::time::Duration::from_secs(30))
    {
        return;
    }
    *last = Some(now);
    drop(last);
    crate::shell::shell_events::record(
        "memory-targets",
        &format!("记忆使用开关读取失败，已按关闭处理（自动检索与注入都不返回记忆）：{error}"),
    );
}

/// 自动检索 / 注入前的判据：`true` = 这个接收方现在不许使用记忆。
///
/// 读不出来按「关闭」：文件坏掉时无法知道用户选过什么，继续注入会扩大读取
/// 范围；状态命令会把原因报给界面，日志另有一条节流留痕。
pub(crate) fn blocked(family: &str) -> bool {
    match read() {
        Ok(targets) => {
            let preference = if target_key(family) == TARGET_MCODE {
                targets.mcode
            } else {
                targets.dsh
            };
            if preference == Some(false) {
                return true;
            }
            if let Some(expected) = pool_pair(&targets, family) {
                return expected.is_empty()
                    || module::pair()
                        .map(|pair| module::key(&pair) != expected)
                        .unwrap_or(true);
            }
            match target_key(family) {
                TARGET_MCODE => targets.mcode == Some(false),
                _ => targets.dsh == Some(false),
            }
        }
        Err(error) => {
            note_read_failure(&error);
            true
        }
    }
}

/// 每内核开关同时控制引用与候选提取；`pending=1` 不能绕过关闭状态。
/// 保留请求类型参数，以明确覆盖普通上下文和提取上下文两种调用。
pub(crate) fn blocks_context(family: &str, _pending: bool) -> bool {
    blocked(family)
}

/// 旧设置口径下「这个目标现在能不能用记忆」：自己的 `use_approved`，或已授权
/// 的模块互通。两边都是旧语义，这里只用来显示「跟随原设置」时的效值。
fn legacy_effective(ns: &Namespace) -> Result<bool, AppError> {
    let doc = store::load(ns)?;
    if doc.meta.schema_version > store::SCHEMA_VERSION {
        return Err(AppError::Memory(format!(
            "{} 记忆由更新版本写入，无法确认是否在用",
            label(target_key(&ns.family))
        )));
    }
    Ok(doc.meta.use_approved || module::enabled_for(ns).unwrap_or(false))
}

pub(crate) fn status() -> Vec<TargetUseView> {
    let (targets, file_warning) = match read() {
        Ok(targets) => (targets, None),
        Err(error) => (
            Targets {
                version: 1,
                ..Default::default()
            },
            Some(error.to_string()),
        ),
    };
    let dsh = Namespace::current();
    let mcode = crate::kernel::mcode_service::configured_instance_id()
        .ok()
        .flatten()
        .map(|id| Namespace::for_bridge(instance::KERNEL_FAMILY_MCODE, &id));
    let mut views = Vec::with_capacity(2);
    for (target, ns, preference) in [
        (TARGET_DSH, Some(dsh), targets.dsh),
        (TARGET_MCODE, mcode, targets.mcode),
    ] {
        let mut warning = file_warning.clone();
        let legacy = match ns.as_ref() {
            Some(ns) => match legacy_effective(ns) {
                Ok(value) => Some(value),
                Err(error) => {
                    warning = Some(match warning {
                        Some(existing) => format!("{existing}；{error}"),
                        None => error.to_string(),
                    });
                    None
                }
            },
            None => None,
        };
        let pool_enabled = ns.as_ref().is_some_and(|ns| {
            pool_enabled_for(ns)
                .ok()
                .flatten()
                .unwrap_or_else(|| module::enabled_for(ns).unwrap_or(false))
        });
        let configured_pool = pool_pair(
            &targets,
            if target == TARGET_MCODE {
                instance::KERNEL_FAMILY_MCODE
            } else {
                instance::KERNEL_FAMILY_DSH
            },
        );
        let consent_current = configured_pool.is_some_and(|expected| {
            !expected.is_empty() && module::pair().is_ok_and(|pair| module::key(&pair) == expected)
        });
        if configured_pool.is_some_and(|key| !key.is_empty()) && !consent_current {
            warning = Some("记忆来源范围已改变，请重新确认后启用；当前已停止引用公共池".into());
        }
        views.push(TargetUseView {
            target,
            preference,
            effective: if configured_pool.is_some() {
                pool_enabled
            } else {
                preference.or(legacy).unwrap_or(false)
            },
            pool_enabled: pool_enabled && preference != Some(false),
            // 旧双向许可只包含引用，不代表候选提取授权；只有新模式的
            // 当前来源快照可免确认，不能用旧模块的 enabled 替代。
            needs_consent: !consent_current,
            follows_legacy: preference.is_none() && legacy.is_some(),
            warning,
        });
    }
    views
}

pub(crate) fn set(target: &str, enabled: bool) -> Result<Vec<TargetUseView>, AppError> {
    if target != TARGET_DSH && target != TARGET_MCODE {
        return Err(AppError::Memory("记忆使用开关的目标无效".into()));
    }
    let _lock = super::share::lock_grants()?;
    // 读不出来（损坏或未来版本）就拒绝写入：凭一份读不出来的文件重建，会把
    // 另一个目标已经存下的显式选择静默清掉。报错带路径与下一步，记录与授权
    // 不受影响。
    let current = read().map_err(|error| {
        AppError::Memory(format!(
            "记忆使用开关无法保存：{error}。请修好或删除 {} 后重试（不会影响记忆记录与已有授权）",
            file().display()
        ))
    })?;
    let mut next = Targets {
        version: 1,
        dsh: current.dsh,
        mcode: current.mcode,
        dsh_pool_pair: current.dsh_pool_pair,
        mcode_pool_pair: current.mcode_pool_pair,
    };
    if target == TARGET_DSH {
        next.dsh = Some(enabled);
    } else {
        next.mcode = Some(enabled);
    }
    let path = file();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| fail(format!("无法准备目录：{error}")))?;
    }
    let bytes = serde_json::to_vec_pretty(&next).map_err(fail)?;
    crate::shell::atomic_write::atomic_write_private(&path, &bytes)
        .map_err(|error| fail(format!("无法保存：{error}")))?;
    Ok(status())
}

/// 一次原子写入完成“启用此内核 + 确认公共池范围”，不要求用户操作第二层开关。
pub(crate) fn set_pool(
    target: &str,
    enabled: bool,
    expected_pair: &str,
) -> Result<Vec<TargetUseView>, AppError> {
    if target != TARGET_DSH && target != TARGET_MCODE {
        return Err(fail("目标无效"));
    }
    let _lock = super::share::lock_grants()?;
    let mut next = read()?;
    let consent_key = if enabled {
        let pair = module::pair()?;
        let expected_family = if target == TARGET_MCODE {
            instance::KERNEL_FAMILY_MCODE
        } else {
            instance::KERNEL_FAMILY_DSH
        };
        if expected_pair != module::key(&pair)
            || !pair.iter().any(|ns| ns.family == expected_family)
        {
            return Err(fail("记忆来源或目标已改变，请刷新并重新确认"));
        }
        for ns in &pair {
            module::checked_doc(ns)?;
        }
        expected_pair.to_owned()
    } else {
        // 关闭保留已确认范围，原范围重开不重复询问；新来源仍需重新确认。
        pool_pair(
            &next,
            if target == TARGET_MCODE {
                instance::KERNEL_FAMILY_MCODE
            } else {
                instance::KERNEL_FAMILY_DSH
            },
        )
        .unwrap_or_default()
        .to_owned()
    };
    if target == TARGET_DSH {
        next.dsh = Some(enabled);
        next.dsh_pool_pair = Some(consent_key);
    } else {
        next.mcode = Some(enabled);
        next.mcode_pool_pair = Some(consent_key);
    }
    let bytes = serde_json::to_vec_pretty(&next).map_err(fail)?;
    crate::shell::atomic_write::atomic_write_private(&file(), &bytes).map_err(fail)?;
    crate::memory::recent::mark_setting_changed();
    Ok(status())
}

#[tauri::command]
pub async fn memory_set_pool_target(
    target: String,
    enabled: bool,
    expected_pair: String,
) -> Result<Vec<TargetUseView>, String> {
    crate::commands::blocking(move || set_pool(&target, enabled, &expected_pair)).await
}

#[tauri::command]
pub async fn memory_target_use_status() -> Result<Vec<TargetUseView>, String> {
    crate::commands::blocking(|| Ok::<_, String>(status())).await
}

#[tauri::command]
pub async fn memory_set_target_use(
    target: String,
    enabled: bool,
) -> Result<Vec<TargetUseView>, String> {
    crate::commands::blocking(move || set(&target, enabled).map_err(|error| error.to_string()))
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::store::testkit::{general_input, TempHome};

    fn seed(ns: &Namespace) {
        let mut input = general_input("偏好", "正文");
        input.pinned = true;
        store::save(ns, input).unwrap();
    }

    fn view(target: &str) -> TargetUseView {
        let views = status();
        views
            .into_iter()
            .find(|view| view.target == target)
            .expect("两个目标都要有状态")
    }

    #[test]
    fn no_explicit_choice_follows_legacy_without_blocking() {
        let _home = TempHome::new("target-default");
        assert!(!blocked(instance::KERNEL_FAMILY_DSH));
        assert!(!blocked(instance::KERNEL_FAMILY_MCODE));
        let dsh = Namespace::current();
        seed(&dsh);
        store::set_use_approved(&dsh, true).unwrap();
        let view = view(TARGET_DSH);
        assert_eq!(view.preference, None);
        assert!(view.follows_legacy);
        assert!(view.effective);
    }

    #[test]
    fn explicit_off_blocks_only_that_target_and_keeps_records() {
        let _home = TempHome::new("target-explicit");
        let dsh = Namespace::current();
        seed(&dsh);
        store::set_use_approved(&dsh, true).unwrap();
        set(TARGET_DSH, false).unwrap();
        assert!(blocked(instance::KERNEL_FAMILY_DSH));
        assert!(!blocked(instance::KERNEL_FAMILY_MCODE));
        // 记录与旧开关原样保留：关的是「用」，不是「有」。
        let doc = store::load(&dsh).unwrap();
        assert!(doc.meta.use_approved);
        assert_eq!(doc.memories.len(), 1);
        let view = view(TARGET_DSH);
        assert_eq!(view.preference, Some(false));
        assert!(!view.effective);
        assert!(!view.follows_legacy);
        set(TARGET_DSH, true).unwrap();
        assert!(!blocked(instance::KERNEL_FAMILY_DSH));
    }

    #[test]
    fn mcode_retrieval_returns_no_memory_when_target_is_off() {
        let _home = TempHome::new("target-mcode-gate");
        let mcode = Namespace::for_bridge(instance::KERNEL_FAMILY_MCODE, "target-mini");
        seed(&mcode);
        store::set_use_approved(&mcode, true).unwrap();
        let before = super::super::share::retrieve_mcode(&mcode, "偏好", "").unwrap();
        assert!(!before.hits.is_empty(), "旧开关打开时按旧行为检索");
        set(TARGET_MCODE, false).unwrap();
        let after = super::super::share::retrieve_mcode(&mcode, "偏好", "").unwrap();
        assert!(after.hits.is_empty(), "关闭后不得返回记忆");
        assert!(!after.enabled);
        assert!(blocks_context(instance::KERNEL_FAMILY_MCODE, false));
    }

    #[test]
    fn target_off_blocks_both_extraction_context_and_injection() {
        let _home = TempHome::new("target-pending");
        set(TARGET_DSH, false).unwrap();
        assert!(blocks_context(instance::KERNEL_FAMILY_DSH, false));
        assert!(blocks_context(instance::KERNEL_FAMILY_DSH, true));
    }

    #[test]
    fn corrupt_state_blocks_and_reports_instead_of_guessing() {
        let _home = TempHome::new("target-corrupt");
        let path = file();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"{ not json").unwrap();
        assert!(blocked(instance::KERNEL_FAMILY_DSH));
        assert!(blocked(instance::KERNEL_FAMILY_MCODE));
        assert!(status().iter().any(|view| view.warning.is_some()));
        // 坏文件上拒绝写入：不能凭读不出来的内容重建，把另一个目标的显式选择清掉。
        let error = set(TARGET_DSH, true).expect_err("坏文件上不许静默重建");
        assert!(error.to_string().contains("无法保存"), "{error}");
        assert!(path.exists(), "拒绝写入时原文件必须原样留着");
        // 用户删掉坏文件后恢复可写（记录与授权本来就不在这个文件里）。
        std::fs::remove_file(&path).unwrap();
        set(TARGET_DSH, true).unwrap();
        assert!(!blocked(instance::KERNEL_FAMILY_DSH));
    }

    #[test]
    fn explicit_choice_preserves_the_other_target_and_rejects_bad_target() {
        let _home = TempHome::new("target-preserve");
        set(TARGET_DSH, false).unwrap();
        set(TARGET_MCODE, false).unwrap();
        set(TARGET_DSH, true).unwrap();
        assert_eq!(view(TARGET_DSH).preference, Some(true));
        assert_eq!(
            view(TARGET_MCODE).preference,
            Some(false),
            "改一个目标不能清掉另一个目标的显式选择"
        );
        assert!(set("other", true).is_err(), "未知目标必须被拒绝");
        assert_eq!(view(TARGET_MCODE).preference, Some(false));
    }
}
