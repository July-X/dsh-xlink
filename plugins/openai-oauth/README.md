# 内嵌 OpenAI OAuth 插件（P1 骨架）

状态：**P1 骨架**。接线配方、适配器注册、提供方目录声明与客户端卡片
插槽已在窗口内三版本实测（见
[docs/features/extensions/openai-oauth-p0-findings.md](../../docs/features/extensions/openai-oauth-p0-findings.md)
§5/§6）；推理桥接、OAuth 与凭据是 P2+ 交付，本骨架对未实现路径一律
抛稳定 code 的错误，不产生模拟回答或假绿状态。

## 布局

```text
host/index.js      入口：registerConfigurableProviders + registerAdapter
host/adapter.js    LlmAdapter 子类：目录/解析转发桥接，stream 抛 NOT_IMPLEMENTED
host/bridge.js     本地桥接客户端（握手 /v1/handshake、目录 /v1/models）
host/constants.js  稳定标识与环境变量契约
client/client.js   设置页提供方卡插槽（keyed by settingsNs，自包含束）
locales/{zh,en}.js 文案源（client.js 内联孪生，构建步骤落地前手工同步）
test/smoke.mjs     端到端冒烟：物化+接线+启动内核+桩桥接+断言
```

## 标识契约

| 标识 | 值 | 用途 |
| --- | --- | --- |
| 包名 = loader 行 id = settingsNs | `xlink-openai-oauth` | 接线行 id 经 `ctx.fiber.entry?.options.id` 成为设置命名空间，client 卡片同 key |
| 提供方路由 | `xlink-openai-chatgpt` | dsh 会话与配置 |
| 桥接 env | `DSH_XLINK_OPENAI_BRIDGE_URL` / `…_TOKEN` | 仅经子进程环境传入（开发计划 §5） |
| 冒烟标记 env | `DSH_XLINK_OPENAI_TEST_MARKER` | 设置后 Host 写状态文件；生产不设、不写 |

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

v0.1.12 修正套餐路线的函数工具分组：`tools` 中使用 `type: "namespace"` 容器，函数定义放入其 `tools` 数组，不在函数定义上附加 `namespace`。工具调用历史与结果按 `call_id` 配对，名称转换和回映射保持一致。

```sh
node test/smoke.mjs --kernel <内核安装根> [--port 3130] [--keep]
```

冒烟自建临时 DSH home（不触碰用户数据），起桩桥接，断言：目录行注册、
settingsNs 取行 id、桥接握手、目录拉取、boot 图含 client 条目。退出码
非 0 即失败。窗口内三版本（`0.2.1-alpha.1` / `0.2.0-rc.2` / `0.2.0-rc.1`）
均已通过（2026-10-08）。

## 门禁与预算

`scripts/check-code-budget.mjs` 目前只扫 `src-tauri/src` 与 `ui/src`；
本目录在其扫描范围外。资源管线（构建期预编译 + 摘要清单 + 体积预算）
落地时把本目录一并纳入（开发计划 §11）。

## 后续（按开发计划）

- P2：Rust 桥接服务（OAuth 动态注册、加密凭据、本地监听）+ `/v1/responses` 流
- P4：stream/工具/回放转换（`ReplayEnvelope` 两半由桥接事件组装）
- P5：登录交互与完整文案（locales 构建接线）
