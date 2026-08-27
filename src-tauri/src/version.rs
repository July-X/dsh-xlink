//! Version comparison shared by the kernel release list (`releases`) and the
//! community-plugin update checks (`plugins`).

use std::cmp::Ordering;

/// Semver-ish comparison tolerant of the community's tag shapes: optional v
/// prefix, dot-separated numeric core, optional prerelease after a dash.
/// Release > prerelease; prerelease segments compare numerically when both
/// numeric, lexically otherwise.
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
        // Numeric prerelease segments compare numerically, not lexically.
        assert_eq!(cmp_versions("0.1.1-rc.10", "0.1.1-rc.2"), Ordering::Greater);
        assert_eq!(cmp_versions("1.10.0", "1.9.9"), Ordering::Greater);
        assert_eq!(cmp_versions("0.1.66", "0.1.70"), Ordering::Less);
        assert_eq!(cmp_versions("0.1.1-rc.2", "0.1.1"), Ordering::Less);
        assert_eq!(
            cmp_versions("1.2.3-alpha.1", "1.2.3-alpha.2"),
            Ordering::Less
        );
        // HEAD hash vs a semver tag: the semver parses as [n,n,n] and the
        // hash parses as empty, so the hash sorts as Less — i.e. `latest =
        // "0.16.0"` reports as newer than `installed = "head"`.  The fix in
        // `plugins::update` clears `latest_version` after install so this
        // comparison never runs against a stale semver.
        assert_eq!(cmp_versions("0.16.0", "head"), Ordering::Greater);
        assert_eq!(cmp_versions("head", "head"), Ordering::Equal);
    }
}
