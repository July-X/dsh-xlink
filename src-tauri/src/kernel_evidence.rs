//! 证据判读：这一行到底在说什么。
//!
//! 从 [`crate::guard`] 里拆出来的独立职责——**认字**，不认人。
//!
//! `guard.rs` 回答「这次启动该怎么处置」，而本模块只回答一个更窄的问题：
//! 给定一行错误文本，它指向内核自己，还是指向某个第三方插件？两条判据各有
//! 一个必须钉死的反例：
//!
//! - **内核命名空间**（[`is_kernel_evidence`] 的第一半）：锚定 `@deepseek-ai/dsh`
//!   后接非字母数字边界。放行会让一个叫 `@scope/dsh-foo` 的社区插件仅凭子串被
//!   当成内核包；收紧又会漏掉 `@deepseek-ai/dsh-client-ui-theme` 这类随附模块。
//! - **内核自述的固定文案**（[`LOADER_PHRASES`] / [`SLOT_PHRASES`]）：这些字符串
//!   由内核的 loader 与渲染器写出，插件伪造不出来，因此**不受**多成员组合路由那条
//!   拒绝规则约束。多成员组合是若干包拼成的同一个脚本，按成员逐个匹配会把同批的
//!   旁观者写进隔离清单——所以命名空间那条规则对组合路由必须让路。
//!
//! 2026-09-29 实测（release 实例、内核 0.2.0-rc.1）：
//! `scope 'session-maybe' rendered without an installed adapter`。这条让归因从
//! 「前端 bundle 异常（未定位到包名）」——连带建议用户去停用第三方插件——变成了
//! 指向内核版本的启动顺序问题，而插件一个字都没被牵连。

/// 内核 client-module loader 在预打包 chunk 表缺条目时输出的固定文案。
///
/// 这些短语在不同内核版本间稳定且很少变动，因此判据是「命中其中任何一条」，
/// 而不是只认当前这份列表里的某一条。
pub const LOADER_PHRASES: [&str; 6] = [
    "client-modules",
    "build-time externals drift",
    "missed the module table",
    "platform seed word",
    "not a materialized module",
    "no registered package factory",
];

/// 内核渲染器（`@deepseek-ai/dsh-client-ui-renderer`）抛出的槽位装配不变量。
///
/// 单独成一个常量而不是并进 [`LOADER_PHRASES`]：两者都能绕过组合路由的拒绝规则，
/// 但来源不同（渲染器 vs loader），分开写才看得见各自覆盖了哪一类故障。全部取自
/// 内核的抛点文案。
///
/// 最典型的形态是启动顺序竞态：`renderRoot` 无条件地把应用包进
/// `ScopeProvider scope="session-maybe"`，而那个 scope 的 adapter 只由
/// `dsh-client-ui-session` 安装——它 inject 了 `sessions` / `slots` / `remote`
/// 三个服务，任一没到位就不激活。挂载抢在它前面时，整个工作台直接抛这一句。
pub const SLOT_PHRASES: [&str; 4] = [
    "rendered without an installed adapter",
    "renderSlot('root') before any 'root' registration",
    "rendered outside the root standard-source provider",
    "rendered outside its scope provider",
];

/// 行内内核 client-modules 组合路由的成员数。
///
/// 形态：`/plugins/??<包名>/client.js,<包名>/client.js&rev=<hash>`（见内核的
/// `dsh-client-modules`：`comboUrl`）。`None` 表示这一行里没有组合路由。
pub fn combo_route_members(line: &str) -> Option<usize> {
    const ROUTE: &str = "/plugins/??";
    let start = line.find(ROUTE)? + ROUTE.len();
    let rest = &line[start..];
    let end = rest
        .find(|c: char| c.is_whitespace() || matches!(c, '&' | '"' | '\'' | ')' | ']' | '<' | '>'))
        .unwrap_or(rest.len());
    let members = rest[..end]
        .split(',')
        .filter(|member| !member.is_empty())
        .count();
    (members > 0).then_some(members)
}

/// 这一行是否只是「多成员组合 bundle」的地址：一个脚本里同时打着多个包，里面出现
/// 任何包名都不能作为指向该包的证据。
pub fn is_ambiguous_combo_line(line: &str) -> bool {
    matches!(combo_route_members(line), Some(members) if members > 1)
}

/// `line` 是否引用了内核自己的包命名空间（`@deepseek-ai/dsh` 后接非字母数字边界）。
///
/// 不应匹配 `@deepseek-ai/dshfoo`——那是名字里碰巧含有 `dsh` 的社区包。
pub fn has_kernel_package_ref(line: &str) -> bool {
    const NEEDLE: &str = "@deepseek-ai/dsh";
    let mut start = 0;
    while let Some(idx) = line[start..].find(NEEDLE) {
        let after = start + idx + NEEDLE.len();
        // 手写一个「下一个字符（如果有）是否为非字母数字边界？」的判断——`Option::is_none_or`
        // 是 1.77 之后才有的，`Cargo.toml` 的 MSRV 门禁会拒绝在此调用该方法。
        let boundary_ok = match line[after..].chars().next() {
            None => true,
            Some(c) => !c.is_ascii_alphanumeric(),
        };
        if boundary_ok {
            return true;
        }
        start = after;
    }
    false
}

/// 这一行是否指向内核自己。
///
/// `is_error_line`（形态确认）是调用方的前置条件：这里只叠加「是否归内核管」。
/// 形如 `Error: EADDRINUSE` 的环境类失败不该落进来——那属于
/// [`crate::guard`] 的环境分支。
pub fn is_kernel_evidence(line: &str) -> bool {
    // 同 `guard.rs` 的插件锚定：Windows 上包路径是反斜杠形态，
    // `@deepseek-ai\dsh\lib\bin.js` 也必须能命中内核命名空间（P2-5）。
    let normalized = line.replace('\\', "/");
    if has_kernel_package_ref(&normalized) && !is_ambiguous_combo_line(&normalized) {
        return true;
    }
    LOADER_PHRASES.iter().any(|p| line.contains(p)) || SLOT_PHRASES.iter().any(|p| line.contains(p))
}

/// 健康证据里是否出现内核渲染器的槽位装配不变量。
///
/// 只看前端堆栈行与消息，与 `guard.rs` 的 `has_client_bundle_frames` 同一口径：
/// 内核日志里出现同样字样说的是另一回事（例如插件侧自己复述了这段文案）。
pub fn is_slot_failure(evidence: &str) -> bool {
    let stack_prefix = "前端堆栈：";
    evidence.lines().any(|line| {
        (line.contains(stack_prefix) || line.contains("工作台前端错误"))
            && SLOT_PHRASES.iter().any(|p| line.contains(p))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 多成员组合里的内核包名**不得**当成内核证据——那个脚本同时打着第三方插件的
    /// bundle（P2 级归因误报）。这条是本模块最容易在「放宽一点」时踩坏的。
    #[test]
    fn multi_member_combo_with_kernel_names_is_not_kernel_evidence() {
        let line =
            "Error: GET /plugins/??@deepseek-ai/dsh-web-app/client.js,ghost/client.js&rev=ab 404";
        assert!(!is_kernel_evidence(line));
        assert!(is_ambiguous_combo_line(line));
    }

    #[test]
    fn single_member_combo_with_kernel_name_is_kernel_evidence() {
        let line = "Error: GET /plugins/??@deepseek-ai/dsh-web-app/client.js&rev=ab 500";
        assert!(is_kernel_evidence(line));
    }

    #[test]
    fn loader_and_slot_phrases_are_recognised() {
        for phrase in LOADER_PHRASES {
            assert!(
                is_kernel_evidence(&format!("Error: client-modules: {phrase}")),
                "loader 短语漏了：{phrase}"
            );
        }
        assert!(is_kernel_evidence(
            "Error: scope 'session-maybe' rendered without an installed adapter"
        ));
        assert!(is_kernel_evidence(
            "Error: renderSlot('root') before any 'root' registration (boot order)"
        ));
    }

    #[test]
    fn community_package_named_dsh_is_not_kernel() {
        assert!(!is_kernel_evidence("Error: @scope/dsh-foo exploded"));
        assert!(has_kernel_package_ref(
            "@deepseek-ai/dsh-client-ui-theme/x.js"
        ));
        assert!(!has_kernel_package_ref("@deepseek-ai/dshfoo/x.js"));
    }

    #[test]
    fn windows_backslash_paths_still_count_as_kernel() {
        assert!(is_kernel_evidence(
            "Error: cannot find module C:\\app\\node_modules\\@deepseek-ai\\dsh\\lib\\bin.js"
        ));
    }

    #[test]
    fn slot_failure_only_reads_frontend_evidence_lines() {
        let evidence =
            "Error: 前端堆栈：Error: scope 'session-maybe' rendered without an installed adapter";
        assert!(is_slot_failure(evidence));
        // 同样的字样出现在内核日志行里不算——那是另一回事。
        assert!(!is_slot_failure(
            "Error: 内核日志：scope 'session-maybe' rendered without an installed adapter"
        ));
        assert!(!is_slot_failure(
            "Error: 前端堆栈：TypeError: x is not a function"
        ));
    }
}
