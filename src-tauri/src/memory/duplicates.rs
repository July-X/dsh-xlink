//! 「同一件事别长成两条」：按**正文指纹**找库里已有的那条，并决定是并入来源还是拒收。
//!
//! 两条写入路径共用这份判据：模型提候选（`submit_candidate`）与用户手动保存 / 批准
//! （`save` / `decide_candidate`）。判据写在 `store` 里会让那个文件继续长大——它已经贴着
//! 665 行的反棘轮线（≥600 只许下调）；写在 `input.rs` 也不对：那一层管「单条记录怎么规范化」，
//! 这一层管「与库里已有的比一比」，是两件事。
//!
//! **指纹判据只认精确重复**（正文去空白后 SHA-256 相同）：跨会话再提一次同一件事是自动提取
//! 最常见的形状，而它现在**并入已有记录的来源列表**（C4），不再被整条丢掉。判「意思一样但
//! 写法不同」需要模型比对，那是 P2 的事——见 `docs/features/memory/memory-progress.md` 的 C5/C6。
//! 宁可多收一条让人自己看，也不要拿近似当重复：误判成重复会让用户写下的东西**静默消失**。

/// 这条记录**在候选的范围里看得见吗**——「算不算重复」（[`find_hit`]）、
/// 「要不要把待确认里的收掉」（[`supersede_pending_duplicates`]）与「并入来源」
/// （[`merge_source`]）**共用这一份规则**。
///
/// 独立评审第 1 条（记忆内核那次）正是这条规则只写了一半的后果：写入闸按范围判了，
/// 而收掉重复候选那一半仍按**全库**指纹扫——A、B 两个仓库各自提了同一条约定时，
/// 用户在 A 点保存会让 **B 的那条从待确认里静默消失**（面板与导出都只列 pending，
/// 任何界面上都看不到它）。**规则一旦有两份实现就会分叉**，这是本仓的老账。
fn in_scope(
    record_scope: &str,
    record_workspace_id: &str,
    record_workspace_path: &str,
    scope: &str,
    workspace_id: &str,
    workspace_path: &str,
) -> bool {
    if scope == super::store::SCOPE_GENERAL {
        // 通用候选只认通用记录：项目记录在别的项目里看不见，为它拒收一条通用偏好是错的。
        return record_scope == super::store::SCOPE_GENERAL;
    }
    super::query::scope_matches_fields(
        record_scope,
        record_workspace_id,
        record_workspace_path,
        workspace_id,
        workspace_path,
    )
}

/// 库里那件「同一件事」落在哪儿。**一次查找**给出全部结论：合并与拒收读的是同一份指纹与
/// 同一份范围规则——先前是两次独立查找，于是「能并入来源」与「该拒收」可以各判各的，
/// 而两者的分叉点只有「已接受 / 已停用」这一处必须由一处代码说了算。
enum Hit {
    /// 一条**已接受**记录的下标：来源可以并进去。
    Accepted(usize),
    /// 其余落点一律拒收，附一句给用户看的原因。
    Other(String),
}

/// 同正文、且在这次提交的范围里看得见的那个落点。
///
/// **用指纹而不是语义**：判「意思一样但写法不同」需要模型比对，那是 P2 的事（清单里的 C5）。
/// 首版把**精确重复**认出来——它是模型跨会话反复提同一件事时最常见的形状，也是唯一能机械判的。
///
/// **指纹要带范围**（独立评审第 16 条）：原先在全库比，于是 A 项目里已有的那句
/// 「提交前跑 npm run check」会让 B 项目提同一句时被拒——两个项目各自定下同一条约定完全正常，
/// 而 B 项目的用户看到的是「已经在记忆里了」却在自己的项目里找不到它。
/// 判据复用 [`crate::memory::query::scope_matches_fields`]（**同一份规则**，不另写一遍）。
fn find_hit(doc: &super::store::MemoryDoc, clean: &super::input::Clean) -> Option<Hit> {
    let value = super::store::fingerprint(&clean.body);
    // 提交侧的三个坐标取一次名字：下面两处判重读的是**同一组**坐标，抄两遍就会漂。
    let (scope, id, path) = (&clean.scope, &clean.workspace_id, &clean.workspace_path);
    let visible = |record_scope: &str, record_id: &str, record_path: &str| -> bool {
        in_scope(record_scope, record_id, record_path, scope, id, path)
    };
    if let Some(index) = doc.memories.iter().position(|record| {
        super::store::fingerprint(&record.body) == value
            && visible(&record.scope, &record.workspace_id, &record.workspace_path)
    }) {
        let record = &doc.memories[index];
        // **已停用的记录不给合并**：用户明确停用过的东西，模型不该顺手把它带回来。
        return Some(if record.state == super::store::STATE_ACCEPTED {
            Hit::Accepted(index)
        } else {
            Hit::Other(already_stored(record))
        });
    }
    doc.candidates
        .iter()
        .find(|candidate| {
            candidate.state == super::store::CANDIDATE_PENDING
                && candidate.kind != "experience"
                // 交接候选的正文常为空（内容在 goal/done/todo/blocked 字段里），
                // 空正文的指纹全相等——按正文去重会把两份不同的交接草稿判成同一条。
                && candidate.kind != "handoff"
                && super::store::fingerprint(&candidate.body) == value
                && visible(
                    &candidate.scope,
                    &candidate.workspace_id,
                    &candidate.workspace_path,
                )
        })
        .map(|candidate| {
            Hit::Other(format!(
                "这条内容已经在待确认里了（「{}」）。请先在管理面板里处理掉它。",
                candidate.title
            ))
        })
}

fn already_stored(record: &super::store::MemoryRecord) -> String {
    format!(
        "这条内容已经在记忆里了（「{}」，状态 {}）。重复收它只会让同一件事出现两份。",
        record.title, record.state
    )
}

/// 同一正文已在库里、而这次又没有东西可并进去时的拒收原因（给用户看的一句话）。
///
/// **已接受但并不进去的记录同样拒收**：那一刻 [`merge_source`] 已经判过这次提交没有新东西
/// 可以记给它（来源没有来处、或来源列表已满、或这条来源本来就在列表里）。若此时放行，
/// 收进来的就是**第二条同正文的记录**——而那正是这道闸要挡的东西。判据只认指纹与状态，
/// 「这次来源算不算新」由 [`merge_source`] 那一半说了算，两半都过不了才拒收。
pub(crate) fn duplicate_reason(
    doc: &super::store::MemoryDoc,
    clean: &super::input::Clean,
) -> Option<String> {
    match find_hit(doc, clean) {
        Some(Hit::Other(reason)) => Some(reason),
        Some(Hit::Accepted(index)) => Some(already_stored(&doc.memories[index])),
        None => None,
    }
}

/// 把这次提交的来源并进**同正文且已接受**的那条记录，返回并入之后的那条。
///
/// §7.2「相同规范化正文可合并来源」：原先这里只有拒收，于是「这件事在会话 B 也说过」这条信息
/// 跟着第二次提交一起没了，而它正是用户想看到的东西——一条记忆带多个来源，面板与导出都列得出来。
/// 合并**不新建记录、不进待确认**，因此也不占用户的确认工作量。
pub(crate) fn merge_source(
    doc: &mut super::store::MemoryDoc,
    clean: &super::input::Clean,
    source: &super::store::MemorySource,
    now: &str,
) -> Option<super::store::MemoryRecord> {
    let Hit::Accepted(index) = find_hit(doc, clean)? else {
        return None;
    };
    let record = doc.memories.get_mut(index)?;
    // 并不进去（来源已满、或这条来源本来就在列表里）时返回 `None`，让提交方按「重复」拒收
    // ——**不静默截断来源列表**：那份列表一旦与实际发生的事对不上，它就不再可信了。
    if !crate::memory::sources::merge(&mut record.sources, source) {
        return None;
    }
    record.revision = record.revision.saturating_add(1);
    record.updated_at = now.to_string();
    Some(record.clone())
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
    saved: &super::store::MemoryRecord,
    now: &str,
) -> usize {
    let value = super::store::fingerprint(&saved.body);
    let mut superseded = 0;
    for candidate in doc.candidates.iter_mut() {
        if candidate.state == super::store::CANDIDATE_PENDING
            && candidate.kind != "experience"
            && candidate.kind != "handoff"
            && super::store::fingerprint(&candidate.body) == value
            // **同一份范围规则**（评审第 1 条）：A 项目保存一条，不该把 B 项目里
            // 同正文的待确认静默收掉——那条在 B 里是独立的一件事，而面板只列 pending，
            // 收掉之后用户在任何界面上都看不到它了。
            && in_scope(
                &candidate.scope,
                &candidate.workspace_id,
                &candidate.workspace_path,
                &saved.scope,
                &saved.workspace_id,
                &saved.workspace_path,
            )
        {
            candidate.state = super::store::CANDIDATE_REJECTED.to_string();
            candidate.decided_at = now.to_string();
            superseded += 1;
        }
    }
    superseded
}
