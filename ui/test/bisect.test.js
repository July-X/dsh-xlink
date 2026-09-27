import assert from 'node:assert/strict';
import test from 'node:test';

// 二分定位的纯展示函数：结局映射、轮数估算、每一步的标题与配色。
// 判定与分治逻辑都在 Rust 侧（bisect.rs），前端只呈现。
const core = {
  invoke() {
    return Promise.reject(new Error('not used in these tests'));
  },
  Channel: class {
    onmessage = null;
  },
};

globalThis.window = { __TAURI__: { core }, navigator: { userAgent: 'node' } };
globalThis.document = {
  hidden: false,
  createElement: () => ({}),
  body: { classList: { toggle() {} } },
  addEventListener() {},
};
Object.defineProperty(globalThis, 'navigator', {
  configurable: true,
  value: { userAgent: 'node' },
});

const {
  conclusionView,
  roundsLeft,
  bisectHeadline,
  stepTitle,
  stepClassName,
} = await import('../src/bisect.js');

test('三种结局各有各的说法，没有「找到根因」这种取值', () => {
  const minimal = conclusionView({ kind: 'minimal-bad-set', members: ['dsh-x'] });
  assert.equal(minimal.label, '定位到最小可疑集合');
  assert.deepEqual(minimal.members, ['dsh-x']);

  const notInSet = conclusionView({ kind: 'not-in-set', members: [] });
  assert.equal(notInSet.label, '原因不在插件与技能里');

  const aborted = conclusionView({ kind: 'aborted' });
  assert.equal(aborted.label, '排查已中断');
});

test('未知 kind 不能被吞成"排查中"', () => {
  // 否则一次已结束的排查会看起来还在跑，用户会一直等下去。
  const unknown = conclusionView({ kind: 'future-kind-v2' });
  assert.notEqual(unknown.label, '排查中');
  assert.match(unknown.label, /future-kind-v2/);
});

test('没有结论时才算"排查中"', () => {
  assert.equal(conclusionView(null).label, '排查中');
  assert.equal(conclusionView(undefined).label, '排查中');
});

test('轮数估算等于 ceil(log2(n))，与后端同判据', () => {
  assert.equal(roundsLeft(1), 0);
  assert.equal(roundsLeft(2), 1);
  assert.equal(roundsLeft(3), 2);
  assert.equal(roundsLeft(4), 2);
  assert.equal(roundsLeft(8), 3);
  assert.equal(roundsLeft(12), 4);
  assert.equal(roundsLeft(0), 0);
});

test('进行中时把"还要几轮"和"每轮要起一次内核"都说出来', () => {
  // 用户最怕的是以为卡死。耗时预期与剩余轮数缺一不可。
  const text = bisectHeadline({ running: true, remaining: 12, candidate_count: 16 });
  assert.match(text, /12 个待排除/);
  assert.match(text, /4 轮/);
  assert.match(text, /临时内核/);
  assert.match(text, /耐心/);
});

test('进行中但只剩一个候选时不再承诺轮数', () => {
  const text = bisectHeadline({ running: true, remaining: 1, candidate_count: 16 });
  assert.match(text, /0 轮/);
});

test('还没开始 / 已中断说不同的话', () => {
  const none = bisectHeadline({ running: false, conclusion: null, candidate_count: 0 });
  assert.match(none, /还没有排查记录/);

  const partial = bisectHeadline({ running: false, conclusion: null, candidate_count: 8 });
  assert.match(partial, /已中断/);
  assert.match(partial, /接着/, '中断后要说明重新发起能续上');
});

test('每一轮的标题说清试了什么、结果如何', () => {
  assert.equal(
    stepTitle({ round: 1, tried: ['a', 'b'], outcome: 'fail' }),
    '第 1 轮：启用 a、b → 起不来'
  );
  assert.equal(
    stepTitle({ round: 2, tried: ['c'], outcome: 'pass' }),
    '第 2 轮：启用 c → 正常'
  );
  // 没试成不能渲染成"正常"——那会把二分往错方向带。
  assert.match(stepTitle({ round: 3, tried: ['d'], outcome: 'inconclusive' }), /没能真正试起来/);
});

test('配色按结果分三类', () => {
  assert.equal(stepClassName({ outcome: 'fail' }), 'step-fail');
  assert.equal(stepClassName({ outcome: 'pass' }), 'step-pass');
  assert.equal(stepClassName({ outcome: 'inconclusive' }), 'step-unknown');
});
