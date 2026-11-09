//! 记忆路由：检索、按 id 读、候选提交、注入回报。
//!
//! **提取任务（P2）的那七条路由在 `jobs_routes.rs`**（2026-10-15 再拆一层）：这一份管
//! 「记忆本身的调用」，那一份管「一次提取从排队到收尾」。
//!
//! 从 `plugins/builtin/bridge.rs` 拆出来（2026-10-15）：那一份连着两轮被这一族路由顶过预算线
//! ——上一轮上调时写的注释里就留了条件「下次再长就该拆」，这一轮长到 487 行（预算 420），
//! 按规则拆而不是再调数字。拆完桥那一层只留三件事：分发、内置插件同步、桥的生命周期。
//!
//! 三条纪律与桥那一侧逐条对齐：
//! · **GET + 查询串，不读请求体**（`openai::http::read_request` 只保证读到请求头，body 可能
//!   在同一次 read 里到达也可能没有）；
//! · **谁能调、最坏能造成什么**写清（见各函数文档）——调用方是模型能触达的代码；
//! · **这些路由都不写权威记录**：候选只能进待确认，用户点了保存才成记忆。

use std::net::TcpStream;

use crate::plugins::builtin::bridge::query_decoded;

/// 按行读一个查询参数（一行一条），与面板编辑器同一口径（`memory.js` 按 `\n` 拆）。
fn query_lines(path: &str, key: &str) -> Vec<String> {
    query_decoded(path, key)
        .split('\n')
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

/// 记忆命名空间：内核族与实例取自这条桥的绑定（`family:instance`），profile 取自壳
/// 设置——与面板（`Namespace::current`）同一份来源。**不信调用方传来的坐标**：
/// 桥知道自己在替哪个实例服务，插件只需要说「查什么、写什么」。
pub(crate) fn memory_namespace(scope_key: &str) -> crate::memory::store::Namespace {
    let (family, instance) = scope_key.split_once(':').unwrap_or((scope_key, ""));
    crate::memory::store::Namespace::for_bridge(family, instance)
}

/// 记忆路由的调用留痕。
///
/// **为什么留痕在壳这一侧**：2026-10-14 实测，插件里的 `ctx.logger.info/warn` 在这个
/// 内核里一行都不进 stdout（同一个进程里 `dsh:` 自己的告警照常打印），所以「记忆到底
/// 有没有被调用过」在插件那边答不了。壳这边写一行 `shell_events` 就同时出现在
/// 「查看日志」面板里——**只记路由与计数，不记记忆正文**（正文是用户私有内容）。
fn note_memory_call(route: &str, detail: &str) {
    // 顺带记「插件刚调用过壳」——面板靠它判「开关的改动认了没有」（E6）。
    crate::memory::recent::mark_plugin_call();
    crate::shell::shell_events::record("memory-bridge", &format!("{route} {detail}"));
}

fn automatic_module_request(path: &str) -> bool {
    query_decoded(path, "module_context") == "1"
        && query_decoded(path, "pending") != "1"
        && query_decoded(path, "context_only") != "1"
}

pub(crate) fn context(stream: &mut TcpStream, path: &str, scope_key: &str) {
    // 与 sync 同一条纪律：**GET + 查询串，不读请求体**（见下面 sync 那条注释）。
    // 中文与空格由插件百分号编码，这里解码后再进检索。
    // 与模块授权、源记录修改串行到响应写出，关闭后不能发出旧快照。
    let _module_lock = match crate::memory::share::lock_grants() {
        Ok(lock) => lock,
        Err(_) => {
            let _ = crate::openai::http::write_response(
                stream,
                503,
                "application/json",
                r#"{"error":"authorization unavailable"}"#,
            );
            return;
        }
    };
    let namespace = memory_namespace(scope_key);
    let query = query_decoded(path, "q");
    let workspace = query_decoded(path, "workspace");
    // `workspace_id` 是插件经工作区注册表解析出来的内核 id；解析不到就只带路径，
    // 后端按路径兜底匹配（见 `query::scope_matches`）。三条路由统一叫
    // `workspace_id`——`read` 那条的 `id` 是**记忆 id**，两处同名必然记混。
    let workspace_id = query_decoded(path, "workspace_id");
    // 每内核开关关闭后，引用与候选提取都停止；`pending=1` 也不能绕过。
    // 显式读取和候选清单读取另有相同门禁，旧的 use_approved 不能恢复关闭状态。
    let target_blocked = crate::memory::targets::blocks_context(
        &namespace.family,
        query_decoded(path, "pending") == "1",
    );
    // 只读工具可以缩小到扩展资料；权限判据相同，且不会把记忆的固定项挤进这份预算。
    let retrieved = if target_blocked {
        Ok(crate::memory::query::RetrievedContext::default())
    } else if query_decoded(path, "context_only") == "1" {
        crate::memory::query::retrieve_artifacts(&namespace, &query, &workspace_id, &workspace)
    } else if automatic_module_request(path) {
        crate::memory::module::retrieve(&namespace, &query, &workspace_id, &workspace).and_then(
            |shared| match shared {
                Some(context) => Ok(context),
                None => {
                    crate::memory::query::retrieve(&namespace, &query, &workspace_id, &workspace)
                }
            },
        )
    } else {
        crate::memory::query::retrieve(&namespace, &query, &workspace_id, &workspace)
    };
    match retrieved {
        Ok(mut context) => {
            // **只有提取那一条路要待确认候选**（C6）：插件带 `pending=1` 来要它，比的是
            // 「用户这次说的是不是在推翻先前那条」。运行时注入不带这个参数，所以响应里的
            // `text` 与 `hits` 一个字节都不变——未确认的东西不许影响模型看到什么。
            if query_decoded(path, "pending") == "1" {
                context.pending = crate::memory::query::recent_pending(
                    &namespace,
                    crate::memory::query::MAX_PENDING_FOR_EXTRACT,
                )
                .unwrap_or_else(|error| {
                    // 读不出待确认清单**不许拖垮整次检索**：提取链会降级成「没有清单可比对」，
                    // 而那正是插件侧「没有校验集合时标记一律丢弃」的口径（提取本身照跑）。
                    // 但不许静默：失败原因要进 `shell_events`，否则症状是「模型最近从不标取代」
                    // 而面板与日志一个字都没有。
                    note_memory_call("context", &format!("pending 清单读取失败：{error}"));
                    Vec::new()
                });
            }
            note_memory_call(
                "context",
                &format!(
                    "enabled={} hits={} pending={}",
                    context.enabled,
                    context.hits.len(),
                    context.pending.len()
                ),
            );
            let body = serde_json::to_string(&context)
                .unwrap_or_else(|_| r#"{"enabled":false}"#.to_string());
            let _ = crate::openai::http::write_response(stream, 200, "application/json", &body);
        }
        Err(error) => {
            let body = serde_json::json!({ "error": error.to_string() }).to_string();
            let _ = crate::openai::http::write_response(stream, 500, "application/json", &body);
        }
    }
}

/// `/v1/memory/note` 的查询参数（P1 注入账本）。
///
/// **全部可选**：旧插件只带 `session/workspace/ids/enabled`，缺的参数走默认值——账本里
/// 那条记录仍按「一次注入」记下（kind=inject、reason=turn、turn/step=-1、bytes=0、
/// plugin_version=unknown），旧插件不需要为账本改一行。
struct NoteParams {
    session: String,
    workspace: String,
    workspace_id: String,
    enabled: bool,
    ids: String,
    kind: String,
    reason: String,
    turn: i64,
    step: i64,
    bytes: u64,
    generation: u64,
    plugin_version: String,
    kernel_version: String,
    /// P2 对照实验分组："control" / "treatment" / ""。
    experiment: String,
}

/// 解析 note 的查询串。**纯函数**：不读库、不读路径，所以路由层的兼容性可以直接测。
fn parse_note_params(path: &str) -> NoteParams {
    // P2：kind 允许 experiment（会话分组事件）；未知值仍按 inject（旧插件只带 kind=inject）。
    let kind = match query_decoded(path, "kind").as_str() {
        crate::memory::effect::ledger::KIND_SKIP => {
            crate::memory::effect::ledger::KIND_SKIP.to_string()
        }
        crate::memory::effect::ledger::KIND_EXPERIMENT => {
            crate::memory::effect::ledger::KIND_EXPERIMENT.to_string()
        }
        _ => crate::memory::effect::ledger::KIND_INJECT.to_string(),
    };
    let reason = {
        let raw = query_decoded(path, "reason");
        if !raw.is_empty() {
            raw
        } else if kind == crate::memory::effect::ledger::KIND_INJECT {
            // 旧插件不带 reason：它唯一会走到这里的路径就是「回合预算内命中」。
            "turn".to_string()
        } else {
            String::new()
        }
    };
    let plugin_version = {
        let raw = query_decoded(path, "plugin_version");
        if raw.is_empty() {
            "unknown".to_string()
        } else {
            raw
        }
    };
    NoteParams {
        session: query_decoded(path, "session"),
        workspace: query_decoded(path, "workspace"),
        workspace_id: query_decoded(path, "workspace_id"),
        enabled: query_decoded(path, "enabled") != "0",
        ids: query_decoded(path, "ids"),
        kind,
        reason,
        // 解析失败按「不知道」：turn/step 的缺省 -1 是合同里写明的（旧插件与 step 都一样）。
        turn: query_decoded(path, "turn").parse().unwrap_or(-1),
        step: query_decoded(path, "step").parse().unwrap_or(-1),
        bytes: query_decoded(path, "bytes").parse().unwrap_or(0),
        generation: query_decoded(path, "generation").parse().unwrap_or(0),
        plugin_version,
        kernel_version: query_decoded(path, "kernel_version"),
        experiment: query_decoded(path, "experiment"),
    }
}

/// 壳侧补内核版本：桥的 `scope_key` 就是 `family:instance`（桥知道自己在替哪个实例服务）。
///
/// 插件传的 `kernel_version` 只是兜底：能查到实例记录就以壳为准——插件看到的是自己
/// 进程里的版本，而实例记录才是「这个内核是谁装的」。取不到（无记录、坐标不合法）保留
/// 插件给的值（旧插件为空串），**绝不因为查版本失败而影响 note**。
fn resolve_kernel_version(scope_key: &str, fallback: &str) -> String {
    let (family, instance) = scope_key.split_once(':').unwrap_or((scope_key, ""));
    if family.is_empty()
        || instance.is_empty()
        || crate::shell::paths::validate_id_component(family).is_err()
        || crate::shell::paths::validate_id_component(instance).is_err()
    {
        return fallback.to_string();
    }
    match crate::shell::instance::load_record_from_disk(family, instance)
        .and_then(|record| record.kernel_version)
        .filter(|version| !version.is_empty())
    {
        Some(version) => version,
        None => fallback.to_string(),
    }
}

/// 把解析后的参数收敛成一条账本行。
///
/// **skip 的形状由壳兜底**（ids 空、bytes 0、不计数）：插件少带/带错不该让账本里出现
/// 「跳过了 758 字节」这种自相矛盾的行。`at_ms` 取壳的时钟——插件的时钟不参与任何决策。
fn ledger_note(
    params: &NoteParams,
    scope_key: &str,
    now_ms: u64,
) -> crate::memory::effect::ledger::LedgerNote {
    let inject = params.kind == crate::memory::effect::ledger::KIND_INJECT;
    crate::memory::effect::ledger::LedgerNote {
        at_ms: now_ms,
        session: params.session.clone(),
        turn: params.turn,
        step: params.step,
        kind: params.kind.clone(),
        reason: params.reason.clone(),
        enabled: params.enabled,
        ids: if inject {
            crate::memory::effect::ledger::normalize_ids(&params.ids)
        } else {
            Vec::new()
        },
        bytes: if inject { params.bytes } else { 0 },
        generation: params.generation,
        workspace_id: params.workspace_id.clone(),
        plugin_version: params.plugin_version.clone(),
        kernel_version: resolve_kernel_version(scope_key, &params.kernel_version),
        experiment: params.experiment.clone(),
    }
}

pub(crate) fn note(stream: &mut TcpStream, path: &str, scope_key: &str) {
    // 插件在**真的用了**这次检索之后回来说一声（含「开关关着/一条没命中」——
    // 那也是模型实际看到的状态）。谁能调：只有拿到 Bearer 的插件进程；最坏能造成
    // 什么：往一个进程内、上限 20 条的环形里塞一条标题列表，不碰权威记录。
    //
    // `session` 只用于展示与排查（面板按会话分组），**不参与任何授权判断**：
    // 命名空间仍由桥的绑定决定，调用方传什么都不影响。
    let params = parse_note_params(path);
    let namespace = memory_namespace(scope_key);

    // ① 先写注入账本（inject 与 skip 都写）：它是「这一回合为什么有/没有记忆」的唯一
    //    事实源。**写失败不影响响应**——`ledger::append` 内部只落一行壳日志，账本是观测
    //    数据，不是权威记录（权威记录是 memory.json）。
    let ledger_dir =
        crate::memory::effect::ledger::dir_under(&crate::shell::paths::instance_memory_effect_dir(
            &namespace.family,
            &namespace.instance_id,
            &namespace.profile,
        ));
    let entry = ledger_note(&params, scope_key, crate::shell::process::epoch_millis());
    crate::memory::effect::ledger::append(&ledger_dir, &entry);

    // ② 旧行为只留给 inject：skip 不写「最近注入」环形、也不计 recall——否则每回合一次的
    //    跳过事件会把上限 20 条的环形挤爆，而那张卡片的语义是「模型实际看到了什么」。
    //
    //    环形对 inject **不看 enabled**（旧插件在开关关着时也回报一次，面板靠它显示
    //    「开关已关闭」状态）；recall 计数仍然只在 enabled=1 时记。
    if entry.is_inject() {
        crate::memory::recent::record(
            &namespace,
            &params.session,
            &params.workspace,
            params.enabled,
            &params.ids,
        );
        // 每条记录上的「被注入 N 次」（`store::recall_count`）。**只在真的注入了才记**：
        // `enabled=0` 是「这一回合开关关着」，不是一次注入。
        //
        // **失败只留痕、不影响响应**：注入已经发生了，计数只是面板上的可见性信号；让插件拿到
        // 一个失败会让它在下一步重试，而它重试也不会让计数更准。写不进去的原因（另一个壳占着
        // 锁、库损坏）由「查看日志」的 memory-bridge 标签给出。
        if params.enabled {
            let id_list: Vec<&str> = params
                .ids
                .split(',')
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .collect();
            // 模块互通时命中 id 是 `namespace/id` 全限定形态（本侧也加前缀）：
            // 按来源分库记账，否则这些命中永远记不到任何记录上。
            if let Err(error) = crate::memory::module::note_recall_for_hits(&namespace, &id_list) {
                note_memory_call("note", &format!("计数没写进去：{error}"));
            }
        }
    }
    let hits = params.ids.split(',').filter(|id| !id.is_empty()).count();
    note_memory_call(
        "note",
        &format!(
            "kind={} reason={} enabled={} hits={}",
            entry.kind, entry.reason, params.enabled, hits
        ),
    );
    let _ = crate::openai::http::write_response(stream, 200, "application/json", r#"{"ok":true}"#);
}

/// `/v1/memory/experiment?session=<id>`：P2 对照实验的**只读**分组查询。
///
/// 谁能调：只有拿到 Bearer 的插件进程；最坏能造成什么：什么也不写、不返回任何记忆内容。
/// **每次调用都不算一次注入**（不进 recent、不计 recall、不写账本）——它只是问一句
/// 「这个会话在实验里属于哪组」。插件每会话问一次、control 会话最多每 3 turn 再确认一次
/// 是否已停止，所以这里的成本是一次 JSON 序列化。
///
/// 未配置/已停止/已到期一律回 configured=false、active=false、cohort=""：
/// 插件据此走正常流程，而不是把「实验没开」误当成 treatment。
pub(crate) fn experiment(stream: &mut TcpStream, path: &str, scope_key: &str) {
    let namespace = memory_namespace(scope_key);
    let session = query_decoded(path, "session");
    let config_path = crate::shell::paths::instance_memory_effect_experiment_file(
        &namespace.family,
        &namespace.instance_id,
        &namespace.profile,
    );
    let config = crate::memory::effect::experiment::load(&config_path);
    let now_ms = crate::shell::process::epoch_millis();
    // **会话创建时刻**（毫秒）：实验只收「实验开始后新建」的会话，判据在 experiment.rs。
    // 解析失败按「拿不到」→ 不发分组（保守，见 bridge_response 的注释）。
    let created_at_ms = query_decoded(path, "created_at_ms").parse().unwrap_or(0);
    // **会话来源**（startup / resume / clear / compact，见内核的 SessionStartSource）：
    // 「插件到底读没读到会话创建时刻」靠它分得开，判别逻辑在 `participation_line`。
    let source = query_decoded(path, "source");
    let body = crate::memory::effect::experiment::bridge_response(
        &config,
        now_ms,
        &session,
        created_at_ms,
    );
    let cohort = body
        .get("cohort")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    if let Some(line) = crate::memory::effect::experiment::participation_line(
        &config,
        now_ms,
        &session,
        created_at_ms,
        &source,
        cohort,
    ) {
        note_memory_call("experiment", &line);
    }
    let _ = crate::openai::http::write_response(stream, 200, "application/json", &body.to_string());
}

/// `/v1/memory/effect?range_days=7`：P2 只读效果快照，给 agent 直接读报告摘要。
///
/// 谁能调：只有拿到 Bearer 的插件进程；最坏能造成什么：重写本实例 `effect/` 下的
/// 派生快照（report.json / report.md / history/），**不碰权威记忆库**。返回的摘要
/// **不含记忆标题**（榜单只给 id）：它会随 `memory_effect` 工具进入模型上下文，而实例级
/// 榜单可能含别的项目的标题；标题只写在本机 report.md。与面板的 `memory_effect_report`
/// 走同一条 `readout_for` 写快照 + 归档 + 裁剪路径（进程内单飞锁串行），两边口径一致。
///
/// 时间窗只收 7/30/90 三档：`range_days` 缺失或非法归一到**最近**的一档（如 45→30），
/// 与面板同一份归一逻辑（`effect::normalize_range_days`）。
pub(crate) fn effect(stream: &mut TcpStream, path: &str, scope_key: &str) {
    let namespace = memory_namespace(scope_key);
    let requested = query_decoded(path, "range_days").parse::<u32>().ok();
    let range = crate::memory::effect::normalize_range_days(requested);
    match crate::memory::effect_export::readout_for(&namespace, Some(range)) {
        Ok(readout) => {
            // 只记时间窗与历史份数，**不记摘要正文**（正文含榜单标题，是用户私有内容）。
            note_memory_call(
                "effect",
                &format!(
                    "range={} history={}",
                    readout.range_days, readout.history_files
                ),
            );
            let body = serde_json::to_string(&readout)
                .unwrap_or_else(|_| r#"{"error":"serialize"}"#.to_string());
            let _ = crate::openai::http::write_response(stream, 200, "application/json", &body);
        }
        Err(error) => {
            note_memory_call("effect", &format!("range={range} failed"));
            let body = serde_json::json!({ "error": error.to_string() }).to_string();
            let _ = crate::openai::http::write_response(stream, 500, "application/json", &body);
        }
    }
}

pub(crate) fn read(stream: &mut TcpStream, path: &str, scope_key: &str) {
    // 授权检查、记录检查、使用计数与发送串行，不能在检查后被并发撤销或改正文。
    let _authorization = match crate::memory::share::lock_grants() {
        Ok(lock) => lock,
        Err(_) => {
            let _ = crate::openai::http::write_response(
                stream,
                503,
                "application/json",
                r#"{"error":"authorization unavailable"}"#,
            );
            return;
        }
    };
    let namespace = memory_namespace(scope_key);
    let pool = match crate::memory::targets::pool_enabled_for(&namespace) {
        Ok(pool) => pool,
        Err(_) => {
            let _ = crate::openai::http::write_response(
                stream,
                404,
                "application/json",
                r#"{"error":"记忆授权不可用"}"#,
            );
            return;
        }
    };
    // 接收方开关关闭时按 id 读也拒绝：id 会从历史注入里留下来，只挡检索不挡
    // 按 id 读，关闭这个目标就停不住模型继续使用记忆。返回 404 而不是空记录，
    // 不区分「没有这条」与「不许读」，也不透露 id 是否存在。
    if pool == Some(false) || crate::memory::targets::blocked(&namespace.family) {
        note_memory_call("read", "blocked=target-off");
        let _ = crate::openai::http::write_response(
            stream,
            404,
            "application/json",
            r#"{"error":"记忆使用开关已关闭"}"#,
        );
        return;
    }
    let id = query_decoded(path, "id");
    if id.is_empty() {
        let _ = crate::openai::http::write_response(
            stream,
            400,
            "application/json",
            r#"{"error":"missing id"}"#,
        );
        return;
    }
    let workspace = query_decoded(path, "workspace");
    let workspace_id = query_decoded(path, "workspace_id");
    // **三道门放进同一次事务**：项目归属、启用状态、来源许可（旧 use_approved，
    // 或已授权的模块互通）。拆成「先查、再判、最后计数」时，用户在这三拍之间
    // 停用或撤销许可，正文照样读得出去、`use_count` 也照样加一——撤销对显式读取
    // 就不生效了。
    //
    // · 项目：id 猜不到，但会话历史里留着旧 id，不校验就能在 B 项目里把 A 项目
    //   的记忆读出来（设计承诺的是不进入）。通用偏好对谁都算；项目记录必须与
    //   调用方给的坐标对得上。
    // · 状态：只认「已启用」。用户点了「停用」就是不想再被模型用着，而注入块
    //   的表头里有 id（`[id] 标题`），会话历史里因此一直留着它——只挡检索不挡
    //   按 id 读，停用这个按钮对模型就是无效的。
    // · 许可：目标开关只决定接收方能不能用记忆；来源库的 use_approved 被关掉后
    //   显式读取必须一起停，不能靠目标开关显式 true 绕回来。模块互通是另一条
    //   显式许可，`enabled_for` 已经核过 pair 范围。
    let allow = |doc: &crate::memory::store::MemoryDoc,
                 record: &crate::memory::store::MemoryRecord| {
        let granted = match pool {
            Some(enabled) => {
                enabled && record.verification == crate::memory::store::VERIFICATION_VERIFIED
            }
            None => {
                doc.meta.use_approved
                    || crate::memory::module::enabled_for(&namespace).unwrap_or(false)
            }
        };
        granted && crate::memory::query::scope_matches(record, &workspace_id, &workspace)
    };
    match crate::memory::store::read_and_note_use(&namespace, &id, allow) {
        Ok(Some(record)) => {
            note_memory_call(
                "read",
                &format!("id={} revision={}", record.id, record.revision),
            );
            let body = serde_json::to_string(&record).unwrap_or_else(|_| "{}".to_string());
            let _ = crate::openai::http::write_response(stream, 200, "application/json", &body);
        }
        Ok(None) => {
            // 不区分「没有这条」「已停用」「不许读」：不透露 id 是否存在。
            note_memory_call("read", "denied=out-of-scope-disabled-or-ungranted");
            let _ = crate::openai::http::write_response(
                stream,
                404,
                "application/json",
                r#"{"error":"这条记忆不属于当前项目、已被停用，或当前未获准读取"}"#,
            );
        }
        Err(error) => {
            // 文档读不出来时**如实说读不出来**，不退成「不属于当前项目」——
            // 后者会让用户去查自己有没有记错项目 id，而真因在盘上那份文件。
            // 留痕**别记成 `denied=`**：那是授权事件，而这一条是盘上那份文件坏了。
            note_memory_call("read", "failed=unreadable");
            let body = serde_json::json!({ "error": error.to_string() }).to_string();
            let _ = crate::openai::http::write_response(stream, 500, "application/json", &body);
        }
    }
}

pub(crate) fn propose(stream: &mut TcpStream, path: &str, scope_key: &str) {
    // 关闭模块也必须停止候选写入；检查、合并来源与响应持有同一授权锁，
    // 避免用户关闭后，早先通过检查的请求仍写进库。旧模式不在这里增加权限。
    let _authorization = match crate::memory::share::lock_grants() {
        Ok(lock) => lock,
        Err(_) => {
            let _ = crate::openai::http::write_response(
                stream,
                503,
                "application/json",
                r#"{"error":"authorization unavailable"}"#,
            );
            return;
        }
    };
    let namespace = memory_namespace(scope_key);
    match crate::memory::targets::pool_enabled_for(&namespace) {
        Ok(Some(false)) => {
            note_memory_call("propose", "blocked=pool-disabled-or-source-changed");
            let _ = crate::openai::http::write_response(
                stream,
                403,
                "application/json",
                r#"{"error":"记忆模块已关闭或来源已改变，请在记忆页确认后启用"}"#,
            );
            return;
        }
        Err(_) => {
            let _ = crate::openai::http::write_response(
                stream,
                503,
                "application/json",
                r#"{"error":"记忆模块授权不可用"}"#,
            );
            return;
        }
        Ok(Some(true) | None) => {}
    }
    // 候选**只能进待确认**：这条路由调的是 `submit_candidate`，它写不进权威
    // 记录。模型想自己批准候选没有入口——批准只存在于面板的 Tauri 命令面。
    let kind = query_decoded(path, "kind");
    // 空范围按类型给默认：经验候选默认通用（适用面写在情境里），其余默认项目。
    let scope = match query_decoded(path, "scope") {
        scope if !scope.is_empty() => scope,
        _ if kind == "experience" => crate::memory::store::SCOPE_GENERAL.to_string(),
        _ => crate::memory::store::SCOPE_PROJECT.to_string(),
    };
    let input = crate::memory::store::CandidateInput {
        kind,
        experience: crate::memory::experience::ExperienceFields {
            situation: query_decoded(path, "situation"),
            action: query_decoded(path, "action"),
            outcome: query_decoded(path, "outcome"),
            lesson: query_decoded(path, "lesson"),
        },
        // 交接候选（2026-10-24）：done / todo / blocked 按**行**收（一行一条），
        // 与面板编辑器同一口径（memory.js 的 draftPayload 也按 \n 拆）。
        handoff: crate::memory::handoff::HandoffFields {
            goal: query_decoded(path, "goal"),
            done: query_lines(path, "done"),
            todo: query_lines(path, "todo"),
            blocked: query_lines(path, "blocked"),
        },
        // **冲突标记只走提取那条路**（`/jobs/submit`）：C5 的机械判据是「模型标的 id 必须
        // 确实来自壳发给它的那次检索结果」，而这条路由是模型直接调工具——壳这边**没有**那次
        // 检索可对，于是收下的 id 就只能任它凭空指认，正是设计要防的那件事。空列表。
        conflicts_with: Vec::new(),
        // 取代标记同理（C6）：模型直接调工具时壳没有那份「待确认候选清单」可对，
        // 照收 id 就是任它凭空指认某条候选不存在或已被否掉。
        supersedes: Vec::new(),
        scope,
        workspace_id: query_decoded(path, "workspace_id"),
        workspace_path: query_decoded(path, "workspace"),
        title: query_decoded(path, "title"),
        body: query_decoded(path, "body"),
        keywords: Vec::new(),
        source: crate::memory::store::MemorySource {
            kind: crate::memory::store::SOURCE_SESSION.to_string(),
            session: query_decoded(path, "session"),
            at: crate::memory::store::now_iso(),
            ..Default::default()
        },
        verification: String::new(),
    };
    match crate::memory::store::submit_candidate(&namespace, input) {
        // **并入已有记录的来源**不是批准，但也不是拒收：`state` 会告诉模型这条已经在库里
        // （`accepted`）而不是等着他点保存，否则它会换个说法再提一次——而那正是这次合并要
        // 避免的循环。两个结局共用一份回包，省的是「报给模型的东西」只有一份实现。
        Ok(outcome) => {
            let (id, title, state) = match outcome {
                crate::memory::store::SubmitOutcome::Merged(record) => {
                    (record.id, record.title, record.state)
                }
                crate::memory::store::SubmitOutcome::Candidate(candidate) => {
                    (candidate.id, candidate.title, candidate.state)
                }
            };
            note_memory_call("propose", &format!("propose={state} {id}"));
            let body = serde_json::json!({ "id": id, "state": state, "title": title }).to_string();
            let _ = crate::openai::http::write_response(stream, 200, "application/json", &body);
        }
        Err(error) => {
            let body = serde_json::json!({ "error": error.to_string() }).to_string();
            let _ = crate::openai::http::write_response(stream, 400, "application/json", &body);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::effect::ledger::{KIND_INJECT, KIND_SKIP};

    /// 旧插件（P0 形状）只带 session/workspace/ids/enabled：缺省参数必须落成一次
    /// 「回合内注入」，而不是被当成 skip 或解析失败——账本上线不该要求旧插件同步升级。
    #[test]
    fn old_plugin_params_keep_the_p0_meaning() {
        let params =
            parse_note_params("/v1/memory/note?session=s1&workspace=%2Fw&ids=a,b&enabled=1");
        assert_eq!(params.session, "s1");
        assert_eq!(params.workspace, "/w");
        assert_eq!(params.ids, "a,b");
        assert!(params.enabled);
        assert_eq!(params.kind, KIND_INJECT);
        assert_eq!(params.reason, "turn", "旧插件缺省 reason 记 turn");
        assert_eq!(params.turn, -1);
        assert_eq!(params.step, -1);
        assert_eq!(params.bytes, 0);
        assert_eq!(params.generation, 0);
        assert_eq!(params.plugin_version, "unknown");
        assert_eq!(params.kernel_version, "");
        assert_eq!(params.workspace_id, "");
    }

    /// P1 插件的新参数全部被接受（inject 形状，字段一个不丢）。
    #[test]
    fn new_inject_params_are_accepted() {
        let params = parse_note_params(
            "/v1/memory/note?session=s1&workspace=%2Fw&workspace_id=ws-1&ids=aa&enabled=1\
             &kind=inject&reason=library_change&turn=25&step=-1&bytes=758&generation=34\
             &plugin_version=0.2.0&kernel_version=9.9.9",
        );
        assert_eq!(params.kind, KIND_INJECT);
        assert_eq!(params.reason, "library_change");
        assert_eq!(params.turn, 25);
        assert_eq!(params.step, -1);
        assert_eq!(params.bytes, 758);
        assert_eq!(params.generation, 34);
        assert_eq!(params.workspace_id, "ws-1");
        assert_eq!(params.plugin_version, "0.2.0");
        assert_eq!(params.kernel_version, "9.9.9");
    }

    /// skip 形状：kind=skip 被认出来，五个原因桶原样保留（报告按它分解）。
    #[test]
    fn skip_kind_and_reason_buckets_are_recognized() {
        for bucket in ["bridge_failed", "budget", "no_hit", "no_change", "disabled"] {
            let params = parse_note_params(&format!("/v1/memory/note?kind=skip&reason={bucket}"));
            assert_eq!(params.kind, KIND_SKIP);
            assert_eq!(params.reason, bucket);
        }
    }

    /// 数字参数坏了按「不知道」收：turn/step=-1、bytes/generation=0；kind 认不出按 inject
    /// （旧插件没这个参数，认不出就等于没有，绝不能把一次真注入记成 skip）。
    #[test]
    fn garbage_numbers_fall_back_to_unknown() {
        let params =
            parse_note_params("/v1/memory/note?kind=weird&turn=abc&step=1.5&bytes=-3&generation=x");
        assert_eq!(
            params.kind, KIND_INJECT,
            "认不出的 kind 退回 inject（旧插件语义）"
        );
        assert_eq!(params.turn, -1);
        assert_eq!(params.step, -1);
        assert_eq!(params.bytes, 0);
        assert_eq!(params.generation, 0);
    }

    /// 账本行：inject 带 ids 且最多 20 个；skip 的 ids/bytes 由壳清零（插件带错也不写矛盾行）。
    #[test]
    fn ledger_entry_normalizes_inject_ids_and_zeroes_skip_shape() {
        let ids: String = (0..25)
            .map(|index| format!("{index:032x}"))
            .collect::<Vec<_>>()
            .join(",");
        let inject = ledger_note(
            &parse_note_params(&format!(
                "/v1/memory/note?kind=inject&ids={ids}&bytes=42&turn=7"
            )),
            "",
            1234,
        );
        assert_eq!(inject.at_ms, 1234);
        assert_eq!(inject.ids.len(), 20, "ids 最多 20 个");
        assert_eq!(inject.bytes, 42);
        assert_eq!(inject.turn, 7);
        assert!(inject.is_inject());

        let skip = ledger_note(
            &parse_note_params(&format!(
                "/v1/memory/note?kind=skip&reason=budget&ids={ids}&bytes=999"
            )),
            "",
            1234,
        );
        assert!(skip.ids.is_empty(), "skip 不许带 id");
        assert_eq!(skip.bytes, 0, "skip 的字节数恒 0");
        assert_eq!(skip.reason, "budget");
        assert!(!skip.is_inject());
    }

    /// kernel_version 以实例记录为准（桥的 scope_key 就是 family:instance）；查不到才用插件值。
    #[test]
    fn kernel_version_is_filled_from_the_instance_record() {
        let home = std::env::temp_dir().join(format!(
            "memory-routes-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&home).expect("建临时 home");
        let _guard = crate::tests::scoped_xlink_home(&home);

        let mut record = crate::shell::instance::InstanceRecord::new("default", "dsh", 1234, 0);
        record.kernel_version = Some("0.2.2-alpha.2".to_string());
        crate::shell::instance::save_record_to_disk(&record).expect("写实例记录");

        assert_eq!(resolve_kernel_version("dsh:default", ""), "0.2.2-alpha.2");
        assert_eq!(
            resolve_kernel_version("dsh:missing", "9.9.9"),
            "9.9.9",
            "查不到实例记录时保留插件给的值"
        );
        assert_eq!(
            resolve_kernel_version("", "9.9.9"),
            "9.9.9",
            "坐标不合法不查盘"
        );
        assert_eq!(resolve_kernel_version("dsh:", "9.9.9"), "9.9.9");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn experiment_kind_and_cohort_are_accepted_in_note_params() {
        let params = parse_note_params(
            "/v1/memory/note?session=s1&kind=experiment&reason=control&experiment=control&turn=5",
        );
        assert_eq!(params.kind, crate::memory::effect::ledger::KIND_EXPERIMENT);
        assert_eq!(params.reason, "control");
        assert_eq!(params.experiment, "control");
        assert_eq!(params.turn, 5);
        let note = ledger_note(&params, "dsh:default", 123);
        assert_eq!(note.kind, crate::memory::effect::ledger::KIND_EXPERIMENT);
        assert_eq!(note.experiment, "control");
        assert!(note.ids.is_empty());
        assert_eq!(note.bytes, 0);
        assert!(!note.is_inject());
    }

    #[test]
    fn unknown_kind_still_falls_back_to_inject() {
        let params = parse_note_params("/v1/memory/note?kind=weird&session=s1");
        assert_eq!(params.kind, crate::memory::effect::ledger::KIND_INJECT);
        assert_eq!(params.experiment, "");
    }
}

#[cfg(test)]
mod module_route_tests {
    use super::automatic_module_request;

    #[test]
    fn only_automatic_injection_uses_cross_source_ids() {
        assert!(automatic_module_request(
            "/v1/memory/context?module_context=1&q=hello"
        ));
        for path in [
            "/v1/memory/context?q=hello",
            "/v1/memory/context?pending=1",
            "/v1/memory/context?module_context=1&pending=1",
            "/v1/memory/context?module_context=1&context_only=1",
        ] {
            assert!(!automatic_module_request(path), "{path}");
        }
    }
}
#[cfg(test)]
mod target_gate_tests {
    use super::*;
    use crate::memory::store::testkit::{general_input, TempHome};
    use crate::memory::store::{self, Namespace};
    use std::io::{Read as _, Write as _};
    use std::net::{TcpListener, TcpStream};

    /// 真起一个回环连接跑一遍路由：判据要覆盖「路由里到底接没接门禁」。
    /// 只测 `targets::blocked` 纯函数的话，把这里那道 if 删掉不会有任何测试变红。
    fn exchange(handler: fn(&mut TcpStream, &str, &str), path: &str, scope_key: &str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let request = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n");
        let client = std::thread::spawn(move || {
            let mut stream = TcpStream::connect(addr).expect("connect");
            stream.write_all(request.as_bytes()).expect("write");
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .expect("timeout");
            let mut out = String::new();
            let _ = stream.read_to_string(&mut out);
            out
        });
        let (mut server, _) = listener.accept().expect("accept");
        // 真实链路里请求头由分发层读走；这里不读掉就直接响应 + 关连接，内核可能
        // 对未读入站数据发 RST，客户端偶尔只读到半截响应——判据会随机红。
        let mut request_buf = [0u8; 1024];
        let _ = server.read(&mut request_buf);
        handler(&mut server, path, scope_key);
        drop(server);
        client.join().expect("client")
    }

    fn seed(namespace: &Namespace) {
        let mut input = general_input("提交前跑检查", "提交前先跑 npm run check");
        input.pinned = true;
        store::save(namespace, input).expect("save");
        store::set_use_approved(namespace, true).expect("enable");
    }

    fn enable_single_pool(namespace: &Namespace) {
        let mut registry = crate::shell::instance::InstanceRegistry::default();
        let entry = crate::shell::instance::InstanceRecord::new(
            &namespace.instance_id,
            &namespace.family,
            31001,
            1,
        );
        crate::shell::instance::save_record_to_disk(&entry).unwrap();
        registry.add(entry).unwrap();
        crate::shell::instance::save_registry(&registry).unwrap();
        let pair = crate::memory::module::pair().unwrap();
        crate::memory::targets::set_pool("dsh", true, &crate::memory::module::key(&pair)).unwrap();
    }

    #[test]
    fn pool_propose_stops_all_writes_when_disabled_or_source_changed() {
        let _home = TempHome::new("pool-propose-gate");
        let namespace = Namespace::current();
        let existing = store::save(
            &namespace,
            general_input("existing", "already-reviewed-canary"),
        )
        .unwrap();
        enable_single_pool(&namespace);
        let scope = format!("dsh:{}", namespace.instance_id);
        crate::memory::targets::set_pool("dsh", false, "").unwrap();
        let before = std::fs::read(namespace.file()).unwrap();
        // 新候选和已有记录的来源合并都是写操作，关闭后两者均不得落盘。
        for body in ["new-candidate-canary", "already-reviewed-canary"] {
            let response = exchange(
                propose,
                &format!(
                    "/v1/memory/propose?scope=general&title=draft&body={body}&session=new-source"
                ),
                &scope,
            );
            assert!(response.contains("403"), "{response}");
            assert_eq!(std::fs::read(namespace.file()).unwrap(), before);
        }
        let pair = crate::memory::module::pair().unwrap();
        crate::memory::targets::set_pool("dsh", true, &crate::memory::module::key(&pair)).unwrap();
        let allowed = exchange(
            propose,
            "/v1/memory/propose?scope=general&title=draft&body=new-candidate-canary&session=source",
            &scope,
        );
        assert!(allowed.contains("200"), "{allowed}");
        assert!(allowed.contains(r#""state":"pending""#), "{allowed}");
        let doc = store::load(&namespace).unwrap();
        assert_eq!(doc.candidates.len(), 1);
        assert_eq!(doc.memories.len(), 1);
        assert_eq!(doc.memories[0].id, existing.id);
        // 同族另一实例不在已确认来源内，即使开关为开也不能借用许可写入。
        let other = Namespace::for_bridge("dsh", "not-in-confirmed-pair");
        store::save(&other, general_input("other", "other-canary")).unwrap();
        let other_before = std::fs::read(other.file()).unwrap();
        let denied = exchange(
            propose,
            "/v1/memory/propose?scope=general&title=draft&body=other-canary",
            "dsh:not-in-confirmed-pair",
        );
        assert!(denied.contains("403"), "{denied}");
        assert_eq!(std::fs::read(other.file()).unwrap(), other_before);
    }

    #[test]
    fn unconfigured_pool_propose_keeps_legacy_candidate_only_behavior() {
        let _home = TempHome::new("legacy-propose-compatible");
        let namespace = Namespace::current();
        let scope = format!("dsh:{}", namespace.instance_id);
        let response = exchange(
            propose,
            "/v1/memory/propose?scope=general&title=legacy&body=legacy-candidate",
            &scope,
        );
        assert!(response.contains(r#""state":"pending""#), "{response}");
        let doc = store::load(&namespace).unwrap();
        assert!(doc.memories.is_empty());
        assert_eq!(doc.candidates.len(), 1);
    }

    #[test]
    fn pool_tools_only_return_reviewed_memories_without_legacy_permission() {
        let _home = TempHome::new("pool-tool-review");
        let namespace = Namespace::current();
        let approved = store::save(
            &namespace,
            general_input("approved", "reviewed-only-canary"),
        )
        .unwrap();
        let old = store::save(&namespace, general_input("old", "unreviewed-canary")).unwrap();
        store::mutate(&namespace, |doc| {
            doc.memories
                .iter_mut()
                .find(|row| row.id == old.id)
                .unwrap()
                .verification = store::VERIFICATION_UNVERIFIED.into();
            Ok(())
        })
        .unwrap();
        assert!(!store::status(&namespace).use_approved);
        enable_single_pool(&namespace);
        let scope = format!("dsh:{}", namespace.instance_id);
        // memory_search 不带 module_context；提取的 pending=1 也不能夹带历史未审核正文。
        for suffix in ["", "&module_context=1", "&pending=1"] {
            let response = exchange(
                context,
                &format!("/v1/memory/context?q=canary{suffix}"),
                &scope,
            );
            assert!(response.contains("reviewed-only-canary"), "{response}");
            assert!(!response.contains("unreviewed-canary"), "{response}");
        }
        let artifacts = exchange(
            context,
            "/v1/memory/context?q=canary&context_only=1",
            &scope,
        );
        assert!(!artifacts.contains("reviewed-only-canary"));
        assert!(!artifacts.contains("unreviewed-canary"));
        let denied = exchange(read, &format!("/v1/memory/read?id={}", old.id), &scope);
        assert!(denied.contains("404"), "{denied}");
        assert!(!denied.contains("unreviewed-canary"));
        assert_eq!(
            store::find_by_id(&namespace, &old.id, true)
                .unwrap()
                .unwrap()
                .use_count,
            0
        );
        let allowed = exchange(read, &format!("/v1/memory/read?id={}", approved.id), &scope);
        assert!(allowed.contains("reviewed-only-canary"), "{allowed}");
        // 旧许可即使开着，也不能让新模式下关闭的内核绕回旧路径。
        store::set_use_approved(&namespace, true).unwrap();
        crate::memory::targets::set_pool("dsh", false, "").unwrap();
        let disabled = exchange(context, "/v1/memory/context?q=canary&pending=1", &scope);
        assert!(!disabled.contains("reviewed-only-canary"));
        assert!(!disabled.contains("unreviewed-canary"));
        let disabled_read = exchange(read, &format!("/v1/memory/read?id={}", approved.id), &scope);
        assert!(disabled_read.contains("404"));
    }

    #[test]
    fn pool_tool_reads_keep_project_scope_and_legacy_search_stays_compatible() {
        let _home = TempHome::new("pool-tool-scope");
        let namespace = Namespace::current();
        let project = store::save(
            &namespace,
            store::testkit::project_input("ws-a", "scope", "project-private-canary"),
        )
        .unwrap();
        store::set_use_approved(&namespace, true).unwrap();
        store::mutate(&namespace, |doc| {
            doc.memories[0].verification = store::VERIFICATION_UNVERIFIED.into();
            Ok(())
        })
        .unwrap();
        let scope = format!("dsh:{}", namespace.instance_id);
        let legacy = exchange(
            context,
            "/v1/memory/context?q=canary&workspace_id=ws-a",
            &scope,
        );
        assert!(
            legacy.contains("project-private-canary"),
            "旧实例尚未迁移时保留原范围：{legacy}"
        );
        enable_single_pool(&namespace);
        let unreviewed = exchange(
            context,
            "/v1/memory/context?q=canary&workspace_id=ws-a",
            &scope,
        );
        assert!(!unreviewed.contains("project-private-canary"));
        store::review_legacy_record(&namespace, &project.id, project.revision, true).unwrap();
        let wrong = exchange(
            read,
            &format!("/v1/memory/read?id={}&workspace_id=ws-b", project.id),
            &scope,
        );
        assert!(wrong.contains("404"));
        let right = exchange(
            read,
            &format!("/v1/memory/read?id={}&workspace_id=ws-a", project.id),
            &scope,
        );
        assert!(right.contains("project-private-canary"));
    }

    #[test]
    fn context_route_returns_no_memory_when_the_target_is_off() {
        let _home = TempHome::new("route-target-context");
        let namespace = Namespace::current();
        seed(&namespace);
        let scope_key = format!("dsh:{}", namespace.instance_id);
        let path =
            "/v1/memory/context?q=%E6%8F%90%E4%BA%A4&workspace=%2Ftmp%2Fproj&workspace_id=ws-1";
        let before = exchange(context, path, &scope_key);
        assert!(
            before.contains("\"hits\":[{"),
            "开关开着时必须真的检索到：{before}"
        );
        crate::memory::targets::set("dsh", false).expect("close target");
        let after = exchange(context, path, &scope_key);
        assert!(
            after.contains("\"hits\":[]"),
            "目标关闭后不得返回记忆：{after}"
        );
        assert!(after.contains("\"enabled\":false"), "{after}");
    }

    #[test]
    fn read_route_rejects_without_leaking_when_the_target_is_off() {
        let _home = TempHome::new("route-target-read");
        let namespace = Namespace::current();
        seed(&namespace);
        let record = store::list(&namespace, &store::ListFilter::default())
            .memories
            .into_iter()
            .next()
            .expect("seeded record");
        let scope_key = format!("dsh:{}", namespace.instance_id);
        let path = format!("/v1/memory/read?id={}", record.id);
        let before = exchange(read, &path, &scope_key);
        assert!(
            before.contains("\"title\""),
            "开关开着时按 id 可读：{before}"
        );
        crate::memory::targets::set("dsh", false).expect("close target");
        let after = exchange(read, &path, &scope_key);
        assert!(
            !after.contains(&record.title),
            "目标关闭后不得读出正文：{after}"
        );
        assert!(after.contains("404"), "{after}");
    }

    /// 目标开关「显式 true」不是来源许可：use_approved 关掉后按 id 读必须停，
    /// 被拒的读取也不能记进 use_count（取消授权不该留下使用痕迹）。
    #[test]
    fn read_route_follows_the_source_use_approved_gate() {
        let _home = TempHome::new("route-read-use-approved");
        let namespace = Namespace::current();
        seed(&namespace);
        let record = store::list(&namespace, &store::ListFilter::default())
            .memories
            .into_iter()
            .next()
            .expect("seeded record");
        let scope_key = format!("dsh:{}", namespace.instance_id);
        let path = format!(
            "/v1/memory/read?id={}&workspace=%2Ftmp%2Fproj&workspace_id=ws-1",
            record.id
        );
        let before = exchange(read, &path, &scope_key);
        assert!(
            before.contains(&record.title),
            "use_approved=true 时按 id 可读：{before}"
        );

        // 用户关掉「DSH 原有记忆与自动提取」；目标开关仍未显式设置（跟随原设置）。
        store::set_use_approved(&namespace, false).expect("disable");
        let after = exchange(read, &path, &scope_key);
        assert!(
            !after.contains(&record.title),
            "撤销来源许可后不得读出正文：{after}"
        );
        assert!(after.contains("404"), "{after}");

        let used = store::find_by_id(&namespace, &record.id, true)
            .expect("readable")
            .expect("record");
        assert_eq!(used.use_count, 1, "被拒的读取不许增加 use_count");

        // 显式把接收方开关打到 true 也不能绕过未授予的来源许可。
        crate::memory::targets::set("dsh", true).expect("explicit allow");
        let explicit = exchange(read, &path, &scope_key);
        assert!(
            !explicit.contains(&record.title),
            "目标开关不能替代来源许可：{explicit}"
        );
    }

    #[test]
    fn pending_extraction_context_follows_the_unified_kernel_switch() {
        let _home = TempHome::new("route-target-pending");
        let namespace = Namespace::current();
        seed(&namespace);
        store::submit_candidate(
            &namespace,
            store::testkit::candidate_input("待审核偏好", "pending-private-canary"),
        )
        .expect("seed candidate");
        let scope_key = format!("dsh:{}", namespace.instance_id);
        let path = "/v1/memory/context?q=%E6%8F%90%E4%BA%A4&pending=1";
        let before = exchange(context, path, &scope_key);
        assert!(
            before.contains("\"enabled\":true"),
            "启用时允许提取上下文：{before}"
        );
        assert!(
            before.contains("pending-private-canary"),
            "启用时可读取候选以判断重复：{before}"
        );

        // 旧接收方关闭和新公共池关闭都必须停止提取；旧库授权仍然保持 true。
        crate::memory::targets::set("dsh", false).expect("close legacy target");
        for use_pool in [false, true] {
            if use_pool {
                enable_single_pool(&namespace);
                let reopened = exchange(context, path, &scope_key);
                assert!(
                    reopened.contains("pending-private-canary"),
                    "重新启用后原候选仍在：{reopened}"
                );
                crate::memory::targets::set_pool("dsh", false, "").expect("close pool");
            }
            let saved = std::fs::read(namespace.file()).expect("library bytes");
            let response = exchange(context, path, &scope_key);
            assert!(
                response.contains("200"),
                "关闭以空结果应答，不把正常停用当故障：{response}"
            );
            let body: serde_json::Value =
                serde_json::from_str(response.split("\r\n\r\n").nth(1).expect("HTTP body"))
                    .expect("JSON");
            assert_eq!(body["enabled"], false, "关闭后不能继续提取：{response}");
            assert_eq!(body["hits"], serde_json::json!([]));
            // 空候选字段按协议省略，不能把缺字段误判成仍有候选。
            assert!(
                body.get("pending").is_none(),
                "关闭后不得返回候选：{response}"
            );
            assert!(!response.contains("pending-private-canary"));
            assert_eq!(
                std::fs::read(namespace.file()).unwrap(),
                saved,
                "停用只停止使用，不删除候选或改变旧库授权"
            );
        }
    }
}
