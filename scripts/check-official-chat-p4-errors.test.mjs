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
