//! OAuth/OIDC 授权核心（设计 §8；开发计划 §5「账户状态 / 授权动作」）。
//!
//! **协议形状按 OpenAI SIWC 官方规范实现**（developers.openai.com/siwc/
//! token-sharing-open-source/sign-in，2026-10-09 实机联调后对齐）：OpenAI
//! 不提供 RFC 7591 动态注册端点（线上发现文档没有 `registration_endpoint`），
//! 「注册」发生在**浏览器授权**里——首次用公开引导 client
//! [`DYNAMIC_CLIENT_ID`] + `agent_name_hint` + `ext_agent_host_id` 发起
//! 授权，签发的正式 client_id（`oaiapp_…`）从**回调 query** 里取回并
//! 持久保存；再次登录复用签发的 client_id，回调再带 client_id 必须与
//! 已保存的一致。其余沿用 OIDC 标准：发现文档、RFC 7636 PKCE（S256）、
//! `state`/`nonce` 一次性高熵值、RS256 ID token 验签（[`crate::openai::
//! jwk`]）。
//!
//! 传输（发现/换令牌的 HTTP）不在本文件：出网必须走
//! `net_proxy::routes()`（src-tauri/AGENTS.md），以参数注入。

use std::path::{Path, PathBuf};

use rand::Rng;
use sha2::Digest;

use crate::openai::jwk::{find_rsa_jwk, verify_rs256, RsaJwk};

/// SIWC 的 issuer 基址（2026-10-09 与线上发现文档的 `issuer` 逐字节核对）。
pub(crate) const SIWC_ISSUER: &str = "https://auth.openai.com";

/// 首次注册用的公开引导 client（官方规范；授权完成后作废，换签发 id）。
pub(crate) const DYNAMIC_CLIENT_ID: &str = "dynamic_agent_client";

/// 宿主应用名（`agent_name_hint`）：授权页展示给用户的应用身份。
pub(crate) const APP_NAME_HINT: &str = "dsh-xlink";

/// 套餐推理资源（授权与换令牌必须带同一个 `resource`）。
pub(crate) const OPENAI_RESOURCE: &str = "https://api.openai.com/v1";

/// 授权 scope（官方规范的 token-sharing 集合）。
pub(crate) const AUTH_SCOPE: &str =
    "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";

/// ID token 里承载 ChatGPT 账号 id 的声明（RFC 命名空间形式）。
///
/// 它**不在 access token 上**——访问令牌的受众是 [`OPENAI_RESOURCE`]，
/// 不含账号上下文；账号 id 只随 ID token 下发。套餐用量端点要拿它当
/// `ChatGPT-Account-Id` 请求头（2026-10-09 真机验证：`chatgptAuthTokens`
/// 登录要的 `chatgptAccountId` + `chatgptPlanType` 就是这两条声明）。
pub(crate) const CHATGPT_ACCOUNT_ID_CLAIM: &str = "https://api.openai.com/auth.chatgpt_account_id";
/// ID token 里承载账号套餐档位的声明（如 `plus` / `pro`）。
pub(crate) const CHATGPT_PLAN_TYPE_CLAIM: &str = "https://api.openai.com/auth.chatgpt_plan_type";

/// 发现文档：`<issuer>/.well-known/openid-configuration`。
pub(crate) fn discovery_url(issuer: &str) -> String {
    format!("{issuer}/.well-known/openid-configuration")
}

/// 解析发现文档；缺任一必需端点即错误（fail-closed，不猜 URL）。
/// 注意 SIWC 没有 `registration_endpoint`——注册不走发现文档。
pub(crate) fn parse_metadata(json: &str) -> Result<Metadata, String> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|error| format!("发现文档不是有效 JSON：{error}"))?;
    let string_of = |field: &str| -> Result<String, String> {
        value
            .get(field)
            .and_then(|v| v.as_str())
            .map(String::from)
            .ok_or_else(|| format!("发现文档缺 {field}"))
    };
    Ok(Metadata {
        issuer: string_of("issuer")?,
        authorization_endpoint: string_of("authorization_endpoint")?,
        token_endpoint: string_of("token_endpoint")?,
        jwks_uri: string_of("jwks_uri")?,
        revocation_endpoint: value
            .get("revocation_endpoint")
            .and_then(|v| v.as_str())
            .map(String::from),
    })
}

#[derive(Debug, PartialEq)]
pub(crate) struct Metadata {
    pub(crate) issuer: String,
    pub(crate) authorization_endpoint: String,
    pub(crate) token_endpoint: String,
    pub(crate) jwks_uri: String,
    pub(crate) revocation_endpoint: Option<String>,
}

/// RFC 7636 PKCE：verifier 至少 43 字符（此处 64），challenge 为其 SHA-256
/// 的 base64url（无填充）。字符集是 RFC 的 unreserved 集。
pub(crate) struct Pkce {
    pub(crate) verifier: String,
    pub(crate) challenge: String,
}

const PKCE_ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";

pub(crate) fn new_pkce() -> Pkce {
    let mut rng = rand::rng();
    let verifier: String = (0..64)
        .map(|_| {
            // 拒绝采样：66×3=198 < 256，余数落在符号表外就重取，无偏。
            loop {
                let mut pick = [0u8; 1];
                rng.fill_bytes(&mut pick);
                let value = pick[0] as usize;
                if value < PKCE_ALPHABET.len() * 3 {
                    break PKCE_ALPHABET[value % PKCE_ALPHABET.len()] as char;
                }
            }
        })
        .collect();
    use base64::Engine;
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(sha2::Sha256::digest(verifier.as_bytes()));
    Pkce {
        verifier,
        challenge,
    }
}

/// 一次性高熵值（`state` / `nonce` 各一份，每次登录独立）。
pub(crate) fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    rand::rng().fill_bytes(&mut buf);
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

/// 宿主标识（设计 §8「生成后持久保存，不能因每次启动或登录而变化」）。
/// 存在 `shell/<mode>/openai-oauth/host-id`（由调用方解析目录）。
///
/// **格式是 `urn:uuid:<v4>`**（官方规范的宿主标识示例；2026-10-09 实测
/// 64 位裸 hex 被授权端点以 `invalid_request_error, param:
/// ext_agent_host_id` 拒绝）。历史遗留的 64 位 hex 按**确定性映射**转成
/// URN UUID（取前 32 位 hex 摆成 8-4-4-4-12 并钉上 v4 版本位与变体位）——
/// 同一台宿主迁移前后仍是同一个身份，不要求用户手工删除重生成。
pub(crate) fn ensure_host_id(dir: &Path) -> Result<String, String> {
    let path = host_id_path(dir);
    if let Ok(existing) = std::fs::read_to_string(&path) {
        let trimmed = existing.trim();
        if trimmed.starts_with(URN_UUID_PREFIX) {
            return Ok(trimmed.to_string());
        }
        if trimmed.len() == 64 && trimmed.bytes().all(|b| b.is_ascii_hexdigit()) {
            let migrated = format!("{URN_UUID_PREFIX}{}", legacy_hex_to_uuid(trimmed));
            crate::shell::process::atomic_write(&path, format!("{migrated}\n").as_bytes())
                .map_err(|error| format!("迁移宿主标识失败（{}）：{error}", path.display()))?;
            return Ok(migrated);
        }
        return Err(format!(
            "宿主标识文件损坏（{}）：内容不是合法的宿主标识；请手工删除该文件后重试（会生成新的宿主标识）",
            path.display()
        ));
    }
    let id = new_urn_uuid();
    std::fs::create_dir_all(dir)
        .map_err(|error| format!("创建 OpenAI 服务目录失败（{}）：{error}", dir.display()))?;
    crate::shell::process::atomic_write(&path, format!("{id}\n").as_bytes())
        .map_err(|error| format!("写宿主标识失败（{}）：{error}", path.display()))?;
    Ok(id)
}

const URN_UUID_PREFIX: &str = "urn:uuid:";

/// 生成 `urn:uuid:<v4>`（16 随机字节，钉版本位 4 与 RFC 变体位）。
fn new_urn_uuid() -> String {
    let mut bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!("{URN_UUID_PREFIX}{}", uuid_from_bytes(&bytes))
}

/// 16 字节 → 8-4-4-4-12 小写 hex。
fn uuid_from_bytes(bytes: &[u8; 16]) -> String {
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// 64 位 hex（历史宿主标识）→ UUID 形态的确定性映射：取前 32 位 hex 摆成
/// 8-4-4-4-12，并把第 13 位钉成 '4'（v4）、第 17 位钉进 8..=b（变体位）。
fn legacy_hex_to_uuid(hex: &str) -> String {
    let compact: String = hex.chars().take(32).collect();
    let mut chars: Vec<char> = compact.chars().collect();
    chars[12] = '4';
    chars[16] = ['8', '9', 'a', 'b'][(chars[16].to_digit(16).unwrap_or(0) % 4) as usize];
    let s: String = chars.into_iter().collect();
    format!(
        "{}-{}-{}-{}-{}",
        &s[0..8],
        &s[8..12],
        &s[12..16],
        &s[16..20],
        &s[20..32]
    )
}

pub(crate) fn host_id_path(dir: &Path) -> PathBuf {
    dir.join("host-id")
}

/// 已签发 client_id 的持久化（`registration.json`，格式与历史兼容）。
pub(crate) fn save_registration(dir: &Path, client_id: &str) -> Result<(), String> {
    let path = registration_path(dir);
    std::fs::create_dir_all(dir)
        .map_err(|error| format!("创建 OpenAI 服务目录失败（{}）：{error}", dir.display()))?;
    crate::shell::process::atomic_write(
        &path,
        format!("{{\"clientId\":{client_id:?}}}\n").as_bytes(),
    )
    .map_err(|error| format!("写注册记录失败（{}）：{error}", path.display()))
}

/// 取已签发的 client_id：没有注册记录返回 `Ok(None)`；文件损坏报错
/// （fail-closed——猜一个 id 等于把令牌发进别人的应用）。
pub(crate) fn load_registration(dir: &Path) -> Result<Option<String>, String> {
    let path = registration_path(dir);
    let existing = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("读注册记录失败（{}）：{error}", path.display())),
    };
    let value: serde_json::Value = serde_json::from_str(&existing).map_err(|error| {
        format!(
            "注册记录损坏（{}）：{error}；请手工删除该文件后重试（会触发重新注册）",
            path.display()
        )
    })?;
    value
        .get("clientId")
        .and_then(|v| v.as_str())
        .map(|id| Ok(Some(id.to_string())))
        .unwrap_or_else(|| {
            Err(format!(
                "注册记录损坏（{}）：缺 clientId；请手工删除该文件后重试",
                path.display()
            ))
        })
}

pub(crate) fn registration_path(dir: &Path) -> PathBuf {
    dir.join("registration.json")
}

/// 授权 URL（浏览器打开的那一条）。宿主标识在首次注册和后续登录都必须
/// 发送；只有 `agent_name_hint` 仅属于首次动态注册。
#[allow(clippy::too_many_arguments)]
pub(crate) fn authorize_url(
    endpoint: &str,
    client_id: &str,
    redirect_uri: &str,
    state: &str,
    nonce: &str,
    challenge: &str,
    host_id: &str,
    first_registration: bool,
) -> String {
    let mut url = format!(
        "{endpoint}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={state}&nonce={nonce}&code_challenge={challenge}&code_challenge_method=S256&resource={}",
        urlencode(client_id),
        urlencode(redirect_uri),
        urlencode(AUTH_SCOPE),
        urlencode(OPENAI_RESOURCE),
    );
    url.push_str(&format!("&ext_agent_host_id={}", urlencode(host_id)));
    if first_registration {
        url.push_str(&format!("&agent_name_hint={}", urlencode(APP_NAME_HINT),));
    }
    url
}

fn urlencode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// 回调校验：`state` 必须逐字节匹配；`error` 参数原样上报（浏览器侧
/// 拒绝/取消不覆盖现有账户——由调用方决定，这里只给分类结果）。首次
/// 注册时授权服务器会在回调里带**签发的 client_id**（官方规范），原样
/// 交给调用方持久化。
#[derive(Debug)]
pub(crate) enum Callback {
    Code {
        code: String,
        client_id: Option<String>,
    },
    Error {
        error: String,
    },
    StateMismatch,
}

pub(crate) fn parse_callback(query: &str, expected_state: &str) -> Callback {
    let mut code: Option<String> = None;
    let mut client_id: Option<String> = None;
    let mut error: Option<String> = None;
    let mut state: Option<String> = None;
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        match key {
            "code" => code = Some(urldecode(value)),
            "client_id" => client_id = Some(urldecode(value)),
            "error" => error = Some(urldecode(value)),
            "state" => state = Some(urldecode(value)),
            _ => {}
        }
    }
    if state.as_deref() != Some(expected_state) {
        return Callback::StateMismatch;
    }
    if let Some(error) = error {
        return Callback::Error { error };
    }
    match code {
        Some(code) if !code.is_empty() => Callback::Code { code, client_id },
        _ => Callback::Error {
            error: "missing_code".into(),
        },
    }
}

fn urldecode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                if let Ok(byte) = u8::from_str_radix(hex, 16) {
                    out.push(byte);
                    i += 3;
                } else {
                    out.push(b'%');
                    i += 1;
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// ID token 校验（设计 §8：签名、签发方、受众、有效期、nonce）。
/// 时间用注入的 `now_unix`（测试可控）；`exp` 与 `iat` 都查。
pub(crate) fn verify_id_token(
    id_token: &str,
    jwks_json: &str,
    issuer: &str,
    audience: &str,
    expected_nonce: &str,
    now_unix: u64,
) -> Result<IdClaims, String> {
    let kid = kid_of(id_token)?;
    let jwk: RsaJwk =
        find_rsa_jwk(jwks_json, kid.as_deref()).ok_or("JWKS 里找不到对应的 RSA 公钥")?;
    let payload = verify_rs256(id_token, &jwk)?;
    let value: serde_json::Value = serde_json::from_str(&payload)
        .map_err(|error| format!("ID token 载荷不是 JSON：{error}"))?;
    let string_field = |name: &str| -> Result<String, String> {
        value
            .get(name)
            .and_then(|v| v.as_str())
            .map(String::from)
            .ok_or_else(|| format!("ID token 缺 {name} 声明"))
    };
    let got_issuer = string_field("iss")?;
    if got_issuer != issuer {
        return Err(format!(
            "ID token 签发方不匹配（{got_issuer}，期望 {issuer}）"
        ));
    }
    let got_audience = value.get("aud");
    let audience_ok = match got_audience {
        Some(serde_json::Value::String(single)) => single == audience,
        Some(serde_json::Value::Array(many)) => {
            many.iter().any(|item| item.as_str() == Some(audience))
        }
        _ => false,
    };
    if !audience_ok {
        return Err(format!("ID token 受众不匹配（期望 {audience}）"));
    }
    if string_field("nonce")? != expected_nonce {
        return Err("ID token nonce 不匹配（可能是重放）".into());
    }
    let exp = value
        .get("exp")
        .and_then(|v| v.as_u64())
        .ok_or("ID token 缺 exp 声明")?;
    if now_unix >= exp {
        return Err("ID token 已过期".into());
    }
    if let Some(iat) = value.get("iat").and_then(|v| v.as_u64()) {
        if iat > now_unix + 300 {
            return Err("ID token 签发时间超前超过 5 分钟（时钟异常？）".into());
        }
    }
    Ok(IdClaims {
        subject: string_field("sub")?,
        email: value
            .get("email")
            .and_then(|v| v.as_str())
            .map(String::from),
        chatgpt_account_id: namespaced_string(&value, CHATGPT_ACCOUNT_ID_CLAIM),
        chatgpt_plan_type: namespaced_string(&value, CHATGPT_PLAN_TYPE_CLAIM),
    })
}

/// 读取一个命名空间声明（非空才算数）。**可选**：缺声明不报错——它是
/// 套餐用量查询的附加条件，不参与身份判定（签名 / iss / aud / exp / nonce
/// 才是 fail-closed 的部分）。
fn namespaced_string(value: &serde_json::Value, claim: &str) -> Option<String> {
    value
        .get(claim)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
}

fn kid_of(jws: &str) -> Result<Option<String>, String> {
    let header_b64 = jws.split('.').next().ok_or("ID token 缺头部")?;
    use base64::Engine;
    let header = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(header_b64)
        .map_err(|_| "ID token 头部不是合法 base64url".to_string())?;
    let header: serde_json::Value = serde_json::from_slice(&header)
        .map_err(|error| format!("ID token 头部不是 JSON：{error}"))?;
    Ok(header.get("kid").and_then(|v| v.as_str()).map(String::from))
}

/// token 端点响应的必需字段（缺任一即错误——半份令牌不如报错）。
pub(crate) struct TokenSet {
    pub(crate) access_token: String,
    pub(crate) refresh_token: String,
    pub(crate) id_token: String,
    /// `expires_in`（秒）；0 视为未知，刷新按保守策略处理。
    pub(crate) expires_in: u64,
    pub(crate) scopes: Vec<String>,
}

pub(crate) fn parse_token_response(json: &str) -> Result<TokenSet, String> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|error| format!("token 响应不是有效 JSON：{error}"))?;
    let string_field = |name: &str| -> Result<String, String> {
        value
            .get(name)
            .and_then(|v| v.as_str())
            .map(String::from)
            .ok_or_else(|| format!("token 响应缺 {name}"))
    };
    Ok(TokenSet {
        access_token: string_field("access_token")?,
        refresh_token: string_field("refresh_token")?,
        id_token: string_field("id_token")?,
        expires_in: value
            .get("expires_in")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        scopes: value
            .get("scope")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .split_whitespace()
            .filter(|scope| !scope.is_empty())
            .map(String::from)
            .collect(),
    })
}

pub(crate) struct IdClaims {
    pub(crate) subject: String,
    pub(crate) email: Option<String>,
    /// ChatGPT 账号 id（ID token 的 `…/auth.chatgpt_account_id` 声明）。
    /// 套餐用量查询端点要求它作为 `ChatGPT-Account-Id` 请求头；登录时一并
    /// 落库，查询时不必再解 ID token。
    pub(crate) chatgpt_account_id: Option<String>,
    /// 账号套餐（`…/auth.chatgpt_plan_type`，如 `plus`）。纯展示用。
    pub(crate) chatgpt_plan_type: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_matches_rfc7636_test_vector() {
        // RFC 7636 附录 B 的官方向量：challenge 必须逐字节等于该值。
        let digest = {
            use base64::Engine;
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sha2::Sha256::digest(
                "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk".as_bytes(),
            ))
        };
        assert_eq!(digest, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
        // 字符表只含 RFC 的 unreserved 集，长度 64。
        let pkce = new_pkce();
        assert_eq!(pkce.verifier.len(), 64);
        assert!(pkce
            .verifier
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"b-._~".contains(&b)));
        let again = new_pkce();
        assert_ne!(pkce.verifier, again.verifier);
    }

    #[test]
    fn metadata_parse_is_fail_closed() {
        // 与线上 auth.openai.com 同形状：没有 registration_endpoint。
        let ok = parse_metadata(
            r#"{"issuer":"https://i","authorization_endpoint":"https://i/a","token_endpoint":"https://i/t","jwks_uri":"https://i/j"}"#,
        )
        .unwrap();
        assert_eq!(ok.issuer, "https://i");
        assert_eq!(
            discovery_url("https://i"),
            "https://i/.well-known/openid-configuration"
        );
        for missing in [
            "issuer",
            "authorization_endpoint",
            "token_endpoint",
            "jwks_uri",
        ] {
            let full = r#"{"issuer":"i","authorization_endpoint":"a","token_endpoint":"t","jwks_uri":"j"}"#;
            let value: serde_json::Value = serde_json::from_str(full).unwrap();
            let mut broken = value.clone();
            broken.as_object_mut().unwrap().remove(missing);
            assert!(
                parse_metadata(&broken.to_string()).is_err(),
                "{missing} 缺失应报错"
            );
        }
        let _ = ok;
    }

    #[test]
    fn host_id_is_stable_and_corruption_is_loud() {
        let dir = std::env::temp_dir().join(format!("oop-hostid-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let first = ensure_host_id(&dir).unwrap();
        // 格式：urn:uuid:<v4>（官方规范；裸 hex 会被授权端点拒绝）。
        assert!(first.starts_with("urn:uuid:"), "{first}");
        let body = first.trim_start_matches("urn:uuid:");
        assert_eq!(body.len(), 36);
        assert_eq!(body.chars().nth(14), Some('4'), "v4 版本位");
        assert_eq!(ensure_host_id(&dir).unwrap(), first, "已存在时必须原样复用");

        // 历史遗留的 64 位 hex 确定性迁移：同一输入永远得到同一身份。
        let legacy = format!("{}\n", "a".repeat(64));
        std::fs::write(host_id_path(&dir), legacy).unwrap();
        let migrated = ensure_host_id(&dir).unwrap();
        assert!(migrated.starts_with("urn:uuid:"), "{migrated}");
        assert_eq!(ensure_host_id(&dir).unwrap(), migrated, "迁移结果必须稳定");
        assert_eq!(
            ensure_host_id(&dir).unwrap(),
            migrated,
            "迁移是确定性的（同输入同输出）"
        );

        std::fs::write(host_id_path(&dir), "not-hex!\n").unwrap();
        let error = ensure_host_id(&dir).unwrap_err();
        assert!(error.contains("损坏"), "{error}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn registration_and_authorize_url_shapes() {
        // 注册记录的存取：首次落盘、复用、损坏报错。
        let dir = std::env::temp_dir().join(format!("oop-reg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(load_registration(&dir).unwrap().is_none());
        save_registration(&dir, "oaiapp_abc").unwrap();
        assert_eq!(
            load_registration(&dir).unwrap().as_deref(),
            Some("oaiapp_abc")
        );
        std::fs::write(registration_path(&dir), "{\"nope\":1}\n").unwrap();
        assert!(load_registration(&dir).is_err(), "缺 clientId 必须报损坏");
        let _ = std::fs::remove_dir_all(&dir);

        // 再次登录仍带稳定宿主标识，但不带首次注册提示。
        let url = authorize_url(
            "https://i/authorize",
            "oaiapp_abc",
            "http://127.0.0.1:54123/callback",
            "st",
            "no",
            "ch",
            "host-1",
            false,
        );
        assert!(url.starts_with("https://i/authorize?"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("client_id=oaiapp_abc"));
        assert!(url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A54123%2Fcallback"));
        assert!(url.contains("state=st&nonce=no"));
        assert!(url.contains("code_challenge=ch&code_challenge_method=S256"));
        assert!(url.contains(&format!("scope={}", urlencode(AUTH_SCOPE))));
        assert!(url.contains(&format!("resource={}", urlencode(OPENAI_RESOURCE))));
        assert!(url.contains("ext_agent_host_id=host-1"));
        assert!(!url.contains("agent_name_hint"), "再次登录不带宿主提示");

        // 首次注册的授权 URL：引导 client + 宿主提示。
        let first = authorize_url(
            "https://i/authorize",
            DYNAMIC_CLIENT_ID,
            "http://127.0.0.1:54123/callback",
            "st",
            "no",
            "ch",
            "host-1",
            true,
        );
        assert!(first.contains(&format!("client_id={}", urlencode(DYNAMIC_CLIENT_ID))));
        assert!(first.contains(&format!("agent_name_hint={}", urlencode(APP_NAME_HINT))));
        assert!(first.contains("ext_agent_host_id=host-1"));
    }

    #[test]
    fn callback_classifies_state_error_and_code() {
        match parse_callback("code=xyz&state=st", "st") {
            Callback::Code { code, client_id } => {
                assert_eq!(code, "xyz");
                assert!(client_id.is_none());
            }
            other => panic!("{other:?}"),
        }
        // 首次注册的回调带签发 client_id。
        match parse_callback("code=xyz&client_id=oaiapp_a&state=st", "st") {
            Callback::Code { code, client_id } => {
                assert_eq!(code, "xyz");
                assert_eq!(client_id.as_deref(), Some("oaiapp_a"));
            }
            other => panic!("{other:?}"),
        }
        match parse_callback("error=access_denied&state=st", "st") {
            Callback::Error { error } => assert_eq!(error, "access_denied"),
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            parse_callback("code=xyz&state=evil", "st"),
            Callback::StateMismatch
        ));
        assert!(matches!(
            parse_callback("state=st", "st"),
            Callback::Error { .. }
        ));
    }

    /// ID token 校验全链路：固定测试密钥签发 → 各拒绝分支逐个验红。
    /// 密钥材料复用 jwk.rs 测试的生成方式（openssl 预生成，仅测试用）。
    #[test]
    fn id_token_verification_branches() {
        use base64::Engine;
        use ring::rsa::KeyPair;
        use ring::signature::KeyPair as _;
        let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        let std_b64 = base64::engine::general_purpose::STANDARD;
        let der = std_b64.decode(KEY1_PKCS1).unwrap();
        let private = KeyPair::from_der(&der).unwrap();
        let modulus = private.public_key().modulus_len();
        let jwks = format!(
            r#"{{"keys":[{{"kty":"RSA","kid":"k1","n":"{}","e":"AQAB"}}]}}"#,
            {
                // 从 PKCS#1 DER 头部直接取 n 太绕，用 openssl 模数重算一次：
                // 测试常量里已有 n（KEY1_N），这里直接引用。
                KEY1_N
            }
        );
        let sign_token = |claims: &str| -> String {
            let header = b64.encode(br#"{"alg":"RS256","kid":"k1"}"#);
            let payload = b64.encode(claims.as_bytes());
            let signing_input = format!("{header}.{payload}");
            let mut signature = vec![0u8; modulus];
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
        };
        let now = 1_700_000_000u64;
        let good_claims = format!(
            r#"{{"iss":"https://i","aud":"client-1","sub":"user-1","nonce":"n1","exp":{},"iat":{}}}"#,
            now + 600,
            now - 10
        );
        let token = sign_token(&good_claims);
        let claims = verify_id_token(&token, &jwks, "https://i", "client-1", "n1", now).unwrap();
        assert_eq!(claims.subject, "user-1");

        let reject = |claims_json: &str, issuer: &str, audience: &str, nonce: &str| {
            let token = sign_token(claims_json);
            verify_id_token(&token, &jwks, issuer, audience, nonce, now).is_err()
        };
        // 错 issuer / 错 audience / 错 nonce（重放）各自被拒。
        assert!(reject(&good_claims, "https://evil", "client-1", "n1"));
        assert!(reject(&good_claims, "https://i", "client-2", "n1"));
        assert!(reject(&good_claims, "https://i", "client-1", "n2"));
        // 过期 / iat 超前。
        let expired = format!(
            r#"{{"iss":"https://i","aud":"client-1","sub":"u","nonce":"n1","exp":{}}}"#,
            now - 1
        );
        assert!(reject(&expired, "https://i", "client-1", "n1"));
        let future = format!(
            r#"{{"iss":"https://i","aud":"client-1","sub":"u","nonce":"n1","exp":{},"iat":{}}}"#,
            now + 600,
            now + 3600
        );
        assert!(reject(&future, "https://i", "client-1", "n1"));
        // audience 是数组且包含期望值时放行。
        let array_aud = format!(
            r#"{{"iss":"https://i","aud":["x","client-1"],"sub":"u","nonce":"n1","exp":{}}}"#,
            now + 600
        );
        let token = sign_token(&array_aud);
        assert!(verify_id_token(&token, &jwks, "https://i", "client-1", "n1", now).is_ok());
    }

    #[test]
    fn token_response_requires_every_field() {
        let full = parse_token_response(
            r#"{"access_token":"at","refresh_token":"rt","id_token":"it","expires_in":3600}"#,
        )
        .unwrap();
        assert_eq!(full.access_token, "at");
        assert_eq!(full.expires_in, 3600);
        for missing in ["access_token", "refresh_token", "id_token"] {
            let json = r#"{"access_token":"at","refresh_token":"rt","id_token":"it"}"#;
            let mut value: serde_json::Value = serde_json::from_str(json).unwrap();
            value.as_object_mut().unwrap().remove(missing);
            assert!(
                parse_token_response(&value.to_string()).is_err(),
                "{missing} 缺失应报错"
            );
        }
    }

    use crate::openai::testkeys::{KEY1_N, KEY1_PKCS1};
}
