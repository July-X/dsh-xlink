# 内嵌 OpenAI OAuth 插件开发计划

日期：2026-10-08。状态：P0–P4 已实现并通过离线验证，P5 桌面侧已落地（工作台账户卡与实机点击验收待做），P6 未开始——分期明细见文末「实现状态」。产品目标、用户流程和兼容窗口见[设计文档](openai-oauth-design.md)。本文给出模块边界、接线协议、实施顺序与验收要求。

## 1. 基线与实现纪律

当前仓库由 Rust/Tauri 管理内核进程，Vue 管理桌面壳界面，dsh 工作台使用内核自己的客户端插件体系。新增功能沿用这些边界。

本次阅读确认的实现入口：

| 现有位置 | 用途与改造边界 |
| --- | --- |
| `src-tauri/src/kernel/kernel_adapter.rs` | dsh 实例准备入口；只委托内嵌插件准备，不塞入 OAuth 主体 |
| `src-tauri/src/kernel/lifecycle.rs` | 目标版本与进程启动；为本次进程传递桥接配置、回收连接 |
| `src-tauri/src/plugins/center.rs` | 社区插件管理；不得把内嵌插件伪装成社区条目 |
| `src-tauri/src/pkg/net_proxy.rs` | OpenAI 出网复用 `net_proxy::routes()` 与传输错误分类，不另建网络栈 |
| `src-tauri/src/shell/paths.rs`、`instance.rs` | 路径、实例运行判据、壳模式隔离 |
| `src-tauri/src/diagnostics/` | 变更前快照、临时验证、隔离与恢复 |
| `ui/src/plugins/`、`ui/src/shell/bridge.js` | 内嵌区域、动作状态和 Tauri 通信 |
| `src-tauri/tauri.conf.json` | 登记内嵌资源；当前只有 patches 资源，不能认为插件已被打包 |
| `scripts/check-code-budget.mjs` | 新源码文件登记，遵守现有预算，不上调大文件预算 |

当前社区接线会运行 pnpm 并清理社区物化目录（实现细节见 [plugin-internals.md](plugin-internals.md)，用户可见设计见 [plugin-management.md](plugin-management.md)）：物化目标是实例 `extensions/plugins/<id>/`，接线状态记录在实例 `extensions/wiring.json`，profile manifest 由 `ensure_wiring` 链改写。新增离线接线必须与这两处的调和规则共同验证，不能仅在启动前追加一行配置，然后被后续社区同步删掉。

改后端先读 [Rust 约定](../../../src-tauri/AGENTS.md)，改面板先读 [UI 约定](../../../ui/AGENTS.md)。所有测试使用自己的临时数据目录和 `scoped_xlink_home` guard（作用域守卫）。用户真实实例、凭据、系统登录项和安装树不能作为测试夹具。

### 1.1 当前兼容窗口快照

活数据：随 P0 调查与每次发布验收重新计算并回写本表；[设计文档](openai-oauth-design.md)只保留计算规则。快照是待验证对象，不构成兼容声明。P0 静态接口调查的逐版本证据见 [P0 发布包调查](openai-oauth-p0-findings.md)。

更新于 2026-10-08，查询[官方版本记录](https://registry.npmjs.org/@deepseek-ai%2Fdsh)，P0 已完成（接口表 + 三版本离线接线原型）：

| 内核版本 | 官方发布时间（UTC） | 验证状态 |
| --- | --- | --- |
| `0.2.1-alpha.1` | 2026-10-03 04:53:22 | 未验证 |
| `0.2.0-rc.2` | 2026-09-29 09:56:27 | 未验证 |
| `0.2.0-rc.1` | 2026-09-28 12:34:03 | 未验证 |

## 2. 建议模块布局

以下是待新增目录，不是现有实现。保持资源、宿主兼容层与凭据服务分开。

```text
plugins/openai-oauth/                  # 应用自有插件源码与测试
  host/                               # dsh 模型、消息、工具、回放转换
  client/                             # 内核模型设置与套餐提示
  compat/                             # 已验证的内核接口适配
  locales/                            # 中文与英文文案
src-tauri/src/plugins/builtin/         # 离线资源、兼容选择、接线事务
src-tauri/src/openai/                  # OAuth、凭据库、目录、网络、桥接
src-tauri/resources/builtin-plugins/   # 构建生成的插件资源入口
scripts/                              # 资源打包、兼容测试入口
ui/src/plugins/                       # 内嵌开关与兼容状态
```

`openai/` 可按 `auth`、`vault`、`catalog`、`request`、`bridge` 拆分。入口只组织生命周期，不建立万能服务类。Host 与 Client 共享纯协议类型和校验；客户端代码不能导入宿主密钥实现。

构建时保留插件自身依赖和许可声明；Cordis、dsh 服务包及 React 作为宿主依赖或客户端外部模块。不要打包第二份宿主服务。最终文件列表、摘要和外部依赖必须由产物检查得出，不能只检查源码中的 manifest（清单）。[dsh 插件规范](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/preset/agent-preset/skills/cordis-plugin-development/references/host-plugin.md)、[客户端规范](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/preset/agent-preset/skills/cordis-plugin-development/references/ui-plugin.md)

## 3. 路径与状态所有权

所有数据路径经 `paths` 解析并跟随 `DSH_XLINK_HOME`，不在代码里展开固定的用户家目录。

| 数据 | 建议位置 | 所有权 |
| --- | --- | --- |
| 只读内嵌资源 | 应用资源目录 `builtin-plugins/openai-oauth/` | 当前应用构建 |
| 启用意图、接线记录 | 实例 `extensions/builtin/openai-oauth/state.json`，按 profile 与壳模式分键 | 内嵌接线模块 |
| 本地插件产物 | 实例 `extensions/builtin/openai-oauth/<plugin-version>/<kernel-fingerprint>/<compat-id>/` | 指纹对应的不可变产物 |
| 本次进程连接状态 | 实例 `runtime/openai-oauth/` | 当前启动代次；不保存桥接密钥 |
| 脱敏账户索引、目录缓存 | `shell/<mode>/openai-oauth/`，按实例与 profile 分键 | Rust OpenAI 服务 |
| 加密凭据文件 | 同一模式下的 OpenAI 服务目录 | Rust 凭据库；排除诊断与快照 |
| 加密文件密钥 | 系统凭据库，按 home 指纹、模式与账户注册分键 | 当前用户的系统凭据库 |

相同源码资源可以只读共享，peer 链接和可变状态不能跨实例或内核指纹共享。release 与 dev 的授权默认分开，不自动读 Codex CLI（命令行工具）、浏览器或另一个实例的登录。

同一实例被另一壳占用时拒绝接线变更。关闭插件只移除本插件拥有的激活项，不递归删除资源或账户。旧产物清理作为后续独立任务，在所有引用已释放后进行。

## 4. 状态与命令契约

### 4.1 内嵌开关

`requestedEnabled` 表示用户已保存的意图；`effectiveEnabled` 表示本次内核确实加载。另报 `compatibility`、`pluginVersion`、`compatId`、`kernelFingerprint`、`reason` 和 `logPath`。

开关默认关闭。停止的实例启用时：获取实例锁 → 留变更前快照 → 校验兼容与资源 → 准备候选目录 → 验证接线 → 提交自有配置 → 保存意图。未启动时 `effectiveEnabled` 仍为 false；收到 Host 握手才变为 true。关闭也在停止状态执行，对自有项做精确移除。

加载状态至少区分 `disabled`、`prepared`、`active`、`incompatible`、`quarantined` 和 `failed`。授权状态另分 `signed-out`、`authorizing`、`authorized`、`reauth-required`；模型连接状态不能由“有一份令牌”推导。

加载状态的转移与触发者（用户 = 桌面壳动作，系统 = 启动、握手或运行期）：

| 从 | 事件 | 到 | 约束 |
| --- | --- | --- | --- |
| disabled | 停止状态下启用事务提交（用户） | prepared | `requestedEnabled=true`；`effectiveEnabled` 仍为 false |
| disabled | 兼容判定不通过（事务内，系统） | incompatible | 开关保持关闭；结论可按内核指纹缓存 |
| prepared | 本代次启动收到 Host 握手（系统） | active | 仅此状态 `effectiveEnabled=true` |
| prepared | 停止状态下关闭事务（用户） | disabled | 精确移除自有接线；资源与账户保留 |
| prepared / active | 启动验证失败、握手超时或桥接致命错误（系统） | quarantined | 保留 `requestedEnabled`；在途请求按 §8.3 结算 |
| active | 工作台停止（系统） | prepared | 撤销桥接令牌；下一代次重新握手后才回 active |
| quarantined | 恢复事务成功（用户显式触发） | prepared | 不自动恢复 |
| quarantined | 关闭开关（用户） | disabled | 清自有接线；账户与资源保留 |
| 任意 | 事务失败且回滚无法确认（系统） | failed | `reason` 与 `logPath` 指明失败步骤；重试重新走启用事务 |

事务失败但回滚成功时，状态回到事务前的已提交值（`disabled` 或 `prepared`），失败详情由 `reason` 与 `logPath` 另报，不新设状态。`incompatible` 是兼容判定的确定性结论，`failed` 是事务或回滚的执行错误，两者不混用。

授权状态按账户记录：`signed-out → authorizing → authorized`；`authorizing` 取消或超时回 `signed-out`，不覆盖已有账户；`authorized` 在刷新无效或授权被撤销时转 `reauth-required`，重新登录经 `authorizing` 回 `authorized`；退出登录完成清理后回 `signed-out`。

### 4.2 桌面壳动作

拟新增 `builtin_openai_status`、`builtin_openai_set_enabled`、`openai_account_status`、`openai_authorize_start/cancel`、`openai_logout` 和 `openai_catalog_refresh`。这些是建议名称，开发时按仓库命令布局落定。

全部命令绑定当前授权窗口与目标实例，先验证 dsh 族及实例归属。读动作不创建变更快照。写动作使用现有异步命令规范，组件只调动作层；I/O（输入输出）按钮使用统一 loading，长任务保留原始日志与失败面板。

## 5. Rust 与 Host 的本地协议

首版协议版本设为 `1`，通过 `127.0.0.1` 随机端口提供少量受认证端点。每个内核启动生成独立的高熵令牌，并在子进程环境中传入连接地址和令牌；禁止写入命令行、profile、日志或浏览器初始化数据。

| 操作 | 内容 |
| --- | --- |
| 握手 | 核对协议、插件版本、内核指纹及进程代次 |
| 账户状态 / 授权动作 | 只返回脱敏描述和授权页面地址；不回传令牌 |
| 模型目录 | 返回精确模型 ID、能力、目录 revision（修订号）及新鲜度 |
| 推理 | 接收已转换的模型请求，校验后发送到固定官方端点 |
| 取消 | 按请求关联标识终止对应连接与上游请求 |

握手发现协议版本或插件版本不匹配（典型来源：应用升级失败后恢复了上一份已验证接线，旧版本 Host 对上新版本服务）时按启动验证失败处理：本代次不注册模型提供方、进入 `quarantined`，日志记录两侧版本；恢复事务以当前应用资源重新准备，不提供跨协议降级。内核与工作台按未启用插件继续运行。

服务端根据连接凭据绑定账户范围，不信任请求体提供的任意实例路径、上游 URL 或账户凭据。认证失败不执行动作；浏览器跨源请求不放行，校验 Host 与 Origin（来源）等请求边界。dsh 插件本身拥有宿主进程权限，这条连接令牌不能被描述成针对恶意宿主插件的沙箱。

请求记录包含 `requestId`、`catalogRevision`、`capabilityRevision`、`modelId`、可选的原始 `reasoningEffort` 和协议输入。账户切换与发送在同一状态锁下确定账户代次：已在途请求保留原账户，新请求采用新账户。退出或禁用的生命周期回收使对应连接令牌失效。

桥接流承载有界的 OpenAI 事件及结构化错误，Host 再转换成对应内核的流式分片。设置请求体、事件和并发上限，采用背压和取消传播；不能缓存整条长回答后才发送。断流没有成功终止事件时视为失败。

这个协议仅服务内嵌插件，不提供任意代理 URL、公开监听或远程账户托管。

### 5.1 待定参数

以下参数在设计阶段不给数值，集中在所属阶段落定并回填本表；实现不得各自发明默认值：

| 参数 | 落定阶段 | 约束 |
| --- | --- | --- |
| OAuth 回调等待与刷新超时 | P2 | 超时进入明确失败，不悬挂 |
| Host 握手等待窗口 | P2 | 超时归入启动验证失败（`quarantined`），不无限等待 |
| 桥接流空闲上限、背压缓冲与并发上限 | P4 | 有界；超限按流错误结算 |
| 目录缓存有效期与新鲜度判定 | P3 | 按账号、路线与能力表版本隔离 |
| 凭据文件与密钥条目的命名和轮换策略 | P2 | 按 home 指纹、壳模式与账户注册分键 |

## 6. 免安装接线事务

1. 从已安装内核读取包导出与依赖指纹，选择已验证兼容层；不修改内核目录。
2. 校验资源清单与文件摘要，写入实例候选目录。Node 原生依赖应尽量避免，确有需要则两平台均预构建。
3. 将 Host 需要的宿主依赖解析到该指纹专属目录，验证导入的是目标内核同一份 Cordis 与服务定义。
4. 根据实际发布包的 Loader（加载器）契约选择接线方式：可直接按本地路径加载时使用独立托管配置；必须 package/bundle 解析时，离线维护仅本插件拥有的 profile 项和本地解析入口。社区物化目录 `extensions/plugins/<id>/` 与接线状态 `extensions/wiring.json` 归社区同步链管理，本插件条目必须与之互不覆盖（验收见第 10 节“接线调和”组）。仓内先例：技能接线 `kernel_adapter::ensure_skill_wiring` 在每次准备实例时向 `cordis.patch.yml` 追加 loader 行——只追加不改写、顶层不是列表就不碰、写失败只落 `shell_events` 不阻断启动；host 模型插件的加载路径与 loader 行不同，但“patch 是内核 fail-loud 输入”的纪律同样适用。
5. 先在临时实例运行不加载候选插件的启动基线，再检查候选的合并配置、插件加载、客户端资源和模型服务注册。基线失败记为无法判定，不能归因给插件。禁止把 `dsh.bundle.patch` 配置补丁与修改内核源码的补丁机制混为一谈。
6. 持实例锁提交接线与记录；失败恢复原配置，候选目录不成为活动产物。损坏的用户配置不按空文件处理，更不能覆盖重建。
7. 正常启动收到 Host 握手后公布有效状态。失败进入诊断隔离，保留用户意图；恢复须由用户明确触发。

步骤 4 的具体形状是阶段一的交付物，现阶段不能写成“随便追加 YAML（数据序列化语言）就能工作”。验收要覆盖启用后执行社区同步、关闭后重新启动、应用升级、内核切换和安全模式，证明各条调和链不互相覆盖。

免安装指插件激活无需额外安装。用户第一次安装 dsh 内核仍按现有流程进行；这两件事不能混为一个离线承诺。

## 7. 模型与强度能力协议

每个模型的内部能力记录至少包含：精确 ID、官方名称、可用状态、模态、上下文容量、工具支持、强度支持状态与值列表、能力来源、验证日期及适配器要求。

`reasoning` 需区别 `unsupported`、`supported` 和 `unknown`。支持值使用原始字符串；“模型默认”属于用户偏好，由序列化器省略字段，不把它发送成 `auto`。上下文容量未知时不能填一个统一大值，否则历史压缩阈值会失真；该模型进入能力待验证列表。

向现代 dsh 投影 `LlmModelReasoningInfo.efforts`，保留精确 ID 与显示标签。不设置全局默认强度；`defaultEffort` 缺席应保留提供方默认。旧内核兼容层必须证实控件值能够进入持久会话和请求，不能只补一个显示菜单。[dsh 模型能力类型](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/llm/llm/src/types.ts)

发送前校验模型、强度与两份 revision。旧能力下选出的已失效值返回“重新选择思考强度”；不把明确选择静默改成默认。测试比较最终发给模拟 OpenAI 服务的载荷，不能只检查下拉框文字。[OpenAI 强度参数](https://developers.openai.com/api/docs/guides/reasoning)

## 8. 请求转换、回放与错误

### 8.1 请求转换

套餐授权采用公开 Responses 端点，强制 `store: false`、`stream: true`。保持历史顺序，按路线规则转换 system（系统）指令、developer（开发者）消息、用户内容及工具结果。每次 HTTP（超文本传输协议）调用携带所需历史，不依赖 `previous_response_id`。[套餐调用说明](https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference)

为套餐路线维护专门的载荷白名单，不照搬普通 API SDK 的默认字段。其限制优先于普通 Responses 示例；例如当前路线不支持 `temperature`、`max_output_tokens` 等字段。适配器不声明可执行的输出限制；内核自行填入的默认输出预算不传上游。用户明确配置了无法执行的限制时拒绝并提示，不能声称已限制输出。[套餐预览限制](https://developers.openai.com/siwc/token-sharing-open-source/preview-limitations)

工具定义、调用、结果各有独立类型。按套餐规则将 function/custom 工具组织到 namespace（命名空间）或已验证的 `additional_tools` 形式；不使用该路线禁止的 `tool_search`。调用参数按增量传输，在完成时解析一次并校验；错误参数不执行工具。工具名称映射可逆，调用标识不丢失，多工具结果按原关联回传。底层桥接及客户端都不直接执行工具。

模型不能接收工具而本次 dsh 配置声明了工具时，请求前拒绝并说明能力差异。无工具对话配置需单独验证，不能通过悄悄去掉 `tools` 来制造兼容结果。

### 8.2 私有回放

保存用于继续推理的原生输出材料，包括加密思考条目、工具关联、内容位置和必要的消息阶段；使用内核提供的 `replayState` 或该版本等价公开接口持久化。若发布包缺少可用持久化接口，该版本兼容不能判通过。

回放记录按账号注册、路线、模型及协议版本校验，跟随相应 assistant（助手）消息存储。跨模型兼容需有测试才能允许。历史压缩和图片卸载后，元数据也必须与保留下来的消息和块对齐；不修改冻结请求、不在后台制造未记录的模型可见上下文。

响应的公开思考摘要与不透明回放材料分别处理，后者不成为显示文本。普通文件沿用 dsh 附件投影约定，不能绕过会话日志直接读取额外文件发送。

### 8.3 终止与错误分类

| 情况 | 对外分类与行为 |
| --- | --- |
| 访问令牌过期 | 同账号串行刷新；尚未输出且可安全重发时最多恢复一次 |
| 授权撤销或刷新无效 | 要求重新登录，停止盲目重试 |
| 套餐额度不足或套餐不可用 | 映射相应额度状态，保留文本，提供管理用量入口 |
| 模型或强度无效 | 模型配置错误；提示刷新目录或重新选择 |
| 连接、代理、TLS 失败 | 网络错误，展示阶段与日志；不归因成插件故障 |
| 服务端限流、临时失败 | 交给 dsh 现有重试策略，一层拥有重试 |
| 成功完成 | 返回用量，再发且仅发一次成功 finish（终止分片） |
| 用户取消、未完成、断流 | 各自结算，不能因为已有文本就认定成功 |

OpenAI 服务禁用隐藏推理重试，避免与 dsh 重试相乘。令牌刷新和授权恢复不跨账号自动切换；开始输出、产生工具调用或取消后，不透明重发风险必须进入 dsh 的显式失败路径。

## 9. 实施顺序与完成条件

| 阶段 | 工作 | 完成条件 |
| --- | --- | --- |
| P0：发布包调查 | 固定最新窗口的 3 个包，记录导出、客户端构建、模型、强度、历史接口 | 形成逐版本接口表与离线接线原型，所有缺口有明确实现路径 |
| P1：离线交付 | 构建 Host/Client、资源摘要、宿主依赖解析、启停事务 | 无插件网络下载和 pnpm；未启用时行为与原应用一致 |
| P2：授权服务 | 动态注册、回调验证、加密凭据、刷新、吊销、本地桥接；构建模拟授权服务器与模拟模型服务（后续阶段与 CI 复用） | 模拟安全测试通过；真实账号完成授权和一次明确触发的推理 |
| P3：模型与强度 | 账号目录、精确能力表、正式选择器、会话偏好 | 每个公布模型与强度均有验证证据，错误值请求前被拦下 |
| P4：会话适配 | 流、工具、图片、回放、压缩、取消和重试归属 | 多轮与重启后正确继续，错误和部分输出如实结算 |
| P5：界面与诊断 | 内嵌开关、模型设置账户卡片、用量说明、隔离与恢复 | 3 个内核中均无需命令行即可完成启用、登录和选模 |
| P6：发布验证 | 两平台、3 个内核、实际安装包、离线激活和全量门禁 | 全部必须项通过，发布说明列出精确版本及模型范围 |

P0 的退出准则：窗口内每个版本要么形成明确实现路径，要么记录为发布阻塞项。若三个版本都缺少必要能力、或缺口无法形成实现路径，不进入 P1——缩小窗口、推迟功能或调整范围由维护者拍板，决定回写进本文与设计文档；不得带病开工，也不得把“待验证”写成“支持”。

各阶段的接口和可运行验证先于界面完善。任何必要能力缺失都阻止兼容声明；不以“基本能对话”提前结项。

## 10. 验收矩阵

每次应用发布锁定 3 个官方包的版本和摘要，分别在 Intel macOS 与 Windows 上执行，形成 6 组结果。仅在 macOS 上通过不能代替 Windows 安装包验证。

| 测试组 | 必须覆盖 |
| --- | --- |
| 免安装 | 干净实例、已有社区插件实例、离线激活、无运行时 npm/pnpm、内核树摘要不变 |
| 启停 | 默认关闭、停止后启用/关闭、运行中阻止修改、重启幂等、其他内核族无副作用 |
| 宿主一致性 | 同一 Cordis 与服务实例、客户端不重复 React、双实例不同版本同时运行 |
| 接线调和 | 社区同步、隔离、安全模式、应用升级和内核切换不覆盖用户项 |
| OAuth | 成功、拒绝、取消、超时、回调重放、state/nonce/签名错误、注册身份不匹配 |
| 凭据 | 两平台权限、系统密钥库锁定、刷新轮换并发、写入中断、退出远端失败、无令牌泄漏 |
| 模型 | 当前账户排序与标识、切换账号失效缓存、未知能力、目录失败、模型下架 |
| 强度 | 模型默认不发送字段、每个支持值精确发送、非法值拦截、切模型后失效、重启偏好 |
| 对话 | 中文、多轮、系统指令、公开摘要、思考回放、工具往返、多工具、图片和压缩 |
| 流与错误 | 增量边界、大参数流、取消与背压、输出后额度错误、未完成、无成功终止的断流 |
| 网络 | 浏览器与 Rust 路径不同、系统代理、传输失败回退、TLS 校验、断网恢复 |
| 界面 | 两主题、中英文案、键盘操作、状态含义、加载与失败反馈、原生模型与强度选择 |

“每个支持值”按已公布模型的能力表展开，而不是抽一个 `high` 值代替全表。真实账号权限不足的模型不能记为通过；从当前公布的可用集合中移出并说明原因，或补齐验证。

持续集成使用模拟授权服务器、模型服务和临时凭据库，不把真实账号令牌存入 CI（持续集成）密钥。真实账号测试由用户主动授权，在本地或隔离测试机运行，产物只保存脱敏结果。

## 11. 门禁与发布记录

开发完成后，把资源完整性、兼容协议、模型能力及相应测试接入现有门禁；判据写在共享脚本，工作流只调用。内置插件资源在 P1 落地时设体积预算并纳入同一门禁（与 UI 产物预算同一原则），超预算即失败；预算数字随首个资源清单登记进共享脚本。新增源码先登记预算，禁止为大文件上调预算。

提交前执行 `npm run check`。它是现有唯一全量门禁；插件测试与资源检查需要纳入其中，不能靠一条未被门禁调用的手工命令声称覆盖。两平台安装包和真实账号验证仍需另有记录。

内嵌产物生成目录不得作为普通源码提交；按现有产物管理方式由构建生成并收进安装包。构建失败或资源缺失必须失败退出，不能打出缺少插件的成功安装包。

每份支持记录包含：应用/插件版本、3 个内核版本与完整依赖指纹、兼容层、两平台结果、模型能力表版本、验证模型与强度集合、账号路线、未通过项。不能记录真实令牌或完整授权 URL（统一资源定位符）。

上游发布新内核后，先更新测试窗口并运行门禁与安装包验收，再更新支持声明。现有应用对未知指纹显示“待验证”，保留普通工作台使用，不偷偷切内核或装包。

实现交付时同步更新根 README、架构、插件管理、排障及相关前后端约定。当前这两份文档是设计与开发计划，不将待实现行为写成已经上线。

## 12. 资料与证据

本地入口以当前仓库源码为准，旧插件文档中有历史路径，不据此决定写入位置。上游接口资料固定在 `5badb15009ae1756c3afe0ae0cef1faafc290ccc`，逐版本实现还须核对实际 npm 包。

- [官方 dsh 版本记录](https://registry.npmjs.org/@deepseek-ai%2Fdsh)：计算兼容窗口，不代表实际兼容测试。
- [dsh 模型服务](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/llm/llm/README.zh.md)：提供方注册、流终止、历史与重试边界。
- [dsh 模型设置](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/client/ui-settings-models/README.zh.md)：设置扩展入口与模型编辑边界。
- [OpenAI 动态注册](https://developers.openai.com/siwc/token-sharing-open-source/sign-in)：客户端注册、回调与身份验证。
- [OpenAI 账户与刷新](https://developers.openai.com/siwc/token-sharing-open-source/profiles-and-sessions)：账号隔离、旋转刷新令牌和吊销。
- [OpenAI 套餐调用限制](https://developers.openai.com/siwc/token-sharing-open-source/preview-limitations)：载荷、工具、输入与会话约束。

当前实施状态（截至 2026-10-09，逐阶段对照 §9）：

| 阶段 | 状态 | 依据 |
| --- | --- | --- |
| P0 发布包调查 | 完成 | [P0 发布包调查](openai-oauth-p0-findings.md)：三版本接口表 + 离线接线原型（§5）+ 目录与客户端链路（§6） |
| P1 离线交付 | 完成（Rust 交付层 + 插件源码 + 资源管线 + 面板开关；三版本冒烟） | 提交 `3457281`/`4b09293`/`0bda3c6`/`fca9192`/`3f34bd8` |
| P2 授权服务 | 功能面完成（桥接/auth/jwk/vault/callback/transport/flow/refresh/命令面；模拟授权服务器端到端）；**真实账号联调未做** | 提交 `80a6c21`…`a0282c5` |
| P3 模型与强度 | 目录/能力表/缓存/新鲜窗口完成；强度线格式端到端通过；**真实账号目录形状未验证**；能力表 v0 为空（P6 逐模型验收后填表） | 提交 `9e261c6`/`6a893dd`/`3273abe` |
| P4 会话适配 | 校验/流转发/请求转换/工具往返完成（headless 会话端到端三版本）；reasoning delta 会话级细节与回放重启保留待深化 | 提交 `03b8289`/`1a05a7f`/`ee8749d`/`71305c4` |
| P5 界面与诊断 | 桌面开关完成；工作台账户卡接真（ACL + 状态机）；**实机点击验收未做**；3 个内核的完整走查按 §10 执行 | 提交 `fca9192`/`df82039` |
| P6 发布验证 | 未开始（两平台构建、真实账号验收、发布说明） | — |

离线验证的覆盖面与边界：`openai` 组 38 个 Rust 测试 + 插件 10 个单测 + 三版本 headless 会话冒烟（含工具往返、强度线格式、信封白名单、boot 图），全部**不依赖真实 OpenAI 服务**；官方 SIWC 端点形状、issuer 常量与真实账号行为仍是设计 §10 的未验证项，联调时只改配置层。
