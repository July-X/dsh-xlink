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
  const { effectStore, experimentStore, resetSessionTimeline } = await import('../src/memory/effect.js');
  effectStore.report = null;
  effectStore.loading = false;
  effectStore.error = '';
  effectStore.rangeDays = 7;
  experimentStore.error = '';
  experimentStore.loading = false;
  resetSessionTimeline();
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
    // P1：账本 / 原因分解 / L2 对比 / 提取质量 / 弱引用
    'ledger', 'injected_events', 'skip_events', 'reasons', 'versions', 'recent', 'link',
    'linked', 'unlinked', 'rate', 'plugin_version', 'kernel_version',
    'comparison', 'available', 'method', 'sessions_with_both', 'exposed_requests',
    'unexposed_requests', 'metrics', 'ci_low', 'ci_high', 'caveat',
    'extraction', 'extract_memories', 'never_exposed', 'mentions', 'extractor_versions',
    // P2：会话级随机对照实验（L3）
    'experiment', 'configured', 'active', 'started_at_ms', 'ends_at_ms', 'stopped_at_ms',
    'ratio_percent', 'control_sessions', 'treatment_sessions', 'required_sessions',
    'metrics_available', 'control', 'treatment', 'n_control', 'n_treatment',
  ]) {
    assert.ok(all.includes(field), '合同字段没有被读到：' + field);
  }
});

test('效果面板有 11 块（P0 六组 + P1 四块 + P2 对照实验）与四态；趋势是手写 SVG，不引图表库', () => {
  const src = read('../src/memory/MemoryEffectPanel.vue');
  const tpl = templateOf(src);

  // ① 读数条（5 块，v-for 一份模板）+ 9 张功能卡。
  assert.match(tpl, /class="effect-kpis"/, '读数条');
  assert.match(tpl, /class="card effect-kpi"/, '读数块');
  for (const title of [
    '效果漏斗',
    '趋势（日）',
    '未注入原因分解',
    '分组对比（L2 关联）',
    '账本健康 / 版本',
    '记忆榜单',
    '提取质量',
    '对照实验（L3）',
    '成本',
    '数据健康',
  ]) {
    assert.ok(src.includes(title), '缺卡片：' + title);
  }
  assert.equal((tpl.match(/class="card-head"/g) || []).length, 10, '十张功能卡各有一个卡头');
  for (const label of ['触达会话', '注入事件', '注入字节', '携带记忆请求率', '主动读取 · 检索']) {
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

  // ④ L2 与 L3 分开写、不混算；L2 不许写成因果，因果只来自用户显式开启的对照实验。
  assert.ok(!/具有因果|证明(了|出)因果|因果差异/.test(src), 'L2 关联不许写成因果');
  assert.ok(src.includes('不是因果'), '分组对比要写明「不是因果」');
  assert.ok(src.includes('同一证据级别'), '对照实验要写明与 L2 不是同一证据级别');
  // ⑤ 估算值必须标注「估算」（成本卡的判据）。
  assert.ok(src.includes('估算'), 'token 是估算值，必须明说');
  // ⑥ 可访问性：图标按钮同时有 title 与 aria-label。
  const refreshAt = tpl.indexOf('icon-btn');
  assert.ok(refreshAt > 0, '缺少刷新按钮');
  const refreshBlock = tpl.slice(refreshAt, tpl.indexOf('/>', refreshAt));
  assert.ok(refreshBlock.includes('title='), '刷新按钮缺 title');
  assert.ok(refreshBlock.includes('aria-label='), '刷新按钮缺 aria-label');
});

test('P1 展示映射：原因中文名、CI 文本、对比单位、带符号差值、会话 / 时间 / 版本短串', async () => {
  const {
    reasonText,
    kindText,
    formatCi,
    ciCrossesZero,
    formatMetricValue,
    formatSigned,
    shortSessionId,
    timestampShort,
    shortVersion,
  } = await import('../src/memory/effect.js');

  // 原因映射：合同 §1.2 的 reason 枚举，认不出的原样返回（新原因先看得见，不吞成「未知」）。
  const reasons = {
    turn: '新回合',
    library_change: '库变化',
    probe: '探测',
    disabled: '开关关闭',
    no_hit: '零命中',
    budget: '预算/本回合不取',
    bridge_failed: '桥失败',
    no_change: '内容未变',
  };
  for (const [raw, label] of Object.entries(reasons)) {
    assert.equal(reasonText(raw), label, '原因 ' + raw + ' 的中文名不对');
  }
  assert.equal(reasonText('weird_reason'), 'weird_reason', '认不出的原因原样显示');
  assert.equal(reasonText(null), '—');
  assert.equal(reasonText(''), '—');
  assert.equal(kindText('inject'), '注入');
  assert.equal(kindText('skip'), '跳过');
  assert.equal(kindText('other'), 'other');
  assert.equal(kindText(null), '—');

  // CI 文本与跨 0 判断：跨 0 标「未见差异」，不做显著性结论。
  assert.equal(formatCi(-0.51, -0.05), '-0.51 ~ -0.05');
  assert.equal(formatCi(0.1, 0.2), '0.10 ~ 0.20');
  assert.equal(formatCi(null, 1), '—');
  assert.equal(formatCi(1, undefined), '—');
  assert.equal(ciCrossesZero(-0.1, 0.2), true);
  assert.equal(ciCrossesZero(0, 0.2), true, '端点 0 也算跨 0');
  assert.equal(ciCrossesZero(0.1, 0.2), false);
  assert.equal(ciCrossesZero(-1, -0.5), false);
  assert.equal(ciCrossesZero(null, 0.2), false, '缺一端不猜');

  // 对比指标按 unit 格式化：次 / token / %。
  assert.equal(formatMetricValue(0.82, '次'), '0.82 次');
  assert.equal(formatMetricValue(1234, 'token'), '1.23K');
  assert.equal(formatMetricValue(81.1, '%'), '81.1%');
  assert.equal(formatMetricValue(1.234, 'ratio'), '1.23');
  assert.equal(formatMetricValue(null, '次'), '—');

  // 带符号差值：0.00 不带符号（"-0.00" 读起来像 bug）。
  assert.equal(formatSigned(-0.28), '-0.28');
  assert.equal(formatSigned(0.28), '+0.28');
  assert.equal(formatSigned(0), '0.00');
  assert.equal(formatSigned(null), '—');

  // 短串：会话 id、时间、版本。
  assert.equal(shortSessionId('session-00a67abf-57a4-4f93-a645-8bc184dfa4b4'), '00a67abf');
  assert.equal(shortSessionId('00a67abf-57a4'), '00a67abf', '没有 session- 前缀的裸 id 也取前 8 个 hex');
  assert.equal(shortSessionId(null), '—');
  assert.match(timestampShort('2026-10-31T12:40:00Z'), /^\d{2}-\d{2} \d{2}:\d{2}$/, 'ISO 时间要压成 月-日 时:分');
  assert.equal(timestampShort('不是时间'), '—');
  assert.equal(timestampShort(null), '—');
  assert.equal(shortVersion('unknown'), '未知');
  assert.equal(shortVersion('0.2.0'), '0.2.0');
  assert.equal(shortVersion('xlink-memory-extract/5'), 'extract/5', '提取器公共前缀要压掉');
  assert.equal(shortVersion(''), '—');
});

test('会话下钻状态：memory_effect_session + IPC sessionId；同会话吃缓存、force 重读、换会话先清旧数据', async () => {
  await resetStore();
  const { loadSessionTimeline, sessionStore } = await import('../src/memory/effect.js');
  const sessionA = 'session-00a67abf-1111-2222-3333-444455556666';
  const sessionB = 'session-bbbbbbbb-1111-2222-3333-444455556666';
  invokeImpl = (cmd, args) => Promise.resolve({ session: args.sessionId, available: true, nodes: [] });

  const first = await loadSessionTimeline(sessionA);
  assert.equal(invoked.length, 1, '打开下钻要真的发一次命令');
  assert.deepEqual(
    invoked[0],
    ['memory_effect_session', { sessionId: sessionA }],
    '命令名与 IPC 参数（camelCase sessionId）必须逐字符合合同 §2.6'
  );
  assert.equal(first.session, sessionA);

  await loadSessionTimeline(sessionA);
  assert.equal(invoked.length, 1, '同一会话再次打开吃缓存');

  await loadSessionTimeline(sessionA, { force: true });
  assert.equal(invoked.length, 2, '「重试」必须强制重读');
  assert.deepEqual(invoked[1], ['memory_effect_session', { sessionId: sessionA }]);

  // 换会话：在途期间旧会话时间线必须已清空（两个会话混在一张表里比空态更糟）。
  let resolveNext;
  invokeImpl = (cmd, args) =>
    new Promise((resolve) => {
      resolveNext = () => resolve({ session: args.sessionId, available: true, nodes: [] });
    });
  const pending = loadSessionTimeline(sessionB);
  assert.equal(sessionStore.timeline, null, '换会话要先清掉上一条时间线');
  assert.equal(sessionStore.loading, true);
  assert.equal(sessionStore.sessionId, sessionB);
  resolveNext();
  await pending;
  assert.equal(sessionStore.timeline.session, sessionB);

  // 失败：保留 error 供重试，loading 必须复位；空 id 不发命令。
  invoked.length = 0;
  invokeImpl = () => Promise.reject(new Error('会话不存在或已删除'));
  await loadSessionTimeline('session-cccccccc-1111', { force: true });
  assert.equal(sessionStore.error, '会话不存在或已删除', '错误要能读到具体原因');
  assert.equal(sessionStore.loading, false, '失败后 loading 必须复位');
  await loadSessionTimeline('');
  assert.equal(invoked.length, 1, '空会话 id 不许发命令');
});

test('P1 四块卡的字段与口径文案都接上了：原因分解 / L2 对比 / 账本健康 / 提取质量 / 引用列', () => {
  const src = read('../src/memory/MemoryEffectPanel.vue');
  const tpl = templateOf(src);

  // KPI「注入事件」：账本优先，账本不可用退回扫描口径并写明。
  assert.ok(src.includes("'P1 账本'") && src.includes("'扫描口径'"), '读数条要有来源角标');
  assert.ok(src.includes('账本未启用，退回扫描口径'), '账本不可用时要写清退回哪条口径');

  // 未注入原因分解：固定五个桶（文案来自 effect.js 的 reasonText）+ 跳过率。
  const effectSrc = read('../src/memory/effect.js');
  for (const reason of ['no_hit', 'budget', 'no_change', 'bridge_failed', 'disabled']) {
    assert.match(effectSrc, new RegExp('\\b' + reason + ':'), '原因分解缺桶：' + reason);
  }
  assert.ok(src.includes("const SKIP_REASONS = ['no_hit', 'budget', 'no_change', 'bridge_failed', 'disabled']"));
  assert.ok(src.includes('跳过率'));
  assert.match(tpl, /v-if="ledgerAvailable"/, '无账本要空态');
  assert.ok(src.includes('账本未启用'), '无账本不许显示 0 假数据');

  // 分组对比：只做 L2、CI 跨 0 标「未见差异」、n<10 空态、caveat 固定。
  assert.ok(src.includes('未见差异'), 'CI 跨 0 要标「未见差异」而不是显著性结论');
  assert.ok(src.includes('样本不足：需要至少'), 'n<10 要给空态与所需样本量');
  assert.ok(src.includes("'L2 关联：同会话内暴露 vs 未暴露请求的配对差；不是因果。'"), '缺 caveat 时要兜底同一句话');
  assert.match(tpl, /v-if="comparisonAvailable"/);
  assert.ok(!/具有因果|证明(了|出)因果|因果差异/.test(src), 'L2 关联不许写成因果');

  // 账本健康 / 版本 / 最近事件：关联率、版本表、最近事件可点开下钻。
  for (const text of ['关联率', '关联情况', '插件版本', '内核版本', '最近事件']) {
    assert.ok(src.includes(text), '账本健康卡缺：' + text);
  }
  assert.match(tpl, /@click="openSessionTimeline\(row\.session\)"/, '最近事件行要能打开下钻');

  // 提取质量：五个读数 + 提取器版本表 + 空态。
  for (const text of ['提取来源记忆', '已停用', '窗口内从未曝光', '窗口内被读取', '引用（弱代理）', '提取器版本']) {
    assert.ok(src.includes(text), '提取质量卡缺：' + text);
  }
  assert.ok(src.includes('窗口内没有提取来源的记忆'), '提取质量要有自己的空态');

  // 榜单新增「引用」列（弱代理，tooltip 写清是启发式）。
  assert.ok(tpl.includes('<span>引用</span>'), '榜单要新增引用列');
  assert.match(tpl, /\{\{ row\.mentions \}\}/);
  assert.ok(src.includes('弱代理'), '引用列必须标注是弱代理');
});

test('会话下钻：命令在 effect.js，弹窗挂在面板上，节点只展示无正文', () => {
  const effectSrc = read('../src/memory/effect.js');
  assert.match(
    effectSrc,
    /invoke\('memory_effect_session', \{ sessionId: id \}\)/,
    '下钻命令名与 IPC 参数（camelCase sessionId）'
  );

  const panelSrc = read('../src/memory/MemoryEffectPanel.vue');
  const panelTpl = templateOf(panelSrc);
  assert.match(panelTpl, /<MemoryEffectSessionModal[\s\S]*?:session-id="sessionModalId"/, '面板要真的挂上下钻弹窗');
  assert.match(panelTpl, /@close="closeSessionTimeline"/, '弹窗关闭要收回 id');
  assert.match(panelSrc, /function openSessionTimeline\(session\)/, '打开下钻要接在最近事件行上');

  const modalSrc = read('../src/memory/MemoryEffectSessionModal.vue');
  const modalTpl = templateOf(modalSrc);
  // 弹窗写法与同屏另外两个详情弹窗同一条（append-to-body、固定宽度、点遮罩不关）。
  for (const attr of [
    ':model-value="true"',
    'width="min(640px, 92vw)"',
    'append-to-body',
    ':close-on-click-modal="false"',
  ]) {
    assert.equal(modalTpl.split(attr).length - 1, 1, '下钻弹窗缺 ' + attr);
  }
  assert.match(modalTpl, /@update:model-value="\(open\) => !open && emit\('close'\)"/, '关闭要通知面板');
  assert.ok(modalSrc.includes('只读；不显示任何正文'), '弹窗要写明只读无正文');
  for (const kind of ["injection: '注入'", "request: '请求'", "compaction: '压缩'"]) {
    assert.ok(modalSrc.includes(kind), '节点类型缺：' + kind);
  }
  assert.match(modalTpl, /v-if="error"/, '错误态');
  assert.match(modalTpl, /v-loading="loading && !rows.length"/, '加载态');
  assert.ok(modalSrc.includes('这个会话没有可展示的节点'), '空态');
  assert.ok(!modalSrc.includes('invoke('), '弹窗不许自己发命令（统一走 effect.js 的 sessionStore）');
  assert.ok(!/\.body\b|\.title\b/.test(modalSrc), '弹窗不许碰正文 / 标题字段（后端也不下发）');
});

test('P2 展示映射：毫秒时间、比例百分数、实验四态；缺字段显示「—」', async () => {
  const { formatMsDate, ratioPercentText, experimentStateText } = await import('../src/memory/effect.js');

  // 毫秒时间：0 表示「还没开始 / 没停止」，不许显示成 1970 年。
  assert.equal(formatMsDate(0), '—');
  assert.equal(formatMsDate(null), '—');
  assert.equal(formatMsDate(undefined), '—');
  assert.equal(formatMsDate(-1), '—');
  assert.match(formatMsDate(1792243424586), /^\d{2}-\d{2} \d{2}:\d{2}$/, '毫秒时间要压成 月-日 时:分');

  // 比例：integer 不带小数，非整数一位小数；缺字段「—」。
  assert.equal(ratioPercentText(10), '10%');
  assert.equal(ratioPercentText(5.5), '5.5%');
  assert.equal(ratioPercentText(0), '0%');
  assert.equal(ratioPercentText(null), '—');

  // 四态：未开展 / 进行中 / 已停止 / 已到期。active 由后端算好，前端不重新推导。
  assert.equal(experimentStateText(null), '未开展');
  assert.equal(experimentStateText({ configured: false }), '未开展');
  assert.equal(experimentStateText({ configured: true, active: true }), '进行中');
  assert.equal(experimentStateText({ configured: true, active: false, stopped_at_ms: 123 }), '已停止');
  assert.equal(
    experimentStateText({ configured: true, active: false, stopped_at_ms: 0, ends_at_ms: 1000 }, 2000),
    '已到期'
  );
  assert.equal(
    experimentStateText({ configured: true, active: false, stopped_at_ms: 0, ends_at_ms: 5000 }, 1000),
    '未开展',
    '既没停也没到期时不许伪造成「进行中」'
  );
});

test('开始 / 停止实验：命令名与 camelCase 参数正确，成功后强制刷新报告，失败写卡内 error', async () => {
  await resetStore();
  const { experimentStore, startExperiment, stopExperiment } = await import('../src/memory/effect.js');
  invokeImpl = (cmd, args) => {
    if (cmd === 'memory_effect_experiment_start') {
      return Promise.resolve({ configured: true, active: true, ratio_percent: 10, started_at_ms: 1, ends_at_ms: 2 });
    }
    if (cmd === 'memory_effect_experiment_stop') {
      return Promise.resolve({ configured: true, active: false, stopped_at_ms: 3 });
    }
    if (cmd === 'memory_effect_report') {
      return Promise.resolve({ range_days: args.rangeDays });
    }
    return Promise.resolve(null);
  };

  // 开始：默认 7 天 / 10% → ratioBp 用万分比 1000（IPC camelCase）。
  const started = await startExperiment();
  assert.deepEqual(
    invoked[0],
    ['memory_effect_experiment_start', { days: 7, ratioBp: 1000 }],
    '命令名与参数必须逐字符合合同 §2.2'
  );
  assert.equal(started.active, true);
  assert.equal(invoked.length, 2, '成功后要强制刷新一次报告');
  assert.equal(invoked[1][0], 'memory_effect_report');
  assert.deepEqual(invoked[1][1], { rangeDays: 7 });
  assert.equal(experimentStore.error, '');
  assert.equal(experimentStore.loading, false);

  // 停止：无参数命令，同样刷新报告。
  invoked.length = 0;
  const stopped = await stopExperiment();
  assert.deepEqual(invoked[0], ['memory_effect_experiment_stop', {}]);
  assert.equal(stopped.active, false);
  assert.equal(invoked[1][0], 'memory_effect_report');
  assert.equal(experimentStore.loading, false);

  // 失败：error 可在卡内显示，返回值是 null（调用方不必 catch），loading 复位。
  invoked.length = 0;
  invokeImpl = () => Promise.reject(new Error('已有进行中的对照实验，请先停止'));
  const failed = await startExperiment(7, 10);
  assert.equal(failed, null);
  assert.equal(experimentStore.error, '已有进行中的对照实验，请先停止');
  assert.equal(experimentStore.loading, false);
  assert.equal(invoked.length, 1, '失败后不该再发报告请求');
});

test('实验参数越界不 clamp、也不发命令（这是会改变行为的开关，越界直接报错）', async () => {
  await resetStore();
  const { experimentStore, startExperiment, EXPERIMENT_MAX_DAYS, EXPERIMENT_RATIO_MAX, EXPERIMENT_RATIO_MIN } =
    await import('../src/memory/effect.js');
  invokeImpl = () => Promise.resolve({ configured: true, active: true });

  assert.equal(EXPERIMENT_MAX_DAYS, 14);
  assert.equal(EXPERIMENT_RATIO_MIN, 5);
  assert.equal(EXPERIMENT_RATIO_MAX, 30);

  await startExperiment(0, 10);
  assert.match(experimentStore.error, /1–14/, '天数下界要报错');
  assert.equal(invoked.length, 0, '越界不许发命令');

  await startExperiment(15, 10);
  assert.match(experimentStore.error, /1–14/, '天数上界要报错');
  assert.equal(invoked.length, 0);

  await startExperiment(7, 4);
  assert.match(experimentStore.error, /5%–30%/, '比例下界要报错');
  assert.equal(invoked.length, 0);

  await startExperiment(7, 31);
  assert.match(experimentStore.error, /5%–30%/, '比例上界要报错');
  assert.equal(invoked.length, 0);
});

test('对照实验（L3）卡：默认关闭说明 + 二次确认 + 进行中停止 + 指标表 + 样本不足空态 + 固定 caveat', () => {
  const src = read('../src/memory/MemoryEffectPanel.vue');
  const tpl = templateOf(src);
  const effectSrc = read('../src/memory/effect.js');

  // 命令接线：开始 / 停止（IPC camelCase days / ratioBp；停止无参数）。
  assert.match(
    effectSrc,
    /invoke\('memory_effect_experiment_start', \{ days: dayValue, ratioBp: percentValue \* 100 \}\)/,
    '开始命令名与参数'
  );
  assert.match(effectSrc, /invoke\('memory_effect_experiment_stop'\)/, '停止命令名');

  // 未配置：默认关闭说明 + 开始按钮；说明必须写到「约 10% / 新会话 / 可随时停止 / 只影响新会话」。
  assert.ok(src.includes('对照实验（L3）'), '卡片标题');
  assert.ok(src.includes('默认关闭'), '必须写明默认关闭');
  assert.ok(src.includes("EXPERIMENT_DEFAULT_RATIO") && src.includes('EXPERIMENT_DEFAULT_DAYS'), '比例与天数读常量');
  assert.ok(tpl.includes('开始 7 天实验'), '未配置时要有开始按钮');
  assert.match(tpl, /@click="startExperimentFlow"/, '开始按钮要接二次确认');

  // 二次确认：ElMessageBox（经 confirmDialog）文案含比例 / 影响范围 / 停止入口。
  assert.match(src, /confirmDialog\(\s*'开启对照实验？'/, '开始前必须过一次确认框');
  assert.ok(src.includes("'实验期间约 '"), '确认文案要写比例');
  assert.ok(src.includes('% 的新会话不会带记忆（也不会自动提取）'), '确认文案要写影响范围');
  assert.ok(src.includes('可在本卡随时停止'), '确认文案要写停止入口');

  // 进行中：状态 / 结束时间 / 两组会话数 / 停止按钮。
  assert.ok(tpl.includes('停止实验'), '进行中要有停止入口');
  assert.match(tpl, /v-if="experimentActive"/, '停止按钮只在进行中出现');
  assert.match(tpl, /@click="stopExperimentFlow"/, '停止按钮要接线');
  assert.ok(src.includes('对照组比例') && src.includes('开始 / 结束') && src.includes('对照 / 实验会话'));
  assert.ok(src.includes('停止后对照组会话最迟在下一步恢复注入'), '停止语义要写清');

  // metrics：表头两组均值 / 差值 / CI / 两组 n；CI 跨 0 标「未见差异」，不给显著性结论。
  for (const text of ['对照组', '实验组', '95% CI', 'n（对照/实验）', '未见差异']) {
    assert.ok(src.includes(text), '指标表缺：' + text);
  }
  assert.match(tpl, /v-if="experimentMetricsAvailable"/, 'metrics_available 分支');
  assert.ok(src.includes('样本不足（每组至少'), '样本不足要给空态');
  assert.ok(src.includes('只给计数，不出读数'), '样本不足只给计数');
  assert.ok(src.includes("'同一证据级别'") || src.includes('同一证据级别'), '固定 caveat 要写与 L2 的证据级别差异');
  assert.ok(src.includes('L3 随机对照'), 'caveat 要标明 L3');

  // 缺字段「—」：状态 / 比例 / 时间都走 effect.js 的防御式格式化（不补 0）。
  assert.match(src, /formatMsDate/, '结束时间走毫秒格式化');
  assert.match(src, /ratioPercentText/, '比例走百分数格式化');
  assert.match(src, /experimentStateText/, '状态文案走状态函数');

  // 不引图表库；实验卡不新增依赖。
  for (const lib of ['echarts', 'chart.js', 'chartjs', 'd3', '@antv', 'highcharts', 'recharts']) {
    assert.ok(!src.includes(lib), '不许引图表库：' + lib);
  }
});
