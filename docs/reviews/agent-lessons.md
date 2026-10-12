# 事故经过存档

`AGENTS.md` 里每条纪律背后都有一次真实踩坑。纪律本身留在 `AGENTS.md`（它受运行时
32768 字节的**硬预算**约束，超出部分根本不会被加载），事故经过归档在这里，供需要
追溯时查。

写作约定：每条写清 **现象 → 根因 → 结论**，结论一句话。日期用事件发生日。

---

## 1. 给内核打补丁「可逆所以安全」是答错了问题（2026-09-29）

**现象**：有人给 `dsh-client-ui-renderer/lib/client.js` 打过一个补丁——「作用域缺席时
挂起而不是抛错」，随 0.3.5-rc.4 发布。

**经过**：该补丁的论证是「可逆 + 有备份 + 有哈希校验」，很快被认定为违背产品定位并撤销。

**结论**：那套论证回答的是「破坏可不可逆」，不是「该不该做」。两者是不同的问题，
而「该不该」在 `AGENTS.md` 的只读内核边界那一条里已经答完了。可逆性不构成写入理由。

---

## 2. 从仓库删掉补丁，不等于用户机器上消失（2026-09-29）

**现象**：删掉两个补丁目录后重新装 rc.4，用户安装目录 `<install>/patches/` 下
**两个目录仍在**。

**根因**：更新只覆盖与新增，不清理上游删掉的东西。

**结论**：要真正清干净，得让应用自带一份「本次构建带了哪些补丁 id」的名单（构建期
生成），再在 `load_patches` 时按它裁剪资源目录。在此之前，删除补丁后必须在发布说明里
写明它不会自动卸载。

---

## 3. PowerShell 恢复文件后不更新时间戳，验的是旧产物（2026-09-30）

**现象**：给 `kernel::tests` 做反向验证，「备份 → 改坏 → 跑测试（红）→ 恢复」四步走完，
恢复后**仍然红**。

**根因**：PowerShell 的 `Copy-Item` **保留源文件的 `LastWriteTime`**。恢复那一步写回去的
是比「改坏」那一步**更早**的 mtime，cargo 判定无需重编译，于是第二次跑的还是改坏时的
产物。真相是根本没重编，不是断言不成立。

**结论**：恢复后补一句 `(Get-Item <file>).LastWriteTime = Get-Date` 再跑。
同理，**反向验的红必须是「改坏的当次」的红**——中途任何一次重编失败都可能让红变成
「编不过」而不是「断言不成立」，那不算验过。

---

## 4. 版本号只改两处，门禁比文档多查一处（2026-10-03，v0.3.9-rc.1）

**现象**：`package.json` 与 `src-tauri/tauri.conf.json` 都改了版本，`Cargo.toml` 漏了。

**后果不是编译失败**：crate 版本与壳版本不同源，Tauri 打的产物里元数据会带着一个
对不上的壳版本号，updater 与「关于」面板显示的都可能不是这个 release。是
`check:invariants` 的 `[versions]` 项当场抓出来的。

**结论**：**改版本号一律三个文件一起改**，别等门禁报。

---

## 5. `bundle.resources` 少一步生成，CI 全红而本地全绿（2026-10-09，v0.4.4）

**现象**：新 checkout 上第一个 cargo 命令死在
`resource path 'resources/builtin-plugins' doesn't exist`。表现为 quality job 的
`cargo test` 与两个平台的 `tauri build` 报同一行、而 `Publish release` 被跳过——
**red 的表象与真正原因隔着两层**。该 break 在 main 上已连红 4 个提交。

**根因**：`src-tauri/resources/builtin-plugins/` 是构建期产物（`.gitignore` 挡住，
由 `npm run prep:builtin` 从 `plugins/openai-oauth/` 生成），而 build script 校验的是
登记在 `bundle.resources` 里的**每一个**路径。本地 `npm run dev` / `build` / `check`
都各自带着这一步，所以**本地永远看不出来**。

**结论**：`scripts/dev-builtin.test.mjs` 钉住接线形状——任何会跑
`cargo build/check/test/clippy` 的 workflow job 都必须先生成资源，并带「判据空转时
必须红」的兜底。新增一个同类生成目录时，把它的生成步骤挂进判据，不要只改 workflow。

---

## 6. 常驻行为「抄一份就会漂」的三次实例（2026-10-02 / 10-05 / 10-09）

**a. 恢复动作被抄了一份（10-02）**：`lib::show_main_shell` 与工作台拉绳的
`focus_main_shell` 各自实现了一份唤回逻辑，Windows 上两边互调过一次，直接
`thread 'main' has overflowed its stack`。

**b. 收起动作绕过了统一实现（10-05）**：登录自启分支裸调 `window.hide()`，macOS 上
激活等级没降到 Accessory——进程带着「在运行」小点的 Dock 图标、**零可见窗口**地挂在
后台，点它还没反应。`RunEvent::Reopen` 只抬工作台，而 macOS 的激活不会替我们显示
`hide()` 掉的窗口，抬不到时必须兜底 `show_main_shell`。

**c. 任务栏按钮只删不补（10-09）**：删除走 `ITaskbarList::DeleteTab`，而 `show()` 不会
让它自己回来。补回的那一份曾经住在 `tray::show_main_shell` 而**没有任何调用方**走它，
于是登录自启藏起面板后，用户点启动项唤回的窗口在任务栏上没有按钮。那份实现已删。

**结论**：常驻行为（关闭 / 收起 / 恢复）三件事各只有一份实现，全在 `shell/resident.rs`。
抄一份就会漂，而漂在 Windows 上往往直接是崩栈或「点了没反应」。

---

## 7. 「只搬进 release」防的是再犯，防不了已经犯的（2026-09-28）

**现象**：release 侧工作台从此是空列表，而 `~/.dsh` 已空——搬不动第二次。

**根因**：dev 壳在闸门落地（`713bb40`）前 15 分钟就把 `~/.dsh` 并进了 `default-dev`。

**结论**：补的是回收路径 `home_recovery`——只读扫描别的实例 home 里本实例缺的
`sessions/` 与 `attachments/`，用户拍板后**复制**过来，源永不删除、目标已有条目永不覆盖。
执行前要求**目标实例**的内核未在运行（判据按实例 pid 文件而非本壳工作台：用户自建实例
留在两份注册表里，另一壳正跑着它同样要拒）。UI 挂在「数据迁移」面板的「找回历史会话」
卡片上（`scan_misplaced_home` / `recover_misplaced_home`）。

---

## 8. `DSH_CUSTOM_SKILL_DIRS` 注了半年，内核从来不读（2026-09-30 查清）

**现象**：技能「装得上、内核看不见」持续约半年，无人察觉。

**根因**：`dsh-skill-filesystem` 的 `customSkillDirs` 只从 `cordis.patch.yml` 的插件配置
读；0.2.0-rc.2 全树 3481 个 js/d.ts 里无 `CUSTOM_SKILL` 命中。

**结论**：**判断「内核到底认不认某个注入方式」必须实测，不能读接口名推断**。已用
`--dump-config` + `skills/list` RPC 端到端验证。接通靠的是 `ensure_skill_wiring` 往
`cordis.patch.yml` 追加的 loader 行，不是那条 env。