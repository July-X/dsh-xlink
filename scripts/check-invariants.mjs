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
 *      copy 模式的 `from` 文件确实存在于仓库里）。
 */

import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs';
import { dirname, isAbsolute, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const failures = [];
const notes = [];

const fail = (section, message) => failures.push(`[${section}] ${message}`);
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
      const segments = String(target).split(/[\\/]/);
      if (!target || isAbsolute(target) || /^[A-Za-z]:/.test(target) || segments.includes('..')) {
        fail('patches', `${label} 的 ${id} 有非法目标路径：${JSON.stringify(target)}`);
      }
      if (entry.mode === 'copy') {
        const from = entry.from;
        if (!from) {
          fail('patches', `${label} 的 ${id} copy 模式缺少 from（目标 ${target}）`);
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

// --- 结果 --------------------------------------------------------------------

for (const message of notes) console.log(`✓ ${message}`);
if (failures.length > 0) {
  console.error('');
  for (const message of failures) console.error(`✗ ${message}`);
  console.error(`\n${failures.length} 项不变量检查失败。`);
  process.exit(1);
}
console.log('\n全部不变量检查通过。');
