# 内置插件（Builtin Plugins）

随应用交付的插件：**不进社区中央库**，没有安装 / 卸载 / 更新语义，只有「启用意图」
一个开关。资源随安装包分发，启用时按内核指纹物化进实例自己的目录，内核安装树保持
只读。2026-10-12 起通道按 **N 个**插件设计（此前只有 OpenAI OAuth 一个，
`openai-oauth` 这个字面量散在 8 处），当前两个：OpenAI OAuth 与 Ponytail。

> OpenAI OAuth 那个插件自身的协议、授权与联调见
> [openai-oauth-design.md](openai-oauth-design.md) 与
> [openai-oauth-development-plan.md](openai-oauth-development-plan.md)。
> 本文只讲**通道**：登记表、物化、接线、状态、命令面。

## 登记表是唯一事实源

`src-tauri/src/plugins/builtin/builtins.rs` 的 `BUILTINS: &[BuiltinSpec]` 回答
「这个版本的应用带哪几个内置插件」，每个 spec 有这些字段：

| 字段 | 是什么 | 为什么在这一层 |
| --- | --- | --- |
| `id` | 物化目录名 / 资源目录名 / 面板行 key | 改它等于换一根数据根 |
| `row_id` | `cordis.patch.yml` 行 id = settingsNs = 包名 | 内核按它认插件 |
| `entries` | 物化进实例的运行时文件集 | 按插件各列各的（见下） |
| `peers` | 从内核树链进来的 `@deepseek-ai` 包（必需 / 可选见下） | 只列 host 真正 `import` 的 |
| `entry` | 接线行 `name` 指向的入口 | 通常 `host/index.js` |
| `display_name` / `summary` / `note` | 面板上的三段文案 | **由后端下发**，前端不抄 |
| `has_account_card` | 工作台设置页里有没有自绘账户卡 | 能力开关，不是样式 |

**新增内置插件只改这一处**，其余模块（资源管线 / 物化 / 接线 / 状态 / 命令 /
面板）都从这里读。面板那一侧不需要改代码：行是 `v-for`，文案与能力随状态一起下发。

### 两份清单必须逐项一致

`BuiltinSpec::entries`（Rust，物化时复制）与 `scripts/prepare-builtin-plugins.mjs`
的 `BUILTINS[].entries`（JS，打包时复制）是同一份东西的两门语言。一个 Rust、一个
Node，实现上跨不过去，所以由 `scripts/dev-builtin.test.mjs` 钉死。

两侧不一致时的症状很难定位：「多出来」的那份**打包了但没物化**，看着一切正常；
「少掉」的那份物化时报「应用资源可能不完整，请重新安装 dsh-xlink」——把一个源码侧
的错误报成用户的安装包坏了。

### `entries` / `peers` 为什么要按插件各列各的

文件集随插件而变：OpenAI OAuth 有 `client/`（工作台里的账户卡）与 `locales/`，
Ponytail 两样都没有。写成一份共用清单就得为不存在的目录编造条目。

peer 同理：多链一个包就是多一条会断的链——旧内核装不上那个包时物化直接失败，于是
「加了个用不到它的 peer」会让这个插件在新旧内核上彻底装不进去。

### peer 分必需与可选（2026-10-12 起）

每个 peer 在登记表里都要回答一个问题：**内核树里没有它，这个插件还能不能装**。
peer 的类型与缺失探针在 `src-tauri/src/plugins/builtin/peers.rs`，登记表（谁带哪几个 peer）
在 `builtins.rs`——分开是为了让「加一个内置插件」与「改 peer 的全局语义」不撞在同一个文件里。

| | 缺失时的行为 | 例子 |
| --- | --- | --- |
| 必需（`PeerSpec::required`） | 物化失败，启用被拦下 | `dsh-skill`、`dsh-llm`、`schemastery`——都是 `@deepseek-ai/dsh` 的**运行时依赖**，每个内核版本都带 |
| 可选（`PeerSpec::optional`） | **先从 npm 补一份**（见下节）；补不到才跳过链接、物化成功、host 降级、面板如实标注 | `dsh-hooks-claude-code`——内核上游自己的打包决定，`0.2.1-alpha.1` 带、`0.2.1-alpha.2` 不带 |

判据是「这个包是不是内核**必然**带」，不是「host 需不需要它」。上游会把包从
`dependencies` 降级成 `devDependencies`（`dsh-hooks-claude-code` 就是这样），
那一刻内核树里就没有它了。把可选包写成必需，代价是**整个插件（含它自己的技能）
一起装不进去**，而旧报错还把用户支去「内核版本」页重装一个没坏的内核——2026-10-12
用户在 `0.2.1-alpha.2` 上点 Ponytail 的开关，得到的就是这条。

可选 peer 缺失是**能力降级**，所以每一处都要说出口，三处缺一不可：

1. `PeerSpec::missing_note` —— 面板那条 ⚠ 的 tooltip（文案由后端出）。
2. `BuiltinPluginStatus::missing_peers` —— 状态命令与 `builtin_set_enabled` 的返回值
   都带它；启用成功但降级时，前端发的是 warning toast 而不是「已启用」。
3. host 自己的降级日志 —— 工作台内核日志里那一行说清少的是哪项能力。

探针（`missing_optional_peers`）判的是「内核树里有**或**壳补过」：补过之后能力是
有的，再挂 ⚠ 就是谎报，用户会去点「换内核版本」换一个根本不需要换的东西。

**host 不许静态 `import` 可选 peer**：静态 import 在内核不带那个包时会让整个插件
加载失败（技能一起消失），降级也就无从谈起。Ponytail 的
`loadHookBridge()` 就是这条的形状。

### 缺的可选 peer 由壳从 npm 补一份（2026-10-13 起）

`src-tauri/src/plugins/builtin/peer_supply.rs`：内核树里没有的可选 peer，壳按**内核
版本**（读内核树里 `dsh` 自己的 `package.json`）从 npm 取一份，落进
`<DSH_HOME>/plugins/builtin/<id>/peers/<包>@<版本>/`，物化时**复制**进插件的
`node_modules/@deepseek-ai/`。四条纪律：

- **只补树里没有的**：树里有就链树里的原件（那才是内核自己的实现）。
- **只取源，不碰内核树**：与上游内容（`upstream/`）同一条纪律，根 AGENTS 的
  「绝不往已安装的内核目录里写任何东西」照旧成立。
- **版本严格等于内核版本**：这些是内核 monorepo 的锁步子包，实物的
  `peerDependencies` 逐包钉死同一个版本号；取 `latest` 会装进一份与树里其余包
  不同代的实现。
- **失败是软失败**：`ensure_soft` 的返回值刻意是「一句日志」而不是 `Result`——
  `ensure_wired` 的 Err 会经 `kernel_adapter` 变成 `AdapterError::Io` **阻断工作台
  启动**，而供货只是补一项能力，离线机器不该因此起不来。

**为什么必须复制而不是链接**：Node 解析 import 用的是**真实路径**。链接过去的包
真实位置在供货目录，那里的上一层没有 `node_modules`，它自己的
`import '@deepseek-ai/dsh-hook-protocol'` 会当场失败，症状是「补了却还是不生效」。
所以：**补的包与它自己缺的 peer 一起补**（闭包），复制进插件目录；树里有的 peer
照旧链原件。

失败后 30 分钟内不再试（`<peers>/.attempt-<包>@<版本>.json`）：工作台每次启动都会
走 `ensure_wired`，没有节流的话离线用户每启动一次都要重付一遍网络超时。

## 启用 / 停用

`builtin_set_enabled { plugin, enabled }` 的事务顺序（开发计划 §4.1）：

1. 实例判据：dsh 族 → 实例存在 → 未被另一壳占用 → **内核已停止**（实例级判据，
   不是「本壳工作台」）
2. 变更前快照（`snapshot::record`，打点失败只落日志，不阻断用户操作）
3. 启用：`ensure_wired` = 物化（幂等）→ 追加自有接线行（幂等）；
   停用：`ensure_unwired` = 精确摘掉自有接线行，**不删**物化资源与账号
4. 意图落盘（按壳模式 / profile 分键）

事务失败时意图**不落盘**，开关状态与接线保持一致。认不出的插件 id 直接报错，
**不回退到任何默认插件**——回退会让一次拨错的开关作用在别的插件上，且无任何症状。

### 状态文件按插件分文件

`<DSH_HOME>/extensions/builtin/<id>/state.json`，按「壳模式 / profile」分键。

分文件（而不是一份文件按 id 分键）是刻意的：openai-oauth 既有用户的路径原地不动。
分键的话整个搬家，升级后用户的启用意图**静默变成「未启用」**，界面上看不出任何异常。

## 接线

实例 `cordis.patch.yml` 里每个内置插件一行：

```text
- insert:
    - id: xlink-ponytail
      name: "../../extensions/builtin/ponytail/0.1.0/<指纹>/compat-a/host/index.js"
```

`row_id` 是**参数**不是常量：`wiring.rs` 里的形状识别（哪几行算我们写的）与 id
无关，只有「找哪一行」有关。写死成模块常量时，摘行会顺着**别人的** id 去删——
症状是「关掉 A 把 B 也关了」。

## 启动前刷新

每次启动内核前，`runtime::refresh_enabled` 遍历**全部**内置插件，把已启用的刷新到
当前壳携带的版本（应用升级后自动生效，无需用户拨开关）。停用意图不触碰资源。
每个插件各记一条事件（`builtin-<id>-refresh`），查日志时不必从文案里认。

## 分发第三方技能类插件的两条硬要求

以 Ponytail 为例（[plugins/ponytail/README.md](../../../plugins/ponytail/README.md)）：

1. **默认关闭。** 技能是指令文本、会整体进模型上下文；这与 OAuth 那类「不登录就没
   副作用」的内置插件不是一类——它第一条消息就改变行为。同名时用户自己装的技能优先
   （bundled rank 600 低于壳管理的活动视图 300 与内核默认根 400/500），所以「随应用
   交付」不等于「强制生效」。
2. **说清做不到的部分。** 面板的 `note` 里必须写明，否则宣传口径大于实际能力。

## 有上游的插件：内容不在安装包里（2026-12 起）

`BuiltinSpec::upstream` 非空时，**随包只发适配层**，内容由壳在运行时拉取：

- `plugins/<id>/` 里只有 `host/` + `adapter/` + `package.json` + `LICENSE`；
  上游的技能正文与 hook 脚本**不在仓库里**。
- 取源走 npm（`@dietrichgebert/ponytail`），复用 [`crate::pkg::fetch`]：npm 文档 →
  下载 → `dist.integrity` 校验（fail-closed）→ 防路径穿越解包。默认 registry 是
  官方 `registry.npmjs.org`，`npmmirror` 只在传输层不可达时兜底——本机实测 GitHub
  偶发 8 秒级延迟。
- 落点是 `<DSH_HOME>/plugins/builtin/<id>/upstream/<版本>/`，**按版本分目录**。
  同步只写数据目录，不碰物化目录与接线行，所以「用了新版」不需要重新接线。

### 为什么不能 vendor 进仓库

vendor 会**制造两份内容**：仓库里钉死的那一份，和用户手上的那一份。上游每改一次
就要重发一版壳才能跟上，用户升级之前一直用旧内容；更要命的是「面板上写的版本」与
「工作台里跑的内容」会分叉，而两边都不报错。实测事故：手写转录时丢了一个逗号，
文件**大小完全相同**（2901 字节）而内容有 122 字节位移。

### 四个触发点

| 触发 | 节流 | 失败时 |
| --- | --- | --- |
| 启用插件 | 无（首次必然拉） | **拦下启用**并说明原因——不能让用户拿到一个「装上了但没有内容」的插件 |
| 启动工作台 | 24 小时 | 30 分钟短退避（不吃每日额度），继续用旧版本 |
| 面板「检查更新」 | 无（`force`） | 失败落在状态里由面板显示；**不是命令失败** |
| `/ponytail-update` | 无（`force`） | 技能如实转述失败，不说成「已是最新」 |

「从没试过」必须立刻放行：写成 `now - 0 >= 24h` 的话，刚开机的新装用户永远等不到
首次同步，而状态文件是空的、面板上什么异常都看不出来。

### 内核里的技能怎么让壳去同步

`/ponytail-update` 跑在**内核进程**里，同步必须在**壳这一侧**做（出网纪律、代理路由、
SRI 校验只有壳有）。两者之间用一条只绑回环的桥（`builtin/bridge.rs`，形状与
`openai/bridge.rs` 相同：Bearer 令牌，按「(族, 实例)」一代一换），端点只有一个：

```text
GET /v1/builtin/sync?plugin=ponytail
```

**用 GET + 查询串而不是 POST**：本仓的请求读取原语只保证读到请求头，body 可能在同一次
`read` 里到达也可能没有（TCP 无消息边界）——拿半截缓冲当请求体，在客户端分两次发的写法
下会读到空串，而症状是「插件名缺失」，查不出原因。

## 新增一个内置插件的清单

1. `builtins.rs` 的 `BUILTINS` 加一条
2. `scripts/prepare-builtin-plugins.mjs` 的 `BUILTINS` 加一条（`entries` 必须与 1 一致）
3. `plugins/<id>/package.json`（`name` 必须是 `xlink-<id>`；`private: true`；
   `main` 指向 host 入口）
4. `plugins/<id>/host/index.js` 导出 `{ apply, inject, name }`
5. 第三方内容写清出处与许可（README.md + LICENSE）
6. 有上游的：`BuiltinSpec::upstream` 填 npm 包名，并确认 `peers` 里列了 host 真正
   import 的每个 `@deepseek-ai` 包（多一个就是多一条会断的链）
7. 跑 `npm run prep:builtin && npm run check`

命令面已泛化（`builtin_status` / `builtin_set_enabled` / `builtin_check_upstream`），
`permissions/app-commands.json` 通常不用动。面板那一侧**不需要改代码**：行是 `v-for`，
文案、版本与能力都由后端下发。