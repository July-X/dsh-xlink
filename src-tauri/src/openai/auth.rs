//! OAuth/OIDC 授权核心（设计 §8；开发计划 §5「账户状态 / 授权动作」）。
//!
//! **协议形状按 OIDC 标准实现**：发现文档（`<issuer>/.well-known/
//! openid-configuration`）、RFC 7591 动态注册、RFC 7636 PKCE（S256）、
//! `state`/`nonce` 一次性高熵值、RS256 ID token 验签（[`crate::openai::
//! jwk`]）。模块内每个与线上一致性相关的判断都被模拟授权服务器测试
//! 钉住；**官方 SIWC 服务的真实端点行为属设计 §10 的未验证项**——issuer
//! 常量与任何专有差异在联调（真实账号）时只改本文件的配置层。
//!
//! 传输（发现/注册/换令牌的 HTTP）不在本文件：出网必须走
//! `net_proxy::routes()`（src-tauri/AGENTS.md），届时以参数注入；纯逻辑
//! 先行落定与离线验证。

use std::path::{Path, PathBuf};

use rand::Rng;
use sha2::Digest;

use crate::openai::jwk::{find_rsa_jwk, verify_rs256, RsaJwk};

/// **未验证常量**：SIWC 的 issuer 基址（设计 §10「官方动态注册到真实
/// 套餐推理」未验证项；真实联调时更正，不改协议层）。
pub(crate) const SIWC_ISSUER: &str = "https://auth.openai.com";

/// 发现文档：`<issuer>/.well-known/openid-configuration`。
pub(crate) fn discovery_url(issuer: &str) -> String {
    format!("{issuer}/.well-known/openid-configuration")
}

/// 解析发现文档；缺任一必需端点即错误（fail-closed，不猜 URL）。
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
        registration_endpoint: string_of("registration_endpoint")?,
        jwks_uri: string_of("jwks_uri")?,
    })
}

#[derive(Debug, PartialEq)]
pub(crate) struct Metadata {
    pub(crate) issuer: String,
    pub(crate) authorization_endpoint: String,
    pub(crate) token_endpoint: String,
    pub(crate) registration_endpoint: String,
    pub(crate) jwks_uri: String,
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
/// 存在 `shell/<mode>/openai-oauth/host-id`（由调用方解析目录），一行 32 字节 hex。
pub(crate) fn ensure_host_id(dir: &Path) -> Result<String, String> {
    let path = host_id_path(dir);
    if let Ok(existing) = std::fs::read_to_string(&path) {
        let trimmed = existing.trim();
        if trimmed.len() == 64 && trimmed.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Ok(trimmed.to_string());
        }
        return Err(format!(
            "宿主标识文件损坏（{}）：内容不是 64 位 hex；请手工删除该文件后重试（会触发重新动态注册）",
            path.display()
        ));
    }
    let id = random_hex(32);
    std::fs::create_dir_all(dir)
        .map_err(|error| format!("创建 OpenAI 服务目录失败（{}）：{error}", dir.display()))?;
    crate::shell::process::atomic_write(&path, format!("{id}\n").as_bytes())
        .map_err(|error| format!("写宿主标识失败（{}）：{error}", path.display()))?;
    Ok(id)
}

pub(crate) fn host_id_path(dir: &Path) -> PathBuf {
    dir.join("host-id")
}

/// 动态注册请求体（RFC 7591 + SIWC 的宿主标识）。
pub(crate) fn registration_request(host_id: &str, redirect_uris: &[String]) -> String {
    serde_json::json!({
        "client_name": format!("dsh-xlink/{host_id}"),
        "redirect_uris": redirect_uris,
        "token_endpoint_auth_method": "none",
        "grant_types": ["authorization_code"],
        "response_types": ["code"],
    })
    .to_string()
}

/// 注册响应里必须能取到 `client_id`（再次登录复用对应注册）。
pub(crate) fn parse_registration(json: &str) -> Result<String, String> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|error| format!("注册响应不是有效 JSON：{error}"))?;
    value
        .get("client_id")
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or_else(|| "注册响应缺 client_id".into())
}

/// 授权 URL（浏览器打开的那一条）：response_type=code + PKCE + state +
/// nonce + scope（套餐授权范围；scope 字符串与 SIWC 文档一致属未验证项）。
pub(crate) fn authorize_url(
    endpoint: &str,
    client_id: &str,
    redirect_uri: &str,
    state: &str,
    nonce: &str,
    challenge: &str,
) -> String {
    format!(
        "{endpoint}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={state}&nonce={nonce}&code_challenge={challenge}&code_challenge_method=S256",
        urlencode(client_id),
        urlencode(redirect_uri),
        urlencode("openid offline_access model.request"),
    )
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
/// 拒绝/取消不覆盖现有账户——由调用方决定，这里只给分类结果）。
#[derive(Debug)]
pub(crate) enum Callback {
    Code { code: String },
    Error { error: String },
    StateMismatch,
}

pub(crate) fn parse_callback(query: &str, expected_state: &str) -> Callback {
    let mut code: Option<String> = None;
    let mut error: Option<String> = None;
    let mut state: Option<String> = None;
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        match key {
            "code" => code = Some(urldecode(value)),
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
        Some(code) if !code.is_empty() => Callback::Code { code },
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
    })
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

pub(crate) struct IdClaims {
    pub(crate) subject: String,
    pub(crate) email: Option<String>,
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
        let ok = parse_metadata(
            r#"{"issuer":"https://i","authorization_endpoint":"https://i/a","token_endpoint":"https://i/t","registration_endpoint":"https://i/r","jwks_uri":"https://i/j"}"#,
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
            "registration_endpoint",
            "jwks_uri",
        ] {
            let full = r#"{"issuer":"i","authorization_endpoint":"a","token_endpoint":"t","registration_endpoint":"r","jwks_uri":"j"}"#;
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
        assert_eq!(first.len(), 64);
        assert_eq!(ensure_host_id(&dir).unwrap(), first, "已存在时必须原样复用");
        std::fs::write(host_id_path(&dir), "not-hex!\n").unwrap();
        let error = ensure_host_id(&dir).unwrap_err();
        assert!(error.contains("损坏"), "{error}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn registration_and_authorize_url_shapes() {
        let client = parse_registration(r#"{"client_id":"abc"}"#).unwrap();
        assert_eq!(client, "abc");
        assert!(parse_registration("{}").is_err());
        let url = authorize_url(
            "https://i/authorize",
            "abc",
            "http://127.0.0.1:54123/callback",
            "st",
            "no",
            "ch",
        );
        assert!(url.starts_with("https://i/authorize?"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("client_id=abc"));
        assert!(url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A54123%2Fcallback"));
        assert!(url.contains("state=st&nonce=no"));
        assert!(url.contains("code_challenge=ch&code_challenge_method=S256"));
        let request: serde_json::Value = serde_json::from_str(&registration_request(
            "host-1",
            &["http://127.0.0.1:1/cb".into()],
        ))
        .unwrap();
        assert_eq!(request["client_name"], "dsh-xlink/host-1");
        assert_eq!(request["token_endpoint_auth_method"], "none");
    }

    #[test]
    fn callback_classifies_state_error_and_code() {
        match parse_callback("code=xyz&state=st", "st") {
            Callback::Code { code } => assert_eq!(code, "xyz"),
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

    /// 与 jwk.rs 同源的固定测试密钥（仅测试用）。
    const KEY1_PKCS1: &str = "MIIEowIBAAKCAQEA1y6x1vPCLBzqw41U9X+ytxLRHNzKYI20B22QYwV/KhPpKDy1bo+mMXEKjEHKb5Fu6AyEkQPFXf8v4bM6GL5V4Dlit+coKqIPhs7r7NNg0VYthmT7LOV9r/t/ogKNB6otyH0+pXtDhi4cWK6ZJK0VPLPt/EL/L40kO9Un9LHM/A9xO42StP0hnQKrMK7h6klygpAxyoD0dxm8pPlvDHcZMhqkOR81vdsNX66y/RtXDHbQQMXZJ+Lu2z8ymCfvtL+2a3PiStLWjfDarZqtlJEs2tSYJr7IU0fKRJGSAE96tZu1aWrYn164VYpb/mklGc4+NyHfU91hskHQO2UFOavooQIDAQABAoIBAC9VRilSVVP+yGVboWSfQmCi8vy2VI4InaFEqI4fl2laF9+R+xbm4lfd1cQkdLM1+n9wwXhkq/WRPKcZFZ57v8gi12Q8pMk7/M5aleryVEm3+yuk6ttlX9BmMh0hEoStGoUPh8g+5QuO+Q1I2scGi7VenurukdOT6HSA3tkkg0Kue6dQugZGgKazTxIwWPsTzEBVmfQXtoA2qeUzeH6wAWKcsaB4Z3fUmadgJwR/1CvMb+qEuUPnO/BT3hZOv6e1Thc1BYraIWhygL02JmQOiQKYRCnTP1JK2rtH6BAMvgG4AraHj6YAX7L5gB5QzeAZzSzInmracq7oYgGRAj+fFkECgYEA7KbvO5duRqlYmTAClYh/UfZ1N98E3d1X+K/E9HK0X45jHZyWXD9omvajDaAaDZjH0sic145NRUmnJ1t9nZE7l3+7IfaxA4upXQg5B8yuxdsWNbWPgy/Zt2dN9f9fwyKLHEdJVCDxk2VnR2uefYWE0VXiTfCB4cOp3xU87vhU3YMCgYEA6MZnw4c+m6QJuB7NDxdnsD44oIgCk8K4xygcXm79idwCLSlIx5bzsvqhIE72DUtTk6ZkCa+83/y2wgkClDnfypizaRp1tWnqs6KzsmKlpp+vfWWwGiL3E5uVYQw3Wh+aCayaclpXcwOUG9JrigOHaevxO82/jopiObWWi5bWzAsCgYEAuZUH0tGkFyHCaw8tV5qdTedacSAhruNfk5Qzfgddz/nXXGdpupm3LJ7xq0O8aqE/QtsztA7SJd3miYTD84brFpmCZNYSZtdlT6GdJ7Kp9FslBaWGD7i8oYkPqDRGIr66HMkChkj3aUGCRo3s0j6cs5UITVqoYCWS13DOQhDYbIUCgYBzUCaDNHKNg9vUvF11RnD1XD2NOROdw27qKjKzjWRIcRca7ELDrUIYvhQn/zXhLBnBIUKZkdeNVpHq2a/PYkQ9BxyJyrPZJRlB2C4RBtFtE9pJ0qBEsmGX8xEzPGwHV3Rlqn3wfFSqA3HRvpHLkyf4Dww4RhrJMECsugpUKGtMNQKBgF26tonZb4V44b3jDvvd37qp6nzOqCbgLRn5F//a5X4Isl4JHIYVWR5XQtNVlZjx+Y8XWCq/bfxji3ClcxSSpVAgxh2KuBeADhN9w90pn2z4NfLB1tM3YLn0k+tVFhBTbDQXCkG/eXAhPw9cADzzbp6OByPKRJsF3KIL11yqPXWK";
    const KEY1_N: &str = "1y6x1vPCLBzqw41U9X-ytxLRHNzKYI20B22QYwV_KhPpKDy1bo-mMXEKjEHKb5Fu6AyEkQPFXf8v4bM6GL5V4Dlit-coKqIPhs7r7NNg0VYthmT7LOV9r_t_ogKNB6otyH0-pXtDhi4cWK6ZJK0VPLPt_EL_L40kO9Un9LHM_A9xO42StP0hnQKrMK7h6klygpAxyoD0dxm8pPlvDHcZMhqkOR81vdsNX66y_RtXDHbQQMXZJ-Lu2z8ymCfvtL-2a3PiStLWjfDarZqtlJEs2tSYJr7IU0fKRJGSAE96tZu1aWrYn164VYpb_mklGc4-NyHfU91hskHQO2UFOavooQ";
}
