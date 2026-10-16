//! 候选的**冲突标记**（C5，设计 §7-2「意义冲突的记忆标『待处理冲突』」）。
//!
//! ## 这一半为什么独立成模块
//!
//! 它是**判据 + 限额**，不是落盘事务：`store.rs` 已经在反棘轮线上（生产代码 661 行 / 预算
//! 665，基线 ≥ 600 的文件**只许下调**），而这里要加的是一组形状判断与上限。按本仓的规矩，
//! 被预算拦下时的正确反应是**把新逻辑拆出去**，不是调数字——所以规范化与限额都在这里，
//! `store.rs` 那边只留字段与一行调用。
//!
//! ## 两条不许破的纪律
//!
//! 1. **模型只标记，不裁决**（§7-2）。这里不判断两条记忆是不是真的矛盾——那是模型的事，
//!    而且它只能**标记**；胜者由用户在面板里自己删一条（`ui/src/memory/MemoryConflict.vue`）。
//!    于是这一层只剩「形状对不对、别把库撑爆」。
//! 2. **不许静默截断**。超限的、被丢掉的都进 [`Normalized::dropped`]，由调用方带回给
//!    调用者（面板与日志都看得见）；丢掉一条冲突标记而不说，用户看到的就是「模型说没冲突」。
//!
//! ## 为什么不校验「这条 id 真的存在」
//!
//! 模型拿到的冲突集合是壳自己发出去的那次检索结果（插件侧逐项校验过，见 `extract.js`），
//! 而在那之后到提交之间，那条记忆可能已经被用户删了。此刻去壳里查一遍并不能让面板更准，
//! 只会制造一个新分支：**该显示成什么**——查不到就报「找不到」还是照样并排？面板本来就
//! 要处理「引用的那条已删除/不在读范围」这两种情况（见 `MemoryConflict.vue`），所以这里
//! 保持只收形状校验：多存一个已经消失的 id，面板如实说它不在了，比在写入路径上多一个
//! 会把人挡在门外的拒绝更好。

/// 一条候选最多标几条冲突。
///
/// 取 4 是因为**并排展示**才是这件事的解法（§7-2）：再多就不是「让人删一条」而是
/// 「让人读一篇对照报告」了，而那正是用户自己都会放弃的形状。
pub const MAX_CONFLICTS_PER_CANDIDATE: usize = 4;

/// 单个冲突 id 的字节上限。
///
/// 记忆 id 由 [`crate::memory::store`] 的 `new_id()` 生成（32 位十六进制），64 字节给足
/// 余量；这一条同时挡住**模型编出来的长串**——那类值长度上没有一个可信的上限形状，
/// 而它们会被原样写进文档。
pub const MAX_CONFLICT_ID_BYTES: usize = 64;

/// 规范化之后的结果：**留下的**与**被丢掉的**。
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Normalized {
    pub kept: Vec<String>,
    /// 被丢掉的原值（按原样带回，好让用户看见「模型标的这条没被收下」）。
    pub dropped: Vec<String>,
}

/// 规范化一组冲突标记：去空白、去重（**保序**）、限条数、限长度。
///
/// **保序而不是排序**：模型标的顺序本身带信息（它先看到的那条冲突更重要），而排序会让
/// 「同一次提取的两次运行标出不同的顺序」，那种不稳定在面板上看起来像偶发。
pub fn normalize(raw: &[String]) -> Normalized {
    let mut kept: Vec<String> = Vec::new();
    let mut dropped: Vec<String> = Vec::new();
    for value in raw {
        let id = value.trim();
        if id.is_empty() {
            continue;
        }
        if id.len() > MAX_CONFLICT_ID_BYTES {
            dropped.push(value.clone());
            continue;
        }
        if kept.iter().any(|existing| existing == id) {
            continue;
        }
        if kept.len() >= MAX_CONFLICTS_PER_CANDIDATE {
            dropped.push(value.clone());
            continue;
        }
        kept.push(id.to_string());
    }
    Normalized { kept, dropped }
}

/// 面板要显示的文案：这条候选与既有记忆有冲突，**由用户裁决**。
///
/// 单独一个函数而不是把这句话写在面板里，是因为这句话同时出现在候选卡片与导出快照里，
/// 而两处各写一遍就会漂（面板改了、导出没改，导出的文件就在对人撒谎）。
pub fn note_label(count: usize) -> String {
    format!("与既有 {count} 条记忆意义冲突：它们说的不是同一件事，或者说的相反。由你决定留哪一条。")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn empty_is_the_common_case_and_stays_empty() {
        assert_eq!(normalize(&[]), Normalized::default());
        // 绝大多数候选根本没有冲突：这条不许被「顺带」塞点什么。
        assert_eq!(normalize(&ids(&["  ", ""])).kept, Vec::<String>::new());
    }

    #[test]
    fn dedupes_while_keeping_the_original_order() {
        // 顺序带信息（模型先标的那条更重要），排序会让两次运行的展示不一样。
        let result = normalize(&ids(&["c", "a", "b", "a"]));
        assert_eq!(result.kept, ids(&["c", "a", "b"]));
        assert!(result.dropped.is_empty());
    }

    #[test]
    fn caps_the_count_and_reports_what_it_dropped() {
        let result = normalize(&ids(&["a", "b", "c", "d", "e", "f"]));
        assert_eq!(result.kept, ids(&["a", "b", "c", "d"]));
        // **不许静默截断**：多出来的两条要能报回去，否则面板上「模型标了 6 条」就变成
        // 「只显示 4 条」而用户不知道为什么。
        assert_eq!(result.dropped, ids(&["e", "f"]));
    }

    #[test]
    fn refuses_an_over_long_id_because_that_shape_has_no_id_in_it() {
        let long = "x".repeat(MAX_CONFLICT_ID_BYTES + 1);
        let result = normalize(&ids(&["ok", &long]));
        assert_eq!(result.kept, ids(&["ok"]));
        assert_eq!(result.dropped, ids(&[&long]));
        // 恰好等于上限的留着：判据是「不许超」，不是「接近上限就疑心」。
        let exact = "x".repeat(MAX_CONFLICT_ID_BYTES);
        assert_eq!(normalize(&ids(&[&exact])).kept, ids(&[&exact]));
    }

    #[test]
    fn label_says_the_user_decides() {
        // 面板与导出共用这一句；文案里出现「由你决定」是这条纪律的可测形状——
        // 「已自动保留新的一条」那种话会让用户以为系统替他裁过。
        assert!(note_label(1).contains("由你决定留哪一条"));
        assert!(note_label(3).contains("3 条"));
    }
}
