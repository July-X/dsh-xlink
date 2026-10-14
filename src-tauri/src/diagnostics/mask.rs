//! 凭据形状的掩码：把**形状明确**的几类片段抹成 `***`。
//!
//! 从 `diagnostics/run.rs` 拆出来（2026-10-15）：那份文件已经 614 行、预算 590，而这一块
//! 与「怎么落盘、怎么生成运行记录 id」是两件事——它是一条**可复用判据**，日志、模型输入
//! 与插件侧（`plugins/memory/host/mask.js`，两边形状由冒烟测试逐项钉住）都在用它。
//!
//! 两条自律，改之前先读：
//! · **只认形状明确的几类**（`sk-` 开头的 key、`Bearer <令牌>`、赋值形态的 token/secret）。
//!   刻意不做通用正则去匹配任意长随机串——那会把正常的版本号、指纹、端口号也吃掉，反而把
//!   证据毁掉；
//! · **宁可漏抹也不要误抹**。误抹的是用户写下的正文（提取输入就是散文），而正文是这条链路
//!   唯一的价值。所以空格**不是**分隔符：`token 这个词` 不该被吃（中文没有词间空格，一吃
//!   就是一整句），但 `token = abc` 与 `token: abc` 都算——分隔符两侧允许空格，只是必须真的
//!   有一个 `=` 或 `:`。

/// 剥掉看起来像凭据的片段。
///
/// 只处理**形状明确**的几类（`sk-` 开头的 key、`Bearer ` 头、赋值形态的
/// token/secret）。刻意不做通用正则去匹配任意长随机串——那会把正常的
/// 版本号、指纹、端口号也吃掉，反而把证据毁掉。
pub(crate) fn strip_token_shapes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        // `sk-` 开头的 key：DeepSeek / OpenAI 风格的最小前缀。
        if chars[i] == 's' && chars.get(i + 1) == Some(&'k') && chars.get(i + 2) == Some(&'-') {
            let mut j = i + 3;
            while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '-') {
                j += 1;
            }
            if j - i > 8 {
                out.push_str("sk-***");
                i = j;
                continue;
            }
        }
        // `Bearer <token>`
        if chars[i] == 'B' && text_at(&chars, i, "Bearer ") {
            let mut j = i + 7;
            while j < chars.len() && !chars[j].is_whitespace() {
                j += 1;
            }
            if j > i + 7 {
                out.push_str("Bearer ***");
                i = j;
                continue;
            }
        }
        // `<key>=<value>` / `<key>: <value>`，键名命中 token/secret/key/password。
        if chars[i].is_ascii_alphabetic() || chars[i] == '_' {
            let mut j = i;
            while j < chars.len()
                && (chars[j].is_ascii_alphanumeric() || chars[j] == '_' || chars[j] == '-')
            {
                j += 1;
            }
            let key: String = chars[i..j].iter().collect();
            let lowered = key.to_ascii_lowercase();
            let sensitive = [
                "token",
                "secret",
                "apikey",
                "api_key",
                "password",
                "passwd",
                "credential",
                "authorization",
            ]
            .iter()
            .any(|needle| lowered.contains(needle));
            // 分隔符只认 `=` 与 `:`，**空格不算**：`token 这个词` 在散文里是常态，把空格当
            // 分隔符会把后面那一段整个吃掉（中文没有词间空格，一吃就是一整句）。
            // 2026-10-15 从插件侧复刻同一套形状时发现，两边一起改。
            // 空格可以在分隔符**两侧**（`token = abc` 与 `token: abc` 都算），但**必须真的
            // 有一个 `=` 或 `:`**：`token 这个词` 是散文，没有分隔符就不该被吃（中文没有
            // 词间空格，一吃就是一整句）。2026-10-15 从插件侧复刻同一套形状时发现。
            let mut k = j;
            let mut has_sep = false;
            while k < chars.len() && (chars[k] == ' ' || chars[k] == '=' || chars[k] == ':') {
                if chars[k] != ' ' {
                    has_sep = true;
                }
                k += 1;
            }
            if sensitive && has_sep {
                let mut v = k;
                while v < chars.len() && !chars[v].is_whitespace() && chars[v] != ',' {
                    v += 1;
                }
                if v > k {
                    // 值本身是**认证方案名**时（`Authorization: Bearer <令牌>`），真正的凭据在
                    // 它后面那一段。只抹掉 `Bearer` 是最坏的一种结果——输出看起来已经脱敏，
                    // 而令牌原样留在后面。2026-10-15 由插件侧复刻同一套形状时发现，两边一起修。
                    v = scheme_value_end(&chars, k, v).unwrap_or(v);
                    out.push_str(&key);
                    out.push('=');
                    out.push_str("***");
                    i = v;
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// 凭据值本身只是认证方案名时，真正的凭据在它**后面**那一段：跳过空白，吃掉下一个非空白段。
///
/// 认的是三种常见方案名；不是方案名就返回 `None`（保持原行为——值就是值）。
fn scheme_value_end(chars: &[char], value_start: usize, value_end: usize) -> Option<usize> {
    const SCHEMES: [&str; 3] = ["Bearer", "Basic", "Token"];
    let word: String = chars[value_start..value_end].iter().collect();
    if !SCHEMES.contains(&word.as_str()) {
        return None;
    }
    let mut cursor = value_end;
    while cursor < chars.len() && chars[cursor].is_whitespace() {
        cursor += 1;
    }
    let mut end = cursor;
    while end < chars.len() && !chars[end].is_whitespace() && chars[end] != ',' {
        end += 1;
    }
    if end > cursor {
        Some(end)
    } else {
        None
    }
}

fn text_at(chars: &[char], start: usize, needle: &str) -> bool {
    needle
        .chars()
        .enumerate()
        .all(|(offset, expected)| chars.get(start + offset) == Some(&expected))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Authorization: Bearer <令牌>` 里的**令牌**必须一起抹掉。
    ///
    /// 这一条是 2026-10-15 从插件侧发现的：那边的同一套形状在复刻时撞见了它——只抹掉
    /// `Bearer` 这个词，令牌原样留在后面，而输出**看起来**已经脱敏了（比不抹更坏：
    /// 看到 `Bearer ***` 的人不会再去看一眼）。两边一起修，夹具也同一组。
    #[test]
    fn a_bearer_token_is_stripped_with_its_scheme_word() {
        // 空格不再算分隔符之后，抹掉令牌的是 `Bearer <token>` 那条规则本身——结果里留着
        // 方案名，令牌没了，要的正是这个。
        assert_eq!(
            strip_token_shapes("Authorization: Bearer abc.def.ghi"),
            "Authorization=***"
        );
        assert_eq!(
            strip_token_shapes("authorization=Basic dXNlcjpwYXNz"),
            "authorization=***"
        );
        assert_eq!(
            strip_token_shapes("Authorization: Bearer"),
            "Authorization=***"
        );
        // 值不是方案名时行为不变。
        assert_eq!(strip_token_shapes("password=hunter2"), "password=***");
        assert_eq!(
            strip_token_shapes("端口 3099，版本 v1.2.3"),
            "端口 3099，版本 v1.2.3"
        );
        // **空格不再是分隔符**：散文里的「token 这个词」不该被吃掉——中文没有词间空格，
        // 一吃就是一整句。结构化日志用的是 `=`/`:`，那两种照旧抹。
        assert_eq!(
            strip_token_shapes("正文里提到 token 这个词但不带分隔符"),
            "正文里提到 token 这个词但不带分隔符"
        );
        assert_eq!(strip_token_shapes("token: abc123"), "token=***");
        assert_eq!(strip_token_shapes("token = abc123"), "token=***");
        assert_eq!(strip_token_shapes("token abc123"), "token abc123");
    }
}
