//! 社区插件目录的检索层：把面板上选中的**三个参数**（关键词 / 分类 / 排序）
//! 收在一起，在远端目录上搜出要显示的那一页。
//!
//! ## 为什么搜索落在这一层而不是浏览器里
//!
//! dshfind 的公开目录是一份 1.7 万条、约 9 MB 的 JSON（落地成
//! `<data_dir>/plugins-catalog.json`）。旧路径是把它整份拉进 webview 再用 JS
//! 过滤：每次打开插件页都有一次 9 MB 的 IPC 传输和一份常驻的对象图，每按一次
//! 键还要在 webview 里重扫 1.7 万条。搜索放在这里之后，**过一次 IPC 的只有
//! 当前这一页**（默认 24 条），关键词、分类、排序三个参数一起发过来。
//!
//! ## 与 [`crate::plugins::center`] 的分工
//!
//! 取源（拉取、6 小时缓存、dshfind 不可达时回退参考市场）仍在 `center`，
//! 这里**不重复实现一遍**——筛选规则只有这一份，而缓存与新鲜度只有那一份。
//! `search` 是唯一对外入口，内部拆成「取目录」+「纯函数筛这一份列表」，
//! 后者不碰 fs、不出网，因此可以在没有网络与工具链的环境里测。

use crate::commands::AppState;
use crate::plugins::center::{self, CatalogItem};
use crate::shell::error::AppError;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;
use tauri::State;

/// 一次搜索最多返回多少条。面板的「显示更多」每次加 `CATALOG_PAGE`，
/// 走到这里就够了；**这条硬顶是有意的**：没有它，一次 `limit: 99999` 就能把
/// 9 MB 重新塞回 IPC，而这层存在的全部意义就是不再那样做。
const MAX_LIMIT: usize = 200;
/// `limit` 缺省时取多少。与前端 `CATALOG_PAGE` 同值，但**以那份为准**——
/// 这里只是兜底（调用方没传时别返回空列表），不是第二份步长定义。
const DEFAULT_LIMIT: usize = 24;

/// 面板选中的一次搜索。三个参数就是界面上那三个控件：关键词输入框、
/// 分类下拉、排序下拉。
#[derive(Debug, Clone, Default)]
pub struct CatalogQuery {
    /// 关键词输入框回车后提交的文本（提交前输入框里的草稿不发）。
    pub query: String,
    /// 分类 id；`all` 或空串表示不限。
    pub category: String,
    /// `stars` 或 `updated`。
    pub sort: String,
    /// 本次要显示多少条（面板「显示更多」累加出来的值）。
    pub limit: usize,
    /// 面板展示用的**分类中文名**（分类 id → 中文名），
    /// 由 `ui/src/plugins/plugins.js` 的 `CATALOG_CATEGORIES` 随请求带下来。
    ///
    /// 为什么要多带这一份字典：dshfind 目录里的分类 id 是英文
    /// （`memory` / `ui` / …），中文名只存在于前端。在这一层只按 id 检索，
    /// 用户搜「记忆」只能命中描述里恰好带这两个字的 21% 条目（实测 831 条
    /// 记忆类里 179 条），而他明明能在下拉里看到「记忆上下文」这四个字。
    /// **分类名的唯一定义留在前端**，Rust 侧不抄一份——抄了就必然漂。
    pub labels: BTreeMap<String, String>,
    /// 跳过目录缓存窗口重新拉取（对应面板的「刷新数据」）。
    pub force: bool,
}

/// 一页搜索结果，外加分类下拉要用的计数。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogPage {
    /// 本页条目（最多 `limit` 条）。
    pub items: Vec<CatalogItem>,
    /// 本次条件下命中的**总**条数，与 `items.len()` 无关（「显示更多」据此
    /// 判断还有没有）。
    pub total: usize,
    /// 分类 id → 命中条数。**在关键词范围内统计、不受当前分类筛选影响**，
    /// 于是「全部（128）」与「记忆上下文（37）」讲的是同一次搜索的两面，
    /// 用户改分类时能预先看到那一类还剩多少。
    pub counts: BTreeMap<String, usize>,
    /// 本次实际生效的关键词（已 trim），供面板在结果区回显。
    pub query: String,
    /// 本次实际生效的分类。
    pub category: String,
    /// 本次实际生效的排序。
    pub sort: String,
}

/// 检索词表：名称、描述、仓库、分类 id、标签，以及该分类的中文名。
fn haystack(item: &CatalogItem, label: Option<&str>) -> String {
    let mut text =
        String::with_capacity(item.name.len() + item.description.len() + item.category.len() + 32);
    text.push_str(&item.name);
    text.push(' ');
    text.push_str(&item.description);
    text.push(' ');
    text.push_str(item.repo.as_deref().unwrap_or_default());
    text.push(' ');
    text.push_str(&item.category);
    if let Some(label) = label {
        text.push(' ');
        text.push_str(label);
    }
    for tag in &item.tags {
        text.push(' ');
        text.push_str(tag);
    }
    text.to_lowercase()
}

/// 命中权重：名称前缀 > 名称中段 > 其余字段，0 表示不命中。
///
/// 与旧的前端实现同一套三档权重，**理由不变**：命中与否曾是二值的，
/// 1.7 万条里任何一个高频词（如 "agent"）都会把结果搅成目录原序——用户搜
/// agent 期待先看到名为 agent 的，而不是描述里提了一句 agent 的。
fn score(item: &CatalogItem, needle: &str, label: Option<&str>) -> u8 {
    let name = item.name.to_lowercase();
    if name.starts_with(needle) {
        return 3;
    }
    if name.contains(needle) {
        return 2;
    }
    if haystack(item, label).contains(needle) {
        1
    } else {
        0
    }
}

/// `dshfind` 的 `pushedAt` 一律是 `YYYY-MM-DDTHH:MM:SSZ`，定宽、零填充、
/// 同一时区，因此**字典序就是时间序**；缺时间戳的条目（参考市场回退源）
/// 拿不到日期串，排在最后。
fn updated_at(item: &CatalogItem) -> &str {
    item.updated.as_str()
}

/// 在**已经取好的**目录上做一次搜索。纯函数：不碰 fs、不出网，
/// 因此筛选规则可以在没有网络的环境里直接测。
pub fn search_items(all: &[CatalogItem], q: &CatalogQuery) -> CatalogPage {
    let needle = q.query.trim().to_lowercase();
    let unlimited = q.category.is_empty() || q.category == "all";
    let limit = if q.limit == 0 {
        DEFAULT_LIMIT
    } else {
        q.limit.min(MAX_LIMIT)
    };

    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut hits: Vec<(u8, usize)> = Vec::new();
    for (index, item) in all.iter().enumerate() {
        let label = q.labels.get(&item.category).map(String::as_str);
        let score = if needle.is_empty() {
            1
        } else {
            score(item, &needle, label)
        };
        if score == 0 {
            continue;
        }
        // 空分类（dshfind 目录里有一千多条没归类）不进下拉计数：面板那
        // 侧也没有对应选项，只会让「全部」与各分类之和对不上。
        if !item.category.is_empty() {
            *counts.entry(item.category.clone()).or_insert(0) += 1;
        }
        if unlimited || item.category == q.category {
            hits.push((score, index));
        }
    }

    // 相关度优先：有关键词时用户找的是「叫这个名字的」，排序下拉此时让位；
    // 没有关键词时排序下拉才接管。两条路径都用稳定排序，同级保持目录原序
    // （`center::catalog` 已按 star 降序给出），所以结果不会自己乱跳。
    if !needle.is_empty() {
        hits.sort_by_key(|hit| std::cmp::Reverse(hit.0));
    } else if q.sort == "updated" {
        hits.sort_by(|a, b| updated_at(&all[b.1]).cmp(updated_at(&all[a.1])));
    }

    let total = hits.len();
    let items = hits
        .into_iter()
        .take(limit)
        .map(|(_, index)| all[index].clone())
        .collect();
    CatalogPage {
        items,
        total,
        counts,
        query: q.query.trim().to_string(),
        category: if unlimited {
            String::from("all")
        } else {
            q.category.clone()
        },
        sort: if q.sort.is_empty() {
            String::from("stars")
        } else {
            q.sort.clone()
        },
    }
}

/// 面板的「插件仓库」列表：取远端目录（带 6 小时缓存）后按三个参数筛一页。
pub fn search(data_dir: &Path, q: &CatalogQuery) -> Result<CatalogPage, AppError> {
    let all = center::catalog(data_dir, q.force)?;
    Ok(search_items(&all, q))
}

/// 按面板选中的关键词 / 分类 / 排序，在远端插件目录里搜一页。
///
/// `labels` 是面板展示用的分类中文名（见 [`CatalogQuery::labels`]）；`limit`
/// 缺省 24、上限 200（见 [`MAX_LIMIT`]）。取目录在 `spawn_blocking` 里跑——
/// 首次搜索可能要去网络上拉 9 MB 目录。
#[tauri::command]
pub async fn plugin_catalog_search(
    state: State<'_, AppState>,
    query: Option<String>,
    category: Option<String>,
    sort: Option<String>,
    limit: Option<usize>,
    labels: Option<BTreeMap<String, String>>,
    force: Option<bool>,
) -> Result<CatalogPage, String> {
    let data_dir = state.data_dir.clone();
    let request = CatalogQuery {
        query: query.unwrap_or_default(),
        category: category.unwrap_or_default(),
        sort: sort.unwrap_or_default(),
        limit: limit.unwrap_or(0),
        labels: labels.unwrap_or_default(),
        force: force.unwrap_or(false),
    };
    crate::commands::blocking(move || search(&data_dir, &request)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    /// 临时数据目录。**只清自己这一块**——按 `paths::*` 解析出来的家目录
    /// 是用户真实的 `~/.dsh-xlink`，对它 `remove_dir_all` 一次就能把用户
    /// 全部内核、实例与会话删干净（2026-09-29 实测）。
    fn temp_data_dir(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "dsh-catalog-test-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn item(
        name: &str,
        category: &str,
        stars: u64,
        updated: &str,
        description: &str,
    ) -> CatalogItem {
        CatalogItem {
            id: name.to_string(),
            name: name.to_string(),
            kind: String::new(),
            description: description.to_string(),
            stars,
            forks: 0,
            downloads: 0,
            verified: false,
            repo: Some(format!("someone/{name}")),
            spec: format!("https://github.com/someone/{name}.git"),
            origin: "git".to_string(),
            category: category.to_string(),
            version: String::new(),
            tags: Vec::new(),
            updated: updated.to_string(),
            detail_url: String::new(),
        }
    }

    /// 目录原序即 star 降序（`center::catalog` 的产出形态）。
    fn catalog() -> Vec<CatalogItem> {
        vec![
            item(
                "agent-toolkit",
                "agent",
                900,
                "2026-01-01T00:00:00Z",
                "工具箱",
            ),
            item(
                "my-agent-plugin",
                "agent",
                500,
                "2026-09-01T00:00:00Z",
                "另一个 agent 插件",
            ),
            item(
                "theme-dark",
                "skin",
                800,
                "2026-05-01T00:00:00Z",
                "深色主题",
            ),
            item(
                "memory-keeper",
                "memory",
                300,
                "2026-03-01T00:00:00Z",
                "记住你说的话",
            ),
            item("plain-plugin", "", 100, "", "没有归类"),
        ]
    }

    fn names(page: &CatalogPage) -> Vec<&str> {
        page.items.iter().map(|i| i.name.as_str()).collect()
    }

    fn query() -> CatalogQuery {
        CatalogQuery {
            limit: 24,
            ..Default::default()
        }
    }

    #[test]
    fn name_prefix_outranks_mid_name_outranks_other_fields() {
        let all = catalog();
        let page = search_items(
            &all,
            &CatalogQuery {
                query: String::from("agent"),
                ..query()
            },
        );
        assert_eq!(names(&page), vec!["agent-toolkit", "my-agent-plugin"]);
        assert_eq!(page.total, 2);
    }

    #[test]
    fn keyword_is_case_insensitive_and_trimmed() {
        let all = catalog();
        let page = search_items(
            &all,
            &CatalogQuery {
                query: String::from("  AGENT  "),
                ..query()
            },
        );
        assert_eq!(
            page.query, "AGENT",
            "回显的是 trim 后的原文，不是小写化后的"
        );
        assert_eq!(page.total, 2);
    }

    #[test]
    fn category_filter_narrows_items_but_not_the_counts() {
        let all = catalog();
        let page = search_items(
            &all,
            &CatalogQuery {
                query: String::from("agent"),
                category: String::from("skin"),
                ..query()
            },
        );
        assert!(page.items.is_empty());
        // 计数讲的是「这次搜索里每个分类还剩多少」，所以不受当前分类影响——
        // 用户改分类时能预先看见另一类有几条。
        assert_eq!(page.counts.get("agent"), Some(&2));
        // 皮肤主题里没有一条命中 "agent"，于是它根本不出现（不是 0）。
        assert!(!page.counts.contains_key("skin"));
    }

    #[test]
    fn uncategorized_items_count_towards_total_but_not_the_dropdown() {
        let all = catalog();
        let page = search_items(&all, &query());
        assert_eq!(page.total, 5);
        assert_eq!(page.counts.get("agent"), Some(&2));
        assert!(
            !page.counts.contains_key(""),
            "空分类在下拉里没有对应选项，计入会让「全部」与各分类之和对不上"
        );
    }

    #[test]
    fn chinese_category_label_is_searchable() {
        let all = catalog();
        let mut labels = BTreeMap::new();
        labels.insert(String::from("memory"), String::from("记忆上下文"));
        let page = search_items(
            &all,
            &CatalogQuery {
                query: String::from("记忆"),
                labels,
                ..query()
            },
        );
        assert_eq!(names(&page), vec!["memory-keeper"]);
    }

    #[test]
    fn sort_by_updated_only_takes_over_without_a_keyword() {
        let all = catalog();
        let page = search_items(
            &all,
            &CatalogQuery {
                sort: String::from("updated"),
                ..query()
            },
        );
        assert_eq!(
            names(&page),
            vec![
                "my-agent-plugin",
                "theme-dark",
                "memory-keeper",
                "agent-toolkit",
                "plain-plugin"
            ]
        );

        // 有关键词时相关度优先：my-agent-plugin 更新更近，但用户搜的是
        // 「叫 agent 的」，它只能排第二。
        let ranked = search_items(
            &all,
            &CatalogQuery {
                query: String::from("agent"),
                sort: String::from("updated"),
                ..query()
            },
        );
        assert_eq!(names(&ranked), vec!["agent-toolkit", "my-agent-plugin"]);
    }

    #[test]
    fn limit_pages_the_result_but_never_truncates_total() {
        let all = catalog();
        let page = search_items(
            &all,
            &CatalogQuery {
                limit: 2,
                ..query()
            },
        );
        assert_eq!(page.items.len(), 2);
        assert_eq!(page.total, 5);
    }

    #[test]
    fn limit_is_capped_so_one_call_cannot_pull_the_whole_catalog_back() {
        let all = catalog();
        let page = search_items(
            &all,
            &CatalogQuery {
                limit: 99_999,
                ..query()
            },
        );
        assert!(page.items.len() <= MAX_LIMIT);
        assert_eq!(page.total, 5);
    }

    #[test]
    fn empty_query_still_applies_the_category() {
        let all = catalog();
        let page = search_items(
            &all,
            &CatalogQuery {
                category: String::from("agent"),
                ..query()
            },
        );
        assert_eq!(names(&page), vec!["agent-toolkit", "my-agent-plugin"]);
        assert_eq!(page.category, "agent");
    }

    #[test]
    fn all_category_is_normalized_away() {
        let all = catalog();
        let page = search_items(
            &all,
            &CatalogQuery {
                category: String::from("all"),
                ..query()
            },
        );
        assert_eq!(page.category, "all");
        assert_eq!(page.total, 5);
    }

    /// `search` 走的是「取目录 → 筛这一页」两步。缓存文件刚写下的那份是
    /// 新鲜的（TTL 6 小时），所以这一步**不出网**——夹具里放一条
    /// 装不出来的条目（spec 为空）顺带钉住「无法安装的条目不进结果」。
    #[test]
    fn search_reads_the_cached_catalog_and_filters_it() {
        let dir = temp_data_dir("cached");
        let mut installable = item(
            "agent-toolkit",
            "agent",
            900,
            "2026-01-01T00:00:00Z",
            "工具箱",
        );
        installable.id = String::new();
        let mut uninstallable = item("ghost", "agent", 10, "2026-01-01T00:00:00Z", "装不了");
        uninstallable.spec = String::new();
        let cache = dir.join("plugins-catalog.json");
        fs::write(
            &cache,
            serde_json::to_string(&vec![installable, uninstallable]).unwrap(),
        )
        .unwrap();

        let page = search(
            &dir,
            &CatalogQuery {
                query: String::from("agent"),
                ..query()
            },
        )
        .expect("缓存新鲜时不该去网络上拉目录");
        assert_eq!(names(&page), vec!["agent-toolkit"]);
        assert_eq!(page.total, 1);
        assert_eq!(page.counts.get("agent"), Some(&1));

        fs::remove_dir_all(&dir).ok();
    }
}
