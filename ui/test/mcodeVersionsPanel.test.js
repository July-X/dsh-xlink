import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';

import { createRequire } from 'node:module';
const require = createRequire(import.meta.url);
const { parse } = require(require.resolve('@vue/compiler-sfc', { paths: [require.resolve('vue')] }));

const panel = readFileSync(new URL('../src/kernel/VersionsPanel.vue', import.meta.url), 'utf8');

/// 扫模板不扫全文：本仓模板带大段解释「为什么这么写」的中文注释，注释里经常
/// 原样引用被判据的词，按整份文件 indexOf 会把自己写的解释当成命中。
function templateOf(src) {
  const start = src.indexOf('<template>');
  assert.ok(start >= 0, '找不到 <template>');
  const styleAt = src.indexOf('<style', start);
  const end = styleAt > start ? styleAt : src.length;
  return src.slice(start, end).replace(/<!--[\s\S]*?-->/g, '');
}

const tpl = templateOf(panel);

test('MiniMax 已安装列表读失败时，重试入口在行外', () => {
  const retry = tpl.indexOf('loadMcodeInstalledVersions()');
  const row = tpl.indexOf('v-for="item in mcode.installedVersions"');
  assert.ok(retry >= 0, '找不到重试入口');
  assert.ok(row >= 0, '找不到已安装列表');
  // 重试按钮写在行内就永远渲染不出来：读失败时列表被清空，行不渲染。
  assert.ok(retry < row, '重试按钮必须写在行外，否则列表一清空它就跟着消失');
  assert.match(tpl, />重试</, '重试入口要有文字，不是一个没有名字的图标按钮');
});

test('读失败时不说「尚未安装」：错误文案与空态互斥', () => {
  const error = tpl.indexOf('v-if="mcode.installedVersionsError"');
  const empty = tpl.search(/v-else-if="[^"\n]*mcode\.installedVersions\.length === 0"/);
  assert.ok(error >= 0, '读失败时要有原因');
  assert.ok(empty > error, '空态必须让位给错误文案：不是没装，是读不出来');
});


test('读取中有加载反馈，不把尚未返回的列表当作未安装', () => {
  assert.match(tpl, /v-loading="isLoading\('mcodeInstalledVersions'\)"/);
  assert.match(tpl, /v-else-if="!isLoading\('mcodeInstalledVersions'\) && mcode\.installedVersions\.length === 0"/);
  assert.match(tpl, /:loading="isLoading\('mcodeInstalledVersions'\)"[^>]*@click="loadMcodeInstalledVersions\(\)"/);
});

// 用 Vue AST 验证真实嵌套关系，不能只比文本先后顺序。
function elements(node) {
  return [node, ...(node.children || []).flatMap(elements)];
}
function hasClass(node, name) {
  return node.props?.some(prop => prop.name === 'class' && prop.value?.content.split(' ').includes(name));
}
function assertInstallScope(source) {
  const nodes = elements(parse(source).descriptor.template.ast);
  const scope = nodes.find(node => hasClass(node, 'kernel-installation'));
  assert.ok(scope, '必须有安装区');
  const children = elements(scope);
  assert.equal(children.filter(node => node.tag === 'FamilySwitch').length, 1, '安装区持有唯一切换器');
  assert.ok(children.some(node => hasClass(node, 'updates-lists')), '两个版本列表属于安装区');
  assert.ok(!children.some(node => hasClass(node, 'disk-usage')), '磁盘统计必须在安装区外');
}

test('内核切换只包住安装区，磁盘统计不参与切换', () => {
  assertInstallScope(panel);
  const moved = panel.replace(/<FamilySwitch[^>]*\/>/, '').replace('<div class="page-head">', '<div class="page-head"><FamilySwitch />');
  assert.throws(() => assertInstallScope(moved), /安装区持有唯一切换器/);
});

test('损坏的 MiniMax 安装仍提供安装入口', async () => {
  const { compile, createSSRApp, h } = require('vue');
  const { renderToString } = require(require.resolve('@vue/server-renderer', { paths: [require.resolve('vue')] }));
  const row = elements(parse(panel).descriptor.template.ast).find(node =>
    hasClass(node, 'release-row') && node.loc.source.includes('mcode.availableVersions'));
  assert.ok(row);
  const render = async supported => {
    const app = createSSRApp({
      data: () => ({
        mcode: { installedVersions: [{ version: '0.1.7', supported }], availableVersions: [{ version: '0.1.7', installable: true }], view: {} },
        globalBusy: false, Download: null,
      }),
      methods: { installMcode() {} },
      render: compile(row.loc.source),
    });
    app.component('el-tag', { render() { return h('span', this.$slots.default?.()); } });
    app.component('el-button', { render() { return h('button', this.$slots.default?.()); } });
    return renderToString(app);
  };
  assert.match(await render(true), />已安装</);
  assert.match(await render(false), />安装</);
});

test('MiniMax 官方列表沿用逐行安装布局，保留固定版本与完整性门槛', () => {
  assert.match(tpl, /@click="loadMcodeAvailableVersions\(\)"/);
  assert.match(tpl, /v-else-if="item.installable && item.version === '0.1.7'"/);
  assert.match(tpl, /@click="installMcode\(\)"/);
  assert.match(tpl, /globalBusy \|\| !!mcode.view\?\.stop_required/);
  assert.doesNotMatch(tpl, /安装 MiniMax 0.1.7|查看 npm 版本|手动设置安装位置/);
});

/** 打开这一页 / 切到 MiniMax 时必须自动拉发布列表，不能等人先点刷新。 */
function assertAutoLoad(source) {
  const watch = /watch\(\s*isMcode,\s*\(selected\) => \{[\s\S]*?\}, \{ immediate: true \}\)/.exec(source);
  assert.ok(watch, '找不到内核切换监听');
  assert.match(watch[0], /loadMcodeAvailableVersions\(\)/, '打开页面 / 切到 MiniMax 时自动拉取 npm 版本列表');
}

test('打开页面自动拉取官方发布列表，不要求先点刷新', () => {
  assertAutoLoad(panel);
  const watch = /watch\(\s*isMcode,\s*\(selected\) => \{[\s\S]*?\}, \{ immediate: true \}\)/.exec(panel)[0];
  const manual = panel.replace(watch, watch.replace(/\n\s*loadMcodeAvailableVersions\(\);/, ''));
  assert.notEqual(manual, panel, '反向验证必须改到真实监听');
  assert.throws(() => assertAutoLoad(manual), /自动拉取 npm 版本列表/);
});

/** 与 DSH 不一致的运行环境 / 手填路径入口（2026-11-08 用户要求移除）。 */
function assertNoEnvBlock(source) {
  // 注释里会解释「为什么删掉这个块」并原样写出它原来的名字——扫全文会把
  // 维护者写的说明当成残留命中（本仓「扫模板不扫全文」那条纪律）。
  const code = source
    .replace(/<!--[\s\S]*?-->/g, '')
    .replace(/\/\*[\s\S]*?\*\//g, '')
    .replace(/^[ \t]*\/\/.*$/gm, '');
  for (const token of ['运行环境与已有安装', 'mcode-paths', 'mcode-env-line', 'saveMcodePaths']) {
    assert.ok(!code.includes(token), `安装区不该再有 ${token}：与 DSH 不一致的额外入口`);
  }
}

test('不再有「运行环境与已有安装」折叠块，运行环境由壳管理', () => {
  assertNoEnvBlock(panel);
  assert.ok(!panel.includes('detectMcode('), '重新检测运行环境的入口随折叠块一起删除');
  // 「选用」是逐个已装目录的动作（与 DSH 的切换同形），保留。
  assert.match(tpl, /useMcodeInstallation/);
  const restored = panel.replace(
    '<div v-if="isMcode" class="list-group">',
    '<div v-if="isMcode" class="list-group"><details class="mcode-paths"><summary>运行环境与已有安装</summary></details>',
  );
  assert.notEqual(restored, panel, '反向验证必须改到真实安装区');
  assert.throws(() => assertNoEnvBlock(restored), /运行环境与已有安装/);
});
