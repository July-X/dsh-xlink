//! 「同一件事别长成两条」：按**正文指纹**判重复，以及用户写下之后收掉重复候选。
//!
//! 两条写入路径共用这份判据：模型提候选（`submit_candidate`）与用户手动保存 / 批准
//! （`save` / `decide_candidate`）。判据写在 `store` 里会让那个文件继续长大——它已经贴着
//! 665 行的反棘轮线（≥600 只许下调）；写在 `input.rs` 也不对：那一层管「单条记录怎么规范化」，
//! 这一层管「与库里已有的比一比」，是两件事。
//!
//! **首版只有指纹判据**（精确重复，正文去空白后 SHA-256 相同）。判「意思一样但写法不同」
//! 需要模型比对，那是 P2 的事——见 `docs/features/memory/memory-progress.md` 的 C4/C5/C6。
//! 宁可多收一条让人自己看，也不要拿近似当重复：误判成重复会让用户写下的东西**静默消失**。

/// 同一正文是否**在这个范围里**已经在库（权威记录或待确认候选），是就返回一句给用户的原因。
///
/// **用指纹而不是语义**：判「意思一样但写法不同」需要模型比对，那是 P2 的事（清单里的 C5）。
/// 首版把**精确重复**挡掉——它是模型跨会话反复提同一件事时最常见的形状，也是唯一能机械判的。
///
/// **指纹要带范围**（独立评审第 16 条）：原先在全库比，于是 A 项目里已有的那句
/// 「提交前跑 npm run check」会让 B 项目提同一句时被拒——两个项目各自定下同一条约定完全正常，
/// 而 B 项目的用户看到的是「已经在记忆里了」却在自己的项目里找不到它。
/// 判据复用 [`crate::memory::query::scope_matches_fields`]（**同一份规则**，不另写一遍）：
/// **通用**候选只与通用记录比（通用记录处处可见，再存一份才是真重复）；**项目**候选与该项目的
/// 可见记录比（含通用记录）。
pub(crate) fn duplicate_reason_for(
    doc: &super::store::MemoryDoc,
    clean: &super::input::Clean,
) -> Option<String> {
    duplicate_reason(
        doc,
        &clean.scope,
        &clean.workspace_id,
        &clean.workspace_path,
        &clean.body,
    )
}

pub(crate) fn duplicate_reason(
    doc: &super::store::MemoryDoc,
    scope: &str,
    workspace_id: &str,
    workspace_path: &str,
    body: &str,
) -> Option<String> {
    let value = super::store::fingerprint(body);
    let general_candidate = scope == super::store::SCOPE_GENERAL;
    // 这条记录**在候选的范围里看得见吗**——看不见就不算重复。
    let in_scope = |record_scope: &str, id: &str, path: &str| -> bool {
        if general_candidate {
            // 通用候选只认通用记录：项目记录在别的项目里看不见，为它拒收一条通用偏好是错的。
            return record_scope == super::store::SCOPE_GENERAL;
        }
        super::query::scope_matches_fields(record_scope, id, path, workspace_id, workspace_path)
    };
    if let Some(record) = doc.memories.iter().find(|record| {
        super::store::fingerprint(&record.body) == value
            && in_scope(&record.scope, &record.workspace_id, &record.workspace_path)
    }) {
        return Some(format!(
            "这条内容已经在记忆里了（「{}」，状态 {}）。重复收它只会让同一件事出现两份。",
            record.title, record.state
        ));
    }
    doc.candidates
        .iter()
        .find(|candidate| {
            candidate.state == super::store::CANDIDATE_PENDING
                && super::store::fingerprint(&candidate.body) == value
                && in_scope(
                    &candidate.scope,
                    &candidate.workspace_id,
                    &candidate.workspace_path,
                )
        })
        .map(|candidate| {
            format!(
                "这条内容已经在待确认里了（「{}」）。请先在管理面板里处理掉它。",
                candidate.title
            )
        })
}

/// 用户已经用同样的正文写下（或批准）了一条记忆之后，把还在待确认里的重复候选收掉。
///
/// §7.2 的「用户纠正优先于旧候选」在首版能精确做的那一半：**精确重复**由指纹判，用户一旦
/// 自己写下同一条，那条候选就没有意义了（留着它，用户还得再点一次「保存」并得到第二份）。
/// 「纠正」（意思相近但写法不同）没有语义判断，留给 P2 的模型比对，或由用户在面板里删掉其一。
///
/// **状态改成已拒绝，不是删掉**：文档里留痕，用户问「我提的那条呢」时有据可查。
pub(crate) fn supersede_pending_duplicates(
    doc: &mut super::store::MemoryDoc,
    body: &str,
    now: &str,
) -> usize {
    let value = super::store::fingerprint(body);
    let mut superseded = 0;
    for candidate in doc.candidates.iter_mut() {
        if candidate.state == super::store::CANDIDATE_PENDING
            && super::store::fingerprint(&candidate.body) == value
        {
            candidate.state = super::store::CANDIDATE_REJECTED.to_string();
            candidate.decided_at = now.to_string();
            superseded += 1;
        }
    }
    superseded
}
