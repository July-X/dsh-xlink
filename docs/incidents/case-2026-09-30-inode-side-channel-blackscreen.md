# 案例：dev 壳装 / 删内核打死 release 工作台（pnpm store 的 inode 侧信道，2026-09-30 定案）

> 这是本仓库最重要的一类留存：**一次跨了三天的排查，三个理论先后被证据杀死，最后靠一条不经过任何路径的元数据侧信道定案**。结论已经进代码与 [AGENTS.md](../../AGENTS.md)，本文记的是**完整证据链与排查方法**——下次遇到「两个分了家的东西仍然互相惊动」时，照这里的工具箱来。

## 摘要

**一句话**：两个壳的内核安装树物理不相交，但 pnpm 的内容寻址 store 让它们与 store 里**内容相同的文件共享同一个 inode**；对面的树在被硬链接 / 取消链接时，NTFS 更新这些文件的 ChangeTime，内核 `dsh-client-hmr` 每 500ms 的 bundle stat 轮询把这种噪声当成「模块重建」推给**活页面**，页面在换模块的窗口里撞槽位装配不变量死掉。**与 CPU / 磁盘 / 网络负载无关**——「资源争用」是本案存活最久的错误结论。

**修复**：内核安装固定 `--config.package-import-method=copy`（树持有全新 inode，根治）+ 恢复动作等风停 + 重建预算改为「3 次 + 20s 冷却」+ 导航观测。提交 `022a734`。

## 现象与时间线

用户报告的两个症状（真机，2026-09-30）：

1. dev 装（新）内核 → release 工作台黑屏，reload 一次后恢复；
2. dev 卸载内核 → release 工作台黑屏**两次**，reload 一次看似恢复又再黑，最终卡死，只能手动「刷新工作台」。

对齐两壳的日志（`shell/dev/logs/dev-kernel-other-shell-*.log` × `shell/release/logs/release-harness-window-*.log`）后，当天**六次**装 / 删全部命中，间隔稳定在 4~6 秒：

| dev 侧动作 | release 工作台的反应 | 间隔 |
| --- | --- | --- |
| 10:32:42 装 0.2.0-rc.1（更早会话实测） | 10:32:47 重载、10:32:48 抛 `scope 'session-maybe' rendered without an installed adapter` | 5s |
| 13:59:58 安装 | 14:00:04 页面开始加载 | 6s |
| 14:01:08 卸载 | 14:02:01 页面开始加载 | 53s |
| 14:55:35 卸载 | 14:55:40 页面开始加载 | 5s |
| 15:35:28 安装 | 15:35:33 页面开始加载（自愈刷新） | 5s |
| 15:36:28 卸载 | 15:36:32 **活页面**报槽位故障 → 自动重建 → 4s 后新窗口又死 → 额度尽，黑屏挂到 15:37:05 手动刷新 | 4s |

**这张表本身就是第一份证据**：6/6 命中、间隔 4~6 秒的**确定性**触发，直接排除一切「概率性」解释。

15:36 那次的时间线值得细看（它同时解释了「为什么恢复失败」）：

```
15:36:28  dev 写装包信标，开始 remove_dir_all（34s 的文件删除）
15:36:32  release 活页面报槽位故障（第一次，slot-assembly）
          → 页面自愈：3 秒后 reload
15:36:32  壳自动重建窗口（页面第二次报 runtime-error，判「刷新救不回来」）
15:36:36  新窗口加载 → 风暴未停 → 又撞死（新窗口 sessionStorage 是空的，
          自愈额度重置，又自愈 reload 一次）
15:36:40  再报同一处故障 → 此时「每进程只重建一次」的闸已落下
          → 没有任何自动动作 → 黑屏挂死
15:37:05  用户手动「刷新工作台」→ 卸载已结束 → 恢复
```

**两个教训**：恢复动作（自愈刷新 / 自动重建）全部落进风暴里，几秒内又死一次——**落点比次数重要**；一次性预算闸在真正需要第二次的时候恰好把出路关死。

## 三个先后被证伪的理论

知识库里最重要的部分。每个理论都记录「当时为什么像真的」与「什么证据杀死了它」。

### 理论一：「内核的文件监视器互相惊动」（事件风暴）

- **为什么像真的**：2026-09-29 有一次同壳实测（删除一个未被使用的同级内核目录，25643 文件 / 34.2s，6 秒后页面死，boot rev 确有变化）看起来就是「事件风暴 → 模块图变更」。跨壳那次症状一模一样，顺理成章外推。
- **怎么死的**：2026-09-30 上午的全树审计——装着的 0.2.0-rc.2 内核全树**只有四处 chokidar**（凭据文件、`dsh-fs-local` 被点名的 target、`dsh-hmr` 的 profile patch、技能根），**没有一处盯内核安装树**，`bootRev` 全树零命中。
- **死后的幽灵**：审计结论本身没错，错的是**由「没找到 watcher」推出「没有反应源」**——真正的反应源（stat 轮询）不是 watcher，恰好不在审计的搜索范围内。`dsh-client-hmr` 自己的 invariant 注释里明写着它是「the composition's only stat-poll user」，就摆在没人 grep 的地方。

### 理论二：「机器资源争用」（存活最久，写进过文档）

完整版本：pnpm 硬链接数万文件 + node-gyp 编译打满 CPU / 磁盘 → 对面页面加载被拖过看门狗 15s 门槛 → 看门狗把「慢」判成「死」并重载 → 重载落在机器最忙的时刻 → 撞上启动顺序竞态。

- **为什么像真的**：链条每一环都符合直觉；「装包确实又慢又重」是真的；看门狗 15s 误判的故事有鼻子有眼。
- **怎么死的**，四刀：
  1. **看门狗重载计数为 0**——当天六次事故里，grep「超过 15s 没有加载完成，已自动重载」零命中。被判成凶手的那个机制一次都没开过枪。
  2. **触发安装只跑了 9.2 秒**，537 个包全部从 store 复用（`downloaded 0`）、没有原生模块编译、pnpm 在 BELOW_NORMAL 优先级——它没有能力打满一块 14 核 / 64GB / NVMe 的机器。
  3. **15:36:32 是活页面报的错**：15:35:33 加载完成后到 15:36:32 之间没有任何加载事件，页面是**活着**的时候被换模块换死的，不是「没加载出来」。
  4. 用户自己的直觉：「当前 Windows 硬件性能不应该出现这种资源紧张」。**用户的怀疑是对的，排查应该更早顺着它走。**
- **死因总结**：把「机器忙时确实可能更糟」当成了「机器忙是原因」。放大器不等于凶手。

### 理论三：「WebView2 共享浏览器进程级联崩溃」

- **为什么像真的**：两个壳用同一个 bundle identifier → 同一个 `com.zhongxingxing.dsh-xlink\EBWebView` 用户数据目录 → **确实共享同一个浏览器进程**（dev 主窗的渲染进程挂在 release 壳的 browser process 下，进程列表实证）。共享 GPU 进程崩溃会级联杀死两边所有渲染进程，是现成的机制。
- **怎么死的**：进程创建时间——browser / GPU 进程创建于 15:34:08，**整个事故期间（15:35–15:37）从未重启**；渲染进程的更替全部对应窗口重建动作。级联不存在。
- **留存价值**：虽死，但「两个壳共享一个 WebView2 浏览器进程」本身是**真实的耦合面**，将来排查 UI 类怪象时先看这个（一条命令：`Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'"` 看 `--user-data-dir` 与创建时间）。

## 定案机制：每一环的证据

```
dev 装/删内核 = 对 pnpm store 的文件创建/撤销硬链接
   ↓ ① store 按文件内容寻址：两个壳的树 + store 里内容相同的文件是同一个 inode
   ↓ ② NTFS：链接数增减 → ChangeTime 更新（mtime 不动）
   ↓ ③ 内核 dsh-client-hmr：每 500ms statSync 每个客户端 bundle，
   ↓    sameBundleStat(mtime,ctime,size) 判定「变了」→ clientModules.rebuilt(id)
   ↓ ④ SSE "rebuilt" 帧 → 浏览器半 entries.reload(id, rev)：活页面换模块
   ↓ ⑤ 换模块瞬间 dsh-client-ui-session 被销毁，session 作用域被 disposer 摘掉
   ↓ ⑥ ScopeProvider 在「已删除、尚未重装」窗口里重渲染
   → 抛 "scope 'session-maybe' rendered without an installed adapter"，页面死透
```

| 环 | 证据 | 怎么取得的 |
| --- | --- | --- |
| ① inode 共享 | release rc.2 正被轮询的 `dsh-client-ui-session/lib/client.js` 被 **4 条路径**共享：store 文件、release rc.1 树、dev rc.2 树、release rc.2 树 | `fsutil hardlink list <文件>`；`fsutil file queryfileid` 证 rc.1/rc.2 同 id |
| ② NTFS 语义 | 对**自己的临时文件**加 / 删一个硬链接：`ctimeMs` 变、`mtimeMs` 不变、size 不变 | `node -e statSync` 前后对照（见下文工具箱） |
| ③ 轮询器 | `dsh-client-hmr/lib/index.js`：`pollIntervalMs` 默认 500，`sameBundleStat` 比较 mtime/ctime/size，变了就 `rebuilt(id)`；模块文档自述「One interval stat-polls every graph row's client bundle」 | 直接读装着的内核源码 |
| ④ 活页面换模块 | 浏览器半 `client.js`：EventSource 收 `rebuilt` 帧 → `entries.reload(frame.id, frame.rev)` | 同上 |
| ⑤⑥ 槽位崩溃 | 抛点在 `dsh-client-ui-renderer/lib/client.js:292`；作用域由 `dsh-client-ui-session` 的 effect 安装、disposer 摘除（`client.js:1466`） | 同上 + 壳内 `harness-health.js` 的注释（2026-09-29 的实测观察早已写下，只是机制归因错了） |
| 时序 | 4~6s ≈ pnpm resolve（2~4s）+ 首批硬链接 + ≤500ms 轮询 + SSE + 换模块 | 上表六次实测 |

**为什么内核日志一行都没有、HTTP 全程 200、进程健康**：轮询、推送、换模块全程在「内核认为一切正常」的路径上——它忠实地把一次误判的重建推给了页面，页面忠实地死了。

**为什么同壳也中**（09-29 那次）：同一个 store、同一个机制，删自己树里的链接同样 bump 对面（运行中内核树）的共享 inode。跨壳不是必要条件，「共享 store + stat 轮询」才是。

## 修复（四层，提交 `022a734`）

| 层 | 内容 | 验证 |
| --- | --- | --- |
| **⓪ 根治** | 安装参数固定 `--config.package-import-method=copy`：树持有全新 inode，装 / 删**物理上碰不到对面的任何文件**。连带收益：用户自己的 pnpm 项目（同一 store）也不再能惊动工作台——修之前它们走同一侧信道也能。代价：每版本真实占盘约 450 MB、安装慢几秒。**存量硬链接树不迁移**：卸载旧版树仍会惊动对面一次（下层兜住），把该版本卸载后重装一次即彻底隔离 | 端到端实证（下文工具箱末条）：copy 安装前后 release bundle 的 stat **分毫未动**；新树文件 `fsutil hardlink list` 只有自身一条路径。机械检查 `kernel-install-isolated-inodes` 钉住参数不得被删 |
| **① 恢复等风停** | 信标（`package_activity.rs`）主职改为 `recovery_backoff()`：页面自愈刷新前经 `harness_reload_backoff` 命令问壳（≤5s 一轮、上限 30 轮、IPC 失败按老行为 3s 照刷）；壳自动重建走 `recreate_when_quiet`（后台线程等风停、上限 240s、落地前重核内核在服务、单等待者）。看门狗阈值放宽（clamp 15s→120s）降级为防御——真机从未触发过那条路径 | 纯函数测试 + 注入脚本 VM 测试 + 接线机械检查 |
| **② 输入不丢** | 草稿存续（`harness_draft.rs`，前一轮已落地）照旧 | 前一轮的测试 |
| **③ 重建预算** | 「每进程只重建一次」→ **3 次 + 20s 冷却**（`MAX_REBUILDS` / `REBUILD_COOLDOWN`）：一次性闸门在 15:36 的事故里把用户晾在黑屏上 25 秒。防「窗口自己跟自己打架」的闸是冷却，不是一次性。手动「刷新工作台」先 `reset_budget` 清账，手动出路永远有额度 | 纯函数测试 + 接线机械检查 |
| **观测** | harness 窗口 `on_navigation`：每个导航记一行地址 + 当时的跨壳装包上下文（「跨壳装包活动进行中，还剩约 Ns」）——补上「页面无声死掉时壳看不见它去过哪」的缺口 | 15:35:33 那种「无声自愈刷新」下次会留下带上下文的一行 |

门禁与反向验：clippy 零警告、invariants 全过（含新增 / 扩展项）、budget 35102/35120、test:ui 153/0、cargo test 593 pass / 4 fail（基线）；**五个方向反向验全过**——其中反向验抓出旧 ACL 检查**只认单引号**的盲区（`report_harness_fault` 从未被覆盖），已修成两种引号都认。

## 排查工具箱（可复用）

```powershell
# 1) inode 归属：一个文件被哪些路径共享（跨树、跨壳、store 全在里面）
fsutil hardlink list "C:\…\node_modules\@deepseek-ai\dsh-client-ui-session\lib\client.js"

# 2) 两个文件是否同一个 inode
fsutil file queryfileid <路径A>   # 与 <路径B> 的输出比对

# 3) NTFS ctime 语义验证（在自己的临时文件上做，绝不动正被服务的内核树！）
$a = "$env:TEMP\a.txt"; Set-Content $a x
node -e "console.log(require('fs').statSync(process.argv[1]).ctimeMs)" $a
fsutil hardlink create "$env:TEMP\b.txt" $a
node -e "console.log(require('fs').statSync(process.argv[1]).ctimeMs)" $a  # 变了
Remove-Item "$env:TEMP\b.txt"
node -e "console.log(require('fs').statSync(process.argv[1]).ctimeMs)" $a  # 又变了

# 4) WebView2 进程拓扑与共享面（user-data-dir 相同 = 共享浏览器进程）
Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" |
  ForEach-Object { /* 解析 --user-data-dir 与 --type，看创建时间 */ }

# 5) 时间线对齐：把两个壳的日志按时刻拼起来（本案的定案手法）
#    shell/<mode>/logs/ 下的 *-harness-window-*.log × *-kernel-other-shell-*.log

# 6) 端到端验证一个修复（安全版：带 copy 参数，全程不 bump 任何共享 inode）
$rel = "<正被轮询的 bundle 路径>"; $t = "$env:TEMP\copy-check"
node -e "console.log(require('fs').statSync(process.argv[1]).mtimeMs, require('fs').statSync(process.argv[1]).ctimeMs)" $rel
pnpm add --prefix $t --config.node-linker=hoisted "--config.package-import-method=copy" "@deepseek-ai/dsh@<版本>"
node -e "…再 stat 一次，必须分毫未动…"; fsutil hardlink list "$t\node_modules\…\client.js"  # 只该有它自己
Remove-Item -Recurse -Force $t
```

**安全红线**：在第 3 / 6 步里**绝不能**对正被服务的内核树创建或删除链接、也绝不能跑**不带 copy 参数**的对照安装——那正是本案的凶器，会当场打死正在跑的工作台（本文六次事故就是对照组）。

## 方法论教训

1. **时间线对齐先于理论**。把两壳日志按时刻拼成一张表（本案 6/6、间隔 4~6s）之后，「概率性资源争用」这类解释当场死亡。任何「可能是负载/时序/竞态」的叙事，先问一句：命中率是多少？
2. **「审计没找到 X」≠「没有 X」**。审计只覆盖它搜索的类别（本案：watcher）。反应源不止 watcher 一种——stat 轮询、目录 mtime 轮询、USN 日志都可能是。读代码时注意「自述文档」：真正的轮询器在它自己的 invariant 注释里写着「the composition's only stat-poll user」。
3. **隔离是分层的**：路径分家 ≠ inode 分家。内容寻址的去重存储（pnpm store、硬链接缓存、去重备份）会把「物理不相交的目录」重新耦回**同一个 inode**——写入侧的隔离承诺会被存储层静默撤销。
4. **元数据侧信道**：不写对方的任何数据也能杀死对方（链接数 → ChangeTime）。判断「A 能否影响 B」时，除了「A 写了什么 B 读的东西」，还要问「A 改了什么 B 轮询的元数据」。
5. **放大器不是凶手**。「重载落在机器最忙的时刻」是真的，但它只是把一次必死弄得更难看。分清因果链上谁是触发、谁是放大，修放大器永远治不好触发。
6. **用户的直觉是数据**。「这硬件不该紧张」这个怀疑本身就该更早被当作待检验命题——它最后被证明是本案最准的一条线索。
7. **反向验是检查的检查**。本案的新 ACL 检查第一版对双引号 invoke 整体失明（旧的 `report_harness_fault` 同样从未被覆盖），是反向验（弄坏 → 看它响不响）抓出来的。一条没被弄坏过的检查等于没检查。

## 残留缺口与内核侧缺陷（截至定案日）

- **内核侧两缺陷仍在**（归内核仓库，壳不改内核代码）：① `dsh-client-hmr` 把 stat 噪声（链接数变化引起的 ChangeTime）当成 bundle 重建并推给活页面——建议比对 mtime+size 而非 ctime，或对「内容未变」的重建帧不推送；② 页面换模块的窗口里槽位装配不变量会崩（`dsh-client-ui-session` 的 disposer 摘作用域与 `ScopeProvider` 重渲染之间的窗口）。⓪ 让两者失去触发源，但缺陷本身值得反馈。
- **存量硬链接树**：卸载旧版装的内核仍会惊动对面一次（自愈兜住）；重装该版本即隔离。**依赖 `@deepseek-ai/dsh-client-*` 的插件安装**理论上仍能经同一 store 触发，同靠自愈兜底。
- **插件装 / 删不打信标**：恢复退避目前只在内核装 / 删两端踩刹车。

## 代码与文档索引

- 根治参数：`src-tauri/src/kernel.rs` 安装 args 里的 `--config.package-import-method=copy`（机械检查：`scripts/check-invariants.mjs` 的 `kernel-install-isolated-inodes`）
- 恢复链：`src-tauri/src/harness_window.rs`（`recreate_when_quiet` / `MAX_REBUILDS` / `REBUILD_COOLDOWN` / `on_navigation`）、`src-tauri/src/package_activity.rs`（`recovery_backoff`）、`src-tauri/src/harness_cmd.rs`（`harness_reload_backoff` 命令 + ACL）、`src-tauri/src/harness-health.js`（`reloadWhenQuiet`）
- 规则与机制叙事：[AGENTS.md](../../AGENTS.md)「装 / 删内核不许惊动另一个壳的工作台」；[architecture.md](../architecture/architecture.md)「分四层处理」；[troubleshooting.md](../operations/troubleshooting.md) 对应两行与残留缺口
- 提交：`022a734`（2026-09-30）
