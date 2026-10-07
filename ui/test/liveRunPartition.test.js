// 实时时间线只装**当前那一次**运行的事件（审查 R2-P2-01）。
//
// 过去判据是「`__runId` 与上一条不同**或** seq 更新」——不同也照收。于是迟到的
// 上一次事件会跟当前的事件排在同一条时间线上，两个运行的阶段交替出现；而它们
// 的 seq 是各自计数的，会撞号，用户看到一条自相矛盾的时间线。
//
// 关键在于「新运行的第一条」与「旧运行的迟到消息」在信封里长得一模一样，只能
// 靠「见过没有」区分。所以下面第一条测试先把「见过」这件事本身钉住。
import assert from 'node:assert/strict';
import test from 'node:test';

globalThis.window = {
  __TAURI__: {
    core: {
      invoke() {
        return Promise.resolve([]);
      },
      Channel: class {
        onmessage = null;
      },
    },
  },
  navigator: { userAgent: 'node' },
  addEventListener() {},
  removeEventListener() {},
};
Object.defineProperty(globalThis, 'navigator', {
  configurable: true,
  value: { userAgent: 'node' },
});
globalThis.document = {
  hidden: false,
  createElement: () => ({}),
  body: { classList: { toggle() {} } },
  addEventListener() {},
};

const {
  diagnosticStore,
  ingestChannelMessage,
  clearLiveEvents,
  visibleEvents,
} = await import('../src/diagnostics/diagnostics.js');

/** 一条带信封的诊断事件（`type` 是硬要求，见 `parseChannelMessage`）。 */
function envelope(runId, seq, stage) {
  return JSON.stringify({
    type: 'diagnostic-event',
    runId,
    event: { seq, stage, status: 'running', message: `${stage} #${seq}` },
  });
}

/** 一条**没有**信封 runId 的诊断事件。 */
function plainEnvelope(seq, stage) {
  return JSON.stringify({
    type: 'diagnostic-event',
    event: { seq, stage, status: 'running', message: `${stage} #${seq}` },
  });
}

function runIds() {
  return visibleEvents().map((e) => e.__runId);
}

test('旧运行的迟到事件不会进当前时间线', () => {
  clearLiveEvents();
  // 运行 A 跑了两条。
  ingestChannelMessage(envelope('run-A', 1, 'baseline'), 'run-A');
  ingestChannelMessage(envelope('run-A', 2, 'candidate'), 'run-A');
  assert.deepEqual(runIds(), ['run-A', 'run-A']);

  // 运行 B 开始，时间线重开。
  ingestChannelMessage(envelope('run-B', 1, 'baseline'), 'run-B');
  ingestChannelMessage(envelope('run-B', 2, 'candidate'), 'run-B');
  assert.deepEqual(runIds(), ['run-B', 'run-B'], '新运行要重开时间线，而不是追加');

  // A 的第三条迟到。它是 A 的第 3 条、seq 比当前流里任何一条都小——正是
  // 过去会被收进来的那一条。
  ingestChannelMessage(envelope('run-A', 3, 'rollback'), 'run-B');
  assert.deepEqual(
    runIds(),
    ['run-B', 'run-B'],
    '迟到的旧事件必须被丢掉，否则时间线上会出现另一个运行的阶段'
  );
});

test('迟到的事件仍然报告「这是一条诊断事件」', () => {
  clearLiveEvents();
  ingestChannelMessage(envelope('run-A', 1, 'x'), 'run-A');
  ingestChannelMessage(envelope('run-B', 1, 'x'), 'run-B');
  // 返回 true 的调用方会顺手刷新列表；丢掉的是**时间线条目**，不是这条消息。
  assert.equal(ingestChannelMessage(envelope('run-A', 2, 'y'), 'run-B'), true);
});

test('打开历史记录后紧接着来的旧事件不会污染它', () => {
  clearLiveEvents();
  ingestChannelMessage(envelope('run-A', 1, 'x'), 'run-A');
  ingestChannelMessage(envelope('run-B', 1, 'x'), 'run-B');
  // 用户点开一条具体的历史记录：`openStartupDiagnosis` 会设 active.runId、
  // 清空实时流，然后从磁盘加载。这一步是**当前那组断言的前提**——没有
  // active.runId 时，实时流分不出「这条属于谁」，任何迟到消息都会被当成
  // 新流，把磁盘那份顶掉（第一版实现就栽在这里，是这条测试红出来的）。
  diagnosticStore.active = { kind: 'startup', runId: 'run-A' };
  clearLiveEvents();
  diagnosticStore.currentRun = { id: 'run-A', events: [{ seq: 1, stage: 'from-disk' }] };
  assert.deepEqual(
    visibleEvents().map((e) => e.stage),
    ['from-disk'],
    '打开历史记录后显示的是落盘那份'
  );

  // 这时上一条运行的尾巴才到。
  ingestChannelMessage(envelope('run-B', 2, 'late'), 'run-B');
  assert.deepEqual(
    visibleEvents().map((e) => e.stage),
    ['from-disk'],
    '开历史记录时最可能收到旧事件——它不能把当前这条覆盖掉'
  );
  diagnosticStore.active = null;
});

test('正在看的是最新一条（没有具体 id）时，新运行仍然能接管实时流', () => {
  // `active.runId` 为空是「看最近一次」的常态（概览横幅走的就是这条），
  // 这时不能把「目标 = active.runId」当成唯一判据，否则新运行永远进不来。
  diagnosticStore.active = { kind: 'startup', runId: '' };
  clearLiveEvents();
  ingestChannelMessage(envelope('run-C', 1, 'baseline'), 'run-C');
  ingestChannelMessage(envelope('run-D', 1, 'baseline'), 'run-D');
  assert.deepEqual(runIds(), ['run-D'], '没见过的 runId = 新的一次运行，要接管实时流');
  diagnosticStore.active = null;
});

test('同一次运行里 seq 更大的事件照常追加，乱序的小 seq 被丢', () => {
  clearLiveEvents();
  ingestChannelMessage(envelope('run-A', 1, 'a'), 'run-A');
  ingestChannelMessage(envelope('run-A', 3, 'c'), 'run-A');
  ingestChannelMessage(envelope('run-A', 2, 'b'), 'run-A');
  assert.deepEqual(
    visibleEvents().map((e) => e.stage),
    ['a', 'c'],
    '同一次运行内按 seq 单调，迟到的更小 seq 不该插进来'
  );
});

test('没有 runId 的事件照旧进时间线，不被分区逻辑吃掉', () => {
  clearLiveEvents();
  ingestChannelMessage(envelope('run-A', 1, 'first'), 'run-A');
  // 预检的某些阶段消息没有信封。
  ingestChannelMessage(plainEnvelope(2, 'plain'), 'run-A');
  assert.deepEqual(
    visibleEvents().map((e) => e.stage),
    ['first', 'plain'],
    '无法归属的事件不该被静默丢弃——丢掉就是让用户的进度面板少一格'
  );
});
