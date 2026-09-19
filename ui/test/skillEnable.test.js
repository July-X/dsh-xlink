import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

// 技能启停的后端能力（`skill_set_enabled` + `enabled:false` 语义 + 启动对账）一直是完整的，
// 但 v1 面板从未调用过它：`docs/skill-management.md` 把它记为「尚未接线」，用户只能靠整包
// 卸载来收手。这组用例钉住「面板 → 动作 → 命令 → Rust 签名」这条链，避免它再次断掉。
const read = (path) => readFileSync(path, 'utf8');

test('技能启停动作接的是后端 skill_set_enabled 命令', () => {
  const skills = read('ui/src/skills.js');
  assert.match(skills, /export function setSkillEnabled\(id, name, enabled\)/);
  assert.match(skills, /cmd: 'skill_set_enabled'/);
  assert.match(skills, /\(\{ id, name, enabled, onEvent: channel \}\)/);
});

test('前端载荷的参数名与 Rust 命令签名逐字对齐', () => {
  const commands = read('src-tauri/src/commands.rs');
  const signature = commands.match(/pub async fn skill_set_enabled\(([\s\S]*?)\)\s*->/);
  assert.ok(signature, 'commands.rs 里必须能找到 skill_set_enabled 的签名');
  for (const param of ['id: String', 'name: String', 'enabled: bool', 'on_event: Channel<String>']) {
    assert.ok(signature[1].includes(param), `命令参数缺少 ${param}`);
  }
});

test('面板逐个技能暴露开关，并挂按条目的 loading', () => {
  const panel = read('ui/src/components/SkillsPanel.vue');
  assert.match(panel, /setSkillEnabled/);
  assert.match(panel, /v-for="skill in row\.skills"/);
  // loading 必须是按条目 key，而不是全局 globalBusy（P2-10 / P2-42 那类死绑定）。
  assert.match(panel, /:loading="isLoading\(skillEnabledKey\(row\.id, skill\.name\)\)"/);
  assert.match(panel, /@change="\(value\) => toggleSkill\(row, skill, value\)"/);
});

test('启停开关位于包头动作区，不渲染下方技能清单', () => {
  const panel = read('ui/src/components/SkillsPanel.vue');
  const actions = panel.indexOf('<div class="entity-actions skill-row-actions">');
  assert.ok(actions >= 0);
  assert.match(panel.slice(actions), /v-for="skill in row\.skills"[\s\S]*<el-switch/);
  assert.doesNotMatch(panel, /class="skill-list"|class="skill-item"/);
});

test('条目该在却不在时给出提示，而不是静默显示为已启用', () => {
  const panel = read('ui/src/components/SkillsPanel.vue');
  assert.match(panel, /skill\.enabled && !skill\.present/);
  assert.match(panel, /条目缺失/);
});

test('启停开关不依赖技能清单或弹层', () => {
  const panel = read('ui/src/components/SkillsPanel.vue');
  assert.doesNotMatch(panel, /skill-list|skill-item|skill-toggle-popover|skillCountText/);
});

test('包版本 tag 与包名同行，长说明收进单行 Tooltip', () => {
  const panel = read('ui/src/components/SkillsPanel.vue');
  const theme = read('ui/src/theme.css');
  assert.match(panel, /class="entity-name"[\s\S]*class="meta-version"/);
  assert.match(panel, /<el-tooltip v-if="row\.description"[\s\S]*:content="row\.description"/);
  assert.match(panel, /class="entity-desc skill-package-description"/);
  assert.match(theme, /\.entity-desc[\s\S]*white-space: nowrap;[\s\S]*text-overflow: ellipsis;/);
  assert.match(theme, /\.skill-package-description\s*\{\s*display: block;\s*margin: 6px 0 0;/);
});
