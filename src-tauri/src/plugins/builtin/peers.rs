//! peer 的语义：**哪些内核包是必需的、哪些只是能力**。
//!
//! 独立于 `builtins.rs` 的理由：那边是**登记表**（这个版本的应用带哪几个内置
//! 插件、各物化什么），回答的是「有哪些」；这里回答的是「缺了会怎样」——必需 /
//! 可选的判据、缺失探针、给用户的那句话。两件事混在一起时，「加一个内置插件」
//! 与「改 peer 的缺失语义」会撞在同一个文件里，而后者是全局规则：改一次影响
//! 所有插件。
//!
//! ## 为什么必需 / 可选必须分开
//!
//! 内核的包集合由**上游自己的打包决定**，不是壳能定的。判据是「**内核树里是不是
//! 必然有它**」——**不是**「`@deepseek-ai/dsh` 的 `dependencies` 里有没有」。
//! 2026-10-15 实测（`0.2.1-alpha.1` 与 `alpha.2` 两棵树逐包核对）：
//!
//! - `dsh-llm` 与 `dsh-tools` 在 `dsh` 自己的清单里都是 **`devDependencies`**，
//!   却在两棵树里都实实在在躺着——`dsh-base` 与 `dsh-sdk-minimal` 在**运行时**
//!   依赖它们。照 `dsh` 一份清单判，会把这两个判成「装不上」。
//! - `@deepseek-ai/schemastery` 反过来：它是 `dsh` 真正的 `dependencies`。
//! - 真会消失的是**没有任何内核包在运行时依赖**的那些：
//!   `@deepseek-ai/dsh-hooks-claude-code` 在 `0.2.1-alpha.1` 是 `dependencies`，
//!   `0.2.1-alpha.2` 里被降级成 `devDependencies`，树里就没有了——那才是可选
//!   peer 的形态。
//!
//! 所以「这个包会不会在」只能看**装出来的那棵树**（或看有没有别的内核包在运行时
//! 依赖它），读 `dsh` 一份清单会得出反的结论。`builtins.rs` 里每个 peer 的注释
//! 都记着它是哪一种。
//!
//! 于是「这个包在不在」与「这个插件能不能装」是两件事：
//!
//! - **必需**（[`PeerSpec::required`]）：内核树里没有它，插件根本跑不起来
//!   （host 静态 import 的东西），物化失败是正确行为。
//! - **可选**（[`PeerSpec::optional`]）：缺了只少一项能力，插件照常装、照常跑，
//!   由 host 自己降级 + 面板如实说明。
//!
//! 把可选包写成必需，代价是**整个插件（含它自己的技能）一起装不进去**，而报错
//! 还会把用户支去「内核版本」页重装一个没坏的内核——2026-10-12 用户点 Ponytail
//! 的开关时，得到的就是这条。**2026-11-03 起 Ponytail 改原生 hook，三个事件自己
//! 监听内核扩展点，不再有可选 peer**；这条机制与它的探针保留给以后的可选能力，
//! 单测改用合成登记表（`optional_peer_spec`），不拿真实插件当夹具。

use std::path::{Path, PathBuf};

/// 一个 peer：Host 运行时要从内核树链进来的 `@deepseek-ai` 包。
pub(crate) struct PeerSpec {
    /// `@deepseek-ai/` 下的包名。
    pub(crate) package: &'static str,
    /// `true` = 内核树里没有它，物化就失败。
    pub(crate) required: bool,
    /// 可选 peer 缺失时给用户的一句解释（由后端出文案，前端不拼）。
    pub(crate) missing_note: &'static str,
}

impl PeerSpec {
    /// 必需 peer：内核树里没有它 → 物化失败。
    pub(crate) const fn required(package: &'static str) -> Self {
        Self {
            package,
            required: true,
            missing_note: "",
        }
    }

    /// 可选 peer：内核树里没有它 → 跳过链接，插件自己降级，面板如实说明。
    ///
    /// `missing_note` 是**给用户看的那句话**，不是给维护者看的注释：它必须说清
    /// 「少的是哪项能力、还有哪条路可走」。写「可选依赖缺失」等于没说。
    ///
    /// 2026-11-03 起没有内置插件用到它（ponytail 改原生 hook 后不再有可选 peer），
    /// 但可选能力这套语义与探针保留给以后：删掉就得把 `peer_supply`、面板降级、
    /// 物化跳过分支一起删，代价远大于一个 allow。
    #[allow(dead_code)]
    pub(crate) const fn optional(package: &'static str, missing_note: &'static str) -> Self {
        Self {
            package,
            required: false,
            missing_note,
        }
    }

    /// 这个 peer 在给定内核安装树里的落地位置。
    pub(crate) fn path_in(&self, kernel_root: &Path) -> PathBuf {
        package_path(kernel_root, self.package)
    }
}

/// 内核安装树里某个 `@deepseek-ai` 包的落点。
///
/// **只此一份**：物化建链接、依赖指纹读版本、可选 peer 探针、供货计划判
/// 「在不在」问的都是同一个路径，各拼一份的漂移症状是「探针说在、物化说不在」。
pub(crate) fn package_path(kernel_root: &Path, package: &str) -> PathBuf {
    kernel_root
        .join("node_modules")
        .join("@deepseek-ai")
        .join(package)
}

/// 读一个包目录里 `package.json` 的版本。用于判「这两份是不是同一版」
/// （壳补的副本要不要换新）与指纹计算。
pub(crate) fn manifest_version(package_dir: &Path) -> Result<String, String> {
    let manifest = package_dir.join("package.json");
    let text = std::fs::read_to_string(&manifest)
        .map_err(|error| format!("读取包清单失败（{}）：{error}", manifest.display()))?;
    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| format!("包清单不是有效 JSON（{}）：{error}", manifest.display()))?;
    value
        .get("version")
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or_else(|| format!("包清单缺 version 字段（{}）", manifest.display()))
}

/// 读内核树里某个包的清单版本。
///
/// `dsh` 的版本就是内核版本（内核 monorepo 锁步发布），补齐缺失的 peer 时按它
/// 取源：这些包的 `peerDependencies` 逐包钉死同一个版本号，跨版本拼装是没验证过
/// 的组合。
pub(crate) fn package_version(kernel_root: &Path, package: &str) -> Result<String, String> {
    manifest_version(&package_path(kernel_root, package)).map_err(|error| {
        format!("{error}；内核安装树可能不完整，请在「内核版本」页重装当前内核版本")
    })
}

/// 内核树里缺的一个**可选** peer：包名 + 「少了它意味着什么」。
///
/// 用自有结构而不是 `&PeerSpec`：登记表是 `'static` 的，而探针的入参
/// `&BuiltinSpec` 不是——把 `'static` 泄漏到探针签名上只会逼调用方去改
/// 生命周期，这里只需要两个字符串。
#[derive(Clone, Copy)]
pub(crate) struct MissingPeer {
    pub(crate) package: &'static str,
    pub(crate) note: &'static str,
}

/// 内核树里缺哪些**可选** peer（按登记表顺序），**供货目录里补过的算不缺**。
///
/// 只读探针，给面板与状态命令用：缺失的**必需** peer 由物化直接报错（那条路
/// 说的是「这个内核装不上这个插件」），而可选 peer 缺失只是「这个内核版本少
/// 一项能力」——那件事必须**如实说给用户**，但不是拦下启用。
///
/// `dsh_home` 是给 [`super::peer_supply`] 判「壳自己补过一份没有」用的：补过就
/// 不该再挂 ⚠——那条说明写的是「本内核版本少一项能力」，而补过之后能力是有的。
pub(crate) fn missing_optional_peers(
    spec: &super::builtins::BuiltinSpec,
    kernel_root: &Path,
    dsh_home: &Path,
) -> Vec<MissingPeer> {
    spec.peers
        .iter()
        .filter(|peer| {
            !peer.required
                && super::peer_supply::resolve(dsh_home, spec, peer.package, kernel_root).is_none()
        })
        .map(|peer| MissingPeer {
            package: peer.package,
            note: peer.missing_note,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::builtin::builtins::{self, BuiltinSpec, BUILTINS, OPENAI_OAUTH, PONYTAIL};

    /// 可选 peer 探针的合成登记。ponytail 2026-11-03 改为原生 hook 后不再有可选
    /// peer；但「缺可选 peer 只降级、不拦启用」这条全局语义仍要给以后的内置插件
    /// 留着，所以用一份测试专用 spec 验，不拿真实登记表当夹具。
    static OPTIONAL_PEER_SPEC: BuiltinSpec = BuiltinSpec {
        id: "demo-optional",
        row_id: "xlink-demo-optional",
        entries: &["host", "package.json"],
        peers: &[
            PeerSpec::required("dsh-skill"),
            PeerSpec::optional(
                "dsh-hooks-claude-code",
                "演示用的可选 peer 缺失：插件照常启用，只是少一项演示能力。",
            ),
        ],
        entry: "host/index.js",
        display_name: "Demo",
        summary: "",
        note: "",
        has_account_card: false,
        upstream: None,
    };

    fn optional_peer_spec() -> &'static BuiltinSpec {
        &OPTIONAL_PEER_SPEC
    }

    /// 每个 peer 的必需性都要说清楚：**可选 peer 必须带缺失说明**（面板就靠
    /// 这句话告诉用户少了什么），必需 peer 不该带（带了没人读，还会让人以为
    /// 「缺了也没事」）。包名也不许重复——同一个包同时出现在必需与可选两处时，
    /// 物化的两个分支各判一次，结果取决于遍历顺序。
    #[test]
    fn every_peer_declares_requiredness_and_optional_ones_explain_the_loss() {
        for spec in BUILTINS {
            let mut names: Vec<&str> = spec.peers.iter().map(|peer| peer.package).collect();
            let count = names.len();
            names.sort_unstable();
            names.dedup();
            assert_eq!(names.len(), count, "{} 的 peer 包名重复了", spec.id);
            for peer in spec.peers {
                assert!(
                    !peer.package.is_empty() && !peer.package.contains('@'),
                    "{} 的 peer 包名要写成 @deepseek-ai/ 下的裸名，实际 {:?}",
                    spec.id,
                    peer.package
                );
                if peer.required {
                    assert!(
                        peer.missing_note.is_empty(),
                        "{} 的必需 peer {} 不该带缺失说明：缺了它物化就失败，没有「降级」这一档",
                        spec.id,
                        peer.package
                    );
                } else {
                    assert!(
                        peer.missing_note.len() > 20,
                        "{} 的可选 peer {} 必须写清缺了会少什么（面板原样给用户看）",
                        spec.id,
                        peer.package
                    );
                }
            }
        }
    }

    /// **ponytail 的 hook 改为原生接线**（2026-11-03），所以它只需要
    /// `dsh-skill`（技能 rank）与 `dsh-llm`（把 hook 返回的 additionalContext
    /// 包成内核消息）两个**内核树必然有**的包；如果谁又把兼容桥写成 peer，
    /// 这条会红——那等于把「hook 能不能用」重新绑到一个可选包上。
    #[test]
    fn ponytail_needs_only_packages_the_kernel_tree_always_has() {
        let ponytail = builtins::find(PONYTAIL).unwrap();
        let packages: Vec<&str> = ponytail.peers.iter().map(|peer| peer.package).collect();
        assert_eq!(packages, vec!["dsh-skill", "dsh-llm"]);
        assert!(
            ponytail.peers.iter().all(|peer| peer.required),
            "两个 peer 都是内核树里必然有的包，缺了就该物化失败"
        );
        assert!(
            !packages.contains(&"dsh-hooks-claude-code"),
            "hook 已改原生扩展点，不要再把兼容桥列为 peer"
        );
        // OpenAI OAuth 的 peer 都是运行时依赖：它的可启用性不该随内核版本变，
        // 这正是「参考 OpenAI OAuth 的处理方式」要保住的性质。
        assert!(builtins::find(OPENAI_OAUTH)
            .unwrap()
            .peers
            .iter()
            .all(|peer| peer.required));
    }

    /// 探针按内核树如实回答：可选 peer 不在 → 报出来（带说明）；在 → 什么都不报。
    #[test]
    fn missing_optional_peers_follows_the_kernel_tree() {
        let spec = optional_peer_spec();
        let root = std::env::temp_dir().join(format!("builtin-peers-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let at_deepseek = root.join("node_modules").join("@deepseek-ai");
        std::fs::create_dir_all(at_deepseek.join("dsh-skill")).unwrap();
        let missing = missing_optional_peers(spec, &root, &root);
        assert_eq!(
            missing.iter().map(|peer| peer.package).collect::<Vec<_>>(),
            vec!["dsh-hooks-claude-code"],
            "内核不带可选 peer 时要报出来（必需 peer 缺了不走这条路，它由物化直接失败）"
        );
        assert!(
            !missing[0].note.is_empty(),
            "报出来就必须带上说明，否则面板只能显示一个包名"
        );
        std::fs::create_dir_all(at_deepseek.join("dsh-hooks-claude-code")).unwrap();
        assert!(
            missing_optional_peers(spec, &root, &root).is_empty(),
            "可选 peer 在内核树里时不该报降级"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 壳自己补过一份之后，探针同样不该再报降级。
    ///
    /// 那条说明写的是「本内核版本少一项能力」——补过之后能力是有的，再挂 ⚠ 就是
    /// 谎报；而用户看到的后果是他去点「换内核版本」换一个根本不需要换的东西。
    #[test]
    fn a_peer_supplied_by_the_shell_is_not_reported_missing() {
        let spec = optional_peer_spec();
        let root = std::env::temp_dir().join(format!(
            "builtin-peers-supplied-{}-{}",
            std::process::id(),
            crate::shell::process::epoch_millis()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let home = root.join("home");
        let kernel = root.join("kernel");
        let at_deepseek = kernel.join("node_modules/@deepseek-ai");
        std::fs::create_dir_all(at_deepseek.join("dsh-skill")).unwrap();
        std::fs::create_dir_all(at_deepseek.join("dsh")).unwrap();
        std::fs::write(
            at_deepseek.join("dsh/package.json"),
            r#"{"version":"0.2.1-alpha.2"}"#,
        )
        .unwrap();
        // 内核树里没有可选 peer，但壳按内核版本补过一份。
        let supplied = home.join(format!(
            "plugins/builtin/{}/peers/dsh-hooks-claude-code@0.2.1-alpha.2",
            spec.id
        ));
        std::fs::create_dir_all(&supplied).unwrap();
        std::fs::write(
            supplied.join("package.json"),
            r#"{"version":"0.2.1-alpha.2"}"#,
        )
        .unwrap();
        assert!(
            missing_optional_peers(spec, &kernel, &home).is_empty(),
            "补过的包算「有」，不该再报降级"
        );
        // 换一个内核版本（供货目录里那份是给旧版本补的）→ 重新如实报缺。
        std::fs::write(
            at_deepseek.join("dsh/package.json"),
            r#"{"version":"0.2.2-alpha.1"}"#,
        )
        .unwrap();
        assert_eq!(
            missing_optional_peers(spec, &kernel, &home)
                .iter()
                .map(|peer| peer.package)
                .collect::<Vec<_>>(),
            vec!["dsh-hooks-claude-code"],
            "补的那份是按内核版本分的，换内核版本后必须重补——这里要如实报缺"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
