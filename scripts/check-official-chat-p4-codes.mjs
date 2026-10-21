#!/usr/bin/env node
// P4 协议 §10 判据**第 6 条**（错误面）—— **不新增第 14 个码**（13 码闭集不动）⇒ 按**常量闭集**断。
//
// ## 依据
//
// · 协议 §10 第 6 条：「错误面：不新增第 14 个码（13 码闭集不动）⇒ 按常量闭集断」。
// · **13 码闭集**的**契约源** ＝ P1 协议 §3 的表（`docs/features/official-chat/
//   official-chat-p1-protocol.md:144-158`）——**本脚本的期望清单抄自那张表**，
//   **不是**抄自被测实现（否则判据就被被测物定义了）。
// · 闭集的**两份实现副本**（两边都该与契约逐项相等）：
//   · Rust 侧：`src-tauri/src/official_chat/protocol.rs` 的 `pub const ERROR_CODES: [&str; 13]`
//   · JS 侧：`plugins/official-chat/host/protocol.js` 的 `ERROR_CODE_NAMES`（＋ `ERROR_CODES` 映射）
//
// ## 为什么这条判据**现在就落**（PM 2026-10-21 16:1x 裁定）
//
// 13 码闭集**不依赖 P4 实现**（闭集现成在仓库里）⇒ ⇒ 它是**真判据、不是占位**
// ⇒ ⇒ 与「占位型判据是负资产」的纪律不冲突。
// 它补的正是 Rust 单测的盲区：现有 `error_codes_are_thirteen_and_carry_retryability`
// **只断长度 13**，**没断逐项相等**，也**没断「`match` 分支里塞了个闭集外的硬编码码」**
// （`protocol.rs` 的 `match raw.as_str()` 那段就是高风险面）⇒ ⇒ 本判据断这三件事。
//
// ## 断的四件事
//
// ① Rust 侧闭集 ＝ 期望 13 码（**逐项**，差集点名）；
// ② JS 侧闭集 ＝ 期望 13 码（**逐项**）；
// ③ 闭集**长度 ＝ 13**（不扩第 14 个）＋ **无重复**；
// ④ **所有「码返回点」的字面量 ∈ 期望闭集**（`=> "x"` / `return "x"` 形态）⇒ 闭集外
//    的硬编码码当场红（点名 文件:行 ＋ 字面量）。
//
// ## 扫描面（**只看 P4 壳侧实现**；内核侧闭集副本只断「与契约逐项相等」）
//
// · `src-tauri/src/official_chat/**` —— P4 的壳侧实现（`protocol.rs` 的闭集真身在这里）。
// · `plugins/official-chat/host/protocol.js` —— 内核侧闭集副本（**P1 的资产**）。
// · **明确排除**：`plugins/official-chat/**` 的其余部分（内核侧资产，由 P1 自己的测试与
//   夹具同构守着）｜`src-tauri/examples/**`（取证探针）｜`scripts`｜`docs`。
//
// ## 纪律
//
// · **自包含**：任意干净 checkout 上独立跑通；**零污染**（夹具全在内存里，不落盘）。
// · **只断码名**：只看**码名字面量**与**闭集内容**；**不回任何正文、标题、会话 id、凭据**。
// · **三态 verdict**：`pass` / `fail` / `skipped`（「没测到」≠「测了没过」）。
// · **退出码按场景级算**：故意造坏数据的场景，其内层断言本就该 FAIL。
//
// @module scripts/check-official-chat-p4-codes

import { mkdirSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

/** verdict 三态。 */
export const PASS = 'pass';
export const FAIL = 'fail';
export const SKIPPED = 'skipped';

/**
 * **期望的 13 码闭集**（契约源 ＝ P1 协议 §3 的表，`official-chat-p1-protocol.md:144-158`）。
 *
 * ⚠️ **这份清单抄自协议，不抄自实现** —— 判据不能被被测物定义。
 */
export const EXPECTED_CODES = Object.freeze([
  'not_logged_in',
  'account_changed',
  'source_unsupported',
  'schema_changed',
  'partial_content',
  'body_too_large',
  'timeout',
  'cancelled',
  'snapshot_invalid',
  'kernel_incompatible',
  'catalog_empty',
  'catalog_failed',
  'stale_draft',
]);

/** Rust 侧闭集真身（相对仓库根）。 */
export const RUST_CODES_REL = 'src-tauri/src/official_chat/protocol.rs';

/** JS 侧闭集副本（相对仓库根）。 */
export const JS_CODES_REL = 'plugins/official-chat/host/protocol.js';

/** **扫描面**（相对仓库根）：P4 壳侧实现。 */
export const SCAN_GLOBS = Object.freeze(['src-tauri/src/official_chat/**/*.rs']);

/** 判据要读的**全部**文件 ＝ 扫描面 ＋ JS 侧闭集副本。 */
export const SCAN_FILES = Object.freeze([...SCAN_GLOBS, JS_CODES_REL]);

/** **明确排除**的路径前缀（附理由，理由进代码，不靠记忆）。 */
export const EXCLUDED_PREFIXES = Object.freeze([
  ['src-tauri/examples', '取证探针：不是生产实现'],
  ['scripts', '判据与工具自身'],
  ['docs', '文档不是实现'],
]);

/**
 * 去掉**行注释**与**块注释**（**保留字符串字面量**）。
 *
 * ⚠️ 只为「找码返回点」服务 ⇒ 只需要知道**哪一行还剩代码**。
 *
 * @param {string} src - 源码文本。
 * @returns {string[]} 清理后的行（行号与原文对齐）。
 */
export function stripComments(src) {
  const lines = String(src ?? '').split('\n');
  const out = [];
  let inBlock = false;
  for (const raw of lines) {
    let line = raw;
    if (inBlock) {
      const end = line.indexOf('*/');
      if (end === -1) {
        out.push('');
        continue;
      }
      line = line.slice(end + 2);
      inBlock = false;
    }
    // 同一行里可能有多个块注释
    for (let guard = 0; guard < 32; guard += 1) {
      const b = line.indexOf('/*');
      if (b === -1) break;
      const e = line.indexOf('*/', b + 2);
      if (e === -1) {
        line = line.slice(0, b);
        inBlock = true;
        break;
      }
      line = line.slice(0, b) + line.slice(e + 2);
    }
    // 行注释：`//` 必须在字符串**之外**才算（否则 "https://…" 会被误截）
    let quote = null;
    let cut = -1;
    for (let i = 0; i < line.length; i += 1) {
      const ch = line[i];
      if (quote) {
        if (ch === '\\') {
          i += 1;
          continue;
        }
        if (ch === quote) quote = null;
        continue;
      }
      if (ch === '"' || ch === "'" || ch === '`') {
        quote = ch;
        continue;
      }
      if (ch === '/' && line[i + 1] === '/') {
        cut = i;
        break;
      }
    }
    out.push(cut === -1 ? line : line.slice(0, cut));
  }
  return out;
}

/**
 * 从 Rust 源码里抽 `pub const ERROR_CODES: [&str; N] = [ ... ];` 的**码名数组**。
 *
 * @param {string} src - Rust 源码。
 * @returns {{codes: string[], declaredLen: number|null}} 抽到的码名 ＋ 常量里写的长度。
 */
export function extractRustCodes(src) {
  const text = String(src ?? '');
  const m = /pub\s+const\s+ERROR_CODES\s*:\s*\[&str;\s*(\d+)\]\s*=\s*\[([\s\S]*?)\]\s*;/.exec(text);
  if (!m) return { codes: [], declaredLen: null };
  const codes = [];
  for (const lit of m[2].matchAll(/["']([a-z_]{2,})["']/g)) codes.push(lit[1]);
  return { codes, declaredLen: Number(m[1]) };
}

/**
 * 从 JS 源码里抽 `export const ERROR_CODE_NAMES = Object.freeze([ ... ])` 的**码名数组**。
 *
 * @param {string} src - JS 源码。
 * @returns {string[]} 码名数组（抽不到 ⇒ 空数组）。
 */
export function extractJsNames(src) {
  const text = String(src ?? '');
  const m = /export\s+const\s+ERROR_CODE_NAMES\s*=\s*Object\.freeze\(\[([\s\S]*?)\]\)/.exec(text);
  if (!m) return [];
  const names = [];
  for (const lit of m[1].matchAll(/["']([a-z_]{2,})["']/g)) names.push(lit[1]);
  return names;
}

/**
 * 从 JS 源码里抽 `export const ERROR_CODES = { key: {...}, ... }` 的**键集合**。
 *
 * @param {string} src - JS 源码。
 * @returns {string[]} 映射的键（抽不到 ⇒ 空数组）。
 */
export function extractJsMapKeys(src) {
  const text = String(src ?? '');
  const m = /export\s+const\s+ERROR_CODES\s*=\s*\{([\s\S]*?)\n\};/.exec(text);
  if (!m) return [];
  const keys = [];
  for (const km of m[1].matchAll(/^[ \t]*([a-z_]{2,})[ \t]*:/gm)) keys.push(km[1]);
  return keys;
}

/**
 * 抽**码返回点**：返回**码名字面量**的位置（`=> "x"` / `return "x"` 形态）。
 *
 * ⚠️ **只认 `=>` 右侧那一个**（match arm 的**返回值**）⇒ match arm **匹配侧**的原因词
 *   （`timed_out` / `deadline_exceeded` …）**不算**码返回点（它们是**输入原因词**，
 *   本来就不在 13 码闭集里）。
 *
 * @param {string} src - 源码文本。
 * @param {string} [file] - 文件名（只进证据，不进判定）。
 * @returns {Array<{value: string, line: number, file: string, text: string}>}
 */
export function extractCodeReturns(src, file = '<inline>') {
  const lines = stripComments(src);
  const hits = [];
  lines.forEach((line, index) => {
    const re = /=>\s*"([a-z_]{2,})"|return\s+"([a-z_]{2,})"/g;
    let m;
    while ((m = re.exec(line)) !== null) {
      hits.push({
        value: m[1] ?? m[2],
        line: index + 1,
        file,
        text: line.trim().slice(0, 160),
      });
    }
  });
  return hits;
}

/**
 * 判定内核：按判据第 6 条的口径判一份读数。
 *
 * @param {{rustCodes?: string[], rustDeclaredLen?: number|null, jsNames?: string[],
 *          jsMapKeys?: string[], codeReturns?: Array<object>}} reading - 读数。
 * @returns {{ok: boolean, checks: Array<{name: string, status: string, detail: string}>}}
 */
export function judgeCodes(reading) {
  const checks = [];
  const add = (name, status, detail) => checks.push({ name, status, detail });

  const rust = Array.isArray(reading?.rustCodes) ? reading.rustCodes : [];
  const js = Array.isArray(reading?.jsNames) ? reading.jsNames : [];
  const jsKeys = Array.isArray(reading?.jsMapKeys) ? reading.jsMapKeys : [];
  const returns = Array.isArray(reading?.codeReturns) ? reading.codeReturns : [];
  const expected = new Set(EXPECTED_CODES);

  const diff = (actual) => {
    const set = new Set(actual);
    const missing = EXPECTED_CODES.filter((c) => !set.has(c));
    const extra = actual.filter((c) => !expected.has(c));
    return { missing, extra };
  };

  for (const [label, actual] of [
    ['Rust 侧', rust],
    ['JS 侧', js],
  ]) {
    if (actual.length === 0) {
      add(`P4-6 ${label}闭集 ＝ 期望 13 码（逐项）`, SKIPPED, `没读到${label}闭集 ⇒ 判据未执行`);
      continue;
    }
    const d = diff(actual);
    add(
      `P4-6 ${label}闭集 ＝ 期望 13 码（逐项）`,
      d.missing.length === 0 && d.extra.length === 0 ? PASS : FAIL,
      d.missing.length === 0 && d.extra.length === 0
        ? `${label}闭集与契约逐项相等（${actual.length} 码）`
        : `${label}闭集与契约不符：缺 ${d.missing.join('、') || '无'}／多或改 ${d.extra.join('、') || '无'}`,
    );
  }

  const declared = reading?.rustDeclaredLen ?? null;
  const lenOk = rust.length === EXPECTED_CODES.length && js.length === EXPECTED_CODES.length;
  add(
    'P4-6 闭集长度 ＝ 13（不扩第 14 个）',
    lenOk ? PASS : FAIL,
    `Rust 侧 ${rust.length} 码／JS 侧 ${js.length} 码（期望 ${EXPECTED_CODES.length}）` +
      (declared !== null ? `；Rust 常量里写的长度＝${declared}` : ''),
  );

  const dupOf = (list) => list.filter((c, i) => list.indexOf(c) !== i);
  const dups = [...new Set([...dupOf(rust), ...dupOf(js)])];
  add(
    'P4-6 闭集无重复',
    dups.length === 0 ? PASS : FAIL,
    dups.length === 0 ? '两侧闭集都没有重复码' : `重复出现：${dups.join('、')}`,
  );

  if (js.length === 0 || jsKeys.length === 0) {
    add('P4-6 JS 侧 ERROR_CODES 映射键集合 ＝ ERROR_CODE_NAMES', SKIPPED, '没读到 JS 侧映射 ⇒ 判据未执行');
  } else {
    const keySet = new Set(jsKeys);
    const missing = js.filter((c) => !keySet.has(c));
    const extra = jsKeys.filter((c) => !js.includes(c));
    add(
      'P4-6 JS 侧 ERROR_CODES 映射键集合 ＝ ERROR_CODE_NAMES',
      missing.length === 0 && extra.length === 0 ? PASS : FAIL,
      missing.length === 0 && extra.length === 0
        ? `映射 ${jsKeys.length} 键与名字数组一致`
        : `映射与名字数组不一致：映射缺 ${missing.join('、') || '无'}／映射多 ${extra.join('、') || '无'}`,
    );
  }

  if (returns.length === 0) {
    add('P4-6 所有码返回点的值 ∈ 期望闭集', SKIPPED, '扫描面里没找到码返回点 ⇒ 判据未执行');
  } else {
    const outside = returns.filter((r) => !expected.has(r.value));
    add(
      'P4-6 所有码返回点的值 ∈ 期望闭集',
      outside.length === 0 ? PASS : FAIL,
      outside.length === 0
        ? `${returns.length} 处码返回点全部在 13 码闭集内`
        : `闭集外的硬编码码 ${outside.length} 处：` +
          outside
            .slice(0, 5)
            .map((r) => `${r.file}:${r.line} "${r.value}"`)
            .join('；'),
    );
  }

  const judged = checks.filter((c) => c.status !== SKIPPED);
  return { ok: judged.length > 0 && judged.every((c) => c.status === PASS), checks };
}

/**
 * **场景表**（合成读数）：每个场景声明「应当通过」还是「应当失败」。
 *
 * ⚠️ 场景 B 就是 PM 点名的那条：**「改一个码名但长度不变 ⇒ 必红」**。
 *
 * @returns {Array<{id: string, name: string, expect: string, reading: object}>}
 */
export function buildScenarios() {
  const clean = {
    rustCodes: [...EXPECTED_CODES],
    rustDeclaredLen: EXPECTED_CODES.length,
    jsNames: [...EXPECTED_CODES],
    jsMapKeys: [...EXPECTED_CODES],
    codeReturns: [
      { value: 'timeout', line: 1, file: 'fixture.rs', text: '_ => "timeout"' },
      { value: 'schema_changed', line: 2, file: 'fixture.rs', text: '"x" => "schema_changed"' },
    ],
  };

  // 场景 B：改一个码名、**长度不变**
  const renamed = [...EXPECTED_CODES];
  renamed[renamed.indexOf('catalog_empty')] = 'catalog_void';

  // 场景 C：`match` 分支里塞了个**闭集外的硬编码码**（闭集与长度都不变）
  const hardcoded = {
    ...clean,
    codeReturns: [
      ...clean.codeReturns,
      { value: 'not_found', line: 3, file: 'fixture.rs', text: '"missing" => "not_found"' },
    ],
  };

  // 场景 D：少一个码
  const fewer = [...EXPECTED_CODES].slice(0, -1);

  // 场景 E：多一个码（扩第 14 个）
  const more = [...EXPECTED_CODES, 'kernel_timeout'];

  return [
    { id: 'A', name: '场景A 真身闭集（13 码、返回点都在闭集内）⇒ 应通过', expect: PASS, reading: clean },
    {
      id: 'B',
      name: '场景B 改一个码名但长度不变（catalog_empty→catalog_void）⇒ 应失败',
      expect: FAIL,
      reading: { ...clean, rustCodes: renamed, jsNames: renamed, jsMapKeys: renamed },
    },
    { id: 'C', name: '场景C match 分支塞闭集外硬编码码（not_found）⇒ 应失败', expect: FAIL, reading: hardcoded },
    {
      id: 'D',
      name: '场景D 少一个码（12 个）⇒ 应失败',
      expect: FAIL,
      reading: { ...clean, rustCodes: fewer, jsNames: fewer, jsMapKeys: fewer },
    },
    {
      id: 'E',
      name: '场景E 多一个码（14 个）⇒ 应失败',
      expect: FAIL,
      reading: { ...clean, rustCodes: more, jsNames: more, jsMapKeys: more },
    },
  ];
}

/**
 * 跑场景表：核「实际判定」是否等于「应当的判定」。
 *
 * @param {Array<object>} [scenarios] - 场景表。
 * @returns {Array<{name: string, status: string, detail: string, checks: Array<object>}>}
 */
export function runScenarios(scenarios = buildScenarios()) {
  return scenarios.map((sc) => {
    const { ok, checks } = judgeCodes(sc.reading);
    const behavedAsExpected = sc.expect === PASS ? ok : !ok;
    const bad = checks.filter((c) => c.status === FAIL).map((c) => c.name);
    const skipped = checks.filter((c) => c.status === SKIPPED).map((c) => c.name);
    return {
      name: `P4-6a ${sc.name}`,
      status: behavedAsExpected ? PASS : FAIL,
      detail:
        `实际判定＝${ok ? '通过' : '失败'}（期望＝${sc.expect === PASS ? '通过' : '失败'}）` +
        (bad.length ? `｜未过的断言：${bad.join('、')}` : '｜可机检的断言全过') +
        (skipped.length ? `｜未执行：${skipped.join('、')}` : ''),
      checks,
    };
  });
}

/**
 * 收集扫描面内的文件（**只读**）。
 *
 * @param {string} root - 仓库根。
 * @returns {Array<{path: string, code: string}>}
 */
export function collectSources(root) {
  const out = [];
  const seen = new Set();
  const push = (rel) => {
    if (seen.has(rel)) return;
    seen.add(rel);
    if (EXCLUDED_PREFIXES.some(([prefix]) => rel.startsWith(prefix))) return;
    try {
      out.push({ path: rel, code: readFileSync(join(root, rel), 'utf8') });
    } catch {
      /* 读不动的文件跳过 */
    }
  };
  // 扫描面：壳侧实现的 .rs（目录只有一层，用目录遍历取，避免引第三方 glob）
  const base = join(root, 'src-tauri/src/official_chat');
  try {
    for (const entry of readdirSync(base, { withFileTypes: true })) {
      if (entry.isFile() && entry.name.endsWith('.rs')) push(`src-tauri/src/official_chat/${entry.name}`);
    }
  } catch {
    /* 目录不存在 ⇒ 扫描面为空 */
  }
  push(JS_CODES_REL);
  return out;
}

/**
 * 汇总计数（与 P1/P2/P3 判据的产物同构）。
 *
 * @param {Array<{status: string}>} results
 * @returns {{total: number, pass: number, fail: number, skipped: number}}
 */
export function summarize(results) {
  const list = Array.isArray(results) ? results : [];
  return {
    total: list.length,
    pass: list.filter((r) => r?.status === PASS).length,
    fail: list.filter((r) => r?.status === FAIL).length,
    skipped: list.filter((r) => r?.status === SKIPPED).length,
  };
}

/**
 * 主入口。
 *
 * @param {string[]} [argv] - 命令行参数（默认 `process.argv.slice(2)`）。
 * @returns {{results: Array<object>, summary: object}}
 */
export async function main(argv = process.argv.slice(2)) {
  const repo = fileURLToPath(new URL('../', import.meta.url));
  const roundIndex = argv.findIndex((a) => a === '--round' || String(a).startsWith('--round='));
  let round = 'p4c6';
  if (roundIndex >= 0) {
    const inline = String(argv[roundIndex]).split('=')[1];
    const next = argv[roundIndex + 1];
    round = inline ?? (next !== undefined && !String(next).startsWith('--') ? next : 'p4c6');
  }
  const targetIndex = argv.findIndex((a) => a === '--target' || String(a).startsWith('--target='));
  let root = repo.replace(/[\\/]+$/, '');
  if (targetIndex >= 0) {
    const inline = String(argv[targetIndex]).split('=')[1];
    const next = argv[targetIndex + 1];
    root = (inline ?? (next !== undefined && !String(next).startsWith('--') ? next : repo)).replace(
      /[\\/]+$/,
      '',
    );
  }
  const asJson = argv.includes('--json');

  const sources = collectSources(root);
  const rustSrc = sources.find((s) => s.path === RUST_CODES_REL)?.code ?? '';
  const jsSrc = sources.find((s) => s.path === JS_CODES_REL)?.code ?? '';
  const rust = extractRustCodes(rustSrc);
  const codeReturns = sources.flatMap((s) => extractCodeReturns(s.code, s.path));

  const reading = {
    rustCodes: rust.codes,
    rustDeclaredLen: rust.declaredLen,
    jsNames: extractJsNames(jsSrc),
    jsMapKeys: extractJsMapKeys(jsSrc),
    codeReturns,
  };

  const results = [];
  const { ok, checks } = judgeCodes(reading);
  for (const c of checks) results.push({ name: c.name, status: c.status, detail: c.detail });

  // 场景级自证（退出码按这一层算）
  const scenarioResults = runScenarios();
  for (const r of scenarioResults) {
    results.push({ name: r.name, status: r.status, detail: r.detail });
  }
  if (!ok) {
    results.push({
      name: 'P4-6 真实仓库读数的判定',
      status: FAIL,
      detail: '真实读数未全过（见上）',
    });
  } else {
    results.push({
      name: 'P4-6 真实仓库读数的判定',
      status: PASS,
      detail: `真实读数全过（Rust ${reading.rustCodes.length} 码／JS ${reading.jsNames.length} 码／码返回点 ${codeReturns.length} 处）`,
    });
  }

  const summary = summarize(results);
  const evidenceDir = join(tmpdir(), 'dsh-p4-codes');
  mkdirSync(evidenceDir, { recursive: true });
  const outPath = join(evidenceDir, `results-${round}.json`);
  writeFileSync(
    outPath,
    `${JSON.stringify({ round, at: new Date().toISOString(), scanned: sources.map((s) => s.path), reading, results, summary }, null, 2)}\n`,
    'utf8',
  );

  if (asJson) {
    process.stdout.write(`${JSON.stringify({ round, results, summary }, null, 2)}\n`);
  } else {
    for (const r of results) {
      const mark = r.status === PASS ? '✔' : r.status === FAIL ? '✘' : '—';
      process.stdout.write(`${mark} ${r.name}：${r.detail}\n`);
    }
    process.stdout.write(
      `\n合计 ${summary.total} 条：通过 ${summary.pass} · 失败 ${summary.fail} · 未执行 ${summary.skipped}\n` +
        `扫描面：src-tauri/src/official_chat/** ＋ plugins/official-chat/host/protocol.js（JS 侧闭集副本）\n` +
        `扫了 ${sources.length} 个文件 · 证据：${outPath}\n`,
    );
  }
  return { results, summary };
}

// 直接跑（被 import 时不跑）
if (process.argv[1] && process.argv[1].endsWith('check-official-chat-p4-codes.mjs')) {
  await main();
}
