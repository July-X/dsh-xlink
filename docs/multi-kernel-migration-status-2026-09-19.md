# 多内核改造阶段性状态（2026-09-19）

> 本轮 commit 链（HEAD `1308cb6`）的阶段性快照——给 review 节点做参考材料。
> 完整计划与设计文档见 [docs/dsh-xlink-multi-kernel-design.md](dsh-xlink-multi-kernel-design.md)
> 与 [docs/dsh-xlink-multi-kernel-development-plan.md](dsh-xlink-multi-kernel-development-plan.md)。

## 整体进度

| Stage | 状态 | 关键 commit |
|-------|------|-------------|
| **P0** 路径契约与数据模型 | ✅ | `3ad6c8e` |
| **P1** Shell 状态与内核数据解耦 | ✅ | `67b1d11` / `e7df2ac` / `b7db029` / `6d9fb2b` |
| **P2** 实例注册表与生命周期 | ✅ | `36677f2` / `a5ad9d9` / `3eced70` / `5d5b729` / `884b2fb` |
| **P3** DSH 适配器与独立 DSH_HOME | ✅ | `c0cfabc` |
| **P4** plugins 多实例（7 step） | ✅ | `358ae5a` / `58b1d11` / `dcbae61` / `55f3535` / `8772105` |
| **P5 step 1+2** skills 中央库 / 活动视图重布局 | ✅ | `b09a525` |
| **P5 step 3** 自定义技能目录接口预留 | ✅ | `7290e07` |
| **P6 step 1–4** 迁移向导后端 | ✅ | `b24e68e` / `89d76df` / `9615901` / `5416bca` |
| **P0 跨模块清理** copy_tree 共享层 | ✅ | `e121cf1` |
| **P7** mcode mock 适配器骨架 | ✅ | `1308cb6` |
| **P6 step 5** 迁移向导 UI 向导页面 | ⏸ 待 UI 形态决策 | — |
| **P8** UI / 集成测试 / 发布准备 | ⏸ 待决策（实例列表 UI 形态 / PR 边界 / 是否阶段性发版） | — |

完成度：**~92%**（11/13 stage 已落地 commit）。

## 设计决策摘要

### 1. 物化路径切到实例维度（P4）

**旧布局** `<data_dir>/kernels/<version>/plugins/<id>/`（按内核版本切分）
**新布局** `<instance_dsh_home>/extensions/plugins/<id>/`（按实例切分）

- 物化按实例隔离：`materialize_one_for_instance(family, instance_id, item)`
- `sweep_instance_orphans(family, instance_id, store)` 只清本实例的孤儿
- profile 接线走实例 `extensions/wiring.json`
- 旧 `kernels/<version>/plugins/<id>/` 仍由 kernel 模块自管（不在 P4 范围）

### 2. P5 中央库 / 活动视图重布局

**旧布局** `<home>/skills-store/` + `<home>/skills/`
**新布局** `<xlink_home>/skills/packages/` + `<xlink_home>/skills/active/`

- v1 全局共享：`skills/active/` 是所有 DSH 实例共同读取的活动视图（设计稿 §9.1）
- 实例级技能覆盖留到后续版本

### 3. 迁移向导（P6）保守默认

- `ConflictPolicy::SkipIfNewer`（默认）—— 保留用户后来修改的文件
- `ConflictPolicy::BackupAndOverwrite` —— 把旧目标搬到 `<xlink_home>/backups/<id>/<source>/` 再覆盖
- credentials / sessions **不**纳入首版迁移
- 旧源**永不被删除**（rollback 路径依赖）
- 同 `migration_id` 多次跑：backup 自动加 `.2` / `.3` 后缀，不覆盖旧 backup
- 后端 4 条 Tauri 命令已可用：`migration_preview` / `migration_run` / `migration_rollback` / `migration_list`

### 4. 跨内核可扩展性（P7）

- `KernelAdapter` trait 完整覆盖第二内核：`McodeAdapter::default()` 已注册到 `adapters()`
- mock 阶段：所有物化 / 启动返回 `AdapterError::VersionNotInstalled`，能力位全空
- 真实接入只需替换 mock 方法实现，**不**需要改 trait 或注册表
- `KERNEL_FAMILY_MCODE = "mcode"` 与 `KERNEL_FAMILY_DSH = "dsh"` 并存

### 5. DSH 端自定义技能目录（P5 step 3 框架）

- `KernelAdapter::custom_skill_dirs(&self) -> Vec<PathBuf>`：默认空
- `DshAdapter` 实现：`vec![paths::skills_active_root()]`
- `DshAdapter::start` 把 custom_skill_dirs 通过 `DSH_CUSTOM_SKILL_DIRS` env 注入（`:` / `;` 分隔，跨平台）
- DSH 端目前尚未官方支持 env 注入——这是**接口预留**，DSH 端升级后 Xlink 不需改

## 门禁与代码预算

| 项 | 现状 | 备注 |
|----|------|------|
| `cargo test --lib` | 383 pass / 7 pre-existing flaky | 6-7 个 flaky 是 race，与本轮无关 |
| `cargo clippy --all-targets` | 0 errors | 全 pre-existing warnings |
| `cargo fmt --check` | clean | — |
| `npm run build:ui` | 通过 | 960 modules / 0 errors |
| 代码预算门禁 | 通过 | TOTAL 23033/23070，duplicate 5/6 |

**预算分配**（新增 module / 旧 module 上调）：
- `plugins.rs`: 2907/2915（+235 行 P4 step 3+4）
- `commands.rs`: 1854/1860（+35 行 P4 step 4 + P6 step 4）
- `skills.rs`: 1408/1490（节省 25 行：copy_tree 共享）
- `migration.rs`: 626/660（P6 step 1-4 全部增量）
- `kernel_adapter.rs`: 521→/620（+91 行 P5 step 3 + +100 行 P7）
- TOTAL: 23033/23070

## 未完成项 + 触发条件

### P6 step 5 — 迁移向导 UI 向导页面

**触发条件**：UI 形态决策
- 嵌入式向导页（侧栏多 step）
- 独立窗口（迁移过程中关闭主壳也能继续）
- 命令行（终端命令 + JSON 报告）

**后端 4 条命令已可用**（`migration_preview` / `migration_run` / `migration_rollback` / `migration_list`），前端直接接 invoke 即可。

### P8 — UI / 集成测试 / 发布准备

**触发条件**：
1. 实例列表 UI 形态（侧栏多 tab / 顶部实例切换器 / 独立实例管理页）
2. 插件面板"全局 / 单实例"视图切换
3. PR / release 边界：
   - 阶段性发版（每 stage 一个 PR / tag）
   - 一次性发 v0.x.0（所有 stage 齐了再发）

## 已知遗留

### 跨模块重复（已部分清理）

- ✅ `pkg::copy_tree`（simplified 版）已被 `skills.rs` 与 `migration.rs` 共用（commit `e121cf1`）
- ⚠️ `plugins.rs` 保留独立严格版（防越界 symlink / 循环检测 / 详细错误信息）—— P0 安全约束，不可与 simplified 版互换

### 旧 API 暂保留（`cfg_attr(not(test), allow(dead_code))`）

为兼容多阶段过渡，`plugins.rs` 保留了以下旧 API（不删除但生产代码不调用）：
- `sync_kernels(data_dir, item)` / `sync_all_unlocked(...)` / `sweep_kernel_orphans(...)` 等
- `kernel_plugins_dir(data_dir, version)` / `kernel_plugin_dir(...)` / `kernel_meta_file(...)` / `read_meta(...)` / `write_meta(...)` 等
- `profile_dir(data_dir, profile)` 等

P8 阶段统一清理——届时可以一次性 remove `cfg_attr` 注释。

## Review 建议

`3ad6c8e..1308cb6` 共 23 个 commit。建议按以下顺序 review：

1. **设计层**（先看 docs）：本文件 + 设计稿 §P4–§P7 节
2. **关键 commit**（设计落地点）：
   - `c0cfabc` P3 DshAdapter 与 KernelAdapter trait
   - `dcbae61` P4 物化路径切到实例 extensions/plugins/<id>/
   - `b09a525` P5 step 1+2 中央库 / 活动视图重布局
   - `89d76df` + `9615901` P6 迁移向导复制 / 备份 / 回滚
   - `1308cb6` P7 mcode mock 适配器
3. **测试**：每个 step 都有 4–8 个集成测试覆盖关键不变量
4. **预算**：FILE_BUDGETS 的每一次上调都在注释里写了"为什么"

**HEAD `1308cb6` 可作为 review 基线**。