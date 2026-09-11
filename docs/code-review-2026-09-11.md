# dsh-xlink 代码审查报告（2026-09-11）

审查对象：`dsh-xlink@0.1.2-rc.20`（`main`，HEAD `70f173c`；范围 `233c66d..70f173c`，2026-09-10 ~ 09-11 的 36 个提交，73 文件，+9272 / −1191）。
审查日期：2026-09-11。

上一轮（`docs/code-review-2026-09-10.md`，rc.18 → rc.19 的 22 个提交 / 121 条）修掉的是"点"。本轮是那次修复**之后**的独立复核，重点是：① 同类残留；② 修复自身新写的代码；③ 台账里标 ✅ 但实际未生效的条目。

## 审查方法与已执行的验证

| 验证 | 结果 |
| --- | --- |
| `cargo check --all-targets` / `clippy --all-targets` / `fmt --check` | 全绿 |
| `cargo test` | 272 passed / 0 failed / 1 ignored |
| `node --test ui/test/*.js scripts/*.test.mjs` | 34 passed |
| `check:invariants` / `build:ui` | 通过；JS 424 KB、CSS 137 KB（预算 700 KB / 180 KB） |
| `process.rs::job_object`、`tray.rs` 以 `x86_64-pc-windows-msvc` 交叉编译 | 通过（这两个文件在 macOS CI 里从不编译，是门禁盲区） |
| `/tmp` crate 副本上的**探针测试**（不改本仓库） | P0-1、P0-7 均复现（见明细） |

## 为什么上一轮之后仍有这么多问题

1. **按点修，没按类扫**：`load_store(` 在 `plugins.rs` 仍有 7 处（`:2403`/`:2674`/`:3226` 直接喂写/删）；收版本号的 4 个命令只加了 2 道闸；`entry_is_owned` 三条路径只修了 uninstall；`:loading="globalBusy"` 修了 OverviewPanel 没修 PluginsPanel。
2. **修复自身的新代码，新分支零测试**：`rollback_files` 完全不在任何测试路径上（`apply` 的失败回滚无人测）；`sweep_unrecorded_store_dirs` / `ensure_wiring_filtered` 虽有经由 `reconcile_store` / `ensure_wiring` 的间接覆盖，但那些用例走的都是"清单存在且正常"的路径——**本轮出问题的"清单缺失/损坏"分支没有任何用例**。
3. **"已修"的判据被稀释**：台账 121/122 条标 ✅，其中 P1-15（写路径只改一半）、P2-29（`stop()` 永远 `Ok`，"如实上报"是死代码）经本轮复核确认为名义已修。P2-35 则是真缺陷（见下），审查期间被并发会话修掉。
4. **二分语义只做了"损坏"半边**：`StateRead` 只拦 `Corrupt`，`Missing` 被当成正常空值；而应用自己的错误文案就在指导用户"删除该文件后重试"——提示语与破坏路径是同一个动作（P0-3/P0-4）。
5. **门禁盲区就是漏点**：CI 只有 macOS；UI 的 loading/disabled 绑定没有静态检查；`check-signing-keys.mjs` 的入口守卫在 Windows 上恒不成立（P1-5）。
6. **速率**：上一批 22 个提交 / 137 分钟 / +7841 行 / 121 条，平均 68 秒一条，不可能有第二遍同类扫描与"审自己的修复"。

补充一条本轮自己的教训：审查期间工作区**有并发写入者**（另一个会话在同一 checkout 上工作，
`ui/` 与 `ui/dist` 都被它改动）。这直接污染过一次判定——第一次复核 P1-2 时读的是工作区快照
（当时它已被对方改好），于是写下了"误报"的结论；把审查对象钉回提交 `70f173c` 后可见该缺陷真实
存在（详见下文）。结论：**判定必须钉住被审的那个提交**（`git show 70f173c:<file>`），并且复核
时要先看 `git log` 有没有并发提交动过同一个文件。

因此本轮除了修缺陷，还要补三类**门禁**：Windows 编译覆盖（P2-16）、UI 模板标识符检查（P2-19）、`check-invariants` 的补丁路径判据（P2-18）。

## 修复进度

| 编号 | 问题 | 状态 |
| --- | --- | --- |
| P0-1 | 补丁回滚删除"应用前已存在"的文件 | ✅ 已修（探针复现 + 反证命中） |
| P0-2 | `plugin_check_updates` 写路径用容错读取，覆盖 store.json | ✅ 已修 |
| P0-3 | plugins：store.json 缺失被当空清单 → 中央库目录全量清理 | ✅ 已修（反证命中） |
| P0-4 | skills：store.json 缺失被当空清单 → 活动根链接全量删除 | ✅ 已修（反证命中） |
| P0-5 | `ensure_wiring_filtered` 容错读取 → 清内核物化目录与 profile 依赖 | ✅ 已修 |
| P0-6 | skills `replace_owned` 删除用户改过的 copy 副本 | ✅ 已修（反证命中） |
| P0-7 | 补丁"无备份记录"永久不可撤销，且提示的重装建议无效 | ✅ 已修 |
| P1-1 | 托管 Node 解包根目录写死 `darwin-x64` | ✅ 已修 |
| P1-2 | 设置页 `globalBusy` 未导入 → `:disabled` 死绑定 | ✅ 已修（由并发会话 `dbce6a5` 修，本轮不重复） |
| P1-3 | Windows 收起提示渲染在已隐藏的窗口里（README/architecture 已承诺） | ✅ 已修 |
| P1-4 | 托盘「退出」在无内核时仍弹"工作台仍在运行" | ✅ 已修 |
| P1-5 | `check-signing-keys.mjs` 在 Windows 上静默空转（误绿） | ✅ 已修 |
| P1-6 | Node 回滚诊断在删除目录之后探测 → 诊断恒为空 | ✅ 已修 |
| P1-7 | `remove_version` / `activate_version` 缺版本号闸（纵深） | ✅ 已修 |
| P2-1 | `workbench_pid` 丢掉端口/cwd 关联 → 误杀另一实例内核 | ✅ 已修（反证命中） |
| P2-2 | `kernel::stop` 永远返回 `Ok`，"停止失败如实上报"是死代码 | ✅ 已修 |
| P2-3 | `SpawnFailed` 仍跑 pnpm 恢复接线，事故文案按插件口径 | ✅ 已修 |
| P2-4 | "进程起来了但立刻退出"仍进安全模式（环境故障记成插件故障） | ✅ 已修（含 `env` cause） |
| P2-5 | guard 归因在 Windows 匹配不到反斜杠路径 | ✅ 已修 |
| P2-6 | SRI：多摘要（空格分隔）硬失败；缺摘要时的策略 | ✅ 已修（缺摘要仍为跳过，见明细） |
| P2-7 | `save_settings` 清空 `node_path`；文案指向不存在的设置项 | ✅ 已修 |
| P2-8 | `detect_node` 不回写缓存 → 陈旧 `ok:false` 挡死启动 | ✅ 已修 |
| P2-9 | Windows 每 2.5s 派生一次 PowerShell 做进程身份校验 | ✅ 已修（3 秒缓存） |
| P2-10 | `PluginsPanel` 模式徽章用 `:loading="globalBusy"`（P2-42 同款） | ✅ 已修 |
| P2-11 | 进度浮层 z-index 3000 压住 ElMessage / ElMessageBox | ✅ 已修（在 `notify.js` 显式指定 zIndex，未改 theme.css） |
| P2-12 | 技能自动检查失败不退避（注释显示为有意取舍，缺退避） | ✅ 已修 |
| P2-13 | 发布：资产清理/断言排在 `draft=false` 之后，失败即锁死 | ✅ 已修 |
| P2-14 | 发布：版本单调性门 fail-open（`curl … \|\| true`） | ✅ 已修 |
| P2-15 | 发布：annotated tag 会让 publish 在跑满全流程后必挂 | ✅ 已修 |
| P2-16 | CI 无 Windows job，Windows 专属编译错误只在打 tag 时暴露 | ✅ 已修 |
| P2-17 | `open_log_window` 在 async 命令里阻塞 `recv_timeout(20s)` | ✅ 已修 |
| P2-18 | `check-invariants.mjs` 对补丁 `from` 缺穿越校验、`to` 放行 UNC | ✅ 已修 |
| P2-19 | 缺"UI 模板未定义标识符"门禁 | ✅ 已修（新增 `scripts/check-ui-bindings.mjs`） |
| P3-* | 轻微项（见文末"轻微与建议"） | 🔄 进行中（已修 `prune_old_logs` 当天日志语义） |

### 关于 P1-2 的最终判定（两次更正后）

**结论：P1-2 是真缺陷，存在于审查对象 `70f173c`；由并发会话在审查期间修复并提交为 `dbce6a5`，
本轮不重复修。** 状态记录如下，避免以后被误读：

| 时间 | 提交 | `ui/src/components/SettingsPanel.vue:8` | 说明 |
| --- | --- | --- | --- |
| 审查对象 | `70f173c` | `import { isLoading, withLoading } …` | 模板用了 `globalBusy`，绑定落空 → `:disabled` 恒为 `undefined` |
| 22:07:27 | `dbce6a5`（July-X） | `import { globalBusy, isLoading, withLoading } …` | 并发会话修掉了它 |
| 我的复核 | `dbce6a5` 之后 | 同上（已修） | 我读到的是"已修"状态，因而错误地写成"误报" |

两次更正的过程本身就是方法教训，两条都已落进验证规范：

1. **判定要钉住被审提交**：用 `git show 70f173c:<file>`，并在复核前先看 `git log -- <file>`，
   确认没有并发提交动过它；只读工作区快照会得出相反结论。
2. **"模板标识符未定义"必须跑编译器并传 `bindingMetadata`**：不传 `compileScript` 的
   `bindingMetadata` 时，`compileTemplate` 会把**所有**标识符渲染成 `_ctx.*`，看上去"处处未定义"，
   据此判断必然误报。正确做法已落进 `scripts/check-ui-bindings.mjs`。

顺带一提：`scripts/check-ui-bindings.mjs` 的反证用的就是这个文件——去掉那行导入，门禁立刻报
`模板引用了未定义的标识符 globalBusy`，说明这类缺陷是可以被自动拦住的。

---

## P0（数据破坏）

### P0-1 补丁回滚删掉"应用前就存在"的文件 —— 已用探针复现

- 位置：`src-tauri/src/patches.rs:951-959`（`already_patched` 分支返回 `had_original = true`、`backup_rel = None`）+ `:998-1002`（`rollback_files` 对 `backup_rel == None` 无条件 `remove_file`）。
- 触发：清单里有 ≥2 个文件，第一个在应用前**已经是补丁内容**（手工打过 / 上次应用的记录丢失），靠后的文件在执行阶段失败（权限、父路径不是目录……）。
- 后果：回滚把那个从未被本次写入的文件删掉，而错误文案写"本次已写入的内核文件已回到应用前的状态"。`rollback_files` 全仓无测试。
- 复现：`/tmp` crate 副本上新增探针测试（双文件补丁 + 第二个文件父路径是普通文件），`apply()` 返回错误后断言第一个文件仍存在 → **失败**：`回滚删除了应用前就存在的文件 …/node_modules/x/a.js`。
- 修法：`rollback_files` 的 `None` 分支先看 `had_original`；为真则不删、把"未还原"记入残留说明。补反证测试。

### P0-2 `plugin_check_updates` 用容错读取做读-改-写

- 位置：`src-tauri/src/plugins.rs:2672-2683`（`let mut store = load_store(data_dir);` → `save_store_unlocked`）。
- 对照：技能侧同一函数已改严格读取（`src-tauri/src/skills.rs:1739`）。
- 触发：打开插件面板 / 点「检查更新」时 store.json 损坏或被短暂锁住。
- 后果：`Store::default()` 被原子写回，已装插件记录永久丢失；下一次启动的清扫路径再按空清单删目录（P0-3）。
- 修法：改 `load_store_checked`，`Corrupt` 时不写库并把错误上报给该次检查的结果。

### P0-3 / P0-4 `Missing` 被当成"空清单"→ 清扫删光用户数据

- 位置：`plugins.rs:592-600`（`load_store_checked` 把 `Missing` 归为 `Ok(Store::default())`）→ `:1313-1318`（`reconcile_store` 调用清扫）→ `:1334-1367`（`sweep_unrecorded_store_dirs` 删除"有 `.dsh-id` 标记、无记录"的目录）；`skills.rs:1788`（`reconcile_home` 同样 `Missing → 空 store`）→ `:1845-1892`（活动根里指向中央库的链接与非链接孤儿被 `remove_target`）。
- 触发：store.json **不存在**（用户按应用提示"修复或删除该文件后重试"删掉了损坏的清单；或首次安装中途崩溃），下次启动 `lib.rs` 的 `setup` 立即执行清扫。
- 后果：`<dsh_home>/plugins/<id>`（含 node_modules）被物理删除；`~/.dsh/skills` 里全部技能链接被删，运行中的内核通过 watcher 立刻丢技能。`skills.rs:1782` 的注释写的正是"绝不能当成没有任何已装技能"，实现却对 `Missing` 这么做了。
- 修法：清扫只在 `StateRead::Loaded` 时执行；`Missing` 且存在带标记目录/指向中央库的链接时保留现场并写 warning（附下一步：修复或恢复 store.json）。

### P0-5 `ensure_wiring_filtered` 用容错读取

- 位置：`plugins.rs:2403`（`let store = load_store(data_dir);`）→ `:2448` `sweep_kernel_orphans`（删内核物化目录）→ `:2457` `wire_manifest`（`deps.retain` 清退 profile 全部托管依赖）→ 重跑 pnpm。
- 触发：store.json 损坏时启动工作台 / 切换内核 / 同步插件。
- 后果：正是 `load_store_checked` 文档注释点名的那场灾难，也是台账 P1-15 声称"写路径与清扫路径全部改严格读取"时漏掉的一条。
- 修法：改严格读取；`Corrupt` 时中止接线且绝不改写 profile，错误并入 `failures`。

### P0-6 技能 `replace_owned` 会删掉用户改过的副本

- 位置：`skills.rs:1340-1347`（非本商店所有 + `replace_owned` → `remove_target` 后重新 copy）；调用点 `:1827`（reconcile，每次启动）、`:1532`（update）。
- 触发：copy 模式（Windows 未开开发者模式时的必然回退）下用户编辑了技能内容 → 指纹不匹配 → 下次启动或点「更新」即整目录删除并覆盖。
- 后果：用户本地改动无备份、无提示地消失；与 `skills.rs:1293` 的承诺"落地后被改写过的条目一律不动"直接矛盾。P0-3 的修复只覆盖了 uninstall 一条路径（现有测试也只有 `modified_copy_entry_survives_uninstall`）。
- 修法：非本商店所有时保留现场（改名 `<name>.user-<ts>` 备份 + warning），`replace_owned` 只对本商店所有的条目生效；补 reconcile / update 两条路径的测试。

### P0-7 "无备份记录"永久不可撤销，且提示的重装建议无效 —— 已用真实清单复现

- 位置：`patches.rs:1084-1156`（`revert_one` 的 `None` 分支 → `handle_missing_backup`）+ `:1180-1223`；无任何"清除记录"入口（`commands.rs` 只有 `patch_status` / `patch_apply` / `patch_revert`）。
- 复现（`/tmp` crate 副本，用**真实内置清单** `dsh-session-perf`，适用范围覆盖当前内核）：目标已是补丁内容时 `apply()` 生成 `backup_rel=None / had_original=true / original_sha256=None` 的记录 →
  1. 撤销#1：「原文件备份已丢失…请重新安装该内核版本后重试」；
  2. 照做（重装 = 文件回上游原文）→ 撤销#2：「目标文件已被修改（内容与补丁记录不一致），请检查后手动处理」；
  3. 重新应用：「已应用到内核版本 0.1.5-rc.2，请先撤销后再重新应用」。
- 后果：记录永久卡在 `state.json`（UI 显示"已应用"却撤不掉、也重打不了），而这条链的起点正是应用自己提示的"删除损坏的 state.json"。
- 修法：`had_original && original_sha256.is_none()` 且目标内容既非补丁内容也非"未知"时，允许一次显式的"清除记录（保留当前文件）"动作；`patch_status` 为该形态给出可操作提示。补测试。

## P1（功能失效 / 误导）

### P1-1 托管 Node 的平台适配只做了一半

- 位置：`src-tauri/src/node_install.rs:157`（非 Windows 的 `ROOT = "node-v24.20.0-darwin-x64"`）、`:230`（Windows `win-x64`）。
- 触发：Apple Silicon（`tauri dev` 或源码构建）或 Linux 上点「帮我安装」——`artifact_for_platform` 本轮新增了 `darwin-arm64` / `linux-*`，会先下 51 MB、校验 SHA-256 通过，再在 `strip_root` 报「归档根目录必须是 …-darwin-x64/」。
- 修法：`ROOT` 由 slug 推导（`node-v{VERSION}-{slug}`），补按平台断言 ROOT 的单测。

### P1-2 设置页 `globalBusy` 未导入 → `:disabled` 死绑定

- 位置：`ui/src/components/SettingsPanel.vue:8`（只导入 `isLoading, withLoading`）vs `:122`、`:131`（模板用 `globalBusy`）。
- 后果：`<script setup>` 里找不到的标识符回落到渲染上下文 → 恒 `undefined` → 按钮永不 disabled；
  互斥租约期间点「保存设置」/「检测 Node.js」，`withExclusive` 返回 `undefined`，任务体不执行、
  无任何 toast，2.5s 轮询再把输入框回滚。
- 状态：**已由并发会话在 `dbce6a5` 修复**（补上导入），本轮只补门禁（P2-19）防止复发。

### P1-3 Windows「已收起到通知区域」提示用户看不到

- 位置：`src-tauri/src/tray.rs:104-110`（先 `hide_to_tray` 再 `notify_hidden_once`）、`commands.rs:1186-1187` → `ui/src/App.vue:176`（页内 `ElMessage`）。
- 后果：提示画在刚被隐藏的 webview 里；仓库无 notification 插件，没有任何系统级通道。而 `README.md` 与 `docs/architecture.md` 都承诺了这次提示。
- 修法：改为窗口重新可见时补发一次性提示（Rust 在 `show_main_shell` 后 emit，前端在可见时 toast），或引入系统通知；同时校正文档口径。

### P1-4 托盘「退出」在什么都没运行时仍说"工作台仍在运行"

- 位置：`tray.rs:141-147`（无条件 emit）→ `ui/src/App.vue:107-116`（`else` 分支无条件假设内核在跑）。
- 修法：`onQuitConfirmRequest` 区分"内核/对话都没有"的情形，给中性的确认文案。

### P1-5 `check-signing-keys.mjs` 在 Windows 上静默空转（误绿）

- 位置：`scripts/check-signing-keys.mjs:190` 的 `if (import.meta.url === \`file://${process.argv[1]}\`)`；仓库另两个脚本用的是 `pathToFileURL(resolve(process.argv[1])).href`。
- 证据：Windows 上拼出 `file://D:\a\…` ≠ `file:///D:/a/…` → `main()` 不执行、exit 0；生产 run 34565441709 的 macOS job 有"✓ 签名密钥成对"，Windows job 同一步骤零输出却 success。
- 修法：改用跨平台写法；未命中入口时显式报错。

### P1-6 Node 回滚诊断在删除目录之后才探测

- 位置：`node_install.rs:480-486`（先 `remove_dir_all(&version_dir)`，再对就在该目录里的 `exe` 调 `probe_failure_detail`）。
- 后果：探测必然失败 → 用户永远只看到「实际输出：未返回任何诊断输出」，与事实相反，正是本轮声称修掉的"掩盖真实原因"。
- 修法：先取诊断再删目录（或把 exe 临时保留到探测之后）。

### P1-7 `remove_version` / `activate_version` 缺版本号闸

- 位置：`commands.rs:556`、`:576` vs 已有闸的 `:491`、`:1788`；`kernel::uninstall`（`kernel.rs:482-493`）直接 `remove_dir_all(kernels_dir.join(version))`。
- 后果：`..` 会删掉整个 data dir。当前**不可达**（只有 `windows:["main"]` 本地面板能调、UI 只传目录名），属纵深与一致性缺陷；上一轮台账 P1-2 声称"两个命令边界"，实际有 4 个命令收版本号。
- 修法：两个入口补同一判据，并在 `kernel_dir` 处断言父目录为 `kernels/`。

## P2（中等）

### P2-1 `workbench_pid` 丢掉端口/cwd 关联

`kernel.rs:1596-1601` 调 `pid_is_kernel(pid, None)`，跳过第 2 层（`--port` 相等）与第 3 层（OS 反查监听者==pid），而 `kernel.rs:1522-1528` 的文档写明第 3 层是"唯一可信的'这还是不是同一个内核'判据"。触发：内核自行退出时 `clear_pid` 不会被调用，pid 文件残留；壳重启后该 pid 被复用给**另一个 dsh 内核**（dev/release 双开、另一 data dir、CLI 直跑）→ `status` 报"运行中"→「关闭工作台」对该内核整个进程组 SIGTERM/SIGKILL。修法：pid 文件记 `pid + port`（或 `kernel_dir`），校验带上；或像 `reap_orphans` 那样加 cwd 活体校验。

### P2-2 `kernel::stop` 永远返回 `Ok`

`kernel.rs:1287-1326` 两个分支都只 `Ok(())`，`commands.rs:749-771` 的 `map_err` 是死代码（台账 P2-29 的"停止失败仍如实上报"未实现）。修法：`wait` 超时/进程仍在即返回带 pid 与下一步的错误。

### P2-3 `SpawnFailed` 仍跑 pnpm 恢复接线

`guard.rs:702-740` 无条件 `restore_profile_manifest`（`plugins.rs:2575-2576` 会跑 `run_profile_install`），`cause` 落 `unknown`，可操作信息（换/释放端口）只埋在 `IncidentModal.vue:159` 的折叠区。修法：`SpawnFailed` 跳过 restore，把 `verdict.reason()` 写进 message/hint。

### P2-4 "进程起来了但立刻退出"仍进安全模式

`guard.rs:203-216`（`Exited → Failed`）+ `:567`（`kernel_started` 只看 `SpawnFailed`）+ `:632-700`（停用全部插件并写 `cause:"plugin"`）。EADDRINUSE（预检只测 IPv4 回环，`kernel.rs:1102-1122`）等环境故障归因不到插件，重试若恰好成功就把责任记到插件头上。修法：识别环境类退出并跳过插件阶梯。

### P2-5 guard 归因在 Windows 匹配不到反斜杠路径

`guard.rs:287-323` 只找 `plugins/<id>`、`node_modules/<name>`，无 `\\`→`/` 归一化（`kernel.rs:1492`、`skills.rs:1254` 都做了）。Windows 上只剩"带引号包名"一条 → 定向停用退化成"未归因"或全量安全模式。

### P2-6 SRI 语义

`releases.rs:55-57`：`integrity` 缺失即 `Ok(None)`，调用方只打一行"跳过内容校验"（`plugins.rs:1434`、`skills.rs:1015`），而 `dist.shasum` 根本没解析；`:58-64`：多摘要（空格分隔）整串进 base64 → 硬失败。修法：缺 integrity 时回退 `shasum`，两者皆无则拒绝（或显式告警）；按 SRI 规范遍历 token 取最强算法。

### P2-7 `save_settings` 清空 `node_path`

`commands.rs:234-256` 用 `#[serde(default)]` 的 `Settings` 整体覆盖，UI 只发 `{port, profile}`（`ui/src/store.js:481-484`）→ 手改 settings.json 的 `node_path` 被静默清空；而 `node_install.rs:489` 的新文案让用户"在「设置」里指定 Node 路径"，设置页并没有这个字段。修法：后端与 `previous` 合并缺失字段（或面板补该输入项），文案与实际入口对齐。

### P2-8 node 缓存陈旧

`commands.rs:173-191` 的缓存键只有 `settings.node_path`，作废点只有 `install_node:226`、`install_kernel:510`；`detect_node:193-207` 不回写，`start_kernel:700-703` 命中陈旧 `ok:false` 直接拒绝 → 用户看到"已检测到 node"与"未检测到 Node.js"并存。

### P2-9 Windows 每 2.5s 派生 PowerShell

`kernel.rs:1470-1488`（`Get-CimInstance`）+ `:1596-1610` + `App.vue:167`（2.5s 轮询）→ 约 24 次/分钟的子进程与 CIM 查询，常驻托盘后是持续负载。修法：换 `QueryFullProcessImageName` 或对同一 pid 缓存几秒。

### P2-10 `PluginsPanel` 模式徽章 `:loading="globalBusy"`

`ui/src/components/PluginsPanel.vue:244`：任何全局 IO 都让每行徽章转圈并禁用——同批在 `OverviewPanel.vue:38-40` 作为 P2-42 修掉的同款反模式。修法：`withLoading('pluginMode:' + row.id, …)`。

### P2-11 进度浮层 z-index 压住 Element Plus 弹层

`ui/src/theme.css` 的 `.progress-overlay { z-index: 3000 }` 高于 ElMessage/ElMessageBox 的自增基线（2000+）→ 长任务中托盘「退出」的确认框不可见/不可点，而任务未失败时浮层没有关闭按钮。

### P2-12 技能自动检查失败不退避

`ui/src/skills.js:74-90`：逐包失败时不推进 TTL、且不区分手动/自动都弹 8s 提示 → 有持续失败包的用户每次切页/回焦/长任务结束都重跑全量探测。与 `plugins.js:229-243` 的口径（自动路径静默、成功才推进 TTL）不一致。

### P2-13 发布：资产清理/断言排在 `draft=false` 之后

`.github/workflows/desktop-release.yml:471-483`（先转正）在 `:485-585`（删计划外资产 + 计数 + 逐项比对）之前。失败即：已公开的 Release 被就地删资产，且重跑会被"already published"与单调性门拦住 → 无法自愈。修法：把清理与比对整体上移到转正之前（草稿态清理安全且可重入）。

### P2-14 发布：单调性门 fail-open

`:96-102` 用 `curl … || true`，`published` 为空（超时/5xx/CDN 抖动/JSON 无 version）即打印"跳过版本单调性检查"继续发布——恰在异常时丢掉唯一防止更新通道回退的保护。修法：用 `--write-out '%{http_code}'` 区分 404（首发）与其它失败，非 404 重试后 `exit 1`。

### P2-15 发布：annotated tag 让 publish 必挂

`:382-389` 用 `gh api git/ref/tags/<tag> --jq .object.sha`（annotated tag 返回 tag 对象 SHA）与 `GITHUB_SHA`（提交）比对，而 preflight（`:85-89`）用的是剥壳后的 `^{commit}`。历史上已有 4 个 annotated tag（rc.1/rc.5/rc.6、v0.1.1-rc.10）。修法：改用 `gh api repos/$R/commits/$tag --jq .sha` 或 `git rev-parse "$tag^{commit}"`，两处共用同一实现。

### P2-16 CI 无 Windows job

`.github/workflows/desktop-ci.yml:23` 唯一 runner 是 `macos-15-intel`；rc.19/rc.20 当天有 4 次发布 run 因 Windows 专属编译错误白跑。修法：加 `windows-latest` 的 `cargo check --all-targets`（或 clippy）矩阵项，不需要签名与打包。

### P2-17 `open_log_window` 在 async 命令里阻塞

`commands.rs:1091-1130` 的 `rx.recv_timeout(20s)` 直接在 async 命令里等待（对比 `open_harness` 放在 `spawn_blocking`），违反 AGENTS.md 约定，最长占住一个 tokio worker 20s。

### P2-18 `check-invariants.mjs` 的补丁路径判据弱于 Rust

`scripts/check-invariants.mjs:206-212` 对 `from` 只做 `existsSync`（Rust `validate_def` 会拒 `..`、绝对路径、盘符前缀），`:200-205` 的 `to` 放行 UNC。修法：抽一个"分段非空且非 `.`/`..`、不以分隔符/盘符开头"的判据两处共用。

### P2-19 缺"UI 模板未定义标识符"门禁

模板里引用了一个既不在 `setup()` 绑定、也没有全局注册的标识符时，272 个 Rust 测试 + 17 个 UI 测试都不会红，Vue 也只在 dev 运行时打一条控制台警告（生产构建静默把绑定求值成 `undefined`）。这正是本报告自己踩过的坑：仅凭"看模板 + 裸编译"既可能误报（P1-2 的教训），也可能漏报。修法：新增 `scripts/check-ui-bindings.mjs`，用 `@vue/compiler-sfc` 逐文件 `parse` → `compileScript`（**必须取其 `bindingMetadata`**）→ `compileTemplate`，凡渲染结果里仍出现 `_ctx.<name>` 的标识符（即未命中任何绑定）即失败；同时把 `app.config.globalProperties` / 全局组件注册纳入白名单。接入 `check:invariants`。

## 轻微与建议（择机处理）

- `openExternal` 仍 `.catch(() => {})`（`ui/src/bridge.js:55-57`）；`plugins.js:229-243` 忽略后端逐包 `error`。
- `<dl class="entity-meta">` 直挂 `<span>`（`PluginsPanel.vue:219-223`、`SkillsPanel.vue:86-91`）。
- `.mac-titlebar--win .mac-titlebar__caption { right: 104px }` 与注释"Windows 上标题真正居中"矛盾（几何上左偏 52px）。
- `errors.js` 的兜底文案把生命周期/事件处理器里的同步抛出也说成"渲染出错"。
- `replace_child_slot` 的"同一 data dir 两个内核"告警只进 stderr（`commands.rs:650-677`）。
- `prune_old_logs` 的宽限期只看 mtime，注释里的理由在启动期不成立；总量超限会删当天日志（`process.rs:396-451`）。
- `status()` 每轮询重复读设置，设置损坏时每 2.5s 复制一次 `settings.json.corrupt`（且固定名会被二次覆盖）。
- `kernel.rs` 失败的"重装"仍留下被 `list_installed` 当成已安装的半成品。
- 注释漂移：`guard.rs:196-201` 的 `Ok(None)` 语义、`commands.rs:365`/`kernel.rs:1052` 仍引用已删除的 `get_kernel_log`。
- 发布：Verify 的白名单口径与上传口径是两处独立事实（`:503-507` vs `:434-444`），将来必然漂移；ids/names 两次 API 调用按下标对齐有竞态；dispatch 且 tag 缺失时 workflow 自建 tag 会触发第二条必红的 Run。
- `check-signing-keys.test.mjs` 把 rc.18 的 key id 钉死：合法轮换会误红，替绿则取消保护——建议把断言移进脚本并写明轮换流程。

## 未覆盖范围（后续单独立项）

- 存量代码（`233c66d` 之前就存在的路径）：`archive.rs`、`quarantine.rs`、`env.rs`、official-chat 窗口逻辑等未系统审查。
- 运行时行为：Windows 托盘交互、Job Object 实跑、PowerShell 轮询开销、反斜杠归因路径均为静态推理；无真机 E2E（装内核 / 打补丁 / 启停 / 发布全流程）。
- 视觉产物：`theme.css` 的 +435 行只有代码层核对；图标 PNG 未逐像素验证。
- 文档全量事实核对（本轮只比对托盘、`node_path`、端口三处）。

---

## 本轮修复明细（按批次记录）

门禁口径：每条修复都跑 `cargo test` / `cargo clippy --all-targets -- -D warnings` / `cargo fmt --check` /
`node --test ui/test/*.js` / `node --test scripts/*.test.mjs` / `check:invariants` / `build:ui`；
标注"反证命中"的条目做过一次"临时退回旧行为 → 对应用例必须变红 → 恢复"的验证。

### 批次 1：P0 数据破坏

| 编号 | 改动 | 回归/反证 |
| --- | --- | --- |
| P0-1 | `patches.rs::rollback_files` 的"无备份"分支先看 `had_original`：应用前就存在的文件不再被回滚删除，改为在错误文案里如实说明"N 个文件本次未写入、回滚保留原样" | 新增 `rollback_keeps_preexisting_file_when_a_later_file_fails`；反证命中（退回无条件删除即失败） |
| P0-2 | `plugins.rs::check_updates` 的提交阶段改用 `load_store_checked`（与技能侧一致），清单损坏时中止而不覆盖 | 由既有 `load_store_checked` 用例覆盖；行为变化写在函数注释里 |
| P0-3 | 新增 `load_store_for_sweep`（区分"文件不存在"与"空清单"）与 `marked_store_dirs`；`reconcile_store` 只在清单**存在且能解析**时清扫；清单缺失但仍有带标记目录时保留现场并写入 `store.warning`（面板告警条可见）；`Corrupt` 文案改为"优先修复，删掉也不会丢目录但需重装" | 新增 `reconcile_without_store_file_keeps_marked_store_dirs`，同时钉住边界（清单存在且为空时未记账目录仍被清理）；反证命中 |
| P0-4 | `skills.rs::reconcile_home` 增加 `store_present` 判据：清单文件不存在时不做链接清扫；新增 `link_into_store` / `store_link_count` 供清扫与"缺失告警"共用；`status_for_home` 对 `Missing + 仍有技能库链接` 给出告警 | 新增 `reconcile_without_store_file_keeps_store_links`；反证命中 |
| P0-5 | `plugins.rs::ensure_wiring_filtered` 改用 `load_store_checked`，清单损坏时中止接线且不改写 profile（错误经 `ensure_wiring` 的调用方进 trail / `store.warning`） | 既有 `ensure_wiring` 用例链路；行为变化写在函数注释里 |
| P0-6 | `skills.rs::ensure_entry` 在 `replace_owned` 且不归本商店所有时改为 `keep_aside()` 改名保留（`<name>.user-<时间戳>`），`Materialized` 新增 `kept_aside` 并由 reconcile / update 写进 central-store 告警 | 新增 `reconcile_keeps_user_modified_copy_aside`；反证命中（退回 `remove_target` 即失败） |
| P0-7 | `patches::revert` 新增 `force`：对"没有可恢复的原文件"的文件保留现状、如实告警并清除记录；两条失败分支的文案给出「清除记录」出路；`patch_revert` 透传 `force`；UI 只在后端给出该提示时弹二次确认 | 新增 `force_revert_clears_a_record_that_has_no_backup`（钉住"普通撤销必须给出生路 / 重装后仍失败 / force 清记录且不改文件"）；真实内置清单 `dsh-session-perf` 上的复现见 P0-7 明细 |

### 批次 2：P1 功能失效 / 误导

| 编号 | 改动 | 验证 |
| --- | --- | --- |
| P1-1 | `node_install.rs` 新增 `archive_root()`（由 `artifact_for_platform` 的产物名推导顶层目录），`extract_tarball` / `extract_zip` 不再写死 `darwin-x64` / `win-x64` | 新增 `archive_root_follows_the_platform_artifact`（含"写死的 x64 目录名在其它平台必须被拒"）；既有产物名用例补齐 arm64 / linux 三个平台 |
| P1-3 | `tray.rs` 新增 `HIDDEN_TO_TRAY` 状态，`show_main_shell` 在从通知区域恢复时补发 `shell-restored-from-tray`；`App.vue` 只在窗口可见时提示；README / architecture 口径同步改写 | UI 侧为事件驱动，无单测；`check-ui-bindings` 覆盖模板绑定 |
| P1-4 | `App.vue::onQuitConfirmRequest` 区分"内核与对话都没有"的情形，给出中性文案 | 同上 |
| P1-5 | `check-signing-keys.mjs` 入口守卫改用 `pathToFileURL(resolve(argv[1]))`，并在"确实以自身为入口却仍对不上"时显式报错（避免再次静默成功） | `node --test scripts/*.test.mjs` 20 项通过；CLI 直跑验证 |
| P1-6 | `node_install.rs` 回滚分支改为**先探测再删目录**，并把手改 `node_path` 的出路指到真实存在的 `settings.json` 路径 | 由 `probe_failure_detail` 的既有语义覆盖 |
| P1-7 | `commands.rs` 的 `activate_version` / `remove_version` 补 `is_valid_kernel_version` 闸（与 install_kernel / kernel_plugin_list 同一判据） | 判据本身已有单测；两条命令的边界行为写入注释 |

### 批次 3：门禁（本轮新增，防止同类复发）

| 编号 | 改动 | 验证 |
| --- | --- | --- |
| P2-18 | `check-invariants.mjs` 抽出 `isSafeRelativePath`（拒绝空段、`.`、`..`、绝对路径、盘符、UNC），`to` 与 `from` 共用，与 Rust 的 `check_target_path` 对齐 | 现有 3 个补丁定义通过；判据收紧方向为"拒绝更多" |
| P2-19 | 新增 `scripts/check-ui-bindings.mjs` + `npm run check:ui-bindings`，并接入 `check-invariants`（CI 与发布流程已有的两步自动覆盖）；新增 `scripts/check-ui-bindings.test.mjs` | 3 条用例：判据单元、fixture（好/坏/选项式）、真实组件树全绿；反证命中（去掉 `globalBusy` 导入即报错） |

### 批次 1–4 的门禁结果（本轮结束时的实测）

```
cargo fmt --check                                   OK
cargo clippy --all-targets -- -D warnings           OK（零警告）
cargo test                                          278 passed / 0 failed / 1 ignored
node --test ui/test/*.test.js                       17 passed
node --test scripts/*.test.mjs                      20 passed
node scripts/check-invariants.mjs                   OK（45 命令 + 补丁清单 + 三处版本 + UI 绑定）
npm run build:ui                                    OK（JS 424 KB / CSS 138 KB，均在预算内）
```

### 一个必须记录的环境事实：工作区有并发写入者

本轮修复期间，**另一个会话在同一个 checkout 上工作**（`ui/src/theme.css` 被它改动并重建了
`ui/dist`；`ui/src/components/SettingsPanel.vue` 也被它在同一时段写过——P1-2 的误报就是这么来的）。
影响与约定：

- `ui/src/theme.css` 当前的改动**不属于本轮修复**，提交时不要与本轮的修复混在一起；
- 本轮所有结论都已改为以 `git show HEAD:<file>` 复核，修复只落在上面列出的文件上；
- 涉及 UI 的剩余待修项（P2-10 `PluginsPanel.vue`、P2-11 `theme.css`、P2-12 `skills.js`）
  与对方的工作面有重叠，开始前需要确认由哪一方来做。

### 提交记录（本轮，均已通过上述门禁）

| 提交 | 内容 |
| --- | --- |
| `docs: 两天提交的独立复核报告与修复台账（2026-09-11）` | 本报告 |
| `docs: 修正 P1-2 的判定——它在 70f173c 中确实存在` | 判定更正与方法教训 |
| `fix(patches): 回滚不再删除应用前就已存在的文件（P0-1）` | 回滚语义 |
| `fix(plugins): 清单缺失或损坏时不再清理/覆盖用户数据（P0-2/P0-3/P0-5）` | 容错读取链 |
| `fix(skills): 清单缺失不清扫链接；覆盖非本商店条目时改名保留（P0-4/P0-6）` | 技能侧同链 |
| `fix(node_install): 解包根目录按平台推导 + 回滚先取诊断（P1-1/P1-6）` | 平台适配与诊断 |
| `fix(commands): activate/remove_version 补版本号闸（P1-7）` | 纵深 |
| `fix(tray,ui): 收起提示改为恢复时补发；托盘退出文案与实际状态一致（P1-3/P1-4）` | 托盘 |
| `fix(scripts): check-signing-keys 入口守卫改跨平台写法（P1-5）` | 发布门禁误绿 |
| `test(scripts,ci): 补 UI 模板绑定门禁与补丁路径判据（P2-18/P2-19）` | 新门禁 |
| `fix(patches,commands,ui): 给"无备份记录"一条清除出路（P0-7）` | 死记录出路 |
| `fix(kernel,commands): pid 记录带端口 + stop 如实报告失败（P2-1/P2-2）` | 内核身份 |
| `fix(releases,plugins,skills): SRI 支持多摘要，不再被空格卡死（P2-6）` | 下载校验 |
| `fix(commands): 保存设置不再清空 node_path；node 缓存不再挡死启动（P2-7/P2-8/P2-17）` | 设置与阻塞 |
| `fix(guard): 环境类失败不再进插件阶梯；归因认识 Windows 反斜杠路径（P2-3/P2-4/P2-5）` | 启动看护 + 夹具隔离 |

> 提交只包含本轮修复涉及的文件；`ui/src/theme.css` 属于并发会话的改动，未被纳入。

### 下一步（剩余待修）

| 优先级 | 编号 | 说明 |
| --- | --- | --- |
| 中 | P2-9 | Windows 进程身份校验换轻量 API（现在每 2.5s 派生一次 PowerShell） |
| 中 | P2-13 / P2-14 / P2-15 | 发布流程：清理前置、单调性门 fail-closed、tag 比对改用 `commits/<tag>` |
| 中 | P2-16 | CI 增加 `windows-latest` 的 `cargo check --all-targets` |
| 中 | P2-10 / P2-11 / P2-12 | UI 三项（与并发写入者的工作面重叠，待分工确认） |
| 低 | P3-* | 轻微项：注释漂移、`eprintln` 可见性、`prune_old_logs` 语义、发布白名单口径等 |

### 批次 4：P2 内核身份、环境失败分类与 SRI

| 编号 | 改动 | 回归/反证 |
| --- | --- | --- |
| P2-1 | `kernel.pid` 记 `"<pid> <port>"`（`read_pid_record` 兼容旧格式），`workbench_pid` 对带端口的记录走完整三层判据，端口兜底分支同样带端口；`kill_pid` 调用点带 `recorded_kernel_port` | `pid_record_carries_the_start_port_and_accepts_the_legacy_format`、`recorded_port_mismatch_is_not_our_workbench`（真实诱饵进程）；反证命中：退回 `pid_is_kernel(pid, None)` 即失败 |
| P2-2 | 新增三态 `process_state`（Alive / Gone / Unknown），`stop` 只在有正面证据时返回带 pid 与下一步的错误；`process_command` 空输出改为 `None`（顺带修掉"Windows 上把不存在的 pid 判成 NotKernel、误拒打开工作台"） | `process_state_distinguishes_gone_from_unknown` |
| P2-3 | 只有本轮真的改过隔离/接线才跑 `restore_profile_manifest`；环境类失败的事故文案给出真实原因与下一步，`cause = "env"` | 见 P2-4 用例的轨迹断言（含"跳过接线恢复与 pnpm 重装"） |
| P2-4 | `ENV_FAILURE_MARKERS` + `environment_failure`：命中 EADDRINUSE / EACCES / ENOSPC / NODE_MODULE_VERSION 等时不进插件归因、不重试停用、不进安全模式 | `environment_failure_recognizes_port_and_permission_errors`、`environment_exit_skips_the_plugin_ladder_and_the_pnpm_restore` |
| P2-5 | `is_anchored_plugin_hit` / `is_kernel_evidence_line` 先做反斜杠归一化 | `attribution_matches_windows_backslash_paths`（含段落边界反例） |
| P2-6 | SRI 按 token 解析多摘要、取最强受支持算法、未知算法跳过；注释里的威胁模型改正 | 既有 `download_integrity_compares_content` 扩到 5 种形态 |
| P2-7 | `merge_settings`：`None` = 未提交（继承现值），`Some("")` = 显式清空 | `merge_keeps_paths_the_panel_did_not_submit` |
| P2-8 | `start_kernel` 命中 `ok:false` 缓存时强制重探；`detect_node` 成功回写缓存 | 行为变化写在注释与提交里（无独立用例：需要真实的 Node 探测环境） |
| P2-17 | `recv_timeout(20s)` 移入 `spawn_blocking` | 编译与既有 UI 测试覆盖 |

**P2-6 的保留项**：`integrity` 与 `shasum` 都没有时仍然"跳过校验并继续安装"（只在
进度里说明）。要改成默认拒绝需要：① 消费 `dist.shasum`（sha1）作为回退，而仓库目前
没有 sha1 依赖（要么新增 `sha1` crate，要么手写 SHA-1 —— 后者属于"手搓密码学"，
不建议）；② 或者把"无摘要"变成硬失败（会影响使用老 packument 的 registry）。这是
需要产品决策的一项，未擅自改。

**顺带发现并修掉的测试夹具缺陷**：`plugins::store_dir()` 取的是 data dir 的**父目录**
（`<home>/plugins`），而 guard 的 `temp_data_dir` 把临时目录本身当 data dir——于是所有
guard 用例共用 `/tmp/plugins`，某个用例的清理会删掉别人正在用的目录。表现为与被测
行为无关的偶发失败（10 轮里 6 次；隔离后 8/8 稳定）。已改为每个用例 `<home>/desktop`。

**待办（UI，需与并发会话协调）**：`cause = "env"` 需要在 `IncidentModal.vue` 与
`OverviewPanel.vue` 的 cause 白名单/标题映射里加一项（现在会回落到"暂未能归因"，
而面板正文已经是明确的环境原因）。这两个文件当时正被另一个会话编辑，本轮没有改动。

### 批次 5：Windows 轮询、UI 三处、发布流程与 CI

（P1-2 不在批次表里：它的判定经过两次更正，单独记在「关于 P1-2 的最终判定」与
P1 明细里。）

| 编号 | 改动 | 回归/反证 |
| --- | --- | --- |
| P2-9 | Windows 专属 `command_cache`：同一 pid 的命令行结果缓存 3 秒；查询失败不缓存。端口活体与进程存活仍是实时查询，因此启停判定不受影响 | Windows 专属模块无法本机编译，已抽到临时 crate 以 `x86_64-pc-windows-msvc` 类型检查通过 |
| P2-10 | 模式徽章改 `withLoading('pluginMode:' + row.id, …)` + `:loading="isLoading(...)"`，保留 `:disabled="globalBusy"` | `check-ui-bindings` + 22 条 UI 测试 |
| P2-12 | 逐包失败只在手动点击时提示；失败后 5 分钟自动退避（手动不受限） | 新增用例 `failed skill update checks back off, and manual checks bypass the backoff` |
| P2-13 | 资产整理（删计划外 + 恰好 6 个 + 逐名比对）前置为 `Normalize draft assets before publishing`，位于 `draft=false` 之前 | 33 个 `run:` 块全部 `bash -n` 通过；步骤顺序已打印核对 |
| P2-14 | 单调性门区分 404 与其它失败：非 404 重试 3 次后 `exit 1` | 同上 |
| P2-15 | tag 比对改 `commits/<tag>`（服务端剥壳），与 preflight 的 `^{commit}` 口径一致 | 同上 |
| P2-16 | `desktop-ci.yml` 新增 `windows-compile` job（`cargo check --all-targets`，只需一个最小 `ui/dist`） | 本机验证过"最小 dist + cargo check"可通过 |
| P3（部分） | `prune_old_logs` 按文件名日期戳跳过当天日志，并把"总量预算是软上限"写进注释 | 新增 `todays_logs_survive_even_when_the_budget_is_exceeded`；反证命中 |
| P2-11 | 不改 `theme.css`（并发会话工作面）：在 `ui/src/notify.js` 显式给 `ElMessage` / `ElMessageBox` 指定 `zIndex = 浮层 + 1000`，两者都浮在进度浮层之上，其它 `el-dialog` 仍按原设计压在浮层之下 | 新增用例 `提示与确认框显式抬到进度浮层之上`（对 `theme.css` 的浮层数值与 `notify.js` 的常量做关系断言） |

**P2-11 的处理方式**：不改 `ui/src/theme.css`（并发会话工作面），改为在
`ui/src/notify.js` 里显式给 `ElMessage` / `ElMessageBox` 指定 `zIndex`（浮层 + 1000），
并新增用例钉住"提示层级 > 浮层层级"这条关系。

**仍未处理**："轻微与建议"里剩余的纯文案/可见性项（`replace_child_slot` 告警只进
stderr、`errors.js` 兜底措辞、`openExternal` 静默、`<dl>` 内容模型、标题栏 `right:104px`
与注释不符、`check-signing-keys.test.mjs` 的 key id 钉死、发布白名单两处口径、
dispatch 自建 tag、`status()` 重复读设置、`.corrupt` 固定名）。
