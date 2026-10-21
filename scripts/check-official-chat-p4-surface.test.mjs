// P4 判据**第 5 条**（能力面）的自证。
//
// @module scripts/check-official-chat-p4-surface-test

import assert from 'node:assert/strict';
import test from 'node:test';

import { PASS, buildScenarios, judgeSurface, readReading } from './check-official-chat-p4-surface.mjs';
import { repoRoot } from './lib/p4-source-shape.mjs';

test('P4-5：场景表逐条自证（常量脚本 ＋ 命令白名单 ＋ 只碰官方对话 ＋ 不写站点数据）', () => {
  for (const sc of buildScenarios()) {
    const { ok, checks } = judgeSurface(sc.reading);
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

test('P4-5：真实仓库读数全过', () => {
  const { ok, checks } = judgeSurface(readReading(repoRoot(import.meta.url)));
  const bad = checks.filter((c) => c.status !== PASS);
  assert.equal(ok, true, bad.map((c) => `${c.name}：${c.detail}`).join('；'));
});
