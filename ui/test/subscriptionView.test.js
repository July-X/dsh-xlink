// 套餐用量的纯展示函数：配色档位、余额行、收起态摘要、错误收集与时间
// 标签。查询与缓存决策都在 Rust 侧（subscription.rs），前端保证把状态摆对、
// 失败不被渲染成「0 余额 / 0%」。
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
  percentLevel,
  currencySymbol,
  balanceText,
  providerStateText,
  providerShortState,
  collectErrors,
  tierRow,
  queriedAtLabel,
  providerView,
  subscription,
} = await import('../src/subscription.js');
const { countdownLabel, relativeTimeLabel } = await import('../src/labels.js');

test('percentLevel 按剩余百分比三档配色', () => {
  assert.equal(percentLevel(73.2), 'ok');
  assert.equal(percentLevel(50), 'ok');
  assert.equal(percentLevel(49.9), 'warning');
  assert.equal(percentLevel(20), 'warning');
  assert.equal(percentLevel(19.9), 'danger');
  assert.equal(percentLevel(0), 'danger');
  assert.equal(percentLevel(undefined), 'danger', '非数字按最危险档处理');
});

test('currencySymbol 映射币种，未知币种退回代码', () => {
  assert.equal(currencySymbol('CNY'), '¥');
  assert.equal(currencySymbol('USD'), '$');
  assert.equal(currencySymbol('EUR'), 'EUR ');
  assert.equal(currencySymbol(''), '');
});

test('balanceText 透传金额字符串并省略零值赠送 / 充值', () => {
  assert.equal(
    balanceText({ currency: 'CNY', total: '110.00', granted: '10.00', topped_up: '100.00' }),
    '¥110.00（赠送 ¥10.00 · 充值 ¥100.00）'
  );
  assert.equal(balanceText({ currency: 'USD', total: '5.21', granted: '0.00', topped_up: '5.21' }), '$5.21（充值 $5.21）');
  assert.equal(balanceText({ currency: 'CNY', total: '88.00' }), '¥88.00');
  assert.equal(balanceText(null), '');
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

test('tierRow 产出档位 / 倒计时 / 双向口径 tooltip', () => {
  const now = 1_761_308_400_000;
  const row = tierRow({ name: '5h', remaining_percent: 73.2, resets_at_ms: now + 3 * 3600_000 + 47 * 60_000 }, now);
  assert.equal(row.name, '5 小时窗口');
  assert.equal(row.percent, 73);
  assert.equal(row.level, 'ok');
  assert.equal(row.countdown, '3 小时 47 分');
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
  assert.equal(countdownLabel(59 * 60_000), '59 分钟');
  assert.equal(countdownLabel(3 * 3600_000 + 47 * 60_000), '3 小时 47 分');
  assert.equal(countdownLabel(4 * 86_400_000 + 11 * 3600_000), '4 天 11 小时');
  assert.equal(relativeTimeLabel(0), '');
});

test('providerView 从共享状态取视图，失败路径不清空 data（keep-last-good）', async () => {
  const { refreshSubscription } = await import('../src/subscription.js');
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
