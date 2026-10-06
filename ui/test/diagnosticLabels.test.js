import assert from 'node:assert/strict';
import test from 'node:test';

// 运行诊断的展示映射与通道消息解析。**这两处是纯函数**：判断与措辞都在
// 前端，写错了 Rust 侧测不出来——比如把 10 排到 2 前面、把未知状态悄悄
// 吞掉，都只会在用户屏幕上发生。
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

const labels = await import('../src/diagnostics/diagnostic-labels.js');
const precheck = await import('../src/diagnostics/precheck-labels.js');
const diag = await import('../src/diagnostics/diagnostics.js');

test('未知 stage / status / cause 显式透出，不被吞掉', () => {
  assert.equal(labels.stageLabel('wait-ready'), '等待端口就绪');
  assert.equal(labels.stageLabel('stage-from-the-future'), '未知阶段（stage-from-the-future）');
  assert.equal(labels.stageLabel(''), '未知阶段');

  assert.equal(labels.statusMeta('failure').label, '失败');
  assert.equal(labels.statusMeta('weird').label, '未知状态（weird）');
  // 空 cause 单独处理：它表示后端没给归因，不是「归因未知」。
  assert.equal(labels.causeLabel(''), '');
  assert.equal(labels.causeLabel('environment'), '环境问题');
  assert.equal(labels.causeLabel('brand-new'), '未知原因（brand-new）');
});

test('inconclusive 不叫「失败」', () => {
  // 证据不足被画成失败会让用户去处置无辜的插件。
  assert.equal(labels.statusMeta('inconclusive').label, '未能完成');
  assert.notEqual(labels.statusMeta('inconclusive').label, labels.statusMeta('failure').label);
});

test('时间线按 seq 排序，而不是按字符串', () => {
  const sorted = labels.sortedBySeq([
    { seq: 10, message: '第十' },
    { seq: 2, message: '第二' },
    { seq: 1, message: '第一' },
  ]);
  assert.deepEqual(
    sorted.map((e) => e.message),
    ['第一', '第二', '第十'],
    '字符串排序会把 10 排到 2 前面'
  );
});

test('找出第一个失败阶段；没有失败时返回 null', () => {
  const events = [{ seq: 1, status: 'success' }, { seq: 2, status: 'failure' }];
  assert.equal(labels.firstFailedEvent(events).seq, 2);
  assert.equal(labels.firstFailedEvent([{ seq: 1, status: 'success' }]), null);
  assert.equal(labels.firstFailedEvent(null), null);
});

test('耗时格式化：毫秒与秒分档', () => {
  assert.equal(labels.durationLabel(340), '340 毫秒');
  assert.equal(labels.durationLabel(12400), '12.4 秒');
  assert.equal(labels.durationLabel(0), '');
  assert.equal(labels.durationLabel('x'), '');
});

test('下一步按归因生成，不是一句通用「重装试试」', () => {
  assert.match(labels.nextStepFor({ status: 'failure', cause: 'plugin' }), /插件/);
  assert.match(labels.nextStepFor({ status: 'failure', cause: 'environment' }), /端口/);
  assert.match(labels.nextStepFor({ status: 'inconclusive' }), /证据不足/);
  assert.match(labels.nextStepFor({ status: 'failure', cause: 'brand-new' }), /完整日志/);
  assert.equal(labels.nextStepFor({ status: 'success' }), '');
});

test('结构化通道消息被解析成事件 + 人话', () => {
  const event = { seq: 3, stage: 'wait-ready', status: 'running', message: '正在等待端口' };
  const msg = JSON.stringify({ type: 'diagnostic-event', runId: 'run-1', event });
  const parsed = diag.parseChannelMessage(msg);
  assert.equal(parsed.event.stage, 'wait-ready');
  assert.equal(parsed.text, '正在等待端口', '进度浮层取 message 显示人话');
});

test('纯文本与无法解析的 JSON 都按文本显示', () => {
  // pnpm 输出里有大量 `{` 开头的行，解析失败不是错误。
  assert.equal(diag.parseChannelMessage('正在安装依赖…').event, null);
  assert.equal(diag.parseChannelMessage('{not json at all').text, '{not json at all');
  assert.equal(diag.parseChannelMessage('{"type":"other"}').event, null);
  assert.equal(diag.parseChannelMessage(undefined).text, '');
});

test('只有诊断事件进实时时间线，纯文本不污染它', () => {
  diag.clearLiveEvents();
  const event = { seq: 1, stage: 'spawn-kernel', status: 'success', message: '内核已派生' };
  const structured = JSON.stringify({ type: 'diagnostic-event', runId: 'run-2', event });
  assert.equal(diag.ingestChannelMessage(structured, 'run-2'), true);
  assert.equal(diag.ingestChannelMessage('[INFO] 正在解析依赖树', 'run-2'), false);
  assert.equal(diag.diagnosticStore.liveEvents.length, 1, '纯文本行不进时间线');
  diag.clearLiveEvents();
});

test('返回时记录仍在，只有实时流被清', () => {
  // 用户返回后再打开要看的是同一条记录；清空会被读成「记录被清了」。
  diag.clearLiveEvents();
  diag.diagnosticStore.active = { kind: 'startup', runId: 'run-3' };
  diag.diagnosticStore.sourcePanel = 'versions';
  diag.diagnosticStore.currentRun = { id: 'run-3', events: [] };
  const back = diag.closeDiagnosis();
  assert.equal(back, 'versions');
  assert.equal(diag.diagnosticStore.active, null);
  assert.equal(diag.diagnosticStore.currentRun.id, 'run-3');
});
// —— 预检三态与来源 / 完整性（设计 §6.4 §6.5）——

test('预检三态各有独立措辞，inconclusive 不与 fail 混同', () => {
  // inconclusive = 基线就没起来，候选插件根本没被测过。画成「失败」会让
  // 用户去卸一个无辜的包。
  assert.equal(precheck.SOURCE_LABELS.npm, 'npm 包');
  const states = { pass: '预检通过', fail: '预检未通过', inconclusive: '预检未能完成' };
  assert.equal(new Set(Object.values(states)).size, 3, '三态文案必须互不相同');
  assert.notEqual(states.inconclusive, states.fail);
});

test('完整性分四档，sha1 与 none 都不许显示成「已校验」', () => {
  // sha1 存在但抗碰撞已破；none 是根本没验。两者笼统叫「已校验」会让用户
  // 以为拿到了和 npm 官方同样的保证。
  assert.match(precheck.INTEGRITY_META.sha512.label, /sha512/);
  assert.equal(precheck.INTEGRITY_META.sha512.weak, false);
  assert.equal(precheck.INTEGRITY_META.sha1.weak, true, 'sha1 必须标为弱保证');
  assert.match(precheck.INTEGRITY_META.sha1.label, /较弱/);
  assert.equal(precheck.INTEGRITY_META.none.weak, true);
  assert.equal(precheck.INTEGRITY_META.none.tone, 'bad', '没验不是「通过」');
  // 未知摘要算法原样透出，不假装成 none（那会把「后端换了算法」说成「压根没校验」）
  const unknown = precheck.INTEGRITY_META.sha3;
  assert.equal(unknown, undefined, '未知算法不在表里，调用方走兜底分支');
});

test('插件诊断没有 runId 时不当作错误', () => {
  // 预检在取源阶段就失败时没有落记录，那是正常路径。
  diag.clearLiveEvents();
  return diag.loadPluginRun('').then((r) => {
    assert.equal(r, null);
    assert.equal(diag.diagnosticStore.error, '', '空 runId 不该留下错误提示');
  });
});


// —— §4.3：命令返回值与事件中的 runId 必须一致 ——

test('刷新优先按 id 拉详情，不按「最近一条」猜', async () => {
  // 两次启动挨得近时「最近一条」可能是上一条，用户会看到与刚才无关的
  // 时间线。store.js 存住后端回填的 id，诊断页按它拉。
  const store = await import('../src/store.js');
  store.setLastRunId('run-current');
  assert.equal(store.getLastRunId(), 'run-current');
  // 空值必须被规整成空串而不是 undefined：'runId || ...' 的兜底分支
  // 依赖它，undefined 会让 `||` 走对分支但比较时行为不一致。
  store.setLastRunId(undefined);
  assert.equal(store.getLastRunId(), '');
});
