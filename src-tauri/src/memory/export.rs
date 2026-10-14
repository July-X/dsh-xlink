//! 记忆的 Markdown 快照导出。
//!
//! 设计 §2 的那一条：需要带走或人工审阅时导出一次，**一次性快照**——不参与运行时、
//! 没有代次、手工修改不反向入库（盘上那份文件与权威记录的唯一关系是「它是某一次
//! 导出的样子」）。落点是 `<memory>/exports/memory-<时间戳>.md`，0600。
//!
//! 为什么独立成文件：它只做「把一份列表视图渲染成文本」，与 store 的读写纪律
//! （CAS / 指纹 / 原子替换）无关；塞进 store.rs 会让那份文件多出一个与它无关的
//! 关注点，而它已经贴着预算上限。
//!
//! 三条自律：
//! · **导出的是权威记录的那一份**（`store::list` 的视图），不是「按当前查询会注入
//!   的那几条」——后者是运行时视图，拿它当快照会漏掉停用项与候选；
//! · **文档读不出来时照实说**（status 的 warning 原样进文件头），绝不导出一份
//!   看起来正常、其实是空库的快照；
//! · **不覆盖已有文件**（同一秒内连续导出加后缀），导出是用户的动作，不是幂等写。

use crate::memory::store::{
    self, ListFilter, MemoryCandidate, MemoryListView, MemoryRecord, Namespace,
};
use crate::shell::error::AppError;
use crate::shell::paths;

/// 导出结果：面板拿它显示路径与条数。
#[derive(Debug, Clone, serde::Serialize)]
pub struct ExportResult {
    pub path: String,
    pub bytes: u64,
    pub memories: usize,
    pub candidates: usize,
    pub generated_at: String,
}

/// 导出一次快照。
pub fn export(ns: &Namespace) -> Result<ExportResult, AppError> {
    let view = store::list(
        ns,
        &ListFilter {
            scope: String::new(),
            include_candidates: true,
            include_disabled: true,
        },
    );
    let generated_at = store::now_iso();
    let text = render(&view, &generated_at, &ns.key());
    let dir = paths::instance_memory_dir(&ns.family, &ns.instance_id, &ns.profile).join("exports");
    std::fs::create_dir_all(&dir).map_err(|error| {
        AppError::Memory(format!(
            "无法创建导出目录 {}：{error}。请检查该目录的写入权限后重试。",
            dir.display()
        ))
    })?;
    let path = unique_path(&dir, &generated_at);
    crate::shell::atomic_write::atomic_write_private(&path, text.as_bytes()).map_err(|error| {
        AppError::Memory(format!(
            "写入导出文件失败（{}）：{error}。磁盘是否已满？也可以先删除 exports/ 下的旧文件再试。",
            path.display()
        ))
    })?;
    Ok(ExportResult {
        path: path.to_string_lossy().into_owned(),
        bytes: text.len() as u64,
        memories: view.memories.len(),
        candidates: view.candidates.len(),
        generated_at,
    })
}

/// `memory-20261014T004629Z.md`；同一秒里再导一次就加 `-2`。
///
/// 时间戳取 RFC3339 去掉 `-` 与 `:`（冒号在 Windows 上是非法文件名字符，
/// 而这份文件是要给用户拿走的）。
fn unique_path(dir: &std::path::Path, generated_at: &str) -> std::path::PathBuf {
    let stamp = generated_at.replace(['-', ':'], "");
    let base = format!("memory-{stamp}");
    let first = dir.join(format!("{base}.md"));
    if !first.exists() {
        return first;
    }
    for index in 2..1000 {
        let candidate = dir.join(format!("{base}-{index}.md"));
        if !candidate.exists() {
            return candidate;
        }
    }
    dir.join(format!(
        "{base}-{}.md",
        store::now_iso().replace(['-', ':'], "")
    ))
}

/// 渲染成 Markdown。
fn render(view: &MemoryListView, generated_at: &str, namespace: &str) -> String {
    let status = &view.status;
    let mut out = String::new();
    out.push_str("# 本地记忆快照\n\n");
    out.push_str(&format!("- 导出时间：{generated_at}\n"));
    out.push_str(&format!("- 命名空间：{namespace}\n"));
    out.push_str(&format!(
        "- 数据代次：{}（每次成功写入 +1）\n",
        status.generation
    ));
    out.push_str(&format!(
        "- 开关：使用已批准的记忆 = {}；自动提取 = {}（P2，尚未实现）\n",
        if status.use_approved { "开" } else { "关" },
        if status.auto_extract { "开" } else { "关" }
    ));
    out.push_str(&format!("- 落盘位置：{}\n", status.file));
    if let Some(warning) = &status.warning {
        out.push_str(&format!(
            "\n> ⚠ 权威记录这次**没有读出来**：{warning}\n> 下面是空列表，不代表你没有记忆。\n"
        ));
    }
    out.push_str(
        "\n> 这是一次性快照：它不参与运行时检索，**手工修改不会反向入库**。\n\
         > 要改内容请在管理面板里改——那份才是权威记录。\n",
    );

    let general: Vec<&MemoryRecord> = view
        .memories
        .iter()
        .filter(|row| row.scope == store::SCOPE_GENERAL)
        .collect();
    let project: Vec<&MemoryRecord> = view
        .memories
        .iter()
        .filter(|row| row.scope != store::SCOPE_GENERAL)
        .collect();
    if view.memories.is_empty() && view.candidates.is_empty() {
        out.push_str("\n（还没有任何记忆，也没有待确认候选。）\n");
        return out;
    }
    write_records(&mut out, "通用偏好", &general);
    write_records(&mut out, "项目记忆", &project);
    write_candidates(&mut out, &view.candidates);
    out
}

fn write_records(out: &mut String, heading: &str, rows: &[&MemoryRecord]) {
    if rows.is_empty() {
        return;
    }
    out.push_str(&format!("\n## {heading}（{} 条）\n", rows.len()));
    for row in rows {
        out.push_str(&format!("\n### {}\n\n", heading_of(row)));
        out.push_str(&format!(
            "- 范围：{}\n",
            scope_text(&row.scope, &row.workspace_path)
        ));
        out.push_str(&format!(
            "- 状态：{} · 验证：{}\n",
            state_text(&row.state),
            verification_text(&row.verification)
        ));
        if !row.keywords.is_empty() {
            out.push_str(&format!("- 关键词：{}\n", row.keywords.join("、")));
        }
        out.push_str(&format!(
            "- 来源：{} · 创建 {} · 更新 {}\n",
            source_text(&row.source.kind),
            row.created_at,
            row.updated_at
        ));
        out.push_str(&format!(
            "- 已用：{} 次{}\n",
            row.use_count,
            if row.last_used_at.is_empty() {
                String::new()
            } else {
                format!("（最后一次 {}）", row.last_used_at)
            }
        ));
        out.push_str(&format!("\n{}\n", row.body.trim()));
    }
}

fn write_candidates(out: &mut String, rows: &[MemoryCandidate]) {
    let pending: Vec<&MemoryCandidate> = rows
        .iter()
        .filter(|row| row.state == store::CANDIDATE_PENDING)
        .collect();
    if pending.is_empty() {
        return;
    }
    out.push_str(&format!(
        "\n## 待确认候选（{} 条，**尚未生效**）\n\n这些内容还没有被批准，不会进入任何会话。\n",
        pending.len()
    ));
    for row in pending {
        out.push_str(&format!("\n### {}\n\n", heading_of_candidate(row)));
        out.push_str(&format!(
            "- 范围：{}\n- 提出时间：{}\n- 来源：{}\n",
            scope_text(&row.scope, &row.workspace_path),
            row.created_at,
            source_text(&row.source.kind)
        ));
        out.push_str(&format!("\n{}\n", row.body.trim()));
    }
}

fn heading_of(row: &MemoryRecord) -> String {
    let mut heading = if row.title.trim().is_empty() {
        "（无标题）".to_string()
    } else {
        row.title.trim().to_string()
    };
    if row.pinned {
        heading.push_str("（固定项）");
    }
    if row.state != store::STATE_ACCEPTED {
        heading.push_str("（已停用）");
    }
    heading
}

fn heading_of_candidate(row: &MemoryCandidate) -> String {
    if row.title.trim().is_empty() {
        "（无标题）".to_string()
    } else {
        row.title.trim().to_string()
    }
}

fn scope_text(scope: &str, workspace_path: &str) -> String {
    if scope == store::SCOPE_GENERAL {
        return "通用偏好（所有项目）".to_string();
    }
    if workspace_path.is_empty() {
        "本项目".to_string()
    } else {
        format!("本项目 · {workspace_path}")
    }
}

/// 记忆的状态文字。**只服务 `MemoryRecord`**（`STATE_ACCEPTED` / `STATE_DISABLED`），
/// 所以没有 `CANDIDATE_REJECTED` 那一支——候选的状态在导出里另有写法（记忆内核那次评审第 10 条）。
fn state_text(state: &str) -> &str {
    match state {
        store::STATE_ACCEPTED => "accepted（参与检索）",
        store::STATE_DISABLED => "disabled（不参与检索）",
        other => other,
    }
}

fn verification_text(verification: &str) -> &str {
    match verification {
        store::VERIFICATION_VERIFIED => "verified（用户在会话里确认过）",
        store::VERIFICATION_STALE => "stale（可能已过期）",
        store::VERIFICATION_UNVERIFIED => "unverified（还没有被会话验证过）",
        other => other,
    }
}

fn source_text(kind: &str) -> &str {
    match kind {
        store::SOURCE_MANUAL => "user-manual（手动保存）",
        store::SOURCE_SESSION => "session（会话里提出）",
        store::SOURCE_EXTRACT => "extract（自动提取）",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::store::{
        self, CandidateInput, MemorySource, SaveInput, SCOPE_GENERAL, SCOPE_PROJECT,
    };

    /// 每个按 `paths::*` 解析路径的测试都要持住这把锁（见根 AGENTS.md）。
    fn home(tag: &str) -> (std::path::PathBuf, crate::tests::EnvGuard) {
        let dir = std::env::temp_dir().join(format!(
            "xlink-memory-export-{}-{}",
            std::process::id(),
            tag
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let guard = crate::tests::scoped_xlink_home(&dir);
        (dir, guard)
    }

    fn seed(ns: &Namespace) {
        store::save(
            ns,
            SaveInput {
                scope: SCOPE_PROJECT.to_string(),
                workspace_path: "/tmp/demo".to_string(),
                title: "构建前先跑检查".to_string(),
                body: "提交前跑 npm run check".to_string(),
                keywords: vec!["构建".to_string(), "检查".to_string()],
                ..SaveInput::default()
            },
        )
        .expect("seed project");
        store::save(
            ns,
            SaveInput {
                scope: SCOPE_GENERAL.to_string(),
                title: "回答用中文".to_string(),
                body: "默认用中文回答。".to_string(),
                pinned: true,
                ..SaveInput::default()
            },
        )
        .expect("seed general");
        store::submit_candidate(
            ns,
            CandidateInput {
                scope: SCOPE_PROJECT.to_string(),
                workspace_path: "/tmp/demo".to_string(),
                title: "候选标题".to_string(),
                body: "候选正文".to_string(),
                source: MemorySource {
                    kind: store::SOURCE_SESSION.to_string(),
                    session: "s-1".to_string(),
                    ..MemorySource::default()
                },
                ..CandidateInput::default()
            },
        )
        .expect("seed candidate");
    }

    #[test]
    fn export_writes_a_private_snapshot_with_everything_in_it() {
        let (dir, _guard) = home("ok");
        let ns = Namespace::current();
        seed(&ns);

        let result = export(&ns).expect("export");
        assert_eq!(result.memories, 2);
        assert_eq!(result.candidates, 1);
        assert!(result.path.contains("exports"), "{}", result.path);
        assert!(result.path.ends_with(".md"), "{}", result.path);
        let text = std::fs::read_to_string(&result.path).expect("read export");
        assert!(text.contains("# 本地记忆快照"));
        assert!(text.contains("构建前先跑检查"));
        assert!(text.contains("提交前跑 npm run check"));
        assert!(text.contains("关键词：构建、检查"));
        assert!(text.contains("## 通用偏好（1 条）"));
        assert!(text.contains("（固定项）"));
        assert!(text.contains("## 项目记忆（1 条）"));
        assert!(text.contains("待确认候选（1 条，**尚未生效**）"));
        assert!(text.contains("手工修改不会反向入库"));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&result.path)
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600, "导出文件必须是 0600");
        }

        // 同一秒里再导一次：加后缀，不覆盖上一次。
        let again = export(&ns).expect("export twice");
        assert_ne!(again.path, result.path, "导出不许覆盖已有文件");
        assert!(std::path::Path::new(&result.path).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 文档读不出来时，导出的快照必须**自己说出来**——一份看起来正常的空快照
    /// 会让用户以为记忆丢了。
    #[test]
    fn unreadable_document_is_reported_inside_the_snapshot() {
        let (dir, _guard) = home("broken");
        let ns = Namespace::current();
        std::fs::create_dir_all(ns.dir()).expect("mkdir");
        std::fs::write(ns.file(), "{ 这不是 JSON").expect("write broken");

        let result = export(&ns).expect("export");
        let text = std::fs::read_to_string(&result.path).expect("read export");
        assert!(text.contains("没有读出来"), "{text}");
        assert!(text.contains("不代表你没有记忆"), "{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
