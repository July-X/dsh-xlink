// P4 §10 判据**第 6 条**（错误面 · 13 码闭集）的**自测**。
//
// ## 判什么
//
// · **期望清单本身要对着协议核**：脚本里的 `EXPECTED_CODES` ＝ **P1 协议 §3 的表**
//   （`official-chat-p1-protocol.md`）⇒ 协议表一改 ⇒ **自测当场红**（防手抄错、防漂移）。
// · **场景表必须自证**：A（真身）通过 · B（**改一个码名但长度不变**）失败 ·
//   C（`match` 分支塞闭集外硬编码码）失败 · D（少一个码）失败 · E（多一个码）失败。
// · **抽取器不许把噪声当码**：注释里的 `return "x"` 不算 ｜ match arm **匹配侧**的原因词
//   （`timed_out` / `deadline_exceeded` …）不算码返回点（只有 **arm 的返回值**算）。
// · **真实仓库**：两侧闭集逐项相等 ＋ 长度 13 ＋ 无重复 ＋ JS 映射键＝名字数组
//   ＋ 所有码返回点 ∈ 闭集。
//
// ## 纪律
//
// · 夹具全在内存里（不落盘）；**永不碰用户的真实数据目录**、不发任何网络请求。
//
// @module scripts/check-official-chat-p4-codes-test

import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

import {
  EXPECTED_CODES,
  FAIL,
  JS_CODES_REL,
  PASS,
  RUST_CODES_REL,
  SKIPPED,
  buildScenarios,
  collectSources,
  extractCodeReturns,
  extractJsMapKeys,
  extractJsNames,
  extractRustCodes,
  judgeCodes,
  main,
  runScenarios,
  summarize,
} from './check-official-chat-p4-codes.mjs';

const REPO = fileURLToPath(new URL('../', import.meta.url));

/**
 * 找某条判据的结果。
 *
 * @param {Array<{name: string, status: string, detail: string}>} results
 * @param {string} keyword - 判据名关键词。
 * @returns {{name: string, status: string, detail: string}}
 */
function pick(results, keyword) {
  const found = results.find((r) => r.name.includes(keyword));
  assert.ok(found, `判据结果里找不到「${keyword}」`);
  return found;
}

test('期望清单 · 与 P1 协议 §3 的表逐项相等（协议是契约源，漂移即红）', () => {
  const md = readFileSync(
    join(REPO, 'docs/features/official-chat/official-chat-p1-protocol.md'),
    'utf8',
  );
  // 只取 §3 那一节（整篇文档别的表也有反引号首格 ⇒ 必须按节切）
  const sec = /##\s*3\.\s*错误码([\s\S]*?)(?=\n##\s|\n---)/.exec(md);
  assert.ok(sec, 'P1 协议里找不到 §3 错误码这一节');
  // §3 的表：`| \`code\` | 含义 | 可重试 | 用户下一步 |`
  const tableCodes = [];
  for (const m of sec[1].matchAll(/^\|\s*`([a-z_]{2,})`\s*\|/gm)) tableCodes.push(m[1]);
  assert.equal(tableCodes.length, 13, `P1 §3 表应恰好 13 行码（读到 ${tableCodes.length} 行）`);
  assert.deepEqual(
    [...tableCodes].sort(),
    [...EXPECTED_CODES].sort(),
    '脚本里的 EXPECTED_CODES 必须与 P1 协议 §3 的表一致（抄协议，不抄实现）',
  );
});

test('场景表 · 5 个场景的判定全部符合预期（含「改一个码名但长度不变 ⇒ 必红」）', () => {
  const scenarios = buildScenarios();
  assert.equal(scenarios.length, 5);
  const results = runScenarios(scenarios);
  for (const r of results) {
    assert.equal(r.status, PASS, `${r.name} 未按预期：${r.detail}`);
  }
  assert.equal(summarize(results).fail, 0);
});

test('场景 B · 改一个码名但长度不变 ⇒ 必须判红（PM 点名的那条）', () => {
  const sc = buildScenarios().find((s) => s.id === 'B');
  const { ok } = judgeCodes(sc.reading);
  assert.equal(ok, false, '长度不变但改了一个码名 ⇒ 必须判红');
  const { checks } = judgeCodes(sc.reading);
  assert.ok(
    checks.some((c) => c.status === FAIL && c.name.includes('逐项')),
    '必须由「逐项相等」这条断言报红（不是只报长度）',
  );
});

test('场景 C · match 分支塞闭集外硬编码码 ⇒ 必须判红（并点名文件:行）', () => {
  const sc = buildScenarios().find((s) => s.id === 'C');
  const { ok, checks } = judgeCodes(sc.reading);
  assert.equal(ok, false);
  const hit = checks.find((c) => c.name.includes('码返回点'));
  assert.equal(hit.status, FAIL);
  assert.match(hit.detail, /not_found/, '必须点名那个闭集外的码');
  assert.match(hit.detail, /fixture\.rs:\d+/, '必须点名文件与行号');
});

test('抽取器 · Rust 闭集常量（真实文件）', () => {
  const src = readFileSync(join(REPO, RUST_CODES_REL), 'utf8');
  const { codes, declaredLen } = extractRustCodes(src);
  assert.equal(declaredLen, 13, '常量里写的长度必须是 13');
  assert.equal(codes.length, 13);
  assert.deepEqual([...codes].sort(), [...EXPECTED_CODES].sort());
});

test('抽取器 · JS 侧名字数组与映射键（真实文件）', () => {
  const src = readFileSync(join(REPO, JS_CODES_REL), 'utf8');
  const names = extractJsNames(src);
  const keys = extractJsMapKeys(src);
  assert.equal(names.length, 13);
  assert.equal(keys.length, 13);
  assert.deepEqual([...names].sort(), [...EXPECTED_CODES].sort());
  assert.deepEqual([...keys].sort(), [...names].sort(), '映射键集合必须与名字数组一致');
});

test('抽取器 · 注释里的码返回点不算（判据只看真代码）', () => {
  const src = ['// return "not_found";', '/* return "also_not"; */', 'fn f() { return "timeout"; }'].join(
    '\n',
  );
  const hits = extractCodeReturns(src, 'fixture.rs');
  assert.equal(hits.length, 1, '只应认真代码里的那一处');
  assert.equal(hits[0].value, 'timeout');
  assert.equal(hits[0].line, 3);
});

test('抽取器 · match arm 的匹配侧原因词不算码返回点（只有返回值算）', () => {
  const src = [
    'match raw.as_str() {',
    '    "timeout" | "timed_out" | "deadline_exceeded" => "timeout",',
    '    _ => "timeout",',
    '}',
  ].join('\n');
  const hits = extractCodeReturns(src, 'fixture.rs');
  const values = hits.map((h) => h.value);
  assert.deepEqual(values, ['timeout', 'timeout'], '只认 arm 的返回值');
  assert.ok(!values.includes('timed_out'), '匹配侧的原因词不在闭集里，不能算码返回点');
});

test('真实仓库 · 两侧闭集逐项相等 ＋ 长度 13 ＋ 无重复 ＋ 码返回点全在闭集内', () => {
  const sources = collectSources(REPO);
  const rustSrc = sources.find((s) => s.path === RUST_CODES_REL)?.code ?? '';
  const jsSrc = sources.find((s) => s.path === JS_CODES_REL)?.code ?? '';
  const rust = extractRustCodes(rustSrc);
  const codeReturns = sources.flatMap((s) => extractCodeReturns(s.code, s.path));
  const { ok, checks } = judgeCodes({
    rustCodes: rust.codes,
    rustDeclaredLen: rust.declaredLen,
    jsNames: extractJsNames(jsSrc),
    jsMapKeys: extractJsMapKeys(jsSrc),
    codeReturns,
  });
  assert.equal(ok, true, `真实读数未全过：${JSON.stringify(checks.filter((c) => c.status === FAIL))}`);
  for (const c of checks) {
    assert.notEqual(c.status, SKIPPED, `真实仓库里不该有「未执行」：${c.name}`);
  }
  assert.ok(codeReturns.length > 0, '真实扫描面里应该能抽到码返回点');
});

test('真实仓库 · 扫描面文件数达标（防「扫了 0 个文件却报绿」）', () => {
  const sources = collectSources(REPO);
  assert.ok(sources.length >= 5, `扫描面至少应有 5 个官方对话 .rs ＋ 1 个 JS（实际 ${sources.length}）`);
  assert.ok(
    sources.some((s) => s.path === RUST_CODES_REL),
    '必须扫到 Rust 侧闭集真身',
  );
  assert.ok(sources.some((s) => s.path === JS_CODES_REL), '必须扫到 JS 侧闭集副本');
});

test('汇总 · summarize 的计数与 P1/P2/P3 同构', () => {
  assert.deepEqual(
    summarize([
      { status: PASS },
      { status: FAIL },
      { status: SKIPPED },
      { status: PASS },
    ]),
    { total: 4, pass: 2, fail: 1, skipped: 1 },
  );
});

test('主入口 · 全绿且退出码语义正确（场景级）', async () => {
  const { results, summary } = await main(['--round', 'p4c6-selftest', '--json']);
  assert.equal(summary.fail, 0, `不该有失败：${JSON.stringify(summary)}`);
  assert.ok(results.every((r) => [PASS, FAIL, SKIPPED].includes(r.status)));
  assert.equal(pick(results, '真实仓库读数的判定').status, PASS);
});
