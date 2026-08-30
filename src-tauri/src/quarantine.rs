//! 启动守卫已禁用的插件隔离注册表。
//!
//! `<data_dir>/quarantine.json` 记录那些破坏内核启动的插件：每条记录仍保留
//! 在中心仓库中，但会被排除在 profile 接入之外，从而使工作台能够在缺少该
//! 插件的情况下启动。记录中包含原因和支持该隔离决定的日志摘录，正是这些
//! 信息让管理 UI 可以呈现“保持禁用 / 重新启用 / 删除”的选项，而不是面对
//! 一个无法启动的工作台。检测流程位于 [`crate::guard`] 中。

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::process::atomic_write;

/// 外壳数据目录下隔离文档的文件名。
const QUARANTINE_FILE: &str = "quarantine.json";
/// 每次保存时盖上的 schema 版本号；读取端接受任何能解析的内容，
/// 并在下一次写入时重新规范化（发布前立场：不承诺格式兼容性）。
const SCHEMA_VERSION: u32 = 1;

/// 一条被隔离的插件记录。`id` 与中心仓库的 id 一致，便于接入过滤
/// 与插件管理 UI 关联到各自的记录行。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuarantineItem {
    pub id: String,
    pub name: String,
    /// 人类可读的隔离原因，在 UI 中按原文显示（简体中文）。
    pub reason: String,
    /// 支持该决定的日志摘录，在 UI 中按需显示。
    pub evidence: String,
    /// 自 epoch 起经过的秒数，仅用于显示。
    pub at: u64,
}

/// 持久化的隔离文档。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Quarantine {
    #[serde(rename = "schemaVersion", default)]
    pub schema_version: u32,
    #[serde(default)]
    pub items: Vec<QuarantineItem>,
}

fn file_path(data_dir: &Path) -> PathBuf {
    data_dir.join(QUARANTINE_FILE)
}

/// 读取隔离文档。文件缺失或无法解析等同于“没有隔离项”——即使文档已损坏，
/// 外壳也必须能够启动，并且破损的记录绝不能把某个插件永远从接入中隐藏。
pub fn load(data_dir: &Path) -> Quarantine {
    fs::read_to_string(file_path(data_dir))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// 持久化文档，并盖上当前的 schema 版本号。
pub fn save(data_dir: &Path, doc: &Quarantine) -> Result<(), AppError> {
    fs::create_dir_all(data_dir).map_err(|e| AppError::Io(e.to_string()))?;
    let mut normalized = doc.clone();
    normalized.schema_version = SCHEMA_VERSION;
    let text =
        serde_json::to_string_pretty(&normalized).map_err(|e| AppError::Io(e.to_string()))?;
    atomic_write(&file_path(data_dir), format!("{text}\n").as_bytes())
        .map_err(|e| AppError::Io(e.to_string()))
}

/// 当前被隔离的 id 集合——profile 接入过滤所消费的形式。
pub fn ids(data_dir: &Path) -> HashSet<String> {
    load(data_dir)
        .items
        .into_iter()
        .map(|item| item.id)
        .collect()
}

/// 按 id 写入或更新记录，保留首次出现的顺序，使 UI 列表在重复检测时保持稳定。
pub fn add_all(data_dir: &Path, incoming: &[QuarantineItem]) -> Result<(), AppError> {
    let mut doc = load(data_dir);
    for item in incoming {
        match doc.items.iter_mut().find(|existing| existing.id == item.id) {
            Some(existing) => *existing = item.clone(),
            None => doc.items.push(item.clone()),
        }
    }
    save(data_dir, &doc)
}

/// 删除一条记录。在用户重新启用或删除插件时调用；
/// 删除一个不存在的 id 已经是期望的状态，因此会成功。
pub fn remove(data_dir: &Path, id: &str) -> Result<(), AppError> {
    let mut doc = load(data_dir);
    doc.items.retain(|item| item.id != id);
    save(data_dir, &doc)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str) -> QuarantineItem {
        QuarantineItem {
            id: id.to_string(),
            name: format!("plugin-{id}"),
            reason: "测试原因".to_string(),
            evidence: "line".to_string(),
            at: 1,
        }
    }

    #[test]
    fn load_missing_file_is_empty() {
        let dir = std::env::temp_dir().join(format!(
            "dsh-quarantine-test-missing-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&dir);
        assert!(load(&dir).items.is_empty());
        assert!(ids(&dir).is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn add_remove_round_trip() {
        let dir = std::env::temp_dir().join(format!(
            "dsh-quarantine-test-roundtrip-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&dir);

        add_all(&dir, &[item("a"), item("b")]).expect("add");
        assert_eq!(ids(&dir), HashSet::from(["a".to_string(), "b".to_string()]));

        // 按 id 写入或更新，使每个插件只保留一条记录并刷新其字段。
        let updated = QuarantineItem {
            reason: "新原因".to_string(),
            ..item("a")
        };
        add_all(&dir, &[updated]).expect("upsert");
        let doc = load(&dir);
        assert_eq!(doc.items.len(), 2);
        assert_eq!(
            doc.items.iter().find(|i| i.id == "a").unwrap().reason,
            "新原因"
        );
        assert_eq!(doc.schema_version, SCHEMA_VERSION);

        remove(&dir, "a").expect("remove");
        assert_eq!(ids(&dir), HashSet::from(["b".to_string()]));
        // 删除一个不存在的 id 已经是期望的状态。
        remove(&dir, "a").expect("remove absent");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn unparsable_document_reads_as_empty() {
        let dir = std::env::temp_dir().join(format!(
            "dsh-quarantine-test-garbage-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).expect("mkdir");
        fs::write(file_path(&dir), "{not json").expect("write garbage");
        assert!(load(&dir).items.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }
}
