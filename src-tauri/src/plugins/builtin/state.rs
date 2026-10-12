//! 启用意图的落盘（开发计划 §3 / §4.1）。
//!
//! `state.json` 放在**每个插件各自的**物化根下（`extensions/builtin/<id>/`），
//! 按「壳模式 / profile」分键：同一实例可能被两个壳的注册表各自指向，
//! profile 也可切换，意图必须是这两个坐标的组合，不能只有一份布尔。
//!
//! **按插件分文件而不是在一份文件里按 id 分键**（2026-10-12）：文件本身的
//! 路径就是插件 id，两个插件的启停因此不会互相覆盖，而 openai-oauth 既有
//! 用户的 `state.json` 原地仍然是那一个文件（`spec.instance_base_segment()`
//! 对它返回的还是老路径，见 registry 的说明）。
//!
//! 损坏的文件**不按空文件处理**（开发计划 §6.6 的纪律同样适用于我们自己
//! 的状态）：解析失败返回错误，由用户检查后重试——静默重置会把「用户要
//! 开」悄悄变成「关」。字段用 camelCase，与命令载荷同一风格。

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::builtins::BuiltinSpec;

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StateEntry {
    pub(crate) requested_enabled: bool,
    /// 意图落定的毫秒时间戳（`shell::process::epoch_millis`）。
    pub(crate) updated_at_ms: u64,
    /// 启用成功时记录的插件版本与内核指纹（停用后保留上次值，便于诊断）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) plugin_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) fingerprint: Option<String>,
}

pub(crate) fn state_path(dsh_home: &Path, spec: &BuiltinSpec) -> PathBuf {
    dsh_home
        .join(spec.instance_base_segment())
        .join("state.json")
}

/// 分键：`<mode>/<profile>`。
pub(crate) fn state_key(mode: &str, profile: &str) -> String {
    format!("{mode}/{profile}")
}

fn parse(text: &str, path: &Path) -> Result<HashMap<String, StateEntry>, String> {
    let parsed: HashMap<String, StateEntry> = serde_json::from_str(text).map_err(|error| {
        format!(
            "内嵌插件状态文件损坏（{}）：{error}；请手工检查该文件后重试，不会自动重置你的选择",
            path.display()
        )
    })?;
    Ok(parsed)
}

/// 读取全部意图；文件不存在视为空（从未启用过）。
pub(crate) fn load(
    dsh_home: &Path,
    spec: &BuiltinSpec,
) -> Result<HashMap<String, StateEntry>, String> {
    let path = state_path(dsh_home, spec);
    match fs::read_to_string(&path) {
        Ok(text) => parse(&text, &path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(HashMap::new()),
        Err(error) => Err(format!(
            "读取内嵌插件状态文件失败（{}）：{error}",
            path.display()
        )),
    }
}

/// 原子写回全部意图。
pub(crate) fn save(
    dsh_home: &Path,
    spec: &BuiltinSpec,
    entries: &HashMap<String, StateEntry>,
) -> Result<(), String> {
    let path = state_path(dsh_home, spec);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("创建内嵌插件状态目录失败（{}）：{e}", parent.display()))?;
    }
    let text = format!(
        "{}\n",
        serde_json::to_string_pretty(entries).unwrap_or_default()
    );
    crate::shell::process::atomic_write(&path, text.as_bytes())
        .map_err(|e| format!("写入内嵌插件状态文件失败（{}）：{e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::builtin::builtins;

    fn spec() -> &'static BuiltinSpec {
        builtins::find(builtins::OPENAI_OAUTH).expect("openai-oauth 必须在登记表里")
    }

    #[test]
    fn roundtrip_and_keying() {
        let home = std::env::temp_dir().join(format!("oop-state-{}", std::process::id()));
        let _ = fs::remove_dir_all(&home);
        fs::create_dir_all(&home).unwrap();

        assert!(load(&home, spec()).unwrap().is_empty());
        let mut entries = HashMap::new();
        entries.insert(
            state_key("release", "web"),
            StateEntry {
                requested_enabled: true,
                updated_at_ms: 1234,
                plugin_version: Some("0.1.0".into()),
                fingerprint: Some("dsh-x-cordis-y".into()),
            },
        );
        entries.insert(
            state_key("dev", "web"),
            StateEntry {
                requested_enabled: false,
                updated_at_ms: 5678,
                plugin_version: None,
                fingerprint: None,
            },
        );
        save(&home, spec(), &entries).unwrap();
        assert_eq!(load(&home, spec()).unwrap(), entries);

        let _ = fs::remove_dir_all(&home);
    }

    /// 两个内置插件的启停意图**分文件**：把 ponytail 打开不会让 openai 也变开。
    /// 分文件是刻意的（见文件头）——一份文件按 id 分键也能做到，而那会让
    /// openai-oauth 既有用户的路径整个搬家，升级后意图静默丢失。
    #[test]
    fn two_builtins_keep_separate_state_files() {
        let home = std::env::temp_dir().join(format!("oop-state-two-{}", std::process::id()));
        let _ = fs::remove_dir_all(&home);
        fs::create_dir_all(&home).unwrap();
        let openai = builtins::find(builtins::OPENAI_OAUTH).unwrap();
        let ponytail = builtins::find(builtins::PONYTAIL).unwrap();
        let key = state_key("release", "web");

        let mut on = HashMap::new();
        on.insert(
            key.clone(),
            StateEntry {
                requested_enabled: true,
                updated_at_ms: 1,
                plugin_version: None,
                fingerprint: None,
            },
        );
        save(&home, openai, &on).unwrap();
        save(&home, ponytail, &HashMap::new()).unwrap();

        assert!(load(&home, openai).unwrap()[&key].requested_enabled);
        assert!(
            !load(&home, ponytail).unwrap().contains_key(&key),
            "openai 的启停不该出现在 ponytail 的状态文件里"
        );
        assert_ne!(
            state_path(&home, openai),
            state_path(&home, ponytail),
            "两个插件的状态文件必须是两份"
        );
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn corrupt_file_is_an_error_not_a_reset() {
        let home = std::env::temp_dir().join(format!("oop-state-corrupt-{}", std::process::id()));
        let _ = fs::remove_dir_all(&home);
        fs::create_dir_all(home.join(spec().instance_base_segment())).unwrap();
        fs::write(state_path(&home, spec()), "{ not json").unwrap();
        let error = load(&home, spec()).unwrap_err();
        assert!(error.contains("不会自动重置"));
        let _ = fs::remove_dir_all(&home);
    }
}
