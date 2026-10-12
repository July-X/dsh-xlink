//! 实例 `cordis.patch.yml` 里内嵌插件自有接线行的精确增删。
//!
//! 纪律继承 `kernel_adapter` 的技能接线（2026-09-30 实测教训）：patch 是
//! 内核 fail-loud 的输入，顶层不是列表就不碰、失败要说得清。与技能接线
//! 的差异：技能行只追加（路径变了靠后写覆盖），本插件按设计 §3.1 需要
//! 「关闭开关 = 移除接线」，因此 [`remove_row`] 会精确摘掉**我们自己生成
//! 的形状**的行；认不出的结构（比如用户手工把行 id 抄进了别的块）返回
//! 错误而不是硬改——半截编辑比不编辑更危险。
//!
//! 生成形状（与 `plugins/openai-oauth` 冒烟测试一致，P0 调查 §5 实测）：
//!
//! ```text
//! - insert:
//!     - id: xlink-openai-oauth
//!       name: "<相对路径，JSON 转义>"   ← name 必须直指 host/index.js
//! ```
//!
//! **行 id 是参数**（2026-10-12，第二个内置插件进来时改的）：这一层过去把
//! `xlink-openai-oauth` 写死成模块常量，于是「摘行」会顺着**别人的** id 去删。
//! 形状识别（哪几行算我们写的）与 id 无关，只有「找哪一行」有关——所以形状
//! 逻辑一个字没改，改的只是每行往里传哪个 id。

use crate::kernel::skill_wiring::looks_like_patch_list;

/// `name` 字段的写法：YAML 双引号标量的转义规则与 JSON 字符串完全一致
/// （技能接线同一技巧），Windows 路径的反斜杠由 serde_json 统一处理。
fn name_token(entry_rel: &str) -> String {
    serde_json::to_string(entry_rel).unwrap_or_else(|_| "\"\"".into())
}

fn row_block(row_id: &str, entry_rel: &str) -> String {
    format!(
        "- insert:\n    - id: {row_id}\n      name: {}\n",
        name_token(entry_rel)
    )
}

/// 接线行是否已在（按行 id + 路径 token 双判据，与技能接线一致）。
pub(crate) fn row_present(text: &str, row_id: &str, entry_rel: &str) -> bool {
    text.contains(row_id) && text.contains(&name_token(entry_rel))
}

/// 追加自有接线行；已在（id + 当前路径）时原样返回 `Ok(None)`。
///
/// **只追加不改写**：目标路径变化时旧行仍在（后写覆盖先写），新行生效；
/// 旧行的清理走 [`remove_row`] 的事务顺序（先移除全部自有行，再追加新行）。
pub(crate) fn ensure_row(
    text: &str,
    row_id: &str,
    entry_rel: &str,
) -> Result<Option<String>, String> {
    if !looks_like_patch_list(text) {
        return Err(format!(
            "cordis.patch.yml 顶层不是 patch 列表，不追加 {row_id} 行以免破坏内核启动；请手工检查该文件后重试"
        ));
    }
    if row_present(text, row_id, entry_rel) {
        return Ok(None);
    }
    let has_entries = text
        .lines()
        .any(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'));
    let mut next = text.to_string();
    // 与技能接线同一套追加规则：有既有条目时先补行尾换行再空一行；
    // 只有头注释 / 空文件时不制造前导空行（见 kernel_adapter 的实测注释）。
    if has_entries {
        if !next.ends_with('\n') {
            next.push('\n');
        }
        next.push('\n');
    } else if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    next.push_str(&row_block(row_id, entry_rel));
    Ok(Some(next))
}

/// 摘除自有接线行（只认自己生成的形状）；不在时 `Ok(None)`。
///
/// 匹配面：4 空格缩进的列表行 `- id: <row_id>` 打头、其后 6 空格及更深的
/// 从属行，直到下一个 4 空格列表行或块结束。所在 `- insert:` 块只剩这一
/// 行时连块头一起摘（块头也是我们写的）；块里还有别的行时只摘自己。
pub(crate) fn remove_row(text: &str, row_id: &str) -> Result<Option<String>, String> {
    if !text.contains(row_id) {
        return Ok(None);
    }
    if !looks_like_patch_list(text) {
        return Err(format!(
            "cordis.patch.yml 顶层不是 patch 列表，不动 {row_id} 行以免破坏内核启动；请手工检查该文件"
        ));
    }
    let lines: Vec<&str> = text.split('\n').collect();
    let id_line = format!("    - id: {row_id}");
    let mut drop: Vec<usize> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if lines[i] == "- insert:" {
            let block_start = i;
            let mut rows: Vec<(usize, usize)> = Vec::new(); // (start, end_exclusive)
            let mut j = i + 1;
            while j < lines.len() && !lines[j].starts_with("- ") {
                if lines[j].starts_with("    - ") {
                    let start = j;
                    j += 1;
                    while j < lines.len()
                        && !lines[j].starts_with("    - ")
                        && !lines[j].starts_with("- ")
                        && (lines[j].starts_with("      ") || lines[j].trim().is_empty())
                    {
                        j += 1;
                    }
                    rows.push((start, j));
                } else {
                    j += 1;
                }
            }
            let ours: Vec<&(usize, usize)> =
                rows.iter().filter(|(s, _)| lines[*s] == id_line).collect();
            if rows.len() == 1 && ours.len() == 1 {
                // 整块都是我们的：连块头（及其前面的一个空行）一起摘。
                let mut start = block_start;
                if start > 0 && lines[start - 1].trim().is_empty() {
                    start -= 1;
                }
                drop.extend(start..j);
            } else if ours.len() == 1 {
                let (s, e) = *ours[0];
                drop.extend(s..e);
            }
            i = j;
        } else {
            i += 1;
        }
    }
    if drop.is_empty() {
        return Err(format!(
            "cordis.patch.yml 里出现了 {row_id} 字样但没找到可安全摘除的接线行；请手工检查该文件"
        ));
    }
    let drop_set: std::collections::HashSet<usize> = drop.into_iter().collect();
    let mut kept: Vec<&str> = lines
        .iter()
        .enumerate()
        .filter(|(idx, _)| !drop_set.contains(idx))
        .map(|(_, line)| *line)
        .collect();
    // 摘除后可能留下连续空行：只在末尾收敛一个换行，不动中间内容。
    while kept.len() > 1 && kept[kept.len() - 1].trim().is_empty() {
        kept.pop();
    }
    let mut next = kept.join("\n");
    if !next.ends_with('\n') && !next.is_empty() {
        next.push('\n');
    }
    Ok(Some(next))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROW_ID: &str = "xlink-openai-oauth";
    const REL: &str = "../../extensions/builtin/openai-oauth/0.1.0/fp-1/compat-a/host/index.js";
    /// 第二个内置插件的接线行 id 与相对入口。摘 openai 的行时它必须原样留下——
    /// 这正是「行 id 曾经是模块常量」时要防的事（摘行会顺着别人的 id 去删）。
    const OTHER_ROW: &str = "xlink-ponytail";
    const OTHER_REL: &str = "../../extensions/builtin/ponytail/0.1.0/fp-1/compat-a/host/index.js";

    #[test]
    fn ensure_appends_into_empty_and_existing_lists() {
        let empty = ensure_row("", ROW_ID, REL).unwrap().unwrap();
        assert!(empty.contains("- insert:"));
        assert!(empty.contains(&name_token(REL)));
        // 已在 → 不再追加。
        assert!(ensure_row(&empty, ROW_ID, REL).unwrap().is_none());
        // 有其他条目 → 前面留一个空行。
        let with_user = "# 用户注释\n- insert:\n    - id: user-row\n      name: 'x'\n".to_string();
        let next = ensure_row(&with_user, ROW_ID, REL).unwrap().unwrap();
        assert!(next.ends_with(&row_block(ROW_ID, REL)));
        assert!(next.contains("- id: user-row"));
    }

    #[test]
    fn ensure_refuses_non_list_top_level() {
        let err = ensure_row("mapping: true\n", ROW_ID, REL).unwrap_err();
        assert!(err.contains("不是 patch 列表"));
    }

    #[test]
    fn remove_drops_own_block_and_keeps_user_rows() {
        let wired = ensure_row(
            "# c\n- insert:\n    - id: user-row\n      name: 'x'\n",
            ROW_ID,
            REL,
        )
        .unwrap()
        .unwrap();
        let removed = remove_row(&wired, ROW_ID).unwrap().unwrap();
        assert!(!removed.contains(ROW_ID));
        assert!(removed.contains("- id: user-row"));
        // 再删 → 无变化。
        assert!(remove_row(&removed, ROW_ID).unwrap().is_none());
    }

    #[test]
    fn remove_drops_only_our_row_in_shared_block() {
        let shared = format!(
            "- insert:\n    - id: user-row\n      name: 'x'\n    - id: {ROW_ID}\n      name: {}\n",
            name_token(REL)
        );
        let removed = remove_row(&shared, ROW_ID).unwrap().unwrap();
        assert!(removed.contains("- id: user-row"));
        assert!(!removed.contains(ROW_ID));
        assert!(removed.contains("- insert:"));
    }

    #[test]
    fn remove_refuses_unknown_shape() {
        // 行 id 出现在 2 空格缩进（不是我们的形状）→ 拒绝而不是硬改。
        let odd = format!("- insert:\n  - id: {ROW_ID}\n");
        assert!(remove_row(&odd, ROW_ID).is_err());
        // 空文件与没有行 id 的文件都是 no-op。
        assert!(remove_row("", ROW_ID).unwrap().is_none());
        assert!(remove_row("- insert:\n    - id: other\n", ROW_ID)
            .unwrap()
            .is_none());
    }

    #[test]
    fn roundtrip_keeps_user_content_byte_for_byte() {
        let user = "# 头注释\n- insert:\n    - id: a\n      name: 'a'\n\n- insert:\n    - id: b\n      name: 'b'\n";
        let wired = ensure_row(user, ROW_ID, REL).unwrap().unwrap();
        let removed = remove_row(&wired, ROW_ID).unwrap().unwrap();
        assert_eq!(removed, user);
    }

    /// 两个内置插件同时接线时，各开关只动自己那一行。这是「行 id 从模块常量
    /// 变成参数」那一步的反向验：**没有它的话，把 `remove_row` 的 row_id
    /// 传错一个值仍然全绿**，而真机上表现为「关掉 A 把 B 也关了」。
    #[test]
    fn two_builtins_wired_together_survive_each_others_toggle() {
        let user = "# c\n- insert:\n    - id: user-row\n      name: 'x'\n";
        let first = ensure_row(user, ROW_ID, REL).unwrap().unwrap();
        let both = ensure_row(&first, OTHER_ROW, OTHER_REL).unwrap().unwrap();
        assert!(both.contains(ROW_ID) && both.contains(OTHER_ROW));

        let after = remove_row(&both, ROW_ID).unwrap().unwrap();
        assert!(!after.contains(ROW_ID), "openai 的行应当被摘掉");
        assert!(after.contains(OTHER_ROW), "ponytail 的行必须原样留下");
        assert!(after.contains("- id: user-row"));
        // 接回去之后两行仍在、路径各自正确。**不比逐字节**：摘行会连同一个
        // 空行一起摘掉，重新追加时空行落在别处——语义相同而字节不同，
        // 拿字节等当判据会让一次无害的空行调整变成红线。
        let back = ensure_row(&after, ROW_ID, REL).unwrap().unwrap();
        assert_eq!(
            back.matches("- insert:").count(),
            both.matches("- insert:").count()
        );
        assert!(back.contains(OTHER_ROW) && back.contains(OTHER_REL));
        assert!(back.contains(ROW_ID) && back.contains(REL));
    }
}
