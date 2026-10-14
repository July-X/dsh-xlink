//! 一条记忆 / 一条候选的**输入规范化**：去空白、限长、范围归一、关键词清理。
//!
//! 为什么独立成文件：面板保存（[`crate::memory::store::save`]）与模型提候选
//! （`submit_candidate`）是两条写入路径，而「什么算一条合法的记忆」必须两边一致。
//! 这份判断原先各抄一份——改一边漏一边的症状是「面板里存得进去、模型存不进去」，
//! 而那正是用户与模型对同一条约定给出两种结果的时候。
//!
//! 它是**纯函数**（唯一碰盘的是 [`normalize_path`] 的 realpath），与 store 的落盘
//! 事务无关，因此能脱离文件系统单测；store 反过来正贴着 665 行的反棘轮预算。

use crate::memory::store::{MAX_BODY_BYTES, MAX_TITLE_BYTES, SCOPE_GENERAL, SCOPE_PROJECT};
use crate::memory::workspaces::normalize_path;
use crate::shell::error::AppError;

/// 规范化之后的一条记忆/候选。
///
/// 两个来源共用这一个形状，差别只在验证范围的默认值（面板默认「已验证」——用户自己
/// 写下的；候选默认「未验证」——模型提的还没人看过）与「通用范围」不接受项目坐标。
#[derive(Debug)]
pub(crate) struct Clean {
    pub title: String,
    pub body: String,
    pub scope: String,
    pub workspace_id: String,
    pub workspace_path: String,
    pub keywords: Vec<String>,
    pub verification: String,
}

/// 两条写入路径的输入共用的那七个字段。`From` 两个实现让调用点只剩一行。
pub(crate) struct Raw<'a> {
    pub title: &'a str,
    pub body: &'a str,
    pub scope: &'a str,
    pub workspace_id: &'a str,
    pub workspace_path: &'a str,
    pub keywords: &'a [String],
    pub verification: &'a str,
}

/// 校验并规范化一条记忆/候选。
///
/// `default_verification` 是这条来源的验证范围默认值，`subject` 只进错误文案
/// （「记忆」/「候选」）——让用户知道自己操作的是哪一个列表里的那一条。
pub(crate) fn clean(
    raw: Raw<'_>,
    default_verification: &str,
    subject: &str,
) -> Result<Clean, AppError> {
    let title = raw.title.trim().to_string();
    let body = raw.body.trim().to_string();
    if title.is_empty() && body.is_empty() {
        return Err(AppError::Memory(match subject {
            "候选" => "候选内容为空，没有可提交的东西。".to_string(),
            _ => "标题和正文都是空的，没有可保存的内容。至少写一句。".to_string(),
        }));
    }
    if body.len() > MAX_BODY_BYTES {
        return Err(AppError::Memory(format!(
            "{subject}正文 {} 字节，超过单条上限 {MAX_BODY_BYTES} 字节。请拆成几条更短的。",
            body.len()
        )));
    }
    // **正文限了而标题不限是一个洞**：注入时表头里装着完整标题，标题一长
    // `query::render_context` 的正文预算就饱和到 0、整条被跳过，症状是
    // 「检索里排第一的那条就是不出现在注入里」。
    if title.len() > MAX_TITLE_BYTES {
        return Err(AppError::Memory(format!(
            "标题 {} 字节，超过上限 {MAX_TITLE_BYTES} 字节。标题要一句话说得清，正文才是展开的地方。",
            title.len()
        )));
    }
    // 范围只有两个取值：写别的字面量一律当项目处理，而不是新建第三种。
    let scope = if raw.scope == SCOPE_GENERAL {
        SCOPE_GENERAL
    } else {
        SCOPE_PROJECT
    };
    if scope == SCOPE_PROJECT
        && raw.workspace_id.trim().is_empty()
        && raw.workspace_path.trim().is_empty()
    {
        return Err(AppError::Memory(
            "项目记忆需要绑定一个已登记的工作区；没有工作区时请改存「通用偏好」。".to_string(),
        ));
    }
    let keywords: Vec<String> = raw
        .keywords
        .iter()
        .map(|keyword| keyword.trim().to_string())
        .filter(|keyword| !keyword.is_empty())
        .collect();
    // **关键词也要限额**（独立评审第 18 条）：插件那侧有 `MAX_KEYWORDS` / `MAX_KEYWORD_BYTES`
    // （`extract.js`），壳这边原先一个字都不管——于是「纵深」只有一层，而且关键词还**不进**
    // 整库体积闸（`doc_bytes` 只数标题与正文）。今天没有第二条能触达的路径，属纵深缺口，
    // 但这条闸正是防「以后多了一条路径」的。数值与插件逐字对齐，改一边要两边一起改。
    if keywords.len() > MAX_KEYWORDS {
        return Err(AppError::Memory(format!(
            "{subject}的关键词 {} 条，超过上限 {MAX_KEYWORDS} 条。关键词是检索用的索引，不是正文。",
            keywords.len()
        )));
    }
    if let Some(long) = keywords
        .iter()
        .find(|keyword| keyword.len() > MAX_KEYWORD_BYTES)
    {
        return Err(AppError::Memory(format!(
            "关键词「{}」{} 字节，超过单条上限 {MAX_KEYWORD_BYTES} 字节。",
            long,
            long.len()
        )));
    }
    let (workspace_id, workspace_path) = if scope == SCOPE_GENERAL {
        (String::new(), String::new())
    } else {
        (
            raw.workspace_id.trim().to_string(),
            // 与内核那种 realpath 归一：候选的路径将来要当匹配键用，
            // 不归一的话「带符号链接的手填路径」永远匹配不上。
            normalize_path(raw.workspace_path),
        )
    };
    let verification = if raw.verification.trim().is_empty() {
        default_verification.to_string()
    } else {
        raw.verification.trim().to_string()
    };
    Ok(Clean {
        title,
        body,
        scope: scope.to_string(),
        workspace_id,
        workspace_path,
        keywords,
        verification,
    })
}

impl<'a> From<&'a super::store::SaveInput> for Raw<'a> {
    fn from(input: &'a super::store::SaveInput) -> Self {
        Self {
            title: &input.title,
            body: &input.body,
            scope: &input.scope,
            workspace_id: &input.workspace_id,
            workspace_path: &input.workspace_path,
            keywords: &input.keywords,
            verification: &input.verification,
        }
    }
}

impl<'a> From<&'a super::store::CandidateInput> for Raw<'a> {
    fn from(input: &'a super::store::CandidateInput) -> Self {
        Self {
            title: &input.title,
            body: &input.body,
            scope: &input.scope,
            workspace_id: &input.workspace_id,
            workspace_path: &input.workspace_path,
            keywords: &input.keywords,
            verification: &input.verification,
        }
    }
}

/// 整库正文总量的上限。
///
/// 记录数上限（1000）× 单条正文上限（8 KiB）允许一份 8 MB 的文档，而**每回合检索
/// 都要把整份 JSON 读出来解析一遍**（`query::retrieve`），模型每按 id 读一次也是
/// 一遍。设计稿 §4.1 假设的规模是「几百 KB 量级」，1 MiB 给了它三倍余量，同时把
/// 最坏情况压回可接受的量级。
pub(crate) const MAX_DOC_BYTES: usize = 1024 * 1024;

/// 一份文档里标题与正文的总字节数（候选也算：它们同样要被解析）。
/// 关键词的条数与单条字节上限。**与插件的 `extract.js` 逐字对齐**（`MAX_KEYWORDS` /
/// `MAX_KEYWORD_BYTES`）：两边不一致时，合法的一侧会被另一侧拒，而用户看到的是一句
/// 「关键词超过上限」——他会以为是自己写多了。
pub(crate) const MAX_KEYWORDS: usize = 12;
pub(crate) const MAX_KEYWORD_BYTES: usize = 64;

pub(crate) fn doc_bytes(doc: &super::store::MemoryDoc) -> usize {
    // **关键词也算进整库体积**（评审第 18 条）：它们会随记录一起落盘、一起被读出来，
    // 只数标题与正文等于给「多写关键词」留了一条不计量地撑大文档的路。
    let pair = |title: &str, body: &str, keywords: &[String]| {
        title.len() + body.len() + keywords.iter().map(String::len).sum::<usize>()
    };
    doc.memories
        .iter()
        .map(|row| pair(&row.title, &row.body, &row.keywords))
        .sum::<usize>()
        + doc
            .candidates
            .iter()
            .map(|row| pair(&row.title, &row.body, &row.keywords))
            .sum::<usize>()
}

/// 写入**让库变大**且越过上限时拒绝。
///
/// 缩小或不变的写入永远放行：库已经超标时（上限被调小、或老版本写下的），删一条
/// 是唯一的出路——把删除一起挡掉等于让用户卡在一个只能读的状态里。
pub(crate) fn ensure_doc_growth(before: usize, after: usize) -> Result<(), AppError> {
    if after <= MAX_DOC_BYTES || after <= before {
        return Ok(());
    }
    Err(AppError::Memory(format!(
        "记忆总量会超过上限 {} KiB（约 {} KiB）：请先删掉一些不再需要的条目。",
        MAX_DOC_BYTES / 1024,
        after.div_ceil(1024)
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::store::testkit::TempHome;
    use crate::memory::store::{VERIFICATION_UNVERIFIED, VERIFICATION_VERIFIED};

    fn raw<'a>(body: &'a str, keywords: &'a [String]) -> Raw<'a> {
        Raw {
            title: "  构建命令  ",
            body,
            scope: "project",
            workspace_id: " ws-a ",
            workspace_path: "/tmp/ws-a",
            keywords,
            verification: "",
        }
    }

    /// 两条写入路径走同一份判断：面板的默认是「已验证」，候选是「未验证」。
    #[test]
    fn both_sources_get_the_same_validation_only_the_default_differs() {
        let _home = TempHome::new("input");
        let manual = clean(raw("正文", &[]), VERIFICATION_VERIFIED, "记忆").expect("clean");
        assert_eq!(manual.verification, VERIFICATION_VERIFIED);
        let candidate = clean(raw("正文", &[]), VERIFICATION_UNVERIFIED, "候选").expect("clean");
        assert_eq!(candidate.verification, VERIFICATION_UNVERIFIED);
    }

    /// 整库体积：变大且越过上限才拒；缩小或不变的写入永远放行。
    #[test]
    fn growth_over_the_cap_is_refused_but_shrinking_never_is() {
        assert!(
            ensure_doc_growth(0, MAX_DOC_BYTES).is_ok(),
            "刚好到顶不算超"
        );
        let over = ensure_doc_growth(MAX_DOC_BYTES, MAX_DOC_BYTES + 1).unwrap_err();
        assert!(
            over.to_string().contains("总量"),
            "错误要说清是总量超了：{over}"
        );
        assert!(
            ensure_doc_growth(MAX_DOC_BYTES + 1, MAX_DOC_BYTES + 2).is_err(),
            "已经超标时继续长大要拒"
        );
        assert!(
            ensure_doc_growth(MAX_DOC_BYTES + 1, MAX_DOC_BYTES).is_ok(),
            "缩小永远放行——库已经超标时，删一条是唯一的出路"
        );
        assert!(
            ensure_doc_growth(MAX_DOC_BYTES + 1, MAX_DOC_BYTES + 1).is_ok(),
            "不变（例如改开关、记一次使用）也要放行"
        );
    }

    /// **关键词也要限额**（独立评审第 18 条）：插件那侧有上限，壳这边原先一个字都不管——
    /// 纵深只有一层，而且关键词还不进整库体积闸。数值与插件对齐，判据也钉住这一点。
    #[test]
    fn too_many_or_too_long_keywords_are_refused() {
        let _home = TempHome::new("input-keywords");
        let many: Vec<String> = (0..MAX_KEYWORDS + 1)
            .map(|index| format!("k{index}"))
            .collect();
        let too_many = clean(raw("正文", &many), VERIFICATION_VERIFIED, "记忆").unwrap_err();
        assert!(
            too_many.to_string().contains("超过上限 12 条"),
            "{too_many}"
        );

        let long = vec!["甲".repeat(MAX_KEYWORD_BYTES)];
        let too_long = clean(raw("正文", &long), VERIFICATION_VERIFIED, "记忆").unwrap_err();
        assert!(too_long.to_string().contains("超过单条上限"), "{too_long}");

        // 正好到顶的两侧都该放行（上限是「不超过」）。
        let exact: Vec<String> = (0..MAX_KEYWORDS).map(|index| format!("k{index}")).collect();
        clean(raw("正文", &exact), VERIFICATION_VERIFIED, "记忆").expect("正好 12 条该放行");
    }

    /// 关键词要算进**整库体积**：它们随记录一起落盘、一起被读出来，不计量就等于留了一条
    /// 不计量地撑大文档的路。
    #[test]
    fn keywords_count_towards_the_document_size() {
        let _home = TempHome::new("input-doc-bytes");
        let none = clean(raw("正文", &[]), VERIFICATION_VERIFIED, "记忆").expect("clean");
        let some = clean(
            raw("正文", &["构建".to_string(), "检查".to_string()]),
            VERIFICATION_VERIFIED,
            "记忆",
        )
        .expect("clean");
        let with = doc_bytes(&doc_with(&some));
        let without = doc_bytes(&doc_with(&none));
        assert_eq!(
            with - without,
            "构建".len() + "检查".len(),
            "关键词的字节数要进整库体积"
        );
    }

    /// 造一份只含这一条候选的文档（`doc_bytes` 的判据用）。
    fn doc_with(clean: &Clean) -> super::super::store::MemoryDoc {
        let mut doc = super::super::store::MemoryDoc::default();
        doc.candidates.push(super::super::store::MemoryCandidate {
            scope: clean.scope.clone(),
            title: clean.title.clone(),
            body: clean.body.clone(),
            keywords: clean.keywords.clone(),
            ..Default::default()
        });
        doc
    }

    #[test]
    fn trims_scope_normalizes_and_clears_workspace_for_general() {
        let _home = TempHome::new("input-normalize");
        let keywords = vec![" 构建 ".to_string(), "   ".to_string()];
        let project =
            clean(raw("  正文  ", &keywords), VERIFICATION_VERIFIED, "记忆").expect("clean");
        assert_eq!(project.title, "构建命令");
        assert_eq!(project.body, "正文");
        assert_eq!(project.scope, SCOPE_PROJECT);
        assert_eq!(project.workspace_id, "ws-a");
        assert_eq!(project.keywords, vec!["构建"], "空关键词要丢掉");

        let general = clean(
            Raw {
                scope: "general",
                workspace_id: "ws-a",
                workspace_path: "/tmp/ws-a",
                ..raw("正文", &[])
            },
            VERIFICATION_VERIFIED,
            "记忆",
        )
        .expect("clean");
        assert!(general.workspace_id.is_empty(), "通用偏好不绑项目");
        assert!(general.workspace_path.is_empty());
    }

    /// 范围写别的字面量一律当项目处理，不新建第三种范围。
    #[test]
    fn an_unknown_scope_falls_back_to_project_and_then_needs_a_workspace() {
        let _home = TempHome::new("input-scope");
        let error = clean(
            Raw {
                scope: "whatever",
                workspace_id: "",
                workspace_path: "",
                ..raw("正文", &[])
            },
            VERIFICATION_VERIFIED,
            "记忆",
        )
        .expect_err("没有坐标的项目记忆要被拒");
        assert!(error.to_string().contains("工作区"), "{error}");
    }

    #[test]
    fn empty_content_and_over_long_fields_are_refused() {
        let _home = TempHome::new("input-limits");
        let empty = clean(
            Raw {
                title: "   ",
                body: "  ",
                ..raw("占位", &[])
            },
            VERIFICATION_VERIFIED,
            "候选",
        )
        .expect_err("空内容");
        assert!(empty.to_string().contains("候选内容为空"), "{empty}");

        let long_body = "字".repeat(MAX_BODY_BYTES + 3);
        let body_error =
            clean(raw(&long_body, &[]), VERIFICATION_VERIFIED, "记忆").expect_err("正文超限");
        assert!(body_error.to_string().contains("正文"), "{body_error}");

        let long_title = Raw {
            title: &"标".repeat(MAX_TITLE_BYTES + 3),
            ..raw("正文", &[])
        };
        let title_error = clean(long_title, VERIFICATION_VERIFIED, "记忆").expect_err("标题超限");
        assert!(title_error.to_string().contains("标题"), "{title_error}");

        // 显式给的验证范围照用，不被默认值盖掉。
        let explicit = clean(
            Raw {
                verification: "stale",
                ..raw("正文", &[])
            },
            VERIFICATION_VERIFIED,
            "记忆",
        )
        .expect("clean");
        assert_eq!(explicit.verification, "stale");
    }
}
