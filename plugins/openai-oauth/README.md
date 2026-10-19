# 内嵌 OpenAI OAuth 插件（P1 骨架）

状态：**P1 骨架**。接线配方、适配器注册、提供方目录声明与客户端卡片
插槽已在窗口内三版本实测（见
[docs/features/extensions/openai-oauth-p0-findings.md](../../docs/features/extensions/openai-oauth-p0-findings.md)
§5/§6）；推理桥接、OAuth 与凭据是 P2+ 交付，本骨架对未实现路径一律
抛稳定 code 的错误，不产生模拟回答或假绿状态。

## 布局

```text
host/index.js      入口：registerAdapter + 偏好文件变更通知
host/adapter.js    LlmAdapter 子类：目录/解析转发桥接 + listModels 按常用模型过滤
host/bridge.js     本地桥接客户端（握手 /v1/handshake、目录 /v1/models）
host/constants.js  稳定标识与环境变量契约
client/client.js   设置页提供方卡插槽（keyed by settingsNs，自包含束）
locales/{zh,en}.js 文案源（client.js 内联孪生，构建步骤落地前手工同步）
test/smoke.mjs     端到端冒烟：物化+接线+启动内核+桩桥接+断言
test/fixtures.mjs  测试夹具：host/ 副本 + 桩 peer（dsh-llm / schemastery）
test/adapter-catalog.test.mjs 目录 → 会话列表的映射（selected 过滤）
test/selection-watch.test.mjs 偏好文件变更通知（含 env 接线）
```

## 标识契约

| 标识 | 值 | 用途 |
| --- | --- | --- |
| 包名 = loader 行 id = settingsNs | `xlink-openai-oauth` | 接线行 id 经 `ctx.fiber.entry?.options.id` 成为设置命名空间，client 卡片同 key |
| 提供方路由 | `xlink-openai-chatgpt` | dsh 会话与配置 |
| 桥接 env | `DSH_XLINK_OPENAI_BRIDGE_URL` / `…_TOKEN` | 仅经子进程环境传入（开发计划 §5） |
| 偏好文件 env | `DSH_XLINK_OPENAI_SELECTION_FILE` | 常用模型偏好文件的路径；Host 只用它做变更通知 |
| 冒烟标记 env | `DSH_XLINK_OPENAI_TEST_MARKER` | 设置后 Host 写状态文件；生产不设、不写 |

## 常用模型（2026-10-13）

账号目录的模型默认全都进会话选择器，几十条里混着实验模型。设置页的卡片下半部分让用户勾出常用的那几条：

- **整块默认收起成一行**（2026-10-13 用户反馈「占用过大」）：标题 + 「已选 N 项」+ 「选择模型」；展开才画列表与说明文字，保存成功后自动收起。收起时未保存 / 读取失败 / 空态照常显示，只有列表被折起来；未保存时不许收起（另有「放弃改动」作为出口）；
- **那一行只说数量、不列模型名**（2026-10-13 用户要求「不需要显示已经勾选的模型」）：目录几十条时，「已选 4 项：A、B、C…」既占宽度又挤掉同一排的动作按钮。完整清单保留在那一行的 `title`（悬停可见），收起时想核对选了哪几条不必展开整个列表；
- **勾选只改界面上的草稿，点「保存」才写盘**（2026-10-13 用户要求：不要每勾一下写一次）；「恢复默认」同一条路径，未保存时显示「未保存的更改」；
- 偏好落 `<xlink_home>/shell/<mode>/openai-oauth/model-selection.json`（壳侧写入，`atomic_write`）；
- 桥接 `/v1/models` 的载荷带 `selected`（生效集合 = 偏好 ∩ 当前目录，默认集合见 `src-tauri/src/openai/selection.rs`）；
- **只有 `listModels` 被过滤**：`resolveModel` / 推理走完整目录；
- 保存后不必刷新页面：Host 观察偏好文件，stat 一变就 `replace` 一次适配器注册，内核随即广播 `llm/adapters-updated` 让已打开的页面重拉目录。通知失败只写日志，刷新页面照样生效。

## 接线形状（Rust 侧接线器按此生成）

```yaml
- insert:
    - id: xlink-openai-oauth
      name: '<到本包 host/index.js 的相对路径，锚定在 cordis.patch.yml 所在目录>'
```

要点（P0 调查 §5）：`name` **必须直指入口 JS 文件**（ESM 无目录导入）；
插件物化目录按「插件版本 + 内核依赖指纹 + 兼容层」区分，其内
`node_modules/@deepseek-ai/dsh-llm` 符号链接指向目标内核树同一包。

## 验证

v0.1.18 修掉前缀缓存命中率过低：请求从来没带过稳定的 `prompt_cache_key`，上游给每次请求各生成一个随机键，同一会话的缓存只能在分片间碰运气（2026-10-19 实测 58 次请求平均命中 33%，0% 与 98% 成段交替；同账号原生 Codex 用稳定会话标识是 96–97%）。现在 Host 把内核的 `GenerateOptions.sessionId` 清洗成 `[A-Za-z0-9._:-]{1,128}` 后写进载荷，桥接白名单放行并校验同一形状（[inference.rs](../../src-tauri/src/openai/inference.rs)）；压缩/会话标题这类前缀不同的辅助调用不带该键，避免挤占主对话的缓存分片。单测钉住「同一会话同键、跨会话不同键、无身份不发、辅助调用不发、非法字符/超长清洗」，反向验过（把键换成随机值或去掉校验时对应用例变红）。命中率是否真的升到 90%+ 需要下一次真实账号会话复核——看响应里的 `prompt_cache_key` 是否稳定回显、`cached_tokens` 占比是否跃升。

v0.1.17 修掉两条额度相关缺陷（2026-10-19 一条真实会话实测暴露：单轮 58 次调用、463 万输入 token、9 分钟打满一个 Plus 5 小时窗口）：① `pumpStream` 按内核的**互斥桶**口径上报用量——`inputTokens` 减掉 `input_tokens_details.cached_tokens`，缓存另报 `cacheReadTokens`，思考另报 `reasoningTokens`；此前该会话 152.6 万缓存命中被记成未缓存输入。② 上游「Subscription Sharing usage limit」这类额度终局失败在适配器层翻成内核 `LlmError(code=QUOTA)`；此前普通 `Error` 被内核 `normalizeLlmFailure` 记成 `UNKNOWN`，UI 拿不到额度提示与用量入口。单测钉住「三桶之和 == total - output」（内核 `normalizeUsage` 会**整条丢弃**不一致的用量）、缓存减法、reasoning 越界省略，以及额度与限流两类文本的区分（限流不能当额度，否则用户去等一个没被用掉的窗口）。

v0.1.16 把常用模型收起行的摘要改成只说「已选 N 项」（用户要求：不需要显示已经勾选的模型），完整清单移到该行的 `title`。判据钉中文字面量而不是 STRINGS 表——第一版拿 `strings.modelsSummary(...)` 比渲染结果，两次反向验都被它放过。

v0.1.15 把常用模型整块默认收起成一行（用户反馈「占用过大」），展开才画列表。

v0.1.14 把常用模型改成「勾选只改草稿、点保存才落盘」（用户要求：不要每勾一次写一次）。

v0.1.13 增加常用模型选择（`selected` 过滤 + 偏好文件变更通知），并补两条不依赖内核的测试。

v0.1.12 修正套餐路线的函数工具分组：`tools` 中使用 `type: "namespace"` 容器，函数定义放入其 `tools` 数组，不在函数定义上附加 `namespace`。工具调用历史与结果按 `call_id` 配对，名称转换和回映射保持一致。

```sh
node --test test/*.test.mjs                                     # 单测（无需内核）
node test/smoke.mjs --kernel <内核安装根> [--port 3130] [--keep]  # 端到端冒烟
```

单测里 `adapter-catalog.test.mjs` / `selection-watch.test.mjs` 用
`test/fixtures.mjs` 把 `host/` 复制进临时目录并摆两个桩 peer（`dsh-llm`、
`schemastery`）：它们验的是「载荷 → 会话列表」的映射与偏好变更通知，与内核
语义无关，所以不需要真的装内核。内核兼容性仍由 `adapter-images.test.mjs`
（需 `DSH_OAUTH_TEST_KERNEL`）与冒烟覆盖。

冒烟自建临时 DSH home（不触碰用户数据），起桩桥接，断言：目录行注册、
settingsNs 取行 id、桥接握手、目录拉取、常用模型过滤（桩载荷带 `selected`，
会话选择器只拿到勾选的条目）、未勾选模型仍可解析、boot 图含 client 条目。
退出码非 0 即失败。窗口内三版本（`0.2.1-alpha.1` / `0.2.0-rc.2` / `0.2.0-rc.1`）
均已通过（2026-10-08）；`0.2.1-alpha.2` 于 2026-10-13 复跑通过（含常用模型那两条）。

## 门禁与预算

`scripts/check-code-duplicates.mjs` 的重复代码检测目前只扫 `src-tauri/src` 与 `ui/src`；
本目录在其扫描范围外。资源管线（构建期预编译 + 摘要清单 + 体积预算）
落地时把本目录一并纳入（开发计划 §11）。

## 后续（按开发计划）

- P2：Rust 桥接服务（OAuth 动态注册、加密凭据、本地监听）+ `/v1/responses` 流
- P4：stream/工具/回放转换（`ReplayEnvelope` 两半由桥接事件组装）
- P5：登录交互与完整文案（locales 构建接线）
