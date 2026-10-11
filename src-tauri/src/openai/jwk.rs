//! JWK（RFC 7517）与 RS256 验签。
//!
//! `ring` 的 [`RsaPublicKeyComponents`] 直接吃 JWKS 的 RSA 公钥
//! （`n`/`e` 大端），不需要任何 DER 拼装（第一版手拼 SPKI 是不必要的——
//! ring 另有 components 入口）。仓库已有 `ring`（tauri/updater 传递依赖，
//! 零新下载）与 `base64`。
//!
//! **验证边界（如实记录）**：本模块按 OIDC/JWS 标准实现并被固定测试密钥
//! 的自签自验钉住；与 OpenAI 官方 SIWC 服务的线上一致性属设计 §10 的
//! 「官方动态注册到真实套餐推理」未验证项。

use ring::signature::{RsaPublicKeyComponents, RSA_PKCS1_2048_8192_SHA256};

/// 一个 RSA JWK 的最小字段（JWKS 里 `kty=RSA` 的条目）。
pub(crate) struct RsaJwk {
    pub(crate) kid: Option<String>,
    pub(crate) n: Vec<u8>,
    pub(crate) e: Vec<u8>,
}

/// base64url（无填充）解码；JWKS/JWS 的编码都不带 `=`。
fn b64url_decode(text: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(text)
        .ok()
}

/// 解出一个 JWS 的载荷 JSON，**不验签**。
///
/// 调用方要的只是「这份 token 声称自己属于哪个账号」这一个查询上下文，不是
/// 身份判定：验签发生在登录那一刻（[`verify_rs256`]），读出来的 vault 又是
/// 加密的。所以这里只做「切三段 → base64url 解载荷 → 解析 JSON」。
///
/// 判据取三处调用点里最严的那条，且只保留**真正起作用**的两条：字符集必须
/// 是 base64url（外加分隔符）、必须恰好三段。
///
/// 「三段」这一条是这里的实质安全语义：**没有签名段的字符串不该被当成一枚
/// 签名过的 token**。此前三个调用点松紧不一，OpenAI 额度侧只做
/// `split('.').nth(1)`，于是两段串同样能解出「账号 id」；判据分叉本身就是一处
/// 缺陷，哪怕三处恰好喂的都是自家签发的合法 token。
///
/// 曾经还有三条「各段非空」判断，反向验证时发现它们**一条都不起作用**：空载荷
/// 段解码出零字节，JSON 解析自然失败；空头/空签名段根本不影响解出什么。留着
/// 只会让判据看起来更严，而下一次改动不知道它到底守着什么——已删。
pub(crate) fn unverified_claims(jws: &str) -> Option<serde_json::Value> {
    let jws = jws.trim();
    if !jws
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    {
        return None;
    }
    let mut parts = jws.split('.');
    let payload = match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(header), Some(payload), Some(_signature), None) if !header.is_empty() => payload,
        _ => return None,
    };
    serde_json::from_slice(&b64url_decode(payload)?).ok()
}

/// 从 JWKS JSON 里按 `kid` 取 RSA 公钥；无 `kid` 时取第一把 RSA。
pub(crate) fn find_rsa_jwk(jwks_json: &str, kid: Option<&str>) -> Option<RsaJwk> {
    let value: serde_json::Value = serde_json::from_str(jwks_json).ok()?;
    let keys = value.get("keys")?.as_array()?;
    let pick = (if let Some(kid) = kid {
        keys.iter().find(|key| {
            key.get("kty").and_then(|v| v.as_str()) == Some("RSA")
                && key.get("kid").and_then(|v| v.as_str()) == Some(kid)
        })
    } else {
        keys.iter()
            .find(|key| key.get("kty").and_then(|v| v.as_str()) == Some("RSA"))
    })?;
    Some(RsaJwk {
        kid: pick.get("kid").and_then(|v| v.as_str()).map(String::from),
        n: b64url_decode(pick.get("n")?.as_str()?)?,
        e: b64url_decode(pick.get("e")?.as_str()?)?,
    })
}

/// RS256 验签（JWS：`header.payload.signature` 三段 base64url）。
/// 返回 `Ok(payload 明文)` 或带原因的错误文案。
pub(crate) fn verify_rs256(jws: &str, jwk: &RsaJwk) -> Result<String, String> {
    let mut parts = jws.split('.');
    let (_header_b64, Some(payload_b64), Some(signature_b64), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err("ID token 不是三段 JWS".into());
    };
    let signature = b64url_decode(signature_b64).ok_or("JWS 签名段不是合法 base64url")?;
    let signing_input = &jws[..jws.rfind('.').ok_or("JWS 缺分隔符")?];
    let public = RsaPublicKeyComponents::<Vec<u8>> {
        n: jwk.n.clone(),
        e: jwk.e.clone(),
    };
    public
        .verify(
            &RSA_PKCS1_2048_8192_SHA256,
            signing_input.as_bytes(),
            &signature,
        )
        .map_err(|error| format!("RS256 验签失败：{error:?}"))?;
    let payload = b64url_decode(payload_b64).ok_or("JWS 载荷段不是合法 base64url")?;
    String::from_utf8(payload).map_err(|_| "JWS 载荷不是 UTF-8".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openai::testkeys::{KEY1_N, KEY1_PKCS1, KEY2_N, KEY2_PKCS1};

    /// 固定测试密钥（2048 位，openssl 随机生成、仅测试用、不含任何真实身份）：
    /// `openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 -outform DER`。
    /// ring 0.17 没有 keygen API，测试签名方用 `KeyPair::from_der` 装载
    /// （PKCS#1；openssl 的 PKCS#8 v2 编码会被 ring 拒收，实测报 InvalidEncoding）。
    fn b64url_decode(text: &str) -> Vec<u8> {
        use base64::Engine;
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(text)
            .unwrap()
    }

    fn b64_decode(text: &str) -> Vec<u8> {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD
            .decode(text)
            .unwrap()
    }

    fn sign_jws(pkcs8_b64: &str, payload: &str) -> String {
        use ring::rsa::KeyPair;
        let der = b64_decode(pkcs8_b64);
        let private = KeyPair::from_der(&der).unwrap();
        let header = b64url(br#"{"alg":"RS256","kid":"test-key"}"#);
        let signing_input = format!("{header}.{}", b64url(payload.as_bytes()));
        let mut signature = vec![0u8; private.public().modulus_len()];
        let rng = ring::rand::SystemRandom::new();
        private
            .sign(
                &ring::signature::RSA_PKCS1_SHA256,
                &rng,
                signing_input.as_bytes(),
                &mut signature,
            )
            .unwrap();
        format!("{signing_input}.{}", b64url(&signature))
    }

    fn jwk_for(n_b64url: &str) -> RsaJwk {
        RsaJwk {
            kid: Some("test-key".into()),
            n: b64url_decode(n_b64url),
            e: b64url_decode("AQAB"),
        }
    }

    /// 自签自验 + 篡改拒绝 + 错钥匙拒绝：证明 n/e → DER 拼装路径正确。
    #[test]
    fn rs256_roundtrip_and_rejects_tampering() {
        let payload = r#"{"iss":"https://issuer","nonce":"n1"}"#;
        let jws = sign_jws(KEY1_PKCS1, payload);
        let verified = verify_rs256(&jws, &jwk_for(KEY1_N)).unwrap();
        assert_eq!(verified, payload);

        // 篡改载荷（换签名字段不动）→ 验签失败。
        let (header, _) = jws.split_once('.').unwrap();
        let signature = jws.rsplit('.').next().unwrap();
        let tampered = format!("{header}.{}.{}", b64url(br#"{"iss":"evil"}"#), signature);
        assert!(verify_rs256(&tampered, &jwk_for(KEY1_N)).is_err());
        // 用另一把不相关密钥的 JWK → 验签失败。
        assert!(verify_rs256(&jws, &jwk_for(KEY2_N)).is_err());
        // 两把钥匙互相也不通（反向再验一次）。
        let jws2 = sign_jws(KEY2_PKCS1, payload);
        assert!(verify_rs256(&jws2, &jwk_for(KEY1_N)).is_err());
    }

    /// JWKS 按 kid 取钥匙。
    #[test]
    fn jwks_lookup_by_kid() {
        let jwks = serde_json::json!({
            "keys": [
                { "kty": "RSA", "kid": "other", "n": "AAAA", "e": "AQAB" },
                { "kty": "RSA", "kid": "test-key", "n": KEY1_N, "e": "AQAB" },
            ]
        })
        .to_string();
        let found = find_rsa_jwk(&jwks, Some("test-key")).unwrap();
        assert_eq!(found.n, b64url_decode(KEY1_N));
        assert!(find_rsa_jwk(&jwks, Some("missing")).is_none());
    }

    fn b64url(bytes: &[u8]) -> String {
        use base64::Engine;
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
    }

    /// 收敛后的载荷解码：**恰好三段 + base64url 字符集**是唯一判据。
    ///
    /// 每条拒绝用例都挑**能真正隔离出那条规则**的形态。写这条测试时踩过一次
    /// 坑：第一版把 `"a..c"` 当作「空载荷段被拒」的证据，反向验证时把空段判断
    /// 删掉它照样绿——`""` 解出零字节、JSON 解析自然失败，与空段判断无关。
    /// 空段类用例只断言「解不出载荷」，不声称它钉住了空段规则。
    #[test]
    fn unverified_claims_applies_one_strict_shape_to_every_caller() {
        let payload = b64url(br#"{"scope":"chatgpt.tokens.use.direct"}"#);
        let header = b64url(br#"{"alg":"RS256"}"#);
        let good = format!("{header}.{payload}.sig");
        let scope = |token: &str| {
            unverified_claims(token)
                .and_then(|claims| {
                    claims
                        .get("scope")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned)
                })
                .unwrap_or_default()
        };

        // 合法三段：解出载荷（前后空白容忍，与登录时写库前的一致）。
        assert_eq!(scope(&good), "chatgpt.tokens.use.direct");
        assert_eq!(scope(&format!("  {good}  ")), "chatgpt.tokens.use.direct");

        // 缺签名段 / 多一段 / 没有分隔符 / 字符集越界：四条各自隔离一条规则。
        for (bad, why) in [
            (format!("{header}.{payload}"), "没有签名段"),
            (format!("{good}.{payload}"), "多出一段"),
            ("not-a-jwt".to_string(), "没有分隔符"),
            (format!("{header}.{payload}.签 名"), "签名段含非 ASCII"),
        ] {
            assert_eq!(scope(&bad), "", "{why}：{bad:?} 不该被当作合法 JWS");
        }

        // 解不出载荷（靠的是 JSON 解析失败，不是空段规则）。
        for bad in ["", "a..c", ".b.c", "a.b."] {
            assert_eq!(scope(bad), "", "{bad:?} 不该解出载荷");
        }
    }
}
