// 日志入口唯一，且独立窗口没有变成不可达（2026-10-07）。
//
// 起因：概览页上**同时**有两个日志入口，而且做的事不一样——「当前内核」卡主
// 操作排里的「查看日志」按钮直接开独立全屏窗口（跳过列表），「系统健康 →
// 日志系统」那一行走页内弹层。同一屏两个入口、点开结果还不一致，用户没法建立
// 预期（用户原话：统一查看日志的体验）。
//
// 改法是留一个：按钮与它那条「直接开窗」的捷径（`openLogsWindow`）一起删掉，
// 全应用统一走弹层——事故 / 预检 / 诊断页本来也都是弹层。
//
// **代价必须盯住**：删掉捷径后，独立全屏窗口的唯一通路变成弹层里那枚「全屏」
// 按钮。它一旦被顺手删掉，能力就无声地没了——没有测试会红，页面上只是少了
// 一个入口。所以下面第 ④ 条钉它。
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
    '「直接开独立全屏窗口」那条捷径已删除：它与那一行的弹层路径行为不同，' +
      '两个入口并存正是用户要消除的东西'
  );
});

test('日志入口在「系统健康」里，且那一行自己说得出是入口', () => {
  const tower = code(TOWER);
  assert.match(tower, /'日志系统'/, '系统健康必须保留日志那一格');
  assert.match(tower, /cell\(\s*'logs'/, '日志格必须带 logs 动作，点开才打开弹层');
  assert.match(tower, /showLogs\(\)/, '点开走日志弹层（全应用统一的那条路径）');

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

test('「直接开窗」那条捷径已删除，不留第二个行为不同的入口', () => {
  assert.doesNotMatch(
    code(LOGS),
    /export function openLogsWindow/,
    'openLogsWindow 已无调用方，应当删除（留在原地等于给下一个人留一个' +
      '与统一路径行为不同的日志入口）'
  );
});

test('独立全屏窗口仍然可达 —— 弹层里的「全屏」按钮不能丢', () => {
  // 这是删掉 openLogsWindow 之后**唯一**的独立窗口通路。它没了不会报任何错，
  // 只是用户再也开不了大屏看日志。
  const modal = code(LOG_MODAL);
  assert.match(
    modal,
    /open_log_window/,
    'LogModal 仍须调用 open_log_window（弹层里的「全屏」按钮）'
  );
  assert.match(modal, /全屏/, '弹层里必须保留「全屏」这一枚按钮');
});
