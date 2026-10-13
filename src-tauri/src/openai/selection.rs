//! 常用模型选择（用户偏好）。
//!
//! 账号目录回答「能选什么」，这里回答「会话的模型选择器里列出哪些」。两者
//! 刻意分开：未勾选的模型仍然能被 `resolveModel` 解析（Host 适配器拿的是
//! **完整**目录，只过滤 `listModels`），所以「取消勾选」不会把已经用着它的
//! 会话打成 `MODEL_NOT_IN_CATALOG`——那条会话的报错会出现在另一个界面里，
//! 而原因在这张卡片上，查起来无从下手。
//!
//! 存储是**壳 mode 级**的一只文件（`shell/<mode>/openai-oauth/
//! model-selection.json`），与 vault、目录缓存同目录同口径：换账号、换实例
//! 都读同一份偏好。文件里只记用户勾选的 id，与当前目录的**交集在读取时**
//! 算——账号切换后不必重写文件，模型下线也不会在盘上留下死 id。
//!
//! 三条语义（都在 [`effective`] 里，别在调用点重写一份）：
//! - 从未设置过 → **推荐集合**（带能力元数据的条目）。推荐集合为空（账号
//!   目录里一条都没收录能力）时退回完整目录——「一个都没勾」在界面上读起来
//!   像坏了，而「全都勾着」只是回到改动前；
//! - 设置过、但当前目录里一条都不剩（换了账号 / 模型下线）→ 同样退回推荐集合；
//! - 设置过且有交集 → 交集本身，顺序跟目录。
//!
//! [`save`] 只接受当前目录里存在的 id（陌生的丢弃）且**拒绝空集**：空集会让
//! 会话选择器里这个提供方整组消失，而用户看不出是自己清空的。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::openai::catalog::{Catalog, CatalogEntry};
use crate::openai::flow::FlowPaths;

/// 偏好文件名（落在 `FlowPaths::mode_dir` 里）。
pub(crate) const FILE_NAME: &str = "model-selection.json";

/// 未显式设置的条目在界面上的能力标记（`catalog.rs` 的 `merge_capability`）。
const UNKNOWN_CAPABILITY: &str = "unknown";

#[derive(Serialize, Deserialize, Default)]
struct Stored {
    #[serde(default)]
    selected: Vec<String>,
}

/// 偏好文件路径（桥接 env 与命令层共用一份，别各拼一遍）。
pub(crate) fn file(paths: &FlowPaths) -> PathBuf {
    paths.mode_dir.join(FILE_NAME)
}

/// 读取用户勾选的 id。
///
/// `None` = 从未设置过，或文件读不出来 / 不是有效 JSON——两种情况都按
/// 「未设置」处理：这是一份偏好，为它挡住整个模型列表不划算，损坏的后果
/// 只是回到默认集合。空数组同样按未设置处理（见模块文档）。
pub(crate) fn load(paths: &FlowPaths) -> Option<Vec<String>> {
    let text = std::fs::read_to_string(file(paths)).ok()?;
    serde_json::from_str::<Stored>(&text)
        .ok()
        .map(|stored| stored.selected)
}

/// 推荐集合：带能力元数据的条目；一条都没有时退回完整目录。
pub(crate) fn recommended(catalog: &Catalog) -> Vec<String> {
    let annotated: Vec<String> = catalog
        .entries
        .iter()
        .filter(|entry| entry.capability != UNKNOWN_CAPABILITY)
        .map(|entry| entry.id.clone())
        .collect();
    if annotated.is_empty() {
        catalog
            .entries
            .iter()
            .map(|entry| entry.id.clone())
            .collect()
    } else {
        annotated
    }
}

/// 生效集合：目录顺序、去重、非空（规则见模块文档）。
pub(crate) fn effective(catalog: &Catalog, stored: Option<&[String]>) -> Vec<String> {
    if let Some(stored) = stored {
        let picked: Vec<String> = catalog
            .entries
            .iter()
            .filter(|entry| stored.iter().any(|id| id == &entry.id))
            .map(|entry| entry.id.clone())
            .collect();
        if !picked.is_empty() {
            return picked;
        }
    }
    recommended(catalog)
}

/// 保存勾选：按目录过滤 / 排序，拒绝空集。返回落盘的集合。
pub(crate) fn save(
    paths: &FlowPaths,
    catalog: &Catalog,
    ids: &[String],
) -> Result<Vec<String>, String> {
    let picked: Vec<String> = catalog
        .entries
        .iter()
        .filter(|entry| ids.iter().any(|id| id == &entry.id))
        .map(|entry| entry.id.clone())
        .collect();
    if picked.is_empty() {
        return Err("至少要保留一个常用模型；可点「恢复默认」或勾选任意一条".into());
    }
    write(paths, &picked)?;
    Ok(picked)
}

fn write(paths: &FlowPaths, ids: &[String]) -> Result<(), String> {
    let path = file(paths);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            format!(
                "创建偏好目录失败（{}）：{error}；请确认磁盘可写后重试",
                parent.display()
            )
        })?;
    }
    let body = serde_json::to_string(&Stored {
        selected: ids.to_vec(),
    })
    .map_err(|error| format!("常用模型偏好不是有效 JSON（不应发生）：{error}"))?;
    crate::shell::process::atomic_write(&path, format!("{body}\n").as_bytes())
        .map_err(|error| format!("保存常用模型失败（{}）：{error}", path.display()))
}

/// 界面用的一条模型选项。
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Choice {
    pub(crate) id: String,
    pub(crate) name: String,
    /// `verified` / `codex-catalog` / `unknown`——UI 用它区分「能力已验证」。
    pub(crate) capability: String,
    pub(crate) selected: bool,
}

/// 由**已有**目录构造界面视图（不重复读目录：调用方刚读过）。
pub(crate) fn choices(paths: &FlowPaths, catalog: &Catalog) -> Vec<Choice> {
    let selected = effective(catalog, load(paths).as_deref());
    catalog
        .entries
        .iter()
        .map(|entry: &CatalogEntry| Choice {
            id: entry.id.clone(),
            name: entry.name.clone(),
            capability: entry.capability.clone(),
            selected: selected.iter().any(|id| id == &entry.id),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openai::catalog::Catalog;

    /// 只碰自己的临时目录（AGENTS.md：测试不许碰用户真实数据目录）。
    struct Scratch(FlowPaths);

    impl Scratch {
        fn new(tag: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("oop-selection-{}-{tag}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            Self(FlowPaths {
                mode_dir: dir.clone(),
                vault_file: dir.join("accounts.bin"),
                xlink_home: dir.clone(),
                issuer_base: "http://127.0.0.1:1".into(),
                models_base: "http://127.0.0.1:1/v1".into(),
            })
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0.mode_dir);
        }
    }

    fn entry(id: &str, capability: &str) -> CatalogEntry {
        CatalogEntry {
            id: id.into(),
            name: format!("模型 {id}"),
            context_window: None,
            efforts: None,
            capability: capability.into(),
        }
    }

    fn catalog(entries: Vec<CatalogEntry>) -> Catalog {
        Catalog {
            revision: "rev-1".into(),
            capability_revision: 0,
            entries,
            fetched_at: 1,
        }
    }

    /// 未设置 → 推荐集合（只含有能力元数据的条目）；一条元数据都没有 → 完整目录。
    #[test]
    fn default_selection_is_the_annotated_subset() {
        let mixed = catalog(vec![
            entry("gpt-a", "codex-catalog"),
            entry("gpt-b", "unknown"),
            entry("gpt-c", "verified"),
        ]);
        assert_eq!(recommended(&mixed), vec!["gpt-a", "gpt-c"]);
        assert_eq!(effective(&mixed, None), vec!["gpt-a", "gpt-c"]);
        let bare = catalog(vec![entry("gpt-a", "unknown"), entry("gpt-b", "unknown")]);
        assert_eq!(
            effective(&bare, None),
            vec!["gpt-a", "gpt-b"],
            "一条都没收录能力时必须退回完整目录，否则选择器会空掉"
        );
    }

    /// 设置过的集合与目录取交集：顺序跟目录、未知 id 丢掉、空交集退回推荐。
    #[test]
    fn stored_selection_is_intersected_with_the_catalog_order() {
        let current = catalog(vec![
            entry("gpt-a", "codex-catalog"),
            entry("gpt-b", "unknown"),
            entry("gpt-c", "codex-catalog"),
        ]);
        let stored = vec!["gpt-c".to_string(), "gpt-b".to_string()];
        assert_eq!(effective(&current, Some(&stored)), vec!["gpt-b", "gpt-c"]);
        // 换了账号：勾选的 id 一条都不在 → 退回推荐集合（而不是空列表）。
        let other = vec!["gone-1".to_string(), "gone-2".to_string()];
        assert_eq!(effective(&current, Some(&other)), vec!["gpt-a", "gpt-c"]);
    }

    /// 保存：过滤陌生 id、按目录排序、拒绝空集；落盘后能原样读回。
    #[test]
    fn save_filters_unknown_ids_and_refuses_an_empty_selection() {
        let scratch = Scratch::new("save");
        let paths = &scratch.0;
        let current = catalog(vec![
            entry("gpt-a", "codex-catalog"),
            entry("gpt-b", "unknown"),
        ]);
        assert_eq!(load(paths), None, "从没写过 → 未设置");

        let saved = save(
            paths,
            &current,
            &["gpt-b".to_string(), "nope".to_string(), "gpt-a".to_string()],
        )
        .unwrap();
        assert_eq!(saved, vec!["gpt-a", "gpt-b"], "按目录顺序，且丢掉陌生 id");
        assert_eq!(load(paths), Some(vec!["gpt-a".into(), "gpt-b".into()]));
        assert!(file(paths).is_file());

        // 空集（或全是陌生 id）被拒绝，且**不覆盖**上一次的落盘内容。
        let error = save(paths, &current, &["nope".to_string()]).unwrap_err();
        assert!(error.contains("至少"), "{error}");
        assert_eq!(load(paths), Some(vec!["gpt-a".into(), "gpt-b".into()]));
    }

    /// 文件损坏 / 空数组 → 按未设置处理（默认集合），不把整个模型列表挡掉。
    #[test]
    fn broken_or_empty_file_falls_back_to_the_default_set() {
        let scratch = Scratch::new("broken");
        let paths = &scratch.0;
        let current = catalog(vec![
            entry("gpt-a", "codex-catalog"),
            entry("gpt-b", "unknown"),
        ]);
        std::fs::create_dir_all(&paths.mode_dir).unwrap();
        std::fs::write(file(paths), "{ not json").unwrap();
        assert_eq!(load(paths), None);

        std::fs::write(file(paths), "{\"selected\":[]}").unwrap();
        assert_eq!(load(paths), Some(Vec::new()));
        assert_eq!(effective(&current, None), vec!["gpt-a"]);
    }

    /// 界面视图：勾选状态就是生效集合，名字与能力标记透传。
    #[test]
    fn choices_carry_names_and_the_effective_selection() {
        let scratch = Scratch::new("choices");
        let paths = &scratch.0;
        let current = catalog(vec![
            entry("gpt-a", "codex-catalog"),
            entry("gpt-b", "unknown"),
        ]);
        std::fs::create_dir_all(&paths.mode_dir).unwrap();
        std::fs::write(file(paths), "{\"selected\":[\"gpt-b\"]}").unwrap();
        let choices = choices(paths, &current);
        assert_eq!(
            choices,
            vec![
                Choice {
                    id: "gpt-a".into(),
                    name: "模型 gpt-a".into(),
                    capability: "codex-catalog".into(),
                    selected: false,
                },
                Choice {
                    id: "gpt-b".into(),
                    name: "模型 gpt-b".into(),
                    capability: "unknown".into(),
                    selected: true,
                },
            ]
        );
    }
}
