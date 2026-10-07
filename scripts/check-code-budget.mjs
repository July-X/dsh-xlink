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
import { createHash } from 'node:crypto';
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
  let listing = '';
  let tree = new Set();
  let renames = new Map();
  try {
    text = execFileSync('git', ['show', 'HEAD:scripts/check-code-budget.mjs'], {
      cwd: root,
      encoding: 'utf8',
      maxBuffer: 8 * 1024 * 1024,
      stdio: ['ignore', 'pipe', 'ignore'],
    });
    // 暂存区里的重命名。搬移是 `git mv` 暂存着的，git 自己知道新旧路径——
    // 比「内容哈希」可靠：文件搬完往往同时改了里面的路径，内容就变了，
    // 哈希认不出是搬移还是新写。
    renames = new Map();
    const staged = execFileSync('git', ['diff', '--cached', '-M', '--name-status'], {
      cwd: root, encoding: 'utf8', maxBuffer: 8 * 1024 * 1024,
      stdio: ['ignore', 'pipe', 'ignore'],
    });
    for (const line of staged.split('\n').filter(Boolean)) {
      const parts = line.split('\t');
      if (parts[0].startsWith('R') && parts[2]) renames.set(parts[2], parts[1]);
    }
    listing = execFileSync('git', ['ls-tree', '-r', 'HEAD'], {
      cwd: root,
      encoding: 'utf8',
      maxBuffer: 8 * 1024 * 1024,
      stdio: ['ignore', 'pipe', 'ignore'],
    });
    tree = new Set(); // 由下面的 blobs / names 推导，路径本身不再单列
  } catch {
    return null;
  }
  const files = {};
  // 「这份代码以前存在过吗」的三级判据，HEAD 树里各取一份：
  //   blobs  内容哈希——**改名与搬移都认得**（内容没变）。最强。
  //   names  文件名——搬目录换了路径但名字没变时认得。
  //   tree   完整路径——原地未动时认得。
  // 2026-09-30 ui/src 与 src-tauri/src 两次分目录都撞上过：搬过的文件被当成
  // 新文件，逼着补预算、还撞上 800 行硬顶。
  const blobs = new Set();
  const names = new Set();
  for (const line of listing.split('\n').filter(Boolean)) {
    const tab = line.indexOf('\t');
    const meta = line.slice(0, tab).split(/\s+/);
    const path = line.slice(tab + 1);
    if (meta[2]) blobs.add(meta[2]);
    names.add(path.split('/').pop());
  }
  // 路径无关的基线表：文件名 -> 基线预算。搬移后按路径查不到，只能按名字查。
  const fileBudgets = new Map();
  const block = text.match(/FILE_BUDGETS\s*=\s*\{([\s\S]*?)\n\};/);
  if (block) {
    // 条目形如 `  'path/to/file': 1234,`（带行尾注释）。
    for (const line of block[1].split('\n')) {
      const hit = line.match(/'([^']+)'\s*:\s*(\d+)/);
      if (hit) {
        files[hit[1]] = Number(hit[2]);
        fileBudgets.set(moduleId(hit[1]), Number(hit[2]));
      }
    }
  }
  const total = text.match(/TOTAL_BUDGET\s*=\s*(\d+)/);
  if (!total) return null;
  return { files, total: Number(total[1]), tree, names, blobs, fileBudgets, renames };
}

/** 一个 .rs 路径的「模块标识」：`src/usage.rs` 与 `src/usage/mod.rs` 都是
 *  `usage`。搬进子目录后登记路径常写成 `mod.rs`，按文件名查基线会查空——
 * 反棘轮于是对搬过的大文件失效，那正是它最该管的一批。 */
/** 这个工作区文件的内容是不是 HEAD 里某个文件的内容（= 搬移或改名，不是新文件）。 */
function isKnownBlob(baseline, path) {
  if (!baseline.blobs || baseline.blobs.size === 0) return false;
  try {
    const buf = readFileSync(path);
    const hash = createHash('sha1').update(`blob ${buf.length}\0`).update(buf).digest('hex');
    return baseline.blobs.has(hash);
  } catch {
    return false;
  }
}

function moduleId(path) {
  const name = path.split('/').pop();
  if (name !== 'mod.rs' && name !== 'mod.rs.bak') return name.replace(/\.rs$/, '');
  const parts = path.split('/');
  return parts[parts.length - 2] || name;
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
  // 3011 → 3001：清掉上一条改版（按钮配色换成 .btn-chat / .btn-danger）遗留的
  // 死规则 .el-button.official-chat-open / .official-chat-closed，外加因此失去
  // 唯一消费者的 --whale-eye 与本来就没人用的 --whale-eye-soft。
  // 3225 → 3011：移除内核版本行前面那个插件快照 tooltip（用户 2026-10-03）。
  // 带走的：.installed-tip-trigger / .installed-tip-icon 与 .installed-tip* 整族
  // 内容样式，加上 .kernel-plugin-tooltip 的局部覆盖，共 151 行。.el-popper.is-dark
  // 的全局主题对齐**留着**——卡片标题的 ℹ️ 等 tooltip 还在用那套。净 -134。
  'ui/src/theme.css': 3001,
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
  // 2932 → 2871：删掉 KernelPluginRow 与 kernel_plugin_list（+ 它的单测）。
  // 唯一调用方是版本行那个 tooltip，一并移除后整条链没有第二个消费者。
  // 本文件原先正好卡在 2932 = 预算上——它是 72 个已登记文件里最贴线的一个，
  // 这次顺带把它从线上拽下来。净 -61。
  'src-tauri/src/plugins/center.rs': 2871,
  // 2026-10-05：插件目录的**检索**层（catalog.rs）。面板把关键词 / 分类 /
  // 排序三个参数作为一个整体交给它，搜完只回一页。
  // 独立成文件的三条理由：
  // ① center.rs 是 2871 行的反棘轮文件（只许下调），筛选规则放不进去；
  // ② 它与 center.rs 答的不是同一个问题——center 答「目录从哪来、多久过期、
  // 主源挂了回退哪」，这里答「这一页是哪几条」。取源留在 center、筛选留在
  // 这里，两份都不重复；把两者揉进一个文件，那 9 MB JSON 的取用路径与
  // 筛选路径就会缠在一起，谁改都怕碰坏另一半。
  // ③ 筛选规则是纯函数（不碰 fs、不出网），拆出来后 10 条单测不用起临时
  // home、不用等网络——**测筛选规则不该付一次网络往返的钱**。
  // 命令壳（plugin_catalog_search）也在这里：它只有 10 行转发，拆成
  // `catalog_cmd.rs` 反而多一层壳（与 bisect_cmd.rs / harness_cmd.rs 的
  // 分法不同，那两组是「有实质逻辑的命令」）。
  'src-tauri/src/plugins/catalog.rs': 150,
  // 2980 → 2932：删掉 P4 留下的旧签名壳共 11 项（`sync_kernels` /
  // `materialize_one` / `remove_materialized` / `sweep_kernel_orphans` /
  // `sweep_all_kernel_orphans` / `read_meta` / `write_meta` /
  // `kernel_plugins_dir` / `kernel_plugin_dir` / `kernel_meta_file` /
  // `META_SUBDIR`），它们都是「函数体只有一行委托 + 弃用它的形参」的过渡壳。
  // 连带把 6 个仍用旧签名调用的测试改到主路径——那些测试从 P4 之后就没跟着
  // 更新，一直在验证一条没人走的分支，现在改的是真在跑的那条。
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
  // 11 个 mod.rs（2026-09-30 按功能分目录时新建）：每个目录一个，声明子模块
  // 并把子模块标成 pub(crate)。它们不含实现，只是一层路由，所以基线都在 10 行上下。
  // `check-invariants` 的「新文件必须登记」按文件名认搬移，但这 11 个是新出现的，
  // 不在 HEAD 的树里，得逐个登记。
  // 15 → 19：2026-10-02 后台常驻追加 3 条（autostart / menu_bar[macos] /
  // resident），tray 保留 cfg(windows)。仍是一层路由，没有实现。
  // 19 → 20：localtime 进来了（见那条）。仍是一层路由，没有实现。
  // 20 → 21：stream（带超时的流式子进程捕获，git clone 用）进来了。仍是一层
  // 路由，没有实现。
  'src-tauri/src/shell/mod.rs': 21,
  // 带超时的流式子进程输出捕获。**为什么不复用 `process::run_with_progress`**：
  // 那条是给「pnpm 装包 + 跑原生模块构建」设计的，它强制两件 git clone 都不想要的
  // 事——把输出轮转落进 `logs/`（而 clone 的输出紧接着就被 on_progress 收进诊断
  // 记录，同一段 `Receiving objects: 43%` 在磁盘上存两份，且 LOG_RETENTION_DAYS
  // = 30 让它一个月不散），以及 30 分钟固定上限（装包确实可能要那么久，clone 卡满
  // 5 分钟就该让用户换来源）。`process.rs` 又是 1157/1157 的反棘轮文件，塞不下。
  // 真正该共享的那几件（quiet / isolate_process / read_capped_line /
  // terminate_process_tree）全部沿用 `process` 里的同一份实现，没有重造。
  // 2026-10-06：`git clone --progress` 让进度面板实时显示百分比，此前 12KB/s 的
  // 直连是一段十分钟不动的空白，用户分不清「在慢慢拉」和「已经卡死」。
  // 111 → 117：补两处自审出来的缺口。一是**总量**封顶（`read_capped_line` 只
  // 封单行 64 KiB；一个把 stdout 当日志转储的子进程能把内存吃光，而 clone
  // 达不到这个数——它防的是明天那种调用方）。二是超时后补 `child.wait()`：Rust
  // 的 `Child` 在 drop 时**不会**自动 wait，不收这一下，超时的 git 会在进程表
  // 里留一条僵尸直到壳退出。
  'src-tauri/src/shell/stream.rs': 117,
  'src-tauri/src/kernel/mod.rs': 10,
  // 10 → 11：2026-10-05 追加 catalog（插件目录检索层）。仍是一层路由，
  // 没有实现——同 shell/mod.rs 那条 15 → 19 的先例。
  'src-tauri/src/plugins/mod.rs': 11,
  'src-tauri/src/skills/mod.rs': 10,
  // 11 → 12：snapshot_cmd 进来（快照三命令从 commands.rs 搬出，见那条）。
  'src-tauri/src/diagnostics/mod.rs': 12,
  'src-tauri/src/harness/mod.rs': 10,
  'src-tauri/src/usage/mod.rs': 10,
  'src-tauri/src/notify/mod.rs': 10,
  'src-tauri/src/migration/mod.rs': 10,
  'src-tauri/src/pkg/mod.rs': 10,
  'src-tauri/src/node/mod.rs': 10,
  // 2026-10-02「后台常驻 / 自动启动」四个新文件。三条独立的理由：
  //
  // ① resident.rs（81 行）——**跨平台常驻语义只有这一份**。托盘（Windows）
  //    与菜单栏（macOS）只提供图标与菜单，隐藏 / 恢复 / 退出请求三个动作
  //    全在这里，两端各写一份就会漂：2026-10-02 之前 macOS 走的确实就是另一套
  //    语义（关闭 = 退出），用户得重新学一遍。放进 shell/ 而不是 lib.rs，
  //    是因为 lib.rs 只做「按平台分发」，不该承载行为。
  // ② menu_bar.rs（46 行）——macOS 图标与菜单。按 `cfg(macos)` 编译，与
  //    `tray.rs`（cfg(windows)）严格对偶。**独立成文件是为了让两端各自持有
  //    平台特有的那部分**：tray 有 DPI 选帧与注册表主题监听，menu bar 有
  //    模板图与 ActivationPolicy 降级，两边共有的动作已经在 ① 里了。
  // ③ autostart.rs（331 行，其中生产代码 190）——登录项读写 + 三条 Tauri
  //    命令 + 启动判定。独立成模块有两个理由：写 / 读 / 删**会碰用户真实的
  //    系统登录项**（`~/Library/LaunchAgents`、`HKCU\...\Run`），必须与
  //    「面板显示什么」分开，便于给它的测试划出「不许动真实条目」的边界；
  //    平台实现（plist vs 注册表）自成一块 cfg 模块，混进 shell/ 的任何现有
  //    文件都会让那块 cfg 占掉大半篇幅。
  // ④ ui/src/shell/autostart.js（84 行）——与通知设置同形状的状态 + 动作。
  //    与 notifications.js 一样是「Rust 持真相、前端只读 + 保存」，放在
  //    shell/ 下是因为它服务设置页、不属于任何业务面板。
  // 90 → 113：把托盘 / 菜单栏**共用的菜单接线**也收进来了（`build_background_menu`
  //    + 菜单 id）。原先两端各写一份，`check-code-budget` 的重复区间检查抓到
  //    了 14 行逐字相同；更值得担心的是「退出」这条接线只改一端就会让用户点
  //    到一个不响应的菜单项，而那在界面上没有任何症状。共用之后这里反而变短。
  // 113 → 123（2026-10-05）：「收进后台」提示的补发判据收紧为消费式双旗
  // （RESTORE_HINT_SHOWN + HIDDEN_BY_USER，见 consume_restore_hint）——登录
  // 自启藏壳后的第一次唤回不再弹「刚刚把窗口收进了后台」（用户反馈经常
  // 触发），外加完整语义表测试。
  'src-tauri/src/shell/resident.rs': 123,
  'src-tauri/src/shell/menu_bar.rs': 105,
  'src-tauri/src/shell/autostart.rs': 360,
  // 2026-10-02 磁盘占用报表（只读，无删除入口）。独立成模块的理由：
  // ① 它是**纯读 + 纯计算**，不碰任何状态机——塞进 `commands.rs` 会把一个
  //    290ms 的全盘 walk 混进那堆生命周期命令里，让两边都更难读；
  // ② `measure(data_dir, home)` 把两个根目录都做成**参数**，测试因此能指向
  //    临时目录。这条是硬要求：它一旦内部去调 `xlink_home()`，测试就会
  //    扫到用户真实的 `~/.dsh-xlink`，而那里的三个用例都会写临时文件。
  // 210 → 270：先加「壳模式 / 默认实例 id 显示为版本名」那两处映射
  // （`dev` / `release` → 开发版 / 正式版）与其测试，再加「两段式返回 +
  // 一天新鲜度」那层缓存（读 / 写缓存、陈旧判定、后台重扫线程、事件回填）
  // 与钉住它的四条测试。缓存那层有**两处必须测的边界**：时钟为 0、时钟
  // 回拨（`saturating_sub` 写成 `wrapping_sub` 会让缓存永久失效且不报错）。
  // 270 → 282：「缩写 + 悬停全文」这一层。`UsageEntry` / `UsageGroup` 各多一个
  // `detail` 字段，`build_group` 多收一个分类 detail 参数，四个行产出点各多给
  // 一栏，另加 `Row` 类型别名（四个 String 挨在一起容易看串）与钉住它的测试。
  // 涨的是**字段**，不是逻辑：缩写规则没有第二份实现，仍只有拼得出名字的那
  // 个地方知道哪些限定词冗余。本文件基线远低于 RATCHET_THRESHOLD，上调在规则内。
  //
  // 282 → 312：「刷新」按钮强制重扫（用户 2026-10-07 报「刷新按钮，没有正确扫描
  // 磁盘占用」）。命令原先**没有** force 参数，缓存新鲜时直接 `return Ok(cached)`、
  // 连后台线程都不起；而缓存一天才刷一次，于是点按钮几乎永远落在这条分支上——
  // 按钮是个空动作，界面上还完全看不出它没生效。净增 30 行：
  //   · `DiskUsageReply`（6）：返回体从裸报表变成 `{ report, backgroundRefresh }`。
  //     后者是前端「后台正在重新扫描…」转圈的唯一依据，而那个标志此前**只被
  //     置过 false**，是个死标志。判据没法由前端自己推：「缓存新鲜」与「重扫刚
  //     结束」两条路径在前端一模一样。
  //   · `Plan` + `plan()`（21）：把四条分支的判据抽成纯函数，好处是「force 排在
  //     新鲜度之前」这条能脱离 Tauri 测（命令体要 AppHandle，测不了）。
  //   · 命令体重排（3）：强制走 `commands::blocking` 真扫一遍。
  // **不拆模块**：`Plan` / `plan()` 与同模块的缓存语义不可分（`FRESH_MS`、
  // `read_cache` 就在隔壁），拆出去只是躲门禁而不是真写出更少的代码。
  // 顺手收掉的重复：`Plan::Answer` 原本把 `report` / `background_refresh` 两个
  // 字段抄了一遍，现在直接持有 `DiskUsageReply`。
  // 312 是 `cargo fmt` **之后**的实测值（手写时把两个构造写成单行共 312，
  // rustfmt 按自己的规则展开成多行 → 316；这里登记的是门禁真正会量到的数）。
  'src-tauri/src/diskusage.rs': 316,
  'ui/src/shell/autostart.js': 90,
  // 2026-09-30 按功能分目录，三个大文件各上调到实测值：
  //   commands.rs        2110 → 2171
  //   plugins/center.rs  2980 → 2983
  //   usage/subscription  900 → 909
  // **不是新逻辑**，是分目录本身的代价：每处引用从 `crate::process::X` 变成
  // `shell::process::X`，多一段；`use X::{self, Y}` 拆成「绑定组」+「绑定叶子」
  // 两行多出一行，个别过长的调用被 rustfmt 折行。反棘轮拦的是「又把这个模块
  // 撑大了」，而这里一个功能都没加——所以按实测值登记，并明确以本次为新基线，
  // 之后恢复只许下调。shell/mod.rs 是 15 行（14 个 `pub(crate) mod` + 1 个 cfg）。
  // 2166 → 2080：把官方对话那一组常量、布局算术与重排判据摘进
  // harness/official_chat.rs（见该文件的登记），并把只服务它的
  // logical_window_size 一并带走。搬移前基线 2110，实测降到 2087——
  // 反棘轮是「只许下调」，所以这里跟着降，不给回弹留口子。
  // 2087 → 2061：18 条命令从裸 `spawn_blocking(…).await.map_err(|e| e.to_string())?`
  // 换到 `blocking()` 助手（review L5），panic 提示统一带上可操作的下一步。
  // 净减 26 行不是因为删了功能，是因为「自己拼 spawn_blocking + 自己写
  // map_err」比调一个助手长。
  // 2061 → 2047：删掉 kernel_plugin_list 命令（随 tooltip 一起移除）。
  // 顺带清掉 activate_version 里一处提到它的陈旧注释。净 -14。
  // 2047 → 1969：三条快照命令（snapshot_list / snapshot_preview_restore /
  // snapshot_restore）搬去 diagnostics/snapshot_cmd.rs，净 -78。**这次是被
  // 逼出来的**：给 snapshot_restore 接上运行记录（设计 §4.1 的 restore kind）
  // 之后它到了 2051，而反棘轮不允许上调 2047。门禁给出的唯一出路是拆，
  // 而为了 4 行去挤格式只会把难读的东西留给下一个人。照 bisect_cmd.rs 的
  // 先例让这一族命令自成模块，是本来就该做的事。顺带修掉一处旧伤：
  // plugin_set_precheck 的文档注释曾被挤到 snapshot_list 头上，两条命令
  // 共用一段说明。
  // 1969 → 1925：两条插件预检命令（取证 + 应用）与它们共用的通道封装搬去
  // plugins/precheck_cmd.rs。**这次是被逼出来的**：预检拆成两阶段后新加了
  // plugin_precheck_apply，那份文件到了 1998，而反棘轮不允许上调 1969。差
  // 的 29 行去挤格式只会把难读的东西留给下一个人——照 bisect_cmd.rs 与
  // snapshot_cmd.rs 的先例拆更合适，拆完预算还能再降。
  'src-tauri/src/commands.rs': 1925,
  // 安全网 P0 + P1：环境快照。指纹计算（可重建的声明而非备份）、快照文档
  // 读写（走 state.rs 骨架）、裁剪策略（高权重优先 + 永不丢 last-known-good）、
  // 两个打点的入栈规则、给面板的只读视图，以及 P1 的差异计算与恢复执行
  // （只改差异项 + 先备份 + 不删数据 + 动不了的照实报）。
  // **不含**二分定位——那是 P2。
  // P1 追加只读视图与打点。**记录**（指纹 / 快照文档 / 裁剪 / 打点）与**执行**
  // （差异计算 / 恢复落地）拆成了 snapshot.rs + restore.rs：读前者的人关心
  // "这份回退点可不可信"，读后者的人关心"点确认之后会发生什么"。
  'src-tauri/src/diagnostics/snapshot.rs': 590,
  // 差异计算与环境恢复：逐条比对、只改差异项、只停用不卸载、恢复后用
  // verify::probe_once 自检。
  'src-tauri/src/diagnostics/restore.rs': 560,
  // P0 + P1 的前端状态与展示函数：打点原因中文化、摘要拼装、空状态的三句
  // 话，以及恢复预览的维度标签 / 标题 / 动不了的条数提示。
  // 145 → 155：P2 追加 outcomeHeadline。
  // 155 → 180：`verificationView` 三态（verified / failed / not-needed）从
  // outcomeHeadline 里独立出来——三态各自带配色与整句文案，塞回一个函数
  // 只会让它同时负责"怎么说"和"算什么颜色"。
  'ui/src/diagnostics/snapshots.js': 180,
  // P1 的恢复确认弹窗：把「将要失去什么」和「动不了什么」分成两栏列出。
  // 200 → 215：实测告警改由 `alert` computed 决定，需要"有没能完成的条目
  // 就降级成 warning"这条交叉规则，以及失败原因的第二行展示。
  'ui/src/diagnostics/SnapshotRestoreDialog.vue': 215,
  // P2 二分定位的会话与步骤记录。与判定逻辑分开：这里只管"试了什么、
  // 结果如何、排除了谁"，怎么试由命令层驱动 verify::probe_once。
  // 收尾只有三种取值，**没有"找到根因"**——组合效应会让二分停在不可修
  // 的答案上。
  // 350 → 372：会话多带一个 run_id（跨命令接着写同一条运行记录，理由见
  // operation_run 那条），另收进 rounds_estimate —— 原本在 bisect_cmd.rs 有
  // 一份同算法的副本，两边各改一次，界面上的「预计轮数」就会和实际跑的轮数
  // 对不上。少一份实现比多 22 行划算。
  'src-tauri/src/diagnostics/bisect.rs': 372,
  // 二分定位的 Tauri 命令壳。**被反棘轮逼出来的**：四条命令加 rounds_estimate
  // 原本要进 commands.rs，而那条规则不许把一个 2110 行的文件继续撑大。
  // 这组命令只服务二分一件事、内部高度耦合，自成模块也确实更清楚。
  // 120 → 140：每轮真正装进沙盒的插件白名单 + 工作台运行态守卫。
  // 140 → 169：接上运行记录（设计 §4.1 的 bisect kind）。二分是多命令流程，
  // 每轮试探都是一次独立调用，所以每条命令都要 attach 回同一条 recorder、
  // 推本轮事件、并在收出结论时收尾；另有 rounds_estimate 搬去 bisect.rs
  // 之后少掉的 12 行。剩下的净增全是这条接线的收尾分支，删任何一处的后果
  // 都是「记录永远停在进行中」。（169 → 183 是 cargo fmt 重排长表达式的换
  // 行，不是又加了逻辑。）
  'src-tauri/src/diagnostics/bisect_cmd.rs': 183,

  // 给**已有成熟状态文件**的诊断操作补运行记录：恢复与二分（设计 §4.1 的
  // 四种 kind）。
  //
  // **独立成模块而不是塞进 run.rs 或 restore.rs / bisect.rs**：启动与预检各有
  // 专门的编排层，阶段事件是就地打的；而恢复与二分的判定逻辑早就写好并各带
  // 一份状态文件，在它们内部插阶段事件会侵入那两条成熟路径——它们有各自的
  // 测试与边界，重新接线的风险远大于收益。所以这两条链走旁路：命令层调它们
  // 前后各打一次事件，结束时按**终态**而不是「调用成功」记账。
  //
  // 约 110 行是单测（跳过项不算成功、没测过不说成通过、二分不称根因、中止
  // 不算成功、跨命令接续后 seq 仍然连续、未知 stage 原样保留）——这几条正是
  // 「结论会骗人」的高发处。
  // 249 → 252 同理：cargo fmt 的换行，不是新增逻辑。
  'src-tauri/src/diagnostics/operation_run.rs': 252,
  // 本地日历时间的换算与格式化（审查 P2-04）。**独立成模块而不是留在
  // process.rs**：那份是反棘轮文件只许变小，而加东西的唯一出路是拆；
  // 更重要的是它此前有三份实现（进程杂项、事件日志、运行记录 id），
  // 三处的时区回退路径不同，于是两份时间戳对不齐——而它们的作用恰恰是
  // 互相对齐。约 15 行是单测（三格式同源、纪元前回退）。
  'src-tauri/src/shell/localtime.rs': 59,
  // 快照与恢复的 Tauri 命令壳（从 commands.rs 搬出，见那条）。三条命令构成
  // 一个完整动作：看见回退点 → 看差异 → 执行。留在 commands.rs 时那份文件
  // 因接上运行记录而越过反棘轮的 2047；门禁给出的唯一出路是拆，与其为 4 行
  // 去挤格式，不如照 bisect_cmd.rs 的先例让这一族自成模块。
  'src-tauri/src/diagnostics/snapshot_cmd.rs': 86,
  // 「起一次沙盒内核看它起不来」的共享判据：P1 恢复后自检与 P2 二分试探
  // 共用。判据一旦有两份实现就会分叉，而分叉出来的那个会让二分**静默收敛
  // 到错误答案**——它把"没试成"当成"起来了"。
  // 90 → 120：**把被测配置装进沙盒**这一步。少了它，这个函数起的是
  // 零插件零技能的裸内核，等于在另一个问题上做判定。
  'src-tauri/src/diagnostics/verify.rs': 120,
  // P2 的前端状态与展示函数：结局映射（无"根因"）、轮数估算、每轮标题与配色。
  // 110 → 175：`startBisect` 现在**真的把每一轮跑完**（循环 invoke
  // bisect_probe），外加 abort 的错误提示与刷新。只发起不驱动的话，面板会
  // 永远停在"排查进行中 … 请耐心等"。
  'ui/src/diagnostics/bisect.js': 175,
  // P2 的排查面板：逐轮显示"在试哪一半 / 上一轮结果 / 已排除几个 / 还要几轮"。
  // 180 → 187：结论出来之后给一个「查看排查诊断」入口，把同一场排查带进
  // 诊断层与启动 / 预检的记录同口径地看。只在有结论后给——跑的过程中时间线
  // 还少一半，推给用户看到的是一条看不出所以然的半截记录。
  'ui/src/diagnostics/BisectPanel.vue': 187,
  // P0 的概览页卡片。刻意**只读**：提前放"一键回退"会让用户在没看清
  // 差异的情况下丢配置。
  'ui/src/diagnostics/SnapshotCard.vue': 170,
  // 安装预检的沙盒生命周期共享层：一次性实例 id / 端口分配 / 目录骨架、
  // 适配器启动与就绪看护、环回 HTTP 存活确认、日志标记扫描、残留回收、
  // 证据另存，以及 Verdict / PrecheckReport 两个对外类型。
  // **刻意与「装什么」无关**：技能预检将来直接复用同一套起停与探测。
  'src-tauri/src/plugins/sandbox.rs': 560,
  // 运行诊断的统一模型：一次可追踪操作 = 一条记录，事件按 seq 定序，
  // 状态 / 归因用同一套词表。**独立成模块而不是塞进 guard.rs**：它同时服务
  // 启动、预检、恢复、二分四条链，而 guard.rs 在反棘轮上；也让「未知取值
  // 原样保留」这条契约有唯一的实现处——散在四处各写一遍，漏一次就把新
  // 版的阶段静默丢掉。
  // 500 行里有约 180 行是单测（写失败不改结果、引用保护、脱敏形状、
  // 事件预算、未知取值往返）——那些是这套模型最容易回归的地方。
  // 525 → 533：`Recorder::attach` —— 接续一条已落盘但没收尾的记录。二分
  // 跨 N 次命令调用，recorder 活不过命令边界，不接续就得每次重开一条，把
  // 一次排查切成互不相干的好几条。
  // 533 → 585：`sweep_orphans`（启动清扫索引里查不到的孤儿详情）+ `clear`
  // （用户主动清空整段历史）。**孤儿是真缺口不是洁癖**：详情文件先落地、
  // 索引后写入，壳在两者之间被强杀就留下一个列表读不到的 `run-*.json`，
  // 它带着整份事件流一直占盘。`prune` 只清「被 20 条上限挤掉」的，够不着它。
  // 三个新测试里有两个当场抓出实现 bug：(1) 文件名 `{id}.json` 而 id 自带
  // `run-` 前缀，剥掉前缀再比索引会把每一条都当成孤儿；(2) 更要命的一条——
  // 拿 `load_index` 的容错空索引去比对，索引一损坏就会把用户全部诊断历史
  // 当孤儿抹干净。改成 `load_checked`，损坏时一个都不删。
  // 585 → 590：`settle_dangling_stage`——`finish` 收尾时把悬空的 `running`
  // 阶段补成终态。买的是「漏一条出口就多一个永远转圈的阶段」这件事**从
  // 「每条出口记得记得调」变成结构事实**，与 `precheck.rs` 的 `StoreRestore`
  // 同一款取舍。三个 Rust 单测覆盖（含落盘回读）。
  // 本文件 585 < RATCHET_THRESHOLD 600，不受反棘轮约束。
  'src-tauri/src/diagnostics/run.rs': 590,
  // 运行记录的只读命令壳。刻意**没有** prune 命令：裁剪是写入侧
  // `Recorder::finish` 的职责，给前端一条命令就等于开一个能被随手调用的
  // 删除路径。三条命令的 `spawn_blocking` 走 `commands::blocking` 助手，
  // 免得 JoinError 的英文原文直接甩到面板上。
  // 55 → 59：`diagnostic_run_clear`。**它不推翻上面「刻意没有 prune 命令」
  // 那条纪律**：prune 是自动裁剪（超 20 条被动触发），给它一条前端命令毫无
  // 意义且等于开一个能被随手调用的删除路径；`clear` 是用户明确说「这段历史
  // 我不要了」，没有自动决策成分在里面，删的就是他刚看过一眼的那份列表。
  'src-tauri/src/diagnostics/run_cmd.rs': 59,
  // 启动诊断的编排层：开记录 → 逐阶段打点 → 收尾归因 + 事故引用。
  // **被反棘轮逼出来的**：这些逻辑"只在启动诊断这条路径上成立"，原先要
  // 直接写进 2000 行的 commands.rs，而那条规则不许把超阈值文件继续撑大。
  // 同时它承担事故文件持久化——事故属于诊断链，不属于看护。
  'src-tauri/src/diagnostics/startup_run.rs': 420,
  // 诊断层的共享状态与动作：通道消息解析（结构化事件 / 纯文本双路）、
  // 实时事件流、打开与返回。**解析规则只有一份**——进度浮层与 store 各有
  // 一个 makeChannel 消费点，各写一份解析就会漂成两种行为。
  // 150 → 169：恢复 / 排查诊断的打开与加载。四条加载路径的取数逻辑抽成
  // 一个 fetchRunDetail——它们只差 kind、loading key 与空态文案三点，复制
  // 四份之后改一处忘一处，漂掉的恰恰是用户第一次看到的那句话。
  // 169 → 191：按 kind 分派的统一入口（审查 P1-05）、记录加载时落证据路径
  // （P1-02）、解析器带出信封里的 runId（P2-01）。
  // 191 → 213：`clearDiagnosticRuns`。走既有的 `confirmDialog` + `withLoading`
  // + `toastSuccess`，不新造一套确认与提示——确认框的 z-index 那些坑已经踩过
  // 一轮了。删完立刻 `loadRecentRuns`：清空后那行「最近操作」要真的变空态。
  // 213 → 236：R2-P2-01 的实时流分区。先定「这条流属于哪一次运行」再按它
  // 过滤：与目标不同的事件分「见过（迟到，丢弃）」与「没见过（新运行，重开）」
  //。判据只能是「见过没有」——只有记住见过哪些 runId，才能区分「新运行的
  // 第一条」与「旧运行的迟到消息」，这两件事在信封里长得一模一样。
  'ui/src/diagnostics/diagnostics.js': 236,
  // 诊断层的**动作代理**（设计 §8.2）。与 `diagnostics.js` 分开是因为职责
  // 不同：那边管「现在在看什么」，这边管「用户点了会发生什么」——混在一起
  // 每次加动作都要重新读一遍状态定义才能确认没写错层。
  // 60 → 67：补齐 P2-02 点名的动作（打开事故 / 打开工作台 / 恢复变更前 /
  // 刷新内核状态）。本文件开头写着「组件只调这里的动作」，而剩下那一半靠
  // 自觉——下一个人一定会直接 import，所以先补齐再说。另有 P1-05 验收里的
  // 「操作完成后刷新最近操作」：预检与启动跑完各刷一次，否则概览那张卡停在
  // 上一次的结果上，而那正是「用户可能还没意识到插件已装上」的那类误导。
  // 67 → 71：`loadKernelStatusDiagnosis` 只在 `refreshAll` 报告内核状态读取
  // 成功时才推进 `kernelReadAt` 并清错（R2-P1-04）。多出来的 4 行是「失败要
  // 留下可显示的提示」——不给它一句人话，过期提示就只剩一个时间戳在说话。
  // 71 → 88：R2-P2-02 收拢四个诊断页的加载与「应用变更」。上一轮只补了「打开
  // 某处」那一半，剩下的一半由各组件自行 import，于是 loading key、错误处理、
  // 「这条记录属于哪次运行」分散在五处。**概览页（ControlTower）是刻意的例外**，
  // 理由写在本文件注释里并有测试钉住。
  'ui/src/diagnostics/diagnostic-actions.js': 88,
  // 阶段 / 状态 / 归因的中文名与语义色。**未知取值必须显式显示**——丢掉
  // 会让时间线出现一个洞，而那正是新版才有、最值得看的部分。文案集中
  // 在这里，改文案不该牵动落盘格式。
  // 两张状态表是刻意的：`STATUS_META` 给时间线上的单个事件（短标签），
  // `RUN_HEADLINE` 给运行记录卡片的头部（整句）。混成一张会让卡片顶部
  // 只能显示「失败」——那等于没说清什么没能启动。
  // 130 → 158：恢复 / 排查的阶段中文名，以及按 kind 分开的卡片结论表
  // （OPERATION_HEADLINE + headlineFor）。**刻意不与 RUN_HEADLINE 合并**：
  // 那张表每句话的主语都是「工作台」，套到恢复上会写出「工作台已启动」这种
  // 与操作毫无关系的结论。
  // 158 → 194：阶段序列与阶段进度统计（审查 P2-03）。「完成 N / M 个阶段」
  // 里的 M 必须来自一份显式的阶段集合——拿事件条数当分母，一个阶段推三条
  // 事件就会显示成「完成 3 / 6」而实际只走了两步。
  // 194 → 206：`runAgeLabel` / `staleRunHint`——「这条记录有多旧」与「它已经
  // 过期到只代表当时」。三个诊断视图共用一份，抄一份就会漂。
  // 206 → 215：`collapseSupersededRunning`——折叠被终态取代的「进行中」。
  // 放这里而不是 `RunTimeline.vue` 是因为它是纯函数、可单测，且**五个诊断
  // 视图的阶段集合也只有这一个出处**；塞进组件就得对着源码字符串断言。
  // 保留它那行 `durationMs` 等于展示一个错义的秒数（它量的是上一阶段的
  // 尾巴），折叠同时把行数对齐 `STAGE_SEQUENCES` 的阶段数。
  'ui/src/diagnostics/diagnostic-labels.js': 215,
  // 诊断层的独立样式。刻意不进 theme.css：后者是反棘轮文件（只许越来越
  // 小），而诊断层是自成一块的样式，抄进共享文件会让"哪段样式属于哪层"
  // 变得看不出来。
  // 250 → 253：浮层层级阶梯（审查 P1-01）。见下面那段注释——它既是值也是
  // 五个叠面顺序的唯一说明。
  // 253 → 261：「更多」弹层的宽度约束与逐字断行禁令（窄窗实机发现的塌缩，
  // 复现过：EP 的 .el-popper 默认 min-width:10px + break-word，中文被逐字断开）。
  // 261 → 271：概览控制塔的两列排布 + 单行省略（480px 实测省 223px）。
  // 与 ControlTower.vue 删掉的三行重复读数是一件事的两面：删掉的是内容，
  // 这一段是新布局本身。diagnostics.css 远低于 RATCHET_THRESHOLD，只许下调
  // 的棘轮对它不适用；控制塔是自成一块的样式，按上面的理由也不该拆到别处。
  // 271 → 274：概览页控制塔三张卡再压一轮（用户 2026-10-06「紧凑一点，减少纵向
  // 空间」）。**只加了 3 行**——三条覆盖各写成一行（.diag-card 远低于阈值，不受
  // 反棘轮约束，所以这里允许上调）。分行写会变成 12 行：门禁的 codeLineCount 只
  // 剥 `//` 与 `/* */`，**独立的 `}` 会按代码行计**，一条两声明的规则分四行写就
  // 多收 3 行。本文件通篇是一行一规则的写法，这里跟着走。
  // 271 → 285：两轮。第一轮 271 → 274 是概览页控制塔三张卡压紧（用户
  // 2026-10-06「紧凑一点，减少纵向空间」），只加了 3 行——三条覆盖各写成一行
  // （.diag-card 远低于阈值，不受反棘轮约束，所以这里允许上调）。分行写会变成
  // 12 行：门禁的 codeLineCount 只剥 `//` 与 `/* */`，**独立的 `}` 会按代码行
  // 计**，一条两声明的规则分四行写就多收 3 行。本文件通篇一行一规则，跟着走。
  // 第二轮 274 → 285：「更多」弹层补 padding / hover / 图标对齐（+11）。
  // 285 → 287：标题行上的小动作按钮（`.diag-card__action`）。+2 而非更多：
  // hover 与 disabled 各写成一行（连注释共 2 条规则 / 2 行代码）。边框、背景、
  // 字体全部显式声明，因为**全局没有裸 button 的默认样式**，不写就是一个
  // 系统灰方块。不动共享的 `.diag-card__aside`（11 处引用，其中
  // DiagnosisShell 把它当标题后缀用，给它加 flex 会改掉那处的表现）。
  // 287 → 288：+1 来自并行会话给控制塔空态收纵向留白（`.diag-empty` 的
  // `padding-block` 覆盖），**不是本轮工作**。提到这里是不想让那行卡在门禁上，
  // 归属写清楚以免日后误记成本轮产物。
  // 288 → 291：R2-P2-04「还有 N 项 / 收起」那三行。它不带 hover 底色也不带
  // 箭头语义色——它长得越像一条待处理的问题，用户越容易把它当成第五条。
  'ui/src/diagnostics/diagnostics.css': 291,
  // 诊断层外壳：覆盖当前面板而非另开窗口（启动失败时用户正要回到日志 /
  // 换端口 / 回退快照，跨窗口拖拽是白费力气）。头部固定
  // [返回] 标题 [主操作]，标题单行省略以守住 480 宽。
  // 100 → 108：多两种 kind 的路由（恢复 / 排查共用 OperationDiagnosis），
  // 以及一个 NEEDS_FETCH 判定——恢复与排查的记录是「做完之后」才成型的，
  // 头部那个刷新按钮对它们同样有意义。
  // 108 → 136：四个视图的刷新各走各的 loading key 并合成一个头部状态（P2-03），
  // 插件页无报告时退到仅运行记录视图（P1-05）。
  // 136 → 145：下拉的 teleported / placement / popper-class 三个属性。
  // 145 → 151：菜单项从纯文字改成「图标 + 文字」两段（+6）。三项里两项是复制，
  // 只靠文字区分时「复制运行记录编号」与「复制本次诊断摘要」只差最后三个字，
  // 扫读极易点错，而点错的代价是剪贴板多一段用户没要的文本。
  // 151 → 153：R2-P2-02 把三处直接 import 的 load 换成代理（净 +1 是 import 拆行）。
  'ui/src/diagnostics/DiagnosisShell.vue': 153,
  // 诊断层头部的「更多」菜单项与复制逻辑（设计 §2.5.5）。独立成文件是因为
  // 那边只管「头部结构 + 三个视图的路由」，这里是「每页各自有哪些低频动作」
  // 的映射表；混在一起后加一项菜单要重读一遍路由代码才能确认没写错层。
  'ui/src/diagnostics/diagnosis-more-menu.js': 95,

  // 恢复 / 排查的阶段时间线（设计 §4.1）。两种 kind 共用一个组件：它们回答的
  // 是同一个形状的问题——「这次操作停在哪一步、结论是什么、下一步能做什么」，
  // 差异只在头部那句话与阶段的中文名，都在 diagnostic-labels.js 里按 kind
  // 查表，不各写一份模板。这里**不放任何会改变状态的动作**：用户在结论还
  // 不确定的页面上误点恢复，代价是真实的配置。
  // 90 → 99：通用模式（没有候选插件上下文时看的就是一条运行记录本身），
  // 阶段计数改用阶段序列。
  // 99 → 105：证据卡的判据从「有没有 kernelLog」改成「有没有**任何**证据」
  // （审查 R2-P1-03 / R2-P2-06）。插件预检失败与基线失败的运行只带 sandboxLog，
  // 而那恰恰是最需要说清「问题出在哪一步」的一类；用 kernelLog 当门槛等于把
  // 它们的证据卡整块藏起来，页面只剩一句没法点的「请查看下方日志」。
  // 105 → 106：R2-P2-02 同上。
  'ui/src/diagnostics/OperationDiagnosis.vue': 106,
  // 启动与预检共用的阶段时间线。三条硬规则：按 seq 排（不按字符串）、
  // 默认只展开第一个失败阶段、状态不只靠颜色表达。
  'ui/src/diagnostics/RunTimeline.vue': 90,
  // 启动诊断页：结论 + 归因 + 下一步 + 阶段时间线 + 证据索引 + 主动作。
  // 重试**复用** store 的启动编排而不是另写一份，否则两处会各自漂移。
  // 顶部主动作按设计 §5.2 只留一个：成功去开工作台，失败再试一次。
  // 135 → 138：阶段计数改用阶段序列，事故 / 工作台两个动作改走动作代理（P2-02）。
  // 138 → 139：R2-P2-02 同上。
  'ui/src/diagnostics/StartupDiagnosis.vue': 139,
  // 插件安全诊断页。呈现**已有**预检结论而不重跑一次：用户在意的是刚才
  // 那次安装，重跑沙盒要几十秒，而结论并不会因此改变。
  // 四块：候选信息卡（来源 / 完整性 / 物化方式 / 影响范围）、五段实验流程
  // 时间线、通过但有告警、折叠的证据与风险。
  // 220 → 221：详细事件收进折叠区（P2-06）、恢复按钮按有无快照 id 改名（P1-04）。
  // 221 → 250：两阶段的「应用变更」主操作 + 禁用理由，以及「验证通过 ≠
  // 已安装」那两句（审查 P1-03）。判据与文案都在模板里说清，是因为用户是
  // 在这里第一次看见「预检没有改动当前实例」这个事实。
  // 250 → 254：头部补时刻与过期提醒（与另外两个诊断视图同一份口径）。
  // 254 → 262：「应用变更」那个 tooltip 改用 `:disabled`（见
  // `ui/test/tooltipEmptyContent.test.js`：空 content 仍会弹空壳气泡）。属性多了
  // 一行就得拆成多行写，加上把**为什么不能用空串**写进注释——这条注释就是
  // 防下一个人再抄回 `canApply ? '' : reason` 的，压缩它等于把 bug 的成因删掉。
  'ui/src/diagnostics/PluginDiagnosis.vue': 262,
  // 预检的来源类型与完整性摘要映射。**纯函数、无依赖**——数据加载刻意不
  // 在这里：把它塞进「映射表」会让这份表变成半个 store，下次有人加字段就会
  // 发现「反正这里已经能 invoke 了」。
  'ui/src/diagnostics/precheck-labels.js': 45,
  // 内核状态诊断页。**刻意只读 store 里已有的快照**——用户点「查看状态」
  // 的语义是"告诉我这份状态是什么意思"，不是"再探一次"。字段只取后端
  // 真有的（版本 / 运行 / 端口 / 数据目录），不编 pid 与运行时长。
  // 80 → 94：「刷新状态」此前是个**无操作**按钮（审查 P1-06）——点了没反应，
  // 而用户以为自己已经拿到新状态。现在它真的重读，并显示读到的时刻；
  // 读失败时保留上一次的值并说明它可能过期（设计 §9.4）。
  'ui/src/diagnostics/KernelStatusDiagnosis.vue': 94,
  // 概览页控制塔：需要关注 / 系统健康 / 最近操作。独立成组件是因为概览页
  // 已 900 行且在反棘轮上，而这三块与「当前内核」卡片是并列关系。
  // 系统健康按设计 §7.2 列全七行（内核 / 运行时 / Node / 插件接线 / 技能
  // 注册 / 日志 / 快照）。`unavailable` 是一等状态：读失败时**不能**显示成
  // 「正常」——用户看到一片正常会以为系统没事，而真相是这一项没读到。
  // 210 → 215：「最近操作」卡标题行右侧加「清除记录」按钮。入口挂在标题行而
  // 不是记录行内：它针对的是**整段历史**，贴在某一条旁边会让人以为只删那一条。
  // 215 → 261：R2-P2-03（日志清单四态 + 重试 + 失败原因）与 R2-P2-04（首屏三条
  // + 显式 rank 优先级 + 「还有 N 项 / 收起」）。两处都不是排版调整：加了首屏
  // 上限之后**顺序就成了这件事本身**，所以 rank 必须显式写出来——按 push 顺序排
  // 的话，「未找到 Node.js」恰好是最容易被挤掉的那条。
  'ui/src/diagnostics/ControlTower.vue': 261,
  // 安装预检的两段式事务：中央库字节级快照与回滚、基线差分判定、
  // 提交（物化 + 接线）与报告装配。放在独立文件而不是塞进已 2964 行的
  // plugins.rs，是为了两件事：插件模块读不懂、预检想复用到技能上也
  // 无从下手。plugins.rs 侧只暴露 `store_file` 一条可见性缝。
  // 450 → 451：两阶段之后 `commit` 换成了 `plugin_apply`（应用前打快照 +
  // 走生产安装路径 + 重查守卫）。
  // 451 → 465：基线失败路径补 `preserve_evidence`。此前取证**只在 fail 路径**
  // 调用，沙盒目录随 Drop 删掉，于是「最需要线索的那次」恰好没有线索——2026-10-06
  // 排查预检基线失败时，报告里的 evidence 是空的、磁盘上也什么都不剩。
  // 一次取证调用换一条可查路径，不拆模块。
  // 465 → 469：`fill_source_info` 多填一个 `apply_spec`（第二阶段用的来源契约）
  // + `strip_url_credentials`（摘 URL 里的 userinfo，14 行）。凭据必须摘而 spec 必须
  // 能原样装回去，两条需求只能靠这个函数同时满足。
  // 469 → 486：抽出 `finish_with_evidence`（R2-P1-03）+ 基线失败分支改走它。
  // 两条失败分支过去各写一遍收尾，而基线那条**漏挂了证据**：路径只进 shell
  // event，于是运行记录没有 sandbox_log、诊断页没有证据卡可显示——最能说明
  // 「问题与候选插件无关」的那次失败反而点不开日志。抽助手不是为了少写几行，
  // 是让「漏挂证据」变成写不出来的错误：两条分支只能通过这个出口返回。
  // 486 → 496：新增 `StoreRestore` 守卫（2026-10-07 用户实测「预检通过后插件
  // 却被装上了」）。预检把候选包装进**真实中央库**（只有目标实例换成沙盒），
  // 失败的两条出口都调了 `rollback`，唯独「装上并通过」那条没调，于是中央库
  // 留在记账状态，而面板的「已安装」判据读的就是它。改成 `Drop` 守卫：还原
  // 从「每条 return 前记得补一行」变成结构事实，新加出口不必记得。这 10 行买
  // 的是「两阶段契约不会再漏一次」——比它在文件里占的位置值钱得多。
  // 本文件 486 < RATCHET_THRESHOLD 600，不受反棘轮约束。
  // 496 → 503：`SANDBOX_CREATE` 的**成功**终态。该阶段原先只在失败路径补
  // 终态，于是「1. 创建沙盒环境」在通过路径上永远转圈，而摘要已经写了
  // 「已完成」。`check-invariants` 第 21 项钉住「发了『正在…』的阶段必须有
  // SUCCESS 出口」——它把 FAILURE 也算数的话，正好对着自己要去防的洞说通过。
  'src-tauri/src/plugins/precheck.rs': 503,
  // 插件预检的 Tauri 命令壳：取证（plugin_precheck_install）+ 应用
  // （plugin_precheck_apply）+ 它们共用的 `run_precheck_command`（node / pnpm
  // 准备、长任务通道、生命周期锁）。从 commands.rs 搬出——那份在反棘轮上，
  // 而两条命令 + 一个封装本来就是一个自成一块的单元。
  'src-tauri/src/plugins/precheck_cmd.rs': 103,
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
  'src-tauri/src/migration/wizard.rs': 830,
  'src-tauri/src/skills/manage.rs': 1490,
  // 「被更高优先级的根盖住」的处置：把盖住的那几份**改名让路**（只改名不删）。
  // 独立成文件的两条理由：① skills.rs 只剩 10 行余量，这条能力（判据 + 落点
  // 复核 + 改名 + 4 个测试）放进去只能靠调数字过门禁，而它是 1480 行的大文件；
  // ② 判据本身在 paths.rs（纯函数，只读），执行在这里——**读判据的人**与
  // **动手的人**分开，与 snapshot.rs / restore.rs 那对「回退点可信吗」/
  // 「点确认会发生什么」是同一种分法。66 行代码（+200 行含文档与测试）。
  'src-tauri/src/skills/skill_shadow.rs': 70,
  // 「活动视图里被同名条目占住、启用必然失败」的判据 + 出路（2026-09-30，新建）。
  // 与 skill_shadow 同一形状但**不是同一件事**：那条处理的是内核 rank 更高
  // 的根（~/.dsh/skills、~/.agents/skills）盖住壳管理的条目；这条处理的是活动
  // 视图**自己**的位置被一份不归技能库所有的同名条目占住，于是 `ensure_entry`
  // 拒绝启用。本机实况：活动视图里是 humanizer v3.0.0 的旧副本（普通文件、
  // 无指纹），中央库已更新到 v3.1.0，点启用只弹出一个只有「关闭」的对话框。
  // 独立成文件的两条理由：① skills.rs 只剩 10 行余量，而判据 + 改名 + 8 个
  // 测试放不进去；② 它的判据是 `ensure_entry` 的**拒绝条件本身**
  // （`!entry_is_owned && !identical_unowned_copy`），判据与动手分开放才能让
  // 「按钮出现在哪里」与「拒绝发生在哪里」读起来是同一份规格。命令壳也在这里
  // ——commands.rs 已到 2110/2110，加命令只能调数字，而调数字是 AGENTS.md 禁止的。
  // 188 行是什么：判据骨架 16 + `conflict_at` 12（逐条对齐 `ensure_entry` 的拒绝
  // 条件）+ 面板文案 16 + 版本证据 38（只用于解释，不参与判定）+ 改名动作 40
  // （用户可见的进度文案与「落点已不在活动视图内就跳过」的复核占了大头）+ 命令
  // 壳 6 + 结构体与导入。判据与动手**故意**留在同一个文件：拆开之后「按钮出现
  // 在哪里」与「动手动哪些文件」就分居两处，而那种分叉的后果是按钮点下去动了
  // 判据没点名的文件。它离 800 行硬顶还有 600 多行，与 verify.rs(120)、
  // bisect_cmd.rs(140)、harness_window.rs(195) 同一量级。
  'src-tauri/src/skills/skill_conflict.rs': 190,
  // 技能 frontmatter 的解析（2026-09-30，新建）。修的是 `description: |` 这类
  // YAML 块标量被读成字面量 `"|"` —— store.json 里存下 `"description": "|"`，
  // 面板 tooltip 显示的也是 `"|"`。**它几乎不报错**：`"|"` 是非空字符串，技能
  // 照样被接受、照样能启用，只是说明全丢了。
  // 独立成文件的三条理由：① skills.rs 反棘轮「只许下调」，块标量那套状态机
  // 放不进去；② frontmatter 有**两个**消费者（包扫描 + skill_conflict 的版本
  // 证据），各写一份解析器就会分叉，而分叉出来的那份会让「面板显示的版本」与
  // 「文案里说的版本」对不上；③ 解析器是纯函数，拆出来后能单独测，不必拉起
  // 整个技能栈。搬完之后 skills.rs 1486 → 1454（反棘轮方向），skill_conflict.rs
  // 188 → 155（一套 frontmatter 解析，不是两套）。
  'src-tauri/src/skills/skill_frontmatter.rs': 205,
  // 跨壳装包活动信标（2026-09-30，新建）。为什么它该独立成一个文件而不是塞进
  // harness_window.rs：**写**这一侧在 kernel.rs 的装 / 删两端，**读**那一侧在
  // harness_window.rs 的看门狗里，两边都不肯为了对方把自己长成一个什么都管的
  // 模块——正是 AGENTS.md 里 `pkg.rs` / `state.rs` 那条「跨模块重复先提共享层」的
  // 同一刀法。内容与路径解析都不复杂（46 行代码 + 文档与 2 个测试），复杂的是
  // 「它为什么可以是唯一一处跨壳可变数据」那三条自律，写在 paths.rs 的路径函数上。
  'src-tauri/src/kernel/package_activity.rs': 80,
  'src-tauri/src/plugins/patches.rs': 1250,
  // B 类日志 family/instance_id 接入：kernel_log_spec / install_log_spec /
  // current_kernel_log_path / install_version / install_version_into 加形参；
  // attach_log_drainers 改用 family + id。start() legacy 单实例路径硬编码
  // (DSH, "default")——P8 UI 决策后由真实 instance_id 替换。约 +11 行。
  // 1290 → 1380：内核安装依赖锁步对账（scan_dsh_version_skew /
  // write_kernel_workspace_yaml / write_kernel_stub / 安装二遍钉版）。
  // 2026-09-29：这三个函数连同「上游漏发精确钉版时降级重试」一起搬进
  // kernel_deps.rs，kernel.rs 同步从 744 回到 721 行；预算数字不动。
  'src-tauri/src/kernel/lifecycle.rs': 1380,
  // 2026-09-29 新增（265 行代码，其余是解释这次事故的文档）。它回答
  // 「内核安装时，每个官方子包究竟装哪个版本」这一个关注点：写 stub /
  // pnpm-workspace.yaml 的 overrides、装完扫锁步错位、以及 pnpm 报
  // ERR_PNPM_NO_MATCHING_VERSION 时的降级兜底（2026-09-29 实测内核
  // 0.2.0-rc.2 依赖的 @deepseek-ai/dsh-client-ui-settings-account@0.2.0-rc.2
  // 在官方与镜像 registry 上都不存在，pnpm 直接拒绝解析，整条安装失败）。
  // 三者共用同一份 overrides 数据，拆开只会让「谁在决定钉版」变得看不懂；
  // 而它们都不属于 kernel.rs 的「起进程 / 判成败」，继续堆在那里只会推高
  // 一个只许下调的文件的实际行数。
  'src-tauri/src/kernel/kernel_deps.rs': 300,
  // 2026-09-29 新增（27 行代码，其余是解释「为什么 eprintln 不算数」的文档）。
  // 壳是 GUI 应用，stderr 在 Windows 上没有任何去处，而**只有后果、没有原因**
  // 的动作恰恰只走 stderr（工作台窗口自动重载、给 pnpm 降优先级）。这个模块
  // 给它们一个落盘出口：沿用按日轮转的约定，因此自动出现在「查看日志」面板。
  // 单独成文件而不是塞进 process.rs，是因为 process.rs 只剩 9 行预算，且
  // 「事件落盘」与「子进程执行」是两个关注点。
  'src-tauri/src/shell/shell_events.rs': 80,
  // 2026-09-29 新增（36 行代码，其余是这次事故的取证结论）。装内核时 pnpm
  // 硬链接数万文件 + node-gyp 编译，把 CPU 与磁盘打满，同机另一个壳的
  // WebView2 渲染进程被打崩，表现为工作台莫名 reload 并撞上内核的启动顺序
  // 竞态。把**装包工具**降到 BELOW_NORMAL 让它们让路，保留双壳并行调试——
  // 比「另一个壳的工作台在跑就禁止装内核」代价小得多。只降 pnpm/npm/npx：
  // Node 探针降优先级会误报「原生模块加载失败」，那是更糟的假阴性。
  'src-tauri/src/shell/child_priority.rs': 80,
  // 1180 → 1157：本地时间的换算与格式化搬去 shell/localtime.rs（审查 P2-04）。
  // 起因是运行记录 id 的时刻算错了：日期按本地偏移、时刻按「当前时间 - 现在」
  // 估的偏移，在东八区两者差 8 小时。而根因不只是那个估算——**全仓有三处
  // 各写一份本地时间**（日志文件名的日期戳、事件日志的时刻、runId 的时刻），
  // 收成一处是唯一能加东西又不违反反棘轮的地方。
  'src-tauri/src/shell/process.rs': 1157,
  // 1050 → 1090：会话标题改为订阅 `session/control`（baseline 播种 + 标题投影帧
  // 保鲜 + 老内核退回 session/list 快照），这部分逻辑与 Center 同生共死，拆出去
  // 只会把状态机切成两半。详见 docs/features/notifications/notification-design.md §3.3。
  // 1090 → 1170：通知带「最近一轮对话」——Center 增 `last_turns` 表
  // （turnOutline 投影末项；baseline / 投影帧 / session/list / api-session/added
  // 四个来源共用一套吸收逻辑）+ CompletedTask 增 lastPrompt / lastResponse +
  // 通知正文带最近对话与「完成于 HH:MM」。与标题同源同命，不拆分。
  'src-tauri/src/notify/task.rs': 1170,
  // 2026-09-28 新增：系统通知通道的可用性判定（Windows「设置 → 系统 → 通知」
  // 被关时 `ToastNotifier::Show` 仍返回 S_OK 而气泡不出现，macOS 未打包构建
  // 同样投递不出）。单独成文件而不是并进 notify.rs，是因为它与状态机无关——
  // notify.rs 只回答"要不要弹"，这里只回答"平台这一关卡没卡住"；混在一起会
  // 让一个 1170 行、只许下调的文件再多一处平台分支。约 56 行代码（其余是解释
  // 这两种静默失败的文档）。
  'src-tauri/src/notify/notify_gate.rs': 120,
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
  'src-tauri/src/notify/activate.rs': 280,
  // 2026-09-30：工作台窗口的**手动**逃生口（`harness_force_reload`）。看门狗
  // 只在加载事件上判据，而 WebView2 渲染进程被打崩时页面早就 `Finished` 过，
  // 那条判据永远不会触发，`reload()` 又落在一块死掉的文档上（用户原话：
  // 「reload 后，还是会黑屏」）。这条命令换掉整个窗口——壳侧唯一能换掉渲染
  // 进程的动作。**被反棘轮逼出来的**：`commands.rs` 2090/2110，放进去只能
  // 上调数字，而调数字是 AGENTS.md 明确禁止的反应；`bisect_cmd.rs` /
  // `home_recovery_cmd.rs` 是同一处理由。生产代码约 40 行 + 两个把拒绝文案
  // 钉死的单测（拒绝路径的「说法」是用户在故障里唯一能拿到的东西）。重建
  // 动作复用 `harness_window::recreate`，没有复制第二条建窗链。
  'src-tauri/src/harness/harness_cmd.rs': 90,
  // 官方对话窗口的常量、页签表、布局算术与重排判据（87 行，从 commands.rs
  // 摘出）。**为什么该独立**：这些是「官方对话有哪些页签、窗口多大、什么
  // 事件会让子视图 frame 失效」这一组纯窗口事实，不碰数据目录、不读设置、
  // 不发 IPC——它们过去混在 commands.rs 里，而那份文件是全仓最大的命令层，
  // 已经顶在反棘轮上限上。「官方对话的尺寸与页签」原本要在一份两千行的命令
  // 文件里翻才能找到；摘出来之后它有了一个能一眼看全的落点。连带搬走的
  // logical_window_size 只有这一个调用方（relayout_official_chat），跟着走
  // 还顺手消掉了 harness 反过来依赖 commands 的那条边。
  'src-tauri/src/harness/official_chat.rs': 87,
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
  // 190 → 195（2026-09-30，装包活动标记那一层）：**先把上一条记的债还了一部分**
  // ——阈值策略（`ACTIVITY_LOAD_CAP` / `effective_load_timeout` / 三个测试）整体搬进
  // package_activity.rs，因为「装包期间看门狗该等多久」属于**装包活动**这个关注点
  // 的策略，不是窗口管理的一部分。搬完之后本文件 192 行，净增的 2 行是
  // `should_reload` / `should_recreate` 多收一个 `timeout` 形参（判据本身不该知道
  // 装包信标的事，阈值由调用方算好传进来）。
  // 「拆回 120 附近」那笔债**仍未还**：175 行那次顶高的成因（instance 侧跨壳判定）
  // 早已搬走，剩下的 `recreate` / `open` / `build` 三条建窗链仍在本文件内。那是
  // 下一次碰这块时该做的事，不在本次范围。
  'src-tauri/src/harness/harness_window.rs': 310,
  // install_isolation.rs（2026-09-30 傍晚，新建）：回答「这棵已安装的内核树
  // 是否仍与其他目录共享 inode」——采样读硬链接数（Windows 走
  // GetFileInformationByHandle，Unix 走 stat 的 st_nlink）。独立成模块的理由：
  // 跨平台 FFI + 采样策略自成一体，被 kernel.rs 三处（版本列表的
  // shared_storage、横幅门控、装 / 删提示）共用，且有自己的测试面（临时树里
  // 造真实硬链接来钉两个判定方向与保守方向）。塞进 kernel.rs 会让 1380 行的
  // 它再长 90 行，而这不是「安装」逻辑，是「安装的物化方式」的探针。
  'src-tauri/src/kernel/install_isolation.rs': 90,
  // 工作台未发送草稿的存续（2026-09-30，新建）。它该独立成一个文件而不是塞进
  // harness_cmd.rs：**它是数据**（一段纯文本 + 落盘位置 + 取走即删 + 过期 +
  // 「既没字也没图即作废」），harness_cmd.rs 里全是 Tauri 命令与「该不该动手」的判据；
  // 它有 12 个测试，其中六个钉的是「什么时候**不该**把草稿交出去 / 不该留着它」。
  // 把数据与命令分开，下次改取走语义时不必翻命令层。注入脚本 `harness-draft.js`
  // **不计入预算**（check-code-budget 只数 src-tauri/src 的 .rs 与 ui/src），
  // 它的 22 个测试在 `ui/test/harnessDraft.test.js`（已登记进 test:ui）。
  //
  // 70 → 130（2026-09-30，加图片草稿）：**图片的字节与上限搬去了 harness_media.rs**，
  // 本文件只留「什么时候有、什么时候作废」这层生命周期；搬完之后剩下的增长是「只发图
  // 不发字也算有东西」「图没了也要作废」这两条判据与它们的测试。按规则 ① 的正确反应
  // 是把新逻辑拆出去而不是调数字——**拆过了**，这里调的是拆完之后的余量。
  'src-tauri/src/harness/harness_draft.rs': 130,
  // 草稿图片的字节与上限（2026-09-30，加图片草稿时新建）。独立成文件的理由是
  // **它回答的不是同一个问题**：harness_draft.rs 答「草稿什么时候存在、什么时候作废」，
  // 这里答「一段二进制怎么进出这个进程、存不下时丢哪几张」。混在一起的后果是 base64
  // 的位运算淹掉草稿本身的语义，而调上限的人分不清自己动的是哪一半。它与
  // harness_draft 共享的只有那个落盘目录的路径，所以自己持有它。
  // 6 个测试：base64 的已知向量与三种补位、写入读回逐字节不变、超限按「先到先留」
  // 丢弃并报出张数、坏 base64 整张丢掉、换内容不留旧图。
  'src-tauri/src/harness/harness_media.rs': 150,
  // 2026-09-30：出网路由（`net_proxy.rs`，新建）。它回答的唯一问题是
  // **访问 GitHub 的请求该走哪条路**：先本机系统代理（环境变量 → Windows
  // Internet Settings 注册表 / macOS `scutil --proxy`），代理不通再直连。
  // 为什么必须独立成文件而不是塞进 updater.rs：① 它与「更新」无关——pnpm
  // 与 WebView2 各自的代理来源是另外两套机制，但**读法与回退策略**只有这一
  // 份，再来第二条出网路径时应当复用它而不是复制；② updater.rs 已经 680 行
  // 生产代码，代理探测（两个平台的原生读法 + 两份纯解析函数 + 它们的测试）
  // 放进去会顶高一个只许下调的邻近文件。平台相关的读法各自带 cfg，纯解析
  // 函数（注册表 `ProxyServer` 两种写法、`scutil` 字典转储、地址归一化）
  // 可以在任何平台直接测。
  'src-tauri/src/pkg/net_proxy.rs': 180,
  // get_status 的可关闭性能采样与状态快照编排独立成模块：v0.5.0 之前默认开启，
  // 之后默认关闭；开启时只负责结构化记录，不把观测逻辑继续塞进命令层。
  'src-tauri/src/diagnostics/perf.rs': 160,
  'src-tauri/src/diagnostics/guard.rs': 940,
  // 从 guard.rs 拆出的「证据判读」层：只回答「这一行指向内核还是指向某个插件」，
  // 不回答「该怎么处置」。独立成文件有两个理由：① 判据的内核侧（命名空间锚定 +
  // 多成员组合路由的拒绝规则）与插件侧（bundle_member / has_segment_path）必须
  // 并排可读——把两套相反的边界规则隔着一个 900 行的文件，正是当初它们被写成
  // 同一个宽进出的来源；② guard.rs 是只许下调的反棘轮文件，而 2026-09-29 新增的
  // 内核槽位装配不变量判据必须落在它够得着的地方。60 行，离 800 行硬顶很远。
  'src-tauri/src/kernel/kernel_evidence.rs': 80,
  // 2026-09-29：插件中央库的一次性目录搬迁（`dsh-plugins/` → `plugins/dsh[-dev]/`）
  // 从 plugins.rs 拆出。拆的理由有两条，都不是「文件太长了」：① 搬迁只关心旧目录
  // 在不在、要不要搬、搬失败怎么办，不读 store.json、不碰物化指纹——中央库的条目
  // 逻辑在 plugins.rs，目录级的一次性动作在这里，两者是不同层；② plugins.rs 是
  // 反棘轮文件（只许下调），把新逻辑留在那里只能靠调数字过门禁，而调数字正是
  // AGENTS.md 禁止的反应。生产代码约 75 行。
  'src-tauri/src/shell/store_relocate.rs': 120,
  // 2026-09-29：「搬错实例」的遗留数据回收（sessions / attachments）。2026-09-28
  // dev 壳在闸门落地前 15 分钟把 `~/.dsh` 的历史会话并进了 default-dev，release
  // 侧工作台从此是空列表，而 `~/.dsh` 已空、搬不动第二次——instance.rs 只写了
  // 「防再犯」的闸门，没有「已犯怎么办」的回收路径。独立成文件的理由同上：扫描 /
  // 复制 / 合并的主体不碰实例注册表与生命周期，留在 instance.rs 只会顶高那条
  // 540 行的预算。生产代码 185 → 408：多出来的是 `workspace.json` 的合并——会话
  // 列表由它决定，只搬目录的话文件在磁盘上、工作台里仍然不显示（2026-09-29 本机
  // 实测）。408 行离 800 行硬顶还有一半距离。
  'src-tauri/src/migration/home_recovery.rs': 430,
  // 同一条回收路径的 Tauri 命令壳（scan / recover 两条）。与 commands.rs 其余
  // 70 多条命令没有共享逻辑，留在那里只能靠上调数字过反棘轮——`bisect_cmd.rs`
  // 是同一处理由。生产代码约 30 行。
  'src-tauri/src/migration/home_recovery_cmd.rs': 60,
  // 2026-09-29：实例注册表按壳模式**分文件**的一次性拆分（`state/instances.json`
  // / `state/instances-dev.json`）。之前两个壳共用一个文件，于是：跨进程读-改-写
  // 没有任何序列化（互斥只是进程级 Mutex）、dev 壳删/建实例会改到 release 的列表、
  // 以及「本壳服务哪个实例」这类指针写在共享文件里。拆开之后这三类跨壳竞态从根上
  // 消失，instance.rs 里原有的「只有 release 能写共享指针」那套防御也随之撤销。
  // 独立成文件有两个理由：① 拆分规则（认领 / 让位 / 顺序无关 / 幂等）是一套独立
  // 的迁移状态机，塞进 instance.rs 会顶高那条已经贴着 540 的预算；② 它只服务
  // 启动期的一次性动作，与实例生命周期（建/启/停/删）无关。生产代码 72 行。
  'src-tauri/src/shell/registry_split.rs': 110,
  // 430 → 450：概览页「刷新工作台」动作 `forceReloadHarnessWindow`（+19）。它与
  // 既有的 `openHarnessWindow` 同属工作台窗口那一组，放这里而不是新开一个
  // `overview.js`：概览页其余 20 来个动作也都在这个文件里，为一个按钮单开
  // 一个共享层才是真分裂。本条预算基线远低于 RATCHET_THRESHOLD，上调是允许的。
  // 455 → 462：`runRefreshAll` 逐个数据源交回成败（审查 R2-P1-04）。过去只有
  // 一句 catch + toast，调用方拿到的永远是 undefined，于是调用方只能无条件
  // 执行「读取成功了」。插件 / 技能走 createStatusSource（自己吞异常并返回
  // 布尔值），所以读返回值而不是 catch；get_status 会抛，单独接。
  'ui/src/store.js': 462,
  // 多内核改造 P0：新路径模块（paths.rs）。包含 ShellMode、xlink_home、shell
  // /kernels/skills/state/cache 解析、legacy resolver、id 校验与基础数据
  // 模型——是后续 P2–P8 的依赖根，必须单独占预算，避免被 plugins/skills
  // 这两个大文件吞噬。
  'src-tauri/src/shell/paths.rs': 600,
  // P2：实例注册表 + 锁 + 端口分配 + runtime/pid 文件读写 + DSH home
  // 子目录创建 + 默认实例迁移钩子。约 470 行（含 11 个测试 setup 与
  // 路径解析注释）。
  // 2026-09-23：新增内核 home 一次性搬迁（~/.dsh → 实例 DSH_HOME）——递归
  // 并入 / 原子移动 / 跨卷回退 / 幂等标记，~+64 行，470 → 540。
  'src-tauri/src/shell/instance.rs': 540,
  // P3：KernelAdapter trait + AdapterCapabilities + DshAdapter 首实现
  // （DSH_HOME / DSH_PROFILE 注入、profile/package.json 与 cordis.patch.yml
  // 模板、resolve_install_dir 双查找）。约 430 行（含 9 个测试）。
  'src-tauri/src/kernel/kernel_adapter.rs': 620,
  // profile 清单（profiles/<profile>/package.json）的初值、修复与模板 bundle
  // 表。独立成文件是因为它有**两个**写入方——kernel_adapter 建实例时落初值、
  // plugins/center 接线时改写——而清单的形状同时是内核的启动契约，不只服务
  // 于接线：缺 `dsh.profile.bundles` 时内核解析出零个插件就 exit 0 且不打日志
  // （2026-10-06 实测）。两边各写各的形状正是这个 bug 的成因，所以它必须
  // 有一个跨两侧的唯一落点，而不是各自 inline 一份模板。
  'src-tauri/src/kernel/profile_manifest.rs': 190,
  // 模型用量统计（usage.rs）：内核 session 多帧 zstd 流的增量扫描（ruzstd
  // 帧级解码 + offset checkpoint + 坏帧停驻）、按「天 × 模型」预聚合与 90 天
  // 保留剪枝、汇总视图派生与 get_model_usage / open_usage_window 命令，
  // 外加主窗拖动吸附跟随（dock_x / dock_y / docked_position / 事件监听）。
  // 扫描是唯一数据通路，与账目结构同生共死，不宜再拆。生产代码约 665 行
  // （测试另计）。
  'src-tauri/src/usage/local.rs': 700,
  // 模型用量统计（usage.js）：窗口/卡片状态动作 + B/M/K 单位、热力图分级
  // 与周列对齐、趋势堆叠、饼图扇区等纯展示函数（node --test 直测）。
  // 160 → 180：实际落地比估的多（数字格式、模型配色 ring、热力 tooltip
  // 阈值再加注释）；另含 heatLevels / heatmapColumns 的 doc 注释。
  // 180 → 190：修「概览与用量窗口显示两个今日用量」（2026-09-30 用户实测：
  // 卡片 0 tokens、窗口 12.09M）。两处新增都是这个 bug 的必要组成：
  //   · `todayUsage(data)`——「今日」的唯一口径。窗口此前自己取
  //     `sliceDays(days, 1)` 的最后一天，而后端会把晚于今天的异常日期追加到
  //     序列末尾，所以「最后一天」未必是今天；卡片读后端的 `today_tokens`。
  //     同一个数两套算法，迟早显示成两个数。
  //   · `setUsageAutoRefresh(on)`——概览卡片此前只在 onMounted 拉一次就再也不
  //     更新，窗口却每次打开都 force 重扫还带手动刷新。现在挂载期间 60s 对一次
  //     账，卸载即停；间隔取 TTL 本身，闲置时一个请求都不发。
  'ui/src/usage/usage.js': 190,
  // 模型用量统计（UsageWindow.vue）：独立窗口根组件（open_usage_window 弹出，
  // ?usage=1 挂载）——摘要卡 + 热力图 + 堆叠柱状趋势 + 环形图/列表与
  // scoped 样式，全 CSS/内联 SVG 不引图表库。
  // 560 → 700：落地比估的多（环形图 + 列表卡片 + 局部样式），手绘 SVG 段
  // 不可压缩；用法与 LogViewerWindow 同模式（独立窗口根组件 + scoped CSS）。
  // 700 → 780：范围切换 + 时间范围档位（~+50）与热力图/趋势两个共享 hover
  // 明细浮层（~+60）——都是展示层增量，拆文件只会让浮层与图形结构分家。
  'ui/src/usage/UsageWindow.vue': 830,
  // 云端套餐用量（subscription.rs）：MiniMax Token Plan / DeepSeek 余额查询、
  // 5 分钟缓存文档（state.rs 容错读 + 原子写）、Key 指纹绑定（换 Key / 清 Key
  // 作废旧条目）、redact_key 脱敏、三态 Key patch、expired 跳过自动刷新、
  // get_subscription_usage / open_subscription_window 命令（生产代码 877 行，
  // 测试另计；含按 provider 定制的凭据失效文案与失败日志集中记录）。查询 +
  // 缓存 + 解析同生共死，不宜再拆；对应设计稿 docs/features/subscription/subscription-usage-design.md。
  // 911 → 896：分目录后模块路径变长（`credentials` → `usage::credentials`），
  // 单行调用点被 rustfmt 折成两行，一处折行乘以几十处调用点就是 +11 行。
  // 修法不是抬预算，是把各模块按 basename 引进文件内，让体内调用点回到
  // `process::epoch_millis()` 这种长度——可读性也一并回来了。
  'src-tauri/src/usage/subscription.rs': 896,
  // DSH 模型凭据只读解析（credentials.rs）：profile cordis.patch.yml 的
  // provider apiKeyEnv 绑定、.credentials.yaml refs、.env 回退层与默认引用
  // 派生。凭据语义与内核对齐只有一处实现，独立成模块供 subscription.rs 复用。
  // 260 → 270：refs 标量统一字符串化（YAML 数字写法如 `KEY: 2` 也是合法值）。
  'src-tauri/src/usage/credentials.rs': 270,
  // 云端套餐用量前端（subscription.js）：状态动作（keep-last-good 显式落地）
  // + 收起态摘要 / 余额行 / 进度条配色 / 重置倒计时等纯展示函数（node --test 直测）。
  // 180 → 210：失效 provider 的「提示 → 隐藏 → 查询成功自动恢复」状态机
  // （localStorage 持久 + 会话首查 force + collectErrors 跳过已隐藏项）。
  // 210 → 220：`balanceText` → `balanceRow`，按 DeepSeek 的 `total_balance` /
  // `granted_balance` / `topped_up_balance` 三个字段分别产出主行、明细行与
  // hover title（只透传金额，不转浮点）；设计稿本就要求逐条展示这三项。
  'ui/src/subscription/subscription.js': 220,
  // 套餐用量独立窗口根组件（open_subscription_window 弹出，?subscription=1 挂载）：
  // 双 provider 分区 + 进度条 / 余额行 + 错误横幅与 scoped 样式（同 UsageWindow 模式）。
  // 340 → 360：查询时间换成刷新 icon 胶囊（紧凑年龄值）。
  // 360 → 370：余额块多一行赠金 / 充值明细（含 flex-wrap 与明细行样式），
  // 与概览卡共用 subscription.js 的 balanceRow。
  'ui/src/subscription/SubscriptionWindow.vue': 370,
  // 2026-09-30：壳自己窗口的右键菜单策略（新建，10 行）。主面板 / 日志 /
  // 用量 / 套餐 / 官方对话页签栏共用这个 SPA 入口，所以「禁右键、留左键复制」
  // 在前端只有这一处落点；工作台与三个官方对话内容 webview 走 Rust 侧的
  // `src-tauri/src/no-context-menu.js`（注入脚本不计入本门禁），两侧的接线由
  // `scripts/check-invariants.mjs` 第 16 项钉住。独立成文件而不是塞进
  // main.js：main.js 是所有窗口的入口且已 140 行，而这一条是**有测试的
  // 行为**（ui/test/noContextMenu.test.js 直接对它造假事件流），塞进入口
  // 就只能对着源码字符串断言。本次 10 行落在总量既有余量内，TOTAL_BUDGET
  // 不动。
  'ui/src/shell/noContextMenu.js': 20,
  // ⑤ ui/src/shell/VersionBadge.vue——分段式版本号徽标：左「纯色底 + tag
  // 图标」（icon prop 两档：accent 蓝底 / black 黑底）、右「暗底 + muted 文字」。
  // **同一个组件被侧栏品牌区与概览「当前内核」卡两处使用**，两处都在说
  // 「这是一个版本号」，各写一份必然漂。独立成组件而不是把样式塞进
  // theme.css：那份是反棘轮文件、只许越来越小，而这条规则只服务这一个组件；
  // 也不塞进 SideBar / OverviewPanel 任一侧的 scoped 样式——那样另一侧就得
  // 复制一份。38 → 53（2026-10-05）：新增 icon prop 变体（prop 校验 + 变体
  // 类绑定 + 黑图形样式块）与字重钉住（概览 .card h2 的 700 粗体此前渗进
  // 徽标，两侧同字号不同字重）；「当前内核」卡最终形态为无底色 + 黑色图形。
  'ui/src/shell/VersionBadge.vue': 54,
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
// docs/features/subscription/subscription-usage-design.md 落地：凭据复用内核模型设置（外壳不收集 Key）、
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
// 34560 → 34680：让「对面装内核」不再打断本壳工作台，**第一层**（2026-09-30）。
// package_activity.rs 新文件 46 行（见 FILE_BUDGETS）+ harness_window.rs +26
// （`effective_load_timeout` / `ACTIVITY_LOAD_CAP`、两个纯函数多收一个 timeout 形参、
// 3 个测试）+ kernel.rs +14（装 / 删两端各打 / 撤信标、`remove_kernel_dir` 拆成一条
// 让守卫与写盘正好夹住信标、`process_is_definitely_gone` 把**已有的**三态存活探测
// 开给信标用——没有第二份实现）+ paths.rs +4（信标路径 + 为「它是唯一一处跨壳可变
// 数据」写下三条自律）。
// 净增的是**一处判据的准确度**：看门狗原来只有「慢」与「死」两个概念，而装包时页面
// 确实只是慢。2026-09-30 那次黑屏的成因不是内核先坏，而是**看门狗在机器最忙的那
// 一刻重载了页面**，重载才是放大器。余量留给第二 / 三层，不在这里一次要满。
// 34680 → 34740：第二层——**重载 / 重建前把没发出去的那段话保住**（2026-09-30）。
// harness_draft.rs 新文件 68 行（见 FILE_BUDGETS）+ harness_cmd.rs +8（两条命令：
// `stash_harness_draft` / `take_harness_draft`）+ harness_window.rs / commands.rs 各
// +1（把 `harness-draft.js` 接进注入链，两处建窗都要接——漏一处就有一个入口不保
// 草稿，而那种漏是查不出来的）。
// 净增的是**一条数据通路**，不是新概念：会话本来就在服务端，重载后还在；丢的只有
// 「还没发出去的那一段」，而它此前完全没有存续。注入脚本（177 行）不计入预算，
// 它的 7 个测试在 ui/test/harnessDraft.test.js。
// **顺序上的一个非显然点**：恢复必须**先在页面上找到能写的地方，再去壳里取**，
// 因为 `take` 是读走即删的（草稿留在盘上会在下次正常打开工作台时凭空冒出来）——
// 顺序反了就会在「页面还没装配好」的那一刻把草稿吞掉。测试钉的就是这一条。
// 34740 → 34790：第三层——**黑屏之后壳自己换窗口，而不是停在黑屏上**
// （2026-09-30）。harness_window.rs +25（`should_recreate_after_fault` 判据 +
// `recreate_after_fault` 动作 + 文档与测试）、commands.rs +11。
// **commands.rs 那一处的形状是被门禁逼出来的，不是随手写的**：它是反棘轮文件
// （只许下调），第一版把判据、设置读取、端口探测、动作全写在命令里，直接顶到
// 2118 行。正确反应是拆出去而不是调数字，于是判据与动作一起搬进
// `harness_window`（自愈链条该待的地方），命令层只留两行：递证据、转发。
// 证据也不再另外克隆一份——`Incident` 自带 `health: Option<HealthReport>`，
// 那是同一份东西，复制一份必然漂移。
// harness_window.rs 195 → 220：本文件基线远低于 RATCHET_THRESHOLD，上调在规则内。
// 上面记的「拆回 120 附近」那笔债**仍未还**，但候选已经收窄成 `recreate` /
// `open` / `build` 三条建窗链——下一步该拆的是它们，不是自愈判据。
// 35000 → 35050 / harness_window.rs 220 → 310：黑屏根因定案后的第四轮
// （2026-09-30 下午）。真机数据推翻了「每进程只重建一次」的闸（重建落在卸载
// 风暴中间、4 秒后又死、额度已尽、用户晾在黑屏上 25 秒），harness_window.rs
// +90：`recreate_when_quiet`（等风停的后台等待者）、预算账本从 `rebuilt: bool`
// 换成「次数 + 冷却」、`on_navigation` 导航观测、以及对应的测试与文档。
// 总量 35050 → 35120 同一笔。反棘轮（plugins.rs / theme.css / commands.rs）
// 一个数字没动；commands.rs 本轮零增长（新命令落在 harness_cmd.rs）。
// 34790 → 35000：更新检查（以及下载更新）**优先走本机系统代理、失败再直连**
// （2026-09-30 用户实测：系统里明明开着代理，检查更新却直连 GitHub 后报
// `error sending request for url`）。净增约 190 行，几乎全在一个新文件里：
//   · net_proxy.rs（新增，见 FILE_BUDGETS）：系统代理探测（环境变量 /
//     Windows Internet Settings 注册表 / macOS `scutil --proxy`）+ 纯解析
//     函数 + 4 个测试。
//   · updater.rs +80：把「按顺序试路由」收成 `run_routes` 一份实现，`check`
//     与 `install` 两条路径共用；新增 `should_try_next`（只回退传输层失败）与
//     `describe_failure`（错误文案点名试过的每一条路）。
// 不是新概念：reqwest 本来就认环境变量代理，缺的是**系统设置**那一半，以及
// 「代理优先、直连兜底」的顺序。合计 34999 行，预算留 50 行余量。updater.rs
// 本身没有可下调的余地（新逻辑全在 net_proxy.rs）。重复区间数不变。
// 35440 → 35700（2026-09-30 晚）：技能「活动视图里被同名条目占住」的判据 + 出路。
// 本机实况：活动视图里躺着 humanizer **v3.0.0** 的旧副本（普通文件、清单里没有
// 指纹），中央库当天已更新到 v3.1.0，点启用只弹出一个**只有「关闭」**的对话框
// ——判据已经精确到具体文件，而可执行的出路不存在。净增约 218 行，分布：
//   · skill_conflict.rs（新增，见 FILE_BUDGETS）188 行：判据就是
//     `ensure_entry` 的拒绝条件本身（`!entry_is_owned && !identical_unowned_copy`），
//     加上改名让路的动作与命令壳。
//   · skills.rs +6：`target_path_in`（落点算术与「活动根在哪」分开，判据才能在
//     临时目录上跑）、`SkillStatus.conflicts`、拒绝文案改由判据所在的模块出。
//   · SkillsPanel.vue / skills.js +24：确认框列出会被改名的路径与来由。
// 不是新概念：`ensure_entry` 的冲突拒绝、`skills::keep_aside` 的改名保留、
// `shell_events` 的落盘、`skill_shadow` 的「判据 + 一个按钮」形状**全部早已
// 存在**——本机这次缺的是把已有判据接到一个可点的按钮上。反棘轮三个文件
// （plugins.rs / theme.css / commands.rs）本轮零增长：新命令落在新模块里，
// 那是 `bisect_cmd.rs` / `home_recovery_cmd.rs` 的同一处理由。合计 35658 行，
// 预算留 42 行余量。重复区间数不变。
// 35700 → 35810（2026-09-30 晚，接上一条）：修 `description: |` 被读成字面量
// `"|"`。净增约 102 行，全部落在一个新文件里：
//   · skill_frontmatter.rs（新增，见 FILE_BUDGETS）205 行：YAML 块标量
//     （`|` / `>` + chomping）加 `metadata.version` 的状态机。
// 同一笔里两个既有文件**都变小了**：skills.rs 1486 → 1454（解析器搬出去），
// skill_conflict.rs 188 → 155（它自己那个只认 `metadata.version` 的小解析器
// 删掉，改用同一套）——「判据有两份实现就会分叉」那条纪律的第一次实际兑现。
// 反棘轮三个文件本轮仍是零增长。
// 35810 → 35840（2026-09-30 晚）：修「概览与用量窗口显示两个今日用量」。
// 用户实测同一份统计两个数（卡片 0 tokens、窗口 12.09M），查下来是两处成因：
// 口径分叉（卡片读后端 today_tokens、窗口取 days 的最后一天）与卡片永不刷新。
// 净增约 30 行：usage.js +10（todayUsage 唯一口径 + setUsageAutoRefresh）、
// usage.rs +2（today_requests，与 today_tokens 同一次匹配算出）、UsageWindow
// 与 OverviewPanel 各 +2、App.vue +4（接上已有的切页 / 可见性刷新钩子）、
// usageStats.test.js +2。check-invariants 第 ⑦ 项之五钉接线——实测把窗口改回
// 旧算法，14 个 usage 单测全绿。
// 35840 → 36010（2026-09-30 晚，src-tauri/src 按功能分目录）。净增约 170 行：
// 11 个 mod.rs（约 60 行）与三个大文件因路径多一级而增加的引用行（见 FILE_BUDGETS
// 里 2026-09-30 那条）。没有新增功能。预算是软上限，按实测登记。
// 36010 → 36080：新增 get_status 性能采样与 diagnostics 编排；v0.5.0 之前默认
// 开启以收集真实性能数据，之后默认关闭，仍可由 DSH_XLINK_PERF 显式覆盖。
// 额外 10 行余量覆盖版本默认判定与显式关闭分支，不改变单文件预算。
// 36080 → 36090（2026-10-01 晚）：性能采样指出的状态轮询热点修复——子进程
// 等待循环从固定 50ms 睡眠改指数退避、lsof 端口参数连写修复、身份判据与
// 端口活体反查拆分（观察路径不再每 2.5s 派生 lsof）。净增 10 行生产代码：
// process.rs +8（两个退避常量 + next_poll_step + 循环两行）、lifecycle.rs
// 净 +2（lsof_tcp_args 与判据拆分主体是挪位置，注释与测试不计入）、
// instance.rs 观察路径换判据只改名。反棘轮文件本轮零增长。
// 36090 → 36100（2026-10-01 晚）：概览「当前内核」活动版本徽标加 git tag 图标。
// 净增 6 行，全在 OverviewPanel.vue：图标 import 1 行、模板里的 el-icon 1 行、
// 胶囊改 .age-pill 同款 flex 结构 3 行 + 图标字号一行式规则 1 行。样式放组件
// 而非 theme.css（后者是只许下调的反棘轮文件，且这是组件私有样式），单文件
// 预算不动。设计理由写进文件头 `//` 区——门禁的 codeLineCount 只剥 `//` 与
// 多行 /* */，写在模板里的 `<!-- -->` 注释是按行计费的。
// 36100 → 36120（2026-10-01 深夜）：Node 冷启动探测下线（perf 首条 refresh
// 的 node 段 497ms）。净增 20 行生产代码：lib.rs +9（warm_node_cache：后台
// 线程预热缓存，与 WebView 加载并行）、node/detect.rs +7（候选去重
// push_unique——PATH 命中与 common_locations 列出同一条绝对路径，同一个
// 二进制此前要派生两次 node --version，壳进程里一次 ~250ms）及调用点。
// 两个文件均不在 FILE_BUDGETS 登记表（老文件），只计入总量。
// 反棘轮文件本轮零增长。
// 36120 → 37000（2026-10-02）：「后台常驻 / 自动启动」。净增 680 行，落在六个文件里：
//   · shell/resident.rs 113（新文件）：跨平台常驻语义 + 托盘/菜单栏共用的菜单接线。
//   · shell/menu_bar.rs 78（新文件）：macOS 菜单栏图标（模板图，cfg(macos)）
//     + 两条测试（模板图必须纯黑 + alpha；1x/2x 两档必须都在）。
//   · shell/autostart.rs 199（新文件，生产部分）：登录项读写（macOS LaunchAgent
//     plist / Windows HKCU\...\Run）+ 三条 Tauri 命令 + 启动判定。其中
//     `should_launch_kernel_given` 是为可测性抽出的纯函数——不抽就测不了：
//     判据第一个条件读磁盘设置（测试机上默认关），删掉第二个条件测试照样绿，
//     2026-10-02 首次反向验就是被这一点骗过去的。
//   · ui/src/shell/autostart.js 84（新文件）：与通知设置同形状的状态与动作。
//   · lib.rs +40：关闭语义从「按平台分叉」改成「一律收进后台」（净减，但补了
//     自启隐藏与自启拉起内核的两段）、start_kernel 拆出无 Channel 的主体。
//   · commands.rs -1：start_kernel_blocking 是纯搬迁，净增只有 minimize_shell 的
//     文档与 merge_settings 一行。
// 换来的东西：两平台关闭语义统一（此前 macOS 关窗即退出）、macOS 有了常驻入口
// （此前完全没有）、开机自启可配。**反棘轮文件本轮零增长**——theme.css /
// plugins/center.rs / commands.rs 都没涨（commands.rs 实测 2060 ≤ 预算 2061，
// 反而降了 1 行）。门禁同时抓到 tray.rs 与 menu_bar.rs 有 14 行逐字重复
// （「退出」接线），已收进 resident.rs 的共用段，重复区间从 6 处降到 5 处。
// 36800 → 37150（2026-10-02）：磁盘占用报表（只读）。净增 350 行：
//   · src-tauri/src/diskusage.rs 196（新文件）：四个分类的逐目录 walk
//     （内核版本 / 实例数据 / 插件技能备份 / 壳日志）、字节降序、占比计算、
//     读不到的目录如实上报。**不提供任何删除入口**——本机实测最大的一块是
//     实例 DSH home 352M（用户会话与附件），壳无法替用户判断哪块该删。
//   · VersionsPanel.vue 约 +230：按需加载的槽位、字节格式化、一个只读卡片、
//     以及它的 scoped 样式。**样式放在组件里而不是 theme.css**：后者是反
//     棘轮大文件（预算只许下调），把 80 行组件私有样式塞进去会让它当场
//     超预算（实测 3151 → 3238，预算 3225）。拆到组件里两处都不欠账。
//
// 反棘轮大文件本轮**零增长**：theme.css 回到 3151 行未动，commands.rs /
// plugins/center.rs 未动。
// 37220 → 37340（2026-10-03）：占用报表改瓦片卡片布局 + 两段式加载与一天
// 新鲜度缓存。+120 行：Rust 缓存层约 46、UI 约 74。
// 37053 → 37069：同一天第四笔，版本号徽标改成分段式（用户给的参考图）。
// +16 全部在新组件 ui/src/shell/VersionBadge.vue（38 行，已登记进
// FILE_BUDGETS），删掉的是 SideBar 的 .brand-version（9 行）与 OverviewPanel
// 的 .kernel-version（17 行）两处各自的局部实现。**反棘轮文件本轮零增长**：
// theme.css 一个字没动，样式全在新组件的 scoped 块里。
// 37074 → 37087：npm 发布列表加边框 + 底色加深，与上方「已安装」区做出 UI
// 区分，并把上下边缘的半透明过渡区 8 → 12px（都是用户真机截图上明确要的）。
// +13 行全在 VersionsPanel.vue：模板里给滚动区包一层 `.release-list-box`
// （+2）、该层的 border / border-radius / background（+4）、其余是模板块
// 整体重排的行数变化。
// **为什么要多包一层**：边框若与滚动容器是同一个元素，它会被 mask 一起
// 淡掉——那恰好推翻「把滚动区域的边框显示出来」这条要求。边框画在外层、
// mask 只作用在内层的内容上，两者才互不干扰（已在夹具里对着验证）。
// 渐变区只能加到 12px：行内文字上下各留 ≈ 13px 空白，再大就开始啃字，
// 要更大的过渡区得先引入 JS 测量是否溢出。
// 37087 → 37088：修 Windows 构建失败——shell/autostart.rs 的 RegCreateKeyExW
// 漏传第 9 个参数 lpdwdisposition，windows-sys 0.61 起签名是完整 9 参数版。
// 纯 bug 修复，+1 行就是那个参数。
// 37063 → 37053：同一天第三笔，清官方对话按钮的遗留死规则与两个死 token，
// 净 -10。
// 37380 → 37063（2026-10-03 稍）：移除版本行前面的插件快照 tooltip。净 -317，
// 是**删除还回来的**、没有为它上调任何数字。整条链一次清干净：UI 组件
// `VersionPluginsTip.vue`、VersionsPanel 里的槽位与懒加载、theme.css 的
// `.installed-tip*` 整族 + `.kernel-plugin-tooltip` 覆盖（151 行）、IPC
// 命令 `kernel_plugin_list`、Rust 的 `KernelPluginRow` / `kernel_plugin_list`
// 及其单测、以及 `permissions/app-commands.json` 里那条死授权。
// 该 ACL 死授权是被 check:invariants 第 5 项当场抓出来的——手删命令时
// 漏了授权清单，它按「授权了但没注册」报错。四个预基线（theme.css /
// center.rs / commands.rs / TOTAL）全部下调锁死。
// 37340 → 37380（2026-10-03 早）：启动时默认做一次内核更新检查。+40 行：
//   · src-tauri/src/pkg/releases.rs 约 +18：`ReleaseOverview` 结构（发布列表 +
//     「可升级到哪一版」）与纯函数 `newest_upgrade`。版本比较**复用既有的**
//     `shell::version::cmp_versions`（该模块本来就 import 着它给三个 fetch_*
//     排序用），前端因此不需要第二份 semver 实现——`0.2.0` 与 `0.2.0-rc.1`
//     的先后判错会让每次启动都误报「有新版本」。6 条单测覆盖预发布段、
//     「基线取已装最新而非活动版本」、空安装集与空列表。
//     **刻意不把 upgrade 并进 `ReleaseList`**：那个结构的结果会进 60 秒进程
//     缓存，而「可升级到哪一版」取决于本机装了些什么，缓存会把它端给下一个
//     调用者。拆成两个类型，缓存里就只剩与本机无关的纯网络结果。
//   · src-tauri/src/kernel/lifecycle.rs 约 +11：`release_overview` 组装
//     两者。放这层是因为已装版本本就归它管（`kernel → pkg` 已有先例：
//     `kernel_deps.rs` 就这么调 `pkg::registry` / `pkg::releases`）。
//   · ui/src/store.js 约 +6：`checkUpdates(manual)` 补静默路径——不弹提示，
//     失败**不清空**已有列表（网络抖一下就把好数据抹掉，页面会从「有列表」
//     跳回「点击获取」，比没检查更像故障）。
//   · ui/src/App.vue 约 +5：启动钩子里发一次 `checkUpdates(false)`。
//   · ui/test/updateChecks.test.js 约 +0（净减）：新增 2 条静默路径测试，
//     抽出重复的 mock 分支抵掉。
// **反棘轮文件本轮零增长**：theme.css 未动；commands.rs 从 2064 拆回 **2061
// 正好等于预算**——组装逻辑一开始就堆错了地方，搬去 lifecycle.rs 之后命令
// 层退回纯转发（`blocking(|| lifecycle::release_overview(&data_dir)).await`），
// 连签名都不用为它拆行。未上调任何 FILE_BUDGETS。
// 37088 → 37221（2026-10-05）：插件目录检索搬进后端，面板加搜索框与
// 「插件仓库」分组。+133 行，逐处交代：
//   · **新文件** src-tauri/src/plugins/catalog.rs +146：关键词 / 分类 / 排序
//     三个参数的检索层（相关度三档、updated 何时接管、limit 硬顶 200、
//     分类计数随关键词走）+ 一条 10 行的命令转发。
//   · src-tauri/src/plugins/mod.rs +1：多一个 `pub(crate) mod`（见上面那条）。
//   · src-tauri/src/commands.rs **-8**：删掉 `plugin_catalog`——「把 1.7 万条
//     整份搬进 webview 再在浏览器里筛」这条路径整体作废，连同它的 ACL 授权。
//   · ui/src/plugins/plugins.js 约 -20：客户端那套 `catalogMeta` /
//     `matchScore` / `filteredCatalog` / `hasActiveFilter` /
//     `resetCatalogFilters` 全部删掉，**筛选规则从此只有 Rust 那一份**。
//   · ui/src/plugins/PluginsPanel.vue 约 +16：搜索框、分组标题、三个控件
//     各自的提交动作，减去旧的三处 computed 与防抖 watcher。
//   · theme.css **零增长**：搜索框复用 `.install-row`（它本来就是「一行一个
//     输入框」的通用形态），分组标题复用 `.section-divider`，计数复用
//     `.muted`——一条新样式都不加，3001 的反棘轮才守得住。
// 37221 → 37243（2026-10-05）：登录自启 / Dock 恢复修复，净 +22，逐处交代：
//   · src-tauri/src/lib.rs +12：自启收起改走 `resident::hide_to_shell`（裸
//     `hide()` 会在 macOS 留下点不动的 Dock 图标）与 `RunEvent::Reopen` 兜底
//     `show_main_shell` 的接线与注释（check:invariants 第 17 项的由来）。
//   · src-tauri/src/notify/activate.rs +10：`raise_workbench_if_open` 改返回
//     bool——「没抬到工作台」必须让调用方知道，否则兜底接不上。
// 37243 → 37276（2026-10-05）：内核版本页「npm 发布」吃掉页面剩余高度——
// VersionsPanel.vue +33 scoped 布局：面板钉满 main 可视高，纵向滚动收进
// 发布列表内部，日常状态不再出外层滚动条（用户要求；矮窗兜底仍归 main）。
// 37276 → 37312（同日）：发布列表边缘淡出加大范围与力度（12 → 22px、边缘
// 6px 全透明），并按旧注释的预言补上 JS 测溢出——mask 只在真正可滚时生效，
// 短列表的首尾行不再被无谓削掉。VersionsPanel.vue +36。
// 37312 → 37317（同日第三轮收敛）：淡出改为静止覆盖带——滚动区域内完全
// 正常显示，半透明只发生在上下两条 absolute 渐变带上（mask 版会把淡出吃
// 进可视区，用户指出不对）。VersionsPanel.vue +5。
// 37317 → 37334（同日第四轮）：带子挪到盒子**外侧**并弧形外扩（用户手绘
// 示意）——外溢视口（等量负 margin + padding 把滚动窗口越出边框 20px）让
// 行滚出边框仍可见，弧形带在盒子外接手遮盖；标题行与磁盘占用抬 z:2 保
// 可点可见。VersionsPanel.vue +17。
// 37334 → 37335（同日）：外溢深度 20 → 15px（20 压到上下文本），五处引用
// 收敛为 --release-bleed 一个变量（带回退值，css-var 门禁要求）。
// 37335 → 37344（同日）：深度 15 → 12px；带子改半透明（峰值 0.9、外缘回落
// 0.15）——背景是网格纹理 + 玻璃卡片，不透明实色块会显出一块异质矩形
// （用户指出「和原本的 UI 不匹配」）。VersionsPanel.vue +9。
// 37344 → 37345（同日）：「收进后台」提示补发判据收紧（登录自启后的第一次
// 唤回静默，resident.rs +3 / commands.rs +3 - 抵扣）。
// 37345 → 37375（同日）：toast 增加可勾选的「不再提示」（toastWithCheckbox
// 通用入口 + App.vue 监听器改造，localStorage 跨启动保留、分壳生效）。
// 39690 → 39950（同日第二轮）：补 Phase C 的审计缺口——候选来源与完整性
// 字段（后端 PrecheckReport + 前端候选信息卡）、五段实验流程时间线、折叠
// 证据与风险、实例运行守卫、commit 前的 pre-change 快照。
// 39690 → 39690（2026-10-06）：运行诊断（docs/features/diagnostics/runtime-diagnostics-design.md）
// 四个阶段。**这是一次新能力，不是膨胀**：净增 2315 行里，后端 run.rs +
// startup_run.rs + run_cmd.rs 是新模型（一次操作 = 一条记录），前端
// diagnostics/ 是三个新页面加控制塔。既有文件净增接近零——commands.rs
// 与 guard.rs 恰恰因为反棘轮被反向拆出了 startup_run.rs（两者都没有上调
// 预算，而是各自回到基线以下）。总量是软上限，按设计文档要求的范围就该有
// 这个量级。
// **软上限**：刻意不做「只许下调」——试过，实践中只会逼人绕过门禁而不是真写出
// 更少的代码。真正的收紧手段是上面那条「大文件只许下调」的规则，总量这条只在
// 有人加了一整片新能力时要求他把账写清楚。
//
// 40200 → 40750：诊断功能补齐设计 §4.1 要求的**四种 kind**（此前只有 startup 与
// plugin-precheck 两条接了运行记录，restore 与 bisect 被我单方面降级成了「未
// 做」——那是对文档的错误解读，不是取舍）。净增 447 行分布在 7 个文件里：其中
// 249 行是 operation_run.rs（含 110 行单测），86 行是从 commands.rs 搬过来的
// snapshot_cmd.rs（**净减** 78 行，那份反棘轮文件因此从 2047 降到 1969，预算
// 同步下调）。也就是说真正的新逻辑不到 200 行，其余是接线与测试。
// 41130 → 41135：概览控制塔在 480px 窄窗下省下 223px 纵向空间
// （ControlTower 三块卡合计 525 → 302px，真实编译 CSS 实测）。做法是删掉三行
// 与下方「当前内核」卡逐字重复的读数 + 四项读数改两列 + 详情一律单行省略：
// 内容减了，样式加了两列网格与省略号三件套，净 +5。ControlTower.vue 自身
// 因为删行而变小。总量是软上限，这次是新布局本身的代价，不是新能力。
// 41135 → 41138：三个诊断视图补上「这条记录是什么时候的」。用户拿着一张
// 18:09 的失败截图来报「还是过不了检测」，而那一刻修复已落地一小时——页面
// 上没有任何东西能让他看出那份结论说的是哪一次。这条不是新功能，是把已有的
// `startedAtMs` 说出来。
// 41138 → 41196：顶部版本带铺满整窗 + 「磁盘用量」四类占用按 id 配色并各带一句
// 说明（用户 2026-10-06）。净增 58 行，全部落在 VersionsPanel.vue（592 → 650，
// 仍是基线以下 600 的反棘轮阈值，theme.css 那条**零增长**）：
//   · USAGE_TYPES 四类各一组 {color, tip} + usageColor/groupTip 两个取值函数：
//     58 行里的大头。**按 group.id 而不是 label 取表**，label 是文案，改一次
//     「实例数据」不该让配色与说明一起失配；未知 id 回落 --accent，新加一类忘了
//     配色时仍然看得见，而不是渲染成空白。
//   · 瓦片标题从原生 `title` 换成 el-tooltip：原生 title 要悬停约 1s 才出，样式
//     也与界面不一致，而这句话（哪块能删、哪块是你的会话）是用户点进这块区域的
//     真正答案。el-tooltip 默认插槽必须有真实元素，否则取不到触发器且**不报错**
//     ——这条由 ui/test/diskUsageAndChrome.test.js 钉住。
//   · 顶部渐变 `0.25 → 0` 的两处终点从 50%/75% 改 100%：各改一个数字，theme.css
//     连注释重排都是净减 2 行。
// 总量是软上限（见上方「软上限」段），这次是四张瓦片真的能被一眼区分开的代价。
// 41196 → 41199：概览页控制塔三张卡压紧一轮（用户 2026-10-06）。净增 3 行，
// 全在 diagnostics.css 的三条 `--tower` 覆盖（见 FILE_BUDGETS 里那处的说明）；
// ControlTower.vue 只给三张卡各加一个类名，行数一字未变。
// ⚠️ 这 3 行**不含**同时段另一会话在 commands.rs / precheck.rs /
// launch_url.rs 上的在途改动——那部分等它自己提交时再算，不预先写进这里的数字。
// 41199 → 41216：预检的「内核起来了吗」判据。带 launch token 的内核对裸
// `GET /` 回 401，而判据此前只认 2xx/3xx，于是每一次预检都判「内核返回
// HTTP 401」——内核明明已经起来了，预检连装插件那一步都没走到
// （run-20261006-210045-0f19）。这一段是判据本身的修正 + 为什么不去取令牌
// 的说明，不是新能力。
// ⚠️ 这几行**不含**并行会话在同一工作树上的在途改动——那部分等它自己提交时
// 再算，不预先写进这里的数字。
// 41216 → 41237：「更多」弹层补上 EP 缺失的样式 + 菜单项图标（用户 2026-10-06
// 「菜单太简陋、没有 hover」）。净增 21 行：diagnostics.css +14、DiagnosisShell
// +6、main.js +1（补一条 `dropdown/style/css.mjs` 导入）、菜单映射表 ±0
// （icon 字段挂在既有行上）。CSS 产物从 218.6KB 涨到 229.3KB（预算 230KB），
// 涨幅几乎全是那份漏掉的 `el-dropdown.css`——本来就该在产物里的东西。
// 41237 → 41365：git 子进程走系统代理（shell/env.rs 新增 ~90 行 + 三个调用点）。
// git 只认 gitconfig 与 `http_proxy`，**完全看不见操作系统层面配的代理**，
// 而 macOS 的 GUI 应用还不继承 shell rc 里的 `export http_proxy`（那是 launchd
// 的事）。用户在系统里明明配了代理，clone 却绕开它直连——2026-10-06 实测 12 KB/s
// 爬 GitHub，10 分钟拉不完一个插件仓库。优先用已有环境变量，其次系统设置；一个
// 都没探测到就不注入，照直连。
// 41365 → 41568（2026-10-06 晚，用户「git 拉取卡住」实测反馈）。净增 203 行：
//   · src-tauri/src/shell/stream.rs 新增 111：带超时的流式子进程捕获。git clone
//     换上 `--progress` + 逐行回传，进度面板实时显示 `Receiving objects: N%`。
//     此前用 run_command_capture_with_timeout 一次性捕获，12KB/s 的直连下是
//     十分钟不动的空白——用户无法区分「在慢慢拉」和「已经卡死」。
//   · diagnostics/run.rs +37：sweep_orphans + clear。孤儿详情是真缺口（§17.7）：
//     详情先落地、索引后写入，壳在两者之间被强杀就留下列表读不到的文件。
//   · 其余 +55 分布在 run_cmd / diagnostics.js / ControlTower.vue / mod.rs /
//     manage.rs，是上面两条的命令层、动作层与入口。
// 同轮把 GIT_CLONE_TIMEOUT 从 600s 压到 300s：等满 10 分钟等到的仍是一次必然
// 失败的超时，早点失败早点能换来源重试。
// 反棘轮一个数字没动：process.rs 仍 1157/1157（改动只有 `pub(crate)` 可见性与
// 常量值，都在 codeLineCount 之外），center.rs 2825 → 2817。
// 41574 → 41578：`sweep_orphans` 的索引损坏保护。`load_index` 是容错读，损坏时
// 同样返回空索引——拿它去比对孤儿，磁盘上每一条详情文件都会被当成孤儿删掉，
// 一次启动抹掉用户全部诊断历史。改走 `load_checked`（+4 行，钉在 run.rs 那条
// 注释里并配了一条反向验的单测）。这条比它多出来的 4 行便宜得多。
// 41591 → 41620（2026-10-07）：「刷新」按钮强制重扫（用户报「刷新按钮，没有正确
// 扫描磁盘占用」）。全部 33 行都落在 diskusage.rs（282 → 316，逐条理由见
// FILE_BUDGETS 里那个条目的注释），前端 VersionsPanel.vue 只 +7 行（拆开返回的
// 两段 + 按钮传 force）。反棘轮大文件一个数字没动：theme.css 仍 3001/3001、
// process.rs 仍 1157/1157、center.rs 2816/2871。
// 41624 → 41670：按 runtime-diagnostics-review-2026-10-06.md 第二轮复审修 R2-P1-01
// 与 R2-P1-02（两阶段预检的应用契约）。净增 46 行：
//   · plugins/precheck.rs +4：报告多带一个 `apply_spec`，以及摘 URL 凭据的 14 行。
//     UI 过去拿 `pluginId`（`@scope__pkg` 那种中央库 id）回传给
//     `plugin_precheck_apply` 重新解析，会解析成不存在的包 / 丢掉 Git 来源 / 丢掉
//     tag——用户看到「预检通过」，点应用却装上别的东西。
//   · plugins/plugins.js +45：applyPluginChange 改收整份报告、走 `applySpec`、用
//     `onResult` 取回传的完整报告（过去把 `withProgress` 的 `true` 当报告存进
//     store，诊断页读 verdict / installed / preChangeSnapshotId 全是 undefined），
//     并新增 `sourceDrift` 核对「装的」与「验的」是不是同一个对象。顺带删掉一次
//     重复的 refreshAll（`withProgress` 内部已经 await 过）与一次重复的成功 toast。
//   · 两处调用点各净 0 行（`report.value.pluginId || '', report.value.verifiedAtMs`
//     → `report.value`）。
// 新增 2 个测试文件（ui/test/applyPluginChange.test.js 5 条、precheck.rs 内 2 条）：
// R2-P1-01 的四种来源形状 + 凭据剥离要逐字往返，其中 npm 作用域包的 `@` 不是
// userinfo——写错一次就把一次正确的安装变成装上另一个包。
// 反棘轮大文件一个数字没动。
// 41831 → 41860：+29 行 = 上面的 `settle_dangling_stage`(5) +
// `collapseSupersededRunning`(9) + `SANDBOX_CREATE` 成功终态(7) + 测试与
// 不变量判据。这 29 行买的不是功能，是「同一个阶段永远不会同时显示进行中
// 与已完成」——2026-10-07 用户截图里那四个转圈的圈。反棘轮大文件一个数字
// 没动（theme.css / process.rs / commands.rs）。
const TOTAL_BUDGET = 41860;
// 35230 → 35250（2026-09-30 晚）：DeepSeek 余额按三个字段分别展示（用户实测
// 「只看到 ¥16.64，看不出是赠金还是充值」）。净增 17 行，落在三个已有文件里：
//   · ui/src/subscription.js +8：`balanceText` 换成 `balanceRow`，产出主行
//     （总额）/ 明细行（赠金、充值分别列）/ hover title（等式口径）三个字段。
//   · ui/src/SubscriptionWindow.vue +3、ui/src/components/OverviewPanel.vue +6：
//     余额块多一行明细 + 样式，模板结构其余部分不动。
// 设计稿 docs/features/subscription/subscription-usage-design.md 的解析规则本来就写着「逐条展示：
// 币种、总额、赠送（未过期）、充值」，这次是把前端欠掉的展示补齐，不是新增
// 能力，也不是复制粘贴（两处模板共用同一个 balanceRow）。Rust 侧零改动：
// 三个字段早就在 CacheBalance / BalanceView 里透传。反棘轮（plugins.rs /
// theme.css / commands.rs）一个数字没动。
// 35250 → 35440（2026-09-30 晚，草稿加图片）：用户要求草稿连「还没发出去的图」
// 一起保管。净增 ~190 行，落在四个文件里：
//   · harness_media.rs 143 行（新文件，登记见上）：图片的 base64 收发、三个上限、
//     媒体目录的「整目录重建」自愈。编解码是自己写的（不引依赖），占 60 行。
//   · harness_draft.rs 70 → 130：留下「什么时候有、什么时候作废」，图片的字节
//     搬走。净增的是「只发图不发字也算有东西」「图没了也要作废」两条判据。
//   · harness_cmd.rs +8：`stash_harness_draft` 多一个 `images` 形参。
//   · 其余是文档与测试，不计入预算。
// 注入脚本 `harness-draft.js` **不计入**（预算只数 .rs 与 ui/src），那边的增长由
// `ui/test/harnessDraft.test.js` 的 22 个测试兜着。**没有**为省预算去动反棘轮文件
// （plugins.rs / theme.css / commands.rs 一个数字没动），也没有复制粘贴第二份
// 编解码——两边共用 harness_media 里的同一份。
// 35120 → 35230（2026-09-30 傍晚）：横幅按实际残余风险门控——新模块
// install_isolation.rs 66 行（登记见上）、kernel.rs +~40（InstalledVersion 的
// shared_storage、other_shell_tree_shared、门控与装 / 删提示改为按采样条件触发）、
// instance.rs 文案重写净持平。反棘轮（plugins.rs / theme.css / commands.rs）
// 一个数字没动。
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
    if (baseline.names.has(path.split('/').pop())) continue; // 搬移
    if (isKnownBlob(baseline, path)) continue; // 改名：内容没变
    if (baseline.renames.has(path)) continue; // git 记录的重命名
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
    // 基线按**路径**查不到时退回按**文件名**查：文件被搬进子目录后路径变了，
    // 直接 `continue` 等于让这条规则对搬过的文件失效——而 plugins.rs(2968)、
    // commands.rs(2110)、skills.rs(1490) 正是最需要它的那批。「新文件」那条
    // 判据已经按文件名认过搬移，这里不认就是同一个洞的两半。
    const oldPath = baseline.renames.get(path);
    const before = baseline.files[path]
      ?? baseline.fileBudgets.get(moduleId(path))
      ?? (oldPath ? baseline.fileBudgets.get(moduleId(oldPath)) : undefined);
    if (before === undefined) continue; // 真·新文件，见下面的硬顶
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
  if (baseline && baseline.names.has(path.split('/').pop())) continue; // 搬移
  if (baseline && isKnownBlob(baseline, path)) continue; // 改名
  if (baseline && baseline.renames.has(path)) continue; // git 记录的重命名
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
