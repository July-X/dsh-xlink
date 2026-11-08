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
  assert.match(share.memoryTargetUse.error, /已恢复原值/);
  assert.match(share.memoryTargetUse.error, /重试/, '失败提示要有可执行的下一步');
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
  const source = read('../src/memory/MemoryPanel.vue');
  const template = templateOf(source);
  // 构建旗只允许 experience.js 读；这一页的入口可见性只有 mcodeExperience 一个判据。
  assert.match(source, /import \{ MCODE_BUILD_AVAILABLE, mcodeExperience \} from '\.\.\/kernel\/experience\.js';/);
  assert.match(source, /const isMcode = computed\(\(\) => mcodeExperience\.value && family\.value === 'mcode'\)/);
  assert.doesNotMatch(source, /VITE_MCODE_EXPERIMENTAL/, '页面自己读构建旗会让保存的体验偏好失效');

  const start = template.indexOf('<MemoryModulePanel>');
  const end = template.indexOf('<template v-if="isMcode">');
  assert.ok(start > 0 && end > start, '记忆页顶部缺少「允许使用记忆」区域');
  const permission = template.slice(start, end);
  // 精简（2026-11-08 用户要求）：并入通用记忆模块卡，不再单独占一条带边框的横带，
  // 也不再画一排「来源：DSH / 来源：MiniMax」chip——来源名由目标名承担，坐标进 title。
  assert.doesNotMatch(template, /class="memory-permission"/, '旧独立横带必须删掉');
  assert.doesNotMatch(template, /来源：\{\{ source\.label \}\}/, '来源 chip 一排删掉');
  assert.match(permission, /<template #usage>/);
  assert.equal(
    (permission.match(/class="memory-target"/g) || []).length,
    2,
    '两个目标开关要并排',
  );
  // DSH 那半恒显；MiniMax 那半整段跟着体验开关（关掉时不渲染，也不读它的状态）。
  assert.match(permission, /<span class="memory-target">\s*<span class="memory-target__name" :title="sourceNamespace\('DSH'\)">DSH/);
  assert.match(permission, /<span v-if="mcodeExperience" class="memory-target">/);
  assert.match(permission, /<span class="memory-target__name minimax-wordmark" :title="sourceNamespace\('MiniMax'\)">MiniMax<\/span>/);
  assert.match(permission, /@change="\(enabled\) => setMemoryTargetUse\('dsh', enabled\)"/);
  assert.match(permission, /@change="\(enabled\) => setMemoryTargetUse\('mcode', enabled\)"/);
  // 未配置 MiniMax 实例时开关禁用并说明原因（不是画成已关闭）。
  assert.match(permission, /!mcodeConfigured/, 'MiniMax 未配置实例时要禁用');
  assert.match(source, /const mcodeConfigured = computed\(\(\) => !!mcode\.view\?\.instance_id\)/);
  assert.match(source, /'未配置实例'/);
  // 说明收进 ⓘ：关闭只停止使用，不删除记录、不撤销授权；开启本身不创建授权。
  assert.match(template, /:content="TARGET_USE_TIP"/);
  assert.match(template, /:aria-label="TARGET_USE_TIP"/);
  const tip = /const TARGET_USE_TIP = '([^']*)'/.exec(source);
  assert.ok(tip, '找不到 TARGET_USE_TIP');
  assert.match(tip[1], /记录不会删除/, '关闭不等于删除记录');
  assert.match(tip[1], /已有授权也不会撤销/, '关闭不撤销已有授权');
  assert.match(tip[1], /不创建跨内核授权/, '开启本身不创建授权');
  assert.match(tip[1], /跟随原设置/, '没选过时跟旧设置');
  assert.match(tip[1], /实例开关/, '要说清跟随的是哪一套旧设置');
  // 目标开关区域与授权面板严格分开：这里不出现模块授权的命令或动作。
  assert.doesNotMatch(permission, /memory_module|setMemoryModule|requestMemoryModule/);
  // 体验关时给一条短说明，不整宽告警；模块授权面板仍在它上方、仍可撤销。
  assert.match(template, /v-if="MCODE_BUILD_AVAILABLE && !mcodeExperience"/);
  assert.match(template, /MiniMax 入口已隐藏，历史记录仍保存在本机，重新开启体验入口后可查看。/);
  const moduleAt = template.indexOf('<MemoryModulePanel>');
  assert.ok(moduleAt > 0 && moduleAt === start, '通用记忆模块授权面板要保留在页面顶部');
  // 模块卡里第二行是 usage slot；来源 chip 与「已授权互通」状态词都删掉（开关状态自己表达）。
  const modulePanel = templateOf(read('../src/memory/MemoryModulePanel.vue'));
  assert.match(modulePanel, /<slot name="usage" \/>/, '允许使用记忆并入模块卡');
  assert.doesNotMatch(modulePanel, /来源：\{\{ source\.label \}\}/, '模块卡不再画来源 chip 一排');
  assert.doesNotMatch(modulePanel, /未授权互通/, '授权状态由开关表达，不再重复一行状态词');
});

test('体验关时页面固定 DSH 内容，FamilySwitch 与 MiniMax 目标一起隐藏，不删已有选择', () => {
  const source = read('../src/memory/MemoryPanel.vue');
  const template = templateOf(source);
  assert.match(template, /<FamilySwitch v-if="mcodeExperience"/, '体验关时隐藏内核切换器');
  // 分支本身没删：内容仍按 pageFamilies.memory 分两份，MiniMax 记录与 DSH 授权列表都在。
  const mcodeAt = template.indexOf('<template v-if="isMcode">');
  const dshAt = template.indexOf('<template v-else>');
  assert.ok(mcodeAt > 0 && dshAt > mcodeAt, '两个内容分支要保留');
  const mcodeBranch = template.slice(mcodeAt, dshAt);
  assert.match(mcodeBranch, /DSH 授权的记忆/, 'MiniMax 分支保留 DSH 授权列表');
  assert.match(mcodeBranch, /来源：DSH · \{\{ item\.source_instance \}\} · \{\{ item\.source_profile \}\}/,
    'DSH 授权记录继续显示来源实例与 profile');
  assert.match(mcodeBranch, /origin-chip-kernel"><span class="minimax-wordmark">MiniMax<\/span>/,
    'MiniMax 列表卡头明确标出来源');
  const dshBranch = template.slice(dshAt);
  assert.match(dshBranch, /origin-chip-kernel">DSH</, 'DSH 列表卡头明确标出来源');
  // 「不删除已有记录」不只靠文案：两个分支的列表与数据源都在。
  assert.match(dshBranch, /<MemoryRow/);
  assert.match(mcodeBranch, /<MemoryRow/);
});

test('目标开关在挂载与实例变化时重读，卸载时清空，不把旧实例的状态画到新实例上', () => {
  const source = read('../src/memory/MemoryPanel.vue');
  assert.match(source, /onMounted\(refreshMemoryTargetUse\)/, '挂载 / 切回记忆页时刷新');
  assert.match(
    source,
    /watch\(\(\) => \[mcode\.view\?\.instance_id, mcode\.view\?\.memory_profile\][\s\S]*?refreshMemoryTargetUse\(\)/,
    'MiniMax 实例变化时重读目标状态',
  );
  assert.match(source, /onUnmounted\(\(\) => \{[^}]*resetMemoryTargetUse\(\)/, '离开页面时清空，不让迟到的结果回到别的实例');
  // 旧的实例开关改了「跟随原设置」的效值，同一屏上方那枚开关不能停在旧读数上。
  assert.match(
    source,
    /function changeMemoryEnabled\(enabled\) \{\s*return setMemoryEnabled\(enabled\)\.then\(\(\) => refreshMemoryTargetUse\(\)\);/,
    '旧开关改完要重读目标状态',
  );
  assert.match(templateOf(source), /@change="changeMemoryEnabled"/, '旧开关要接重读后的那条动作');
});
