// P4 判据**第 7 条**（两类错误分离）的自证。
//
// @module scripts/check-official-chat-p4-errors-test

import assert from 'node:assert/strict';
import test from 'node:test';

import { PASS, SKIPPED, buildScenarios, judgeErrors, readClosedSet, readReading } from './check-official-chat-p4-errors.mjs';
import { repoRoot } from './lib/p4-source-shape.mjs';

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
  const { ok, checks } = judgeErrors(readReading(repoRoot(import.meta.url)));
  const bad = checks.filter((c) => c.status !== PASS);
  assert.equal(ok, true, bad.map((c) => `${c.name}：${c.detail}`).join('；'));
});
