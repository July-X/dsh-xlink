//! 记忆与自动提取（P2）的路由：检索、按 id 读、候选提交、注入回报、任务与租约。
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
    crate::shell::shell_events::record("memory-bridge", &format!("{route} {detail}"));
}

/// 自动提取（P2）的任务与租约路由。**全是 GET + 查询串**，与记忆那几条同一条纪律。
///
/// 谁能调：只有拿到 Bearer 的插件进程；最坏能造成什么：排一个后台提取任务、领一个租约、
/// 或用错的令牌交结果（交不上）。**这些路由不写权威记录**——提取出来的东西走 `candidates`，
/// 用户点了保存才成记忆。
///
/// 令牌由 `/claim` 生成并返回，`renew` / `finish` / `fail` 带着它回来：只有**当前**令牌能提交
/// （§8-6 的迟到结果检查）。这里不记 `note_memory_call`：这些不是「记忆调用」，混进那个环里
/// 会让面板的「最近注入」多出一堆与注入无关的行。
pub(crate) fn jobs(stream: &mut TcpStream, path: &str, scope_key: &str) {
    use crate::memory::jobs;
    let namespace = memory_namespace(scope_key);
    // 先切掉查询串再匹配：`path` 带着 `?…`（与记忆那几条一样），拿它直接比会一条都匹配不上
    // ——而那正好是最难看出来的一种错：所有带参数的调用都回 404，没参数的却能跑通。
    let action = path
        .trim_start_matches("/v1/memory/jobs")
        .split('?')
        .next()
        .unwrap_or("");
    let failed = |error: crate::shell::error::AppError| {
        (
            500,
            serde_json::json!({ "error": error.to_string() }).to_string(),
        )
    };
    let (status, body) = match action {
        "/enqueue" => {
            // §8-1 的第三条判据「**获许可的**会话」在这里判，不在插件里：开关现在是什么状态
            // 只有一份来源（权威记录），插件手里那份是上一次检索时的快照。答 `disabled` 而不是
            // 报错——这不是故障，是用户没开这个功能。
            let allowed = crate::memory::store::load(&namespace)
                .map(|doc| doc.meta.use_approved)
                .unwrap_or(false);
            if !allowed {
                (
                    200,
                    serde_json::json!({ "outcome": "disabled" }).to_string(),
                )
            } else {
                let key = jobs::JobKey {
                    session: query_decoded(path, "session"),
                    cursor: query_decoded(path, "cursor").parse().unwrap_or(0),
                    extractor: query_decoded(path, "extractor"),
                };
                match jobs::mutate(&namespace, |doc, now| jobs::enqueue(doc, key, now)) {
                    Ok(outcome) => (
                        200,
                        serde_json::json!({ "outcome": outcome.as_str() }).to_string(),
                    ),
                    Err(error) => failed(error),
                }
            }
        }
        // 没东西可领时**不是错误**：插件按固定节奏来问，答 `token: null` 让它安静退下。
        "/claim" => match jobs::mutate(&namespace, jobs::claim_any) {
            Ok(Some(claimed)) => (
                200,
                serde_json::to_string(&claimed).unwrap_or_else(|_| r#"{"token":null}"#.to_string()),
            ),
            Ok(None) => (200, r#"{"token":null}"#.to_string()),
            Err(error) => failed(error),
        },
        "/renew" => {
            let token = query_decoded(path, "token");
            match jobs::mutate(&namespace, |doc, now| jobs::renew(doc, &token, now)) {
                Ok(renewed) => (200, serde_json::json!({ "renewed": renewed }).to_string()),
                Err(error) => failed(error),
            }
        }
        "/finish" => {
            let token = query_decoded(path, "token");
            match jobs::mutate(&namespace, |doc, now| jobs::finish(doc, &token, now)) {
                Ok(done) => (200, serde_json::json!({ "finished": done }).to_string()),
                Err(error) => failed(error),
            }
        }
        "/fail" => {
            let token = query_decoded(path, "token");
            let reason = query_decoded(path, "error");
            match jobs::mutate(&namespace, |doc, now| jobs::fail(doc, &token, &reason, now)) {
                // `retryable: null` = 令牌对不上（不是这个任务的主人），与「不能重试了」区分开。
                Ok(outcome) => (200, serde_json::json!({ "retryable": outcome }).to_string()),
                Err(error) => failed(error),
            }
        }
        // 提取结果落地：**只有持有当前租约令牌的任务能提交**（§8-6），而且提交前要过四道闸——
        // 租约还在跑、开关还开着、会话对得上、引用的序号不超过任务冻结的那个游标。
        // 任何一道不过都答 `submitted: false` + 原因：这不是故障，是「这次结果不该落地」。
        "/submit" => {
            let token = query_decoded(path, "token");
            let session = query_decoded(path, "session");
            let end_seq: u64 = query_decoded(path, "end_seq").parse().unwrap_or(0);
            let start_seq: u64 = query_decoded(path, "start_seq").parse().unwrap_or(0);
            let refuse = |reason: &str| {
                (
                    200,
                    serde_json::json!({ "submitted": false, "reason": reason }).to_string(),
                )
            };
            let job = match crate::memory::jobs::load(&namespace) {
                Ok(doc) => crate::memory::jobs::running_job(&doc, &token).cloned(),
                Err(error) => {
                    let (status, body) = failed(error);
                    let _ = crate::openai::http::write_response(
                        stream,
                        status,
                        "application/json",
                        &body,
                    );
                    return;
                }
            };
            let allowed = crate::memory::store::load(&namespace)
                .map(|doc| doc.meta.use_approved)
                .unwrap_or(false);
            match job {
                None => refuse("这个租约不算数了（任务已被接管、已完成，或令牌不对）"),
                Some(_) if !allowed => refuse("记忆开关已关：这次提取不收"),
                Some(job) if job.key.session != session => refuse("来源会话与任务冻结的那个对不上"),
                Some(job) if end_seq == 0 || end_seq > job.key.cursor => {
                    refuse("引用的序号超过了任务冻结的截止序号")
                }
                Some(job) => {
                    let input = crate::memory::store::CandidateInput {
                        scope: query_decoded(path, "scope"),
                        workspace_id: query_decoded(path, "workspace_id"),
                        workspace_path: query_decoded(path, "workspace"),
                        title: query_decoded(path, "title"),
                        body: query_decoded(path, "body"),
                        keywords: query_decoded(path, "keywords")
                            .split(',')
                            .map(str::trim)
                            .filter(|item| !item.is_empty())
                            .map(str::to_string)
                            .collect(),
                        // 来源引用由**这一侧**按任务的冻结范围写：插件只能声明引用了哪些序号，
                        // 而范围本身来自任务键（这是「不能覆写更新结果」的一半）。
                        source: crate::memory::store::MemorySource {
                            kind: "extract".to_string(),
                            session: job.key.session.clone(),
                            start_seq,
                            end_seq,
                            revision: job.key.extractor.clone(),
                            at: String::new(),
                        },
                        verification: String::new(),
                    };
                    match crate::memory::store::submit_candidate(&namespace, input) {
                        Ok(candidate) => (
                            200,
                            serde_json::json!({ "submitted": true, "id": candidate.id })
                                .to_string(),
                        ),
                        Err(error) => refuse(&error.to_string()),
                    }
                }
            }
        }
        "/status" => match jobs::mutate(&namespace, |doc, _| jobs::summary(doc)) {
            Ok(summary) => (
                200,
                serde_json::to_string(&summary).unwrap_or_else(|_| r#"{"pending":0}"#.to_string()),
            ),
            Err(error) => failed(error),
        },
        _ => (
            404,
            serde_json::json!({ "error": "未知的任务路由" }).to_string(),
        ),
    };
    let _ = crate::openai::http::write_response(stream, status, "application/json", &body);
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
    match crate::memory::query::retrieve(&namespace, &query, &workspace_id, &workspace) {
        Ok(context) => {
            note_memory_call(
                "context",
                &format!("enabled={} hits={}", context.enabled, context.hits.len()),
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

pub(crate) fn note(stream: &mut TcpStream, path: &str, scope_key: &str) {
    // 插件在**真的用了**这次检索之后回来说一声（含「开关关着/一条没命中」——
    // 那也是模型实际看到的状态）。谁能调：只有拿到 Bearer 的插件进程；最坏能造成
    // 什么：往一个进程内、上限 20 条的环形里塞一条标题列表，不碰权威记录。
    //
    // `session` 只用于展示与排查（面板按会话分组），**不参与任何授权判断**：
    // 命名空间仍由桥的绑定决定，调用方传什么都不影响。
    let ids = query_decoded(path, "ids");
    let enabled = query_decoded(path, "enabled") != "0";
    crate::memory::recent::record(
        &memory_namespace(scope_key),
        &query_decoded(path, "session"),
        &query_decoded(path, "workspace"),
        enabled,
        &ids,
    );
    let hits = ids.split(',').filter(|id| !id.is_empty()).count();
    note_memory_call("note", &format!("enabled={enabled} hits={hits}"));
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
    let input = crate::memory::store::CandidateInput {
        scope: {
            let scope = query_decoded(path, "scope");
            if scope.is_empty() {
                crate::memory::store::SCOPE_PROJECT.to_string()
            } else {
                scope
            }
        },
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
        Ok(candidate) => {
            note_memory_call("propose", &format!("candidate={}", candidate.id));
            let body = serde_json::json!({
                "id": candidate.id,
                "state": candidate.state,
                "title": candidate.title,
            })
            .to_string();
            let _ = crate::openai::http::write_response(stream, 200, "application/json", &body);
        }
        Err(error) => {
            let body = serde_json::json!({ "error": error.to_string() }).to_string();
            let _ = crate::openai::http::write_response(stream, 400, "application/json", &body);
        }
    }
}
