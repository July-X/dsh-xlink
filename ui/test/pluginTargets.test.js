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
test('未适配的插件、记忆授权、未知和失败状态不能触发安装', () => {
  for (const id of ['memory', 'official-chat', 'openai-oauth', 'community:ponytail']) assert.equal(minimaxPluginTarget(id, ready).disabled, true);
  for (const state of [{}, { loading: true }, { view: {} }, { ...ready, error: '断开' }, { ...ready, catalogError: '目录失败' }, { view: { ...ready.view, stop_required: true } }, { view: { ...ready.view, ponytail_error: '状态异常' } }]) assert.equal(minimaxPluginTarget('ponytail', state).disabled, true);
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
