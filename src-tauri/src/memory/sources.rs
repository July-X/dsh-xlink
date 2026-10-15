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

use super::store::{MemorySource, SOURCE_EXTRACT, SOURCE_MANUAL, SOURCE_SESSION};

/// 一条记录最多记几个来源。超出的按「并不进去」处理——见 [`merge`]。
pub(crate) const MAX_SOURCES: usize = 8;

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
