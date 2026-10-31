// 记忆效果页签的判据（P0）：纯函数的行为，以及**它们真的接在后端命令与模板上**。
//
// 贯穿全篇的是本仓栽过的形状——「helper 有测试、接线没有」：格式化函数全绿，而
// 面板读的是另一套字段名、或命令根本没被调用。所以每条纯函数判据后面都跟着一条
// 读源码 / 真走一遍命令的判据（与 memoryPanel.test.js 同一套写法）。
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const read = (p) => readFileSync(new URL(p, import.meta.url), 'utf8');

// effect.js 顶层 import 了 bridge.js，后者在模块求值期读 window.__TAURI__
// （与 memoryPanel.test.js 同一套桩）。命令调用记下来给每条接线判据用。
const invoked = [];
let invokeImpl = () => Promise.resolve(null);

globalThis.window = {
  __TAURI__: {
    core: {
      invoke: (cmd, args) => {
        invoked.push([cmd, args]);
        return invokeImpl(cmd, args);
      },
      Channel: class {},
    },
  },
  addEventListener() {},
  removeEventListener() {},
};
Object.defineProperty(globalThis, 'navigator', { value: { userAgent: 'node' }, configurable: true });

/** .vue 的模板块，HTML 注释已剥（注释里会原样引用被判的表达式）。 */
function templateOf(src) {
  const start = src.indexOf('<template>');
  assert.ok(start >= 0, '找不到 <template>');
  const styleAt = src.indexOf('<style', start);
  const end = styleAt > start ? styleAt : src.length;
  return src.slice(start, end).replace(/<!--[\s\S]*?-->/g, '');
}

/** 把 store 复位到测试起点，免得用例之间互相影响（同一文件共用一个模块实例）。 */
async function resetStore() {
  const { effectStore } = await import('../src/memory/effect.js');
  effectStore.report = null;
  effectStore.loading = false;
  effectStore.error = '';
  effectStore.rangeDays = 7;
  invoked.length = 0;
}

test('格式化：缺字段显示「—」而不是 0；百分比一位小数，率值按 0–100 直接显示', async () => {
  const { formatPercent, formatRatio, formatBytes, formatTokens, formatCount, shortDate } = await import(
    '../src/memory/effect.js'
  );
  for (const empty of [null, undefined, '', 'abc', NaN, Infinity]) {
    assert.equal(formatPercent(empty), '—', '缺字段/非法值必须显示「—」： ' + String(empty));
    assert.equal(formatRatio(empty), '—');
    assert.equal(formatBytes(empty), '—');
    assert.equal(formatTokens(empty), '—');
    assert.equal(formatCount(empty), '—');
  }
  // shortDate 只对 null/undefined 回「—」：认不出的字符串原样返回（不吞信息）。
  assert.equal(shortDate(null), '—');
  assert.equal(shortDate(undefined), '—');
  assert.equal(shortDate(''), '—');
  // 0 是「真的是零」，不是缺字段。
  assert.equal(formatPercent(0), '0.0%');
  assert.equal(formatPercent(14.44), '14.4%');
  // 合同 §2.2 的 memory_request_ratio 是 14.4（百分数），前端不再乘 100（Lead 2026-10-31 裁决）。
  assert.equal(formatRatio(14.4), '14.4%');
  assert.equal(formatRatio(0), '0.0%');
  assert.equal(formatRatio(-5), '0.0%', '越界先 clamp 到 0');
  assert.equal(formatRatio(120), '100.0%', '越界先 clamp 到 100');
  assert.equal(formatBytes(0), '0 B');
  assert.equal(formatBytes(81715), '79.8 KB', '1024 进制，与 labels.js 同口径');
  assert.equal(formatTokens(26617), '26.62K');
  assert.equal(formatTokens(1350000), '1.35M');
  assert.equal(formatTokens(0), '0');
  assert.equal(formatCount(12712), '12,712');
  assert.equal(shortDate('2026-10-25'), '10-25');
  assert.equal(shortDate('2026-10-25T20:40:00Z'), '10-25');
  assert.equal(shortDate('不是日期'), '不是日期', '认不出时原样返回，不吞信息');
});

test('健康读数分档：越大越好 / 越小越好都认；没阈值不猜', async () => {
  const { healthLevel } = await import('../src/memory/effect.js');
  assert.equal(healthLevel(96, { good: 95, warn: 80 }), 'ok');
  assert.equal(healthLevel(90, { good: 95, warn: 80 }), 'warn');
  assert.equal(healthLevel(50, { good: 95, warn: 80 }), 'bad');
  assert.equal(healthLevel(0, { good: 0, warn: 2, lowerIsBetter: true }), 'ok');
  assert.equal(healthLevel(1, { good: 0, warn: 2, lowerIsBetter: true }), 'warn');
  assert.equal(healthLevel(9, { good: 0, warn: 2, lowerIsBetter: true }), 'bad');
  assert.equal(healthLevel(null, { good: 0, warn: 2 }), 'unknown', '缺字段不是 ok，也不是 bad');
  assert.equal(healthLevel(3), 'unknown', '没给阈值时不猜');
});

test('覆盖率派生与样本量：分母无效回 null（面板显示「—」），< 30 不给百分比', async () => {
  const { sharePercent, sampleEnough, MIN_SAMPLE } = await import('../src/memory/effect.js');
  assert.equal(sharePercent(12, 48), 25);
  assert.equal(sharePercent(0, 48), 0, '0 / 48 是 0%，不是「—」');
  assert.equal(sharePercent(1, 0), null, '分母 0 不能除');
  assert.equal(sharePercent(null, 48), null);
  assert.equal(sharePercent(1, undefined), null);
  assert.equal(MIN_SAMPLE, 30, '设计定的样本下限是 30');
  assert.equal(sampleEnough(30), true);
  assert.equal(sampleEnough(29), false);
  assert.equal(sampleEnough(null), false);
});

test('时间窗只接受 7/30/90：非法值不改窗口、也不发命令', async () => {
  await resetStore();
  const { effectStore, setEffectRange, isRangeDay } = await import('../src/memory/effect.js');
  assert.equal(isRangeDay(7), true);
  assert.equal(isRangeDay('30'), true, '按钮传过来的是数字，字符串数字也认');
  assert.equal(isRangeDay(15), false);
  assert.equal(isRangeDay(null), false);
  await setEffectRange(15);
  assert.equal(effectStore.rangeDays, 7, '非法窗口不许改动当前窗口');
  assert.equal(invoked.length, 0, '非法窗口不许发命令');
});

test('loadEffectReport 调 memory_effect_report 并带 rangeDays（IPC camelCase）；同窗口吃缓存、force 重读、换档重读', async () => {
  await resetStore();
  const { effectStore, loadEffectReport, setEffectRange } = await import('../src/memory/effect.js');
  invokeImpl = (cmd, args) => Promise.resolve({ range_days: args.rangeDays, kpi: {} });

  const first = await loadEffectReport();
  assert.equal(invoked.length, 1, '首次进入要真的发一次命令');
  assert.deepEqual(invoked[0], ['memory_effect_report', { rangeDays: 7 }], '命令名与参数是合同钉死的');
  assert.equal(first.range_days, 7);

  await loadEffectReport();
  assert.equal(invoked.length, 1, '同一窗口再次进入吃缓存，不重扫');

  await loadEffectReport({ force: true });
  assert.equal(invoked.length, 2, '「刷新」必须强制重读');
  assert.deepEqual(invoked[1], ['memory_effect_report', { rangeDays: 7 }]);

  await setEffectRange(30);
  assert.equal(effectStore.rangeDays, 30);
  assert.equal(invoked.length, 3, '换时间窗必须重读');
  assert.deepEqual(invoked[2], ['memory_effect_report', { rangeDays: 30 }]);
  assert.equal(effectStore.report.range_days, 30);
});

test('失败保留上一次成功数据、只设 error；重试成功后 error 清空', async () => {
  await resetStore();
  const { effectStore, loadEffectReport } = await import('../src/memory/effect.js');
  invokeImpl = () => Promise.resolve({ range_days: 7, kpi: { injection_snapshots: 3 } });
  await loadEffectReport();
  assert.equal(effectStore.report.kpi.injection_snapshots, 3);

  invokeImpl = () => Promise.reject(new Error('扫描失败：权限不足'));
  const failed = await loadEffectReport({ force: true });
  assert.equal(effectStore.error, '扫描失败：权限不足', '错误要能读到具体原因');
  assert.equal(effectStore.report.kpi.injection_snapshots, 3, '失败不许把上一次成功数据冲掉');
  assert.equal(failed.kpi.injection_snapshots, 3, '失败时返回上一次可展示的数据');
  assert.equal(effectStore.loading, false, '失败后 loading 必须复位');

  invokeImpl = () => Promise.resolve({ range_days: 7, kpi: { injection_snapshots: 5 } });
  await loadEffectReport({ force: true });
  assert.equal(effectStore.error, '', '成功一次就清掉错误');
  assert.equal(effectStore.report.kpi.injection_snapshots, 5);
});

test('记忆页有「记忆库 / 效果」两个页签，切到效果页才渲染效果面板并拉数据', () => {
  const source = read('../src/memory/MemoryPanel.vue');
  const tpl = templateOf(source);

  // ① 页签本体：role=tablist + 两枚 role=tab 按钮（可访问性），默认停在记忆库。
  assert.match(tpl, /role="tablist"/, '页签行要有 tablist 语义');
  assert.equal((tpl.match(/role="tab"/g) || []).length, 2, '恰好两枚页签');
  for (const tab of ['library', 'effect']) {
    assert.ok(
      tpl.includes(":aria-selected=\"activeTab === '" + tab + "'\""),
      '页签 ' + tab + ' 缺 aria-selected'
    );
    assert.ok(tpl.includes("@click=\"selectTab('" + tab + "')\""), '页签 ' + tab + ' 没接线');
  }
  assert.ok(tpl.includes('>记忆库') || /记忆库\s*<\/button>/.test(tpl), '要有「记忆库」页签');
  assert.ok(/效果\s*<\/button>/.test(tpl), '要有「效果」页签');
  assert.match(source, /const activeTab = ref\('library'\)/, '默认页签必须是记忆库');
  assert.match(source, /if \(tab === 'effect'\) loadEffectReport\(\)/, '切到效果页要拉数据');

  // ② 条件渲染：记忆库主内容（含开关告警）只在该页签渲染，效果页用独立组件替换。
  assert.match(tpl, /<template v-if="activeTab === 'library'">/, '记忆库主内容要按页签条件渲染');
  assert.match(tpl, /<MemoryEffectPanel v-else \/>/, '效果页签要真的挂上效果面板');
  // 既有判据逐字钉着的告警表达式不许被顺手改掉（加条件只能包在外层 template 上）。
  assert.match(tpl, /v-if="enabled && injectionDisabled\(injectPlugin\)"/);
  // 页头标题与开关保留：效果页只是换一句描述，不删开关。
  assert.match(tpl, /使用已批准的记忆/, '页头开关在两个页签下都要在');
  assert.ok(tpl.includes('本机会话日志的只读扫描与记忆库聚合'), '效果页要换一句自己的页头口径');
});

test('effect.js 调用的就是合同命令；面板读的字段名全是合同 §2.2 的 snake_case', () => {
  const effectSrc = read('../src/memory/effect.js');
  const panelSrc = read('../src/memory/MemoryEffectPanel.vue');
  assert.match(
    effectSrc,
    /invoke\('memory_effect_report', \{ rangeDays: rangeDays \}\)/,
    '命令名与参数必须逐字符合合同 §2.1'
  );
  // 合同 §2.2 的字段：漏读任何一组都意味着某张卡静默显示「—」。
  const all = effectSrc + panelSrc;
  for (const field of [
    'generated_at', 'range_days', 'window', 'coverage',
    'sessions_total', 'sessions_active', 'sessions_with_injection',
    'decode_failures', 'unknown_event_ratio', 'format_versions',
    'kpi', 'injection_snapshots', 'injected_bytes', 'injected_tokens_est',
    'requests', 'memory_requests', 'memory_request_ratio',
    'memory_read_calls', 'memory_search_calls', 'memory_propose_calls',
    'funnel', 'candidates_created', 'memories_created', 'memories_injected', 'memories_read',
    'trend', 'leaderboard', 'cost', 'extraction_jobs_done', 'extraction_jobs_failed',
    'extraction_tokens_upper_est', 'health', 'has_enough_requests', 'has_enough_injections',
  ]) {
    assert.ok(all.includes(field), '合同字段没有被读到：' + field);
  }
});

test('效果面板有 6 组卡片与四态；趋势是手写 SVG，不引图表库', () => {
  const src = read('../src/memory/MemoryEffectPanel.vue');
  const tpl = templateOf(src);

  // ① 6 组：KPI 读数条（5 块，v-for 一份模板）+ 漏斗 / 趋势 / 榜单 / 成本 / 健康。
  assert.match(tpl, /class="effect-kpis"/, '读数条');
  assert.match(tpl, /class="card effect-kpi"/, '读数块');
  for (const title of ['效果漏斗', '趋势（日）', '记忆榜单', '成本', '数据健康']) {
    assert.ok(src.includes(title), '缺卡片：' + title);
  }
  assert.equal((tpl.match(/class="card-head"/g) || []).length, 5, '五张功能卡各有一个卡头');
  for (const label of ['触达会话', '注入快照', '注入字节', '携带记忆请求率', '主动读取 · 检索']) {
    assert.ok(src.includes(label), '读数条缺一块：' + label);
  }

  // ② 四态：加载 / 错误 / 空 / 样本不足。少一个就会在真机上出现白屏或假 0。
  assert.match(tpl, /v-loading="loading && !report"/, '加载态');
  assert.match(tpl, /v-if="error"/, '错误态');
  assert.match(tpl, /重试/, '错误态要能重试');
  assert.match(tpl, /还没有记忆效果数据/, '空态');
  assert.match(src, /样本不足/, '样本不足要降级为计数');
  assert.match(src, /数据不足 3 天/, '趋势少于 3 天不出图');

  // ③ 图表手写 SVG；不引任何图表库（引库会撞 UI 产物体积预算）。
  assert.match(tpl, /<svg /, '趋势必须手写 SVG');
  assert.match(tpl, /role="img"/, '图表要有 role=img');
  assert.match(tpl, /aria-label="效果漏斗/, '漏斗要有文本摘要');
  assert.match(tpl, /:aria-label="trendAria"/, '趋势要有文本摘要');
  for (const lib of ['echarts', 'chart.js', 'chartjs', 'd3', '@antv', 'highcharts', 'recharts']) {
    assert.ok(!src.includes(lib), '不许引图表库：' + lib);
  }

  // ④ P1 项如实写「未接入」，不画假数据（合同 §1 / §3.3）。
  assert.ok(src.includes('P1 未接入'), 'P1 未接入项要写清，而不是留占位或假数');
  // ⑤ 估算值必须标注「估算」（成本卡的判据）。
  assert.ok(src.includes('估算'), 'token 是估算值，必须明说');
  // ⑥ 可访问性：图标按钮同时有 title 与 aria-label。
  const refreshAt = tpl.indexOf('icon-btn');
  assert.ok(refreshAt > 0, '缺少刷新按钮');
  const refreshBlock = tpl.slice(refreshAt, tpl.indexOf('/>', refreshAt));
  assert.ok(refreshBlock.includes('title='), '刷新按钮缺 title');
  assert.ok(refreshBlock.includes('aria-label='), '刷新按钮缺 aria-label');
});
