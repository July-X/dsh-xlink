//! 模型目录（P3，设计 §6.1 / 开发计划 §7）。
//!
//! 三层数据合成「最终可用集合」：**账号目录**（官方端点拉取，带
//! access token）× **能力表**（随应用交付的精确验证记录）× **缓存**
//!（按账号分文件，目录失败时保留最后一次成功记录——不返回空列表冒充
//! 成功）。两个 revision 各自独立：`catalogRevision` 跟账号目录，
//! `capabilityRevision` 跟能力表版本；Host 侧发送前校验双 revision
//!（开发计划 §7）。
//!
//! **能力表 v0 为空**（诚实起点）：未经验证的模型一律 `unknown` 能力，
//! 不猜上下文容量、不猜强度档位（设计 §6.1「未知模型保留为能力待验证，
//! 不猜测」）。P6 逐模型验收后填表，`capabilityRevision` 随之递增。
//!
//! 端点路径与响应形状是**未验证常量**（设计 §10；沙箱内官方文档不可达，
//! 按 OpenAI 公开 API 的惯用形状防御式解析），联调时只改本文件的配置层。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::openai::flow::{FlowPaths, FlowTransport};
use crate::openai::refresh::{ensure_fresh_access, RefreshError};
use crate::openai::transport::Failure;

/// 账号目录的拉取路径（未验证常量；挂在 issuer 基址下）。
const MODELS_PATH: &str = "/v1/models";

/// 能力表版本（表内容变更时递增；v0 = 空表）。
pub(crate) const CAPABILITY_REVISION: u32 = 0;

/// 随应用交付的精确能力表（P6 逐模型验收后填充；键为精确模型 id）。
const CAPABILITY_TABLE: &[(&str, Capability)] = &[];

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Capability {
    pub(crate) context_window: Option<u64>,
    /// 精确强度档位（原始字符串，与上游一一对应）。
    pub(crate) efforts: &'static [&'static str],
    pub(crate) supports_tools: bool,
}

/// 目录里一个模型的完整条目（发给 Host 的形状与 host/adapter.js 对应）。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CatalogEntry {
    pub(crate) id: String,
    pub(crate) name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) context_window: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) efforts: Option<Vec<String>>,
    /// 能力来源标记：`verified`（能力表）/ `unknown`（待验证，不发档位）。
    #[serde(default)]
    pub(crate) capability: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Catalog {
    pub(crate) revision: String,
    pub(crate) capability_revision: u32,
    pub(crate) entries: Vec<CatalogEntry>,
}

/// 目录错误：`Reauth` 要求重新登录；`Message` 稍后重试（用缓存兜底）。
#[derive(Debug)]
pub(crate) enum CatalogError {
    Reauth(String),
    Message(String),
}

impl CatalogError {
    pub(crate) fn message(&self) -> String {
        match self {
            CatalogError::Reauth(detail) => detail.clone(),
            CatalogError::Message(detail) => detail.clone(),
        }
    }
}

fn catalog_cache_path(paths: &FlowPaths, sub: &str) -> PathBuf {
    paths.mode_dir.join(format!("catalog-{sub}.json"))
}

/// 取可用目录：登录 → 令牌新鲜 → 拉取 → 合并能力表 → 落缓存。
/// 拉取失败时回退**最后一次成功缓存**（带原 revision 与时间戳语义——
/// Host 侧按 revision 判断新鲜度，不会把旧目录当成新的）。
pub(crate) fn load_catalog(
    paths: &FlowPaths,
    deps: &impl FlowTransport,
    mode: &str,
) -> Result<Catalog, CatalogError> {
    let tokens = ensure_fresh_access(paths, deps, mode)
        .map_err(|error| match error {
            RefreshError::ReauthRequired(detail) => {
                CatalogError::Reauth(format!("授权已失效（{detail}）；请重新登录"))
            }
            RefreshError::Message(detail) => CatalogError::Message(detail),
        })?
        .ok_or_else(|| CatalogError::Message("尚未登录；请先在工作台登录".into()))?;

    let url = format!("{}{MODELS_PATH}", paths.issuer_base);
    let fetched = fetch_with_token(deps, &url, &tokens.access_token)?;
    let revision = fetched.revision;
    let entries: Vec<CatalogEntry> = fetched.models.into_iter().map(merge_capability).collect();
    let catalog = Catalog {
        revision,
        capability_revision: CAPABILITY_REVISION,
        entries,
    };
    let cache = catalog_cache_path(paths, &tokens.sub);
    // 缓存写失败不失败目录（缓存只是兜底），但要留痕。
    if let Err(error) = std::fs::write(
        &cache,
        format!("{}\n", serde_json::to_string(&catalog).unwrap_or_default()),
    ) {
        let line = format!("模型目录缓存写入失败（{}）：{error}", cache.display());
        crate::shell::shell_events::record("openai-catalog", &line);
    }
    Ok(catalog)
}

/// 只读缓存（诊断与「拉取失败但能看到上次目录」的展示用）。
pub(crate) fn cached_catalog(paths: &FlowPaths, sub: &str) -> Option<Catalog> {
    let text = std::fs::read_to_string(catalog_cache_path(paths, sub)).ok()?;
    serde_json::from_str(&text).ok()
}

struct FetchedModel {
    id: String,
    name: String,
}

struct Fetched {
    revision: String,
    models: Vec<FetchedModel>,
}

/// 拉取并防御式解析：`data`/`models` 两种键名都接受（形状未验证，联调
/// 时收紧）；模型 id 必需，显示名缺省取 slug/id。
fn fetch_with_token(
    deps: &impl FlowTransport,
    url: &str,
    access_token: &str,
) -> Result<Fetched, CatalogError> {
    let text = deps
        .get_json_with_auth(url, access_token)
        .map_err(|failure| match failure {
            Failure::Status(401, _) | Failure::Status(403, _) => {
                CatalogError::Reauth("模型目录请求被拒绝（令牌可能已失效）；请重新登录".into())
            }
            other => CatalogError::Message(other.message(&[])),
        })?;
    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| CatalogError::Message(format!("目录响应不是有效 JSON：{error}")))?;
    let list = value
        .get("data")
        .or_else(|| value.get("models"))
        .and_then(|v| v.as_array())
        .ok_or_else(|| {
            CatalogError::Message("目录响应形状不认识（既无 data 也无 models 数组）".into())
        })?;
    let models: Vec<FetchedModel> = list
        .iter()
        .filter_map(|item| {
            let id = item
                .get("id")
                .or_else(|| item.get("slug"))?
                .as_str()?
                .to_string();
            let name = item
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or(&id)
                .to_string();
            Some(FetchedModel { id, name })
        })
        .collect();
    // revision：响应自带则用之（字符串化）；没有就退化为条目数指纹。
    let revision = value
        .get("revision")
        .map(|v| v.to_string())
        .unwrap_or_else(|| format!("count-{}", models.len()));
    Ok(Fetched { revision, models })
}

fn merge_capability(model: FetchedModel) -> CatalogEntry {
    match CAPABILITY_TABLE.iter().find(|(id, _)| *id == model.id) {
        Some((_, capability)) => CatalogEntry {
            id: model.id,
            name: model.name,
            context_window: capability.context_window,
            efforts: if capability.efforts.is_empty() {
                None
            } else {
                Some(capability.efforts.iter().map(|s| s.to_string()).collect())
            },
            capability: "verified".into(),
        },
        None => CatalogEntry {
            id: model.id,
            name: model.name,
            context_window: None,
            efforts: None,
            // 未知能力：不发 contextWindow（历史压缩阈值不失真）、不发
            // efforts（不显示未经验证的档位）——设计 §6.1/§7 的纪律。
            capability: "unknown".into(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openai::flow::tests::{flow_test_paths, MockIssuer};
    use crate::openai::http::{read_request, write_response};
    use std::net::TcpListener;
    use std::sync::atomic::AtomicBool;

    /// 带鉴权的目录 mock：`/v1/models` 校验 Bearer 后回固定清单。
    /// 挂在 MockIssuer 同款回环模式上（独立小服务，校验不到就 401）。
    fn serve_models_once(body: &'static str, expect_token: &'static str) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let Ok(request) = read_request(&mut stream) else {
                    continue;
                };
                if request.path != MODELS_PATH {
                    let _ = write_response(&mut stream, 404, "application/json", "{}");
                    continue;
                }
                let authorized = request
                    .headers
                    .get("authorization")
                    .is_some_and(|value| value == &format!("Bearer {expect_token}"));
                if !authorized {
                    let _ = write_response(&mut stream, 401, "application/json", "{}");
                    continue;
                }
                let _ = write_response(&mut stream, 200, "application/json", body);
            }
        });
        port
    }

    #[test]
    fn catalog_merge_and_cache_roundtrip() {
        let issuer = MockIssuer::spawn();
        let transport = issuer.transport();
        let paths = flow_test_paths("catalog", issuer.port());
        let cancel = AtomicBool::new(false);
        crate::openai::flow::run_authorize(&paths, &transport, "release", &cancel).unwrap();

        // 目录 mock 独立端口：真实会挂在 issuer 基址，测试里 issuer_base
        // 已被 authorize 用掉——直接换一个 FlowPaths 指向 models mock，
        // vault 复用同一个（同 mode_dir）。
        let models_port = serve_models_once(
            r#"{"data":[{"id":"gpt-x","name":"GPT X"},{"id":"gpt-y"}]}"#,
            "at-mock",
        );
        let mut catalog_paths = paths.clone();
        catalog_paths.issuer_base = format!("http://127.0.0.1:{models_port}");
        let catalog = load_catalog(&catalog_paths, &transport, "release").unwrap();
        assert_eq!(catalog.capability_revision, CAPABILITY_REVISION);
        assert_eq!(catalog.entries.len(), 2);
        // 未验证能力：不发档位、不发容量、capability=unknown。
        let entry = catalog.entries.iter().find(|e| e.id == "gpt-x").unwrap();
        assert_eq!(entry.capability, "unknown");
        assert_eq!(entry.context_window, None);
        assert_eq!(entry.efforts, None);
        // 缓存落盘且可回读。
        let cached = cached_catalog(&catalog_paths, "mock-sub-1").unwrap();
        assert_eq!(cached, catalog);
    }

    #[test]
    fn unauthorized_catalog_asks_for_relogin() {
        let issuer = MockIssuer::spawn();
        let transport = issuer.transport();
        let paths = flow_test_paths("catalog-401", issuer.port());
        let cancel = AtomicBool::new(false);
        crate::openai::flow::run_authorize(&paths, &transport, "release", &cancel).unwrap();
        // 错令牌的目录端点 → Reauth。
        let port = serve_models_once(r#"{"data":[]}"#, "wrong-token");
        let mut catalog_paths = paths.clone();
        catalog_paths.issuer_base = format!("http://127.0.0.1:{port}");
        match load_catalog(&catalog_paths, &transport, "release") {
            Err(CatalogError::Reauth(_)) => {}
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn fetched_shape_accepts_data_and_models_keys() {
        let deps = ShapeDeps(r#"{"models":[{"slug":"gpt-z"}]}"#);
        let fetched = fetch_with_token(&deps, "http://x/v1/models", "token").unwrap();
        assert_eq!(fetched.models.len(), 1);
        assert_eq!(fetched.models[0].id, "gpt-z");
        assert_eq!(fetched.models[0].name, "gpt-z");
    }

    struct ShapeDeps(&'static str);

    impl crate::openai::flow::FlowTransport for ShapeDeps {
        fn get_json(&self, _url: &str) -> Result<String, Failure> {
            Ok(self.0.to_string())
        }
        fn get_json_with_auth(&self, _url: &str, _bearer: &str) -> Result<String, Failure> {
            Ok(self.0.to_string())
        }
        fn post_json(&self, _: &str, _: &str) -> Result<String, Failure> {
            unreachable!()
        }
        fn post_form(&self, _: &str, _: &[(&str, &str)]) -> Result<String, Failure> {
            unreachable!()
        }
        fn open_browser(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn now_unix(&self) -> u64 {
            1_700_000_000
        }
        fn keyring_get(&self, _: &str, _: &str) -> Result<String, String> {
            Err(String::new())
        }
        fn keyring_put(&self, _: &str, _: &str, _: &str) -> Result<(), String> {
            Ok(())
        }
    }
}
