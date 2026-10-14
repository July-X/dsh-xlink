//! 内置插件的**回环桥接**：让跑在内核里的插件能把「立刻拉上游」这个动作
//! 交给桌面壳执行。
//!
//! ## 为什么需要它
//!
//! `/ponytail-update` 是一条**技能**，由模型在工作台里调用；而同步这件事
//! 必须在壳这一侧做（出网纪律、代理路由、SRI 校验、解包防穿越都只有壳这一侧
//! 有，见 [`crate::plugins::builtin::upstream`]）。插件在内核进程里、壳在另一个
//! 进程里，于是需要一条两者之间的通道。
//!
//! ## 为什么是「插件来请求壳」，而不是反过来
//!
//! 反过来（壳主动推给内核）需要长连接或轮询；正过来只要一个一次性请求。
//! 形状与 [`crate::openai::bridge`] 相同：`Authorization: Bearer <令牌>`，
//! 令牌按「(族, 实例)」一代一换，只绑回环。
//!
//! **这不是针对恶意插件的沙盒**（dsh 插件本来就拥有宿主进程权限，与 openai
//! 桥接同一条前提）。令牌防的是**其它本地进程**顺手发请求；随机端口 + 令牌
//! 一起挡住跨源浏览器请求。
//!
//! 只有一个端点：`POST /v1/builtin/sync`。加端点时想清楚「谁会调它」——
//! 这个桥的调用方是模型能触达的代码，能力面越小越好。
//!
//! ## 凭据有第二条路
//!
//! 地址与令牌除环境变量外，壳还会在每个已物化插件的根目录写一份 0600 的
//! `.xlink-bridge.json`：模型跑 `adapter/update.mjs` 的那条链**不继承内核的环境**，
//! 只走 env 时它稳定报「找不到同步桥」而桥其实活着（2026-10-12 真机实测）。
//! 见 [`super::bridge_credential`]。

use std::collections::HashMap;
use std::fs;
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use rand::Rng;

/// 一个运行中的桥接代次。
struct Bridge {
    addr: std::net::SocketAddr,
    token: String,
    shutdown: Arc<AtomicBool>,
    /// 凭据文件路径。`Drop` 时删掉——留着会让下一代的脚本拿着死令牌去请求，
    /// 报一个比真实原因更难查的 401。
    credentials: Vec<PathBuf>,
}

impl Drop for Bridge {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        // 唤醒阻塞在 accept 上的线程：连一次自己。
        let _ = TcpStream::connect(self.addr);
        for path in &self.credentials {
            let _ = fs::remove_file(path);
        }
    }
}

fn bridges() -> &'static Mutex<HashMap<String, Arc<Bridge>>> {
    static REGISTRY: OnceLock<Mutex<HashMap<String, Arc<Bridge>>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

fn new_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// 令牌的环境变量名（与地址的一起交给内核进程）。
pub(crate) const TOKEN_ENV: &str = "DSH_XLINK_BUILTIN_BRIDGE_TOKEN";
/// 地址的环境变量名。
pub(crate) const URL_ENV: &str = "DSH_XLINK_BUILTIN_BRIDGE_URL";

/// 起一代桥接并返回 `Some((地址, 令牌))`；没有需要桥接的内置插件时返回 `None`。
///
/// 调用点只有启动内核那一处，且**起不来不阻断启动**：插件的「检查更新」按钮
/// （面板里那条命令）不依赖它，少一个能力不该让工作台起不来。
pub(crate) fn launch(dsh_home: &Path, scope_key: &str) -> Option<(String, String)> {
    let listener = TcpListener::bind("127.0.0.1:0").ok()?;
    let addr = listener.local_addr().ok()?;
    let token = new_token();
    let url = format!("http://{addr}");
    let bridge = Arc::new(Bridge {
        addr,
        token: token.clone(),
        shutdown: Arc::new(AtomicBool::new(false)),
        credentials: super::bridge_credential::write_all(dsh_home, &url, &token),
    });
    bridges()
        .lock()
        .ok()?
        .insert(scope_key.to_string(), bridge.clone());
    let dsh_home = dsh_home.to_path_buf();
    let scope_key = scope_key.to_string();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            if bridge.shutdown.load(Ordering::SeqCst) {
                break;
            }
            let Ok(mut stream) = stream else { continue };
            let token = bridge.token.clone();
            let dsh_home = dsh_home.clone();
            let scope_key = scope_key.clone();
            // 每个连接一个线程：端点只有一个且是一次性请求，串行化反而会让
            // 一个慢请求堵住后面的（模型那边是并发调用的）。
            std::thread::spawn(move || serve(&mut stream, &token, &dsh_home, &scope_key));
        }
    });
    Some((url, token))
}

fn serve(stream: &mut TcpStream, token: &str, dsh_home: &Path, scope_key: &str) {
    use crate::memory::routes;
    let Ok(request) = crate::openai::http::read_request(stream) else {
        return;
    };
    let authorized = request
        .headers
        .get("authorization")
        .is_some_and(|value| value == &format!("Bearer {token}"));
    if !authorized {
        let _ = crate::openai::http::write_response(
            stream,
            401,
            "application/json",
            r#"{"error":"unauthorized"}"#,
        );
        return;
    }
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/v1/ping") => {
            let _ = crate::openai::http::write_response(
                stream,
                200,
                "application/json",
                r#"{"ok":true}"#,
            );
        }
        (method, path) if method == "GET" && path.starts_with("/v1/memory/context") => {
            routes::context(stream, path, scope_key);
        }
        (method, path) if method == "GET" && path.starts_with("/v1/memory/note") => {
            routes::note(stream, path, scope_key);
        }
        (method, path) if method == "GET" && path.starts_with("/v1/memory/jobs") => {
            routes::jobs(stream, path, scope_key);
        }
        (method, path) if method == "GET" && path.starts_with("/v1/memory/read") => {
            routes::read(stream, path, scope_key);
        }
        (method, path) if method == "GET" && path.starts_with("/v1/memory/propose") => {
            routes::propose(stream, path, scope_key);
        }
        (method, path) if method == "GET" && path.starts_with("/v1/builtin/sync") => {
            // **GET + 查询串，不读请求体**：`openai::http::read_request` 只保证读到
            // 请求头，body 可能在同一次 read 里到达也可能没有（TCP 无消息边界）。
            // 拿 body_prefix 当请求体在「客户端分两次发」的写法下会读到空串，
            // 而症状是「插件名缺失」——查不出原因的那种。
            let plugin = query_value(path, "plugin").unwrap_or_default();
            if plugin.is_empty() {
                let _ = crate::openai::http::write_response(
                    stream,
                    400,
                    "application/json",
                    r#"{"error":"missing plugin"}"#,
                );
                return;
            }
            let Some(spec) = super::builtins::find(&plugin) else {
                let _ = crate::openai::http::write_response(
                    stream,
                    404,
                    "application/json",
                    r#"{"error":"not a builtin plugin"}"#,
                );
                return;
            };
            if spec.upstream.is_none() {
                let _ = crate::openai::http::write_response(
                    stream,
                    409,
                    "application/json",
                    r#"{"error":"plugin has no upstream"}"#,
                );
                return;
            }
            // 同步是慢的（下载 + 解包），而模型那边的工具调用有超时。这条端点
            // **同步等**，让调用方拿到真实结果而不是一个「已受理」——技能要把
            // 结果汇报给用户，拿到「已受理」就只能空口说「正在拉」。
            let state = super::upstream::sync(dsh_home, spec, true);
            let body = serde_json::json!({
                "plugin": spec.id,
                "displayName": spec.display_name,
                "version": state.version,
                "error": state.error,
                "checkedAt": state.checked_at,
            })
            .to_string();
            let _ = crate::openai::http::write_response(stream, 200, "application/json", &body);
        }
        _ => {
            let _ = crate::openai::http::write_response(
                stream,
                404,
                "application/json",
                r#"{"error":"not found"}"#,
            );
        }
    }
}

/// 从查询串里取一个值。读不出来返回 `None` 而不是猜一个默认：猜中的话
/// **替另一个插件拉了更新**，而用户毫无察觉。
fn query_value(path: &str, key: &str) -> Option<String> {
    let query = path.split_once('?')?.1;
    for pair in query.split('&') {
        if let Some((name, value)) = pair.split_once('=') {
            if name == key {
                return Some(value.to_string());
            }
        }
    }
    None
}

/// 取一个查询串参数并做百分号解码（记忆那几条路由用）。
pub(crate) fn query_decoded(path: &str, key: &str) -> String {
    query_value(path, key)
        .map(|value| percent_decode(&value))
        .unwrap_or_default()
}

/// `%XX` 与 `+` 的百分号解码。
///
/// 记忆的标题、正文与项目路径基本都是中文或含空格，插件按 URL 规范编码后送过来；
/// 不解码就会把 `%E4%B8%AD%E6%96%87` 原样存进记忆库——用户看到的是一条乱码，
/// 而链路上每一处都不报错。非法转义**按原样保留**（丢字符比留一个 `%` 更糟）。
fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).ok();
                match hex.and_then(|text| u8::from_str_radix(text, 16).ok()) {
                    Some(byte) => {
                        out.push(byte);
                        index += 3;
                    }
                    None => {
                        out.push(bytes[index]);
                        index += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("xlink-bridge-{}-{}", std::process::id(), tag));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn query_value_reads_the_plugin_name() {
        assert_eq!(
            query_value("/v1/builtin/sync?plugin=ponytail", "plugin").as_deref(),
            Some("ponytail")
        );
        assert_eq!(
            query_value("/v1/builtin/sync?a=1&plugin=ponytail&b=2", "plugin").as_deref(),
            Some("ponytail")
        );
    }

    /// 任务与租约的路由在**真桥**上跑一遍：排任务 → 领取（拿回令牌与冻结游标）→ 续期 → 提交，
    /// 以及**拿起令牌交不上**（§8-6 的迟到结果检查）。
    ///
    /// 这里不碰模型与内核：状态机是纯的，路由只做「读表 → 改 → 写回」——正是这一层该验的东西。
    /// 本地模型的调用在 P2 接线的下一步（H4 之后）。
    #[test]
    fn memory_job_routes_work_over_the_bridge() {
        use crate::memory::store::Namespace;

        let home = temp_dir("memory-jobs");
        let _guard = crate::tests::scoped_xlink_home(&home);
        let dsh_home = home.join("instance");
        let (url, token) = launch(&dsh_home, "dsh:default-dev").expect("bridge");
        let addr = url.trim_start_matches("http://").to_string();

        // 授权先验：这一族路由和记忆那几条一样，**没有 Bearer 一律拒**。
        let unauthorized = http_get(&addr, "/v1/memory/jobs/claim", None);
        assert!(unauthorized.contains("401"), "{unauthorized}");

        // 一开始没东西可领：**不是错误**，答 `token: null`（插件按节奏来问，不该收到 5xx）。
        let empty = http_get(&addr, "/v1/memory/jobs/claim", Some(&token));
        assert!(empty.contains(r#""token":null"#), "{empty}");

        // 开关关着（默认）→ 「获许可的会话」不成立，答 `disabled` 而不是排任务。
        let blocked = http_get(
            &addr,
            "/v1/memory/jobs/enqueue?session=s-1&cursor=9&extractor=v1",
            Some(&token),
        );
        assert!(blocked.contains(r#""outcome":"disabled""#), "{blocked}");

        // 打开开关（用户同意用记忆）之后才排得进去。
        let namespace = Namespace::for_bridge("dsh", "default-dev");
        crate::memory::store::set_use_approved(&namespace, true).expect("enable");
        let queued = http_get(
            &addr,
            "/v1/memory/jobs/enqueue?session=s-1&cursor=9&extractor=v1",
            Some(&token),
        );
        assert!(queued.contains(r#""outcome":"added""#), "{queued}");

        // 同一个键再排一次：已经在跑？还没有——先领再排才是「在跑」。
        let claimed = http_get(&addr, "/v1/memory/jobs/claim", Some(&token));
        assert!(
            claimed.contains(r#""cursor":9"#),
            "领取要带回冻结游标：{claimed}"
        );
        assert!(claimed.contains(r#""attempts":1"#), "{claimed}");
        let start = claimed.find(r#""token":""#).expect("token") + 9;
        let lease = claimed[start..start + 32].to_string();
        assert!(lease.chars().all(|c| c.is_ascii_hexdigit()), "{lease}");

        // 领取之后同一个键不再排（运行中的任务保留自己的冻结输入），也领不到第二个
        // （每命名空间同时只有一个在跑）。
        let again = http_get(
            &addr,
            "/v1/memory/jobs/enqueue?session=s-1&cursor=9&extractor=v1",
            Some(&token),
        );
        assert!(again.contains(r#""outcome":"already_running""#), "{again}");
        let second = http_get(&addr, "/v1/memory/jobs/claim", Some(&token));
        assert!(second.contains(r#""token":null"#), "{second}");

        let renewed = http_get(
            &addr,
            &format!("/v1/memory/jobs/renew?token={lease}"),
            Some(&token),
        );
        assert!(renewed.contains(r#""renewed":true"#), "{renewed}");

        // 拿一个**错的**令牌提交：交不上（这正是「迟到的结果不许覆盖新的」那一条）。
        let late = http_get(&addr, "/v1/memory/jobs/finish?token=deadbeef", Some(&token));
        assert!(late.contains(r#""finished":false"#), "{late}");

        let finished = http_get(
            &addr,
            &format!("/v1/memory/jobs/finish?token={lease}"),
            Some(&token),
        );
        assert!(finished.contains(r#""finished":true"#), "{finished}");
        // 交过之后同一个令牌不能再交（任务已经不在跑）。
        let twice = http_get(
            &addr,
            &format!("/v1/memory/jobs/finish?token={lease}"),
            Some(&token),
        );
        assert!(twice.contains(r#""finished":false"#), "{twice}");

        // 状态与落盘：`done` 计一，且**重启之后还在**（路由每次都读盘、改完写回）。
        let status = http_get(&addr, "/v1/memory/jobs/status", Some(&token));
        assert!(status.contains(r#""done":1"#), "{status}");
        let on_disk = crate::memory::jobs::load(&namespace).expect("load");
        assert_eq!(on_disk.jobs.len(), 1);
        assert_eq!(on_disk.jobs[0].state, crate::memory::jobs::JOB_DONE);
        assert_eq!(on_disk.jobs[0].key.cursor, 9);
    }

    /// 提取结果落地要过四道闸（§8-6）：租约还在跑、开关还开着、会话对得上、引用序号不超过
    /// 冻结的截止序号。每道都单独验一次——**没有这一层，一个被接管的旧任务能把它那份
    /// 过期结果悄悄塞进待确认**。
    #[test]
    fn extraction_submit_is_guarded_by_the_lease() {
        use crate::memory::store::{self, ListFilter, Namespace};

        let home = temp_dir("memory-submit");
        let _guard = crate::tests::scoped_xlink_home(&home);
        let dsh_home = home.join("instance");
        let (url, token) = launch(&dsh_home, "dsh:default-dev").expect("bridge");
        let addr = url.trim_start_matches("http://").to_string();
        let namespace = Namespace::for_bridge("dsh", "default-dev");

        store::set_use_approved(&namespace, true).expect("enable");
        let queued = http_get(
            &addr,
            "/v1/memory/jobs/enqueue?session=s-1&cursor=9&extractor=v1",
            Some(&token),
        );
        assert!(queued.contains(r#""outcome":"added""#), "{queued}");
        let claimed = http_get(&addr, "/v1/memory/jobs/claim", Some(&token));
        let start = claimed.find(r#""token":""#).expect("token") + 9;
        let lease = claimed[start..start + 32].to_string();

        let submit = |params: &str| {
            http_get(
                &addr,
                &format!("/v1/memory/jobs/submit?{params}"),
                Some(&token),
            )
        };

        // ① 令牌不对：任务已经不在跑了。
        let bad = submit("token=deadbeef&session=s-1&end_seq=9&scope=general&title=a&body=b");
        assert!(bad.contains(r#""submitted":false"#), "{bad}");
        assert!(bad.contains("租约"), "{bad}");

        // ② 引用的序号超过了任务冻结的截止序号（9）。
        let beyond = submit(&format!(
            "token={lease}&session=s-1&end_seq=10&scope=general&title=a&body=b"
        ));
        assert!(beyond.contains("截止序号"), "{beyond}");

        // ③ 来源会话对不上。
        let other_session = submit(&format!(
            "token={lease}&session=s-2&end_seq=9&scope=general&title=a&body=b"
        ));
        assert!(other_session.contains("来源会话"), "{other_session}");

        // ④ 正常提交：候选落地，且**来源引用由这一侧按任务键写**。
        let ok = submit(&format!(
            "token={lease}&session=s-1&start_seq=1&end_seq=9&scope=general&title=run+check&body=before+commit"
        ));
        assert!(ok.contains(r#""submitted":true"#), "{ok}");
        let view = store::list(
            &namespace,
            &ListFilter {
                include_candidates: true,
                ..ListFilter::default()
            },
        );
        assert_eq!(view.candidates.len(), 1, "{:?}", view.candidates);
        let candidate = &view.candidates[0];
        assert_eq!(candidate.source.kind, "extract");
        assert_eq!(candidate.source.session, "s-1");
        assert_eq!(
            (candidate.source.start_seq, candidate.source.end_seq),
            (1, 9)
        );
        assert_eq!(candidate.source.revision, "v1", "来源引用里带着提取器版本");
        assert_eq!(
            candidate.state, "pending",
            "候选只能进待确认，用户点保存才成记忆"
        );

        // ⑤ 开关中途关掉：这次提取不收。
        store::set_use_approved(&namespace, false).expect("disable");
        let off = submit(&format!(
            "token={lease}&session=s-1&end_seq=9&scope=general&title=c&body=d"
        ));
        assert!(off.contains("开关"), "{off}");
        store::set_use_approved(&namespace, true).expect("enable again");

        // ⑥ 任务交掉之后，**同一个令牌不再算数**（迟到的结果不许落地）。
        let finished = http_get(
            &addr,
            &format!("/v1/memory/jobs/finish?token={lease}"),
            Some(&token),
        );
        assert!(finished.contains(r#""finished":true"#), "{finished}");
        let late = submit(&format!(
            "token={lease}&session=s-1&end_seq=9&scope=general&title=e&body=f"
        ));
        assert!(late.contains(r#""submitted":false"#), "{late}");
        let view = store::list(
            &namespace,
            &ListFilter {
                include_candidates: true,
                ..ListFilter::default()
            },
        );
        assert_eq!(view.candidates.len(), 1, "被拒的那些一条都没落地");
    }

    /// **P2 整条数据链的验收**（§8、§10 的「自动提取」那一半，不需要模型）：
    /// 排任务 → 领租约 → 提交候选 → 用户批准 → 它**真的能被检索到并注入**。
    ///
    /// 单独一条端到端用例的理由：每一段都有自己的用例，但**接起来**才验证得了那些没人单独
    /// 承诺的东西——来源引用会不会让候选列表读不出来、批准之后正文还是不是原来那句、
    /// 检索的命中判据认不认它。这些正是「环节都对、合起来不通」的典型位置。
    #[test]
    fn the_extraction_chain_lands_a_retrievable_memory() {
        use crate::memory::store::{self, ListFilter, Namespace};

        let home = temp_dir("memory-chain");
        let _guard = crate::tests::scoped_xlink_home(&home);
        let dsh_home = home.join("instance");
        let (url, token) = launch(&dsh_home, "dsh:default-dev").expect("bridge");
        let addr = url.trim_start_matches("http://").to_string();
        let namespace = Namespace::for_bridge("dsh", "default-dev");

        // 用户同意用记忆 → 排任务 → 领租约。
        store::set_use_approved(&namespace, true).expect("enable");
        let queued = http_get(
            &addr,
            "/v1/memory/jobs/enqueue?session=s-1&cursor=12&extractor=v1",
            Some(&token),
        );
        assert!(queued.contains(r#""outcome":"added""#), "{queued}");
        let claimed = http_get(&addr, "/v1/memory/jobs/claim", Some(&token));
        let start = claimed.find(r#""token":""#).expect("token") + 9;
        let lease = claimed[start..start + 32].to_string();

        // 提取（这里由用例代替模型）提交一条候选。正文里带中文与空格，顺带验证整条链的编解码。
        let body = "%E6%8F%90%E4%BA%A4%E5%89%8D%E5%85%88%E8%B7%91%20npm%20run%20check";
        let submitted = http_get(
            &addr,
            &format!(
                "/v1/memory/jobs/submit?token={lease}&session=s-1&start_seq=1&end_seq=12&\
                 scope=general&title=%E6%8F%90%E4%BA%A4%E5%89%8D%E5%85%88%E8%B7%91%E6%A3%80%E6%9F%A5&body={body}"
            ),
            Some(&token),
        );
        assert!(submitted.contains(r#""submitted":true"#), "{submitted}");

        // 候选是**待确认**，不是记忆。
        let view = store::list(
            &namespace,
            &ListFilter {
                include_candidates: true,
                ..ListFilter::default()
            },
        );
        assert_eq!(view.memories.len(), 0, "候选自己成不了记忆");
        assert_eq!(view.candidates.len(), 1);
        let candidate = view.candidates[0].clone();
        assert_eq!(candidate.body, "提交前先跑 npm run check", "正文原样落地");

        // 用户批准 → 权威记录。
        let record = store::decide_candidate(&namespace, &candidate.id, true, None)
            .expect("accept")
            .expect("record");
        assert_eq!(record.body, "提交前先跑 npm run check");
        assert_eq!(record.scope, "general");
        assert_eq!(record.source.kind, "extract", "来源跟着候选过来");

        // 检索命中：这条记忆真的会进模型上下文（§10 要的就是这条链通）。
        let context = crate::memory::query::retrieve(
            &namespace,
            "提交前先跑 npm run check",
            "",
            "/any/project",
        )
        .expect("retrieve");
        assert_eq!(context.hits.len(), 1, "{:?}", context.hits);
        assert!(
            context.text.contains("提交前先跑 npm run check"),
            "{}",
            context.text
        );
    }

    /// 读不出来必须回 `None` 而不是猜一个：技能会把 `None` 报成「插件名缺失」，
    /// 而猜一个默认插件的后果是**替另一个插件拉了更新**。
    #[test]
    fn query_value_gives_up_instead_of_guessing() {
        assert_eq!(query_value("/v1/builtin/sync", "plugin"), None);
        assert_eq!(
            query_value("/v1/builtin/sync?other=ponytail", "plugin"),
            None
        );
        assert_eq!(
            query_value("/v1/builtin/sync?plugin=", "plugin").as_deref(),
            Some("")
        );
    }

    /// 中文经 URL 编码后必须还原：不解码会把 `%E4%B8%AD…` 原样存进记忆库，
    /// 而链路上每一处都不报错——用户看到的是一条乱码记忆。
    #[test]
    fn percent_decode_restores_chinese_and_spaces() {
        assert_eq!(percent_decode("%E4%B8%AD%E6%96%87"), "中文");
        assert_eq!(percent_decode("a%20b+c"), "a b c");
        assert_eq!(percent_decode("plain"), "plain");
        // 非法转义按原样保留，不丢字符。
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%ZZ"), "%ZZ");
    }

    #[test]
    fn query_decoded_reads_percent_encoded_values() {
        let path = "/v1/memory/context?q=%E6%9E%84%E5%BB%BA&workspace=%2Ftmp%2Fmy%20repo";
        assert_eq!(query_decoded(path, "q"), "构建");
        assert_eq!(query_decoded(path, "workspace"), "/tmp/my repo");
        assert_eq!(query_decoded(path, "missing"), "");
    }

    /// 桥知道的坐标（family:instance）决定记忆命名空间；调用方传什么都不影响。
    #[test]
    fn memory_namespace_comes_from_the_bridge_binding() {
        let namespace = crate::memory::routes::memory_namespace("dsh:default-dev");
        assert_eq!(namespace.family, "dsh");
        assert_eq!(namespace.instance_id, "default-dev");
        assert!(!namespace.profile.is_empty());
    }

    /// 记忆路由的端到端：桥真的起来、Bearer 真的校验、中文查询真的解码、
    /// 候选真的只进待确认。**不启动内核**——这一层是插件与壳之间的合同，
    /// 内核那一侧（`agent/created` 注册、`pre-step` 取检索）在真机启动时验。
    #[test]
    fn memory_routes_work_over_the_bridge() {
        use crate::memory::store::{
            self, ListFilter, Namespace, SaveInput, SCOPE_PROJECT, STATE_ACCEPTED,
        };

        let home = std::env::temp_dir().join(format!(
            "xlink-bridge-memory-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = fs::remove_dir_all(&home);
        let _guard = crate::tests::scoped_xlink_home(&home);
        let namespace = Namespace::for_bridge("dsh", "default-dev");

        // 种一条项目记忆并打开开关：桥那边应当命中它。
        let seeded = store::save(
            &namespace,
            SaveInput {
                scope: SCOPE_PROJECT.to_string(),
                workspace_path: "/tmp/demo repo".to_string(),
                title: "构建前先跑检查".to_string(),
                body: "提交前跑 npm run check".to_string(),
                ..SaveInput::default()
            },
        )
        .expect("seed memory")
        .id;
        store::set_use_approved(&namespace, true).expect("enable");

        let dsh_home = home.join("dsh-home");
        fs::create_dir_all(&dsh_home).expect("dsh home");
        let (url, token) = launch(&dsh_home, "dsh:default-dev").expect("桥起不来");
        let addr = url.trim_start_matches("http://").to_string();

        let unauthorized = http_get(&addr, "/v1/memory/context?q=x", None);
        assert!(unauthorized.starts_with("HTTP/1.1 401"), "{unauthorized}");

        // 中文查询 + 带空格的路径都要能过：不解码就会命中不到（而链路不报错）。
        let context = http_get(
            &addr,
            "/v1/memory/context?q=%E6%8F%90%E4%BA%A4&workspace=%2Ftmp%2Fdemo%20repo",
            Some(&token),
        );
        assert!(context.starts_with("HTTP/1.1 200"), "{context}");
        assert!(context.contains("构建前先跑检查"), "{context}");
        assert!(context.contains("\"enabled\":true"), "{context}");

        // 调用要留痕（壳侧 shell_events）：插件里的 `ctx.logger` 在这个内核里不进
        // stdout（2026-10-14 实测），没有这行就没法回答「记忆到底有没有被调用过」。
        assert!(
            log_contains(&home, "memory-bridge", "context enabled=true"),
            "记忆路由没有留下调用记录"
        );

        // 「最近注入」（面板那张卡片的数据源）：插件在**真的用了**这次检索之后回报，
        // 只报 id——标题由壳按 id 从库里取，所以断言标题就是断言解析成功。
        let note = http_get(
            &addr,
            &format!(
                "/v1/memory/note?session=s-1&workspace=%2Ftmp%2Fdemo%20repo&enabled=1&ids={seeded}"
            ),
            Some(&token),
        );
        assert!(note.starts_with("HTTP/1.1 200"), "{note}");
        let mine = crate::memory::recent::recent_injections()
            .into_iter()
            .find(|record| record.session == "s-1")
            .expect("注入记录没落进环形");
        assert_eq!(mine.hits, vec!["构建前先跑检查".to_string()], "{mine:?}");
        assert!(mine.enabled);
        assert!(mine.workspace.contains("demo repo"));

        // 开关关着的那一次也要记（「这一回合没注入」是用户要能看见的事实），
        // 而且**一条都没有**时 hits 为空——不能拿上一次的条目冒充。
        let off = http_get(
            &addr,
            "/v1/memory/note?session=s-2&enabled=0&ids=",
            Some(&token),
        );
        assert!(off.starts_with("HTTP/1.1 200"), "{off}");
        let off_note = crate::memory::recent::recent_injections()
            .into_iter()
            .find(|record| record.session == "s-2")
            .expect("关着开关的那一次也该有记录");
        assert!(!off_note.enabled);
        assert!(off_note.hits.is_empty(), "{off_note:?}");

        // 按内核工作区 id 匹配：项目改名 / 换盘之后路径对不上，仍要命中
        // （插件经工作区注册表解析出 id 随 `workspace_id` 参数送过来）。
        store::save(
            &namespace,
            SaveInput {
                scope: SCOPE_PROJECT.to_string(),
                workspace_id: "ws-renamed".to_string(),
                workspace_path: "/old/location".to_string(),
                title: "改名后的项目".to_string(),
                body: "提交前跑检查".to_string(),
                ..SaveInput::default()
            },
        )
        .expect("seed renamed project");
        let by_id = http_get(
            &addr,
            "/v1/memory/context?q=%E6%8F%90%E4%BA%A4&workspace_id=ws-renamed&workspace=%2Fnew%2Flocation",
            Some(&token),
        );
        assert!(by_id.starts_with("HTTP/1.1 200"), "{by_id}");
        assert!(by_id.contains("改名后的项目"), "{by_id}");
        assert!(
            !by_id.contains("构建前先跑检查"),
            "路径对不上的那条不该被 id 查询带出来：{by_id}"
        );

        // 关掉开关：如实回 enabled:false，而不是「没找到」。
        store::set_use_approved(&namespace, false).expect("disable");
        let disabled = http_get(
            &addr,
            "/v1/memory/context?q=%E6%8F%90%E4%BA%A4",
            Some(&token),
        );
        assert!(disabled.contains("\"enabled\":false"), "{disabled}");

        // 候选：只能进待确认，权威记录不变。
        let proposed = http_get(
            &addr,
            "/v1/memory/propose?title=%E5%80%99%E9%80%89&body=%E4%B8%AD%E6%96%87%E6%AD%A3%E6%96%87&scope=project&workspace=%2Ftmp%2Fdemo%20repo&session=s-1",
            Some(&token),
        );
        assert!(proposed.starts_with("HTTP/1.1 200"), "{proposed}");
        let view = store::list(
            &namespace,
            &ListFilter {
                include_candidates: true,
                ..ListFilter::default()
            },
        );
        assert_eq!(view.candidates.len(), 1);
        assert_eq!(view.candidates[0].title, "候选");
        assert_eq!(view.candidates[0].body, "中文正文");
        assert_eq!(view.candidates[0].source.session, "s-1");
        // 两条 = 上面种的两条记录；候选没有变成第三条。
        assert_eq!(view.memories.len(), 2, "候选不能自己变成记录");
        assert!(view.memories.iter().all(|row| row.state == STATE_ACCEPTED));

        // 按 id 读一条完整正文（模型的 memory_read 走这条）：读一次要记一次显式使用。
        let target = view
            .memories
            .iter()
            .find(|row| row.title == "构建前先跑检查")
            .expect("seeded record");
        let read = http_get(
            &addr,
            &format!(
                "/v1/memory/read?id={}&workspace=%2Ftmp%2Fdemo%20repo",
                target.id
            ),
            Some(&token),
        );
        assert!(read.starts_with("HTTP/1.1 200"), "{read}");
        assert!(read.contains("提交前跑 npm run check"), "{read}");

        // **按 id 读也要过项目这道门**：换个项目坐标（或干脆不给坐标）就读不到。
        // 不校验的话，会话历史里留下的旧 id 能把 A 项目的记忆带进 B 项目。
        for denied in [
            format!("/v1/memory/read?id={}&workspace=%2Ftmp%2Fother", target.id),
            format!("/v1/memory/read?id={}", target.id),
        ] {
            let response = http_get(&addr, &denied, Some(&token));
            assert!(
                response.starts_with("HTTP/1.1 404"),
                "跨项目读必须被拒：{denied} → {response}"
            );
            assert!(response.contains("不属于当前项目"), "{response}");
        }
        let after = store::list(
            &namespace,
            &ListFilter {
                scope: String::new(),
                include_candidates: false,
                include_disabled: true,
            },
        );
        let used = after
            .memories
            .iter()
            .find(|row| row.id == target.id)
            .expect("still there");
        assert_eq!(
            used.use_count, 1,
            "显式读取要计数（搜索不算）；被拒的那两次不算"
        );
        assert!(!used.last_used_at.is_empty());

        // **停用也要挡按 id 读**：注入块的表头是 `[id] 标题`，所以 id 一直留在会话
        // 历史里——只挡检索不挡 `memory_read`，用户在面板上按的「停用」对模型就是无效的。
        store::set_state(&namespace, &target.id, crate::memory::store::STATE_DISABLED)
            .expect("disable");
        let after_disable = http_get(
            &addr,
            &format!(
                "/v1/memory/read?id={}&workspace=%2Ftmp%2Fdemo%20repo",
                target.id
            ),
            Some(&token),
        );
        assert!(
            after_disable.starts_with("HTTP/1.1 404"),
            "停用之后按 id 读必须被拒：{after_disable}"
        );
        assert!(after_disable.contains("已被停用"), "{after_disable}");
        // 检索侧同样看不到它了（这条是状态过滤，不是归属过滤）。
        let context_after_disable = http_get(
            &addr,
            "/v1/memory/context?q=%E6%9E%84%E5%BB%BA%E5%89%8D&workspace=%2Ftmp%2Fdemo%20repo",
            Some(&token),
        );
        assert!(
            !context_after_disable.contains(target.id.as_str()),
            "停用之后检索里不该再有它：{context_after_disable}"
        );
        // 被拒的读不计数：上一步那次成功读仍然是 1。
        let counted = store::list(
            &namespace,
            &ListFilter {
                include_disabled: true,
                ..ListFilter::default()
            },
        )
        .memories
        .into_iter()
        .find(|row| row.id == target.id)
        .expect("still there");
        assert_eq!(counted.use_count, 1, "被拒的那次不计数");

        let missing = http_get(&addr, "/v1/memory/read?id=nope", Some(&token));
        assert!(missing.starts_with("HTTP/1.1 404"), "{missing}");

        // 空 id：400，而不是拿着空串去查一遍再回一句「不属于当前项目」。
        let empty_id = http_get(&addr, "/v1/memory/read", Some(&token));
        assert!(empty_id.starts_with("HTTP/1.1 400"), "{empty_id}");

        let _ = fs::remove_dir_all(&home);
    }

    /// 一次极简的 HTTP GET：这条测试只关心「桥怎么回」，不引第二个客户端库。
    fn http_get(addr: &str, path: &str, token: Option<&str>) -> String {
        use std::io::{Read, Write};
        let mut stream = TcpStream::connect(addr).expect("connect");
        let auth = token
            .map(|value| format!("Authorization: Bearer {value}\r\n"))
            .unwrap_or_default();
        let request =
            format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\n{auth}Connection: close\r\n\r\n");
        stream.write_all(request.as_bytes()).expect("write");
        let mut response = String::new();
        stream.read_to_string(&mut response).expect("read");
        response
    }

    /// 临时目录里有没有一份含这两个片段的日志（壳侧留痕的判据）。
    fn log_contains(dir: &Path, name: &str, needle: &str) -> bool {
        let Ok(entries) = fs::read_dir(dir) else {
            return false;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if log_contains(&path, name, needle) {
                    return true;
                }
                continue;
            }
            if !path
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.contains(name))
            {
                continue;
            }
            if let Ok(text) = fs::read_to_string(&path) {
                if text.contains(needle) {
                    return true;
                }
            }
        }
        false
    }

    #[test]
    fn dropping_the_bridge_removes_the_credential_file() {
        use super::super::bridge_credential::CREDENTIAL_FILE;
        let dir = temp_dir("drop");
        let path = dir.join(CREDENTIAL_FILE);
        fs::write(&path, "{}").expect("写凭据");
        assert!(path.exists());
        {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
            let bridge = Bridge {
                addr: listener.local_addr().expect("addr"),
                token: "t".to_string(),
                shutdown: Arc::new(AtomicBool::new(false)),
                credentials: vec![path.clone()],
            };
            drop(bridge);
        }
        assert!(!path.exists(), "凭据文件没有随桥一起删掉");
    }
}
