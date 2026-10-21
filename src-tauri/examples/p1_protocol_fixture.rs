//! P1 协议夹具的 **Rust 侧** runner（**判据载体**，不是实现）。
//!
//! 协议稿 `docs/features/official-chat/official-chat-p1-protocol.md` §6：
//! 「**同一组 JSON 夹具**，Rust 与 JavaScript **各跑一遍**，断言『同一夹具 → 同一解析结果』」。
//!
//! 本 example 只干这一件事：
//! 读 `plugins/official-chat/fixtures/protocol/` 里的夹具 → 调**生产那份**
//! `official_chat::protocol::resolve_case` → 输出同构 JSON
//! （`{schemaVersion, constants, errorCodes, cases:[{id,result}]}`），
//! 供 JS 侧的判据（`scripts/p1-protocol.run.mjs`）与 Rust 侧的结果做深比较。
//!
//! **为什么连常量与错误码表一起输出**：协议 §6 明写「`schemaVersion`
//! **两端常量必须一致**」，§3 明写 14 个码与各自的可重试性/用户下一步。
//! 只跑夹具结果的话，这两条**没人验**——所以这里把生产源码里的常量一并吐出来，
//! 由 JS 侧判据做**双端深比较**。
//!
//! ## 为什么用 `#[path]` 而不是 `use` 库里的模块
//!
//! example 是**另一个 crate**，`official_chat` 是库里的 `pub(crate) mod`——直接
//! `use` 不到。`#[path]` 把**生产源码本身**编进来（不是复制一份），
//! 所以这里跑的就是实现将来要用的那份代码。
//!
//! ## 隔离
//!
//! 本 example **只读**夹具目录，不写任何文件、不起浏览器、不碰用户数据目录。
//!
//! ## 用法
//!
//! ```powershell
//! cargo build --offline --example p1_protocol_fixture
//! & '.\target\debug\examples\p1_protocol_fixture.exe'
//! ```
//!
//! 夹具目录默认按仓库相对位置找；也可用 `DSH_P1_FIXTURE_DIR` 指定。

use std::fs;
use std::path::PathBuf;

use serde_json::{json, Value};

/// 生产那份协议实现（**源码本身**，不是副本）。
#[allow(dead_code)]
#[path = "../src/official_chat/protocol.rs"]
mod protocol;

/// example 是另一个 crate，没有 `crate::shell::digest::hex`。
/// 这里给一份**同名同形状**的最小实现（小写十六进制、每字节两位），
/// 让 `#[path]` 进来的那份生产源码在 example crate 里也能编译——
/// 跑的仍然是生产那份代码。
mod shell {
    pub mod digest {
        /// 摘要字节 → 小写十六进制（无分隔符）。
        pub fn hex(digest: impl AsRef<[u8]>) -> String {
            const DIGITS: &[u8; 16] = b"0123456789abcdef";
            let bytes = digest.as_ref();
            let mut out = String::with_capacity(bytes.len() * 2);
            for byte in bytes {
                out.push(DIGITS[(byte >> 4) as usize] as char);
                out.push(DIGITS[(byte & 0x0f) as usize] as char);
            }
            out
        }
    }
}

/// 找夹具目录（仓库相对位置；可用环境变量覆盖）。
fn fixture_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("DSH_P1_FIXTURE_DIR") {
        let path = PathBuf::from(dir);
        if path.join("index.json").exists() {
            return path;
        }
    }
    let candidates = [
        PathBuf::from("plugins/official-chat/fixtures/protocol"),
        PathBuf::from("../plugins/official-chat/fixtures/protocol"),
        PathBuf::from("../../plugins/official-chat/fixtures/protocol"),
    ];
    for candidate in candidates.iter() {
        if candidate.join("index.json").exists() {
            return candidate.clone();
        }
    }
    PathBuf::from("plugins/official-chat/fixtures/protocol")
}

/// Rust 侧的**协议常量**（§4 首版常量 + §6 的 `schemaVersion`）。
///
/// **为什么也要输出常量**：协议 §6 明写「`schemaVersion` **两端常量必须一致**」，
/// §3 明写 14 个码与可重试性/用户下一步——只跑夹具结果的话，这两条**没人验**。
/// 这里把生产那份源码里的常量一并吐出来，JS 侧判据做**双端深比较**。
fn constants() -> Value {
    json!({
        "catalogPageSize": protocol::CATALOG_PAGE_SIZE,
        "catalogCacheTtlMs": protocol::CATALOG_CACHE_TTL_MS,
        "searchDebounceMs": protocol::SEARCH_DEBOUNCE_MS,
        "readTimeoutMs": protocol::READ_TIMEOUT_MS,
        "maxBodyBytes": protocol::MAX_BODY_BYTES,
        "maxResponseBytes": protocol::MAX_RESPONSE_BYTES,
        "snapshotRetentionMs": protocol::SNAPSHOT_RETENTION_MS,
        "cacheTotalBudgetBytes": protocol::CACHE_TOTAL_BUDGET_BYTES,
        "maxReferencesPerMessage": protocol::MAX_REFERENCES_PER_MESSAGE,
        "perBodyBudgetBytes": protocol::PER_BODY_BUDGET_BYTES,
        "perMessageBudgetBytes": protocol::PER_MESSAGE_BUDGET_BYTES,
        "schemaVersion": protocol::SCHEMA_VERSION,
    })
}

/// Rust 侧的**错误码表**（§3 的 14 个码 + 可重试性 + 用户下一步）。
fn error_codes() -> Value {
    let mut out = serde_json::Map::new();
    for code in protocol::ERROR_CODES {
        let index = match protocol::error_index(code) {
            Some(index) => index,
            None => continue,
        };
        out.insert(
            code.to_string(),
            json!({
                "retryable": protocol::ERROR_RETRYABLE[index],
                "userNext": protocol::ERROR_USER_NEXT[index],
            }),
        );
    }
    Value::Object(out)
}

fn main() {
    let dir = fixture_dir();
    let index_path = dir.join("index.json");
    let index_raw = match fs::read_to_string(&index_path) {
        Ok(text) => text,
        Err(error) => {
            eprintln!(
                "p1_protocol_fixture: 读不到夹具索引 {}：{error}",
                index_path.display()
            );
            std::process::exit(2);
        }
    };
    let index: Value = match serde_json::from_str(&index_raw) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("p1_protocol_fixture: 夹具索引不是合法 JSON：{error}");
            std::process::exit(2);
        }
    };
    let cases = index
        .get("cases")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut out: Vec<Value> = Vec::with_capacity(cases.len());
    for name in cases {
        let name = name.as_str().unwrap_or("").to_string();
        let path = dir.join(&name);
        let raw = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) => {
                eprintln!(
                    "p1_protocol_fixture: 读不到夹具 {}：{error}",
                    path.display()
                );
                std::process::exit(2);
            }
        };
        let fixture: Value = match serde_json::from_str(&raw) {
            Ok(value) => value,
            Err(error) => {
                eprintln!("p1_protocol_fixture: 夹具 {name} 不是合法 JSON：{error}");
                std::process::exit(2);
            }
        };
        let id = fixture
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or(&name)
            .to_string();
        let input = fixture.get("input").cloned().unwrap_or(Value::Null);
        let result = protocol::resolve_case(&input);
        out.push(json!({ "id": id, "result": result }));
    }

    let payload = json!({
        "schemaVersion": protocol::SCHEMA_VERSION,
        "constants": constants(),
        "errorCodes": error_codes(),
        "cases": out,
    });
    println!(
        "{}",
        serde_json::to_string(&payload).unwrap_or_else(|error| {
            eprintln!("p1_protocol_fixture: 结果序列化失败：{error}");
            String::from("{}")
        })
    );
}
