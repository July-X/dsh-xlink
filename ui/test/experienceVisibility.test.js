// 「体验 MiniMax」这一个开关决定哪些入口出现（T02 / T04 / T05 / T06 / T09）。
//
// 判据在 kernel/experience.js 里只有一份：构建包含这些能力，且用户选了体验。
// 页面只消费 mcodeExperience；关掉入口不停止工作台、不改已有选择、不撤销授权，
// 真实运行状态与退出停机一律按后端状态执行。
//
// 判据纪律（ui/AGENTS.md §判据怎么写）：扫模板不扫全文、先剥注释再断言、
// 钉接线与真实行为而不是字面量。构建旗子的扫描是唯一例外——「只有 experience.js
// 读它」本身就是这条判据要守的事实。
import assert from 'node:assert/strict';
import { readdirSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import * as Vue from 'vue';

const SRC = fileURLToPath(new URL('../src/', import.meta.url));

/** ui/src 下所有 .vue 与 .js，递归。 */
function srcFiles(dir = SRC, out = []) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = resolve(dir, entry.name);
    if (entry.isDirectory()) srcFiles(path, out);
    else if (path.endsWith('.vue') || path.endsWith('.js')) out.push(path);
  }
  return out;
}

/** 相对路径一律用正斜杠：判据里写的是 `kernel/experience.js`，Windows 上是反斜杠。 */
const relative = (path) => path.slice(SRC.length).replaceAll('\\', '/');
const read = (rel) => readFileSync(resolve(SRC, rel), 'utf8');
/** 脚本 / 样式侧注释（块注释 + 整行行注释）：说明里常原样引用被判据的词。 */
const commentless = (src) => src
  .replace(/\/\*[\s\S]*?\*\//g, '')
  .replace(/^[ \t]*\/\/.*$/gm, '');
/** 模板块，HTML 注释已剥；收尾边界取 style 标签。 */
function templateOf(src) {
  const start = src.indexOf('<template>');
  assert.ok(start >= 0, '找不到 <template>');
  const styleAt = src.indexOf('<style', start);
  return src.slice(start, styleAt > start ? styleAt : src.length).replace(/<!--[\s\S]*?-->/g, '');
}

test('构建旗子只有 experience.js 读：其余页面消费 mcodeExperience', () => {
  const readers = srcFiles()
    .filter((path) => commentless(readFileSync(path, 'utf8')).includes('import.meta.env.VITE_MCODE_EXPERIMENTAL'))
    .map(relative);
  assert.deepEqual(
    readers,
    ['kernel/experience.js'],
    '页面自己读构建旗子会让保存的体验偏好失效（关了入口还在 / 开了入口不出现）',
  );
  assert.match(read('kernel/experience.js'), /export const mcodeExperience = computed/);
});

test('体验关时切换器整枚不渲染：闸门在 FamilySwitch 自己身上', () => {
  const source = read('kernel/FamilySwitch.vue');
  assert.match(source, /import \{ mcodeExperience \} from '\.\/experience\.js';/);
  assert.match(templateOf(source), /v-if="options\.length > 1"/, '三个页面不必各加一条 v-if');
  // 手写体品牌词保留：可见性只管出现与否，不改品牌排印。
  assert.match(templateOf(source), /'minimax-wordmark'/);
  assert.match(templateOf(source), /'deepseek-wordmark'/);

  // 真跑一遍组件脚本：选项由 mcodeExperience 求值，关掉时为空。
  function optionsFor(experienceEnabled) {
    const script = source.match(/<script setup>([\s\S]*?)<\/script>/)[1].replace(/^import .*;$/gm, '');
    const scope = Vue.effectScope();
    const run = new Function('computed', 'nextTick', 'ref', 'watch', 'globalBusy', 'isLoading',
      'memoryStore', 'instanceStore', 'loadKernelFamily', 'defineProps', 'defineEmits', 'document',
      'mcodeExperience',
      script + '\nreturn { options };');
    const api = scope.run(() => run(Vue.computed, Vue.nextTick, Vue.ref, Vue.watch, Vue.ref(false),
      () => false, Vue.reactive({ editorVisible: false }),
      Vue.reactive({ familyLoaded: true, familySwitching: false, familyError: '' }),
      () => {}, () => ({ modelValue: 'dsh' }), () => () => {}, { activeElement: null },
      Vue.computed(() => experienceEnabled)));
    const options = api.options.value;
    scope.stop();
    return options;
  }
  assert.deepEqual(optionsFor(false), []);
  assert.deepEqual(optionsFor(true).map((option) => option.id), ['dsh', 'mcode']);
});

test('概览把两套工作台状态传给分段控件，只有概览传', () => {
  const overview = read('shell/OverviewPanel.vue');
  const switchTpl = templateOf(read('kernel/FamilySwitch.vue'));
  assert.match(templateOf(overview), /<FamilySwitch\s+:model-value="family"\s+:statuses="familyStatuses"/);
  assert.match(switchTpl, /v-if="statuses && statuses\[item\.id\]"/, '圆点与短词只在传了状态时渲染');
  assert.match(switchTpl, /:class="statuses\[item\.id\]\.cls"/);
  assert.match(switchTpl, /\{\{ statuses\[item\.id\]\.text \}\}/);
  assert.match(overview, /const familyStatuses = computed\(\(\) => \(\{ dsh: dshKernelStatus\.value, mcode: mcodeKernelStatus\.value \}\)\)/);
  // 内核版本页与记忆页只拿切换器选数据源，不汇报运行状态。
  const consumers = srcFiles()
    .filter((path) => readFileSync(path, 'utf8').includes(':statuses='))
    .map(relative);
  assert.deepEqual(consumers, ['shell/OverviewPanel.vue']);
});

test('MiniMax 状态读不到时给「未知」，不把读取失败画成已停止', () => {
  const body = /const mcodeKernelStatus = computed\(\(\) => \{([\s\S]*?)\n\}\);/.exec(
    read('shell/OverviewPanel.vue'),
  )[1];
  function statusFor(view, { experience = true, error = '' } = {}) {
    return new Function('mcodeExperience', 'mcodeView', 'mcode', body)(
      Vue.computed(() => experience), Vue.ref(view), { error },
    );
  }
  assert.equal(statusFor({ installed: true }, { experience: false }), null, '体验关时概览按 DSH 展示，不需要它的状态');
  assert.equal(statusFor(undefined).text, '未知', '还没读到过不等于已停止');
  assert.equal(statusFor(undefined, { error: '读取失败' }).text, '未知', '读取失败同样不能猜成已停止');
  assert.deepEqual(statusFor({ running: true }), { text: '运行中', cls: 'ok' });
  assert.deepEqual(statusFor({ installed: true }), { text: '已停止', cls: 'bad' });
  assert.deepEqual(statusFor({ installed: false }), { text: '未安装', cls: '' });
  assert.equal(statusFor({ stale_pid: true }).cls, 'bad', '旧记录挡住停止时是待处理状态');
});

test('概览常驻轮询两套状态：未选中 MiniMax 时它的状态也在更新', async () => {
  const overview = read('shell/OverviewPanel.vue');
  const watchSource = /let mcodeStatusTimer;[\s\S]*?\{ immediate: true \},\n\);/.exec(overview);
  assert.ok(watchSource, '找不到概览的 MiniMax 状态轮询');
  assert.match(watchSource[0], /watch\(\s*mcodeExperience,/, '轮询跟体验开关走');
  assert.doesNotMatch(watchSource[0], /isMcode/, '未选中那一侧的状态也得是新的，轮询不能只看选中项');

  const experience = Vue.ref(true);
  const busy = Vue.ref(false);
  const calls = [];
  const timers = [];
  const run = new Function('watch', 'mcodeExperience', 'refreshMcodeStatus', 'globalBusy',
    'MCODE_STATUS_INTERVAL_MS', 'setInterval', 'clearInterval',
    watchSource[0]);
  run(Vue.watch, experience, () => calls.push('refresh'), busy, 5000,
    (fn, ms) => { const timer = { fn, ms, cleared: false }; timers.push(timer); return timer; },
    (timer) => { if (timer) timer.cleared = true; });
  try {
    assert.equal(calls.length, 1, '挂载即刷新一次：两份状态都要有值');
    assert.equal(timers[0].ms, 5000);
    busy.value = true;
    timers[0].fn();
    assert.equal(calls.length, 1, 'globalBusy 期间跳过轮询');
    busy.value = false;
    timers[0].fn();
    assert.equal(calls.length, 2);
    experience.value = false;
    await Vue.nextTick();
    assert.equal(timers[0].cleared, true, '体验关掉后不为看不见的入口发请求');
    experience.value = true;
    await Vue.nextTick();
    assert.equal(calls.length, 3, '重新开启立即补一次');
    assert.equal(timers.length, 2);
  } finally {
    experience.value = false;
    await Vue.nextTick();
  }
});

test('插件 / 技能 / 用量 / 内核版本的 MiniMax 入口跟着 mcodeExperience', () => {
  const plugins = read('plugins/PluginsPanel.vue');
  assert.match(plugins, /import \{ mcodeExperience \} from '\.\.\/kernel\/experience\.js';/);
  assert.equal((templateOf(plugins).match(/<MinimaxPluginTarget v-if="mcodeExperience"/g) || []).length, 2,
    '内置插件与社区插件两处目标控件都要跟体验开关');
  assert.match(plugins, /mcodeExperience\.value \? \[refreshMcodeStatus\(\), loadMcodeBuiltins\(\)\] : \[\]/,
    '体验关着时不去读 MiniMax 状态');

  const skills = read('skills/SkillsPanel.vue');
  assert.match(skills, /import \{ mcodeExperience \} from '\.\.\/kernel\/experience\.js';/);
  assert.match(templateOf(skills), /<el-tooltip v-if="mcodeExperience" placement="top" :content="skillMcodeTip\(skill\)"/,
    '逐项 MiniMax 目标跟着体验开关');
  assert.match(
    skills,
    /watch\(mcodeExperience, \(enabled\) => \{\s*if \(enabled\) refreshMcodeStatus\(\);/,
    '偏好可能在首个状态之后才到：要跟偏好补拉，不能只 onMounted 一次',
  );

  assert.match(templateOf(read('usage/UsageAnalysisPanel.vue')), /v-if="mcodeExperience" class="memory-note usage-model-note"/,
    '用量页只隐藏 MiniMax 说明，外部来源与授权保留');

  assert.match(read('kernel/VersionsPanel.vue'),
    /const isMcode = computed\(\(\) => mcodeExperience\.value && family\.value === 'mcode'\)/,
    '体验关掉时分段控件与 MiniMax 版本操作一起消失');
});

test('日志来源筛选：侧栏由 prop 控制，独立窗口自己取偏好', () => {
  const sidebar = read('logs/LogSidebar.vue');
  assert.doesNotMatch(commentless(sidebar), /VITE_MCODE_EXPERIMENTAL/);
  assert.match(sidebar, /sourceFilterEnabled: \{ type: Boolean, default: false \}/);
  assert.match(templateOf(sidebar), /v-if="sourceFilterEnabled"/);

  const modal = read('logs/LogModal.vue');
  assert.match(modal, /import \{ mcodeExperience \} from '\.\.\/kernel\/experience\.js';/);
  assert.match(modal, /:source-filter-enabled="mcodeExperience"/);

  const viewer = read('logs/LogViewerWindow.vue');
  assert.match(viewer, /import \{ loadMcodeExperience \} from '\.\.\/kernel\/experience\.js';/);
  assert.match(viewer, /const sourceFilterEnabled = ref\(false\)/, '取不到偏好时筛选入口默认隐藏');
  assert.match(viewer, /function syncSourceFilter\(\) \{\s*loadMcodeExperience\(\)/);
  assert.match(viewer, /onMounted\(\(\) => \{[\s\S]*?syncSourceFilter\(\);/,
    '重新打开窗口时读一次偏好');
  assert.match(viewer, /window\.addEventListener\('focus', syncSourceFilter\)/,
    '已打开的窗口重新获得焦点时也要刷新偏好');
  assert.match(viewer, /window\.removeEventListener\('focus', syncSourceFilter\)/);
  assert.match(viewer, /:source-filter-enabled="sourceFilterEnabled"/);
  // 偏好取不到时命令侧兜成 false（构建不含能力 / 读取失败）。
  assert.match(read('kernel/experience.js'),
    /if \(!MCODE_BUILD_AVAILABLE\) return Promise\.resolve\(false\);[\s\S]*?invoke\('get_mcode_experience'\)\.catch\(\(\) => false\)/);
});

test('可见性不短路行为链：退出仍按真实运行状态停止 MiniMax', () => {
  const app = read('App.vue');
  const quit = app.slice(
    app.indexOf('async function onQuitConfirmRequest'),
    app.indexOf('function refreshActivePanelData'),
  );
  assert.ok(quit.length > 0, '找不到退出确认流程');
  assert.match(quit, /payload\.mcode_running/, '退出确认读真实运行状态');
  assert.match(quit, /mcodeRunning \? stopMcodeForExit\(\)/, '仍在运行的 MiniMax 退出时照常停止');
  assert.doesNotMatch(quit, /mcodeExperience/, '入口隐藏不许短路退出停机');
  // 停机动作本身不变：命令 + 状态刷新都留在 mcode.js。
  assert.match(read('kernel/mcode.js'), /export async function stopMcodeForExit\(\) \{\s*invalidateStatus\(\);\s*await invoke\('stop_mcode'\);/);
  // 概览的运行状态仍读真实字段，不受可见性控制。
  assert.match(read('shell/OverviewPanel.vue'),
    /const running = computed\(\(\) => !!\(kernel\.value && kernel\.value\.running\)\)/);
});
test('体验解析规则：构建不可用短路、显式偏好优先、未保存按壳模式兜底', async () => {
  // experience.js 依赖 store.js（会碰 Tauri 桥）：Node 下先把 window 桩上再动态 import。
  globalThis.window = { __TAURI__: { core: { invoke: async () => null } } };
  const { resolveMcodeExperience, mcodeExperience } = await import('../src/kernel/experience.js');
  assert.equal(resolveMcodeExperience(false, true, 'dev'), false, '构建不含能力时任何偏好都不显示');
  assert.equal(resolveMcodeExperience(true, true, 'release'), true, '显式开优先于正式壳默认关');
  assert.equal(resolveMcodeExperience(true, false, 'dev'), false, '显式关优先于开发壳默认开');
  assert.equal(resolveMcodeExperience(true, null, 'dev'), true, '开发壳没保存过默认开');
  assert.equal(resolveMcodeExperience(true, null, 'release'), false, '正式壳没保存过默认关');
  assert.equal(resolveMcodeExperience(true, undefined, 'dev'), true, '缺字段与 null 同义');
  // computed 真的接上了解析：Node 里构建旗子不可用，即便 store 说开也不显示。
  const { store } = await import('../src/store.js');
  store.view = { settings: { mcode_experience: true }, shell_mode: 'dev' };
  assert.equal(mcodeExperience.value, false, '构建不可用时 computed 必须短路');
});
