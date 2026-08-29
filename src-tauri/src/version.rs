//! 内核发布列表（`releases`）与社区插件更新检查（`plugins`）共用的版本比较函数。

use std::cmp::Ordering;

/// 兼容社区 tag 形态的类 semver 比较：可选的 v 前缀、点分数字主体、短横线后的可选预发布段。
/// 发布版大于预发布版；预发布段若两端都是数字则按数值比较，否则按字典序比较。
pub fn cmp_versions(a: &str, b: &str) -> Ordering {
    fn core(v: &str) -> (Vec<u64>, &str) {
        let stripped = v.strip_prefix('v').unwrap_or(v);
        let (head, pre) = stripped.split_once('-').unwrap_or((stripped, ""));
        let nums: Vec<u64> = head
            .split('.')
            .filter_map(|s| s.parse::<u64>().ok())
            .collect();
        (nums, pre)
    }
    let (na, pa) = core(a);
    let (nb, pb) = core(b);
    let mut i = 0;
    while i < na.len() && i < nb.len() {
        match na[i].cmp(&nb[i]) {
            Ordering::Equal => i += 1,
            other => return other,
        }
    }
    if na.len() != nb.len() {
        return na.len().cmp(&nb.len());
    }
    match (pa.is_empty(), pb.is_empty()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        (false, false) => {
            let xa: Vec<&str> = pa.split('.').collect();
            let ya: Vec<&str> = pb.split('.').collect();
            for (xi, yi) in xa.iter().zip(ya.iter()) {
                let ord = match (xi.parse::<u64>(), yi.parse::<u64>()) {
                    (Ok(n), Ok(m)) => n.cmp(&m),
                    (Ok(_), Err(_)) => Ordering::Greater,
                    (Err(_), Ok(_)) => Ordering::Less,
                    (Err(_), Err(_)) => xi.cmp(yi),
                };
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            xa.len().cmp(&ya.len())
        }
    }
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
}
