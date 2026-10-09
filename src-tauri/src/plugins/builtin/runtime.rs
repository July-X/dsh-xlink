//! 每次启动内核前，把已启用插件刷新到当前壳携带的版本；停用意图不被覆盖。
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static SOURCE: OnceLock<Option<PathBuf>> = OnceLock::new();

pub(crate) fn configure(app: &tauri::AppHandle) {
    let _ = SOURCE.set(super::cmd::resolve_plugin_source(app));
}

pub(crate) fn refresh_enabled(home: &Path, profile: &str, kernel: &Path) -> Result<(), String> {
    refresh_from_source(
        SOURCE.get().and_then(Option::as_deref),
        home,
        profile,
        kernel,
    )
}

fn refresh_from_source(
    source: Option<&Path>,
    home: &Path,
    profile: &str,
    kernel: &Path,
) -> Result<(), String> {
    let mut entries = super::state::load(home)?;
    let key = super::state::state_key(crate::shell::settings::current_mode().as_str(), profile);
    let Some(entry) = entries
        .get_mut(&key)
        .filter(|entry| entry.requested_enabled)
    else {
        return Ok(());
    };
    let source =
        source.ok_or("找不到已启用的内嵌插件资源；请重新构建或安装 dsh-xlink 后启动工作台")?;
    let wired = super::ensure_wired(source, home, profile, kernel)?;
    if entry.plugin_version.as_ref() != Some(&wired.plugin_version)
        || entry.fingerprint.as_ref() != Some(&wired.fingerprint)
    {
        entry.plugin_version = Some(wired.plugin_version);
        entry.fingerprint = Some(wired.fingerprint);
        entry.updated_at_ms = crate::shell::process::epoch_millis();
        super::state::save(home, &entries)?;
        crate::shell::shell_events::record(
            "builtin-openai-refresh",
            "启动前已刷新内嵌 OpenAI 插件与接线",
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn enabled_launch_updates_wiring_state_and_same_version_bytes() {
        let root = std::env::temp_dir().join(format!(
            "builtin-runtime-{}-{}",
            std::process::id(),
            crate::shell::process::epoch_millis()
        ));
        let home = root.join("home");
        let _guard = crate::tests::scoped_xlink_home(&home);
        let source = root.join("source");
        let kernel = root.join("kernel");
        for entry in ["host", "client", "locales"] {
            fs::create_dir_all(source.join(entry)).unwrap();
        }
        fs::write(source.join("package.json"), r#"{"version":"0.1.10"}"#).unwrap();
        fs::write(source.join("host/index.js"), "export {}").unwrap();
        for package in ["dsh", "cordis", "dsh-llm", "schemastery"] {
            let dir = kernel.join("node_modules/@deepseek-ai").join(package);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("package.json"), r#"{"version":"1.0.0"}"#).unwrap();
        }
        let key =
            super::super::state::state_key(crate::shell::settings::current_mode().as_str(), "web");
        let mut entries = std::collections::HashMap::new();
        entries.insert(
            key.clone(),
            super::super::state::StateEntry {
                requested_enabled: true,
                updated_at_ms: 0,
                plugin_version: Some("0.1.9".into()),
                fingerprint: None,
            },
        );
        super::super::state::save(&home, &entries).unwrap();
        assert!(refresh_from_source(None, &home, "web", &kernel).is_err());
        refresh_from_source(Some(&source), &home, "web", &kernel).unwrap();
        let updated = super::super::state::load(&home).unwrap();
        assert_eq!(updated[&key].plugin_version.as_deref(), Some("0.1.10"));
        let patch = fs::read_to_string(home.join("profiles/web/cordis.patch.yml")).unwrap();
        assert!(patch.contains("openai-oauth/0.1.10/"));
        let target = home
            .join(super::super::INSTANCE_BASE_SEGMENT)
            .join("0.1.10/dsh-1.0.0-cordis-1.0.0/compat-a/host/index.js");
        fs::write(source.join("host/index.js"), "export const images = true").unwrap();
        refresh_from_source(Some(&source), &home, "web", &kernel).unwrap();
        assert_eq!(
            fs::read_to_string(target).unwrap(),
            "export const images = true"
        );
        entries.get_mut(&key).unwrap().requested_enabled = false;
        super::super::state::save(&home, &entries).unwrap();
        refresh_from_source(None, &home, "web", &kernel).unwrap();
        assert_eq!(
            fs::read_to_string(home.join("profiles/web/cordis.patch.yml")).unwrap(),
            patch
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn disabled_does_not_require_or_materialize_resources() {
        let home =
            std::env::temp_dir().join(format!("builtin-runtime-disabled-{}", std::process::id()));
        let _guard = crate::tests::scoped_xlink_home(&home);
        refresh_from_source(None, &home, "web", Path::new("missing-kernel")).unwrap();
        assert!(!home.exists());
    }
}
