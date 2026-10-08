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
 *   4. 内置补丁清单结构有效（与 patches.rs 的 validate_def 对齐，并额外保证
 *      copy 模式的 `from` 文件确实存在于仓库里）；
 *   5. UI 模板里的绑定都能解析（委托 `scripts/check-ui-bindings.mjs`）；
 *   6. 管理窗口的无边框来自 `tauri.conf.json`（不是运行时 `set_decorations`），
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
 */

import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs';
import { dirname, isAbsolute, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

import { shellSourcePath, shellSourceLabel } from './lib/shell-source.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const failures = [];
const notes = [];

const fail = (section, message) => failures.push(`[${section}] ${message}`);

/// 补丁清单里的路径必须是"补丁目录 / 内核目录内的普通相对路径"。
///
/// 与 Rust 的 `patches::check_target_path` 对齐：拒绝空路径、绝对路径、Windows
/// 盘符（`C:…`）、UNC（`\\\\server\\share`）以及任何 `..` 段。判据比"存在性
/// 检查"更早生效，坏清单在 CI 就报出来，而不是等到运行时才被 Rust 拒绝。
const isSafeRelativePath = (raw) => {
  const value = String(raw ?? '');
  if (!value) return false;
  if (isAbsolute(value) || /^[A-Za-z]:/.test(value) || value.startsWith('\\\\')) return false;
  const segments = value.split(/[\\/]/);
  return segments.length > 0 && segments.every((segment) => segment !== '' && segment !== '.' && segment !== '..');
};
const note = (message) => notes.push(message);
const read = (rel) => readFileSync(join(root, rel), 'utf8');
const readJson = (rel) => JSON.parse(read(rel));

// 单文件判据一律按 basename 定位，不写死 `src-tauri/src/...` 完整路径。理由、
// 踩过的坑与「为什么重名要抛错而不是猜」写在 lib/shell-source.mjs 的文件头；
// 门禁脚本与 UI 测试共用这一份实现——两套解析器迟早只修得上一套。
const shellSource = (fileName) => readFileSync(shellSourcePath(fileName), 'utf8');

/** 相对路径的 basename：`kernel/lifecycle.rs` → `lifecycle.rs`。 */
const baseName = (relPath) => relPath.split(sep).pop();

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

// --- 4. 内置补丁清单 ---------------------------------------------------------

const patchRoot = join(root, 'src-tauri/resources/patches');
const seenPatchIds = new Set();
const patchDirs = existsSync(patchRoot)
  ? readdirSync(patchRoot, { withFileTypes: true }).filter((e) => e.isDirectory())
  : [];
if (patchDirs.length === 0) {
  fail('patches', `没有找到任何内置补丁目录：${show(patchRoot)}`);
}
for (const dir of patchDirs) {
  const patchDir = join(patchRoot, dir.name);
  const manifestPath = join(patchDir, 'manifest.json');
  const label = show(manifestPath);
  if (!existsSync(manifestPath)) {
    fail('patches', `${label} 缺失`);
    continue;
  }
  let manifest;
  try {
    manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
  } catch (error) {
    fail('patches', `${label} 不是合法 JSON：${error.message}`);
    continue;
  }
  if (manifest.schemaVersion !== 1) {
    fail('patches', `${label} 的 schemaVersion 不是 1：${manifest.schemaVersion}`);
  }
  if (!Array.isArray(manifest.patches) || manifest.patches.length === 0) {
    fail('patches', `${label} 的 patches 为空`);
    continue;
  }
  for (const def of manifest.patches) {
    const id = def.id ?? '<无 id>';
    if (!def.id) fail('patches', `${label} 有定义缺少 id`);
    if (typeof def.id === 'string' && /[\\/]/.test(def.id)) {
      fail('patches', `${label} 的 id 含路径分隔符：${def.id}`);
    }
    if (seenPatchIds.has(def.id)) fail('patches', `补丁 id 跨清单重复：${def.id}`);
    seenPatchIds.add(def.id);
    if (!Array.isArray(def.files) || def.files.length === 0) {
      fail('patches', `${label} 的 ${id} 没有 files`);
      continue;
    }
    for (const entry of def.files) {
      const target = entry.to ?? '';
      if (!isSafeRelativePath(target)) {
        fail('patches', `${label} 的 ${id} 有非法目标路径：${JSON.stringify(target)}`);
      }
      if (entry.mode === 'copy') {
        const from = entry.from;
        if (!from) {
          fail('patches', `${label} 的 ${id} copy 模式缺少 from（目标 ${target}）`);
        } else if (!isSafeRelativePath(from)) {
          // 与 to 同一判据（Rust 侧 `validate_def` 对 from 也调 check_target_path）：
          // 缺了它，`from: "../../../../etc/passwd"` 或绝对路径能过 CI 门禁，
          // 审计价值直接归零（P2-18）。
          fail('patches', `${label} 的 ${id} 的 from 非法（${JSON.stringify(from)}）：只接受补丁目录内的相对路径`);
        } else if (!existsSync(join(patchDir, from))) {
          fail('patches', `${label} 的 ${id} 引用的载荷不存在：${from}`);
        }
        if (entry.expectSha256 !== undefined) {
          const hex = String(entry.expectSha256).trim().toLowerCase();
          if (!/^[0-9a-f]{64}$/.test(hex)) {
            fail('patches', `${label} 的 ${id} 的 expectSha256 不是 64 位十六进制`);
          }
        }
      } else if (entry.mode === 'replace') {
        if (!entry.search) {
          fail('patches', `${label} 的 ${id} replace 模式缺少 search（目标 ${target}）`);
        }
        if (entry.replacement === undefined) {
          fail('patches', `${label} 的 ${id} replace 模式缺少 replacement（目标 ${target}）`);
        }
      } else {
        fail('patches', `${label} 的 ${id} 使用了不支持的模式：${entry.mode}`);
      }
    }
  }
}
note(`内置补丁清单有效：${seenPatchIds.size} 个补丁定义`);

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

// --- 5. UI 模板绑定 ----------------------------------------------------------
//
// 模板里引用了既不在 `<script setup>` 绑定、也没有全局注册的标识符时，生产构建
// 不报错，绑定被静默求值成 `undefined`（`:disabled` / `:loading` 变成永不生效），
// 而全部 Rust 与 UI 测试都是绿的。判据由 `scripts/check-ui-bindings.mjs` 提供。
{
  const { checkProject } = await import('./check-ui-bindings.mjs');
  const { checked, skipped, failures: uiFailures } = await checkProject(join(root, 'ui/src'));
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

// --- 8.5 icon-only 按钮必须有无障碍名称 -------------------------------------
//
// 事故来源：code-review-2026-09-27 的 L3。`circle` + `:icon` 的按钮**对屏幕
// ��读器是空的**——可访问名只能来自文本、`aria-label` 或 `aria-labelledby`，
// 而图标本身不是文本。外层 `el-tooltip` 只在鼠标悬停时才出现，键盘与读屏
// 用户永远看不到它，于是这些按钮等于不存在。八个按钮全是插件/技能的更新、
// 打开仓库、卸载这类**唯一的**操作入口。
//
// 这类缺陷没有任何构建期信号：Vue 不校验 aria 属性，test:ui 也钉不住「一个
// 按钮该叫什么」。所以钉进门禁。判据取「`circle` 且有 `:icon`」这一形状——
// 本仓库的 icon-only 按钮一律同时带这两个属性，带了 `circle` 就是视觉上只剩
// 图标；带 `aria-label` 或可见文本（插槽文本）即通过。
{
  const missingAria = [];
  for (const file of walk(join(root, 'ui/src'), ['.vue'])) {
    const text = readFileSync(file, 'utf8');
    // 按 `<el-button` 起、到配对的 `>` 止取属性块。自闭合的 `<el-button … />`
    // 同样覆盖（`circle` 的用法全是自闭合）。
    for (const match of text.matchAll(/<el-button\b([\s\S]*?)\/>/g)) {
      const attrs = match[1];
      if (!/\bcircle\b/.test(attrs)) continue;
      if (!/:icon=|v-bind:icon=/.test(attrs)) continue;
      if (/\baria-label(?:ledby)?=/.test(attrs)) continue;
      const line = text.slice(0, match.index).split('\n').length;
      missingAria.push(`${relative(root, file).split(sep).join('/')}:${line}`);
    }
  }
  if (missingAria.length > 0) {
    for (const where of missingAria) {
      fail(
        'a11y-icon-button',
        `${where} 是只有图标的按钮，却没有 aria-label —— 屏幕阅读器读不出它是什么，` +
          '鼠标用户看得见的 tooltip 对键盘与读屏用户不存在。补 :aria-label="\'动作 \' + 实体名"，' +
          '照同一文件里 el-switch :aria-label 的写法。',
      );
    }
  } else {
    note('icon-only 按钮都带 aria-label');
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
  // 阶梯（--z-*）按审查意见 P1-01 必须写在一处，而 theme.css 恰在反棘轮上限
  // （3001/3001）上一行都加不了——那份阶梯于是落进 diagnostics.css。
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
  // 这份指针曾��� `default_family()` 解析 `data_dir` 的输入，而 `data_dir` 装的是
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
  //    只查 kernel.rs：`patches::ensure_workbench_stopped` 是**另一个**同名函数
  //    （补丁应用 / 撤销的停机守卫），与跨壳判据无关，别把它算进来。
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
          '那个壳点「刷新工作台」——为它硬拦住整个动作，等于废掉双壳并行。',
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
    // （commands.rs 是反棘轮文件，不能为了留一份副本多写三行）。`diagnose_runtime`
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
    const harnessScripts = ['harness/harness-draft.js', 'harness/harness-health.js'].map((name) => ({
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
    for (const file of rustFilesWithScripts) {
      const lines = productionRust(readFileSync(join(srcDir, file), 'utf8')).split('\n');
      lines.forEach((line, index) => {
        if (!line.includes('harness/harness-health.js')) return;
        const window = lines.slice(Math.max(0, index - 6), index + 7).join('\n');
        if (!window.includes('harness/harness-draft.js')) {
          draftMisses.push(`${file}:${index + 1} 注入了 harness-health.js 却没注入 harness-draft.js`);
        }
      });
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

// --- 16. 三个窗口族都禁用右键菜单，且不许连左键复制一起禁 --------------------
//
// 需求只有一句话（「主界面、工作台、官方对话窗口都禁用鼠标右键菜单，保留左键
// 复制」），落点却是三处：壳自己的五个窗口共用 SPA 入口（`ui/src/
// noContextMenu.js`，由 `main.js` 装一次）、工作台、官方对话三个内容 webview
// （Rust 侧注入 `no-context-menu.js`）。**行为测试钉不住接线**：测试直接对着
// 脚本跑，把某一处接线删掉它照样全绿，而症状只是「从某个入口打开的窗口右键
// 菜单还在」——不报任何错。因此这里钉三条：
//
//   ① **每一条加载外部页面的建窗链**都必须注入 `no-context-menu.js`。按
//      `WebviewUrl::External(` 所在的顶层函数去找，而不是按文件数出现次数：
//      工作台有**两条**建窗链（`commands.rs::open_harness` 是用户点「启动工作台」
//      走的那条，`harness_window::build` 是自愈链条 / 手动刷新走的那条），
//      漏一条的症状正是「有时右键菜单还在」，而按次数计的判据恰好抓不住这种
//      漏法。将来新增远程窗口同样会被这条拦住。
//   ② `main.js` 必须 import 并调用 `disableContextMenu()`，且调用在
//      `app.mount` 之前（晚一步会有一段窗口期菜单还在）。
//   ③ 两份实现都不许出现 `user-select` 或对 `copy` / `select` / `mousedown`
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
      // 建窗链的边界取「所在顶层函数的收尾大括号」：链本身跨十几行，而函数
      // 体内的缩进代码不会在第 0 列出现 `}`。
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
        body: lines.slice(index, end).join('\n'),
      });
    });
  }
  // `include_str!` 的相对路径是**相对所在文件**解析的：commands.rs 在 src/
  // 根，写 `harness/no-context-menu.js`；harness_window.rs 与它同在 harness/，
  // 写 `no-context-menu.js`。两条都指同一份文件，所以这里只认 basename——
  // 写死任一侧的完整路径，另一侧就会以「这条建窗链没注入」变红，而注入好好的。
  const unwired = remoteChains.filter((chain) => !chain.body.includes('no-context-menu.js'));
  if (remoteChains.length === 0) {
    fail(
      'no-context-menu',
      '生产代码里找不到任何 WebviewUrl::External 的建窗链——判据本身失效了，' +
        '先确认这条检查还能匹配到建窗代码，再谈它绿不绿。',
    );
  } else if (unwired.length > 0) {
    for (const chain of unwired) {
      fail(
        'no-context-menu',
        `${chain.file}:${chain.line} 这条加载外部页面的建窗链没有注入 no-context-menu.js——` +
          `右键菜单会从**这个入口打开的窗口**里照常弹出来（工作台有 open_harness 与 ` +
          `harness_window::build 两条链，两条都要接；官方对话的每个内容 webview 走 ` +
          `add_official_chat_tab）。本地 SPA 窗口不在此列：它们由 ui/src/noContextMenu.js 负责。`,
      );
    }
  } else {
    note(`${remoteChains.length} 条远程建窗链都注入了禁右键脚本（${remoteChains.map((c) => `${c.file}:${c.line}`).join('、')}）`);
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
  // 判据前先剥注释，理由与上面 `productionRust` 相同：两份实现里都写着
  // 「刻意**不**写 user-select」，照字面查会把这句话本身当成违规——第一版
  // 就是这么误报的。`//` 前是 `:` 时不当作行注释（`https://…` 这类 URL）。
  const stripJsComments = (text) =>
    text
      .replace(/\/\*[\s\S]*?\*\//g, '')
      .split('\n')
      .map((line) => line.replace(/(^|[^:])\/\/.*$/, '$1'))
      .join('\n');
  // 两份实现各自按自己的规则定位：壳侧注入脚本按文件名在 src-tauri/src 下找
  // （它 2026-10-01 搬进了 harness/），面板侧在 ui/src 下且路径稳定。
  const implementations = [
    { file: shellSourceLabel('no-context-menu.js'), source: shellSource('no-context-menu.js') },
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
    note('两份实现都只取消 contextmenu，没碰选中与复制');
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

  if (rawHides.length === 0 && autostartIdx >= 0 && reopenIdx >= 0) {
    note('常驻接线完整：自启收起走 hide_to_shell，Reopen 兜底 show_main_shell');
  }
}

// --- 18. 窗口标题与标题栏主题：一份名字 + 全员钉深色 ----------------------
//
// 2026-10-07 用户截图：主窗口标题栏深色，副窗（套餐用量）标题栏**浅色**——
// 系统切浅色时原生标题栏跟着变白，而壳内内容恒为深色。两处成因各自独立：
//   · 全仓 `prefers-color-scheme` 零命中，`set_theme` 从来没被调用过，
//     所以每一扇走原生装饰的窗口都在跟随系统；
//   · 窗口标题是六处各自写死的字面量，主窗口那份还硬编码在 Vue 组件里。
// 两条都只有真机窗口标题栏看得出来，单测与 UI 测试都够不着，所以放这里。

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
    // ② 主窗口必须钉深色：**它是建窗瞬间的兜底值**，不是最终外观。
    //    自绘标题栏的底色走 `--chrome` token，跟着应用主题走；页面挂载前
    //    `applyTheme()` 会调 `window.setTheme()` 把原生 appearance 纠正过来。
    //    之所以仍要在这里钉：主窗没有「页面上线后纠正」的那一瞬间之前的一切，
    //    而跟随系统会让冷启动首帧在两套主题之间跳。
    if (main.theme !== 'Dark') {
      fail(
        'window-chrome',
        `主窗口 theme 是「${main.theme ?? '（未设，跟随系统）'}」：建窗到页面挂载之间没人纠正原生 appearance，冷启动会在两套主题之间跳`,
      );
    }
  }

  // ③ 每扇副窗都要自己钉标题与主题。**钉 Dark 不等于最终是深色**：主题真值在
  //    localStorage，Rust 读不到，所以建窗时统一从 Dark 起步，页面挂载前
  //    `applyTheme()` 再按实际主题纠正本窗（见 ui/src/shell/bridge.js 的
  //    setWindowTheme）。少钉的那一扇没人纠正，浅色内容就顶着深色原生标题栏
  //    （2026-10-07 用户截图）。两件事分别计数而不是逐扇配对：配对要看
  //    「`.title(` 往后 30 行里有没有 `.theme(`」，那是个靠行距的启发式，往
  //    builder 中间插几行参数就会静默失配。改成全局对账。
  const popupFiles = [];
  let titles = 0;
  let hardcoded = 0;
  let themes = 0;
  for (const rel of RUST_SOURCES) {
    const lines = read(rel).split('\n');
    for (let i = 0; i < lines.length; i += 1) {
      // 只认 `builder.title(`，不认 `notify::task` 里的 `set_title(`。
      if (!/\.title\(/.test(lines[i])) continue;
      titles += 1;
      popupFiles.push(`${rel}:${i + 1}`);
      if (!/window_title\(/.test(lines[i])) hardcoded += 1;
    }
    themes += (lines.join('\n').match(/\.theme\(Some\(tauri::Theme::Dark\)\)/g) ?? []).length;
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
        `建了 ${titles} 扇带标题的窗，只钉了 ${themes} 次 Theme::Dark——少钉的那扇没人按应用主题纠正，浅色内容会顶着系统默认的原生标题栏（2026-10-07 用户截图）`,
      );
    }
  }
  if (hardcoded === 0 && themes === titles) {
    note(
      `窗口标题统一走 window_title()，${titles} 扇副窗都声明了建窗兜底主题（页面上线后由 applyTheme → window.setTheme 纠正）`,
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

// --- 20. 官网网页版的 hostname 名单与页签表一一对应 ----------------------
//
// `titlebar-pulse.js` 靠 `OFFICIAL_HOSTNAMES` 决定顶条走**官网蓝**还是工作台绿。
// 它和 Rust 的 `OFFICIAL_CHAT_TABS` 是同一件事的两份名单，而：
//   · 两边分处 Rust / JS，不共享任何符号；
//   · `titlebar-pulse.test.mjs` 只跑 `127.0.0.1`（非官网那条路），官网分支
//     在测试里根本没被执行到；
//   · 少一个 host 的症状不是报错，是**新页签顶条悄悄变成工作台的绿**。
//
// 2026-10-07 用户要求移除千问页签时，这三份名单（Rust 页签表、前端兜底页签表、
// 这份 host 名单）都必须同步——少改任何一份都留不一致。

{
  const tabsSrc = read('src-tauri/src/harness/official_chat.rs');
  const tabBlock = tabsSrc.match(/OFFICIAL_CHAT_TABS: &\[\(&str, &str\)\] = &\[([\s\S]*?)\];/);
  if (!tabBlock) {
    fail('official-chat-tabs', 'official_chat.rs 里找不到 OFFICIAL_CHAT_TABS，判据失效');
  } else {
    const tabBody = tabBlock[1];
    // 名单里写的是常量名（OFFICIAL_CHAT_URL / OFFICIAL_CHAT_MINIMAX_URL），
    // 真正的地址定义在文件上方，按 `pub const X: &str = "url"` 展开。
    const urls = new Map(
      [...tabsSrc.matchAll(/pub (?:crate )?const (\w+): &str = "(https?:\/\/[^"]+)"/g)].map((m) => [
        m[1],
        new URL(m[2]).hostname,
      ])
    );
    const tabHosts = [...tabBody.matchAll(/OFFICIAL_CHAT_(\w+)/g)].map((m) => {
      const host = urls.get(`OFFICIAL_CHAT_${m[1]}`);
      if (!host) fail('official-chat-tabs', `OFFICIAL_CHAT_TABS 里的 ${m[1]} 在本文件找不到对应的地址常量`);
      return host;
    });
    const js = read('src-tauri/src/harness/titlebar-pulse.js');
    const list = js.match(/OFFICIAL_HOSTNAMES = \[([^\]]*)\]/);
    const scriptHosts = list
      ? [...list[1].matchAll(/"([^"]+)"/g)].map((m) => m[1])
      : fail('official-chat-tabs', 'titlebar-pulse.js 里找不到 OFFICIAL_HOSTNAMES');
    if (scriptHosts) {
      for (const host of tabHosts) {
        if (host && !scriptHosts.includes(host)) {
          fail('official-chat-tabs', `页签 ${host} 不在 titlebar-pulse.js 的 OFFICIAL_HOSTNAMES 里——它的顶条会走工作台绿而不是官网蓝`);
        }
      }
      for (const host of scriptHosts) {
        if (!tabHosts.includes(host)) {
          fail('official-chat-tabs', `titlebar-pulse.js 的 OFFICIAL_HOSTNAMES 里有 ${host}，但页签表里已经没有它了——名单是死的`);
        }
      }
      // 前端那条紧急渲染路径（IPC 不可用时的兜底）也得跟着页签表走。
      const fallback = read('ui/src/official-chat/officialChatTabs.js');
      const titles = [...tabBody.matchAll(/\("([^"]+)",\s*OFFICIAL_CHAT_/g)].map((m) => m[1]);
      const missing = titles.filter((t) => !fallback.includes(`title: '${t}'`));
      if (missing.length > 0) {
        fail('official-chat-tabs', `前端兜底页签表缺 ${missing.join(' / ')}——IPC 不可用时页签栏会与后端不一致`);
      }
      if (!missing.length && tabHosts.length > 0) {
        note(`${titles.length} 个官网页签在 Rust / 品牌条带名单 / 前端兜底表三处一致`);
      }
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

// --- 结果 --------------------------------------------------------------------

for (const message of notes) console.log(`✓ ${message}`);
if (failures.length > 0) {
  console.error('');
  for (const message of failures) console.error(`✗ ${message}`);
  console.error(`\n${failures.length} 项不变量检查失败。`);
  process.exit(1);
}
console.log('\n全部不变量检查通过。');
