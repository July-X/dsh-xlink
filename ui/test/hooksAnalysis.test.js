import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';

// Hook → DeepSeek 分析的 UI 判据:模板接线(入口按钮 / 对话框挂载)与 store 的
// 失败语义(失败保留选择,不关窗)。材料包与派发的行为由 Rust 侧单测覆盖。

const deferred = () => {
  let resolve;
  const promise = new Promise((res) => { resolve = res; });
  return { promise, resolve };
};
const plans = [];
const handoffs = [];
const sessions = [];
const reopens = [];
const invocations = [];
const core = {
  invoke(command, args) {
    invocations.push({ command, args });
    if (command === 'hooks_analysis_plan') return plans.shift().promise;
    if (command === 'hooks_analysis_sessions') return sessions.shift().promise;
    if (command === 'hooks_analysis_reopen') return reopens.shift().promise;
    if (command === 'hooks_analysis_handoff') return handoffs.shift().promise;
    if (command === 'hooks_analysis_status') return Promise.resolve({ state: 'failed', failure: '工作台拒绝了草稿' });
    return Promise.reject(new Error('unexpected command: ' + command));
  },
  Channel: class { onmessage = null; },
};
globalThis.window = { __TAURI__: { core }, navigator: { userAgent: 'node' } };
globalThis.document = {
  hidden: false,
  createElement: () => ({}),
  createElementNS: () => ({}),
  createTextNode: () => ({}),
  createComment: () => ({}),
  querySelector: () => null,
  addEventListener() {},
  removeEventListener() {},
  body: { classList: { toggle() {} } },
};

const hooks = await import('../src/hooks/hooks.js');
const templateOf = (path) => {
  const source = readFileSync(new URL(path, import.meta.url), 'utf8');
  const start = source.indexOf('<template>');
  // 组件里有 <template #header> 等嵌套片段,收尾取最后一个。
  const end = source.lastIndexOf('</template>');
  return source.slice(start, end);
};
const detailTemplate = templateOf('../src/hooks/HookDetail.vue');
const inboxTemplate = templateOf('../src/hooks/HookInbox.vue');

test('详情页脚有「交给 AI 分析」入口,只有 ready 且有材料时可用', () => {
  assert.match(detailTemplate, /交给 AI 分析/, '主按钮存在');
  assert.match(detailTemplate, /:disabled="!analysisReady"/, '可用性绑定后端判据');
  assert.match(detailTemplate, /查看分析（\{\{ analysisCount \}\}）/, '记录入口显示数量');
  assert.match(inboxTemplate, /HookAnalysisDialog/, '准备页挂在收件箱上');
  assert.match(inboxTemplate, /管理分析材料/);
});

test('准备计划读权威后端;后台刷新失败不打断详情', async () => {
  const plan = deferred();
  plans.push(plan);
  hooks.hookStore.detail = { event: { id: 'evt_1' }, attachments: [] };
  const opened = hooks.openAnalysis('prepare');
  assert.equal(hooks.hookStore.analysis.visible, true, '打开即显示');
  plan.resolve({ materials: [], handoffs: [{ handoffId: 'ha_1', state: 'attached' }] });
  await opened;
  assert.equal(hooks.hookStore.analysis.loading, false);
  assert.equal(hooks.hookStore.analysis.plan.handoffs.length, 1);

  // 关窗后作废在途计划响应:迟到的计划不许落回界面。
  const late = deferred();
  plans.push(late);
  const refreshing = hooks.openAnalysis();
  hooks.closeAnalysis();
  late.resolve({ materials: [], handoffs: [] });
  await refreshing;
  assert.equal(hooks.hookStore.analysis.visible, false, '关窗保持关闭');
  assert.equal(hooks.hookStore.analysis.plan.handoffs.length, 1, '迟到响应不覆盖');
});

test('交接失败原地保留:submitError 有可执行下一步,不关窗、选择不丢', async () => {
  hooks.hookStore.analysis.visible = true;
  hooks.hookStore.detail = { event: { id: 'evt_2' }, attachments: [] };
  const ack = deferred();
  handoffs.push(ack);
  const pending = hooks.submitAnalysisHandoff({
    includeBody: true,
    logs: [],
    workspace: { mode: 'new', path: null, sessionId: null },
  });
  // 首条 handoff 请求必须带幂等 requestId。
  await new Promise((resolve) => setImmediate(resolve));
  const request = invocations.find((item) => item.command === 'hooks_analysis_handoff');
  assert.ok(request.args.request.requestId, 'requestId 幂等键必须随请求发送');
  ack.resolve({ handoffId: 'ha_2', sessionTitle: 'Hook · x', workspacePath: '/w' });
  const ok = await pending;
  assert.equal(ok, false, '工作台未确认 attached 前不算成功');
  assert.match(hooks.hookStore.analysis.submitError, /保留/, '失败提示要说明选择已保留');
  assert.equal(hooks.hookStore.analysis.visible, true, '准备页不关,选择不丢');
  assert.equal(hooks.hookStore.analysis.submitting, false);
});

test('确认丢失后同一选择重试复用请求 ID，提交中双击只发一次', async () => {
  hooks.hookStore.analysis.visible = true;
  hooks.hookStore.detail = { event: { id: 'evt_retry' }, attachments: [] };
  const start = invocations.length;
  const selection = { includeBody: true, logs: [], workspace: { mode: 'new', path: null } };
  handoffs.push({ promise: Promise.reject('模拟确认丢失') });
  const first = hooks.submitAnalysisHandoff(selection);
  assert.equal(await hooks.submitAnalysisHandoff(selection), false);
  assert.equal(await first, false);
  handoffs.push({ promise: Promise.resolve({ handoffId: 'ha_retry' }) });
  assert.equal(await hooks.submitAnalysisHandoff(selection), false);
  const requests = invocations.slice(start).filter(({ command }) => command === 'hooks_analysis_handoff');
  assert.equal(requests.length, 2);
  assert.equal(requests[0].args.request.requestId, requests[1].args.request.requestId);
});

test('关闭准备页清理会话加载态，迟到结果不覆盖重新读取', async () => {
  const first = deferred();
  const next = deferred();
  sessions.push(first, next);
  const read = hooks.loadAnalysisSessions();
  hooks.closeAnalysis();
  assert.equal(hooks.hookStore.analysis.sessionsLoading, false);
  const refreshed = hooks.loadAnalysisSessions();
  next.resolve({ available: true, items: [{ id: 'current' }] });
  await refreshed;
  first.resolve({ available: true, items: [{ id: 'old' }] });
  await read;
  assert.equal(hooks.hookStore.analysis.sessions.items[0].id, 'current');
});

test('全局分析材料在没有打开原事件时仍可管理', async () => {
  hooks.closeDetail();
  const plan = deferred();
  plans.push(plan);
  const opened = hooks.openAnalysis('manage');
  assert.equal(invocations.at(-1).args.eventId, null);
  plan.resolve({ materials: [], handoffs: [{ handoffId: 'from-deleted-event' }] });
  await opened;
  assert.equal(hooks.hookStore.analysis.plan.handoffs[0].handoffId, 'from-deleted-event');
});

test('切换到准备页时旧记录操作不留下提交锁', async () => {
  hooks.hookStore.detail = { event: { id: 'evt_switch' }, attachments: [] };
  const old = deferred();
  reopens.push(old);
  const pending = hooks.reopenAnalysisRecord('history');
  assert.equal(hooks.hookStore.analysis.submitting, true);
  const plan = deferred();
  plans.push(plan);
  const opened = hooks.openAnalysis('prepare');
  plan.resolve({ materials: [], handoffs: [] });
  await opened;
  old.resolve();
  assert.equal(await pending, false);
  assert.equal(hooks.hookStore.analysis.submitting, false);
});
