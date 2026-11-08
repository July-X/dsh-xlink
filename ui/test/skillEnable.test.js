import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { readShellSource } from '../../scripts/lib/shell-source.mjs';

// 技能启停的后端能力（`skill_set_enabled` + `enabled:false` 语义 + 启动对账）一直是完整的，
// 但 v1 面板从未调用过它：`docs/features/extensions/skill-management.md` 把它记为「尚未接线」，用户只能靠整包
// 卸载来收手。这组用例钉住「面板 → 动作 → 命令 → Rust 签名」这条链，避免它再次断掉。
const read = (path) => readFileSync(path, 'utf8');

test('技能启停动作接的是后端 skill_set_enabled 命令', () => {
  const skills = read('ui/src/skills/skills.js');
  assert.match(skills, /export function setSkillEnabled\(id, name, enabled\)/);
  assert.match(skills, /cmd: 'skill_set_enabled'/);
  assert.match(skills, /\(\{ id, name, enabled, onEvent: channel \}\)/);
});

test('前端载荷的参数名与 Rust 命令签名逐字对齐', () => {
  const commands = readShellSource('commands.rs');
  const signature = commands.match(/pub async fn skill_set_enabled\(([\s\S]*?)\)\s*->/);
  assert.ok(signature, 'commands.rs 里必须能找到 skill_set_enabled 的签名');
  for (const param of ['id: String', 'name: String', 'enabled: bool', 'on_event: Channel<String>']) {
    assert.ok(signature[1].includes(param), `命令参数缺少 ${param}`);
  }
});

test('面板逐个技能暴露开关，并挂按条目的 loading', () => {
  const panel = read('ui/src/skills/SkillsPanel.vue');
  assert.match(panel, /setSkillEnabled/);
  assert.match(panel, /v-for="skill in row\.skills"/);
  // loading 必须是按条目 key，而不是全局 globalBusy（P2-10 / P2-42 那类死绑定）。
  assert.match(panel, /:loading="isLoading\(skillEnabledKey\(row\.id, skill\.name\)\)"/);
  assert.match(panel, /@change="\(value\) => toggleSkill\(row, skill, value\)"/);
});

test('启停开关位于包头动作区，不渲染下方技能清单', () => {
  const panel = read('ui/src/skills/SkillsPanel.vue');
  const actions = panel.indexOf('<div class="entity-actions skill-row-actions">');
  assert.ok(actions >= 0);
  assert.match(panel.slice(actions), /v-for="skill in row\.skills"[\s\S]*<el-switch/);
  assert.doesNotMatch(panel, /class="skill-list"|class="skill-item"/);
});

test('条目该在却不在时给出提示，而不是静默显示为已启用', () => {
  const panel = read('ui/src/skills/SkillsPanel.vue');
  assert.match(panel, /skill\.enabled && !skill\.present/);
  assert.match(panel, /条目缺失/);
});

test('启停开关不依赖技能清单或弹层', () => {
  const panel = read('ui/src/skills/SkillsPanel.vue');
  assert.doesNotMatch(panel, /skill-list|skill-item|skill-toggle-popover|skillCountText/);
});


// --- 两个内核的目标开关（2026-11-08 用户要求） ---------------------------
//
// 「技能的启用方式参考插件的做法，安装本身应该是全局性质，通过启用控制它
// 在那边内核生效」：安装/中央库共享不变，每个技能多一个 MiniMax 目标开关，
// 与 DSH 的启停互不影响。下面钉「面板 → 动作 → 命令 → Rust 签名」整条链，
// 任何一段断开都会退回到「一个开关同时管两个内核」的旧行为。

test('MiniMax 目标开关接的是后端 skill_set_mcode_enabled 命令', () => {
  const skills = read('ui/src/skills/skills.js');
  assert.match(skills, /export function setSkillMcodeEnabled\(id, name, enabled\)/);
  assert.match(skills, /cmd: 'skill_set_mcode_enabled'/);
  assert.match(skills, /\(\{ id, name, enabled \}\)/);

  const commands = readShellSource('commands.rs');
  const signature = commands.match(/pub async fn skill_set_mcode_enabled\(([\s\S]*?)\)\s*->/);
  assert.ok(signature, 'commands.rs 里必须能找到 skill_set_mcode_enabled 的签名');
  for (const param of ['id: String', 'name: String', 'enabled: bool']) {
    assert.ok(signature[1].includes(param), '命令参数缺少 ' + param);
  }
  // 命令必须真的注册过，否则真机上 ACL 静默拒、点击无反应。
  assert.match(readShellSource('lib.rs'), /commands::skill_set_mcode_enabled/);
});

test('面板给每个技能两个目标开关，MiniMax 那个按条目挂 loading', () => {
  const panel = read('ui/src/skills/SkillsPanel.vue');
  assert.match(panel, /skill\.mcode_enabled/);
  assert.match(panel, /:loading="isLoading\(skillMcodeKey\(row\.id, skill\.name\)\)"/);
  assert.match(panel, /@change="\(value\) => toggleSkillMcode\(row, skill, value\)"/);
  // DSH / MiniMax 两个标签都在行内：仅凭一个无名开关分不出它管哪个内核。
  assert.match(panel, /<span class="target-label">DSH<\/span>/);
  assert.match(panel, /<span class="target-label">MiniMax<\/span>/);
  // 没实例 / 没打开共享库总开关时置灰，避免「点了没反应」。
  assert.match(panel, /:disabled="globalBusy \|\| !mcodeTargetReady"/);
  assert.match(panel, /mcode\.view\?\.skills_linked !== false/);
});

test('渲染用的 mcodeSkillsState 必须真的 import（曾漏 import 导致技能页渲染报错）', () => {
  const panel = read('ui/src/skills/SkillsPanel.vue');
  assert.match(panel, /mcodeSkillsState\(mcode\.view\)/, '状态映射仍在用');
  assert.match(
    panel,
    /import \{[^}]*mcodeSkillsState[^}]*\} from '\.\.\/kernel\/mcode\.js'/,
    '使用了 mcodeSkillsState 却漏 import——构建不报错，真机整页渲染失败',
  );
});

test('后端把 MiniMax 目标与 DSH 启停当成两个独立开关', () => {
  const manage = readShellSource('manage.rs');
  assert.match(manage, /pub mcode_enabled: Option<bool>/);
  assert.match(manage, /fn syncs_to_mcode\(&self\) -> bool \{\s*self\.mcode_enabled\.unwrap_or\(self\.enabled\)/);
  // 切 DSH 时先钉住当时的有效值，否则「停用 DSH」会连带把 MiniMax 那份停掉。
  assert.match(manage, /if entry\.mcode_enabled\.is_none\(\) \{\s*entry\.mcode_enabled = Some\(entry\.enabled\);/);
  // 同步侧只认目标标记，不再按 DSH 的 enabled 收。
  const mcode = readShellSource('mcode.rs');
  assert.match(mcode, /filter\(\|entry\| entry\.syncs_to_mcode\(\)\)/);
  assert.doesNotMatch(mcode, /filter\(\|entry\| entry\.enabled\)/);
});

test('包版本 tag 与包名同行，长说明收进单行 Tooltip', () => {
  const panel = read('ui/src/skills/SkillsPanel.vue');
  const theme = read('ui/src/theme.css');
  assert.match(panel, /class="entity-name"[\s\S]*class="meta-version"/);
  assert.match(panel, /<el-tooltip v-if="row\.description"[\s\S]*:content="row\.description"/);
  assert.match(panel, /class="entity-desc skill-package-description"/);
  assert.match(theme, /\.entity-desc[\s\S]*white-space: nowrap;[\s\S]*text-overflow: ellipsis;/);
  assert.match(theme, /\.skill-package-description\s*\{\s*display: block;\s*margin: 6px 0 0;/);
});
