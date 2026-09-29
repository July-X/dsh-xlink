#!/usr/bin/env node
// 验证内置补丁 dsh-scope-suspend 对当前激活内核仍然成立。
//
// 补丁是精确字符串替换（`replace` 模式），所以它会随内核升级悄悄失效：搜索串
// 不在了，或者被挪到了别处。壳能做的只是「点下去时中止并报内核版本可能已升级」，
// 真正该在这里拦住的是维护者：升级内核之后跑一次 `npm run test:scope-suspend`，
// 立刻知道这个补丁还能不能打。
//
// 跟另外两个 verify 脚本同一条版本门：当前内核不在补丁适用范围内时打印并 exit 0，
// 不适用不等于坏了。（否则在 0.2.0-rc.1 上会看到 `dsh-session-perf` 崩栈，
// 让人以为补丁坏了。）
import { readFileSync, existsSync } from 'node:fs';
import { join, dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, '..');
const patchDir = join(repo, 'src-tauri', 'resources', 'patches', 'dsh-scope-suspend');

const xlinkHome = process.env.DSH_XLINK_HOME || join(process.env.USERPROFILE || process.env.HOME, '.dsh-xlink');
const mode = process.env.DSH_SHELL_MODE === 'dev' ? 'dev' : 'release';
const dataDir = join(xlinkHome, 'dsh', `desktop${mode === 'dev' ? '-dev' : ''}`);

// 版本比较：与 Rust 侧 `version::cmp_versions` 同一套语义（数字段比大小，
// 预发布后缀按 rc > alpha、同级比数字），只用于「够不够 minKernelVersion」。
function parseVersion(text) {
  const [core, pre = ''] = String(text).split('-', 2);
  const nums = core.split('.').map((n) => Number(n) || 0);
  const stage = /^rc/.test(pre) ? 2 : /^alpha/.test(pre) ? 1 : 3; // 无后缀视为正式版
  const preNum = Number((pre.match(/(\d+)/) || [0, 0])[1]) || 0;
  return { nums, stage, preNum };
}
function cmp(a, b) {
  const left = parseVersion(a);
  const right = parseVersion(b);
  for (let i = 0; i < 3; i += 1) {
    if ((left.nums[i] || 0) !== (right.nums[i] || 0)) return (left.nums[i] || 0) - (right.nums[i] || 0);
  }
  if (left.stage !== right.stage) return left.stage - right.stage;
  return left.preNum - right.preNum;
}

const manifest = JSON.parse(readFileSync(join(patchDir, 'manifest.json'), 'utf8'));
const patch = manifest.patches.find((p) => p.id === 'dsh-scope-suspend');
if (!patch) {
  console.error('✗ 清单里找不到 dsh-scope-suspend');
  process.exit(1);
}

const activeFile = join(dataDir, 'active.txt');
if (!existsSync(activeFile)) {
  console.log(`- 没有已激活的内核（${activeFile} 不存在），跳过`);
  process.exit(0);
}
const version = readFileSync(activeFile, 'utf8').trim();
const kernelRoot = join(dataDir, 'kernels', version);
if (!existsSync(kernelRoot)) {
  console.log(`- 激活版本 ${version} 未安装，跳过`);
  process.exit(0);
}
if (cmp(version, patch.minKernelVersion) < 0) {
  console.log(`- 当前内核 ${version} 低于补丁适用范围（≥ ${patch.minKernelVersion}），不适用，跳过`);
  process.exit(0);
}
if (patch.maxKernelVersion && cmp(version, patch.maxKernelVersion) > 0) {
  console.log(`- 当前内核 ${version} 高于补丁适用范围（≤ ${patch.maxKernelVersion}），不适用，跳过`);
  process.exit(0);
}

let failures = 0;
for (const file of patch.files) {
  if (file.mode !== 'replace') {
    console.error(`✗ ${file.to}: 本脚本只验证 replace 模式`);
    failures += 1;
    continue;
  }
  const target = join(kernelRoot, file.to);
  if (!existsSync(target)) {
    console.error(`✗ ${file.to}: 目标文件不存在`);
    failures += 1;
    continue;
  }
  const original = readFileSync(target, 'utf8');
  const hits = original.split(file.search).length - 1;
  if (hits === 0) {
    console.error(`✗ ${file.to}: 搜索串一次都没命中——内核改了实现，补丁已失效（内核版本可能已升级）`);
    failures += 1;
    continue;
  }
  if (hits > 1) {
    // 全文替换：多处命中会让补丁在不该改的地方也改一遍。
    console.error(`✗ ${file.to}: 搜索串命中 ${hits} 次（必须恰好 1 次）`);
    failures += 1;
    continue;
  }
  if (original.includes(file.replacement.trim())) {
    console.log(`= ${file.to}: 已经是补丁后的状态（幂等，无需重打）`);
    continue;
  }
  const patched = original.replace(file.search, file.replacement);
  // 替换结果必须仍是可解析的 JS——否则「补丁生效了」等于「工作台起不来了」。
  const tmp = join(process.env.TEMP || '.', `dsh-scope-suspend-check-${process.pid}.mjs`);
  try {
    const { writeFileSync, rmSync } = await import('node:fs');
    writeFileSync(tmp, patched);
    execFileSync(process.execPath, ['--check', tmp], { stdio: 'pipe' });
    // 判「抛错已消除」要查抛错那句话本身，不能拿 search 的前缀——replacement
    // 与 search 共享 `if (adapter === void 0) ` 开头，截前 40 字符永远还在。
    const stillThrows = patched.includes("rendered without an installed adapter");
    const suspends = /if \(adapter === void 0\) return null;/.test(patched);
    if (stillThrows || !suspends) {
      console.error(`✗ ${file.to}: 替换后抛错仍在=${stillThrows} 挂起语句就位=${suspends}`);
      failures += 1;
    } else {
      console.log(`✓ ${file.to}: 搜索串精确命中 1 次，替换后仍是合法 JS，抛错已换成挂起`);
    }
    rmSync(tmp, { force: true });
  } catch (error) {
    console.error(`✗ ${file.to}: 替换后不是合法 JS —— ${(error.stderr || error.stdout || '').toString().split('\n')[0]}`);
    failures += 1;
  }
}

if (failures > 0) {
  console.error(`\n${failures} 项检查失败。若内核确实改写了这段实现，需要重新对齐补丁的 search/replacement。`);
  process.exit(1);
}
console.log('\n全部检查通过：dsh-scope-suspend 对当前内核仍然可应用。');
