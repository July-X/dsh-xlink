# 内嵌 OpenAI OAuth 插件——真实服务联调指南

日期：2026-10-09。状态：**待执行**——本文是设计 §10 未验证项（「官方动态注册到真实套餐推理」）的收口路径：把代码里标注「未验证」的配置层触点列全，联调时逐项核对与收紧。**执行前提**：真实 ChatGPT 账号 + 官方 SIWC 端点文档可达（本仓开发沙箱内两者曾不可达，协议层按 OIDC 标准实现并经模拟授权服务器端到端验证——见[开发计划](openai-oauth-development-plan.md)「实施状态」）。

## 1. 配置层触点（联调时要核对/修改的全部位置）

| 触点 | 位置 | 当前状态 | 联调动作用 |
| --- | --- | --- | --- |
| issuer 基址 | `src-tauri/src/openai/auth.rs` 的 `SIWC_ISSUER` | 按公开资料猜测值，**未验证** | 对官方文档核对；发现不符只改此常量 |
| 目录端点路径 | `src-tauri/src/openai/catalog.rs` 的 `MODELS_PATH` | `/v1/models`（惯例） | 与官方 models-and-inference 文档核对 |
| 推理端点路径 | `src-tauri/src/openai/inference.rs` 的 `RESPONSES_PATH` | `/responses`，拼接资源基址 `auth::OPENAI_RESOURCE`，最终为 `https://api.openai.com/v1/responses` | 已核对[官方模型与推理文档](https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference)；不得拼到授权 issuer 下 |
| scope 串 | `src-tauri/src/openai/auth.rs` `authorize_url` 的 `scope` 参数 | `openid offline_access model.request`（未验证） | 与官方 sign-in 文档核对；增删 scope 只改此串 |
| 目录响应形状 | `src-tauri/src/openai/catalog.rs` `fetch_with_token` | 防御式接受 `data`/`models` 双键、`id`/`slug` 双名 | 真实形状确认后收紧为单键，删防御分支 |
| 上游事件形状 | `plugins/openai-oauth/host/request.js` `pumpStream` 的事件类型后缀 | 防御式后缀匹配（`completed`/`failed`/`output_text.delta`…） | 与真实事件流逐一对账，收紧匹配 |
| 工具面形状 | 同上 `output_item.added` 分支 | `function_call` + `item_id` 增量（惯例） | 同上 |
| 终止事件 | `src-tauri/src/openai/inference.rs` `classify_terminal` | 后缀三分类 | 同上 |

## 2. 联调步骤（建议顺序）

1. **发现**：`curl <issuer>/.well-known/openid-configuration`，对照 `parse_metadata` 的五字段；issuer 常量修正后跑 `cargo test openai::auth`。
2. **动态注册**：用 `registration_request` 的请求体真实 POST 一次，核对响应字段（`client_id` 必需）；确认 redirect_uri 的回环端口语义（RFC 8252）被接受。
3. **登录**：实机起工作台（P5 账户卡），走完系统浏览器授权；抓回调 query 确认 `state`/`code` 形态；换令牌请求确认 `code_verifier` 被接受、旋转 refresh token 生效。
4. **ID token**：抓真实 JWKS 与 id_token，跑 `verify_rs256`/`verify_id_token` 的断言面（iss/aud/nonce/exp）；发现额外声明要求（如 `aud` 多值、`azp`）按需扩展。
5. **目录**：拉真实模型清单，核对 `catalog.rs` 解析；把已验证模型写进能力表（`CAPABILITY_TABLE`）并递增 `CAPABILITY_REVISION`。
6. **推理**：真实账号发起一次会话（明确触发的推理），核对 SSE 事件类型与 `pumpStream` 匹配面、终止事件与回放材料形状。

每步通过后在[实机验收清单](openai-oauth-acceptance-checklist.md)对应项打勾并记录日期与环境。

## 3. 已知风险与回退

- issuer/端点/形状如有出入：**只改本指南 §1 表所列配置层**，协议与密码学层（PKCE/RS256/信封校验）不动——它们已被模拟授权服务器端到端钉住。
- 若官方要求机密客户端（非 PKCE 公开客户端）：`registration_request` 的 `token_endpoint_auth_method` 与换令牌请求需加 basic 认证——这是协议层变更，回设计文档走变更记录。
- 联调发现的任何差异**先记录再改码**：把官方文档摘录进本文件 §4，保持「未验证 → 已验证」的结论可追溯。

## 4. 联调差异记录

（空——首次联调后在此逐条记录官方文档/行为与实现的差异及修正提交。）
