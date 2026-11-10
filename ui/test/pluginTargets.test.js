import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { minimaxPluginTarget } from '../src/plugins/targetState.js';
const ready = { view: { instance_id: 'mcode-a', ponytail_enabled: false }, loading: false };
test('MiniMax 开关只消费自己的状态，不从 DSH 安装状态推断', () => {
  assert.equal(minimaxPluginTarget('ponytail', ready, true).enabled, false);
  assert.equal(minimaxPluginTarget('ponytail', { ...ready, view: { ...ready.view, ponytail_enabled: true } }).enabled, true);
  assert.equal(minimaxPluginTarget('ponytail', ready, true).disabled, false);
});
test('未适配的插件、未知和失败状态不能触发安装', () => {
  for (const id of ['official-chat', 'openai-oauth', 'community:ponytail']) assert.equal(minimaxPluginTarget(id, ready).disabled, true);
  for (const state of [{}, { loading: true }, { view: {} }, { ...ready, error: '断开' }, { ...ready, catalogError: '目录失败' }, { view: { ...ready.view, stop_required: true } }, { view: { ...ready.view, ponytail_error: '状态异常' } }]) assert.equal(minimaxPluginTarget('ponytail', state).disabled, true);
});
// 记忆的权限入口在记忆页；插件页这一行只把那份授权的现状带过来。已授权却
// 仍显示成「未授权 / 不可用」，用户会以为记忆页那次操作没生效。
test('记忆授权在记忆页：插件页反映现状，不把已授权显示成未授权', () => {
  const unknown = minimaxPluginTarget('memory', ready);
  assert.equal(unknown.disabled, true);
  assert.equal(unknown.text, '读取中');

  const granted = minimaxPluginTarget('memory', ready, false, true);
  assert.equal(granted.enabled, true, '已授权时勾选框应打勾');
  assert.equal(granted.disabled, true, '权限仍在记忆页改，本行不给第二个入口');
  assert.equal(granted.text, '已接入');
  assert.match(granted.tip, /记忆页/);

  const denied = minimaxPluginTarget('memory', ready, false, false);
  assert.equal(denied.enabled, false);
  assert.equal(denied.text, '未接入');
});

test('失败保留已知授权状态，但禁用开关，不伪装成未安装', () => {
  const target = minimaxPluginTarget('ponytail', { error: '读取失败', view: { ...ready.view, ponytail_enabled: true } });
  assert.equal(target.enabled, true);
  assert.equal(target.disabled, true);
  assert.equal(target.text, '状态不可用');
});
test('完整清单保留，目标同行呈现且社区卸载仍需确认', () => {
  const panel = readFileSync(new URL('../src/plugins/PluginsPanel.vue', import.meta.url), 'utf8').split('<template>')[1].split('<style')[0].replace(/<!--[\s\S]*?-->/g, '');
  assert.match(panel, /v-for="builtin in builtinList"/);
  assert.match(panel, /:plugin-id="builtin.id"/);
  assert.match(panel, /<el-checkbox[\s\S]*?:model-value="builtin.requestedEnabled"/);
  const target = readFileSync(new URL('../src/plugins/MinimaxPluginTarget.vue', import.meta.url), 'utf8');
  assert.match(target, /<el-checkbox/);
  assert.doesNotMatch(target, /<el-switch/);
  assert.match(panel, /:plugin-id="'community:' \+ row.id"/);
  assert.match(panel, /@confirm="uninstallPlugin\(row.id\)"/);
  assert.doesNotMatch(panel, /v-for="builtin in mcode.builtins"/);
});

test('更新按钮只更新上游 Ponytail，MiniMax 目标不再挂第二套更新动作', () => {
  const targetFile = readFileSync(new URL('../src/plugins/MinimaxPluginTarget.vue', import.meta.url), 'utf8');
  const target = targetFile.split('<template>')[1].split('</template>')[0].replace(/<!--[\s\S]*?-->/g, '');
  // 行内只需要上游版本旁那一枚「检查更新」。MiniMax 目标后面曾经还有一套
  // 「检查 MiniMax 更新 / 有更新 / 更新」；dsh-xlink 提供的内置插件应当由壳
  // 对两个内核一并负责，不该让用户按内核各更新一次。
  assert.doesNotMatch(target, /检查更新|检查 MiniMax 更新|>更新</);
  assert.doesNotMatch(targetFile, /checkMcodePonytailUpdate|upgradeMcodePonytail/);

  const panel = readFileSync(new URL('../src/plugins/PluginsPanel.vue', import.meta.url), 'utf8');
  // 同一次「检查更新」在 MiniMax 已启用且工作台停止时，把上游内容一并应用过去；
  // 运行中（running / stop_required）改不了实例里的插件文件，跳过等停止后再点。
  assert.match(panel, /@click="checkBuiltinUpstreamAll\(builtin\)"/);
  assert.doesNotMatch(panel, /@click="checkBuiltinUpstream\(builtin\.id\)"/);
  assert.match(panel, /await checkBuiltinUpstream\(builtin\.id\)/);
  assert.match(panel, /await checkMcodePonytailUpdate\(\)/);
  assert.match(panel, /await upgradeMcodePonytail\(\)/);
  assert.match(panel, /mcodeView\.ponytail_enabled/);
  assert.match(panel, /mcodeView\.running/);
  assert.match(panel, /mcodeView\.stop_required/);
});

test('安全占位残留时指向旧记录恢复入口，不要求重复停止', () => {
  const target = minimaxPluginTarget('ponytail', { view: { instance_id: 'test', recovery_snapshot: 'snapshot-a', stop_required: true } }, true);
  assert.equal(target.disabled, true);
  assert.equal(target.text, '需清理旧记录');
  assert.match(target.tip, /重启后清理记录/);
});
