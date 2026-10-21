// P4 判据**第 10 条**（取体时序）的自证。
//
// · **场景表逐条自证** —— 判据自己得先证明「该红就红、该绿就绿」；
// · **真实仓库读数全过** —— 真身必须过（不然就是「判据自己先红」）。
//
// @module scripts/check-official-chat-p4-body-timing-test

import assert from 'node:assert/strict';
import test from 'node:test';

import {
  PASS,
  buildScenarios,
  judgeBodyTiming,
  readReading,
} from './check-official-chat-p4-body-timing.mjs';
import { repoRoot } from './lib/p4-source-shape.mjs';

test('P4-10：场景表逐条自证（该红就红、该绿就绿）', () => {
  for (const sc of buildScenarios()) {
    const { ok, checks } = judgeBodyTiming(sc.reading);
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

test('P4-10：真实仓库读数全过（等 loadingFinished → 再取体 ＋ 空体有专门处理）', () => {
  const { ok, checks } = judgeBodyTiming(readReading(repoRoot(import.meta.url)));
  const bad = checks.filter((c) => c.status !== PASS);
  assert.equal(ok, true, bad.map((c) => `${c.name}：${c.detail}`).join('；'));
});
