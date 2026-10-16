//! 一条记忆「从哪些地方来」：落盘形状、合并与说法。
//!
//! 首版每条记录只有**一个**来源，于是「模型在会话 B 又提了一次同一件事」只有两个结局：
//! 收成第二条同内容的记录，或拒收并**丢掉「这件事在会话 B 也出现过」这条信息**。设计 §7.2 要的
//! 是第三种：**相同规范化正文合并来源**——一条记录带多个来源，用户在面板与导出里一眼看得出
//! 「这条我不止说过一次，而且不只在这个会话里说过」。
//!
//! 单独一个文件而不是塞进 `store.rs`：那个文件已经贴着 665 行的反棘轮线（≥600 只许下调）。
//! 这一层管「列表的形状与规矩」，与 `store`（记什么）、`input`（单条怎么规范化）、
//! `duplicates`（与库里已有的比一比）是四件事，混在一起每加一条规则都要重新谈判行数。
//!
//! **形状迁移走 serde 而不是手写迁移步骤**：`load()` 没有迁移阶段，而 `check_writable` 在版本
//! 对不上时直接拒写——一份需要用户手工改的库，等于把一次数据损坏变成一次事故。两种形状都在
//! **读侧**归一（旧的单对象、新写的数组都读成列表），**写侧**只出新形状；抬 `SCHEMA_VERSION`
//! 之后旧应用读到新库会被拒写而不是静默降级丢字段（那正是版本账要挡的事）。

use serde::{Deserialize, Deserializer};

use super::store::{
    MemoryDoc, MemorySource, SOURCE_EXTRACT, SOURCE_MANUAL, SOURCE_SESSION, VERIFICATION_STALE,
};

/// 一条记录最多记几个来源。超出的按「并不进去」处理——见 [`merge`]。
pub(crate) const MAX_SOURCES: usize = 8;

/// **来源变了就把先前那条待确认候选标过期**（H7，设计 §8-6「来源变化时保留旧版本候选并标过期，
/// 或放弃排新任务，不能覆写更新结果」）。
///
/// ## 口径（2026-10-16 维护者拍板 (a)）
///
/// 三条同时成立才算「来源变了」：
/// 1. **同一个会话**——别的会话说的话不是「改口」，那是另一件事；
/// 2. **序号区间有重叠**——这是**重提**（压缩/编辑之后回头提取同一段），不是「接着往后说」；
/// 3. **内容指纹不同**——序号没动而内容变了，才叫来源变了。
///
///    指纹由插件侧算（见 [`super::store::MemorySource::content_hash`] 为什么不用密码学哈希）。
///
/// ## 第 2 条成不成立，取决于插件**从哪读起**（2026-10-16 实测出来的，不是推演）
///
/// 「区间重叠」本来靠**提取的起点往前推**来收窄：任务键里带着 `from_cursor`（这段会话上次
/// 提取到哪儿），所以第二次提取读的是 `[from_cursor, 新游标]`，与上一条的
/// `[上一条证据, 上一游标]` **一般不相交**。
///
/// **而当时那个 `from_cursor` 在真机上根本没到得了读取那一行**——插件重建任务键时手挑了
/// `session / cursor / extractor` 三个字段（`host/index.js` 里那句旧写法
/// `const key = { session: liveSession, cursor: endSeq, extractor }`；**缺陷在手挑这一步，
/// 不在 `host/bridge.js` 的 `claimKeyOf`——它是 2026-10-16 的修法本身**），
/// 于是 `from` 恒为 0，每次都读 `0..endSeq`。后果正是上面刚否掉的 (b)：**指纹覆盖整段会话，
/// 会话一长就变，于是三条同时成立、每次都标**。
///
/// 所以第 2 条不是「本来就有的保险」，它是**与插件那边的读取起点绑在一起的**：那边一改口径
/// （比如改成每次从 0 读、或把 `from_cursor` 去掉），这边的口径 (a) 就静默退化成 (b)。
/// 插件侧那条判据是 `领取回来的任务键要原样带过去`，改那边时它会红。
///
/// **为什么不按「序号在它之后」判**（曾经的备选 (b)）：同一会话的提取区间天然重叠
/// （`end_seq` 是任务冻结时的截止游标，通常在会话末尾），按那个口径**几乎每一条新候选都会
/// 把上一条标过期**，于是 stale 这个标记变成常态，而它的意义是「看一眼再决定」。
///
/// **没验的**：压缩/编辑之后到底会不会产生一次**真正重叠的重读**（也就是第 2 条成立），
/// 取决于内核压缩时怎么遮蔽与重排序号，而那**本轮没有量过**。所以现在能说的是
/// 「口径 (a) 比修复前窄得多」，**不能**说「压缩一定会触发它」。
///
/// ## 为什么标记落在 `verification` 而不是新字段
///
/// 候选本来就有三档验证范围（`verified` / `unverified` / `stale`，C7 那一批落的），而
/// 「来源变了所以这条可能不准」正是 stale 的定义。复用它就**不需要第三个标记字段**，
/// 面板也已经有现成的展示路径。
///
/// ## 两条不许破的纪律
///
/// 1. **只标过期，不删也不拒收**。用户还要能看见那条候选并自己决定；设计稿那句
///    「不能覆写更新结果」指的是权威记录，而候选本来就不动它。
/// 2. **空指纹一律不判**。面板手工新建的来源没有指纹，旧版本写的候选也没有——拿空值去
///    比字符串会「全都不同」，于是**每来一条新候选就把所有旧候选标过期**。宁可漏标。
pub(crate) fn retire_stale_candidates(doc: &mut MemoryDoc, incoming: &MemorySource) -> usize {
    if incoming.content_hash.is_empty() || incoming.session.is_empty() {
        return 0;
    }
    let mut retired = 0;
    for candidate in doc.candidates.iter_mut() {
        if candidate.state != super::store::CANDIDATE_PENDING {
            continue;
        }
        if candidate.source.session != incoming.session || candidate.source.content_hash.is_empty()
        {
            continue;
        }
        if candidate.source.content_hash == incoming.content_hash {
            continue;
        }
        // 区间相交：`a.start <= b.end && b.start <= a.end`。用 u64 直接比，不做减法——
        // 那在「a.start > b.end」时会下溢，而下溢成一个巨大的正数会让这条判据恒真。
        if candidate.source.start_seq > incoming.end_seq
            || incoming.start_seq > candidate.source.end_seq
        {
            continue;
        }
        candidate.verification = VERIFICATION_STALE.to_string();
        retired += 1;
    }
    retired
}

/// 读回兼容：旧库写的是 `"source": {…}`，新库写 `"sources": [ … ]`，两种都读成列表。
pub(crate) fn one_or_many<'de, D>(deserializer: D) -> Result<Vec<MemorySource>, D::Error>
where
    D: Deserializer<'de>,
{
    // **数组先试**：反过来时 `MemorySource` 的派生实现也能吃序列（serde 的结构体允许按顺序
    // 读字段），`["x", "y"]` 会被读成一条 kind 为 "x" 的来源，而不是两条。
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        Many(Vec<MemorySource>),
        One(MemorySource),
    }
    match OneOrMany::deserialize(deserializer) {
        Ok(OneOrMany::Many(list)) => Ok(list),
        Ok(OneOrMany::One(one)) => Ok(vec![one]),
        Err(error) => Err(error),
    }
}

/// 一条来源的列表。`source` 为空就是「面板里手动写下的」——面板那条 `save` 不带来源，
/// 而它的来源就是用户的原话，把这个默认值写成一条**有名字的规则**而不是每个调用点各自
/// `unwrap_or_else` 一次。
pub(crate) fn one(source: Option<MemorySource>, at: &str) -> Vec<MemorySource> {
    vec![source.unwrap_or_else(|| MemorySource::manual(at))]
}

/// 来源里有一条是手动保存的就算「用户自己写的」——检索加权按这条加分（`query.rs`）。
pub(crate) fn any_manual(list: &[MemorySource]) -> bool {
    list.iter().any(|source| source.kind == SOURCE_MANUAL)
}

/// 并入一条来源，**返回是否真的多了一条**。
///
/// 三种情况都返回 `false`，而理由各不相同：
/// **没有来处**的（`kind` 为空）来源不收——记下来只会在列表里留一条说不清什么时候被并进去的
/// 记录；**已经在列表里**的不重复记；**满了**（[`MAX_SOURCES`]）的按「并不进去」处理——调用方
/// （`duplicates::merge_source`）于是把这次提交按「重复」拒收，而不是**悄悄丢掉这个来源**。
/// 列表被悄悄截断之后，用户看到的「来自哪」与实际发生的不符，而那正是这份列表存在的理由。
pub(crate) fn merge(list: &mut Vec<MemorySource>, source: &MemorySource) -> bool {
    if source.kind.trim().is_empty() || list.contains(source) || list.len() >= MAX_SOURCES {
        return false;
    }
    list.push(source.clone());
    true
}

/// 一条来源的说法（导出用。面板那边各有自己的短词，那边是给人看的，不复用这里的括号说明）。
pub(crate) fn label_of(source: &MemorySource) -> String {
    match source.kind.as_str() {
        SOURCE_MANUAL => "user-manual（手动保存）".to_string(),
        SOURCE_SESSION => "session（会话里提出）".to_string(),
        SOURCE_EXTRACT => "extract（自动提取）".to_string(),
        other => other.to_string(),
    }
}

/// 一条记录的说法：多个来源用 ` + ` 串起来（`user-manual（手动保存） + session（会话里提出）`）。
pub(crate) fn labels(list: &[MemorySource]) -> String {
    if list.is_empty() {
        return "未知来源".to_string();
    }
    list.iter().map(label_of).collect::<Vec<_>>().join(" + ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::store::{
        MemoryCandidate, CANDIDATE_PENDING, CANDIDATE_REJECTED, VERIFICATION_UNVERIFIED,
    };

    fn candidate(session: &str, start: u64, end: u64, hash: &str, state: &str) -> MemoryCandidate {
        MemoryCandidate {
            id: format!("c-{session}-{start}-{hash}"),
            state: state.to_string(),
            verification: VERIFICATION_UNVERIFIED.to_string(),
            source: MemorySource {
                kind: SOURCE_EXTRACT.to_string(),
                session: session.to_string(),
                start_seq: start,
                end_seq: end,
                content_hash: hash.to_string(),
                revision: "v1".to_string(),
                at: String::new(),
            },
            ..Default::default()
        }
    }

    fn incoming(session: &str, start: u64, end: u64, hash: &str) -> MemorySource {
        MemorySource {
            kind: SOURCE_EXTRACT.to_string(),
            session: session.to_string(),
            start_seq: start,
            end_seq: end,
            content_hash: hash.to_string(),
            revision: "v1".to_string(),
            at: String::new(),
        }
    }

    /// **口径 (a) 的主判据**：同会话 + 序号区间重叠 + 指纹不同 → 旧候选标 stale。
    ///
    /// 三条缺一不可，**所以这一条把三个「不成立」都并进来验**——分开写三条判据的话，
    /// 后改代码的人很容易只想着「让主判据保持绿」。
    #[test]
    fn same_session_overlapping_range_and_different_hash_retires_it() {
        // 传入来源的身份只写这一个常量。**第一版把它散成字面量**（传入 `"zzzz"`、
        // 「指纹相同」那条写成 `"dddd"`），于是夹具编码的东西与注释声称的不是一回事：
        // 那条其实**指纹不同**，被标是对的，而这条判据红在「只有第一条该被标」。
        // 判据骗过自己的又一例：注释不等于夹具，只有断言等式才说清「相同」指什么。
        const INCOMING_HASH: &str = "zzzz";
        let mut doc = MemoryDoc::default();
        doc.candidates
            .push(candidate("s1", 10, 84, "aaaa", CANDIDATE_PENDING));
        // 别的会话 —— 那是「另一件事」，不是改口。
        doc.candidates
            .push(candidate("s2", 10, 84, "bbbb", CANDIDATE_PENDING));
        // 序号不重叠 —— 那是「接着往后说」，不是重提。
        doc.candidates
            .push(candidate("s1", 200, 300, "cccc", CANDIDATE_PENDING));
        // 指纹与传入的**同一个常量** —— 内容没变，标过期就是假的。
        doc.candidates
            .push(candidate("s1", 10, 84, INCOMING_HASH, CANDIDATE_PENDING));
        // 空指纹 —— 旧版本写的候选，宁可漏标（判据见函数头那两条纪律）。
        doc.candidates
            .push(candidate("s1", 10, 84, "", CANDIDATE_PENDING));
        // 已决的 —— 用户已经处理过，不该被动。
        doc.candidates
            .push(candidate("s1", 10, 84, "eeee", CANDIDATE_REJECTED));

        let retired = retire_stale_candidates(&mut doc, &incoming("s1", 10, 84, INCOMING_HASH));
        assert_eq!(retired, 1, "只有第一条该被标");
        assert_eq!(doc.candidates[0].verification, VERIFICATION_STALE);
        for untouched in &doc.candidates[1..] {
            assert_eq!(
                untouched.verification, VERIFICATION_UNVERIFIED,
                "「{}」不该被标",
                untouched.source.content_hash
            );
        }
    }

    /// **区间相交的两端都要算重叠**：`a.start <= b.end && b.start <= a.end`。
    ///
    /// 这条钉住一个具体的坑：用减法判重叠（`a.start - b.end <= 0`）在 `a.start > b.end`
    /// 时会**下溢成一个巨大的正数**，于是那条判据变成恒假——表现为「相邻但不重叠的区间
    /// 也被标过期」，而那正是这条纪律要防的方向（误标比漏标坏）。
    ///
    /// 样例都对着固定的 `[20, 30]`，**每一对都得真的相交**才写进来：第一版塞了个
    /// `[10, 10]`，而它与 `[20, 30]` 压根不相交——夹具不成立，红的是夹具不是实现。
    #[test]
    fn the_touching_boundaries_still_count_as_overlapping() {
        // 跨过头、跨到尾、整体被包住、以及退化成一个点（`[25, 25]` 落在 `[20, 30]` 内）。
        for (start, end) in [(10u64, 84u64), (20, 84), (10, 30), (25, 25), (20, 30)] {
            let mut doc = MemoryDoc::default();
            doc.candidates
                .push(candidate("s1", start, end, "aaaa", CANDIDATE_PENDING));
            let retired = retire_stale_candidates(&mut doc, &incoming("s1", 20, 30, "zzzz"));
            assert_eq!(retired, 1, "[{start}, {end}] 与 [20, 30] 相交，该标");
        }
        // **紧挨着但不重叠的两个方向都要验**：只差一个序号。左邻（19→20）、右邻（30→31）、
        // 以及一路到头（0→19，覆盖 `start_seq` 为 0 的边界）。
        //
        // **这半边才是钉住下溢那条坑的方向**：一旦有人把判据改成减法（`a.start - b.end <= 0`），
        // 「不相交」会变成「相交」，误标比漏标坏——所以判据必须两边都验，只验相交那半边
        // 的话，减法版本照样全绿。
        for (start, end) in [(10u64, 19u64), (31, 60), (0, 19)] {
            let mut doc = MemoryDoc::default();
            doc.candidates
                .push(candidate("s1", start, end, "aaaa", CANDIDATE_PENDING));
            assert_eq!(
                retire_stale_candidates(&mut doc, &incoming("s1", 20, 30, "zzzz")),
                0,
                "[{start}, {end}] 与 [20,30] 不相交"
            );
        }
    }

    /// **没有指纹就不判**：拿空串去比字符串，「全都不同」，于是每来一条新候选就把所有旧候选
    /// 标过期——而面板手工新建的来源、旧版本写的候选都没有指纹。
    #[test]
    fn an_empty_hash_retires_nothing() {
        let mut doc = MemoryDoc::default();
        doc.candidates
            .push(candidate("s1", 10, 84, "aaaa", CANDIDATE_PENDING));
        assert_eq!(
            retire_stale_candidates(&mut doc, &incoming("s1", 10, 84, "")),
            0
        );
        assert_eq!(doc.candidates[0].verification, VERIFICATION_UNVERIFIED);
        // 面板手工保存的来源（`kind=user-manual`）压根没有会话，也一样不判。
        assert_eq!(
            retire_stale_candidates(&mut doc, &MemorySource::manual("2026-10-16T00:00:00Z")),
            0
        );
    }

    use crate::memory::store::MemoryRecord;

    #[test]
    fn legacy_single_source_reads_as_a_one_item_list() {
        let record = serde_json::from_str::<MemoryRecord>(
            r#"{"id":"m-1","title":"标题","body":"正文","source":{"kind":"session","session":"s-1"}}"#,
        )
        .expect("旧库的单对象来源也要读得进来");
        assert_eq!(record.sources.len(), 1);
        assert_eq!(record.sources[0].kind, SOURCE_SESSION);
        assert_eq!(record.sources[0].session, "s-1");
    }

    #[test]
    fn a_new_record_writes_an_array_even_with_one_source() {
        let record = serde_json::from_str::<MemoryRecord>(
            r#"{"id":"m-1","title":"标题","body":"正文","sources":[{"kind":"user-manual"},{"kind":"extract"}]}"#,
        )
        .expect("新库的数组来源");
        assert_eq!(record.sources.len(), 2);
        let text = serde_json::to_string(&record).expect("写回");
        assert!(text.contains("\"sources\":[{"), "写出去必须是数组：{text}");
        assert!(
            !text.contains("\"source\":"),
            "写出去不再有单对象那个键：{text}"
        );
    }

    #[test]
    fn a_record_without_any_source_is_not_an_error() {
        let record =
            serde_json::from_str::<MemoryRecord>(r#"{"id":"m-1","title":"标题","body":"正文"}"#)
                .expect("没有来源字段的老库也要读得进来");
        assert!(record.sources.is_empty());
        assert_eq!(labels(&record.sources), "未知来源");
    }

    #[test]
    fn merging_the_same_source_twice_adds_one() {
        let mut list = vec![MemorySource {
            kind: SOURCE_MANUAL.to_string(),
            ..MemorySource::default()
        }];
        let second = MemorySource {
            kind: SOURCE_SESSION.to_string(),
            session: "s-1".to_string(),
            ..MemorySource::default()
        };
        assert!(merge(&mut list, &second), "新来源应当并进去");
        assert!(!merge(&mut list, &second), "同一条来源不重复记");
        assert_eq!(list.len(), 2);
        assert!(any_manual(&list));
    }

    #[test]
    fn a_full_source_list_refuses_to_truncate_silently() {
        let mut list: Vec<MemorySource> = (0..MAX_SOURCES)
            .map(|index| MemorySource {
                kind: SOURCE_SESSION.to_string(),
                session: format!("s-{index}"),
                ..MemorySource::default()
            })
            .collect();
        assert!(!merge(
            &mut list,
            &MemorySource {
                kind: SOURCE_SESSION.to_string(),
                session: "s-new".to_string(),
                ..MemorySource::default()
            }
        ));
        assert_eq!(list.len(), MAX_SOURCES);
    }

    #[test]
    fn labels_join_and_keep_unknown_kinds() {
        assert_eq!(
            labels(&[
                MemorySource::manual("2026-10-15T00:00:00Z"),
                MemorySource {
                    kind: "future-kind".to_string(),
                    ..MemorySource::default()
                }
            ]),
            "user-manual（手动保存） + future-kind"
        );
    }

    #[test]
    fn an_array_shaped_payload_reads_as_a_list_even_under_the_legacy_key() {
        let record = serde_json::from_str::<MemoryRecord>(
            r#"{"id":"m-1","title":"标题","body":"正文","source":[{"kind":"a"},{"kind":"b"}]}"#,
        )
        .expect("数组形状按数组读");
        assert_eq!(record.sources.len(), 2);
    }
}
