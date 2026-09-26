# dsh-xlink 套餐 / Token Plan 用量展示设计

> 本文档描述桌面外壳的「云端套餐用量」功能：查询并展示 MiniMax Token Plan 的双窗口额度
> 进度与 DeepSeek 按量余额。参考实现是
> [cc-switch](https://github.com/farion1231/cc-switch)（Rust + Tauri，MIT）的
> `services/coding_plan.rs` 与 `services/balance.rs`，接口字段以其源码为核对基准。
>
> 与现有「模型用量」（`usage.rs`，本地 token 统计）的关系：usage.rs 回答
> 「过去消耗了多少」，本功能回答「云端账户还剩多少、何时重置」。两层数据互相独立、
> UI 上并列呈现，互不替代。

## 状态（2026-09-25）

**已实施**（本文当日完成实现；步骤 0 的接口与凭据真机验证仍需真实凭据后补跑，
见文末「遗留验证」）。规划阶段曾由 cc-switch 源码级核实接口字段；本文已吸收一轮
review 结论：能力边界前置、复用 dsh 内核模型凭据、按实例解析与缓存、重试语义统一、
HTTP 栈适配（ureq + spawn_blocking，非 reqwest）、原始凭据不跨壳传递作为新增验收项、
代码预算门禁纳入实施步骤。

### 实施落地与设计稿的差异（实现时的补充决策）

- **凭据解析以「直接只读」路线落地**（设计允许的两条路线之一）：`credentials.rs`
  解析 profile `apiKeyEnv` → 环境变量 → `.credentials.yaml` refs → `.env`，
  与内核同优先级；引用名缺省时按内核派生规则（route 大写 + `_API_KEY`）。内核
  受控代理路线需改内核，v1 未采用；因此**凭据解析在内核热更新凭据、加锁写
  `.credentials.yaml` 的瞬间可能读到旧值**——下次刷新自然收敛，不阻塞展示。
- **确定性失败不误标 expired**：仅 HTTP 401/403 置 `credential_status: expired`；
  `base_resp` 业务错误按确定性失败展示但不停自动刷新（status_msg 语义不可靠，
  宁可 5 分钟多试一次也不要把好凭据错标失效）。
- **每个 provider 自带状态**（`fetch_error` / `error` / `credential_status`）：
  多 provider 查询允许部分成功，单个网络失败不吞掉其它 provider 的新数据
  （「错误通道语义」一节中「瞬时失败返回顶层 Err」按此细化为 provider 级错误）。
- **结构不认识的响应**：截断摘要（≤200 字符）写入现有 Shell 日志
  （`<kind>-subscription-<日期>.log`）供「查看日志」反馈，不含凭据。
- **凭据值指纹**为 SHA-256 前 16 个 hex 字符（仓库已有 sha2 依赖），条目同时
  记录 credential reference 名（非秘密）供用户辨识。

### 遗留验证（步骤 0，需真实凭据）

1. MiniMax `/coding_plan/remains` 真实响应结构（字段名 / 时间戳格式 / 周层 status）。
2. 当前 profile 的 `apiKeyEnv` 与实际凭据来源能否被 Shell 与 DSH 解析成同一把 Key。
3. `platform.minimax.cn` 站配置的凭据能否查询 `api.minimaxi.com`；不通则改
   CN 入口常量并回写接口规格一节。
4. MiniMax Token Plan 接口是否接受当前 provider 的模型凭据；若拒绝，按本文
   「凭据设计」的硬前提暂停 MiniMax 展示（错误文案已按此口径实现）。

## 能力边界（先说清楚拿不到什么）

用户诉求是「套餐、token plan 的用量、剩余 token、限额情况」。其中**「剩余 token」
这个绝对数字在两家云端 API 里都不存在**，v1 不提供：

| 供应商 | 云端接口实际能给的 | 拿不到的 |
| --- | --- | --- |
| MiniMax Token Plan | 5 小时窗口与周窗口的**剩余百分比**、窗口重置时间 | 绝对剩余 token 数（官方额度口径甚至是「3-4 个 Agent」级别的模糊描述）；套餐档位名（Plus / Max / Ultra，API 不回传） |
| DeepSeek | 账户**货币余额**（总额 / 赠送 / 充值，CNY 与 USD）、余额是否可用 | 一切「额度」「限额」概念——DeepSeek 是纯按量计费，只有余额 |

设置页与窗口文案按此口径措辞：MiniMax 展示「剩余 X%」，DeepSeek 展示金额；
不做任何绝对 token 数的估算或虚构档位标签。本地「已用 token」统计由现有
usage.rs 窗口承担，本功能的窗口里放一行入口指过去即可。

## 术语

| 词 | 含义 |
| --- | --- |
| 订阅 Key | MiniMax Token Plan 接口要求的凭据类型；本功能不单独保存或输入它，是否与当前 dsh provider 的模型 Key 相同必须由步骤 0 真机验证。 |
| 模型凭据 | 当前 DSH profile 的 provider 通过 `apiKeyEnv` 引用的实际 API Key；查询优先复用这一凭据。 |
| credential reference | `.credentials.yaml` / 环境层中使用的引用名，例如 `MINIMAX_CN_API_KEY`；只传递引用名，不传递值。 |
| 额度窗口 | MiniMax Token Plan 的两层计量周期：5 小时固定窗口 + 周窗口；部分套餐无周限额 |
| tier | 一次查询里的一个额度层（5h / 周），含利用率与重置时间 |
| keep-last-good | 查询失败时保留并继续展示上一次成功数据，错误单独呈现，不清空旧值 |

## 接口规格

两家的查询端点均为**第一方但未文档化 / 轻文档化**的接口。MiniMax 端点没有公开
文档（cc-switch 以源码形式消费它，其注释记载同类接口有「上线当天就改形态」的
先例）；DeepSeek 端点有官方文档但有轻量变化空间。因此本设计的解析层必须
**逐字段防御式**：字段缺失或类型不合就跳过该字段，不做整体失败；上线路前抓真实
响应样张固化进单测（见实施步骤 0）。

### MiniMax Token Plan：`/v1/api/openplatform/coding_plan/remains`

| 项 | 值 |
| --- | --- |
| 端点（CN） | `GET https://api.minimaxi.com/v1/api/openplatform/coding_plan/remains` |
| 端点（EN） | `GET https://api.minimax.io/v1/api/openplatform/coding_plan/remains` |
| 认证 | `Authorization: Bearer <当前 profile 解析出的 provider 凭据>` |

域名注意：MiniMax 当前文档主推 `platform.minimax.cn` / `api.minimax.cn`，但额度
接口只在 `api.minimaxi.com` / `api.minimax.io` 有出处（cc-switch 注释称「同一账号
体系与 Key」，此为 cc-switch 单方注释、非官方承诺）。**minimax.cn 站的订阅 Key
能否查询 minimaxi.com 的接口，必须在实施步骤 0 真机验证**；若不通，CN 入口改指
可用的域名并在文档中记录。

响应结构（关键字段，以 cc-switch `parse_minimax_tiers` 消费口径为准）：

```json
{
  "model_remains": [
    {
      "model_name": "general",
      "current_interval_remaining_percent": 73.2,
      "current_weekly_status": 1,
      "current_weekly_remaining_percent": 41.5,
      "end_time": 1761308400000,
      "weekly_end_time": 1761900000000
    }
  ],
  "base_resp": { "status_code": 0, "status_msg": "success" }
}
```

解析规则：

- 只取 `model_remains[]` 中 `model_name == "general"` 的条目（编程套餐）；`video`
  等其它模型条目跳过。
- 5h 层：`current_interval_remaining_percent` 是**剩余**百分比，已用 = `100 - 剩余`。
  重置时间取 `end_time`（epoch 毫秒；容错兼容字符串 ISO 8601）。
- 周层：仅当 `current_weekly_status == 1` 时激活并展示；`status == 3` 表示该套餐
  无周限额（此时剩余恒为 100），**不渲染周进度条**，避免展示一条永远满格的假数据。
- 业务错误：`base_resp.status_code != 0` 时按确定性失败处理，文案取 `status_msg`。
- 空数组 / 字段缺失：返回空 tier 列表 + 「未查询到套餐额度」的确定性提示，不报错。

### DeepSeek 按量余额：`/user/balance`

| 项 | 值 |
| --- | --- |
| 端点 | `GET https://api.deepseek.com/user/balance` |
| 认证 | `Authorization: Bearer <当前 profile 解析出的 provider API Key>` |
| 文档 | [api-docs.deepseek.com/api/get-user-balance](https://api-docs.deepseek.com/api/get-user-balance) |

```json
{
  "is_available": true,
  "balance_infos": [
    { "currency": "CNY", "total_balance": "110.00", "granted_balance": "10.00", "topped_up_balance": "100.00" },
    { "currency": "USD", "total_balance": "5.21", "granted_balance": "0.00", "topped_up_balance": "5.21" }
  ]
}
```

解析规则：

- `balance_infos[]` 逐条展示：币种、总额、赠送（未过期）、充值。金额是数字字符串，
  透传展示，不转浮点参与任何计算。
- `is_available == false` → 余额不足以发起调用的告警态，即使数字大于 0 也单独标红
  （可能处于赠送过期 / 欠费恢复期）。
- 401 / 403 → Key 失效（确定性），与网络错误严格区分。
- DeepSeek 的并发数限额（rate limit 文档中的 2500 / 500）是静态账号级参数，与本功
  能「还剩多少」无关，v1 不展示。

### 智谱 GLM 编程套餐：`/api/monitor/usage/quota/limit`

| 项 | 值 |
| --- | --- |
| 端点 | `GET https://bigmodel.cn/api/monitor/usage/quota/limit?type=<1\|2>`（个人 = 1，团队 = 2） |
| 认证 | `Authorization: <API key>`（**raw 形式，不带 Bearer**） |
| 必需请求头 | `bigmodel-organization: org-xxx`、`bigmodel-project: proj_xxx`（组织 / 项目上下文） |
| 出处 | 未文档化的 Web 端点；调用要件由社区实现 [pi-glm-quota](https://github.com/focksor/pi-glm-quota) 实测（2026-07-30 套餐改版后的积分制套餐） |

缺 `type` → 业务错误「当前用户不存在 coding plan」；缺组织 / 项目头 → 空 `data:{}`。
这两个上下文值不是秘密，与 Key 走同一条解析链（环境变量 → `.credentials.yaml`
refs → `.env`）配置：`ZAI_CODING_CN_ORGANIZATION` / `ZAI_CODING_CN_PROJECT` /
`ZAI_CODING_CN_PLAN_TYPE`（缺省 1）。获取方法：浏览器登录 bigmodel.cn 的
coding-plan 页，DevTools Network 面板里 `quota/limit` 请求的请求头与 `type`
查询参数。上下文未配置是**确定性失败但不是凭据失效**（不标 expired，照常参与
TTL 刷新，配好后自动恢复），错误文案内联给出上述获取方法。

响应结构（关键字段）：

```json
{
  "data": {
    "limits": [
      {
        "type": "CREDIT_LIMIT", "unit": 3, "number": 5,
        "percentage": 12.0, "usage": 4800, "currentValue": 576, "remaining": 4224,
        "nextResetTime": 1761308400000
      },
      { "type": "CREDIT_LIMIT", "unit": 6, "number": 1, "percentage": 48.0, "nextResetTime": 1761900000000 }
    ]
  }
}
```

解析规则：

- 只取 `limits[]` 中 `type == "CREDIT_LIMIT"` 的条目；`unit == 3` 是 5 小时滚动
  窗口（tier 名 `5h`），`unit == 6` 是周窗口（tier 名 `weekly`），其余 unit 跳过。
- `percentage` 是**已用**百分比（0-100，与 MiniMax 的剩余口径相反）——统一换算成
  剩余百分比（`100 - percentage`）后进入与 MiniMax 相同的 tier 展示路径（三档
  配色、重置倒计时同款）。
- `nextResetTime` 是 epoch 毫秒；积分数字（`usage` / `currentValue` /
  `remaining`）v1 不展示（与「不虚构绝对 token 数」的能力边界一致，字段保留
  待用户反馈后再决定）。
- `data` 缺失 / 非对象：顶层 `msg` / `message` 有可读文案则透出（业务拒绝），
  否则按结构不认识处理；`limits` 缺失或空数组返回空 tier 列表。

## 凭据设计

### 复用当前内核模型凭据

查询 Key 不由 dsh-xlink 重新收集，也不写入 Shell 自己的 `settings.json`。查询时复用当前
DSH 实例 / profile 已配置的模型凭据：

- 当前实例由 Shell 的默认实例解析器确定；凭据根目录是
  `paths::instance_dsh_home(family, instance_id)`，即该实例的 `DSH_HOME`。
- DSH 的本地凭据提供方默认把引用值存于 `<DSH_HOME>/.credentials.yaml`。旧版
  `settings.yaml` 只作为内核迁移输入，不能作为新实现的凭据来源。
- provider 到 credential reference 的解析必须沿用当前 profile 的 `apiKeyEnv`；只有
  profile 没有显式引用时，才使用内核模型页同样的派生规则，例如 provider route
  `minimax-cn` 的默认引用为 `MINIMAX_CN_API_KEY`。DeepSeek 的实际引用以 profile
  `apiKeyEnv` 为准，不能从 route 名称臆造。
- 凭据优先级必须与 DSH 一致：启动环境快照优先，其次是 `.credentials.yaml`，再其次是
  DSH 支持的 `.env` 回退层。若 Shell 无法完整复现该解析优先级，就必须通过内核提供的
  受控代理完成查询，不能声称使用的是“当前模型 Key”。
- dsh-xlink 只读取当前实例实际引用的值，用于构造 HTTPS 请求头；不把原始值返回给 UI、
  不写入缓存、不写入 Shell 日志，也不提供独立的保存 / 清空凭据操作。

DSH 的 `credentials.describe` 只返回 `configured`、来源和可写性，不返回秘密值；因此不能
通过工作台 Remote API “提取” Key。若采用直接读取凭据文件，必须在 Rust 侧实现严格的
只读、路径约束、权限检查和格式解析，并覆盖环境层 / 文件层的优先级；若采用内核代理，
则代理必须只返回查询结果，不能把 Key 回传给 Shell。

这条兼容性是本功能的硬前提：如果 MiniMax Token Plan 接口拒绝当前 dsh provider 的模型凭据，
就不能通过 Shell 另加一个“订阅 Key”输入框绕过；应暂停 MiniMax 展示，并在 dsh 内核增加
只返回额度结果的查询能力后再接入。错误提示必须明确说明“当前模型凭据不能用于该接口”，
不能把一个按量 Key 当成 Token Plan Key 继续重试。

### 设置页交互（`SettingsPanel.vue`）

设置页**不设「套餐用量查询」卡**（真机反馈：与概览页的「套餐用量」卡重复——
展示、刷新、测试入口都由概览卡承担），也不把 Key 加入 Shell `Settings`。当前 dsh
模型设置页仍是唯一的凭据编辑入口：

- 概览「套餐用量」卡承担原本规划给设置页的入口：「刷新」即
  `get_subscription_usage(force=true)` 的逐 provider 测试；凭据未配置时卡头提供
  「前往模型设置」（打开工作台）。
- Key 失效时错误横幅提示用户回到工作台模型设置更新对应 provider；错误信息不得回显
  Key 或凭据文件内容。
- 由于凭据属于实例，切换实例后必须重新解析凭据、重新判定 configured 状态，并读取该
  实例自己的订阅缓存。

## 架构落点

新增独立模块 `src-tauri/src/subscription.rs`，与 `usage.rs` 同级同风格：

| 关注点 | usage.rs（本地统计） | subscription.rs（本功能） |
| --- | --- | --- |
| 数据源 | 实例 sessions 目录（zstd 增量扫描） | 两家云端 HTTPS 接口 |
| 持久化 | `<instance>/usage/state.json` | `<instance>/subscription-cache.json`（Key 跟随当前 DSH 实例 / profile，缓存必须按实例隔离） |
| 状态文件 | `state.rs` 容错读 + 原子写，`STATE_SCHEMA = 1` | 同一套路，`CACHE_SCHEMA = 2`；缓存绑定实例、profile 与 credential fingerprint；「条目不存在」（未配置）与「条目存在但 `credential_status: expired`」（凭据失效）是两个互斥状态 |
| 并发控制 | `SCAN_LOCK` | `FETCH_LOCK`：同 provider 在途请求去重（singleFlight 的 Rust 侧形态） |
| 新鲜度 | 45s 扫描窗口 + UI 60s TTL | **5 分钟缓存 TTL**；force 越过 |

### HTTP 栈适配（与 cc-switch 的关键差异）

cc-switch 用 reqwest async；本仓库用 **ureq 3 阻塞式**（`default-features = false`，
`features = ["json", "rustls"]`，无 gzip——见 `Cargo.toml` 既有注释）。因此：

- 查询函数写成阻塞函数，由 Tauri 命令经 `tauri::async_runtime::spawn_blocking` 调用
  （与 `usage_view` 同款），不引入新依赖、不动 Cargo.toml。
- 超时 15 秒；错误处理对照 `releases.rs` / `pkg.rs` 的既有惯例（连接失败、超时、
  非 2xx、响应非法 JSON 各自有明确文案）。

### 错误通道语义（统一版，替代规划稿的两处矛盾表述）

沿用 cc-switch 的二分，但**重试语义只有一条规则**：

- **瞬时失败**（网络不可达 / 超时 / 读体中断）→ 命令返回 `Err(文案)`；**缓存不写、
  不删**——前端 keep-last-good 继续展示旧值 + 一条「查询失败」横幅。绝不把失败
  渲染成「0 余额 / 0%」误导用户（与 `state.rs`「不存在 ≠ 损坏」同一哲学）。
- **确定性失败**（Key 失效 / 业务错误码 / 响应结构不认识）→ 命令返回
  `Ok(success=false, error=…)`；缓存条目**保留旧 tiers 但置 `credential_status`，
  并写入 `error` 字段**。
- **重试规则**：`credential_status == "expired"` 的条目**不参与自动刷新**（避免拿失
  效 Key 反复打接口），概览页 mount 的 TTL 拉取跳过它；**用户 force（点刷新 / 测试
  连接）永远重试**。余额不足（`is_available == false`）不算 expired，照常参与 TTL
  刷新。

### 缓存文档（`<instance>/subscription-cache.json`）

```json
{
  "schema": 2,
  "instance": { "family": "dsh", "id": "default", "profile": "web" },
  "providers": {
    "minimax_cn": {
      "credential_ref": "MINIMAX_CN_API_KEY",
      "credential_fingerprint": "sha256:…",
      "kind": "plan",
      "tiers": [
        { "name": "5h", "remaining_percent": 73.2, "resets_at_ms": 1761308400000 },
        { "name": "weekly", "remaining_percent": 41.5, "resets_at_ms": 1761900000000 }
      ],
      "credential_status": "valid",
      "queried_at_ms": 1761308400000,
      "error": null
    },
    "deepseek": {
      "credential_ref": "DEEPSEEK_API_KEY",
      "credential_fingerprint": "sha256:…",
      "kind": "balance",
      "is_available": true,
      "balances": [
        { "currency": "CNY", "total": "110.00", "granted": "10.00", "topped_up": "100.00" }
      ],
      "credential_status": "valid",
      "queried_at_ms": 1761308400000,
      "error": null
    }
  }
}
```

- `credential_ref` 只能记录当前 profile 使用的引用名，不能记录 Key 值。
- `credential_fingerprint` 是用于判断缓存是否仍属于当前凭据的不可逆摘要，不参与 UI 展示；
  当前引用解析出的值或来源发生变化时，旧数据必须先标记为 stale，不能当作当前账号数据。
- 未配置模型凭据的 provider **不产生缓存条目**（UI 据此区分「未配置」与「失效」）。
- `tiers` / `balances` 存的是**上一次成功**的数据；确定性失败时旧数据不动，只更新
  `credential_status` 与 `error`。
- 文件损坏按 `usage_ctx()` 同款文案处理（提示可删除重建），绝不静默清空。

### Tauri 命令（2 条，不单独设计 test 命令）

```
get_subscription_usage(provider: Option<String>, force: Option<bool>) -> Result<SubscriptionView, String>
open_subscription_window(app: AppHandle) -> Result<(), String>
```

- `provider = None` 返回全部已配置 provider；`Some(id)` 只查一个——「测试连接」直接
  复用 `Some(id) + force=true`，不另设命令。
- `SubscriptionView`：`providers: Vec<ProviderView>`，每个含 `id` / `kind` / 
  `configured` / `credential_status` / `error` / `queried_at_ms` / `tiers` 或
  `balances`。全部字段 `serde::Serialize`，前端零推导。
- `open_subscription_window` 与 `open_usage_window` 同一条路：新 OS 线程建窗
  （Windows 主线程同步建 webview 会死锁）、mpsc 回传结果、20s 超时兜底；已有窗口
  先 `destroy()` 再重建（窗口只读，重建即顺手 force）。

## 前端设计

### 状态模块（`ui/src/subscription.js`，与 usage.js 同构）

**keep-last-good 在前端显式落地**（cc-switch 靠 react-query 天然行为，本仓库的
`async.js` 没有这层，必须自己写）：

```js
export const subscription = reactive({
  loading: false,
  data: null,     // 上一次成功结果；失败时不清空
  error: null,    // 最近一次失败的用户文案；成功时置 null
  loadedAt: 0,    // Date.now()，60s 内免重复请求（与 usage 卡片同款 TTL）
});
```

`loadSubscriptionSummary()`（TTL 内复用）、`refreshSubscription(provider)`（force）、
`openSubscriptionWindow()` 三个动作，失败路径只写 `error` 不动 `data`。

### 概览页（`OverviewPanel.vue`）

「当前内核」卡下方一个**独立的只读卡**（实施时从「行内入口」改为独立卡，且
内容**直接展示、无折叠**——真机反馈折叠态摘要行会被错误文案挤成多行，且与
横幅、分区三处重复；原「桌面端设置」卡已并入「当前内核」标题的 Node 环境
tooltip，本卡成为「当前内核」下唯一的概览卡片）：

```
套餐用量
  ⚠ DeepSeek：凭据无效或无权限（HTTP 401）。请到工作台的模型设置更新…
  MiniMax Token Plan                     查询于 2 分钟前
  5 小时窗口   ███████░░░░  剩余 73%   3 小时 47 分后重置
  本周窗口     ████░░░░░░░  剩余 42%   4 天 11 小时后重置
  DeepSeek 按量余额
  CNY ¥110.00（赠送 ¥10.00 · 充值 ¥100.00）  凭据失效
  [刷新] [查看详情] [前往模型设置]
```

- 完整可操作错误文案只出现在卡顶的错误横幅；provider 分区与横幅不重复铺长文，
  仅用**短状态词**（「凭据失效」「查询失败」）就近标注。
- 「查询于 X 分钟前」由 `queried_at_ms` 派生——5 分钟 TTL 内数据会陈旧，必须给用户
  时间坐标。
- 未配置任何模型凭据：不发请求，只显示「未配置可查询的模型凭据 · 前往模型设置」。
- 独立窗口按钮「查看详情」→ `open_subscription_window`。

### 独立窗口（`ui/src/SubscriptionWindow.vue` + capability）

完全复刻 usage-viewer 范式：

- URL `?subscription=1` 挂载；`src-tauri/capabilities/subscription-viewer.json`
  仿照 `usage-viewer.json`，只授予 `get_subscription_usage`。
- 尺寸直接复用 `window::USAGE_VIEWER_SIZE`（760×800，min 720×520），不新增常量；
  吸附 / 拖动跟随（`dock_position_logical` + `attach_dock_listener`）白拿。
- 窗口内容：两个 provider 分区 + 「刷新」（force）。v1 不画历史曲线（要曲线得在
  缓存里保留快照序列，等用户反馈再决定），但窗口底部放一行链接说明「本地 token
  用量统计见模型用量窗口」，用 `open_usage_window` 互跳；「前往模型设置」跳回工作台的
  provider 设置页。
- 窗口打开即 `refreshSubscription()`（force 全量）。

### 展示规则

- **进度条三档配色**（按**剩余**百分比，与 cc-switch 的已用口径相反，tooltip 里
  双向注明「已用 X% · 剩余 Y%」）：

  | 剩余 | 颜色 |
  | --- | --- |
  | ≥ 50% | 主题绿 `--accent` |
  | 20–49% | `--el-color-warning` |
  | < 20% | `--el-color-danger` |

  不做闪烁 / 脉冲动画（项目静态优先的既有哲学）。
- **重置倒计时**：相对时间 `X 小时 Y 分` / `X 天 Y 小时`；格式化函数放
  `ui/src/labels.js`（时间相关展示的既有去处），供卡片与窗口共用。
- DeepSeek `is_available == false`：金额照常展示，右侧红色「余额不足，无法发起调用」。
- MiniMax 国内 / 国际两个 provider 凭据都已配置时两个分区上下排列；只配置一个就只显示一个。

### 错误文案口径

本功能的错误详情（HTTP 状态码 + 截断后的响应摘要）直接内联展示在卡片 / 窗口上；
不得写入原始凭据或凭据文件内容。查询事件与错误仍应落入现有 Shell 日志，以便用户
通过「查看日志」反馈问题。文案统一为：

- 瞬时失败：「查询失败（网络不可达或超时）。已保留上次结果，可点击刷新重试。」
- 凭据失效：按 provider 定制——DeepSeek：「DeepSeek 凭据无效或无权限（HTTP 401）。
  请到工作台的模型设置更新 DeepSeek 的 API Key。」；MiniMax：「MiniMax 凭据无效或
  无权限（HTTP 401）。请到工作台的模型设置更新对应 provider 的 API Key（注意：
  查询套餐需用 Token Plan 页的订阅 Key，不是接口密钥页的按量 Key）。」
- 结构不认识：「接口返回了无法识别的数据结构，可能已改版。请到项目仓库反馈。」

## 实施步骤（4 步 + 1 步前置，每步独立可验证）

> 每步合并前跑 `cargo clippy --all-targets && cargo fmt`（Rust）与
> `npm run build:ui`（前端）；**每步涉及的新文件必须在同一提交里在
> `scripts/check-code-budget.mjs` 登记预算条目**（AGENTS.md 硬门禁）；文档随步同步，
> 步骤 4 做整体校对。

### 步骤 0：接口与凭据解析真机验证（先行）

使用当前 DSH 实例 / profile 已在模型设置中配置的真实凭据验证三件事；不得把 Key
复制到 Shell 设置或测试夹具中。样张只保存脱敏后的响应，原始响应不得进入仓库：

1. MiniMax `/coding_plan/remains` 的真实响应结构（字段名 / 时间戳格式 / 周层 status 取值）。
2. 当前 profile 的 `apiKeyEnv`、默认 provider reference 和实际凭据来源（环境、
   `.credentials.yaml`、`.env`）能否被 Shell 与 DSH 解析成同一把 Key。
3. `platform.minimax.cn` 站配置的凭据能否查询 `api.minimaxi.com`；不通则记录验证通过
   的入口，并回写本文档 3.1 节。
4. 直接读取凭据文件是否能满足权限、锁、热更新和跨平台要求；否则改为在内核侧增加
   只返回查询结果的受控代理，不能通过 Remote API 返回秘密值。

DeepSeek 侧用当前模型设置中的实际 provider 配置验证余额接口；错误响应按实际行为
对齐文案。

### 步骤 1：模块骨架 + DeepSeek 余额

- `subscription.rs`：当前实例 / profile 的 provider 与 credential reference 解析、
  只读凭据读取、不可逆 fingerprint、缓存文档读写（`state.rs` 容错读 + 原子写）、
  `FETCH_LOCK`、`query_deepseek`、错误通道实现。
- `paths.rs`：`instance_subscription_cache_file(family, instance_id)`，缓存与模型
  用量一样跟随实例，不放到 Shell 全局设置目录。
- `commands.rs`：`get_subscription_usage` 注册，并确保命令只使用当前实例解析出的凭据。
- 设置页只增加当前 provider 的 configured 状态、「测试连接」和「前往模型设置」，不增加
  Key 输入框、不增加 Shell Settings 字段。
- 概览页收起态「套餐用量」行（先只有 DeepSeek 金额）。
- 预算条目：`subscription.rs`、`SettingsPanel.vue` 增量。

验收：在 dsh 模型设置中配置凭据 → 概览显示 CNY / USD 余额；模型设置中改坏凭据 →
「凭据已失效」+ 旧值保留；断网 → 「查询失败」+ 旧值保留；切换实例后不会显示另一个
实例的余额。

### 步骤 2：MiniMax Token Plan

- `subscription.rs`：`query_minimax(resolved_credential, is_cn)`、`parse_minimax_tiers`
  （纯函数，步骤 0 样张做单测，覆盖：正常双窗口、无周限额 status=3、`model_remains`
  空、`base_resp` 业务错误、字段类型漂移）。
- MiniMax provider 与当前 profile 的凭据 reference 接线，不在 Shell 设置页复制 Key。
- 概览卡展开态：双进度条 + 重置倒计时 + 三档配色。

验收：在 dsh 模型设置中配置对应 provider → 双进度条正确；无周限额套餐 → 周条不渲染；
凭据失效 → 横幅 + 旧值保留。

### 步骤 3：独立窗口 + 体验打磨

- `SubscriptionWindow.vue` + capability + `open_subscription_window`。
- keep-last-good、`credential_status == expired` 跳过自动刷新、陈旧度展示。
- 「前往模型设置」「模型用量窗口互跳」以及主面板 / 独立窗口的数据同步。
- 独立窗口和主面板均显示当前实例与 profile，避免用户误解余额所属账号。

验收：窗口打开即 force；断网后窗口仍显示缓存 + 横幅；expired 后点刷新才重试；切换
实例或 profile 后不会继续展示原实例缓存。

### 步骤 4：文档与单测收口

- `docs/architecture.md` 模块清单加 `subscription.rs` 段（与 `usage.rs` 段并排）。
- `README.md` 在「模型用量」旁补「套餐 / Token Plan 用量」一节（含能力边界声明、
  dsh 模型设置是唯一凭据入口、实例作用域和错误处理说明）。
- 单测补齐：credential reference 解析与优先级、凭据文件容错读、缓存 schema / fingerprint、
  两个 parse 纯函数（MiniMax 样张 + DeepSeek 官方样张）。
- 安全验收：grep 新增路径确认原始 Key 不出现在 Shell UI、日志、事件、缓存、toast 或
  测试样张中。

## 风险与对策

| 风险 | 对策 |
| --- | --- |
| MiniMax 未文档化接口改版 | 步骤 0 真机样张固化单测；解析逐字段防御（缺字段跳过不整体失败）；错误文案引导用户反馈 |
| minimax.cn 账号体系与 minimaxi.com 不互通 | 步骤 0 首项验证；不通则换域名并回写文档 |
| 当前 profile 的 provider reference 与默认派生规则不一致 | 步骤 0 读取实际 `apiKeyEnv`；查询只使用 profile 解析出的引用，不按 provider 名硬编码覆盖 |
| 凭据来源在环境 / 文件 / .env 之间不一致 | 复用 DSH 的同一解析优先级；无法复现时改用内核侧只返回查询结果的受控代理 |
| 失败被渲染成 0 余额误导用户 | 错误通道硬约束：缓存不写不删，UI keep-last-good |
| 拿失效凭据反复打接口 | expired 条目跳过自动刷新，仅 force 重试 |
| 代码预算门禁拦住新模块 | 每步在同一提交登记预算（见实施步骤引言） |
| 多实例下不同账号 | 查询凭据、缓存和展示都绑定当前实例 / profile，不允许 Shell 全局缓存串用 |

## 不在 v1 范围

- Claude / Codex / Gemini 官方订阅配额、GitHub Copilot——cc-switch 那类「自动查询」
  依赖其本地代理截获请求，dsh-xlink 的架构里外壳不是内核请求的代理方，做不了。
- Kimi / 火山方舟 / StepFun / SiliconFlow / OpenRouter / Novita 等其它 provider
  ——模块已按「每家一个 query + parse 纯函数」预留扩展位，逐家接入各约 100–200 行
  （智谱 GLM 编程套餐已按此模式接入，见「接口规格」一节）。
- 自定义 JS 查询脚本（cc-switch 的高级能力）——避免在桌面壳里引入 JS 沙箱。
- 用量历史曲线、自动刷新间隔设置、绝对 token 数估算——见能力边界与缓存章节。
- Key 加密存储——本功能不新增凭据存储；凭据安全策略由 dsh 内核的 credentials provider 负责，
  dsh-xlink 只读并使用，不复制、不修改、不在缓存中保存原始 Key。
