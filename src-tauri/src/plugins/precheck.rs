//! 安装预检的两段式事务：先在一次性沙盒里真的装一次、真的起一次内核，
//! 通过了才把包提交到目标实例。
//!
//! ## 分层
//!
//! - [`crate::plugins::sandbox`] 只管「起一个临时内核、探它、收摊」，与装什么无关，
//!   技能预检将来直接复用它。
//! - 本模块管**事务**：快照中央库 → 装进沙盒 → 启动探测 → 提交或回滚。
//! - [`crate::plugins::center`] 只暴露两条缝：真实的安装入口（`install_for_instance`）
//!   与 `store.json` 的路径。预检因此能验证**生产安装路径本身**，而不是
//!   它的某种近似。
//!
//! 为什么单独成文件而不是塞进 `plugins.rs`：预检是一次跨「取源 / 物化 /
//! 接线 / 启动」的长事务，把它埋在已经 3000 行的插件模块里，会让两件事
//! 同时恶化——插件模块读不懂，预检想复用到技能上也无从下手。
//!
//! ## 判定为什么要有基线
//!
//! 只跑一次「装了候选包的内核」无法归因：它起不来，可能是候选包的锅，也
//! 可能是用户环境本来就坏了。`guard.rs` 早就为同一个问题付过代价——它宁
//! 可放弃插件归因，也不肯「因为环境问题去停用一批无辜插件」。这里用同一
//! 条纪律：**先在不装任何插件的沙盒里起一次作为基线**，只有基线正常、而
//! 装了候选包之后才失败，才判 [`sandbox::Verdict::Fail`]。
//!
//! ## 预检期间用户环境的变化
//!
//! 物化与接线只发生在沙盒实例里。真实实例的 `extensions/` 与
//! `wiring.json` 在预检通过之前一个字节都不会变。判 `Fail` 时中央库按
//! 快照逐字节回滚，用户看到的最终状态与点「安装」之前完全一致。

use crate::plugins;
use std::path::Path;
use std::time::Instant;

/// 预检阶段事件：写进运行记录并立刻推通道。
///
/// 与 `commands.rs` 的同名宏同一条纪律：通道里只发结构化信封
/// （`run::channel_envelope`），进度浮层取它的 `message` 显示人话。
macro_rules! precheck_stage {
    ($send:expr, $recorder:expr, $stage:expr, $status:expr, $message:expr $(,)?) => {{
        $recorder.push($stage, $status, $message, None, None);
        for envelope in $recorder.take_pending() {
            $send(&envelope);
        }
    }};
}

use crate::plugins::sandbox;
use crate::shell::error::AppError;
use crate::shell::settings;

/// 一次沙盒启动的观测结果。
struct BootProbe {
    /// 内核是否正常应答（起来 + HTTP 2xx/3xx）。
    ready: bool,
    /// 给人看的一句话，说明「卡在哪一步」。
    detail: String,
    /// 该次启动的内核日志末尾。
    log: String,
}

/// 预检期间对中央库做的快照。回滚靠它把「用户从没装过这个包」的磁盘状态
/// 原样还回去——**按字节还原 `store.json`**，而不是反算字段：预检的失败
/// 可能发生在写清单的任意一环，按字段反算等于让预检自己实现一套 store
/// 写语义，那正是它要检验的东西。
struct StoreSnapshot {
    /// `store.json` 原始字节；`None` = 预检前根本没有这个文件。
    bytes: Option<Vec<u8>>,
    /// 预检前中央库里已有的条目名。快照之后新增的都属于本次预检。
    entries: std::collections::BTreeSet<String>,
}

impl StoreSnapshot {
    fn capture(data_dir: &Path) -> Self {
        let entries = std::fs::read_dir(plugins::center::store_dir(data_dir))
            .map(|dir| {
                dir.flatten()
                    .filter_map(|e| e.file_name().into_string().ok())
                    .collect()
            })
            .unwrap_or_default();
        StoreSnapshot {
            bytes: std::fs::read(plugins::center::store_file(data_dir)).ok(),
            entries,
        }
    }

    /// 把中央库还原到快照时刻。**不会**碰预检前就存在的目录——用户自己
    /// 装的插件绝不能被一次失败的预检带走。
    fn rollback(&self, data_dir: &Path) {
        let file = plugins::center::store_file(data_dir);
        match &self.bytes {
            Some(raw) => {
                let _ = crate::shell::process::atomic_write(&file, raw);
            }
            None => {
                let _ = std::fs::remove_file(&file);
            }
        }
        if let Ok(dir) = std::fs::read_dir(plugins::center::store_dir(data_dir)) {
            for entry in dir.flatten() {
                let name = entry.file_name();
                let Some(name) = name.to_str() else { continue };
                if self.entries.contains(name) {
                    continue;
                }
                let path = entry.path();
                if path.is_dir() {
                    let _ = std::fs::remove_dir_all(&path);
                } else {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
    }
}

/// 在沙盒里起一次内核、探一次 HTTP、收摊，读回日志。**任何**返回路径都
/// 已经把子进程停掉，调用方拿到的 `sandbox` 可以安全地继续用。
fn probe_boot(
    sandbox: &mut sandbox::Sandbox,
    install_root: &Path,
    node_path: &Path,
    on_progress: &mut dyn FnMut(&str),
) -> BootProbe {
    if let Err(error) = sandbox.start(install_root, node_path) {
        let log = sandbox.read_log_tail();
        return BootProbe {
            ready: false,
            detail: error,
            log,
        };
    }
    // `probe` 自己就判「什么算就绪」（2xx/3xx，且必须带得进去），所以这里
    // 不再重复一遍状态码区间——两处各判一次，迟早有一处忘了带上启动令牌。
    let probe = sandbox.probe();
    let (ready, detail) = match probe {
        Ok(code) => (true, format!("内核应答 HTTP {code}")),
        Err(error) => (false, error),
    };
    let log = sandbox.read_log_tail();
    sandbox.shutdown();
    on_progress(&format!("沙盒内核探测结果：{detail}"));
    BootProbe { ready, detail, log }
}

/// 把候选插件的来源信息填进报告（设计 §6.5 首屏五项）。
///
/// **为什么单独一个函数**：三条返回路径（沙盒建不起来、基线失败、正常走完）
/// 都要填同一批字段，而它们分处函数前中后三段。散着填必然漏一条——漏掉的
/// 那条在 UI 上表现为「来源未知」，用户看到的是一个不敢装的包。
fn fill_source_info(
    report: &mut sandbox::PrecheckReport,
    spec_str: &str,
    mode: &str,
    family: &str,
    target_instance: &str,
) {
    use crate::shell::instance;
    // 解析失败不阻断预检：取源阶段本来就会再解析一次并给出真正的错误。
    // 这里失败就把来源留空，让 UI 显示「来源未知」——**不编一个假来源**。
    if let Ok(spec) = plugins::center::parse_spec(spec_str) {
        report.source_kind = spec.origin.clone();
        // 只给短名称（npm 包名 / `owner/repo`），不给完整 URL——后者可能
        // 带凭据（`https://user:token@…`），而报告会进运行记录。
        report.source_label = if let Some(repo) = &spec.repo_url {
            repo.rsplit('/').next().unwrap_or(repo.as_str()).to_string()
        } else {
            spec.source.clone()
        };
        report.pin = spec.pin.clone().unwrap_or_default();
    }
    report.materialize = mode.to_string();
    report.target_instance = target_instance.to_string();
    let (default_family, default_instance) = instance::resolve_default();
    report.affects_default_instance =
        family == default_family && target_instance == default_instance;
}

/// 插件安装预检：在一个一次性沙盒实例里**真的装一次、真的起一次**内核，
/// 通过了才把包物化到目标实例。
#[allow(clippy::too_many_arguments)]
pub fn plugin_install(
    family: &str,
    target_instance: &str,
    data_dir: &Path,
    settings: &settings::Settings,
    pnpm_exe: &Path,
    node_path: &Path,
    spec_str: &str,
    mode: &str,
    on_progress: &mut dyn FnMut(&str),
) -> Result<sandbox::PrecheckReport, AppError> {
    use crate::diagnostics::run;
    use crate::plugins::sandbox::Verdict;
    let _store_guard = plugins::center::lock_store();
    let started = Instant::now();

    // 目标实例的**内核还在跑**时拒绝预检（设计 §6.2）。判据按实例 pid 文件
    // 而不是「本壳工作台在不在跑」：用户自建实例有意留在两份注册表里，
    // 另一个壳正跑着它时本壳的判据会说「没在跑」，于是预检会把插件装进
    // 一个正在被使用的实例——而预检末尾的 commit 会**改接线**，那等于在
    // 用户正工作的时候动它的依赖。
    if let Some(record) = crate::shell::instance::instance_kernel_running(family, target_instance) {
        // 复用 instance.rs 那份文案：同一句话写两遍，迟早有一处漏掉
        // 「先关闭再试」这半句——而那半句正是用户下一步要做的事。
        return Err(AppError::Io(
            crate::shell::instance::instance_kernel_running_message(
                &record,
                target_instance,
                "做安装预检",
            ),
        ));
    }

    let version = crate::kernel::lifecycle::read_active(data_dir).ok_or_else(|| {
        AppError::Kernel(
            "本机还没有启用任何内核版本，无法为插件做启动预检。请先到「内核版本」页安装并启用一个版本"
                .into(),
        )
    })?;
    // 与 `kernel::start_instance` 完全同一套解析：优先适配器声明的新位置，
    // 落回 legacy `kernels/<version>/`。预检必须跑的是**生产同款**内核。
    let install_root = crate::kernel::kernel_adapter::lookup(family)
        .and_then(|adapter| adapter.resolve_install_dir(&version))
        .unwrap_or_else(|| crate::kernel::lifecycle::kernel_dir(data_dir, &version));

    // 运行记录在**读完内核版本之后**才开：预检跑的是这个版本的沙盒内核，
    // 版本都读不到时根本没有一次可追溯的「预检」——那次只是一次安装。
    let mut recorder = run::Recorder::begin(
        family,
        target_instance,
        run::kind::PLUGIN_PRECHECK,
        &version,
        &settings.profile,
        Default::default(),
    );
    precheck_stage!(
        on_progress,
        &mut recorder,
        run::stage::SANDBOX_CREATE,
        run::status::RUNNING,
        "正在创建一次性沙盒环境"
    );
    let mut sandbox = match sandbox::Sandbox::create(
        family,
        &version,
        &settings.profile,
        &sandbox::used_ports(family),
        on_progress,
    ) {
        Ok(sandbox) => sandbox,
        Err(reason) => {
            // 沙盒自己都建不起来：无从判断候选包好坏。按 fail-open 处理，仍
            // 然走正常安装（取源与完整性校验照样生效），但报告必须说清这次
            // 安装**没有经过启动验证**。
            // **不再直接安装**（两阶段契约，2026-10-06 用户拍板）：沙盒都建
            // 不起来时没有任何证据支持安装，替他装上就是把「没验过」说成
            // 「验过没问题」。这里如实返回 inconclusive，应用按钮据此禁用。
            let mut report = sandbox::PrecheckReport::new("", spec_str, Verdict::Inconclusive);
            fill_source_info(&mut report, spec_str, mode, family, target_instance);
            report.summary = format!("预检环境不可用，没有安装任何东西：{reason}");
            report.hint = "这条提示说明预检环境本身不可用，与插件质量无关。可以在插件中心关闭安装预检后直接安装，或查看日志确认沙盒为何起不来。"
                .into();
            report.duration_ms = started.elapsed().as_millis() as u64;
            // 归因 `environment`：沙盒起不来与候选插件无关，写成 plugin
            // 会让用户去处置一个无辜的包。
            precheck_stage!(
                on_progress,
                &mut recorder,
                run::stage::SANDBOX_CREATE,
                run::status::FAILURE,
                &format!("预检环境不可用：{reason}")
            );
            let _ = recorder.finish(
                run::status::INCONCLUSIVE,
                run::cause::ENVIRONMENT,
                &format!("预检环境不可用，未安装任何插件：{reason}"),
                None,
            );
            report.run_id = recorder.id().to_string();
            report.verified_at_ms = crate::shell::process::epoch_millis();
            return Ok(report);
        }
    };

    on_progress("正在建立环境基线：不装任何插件启动一次内核");
    precheck_stage!(
        on_progress,
        &mut recorder,
        run::stage::BASELINE,
        run::status::RUNNING,
        "正在建立环境基线（不装任何插件）"
    );
    let baseline = probe_boot(&mut sandbox, &install_root, node_path, on_progress);
    if !baseline.ready {
        let mut report = sandbox::PrecheckReport::new("", spec_str, Verdict::Inconclusive);
        report.summary = format!(
            "环境基线就没能起来，预检无法进行：{}。没有安装任何东西",
            baseline.detail
        );
        // 报告里的证据会原样渲染进对话框的 <pre>，而内核启动那行带
        // `?token=`——它是当前进程的入口凭据。落盘那份日志不受影响（那是用户
        // 自己的机器），**进报告的这几行要过一遍脱敏**：`run.rs` 的纪律是
        // 「记录不含凭据」，这条报告也是会落进运行记录的东西。
        report.evidence = crate::diagnostics::run::sanitize(&baseline.log);
        // 沙盒目录随 `Drop` 一起删掉，所以此刻就把内核日志另存一份。
        // 2026-10-06 这次基线失败之所以查了半天，是因为这条路径**根本没有**
        // 取证：报告里的 `evidence` 是一段空字符串（日志本身也是空的——内核
        // 静默退出），沙盒目录又被删干净，磁盘上什么都不剩。fail 路径留了
        // 取证、基线路径没留，等于「最需要线索的那次没线索」。
        if let Some(path) = sandbox::preserve_evidence(data_dir, &sandbox, "baseline") {
            crate::shell::shell_events::record(
                "precheck",
                &format!("环境基线未能启动，内核日志已存至 {}", path.display()),
            );
        }
        report.hint = "先不装任何插件时内核在沙盒里也起不来，问题不在候选插件。请查看下方日志确认是内核版本、Node 环境还是端口问题；也可以在插件中心关闭安装预检。"
            .into();
        precheck_stage!(
            on_progress,
            &mut recorder,
            run::stage::BASELINE,
            run::status::FAILURE,
            &format!("环境基线未能启动：{}", baseline.detail)
        );
        // **候选插件不背这口锅**：基线就没起来时，任何归因到插件的结论都
        // 没有事实基础。判 inconclusive 而不是 fail 是这一层存在的意义。
        let _ = recorder.finish(
            run::status::INCONCLUSIVE,
            run::cause::ENVIRONMENT,
            &format!("环境基线没能起来，无法判断候选插件：{}", baseline.detail),
            None,
        );
        report.run_id = recorder.id().to_string();
        report.verified_at_ms = crate::shell::process::epoch_millis();
        // 同样**不装**（两阶段契约）：基线都没起来时这个候选包根本没被测过，
        // 装上去等于把「没验过」说成「验过没问题」。
        fill_source_info(&mut report, spec_str, mode, family, target_instance);
        report.duration_ms = started.elapsed().as_millis() as u64;
        return Ok(report);
    }

    precheck_stage!(
        on_progress,
        &mut recorder,
        run::stage::BASELINE,
        run::status::SUCCESS,
        &format!("环境基线正常：{}", baseline.detail)
    );
    precheck_stage!(
        on_progress,
        &mut recorder,
        run::stage::INSTALL_CANDIDATE,
        run::status::RUNNING,
        "正在把候选插件装入沙盒"
    );
    // 取源 + 装进沙盒：走的就是生产安装路径本身，只是目标实例换成了沙盒。
    let snapshot = StoreSnapshot::capture(data_dir);
    let item = match plugins::center::install_for_instance(
        family,
        sandbox.instance_id(),
        data_dir,
        settings,
        pnpm_exe,
        spec_str,
        mode,
        on_progress,
    ) {
        Ok(item) => item,
        Err(error) => {
            // `install_for_instance` 在写完 store 行之后的任何一步失败都会留下
            // 一个已记账的插件。预检必须把它撤掉，否则「预检失败」反而让面板
            // 多出一行用户没要求的东西。
            snapshot.rollback(data_dir);
            precheck_stage!(
                on_progress,
                &mut recorder,
                run::stage::INSTALL_CANDIDATE,
                run::status::FAILURE,
                &format!("候选插件未能装入沙盒：{error}")
            );
            let _ = recorder.finish(
                run::status::FAILURE,
                run::cause::UNKNOWN,
                &format!("候选插件安装失败：{error}"),
                None,
            );
            return Err(error);
        }
    };

    precheck_stage!(
        on_progress,
        &mut recorder,
        run::stage::INSTALL_CANDIDATE,
        run::status::SUCCESS,
        &format!("候选插件 {} 已装入沙盒", item.name)
    );
    on_progress(&format!("正在验证插件 {} 能否带起内核", item.name));
    precheck_stage!(
        on_progress,
        &mut recorder,
        run::stage::PROBE_CANDIDATE,
        run::status::RUNNING,
        &format!("正在启动沙盒内核并探测 {}", item.name)
    );
    let candidate = probe_boot(&mut sandbox, &install_root, node_path, on_progress);

    let mut report = sandbox::PrecheckReport::new(&item.id, &item.name, Verdict::Pass);
    // 正常路径也要填来源：它只走到最后一步就 commit，漏填的话用户在诊断页
    // 看到的仍是「来源未知」——而那正是最需要说清来源的一条路径。
    fill_source_info(&mut report, spec_str, mode, family, target_instance);
    report.warnings = sandbox::scan_log_markers(&candidate.log);
    report.duration_ms = started.elapsed().as_millis() as u64;

    if !candidate.ready {
        // 基线正常、装了候选包就挂 —— 这才是可归因的失败。撤掉中央库。
        snapshot.rollback(data_dir);
        report.verdict = Verdict::Fail.as_str().to_string();
        report.summary = format!(
            "预检未通过：装上 {} 之后内核起不来（{}）。已撤销本次安装，你的环境没有被改动",
            item.name, candidate.detail
        );
        report.evidence = crate::diagnostics::run::sanitize(&candidate.log);
        if let Some(path) = sandbox::preserve_evidence(data_dir, &sandbox, &item.id) {
            report.evidence_path = path.to_string_lossy().into_owned();
        }
        report.hint = "这说明该插件与当前内核版本不兼容。可以换一个版本重试，或到插件中心向作者反馈；确认无问题也可以直接关闭预检后安装。"
            .into();
        precheck_stage!(
            on_progress,
            &mut recorder,
            run::stage::PROBE_CANDIDATE,
            run::status::FAILURE,
            &format!("装上 {} 之后内核起不来：{}", item.name, candidate.detail)
        );
        precheck_stage!(
            on_progress,
            &mut recorder,
            run::stage::REPORT,
            run::status::RUNNING,
            "正在生成预检报告"
        );
        let evidence = run::RunEvidence {
            kernel_log: None,
            incident_id: None,
            sandbox_log: report.evidence_path.clone().into(),
        };
        let _ = recorder.finish(
            run::status::FAILURE,
            run::cause::PLUGIN,
            &report.summary,
            Some(&evidence),
        );
        report.run_id = recorder.id().to_string();
        report.verified_at_ms = crate::shell::process::epoch_millis();
        return Ok(report);
    }

    precheck_stage!(
        on_progress,
        &mut recorder,
        run::stage::PROBE_CANDIDATE,
        run::status::SUCCESS,
        &format!("候选插件通过启动验证：{}", candidate.detail)
    );
    // **到此为止，什么都没装**（两阶段契约，2026-10-06 用户拍板）：预检只负责
    // 取证，改接线是用户点「应用变更」之后的事。真实实例的 extensions/ 与
    // wiring.json 在这一刻仍然与点「安装」之前逐字节相同。
    on_progress("预检通过，等待你确认是否安装");
    report.summary = format!(
        "预检通过：{} 已在沙盒实例中成功启动内核。**尚未安装**到当前实例。",
        item.name
    );
    if report.warnings.is_empty() {
        report.hint = "点「应用变更」把它装到当前实例；应用前会自动打一份可回退的快照。预检只覆盖启动阶段（进程存活、端口监听、HTTP 应答与启动日志），工作台页面加载后的运行时异常仍由工作台窗口的健康自检负责。".into();
    } else {
        report.hint = "预检通过，但启动日志里出现了可疑标记（见上）。你可以选择不装；要装的话点「应用变更」，应用后第一次打开工作台请留意是否白屏或报错。".into();
    }
    precheck_stage!(
        on_progress,
        &mut recorder,
        run::stage::REPORT,
        run::status::SUCCESS,
        "预检通过，等待确认应用"
    );
    // 「通过但有告警」必须记成 warning 而不是 success：诊断页据此显示
    // 不同标题，压成普通通过就等于把告警藏起来了。
    let final_status = if report.warnings.is_empty() {
        run::status::SUCCESS
    } else {
        run::status::WARNING
    };
    let _ = recorder.finish(final_status, run::cause::UNKNOWN, &report.summary, None);
    report.run_id = recorder.id().to_string();
    report.verified_at_ms = crate::shell::process::epoch_millis();
    Ok(report)
}

/// 用户确认后把插件应用到目标实例（两阶段契约的第二阶段）。
///
/// **走的就是生产安装路径本身**（`install_for_instance`），不另开一条——
/// 预检那套沙盒事务验证的正是这条路径的等价物，应用时换成它才是「验过的
/// 就是要跑的」。应用前打一份 pre-change 快照，用户装完发现不对能退回去。
///
/// 沙盒的验证**不重跑**：重跑一次要几十秒，而用户已经看过报告并确认过了。
/// 代价要说清：预检与应用之间中央库可能已经变了（`pin` 为空时可能解析到
/// 新版本），所以报告里会带上「未重新验证」这句，由界面显示而不是藏起来。
#[allow(clippy::too_many_arguments)]
pub fn plugin_apply(
    family: &str,
    target_instance: &str,
    data_dir: &Path,
    settings: &settings::Settings,
    pnpm_exe: &Path,
    spec_str: &str,
    mode: &str,
    verified_at_ms: u64,
    on_progress: &mut dyn FnMut(&str),
) -> Result<sandbox::PrecheckReport, AppError> {
    let _store_guard = plugins::center::lock_store();
    // 守卫与预检同一条：目标实例的内核还在跑时改接线，等于在用户正工作的
    // 时候动它的依赖。**这一步不能只在 UI 置灰**。
    if let Some(record) = crate::shell::instance::instance_kernel_running(family, target_instance) {
        return Err(AppError::Io(
            crate::shell::instance::instance_kernel_running_message(
                &record,
                target_instance,
                "安装插件",
            ),
        ));
    }

    // 快照在**任何写入之前**（设计 §6.6）。打不出来不阻断应用：用户等的是
    // 「装上」，而快照是兜底不是前提——挡住了就是用一个可选的保险换一个
    // 硬失败。但 id 要交回报告，让界面能显示「没有回退点」而不是假装有。
    let mut snapshot_id = String::new();
    match crate::diagnostics::snapshot::record(
        data_dir,
        family,
        target_instance,
        &settings.profile,
        settings.port,
        crate::diagnostics::snapshot::reason::PRE_CHANGE,
    ) {
        Ok(Some(snapshot)) => {
            snapshot_id = snapshot.id;
            on_progress(&format!("已保存变更前状态（回退点 {snapshot_id}）"));
        }
        Ok(None) => {}
        Err(error) => {
            eprintln!("dsh-xlink: 应用前未能打 pre-change 快照：{error}");
            on_progress(&format!(
                "注意：未能保存变更前快照（{error}），装完无法一键退回"
            ));
        }
    }

    on_progress("正在安装到当前实例");
    let item = plugins::center::install_for_instance(
        family,
        target_instance,
        data_dir,
        settings,
        pnpm_exe,
        spec_str,
        mode,
        on_progress,
    )?;

    let mut report = sandbox::PrecheckReport::new(&item.id, &item.name, sandbox::Verdict::Pass);
    report.installed = true;
    report.pre_change_snapshot_id = snapshot_id;
    fill_source_info(&mut report, spec_str, mode, family, target_instance);
    report.summary = if report.pre_change_snapshot_id.is_empty() {
        format!(
            "已安装 {} 到当前实例（本次没有可回退的变更前快照）",
            item.name
        )
    } else {
        format!(
            "已安装 {} 到当前实例。变更前的状态存为回退点 {}，结果不对可以退回去。",
            item.name, report.pre_change_snapshot_id
        )
    };
    // 这句必须出现在界面上：沙盒验证发生在 `verified_at_ms` 那一刻，用户点
    // 应用时它可能已经是几分钟前，而版本钉为空时解析到的也可能是另一个版本。
    report.hint = if verified_at_ms > 0 {
        let ago_secs = crate::shell::process::epoch_millis().saturating_sub(verified_at_ms) / 1000;
        format!(
            "本次安装**没有重新做沙盒验证**，直接沿用 {ago_secs} 秒前那次预检的结论。若插件版本在此期间发生变化，预检结论不适用于当前安装的版本。"
        )
    } else {
        "本次安装没有重新做沙盒验证。".to_string()
    };
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::scoped_xlink_home;
    use std::fs;

    /// 来源信息必须在**每一条**返回路径上填满。
    ///
    /// 漏填的那条在 UI 上表现为「来源未知」——而漏掉的恰恰可能是
    /// fail-open 那条：用户是在「预检没做」的情况下把一个包装进实例的，
    /// 那时他最需要知道装的是哪个地址的什么版本。
    #[test]
    fn source_info_is_filled_for_every_npm_spec() {
        let home = std::env::temp_dir().join(format!(
            "dsh-src-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _guard = scoped_xlink_home(&home);
        fs::create_dir_all(&home).expect("home");

        let mut report = sandbox::PrecheckReport::new("id", "name", sandbox::Verdict::Inconclusive);
        fill_source_info(
            &mut report,
            "npm install @scope/pkg@1.2.3",
            "link",
            "dsh",
            "default",
        );
        assert_eq!(report.source_kind, "npm", "取源类型要能区分 npm / github");
        assert_eq!(report.source_label, "@scope/pkg");
        assert_eq!(
            report.pin, "1.2.3",
            "版本钉必须带出来：同一个包名不同版本是两份东西"
        );
        assert_eq!(report.materialize, "link");
        assert_eq!(report.target_instance, "default");
        // 解析失败不编造来源：宁可显示「来源未知」，也不能让用户以为装的是
        // 官方包。
        let mut bad = sandbox::PrecheckReport::new("id", "name", sandbox::Verdict::Fail);
        fill_source_info(
            &mut bad,
            "这不是一个合法的安装请求",
            "copy",
            "dsh",
            "default",
        );
        assert_eq!(bad.source_label, "", "解析失败时来源必须留空而不是编一个");

        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn integrity_label_never_calls_an_unverified_download_verified() {
        use crate::plugins::sandbox::integrity;
        assert!(!integrity::label(integrity::SHA1).contains("较强"));
        assert!(integrity::label(integrity::NONE).contains("未能"));
        assert!(integrity::label(integrity::SHA512).contains("sha512"));
    }

    /// 预检的**全部**安全承诺就是这一条：判失败之后，中央库要回到用户点
    /// 「安装」之前的样子——不多一条 store 行、不多一个插件目录、字节级
    /// 还原。少任何一半，用户都会在面板里看到一个自己没要求、还装坏了的
    /// 插件。
    #[test]
    fn rollback_restores_store_bytes_and_removes_only_new_entries() {
        let home = std::env::temp_dir().join(format!("dsh-precheck-{}", std::process::id()));
        fs::create_dir_all(&home).expect("home");
        let _guard = scoped_xlink_home(&home);
        let data_dir = home.join("desktop");
        let store = plugins::center::store_dir(&data_dir);
        fs::create_dir_all(&store).unwrap();

        // 预检前：已有一个用户自己装的插件，store.json 里有它。
        fs::create_dir_all(store.join("keeper")).unwrap();
        fs::write(store.join("keeper/keep.txt"), "keep").unwrap();
        let before = serde_json::json!({
            "schemaVersion": 1,
            "items": [{ "id": "keeper", "name": "keeper" }],
        });
        let original = serde_json::to_string_pretty(&before).unwrap();
        fs::write(plugins::center::store_file(&data_dir), &original).unwrap();

        let snapshot = StoreSnapshot::capture(&data_dir);

        // 预检装入了一个新包。
        fs::create_dir_all(store.join("candidate")).unwrap();
        fs::write(store.join("candidate/bundle.js"), "boom").unwrap();
        let after = serde_json::json!({
            "schemaVersion": 1,
            "items": [{ "id": "keeper" }, { "id": "candidate" }],
        });
        fs::write(
            plugins::center::store_file(&data_dir),
            serde_json::to_string_pretty(&after).unwrap(),
        )
        .unwrap();

        snapshot.rollback(&data_dir);

        assert_eq!(
            fs::read_to_string(plugins::center::store_file(&data_dir)).unwrap(),
            original,
            "store.json 必须按字节还原"
        );
        assert!(
            !store.join("candidate").exists(),
            "预检装入的插件目录必须被删掉"
        );
        assert!(
            store.join("keeper").is_dir(),
            "用户预检前就装好的插件绝不能被一次失败的预检带走"
        );
        assert!(store.join("keeper/keep.txt").is_file());

        let _ = fs::remove_dir_all(&home);
    }

    /// 首次安装（预检前根本没有 store.json）失败回滚后，不能凭空留下一个
    /// 空清单文件——那会让「装过 / 没装过」在面板上说不清。
    #[test]
    fn rollback_deletes_store_file_when_it_did_not_exist() {
        let home = std::env::temp_dir().join(format!("dsh-precheck-fresh-{}", std::process::id()));
        fs::create_dir_all(&home).expect("home");
        let _guard = scoped_xlink_home(&home);
        let data_dir = home.join("desktop");
        fs::create_dir_all(plugins::center::store_dir(&data_dir)).unwrap();

        let snapshot = StoreSnapshot::capture(&data_dir);
        assert!(snapshot.bytes.is_none());

        fs::write(plugins::center::store_file(&data_dir), "{}").unwrap();
        snapshot.rollback(&data_dir);

        assert!(
            !plugins::center::store_file(&data_dir).exists(),
            "预检前不存在的 store.json 不该被回滚凭空造出来"
        );
        let _ = fs::remove_dir_all(&home);
    }

    /// 启动时的残留回收只认 `sbx-` 前缀：真实实例目录哪怕同名巧合也
    /// 不能被误删。
    #[test]
    fn sweep_removes_only_sandbox_prefixed_instance_dirs() {
        let home = std::env::temp_dir().join(format!("dsh-sweep-{}", std::process::id()));
        let _guard = scoped_xlink_home(&home);
        let family = crate::shell::instance::KERNEL_FAMILY_DSH;
        let root = crate::shell::paths::kernel_instances_dir(family);
        fs::create_dir_all(root.join("sbx-abc-1")).unwrap();
        fs::create_dir_all(root.join("default")).unwrap();
        fs::write(root.join("default/keep.txt"), "x").unwrap();

        let removed = sandbox::sweep_stale(family);

        assert_eq!(removed, 1, "只应回收 1 个沙盒目录");
        assert!(!root.join("sbx-abc-1").exists());
        assert!(root.join("default").is_dir(), "真实实例目录必须原样保留");
        let _ = fs::remove_dir_all(&home);
    }

    /// Rust 与 `PrecheckDialog.vue` 靠这三个字符串对齐判定。改一边不改
    /// 另一边，后果是「预检失败」被画成「预检通过」——所以在这里钉死。
    #[test]
    fn verdict_strings_match_the_ui_contract() {
        use crate::plugins::sandbox::Verdict;
        assert_eq!(Verdict::Pass.as_str(), "pass");
        assert_eq!(Verdict::Fail.as_str(), "fail");
        assert_eq!(Verdict::Inconclusive.as_str(), "inconclusive");
    }

    /// 预检默认开启：省掉的是十几秒，保住的是用户整个工作环境。
    #[test]
    fn precheck_defaults_to_enabled() {
        let on = settings::Settings::default();
        assert!(settings::plugin_precheck_enabled(&on));
        let off = settings::Settings {
            plugin_precheck: Some(false),
            ..settings::Settings::default()
        };
        assert!(!settings::plugin_precheck_enabled(&off));
    }
}
