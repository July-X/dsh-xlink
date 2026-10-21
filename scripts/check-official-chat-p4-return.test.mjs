// P4 判据**第 3 条**（回程只回结构）的自证。
//
// @module scripts/check-official-chat-p4-return-test

import assert from 'node:assert/strict';
import test from 'node:test';

import { FORBIDDEN, PASS, WHITELIST, buildScenarios, judgeReturn, readReading } from './check-official-chat-p4-return.mjs';
import { repoRoot } from './lib/p4-source-shape.mjs';

test('P4-3：场景表逐条自证（白名单之外即红）', () => {
  for (const sc of buildScenarios()) {
    const { ok, checks } = judgeReturn(sc.reading);
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

test('P4-3：白名单里**没有**禁回传的字段（正文/标题/会话 id/凭据）', () => {
  for (const key of FORBIDDEN) {
    assert.ok(!WHITELIST.includes(key), `白名单里混进了 ${key}`);
  }
});

test('P4-3：真实仓库读数全过（页内 ＝ 壳侧 ＝ 白名单）', () => {
  const { ok, checks } = judgeReturn(readReading(repoRoot(import.meta.url)));
  const bad = checks.filter((c) => c.status !== PASS);
  assert.equal(ok, true, bad.map((c) => `${c.name}：${c.detail}`).join('；'));
});
