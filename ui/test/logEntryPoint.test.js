// 日志入口唯一，而独立窗口没有变成不可达（2026-10-07 定，两次修正）。
//
// 起因：概览页上**同时**有两个日志入口，而且做的事不一样——「当前内核」卡主
// 操作排里的「查看日志」按钮直接开独立全屏窗口（跳过列表），「系统健康 →
// 日志系统」那一行走页内弹层。同一屏两个入口、点开结果还不一致，用户没法建立
// 预期（用户原话：统一查看日志的体验）。
//
// **第一次改法（错）**：留一个入口，把按钮与它的「直接开窗」捷径一起删掉，
// 全应用统一走弹层。用户后来指出「查看日志按钮，应该是弹出 window 的形式
// 查看」——**统一的是入口，不是容器**。删掉捷径的同时把幸存的那个也降级成了
// 弹层，等于顺手把「点一下就读长日志」这件事整个换掉了。
//
// **现在**：入口仍然只有一个（「系统健康 → 日志系统」），点开的是独立窗口；
// 带具体证据的事故 / 预检 / 诊断入口走弹层，那不是「去看看有什么日志」，
// 是「去看这一份」，弹层里能对照着切签。事故 / 预检 / 诊断页本来也都是弹层。
//
// **代价必须盯住两处**：① 独立窗口的通路现在有两条（这一格 + 弹层「全屏」），
// 两条都删光能力就无声地没了，页面上只是少了入口；② 两个入口是两个组件，
// 极易各写一份实现，于是又回到「行为不一致」。下面第 ④ ⑤ 条分别钉这两处。
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const read = (path) => readFileSync(path, 'utf8');

/**
 * 去掉注释再断言。
 *
 * 少了这一步，第一条断言会**误报**：代码里必须解释「按钮为什么搬走了」，而那段
 * 说明里必然出现「查看日志」四个字——断言的是「界面上没有这个按钮」，不是
 * 「文件里没出现过这个词」。
 *
 * 行注释那条正则要求整行以斜杠斜杠开头（后面紧跟可选空白），所以代码里
 * `urlText` 那类含协议头的字符串不会被误删。
 */
function code(path) {
  return read(path).replace(/<!--[\s\S]*?-->/g, '').replace(/^\s*[/][/].*$/gm, '');
}

const OVERVIEW = 'ui/src/shell/OverviewPanel.vue';
const TOWER = 'ui/src/diagnostics/ControlTower.vue';
const LOGS = 'ui/src/logs/logs.js';
const LOG_MODAL = 'ui/src/logs/LogModal.vue';

test('概览页不再有第二个日志入口', () => {
  const overview = code(OVERVIEW);
  assert.doesNotMatch(overview, /查看日志/, '主操作排里的「查看日志」按钮已迁到系统健康');
  assert.doesNotMatch(overview, /showLogs/, '概览页不再自己打开日志弹层');
  assert.doesNotMatch(
    overview,
    /openLogsWindow/,
    '「直接开独立全屏窗口」那条捷径已删除：能力搬进了系统健康那一格，' +
      '捷径留着就又是同一屏两个入口'
  );
});

test('日志入口在「系统健康」里，且那一行自己说得出是入口', () => {
  const tower = code(TOWER);
  assert.match(tower, /'日志系统'/, '系统健康必须保留日志那一格');
  assert.match(tower, /cell\(\s*'logs'/, '日志格必须带 logs 动作，点开才有反应');
  assert.match(
    tower,
    /openLogWindow\(\)/,
    '点开开独立窗口：主壳 1040×748 不可缩放，在里面读长日志永远只有这么大'
  );

  // 入口只剩这一处之后，「47 份 ›」这种读数样式就等于把入口藏起来了：
  // 用户想看日志时扫过去读到的只有份数，看不出这里能点开。
  assert.match(
    tower,
    /key === 'logs' \? '查看'/,
    '日志格的右侧提示必须是「查看」而不是 `›`：它是唯一的日志入口，' +
      '长成读数样式就等于没有入口'
  );

  // 提示由 cell() 一次算好：模板不再自己分支。这类"同一句话在两处各判一次"
  // 的写法，改一边另一边会静默失配（AGENTS.md 的跨模块重复纪律）。
  assert.match(tower, /v-if="row\.hint"/, '模板应直接渲染 cell() 算好的提示');
  assert.doesNotMatch(
    tower,
    /row\.unavailable/,
    'unavailable 字段已无读取方（它的唯一用途是挑「点击重试」还是 `›`）'
  );
});

test('确认过一份日志都没有时，那一格不当入口', () => {
  const tower = code(TOWER);
  // 可点 = 读失败（点了重试）或确实读到至少一份（点了开窗）。「暂无」这一档
  // 不可点：开出来是一个左侧空列表 + 「请从左侧选择日志文件」的空窗口，
  // 点了只会让人以为窗口坏了。
  assert.match(
    tower,
    /logModal\.listState === 'ready' && logModal\.files\.length/,
    '「暂无」必须是不可点的那一档'
  );
  assert.match(tower, /return null/, '其余状态返回 null = 没有可执行的动作');
});

test('「直接开窗」那条捷径已删除，不留第二个行为不同的入口', () => {
  assert.doesNotMatch(
    code(LOGS),
    /export function openLogsWindow/,
    'openLogsWindow 已无调用方，应当删除（留在原地等于给下一个人留一个' +
      '与统一路径行为不同的日志入口）'
  );
});

test('独立窗口两条通路都在，且共用 logs.js 里的同一份实现', () => {
  // 通路 ①：系统健康那一格（概览）。通路 ②：弹层里的「全屏」按钮。
  // 两条都删光不会报任何错，只是用户再也开不了大屏看日志。
  assert.match(code(LOG_MODAL), /全屏/, '弹层里必须保留「全屏」这一枚按钮');

  // 「一份实现」是这条判据真正要守的东西：两个入口各写一份 invoke，迟早一个
  // 改了另一个没改，于是又回到「同一屏两个日志入口、点开结果不一致」。
  const logs = code(LOGS);
  assert.match(
    logs,
    /export function openLogWindow/,
    '开窗动作住在共享层 logs.js，两个入口都从这里调'
  );
  assert.equal(
    (logs.match(/invoke\('open_log_window'/g) || []).length,
    1,
    'open_log_window 只能在 logs.js 里发一次'
  );

  // 两个入口都真的接到了那一份实现，而不是各自 import 了一个同名函数。
  assert.match(
    code(TOWER),
    /import\s*\{[^}]*\bopenLogWindow\b[^}]*\}\s*from\s*'\.\.\/logs\/logs\.js'/,
    '控制塔必须从 logs.js 引入 openLogWindow'
  );
  assert.match(
    code(LOG_MODAL),
    /import\s*\{[^}]*\bopenLogWindow\b[^}]*\}\s*from\s*'\.\/logs\.js'/,
    '弹层必须从 logs.js 引入 openLogWindow'
  );
});
