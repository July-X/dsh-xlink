//! profile 清单（`$DSH_HOME/profiles/<profile>/package.json`）的初始形状与修复。
//!
//! ## 为什么它独立成模块
//!
//! 这份清单有**两个**写入方：`kernel_adapter::prepare_instance`（建实例时落初值）
//! 与 `plugins::center`（接线时改写）。此前两边各写各的形状，差了一个字段，
//! 而差出来的后果不是「少一层插件」，是**内核静默退出**。
//!
//! 2026-10-06 实测（受控矩阵，每组只变一个变量，dsh 0.2.0-rc.2 + node 25.9）：
//!
//! | profile 目录 | `dsh web --no-open` |
//! | --- | --- |
//! | 无 `package.json` | 正常起来，打印监听 URL |
//! | `package.json` **带** `dsh.profile.bundles` | 正常起来 |
//! | `package.json` **不带** `dsh.profile.bundles` | **exit 0，且一行日志都不打** |
//!
//! 内核把「有 `package.json` 的 profile 目录」当成用户自定义 profile，读出
//! `dsh.profile.bundles` 求值——缺这个键就是空数组，于是它启动了一个**零插件**
//! 的 profile，没有可监听的东西，正常退出。`prepare_instance` 写的正是这样一个
//! stub，所以**每一次新建的实例**（每一次预检的沙盒）都起不来内核；而
//! `center::ensure_profile` 见到 `package.json` 已存在就早退，永远补不上模板
//! bundle。安装预检的基线于是 100% 失败，报告里只剩一句
//! 「沙盒内核在就绪前退出（exit status: 0）」——没有日志、没有线索。
//!
//! 所以重点不是「谁写」，是**只有一个地方知道这份清单的正确形状**，且写完
//! 还能把已经写坏的修回来（存量实例已经被那个 stub 污染过了）。

use std::path::Path;

use serde_json::{json, Value};

use crate::shell::process::atomic_write;

/// 新建 profile 时的模板 bundle，对应内核的 profile 模板。
pub fn template_bundles(profile: &str) -> Vec<String> {
    match profile {
        "web" => vec![
            String::from("@deepseek-ai/dsh-base"),
            String::from("@deepseek-ai/dsh-web-app"),
        ],
        "headless" => vec![
            String::from("@deepseek-ai/dsh-base"),
            String::from("@deepseek-ai/dsh-headless"),
        ],
        _ => vec![String::from("@deepseek-ai/dsh-base")],
    }
}

/// profile 目录的初始清单形状。
pub fn initial_manifest(profile: &str) -> Value {
    json!({
        "name": format!("dsh-profile-{profile}"),
        "private": true,
        "dependencies": {},
        "dsh": { "profile": { "bundles": template_bundles(profile) } }
    })
}

/// `pnpm-workspace.yaml` 的初值。
///
/// `nodeLinker: hoisted` 让 profile 的依赖平铺，内核解析 bundle 时才找得到；
/// `minimumReleaseAge: 0` 解除 pnpm 新版默认的「24 小时内的新版本先不装」，
/// 否则刚发布的插件会被静默跳过，表现是「装上了但没生效」。
const WORKSPACE_YAML: &str =
    "packages:\n  - .\n\nnodeLinker: hoisted\nautoInstallPeers: false\nminimumReleaseAge: 0\n";

/// 清单是否缺了**内核启动**与**插件接线**都要读的两个字段。
///
/// 判据只看「键在不在、类型对不对」，不看内容：一个 `bundles: []` 是用户自己
/// 表达「这个 profile 我就要空的」，不该被改回去；而「键缺失」只可能是壳写坏
/// 的或损坏的，两边都需要修。
pub fn needs_repair(root: &Value) -> bool {
    !root.get("dependencies").is_some_and(Value::is_object)
        || !root
            .get("dsh")
            .and_then(|d| d.get("profile"))
            .and_then(|p| p.get("bundles"))
            .is_some_and(Value::is_array)
}

/// 补齐缺失字段，返回是否改动了。**已有的键一律原样保留**——
/// 壳只负责自己那两把钥匙，不接管用户写的任何内容。
pub fn repair(root: &mut Value, profile: &str) -> bool {
    let Some(object) = root.as_object_mut() else {
        // 不是对象（`null`、数组、字符串）：清单已经读不出任何东西，
        // 内核与接线都无从下手，换成初始形状。这不是「丢用户数据」——
        // 原值本来就不可能被任何人使用。
        *root = initial_manifest(profile);
        return true;
    };
    let mut changed = false;
    if !object.get("dependencies").is_some_and(Value::is_object) {
        object.insert(String::from("dependencies"), json!({}));
        changed = true;
    }
    // `dsh` / `dsh.profile` 逐层补：中间层缺失或类型不对才重建那一层，
    // 其余兄弟键（内核工作台写的插件配置、用户写的凭据引用）照旧留下。
    let dsh = object.entry("dsh").or_insert_with(|| json!({}));
    if !dsh.is_object() {
        *dsh = json!({});
        changed = true;
    }
    let dsh = dsh.as_object_mut().expect("上一步已保证是对象");
    let profile_node = dsh.entry("profile").or_insert_with(|| json!({}));
    if !profile_node.is_object() {
        *profile_node = json!({});
        changed = true;
    }
    let profile_node = profile_node.as_object_mut().expect("上一步已保证是对象");
    if !profile_node.get("bundles").is_some_and(Value::is_array) {
        profile_node.insert(
            String::from("bundles"),
            Value::Array(
                template_bundles(profile)
                    .into_iter()
                    .map(Value::String)
                    .collect(),
            ),
        );
        changed = true;
    }
    changed
}

/// 幂等地准备好一个 profile 目录：建目录、把清单修到能启动、补两个伴生文件。
///
/// 读不出 JSON（文件损坏 / 半写入）时整份换成初始形状——那种清单内核本来就
/// 读不了，留着只会让每次启动都失败，而用户没有任何手段自己修。
pub fn seed(dir: &Path, profile: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let manifest = dir.join("package.json");
    let next = match std::fs::read_to_string(&manifest) {
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Ok(mut root) => {
                if repair(&mut root, profile) {
                    root
                } else {
                    return finish(dir);
                }
            }
            Err(_) => initial_manifest(profile),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => initial_manifest(profile),
        Err(error) => return Err(error),
    };
    let text = serde_json::to_string_pretty(&next).expect("清单是我们自己造的 Value");
    atomic_write(&manifest, format!("{text}\n").as_bytes())?;
    finish(dir)
}

/// 清单之外的两个伴生文件。workspace 缺 `minimumReleaseAge: 0` 就重写——
/// 那是壳写出的旧形状，与「没有 workspace」等价。
fn finish(dir: &Path) -> std::io::Result<()> {
    let workspace = dir.join("pnpm-workspace.yaml");
    let stale = match std::fs::read_to_string(&workspace) {
        Ok(text) => !text.contains("minimumReleaseAge: 0"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
        Err(error) => return Err(error),
    };
    if stale {
        atomic_write(&workspace, WORKSPACE_YAML.as_bytes())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-10-06 修的那个 stub：`prepare_instance` 此前写的形状。
    /// 它没有 `dsh.profile.bundles`，内核读到零个插件就正常退出。
    const BROKEN_STUB: &str = r#"{
  "name": "dsh-xlink-instance-sbx-1-2-3",
  "private": true,
  "version": "0.0.0",
  "schema_version": 1,
  "kernel_family": "dsh"
}"#;

    #[test]
    fn a_manifest_without_bundles_is_repaired_and_keeps_its_other_fields() {
        let mut root: Value = serde_json::from_str(BROKEN_STUB).unwrap();
        assert!(needs_repair(&root));
        assert!(repair(&mut root, "web"));
        assert!(!needs_repair(&root));
        // 修完必须带上模板 bundle，否则内核仍然起不来。
        let bundles = root["dsh"]["profile"]["bundles"].as_array().unwrap();
        assert!(bundles.iter().any(|b| b == "@deepseek-ai/dsh-web-app"));
        // 用户/别的工具写下的字段不动。
        assert_eq!(root["kernel_family"], "dsh");
        assert_eq!(root["version"], "0.0.0");
    }

    #[test]
    fn an_explicitly_empty_bundle_list_is_left_alone() {
        // `bundles: []` 是用户自己表达「这个 profile 就是空的」，不是壳写坏的。
        // 替他填回模板层等于替他改配置。
        let mut root = json!({"dependencies": {}, "dsh": {"profile": {"bundles": []}}});
        assert!(!needs_repair(&root));
        assert!(!repair(&mut root, "web"));
        assert_eq!(
            root["dsh"]["profile"]["bundles"].as_array().unwrap().len(),
            0
        );
    }

    #[test]
    fn a_manifest_written_by_the_shell_is_never_damaged() {
        let mut root = json!({
            "name": "dsh-profile-web",
            "private": true,
            "dependencies": {"some-plugin": "link:../extensions/plugins/p"},
            "dsh": {"profile": {"bundles": ["@deepseek-ai/dsh-base", "some-plugin"]},
                    "apiKeyEnv": "DSH_KEY"}
        });
        assert!(!needs_repair(&root));
        assert!(!repair(&mut root, "web"));
        assert_eq!(root["dsh"]["apiKeyEnv"], "DSH_KEY");
        assert_eq!(
            root["dependencies"]["some-plugin"],
            "link:../extensions/plugins/p"
        );
    }

    #[test]
    fn an_unusable_manifest_is_replaced_wholesale() {
        // 读不出结构的清单：内核与接线都用不了，换成初始形状是唯一的出路。
        let mut root = Value::Null;
        assert!(repair(&mut root, "headless"));
        assert!(!needs_repair(&root));
        assert_eq!(root["name"], "dsh-profile-headless");
        let bundles = root["dsh"]["profile"]["bundles"].as_array().unwrap();
        assert!(bundles.iter().any(|b| b == "@deepseek-ai/dsh-headless"));
    }

    #[test]
    fn seed_is_idempotent_and_repairs_a_stub_left_on_disk() {
        let dir = std::env::temp_dir().join(format!(
            "dsh-profile-seed-{}-{}",
            std::process::id(),
            BROKEN_STUB.len()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("package.json"), BROKEN_STUB).unwrap();

        seed(&dir, "web").unwrap();
        let after: Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("package.json")).unwrap())
                .unwrap();
        assert!(!needs_repair(&after));
        // 第二次调用不该再动它。
        let text = std::fs::read_to_string(dir.join("package.json")).unwrap();
        seed(&dir, "web").unwrap();
        assert_eq!(
            text,
            std::fs::read_to_string(dir.join("package.json")).unwrap()
        );

        let workspace = std::fs::read_to_string(dir.join("pnpm-workspace.yaml")).unwrap();
        assert!(workspace.contains("minimumReleaseAge: 0"));
        assert!(workspace.contains("nodeLinker: hoisted"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn every_template_profile_carries_the_bundle_the_web_stack_needs() {
        for profile in ["web", "headless", "some-future-profile"] {
            let bundles = template_bundles(profile);
            assert!(
                bundles.contains(&String::from("@deepseek-ai/dsh-base")),
                "{profile} 的模板层缺 base"
            );
        }
    }
}
