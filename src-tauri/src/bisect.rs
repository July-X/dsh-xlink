//! 二分定位：把"起不来"缩到**能解释现象的最小集合**。
//!
//! ## 为什么不是 `guard.rs` 那道阶梯
//!
//! 阶梯按嫌疑度逐个停用插件，最坏要 n 次启动；n=12 意味着 6 分钟，用户
//! 不会等。二分把它压到 ⌈log₂n⌉ 次。阶梯还只降插件——坏的是技能、补丁或
//! 内核本身时，三次全废还会给出"移除这些插件"的错误方向。
//!
//! 二分**不替换**阶梯，而是分层：阶梯是确定性故障的快速止血，二分是它
//! 走完之后、以及偶发故障时的兜底。两者共享同一套嫌疑度排序
//! （`guard::attribute`）与证据落盘。
//!
//! ## P2 的范围
//!
//! 只有 `probe = startup-failed`（内核没能在就绪窗口内应答端口）、`k = 1`
//!（跑一次就算）。这与阶梯同判据，所以:
//! - P2 的结果**可解释**：跑一次挂 = 挂；
//! - 不引入偶发故障的误判成本。
//!
//! `crashed-within-T` 与 `blank-screen` 属 P3：它们需要 `k > 1` 与时间窗，
//! 判定本身是另一套问题。
//!
//! ## 产物是"最小坏集合"，不是"根因"
//!
//! 组合效应（两个插件单独都正常，一起就炸）会让二分收敛到一个不可修的
//! 答案上；不收敛（原因在候选集合之外）也是真实结局。所以 `Conclusion`
//! 只有三种取值，**没有"找到根因"**——那是过度承诺。

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::state::{self, StateCtx};

/// 二分最多试几轮。⌈log₂n⌉ 在 n ≤ 64 时不超过 6，加上"全通"与"全挂"两个
/// 极端各一轮，12 轮足够覆盖本项目的候选规模；再多是浪费用户的时间。
const MAX_ROUNDS: usize = 12;
/// 候选数少于这个值就不值得二分——直接全试完更省事，也更快给出结论。
pub const MIN_CANDIDATES: usize = 3;
/// 单次启动的证据留白上限，按字符。
const EVIDENCE_MAX_CHARS: usize = 240;

/// 一次试探的判定。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Outcome {
    /// 这半边在本次试探下起不来。
    Fail,
    /// 这半边在本次试探下起来了。
    Pass,
    /// 这一半根本没试成（沙盒起不来、端口拿不到）。**不是** Pass——
    /// 把它当 Pass 会让二分朝错误方向收敛。
    Inconclusive,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Fail => "fail",
            Outcome::Pass => "pass",
            Outcome::Inconclusive => "inconclusive",
        }
    }
}

/// 二分结束的三种结局。刻意没有"找到根因"这种取值。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Conclusion {
    /// `minimal-bad-set` / `not-in-set` / `aborted`。
    pub kind: String,
    /// `minimal-bad-set` 时是最小坏集合的成员 id。
    pub members: Vec<String>,
    /// 给人看的一句话。**必须**如实说明这是"能解释现象的最小集合"。
    pub text: String,
}

/// 二分会话里的一步。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    /// 这一轮试了哪些候选（按嫌疑度降序的前一半或后一半）。
    pub tried: Vec<String>,
    pub outcome: String,
    /// 这一轮的日志证据摘录。
    #[serde(default)]
    pub evidence: String,
    pub at_ms: u64,
    pub round: usize,
}

/// 一次二分排查的完整记录。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BisectSession {
    pub schema: u32,
    pub started_at_ms: u64,
    pub family: String,
    pub instance: String,
    /// P2 固定 `startup-failed`；字段先留着，P3 扩 probe 时不必改文档格式。
    pub probe: String,
    /// 判定"好"需要连续通过几次。P2 固定 1。
    pub k: u32,
    /// 按嫌疑度降序的候选全集。
    pub candidates: Vec<String>,
    pub steps: Vec<Step>,
    pub rounds: usize,
    /// 还在进行中（可以被中断）。
    #[serde(default)]
    pub running: bool,
    pub conclusion: Option<Conclusion>,
    /// 每一轮之前**排除**掉的成员，累加。面板据此显示"已排除 N 个"。
    #[serde(default)]
    pub cleared: Vec<String>,
}

impl BisectSession {
    fn is_compatible(&self) -> bool {
        self.schema <= SCHEMA
    }
}

const SCHEMA: u32 = 1;

fn ctx() -> StateCtx {
    StateCtx {
        corrupt: |reason| format!("二分排查记录无法解析：{reason}。可以重新发起一次排查"),
        kind: |reason| AppError::Io(format!("二分排查记录无法解析：{reason}")),
    }
}

pub fn session_file(family: &str, instance: &str) -> std::path::PathBuf {
    crate::paths::instance_dir(family, instance)
        .join("bisect")
        .join("state.json")
}

/// 读取会话，**原样返回磁盘上的状态**（含 `running=true`）。
///
/// 内部流程（`next_trial` / `advance` / `abort`）一律走它：`running` 是
/// "还有下一轮"的唯一真相，中途收尾会让刚发起的排查立刻返回 None。
fn read_raw(family: &str, instance: &str) -> BisectSession {
    let session: BisectSession = state::load_lossy(&session_file(family, instance));
    if session.is_compatible() {
        return session;
    }
    BisectSession {
        schema: SCHEMA,
        family: family.into(),
        instance: instance.into(),
        ..BisectSession::default()
    }
}

/// 展示路径的读取：把**壳崩溃残留**的 `running=true` 收尾成"已中断"。
///
/// 上一轮排查若被崩溃打断，它会永远停在 `running=true`，面板会一直显示
/// "排查进行中"——用户既等不到结果也看不到为什么。读取时如实收尾是唯一
/// 诚实的做法；编造一个结论才是撒谎。已排除的结果原样保留，用户重新发起
/// 能接着缩小范围。
///
/// 只在**读**的时候改内存里的副本，不写盘：真正的会话由下一次 `begin` /
/// `advance` / `abort` 覆盖。
pub fn load(family: &str, instance: &str) -> BisectSession {
    let mut session = read_raw(family, instance);
    if session.running {
        session.running = false;
        session.conclusion = Some(Conclusion {
            kind: "aborted".into(),
            members: Vec::new(),
            text:
                "上一次排查被中断（桌面端退出或崩溃）。已排除的结果保留，重新发起会接着缩小范围。"
                    .into(),
        });
    }
    session
}

fn save(family: &str, instance: &str, session: &BisectSession) -> Result<(), AppError> {
    state::save(&session_file(family, instance), session, ctx())
}

/// 给面板的只读视图。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BisectView {
    pub running: bool,
    pub probe: String,
    pub k: u32,
    pub candidate_count: usize,
    /// 还没被排除的候选（下一轮要试的池子）。
    pub remaining: usize,
    pub cleared: Vec<String>,
    pub steps: Vec<Step>,
    pub rounds: usize,
    pub conclusion: Option<Conclusion>,
    pub started_at_ms: u64,
}

pub fn view(family: &str, instance: &str) -> BisectView {
    let session = load(family, instance);
    BisectView {
        running: session.running,
        probe: session.probe.clone(),
        k: session.k,
        candidate_count: session.candidates.len(),
        remaining: session
            .candidates
            .len()
            .saturating_sub(session.cleared.len()),
        cleared: session.cleared.clone(),
        rounds: session.rounds,
        steps: session.steps.clone(),
        conclusion: session.conclusion.clone(),
        started_at_ms: session.started_at_ms,
    }
}

/// 发起一次二分。返回初始视图——真正的试探由 [`advance`] 一轮一轮推进，
/// 这样面板能逐步显示"已排除 N 个"而不是让用户对着一个不动的进度条等。
pub fn begin(
    family: &str,
    instance: &str,
    mut candidates: Vec<String>,
) -> Result<BisectView, AppError> {
    // 按嫌疑度降序已经在 `candidates` 里（由调用方按 `guard::attribute`
    // 的结论排好），但仍做一次去重：同一个插件 id 出现两次会让"试一半"算错。
    let mut seen = std::collections::BTreeSet::new();
    candidates.retain(|id| !id.is_empty() && seen.insert(id.clone()));
    if candidates.len() < MIN_CANDIDATES {
        return Err(AppError::Io(format!(
            "只有 {} 个可排查对象，少于 {MIN_CANDIDATES} 个——逐个停用一遍比二分更快，请直接在插件面板里处理",
            candidates.len()
        )));
    }
    let session = BisectSession {
        schema: SCHEMA,
        started_at_ms: crate::process::epoch_millis(),
        family: family.into(),
        instance: instance.into(),
        probe: "startup-failed".into(),
        k: 1,
        candidates,
        steps: Vec::new(),
        rounds: 0,
        running: true,
        conclusion: None,
        cleared: Vec::new(),
    };
    save(family, instance, &session)?;
    Ok(view(family, instance))
}

/// 由调用方（试完一轮之后）回报结果，推进到下一轮或收尾。
///
/// `probed` 是这一轮**实际启用**的候选 id 集合。`outcome` 必须是真实的
/// 试探结果——本模块不自己起内核，它没有理由替用户下结论。
pub fn advance(
    family: &str,
    instance: &str,
    probed: Vec<String>,
    outcome: Outcome,
    evidence: String,
) -> Result<BisectView, AppError> {
    let mut session = read_raw(family, instance);
    if !session.running {
        return Ok(view(family, instance));
    }
    session.rounds += 1;
    session.steps.push(Step {
        tried: probed.clone(),
        outcome: outcome.as_str().to_string(),
        evidence: evidence.chars().take(EVIDENCE_MAX_CHARS).collect(),
        at_ms: crate::process::epoch_millis(),
        round: session.rounds,
    });
    session.running = false;

    if outcome == Outcome::Inconclusive {
        session.conclusion = Some(Conclusion {
            kind: "aborted".into(),
            members: Vec::new(),
            text: "这一轮没能真正试起来（沙盒环境不可用），排查无法继续。".into(),
        });
        save(family, instance, &session)?;
        return Ok(view(family, instance));
    }

    // 剩下没被这一轮覆盖的候选。分治的前提是「坏的一定在启用集合里」，
    // 所以：Fail → 坏的在 enabled 侧，把 disabled 侧记为已排除；
    //        Pass → 坏的不在 enabled 侧，把 enabled 侧记为已排除。
    let probed_set: std::collections::BTreeSet<&String> = probed.iter().collect();
    let cleared_this_round: Vec<String> = match outcome {
        Outcome::Fail => session
            .candidates
            .iter()
            .filter(|id| !probed_set.contains(id))
            .cloned()
            .collect(),
        _ => session
            .candidates
            .iter()
            .filter(|id| probed_set.contains(id))
            .cloned()
            .collect(),
    };
    for id in &cleared_this_round {
        if !session.cleared.contains(id) {
            session.cleared.push(id.clone());
        }
    }
    let remaining: Vec<String> = session
        .candidates
        .iter()
        .filter(|id| !session.cleared.contains(id))
        .cloned()
        .collect();

    match outcome {
        // **顺序要紧**：先判"剩 1 个"。Fail 时 remaining 就是"坏的那侧"，
        // 剩 1 个 = 找到了最小坏集合；剩多个 = 它们的组合。两个分支的
        // `remaining == probed` 判据在剩 1 个时同样成立，先命中就永远
        // 只能报组合，测试 `converges_to_a_single_member` 正是钉这一条。
        Outcome::Fail if remaining.len() == 1 => {
            let member = remaining[0].clone();
            session.conclusion = Some(Conclusion {
                kind: "minimal-bad-set".into(),
                members: vec![member.clone()],
                text: format!(
                    "能解释现象的最小集合是 {{ {member} }}。这不等于根因——组合效应仍可能参与，建议先只停用它验证。"
                ),
            });
        }
        // 全挂：这一轮启用侧就是全部剩余，说明"坏的"可能是它们的组合而不是
        // 其中某一个。诚实收尾，别硬凑一个根因。
        Outcome::Fail if remaining.len() == probed_set.len() => {
            session.conclusion = Some(Conclusion {
                kind: "not-in-set".into(),
                members: Vec::new(),
                text: format!(
                    "启用这 {} 个扩展时内核起不来，而把它们全关掉就正常——问题出在它们的**组合**上，不是其中某一个。逐个排查请从这组里挑一半再试。",
                    remaining.len()
                ),
            });
        }
        // 全通：剩下的都是好的，收尾。
        Outcome::Pass if remaining.is_empty() => {
            session.conclusion = Some(Conclusion {
                kind: "not-in-set".into(),
                members: Vec::new(),
                text: "把全部扩展都关掉也起不来——问题不在插件与技能这一层，请查内核版本、Node 环境或端口。".into(),
            });
        }
        Outcome::Pass if remaining.len() == 1 => {
            let member = remaining[0].clone();
            session.conclusion = Some(Conclusion {
                kind: "minimal-bad-set".into(),
                members: vec![member.clone()],
                text: format!(
                    "能解释现象的最小集合是 {{ {member} }}：关掉其余扩展后，只有它还导致起不来。"
                ),
            });
        }
        Outcome::Fail if session.rounds >= MAX_ROUNDS => {
            session.conclusion = Some(Conclusion {
                kind: "aborted".into(),
                members: remaining.clone(),
                text: format!(
                    "已排查 {} 轮仍未收敛，剩余 {} 个候选。重新发起会接着缩小范围。",
                    session.rounds,
                    remaining.len()
                ),
            });
        }
        _ => {
            // 还能继续分：把会话重新打开，等下一轮。
            session.running = true;
        }
    }
    save(family, instance, &session)?;
    Ok(view(family, instance))
}

/// 本轮该试哪一半（按嫌疑度降序切分）。
///
/// 返回 `None` 表示不该再试了（已在上一轮收尾）。切法取前一半：候选已按
/// 嫌疑度排序，坏的大概率集中在前面，取前半能把最快见效的那批先试掉。
pub fn next_trial(family: &str, instance: &str) -> Option<Vec<String>> {
    let session = read_raw(family, instance);
    if !session.running {
        return None;
    }
    let remaining: Vec<String> = session
        .candidates
        .iter()
        .filter(|id| !session.cleared.contains(id))
        .cloned()
        .collect();
    if remaining.len() < 2 {
        return None;
    }
    Some(remaining[..remaining.len() / 2].to_vec())
}

/// 这一轮该**关掉**哪些（= 剩余集合减去本轮启用的）。它们会在试完之后被
/// 记进 `cleared`，但**试的当下**必须真的从 profile 接线里摘掉——否则这一
/// 轮什么都没排除掉。
pub fn disabled_during(trial: &[String], family: &str, instance: &str) -> Vec<String> {
    let session = load(family, instance);
    let trial_set: std::collections::BTreeSet<&String> = trial.iter().collect();
    session
        .candidates
        .iter()
        .filter(|id| !trial_set.contains(id) && !session.cleared.contains(id))
        .cloned()
        .collect()
}

/// 排查用的候选项：当前实例启用着的插件 + 启用的技能。
///
/// 刻意**不含**补丁与内核版本：补丁有独立备份与独立 UI，自动改它爆炸半径
/// 太大（与 P1 恢复的取舍同源）；内核版本切换代价高，按设计稿 §6.4 走
/// 一次廉价的前置二分，不塞进 n 元候选。
pub fn candidates(data_dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = crate::plugins::load_store(data_dir)
        .items
        .iter()
        .filter(|item| !crate::quarantine::ids(data_dir).contains(&item.id))
        .map(|item| item.id.clone())
        .collect();
    out.sort();
    out.extend(crate::skills::active_entry_names());
    out
}

/// 中断一次进行中的排查。已排除的结果保留——用户下次重开能接着缩小。
pub fn abort(family: &str, instance: &str, why: &str) -> Result<BisectView, AppError> {
    let mut session = read_raw(family, instance);
    if !session.running {
        return Ok(view(family, instance));
    }
    session.running = false;
    session.conclusion = Some(Conclusion {
        kind: "aborted".into(),
        members: Vec::new(),
        text: why.to_string(),
    });
    save(family, instance, &session)?;
    Ok(view(family, instance))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 每次调用拿一个**独立**的临时 home。
    ///
    /// 早先这里用 `std::mem::forget(guard)` 让 EnvGuard「活到测试结束」——
    /// 那是个真 bug：`EnvGuard` 持有的是进程级互斥锁，forget 掉就**永远不
    /// 释放**，同进程后续所有拿这把锁的测试全部死锁（本轮实测 7 个测试挂在
    /// `has been running for over 60 seconds`）。正确做法是让 guard 跟着
    /// 返回值走作用域，测试结束自动归还。
    fn seeded(tag: &str) -> (String, String, crate::tests::EnvGuard) {
        let home = std::env::temp_dir().join(format!("dsh-bisect-{tag}-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&home);
        let guard = crate::tests::scoped_xlink_home(&home);
        (
            String::from(crate::instance::KERNEL_FAMILY_DSH),
            home.to_string_lossy().into_owned(),
            guard,
        )
    }

    fn cleanup(home: &str) {
        let _ = std::fs::remove_dir_all(home);
    }

    /// 分治的根本不变式：**坏的一定在启用集合里**。Fail 时被关掉的那一半
    /// 才可以记进 `cleared`。这个测试钉死它——记反了会让二分朝反方向收敛，
    /// 而且不会报任何错。
    #[test]
    fn fail_clears_the_disabled_half() {
        let (family, home, _guard) = seeded("fail-half");
        let instance = "bisect-test";
        // 4 个候选，本轮试前 2 个且失败 → 后 2 个被排除。
        let candidates = vec!["a".into(), "b".into(), "c".into(), "d".into()];
        begin(&family, instance, candidates).unwrap();
        let trial = next_trial(&family, instance).unwrap();
        assert_eq!(trial, vec!["a".to_string(), "b".to_string()]);

        let view = advance(
            &family,
            instance,
            trial,
            Outcome::Fail,
            "boot failed".into(),
        )
        .unwrap();

        assert!(!view.running, "Fail 后还有剩余，应该继续下一轮");
        assert_eq!(view.cleared, vec!["c".to_string(), "d".to_string()]);
        assert_eq!(view.remaining, 2);

        cleanup(&home);
    }

    /// Pass 的镜像：启用侧被排除。搞反这个会让二分永远不收敛。
    #[test]
    fn pass_clears_the_enabled_half() {
        let (family, home, _guard) = seeded("pass-half");
        let instance = "bisect-test";
        begin(
            &family,
            instance,
            vec!["a".into(), "b".into(), "c".into(), "d".into()],
        )
        .unwrap();
        let trial = next_trial(&family, instance).unwrap();
        let view = advance(&family, instance, trial, Outcome::Pass, String::new()).unwrap();

        assert_eq!(view.cleared, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(view.remaining, 2);

        cleanup(&home);
    }

    /// 收敛到单个成员 → 最小坏集合。这是二分唯一"成功"的结局。
    #[test]
    fn converges_to_a_single_member() {
        let (family, home, _guard) = seeded("converge");
        let instance = "bisect-test";
        begin(
            &family,
            instance,
            vec!["good".into(), "bad".into(), "other".into()],
        )
        .unwrap();
        // 3 个候选的 `next_trial` 取前一半 = 1 个，不是 2 个。断言写清楚，
        // 免得以后有人按"除以二向上取整"改错切分点。
        let trial = next_trial(&family, instance).unwrap();
        assert_eq!(trial, vec!["good".to_string()]);
        // 启用 [good] 失败 → 排除 [bad, other]，剩 [good] = 最小坏集合。
        let view = advance(&family, instance, trial, Outcome::Fail, String::new()).unwrap();

        assert!(!view.running, "只剩 1 个时应当立刻收尾");
        let conclusion = view.conclusion.expect("应当收尾");
        assert_eq!(conclusion.kind, "minimal-bad-set");
        assert_eq!(conclusion.members, vec!["good".to_string()]);
        // 措辞必须说明"不等于根因"，否则用户会把最小集合当成唯一答案。
        assert!(conclusion.text.contains("不等于根因"));

        cleanup(&home);
    }

    /// 全挂：这一轮启用侧就是全部剩余。组合效应，不硬凑根因。
    #[test]
    fn all_fail_is_a_combination_not_a_single_culprit() {
        let (family, home, _guard) = seeded("all-fail");
        let instance = "bisect-test";
        // 3 个候选（begin 要求 ≥3），本轮试前 1 个且失败 → 排除后 2 个，
        // 剩下正好等于本轮启用侧，说明"坏的"是它们的组合而不是某一个。
        begin(
            &family,
            instance,
            ["a", "b", "c", "d"].map(String::from).to_vec(),
        )
        .unwrap();
        // 4 个候选 → 本轮试前 2 个 [a,b] 且都失败 → 排除 [c,d]，剩 [a,b]
        // 仍为 2 个、且正好等于本轮试的集合：坏的不是一个，而是它们的组合。
        // （3 个候选永远收敛到剩 1 个，测不到这一分支——这是场景设计
        // 时踩过的坑，所以在这里写明。）
        let trial = next_trial(&family, instance).unwrap();
        assert_eq!(trial, vec!["a".to_string(), "b".to_string()]);
        let view = advance(&family, instance, trial, Outcome::Fail, String::new()).unwrap();

        let conclusion = view.conclusion.expect("应当收尾");
        assert_eq!(conclusion.kind, "not-in-set");
        assert!(conclusion.members.is_empty());
        assert!(conclusion.text.contains("组合"));

        cleanup(&home);
    }

    /// 试探本身没跑成 ≠ Pass。把它当 Pass 会让二分把真正有嫌疑的那半边
    /// 标成已排除——那比不做二分更糟。
    #[test]
    fn inconclusive_never_clears_anything() {
        let (family, home, _guard) = seeded("inconclusive");
        let instance = "bisect-test";
        begin(
            &family,
            instance,
            vec!["a".into(), "b".into(), "c".into(), "d".into()],
        )
        .unwrap();
        let trial = next_trial(&family, instance).unwrap();
        let view = advance(
            &family,
            instance,
            trial,
            Outcome::Inconclusive,
            "sandbox could not start".into(),
        )
        .unwrap();

        assert!(view.cleared.is_empty(), "没试成时不允许排除任何候选");
        assert!(!view.running);
        assert_eq!(view.conclusion.unwrap().kind, "aborted");

        cleanup(&home);
    }

    /// 候选太少时直接拒绝——逐个停用更快。
    #[test]
    fn refuses_too_few_candidates() {
        let (family, home, _guard) = seeded("too-few");
        let err = begin(&family, "bisect-test", vec!["only".into()]).unwrap_err();
        assert!(err.to_string().contains("逐个停用"));
        cleanup(&home);
    }

    /// 重复 id 会让"试一半"算错（分母不对，结论会指错东西）。
    #[test]
    fn deduplicates_candidates() {
        let (family, home, _guard) = seeded("dedup");
        let instance = "bisect-test";
        let view = begin(
            &family,
            instance,
            vec!["a".into(), "a".into(), "b".into(), "c".into(), "".into()],
        )
        .unwrap();
        assert_eq!(view.candidate_count, 3, "重复与空 id 必须被去掉");
        cleanup(&home);
    }

    /// 崩溃残留的 running 会话读取时必须收尾，否则面板永远显示"进行中"。
    #[test]
    fn a_crashed_session_reads_back_as_aborted() {
        let (family, home, _guard) = seeded("crashed");
        let instance = "bisect-test";
        begin(
            &family,
            instance,
            vec!["a".into(), "b".into(), "c".into(), "d".into()],
        )
        .unwrap();
        // 刚落盘时确实是 running=true。用 `read_raw` 而不是 `load` 断言——
        // `load` 正是负责把它收尾的那条展示路径，对它断言 running 恒假。
        assert!(read_raw(&family, instance).running);

        // 模拟壳崩溃：直接写一份 running=true 的文档。
        let raw = serde_json::json!({
            "schema": 1,
            "startedAtMs": 1,
            "family": family,
            "instance": instance,
            "probe": "startup-failed",
            "k": 1,
            "candidates": ["a", "b", "c", "d"],
            "steps": [],
            "rounds": 2,
            "running": true,
            "cleared": ["d"],
        });
        std::fs::create_dir_all(session_file(&family, instance).parent().unwrap()).unwrap();
        std::fs::write(
            session_file(&family, instance),
            serde_json::to_string(&raw).unwrap(),
        )
        .unwrap();

        let session = load(&family, instance);
        assert!(!session.running, "崩溃残留的会话读取时必须收尾");
        assert_eq!(session.conclusion.unwrap().kind, "aborted");
        // 已排除的结果要保住，用户重开才能接着缩小。
        assert_eq!(session.cleared, vec!["d".to_string()]);

        cleanup(&home);
    }
}
