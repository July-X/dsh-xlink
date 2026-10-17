// 检查跨文件重复代码；不限制单文件或全仓代码行数。
import { readFileSync, readdirSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
// 沿用原重复代码判据；窗口长度用于识别复制块，不是文件行数上限。
const DUPLICATE_BUDGET = 8;
const WINDOW = 10;
const MIN_SPAN = 12;

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

export function findDuplicateBlocks(sources) {
  const linesByFile = new Map();
  const keyToHits = new Map();
  for (const [file, raw] of sources) {
    const text = file.endsWith('.rs') ? stripRustTests(raw) : raw;
    const lines = significantLines(text);
    linesByFile.set(file, lines);
    for (let i = 0; i + WINDOW <= lines.length; i += 1) {
      const key = lines
        .slice(i, i + WINDOW)
        .map((entry) => entry.text)
        .join('\n');
      const hits = keyToHits.get(key);
      const hit = { file: file, start: i, line: lines[i].line };
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
  return { count: duplicates, samples: duplicateSamples };
}

function main() {
  const files = [
    ...walk(join(root, 'src-tauri/src'), ['.rs']),
    ...walk(join(root, 'ui/src'), ['.js', '.vue', '.css']),
  ].sort();
  const result = findDuplicateBlocks(files.map((file) => [show(file), readFileSync(file, 'utf8')]));
  console.log(`重复代码：${result.count} 处（允许 ${DUPLICATE_BUDGET} 处）`);
  for (const sample of result.samples) console.log(`  ${sample}`);
  if (!process.argv.includes('--report') && result.count > DUPLICATE_BUDGET) {
    console.error('重复代码检测未通过：请按相同语义提取共享实现，避免复制维护。');
    process.exitCode = 1;
  }
}

if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) main();
