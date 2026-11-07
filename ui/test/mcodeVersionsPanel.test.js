import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';

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
