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
///
/// **令牌不存这里**：它只被 accept 线程用来校验请求，而那个线程**不能**持
/// `Arc<Bridge>`（它一持，上一代的 `Drop` 就永远等不到最后一个引用，端口与线程
/// 一起漏掉——2026-10-15 评审第 2 条就是这么来的）。字段留一份没人读的令牌只会
/// 骗下一个人「这里还有用」。
struct Bridge {
    addr: std::net::SocketAddr,
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

/// 起一代桥接并返回 `Some((地址, 令牌))`；只有**绑定失败**才返回 `None`。
///
/// 调用点只有启动内核那一处，且**起不来不阻断启动**：插件的「检查更新」按钮
/// （面板里那条命令）不依赖它，少一个能力不该让工作台起不来。
///
/// **这里不判「有没有启用的插件」**（2026-10-15 评审第 7 条提过「没插件就别起桥」）：
/// 记忆路由也挂在这个桥上，而「启用」是每个插件一份 `state.json` 的状态——判错一次
/// 就是「插件装上了、面板显示启用、能力静默失效」，正是评审第 1 条那类故障的形状，
/// 为省一个回环套接字不值当。代价只是：没有插件用它时也占一个随机端口。
pub(crate) fn launch(dsh_home: &Path, scope_key: &str) -> Option<(String, String)> {
    let listener = TcpListener::bind("127.0.0.1:0").ok()?;
    let addr = listener.local_addr().ok()?;
    let token = new_token();
    let url = format!("http://{addr}");
    let shutdown = Arc::new(AtomicBool::new(false));
    let bridge = Arc::new(Bridge {
        addr,
        shutdown: Arc::clone(&shutdown),
        credentials: super::bridge_credential::write_all(dsh_home, &url, &token),
    });
    // **换代必须把上一代放掉**（2026-10-15 评审）：`Drop` 才是「置旗标 + 自连一次
    // 唤醒 accept + 删凭据文件」的执行者，而它只在最后一个 `Arc<Bridge>` 消失时跑。
    // 此前这里丢掉 `insert` 的返回值、accept 线程又 move 捕获了整个 `Arc<Bridge>`，
    // 于是上一代永远活着——端口继续接受连接、旧令牌继续有效，每启一次工作台泄漏
    // 一个线程与一个监听套接字。线程现在只持 `shutdown` 旗标与令牌，注册表那一份
    // 就是最后一个，`drop(old)` 因此真的会执行 `Drop`。
    let old = bridges()
        .lock()
        .ok()?
        .insert(scope_key.to_string(), Arc::clone(&bridge));
    drop(bridge);
    if let Some(old) = old {
        drop(old);
    }
    let dsh_home = dsh_home.to_path_buf();
    let scope_key = scope_key.to_string();
    let accept_token = token.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            if shutdown.load(Ordering::SeqCst) {
                break;
            }
            let Ok(mut stream) = stream else { continue };
            let token = accept_token.clone();
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
    // 读超时，与 `openai::bridge` 同一口径（那边是 5s）：连上来却一个字都不发的
    // 客户端会让这个线程永久阻塞在 read 上。每个连接一个线程，所以单次只泄漏一个
    // 线程，但本机任何进程都能靠反复空连接把线程数堆起来。
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
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
            // 提取任务那七条在**另一个模块**里（2026-10-15 从 `routes.rs` 再拆一层，
            // 见 `memory/jobs_routes.rs` 的模块文档）：它们管的是「一次提取从排队到收尾」，
            // 与「记忆本身的调用」不是一件事。
            crate::memory::jobs_routes::jobs(stream, path, scope_key);
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
        age_jobs();

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
        age_jobs();
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

        // ③′ 来源引用两头倒过来（起点比终点靠后）：不是一段有效范围，拒。
        // 独立评审第 17 条：只有 `end_seq` 有校验时，一个更大的 `start_seq` 会被原样写进库里。
        let reversed = submit(&format!(
            "token={lease}&session=s-1&start_seq=9&end_seq=3&scope=general&title=a&body=b"
        ));
        assert!(reversed.contains("起点比终点还靠后"), "{reversed}");

        // ④ 正常提交：**直接成为记忆**（2026-10-19 起不再等用户点保存），且来源引用由
        // 这一侧按任务键写。
        let ok = submit(&format!(
            "token={lease}&session=s-1&start_seq=1&end_seq=9&scope=general&title=run+check&body=before+commit"
        ));
        assert!(ok.contains(r#""submitted":true"#), "{ok}");
        assert!(
            ok.contains(r#""accepted":true"#),
            "没标冲突/取代的提取结果该直接落地：{ok}"
        );
        let view = store::list(
            &namespace,
            &ListFilter {
                include_candidates: true,
                ..ListFilter::default()
            },
        );
        assert_eq!(view.memories.len(), 1, "{:?}", view.memories);
        let record = &view.memories[0];
        assert_eq!(record.title, "run check");
        assert_eq!(record.body, "before commit");
        assert_eq!(
            record.verification, "unverified",
            "没经过用户确认，不能标成已验证"
        );
        assert_eq!(record.sources[0].kind, "extract");
        assert_eq!(record.sources[0].session, "s-1");
        assert_eq!(
            (record.sources[0].start_seq, record.sources[0].end_seq),
            (1, 9)
        );
        assert_eq!(record.sources[0].revision, "v1", "来源引用里带着提取器版本");
        assert_eq!(view.candidates.len(), 0, "直接落地之后没有待确认的条目");

        // ④′ 模型标了冲突：这条**留在待确认**，不能自动落地（C5 的裁决权在用户手里）。
        let marked = submit(&format!(
            "token={lease}&session=s-1&start_seq=1&end_seq=9&scope=general&title=a&body=b&conflicts=m-1"
        ));
        assert!(marked.contains(r#""submitted":true"#), "{marked}");
        assert!(
            marked.contains(r#""accepted":false"#),
            "带冲突标记的要等用户裁决：{marked}"
        );
        let view = store::list(
            &namespace,
            &ListFilter {
                include_candidates: true,
                ..ListFilter::default()
            },
        );
        assert_eq!(view.memories.len(), 1, "带标记的那条没有落地");
        assert_eq!(view.candidates.len(), 1, "{:?}", view.candidates);
        assert_eq!(view.candidates[0].conflicts_with, vec!["m-1".to_string()]);

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
        assert_eq!(view.memories.len(), 1, "被拒的那些一条都没落地");
        assert_eq!(view.candidates.len(), 1, "只剩带冲突标记的那一条");
    }

    /// **P2 整条数据链的验收**（§8、§10 的「自动提取」那一半，不需要模型）：
    /// 排任务 → 领租约 → 提交 → **直接成为记忆** → 它**真的能被检索到并注入**。
    ///
    /// 单独一条端到端用例的理由：每一段都有自己的用例，但**接起来**才验证得了那些没人单独
    /// 承诺的东西——来源引用会不会让列表读不出来、自动落地之后正文还是不是原来那句、
    /// 检索的命中判据认不认它。这些正是「环节都对、合起来不通」的典型位置。
    ///
    /// 2026-10-19 起这条链少了一步「用户批准」：自动提取的结果直接落成记忆，只有模型标了
    /// 冲突 / 取代的才留在面板等用户裁决（那一半见 `extraction_submit_is_guarded_by_the_lease`）。
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
        age_jobs();
        let claimed = http_get(&addr, "/v1/memory/jobs/claim", Some(&token));
        let start = claimed.find(r#""token":""#).expect("token") + 9;
        let lease = claimed[start..start + 32].to_string();

        // 提取（这里由用例代替模型）提交一条结果。正文里带中文与空格，顺带验证整条链的编解码。
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
        assert!(submitted.contains(r#""accepted":true"#), "{submitted}");

        // **直接是记忆**，不经过待确认。
        let view = store::list(
            &namespace,
            &ListFilter {
                include_candidates: true,
                ..ListFilter::default()
            },
        );
        assert_eq!(view.memories.len(), 1, "提取结果直接成为记忆");
        assert_eq!(
            view.candidates.len(),
            0,
            "没有标冲突 / 取代的条目不留待确认"
        );
        let record = view.memories[0].clone();
        assert_eq!(record.body, "提交前先跑 npm run check", "正文原样落地");
        assert_eq!(record.scope, "general");
        assert_eq!(
            record.verification, "unverified",
            "没经过用户确认的记忆保持未验证"
        );
        assert_eq!(record.sources.len(), 1, "候选的来源落在记录的第一个来源上");
        assert_eq!(record.sources[0].kind, "extract", "来源跟着候选过来");

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

    /// C6：待确认候选**只在提取那一次**随 `/context` 一起回来（`?pending=1`）。
    ///
    /// **这条判据盯的是「运行时上下文一个字都不变」**：带不带 `pending=1`，`text` 与 `hits`
    /// 必须完全一样。面板只列 pending，未确认的东西不该影响模型平时看到什么——而这件事
    /// 没有任何报错会说破，症状只是「模型好像知道了它不该知道的事」。
    #[test]
    fn pending_candidates_come_back_only_when_the_extraction_asks_for_them() {
        use crate::memory::store::{self, Namespace, SubmitOutcome};
        let home = std::env::temp_dir().join(format!(
            "xlink-bridge-pending-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = fs::remove_dir_all(&home);
        let _guard = crate::tests::scoped_xlink_home(&home);
        let namespace = Namespace::for_bridge("dsh", "pending-probe");
        store::set_use_approved(&namespace, true).expect("enable");
        let SubmitOutcome::Candidate(seeded) = store::submit_candidate(
            &namespace,
            store::testkit::candidate_input("用 pnpm", "这个仓库用 pnpm。"),
        )
        .expect("seed candidate") else {
            panic!("新正文应该收成候选");
        };

        let dsh_home = home.join("dsh-home");
        fs::create_dir_all(&dsh_home).expect("dsh home");
        let (url, token) = launch(&dsh_home, "dsh:pending-probe").expect("桥起不来");
        let addr = url.trim_start_matches("http://").to_string();

        let without = http_get(
            &addr,
            "/v1/memory/context?q=%E4%BB%93%E5%BA%93",
            Some(&token),
        );
        let with = http_get(
            &addr,
            "/v1/memory/context?q=%E4%BB%93%E5%BA%93&pending=1",
            Some(&token),
        );
        // 判的是**键**而不是「文本里含不含 pending 这个词」：用带引号的键名匹配，
        // 才不会在将来某个无关字段（比如候选的 `state: "pending"`）出现时误判。
        assert!(
            !without.contains("\"pending\""),
            "不带 pending=1 就不该有那个字段：{without}"
        );
        assert!(
            with.contains(&seeded.id),
            "提取那一次要拿得到那条候选：{with}"
        );
        // **库里有候选、pending 带回来了，而运行时那半的 text 仍然是空的**——这就是
        // 「未确认的东西不进运行时上下文」的可测形状。
        //
        // 反向验：把 pending 的正文拼进 `render_context` 的产物之后，这条立刻红。
        // （早先写过一版 `!with.contains("\"text\":\"用 pnpm\"")`，它依赖 text 恰好在
        // 第一个字符就是那句标题——真把候选拼进去时前面还有别的内容，那条判据会**空过**，
        // 于是「候选进了运行时上下文」这件事没有门禁。这种「精确子串」判据要盯形状，不要盯措辞。）
        assert!(
            with.contains("\"text\":\"\""),
            "候选的正文不许进运行时上下文那一半的 text：{with}"
        );
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

    /// 把任务表里所有待跑任务**做旧**，让它们立刻过了静默期。
    ///
    /// 领取那条路（`/jobs/claim`）现在要求 `now - updated_at_ms >= DEBOUNCE_MS`（评审第 11 条：
    /// 去抖原先只作用于合并，于是「先排、再领」等于每回合立刻一次提取）。走路由的用例不该为了
    /// 这一点等 30 秒，所以显式做旧——**这比给生产代码开一个「跳过静默期」的后门干净**。
    fn age_jobs() {
        let ns = crate::memory::store::Namespace::for_bridge("dsh", "default-dev");
        let _ = crate::memory::jobs::mutate(&ns, |doc, now| {
            for job in doc.jobs.iter_mut() {
                job.updated_at_ms = now.saturating_sub(crate::memory::jobs::DEBOUNCE_MS + 1);
            }
        });
    }

    /// 往真桥上发一条 GET，把响应体取回来（`Connection: close`，读到 EOF）。
    fn bridge_get(addr: &str, token: &str, path: &str) -> String {
        use std::io::{Read, Write};
        let mut stream = TcpStream::connect(addr).expect("连桥");
        stream
            .write_all(
                format!(
                    "GET /v1/memory/{path} HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n\r\n"
                )
                .as_bytes(),
            )
            .expect("写请求");
        let mut raw = String::new();
        // 读失败就是「这个请求被丢掉了」：请求行越过 8 KiB 头缓冲时桥直接重置连接、
        // 一个字都不回（实测 ConnectionReset）。这不是测试的意外，正是那条边界的行为。
        let _ = stream.read_to_string(&mut raw);
        raw.split_once("\r\n\r\n")
            .map(|(_, body)| body.to_string())
            .unwrap_or(raw)
    }

    /// **任务表在真桥上往返一圈**：插件怎么发，壳这边就怎么收。
    ///
    /// 为什么值得一条测试：插件那一侧已经在真内核里验过（它发的就是这几个路由与参数），
    /// 但**壳这一侧的真状态机**（`jobs.rs` 的租约、`routes.rs` 的四道闸）从没和插件对接过——
    /// 探针里的桩桥是自己实现的最小状态机，形状对不对没有第二个人校验。这条测试把两侧钉在一起：
    /// 同一个任务键从 HTTP 进去、领出令牌、带令牌提交、再收尾。
    ///
    /// 任务用**过去的时间**排（`now - 60s`），这样它立刻到期——静默期本身在 `jobs.rs` 里有单测，
    /// 这里不替它睡觉。
    #[test]
    fn the_job_table_round_trips_over_the_real_bridge() {
        let root = temp_dir("jobs-roundtrip");
        // 测试不许碰用户的真实数据目录：这个 guard 把 `DSH_XLINK_HOME` 指到临时目录，
        // 而记忆 store 的路径正是从这里解析的。
        let _home = crate::tests::scoped_xlink_home(&root);
        let ns = crate::memory::store::Namespace::for_bridge("dsh", "probe");
        // `/submit` 在「记忆开关关着」时会拒——那是设计的一部分（§8-6 的四道闸之一），
        // 所以这里先把它打开，验的是**开着**时的正常路径。
        crate::memory::store::set_use_approved(&ns, true).expect("打开记忆开关");

        let key = crate::memory::jobs::JobKey {
            session: "s-roundtrip".to_string(),
            cursor: 16,
            extractor: "xlink-memory-extract/1".to_string(),
            // 手动夹具自己造键，起点给 0（与「新会话第一次提取」同形）。
            from_cursor: 0,
        };
        let past = crate::memory::jobs::now_ms().saturating_sub(60_000);
        crate::memory::jobs::mutate(&ns, |doc, _| {
            crate::memory::jobs::enqueue(doc, key.clone(), past)
        })
        .expect("排任务");

        let (url, token) = launch(&root, "dsh:probe").expect("起桥");
        let addr = url.trim_start_matches("http://").to_string();

        // ① 领：回执的形状就是插件按着读的那一个（`{token, key:{…}}`）。
        let claimed = bridge_get(&addr, &token, "jobs/claim");
        assert!(claimed.contains("\"token\""), "要领到令牌：{claimed}");
        assert!(
            claimed.contains("\"s-roundtrip\""),
            "回执里要有任务键：{claimed}"
        );
        assert!(
            claimed.contains("\"cursor\":16"),
            "游标就是冻结截止序号：{claimed}"
        );
        let lease = claimed
            .split("\"token\":\"")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .expect("取令牌")
            .to_string();

        // ② 提交（插件是**一条候选一次调用**）。
        let submitted = bridge_get(
            &addr,
            &token,
            &format!(
                "jobs/submit?token={lease}&session=s-roundtrip&start_seq=0&end_seq=16&scope=project&workspace_id=&workspace=%2Ftmp%2Fws&title=%E6%8F%90%E4%BA%A4%E5%89%8D%E8%B7%91%E6%A3%80%E6%9F%A5&body=%E8%B7%91%20npm%20run%20check&keywords=%E6%8F%90%E4%BA%A4"
            ),
        );
        assert!(
            submitted.contains("\"submitted\":true"),
            "拿着令牌该能提交：{submitted}"
        );

        // ③ 收尾。
        let finished = bridge_get(&addr, &token, &format!("jobs/finish?token={lease}"));
        assert!(finished.contains("\"finished\":true"), "收尾：{finished}");

        // ④ 收尾之后令牌作废——这正是「不能覆写已提交结果」的一半。
        let again = bridge_get(
            &addr,
            &token,
            &format!(
                "jobs/submit?token={lease}&session=s-roundtrip&start_seq=0&end_seq=16&title=x&body=y"
            ),
        );
        assert!(
            again.contains("\"submitted\":false"),
            "任务收尾之后同一个令牌不该再能提交：{again}"
        );
    }

    /// 正文一长，**请求行本身**就装不下——壳这边的头缓冲是固定的 8 KiB。
    ///
    /// 这条测试量的是真行为，不是猜：中文经百分号编码会膨胀到约 3 倍，而插件那边的候选上限
    /// 是按**原文** 8 KiB 定的（与壳的解析上限对齐）。所以「解析通过」不等于「发得出去」。
    #[test]
    fn an_over_long_submit_path_is_dropped_without_a_body() {
        let root = temp_dir("long-submit");
        let _home = crate::tests::scoped_xlink_home(&root);
        let ns = crate::memory::store::Namespace::for_bridge("dsh", "probe");
        crate::memory::store::set_use_approved(&ns, true).expect("打开记忆开关");
        let key = crate::memory::jobs::JobKey {
            session: "s-long".to_string(),
            cursor: 8,
            extractor: "xlink-memory-extract/1".to_string(),
            // 手动夹具自己造键，起点给 0（与「新会话第一次提取」同形）。
            from_cursor: 0,
        };
        let past = crate::memory::jobs::now_ms().saturating_sub(60_000);
        crate::memory::jobs::mutate(&ns, |doc, _| {
            crate::memory::jobs::enqueue(doc, key.clone(), past)
        })
        .expect("排任务");
        let (url, token) = launch(&root, "dsh:probe").expect("起桥");
        let addr = url.trim_start_matches("http://").to_string();
        let claimed = bridge_get(&addr, &token, "jobs/claim");
        let lease = claimed
            .split("\"token\":\"")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .expect("取令牌")
            .to_string();

        // 把正文按百分号编码手工张开：`中` 编成 9 个字符。**故意**让整条请求行越过 8 KiB。
        let body: String = "中".repeat(1_200).chars().map(|_| "%E4%B8%AD").collect();
        let path = format!(
            "jobs/submit?token={lease}&session=s-long&start_seq=0&end_seq=8&scope=project&title=t&body={body}"
        );
        assert!(
            path.len() > 8192,
            "这条请求行要真的越过缓冲：{}",
            path.len()
        );
        // 实测：连接被重置、没有任何响应体。**这比一个 4xx 更值得记住**——插件那边只会看到
        // 一次传输错误，所以「解析通过 ≠ 发得出去」，插件必须提前按编码后的长度拒。
        let answer = bridge_get(&addr, &token, &path);
        assert!(
            !answer.contains("\"submitted\":true"),
            "越过头缓冲的请求不该被当成成功处理（拿到的是：{}）",
            &answer[..answer.len().min(60)]
        );
        assert!(
            answer.is_empty(),
            "实测这条路径下桥不给任何响应体，拿到的是：{answer}"
        );

        // 同一条任务**没有**被这次失败提交掉：好好发一次仍然成功。
        let ok = bridge_get(
            &addr,
            &token,
            // 用 `general`：`project` 需要绑定**已登记的工作区**，否则壳会拒（这条规则
            // 2026-10-15 在真桥上实测到，插件的 `scopeForWorkspace` 就是照它写的）。
            &format!("jobs/submit?token={lease}&session=s-long&start_seq=0&end_seq=8&scope=general&title=t&body=short"),
        );
        assert!(
            ok.contains("\"submitted\":true"),
            "正常长度仍然要提交成功：{ok}"
        );
    }

    /// **手动夹具：把真桥交给探针**，让内核里那个 Node 插件真的打进来。
    ///
    /// 上面那条测试验的是「路由收得下插件的参数」，这一条验的是「两侧在同一次运行里接得上」：
    /// 真桥 + 真任务表 + 真记忆库，对面是真的内核会话（只有模型是桩）。
    ///
    /// 跑法（`docs/features/memory/memory-progress.md` 的「怎么验」那一节记了完整命令）：
    /// 1. 起这个测试（`--ignored --nocapture`），它把地址与令牌写进 `/tmp/p2probe/bridge-cred.json`；
    /// 2. 用探针跑一次会话，把 `DSH_XLINK_BUILTIN_BRIDGE_URL/TOKEN` 指过来；
    /// 3. `touch /tmp/p2probe/bridge-stop`，测试接着断言「任务进过表、候选落进库」。
    #[test]
    #[ignore = "手动夹具：要和 Node 探针一起跑，见清单的「怎么验」那一节"]
    fn manual_bridge_for_the_node_probe() {
        use std::path::PathBuf;
        let root = temp_dir("manual-probe");
        let _home = crate::tests::scoped_xlink_home(&root);
        let ns = crate::memory::store::Namespace::for_bridge("dsh", "probe");
        crate::memory::store::set_use_approved(&ns, true).expect("打开记忆开关");

        let handshake = PathBuf::from("/tmp/p2probe/bridge-cred.json");
        let stop = PathBuf::from("/tmp/p2probe/bridge-stop");
        let _ = fs::remove_file(&stop);
        let _ = fs::create_dir_all("/tmp/p2probe");

        let (url, token) = launch(&root, "dsh:probe").expect("起桥");
        fs::write(
            &handshake,
            format!("{{\"url\":\"{url}\",\"token\":\"{token}\"}}"),
        )
        .expect("写握手文件");
        println!("[probe] 桥已就绪：{url} → 握手文件 {handshake:?}；等 {stop:?} 出现");

        // 最多等 25 分钟：手动夹具，宁可慢也不要假的「通过」。
        //
        // **为什么从 10 分钟放宽**：C6 的验收要在**同一个夹具进程里连跑两次驱动器**（先说
        // 一句、再说一句改口），而实测一轮 ≈ 8 分钟（真模型回合 + 180 秒静默期 + 一次真模型
        // 提取调用）——10 分钟会在第二次跑完之前就红，而那条红是「超时」不是「没跑通」，
        // 看起来却和真缺陷一模一样（2026-10-16 真机上撞到过：494 秒，两轮都还没跑完）。
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(25 * 60);
        while !stop.exists() {
            assert!(
                std::time::Instant::now() < deadline,
                "等了 25 分钟也没看到停止文件 {stop:?}——探针那一步没跑起来？"
            );
            std::thread::sleep(std::time::Duration::from_millis(200));
        }

        // 断言的是**壳这一侧真的收到了什么**：任务进过表、候选落进库。
        let doc = crate::memory::jobs::load(&ns).expect("读任务表");
        let summary = crate::memory::jobs::summary(&doc);
        println!("[probe] 任务表：{summary:?}");
        // **失败原因就在表里**：插件走 `/jobs/fail` 时把 `stage` 与原因一起报上来，
        // 所以「提取链卡在哪一步」在这边读得到，不必去翻插件的日志。
        for job in &doc.jobs {
            println!(
                "[probe] 任务 {}@{} → {}{}",
                job.key.session,
                job.key.cursor,
                job.state,
                if job.error.is_empty() {
                    String::new()
                } else {
                    format!("：{}", job.error)
                }
            );
        }
        assert!(
            summary.pending + summary.done > 0 || summary.running > 0,
            "任务表里一条都没有——插件的 enqueue 没打进来：{summary:?}"
        );
        let memory = crate::memory::store::load(&ns).expect("读记忆库");
        println!(
            "[probe] 记忆库：已有记忆 {} 条，待确认候选 {} 条",
            memory.memories.len(),
            memory.candidates.len()
        );
        assert!(
            !memory.candidates.is_empty(),
            "候选一条都没落下来——提取链没走通（看插件日志里的 stage 与原因）"
        );
        // **不许有任务带着「基础设施」的错停在那儿**。判据**按 stage 判**，不按词表——
        // 第一版列了五个词（`inactive context` / `拿不到` / `落盘屏障` / `读不到` /
        // `not live in this store`），而 `sessionlog.js` 真会产生的几条原因**一条都不含这些词**
        // （「内核没有提供会话服务（sessions.flush）」「打不开会话日志：…」「读会话日志失败：…」
        // 「这个会话没有 id，读不了它的日志」），于是「只成一条、其余全因服务失活失败」照样绿——
        // 独立评审第 13 条正是这么指出来的（而那条环境限制就写在清单里）。
        //
        // 现在插件在 `/jobs/fail` 里把 **stage 与原因一起报**（第 4 条），所以：
        // · `read` / `model` / `parse` = 链自己的基础设施失败 → 不许出现；
        // · `submit` = 壳的业务拒绝（去重闸、开关关了、来源对不上）→ 正常结果，允许；
        // · **没有 stage 前缀**也算坏（那说明第 4 条的接线被摘了）。
        // **唯一一条环境允许项**（逐字，见清单「怎么验」那节）：sdk 探针里回合之间 `sessions`
        // 服务有时已经不在，插件如实报这句并退回——那是环境，不是链的缺陷。其余任何
        // `read`/`model`/`parse` 失败都算缺陷。
        // 反向验：把这条允许项去掉，本轮夹具立刻红（正是它先红了我才来加这条）——
        // 也就是说这条允许项**是必要的、不是顺手放宽**。
        // 同一件环境事实的两种说法：服务整个不在（`sessions.flush` 拿不到）与
        // 服务在、但这个会话已经不是 live 的那个（`sessions.get(id)` 没给）——
        // 两者都是 sdk 探针的回合拆装造成的，插件都如实报了。
        const ENV_ALLOWED: [&str; 2] = [
            "read：内核没有提供会话服务",
            "read：拿不到这个会话的 live 对象",
        ];
        let broken: Vec<String> = doc
            .jobs
            .iter()
            .filter(|job| !job.error.is_empty())
            .filter(|job| {
                !ENV_ALLOWED
                    .iter()
                    .any(|allowed| job.error.contains(allowed))
            })
            .filter(|job| {
                let stage = job.error.split('：').next().unwrap_or_default();
                !matches!(stage, "submit")
            })
            .map(|job| format!("{}@{}：{}", job.key.session, job.key.cursor, job.error))
            .collect();
        assert!(
            broken.is_empty(),
            "有任务带着非业务（或没有 stage）的错停着：{}",
            broken.join("；")
        );
        // **两个标记都要打出来**（2026-10-16 补）：只打标题的话，「模型标了取代」与
        // 「模型什么都没标」在这个夹具的输出里长得一模一样——而那正是 C6 要看的东西。
        for candidate in &memory.candidates {
            println!(
                "[probe] 候选：{} ← {}｜状态 {}｜标了冲突 {:?}｜取代 {:?}",
                candidate.title,
                candidate.source.session,
                candidate.state,
                candidate.conflicts_with,
                candidate.supersedes
            );
        }
        println!(
            "[probe] 有取代标记的候选 {} 条（这一行只作观察，不作断言：同一个夹具也跑             「没有取代」的场景，那种时候它本来就该是 0）",
            memory
                .candidates
                .iter()
                .filter(|candidate| !candidate.supersedes.is_empty())
                .count()
        );
    }

    /// **手动夹具：真模型验「意义冲突只标记不裁决」（C5）**。
    ///
    /// 上一条夹具答的是「提取链走不走得通」，这条答的是**冲突标记的语义质量**。两者不是
    /// 一件事：插件侧的机械判据（「模型标的 id 必须来自本次检索集合」）在桩模型下就钉死了，
    /// 但「模型看到一句与既有记忆**相反**的话，会不会真去标；看到一句只是**相关**的话，
    /// 会不会瞎标」只有真模型答得了——而瞎标比不标更坏（面板会把两条毫不相干的记忆并排
    /// 摆给用户，还配一句「由你决定留哪一条」）。
    ///
    /// ## 为什么必须**预置两条已确认记忆**
    ///
    /// 不能靠上一轮的候选：`query::sorted_hits` 只捞 `state == accepted`，**待确认候选不参与
    /// 检索**。于是「库里那条」必须在夹具里就放成 `accepted`，否则探针那一轮的
    /// `/context` 命中不了任何东西，插件按「没有校验集合」把所有冲突标记丢掉（这是有意的
    /// 纪律，见 `memory/conflicts.rs`），夹具就会因为「候选没标冲突」而红——但那是环境，
    /// 不是缺陷。这条坑第一次跑就会撞上，所以写在这里。
    ///
    /// 两条而不是一条，是为了让「该标的标 / 不该标的别标」各有一个靶子，而且三个靶子
    /// 在**同一批切片**里出现，模型没法靠「上下文里只有一条可疑的」蒙对：
    ///
    /// | 探针说的 | 对库里哪条 | 期望 |
    /// | --- | --- | --- |
    /// | 提交前不用跑门禁了 | 门禁那条（相反） | **标** |
    /// | 提交信息里带上文件名 | 门禁那条（相关但不冲突） | **不标** |
    /// | 这个仓库用 npm install | 包管理器那条（相反） | **标** |
    ///
    /// 跑法（探针三句用 `PROBE_TURNS` 传，见清单「怎么验」）：
    /// ```sh
    /// cd src-tauri && cargo test --lib manual_conflict_probe -- --ignored --nocapture
    /// # 另开一个终端：读握手 → 跑 drive-real-model.mjs → touch /tmp/p2probe/bridge-stop
    /// ```
    #[test]
    #[ignore = "手动夹具：真模型验冲突标记的语义质量（C5）"]
    fn manual_conflict_probe_with_a_real_model() {
        let root = temp_dir("conflict-probe");
        let _home = crate::tests::scoped_xlink_home(&root);
        let ns = crate::memory::store::Namespace::for_bridge("dsh", "probe");
        crate::memory::store::set_use_approved(&ns, true).expect("打开记忆开关");

        // 两条靶子都用**公开写入入口** `save`（`mutate` 是模块私有的），再显式置成
        // `accepted`——`save` 默认落 `verified` 状态，不是检索认的那个状态。
        let gate = crate::memory::store::save(
            &ns,
            crate::memory::store::SaveInput {
                scope: "general".to_string(),
                title: "提交前必须跑 npm run check".to_string(),
                body: "提交前一定先跑一遍 npm run check 这个门禁，不要跳。".to_string(),
                keywords: vec![
                    "提交".to_string(),
                    "门禁".to_string(),
                    "npm run check".to_string(),
                ],
                verification: "verified".to_string(),
                ..Default::default()
            },
        )
        .expect("预置门禁那条");
        let package_manager = crate::memory::store::save(
            &ns,
            crate::memory::store::SaveInput {
                scope: "general".to_string(),
                title: "个人项目一律用 pnpm".to_string(),
                body: "我所有的个人项目都用 pnpm，不要用 npm install。".to_string(),
                keywords: vec![
                    "pnpm".to_string(),
                    "包管理器".to_string(),
                    "npm install".to_string(),
                ],
                verification: "verified".to_string(),
                ..Default::default()
            },
        )
        .expect("预置包管理器那条");
        for id in [&gate.id, &package_manager.id] {
            crate::memory::store::set_state(&ns, id, crate::memory::store::STATE_ACCEPTED)
                .expect("置成已确认（检索只认这个状态）");
        }

        let handshake = PathBuf::from("/tmp/p2probe/bridge-cred.json");
        let stop = PathBuf::from("/tmp/p2probe/bridge-stop");
        let _ = fs::remove_file(&stop);
        let _ = fs::create_dir_all("/tmp/p2probe");
        let (url, token) = launch(&root, "dsh:probe").expect("起桥");
        fs::write(
            &handshake,
            format!("{{\"url\":\"{url}\",\"token\":\"{token}\"}}"),
        )
        .expect("写握手文件");
        println!("[probe] 桥已就绪：{url}；等 {stop:?}");
        println!(
            "[probe] 靶子：门禁那条 {} · 包管理器那条 {}",
            gate.id, package_manager.id
        );

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(900);
        while !stop.exists() {
            assert!(
                std::time::Instant::now() < deadline,
                "等了 15 分钟也没看到停止文件 {stop:?}——探针那一步没跑起来？"
            );
            std::thread::sleep(std::time::Duration::from_millis(200));
        }

        let memory = crate::memory::store::load(&ns).expect("读记忆库");
        println!("[probe] 候选 {} 条", memory.candidates.len());
        assert!(
            !memory.candidates.is_empty(),
            "候选一条都没落下来——提取链没走通（看插件日志里的 stage 与原因）"
        );
        for candidate in &memory.candidates {
            println!(
                "[probe] 候选：{} ← 标了冲突 {:?}",
                candidate.title, candidate.conflicts_with
            );
        }

        // 不变式：标出来的 id 必须都在库里。这条在桩模型下也有判据，但真模型会自由发挥，
        // 而插件侧那道校验是本轮要连着一起验的（它一旦失效，面板上会出现一条「找不到的记忆」）。
        let known: Vec<&str> = memory.memories.iter().map(|r| r.id.as_str()).collect();
        for candidate in &memory.candidates {
            for id in &candidate.conflicts_with {
                assert!(
                    known.contains(&id.as_str()),
                    "候选「{}」标的冲突 {} 不在库里——插件侧那道「id ∈ 本次检索集合」失效了",
                    candidate.title,
                    id
                );
            }
        }

        // 核心判据：两个「相反」的靶子各要挨一次标。
        // **红的含义要说清**：模型把「提交前不用跑门禁」当成一条普通新约定直接提了、
        // 一点冲突都不标——那说明它没真在比对检索到的既有记忆。此时 C5 只兑现了
        // 「插件会把模型标的收下来」，没兑现「标得出来」，机制要重新想（不是夹具坏了）。
        let hits_gate = memory
            .candidates
            .iter()
            .any(|c| c.conflicts_with.contains(&gate.id));
        let hits_package = memory
            .candidates
            .iter()
            .any(|c| c.conflicts_with.contains(&package_manager.id));
        assert!(
            hits_gate,
            "没有候选把「提交前不用跑门禁」标成与门禁那条冲突（{}）——模型没在比对既有记忆",
            gate.id
        );
        assert!(
            hits_package,
            "没有候选把「这个仓库用 npm install」标成与 pnpm 那条冲突（{}）",
            package_manager.id
        );

        // 「不该标的别标」这一半**机械判不出来**：它要求模型压根不提第 2 句那条无关约定，
        // 而提不提、怎么合并都是模型的自由。所以这里不写断言，只在它出现了的时候校验，
        // **没出现就明确打印「这半本轮没验到」**——写成一条会空过的断言等于给自己一道假绿
        // （判据空转时必须说破，这是本仓 `check-invariants` 反复强调的那件事）。
        let unrelated = memory
            .candidates
            .iter()
            .find(|c| c.title.contains("文件名") || c.body.contains("文件名"));
        match unrelated {
            Some(candidate) => assert!(
                candidate.conflicts_with.is_empty(),
                "「提交信息带上文件名」与库里任何一条都不冲突，模型却标了 {:?}",
                candidate.conflicts_with
            ),
            None => println!(
                "[probe] 注意：模型没单独立「提交信息带上文件名」这条，\
                 所以「不瞎标」这半本轮**没有**验到（不是通过了）"
            ),
        }
    }

    /// **手动量测：满库时一次 `/context` 要多久**（B17）。
    ///
    /// 设计 §3.1 那句「150 ms 降级线是建议值不是已验证指标」——这条夹具就是去把指标量出来。
    /// 量的是**整条路**：HTTP 往返 + 读整份文档 + 检索 + 渲染注入文本（1000 条，即库的上限）。
    /// **整条夹具约 80 秒**，其中大部分是填库（逐条 `save` 要重写整份文档，那是公开入口的代价）。
    ///
    /// ```sh
    /// cd src-tauri && cargo test --lib manual_context_latency -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "手动量测：填满 1000 条再量 /context 的延迟（B17）"]
    fn manual_context_latency_with_a_full_store() {
        use std::time::Instant;
        let root = temp_dir("latency");
        let _home = crate::tests::scoped_xlink_home(&root);
        let ns = crate::memory::store::Namespace::for_bridge("dsh", "probe");
        crate::memory::store::set_use_approved(&ns, true).expect("打开记忆开关");

        // 逐条 `save`（那是公开入口；`mutate` 是模块私有的）。每条都要重写整份文档，
        // 所以这一步本身就有几十秒——手动夹具，值得。
        for index in 0..1000 {
            crate::memory::store::save(
                &ns,
                crate::memory::store::SaveInput {
                    scope: "general".to_string(),
                    title: format!("第 {index} 条约定"),
                    body: format!(
                        "这是第 {index} 条约定：提交前跑 npm run check，改完 UI 跑 build:ui，\
                         内核目录只读。带一点中文与 English 混排，接近真实正文的长度。"
                    ),
                    keywords: vec!["提交".to_string(), "检查".to_string()],
                    verification: "verified".to_string(),
                    ..Default::default()
                },
            )
            .expect("填库");
        }
        let doc_bytes = std::fs::metadata(ns.file())
            .map(|meta| meta.len())
            .unwrap_or(0);
        println!("[latency] 库：1000 条、{doc_bytes} 字节");

        let (url, token) = launch(&root, "dsh:probe").expect("起桥");
        let addr = url.trim_start_matches("http://").to_string();

        let mut samples = Vec::new();
        for round in 0..20 {
            let query = format!("context?q=%E6%8F%90%E4%BA%A4%20check%20{round}&workspace_id=&workspace=%2Ftmp%2Fws");
            let started = Instant::now();
            let body = bridge_get(&addr, &token, &query);
            let elapsed = started.elapsed().as_micros() as f64 / 1000.0;
            assert!(
                body.contains("\"enabled\""),
                "这次没答对：{}",
                &body[..body.len().min(80)]
            );
            samples.push(elapsed);
        }
        samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let p50 = samples[samples.len() / 2];
        let p95 = samples[(samples.len() * 95) / 100];
        let max = samples[samples.len() - 1];
        println!("[latency] /context 整条路（HTTP + 读库 + 检索 + 渲染），20 次：");
        println!("[latency] p50 {p50:.1} ms / p95 {p95:.1} ms / max {max:.1} ms");
        // 松判据（这条是手动夹具，不要因为机器忙就红）：整条路远在插件那 1500 ms 超时之内。
        assert!(
            p95 < 500.0,
            "p95 {p95:.1} ms 超出预期——先看看是不是库真的满了"
        );
    }

    /// 换代之后上一代必须真的死掉（2026-10-15 评审第 2 条）。
    ///
    /// 这条判据只能走 `launch`：`Drop` 自己早被下面那条测试覆盖了，而漏掉的恰恰是
    /// 「注册表换代时会不会执行 `Drop`」——accept 线程 move 捕获整个 `Arc<Bridge>` 时
    /// 它永远不会执行，旧端口继续接受连接、旧令牌继续有效。
    #[test]
    fn a_second_generation_kills_the_first_one() {
        let home = temp_dir("generation");
        let (url1, token1) = launch(&home, "dsh:gen").expect("第一代");
        let addr1 = url1.trim_start_matches("http://").to_string();
        let (url2, _token2) = launch(&home, "dsh:gen").expect("第二代");
        let addr2 = url2.trim_start_matches("http://").to_string();
        assert_ne!(url1, url2, "两代不该共用端口");

        // 旧端口必须**不再接受连接**：`Drop` 关掉 listener 之后 connect 会被拒。
        // accept 线程退出是异步的（`Drop` 里那次自连把它唤醒），给它一点时间。
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        let mut refused = false;
        while std::time::Instant::now() < deadline {
            if TcpStream::connect(&addr1).is_err() {
                refused = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(refused, "上一代端口 {addr1} 还在接受连接——换代没执行 Drop");

        // 活着的那一代是第二代：旧令牌在它上面必须被拒。
        let body = bridge_get(&addr2, &token1, "context?workspace=/tmp");
        assert!(
            body.contains("unauthorized"),
            "旧令牌在新一代上没被拒：{}",
            &body[..body.len().min(80)]
        );
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
                shutdown: Arc::new(AtomicBool::new(false)),
                credentials: vec![path.clone()],
            };
            drop(bridge);
        }
        assert!(!path.exists(), "凭据文件没有随桥一起删掉");
    }
}
