# 多内核评审整改与 UI 重构交接

交接日期：2026-11-07。本文记录 2026-11-07 那一轮「评审 → 整改 → UI 重构」的快照：27 条评审问题怎么关的、
内核维度那条 UI 决定怎么落的，以及**还没验的**与**还欠着的**。

当时功能是实验入口（开发模式默认显示，常规构建需 `VITE_MCODE_EXPERIMENTAL=1`），验收未完成；现行显示规则见接入计划。完整验收口径与失败 / 成功证据以
[MiniMax 接入计划](../architecture/multi-kernel/minimax-webui-integration-plan.md)为准；功能实现本身的过程记录见
[开发交接](../architecture/multi-kernel/minimax-webui-handoff-2026-11-07.md)。

## 目标

- 把多内核这轮评审发现的 27 条问题（6 必须修 / 13 应该修 / 8 可选优化）真正关掉，而不是只改说法。
- 把「内核维度」从页面级降成控件级：顶部那条页签条让人以为「下面这一页换内容」，实际是「整个壳换内核」，
  而用量分析这类公共页的数据与内核无关，跟着翻等于把公共数据画成另一个内核的数据。

## 落在仓库里的三个提交

| 提交 | 内容 |
| --- | --- |
| `a0e15ef8` | 修掉评审发现的 27 条问题（30 文件，+1766 / −527）。**逐条清单在提交信息里**，那是这轮评审的完整记录。 |
| `dadf90ad` | 收口 6 必须修与 13 应该修里剩下的缺口（4 文件，+175 / −22）；同提交把 UI 包体上限翻倍（`JS_BUDGET` 772,000 → 1,544,000）。 |
| `4cf611f3` | 内核维度从页面级降成控件级（21 文件，+602 / −234）。 |

中间夹着两次别的会话的提交（`385a0181` / `de8802d9` 文档、`b04ef33d` Rust 调试信息配置），与这轮无关。

## UI 重构后的形状

- 顶部页签条整条删掉（`ui/src/kernel/KernelTabs.vue` 连文件一起没了），换成分段控件
  `ui/src/kernel/FamilySwitch.vue`：药丸按钮、无下划线、方向键可切、焦点跟着走。
- 切换器只出现在**与内核指向性绑定**的三个页的页头：概览、内核版本、记忆。
- 用量分析 / 插件 / 技能 / 设置是公共页：不读内核状态，也不出现切换器。
- 内核选择每页各一份（`instance.js` 的 `pageFamilies`，`FamilySwitch` 是受控组件）：在概览切到 MiniMax 不影响记忆页和内核版本页；落盘仍只有一个 `kernel-selection.json` 值，只当启动时给三页播的种子。
- 逐屏决定与被否掉的方案在 [panel-decisions](../../ui/panel-decisions.md) 的「内核维度从页面级降成控件级」
  一节；规则沉淀在 [ui/AGENTS.md](../../ui/AGENTS.md) 的「内核维度是控件的属性，不是页面的属性」。

## 验证状态（`4cf611f3` 提交时的读数）

| 命令 | 结果 |
| --- | --- |
| `npm run test:ui` | 704 条全绿 |
| `npm run build:ui` | 通过 |
| `npm run check:ui-bundle` | JS 764,647 / 1,544,000；CSS 240,320 / 268,000 |
| `npm run check:invariants` | 通过 |
| `npm run check:duplicates` | 7 处（允许 8 处），全是既有的 |
| `cargo check --locked` | 通过 |
| `cargo test --locked --lib kernel_selection` | 1 条通过 |

全量 `cargo test --locked --lib` 在这轮之前是 1495 通过 / 2 失败。那两条是**本机环境**：`HKCU\Environment\Path`
里留着十几条陈旧的探针临时目录（`mcode-unicode-length-*`、`dsh-xlink-minimax-probe/runs/*` 等），`cmd.exe`
把它们打成 `????`。`shell/env.rs` 与 `shell/process.rs` 这轮没被碰过。

## 还没验的（接手后先看这一节）

- **实机证据为零**：这轮所有 UI 改动都没在壳里看过渲染。分段控件在页头的落位、插件页两块并排的观感、
  设置页三行端口在 label 占 132px 时的余量（靠 `.settings-port` 收缩到 96px 地板兜底）都只按源码推的；
  `npm run build:ui` 只证明能编。
- 评审第 1 / 2 / 4 条（端口判据、退出三态、授权弹窗失败放行）只有单测与静态论证，**没有实机复验**。
- `src-tauri/src/kernel/mcode_tree.rs` 是 Unix-only，本机编不了，只能靠 macOS CI 的 job。
- 第 15 条（MiniMax 选 `system` 时跟随操作系统）依赖上游 `@minimax-webui/minimax-code@0.1.7` 的
  `webui-theme` 取值，**等上游**。
- 第 26 条：`pnpm-lock.yaml` 里还留着 `file:` 路径，要重跑一次 pnpm 才落定。

## 还欠着的

- **本机注册表 PATH 的陈旧条目**：清理能消掉那两条红测试，但要用户授权（按 AGENTS：清理前先核实范围与占用）。
- **MiniMax 那一路的数据仍然半截**：用量页的 MiniMax 用量来源没有可靠数据源（现在降级成卡里一行说明）；
  控制塔三张卡（系统健康 / 需要关注 / 最近操作）只有 DSH 的数据源，`v-if="!isMcode"` 等数据补齐后删掉。
- **公共页现在彻底不读内核状态**：插件页底部那份「已安装插件」是 DSH 的，文案已直说 DSH。若以后 MiniMax 的
  插件清单也要进公共页，归属要重新设计。

## 接手前值得先知道的两件事

- `kernel-selection.json` 的模块文档第一句就是 "The management view selection does not change a running
  instance."——内核选择只改管理界面，不改进程；两个工作台的启停命令本来就分开（`start_mcode` / `stop_mcode`
  对 `start_kernel` / `stop_kernel`）。想在 Rust 侧动这件事之前先读这一句。
- 公共页的数据源与内核无关是**事实**（`SubscriptionUsageCard.vue` 根本不读 `family`）。别再给它们加内核分支，
  那不是「多内核支持」，是把公共数据画成另一个内核的数据。
