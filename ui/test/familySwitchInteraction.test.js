import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';
import * as Vue from 'vue';

const source = readFileSync(new URL('../src/kernel/FamilySwitch.vue', import.meta.url), 'utf8');

// 执行真实组件脚本，保留 Vue 的 ref / computed / watch 语义，避免只扫字符串漏掉 ref 真值错误。
function setupSwitch(experienceEnabled = true) {
  const props = Vue.reactive({ modelValue: 'dsh' });
  const busy = Vue.ref(false);
  const memory = Vue.reactive({ editorVisible: false });
  const instance = Vue.reactive({ familyLoaded: true, familySwitching: false, familyError: '' });
  const loading = Vue.reactive(new Set());
  const events = [];
  const scope = Vue.effectScope();
  const script = source.match(/<script setup>([\s\S]*?)<\/script>/)[1]
    .replace(/^import .*;$/gm, '');
  const run = new Function('computed', 'nextTick', 'ref', 'watch', 'globalBusy', 'isLoading',
    'memoryStore', 'instanceStore', 'loadKernelFamily', 'defineProps', 'defineEmits', 'document',
    'mcodeExperience',
    `${script}\nreturn { disabled, pick, move, tabsEl };`);
  const api = scope.run(() => run(Vue.computed, Vue.nextTick, Vue.ref, Vue.watch, busy,
    key => loading.has(key), memory, instance, () => {}, () => props,
    () => (...args) => events.push(args), { activeElement: null },
    Vue.computed(() => experienceEnabled)));
  return { ...api, props, busy, memory, instance, loading, events, scope };
}

test('空闲时可切换，忙碌、编辑、读取和错误状态阻止操作，解除后恢复', () => {
  const state = setupSwitch();
  try {
    assert.equal(state.disabled.value, false);
    state.pick('mcode');
    assert.deepEqual(state.events, [['update:modelValue', 'mcode']]);
    for (const [object, key, active, idle] of [
      [state.busy, 'value', true, false],
      [state.memory, 'editorVisible', true, false],
      [state.instance, 'familyLoaded', false, true],
      [state.instance, 'familySwitching', true, false],
      [state.instance, 'familyError', '保存失败', ''],
    ]) {
      object[key] = active;
      assert.equal(state.disabled.value, true);
      state.pick('mcode');
      object[key] = idle;
      assert.equal(state.disabled.value, false);
    }
    for (const key of ['memorySave', 'loadKernelFamily']) {
      state.loading.add(key);
      assert.equal(state.disabled.value, true);
      state.pick('mcode');
      state.loading.delete(key);
      assert.equal(state.disabled.value, false);
    }
    state.pick('dsh');
    assert.equal(state.events.length, 1, '禁用与重复选中都不发切换事件');
  } finally { state.scope.stop(); }
});

test('方向键循环切换，Home / End 定位首尾，启动回填不抢焦点', async () => {
  const state = setupSwitch();
  let focused = 0;
  state.tabsEl.value = { contains: () => false, querySelector: () => ({ focus: () => focused++ }) };
  try {
    for (const key of ['ArrowRight', 'ArrowLeft', 'End']) {
      let prevented = false;
      state.move({ key, preventDefault() { prevented = true; } });
      assert.equal(prevented, true);
      assert.deepEqual(state.events.at(-1), ['update:modelValue', 'mcode']);
    }
    state.props.modelValue = 'mcode';
    await Vue.nextTick();
    await Vue.nextTick();
    assert.equal(focused, 0);
    state.move({ key: 'Home', preventDefault() {} });
    assert.deepEqual(state.events.at(-1), ['update:modelValue', 'dsh']);
    assert.match(source, /:tabindex="modelValue === item.id \? 0 : -1"/);
  } finally { state.scope.stop(); }
});

test('版本页进入与每次切入 MiniMax 均加载状态 / 版本 / 发布列表，切回 DSH 不触发 MiniMax 读取', async () => {
  const panel = readFileSync(new URL('../src/kernel/VersionsPanel.vue', import.meta.url), 'utf8');
  const watcher = panel.match(/watch\(isMcode, \(selected\) => \{[\s\S]*?\}, \{ immediate: true \}\);/);
  assert.ok(watcher, '版本页必须响应页内选择变化');
  const selected = Vue.ref(false);
  const calls = [];
  const scope = Vue.effectScope();
  try {
    // 发布列表也随页面打开 / 切入自动拉（2026-11-08 用户要求，不再等点刷新）。
    scope.run(() => new Function('watch', 'isMcode', 'refreshMcodeStatus', 'loadMcodeInstalledVersions', 'loadMcodeAvailableVersions', watcher[0])(
      Vue.watch, selected, () => calls.push('状态'), () => calls.push('版本'), () => calls.push('发布列表')));
    assert.deepEqual(calls, []);
    for (let i = 0; i < 2; i++) {
      selected.value = true;
      await Vue.nextTick();
      selected.value = false;
      await Vue.nextTick();
    }
    assert.deepEqual(calls, ['状态', '版本', '发布列表', '状态', '版本', '发布列表']);
  } finally { scope.stop(); }
});
