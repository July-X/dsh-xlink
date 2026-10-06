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

// —— 审查 2026-10-06 的意见 ——————————————————————————————————————

test('P1-01 诊断层压在进度浮层之上（否则「查看启动诊断」点了没反应）', () => {
  const diag = readSrc('diagnostics/diagnostics.css');
  const ladder = (name) => {
    const m = diag.match(new RegExp(`--${name}:\\s*(\\d+)`));
    return m ? Number(m[1]) : null;
  };
  // 浮层层级必须高于 Element Plus 弹层基线 2000：事故面板里触发的长任务
  // 浮层不能被对话框盖住（theme.css 里那条注释写的就是这件事）。
  assert.ok(ladder('z-progress') > 2000, '进度浮层必须高于 el-overlay 的 2000');
  assert.ok(
    ladder('z-diagnosis') > ladder('z-progress'),
    '诊断层是进度浮层的后续页面，必须压在它之上'
  );
  // theme.css 里不得再出现裸的浮层魔数——两份各写各的必然漂。
  const theme = readSrc('theme.css');
  const overlay = theme.match(/\.progress-overlay\s*\{[^}]*z-index:\s*([^;]+);/s);
  assert.ok(overlay, '必须能从 theme.css 读到 .progress-overlay 的 z-index');
  assert.match(overlay[1], /var\(--z-progress\)/, 'theme.css 里的层级应引用 CSS 阶梯');
});

test('P1-02 日志窗口定位到点名的那一份，而不是列表第一份', async () => {
  const logs = await import('../src/logs/logs.js');
  const names = ['dsh-default-kernel-2026-10-06.log', 'precheck-2026-10-06.log'];
  // 传入完整路径（诊断记录里的证据就是这样）→ 取 basename 匹配。
  logs.logModal.askedEvidence = '/Users/me/.dsh-xlink/dsh/desktop/logs/precheck-2026-10-06.log';
  assert.equal(logs.resolvePreferred(names), 'precheck-2026-10-06.log');
  // Windows 的反斜杠路径同样吃。
  logs.logModal.askedEvidence = 'C:\\Users\\me\\logs\\dsh-default-kernel-2026-10-06.log';
  assert.equal(logs.resolvePreferred(names), 'dsh-default-kernel-2026-10-06.log');
  // 点名的文件不在集合里（被轮转清理）→ 必须报 missing，**不能**退回第一份。
  logs.logModal.askedEvidence = '/tmp/logs/gone.log';
  assert.equal(logs.resolvePreferred(names), 'missing');
  // 没点名 → 走普通流程（null，不是 missing）。
  logs.logModal.askedEvidence = '';
  assert.equal(logs.resolvePreferred(names), null);
  // 集合里没有的文件名绝不能被当成可选目标：那等于让前端读 logs/ 之外的文件。
  logs.logModal.askedEvidence = '/etc/passwd';
  assert.equal(logs.resolvePreferred(names), 'missing');
});

test('概览控制塔不重复正下方「当前内核」卡已经说过的话', () => {
  const tower = readSrc('diagnostics/ControlTower.vue');
  const health = tower.slice(tower.indexOf('const health = computed'), tower.indexOf('const wiringText'));
  // 三行读数在「当前内核」卡里逐字重复一次（版本徽标 / 运行状态胶囊 /
  // Node.js 行），侧栏品牌区还有第三份。它们在 480px 窄窗里占掉 111px，
  // 换来的信息量是零。异常另有出口：Node 不达标进「需要关注」，内核没装
  // 进首屏 callout，运行状态在「当前内核」卡与侧栏。
  for (const key of ['kernel', 'runtime', 'node']) {
    assert.ok(
      !new RegExp(`cell\\('${key}'`).test(health),
      `系统健康不该再列 ${key}：它与正下方的「当前内核」卡重复`
    );
  }
  // 剩下四项必须两列排布，否则一行四项还是会把概览顶下去。
  assert.match(tower, /class="diag-rows diag-rows--grid"/);
  const css = readSrc('diagnostics/diagnostics.css');
  assert.match(css, /\.diag-rows--grid \{[^}]*grid-template-columns: repeat\(2/);
  // 详情一律单行省略：诊断正文可能很长，卡片高度不该由日志长度决定。
  assert.match(css, /\.diag-row__label \{[^}]*text-overflow: ellipsis/);
});

// 控制塔的三张卡压紧到 ~252px（用户 2026-10-06：概览页纵向空间被这三块占掉太多）。
// 判据不写死具体数值——写死就变成「调间距时顺手改测试」的借口；这里只钉**结构**：
// ① 三张卡都带 --tower；② 覆盖确实比基线紧；③ 行用 padding-block 而不是 padding。
test('控制塔的紧凑化只落在 --tower 上，且行覆盖必须用 padding-block', () => {
  const tower = readSrc('diagnostics/ControlTower.vue');
  const css = readSrc('diagnostics/diagnostics.css');

  // ① 三张卡都要挂上，否则漏挂的那张按基线渲染，三块高度参差不齐。
  //    正则必须锚到类名结束：`diag-card__title` / `__aside` 前缀相同，用宽匹配
  //    会把它们一起数进来（第一版就数出 8 个）。
  const cards = tower.match(/class="diag-card( diag-card--tower)?"/g) || [];
  assert.equal(cards.length, 3, '控制塔仍是三张卡');
  for (const card of cards) {
    assert.match(card, /diag-card--tower/, `${card} 必须带 diag-card--tower`);
  }

  // ② 覆盖必须比基线小，否则「紧凑化」只剩一个类名。
  const base = css.match(/\.diag-card \{([^}]*)\}/);
  const tight = css.match(/\.diag-card\.diag-card--tower \{([^}]*)\}/);
  assert.ok(tight, '必须能找到 .diag-card.diag-card--tower 覆盖规则');
  // 单位可选：`margin-bottom: 0` 合法地不带 px，要求 `\\d+px` 会解析成 NaN，
  // 报错变成「NaN !== 0」——读起来像值算错了，其实是正则没匹配上。
  const px = (body, key) => Number((body.match(new RegExp(`${key}:\\s*(\\d+)(?:px)?`)) || [])[1]);
  assert.ok(
    px(tight[1], 'padding') < px(base[1], 'padding'),
    `控制塔卡片 padding 应小于基线 ${px(base[1], 'padding')}px`
  );
  // 必须**恰好是 0**，不是「小于基线」。小于基线会放过 6px——而 `.panel` 的 gap
  // 就是 6px，flex 不折叠的 margin 会直接叠上去，实测那一列就变成 12/12/12/6
  // 的不齐（2026-10-06 用户指出）。缝只由容器 gap 提供。
  assert.equal(
    px(tight[1], 'margin-bottom'),
    0,
    '控制塔卡片的下外边距必须是 0：间距由 .panel 的 gap 统一提供'
  );
  // 声明本身也必须在：作用域选择器只在**它自己声明过的属性上**赢过基线，
  // 把这条声明删掉等于让 `.diag-card` 的 `margin-bottom: 10px` 原样回来。
  assert.match(
    tight[1],
    /margin-bottom:\s*0(px)?\s*;/,
    '必须显式写 margin-bottom: 0，删掉声明会让基线的 10px 回来'
  );

  // ③ **这一条是静默失效的守卫**：列分隔靠
  //    `.diag-rows--grid .diag-row:nth-child(2n) { padding-left: 14px }`，
  //    与 `.diag-card--tower .diag-row` 同为 (0,2,0)。把 `padding-block` 写成
  //    简写 `padding` 就会把 14px 一起清成 0，两列读数贴在一起——不报错、
  //    typecheck 与 build 全绿，只有看图才发现。
  const rowOverride = css.match(/\.diag-card--tower \.diag-row \{([^}]*)\}/);
  assert.ok(rowOverride, '必须能找到 .diag-card--tower .diag-row 覆盖规则');
  assert.match(
    rowOverride[1],
    /padding-block:\s*\d+px/,
    '行覆盖必须用 padding-block（简写 padding 会清掉列分隔的 padding-left）'
  );
  assert.doesNotMatch(
    rowOverride[1],
    /(^|[^-])padding:\s*\d/,
    '行覆盖里不许出现 padding 简写'
  );
  // 列分隔本身也得还在。
  assert.match(css, /\.diag-rows--grid \.diag-row:nth-child\(even\)[^}]*padding-left:\s*14px/);

  // 基线本身不许被顺手改小：.diag-card / .diag-row 还被 5 个诊断页共用，
  // 那些是独立窗口、内容量大、用户会逐条读。
  assert.ok(px(base[1], 'padding') >= 10, '基线卡片 padding 不该跟着控制塔一起收');
  const baseRow = css.match(/\.diag-row \{([^}]*)\}/);
  assert.ok(
    px(baseRow[1], 'padding') >= 8,
    '基线行 padding 不该跟着控制塔一起收（诊断页仍在用）'
  );
});

test('P1-05 最近操作按 kind 分派，四种都不落到启动时间线', async () => {
  const src = readSrc('diagnostics/diagnostics.js');
  // 分派表是契约：四类里前三类各有各的视图，plugin-precheck 走插件视图。
  assert.match(src, /kind === 'startup'\) return openStartupDiagnosis/);
  assert.match(src, /kind === 'restore' \|\| kind === 'bisect'[\s\S]{0,120}openOperationDiagnosis/);
  // 控制塔不得再无条件调 openStartupDiagnosis。
  const tower = readSrc('diagnostics/ControlTower.vue');
  assert.ok(!/openStartupDiagnosis\(/.test(tower), '控制塔必须按 kind 分派，不能一律当启动诊断');
  assert.match(tower, /openRunDiagnosis\(latestRun\.value, 'overview'\)/);
  // 四种 kind 都要在标题里说出来。
  assert.match(tower, /启动 \/ 预检 \/ 恢复 \/ 排查/);
  // 预检项必须带上那一条运行记录，不能是空 spec。
  assert.match(tower, /run: lastPrecheck/);
});

test('P2-01 通道信封里的 runId 优先于调用方传的那个', async () => {
  const diag = await import('../src/diagnostics/diagnostics.js');
  const envelope = (runId, seq) =>
    JSON.stringify({ type: 'diagnostic-event', runId, event: { seq, stage: 'sandbox-create', status: 'success', message: 'x' } });
  // 解析器必须把 runId 带出来——它此前解析完就丢了。
  assert.equal(diag.parseChannelMessage(envelope('run-a', 1)).runId, 'run-a');
  // 纯文本与坏 JSON 不许带出 runId。
  assert.equal(diag.parseChannelMessage('hello').runId, '');
  assert.equal(diag.parseChannelMessage('{oops').runId, '');

  diag.clearLiveEvents();
  // 调用方传的是「当前正在跑的那次」；信封是后端与这条事件一起生成的。
  // 两者不一致恰恰就是迟到的上一次消息，用调用方的值会算到当前这次头上。
  diag.ingestChannelMessage(envelope('run-old', 9), 'run-current');
  assert.equal(diag.diagnosticStore.liveEvents[0].__runId, 'run-old');
  diag.ingestChannelMessage(envelope('run-old', 10), 'run-current');
  assert.equal(diag.diagnosticStore.liveEvents.length, 2, '同一条记录按 seq 接着追加');
  // 换了一条记录就重开时间线，不接在旧记录后面。
  diag.ingestChannelMessage(envelope('run-new', 1), 'run-current');
  assert.equal(diag.diagnosticStore.liveEvents.length, 3);
  assert.equal(diag.diagnosticStore.liveEvents[2].__runId, 'run-new');
  diag.clearLiveEvents();
});

test('P2-03 阶段计数按阶段集合算，不按事件条数', () => {
  const one = (stage, status = 'success') => ({ stage, status });
  // 一个阶段推三条事件时，事件数是 3，阶段数是 1。
  const events = [
    one('sandbox-create'), one('sandbox-create', 'running'), one('sandbox-create'),
    one('baseline', 'failure'),
  ];
  const p = labels.stageProgress('plugin-precheck', events);
  assert.equal(p.total, 5, '分母来自阶段序列，不是事件条数');
  assert.equal(p.done, 2, '走过两个阶段（sandbox-create 与 baseline）');
  assert.equal(p.failed, 1);
  assert.equal(p.pending, 3);
  // 序列外的 stage 不进分母——新版新增的阶段会让它显示成 6/5。
  const extra = labels.stageProgress('plugin-precheck', [...events, one('quarantine-v3')]);
  assert.equal(extra.total, 5, '未知 stage 不许把分母撑大');
  assert.equal(extra.done, 2, '未知 stage 不许把分子撑大');
  // 未知 kind 不编分母：按实际出现过的 distinct stage 算。
  const unknown = labels.stageProgress('brand-new', [one('a'), one('a'), one('b')]);
  assert.equal(unknown.total, 2);
  assert.equal(unknown.done, 2);
  // 四种 kind 都要有序列，否则「完成 N / M」里的 M 是空的。
  for (const kind of ['startup', 'plugin-precheck', 'restore', 'bisect']) {
    assert.ok(labels.STAGE_SEQUENCES[kind]?.length, `${kind} 缺阶段序列`);
  }
});

// —— P1-03：两阶段契约（2026-10-06 用户拍板）—————————————————————
//
// 「预检通过」与「已经装上」是两件事。UI 上任何一处把它们混为一谈，用户就会
// 在工作台里找半天为什么插件不在——所以这三条钉的是**文案与判据**，不是样式。

test('P1-03 预检跑完不装；「应用变更」是唯一会改动真实实例的动作', () => {
  const plugins = readSrc('plugins/plugins.js');
  // 预检跑完的提示必须说清「还没装」——含糊的 done 文案是这套工具最容易犯的错。
  // 只在 `precheckPlugin` 那段里查：预检关闭时走的 `installPlugin` 确实会装上，
  // 它的「已安装」文案是对的。
  const precheckFn = plugins.slice(plugins.indexOf('export function precheckPlugin'));
  assert.match(precheckFn, /还没有装进当前实例/);
  assert.ok(
    !/done: '插件 ' \+ raw \+ ' 已安装/.test(precheckFn),
    '预检的完成文案不得说「已安装」'
  );
  // 应用走的是独立命令，不是把预检再跑一遍。
  assert.match(plugins, /cmd: 'plugin_precheck_apply'/);
  // 后端那条命令存在且被授权。
  const commands = readFileSync(resolve('src-tauri/src/plugins/precheck_cmd.rs'), 'utf8');
  assert.match(commands, /pub async fn plugin_precheck_apply/);
  // 注册在 generate_handler! 里，搬过模块也要在（曾经漏过一次 ACL）。
  const lib = readFileSync(resolve('src-tauri/src/lib.rs'), 'utf8');
  assert.match(lib, /plugins::precheck_cmd::plugin_precheck_apply/);
  const acl = readFileSync(resolve('src-tauri/permissions/app-commands.json'), 'utf8');
  assert.match(acl, /"plugin_precheck_apply"/, '新命令必须在 ACL 白名单里');
});

test('P1-03 只有预检通过且未安装时能应用，其余情况给出理由', () => {
  const dialog = readSrc('plugins/PrecheckDialog.vue');
  // 判据是 verdict === 'pass' && !installed，两条都要在。
  assert.match(dialog, /report\.value\.verdict === 'pass' && !report\.value\.installed/);
  // 禁用必须带理由：一个不说理由的灰按钮会让人以为界面坏了。
  assert.match(dialog, /applyDisabledReason/);
  assert.match(dialog, /预检没通过/);
  assert.match(dialog, /预检没能完成/);
  // 「没有改动当前实例」要单独占一行大字，不能只藏在 summary 里。
  assert.match(dialog, /class="precheck-notinstalled"/);
  assert.match(dialog, /没有改动<\/b>当前实例|这次预检<b>没有改动<\/b>/);
});

test('P1-03 后端两条 fail-open 路径都不再直接安装', () => {
  const precheck = readFileSync(resolve('src-tauri/src/plugins/precheck.rs'), 'utf8');
  // 沙盒建不起来 / 基线没起来：这两条路径此前是 fail-open（直接装上），
  // 而「预检环境坏了」不构成装它的理由——那等于把「没验过」说成「验过没问题」。
  assert.ok(
    !/预检未能进行，插件已直接安装/.test(precheck),
    '基线失败路径不得再直接安装'
  );
  assert.ok(
    !/预检环境不可用，插件已直接安装/.test(precheck),
    '沙盒失败路径不得再直接安装'
  );
  // 应用阶段照样重查守卫：置灰是给人看的，这条是给并发的。
  const apply = precheck.slice(precheck.indexOf('pub fn plugin_apply'));
  assert.match(apply, /instance_kernel_running\(/);
  // 应用前打快照，且打不出来不阻断（它是兜底不是前提）。
  assert.match(apply, /reason::PRE_CHANGE/);
  assert.ok(
    !/能否安装|Err\(AppError.*pre-change/.test(apply),
    '快照失败不应变成硬失败'
  );
});

// —— 窄窗：诊断层浮层塌缩（2026-10-06 实机目视发现）——————————————————
//
// 症状是「显示不全」：右侧一条竖排的碎字，溢出到窗口外，把卡片右半边盖住。
// 复现过：Element Plus 的 `.el-popper` 默认 `min-width: 10px` +
// `overflow-wrap: break-word`，被挤到最小宽度时中文一个字一行。

test('诊断层的「更多」弹层不会被挤成逐字竖排', () => {
  const css = readSrc('diagnostics/diagnostics.css');
  // 只给上限不够——它仍能塌到 EP 的 min-width:10px。
  assert.match(css, /\.diagnosis-more-popper\s*\{[^}]*min-width:\s*148px/s);
  // 只给下限也不够——菜单长时会顶出窗口。
  assert.match(css, /max-width:\s*min\(320px, calc\(100vw - 24px\)\)/);
  // 逐字断行要从根上关掉，否则宽度算错时菜单会读成竖排的一个字。
  assert.match(css, /\.diagnosis-more-popper \.el-dropdown-menu__item\s*\{[^}]*white-space:\s*nowrap/s);

  const shell = readSrc('diagnostics/DiagnosisShell.vue');
  // 诊断层是 position:fixed + overflow:hidden，弹层必须 teleport 到 body。
  assert.match(shell, /<el-dropdown[\s\S]{0,220}\bteleported\b/);
  // 触发器贴着窗口右缘：默认 bottom-start 会向右展开而放不下，向左对齐才稳。
  assert.match(shell, /placement="bottom-end"/);
});

test('诊断页必须说清这条记录是什么时候的', () => {
  // 时刻不是装饰：一条几小时前的失败记录和刚才那次跑的一模一样，没有时刻
  // 用户读到的就是「现在还是这样」。2026-10-06 用户拿着一张 18:09 的截图来报
  // 「还是过不了检测」，而那时修复已落地一小时。
  const old = { status: 'inconclusive', startedAtMs: Date.now() - 3 * 3600 * 1000 };
  assert.match(labels.staleRunHint(old), /^这是 3 小时前那次运行的结果/);
  assert.equal(labels.runAgeLabel(old), '3 小时前');
  // 十分钟内不啰嗦。
  const fresh = { status: 'inconclusive', startedAtMs: Date.now() - 60 * 1000 };
  assert.equal(labels.staleRunHint(fresh), '');
  // 成功的记录不催着重跑；还在跑的记录谈不上过期；缺时刻的不编。
  assert.equal(labels.staleRunHint({ status: 'success', startedAtMs: old.startedAtMs }), '');
  assert.equal(labels.staleRunHint({ status: 'running', startedAtMs: old.startedAtMs }), '');
  assert.equal(labels.staleRunHint({ status: 'inconclusive' }), '');
  // 三个诊断视图的 meta 行都要带时刻，抄一份就会漂。
  for (const file of ['StartupDiagnosis.vue', 'OperationDiagnosis.vue', 'PluginDiagnosis.vue']) {
    const src = readSrc(`diagnostics/${file}`);
    assert.match(src, /runAgeLabel\(run\)/, `${file} 的头部必须显示这条记录有多旧`);
  }
});
