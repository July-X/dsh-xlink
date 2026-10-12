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

/// 全量原子写。写入失败不改动原文件。
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
    // 走私有入口：临时文件**一创建出来**就是 0600，正式路径上从未出现过
    // 权限更宽的那一刻。此前的写法是 `atomic_write` + 事后 `set_owner_only`，
    // 凭据从开始写起就以 0644 躺在同目录的临时文件里，rename 之后还要再宽上
    // 一会儿；进程在中途被杀，那份 0644 就永久留下了。删除 `set_owner_only`
    // 本身就是防回退——没有事后 chmod 的代码可搬回原处。
    crate::shell::atomic_write::atomic_write_private(path, &sealed)
        .map_err(|e| format!("写凭据文件失败（{}）：{e}", path.display()))
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
///
/// **`Ok(None)` 与 `Err` 必须分开**，这是本模块最要紧的约定：
/// - `Ok(None)` = 库里确实没有这条条目（首次登录），可以生成并存入；
/// - `Err(..)` = **读取动作本身失败**（钥匙串锁着、用户点了拒绝、`security`
///   / PowerShell 调不动、超时）。
///
/// 两者混为一谈会让「读不到」被当成「没有」，于是生成一把新密钥并用
/// `-U`（覆盖语义）写回——用户原有的那把当场销毁，`accounts.bin` 从此再也
/// 解不开，只能重走一遍 OAuth，而且**没有任何自愈路径**。读失败必须原样
/// 上抛：宁可让这一轮登录失败，也不能动用户已有的凭据。
pub(crate) type GetResult = Result<Option<String>, String>;

/// 取文件密钥：库里有就复用；确实没有（首次）就生成并存入；**读失败上抛**。
pub(crate) fn load_file_key_with(
    mode: &str,
    xlink_home: &Path,
    get: impl Fn(&str, &str) -> GetResult,
    put: impl Fn(&str, &str, &str) -> Result<(), String>,
) -> Result<[u8; 32], String> {
    let account = keyring_account(mode, xlink_home);
    match get(KEYRING_SERVICE, &account) {
        Ok(Some(existing)) => {
            return hex32(&existing).map_err(|()| {
                format!(
                    "系统凭据库里存的文件密钥不是 64 位 hex（条目 {account}）；请手工删除该条目后重试登录"
                )
            });
        }
        // 库里确实没有 → 首次登录，往下生成。
        Ok(None) => {}
        // 读失败 ≠ 不存在：不生成、不覆盖，把底层原因如实带出去。
        Err(error) => {
            return Err(format!(
                "读取系统凭据库失败（条目 {account}）：{error}。\
                 本次登录已中止，未改动任何现有凭据；\
                 请解锁系统钥匙串（或允许本应用访问）后重试"
            ));
        }
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

/// `security find-generic-password` 的 `errSecItemNotFound`（OSStatus 44）。
/// **只有**这一个退出码算「库里没有这条」；钥匙串锁着
/// （errSecInteractionNotAllowed）、用户拒绝授权（errSecAuthFailed）等一律
/// 是读取失败，原样上抛——它们一旦被当成「没有」，就会生成新密钥覆盖掉
/// 用户原有的那把。
const KEYCHAIN_ITEM_NOT_FOUND: i32 = 44;

/// Windows 脚本用来表示「枚举完了，确实没有这条」的标记。
const KEYRING_NOT_FOUND_MARKER: &str = "__DSH_NOT_FOUND__";

#[cfg(target_os = "macos")]
pub(crate) fn keyring_get(service: &str, account: &str) -> Result<Option<String>, String> {
    let output = std::process::Command::new("security")
        .args(["find-generic-password", "-a", account, "-s", service, "-w"])
        .output()
        .map_err(|e| format!("启动 security 失败：{e}"))?;
    if !output.status.success() {
        if output.status.code() == Some(KEYCHAIN_ITEM_NOT_FOUND) {
            return Ok(None);
        }
        // stderr 带出去：判断不了原因时，至少让用户和「查看日志」看得见
        // 到底是钥匙串锁了还是被拒绝授权。
        return Err(format!(
            "Keychain 查询失败（{account}）：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(Some(
        String::from_utf8_lossy(&output.stdout).trim().to_string(),
    ))
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
//
// 「条目不存在」**不靠异常类型判**。`Retrieve` 在无匹配时确实会抛异常
//（微软文档：Retrieve / FindAllByResource 均有 "If a match does not exist,
// an exception will throw"），但那是 WinRT 异常，经 PowerShell 投影后
// `GetType().FullName` 未必还保留 `ElementNotFound` 这个名字——认不出来就
// 当成「读取失败」的话，**Windows 上首次登录会直接失败**，那是比原缺陷更
// 严重的回归。
//
// 所以改用 `RetrieveAll()` 枚举后自行过滤：它对空库返回空集合、**不抛
// 异常**，判据完全确定。代价是枚举全部条目（几十毫秒），而这一次调用本来
// 就要 spawn 一个 PowerShell（几百毫秒），开销可以忽略。密码只在匹配上那条
// 时才 RetrievePassword，且不会出现在过滤过程的中间输出里。
/// Windows 的查询脚本（纯字符串拼接）。**刻意不带 `#[cfg]`**：这段脚本只
/// 在 Windows 上执行，而它一旦写错（花括号漏转义、续行符吃掉关键字符），
/// 在别的平台上没有任何机制会发现——所以把它抽出来，让任意平台都能对最终
/// 字面量做断言。
fn windows_keyring_get_script(service: &str, account: &str) -> String {
    format!(
        "try{{\
           $v=[Windows.Security.Credentials.PasswordVault,Windows.Security.Credentials,ContentType=WindowsRuntime]::new();\
           $hit=@($v.RetrieveAll() | Where-Object {{ $_.Resource -eq '{service}' -and $_.UserName -eq '{account}' }});\
           if($hit.Count -eq 0){{Write-Output '__DSH_NOT_FOUND__'}}else{{$hit[0].RetrievePassword();Write-Output $hit[0].Password}}\
         }}catch{{Write-Output ('__DSH_ERR__:' + $_.Exception.Message)}}"
    )
}

#[cfg(target_os = "windows")]
pub(crate) fn keyring_get(service: &str, account: &str) -> Result<Option<String>, String> {
    let script = windows_keyring_get_script(service, account);
    // 壳是 GUI 子系统应用：不挂 quiet，每个 powershell 都会闪一个终端窗
    // ——账户卡状态轮询/目录/推理每次都碰凭据库，终端窗会「不停的出现」
    // （2026-10-09 用户实测）。
    let mut cmd = std::process::Command::new("powershell");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
    crate::shell::process::quiet(&mut cmd);
    let output = cmd
        .output()
        .map_err(|e| format!("启动 powershell 失败：{e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    // 顺序有讲究：先认脚本自己给出的标记，再看退出码。标记只在脚本**完整跑完**
    // 时才出现，进程失败时 stdout 为空，会落到下面的退出码分支。
    if stdout == KEYRING_NOT_FOUND_MARKER {
        return Ok(None);
    }
    if let Some(reason) = stdout.strip_prefix("__DSH_ERR__:") {
        return Err(format!(
            "Credential Manager 读取条目时出错（{account}）：{}",
            reason.trim()
        ));
    }
    if !output.status.success() {
        return Err(format!(
            "Credential Manager 查询失败（{account}）：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    if stdout.is_empty() {
        return Err(format!(
            "Credential Manager 查询没有返回任何结果（{account}）"
        ));
    }
    Ok(Some(stdout))
}

#[cfg(target_os = "windows")]
pub(crate) fn keyring_put(service: &str, account: &str, secret: &str) -> Result<(), String> {
    let script = format!(
        "$v=[Windows.Security.Credentials.PasswordVault,Windows.Security.Credentials,ContentType=WindowsRuntime]::new();$c=New-Object Windows.Security.Credentials.PasswordCredential('{service}','{account}','{secret}');$v.Add($c)"
    );
    let mut cmd = std::process::Command::new("powershell");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
    crate::shell::process::quiet(&mut cmd);
    let output = cmd
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
        // 查不到 = 库里确实没有（首次），不是读取失败。
        let get = |service: &str, account: &str| -> GetResult {
            Ok(store
                .borrow()
                .get(&(service.to_string(), account.to_string()))
                .cloned())
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

    /// **读失败不等于「库里没有」**——这条防的是把用户现有登录改到解不开。
    ///
    /// 旧实现是 `if let Ok(existing) = get(...)`：钥匙串锁着、用户拒绝授权、
    /// `security` 调不动，全都是 `Err`，全都被当成「首次登录」，于是生成一把
    /// 新密钥并用 `-U` 覆盖写回——用户原来那把当场销毁，`accounts.bin` 从此
    /// 再也解不开，且没有自愈路径（2026-10-11 评审发现）。
    #[test]
    fn keyring_read_failure_never_regenerates_over_an_existing_key() {
        use std::cell::{Cell, RefCell};
        let home = std::path::Path::new("/home/u/.dsh-xlink");
        let account = keyring_account("release", home);
        let original = "ab".repeat(32);
        assert_eq!(original.len(), 64);

        // 条目确实存在（拿到的是用户原来那把密钥）。
        let store: RefCell<HashMap<(String, String), String>> = RefCell::new(HashMap::new());
        store.borrow_mut().insert(
            (KEYRING_SERVICE.to_string(), account.clone()),
            original.clone(),
        );

        // 读取动作失败：钥匙串锁着 / 用户点了拒绝 / 命令调不动……
        let get =
            |_: &str, _: &str| -> GetResult { Err("User interaction is not allowed.".to_string()) };
        let writes = Cell::new(0u32);
        let put = |_: &str, _: &str, _: &str| {
            writes.set(writes.get() + 1);
            Ok(())
        };

        let error = load_file_key_with("release", home, get, put)
            .expect_err("读取失败必须报错，绝不能当成「库里没有」而重新生成");
        assert!(error.contains("未改动任何现有凭据"), "{error}");
        assert_eq!(
            writes.get(),
            0,
            "读取失败时一次都不许写钥匙串——写了就等于销毁用户原有的密钥"
        );
        assert_eq!(
            store
                .borrow()
                .get(&(KEYRING_SERVICE.to_string(), account))
                .cloned()
                .as_deref(),
            Some(original.as_str()),
            "用户原有的密钥必须原封不动"
        );
    }

    /// 读失败之后凭据文件仍然解得开：拿回真密钥再读一次，账号还在。
    /// 上面那条钉住「没写」，这条钉住「没坏」——两者缺一都不算钉住。
    #[test]
    fn accounts_survive_a_keyring_read_failure() {
        use std::cell::Cell;
        let path = temp_path("survive");
        let key = [3u8; 32];
        let mut accounts = Accounts {
            active: Some("u1".into()),
            ..Default::default()
        };
        accounts.entries.insert(
            "u1".into(),
            AccountTokens {
                sub: "u1".into(),
                email: Some("a@example.com".into()),
                client_id: "c1".into(),
                access_token: "at".into(),
                refresh_token: "rt".into(),
                id_token: "id".into(),
                scopes: vec![],
                access_expires_at: 0,
                chatgpt_account_id: None,
                reauth_required: false,
            },
        );
        save_accounts(&path, &key, &accounts).unwrap();

        let failing = |_: &str, _: &str| -> GetResult { Err("钥匙串锁着".into()) };
        // put 一旦被调用就计数：读失败路径**必须一次都不碰钥匙串**。
        // 早先这里注入的是一个「调用就返回 Err」的闭包，于是 `.is_err()` 在
        // 坏行为下也成立 —— 测试自己把回归盖住了。
        let writes = Cell::new(0u32);
        let denied = |_: &str, _: &str, _: &str| -> Result<(), String> {
            writes.set(writes.get() + 1);
            Err("不该被调用".into())
        };
        assert!(
            load_file_key_with("release", std::path::Path::new("/x"), failing, denied).is_err()
        );
        assert_eq!(writes.get(), 0, "读失败路径不许写钥匙串");

        // 锁解除后再取密钥 → 账号完好。注入的必须正是当初加密用的那把
        // （hex 形态与 `load_file_key_with` 的生成方式一致）。
        let key_hex: String = key.iter().map(|b| format!("{b:02x}")).collect();
        let recovered = load_file_key_with(
            "release",
            std::path::Path::new("/x"),
            |_, _| Ok(Some(key_hex.clone())),
            |_, _, _| Ok(()),
        )
        .unwrap();
        assert_eq!(recovered, key);
        assert_eq!(load_accounts(&path, &recovered).unwrap(), accounts);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// Windows 查询脚本的**形状**。这段脚本只在 Windows 上跑，本机（mac）
    /// 没有 PowerShell 可解析它，所以只能对最终字面量做静态断言——但这几条
    /// 恰好覆盖「写错而且没人会发现」的几类：花括号漏转义、续行符吃掉关键
    /// 片段、又退回了依赖异常类型名的老写法。
    #[test]
    fn windows_keyring_script_shape_is_assertable_off_platform() {
        let script = windows_keyring_get_script("svc", "acct");

        // ① 不依赖异常类型名判断「不存在」：WinRT 异常经 PowerShell 投影后
        //    未必保留 ElementNotFound 这个名字，认不出来会让首次登录直接失败。
        assert!(
            !script.contains(".Retrieve("),
            "不得用单条 Retrieve（它的「不存在」只能靠异常识别）：{script}"
        );
        assert!(script.contains("RetrieveAll()"), "应枚举后过滤：{script}");
        assert!(
            script.contains(KEYRING_NOT_FOUND_MARKER),
            "空集合必须输出明确标记：{script}"
        );
        // ② try / catch 各一次，且 catch 仍兜底（vault 本身不可用也落到这里）。
        assert_eq!(script.matches("try{").count(), 1, "{script}");
        assert_eq!(script.matches("catch{").count(), 1, "{script}");
        assert!(script.contains("__DSH_ERR__:"), "{script}");
        assert!(
            script.contains("$hit[0].RetrievePassword();Write-Output $hit[0].Password"),
            "RetrievePassword 返回 void；必须随后输出 Password 属性：{script}"
        );
        // ③ 花括号配平：`format!` 里字面花括号要写成 `{{`/`}}`，漏一个就产出
        //    语法错误的脚本——而它只在 Windows 上才会暴露。
        assert_eq!(
            script.matches('{').count(),
            script.matches('}').count(),
            "花括号不配平，PowerShell 会语法错误：{script}"
        );
        // ④ 续行符吃掉换行与缩进后必须是一行可执行。
        assert!(!script.contains('\n'), "续行符应把脚本压成一行：{script}");
        // ⑤ service / account 真插进去了，而不是 `{service}` 这种没展开的字面量。
        assert!(script.contains("-eq 'svc'"), "{script}");
        assert!(script.contains("-eq 'acct'"), "{script}");
        assert!(
            !script.contains("{service}") && !script.contains("{account}"),
            "占位符没被展开：{script}"
        );
    }

    /// 执行真实查询脚本，但用内存对象替代 PasswordVault，绝不访问系统凭据库。
    #[cfg(target_os = "windows")]
    #[test]
    fn windows_keyring_script_reads_loaded_password_and_classifies_failures() {
        let query = windows_keyring_get_script("svc", "acct").replace(
            "[Windows.Security.Credentials.PasswordVault,Windows.Security.Credentials,ContentType=WindowsRuntime]::new()",
            "$fixture",
        );
        assert!(!query.contains("PasswordVault"), "必须先替换真实凭据库");
        let fixture = "$credential=[pscustomobject]@{Resource='svc';UserName='acct';Password=''};\
            $credential | Add-Member ScriptMethod RetrievePassword {$this.Password='fixture-key'};\
            $fixture=[pscustomobject]@{Entry=$credential};\
            $fixture | Add-Member ScriptMethod RetrieveAll {if($null -ne $this.Entry){$this.Entry}};";
        for (setup, expected) in [
            ("", "fixture-key"),
            ("$fixture.Entry=$null;", KEYRING_NOT_FOUND_MARKER),
            ("$fixture.Entry.UserName='different-account';", KEYRING_NOT_FOUND_MARKER),
            ("$credential | Add-Member ScriptMethod RetrievePassword {throw 'fixture-denied'} -Force;", "__DSH_ERR__:fixture-denied"),
        ] {
            let mut cmd = std::process::Command::new("powershell");
            cmd.args(["-NoProfile", "-NonInteractive", "-Command", &format!("{fixture}{setup}{query}")]);
            crate::shell::process::quiet(&mut cmd);
            let output = cmd.output().unwrap();
            assert!(output.status.success(), "模拟查询脚本执行失败");
            let stdout = String::from_utf8_lossy(&output.stdout);
            // ScriptMethod 的异常可能包一层 MethodInvocationException，保留原因即可。
            if expected.starts_with("__DSH_ERR__:") {
                assert!(stdout.trim().starts_with("__DSH_ERR__:"));
                assert!(stdout.contains("fixture-denied"));
            } else {
                assert_eq!(stdout.trim(), expected);
            }
        }
    }
}
