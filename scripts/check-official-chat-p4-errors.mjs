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
// @module scripts/check-official-chat-p4-errors

import { readFileSync } from 'node:fs';
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

/** 壳侧取数模块（相对仓库根）。 */
const RUST_REL = 'src-tauri/src/official_chat/session_structure.rs';

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
 * 抽「失败种类 → 协议码」的映射（壳侧那一段 `fn code(&self)`）。
 *
 * @param {string} src - Rust 源码。
 * @returns {Record<string, string>} 映射（抽不到 ⇒ 空对象）。
 */
export function extractMapping(src) {
  const text = String(src ?? '');
  const at = text.indexOf('fn code(&self)');
  if (at < 0) return {};
  const rest = text.slice(at + 1);
  // **只取这一个方法**：再往后是 `detail()`，那里也有 `=>` ⇒ 不切会串味。
  const end = rest.indexOf('\n    fn ');
  const body = end < 0 ? rest : rest.slice(0, end);
  const mapping = {};
  // 一个 match arm 可以是 `A | B => "code"` ⇒ **按 `=>` 分段**，
  // 再把这一段里出现的每个 `CaptureFailure::X` 都记成同一个码。
  const re = /=>\s*"([a-z_]+)"/g;
  let previous = 0;
  let m;
  while ((m = re.exec(body)) !== null) {
    const arm = body.slice(previous, m.index);
    for (const k of arm.matchAll(/CaptureFailure::(\w+)/g)) mapping[k[1]] = m[1];
    previous = m.index + m[0].length;
  }
  return mapping;
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
    `抽到 ${Object.keys(mapping).length} 个分支：${Object.keys(mapping).join('、')}`,
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
 * @param {string} root - 仓库根。
 * @returns {{mapping: object, closedSet: string[]}}
 */
export function readReading(root) {
  const sources = readSources(root, P4_SCAN_FILES);
  const rust = sources.find((s) => s.path === RUST_REL)?.code ?? '';
  return { mapping: extractMapping(rust), closedSet: readClosedSet(root) };
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
    sources,
    scanLabel: 'P4 实现（壳侧取数模块）＋ P1 协议 §3 的闭集（只读）',
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
