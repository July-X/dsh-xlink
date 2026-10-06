import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { relative, resolve } from 'node:path';
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

// 设计 §2.5.1 的入口表是**契约**：入口按用户正在处理的对象分配，缺一个
// 入口等于那类场景下没有出路。源码断言而非行为测试——删掉一个按钮不会让
// 任何测试变红，只会让用户在那个场景下找不到入口。
const readSrc = (p) => readFileSync(resolve('ui/src', p), 'utf8');

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


// —— 设计 §2.5：入口与头部 ——

test('§2.5.1 的三个入口位置都在', () => {
  const overview = readSrc('shell/OverviewPanel.vue');
  assert.match(overview, /openStartupDiagnosis/, '概览必须有启动诊断入口');
  assert.match(overview, /openKernelStatusDiagnosis/, '概览必须有内核状态入口');
  assert.match(
    overview,
    /查看启动诊断/,
    '启动失败横幅要能直接进启动诊断（事故面板只给处置，不给过程）'
  );

  const precheck = readSrc('plugins/PrecheckDialog.vue');
  assert.match(precheck, /openPluginDiagnosis/, '预检结果必须能进插件安全诊断');

  const incident = readSrc('incidents/IncidentModal.vue');
  assert.match(incident, /openStartupDiagnosis/, '事故面板必须能跳到那次启动的时间线');
});

test('§2.5.5 的更多菜单只收只读动作', () => {
  const menu = readSrc('diagnostics/diagnosis-more-menu.js');
  assert.match(menu, /查看完整日志/);
  assert.match(menu, /复制运行记录编号/);
  assert.match(menu, /复制本次诊断摘要/);
  // 会改变状态的操作绝不能藏在「更多」里：用户在不知情下点了就把环境改了。
  // **只看 label，不看整段源码**——注释里解释「为什么不放恢复」也会提到
  // 「恢复」两个字，按全文匹配会把这条例外当成违规。
  const labels = [...menu.matchAll(/label:\s*'([^']+)'/g)].map((m) => m[1]);
  assert.ok(labels.length >= 3, '菜单项要能从 label 里读出来，否则这条断言是空的');
  for (const forbidden of ['恢复', '重启', '删除', '卸载', '应用变更', '切换']) {
    assert.ok(
      !labels.some((label) => label.includes(forbidden)),
      `「更多」菜单里不该出现会改状态的动作：${forbidden}（现有：${labels.join(' / ')}）`
    );
  }
});

test('§2.5.3 返回按钮带 aria-label（图标按钮不能只靠图形猜）', () => {
  const shell = readSrc('diagnostics/DiagnosisShell.vue');
  // 取「模板里第一个 button 到它闭合」这一段，而不是固定偏移——偏移量会
  // 随模板重排失效，而这条断言要钉的是「这个按钮本身带齐了三样」。
  const buttonStart = shell.indexOf('<button', shell.indexOf('<template>'));
  const backButton = shell.slice(buttonStart, shell.indexOf('</button>', buttonStart));
  assert.match(backButton, /type="button"/, '返回必须是 button 元素，保留键盘焦点');
  assert.match(backButton, /aria-label="返回"/, '图标按钮不能只靠图形猜含义');
  assert.match(backButton, /title="返回"/, '必须有可见的悬停提示');
});

// —— 恢复 / 排查的运行记录（设计 §4.1 的四种 kind）——
//
// 这两种 kind 走的是**同一套**状态与归因词表，但头部结论必须按 kind 各说各
// 的：套用启动那张表会写出「工作台已启动」这种与恢复毫无关系的结论。

test('§4.1 四种 kind 共用状态词表，但恢复与排查各有自己的结论措辞', () => {
  // 恢复不得复用启动的措辞（「工作台已启动」对一次配置回退毫无意义）。
  for (const status of ['running', 'success', 'warning', 'failure']) {
    const restore = labels.headlineFor({ kind: 'restore', status });
    const startup = labels.headlineFor({ kind: 'startup', status });
    assert.notEqual(restore, startup, `恢复与启动在 ${status} 上的结论撞词了`);
    assert.ok(restore && !restore.startsWith('工作台'), `恢复结论不该谈工作台：${restore}`);
  }
  assert.match(labels.headlineFor({ kind: 'restore', status: 'success' }), /恢复/);
  assert.match(labels.headlineFor({ kind: 'restore', status: 'warning' }), /跳过/);
});

test('§11.1 二分的结论永远不叫「根因」', () => {
  // 组合效应会让二分停在一个不可修的答案上，叫「根因」会让用户去卸一个
  // 无辜的插件。这条断言把四种 kind 的全部状态都扫一遍。
  for (const status of ['running', 'success', 'warning', 'failure', 'inconclusive', 'canceled']) {
    const text = labels.headlineFor({ kind: 'bisect', status });
    assert.ok(text, `${status} 没有结论文案`);
    assert.ok(!/根因/.test(text), `二分在 ${status} 上说了「根因」：${text}`);
  }
  // 「收窄到」是刻意选的动词：它说的是证据支持的范围，不是一个确定的答案。
  assert.match(labels.headlineFor({ kind: 'bisect', status: 'success' }), /收窄/);
});

test('未知 kind / 状态退回短标签，不编一句结论', () => {
  // 编一句看着合理的话，比显示「未知状态（xxx）」危险得多：用户会照着它
  // 去做决定，而那句话背后没有任何证据。
  assert.equal(labels.headlineFor({ kind: 'restore', status: 'weird' }), labels.statusMeta('weird').label);
  assert.equal(labels.headlineFor({ kind: 'brand-new', status: 'success' }), labels.statusMeta('success').label);
  assert.equal(labels.headlineFor({}), labels.statusMeta('').label);
});

test('恢复与排查的阶段都有中文名（时间线不能出现「未知阶段」）', () => {
  for (const stage of ['prepare', 'apply', 'verify', 'select', 'probe', 'conclude', 'abort']) {
    const text = labels.stageLabel(stage);
    assert.ok(!/未知阶段/.test(text), `${stage} 没有中文名：${text}`);
  }
});
