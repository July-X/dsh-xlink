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

test('条目该在却不在时给出提示，而不是静默显示为已启用', () => {
  const panel = read('ui/src/components/SkillsPanel.vue');
  assert.match(panel, /skill\.enabled && !skill\.present/);
  assert.match(panel, /条目缺失/);
});
