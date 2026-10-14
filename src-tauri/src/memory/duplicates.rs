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

/// 同一正文是否已经在库里（权威记录或待确认候选），是就返回一句给用户的原因。
///
/// **用指纹而不是语义**：判「意思一样但写法不同」需要模型比对，那是 P2 的事（清单里的 C5）。
/// 首版把**精确重复**挡掉——它是模型跨会话反复提同一件事时最常见的形状，也是唯一能机械判的。
pub(crate) fn duplicate_reason(doc: &super::store::MemoryDoc, body: &str) -> Option<String> {
    let value = super::store::fingerprint(body);
    if let Some(record) = doc
        .memories
        .iter()
        .find(|record| super::store::fingerprint(&record.body) == value)
    {
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
