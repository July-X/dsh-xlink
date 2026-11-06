//! P0 only. Launch via e2e/minimax-webui-probe.mjs to get an isolated profile.

use serde_json::{json, Value};
use std::{path::PathBuf, time::Duration};
use tauri::{webview::NewWindowResponse, Manager, Url, WebviewUrl, WebviewWindowBuilder};

fn evidence(value: Value) {
    println!("MCODE_PROBE {value}");
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::var("DSH_E2E_RUN_ROOT")?).canonicalize()?;
    let profile = root.join("browser").canonicalize()?;
    let app_data = root.join("appdata").canonicalize()?;
    let home = PathBuf::from(std::env::var("DSH_XLINK_HOME")?).canonicalize()?;
    let channel = std::env::var("DSH_XLINK_CHANNEL_SUFFIX")?;
    let mut context = tauri::test::mock_context(tauri::test::noop_assets());
    let config: Value = serde_json::from_str(option_env!("TAURI_CONFIG").unwrap_or("{}"))?;
    let override_root = config
        .pointer("/app/appDirectoriesOverride")
        .and_then(Value::as_str)
        .ok_or("missing compiled appDirectoriesOverride")?;
    if PathBuf::from(override_root).canonicalize()? != app_data {
        return Err("compiled appDirectoriesOverride does not match this run".into());
    }
    context.config_mut().identifier = "com.zhongxingxing.dsh-xlink.mcode-probe".into();
    context.config_mut().app.app_directories_override = Some(
        tauri::utils::config::AppDirectoriesOverride::Root(app_data.clone()),
    );
    if !profile.starts_with(&root)
        || !app_data.starts_with(&root)
        || home != root.join("home")
        || root.file_name().and_then(|name| name.to_str()) != Some(channel.as_str())
        || !root.join(".mcode-probe").is_file()
    {
        return Err("probe isolation preflight failed".into());
    }
    let port: u16 = std::env::var("WEBUI_SERVER_PORT")?.parse()?;
    let url = Url::parse(&format!("http://127.0.0.1:{port}/"))?;
    let smoke = std::env::args().any(|arg| arg == "--smoke");
    let store_id: [u8; 16] = serde_json::from_str(&std::env::var("MCODE_PROBE_STORE_ID")?)?;
    let app = tauri::Builder::default()
        .setup(move |app| {
            if app.path().app_local_data_dir()?.canonicalize()? != app_data {
                return Err("compiled appDirectoriesOverride does not match this run".into());
            }
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let result = open_probe(&handle, url, profile, store_id, smoke);
                if result.is_err() {
                    evidence(json!({"event": "window_error"}));
                    handle.exit(1);
                }
            });
            Ok(())
        })
        .build(context)?;
    app.run(|_, _| {});
    Ok(())
}

fn open_probe(
    app: &tauri::AppHandle,
    url: Url,
    profile: PathBuf,
    store_id: [u8; 16],
    smoke: bool,
) -> tauri::Result<()> {
    let origin = url.origin().ascii_serialization();
    let navigation_origin = origin.clone();
    let popup_app = app.clone();
    let popup_profile = profile.clone();
    let builder = WebviewWindowBuilder::new(app, "mcode-probe", WebviewUrl::External(url))
        .title("MiniMax 工作台 P0 · 隔离验证")
        .inner_size(1200.0, 820.0)
        .data_directory(profile)
        .initialization_script(include_str!("minimax_webui_probe_observe.js"))
        .on_navigation(move |target| {
            let allowed = target.origin().ascii_serialization() == navigation_origin;
            if !allowed {
                evidence(json!({"event": "main_navigation_denied", "origin": target.origin().ascii_serialization()}));
            }
            allowed
        })
        .on_new_window(move |target, features| {
            if !popup_allowed(&target, &origin) {
                evidence(json!({"event": "popup_denied", "origin": target.origin().ascii_serialization()}));
                return NewWindowResponse::Deny;
            }
            evidence(json!({"event": "popup_open", "origin": target.origin().ascii_serialization()}));
            let popup_origin = origin.clone();
            let popup = WebviewWindowBuilder::new(&popup_app, "mcode-probe-auth", WebviewUrl::External(target))
                .window_features(features)
                .title("MiniMax P0 · 测试账号登录")
                .data_directory(popup_profile.clone())
                .on_navigation(move |target| {
                    let allowed = popup_allowed(target, &popup_origin);
                    evidence(json!({"event": "popup_navigation", "origin": target.origin().ascii_serialization(), "allowed": allowed}));
                    allowed
                });
            #[cfg(target_os = "macos")]
            let popup = popup.data_store_identifier(store_id);
            match popup.build() {
                Ok(window) => NewWindowResponse::Create { window },
                Err(_) => {
                    evidence(json!({"event": "popup_build_failed"}));
                    NewWindowResponse::Deny
                }
            }
        });
    #[cfg(target_os = "macos")]
    let builder = builder.data_store_identifier(store_id);
    #[cfg(not(target_os = "macos"))]
    let _ = store_id;
    let window = builder.build()?;
    evidence(json!({"event": "window_open", "webview": tauri::webview_version().ok()}));
    let exit = app.clone();
    window.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::Destroyed) {
            exit.exit(0);
        }
    });
    let mut prior = Value::Null;
    let mut ticks = 0;
    while app.get_webview_window("mcode-probe").is_some() {
        std::thread::sleep(Duration::from_millis(500));
        let (tx, rx) = std::sync::mpsc::channel();
        let evaluation =
            window.eval_with_callback("globalThis.__mcodeProbe?.() ?? null", move |text| {
                let _ = tx.send(text);
            });
        if app.get_webview_window("mcode-probe").is_none() {
            return Ok(());
        }
        evaluation?;
        let scene = rx
            .recv_timeout(Duration::from_secs(2))
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .filter(Value::is_object);
        if let Some(scene) = scene {
            if scene != prior {
                evidence(json!({"event": "page", "scene": scene}));
                prior = scene;
            }
            if smoke && prior["connected"] == true && prior["rendered"] == true {
                app.exit(0);
                return Ok(());
            }
        }
        ticks += 1;
        if ticks == 60 && (prior["connected"] != true || prior["rendered"] != true) {
            evidence(json!({"event": "page_timeout"}));
            if smoke {
                app.exit(1);
                return Ok(());
            }
        }
    }
    Ok(())
}

fn popup_allowed(url: &Url, origin: &str) -> bool {
    // This discovery probe records origins only. P2 derives its narrower policy from P0.
    url.origin().ascii_serialization() == origin
        || url.scheme() == "https"
        || url.as_str() == "about:blank"
}
