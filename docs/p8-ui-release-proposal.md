# P8 UI / 集成测试 / 发布准备 提案

> P0–P7 全 stage + B 类日志 4 字段已修（45 commit）。本文档为
> P8 阶段剩余的 UI / 集成测试 / 发布准备给具体方案 + 列出决策点。

## 范围

P8 含 4 件事：
1. 实例列表 UI（多实例切换）
2. 插件面板「全局 / 单实例」视图切换
3. PR / release 边界（阶段性发版 vs 一次性发版）
4. 默认实例解析器全面接入（替换 `(KERNEL_FAMILY_DSH, "default")` 硬编码）

## 建议 1：实例列表 UI 形态

**顶部实例切换器（top-bar dropdown）** —— 与现有 `WindowTitleBar`
组件融合。

理由：
1. **状态可见性优先**：用户每次打开外壳都需要知道当前在哪个
   实例——顶部持续可见的指示器比侧栏多 tab 更直接
2. **切换是高频操作**：用户可能在「工作实例」+「测试实例」之间
   来回切，top-bar 单击比侧栏 2 次点击快
3. **不占用侧栏空间**：侧栏已 5 项，再加「实例列表」会变 6 项，
   信息密度过高
4. **与「活动 Shell 状态」语义同源**：top-bar 已经是状态胶囊所在

UI 形态：
- top-bar 当前显示「DeepSeek Harness 桌面管理台」
- 改造为：`[实例图标] 默认实例 ▼` dropdown
- dropdown 列表：实例 id + 状态（运行中 / 已停止）+ family
  + 「创建实例」「管理实例」入口
- 点击 dropdown 项 → 切换活动实例（实际由后端解析默认实例）
- 切换过程走 ProgressOverlay（与装内核 / 装插件同 UI）

不推荐侧栏多 tab：实例切换是「动作」不是「分类」——侧栏分类
「概览/版本/插件/技能/设置」表达「查看什么」，实例选择表达
「在哪个上下文中查看」。
不推荐独立实例管理页：日常切换不应跳页。

## 建议 2：插件面板视图切换

**双模式 toggle** —— `PluginsPanel.vue` 顶部加「全局 / 单实例」tab。

UI 形态：
- tab 1「全局」：当前所有实例共享的中央库插件（`dsh-plugins/<id>/`）
- tab 2「单实例」：仅当前实例物化的 `extensions/plugins/<id>/`
  （按 family + instance_id 过滤）

理由：
1. **全局 / 单实例是不同语义**：全局是「源」，单实例是「物化
   结果」——用户对两者管理动作不同（全局管更新，单实例管
   是否启用 / 是否隔离）
2. **复用现有 `PluginsPanel.vue` 结构**：中央库 / 物化 / 接线
   三段已经在 `plugins.js` state 里，加 tab 只需切 filter
3. **不引入新 panel**：避免侧栏 / 顶栏再添菜单项

不推荐独立新 panel（插件 / 实例 2 个 panel）：增加导航负担，
用户切换场景时会反复横跳。

## 建议 3：PR / release 边界

**阶段性发版（每 stage 一个 PR + tag）**——已有经验铺垫：
- 本轮多内核链已自然分段（P0–P8 共 24 笔代码 commit，对应
  5–9 个 PR 边界）
- 现有 `.github/workflows/desktop-release.yml` 是 `desktop-v*` tag
  触发，每个 PR 各自带 commit 不带 tag 即可
- 用户的 mavis `git tag 频率偏好` 规则明确指出：「不要每个 fix
  commit 都打 git tag——tag 应该是大版本节点」

理由：
1. **大版本节点已就绪**：本次多内核链可作为一个 `desktop-v0.2.0`
   （含 P0–P8 + B 类日志 4 字段），不需要每个 stage 单独 tag
2. **PR 边界 = review 边界**：阶段性发版让 reviewer 一次看一段，
   避免一次性发版 review 40+ commit
3. **风险分散**：阶段性发版能让用户逐步升级（如果某个 stage
   有 bug，不至于回退整个改造）

具体 PR 边界建议（按 commit 顺序）：
1. **PR 1**：P0 路径契约（commit `3ad6c8e`）
2. **PR 2**：P1 Shell 解耦（4 commit `67b1d11`/`e7df2ac`/`b7db029`/`6d9fb2b`）
3. **PR 3**：P2 实例注册表（5 commit `36677f2`/`a5ad9d9`/`3eced70`/`5d5b729`/`884b2fb`）
4. **PR 4**：P3 DshAdapter（commit `c0cfabc`）
5. **PR 5**：P4 plugins 多实例（5 commit `358ae5a`/`58b1d11`/`dcbae61`/`55f3535`/`8772105`）
6. **PR 6**：P5 skills 中央库 + customSkillDirs（2 commit `b09a525`/`7290e07`）
7. **PR 7**：P6 迁移向导后端（4 commit `b24e68e`/`89d76df`/`9615901`/`5416bca`）
8. **PR 8**：P7 mcode mock（commit `1308cb6`）
9. **PR 9**：B 类日志 4 字段（5 commit `eba8c96`/`158ced3`/`59db55b`/`30af557`/`9b2fce8`）
10. **PR 10**：迁移向导 UI（commit `efffe08` 后 3 笔，待实现）
11. **PR 11**：P8 UI 实例列表 + 插件面板（待实现）

最终发版：`desktop-v0.2.0` 一个 tag，包含 PR 1–11 所有 commit。

不推荐一次性发版：一次性 40+ commit PR 让 reviewer 难以消化，
出问题时回退粒度太大。

## 建议 4：默认实例解析器

**新增 `instance::resolve_default()` 函数** —— 替代散落 7+ 处的
`(KERNEL_FAMILY_DSH, DEFAULT_INSTANCE_ID)` 硬编码。

函数签名：
```rust
/// 当前 Shell 记住的默认实例；`InstanceRegistry::default_instance_id`
/// 已存在但未对外暴露。包装一层让所有 caller 走同一入口。
pub fn resolve_default() -> (Family, String) {
    (KERNEL_FAMILY_DSH, DEFAULT_INSTANCE_ID)
}
```

理由：
1. **集中点改默认语义只动 instance.rs 一处**——目前 `DEFAULT_INSTANCE_ID`
   是常量，已经够用；加 `resolve_default()` 让 caller 显式表达
   「我想要默认实例」而不是「我硬编码 (DSH, "default")」
2. **P8 UI 决策落地后**：把 `resolve_default()` 改成读
   `InstanceRegistry::default_instance_id` 即可，所有 caller
   自动跟进——这就是为什么需要这个函数的根本原因
3. **替换 7+ 处 caller**：`guard::GuardDeps` / `kernel_workbench_url_from_log`
   / `notify::*` / `start()` 等

工作量：~30 行（1 个函数 + 8 处替换 + 注释更新）。

## 关键决策点（待你拍板）

### 1. 实例列表 UI 形态

- ✅ **顶部 dropdown（建议）**
- ❓ 侧栏多 tab
- ❓ 独立实例管理页

### 2. 插件面板视图

- ✅ **单 panel + 双 tab（建议）**
- ❓ 两个独立 panel（全局 / 实例）
- ❓ 单实例 + 内嵌全局 toggle 折叠

### 3. PR / release 边界

- ✅ **阶段性发版（11 PR + 1 个 desktop-v0.2.0 tag，建议）**
- ❓ 一次性发版（1 PR + 1 tag）
- ❓ 单 stage 单 PR 单 tag（粒度太细，与 mavis tag 频率偏好冲突）

### 4. 默认实例解析器接入方式

- ✅ **新增 `instance::resolve_default()` 函数 + 8 处替换（建议）**
- ❓ 直接替换硬编码为 `DEFAULT_INSTANCE_ID` 常量（已做，但耦合）
- ❓ 改 InstanceRegistry 暴露 `default_instance_id()` 方法（侵入性大）

## 工作量估算

| 任务 | 行数 | commit 数 |
|------|------|----------|
| 顶部 dropdown | ~150 前端 | 1 |
| PluginsPanel 双 tab | ~80 前端 | 1 |
| `resolve_default()` + 8 处替换 | ~30 后端 | 1 |
| 集成测试（多实例切换 / 插件隔离） | ~200 后端 | 1 |
| 总计 | ~460 行 | 4 commit |

## 待确认

请你确认：

1. **实例列表 UI**（顶部 dropdown / 侧栏 tab / 独立页）
2. **插件面板视图**（单 panel + 双 tab / 双 panel / 单实例+toggle）
3. **PR / release 边界**（阶段性发版 / 一次性 / 单 stage 粒度）
4. **默认实例解析器**（`resolve_default()` 函数 / 常量直替 / Registry 暴露方法）

收到回复后即可开始实现——预计 4 笔 commit，与 P6 step 5 提案（3 commit）
合计 ~7 commit 完成 P6 step 5 + P8 全量。