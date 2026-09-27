import assert from 'node:assert/strict';
import test from 'node:test';

// 与其他 UI 测试同款：只测共享 JS 模块，不渲染 Vue 组件。
// snapshots.js 用了 `reactive`，因此 document mock 必须能撑起 vue
// runtime-dom 在模块加载时的那一次 createElement('template')。
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
globalThis.requestAnimationFrame = (cb) => {
  cb();
  return 1;
};

const {
  reasonLabel,
  reasonHint,
  entrySummary,
  headline,
  diffKindLabel,
  diffHeadline,
  diffBlockedNote,
} = await import('../src/snapshots.js');

test('打点原因给出中文名，未知值原样透出而不是被吞掉', () => {
  assert.equal(reasonLabel('startup-ok'), '成功启动过');
  assert.equal(reasonLabel('pre-change'), '变更之前');
  assert.equal(reasonLabel('manual'), '手动回退点');
  // 未知来源必须看得见，而不是被渲染成空标签。
  assert.equal(reasonLabel('from-the-future'), 'from-the-future');
  assert.equal(reasonLabel(''), '未知来源');
});

test('每种打点原因都有对应解释', () => {
  for (const reason of ['startup-ok', 'pre-change', 'manual']) {
    assert.ok(reasonHint(reason).length > 0, `${reason} 缺解释`);
  }
  assert.ok(reasonHint('??').length > 0, '未知原因也要有兜底解释');
});

test('摘要带出四个维度，缺内核版本时不显示 undefined', () => {
  const text = entrySummary({
    kernel_version: '0.1.5-rc.2',
    plugin_count: 2,
    skill_count: 3,
    patch_count: 0,
  });
  assert.match(text, /0\.1\.5-rc\.2/);
  assert.match(text, /插件 2/);
  assert.match(text, /技能 3/);
  assert.match(text, /补丁 0/);
  assert.doesNotMatch(entrySummary({}), /undefined/, '缺字段不能露出 undefined');
});

test('三种列表状态说三句不同的话', () => {
  // 还没拉过 → 不编句子。
  assert.equal(headline(null), '');

  // 一次都没有 → 说清"怎么才会产生"，而不是渲染空列表。
  const empty = headline({ entries: [], has_last_known_good: false });
  assert.match(empty, /还没有回退点/);
  assert.match(empty, /启动|改动/, '空状态要告诉用户怎么才会产生');

  // 有回退点但从没成功启动过 → 必须说清"还不是良好证据"。
  // 把它说成普通计数会让用户误以为已经验证过。
  const noGood = headline({ entries: [{}], has_last_known_good: false });
  assert.match(noGood, /还没有一次「成功启动」记录/);
});

// —— P1：恢复预览的纯展示函数 ——

test('恢复动作按维度给出中文标签，未知值原样透出', () => {
  assert.equal(diffKindLabel('plugin-disable'), '插件');
  assert.equal(diffKindLabel('patch-revert'), '补丁');
  assert.equal(diffKindLabel('weird-new-thing'), 'weird-new-thing');
});

test('没有改动时，恢复是空操作——确认按钮必须据此禁用', () => {
  const text = diffHeadline({ changes: [], blocked_count: 0 });
  assert.match(text, /完全一致/);
  assert.match(text, /不会做任何改动/, '空操作要说清"什么都不用做"');
});

test('有改动时标题带上改动条数，用户不用数', () => {
  const text = diffHeadline({ changes: [{}, {}], blocked_count: 0 });
  assert.match(text, /2 处改动/);
});

test('有动不了的条目时必须把这个数摆到用户面前', () => {
  // 这条是 P1 的核心：动不了的条目如果在确认之前没说，用户就是在一个
  // 不完整的承诺上点的确认。
  const note = diffBlockedNote({ blocked_count: 3 });
  assert.match(note, /3 处无法自动完成/);
  assert.match(note, /已标注原因/);
  // 全都能恢复时不啰嗦。
  assert.equal(diffBlockedNote({ blocked_count: 0 }), '');
  assert.equal(diffBlockedNote(null), '');
});
