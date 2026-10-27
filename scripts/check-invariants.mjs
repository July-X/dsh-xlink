#!/usr/bin/env node
/**
 * 跨文件不变量校验。
 *
 * 这里检查的约束都分散在多个文件里：单看任何一处都看不出问题，但任何一处
 * 漏改都会造成用户可见的故障。这个脚本的存在理由是真实事故——
 * `install_node` 注册进 `tauri::generate_handler!` 却漏了 ACL 白名单，于是
 * 「帮我安装 Node.js」在 rc.11~rc.18 共 8 个发布版本里 100% 失败，而代码、
 * 编译和测试全部是绿的。
 *
 * 用法：
 *   node scripts/check-invariants.mjs        # 校验，失败以非零退出码结束
 *
 * 检查项：
 *   1. `generate_handler!` 注册的命令集合 == `allow-local-commands` 白名单集合；
 *   2. 每个 capability 引用的自定义权限标识都真实存在；
 *   3. UI 与注入脚本调用的每个命令都已注册、且已授权给对应窗口；
 *   4. UI 模板里的绑定都能解析（委托 `scripts/check-ui-bindings.mjs`）；
 *   5. 管理窗口的无边框来自 `tauri.conf.json`（不是运行时 `set_decorations`），
 *      且 macOS 标题栏最小化走系统原生最小化——写反了黄灯会静默失效。
 *   7. workflow 的 `uses:` 全部固定到 commit SHA，且 `.github/dependabot.yml` 存在并
 *      覆盖 github-actions——钉住的 SHA 只能靠 Dependabot 推进，缺了它策略就是空话。
 *   8. UI 模板里用到的 `el-*` 组件都已在 main.js 注册——本项目不引入 unplugin
 *      自动导入器，漏注册的组件在构建与测试全绿的情况下被当未知自定义元素
 *      原样渲染（P8 双 tab 因此整页平铺、迁移向导的勾选/单选全部失效）。反向
 *      也查：注册了却没人用的组件会把 `theme-chalk` 的样式白打进产物，撞穿
 *      CI 的 CSS 预算（`el-table` 赖在 main.js 里，直到 UI CSS 只剩 838 字节
 *      余量才被发现）。本仓库没有动态 `<component :is>` 引用 EP 组件的写法，
 *      所以 `registered ⊆ used` 不会误报。
 *   9. 前端读的 IPC 字段名与 Rust 侧 `rename_all = "camelCase"` 发出的名字对得上
 *      ——整套快照 UI 曾一直在读 snake_case，「回到良好状态」点了没反应、二分
 *      按钮恒置灰，而单测全绿（夹具是手写的 snake_case，没有真实响应穿过）。
 *  10. 生产代码里 `InstanceRecord::new` 的 id 实参不得是常量。
 *  11. 注册表那个共享的 `default_instance_id` 只准 `instance.rs` 指向具体实例。
 *  12. 内核树的三条变更动作只有**本壳**守卫能阻断，跨壳判据只提示不拦。
 *      （第 10、11 条的由来见下方注释，第 12 条见文件末尾的 ④；第 13、14 条
 *      只在脚本里带注释。）
 *  15. 面板里的资源（`<img src>` / CSS `url()`）不许指向远端，且以 `/` 开头的
 *      路径必须真的存在于 `ui/public`——WebView 出不出网不由我们决定。
 *  16. 跨模块 `use` 不得无条件引用只在某个 `target_os` 下定义的条目。Windows
 *      target 只在发布流水线里被编译一次（`check:rust` 只编宿主 target，Quality
 *      gates 跑在 ubuntu 上也不是 Windows），所以这类错误一次就是一次完整发布
 *      ——rc.2 在 2026-10-01 为此连炸两次。
 *  17. 常驻的两条接线（2026-10-05「重启后 Dock 有启动状态、看不到主界面」的
 *      两半成因）：① lib.rs 里不得裸调 `window.hide()`，自启收起必须走
 *      `resident::hide_to_shell`——裸 hide 不降 macOS 激活等级，进程带着
 *      Regular 等级、零可见窗口地挂在 Dock 上；② `RunEvent::Reopen` 抬不到
 *      工作台时必须兜底 `show_main_shell`——macOS 的激活不会替我们显示
 *      `hide()` 掉的窗口，缺了它那枚 Dock 图标点了没反应。
 *  21. 壳派生的每个子进程都不得继承一个「指向已删除文件」的 Node 预加载：
 *      用户级 `NODE_OPTIONS=--require=<已删除的文件>` 会让内核 / pnpm / npm
 *      在 Node 的预加载阶段就退出，内核代码一行都没加载——工作台起不来而事故
 *      面板归不出任何插件或内核组件（2026-10-20 实测）。
 *  36. 启动时后台预启动的接线：`shell/prestart.rs` 的预热与判据都是可单测的
 *      纯函数，生产上漏接一行就全绿而无效——setup 必须真派线程、线程体必须
 *      走判据 + 预热 + `start_kernel_blocking`，退出时必须置位退出标记。
 */

import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs';
import { dirname, isAbsolute, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

import { shellSourcePath, shellSourceLabel } from './lib/shell-source.mjs';
import { stripComments } from './check-official-chat-p4-codes.mjs';
import {
  findDataDirectoryViolationsInFiles,
  findProfileViolationsInFiles,
} from './lib/probe-profile-isolation.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const failures = [];
const notes = [];

const fail = (section, message) => failures.push(`[${section}] ${message}`);

const note = (message) => notes.push(message);
const read = (rel) => readFileSync(join(root, rel), 'utf8');
const readJson = (rel) => JSON.parse(read(rel));

// 单文件判据一律按 basename 定位，不写死 `src-tauri/src/...` 完整路径。理由、
// 踩过的坑与「为什么重名要抛错而不是猜」写在 lib/shell-source.mjs 的文件头；
// 门禁脚本与 UI 测试共用这一份实现——两套解析器迟早只修得上一套。
const shellSource = (fileName) => readFileSync(shellSourcePath(fileName), 'utf8');

/** 相对路径的 basename：`kernel/lifecycle.rs` → `lifecycle.rs`。
 *
 * **两种分隔符都要认。** `RUST_SOURCES` 这类列表在构造时被归一化成**正斜杠**
 * （见该常量的注释），而 `walk()` 产出的路径在 Windows 上是反斜杠；只按
 * `node:path` 的 `sep` 切分时，Windows 上正斜杠路径切不开、basename 变成整条
 * 路径——`window-chrome` 的「`Theme::Dark` 只许出现在 `appearance.rs`」白名单
 * 因此把策略出口自己误报成违规（2026-10-18 实机；CI 的 Linux 检出不复现）。
 */
const baseName = (relPath) => relPath.split(/[\\/]/).pop();

/** `where` 形如 `kernel/lifecycle.rs:549`——问「是不是落在 baseName 这个模块里」。
 *
 *  判据里的「这个文件」必须按 basename 认，不能按相对路径认。2026-10-01
 *  `src-tauri/src` 分目录后，一批「唯一合法的写者 / 落点」白名单全部失配：
 *  `kernel.rs` 变成了 `kernel/lifecycle.rs`、`instance.rs` 变成
 * `shell/instance.rs`，于是本该只报一次的**真违规**被报成「出现在
 * kernel/lifecycle.rs:549、603」——红的原因与要检查的东西无关。与
 * `findShellSource` 同源：路径是摆放，basename 才是模块的身份。
 */
const inFile = (where, name) => baseName(String(where).split(':')[0]) === name;

/** 递归收集指定后缀的文件（跳过 node_modules 与构建产物）。 */
function walk(dir, suffixes) {
  const out = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (entry.name === 'node_modules' || entry.name === 'dist' || entry.name === 'target') {
      continue;
    }
    const full = join(dir, entry.name);
    if (entry.isDirectory()) {
      out.push(...walk(full, suffixes));
    } else if (suffixes.some((suffix) => entry.name.endsWith(suffix))) {
      out.push(full);
    }
  }
  return out;
}

const show = (path) => relative(root, path).split('\\').join('/');

// --- 1. 命令注册 ↔ ACL 白名单 ------------------------------------------------

const libSource = read('src-tauri/src/lib.rs');
const handlerMatch = libSource.match(/generate_handler!\[([\s\S]*?)\]/);
if (!handlerMatch) {
  fail('commands', '在 src-tauri/src/lib.rs 中找不到 generate_handler![...]，脚本需要更新');
}
const registered = (handlerMatch?.[1] ?? '')
  .split(',')
  .map((entry) => entry.trim())
  .filter(Boolean)
  .map((entry) => entry.split('::').pop());

const permissions = readJson('src-tauri/permissions/app-commands.json').permission;
const findPermission = (identifier) => permissions.find((p) => p.identifier === identifier);
const localPermission = findPermission('allow-local-commands');
if (!localPermission) {
  fail('commands', 'app-commands.json 缺少 allow-local-commands 权限定义');
}
const allowedLocally = localPermission?.commands?.allow ?? [];

const countBy = (values) =>
  values.reduce((acc, value) => acc.set(value, (acc.get(value) ?? 0) + 1), new Map());
for (const [name, count] of countBy(registered)) {
  if (count > 1) fail('commands', `命令在 generate_handler! 中重复注册 ${count} 次：${name}`);
}
for (const [name, count] of countBy(allowedLocally)) {
  if (count > 1) fail('commands', `命令在 allow-local-commands 中重复列出 ${count} 次：${name}`);
}

const registeredSet = new Set(registered);
const allowedSet = new Set(allowedLocally);
for (const name of registeredSet) {
  if (!allowedSet.has(name)) {
    fail(
      'commands',
      `命令已注册但未列入 allow-local-commands：${name} —— 面板调用它会被 ACL 拒绝` +
        `（release 构建下报 "Command ${name} not allowed by ACL"）`,
    );
  }
}
for (const name of allowedSet) {
  if (!registeredSet.has(name)) {
    fail('commands', `allow-local-commands 里的命令并未注册：${name}（死授权）`);
  }
}
if (failures.length === 0) {
  note(`命令注册与白名单一致：${registeredSet.size} 个命令`);
}

// --- 2. capability → 权限标识 ------------------------------------------------

const customPermissions = new Set(permissions.map((p) => p.identifier));
const capabilitiesDir = join(root, 'src-tauri/capabilities');
for (const file of walk(capabilitiesDir, ['.json'])) {
  const capability = JSON.parse(readFileSync(file, 'utf8'));
  for (const permission of capability.permissions ?? []) {
    // 含冒号的是 Tauri 自带或插件权限（core:*、opener:* 等），由 tauri-build
    // 在编译期校验；这里只保证自定义权限的引用有效。
    if (permission.includes(':')) continue;
    if (!customPermissions.has(permission)) {
      fail('capabilities', `${show(file)} 引用了未定义的权限标识：${permission}`);
    }
  }
}
note(`capability 权限引用有效：${walk(capabilitiesDir, ['.json']).length} 个文件`);

// --- 3. UI / 注入脚本调用的命令都已授权 ---------------------------------------

// 远端或局部窗口通过 capability 直接授予的命令（不经 allow-local-commands）。
const grantedOutsideLocal = new Set();
for (const permission of permissions) {
  if (permission.identifier === 'allow-local-commands') continue;
  for (const command of permission.commands?.allow ?? []) grantedOutsideLocal.add(command);
}

const callers = [
  ...walk(join(root, 'ui/src'), ['.js', '.vue']),
  // 注入到远程页面的初始化脚本同样会调用外壳命令。
  ...walk(join(root, 'src-tauri/src'), ['.js']),
];
const invoked = new Map(); // 命令 → 首个调用点
for (const file of callers) {
  const text = readFileSync(file, 'utf8');
  const patterns = [/invoke\(\s*['"]([a-z_]+)['"]/g, /cmd:\s*['"]([a-z_]+)['"]/g];
  for (const pattern of patterns) {
    for (const match of text.matchAll(pattern)) {
      if (!invoked.has(match[1])) invoked.set(match[1], show(file));
    }
  }
}
for (const [command, where] of invoked) {
  if (!registeredSet.has(command)) {
    fail('frontend', `${where} 调用了未注册的命令：${command}`);
    continue;
  }
  if (!allowedSet.has(command) && !grantedOutsideLocal.has(command)) {
    fail('frontend', `${where} 调用了未授权的命令：${command}`);
  }
}
note(`前端与注入脚本调用 ${invoked.size} 个命令，全部已注册且已授权`);

// --- 版本一致性 ---------------------------------------------------------------
//
// 三个版本字段必须一致：`package.json` 与 `tauri.conf.json` 决定发布产物与
// updater 的版本，`src-tauri/Cargo.toml` 是 `env!("CARGO_PKG_VERSION")` 的来源
// —— 它被 releases.rs 当作对外 User-Agent（`dsh-xlink/<版本>`）。历史上
// Cargo.toml 长期停在 0.1.0，外壳因此对 npm registry / GitHub 自称另一个版本
// （P2-31）。preflight 也会拦，但那个门只在发布时才会跑到。

{
  const packageVersion = readJson('package.json').version;
  const tauriVersion = readJson('src-tauri/tauri.conf.json').version;
  const cargoMatch = read('src-tauri/Cargo.toml').match(/^version = "([^"]+)"/m);
  const cargoVersion = cargoMatch?.[1];
  if (!cargoVersion) {
    fail('versions', '在 src-tauri/Cargo.toml 中找不到 version = "..."');
  } else if (!(packageVersion === tauriVersion && packageVersion === cargoVersion)) {
    fail(
      'versions',
      `版本不一致：package.json=${packageVersion}, tauri.conf.json=${tauriVersion}, ` +
        `src-tauri/Cargo.toml=${cargoVersion}`,
    );
  } else {
    note(`三处版本一致：${packageVersion}`);
  }
}

// --- 协议版本声明一致 ---------------------------------------------------------
//
// 官方对话四份协议各自把「当前是第几版」写在**几个地方**：文首标题、文首状态行
// （P1／P2 走 frontmatter 的 `status`）、以及变更记录表。升版时漏改其中任何一处，
// 就出现「**正文已经是新版本、声明还停在旧版本**」—— 读者对不上这份稿子到底是哪一版。
//
// **本项目已发生四次同型**：① v0.8 文首标题漏改（正文 v0.8、标题还写 v0.7）
// ② v0.9 文首标题漏改 ③ v0.11 那一轮 P4 ＋ P3 v0.12 的状态行未跟上版本表
// ④ v0.13 那一轮状态行仍写 v0.11（表里已有 v0.12／v0.13）⇒ ⇒ 四次都是**人漏了**
// ⇒ ⇒ 这里把它变成**机器可拦**的：各处声明的版本号必须相等。
//
// 比对口径（三处各按各的形态取）：
//   · **文首标题** ＝ 标题行里的版本号（标题**不带**版本号时不参与比对；一行出现
//     多个版本号 ⇒ 形态不明、也不参与，避免误报）；
//   · **文首状态行** ＝ 状态行里**紧跟在「＝」「（」之前**的那个版本号（P3／P4 是
//     文首 `> **状态**：…`；P1／P2 是 frontmatter 的 `status:` 行 —— 注意它后面
//     跟着版本线枚举，枚举里的旧版本号**不是**当前版本，所以只认紧跟分隔符的那个）；
//   · **变更记录表** ＝ 表里**最高**的版本号（那一行才是最新的一版；没有这张表的
//     稿子不参与该项比对）。

/** 取状态行里「紧跟分隔符」的版本号（`v0.14` 那种）；取不到返回 null。 */
const declaredProtocolVersion = (text) => {
  const m = String(text ?? '').match(/\bv(\d+\.\d+)\s*(?:＝|=|\(|（)/);
  return m ? `v${m[1]}` : null;
};

/** 取标题行里的版本号；没有、或一行里有多个（形态不明）时返回 null。 */
const titleProtocolVersion = (line) => {
  const all = [...String(line ?? '').matchAll(/\bv(\d+\.\d+)\b/g)].map((m) => `v${m[1]}`);
  return all.length === 1 ? all[0] : null;
};

/** 取变更记录表里**最高**的版本号；表里没有版本行时返回 null。 */
const tableProtocolVersion = (lines) => {
  const all = lines
    .filter((l) => /^\|\s*v\d+\.\d+\s*\|/.test(l))
    .flatMap((l) => [...l.matchAll(/\bv(\d+\.\d+)\b/g)].map((m) => `v${m[1]}`));
  if (!all.length) return null;
  return all.reduce((a, b) => {
    const [am, an] = a.slice(1).split('.').map(Number);
    const [bm, bn] = b.slice(1).split('.').map(Number);
    return am > bm || (am === bm && an >= bn) ? a : b;
  });
};

{
  const PROTOCOL_DOCS = [
    ['P1', 'official-chat-p1-protocol.md'],
    ['P2', 'official-chat-p2-protocol.md'],
    ['P3', 'official-chat-p3-protocol.md'],
    ['P4', 'official-chat-p4-protocol.md'],
  ];
  for (const [label, file] of PROTOCOL_DOCS) {
    const rel = `docs/features/official-chat/${file}`;
    if (!existsSync(join(root, rel))) {
      fail('protocol-versions', `找不到协议文件：${rel}`);
      continue;
    }
    const lines = read(rel).split(/\r?\n/);
    // ⚠️ **标题不一定是第 0 行** —— P1／P2 的 frontmatter 在标题**前面** ⇒ ⇒ 按
    // 「第一行 `# ` 开头」找标题，别写死 `lines[0]`（否则 P1／P2 的标题永远取不到、
    // 那两份稿子的标题就永远不参与比对）。
    const titleLine = lines.find((l) => l.startsWith('# ')) ?? '';
    const statusLine =
      lines.find((l) => l.startsWith('> **状态**')) ??
      lines.find((l) => l.startsWith('status:')) ??
      '';
    const declared = [
      ['文首标题', titleProtocolVersion(titleLine)],
      ['文首状态行', declaredProtocolVersion(statusLine)],
      ['变更记录表', tableProtocolVersion(lines)],
    ].filter(([, v]) => v !== null);

    if (declared.length < 2) {
      note(`${label} 协议只找到 ${declared.length} 处版本声明 ⇒ 无从比对（跳过）`);
      continue;
    }
    const distinct = new Set(declared.map(([, v]) => v));
    if (distinct.size === 1) {
      note(
        `${label} 协议版本声明一致：${declared.map(([w]) => w).join(' ／ ')} ＝ ${[...distinct][0]}`,
      );
    } else {
      fail(
        'protocol-versions',
        `${label} 协议版本声明不一致：${declared
          .map(([w, v]) => `${w}=${v}`)
          .join('，')}（升版时这几处必须同时改 —— 本项目已发生过四次同型）`,
      );
    }
  }
}

// **提取口径的自证**（合成夹具）：只认「紧跟分隔符」的那个版本号 ⇒ 后面那串版本线
// 枚举里的旧版本号**不许**被当成当前版本（认错了等于没检查）。
{
  const cases = [
    {
      text: '> **状态**：**v0.14 ＝ PM 订正稿**（PM 2026-10-21 19:1x）—— v0.1 把已实测的事实写死成条款；v0.13 ＝ PM 订正笔',
      want: 'v0.14',
    },
    {
      text: '> **状态**：冻结稿 v0.12（PM 定；v0.1 → v0.2 = 第一批实现已落地；v0.11 → v0.12 = 码闭集 13 → 14 同步）',
      want: 'v0.12',
    },
    {
      text: 'status: "P2 协议冻结稿 v0.7（2026-10-21 17:5x，PM 定；v0.6 → v0.7 = 码闭集 13 → 14 同步）"',
      want: 'v0.7',
    },
  ];
  for (const { text, want } of cases) {
    const got = declaredProtocolVersion(text);
    if (got !== want) {
      fail('protocol-versions', `状态行版本提取口径失灵：期望 ${want}、实得 ${got}`);
    }
  }
  note(`协议版本声明提取口径自证：${cases.length} 个合成样本全部取对`);
}

// --- 5. UI 模板绑定 ----------------------------------------------------------
//
// 模板里引用了既不在 `<script setup>` 绑定、也没有全局注册的标识符时，生产构建
// 不报错，绑定被静默求值成 `undefined`（`:disabled` / `:loading` 变成永不生效），
// 而全部 Rust 与 UI 测试都是绿的。判据由 `scripts/check-ui-bindings.mjs` 提供。
{
  const { checkProject } = await import('./check-ui-bindings.mjs');
  const { checked, skipped, failures: uiFailures } = await checkProject(join(root, 'ui/src'));
  if (checked < 10) {
    fail('ui-bindings', `仅检查到 ${checked} 个 setup 组件，组件扫描可能已失效`);
  }
  if (uiFailures.length > 0) {
    for (const failure of uiFailures) {
      fail(
        'ui-bindings',
        `${relative(root, failure.file)} 的模板引用了未定义的标识符：${failure.names.join('、')}`,
      );
    }
  } else {
    note(`UI 模板绑定全部可解析：${checked} 个 <script setup> 组件（跳过 ${skipped} 个）`);
  }
}

// --- 6. 管理窗口的无边框与标题栏按钮 ------------------------------------------
//
// 管理面板在 macOS / Windows 上自绘标题栏（`WindowTitleBar.vue`），窗口必须
// 出生即无边框：一旦改成运行时 `set_decorations(false)`，tao 会重算 macOS 的
// `NSWindowStyleMask` 并抹掉 `Miniaturizable`，标题栏黄灯随即变成点了没反应的
// 死按钮（`miniaturize:` 静默失败，Tauri 的 `minimize()` 还返回 `Ok(())`）。
// 这类回归在 CI 与 macOS 上的编译都不会报错，只有在真机点一次黄灯才看得见，
// 所以这里把「可最小化」自检和声明式配置一起钉住。
{
  const mainWindow = readJson('src-tauri/tauri.conf.json').app?.windows?.find(
    (window) => window.label === 'main',
  );
  if (!mainWindow) {
    fail('titlebar', 'tauri.conf.json 里找不到 label 为 main 的窗口');
  } else if (mainWindow.decorations !== false) {
    fail(
      'titlebar',
      '`main` 窗口的 decorations 不是 false —— 自绘标题栏会叠在系统标题栏上；' +
        '而且不能在 setup 里事后改（见下面那条）',
    );
  }

  // 去掉注释与 cfg 门控后再匹配：注释里就写着这个 API 名字，直接匹配必然误报。
  const rustCode = libSource
    .replace(/\/\*[\s\S]*?\*\//g, ' ')
    .replace(/\/\/[^\n]*/g, ' ')
    .replace(/#\[cfg\([^\]]*\)\]/g, ' ');

  if (/set_decorations\s*\(/.test(rustCode)) {
    fail(
      'titlebar',
      'src-tauri/src/lib.rs 在运行时调用 set_decorations —— tao 会重算 macOS 样式位并抹掉 ' +
        'Miniaturizable，标题栏黄灯变死按钮；无边框请写在 tauri.conf.json 的 decorations: false',
    );
  }

  if (!/fn check_main_window_minimizable/.test(rustCode)) {
    fail('titlebar', 'src-tauri/src/lib.rs 缺少 check_main_window_minimizable 自检（黄灯回归哨兵被删了）');
  }

  const titlebar = read('ui/src/shell/WindowTitleBar.vue');
  const minimizeWindow = titlebar.match(/function minimizeWindow\s*\([^)]*\)\s*\{[\s\S]*?\n\}/);
  if (!minimizeWindow) {
    fail('titlebar', 'WindowTitleBar.vue 里找不到 minimizeWindow()（脚本需要更新）');
  } else if (!/callWindow\(\s*'minimize'/.test(minimizeWindow[0])) {
    fail(
      'titlebar',
      'WindowTitleBar.vue 的 minimizeWindow() 没有走 macOS 分支的 windowAction("minimize") —— ' +
        'macOS 必须用系统原生最小化，minimize_shell 只在 Windows 收进通知区域',
    );
  }
  note('管理窗口无边框来自声明式配置，标题栏最小化语义按平台分开');
}

// --- 7. workflow 供应链：SHA 固定 + Dependabot ---------------------------------
//
// AGENTS.md 与 docs/operations/release.md 都承诺「workflow 中所有 `uses:` 固定到 40 位 commit
// SHA，升级走 Dependabot」。这两件事必须同时成立：SHA 固定把 action 冻在一个已知提交
// 上，而没有 Dependabot 就没有任何东西会推进它——安全修复永远进不来，策略只剩文字。
// 这类缺口不会让任何测试变红（仓库曾经就是这样：三份文档都写着「由 Dependabot 升级」，
// 而 `.github/dependabot.yml` 并不存在），所以在这里显式钉住。
{
  const workflowDir = join(root, '.github/workflows');
  const workflows = existsSync(workflowDir)
    ? readdirSync(workflowDir).filter((name) => /\.ya?ml$/.test(name))
    : [];
  if (workflows.length === 0) fail('supply-chain', '找不到任何 workflow 文件');

  let usesCount = 0;
  for (const name of workflows) {
    const lines = read(join('.github/workflows', name)).split('\n');
    lines.forEach((line, index) => {
      const match = line.match(/^\s*(?:-\s*)?uses:\s*(\S+)/);
      if (!match) return;
      const target = match[1].replace(/^['"]|['"]$/g, '');
      if (target.startsWith('./')) return; // 仓库内 action 由本仓库代码决定，不需要 SHA
      usesCount += 1;
      if (!/@[0-9a-f]{40}$/.test(target)) {
        fail('supply-chain', `${name}:${index + 1} 的 uses: ${target} 没有固定到 40 位 commit SHA`);
      }
    });
  }
  note(`workflow 的 ${usesCount} 处 uses: 全部固定到 commit SHA`);

  const dependabot = '.github/dependabot.yml';
  if (!existsSync(join(root, dependabot))) {
    fail(
      'supply-chain',
      `缺少 ${dependabot}：钉死的 SHA 不会被任何东西自动升级，AGENTS.md 的「升级走 Dependabot」无从执行`,
    );
  } else if (!/package-ecosystem:\s*github-actions/.test(read(dependabot))) {
    fail('supply-chain', `${dependabot} 没有 github-actions 条目，action 的 SHA 永远不会被推进`);
  } else {
    note('Dependabot 覆盖 github-actions（SHA 固定才有升级路径）');
  }
}

// --- 8. Element Plus 组件：模板用到的必须已注册 ------------------------------
//
// main.js 按「显式 import + app.component」注册 EP 组件，不引入 unplugin 自动
// 导入器。模板里写了 `<el-tabs>` 这类未注册组件时生产构建照样绿：Vue 把它当
// 未知自定义元素原样渲染——tab 头消失、所有 pane 内容平铺、v-model 静默失效
// （P8 插件双 tab 与迁移向导的勾选/单选都栽在这里）。把「模板 el-* ⊆ main.js
// 注册集合」钉进门禁。局部 `components:` 注册在本仓库不存在（已约定全局注册），
// 如未来引入需同步扩展本检查。
{
  const mainJs = read('ui/src/main.js');
  const registrationBlock = mainJs.match(
    /\[([\s\S]*?)\]\s*\.forEach\(\(component\)\s*=>\s*app\.component/,
  );
  if (!registrationBlock) {
    fail(
      'ep-registry',
      'main.js 里找不到 `[…].forEach((component) => app.component(component.name, component))` 注册块',
    );
  } else {
    // ElTableColumn → el-table-column；注册集合统一带 `el-` 前缀，与模板写法对齐。
    const toKebab = (ident) =>
      'el-' +
      ident
        .replace(/^El/, '')
        .replace(/([A-Z])/g, '-$1')
        .toLowerCase()
        .replace(/^-/, '');
    const registered = new Set(
      (registrationBlock[1].match(/El[A-Za-z]+/g) ?? []).map(toKebab),
    );
    const used = new Map();
    for (const file of walk(join(root, 'ui/src'), ['.vue'])) {
      for (const match of readFileSync(file, 'utf8').matchAll(/<el-([a-z][a-z0-9-]*)/g)) {
        const name = `el-${match[1]}`;
        if (!used.has(name)) used.set(name, show(file));
      }
    }
    const missing = [...used.keys()].filter((name) => !registered.has(name));
    if (missing.length > 0) {
      for (const name of missing) {
        fail(
          'ep-registry',
          `${used.get(name)} 用了 <${name}>，但 main.js 没有注册该组件——未知自定义元素会被原样渲染（tab 头消失、v-model 失效）`,
        );
      }
    } else {
      note(`EP 组件注册完整：模板用到 ${used.size} 种，全部已在 main.js 注册`);
    }

    // 反方向：注册了却没有任何模板用到的组件，同样是错的——它把 `theme-chalk`
    // 的样式一起打进产物，而 CI 的 CSS 预算是硬的。`el-table`（17 KB）就是这样
    // 在 MigrationPanel 改用自绘表格之后留在 main.js 里，白占了一个组件的体积，
    // 直到 UI CSS 只剩 838 字节余量才被发现。本仓库没有动态 `<component :is>`
    // 引用 EP 组件的写法（只有面板切换与图标），所以这里不会出现误报。
    const unused = [...registered.keys()].filter((name) => !used.has(name));
    if (unused.length > 0) {
      fail(
        'ep-unused',
        `main.js 注册了 ${unused.join('、')}，但没有任何模板用到——它们的样式会白进产物包` +
          '（CI 的 UI CSS 预算因此少掉相应体积）。请删掉 main.js 里的 import、style 与注册项。',
      );
    } else {
      note(`EP 组件无冗余注册：注册的 ${registered.size} 种全部有模板在用`);
    }

    // group 类组件没有模型时，子项的独立 :model-value 会被组模式忽略：
    // checkbox 进入 group 后读的是 group 的模型值，点了没反应（迁移向导
    // 来源勾选因此整个失效）。凡是用 group 就必须绑模型。
    for (const file of walk(join(root, 'ui/src'), ['.vue'])) {
      for (const match of readFileSync(file, 'utf8').matchAll(/<(el-checkbox-group|el-radio-group)\b([^>]*)>/g)) {
        const attrs = match[2];
        if (!/v-model|:model-value/.test(attrs)) {
          fail(
            'ep-registry',
            `${show(file)} 的 <${match[1]}> 没有 v-model / :model-value——子项会进组模式并忽略自己的 :model-value，点击静默失效`,
          );
        }
      }
    }
  }
}

// --- 8.6 Tauri 命令不许裸调 spawn_blocking ---------------------------------
//
// 事故来源：code-review-2026-09-27 的 L5。9 条命令写成了
// `spawn_blocking(…).await.map_err(|e| e.to_string())?`，于是后台任务 panic 时
// `JoinError` 被原样 `to_string()` 成一句英文 `task panicked` 透给用户——GUI
// 应用里这是他唯一能拿到的东西，而它不含任何可操作的下一步（AGENTS.md：
// 「错误信息必须包含可操作的下一步与相关日志路径」）。
//
// `blocking()` 助手已经把这条路径写对了：panic 时给出「后台任务异常结束…
// 请在终端用 `npm run dev` 启动以便看到完整输出」。本检查要求命令层统一走它。
//
// 判据是「`#[tauri::command]` 函数体里出现 `spawn_blocking(`」。注释里的
// 字面量已被剥离，不受影响；`blocking()` 助手自身的实现**在命令函数之外**，
// 因此不会被这条误伤。
{
  const commandsSrc = productionRust(read('src-tauri/src/commands.rs'));
  const bareCalls = [];
  for (const match of commandsSrc.matchAll(/pub async fn ([a-z0-9_]+)\([^)]*\)[^{]*\{/g)) {
    // 取函数体到下一处 `^}` 为止（命令都是顶层独立函数，缩进 0）。
    const start = match.index + match[0].length;
    const end = commandsSrc.indexOf('\n}', start);
    const body = commandsSrc.slice(start, end < 0 ? start + 4000 : end);
    if (body.includes('spawn_blocking(')) bareCalls.push(match[1]);
  }
  if (bareCalls.length > 0) {
    for (const name of bareCalls) {
      fail(
        'blocking-join-error',
        `commands::${name} 裸调 spawn_blocking —— 后台任务 panic 时 JoinError 会被 ` +
          "to_string() 成一句英文 `task panicked` 透给用户，不含任何可操作的下一步。" +
          '改用同文件的 blocking() 助手，它已经把这条路径的文案写对了。',
      );
    }
  } else {
    note('命令层不再裸调 spawn_blocking：panic 提示统一走 blocking()');
  }
}

// --- 8.5 icon-only 按钮必须有无障碍名称与 hover 说明 ------------------------
//
// 事故来源：code-review-2026-09-27 的 L3。`circle` + `:icon` 的按钮**对屏幕
// 读屏器是空的**——可访问名只能来自文本、`aria-label` 或 `aria-labelledby`，
// 而图标本身不是文本。外层 `el-tooltip` 只在鼠标悬停时才出现，键盘与读屏
// 用户永远看不到它，于是这些按钮等于不存在。八个按钮全是插件/技能的更新、
// 打开仓库、卸载这类**唯一的**操作入口。
//
// 这类缺陷没有任何构建期信号：Vue 不校验 aria 属性，test:ui 也钉不住「一个
// 按钮该叫什么」。所以钉进门禁。
//
// **2026-10-10 两处扩面**，起因是用户指着内核版本页那两个按钮报「无文字的按钮
// 背景范围需要缩小，增加 hover 后的功能说明文字」：
//
//   ① 判据原来只认 `circle` 这一形状，但**真正的问题按钮往往不带 `circle`**
//      ——内核版本页那两枚是 `text` + 自闭合，此前整条判据对它们是瞎的。
//      改为按「自闭合 el-button（插槽为空）且有 `:icon`」判定，这才是 icon-only
//      的**定义**，而不是它的一种画法。
//   ② 增加 `title` 要求。原判据只管读屏（`aria-label`），而 `title` 是**唯一
//      对键盘用户也生效**的那份 hover 说明——`el-tooltip` 只认鼠标。
//
// 判据仍逐条报出文件名与行号，而不是给一句总数。
{
  const missingAria = [];
  const missingTitle = [];
  for (const file of walk(join(root, 'ui/src'), ['.vue'])) {
    const text = readFileSync(file, 'utf8');
    // **必须只匹配真正自闭合的标签**：`<el-button … />`。这一条踩了两次坑，
    // **必须只匹配真正自闭合的标签**：`<el-button … />`。
    //
    // 这一条踩了三次坑，三次都是判据自己写错：前两版给出成片假阳性，
    // 第三版反过来漏报——**假门禁比没有门禁更坏**，它让人以为报出来的都是
    // 真问题，从而忽略真正的问题，所以每一步都用反向验逼出真实行为。
    //
    // ① `/<el-button\b([\s\S]*?)\/>/g`：惰性匹配从一个按钮的开标签一路
    //    往后找 `/>`，跨过它自己的文字与闭标签、吞到**下一个**自闭合标签去。
    //    「选择并安装内核」这类有文字的按钮全被判成 icon-only，误报 20 处。
    // ② `/<el-button\b((?:[^>]|>(?!\/))*)\/>/g`：加了「`>` 后不能紧跟 `/`」，
    //    但闭标签 `</el-button>` 的 `/` 在 `>` **之前**，那个 `>` 照样放行，
    //    于是继续往后吞，又误报 8 处。
    // ③ `/<el-button\b((?:[^<]|<(?!\/))*)\/>/g`：试图用 `</` 断配对，
    //    但 `(?!\/)` 拦不住**另一个开标签**的 `<`——`<el-button` 的 `<` 后面
    //    跟的是 `e` 而非 `/`，于是第一个匹配把第二个按钮整段吞进捕获块，
    //    判据于是对第二个按钮完全失明（反向验实测：只报 1 处，实际破坏 3 处）。
    //
    // 现在这版干脆**不允许捕获块里出现任何 `<`**：标签属性里本就不该有裸 `<`
    // （`v-if="a<b"` 这种写法本仓 119 个按钮里确无），而闭合标签必定含 `<`，
    // 于是「自闭合」与「有文字」被干净地分开——这正是判据要判的那件事。
    for (const match of text.matchAll(/<el-button\b([^<]*)\/>/g)) {
      const attrs = match[1];
      if (!/:icon=|v-bind:icon=/.test(attrs)) continue;
      const line = text.slice(0, match.index).split('\n').length;
      const where = `${relative(root, file).split(sep).join('/')}:${line}`;
      if (!/\baria-label(?:ledby)?=/.test(attrs)) missingAria.push(where);
      if (!/(?:^|\s):?title=/.test(attrs)) missingTitle.push(where);
    }
  }
  for (const where of missingAria) {
    fail(
      'a11y-icon-button',
      `${where} 是只有图标的按钮，却没有 aria-label —— 屏幕阅读器读不出它是什么，` +
        '鼠标用户看得见的 tooltip 对键盘与读屏用户不存在。补 :aria-label="\'动作 \' + 实体名"，' +
        '照同一文件里 el-switch :aria-label 的写法。',
    );
  }
  for (const where of missingTitle) {
    fail(
      'a11y-icon-button',
      `${where} 是只有图标的按钮，却没有 title —— 2026-10-10 用户定的规范：每个无文字` +
        '按钮都要有 hover 后的功能说明文字。el-tooltip 只认鼠标，title 是键盘用户' +
        '唯一能拿到的那一份。两个补齐即可（文案与 aria-label 保持一致）。',
    );
  }
  if (missingAria.length === 0 && missingTitle.length === 0) {
    note('icon-only 按钮都带 aria-label 与 title');
  }
}

// --- 9. CSS 自定义属性：var() 引用必须有定义 ---------------------------------
//
// 事故来源：code-review-2026-09-27 的 H4。`--text-muted` / `--surface-soft`
// 被当时的顶部工作条与 MigrationPanel 引用了 8 处，却从未在 :root 定义。CSS 自定义
// 属性没有回退值时整条声明在 computed-value time 非法 → 被丢弃 → 继承父级，
// 于是「压低非激活 tab 权重」「弱化提示文字」的设计全部静默失效，而**不产生
// 任何构建错误或构建警告**，102 个 test:ui 与原有 10 项不变量都发现不了。
//
// 这类缺陷靠人读代码只能碰运气，因此钉成门禁。带回退值的引用
// （`var(--x, #999)`）不在管辖范围内——有回退就不算静默失效。JS 里通过
// `:style` 注入的自定义属性（`--sidebar-width`）由父组件负责，命中回退时
// 同样不受影响。
{
  const srcDir = join(root, 'ui/src');
  // **定义**来自所有 `.css` 与 `.vue` 里的 `:root`，不再只认 theme.css。
  //
  // 2026-10-06：这条判据写的时候全仓只有 theme.css 一份样式表，定义收在它
  // 上面是「唯一定义处」这个事实的**记述**，不是一条设计决定。而浮层层级
  // 阶梯（--z-*）按审查意见 P1-01 必须写在一处，那份阶梯位于 diagnostics.css。
  //
  // 把它改成扫全部 CSS 是**修正**而不是放宽：门禁要保的不变量是「没有静默失效
  // 的 var() 引用」，而 `:root` 自定义属性本来就是全局的，定义在哪个样式表里
  // 运行时没有区别。这个改动只可能减少假阳性，不可能放过一个真缺失。
  //
  // `.vue` 只认 `:root` 内的定义：组件的 `<style scoped>` 里定义的变量是**局部
  // 的**，别的文件用不到，收进来会让判据失明。
  const defined = new Set();
  // **先剥注释再扫**（2026-10-08）。这条判据原先按**原文**匹配，于是两个方向
  // 都错：
  //   · 假阳性——theme.css 里一句解释「Element Plus 的 `.el-icon` 颜色是
  //     `var(--color)`」的注释被判成一次无回退引用，`.el-icon` 的 `--color`
  //     由**组件库**定义、不在本仓任何 `:root` 里，于是门禁红了，而真实缺陷
  //     （真声明里少一个定义）一个都没有。
  //   · 假阴性——`defined` 那一侧扫的是全文，一条**注释里**写的 `--x:` 会被
  //     当成「已定义」，于是真声明里那个真的没有定义的 `--x` 被放过。
  // 两边都是同一个根因：**注释不是声明**。剥掉它只可能更准，不可能放过真缺失
  // ——这与本仓「判据扫模板不扫全文」的纪律是同一条。
  const stripForScan = (text) =>
    text
      .replace(/\/\*[\s\S]*?\*\//g, '')
      // 行注释只在行首或空白之后才剥：`http://` 这种 URL 里也有 `//`。
      .replace(/(^|[\s;{(])[^\n]*?(\/\/[^\n]*)$/gm, '$1');
  const collect = (text, onlyRoot) => {
    const source = onlyRoot
      ? [...stripForScan(text).matchAll(/:root\s*\{([^}]*)\}/g)].map((m) => m[1]).join('\n')
      : stripForScan(text);
    for (const m of source.matchAll(/(^|[;{\s])(--[a-z0-9-]+)\s*:/g)) defined.add(m[2]);
  };
  for (const file of walk(srcDir, ['.css'])) collect(readFileSync(file, 'utf8'), false);
  for (const file of walk(srcDir, ['.vue'])) collect(readFileSync(file, 'utf8'), true);
  const missing = new Map();
  for (const file of walk(srcDir, ['.css', '.vue', '.js'])) {
    const text = stripForScan(readFileSync(file, 'utf8'));
    for (const match of text.matchAll(/var\(\s*(--[a-z0-9-]+)\s*([,)])/g)) {
      // 名字后面跟逗号 = 带了回退值（`var(--x, #999)`），有定义与否都不会
      // 静默失效；跟右括号 = 没给回退，才需要真有定义。
      if (match[2] === ',') continue;
      const name = match[1];
      if (defined.has(name) || missing.has(name)) continue;
      missing.set(name, show(file));
    }
  }
  if (missing.size > 0) {
    for (const [name, where] of missing) {
      fail(
        'css-var',
        `${where} 引用了 ${name}，但没有任何样式表在 :root 里定义它（且未给回退值）——整条声明会被浏览器丢弃并继承父级，样式静默失效`,
      );
    }
  } else {
    note('CSS 变量引用完整：无回退的 var() 全部有定义');
  }
}

// --- 9. IPC 字段契约：前端读的名字必须是后端真的发出来的名字 ---------------
//
// 这一项来自一次真实事故：安全网 P0/P1 的整套面板在读 snake_case，而 Rust
// 侧 `RestoreDiff` / `SnapshotListView` / `BisectView` 全部是
// `rename_all = "camelCase"`。结果是「回到良好状态」点了没反应（id 恒为
// undefined，`if (!id) return` 直接短路）、动不了的条目数恒不显示、每条快照
// 的四个维度全渲染成「未知 / 0」、二分「开始排查」按钮恒置灰——而编译、
// 单测、代码预算三道门禁全绿，因为前端测试喂的是**手写的 snake_case 夹具**，
// 从来没有一份真实响应穿过它。
//
// 修好之后把这条钉死：凡是 Rust 里声明了会改名序列化的结构体，其字段的
// snake_case 形式在前端被读到就是错的。

/// `snake_case` → `camelCase`，与 serde 的 rename_all = "camelCase" 对齐。
function toCamel(name) {
  return name.replace(/_([a-z0-9])/g, (_, c) => c.toUpperCase());
}

const renamedFields = new Map(); // snake → { camel, struct, file }
{
  const srcDir = join(root, 'src-tauri', 'src');
  // 递归：判据的覆盖面不该取决于文件放在哪一层。src-tauri/src 一旦分目录，
  // 非递归的 readdirSync 会让下面四项检查**静默失去覆盖**——不报错，只是再也
  // 看不到那些文件了。
  for (const rel of walk(srcDir, ['.rs'])) {
    const file = rel.slice(srcDir.length + 1);
    const text = readFileSync(rel, 'utf8');
    const re =
      /#\[derive\([^)]*Serialize[^)]*\)\]\s*#\[serde\(rename_all\s*=\s*"camelCase"\)\]\s*pub struct\s+(\w+)\s*\{([^}]*)\}/gs;
    let m;
    while ((m = re.exec(text)) !== null) {
      const [, structName, body] = m;
      for (const field of body.matchAll(/pub\s+([a-z][a-z0-9_]*)\s*:/g)) {
        renamedFields.set(field[1], {
          camel: toCamel(field[1]),
          struct: structName,
          file,
        });
      }
    }
  }
}

if (renamedFields.size > 0) {
  const uiDir = join(root, 'ui', 'src');
  const badReads = new Map();
  const walk = (dir) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const full = join(dir, entry.name);
      if (entry.isDirectory()) {
        walk(full);
        continue;
      }
      if (!/\.(js|vue)$/.test(entry.name)) continue;
      const rel = relative(uiDir, full);
      const text = readFileSync(full, 'utf8');
      for (const access of text.matchAll(/\.([a-z][a-z0-9]*(?:_[a-z0-9]+)+)/g)) {
        const hit = renamedFields.get(access[1]);
        if (!hit) continue;
        const line = text.slice(0, access.index).split('\n').length;
        badReads.set(
          `${rel}:${line}`,
          `${rel}:${line} 读 .${access[1]}，但 Rust 的 ${hit.struct}.${hit.camel} 才是实际发出的名字（${hit.file}）`,
        );
      }
    }
  };
  walk(uiDir);
  for (const message of badReads.values()) {
    fail(
      'ipc-fields',
      `${message} —— 取到的是 undefined：字段会静默变成「未知 / 0」，或让按钮直接失灵`,
    );
  }
  if (badReads.size === 0) {
    note(
      `IPC 字段契约：前端没有读任何会被 camelCase 改名的字段（已覆盖 ${renamedFields.size} 个）`,
    );
  }
}

// --- 10/11. 实例归属的两条硬纪律 --------------------------------------------
//
// 这两条来自一次真实的排查事故。dev 与 release 两个壳把自己的数据目录分家了
// （`desktop/` 与 `desktop-dev/`），实例却还共用同一个默认实例——于是 dev 换一次
// 内核版本、重跑一次 `profiles/web/` 接线，就落在 release 正在跑的内核上，工作台
// 当场白屏（`scope '…' rendered without an installed adapter`）。分家之后又留下一条
// 幽灵记录：注册表里除 `default-dev` 外还多出一条 `default`，端口被写成 dev 的 3091、
// `kernel_version` 为空。
//
// 追了两轮才定位到写入者：一个**前端从不调用**的 Tauri 命令
// `ensure_default_instance_migrated`，它把 id 写死成 `DEFAULT_INSTANCE_ID`。dev 壳
// 走到它就会往**共享**注册表里塞一条属于 release 名字的记录，并顺手改掉共享指针。
//
// 教训不能只留在 commit message 里。这两条检查把它变成机械可查的：
//   ① 生产代码里 `InstanceRecord::new` 的 id 实参不得是常量——常量意味着「这条路径
//      不管当前是哪个壳，都会造出同一个 id 的记录」；
//   ② 注册表那个 `default_instance_id` 是 dev 与 release 共享的一份，是
//      `instance::default_family()` 解析 `data_dir` 的输入，因此只准 `instance.rs`
//      写。壳自己「切到哪个实例」属于壳内状态，走 `settings.current_instance_id`。

/** 去掉 `#[cfg(test)]` 整块与行注释，只留下真正的生产代码。
 *
 * **行号必须对齐**：注释与测试块各自替换成等长的空串，而不是从数组里删掉，
 * 否则下面报出来的行号会指到不相干的地方（测试块一长就偏出去几十行）。
 */
function productionRust(text) {
  const lines = text.split('\n');
  const kept = [];
  for (let i = 0; i < lines.length; i += 1) {
    const trimmed = lines[i].trim();
    if (/^#\[cfg\(.*\btest\b.*\)\]$/.test(trimmed)) {
      let depth = 0;
      let seen = false;
      kept.push('');
      for (i += 1; i < lines.length; i += 1) {
        kept.push('');
        const code = lines[i].replace(/\/\/.*$/, '');
        depth += (code.match(/\{/g) || []).length - (code.match(/\}/g) || []).length;
        if (code.includes('{')) seen = true;
        if (seen && depth <= 0) break;
      }
      continue;
    }
    kept.push(trimmed.startsWith('//') ? '' : lines[i].replace(/\/\/.*$/, ''));
  }
  return kept.join('\n');
}

/** 取 Rust 函数的函数体（按大括号配平），找不到时返回 null。
 *
 * 两处细节：
 *  - **泛型参数在函数名与左括号之间**（`fn f<T: AsRef<OsStr>>(…)`），所以
 *    定位到函数名后要先跳过尖括号配平，再找参数列表的左括号。
 *  - 用 `fn <name>[(<]` 定位，避免前缀相同的函数互相误命中——`fn spawn(` 与
 *    `fn spawn_log_drain(` 只差一个下划线，按 `fn spawn` 找会拿到后者。
 */
function rustFunctionBody(source, name) {
  const start = source.search(new RegExp(`fn ${name}[(<]`));
  if (start < 0) return null;
  // 跳过泛型参数，找到参数列表的左括号（泛型约束里也可能出现括号）。
  let i = start;
  let angle = 0;
  while (i < source.length) {
    const ch = source[i];
    if (ch === '<') angle += 1;
    else if (ch === '>') angle -= 1;
    else if (ch === '(' && angle === 0) break;
    i += 1;
  }
  if (i >= source.length) return null;
  // 配平参数列表的圆括号。
  let parens = 0;
  for (; i < source.length; i += 1) {
    if (source[i] === '(') parens += 1;
    else if (source[i] === ')') {
      parens -= 1;
      if (parens === 0) {
        i += 1;
        break;
      }
    }
  }
  const open = source.indexOf('{', i);
  if (open < 0) return null;
  let braces = 0;
  for (let j = open; j < source.length; j += 1) {
    if (source[j] === '{') braces += 1;
    else if (source[j] === '}') {
      braces -= 1;
      if (braces === 0) return source.slice(open, j + 1);
    }
  }
  return null;
}

{
  const srcDir = join(root, 'src-tauri', 'src');
  // 递归，理由同上面那处：分目录后非递归扫描会静默漏掉子目录里的文件。
  const rustFiles = walk(srcDir, ['.rs']).map((full) => full.slice(srcDir.length + 1));

  // ① 实例 id 不得 hard-code。
  const hardCodedIds = [];
  for (const file of rustFiles) {
    const lines = productionRust(readFileSync(join(srcDir, file), 'utf8')).split('\n');
    lines.forEach((line, index) => {
      const call = /InstanceRecord::new\(\s*([^,)]+)/.exec(line);
      if (!call) return;
      const arg = call[1].trim();
      const isConstant =
        /^(instance::)?(DEV_)?DEFAULT_INSTANCE_ID$/.test(arg) || /^"[^"]*"$/.test(arg);
      if (isConstant) {
        hardCodedIds.push(
          `${file}:${index + 1} 造实例时把 id 写死成 ${arg}——dev 壳与 release 壳会造出同一个 id`,
        );
      }
    });
  }
  if (hardCodedIds.length > 0) {
    for (const message of hardCodedIds) {
      fail(
        'instance-id-constant',
        `${message}。当前壳的实例 id 走 instance::default_instance_id()` +
          '（或 current_instance_id()），常量会造出不属于这个壳的记录。',
      );
    }
  } else {
    note(`实例 id 不写死：生产代码里 ${rustFiles.length} 个文件没有常量实参`);
  }

  // ② 共享的 default_instance_id 只准 instance.rs **指向**某个 id。
  //    清空（`= None`）不算抢：那是把指针还原成「无人认领」，setup 的
  //    `ensure_default_registered` 会按 release 的默认值把它重建回来——删掉被指向的
  //    实例后清空指针是**修复**（跨壳误认领之后唯一的自愈路径），必须留着。
  const sharedWriters = [];
  for (const file of rustFiles) {
    if (baseName(file) === 'instance.rs') continue; // 唯一合法的写者
    const lines = productionRust(readFileSync(join(srcDir, file), 'utf8')).split('\n');
    lines.forEach((line, index) => {
      const assign = /\.default_instance_id\s*=(?!=)\s*(.+?);?\s*$/.exec(line);
      if (!assign) return;
      if (/^None\s*$/.test(assign[1].trim())) return; // 清空是修复，不是抢
      sharedWriters.push(`${file}:${index + 1}`);
    });
  }
  if (sharedWriters.length > 0) {
    for (const where of sharedWriters) {
      fail(
        'shared-registry-default',
        `${where} 把注册表的 default_instance_id 指向了某个实例 —— 那是 dev 与 release 共享的一份，` +
          '也是 instance::default_family() 解析 data_dir 的输入：被任意一个壳改掉，' +
          '另一个壳下次启动就会把数据目录指到不相干的实例上，而它不可自愈' +
          '（ensure_default_registered 只在「无人认领」时认领）。壳内选择请写 ' +
          'settings.current_instance_id。（清空成 None 不算——那是修复。）',
      );
    }
  } else {
    note('共享的 default_instance_id 只有 instance.rs 会指向具体实例');
  }

  // ③ 共享的 default_instance_id **不得被读**来决定路径 / 族。
  //
  // 这份指针曾是 `default_family()` 解析 `data_dir` 的输入，而 `data_dir` 装的是
  // 内核安装树（active.txt + kernels/<version>/ 几百 MB）。让「本壳的数据目录」
  // 取决于「另一个壳上次写了什么」是一次典型的跨壳竞态：谁先写谁赢，写错的
  // 一方还不自愈（认领条件是「无人认领」）。2026-09-29 起族解析只走壳自己的
  // 状态（本壳当前实例 → 本壳默认实例 → dsh），该字段降级为**只写不读**的
  // 旧版本兼容位。这条检查让「不要接回去」变成机械可查的。
  const sharedReaders = [];
  for (const file of rustFiles) {
    if (baseName(file) === 'instance.rs') continue; // 唯一合法的维护者
    const lines = productionRust(readFileSync(join(srcDir, file), 'utf8')).split('\n');
    lines.forEach((line, index) => {
      // 写（`= `）由 ② 管；这里只抓读：取值 / 比较 / 传给函数。
      if (/\.default_instance_id\s*(=[^=]|\+=)/.test(line)) return;
      if (!/\.default_instance_id\b/.test(line)) return;
      sharedReaders.push(`${file}:${index + 1}`);
    });
  }
  if (sharedReaders.length > 0) {
    for (const where of sharedReaders) {
      fail(
        'shared-registry-read',
        `${where} 读了注册表里共享的 default_instance_id —— 那是 dev 与 release 共享的一份可变字段：` +
          '任何一壳写入都可能让另一壳下次启动把数据目录（= 内核安装树）指到不相干的地方，' +
          '且写错的一方不自愈。要按实例解析，先看 instance::current_instance_id()（壳内选择，' +
          '两个壳各一份）或 instance::default_family()（已只走壳自己的状态）。',
      );
    }
  } else {
    note('共享的 default_instance_id 无人读作决策依据');
  }

  // ④ 内核树的三条变更动作：**只有本壳守卫能阻断**，跨壳一律「提示不拦」。
  //
  // 2026-09-30 这条走过一次弯路，值得把过程记在这里。三条动作一度共用一条
  // `ensure_workbench_stopped`，它对**另一个壳**的工作台也硬拦，理由是「内核的
  // 文件监视器会把事件风暴当成模块图变更，即使变化发生在它根本不服务的目录里」。
  // **那个机制查不实**：装了 0.2.0-rc.2 的内核全树只有四处 chokidar（凭据文件、
  // `fs.watch` 点名的 target 且 depth 0、profile 的 patch 文件、技能根），没有一处
  // 盯内核安装树，`bootRev` 零命中。对得上的是**机器资源争用**：pnpm 硬链接 + node-gyp
  // 打满 CPU 与磁盘 → 对方页面加载被拖过看门狗阈值 → 自动重载 → 撞上启动顺序竞态
  // （release 内核那 10 秒一行输出都没有，它没被惊动，出事的是客户端）。
  //
  // 概率性的资源争用配一条硬拦，代价是**废掉双壳并行**——而双壳并行正是 dev 壳
  // 存在的理由。降优先级（`child_priority.rs`）才是对症的那一半。跨壳因此降级为
  // `warn_other_shell_workbench`：照做，但把后果与出路说清楚。
  //
  // **本壳那条守卫必须留着**：装 / 删改的是自己脚下那棵树，运行中的内核就在里面。
  // 写这条检查是因为「谁走哪条」钉不住：`instance_kernel_running` 的正向判据要求
  // 真有一个 dsh 内核在跑（pid 活体 + `command_is_kernel` 身份校验），单测造不出来。
  const kernelSrc = productionRust(shellSource('lifecycle.rs'));
  // 文案里点名文件时用解析出来的真实路径，别写死 `kernel.rs`——模块搬进
  // kernel/ 之后，那三个字已经指不到任何文件，报错会让人去翻一个不存在的地方。
  const kernelFile = shellSourceLabel('lifecycle.rs');
  const fnBody = (name) => {
    const lines = kernelSrc.split('\n');
    const start = lines.findIndex((line) => new RegExp(`^pub(\\(crate\\))? fn ${name}\\b`).test(line));
    if (start < 0) return null;
    for (let i = start; i < lines.length; i += 1) {
      if (lines[i] === '}') return lines.slice(start, i + 1).join('\n');
    }
    return null;
  };
  const guardMisses = [];
  // ① 三条动作都必须过本壳守卫。
  for (const action of ['set_active', 'install_version', 'uninstall']) {
    const body = fnBody(action);
    if (body === null) {
      guardMisses.push(`${kernelFile} 里找不到 ${action} —— 守卫的去向就无从检查了`);
      continue;
    }
    if (!body.includes('ensure_own_shell_stopped(')) {
      guardMisses.push(`${kernelFile} 的 ${action} 没有调用 ensure_own_shell_stopped —— 本壳工作台运行期间必须拦住`);
    }
  }
  // ② 旧的跨壳硬拦入口不得复活（名字一旦回来，就有人会再挂一个 Err 上去）。
  //    只查 kernel.rs：补丁子系统已整体删除（2026-10-11），这里曾要排除
  //    `patches::ensure_workbench_stopped`（补丁应用 / 撤销的停机守卫）。
  if (kernelSrc.includes('ensure_workbench_stopped(')) {
    guardMisses.push(`${kernelFile} 又出现了 ensure_workbench_stopped —— 跨壳判据已降级为提示，硬拦入口不该复活`);
  }
  // ③ 跨壳判据的调用点只允许在 kernel.rs 的提示与状态两处，且**不得**出现在
  //    任何会 return Err 的路径上——这正是「提示不拦」的可查形式。定义行本身
  //    （`pub fn workbench_running_in_other_shell(`）不算调用点。
  const otherShellCallSites = [];
  for (const file of rustFiles) {
    const lines = productionRust(readFileSync(join(srcDir, file), 'utf8')).split('\n');
    lines.forEach((line, index) => {
      if (!line.includes('workbench_running_in_other_shell(')) return;
      if (/^\s*(pub(\(crate\))?\s+)?fn\s+workbench_running_in_other_shell\b/.test(line)) return;
      otherShellCallSites.push(`${file}:${index + 1}`);
    });
  }
  const unexpected = otherShellCallSites.filter((where) => !inFile(where, 'lifecycle.rs'));
  if (unexpected.length > 0) {
    guardMisses.push(
      `${unexpected.join('、')} 调用了 workbench_running_in_other_shell —— 跨壳判据只该出现在 ` +
        `${kernelFile} 的 warn_other_shell_workbench（提示）与 status（让 UI 在点之前看见）两处`,
    );
  }
  if (otherShellCallSites.length !== 2) {
    guardMisses.push(
      `workbench_running_in_other_shell 在生产代码里出现了 ${otherShellCallSites.length} 个调用点` +
        '（期望 2：warn_other_shell_workbench 与 status）—— 多出来的那个八成是在某个 ' +
        'Result 路径上把它接成了阻断，那正是要挡回来的改法',
    );
  }
  // ④ 跨壳提示的**返回类型必须是无**。这是唯一挡得住「把提示接成一次拒绝」的
  //    形式：签名返回 `()`，调用方就没有 `?` 可接；要新增跨壳检查点只能新增调用
  //    点，而那由 ③ 计数。**只按名字查是不够的**——第一版检查就栽在这里：它只禁
  //    `ensure_workbench_stopped` 这个名字，于是「把 warn 的返回值接成 Err」一路
  //    畅通，检查始终绿着。
  const warnSignature = /fn\s+warn_other_shell_workbench[\s\S]*?\{/.exec(kernelSrc);
  if (warnSignature === null) {
    guardMisses.push(`${kernelFile} 里找不到 warn_other_shell_workbench —— 跨壳提示的落点没了`);
  } else if (warnSignature[0].includes('->')) {
    guardMisses.push(
      `warn_other_shell_workbench 的返回类型变成了 ${warnSignature[0].split('->')[1].split('{')[0].trim()}` +
        ' —— 跨壳判据降级为提示后就该返回无，返回一个值等于把「接成阻断」的路重新打开',
    );
  }
  if (guardMisses.length > 0) {
    for (const message of guardMisses) {
      fail(
        'kernel-stop-guard-scope',
        `${message}。本壳守卫（ensure_own_shell_stopped）三条动作都要过——装 / 删改的是` +
          '自己脚下那棵树，运行中的内核就在里面。跨壳判据只负责**提示**：另一个壳可能因' +
          '机器资源争用而卡住或黑屏，但两个壳的安装树物理不相交、不碰数据，恢复只要在' +
          '那个壳关掉工作台窗口再重新打开——为它硬拦住整个动作，等于废掉双壳并行。',
      );
    }
  } else {
    note('内核树只有本壳守卫能阻断，跨壳判据只提示不拦（双壳并行）');
  }

  // ⑤ 工作台自愈链必须**接到底**：黑屏之后壳要自己换窗口，而不是弹个面板就停着。
  //
  // 2026-09-30 实测的形状：`harness_window::fault_needs_new_window` 把「刷新已经
  // 救不回来」判得一清二楚，**却没有任何生产调用方**——只有测试引用它。于是链条的
  // 末端是：页面自己刷过一次、没刷好、壳弹一个事故面板、**窗口停在黑屏上**。而手动
  // 「刷新工作台」（同一个 `recreate`）恰恰证明换窗口能救回来。是**漏接**，不是判断
  // 为「不该接」。
  //
  // 写成机械检查的理由和第 12 项一样，而且更硬：**单测永远抓不到它**。纯函数
  // 测得再准，摘掉调用它照样全绿（第一版就是这么骗过去的——把
  // `port_open(settings.port)` 改成 `false`，判据测试依然 ok）。要能抓住，判据就
  // 必须落到「命令层那个函数体里同时出现了判据与 recreate」这种**接线**的形状上。
  {
    const commandsSrc = productionRust(shellSource('commands.rs'));
    const lines = commandsSrc.split('\n');
    const start = lines.findIndex((line) => line.startsWith('pub async fn report_harness_fault('));
    const selfHealMisses = [];
    if (start < 0) {
      selfHealMisses.push('commands.rs 里找不到 report_harness_fault —— 工作台页面报故障的入口没了');
    } else {
      let body = '';
      for (let i = start; i < lines.length; i += 1) {
        body += `${lines[i]}\n`;
        if (lines[i] === '}') break;
      }
      if (!body.includes('harness_window::recreate_after_fault')) {
        selfHealMisses.push('没有走「黑屏后自己换窗口」那条动作');
      }
    }
    // 判据必须落在那条动作**里面**：动作与判据被拆开时，命令层只该看到动作。
    const harnessSrc = productionRust(shellSource('harness_window.rs'));
    // 证据必须真的在 Incident 上：命令层是从 `incident.health` 取出来递给判据的
    // （命令层不应重复维护诊断证据）。`diagnose_runtime`
    // 把它设成 None 的话，`if let Some(health)` 那一步会静默跳过，**第三层整个不工作
    // 且没有任何线索**——与 ACL 事故同一类。它的提前返回路径由
    // `existing.health.as_ref() == Some(&report)` 这个条件保证带着 health，所以这里
    // 只需钉住唯一那个构造点。
    const guardSrc = productionRust(shellSource('guard.rs'));
    if (!/fn diagnose_runtime\([\s\S]*?health: Some\(report\)/.test(guardSrc)) {
      selfHealMisses.push('guard::diagnose_runtime 不再把 health 带在 Incident 上');
    }
    const actionLines = harnessSrc.split('\n');
    const actionStart = actionLines.findIndex((line) => line.startsWith('pub fn recreate_after_fault('));
    if (actionStart >= 0) {
      let actionBody = '';
      for (let i = actionStart; i < actionLines.length; i += 1) {
        actionBody += `${actionLines[i]}\n`;
        if (actionLines[i] === '}') break;
      }
      if (!actionBody.includes('should_recreate_after_fault')) {
        selfHealMisses.push('recreate_after_fault 里没有问判据，等于无脑重建');
      }
      // 2026-09-30 下午的真机数据：自动重建落在对面卸载内核的风暴中间
      // （15:36:32 重建 → 15:36:40 新窗口又撞死）。「等风停再动手」如果只写在
      // commit message 里，下一次重构就会把它接丢——和判据没人调是同一类洞。
      if (!actionBody.includes('recreate_when_quiet')) {
        selfHealMisses.push(
          'recreate_after_fault 没走「等风停再重建」（recreate_when_quiet）——' +
            '风暴中间重建等于再掷一次骰子（2026-09-30 实测：重建 4 秒后新窗口又死）',
        );
      }
    }
    if (selfHealMisses.length > 0) {
      fail(
        'harness-self-heal-wiring',
        `report_harness_fault ${selfHealMisses.join('、')}——` +
          '工作台黑屏后壳就只会弹一个事故面板然后停在那儿。手动「刷新工作台」走的是同一个 ' +
          '`harness_window::recreate`，它能救回来，自动链没走这一步是漏接。' +
          '注意：这一项单测抓不到（纯函数测得再准，摘掉调用它照样全绿）。',
      );
    } else {
      note('工作台自愈链接到了 recreate（黑屏后自己换窗口）');
    }
  }

  // ⑥ 注入脚本 `invoke` 时传的对象键，必须与 Rust 命令的形参名逐个对得上。
  //
  // 这一类漂移**四处都不报**：JS 那边拼错一个键、编译不过、测试照过、门禁照过——
  // 后果是那条命令反序列化失败，而调用方的 `.catch` 把它吞掉，于是「草稿功能突然
  // 不工作了」而没有任何线索。2026-09-30 已经在**返回值**上栽过一次同类的
  // （`settings_warning` 与 camelCase 的对不上，靠一个单测才抓到），这里是入参
  // 那一侧。命令名由第 3 项查（已注册 + 已授权），参数名要单独查。
  {
    const harnessScripts = [
      'harness/harness-draft.js',
      'harness/harness-health.js',
      // 2026-10-17 起工作台的右键兜底脚本也 invoke 壳（弹原生编辑菜单）。漏进这份
      // 名单不会有任何症状：参数名漂了是 ACL 静默拒 + `.catch` 吞掉，「右键没有
      // 菜单」看起来只像没做这个功能。
      'harness/context-menu-fallback.js',
      // 2026-10-18 起外观镜像脚本也 invoke 壳（`set_window_appearance`）。漏进这份
      // 名单没有任何症状：ACL 没授就是静默拒 + `.catch` 吞掉，症状只是「浅色系统
      // 下选深色 → 标题栏还是白的」。
      'harness/workbench-appearance.js',
    ].map((name) => ({
      name,
      source: readFileSync(join(srcDir, name), 'utf8'),
    }));
    const cmdSrc = productionRust(shellSource('harness_cmd.rs'));
    /** Rust 形参名：只取简单标识，跳过 `app: AppHandle` 之类的注入参数。 */
    const rustParams = (fnName) => {
      const hit = new RegExp(`pub fn ${fnName}\\(([^)]*)\\)`).exec(cmdSrc);
      if (!hit) return null;
      return hit[1]
        .split(',')
        .map((piece) => piece.split(':')[0].trim())
        .filter((name) => /^[a-z_][a-z0-9_]*$/.test(name));
    };
    /** Tauri 命令参数走 camelCase→snake_case 的自动映射（`pageUrl` ⇒ `page_url`）。 */
    const toSnake = (key) => key.replace(/([a-z0-9])([A-Z])/g, '$1_$2').toLowerCase();
    const paramMisses = [];
    // 每个注入脚本里每一处 `invoke('<cmd>', { … })` 的对象键。引号两种都认：
    // harness-draft.js 用单引号、harness-health.js 用双引号——只认一种的检查会
    // 对另一个脚本**整体失明**（旧版就是这样，ACL 那半从未覆盖过
    // `report_harness_fault`，直到反向验抓住这个洞）。
    const callRe = /invoke\(\s*['"]([a-z_]+)['"]\s*,\s*\{([\s\S]*?)\}\s*\)/g;
    for (const { name, source } of harnessScripts) {
      let match;
      callRe.lastIndex = 0;
      while ((match = callRe.exec(source)) !== null) {
        const command = match[1];
        const keys = [...match[2].matchAll(/(^|[{,\s])([a-z_][a-zA-Z0-9]*)\s*:/g)].map((k) => k[2]);
        const expected = rustParams(command);
        if (expected === null) continue; // 命令不在 harness_cmd.rs 里，第 3 项管
        for (const key of keys) {
          if (!expected.includes(toSnake(key))) {
            paramMisses.push(
              `${name} 里 ${command} 的入参对不上：脚本传了 ${key}，Rust 那边没有这个形参（现有 ${expected.join(' / ') || '无'}）`,
            );
          }
        }
      }
    }
    if (paramMisses.length > 0) {
      // 同一个命令可能在脚本里有多处调用（存一次、放回去一次），报两遍只是噪音。
      for (const message of new Set(paramMisses)) {
        fail(
          'harness-invoke-params',
          `${message}。反序列化失败会被调用方的 .catch 吞掉，于是那项功能静默失效` +
            '——编译不报、测试不红、门禁不响。改名字要两边一起改。',
        );
      }
    } else {
      note('注入脚本 invoke 的参数名与 Rust 命令形参一致');
    }

    // ⑥之半：**命令名**也要对得上。注入脚本跑在 harness 窗口里，那是个 remote
    //    origin（http://127.0.0.1:<port>），Tauri's ACL 默认拒它的一切 invoke，
    //    只有 capabilities/harness-remote.json 里显式授过的那几条能用。
    //
    //    2026-09-30 实测踩中：两条草稿命令登记进了 app-commands.json（第 1、2 项
    //    因此都绿），却**没授给 harness 窗口**——真机上 invoke 被 ACL 直接拒，
    //    而脚本的 `.catch` 把它吞掉，于是「草稿功能完全不工作」且没有任何线索。
    //    dev 下不出来（dev 壳用的是本地 origin 的宽松路径）。这正是 a5ddedb 已经
    //    吃过一次的亏（`install_node` 漏进白名单，8 个发布版本里 100% 失败）。
    const granted = new Set();
    const capabilityFile = join(root, 'src-tauri', 'capabilities', 'harness-remote.json');
    if (existsSync(capabilityFile)) {
      const capability = JSON.parse(readFileSync(capabilityFile, 'utf8'));
      for (const permission of capability.permissions ?? []) {
        const declared = JSON.parse(readFileSync(join(root, 'src-tauri', 'permissions', 'app-commands.json'), 'utf8'));
        const entry = (declared.permission ?? []).find((item) => item.identifier === permission);
        for (const command of entry?.commands?.allow ?? []) granted.add(command);
      }
    }
    const invokeRe = /invoke\(\s*['"]([a-z_]+)['"]/g;
    const aclMisses = new Set();
    for (const { name, source } of harnessScripts) {
      let invokeMatch;
      invokeRe.lastIndex = 0;
      while ((invokeMatch = invokeRe.exec(source)) !== null) {
        if (!granted.has(invokeMatch[1])) {
          aclMisses.add(
            `${name} 里 invoke 了 ${invokeMatch[1]}，但 harness 窗口没有被授权它——` +
              '真机上会被 ACL 直接拒，而脚本的 .catch 把错误吞掉，症状是「功能完全不工作」' +
              '且没有任何线索。请在 capabilities/harness-remote.json 的 permissions 里加对应的' +
              'allow-* 条目（并在 permissions/app-commands.json 里声明它）。',
          );
        }
      }
    }
    for (const message of aclMisses) fail('harness-invoke-params', message);
  }

  // ⑦之四：技能「活动视图里被同名条目占住」的两处接线。
  //
  // 与本组前三条同形状，且是**实测过**的洞：判据 `skill_conflict::list()` 是纯
  // 函数、测试也覆盖得严（正反两面都钉了），但 2026-09-30 把
  // `skills::status()` 里那一句调用摘掉之后，`cargo test` 630 项全绿、这道门禁
  // 也全绿——而面板上的「移走冲突条目」从此再也不出现，用户重新回到「只有关闭
  // 的对话框」。判据准不准与判据有没有被调用是两件事，这里钉的是后者。
  {
    const misses = [];
    const statusSrc = productionRust(shellSource('manage.rs'));
    // ① 状态视图必须真的调判据，且把结果交给 `conflicts` 字段——字段在而恒为空，
    //    UI 的 `v-if="conflicts.length"` 永远不成立，按钮永远不出现。
    // 判据按 **basename** 匹配，不写死 `crate::skill_conflict::` 这条路径：
    // skills 分目录后它变成 `crate::skills::skill_conflict::`，而这条要查的
    // 契约是「status 有没有真的调 list()」，与调用方写成几级路径无关。
    if (!/let conflicts\s*=\s*[\w:]*skill_conflict::list\(\)/.test(statusSrc)) {
      misses.push('skills::status() 不再调用 skill_conflict::list()');
    }
    if (!/SkillStatus\s*\{[\s\S]*?\n\s*conflicts,/.test(statusSrc)) {
      misses.push('SkillStatus 不再把判据结果交给 conflicts 字段（按钮将永不出现）');
    }
    // ② 告警文案与按钮必须指向同一个动作：`ensure_entry` 的拒绝文案里写着
    //    「点告警下方的『移走冲突条目』」，按钮若改名或删掉，那句话就指向了一个
    //    面板上不存在的东西。命令名由第 1、2 项查（已注册 + 已授权），这里查文案。
    const conflictSrc = productionRust(shellSource('skill_conflict.rs'));
    const panelSrc = readFileSync(join(root, 'ui', 'src', 'skills', 'SkillsPanel.vue'), 'utf8');
    const errorCopy = /pub\(crate\) fn conflict_error[\s\S]*?\n\}/.exec(conflictSrc)?.[0] ?? '';
    if (!errorCopy.includes('移走冲突条目')) {
      misses.push('启用冲突的报错文案不再指向「移走冲突条目」这个按钮');
    }
    if (!panelSrc.includes('>\n          移走冲突条目\n        </el-button>')) {
      misses.push('技能面板上不再有「移走冲突条目」按钮（报错的出路会指向一个不存在的按钮）');
    }
    if (misses.length > 0) {
      fail(
        'skill-conflict-wiring',
        `${misses.join('、')}——技能启用撞上同名条目时，用户拿到的仍然是一个只有` +
          '「关闭」的死胡同。注意：判据本身测得再准，摘掉调用它照样全绿' +
          '（2026-09-30 实测：摘掉后 cargo test 630 项与本门禁都绿）。',
      );
    } else {
      note('技能同名冲突的判据接到了状态视图与面板按钮上（启用失败有出路）');
    }
  }

  // ⑦之六：前端不许把**已搬迁的**数据目录写进用户可见文案。
  //
  // AGENTS.md 那条「数据目录不许在前端写死」是本项目的既有纪律，而它此前的
  // 落地方式是**靠人看**：2026-09-30 走查发现插件页的气泡仍写着
  // `~/.dsh-xlink/dsh-plugins/`，而 `store_relocate` 早已把中央库整体搬进
  // `plugins/dsh/`，用户照着提示去找会找到一个不存在的目录。同一句提示的技能页
  // 版本读的是后端 `SkillStatus.store_root`，那边是对的——修一边漏一边。
  //
  // 钉的是**路径上下文里的已搬迁目录名**，不是「不许出现这两个词」：迁移向导把
  // `skills-store` 当**逻辑来源 id** 用（`SOURCES` 里的值显示成「旧技能中央库」），
  // 那是历史数据的位置、不是当前存储位置。第一版为此开了整文件豁免，实测根本
  // 用不上——判据要求目录名前面带 `~/` 或 `/`，裸 id 天然不命中——留着只是给
  // 将来在迁移页里写死一条真路径留了个静默的口子。
  {
    const LEGACY = ['dsh-plugins', 'skills-store'];
    const offenders = [];
    const uiFiles = [];
    const collectUi = (dir) => {
      for (const entry of readdirSync(dir, { withFileTypes: true })) {
        const full = join(dir, entry.name);
        if (entry.isDirectory()) collectUi(full);
        else if (/\.(vue|js)$/.test(entry.name)) uiFiles.push(full);
      }
    };
    collectUi(join(root, 'ui', 'src'));
    for (const file of uiFiles) {
      const rel = file.split(sep).slice(-3).join('/');
      const lines = readFileSync(file, 'utf8').split('\n');
      lines.forEach((line, i) => {
        const code = line.replace(/\/\/.*$/, '').replace(/\/\*.*?\*\//g, '');
        if (!/['"`]/.test(code)) return;
        for (const name of LEGACY) {
          // 只在「看起来是路径」时才算：存储位置提示里的目录名。
          if (new RegExp(`(~/|/)\\s*${name}\\b|~\\/\\.[\\w.-]*${name}`).test(code)) {
            offenders.push(`${rel}:${i + 1} 文案里出现已搬迁的目录 ${name}`);
          }
        }
      });
    }
    if (offenders.length > 0) {
      fail(
        'no-legacy-paths-in-ui',
        `${offenders.join('；')}——这些目录已被 store_relocate 整体搬走，不再是「存放于」的答案；` +
          '用户照着提示找会找不到。请让该提示读后端返回的真实路径（PluginStatus / SkillStatus 的 ' +
          'store_root），前端只负责 tildePath 折叠。',
      );
    } else {
      note('前端文案里没有已搬迁的数据目录（存储位置提示读后端返回的真实路径）');
    }
  }

  // ⑦之五：「今日用量」在两个面板上必须是**同一份**口径，且概览卡片必须会刷新。
  //
  // 同形状的老毛病，2026-09-30 用户实测撞上：概览卡片显示 0 tokens，而独立的
  // 「模型用量」窗口显示 12.09M，同一份统计两个数。查下来是两处独立成因：
  //   ① 口径分叉——卡片读后端的 `today_tokens`（按本地日历日精确匹配），窗口
  //      自己取 `sliceDays(days, 1)` 的**最后一天**；而 Rust 恰恰会把晚于今天
  //      的异常日期（时钟漂移 / 手改 session）追加到序列末尾，所以「最后一天」
  //      未必是今天。`612ecde` 当时在后端把 `days.last()` 换成精确匹配，窗口
  //      这侧却原样留着同一个坑。
  //   ② 卡片只在 `onMounted` 拉一次就再也不更新，窗口每次打开都 force 重扫还带
  //      手动刷新——卡片那侧永远不会自己追上。
  // 两处都是「纯函数测得再准，摘掉调用照样全绿」：实测把窗口改回
  // `sliceDays(days, 1)`，14 个 usage 单测全绿。这里钉接线形状。
  {
    const misses = [];
    const usageJs = readFileSync(join(root, 'ui', 'src', 'usage', 'usage.js'), 'utf8');
    const window = readFileSync(join(root, 'ui', 'src', 'usage', 'UsageWindow.vue'), 'utf8');
    const overview = readFileSync(join(root, 'ui', 'src', 'shell', 'OverviewPanel.vue'), 'utf8');
    const app = readFileSync(join(root, 'ui', 'src', 'App.vue'), 'utf8');
    // ① 唯一口径：usage.js 导出 todayUsage，窗口与卡片都走它。
    if (!/export function todayUsage\(/.test(usageJs)) {
      misses.push('usage.js 不再导出 todayUsage（今日口径失去唯一出处）');
    }
    // 判「摘要卡里那个 today 变量从哪来」，而不是全文找 sliceDays(…, 1)：
    // 范围切换器（今日 / 7 / 15 天）**本来就该**按所选范围切片，那里出现
    // `sliceDays(days, rangeDays)` 是正确的；而解释旧坑的注释里也写着
    // `sliceDays(days, 1)`。第一版检查就是这么写的，两处都误报。
    if (!/const today = todayUsage\(/.test(window)) {
      misses.push('用量窗口的「今日用量」不再取 todayUsage(data)');
    }
    // ② 卡片会刷新：挂载起定时器、卸载停掉，窗口重新可见时立即对账。
    if (!/setUsageAutoRefresh\(true\)/.test(overview)) {
      misses.push('概览卡片不再起定时刷新（它会永远停在挂载那一刻的快照）');
    }
    if (!/setUsageAutoRefresh\(false\)/.test(overview)) {
      misses.push('概览卡片卸载时不停止定时刷新（离开面板后仍在空转）');
    }
    if (!/loadUsageSummary\(\)/.test(app)) {
      misses.push('窗口重新可见 / 切回概览时不再立即对账（要等满一个刷新周期）');
    }
    if (misses.length > 0) {
      fail(
        'usage-today-agreement',
        `${misses.join('、')}——概览卡片与「模型用量」窗口会为同一份统计显示两个数` +
          '（2026-09-30 用户实测：卡片 0 tokens、窗口 12.09M）。注意：usage 单测抓不到' +
          '这些，实测把窗口改回 sliceDays(days, 1) 之后 14 个测试照样全绿。',
      );
    } else {
      note('「今日用量」两个面板同一口径，且概览卡片会定期与切回时刷新');
    }
  }

  // ⑦ 三处「接线」，单测都抓不到。
  //
  // 共同形状：**判据 / 组件本身是对的，某个人把它接到某处时漏了**。前两轮各吃过一次
  // （`fault_needs_new_window` 算了没人调；草稿脚本漏注入一条建窗路径），所以这里
  // 把三处接线都变成机械检查而不是留在 commit message 里。
  {
    const rustFilesWithScripts = rustFiles.filter((file) =>
      productionRust(readFileSync(join(srcDir, file), 'utf8')).includes('initialization_script('),
    );
    // ① 凡是注入了 harness-health.js 的建窗路径，都必须也注入 harness-draft.js。
    //    用「同一文件 ±6 行内」近似同一条 builder 链——够挡住「新增一条建窗路径时
    //    照抄了健康脚本、忘了草稿脚本」这种漏，而那正是 2026-09-30 commit message
    //    里写着「查不出来」的那一种。
    const draftMisses = [];
    const appearanceMisses = [];
    for (const file of rustFilesWithScripts) {
      const lines = productionRust(readFileSync(join(srcDir, file), 'utf8')).split('\n');
      lines.forEach((line, index) => {
        if (!line.includes('harness/harness-health.js')) return;
        const window = lines.slice(Math.max(0, index - 6), index + 7).join('\n');
        if (!window.includes('harness/harness-draft.js')) {
          draftMisses.push(`${file}:${index + 1} 注入了 harness-health.js 却没注入 harness-draft.js`);
        }
        // 外观镜像脚本同理成对：漏一条链的症状是「那条链建出来的工作台，标题栏
        // 不跟内核主题」——而自愈重建走的正是 `harness_window::build` 那条链。
        if (!window.includes('harness/workbench-appearance.js')) {
          appearanceMisses.push(
            `${file}:${index + 1} 注入了 harness-health.js 却没注入 harness/workbench-appearance.js`,
          );
        }
      });
    }
    for (const message of appearanceMisses) {
      fail(
        'harness-wiring',
        `${message}——那条建窗路径建出来的工作台，原生标题栏不会跟内核的 ui-theme 走。` +
          '两条脚本是成对的，加一条路径要一起加。',
      );
    }
    if (draftMisses.length > 0) {
      for (const message of draftMisses) {
        fail(
          'harness-wiring',
          `${message}——那条建窗路径上没发出去的话不会被保管，重载 / 重建之后输入框会是空的。` +
            '两条脚本是成对的，加一条路径要一起加。',
        );
      }
    }

    // ② 装 / 删内核两端必须各打一次信标、撤一次。
    const kernelBody = (name) => {
      const lines = productionRust(shellSource('lifecycle.rs')).split('\n');
      const start = lines.findIndex((line) => new RegExp(`^pub fn ${name}\\b`).test(line));
      if (start < 0) return null;
      for (let i = start; i < lines.length; i += 1) {
        if (lines[i] === '}') return lines.slice(start, i + 1).join('\n');
      }
      return null;
    };
    for (const action of ['install_version', 'uninstall']) {
      const body = kernelBody(action);
      if (body === null) {
        draftMisses.push(`${kernelFile} 里找不到 ${action}，信标接线无从检查`);
        continue;
      }
      if (!body.includes('package_activity::begin(')) {
        fail(
          'harness-wiring',
          `kernel::${action} 没有打装包信标——对面壳的看门狗不会知道机器正在被打满，` +
            '于是把「慢」判成「死」并重载（2026-09-30 实测的成因）。',
        );
      }
      if (!body.includes('package_activity::end(')) {
        fail(
          'harness-wiring',
          `kernel::${action} 打了信标却不撤——这一次装包会永久放宽对面壳的看门狗` +
            `（直到 10 分钟到期，或直到壳重启）。`,
        );
      }
    }

    if (draftMisses.length === 0) {
      note('工作台建窗路径都注入了草稿脚本，装 / 删内核两端都打了信标');
    }

    // ③ 内核安装必须用 `package-import-method=copy`：与 pnpm store 硬链接共享
    //    inode 的树，会被**任何**一次链接数变化（装 / 删任意版本、用户自己的
    //    pnpm 项目）在 NTFS 上更新 ChangeTime，而内核 `dsh-client-hmr` 每 500ms
    //    的 bundle stat 轮询把这种噪声当成「模块重建」推给**活页面**——
    //    2026-09-30 五次装 / 删全部在 4~6 秒内打死对面正在服务的工作台页面，
    //    与 CPU / 磁盘负载无关（那次安装只跑 9.2s、全部从 store 复用）。
    //    copy 让树持有全新 inode，装 / 删从此物理上碰不到对面的任何文件。
    //    这个参数被删掉时编译不报、测试不红——只有这条机械检查会响。
    {
      const kernelInstallSrc = productionRust(
        shellSource('lifecycle.rs'),
      );
      if (!kernelInstallSrc.includes('--config.package-import-method=copy')) {
        fail(
          'kernel-install-isolated-inodes',
          `${kernelFile} 的 pnpm 安装参数里没有 --config.package-import-method=copy —— ` +
            '内核树会重新与 pnpm store 共享 inode，对面正在服务的工作台页面会再次被' +
            '装 / 删内核打死（2026-09-30 实证机制：链接数变化 → NTFS ChangeTime → ' +
            'client-hmr 的 500ms stat 轮询误判 bundle 重建 → 活页面换模块 → 槽位不变量' +
            '崩溃。见 AGENTS.md「装包任务不许惊动另一个壳的工作台」）。',
        );
      } else {
        note('内核安装用 copy 落盘：树与 pnpm store 不共享 inode');
      }

      // ③之又半：跨壳横幅必须**按实际残余风险门控**（2026-09-30 傍晚）。
      //    机制定案的推论：风险 = 本壳有共享 inode 的旧树 × 对面正在服务的树
      //    也共享；两侧任一独立，任何装 / 删都物理碰不到对方。此前横幅无条件
      //    常驻——用户把版本全部重装隔离之后它还挂在脸上，等于警告在撒谎。
      //    门控的形状：status 的映射必须**以 `.filter(` 接在判据后面**，且判据
      //    里引用 install_isolation 的采样。退回无条件 `.map(` 编译不报、测试
      //    不红，只有这条会响。
      // 同样按 basename 认 `instance::`：分目录后实际调用是
      // `shell::instance::workbench_running_in_other_shell()`，写死少一级
      // 就会让这条判据对着改过的代码报「没有 .filter( 门控」——红的原因与
      // 要检查的东西无关。
      // 2026-10-01 起接受两种形状：旧的内联 `other_shell_workbench:` 字段初始化，
      // 与新的 `let other_shell_workbench = if cross_shell_risk { … }` 短路——后者
      // 是 perf 采样逼出来的加强（无共享版本时连探测都不做，省掉每 2.5s 一轮的
      // 跨壳 pid + 端口探测）。**两种形状都必须保留 `.filter(`**；去掉
      // `if cross_shell_risk` 短路（探测照跑、结果被 filter 丢掉）同样不匹配——
      // 那是白烧 CPU 的回归，也要拦。
      if (
        !/(?:other_shell_workbench:|let other_shell_workbench = if cross_shell_risk \{)\s*[\w:]*instance::workbench_running_in_other_shell\(\)\s*\.filter\(/.test(
          kernelInstallSrc,
        )
      ) {
        fail(
          'cross-shell-notice-single-source',
          `${kernelFile} 的跨壳横幅映射没有先 .filter( 门控——它会在两侧树都隔离后` +
            '仍然常驻，警告比没有更坏（用户已完成隔离却还被吓唬）。映射必须先按' +
            '「本壳有共享 inode 的版本 && 对面正在服务的树也共享」过滤（install_isolation）。',
        );
      } else {
        note('跨壳横幅按实际残余风险门控（两侧任一独立即消失）');
      }

      // ③之半：跨壳横幅文案**单源**。真相源是 instance.rs 的
      //    other_shell_workbench_notice（措辞契约有测试钉住）；状态映射必须从它
      //    取文案，VersionsPanel.vue 只许渲染 `notice`、不许再自己拼。2026-09-30
      //    下午用户截图里的横幅就是这么漂出来的：机制定案后 Rust 文案改对了，
      //    Vue 里那份自己拼的旧文案（「机器最忙…甚至黑屏」）还挂在用户脸上——
      //    与「数据目录不许在前端写死」同一类病：前端那份不参与编译，错了没人报。
      const vueSrc = readFileSync(
        join(root, 'ui', 'src', 'kernel', 'VersionsPanel.vue'),
        'utf8',
      );
      const singleSourceMisses = [];
      if (!kernelInstallSrc.includes('other_shell_workbench_notice(')) {
        singleSourceMisses.push(
          `${kernelFile} 里没有 other_shell_workbench_notice( 调用——横幅文案的真相源断了，` +
            '状态映射退回自己拼字符串就会再漂移出一份旧机制的说法',
        );
      }
      if (!vueSrc.includes('other.notice')) {
        singleSourceMisses.push(
          'VersionsPanel.vue 没有渲染 other.notice——横幅要么没接上后端文案，' +
            '要么又在本地拼了一份（必然漂移）',
        );
      }
      for (const stale of ['仍可继续', '两万个文件并编译原生模块']) {
        if (vueSrc.includes(stale)) {
          singleSourceMisses.push(
            `VersionsPanel.vue 又出现了自拼文案（「${stale}」）——横幅只许渲染 other.notice`,
          );
        }
      }
      if (singleSourceMisses.length > 0) {
        for (const message of singleSourceMisses) {
          fail('cross-shell-notice-single-source', message);
        }
      } else {
        note('跨壳横幅文案单源：Rust 生成，前端只渲染 notice');
      }
    }
  }
}

// --- 14. 出网路由：唯一入口，且「直连」必须真的绕开代理 ----------------------
//
// 2026-09-30 用户实测：「系统里明明开着代理，检查更新却直连 GitHub 然后超时」
// （reqwest 只认环境变量，不认系统设置；壳是 GUI 程序，继承不到命令行里那些
// 变量）。修法是 `net_proxy::routes()` 给出**有序**路由、`updater` 按序试。
// 这里的两条接线**单测抓不到**：路由表是纯函数，测的是形状而不是「真的走了
// 代理」；而 `Route::Direct => no_proxy()` 只影响真实客户端的行为——把它删掉，
// 回退到直连的那一次仍会被 `HTTPS_PROXY` 拉回代理，等于把同一条路试两遍，
// 而全部测试照样全绿。与第 13 项同一类：判据是对的，接线漏了没人知道。
{
  const srcDir = join(root, 'src-tauri', 'src');
  // 递归，理由同上面那处：分目录后非递归扫描会静默漏掉子目录里的文件。
  const rustFiles = walk(srcDir, ['.rs']).map((full) => full.slice(srcDir.length + 1));
  const builders = [];
  for (const file of rustFiles) {
    const lines = productionRust(readFileSync(join(srcDir, file), 'utf8')).split('\n');
    lines.forEach((line, index) => {
      if (line.includes('.updater_builder()')) builders.push({ file, line: index + 1, window: lines.slice(index, index + 12).join('\n') });
    });
  }
  if (builders.length !== 1 || baseName(builders[0].file) !== 'updater.rs') {
    fail(
      'net-route',
      `生产代码里 updater_builder() 出现 ${builders.length} 次` +
        `（${builders.map((hit) => `${hit.file}:${hit.line}`).join('、') || '一处都没有'}）——` +
        '它必须只有一处，且在 updater.rs 的 updater_via 里。新增 GitHub 出网路径请' +
        '复用 net_proxy::routes()，不要另起一个客户端。',
    );
  } else {
    const window = builders[0].window;
    const missing = ['Route::Direct', 'no_proxy(', 'Route::Proxy', '.proxy('].filter(
      (token) => !window.includes(token),
    );
    if (missing.length > 0) {
      fail(
        'net-route',
        `updater.rs:${builders[0].line} 的客户端构造缺 ${missing.join(' / ')}——` +
          '「直连」必须显式 no_proxy()（否则回退仍被 HTTPS_PROXY 拉回代理，等于同一条' +
          '路试两遍），「代理」必须显式 .proxy()（reqwest 默认不读系统设置）。',
      );
    } else {
      note('出网客户端只有一处构造，且直连 / 代理两条都显式接上了');
    }
  }
}

// --- 15. 面板的资源不许出网，且本地路径必须真的存在 ---------------------------
//
// 版本面板的 npm 标志曾经是 `<img src="https://avatars.githubusercontent.com/…">`：
// 一行代码、构建全绿、单测全绿，而它每次渲染都要出网取一次——WebView 出不出网
// 取决于用户那台机器（`tauri.conf.json` 的 `csp` 是 null，没有任何东西拦它），
// 断网或墙内时那 16px 就是一个白方块，`.brand-logo` 的 `background: #fff` 正好
// 把它垫成一块看得见的白砖。这类故障没有任何一条既有门禁会响，所以这里补两条：
// ① 模板 / CSS / 入口 HTML 里不许出现**页面自己去取**的 `http(s)://` 资源；
// ② 以 `/` 开头的 `src` 必须能在 `ui/public` 里找到文件——写错一个字与出网一样，
//    同样是一个静默的空图。
//
// 判据只认「页面自己去取」，**不认用户点了要打开外链**：`<a href="https://…">`
// 是有意为之的用户动作（面板里本来就有好几处，另有走 `openExternalLink` 的按钮），
// 第一版判据把 `href` 一并查了，插件中心的 dshfind.com 与技能页的 GitHub topic
// 两个链接当场误报——凡是把「正当的外链」也拦下来的检查，最后都会被改成拦不住
// 任何东西。`<link href>` 是另一回事：样式表与字体确实由页面去取，所以照拦。
{
  const publicDir = join(root, 'ui', 'public');
  const remote = [];
  const missing = [];
  const files = [...walk(join(root, 'ui/src'), ['.vue', '.js', '.css'])];
  const entry = join(root, 'ui', 'index.html');
  if (existsSync(entry)) files.push(entry);
  for (const file of files) {
    const lines = readFileSync(file, 'utf8').split('\n');
    lines.forEach((line, index) => {
      const where = `${show(file)}:${index + 1}`;
      if (
        /(?:^|[\s:(])src\s*=\s*["'][^"']*https?:\/\//.test(line) ||
        /<link\b[^>]*href\s*=\s*["']https?:\/\//.test(line) ||
        /url\(\s*["']?https?:\/\//.test(line) ||
        /@import\s+(?:url\()?\s*["']?https?:\/\//.test(line)
      ) {
        remote.push(where);
      }
      for (const match of line.matchAll(/(?:^|[\s:(])src\s*=\s*["'](\/[^"'#?]+)["']/g)) {
        // `/src/**` 是 Vite 自己解析的源码路径、`/assets/**` 是构建产物，都不是
        // public 里的静态资源，拿它们去 ui/public 查只会永远报红。
        if (/^\/(?:src|assets|@)\b/.test(match[1])) continue;
        if (!existsSync(join(publicDir, match[1]))) missing.push(`${where} → ${match[1]}`);
      }
      // 面板资源也可能以**裸字符串**的形式引用：`subscription.js` 的
      // `PROVIDER_LOGOS` 就是一张 `provider.id → '/xxx-logo.svg'` 的表，模板再用
      // `:src` 绑上去（`src=` 是动态表达式，上面那条正则一个字都匹配不上）。这类
      // 写法出现在 2026-10-10 之前时没有先例，所以判据没覆盖它，于是「把路径拼错
      // 一个字母」和「出网取图」一样全绿、同样只在用户机器上渲染成空图。放宽到
      // 「引号里 + 以 `/` 开头 + 以图片扩展名结尾」，靠扩展名把路由路径与命令名
      // 挡在外面。
      for (const match of line.matchAll(/['"](\/[^'"]+\.(?:svg|png|jpe?g|webp|gif|ico))['"]/g)) {
        if (/^\/(?:src|assets|@)\b/.test(match[1])) continue;
        if (!existsSync(join(publicDir, match[1]))) missing.push(`${where} → ${match[1]}`);
      }
    });
  }
  const problems = [
    ...remote.map((where) => `${where}（远端资源）`),
    ...missing.map((where) => `${where}（ui/public 里没有这个文件）`),
  ];
  if (problems.length > 0) {
    fail(
      'ui-assets-local',
      `面板里有 ${problems.length} 处资源没有真正落在本地：${problems.join('、')}——` +
        `WebView 出不出网不由我们决定，离线 / 墙内时它就是一个空图，页面上没有任何报错。` +
        `请把资源放进 ui/public/ 并保留来源与许可声明（docs/ui/icon-design.md` +
        `「面板里的第三方标志」）。用户点击打开的外链（<a href>）不在此列。`,
    );
  } else {
    note('面板资源全部来自 ui/public，没有远端地址');
  }
}

// --- 15b. ui/public 里的 SVG 必须是能解析的 XML --------------------------------
//
// 这是一条**已经真实发生过的静默失效**（2026-10-10）：页头缎带
// `ui/public/header-ribbons.svg` 的注释里写了 token 名 `--ds-header-glow`，
// 而 XML 规范把两个连字符当作注释终止符——整个文件因此不是合法 XML，浏览器
// **静默地什么都不画**：构建绿、门禁全绿、`curl` 能取到 200（文件确实在），
// 页面上只是「背景装饰没出现」。
//
// 上面那条 `ui-assets-local` 钉不住它：那条只查文件**存在**，而这个文件存在、
// 也能被 HTTP 取到，它坏在**内容**上。CSS / 单测同样看不见——SVG 不是 JS 模块，
// 没有 import 关系可查。所以判据是「真的拿 XML 解析器解析一遍」，这是唯一能让
// 这类损坏在提交前响起来的办法。
//
// 顺带钉住注释里的 `--`：解析器已经能抓到绝大多数，但给出更直白的报错，
// 省得下次拿到 expat 的 `not well-formed` 再自己反推是哪一行。
const PUBLIC_DIR = resolve(root, 'ui/public');
const svgFiles = existsSync(PUBLIC_DIR)
  ? readdirSync(PUBLIC_DIR).filter((f) => f.endsWith('.svg'))
  : [];
const brokenSvgs = [];
for (const name of svgFiles) {
  const raw = readFileSync(resolve(PUBLIC_DIR, name), 'utf8');
  // 先给一条人话报错：XML 注释里出现连续的连字符就是最常见的原因
  const badComment = [...raw.matchAll(/<!--[\s\S]*?-->/g)].find((m) => m[0].includes('--', 4));
  if (badComment && badComment[0].indexOf('--', 4) < badComment[0].length - 3) {
    brokenSvgs.push(`${name}（注释里有连续的 "--"，XML 会把它当成注释结束——中文破折号也一样）`);
    continue;
  }
  try {
    // 这里**不是**真正的 XML 解析（node 没有 DOMParser，sax 要引依赖），
    // 而是三条针对性判据的合集。它挡不住全部 XML 损坏，只挡上面那类——
    // 注释里多一个连字符、根标签被吃掉、裸 & 未转义。这三种在本仓都真实发生过，
    // 而它们的后果都是同一个：浏览器静默不渲染。
    // 别把它当通用 XML 校验用；遇到别的损坏形态而这里没响，那要补的是判据，不是绕过。
    const opens = (raw.match(/<svg[\s>]/g) || []).length;
    const closes = (raw.match(/<\/svg\s*>/g) || []).length;
    if (opens !== 1 || closes !== 1) {
      brokenSvgs.push(`${name}（svg 根标签不成对：开 ${opens} 个、闭 ${closes} 个）`);
      continue;
    }
    // 未转义的裸 & 是另一个静默杀手
    if (/&(?!amp;|lt;|gt;|quot;|apos;|#\d+;|#x[0-9a-f]+;)/i.test(raw)) {
      brokenSvgs.push(`${name}（有未转义的 & 字符）`);
    }
  } catch (e) {
    brokenSvgs.push(`${name}（解析失败：${e.message}）`);
  }
}
if (brokenSvgs.length) {
  fail(
    'public-svg-parses',
    `ui/public 里有 ${brokenSvgs.length} 个 SVG 不是合法的 XML：${brokenSvgs.join('、')}。` +
      `浏览器遇到不合法的 SVG 会**静默不渲染**：文件在、能取到 200、构建与测试全绿，` +
      `页面上只是「那个图没出来」，没有任何报错。`,
  );
} else if (svgFiles.length) {
  note(`ui/public 的 ${svgFiles.length} 个 SVG 均可被解析（${svgFiles.join('、')}）`);
}

// --- 16. 四个窗口族的右键策略都接在线上，且不许连左键复制一起禁 ----------------
//
// 2026-10-17 起需求分成了两半（此前是一句话「哪扇窗都不许弹右键菜单，保留左键
// 复制」，工作台的实现把内核自绘的菜单也一并拦死了）：
//
//   - 工作台加载的是**内核的** Web UI，它自己有右键功能（`ui-primitives` 的
//     JSON 树「选择复制方式」、`ui-dockkit` 的右侧栏标签页菜单）⇒ 只压引擎
//     原生菜单，放行页面自绘的菜单（`context-menu-fallback.js`，冒泡兜底）；
//   - 官方对话三个内容 webview 加载的是**别人的**站点 ⇒ 维持一律取消
//     （`no-context-menu.js`，捕获阶段连页面自绘的一起拦）；
//   - 内核 `window.open` 弹窗（账号登录授权页 / 终端链接等，label 前缀
//     `workbench-popup-`）同样是**别人的**站点 ⇒ 走 strict 那份；
//   - 壳自己的五个窗口共用 SPA 入口（`ui/src/shell/noContextMenu.js`，
//     `main.js` 装一次）。**行为测试钉不住接线**：测试直接对着脚本跑，把某一处
//     接线删掉它照样全绿，而症状只是「从某个入口打开的窗口右键行为不对」——
//     不报任何错。因此这里钉三条：
//
//   ① **每一条加载外部页面的建窗链**都必须注入它那一族该用的脚本，且不许注入
//      另一族的。按 `WebviewUrl::External(` 所在的顶层函数去找，而不是按文件数
//      出现次数：工作台有**两条**建窗链（`commands.rs::open_harness` 是用户点
//      「启动工作台」走的那条，`harness_window::build` 是自愈链条 / 手动刷新走的
//      那条），漏一条的症状正是「有时右键菜单还在」，而按次数计的判据恰好抓不住
//      这种漏法；两条链用反脚本的症状则相反——内核自己的菜单全没了，同样没有
//      报错。认不出的新窗口族当场报错，不替它默认。
//   ② `main.js` 必须 import 并调用 `disableContextMenu()`，且调用在
//      `app.mount` 之前（晚一步会有一段窗口期菜单还在）。
//   ③ 三份实现都不许出现 `user-select` 或对 `copy` / `select` / `mousedown`
//      的监听——「禁右键」写成「禁选中」是这条需求最常见的写错方式，它不报
//      任何错，坏掉的症状只是用户再也复制不出东西。
{
  const srcDir = join(root, 'src-tauri', 'src');
  // 递归，理由同上面那处：分目录后非递归扫描会静默漏掉子目录里的文件。
  const rustFiles = walk(srcDir, ['.rs']).map((full) => full.slice(srcDir.length + 1));
  const remoteChains = [];
  for (const file of rustFiles) {
    const lines = productionRust(readFileSync(join(srcDir, file), 'utf8')).split('\n');
    lines.forEach((line, index) => {
      if (!line.includes('WebviewUrl::External(')) return;
      // 建窗链的边界取**整个所在顶层函数**：向前退到函数签名、向后收在第 0 列的
      // 收尾大括号。起点不能取 `WebviewUrl::External(` 那一行——窗口身份（官方
      // 对话的 label 是在上一行 `format!` 出来的）常常出现在它之前，按起点切片
      // 会让窗口族判不出来（第一版就是这么错的）。
      let start = index;
      for (let i = index; i >= 0; i -= 1) {
        if (/^(pub )?(async )?fn /.test(lines[i])) {
          start = i;
          break;
        }
      }
      let end = lines.length;
      for (let i = index + 1; i < lines.length; i += 1) {
        if (lines[i] === '}') {
          end = i;
          break;
        }
      }
      remoteChains.push({
        file,
        line: index + 1,
        body: lines.slice(start, end).join('\n'),
      });
    });
  }
  // `include_str!` 的相对路径是**相对所在文件**解析的：commands.rs 在 src/
  // 根，写 `harness/context-menu-fallback.js`；harness_window.rs 与它同在
  // harness/，写 `context-menu-fallback.js`。两条都指同一份文件，所以这里只认
  // basename——写死任一侧的完整路径，另一侧就会以「这条建窗链没注入」变红，
  // 而注入好好的。
  const FALLBACK = 'context-menu-fallback.js';
  const STRICT = 'no-context-menu.js';
  // 窗口族按建窗链里的窗口身份词判，不按脚本名反推：工作台链带 `"harness"`
  // 标签，官方对话链带 `official-chat-tab`。两个都不是的新链一律报错——新加
  // 一扇加载远程页面的窗口时，右键策略必须由人当场定，不能靠默认值蒙。
  const kindOf = (chain) => {
    // 内核 `window.open` 弹窗用 `NewWindowResponse::Create` 建窗（见
    // `harness_window::create_popup_window`），这是它与前两族的结构区别：
    // 它没有固定 label 字面量（label 带递增序号），所以按「建窗后交给 Create」
    // 认族，不按 label 字符串认。
    if (chain.body.includes('NewWindowResponse::Create')) return 'workbench-popup';
    if (chain.body.includes('"harness"')) return 'harness';
    if (chain.body.includes('official-chat')) return 'official-chat';
    return 'unknown';
  };
  const POLICY = {
    harness: { want: FALLBACK, ban: STRICT, why: '工作台加载内核 Web UI，页面自绘的右键菜单必须留着' },
    'official-chat': { want: STRICT, ban: FALLBACK, why: '官方对话加载别人的站点，页面自绘的菜单一并取消' },
    'workbench-popup': {
      want: STRICT,
      ban: FALLBACK,
      why: '内核 window.open 弹窗加载授权站点 / 内核回调页（别人的页面），页面自绘的菜单一并取消',
    },
  };
  const miswired = [];
  const wiredKinds = new Map();
  for (const chain of remoteChains) {
    const kind = kindOf(chain);
    const policy = POLICY[kind];
    if (policy === undefined) {
      miswired.push({ chain, reason: 'unknown' });
      continue;
    }
    if (!chain.body.includes(policy.want) || chain.body.includes(policy.ban)) {
      miswired.push({ chain, reason: 'script', kind, policy });
      continue;
    }
    wiredKinds.set(kind, (wiredKinds.get(kind) ?? 0) + 1);
  }
  if (remoteChains.length === 0) {
    fail(
      'no-context-menu',
      '生产代码里找不到任何 WebviewUrl::External 的建窗链——判据本身失效了，' +
        '先确认这条检查还能匹配到建窗代码，再谈它绿不绿。',
    );
  } else if (miswired.length > 0) {
    for (const { chain, reason, kind, policy } of miswired) {
      if (reason === 'unknown') {
        fail(
          'no-context-menu',
          `${chain.file}:${chain.line} 这条加载外部页面的建窗链认不出属于哪一族——` +
            `右键策略按窗口族分：工作台用 ${FALLBACK}、官方对话与内核弹窗用 ${STRICT}。` +
            `新窗口属于哪一族要先定下来，再按那一族注入（认族依据见脚本里的 kindOf）。`,
        );
        continue;
      }
      fail(
        'no-context-menu',
        `${chain.file}:${chain.line} 这条 ${kind} 建窗链的右键脚本接错了：它必须注入 ` +
          `${policy.want}、且不许出现 ${policy.ban}——${policy.why}。写反的症状是 ` +
          `「内核自己的右键菜单全没了」或「别人的站点也能自绘菜单」，两种都不报错。` +
          `（工作台有 open_harness 与 harness_window::build 两条链，两条都要接；官方对话的 ` +
          `每个内容 webview 走 add_official_chat_tab。本地 SPA 窗口不在此列：它们由 ` +
          `ui/src/shell/noContextMenu.js 负责。）`,
      );
    }
  } else {
    note(
      `远程建窗链的右键脚本按族接好（${[...wiredKinds].map(([kind, n]) => `${kind} ${n} 条 / ${POLICY[kind].want}`).join('，')}）`,
    );
  }

  // ② 壳自己的窗口：入口必须真的调用一次，且早于挂载。
  const mainJs = read('ui/src/main.js');
  const entryProblems = [];
  // 路径随 ui/src 的模块重组变过（noContextMenu.js 在 shell/ 下），判据只认
  // 「入口确实 import 了它」，不把目录结构也钉进来——结构变了这条检查就该失效。
  if (!/import \{ disableContextMenu \} from '[^']*noContextMenu\.js';/.test(mainJs)) {
    entryProblems.push('没有 import disableContextMenu');
  }
  const call = mainJs.indexOf('disableContextMenu();');
  if (call === -1) entryProblems.push('没有调用 disableContextMenu()');
  else if (mainJs.indexOf("app.mount('#app')") !== -1 && call > mainJs.indexOf("app.mount('#app')")) {
    entryProblems.push('调用在 app.mount 之后，会有一段窗口期菜单还在');
  }
  if (entryProblems.length > 0) {
    fail(
      'no-context-menu',
      `ui/src/main.js ${entryProblems.join('；')}——主面板 / 日志 / 用量 / 套餐 / 官方对话页签栏` +
        `共用这一个入口，漏在这里就是五个窗口的右键菜单全都还在，而任何行为测试都不会红。`,
    );
  } else {
    note('壳自己的窗口在入口装一次禁右键策略，且早于组件挂载');
  }

  // ③ 「禁右键」不许顺手变成「禁选中」。
  // 判据前先剥注释，理由与上面 `productionRust` 相同：三份实现里都写着
  // 「刻意**不**写 user-select」，照字面查会把这句话本身当成违规——第一版
  // 就是这么误报的。`//` 前是 `:` 时不当作行注释（`https://…` 这类 URL）。
  const stripJsComments = (text) =>
    text
      .replace(/\/\*[\s\S]*?\*\//g, '')
      .split('\n')
      .map((line) => line.replace(/(^|[^:])\/\/.*$/, '$1'))
      .join('\n');
  // 三份实现各自按自己的规则定位：壳侧两份注入脚本按文件名在 src-tauri/src 下找
  // （它们 2026-10-01 搬进了 harness/），面板侧在 ui/src 下且路径稳定。
  const implementations = [
    { file: shellSourceLabel(FALLBACK), source: shellSource(FALLBACK) },
    { file: shellSourceLabel(STRICT), source: shellSource(STRICT) },
    { file: 'ui/src/shell/noContextMenu.js', source: read('ui/src/shell/noContextMenu.js') },
  ];
  const killsSelection = [];
  for (const { file, source: raw } of implementations) {
    const source = stripJsComments(raw);
    const userSelect = /user-select/i.test(source);
    const guarded = [...source.matchAll(/addEventListener\(\s*['"]([^'"]+)['"]/g)].map(
      (match) => match[1],
    );
    const stolen = guarded.filter((type) => type !== 'contextmenu');
    if (userSelect || stolen.length > 0) {
      killsSelection.push(
        `${file}${userSelect ? ' 写了 user-select' : ''}${stolen.length ? ` 监听了 ${stolen.join(' / ')}` : ''}`,
      );
    }
  }
  if (killsSelection.length > 0) {
    fail(
      'no-context-menu',
      `${killsSelection.join('；')}——这条需求禁的是**右键菜单**，不是选中：左键拖选与 Ctrl/⌘+C 复制必须照旧可用。` +
        `写 user-select 或拦下 copy 不会报任何错，症状只是用户再也复制不出东西。`,
    );
  } else {
    note('三份实现都只取消 contextmenu，没碰选中与复制');
  }
}

// --- 16. 跨模块 import 不得无条件引用只在某个 target_os 下定义的条目 --------
//
// 为什么这条要存在：Windows target 只在发布流水线里被编译一次。`check:rust`
// 只编宿主 target（macOS），CI 的 Quality gates 跑在 ubuntu 上也不是 Windows——
// 所以只影响 Windows 的编译错误，必然要烧掉一次**完整发布**（两平台构建 +
// 20~30 分钟）才暴露。2026-10-01 的 rc.2 因此连炸两次，第二次就是这个：
// 19b2d8a 把常量从 commands.rs 搬进 harness/official_chat.rs 时带着
// `#[cfg(target_os = "macos")]`，但新加的 `use` 没带，Windows 侧 E0432。
//
// 判据：收齐「只在 target_os 门控下定义、且没有通用定义」的条目名，再找
// 自身不带 target_os 门控的 `use crate::…`。跨文件取并集是**故意保守**的：
// 只要任何文件里存在该名字的无门控定义就不报——宁可漏报，不可误报。

/// 一条 `#[cfg]` 是不是「限定到某个 os」的正向门控。
/// `not(target_os = …)` 是反向的，说明它在**其它**平台上有定义，按通用处理。
const restrictsToOneOs = (attr) =>
  attr.startsWith('#[cfg') && attr.includes('target_os') && !/not\s*\(\s*target_os/.test(attr);

/// `src-tauri/src` 下的全部 Rust 源文件，仓库相对、正斜杠分隔。
const RUST_SOURCES = walk(join(root, 'src-tauri/src'), ['.rs']).map((full) =>
  relative(root, full).split(sep).join('/'),
);

const osRestrictedItems = () => {
  const items = new Map();
  for (const rel of RUST_SOURCES) {
    const lines = read(rel).split('\n');
    for (let i = 0; i < lines.length; i += 1) {
      const decl = lines[i].match(
        /^\s*(?:pub(?:\(crate\))?\s+)?(?:const|static|fn|struct|enum|type)\s+([A-Za-z_]\w*)/,
      );
      if (!decl) continue;
      // 往上收集紧邻的属性行（`#[…]` 连续若干行）。
      let attr = '';
      for (let j = i - 1; j >= 0 && lines[j].trim().startsWith('#['); j -= 1) {
        attr = `${lines[j].trim()} ${attr}`;
      }
      const entry = items.get(decl[1]) ?? { oses: new Set(), open: false };
      if (restrictsToOneOs(attr)) {
        const os = attr.match(/target_os\s*=\s*"(\w+)"/);
        if (os) entry.oses.add(os[1]);
      } else {
        entry.open = true;
      }
      items.set(decl[1], entry);
    }
  }
  // `open` = 存在不带正向 os 门控的定义 → 该名字不是 os 专属。
  return new Map([...items].filter(([, v]) => !v.open && v.oses.size > 0));
};

const osRestricted = osRestrictedItems();
const ungatedOsImports = [];
for (const rel of RUST_SOURCES) {
  const lines = read(rel).split('\n');
  for (let i = 0; i < lines.length; i += 1) {
    if (!/^\s*use\s+crate/.test(lines[i])) continue;
    // use 可能是多行花括号列表，收到闭合为止。
    let statement = lines[i];
    let end = i;
    while (statement.includes('{') && !statement.includes('}') && end + 1 < lines.length) {
      end += 1;
      statement += ` ${lines[end].trim()}`;
    }
    let attr = '';
    for (let j = i - 1; j >= 0 && lines[j].trim().startsWith('#['); j -= 1) {
      attr = `${lines[j].trim()} ${attr}`;
    }
    if (attr.includes('target_os')) {
      i = end;
      continue;
    }
    for (const name of osRestricted.keys()) {
      if (new RegExp(`\\b${name}\\b`).test(statement)) {
        ungatedOsImports.push(
          `${rel}:${i + 1} 无条件 use ${name}（它只在 ${[...osRestricted.get(name).oses].join('/')} 下定义）`,
        );
      }
    }
    i = end;
  }
}
if (ungatedOsImports.length > 0) {
  const advice =
    '给 import 加上与定义相同的 target_os 门控，不要给常量编一个跨平台的假定义：' +
    '本机编不了非宿主 target（交叉 C 工具链 / 交叉 sysroot 都不具备），' +
    '这类错误只有 Windows 流水线会报。';
  fail(
    'os-gated-import',
    `${ungatedOsImports.join('；')}——这些条目在别的平台上不存在，无条件 import 会让那个平台的编译期直接报 E0432。${advice}`,
  );
} else {
  note(
    `${osRestricted.size} 个 target_os 专属条目的跨模块 import 都带门控（Windows 侧不会 E0432）`,
  );
}

// --- 17. 常驻接线：自启收起走 hide_to_shell，Reopen 兜底 show_main_shell -----
//
// 2026-10-05 用户重启实测：登录自启拉起后 Dock 上挂着带「在运行」小点的图标，
// 点它没反应，主界面出不来。两半成因，都是"判据是对的、接线漏了没人知道"：
// ① 自启分支裸调 `window.hide()` 而没走 `resident::hide_to_shell`，macOS 的
//    激活等级没降到 Accessory——进程以 Regular 等级、零可见窗口留在 Dock 里
//    （真机 `lsappinfo` 实测 `type="Foreground"`）；
// ② `RunEvent::Reopen` 只抬工作台，注释却假设"没开工作台时系统已把管理面板
//    带回前台"——错的：macOS 的激活不会替我们显示 `hide()` 掉（orderOut）的
//    窗口。两半各自单看都能被"菜单栏图标才是入口"的设计说辞盖过去，合在一起
//    就是用户唯一看得见的痕迹（Dock 图标）成了一个死按钮。
// 单测抓不到这两条：它们都是 macOS 窗口管理器的运行时行为，测试进程里没有
// NSApp 可断言，只能钉接线形状。
{
  const libLines = productionRust(read('src-tauri/src/lib.rs')).split('\n');

  // ① 收起动作全仓只有一份实现：lib.rs 不许出现裸 `.hide(`。
  const rawHides = [];
  libLines.forEach((line, index) => {
    if (/\.hide\(/.test(line)) rawHides.push(`lib.rs:${index + 1} ${line.trim()}`);
  });
  if (rawHides.length > 0) {
    fail(
      'resident-wiring',
      `${rawHides.join('；')}——窗口收起必须走 shell::resident::hide_to_shell（它会顺带在 ` +
        'macOS 上把激活等级降到 Accessory、Windows 上补 skip-taskbar），裸 hide 会把进程 ' +
        '以「Dock 有图标但没有任何窗口」的状态留在后台。',
    );
  }

  // ② 自启分支必须真的调到 hide_to_shell。
  const autostartIdx = libLines.findIndex((line) => /started_by_autostart\(\)\s*\{/.test(line));
  if (autostartIdx < 0) {
    fail('resident-wiring', 'lib.rs 里找不到 `started_by_autostart()` 分支——自启收起逻辑被挪走了？');
  } else {
    let end = autostartIdx + 1;
    while (
      end < libLines.length &&
      end - autostartIdx < 60 &&
      libLines[end].trim() !== '}'
    ) {
      end += 1;
    }
    const body = libLines.slice(autostartIdx, end).join('\n');
    if (!body.includes('hide_to_shell')) {
      fail(
        'resident-wiring',
        `lib.rs:${autostartIdx + 1} 的自启分支没有调 shell::resident::hide_to_shell——` +
          '登录拉起的面板只被裸 hide，macOS 上会留下点不动的 Dock 图标（2026-10-05 事故）。',
      );
    }
  }

  // ③ Reopen 抬不到工作台时必须兜底 show_main_shell。
  const reopenIdx = libLines.findIndex((line) => line.includes('RunEvent::Reopen'));
  if (reopenIdx < 0) {
    fail('resident-wiring', 'lib.rs 里找不到 RunEvent::Reopen 分支——macOS 的 Dock 激活没有接线？');
  } else {
    const body = libLines.slice(reopenIdx, reopenIdx + 8).join('\n');
    const missing = ['raise_workbench_if_open', 'show_main_shell'].filter(
      (token) => !body.includes(token),
    );
    if (missing.length > 0) {
      fail(
        'resident-wiring',
        `lib.rs:${reopenIdx + 1} 的 Reopen 分支缺 ${missing.join(' / ')}——` +
          'macOS 的激活不会替我们显示 hide() 掉的窗口：抬不到工作台时必须自己把管理面板叫回来，' +
          '否则点 Dock 图标什么都不会发生。',
      );
    }
  }

  // ④ Windows 上任务栏按钮的删与补必须成对，且住在同一个函数里。
  //
  // 2026-10-09 用户实测：登录自启把面板收进后台后，点任务栏启动项唤回它，
  // 窗口出来了——**任务栏上却没有它的按钮**。`hide_to_shell` 的
  // `set_skip_taskbar(true)` 走 ITaskbarList::DeleteTab 把按钮直接删掉，
  // `show()` 不会让它自己回来。补回的那份实现曾经单独放在
  // `tray::show_main_shell` 里，而**没有任何调用方走它**（`lib::show_main_shell`
  // 直接调 `resident::show_main_shell`），于是「恢复时补回」一直是句空话，
  // 单测与编译都不响。钉成对：两个方向必须在同一个文件里成对出现。
  const residentSource = productionRust(read('src-tauri/src/shell/resident.rs'));
  const skipTaskbarTrue = /set_skip_taskbar\(true\)/.test(residentSource);
  const skipTaskbarFalse = /set_skip_taskbar\(false\)/.test(residentSource);
  if (!skipTaskbarTrue || !skipTaskbarFalse) {
    fail(
      'resident-wiring',
      'shell/resident.rs 里 set_skip_taskbar 的删（true）与补（false）必须成对：' +
        `收起时 true=${skipTaskbarTrue}、恢复时 false=${skipTaskbarFalse}——` +
        'DeleteTab 之后按钮不会自己回来，缺哪一半都会留下「窗口回来了但任务栏上找不到它」。',
    );
  }

  if (rawHides.length === 0 && autostartIdx >= 0 && reopenIdx >= 0 && skipTaskbarTrue && skipTaskbarFalse) {
    note(
      '常驻接线完整：自启收起走 hide_to_shell，Reopen 兜底 show_main_shell，' +
        '任务栏按钮删补成对',
    );
  }
}

// --- 18. 窗口标题与标题栏主题：一份名字 + 壳自有窗口钉深色、工作台不钉 --------
//
// 2026-10-07 用户截图：主窗口标题栏深色，副窗（套餐用量）标题栏**浅色**——
// 系统切浅色时原生标题栏跟着变白，而壳内内容恒为深色。两处成因各自独立：
//   · 全仓 `prefers-color-scheme` 零命中，`set_theme` 从来没被调用过，
//     所以每一扇走原生装饰的窗口都在跟随系统；
//   · 窗口标题是六处各自写死的字面量，主窗口那份还硬编码在 Vue 组件里。
// 两条都只有真机窗口标题栏看得出来，单测与 UI 测试都够不着，所以放这里。
//
// 2026-10-18 用户复报的是**反方向**：工作台（内核 Web UI）里选「跟随系统」，
// 系统浅色却仍渲染深色。那次修复把每扇窗都钉了深色，工作台也在内——而内核的
// `system` 一档是它在页内用 `prefers-color-scheme` 解析的，壳钉住窗口就等于把
// 内核的「跟随系统」关掉。现在这条的分界是**页面的主人是谁**：壳自有页面
// （面板 + 三扇副窗）建窗钉深色、页面挂载后自己纠正；工作台一律不钉。

{
  const conf = JSON.parse(read('src-tauri/tauri.conf.json'));
  const main = (conf.app?.windows ?? []).find((w) => w.label === 'main');
  if (!main) {
    fail('window-chrome', 'tauri.conf.json 里找不到 label 为 main 的窗口');
  } else {
    const expected = main.title;
    if (!expected) {
      fail('window-chrome', '主窗口没有 title，副窗的「窗口名 — 应用名」就没有可对齐的那个名字');
    } else {
      // ① tauri.conf.json ↔ Rust 常量 ↔ Vue 自绘标题，三处必须同一份。
      const rustTitle = read('src-tauri/src/shell/window.rs').match(
        /pub const APP_TITLE: &str = "([^"]+)"/,
      );
      if (!rustTitle) {
        fail('window-chrome', 'shell/window.rs 里找不到 APP_TITLE——窗口标题的唯一出口没了');
      } else if (rustTitle[1] !== expected) {
        fail(
          'window-chrome',
          `窗口标题对不上：tauri.conf.json 是「${expected}」，shell/window.rs 的 APP_TITLE 是「${rustTitle[1]}」`,
        );
      }
      // ③ 自绘标题。**这一条 2026-10-08 从「按字面量比对」改成「求值」**。
      //
      // 原实现用 `/<span>([^<]+)<\/span>/` 抓模板里的标题节点，逐字对比
      // `tauri.conf.json` 的 title。三扇副窗接上自绘标题栏后标题变成
      // `{{ caption }}`（各带功能名），那条正则抓到的就是插值本身——判据红了，
      // 但**它红得对**：模板里那行确实不再是可逐字比对的东西了。
      //
      // 这正是「判据名与判据说的不是一回事」那一类：它叫「窗口标题对不上」，
      // 真正要守的是**主壳那一扇**的标题仍是同一个名字。副窗显示功能标题
      // （用户 2026-10-08 要求）是**另一件事**，不该由这条判据管。
      //
      // 中间那版改成了「查 `props.title || '应用名'` 里的回落值」——那仍然只
      // 钉住了**没传 title** 时的一半：分隔符从 `@` 换成别的、或者副窗那一支
      // 拼错了，这版判据全绿。所以现在**真的把那段表达式求值**：主壳（不传
      // title）必须等于 APP_TITLE，副窗（传功能名）必须是 `功能名@APP_TITLE`。
      // 改分隔符、改成只显示功能名、改成回落应用名——三样都会转红。
      const vueSrc = read('ui/src/shell/WindowTitleBar.vue');
      const expr = vueSrc.match(/const caption = ([\s\S]*?);\n/);
      if (!expr) {
        fail('window-chrome', 'WindowTitleBar.vue 里找不到 `const caption = …`——主壳的标题从哪来已经无从判断');
      } else {
        let captionOf;
        try {
          // props 只用到 title；用 `new Function` 求值而不是解析 AST，是为了
          // 顺带覆盖模板字符串、嵌套表达式这些写法（副窗那支就是模板字符串）。
          captionOf = new Function('props', `return ${expr[1]};`);
        } catch {
          captionOf = null;
        }
        if (!captionOf) {
          fail('window-chrome', `WindowTitleBar.vue 的 caption 表达式求值失败：${expr[1].slice(0, 80)}`);
        } else {
          const mainCaption = captionOf({ title: '' });
          if (mainCaption !== expected) {
            fail(
              'window-chrome',
              `窗口标题对不上：tauri.conf.json 是「${expected}」，WindowTitleBar.vue 不传 title 时显示的是「${mainCaption}」`,
            );
          }
          // 副窗那一支也要钉：格式是 `功能名@应用名`（用户 2026-10-08 定），
          // 且应用名部分仍要与 APP_TITLE 一致。
          const viewerCaption = captionOf({ title: '日志' });
          if (viewerCaption !== `日志@${expected}`) {
            fail(
              'window-chrome',
              `副窗标题格式不对：应是「功能名@${expected}」，实得「${viewerCaption}」`,
            );
          }
          if (!/<span>\{\{ caption }}<\/span>/.test(vueSrc)) {
            fail(
              'window-chrome',
              'WindowTitleBar.vue 渲染的不是 caption——算了 caption 却没渲染它，等于标题仍走模板里的字面量',
            );
          }
          // ④ 三扇副窗都得传**非空**标题。这一条是反向验逼出来的：把日志窗的
          //    `:title="activeName || '日志'"` 改成 `:title="activeName"`，
          //    UI 测试会红（它逐扇查了绑定），但本不变量此前全绿——而现象是
          //    「还没选中任何日志文件时，副窗标题退成 `@Dsh-Xlink`」，功能名那
          //    半截没了。两道门各管一层，这里补的是「传的值可能为空」那一层。
          for (const [rel, label] of [
            ['ui/src/logs/LogViewerWindow.vue', '日志'],
            ['ui/src/usage/UsageWindow.vue', '模型用量'],
            ['ui/src/subscription/SubscriptionWindow.vue', '套餐用量'],
          ]) {
            // 这一段踩了两次正则的坑，两次都是「判据全绿但它其实什么都没查」：
            //   1. `\btitle="` —— `:title` 里 `:` 与 `t` 都是词字符，`\b` 不成立，
            //      正则于是跳到**后面那个** `title=`（`shell-class` 之后），
            //      把 `:title="activeName"` 读成 `title="activeName"`。
            //   2. 改成 `\s:?title="` 后 `:` 又被 `\s` 吃掉，`startsWith(':')`
            //      恒为 false，整个兜底检查成死代码。
            // 现在分开做：先抓**整个开标签**，再用 `\s:title="`（冒号不吞）
            // 单独判断是不是绑定，最后才取属性值。
            const src = read(rel);
            const tag = src.match(/<ViewerShell\b[^>]*>/);
            const bound = tag && tag[0].match(/\s:title="([^"]*)"/);
            if (!tag) {
              fail('window-chrome', `${rel} 里找不到 <ViewerShell> 开标签`);
            } else if (!bound) {
              const literal = tag[0].match(/\stitle="([^"]*)"/);
              if (!literal) {
                fail('window-chrome', `${rel} 没给 ViewerShell 传标题`);
              } else if (!literal[1].trim()) {
                // 字面量只要不为空即可，只有绑定才需要兜底。
                fail('window-chrome', `${rel} 给 ViewerShell 传了空标题`);
              }
            } else if (!/\|\|\s*'[^']+'/.test(bound[1])) {
              fail(
                'window-chrome',
                `${rel} 的标题是绑定但没有兜底：${bound[1]} 为空时副窗会显示成「@应用名」，功能名那半截没了`,
              );
            }
          }
        }
      }
    }
    // macOS 的声明式 Dark 会覆盖 NSApplication，禁用它；页加载时按窗口设置。
    if (main.theme != null) {
      fail('window-chrome', '主窗口不得声明应用级主题：macOS 会失去系统外观变化');
    }
    if (!libSource.includes('shell::appearance::initialize(webview.window())')) {
      fail('window-chrome', '缺少窗口级外观初始化');
    }
  }

  // ② 外观策略只有 `shell/appearance.rs` 一个出口，而且必须**按窗口**分流。
  //
  // 2026-10-18 用户实测：系统浅色 + 内核「设置 → 通用设置 → 外观 = 跟随系统」，
  // 工作台仍渲染深色。根因不在内核——内核的 `system` 一档是它在**页内**用
  // `matchMedia('(prefers-color-scheme: dark)')` 解析的（`dsh-client-ui-theme` 的
  // README 与 boot 脚本），而壳把这扇窗的原生外观钉成了 dark：macOS 走 app 级
  // `on_page_load` 里的逐窗口 NSAppearance，Windows 走建窗 `.theme(Some(Dark))`
  // → tauri-runtime-wry → wry 的 WebView2 `SetPreferredColorScheme`。那个查询
  // 于是恒为 dark。**替别人的页面决定明暗，就是把它的「跟随系统」关掉。**
  //
  // 行为由 `appearance.rs` 的单测钉（工作台恒 `None`、壳自有窗口照旧深色起步）；
  // 这里钉接线形状——纯函数测得再准，把调用摘掉照样全绿。
  const appearanceSrc = productionRust(read('src-tauri/src/shell/appearance.rs'));
  if (!/pub fn owns_appearance\(/.test(appearanceSrc)) {
    fail('window-chrome', 'appearance.rs 里没有 owns_appearance——「这扇窗的外观谁说了算」无从判断');
  }
  if (!/owns_appearance\(window\.label\(\)\)/.test(appearanceSrc)) {
    fail(
      'window-chrome',
      '外观初始化没有按窗口 label 门控：app 级 on_page_load 会把工作台也钉成深色，内核「跟随系统」失效',
    );
  }
  if (!/pub fn initial_theme\(\s*label: &str\s*\)/.test(appearanceSrc)) {
    fail('window-chrome', 'initial_theme 不再接收窗口 label——调用方无法表达「这扇窗属于谁」');
  }
  // 工作台与内核 `window.open` 弹窗都加载别人的页面：两者都必须被
  // `owns_appearance` 豁免，否则窗口一建就钉住 `prefers-color-scheme`。
  if (!/label != WORKBENCH_LABEL && !label\.starts_with\(POPUP_LABEL_PREFIX\)/.test(appearanceSrc)) {
    fail(
      'window-chrome',
      'owns_appearance 没有同时豁免工作台与内核弹窗（POPUP_LABEL_PREFIX）——弹窗会被钉成深色，授权页的明暗不再跟随系统',
    );
  }

  // ③ 每扇带标题的窗都要声明建窗主题；**工作台是唯一例外，而且必须是例外**。
  //    壳自有窗口：主题真值在 localStorage，Rust 读不到，所以建窗统一从 Dark
  //    起步，页面挂载前 `applyTheme()` 按实际主题纠正本窗（见 ui/src/shell/bridge.js
  //    的 setWindowTheme）。少钉的那扇没人纠正，浅色内容就顶着深色原生标题栏
  //    （2026-10-07 用户截图）。工作台反过来：钉住它，内核就跟随不了系统（②）。
  //
  //    例外表达在**调用参数**上：两条工作台建窗链传 `WORKBENCH_LABEL`，
  //    `initial_theme` 对它两个平台都返回 `None`。工作台随后由注入脚本
  //    `workbench-appearance.js` 按内核声明的偏好镜像（`system` 不覆盖）。所以
  //    全局对账照旧（每扇窗一次 `.theme`），另外单点三件：工作台那一支恰好两处、
  //    不许写成字符串字面量、`.theme(` 不许绕过策略出口。
  //
  //    两件事分别计数而不是逐扇配对：配对要看「`.title(` 往后 30 行里有没有
  //    `.theme(`」，那是个靠行距的启发式，往 builder 中间插几行参数就会静默失配。
  const popupFiles = [];
  let titles = 0;
  let hardcoded = 0;
  let themes = 0;
  let workbenchThemes = 0;
  const bypassedThemes = [];
  const rawThemeLiterals = [];
  const literalWorkbenchLabels = [];
  for (const rel of RUST_SOURCES) {
    const lines = read(rel).split('\n');
    for (let i = 0; i < lines.length; i += 1) {
      // 只认 `builder.title(`，不认 `notify::task` 里的 `set_title(`。
      if (!/\.title\(/.test(lines[i])) continue;
      titles += 1;
      popupFiles.push(`${rel}:${i + 1}`);
      if (!/window_title\(/.test(lines[i])) hardcoded += 1;
    }
    const source = productionRust(lines.join('\n'));
    themes += (source.match(/\.theme\(\s*(?:crate::shell::appearance::)?initial_theme\(/g) ?? []).length;
    workbenchThemes += (
      source.match(/initial_theme\(\s*crate::shell::appearance::WORKBENCH_LABEL\s*,?\s*\)/g) ?? []
    ).length;
    const bypassed = source.match(/\.theme\((?!\s*(?:crate::shell::appearance::)?initial_theme\()/g) ?? [];
    if (bypassed.length) bypassedThemes.push(`${rel}×${bypassed.length}`);
    // `Theme::Dark` / `Theme::Light` 字面量只许出现在策略出口里：绕过它的写法
    // （`.theme(Some(Theme::Dark))`）正是这次故障的形状。
    if (baseName(rel) !== 'appearance.rs') {
      const literals = source.match(/Theme::(?:Dark|Light)/g) ?? [];
      if (literals.length) rawThemeLiterals.push(`${rel}×${literals.length}`);
    }
    // 工作台 label 只许经常量传：写成字面量时，改 label 或写错一处都不会报错，
    // 而症状是「工作台又被钉住了」或「壳自有窗口不再钉」——两头都静默。
    const labelLiterals = source.match(/initial_theme\(\s*"harness"\s*\)/g) ?? [];
    if (labelLiterals.length) literalWorkbenchLabels.push(`${rel}×${labelLiterals.length}`);
  }
  if (bypassedThemes.length) {
    fail(
      'window-chrome',
      `有建窗主题绕过了 appearance::initial_theme：${bypassedThemes.join('、')}——钉住别人的页面就是关掉它的「跟随系统」`,
    );
  }
  if (rawThemeLiterals.length) {
    fail(
      'window-chrome',
      `生产代码里出现 Theme::Dark / Theme::Light 字面量（${rawThemeLiterals.join('、')}）：主题只许从 appearance::initial_theme 出`,
    );
  }
  if (literalWorkbenchLabels.length) {
    fail(
      'window-chrome',
      `工作台 label 被写成了字面量（${literalWorkbenchLabels.join('、')}）：请用 appearance::WORKBENCH_LABEL`,
    );
  }
  if (workbenchThemes !== 2) {
    fail(
      'window-chrome',
      `工作台建窗链应有 2 处传 WORKBENCH_LABEL（harness_window::build 与 commands::open_harness），实得 ${workbenchThemes} 处——少的那条会把工作台钉住，内核「跟随系统」失效`,
    );
  }
  if (titles === 0) {
    fail('window-chrome', '一个 `.title(` 都没扫到——建窗路径被整体挪走了？检查项本身该跟着更新');
  } else {
    if (hardcoded > 0) {
      fail(
        'window-chrome',
        `有 ${hardcoded} 处窗口标题是写死的字面量，没走 window_title()（共 ${titles} 处标题）`,
      );
    }
    if (themes !== titles) {
      fail(
        'window-chrome',
        `建了 ${titles} 扇带标题的窗，只声明了 ${themes} 次建窗主题——少声明的那扇没人按应用主题纠正，浅色内容会顶着系统默认的原生标题栏（2026-10-07 用户截图）`,
      );
    }
  }
  if (hardcoded === 0 && themes === titles) {
    note(
      `窗口标题统一走 window_title()，${titles} 扇带标题的窗都声明了建窗主题（壳自有窗口由 applyTheme → window.setTheme 纠正；工作台与内核弹窗都不钉，明暗由页面 / 系统说了算）`,
    );
  }
}

// --- 19. `view.settings` 读的是 `Settings` 的 snake_case 字段名 -------------
//
// 第 9 项管的是反方向：套了 `rename_all = "camelCase"` 的结构体，前端读到
// snake_case 就是错的。这一项管它的镜像。`Settings` 只挂了 `#[serde(default)]`、
// **没有** rename_all，而它还要按原样读写磁盘上的 settings.json，所以对外就是
// snake_case。
//
// 2026-10-07 用户实测：插件页的「预检」开关点了没反应——toast 说「已关闭安装预检」，
// 开关却还是「预检」态。前端读的是 `store.view.settings.pluginPrecheck`，实际键是
// `plugin_precheck`，于是取到 undefined；而判据里写着「undefined 按 true 解释」，
// 把它变成**恒真**。编译、单测、代码预算三道门禁全绿——前端测试喂的是手写夹具，
// 真实响应从没穿过它。
//
// 为什么只盯 `settings.`：前端会整体读入并按字段名逐个访问的载荷里，只有它有一条
// 固定路径（`store.view.settings`）。按结构体泛化会误报——同一个 camelCase 名字在
// 别的载荷里可能真的是对的（第 9 项的注释就记过一次这样的误判）。

{
  const body = read('src-tauri/src/shell/settings.rs').match(/pub struct Settings \{([\s\S]*?)\n\}/);
  if (!body) {
    fail('settings-fields', 'settings.rs 里找不到 `pub struct Settings`，本项判据失效');
  } else {
    const fields = new Set([...body[1].matchAll(/pub\s+([a-z][a-z0-9_]*)\s*:/g)].map((m) => m[1]));
    if (fields.size === 0) {
      fail('settings-fields', 'Settings 结构体里一个字段都没解析到，判据本身该更新');
    } else {
      const bad = [];
      const uiDir = join(root, 'ui', 'src');
      const scan = (dir) => {
        for (const entry of readdirSync(dir, { withFileTypes: true })) {
          const full = join(dir, entry.name);
          if (entry.isDirectory()) {
            scan(full);
            continue;
          }
          if (!/\.(js|vue)$/.test(entry.name)) continue;
          const rel = relative(uiDir, full);
          const text = readFileSync(full, 'utf8');
          // 前置只挡「紧贴前一个词」（`kernel_settings` / `${...}settings`），
          // **不能**挡 `.`：真实路径长这样 `store.view.settings.port`，前面那个
          // 链式访问的点是合法的一部分。第一版写成 `(?<![\w.])`，结果本项对
          // PluginsPanel 那条真错读完全无声——反向验才看见。
          for (const access of text.matchAll(/(?<![\w$])settings\.([A-Za-z_]\w*)/g)) {
            const name = access[1];
            // `settings.json` 是磁盘文件名，不是字段访问。
            if (name === 'json' || fields.has(name)) continue;
            const line = text.slice(0, access.index).split('\n').length;
            bad.push(`${rel}:${line} 读 settings.${name}，但 Settings 没有这个字段（无 rename_all，键是 snake_case）`);
          }
        }
      };
      scan(uiDir);
      if (bad.length > 0) {
        fail('settings-fields', `${bad.join('；')}——取到 undefined，读写双向都不成立`);
      } else {
        note(`view.settings 的 ${fields.size} 个字段与前端读取一一对应`);
      }
    }
  }
}

// --- 20. 官网网页版的站表与页签表一一对应 -------------------------------
//
// `titlebar-pulse.js` 靠一张「站 → hostname + 品牌色」的表决定顶条走哪个站
// 的颜色、还是工作台绿。它和 Rust 的 `OFFICIAL_CHAT_TABS` 是同一件事的两份
// 名单，而：
//   · 两边分处 Rust / JS，不共享任何符号；
//   · `titlebar-pulse.test.mjs` 只跑 `127.0.0.1`（非官网那条路），官网分支
//     在测试里根本没被执行到；
//   · 少一个 host 的症状不是报错，是**新页签顶条悄悄变成工作台的绿**。
//
// 2026-10-07 用户要求移除千问页签时，这三份名单（Rust 页签表、前端兜底页签表、
// 这份 host 名单）都必须同步——少改任何一份都留不一致。
//
// 2026-10-18 引入**条件页签**（本机 Codex 登录可用时追加的 ChatGPT）后，这份
// 比对扩成四份事实：两张 Rust 页签表（`OFFICIAL_CHAT_TABS` 恒定 +
// `OFFICIAL_CHAT_OPTIONAL_TABS` 按登录态追加）、`titlebar-pulse.js` 的 host
// 名单、前端兜底表。分寸是刻意分开的：host 名单比对**两张表的并集**（条件页签
// 出现时顶条也该是官网色），而前端兜底表**只要求固定页签**——兜底存在的场景
// 正是问不到后端，此时无从判断登录态，硬画一个条件页签等于绕过判据。
//
// 2026-10-24 起口径从「host 一一对应」改成**站级**，并从「官方一个蓝」改成
// **三站三色**（用户实机报 MiniMax 顶条是绿的、ChatGPT 顶条该是黑的）。
// 改成站级的原因是实测事实：MiniMax 的入口 `agent.minimaxi.com` 会 302 到
// 落地域 `agent.minimax.cn`，内容 webview 最终的 `location.hostname` 是后者
// ——落地域必须在站表里，却**不在** Rust 页签表里（页签表给的是入口地址），
// 仍按 host 一一对应就会被判成「死条目」，而删掉它就正好复现用户报的那条
// 绿线。

{
  const tabsSrc = read('src-tauri/src/harness/official_chat.rs');
  // 两张表都要找：**条件页签表找不到时判据必须红**，而不是当成空名单放过
  // ——那正是「判据空转」最典型的形态（锚点没匹配上却一路绿到底）。
  const tabBlock = tabsSrc.match(/OFFICIAL_CHAT_TABS: &\[\(&str, &str\)\] = &\[([\s\S]*?)\];/);
  const optionalBlock = tabsSrc.match(
    /OFFICIAL_CHAT_OPTIONAL_TABS: &\[\(&str, &str\)\] = &\[([\s\S]*?)\];/
  );
  if (!tabBlock) {
    fail('official-chat-tabs', 'official_chat.rs 里找不到 OFFICIAL_CHAT_TABS，判据失效');
  } else if (!optionalBlock) {
    fail(
      'official-chat-tabs',
      'official_chat.rs 里找不到 OFFICIAL_CHAT_OPTIONAL_TABS（条件页签表），判据失效'
    );
  } else {
    const tabBody = tabBlock[1];
    const optionalBody = optionalBlock[1];
    // 名单里写的是常量名（OFFICIAL_CHAT_URL / OFFICIAL_CHAT_MINIMAX_URL），
    // 真正的地址定义在文件上方，按 `pub const X: &str = "url"` 展开。
    const urls = new Map(
      [...tabsSrc.matchAll(/pub (?:crate )?const (\w+): &str = "(https?:\/\/[^"]+)"/g)].map((m) => [
        m[1],
        new URL(m[2]).hostname,
      ])
    );
    const tabHostsFixed = [...tabBody.matchAll(/OFFICIAL_CHAT_(\w+)/g)].map((m) => {
      const host = urls.get(`OFFICIAL_CHAT_${m[1]}`);
      if (!host) fail('official-chat-tabs', `OFFICIAL_CHAT_TABS 里的 ${m[1]} 在本文件找不到对应的地址常量`);
      return host;
    });
    const optionalHosts = [...optionalBody.matchAll(/OFFICIAL_CHAT_(\w+)/g)].map((m) => {
      const host = urls.get(`OFFICIAL_CHAT_${m[1]}`);
      if (!host) fail('official-chat-tabs', `条件页签表里的 ${m[1]} 在本文件找不到对应的地址常量`);
      return host;
    });
    // 品牌条带认的是**两张表的并集**：条件页签出现时同样是官网页签，顶条
    // 变绿会看起来像串了页。反向也成立——站表里多出来的站是死的。
    //
    // 2026-10-24 起比对口径从「host 一一对应」改成**站级**：脚本里是一张
    // 「站 → 该站可能出现的内容页 hostname + 品牌色」的表，一个站可能有多个
    // hostname（MiniMax 的入口 `agent.minimaxi.com` 会 302 到落地域
    // `agent.minimax.cn`，而内容 webview 最终的 location.hostname 是后者）。
    // 仍是一一对应的话，落地域会被判成「死条目」——而它恰恰是**必须**在的
    // 那一个（漏了它，MiniMax 顶条就走成工作台绿，见 2026-10-24 用户实机）。
    const tabHosts = [...new Set([...tabHostsFixed, ...optionalHosts])];
    // 品牌色表（Rust 侧 `official_chat::OFFICIAL_CHAT_BRAND_COLORS`）是**页签
    // 文字颜色**的事实源：页签栏是本地 SPA，颜色由 `official_chat_tabs` 随页签
    // 下发，它自己不写死十六进制（判据扫 `OfficialChatTabs.vue` 的字面量）。
    //
    // 2026-10-24：品牌色原先住在注入脚本 `titlebar-pulse.js` 的站表里，而那条
    // 脚本连同它给官方内容页画的 3px 彩线一起退役了（用户拍板移除），颜色因
    // 此搬回 Rust——那里本来就有页签表，两张表同源，不必跨文件对齐。
    const colorsBlock = tabsSrc.match(
      /OFFICIAL_CHAT_BRAND_COLORS: &\[\(&str, &str\)\] = &\[([\s\S]*?)\];/
    );
    const colorEntries = colorsBlock
      ? [
          ...colorsBlock[1].matchAll(/\(\s*"([^"]+)"\s*,\s*"(#[0-9a-fA-F]{3,8})"\s*\)/g),
        ].map((m) => ({ title: m[1], color: m[2] }))
      : fail(
          'official-chat-tabs',
          'official_chat.rs 里找不到 OFFICIAL_CHAT_BRAND_COLORS（品牌色表），判据失效'
        );
    // 页签标题：固定页签 + 条件页签两张表的并集。颜色表按标题配对。
    const fixedTitles = [...tabBody.matchAll(/\("([^"]+)",\s*OFFICIAL_CHAT_/g)].map((m) => m[1]);
    const optionalTitles = [...optionalBody.matchAll(/\("([^"]+)",\s*OFFICIAL_CHAT_/g)].map(
      (m) => m[1]
    );
    const tabTitles = [...fixedTitles, ...optionalTitles];
    if (colorEntries) {
      // ① 页签表里的每个页签都必须有品牌色：漏了 ⇒ 该页签文字退回中性色，
      //    页签栏一眼看不出是哪个站。
      for (const title of tabTitles) {
        if (!colorEntries.some((c) => c.title === title)) {
          fail(
            'official-chat-tabs',
            `页签 ${title} 没有品牌色——它的文字会退回中性色，页签栏读不出是哪个站`
          );
        }
      }
      // ② 颜色表里不许有页签表之外的条目（死条目）。
      for (const entry of colorEntries) {
        if (!tabTitles.includes(entry.title)) {
          fail('official-chat-tabs', `品牌色表里有 ${entry.title}，但页签表里没有它——那是死条目`);
        }
      }
      // ③ 两个页签不许同色：同色等于没分色。
      const seenColors = new Map();
      for (const entry of colorEntries) {
        if (seenColors.has(entry.color.toLowerCase())) {
          fail(
            'official-chat-tabs',
            `${entry.title} 与 ${seenColors.get(entry.color.toLowerCase())} 用了同一个品牌色 ${entry.color}——切页签时看不出换站`
          );
        }
        seenColors.set(entry.color.toLowerCase(), entry.title);
      }
      // ④ 三个值是用户拍板 + 官方素材取来的，写死在这儿是为了「谁把它换成别的
      //    粉/别的黑」能报红。ChatGPT 取**白**档：它的品牌主色是黑，而页签栏
      //    恒为深色（2026-10-08 用户拍板），黑字在深底上读不出来。
      const expected = { DeepSeek: '#4D6BFE', MiniMax: '#E2167E', ChatGPT: '#FFFFFF' };
      for (const [title, color] of Object.entries(expected)) {
        const entry = colorEntries.find((c) => c.title === title);
        if (entry && entry.color.toLowerCase() !== color.toLowerCase()) {
          fail('official-chat-tabs', `${title} 的品牌色是 ${entry.color}，不是 ${color}`);
        }
      }
    }
      // 条件页签表还必须真的被接线读到：表非空、而 `resolved_tabs` 不引用
      // 它的话，ChatGPT 页签永远不会被追加，这张表就成了摆设。
      if (optionalHosts.filter(Boolean).length > 0) {
        const resolver = tabsSrc.match(/pub\(crate\) fn resolved_tabs\([\s\S]*?\n\}/);
        if (!resolver || !resolver[0].includes('OFFICIAL_CHAT_OPTIONAL_TABS')) {
          fail(
            'official-chat-tabs',
            '条件页签表非空，但 resolved_tabs 没有引用 OFFICIAL_CHAT_OPTIONAL_TABS——条件页签永远不会被追加'
          );
        }
      }
      // **ACL 这一半最容易被漏掉**：内容 webview 的 origin 必须在
      // `capabilities/official-chat-remote.json` 的 remote.urls 里显式授过。
      // 漏了不会编译失败、build 也全绿，真机上是 Tauri 的 ACL 直接拒掉那个
      // webview 的一切 invoke，而页面自己的 `.catch` 把错误吞掉——症状是
      // 「页签点得开、里面一片空白」，没有任何线索。只查这一个方向：多出来的
      // origin 只是无用的 ACL 面，漏掉的 origin 才是静默故障。
      const remoteFile = join(root, 'src-tauri', 'capabilities', 'official-chat-remote.json');
      if (existsSync(remoteFile)) {
        const granted = new Set(
          (JSON.parse(readFileSync(remoteFile, 'utf8')).remote?.urls ?? []).map((entry) => {
            try {
              return new URL(entry).hostname;
            } catch {
              return null;
            }
          })
        );
        for (const host of tabHosts) {
          if (host && !granted.has(host)) {
            fail(
              'official-chat-tabs',
              `页签 ${host} 不在 capabilities/official-chat-remote.json 的 remote.urls 里——` +
                '该内容 webview 会被 Tauri 的 ACL 静默拒绝，表现为页签点开一片空白'
            );
          }
        }
      } else {
        fail('official-chat-tabs', '找不到 capabilities/official-chat-remote.json，判据失效');
      }
      // 前端那条紧急渲染路径（IPC 不可用时的兜底）也得跟着**固定**页签表走。
      // 条件页签刻意不要求出现在兜底表里：兜底存在的场景正是「问不到后端」，
      // 此时无从判断 Codex 是否可用，硬画一个页签会绕过登录态判据。
      const fallback = read('ui/src/official-chat/officialChatTabs.js');
      const missing = fixedTitles.filter((t) => !fallback.includes(`title: '${t}'`));
      if (missing.length > 0) {
        fail('official-chat-tabs', `前端兜底页签表缺 ${missing.join(' / ')}——IPC 不可用时页签栏会与后端不一致`);
      }
      const leaked = optionalTitles.filter((t) => fallback.includes(`title: '${t}'`));
      if (leaked.length > 0) {
        fail(
          'official-chat-tabs',
          `前端兜底页签表不该包含条件页签 ${leaked.join(' / ')}——兜底路径问不到后端，画出来等于绕过登录态判据`
        );
      }
      // 兜底表里的**颜色**也要与 Rust 表同值：颜色现在由后端下发，兜底路径
      // （IPC 不可用）走的是前端那份副本——两处不同值就是「IPC 一坏页签就变色」。
      // 只查固定页签：条件页签刻意不进兜底表（上一条）。
      for (const entry of colorEntries || []) {
        if (!fixedTitles.includes(entry.title)) continue;
        const m = new RegExp(
          `title:\\s*'${entry.title}'\\s*,\\s*color:\\s*'(#[0-9a-fA-F]{3,8})'`
        ).exec(fallback);
        if (!m) {
          fail(
            'official-chat-tabs',
            `前端兜底页签表里 ${entry.title} 缺品牌色——IPC 不可用时它的文字会没有颜色`
          );
        } else if (m[1].toLowerCase() !== entry.color.toLowerCase()) {
          fail(
            'official-chat-tabs',
            `前端兜底页签表的 ${entry.title} 颜色是 ${m[1]}，与后端 ${entry.color} 不一致——IPC 一坏页签就变色`
          );
        }
      }
      if (!missing.length && tabHosts.length > 0) {
        note(
          `${fixedTitles.length} 个固定页签 + ${optionalTitles.length} 个条件页签在 Rust 页签表 / 品牌色表 / 前端兜底表三处一致`
        );
      }
  }
}

// [21] 预检阶段发了「正在…」就必须有终态出口
//
// 2026-10-07 用户截图：预检「通过」之后，时间线里「1. 创建沙盒环境」永远转着
// 圈，而摘要已经写了「已完成」。根因在 `precheck.rs`——`SANDBOX_CREATE` 只在
// **失败**路径补了终态，成功路径一条都没发。前端折叠救不了它：折叠只吃「被
// 同阶段终态取代」的进行中，悬空的那条谁也取代不了。
//
// 这条判据只扫 `precheck.rs`，因为只有它把 `run::status::RUNNING` 直接写在宏
// 调用里。`startup_run.rs` / `operation_run.rs` 走 `WatchText` 结构体的
// `running_status` / `done_status` 字段对，扫不出配对关系——那两处靠
// `Recorder::finish` 的兜底（见 `run.rs` 的 `settle_dangling_stage`）。而兜底
// 只保证「收尾那条不是进行中」，管不到**中间**悬空的阶段。

{
  const src = read('src-tauri/src/plugins/precheck.rs');
  const running = [
    ...new Set([...src.matchAll(/run::stage::(\w+),\s*run::status::RUNNING/g)].map((m) => m[1])),
  ];
  if (running.length === 0) {
    fail('precheck-stage-terminal', 'precheck.rs 里一个「正在…」都扫不到，判据失效');
  } else {
    // **只认 SUCCESS 出口**：这个 bug 的形状恰恰是「失败路径有终态、成功路径
    // 没有」——`SANDBOX_CREATE` 原本只有一条 FAILURE 出口，若把 FAILURE 也算
    // 数，判据对着自己要去防的那个洞说通过（2026-10-07 实测）。
    const dangling = running.filter(
      (stage) => !new RegExp(`run::stage::${stage},\\s*run::status::SUCCESS`).test(src)
    );
    for (const stage of dangling) {
      fail(
        'precheck-stage-terminal',
        `预检阶段 ${stage} 发了「正在…」却没有任何终态出口——时间线上它会永远转圈`
      );
    }
    if (!dangling.length) {
      note(`${running.length} 个预检阶段的「正在…」都有终态出口（${running.join(' / ')}）`);
    }
  }
}

// --- 24. 数据目录恒在 `~/.dsh-xlink`，不再有平台分支 ------------------------
//
// `kernel::lifecycle::data_dir` 曾有一条回落到 Tauri `app.path().app_data_dir()`
// 的兜底，理由是「xlink_home 不可写时宁愿在某个地方启动」。2026-10-08 用户拍板
// 撤掉：那条分支让**同一个产品在两个平台上数据落在完全不同的地方**——macOS 是
// `~/Library/Application Support/<id>`、Windows 是 `%APPDATA%\<id>`，既不在
// `~` 下面也不随 `DSH_XLINK_HOME` 走。于是「数据目录在 `~/.dsh-xlink`」这个承诺
// 在 Windows 上根本不成立，而 UI 显示的那条路径与用户实际被写到哪儿会分家。
//
// 为什么用机械判据钉：这种兜底写起来只有一行、读起来像一条负责的降级，删掉它
// 的理由（跨平台一致性）又完全不在编译器眼里。加回来不会有任何测试变红，只有
// 用户在 Windows 上找不到自己的数据。
//
// 判据扫**生产代码**（剥掉 `#[cfg(test)]` 整块与行注释）：测试里提到这个名字是
// 正常的——本条的存在本身就要在注释里解释它为什么被删掉。

{
  const dataDirRoot = join(root, 'src-tauri', 'src');
  const rustFiles = walk(dataDirRoot, ['.rs']);
  const offenders = [];
  for (const full of rustFiles) {
    const file = full.slice(dataDirRoot.length + 1);
    const body = productionRust(readFileSync(full, 'utf8'));
    if (/app_data_dir|app\.path\(\)\s*\.\s*app_data/.test(body)) {
      offenders.push(file);
    }
  }
  for (const file of offenders) {
    fail(
      'data-dir-platform-branch',
      `${file} 又出现了 app_data_dir()：数据目录必须恒在 ~/.dsh-xlink` +
        '（macOS 的 ~/Library/Application Support 与 Windows 的 %APPDATA% 都不在 ~ 下，' +
        '会让同一产品在两个平台上落在不同地方，UI 显示的路径也与实际写入处分家）'
    );
  }
  if (!offenders.length) {
    note('数据目录解析无平台分支：两平台都恒在 ~/.dsh-xlink（无 app_data_dir 兜底）');
  }
}

// --- 25. AGENTS.md 不超运行时指令预算 --------------------------------------
//
// 运行时把项目指令渲染进上下文时有一个字节预算，**这个数由内核配置给出，不是猜
// 的**：`kernels/<版本>/node_modules/@deepseek-ai/dsh-base/cordis.patch.yml` 里
// `agent-instructions: { maxBytes: 65536 }`（0.2.1-alpha.2 实读；`dsh-web-app` 的
// 三份 preset 同值）。超了会怎样：尾部被截断，运行时在渲染结果的最前面插一行
// `Workspace instruction budget 65536 bytes: truncated AGENTS.md from 33683 to
// …bytes`（内核 `dsh-agent-instructions` 的 `markerText`）。
//
// **所以判据仍然必要**：截断从「静默」变成了「会自己出声」，但**没有人会去读那行
// 标记**——症状仍然是「文档里明明写了却没人遵守」，而读到文档的人根本无从知道
// 自己漏读了哪一段。2026-10-11 实测就是这个形状：文件 33683 字节，尾部两节
// （「## 文档」「## 窗口外观作用域」）从来没被读到过，「修改用户可见行为要同步
// 更新 README 和 docs」那条纪律写了半年没人看到。
//
// 上限取 64000，比运行时的 65536 低 1536：余量让「下次正常加一条规则」不会立刻
// 红，而余量用尽时门禁会明确告诉你该把故事搬去 `docs/reviews/agent-lessons.md`
// 而不是删纪律。**内核升版后要重新读那份 `maxBytes`**，别照抄这里的数——
// 32768 → 65536 就是这么变过一次（本条 2026-10-11 写下时按的是 32768）。

{
  const RUNTIME_BYTE_BUDGET = 65536;
  const AGENTS_BYTE_BUDGET = 64000;
  // **三份都要查**：运行时把三份都当工作区指令渲染（根目录那份 + 各子目录那份），
  // 2026-10-25 实测 `ui/AGENTS.md` 到 66089 字节时被静默截到 65219——**尾部 870 字节
  // 从来没被读到过**，而当时这道判据只查根目录那份，所以它是绿的。
  // 只查一份的判据等于给另外两份开了后门：那是**假门禁**。
  for (const rel of ['AGENTS.md', 'ui/AGENTS.md', 'src-tauri/AGENTS.md']) {
    const agentsPath = join(root, rel);
    if (!existsSync(agentsPath)) {
      note(`${rel} 不存在，跳过字节预算检查`);
      continue;
    }
    const bytes = readFileSync(agentsPath).length;
    if (bytes > AGENTS_BYTE_BUDGET) {
      // 两种超法要说两句话：超本仓预算是「该搬故事了」，超运行时上限才是「尾部已经
      // 真的不会被读到」。混成一句就会在数字改小后开始说假话（反向验时见过）。
      const overRuntime = bytes > RUNTIME_BYTE_BUDGET;
      const headline = overRuntime
        ? `已经越过运行时 ${RUNTIME_BYTE_BUDGET} 字节的指令上限，尾部会被截掉：` +
          `运行时会插一行 "Workspace instruction budget ${RUNTIME_BYTE_BUDGET} bytes: truncated …"，` +
          `但没人会去读它——写在这里却从没被读到过的规则比没写更坏。`
        : `已超出本仓 ${AGENTS_BYTE_BUDGET} 字节的预算（运行时上限 ${RUNTIME_BYTE_BUDGET}，` +
          `离被截断还剩 ${RUNTIME_BYTE_BUDGET - bytes} 字节）：预算刻意留了余量，` +
          `好让「加一条规则」不会拖到某天把纪律半截丢掉。`;
      fail(
        'agents-md-byte-budget',
        `${rel} 已 ${bytes} 字节。${headline}` +
          `把事故经过搬到 docs/reviews/agent-lessons.md，纪律本身留在这里` +
          `（超预算 ${bytes - AGENTS_BYTE_BUDGET} 字节）。`
      );
    } else {
      note(
        `${rel} ${bytes} 字节，在 ${AGENTS_BYTE_BUDGET} 字节预算内` +
          `（距运行时 ${RUNTIME_BYTE_BUDGET} 字节上限还有 ${RUNTIME_BYTE_BUDGET - bytes} 字节）`
      );
    }
  }
}

// --- 26. 启动对账不许变成死代码 --------------------------------------------
//
// `skills::manage::reconcile` 曾长期定义着却**没有任何生产调用方**：
// `reconcile_home` 的五个调用点全在 `#[cfg(test)]` 内。后果是
// `fetch_into_store` 那两步 rename 之间崩溃留下的 `.backup-*`（此刻正式位置
// 是空的）永远没人收拾，而 `manage.rs` 给用户的原文「下次启动时 reconcile
// 会接手」承诺的那次启动永远不会到来。
//
// 为什么用机械判据钉：调用方消失不会让任何编译或测试变红（`reconcile` 是
// `pub fn`，且 crate 级 `#![allow(dead_code)]` 把编译器本该指出的声音一并静音），
// 单测也照样全绿——它测的是 `reconcile_home` 本身，不是「壳启动时会不会跑它」。
// 判据扫 `lib.rs` 的 `setup` 闭包文本，确保这条恢复路径接在启动流程里。
//
// **判据必须匹配未被注释的调用，不能匹配原始文本**：2026-10-11 反向验时把
// 调用改成 `// MUTATED: ...` 之后判据依然绿——正则照样命中注释里那一份串。
// 「形状断言会忠实地保护错误的形状」：钉"文本里出现过这个串"钉不住"它在跑"。
// 所以这里先过 `productionRust`（它剥掉 `#[cfg(test)]` 整块与行注释）再匹配。

{
  const libPath = join(root, 'src-tauri', 'src', 'lib.rs');
  const lib = readFileSync(libPath, 'utf8');
  const setupStart = lib.indexOf('.setup(|app|');
  const setupBody = setupStart >= 0 ? lib.slice(setupStart, setupStart + 40000) : '';
  const setupProduction = productionRust(setupBody);
  if (!/skills::manage::reconcile\(\)/.test(setupProduction)) {
    fail(
      'skills-reconcile-wired',
      'src-tauri/src/lib.rs 的 setup() 里没有生效的 skills::manage::reconcile() 调用：' +
        '技能中央库的启动对账又变成了死代码，崩溃残留（.backup-*/.new-*/.tmp-*）' +
        '会无限堆积，而 manage.rs 给用户承诺的「下次启动时 reconcile 会接手」' +
        '不会兑现。判据只扫 setup 闭包的生产代码——注释里写一遍不算。'
    );
  } else {
    note('技能中央库启动对账接在 setup() 里（崩溃残留有人回收）');
  }
}

// --- 27. reconcile 必须带跨进程守卫 ----------------------------------------
//
// 第 26 项把 reconcile 接进了 setup，但**接上它本身引入了一个新风险**：技能中央库
// 与活动视图是两个壳共享的（有意选的，见 AGENTS.md），而 `lock_store()` 只到进程级
// Mutex。dev 壳正在装技能时它的 `.tmp-<pid>-<nanos>` 还在，release 壳此刻跑对账会
// ① `recover_staging` 把对方正在用的 `.tmp-*` 当上次崩溃的残留回收掉，
// ② 孤儿清扫把「清单里还没记录、但活动根里已有链接」的那份技能当孤儿删掉——
// 正是另一个壳正在装的那一个。
//
// 守卫是 `skills/store_busy.rs` 的 `another_writer_active`（判暂存目录名里折着的
// pid 是否还活着，且目录近期被写过）。它被后人删掉时不会有任何单测变红——那些
// 测的是那个函数本身，不是「reconcile 有没有用它」——所以接线在这里钉。
// 与第 26 项同一条纪律：**匹配生产代码，注释里写一遍不算**（判据先过
// `productionRust`；直接匹配原始文本的话，把调用改成注释会假绿）。

{
  const managePath = join(root, 'src-tauri', 'src', 'skills', 'manage.rs');
  const manage = productionRust(readFileSync(managePath, 'utf8'));
  const start = manage.indexOf('pub fn reconcile()');
  const body = start >= 0 ? manage.slice(start, start + 2000) : '';
  if (!/store_busy::another_writer_active/.test(body)) {
    fail(
      'skills-reconcile-cross-process-guard',
      'skills::manage::reconcile() 里没有 store_busy::another_writer_active 调用：' +
        '技能中央库是两个壳共享的而 lock_store() 只到进程级，dev 壳正在装技能时 ' +
        'release 壳的对账会把它正在用的暂存目录当崩溃残留回收掉，并把「清单里还没' +
        '记录、但活动根里已有链接」的那份技能当孤儿删掉——正是另一个壳正在装的那一个。'
    );
  } else {
    note('启动对账带跨进程守卫（另一个壳正在装技能时整轮跳过）');
  }
}

// --- 28. 插件中央库的启动对账必须接在 setup() 里 ---------------------------
//
// 与第 26 项同一个仓库、同一种事故：**定义在那里、doc 里承诺「下次启动时
// reconcile 会接手」，但零生产调用方**。2026-10-12 扫出来时，
// `plugins::center::reconcile_store` 的四个调用点全在 `#[cfg(test)]` 内，于是
// ① 装/更新/卸载中途崩溃留下的 `tmp-*` / `new-*` / `backup-*` 永久堆积；
// ②「已发布但目录缺失」的中间态永不修复；③ v0.2.x 平铺中央库的搬迁从未发生。
//
// skills 那次修完，plugins 这条一直没人碰——所以单独钉，不指望「顺便发现」。
// 判据只扫 setup 闭包的生产代码，注释里写一遍不算（与第 26 项同一条纪律：
// 直接匹配原始文本的话，把调用改成注释会假绿）。

{
  const libPath = join(root, 'src-tauri', 'src', 'lib.rs');
  const lib = readFileSync(libPath, 'utf8');
  // 锚点必须在**原始文本**上找，剥注释只对切出来的闭包体做。顺序反过来
  // （先 productionRust 全文再找锚点）会踩两个坑：① Tauri 的入口是
  // `.setup(|app| { … })` **闭包**，`fn setup(` 这个串在 lib.rs 里根本不存在；
  // ② 剥掉注释后行号与位置全变。第 26 项就是这个顺序，别再抄错。
  const setupStart = lib.indexOf('.setup(|app|');
  const setupBody = setupStart >= 0 ? lib.slice(setupStart, setupStart + 40000) : '';
  const setupProduction = productionRust(setupBody);
  if (!/plugins::center::reconcile_store\(/.test(setupProduction)) {
    fail(
      'plugins-reconcile-wired',
      'src-tauri/src/lib.rs 的 setup() 里没有生效的 plugins::center::reconcile_store 调用：' +
        '插件中央库的启动对账又变成了死代码，崩溃残留（tmp-*/new-*/backup-*）会无限堆积，' +
        '「已发布但目录缺失」的中间态永不修复，v0.2.x 平铺中央库的搬迁也不会发生——' +
        '而 center.rs 多处注释写着「下次启动时 reconcile 会接手」。' +
        '判据只扫 setup 闭包的生产代码——注释里写一遍不算。'
    );
  } else {
    note('插件中央库启动对账接在 setup() 里（崩溃残留有人回收）');
  }
}

// --- 29. 「点通知横幅回到工作台」的两条入口都必须接在 serve() 上 --------------
//
// 2026-10-13 macOS 26.7.1 实测：横幅点击只派发 `applicationDidBecomeActive`，
// `applicationShouldHandleReopen` 一次都没有来——挂在 `RunEvent::Reopen` 上的那条
// 接线在真机上从未触发过一次，点横幅表现为完全没反应（2026-10-13 修，改成
// `activate::serve` 里注册激活观察者）。
//
// 单测钉不住这件事：`banner_activation` 是纯函数，被测得再准，把
// `observe_activation(app)` 从 `serve()` 里摘掉之后**全部门禁照样绿**——判据在
// 测试里被引用着，编译器与测试都不会说话。同一份代码里还有一条更便宜的失手
// 方式：三个 macOS 专属符号漏挂 `#[cfg(target_os = "macos")]`，Windows 上全成死
// 代码，而 clippy `-D warnings` 只在 macOS 上跑（两个 workflow 的 quality job
// 都是 `macos-15-intel`），发布流水线看不见，只有本地 Windows 上的
// `npm run check` 会红。所以这里把两半一起钉成接线形状。
//
// 判据扫的是生产代码（`productionRust` 剥掉测试块与注释）。单测自己的 cfg 由
// Windows CI 那个 job 兜住：它编 Windows 的测试目标（2026-10-15 起还跑
// `cargo test --locked`），漏挂时编不过。

{
  const activateLines = productionRust(shellSource('activate.rs')).split('\n');

  /** 形如「符号声明 / 调用点的前一行就是指定的 cfg 门控」时为 true。 */
  const gatedBy = (pattern, gate) => {
    const index = activateLines.findIndex((line) => pattern.test(line));
    if (index < 0) return null;
    return activateLines[index - 1].trim() === gate;
  };

  const serveIndex = activateLines.findIndex((line) => /pub fn serve\(/.test(line));
  if (serveIndex < 0) {
    fail(
      'banner-activate-wiring',
      'notify/activate.rs 里找不到 serve()——「点通知横幅回到工作台」的两条入口都没地方接了。',
    );
  } else {
    const body = activateLines.slice(serveIndex, serveIndex + 12).join('\n');
    const missing = [
      ['ipc::serve(app);', 'Windows：系统点横幅拉起的第二个进程靠它把「回到工作台」递进管道'],
      ['observe_activation(app);', 'macOS：横幅点击只派发激活观察者，这条是唯一的信号'],
    ].filter(([token]) => !body.includes(token));
    if (missing.length > 0) {
      fail(
        'banner-activate-wiring',
        `notify/activate.rs:${serveIndex + 1} 的 serve() 里缺 ${missing.map(([token]) => token).join(' / ')}：` +
          missing.map(([, why]) => why).join('；') +
          '。serve() 是两条入口唯一的挂载点，摘掉任一条，点横幅就退回「完全没反应」，而单测与编译器都不会响。',
      );
    }
  }

  const ungated = [
    [/ipc::serve\(app\);/, '#[cfg(target_os = "windows")]', 'Windows 的管道监听入口'],
    [/observe_activation\(app\);/, '#[cfg(target_os = "macos")]', 'macOS 的激活观察者挂载'],
    [/const BANNER_FRESH_MS/, '#[cfg(target_os = "macos")]', '横幅新鲜度常量'],
    [/fn banner_activation\(/, '#[cfg(target_os = "macos")]', '激活判据函数'],
    [/fn on_activated\(/, '#[cfg(target_os = "macos")]', '激活回调'],
  ].filter(([pattern, gate]) => gatedBy(pattern, gate) !== true);
  if (ungated.length > 0) {
    fail(
      'banner-activate-wiring',
      `notify/activate.rs 里这些符号前面的门控不是它该有的那一条：` +
        ungated.map(([, gate, what]) => `${what} 期望 ${gate}`).join('；') +
        '。漏挂 macOS 门控时，Windows 上这几个符号全是死代码，而 clippy `-D warnings` ' +
        '只在 macOS 上跑，本地 Windows 上的 `npm run check` 会红；漏挂 Windows 门控则把 ' +
        'macOS 也编成一份没人调的死代码。',
    );
  }

  if (serveIndex >= 0 && ungated.length === 0) {
    note('点通知横幅回到工作台：Windows 管道交接与 macOS 激活观察都接在 serve() 上，门控成对');
  }
}

// --- 记忆进度表必须覆盖每个记忆模块 -------------------------------------------
//
// `docs/features/memory/memory-progress.md` 是记忆系统的开发清单：设计稿回答「为什么这么
// 设计」，它回答「现在做到哪了、还差什么」。它会失效的方式只有一种——**加了模块却没人补
// 那一行**，而这件事没有任何症状：文档还在、门禁全绿、下次读清单的人以为「就这些」。
//
// 判据只做覆盖（每个记忆模块都要在清单里出现），不判状态写得对不对——那要靠评审。覆盖能
// 机械判，而且正是最容易漏的一步：新增一个 `memory/xxx.rs` 时，必须同步补充清单。
{
  const progressPath = join(root, 'docs/features/memory/memory-progress.md');
  let progress = null;
  try {
    progress = readFileSync(progressPath, 'utf8');
  } catch {
    fail(
      'memory-progress-coverage',
      '找不到 docs/features/memory/memory-progress.md：它是记忆系统的开发清单，删掉或改名都要同步这条判据。',
    );
  }
  if (progress) {
    const modules = [
      ...walk(join(root, 'src-tauri/src/memory'), ['.rs']),
      ...walk(join(root, 'plugins/memory/host'), ['.js']),
      ...walk(join(root, 'ui/src/memory'), ['.js', '.vue']),
    ].map((file) => relative(root, file).split(sep).join('/'));
    // 路径或文件名命中都算：清单里写 `memory/store.rs` 就够，不必抄全路径。
    const missing = modules.filter((file) => {
      const base = file.split('/').pop();
      return !progress.includes(file) && !progress.includes(base);
    });
    if (missing.length > 0) {
      fail(
        'memory-progress-coverage',
        `memory-progress.md 里没有这些记忆模块：${missing.join('、')}。` +
          '在对应区段补一行（状态 + 判据 + 还差什么）；确实不该出现在清单里的，把这条判据一起改。',
      );
    } else {
      note(`记忆进度表覆盖全部 ${modules.length} 个记忆模块`);
    }
  }
}

// --- 记忆清单里引用的判据必须真的存在 ------------------------------------------
//
// 清单的价值全在「判据在哪跑」那一列：它写着 `the_same_body_is_not_collected_twice`，维护者据此
// 相信那条功能有判据护着；写着 `memory/routes.rs`，据此知道去哪改。**而重命名一个测试、删掉一条
// 路由、搬走一个文件都不会让清单报错**——它会安静地指向一个不存在的东西，比没有清单更坏：
// 有人照着它以为自己验过了（2026-10-15 加这一项时，清单里 56 条测试名引用、48 条文件引用全部
// 都对得上，但它此前的「对得上」纯属运气）。
//
// 三类引用各按能机械判的方式钉：
// · **测试名**（形如 `the_same_body_...` 的长标识符）必须出现在产品与测试源码里；只出现在
//   文档或脚本注释里的不算——那正是「判据被删了而清单还在」的形状。**不是本仓符号**的引用
//   登记在下表里，并写清它是什么。
// · **文件**：仓库相对路径、`src-tauri/` 下的路径、或任一已知根下的同名文件都算命中
//   （清单里常只写 `bridge.rs` 这样的文件名）。运行期数据文件在下表里登记。
// · **桥路由**：`/v1/memory/...` 要么整串出现在路由源码里，要么它的最后一段作为 match 臂的
//   字面量出现（路由是「前缀 + 臂」的形状，整串并不在源码里）。
{
  const progressPath = join(root, 'docs/features/memory/memory-progress.md');
  let progress = '';
  try {
    progress = readFileSync(progressPath, 'utf8');
  } catch {
    // 上一项已经报过「清单缺失」，这里不重复报。
  }
  if (progress) {
    /** 引用的**不是本仓符号**：在这里登记并写清它是什么。 */
    const external = new Set([
      'bridge-cred.json', // 手动 memory 夹具在独立证据目录生成的私有桥接握手文件
      'kernel-evidence-r3/results.r1.json', // 2026-10-23 真内核请求复验的本机结果文件，原始请求随证据目录保留
      'memory_search', // 插件暴露给模型的工具名（在 plugins/memory/host 里，不是 Rust 侧符号）
      'memory_submit_candidate', // 已删除的命令：清单里作为历史提及
      'screencapture', // macOS 系统命令
      // **计划中的字段名**：C5 的候选冲突标记，尚未落地（清单里那一条写明了落地形状与前置条件）。
      // 登记在这里是为了让这条判据继续对**已落地**的引用保持严格——计划中的名字不该绕过它。
      'conflicts_with',
      'jobs.json', // 运行期数据文件（与 memory.json 同目录），不在仓库里
      'memory.json', // 同上：权威记录本体
      'state.json', // 内置插件的运行期状态文件（`extensions/builtin/<id>/state.json`），不在仓库里
    ]);

    const sourceRoots = [
      join(root, 'src-tauri', 'src'),
      join(root, 'plugins'),
      join(root, 'ui'),
      join(root, 'e2e'),
    ];
    const sourceFiles = sourceRoots.flatMap((dir) => walk(dir, ['.rs', '.js', '.mjs', '.vue']));
    // **必须剥注释再匹配**：这一条第一次写出来是假绿的——把 store.rs 里的判据改名之后它照样
    // 通过，因为旧名字还留在上面那几行注释里（判据命中的是解释，不是代码）。同一类坑本仓已经
    // 栽过三次（见 agent-lessons §9）。
    //
    // **只剥行首的注释**：第二版把「任意位置的 `/* … */`」都剥掉，结果误伤了真代码——
    // 源码里有 `'**/*.js'` 这样的 glob 字符串，它的 `/*` 会与几千行之后的 `*/` 配成一对，
    // 一次吞掉一大片，于是 37 条**明明存在**的判据全被判成找不到。要求注释符号出现在
    // 行首（前面只允许空白），假配对就基本不可能发生，而 `///` 文档注释照样剥得到。
    const stripComments = (text) =>
      text
        .replace(/^[ \t]*\/\*[\s\S]*?\*\//gm, '')
        .replace(/^[ \t]*<!--[\s\S]*?-->/gm, '')
        .replace(/^[ \t]*\/\/.*$/gm, '');
    const sourceBlob = stripComments(
      sourceFiles.map((file) => readFileSync(file, 'utf8')).join('\n'),
    );

    const allRoots = [...sourceRoots, join(root, 'scripts'), join(root, 'docs')];
    const knownFiles = allRoots.flatMap((dir) =>
      walk(dir, ['.rs', '.js', '.mjs', '.vue', '.json', '.md']),
    );
    const knownNames = new Set(knownFiles.map((file) => file.split(sep).pop()));

    const cited = new Set([...progress.matchAll(/`([^`\n]+)`/g)].map((match) => match[1]));
    const problems = [];
    for (const citation of cited) {
      if (external.has(citation) || citation.includes('<') || citation.includes('*')) continue;
      if (/^\/[a-z]/i.test(citation)) continue; // 路由单独判
      const isTestName = /^[a-z][a-z0-9_]{12,}$/.test(citation);
      const isFile = /^[A-Za-z0-9_./-]+\.(rs|js|mjs|vue|json|md)$/.test(citation);
      if (isFile && !isTestName) {
        const candidates = [join(root, citation), join(root, 'src-tauri', citation)];
        if (candidates.some((file) => existsSync(file))) continue;
        if (knownNames.has(citation.split('/').pop())) continue;
        problems.push(`文件 \`${citation}\` 找不到`);
        continue;
      }
      if (isTestName && !sourceBlob.includes(citation)) {
        problems.push(`判据名 \`${citation}\` 在产品与测试源码里找不到`);
      }
    }

    // 桥路由：整串或它的最后一段必须出现在**分发或路由源码**里。
    //
    // 两个文件都要读：2026-10-15 把路由**臂的主体**拆进 `memory/routes.rs` 时，`starts_with("/v1/memory/…")`
    // 那些字面量**留在 `bridge.rs` 的分发里**——只读 routes.rs 的判据因此把清单里明明正确的路由引用
    // 判成了找不到（判据自己第一次跑红就是这个原因）。
    const routesSource = [
      join(root, 'src-tauri', 'src', 'memory', 'routes.rs'),
      join(root, 'src-tauri', 'src', 'plugins', 'builtin', 'bridge.rs'),
    ]
      .map((file) => readFileSync(file, 'utf8'))
      .join('\n');
    for (const citation of cited) {
      if (!/^\/v1\/memory\//.test(citation) || citation.includes('*')) continue;
      // 路由源码里是「前缀 + 臂」：`/v1/memory/jobs/submit` 对应臂 `"/submit"`。
      const leaf = `"/${citation.split('/').pop()}"`;
      if (!routesSource.includes(citation) && !routesSource.includes(leaf)) {
        problems.push(`路由 \`${citation}\` 在 memory/routes.rs 里找不到`);
      }
    }

    if (problems.length > 0) {
      fail(
        'memory-checklist-citations',
        `memory-progress.md 里的引用对不上代码：${problems.join('；')}。` +
          '改名的判据 / 挪走的文件要同步清单；确实不是本仓符号的，登记进这一项的 external 表并写清它是什么。',
      );
    } else {
      note(`记忆清单里的 ${cited.size} 条反引号引用全部对得上代码`);
    }
  }
}

// 内置插件的 `peers` 必须覆盖 host 静态 import 的每个 `@deepseek-ai` 包
// （2026-10-15，评审第 1 条）。
//
// 起因：`plugins/memory/host/index.js` 静态 import 了 `@deepseek-ai/dsh-llm`，
// 而 `builtins.rs` 的 spec 只登记了 `dsh-tools`。物化只按 `spec.peers` 在
// `<物化目录>/node_modules/@deepseek-ai/` 下建链接，**内核树不是那个目录的
// 祖先**——那一行 ESM 解析不到，整个插件 `failed to import`。因为
// `xlink-memory` 不在内核的必需启动项里，症状只是内核日志多一行
// `warning: 1 entry did not activate`，而工作台照常开、面板照常显示「已启用」：
// **注入、三个工具、提取全静默死掉**，用户看到的是「功能像是开着，但模型
// 从不提记忆」。
//
// 四道门当时全都没拦住，因为**没有任何一处 import 过 `host/index.js`**：
// 冒烟判据只 import 纯逻辑模块，接线那两条把 `index.js` 当文本正则匹配，
// `prepare-builtin-plugins --check` 只比源 hash。所以这里查的不是行为而是
// **声明与用法的差**——它是跨语言的（Rust 的 `peers` vs JS 的 `import`），
// 编译器与打包器都看不见。
//
// 两条规则，与 `src-tauri/AGENTS.md` 的「peer 分必需与可选」逐字对应：
//   ① 静态 import 的每个包都必须在 `peers` 里；
//   ② 静态 import 的必须是 `required`——静态 import 一个可选 peer，缺包时
//      整个插件加载失败，降级无从谈起（可选 peer 只许动态 `import()`）。
// 动态 `import()` 的包可以不出现在 `peers` 里（那是降级路径，缺了按 missing 报）。
//
// 锚点用**行首** `import`：注释里写一遍不算，也不做全量剥注释（字符串里的
// `//` 会让剥注释反过来吃掉真代码）。本仓的插件 host 都是顶层 ESM import，
// 这个锚点足够准；真出现行内 import 时这里要跟着改。
//
// **引号两种都要认**（2026-10-15 实测补）：第一版只写了单引号，而
// `plugins/openai-oauth/` 全篇用双引号——于是它把 `dsh-llm` 从 peers 里漏掉
// 之后，这条判据照旧打印「通过」。同一个洞还有第二半：相对 import 的遍历也只
// 认单引号，`host/index.js` 里那句 `from "./adapter.js"` 因此没被跟上，
// `adapter.js` 那份静态 import 从头到尾没进过集合。**判据的假绿比没有判据更坏**
// ——它当时是唯一一条会看 `peers` 与 `import` 的差的门禁。
{
  const builtinsSource = read(join('src-tauri', 'src', 'plugins', 'builtin', 'builtins.rs'));
  const problems = [];
  const specs = builtinsSource.split(/BuiltinSpec\s*\{/).slice(1);
  if (specs.length === 0) {
    fail(
      'builtin-peer-imports',
      '在 builtins.rs 里一个 `BuiltinSpec {` 都没解析出来——判据的锚点过期了，' +
        '它现在恒绿。先修锚点再谈通过。',
    );
  }
  for (const spec of specs) {
    const rowId = /row_id:\s*"([^"]+)"/.exec(spec)?.[1];
    const entry = /entry:\s*"([^"]+)"/.exec(spec)?.[1];
    if (!rowId || !entry) continue;
    const dir = rowId.replace(/^xlink-/, '');
    const peersBlock = /peers:\s*&\[([\s\S]*?)\]/.exec(spec)?.[1] ?? '';
    const required = new Set();
    const optional = new Set();
    for (const match of peersBlock.matchAll(/PeerSpec::(required|optional)\(\s*"([^"]+)"\s*\)/g)) {
      (match[1] === 'required' ? required : optional).add(match[2]);
    }

    // 顺着相对 import 走一遍 host 那一侧，收集所有 `@deepseek-ai` 引用。
    const hostDir = join(root, 'plugins', dir, 'host');
    const staticImports = new Set();
    const dynamicImports = new Set();
    const seen = new Set();
    const queue = [join(hostDir, baseName(entry))];
    while (queue.length > 0) {
      const file = queue.pop();
      if (seen.has(file) || !existsSync(file)) continue;
      seen.add(file);
      const text = readFileSync(file, 'utf8');
      for (const match of text.matchAll(/^\s*import\s[^;]*?from\s+['"](@deepseek-ai\/[^'"]+)['"]/gm)) {
        staticImports.add(match[1]);
      }
      for (const match of text.matchAll(/^\s*import\s+['"](@deepseek-ai\/[^'"]+)['"]/gm)) {
        staticImports.add(match[1]);
      }
      for (const match of text.matchAll(/\bimport\(\s*['"](@deepseek-ai\/[^'"]+)['"]/g)) {
        dynamicImports.add(match[1]);
      }
      // 相对引用三种形状都跟：`from './x.js'`（含 `export … from`）、
      // 副作用 `import './x.js'`、以及省略扩展名的 `from './x'`。
      const relatives = [];
      for (const match of text.matchAll(/\bfrom\s+['"](\.[^'"]+)['"]/g)) relatives.push(match[1]);
      for (const match of text.matchAll(/^\s*import\s+['"](\.[^'"]+)['"]/gm)) relatives.push(match[1]);
      for (const spec of relatives) {
        const target = resolve(dirname(file), spec);
        const candidates = [target, `${target}.js`, `${target}.mjs`, join(target, 'index.js')];
        const found = candidates.find((candidate) => existsSync(candidate));
        if (found === undefined) {
          problems.push(`${rowId} 的 ${baseName(file)} 引用了 \`${spec}\`，但那个文件不在磁盘上`);
          continue;
        }
        queue.push(found);
      }
    }

    for (const pkg of staticImports) {
      const short = pkg.replace('@deepseek-ai/', '');
      if (!required.has(short)) {
        problems.push(
          `${rowId} 的 host 静态 import 了 \`${pkg}\`，但 peers 里` +
            (optional.has(short) ? '它是 **optional**' : '没有它') +
            `——物化不会建这条链接，真壳里整个插件 import 失败`,
        );
      }
    }
  }

  if (problems.length > 0) {
    fail(
      'builtin-peer-imports',
      `${problems.join('；')}。静态 import 的包登记成 \`PeerSpec::required\`；` +
        '确实该可选就改成动态 \`import()\` 再降级（见 src-tauri/AGENTS.md「peer 分必需与可选」）。',
    );
  } else {
    note('内置插件的 peers 覆盖了 host 静态 import 的每个 @deepseek-ai 包');
  }
}

// ⑳ Windows 上的目录链接只有一个入口，且**不许自己声明 Win32 FFI**（2026-10-19）。
//
// 两条都是「编译不报错、单测也可能绿、坏在真机上」那一类：
//
//   ① `std::os::windows::fs::symlink_dir` 散落在业务模块里，等于绕开了
//      `shell::link::make_dir_link` 的瞬时重试。症状不是「建不出来」，而是**偶发**
//      建不出来——安全软件的过滤器驱动扫描目录时会拒掉 reparse point 建链
//      （ERROR_ACCESS_DENIED），而内嵌插件的 peer 链接建不出来会挡住**整个工作台**
//      （`ensure_wired` → `refresh_enabled` → `AdapterError::Io`）。
//   ② 自己声明 `CreateSymbolicLinkW` 更糟：实测（2026-10-19）裸 extern 声明的那份
//      **返回 TRUE 却什么也没建**——探针里 `rc != 0` 而 `symlink_metadata()` 查不到，
//      是静默故障，比报错更难查。`windows-sys` 那份则返回 ERROR_PRIVILEGE_NOT_HELD。
//
// 因此：**建目录链接一律走 `shell::link::make_dir_link`**；文件链接（技能条目）走
// `skills/manage.rs::make_entry_link`，那是另一条有意走 junction 的路，不归本项管。
{
  const rustSrcDir = join(root, 'src-tauri', 'src');
  // 按**模块名**认这个文件，不写死 `src-tauri/src/...` 路径：判据钉的是「建链
  // 只有一个入口」，不是「入口恰好在这个目录层」。写死路径时，link.rs 一搬家
  // 合法入口自己就会被判成违规，而判据在意的那件事（有没有第二个入口）反而
  // 可能漏判——2026-10-01 重组时逐条改过一次的正是这个形状。
  const linkEntry = baseName('shell/link.rs');
  const strays = [];
  const rawFfi = [];
  for (const full of walk(rustSrcDir, ['.rs'])) {
    const file = full.slice(rustSrcDir.length + 1).split('\\').join('/');
    const code = productionRust(readFileSync(full, 'utf8'));
    const lines = code.split('\n');
    lines.forEach((line, index) => {
      if (baseName(file) !== linkEntry && /\bsymlink_dir\s*\(/.test(line)) {
        strays.push(`${file}:${index + 1}`);
      }
      if (/\bCreateSymbolicLink[AW]?\b/.test(line)) {
        rawFfi.push(`${file}:${index + 1}`);
      }
    });
  }
  if (strays.length > 0) {
    fail(
      'dir-link-entry',
      `${strays.join('、')} 绕过了 ${linkEntry} 直接建目录链接。` +
        '建链失败会经 ensure_wired → refresh_enabled 变成 AdapterError::Io 挡住整个工作台，' +
        '而其中一类是安全软件扫描时的瞬时拒绝——重试就在那个原语里。' +
        '改成 `crate::shell::link::make_dir_link`。',
    );
  } else {
    note(`建目录链接只有 ${linkEntry} 一个入口`);
  }
  if (rawFfi.length > 0) {
    fail(
      'dir-link-entry',
      `${rawFfi.join('、')} 自己声明或调用了 CreateSymbolicLinkW。` +
        '实测裸 extern 声明的那份返回 TRUE 却什么也没建（静默故障），windows-sys 那份返回' +
        ' ERROR_PRIVILEGE_NOT_HELD，std 的封装在同一台机器上 320 次全绿——用 std，' +
        '经 `crate::shell::link::make_dir_link`。',
    );
  }
}

// ㉑ 壳派生的每个子进程都不得继承一个「指向已删除文件」的 Node 预加载（2026-10-20）。
//
// 事故：用户级 `NODE_OPTIONS=--require="C:/Users/me/AppData/Local/node-system-proxy/bootstrap.cjs"`
// 指向的文件已被删除（工具卸载 / 清理脚本），变量没跟着清。Node 在**预加载阶段**
// 就抛 `MODULE_NOT_FOUND` 退出——内核一行代码都没加载，事故面板于是归不出任何
// 插件或内核组件，只能给出「删除当前版本后重新安装」这种**错的**建议（预加载
// 失败发生在内核代码加载之前，内核装得好好的，重装毫无帮助）。
//
// 之所以要有这条判据而不是只靠单测：单测测的是 `sanitize` 这个纯函数，而
// 「每个子进程入口都调用了它」是**接线**——摘掉接线后单测照样全绿，正是判据
// 要挡的那类假绿。
//
// 判据：① `shell/process.rs` 里每个构造子进程 `Command` 的入口都必须调用
// `node_options::apply_to`；② 内核那条路必须继续走 process.rs 的构造器。
//
// ② 之所以也要判：`Command::new` 一样能编译，绕开构造器编译器完全看不出来。
// （模块有没有在 `shell/mod.rs` 里注册、`node_options.rs` 在不在——那两件是
// **编译器**管的事，判据不重复钉：Rust 不允许用没声明的模块。）
{
  const processCode = productionRust(shellSource('process.rs'));
  const entryPoints = [
    'command_with_path',
    'command_with_path_dirs',
    'script_capture',
    'spawn',
  ];
  const missing = [];
  for (const name of entryPoints) {
    const body = rustFunctionBody(processCode, name);
    if (body === null) {
      missing.push(`${name}（找不到函数体——判据要跟着函数改名一起更新）`);
      continue;
    }
    if (!body.includes('node_options::apply_to')) {
      missing.push(name);
    }
  }
  if (missing.length > 0) {
    fail(
      'node-options-preload',
      `${missing.join('、')} 没有调用 node_options::apply_to。` +
        '用户环境里一个指向已删除文件的 NODE_OPTIONS 预加载会让每个 Node 子进程' +
        '（内核、pnpm、npm）在预加载阶段就退出，内核代码一行都没加载——工作台' +
        '起不来而事故面板归不出任何插件或内核组件。',
    );
  } else {
    note('每个子进程入口都清洗了 NODE_OPTIONS 的坏预加载');
  }

  const kernelCode = productionRust(shellSource('kernel_adapter.rs'));
  if (!kernelCode.includes('command_with_path_dirs')) {
    fail(
      'node-options-preload',
      'kernel_adapter.rs 不再走 process::command_with_path_dirs。内核是 node 进程，' +
        '绕开那个构造器就等于绕开了 NODE_OPTIONS 的清洗。',
    );
  }
  if (/Command::new\s*\(/.test(kernelCode)) {
    fail(
      'node-options-preload',
      'kernel_adapter.rs 自己构造了 Command。内核的启动必须经 process.rs 的构造器，' +
        '那里才有 PATH 合并与 NODE_OPTIONS 清洗。',
    );
  }
}

// ㉒ 代理地址只从当下读到的系统配置来：不落盘、不记忆（2026-10-20）。
//
// 旧实现把「上次成功过的代理」存进 `~/.dsh-xlink/last-proxy.txt`，探测不到时
// 把它当兜底路由插回路由表。那是一份**快照**，坏在三处：本地代理客户端换端口
// 后每次请求先白撞一次死端口；那个端口若已被别的进程占用，请求就被送进用户
// 没打算让它走的服务；它还**盖过用户自己的开关**——`NO_PROXY=*` 与「刚把系统
// 代理关掉」在旧实现里都只是"探测不到"，于是记忆地址被插回去。
//
// 撤掉它的理由与当初引入它的理由正好相反：原设计要救"探测间歇失灵"，但抖动
// 期间客户端本身就在重启、端口根本不在线——那正是快照帮不上忙的时刻。
//
// 判据扫**全仓**生产 Rust：记忆机制那两个函数名、以及它落过的那个文件名，
// 都不许再出现（退休清理那一处除外）。单测只盯着 `pkg/net_proxy.rs` 一个文件，
// 新模块再写一份一样的东西它抓不到。
{
  const srcDir = join(root, 'src-tauri', 'src');
  const offenders = [];
  for (const full of walk(srcDir, ['.rs'])) {
    const file = full.slice(srcDir.length + 1).split('\\').join('/');
    const code = productionRust(readFileSync(full, 'utf8'));
    code.split('\n').forEach((line, index) => {
      // 退休清理函数名带 `remembered_proxy_file`，`\b` 不会把它算进去
      // （`y` 与 `_` 之间没有词边界），这里判的是**独立标识符**。
      if (/\bremember(?:ed)?_proxy\b/.test(line)) {
        offenders.push(`${file}:${index + 1} 记忆代理机制已退休（代理地址只从系统配置读）`);
      }
      if (/last-proxy\.txt/.test(line) && !/RETIRED_PROXY_MEMORY_FILE/.test(line)) {
        offenders.push(`${file}:${index + 1} 代理地址不许落盘`);
      }
    });
  }
  if (offenders.length > 0) {
    fail(
      'proxy-address-not-persisted',
      `${offenders.join('、')}。` +
        '代理地址只能来自当下读到的系统配置：本地代理客户端换端口后，快照会让每次请求' +
        '先白撞一次死端口（或被别的进程占了那个端口而被送错地方），而且它会盖过用户的' +
        'NO_PROXY=* 与「刚把系统代理关掉」。用 pkg::net_proxy::routes()，别自己存地址。',
    );
  } else {
    note('代理地址没有落盘、没有记忆：只从当下读到的系统配置来');
  }
}

// ㉓ 测试里改 home 类环境变量必须走共享守卫（2026-10-20）。
//
// 事故：Windows CI 上 `shell::paths::integration_paths_tests::release_and_dev_shells_do_not_overlap`
// 连红两次（run 39346453918 / 39363486523），红的断言是
// `release_settings.starts_with(&home)`。根因不是路径逻辑，是**并发**：
// `xlink_home()` 每次调用都现读 `DSH_XLINK_HOME`，而
// - 有用例**裸 `set_var`/`remove_var`** 改它（`diagnostics/run.rs` 改
//   `HOME` 还不还原、`DSH_XLINK_HOME` 改了再删掉），那段窗口里 env 指向一个
//   文件、或者整个消失；
// - 有用例**不持锁**地读它，于是两次读到的根不是同一个。
//
// 判据扫**全仓** Rust（不是只有生产代码——问题正出在测试里）：除
// `src-tauri/src/lib.rs` 的共享守卫（`crate::tests::EnvGuard`）之外，任何地方
// 都不许裸 `env::set_var` / `env::remove_var` 改 `HOME` / `DSH_HOME` /
// `DSH_XLINK_HOME`。要改就走 `scoped_xlink_home` / `scoped_dsh_home`，它们持
// 进程级锁并在 drop 时还原。
{
  const srcDir = join(root, 'src-tauri', 'src');
  const guardFile = 'lib.rs'; // 共享守卫自己住在这里
  const offenders = [];
  for (const full of walk(srcDir, ['.rs'])) {
    const file = full.slice(srcDir.length + 1).split('\\').join('/');
    if (file === guardFile) continue;
    const code = readFileSync(full, 'utf8');
    const lines = code.split('\n');
    lines.forEach((line, index) => {
      const call = line.match(/env::(?:set|remove)_var\s*\(\s*("[^"]*"|[^,)]*)/);
      if (!call) return;
      // 只看**第一个实参**（环境变量名）：`set_var("HOME", &x)` 的第一个是
      // `"HOME"`，而 `set_var(self.var.name(), value)`（守卫自己）被排除在外。
      if (!/HOME/.test(call[1])) return;
      // 注释里提到这两个函数不算——散文里的 `std::env::set_var` 后面跟的是
      // 中文说明，不会带 HOME。
      offenders.push(`${file}:${index + 1} 裸改 home 类环境变量：${line.trim()}`);
    });
  }
  if (offenders.length > 0) {
    fail(
      'env-home-guard',
      `${offenders.join('、')}。` +
        '改 home 类环境变量（HOME / DSH_HOME / DSH_XLINK_HOME）必须走 ' +
        'crate::tests::scoped_xlink_home / scoped_dsh_home：它们持进程级锁并在 ' +
        'drop 时还原。裸 set_var/remove_var 会在别的用例读路径时留下一段「env ' +
        '指向别处」的窗口，那不是并发噪声——它让 `xlink_home()` 两次解析出不同的根，' +
        '断言于是假红（2026-10-20 Windows CI 连红两次）。',
    );
  } else {
    note('测试里改 home 类环境变量都走共享守卫：没有裸 set_var/remove_var');
  }
}

// ㉔ sha2 摘要的十六进制化必须走 `shell::digest::hex`（2026-10-20）。
//
// 事故：Dependabot PR #9 把 sha2 0.10.9 → 0.11.1，CI 两个 job 全红（run 39366295003）。
// 断点**不在 sha2 的主 API**（`new` / `update` / `digest` 两版一样），而在两处 trait 消失：
//
//   - `format!("{:x}", hasher.finalize())`：0.11 的 `finalize()` 返回
//     `hybrid_array::Array`，它**不再实现 `LowerHex`**（0.10 的 `GenericArray` 实现）；
//   - `io::copy(&mut file, &mut hasher)`：0.11 **不再给摘要器实现 `io::Write`**。
//     这条更阴——它不在摘要那一行，而在「把文件读进 hasher」那行。
//
// 判据扫**生产 Rust**（`productionRust` 剥掉测试块与注释），抓这两种写法。
// 注意 `pkg/releases.rs` 的 **sha1** 摘要、`memory/store.rs` 的随机 id、
// `openai/*` 的 key 字节都不在这两条里——它们不是 sha2 摘要，与 0.11 无关。
{
  const srcDir = join(root, 'src-tauri', 'src');
  const offenders = [];
  for (const full of walk(srcDir, ['.rs'])) {
    const file = full.slice(srcDir.length + 1).split('\\').join('/');
    const code = productionRust(readFileSync(full, 'utf8'));
    code.split('\n').forEach((line, index) => {
      // 摘要值用 `{:x}` 格式化 = 依赖 LowerHex（0.11 没了）。
      if (
        /format!\(\s*"\{:x\}"/.test(line) &&
        /(finalize\(|::digest\()/.test(line)
      ) {
        offenders.push(
          `${file}:${index + 1} 摘要值用了 format!("{:x}", …)（依赖 LowerHex）：${line.trim()}`,
        );
      }
      // 把文件 io::copy 进摘要器 = 依赖摘要器的 io::Write（0.11 没了）。
      if (/io::copy\s*\(/.test(line) && /hasher/.test(line)) {
        offenders.push(
          `${file}:${index + 1} 把文件 io::copy 进摘要器（依赖 io::Write）：${line.trim()}`,
        );
      }
    });
  }
  if (offenders.length > 0) {
    fail(
      'sha2-digest-hex',
      `${offenders.join('、')}。` +
        'sha2 0.11 不再给摘要器实现 `LowerHex`（`format!("{:x}", …)` 编不过）' +
        '也不再实现 `io::Write`（`io::copy(&mut file, &mut hasher)` 编不过）——' +
        '十六进制化统一走 `crate::shell::digest::hex(&digest[..])`，' +
        '读文件算哈希用分块 `read` + `hasher.update`（两版 sha2 都能编，' +
        '输出逐字节相同）。',
    );
  } else {
    note('sha2 摘要的十六进制化都走 shell::digest::hex：没有 LowerHex / io::Write 依赖');
  }
}

// --- 30. 测试/探针不得以用户真实数据目录下的浏览器 profile 起 WebView2 --------

// 事故（2026-10-20 official-chat 首站探测）：探针用
//   --profile "C:\Users\<user>\.dsh-xlink\dsh\desktop-dev\webview-official-chat"
// 起 WebView2，浏览器在**用户真实数据目录**里连续写了两分钟——实测
// `EBWebView\Default\Network\Cookies` 与 `Preferences` 的 mtime 都是 12:46:53，
// 另有 Local Storage / History / IndexedDB / Secure Preferences / Favicons。
// Cookies 是活文件，**写坏了回滚不了**；而「测试永远不许碰用户的真实数据目录」
// 是 AGENTS.md「开发规则」里的红线——口头的「我会注意」管不住。
//
// 判据扫**测试 / 探针**代码（examples / tests / e2e / plugins/*/test），抓两类：
//   ① 出现产品 profile 目录名 `webview-official-chat`——那是官网对话窗口真实的
//      user-data 目录，测试一旦指向它，就是在写用户的浏览器档案；
//   ② 同一行里同时出现「真实家目录标记」与「浏览器 profile 标记」
//      （`.dsh-xlink` + `webview-` / `EBWebView` / `--user-data-dir` /
//      `data_directory`）——路径是拼出来的也算。
//
// **只读路径解析放行**：`join(HOME, '.dsh-xlink', mode, 'desktop', 'active.txt')`
// 是读内核安装树的位置，不是浏览器 profile（memory 插件的探针早就这么干，
// 见 `plugins/memory/test/probe.test.mjs` 里同名的那条纪律）。
// **生产代码不在扫描范围**：壳自己的官网对话窗口本来就用那个 profile
// （`src-tauri/src/commands.rs` 里的 `state.data_dir.join("webview-official-chat")`）。
//
// 逃生阀：确有必要在测试里引用真实路径（例如断言「我们没有碰它」）时，
// 在该行写 `invariant-allow: real-data-dir` 显式放行——放行要留痕，不许匿名。
/**
 * 判据 30 / 31 的扫描范围：**测试 / 探针**代码
 * （examples / tests / e2e / plugins 下各插件的 test 目录）。
 *
 * 范围只有这一处定义——两个判据共用，免得各扫各的、范围悄悄漂开。
 */
function collectProbeScopeFiles() {
  const probeRoots = [
    join(root, 'src-tauri', 'examples'),
    join(root, 'src-tauri', 'tests'),
    join(root, 'e2e'),
  ];
  const pluginRoot = join(root, 'plugins');
  if (existsSync(pluginRoot)) {
    for (const entry of readdirSync(pluginRoot, { withFileTypes: true })) {
      if (!entry.isDirectory()) continue;
      const testDir = join(pluginRoot, entry.name, 'test');
      if (existsSync(testDir)) probeRoots.push(testDir);
    }
  }

  const files = [];
  for (const dir of probeRoots) {
    if (!existsSync(dir)) continue;
    for (const full of walk(dir, ['.rs', '.mjs', '.js', '.cjs', '.ts'])) {
      files.push({ file: show(full), text: readFileSync(full, 'utf8') });
    }
  }
  return files;
}

{
  const files = collectProbeScopeFiles();

  const offenders = findProfileViolationsInFiles(files);
  if (offenders.length > 0) {
    fail(
      'probe-profile-isolation',
      `${offenders.map((o) => `${o.file}:${o.line} ${o.text}`).join('、')}。` +
        '测试/探针绝不能用用户真实数据目录下的 WebView2/Chromium profile 起浏览器：' +
        '浏览器一启动就写 Cookies/Preferences/Local Storage/History/IndexedDB，' +
        '而 Cookies 是活文件、写坏了回滚不了。要复用登录态就**只读拷贝**一份到临时目录' +
        '（Windows 上 Cookies 由 DPAPI 绑机器+用户、不绑路径，拷贝可移植）；' +
        '确有必要引用真实路径时，在该行写 `invariant-allow: real-data-dir` 显式放行。',
    );
  } else {
    note(
      `测试/探针代码没有指向用户真实数据目录下的浏览器 profile（已扫 ${files.length} 个文件）`,
    );
  }
}

// --- 31. 建 webview 必须**无条件**给 data_directory --------------------------
//
// 判据 30 抓的是「写死的真实路径」。但 2026-10-20 13:48 那次写入**没有任何
// 写死的路径**：探针把 profile 传成 `Option<&Path>`，`None` 时**不设**
// `data_directory`，WebView2 于是用**默认**数据目录——
// `%LOCALAPPDATA%\com.zhongxingxing.dsh-xlink\EBWebView`（实测 Cookies-journal
// 与 Local State 被写过）。源码里没有路径字面量，判据 30 抓不到，
// 于是「用默认路径」这个写法一直是个洞。
//
// 这条按**代码形状**判：建 webview 的那一行所在的块里，`data_directory` 必须
// **无条件**生效——一次都没给是违规；「只在 `if let Some(...)` 里给」同样是违规
// （条件不成立时照样落回默认目录）。
// 口径细节（分支链视角）：`if / else` 每个分支都给了 → 每条路径都有目录 → 放行；
// `if / else if` 没有最终 `else` → 还有「条件全不成立」那条隐式路径没给 → 违规。
{
  const files = collectProbeScopeFiles();

  const missing = findDataDirectoryViolationsInFiles(files);
  if (missing.length > 0) {
    fail(
      'probe-data-directory',
      `${missing.map((o) => `${o.file}:${o.line} ${o.detail}`).join('、')}。` +
        '探针建 webview 时必须**无条件**给 `data_directory`（用户数据目录）：' +
        '不给的话 WebView2 会用默认数据目录——那是用户真实安装目录' +
        '（`%LOCALAPPDATA%\\com.zhongxingxing.dsh-xlink\\EBWebView`），' +
        '浏览器一启动就往那里写 Cookies / Preferences / Local Storage / History / IndexedDB，' +
        '而 Cookies 是活文件、写坏了回滚不了。' +
        '要复用登录态就**只读拷贝**一份到 `%TEMP%` 下的一次性目录再指过去。',
    );
  } else {
    note(
      `探针建 webview 时都无条件给了 data_directory，不会落回默认数据目录（已扫 ${files.length} 个文件）`,
    );
  }
}

// --- 32. 官方对话的登录链不许跨出壳，托管弹窗必须继承发起方档案 ------------
//
// **症状**（2026-10-20 实测）：官方网页对话窗口里的 ChatGPT 页签，登录时
// **选不了 Google 邮箱做快速认证**——Google 只给「邮箱或电话号码」表单，
// 不给账号选择器。
//
// **根因（实测，不是推断）**，三件事叠在一起：
//
// (1) ChatGPT 的「使用 Google 账户继续」是 **GSI 按钮**，而它的形态**随 GSI
//     是否就绪而变**（实测）：GSI 就绪前点 ⇒ **同帧**重定向到
//     `accounts.google.com/v3/signin/identifier?...`；GSI 就绪后点 ⇒ 调
//     `window.open` 弹窗（页内钩子记到调用，宿主 `on_new_window` 也收得到——
//     真人点击有用户手势，WebKit 不拦）。修复前那条弹窗**一律交系统浏览器**
//     ⇒ 会话 cookie 落在别的浏览器里 ⇒ 壳内永远拿不到登录态 ⇒ 用户看到
//     「点了 Google，壳里还是没登录」，**壳侧一行日志都没有**（静默失败）。
// (2) 官方对话的内容 webview 用的是**壳专属的浏览器档案**（macOS =
//     `dsh-chat-rel-001` / `dsh-chat-dev-001` 的 WKWebsiteDataStore，Windows =
//     `webview-official-chat` 的 user-data 目录）⇒ 与用户的 Safari / Chrome
//     **永不共享**登录态 ⇒ 首次登录时 Google 侧没有已登录 session ⇒ 账号
//     选择器（快速认证）在壳内**结构上不可达**，只剩「邮箱或电话号码」表单。
// (3) **「嵌入式 webview 能力缺失」这条假设已被实测证伪**：把壳内那份档案带上
//     已登录的 Google session 去跑，`myaccount.google.com` 直接渲染出账号
//     （`zhong izzy / zxx0406@gmail.com`），没有「此浏览器或应用可能不安全」
//     那类拦截 ⇒ 壳的 WKWebView **被 Google 正常接受**。
//
// **判据要钉的是「登录链不被劈成两半」**：普通外链交系统浏览器是对的，但
// **认证域交出去**就会把会话留在别的浏览器里 ⇒ 壳内永远拿不到登录态 ⇒
// 「登录做完了但页签还是未登录」，且**壳侧一行日志都没有**（静默失败）。
// 这条一旦写反，用户侧看到的还是「Google 选不了」，而壳侧一切正常。
//
// 五处形状：
//   ① 官方对话内容页签的 `on_new_window` 必须带 `keep_login_in_shell: true`；
//   ② 同一处必须把**内容页签那份档案**传给托管弹窗（`profile_dir` +
//      `browser_args`）——Windows 上 user-data 目录不同就是两套 cookie jar，
//      同一目录的环境配置还必须完全一致（所以 `additional_browser_args` 也要给）；
//   ③ `popup_route` 必须真的按 `keep_login_in_shell` 分流（否则 ① 是空话）；
//   ④ 同一处必须把 **macOS 的 WKWebsiteDataStore 标识符**也传给托管弹窗——
//      WKWebView **不认** `data_directory`（tauri 的方法文档原话就是
//      「WKWebView 里 data_directory 不可用时用它替代」），只认标识符。不绑
//      ⇒ 弹窗落到**默认**数据存储 ⇒ 里面没有发起方那份 Google session ⇒
//      认证弹窗里照样出不来账号选择器 ⇒ ①②③ 全绿而功能仍然不通；
//   ⑤ 同一处必须给托管弹窗注入**同一份指纹脚本**（`fingerprint: true`）——
//      弹窗与内容页签是同一族，页面探针看到的 `navigator.webdriver` /
//      `__TAURI_*` 在两处必须一致。
{
  const commandsSource = read('src-tauri/src/commands.rs');
  const harnessWindowSource = read('src-tauri/src/harness/harness_window.rs');

  // ① ② ④ ⑤：官方对话内容页签的建窗链（`add_official_chat_tab` 这个函数体）
  const tabStart = commandsSource.indexOf('fn add_official_chat_tab(');
  const tabEnd = commandsSource.indexOf('\nfn ', tabStart + 1);
  const tabSource =
    tabStart >= 0 ? commandsSource.slice(tabStart, tabEnd > 0 ? tabEnd : undefined) : '';

  if (!tabSource) {
    fail(
      'official-chat-login-routing',
      'src-tauri/src/commands.rs 里找不到 `fn add_official_chat_tab(` —— ' +
        '官方对话内容页签的建窗链改名了的话，这条判据要跟着改（它盯的是' +
        '「登录链留在壳内 + 弹窗继承档案」这两件事）。',
    );
  } else {
    if (!/keep_login_in_shell:\s*true/.test(tabSource)) {
      fail(
        'official-chat-login-routing',
        'src-tauri/src/commands.rs 的 `add_official_chat_tab` 里没有 `keep_login_in_shell: true`。' +
          '官方对话的 `window.open` 必须把**认证域**留在壳内：认证链交系统浏览器 ⇒ ' +
          '会话 cookie 落在别的浏览器里 ⇒ 壳内的 webview 永远拿不到登录态 ⇒ ' +
          '「登录做完了但页签还是未登录」，而且壳侧没有日志。',
      );
    }
    if (!/profile_dir:\s*Some\(/.test(tabSource)) {
      fail(
        'official-chat-login-routing',
        'src-tauri/src/commands.rs 的 `add_official_chat_tab` 里没有把内容页签那份档案' +
          '（`profile_dir`）传给托管弹窗。弹窗与页签必须是**同一份**浏览器档案：' +
          'Windows 上 user-data 目录不同 = 两套 cookie jar，弹窗里认证完，页签拿不到登录态。',
      );
    }
    if (!/browser_args:\s*Some\(/.test(tabSource)) {
      fail(
        'official-chat-login-routing',
        'src-tauri/src/commands.rs 的 `add_official_chat_tab` 里没有把 `browser_args` 传给' +
          '托管弹窗。WebView2 要求同一个 user-data 目录下的环境配置**完全一致**，' +
          '少了 `additional_browser_args` 就会在同一个目录上建出不一致的环境。',
      );
    }
    if (!/data_store_identifier:\s*platform_data_store_identifier\(\)/.test(tabSource)) {
      fail(
        'official-chat-login-routing',
        'src-tauri/src/commands.rs 的 `add_official_chat_tab` 里没有用 ' +
          '`platform_data_store_identifier()` 给托管弹窗传 WKWebsiteDataStore 标识符。' +
          '**macOS 上 `data_directory` 是被 WKWebView 忽略的**，认的只有这个标识符：' +
          '不绑 ⇒ 弹窗落到默认数据存储 ⇒ 里面没有发起方那份 Google session ⇒ ' +
          '认证弹窗里出不来账号选择器，而上面几条判据仍然全绿（这正是本条存在的理由）。',
      );
    }
    if (!/fingerprint:\s*true/.test(tabSource)) {
      fail(
        'official-chat-login-routing',
        'src-tauri/src/commands.rs 的 `add_official_chat_tab` 里没有给托管弹窗注入' +
          '指纹脚本（`fingerprint: true`）。弹窗与内容页签是同一族（都是「别人的站点」），' +
          '页面探针看到的 `navigator.webdriver` / `__TAURI_*` 在两处必须一致。',
      );
    }
  }

  // ④ 的跨平台一半：这个标识符**只在 macOS 上有定义**
  // （`OFFICIAL_CHAT_DATA_STORE_IDENTIFIER` 两个分支都是
  // `cfg(all(target_os = "macos", …))`），而**引用它的地方是平台无关的**
  // ⇒ 在那里直接引用那个常量，Windows 侧编译期就报 E0425。
  //
  // 2026-10-21 实测：desktop-ci 的 **Windows job** 抓到了这个（本地 macOS
  // 门禁全绿，覆盖不到）⇒ 跨平台的坏法必须在这里被钉住，不能只靠本地门禁。
  const chatSource = read('src-tauri/src/harness/official_chat.rs');
  if (
    !/fn platform_data_store_identifier\(\)\s*->\s*Option<\[\s*u8;\s*16\s*\]>/.test(
      chatSource,
    )
  ) {
    fail(
      'official-chat-login-routing',
      'src-tauri/src/harness/official_chat.rs 里没有 ' +
        '`platform_data_store_identifier() -> Option<[u8; 16]>`。' +
        '标识符的取值必须走一个**平台无关**的入口：非 macOS 平台没有 ' +
        'WKWebsiteDataStore 这个概念（Windows 靠 user-data 目录）⇒ 那里返回 ' +
        '`None`，而不是去引用一个只在 macOS 上存在的常量。',
    );
  }
  if (!/cfg\(not\(target_os\s*=\s*"macos"\)\)\]\s*\{\s*None\s*\}/.test(chatSource)) {
    fail(
      'official-chat-login-routing',
      'src-tauri/src/harness/official_chat.rs 的 ' +
        '`platform_data_store_identifier()` 在非 macOS 分支上没有返回 `None`。' +
        '非 macOS 平台**没有** WKWebsiteDataStore 标识符这回事 ⇒ 那里必须返回 ' +
        '`None`（弹窗靠 user-data 目录共享档案），否则就是给不存在的东西赋值。',
    );
  }

  // ③：`popup_route` 必须真的按策略分流
  if (!/keep_login_in_shell\s*&&\s*super::official_chat::is_login_domain\(link\)/.test(harnessWindowSource)) {
    fail(
      'official-chat-login-routing',
      'src-tauri/src/harness/harness_window.rs 的 `popup_route` 没有按 `keep_login_in_shell` ' +
        '分流认证域。`PopupPolicy.keep_login_in_shell` 只是字段、不被读的话，' +
        '「认证域留在壳内」就是一句空话。',
    );
  }

  // ④ 的另一半：策略里的标识符必须真的被用上（不能只是字段）。
  if (
    !/data_store_identifier\(identifier\)/.test(harnessWindowSource) &&
    !/builder = builder\.data_store_identifier\(/.test(harnessWindowSource)
  ) {
    fail(
      'official-chat-login-routing',
      'src-tauri/src/harness/harness_window.rs 的 `PopupPolicy.data_store_identifier` ' +
        '没有在建托管弹窗时被用上（`data_store_identifier` 方法调用）。' +
        '字段存在但不被读 ⇒ macOS 上的弹窗仍然落到默认数据存储 ⇒ 认证弹窗里' +
        '没有发起方那份 Google session ⇒ 账号选择器出不来。',
    );
  }

  if (failures.filter((m) => m.startsWith('[official-chat-login-routing]')).length === 0) {
    note(
      '官方对话的登录链留在壳内、托管弹窗继承内容页签那份档案（含 macOS 的 data store 标识符）',
    );
  }
}

// --- 33. 跨语言判据的 Rust 侧产物必须**先构建**，否则 CI 与本地读数不一致 ---
//
// **症状**（2026-10-21 实测）：desktop-ci 的 macOS job 红在
// `scripts/p2-protocol.test.mjs` 的「P2-D 不该是「未执行」：Rust 侧没有产物
// （未构建 `p1_protocol_fixture` example）」。**同一份代码在本地是绿的** ——
// 因为本地 `target/debug/examples/` 里**有**那份产物（之前构建过）。
//
// **根因**：`scripts/lib/rust-fixture-runner.mjs` 的产物路径是**硬编码**的
// （`src-tauri/target/debug/examples/p1_protocol_fixture`），**产物不存在就返回
// `undefined`** ⇒ 调用方按「Rust 侧没产物 ⇒ 本条未执行，不判通过」处理 ⇒ 判据
// 报红。而 **CI 每次都是全新 checkout** ⇒ `target/` 里**没有**那份产物 ⇒ 必红。
//
// ⇒ 修法是**在跑判据之前把产物构建出来**，而不是在判据里就地构建（判据要只读、
// 且构建很慢）。**这条纪律同时约束本地与 CI**（都在 `check:js` 这条链上）⇒
// 「本地绿、CI 红」这一类最贵的偏差被消掉。
//
// 判据钉两处形状：
//   ① `package.json` 的 `build:protocol-fixture` 脚本必须构建**那个** example
//      （且用 **debug** profile —— 判据找的是 `target/debug/examples/`，release
//      的产物落在 `target/release/examples/` ⇒ 对不上）；
//   ② `check:js` 链里这一步必须在 `test:scripts` **之前**（跑判据前才有产物）。
{
  const pkg = JSON.parse(read('package.json'));
  const scripts = pkg.scripts ?? {};
  const buildFixture = scripts['build:protocol-fixture'] ?? '';

  if (!/--example\s+p1_protocol_fixture/.test(buildFixture)) {
    fail(
      'cross-language-fixture-prebuilt',
      'package.json 里没有能构建 `p1_protocol_fixture` example 的 `build:protocol-fixture` 脚本' +
        '（或那个脚本没带 `--example p1_protocol_fixture`）。跨语言判据（P1 A1c / P2-D）' +
        '读的 Rust 侧产物路径是**硬编码**的 `src-tauri/target/debug/examples/p1_protocol_fixture`，' +
        '产物不存在 ⇒ 判据按「Rust 侧没产物 ⇒ 未执行」报红 ⇒ **CI 必红**（全新 checkout 没有 target/）。',
    );
  }
  if (/--release/.test(buildFixture) || /--profile\s+release/.test(buildFixture)) {
    fail(
      'cross-language-fixture-prebuilt',
      '`build:protocol-fixture` 用了 **release** profile。判据找的是 ' +
        '`target/debug/examples/`，release 的产物落在 `target/release/examples/` ⇒ 对不上。' +
        '（cargo 默认就是 debug profile，这里**不需要**显式加 `--profile dev`。）',
    );
  }

  const checkJs = scripts['check:js'] ?? '';
  const fixtureAt = checkJs.indexOf('build:protocol-fixture');
  const testAt = checkJs.indexOf('test:scripts');
  if (fixtureAt < 0) {
    fail(
      'cross-language-fixture-prebuilt',
      'package.json 的 `check:js` 链里**没有** `build:protocol-fixture` 这一步。' +
        '跨语言判据要读 Rust 侧产物，而 CI 是全新 checkout ⇒ 不先构建就必红。',
    );
  } else if (testAt >= 0 && fixtureAt > testAt) {
    fail(
      'cross-language-fixture-prebuilt',
      'package.json 的 `check:js` 链里 `build:protocol-fixture` 排在 `test:scripts` **之后**。' +
        '产物必须在**跑判据之前**构建出来，否则判据读到的还是「没有产物」。',
    );
  }

  if (failures.filter((m) => m.startsWith('[cross-language-fixture-prebuilt]')).length === 0) {
    note('跨语言判据的 Rust 侧产物在跑判据前构建（本地与 CI 同一份读数）');
  }
}

// --- 34. 官网网页版顶条：官方内容页不再注入品牌线脚本，两处蓝同值 ---------
//
// **2026-10-24 用户拍板「移除下方的彩色条」**：那条 3px 品牌线原先是**注入到
// 内容页**的（`titlebar-pulse.js`），官方那条走 DeepSeek 蓝。现在它整体退役
// ——内容页是**别人的页面**，那条线本来就盖在人家的页头上；品牌色改由**页签栏
// 的文字**承担（Rust 侧 `OFFICIAL_CHAT_BRAND_COLORS` → `official_chat_tabs`
// → 页签栏）。`titlebar-pulse.js` 只剩工作台用（Gitea 绿渐变）。
//
// 三件不会报错、只会「看起来不对」的事：
//
//   ① 官方内容页**又把**这条注入挂回去 ⇒ 页签栏下沿那条彩色条又回来了
//      （正是用户要移除的那条）。2026-10-23 删过一次（改画「页签栏里的品牌
//      带」），用户回来说「不要这个额外增加的下面这一条」⇒ 回退；2026-10-24
//      用户改口要「移除下方的彩色条」⇒ 这次是**彻底不画**，注入不许回来。
//   ② 同一条注入脚本在同一个 webview 上挂两遍 ⇒ 它跑两遍。指纹脚本会被打
//      两遍补丁，而**没有任何一行会报错**（2026-10-23 改上面那条注入时手滑
//      就多挂了一遍 `chat-fingerprint.js`——review 发现的，判据当时不存在）。
//   ③ DeepSeek 站色在品牌色表与拉绳小台灯里分成两个数 ⇒ 页签栏里出现两种蓝。
//
// 「工作台那条线两端是渐变淡出」这条形状由
// `scripts/titlebar-pulse.test.mjs` 钉住——它把脚本在 VM 里跑一遍读生成的 CSS
// （判据扫源码里的形状会随写法漂移）。
{
  const commandsSrc = productionRust(read('src-tauri/src/commands.rs'));
  const builder = rustFunctionBody(commandsSrc, 'add_official_chat_tab');
  if (builder === null) {
    fail('official-chat-topline', 'commands.rs 里找不到 add_official_chat_tab 的函数体，判据失效');
  } else {
    // ① 官方内容页**不许**再注入 titlebar-pulse.js。
    //
    // 2026-10-24 用户拍板「移除下方的彩色条」：此前这条注入给官方内容页顶端
    // 画了一条 3px 品牌线，而内容页是**别人的页面**，那条线本来就盖在人家的
    // 页头上。品牌色改由页签栏的文字承担。判据因此从「必须注入」反转成
    // 「不许注入」——留着它就是那条线又回来了。
    if (builder.includes('titlebar-pulse')) {
      fail(
        'official-chat-topline',
        'add_official_chat_tab 还在注入 titlebar-pulse.js——官方内容页顶端那条 3px 彩色条会回来' +
          '（2026-10-24 用户拍板移除；品牌色已改由页签栏文字承担）'
      );
    }
    // ② 同一条注入脚本不挂两遍。
    const injected = [
      ...builder.matchAll(/initialization_script\(include_str!\("([^"]+)"\)\)/g),
    ].map((m) => m[1]);
    const dup = [...new Set(injected.filter((s, i) => injected.indexOf(s) !== i))];
    if (dup.length > 0) {
      fail(
        'official-chat-topline',
        `官方内容 webview 上同一条注入脚本挂了两遍：${dup.join('、')}——它会跑两遍` +
          '（指纹脚本打两遍补丁），而编译与测试全绿'
      );
    }
  }

  // ③ 台灯绳色 == DeepSeek 站的品牌色。
  //
  // **不是「所有官方站一个色」**：三站三色（DeepSeek 蓝、MiniMax 粉、ChatGPT
  // 白档）之后，台灯是**页签栏自己的固定 chrome**——它挂在 strip webview 上，
  // 那个页面里没有「当前激活的是哪个站」的状态（切页签只换内容 webview，不
  // 通知 strip），所以灯的颜色锚在 DeepSeek 蓝上：它是官网那族里最早确定的
  // 一个值，页签栏整体读作「官网蓝」。这一项钉的是「DeepSeek 站色 == 灯绳色」，
  // 两端同值就不会在页签栏里出现两种蓝。
  //
  // 事实源在 Rust（`official_chat::OFFICIAL_CHAT_BRAND_COLORS`，2026-10-24 起
  // 品牌色从注入脚本搬回来）。
  const launcherSrc = shellSource('pullstring-launcher.js');
  const pulseDeepseek = read('src-tauri/src/harness/official_chat.rs').match(
    /\("DeepSeek",\s*"(#[0-9a-fA-F]{3,8})"\)/
  );
  const cordOfficial = launcherSrc.match(/isOfficial\s*\?\s*\{([\s\S]*?)\}/);
  const cord = cordOfficial
    ? (cordOfficial[1].match(/cord\s*:\s*"(#[0-9a-fA-F]{3,8})"/) || [])[1]
    : null;
  if (!pulseDeepseek) {
    fail('official-chat-topline', 'official_chat.rs 的品牌色表里找不到 DeepSeek 站，判据失效');
  } else if (!cord) {
    fail('official-chat-topline', 'pullstring-launcher.js 里找不到官方分支的绳色，判据失效');
  } else if (pulseDeepseek[1].toLowerCase() !== cord.toLowerCase()) {
    fail(
      'official-chat-topline',
      `DeepSeek 站色与台灯绳色不一致：品牌色 ${pulseDeepseek[1]} vs 台灯绳色 ${cord}——页签栏里会出现两种蓝`
    );
  }

  if (failures.filter((m) => m.startsWith('[official-chat-topline]')).length === 0) {
    note('官网顶条：官方内容页不再注入品牌线脚本，DeepSeek 站色与台灯绳色同值');
  }
}

// --- 第 35 项：官方对话「切页签」的可见性时序 -------------------------------
//
// 2026-10-24 用户反馈：「首次打开后点击 tab 切换，网页显示会延迟很久才加载
// 出来」。取证结论是三层叠加，每一层单独存在都不致命，合起来就���用户读作
// 「卡死了」：
//
//   ① 内容 webview **按需挂载**：原先点 tab 才第一次建 webview 并现场导航，
//      用户等的是整个站点首屏（实测 MiniMax 首屏要拉约 80 个 JS chunk +
//      33 个 CSS + 55 张图，且入口还会 302 跨到 minimax.cn）。
//   ② **新建的 webview 默认可见**（wry 的 `WebViewBuilder` 默认
//      `visible: true`，tauri 没覆盖它），它会在自己加载完成之前就盖在旧
//      页面上——用户看到的是「旧页瞬间消失 → 白屏 → 几秒后新页面才出来」。
//   ③ 加载期**零反馈**：页签栏既不转圈也不置灰。
//
// 修法（P0+P1+P2，2026-10-24 用户拍板）：
//   · 新建的 webview **立即隐藏**，显示权交给状态机；
//   · 目标页签**未就绪时不切**，旧页面继续显示，等它 page-load 完成再切；
//   · 开窗后台**预热**其余页签，首次点击几乎无等待；
//   · 后端随页签下发 `loading`，页签栏据此显示加载指示。
//
// 这一项钉住那四件事不许回退：它们都不会报错、不会让编译失败，只会「看起来
// 不对」。状态机本身的行为（未就绪不切、先加载完的页签不抢页面）由
// `harness::official_chat` 的单测钉住，这里钉的是「接线还在」。
{
  const commandsSrc = productionRust(read('src-tauri/src/commands.rs'));
  // 判据扫的是源码文本，**必须先剥注释**——本文件自己的说明文字里就写着
  // `.show()` / `.hide()`，不剥会把说明当成命中。
  const body = (name) => {
    const raw = rustFunctionBody(commandsSrc, name);
    return raw === null ? null : stripComments(raw).join('\n');
  };

  // ① 新建的内容 webview 必须立即隐藏。
  const addTab = body('add_official_chat_tab');
  if (addTab === null) {
    fail('official-chat-tab-visibility', 'commands.rs 里找不到 add_official_chat_tab 的函数体，判据失效');
  } else if (!/\.hide\(\)/.test(addTab)) {
    fail(
      'official-chat-tab-visibility',
      'add_official_chat_tab 没有隐藏新建的 webview——它默认可见（wry 的 WebViewBuilder 默认 visible: true），' +
        '会在页面加载完成前盖住旧页面，用户看到的是「旧页瞬间消失 → 白屏」'
    );
  }

  // ② 切换路径不许自己 show：显示权只走状态机。
  const switchTab = body('switch_official_chat_tab_blocking');
  if (switchTab === null) {
    fail(
      'official-chat-tab-visibility',
      'commands.rs 里找不到 switch_official_chat_tab_blocking 的函数体，判据失效'
    );
  } else {
    if (!/request_switch\(/.test(switchTab)) {
      fail(
        'official-chat-tab-visibility',
        'switch_official_chat_tab_blocking 没有走状态机（request_switch）——目标页签未就绪时不该切，' +
          '否则新 webview 会在加载完成前顶掉旧页面'
      );
    }
    if (/\.show\(\)/.test(switchTab)) {
      fail(
        'official-chat-tab-visibility',
        'switch_official_chat_tab_blocking 自己调用了 show()——显示权只走 show_official_chat_tab，' +
          '由状态机决定「谁该被看见」'
      );
    }
  }

  // ③ 开窗后台预热其余页签。
  const openChat = body('open_official_chat');
  if (openChat === null) {
    fail('official-chat-tab-visibility', 'commands.rs 里找不到 open_official_chat 的函数体，判据失效');
  } else if (
    !openChat.includes('dsh-warm-official-chat-tabs') ||
    !openChat.includes('ensure_official_chat_tab') ||
    !/\.skip\(1\)/.test(openChat)
  ) {
    fail(
      'official-chat-tab-visibility',
      'open_official_chat 没有后台预热**其余**页签（预热线程 + ensure_official_chat_tab + 跳过激活页签）——' +
        '用户第一次点某个页签时要等整个站点首屏'
    );
  } else if (!/OfficialChatRuntime::new\(0\)/.test(openChat)) {
    // 上一次窗口的 `active` 属于**已经销毁的 webview**（窗口可能被系统 chrome
    // 关闭按钮拆解过）。不重置的话，重开窗口会显示错页签。
    fail(
      'official-chat-tab-visibility',
      'open_official_chat 没有重置运行时状态（OfficialChatRuntime::new(0)）——上一次窗口的 active 指向已销毁的 webview，' +
        '重开窗口会显示错页签'
    );
  }

  // ④ 后端必须下发 loading 态（值取自状态机的 is_loading）。
  const tabsCmd = body('official_chat_tabs');
  if (tabsCmd === null) {
    fail('official-chat-tab-visibility', 'commands.rs 里找不到 official_chat_tabs 的函数体，判据失效');
  } else if (!/is_loading\(/.test(tabsCmd)) {
    fail(
      'official-chat-tab-visibility',
      'official_chat_tabs 没有按状态机的 is_loading 下发 loading 态——页签栏无从区分「在加载」和「卡死了」'
    );
  }

  // ⑤ 页签栏必须真的渲染加载指示。
  //
  // **只查模板段**：CSS 里也有 `.chat-tab-spinner` 这条规则，删掉模板里的
  // 那个元素而留着样式，全文匹配照样命中——反向验实测过这条漏报。
  const tabsVue = read('ui/src/official-chat/OfficialChatTabs.vue');
  const templateStart = tabsVue.indexOf('<template>');
  const templateEnd = tabsVue.indexOf('</template>');
  const template = templateStart >= 0 && templateEnd > templateStart
    ? tabsVue.slice(templateStart, templateEnd)
    : '';
  if (!template) {
    fail('official-chat-tab-visibility', 'OfficialChatTabs.vue 里找不到 <template> 段，判据失效');
  } else if (!/v-if="tab\.loading"/.test(template) || !/chat-tab-spinner/.test(template)) {
    fail(
      'official-chat-tab-visibility',
      'OfficialChatTabs.vue 的模板里没有按 loading 态渲染加载指示（v-if="tab.loading" + chat-tab-spinner）——' +
        '加载期不给反馈时，用户读到的是「卡死了」'
    );
  }

  if (failures.filter((m) => m.startsWith('[official-chat-tab-visibility]')).length === 0) {
    note('官网页签可见性：新建 webview 立即隐藏、未就绪不切、开窗预热、加载态有指示');
  }
}

// --- 第 36 项：启动时后台预启动的接线 -----------------------------------------
//
// `shell/prestart.rs` 里的 `should_prestart` / `prewarm_tree` 都是可单测的
// 纯函数，单测能全绿而生产上一行都没接：症状只是「点『启动工作台』还是要等
// 冷启动」——不报错、不崩溃，没人会注意到缓解没生效。因此这里钉接线形状：
// ① setup 里恰好派一次线程；② 退出路径置位退出标记；③ `spawn` 必须真的把
// `run` 交给后台线程（不能在 setup 里同步跑）；④ `run` 必须走判据、预热、
// 启动主体、事件日志与退出标记；⑤ 开关命令必须真的写设置项。
{
  const libSrc = productionRust(read('src-tauri/src/lib.rs'));
  const prestartSrc = productionRust(shellSource('prestart.rs'));
  const autostartSrc = productionRust(shellSource('autostart.rs'));

  const spawnCall = 'shell::prestart::spawn(app.handle());';
  if (libSrc.split(spawnCall).length - 1 !== 1) {
    fail(
      'prestart-wiring',
      `lib.rs 的 setup 里必须恰好调用一次 \`${spawnCall}\`——没有它，预热与后台启动只是零调用的库函数，` +
        '用户点「启动工作台」仍要等冷启动，而 cargo test 全绿'
    );
  }
  if (!libSrc.includes('shell::prestart::mark_shutting_down();')) {
    fail(
      'prestart-wiring',
      'lib.rs 的 RunEvent::Exit 没有调用 prestart::mark_shutting_down()——用户在预热期间退出时，' +
        '后台线程会在退出清理之后再派生一个占端口的孤儿内核'
    );
  }

  const spawnBody = rustFunctionBody(prestartSrc, 'spawn');
  if (
    spawnBody === null ||
    !spawnBody.includes('std::thread::Builder::new()') ||
    !spawnBody.includes('run(')
  ) {
    fail(
      'prestart-wiring',
      'prestart::spawn 没有把 run 丢进 std::thread::Builder 线程——它会在 setup 里同步跑，启动被阻塞'
    );
  }

  const runBody = rustFunctionBody(prestartSrc, 'run');
  if (runBody === null) {
    fail('prestart-wiring', 'prestart.rs 里找不到 run 的函数体，判据失效');
  } else {
    for (const needle of [
      'should_prestart(',
      'prewarm_tree(',
      'start_kernel_blocking(',
      'shell_events::record(',
      'SHELL_SHUTTING_DOWN.load(',
    ]) {
      if (!runBody.includes(needle)) {
        fail(
          'prestart-wiring',
          `prestart::run 缺了 ${needle}——判据、预热、启动主体、事件日志或退出标记的接线被摘掉了`
        );
      }
    }
  }

  const setBody = rustFunctionBody(autostartSrc, 'prestart_set_kernel');
  if (setBody === null || !setBody.includes('prestart_kernel = Some(enabled)')) {
    fail(
      'prestart-wiring',
      'prestart_set_kernel 没有写 `prestart_kernel = Some(enabled)`——面板开关会显示保存成功但设置不生效'
    );
  }

  if (failures.filter((m) => m.startsWith('[prestart-wiring]')).length === 0) {
    note('启动时后台预启动：setup 派线程、判据/预热/启动主体、退出标记与开关写入都在位');
  }
}

// --- 结果 --------------------------------------------------------------------

for (const message of notes) console.log(`✓ ${message}`);
if (failures.length > 0) {
  console.error('');
  for (const message of failures) console.error(`✗ ${message}`);
  console.error(`\n${failures.length} 项不变量检查失败。`);
  process.exit(1);
}
console.log('\n全部不变量检查通过。');
