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
 */

import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs';
import { dirname, isAbsolute, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

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

  const titlebar = read('ui/src/components/WindowTitleBar.vue');
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

// --- 结果 --------------------------------------------------------------------

for (const message of notes) console.log(`✓ ${message}`);
if (failures.length > 0) {
  console.error('');
  for (const message of failures) console.error(`✗ ${message}`);
  console.error(`\n${failures.length} 项不变量检查失败。`);
  process.exit(1);
}
console.log('\n全部不变量检查通过。');
