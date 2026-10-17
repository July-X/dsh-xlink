//! 实例 `cordis.patch.yml` 的公共写入骨架：壳自己管理的每一行都按同一套纪律追加。
//!
//! 两处调用——技能接线（[`crate::kernel::skill_wiring`]）与内置插件接线
//! （`plugins::builtin::wiring`）——此前各抄一份「顶层是不是列表 + 有既有条目时先补空行」
//! 的簿记。同一形状抄两遍，改一处就会漂（内置插件那份的注释里甚至写着「与技能接线同一套
//! 追加规则」，靠人记）。这里只收**与写什么行无关**的那一半；「这一行在不在」的判据各不
//! 相同（行 id + 路径 token / 行 id + 相对路径），留在各自的调用方。
//!
//! **够不到 preset 里的行**：patch 的索引只递归 `entry.group && Array.isArray(entry.config)`
//! （`dsh-app-boot` 的 `applyEntryPatches`），预设 `config.plugins` 里的行不在其中——2026-10-17
//! 想用它调压缩参数时实测过（内核直接警告 `patch: entry "…" not found`），见
//! `docs/features/memory/cache-impact.md`。
//!
//! 为什么这些行都写同一个文件：内核的 loader patch 是「按 id 定位、后写覆盖先写」的
//! 顶层列表，壳的每一条接线都是往这份列表末尾追加一条 `- insert:` 块。文件是内核
//! **fail-loud** 的输入（历史上一次坏模板就让内核启动即崩），所以判据只能是「看不懂
//! 就不碰」。

/// 顶层是不是一个 patch 列表，**且是单文档**。只有列表（空、或以 `-` 起头的条目）才允许追加。
///
/// 「单文档」这一条是 2026-10-17 对抗性复核发现的既有隐患：YAML 的 `...` 结束当前文档、
/// `---` 另起一个，而在这类文件后面追加块序列会造出「一个流里两个文档」——内核
/// `loadOptionalPatches` 对此是 **fail-loud 抛错**（复核实测：追加后 js-yaml 报
/// `expected a single document in the stream, but found more`），也就是工作台直接起不来。
/// 旧实现三处各抄一份、都不查这个；提到共享层之后一处修好三处。
pub(crate) fn looks_like_patch_list(text: &str) -> bool {
    // **文档标记只在行首算数**（YAML 就是这么规定的）：块标量里缩进着的一行 `...` 是普通
    // 内容，把它当结束标记会让我们白白拒绝一个本来能安全追加的文件。
    let mut starts = 0usize;
    let mut first_meaningful: Option<&str> = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        // 文档结束标记：它之后的一切都是另一个文档，往后追加必然造出双文档。
        if line.starts_with("...") && line[3..].trim().is_empty() {
            return false;
        }
        // 文档开始标记：允许一个（文件开头的 `---`），两个以上就是两个文档。
        if line.starts_with("---") && line[3..].trim().is_empty() {
            starts += 1;
            if starts > 1 {
                return false;
            }
            continue;
        }
        if first_meaningful.is_none() {
            first_meaningful = Some(trimmed);
        }
    }
    first_meaningful.is_none_or(|line| line.starts_with('-'))
}

/// 把一段 `- insert:` 块追加到 patch 文本末尾，返回新文本。
///
/// **调用方必须先过一次 [`looks_like_patch_list`]**：这里不做拒绝，只做「有既有条目时
/// 先补行尾换行再空一行；只有头注释 / 空文件时不制造前导空行」这一件事。
pub(crate) fn append_row(text: &str, block: &str) -> String {
    let has_entries = text
        .lines()
        .any(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'));
    let mut next = text.to_string();
    if has_entries {
        if !next.ends_with('\n') {
            next.push('\n');
        }
        next.push('\n');
    } else if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    next.push_str(block);
    next
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 头注释与空行不算条目：只有它们时不该制造前导空行（否则文件以空行开头）。
    #[test]
    fn append_row_does_not_lead_with_a_blank_line() {
        assert_eq!(append_row("# head\n", "- a\n"), "# head\n- a\n");
        assert_eq!(append_row("", "- a\n"), "- a\n");
        assert_eq!(append_row("# head", "- a\n"), "# head\n- a\n");
    }

    /// 有既有条目时先补行尾换行、再空一行，两次追加互不粘连。
    #[test]
    fn append_row_separates_two_blocks() {
        let once = append_row("# head\n", "- a\n");
        let twice = append_row(&once, "- b\n");
        assert_eq!(twice, "# head\n- a\n\n- b\n");
    }

    /// 拒绝判据只看第一个非空、非注释行是不是以 `-` 起头。
    #[test]
    fn patch_list_detection_ignores_comments_and_blanks() {
        assert!(looks_like_patch_list(""));
        assert!(looks_like_patch_list("# c\n\n"));
        assert!(looks_like_patch_list("# c\n- insert:\n"));
        assert!(!looks_like_patch_list("schema_version: 1\n"));
        assert!(!looks_like_patch_list("# c\nschema_version: 1\n"));
    }

    /// **多文档一律不碰**：往 `...` 或第二个 `---` 之后追加会让内核的 patch 加载抛错
    /// （复核实测 `expected a single document in the stream, but found more`），那是
    /// 「壳把用户文件改成内核启动失败」——比不做这件事坏得多。
    #[test]
    fn patch_list_detection_rejects_multi_document_streams() {
        assert!(
            !looks_like_patch_list("- a\n...\n"),
            "文档结束标记之后不能再追加"
        );
        assert!(
            !looks_like_patch_list("...\n"),
            "只有结束标记也属于已结束的文档"
        );
        assert!(
            !looks_like_patch_list("---\n- a\n---\n- b\n"),
            "第二个文档开始标记：已是双文档"
        );
        assert!(
            looks_like_patch_list("---\n- a\n"),
            "单个文档开始标记是合法的"
        );
        assert!(looks_like_patch_list("# c\n---\n- a\n"));
        // 缩进着的 `...` 是块标量里的普通内容，不是文档结束标记：不能因此拒绝追加。
        assert!(looks_like_patch_list("- a\n  ...\n"));
    }
}
