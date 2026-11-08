import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readFileSync } from 'node:fs';

let handler;
const calls = [];
globalThis.window = { __TAURI__: { core: { invoke: async (command, args) => {
  calls.push([command, args]);
  return handler(command, args);
} } } };
const {
  memoryModule, moduleDetail, moduleRecordKey, refreshMemoryModule, resetMemoryModule,
  requestMemoryModuleEnabled, confirmMemoryModuleEnable, cancelMemoryModuleEnable,
  openModuleDetail,
} = await import('../src/memory/module.js');
const sources = [{ label: 'DSH', namespace: 'dsh:a' }, { label: 'MiniMax', namespace: 'mcode:b' }];
const status = (enabled = false, pair_key = 'pair-a') => ({ enabled, pair_key, sources, warning: null });
const records = sources.map(source => ({ origin_label: source.label, source_namespace: source.namespace, record: { id: 'same-id', title: source.label } }));
function setup() {
  resetMemoryModule();
  calls.length = 0;
  handler = command => command === 'memory_module_list' ? records : status();
}

test('统一列表保留相同 ID 的不同来源，详情按来源只读定位', async () => {
  setup();
  await refreshMemoryModule();
  assert.equal(memoryModule.records.length, 2);
  assert.notEqual(moduleRecordKey(records[0]), moduleRecordKey(records[1]));
  openModuleDetail(records[1]);
  assert.equal(moduleDetail.value.origin_label, 'MiniMax');
});

test('启用先确认，取消不写权限，确认只提交所示来源配对', async () => {
  setup();
  await refreshMemoryModule();
  requestMemoryModuleEnabled(true);
  assert.equal(calls.some(([name]) => name === 'memory_module_set_enabled'), false);
  cancelMemoryModuleEnable();
  await confirmMemoryModuleEnable();
  assert.equal(calls.some(([name]) => name === 'memory_module_set_enabled'), false);
  requestMemoryModuleEnabled(true);
  memoryModule.status = status(false, 'pair-b');
  await confirmMemoryModuleEnable();
  assert.deepEqual(calls.find(([name]) => name === 'memory_module_set_enabled')[1], { enabled: true, expectedPair: 'pair-a' });
  assert.ok(calls.every(([name]) => name.startsWith('memory_module_')));
});

test('停用只撤销模块权限，不触碰旧实例或逐条授权', async () => {
  setup();
  memoryModule.status = status(true);
  await requestMemoryModuleEnabled(false);
  assert.equal(memoryModule.confirmation, null);
  assert.deepEqual(calls[0], ['memory_module_set_enabled', { enabled: false, expectedPair: 'pair-a' }]);
  assert.ok(calls.every(([name]) => name.startsWith('memory_module_')));
});

test('空配对阻止启用，但已启用模块仍允许撤销', async () => {
  setup();
  memoryModule.status = { ...status(false, ''), warning: '来源缺失' };
  requestMemoryModuleEnabled(true);
  assert.equal(memoryModule.confirmation, null);
  assert.equal(calls.length, 0);
  memoryModule.status.enabled = true;
  await requestMemoryModuleEnabled(false);
  assert.equal(calls[0][1].enabled, false);
});

test('刷新中来源配对改变时拒绝显示混合快照', async () => {
  setup();
  let count = 0;
  handler = command => command === 'memory_module_list' ? records : status(false, ++count === 1 ? 'pair-a' : 'pair-b');
  await refreshMemoryModule();
  assert.deepEqual(memoryModule.records, []);
  assert.equal(memoryModule.status, null);
  assert.match(memoryModule.error, /来源已改变/);
});

test('离开页面后迟到的刷新或写入不能恢复旧状态', async () => {
  for (const mutation of [false, true]) {
    setup();
    let release;
    handler = () => new Promise(resolve => { release = resolve; });
    memoryModule.status = status(true);
    const pending = mutation ? requestMemoryModuleEnabled(false) : refreshMemoryModule();
    resetMemoryModule();
    release(status(true));
    await pending;
    assert.equal(memoryModule.status, null);
    assert.deepEqual(memoryModule.records, []);
    assert.equal(memoryModule.busy, false);
  }
});

test('读取失败清空快照并提示刷新；重复动作不重入', async () => {
  setup();
  let reject;
  handler = () => new Promise((_, fail) => { reject = fail; });
  const pending = refreshMemoryModule();
  await refreshMemoryModule();
  assert.equal(calls.length, 1);
  reject(new Error('测试读取失败'));
  await pending;
  assert.equal(memoryModule.status, null);
  assert.match(memoryModule.error, /请刷新/);
});

test('面板在旧视图之上挂载，统一详情无修改事件且标明来源', () => {
  const source = name => readFileSync(new URL(`../src/memory/${name}.vue`, import.meta.url), 'utf8');
  const template = text => text.slice(text.indexOf('<template>'), text.indexOf('<style')).replace(/<!--[\s\S]*?-->/g, '');
  const panel = template(source('MemoryPanel'));
  assert.ok(panel.indexOf('<MemoryModulePanel>') < panel.indexOf('<template v-if="isMcode">'));
  const modulePanel = template(source('MemoryModulePanel'));
  const detail = modulePanel.match(/<MemoryDetail\b[^>]*\/>/)[0];
  assert.match(detail, /\breadonly\b/);
  assert.match(detail, /:source-label=/);
  assert.doesNotMatch(detail, /@(edit|remove|toggle|share)=/);
  assert.match(modulePanel, /启用后重启 MiniMax 工作台/);
});


test('来源改变警告仍可确认当前有效配对，不静默授权新来源', async () => {
  setup();
  memoryModule.status = { ...status(false, 'pair-new'), warning: '实例或 profile 已改变，请重新授权当前记忆模块' };
  requestMemoryModuleEnabled(true);
  assert.equal(memoryModule.confirmation.pair_key, 'pair-new');
  assert.equal(calls.length, 0);
  await confirmMemoryModuleEnable();
  assert.deepEqual(calls[0], ['memory_module_set_enabled', { enabled: true, expectedPair: 'pair-new' }]);
});

test('坏授权保留后端警告状态，不继续读取列表', async () => {
  setup();
  const broken = { ...status(false, ''), warning: '授权不可读，请撤销后重新授权' };
  handler = command => {
    assert.equal(command, 'memory_module_status');
    return broken;
  };
  await refreshMemoryModule();
  assert.deepEqual(memoryModule.status, broken);
  assert.equal(memoryModule.error, '');
  assert.deepEqual(memoryModule.records, []);
  assert.equal(calls.length, 1);
});

test('未知或坏状态可明确撤销模块且清除待确认启用，不修改旧权限', async () => {
  for (const broken of [null, { ...status(false, ''), warning: '授权损坏' }]) {
    setup();
    memoryModule.status = status();
    requestMemoryModuleEnabled(true);
    memoryModule.status = broken;
    await requestMemoryModuleEnabled(false);
    assert.deepEqual(calls[0], ['memory_module_set_enabled', { enabled: false, expectedPair: '' }]);
    assert.equal(memoryModule.confirmation, null);
    assert.ok(calls.every(([name]) => name.startsWith('memory_module_')));
    assert.equal(calls.filter(([name]) => name === 'memory_module_set_enabled').length, 1);
  }
});

test('警告与错误态提供独立撤销入口，开关不按警告禁止重新授权', () => {
  const source = readFileSync(new URL('../src/memory/MemoryModulePanel.vue', import.meta.url), 'utf8');
  const template = source.slice(source.indexOf('<template>'), source.indexOf('<style')).replace(/<!--[\s\S]*?-->/g, '');
  const control = template.match(/<el-switch\b[\s\S]*?\/>/)[0];
  assert.match(control, /!memoryModule.status.pair_key/);
  assert.doesNotMatch(control, /status.warning/);
  assert.match(template, /v-if="memoryModule.error \|\| memoryModule.status\?\.warning"/);
  assert.match(template, /@click="requestMemoryModuleEnabled\(false\)">撤销模块授权<\/el-button>/);
});
