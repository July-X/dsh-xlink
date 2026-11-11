import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';
import { ref, watch, effectScope, nextTick, reactive } from 'vue';
import { withLoading } from '../src/shell/loading.js';
import { adoptDraft } from '../src/shell/portDrafts.js';

// R08/R09：端口草稿被轮询/晚到快照覆盖。判据分两层——纯决策函数真的怎么判，
// 以及组件有没有把它接上（本仓反复栽在「helper 有测试、接线没有」）。

const panel = readFileSync(new URL('../src/shell/SettingsPanel.vue', import.meta.url), 'utf8');

test('没有草稿的字段接受首个晚到快照（R09：开启体验后端口要能回填）', () => {
  const field = ref(undefined);
  const dirty = ref(false);
  adoptDraft(field, dirty, 3100);
  assert.equal(field.value, 3100, '聚焦期间回来的首个快照也要回填');
  adoptDraft(field, dirty, 3101);
  assert.equal(field.value, 3101, '没有草稿就跟随最新快照');
});

test('有草稿的字段不被轮询覆盖，失焦不是放弃修改（R08）', () => {
  const field = ref(3000);
  const dirty = ref(false);
  adoptDraft(field, dirty, 3000);
  // 用户把 3000 改成 4000，随后点了卡片空白处（焦点离开）再等下一轮 get_status。
  field.value = 4000;
  dirty.value = true;
  adoptDraft(field, dirty, 3000);
  assert.equal(field.value, 4000, '未保存的草稿不能被轮询覆盖');
  assert.equal(dirty.value, true);
});

test('相同值快照也不解除未确认的草稿，避免改回原值的编辑被误认作保存', () => {
  const field = ref(4000);
  const dirty = ref(true);
  adoptDraft(field, dirty, 4000);
  assert.equal(dirty.value, true);
  adoptDraft(field, dirty, 5000);
  assert.equal(field.value, 4000);
});

test('接线上：三个端口一起改用了按字段 dirty，焦点保护已删除', () => {
  for (const [field, dirty] of [
    ['port', 'portDirty'],
    ['mcodePort', 'mcodePortDirty'],
    ['mcodeMiniappPort', 'mcodeMiniappDirty'],
  ]) {
    assert.match(
      panel,
      new RegExp('adoptDraft\\(' + field + ', ' + dirty + ','),
      field + ' 的回填必须走 adoptDraft',
    );
    assert.match(
      panel,
      new RegExp('v-on:update:model-value="' + dirty + ' = true;'),
      field + ' 的用户编辑要标 dirty',
    );
    assert.match(panel, new RegExp(dirty + '\\.value = false'), dirty + ' 保存成功后要清掉');
  }
  assert.doesNotMatch(panel, /@focusin|@focusout/);
  assert.doesNotMatch(panel, /\bediting\b/, '焦点不再是草稿保护机制');
});

// 执行组件实际的表单状态、快照 watcher 和保存函数，延迟由测试控制。
function deferred() {
  let resolve;
  const promise = new Promise((done) => { resolve = done; });
  return { promise, resolve };
}
function form(t) {
  const scope = effectScope();
  t.after(() => scope.stop());
  const dsh = deferred();
  const mini = deferred();
  const responses = { dsh, mini };
  const calls = [];
  const store = { view: ref({ settings: { port: 3000, profile: 'web' } }) };
  // 真实组件的 store / mcode 是 reactive，watch 必须观察实际快照更换。
  const state = reactive({ view: store.view.value });
  const mcode = reactive({ view: { port: 3100, miniapp_port: 3200 } });
  const script = panel.slice(panel.indexOf('const port = ref('), panel.indexOf('// 开关的语义'));
  const fields = ['port', 'mcodePort', 'mcodeMiniappPort'];
  const inputs = [...panel.matchAll(/<el-input-number[\s\S]*?\/>/g)].map((m) => m[0]);
  const edits = fields.map((field) => {
    const input = inputs.find((s) => s.includes(`v-model="${field}"`));
    const handler = input.match(/v-on:update:model-value="([^"]+)"/)[1]
      .replace(/(\w+Dirty) =/g, '$1.value =');
    return `${field}: (value) => { ${field}.value = value; ${handler}; }`;
  }).join(',');
  const api = scope.run(() => new Function('ref', 'watch', 'adoptDraft', 'withLoading',
    'store', 'mcode', 'mcodeExperience', 'saveSettings', 'saveMcodePorts',
    script + `; return { port, portDirty, mcodePort, mcodePortDirty, mcodeMiniappPort, mcodeMiniappDirty, onSavePorts, edit: {${edits}} };`)(
    ref, watch, adoptDraft, withLoading, state, mcode, ref(true),
    (...args) => { calls.push(['dsh', ...args]); return responses.dsh.promise; },
    (...args) => { calls.push(['mini', ...args]); return responses.mini.promise; },
  ));
  return { ...api, dsh, mini, calls, state, mcode, responses };
}


test('保存成功只解除对应草稿；在途新编辑经后续快照仍保留', async (t) => {
  const f = form(t);
  f.edit.port(4000);
  f.edit.mcodePort(4100);
  f.edit.mcodeMiniappPort(4200);
  const saving = f.onSavePorts();
  f.edit.port(4001);
  f.edit.mcodePort(4101);
  f.dsh.resolve(true);
  await nextTick();
  assert.deepEqual(f.calls, [['dsh', 4000, 'web'], ['mini', 4100, 4200]]);
  f.mini.resolve(true);
  await saving;
  assert.equal(f.portDirty.value, true);
  assert.equal(f.mcodePortDirty.value, true);
  assert.equal(f.mcodeMiniappDirty.value, false);
  f.state.view = { settings: { port: 4000, profile: 'web' } };
  f.mcode.view = { port: 4100, miniapp_port: 4200 };
  await nextTick();
  assert.equal(f.port.value, 4001);
  assert.equal(f.mcodePort.value, 4101);
});

test('改回提交值和重复相同值编辑仍是新草稿，旧响应和相同快照不能解除', async (t) => {
  const f = form(t);
  f.edit.port(4000);
  f.edit.mcodePort(4100);
  f.edit.mcodeMiniappPort(4200);
  const saving = f.onSavePorts();
  f.edit.port(4001);
  f.edit.port(4000);
  f.edit.mcodePort(4100);
  f.dsh.resolve(true);
  await nextTick();
  f.edit.mcodeMiniappPort(4201);
  f.edit.mcodeMiniappPort(4200);
  f.mini.resolve(true);
  await saving;
  f.state.view = { settings: { port: 4000 } };
  f.mcode.view = { port: 4100, miniapp_port: 4200 };
  await nextTick();
  for (const dirty of [f.portDirty, f.mcodePortDirty, f.mcodeMiniappDirty]) assert.equal(dirty.value, true);
});

test('整次保存去重，失败保留草稿且允许重试', async (t) => {
  const f = form(t);
  f.edit.port(4000);
  f.edit.mcodePort(4100);
  const saving = f.onSavePorts();
  await f.onSavePorts();
  assert.equal(f.calls.length, 1);
  f.dsh.resolve(false);
  await nextTick();
  await f.onSavePorts();
  assert.equal(f.calls.length, 2);
  f.mini.resolve(false);
  await saving;
  assert.equal(f.portDirty.value, true);
  assert.equal(f.mcodePortDirty.value, true);
  f.responses.dsh = deferred();
  f.responses.mini = deferred();
  const retry = f.onSavePorts();
  f.responses.dsh.resolve(true);
  f.responses.mini.resolve(true);
  await retry;
  assert.equal(f.portDirty.value, false);
  assert.equal(f.mcodePortDirty.value, false);
  assert.equal(f.calls.length, 4);
});

test('没有后续编辑时成功解除草稿，后续快照正常回填', async (t) => {
  const f = form(t);
  f.edit.port(4000);
  f.edit.mcodePort(4100);
  const saving = f.onSavePorts();
  f.dsh.resolve(true);
  f.mini.resolve(true);
  await saving;
  assert.equal(f.portDirty.value, false);
  assert.equal(f.mcodePortDirty.value, false);
  f.state.view = { settings: { port: 5000 } };
  f.mcode.view = { port: 5100, miniapp_port: 5200 };
  await nextTick();
  assert.equal(f.port.value, 5000);
  assert.equal(f.mcodePort.value, 5100);
});

test('MiniMax 草稿改回已保存值后，显式保存确认草稿且不重复写入', async (t) => {
  const f = form(t);
  f.edit.mcodePort(4100);
  f.edit.mcodePort(3100);
  const saving = f.onSavePorts();
  f.dsh.resolve(true);
  await saving;
  assert.equal(f.mcodePortDirty.value, false);
  assert.equal(f.calls.length, 1);
});
