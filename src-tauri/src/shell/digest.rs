//! 摘要的十六进制表示。
//!
//! **不要用 `format!("{:x}", hasher.finalize())`**：sha2 0.11 的 `finalize()`
//! 返回的类型（`hybrid_array::Array`）不再实现 `LowerHex`，而 0.10 返回的
//! `GenericArray` 实现了。写死那个格式等于把编译锁在 0.10——依赖一升就编不过
//! （Dependabot 的 sha2 0.11 PR 从 2026-09-13 起一直红在这里；2026-10-20 顺着
//! 它才找到这 6 处）。
//!
//! 这里自己转十六进制：**两版 sha2 都能编，输出逐字节相同**（SHA-256 的十六
//! 进制是标准：小写、无分隔符、每字节两位、零填充）。仓库里另有几处用
//! `digest.iter().map(|b| format!("{b:02x}")).collect()` 写的一样的逻辑，统一
//! 调这里——**同一语义只留一份**，下次再升 sha2 只改这一处。
//!
//! **调用点一律传 `&digest[..]` 切片**，不要直接传 `finalize()` 的返回值：
//! 0.10 返回 `GenericArray`、0.11 返回 `hybrid_array::Array`，两者各自的
//! `AsRef` / `Index` 实现细节并不相同，**切片 `&x[..]` 才是两版都成立的
//! 那个交点**。

/// 摘要字节 → 小写十六进制（无分隔符）。
///
/// 与 `format!("{:x}", …)` 逐字节相同：小写、每字节两位、零填充。
pub(crate) fn hex(digest: impl AsRef<[u8]>) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let bytes = digest.as_ref();
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 标准向量：SHA-256("") 与 SHA-256("abc")。
    ///
    /// **这是兼容性判据，不是新鲜感断言**：这些十六进制串会落进盘上的状态文件
    /// （技能的 `materialized_sha256`、插件 id 的短哈希、凭据指纹、Node 产物
    /// 校验），改一位就会让存量数据对不上——所有权判定静默失效，用户的技能被
    /// 当成「不是我们的」而重新物化。所以钉的是**逐字节相同**，不是「算得出来」。
    #[test]
    fn matches_the_standard_sha256_hex() {
        use sha2::{Digest, Sha256};
        assert_eq!(
            hex(&Sha256::digest(b"")[..]),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex(&Sha256::digest(b"abc")[..]),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    /// 空输入与单字节边界：零要补成两位（`00`），高位不能丢。
    #[test]
    fn zero_padding_and_high_bits() {
        assert_eq!(hex([0x00u8, 0x0f, 0xf0, 0xff]), "000ff0ff");
    }
}
