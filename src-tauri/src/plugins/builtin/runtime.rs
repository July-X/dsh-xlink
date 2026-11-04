//! 每次启动内核前，把已启用插件刷新到当前壳携带的版本；停用意图不被覆盖。
//!
//! **遍历全部内置插件**（2026-10-12）：过去这里只管 openai-oauth 一个。第二个
//! 内置插件接进来时若忘了改这一处，它在应用升级后拿不到新版本——症状是
//! 「开关显示已启用，工作台里却是旧行为」，没有任何报错。
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use super::builtins::BuiltinSpec;

/// 各内置插件的资源目录（app handle 只能取一次，见 `configure`）。
static SOURCES: OnceLock<HashMap<&'static str, PathBuf>> = OnceLock::new();

pub(crate) fn configure(app: &tauri::AppHandle) {
    let map = super::builtins::BUILTINS
        .iter()
        .chain(std::iter::once(&super::builtins::SHELL_NAVIGATION))
        .filter_map(|spec| super::cmd::resolve_plugin_source(app, spec).map(|path| (spec.id, path)))
        .collect();
    let _ = SOURCES.set(map);
}

/// 每个插件各记一条事件：一次启动要能分清是谁刷新了、谁被跳过。事件名带
/// 插件 id，查日志时不必从文案里认。
fn record(id: &str, message: String) {
    crate::shell::shell_events::record(&format!("builtin-{id}-refresh"), &message);
}

pub(crate) fn refresh_enabled(home: &Path, profile: &str, kernel: &Path) -> Result<(), String> {
    let sources = SOURCES.get();
    let navigation = &super::builtins::SHELL_NAVIGATION;
    let navigation_available = ["dsh-client-ui-workspace", "dsh-api-session-controller"]
        .iter()
        .all(|package| {
            kernel
                .join("node_modules/@deepseek-ai")
                .join(package)
                .join("package.json")
                .is_file()
        });
    if !navigation_available {
        super::ensure_unwired(navigation, home, profile)?;
        record(
            navigation.id,
            "当前内核缺少通知导航所需客户端模块，已摘除内部接线；工作台仍可使用".into(),
        );
    } else if let Some(source) = sources.and_then(|map| map.get(navigation.id)) {
        if let Err(error) = super::ensure_wired(navigation, source, home, profile, kernel) {
            record(navigation.id, format!("通知导航接入失败：{error}"));
        }
    } else {
        record(
            navigation.id,
            "缺少内部通知导航资源，请重新构建或安装 dsh-xlink".into(),
        );
    }
    for spec in super::builtins::BUILTINS {
        let started = std::time::Instant::now();
        // 稳态每次启动也做两遍全树摘要 + 接线读取，此前只有真变更才落一行，这
        // 笔每启动一次的固定耗时在日志里完全不可见。启动是用户触发的低频动作，
        // 一行/次可忽略；插件包膨胀或物化变慢时有据可查。失败原样上抛不阻断记。
        let outcome = refresh_from_source(
            spec,
            sources
                .and_then(|map| map.get(spec.id))
                .map(PathBuf::as_path),
            home,
            profile,
            kernel,
        );
        let ms = started.elapsed().as_millis();
        record(
            spec.id,
            match &outcome {
                Ok(()) => format!("启动前内嵌插件 {} 检查完成，耗时 {ms}ms", spec.id),
                Err(error) => {
                    format!(
                        "启动前内嵌插件 {} 刷新失败（耗时 {ms}ms）：{error}",
                        spec.id
                    )
                }
            },
        );
        outcome?;
    }
    Ok(())
}

fn refresh_from_source(
    spec: &'static BuiltinSpec,
    source: Option<&Path>,
    home: &Path,
    profile: &str,
    kernel: &Path,
) -> Result<(), String> {
    let mut entries = super::state::load(home, spec)?;
    let key = super::state::state_key(crate::shell::settings::current_mode().as_str(), profile);
    let Some(entry) = entries
        .get_mut(&key)
        .filter(|entry| entry.requested_enabled)
    else {
        return Ok(());
    };
    let source = source.ok_or_else(|| {
        format!(
            "找不到已启用的内嵌插件 {} 的资源；请重新构建或安装 dsh-xlink 后启动工作台",
            spec.id
        )
    })?;
    let wired = super::ensure_wired(spec, source, home, profile, kernel)?;
    if entry.plugin_version.as_ref() != Some(&wired.plugin_version)
        || entry.fingerprint.as_ref() != Some(&wired.fingerprint)
    {
        entry.plugin_version = Some(wired.plugin_version);
        entry.fingerprint = Some(wired.fingerprint);
        entry.updated_at_ms = crate::shell::process::epoch_millis();
        super::state::save(home, spec, &entries)?;
        record(spec.id, format!("启动前已刷新内嵌插件 {} 与接线", spec.id));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::builtin::builtins;
    use std::fs;

    fn spec() -> &'static BuiltinSpec {
        builtins::find(builtins::OPENAI_OAUTH).expect("openai-oauth 必须在登记表里")
    }

    /// 按 spec 的文件集摆一个源目录（不再写死 `host/client/locales`）。
    fn write_source(source: &Path, spec: &BuiltinSpec, version: &str) {
        for entry in spec.entries {
            if *entry == "package.json" {
                fs::create_dir_all(source).unwrap();
                fs::write(
                    source.join("package.json"),
                    format!("{{\"version\":\"{version}\"}}"),
                )
                .unwrap();
            } else {
                fs::create_dir_all(source.join(entry)).unwrap();
            }
        }
        fs::write(source.join("host/index.js"), "export {}").unwrap();
    }

    /// 按**全部内置插件**的 peer 摆内核包：夹具不该因为登记表多了一个插件
    /// 就失败——那会让「加第二个内置插件」先撞上一个无关的红。
    fn write_kernel(kernel: &Path) {
        for package in ["dsh", "cordis"] {
            let dir = kernel.join("node_modules/@deepseek-ai").join(package);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("package.json"), r#"{"version":"1.0.0"}"#).unwrap();
        }
        for builtin in builtins::BUILTINS {
            for peer in builtin.peers {
                let dir = kernel.join("node_modules/@deepseek-ai").join(peer.package);
                fs::create_dir_all(&dir).unwrap();
                if !dir.join("package.json").exists() {
                    fs::write(dir.join("package.json"), r#"{"version":"1.0.0"}"#).unwrap();
                }
            }
        }
    }

    #[test]
    fn shell_navigation_source_is_materialized_byte_for_byte_without_user_enablement() {
        let root = std::env::temp_dir().join(format!(
            "builtin-navigation-{}-{}",
            std::process::id(),
            crate::shell::process::epoch_millis()
        ));
        let home = root.join("home");
        let _guard = crate::tests::scoped_xlink_home(&home);
        let kernel = root.join("kernel");
        write_kernel(&kernel);
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("plugins/shell-navigation");
        let spec = &builtins::SHELL_NAVIGATION;
        let wired =
            crate::plugins::builtin::ensure_wired(spec, &source, &home, "web", &kernel).unwrap();
        for entry in ["package.json", "host/index.js", "client/client.js"] {
            assert_eq!(
                fs::read(wired.dir.join(entry)).unwrap(),
                fs::read(source.join(entry)).unwrap(),
                "{entry}"
            );
        }
        let patch = fs::read_to_string(home.join("profiles/web/cordis.patch.yml")).unwrap();
        assert!(patch.contains(spec.row_id));
        assert!(
            !builtins::BUILTINS.iter().any(|item| item.id == spec.id),
            "内部模块不能增加用户插件开关"
        );
        // 切回不含公开客户端模块的内核时，必须摘掉旧接线而非破坏工作台模块图。
        refresh_enabled(&home, "web", &kernel).unwrap();
        let patch = fs::read_to_string(home.join("profiles/web/cordis.patch.yml")).unwrap();
        assert!(!patch.contains(spec.row_id));
    }

    #[test]
    fn enabled_launch_updates_wiring_state_and_same_version_bytes() {
        let root = std::env::temp_dir().join(format!(
            "builtin-runtime-{}-{}",
            std::process::id(),
            crate::shell::process::epoch_millis()
        ));
        let home = root.join("home");
        let _guard = crate::tests::scoped_xlink_home(&home);
        let spec = spec();
        let source = root.join("source");
        let kernel = root.join("kernel");
        write_source(&source, spec, "0.1.10");
        write_kernel(&kernel);
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
        super::super::state::save(&home, spec, &entries).unwrap();
        assert!(refresh_from_source(spec, None, &home, "web", &kernel).is_err());
        refresh_from_source(spec, Some(&source), &home, "web", &kernel).unwrap();
        let updated = super::super::state::load(&home, spec).unwrap();
        assert_eq!(updated[&key].plugin_version.as_deref(), Some("0.1.10"));
        let patch = fs::read_to_string(home.join("profiles/web/cordis.patch.yml")).unwrap();
        assert!(patch.contains("openai-oauth/0.1.10/"));
        let target = home
            .join(spec.instance_base_segment())
            .join("0.1.10/dsh-1.0.0-cordis-1.0.0/compat-a/host/index.js");
        fs::write(source.join("host/index.js"), "export const images = true").unwrap();
        refresh_from_source(spec, Some(&source), &home, "web", &kernel).unwrap();
        assert_eq!(
            fs::read_to_string(target).unwrap(),
            "export const images = true"
        );
        entries.get_mut(&key).unwrap().requested_enabled = false;
        super::super::state::save(&home, spec, &entries).unwrap();
        refresh_from_source(spec, None, &home, "web", &kernel).unwrap();
        assert_eq!(
            fs::read_to_string(home.join("profiles/web/cordis.patch.yml")).unwrap(),
            patch
        );
        fs::remove_dir_all(&root).unwrap();
    }

    /// 第二个内置插件的启停意图也必须在启动前被刷新。
    ///
    /// 这是「`refresh_enabled` 遍历全部内置插件」的反向验：把循环改回只处理
    /// openai-oauth 时，本条立刻转红——而症状在真机上只是「Ponytail 升级后
    /// 没生效」，没有任何一行日志说它被跳过了（`refresh_enabled` 不落
    /// 「未启用」的日志，那种行会把日志灌满）。
    #[test]
    fn a_second_builtin_is_also_refreshed_on_launch() {
        let root = std::env::temp_dir().join(format!(
            "builtin-runtime-two-{}-{}",
            std::process::id(),
            crate::shell::process::epoch_millis()
        ));
        let home = root.join("home");
        let _guard = crate::tests::scoped_xlink_home(&home);
        let ponytail = builtins::find(builtins::PONYTAIL).unwrap();
        let source = root.join("source-ponytail");
        let kernel = root.join("kernel");
        write_source(&source, ponytail, "2.0.0");
        write_kernel(&kernel);
        // 上游内容：有上游的插件没有它就不许物化（空壳在界面上看不出异常）。
        super::super::upstream::seed_fixture(&home, ponytail);

        let key =
            super::super::state::state_key(crate::shell::settings::current_mode().as_str(), "web");
        let mut entries = std::collections::HashMap::new();
        entries.insert(
            key.clone(),
            super::super::state::StateEntry {
                requested_enabled: true,
                updated_at_ms: 0,
                plugin_version: Some("1.9.0".into()),
                fingerprint: None,
            },
        );
        super::super::state::save(&home, ponytail, &entries).unwrap();

        refresh_from_source(ponytail, Some(&source), &home, "web", &kernel).unwrap();
        let updated = super::super::state::load(&home, ponytail).unwrap();
        assert_eq!(updated[&key].plugin_version.as_deref(), Some("2.0.0"));
        let patch = fs::read_to_string(home.join("profiles/web/cordis.patch.yml")).unwrap();
        assert!(patch.contains("id: xlink-ponytail"), "{patch}");
        assert!(patch.contains("ponytail/2.0.0/"));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn disabled_does_not_require_or_materialize_resources() {
        let home =
            std::env::temp_dir().join(format!("builtin-runtime-disabled-{}", std::process::id()));
        let _guard = crate::tests::scoped_xlink_home(&home);
        refresh_from_source(spec(), None, &home, "web", Path::new("missing-kernel")).unwrap();
        assert!(!home.exists());
    }
}
