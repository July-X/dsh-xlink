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
    /// npm 包名。走 npm 而不是 GitHub tarball：默认 registry 是官方
    /// `registry.npmjs.org`（传输层不可达时才落到 npmmirror，见
    /// [`crate::pkg::registry`]；本机实测 GitHub 偶发 8 秒级延迟），而
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
/// 内置本地记忆的 id 常量。接线之后内核侧按它调回环桥取记忆。
pub(crate) const MEMORY: &str = "memory";
/// 内置官网对话引用的 id 常量。
///
/// P0 可行性验证阶段：它带一个**内核客户端模块**（工作台 `@` 菜单里的
/// 「官方对话」来源），用来证明「选择 → 异步读取 → 发送门控」这条链路在内核
/// 工作台里成立。当前候选与正文都是**固定夹具**，不连接任何官网账号。
pub(crate) const OFFICIAL_CHAT: &str = "official-chat";

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
        // 判据是「**内核树里必然在**」，两个内核版本实测都在——但**理由各不相同**：
        // `schemastery` 确实是 `@deepseek-ai/dsh` 的 `dependencies`；`dsh-llm` 在
        // `dsh` 的清单里是 `devDependencies`，靠 `dsh-base` / `dsh-sdk-minimal`
        // 的运行时依赖才躺进树里。**照 `dsh` 一份清单判会得出反的结论**，详见
        // `peers.rs` 的模块文档与下面 memory 那条。
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
        // 现在它缺失 = 只少「会话启动即注入」与档位切换；壳会先按**同版本**从
        // npm 补一份（`peer_supply.rs`，启用与启动工作台各试一次，补到就照常有
        // 那两项能力），补不到才走降级，这条文案就是说「补不到」时的那一句。
        peers: &[
            PeerSpec::required("dsh-skill"),
            PeerSpec::optional(
                "dsh-hooks-claude-code",
                "当前内核版本不带 @deepseek-ai/dsh-hooks-claude-code（上游把它从运行时依赖\
                 降级成了开发依赖），壳也没能从 npm 补上它（多半是这台机器现在连不上 \
                 registry）：技能照常可用，但「会话启动时自动注入规则」与 /ponytail 的\
                 档位切换暂时不可用。联网后重启工作台会自动重试；也可以换一个仍带这个包\
                 的内核版本（如 0.2.1-alpha.1），不必重装本应用。",
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
    BuiltinSpec {
        id: MEMORY,
        row_id: "xlink-memory",
        // host + client + package.json：**没有上游内容**（记忆正文在用户的实例目录
        // 里，不在安装包里）。client 是会话头右侧那枚「记忆 N」徽标（2026-10-15
        // 落地的 D6b）——它要成为一个内核客户端模块，靠的是 package.json 里那份
        // `dsh.client` 声明 + `exports["./client"]`，内核按 `dsh-client-modules`
        // 扫 host Loader 的 entries 找它，**不需要改接线行**（Loader 行的名字
        // 是路径形态，`locatePkgJson` 会沿它向上找到同目录的 package.json）。
        // 管理页（面板）仍然是壳自己的 Vue 面板，与 client 是两回事。
        entries: &["host", "client", "package.json"],
        // host 静态 import 的两个包：`dsh-tools`（`defineTool`）与 `dsh-llm`
        // （提取调用要用的 `BlockAssembler` / `createUserMessage`）。其余能力都经
        // `ctx` 拿：`systemPrompt` / `tools` / `workingDirectory` 都是内核自己的
        // 服务，不是包——**多列一个 peer 就是多一条会断的链**。
        //
        // `dsh-llm` 是 2026-10-15 补的，之前漏登记，而物化只按 peers 建链接：
        // 内核树不是物化目录的祖先，那一行 `import` 解析不到 → 整个插件
        // `failed to import`。因为 `xlink-memory` 不在内核的必需启动项里，症状
        // 只是内核日志多一行 `warning: 1 entry did not activate`，而工作台与
        // 记忆面板照常显示「已启用」——**注入、三个工具、提取全静默死掉**。
        // 现在由 `check-invariants` 的 `builtin-peer-imports` 逐包比对，别再漏。
        //
        // 判据是「**内核树里必然在**」，不是「`dsh` 的 `dependencies` 里有」：
        // `dsh-tools` 与 `dsh-llm` 在 `dsh` 的清单里都是 `devDependencies`，但
        // `dsh-base` 与 `dsh-sdk-minimal` 都把它们当运行时依赖（alpha.1 与
        // alpha.2 两棵树实测都在）。真正会变的是 `dsh-hooks-claude-code` 那种
        // 「上游自己不再装它」的包——那是 optional 的形态，见 `peers.rs`。
        peers: &[
            PeerSpec::required("dsh-tools"),
            PeerSpec::required("dsh-llm"),
        ],
        entry: "host/index.js",
        display_name: "本地记忆",
        summary: "把本机保存的长期偏好与项目约定接进会话：模型只能读取与提候选，批准在面板里",
        note: "开着「使用已批准的记忆」时，工作台的相关会话会带上一份有界、带来源的\
               记忆（每条注明是通用偏好还是本项目）。模型只能**读取**与**提出候选**：\
               候选必须你到面板点保存才生效，它改不了开关、也删不掉东西。记忆存在本机\
               实例目录里（面板底部有路径）；选远端模型时它会随请求一起出网。\
               必须始终生效、要随仓库走的规则写 `AGENTS.md`（它每次都在上下文里）；\
               需要能检索、能停用、能删掉的条目才放这里。",
        has_account_card: false,
        upstream: None,
    },
    BuiltinSpec {
        id: OFFICIAL_CHAT,
        row_id: "xlink-official-chat",
        // host + client + package.json：**没有上游内容**（官网正文不在安装包里，
        // 它属于用户自己的账号数据）。client 是**内核客户端模块**——工作台 `@`
        // 菜单里的「官方对话」来源，靠 package.json 里那份 `dsh.client` 声明 +
        // `exports["./client"]` 进浏览器模块图（与 memory 的徽标同一条通道）。
        //
        // **host 半边必须真的存在**：内核的 `dsh-client-modules` 是扫 host Loader
        // 的 entries 找带 `dsh.client` 声明的包——没有 host 半边，client/client.js
        // 一行都不会执行，而面板上照样显示「已启用」。
        entries: &["host", "client", "package.json"],
        // **空**：host/index.js 不静态 import 任何 `@deepseek-ai` 包（它只是
        // 客户端半边的锚点，刻意什么都不消费）。
        //
        // client 半边 inject 的那三个包（`dsh-client-ui-input-trigger` 等）是
        // **浏览器模块图**里的东西，由内核自己的 client 模块系统提供，不走
        // 这里建链接——与 memory 的先例一致（它 inject `dsh-client-ui-slots`，
        // peers 里也只有 host 侧那两个包）。多列一个 peer 就是多一条会断的链。
        peers: &[],
        entry: "host/index.js",
        display_name: "官网对话引用",
        summary: "把官网对话按需读进当前会话：工作台里 @ 选中，正文落成文件，模型自己去读",
        note: "从 DeepSeek、MiniMax、ChatGPT 官网读取最近会话，选中后保存固定正文。\
               请先在壳内官网页签登录；当前不保证取全历史。账户无法确认时拒绝引用，早期引用需重新选择。",
        has_account_card: false,
        upstream: None,
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

    /// 四个内置插件是 OpenAI OAuth、Ponytail、本地记忆与官网对话引用——新增第五个时
    /// 这条要一起改，它逼着下一个加内置插件的人把话说清楚（而不是悄悄多一个）。
    ///
    /// 2026-10-14 加第三个（`memory`）：它与前两个不是一类——**没有上游内容**
    /// （记忆正文在用户的实例目录里，不在安装包里），也没有工作台客户端模块
    /// （管理页是壳自己的面板）。它唯一的对外通道是回环桥上的三条记忆路由，
    /// 因此 `entries` 只有 `host` 与 `package.json`、`peers` 只有 host 真正
    /// `import` 的两个包（`dsh-tools` 与 `dsh-llm`）。
    ///
    /// 2026-10-20 加第四个（`official-chat`）：P0 可行性验证用。它与 memory
    /// 一样**没有上游内容**，但多了一枚**内核客户端模块**（工作台 `@` 菜单里的
    /// 「官方对话」来源）——所以 `entries` 里有 `client`，而 `peers` 是**空的**：
    /// 它的 host 半边不静态 import 任何内核包，client 半边 inject 的包由内核的
    /// 浏览器模块图提供，不走物化建链接那条路。
    #[test]
    fn the_registry_carries_the_shipped_builtins() {
        assert_eq!(BUILTINS.len(), 4, "改了登记表就在这里说一句为什么");
        assert!(find(OPENAI_OAUTH).is_some());
        assert!(find(PONYTAIL).is_some());
        assert!(find(MEMORY).is_some());
        assert!(find(OFFICIAL_CHAT).is_some());
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

    /// 新登记的 `official-chat` 必须能被**真实物化链**物化出来。
    ///
    /// 它是第四个内置插件，登记表的任何一处写错，症状都不是「报错」而是
    /// **静默半死**：物化目录少一份文件 → 面板显示「已启用」而 client 半边
    /// 一行都不执行；接线行 id 与包名对不上 → 内核按 settingsNs 认不出这个
    /// 插件；peers 多列一个包 → 换台机器就装不上。
    ///
    /// 这条走的是**生产同一条路**（`ensure_wired` → 物化 → 追加接线行），
    /// 不是照着形状断言字符串。夹具的内核树用假版本号（指纹只影响目录名），
    /// 插件源按 `spec.entries` **现生成**——写死文件集的话，登记表改了文件集
    /// 而夹具不变，验的就不是登记的那一份。
    #[test]
    fn the_official_chat_spec_materializes_through_the_real_chain() {
        let spec = find(OFFICIAL_CHAT).expect("official-chat 必须在登记表里");
        // 行 id = 包名：内核按 settingsNs 认插件，对不上就拿不到命名空间。
        assert_eq!(spec.row_id, "xlink-official-chat");
        // host 半边不静态 import 任何内核包 → peers 为空。**多列一个 peer 就
        // 多一条会断的链**（旧内核装不上那个包时，插件根本装不进去）。
        assert!(
            spec.peers.is_empty(),
            "official-chat 的 host 不 import 任何内核包，peers 应当为空"
        );

        let root = std::env::temp_dir().join(format!("oop-builtin-oc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let home = root.join("home");
        std::fs::create_dir_all(home.join("profiles/web")).unwrap();

        // 夹具插件源：按 spec.entries 现生成。
        let source = root.join("source");
        for entry in spec.entries {
            if *entry == "package.json" {
                std::fs::write(
                    source.join("package.json"),
                    format!("{{\"name\":\"{}\",\"version\":\"0.1.0\"}}", spec.row_id),
                )
                .unwrap();
            } else {
                std::fs::create_dir_all(source.join(entry)).unwrap();
                std::fs::write(source.join(entry).join("index.js"), "// fixture").unwrap();
            }
        }
        // 夹具内核树：dsh + cordis 两个包的版本决定指纹。
        let kernel = root.join("kernel");
        for (pkg, version) in [("dsh", "0.2.1-alpha.1"), ("cordis", "4.0.5-alpha.1")] {
            let dir = kernel.join("node_modules/@deepseek-ai").join(pkg);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join("package.json"),
                format!("{{\"version\":\"{version}\"}}"),
            )
            .unwrap();
        }

        let wired =
            crate::plugins::builtin::ensure_wired(spec, &source, &home, "web", &kernel).unwrap();
        assert!(wired.row_added, "首次接线必须真的写进 patch");
        assert_eq!(wired.plugin_version, "0.1.0");
        // 物化目录：<home>/extensions/builtin/<id>/<版本>/<指纹>/compat-a。
        // **目录名用 id（official-chat），不是行 id（xlink-official-chat）**——
        // 两者混了不会报错，只是物化到了另一个地方。
        assert_eq!(
            wired.dir,
            home.join(
                "extensions/builtin/official-chat/0.1.0/dsh-0.2.1-alpha.1-cordis-4.0.5-alpha.1/compat-a"
            )
        );
        for entry in spec.entries {
            assert!(
                wired.dir.join(entry).exists(),
                "物化缺 {entry}：面板会显示「已启用」而 client 半边一行都不执行"
            );
        }
        // 完成标记：物化的幂等判定靠它（源摘要 / 指纹 / 落位后摘要三项）。
        assert!(wired.dir.join(".xlink-materialized.json").is_file());
        // peers 为空 → 不建 peer 链接目录。
        assert!(
            !wired.dir.join("node_modules").exists(),
            "peers 为空时不该出现 node_modules"
        );

        // 接线行：id 是包名，name 相对 profile 目录、直指 host/index.js。
        let patch = std::fs::read_to_string(home.join("profiles/web/cordis.patch.yml")).unwrap();
        let entry_rel = crate::plugins::builtin::materialize::relative_entry(
            &home.join("profiles/web"),
            &wired.dir.join(spec.entry),
        );
        assert!(patch.contains("- id: xlink-official-chat"), "{patch}");
        assert!(patch.contains(&entry_rel), "{patch}");
        assert!(entry_rel.ends_with("host/index.js"), "{entry_rel}");

        // 幂等：再来一次不重复追加（面板上反复点开关是常态）。
        let again =
            crate::plugins::builtin::ensure_wired(spec, &source, &home, "web", &kernel).unwrap();
        assert!(!again.row_added, "重复接线不得再追加一行");
        let after = std::fs::read_to_string(home.join("profiles/web/cordis.patch.yml")).unwrap();
        assert_eq!(after, patch, "重复接线不得改动 patch");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 手动驱动：把 `official-chat` 经**真实物化链**物化进一个指定实例，
    /// 供内核工作台冒烟用——内核读到的那条接线行与那份物化产物都出自这条链，
    /// 不再是照着源码手写的复刻件。
    ///
    /// `#[ignore]`：它要写盘、且路径是本机的，CI 不该跑。
    ///
    /// 用法（PowerShell）：
    /// ```powershell
    /// $env:DSH_XLINK_G1_DSH_HOME     = "$env:TEMP\dsh-xlink-g1\dsh-home"
    /// $env:DSH_XLINK_G1_KERNEL_ROOT  = "$env:TEMP\dsh-xlink-g1\dsh\desktop\kernels\0.2.1-rc.2"
    /// cargo test --manifest-path src-tauri/Cargo.toml --lib \
    ///   the_official_chat_spec_materializes_into_a_real_instance -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "手动驱动：写盘到指定实例并依赖本机路径，CI 不跑"]
    fn the_official_chat_spec_materializes_into_a_real_instance() {
        let dsh_home = std::path::PathBuf::from(
            std::env::var("DSH_XLINK_G1_DSH_HOME")
                .expect("必须给 DSH_XLINK_G1_DSH_HOME（实例根；内核读的 profile 目录在它下面）"),
        );
        let kernel_root = std::path::PathBuf::from(
            std::env::var("DSH_XLINK_G1_KERNEL_ROOT")
                .expect("必须给 DSH_XLINK_G1_KERNEL_ROOT（内核树，只读；指纹从它算）"),
        );
        let spec = find(OFFICIAL_CHAT).expect("official-chat 必须在登记表里");

        // 插件源优先取**打包资源**（生产真正喂给物化链的那份，带 manifest，
        // 于是 verify_manifest 也会被跑到），没有就退回仓库源码（dev 回退口）。
        let packaged = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("builtin-plugins")
            .join(spec.id);
        let source = if packaged.join("package.json").is_file() {
            packaged
        } else {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("plugins")
                .join(spec.id)
        };
        assert!(
            source.join("package.json").is_file(),
            "插件源不存在：{}",
            source.display()
        );

        let wired =
            crate::plugins::builtin::ensure_wired(spec, &source, &dsh_home, "web", &kernel_root)
                .expect("真实物化链应当成功");
        let patch_path = dsh_home.join("profiles/web/cordis.patch.yml");
        let patch = std::fs::read_to_string(&patch_path).unwrap_or_default();
        println!("G1-MATERIALIZE source={}", source.display());
        println!("G1-MATERIALIZE dir={}", wired.dir.display());
        println!("G1-MATERIALIZE fingerprint={}", wired.fingerprint);
        println!("G1-MATERIALIZE plugin_version={}", wired.plugin_version);
        println!("G1-MATERIALIZE row_added={}", wired.row_added);
        println!("G1-MATERIALIZE patch={}", patch_path.display());
        println!("G1-MATERIALIZE patch_body=\n{patch}");
        assert!(patch.contains("- id: xlink-official-chat"), "{patch}");
        assert!(wired.dir.join("host/index.js").is_file());
        assert!(wired.dir.join("client/client.js").is_file());
        assert!(wired.dir.join("package.json").is_file());
    }
}
