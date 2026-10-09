//! 加密凭据库（设计 §8）：令牌只落加密文件，文件密钥存系统凭据库。
//!
//! - 文件格式：`XOV1` 魔数 + 12 字节随机 nonce + AES-256-GCM 密文（含
//!   tag），明文是 [`Accounts`] 的 JSON。每次写全量原子替换，nonce 随之
//!   更新；Unix 上文件权限 0600（Windows 侧由用户 ACL 承担，见设计
//!   §8「不能以 Unix 权限位代替 Windows 权限验证」——写入路径不提权，
//!   落在用户 profile 下的默认 ACL 即用户私有）。
//! - 密钥：32 字节随机，首次生成后存 macOS Keychain（`security` CLI）/
//!   Windows Credential Manager（PowerShell `PasswordVault`，WinRT 投影，
//!   不走 Win32 FFI——src-tauri/AGENTS.md 的参数个数陷阱）。按「壳模式 +
//!   home 指纹」分键（开发计划 §3）。
//! - **测试纪律**：单测绝不触碰真实系统凭据库（同 `autostart.rs` 的
//!   「写路径人工验证」纪律）——密钥库函数保持薄且不进测试，可测逻辑
//!   （加密、文件、分键、生成）走注入的 get/put 闭包。
//! - 系统凭据库不可用时**不退回明文**：报错由调用方暂停登录（设计 §8）。

use std::collections::HashMap;
use std::path::Path;

use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM};
use serde::{Deserialize, Serialize};

const MAGIC: &[u8; 4] = b"XOV1";

/// 一份账号的令牌材料。相同邮箱的不同注册（`sub` 不同）分别保存，
/// `client_id` 随账号走（设计 §8「不能混用客户端标识与令牌」）。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccountTokens {
    pub(crate) sub: String,
    pub(crate) email: Option<String>,
    pub(crate) client_id: String,
    pub(crate) access_token: String,
    pub(crate) refresh_token: String,
    /// 保留的 ID token，用于后续 reauthorization 的 id_token_hint。
    #[serde(default)]
    pub(crate) id_token: String,
    /// 授权服务器实际授予的 scope；缺少套餐权限不得推理。
    #[serde(default)]
    pub(crate) scopes: Vec<String>,
    /// 访问令牌过期时刻（Unix 秒）；0 表示未知。
    pub(crate) access_expires_at: u64,
    /// ChatGPT 账号 id（ID token 命名空间声明，见 `auth::CHATGPT_ACCOUNT_ID_CLAIM`）。
    /// 套餐用量查询端点要它当 `ChatGPT-Account-Id` 请求头。登录时从 ID token
    /// 落库；老 vault 文件没有这个字段，按缺省处理（此时该分区查不了，退回
    /// 「未配置」而不是拿空值去打接口）。
    #[serde(default)]
    pub(crate) chatgpt_account_id: Option<String>,
    /// 刷新被判失效（invalid_grant）后置位；重新登录清除。老 vault 文件
    /// 没有这个字段，反序列化按 false 兜底。
    #[serde(default)]
    pub(crate) reauth_required: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Accounts {
    /// 当前活跃账号（`sub`）。无账号为 `None`。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) active: Option<String>,
    pub(crate) entries: HashMap<String, AccountTokens>,
}

// ── 文件加密 ─────────────────────────────────────────────────────────────

fn seal(key: &LessSafeKey, plaintext: &[u8]) -> Vec<u8> {
    use rand::Rng;
    let mut nonce_bytes = [0u8; 12];
    rand::rng().fill_bytes(&mut nonce_bytes);
    // seal_in_place_append_tag 要求 `&mut Vec`（要原地追加 tag）：
    // 先密封明文，再拼「魔数 + nonce + 密文」。
    let mut sealed = plaintext.to_vec();
    key.seal_in_place_append_tag(
        Nonce::assume_unique_for_key(nonce_bytes),
        Aad::empty(),
        &mut sealed,
    )
    .expect("AES-GCM seal 对合法输入不会失败");
    let mut out = Vec::with_capacity(MAGIC.len() + 12 + sealed.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&sealed);
    out
}

fn open(key: &LessSafeKey, bytes: &[u8]) -> Result<Vec<u8>, String> {
    if bytes.len() < MAGIC.len() + 12 + 16 || &bytes[..4] != MAGIC {
        return Err("凭据文件格式不正确（魔数不符或过短）；请退出登录后重新登录".into());
    }
    let mut nonce = [0u8; 12];
    nonce.copy_from_slice(&bytes[4..16]);
    let mut body = bytes[16..].to_vec();
    let plain = key
        .open_in_place(Nonce::assume_unique_for_key(nonce), Aad::empty(), &mut body)
        .map_err(|_| "凭据文件解密失败（密钥不匹配或文件被改动）".to_string())?;
    Ok(plain.to_vec())
}

fn key_from(bytes: &[u8; 32]) -> LessSafeKey {
    let unbound = UnboundKey::new(&AES_256_GCM, bytes).expect("32 字节是 AES-256 合法密钥长度");
    LessSafeKey::new(unbound)
}

/// 全量原子写（0600）。写入失败不改动原文件。
pub(crate) fn save_accounts(
    path: &Path,
    key_bytes: &[u8; 32],
    accounts: &Accounts,
) -> Result<(), String> {
    let plaintext = serde_json::to_vec(accounts).map_err(|e| format!("账号序列化失败：{e}"))?;
    let sealed = seal(&key_from(key_bytes), &plaintext);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("创建凭据目录失败（{}）：{e}", parent.display()))?;
    }
    crate::shell::process::atomic_write(path, &sealed)
        .map_err(|e| format!("写凭据文件失败（{}）：{e}", path.display()))?;
    set_owner_only(path);
    Ok(())
}

/// 读取；文件不存在视为「从未登录过」的空库，损坏如实报错（不按空文件
/// 处理——静默清空等于删掉用户的登录）。
pub(crate) fn load_accounts(path: &Path, key_bytes: &[u8; 32]) -> Result<Accounts, String> {
    let sealed = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Accounts::default())
        }
        Err(error) => return Err(format!("读凭据文件失败（{}）：{error}", path.display())),
    };
    let plain = open(&key_from(key_bytes), &sealed)?;
    serde_json::from_slice(&plain).map_err(|e| format!("凭据文件内容损坏：{e}"))
}

#[cfg(unix)]
fn set_owner_only(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(windows)]
fn set_owner_only(_path: &Path) {
    // 用户 profile 下的默认 ACL 即用户私有；设计 §8 的 Windows 权限校验
    // 属实机验收项，不在写入路径提权。
}

// ── 系统凭据库（薄壳，不进测试） ─────────────────────────────────────────

/// 密钥条目的 service 名（固定）。
pub(crate) const KEYRING_SERVICE: &str = "dsh-xlink.openai-oauth";

/// 条目 account：`<mode>-<home 指纹前 12>`（开发计划 §3 的分键）。
pub(crate) fn keyring_account(mode: &str, xlink_home: &Path) -> String {
    let digest = {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(xlink_home.to_string_lossy().as_bytes());
        let full = hasher.finalize();
        full.iter().map(|b| format!("{b:02x}")).collect::<String>()
    };
    format!("{mode}-{}", &digest[..12])
}

// 用泛型而不是 fn 指针：测试要注入捕获 RefCell 的闭包，fn 指针接不住。
pub(crate) type GetResult = Result<String, String>;

/// 取文件密钥：库里有就复用；没有（首次）就生成并存入。
pub(crate) fn load_file_key_with(
    mode: &str,
    xlink_home: &Path,
    get: impl Fn(&str, &str) -> Result<String, String>,
    put: impl Fn(&str, &str, &str) -> Result<(), String>,
) -> Result<[u8; 32], String> {
    let account = keyring_account(mode, xlink_home);
    if let Ok(existing) = get(KEYRING_SERVICE, &account) {
        if let Ok(bytes) = hex32(&existing) {
            return Ok(bytes);
        }
        return Err(format!(
            "系统凭据库里存的文件密钥不是 64 位 hex（条目 {account}）；请手工删除该条目后重试登录"
        ));
    }
    use rand::Rng;
    let mut key = [0u8; 32];
    rand::rng().fill_bytes(&mut key);
    let hex: String = key.iter().map(|b| format!("{b:02x}")).collect();
    put(KEYRING_SERVICE, &account, &hex).map_err(|error| {
        format!("把文件密钥写入系统凭据库失败：{error}；登录已暂停（不退回明文保存）")
    })?;
    Ok(key)
}

fn hex32(text: &str) -> Result<[u8; 32], ()> {
    if text.len() != 64 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(());
    }
    let mut out = [0u8; 32];
    for (i, chunk) in text.as_bytes().chunks(2).enumerate() {
        out[i] = u8::from_str_radix(std::str::from_utf8(chunk).unwrap_or(""), 16).unwrap_or(0);
    }
    Ok(out)
}

#[cfg(target_os = "macos")]
pub(crate) fn keyring_get(service: &str, account: &str) -> Result<String, String> {
    let output = std::process::Command::new("security")
        .args(["find-generic-password", "-a", account, "-s", service, "-w"])
        .output()
        .map_err(|e| format!("启动 security 失败：{e}"))?;
    if !output.status.success() {
        return Err(format!(
            "Keychain 查询失败（{account}）：{}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(target_os = "macos")]
pub(crate) fn keyring_put(service: &str, account: &str, secret: &str) -> Result<(), String> {
    let output = std::process::Command::new("security")
        .args([
            "add-generic-password",
            "-a",
            account,
            "-s",
            service,
            "-w",
            secret,
            "-U",
        ])
        .output()
        .map_err(|e| format!("启动 security 失败：{e}"))?;
    if !output.status.success() {
        return Err(format!(
            "Keychain 写入失败（{account}）：{}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(())
}

// Windows：PowerShell 的 PasswordVault（WinRT 投影，不走 Win32 FFI）。
// 实机验证属发布验收项（同 autostart 的写路径纪律），此处保持薄与可审。
#[cfg(target_os = "windows")]
pub(crate) fn keyring_get(service: &str, account: &str) -> Result<String, String> {
    let script = format!(
        "[Windows.Security.Credentials.PasswordVault,Windows.Security.Credentials,ContentType=WindowsRuntime]::new().Retrieve('{service}','{account}').Password"
    );
    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .map_err(|e| format!("启动 powershell 失败：{e}"))?;
    if !output.status.success() {
        return Err(format!("Credential Manager 查询失败（{account}）"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(target_os = "windows")]
pub(crate) fn keyring_put(service: &str, account: &str, secret: &str) -> Result<(), String> {
    let script = format!(
        "$v=[Windows.Security.Credentials.PasswordVault,Windows.Security.Credentials,ContentType=WindowsRuntime]::new();$c=New-Object Windows.Security.Credentials.PasswordCredential('{service}','{account}','{secret}');$v.Add($c)"
    );
    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .map_err(|e| format!("启动 powershell 失败：{e}"))?;
    if !output.status.success() {
        return Err(format!("Credential Manager 写入失败（{account}）"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("oop-vault-{}-{}", std::process::id(), tag));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("accounts.bin")
    }

    #[test]
    fn seal_open_roundtrip_and_tamper_detection() {
        let key = [7u8; 32];
        let sealed = seal(&key_from(&key), br#"{"active":"u1"}"#);
        assert_eq!(&sealed[..4], b"XOV1");
        assert_eq!(
            open(&key_from(&key), &sealed).unwrap(),
            br#"{"active":"u1"}"#
        );
        // 篡改密文中段 → 解密失败。
        let mut tampered = sealed.clone();
        let mid = tampered.len() / 2;
        tampered[mid] ^= 0xff;
        assert!(open(&key_from(&key), &tampered).is_err());
        // 换钥匙 → 解密失败。
        assert!(open(&key_from(&[8u8; 32]), &sealed).is_err());
        // 坏魔数 → 格式错误。
        let mut bad = sealed;
        bad[0] = b'X';
        bad[1] = b'Z';
        assert!(open(&key_from(&key), &bad).is_err());
    }

    #[test]
    fn accounts_file_lifecycle() {
        let path = temp_path("life");
        let key = [1u8; 32];
        // 不存在 → 空库。
        assert_eq!(load_accounts(&path, &key).unwrap(), Accounts::default());
        // 存取往返；多账号 + active。
        let mut accounts = Accounts::default();
        accounts.entries.insert(
            "u1".into(),
            AccountTokens {
                sub: "u1".into(),
                email: Some("a@example.com".into()),
                client_id: "c1".into(),
                access_token: "at1".into(),
                refresh_token: "rt1".into(),
                id_token: "id1".into(),
                scopes: vec!["chatgpt.tokens.use.direct".into()],
                access_expires_at: 123,
                chatgpt_account_id: None,
                reauth_required: false,
            },
        );
        accounts.entries.insert(
            "u2".into(),
            AccountTokens {
                sub: "u2".into(),
                email: Some("a@example.com".into()), // 同邮箱不同注册
                client_id: "c2".into(),
                access_token: "at2".into(),
                refresh_token: "rt2".into(),
                id_token: "id2".into(),
                scopes: vec!["chatgpt.tokens.use.direct".into()],
                access_expires_at: 0,
                chatgpt_account_id: None,
                reauth_required: false,
            },
        );
        accounts.active = Some("u2".into());
        save_accounts(&path, &key, &accounts).unwrap();
        assert_eq!(load_accounts(&path, &key).unwrap(), accounts);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "凭据文件必须仅属主可读写");
        }
        // 损坏 → 报错（不按空库处理）。
        let sealed = std::fs::read(&path).unwrap();
        std::fs::write(&path, &sealed[..sealed.len() - 1]).unwrap();
        assert!(load_accounts(&path, &key).is_err());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// 密钥装载：注入的 get/put 闭包驱动（不碰真实系统凭据库）。
    #[test]
    fn file_key_generation_and_reuse() {
        use std::cell::RefCell;
        let store: RefCell<HashMap<(String, String), String>> = RefCell::new(HashMap::new());
        let get = |service: &str, account: &str| {
            store
                .borrow()
                .get(&(service.to_string(), account.to_string()))
                .cloned()
                .ok_or_else(|| "missing".to_string())
        };
        let put = |service: &str, account: &str, secret: &str| {
            store.borrow_mut().insert(
                (service.to_string(), account.to_string()),
                secret.to_string(),
            );
            Ok(())
        };
        let home = std::path::Path::new("/home/z/.dsh-xlink");
        let first = load_file_key_with("release", home, get, put).unwrap();
        // 二次读取复用同一条目。
        let second = load_file_key_with("release", home, get, put).unwrap();
        assert_eq!(first, second);
        // 分键：不同模式拿到不同条目（不同密钥）。
        let dev = load_file_key_with("dev", home, get, put).unwrap();
        assert_ne!(first, dev);
        // 库里存了坏值 → 报错并指明条目，不静默重生成（防把用户现有登录变砖）。
        let account = keyring_account("release", home);
        store
            .borrow_mut()
            .insert((KEYRING_SERVICE.to_string(), account), "zz".into());
        let error = load_file_key_with("release", home, get, put).unwrap_err();
        assert!(error.contains("不是 64 位 hex"), "{error}");
    }
}
