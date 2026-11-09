import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';
import { ref } from 'vue';
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

test('后端已是草稿里的值：草稿自动作废（保存成功后继续跟随）', () => {
  const field = ref(4000);
  const dirty = ref(true);
  adoptDraft(field, dirty, 4000);
  assert.equal(dirty.value, false);
  adoptDraft(field, dirty, 5000);
  assert.equal(field.value, 5000, '保存成功后快照重新可回填');
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
      new RegExp('v-on:update:model-value="' + dirty + ' = true"'),
      field + ' 的用户编辑要标 dirty',
    );
    assert.match(panel, new RegExp(dirty + '\\.value = false'), dirty + ' 保存成功后要清掉');
  }
  assert.doesNotMatch(panel, /@focusin|@focusout/);
  assert.doesNotMatch(panel, /\bediting\b/, '焦点不再是草稿保护机制');
});
