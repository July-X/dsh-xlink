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
| `peers` | 从内核树链进来的 `@deepseek-ai` 包 | 只列 host 真正 `import` 的 |
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
2. **说清做不到的部分。** Ponytail 上游的档位切换与会话级常驻依赖别家 agent 的生命
   周期钩子，dsh 没有对应面。面板的 `note` 里必须写明，否则宣传口径大于实际能力。

## 新增一个内置插件的清单

1. `builtins.rs` 的 `BUILTINS` 加一条
2. `scripts/prepare-builtin-plugins.mjs` 的 `BUILTINS` 加一条（`entries` 必须与 1 一致）
3. `plugins/<id>/package.json`（`name` 必须是 `xlink-<id>`；`private: true`；
   `main` 指向 host 入口）
4. `plugins/<id>/host/index.js` 导出 `{ apply, inject, name }`
5. 第三方内容补 `LICENSE` 与 `README.md`（出处、版本、上游 tree）
6. 跑 `npm run prep:builtin && npm run check`

命令面已泛化（`builtin_status` / `builtin_set_enabled`），`permissions/app-commands.json`
通常不用动。