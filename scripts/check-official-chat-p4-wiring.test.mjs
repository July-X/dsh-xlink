// P4 判据**第 4 条**（接线）的自证。
//
// @module scripts/check-official-chat-p4-wiring-test

import assert from 'node:assert/strict';
import test from 'node:test';

import { PASS, buildScenarios, judgeWiring, readReading } from './check-official-chat-p4-wiring.mjs';
import { repoRoot } from './lib/p4-source-shape.mjs';

test('P4-4：场景表逐条自证（现役桥 ＋ POST ＋ 读体 ＋ 上限 ＋ 不新端口）', () => {
  for (const sc of buildScenarios()) {
    const { ok, checks } = judgeWiring(sc.reading);
    const behaved = sc.expect === PASS ? ok : !ok;
    assert.equal(
      behaved,
      true,
      `场景 ${sc.id} 的判定与期望不符：${checks
        .map((c) => `${c.name}=${c.status}（${c.detail}）`)
        .join('；')}`,
    );
  }
});

test('P4-4：真实仓库读数全过', () => {
  const { ok, checks } = judgeWiring(readReading(repoRoot(import.meta.url)));
  const bad = checks.filter((c) => c.status !== PASS);
  assert.equal(ok, true, bad.map((c) => `${c.name}：${c.detail}`).join('；'));
});
