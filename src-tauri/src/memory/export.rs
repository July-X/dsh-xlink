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
    pub handoffs: usize,
    pub experiences: usize,
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
        handoffs: view.handoffs.len(),
        experiences: view.experiences.len(),
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
    out.push_str(&format!(
        "- 记忆库：{namespace}（依次是 内核族 / 实例 / profile，三段不同就是两份互不相干的记忆）\n"
    ));
    out.push_str(&format!(
        "- 数据代次：{}（内容每写入一次 +1，用来对照「这份快照取的是哪一版」；注入计数不算内容写入，不会抬它）\n",
        status.generation
    ));
    out.push_str(&format!(
        "- 开关：「使用已批准的记忆」= {}\n",
        if status.use_approved { "开" } else { "关" }
    ));
    // **刻意不展示 `status.auto_extract`**（2026-10-18 用户定的标准：给用户看的东西不许有过时
    // 信息）。那个字段全仓没有任何读取方——自动提取真正的闸门是 `use_approved`
    // （`jobs_routes.rs` 的 enqueue 在它关着时回 `disabled`，submit 也有一道「开关已关」的拒收），
    // 而真机 `jobs.json` 里提取任务一直在跑。原先那行写「自动提取 = 关（P2，尚未实现）」：
    // `P2` 是内部阶段名，「尚未实现」与事实相反，取值来自一个死字段。
    out.push_str(
        "- 这个开关管两件事：记忆注入与自动提取；关着时两件事都停（记忆不进入会话，也不再从会话里提取候选）。\n",
    );
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
    if view.memories.is_empty()
        && view.candidates.is_empty()
        && view.handoffs.is_empty()
        && view.experiences.is_empty()
    {
        out.push_str("\n（还没有任何记忆，也没有待确认候选。）\n");
        return out;
    }
    write_records(&mut out, "通用偏好", &general);
    write_records(&mut out, "项目记忆", &project);
    write_candidates(&mut out, &view.candidates);
    let experiences: Vec<_> = view.experiences.iter().map(|row| &row.record).collect();
    write_records(&mut out, "经验", &experiences);
    if !view.handoffs.is_empty() {
        out.push_str("\n## 工作交接（含归档）\n");
    }
    for row in &view.handoffs {
        out.push_str(&format!(
            "\n### {} · {}\n\n项目：{}\n\n目标：{}\n\n{}\n",
            row.record.title,
            if row.record.state == "active" {
                "进行中"
            } else {
                "已归档"
            },
            row.record.workspace_path,
            row.details.goal,
            row.record.body
        ));
        for (label, rows) in [
            ("已完成", &row.details.done),
            ("待办", &row.details.todo),
            ("阻塞", &row.details.blocked),
        ] {
            if !rows.is_empty() {
                out.push_str(&format!("\n{label}：\n- {}\n", rows.join("\n- ")));
            }
        }
    }
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
            // 一条记忆可以带好几处来源（C4），这里全列出来——导出是用户拿去核对的东西，
            // 只列其中一处就等于让这份文件说谎。
            crate::memory::sources::labels(&row.sources),
            row.created_at,
            row.updated_at
        ));
        // **两个数分开写**（2026-10-31）：`recall_count` 是被自动带进上下文的次数，
        // `use_count` 是模型主动调 `memory_read` 按 id 读全文的次数。原先只写后者，
        // 用户看到「模型读过 0 次」会以为记忆没生效——而它可能每一轮都被注入了。
        // 面板上那两处（`MemoryRow.vue` / `MemoryDetail.vue`）用的是同一套说法。
        out.push_str(&format!(
            "- 被注入：{} 次（自动带进会话上下文）\n",
            row.recall_count
        ));
        out.push_str(&format!(
            // 「模型读过」而不是「已用」：`use_count` 只由 `memory_read`（模型读全文）增加，
            // 检索命中与注入**都不计数**——写「已用 N 次」会被读成「被注入过几次」。
            "- 模型读过：{} 次{}\n",
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
            // 候选还没有被批准，仍是「一次提交」而不是「一条记忆」，所以这里只有一个来源。
            crate::memory::sources::label_of(&row.source)
        ));
        // 冲突标记要进快照：它解释了「用户为什么在这条上要想一想」，而导出是**一次性快照**——
        // 漏了它，这份文件就比库里的事实少一件事。
        if !row.conflicts_with.is_empty() {
            out.push_str(&format!(
                "\n- {}\n",
                crate::memory::marks::note_label(row.conflicts_with.len())
            ));
            for id in &row.conflicts_with {
                out.push_str(&format!("  - 与之冲突的既有记忆 id：`{id}`\n"));
            }
        }
        // 取代标记同理（快照里漏了它，导出文件就看不出「这两条候选是同一件事的两个说法」）。
        if !row.supersedes.is_empty() {
            out.push_str(&format!(
                "\n- {}\n",
                crate::memory::marks::superseded_note(row.supersedes.len())
            ));
            for id in &row.supersedes {
                out.push_str(&format!("  - 被本条取代的待确认候选 id：`{id}`\n"));
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::store::{
        self, CandidateInput, ListFilter, MemorySource, SaveInput, SCOPE_GENERAL, SCOPE_PROJECT,
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
        // 给其中一条补一次「被注入」：导出的两个计数必须各写各的。
        let injected = store::list(&ns, &ListFilter::default()).memories[0]
            .id
            .clone();
        store::note_recall(&ns, &[injected.as_str()]).expect("note injection");

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
        assert!(
            text.contains("被注入：1 次"),
            "被注入次数要进导出（它才是「模型看到过它」的那个数）：{text}"
        );
        assert!(
            text.contains("被注入：0 次"),
            "没被注入过的记录也要如实写 0：{text}"
        );
        assert!(
            text.contains("模型读过：0 次"),
            "`use_count` 只由 memory_read 增加，导出要用与面板同一句话：{text}"
        );

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

    /// 头部那行开关必须**如实**，且不许展示死字段（2026-10-18 用户定的标准：给用户看的
    /// 东西必须语义精确、不展示过时信息）。
    ///
    /// 原先写的是「使用已批准的记忆 = 开/关；自动提取 = 关（P2，尚未实现）」：`P2` 是内部
    /// 阶段名；「尚未实现」与事实相反（真机 `jobs.json` 里提取任务一直在跑）；而那个取值
    /// 来自 `meta.auto_extract`——全仓没有读取方，真正的闸门是 `use_approved`。
    #[test]
    fn the_header_states_the_real_switch_and_never_a_dead_field() {
        let (dir, _guard) = home("switch");
        let ns = Namespace::current();
        seed(&ns);

        let off = export(&ns).expect("export");
        let off_text = std::fs::read_to_string(&off.path).expect("read export");
        assert!(
            off_text.contains("- 开关：「使用已批准的记忆」= 关"),
            "默认关着时要如实写「关」：{off_text}"
        );
        assert!(
            !off_text.contains("自动提取 ="),
            "`auto_extract` 没有任何读取方，它的取值不是开关状态：{off_text}"
        );
        assert!(
            !off_text.contains("P2"),
            "导出是给用户读的文件，不许出现内部阶段名：{off_text}"
        );
        assert!(
            off_text.contains("这个开关管两件事：记忆注入与自动提取"),
            "开关管什么必须写清楚（自动提取跟着它一起开关）：{off_text}"
        );

        store::set_use_approved(&ns, true).expect("enable");
        let on = export(&ns).expect("export again");
        let on_text = std::fs::read_to_string(&on.path).expect("read export");
        assert!(
            on_text.contains("- 开关：「使用已批准的记忆」= 开"),
            "打开之后要如实写「开」：{on_text}"
        );
        assert!(!on_text.contains("自动提取 ="), "{on_text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 一条记忆带好几处来源时，导出要**全列**（C4）：只列其中一处，这份拿去核对的
    /// 快照就在替用户说谎——而来源正是用户在导出这份文件时最想核对的东西之一。
    #[test]
    fn every_source_of_a_merged_memory_shows_up_in_the_export() {
        let (dir, _guard) = home("sources");
        let ns = Namespace::current();
        seed(&ns);
        let outcome = store::submit_candidate(
            &ns,
            CandidateInput {
                scope: SCOPE_PROJECT.to_string(),
                workspace_path: "/tmp/demo".to_string(),
                title: "构建前先跑检查".to_string(),
                body: "提交前跑 npm run check".to_string(),
                source: MemorySource {
                    kind: store::SOURCE_EXTRACT.to_string(),
                    session: "s-9".to_string(),
                    ..MemorySource::default()
                },
                ..CandidateInput::default()
            },
        )
        .expect("同一个正文换一个会话再提一次");
        assert!(
            matches!(outcome, store::SubmitOutcome::Merged(_)),
            "同正文应当并入来源，而不是新收一条候选"
        );

        let result = export(&ns).expect("export");
        assert_eq!(result.memories, 2, "并入来源不新建记录");
        assert_eq!(result.candidates, 1, "并入来源不占用户的待确认清单");
        let text = std::fs::read_to_string(&result.path).expect("read export");
        assert!(
            text.contains("user-manual（手动保存） + extract（自动提取）"),
            "两个来源都要出现在导出里：{text}"
        );
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
