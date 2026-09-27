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
  outcomeHeadline,
  verificationView,
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
  // 夹具用**后端真实的 camelCase 字段名**。这里曾经写成 snake_case，于是
  // 一份永远取不到值的夹具让这个用例绿了很久，而真实响应里每个维度都
  // 显示成「未知 / 0」。
  const text = entrySummary({
    kernelVersion: '0.1.5-rc.2',
    pluginCount: 2,
    skillCount: 3,
    patchCount: 0,
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
  const empty = headline({ entries: [], hasLastKnownGood: false });
  assert.match(empty, /还没有回退点/);
  assert.match(empty, /启动|改动/, '空状态要告诉用户怎么才会产生');

  // 有回退点但从没成功启动过 → 必须说清"还不是良好证据"。
  // 把它说成普通计数会让用户误以为已经验证过。
  const noGood = headline({ entries: [{}], hasLastKnownGood: false });
  assert.match(noGood, /还没有一次「成功启动」记录/);
});

// —— P1：恢复预览的纯展示函数 ——

test('恢复动作按维度给出中文标签，未知值原样透出', () => {
  assert.equal(diffKindLabel('plugin-disable'), '插件');
  assert.equal(diffKindLabel('patch-revert'), '补丁');
  assert.equal(diffKindLabel('weird-new-thing'), 'weird-new-thing');
});

test('没有改动时，恢复是空操作——确认按钮必须据此禁用', () => {
  const text = diffHeadline({ changes: [], blockedCount: 0 });
  assert.match(text, /完全一致/);
  assert.match(text, /不会做任何改动/, '空操作要说清"什么都不用做"');
});

test('有改动时标题带上改动条数，用户不用数', () => {
  const text = diffHeadline({ changes: [{}, {}], blockedCount: 0 });
  assert.match(text, /2 处改动/);
});

test('有动不了的条目时必须把这个数摆到用户面前', () => {
  // 这条是 P1 的核心：动不了的条目如果在确认之前没说，用户就是在一个
  // 不完整的承诺上点的确认。
  const note = diffBlockedNote({ blockedCount: 3 });
  assert.match(note, /3 处无法自动完成/);
  assert.match(note, /已标注原因/);
  // 全都能恢复时不啰嗦。
  assert.equal(diffBlockedNote({ blockedCount: 0 }), '');
  assert.equal(diffBlockedNote(null), '');
});

test('恢复结果的实测状态是三态，缺一不可', () => {
  // 这条是 P1 最后一块：让"没测"看起来像"测过没问题"，是这类工具最容易
  // 犯也最伤害信任的错。第三态 `not-needed`（没改东西所以没测）曾经就是
  // 借用"实测通过"那一句的——空操作返回 verified:true，界面画成绿色。
  const verified = verificationView({ verification: 'verified' });
  assert.equal(verified.type, 'success');
  assert.match(verified.label, /实测通过/);

  const failed = verificationView({ verification: 'failed' });
  assert.equal(failed.type, 'warning');
  assert.match(failed.label, /未能通过|未通过/);
  assert.match(failed.label, /不代表回退失败/);

  const notNeeded = verificationView({ verification: 'not-needed' });
  assert.equal(notNeeded.type, 'info', '"没改所以没测"不能画成成功或警告');
  assert.match(notNeeded.label, /没有做启动实测/);
  assert.doesNotMatch(notNeeded.label, /实测通过/);
});

test('后端没给 verification 时按最保守的一态处理', () => {
  // 老会话 / 异常返回都不该被当成"验过了"。
  const view = verificationView({});
  assert.equal(view.type, 'info');
  assert.doesNotMatch(view.label, /实测通过/);
});

test('有没能完成的条目时，总结句必须把它说清楚', () => {
  const text = outcomeHeadline({
    applied: ['a', 'b'],
    skipped: ['x 不在中央库', 'y 失败'],
  });
  assert.match(text, /2 处/);
  assert.match(text, /没能完成/);
});

test('空结果不编句子', () => {
  assert.equal(outcomeHeadline(null), '');
});
