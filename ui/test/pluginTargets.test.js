import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { minimaxPluginTarget } from '../src/plugins/targetState.js';
const ready = { view: { instance_id: 'mcode-a', ponytail_enabled: false }, loading: false };
// 两个新接入的效果真值在后端：openai-oauth = MiniMax config.yaml 的
// beta.codexOAuth；official-chat = 实例里的插件目录。
const openaiView = { instance_id: 'mcode-a', openai_oauth_supported: true };
const chatView = { instance_id: 'mcode-a', official_chat_enabled: false, official_chat_supported: true };
test('MiniMax 开关只消费自己的状态，不从 DSH 安装状态推断', () => {
  assert.equal(minimaxPluginTarget('ponytail', ready, true).enabled, false);
  assert.equal(minimaxPluginTarget('ponytail', { ...ready, view: { ...ready.view, ponytail_enabled: true } }).enabled, true);
  assert.equal(minimaxPluginTarget('ponytail', ready, true).disabled, false);
});
test('未适配的插件、未知和失败状态不能触发安装', () => {
  for (const id of ['community:ponytail', 'shell-navigation']) assert.equal(minimaxPluginTarget(id, ready).disabled, true);
  for (const state of [{}, { loading: true }, { view: {} }, { ...ready, error: '断开' }, { ...ready, catalogError: '目录失败' }, { view: { ...ready.view, stop_required: true } }, { view: { ...ready.view, ponytail_error: '状态异常' } }]) assert.equal(minimaxPluginTarget('ponytail', state).disabled, true);
});
test('OpenAI 登录是 MiniMax 自带能力：只读显示，旧内核说清需要升级', () => {
  // 0.1.7 起 MiniMax 自带 Codex 登录、默认可用：壳没有开关可拨，也不替用户登录。
  const builtin = minimaxPluginTarget('openai-oauth', { ...ready, view: openaiView }, true);
  assert.equal(builtin.enabled, true);
  assert.equal(builtin.disabled, true, '没有壳可拨的开关');
  assert.equal(builtin.text, '已内置');
  assert.match(builtin.tip, /MiniMax 工作台/);
  assert.match(builtin.tip, /相互独立/);

  const oldKernel = minimaxPluginTarget('openai-oauth', { ...ready, view: { ...openaiView, openai_oauth_supported: false } }, true);
  assert.equal(oldKernel.disabled, true);
  assert.equal(oldKernel.text, '需升级内核');
  assert.match(oldKernel.tip, /0\.1\.7|升级/);
});
test('官网对话引用接入同一套阻塞态，并如实说明没有 @ 菜单', () => {
  const off = minimaxPluginTarget('official-chat', { ...ready, view: chatView }, true);
  assert.equal(off.disabled, false);
  assert.equal(off.text, '未接入');
  assert.match(off.tip, /@ 菜单/);
  assert.match(off.tip, /不完整/);

  const on = minimaxPluginTarget('official-chat', { ...ready, view: { ...chatView, official_chat_enabled: true } }, true);
  assert.equal(on.enabled, true);

  for (const state of [
    { ...ready, view: { ...chatView, stop_required: true } },
    { ...ready, view: { ...chatView, official_chat_error: '插件损坏' } },
    { view: chatView, error: '断开' },
  ]) assert.equal(minimaxPluginTarget('official-chat', state, true).disabled, true);

  const unsupported = minimaxPluginTarget('official-chat', { ...ready, view: { ...chatView, official_chat_supported: false } }, true);
  assert.equal(unsupported.disabled, true);
  assert.equal(unsupported.text, '需升级内核');
});
test('官网引用可恢复状态可操作，读取错误仍不可操作', () => {
  for (const enabled of [true, false, null]) {
    const view = { ...chatView, official_chat_enabled: enabled, official_chat_error: '接线不一致', official_chat_recoverable: true };
    const target = minimaxPluginTarget('official-chat', { ...ready, view }, true);
    assert.equal(target.disabled, false);
    assert.equal(target.text, enabled === false ? '需清理' : '需修复');
    assert.equal(!!target.retryDisable, enabled === false);
    assert.equal(target.enabled, !!enabled);
    assert.equal(minimaxPluginTarget('official-chat', { ...ready, view: { ...view, stop_required: true } }, true).disabled, true);
  }
  const unreadable = { ...chatView, official_chat_error: '读取失败', official_chat_recoverable: false };
  assert.equal(minimaxPluginTarget('official-chat', { ...ready, view: unreadable }, true).disabled, true);
  const oldEnabled = { ...chatView, official_chat_enabled: true, official_chat_supported: false };
  assert.equal(minimaxPluginTarget('official-chat', { ...ready, view: oldEnabled }, true).disabled, false);
  const oldResidual = { ...chatView, official_chat_supported: false, official_chat_error: '残留', official_chat_recoverable: true };
  assert.equal(minimaxPluginTarget('official-chat', { ...ready, view: oldResidual }, true).retryDisable, true);
  const component = readFileSync(new URL('../src/plugins/MinimaxPluginTarget.vue', import.meta.url), 'utf8');
  assert.match(component, /v-if="target.retryDisable"[\s\S]*?@click="change\(false\)"/);
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
  // ponytail 走自己那条命令；其余内置插件走泛化目标命令——别让新插件复用手
  // 握树事务那条命令。
  assert.match(target, /setMcodePluginTarget/);
  assert.match(target, /setMcodePonytail/);
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
