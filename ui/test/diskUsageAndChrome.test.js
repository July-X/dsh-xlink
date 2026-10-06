import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

import { readShellSource } from '../../scripts/lib/shell-source.mjs';

// 两条会静默失效的约定，都得钉住：
//  ① 顶部版本带（dev 鲸眼红 / release Gitea 绿）铺满**整窗**而不是半屏/四分之三屏；
//  ② 「磁盘用量」四个占用类型的配色按 **group.id** 取，不是按 label，也不是按顺序。

const theme = readFileSync('ui/src/theme.css', 'utf8');
const versionsPanel = readFileSync('ui/src/kernel/VersionsPanel.vue', 'utf8');
const diskusage = readShellSource('diskusage.rs');

// --- ① 顶部渐变带 ---

// 只匹配这两条 body 版本带的 background-image。写成通用渐变断言会把
// .mac-titlebar / .app-bg 那一堆同形规则一并卷进来，红了也不知道是哪条。
function versionBandGradient(selector) {
  const rule = theme.match(new RegExp(`body\\.${selector}\\s*\\{[^}]*background-image:\\s*([^;]+);`, 's'));
  assert.ok(rule, `必须能找到 body.${selector} 的 background-image`);
  return rule[1];
}

test('顶部版本带铺满整窗：起点 α 与降幅不变，只把归零位置从半屏拉到窗底', () => {
  for (const [selector, rgb] of [
    ['dev-build', '255, 45, 48'],
    ['rel-build', '96, 153, 38'],
  ]) {
    const gradient = versionBandGradient(selector);

    // 起点与终点都**不许动**：用户 2026-10-06 要求的是「渐变色、变化幅度都不变」，
    // 只把覆盖范围从 50%/75% 拉到全高。改 α 就是另一件事了。
    assert.match(
      gradient,
      new RegExp(`rgba\\(${rgb}, 0\\.25\\) 0%`),
      `${selector}：顶端 α 必须仍是 0.25（渐变色与变化幅度不变）`,
    );
    assert.match(
      gradient,
      new RegExp(`rgba\\(${rgb}, 0\\) 100%`),
      `${selector}：必须在 100%（窗底）归 0，而不是半屏`,
    );
    assert.doesNotMatch(
      gradient,
      /rgba\([^)]*,\s*0\)\s+(?:[1-9]\d?|0)%/,
      `${selector}：终点百分比必须正好是 100%`,
    );
  }
});

// --- ② 四个占用类型的配色 / 说明 ---

// 后端 build_group 的第一个实参就是 group.id。四类写死在 `build_group("…", …)`
// 上，测试里那个多行调用的 id 排在下一行，所以这里按「id 后面跟逗号或换行」取。
const backendGroupIds = Array.from(
  diskusage.matchAll(/build_group\(\s*"(kernels|instances|stores|logs)"/g),
  (m) => m[1],
);

test('后端四个占用分类的 id 在前端都有配色与说明', () => {
  assert.deepEqual(
    [...backendGroupIds].sort(),
    ['instances', 'kernels', 'logs', 'stores'],
    '后端 build_group 的分类集合变了，本测试要先更新',
  );

  for (const id of backendGroupIds) {
    assert.match(
      versionsPanel,
      new RegExp(`\\b${id}:\\s*\\{`),
      `前端 USAGE_TYPES 缺少 ${id}——未知 id 会回落到 --accent，四类里有一类与其它同色`,
    );
    assert.match(
      versionsPanel,
      new RegExp(`\\b${id}:\\s*\\{[\\s\\S]{0,220}?color:\\s*'#[0-9a-fA-F]{6}'`),
      `${id} 必须显式给一个十六进制色值`,
    );
    assert.match(
      versionsPanel,
      new RegExp(`\\b${id}:\\s*\\{[\\s\\S]{0,400}?tip:\\s*'[^']{8,}'`),
      `${id} 必须给一句能读的说明（悬停提示用）`,
    );
  }
});

test('四个类型的颜色互不相同（区分开了才叫区分）', () => {
  const colors = Array.from(versionsPanel.matchAll(/color:\s*'(#[0-9a-fA-F]{6})'/g), (m) => m[1]);
  assert.equal(colors.length, 4, `应恰好读到 4 个类型色，实际读到 ${colors.length}：${colors.join(' ')}`);
  assert.equal(new Set(colors).size, 4, `四个类型色撞了：${colors.join(' ')}`);
});

test('配色按 group.id 取，不按 label 或瓦片顺序', () => {
  // usageColor 只认 id：label 是文案，改一次「实例数据」不该让配色失配。
  assert.match(
    versionsPanel,
    /function usageColor\(group\)\s*\{\s*const found = USAGE_TYPES\[group && group\.id\];/,
    'usageColor 必须用 group.id 查表',
  );
  assert.doesNotMatch(
    versionsPanel,
    /function usageColor\(group\)[\s\S]{0,200}?group\.label/,
    'usageColor 不许读 label',
  );
});

test('瓦片标题挂的是「这一类是什么」的气泡，且 el-tooltip 真的渲染了触发器', () => {
  assert.match(
    versionsPanel,
    /<el-tooltip[^>]*>\s*<template #content>\s*<div class="card-info-tooltip usage-type-tip">\s*\{\{ groupTip\(group\) \}\}/,
    '瓦片标题必须用 groupTip 的文案挂 tooltip',
  );
  // el-tooltip 的默认插槽里必须有一个真实元素，否则 Element Plus 取不到触发器、
  // 整块不显示——而模板里不会报错，typecheck 与 build 全绿。
  assert.match(
    versionsPanel,
    /\{\{ groupTip\(group\) \}\}\s*<\/div>\s*<\/template>\s*<span class="usage-tile-label">/,
    'tooltip 默认插槽里必须紧跟一个真实元素（.usage-tile-label）作为触发器',
  );
  // 原生 title 已被 tooltip 取代：两套并存时用户会看到两个提示，
  // 且原生 title 要悬停约 1s 才出、样式也与界面不一致。
  assert.doesNotMatch(
    versionsPanel,
    /class="usage-tile-label"[^>]*:title=/,
    '标题上不该再有原生 title（tooltip 已经接管）',
  );
});

test('卡片标题写「磁盘用量」，旧的「磁盘占用」不再出现在界面上', () => {
  assert.match(versionsPanel, /<h2>\s*磁盘用量\s*</, '标题必须是「磁盘用量」');
  // 模块名、字段名、注释里的「磁盘占用」是另一回事（模块仍叫 diskusage.rs），
  // 只有模板里给用户看的那一行受这条约束。
  const template = versionsPanel.slice(versionsPanel.indexOf('<template>'), versionsPanel.indexOf('</template>'));
  assert.doesNotMatch(template, /磁盘占用/, '模板里不该再出现「磁盘占用」');
});

test('类型色同时驱动圆点、占比条与容量胶囊三处', () => {
  // 三处必须共用同一个变量，否则会出现「圆点一种色、条另一种色」的分裂。
  const wired = versionsPanel.match(/var\(--usage-color/g) || [];
  assert.ok(
    wired.length >= 3,
    `--usage-color 应至少出现在圆点、占比条、容量胶囊三处，实际 ${wired.length} 处`,
  );
});