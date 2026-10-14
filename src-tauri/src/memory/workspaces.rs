//! 内核工作区注册表的**只读**视图。
//!
//! 记忆要按项目归属，而「路径 ↔ 工作区 id」这份对应只有内核知道：它持久化在
//! `<DSH_HOME>/storages/workspace.json`（`dsh-workspace` 的 storage domain：
//! `tables.workspaces.<uuid>.{path,title,sessionIds,createdAt,updatedAt}`）。面板因此
//! 能把「本项目」从一个手填路径变成一个下拉选择，并顺手拿到真实的 `workspace_id`——
//! 靠路径字符串相等匹配，改名一次就失配。
//!
//! 三条自律，因为这是**别人的私有格式**：
//!
//! · **只读**。不写、不迁移、不缓存（内核随时可能在跑并原子替换这个文件）。
//! · **读不出来不是错误**：没有文件、内核还没建过工作区、结构变了都返回空列表，
//!   面板退回手填路径。把「格式变了」报成「记忆坏了」会让用户去修错的东西。
//! · **不猜字段**：`path` 缺失的行直接丢掉，而不是拿 id 或标题凑一个路径。

use serde::{Deserialize, Serialize};
use std::path::Path;

/// 面板下拉里的一项。
#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceView {
    /// 内核的工作区 id（`tables.workspaces` 的键）。
    pub id: String,
    /// 规范化后的绝对路径（内核在创建时做过 `realpath`）。
    pub path: String,
    /// 显示名（内核默认取路径最后一段）。
    pub title: String,
    /// 最近一次活动时间（RFC3339，UTC）。空串表示内核没写。
    pub updated_at: String,
    /// 该工作区下的会话数。
    pub session_count: usize,
}

#[derive(Deserialize)]
struct Stored {
    tables: Option<Tables>,
}

#[derive(Deserialize)]
struct Tables {
    workspaces: Option<std::collections::BTreeMap<String, Row>>,
}

/// 存储文档的键是 camelCase（内核自己的 storage domain 形状），不是我们定的。
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Row {
    path: Option<String>,
    title: Option<String>,
    updated_at: Option<String>,
    session_ids: Option<Vec<String>>,
}

/// 列出这个实例的内核已知的工作区；读不出来就是空列表。
pub fn list(dsh_home: &Path) -> Vec<WorkspaceView> {
    let file = dsh_home.join("storages").join("workspace.json");
    let Ok(raw) = std::fs::read_to_string(&file) else {
        return Vec::new();
    };
    let Ok(stored) = serde_json::from_str::<Stored>(&raw) else {
        return Vec::new();
    };
    let Some(tables) = stored.tables else {
        return Vec::new();
    };
    let Some(rows) = tables.workspaces else {
        return Vec::new();
    };
    let mut out: Vec<WorkspaceView> = rows
        .into_iter()
        .filter_map(|(id, row)| {
            let path = row.path?;
            if path.is_empty() {
                return None;
            }
            Some(WorkspaceView {
                title: row
                    .title
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| path.clone()),
                id,
                path,
                updated_at: row.updated_at.unwrap_or_default(),
                session_count: row.session_ids.map(|ids| ids.len()).unwrap_or(0),
            })
        })
        .collect();
    // 最近用过的排前面：面板下拉的第一项就该是用户现在这个项目。
    out.sort_by(|left, right| {
        right
            .updated_at
            .cmp(&left.updated_at)
            .then_with(|| left.title.cmp(&right.title))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_storage(tag: &str, body: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "xlink-memory-workspaces-{}-{}",
            std::process::id(),
            tag
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("storages")).unwrap();
        std::fs::write(dir.join("storages").join("workspace.json"), body).unwrap();
        dir
    }

    #[test]
    fn reads_rows_and_sorts_by_recent_activity() {
        let dir = write_storage(
            "ok",
            r#"{"unit":{"name":"workspace","version":1},
                "tables":{"workspaces":{
                  "old":{"path":"/tmp/old","title":"旧项目","sessionIds":["a"],"updatedAt":"2026-10-01T00:00:00Z"},
                  "new":{"path":"/tmp/new","title":"新项目","sessionIds":["a","b"],"updatedAt":"2026-10-14T00:00:00Z"}
                }}}"#,
        );
        let list = list(&dir);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, "new");
        assert_eq!(list[0].title, "新项目");
        assert_eq!(list[0].session_count, 2);
        assert_eq!(list[1].path, "/tmp/old");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 没有文件 / 结构不认识 / 行里没有路径：都回空列表或跳过，**不报错**。
    /// 把「内核换了格式」报成「记忆坏了」会让用户去修错的东西。
    #[test]
    fn unreadable_or_unknown_shapes_give_an_empty_list() {
        let missing =
            std::env::temp_dir().join(format!("xlink-memory-ws-missing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&missing);
        assert!(list(&missing).is_empty());

        let garbage = write_storage("garbage", "{not json");
        assert!(list(&garbage).is_empty());
        let _ = std::fs::remove_dir_all(&garbage);

        let no_path = write_storage(
            "no-path",
            r#"{"tables":{"workspaces":{"x":{"title":"没有路径"}}}}"#,
        );
        assert!(list(&no_path).is_empty(), "缺 path 的行不能拿 id 凑一个");
        let _ = std::fs::remove_dir_all(&no_path);

        let renamed = write_storage(
            "renamed",
            r#"{"tables":{"workSpaces":{"x":{"path":"/tmp/x"}}}}"#,
        );
        assert!(list(&renamed).is_empty());
        let _ = std::fs::remove_dir_all(&renamed);
    }
}
