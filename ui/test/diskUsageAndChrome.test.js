import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

import { readShellSource } from '../../scripts/lib/shell-source.mjs';

// 两条会静默失效的约定，都得钉住：
//  ① 构建标识（dev 鲸眼红 / release Gitea 绿）由一枚 CSS 变量驱动，两种构建
//     共用同一条绘制规则，不会各画各的而漂掉；
//  ② 「磁盘用量」四个占用类型的配色按 **group.id** 取，不是按 label，也不是按顺序。

const theme = readFileSync('ui/src/theme.css', 'utf8');
const versionsPanel = readFileSync('ui/src/kernel/VersionsPanel.vue', 'utf8');
const diskusage = readShellSource('diskusage.rs');

// --- ① 构建标识细线 ---

// uiv2 改版把原来「从窗顶铺满全高的品牌色渐变带」收成窗顶 2px 一条线：
// 新设计的层级建立在「不透明表面 + 细边框 + 单一蓝色主色」上，整窗渐变会与
// 卡片描边互相拍频。但「一眼分清 dev / release」这件事必须留下——它是并排装两个
// 壳时唯一能区分的办法。做法是两种构建**共用同一条 `.app-shell::before` 规则**，
// 只换一个颜色变量，而不是各写一条背景渐变。

test('构建标识由 --build-color 驱动：两种构建共用同一条绘制规则', () => {
  const line = theme.match(/\.app-shell::before \{([^}]*)\}/);
  assert.ok(line, '必须能找到 .app-shell::before（构建标识细线）');
  assert.match(line[1], /background:\s*var\(--build-color\)/, '细线必须用 --build-color 取色');
  assert.match(line[1], /height:\s*2px/, '细线高度必须是 2px');

  // 画法只有这一处：一旦有人再加一条 body.* 的背景渐变，两个构建会各画各的。
  assert.doesNotMatch(
    theme,
    /body\.(?:dev-build|rel-build)\s*\{[^}]*background-image/s,
    '版本带已收成细线，body.dev-build / body.rel-build 不该再有背景渐变',
  );
});

test('dev 是鲸眼红、release 是 Gitea 绿（两个色值都不许改）', () => {
  // release 是 :root 上的默认值，dev 单独覆盖。
  const root = theme.match(/^:root \{([\s\S]*?)\n\}/m);
  assert.ok(root, '必须能找到 :root');
  assert.match(root[1], /--build-color:\s*#609926/, 'release 默认必须是 Gitea 绿 #609926');

  const dev = theme.match(/body\.dev-build \{([^}]*)\}/);
  assert.ok(dev, '必须能找到 body.dev-build');
  assert.match(dev[1], /--build-color:\s*#ff5a5e/, 'dev 必须是鲸眼红 #ff5a5e');
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
  // 标题左侧那枚 accent 方块（`.card-title-icon`）2026-10-10 铺到了全部六个
  // 面板，它必须被当成可选前缀吃掉而不是整条断言放宽——放宽之后就钉不住
  // 「方块紧挨标题、标题后面没有别的东西」了。
  assert.match(
    versionsPanel,
    /<h2[^>]*>\s*(?:<span class="card-title-icon"[^>]*>[\s\S]*?<\/span>\s*)*磁盘用量\s*</,
    '标题必须是「磁盘用量」',
  );
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

// --- ③ 「刷新」按钮必须真的重扫（用户 2026-10-07 报）---
//
// 症状：点「刷新」数字纹丝不动。根因在 Rust 侧——`disk_usage` 压根没有
// `force` 参数，缓存新鲜时直接 `return Ok(cached)`，连后台线程都不起。
// 而缓存一天才刷一次，于是「点按钮」几乎永远落在这条分支上：按钮是个空动作，
// 界面上还完全看不出它没生效。这条断言钉住前后两端都要传 force。

test('「刷新」按钮传 force，而自动加载不传——两者语义相反，不能共用一次调用', () => {
  assert.match(
    versionsPanel,
    /@click="loadDiskUsage\(true\)"/,
    '刷新按钮必须传 force=true，否则拿到的是缓存（这正是用户报的 bug）',
  );
  // 自动加载仍走两段式：每次进面板都重扫 2.5 万个文件没道理。
  // 连着 refreshAll 一起匹配，免得同一个文件里另一个 onMounted 被误配。
  assert.match(
    versionsPanel,
    /onMounted\(\(\) => \{\s*refreshAll\(\);\s*loadDiskUsage\(\);/,
    'onMounted 的自动加载必须是无参调用（默认不强制）',
  );
});

test('后端把 force 排在新鲜度之前，且命令接受这个参数', () => {
  assert.match(
    diskusage,
    /pub async fn disk_usage\(\s*app: tauri::AppHandle,\s*force: Option<bool>,?\s*\)/,
    'disk_usage 必须接受 force: Option<bool>（与 usage::get_model_usage 同一形状）',
  );
  // 判据要落在「force 提前 return Scan」这一步。只断言函数签名的话，
  // 参数收下了却被缓存分支忽略掉，照样是空按钮。
  assert.match(
    diskusage,
    /fn plan\([^)]*force: bool[^)]*\)\s*->\s*Plan\s*\{\s*if force \{\s*return Plan::Scan;/s,
    'plan() 里 force 必须先于新鲜度判定返回 Scan',
  );
  // 返回体带上 backgroundRefresh，否则前端那个「后台正在重新扫描…」转圈
  // 永远没有置 true 的时机（它此前就是个死标志）。
  assert.match(
    diskusage,
    /pub struct DiskUsageReply\s*\{[^}]*pub background_refresh: bool,/s,
    'disk_usage 必须回传 background_refresh，让前端的转圈标志真的有信号可依',
  );
});

test('前端拆开返回的 report 与 backgroundRefresh，并据此点亮转圈标志', () => {
  assert.match(
    versionsPanel,
    /applyDiskReport\(reply\.report\)/,
    '必须从 { report, backgroundRefresh } 里取出 report 再套用',
  );
  // refreshing 曾经是个**死标志**：只有收到事件时被置 false，没有任何地方
  // 置 true。后台重扫期间数字停在旧值上、界面上零反馈，那是最需要反馈的时刻。
  assert.match(
    versionsPanel,
    /diskUsage\.refreshing\s*=\s*Boolean\(reply\.backgroundRefresh\)/,
    'refreshing 必须真的被置为 true——否则「后台正在重新扫描…」的转圈永远不出现',
  );
});