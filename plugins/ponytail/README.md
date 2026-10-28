# Ponytail 内置插件（适配层）

把 [DietrichGebert/ponytail](https://github.com/DietrichGebert/ponytail) 的
「懒惰阶梯」接进 dsh-xlink 的工作台。**本仓库只有适配层，上游内容不随应用分发。**

## 这里发什么、不发什么

| | 随应用分发 | 不分发 |
| --- | --- | --- |
| 内容 | `host/`（Host 适配器）、`adapter/`（更新脚本 + `ponytail-update` 技能）、`i18n/`（中文层）、`LICENSE` | 六个 `SKILL.md`、`hooks/`（规则注入与档位切换） |

上游内容由**桌面壳**在启用时从 npm 拉取（默认官方 registry，镜像只在传输层
不可达时兜底），校验 `dist.integrity` 后解包到实例的物化目录。Host 适配器从那里读。

**为什么不 vendor 进仓库**（前一版真的 vendor 了，还为此写过一套逐字落盘 + 哈希
锁定的脚本，那套脚本本身是对的）：vendor 会**制造两份内容** —— 仓库里钉死的那
一份，和用户手上的那一份。上游每改一次就要重发一版应用才能跟上，用户升级之前
一直用旧内容；更要命的是「面板上写的版本」与「工作台里跑的内容」会分叉，而两边
都不报错。顺带一个真实事故：第一版手写转录时丢了一个逗号，文件**大小完全相同**
（2901 字节）而内容有 122 字节位移 —— 当时 README 写着「逐字 vendor」。

## 中文层（`i18n/zh-CN/`）

上游只有英文版。用户要中文，而规则与技能正文都是**指令文本、整体进模型上下文**
的 —— 那是最该说人话的地方。所以适配层自带一份中文副本。

### 覆盖的是哪一层，为什么

工作台里 ponytail 的三处 prompt 全部来自物化目录里 `<root>/skills/<name>/SKILL.md`
这**同一批文件**：

| 出现在哪 | 谁在读 |
| --- | --- |
| 技能列表里的 description | `host/skills.js` 的 provider（解析 frontmatter） |
| 技能正文 | provider 的 `get()` |
| 会话开始注入的规则集 | 上游 `hooks/ponytail-instructions.js`，**直接读盘、绕过 provider** |

所以 `host/i18n.js` 在内核启动、**建 provider 之前**把物化目录里那六个文件换成
中文。第三处是每轮会话开头进上下文的那份；只在 provider 层拦截的话它仍是英文 ——
而覆盖文件本身一次就覆盖了三处，**不用改上游任何一行 JS**。

### 陈旧怎么判定

`i18n/zh-CN/manifest.json` 逐文件记着 sha256：这份译文所依据的那一版上游长什么样。
每次启动按指纹分四种情况：

| 当前物化文件的指纹 | 处置 |
| --- | --- |
| 等于现译文的 | 什么都不做（幂等） |
| 等于上次写下的译文（我们改了中文层） | 重写新译文 |
| 等于 manifest 记的上游指纹 | **先备份上游原件**再覆盖 |
| 三者都不是 | 上游已更新：**有原件就回落英文**，没有就一个字不动（当前那份就是新的上游原文） |

**陈旧靠指纹，不靠版本号**：物化目录里没有上游版本号可读（`skills/plugin.json`
是技能清单，根上的 `package.json` 是我们自己的），而指纹还多分辨出一件事 ——
上游改了内容但没发版。

### 改译文时必须守的两条

1. **档位标签保持字面的 `**lite**` / `**full**` / `**ultra**`。** 上游的
   `filterSkillBodyForMode` 按「表格首列的加粗标签」筛行，译成 `**全量**` 的话
   full 档会把那一行**整行删掉** —— 文件读起来完全正常，只有注入给模型的规则集少了
   一条。`test/i18n.test.mjs` 拿上游自己的规则逐档验这条。
2. **正文用全角冒号「：」。** 同一个过滤器还有一条 `- <词>: "…"` 的示范行规则，
   半角冒号加引号的普通条目会被误认成示范行，在其它档位被删。

### 这不是「两份内容」

上游正文仍然**只有一份**，随 npm 更新自动跟随。中文层是**我们自己的资产**，
并且明确记着自己停在哪一版上游 —— 上游一改，指纹对不上就回落英文并把陈旧报进
日志，而不是拿一份旧中文假装自己是最新的。

## 上游坐标与许可

| | |
| --- | --- |
| 上游 | <https://github.com/DietrichGebert/ponytail> |
| npm 包 | `@dietrichgebert/ponytail` |
| 许可 | MIT（`LICENSE`），Copyright © Dietrich Gebert |
| 原生支持 | 上游自己支持 16+ 个 agent，**能直接用的宿主不必装本适配层** |

本适配层代码与文档以 MIT 发布。第三方归属见 `LICENSE`。

## 什么时候更新

| 触发 | 时机 |
| --- | --- |
| 启用插件 | 同步一次；**失败会当场拦下启用**并说明原因（用户手上因此不会有一个「装上了但没有内容」的插件） |
| 每次启动工作台 | 检查一次，按 24 小时节流；失败走 30 分钟短退避、不消耗每日额度 |
| 面板「检查更新」 | 立即拉一次 |
| 工作台里 `/ponytail-update` | 立即拉一次，并汇报结果（技能会跑 `adapter/update.mjs`，它请求桌面壳的回环桥） |

同步**只写数据目录**（`<DSH_HOME>/plugins/builtin/ponytail/upstream/<版本>/`），
永远不改随包产物，也不碰实例的物化目录与接线行。失败时继续用手上那一版。

换内容要**重启工作台**才生效（工作台已经加载过旧的），面板与技能文案都这么说。

### 桥的凭据走三条路（env 那条实测不通）

壳把地址与令牌注进**内核进程**的环境。但 `/ponytail-update` 是**模型**在工作台里
用 shell 工具执行的，那条链看不到内核的 `DSH_*`（内核给子进程的环境会把这些名字
剔掉）—— 2026-10-12 用户实测这个脚本稳定报「找不到同步桥」，而同一时刻
`ps eww <内核 pid>` 里两个变量都在、桥也活着。

所以**落盘**才是主路。脚本 env 优先，盘上按这个顺序找两处：

| 落点 | 谁写的 | 什么时候会没有 |
| --- | --- | --- |
| `<插件根>/.xlink-bridge.json` | 壳（`bridge_credential.rs`），启动内核时 | 跑着还没这一步的旧壳；或物化目录刚被重建 |
| `<插件根>/../.xlink-bridge.json` | 插件自己（`host/bridge-credential.js`），加载时 | 内核不是由壳启动的（那时也没有桥可同步） |

两份都是 0600（Unix 上只有本人可读）。插件自己写那一份，是因为物化根**会被重建**：
我们改了 `skills/`，壳的物化摘要必然对不上，于是每次启动内核、面板每次「检查更新」
都会 `remove_dir_all` 重建整棵插件目录 —— 壳写的那份当场消失，而插件**不会**跟着
重新加载。写在**上一级**就躲开了那次重建。

内核正常停止时，插件把自己写的那一代收走（令牌对不上就不动，那是更新的一代）；
被强杀留下的死令牌会得到一句专门的话而不是通用的 401，并且**带着这枚令牌的每一份**
都会被删掉。

**这不放宽威胁模型**：令牌原本就在内核进程的环境变量里，而 macOS 上同用户本来就能用
`ps eww` 读到它。变的只是传递通道，不是能读到它的人。

## hook 怎么用

ponytail 的 hook 是一份**标准 Claude Code `hooks.json`**：`SessionStart` 注入规则集、
`UserPromptSubmit` 切档位、`SubagentStart` 传给子 agent。dsh 内核自带
`@deepseek-ai/dsh-hooks-claude-code` 这个桥，跑的就是 unmodified 的 Claude Code
hook 配置，所以**上游脚本一个字都不用改**。

挂它的只有一处：`host/index.js` 的 `apply()` 里 `ctx.plugin(...)`。桥**不扫插件目录**
（`configPath` 必填），所以必须显式挂。

### 桥不是每个内核版本都有（2026-10-12）

**它随不随内核安装，由内核上游自己的打包决定**：`0.2.1-alpha.1` 里它是
`@deepseek-ai/dsh` 的 `dependencies`（跟着内核一起装），`0.2.1-alpha.2` 里被降级成了
`devDependencies`（内核树里没有）。所以本插件把它声明成**可选 peer**，并且**动态
`import`**：

- 桥在内核树里 → 规则集注入与档位切换照旧；
- 桥不在内核树里 → **壳先从 npm 按内核版本补一份**（`peer_supply.rs`，`0.2.1-alpha.2`
  上实测补得到：`dsh-hooks-claude-code` 与它要的 `dsh-hook-protocol` 一起下，
  Node 能正常加载），补到之后这两项能力照常有；
- 连补也补不到（离线、registry 不可达）→ 插件**照常启用**、六个技能照常注册，只是
  这两项能力暂时缺席。壳在插件面板挂一条 ⚠ 如实说明（`PeerSpec::missing_note`），
  内核日志里也有一行 warn；联网后重启工作台会自动重试。

静态 `import` 会把「少一项能力」变成「整个插件加载失败」——技能一起消失，而用户从
界面上只看得出「开关拨不动」，所以那条路已经堵掉（`test/smoke.test.mjs` 钉住）。

核对过的三处（`@deepseek-ai/*` 0.2.1-alpha.1 实读）：

1. `SessionStartSource = 'startup'|'resume'|'clear'|'compact'`，与上游 matcher
   `startup|resume|clear|compact` 的四个词**逐字相同**；桥按 claude-code 方言把
   `a|b|c` 当**字面** alternation 比对。
2. 脚本吐 `hookSpecificOutput.additionalContext`，协议包解析该键后由桥
   `agent.inject(...)` 注入 —— 规则**只注入一次**，不是每轮重发。
3. `${CLAUDE_PLUGIN_ROOT}` 由桥的 `pluginRoot` 替换成物化目录。

**此前文档写「档位切换依赖别家 agent 的生命周期钩子，dsh 没有对应面」是错的**，
代价是白扔了一个核心能力。

两个必须记住的前提：

- **`hooks/package.json`（`{"type":"commonjs"}`）不能少**。上游脚本是 `require()`
  的 CommonJS，而插件根的 `package.json` 写着 `"type": "module"`；Node 按**离文件
  最近**的 package.json 判定模块类型。缺了它 `require()` 抛 ERR_REQUIRE_ESM，
  而 **hook 失败是非阻塞的** —— 症状是「配好了但什么都没发生」，日志里只有一行 warn。
  壳在物化时也会补这一份，于是两条路径的模块类型一致。
- **hook 需要 `node` 在 PATH 上**。命令串是 `node -e "require(...)"`，由内核 spawn。
  本仓起内核时用 `process::command_with_path` 的合并 PATH，Node 因此在 PATH 上。

## 默认关闭

技能与规则是指令文本、会整体进模型上下文。这与 OpenAI OAuth 那类「不登录就没副作用」
的内置插件不是一回事 —— 它第一条消息就改变行为。同名时**用户自己装的技能优先**
（bundled rank 600 低于壳管理的活动视图 300 与内核默认根 400/500）。

## 做不到的部分

- **statusline 徽标**（`ponytail-statusline.sh` / `.ps1`）：宿主没有可配置的
  状态栏，没有对应面。
- `ponytail-gain` 的收益记分牌依赖上游的基准数据与宿主侧统计展示。
- 注入文本的**第一行**仍是上游写死的英文档位声明（`PONYTAIL MODE ACTIVE — level: full`，
  在 `hooks/ponytail-instructions.js` 里拼的）。正文已经是中文；那行要中文化就得
  fork 上游那个脚本，不值当。

## 目录结构

```
plugins/ponytail/
  host/index.js                 Host 适配器：桥凭据落盘 + 应用中文层 + 注册 provider + 挂 hook 桥
  host/skills.js                发现 / 解析 / provider 工厂（无外部依赖，可被测试 import）
  host/i18n.js                  中文覆盖 / 回落（纯文件操作，可被测试 import）
  host/bridge-credential.js     把内核环境里的桥凭据落到物化根上一级（纯文件操作，可被测试 import）
  adapter/update.mjs            /ponytail-update 的执行体（请求壳同步；env 优先，盘上两处兜底）
  adapter/skills/ponytail-update/SKILL.md
  i18n/zh-CN/manifest.json      逐文件的上游 sha256（陈旧判定的唯一依据）
  i18n/zh-CN/skills/*/SKILL.md  六个技能的中文副本
  test/smoke.test.mjs           跑真实 provider，抄内核的 validateCandidate 校验
  test/i18n.test.mjs            覆盖 / 回落 / 幂等 / 陈旧；拿上游自己的裁剪规则验译文
  test/bridge-credential.test.mjs  凭据落在哪一级、0600、不重写、只收走自己那一代
  test/update.test.mjs          脚本与插件之间的契约：起个假桥，真跑一遍那个脚本
  package.json / LICENSE / README.md
```

`host/skills.js`、`host/i18n.js`、`host/bridge-credential.js` 与 `index.js` 分开是有
原因的：前三个**不依赖任何 `@deepseek-ai` 包**（rank 由调用方传入），所以
`npm run check` 里的测试能 import 它们并**真的把 provider、覆盖逻辑与凭据落盘各跑
一遍**。第一版把解析器写在 `index.js` 里，测试
只好读源码文本，于是「`list()` 少填了 source / rank / provider」一路绿灯直到真机
启动才炸。
