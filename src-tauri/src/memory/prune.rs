//! 候选表的**裁剪**：到顶之后只丢已决的那些。
//!
//! ## 为什么从 `store.rs` 拆出来
//!
//! `store.rs` 的生产代码贴着反棘轮线（基线预算 665，基线 ≥ 600 的文件**只许下调**），而
//! 2026-10-16 那一批要往候选上**加**第二个标记字段（C6 的 `supersedes`）。按本仓的规矩，
//! 被预算拦下时的正确反应是**把新逻辑拆出去**，不是调数字——而这段裁剪本来就与「写入
//! 事务」不是一件事：它不判断能不能写，只在写完之后维持文档的规模上限。
//!
//! ## 为什么只丢已决的
//!
//! 候选表到顶（[`MAX_CANDIDATES`]）之后，文档会一直变大，而**每次读写都要重写整份文档**
//! ——放任它涨下去，报出来的「库有多大」也跟着失真。丢的时候只丢已决的那些：
//!
//! · **待确认的一条都不丢**：那是用户还没看见的东西。静默丢掉等于用户提过的东西凭空
//!   消失，而他连它存在过都不知道——这与「删除要留抑制指纹、面板要说明影响范围」那条
//!   纪律是同一个方向（§7.3）。
//! · 已决的要么已经变成一条权威记录，要么被用户明确否掉，结论已经落地，留着只是历史。
//!
//! 裁掉的是**候选**，而面板本来只列 pending，所以被裁的已决候选在界面上不出现——它是文件里
//! 的历史，不是用户正在看的东西。这条形状是既有的（`MAX_PENDING_CANDIDATES` 那道闸也只管
//! pending），不是这一轮引入的。

use super::store::{MemoryDoc, CANDIDATE_PENDING};

/// 候选表的上限。
///
/// 取 200 的理由与 `MAX_PENDING_CANDIDATES` 不同：后者卡的是**用户的确认工作量**（满了他
/// 就做不过来了，于是拒收新的）；这里卡的是**文档规模**（读写都重写整份文档），而被丢的
/// 已决候选不再增加任何工作量。
pub const MAX_CANDIDATES: usize = 200;

/// 候选到顶之后裁掉已决的那些，直到回到上限内。
///
/// **从最老的已决那条开始丢**：丢最近的那条会让用户刚点过的保存/拒绝消失在一次无关的
/// 提取之后——那是「我的操作被别的事波及」那种没法解释的消失。
pub(crate) fn trim_decided_candidates(doc: &mut MemoryDoc) {
    let mut decided: Vec<usize> = (0..doc.candidates.len())
        .filter(|index| doc.candidates[*index].state != CANDIDATE_PENDING)
        .collect();
    decided.sort_by(|left, right| {
        doc.candidates[*left]
            .decided_at
            .cmp(&doc.candidates[*right].decided_at)
    });
    while doc.candidates.len() > MAX_CANDIDATES && !decided.is_empty() {
        let index = decided.remove(0);
        doc.candidates.remove(index);
        for remaining in decided.iter_mut() {
            *remaining -= 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::store::{MemoryCandidate, CANDIDATE_REJECTED};

    fn candidate(id: &str, state: &str, decided_at: &str) -> MemoryCandidate {
        MemoryCandidate {
            id: id.to_string(),
            state: state.to_string(),
            decided_at: decided_at.to_string(),
            ..Default::default()
        }
    }

    /// **待确认的一条都不能丢**——那是用户还没看见的东西，静默丢掉等于用户提过的东西凭空
    /// 消失，而他连它存在过都不知道。
    ///
    /// 反向验：把筛选里的 `!=` 改成 `==`，本条立刻红，而那时**被裁掉的恰好就是待确认的
    /// 那些**——判据要红在被破坏的那条纪律上，不是「数量不对」。
    #[test]
    fn never_drops_a_pending_candidate() {
        let mut doc = MemoryDoc::default();
        for index in 0..MAX_CANDIDATES + 5 {
            doc.candidates.push(candidate(
                &format!("pending-{index}"),
                CANDIDATE_PENDING,
                "",
            ));
        }
        trim_decided_candidates(&mut doc);
        assert_eq!(
            doc.candidates.len(),
            MAX_CANDIDATES + 5,
            "全库都是待确认时一条都不该被裁掉"
        );
        assert!(doc
            .candidates
            .iter()
            .all(|candidate| candidate.state == CANDIDATE_PENDING));
    }

    /// 到了上限就裁已决的，而且**从最老的已决那条开始**。
    ///
    /// 为什么按 `decided_at` 排而不是按列表位置：丢最近的那条会让用户刚点过的保存/拒绝
    /// 消失在一次无关的提取之后——「我的操作被别的事波及」那种没法解释的消失。
    #[test]
    fn drops_the_oldest_decided_first() {
        let mut doc = MemoryDoc::default();
        // 待确认那条放中间：它不该因为「排在已决前面」而被当成裁剪对象。
        doc.candidates
            .push(candidate("pending", CANDIDATE_PENDING, ""));
        doc.candidates
            .push(candidate("oldest", CANDIDATE_REJECTED, "2026-10-01"));
        doc.candidates
            .push(candidate("newer", CANDIDATE_REJECTED, "2026-10-02"));
        while doc.candidates.len() <= MAX_CANDIDATES {
            doc.candidates
                .push(candidate("filler", CANDIDATE_REJECTED, "2026-10-09"));
        }
        trim_decided_candidates(&mut doc);
        let ids: Vec<&str> = doc
            .candidates
            .iter()
            .map(|candidate| candidate.id.as_str())
            .collect();
        assert_eq!(
            doc.candidates.len(),
            MAX_CANDIDATES,
            "裁到上限就该停手，不多删也不少删"
        );
        assert!(
            !ids.contains(&"oldest"),
            "最老的那条已决候选该先走，留下的：{ids:?}"
        );
        assert!(
            ids.contains(&"pending") && ids.contains(&"newer"),
            "待确认的与较新的已决候选都该留着：{ids:?}"
        );
    }
}
