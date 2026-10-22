// P4 判据**第 7 条**（两类错误分离）的自证。
//
// @module scripts/check-official-chat-p4-errors-test

import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';

import {
  PASS,
  SKIPPED,
  buildScenarios,
  extractMapping,
  findMapping,
  judgeErrors,
  readClosedSet,
  readReading,
} from './check-official-chat-p4-errors.mjs';
import { repoRoot } from './lib/p4-source-shape.mjs';

const REPO = repoRoot(import.meta.url);
const CHAT_REL = 'src-tauri/src/official_chat';
const P1_REL = 'docs/features/official-chat/official-chat-p1-protocol.md';

/**
 * 造一个**影子根**：官方对话目录 ＋ P1 协议正文（**只读**，不动工作树）。
 *
 * @param {Record<string, string>} files - 相对影子根的路径 → 内容。
 * @returns {string} 影子根的绝对路径。
 */
function makeShadow(files) {
  const dir = mkdtempSync(join(tmpdir(), 'dsh-p4-errors-shadow-'));
  for (const [rel, content] of Object.entries(files)) {
    const abs = join(dir, rel);
    mkdirSync(join(abs, '..'), { recursive: true });
    writeFileSync(abs, content, 'utf8');
  }
  return dir;
}

/**
 * 把真实实现里那个**目录链**的码映射方法剪出来（按行找方法头 → 同缩进的 `}`）。
 *
 * @returns {{method: string, rest: string}} 方法文本 ＋ 去掉它之后的源码。
 */
function cutMappingMethod(src, fnName = 'code') {
  const lines = src.split(/\r?\n/);
  const re = new RegExp(`fn\\s+${fnName}\\s*\\(\\s*&self\\s*\\)`);
  const start = lines.findIndex((line) => re.test(line));
  if (start < 0) return { method: '', rest: src };
  const indent = (lines[start].match(/^\s*/) ?? [''])[0];
  let end = start + 1;
  while (end < lines.length && !new RegExp(`^${indent}\\}`).test(lines[end])) end += 1;
  return {
    method: lines.slice(start, end + 1).join('\n'),
    rest: [...lines.slice(0, start), ...lines.slice(end + 1)].join('\n'),
  };
}

test('P4-7：场景表逐条自证（回程失败 ≠ 站点返回空 ⇒ 两个不同的既有码）', () => {
  for (const sc of buildScenarios()) {
    const { ok, checks } = judgeErrors(sc.reading);
    const allSkipped = checks.length > 0 && checks.every((c) => c.status === SKIPPED);
    const behaved = sc.expect === SKIPPED ? allSkipped : sc.expect === PASS ? ok : !ok;
    assert.equal(
      behaved,
      true,
      `场景 ${sc.id} 的判定与期望不符：${checks
        .map((c) => `${c.name}=${c.status}（${c.detail}）`)
        .join('；')}`,
    );
  }
});

test('P4-7：闭集**抄自 P1 协议 §3 的表**（不抄实现）—— 14 码', () => {
  const closed = readClosedSet(repoRoot(import.meta.url));
  assert.equal(closed.length, 14, `闭集读到 ${closed.length} 码：${closed.join('、')}`);
  for (const code of ['timeout', 'catalog_empty', 'catalog_failed', 'schema_changed']) {
    assert.ok(closed.includes(code), `闭集里没有 ${code}`);
  }
});

test('P4-7：真实仓库读数全过（映射抽得到 ＋ 两个码不同 ＋ 都在闭集里）', () => {
  const { ok, checks } = judgeErrors(readReading(REPO));
  const bad = checks.filter((c) => c.status !== PASS);
  assert.equal(ok, true, bad.map((c) => `${c.name}：${c.detail}`).join('；'));
});

test('P4-7：抽取器 · **注释里的 `=> "x"` 不许污染映射**（回归点）', () => {
  // 这就是本轮那条红（6 条全 x）的成因：旧写法把函数体后面的**注释**也切了进去
  // ⇒ 注释里那句「判据按 `=> "x"` 形态读」被当成 match arm ⇒ 映射被覆盖成 `x`。
  const src = [
    'impl CaptureFailure {',
    '    pub(crate) fn code(&self) -> &\'static str {',
    '        match self {',
    '            CaptureFailure::Timeout(_) => "catalog_failed",',
    '            CaptureFailure::Empty => "catalog_empty",',
    '            CaptureFailure::Return(_) | CaptureFailure::SiteCode { .. } => "catalog_failed",',
    '        }',
    '    }',
    '    /// 顺带说一句：判据按 `=> "x"` 形态读（**注释里的**，不许当真）。',
    '    pub(crate) fn detail(&self) -> String {',
    '        match self {',
    '            CaptureFailure::Return(d) => format!("回程失败：{d}"),',
    '        }',
    '    }',
    '}',
  ].join('\n');
  const mapping = extractMapping(src, 'code');
  assert.equal(mapping.Timeout, 'catalog_failed');
  assert.equal(mapping.Empty, 'catalog_empty');
  assert.equal(mapping.Return, 'catalog_failed');
  assert.equal(mapping.SiteCode, 'catalog_failed');
  assert.equal(mapping.Schema, undefined, '不该凭空多出分支');
});

test('P4-7：**自证 ①** 在 `fn code` 的函数体里插一段含 `=> "x"` 的注释 ⇒ 读数逐条不变', () => {
  // 判据**不许**把注释里的样例字面量当真码返回点（实现线自己就写了这种说明）。
  const src = readFileSync(join(REPO, CHAT_REL, 'session_structure.rs'), 'utf8');
  const before = judgeErrors(readReading(REPO)).checks.map((c) => `${c.name}=${c.status}|${c.detail}`);

  // 在方法体第一行之后插一段注释（含 `=> "x"` 样例字面量）
  const lines = src.split(/\r?\n/);
  const start = lines.findIndex((l) => /fn\s+code\s*\(\s*&self\s*\)/.test(l));
  assert.ok(start >= 0, '真实实现里要能找到那个方法');
  const patched = [
    ...lines.slice(0, start + 1),
    '        // ⚠️ 顺带说一句：第 6 条的判据把「`=> "x"` 形态的字面量」当码返回点扫（**注释里的**，不许当真）。',
    ...lines.slice(start + 1),
  ].join('\n');

  const dir = makeShadow({
    [`${CHAT_REL}/session_structure.rs`]: patched,
    [P1_REL]: readFileSync(join(REPO, P1_REL), 'utf8'),
  });
  try {
    const after = judgeErrors(readReading(dir)).checks.map((c) => `${c.name}=${c.status}|${c.detail}`);
    assert.deepEqual(after, before, '插了含 `=> "x"` 的注释后，读数必须逐条一模一样');
    assert.equal(judgeErrors(readReading(dir)).ok, true, '插注释后第 7 条必须仍全绿');
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('P4-7：**自证 ②** impl 块里**加**一个 `    fn xxx()` ⇒ 读数逐条不变（边界不再依赖「恰好还有一个 fn」）', () => {
  const src = readFileSync(join(REPO, CHAT_REL, 'session_structure.rs'), 'utf8');
  const before = judgeErrors(readReading(REPO)).checks.map((c) => `${c.name}=${c.status}|${c.detail}`);

  // 在 `fn code` 之后**加**一个缩进 4 的方法（模拟 HEAD 版那个 `fn from_body`）
  const lines = src.split(/\r?\n/);
  const start = lines.findIndex((l) => /fn\s+code\s*\(\s*&self\s*\)/.test(l));
  assert.ok(start >= 0, '真实实现里要能找到那个方法');
  const patched = [
    ...lines.slice(0, start + 1),
    '    fn xxx() -> u8 { 7 }',
    ...lines.slice(start + 1),
  ].join('\n');

  const dir = makeShadow({
    [`${CHAT_REL}/session_structure.rs`]: patched,
    [P1_REL]: readFileSync(join(REPO, P1_REL), 'utf8'),
  });
  try {
    const after = judgeErrors(readReading(dir)).checks.map((c) => `${c.name}=${c.status}|${c.detail}`);
    assert.deepEqual(after, before, '加了一个方法后，读数必须逐条一模一样');
    assert.equal(judgeErrors(readReading(dir)).ok, true, '加方法后第 7 条必须仍全绿');
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('P4-7：边界 ＝ **大括号配平**（impl 块里还剩几个方法都无所谓）', () => {
  const src = [
    'impl CaptureFailure {',
    '    pub(crate) fn code(&self) -> &\'static str {',
    '        match self {',
    '            CaptureFailure::Timeout(_) => "catalog_failed",',
    '            // 注释里也有 `=> "x"`（不许当真）。',
    '            CaptureFailure::Empty => "catalog_empty",',
    '            CaptureFailure::Return(_) | CaptureFailure::SiteCode { .. } => "catalog_failed",',
    '        }',
    '    }',
    '    fn from_body(detail: String) -> Self {',
    '        Self::Return(detail)',
    '    }',
    '    fn ok_body() { let s = "{ not a brace"; }',
    '}',
  ].join('\n');
  const mapping = extractMapping(src, 'code');
  assert.deepEqual(
    mapping,
    { Timeout: 'catalog_failed', Empty: 'catalog_empty', Return: 'catalog_failed', SiteCode: 'catalog_failed' },
    '只应抽到 `fn code` 里的四条臂（后面方法里的 `=>` 不许串味）',
  );
});

test('P4-7：映射**挪到别的文件** ⇒ 判据仍抽得到、本体仍全绿（读取面跟着实现走）', () => {
  const src = readFileSync(join(REPO, CHAT_REL, 'session_structure.rs'), 'utf8');
  const { method, rest } = cutMappingMethod(src, 'code');
  assert.ok(method.includes('fn code(&self)'), '真实实现里要能找到那个方法（实现改名了就先改候选名）');

  const dir = makeShadow({
    [`${CHAT_REL}/session_structure.rs`]: rest,
    [`${CHAT_REL}/zz_moved_mapping.rs`]: `impl CaptureFailure {\n${method}\n}\n`,
    [P1_REL]: readFileSync(join(REPO, P1_REL), 'utf8'),
  });
  try {
    const { mapping, from } = findMapping(dir);
    assert.ok(
      Object.keys(mapping).length >= 4,
      `挪位置后仍要抽得到映射（实际 ${JSON.stringify(mapping)}）`,
    );
    assert.match(from, /zz_moved_mapping\.rs/, `映射应抽自挪过去的那个文件（实际 ${from}）`);
    const { ok, checks } = judgeErrors(readReading(dir));
    assert.equal(ok, true, `挪位置后第 7 条本体仍全绿：${checks.map((c) => `${c.name}：${c.detail}`).join('；')}`);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('P4-7：**正文链**那个 `capture_code`（语义不同）不许被当成目录链的映射', () => {
  // 正文链（协议 §4.4）的 `capture_code`：`Return`/`Timeout` ⇒ `timeout`，
  // 而目录链的 `Timeout` ⇒ `catalog_failed` ⇒ ⇒ 抽错对象会让
  // 「还没抓到结构＝catalog_failed」那条**假红** ⇒ ⇒ 只剩 capture_code 时
  // 必须报「没抽到」（未执行），而不是拿它去判红。
  const src = readFileSync(join(REPO, CHAT_REL, 'session_structure.rs'), 'utf8');
  const { rest } = cutMappingMethod(src, 'code');
  const body = readFileSync(join(REPO, CHAT_REL, 'session_body.rs'), 'utf8');
  const dir = makeShadow({
    [`${CHAT_REL}/session_structure.rs`]: rest,
    [`${CHAT_REL}/session_body.rs`]: body,
    [P1_REL]: readFileSync(join(REPO, P1_REL), 'utf8'),
  });
  try {
    const { mapping } = findMapping(dir);
    assert.equal(
      Object.keys(mapping).length,
      0,
      `只剩正文链的 capture_code 时不该当成目录链的映射（实际 ${JSON.stringify(mapping)}）`,
    );
    const { ok, checks } = judgeErrors(readReading(dir));
    assert.equal(ok, false, '抽不到目录链的映射 ⇒ 判据不该判绿');
    assert.ok(
      checks.every((c) => c.status === SKIPPED),
      `抽不到映射时应报「未执行」而不是「失败」（实际 ${checks.map((c) => `${c.name}=${c.status}`).join('；')}）`,
    );
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
