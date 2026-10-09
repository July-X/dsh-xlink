// 套餐用量的纯展示函数：配色档位、余额行、收起态摘要、错误收集与时间
// 标签。查询与缓存决策都在 Rust 侧（subscription.rs），前端保证把状态摆对、
// 失败不被渲染成「0 余额 / 0%」。
import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import * as Vue from 'vue';

const require = createRequire(import.meta.url);
const { parse } = require(require.resolve('@vue/compiler-sfc', { paths: [require.resolve('vue')] }));
const { compile } = require(require.resolve('@vue/compiler-dom', { paths: [require.resolve('vue')] }));
const { renderToString } = require(require.resolve('@vue/server-renderer', { paths: [require.resolve('vue')] }));

// 编译真实组件中的额度行，不复制模板：数据层单测无法发现 v-else 误落到无限额度。
async function renderOverviewTiers(tiers) {
  const { descriptor } = parse(readFileSync(new URL('../src/shell/OverviewPanel.vue', import.meta.url), 'utf8'));
  const template = descriptor.template;
  function findTier(node) {
    if (node.props?.some((prop) => prop.name === 'class' && prop.value?.content === 'plan-tier-col')) return node;
    for (const child of node.children || []) {
      const found = findTier(child);
      if (found) return found;
    }
  }
  const node = findTier(template.ast);
  assert.ok(node, '概览页额度行必须存在');
  const { code } = compile(node.loc.source, {
    mode: 'function', comments: false, prefixIdentifiers: true, isCustomElement: () => true,
  });
  const render = new Function('Vue', code)(Vue);
  return renderToString(Vue.createSSRApp({ data: () => ({ row: { tiers } }), render }));
}

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
  percentLevel,
  currencySymbol,
  balanceRow,
  providerStateText,
  providerShortState,
  collectErrors,
  tierRow,
  planTierRows,
  queriedAtLabel,
  providerView,
  subscription,
  failurePromptPending,
  markFailurePrompted,
  isProviderHidden,
  hideProvider,
} = await import('../src/subscription/subscription.js');
const { countdownLabel, countdownFullLabel, relativeTimeLabel, relativeAgeCompact } = await import('../src/shell/labels.js');

test('概览真实模板：缺失窗口只显示暂无数据，不显示无限额度或进度条', async () => {
  const html = await renderOverviewTiers(planTierRows({ id: 'openai_codex', tiers: [] }));
  assert.ok(html.includes('暂无数据'));
  assert.ok(!html.includes('无限周额度'));
  assert.ok(!html.includes('plan-bar'));
  assert.ok(!html.includes('100%'));
});

test('概览真实模板：明确 unlimited 的窗口仍显示无限额度', async () => {
  const html = await renderOverviewTiers([{ name: '7d', unlimited: true, missing: false }]);
  assert.ok(html.includes('无限周额度'));
  assert.ok(!html.includes('暂无数据'));
});

test('percentLevel 按剩余百分比三档配色（≥70 绿 / 40–69.99 橙 / <39.99 红）', () => {
  assert.equal(percentLevel(100), 'ok');
  assert.equal(percentLevel(73.2), 'ok');
  assert.equal(percentLevel(70), 'ok');
  assert.equal(percentLevel(69.9), 'warning');
  assert.equal(percentLevel(40), 'warning');
  assert.equal(percentLevel(39.9), 'danger');
  assert.equal(percentLevel(0), 'danger');
  assert.equal(percentLevel(undefined), 'danger', '非数字按最危险档处理');
});

test('currencySymbol 映射币种，未知币种退回代码', () => {
  assert.equal(currencySymbol('CNY'), '¥');
  assert.equal(currencySymbol('USD'), '$');
  assert.equal(currencySymbol('EUR'), 'EUR ');
  assert.equal(currencySymbol(''), '');
});

test('balanceRow 把总额与赠金 / 充值分开列（DeepSeek 三个字段口径不同）', () => {
  const row = balanceRow({ currency: 'CNY', total: '16.64', granted: '4.64', topped_up: '12.00' });
  assert.equal(row.main, '余额：¥16.64');
  assert.equal(row.detail, '赠金 ¥4.64 · 充值 ¥12.00', '赠金与充值必须分别标出');
  assert.equal(row.tip, '总余额 ¥16.64 ＝ 赠金（未过期）：¥4.64 ＋ 充值：¥12.00');
});

test('balanceRow 金额只透传不运算：缺字段不补零，只有充值时也照常显示', () => {
  const onlyTopped = balanceRow({ currency: 'USD', total: '5.21', granted: null, topped_up: '5.21' });
  assert.equal(onlyTopped.main, '余额：$5.21');
  assert.equal(onlyTopped.detail, '充值 $5.21', '赠金缺失就只列充值，不补 0');
  assert.equal(onlyTopped.tip, '总余额 $5.21 ＝ 充值：$5.21');
  // 两项都缺（接口只给总额）→ 明细行留空，hover 明确说明而不是编一个 0。
  const bare = balanceRow({ currency: 'CNY', total: '88.00' });
  assert.equal(bare.main, '余额：¥88.00');
  assert.equal(bare.detail, '');
  assert.equal(bare.tip, '总余额 ¥88.00（接口未返回赠送 / 充值明细）');
  // 零值也照常透传展示（0.00 是接口给的事实，不当作「没有」）。
  assert.equal(balanceRow({ currency: 'CNY', total: '0.00', granted: '0.00', topped_up: '0.00' }).detail, '赠金 ¥0.00 · 充值 ¥0.00');
  assert.equal(balanceRow(null), null);
});

test('providerStateText 只承载完整错误文案（横幅用）', () => {
  assert.equal(
    providerStateText({ configured: true, fetch_error: '查询失败（网络不可达或超时）' }),
    '查询失败（网络不可达或超时）'
  );
  assert.equal(
    providerStateText({ configured: true, credential_status: 'expired', error: '凭据无效（HTTP 401）' }),
    '凭据无效（HTTP 401）'
  );
  assert.equal(
    providerStateText({ configured: true, kind: 'plan', tiers: [] }),
    null,
    '「暂无额度数据」是状态词不是错误，不进横幅'
  );
  assert.equal(providerStateText({ configured: true, kind: 'plan', tiers: [{ remaining_percent: 50 }] }), null);
  assert.equal(providerStateText(null), null);
});

test('providerShortState 输出 2–6 字短状态词（摘要 / 分区小字用）', () => {
  assert.equal(providerShortState({ configured: true, fetch_error: '查询失败（…长文案…）' }), '查询失败');
  assert.equal(providerShortState({ configured: true, credential_status: 'expired', error: '…' }), '凭据失效');
  assert.equal(providerShortState({ configured: true, error: '业务错误（1004）：…' }), '查询异常');
  assert.equal(providerShortState({ configured: true, kind: 'plan', tiers: [] }), '暂无额度数据');
  assert.equal(providerShortState({ configured: true, kind: 'plan', tiers: [{ remaining_percent: 50 }] }), null);
  assert.equal(providerShortState({ configured: false, kind: null, tiers: [] }), null);
  assert.equal(providerShortState(null), null);
});

test('collectErrors 汇总 provider 级错误，成功时为空', () => {
  assert.deepEqual(collectErrors({ providers: [{ id: 'a', configured: true, kind: 'plan', tiers: [1] }] }), []);
  const errors = collectErrors({
    providers: [
      { id: 'a', label: 'A', configured: true, fetch_error: '网络不可达' },
      { id: 'b', label: 'B', configured: false },
    ],
  });
  assert.deepEqual(errors, ['A：网络不可达']);
});

test('providerStateText 覆盖三类失败态：本次失败 / 凭据失效 / 业务错误', () => {
  assert.equal(providerStateText({ fetch_error: '网络不可达' }), '网络不可达');
  assert.equal(providerStateText({ credential_status: 'expired' }), '凭据已失效');
  assert.equal(providerStateText({ error: '业务错误（1004）' }), '业务错误（1004）');
  assert.equal(providerStateText({ credential_status: 'valid' }), null);
  assert.equal(providerStateText({}), null);
  assert.equal(providerStateText(null), null);
});

test('查不到数据时提示隐藏：hideProvider 被记住、collectErrors 不再提及、成功自动恢复', async () => {
  const failing = {
    providers: [
      { id: 'deepseek', label: 'DeepSeek', configured: true, fetch_error: '凭据无效（HTTP 401）' },
      { id: 'minimax-cn', label: 'MiniMax-CN', configured: true, kind: 'plan', tiers: [{ remaining_percent: 50 }] },
    ],
  };
  // 配了 key 但失败 → 需要提示；提示过 / 已隐藏 / 未配置 → 不再提示。
  assert.equal(failurePromptPending(failing.providers[0]), true);
  assert.equal(failurePromptPending(failing.providers[1]), false);
  assert.equal(failurePromptPending({ id: 'x', configured: false, fetch_error: 'x' }), false);
  markFailurePrompted('deepseek');
  assert.equal(failurePromptPending(failing.providers[0]), false, '本会话内不重复打扰');

  hideProvider('deepseek');
  assert.equal(isProviderHidden('deepseek'), true);
  assert.deepEqual(
    collectErrors(failing),
    [],
    '隐藏后错误横幅不再提及（分区本身也不再渲染）'
  );

  // 查询成功（换 key 后拿到数据）→ 自动恢复显示，横幅口径同时恢复。
  const previousInvoke = window.__TAURI__.core.invoke;
  window.__TAURI__.core.invoke = () =>
    Promise.resolve({
      providers: [
        {
          id: 'deepseek',
          label: 'DeepSeek',
          configured: true,
          kind: 'balance',
          balances: [{ currency: 'CNY', total: '9.90' }],
          queried_at_ms: 1,
        },
      ],
    });
  try {
    const { refreshSubscription } = await import('../src/subscription/subscription.js');
    await refreshSubscription();
    assert.equal(isProviderHidden('deepseek'), false, '查询成功后自动恢复显示');
    assert.deepEqual(subscription.errors, []);
    assert.equal(providerView('deepseek').balances[0].total, '9.90');
  } finally {
    window.__TAURI__.core.invoke = previousInvoke;
  }
});

test('tierRow 对无限周额度输出 ♾️ 标记，不产进度数据', () => {
  const row = tierRow({ name: 'weekly', unlimited: true, remaining_percent: 100 }, Date.now());
  assert.equal(row.name, '7d');
  assert.equal(row.unlimited, true);
  assert.equal(row.percent, null);
  assert.equal(row.tip, '无限周额度');
});

test('tierRow 产出档位 / 倒计时 / 双向口径 tooltip', () => {
  const now = 1_761_308_400_000;
  const row = tierRow({ name: '5h', remaining_percent: 73.2, resets_at_ms: now + 3 * 3600_000 + 47 * 60_000 }, now);
  assert.equal(row.name, '5h');
  assert.equal(row.percent, 73);
  assert.equal(row.level, 'ok');
  assert.equal(row.countdown, '3h47m');
  assert.equal(row.countdownTitle, '3 小时 47 分后重置');
  assert.equal(row.tip, '已用 27% · 剩余 73%');
  const expired = tierRow({ name: 'weekly', remaining_percent: 5, resets_at_ms: now - 1000 }, now);
  assert.equal(expired.countdown, '已重置');
  assert.equal(expired.level, 'danger');
});

test('queriedAtLabel 只对成功查询过的时间出文案', () => {
  assert.equal(queriedAtLabel(null), null);
  assert.equal(queriedAtLabel({ queried_at_ms: 0 }), null);
  assert.equal(queriedAtLabel({ queried_at_ms: Date.now() - 30_000 }), '刚刚');
  assert.equal(queriedAtLabel({ queried_at_ms: Date.now() - 5 * 60_000 }), '5 分钟前');
});

test('countdownLabel / relativeTimeLabel 的时间口径', () => {
  assert.equal(countdownLabel(0), '已重置');
  assert.equal(countdownLabel(-5), '已重置');
  assert.equal(countdownLabel(59 * 60_000), '59m');
  assert.equal(countdownLabel(3 * 3600_000 + 47 * 60_000), '3h47m');
  assert.equal(countdownLabel(4 * 86_400_000 + 11 * 3600_000), '4d11h');
  assert.equal(countdownFullLabel(4 * 86_400_000 + 11 * 3600_000), '4 天 11 小时后重置');
  assert.equal(countdownFullLabel(45 * 60_000), '45 分钟后重置');
  assert.equal(relativeTimeLabel(0), '');
  const now = Date.now();
  assert.equal(relativeAgeCompact(now - 30_000), '<1min');
  assert.equal(relativeAgeCompact(now - 12 * 60_000), '12min');
  assert.equal(relativeAgeCompact(now - 3 * 3600_000), '3h');
  assert.equal(relativeAgeCompact(now - 2 * 86_400_000), '2d');
});

test('providerView 从共享状态取视图，失败路径不清空 data（keep-last-good）', async () => {
  const { refreshSubscription } = await import('../src/subscription/subscription.js');
  const previousInvoke = window.__TAURI__.core.invoke;
  // bridge.invoke 每次调用都读 core.invoke 属性，替换即生效。
  window.__TAURI__.core.invoke = () => Promise.reject(new Error('网络断了'));
  subscription.data = {
    providers: [{ id: 'deepseek', configured: true, kind: 'balance', balances: [{ currency: 'CNY', total: '1.00' }] }],
  };
  subscription.errors = [];
  assert.equal(providerView('deepseek').balances[0].total, '1.00');
  // 模拟一次失败的强制刷新：data 保持不变，errors 落文案。
  await refreshSubscription();
  assert.deepEqual(subscription.errors, ['查询套餐用量失败：网络断了。已保留上次结果，可点击刷新重试']);
  assert.equal(providerView('deepseek').balances[0].total, '1.00', '失败绝不能清空旧数据');
  window.__TAURI__.core.invoke = previousInvoke;
});

// OpenAI 的 5h 窗口可能整个不返回（跨应用共享额度）。缺席的窗口必须**占位**
// 并写明「暂无数据」，既不能少一行让人误读成「这个套餐只有周窗口」，也不能
// 补成 100%（那是把「没测到」说成「还很空」）。
test('OpenAI 缺席的窗口占位为「暂无数据」，绝不补成 100%', () => {
  const onlyWeekly = {
    id: 'openai_codex',
    kind: 'plan',
    tiers: [{ name: '7d', remaining_percent: 40, resets_at_ms: null }],
  };
  const rows = planTierRows(onlyWeekly);
  assert.deepEqual(
    rows.map((row) => row.name),
    ['7d', '5h'],
    '两个窗口都应出现，缺席的那个排在后面'
  );
  const missing = rows.find((row) => row.name === '5h');
  assert.equal(missing.missing, true);
  assert.equal(missing.percent, null, '缺席窗口不得有百分比');
  assert.equal(missing.countdown, null);
});

test('OpenAI 两个窗口都在时不补占位', () => {
  const rows = planTierRows({
    id: 'openai_codex',
    kind: 'plan',
    tiers: [
      { name: '5h', remaining_percent: 75, resets_at_ms: null },
      { name: '7d', remaining_percent: 40, resets_at_ms: null },
    ],
  });
  assert.equal(rows.length, 2);
  assert.ok(rows.every((row) => row.missing === undefined));
});

test('非 OpenAI 的 provider 不补占位（无周限额套餐的 7d 缺席不是「暂无数据」）', () => {
  const rows = planTierRows({
    id: 'minimax_cn',
    kind: 'plan',
    tiers: [{ name: '5h', remaining_percent: 50, resets_at_ms: null }],
  });
  assert.deepEqual(
    rows.map((row) => row.name),
    ['5h']
  );
});
