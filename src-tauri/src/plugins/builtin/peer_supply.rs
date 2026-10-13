//! 可选 peer 在内核树里缺失时，壳从 npm 补一份（版本与内核**严格一致**）。
//!
//! 起因是上游自己的打包决定：`@deepseek-ai/dsh-hooks-claude-code` 在
//! `0.2.1-alpha.1` 是运行时依赖、在 `0.2.1-alpha.2` 被降级成开发依赖，于是同一个
//! 插件在两个内核上一个能注入会话规则、一个不能——2026-10-13 用户点 Ponytail 的
//! 开关，看到的就是这条。补一份比「让用户换内核版本」正确：能力缺口是壳能修的，
//! 而换内核会连带换掉用户整个工作台。
//!
//! ## 四条纪律
//!
//! - **只补内核树里没有的包**。树里有就链树里的——那才是内核自己的实现，壳补的
//!   永远是替身。
//! - **只取源，不碰内核树**。落点在插件自己的数据目录
//!   `<DSH_HOME>/plugins/builtin/<id>/peers/<包>@<版本>/`，与上游内容
//!   （`upstream/`）同一层。根 AGENTS 那条「绝不往已安装的内核目录里写任何东西」
//!   照旧成立。
//! - **版本等于内核版本，不许挑**。这些是内核 monorepo 的锁步子包，实物的
//!   `peerDependencies` 逐包钉死 `0.2.1-alpha.2`；取 `latest`（`0.0.1-rc.5`）会
//!   装进一份与树里其余包不同代的实现。
//! - **软失败**。补不到就照旧走可选 peer 的降级路径（host 动态 `import` 失败 →
//!   自己降级 + 面板 ⚠），绝不因此拦下启用。
//!
//! ## 连它的 peer 一起补
//!
//! 补进来的包自己也有 peer（`dsh-hooks-claude-code` 要 `dsh-hook-protocol`，后者
//! 要 `dsh-shell` / `dsh-session` / `cordis`）。树里有的按普通 peer 链过去，树里
//! 也缺的继续补——所以 [`ensure`] 走的是**闭包**而不是一张固定清单。
//!
//! ## 节流
//!
//! 工作台每次启动都会走 `ensure_wired`（[`super::runtime`]），没有节流的话离线
//! 用户每启动一次都要重付一遍网络超时。失败后 30 分钟内不再试：判据是
//! `<peers>/.attempt-<包>@<版本>.json` 里的时间戳，成功即删。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::builtins::BuiltinSpec;
use super::peers;

/// 失败后多久才再试一次。与上游同步的短退避同量级：够躲开「一次开机里连打
/// 几遍」，又不至于让用户等一天。
const RETRY_BACKOFF_MS: u64 = 30 * 60 * 1000;

/// 一个包从哪儿来。
#[derive(Clone)]
pub(crate) enum PeerSource {
    /// 内核安装树里的原件（物化时建符号链接）。
    Kernel(PathBuf),
    /// 壳补的那一份（物化时**复制**进插件目录，见下）。
    Supplied(PathBuf),
}

/// 计划里的一条：要摆进插件 `node_modules/@deepseek-ai/` 的包与它的来源。
#[derive(Clone)]
pub(crate) struct PlanEntry {
    pub(crate) package: String,
    pub(crate) source: PeerSource,
}

/// 补进来的包在插件目录里必须是**真目录**而不是符号链接。
///
/// Node 解析 import 时用的是**真实路径**：链接进插件目录的那份包，往上一层层找
/// `node_modules` 时看的是它自己所在的真实位置。内核树里的包因此天然在树的
/// `node_modules` 里解析自己的依赖（链过去的就是原件）；而壳补的这份如果也做成
/// 链接，它的真实位置在 `<DSH_HOME>/plugins/builtin/<id>/peers/...`，那里没有
/// `node_modules`，它的 `import '@deepseek-ai/dsh-hook-protocol'` 会当场解析失败
/// ——症状是「补了却还是不生效」，且没有任何报错指向这里。
fn root(dsh_home: &Path, spec: &BuiltinSpec) -> PathBuf {
    dsh_home
        .join("plugins")
        .join("builtin")
        .join(spec.id)
        .join("peers")
}

/// 某个包在供货目录里的落点（**按版本分目录**：换内核版本后补的那份必须重取，
/// 就地覆盖会让旧内核的树拿到新版本的包）。
pub(crate) fn dir(dsh_home: &Path, spec: &BuiltinSpec, package: &str, version: &str) -> PathBuf {
    root(dsh_home, spec).join(format!("{package}@{version}"))
}

/// 内核版本（读内核树里 `dsh` 自己的清单，不猜目录名）。
pub(crate) fn kernel_version(kernel_root: &Path) -> Option<String> {
    peers::package_version(kernel_root, "dsh").ok()
}

/// 这个包从哪儿来：内核树优先，其次壳补过的那份，都没有就是 `None`。
///
/// **只读**，不联网——物化与探针都调它，两者必须给出同一个答案（漂移的症状是
/// 「面板说能力在、工作台里加载不了」）。
pub(crate) fn resolve(
    dsh_home: &Path,
    spec: &BuiltinSpec,
    package: &str,
    kernel_root: &Path,
) -> Option<PeerSource> {
    let in_kernel = peers::package_path(kernel_root, package);
    if in_kernel.is_dir() {
        return Some(PeerSource::Kernel(in_kernel));
    }
    let version = kernel_version(kernel_root)?;
    let supplied = dir(dsh_home, spec, package, &version);
    supplied.is_dir().then_some(PeerSource::Supplied(supplied))
}

/// 要摆进插件 `node_modules/@deepseek-ai/` 的完整清单：登记表里的 peer，加上
/// **壳补的那些包自己带的 peer / 依赖**（逐层展开；内核树里的包不展开——它们在
/// 树里解析自己的依赖，见 [`PeerSource`] 的注释）。
///
/// 顺序稳定：先按登记表，再按展开顺序，便于单测与排查。
pub(crate) fn plan(spec: &BuiltinSpec, dsh_home: &Path, kernel_root: &Path) -> Vec<PlanEntry> {
    let mut entries: Vec<PlanEntry> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut worklist: Vec<String> = Vec::new();
    for peer in spec.peers {
        if !seen.insert(peer.package.to_string()) {
            continue;
        }
        if let Some(source) = resolve(dsh_home, spec, peer.package, kernel_root) {
            if matches!(source, PeerSource::Supplied(_)) {
                worklist.push(peer.package.to_string());
            }
            entries.push(PlanEntry {
                package: peer.package.to_string(),
                source,
            });
        }
    }
    while let Some(package) = worklist.pop() {
        let Some(PeerSource::Supplied(from)) = entries
            .iter()
            .find(|entry| entry.package == package)
            .map(|entry| entry.source.clone())
        else {
            continue;
        };
        for dependency in declared_packages(&from) {
            if !seen.insert(dependency.clone()) {
                continue;
            }
            if let Some(source) = resolve(dsh_home, spec, &dependency, kernel_root) {
                if matches!(source, PeerSource::Supplied(_)) {
                    worklist.push(dependency.clone());
                }
                entries.push(PlanEntry {
                    package: dependency,
                    source,
                });
            }
        }
    }
    entries
}

/// 一份包的 `peerDependencies` + `dependencies` 里所有 `@deepseek-ai/*` 的短名。
///
/// 两类都要：peer 是它期望宿主提供的（`dsh-hook-protocol`），而 dependencies 里
/// 那个 `schemastery` 是真会 `import` 的运行时依赖，漏了同样加载不起来。
pub(crate) fn declared_packages(package_dir: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(package_dir.join("package.json")) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    for field in ["peerDependencies", "dependencies"] {
        let Some(map) = value.get(field).and_then(|v| v.as_object()) else {
            continue;
        };
        for name in map.keys() {
            if let Some(short) = name.strip_prefix("@deepseek-ai/") {
                names.push(short.to_string());
            }
        }
    }
    names.sort();
    names.dedup();
    names
}

/// 取源函数的形状（测试缝：不联网也能验闭包与节流）。
type Fetch<'a> = dyn Fn(&str, &str, &Path) -> Result<(), String> + 'a;

/// 把缺的包补齐（联网），返回**一句要记进日志的话**（`None` = 不需要补，也没什么
/// 可说的）。
///
/// 返回值刻意不是 `Result`：调用点在 [`super::ensure_wired`]，而它的 Err 会经
/// `kernel_adapter::prepare_instance` 的 `map_err(AdapterError::Io)?` **阻断工作台
/// 启动**。供货只是补一项能力——离线机器不该因此起不来。写成 Result 的话，某天
/// 有人顺手加一个 `?` 就能把这条变成真的；现在那种写法根本编译不过。
pub(crate) fn ensure_soft(
    dsh_home: &Path,
    spec: &BuiltinSpec,
    kernel_root: &Path,
) -> Option<String> {
    let fetch = |package: &str, version: &str, dest: &Path| {
        crate::pkg::fetch::fetch_npm_package(
            &format!("@deepseek-ai/{package}"),
            Some(version),
            dest,
            &mut |_message| {},
        )
        .map(|_installed| ())
    };
    match ensure_with(dsh_home, spec, kernel_root, &fetch) {
        Ok(supplied) if supplied.is_empty() => None,
        Ok(supplied) => Some(format!(
            "{} 的内核包已从 npm 按内核版本补齐：{}",
            spec.display_name,
            supplied.join("、")
        )),
        Err(reason) => Some(format!(
            "{} 的可选内核包没能补齐，本次按缺能力启动（面板会如实标注）：{reason}",
            spec.display_name
        )),
    }
}

fn ensure_with(
    dsh_home: &Path,
    spec: &BuiltinSpec,
    kernel_root: &Path,
    fetch: &Fetch<'_>,
) -> Result<Vec<String>, String> {
    let Some(version) = kernel_version(kernel_root) else {
        return Ok(Vec::new());
    };
    let mut supplied: Vec<String> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut queue: Vec<String> = spec
        .peers
        .iter()
        .filter(|peer| !peers::package_path(kernel_root, peer.package).is_dir())
        .map(|peer| peer.package.to_string())
        .collect();
    while let Some(package) = queue.pop() {
        if !seen.insert(package.clone()) {
            continue;
        }
        let target = dir(dsh_home, spec, &package, &version);
        if !target.is_dir() {
            if throttled(dsh_home, spec, &package, &version) {
                continue;
            }
            // 先落进暂存目录再改名：解包中途失败时留下的半个目录读起来是
            // 「这份补过了」（判据就是 `is_dir`），而它其实是坏的。
            let staging = root(dsh_home, spec).join(format!(".staging-{package}"));
            let _ = std::fs::remove_dir_all(&staging);
            std::fs::create_dir_all(&staging)
                .map_err(|error| format!("建供货暂存目录失败（{}）：{error}", staging.display()))?;
            match fetch(&package, &version, &staging) {
                Ok(()) => {
                    let _ = std::fs::remove_dir_all(&target);
                    std::fs::rename(&staging, &target).map_err(|error| {
                        format!(
                            "供货目录落位失败（{} → {}）：{error}",
                            staging.display(),
                            target.display()
                        )
                    })?;
                    let _ =
                        std::fs::remove_file(attempt_marker(dsh_home, spec, &package, &version));
                    supplied.push(format!("{package}@{version}"));
                }
                Err(reason) => {
                    let _ = std::fs::remove_dir_all(&staging);
                    record_attempt(dsh_home, spec, &package, &version);
                    return Err(format!("补齐内核包 {package}@{version} 失败：{reason}"));
                }
            }
        }
        for dependency in declared_packages(&target) {
            if resolve(dsh_home, spec, &dependency, kernel_root).is_none() {
                queue.push(dependency);
            }
        }
    }
    Ok(supplied)
}

fn attempt_marker(dsh_home: &Path, spec: &BuiltinSpec, package: &str, version: &str) -> PathBuf {
    root(dsh_home, spec).join(format!(".attempt-{package}@{version}.json"))
}

/// 上次失败是不是还在退避窗口里。读不出来按「不在」——宁可多试一次，也别让
/// 一个坏掉的时间戳把能力永久锁死。
fn throttled(dsh_home: &Path, spec: &BuiltinSpec, package: &str, version: &str) -> bool {
    let Ok(text) = std::fs::read_to_string(attempt_marker(dsh_home, spec, package, version)) else {
        return false;
    };
    let failed_at = serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|value| value.get("failedAtMs").and_then(|v| v.as_u64()));
    let Some(failed_at) = failed_at else {
        return false;
    };
    crate::shell::process::epoch_millis().saturating_sub(failed_at) < RETRY_BACKOFF_MS
}

fn record_attempt(dsh_home: &Path, spec: &BuiltinSpec, package: &str, version: &str) {
    let path = attempt_marker(dsh_home, spec, package, version);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let body = format!(
        "{{\n  \"package\": \"{package}\",\n  \"version\": \"{version}\",\n  \"failedAtMs\": {}\n}}\n",
        crate::shell::process::epoch_millis()
    );
    // 节流标记写不进去不影响正确性（只是防重试），静默。
    let _ = std::fs::write(&path, body);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::builtin::builtins::{self, BuiltinSpec, PONYTAIL};

    /// 测试夹具：`<root>/kernel` 只有 `dsh`（版本 0.2.1-alpha.2）+ 登记表里
    /// **必需要求**之外的包由各用例自己摆；`<root>/home` 是 DSH home。
    fn fixture(tag: &str) -> (PathBuf, PathBuf, PathBuf, &'static BuiltinSpec) {
        let root = std::env::temp_dir().join(format!(
            "builtin-supply-{tag}-{}-{}",
            std::process::id(),
            crate::shell::process::epoch_millis()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let kernel = root.join("kernel");
        write_package(&kernel, "dsh", "0.2.1-alpha.2", &[]);
        let home = root.join("home");
        std::fs::create_dir_all(&home).unwrap();
        (root, kernel, home, builtins::find(PONYTAIL).unwrap())
    }

    fn write_package(kernel: &Path, name: &str, version: &str, deps: &[&str]) {
        let dir = peers::package_path(kernel, name);
        std::fs::create_dir_all(&dir).unwrap();
        let peers = deps
            .iter()
            .map(|dep| format!("\"@deepseek-ai/{dep}\": \"{version}\""))
            .collect::<Vec<_>>()
            .join(",");
        std::fs::write(
            dir.join("package.json"),
            format!("{{\"name\":\"{name}\",\"version\":\"{version}\",\"peerDependencies\":{{{peers}}}}}"),
        )
        .unwrap();
        std::fs::write(dir.join("index.js"), "export {}\n").unwrap();
    }

    /// 供货目录里摆一份（模拟「上次已经补过」）。
    fn supply(
        home: &Path,
        spec: &BuiltinSpec,
        kernel: &Path,
        name: &str,
        deps: &[&str],
    ) -> PathBuf {
        let version = kernel_version(kernel).unwrap();
        let dir = dir(home, spec, name, &version);
        std::fs::create_dir_all(&dir).unwrap();
        let peers = deps
            .iter()
            .map(|dep| format!("\"@deepseek-ai/{dep}\": \"{version}\""))
            .collect::<Vec<_>>()
            .join(",");
        std::fs::write(
            dir.join("package.json"),
            format!("{{\"name\":\"{name}\",\"version\":\"{version}\",\"peerDependencies\":{{{peers}}}}}"),
        )
        .unwrap();
        dir
    }

    /// 内核树优先：树里有就走树里，只有缺的才去供货目录找。
    #[test]
    fn plan_prefers_the_kernel_tree() {
        let (root, kernel, home, spec) = fixture("plan");
        write_package(&kernel, "dsh-skill", "0.2.1-alpha.2", &[]);
        supply(&home, spec, &kernel, "dsh-hooks-claude-code", &[]);

        let plan = plan(spec, &home, &kernel);
        let by_name: Vec<(&str, bool)> = plan
            .iter()
            .map(|entry| {
                (
                    entry.package.as_str(),
                    matches!(entry.source, PeerSource::Kernel(_)),
                )
            })
            .collect();
        assert_eq!(
            by_name,
            vec![("dsh-skill", true), ("dsh-hooks-claude-code", false)],
            "必需 peer 从内核树来；补过的桥从供货目录来"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 只有「壳补的」那份包才展开它自己的 peer：树里的包在树内解析自己的依赖。
    #[test]
    fn plan_expands_only_supplied_packages() {
        let (root, kernel, home, spec) = fixture("expand");
        write_package(&kernel, "dsh-skill", "0.2.1-alpha.2", &[]);
        supply(
            &home,
            spec,
            &kernel,
            "dsh-hooks-claude-code",
            &["dsh-hook-protocol"],
        );
        supply(&home, spec, &kernel, "dsh-hook-protocol", &["dsh-shell"]);
        write_package(&kernel, "dsh-shell", "0.2.1-alpha.2", &[]);

        let names: Vec<String> = plan(spec, &home, &kernel)
            .into_iter()
            .map(|entry| entry.package)
            .collect();
        assert_eq!(
            names,
            vec![
                "dsh-skill",
                "dsh-hooks-claude-code",
                "dsh-hook-protocol",
                "dsh-shell"
            ],
            "闭包要跟到「补的包的 peer」为止；dsh-shell 在树里就链树里的"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 联网那一步：只取树里没有的，而且**版本严格等于内核版本**；取下来的包自己
    /// 缺的 peer 也一起取（桥要 protocol，protocol 要 shell）。
    #[test]
    fn ensure_fetches_the_missing_closure_at_the_kernel_version() {
        let (root, kernel, home, spec) = fixture("ensure");
        write_package(&kernel, "dsh-skill", "0.2.1-alpha.2", &[]);
        write_package(&kernel, "dsh-shell", "0.2.1-alpha.2", &[]);
        let fetched = std::cell::RefCell::new(Vec::new());
        let result = ensure_with(&home, spec, &kernel, &|package, version, dest| {
            fetched
                .borrow_mut()
                .push(format!("{package}@{version}"));
            let deps: &[&str] = match package {
                "dsh-hooks-claude-code" => &["dsh-hook-protocol"],
                "dsh-hook-protocol" => &["dsh-shell"],
                _ => &[],
            };
            let peers = deps
                .iter()
                .map(|dep| format!("\"@deepseek-ai/{dep}\": \"{version}\""))
                .collect::<Vec<_>>()
                .join(",");
            std::fs::write(
                dest.join("package.json"),
                format!("{{\"name\":\"{package}\",\"version\":\"{version}\",\"peerDependencies\":{{{peers}}}}}"),
            )
            .unwrap();
            Ok(())
        })
        .unwrap();

        let mut got = fetched.into_inner();
        got.sort();
        assert_eq!(
            got,
            vec![
                "dsh-hook-protocol@0.2.1-alpha.2",
                "dsh-hooks-claude-code@0.2.1-alpha.2"
            ],
            "树里有的（dsh-skill / dsh-shell）不许取；取的全是同版本"
        );
        let mut supplied = result;
        supplied.sort();
        assert_eq!(supplied.len(), 2);
        assert!(dir(&home, spec, "dsh-hooks-claude-code", "0.2.1-alpha.2").is_dir());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 失败写节流标记并且**不阻断**：30 分钟内不再试（离线用户不该每次启动工作台
    /// 都重付一遍网络超时）。
    #[test]
    fn a_failed_fetch_is_reported_and_throttled() {
        let (root, kernel, home, spec) = fixture("throttle");
        write_package(&kernel, "dsh-skill", "0.2.1-alpha.2", &[]);
        let calls = std::cell::Cell::new(0);
        let failing = |_package: &str, _version: &str, _dest: &Path| {
            calls.set(calls.get() + 1);
            Err("registry 连不上".to_string())
        };
        let error = ensure_with(&home, spec, &kernel, &failing).expect_err("取不到要如实报错");
        assert!(error.contains("dsh-hooks-claude-code"), "{error}");
        assert_eq!(calls.get(), 1);
        assert!(
            ensure_with(&home, spec, &kernel, &failing).is_ok(),
            "退避窗口里不该再失败一次（也不该再联网）"
        );
        assert_eq!(calls.get(), 1, "第二次不许再调取源");
        assert!(
            dir(&home, spec, "dsh-hooks-claude-code", "0.2.1-alpha.2")
                .parent()
                .unwrap()
                .join(".attempt-dsh-hooks-claude-code@0.2.1-alpha.2.json")
                .is_file(),
            "失败要落可查的标记"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 逐字段防御：清单缺字段 / 不是 JSON / 没有 `@deepseek-ai` 依赖时都给空表，
    /// 不 panic（这个函数在物化路径上读的是刚解包的内容）。
    #[test]
    fn declared_packages_survives_broken_manifests() {
        let dir = std::env::temp_dir().join(format!(
            "builtin-supply-manifest-{}-{}",
            std::process::id(),
            crate::shell::process::epoch_millis()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(declared_packages(&dir).is_empty(), "没有清单");
        std::fs::write(dir.join("package.json"), "not json").unwrap();
        assert!(declared_packages(&dir).is_empty(), "坏 JSON");
        std::fs::write(
            dir.join("package.json"),
            r#"{"dependencies":{"lodash":"1.0.0"},"peerDependencies":{"@deepseek-ai/dsh-session":"1"}}"#,
        )
        .unwrap();
        assert_eq!(
            declared_packages(&dir),
            vec!["dsh-session".to_string()],
            "只认能解析到的 @deepseek-ai 包，第三方依赖不在这里猜"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
