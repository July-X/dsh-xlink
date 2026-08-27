//! Workbench boot fault tolerance: watchdog, attribution, progressive
//! plugin disabling, and incident reporting.
//!
//! [`guarded_start`] wraps a kernel launch in a watchdog. The spawned child
//! must either answer its port within [`READY_TIMEOUT_SECS`] or exit; either
//! non-ready outcome triggers attribution over the kernel log tail, then up
//! to three boot attempts in progressively safer wiring states:
//!
//! 1. as wired (the pre-guard behavior);
//! 2. suspects from the log disabled via the quarantine registry;
//! 3. safe mode — every third-party plugin disabled.
//!
//! When even safe mode cannot boot, the flow restores the wiring and the
//! quarantine state captured before the incident and reports an unrecovered
//! [`Incident`] with an actionable hint. Every recovered outcome persists its
//! quarantines so the management UI can offer the keep-disabled / re-enable /
//! remove decision per suspect instead of leaving a dead workbench.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::process::read_tail;
use crate::quarantine::{self, QuarantineItem};
use crate::{kernel, plugins, settings};

/// How long the watchdog waits for the spawned kernel to answer its port
/// before treating the boot as hung and killing the process. A healthy
/// `dsh web` binds within a few seconds; thirty covers cold caches on
/// slow disks without stretching the failure path unreasonably.
const READY_TIMEOUT_SECS: u64 = 30;
/// Poll interval while watching a booting child.
const WATCH_POLL_MILLIS: u64 = 500;
/// Tail length read from `kernel.log` for attribution. Stack traces plus
/// loader chatter fit comfortably; the full log stays on disk regardless.
const LOG_TAIL_BYTES: u64 = 32 * 1024;
/// Suspects reported per incident. A boot failure rarely implicates more
/// than a couple of plugins; the cap keeps the incident panel readable.
const MAX_SUSPECTS: usize = 8;
/// Evidence excerpt cap per suspect, in characters.
const EVIDENCE_MAX_CHARS: usize = 480;

/// One plugin or kernel component the log evidence points at.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Suspect {
    /// `plugin` or `kernel`.
    pub kind: String,
    /// Store id for plugins, kernel version for the kernel itself.
    pub id: String,
    /// Display name shown verbatim in the UI.
    pub name: String,
    /// Log excerpt backing the attribution, shown on demand in the UI.
    pub evidence: String,
}

/// The outcome of one guarded start, persisted under the data dir so the
/// incident survives shell restarts and can be referenced by later messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Incident {
    /// Whether a retry after disabling plugins got the workbench up.
    pub recovered: bool,
    /// Whether the workbench is now running without some third-party
    /// plugins (recovered incidents only).
    pub safe_mode: bool,
    /// One-paragraph summary rendered as the panel headline (简体中文).
    pub message: String,
    pub suspects: Vec<Suspect>,
    /// Human-readable trail of what the guard tried, in order.
    pub attempts: Vec<String>,
    /// Tail of `kernel.log` at the moment of attribution.
    pub log_tail: String,
    /// Full path of `kernel.log` for the「打开日志」action.
    pub log_path: String,
    /// Actionable next step when not recovered (简体中文).
    pub hint: Option<String>,
    /// Seconds since epoch, for display.
    pub at: u64,
}

/// Result payload of the guarded `start_kernel` command.
#[derive(Debug, Clone, Serialize)]
pub struct StartReport {
    pub port: u16,
    /// Whether the workbench is serving when the command returns.
    pub running: bool,
    /// Convenience flag mirroring "quarantine registry is non-empty".
    pub safe_mode: bool,
    pub incident: Option<Incident>,
}

/// Everything the guard needs to spawn, rewire, and roll back.
pub struct GuardDeps<'a> {
    pub data_dir: &'a Path,
    pub settings: &'a settings::Settings,
    /// Validated node executable the kernel child is spawned with.
    pub node_path: &'a Path,
    /// Resolved pnpm executable used to re-sync the profile between attempts.
    pub pnpm_exe: &'a Path,
}

fn epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn kernel_log_path(data_dir: &Path) -> PathBuf {
    kernel::logs_dir(data_dir).join("kernel.log")
}

// --- watchdog ---------------------------------------------------------------

enum WatchVerdict {
    Ready,
    Exited(std::process::ExitStatus),
    Hung,
}

/// Watch a freshly spawned kernel until the port answers, the process exits,
/// or the deadline passes. A hung child is killed here (whole process group,
/// matching how「关闭工作台」tears kernels down) so a half-booted instance can
/// never linger and later masquerade as a running workbench.
fn watch_child(child: &mut Child, port: u16) -> WatchVerdict {
    let deadline = Instant::now() + Duration::from_secs(READY_TIMEOUT_SECS);
    loop {
        if kernel::port_open(port) {
            return WatchVerdict::Ready;
        }
        if let Ok(Some(status)) = child.try_wait() {
            return WatchVerdict::Exited(status);
        }
        // An OS-level wait error (the `Err` arm) means the child state is
        // unknowable; keep polling the port so a healthy boot still wins
        // the race.
        if Instant::now() >= deadline {
            let _ = kernel::stop(child);
            return WatchVerdict::Hung;
        }
        std::thread::sleep(Duration::from_millis(WATCH_POLL_MILLIS));
    }
}

enum BootVerdict {
    Ready,
    Failed(String),
    Hung,
}

impl BootVerdict {
    fn reason(&self) -> String {
        match self {
            BootVerdict::Ready => String::from("启动成功"),
            BootVerdict::Failed(detail) => detail.clone(),
            BootVerdict::Hung => format!("等待内核就绪超时（{READY_TIMEOUT_SECS} 秒）"),
        }
    }
}

/// One guarded boot attempt: spawn through the ordinary path, then watch.
/// Returns the verdict plus the live child on the `Ready` path (the caller
/// registers it with the app state); failures consume the child.
fn boot_once(
    deps: &GuardDeps<'_>,
    on_progress: &mut dyn FnMut(&str),
) -> (BootVerdict, Option<Child>) {
    match kernel::start_maybe(deps.data_dir, deps.node_path) {
        Ok(None) => (
            // The port started answering mid-flow (another shell instance or
            // a leftover orphan won the race). Treat as ready; reap_orphans
            // owns the orphan case at shell startup.
            BootVerdict::Ready,
            None,
        ),
        Ok(Some(mut child)) => match watch_child(&mut child, deps.settings.port) {
            WatchVerdict::Ready => (BootVerdict::Ready, Some(child)),
            WatchVerdict::Exited(status) => {
                let _ = child.wait();
                on_progress(&format!("内核进程在就绪前退出（{status}）"));
                (
                    BootVerdict::Failed(format!("内核进程在就绪前退出（{status}）")),
                    None,
                )
            }
            WatchVerdict::Hung => {
                let _ = child.wait();
                (BootVerdict::Hung, None)
            }
        },
        Err(e) => {
            on_progress(&format!("无法拉起内核进程：{e}"));
            (BootVerdict::Failed(e.to_string()), None)
        }
    }
}

// --- attribution ------------------------------------------------------------

/// Lines that plausibly carry the failure cause. Boot logs interleave loader
/// chatter («loading plugin …») with the actual error; matching only
/// error-shaped lines keeps chatty-but-innocent plugins out of the suspect
/// list, which is what makes the automatic disable safe to run unattended.
fn is_error_line(line: &str) -> bool {
    const MARKERS: [&str; 8] = [
        "Error", "error", "ERR_", "Cannot", "cannot", "throw", "Failed", "failed",
    ];
    MARKERS.iter().any(|marker| line.contains(marker))
}

/// Join the hit line with one line of context on each side, capped.
fn excerpt(lines: &[&str], idx: usize) -> String {
    let start = idx.saturating_sub(1);
    let end = (idx + 2).min(lines.len());
    let joined = lines[start..end].join("\n");
    if joined.chars().count() <= EVIDENCE_MAX_CHARS {
        return joined;
    }
    let truncated: String = joined.chars().take(EVIDENCE_MAX_CHARS).collect();
    format!("{truncated}…")
}

/// Attribute a boot failure to installed plugins (or the kernel itself) from
/// the log tail. Plugin candidates match on anchored shapes only — their
/// shell materialization path segment `plugins/<id>`, a `/`-prefixed path
/// segment of the package name, or the name in quotes — because a bare
/// substring test turns every short package name into a suspect whenever the
/// word happens to occur anywhere in a stack trace.
///
/// Conservative by design: no candidate match yields an empty list, and the
/// empty case routes to safe mode rather than blaming an arbitrary plugin.
pub fn attribute(
    log_tail: &str,
    store_items: &[plugins::StoreItem],
    kernel_label: &str,
) -> Vec<Suspect> {
    let lines: Vec<&str> = log_tail.lines().collect();
    let mut suspects: Vec<Suspect> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for item in store_items {
        if suspects.len() >= MAX_SUSPECTS {
            return suspects;
        }
        // `plugins/<id>` covers link-mode kernel plugins dirs; `/name` also
        // hits `/name/...` inside copy-mode profile paths; the quoted forms
        // catch `Cannot find package 'x'` / `Cannot find module 'x'`.
        let needles = [
            format!("plugins/{}", item.id),
            format!("/{}", item.name),
            format!("'{}'", item.name),
            format!("\"{}\"", item.name),
        ];
        let Some(idx) = lines.iter().position(|line| {
            is_error_line(line) && needles.iter().any(|needle| line.contains(needle))
        }) else {
            continue;
        };
        if seen.insert(item.id.clone()) {
            suspects.push(Suspect {
                kind: String::from("plugin"),
                id: item.id.clone(),
                name: item.name.clone(),
                evidence: excerpt(&lines, idx),
            });
        }
    }

    if suspects.is_empty() {
        // Corrupted kernel install signal: module resolution failing on the
        // dsh package itself. Any crash trace references the entry file, so
        // only a resolution failure on the package counts as kernel evidence.
        let Some(idx) = lines.iter().position(|line| {
            is_error_line(line)
                && line.contains("@deepseek-ai/dsh")
                && (line.contains("Cannot find") || line.contains("ERR_MODULE_NOT_FOUND"))
        }) else {
            return suspects;
        };
        suspects.push(Suspect {
            kind: String::from("kernel"),
            id: kernel_label.to_string(),
            name: format!("dsh 内核 {kernel_label}"),
            evidence: excerpt(&lines, idx),
        });
    }
    suspects
}

fn suspect_records(suspects: &[Suspect], reason: String) -> Vec<QuarantineItem> {
    suspects
        .iter()
        .filter(|s| s.kind == "plugin")
        .map(|s| QuarantineItem {
            id: s.id.clone(),
            name: s.name.clone(),
            reason: reason.clone(),
            evidence: s.evidence.clone(),
            at: epoch_secs(),
        })
        .collect()
}

// --- wiring helpers ---------------------------------------------------------

/// First-attempt wiring repair, identical in spirit to the pre-guard quiet
/// pass: failures land in the store warning instead of blocking the boot.
fn sync_wiring_record_warning(deps: &GuardDeps<'_>, on_progress: &mut dyn FnMut(&str)) {
    match plugins::ensure_wiring(deps.data_dir, deps.settings, deps.pnpm_exe, on_progress) {
        Ok(_) => plugins::set_store_warning(deps.data_dir, None),
        Err(e) => plugins::set_store_warning(deps.data_dir, Some(e.to_string())),
    }
}

/// Re-sync the profile between attempts, appending the outcome to the
/// incident trail either way.
fn refresh_wiring(
    deps: &GuardDeps<'_>,
    on_progress: &mut dyn FnMut(&str),
    trail: &mut Vec<String>,
) {
    match plugins::ensure_wiring(deps.data_dir, deps.settings, deps.pnpm_exe, on_progress) {
        Ok((count, changed)) => trail.push(format!(
            "已按屏蔽清单重新接线（{count} 个插件接入，清单变更：{changed}）"
        )),
        Err(e) => trail.push(format!("重新接线失败：{e}")),
    }
}

fn log_tail(deps: &GuardDeps<'_>) -> String {
    read_tail(&kernel_log_path(deps.data_dir), LOG_TAIL_BYTES)
}

// --- incident persistence ---------------------------------------------------

fn incident_path(data_dir: &Path) -> PathBuf {
    data_dir.join("last-incident.json")
}

/// Persist the incident so it survives shell restarts; best-effort because a
/// failed write must not mask the boot outcome the user is waiting on.
fn save_incident(data_dir: &Path, incident: &Incident) {
    if let Ok(text) = serde_json::to_string_pretty(incident) {
        let _ = std::fs::write(incident_path(data_dir), text + "\n");
    }
}

/// Drop the recorded incident after a clean normal boot — the stale report
/// would otherwise contradict a workbench that just came up healthy.
fn clear_incident(data_dir: &Path) {
    let _ = std::fs::remove_file(incident_path(data_dir));
}

/// Read the last recorded incident (used by commands that surface history).
pub fn load_incident(data_dir: &Path) -> Option<Incident> {
    let text = std::fs::read_to_string(incident_path(data_dir)).ok()?;
    serde_json::from_str(&text).ok()
}

// --- orchestration ----------------------------------------------------------

/// Launch the active kernel under the boot guard. Returns the report for the
/// UI plus the live child on success (the caller registers it with the app
/// state and records its pid).
pub fn guarded_start(
    deps: &GuardDeps<'_>,
    on_progress: &mut dyn FnMut(&str),
) -> (StartReport, Option<Child>) {
    let port = deps.settings.port;

    if kernel::port_open(port) {
        // Idempotent start: something already serves the port. Do not nag
        // about historical quarantines here; the overview banner owns that.
        return (
            StartReport {
                port,
                running: true,
                safe_mode: false,
                incident: None,
            },
            None,
        );
    }

    sync_wiring_record_warning(deps, on_progress);

    let store_items = plugins::load_store(deps.data_dir).items;
    let kernel_label = kernel::read_active(deps.data_dir).unwrap_or_default();
    let prior_quarantine = quarantine::load(deps.data_dir);
    let manifest_snapshot =
        plugins::snapshot_profile_manifest_text(deps.data_dir, &deps.settings.profile);
    let mut trail: Vec<String> = Vec::new();

    // Attempt 1: exactly as wired.
    on_progress("正在启动工作台…");
    let (verdict, child) = boot_once(deps, on_progress);
    // A `Ready` verdict without a child means something else started
    // answering the port mid-flow; that is still a running workbench.
    if matches!(verdict, BootVerdict::Ready) {
        clear_incident(deps.data_dir);
        return (
            StartReport {
                port,
                running: true,
                safe_mode: !prior_quarantine.items.is_empty(),
                incident: None,
            },
            child,
        );
    }
    trail.push(format!("常规启动失败：{}", verdict.reason()));
    let tail = log_tail(deps);
    let mut suspects = attribute(&tail, &store_items, &kernel_label);

    // Attempt 2: disable the attributed suspects and retry. Skipped when
    // attribution found nothing — guessing would punish innocent plugins.
    if !suspects.is_empty() {
        on_progress("检测到疑似引发故障的插件，正在停用后重试…");
        let records = suspect_records(
            &suspects,
            String::from("内核启动失败，错误日志指向该插件，已自动停用"),
        );
        if quarantine::add_all(deps.data_dir, &records).is_ok() {
            refresh_wiring(deps, on_progress, &mut trail);
            let (verdict2, child2) = boot_once(deps, on_progress);
            if matches!(verdict2, BootVerdict::Ready) {
                trail.push("停用疑似插件后启动成功".to_string());
                let incident = Incident {
                    recovered: true,
                    safe_mode: true,
                    message: String::from(
                        "工作台已在停用以下插件后成功启动。请查看错误原因，选择移除或保持禁用；确认插件已修复后可重新启用。",
                    ),
                    suspects,
                    attempts: trail,
                    log_tail: tail,
                    log_path: kernel_log_path(deps.data_dir).display().to_string(),
                    hint: None,
                    at: epoch_secs(),
                };
                save_incident(deps.data_dir, &incident);
                return (
                    StartReport {
                        port,
                        running: true,
                        safe_mode: true,
                        incident: Some(incident),
                    },
                    child2,
                );
            }
            trail.push(format!("停用疑似插件后仍失败：{}", verdict2.reason()));
            suspects.extend(attribute(&log_tail(deps), &store_items, &kernel_label));
        } else {
            trail.push(String::from("写入隔离记录失败，跳过定向停用"));
        }
    }

    // Attempt 3: safe mode. With no third-party plugins there is nothing to
    // disable — a bare-profile failure is the kernel or environment's fault.
    if !store_items.is_empty() {
        on_progress("仍未启动成功，正在进入安全模式（停用全部第三方插件）后重试…");
        let already: HashSet<String> = quarantined_ids_now(deps.data_dir);
        let rest: Vec<Suspect> = store_items
            .iter()
            .filter(|item| !already.contains(&item.id))
            .map(|item| Suspect {
                kind: String::from("plugin"),
                id: item.id.clone(),
                name: item.name.clone(),
                evidence: String::new(),
            })
            .collect();
        let records = suspect_records(
            &rest,
            String::from("无法定位具体引发故障的插件，安全模式已停用全部第三方插件"),
        );
        if quarantine::add_all(deps.data_dir, &records).is_ok() {
            refresh_wiring(deps, on_progress, &mut trail);
            let (verdict3, child3) = boot_once(deps, on_progress);
            if matches!(verdict3, BootVerdict::Ready) {
                trail.push("安全模式（全部第三方插件停用）下启动成功".to_string());
                // Report every plugin as a suspect: the user must decide per
                // plugin whether to remove it or leave it disabled, and the
                // ones with real log evidence carry their excerpts.
                let all_suspects: Vec<Suspect> = store_items
                    .iter()
                    .map(|item| Suspect {
                        kind: String::from("plugin"),
                        id: item.id.clone(),
                        name: item.name.clone(),
                        evidence: suspects
                            .iter()
                            .find(|s| s.id == item.id)
                            .map(|s| s.evidence.clone())
                            .unwrap_or_default(),
                    })
                    .take(MAX_SUSPECTS)
                    .collect();
                let incident = Incident {
                    recovered: true,
                    safe_mode: true,
                    message: String::from(
                        "工作台仅在不加载任何第三方插件时才能启动，已将全部插件临时停用。请逐个查看并决定移除或恢复。",
                    ),
                    suspects: all_suspects,
                    attempts: trail,
                    log_tail: tail,
                    log_path: kernel_log_path(deps.data_dir).display().to_string(),
                    hint: None,
                    at: epoch_secs(),
                };
                save_incident(deps.data_dir, &incident);
                return (
                    StartReport {
                        port,
                        running: true,
                        safe_mode: true,
                        incident: Some(incident),
                    },
                    child3,
                );
            }
            trail.push(format!("安全模式下仍失败：{}", verdict3.reason()));
        } else {
            trail.push(String::from("写入隔离记录失败，跳过安全模式"));
        }
    }

    // Everything failed: this is not plugin-induced. Undo everything the
    // guard changed so a fix applied outside the guard (reinstalling the
    // kernel version, replacing a broken disk state) is never masked by
    // half-applied quarantine or wiring state.
    on_progress("多次尝试后仍无法启动，正在恢复原有配置…");
    trail.push(String::from("已放弃自动修复，恢复原有接线与隔离状态"));
    let _ = quarantine::save(deps.data_dir, &prior_quarantine);
    if let Err(e) = plugins::restore_profile_manifest(
        deps.data_dir,
        deps.settings,
        deps.pnpm_exe,
        manifest_snapshot.as_deref(),
        on_progress,
    ) {
        trail.push(format!("恢复原接线失败：{e}"));
    }
    let kernel_suspected = suspects.iter().any(|s| s.kind == "kernel");
    let multiple_versions = kernel::list_installed(deps.data_dir).len() > 1;
    let mut hint = String::from("此次失败与第三方插件无关。请通过「打开日志」查看完整内核日志；也可在「内核版本」页删除当前版本后重新安装。");
    if kernel_suspected || multiple_versions {
        hint = format!("也可先尝试在「内核版本」页切换到其他已安装版本。{hint}");
    }
    let incident = Incident {
        recovered: false,
        safe_mode: false,
        message: String::from("多次尝试后工作台仍无法启动，已恢复原有插件配置。"),
        suspects: dedup_suspects(suspects),
        attempts: trail,
        log_tail: tail,
        log_path: kernel_log_path(deps.data_dir).display().to_string(),
        hint: Some(hint),
        at: epoch_secs(),
    };
    save_incident(deps.data_dir, &incident);
    (
        StartReport {
            port,
            running: false,
            safe_mode: false,
            incident: Some(incident),
        },
        None,
    )
}

fn quarantined_ids_now(data_dir: &Path) -> HashSet<String> {
    quarantine::ids(data_dir)
}

/// Merge attribution results across attempts by id, keeping first evidence.
fn dedup_suspects(suspects: Vec<Suspect>) -> Vec<Suspect> {
    let mut out: Vec<Suspect> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for suspect in suspects {
        if seen.insert(suspect.id.clone()) {
            out.push(suspect);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_item(id: &str, name: &str) -> plugins::StoreItem {
        plugins::StoreItem {
            id: id.to_string(),
            name: name.to_string(),
            origin: String::from("npm"),
            source: name.to_string(),
            installed_version: String::from("1.0.0"),
            latest_version: None,
            mode: String::from("link"),
            pinned: false,
            installed_at: String::new(),
            updated_at: String::new(),
            repo_url: None,
            description: None,
        }
    }

    #[test]
    fn attributes_link_layout_by_materialized_path() {
        // Link-mode crashes resolve through the kernel plugins dir, whose
        // segment carries the store id (`/` → `__`), not the npm name.
        let tail = "node:internal/modules/esm/resolve\n\
                    Error: Cannot find module '/Users/u/.dsh/desktop/kernels/0.1.1/plugins/@scope__pkg/lib/index.js'\n";
        let items = vec![store_item("@scope__pkg", "@scope/pkg")];
        let suspects = attribute(tail, &items, "0.1.1");
        assert_eq!(suspects.len(), 1);
        assert_eq!(suspects[0].kind, "plugin");
        assert_eq!(suspects[0].id, "@scope__pkg");
        assert!(suspects[0].evidence.contains("Cannot find module"));
    }

    #[test]
    fn attributes_copy_layout_by_package_name() {
        // Copy-mode crashes resolve through the profile node_modules, which
        // carries the package name rather than the store id.
        let tail = "Error [ERR_MODULE_NOT_FOUND]: Cannot find package '@scope/pkg' imported from /Users/u/.dsh/profiles/web/node_modules/@scope/pkg/lib/index.js\n";
        let items = vec![store_item("@scope__pkg", "@scope/pkg")];
        let suspects = attribute(tail, &items, "0.1.1");
        assert_eq!(suspects.len(), 1);
        assert_eq!(suspects[0].name, "@scope/pkg");
    }

    #[test]
    fn ignores_chatty_but_innocent_plugins() {
        // Loader progress lines mention many plugins; they are not
        // error-shaped, so the chatty loader must stay out of the suspect
        // list even though the failing package appears right next to it.
        let tail = "[loader] loading plugin alpha\n\
                    [loader] loading plugin beta\n\
                    Error: Cannot find package 'beta' imported from lib/index.js\n";
        let items = vec![store_item("alpha", "alpha"), store_item("beta", "beta")];
        let suspects = attribute(tail, &items, "0.1.1");
        assert_eq!(suspects.len(), 1);
        assert_eq!(suspects[0].id, "beta");
    }

    #[test]
    fn bare_substring_occurrences_do_not_blame_short_names() {
        // A short package name occurring unquoted inside an unrelated error
        // line is not evidence; only the anchored shapes count.
        let tail = "Error: something failed while preparing plugins\n";
        let items = vec![store_item("p", "p"), store_item("plug_x", "plug")];
        assert!(attribute(tail, &items, "0.1.1").is_empty());
    }

    #[test]
    fn no_match_yields_empty_list() {
        let tail = "Error: EADDRINUSE: address already in use 127.0.0.1:3090\n";
        let items = vec![store_item("p", "p")];
        assert!(attribute(tail, &items, "0.1.1").is_empty());
    }

    #[test]
    fn kernel_fallback_when_no_plugin_matches() {
        let tail = "Error: Cannot find module '@deepseek-ai/dsh/lib/bin.js'\n";
        let items = vec![store_item("p", "p")];
        let suspects = attribute(tail, &items, "0.1.2");
        assert_eq!(suspects.len(), 1);
        assert_eq!(suspects[0].kind, "kernel");
        assert_eq!(suspects[0].id, "0.1.2");
    }

    #[test]
    fn excerpt_caps_long_evidence() {
        let long_line = format!("Error: {}", "x".repeat(2000));
        let lines = vec!["context", long_line.as_str()];
        let text = excerpt(&lines, 1);
        assert!(text.chars().count() <= EVIDENCE_MAX_CHARS + 1);
        assert!(text.ends_with('…'));
    }
}
