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

pub(crate) fn context(stream: &mut TcpStream, path: &str, scope_key: &str) {
    // 与 sync 同一条纪律：**GET + 查询串，不读请求体**（见下面 sync 那条注释）。
    // 中文与空格由插件百分号编码，这里解码后再进检索。
    let namespace = memory_namespace(scope_key);
    let query = query_decoded(path, "q");
    let workspace = query_decoded(path, "workspace");
    // `workspace_id` 是插件经工作区注册表解析出来的内核 id；解析不到就只带路径，
    // 后端按路径兜底匹配（见 `query::scope_matches`）。三条路由统一叫
    // `workspace_id`——`read` 那条的 `id` 是**记忆 id**，两处同名必然记混。
    let workspace_id = query_decoded(path, "workspace_id");
    // 只读工具可以缩小到扩展资料；权限判据相同，且不会把记忆的固定项挤进这份预算。
    let retrieved = if query_decoded(path, "context_only") == "1" {
        crate::memory::query::retrieve_artifacts(&namespace, &query, &workspace_id, &workspace)
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
}

/// 解析 note 的查询串。**纯函数**：不读库、不读路径，所以路由层的兼容性可以直接测。
fn parse_note_params(path: &str) -> NoteParams {
    let kind = if query_decoded(path, "kind") == crate::memory::effect::ledger::KIND_SKIP {
        crate::memory::effect::ledger::KIND_SKIP.to_string()
    } else {
        crate::memory::effect::ledger::KIND_INJECT.to_string()
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
            if let Err(error) = crate::memory::store::note_recall(&namespace, &id_list) {
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

pub(crate) fn read(stream: &mut TcpStream, path: &str, scope_key: &str) {
    let namespace = memory_namespace(scope_key);
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
    // **按 id 读也要过两道门**：项目归属，与启用状态。
    //
    // · 项目：id 猜不到，但会话历史里留着旧 id，不校验就能在 B 项目里把 A
    //   项目的记忆读出来（设计承诺的是不进入）。通用偏好对谁都算；项目记录
    //   必须与调用方给的坐标对得上。
    // · 状态：`find_by_id(.., false)` 把**已停用**的挡在外面。用户点了「停用」
    //   就是不想再被模型用着，而注入块的表头里有 id（`[id] 标题`），会话历史里
    //   因此一直留着它——只挡检索不挡按 id 读，停用这个按钮对模型就是无效的。
    //
    // 顺序是「先查、再判、最后计数」：反过来会让**被拒的读也记一次使用**，
    // 统计被越权尝试污染。
    let workspace = query_decoded(path, "workspace");
    let workspace_id = query_decoded(path, "workspace_id");
    let found = match crate::memory::store::find_by_id(&namespace, &id, false) {
        Ok(found) => found,
        Err(error) => {
            // 文档读不出来时**如实说读不出来**，不退成「不属于当前项目」——
            // 后者会让用户去查自己有没有记错项目 id，而真因在盘上那份文件。
            // 留痕**别记成 `denied=`**：那是授权事件，而这一条是盘上那份文件坏了。
            // 两类混在一个前缀下，排查的人打开「查看日志」会往错的方向查。
            note_memory_call("read", "failed=unreadable");
            let body = serde_json::json!({ "error": error.to_string() }).to_string();
            let _ = crate::openai::http::write_response(stream, 500, "application/json", &body);
            return;
        }
    };
    let allowed = found.is_some_and(|record| {
        crate::memory::query::scope_matches(&record, &workspace_id, &workspace)
    });
    if !allowed {
        note_memory_call("read", "denied=out-of-scope-or-disabled");
        let _ = crate::openai::http::write_response(
            stream,
            404,
            "application/json",
            r#"{"error":"这条记忆不属于当前项目，或已被停用"}"#,
        );
        return;
    }
    match crate::memory::store::read_and_note_use(&namespace, &id) {
        Ok(record) => {
            note_memory_call(
                "read",
                &format!("id={} revision={}", record.id, record.revision),
            );
            let body = serde_json::to_string(&record).unwrap_or_else(|_| "{}".to_string());
            let _ = crate::openai::http::write_response(stream, 200, "application/json", &body);
        }
        Err(error) => {
            let body = serde_json::json!({ "error": error.to_string() }).to_string();
            let _ = crate::openai::http::write_response(stream, 404, "application/json", &body);
        }
    }
}

pub(crate) fn propose(stream: &mut TcpStream, path: &str, scope_key: &str) {
    // 候选**只能进待确认**：这条路由调的是 `submit_candidate`，它写不进权威
    // 记录。模型想自己批准候选没有入口——批准只存在于面板的 Tauri 命令面。
    let namespace = memory_namespace(scope_key);
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
}
