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
//! 内核的包集合由**上游自己的打包决定**，不是壳能定的：`@deepseek-ai/dsh` 的
//! `dependencies` 才跟着内核一起装，被降级成 `devDependencies` 的包在内核树里
//! 就没有了。实测过一次：`@deepseek-ai/dsh-hooks-claude-code` 在 `0.2.1-alpha.1`
//! 里是运行时依赖，`0.2.1-alpha.2` 里是开发依赖。
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
//! 的开关时，得到的就是这条。

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
    pub(crate) const fn optional(package: &'static str, missing_note: &'static str) -> Self {
        Self {
            package,
            required: false,
            missing_note,
        }
    }

    /// 这个 peer 在给定内核安装树里的落地位置。
    ///
    /// **只此一份**：物化建链接与状态探针判「在不在」问的是同一个路径，
    /// 各拼一份的漂移症状是「探针说在、物化说不在」。
    pub(crate) fn path_in(&self, kernel_root: &Path) -> PathBuf {
        kernel_root
            .join("node_modules")
            .join("@deepseek-ai")
            .join(self.package)
    }
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

/// 内核树里缺哪些**可选** peer（按登记表顺序）。
///
/// 只读探针，给面板与状态命令用：缺失的**必需** peer 由物化直接报错（那条路
/// 说的是「这个内核装不上这个插件」），而可选 peer 缺失只是「这个内核版本少
/// 一项能力」——那件事必须**如实说给用户**，但不是拦下启用。
pub(crate) fn missing_optional_peers(
    spec: &super::builtins::BuiltinSpec,
    kernel_root: &Path,
) -> Vec<MissingPeer> {
    spec.peers
        .iter()
        .filter(|peer| !peer.required && !peer.path_in(kernel_root).is_dir())
        .map(|peer| MissingPeer {
            package: peer.package,
            note: peer.missing_note,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::builtin::builtins::{self, BUILTINS, OPENAI_OAUTH, PONYTAIL};

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

    /// **hook 桥是可选 peer**——本次 bug 的回归判据（2026-10-12）。
    ///
    /// 内核 `0.2.1-alpha.2` 把 `@deepseek-ai/dsh-hooks-claude-code` 从
    /// `dependencies` 降级成 `devDependencies`，内核树里不再有它。改回必需时
    /// 「切换 PonyTail」会重新变成整插件装不进去，这条会红。
    #[test]
    fn the_hook_bridge_is_optional_while_the_skill_package_is_required() {
        let ponytail = builtins::find(PONYTAIL).unwrap();
        let bridge = ponytail
            .peers
            .iter()
            .find(|peer| peer.package == "dsh-hooks-claude-code")
            .expect("ponytail 必须列 hook 桥");
        assert!(!bridge.required, "hook 桥必须可选：内核会不带它");
        assert!(
            bridge.missing_note.contains("dsh-hooks-claude-code"),
            "缺失说明要点名包名，实际：{}",
            bridge.missing_note
        );
        let skill = ponytail
            .peers
            .iter()
            .find(|peer| peer.package == "dsh-skill")
            .expect("ponytail 必须列 dsh-skill");
        assert!(
            skill.required,
            "dsh-skill 是内核运行时依赖，缺了技能注册不了"
        );
        // OpenAI OAuth 的 peer 都是运行时依赖：它的可启用性不该随内核版本变，
        // 这正是「参考 OpenAI OAuth 的处理方式」要保住的性质。
        assert!(builtins::find(OPENAI_OAUTH)
            .unwrap()
            .peers
            .iter()
            .all(|peer| peer.required));
    }

    /// 探针按内核树如实回答：桥不在 → 报出来（带说明）；桥在 → 什么都不报。
    #[test]
    fn missing_optional_peers_follows_the_kernel_tree() {
        let ponytail = builtins::find(PONYTAIL).unwrap();
        let root = std::env::temp_dir().join(format!("builtin-peers-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let at_deepseek = root.join("node_modules").join("@deepseek-ai");
        std::fs::create_dir_all(at_deepseek.join("dsh-skill")).unwrap();
        let missing = missing_optional_peers(ponytail, &root);
        assert_eq!(
            missing.iter().map(|peer| peer.package).collect::<Vec<_>>(),
            vec!["dsh-hooks-claude-code"],
            "内核不带桥时要报出来（必需 peer 缺了不走这条路，它由物化直接失败）"
        );
        assert!(
            !missing[0].note.is_empty(),
            "报出来就必须带上说明，否则面板只能显示一个包名"
        );
        std::fs::create_dir_all(at_deepseek.join("dsh-hooks-claude-code")).unwrap();
        assert!(
            missing_optional_peers(ponytail, &root).is_empty(),
            "桥在内核树里时不该报降级"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
