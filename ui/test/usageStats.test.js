// 模型用量统计的纯展示函数：B/M/K 单位、热力图分级与周列对齐、趋势堆叠、
// 饼图扇区。扫描与聚合在 Rust 侧（usage.rs），前端只保证把数字摆对。
import assert from 'node:assert/strict';
import test from 'node:test';

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
};
Object.defineProperty(globalThis, 'navigator', {
  configurable: true,
  value: { userAgent: 'node' },
});

const {
  formatTokens,
  formatPercent,
  heatLevels,
  heatmapColumns,
  stackTrend,
  donutSlices,
  modelColor,
  sliceDays,
  summarizeDays,
  weekdayLabel,
  RANGE_OPTIONS,
} = await import('../src/usage.js');

test('formatTokens 走 B / M / K 标准单位，精确到两位小数', () => {
  assert.equal(formatTokens(0), '0');
  assert.equal(formatTokens(512), '512');
  assert.equal(formatTokens(1234), '1.23K');
  assert.equal(formatTokens(12_345_678), '12.35M');
  assert.equal(formatTokens(3_149_676_512), '3.15B');
  assert.equal(formatTokens(123_400_000_000), '123.40B');
  assert.equal(formatTokens(undefined), '0');
  // 统计数字的位数一致性比短更重要：单位换算后恒两位小数。
  assert.equal(formatTokens(8_400_000_000), '8.40B');
});

test('formatPercent 固定一位小数', () => {
  assert.equal(formatPercent(0.361), '36.1%');
  assert.equal(formatPercent(0), '0.0%');
  assert.equal(formatPercent(1), '100.0%');
});

test('heatLevels 按非零日分位切 4 档，尖峰不淹没其余日子', () => {
  const days = [
    { tokens: 0 },
    { tokens: 10 },
    { tokens: 20 },
    { tokens: 30 },
    { tokens: 40 },
    { tokens: 1_000_000_000 }, // 单日尖峰
  ];
  const levels = heatLevels(days);
  assert.equal(levels[0], 0, '无用量日必须是 0 档');
  assert.ok(levels[5] === 4 || levels[5] === 3, '尖峰落在高档');
  assert.ok(levels[1] <= 2, '普通低用量日不被尖峰压到最低档');
});

test('heatmapColumns 按周一开头排周列并补 null 占位', () => {
  // 2026-09-21 是周一，27 日是周日：七天正好一列，28 日开启第二列。
  const tokens = [1, 2, 3, 4, 5, 6, 7, 8];
  const days = tokens.map((t, i) => {
    const day = String(21 + i);
    return { date: `2026-09-${day}`, tokens: t };
  });
  const columns = heatmapColumns(days);
  assert.equal(columns.length, 2, '21–27 一列，28 起第二列');
  assert.equal(columns[0][0], days[0], '周一的格子排在行 0');
  assert.equal(columns[0][4], days[4], '周五的格子排在行 4');
  assert.equal(columns[0][6], days[6], '周日收尾第一列');
  assert.equal(columns[1][0], days[7], '下一个周一开始新列');

  // 首列从周三开始：前两格是 null 占位。
  const wedged = [{ date: '2026-09-23', tokens: 1 }];
  const [only] = heatmapColumns(wedged);
  assert.equal(only[0], null);
  assert.equal(only[1], null);
  assert.equal(only[2], wedged[0], '周三排在行 2');
});

test('stackTrend 取前 N 个模型并把其余并入「其他」', () => {
  const models = [
    { key: 'a/A', tokens: 500 },
    { key: 'b/B', tokens: 300 },
    { key: 'c/C', tokens: 100 },
  ];
  const days = [
    { date: '2026-09-24', tokens: 0, requests: 0, models: {} },
    {
      date: '2026-09-25',
      tokens: 900,
      requests: 3,
      models: { 'a/A': 500, 'b/B': 300, 'c/C': 100 },
    },
  ];
  const { rows, max } = stackTrend(days, models, 2);
  assert.equal(max, 900);
  assert.equal(rows[0].parts.length, 0, '零日没有段');
  const parts = rows[1].parts;
  assert.deepEqual(
    parts.map((p) => p.key),
    ['a/A', 'b/B', '其他'],
    '只保留前 2 个模型，c/C 并入「其他」'
  );
  assert.equal(parts[2].tokens, 100);
});

test('donutSlices 生成占比与起始偏移，尾部合并为「其他」', () => {
  const models = [
    { key: 'a/A', tokens: 60 },
    { key: 'b/B', tokens: 30 },
    { key: 'c/C', tokens: 10 },
  ];
  const slices = donutSlices(models, 2);
  assert.deepEqual(
    slices.map((s) => s.key),
    ['a/A', 'b/B', '其他']
  );
  const ratioSum = slices.reduce((sum, s) => sum + s.ratio, 0);
  assert.ok(Math.abs(ratioSum - 1) < 1e-9, '占比之和必须是 1');
  assert.ok(Math.abs(slices[1].start - 0.6) < 1e-9, '第二段从 60% 处开始');
  assert.ok(Math.abs(slices[2].start - 0.9) < 1e-9, '「其他」从 90% 处开始');
});

test('modelColor 按序循环取色且容忍越界', () => {
  assert.equal(modelColor(0), modelColor(8), '第 9 个模型回到第一色');
  assert.match(modelColor(3), /^#/);
});

test('RANGE_OPTIONS 只保留 15/30/60/90 天四档', () => {
  assert.deepEqual(
    RANGE_OPTIONS.map((o) => o.days),
    [15, 30, 60, 90]
  );
  assert.equal(RANGE_OPTIONS[RANGE_OPTIONS.length - 1].label, '90 天');
});

test('weekdayLabel 给出中文星期且容忍坏输入', () => {
  // 2026-09-21 是周一（与 heatmapColumns 用例同一锚点）。
  assert.equal(weekdayLabel('2026-09-21'), '周一');
  assert.equal(weekdayLabel('2026-09-25'), '周五');
  assert.equal(weekdayLabel('2026-09-27'), '周日');
  assert.equal(weekdayLabel('not-a-date'), '');
  assert.equal(weekdayLabel(undefined), '');
});

test('sliceDays 取窗口末尾 N 天，超出全长时原样返回', () => {
  const days = [1, 2, 3, 4, 5].map((t) => ({ tokens: t }));
  assert.deepEqual(sliceDays(days, 2), [{ tokens: 4 }, { tokens: 5 }], '切的是最近的天');
  assert.equal(sliceDays(days, 7), days, '范围大于全长时不得复制或截断');
  assert.deepEqual(sliceDays(undefined, 7), []);
  assert.deepEqual(sliceDays(days, 5), days);
});

test('summarizeDays 聚合 tokens / 请求 / 活跃天与按模型降序合计', () => {
  const days = [
    { date: '2026-09-23', tokens: 0, requests: 0, models: {} },
    {
      date: '2026-09-24',
      tokens: 300,
      requests: 2,
      models: { 'a/Alpha': 200, 'b/Beta': 100 },
    },
    {
      date: '2026-09-25',
      tokens: 150,
      requests: 1,
      models: { 'a/Alpha': 150 },
    },
  ];
  const s = summarizeDays(days);
  assert.equal(s.tokens, 450);
  assert.equal(s.requests, 3);
  assert.equal(s.activeDays, 2, '零日不计活跃');
  assert.deepEqual(
    s.models.map((m) => [m.key, m.tokens]),
    [
      ['a/Alpha', 350],
      ['b/Beta', 100],
    ],
    '模型跨日合计并按 tokens 降序'
  );
  assert.equal(s.models[0].provider, 'a');
  assert.equal(s.models[0].model, 'Alpha');
  assert.equal(s.models[1].model, 'Beta');
  // 无分隔符的模型键整串当 provider，model 留空——列表行展示不缺字段。
  const odd = summarizeDays([{ tokens: 5, requests: 1, models: { weird: 5 } }]);
  assert.equal(odd.models[0].provider, 'weird');
  assert.equal(odd.models[0].model, '');
});
