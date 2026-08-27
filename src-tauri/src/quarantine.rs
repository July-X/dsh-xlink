//! Quarantine registry for plugins the boot guard has disabled.
//!
//! `<data_dir>/quarantine.json` records plugins that broke kernel boot: each
//! item stays installed in the central store but is excluded from profile
//! wiring, so the workbench can start without it. Records carry the reason
//! and the log excerpt that justified the isolation, which is what lets the
//! management UI present a keep-disabled / re-enable / remove choice instead
//! of a dead workbench. The detection flow lives in [`crate::guard`].

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::AppError;

/// Quarantine document file name under the shell data dir.
const QUARANTINE_FILE: &str = "quarantine.json";
/// Schema version stamped on every save; readers accept anything they can
/// parse and renormalize on the next write (pre-release stance: no format
/// compatibility promise).
const SCHEMA_VERSION: u32 = 1;

/// One isolated plugin. `id` matches the central store id so wiring filters
/// and the plugin management UI can join against their own rows.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuarantineItem {
    pub id: String,
    pub name: String,
    /// Human-readable isolation reason shown verbatim in the UI (简体中文).
    pub reason: String,
    /// Log excerpt backing the decision, shown on demand in the UI.
    pub evidence: String,
    /// Seconds since epoch, for display.
    pub at: u64,
}

/// The persisted quarantine document.
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

/// Read the quarantine document. A missing or unparsable file means "nothing
/// is quarantined" — the shell must still boot when the document is damaged,
/// and a broken record must never permanently hide a plugin from wiring.
pub fn load(data_dir: &Path) -> Quarantine {
    fs::read_to_string(file_path(data_dir))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Persist the document, stamping the current schema version.
pub fn save(data_dir: &Path, doc: &Quarantine) -> Result<(), AppError> {
    fs::create_dir_all(data_dir).map_err(|e| AppError::Io(e.to_string()))?;
    let mut normalized = doc.clone();
    normalized.schema_version = SCHEMA_VERSION;
    let text =
        serde_json::to_string_pretty(&normalized).map_err(|e| AppError::Io(e.to_string()))?;
    fs::write(file_path(data_dir), text + "\n").map_err(|e| AppError::Io(e.to_string()))
}

/// Ids currently quarantined — the shape profile-wiring filters consume.
pub fn ids(data_dir: &Path) -> HashSet<String> {
    load(data_dir)
        .items
        .into_iter()
        .map(|item| item.id)
        .collect()
}

/// Upsert records by id, preserving first-appearance order so the UI list is
/// stable across re-detections.
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

/// Drop one record. Called when the user re-enables or removes the plugin;
/// removing an absent id is already the requested state, so it succeeds.
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

        // Upsert by id keeps one record per plugin and refreshes its fields.
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
        // Removing an absent id is already the requested state.
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
