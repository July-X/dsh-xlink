#!/usr/bin/env node
// P4 协议 §10 判据**第 7 条**（两类错误分离）—— 按**源码形态**断。
//
// ## 依据
//
// · 协议 §10 第 7 条：「**回程失败 ≠ 站点返回空** ⇒ **两个不同的既有码**」。
// · 协议 §6.3：「**回程失败**（求值超时 / 页面销毁 / 导航 / 壳侧错误）与**站点返回空**
//   （接口正常返回但没有会话）**不许合并成一个码、不许互相冒充**」。
// · 协议 §6.1：**不扩** P1 §3 的闭集 ⇒ 映射到的码**必须都在闭集里**。
// · 协议 §6.2：站内码**只原样上报**，不解释语义 ⇒ 站内码也归到既有码（`catalog_failed`）。
//
// ## 断的四件事
//
// ① 取数模块的**失败 → 协议码**映射抽得到（`Return` / `Timeout` / `Empty` / `Schema` /
//    `SiteCode` 五个分支）——抽不到 ⇒ 报「未执行」，不是「失败」；
// ② **回程失败（`Return`）与站点返回空（`Empty`）映射到两个不同的码**（本条的主干）；
// ③ 那两个码**都在 P1 §3 的闭集里**（闭集**抄自协议正文**，不抄实现）；
// ④ **超时**也与站点返回空不同（超时是第三类：回程那侧的可重试失败）。
//
// ## 闭集的来源
//
// **P1 协议 §3 的表**（`docs/features/official-chat/official-chat-p1-protocol.md`）——
// 只读，不改协议一个字。**不抄实现**（判据不能被被测物定义）。
//
// ## 读取面（跟着实现走）
//
// **整个 `src-tauri/src/official_chat/` 目录**（递归 `.rs`）—— **不是某一个文件**
// ⇒ ⇒ 实现把那个「失败 → 协议码」的映射**挪到别的文件**（或改名）时，判据
// **不会失能**（失能＝假红，而红的原因与本条的语义无关）。
//
// ⚠️ **只认「目录链」那个映射**（`fn code(&self)` 一类）⇒ ⇒ **不误抽正文链的
// `capture_code`**（协议 §4.4 那条链的映射，**语义不同**：`Return`/`Timeout`
// ⇒ `timeout`，而目录链的 `Timeout` ⇒ `catalog_failed`）⇒ ⇒ 抽错对象会让
// 「还没抓到结构＝catalog_failed」那条**假红**。
//
// ## 抽取器（为什么不是「找函数头再往后切一段」）
//
// 旧写法按「下一个 `fn `」切 ⇒ ⇒ **会吞进后面的方法与注释** ⇒ ⇒ 注释里出现的
// `=> "x"`（本仓就有：讲「判据按 `=> "x"` 形态读」的那句）被当成 match arm
// ⇒ ⇒ **把前面所有变体的映射覆盖成 `x`** ⇒ ⇒ 症状＝「抽到 N 个分支，值全是 x」。
//
// ⇒ ⇒ 现在**逐行**抽：**注释行跳过** ＋ **与函数头同缩进的 `}` 即止** ＋ 每个
// `=> "码"` 把它上面那些 `CaptureFailure::X` 一起赋成这个码。
//
// @module scripts/check-official-chat-p4-errors

import { readdirSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import {
  FAIL,
  PASS,
  P4_SCAN_FILES,
  SKIPPED,
  argValue,
  emit,
  judgeChecks,
  readSources,
  repoRoot,
  runScenarios,
} from './lib/p4-source-shape.mjs';

// 三态常量**从共用原语转出去**（自测文件从本模块取它们）。
export { FAIL, PASS, SKIPPED };

/** P1 协议正文（相对仓库根；**只读**）。 */
export const P1_PROTOCOL_REL = 'docs/features/official-chat/official-chat-p1-protocol.md';

/**
 * 壳侧取数模块**所在目录**（相对仓库根）—— 第 7 条的**读取面跟着实现走**。
 *
 * ⚠️ **不再硬编码某一个 `.rs` 文件**：实现把那个映射挪到别的文件（或改名）时，
 * 判据**不该失能**（那会变成「假红」，而红的原因跟判据的语义无关）。
 */
export const RUST_DIR = 'src-tauri/src/official_chat';

/**
 * 从 **P1 协议 §3 的表**里抽错误码闭集（**契约源**，不抄实现）。
 *
 * @param {string} root - 仓库根。
 * @returns {string[]} 码名（抽不到 ⇒ 空数组）。
 */
export function readClosedSet(root) {
  let md = '';
  try {
    md = readFileSync(join(root, P1_PROTOCOL_REL), 'utf8');
  } catch {
    return [];
  }
  const start = md.indexOf('## 3. 错误码');
  if (start < 0) return [];
  const rest = md.slice(start);
  const end = rest.indexOf('\n## ', 4);
  const section = end < 0 ? rest : rest.slice(0, end);
  const codes = [];
  for (const m of section.matchAll(/^\|\s*`([a-z_]{3,})`\s*\|/gm)) codes.push(m[1]);
  return codes;
}

/**
 * **目录链**那个「失败 → 协议码」映射的**方法名候选**。
 *
 * ⚠️ **故意不含 `capture_code`** —— 那是**正文那条链**（协议 §4.4）的映射，
 * **语义不同**（`Return`/`Timeout` ⇒ `timeout`，而目录链的 `Timeout` ⇒
 * `catalog_failed`）⇒ ⇒ 抽错对象会让「还没抓到结构＝catalog_failed」那条**假红**
 * ⇒ ⇒ 本判据只认**目录链**那个。
 */
export const MAPPING_FN_CANDIDATES = Object.freeze([
  'code',
  'catalog_code',
  'dir_code',
  'structure_code',
  'session_code',
]);

/**
 * 抽「失败种类 → 协议码」的映射（壳侧那一段 `fn code(&self)` 那种方法）。
 *
 * ## 为什么不用「找 `fn code(&self)` 再往后切一段」
 *
 * 旧写法按「下一个 `fn `」切函数体 ⇒ ⇒ **会吞进后面的方法与注释** ⇒ ⇒ 注释里
 * 出现的 `=> "x"`（本仓就有：讲「判据按 `=> "x"` 形态读」的那句）会被当成
 * match arm ⇒ ⇒ **把前面所有变体的映射覆盖成 `x`** ⇒ ⇒ 症状＝「抽到 7 个分支，
 * 值全是 x」。
 *
 * ⇒ ⇒ 现在**逐行**抽：① **注释行直接跳过**（`//` / `///` / `*` 开头）② 遇到
 * **与函数头同缩进的 `}`** 即止（那是方法结束）③ 每个 `=> "码"` 把它**上面**那些
 * `CaptureFailure::X` 一起赋成这个码（一个 arm 可以是 `A | B => "码"`）。
 *
 * @param {string} src - Rust 源码。
 * @param {string} [fnName] - 方法名（默认 `code`）。
 * @returns {Record<string, string>} 映射（抽不到 ⇒ 空对象）。
 */
export function extractMapping(src, fnName = 'code') {
  const lines = String(src ?? '').split(/\r?\n/);
  const re = new RegExp(`fn\\s+${fnName}\\s*\\(\\s*&self\\s*\\)`);
  const startIdx = lines.findIndex((line) => re.test(line));
  if (startIdx < 0) return {};
  const indent = (lines[startIdx].match(/^\s*/) ?? [''])[0];
  const endRe = new RegExp(`^${indent}\\}`);

  const mapping = {};
  let pending = [];
  for (let i = startIdx; i < lines.length; i += 1) {
    const raw = lines[i];
    if (i > startIdx && endRe.test(raw)) break; // 方法结束
    const line = raw.trim();
    if (line === '' || line.startsWith('//') || line.startsWith('*') || line.startsWith('#[')) {
      continue; // 注释／属性宏行 ⇒ 不当代码读
    }
    for (const k of line.matchAll(/CaptureFailure::(\w+)/g)) pending.push(k[1]);
    const m = /=>\s*"([a-z_]+)"/.exec(line);
    if (m) {
      for (const k of pending) mapping[k] = m[1];
      pending = [];
    }
  }
  return mapping;
}

/**
 * 读**目录**里的全部 `.rs`（**读取面跟着实现走** ⇒ 实现再挪位置也不会失能）。
 *
 * @param {string} root - 仓库根。
 * @param {string} [dirRel] - 目录（相对仓库根）。
 * @returns {Array<{path: string, code: string}>}
 */
export function readRustDir(root, dirRel = RUST_DIR) {
  const abs = join(root, dirRel);
  let names = [];
  try {
    names = readdirSync(abs, { recursive: true, encoding: 'utf8' });
  } catch {
    return [];
  }
  const out = [];
  for (const name of names.filter((n) => String(n).endsWith('.rs')).sort()) {
    const rel = `${dirRel}/${String(name).replace(/\\/g, '/')}`;
    try {
      out.push({ path: rel, code: readFileSync(join(root, rel), 'utf8') });
    } catch {
      /* 读不动的跳过 */
    }
  }
  return out;
}

/**
 * 在**目录**里找「目录链的失败 → 协议码」映射。
 *
 * ⚠️ **先按方法名候选找**（`code` 等）⇒ ⇒ **不误抽正文链的 `capture_code`**
 * （那个方法名不在候选里，且 `fn capture_code(&self)` 也匹配不上 `fn code(&self)`）。
 * ⇒ ⇒ 抽到的映射**变体最多**的那份算数；一份都抽不到 ⇒ 空对象（判据如实报
 * 「没抽到」⇒ 未执行，**不假绿**）。
 *
 * @param {string} root - 仓库根。
 * @returns {{mapping: Record<string, string>, from: string, files: Array<{path: string, code: string}>}}
 */
export function findMapping(root) {
  const files = readRustDir(root);
  let best = { mapping: {}, from: '' };
  for (const file of files) {
    for (const fnName of MAPPING_FN_CANDIDATES) {
      const mapping = extractMapping(file.code, fnName);
      if (Object.keys(mapping).length > Object.keys(best.mapping).length) {
        best = { mapping, from: `${file.path}（fn ${fnName}）` };
      }
    }
  }
  return { ...best, files };
}

/**
 * 判定内核。
 *
 * @param {object} reading - 读数（`mapping` / `closedSet`）。
 * @returns {{ok: boolean, checks: Array<object>}}
 */
export function judgeErrors(reading) {
  const checks = [];
  const add = (name, status, detail) => checks.push({ name, status, detail });
  const mapping = reading?.mapping && typeof reading.mapping === 'object' ? reading.mapping : {};
  const closedSet = Array.isArray(reading?.closedSet) ? reading.closedSet : [];

  if (Object.keys(mapping).length === 0) {
    add('P4-7 失败 → 协议码的映射', SKIPPED, '没抽到映射 ⇒ 判据未执行');
    return judgeChecks(checks);
  }
  add(
    'P4-7 失败 → 协议码的映射抽得到',
    PASS,
    `抽到 ${Object.keys(mapping).length} 个分支：${Object.keys(mapping).join('、')}` +
      (reading?.from ? `（抽自 ${reading.from}）` : ''),
  );

  const returnCode = mapping.Return;
  const emptyCode = mapping.Empty;
  const timeoutCode = mapping.Timeout;
  add(
    'P4-7 **回程失败 ≠ 站点返回空**（两个不同的既有码，§6.3／§10 第 7 条）',
    returnCode !== undefined && emptyCode !== undefined && returnCode !== emptyCode ? PASS : FAIL,
    `回程失败=${returnCode ?? '（没抽到）'}／站点返回空=${emptyCode ?? '（没抽到）'}`,
  );

  add(
    'P4-7 超时也是**第三类**（与站点返回空不同）',
    timeoutCode !== undefined && emptyCode !== undefined && timeoutCode !== emptyCode ? PASS : FAIL,
    `超时=${timeoutCode ?? '（没抽到）'}／站点返回空=${emptyCode ?? '（没抽到）'}`,
  );

  add(
    'P4-7 **「还没抓到结构」落 `catalog_failed`**（v0.11 §8.5.2 G4-14 ③；绝不 `catalog_empty`）',
    timeoutCode === 'catalog_failed' ? PASS : FAIL,
    `「还没抓到结构」=${timeoutCode ?? '（没抽到）'}（必须 ＝ catalog_failed，且 ≠ catalog_empty）`,
  );

  if (closedSet.length === 0) {
    add('P4-7 那两个码都在 P1 §3 的闭集里', SKIPPED, '没读到 P1 §3 的闭集 ⇒ 判据未执行');
  } else {
    const used = [returnCode, emptyCode, timeoutCode].filter((c) => c !== undefined);
    const outside = used.filter((c) => !closedSet.includes(c));
    add(
      'P4-7 那两个码都在 P1 §3 的闭集里（§6.1：不扩闭集）',
      outside.length === 0 ? PASS : FAIL,
      outside.length === 0
        ? `用到的码（${used.join('、')}）全在 ${closedSet.length} 码闭集内`
        : `闭集外的码：${outside.join('、')}`,
    );
  }

  const siteCode = mapping.SiteCode;
  add(
    'P4-7 站内码也归到**既有码**（§6.2：原样上报、不解释语义）',
    siteCode !== undefined && closedSet.includes(siteCode) ? PASS : FAIL,
    `站内码映射到 ${siteCode ?? '（没抽到）'}`,
  );

  return judgeChecks(checks);
}

/**
 * 场景表（合成读数）。
 *
 * @returns {Array<object>}
 */
export function buildScenarios(closedSet = ['catalog_empty', 'catalog_failed', 'schema_changed', 'source_unsupported']) {
  const clean = {
    mapping: {
      Return: 'catalog_failed',
      Timeout: 'catalog_failed',
      Empty: 'catalog_empty',
      Schema: 'schema_changed',
      SiteCode: 'catalog_failed',
      Unsupported: 'source_unsupported',
    },
    closedSet: [...closedSet],
  };
  return [
    { id: 'A', name: '场景A 真身映射（回程失败 ＝ catalog_failed，站点返回空 ＝ catalog_empty）⇒ 应通过', expect: PASS, reading: clean },
    {
      id: 'B',
      name: '场景B 两类错误**合并成同一个码** ⇒ 应失败',
      expect: FAIL,
      reading: { ...clean, mapping: { ...clean.mapping, Empty: 'catalog_failed' } },
    },
    {
      id: 'C',
      name: '场景C 映射到**闭集外**的码 ⇒ 应失败',
      expect: FAIL,
      reading: { ...clean, mapping: { ...clean.mapping, Empty: 'catalog_void' } },
    },
    {
      id: 'D',
      name: '场景D 「还没抓到结构」也用成 `catalog_empty` ⇒ 应失败',
      expect: FAIL,
      reading: { ...clean, mapping: { ...clean.mapping, Timeout: 'catalog_empty' } },
    },
    {
      id: 'E',
      name: '场景E 抽不到映射 ⇒ 报「未执行」（不是失败）',
      expect: SKIPPED,
      reading: { ...clean, mapping: {} },
    },
    {
      id: 'F',
      name: '场景F 「还没抓到结构」落成别的可重试码（不是 catalog_failed）⇒ 应失败',
      expect: FAIL,
      reading: { ...clean, mapping: { ...clean.mapping, Timeout: 'timeout' } },
    },
  ];
}

/**
 * 读真实仓库的读数。
 *
 * ⚠️ **读取面 ＝ `src-tauri/src/official_chat/` 整个目录**（不是某一个文件）⇒ ⇒
 * 实现把那个映射挪到别的文件（或改名）时，**判据不会失能**。
 *
 * @param {string} root - 仓库根。
 * @returns {{mapping: object, closedSet: string[], from: string, files: Array<object>}}
 */
export function readReading(root) {
  const { mapping, from, files } = findMapping(root);
  return { mapping, closedSet: readClosedSet(root), from, files };
}

/**
 * 主入口。
 *
 * @param {string[]} [argv] - 命令行参数。
 * @returns {Promise<{results: Array<object>, summary: object}>}
 */
export async function main(argv = process.argv.slice(2)) {
  const root = argValue(argv, '--target') ?? repoRoot(import.meta.url);
  const round = argValue(argv, '--round') ?? 'p4c7';
  const asJson = argv.includes('--json');

  const sources = readSources(root, P4_SCAN_FILES);
  const reading = readReading(root);
  const { ok, checks } = judgeErrors(reading);

  const results = [];
  for (const c of checks) results.push({ name: c.name, status: c.status, detail: c.detail });
  results.push({
    name: 'P4-7 真实仓库读数的判定',
    status: ok ? PASS : FAIL,
    detail: ok ? '真实读数全过' : '真实读数未全过（见上）',
  });
  for (const r of runScenarios(buildScenarios(reading.closedSet), judgeErrors, 'P4-7a')) {
    results.push({ name: r.name, status: r.status, detail: r.detail });
  }

  return emit({
    round,
    // **扫描面 ＝ 实际读到的文件**：目录里的 `.rs` ＋ P1 协议正文（闭集来源）
    // ⇒ ⇒ 「扫了 N 个文件」与真实读取面一致（PM 点名过：标签与实际不一致要修）。
    sources: [...reading.files, { path: P1_PROTOCOL_REL, code: '' }],
    scanLabel: `P4 实现（\`${RUST_DIR}/\` 目录里的 Rust 源）＋ P1 协议 §3 的闭集（只读）`,
    reading,
    results,
    evidenceDir: join(tmpdir(), 'dsh-p4-errors'),
    asJson,
  });
}

// 直接跑（被 import 时不跑）
if (process.argv[1] && process.argv[1].endsWith('check-official-chat-p4-errors.mjs')) {
  await main();
}
