//! Persisted desktop-shell settings.
//!
//! Settings live in `<data_dir>/settings.json` as a flat JSON struct so the
//! UI can read and update them through ordinary command round-trips
//! (`<data_dir>` is `<dsh_home>/desktop[-dev]/`, see [`crate::kernel::data_dir`]).

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// The port the management UI expects the dsh web server on when the
/// user has not yet persisted a value of their own. Re-exports
/// [`crate::kernel::DEFAULT_PORT`] so the two definitions cannot drift —
/// debug builds (3091) and release builds (3090) agree on the same
/// fallback.
pub use crate::kernel::DEFAULT_PORT;

/// User-facing configuration the desktop shell needs to run a kernel.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Explicit path to the `node` executable; when empty the shell detects
    /// it from the environment.
    pub node_path: Option<String>,
    /// Explicit path to the `pnpm` executable; when empty it is resolved next
    /// to `node` or from the environment. pnpm installs kernel versions.
    pub pnpm_path: Option<String>,
    /// Explicit path to the `npm` executable; when empty it is resolved next
    /// to `node` or from the environment. npm is the auto-install fallback
    /// when pnpm is missing, so a custom install (portable layout, nvm
    /// without the node-sibling npm) needs this slot to skip a wasted probe.
    pub npm_path: Option<String>,
    /// Port the kernel's web UI listens on (dsh defaults to 3080).
    pub port: u16,
    /// Profile name the shell wires plugins into (dsh default: web).
    pub profile: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            node_path: None,
            pnpm_path: None,
            npm_path: None,
            port: DEFAULT_PORT,
            profile: crate::plugins::DEFAULT_PROFILE.to_string(),
        }
    }
}

/// Path of the settings file under `data_dir`.
pub fn settings_file(data_dir: &Path) -> std::path::PathBuf {
    data_dir.join("settings.json")
}

/// Read settings, returning defaults when the file is missing or unreadable.
pub fn load(data_dir: &Path) -> Settings {
    let path = settings_file(data_dir);
    fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Persist settings, creating the parent directory when needed.
pub fn save(data_dir: &Path, settings: &Settings) -> Result<(), String> {
    let path = settings_file(data_dir);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    fs::write(&path, text).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The UI sends only `{port, profile}`; Tauri deserializes the `settings`
    /// arg with serde_json::from_value just like this.
    #[test]
    fn ui_payload_deserializes() {
        let v = serde_json::json!({"port": 8080, "profile": "web"});
        let s: Settings = serde_json::from_value(v).unwrap();
        assert_eq!(s.port, 8080);
        assert_eq!(s.node_path, None);
    }

    /// Ports outside u16 are rejected at the IPC boundary; the UI validates
    /// 1024–65535 before sending so the user gets an actionable message.
    #[test]
    fn out_of_range_port_rejected() {
        let v = serde_json::json!({"port": 70000, "profile": "web"});
        assert!(serde_json::from_value::<Settings>(v).is_err());
    }
}
