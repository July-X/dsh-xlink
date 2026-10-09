//! 访问令牌刷新（设计 §8「刷新按账户注册串行，写入失败不报告更新成功」；
//! 官方令牌规范是**旋转 refresh token**——每次刷新换发新的 refresh token）。
//!
//! 失败分两类，处置不同：
//! - `invalid_grant` / `invalid_client`（授权被撤销或刷新令牌失效）→
//!   [`RefreshError::ReauthRequired`]：账号在 vault 里标记 `reauthRequired`，
//!   状态命令据此显示第四态「需要重新登录」，**不盲目重试**；
//! - 网络类失败 → [`RefreshError::Message`]：保持原令牌原样（只有写入成功
//!   才算刷新成功），调用方稍后再试——桥接每次服务请求前才检查，天然重试。

use std::sync::{Mutex, OnceLock};

use crate::openai::auth::{discovery_url, parse_metadata, parse_token_response};
use crate::openai::flow::{FlowPaths, FlowTransport};
use crate::openai::transport::Failure;
use crate::openai::vault::{self, AccountTokens};

#[derive(Debug)]
pub(crate) enum RefreshError {
    /// 需要重新登录（授权撤销 / 刷新令牌失效）。`reauth_required` 已落库。
    ReauthRequired(String),
    /// 其它失败（网络、解析、IO）：原令牌原样保留，稍后重试。
    Message(String),
}

impl RefreshError {
    pub(crate) fn message(&self) -> String {
        match self {
            RefreshError::ReauthRequired(detail) => format!("授权已失效（{detail}）；请重新登录"),
            RefreshError::Message(detail) => detail.clone(),
        }
    }
}

fn refresh_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// 取「已确保新鲜」的活跃账号令牌：未过期（30 秒余量）原样返回；过期
/// 先按串行刷新并整份写回 vault。未登录返回 `Ok(None)`。
pub(crate) fn ensure_fresh_access(
    paths: &FlowPaths,
    deps: &impl FlowTransport,
    mode: &str,
) -> Result<Option<AccountTokens>, RefreshError> {
    let key = vault::load_file_key_with(
        mode,
        &paths.xlink_home,
        |service, account| deps.keyring_get(service, account),
        |service, account, secret| deps.keyring_put(service, account, secret),
    )
    .map_err(RefreshError::Message)?;
    let mut accounts =
        vault::load_accounts(&paths.vault_file, &key).map_err(RefreshError::Message)?;
    let Some(sub) = accounts.active.clone() else {
        return Ok(None);
    };
    let entry = accounts.entries.get(&sub).cloned().ok_or_else(|| {
        RefreshError::Message("活跃账号的凭据条目缺失（vault 数据不一致）；请重新登录".into())
    })?;

    if entry.reauth_required {
        return Err(RefreshError::ReauthRequired("刷新令牌已失效".into()));
    }
    if entry.access_expires_at > deps.now_unix() + 30 {
        return Ok(Some(entry));
    }

    let refreshed = refresh_once(paths, deps, &entry)?;
    match refreshed {
        Some(tokens) => {
            accounts.entries.insert(sub.clone(), tokens.clone());
            accounts.active = Some(sub);
            vault::save_accounts(&paths.vault_file, &key, &accounts).map_err(|error| {
                RefreshError::Message(format!(
                    "刷新后写入凭据失败（新令牌未生效，稍后重试）：{error}"
                ))
            })?;
            Ok(Some(tokens))
        }
        // invalid_grant：把 reauth_required 标记落库（这本身是一次写入，
        // 失败也不回滚内存结论——状态命令读内存标记）。
        None => {
            let mut flagged = entry.clone();
            flagged.reauth_required = true;
            accounts.entries.insert(sub, flagged);
            let _ = vault::save_accounts(&paths.vault_file, &key, &accounts);
            Err(RefreshError::ReauthRequired("服务端拒绝了刷新令牌".into()))
        }
    }
}

/// 一次刷新。`Ok(None)` 表示服务端判 `invalid_grant`/`invalid_client`。
fn refresh_once(
    paths: &FlowPaths,
    deps: &impl FlowTransport,
    entry: &AccountTokens,
) -> Result<Option<AccountTokens>, RefreshError> {
    let _guard = refresh_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let discovery = deps
        .get_json(&discovery_url(&paths.issuer_base))
        .map_err(|failure| RefreshError::Message(failure.message(&[])))?;
    let metadata = parse_metadata(&discovery).map_err(RefreshError::Message)?;
    // token 端点按 OAuth 惯例用 400/401 + {"error":"invalid_grant"} 表达
    // 「授权失效」——ureq 把 4xx 转成 Status 失败，必须在 map_err 之前
    // 先分类（直接 map_err 会把「该重新登录」误报成「网络问题」）。
    let response = match deps.post_form(
        &metadata.token_endpoint,
        &[
            ("grant_type", "refresh_token"),
            ("refresh_token", entry.refresh_token.as_str()),
            ("client_id", entry.client_id.as_str()),
            // 官方规范：刷新与授权必须带同一个 resource。
            ("resource", crate::openai::auth::OPENAI_RESOURCE),
        ],
    ) {
        Ok(text) => text,
        Err(Failure::Status(status, body))
            if (status == 400 || status == 401)
                && (body.contains("invalid_grant") || body.contains("invalid_client")) =>
        {
            return Ok(None)
        }
        Err(failure) => return Err(RefreshError::Message(failure.message(&[]))),
    };
    let tokens = parse_token_response(&response)
        .map_err(|error| RefreshError::Message(format!("刷新响应不合规：{error}")))?;
    Ok(Some(AccountTokens {
        sub: entry.sub.clone(),
        email: entry.email.clone(),
        client_id: entry.client_id.clone(),
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token,
        access_expires_at: deps.now_unix() + tokens.expires_in,
        reauth_required: false,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openai::flow::tests::{
        flow_test_paths, read_vault_with, set_access_expiry, MockIssuer,
    };
    use std::sync::atomic::AtomicBool;

    /// 过期 → 串行刷新（旋转 refresh token）→ 新令牌整份落库。
    #[test]
    fn expired_token_refreshes_with_rotation() {
        let issuer = MockIssuer::spawn();
        let transport = issuer.transport();
        let paths = flow_test_paths("refresh-ok", issuer.port());
        let cancel = AtomicBool::new(false);
        crate::openai::flow::run_authorize(&paths, &transport, "release", &cancel).unwrap();

        // 把过期时间改成过去，强制走刷新。
        set_access_expiry(&transport, &paths.vault_file, "mock-sub-1", 1);
        let fresh = ensure_fresh_access(&paths, &transport, "release")
            .unwrap()
            .unwrap();
        assert_eq!(fresh.access_token, "at-2");
        assert_eq!(fresh.refresh_token, "rt-2");
        // 落库的也是新值；再取一次直接命中「未过期」路径（值不变）。
        let stored = read_vault_with(&transport, &paths.vault_file).unwrap();
        assert_eq!(stored.entries["mock-sub-1"].access_token, "at-2");
        let again = ensure_fresh_access(&paths, &transport, "release")
            .unwrap()
            .unwrap();
        assert_eq!(again.access_token, "at-2");
    }

    /// 服务端撤销（invalid_grant）→ ReauthRequired + 标记落库 →
    /// 再次调用走标记短路，不再打服务端。
    #[test]
    fn revoked_grant_flags_reauth_and_short_circuits() {
        let issuer = MockIssuer::spawn();
        let transport = issuer.transport();
        let paths = flow_test_paths("refresh-deny", issuer.port());
        let cancel = AtomicBool::new(false);
        crate::openai::flow::run_authorize(&paths, &transport, "release", &cancel).unwrap();
        issuer.revoke();
        set_access_expiry(&transport, &paths.vault_file, "mock-sub-1", 1);

        let error = ensure_fresh_access(&paths, &transport, "release").unwrap_err();
        assert!(
            matches!(error, RefreshError::ReauthRequired(_)),
            "{error:?}"
        );
        let stored = read_vault_with(&transport, &paths.vault_file).unwrap();
        assert!(stored.entries["mock-sub-1"].reauth_required, "标记必须落库");
        // 标记短路：即使服务端解除撤销（这里不再调用），也会立刻拒绝。
        let again = ensure_fresh_access(&paths, &transport, "release").unwrap_err();
        assert!(matches!(again, RefreshError::ReauthRequired(_)));

        // 重新登录清除标记（流程全量 upsert）。
        let view =
            crate::openai::flow::run_authorize(&paths, &transport, "release", &cancel).unwrap();
        assert_eq!(view.sub, "mock-sub-1");
        let stored = read_vault_with(&transport, &paths.vault_file).unwrap();
        assert!(!stored.entries["mock-sub-1"].reauth_required);
    }

    /// 未过期（30 秒余量内）→ 原样返回，不触网。
    #[test]
    fn fresh_token_is_returned_as_is() {
        let issuer = MockIssuer::spawn();
        let transport = issuer.transport();
        let paths = flow_test_paths("refresh-fresh", issuer.port());
        let cancel = AtomicBool::new(false);
        crate::openai::flow::run_authorize(&paths, &transport, "release", &cancel).unwrap();
        // now = 1_700_000_000，授权时 expires = now + 3600 → 未过期。
        let fresh = ensure_fresh_access(&paths, &transport, "release")
            .unwrap()
            .unwrap();
        assert_eq!(fresh.access_token, "at-1");
    }
}
