//! `NODE_OPTIONS` 的清洗：不让一个坏掉的 Node 预加载把壳派生的子进程一起拖死。
//!
//! **为什么需要它**：`NODE_OPTIONS` 里的 `--require` / `--import` 指向**用户
//! 机器上的文件**。那个文件被删掉（工具卸载、清理脚本、换机器）而变量没跟着
//! 清时，**每一个** Node 进程都会在预加载阶段崩掉：
//!
//! ```text
//! Error: Cannot find module 'C:/Users/me/AppData/Local/x/bootstrap.cjs'
//! Require stack:
//! - internal/preload
//! ```
//!
//! 崩在 Node 的**启动阶段**意味着内核一行代码都没加载——用户看到的是「工作台
//! 起不来」，而事故面板查不到任何插件或内核组件，连"重装内核"这种**错的**
//! 建议都只会把用户引到更远的地方去（2026-10-20 实测：内核 0.2.1-rc.2 本身
//! 完好，纯粹被这条环境变量挡住）。
//!
//! 壳是 GUI 程序，从资源管理器启动，继承得到用户环境里的这个变量——于是**每
//! 一次**内核启动都撞上它，而壳自己没有任何拦截手段。
//!
//! **只剔除坏的，不动好的**：用户自装的转译器、CA 注入、代理补丁都是**有效**
//! 预加载，壳不该改写它们。因此这里只丢掉**指向不存在文件**的那几项，其余
//! 原样保留——变量本身完全正常时，一个字节都不改。

use std::path::Path;

/// 会触发预加载的选项名。`--require` 与 `-r` 等价；`--import` 是 ESM 版的
/// 预加载（Node ≥ 20.6），同样在启动阶段加载磁盘上的文件。
const PRELOAD_FLAGS: [&str; 3] = ["--require", "-r", "--import"];

/// 一条预加载项。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Preload {
    /// 选项名（`--require` / `-r` / `--import`），重建时原样写回。
    flag: String,
    /// 预加载目标路径。
    path: String,
}

/// `NODE_OPTIONS` 拆开后的一个条目。
#[derive(Debug, Clone, PartialEq, Eq)]
enum Entry {
    /// 普通选项，原样保留。
    Plain(String),
    /// 预加载项。
    Preload(Preload),
}

/// 纯函数：把一份 `NODE_OPTIONS` 洗成可以安全继承的值。
///
/// 返回 `None` 表示「别给子进程留 `NODE_OPTIONS`」（原值是空的，或清洗后
/// 什么都不剩）。其余情况返回应当写入子进程的值。
pub fn sanitize(raw: &str) -> Option<String> {
    if raw.trim().is_empty() {
        return None;
    }
    let entries = classify(&split(raw));
    // 一个都没坏就原样返回：连引号、分隔符都不改写，别动用户自己写的形式。
    let broken = entries.iter().any(|entry| match entry {
        Entry::Preload(preload) => !Path::new(&preload.path).is_file(),
        Entry::Plain(_) => false,
    });
    if !broken {
        return Some(raw.to_string());
    }
    let mut kept: Vec<String> = Vec::new();
    for entry in &entries {
        match entry {
            Entry::Plain(token) => kept.push(token.clone()),
            Entry::Preload(preload) if Path::new(&preload.path).is_file() => {
                kept.push(format!("{}={}", preload.flag, quote_value(&preload.path)));
            }
            // 指向不存在文件的预加载：丢掉它。留着就是让子进程**必崩**。
            Entry::Preload(_) => {}
        }
    }
    let joined = kept.join(" ");
    (!joined.trim().is_empty()).then_some(joined)
}

/// 把清洗后的 `NODE_OPTIONS` 写进子进程命令。
///
/// 父进程本来就没有这个变量时**什么都不做**（保持继承语义）；有但完全正常时
/// 也什么都不做（一个字节都不改）；只有真的剔掉了坏预加载、或整条变量作废
/// 时才显式覆盖／删除。
pub fn apply_to(cmd: &mut std::process::Command) {
    let Ok(raw) = std::env::var("NODE_OPTIONS") else {
        return;
    };
    match sanitize(&raw) {
        Some(value) if value != raw => {
            cmd.env("NODE_OPTIONS", value);
        }
        Some(_) => {}
        None => {
            cmd.env_remove("NODE_OPTIONS");
        }
    }
}

/// 按 Node 自己的规则拆 `NODE_OPTIONS`：空白分隔，引号里的片段算整体。
///
/// Node 的解析器是类 shell 的：引号被剥除，`--require="a b.js"` 因此是**一个**
/// token（`--require=a b.js`），路径里的空格不会把它切成两半。按裸空白切会
/// 把路径截断成 `a`，把一个存在的文件误判成"不存在"——那种误判会让壳悄悄
/// 丢掉一个用户真正需要的预加载。
fn split(value: &str) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut quote: Option<char> = None;
    for ch in value.chars() {
        match quote {
            Some(open) if ch == open => quote = None,
            Some(_) => current.push(ch),
            None if ch == '"' || ch == '\'' => {
                quote = Some(ch);
                started = true;
            }
            None if ch.is_whitespace() => {
                if started {
                    tokens.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            None => {
                current.push(ch);
                started = true;
            }
        }
    }
    if started {
        tokens.push(current);
    }
    tokens
}

/// 把 token 序列分成普通选项与预加载项。
///
/// 两种写法都要认：`--require=<path>`（等号连写）与 `--require <path>`
/// （空格分隔）。后者必须连同它的路径 token 一起消费掉——只丢掉路径而留下
/// 裸的 `--require`，Node 会把**下一个**选项当成路径吃掉。
fn classify(tokens: &[String]) -> Vec<Entry> {
    let mut entries: Vec<Entry> = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        let token = &tokens[index];
        if let Some((flag, path)) = token.split_once('=') {
            if PRELOAD_FLAGS.contains(&flag) {
                entries.push(Entry::Preload(Preload {
                    flag: flag.to_string(),
                    path: path.to_string(),
                }));
                index += 1;
                continue;
            }
        }
        if PRELOAD_FLAGS.contains(&token.as_str()) {
            if let Some(path) = tokens.get(index + 1) {
                entries.push(Entry::Preload(Preload {
                    flag: token.clone(),
                    path: path.clone(),
                }));
                index += 2;
                continue;
            }
        }
        entries.push(Entry::Plain(token.clone()));
        index += 1;
    }
    entries
}

/// 路径含空白时加引号——`NODE_OPTIONS` 的分词是类 shell 的，不加引号的路径
/// 会被空格切成两半（Node 拿到的是截断后的那一半，然后报"文件不存在"）。
fn quote_value(value: &str) -> String {
    if value.contains(char::is_whitespace) {
        format!("\"{value}\"")
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(tag: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "dsh-node-options-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::write(&path, b"// preload stub").expect("写测试用预加载文件");
        path
    }

    fn missing_file(tag: &str) -> String {
        std::env::temp_dir()
            .join(format!(
                "dsh-node-options-missing-{tag}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ))
            .display()
            .to_string()
    }

    /// 2026-10-20 的真实形态：用户级 `NODE_OPTIONS` 指向一个已被删除的
    /// 预加载文件。清洗后必须**整条作废**——留着它就等于让内核必崩。
    #[test]
    fn a_preload_pointing_at_a_deleted_file_is_removed() {
        let raw = format!("--require=\"{}\"", missing_file("deleted"));
        assert_eq!(sanitize(&raw), None);
    }

    /// 只丢坏的，好的必须留着：用户的转译器 / CA 注入不能被壳顺手删掉。
    #[test]
    fn a_valid_preload_survives_a_broken_neighbour() {
        let good = temp_file("good");
        let raw = format!(
            "--require=\"{}\" --require=\"{}\" --max-old-space-size=4096",
            good.display(),
            missing_file("bad")
        );
        let cleaned = sanitize(&raw).expect("清洗后仍有有效预加载");
        assert!(
            cleaned.contains(&good.display().to_string()),
            "有效预加载必须留着"
        );
        assert!(!cleaned.contains("missing"), "坏预加载必须被丢掉");
        assert!(
            cleaned.contains("--max-old-space-size=4096"),
            "与预加载无关的选项必须原样保留：{cleaned}"
        );
    }

    /// 变量完全正常时**一个字节都不改**——用户的写法（含引号）原样继承。
    #[test]
    fn a_healthy_variable_is_returned_untouched() {
        let good = temp_file("healthy");
        let raw = format!("--require=\"{}\"", good.display());
        assert_eq!(sanitize(&raw), Some(raw.clone()));
    }

    /// 路径里有空格时不能被截断——截断后 Node 拿到的是半个路径，报的仍是
    /// "文件不存在"，而文件其实好好地在那儿。
    #[test]
    fn a_path_with_spaces_is_not_truncated() {
        let dir = std::env::temp_dir().join("dsh node options dir");
        std::fs::create_dir_all(&dir).expect("建带空格的目录");
        let good = dir.join("preload.cjs");
        std::fs::write(&good, b"// stub").expect("写预加载文件");
        let raw = format!("--require=\"{}\"", good.display());
        // 文件存在 → 整条原样返回，不该被当成"缺失"。
        assert_eq!(sanitize(&raw), Some(raw.clone()));
        // 掺一个坏项后重建：路径必须仍是一个整体（带引号）。
        let mixed = format!("{raw} --require=\"{}\"", missing_file("spaced"));
        let cleaned = sanitize(&mixed).expect("清洗后仍有有效预加载");
        assert!(
            cleaned.contains(&format!("\"{}\"", good.display())),
            "带空格的路径重建后必须仍被引号包住：{cleaned}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 空格分隔的写法（`--require <path>`）也要认，且必须连路径一起处理。
    #[test]
    fn the_space_separated_form_is_understood() {
        let good = temp_file("space-form");
        let raw = format!("--require {} --max-old-space-size=4096", good.display());
        let cleaned = sanitize(&raw).expect("有效预加载应原样保留");
        assert_eq!(cleaned, raw);
        // 路径缺失时整条作废。
        let broken = format!("--require {}", missing_file("space-form-broken"));
        assert_eq!(sanitize(&broken), None);
    }

    /// 与预加载无关的选项永远不会被删——`NODE_OPTIONS` 还能装别的东西。
    #[test]
    fn ordinary_options_are_never_dropped() {
        let raw = "--max-old-space-size=4096 --experimental-strip-types";
        assert_eq!(sanitize(raw), Some(raw.to_string()));
    }

    /// 空变量等于没有变量：交给调用方的是"删掉它"，而不是塞一个空串。
    #[test]
    fn an_empty_variable_means_remove_it() {
        assert_eq!(sanitize(""), None);
        assert_eq!(sanitize("   "), None);
    }
}
