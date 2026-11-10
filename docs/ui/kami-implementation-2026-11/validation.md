# Kami 七页改版实施验证（2026-11-10）

- 分支：`feat/ui-kami-redesign`
- 实施提交：`8f8665e1`…`54990e55`（P1 基础 → P2 页头/概览/内核/设置 → P3/P4 插件/技能/用量/记忆 → 响应式/用量口径 → 文档与 token 判据 → 行高/列宽收口）
- 平台：Intel macOS 26.7.2（x86_64）；目标窗口 1040×748
- 结论：**P1–P4 代码与静态门禁完成；实机截图与行为矩阵未执行（not_run）**，不得据此宣称真实桌面验收通过。

## 1. 已执行检查

| 检查 | 状态 | 命令 / 证据 |
| --- | --- | --- |
| 设计合同 token 对照（两主题、字面量、geometry 不在暗色） | pass | `node --test ui/test/kamiTheme.test.js` |
| UI 定向/全量单测 | pass（822/822） | `npm run test:ui` |
| UI 构建 | pass | `npm run build:ui` |
| 不变量门禁 | pass | `npm run check:invariants` |
| 重复代码门禁 | pass（7/8 处允许） | `npm run check:duplicates` |
| UI 产物体积 | pass | `npm run check:ui-bundle` |
| 设计合同完整性脚本 | pass | `node docs/ui/kami-implementation-2026-11/verify-spec.mjs` |

## 2. 未执行（not_run）

以下项目需要真实桌面壳、GUI 自动化或 Windows 机器，本轮未跑；不伪造截图或标记通过：

- 浅色 / 暗色 / 窄容器 / 收起侧栏的 14+14 张真实截图；
- `acceptance.md` §5 的几何测量（main 无横溢出、520/719/720/767/768/899/900 交界、125%/200% 缩放）；
- §6 行为矩阵 B01–B16（启停、预检、安装、记忆授权、套餐失败重试等）；
- Windows 实机（ClearType、玻璃退实色、高对比模式）；
- 键盘 / 读屏的人工走查与焦点顺序；
- e2e 截图 / 几何场景（本轮未新增 spec，也未从 launcher 跑）。

## 3. 实施范围（按合同）

- **P1**：补齐 `tokens.json` 两主题全部 token；控件 / 行 / 导航 6px、独立浮层 8px；主内容 20/24/24、双列缝 24、块缝 16；页头 48 / 图标 22；控件高 30 / `--control-border`；焦点环 `--focus-ring`；玻璃只给标题栏 / 侧栏托盘 / 菜单；图表色 `--chart-1..8`；删除页头缎带与暗色径向光。
- **P2**：七页页头只留标题 + 可聚焦 ⓘ；概览「官网网页版 + FamilySwitch」移到页头下动作排，栅格 24；内核版本组缝 24、磁盘四块 2×2 padding 12；设置两列 / 端口 112/136 / 外观 min-height 76 / MiniMax 粉边轻填。
- **P3**：插件内置 + 自装合并成一张五列表，key `builtin:` / `community:`，内置徽标，未适配「待适配」，自装 DeepSeek 只读，安装区在列表下方；技能改五列已安装表，每个技能分别控制两个内核。
- **P4**：套餐改整宽横向行（厂商 180 / 内容 1fr / 时间 210，条高 6，明确「剩余 / 已用」）；KPI + 热力图 1.4/1；记忆启用行显示 DeepSeek、tab 34/底线 2px、正文 1.2/1、行 min-height 84、窄内容退整页流。

## 4. 已知偏差与说明

- **页面副标题整体收进 ⓘ**：合同 §3.1 的明文要求覆盖了概念图与旧 ui/AGENTS 的「页头主副标题同行」；`.page-desc` 全局删除。
- **自装插件 DeepSeek 仍是只读「已安装」**：合同 §7.2 明确本轮不造通用启停 API；取消勾选绝不会调用卸载。
- **e2e 几何 / 截图场景未新增**：本轮没有可用 GUI 自动化环境，加一份未运行过的 spec 只会变成假门禁；已在 §2 标 not_run，下一轮实机验收时补齐。
- **暗色沿用既有「夜航日志」**：本轮没有新暗色概念图，只按 `tokens.json` 适配；真实暗色截图未拍。
- 玻璃在无 `backdrop-filter`、`prefers-reduced-transparency`、`forced-colors` 下退回实色；未改 Tauri 透明 / Windows ClearType 配置。
- 未实现合同 §2.3 的功能扩展（技能目录/搜索、跨模块活动流、记忆统一导出等）。

## 5. 下一轮实机验收清单

1. 按 `acceptance.md` §4 拍 14+14 张截图并标 pass/fail/not_run；
2. 跑 §5 几何测量与 §6 B01–B16；
3. Windows 125%/200% 缩放与高对比；
4. 若实现 e2e 场景，用 `npm run e2e -- --keep --spec e2e/specs/<新 spec>.mjs` 从 launcher 跑并记录证据路径。
