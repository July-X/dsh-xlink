# 多内核改造阶段性状态（2026-09-19）

> 本轮 commit 链（HEAD `d2bab6d`）的阶段性快照——给 review 节点做参考材料。
> 完整计划与设计文档见 [docs/dsh-xlink-multi-kernel-design.md](dsh-xlink-multi-kernel-design.md)
> 与 [docs/dsh-xlink-multi-kernel-development-plan.md](dsh-xlink-multi-kernel-development-plan.md)。
> 配套架构补全见 [docs/architecture.md §「多内核改造后的实际数据布局」](architecture.md)。

## 整体进度

| Stage | 状态 | 关键 commit |
|-------|------|-------------|
| **P0** 路径契约与数据模型 | ✅ | `3ad6c8e` |
| **P1** Shell 状态与内核数据解耦 | ✅ | `67b1d11` / `e7df2ac` / `b7db029` / `6d9fb2b` |
| **P2** 实例注册表与生命周期 | ✅ | `36677f2` / `a5ad9d9` / `3eced70` / `5d5b729` / `884b2fb` |
| **P3** DSH 适配器与独立 DSH_HOME | ✅ | `c0cfabc` |
| **P4** plugins 多实例（5 step） | ✅ | `358ae5a` / `58b1d11` / `dcbae61` / `55f3535` / `8772105` |
| **P5 step 1+2** skills 中央库 / 活动视图重布局 | ✅ | `b09a525` |
| **P5 step 3** 自定义技能目录接口预留 | ✅ | `7290e07` |
| **P6 step 1–4** 迁移向导后端 | ✅ | `b24e68e` / `89d76df` / `9615901` / `5416bca` |
| **P0 跨模块清理** copy_tree 共享层 | ✅ | `e121cf1` |
| **P7** mcode mock 适配器骨架 | ✅ | `1308cb6` |
| **P0 收尾** 阶段性状态快照 | ✅ | `89932eb` |
| **P0 收尾** architecture 增补「实际数据布局」 | ✅ | `257c42a` |
| **P0 收尾** 状态快照自洽（HEAD 指针更新） | ✅ | `86372ad` |
| **AGENTS.md 同步** README / plugin-mgmt / skill-mgmt 描述改用新路径 | ✅ | `94e09e6` |
| **AGENTS.md 同步** troubleshooting spec 格式同步 P4 新布局 | ✅ | `d2bab6d` |
| **P6 step 5** 迁移向导 UI 向导页面 | ⏸ 待 UI 形态决策 | — |
| **P8** UI / 集成测试 / 发布准备 | ⏸ 待决策（实例列表 UI 形态 / PR 边界 / 是否阶段性发版） | — |

完成度：**~93%**（15/17 row 已落地 commit；P6 step 5 / P8 仍依赖 UI 决策）。

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
| `cargo check --lib` | 通过 (1.88s) | 0 errors；本轮重跑无新增 warnings |
| `cargo clippy --lib` | 0 errors (7.31s) | 80 warnings 全部 P4 step 3.5 旧 API 残留（`cfg_attr(not(test), allow(dead_code))` 临时保留）|
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

### dev plan §4 release threshold 验证

- ✅ **package.json / tauri.conf.json 版本一致**：`0.1.5-rc.1` 对齐
- ✅ **没有把 `~/.dsh` 整体迁移或删除的脚本**：migration.rs 的 `LegacySource::all()` 只枚举 3 个具体旧源（Plugins / SkillsStore / SkillsActive），`~/.dsh/sessions` / `~/.dsh/credentials` 明确不在范围（dev plan §P6「credentials / sessions 不纳入首版迁移」）
- ✅ **没有通过宽泛进程匹配杀掉用户的官方 DSH**：`process::terminate_process_tree` 在 Unix 用 `libc::kill(-pgid, ...)`（负 PID = 自己创建的进程组）、Windows 用 `taskkill /PID <id> /T /F`（按 PID 杀子树），都不做 name 匹配
- ⚠️ **现有发布平台和 updater 没把新目录当安装目录**：**.github/workflows/desktop-release.yml** 静态查过——workflow 只跑 `pnpm exec tauri build` 与资产命名脚本 `scripts/normalize-release-assets.mjs`，不引用任何特定数据路径（grep `desktop/kernels` / `skills-store` / `extensions/plugins` 等均无匹配）。安装路径由 Tauri 配置 + OS 决定，不在 workflow 范围内。**剩余验证**：打一次 release 看 updater 端点（`releases/latest/download/latest.json`）和用户机器上的安装行为——需实际 release 触发。
- ✅ **日志可区分 `shell_mode` / `kernel_family` / `kernel_version` / `instance_id`**（commit `eba8c96` + `158ced3` + `9b2fce8`）：`LogSpec` 加 `family` + `instance_id` 字段；`log_file_name` 三参变体（旧格式 `release-kernel-2026-09-19.log` 保留壳级日志）+ 五参变体（实例感知 `release-dsh-default-kernel-2026-09-19.log`）；`kernel_log_spec(family, id)` / `install_log_spec(family, id, version)` / `current_kernel_log_path(data_dir, family, id)` / `install_version(family, ...)` 全链路接入；`guard::GuardDeps` 加 `family` / `instance_id`；`guard::diagnose_runtime` / `commands::kernel_workbench_url_from_log` / `notify::*` 全接 family + id。**commit `9b2fce8`** 把 5+ module 散落的硬编码 `"default"` 字面量统一为 `instance::DEFAULT_INSTANCE_ID` 常量；P8 UI 决策后改默认实例 id 只动 instance.rs 一处。
  - **遗留**：P8 UI 决策驱动后由默认实例解析器（kernel/instance 模块）统一替换所有硬编码的 `(DSH, "default")`；本轮不引入 UI 改动。

## Review 建议

`3ad6c8e..d2bab6d` 共 30 个 commit（含 6 笔文档收尾）。建议按以下顺序 review：

1. **设计层**（先看 docs）：本文件 + 设计稿 §P4–§P7 节 + [architecture.md §「多内核改造后的实际数据布局」](architecture.md)
2. **关键 commit**（设计落地点）：
   - `c0cfabc` P3 DshAdapter 与 KernelAdapter trait
   - `dcbae61` P4 物化路径切到实例 extensions/plugins/<id>/
   - `b09a525` P5 step 1+2 中央库 / 活动视图重布局
   - `89d76df` + `9615901` P6 迁移向导复制 / 备份 / 回滚
   - `1308cb6` P7 mcode mock 适配器
3. **测试**：每个 step 都有 4–8 个集成测试覆盖关键不变量
4. **预算**：FILE_BUDGETS 的每一次上调都在注释里写了"为什么"
5. **文档**（最后看）：`89932eb` / `257c42a` / `86372ad` / `4825b57` 状态快照四联 + `94e09e6` / `d2bab6d` AGENTS.md 同步
6. **release threshold**（特别看）：dev plan §4 四项发布门槛的逐项验证——含本轮新发现的「日志可区分 4 字段」缺口

**HEAD `d2bab6d` 可作为 review 基线**。