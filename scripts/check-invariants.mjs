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
 *      （第 10、11 条的由来见下方注释，第 12 条见文件末尾的 ④。）
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

// --- 7. workflow 供应链：SHA 固定 + Dependabot ---------------------------------
//
// AGENTS.md 与 docs/release.md 都承诺「workflow 中所有 `uses:` 固定到 40 位 commit
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

// --- 9. CSS 自定义属性：var() 引用必须有定义 ---------------------------------
//
// 事故来源：code-review-2026-09-27 的 H4。`--text-muted` / `--surface-soft`
// 被 KernelTabs 与 MigrationPanel 引用了 8 处，却从未在 :root 定义。CSS 自定义
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
  const defined = new Set(
    [...readFileSync(join(srcDir, 'theme.css'), 'utf8').matchAll(/^\s*(--[a-z0-9-]+)\s*:/gm)].map(
      (m) => m[1],
    ),
  );
  const missing = new Map();
  for (const file of walk(srcDir, ['.css', '.vue', '.js'])) {
    const text = readFileSync(file, 'utf8');
    for (const match of text.matchAll(/var\(\s*(--[a-z0-9-]+)\s*([,)])/g)) {
      // 名字后面跟逗号 = 带了回退值（`var(--x, #999)`），有定义与否都不会
      // 静默失效；跟右括号 = 没给回退，才需要 theme.css 里真有定义。
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
        `${where} 引用了 ${name}，但 theme.css 没有定义它（且未给回退值）——整条声明会被浏览器丢弃并继承父级，样式静默失效`,
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
  for (const file of readdirSync(srcDir)) {
    if (!file.endsWith('.rs')) continue;
    const text = readFileSync(join(srcDir, file), 'utf8');
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
  const rustFiles = readdirSync(srcDir).filter((name) => name.endsWith('.rs'));

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
    if (file === 'instance.rs') continue; // 唯一合法的写者
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
    if (file === 'instance.rs') continue; // 唯一合法的维护者
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
  const kernelSrc = productionRust(readFileSync(join(srcDir, 'kernel.rs'), 'utf8'));
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
      guardMisses.push(`kernel.rs 里找不到 ${action} —— 守卫的去向就无从检查了`);
      continue;
    }
    if (!body.includes('ensure_own_shell_stopped(')) {
      guardMisses.push(`kernel.rs 的 ${action} 没有调用 ensure_own_shell_stopped —— 本壳工作台运行期间必须拦住`);
    }
  }
  // ② 旧的跨壳硬拦入口不得复活（名字一旦回来，就有人会再挂一个 Err 上去）。
  //    只查 kernel.rs：`patches::ensure_workbench_stopped` 是**另一个**同名函数
  //    （补丁应用 / 撤销的停机守卫），与跨壳判据无关，别把它算进来。
  if (kernelSrc.includes('ensure_workbench_stopped(')) {
    guardMisses.push(`kernel.rs 又出现了 ensure_workbench_stopped —— 跨壳判据已降级为提示，硬拦入口不该复活`);
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
  const unexpected = otherShellCallSites.filter((where) => !where.startsWith('kernel.rs:'));
  if (unexpected.length > 0) {
    guardMisses.push(
      `${unexpected.join('、')} 调用了 workbench_running_in_other_shell —— 跨壳判据只该出现在 ` +
        'kernel.rs 的 warn_other_shell_workbench（提示）与 status（让 UI 在点之前看见）两处',
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
    guardMisses.push('kernel.rs 里找不到 warn_other_shell_workbench —— 跨壳提示的落点没了');
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
    const commandsSrc = productionRust(readFileSync(join(srcDir, 'commands.rs'), 'utf8'));
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
    const harnessSrc = productionRust(readFileSync(join(srcDir, 'harness_window.rs'), 'utf8'));
    // 证据必须真的在 Incident 上：命令层是从 `incident.health` 取出来递给判据的
    // （commands.rs 是反棘轮文件，不能为了留一份副本多写三行）。`diagnose_runtime`
    // 把它设成 None 的话，`if let Some(health)` 那一步会静默跳过，**第三层整个不工作
    // 且没有任何线索**——与 ACL 事故同一类。它的提前返回路径由
    // `existing.health.as_ref() == Some(&report)` 这个条件保证带着 health，所以这里
    // 只需钉住唯一那个构造点。
    const guardSrc = productionRust(readFileSync(join(srcDir, 'guard.rs'), 'utf8'));
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
    const harnessScripts = ['harness-draft.js', 'harness-health.js'].map((name) => ({
      name,
      source: readFileSync(join(srcDir, name), 'utf8'),
    }));
    const cmdSrc = productionRust(readFileSync(join(srcDir, 'harness_cmd.rs'), 'utf8'));
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
        if (!line.includes('harness-health.js')) return;
        const window = lines.slice(Math.max(0, index - 6), index + 7).join('\n');
        if (!window.includes('harness-draft.js')) {
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
      const lines = productionRust(readFileSync(join(srcDir, 'kernel.rs'), 'utf8')).split('\n');
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
        draftMisses.push(`kernel.rs 里找不到 ${action}，信标接线无从检查`);
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
        readFileSync(join(srcDir, 'kernel.rs'), 'utf8'),
      );
      if (!kernelInstallSrc.includes('--config.package-import-method=copy')) {
        fail(
          'kernel-install-isolated-inodes',
          'kernel.rs 的 pnpm 安装参数里没有 --config.package-import-method=copy —— ' +
            '内核树会重新与 pnpm store 共享 inode，对面正在服务的工作台页面会再次被' +
            '装 / 删内核打死（2026-09-30 实证机制：链接数变化 → NTFS ChangeTime → ' +
            'client-hmr 的 500ms stat 轮询误判 bundle 重建 → 活页面换模块 → 槽位不变量' +
            '崩溃。见 AGENTS.md「装包任务不许惊动另一个壳的工作台」）。',
        );
      } else {
        note('内核安装用 copy 落盘：树与 pnpm store 不共享 inode');
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
  const rustFiles = readdirSync(srcDir).filter((name) => name.endsWith('.rs'));
  const builders = [];
  for (const file of rustFiles) {
    const lines = productionRust(readFileSync(join(srcDir, file), 'utf8')).split('\n');
    lines.forEach((line, index) => {
      if (line.includes('.updater_builder()')) builders.push({ file, line: index + 1, window: lines.slice(index, index + 12).join('\n') });
    });
  }
  if (builders.length !== 1 || builders[0].file !== 'updater.rs') {
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

// --- 结果 --------------------------------------------------------------------

for (const message of notes) console.log(`✓ ${message}`);
if (failures.length > 0) {
  console.error('');
  for (const message of failures) console.error(`✗ ${message}`);
  console.error(`\n${failures.length} 项不变量检查失败。`);
  process.exit(1);
}
console.log('\n全部不变量检查通过。');
