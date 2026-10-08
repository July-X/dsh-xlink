//! 技能接线：把壳的活动视图告诉内核（写进实例 `cordis.patch.yml` 的
//! loader 行）。2026-10-08 从 `kernel_adapter.rs` 原样拆出——那边在代码
//! 预算的反棘轮上（只许下调），P2 的桥接 env 注入需要腾出预算，而这块
//! 本就是自成一体的关注点（行为与测试一并搬迁，无任何语义变化）。

use std::path::Path;

// ─── 技能接线：把壳的活动视图告诉内核 ─────────────────────────────────────

/// 壳在实例 `cordis.patch.yml` 里插入的 loader 行 id。
///
/// **不能**复用内核自带的 `skill-filesystem` 行 id：`dsh-web-app` 明确把宿主层
/// 那一行 `disabled: true`（同文件注释写着「preset 拥有本地发现」），按 id 打
/// 补丁只会改到那个被禁用的行，等于什么都没接上。插入一条**自己的**行才是
/// 内核自己说的「deployment 级 provider」——它注册进全局层，所有 session 的
/// scope chain 都会读到。
pub(crate) const SKILL_WIRING_ROW_ID: &str = "xlink-skill-filesystem";

/// 活动视图路径在 patch 文件里的写法：YAML 双引号标量，而双引号标量的转义
/// 规则与 JSON 字符串**完全一致**，所以直接用 `serde_json` 编码。Windows 的
/// 家目录路径带大量 `\`，单引号写法虽然不转义反斜杠、却要在路径含 `'` 时手工
/// 加倍——交给 serde 就没有这类边角。
pub(crate) fn skill_wiring_path_token(active: &Path) -> String {
    serde_json::to_string(&active.to_string_lossy()).unwrap_or_else(|_| "\"\"".to_string())
}

/// 生成接线用的 patch 条目（一个 `insert`，把新行挂进实例的 loader 树）。
///
/// `includeDefaultRoots: false` 是关键：这一行只提供壳管理的活动视图，不再重复
/// 扫 `<DSH_HOME>/skills`、`<agentsHome>/skills`、项目根与打包根——那些是 preset
/// 自己的 `skill-filesystem` 行负责的（它们各自带 `customSkillDirs` 指向
/// agent-preset 包内的 skills）。`providerName` 也与 preset 的 `filesystem` 区分
/// 开：同一层里两个同名 provider 只会让后来者拿到一个空壳 disposer。
///
/// **不要**改用 `DSH_BUNDLED_SKILL_DIR`：它是内核唯一读的技能目录 env，但对应
/// 的根带 `trustedHost: true`——把社区技能标成「随应用打包的可信技能」是安全
/// 语义的错配。
fn skill_wiring_block(active: &Path) -> String {
    format!(
        "- insert:\n    - id: {SKILL_WIRING_ROW_ID}\n      name: '@deepseek-ai/dsh-skill-filesystem'\n      config:\n        providerName: xlink\n        includeDefaultRoots: false\n        customSkillDirs:\n          - {}\n",
        skill_wiring_path_token(active)
    )
}

/// 顶层是不是一个 patch 列表。只有列表（空、或以 `-` 起头的条目）才允许追加。
pub(crate) fn looks_like_patch_list(text: &str) -> bool {
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        return trimmed.starts_with('-');
    }
    true
}

/// 返回补上技能接线行之后的 patch 文本；已经就位时原样返回。
///
/// 三条纪律：
///
/// 1. **只追加，不改写**。patch 按 id 定位、**后写覆盖先写**，所以路径变了就
///    再追加一行新的，旧的自然失效；用户自己写的条目一律原样保留。
/// 2. **不碰看不懂的文件**。顶层不是列表时返回 `Err` 而不是硬写——patch 文件
///    是内核 fail-loud 的输入（历史上一次坏模板就让内核启动即崩），由壳把它
///    改成语法错误是最坏的结果。
/// 3. **空列表等价于什么都没有**。`[]` 结束了一个 YAML 文档，后面再跟块序列
///    是语法错误，所以只有空内容时才把头注释 + 接线行写成整份文件。
pub(crate) fn ensure_skill_wiring(existing: &str, active: &Path) -> Result<String, String> {
    let token = skill_wiring_path_token(active);
    if existing.contains(SKILL_WIRING_ROW_ID) && existing.contains(&token) {
        return Ok(existing.to_string());
    }
    if !looks_like_patch_list(existing) {
        return Err("cordis.patch.yml 顶层不是 patch 列表，不追加以免破坏内核启动".into());
    }
    let has_entries = existing.lines().any(|line| {
        let trimmed = line.trim();
        !trimmed.is_empty() && !trimmed.starts_with('#')
    });
    let mut next = existing.to_string();
    if has_entries {
        if !next.ends_with('\n') {
            next.push('\n');
        }
        next.push('\n');
    } else if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    next.push_str(&skill_wiring_block(active));
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// 活动视图路径随 `DSH_XLINK_HOME` 变化时必须补写新行（旧行因后写覆盖
    /// 先写而自动失效），而不是留着一条指向旧目录的死接线。
    #[test]
    fn ensure_skill_wiring_appends_a_new_row_for_a_moved_view() {
        let old_view = PathBuf::from("/xlink-a/skills/active");
        let new_view = PathBuf::from("/xlink-b/skills/active");
        let first = ensure_skill_wiring("# head\n", &old_view).unwrap();
        let second = ensure_skill_wiring(&first, &new_view).unwrap();
        assert!(second.contains(&skill_wiring_path_token(&old_view)));
        assert!(
            second
                .trim_end()
                .ends_with(skill_wiring_block(&new_view).trim_end()),
            "新行必须追加在最后——patch 是后写覆盖先写，实际：{second:?}"
        );
    }

    /// 顶层不是列表的文件一律不碰：patch 文件是内核 fail-loud 的输入，壳没有
    /// 资格把它改成另一种语法。
    #[test]
    fn ensure_skill_wiring_refuses_a_non_list_file() {
        let broken = "schema_version: 1\n";
        assert!(ensure_skill_wiring(broken, Path::new("/x/skills/active")).is_err());
    }

    /// 接线文本逐字节钉死：这份 YAML 要被内核的 loader 解析，格式变了就是
    /// 内核启动失败（历史上一次坏模板就 fatal 过），而"YAML 仍然合法"这件事
    /// Rust 侧没有任何工具能验，只能把期望值写死。
    #[test]
    fn skill_wiring_block_text_is_pinned() {
        let block = skill_wiring_block(Path::new("/xlink/skills/active"));
        assert_eq!(
            block,
            "- insert:\n    - id: xlink-skill-filesystem\n      name: '@deepseek-ai/dsh-skill-filesystem'\n      config:\n        providerName: xlink\n        includeDefaultRoots: false\n        customSkillDirs:\n          - \"/xlink/skills/active\"\n"
        );
    }

    /// Windows 家目录路径的 `\` 必须被转义，否则 YAML 双引号标量里它是转义
    /// 起始符，整份 patch 文件会解析成别的东西。
    #[test]
    fn skill_wiring_escapes_windows_paths() {
        let block = skill_wiring_block(Path::new(r"C:\Users\zxx\.dsh-xlink\skills\active"));
        assert!(
            block.contains(r#"- "C:\\Users\\zxx\\.dsh-xlink\\skills\\active""#),
            "路径必须按 JSON 规则转义，实际：{block:?}"
        );
    }
}
