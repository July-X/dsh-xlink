//! 内置插件登记表：**随应用交付**的插件各是什么、物化什么、接哪一行。
//!
//! 这张表是「内置插件」这件事的**唯一事实源**。此前整条通道（资源管线、
//! 物化、接线、状态、命令、面板行）都按「有且只有 openai-oauth 一家」写死：
//! `openai-oauth` 这个字面量散在 8 处，每一处都要人记得同步。加第二个内置
//! 插件（ponytail）时把那些常量逐个参数化，不如一开始就让它们从这里读。
//!
//! ## 每个字段为什么在这一层
//!
//! - [`BuiltinSpec::id`]：物化目录名 / 打包资源目录名 / 面板行 key。**改它等于
//!   换一根数据根**（用户磁盘上已有的物化产物与 `state.json` 都会留在旧路径下），
//!   所以 openai-oauth 的 id 是不能动的既有契约，不是随手起的名字。
//! - [`BuiltinSpec::row_id`]：`cordis.patch.yml` 的行 id，同时是内核设置页
//!   命名空间的 settingsNs（OpenAI 的账户卡靠它对上一致）。它必须等于包名，
//!   `plugins/<id>/package.json` 的 `name` 与之逐字相同。
//! - [`BuiltinSpec::entries`]：物化进实例的运行时文件集。它同时被
//!   `scripts/prepare-builtin-plugins.mjs` 的同名清单读——**两份各管一门语言，
//!   改一边必须改另一边**，而 `check:builtin` 钉的是「产物必有清单且与源码一致」。
//! - [`BuiltinSpec::peers`]：Host 运行时要从内核树链进来的 `@deepseek-ai` 包。
//!   按插件实际 `import` 的东西列，不要图省事全列：多链一个包就多一条会断的
//!   链（旧内核装不上这个包时，物化直接失败，插件根本装不进去）。
//!   **每条还要说清是必需还是可选**（[`PeerSpec`]）：内核的包集合会变，
//!   必需与可选混为一谈的代价见 [`PeerSpec`] 的文档。
//! - [`BuiltinSpec::entry`]：接线行 `name` 指向的入口（相对物化目录）。
//! - 显示三件（`display_name` / `summary` / `note`）：面板那一行要的文案。
//!   后端出、前端画，避免同一句话在前端组件里再抄一份（抄的那份不参与
//!   Rust 侧测试，也不会随文案改动而报错）。
//! - [`BuiltinSpec::has_account_card`]：工作台设置页里有没有该插件自绘的
//!   账户卡。OpenAI 有（它要把 OAuth 走通），ponytail 没有（它不发请求）。

use super::peers::PeerSpec;

/// 一个内置插件的上游取源信息。
///
/// **只有随应用分发第三方内容、且上游有 npm 发布的插件才需要**（ponytail 有；
/// openai-oauth 没有——它是本仓自己写的插件，不存在「上游」这回事）。
/// 字段是 `&'static`：取源坐标随应用版本一起钉住，不读配置（理由同 [`BuiltinSpec`]）。
#[derive(Clone, Copy)]
pub(crate) struct UpstreamSpec {
    /// npm 包名。走 npm 而不是 GitHub tarball：默认 registry 是 npmmirror
    /// （快且稳，本机实测 GitHub 偶发 8 秒级延迟），而
    /// [`crate::pkg::fetch::fetch_npm_package`] 已经把 SRI 校验、解包防穿越、
    /// 代理路由三件事做齐了。
    pub(crate) npm_package: &'static str,
}

/// 一个内置插件的全部登记信息。字段全是 `&'static` 常量：登记表是编译期
/// 数据，不读配置、不读环境——「这个版本的应用带哪几个内置插件」必须由
/// 构建期决定，否则资源管线与清单校验（`check:builtin`）就无从校验。
pub(crate) struct BuiltinSpec {
    /// 物化目录 / 资源目录 / 面板行 key（见上）。
    pub(crate) id: &'static str,
    /// 接线行 id = settingsNs = 包名。
    pub(crate) row_id: &'static str,
    /// 物化的运行时文件集（相对插件源目录）。
    pub(crate) entries: &'static [&'static str],
    /// 从内核树 peer 进来的 `@deepseek-ai` 包（必需 / 可选见 [`PeerSpec`]）。
    pub(crate) peers: &'static [PeerSpec],
    /// 接线行 `name` 指向的入口（相对物化目录）。
    pub(crate) entry: &'static str,
    /// 面板行的名称。
    pub(crate) display_name: &'static str,
    /// 一句话说明（行内状态文字的兜底 tooltip）。
    pub(crate) summary: &'static str,
    /// 面向用户的一段说明（开关旁边的信息气泡）。
    pub(crate) note: &'static str,
    /// 工作台设置页里是否有该插件自绘的账户卡。
    pub(crate) has_account_card: bool,
    /// 上游取源（`None` = 本仓自有插件，无上游）。
    pub(crate) upstream: Option<UpstreamSpec>,
}

impl BuiltinSpec {
    /// 实例内物化根：`<DSH_HOME>/extensions/builtin/<id>`。
    ///
    /// **openai-oauth 的这一段是既有数据契约**：用户磁盘上已有的物化产物与
    /// `state.json` 都在这个路径下。改 id = 升级后用户的启用意图静默变成
    /// 「未启用」，而界面上看不出任何异常（这正是 [`crate::plugins::builtin::state`]
    /// 那条「解析失败不按空文件处理」要防的同一类事故，从路径迁移那头绕过来了）。
    pub(crate) fn instance_base_segment(&self) -> String {
        format!("extensions/builtin/{}", self.id)
    }
}

/// 内置 openai-oauth 的 id 常量。消费侧（`usage::subscription_openai` 的
/// 分区门控）按 id 取 spec，不直接 new 一个——spec 里还有物化文件集与
/// 接线行 id，凭 id 造一个残缺 spec 出来会让门控与实际接线脱钩。
pub(crate) const OPENAI_OAUTH: &str = "openai-oauth";
/// 内置 ponytail 的 id 常量。
pub(crate) const PONYTAIL: &str = "ponytail";

/// 全部内置插件。**新增内置插件只改这一处**，其余模块都从这里读。
pub(crate) const BUILTINS: &[BuiltinSpec] = &[
    BuiltinSpec {
        id: OPENAI_OAUTH,
        row_id: "xlink-openai-oauth",
        // 与 `plugins/openai-oauth/` 的目录形状一一对应：client 是工作台里的
        // 账户卡，locales 是它的中英文案。test/ 与 README 不进实例。
        entries: &["host", "client", "locales", "package.json"],
        // host/index.js 实际 import 的两个包：`dsh-llm`（适配器基类）与
        // `schemastery`（Config schema 的构造器，设置页的命名空间视图由它派生）。
        // 两个都是 `@deepseek-ai/dsh` 的**运行时依赖**，每个内核版本都带，
        // 所以都是必需 peer——「OpenAI OAuth 在任何内核上都能启用」靠的就是它。
        peers: &[
            PeerSpec::required("dsh-llm"),
            PeerSpec::required("schemastery"),
        ],
        entry: "host/index.js",
        display_name: "OpenAI-OAuth-Plugin",
        summary: "随应用交付的 OpenAI 套餐接入；启用不需要联网安装，关闭不删除账号",
        note: "把 ChatGPT 套餐接进当前内核的模型列表。令牌只存在本机（系统凭据库），\
               不进工作台页面；关闭只摘接线，不删账号，随时可再启用。",
        has_account_card: true,
        upstream: None,
    },
    BuiltinSpec {
        id: PONYTAIL,
        row_id: "xlink-ponytail",
        // 现在随包的是适配层（host + adapter + i18n）与许可。上游的技能正文
        // （六个 SKILL.md）与 hook 脚本**不随包走**（2026-10-12 改）：壳在启用时
        // 从上游拉最新并物化进实例，host 从物化目录里读。
        //
        // 为什么不再 vendor 进安装包（前一版真的 vendor 了，还为此写过一个逐字
        // 落盘 + 哈希锁定的脚本，那套东西本身是对的）：vendor 会**制造两份内容**
        // ——仓库里钉死的那一份，和用户手上的那一份。上游每改一次就要重发一版
        // 应用才能跟上，用户升级之前一直用旧内容；更要命的是「面板上写的版本」与
        // 「工作台里跑的内容」会分叉，而两边都不报错。现在随包的只有适配层，
        // 内容永远来自一次真实的上游同步。
        //
        // `i18n` 是**中文覆盖层**：上游只有英文版，而它依据哪一版上游由
        // `i18n/zh-CN/manifest.json` 逐文件记着 sha256。壳把这层物化进去，
        // host 启动时按指纹覆盖物化目录里那六个 SKILL.md（技能列表、技能正文、
        // 会话注入的规则集读的是同一批文件）；上游一改，指纹对不上就回落英文。
        // 不物化它 = 中文层永远到不了实例。
        entries: &["host", "adapter", "i18n", "package.json", "LICENSE"],
        // host/index.js 实际 import 的两件事：`BUNDLED_SKILL_RANK`（bundled 技能
        // 的优先级 600）与 `dsh-hooks-claude-code` 的 `apply`（把上游 hook 脚本
        // 挂到内核的拦截面上）。不 import 就不该链——多链一个包等于多一条会断
        // 的链。
        //
        // 两者的**必需性不同**（2026-10-12 真机报「切换 PonyTail 失败」之后改）：
        // `dsh-skill` 是 `@deepseek-ai/dsh` 的运行时依赖，任何内核版本都带，缺了
        // 技能也没法注册，所以必需；hook 桥是**上游内核自己的打包决定**——
        // `0.2.1-alpha.1` 里它是 `dsh` 的 `dependencies`，`0.2.1-alpha.2` 里被降级
        // 成 `devDependencies`，于是内核树里不再有它。把它当必需 peer，等于
        // 「技能能不能注册」由内核带不带一个可选的 hook 桥决定：在 alpha.2 上
        // 整个插件（含六个技能）都启用不了，而报错还把用户支去重装内核。
        // 现在它缺失 = 只少「会话启动即注入」与档位切换，由 host 动态 import 降级。
        peers: &[
            PeerSpec::required("dsh-skill"),
            PeerSpec::optional(
                "dsh-hooks-claude-code",
                "当前内核版本不带 @deepseek-ai/dsh-hooks-claude-code（上游把它从运行时依赖\
                 降级成了开发依赖）：技能照常可用，但「会话启动时自动注入规则」与 \
                 /ponytail 的档位切换在这个内核上不可用。换一个仍带这个包的内核版本\
                 （如 0.2.1-alpha.1）即可恢复，不必重装本应用。",
            ),
        ],
        entry: "host/index.js",
        display_name: "Ponytail",
        summary: "把「懒惰的资深开发者」这套规则接进工作台：会话启动即生效，可按档位调松紧",
        note: "来自 DietrichGebert/ponytail（MIT）的技能包：会话启动时把规则注入上下文，\
               /ponytail lite|full|ultra 可当场调松紧。**首次启用需要联网**从上游拉取，\
               之后每次启动工作台自动检查更新（一天一次），也可随时用 \
               /ponytail-update 立刻拉最新。规则是指令文本、会进入模型上下文，\
               因此**默认关闭**；同名时你自己装的技能优先。规则正文是上游英文原文。",
        has_account_card: false,
        upstream: Some(UpstreamSpec {
            npm_package: "@dietrichgebert/ponytail",
        }),
    },
];

/// 按 id 取 spec。`None` 表示「这个 id 不是本版本的内置插件」——命令层据此
/// 回「内置插件不存在」，而不是回退到某个默认值（回退会让拨错 id 的开关
/// 作用在别的插件上）。
pub(crate) fn find(id: &str) -> Option<&'static BuiltinSpec> {
    BUILTINS.iter().find(|spec| spec.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 登记表自身的形状：**不靠任何调用方来发现**（漏登记的表现是「界面上的
    /// 插件不在表里」，而那要等真机点一次开关才看得见）。
    #[test]
    fn ids_and_row_ids_are_unique_and_row_id_matches_the_package_name() {
        let mut ids: Vec<&str> = BUILTINS.iter().map(|spec| spec.id).collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(
            ids.len(),
            count,
            "内置插件 id 必须唯一（物化目录与资源目录同名）"
        );

        let mut rows: Vec<&str> = BUILTINS.iter().map(|spec| spec.row_id).collect();
        rows.sort_unstable();
        rows.dedup();
        assert_eq!(
            rows.len(),
            count,
            "接线行 id 必须唯一：两行同 id 会互相摘掉"
        );

        for spec in BUILTINS {
            assert_eq!(
                spec.row_id,
                format!("xlink-{}", spec.id),
                "{} 的接线行 id 必须等于 xlink-<id>：内核按 settingsNs 认插件，\
                 包名与行 id 对不上会让设置页拿不到命名空间",
                spec.id
            );
            // `entry` 是**物化目录里的文件**（`host/index.js`），不是清单里的
            // 一项——所以查的是「它落在某个被物化的目录下」，不是 `contains`。
            // 写成 `contains` 时这条判据恒红，而恒红的判据比没有判据更坏。
            assert!(
                spec.entries
                    .iter()
                    .any(|dir| spec.entry.starts_with(&format!("{dir}/"))),
                "{} 的入口 {} 落在物化清单之外，接线行会指向一个不存在的文件",
                spec.id,
                spec.entry
            );
            assert!(
                spec.entries.contains(&"package.json"),
                "{} 必须物化 package.json",
                spec.id
            );
            assert!(!spec.display_name.is_empty() && !spec.note.is_empty());
        }
    }

    /// 两个内置插件恰好是 OpenAI OAuth 与 Ponytail——新增第三个时这条要一起改，
    /// 它逼着下一个加内置插件的人把话说清楚（而不是悄悄多一个）。
    #[test]
    fn the_registry_carries_both_shipped_builtins() {
        assert_eq!(BUILTINS.len(), 2, "改了登记表就在这里说一句为什么");
        assert!(find(OPENAI_OAUTH).is_some());
        assert!(find(PONYTAIL).is_some());
        assert!(find("not-a-builtin").is_none());
    }

    /// 物化根既有契约：openai-oauth 的用户数据仍在老路径下。
    #[test]
    fn instance_base_segment_keeps_the_existing_openai_path() {
        assert_eq!(
            find(OPENAI_OAUTH).unwrap().instance_base_segment(),
            "extensions/builtin/openai-oauth"
        );
        assert_eq!(
            find(PONYTAIL).unwrap().instance_base_segment(),
            "extensions/builtin/ponytail"
        );
    }
}
