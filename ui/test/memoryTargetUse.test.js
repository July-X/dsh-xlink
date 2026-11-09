// 记忆页的「接收方开关」（T08）：两个目标各一个开关，状态词与授权分开。
//
// 这一组盯三件真实会坏且不报错的事：
// ① 命令接线——纯函数对了、模板没接（本仓反复栽的形状）；
// ② 状态词——preference=null 时按 effective 猜成开/关，等于替用户做了没做过的选择；
// ③ 目标开关与模块授权互相代改——关一个把另一个也关了，或者目标开关写进授权命令。
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

let handler;
const calls = [];
globalThis.window = { __TAURI__: { core: { invoke: async (command, args) => {
  calls.push([command, args]);
  return handler(command, args);
} } } };

const share = await import('../src/memory/share.js');
const moduleApi = await import('../src/memory/module.js');
const { mcode } = await import('../src/kernel/mcode.js');

const read = (path) => readFileSync(new URL(path, import.meta.url), 'utf8');
const view = (target, preference, effective, follows_legacy, warning = null) =>
  ({ target, preference, effective, follows_legacy, warning });
const views = (overrides = {}) => [
  view('dsh', null, true, true),
  view('mcode', null, false, false),
].map((item) => ({ ...item, ...(overrides[item.target] || {}) }));

function setup(overrides) {
  calls.length = 0;
  share.resetMemoryTargetUse();
  moduleApi.resetMemoryModule();
  mcode.view = { instance_id: 'mcode-a', memory_profile: 'web' };
  handler = (command) => {
    if (command === 'memory_target_use_status' || command === 'memory_set_target_use') return views(overrides);
    throw new Error(`意外命令 ${command}`);
  };
}

/** `.vue` 的模板块；HTML 注释已剥（注释里会原样引用被判的词）。 */
function templateOf(source) {
  const start = source.indexOf('<template>');
  assert.ok(start >= 0, '找不到 <template>');
  const styleAt = source.indexOf('<style', start);
  return source.slice(start, styleAt > start ? styleAt : source.length).replace(/<!--[\s\S]*?-->/g, '');
}

test('状态词只读后端事实：已开启 / 已关闭 / 跟随原设置 / 状态未知', () => {
  assert.equal(share.targetUseText(null), '读取中');
  assert.equal(share.targetUseText(view('dsh', true, true, false)), '已开启');
  assert.equal(share.targetUseText(view('dsh', false, false, false)), '已关闭');
  // 没选过时不猜成开或关：effective 只是旧设置现在的效果。
  assert.equal(share.targetUseText(view('dsh', null, true, true)), '跟随原设置');
  assert.equal(share.targetUseText(view('mcode', null, false, false)), '已关闭');
  // 状态读不出来时不能画成开或关。
  assert.equal(share.targetUseText(view('dsh', true, true, false, '开关文件损坏')), '状态未知');
  assert.equal(share.targetUseText(view('dsh', null, true, true, '开关文件损坏')), '状态未知');
});

test('读写都走目标开关命令：读刷新回填，写原样带回 target / enabled 并回填返回值', async () => {
  setup({ dsh: { preference: true, effective: true, follows_legacy: false } });
  await share.refreshMemoryTargetUse();
  assert.deepEqual(calls[0], ['memory_target_use_status', {}], '读的是 memory_target_use_status');
  assert.equal(share.targetUseView('dsh').preference, true);
  assert.equal(share.targetUseView('mcode').effective, false);

  calls.length = 0;
  handler = (command) => {
    assert.equal(command, 'memory_set_target_use');
    return views({ dsh: { preference: false, effective: false, follows_legacy: false } });
  };
  assert.equal(await share.setMemoryTargetUse('dsh', false), true);
  assert.deepEqual(calls[0], ['memory_set_target_use', { target: 'dsh', enabled: false }]);
  const dsh = share.targetUseView('dsh');
  assert.equal(dsh.preference, false, '后端返回的视图要回填');
  assert.equal(dsh.effective, false);
  assert.equal(share.targetUseText(dsh), '已关闭');
});

test('写入失败恢复原值并给出下一步，开关不停在用户点的那一侧', async () => {
  setup();
  await share.refreshMemoryTargetUse();
  calls.length = 0;
  handler = () => Promise.reject(new Error('记忆使用开关不可用：无法保存'));
  assert.equal(await share.setMemoryTargetUse('dsh', false), false);
  const dsh = share.targetUseView('dsh');
  assert.equal(dsh.preference, null, '失败后回到原值（没选过）');
  assert.equal(dsh.follows_legacy, true, '跟随原设置的语义不丢');
  assert.equal(dsh.effective, true);
  assert.equal(share.memoryTargetUse.changing, '');
  assert.match(share.memoryTargetUse.error, /状态需要重新读取/);
  assert.match(share.memoryTargetUse.error, /重试/, '失败提示要有可执行的下一步');
  handler = () => views();
  await share.refreshMemoryTargetUse();
  assert.equal(share.memoryTargetUse.error, '', '刷新成功后恢复可用');
});

test('写入在途时禁用并拒绝第二次提交', async () => {
  setup();
  await share.refreshMemoryTargetUse();
  calls.length = 0;
  let release;
  handler = () => new Promise((resolve) => { release = resolve; });
  const pending = share.setMemoryTargetUse('mcode', true);
  assert.equal(share.memoryTargetUse.changing, 'mcode', '在途目标要能被模板读出来禁用开关');
  assert.equal(await share.setMemoryTargetUse('dsh', false), false, '在途时不再发第二次写入');
  assert.equal(calls.filter(([command]) => command === 'memory_set_target_use').length, 1);
  release(views());
  assert.equal(await pending, true);
  assert.equal(share.memoryTargetUse.changing, '');
});

test('目标开关与模块授权各写各的命令，互不改对方状态', async () => {
  share.resetMemoryTargetUse();
  moduleApi.resetMemoryModule();
  mcode.view = { instance_id: 'mcode-a', memory_profile: 'web' };
  const current = views({ dsh: { preference: true, effective: true, follows_legacy: false } });
  handler = (command) => {
    if (command === 'memory_target_use_status') return current;
    if (command === 'memory_set_target_use') return current;
    if (command === 'memory_module_status') {
      return { enabled: true, pair_key: 'pair-a', sources: [{ label: 'DSH', namespace: 'dsh:a' }], warning: null };
    }
    if (command === 'memory_module_list') return [];
    throw new Error(`意外命令 ${command}`);
  };
  await share.refreshMemoryTargetUse();
  calls.length = 0;
  await share.setMemoryTargetUse('mcode', false);
  assert.deepEqual(calls.map(([command]) => command), ['memory_set_target_use'], '目标开关只写自己的命令');
  assert.equal(moduleApi.memoryModule.status, null, '目标开关不改模块授权状态');

  await moduleApi.refreshMemoryModule();
  calls.length = 0;
  const before = share.memoryTargetUse.views;
  await moduleApi.requestMemoryModuleEnabled(false);
  assert.ok(calls.every(([command]) => command.startsWith('memory_module_')), '模块授权只写模块命令');
  assert.deepEqual(share.memoryTargetUse.views, before, '模块授权不改目标开关');
});

test('模板：DSH 恒显、MiniMax 目标跟体验开关，两个开关并排且绑各自的命令', () => {
  const source = read('../src/memory/MemoryModulePanel.vue');
  assert.match(source, /key: 'dsh'/); assert.match(source, /mcodeExperience.value/); assert.match(source, /key: 'mcode'/);
  assert.match(source, /v-for="target in targets"/);
  assert.match(source, /requestPoolTarget\(target.key, enabled\)/);
  assert.match(source, /poolTarget\(target.key\)\?\.effective/);
  assert.match(source, /未配置实例/);
  assert.doesNotMatch(source, /原有实例接入|双向普通记忆互通/);
});

test('体验隐藏时保留历史来源与授权管理，但不暴露 MiniMax 修改入口', () => {
  const source = read('../src/memory/MemoryPanel.vue');
  assert.match(source, /mcodeExperience/);
  assert.match(source, /:readonly="!mcodeExperience"/);
  assert.match(source, /revokeAnyMemoryShare\(item\)/);
  assert.match(source, /memoryModule.records.filter\(item => item.origin_label === 'MiniMax'\)/);
  assert.match(read('../src/memory/MemoryModulePanel.vue'), /mcodeExperience.value/);
});

test('目标开关在挂载与实例变化时重读，卸载时清空，不把旧实例的状态画到新实例上', () => {
  const source = read('../src/memory/MemoryPanel.vue');
  const header = read('../src/memory/MemoryModulePanel.vue');
  assert.match(header, /onMounted\(refreshMemoryPool\)/);
  assert.match(header, /onUnmounted\(resetMemoryPool\)/);
  assert.match(source, /if \(previous\) \{ refreshMemoryModule\(\); refreshMemoryPool\(\); resetPoolReview\(\); refreshPoolReview\(\); \}/);
  assert.match(source, /resetPoolReview\(\)/);
});
