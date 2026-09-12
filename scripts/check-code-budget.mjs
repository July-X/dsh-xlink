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
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const reportOnly = process.argv.includes('--report');

/** 生产代码行数预算：文件 → 上限。包含注释以外的所有代码行。 */
const FILE_BUDGETS = {
  'ui/src/theme.css': 2900,
  'src-tauri/src/plugins.rs': 2650,
  'src-tauri/src/commands.rs': 1550,
  'src-tauri/src/skills.rs': 1490,
  'src-tauri/src/patches.rs': 1250,
  'src-tauri/src/kernel.rs': 1180,
  'src-tauri/src/process.rs': 1180,
  // 1050 → 1090：会话标题改为订阅 `session/control`（baseline 播种 + 标题投影帧
  // 保鲜 + 老内核退回 session/list 快照），这部分逻辑与 Center 同生共死，拆出去
  // 只会把状态机切成两半。详见 docs/notification-design.md §3.3。
  'src-tauri/src/notify.rs': 1090,
  'src-tauri/src/guard.rs': 940,
  'ui/src/store.js': 430,
};
/** 全部受检文件的合计预算（Tauri 生产代码 + 前端 js/vue/css）。 */
const TOTAL_BUDGET = 20400;
/** 重复区间数上限。 */
const DUPLICATE_BUDGET = 6;
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
