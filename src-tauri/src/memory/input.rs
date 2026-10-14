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
