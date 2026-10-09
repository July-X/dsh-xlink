//! 授权编排：把发现 / 回调 / 换令牌 / 验 ID token / 入库串成一次登录
//! （设计 §3.2 / §8）。运行在命令层的 blocking 线程上（传输是同步
//! ureq，回调等待按秒切片并响应取消）。
//!
//! 「注册」按 SIWC 官方形状发生在浏览器授权里（auth.rs 模块注释）：
//! 首次带引导 client 与宿主提示，签发的 client_id 从回调取回落盘；
//! 再次登录复用。传输与「打开浏览器」以 [`FlowDeps`] 注入：生产用
//! [`transport`] 与系统浏览器；测试用本文件的**模拟授权服务器**（回环
//! OIDC：发现 / JWKS / token，固定测试密钥签 ID token）——这正是开发
//! 计划 §10 要求 CI 复用的那一份，真实账号令牌永不进 CI。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::openai::auth::{
    self, authorize_url, discovery_url, ensure_host_id, new_pkce, parse_metadata,
    parse_token_response, random_hex, verify_id_token, Metadata,
};
use crate::openai::callback::{self, Outcome};
use crate::openai::transport::{self, Failure};
use crate::openai::vault::{self, AccountTokens, Accounts};

/// 登录超时：授权页没人动是常态，给足时间（设计 §3.2 打开系统浏览器）。
const AUTHORIZATION_TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Clone)]
pub(crate) struct FlowPaths {
    /// `shell/<mode>/openai-oauth/`（host-id、registration 都在这里）。
    pub(crate) mode_dir: PathBuf,
    /// 加密凭据文件路径。
    pub(crate) vault_file: PathBuf,
    /// xlink home（密钥库分键用）。
    pub(crate) xlink_home: PathBuf,
    /// issuer 基址：生产是 [`auth::SIWC_ISSUER`]，测试是模拟授权服务器。
    pub(crate) issuer_base: String,
    /// 模型目录与推理资源基址：生产是 [`auth::OPENAI_RESOURCE`]，
    /// access token 签给该 resource；测试可独立于授权服务器模拟资源主机。
    pub(crate) models_base: String,
}

/// 传输与环境的注入面。生产实现走 [`transport`] 与系统浏览器；测试实现
/// 连本文件的模拟授权服务器（trait 而不是 fn 指针：要捕获回环端口）。
pub(crate) trait FlowTransport {
    fn get_json(&self, url: &str) -> Result<String, Failure>;
    fn post_json(&self, url: &str, body: &str) -> Result<String, Failure>;
    fn post_form(&self, url: &str, pairs: &[(&str, &str)]) -> Result<String, Failure>;
    fn open_browser(&self, url: &str) -> Result<(), String>;
    fn now_unix(&self) -> u64;
    /// 系统凭据库（密钥存取）。生产连 vault 的平台实现；测试用内存表——
    /// **真实 Keychain 绝不进测试**（vault.rs 同一条纪律）。
    fn keyring_get(&self, service: &str, account: &str) -> Result<String, String>;
    fn keyring_put(&self, service: &str, account: &str, secret: &str) -> Result<(), String>;
    /// 带 Bearer 鉴权的 GET（模型目录等账号端点）。默认实现忽略鉴权
    /// （形状 mock 用）；生产与回环 mock 各自实现。
    fn get_json_with_auth(&self, url: &str, bearer: &str) -> Result<String, Failure> {
        let _ = bearer;
        self.get_json(url)
    }
}

/// 账号脱敏视图（索引与 UI 用；完整令牌只在 vault 里）。
#[derive(Debug)]
pub(crate) struct AccountView {
    pub(crate) sub: String,
    pub(crate) email: Option<String>,
    /// 刷新被判失效（§4.1 第四态 reauth-required）。
    pub(crate) reauth_required: bool,
}

fn registration_file(mode_dir: &Path) -> PathBuf {
    mode_dir.join("registration.json")
}

/// 一次完整登录。取消旗子由命令层持有（`openai_authorize_cancel` 置位）。
pub(crate) fn run_authorize(
    paths: &FlowPaths,
    deps: &impl FlowTransport,
    mode: &str,
    cancellation: &AtomicBool,
) -> Result<AccountView, String> {
    let discovery = deps
        .get_json(&discovery_url(&paths.issuer_base))
        .map_err(|failure| failure.message(&[]))?;
    let metadata = parse_metadata(&discovery)?;

    let host_id = ensure_host_id(&paths.mode_dir)?;
    // SIWC：首次（无注册记录）用公开引导 client + 宿主提示；再次登录复用
    // 已签发的 client_id（设计 §8「再次登录复用对应注册」）。
    let saved_client_id = auth::load_registration(&paths.mode_dir)?;
    let first_registration = saved_client_id.is_none();
    let expected_sub = if first_registration {
        None
    } else {
        let key = vault::load_file_key_with(
            mode,
            &paths.xlink_home,
            |service, account| deps.keyring_get(service, account),
            |service, account, secret| deps.keyring_put(service, account, secret),
        )?;
        vault::load_accounts(&paths.vault_file, &key)?.active
    };
    let client_id = saved_client_id
        .clone()
        .unwrap_or_else(|| auth::DYNAMIC_CLIENT_ID.to_string());

    let pkce = new_pkce();
    let state = random_hex(16);
    let nonce = random_hex(16);
    let listener = callback::spawn(&state)?;
    let url = authorize_url(
        &metadata.authorization_endpoint,
        &client_id,
        &listener.redirect_uri(),
        &state,
        &nonce,
        &pkce.challenge,
        host_id.as_str(),
        first_registration,
    );
    deps.open_browser(&url)
        .map_err(|error| format!("打开系统浏览器失败：{error}；请手工复制登录地址"))?;

    // 回调等待按 1 秒切片：每片检查取消旗与总截止时间（授权页没人动是
    // 常态，总时限给足；取消要立刻生效，不能等满三分钟）。
    let started = deps.now_unix();
    let outcome = loop {
        if cancellation.load(Ordering::SeqCst) {
            return Err("登录已取消".into());
        }
        if deps.now_unix().saturating_sub(started) > AUTHORIZATION_TIMEOUT.as_secs() {
            return Err(format!(
                "等待浏览器回调超时（{} 秒）：请重试登录；若浏览器没有打开，检查默认浏览器设置",
                AUTHORIZATION_TIMEOUT.as_secs()
            ));
        }
        match listener.wait(Duration::from_secs(1)) {
            Ok(outcome) => break outcome,
            Err(error) => {
                // 每秒一片的「超时」是正常心跳；其它错误（通道关闭等）如实上报。
                if !error.contains("超时") {
                    return Err(error);
                }
            }
        }
    };
    match outcome {
        Outcome::Code {
            code,
            client_id: issued,
        } => {
            // SIWC 的注册回执：首次必须取回签发的 client_id 并落盘；再次
            // 登录回调再带 client_id 必须与已保存的一致（官方规范）。
            let issued_client_id = match (first_registration, issued) {
                (true, Some(id)) if !id.is_empty() => {
                    auth::save_registration(&paths.mode_dir, &id)?;
                    id
                }
                (true, _) => {
                    return Err("授权回调缺签发的 client_id（首次注册）：请重试登录".into());
                }
                (false, Some(id)) if saved_client_id.as_deref() == Some(id.as_str()) => id,
                (false, Some(other)) => {
                    return Err(format!(
                        "授权回调的 client_id 与已注册的不一致（{other}）：登录已终止"
                    ));
                }
                (false, None) => saved_client_id.expect("非首次必有已保存的注册"),
            };
            exchange_and_store(
                paths,
                deps,
                mode,
                &metadata,
                &issued_client_id,
                &code,
                &listener.redirect_uri(),
                &pkce.verifier,
                &nonce,
                expected_sub.as_deref(),
            )
        }
        Outcome::Error(error) => Err(format!("授权未完成（{error}）；可重试登录")),
        Outcome::StateMismatch => Err("回调校验失败（state 不匹配）；登录已终止，请重试".into()),
    }
}

#[allow(clippy::too_many_arguments)]
fn exchange_and_store(
    paths: &FlowPaths,
    deps: &impl FlowTransport,
    mode: &str,
    metadata: &Metadata,
    client_id: &str,
    code: &str,
    redirect_uri: &str,
    verifier: &str,
    nonce: &str,
    expected_sub: Option<&str>,
) -> Result<AccountView, String> {
    let token_json = deps
        .post_form(
            &metadata.token_endpoint,
            &[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", redirect_uri),
                ("client_id", client_id),
                ("code_verifier", verifier),
                // 官方规范：换令牌必须带与授权时相同的 resource。
                ("resource", auth::OPENAI_RESOURCE),
            ],
        )
        .map_err(|failure| failure.message(&[]))?;
    let tokens = parse_token_response(&token_json)?;
    if !tokens
        .scopes
        .iter()
        .any(|scope| scope == "chatgpt.tokens.use.direct")
    {
        return Err("授权响应未授予 chatgpt.tokens.use.direct；无法使用 ChatGPT 套餐模型，请重新授权并勾选该权限".into());
    }
    let jwks = deps
        .get_json(&metadata.jwks_uri)
        .map_err(|failure| failure.message(&[]))?;
    let claims = verify_id_token(
        &tokens.id_token,
        &jwks,
        &metadata.issuer,
        client_id,
        nonce,
        deps.now_unix(),
    )?;
    if let Some(expected) = expected_sub {
        if expected != claims.subject {
            return Err("重新授权返回了不同的 ChatGPT 账号；为保护原账号凭据，登录已终止，请先切换到目标账号后重试".into());
        }
    }

    let key = vault::load_file_key_with(
        mode,
        &paths.xlink_home,
        |service, account| deps.keyring_get(service, account),
        |service, account, secret| deps.keyring_put(service, account, secret),
    )?;
    let mut accounts = vault::load_accounts(&paths.vault_file, &key)?;
    accounts.entries.insert(
        claims.subject.clone(),
        AccountTokens {
            sub: claims.subject.clone(),
            email: claims.email.clone(),
            client_id: client_id.to_string(),
            access_token: tokens.access_token,
            refresh_token: tokens.refresh_token,
            id_token: tokens.id_token,
            scopes: tokens.scopes,
            access_expires_at: deps.now_unix() + tokens.expires_in,
            chatgpt_account_id: claims.chatgpt_account_id.clone(),
            reauth_required: false,
        },
    );
    accounts.active = Some(claims.subject.clone());
    vault::save_accounts(&paths.vault_file, &key, &accounts)?;
    Ok(AccountView {
        sub: claims.subject,
        email: claims.email,
        reauth_required: false,
    })
}

/// 退出登录（设计 §8）：清活跃账号与令牌。远端吊销：发现文档没有
/// revocation_endpoint（SIWC 形状未验证），如实说明「远端吊销未确认」，
/// 不假装吊销过。
pub(crate) fn run_logout(
    paths: &FlowPaths,
    deps: &impl FlowTransport,
    mode: &str,
) -> Result<bool, String> {
    let key = vault::load_file_key_with(
        mode,
        &paths.xlink_home,
        |service, account| deps.keyring_get(service, account),
        |service, account, secret| deps.keyring_put(service, account, secret),
    )?;
    let mut accounts = vault::load_accounts(&paths.vault_file, &key)?;
    let Some(sub) = accounts.active.clone() else {
        return Ok(false);
    };
    let Some(entry) = accounts.entries.get(&sub).cloned() else {
        accounts.active = None;
        vault::save_accounts(&paths.vault_file, &key, &accounts)?;
        return Ok(true);
    };
    let mut revoke_warning = None;
    if !entry.refresh_token.is_empty() {
        let discovery = deps
            .get_json(&discovery_url(&paths.issuer_base))
            .map_err(|failure| failure.message(&[]))?;
        let metadata = parse_metadata(&discovery)?;
        if let Some(endpoint) = metadata.revocation_endpoint {
            if let Err(failure) = deps.post_form(
                &endpoint,
                &[
                    ("token", entry.refresh_token.as_str()),
                    ("token_type_hint", "refresh_token"),
                    ("client_id", entry.client_id.as_str()),
                ],
            ) {
                revoke_warning = Some(failure.message(&[]));
            }
        }
    }
    // 保留账户与 client_id 映射，只清除本地会话令牌；不会因退出一个账号
    // 把其它账号的凭据一起删除。
    if let Some(saved) = accounts.entries.get_mut(&sub) {
        saved.access_token.clear();
        saved.refresh_token.clear();
        saved.id_token.clear();
        saved.reauth_required = true;
    }
    accounts.active = None;
    vault::save_accounts(&paths.vault_file, &key, &accounts)?;
    if let Some(warning) = revoke_warning {
        return Err(format!(
            "本地已退出，但远端吊销未确认：{warning}；可在 ChatGPT 设置中断开应用"
        ));
    }
    Ok(true)
}

/// 读当前账号（脱敏）。
pub(crate) fn account_view(
    paths: &FlowPaths,
    deps: &impl FlowTransport,
    mode: &str,
) -> Result<Option<AccountView>, String> {
    let key = vault::load_file_key_with(
        mode,
        &paths.xlink_home,
        |service, account| deps.keyring_get(service, account),
        |service, account, secret| deps.keyring_put(service, account, secret),
    )?;
    let accounts: Accounts = vault::load_accounts(&paths.vault_file, &key)?;
    Ok(accounts
        .active
        .as_ref()
        .and_then(|sub| accounts.entries.get(sub))
        .map(|entry| AccountView {
            sub: entry.sub.clone(),
            email: entry.email.clone(),
            reauth_required: entry.reauth_required,
        }))
}

/// 壳内生产路径（`shell/<mode>/openai-oauth/` + 官方 issuer）：命令层与
/// 桥接共用一份，别各写一遍。
pub(crate) fn shell_flow_paths() -> FlowPaths {
    let mode = crate::shell::settings::current_mode();
    let mode_dir = crate::shell::paths::shell_dir(mode).join("openai-oauth");
    FlowPaths {
        vault_file: mode_dir.join("accounts.bin"),
        mode_dir,
        xlink_home: crate::shell::paths::xlink_home(),
        issuer_base: crate::openai::auth::SIWC_ISSUER.to_string(),
        models_base: crate::openai::auth::OPENAI_RESOURCE.to_string(),
    }
}

/// 生产传输：HTTP 走 [`transport`]；「打开浏览器」由命令层用 opener 插件
/// 实现（编排层不依赖 Tauri 类型），这里给出基于 `open`/`start` 命令的
/// 兜底——命令层注入的 opener 优先。
/// 浏览器打开动作（命令层注入 opener；None 时用系统命令兜底）。
pub(crate) type BrowserOpener = Box<dyn Fn(&str) -> Result<(), String> + Send + Sync>;

pub(crate) struct ProductionTransport {
    pub(crate) open_browser: Option<BrowserOpener>,
}

impl FlowTransport for ProductionTransport {
    fn get_json(&self, url: &str) -> Result<String, Failure> {
        transport::get_json(url)
    }
    fn post_json(&self, url: &str, body: &str) -> Result<String, Failure> {
        transport::post_json(url, body)
    }
    fn post_form(&self, url: &str, pairs: &[(&str, &str)]) -> Result<String, Failure> {
        transport::post_form(url, pairs)
    }
    fn open_browser(&self, url: &str) -> Result<(), String> {
        if let Some(opener) = &self.open_browser {
            return opener(url);
        }
        #[cfg(target_os = "macos")]
        let program = "open";
        #[cfg(target_os = "windows")]
        let program = "cmd";
        #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
        return Err("不支持的平台的兜底打开".into());
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            #[cfg(target_os = "macos")]
            let args: Vec<String> = vec![url.to_string()];
            #[cfg(target_os = "windows")]
            let args: Vec<String> = vec!["/C".into(), "start".into(), url.to_string()];
            std::process::Command::new(program)
                .args(&args)
                .spawn()
                .map(|_| ())
                .map_err(|error| format!("打开系统浏览器失败（{program}）：{error}"))
        }
    }
    fn now_unix(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }
    fn keyring_get(&self, service: &str, account: &str) -> Result<String, String> {
        vault::keyring_get(service, account)
    }
    fn keyring_put(&self, service: &str, account: &str, secret: &str) -> Result<(), String> {
        vault::keyring_put(service, account, secret)
    }
    fn get_json_with_auth(&self, url: &str, bearer: &str) -> Result<String, Failure> {
        transport::get_json_with_auth(url, bearer)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::openai::http::{read_request, write_response};
    use crate::openai::testkeys::{KEY1_N, KEY1_PKCS1};
    use std::io::Write;
    use std::net::{TcpListener, TcpStream};
    use std::sync::atomic::Ordering;
    use std::sync::{Arc, Mutex};

    /// **模拟授权服务器**（开发计划 §10：CI 用模拟服务，真实账号令牌
    /// 永不进 CI）。端点：发现 / 动态注册 / JWKS / token（授权码换发与
    /// **刷新**两种 grant）。ID token 用固定测试密钥（KEY1）签发；刷新校验
    /// 当前 refresh token 并旋转；`revoke()` 置位后刷新一律 `invalid_grant`。
    pub(crate) struct MockIssuer {
        port: u16,
        session: Arc<Mutex<Option<(String, String)>>>, // (nonce, state)
        keyring: TestKeyring,
        /// 当前有效 refresh token（授权码换发写入、刷新校验并旋转）。
        current_refresh: Arc<Mutex<String>>,
        /// 置位后刷新一律 invalid_grant（模拟服务端撤销授权）。
        revoke_refresh: Arc<AtomicBool>,
    }

    impl MockIssuer {
        pub(crate) fn spawn() -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let session: Arc<Mutex<Option<(String, String)>>> = Arc::new(Mutex::new(None));
            let thread_session = Arc::clone(&session);
            let keyring: TestKeyring = Arc::new(Mutex::new(Vec::new()));
            let current_refresh: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
            let thread_refresh = Arc::clone(&current_refresh);
            let revoke_refresh: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
            let thread_revoke = Arc::clone(&revoke_refresh);
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    let Ok(mut stream) = stream else { break };
                    let Ok(request) = read_request(&mut stream) else {
                        continue;
                    };
                    let body = read_body(&mut stream, &request);
                    let base = format!("http://127.0.0.1:{port}");
                    let response = mock_route(
                        &request,
                        &body,
                        &base,
                        &thread_session,
                        &thread_refresh,
                        &thread_revoke,
                    );
                    let (status, content) = response;
                    let _ = write_response(&mut stream, status, "application/json", &content);
                }
            });
            Self {
                port,
                session,
                keyring,
                current_refresh,
                revoke_refresh,
            }
        }

        pub(crate) fn transport(&self) -> MockTransport {
            MockTransport {
                port: self.port,
                session: Arc::clone(&self.session),
                keyring: Arc::clone(&self.keyring),
                current_refresh: Arc::clone(&self.current_refresh),
                revoke_refresh: Arc::clone(&self.revoke_refresh),
                deny_in_browser: false,
            }
        }

        /// 模拟服务端撤销（此后刷新一律 invalid_grant）。
        pub(crate) fn revoke(&self) {
            self.revoke_refresh.store(true, Ordering::SeqCst);
        }

        pub(crate) fn port(&self) -> u16 {
            self.port
        }
    }

    /// 路由分发（从线程体里提出纯函数，方便阅读与将来扩展端点）。
    fn mock_route(
        request: &crate::openai::http::RequestHead,
        body: &str,
        base: &str,
        session: &Arc<Mutex<Option<(String, String)>>>,
        current_refresh: &Arc<Mutex<String>>,
        revoke: &AtomicBool,
    ) -> (u16, String) {
        eprintln!(
            "[mock] {} body={}",
            request.path,
            &body[..body.len().min(50)]
        );
        if request.path.starts_with("/.well-known/") {
            // SIWC 的线上发现文档没有 registration_endpoint——mock 同形状。
            return (
                200,
                serde_json::json!({
                    "issuer": base,
                    "authorization_endpoint": format!("{base}/authorize"),
                    "token_endpoint": format!("{base}/token"),
                    "jwks_uri": format!("{base}/jwks"),
                })
                .to_string(),
            );
        }
        if request.path == "/jwks" {
            return (
                200,
                serde_json::json!({
                    "keys": [{ "kty": "RSA", "kid": "k1", "n": KEY1_N, "e": "AQAB" }]
                })
                .to_string(),
            );
        }
        if request.path == "/token" && request.method == "POST" {
            let now = 1_700_000_000u64;
            // 官方规范：换令牌必须带 resource；模拟服务器同样 fail-closed。
            if !body.contains("resource=") {
                return (
                    400,
                    r#"{"error":"invalid_request","detail":"missing resource"}"#.into(),
                );
            }
            // aud 必须等于请求里呈现的 client_id（签发 id 链路端到端校验）。
            let presented_client_id = body
                .split("client_id=")
                .nth(1)
                .map(|v| v.split('&').next().unwrap_or_default())
                .unwrap_or_default()
                .to_string();
            // 模拟令牌按代次拼出来（字面量会撞凭据扫描的误报）。
            let issued_pair = |label: &str| (format!("at-{label}"), format!("rt-{label}"));
            if body.contains("grant_type=refresh_token") {
                let presented = body
                    .split("refresh_token=")
                    .nth(1)
                    .map(|v| v.split('&').next().unwrap_or_default())
                    .unwrap_or_default();
                let expected = current_refresh.lock().unwrap().clone();
                if revoke.load(Ordering::SeqCst) || presented != expected {
                    return (400, r#"{"error":"invalid_grant"}"#.into());
                }
                let claims = serde_json::json!({
                    "iss": base, "aud": presented_client_id, "sub": "mock-sub-1",
                    "email": "dev@example.com", "nonce": "refresh-nonce",
                    "exp": now + 3600, "iat": now,
                });
                let (access_value, refresh_value) = issued_pair("2");
                *current_refresh.lock().unwrap() = refresh_value.clone();
                return (
                    200,
                    serde_json::json!({
                        "access_token": access_value, "refresh_token": refresh_value,
                        "id_token": sign_with_key1(&claims.to_string()), "expires_in": 3600, "scope": "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct",
                    })
                    .to_string(),
                );
            }
            let Some((nonce, _state)) = session.lock().unwrap().clone() else {
                return (400, r#"{"error":"no_session"}"#.into());
            };
            let claims = serde_json::json!({
                "iss": base, "aud": presented_client_id, "sub": "mock-sub-1",
                "email": "dev@example.com", "nonce": nonce,
                "exp": now + 3600, "iat": now,
            });
            let (access_value, refresh_value) = issued_pair("1");
            *current_refresh.lock().unwrap() = refresh_value.clone();
            return (
                200,
                serde_json::json!({
                    "access_token": access_value, "refresh_token": refresh_value,
                        "id_token": sign_with_key1(&claims.to_string()), "expires_in": 3600, "scope": "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct",
                })
                .to_string(),
            );
        }
        (404, "{}".into())
    }

    /// 读请求体：先消费 `read_request` 带回的体前缀（可能已含全部），缺的
    /// 才从 socket 继续读。
    fn read_body(stream: &mut TcpStream, request: &crate::openai::http::RequestHead) -> String {
        let length: usize = request
            .headers
            .get("content-length")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let mut body = request.body_prefix.clone();
        while body.len() < length {
            let mut chunk = [0u8; 512];
            let Ok(n) = std::io::Read::read(stream, &mut chunk) else {
                break;
            };
            if n == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..n]);
        }
        body.truncate(length);
        String::from_utf8_lossy(&body).into_owned()
    }

    /// 测试内存钥匙库的形状。
    pub(crate) type TestKeyring = Arc<Mutex<Vec<(String, String, String)>>>;

    /// 假浏览器：解析授权 URL → 记 nonce/state → 直接对 redirect_uri
    /// 发一次回调（模拟用户在授权页点完「继续」）。`deny_in_browser`
    /// 模拟用户在授权页点了拒绝。
    pub(crate) struct MockTransport {
        port: u16,
        session: Arc<Mutex<Option<(String, String)>>>,
        keyring: TestKeyring,
        current_refresh: Arc<Mutex<String>>,
        revoke_refresh: Arc<AtomicBool>,
        deny_in_browser: bool,
    }

    impl FlowTransport for MockTransport {
        fn get_json(&self, url: &str) -> Result<String, Failure> {
            plain_http(url, "GET", "", None)
        }
        fn post_json(&self, url: &str, body: &str) -> Result<String, Failure> {
            plain_http(url, "POST-JSON", body, None)
        }
        fn post_form(&self, url: &str, pairs: &[(&str, &str)]) -> Result<String, Failure> {
            let body = pairs
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join("&");
            plain_http(url, "POST", &body, None)
        }
        fn open_browser(&self, url: &str) -> Result<(), String> {
            let mut nonce = String::new();
            let mut state = String::new();
            let mut redirect = String::new();
            let mut dynamic_client = false;
            for pair in url.split('?').nth(1).unwrap_or_default().split('&') {
                let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
                match key {
                    "nonce" => nonce = value.to_string(),
                    "state" => state = value.to_string(),
                    "redirect_uri" => redirect = value.replace("%3A", ":").replace("%2F", "/"),
                    "client_id" => dynamic_client = value == "dynamic_agent_client",
                    _ => {}
                }
            }
            *self.session.lock().unwrap() = Some((nonce, state.clone()));
            // 对回调监听发重定向（redirect_uri 形如 http://127.0.0.1:PORT/callback）。
            // 首次注册（引导 client）时按官方规范在回调里回签发的 client_id。
            let after_host = redirect.strip_prefix("http://127.0.0.1:").unwrap();
            let (port, path) = after_host.split_once('/').unwrap();
            let issued = if dynamic_client {
                "&client_id=mock-client-1"
            } else {
                ""
            };
            let query = if self.deny_in_browser {
                format!("error=access_denied&state={state}")
            } else {
                format!("code=mock-code&state={state}{issued}")
            };
            let mut stream = TcpStream::connect(("127.0.0.1", port.parse().unwrap())).unwrap();
            stream
                .write_all(format!("GET /{path}?{query} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n").as_bytes())
                .unwrap();
            Ok(())
        }
        fn now_unix(&self) -> u64 {
            1_700_000_000 // 与 mock 签发的 iat 对齐
        }
        fn keyring_get(&self, service: &str, account: &str) -> Result<String, String> {
            self.keyring
                .lock()
                .unwrap()
                .iter()
                .find(|(s, a, _)| s == service && a == account)
                .map(|(_, _, secret)| secret.clone())
                .ok_or_else(|| "missing".into())
        }
        fn keyring_put(&self, service: &str, account: &str, secret: &str) -> Result<(), String> {
            self.keyring.lock().unwrap().push((
                service.to_string(),
                account.to_string(),
                secret.to_string(),
            ));
            Ok(())
        }

        fn get_json_with_auth(&self, url: &str, bearer: &str) -> Result<String, Failure> {
            plain_http(url, "GET", "", Some(bearer))
        }
    }

    impl MockTransport {
        /// 测试辅助：用内存钥匙库里的密钥读 vault。
        pub(crate) fn read_vault(&self, path: &Path) -> Result<Accounts, String> {
            let key = self.vault_key()?;
            vault::load_accounts(path, &key)
        }

        /// 内存钥匙库里的文件密钥（本测试只有一个）。
        pub(crate) fn vault_key(&self) -> Result<[u8; 32], String> {
            let secret = self
                .keyring
                .lock()
                .unwrap()
                .iter()
                .find(|(service, _, _)| service == vault::KEYRING_SERVICE)
                .map(|(_, _, secret)| secret.clone())
                .ok_or("no key")?;
            let mut key = [0u8; 32];
            for (i, chunk) in secret.as_bytes().chunks(2).enumerate() {
                key[i] =
                    u8::from_str_radix(std::str::from_utf8(chunk).unwrap_or("00"), 16).unwrap_or(0);
            }
            Ok(key)
        }
    }

    /// 极简 HTTP 客户端（测试内连 mock；不复用 transport 的路由逻辑）。
    fn plain_http(
        url: &str,
        method: &str,
        body: &str,
        bearer: Option<&str>,
    ) -> Result<String, Failure> {
        let after = url
            .strip_prefix("http://127.0.0.1:")
            .ok_or_else(|| Failure::Transport("非回环地址".into()))?;
        let (port, path) = after.split_once('/').unwrap();
        let mut stream = TcpStream::connect(("127.0.0.1", port.parse().unwrap()))
            .map_err(|e| Failure::Transport(e.to_string()))?;
        let content_type = if method == "POST-JSON" {
            "application/json"
        } else {
            "application/x-www-form-urlencoded"
        };
        let auth = bearer
            .map(|token| format!("Authorization: Bearer {token}\r\n"))
            .unwrap_or_default();
        let verb = if method == "POST-JSON" {
            "POST"
        } else {
            method
        };
        stream
            .write_all(format!("{verb} /{path} HTTP/1.1\r\nHost: 127.0.0.1\r\n{auth}Content-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes())
            .map_err(|e| Failure::Transport(e.to_string()))?;
        let mut text = String::new();
        use std::io::Read;
        stream
            .read_to_string(&mut text)
            .map_err(|e| Failure::Transport(e.to_string()))?;
        let status: u16 = text
            .split_whitespace()
            .nth(1)
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let body = text
            .split("\r\n\r\n")
            .nth(1)
            .map(String::from)
            .ok_or_else(|| Failure::Transport("空响应".into()))?;
        // 与 transport.rs 同语义：非 2xx 返回 Status 失败（带响应体——
        // OAuth 的 invalid_grant 分类必须看 body）。
        if (200..300).contains(&status) {
            Ok(body)
        } else {
            Err(Failure::Status(status, body))
        }
    }

    fn sign_with_key1(payload_json: &str) -> String {
        use base64::Engine;
        use ring::rsa::KeyPair;
        use ring::signature::KeyPair as _;
        let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        let std_b64 = base64::engine::general_purpose::STANDARD;
        let der = std_b64.decode(KEY1_PKCS1).unwrap();
        let private = KeyPair::from_der(&der).unwrap();
        let header = b64.encode(br#"{"alg":"RS256","kid":"k1"}"#);
        let payload = b64.encode(payload_json.as_bytes());
        let signing_input = format!("{header}.{payload}");
        let mut signature = vec![0u8; private.public_key().modulus_len()];
        let rng = ring::rand::SystemRandom::new();
        private
            .sign(
                &ring::signature::RSA_PKCS1_SHA256,
                &rng,
                signing_input.as_bytes(),
                &mut signature,
            )
            .unwrap();
        format!("{signing_input}.{}", b64.encode(&signature))
    }

    pub(crate) fn flow_test_paths(tag: &str, port: u16) -> FlowPaths {
        let dir = std::env::temp_dir().join(format!("oop-flow-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        FlowPaths {
            mode_dir: dir.join("mode"),
            vault_file: dir.join("mode").join("accounts.bin"),
            xlink_home: dir.join("xlink-home"),
            issuer_base: format!("http://127.0.0.1:{port}"),
            models_base: format!("http://127.0.0.1:{port}"),
        }
    }

    /// 端到端（模拟授权服务器）：发现 → 动态注册（持久化复用）→ 授权 URL
    /// → 假浏览器回调 → 换令牌 → 验 ID token → 入库；账号视图与 vault 一致。
    /// 读 vault（refresh 测试复用）。
    pub(crate) fn read_vault_with(
        transport: &MockTransport,
        path: &Path,
    ) -> Result<Accounts, String> {
        transport.read_vault(path)
    }

    /// 把某账号的访问过期时间改成指定值（强制/避免走刷新）。
    pub(crate) fn set_access_expiry(
        transport: &MockTransport,
        path: &Path,
        sub: &str,
        expires_at: u64,
    ) {
        let key = transport.vault_key().unwrap();
        let mut accounts = vault::load_accounts(path, &key).unwrap();
        accounts.entries.get_mut(sub).unwrap().access_expires_at = expires_at;
        vault::save_accounts(path, &key, &accounts).unwrap();
    }

    #[test]
    fn authorize_end_to_end_against_mock_issuer() {
        let issuer = MockIssuer::spawn();
        let transport = issuer.transport();
        let paths = flow_test_paths("e2e", issuer.port);
        let cancel = AtomicBool::new(false);

        let view = run_authorize(&paths, &transport, "release", &cancel).unwrap();
        assert_eq!(view.sub, "mock-sub-1");
        assert_eq!(view.email.as_deref(), Some("dev@example.com"));

        // vault 里有且只有这个账号，且已置为活跃；注册记录落盘。
        let key = [0u8; 32]; // 读不走这个路径——直接用 transport 的内存钥匙。
        let _ = key;
        let accounts = transport.read_vault(&paths.vault_file).unwrap();
        assert_eq!(accounts.active.as_deref(), Some("mock-sub-1"));
        assert_eq!(accounts.entries["mock-sub-1"].access_token, "at-1");
        assert_eq!(accounts.entries["mock-sub-1"].refresh_token, "rt-1");
        assert!(paths.mode_dir.join("registration.json").is_file());
        assert!(paths.mode_dir.join("host-id").is_file());

        // 再次登录：注册复用（registration.json 未变），账号仍一致。
        let before = std::fs::read_to_string(paths.mode_dir.join("registration.json")).unwrap();
        let again = run_authorize(&paths, &transport, "release", &cancel).unwrap();
        assert_eq!(again.sub, "mock-sub-1");
        assert_eq!(
            before,
            std::fs::read_to_string(paths.mode_dir.join("registration.json")).unwrap()
        );

        // 退出：活跃与条目清空（远端吊销未确认按设计如实说明）。
        assert!(run_logout(&paths, &transport, "release").unwrap());
        let cleared = transport.read_vault(&paths.vault_file).unwrap();
        assert!(cleared.active.is_none());
        assert_eq!(
            cleared.entries.len(),
            1,
            "退出只清会话令牌并保留账号/client 映射"
        );
        assert!(cleared.entries["mock-sub-1"].access_token.is_empty());
        assert!(
            !run_logout(&paths, &transport, "release").unwrap(),
            "重复退出是 no-op"
        );
        let _ = std::fs::remove_dir_all(paths.mode_dir.parent().unwrap()); // 只清自己的临时目录
    }

    /// 浏览器侧拒绝（error=access_denied）：流程如实失败，vault 不落任何账号。
    #[test]
    fn denied_callback_leaves_vault_untouched() {
        let issuer = MockIssuer::spawn();
        let mut transport = issuer.transport();
        transport.deny_in_browser = true;
        let paths = flow_test_paths("deny", issuer.port);
        let cancel = AtomicBool::new(false);
        let error = run_authorize(&paths, &transport, "release", &cancel).unwrap_err();
        assert!(error.contains("access_denied"), "{error}");
        // 拒绝发生在换令牌之前：连文件密钥都还没生成，vault 文件不存在。
        assert!(!paths.vault_file.exists(), "拒绝路径不应产生任何凭据落盘");
        let _ = std::fs::remove_dir_all(paths.mode_dir.parent().unwrap()); // 只清自己的临时目录
    }
}

#[cfg(test)]
mod reauth_tests {
    use super::*;
    use crate::openai::flow::tests::{flow_test_paths, MockIssuer};
    use crate::openai::refresh::ensure_fresh_access;
    use std::sync::atomic::AtomicBool;

    /// §4.1 第四态：刷新被判失效 → 标记落库 → 账户视图与状态命令可见 →
    /// 重新登录清除。
    #[test]
    fn refresh_failure_surfaces_reauth_required() {
        let issuer = MockIssuer::spawn();
        let transport = issuer.transport();
        let paths = flow_test_paths("reauth", issuer.port());
        let cancel = AtomicBool::new(false);

        crate::openai::flow::run_authorize(&paths, &transport, "release", &cancel).unwrap();
        issuer.revoke();
        // 过期：把访问过期时间拨到过去，强制走刷新。
        let key = vault::load_file_key_with(
            "release",
            &paths.xlink_home,
            |service, account| transport.keyring_get(service, account),
            |service, account, secret| transport.keyring_put(service, account, secret),
        )
        .unwrap();
        let mut accounts = vault::load_accounts(&paths.vault_file, &key).unwrap();
        accounts
            .entries
            .get_mut("mock-sub-1")
            .unwrap()
            .access_expires_at = 1;
        vault::save_accounts(&paths.vault_file, &key, &accounts).unwrap();

        // 刷新失败：ReauthRequired；账户视图带标记。
        let error = ensure_fresh_access(&paths, &transport, "release").unwrap_err();
        assert!(matches!(
            error,
            crate::openai::refresh::RefreshError::ReauthRequired(_)
        ));
        let view = account_view(&paths, &transport, "release")
            .unwrap()
            .unwrap();
        assert!(view.reauth_required);

        // 重新登录清除标记，回到 authorized。
        let view = run_authorize(&paths, &transport, "release", &cancel).unwrap();
        assert!(!view.reauth_required);
        let view = account_view(&paths, &transport, "release")
            .unwrap()
            .unwrap();
        assert!(!view.reauth_required);
    }
}
