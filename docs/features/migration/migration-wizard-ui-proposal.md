# 迁移向导 UI 提案（P6 step 5）

> 4 条后端命令已可用：`migration_preview` / `migration_run` /
> `migration_rollback` / `migration_list`（commit `b24e68e` /
> `89d76df` / `9615901` / `5416bca`）。本文档为 step 5 前端实现提
> 建议方案 + 列出需要你拍板的具体决策点。

## 设计目标

让旧版（DSH home 旧布局）用户安全地把数据迁到新多实例布局：
- 凭据与会话**不**纳入首版迁移（dev plan §P6 保守默认）
- 旧源**永不被删除**（rollback 路径依赖）
- 默认 `ConflictPolicy::SkipIfNewer`（保留用户后来修改）

## 建议 UI 形态

**嵌入式向导页（侧栏新 panel）**——与现有 `SettingsPanel` /
`VersionsPanel` 等同构。

理由：
1. **复用现有导航模式**：`SideBar.vue` 的 `MENU` 数组（概览 /
   内核版本 / 插件 / 技能 / 设置）加一项「数据迁移」即可，零
   新增组件基础设施
2. **多步骤不打断用户**：迁移通常是一次性操作，向导页与设置页
   在同一深度，用户预期「点进去看完再走」
3. **避免新建窗口带来的状态同步问题**：独立窗口需要 IPC 广播
   「迁移完成」回主页，嵌入式页面切换即可
4. **可观察性更好**：迁移运行期间（10~30s）向 `ProgressOverlay`
   报告，与现有「安装内核」「装插件」共享同一进度 UI

不推荐独立窗口：与现有 UI 风格不一致，IPC 路径加复杂度。
不推荐 CLI：用户已经在 GUI 内管理，不需要切到终端。

## 向导 4 步骤（`el-steps`）

**Step 1：发现**
- 调 `migration_list()` 列出历史迁移
- 调 `migration_preview()` 拿 3 个 LegacySource（Plugins /
  SkillsStore / SkillsActive）的 file_count / total_bytes / 存在性
- 如果 `preview.has_migratable() === false` → 显「旧版数据未检测
  到，无需迁移」直接退出
- 如果有历史迁移 → 列出 `migration_id` / `started_at` / `status`
  给用户回查 / rollback 入口

**Step 2：选择**（多选 + 文本说明）
- 三个 LegacySource checkbox（默认全选）
- 凭据与会话**显式列出但不勾选**（dev plan §P6 保守默认）
  ——首次实现仅做只读显示 + 「首版暂不迁移」说明，避免误迁
- 每个 source 显示来源路径 + 旧目标路径 + 大小
- 默认 `ConflictPolicy::SkipIfNewer` radio（vs `BackupAndOverwrite`）
  + 说明文字

**Step 3：运行**
- 调 `migration_run({ sources, conflict_policy })`
- 实时进度走 `ProgressOverlay`（与装内核 / 装插件同 UI）
- 完成后展示 `MigrationReport`：
  - per-source: items / bytes / conflicts / skipped
  - backup root 路径（点击复制）
  - 「查看日志」链接

**Step 4：完成 / 回滚**
- 迁移成功：提示「默认 Shell 已指向新实例」+ 「查看新实例」链接
- 失败 / 用户改主意：「回滚」按钮 → `migration_rollback({ migration_id })`
  → 还原到迁移前状态 + 提示「旧源未删除，可手动清理」

## 关键决策点（待你拍板）

### 1. UI 形态

- ✅ **嵌入式向导页**（建议）
- ❓ 独立窗口（与现有 UI 不一致）
- ❓ 命令行（用户已在 GUI 内）

### 2. 凭据与会话处理

首版 migration 后端已**不**纳入 sessions / credentials。建议 UI：

- ✅ **显式列出 + 默认不勾选 + 说明文字**（建议，最保守）
- ❌ 隐藏在高级选项里（用户可能漏看）
- ❌ 默认勾选 + 二次确认（违反 dev plan §P6 保守默认）

### 3. 冲突策略 UI 表达

- ✅ **`ConflictPolicy::SkipIfNewer` 默认 radio + `BackupAndOverwrite` 备选**（建议）
- ❌ 永远 Skip（无法覆盖旧目标，限制用户）
- ❌ 永远 BackupAndOverwrite（违反 dev plan §P6 默认）

### 4. 侧栏 menu 顺序

- ✅ **设置 / 迁移 / 退出登录** 三项之间：迁移在设置之上（建议）
- ❌ 迁移在底部（用户找不到）
- ❌ 合并到设置页 tab（与设置面板职责混淆）

## 实现工作量估算

| 模块 | 行数 | 备注 |
|------|------|------|
| `ui/src/components/MigrationPanel.vue` | ~250 | 主体向导 UI |
| `ui/src/migration.js` | ~120 | invoke 封装 + 状态管理 |
| `ui/src/labels.js` 加 i18n key | ~30 | 8-10 个中文文案 |
| SideBar 加 menu 项 | ~5 | 一行数组项 |
| store.js 加 activePanel = 'migration' | ~3 | 路由注册 |

**总计 ~400 行前端 + 0 行后端**（后端 4 命令已可用）。

## 跨模块注意点

1. **`migration_id` 由后端生成**（`AutoYYYYMMDD-HHMMSS-<short>`），
   前端不构造——避免时间戳冲突
2. **进度走 `ProgressOverlay` 而非内嵌 loading**——保持进度 UI
   全局一致（install / plugin 都用同一组件）
3. **`MigrationReport.backup_root`** 路径需在 UI 里可点击复制——
   沿用 theme.css 的「路径块」样式（参考 plugins.js 的 backup 展示）

## 待确认

请你确认：

1. **UI 形态选哪个**（嵌入式 / 独立窗口 / CLI）
2. **凭据与会话处理**（列 + 默认不勾 / 列 + 默认勾 / 隐藏）
3. **冲突策略 UI 表达**（默认 SkipIfNewer + radio / 固定 / 备选）
4. **侧栏 menu 顺序**（迁移在设置之上 / 底部 / 合并到设置）

收到回复后即可开始实现——预计 3 笔 commit（panel 主文件 +
migration.js + SideBar 接入），不引入新依赖，复用现有 el-steps
/ ProgressOverlay / labels.js。