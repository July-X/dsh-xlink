//! 「最近注入」：模型**实际看到**了哪几条记忆。
//!
//! 面板要能回答「记忆到底有没有生效」。原先只有 `memory_preview_context`（拿你填的查询
//! **模拟**一遍），而用户真正想知道的是实况。这份环形记录就是实况：插件在真的用了某次
//! 检索之后回报（`/v1/memory/note`），壳记下会话、项目、开关状态与命中的标题。
//!
//! **为什么由插件回报，而不是桥在 `context` 里自己记**：预算用完之后插件每回合都探一次
//! 代次（`policy::shouldProbeLibrary`），探到的结果在库没变时是被丢掉的——桥自己记会把
//! 那些探测也算成「注入」，于是用户看到的是一串「模型看到了 3 条」，而真正的注入被挤出环形。
//!
//! 三条边界：**进程内**（壳重启即空——这是「刚才发生了什么」的信号，不是权威记录，
//! 权威记录是 `memory.json`）、**上限 20 条**、**只存标题与 id，不存正文**（正文是用户
//! 私有内容，日志与状态里都不留）。

/// 一条「模型实际看到了什么」的记录（面板「最近注入」卡片的数据源）。
///
/// **为什么由插件回来说，而不是桥自己记**：桥每次 `context` 调用都看得到命中，但它分不清
/// 这一次的结果**有没有被用上**——预算用完之后插件每回合都探一次代次（`probe`），探到的
/// 东西在库没变时是被丢掉的。桥自己记就会把那些探测也当成「注入」，而用户看到的会是
/// 「模型看到了 3 条」× 十几次，真正的注入反而被挤出去。
///
/// **只记标题与 id，不记正文**：正文是用户私有内容，日志与状态里都不留（同 §9 那条）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct InjectionNote {
    pub at: String,
    pub session: String,
    pub workspace: String,
    pub enabled: bool,
    /// 本次注入实际进上下文的条目（标题可能取不到——那时退回 id）。
    pub hits: Vec<String>,
}

/// 最近若干次注入，新的在前。**进程内**：壳重启就没了——它是一条「刚才发生了什么」的
/// 信号，不是权威记录（权威记录是 memory.json），没必要落盘，也就没有陈旧文件要清理。
const MAX_NOTES: usize = 20;

fn injection_notes() -> &'static std::sync::Mutex<std::collections::VecDeque<InjectionNote>> {
    static NOTES: std::sync::OnceLock<std::sync::Mutex<std::collections::VecDeque<InjectionNote>>> =
        std::sync::OnceLock::new();
    NOTES.get_or_init(|| std::sync::Mutex::new(std::collections::VecDeque::new()))
}

/// 面板读它。中毒的锁不放弃（与 `store::lock` 同一个理由：中毒只说明别人 panic 过）。
pub fn recent_injections() -> Vec<InjectionNote> {
    let guard = injection_notes()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.iter().cloned().collect()
}

/// 插件报告「这一次的结果真的进了上下文」。正文不在参数里——标题由壳按 id 从库里取。
pub(crate) fn record(
    namespace: &super::store::Namespace,
    session: &str,
    workspace: &str,
    enabled: bool,
    ids: &str,
) {
    let hits: Vec<String> = ids
        .split(',')
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(|id| match super::store::find_by_id(namespace, id, true) {
            Ok(Some(record)) if !record.title.trim().is_empty() => record.title,
            // 取不到就退回 id：宁可给用户一个认不出的编号，也不要静默少一条——
            // 「明明注入了三条，这里只显示两条」会让这个卡片整个失去可信度。
            _ => id.to_string(),
        })
        .collect();
    let note = InjectionNote {
        at: crate::memory::store::now_iso(),
        session: session.to_string(),
        workspace: workspace.to_string(),
        enabled,
        hits,
    };
    let mut guard = injection_notes()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.push_front(note);
    guard.truncate(MAX_NOTES);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::store::save;
    use crate::memory::store::testkit::{namespace, project_input, TempHome};

    /// 上限是硬的：环形只留最近 [`MAX_NOTES`] 条，新的在前。
    #[test]
    fn the_ring_keeps_only_the_newest_notes() {
        let _home = TempHome::new("recent-cap");
        let ns = namespace();
        for index in 0..(MAX_NOTES + 5) {
            record(&ns, "cap-test", "/tmp/w", true, "");
            // 造出不同的时间戳不现实（同一毫秒内），所以用 session 之外的字段判不了顺序；
            // 这里只钉「条数」这一头，顺序由下面的单条用例钉。
            let _ = index;
        }
        let mine = recent_injections()
            .into_iter()
            .filter(|note| note.session == "cap-test")
            .count();
        assert_eq!(mine, MAX_NOTES, "环形必须封顶");
    }

    /// 标题取不到时退回 id：宁可给一个认不出的编号，也不要静默少一条——
    /// 「明明注入了三条、这里只显示两条」会让整张卡片失去可信度。
    #[test]
    fn a_missing_record_falls_back_to_its_id() {
        let _home = TempHome::new("recent-fallback");
        let ns = namespace();
        let saved =
            save(&ns, project_input("ws-a", "构建前先跑检查", "提交前跑检查")).expect("save");
        record(
            &ns,
            "fallback-test",
            "/tmp/w",
            true,
            &format!("{},no-such-id", saved.id),
        );
        let note = recent_injections()
            .into_iter()
            .find(|note| note.session == "fallback-test")
            .expect("记录没落进环形");
        assert_eq!(note.hits, vec!["构建前先跑检查", "no-such-id"]);
        assert!(note.enabled);
    }
}
