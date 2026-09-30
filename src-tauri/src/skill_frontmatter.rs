//! 技能 frontmatter 的解析：YAML 的一个**有意受限**的子集。
//!
//! 独立成文件的两条理由：① `skills.rs` 只剩 4 行余量且受反棘轮「只许下调」，
//! 块标量那套状态机放不进去；② frontmatter 有**两个**消费者（包扫描与
//! `skill_conflict` 的版本证据），判据一旦有两份实现就会分叉——第一版
//! `skill_conflict` 自己写了一个只认 `metadata.version` 的小解析器，那正是
//! 分叉的种子。
//!
//! 范围刻意窄：只解析顶层标量 + 顶层块标量 + `metadata:` 下的 `version`。
//! 安装一个外壳自身无法校验的技能会让用户在不知情的情况下看到一个不可见
//! 的技能，因此**解析不了就拒绝该候选**（见 `skills::parse_skill_markdown`），
//! 而不是信任它——这条纪律要求这个解析器宁可少认，不可乱认。
//!
//! 支持的块标量写法：YAML 的 `|`（literal）与 `>`（folded），各带 chomping
//! 指示符 `-` / `+`。2026-09-30 修的就是这里：块标量的值过去被读成字面量
//! `"|"`，于是 `store.json` 里存下 `"description": "|"`，面板 tooltip 显示的
//! 也是 `"|"` 而不是那几行说明。**bug 的隐蔽之处在于它几乎不报错**——`"|"` 是
//! 非空字符串，所以技能照样被接受、照样能启用，只是说明全丢了。

/// 解析结果。字段都是 `Option`：解析 frontmatter 与「这份 frontmatter 够不够
/// 格装成一个技能」是两件事，后者由调用方按需组合。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Frontmatter {
    /// frontmatter `name`（kebab-case 技能名）。
    pub name: Option<String>,
    /// frontmatter `description`。
    pub description: Option<String>,
    /// `metadata.version`，去掉 `v` 前缀与引号后的归一化形式。
    pub version: Option<String>,
}

/// 解析开头的 YAML frontmatter。没有 frontmatter 块时返回 `None`；块存在但
/// 里面什么都没有则返回全 `None` 的结果——「读到了」与「认出了」要分开。
pub fn parse(text: &str) -> Option<Frontmatter> {
    let body = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines = body.lines();
    if lines.next()?.trim_end() != "---" {
        return None;
    }
    let mut out = Frontmatter::default();
    let mut pending: Option<Pending> = None;
    for line in lines {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line == "---" || line == "..." {
            break;
        }
        let indented = line.starts_with(' ') || line.starts_with('\t');
        if indented {
            if let Some(p) = pending.as_mut() {
                p.push(line);
            }
            continue;
        }
        // 空行属于块标量的内容（literal 块里空行是有意义的），不属于终止。
        if line.trim().is_empty() {
            if let Some(p) = pending.as_mut() {
                if p.is_block() {
                    p.push(line);
                }
            }
            continue;
        }
        if let Some(p) = pending.take() {
            p.commit(&mut out);
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        if let Some(style) = BlockStyle::of(value) {
            pending = Some(Pending::block(key, style));
        } else if value.is_empty() {
            // 空值：可能是嵌套映射（`metadata:`）或列表（`keywords:`），都先
            // 收着，认出来哪个算哪个。
            pending = Some(Pending::nested(key));
        } else {
            put(&mut out, key, strip_quotes(value));
        }
    }
    if let Some(p) = pending.take() {
        p.commit(&mut out);
    }
    Some(out)
}

/// 只要版本号（`skill_conflict` 的版本证据用）。读不出来返回 `None`，调用方
/// 据此把文案退回通用说法——**判据不依赖它**，所以解析失败不该影响任何行为。
pub fn version(text: &str) -> Option<String> {
    parse(text)?.version
}

/// 块标量的折叠方式与 chomping 指示符。
#[derive(Clone, Copy, PartialEq, Eq)]
struct BlockStyle {
    /// `true` = literal（`|`，保留换行），`false` = folded（`>`，折成空格）。
    literal: bool,
    /// `true` = 保留块尾全部换行（`+`），`false` = 去掉（`-` 或默认 clip）。
    keep_trailing: bool,
}

impl BlockStyle {
    /// 只认「整个值就是块标量指示符」的情况。`description: |` 后面跟着缩进
    /// 的正文；`| extra` 不是合法 YAML，当普通标量处理（值就是 `"| extra"`）。
    fn of(value: &str) -> Option<Self> {
        if value.is_empty() {
            return None;
        }
        let (head, tail) = value.split_at(1);
        let literal = match head {
            "|" => true,
            ">" => false,
            _ => return None,
        };
        // 尾随只允许 chomping 指示符与可选的显式缩进位数。
        let keep_trailing = tail.starts_with('+');
        let rest = tail.trim_start_matches(['-', '+']);
        if !rest.is_empty() && !rest.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        Some(Self {
            literal,
            keep_trailing,
        })
    }
}

/// 正在收集的一段：`key` 的值要么是块标量的正文，要么是嵌套映射的若干行。
enum Pending {
    Block {
        key: String,
        style: BlockStyle,
        lines: Vec<String>,
    },
    Nested {
        key: String,
        lines: Vec<String>,
    },
}

impl Pending {
    fn block(key: &str, style: BlockStyle) -> Self {
        Pending::Block {
            key: key.to_string(),
            style,
            lines: Vec::new(),
        }
    }

    fn nested(key: &str) -> Self {
        Pending::Nested {
            key: key.to_string(),
            lines: Vec::new(),
        }
    }

    fn is_block(&self) -> bool {
        matches!(self, Pending::Block { .. })
    }

    fn push(&mut self, line: &str) {
        let lines = match self {
            Pending::Block { lines, .. } | Pending::Nested { lines, .. } => lines,
        };
        lines.push(line.to_string());
    }

    fn commit(self, out: &mut Frontmatter) {
        match self {
            Pending::Block { key, style, lines } => {
                let value = fold(&lines, style);
                put(out, &key, &value);
            }
            Pending::Nested { key, lines } => {
                // 只认 `metadata:` 下的 `version:`。顶层 `version:` 不是这个
                // 约定（frontmatter 顶层没有它），认错会把无关字段报成版本。
                if key != "metadata" {
                    return;
                }
                for line in lines {
                    let Some((k, v)) = line.trim().split_once(':') else {
                        continue;
                    };
                    if k.trim() == "version" {
                        out.version = out.version.clone().or_else(|| normalize_version(v));
                        return;
                    }
                }
            }
        }
    }
}

/// 把块标量的正文折成最终字符串。缩进以**第一条非空行**为准（YAML 的规则），
/// 因为块的第一行可以是空行。
fn fold(lines: &[String], style: BlockStyle) -> String {
    let indent = lines
        .iter()
        .find(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .unwrap_or(0);
    let body: Vec<&str> = lines
        .iter()
        .map(|l| {
            // 只剥掉**这一行真的有的**那部分缩进。按块缩进硬切会在缩进更少
            // 的行上从半个词中间切开（`  back` 变成 `ck`）；少认可以，错切不
            // 可以。前导空白全是 ASCII，cut 必然落在字符边界上。
            let ws = l.len() - l.trim_start_matches([' ', '\t']).len();
            &l[indent.min(ws)..]
        })
        .collect();
    let mut text = String::new();
    if style.literal {
        text = body.join("\n");
    } else {
        // folded：一段连续的非空行折成空格，空行本身是一个换行。
        let mut para = String::new();
        for l in &body {
            if l.trim().is_empty() {
                if !para.is_empty() {
                    text.push_str(para.trim_end());
                    para.clear();
                }
                text.push('\n');
            } else {
                if !para.is_empty() {
                    para.push(' ');
                }
                para.push_str(l);
            }
        }
        if !para.is_empty() {
            text.push_str(&para);
        }
    }
    // 块开头的空行一律去掉：`description: |` 后面空一行再写正文是极常见的
    // 排版，而它对读者毫无意义。块**中间**的空行保留——folded 块里那是段落分隔。
    let text = text.trim_start_matches('\n');
    let text = if style.keep_trailing {
        text.to_string()
    } else {
        text.trim_end().to_string()
    };
    text
}

/// 赋值。**同名只取第一个**——块状 frontmatter 里后写的同名键不覆盖先写的，
/// 与改动前的行为一致。
fn put(out: &mut Frontmatter, key: &str, value: &str) {
    match key {
        "name" if out.name.is_none() => out.name = Some(value.to_string()),
        "description" if out.description.is_none() => out.description = Some(value.to_string()),
        _ => {}
    }
}

fn strip_quotes(value: &str) -> &str {
    for q in ['"', '\''] {
        if value.starts_with(q) && value.ends_with(q) && value.len() >= 2 {
            return &value[1..value.len() - 1];
        }
    }
    value
}

/// 去掉引号与前导 `v`，让 `v3.1.0` 与 `3.1.0` 能被比成同一份。
pub fn normalize_version(raw: &str) -> Option<String> {
    let value = strip_quotes(raw.trim());
    let value = value.strip_prefix('v').unwrap_or(value);
    (!value.is_empty()).then(|| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn desc(text: &str) -> Option<String> {
        parse(text)?.description
    }

    /// 本次修的那一行：块标量的值过去是字面量 `"|"`。
    #[test]
    fn literal_block_scalar_is_the_block_not_a_pipe_character() {
        let text = "---\nname: n\ndescription: |\n  line one\n  line two\n---\n\nbody\n";
        assert_eq!(desc(text).as_deref(), Some("line one\nline two"));
        let parsed = parse(text).unwrap();
        assert_eq!(parsed.name.as_deref(), Some("n"));
        // 块标量的正文行不是键：正文里的 `version:` 不得被当成 metadata 版本。
        assert_eq!(parsed.version, None);
    }

    /// folded 块：换行折成空格，段落之间仍是一个换行。
    #[test]
    fn folded_block_scalar_folds_newlines_into_spaces() {
        let text = "---\nname: n\ndescription: >\n  a b\n  c d\n\n  next para\n---\n";
        assert_eq!(desc(text).as_deref(), Some("a b c d\nnext para"));
    }

    /// chomping 指示符：`-` 与默认都去掉块尾换行，`+` 保留。
    #[test]
    fn chomping_indicators_are_honoured() {
        let keep = "---\nname: n\ndescription: |+\n  keep\n\n---\n";
        assert!(desc(keep).unwrap().ends_with('\n'), "|+ 要保留块尾换行");
        let strip = "---\nname: n\ndescription: |-\n  strip\n---\n";
        assert_eq!(desc(strip).as_deref(), Some("strip"));
    }

    /// 缩进以第一条非空行为准：块的第一行可以是空行。缩进比块缩进**更少**的
    /// 行（严格 YAML 里那已经结束块了）不许被从半个词中间切开。
    #[test]
    fn block_indent_comes_from_the_first_non_blank_line() {
        let text = "---\nname: n\ndescription: |\n\n    indented body\n  back to two\n---\n";
        assert_eq!(desc(text).as_deref(), Some("indented body\nback to two"));
    }

    /// `metadata.version` 要认，且归一化掉引号与 `v` 前缀。
    #[test]
    fn metadata_version_is_read_and_normalized() {
        let text = "---\nname: n\nversion: 8.8.8\nmetadata:\n  version: \"v3.1.0\"\n---\n";
        assert_eq!(version(text).as_deref(), Some("3.1.0"));
    }

    /// 顶层 `version:` 不是 `metadata.version`：认错会把无关字段报成版本。
    #[test]
    fn top_level_version_is_not_the_metadata_version() {
        assert_eq!(version("---\nname: n\nversion: 8.8.8\n---\n"), None);
    }

    /// 目录包也走同一套解析（`version()` 的调用方传的是文件全文）。
    #[test]
    fn no_frontmatter_and_empty_blocks_are_distinguished() {
        assert_eq!(parse("no frontmatter"), None);
        // 有块但没有认识的键：读到了，认出的字段全空。
        let empty = parse("---\nfoo: bar\n---\n").unwrap();
        assert_eq!(empty, Frontmatter::default());
    }

    /// 改动前就支持的写法一个都不能退化：引号内的冒号、单行标量、未知键、
    /// 列表、以及「同名只取第一个」。
    #[test]
    fn pre_existing_behaviour_is_preserved() {
        let text =
            "---\nname: my-skill\ndescription: \"Does things: well\"\nlicense: MIT\n---\nBody";
        let parsed = parse(text).unwrap();
        assert_eq!(parsed.name.as_deref(), Some("my-skill"));
        assert_eq!(parsed.description.as_deref(), Some("Does things: well"));
        let dup = "---\nname: a\nname: b\ndescription: one\ndescription: two\n---\n";
        let parsed = parse(dup).unwrap();
        assert_eq!(parsed.name.as_deref(), Some("a"));
        assert_eq!(parsed.description.as_deref(), Some("one"));
        let list = "---\nname: n\nkeywords:\n  - one\n  - two\ndescription: d\n---\n";
        assert_eq!(desc(list).as_deref(), Some("d"));
    }

    /// BOM 与 CRLF：Windows 上写出来的 frontmatter 两种都常见。
    #[test]
    fn bom_and_crlf_are_tolerated() {
        let text = "\u{feff}---\r\nname: n\r\ndescription: |\r\n  a\r\n  b\r\n---\r\nbody\r\n";
        assert_eq!(desc(text).as_deref(), Some("a\nb"));
    }

    /// `| extra` 不是合法块标量写法，当普通标量处理——宁可少认，不可乱认。
    #[test]
    fn a_bogus_block_indicator_falls_back_to_a_plain_scalar() {
        let text = "---\nname: n\ndescription: | extra\n---\n";
        assert_eq!(desc(text).as_deref(), Some("| extra"));
    }
}
