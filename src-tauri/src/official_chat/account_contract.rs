//! 账户端点与页内回包白名单，供生产取数和独立探针共用。

pub(crate) fn identity_path(site: &str) -> Option<(&'static str, &'static [&'static str])> {
    match site {
        "deepseek" => Some(("/api/v0/users/current", &["data", "biz_data", "id"])),
        "minimax" => Some(("/v1/api/user/info", &["data", "userInfo", "realUserID"])),
        "chatgpt" => Some(("/api/auth/session", &["user", "id"])),
        _ => None,
    }
}

#[cfg(any(test, target_os = "macos"))]
pub(crate) fn projection_keys(path: &str) -> Option<&'static [&'static str]> {
    match path {
        "/api/v0/users/current" => Some(&["code", "data", "biz_code", "biz_data", "id"]),
        "/v1/api/user/info" => Some(&[
            "data",
            "userInfo",
            "realUserID",
            "statusInfo",
            "code",
            "base_resp",
            "status_code",
        ]),
        "/api/auth/session" => Some(&["user", "id"]),
        _ => None,
    }
}
