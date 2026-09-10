# dsh-escalation-same-mode v1.1.0：同模式 sandbox_permissions 不再抛错

让 `approveEscalation` 在「请求模式等于当前 effective mode」时直接返回，不再被严格更宽表击穿；UI 的 apply/撤销走最朴素的 copy/replace 语义。

v1.1.0 重新对齐官方 `0.1.3-alpha.2` dist：仅 `assertNever` 的 import 从 `@deepseek-ai/dsh-llm` 拆到 `@deepseek-ai/dsh-util-values`（官方无关重构），`approveEscalation` 仍未带同模式短路，因此补丁载荷与预期 SHA 都得随 dist 一起更新。v1.0.0 仅锚定 `0.1.1-rc.2`；其 `state.json` 应用记录（旧 patchVersion）仍可在 `0.1.1-rc.2` 内核上撤销（备份路径未变），但重应用需先撤销旧记录再用 v1.1.0。

## 触发场景

`ctx.sandboxPolicy.resolve(...).mode === "danger-full-access"`（或 `workspace-write`），且模型在工具调用里同时传入 `sandbox_permissions: <同一模式>` 与 `justification`。

`@deepseek-ai/dsh-sandbox/lib/index.js#approveEscalation` 的当前实现查 `WIDER_MODES[effectiveMode] ?? []`，而 `WIDER_MODES` 不含 `danger-full-access` 键（`workspace-write` 同模式时同理），结果列表为空 → `includes(mode)` 为 false → 直接抛：

```
Error: sandbox escalation to "workspace-write" is not strictly wider than this call's current "danger-full-access" mode
```

抛错按设计发生，但它把"请求等价于当前模式"这种语义上无害的输入也一并拒绝。本错误在 `0.1.3-alpha.2` 上依旧存在——官方既未把我们的短路并入内核，也未把 `WIDER_MODES` 补全。

## 修改

在 `approveEscalation` 顶部加一行同模式短路：

```diff
 async function approveEscalation(request, approval) {
   const { requestedMode: mode, effectiveMode, justification, subject } = request;
 +	if (mode === effectiveMode) return mode;
   if (!(WIDER_MODES[effectiveMode] ?? []).includes(mode)) throw new Error(`sandbox escalation to "${mode}" is not strictly wider than this call's current "${effectiveMode}" mode`);
   …
```

新插入的短路行在严格更宽表之前生效，且**不**调用 `approval.approver.request(...)`——请求本身就是 no-op，没有可批准的内容。其他分支保持原样：

- `requestedMode` 严格更宽（`read-only` → `workspace-write` / `danger-full-access`，`workspace-write` → `danger-full-access`）：仍走原审批流。
- `requestedMode` 既不等于当前模式也**不**更宽（`danger-full-access` → `workspace-write` / `read-only`，`workspace-write` → `read-only`）：仍按原路径抛「not strictly wider」。
- 没有 `approver` / `agent`：仍按原路径抛对应错误。
- 用户拒绝 / 审批取消 / 审批通道不可用：仍按原路径抛错。

## 目标

```text
node_modules/@deepseek-ai/dsh-sandbox/lib/index.js
```

来自 npm 包 `@deepseek-ai/dsh-sandbox@0.1.3-alpha.2`，MIT © 2026 DeepSeek。

## 文件模式

`copy + expectSha256`：补丁载荷是整个修改后的 `lib/index.js`（保存在 `files/dsh-sandbox/index.js`），不是精确字符串替换。UI 的 apply/revert 走文件系统层最朴素的覆盖语义：

| 操作 | 实际步骤 |
 | --- | --- |
 | 应用补丁 | `fs::copy(files/dsh-sandbox/index.js, target)` + 备份 target 到 `backups/<patch-id>/<version>/<相对路径>`，最后写 `state.json` |
 | 撤销补丁 | `fs::copy(backup, target)` + 删除备份项 + 改 `state.json` |

没有"搜索匹配"状态机，没有"先备份再搜索再写入"这种分两步——`copy` 模式就是一次原子覆盖，搜索串漂移、文件被其他工具改过、内核升级全部由 `expectSha256` 在 copy 之前一次性挡住。

## 哈希

```text
原始 dist 目标文件 SHA-256（manifest.expectSha256）：8994b3e497b0673eddd3640392de4671621aa8972846f4a66d0b1219decf3c03
补丁载荷 files/dsh-sandbox/index.js SHA-256        ：f741cf32e7918b243b423ac549e916f407db565637e6cfed7ca1baf0de7fba35
```

v1.0.0 锚定 `0.1.1-rc.2` 时分别为 `63ee2a10…e324f` / `dafc42d2…22c5e`——已被 v1.1.0 取代。

`expectSha256` 与原始 dist 一致——目标若已被第三方工具改过、`pnpm install` 重写、或内核升级，应用会以"目标 SHA 与预期原文件不符"明确失败，绝不会盲目覆盖未知内容。补丁载荷按包名保留在 `files/dsh-sandbox/index.js`，跟现有 `dsh-file-perf` / `dsh-session-perf` 的目录布局一致。

## 启用 / 撤销

补丁随 dsh-xlink 发布包内置，默认不生效：

1. 关闭工作台；
2. 打开「设置 → 内核补丁」，找到「dsh 同模式 sandbox_permissions 不再抛错」；
3. 点击「应用到当前内核」：UI 把 payload 拷到目标，备份原文件到 `<data_dir>/patches/backups/dsh-escalation-same-mode/0.1.3-alpha.2/...`，`state.json` 落盘；
4. 重启工作台使新 sandbox 模块生效。

需要回滚时同卡片点「撤销补丁」：UI 把备份拷回去、删备份、`state.json` 移除记录。可重复应用与撤销，无残留状态。

## 验证

```sh
npm run test:escalation-same-mode            # 默认自动定位激活内核
node scripts/verify-dsh-escalation-same-mode.mjs <内核根目录>   # 显式传入内核目录
node scripts/verify-dsh-escalation-same-mode.mjs --require-applied
```

验证脚本只读：会检查 manifest 内容、目标文件 SHA-256（原始 / 补丁后 / 当前）、载荷 SHA-256 与 manifest 一致，并通过临时 node_modules 链接加载 `approveEscalation` / `WIDER_MODES` / `SandboxUnavailableError`，复跑同模式 no-op / 严格更宽 / 缩小 / 缺 approver / 缺 agent / 审批取消等行为断言。v1.1.0 起为 `@deepseek-ai/dsh-llm` 与 `@deepseek-ai/dsh-util-values` 分别提供独立 stub。

## 已知限制

- 仅声明适用于已核验的 `0.1.3-alpha.2` 内核；其它内核版本需要先重新核对 `dsh-sandbox` 的 `approveEscalation` 实现与目标 SHA-256，再调整版本范围与载荷。
- 因为载荷是整文件，将来 `dsh-sandbox` 升级版本时必须重新生成 payload 并把 manifest 版本号往上拨（已应用旧版的内核会变 `dirty`，由用户先撤销再重应用）。
- 模型仍应避免在已被禁用审批的会话里送入 `sandbox_permissions`——本补丁只是兜底，不是放开请求语义。
