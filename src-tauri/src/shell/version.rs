//! 内核发布列表（`releases`）与社区插件更新检查（`plugins`）共用的版本比较函数。

use std::cmp::Ordering;

/// 判断一个版本号是否可以安全地当作**文件系统路径段**与 JSON 字符串使用。
///
/// 版本号来自远端（npm packument、GitHub release tag），会被拼进
/// `kernels/<version>` 目录名与内核 stub `package.json`。默认 registry 是第三方
/// 镜像（见 `registry.rs`），镜像被投毒或 CDN 被篡改时，形如
/// `..\..\..\Users\<user>\Startup\x` 的"版本号"会在 Windows 上越界创建目录并写入
/// 文件；含 `"` 的版本号还能闭合 stub JSON、注入任意字段（pnpm 随后会执行其中
/// 的 `preinstall` 脚本）。git tag 也允许 `/`（`dsh-v1.0/hotfix`），会在
/// `kernels/` 下建出嵌套目录，被 `list_installed` 误当成已安装版本。
///
/// 因此只接受 semver 形态的字符集，并显式拒绝 `.` / `..`。
pub fn is_valid_kernel_version(version: &str) -> bool {
    if version.is_empty() || version.len() > 128 || version == "." || version == ".." {
        return false;
    }
    let mut chars = version.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_alphanumeric() {
        return false;
    }
    version
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '+' | '-'))
}

/// 语义化版本比较（semver 2.0 的排序规则）。
///
/// 预发布段逐个标识符比：**纯数字的标识符低于**带字母的（`1.0.0-1 < 1.0.0-alpha`，
/// 这是 semver 的规定）；两边都是数字按数值比，两边都带字母按字典序比。
/// 数字段用「长度 + 字典序」比，因此超出 u64 的版本号也是对的。
///
/// 主体里不是数字的段（社区 tag 的 `head`、短 hash）被丢掉——那种东西不是版本号，
/// 于是它排在任何 semver 之后，与「它不是版本」的事实一致。
pub fn cmp_semver(a: &str, b: &str) -> Ordering {
    fn is_numeric(value: &str) -> bool {
        !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
    }
    /// 任意精度的数字比较：先比长度，再比字典序（没有前导零的十进制数）。
    fn numeric(a: &str, b: &str) -> Ordering {
        a.len().cmp(&b.len()).then_with(|| a.cmp(b))
    }
    fn core(value: &str) -> (Vec<&str>, &str) {
        let stripped = value.strip_prefix('v').unwrap_or(value);
        let head = stripped.split('+').next().unwrap_or(stripped);
        let (head, pre) = head
            .split_once('-')
            .map_or((head, ""), |(head, pre)| (head, pre));
        (
            head.split('.').filter(|part| is_numeric(part)).collect(),
            pre,
        )
    }
    fn pre_release(a: &str, b: &str) -> Ordering {
        match (a.is_empty(), b.is_empty()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            (false, false) => {
                let mut left = a.split('.');
                let mut right = b.split('.');
                loop {
                    match (left.next(), right.next()) {
                        (None, None) => return Ordering::Equal,
                        (None, Some(_)) => return Ordering::Less,
                        (Some(_), None) => return Ordering::Greater,
                        (Some(x), Some(y)) => {
                            let order = match (is_numeric(x), is_numeric(y)) {
                                (true, true) => numeric(x, y),
                                (true, false) => Ordering::Less,
                                (false, true) => Ordering::Greater,
                                (false, false) => x.cmp(y),
                            };
                            if order != Ordering::Equal {
                                return order;
                            }
                        }
                    }
                }
            }
        }
    }
    let (na, pa) = core(a);
    let (nb, pb) = core(b);
    let mut index = 0;
    while index < na.len() && index < nb.len() {
        match numeric(na[index], nb[index]) {
            Ordering::Equal => index += 1,
            other => return other,
        }
    }
    if na.len() != nb.len() {
        return na.len().cmp(&nb.len());
    }
    pre_release(pa, pb)
}

/// 兼容社区 tag 形态的类 semver 比较：可选的 v 前缀、点分数字主体、短横线后的可选预发布段。
///
/// 与 [`cmp_semver`] 同一套规则；发布列表、依赖降级、插件更新检查都在用它，
/// 名字保留是为了不动那些调用点。
pub fn cmp_versions(a: &str, b: &str) -> Ordering {
    cmp_semver(a, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions() {
        assert_eq!(cmp_versions("1.2.3", "1.2.3"), Ordering::Equal);
        assert_eq!(cmp_versions("1.2.3", "1.2.4"), Ordering::Less);
        assert_eq!(cmp_versions("v1.2.3", "1.2.3"), Ordering::Equal);
        assert_eq!(cmp_versions("1.2.3-rc.1", "1.2.3"), Ordering::Less);
        assert_eq!(cmp_versions("1.2.3-rc.2", "1.2.3-rc.1"), Ordering::Greater);
        // 数字形式的预发布段按数值比较，而不是按字典序比较。
        assert_eq!(cmp_versions("0.1.1-rc.10", "0.1.1-rc.2"), Ordering::Greater);
        assert_eq!(cmp_versions("1.10.0", "1.9.9"), Ordering::Greater);
        assert_eq!(cmp_versions("0.1.66", "0.1.70"), Ordering::Less);
        assert_eq!(cmp_versions("0.1.1-rc.2", "0.1.1"), Ordering::Less);
        assert_eq!(
            cmp_versions("1.2.3-alpha.1", "1.2.3-alpha.2"),
            Ordering::Less
        );
        // HEAD 哈希与 semver tag 比较：semver 解析为 [n,n,n]，哈希解析为空，
        // 因此哈希被排为 Less —— 也就是说 `latest = "0.16.0"` 会被报告为
        // 比 `installed = "head"` 更新的版本。`plugins::update` 中的修
        // 复是在安装完成后清空 `latest_version`，从而避免对陈旧的 semver
        // 执行这次比较。
        assert_eq!(cmp_versions("0.16.0", "head"), Ordering::Greater);
        assert_eq!(cmp_versions("head", "head"), Ordering::Equal);
    }

    /// 预发布段的排序按 semver：纯数字标识符**低于**带字母的。
    #[test]
    fn compares_semver_prereleases_and_ignores_build_metadata() {
        assert_eq!(cmp_semver("1.2.3+build.7", "1.2.3+other"), Ordering::Equal);
        assert_eq!(cmp_semver("1.2.3-rc.10", "1.2.3-rc.2"), Ordering::Greater);
        assert_eq!(cmp_semver("1.2.3-1", "1.2.3-alpha"), Ordering::Less);
        assert_eq!(cmp_semver("1.2.3", "1.2.3-rc.1"), Ordering::Greater);
        assert_eq!(
            cmp_semver("99999999999999999999.2.3", "2.2.3"),
            Ordering::Greater
        );
    }

    #[test]
    fn accepts_only_semver_shaped_versions() {
        for ok in [
            "0.1.2",
            "0.1.2-rc.18",
            "0.1.2-alpha.3",
            "v1.0.0",
            "1.0.0+build.5",
        ] {
            assert!(is_valid_kernel_version(ok), "{ok} 应当被接受");
        }
        for bad in [
            "",
            ".",
            "..",
            "../..",
            r"..\..\Users\x",
            "1.0/hotfix",
            "1.0\\x",
            "1.0\"x",
            "1.0 x",
            "-1.0.0",
        ] {
            assert!(!is_valid_kernel_version(bad), "{bad:?} 必须被拒绝");
        }
    }
}
