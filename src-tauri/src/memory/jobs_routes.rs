//! **提取任务（P2）的路由**：排任务、领租约、续租、交结果、报失败、看状态。
//!
//! 从 `memory/routes.rs` 再拆出来（2026-10-15）：那个文件是路由分发器，jobs 那七条已经占了
//! 一多半，而它当天连着两次被顶过预算线——`FILE_BUDGETS` 里上一轮就写了「拆分已经欠着了」。
//! 拆的界线是**两件事**：这一份管「一次提取从排队到收尾」，`routes.rs` 管「记忆本身的调用」
//! （检索、按 id 读、候选提交、注入回报）。
//!
//! 三条纪律与另外那半边逐条对齐：GET + 查询串不读请求体；「谁能调、最坏能造成什么」写清；
//! 这些路由**都不写权威记录**——提取出来的东西走 `candidates`，用户点了保存才成记忆。
//!
//! **令牌由 `/claim` 生成并返回**，`renew` / `finish` / `fail` 带着它回来：只有**当前**令牌
//! 能提交（§8-6 的迟到结果检查）。这里不记 `note_memory_call`（那会把与注入无关的行混进
//! 「最近注入」），但用**另一个标签**（`memory-jobs`）留痕：进「查看日志」不进「最近注入」，
//! **只记路由与结局，不记正文、不记令牌**（正文是用户私有内容，令牌是凭据）。

use std::net::TcpStream;

use super::routes::memory_namespace;
use crate::plugins::builtin::bridge::query_decoded;

/// 提取任务路由的留痕：**另一个标签**，与「最近注入」分开（见 [`jobs`] 的文档）。
fn note_job_call(action: &str, detail: &str) {
    // 提取任务那几条也是「插件调用过壳」——开关改动的确认不区分是哪条路由。
    crate::memory::recent::mark_plugin_call();
    crate::shell::shell_events::record("memory-jobs", &format!("{action} {detail}"));
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
///
/// **但要可见**（评审第 12 条）：原先一条痕迹都不留，而插件 logger 在这个内核里不进 stdout——
/// 用户问「为什么一条记忆都没提取出来」时唯一可查处是 `jobs.json`。现在用**另一个标签**
/// （`memory-jobs`）记一行：进「查看日志」不进「最近注入」，**只记路由与结局，不记正文与令牌**。
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
        // §8-1 的第三条判据「**获许可的**会话」在这里判，不在插件里：开关现在是什么状态
        // 只有一份来源（权威记录），插件手里那份是上一次检索时的快照。答 `disabled` 而不是
        // 报错——这不是故障，是用户没开这个功能。
        //
        // **读不出来 ≠ 开关关着**（记忆内核那次评审第 8 条）：库损坏时原先回 `disabled`，
        // 插件与日志都不留真因，用户只在记忆页看到那条 warning。所以先分开答复。
        "/enqueue" => match crate::memory::store::load(&namespace) {
            Err(error) => failed(error),
            Ok(doc) if !doc.meta.use_approved => (
                200,
                serde_json::json!({ "outcome": "disabled" }).to_string(),
            ),
            Ok(_) => {
                let key = jobs::JobKey {
                    session: query_decoded(path, "session"),
                    cursor: query_decoded(path, "cursor").parse().unwrap_or(0),
                    extractor: query_decoded(path, "extractor"),
                    // 起点由 `jobs::enqueue` 按「这段会话上一次提取到哪儿」算出来（评审第 7 条），
                    // 路由这里给 0 就会被它覆盖。
                    from_cursor: 0,
                };
                match jobs::mutate(&namespace, |doc, now| jobs::enqueue(doc, key, now)) {
                    Ok(outcome) => {
                        note_job_call("enqueue", outcome.as_str());
                        (
                            200,
                            serde_json::json!({ "outcome": outcome.as_str() }).to_string(),
                        )
                    }
                    Err(error) => failed(error),
                }
            }
        },
        // 没东西可领时**不是错误**：插件按固定节奏来问，答 `token: null` 让它安静退下。
        // `session` 可选：带上就**只领这个会话的任务**（插件总是带）。不带=领全表最早的，
        // 保留给测试与旧调用点。领自己那条是「A 的内容不许带 B 的项目坐标」的前提（评审第 3 条）。
        "/claim" => match jobs::mutate(&namespace, |doc, now| {
            let wanted = query_decoded(path, "session");
            let wanted = if wanted.is_empty() {
                None
            } else {
                Some(wanted.as_str())
            };
            // **到期才领**：去抖是调度策略，放在这一层（见 `jobs::claim_due_for` 的文档）。
            jobs::claim_any_due_for(doc, now, wanted, true)
        }) {
            Ok(Some(claimed)) => {
                note_job_call(
                    "claim",
                    &format!(
                        "cursor={} attempts={}",
                        claimed.key.cursor, claimed.attempts
                    ),
                );
                (
                    200,
                    serde_json::to_string(&claimed)
                        .unwrap_or_else(|_| r#"{"token":null}"#.to_string()),
                )
            }
            Ok(None) => {
                note_job_call("claim", "none");
                (200, r#"{"token":null}"#.to_string())
            }
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
                Ok(done) => {
                    // 交不上通常是「租约被接管了」——这一行就是排障要看的那一行。
                    note_job_call("finish", if done { "ok" } else { "stale-token" });
                    (200, serde_json::json!({ "finished": done }).to_string())
                }
                Err(error) => failed(error),
            }
        }
        "/fail" => {
            let token = query_decoded(path, "token");
            let reason = query_decoded(path, "error");
            // 失败只记 **stage**（`read` / `model` / `parse` / `submit`）：那是排障要看的那一列，
            // 而 reason 里可能带记忆标题（业务拒绝的文案会引用它）——日志面板不记用户正文。
            match jobs::mutate(&namespace, |doc, now| jobs::fail(doc, &token, &reason, now)) {
                // `retryable: null` = 令牌对不上（不是这个任务的主人），与「不能重试了」区分开。
                Ok(outcome) => {
                    let stage = reason.split('：').next().unwrap_or("unknown");
                    note_job_call("fail", &format!("stage={stage} retryable={outcome:?}"));
                    (200, serde_json::json!({ "retryable": outcome }).to_string())
                }
                Err(error) => failed(error),
            }
        }
        // 提取结果落地：**只有持有当前租约令牌的任务能提交**（§8-6），而且提交前要过四道闸——
        // 租约还在跑、开关还开着、会话对得上、引用的序号不超过任务冻结的那个游标。
        // 任何一道不过都答 `submitted: false` + 原因：这不是故障，是「这次结果不该落地」。
        "/submit" => {
            // **判定与写入必须在同一个临界区里**（独立评审第 10 条）：原先「读任务表判令牌」与
            // 「写候选」是两次各自加锁的操作，中间那段窗口里租约可以过期并被接管——迟到的结果
            // 照样落库，§8-6 的迟到结果检查被绕过（窗口窄但非零）。这里把跨进程锁**提前拿到**
            // 整个分支结束，中间那次 `store::submit_candidate` 靠锁的**同线程可重入**通过。
            let _guard = match crate::memory::lock::WriteLock::new(&namespace.dir()) {
                Ok(guard) => guard,
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
                // 来源引用是「这一条候选出自哪句话」，两头都该落在冻结范围里（评审第 17 条）：
                // 只有 `end_seq` 有校验时，一个比它更大的 `start_seq` 会被原样写进库里。
                Some(_) if start_seq > end_seq => {
                    refuse("来源引用的起点比终点还靠后，这不是一段有效的范围")
                }
                // 起点还该在**这次真的读过的那一段**里（评审第 7 条）：插件从 `from_cursor`
                // 开始读，指到它之前的序号说明它引用的是没给模型看过的内容。
                Some(job) if start_seq < job.key.from_cursor => {
                    refuse("来源引用指向这次没有读过的那一段（在起点之前）")
                }
                Some(job) => {
                    let input = crate::memory::store::CandidateInput {
                        // 冲突标记是**模型标的**（提取那条路才有），壳只做形状与限额校验
                        // （`memory/conflicts.rs`）：限条数、限长度、去重保序，超限的报回去。
                        // 「id 确实来自那次检索」那一半在插件侧校验——那才是拿到检索结果的那一侧。
                        conflicts_with: query_decoded(path, "conflicts")
                            .split(',')
                            .map(str::trim)
                            .filter(|item| !item.is_empty())
                            .map(str::to_string)
                            .collect(),
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
                            kind: crate::memory::store::SOURCE_EXTRACT.to_string(),
                            session: job.key.session.clone(),
                            start_seq,
                            end_seq,
                            revision: job.key.extractor.clone(),
                            at: String::new(),
                        },
                        verification: String::new(),
                    };
                    match crate::memory::store::submit_candidate(&namespace, input) {
                        // **并入来源也算提交成功**（C4）：同一个提取器在另一个会话又提了
                        // 同一件事时，不生成候选而是给已有那条补一个来源——插件那边按
                        // `submitted === true` 计数，把并入当成失败会让任务反复重排。
                        Ok(crate::memory::store::SubmitOutcome::Merged(record)) => (
                            200,
                            serde_json::json!({ "submitted": true, "merged": true, "id": record.id })
                                .to_string(),
                        ),
                        Ok(crate::memory::store::SubmitOutcome::Candidate(candidate)) => (
                            200,
                            serde_json::json!({ "submitted": true, "id": candidate.id }).to_string(),
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
