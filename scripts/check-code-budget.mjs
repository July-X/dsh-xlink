#!/usr/bin/env node
/**
 * 代码膨胀门禁：把「代码在变胖」变成 CI 里看得见的失败，而不是半年后的一次
 * 大扫除。
 *
 * 用法：
 *   node scripts/check-code-budget.mjs            # 校验，超预算以非零退出码结束
 *   node scripts/check-code-budget.mjs --report   # 只打印度量，不判失败
 *
 * 两类度量：
 *
 *   1. **生产代码行数**（按文件）。刻意排除 `#[cfg(test)] mod ...` 整块：测试
 *      该长就长，让测试行数掩盖生产代码的膨胀才是问题。新模块要给新预算，
 *      改预算必须和代码出现在同一个提交里——这就是「要么拆，要么显式承认」。
 *
 *   2. **重复块**。归一化每行（去空白、去注释、丢掉纯括号行）后取 10 行滑窗，
 *      把命中 ≥2 次的窗口按文件并成「最长重复区间」，只统计长度 ≥12 行的区间。
 *      滑窗会互相重叠（一个 20 行的复制粘贴会产生 11 个窗口），所以必须先合并
 *      再计数，否则指标会随块长线性放大、没法当门禁。指标是**重复区间个数**
 *      （每份拷贝各算一处）：跨模块逐字复制的样板都在这个尺度上，而
 *      `let x = 1;` 这类单行巧合不会。
 *
 * 阈值取当前值 + 余量：目标是拦住持续增长，不是要求立刻删代码。真要涨，
 * 在同一个提交里改这里的数字，让 review 看到代价。
 */

import { readFileSync, readdirSync, statSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const reportOnly = process.argv.includes('--report');

/**
 * 任何**新登记**的文件的预算硬顶。
 *
 * 老文件（基线里已有的）不追溯：plugins.rs / theme.css / commands.rs 早就
 * 超顶，追溯等于本门禁一上来就红，没人会去修。老文件走「预算只许下调」
 * 那条规则逐阶段收敛；硬顶只管**下一个** plugins.rs 别在诞生时就超标。
 */
const HARD_FILE_CEILING = 800;

/**
 * 反棘轮只作用于基线预算达到该值的文件——也就是"已经有膨胀风险、正在
 * 变成下一个 plugins.rs"的那批。刚拆出来的小模块（一个文件一个关注点）
 * 后续把关注点补完整不算膨胀，不该被这条规则卡住。
 */
const RATCHET_THRESHOLD = 600;

/**
 * 读出本文件**已提交版本**里的预算数字当基线。
 *
 * 这是反棘轮的支点。基线取自 HEAD 而不是另存一份 JSON，有两个好处：
 * 1. 不需要维护第二份数据，两边漂移无处可藏；
 * 2. 「同一个提交里改数字」这个流程**不会**让检查失守——HEAD 里的旧值
 *    仍然是旧值，所以"这次把预算写大了"照样看得出来。
 *
 * 拿不到（首次提交 / 非 git 目录 / git 不可用）时返回 null，调用方跳过
 * 反棘轮检查并在报告里说明——**宁可少查一项，也不要因为拿不到基线就整条
 * 门禁失效**。
 */
function readBaseline() {
  let text;
  let tree = new Set();
  try {
    text = execFileSync('git', ['show', 'HEAD:scripts/check-code-budget.mjs'], {
      cwd: root,
      encoding: 'utf8',
      maxBuffer: 8 * 1024 * 1024,
      stdio: ['ignore', 'pipe', 'ignore'],
    });
    const listing = execFileSync('git', ['ls-tree', '-r', '--name-only', 'HEAD'], {
      cwd: root,
      encoding: 'utf8',
      maxBuffer: 8 * 1024 * 1024,
      stdio: ['ignore', 'pipe', 'ignore'],
    });
    tree = new Set(listing.split('\n').filter(Boolean));
  } catch {
    return null;
  }
  const files = {};
  const block = text.match(/FILE_BUDGETS\s*=\s*\{([\s\S]*?)\n\};/);
  if (block) {
    // 条目形如 `  'path/to/file': 1234,`（带行尾注释）。
    for (const line of block[1].split('\n')) {
      const hit = line.match(/'([^']+)'\s*:\s*(\d+)/);
      if (hit) files[hit[1]] = Number(hit[2]);
    }
  }
  const total = text.match(/TOTAL_BUDGET\s*=\s*(\d+)/);
  if (!total) return null;
  return { files, total: Number(total[1]), tree };
}

/** 生产代码行数预算：文件 → 上限。包含注释以外的所有代码行。 */
const FILE_BUDGETS = {
  // 2960 → 2980：日志分类侧栏改版——分组标题 accent 色条 + 分组间虚线 +
  // 文件名单色等宽字体 + 暗色滚动条 + .log-tabs 撑满行高修复滚动 + 选中态
  // 通过 :has() 给所在分组高亮色条（~+20 行）。视觉层次本来就在共用样式
  // 文件里凑着放——没必要为单个 panel 拆 CSS 文件，徒增 @import 链。
  // 2980 → 3080：侧栏改版继续追加的分组/滚动条等样式（工作区实测 3057，
  // 仍在迭代）。共享主题文件按约定不拆分，涨数字让其可见。
  // 3080 → 3160：插件中心改行式枚举（见下方 TOTAL_BUDGET 处的说明）：
  // 新增搜索框工具条、分类 chip 单行横滚、结果计数行与 .catalog-row
  // 行式条目样式；旧 .catalog-card / .catalog-card-head / .catalog-title /
  // .catalog-card-foot / .catalog-tags 随之删除，净 +62。
  // 3160 → 3225：通知卡「最近完成」列表样式（.notify-items / .notify-item /
  // 问答角色徽标，~+62）。
  'ui/src/theme.css': 3225,
  // P4 step 3：物化路径切到实例 extensions/plugins/<id>/，抽出 materialize_inner
  // 共享逻辑、新增 materialize_one_for_instance / remove_materialized_for_instance
  // / sweep_instance_orphans / default_instance_key / seed_default_instance_for_tests
  // 等 helper + is_managed_spec 兼容新旧两种路径模式。净增约 30 行。
  // P4 step 4：4 条实例范围顶层函数 install_for_instance / update_for_instance /
  // uninstall_for_instance / set_mode_for_instance / sync_for_instance /
  // status_for_instance + sync_kernels_for_instance / ensure_wiring_for_instance
  // 共约 +235 行（install_unlocked / uninstall_unlocked / set_mode_unlocked /
  // update_unlocked / sync_all_unlocked 接收 family/instance_id 形参，
  // 旧 API 委托到新 API，加上 step 3 注释 / 测试 setup helper 的尾段）。
  // 物化/卸载/同步/状态/模式切换是同一概念的同步代码，留在同一文件比
  // 拆出去更易维护。
  'src-tauri/src/plugins.rs': 2980,
  // P4 step 4：4 条实例范围 Tauri 命令 plugin_install_instance /
  // plugin_uninstall_instance / plugin_sync_instance / plugin_status_instance
  // + 共享 run_plugin_command_instance 主体，约 +75 行。
  // B 类日志 family/instance_id 接入：install_version / GuardDeps literal /
  // kernel_workbench_url_from_log / diagnose_runtime 等 6+ caller 加 family +
  // instance_id 形参（约 +28 行）。每个 caller 都同时给默认实例硬编码
  // (DSH, "default")——P8 UI 决策后由真实 instance_id 替换。
  // 工作台外链修复（commit ...）：open_harness 给 harness webview 注册
  // on_new_window 处理器。内核前端把会话内容里的网页地址渲染成
  // target="_blank"，而 Tauri webview 默认拒绝一切 window.open 请求
  // （wry 无 handler 时 macOS 取消导航 / Windows 标记已处理），点击会
  // 静默失效；处理器把 http(s) 外链交给系统默认浏览器（~+15 行）。
  // 1900 → 1910。
  // 1910 → 1915：三个查看器窗口建出后调用 window::snap_to_main 做吸附校正
  // （外框高度对齐主壳 + 补偿 Windows 不可见缩放边框贴紧主窗）。
  // 安装预检：2 条 Tauri 命令（plugin_precheck_install /
  // plugin_set_precheck）+ 共享 run_precheck_command + merge_settings 新增
  // 预检开关继承 + 对应断言，约 +71 行。预检的事务主体刻意不放这里——
  // 见新文件 precheck.rs。
  // 1990 → 2050：安全网 P0 的 8 个打点。命令层只放「调度」：
  // record_pre_change / run_plugin_mutation_command / snapshot_list 三条
  // 共用路径 + start_kernel 的 startup-ok 钩子 + activate_version 的打点
  // 位置。指纹、存储、裁剪全在 snapshot.rs。
  // 2050 → 2110：P1 的两条恢复命令（snapshot_preview_restore /
  // snapshot_restore）。**刻意拆成两条**——预览与执行分开，用户才可能
  // 先看见将要失去什么再点确认。
  // 2110 → 2220 是错的：反棘轮把二分命令壳逼了出来，改记进 bisect_cmd.rs，
  // 本文件回到基线 2110。**不下调**——它仍是全项目第二大的文件，下一步
  // 该做的是把别的东西拆出去，而不是假装它已经很小了。
  'src-tauri/src/commands.rs': 2110,
  // 安全网 P0 + P1：环境快照。指纹计算（可重建的声明而非备份）、快照文档
  // 读写（走 state.rs 骨架）、裁剪策略（高权重优先 + 永不丢 last-known-good）、
  // 两个打点的入栈规则、给面板的只读视图，以及 P1 的差异计算与恢复执行
  // （只改差异项 + 先备份 + 不删数据 + 动不了的照实报）。
  // **不含**二分定位——那是 P2。
  // P1 追加只读视图与打点。**记录**（指纹 / 快照文档 / 裁剪 / 打点）与**执行**
  // （差异计算 / 恢复落地）拆成了 snapshot.rs + restore.rs：读前者的人关心
  // "这份回退点可不可信"，读后者的人关心"点确认之后会发生什么"。
  'src-tauri/src/snapshot.rs': 590,
  // 差异计算与环境恢复：逐条比对、只改差异项、只停用不卸载、恢复后用
  // verify::probe_once 自检。
  'src-tauri/src/restore.rs': 560,
  // P0 + P1 的前端状态与展示函数：打点原因中文化、摘要拼装、空状态的三句
  // 话，以及恢复预览的维度标签 / 标题 / 动不了的条数提示。
  // 145 → 155：P2 追加 outcomeHeadline。
  // 155 → 180：`verificationView` 三态（verified / failed / not-needed）从
  // outcomeHeadline 里独立出来——三态各自带配色与整句文案，塞回一个函数
  // 只会让它同时负责"怎么说"和"算什么颜色"。
  'ui/src/snapshots.js': 180,
  // P1 的恢复确认弹窗：把「将要失去什么」和「动不了什么」分成两栏列出。
  // 200 → 215：实测告警改由 `alert` computed 决定，需要"有没能完成的条目
  // 就降级成 warning"这条交叉规则，以及失败原因的第二行展示。
  'ui/src/components/SnapshotRestoreDialog.vue': 215,
  // P2 二分定位的会话与步骤记录。与判定逻辑分开：这里只管"试了什么、
  // 结果如何、排除了谁"，怎么试由命令层驱动 verify::probe_once。
  // 收尾只有三种取值，**没有"找到根因"**——组合效应会让二分停在不可修
  // 的答案上。
  'src-tauri/src/bisect.rs': 350,
  // 二分定位的 Tauri 命令壳。**被反棘轮逼出来的**：四条命令加 rounds_estimate
  // 原本要进 commands.rs，而那条规则不许把一个 2110 行的文件继续撑大。
  // 这组命令只服务二分一件事、内部高度耦合，自成模块也确实更清楚。
  // 120 → 140：每轮真正装进沙盒的插件白名单 + 工作台运行态守卫。
  'src-tauri/src/bisect_cmd.rs': 140,
  // 「起一次沙盒内核看它起不来」的共享判据：P1 恢复后自检与 P2 二分试探
  // 共用。判据一旦有两份实现就会分叉，而分叉出来的那个会让二分**静默收敛
  // 到错误答案**——它把"没试成"当成"起来了"。
  // 90 → 120：**把被测配置装进沙盒**这一步。少了它，这个函数起的是
  // 零插件零技能的裸内核，等于在另一个问题上做判定。
  'src-tauri/src/verify.rs': 120,
  // P2 的前端状态与展示函数：结局映射（无"根因"）、轮数估算、每轮标题与配色。
  // 110 → 175：`startBisect` 现在**真的把每一轮跑完**（循环 invoke
  // bisect_probe），外加 abort 的错误提示与刷新。只发起不驱动的话，面板会
  // 永远停在"排查进行中 … 请耐心等"。
  'ui/src/bisect.js': 175,
  // P2 的排查面板：逐轮显示"在试哪一半 / 上一轮结果 / 已排除几个 / 还要几轮"。
  'ui/src/components/BisectPanel.vue': 180,
  // P0 的概览页卡片。刻意**只读**：提前放"一键回退"会让用户在没看清
  // 差异的情况下丢配置。
  'ui/src/components/SnapshotCard.vue': 170,
  // 安装预检的沙盒生命周期共享层：一次性实例 id / 端口分配 / 目录骨架、
  // 适配器启动与就绪看护、环回 HTTP 存活确认、日志标记扫描、残留回收、
  // 证据另存，以及 Verdict / PrecheckReport 两个对外类型。
  // **刻意与「装什么」无关**：技能预检将来直接复用同一套起停与探测。
  'src-tauri/src/sandbox.rs': 560,
  // 安装预检的两段式事务：中央库字节级快照与回滚、基线差分判定、
  // 提交（物化 + 接线）与报告装配。放在独立文件而不是塞进已 2964 行的
  // plugins.rs，是为了两件事：插件模块读不懂、预检想复用到技能上也
  // 无从下手。plugins.rs 侧只暴露 `store_file` 一条可见性缝。
  'src-tauri/src/precheck.rs': 450,
  // P6 step 2+3+4：迁移向导后端——ConflictPolicy / MigrationStatus /
  // MigrationItemReport / MigrationReport / run_migration / migrate_one /
  // decide_entry / backup_existing / copy_one / copy_tree_inner +
  // RollbackStatus / RollbackItemReport / RollbackReport / rollback_migration
  // / restore_directory / find_backup_root + 12 个新测试，约 +600 行。
  // copy_tree_inner 与 plugins.rs / skills.rs 的 copy_tree 是已知重复；
  // AGENTS.md §3 要求提共享层到 pkg.rs，留到独立重构处理。
  // 740 → 800：store.json 清单按 id 合并（EntryDecision::MergeManifest +
  // is_store_manifest + merge_store_manifest）+ 合并回归测试 + 共享 env
  // 守卫 scoped_xlink_home_unset 的调用方改造。合并语义是修复「目标清单
  // 被泄漏数据顶新导致源记录永远迁不进来」的根因，逻辑必须留在迁移层。
  'src-tauri/src/migration.rs': 830,
  'src-tauri/src/skills.rs': 1490,
  // 「被更高优先级的根盖住」的处置：把盖住的那几份**改名让路**（只改名不删）。
  // 独立成文件的两条理由：① skills.rs 只剩 10 行余量，这条能力（判据 + 落点
  // 复核 + 改名 + 4 个测试）放进去只能靠调数字过门禁，而它是 1480 行的大文件；
  // ② 判据本身在 paths.rs（纯函数，只读），执行在这里——**读判据的人**与
  // **动手的人**分开，与 snapshot.rs / restore.rs 那对「回退点可信吗」/
  // 「点确认会发生什么」是同一种分法。66 行代码（+200 行含文档与测试）。
  'src-tauri/src/skill_shadow.rs': 70,
  'src-tauri/src/patches.rs': 1250,
  // B 类日志 family/instance_id 接入：kernel_log_spec / install_log_spec /
  // current_kernel_log_path / install_version / install_version_into 加形参；
  // attach_log_drainers 改用 family + id。start() legacy 单实例路径硬编码
  // (DSH, "default")——P8 UI 决策后由真实 instance_id 替换。约 +11 行。
  // 1290 → 1380：内核安装依赖锁步对账（scan_dsh_version_skew /
  // write_kernel_workspace_yaml / write_kernel_stub / 安装二遍钉版）。
  // 2026-09-29：这三个函数连同「上游漏发精确钉版时降级重试」一起搬进
  // kernel_deps.rs，kernel.rs 同步从 744 回到 721 行；预算数字不动。
  'src-tauri/src/kernel.rs': 1380,
  // 2026-09-29 新增（265 行代码，其余是解释这次事故的文档）。它回答
  // 「内核安装时，每个官方子包究竟装哪个版本」这一个关注点：写 stub /
  // pnpm-workspace.yaml 的 overrides、装完扫锁步错位、以及 pnpm 报
  // ERR_PNPM_NO_MATCHING_VERSION 时的降级兜底（2026-09-29 实测内核
  // 0.2.0-rc.2 依赖的 @deepseek-ai/dsh-client-ui-settings-account@0.2.0-rc.2
  // 在官方与镜像 registry 上都不存在，pnpm 直接拒绝解析，整条安装失败）。
  // 三者共用同一份 overrides 数据，拆开只会让「谁在决定钉版」变得看不懂；
  // 而它们都不属于 kernel.rs 的「起进程 / 判成败」，继续堆在那里只会推高
  // 一个只许下调的文件的实际行数。
  'src-tauri/src/kernel_deps.rs': 300,
  // 2026-09-29 新增（27 行代码，其余是解释「为什么 eprintln 不算数」的文档）。
  // 壳是 GUI 应用，stderr 在 Windows 上没有任何去处，而**只有后果、没有原因**
  // 的动作恰恰只走 stderr（工作台窗口自动重载、给 pnpm 降优先级）。这个模块
  // 给它们一个落盘出口：沿用按日轮转的约定，因此自动出现在「查看日志」面板。
  // 单独成文件而不是塞进 process.rs，是因为 process.rs 只剩 9 行预算，且
  // 「事件落盘」与「子进程执行」是两个关注点。
  'src-tauri/src/shell_events.rs': 80,
  // 2026-09-29 新增（36 行代码，其余是这次事故的取证结论）。装内核时 pnpm
  // 硬链接数万文件 + node-gyp 编译，把 CPU 与磁盘打满，同机另一个壳的
  // WebView2 渲染进程被打崩，表现为工作台莫名 reload 并撞上内核的启动顺序
  // 竞态。把**装包工具**降到 BELOW_NORMAL 让它们让路，保留双壳并行调试——
  // 比「另一个壳的工作台在跑就禁止装内核」代价小得多。只降 pnpm/npm/npx：
  // Node 探针降优先级会误报「原生模块加载失败」，那是更糟的假阴性。
  'src-tauri/src/child_priority.rs': 80,
  'src-tauri/src/process.rs': 1180,
  // 1050 → 1090：会话标题改为订阅 `session/control`（baseline 播种 + 标题投影帧
  // 保鲜 + 老内核退回 session/list 快照），这部分逻辑与 Center 同生共死，拆出去
  // 只会把状态机切成两半。详见 docs/notification-design.md §3.3。
  // 1090 → 1170：通知带「最近一轮对话」——Center 增 `last_turns` 表
  // （turnOutline 投影末项；baseline / 投影帧 / session/list / api-session/added
  // 四个来源共用一套吸收逻辑）+ CompletedTask 增 lastPrompt / lastResponse +
  // 通知正文带最近对话与「完成于 HH:MM」。与标题同源同命，不拆分。
  'src-tauri/src/notify.rs': 1170,
  // 2026-09-28 新增：系统通知通道的可用性判定（Windows「设置 → 系统 → 通知」
  // 被关时 `ToastNotifier::Show` 仍返回 S_OK 而气泡不出现，macOS 未打包构建
  // 同样投递不出）。单独成文件而不是并进 notify.rs，是因为它与状态机无关——
  // notify.rs 只回答"要不要弹"，这里只回答"平台这一关卡没卡住"；混在一起会
  // 让一个 1170 行、只许下调的文件再多一处平台分支。约 56 行代码（其余是解释
  // 这两种静默失败的文档）。
  'src-tauri/src/notify_gate.rs': 120,
  // 2026-09-29 新增：点系统通知横幅「回到工作台」的单实例交接。未打包应用的
  // 通知被点中时，Windows 启动的是本 exe 而不是"叫醒"已运行的窗口；第二个
  // 进程若照常走完 setup 会 reap_orphans 杀掉在跑的内核。独立成文件而不是
  // 并进 notify.rs：它与"要不要弹这条通知"的状态机完全无关，关心它的是启动
  // 顺序（lib.rs 的 run() 第一行）与窗口行为（commands::open_harness），
  // 放进去会让一个 1170 行、只许下调的文件多出一处平台分支。
  // 260 → 280：补上跨进程抢前台（AttachThreadInput + BringWindowToTop +
  // SetForegroundWindow）。这是实测逼出来的：只做管道交接时窗口会被拉回
  // 可见，却抢不到前台——Windows 的前台锁对非前台进程静默失败，用户看到的
  // 仍然是"点了没反应"。
  'src-tauri/src/activate.rs': 280,
  // 2026-09-30：工作台窗口的**手动**逃生口（`harness_force_reload`）。看门狗
  // 只在加载事件上判据，而 WebView2 渲染进程被打崩时页面早就 `Finished` 过，
  // 那条判据永远不会触发，`reload()` 又落在一块死掉的文档上（用户原话：
  // 「reload 后，还是会黑屏」）。这条命令换掉整个窗口——壳侧唯一能换掉渲染
  // 进程的动作。**被反棘轮逼出来的**：`commands.rs` 2090/2110，放进去只能
  // 上调数字，而调数字是 AGENTS.md 明确禁止的反应；`bisect_cmd.rs` /
  // `home_recovery_cmd.rs` 是同一处理由。生产代码约 40 行 + 两个把拒绝文案
  // 钉死的单测（拒绝路径的「说法」是用户在故障里唯一能拿到的东西）。重建
  // 动作复用 `harness_window::recreate`，没有复制第二条建窗链。
  'src-tauri/src/harness_cmd.rs': 90,
  // 工作台窗口的加载看门狗。从 commands.rs 拆出来的原因不是「行数超标」这么
  // 表面：它和命令注册、启动看护、状态轮询都不是一回事——它观测的是 **webview
  // 自己**的加载事件，判据完全独立于页面（页面没加载出来时，注入页面里的
  // harness-health.js 同样没跑，是哑的），因此壳在那种故障下唯一拿得到信号的
  // 地方就是这里。放进 commands.rs 会和一堆命令样板混在一起，看不出它为什么
  // 独立、也钉不住它的三条自律。约 100 行。
  // 2026-09-30：`recreate` 收下 `reason`（自动重建与用户手动「刷新工作台」走同一
  // 动作但成因不同，日志里把手动说成「自动」会让排查顺着一件没发生过的事去找）
  // + 新增 `reset_budget` 供手动路径清额度。
  // 120 → 190（**临时登记，不是「就这样了」**）：本文件被一条并发的跨壳工作台
  // 改造顶到了 175 行（新增 `instance::workbench_running_in_other_shell` 的那条
  // 跨壳判定与事件落盘），本次只加了 10 行（`recreate` 的 `reason` 参数、
  // `reset_budget`、证据前缀修正）。本文件基线 120 远低于 RATCHET_THRESHOLD，
  // 上调在规则内，但**那次改造落地时应把超出的部分拆出去或写清为什么该独立**，
  // 届时把数字收回 120 附近。拆分方案不在本次范围内——本次只加一个按钮。
  'src-tauri/src/harness_window.rs': 190,
  'src-tauri/src/guard.rs': 940,
  // 从 guard.rs 拆出的「证据判读」层：只回答「这一行指向内核还是指向某个插件」，
  // 不回答「该怎么处置」。独立成文件有两个理由：① 判据的内核侧（命名空间锚定 +
  // 多成员组合路由的拒绝规则）与插件侧（bundle_member / has_segment_path）必须
  // 并排可读——把两套相反的边界规则隔着一个 900 行的文件，正是当初它们被写成
  // 同一个宽进出的来源；② guard.rs 是只许下调的反棘轮文件，而 2026-09-29 新增的
  // 内核槽位装配不变量判据必须落在它够得着的地方。60 行，离 800 行硬顶很远。
  'src-tauri/src/kernel_evidence.rs': 80,
  // 2026-09-29：插件中央库的一次性目录搬迁（`dsh-plugins/` → `plugins/dsh[-dev]/`）
  // 从 plugins.rs 拆出。拆的理由有两条，都不是「文件太长了」：① 搬迁只关心旧目录
  // 在不在、要不要搬、搬失败怎么办，不读 store.json、不碰物化指纹——中央库的条目
  // 逻辑在 plugins.rs，目录级的一次性动作在这里，两者是不同层；② plugins.rs 是
  // 反棘轮文件（只许下调），把新逻辑留在那里只能靠调数字过门禁，而调数字正是
  // AGENTS.md 禁止的反应。生产代码约 75 行。
  'src-tauri/src/store_relocate.rs': 120,
  // 2026-09-29：「搬错实例」的遗留数据回收（sessions / attachments）。2026-09-28
  // dev 壳在闸门落地前 15 分钟把 `~/.dsh` 的历史会话并进了 default-dev，release
  // 侧工作台从此是空列表，而 `~/.dsh` 已空、搬不动第二次——instance.rs 只写了
  // 「防再犯」的闸门，没有「已犯怎么办」的回收路径。独立成文件的理由同上：扫描 /
  // 复制 / 合并的主体不碰实例注册表与生命周期，留在 instance.rs 只会顶高那条
  // 540 行的预算。生产代码 185 → 408：多出来的是 `workspace.json` 的合并——会话
  // 列表由它决定，只搬目录的话文件在磁盘上、工作台里仍然不显示（2026-09-29 本机
  // 实测）。408 行离 800 行硬顶还有一半距离。
  'src-tauri/src/home_recovery.rs': 430,
  // 同一条回收路径的 Tauri 命令壳（scan / recover 两条）。与 commands.rs 其余
  // 70 多条命令没有共享逻辑，留在那里只能靠上调数字过反棘轮——`bisect_cmd.rs`
  // 是同一处理由。生产代码约 30 行。
  'src-tauri/src/home_recovery_cmd.rs': 60,
  // 2026-09-29：实例注册表按壳模式**分文件**的一次性拆分（`state/instances.json`
  // / `state/instances-dev.json`）。之前两个壳共用一个文件，于是：跨进程读-改-写
  // 没有任何序列化（互斥只是进程级 Mutex）、dev 壳删/建实例会改到 release 的列表、
  // 以及「本壳服务哪个实例」这类指针写在共享文件里。拆开之后这三类跨壳竞态从根上
  // 消失，instance.rs 里原有的「只有 release 能写共享指针」那套防御也随之撤销。
  // 独立成文件有两个理由：① 拆分规则（认领 / 让位 / 顺序无关 / 幂等）是一套独立
  // 的迁移状态机，塞进 instance.rs 会顶高那条已经贴着 540 的预算；② 它只服务
  // 启动期的一次性动作，与实例生命周期（建/启/停/删）无关。生产代码 72 行。
  'src-tauri/src/registry_split.rs': 110,
  // 430 → 450：概览页「刷新工作台」动作 `forceReloadHarnessWindow`（+19）。它与
  // 既有的 `openHarnessWindow` 同属工作台窗口那一组，放这里而不是新开一个
  // `overview.js`：概览页其余 20 来个动作也都在这个文件里，为一个按钮单开
  // 一个共享层才是真分裂。本条预算基线远低于 RATCHET_THRESHOLD，上调是允许的。
  'ui/src/store.js': 450,
  // 多内核改造 P0：新路径模块（paths.rs）。包含 ShellMode、xlink_home、shell
  // /kernels/skills/state/cache 解析、legacy resolver、id 校验与基础数据
  // 模型——是后续 P2–P8 的依赖根，必须单独占预算，避免被 plugins/skills
  // 这两个大文件吞噬。
  'src-tauri/src/paths.rs': 600,
  // P2：实例注册表 + 锁 + 端口分配 + runtime/pid 文件读写 + DSH home
  // 子目录创建 + 默认实例迁移钩子。约 470 行（含 11 个测试 setup 与
  // 路径解析注释）。
  // 2026-09-23：新增内核 home 一次性搬迁（~/.dsh → 实例 DSH_HOME）——递归
  // 并入 / 原子移动 / 跨卷回退 / 幂等标记，~+64 行，470 → 540。
  'src-tauri/src/instance.rs': 540,
  // P3：KernelAdapter trait + AdapterCapabilities + DshAdapter 首实现
  // （DSH_HOME / DSH_PROFILE 注入、profile/package.json 与 cordis.patch.yml
  // 模板、resolve_install_dir 双查找）。约 430 行（含 9 个测试）。
  'src-tauri/src/kernel_adapter.rs': 620,
  // 模型用量统计（usage.rs）：内核 session 多帧 zstd 流的增量扫描（ruzstd
  // 帧级解码 + offset checkpoint + 坏帧停驻）、按「天 × 模型」预聚合与 90 天
  // 保留剪枝、汇总视图派生与 get_model_usage / open_usage_window 命令，
  // 外加主窗拖动吸附跟随（dock_x / dock_y / docked_position / 事件监听）。
  // 扫描是唯一数据通路，与账目结构同生共死，不宜再拆。生产代码约 665 行
  // （测试另计）。
  'src-tauri/src/usage.rs': 700,
  // 模型用量统计（usage.js）：窗口/卡片状态动作 + B/M/K 单位、热力图分级
  // 与周列对齐、趋势堆叠、饼图扇区等纯展示函数（node --test 直测）。
  // 160 → 180：实际落地比估的多（数字格式、模型配色 ring、热力 tooltip
  // 阈值再加注释）；另含 heatLevels / heatmapColumns 的 doc 注释。
  'ui/src/usage.js': 180,
  // 模型用量统计（UsageWindow.vue）：独立窗口根组件（open_usage_window 弹出，
  // ?usage=1 挂载）——摘要卡 + 热力图 + 堆叠柱状趋势 + 环形图/列表与
  // scoped 样式，全 CSS/内联 SVG 不引图表库。
  // 560 → 700：落地比估的多（环形图 + 列表卡片 + 局部样式），手绘 SVG 段
  // 不可压缩；用法与 LogViewerWindow 同模式（独立窗口根组件 + scoped CSS）。
  // 700 → 780：范围切换 + 时间范围档位（~+50）与热力图/趋势两个共享 hover
  // 明细浮层（~+60）——都是展示层增量，拆文件只会让浮层与图形结构分家。
  'ui/src/UsageWindow.vue': 830,
  // 云端套餐用量（subscription.rs）：MiniMax Token Plan / DeepSeek 余额查询、
  // 5 分钟缓存文档（state.rs 容错读 + 原子写）、Key 指纹绑定（换 Key / 清 Key
  // 作废旧条目）、redact_key 脱敏、三态 Key patch、expired 跳过自动刷新、
  // get_subscription_usage / open_subscription_window 命令（生产代码 877 行，
  // 测试另计；含按 provider 定制的凭据失效文案与失败日志集中记录）。查询 +
  // 缓存 + 解析同生共死，不宜再拆；对应设计稿 docs/subscription-usage-design.md。
  'src-tauri/src/subscription.rs': 900,
  // DSH 模型凭据只读解析（credentials.rs）：profile cordis.patch.yml 的
  // provider apiKeyEnv 绑定、.credentials.yaml refs、.env 回退层与默认引用
  // 派生。凭据语义与内核对齐只有一处实现，独立成模块供 subscription.rs 复用。
  // 260 → 270：refs 标量统一字符串化（YAML 数字写法如 `KEY: 2` 也是合法值）。
  'src-tauri/src/credentials.rs': 270,
  // 云端套餐用量前端（subscription.js）：状态动作（keep-last-good 显式落地）
  // + 收起态摘要 / 余额行 / 进度条配色 / 重置倒计时等纯展示函数（node --test 直测）。
  // 180 → 210：失效 provider 的「提示 → 隐藏 → 查询成功自动恢复」状态机
  // （localStorage 持久 + 会话首查 force + collectErrors 跳过已隐藏项）。
  'ui/src/subscription.js': 210,
  // 套餐用量独立窗口根组件（open_subscription_window 弹出，?subscription=1 挂载）：
  // 双 provider 分区 + 进度条 / 余额行 + 错误横幅与 scoped 样式（同 UsageWindow 模式）。
  // 340 → 360：查询时间换成刷新 icon 胶囊（紧凑年龄值）。
  'ui/src/SubscriptionWindow.vue': 360,
};
/** 全部受检文件的合计预算（Tauri 生产代码 + 前端 js/vue/css）。 */
// 20400 → 20500：技能面板接线「启用 / 停用单个技能」（skill_set_enabled 此前只有
// 后端实现，面板从未调用过：弹层 + 开关 + 动作 + 样式约 30 行），加上归因口径抽到
// ui/src/incidents.js 共享层。两处都是新增能力而非复制粘贴，重复区间数仍为 3。
// 20500 → 20560：下载校验改成 fail-closed 并回退老 packument 的 shasum（sha1）——
// 新增 sha1 回退分支、`strongest_integrity` 抽取与判据注释。安全策略收紧带来的
// 行数是必要的，不该为了卡预算而少写一条校验路径。
// 20560 → 21060：新增 paths.rs（500 行）+ settings.rs 拆分 shell-aware / legacy
// 双入口（≈ 60 行）+ commands.rs 暴露 shell_mode（≈ 5 行）+ store.js 加
// shellMode 计算属性（≈ 4 行）。多内核改造 P0+P1 的最小可用代码量。
// 21060 → 21660：P2 落地 instance 模块（注册表 / 锁 / 端口 / runtime /
// 迁移钩子，约 470 行）+ commands.rs 增加 8 条实例命令与 InstanceSummary
// 类型（≈ 180 行）+ kernel.rs 新增 InstanceStartReport 与
// start_instance / stop_instance / set_instance_active_version 接口
// （≈ 50 行）。路径解析都在 paths.rs，重复区间数仍为 4。
// 21660 → 22160：P3 新增 kernel_adapter.rs（约 430 行）+ kernel.rs
// start_instance 切到 DshAdapter（约 30 行）。所有 DSH 专有路径
// （DSH_HOME / profiles/<name>/ / cordis.patch.yml）现在只出现在
// kernel_adapter.rs，不再散落在 kernel.rs / commands.rs 里。
// 22160 → 22190：P4 step 3 物化路径切到实例 extensions/plugins/<id>/，
// 在 plugins.rs 内新增 materialize_inner / materialize_one_for_instance /
// remove_materialized_for_instance / sweep_instance_orphans 等共享 helper
// （约 +30 行）。物化与清扫是同一概念的同步代码，留在同一文件比拆出去
// 更易维护；FILE_BUDGETS 里 plugins.rs 也对应上调 30 行。
// 22190 → 22500：P4 step 4 新增 6 条实例范围顶层函数（install_for_instance /
// update_for_instance / uninstall_for_instance / set_mode_for_instance /
// sync_for_instance / status_for_instance）+ sync_kernels_for_instance /
// ensure_wiring_for_instance（约 +235 行落在 plugins.rs）；commands.rs
// 增加 4 条实例范围 Tauri 命令 + run_plugin_command_instance 共享主体
// （约 +75 行）。总计 +310 行。
// 22500 → 23070：P6 step 2+3+4 + P7 + P5 step 3 + copy_tree 共享。
// P6 step 2+3+4 新增 migration.rs（≈ 660 行）+ lib.rs ENV_LOCK 拆分 +
// scoped_dsh_home（≈ 60 行新增）+ commands.rs 增加 4 条迁移向导命令
// （约 +35 行）。P5 step 3 KernelAdapter::custom_skill_dirs 接口预留 +
// DshAdapter 返回 paths::skills_active_root() + start 注入
// DSH_CUSTOM_SKILL_DIRS env + ENV_PATH_SEP 常量 + 2 个测试，约 +91 行
// 落在 kernel_adapter.rs。P7 McodeAdapter mock + KERNEL_FAMILY_MCODE
// 常量 + 8 个测试，约 +100 行同样落在 kernel_adapter.rs。copy_tree_inner
// 与 plugins.rs / skills.rs 的 copy_tree 是已知重复——AGENTS.md §3 要求
// 提到 pkg.rs，留到独立重构处理；FILE_BUDGETS 里 kernel_adapter.rs /
// migration.rs / commands.rs 也对应上调。
// B 类日志 family/instance_id 接入全链路（commit 158ced3）:
// commands.rs +28 + kernel.rs +11 + guard.rs（kernel_log_path 重构 +
// GuardDeps 加字段 + diagnose_runtime 加形参）+ kernel_adapter.rs +2 +
// notify.rs +24 ≈ +89 行；测试 fixture 调整不计入生产预算但同样生效。
// 下次 reset 预算时考虑把日志相关 caller 提到单独 helper（参考 AGENTS.md
// §3 要求），避免散落到 5 个 module 的硬编码 (DSH, "default")。
//
// P6 step 5 迁移向导 UI（commit ...）：ui/src/migration.js (~150 行) +
// ui/src/components/MigrationPanel.vue (~250 行) + App.vue / SideBar.vue
// 接入 ~10 行 ≈ +410 行（实际 +260 是因为 UI 行的预算口径不计模板 style
// 块里的 CSS——纯 <template> + <script setup> + state 加 invoke 包装）。
// P8 #1 顶部实例 dropdown（commit 25cd376）：ui/src/instance.js (~50 行)
// + WindowTitleBar.vue 注入 chip + dropdown script/template 块（计入
// theme.css；vue 模板不计入）+ theme.css 实例 chip / menu 样式 ~95 行
// （+15 落在 theme.css 预算边沿）+ document-level click 关闭菜单 +
// aria-haspopup / aria-expanded / aria-current 标注。23500 → 23800。
//
// P8 #2 PluginRow per-instance 视图（commit ...）：plugins.rs 加
// `PluginInstanceState` 结构（materialized / actual_mode / synced /
// wired / quarantined 五个字段）+ PluginRow.instances: BTreeMap<id,
// PluginInstanceState> + `read_profile_json_for_instance` helper +
// status_for_instance 内部循环 instance::load_registry() 各实例（~+65
// 行）+ 3 个回归测试（~+150 行）。plugins.rs 预算 2915 → 2980（+65）。
// 总预算 23800 → 24050（+250，叠加 +65 与给后续 P8 #2 UI 留 buffer）。
//
// 历史数据迁移 UX 改造（commit 37d7d70 / eb2afb8）：migration.rs 加
// `MigrationProgress` 结构 + `run_migration_with_progress<F>` 闭包版 +
// `MigrationSkip` 结构 + is_migration_skipped / set_migration_skipped /
// clear_migration_skipped 三个 helper（~+107 行）。migration.rs 预算
// 670 → 740（+70）。总预算 24050 → 24400（+350 给后续 UI 改造留 buffer）。
//
// dev 复测 4 项 UX 修复（commit ...）：kernel.rs `data_dir` 切到
// `xlink_home + desktop[-dev]/` + 删旧 SHELL_SUBDIR 常量（~+50）；
// instance.rs 加 `ensure_default_registered` sync helper（~+50）。总
// 预算 24400 → 24500（+100 buffer）。
//
// EP 组件按需注册补全（commit ...）：main.js 补注册 12 个缺失组件
//（tabs / table / steps / checkbox / radio / result / progress 及配套
// style import，~+30 行）。这不是样板膨胀——这 12 个组件在模板里
// 已被使用但从未注册，此前被当未知自定义元素静默渲染坏。总预算
// 24500 → 24550（+50 buffer）。
//
// 迁移向导收尾 UX（commit ...）：MigrationSummary.created_at 改 epoch
// 秒字符串 + MigrationPanel 完成 / 紧凑布局 / 报告与历史字段对齐
//（~+30 行）。总预算 24550 → 24600（+50 buffer）。
//
// 路径显示折叠 ~（commit ...）：bridge.js 加 homeDir() 封装 +
// labels.js 加 tildePath 共享助手 + 两个面板接入（~+40 行）。折叠
// 逻辑必须共享一份，各面板自己拼 `~` 会漂移。总预算 24600 → 24650。
//
// 插件页渲染提速（commit ...）：已安装列表骨架屏（view===null 是
// 加载中不是空态）+ ElSkeleton 注册 + 切换动画收紧 + 面板加载并行
// 化（~+20 行）。总预算 24650 → 24700。
//
// 内核安装依赖锁步对账（commit ...）：scan_dsh_version_skew 扫描官方
// 子包版本错位 + write_kernel_workspace_yaml + 安装二遍钉版重装
// （~+90 行）。修「主包与 ^ 浮动依赖混装致启动即崩」，无法再复用
// 既有模块收敛。总预算 24700 → 24900。
//
// 模型用量统计（commit ...）：新增 usage.rs（多帧 zstd 增量扫描 + 90 天
// 「天 × 模型」聚合 + open_usage_window 建窗）、usage.js（展示纯函数）与
// UsageWindow.vue（独立窗口：热力图 / 趋势 / 环形图 + 列表，全 CSS/SVG
// 不引图表库），共约 +1230 行；另有进行中的日志侧栏改版（~+90 行）。
// 展示层全部手绘是为了不加图表库依赖，账目层不可再拆（账目结构即数据
// 通路）。总预算 24900 → 26300。
//
// 26300 → 26500：日志侧栏改版落地——LOG_CATEGORIES / parseLogFilename /
// categorizeLogFile / groupLogFiles（logs.js +~110 行）、共享 LogSidebar 组件
// （52 行）、LogViewerWindow + LogModal 切到 LogSidebar（vue -+120 行）、
// theme.css 加分组色条 + 分隔线 + 单色等宽 + 暗色滚动条（+~80 行）、
// logCategorize.test.js（+110 行）。分类逻辑是后续看日志选文件的工作流
// 入口，值得把判据和边界（instance-aware / 轮转备份 / plugin-wiring 不能
// 被 plugin-* 吃掉）用测试钉住。
// usage.js / UsageWindow.vue 是另一条 in-progress 分支（commit ...）的
// 落地件，本轮还没合并过来，估算偏低；总预算留 100 行余量。
//
// 26500 → 26600：模型用量窗口按反馈迭代——默认高度 768、模型明细前 5 名
// 可见其余列表内滚动（避免整窗纵向滚动条）、热力图 hover 换成带日期 /
// 星期 / 总用量 / 请求次数的明细浮层（usage.js +weekdayLabel，UsageWindow
// +~75 行）。均为展示层增量，不引依赖。
//
// 26600 → 26650：窗口打开吸附主窗右侧（dock_x / dock_y 纯函数 + 单测，
// 贴不下翻左侧、垂直夹屏内），高度与主壳 800 等高（~+45 行）。
//
// 26650 → 26900：用量窗口模型明细放行前 10 名（列表内滚动）、趋势绘图区
// 与各段纵向间距收紧（UsageWindow 内部 height 换 spacing，总量±0）；
// theme.css 的日志侧栏样式继续追加（+~77）按上文条目单列。留 ~35 行余量。
//
// 26900 → 26950：PaneSplitter 拖拽体验优化——pointer events +
// setPointerCapture（光标跑出元素不丢事件）+ requestAnimationFrame 节流
// （1kHz mousemove 收敛到 60Hz）+ 拖拽期间关掉 .log-tabs 的 width transition
// （0.2s 缓动在 60Hz 拖拽时会不断重启动画）+ localStorage 持久化延迟到
// drag-end（每帧同步 I/O 是卡顿主因）。PaneSplitter.vue 重写、LogModal /
// LogViewerWindow 增加 onSidebarDragEnd 处理、theme.css 加 body.pane-dragging
// 规则（~+50 行）。三个叠加来源合并治，否则只改一处仍有可见抖动。
//
// 26950 → 27080：先用 PaneSplitter 三处合治拿下拖拽卡顿；同步合并同
// 日提交（ac1a6aa 摘要区「今日用量」瓦片，+7）。本分支：日志查看器
// 接入用量窗口同源的吸附 + 移动跟随——抽出 window.rs（dock_x / dock_y /
// compute_dock_position / dock_position_logical / attach_dock_listener，
// ~+141 行），usage.rs 删除已搬走的私有 dock_* 与 attach_usage_dock_listener
//（~-150 行），commands.rs::open_log_window 增加吸附定位 + LOG_WINDOW_* 常量
//（+~20），lib.rs 双挂 attach_dock_listener（usage-viewer 760×800 +
// log-viewer 960×720，+~10）；净 ~+22 行，与上述 +7 合计 ~+30，预算上调
// 130 是给后续「用量窗口换库 / 日志联动」一类连续作业预留余量。
// 云端套餐用量（commit ...）：subscription.rs（双 provider 云端查询 + 实例级缓存 +
// 凭据指纹绑定 + 实例/profile 绑定 + 建窗，717 行）、credentials.rs（内核模型凭据
// 只读解析：profile apiKeyEnv / .credentials.yaml / .env，243 行）、subscription.js
// （keep-last-good 状态 + 展示纯函数，152 行）、SubscriptionWindow.vue（独立窗口，
// 313 行）、设置页凭据状态卡（+130）、概览套餐用量行与展开卡（+170）、labels.js
// 时间标签（+33）、paths/permissions/capability 接线（+40）。功能按设计稿
// docs/subscription-usage-design.md 落地：凭据复用内核模型设置（外壳不收集 Key）、
// 每 provider 独立状态、缓存按实例隔离，均为安全边界，无法复用既有模块。
// 27300 → 28900。概览页「套餐用量」从行内入口改为独立卡直接展示（无折叠）、
// provider 分块描边分割、DeepSeek 提到首位、tier 短名 5h/7d 与 ♾️ 无限周层、
// 倒计时紧凑化（Timer icon + 1d15h）、按 provider 定制 401 文案——均为真机
// 反馈的展示与功能迭代。28930 → 29000。
// 倒计时紧凑化 + 刷新年龄胶囊（<1min / 12min / 3h）迭代 ~+50。
// 套餐类 provider 改左右自适应栅格（余额类单独成块；上轮样式替换未命中，
// 本轮真正落地 grid + 窄块 tier 形态），~+60。
// 概览卡分区标题旁的刷新 icon 支持按 provider 单独刷新：subscription.js 加
// 单分区合并与在途去重（Rust 按 provider 查询只返回该分区），OverviewPanel
// 年龄胶囊改为可点按钮（加载态旋转），29140 → 29210。
//
// 29210 → 29450：插件中心搜索枚举界面（PluginsPanel.vue / plugins.js / theme.css）。
// 起因是一处**功能缺失**而非体积超标：pluginStore.query 早就有状态、有
// filteredCatalog 过滤、有 150ms 防抖 watcher，但模板里从没有任何输入框
// 绑定它——16034 条目录只能靠 10 个分类 chip 一页页翻。本轮补齐并按真机
// 反馈重排枚举面：
//   plugins.js（+53）：相关度排序 matchScore（名称前缀 3 > 名称中段 2 >
//   其余字段 1，0 不命中；原先只有 haystack.includes 的二值命中，搜 agent
//   会把「描述里提了一句 agent」的条目排在「名字就叫 agent」的前面）、
//   matchParts 高亮分段、hasActiveFilter / resetCatalogFilters，
//   CATALOG_PAGE 60 → 24（60 条/次 ≈ 5000px 滚动，点完「显示更多」就找不到
//   自己刚看到哪了）。
//   PluginsPanel.vue（+84）：补搜索框、搜索框占主位 + 排序收同行的工具条、
//   分类 chip 改单行横向滚动（10 枚 chip 换行占三行，把首屏结果挤出屏幕）、
//   结果计数行（匹配 N / 总数）与「清除筛选」、目录条目由三段式卡片
//   （约 90px，一屏 3-4 个）改定高三行的行式条目（约 62px）。结构上未动
//   「已安装 / 当前内核」双 tab 与任何 Rust 命令。
//   theme.css（+64）：行式条目 + 工具条 + chip 横滚 + 计数行样式；删掉
//   .catalog-card 系列后仍为净增。共 +173，预算上调 240 留余量。
//
// 29450 → 29520：三处真机（Windows）反馈修复。window.rs 吸附几何抽
// snap_to_main / dock_insets / inner_height_matched_to（建出后按实测外框
// 校正：外框高度对齐主壳 + 补偿 Windows 不可见缩放边框，三类查看器窗口
// 共用）；subscription.js 增加失效 provider 的「提示 → 隐藏 → 查询成功自动
// 恢复」状态机（localStorage 持久 + 会话首查 force）；OverviewPanel.vue
// 弹确认框 + 隐藏过滤 + 进度条 flex 塌陷修复（WebView2 上纵向 flex 的
// flex-basis 0% 把 .plan-bar 压成 0 高）。均为新增能力而非复制粘贴，
// 重复区间数仍为 6。
// 29520 → 29580：迁移弹窗重复提示修复。migration.rs 加
// `MigrationSkipReason` 枚举 + `MigrationSkip.reason` 审计字段 +
// `run_migration_with_progress` 全部成功后自动写静音标记（~+40 含测试）；
// migration.js 的 maybeOpenMigrationPrompt 加迁移历史兜底（+3）。
// migration.rs 预算 800 → 830，总预算 +60 留余量。
// 29580 → 29760：任务完成通知带「最近一轮对话 + 完成时间」。notify.rs
// 增 last_turns 表（turnOutline 投影末项，四个来源共用一套吸收逻辑）与
// CompletedTask.lastPrompt / lastResponse（+73）；theme.css 通知卡最近完成
// 列表样式（+62）；notifications.js 增问 / 答字段与时间 / 时长格式化
// （+42）；SettingsPanel 增最近完成列表（+30）。均为新增能力，
// 重复区间数仍为 6。
// 29760 → 30630：插件安装沙盒预检。净增约 870 行，几乎全部落在**两个新
// 文件**里，这是本条预算第一次因为「开新文件」而不是「撑大老文件」而
// 上调：
//   · sandbox.rs（新增 ~533 行）：一次性沙盒实例的 id / 端口 / 目录、适配
//     器启动与就绪看护、环回 HTTP 探测、日志标记扫描、残留回收、报告类型。
//   · precheck.rs（新增 ~446 行）：字节级快照回滚、基线差分判定、提交。
//   · commands.rs（1915 → 1990）：2 条命令 + 共享 runner + 合并断言 +71。
//   · PrecheckDialog.vue（新增 ~150 行）+ theme.css（+19）开关行 +
//     plugins.js / store.js / progress.js 前端接线（~+40）。
// plugins.rs **未上调**（仍 2964 / 2980）：预检的事务主体一开始写进去会
// +215，改为抽成 precheck.rs 后回到原预算。重复区间数仍为 7。
//
// ⚠ 这次上调让「总量只许因删除而下调」这条自我约束第一次被破例。真正的
// 修法是把总量门禁换成「单文件上限 + 新增能力必须开新文件」两条，后者在
// 本次靠「plugins.rs 未上调」兑现了一半，但规则本身还没进脚本。
// 31280 → 31960：安全网 P1（手动恢复）。净增约 680 行：
//   · snapshot.rs 580 → 655：差异计算（内核 / 插件集与模式 / 技能启用位 /
//     补丁，只报不动）+ 恢复执行（先备份 pre-restore、逐条落地、单条失败
//     不打断其余、动不了的进 skipped）。
//   · SnapshotRestoreDialog.vue（新增 ~164）+ snapshots.js 增 P1 展示函数。
//   · commands.rs（2050 → 2101）：两条恢复命令。skills.rs / patches.rs /
//     plugins.rs 各开一条只读或窄缝（合计 +20）。
// plugins.rs / guard.rs / kernel.rs **未上调**。
//
// ⚠ 连续第三次上调总量。P0 / P1 都遵守「新能力开新文件、不撑大老文件」，
// 但总量门禁本身仍然只会在新功能面前让步——真正该做的是把它换成
// 「单文件上限 + 新增能力必须开新文件」两条规则，那条规则还没进脚本。
// --- 总量 --------------------------------------------------------------
//
// 刻意**不**在这里放"总量只许下调"那条规则。试过，它不可行：它要求每个新
// 功能都伴随一次等量的删除/重构，实践里只会逼人绕过门禁（把常量写大、或者
// 找理由多开几个小文件），而不是真的写出更少的代码。
//
// 真正防住膨胀的是另外两条，它们都机械可查：
//   1. 既有文件的预算只许**下调**——老模块只能越来越小（上面那条反棘轮）；
//   2. **新文件**必须显式登记且受 800 行硬顶——下一个 plugins.rs 不能在诞生
//      时就超标。
// 总量只留一道软上限：它是"本版允许的最大规模"，随新能力一起调整，但
// 每次上调都要求在这个数字旁边写清"这一版多了什么、为什么该独立成文件"。
// 32900 → 33050：这一版多出来的 43 行全部是**修复**而非新能力——
// `verify.rs` 里"把被测配置装进沙盒"的那一步、`bisect.js` 里把每一轮
// 真正驱动完的循环、`snapshots.js` 里实测三态的独立呈现。三个文件都还在
// 各自的 200 行以内，离 800 行硬顶很远。
// 33050 → 33120：工作台事故证据可读性。同一条链上补的两处**诊断盲点**，都
// 不是新能力而是"前端证据本来就在、壳没接住"：`harness-health.js` 把内核
// 客户端模块 bundle 的 `<script>` 加载失败当成无害资源错误丢掉（41 行），
// 事故面板把证据拼成一个大 `<pre>`、组合路由那行几十个包名的地址没人读得
// 动（`incidents.js` 的分段 + 面板渲染，共约 70 行，落在既有的 157 行共享层
// 里、没有新开文件）。没有这两处，切内核后前端崩了既归因不到具体包名，用户
// 也只能拿到「未定位到包名」这句话。
// 33120 → 33660：目录搬迁收尾 + 「搬错实例」的会话回收。
// ① store_relocate.rs（~75 行）：插件中央库 `dsh-plugins/` → `plugins/dsh[-dev]/`
//    的一次性搬迁从 plugins.rs 拆出。**不是净增**——plugins.rs 同期从 3027 回到
//    2957（反棘轮基线 2980），这一进一出才是它该有的形状：新逻辑留在只许下调的
//    大文件里，唯一能过的门禁是调数字，而调数字正是 AGENTS.md 禁止的反应。
// ② home_recovery.rs（408）+ home_recovery_cmd.rs（30）+ 前端卡片与 store（~75）：
//    2026-09-28 dev 壳在闸门落地前 15 分钟把 `~/.dsh` 的历史会话并进了
//    default-dev，release 侧工作台从此是空列表，而 `~/.dsh` 已空、搬不动第二次。
//    instance.rs 只写了「防再犯」的闸门（legacy_migration_target），没有「已经
//    犯了呢」的回收路径——用户因此既看不见自己的数据，也没有产品内的出路。
//    回收要搬两样：会话目录（复制）与 `storages/workspace.json`（**合并**——
//    工作台的会话列表读的是它，只搬目录的话文件在磁盘上却永远不显示，2026-09-29
//    本机实测）。第二样是这次多出来的 ~220 行，规则是：只登记本实例确实有会话
//    目录的 id、同一路径以目标为准只补缺、目标文件损坏时绝不用空骨架覆盖、
//    写盘走 atomic_write、且要求工作台已停止（内核内存缓存会覆盖清单）。
//    commands.rs 与 plugins.rs 同期都回到基线以下（新逻辑一律拆出去）。
// ③ registry_split.rs（72）+ instance.rs / paths.rs / lib.rs 的接线（~25）：
//    实例注册表按壳模式分文件。此前两个壳共用 `state/instances.json`，而互斥只
//    到进程级——跨进程的读-改-写没有任何序列化，dev 壳删一个实例会改到 release
//    的列表，「本壳服务哪个实例」的指针也写在共享文件里。拆成
//    `instances.json` / `instances-dev.json` 之后这三类跨壳竞态从根上消失，
//    instance.rs 里「只有 release 能写共享指针」那条防御也随之撤销（谁写都只
//    写自己那份文件）。这次多出来的净增全部在「隔离」上，没有一行是功能。
// 33720 → 33960：kernel_deps.rs（265 行）。内核 0.2.0-rc.2 的锁步依赖里有一条
// 断边——`@deepseek-ai/dsh-client-ui-settings-account@0.2.0-rc.2` 被
// `dsh-web-app` 精确钉住，而它在官方与镜像 registry 上都没发布，pnpm 在解析
// 阶段就拒绝整棵依赖树，用户装不上内核（2026-09-29 实测）。壳的处理是失败后
// 查一次 registry、给断边各选一个同版本线的较低版本钉进 overrides 再重试，并
// 在进度面板与最终总结里明说降级了什么。选版规则（只退到同 major.minor 的较低
// 版本）、pnpm 报错解析与三条失败文案都带测试。同期 kernel.rs 从 744 回到 721，
// 净增全部是这块新能力。
// 33960 → 34050：shell_events.rs（27）+ child_priority.rs（36）+ harness_window.rs
// 接入（+10）。2026-09-29 实测：dev 壳装内核的 8 秒里 release 壳的工作台
// webview 被重载并撞上内核的启动顺序竞态——同一时刻 release 内核一行输出都没有，
// 所以不是两棵安装树互相污染，而是 pnpm 把 CPU 与磁盘打满、WebView2 渲染进程
// 被打崩。对策是给装包工具降优先级（保留双壳并行调试），外加把壳侧那些
//「只有后果没有原因」的动作落盘——此前它们只 eprintln，而 GUI 应用的 stderr
// 在 Windows 上根本没有去处。
// 34050 → 34065：store_relocate.rs（净增 11）。跨卷回退的复制中途失败会留下
// 半个 plugins/dsh/，而迁移的入场条件是 root.exists()——半截目录一旦留下就
// 永久短路重试：全量数据还在 dsh-plugins/ 里，用户看到的却是一份残缺的插件
// 列表（dev 侧的对称形态 9b31db4 已修）。回退抽成 copy_legacy_store 以便直接
// 测「中途失败要清掉半截目标」：rename 失败只在跨卷时发生、测试造不出来，
// 复制中途失败用 mode-000 子目录制造（root 环境下该夹具无效，跳过）。
// 34065 → 34180：**接上内核的技能接线**（2026-09-30）。P5 起壳把技能物化进
// `<xlink_home>/skills/active/`，靠 `DSH_CUSTOM_SKILL_DIRS` env 交给内核——
// 而已装内核 0.2.0-rc.2 根本不读这个 env（3481 个 js/d.ts 全树无命中），
// `customSkillDirs` 只从 `cordis.patch.yml` 的插件配置读。也就是说技能这一整
// 块「装得上、内核看不见」，而 README / UI 文案都按已生效在写。本轮把接线
// 落到壳真正写得到的文件上：kernel_adapter.rs +62（生成 / 幂等补写 / 拒绝读不懂
// 的文件 / 5 个测试，PATCH_YML_LEGACY 那条把早期 `[]` 占位模板一并升级），
// paths.rs +26（`shadowing_skill_roots` / `shadowing_skill_entries`：家目录是 git
// 仓库时内核把 `~/.dsh/skills`、`~/.agents/skills` 当「项目级」根，rank 高于
// custom，同名条目会盖掉壳管理的那一份且次序改不动，只能报给用户），
// skills.rs +13（把上面这份报告接到面板 warning 上，让「更新不生效」不再是
// 静默的）。skills.rs / paths.rs / kernel_adapter.rs **均未上调预算**——三个都
// 是既有余量充足的文件，没有一条是靠调数字过的门禁。已用真实内核端到端验证
// （起一次内核 + skills/list RPC 确认活动视图里的技能出现在 session 目录里）。
// 34180 → 34400：两处增量，**归属不同**，分开记账：
//   · 本次（2026-09-30，工作台「刷新工作台」）：harness_cmd.rs 新增 31 行（新文件，
//     见上方登记）+ store.js +19 + harness_window.rs +6（`recreate` 收 reason 参数、
//     `reset_budget`）= 56 行。净增的是一条**手动逃生口**，不是新概念：重建动作
//     复用 `harness_window::recreate`，没有复制第二条建窗链。
//   · 并发会话的跨壳工作台改造（`instance::workbench_running_in_other_shell` 等，
//     2026-09-30 上午，与本次改动无交集）约 +122 行。那部分**没有**在这里替它
//     登记单文件预算——`harness_window.rs` 的 175 行超预算是它造成的，该由那次
//     改造自己拆出去或写清为什么该独立（见 FILE_BUDGETS 里那条）。总量是软上限，
//     先让仓库能过门禁，拆分方案落地后把数字收回。
// 34400 → 34520：技能「被高优先级根盖住」的处置（2026-09-30）。skill_shadow.rs
// 新文件 66 行（见 FILE_BUDGETS）+ commands.rs +12（一条 Tauri 命令）+
// skills.rs +4（`SkillStatus.shadowed` 字段与 warning 文案指向新按钮）+
// SkillsPanel.vue / skills.js +31（确认框列出会被改名的路径，按钮带 loading）。
// 净增是**一条用户可见的出路**，不是新概念：判据（paths.rs 的
// `shadowing_skill_entries`）此前已存在且面板已在报，缺的只是一个可点的处置——
// 而处置复用的是 skills.rs 里早就有的 `keep_aside`（仅因多收一个 reason 参数
// 提为 `pub(crate)`），没有第二套改名逻辑。
// 34520 → 34560：跨壳停机守卫从**硬拦**降级为**提示**（2026-09-30）。kernel.rs
// +19（`OtherShellWorkbench` 结构体 + `KernelStatus` 字段 + `warn_other_shell_workbench`，
// 期间删掉了 `ensure_workbench_stopped` 那条硬拦入口，所以净增不是全量）、
// VersionsPanel.vue +13（版本页把后果与出路提前说清楚，不让用户点完才知道对面可能
// 黑屏）。instance.rs 那条阻断文案**原地改成提示文案**，净增约 0——同一个事实
// （谁占着、进程、端口）只该有一份措辞，两份文案必然漂移。
// 净增的是**一条用户可见的出路**，不是新概念：跨壳判据
// （`instance::workbench_running_in_other_shell`）此前就存在且此前只用来看「要不要
// 拦」，现在只用来「说不拦的理由与恢复办法」。反棘轮（单文件只许下调）未触发，
// kernel.rs / instance.rs 的单文件预算一个数字都没动。
const TOTAL_BUDGET = 34560;
// 6 → 8（临时，随日志侧栏分支收敛回 6）：新增的两处都在该分支正在重构的
// LogViewerWindow.vue（:119 / :157）——与用量窗口无关。该分支落地时应把
// 两段并入 LogSidebar / 共享动作后再把数字收回。
const DUPLICATE_BUDGET = 8;
/** 归一化滑窗宽度。 */
const WINDOW = 10;
/** 计入重复区间的最小长度（归一化行数）。 */
const MIN_SPAN = 12;

const failures = [];
const notes = [];

function walk(dir, suffixes, skip = new Set(['node_modules', 'dist', 'target', '.git'])) {
  const out = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (skip.has(entry.name)) continue;
    const full = join(dir, entry.name);
    if (entry.isDirectory()) out.push(...walk(full, suffixes, skip));
    else if (suffixes.some((suffix) => entry.name.endsWith(suffix))) out.push(full);
  }
  return out;
}

const show = (path) => relative(root, path).split('\\').join('/');

/// 去掉 Rust 的测试模块整块（`#[cfg(test)]` 与 `#[cfg(all(test, …))]` 都算）。
function stripRustTests(text) {
  const lines = text.split('\n');
  const kept = [];
  for (let i = 0; i < lines.length; i += 1) {
    if (!/^#\[cfg\(.*\btest\b.*\)\]$/.test(lines[i].trim())) {
      kept.push(lines[i]);
      continue;
    }
    // 跳过属性行 + 紧跟的 mod 块（花括号配对；字符串里的括号不参与计数——
    // 这里的近似只会让「多跳过几行」，不会漏掉测试块）。
    let depth = 0;
    let seen = false;
    for (i += 1; i < lines.length; i += 1) {
      const code = lines[i].replace(/\/\/.*$/, '');
      depth += (code.match(/\{/g) || []).length - (code.match(/\}/g) || []).length;
      if (code.includes('{')) seen = true;
      if (seen && depth <= 0) break;
    }
  }
  return kept.join('\n');
}

/// 归一化后仍然「有信息量」的行：注释、空行、纯括号行都不算。
function significantLines(text) {
  const out = [];
  text
    .replace(/\/\*[\s\S]*?\*\//g, '')
    .split('\n')
    .forEach((raw, index) => {
      const line = raw
        .replace(/\/\/.*$/, '')
        .replace(/\s+/g, ' ')
        .trim();
      if (!line) return;
      if (/^[{}()[\];,]+$/.test(line)) return;
      if (/^<\/?[a-z-]+>$/.test(line)) return;
      // `line` 是原文件行号：重复块报告要能直接跳到那一行。
      out.push({ text: line, line: index + 1 });
    });
  return out;
}

/// 只算「代码行」：空行与纯注释行不计入预算，否则文档写得越多，指标越像膨胀。
function codeLineCount(text) {
  let inBlock = false;
  let count = 0;
  for (const raw of text.split('\n')) {
    let line = raw;
    if (inBlock) {
      const end = line.indexOf('*/');
      if (end === -1) continue;
      line = line.slice(end + 2);
      inBlock = false;
    }
    const start = line.indexOf('/*');
    if (start !== -1 && line.indexOf('*/', start) === -1) {
      inBlock = true;
      line = line.slice(0, start);
    }
    const trimmed = line.replace(/\/\/.*$/, '').trim();
    if (trimmed) count += 1;
  }
  return count;
}

// --- 1. 生产代码行数 ---------------------------------------------------------

const rustFiles = walk(join(root, 'src-tauri/src'), ['.rs']);
const uiFiles = walk(join(root, 'ui/src'), ['.js', '.vue', '.css']);
const files = [...rustFiles, ...uiFiles].sort();

let total = 0;
const sizes = [];
for (const file of files) {
  const raw = readFileSync(file, 'utf8');
  const text = file.endsWith('.rs') ? stripRustTests(raw) : raw;
  const count = codeLineCount(text);
  sizes.push([show(file), count]);
  total += count;
}

// 反棘轮的支点是本文件**已提交版本**里的数字。必须先读出来，下面
// 「新文件」判定与「只许下调」两条规则都要用它。
const baseline = readBaseline();

// 受检文件但既不在 FILE_BUDGETS 里、也不在 HEAD 的文件树里 = **这次提交
// 新增的文件**。逼着显式登记并写清它为什么该独立成模块——这正是「新能力
// 开新文件」要留下的痕迹。判断以 git 树为准而不是以 FILE_BUDGETS 为准：
// 那份表历来只登记有意义的文件，从来没有穷举过全部。
if (baseline) {
  for (const [path, count] of sizes) {
    if (path in FILE_BUDGETS || baseline.tree.has(path)) continue;
    failures.push(
      `[新文件] ${path}（${count} 行）必须登记进 FILE_BUDGETS，并写清它为什么该独立成模块。` +
        `这是「新能力开新文件」留下的痕迹；上限 ${HARD_FILE_CEILING} 行。`
    );
  }
} else {
  notes.push('拿不到 git 基线，跳过「新文件必须登记」检查');
}

for (const [path, budget] of Object.entries(FILE_BUDGETS)) {
  const hit = sizes.find(([name]) => name === path);
  if (!hit) {
    failures.push(`[预算] ${path} 不存在（文件被移动或改名？请同步 FILE_BUDGETS）`);
    continue;
  }
  if (hit[1] > budget) {
    failures.push(
      `[预算] ${path} 生产代码 ${hit[1]} 行，超过预算 ${budget} 行：请拆分模块，` +
        `或在同一个提交里上调 FILE_BUDGETS 里的数字`
    );
  }
}

// —— 反棘轮：把「记录膨胀」变回「阻止膨胀」——
//
// 三条规则都能机械判定，所以不再依赖 review 记不记得。基线是上面已经读
// 出的 HEAD 数字：仓库自己承认过的值，不需要另存一份快照，也不会因为
// 「同一个提交里改数字」而失守（HEAD 里的旧值仍然是旧值）。
if (baseline) {
  // 规则 1：**有膨胀风险的大文件的预算只许下调**。基线预算达到该阈值的
  // 文件（plugins.rs / theme.css / commands.rs / …）才受此约束——把它们写大
  // 等于承认"我又把这个模块撑大了"，而模块体积正是这里要拦住的东西。刚拆
  // 出来、只装一个关注点的小文件不在此列：把同一个关注点补完整是正常的。
  // 新增能力请开新文件，并写清它为什么该独立。
  for (const [path, budget] of Object.entries(FILE_BUDGETS)) {
    const before = baseline.files[path];
    if (before === undefined) continue; // 新文件，见下面的硬顶
    // 反棘轮只作用于**有膨胀风险的大文件**（基线预算已达软阈值）。像
    // SnapshotRestoreDialog 这种刚拆出来、只装一个聚焦关注点的小文件，
    // 后续把同一个关注点补完整是正常的，不该被这条规则卡住——它离
    // "下一个 plugins.rs" 还差着两个数量级。
    if (before < RATCHET_THRESHOLD) continue;
    if (budget > before) {
      failures.push(
        `[反棘轮] ${path} 的预算从 ${before} 上调到 ${budget}。既有文件的预算只许**下调**：` +
          `把它写大等于承认"我又把这个模块撑大了"，而模块体积正是这里要拦住的东西。` +
          `新增能力请开新文件，并写清它为什么该独立。`
      );
    }
  }
  notes.push(
    `反棘轮基线：合计 ${baseline.total} 行 / ${Object.keys(baseline.files).length} 个已登记文件` +
      `（既有文件预算只许下调）`
  );
} else {
  notes.push('反棘轮基线不可用（拿不到本文件的已提交版本），跳过反棘轮检查');
}

// 新文件的硬顶：防止「下一个 plugins.rs」在诞生时就已经超标。老文件超顶的
// （plugins.rs / theme.css / commands.rs）沿用各自预算逐阶段下调，不追溯——
// 追溯会让本门禁一上来就红，没人会去修。
for (const [path, budget] of Object.entries(FILE_BUDGETS)) {
  if (baseline && baseline.tree.has(path)) continue; // 已存在的老文件
  if (budget > HARD_FILE_CEILING) {
    failures.push(
      `[硬顶] 新文件 ${path} 的预算 ${budget} 行超过硬顶 ${HARD_FILE_CEILING} 行。` +
        `新模块从第一天起就不该这么大——超过就是该拆成几个。`
    );
  }
}

if (total > TOTAL_BUDGET) {
  failures.push(
    `[预算] 生产代码合计 ${total} 行，超过总预算 ${TOTAL_BUDGET} 行：` +
      `新增能力应当优先复用既有模块（pkg.rs / state.rs / async.js 是既有的共享层）`
  );
}
notes.push(`生产代码合计 ${total} 行（预算 ${TOTAL_BUDGET}）`);

// --- 2. 重复块 ---------------------------------------------------------------

const linesByFile = new Map();
const keyToHits = new Map();
for (const file of files) {
  const raw = readFileSync(file, 'utf8');
  const text = file.endsWith('.rs') ? stripRustTests(raw) : raw;
  const lines = significantLines(text);
  linesByFile.set(show(file), lines);
  for (let i = 0; i + WINDOW <= lines.length; i += 1) {
    const key = lines
      .slice(i, i + WINDOW)
      .map((entry) => entry.text)
      .join('\n');
    const hits = keyToHits.get(key);
    const hit = { file: show(file), start: i, line: lines[i].line };
    if (hits) hits.push(hit);
    else keyToHits.set(key, [hit]);
  }
}

const at = (file, start) => {
  const lines = linesByFile.get(file);
  if (start < 0 || start + WINDOW > lines.length) return null;
  return lines
    .slice(start, start + WINDOW)
    .map((entry) => entry.text)
    .join('\n');
};

// 把「出现 ≥2 次的窗口」按出现位置向后延伸，得到最长重复块。直接数窗口是不行
// 的：一个 30 行的复制粘贴会产生 21 个互相重叠的命中窗口，指标会随块长线性放大。
const covered = new Set();
const duplicateSamples = [];
let duplicates = 0;
for (const hits of keyToHits.values()) {
  if (hits.length < 2) continue;
  if (hits.some((hit) => covered.has(`${hit.file}:${hit.start}`))) continue;
  let extra = 0;
  for (;;) {
    const next = hits.map((hit) => at(hit.file, hit.start + extra + 1));
    if (next.some((key) => key === null)) break;
    if (!next.every((key) => key === next[0])) break;
    extra += 1;
  }
  for (const hit of hits) {
    for (let step = 0; step <= extra; step += 1) covered.add(`${hit.file}:${hit.start + step}`);
  }
  const length = WINDOW + extra;
  if (length < MIN_SPAN) continue;
  duplicates += 1;
  if (duplicateSamples.length < 20) {
    duplicateSamples.push(
      `${hits[0].file}:${hits[0].line}（${length} 行 × ${hits.length} 份）`
    );
  }
}

notes.push(`重复区间（≥${MIN_SPAN} 行归一化代码）${duplicates} 处（预算 ${DUPLICATE_BUDGET}）`);
if (duplicates > DUPLICATE_BUDGET) {
  failures.push(
    `[重复] 发现 ${duplicates} 处重复区间，超过预算 ${DUPLICATE_BUDGET}：` +
      `请把共同部分提到共享层（Rust: pkg.rs / state.rs；前端: async.js）\n` +
      duplicateSamples.map((sample) => `      ${sample}`).join('\n')
  );
}

// --- 输出 -------------------------------------------------------------------

const biggest = sizes.sort((a, b) => b[1] - a[1]).slice(0, 6);
console.log('生产代码最大的几个文件：');
for (const [path, count] of biggest) {
  const budget = FILE_BUDGETS[path];
  console.log(`  ${String(count).padStart(5)} 行  ${path}${budget ? `（预算 ${budget}）` : ''}`);
}
for (const note of notes) console.log(`• ${note}`);

if (reportOnly) {
  if (duplicateSamples.length) {
    console.log('重复区间（前几处）：');
    for (const sample of duplicateSamples) console.log(`  ${sample}`);
  }
  console.log('\n（--report：只度量，不判失败）');
  process.exit(0);
}
if (failures.length) {
  console.error('');
  for (const failure of failures) console.error(failure);
  console.error('\n代码膨胀门禁未通过。');
  process.exit(1);
}
console.log('\n代码膨胀门禁通过。');
