//! 候选的**两个标记**的形状与限额：`conflicts_with`（C5 意义冲突）与 `supersedes`
//! （C6 用户纠正优先于旧候选）。两者的规范化是同一段代码，**限额各是各的**。
//!
//! ## 这一块为什么独立成模块
//!
//! 它是**判据 + 限额**，不是落盘事务：`store.rs` 在反棘轮线上（基线预算 665，基线 ≥ 600
//! 的文件**只许下调**），而这里要加的是一组形状判断与上限。按本仓的规矩，被预算拦下时的
//! 正确反应是**把新逻辑拆出去**，不是调数字——所以规范化与限额都在这里，`store.rs` 那边
//! 只留字段与两行调用。
//!
//! 2026-10-16 起这个模块从「只管冲突」扩成「管两个标记」：规范化逻辑本就是同一段，
//! 而两条纪律（模型只标记不执行、标记错误只丢标记不丢候选）对两者**完全一致**——分成两个
//! 模块就要把同一句话抄两遍，而抄出来的那份迟早会漂。
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

/// 一条候选最多说它取代了几条待确认候选。
///
/// **与冲突那 4 条不是一个理由**，所以不是一个常量：标冲突是对**已生效**的记忆，用户要
/// 逐条并排看；而标取代是对**还没生效**的候选，面板上只出现一句「你说这句时推翻了先前那条」。
/// 这里的形状问题是「模型把整段对话都读成了改口」——一段对话里连续推翻四次以上，通常
/// 意味着它提取错了，而不是用户真的改了四次口。那种情况下多标的几条不会给用户任何信息，
/// 只会把那句话变成一长串 id。
pub const MAX_SUPERSEDES_PER_CANDIDATE: usize = 4;

/// 单个标记 id 的字节上限。
///
/// 记忆 id 与候选 id 都由 [`crate::memory::store`] 的 `new_id()` 生成（32 位十六进制），
/// 64 字节给足余量；这一条同时挡住**模型编出来的长串**——那类值长度上没有一个可信的上限
/// 形状，而它们会被原样写进文档。
pub const MAX_MARK_ID_BYTES: usize = 64;

/// 规范化之后的结果：**留下的**与**被丢掉的**。
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Normalized {
    pub kept: Vec<String>,
    /// 被丢掉的原值（按原样带回，好让用户看见「模型标的这条没被收下」）。
    pub dropped: Vec<String>,
}

/// 规范化一组标记：去空白、去重（**保序**）、限条数（`max_count`）、限长度。
///
/// **保序而不是排序**：模型标的顺序本身带信息（它先看到的那条更重要），而排序会让
/// 「同一次提取的两次运行标出不同的顺序」，那种不稳定在面板上看起来像偶发。
pub fn normalize(raw: &[String], max_count: usize) -> Normalized {
    let mut kept: Vec<String> = Vec::new();
    let mut dropped: Vec<String> = Vec::new();
    for value in raw {
        let id = value.trim();
        if id.is_empty() {
            continue;
        }
        if id.len() > MAX_MARK_ID_BYTES {
            dropped.push(value.clone());
            continue;
        }
        if kept.iter().any(|existing| existing == id) {
            continue;
        }
        if kept.len() >= max_count {
            dropped.push(value.clone());
            continue;
        }
        kept.push(id.to_string());
    }
    Normalized { kept, dropped }
}

/// 一次算两个标记——**两个标记的规范化在这里只有一个入口**。
///
/// 写成两次调用的话，`submit_candidate` 那边就是两段几乎一样的代码，而将来再加第三个标记
/// 时，第三段是「照着前两段抄」的概率极高。限额各是各的（见两个常量上的理由），它们只是
/// 碰巧现在相等。
pub fn from_input(conflicts: &[String], supersedes: &[String]) -> Marked {
    Marked {
        conflicts: normalize(conflicts, MAX_CONFLICTS_PER_CANDIDATE).kept,
        supersedes: normalize(supersedes, MAX_SUPERSEDES_PER_CANDIDATE).kept,
    }
}

/// 两个标记规范化之后的结果（[`from_input`] 的产物）。
#[derive(Debug, Default)]
pub struct Marked {
    pub conflicts: Vec<String>,
    pub supersedes: Vec<String>,
}

/// **导出**里那句话：这条候选与既有记忆有冲突，**由用户裁决**。
///
/// **面板不调它**（2026-10-16 更正一句此前写错的注释）：面板那份在
/// `ui/src/memory/MemoryConflict.vue` 里，是组件自己写的——面板拿不到这个 Rust 函数，
/// 除非为一个句子加一条 Tauri 命令，那不划算。于是**两处各有一份文案**，靠各自的判据
/// 钉住不许漂：导出这条由本函数的用例钉住「由用户决定」，面板那条由
/// `ui/test/memoryPanel.test.js` 的「冲突块的文案只摆事实」钉住同一句。
/// 之前这里写着「同时出现在候选卡片与导出快照里」，**那是注释与实现不符**：面板从来没
/// 调过它。与其改面板去调一个跨语言不到的函数，不如把这句注释改成实情。
pub fn note_label(count: usize) -> String {
    format!("与既有 {count} 条记忆意义冲突：它们说的不是同一件事，或者说的相反。由你决定留哪一条。")
}

/// **导出**里那句话：这条候选**取代**了先前那几条待确认的（面板那份在
/// `MemorySuperseded.vue` 里，理由同 [`note_label`] 那段注释）。
///
/// **不许出现「已自动作废前面的」这类话**：壳不会执行取代（C6 的纪律一），面板给的是
/// 「仍要保存两条 / 丢弃先前那几条」两个动作，由用户按一次。这句话因此必须写清「还没作废」。
pub fn superseded_note(count: usize) -> String {
    format!(
        "你说这句时推翻了先前提出的 {count} 条。它们还在「待确认」里，\
由你决定丢弃哪几条——**系统没有替你作废任何一条**。"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn empty_is_the_common_case_and_stays_empty() {
        assert_eq!(
            normalize(&[], MAX_CONFLICTS_PER_CANDIDATE),
            Normalized::default()
        );
        // 绝大多数候选根本没有冲突：这条不许被「顺带」塞点什么。
        assert_eq!(
            normalize(&ids(&["  ", ""]), MAX_CONFLICTS_PER_CANDIDATE).kept,
            Vec::<String>::new()
        );
    }

    #[test]
    fn dedupes_while_keeping_the_original_order() {
        // 顺序带信息（模型先标的那条更重要），排序会让两次运行的展示不一样。
        let result = normalize(&ids(&["c", "a", "b", "a"]), MAX_CONFLICTS_PER_CANDIDATE);
        assert_eq!(result.kept, ids(&["c", "a", "b"]));
        assert!(result.dropped.is_empty());
    }

    #[test]
    fn caps_the_count_and_reports_what_it_dropped() {
        // 限额**由调用方给**：冲突与取代是两个数，理由写在各自的常量上。
        let result = normalize(
            &ids(&["a", "b", "c", "d", "e", "f"]),
            MAX_CONFLICTS_PER_CANDIDATE,
        );
        assert_eq!(result.kept, ids(&["a", "b", "c", "d"]));
        // **不许静默截断**：多出来的两条要能报回去，否则面板上「模型标了 6 条」就变成
        // 「只显示 4 条」而用户不知道为什么。
        assert_eq!(result.dropped, ids(&["e", "f"]));
    }

    #[test]
    fn refuses_an_over_long_id_because_that_shape_has_no_id_in_it() {
        let long = "x".repeat(MAX_MARK_ID_BYTES + 1);
        let result = normalize(&ids(&["ok", &long]), MAX_CONFLICTS_PER_CANDIDATE);
        assert_eq!(result.kept, ids(&["ok"]));
        assert_eq!(result.dropped, ids(&[&long]));
        // 恰好等于上限的留着：判据是「不许超」，不是「接近上限就疑心」。
        let exact = "x".repeat(MAX_MARK_ID_BYTES);
        assert_eq!(
            normalize(&ids(&[&exact]), MAX_CONFLICTS_PER_CANDIDATE).kept,
            ids(&[&exact])
        );
    }

    /// **两个标记的限额各是各的**——用同一个数只是碰巧相等，理由写在各自的常量注释里。
    /// 本条钉住的是「限额真的由参数说了算」：传 1 就只留 1 条，且多的那条要报回去。
    #[test]
    fn the_cap_comes_from_the_caller() {
        let result = normalize(&ids(&["a", "b", "c"]), 1);
        assert_eq!(result.kept, ids(&["a"]));
        assert_eq!(result.dropped, ids(&["b", "c"]));
    }

    #[test]
    fn label_says_the_user_decides() {
        // 导出这一侧的形状；文案里出现「由你决定」是这条纪律的可测形状——
        // 「已自动保留新的一条」那种话会让用户以为系统替他裁过。
        assert!(note_label(1).contains("由你决定留哪一条"));
        assert!(note_label(3).contains("3 条"));
    }
}
