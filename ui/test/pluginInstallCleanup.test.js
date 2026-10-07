// 手动安装与失败诊断的收尾路径。
//
// 这条链路容易出现两个相反的问题：手动输入绕过预检，或失败后只留下日志，
// 用户却找不到删除已下载插件的入口。测试钉住入口分流、动作代理和危险动作
// 的显示条件；真正删除文件仍由 Rust 的 plugin_uninstall 做安全校验。
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const read = (path) => readFileSync(path, 'utf8');

test('手动安装跟随预检开关，不再始终直装', () => {
  const panel = read('ui/src/plugins/PluginsPanel.vue');
  assert.match(
    panel,
    /@keyup\.enter="precheckOn \? precheckPlugin\(''\) : installPlugin\(''\)"/,
    '手动输入回车必须和插件仓库按钮使用同一份预检开关',
  );
});

test('失败诊断显示带确认的本地插件清理动作', () => {
  for (const file of ['ui/src/plugins/PrecheckDialog.vue', 'ui/src/diagnostics/PluginDiagnosis.vue']) {
    const source = read(file);
    assert.match(
      source,
      /report(?:\.value)?(?:\?\.|\.)pluginId[\s\S]{0,140}installed === true[\s\S]{0,140}verdict !== 'pass'/,
      `${file} 只能对报告确认已安装且诊断未通过的插件显示清理动作`,
    );
    assert.match(source, /PluginCleanupButton/);
  }
  const button = read('ui/src/diagnostics/PluginCleanupButton.vue');
  assert.match(button, /removeDiagnosedPlugin/);
  assert.match(button, /<el-popconfirm/);
  assert.match(button, /移除并清理/);
  assert.match(button, /diagnosticPluginCleanup/);
});

test('清理动作复用完整卸载链路并明确反馈本地文件', () => {
  const actions = read('ui/src/diagnostics/diagnostic-actions.js');
  assert.match(actions, /uninstallPlugin\(id, \{ cleanup: true \}\)/);
  assert.match(actions, /withLoading\('diagnosticPluginCleanup'/);

  const plugins = read('ui/src/plugins/plugins.js');
  assert.match(plugins, /cmd: 'plugin_uninstall'/);
  assert.match(plugins, /插件及其本地文件已清理/);

  const incident = read('ui/src/incidents/IncidentModal.vue');
  assert.match(incident, /确认移除并清理该插件/);
  assert.match(incident, /会删除本地插件文件和接线/);
});
