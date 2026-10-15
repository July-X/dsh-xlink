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
/// 插件最后一次**真的调用**壳是什么时候（`context` / `read` / `note` / 提取任务那几条都算），
/// 以及开关最后一次改动是什么时候——用来答 E6 那句「你刚拨的开关，运行中的工作台认了吗」。
///
/// **两个时刻都只在本次壳进程里有意义**（`AtomicU64` 的毫秒时间戳，0 = 没见过）：
/// 壳重启之后工作台也重启了，「认没认」这个问题自然消失。所以它们**不需要进 schema**——
/// 设计稿要的那个「处理中」状态，靠进程内这两个数就能算准，不必为它抬一次版本。
static LAST_PLUGIN_CALL_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static SETTING_CHANGED_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|delta| delta.as_millis() as u64)
        .unwrap_or(0)
}

/// 记一次插件调用（每条路由都调它，见 `routes::note_memory_call` 与 `note_job_call`）。
pub(crate) fn mark_plugin_call() {
    LAST_PLUGIN_CALL_MS.store(now_ms(), std::sync::atomic::Ordering::Relaxed);
}

/// 记一次开关改动（`memory_set_enabled`）。
pub(crate) fn mark_setting_changed() {
    SETTING_CHANGED_MS.store(now_ms(), std::sync::atomic::Ordering::Relaxed);
}

/// **开关改过、而插件还没再调用过壳** = 运行中的工作台还没认这次改动。
///
/// 两个时刻任一没见过就回 `false`（「不知道」不当成「处理中」——那会给一句假警报）。
/// 插件**每一步**都会问一次 `/context`，所以正常情况下这个状态只存在几秒。
pub fn effect_pending() -> bool {
    pending_effect(
        SETTING_CHANGED_MS.load(std::sync::atomic::Ordering::Relaxed),
        LAST_PLUGIN_CALL_MS.load(std::sync::atomic::Ordering::Relaxed),
    )
}

/// 判据本体（纯函数，两个时刻当参数）——进程级静态在并行测试里会互相干扰，
/// 所以「算得对不对」与「数从哪来」分开。
fn pending_effect(changed_ms: u64, called_ms: u64) -> bool {
    if changed_ms == 0 {
        return false;
    }
    changed_ms > called_ms
}

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
        // **条数上限**（第二次评审第 9 条）：正常路径只有 ≤6 个 id（插件报的是它注入的那几条），
        // 而请求头最大 8 KiB、最坏能塞 ~250 个——每个都要整库解析一遍。对齐 `MAX_HITS`。
        .take(super::query::MAX_HITS)
        .map(|id| match super::store::find_by_id(namespace, id, true) {
            // **可见性也要过**（同一条评审）：这是唯一一条不过 `scope_matches` 的按 id 读取路径，
            // 一个越权/传错的 id 会让**别的项目**的标题出现在面板「最近注入」里。
            // 坐标只有路径（`note` 只带 `workspace`），所以按路径那条兜底判——通用偏好照旧可见。
            Ok(Some(record))
                if !record.title.trim().is_empty()
                    && super::query::scope_matches(&record, "", workspace) =>
            {
                record.title
            }
            // 取不到（或不在这个范围里）就退回 id：宁可给用户一个认不出的编号，也不要静默少一条——
            // 「明明注入了三条，这里只显示两条」会让这个卡片整个失去可信度。**退回 id 也不泄露标题。**
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
    use crate::memory::store::testkit::{general_input, namespace, project_input, TempHome};

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

    /// **别的项目的标题不许出现在「最近注入」里**（第二次评审第 9 条）：`note` 是唯一一条
    /// 不过 `scope_matches` 的按 id 读取路径——越权/传错的 id 会让 A 项目的标题出现在
    /// B 项目的卡片上。退回 id 而不是标题，既不泄露也不静默少一条。
    #[test]
    fn another_projects_title_is_not_shown() {
        let _home = TempHome::new("recent-scope");
        let ns = namespace();
        let a = save(&ns, project_input("ws-a", "A 项目的秘密", "只有 A 知道")).expect("save a");
        let general = save(&ns, general_input("通用偏好", "回答用中文")).expect("save general");

        record(
            &ns,
            "scope-test",
            "/tmp/ws-b",
            true,
            &format!("{},{}", a.id, general.id),
        );
        let note = recent_injections()
            .into_iter()
            .find(|note| note.session == "scope-test")
            .expect("记录没落进环形");
        assert_eq!(
            note.hits,
            vec![a.id.as_str(), "通用偏好"],
            "A 项目的标题不该出现在 B 项目里；通用偏好照旧可见"
        );
    }

    /// **「处理中」的判据**（E6）：开关改过、而插件还没再调用过壳 → 运行中的工作台还没认。
    /// 两个时刻任一没见过（0）都回 false——「不知道」不当成「处理中」，那会给一句假警报。
    #[test]
    fn effect_pending_needs_both_timestamps_and_a_later_change() {
        assert!(!pending_effect(0, 0), "两个都没见过：不知道就不说");
        assert!(!pending_effect(0, 100), "没改过开关：谈不上待确认");
        // 改过、而插件**从没**调用过：也算没认——我们确实没法声称它生效了
        // （插件可能压根没在跑，那种情况由「插件没启用」那条提示负责，这里不重复也不撒谎）。
        assert!(pending_effect(100, 0), "从没确认过就不能说已生效");
        assert!(pending_effect(200, 100), "改动在最后一次调用之后 → 还没认");
        assert!(!pending_effect(100, 200), "调用在改动之后 → 认了");
        assert!(
            !pending_effect(100, 100),
            "同一毫秒算认了（不放大成待确认）"
        );
    }

    /// 开关改动的时刻真的会被记下来（接线判据：helper 有测试、接线没有是本仓反复栽的形状）。
    #[test]
    fn marking_a_setting_change_makes_it_pending_until_the_next_call() {
        mark_plugin_call();
        assert!(!effect_pending(), "刚调用过、还没改开关：不是待确认");
        // 两个时刻都是毫秒级：睡 2ms 让「改动在后」这件事**确定**，否则同毫秒内是竞态。
        std::thread::sleep(std::time::Duration::from_millis(2));
        mark_setting_changed();
        assert!(effect_pending(), "改完开关、插件还没再来问：待确认");
        mark_plugin_call();
        assert!(!effect_pending(), "插件又来问过了：认了");
    }

    /// 标题取不到时退回 id：宁可给一个认不出的编号，也不要静默少一条——
    /// 「明明注入了三条、这里只显示两条」会让整张卡片失去可信度。
    #[test]
    fn a_missing_record_falls_back_to_its_id() {
        let _home = TempHome::new("recent-fallback");
        let ns = namespace();
        let saved =
            save(&ns, project_input("ws-a", "构建前先跑检查", "提交前跑检查")).expect("save");
        // 工作区路径要**对得上**：`record` 现在按路径过一遍可见性（第二次评审第 9 条），
        // 路径不匹配的项目记忆会退回 id 而不是显示标题。
        record(
            &ns,
            "fallback-test",
            "/tmp/ws-a",
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
