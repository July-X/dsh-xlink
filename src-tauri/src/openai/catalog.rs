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

/// 账号目录的拉取路径：挂在**资源主机**（`auth::OPENAI_RESOURCE`，
/// api.openai.com/v1）下——access token 签给该 resource，issuer 主机
/// （auth.openai.com）上没有这个端点（2026-10-09 实测 401）。
const MODELS_PATH: &str = "/models";

/// 能力表版本（表内容变更时递增；v0 = 空表）。
pub(crate) const CAPABILITY_REVISION: u32 = 0;

/// 目录缓存新鲜窗口（秒）：窗口内的重复请求（工作台每次打开设置都会
/// listModels）直接用缓存，不反复打账号目录端点。过期后照常拉取，
/// 拉取失败仍回退缓存（原 revision，不冒充新）。
const CATALOG_FRESH_SECS: u64 = 60;

/// 随应用交付的精确能力表（P6 逐模型验收后填充；键为精确模型 id）。
const CAPABILITY_TABLE: &[(&str, Capability)] = &[];

/// 模型展示元数据：名称、上下文容量与推理档位。数据源 = Codex CLI 官方
/// 捆绑目录（openai/codex `models-manager/models.json`，2026-10-09 取自
/// main）——与 Codex Desktop 的模型列表同源；账号目录接口（api.openai.
/// com/v1/models）只回 slug、不带这些元数据。
///
/// 已验证模型的本地能力元数据。它不决定账号目录的可见集合；未收录模型
/// 仍保留为 unknown，并使用服务端返回的 display_name 与顺序。
struct ModelPresentation {
    slug: &'static str,
    display_name: &'static str,
    context_window: u64,
    efforts: &'static [&'static str],
}

const MODEL_PRESENTATION: &[ModelPresentation] = &[
    ModelPresentation {
        slug: "gpt-6.1-sol",
        display_name: "GPT-6.1 Sol",
        context_window: 272_000,
        efforts: &["low", "medium", "high", "xhigh", "max", "ultra"],
    },
    ModelPresentation {
        slug: "gpt-6-astra",
        display_name: "GPT-6 Astra",
        context_window: 272_000,
        efforts: &["low", "medium", "high", "xhigh", "max", "ultra"],
    },
    ModelPresentation {
        slug: "gpt-6-sol",
        display_name: "GPT-6 Sol",
        context_window: 272_000,
        efforts: &["low", "medium", "high", "xhigh", "max", "ultra"],
    },
    ModelPresentation {
        slug: "gpt-6-luna",
        display_name: "GPT-6 Luna",
        context_window: 272_000,
        efforts: &["low", "medium", "high", "xhigh", "max"],
    },
    ModelPresentation {
        slug: "gpt-5.6-sol",
        display_name: "GPT-5.6 Sol",
        context_window: 272_000,
        efforts: &["low", "medium", "high", "xhigh", "max", "ultra"],
    },
    ModelPresentation {
        slug: "gpt-5.6-terra",
        display_name: "GPT-5.6 Terra",
        context_window: 272_000,
        efforts: &["low", "medium", "high", "xhigh", "max", "ultra"],
    },
    ModelPresentation {
        slug: "gpt-5.6-luna",
        display_name: "GPT-5.6 Luna",
        context_window: 272_000,
        efforts: &["low", "medium", "high", "xhigh", "max"],
    },
];

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
    /// 拉取时刻（Unix 秒）；新鲜窗口内的后续请求直接用缓存。
    #[serde(default)]
    pub(crate) fetched_at: u64,
}

/// 目录错误：`NotSignedIn` 是目录的**空态**（未登录是前置常态，工作台
/// 不当警告展示）；`Reauth` 要求重新登录；`Message` 稍后重试（用缓存兜底）。
#[derive(Debug)]
pub(crate) enum CatalogError {
    NotSignedIn,
    Reauth(String),
    Message(String),
}

impl CatalogError {
    pub(crate) fn message(&self) -> String {
        match self {
            CatalogError::NotSignedIn => "尚未登录；请先在工作台登录".into(),
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
    load_catalog_with(paths, deps, mode, false)
}

/// `force = true` 跳过新鲜窗口（工作台「刷新模型列表」动作）。
pub(crate) fn load_catalog_with(
    paths: &FlowPaths,
    deps: &impl FlowTransport,
    mode: &str,
    force: bool,
) -> Result<Catalog, CatalogError> {
    let tokens = ensure_fresh_access(paths, deps, mode)
        .map_err(|error| match error {
            RefreshError::ReauthRequired(detail) => {
                CatalogError::Reauth(format!("授权已失效（{detail}）；请重新登录"))
            }
            RefreshError::Message(detail) => CatalogError::Message(detail),
        })?
        .ok_or(CatalogError::NotSignedIn)?;

    let now = deps.now_unix();
    // 新鲜窗口：工作台每次打开设置都会 listModels，窗口内的重复请求
    // 直接用缓存（设计 §6.1 离线缓存语义；窗口外照常拉取）。
    if !force {
        if let Some(cached) = cached_catalog(paths, &tokens.sub) {
            if cached.capability_revision == CAPABILITY_REVISION
                && now.saturating_sub(cached.fetched_at) < CATALOG_FRESH_SECS
            {
                return Ok(cached);
            }
        }
    }

    let url = format!("{}{MODELS_PATH}", paths.models_base);
    let fetched = fetch_with_token(deps, &url, &tokens.access_token)?;
    let revision = fetched.revision;
    let entries: Vec<CatalogEntry> = fetched
        .models
        .into_iter()
        .filter_map(merge_capability)
        .collect();
    let catalog = Catalog {
        revision,
        capability_revision: CAPABILITY_REVISION,
        entries,
        fetched_at: now,
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

/// 桥接 `/v1/models` 的载荷（Host `bridge.js` 的契约形状：`{revision,
/// models}`；revision 把目录与能力表两个版本拼在一起，Host 发送前校验）。
/// 拉取失败时回退活跃账号的最后一次成功缓存（带原 revision，不冒充新）；
/// 连缓存都没有才报错——错误码与文案给 Host 分类用。
pub(crate) fn serve_payload(
    paths: &FlowPaths,
    deps: &impl FlowTransport,
    mode: &str,
) -> Result<String, (u16, String)> {
    match load_catalog(paths, deps, mode) {
        Ok(catalog) => Ok(catalog_to_payload(&catalog)),
        // 未登录 → **空目录载荷（200）**：未登录是前置常态，工作台的模型
        // 选择器不该为它显示「加载失败」（2026-10-09 用户反馈）；登录后
        // 同一端点自然给出真实模型。
        Err(CatalogError::NotSignedIn) => Ok(catalog_to_payload(&Catalog {
            revision: String::from("-"),
            capability_revision: CAPABILITY_REVISION,
            entries: Vec::new(),
            fetched_at: 0,
        })),
        Err(CatalogError::Reauth(detail)) => Err((401, detail)),
        Err(CatalogError::Message(detail)) => {
            // 缓存兜底：活跃账号的最后一次成功目录。
            let sub = crate::openai::flow::account_view(paths, deps, mode)
                .ok()
                .flatten()
                .map(|view| view.sub);
            if let Some(catalog) = sub.as_deref().and_then(|sub| cached_catalog(paths, sub)) {
                // 不在载荷里嵌「已回退」说明：Host 契约形状固定，追加字段
                // 会制造两份解析路径；回退语义由原 revision 表达（Host 按
                // revision 判断新鲜度，旧目录不会冒充新的）。
                return Ok(catalog_to_payload(&catalog));
            }
            Err((503, format!("模型目录不可用：{detail}")))
        }
    }
}

fn catalog_to_payload(catalog: &Catalog) -> String {
    serde_json::json!({
        "revision": format!("{}|cap{}", catalog.revision, catalog.capability_revision),
        "models": catalog.entries.iter().map(|entry| serde_json::json!({
            "id": entry.id,
            "name": entry.name,
            "contextWindow": entry.context_window,
            // Host 适配器的契约是**档位对象**（{id, name}，smoke 桩同形状）：
            // 内核校验 effort.id/name 必须是非空字符串且不重复。注意
            // `entry.efforts` 是 Option：`.iter().flatten()` 才是逐档位迭代——
            // 漏了 flatten 会把整个 Vec 当一个元素产出（id 变成数组 →
            // 内核侧 String 化成逗号拼接串，2026-10-09 实测）。
            "efforts": entry.efforts.iter().flatten().map(|effort| serde_json::json!({
                "id": effort,
                "name": effort,
            })).collect::<Vec<_>>(),
            "capability": entry.capability,
        })).collect::<Vec<_>>(),
    })
    .to_string()
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
            if item.get("visibility").and_then(|v| v.as_str()) != Some("list") {
                return None;
            }
            let name = item
                .get("display_name")
                .or_else(|| item.get("name"))
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

/// 合并本地已验证能力。服务端模型目录决定可见集合、顺序和显示名。
fn merge_capability(model: FetchedModel) -> Option<CatalogEntry> {
    if let Some((_, capability)) = CAPABILITY_TABLE.iter().find(|(id, _)| *id == model.id) {
        return Some(CatalogEntry {
            id: model.id,
            name: model.name,
            context_window: capability.context_window,
            efforts: if capability.efforts.is_empty() {
                None
            } else {
                Some(capability.efforts.iter().map(|s| s.to_string()).collect())
            },
            capability: "verified".into(),
        });
    }
    let presentation = MODEL_PRESENTATION.iter().find(|p| p.slug == model.id);
    Some(CatalogEntry {
        id: model.id,
        name: model.name,
        context_window: presentation.map(|p| p.context_window),
        efforts: presentation.map(|p| p.efforts.iter().map(|s| s.to_string()).collect()),
        capability: presentation.map_or_else(|| "unknown".into(), |_| "codex-catalog".into()),
    })
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
            r#"{"data":[{"slug":"gpt-6.1-sol","visibility":"list","display_name":"Server Sol"},{"slug":"gpt-reserve","visibility":"list","display_name":"Reserve"},{"slug":"gpt-6.1-sol-x","visibility":"list","display_name":"Future"}]}"#,
            "at-1",
        );
        let mut catalog_paths = paths.clone();
        catalog_paths.models_base = format!("http://127.0.0.1:{models_port}");
        let catalog = load_catalog(&catalog_paths, &transport, "release").unwrap();
        assert_eq!(catalog.capability_revision, CAPABILITY_REVISION);
        // 只保留展示表内的 slug（gpt-reserve / 未收录尾缀被过滤），
        // 名称、容量与推理档位来自 Codex 官方目录元数据。
        assert_eq!(catalog.entries.len(), 3);
        let entry = &catalog.entries[0];
        assert_eq!(entry.id, "gpt-6.1-sol");
        assert_eq!(entry.name, "Server Sol");
        assert_eq!(entry.capability, "codex-catalog");
        assert_eq!(entry.context_window, Some(272_000));
        assert_eq!(
            entry.efforts.as_deref(),
            Some(
                [
                    "low".to_string(),
                    "medium".to_string(),
                    "high".to_string(),
                    "xhigh".to_string(),
                    "max".to_string(),
                    "ultra".to_string(),
                ]
                .as_slice()
            )
        );
        // 缓存落盘且可回读。
        let cached = cached_catalog(&catalog_paths, "mock-sub-1").unwrap();
        assert_eq!(cached, catalog);

        // 桥接载荷的档位必须是**对象数组**（{id, name}，字符串值）——Host
        // 适配器按 effort.id / effort.name 取值；曾因 Option::iter 漏
        // flatten 把整个 Vec 塞进单元素，id 退化成数组 → 内核侧串接成一个
        // 档位（2026-10-09 实测）。
        let payload: serde_json::Value =
            serde_json::from_str(&catalog_to_payload(&catalog)).unwrap();
        let efforts = payload["models"][0]["efforts"].as_array().unwrap();
        assert_eq!(efforts.len(), 6);
        for (index, effort) in efforts.iter().enumerate() {
            assert_eq!(
                effort["id"].as_str().unwrap(),
                ["low", "medium", "high", "xhigh", "max", "ultra"][index]
            );
            assert_eq!(
                effort["name"].as_str().unwrap(),
                effort["id"].as_str().unwrap()
            );
        }
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
        catalog_paths.models_base = format!("http://127.0.0.1:{port}");
        match load_catalog(&catalog_paths, &transport, "release") {
            Err(CatalogError::Reauth(_)) => {}
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn fetched_shape_accepts_data_and_models_keys() {
        let deps = ShapeDeps(r#"{"models":[{"slug":"gpt-z","visibility":"list"}]}"#);
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

#[cfg(test)]
mod serve_tests {
    use super::*;
    use crate::openai::flow::tests::{flow_test_paths, MockIssuer};
    use crate::openai::http::{read_request, write_response};
    use std::net::TcpListener;
    use std::sync::atomic::AtomicBool;

    fn serve_models(body: &'static str, token: &'static str) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let Ok(request) = read_request(&mut stream) else {
                    continue;
                };
                let ok = request
                    .headers
                    .get("authorization")
                    .is_some_and(|v| v == &format!("Bearer {token}"));
                if !ok {
                    let _ = write_response(&mut stream, 401, "application/json", "{}");
                    continue;
                }
                let _ = write_response(&mut stream, 200, "application/json", body);
            }
        });
        port
    }

    /// 目录源失败（非授权问题）→ 回退最后一次成功缓存，载荷 revision 不变；
    /// 连缓存都没有 → 503。
    #[test]
    fn serve_payload_falls_back_to_cache_then_503() {
        let issuer = MockIssuer::spawn();
        let transport = issuer.transport();
        let paths = flow_test_paths("serve-fb", issuer.port());
        let cancel = AtomicBool::new(false);
        crate::openai::flow::run_authorize(&paths, &transport, "release", &cancel).unwrap();
        let models_port = serve_models(
            r#"{"data":[{"id":"gpt-6.1-sol","visibility":"list","display_name":"GPT-6.1 Sol"}]}"#,
            "at-1",
        );
        let mut live = paths.clone();
        live.models_base = format!("http://127.0.0.1:{models_port}");
        let payload = serve_payload(&live, &transport, "release").unwrap();
        assert!(payload.contains("GPT-6.1 Sol"));
        let parsed_payload: serde_json::Value = serde_json::from_str(&payload).unwrap();
        let revision_with_cache = parsed_payload["revision"].as_str().unwrap().to_string();

        // 目录源挂掉（死端口）：回退缓存，revision 与上次一致。
        let mut dead = paths.clone();
        let probe = TcpListener::bind("127.0.0.1:0").unwrap();
        let dead_port = probe.local_addr().unwrap().port();
        drop(probe);
        dead.models_base = format!("http://127.0.0.1:{dead_port}");
        // 注意：死端口让发现文档（issuer 基址）失败——load_catalog 里
        // ensure_fresh_access 未过期会短路（不触网），随后目录拉取才触网。
        let fallback = serve_payload(&dead, &transport, "release").unwrap();
        let parsed_fallback: serde_json::Value = serde_json::from_str(&fallback).unwrap();
        let revision = parsed_fallback["revision"].as_str().unwrap().to_string();
        assert_eq!(revision, revision_with_cache);
        assert!(fallback.contains("GPT-6.1 Sol"));
    }

    /// 未登录 → **空目录载荷（200）**：前置常态不是故障，工作台模型
    /// 选择器不该为它显示「加载失败」（2026-10-09 用户反馈）。
    #[test]
    fn serve_payload_without_login_is_empty_catalog() {
        let issuer = MockIssuer::spawn();
        let transport = issuer.transport();
        let paths = flow_test_paths("serve-nologin", issuer.port());
        let payload = serve_payload(&paths, &transport, "release").unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(
            parsed["revision"].as_str().unwrap(),
            format!("-|cap{CAPABILITY_REVISION}")
        );
        assert_eq!(
            parsed["models"].as_array().unwrap().len(),
            0,
            "未登录的目录是空态，不是错误"
        );
    }
}

#[cfg(test)]
mod fresh_window_tests {
    use super::*;
    use crate::openai::flow::tests::{flow_test_paths, MockIssuer};
    use crate::openai::http::{read_request, write_response};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;

    /// 带命中计数的目录 mock：断言「窗口内不触网」的观察点。
    fn serve_counting(body: &'static str, token: &'static str, hits: Arc<AtomicUsize>) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let Ok(request) = read_request(&mut stream) else {
                    continue;
                };
                if request.headers.get("authorization").map(|v| v.as_str())
                    != Some(format!("Bearer {token}").as_str())
                {
                    let _ = write_response(&mut stream, 401, "application/json", "{}");
                    continue;
                }
                hits.fetch_add(1, Ordering::SeqCst);
                let _ = write_response(&mut stream, 200, "application/json", body);
            }
        });
        port
    }

    /// 时钟推进需要 deps 可变——用 Cell 包 now_unix 的返回值。
    #[test]
    fn fresh_window_serves_cache_without_refetch() {
        let issuer = MockIssuer::spawn();
        let transport = issuer.transport();
        let paths = flow_test_paths("fresh", issuer.port());
        let cancel = AtomicBool::new(false);
        crate::openai::flow::run_authorize(&paths, &transport, "release", &cancel).unwrap();

        let hits = Arc::new(AtomicUsize::new(0));
        let hits_for_server = Arc::clone(&hits);
        let models_port = serve_counting(
            r#"{"data":[{"id":"gpt-6.1-sol","visibility":"list","display_name":"GPT-6.1 Sol"}]}"#,
            "at-1",
            hits_for_server,
        );
        let mut catalog_paths = paths.clone();
        catalog_paths.models_base = format!("http://127.0.0.1:{models_port}");

        // 第一次：拉取并落缓存（命中 1）。
        let first = load_catalog(&catalog_paths, &transport, "release").unwrap();
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        assert_eq!(first.entries.len(), 1);
        // 第二次（窗口内）：不触网，直接命中缓存。
        let second = load_catalog(&catalog_paths, &transport, "release").unwrap();
        assert_eq!(hits.load(Ordering::SeqCst), 1, "窗口内不应再次触网");
        assert_eq!(second, first);
    }
}
