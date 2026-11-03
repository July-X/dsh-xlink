import assert from 'node:assert/strict';
import test from 'node:test';

import {
  MAX_INJECTIONS_PER_SESSION,
  MAX_TOTAL_BYTES_PER_SESSION,
  SKIP_REASONS,
  classifyNote,
  skipBudgetReason,
} from '../host/policy.js';

/**
 * 「为什么没注入」的桶：同回合去抖与会话预算用完必须是两个词。
 *
 * 这两个成因原先混在一个 `budget` 桶里，而实测账本里的 10 次 `budget` 没有一次是真的预算
 * 用完——全是「同一回合已经查过」（内核 pre-step 每个 step 触发一次，第 2 个 step 起
 * `state.turn === turn`）。混着记，面板上的「预算用尽」就一直亮着。
 *
 * 桶名是**跨进程契约**（壳侧账本 `reason`、面板中文标签同一套词），所以这里逐字钉住。
 */

test('未注入原因桶：同回合去抖与预算用完分开记', () => {
  const state = { turn: 7, injections: 2, bytes: 1024 };

  // 同回合：pre-step 的第 2 个 step 起命中，这是去抖，不是预算。
  assert.equal(skipBudgetReason(state, 7), 'already_queried_this_turn', '同一回合已查过');

  // 换回合且预算都没到顶：仍归预算桶（生产里这一步会问桥，走不到这里）。
  assert.equal(skipBudgetReason(state, 8), 'budget_exhausted', '不是同回合就算预算这一侧');

  // 次数到顶。
  assert.equal(
    skipBudgetReason({ turn: 7, injections: MAX_INJECTIONS_PER_SESSION, bytes: 0 }, 8),
    'budget_exhausted',
    '注入次数到上限 = 预算用完'
  );

  // 累计字节到顶（次数还够也会先撞上字节闸）。
  assert.equal(
    skipBudgetReason({ turn: 7, injections: 1, bytes: MAX_TOTAL_BYTES_PER_SESSION }, 8),
    'budget_exhausted',
    '累计字节到上限 = 预算用完'
  );
});

test('未注入原因桶：恰好六个，budget 已拆成两个', () => {
  assert.deepEqual(
    [...SKIP_REASONS],
    ['bridge_failed', 'already_queried_this_turn', 'budget_exhausted', 'no_hit', 'no_change', 'disabled']
  );
});

test('账本归类：没到取的时候按 sameTurn 分桶，其余分支不变', () => {
  // 桥失败优先于一切：连结果都没有时不能把 enabled 说成 false（那是「开关关着」）。
  assert.equal(
    classifyNote({ ok: false, enabled: false, delta: null, text: '', memoryDue: true, handoffDue: true }),
    'bridge_failed'
  );

  // 这一步压根没问桥：同回合已取过 vs 预算真的用完（老调用方不传 sameTurn = 预算）。
  assert.equal(
    classifyNote({ ok: true, enabled: false, delta: 'x', text: 'x', memoryDue: false, handoffDue: false, sameTurn: true }),
    'already_queried_this_turn'
  );
  assert.equal(
    classifyNote({ ok: true, enabled: false, delta: 'x', text: 'x', memoryDue: false, handoffDue: false, sameTurn: false }),
    'budget_exhausted'
  );
  assert.equal(
    classifyNote({ ok: true, enabled: false, delta: 'x', text: 'x', memoryDue: false, handoffDue: false }),
    'budget_exhausted',
    '没传 sameTurn 时按预算算'
  );

  // 开关关着：比「没变化」优先（关着时正文被清空，delta 是空串而不是 null）。
  assert.equal(
    classifyNote({ ok: true, enabled: false, delta: '', text: '', memoryDue: true, handoffDue: true }),
    'disabled'
  );

  // 没变化：没有 delta 且没有旧正文 = 零命中；有旧正文 = 内容没变。
  assert.equal(
    classifyNote({ ok: true, enabled: true, delta: null, text: '', memoryDue: true, handoffDue: true }),
    'no_hit'
  );
  assert.equal(
    classifyNote({ ok: true, enabled: true, delta: null, text: '上一份正文', memoryDue: true, handoffDue: true }),
    'no_change'
  );

  // 其余都算注入：真的变了（包括清空——模型看到的东西确实变了）。
  assert.equal(
    classifyNote({ ok: true, enabled: true, delta: '新正文', text: '新正文', memoryDue: true, handoffDue: true }),
    'inject'
  );
  assert.equal(
    classifyNote({ ok: true, enabled: true, delta: '', text: '', memoryDue: true, handoffDue: true }),
    'inject'
  );
});
