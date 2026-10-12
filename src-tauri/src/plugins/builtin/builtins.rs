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
//! - [`BuiltinSpec::entry`]：接线行 `name` 指向的入口（相对物化目录）。
//! - 显示三件（`display_name` / `summary` / `note`）：面板那一行要的文案。
//!   后端出、前端画，避免同一句话在前端组件里再抄一份（抄的那份不参与
//!   Rust 侧测试，也不会随文案改动而报错）。
//! - [`BuiltinSpec::has_account_card`]：工作台设置页里有没有该插件自绘的
//!   账户卡。OpenAI 有（它要把 OAuth 走通），ponytail 没有（它不发请求）。

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
    /// 从内核树 peer 进来的 `@deepseek-ai` 包。
    pub(crate) peers: &'static [&'static str],
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
        peers: &["dsh-llm", "schemastery"],
        entry: "host/index.js",
        display_name: "OpenAI-OAuth-Plugin",
        summary: "随应用交付的 OpenAI 套餐接入；启用不需要联网安装，关闭不删除账号",
        note: "把 ChatGPT 套餐接进当前内核的模型列表。令牌只存在本机（系统凭据库），\
               不进工作台页面；关闭只摘接线，不删账号，随时可再启用。",
        has_account_card: true,
    },
    BuiltinSpec {
        id: PONYTAIL,
        row_id: "xlink-ponytail",
        // 只有 host + 技能正文：它没有工作台内的卡片，也没有本地化文案
        // （技能正文是英文原文，见 `plugins/ponytail/README.md` 的归属说明）。
        entries: &["host", "skills", "package.json", "LICENSE"],
        // host/index.js 只 import 一件事：`BUNDLED_SKILL_RANK`（bundled 技能的
        // 优先级 600）。不 import 就不该链——多链一个包等于多一条会断的链。
        peers: &["dsh-skill"],
        entry: "host/index.js",
        display_name: "Ponytail",
        summary: "把「懒惰的资深开发者」这套规则接进工作台：能少写的代码就少写",
        note: "来自 DietrichGebert/ponytail（MIT）的六个技能：写代码时先找现成的、再考虑\
               标准库，最后才自己写；另有评审、全仓体检、欠账清单三个命令型技能。\
               技能是指令文本、会进入模型上下文，因此**默认关闭**，由你决定何时打开。\
               不支持的部分：它的档位切换与会话级常驻依赖别家 agent 的生命周期钩子，\
               这里只有规则与按需调用。",
        has_account_card: false,
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
