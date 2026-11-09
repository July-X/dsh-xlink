import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

// T03：设置页的「MiniMax 内核体验」入口开关（docs/ui/multi-kernel-ui-development-plan.md
// §1–§3）。四件事都会静默坏，所以各有判据：
//   ① 开关是**用户偏好**，不是构建旗子：实际显示条件是「构建可用 ∧ 用户已选择」，
//      构建不含能力时禁用并说明，不能把已保存的偏好显示成能力可用；
//   ② 关闭前按真实运行 / 授权状态确认；读不出来时不许显示成「已停止 / 无授权」；
//   ③ 入口隐藏时 MiniMax 端口组整组不提交，隐藏前的草稿要留到重新开启；
//   ④ 两套端口（DSH / MiniMax）的保存结果各说各的，不合并成一条含糊提示。
//
// 判据扫模板（HTML 注释已剥）与脚本（JS 注释已剥）：本仓模板带大段解释「为什么
// 这么写」的中文注释，注释里常原样引用被判据的词（ui/AGENTS.md「判据怎么写」）。

const panel = readFileSync('ui/src/shell/SettingsPanel.vue', 'utf8');

function templateOf(src) {
  const start = src.indexOf('<template>');
  assert.ok(start >= 0, '找不到 <template>');
  const styleAt = src.indexOf('<style', start);
  return src.slice(start, styleAt > start ? styleAt : src.length).replace(/<!--[\s\S]*?-->/g, '');
}

function styleOf(src) {
  const start = src.indexOf('<style');
  assert.ok(start >= 0, '找不到 <style');
  return src.slice(start, src.indexOf('</style>', start)).replace(/\/\*[\s\S]*?\*\//g, '');
}

function scriptOf(src) {
  const start = src.indexOf('<script');
  assert.ok(start >= 0, '找不到 <script');
  return src
    .slice(start, src.indexOf('</script>', start))
    .replace(/\/\*[\s\S]*?\*\//g, '')
    .replace(/^\s*\/\/.*$/gm, '');
}

/** 取一个具名函数的函数体（花括号配对；这两个函数体里没有字符串花括号）。 */
function functionBody(script, name) {
  const at = script.search(new RegExp('(?:async\\s+)?function\\s+' + name + '\\s*\\('));
  assert.ok(at >= 0, '找不到 ' + name);
  const open = script.indexOf('{', at);
  let depth = 0;
  for (let i = open; i < script.length; i += 1) {
    if (script[i] === '{') depth += 1;
    else if (script[i] === '}') {
      depth -= 1;
      if (depth === 0) return script.slice(open + 1, i);
    }
  }
  throw new Error(name + ' 的函数体没有闭合');
}

const tpl = templateOf(panel);
const script = scriptOf(panel);
const style = styleOf(panel);

/** 体验开关行：必须落在工作台卡的表单里、端口分组之前。 */
function experienceRow(sourceTpl = tpl) {
  const formAt = sourceTpl.indexOf('class="settings-form settings-ports-form"');
  const rowAt = sourceTpl.indexOf('class="settings-experience"');
  const dshAt = sourceTpl.indexOf('<fieldset class="settings-port-group">');
  assert.ok(formAt > 0, '找不到工作台端口表单');
  assert.ok(rowAt > formAt, '体验开关行必须放在工作台卡的端口表单里');
  assert.ok(dshAt > rowAt, '体验开关行必须落在端口 fieldset 之前（设计稿的位置）');
  return sourceTpl.slice(rowAt, dshAt);
}

/** 关闭确认的接线：确认存在 / 状态未知 / 取消三条路各走各的。 */
function assertCloseContract(scriptText) {
  const guard = functionBody(scriptText, 'experienceCloseDetail');
  assert.match(guard, /await refreshMcodeStatus\(\)/, '关闭前先拿最新运行状态，不用旧轮询值');
  assert.match(
    guard,
    /if \(mcode\.error\) unknown = true;\s*else if \(mcode\.view\?\.running\) active = true;/,
    '运行状态读失败算未知；读成功才按 running 判「仍在运行」',
  );
  assert.match(
    guard,
    /const module = await invoke\('memory_module_status'\);/,
    '模块授权状态要整份读回来：enabled 与 warning 都是「仍可能生效」的事实',
  );
  assert.match(
    guard,
    /if \(module\?\.enabled \|\| module\?\.warning\) active = true;/,
    '模块授权 enabled，或授权落盘但实例 / profile 已失配（warning），都算仍可能生效',
  );
  assert.match(guard, /invoke\('list_all_memory_shares'\)/, '逐条共享授权也要查');
  assert.match(guard, /shares\.length > 0\) active = true;/, '有任何一条共享授权就确认');
  const activeAt = guard.indexOf('if (active)');
  const unknownAt = guard.indexOf('if (unknown)');
  assert.ok(activeAt > -1 && unknownAt > activeAt,
    '确认存在优先于未知：只要有一项读出来为真，就按「仍可能生效」说');
  assert.equal((guard.match(/unknown = true/g) || []).length, 3,
    '三个状态来源（运行 / 模块授权 / 逐条授权）读不出来都要落到 unknown');
  assert.match(guard, /if \(unknown\) return EXPERIENCE_CLOSE_BASE \+ EXPERIENCE_CLOSE_UNKNOWN;/,
    '读不出状态时必须按「无法确认」提示，不能当作已停止 / 无授权直接放行');
  assert.match(guard, /return null;/, '确认没在跑也没有共享授权时才直接隐藏（不弹框）');

  const toggle = functionBody(scriptText, 'onToggleExperience');
  assert.match(toggle, /const detail = await experienceCloseDetail\(\);/);
  assert.match(
    toggle,
    /if \(detail && !\(await confirmDialog\(EXPERIENCE_CLOSE_TITLE, detail, '仅隐藏入口'\)\)\) return;/,
    '关闭要先确认；按钮文案是「仅隐藏入口」，不是「停止」或「删除」',
  );
  const confirmAt = toggle.indexOf('confirmDialog');
  const saveAt = toggle.indexOf('setMcodeExperience');
  assert.ok(confirmAt > -1 && saveAt > confirmAt, '确认通过才保存偏好');
  assert.match(toggle.slice(confirmAt, saveAt), /return;/,
    '取消（confirmDialog 返回 false）直接返回，不改状态');

  assert.match(scriptText, /const EXPERIENCE_CLOSE_TITLE = '关闭 MiniMax 体验入口？'/, '确认框标题');
  assert.match(scriptText, /不会停止工作台，也不会删除数据或撤销已有授权/, '关闭只隐藏入口，正文要说清');
  assert.match(scriptText, /MiniMax 工作台仍在运行，已授权的共享记忆仍可能被使用/, '运行 / 授权仍在时的下一句');
  assert.match(scriptText, /无法确认当前状态，仍可能生效/, '状态读不出来时的下一句');
}

/** 隐藏端口组 + 不提交隐藏字段 + 草稿保留。 */
function assertHiddenPorts(tplText, scriptText) {
  assert.match(
    tplText,
    /<fieldset v-if="mcodeExperience" class="settings-port-group settings-port-group--experimental">/,
    '体验关闭时 MiniMax 端口组必须 v-if 掉',
  );
  const save = functionBody(scriptText, 'onSavePorts');
  assert.match(save, /mcodeExperience\.value &&/,
    '隐藏期间「保存配置」不得提交 MiniMax 端口');
  // 体验关闭时端口组 v-if 掉，草稿还在：watch 只记录已保存值，不回填；开启后按
  // 逐字段 dirty 回填（无草稿的字段必须接受首个快照——原实现的焦点保护会漏掉它）。
  assert.match(
    scriptText,
    /watch\(\s*\[\(\) => mcode\.view, mcodeExperience\][\s\S]*?savedMcodePorts\.value = [\s\S]*?if \(!experience\) return;[\s\S]*?adoptDraft\(mcodePort, mcodePortDirty, view\.port\);[\s\S]*?adoptDraft\(mcodeMiniappPort, mcodeMiniappDirty, view\.miniapp_port\);/,
    '体验关闭时不用轮询值覆盖草稿；开启后按 dirty 回填',
  );
  assert.doesNotMatch(scriptText, /editing/, '焦点不再是草稿保护机制');
}

test('体验开关行：名称 + 实验 chip + ⓘ + 开关，落在端口分组之前', () => {
  const row = experienceRow();
  assert.match(row, /<span class="minimax-wordmark">MiniMax<\/span> 内核体验/,
    '「MiniMax」用手写体类挂在词本身，中文说明仍是正文字族');
  assert.match(row, /class="origin-chip settings-experience-chip">实验</,
    '「实验」chip 复用来源 chip 的现有形状');
  assert.match(row, /class="card-info-icon" tabindex="0" :aria-label="EXPERIENCE_NOTE"/,
    'ⓘ 必须可聚焦，可访问名与 tooltip 内容取同一常量');
  assert.match(row, /<template #content>[\s\S]*?\{\{ EXPERIENCE_NOTE \}\}/,
    'tooltip 内容也读同一个常量');
  assert.match(row, /:model-value="mcodeExperience"/, '开关读 experience.js 的组合判据，不读原始偏好');
  assert.match(row, /aria-label="MiniMax 内核体验"/, '开关要有可访问名');
  assert.match(row, /@change="onToggleExperience"/, 'el-switch 派发新值');
  // 只做「入口显示」这一件事：不自动安装 / 启动 / 停止，也不动授权。
  assert.match(script, /const EXPERIENCE_NOTE = '只控制 MiniMax 相关入口的显示，不自动安装、启动或停止内核，也不修改授权。'/);
  // 「实验」chip 的颜色走实验档，组件里不另拼手写体字族。
  const rule = /\.settings-experience-chip\s*\{([^}]*)\}/.exec(style);
  assert.ok(rule, '缺少 .settings-experience-chip 规则');
  assert.match(rule[1], /color: var\(--experimental\)/);
  assert.match(rule[1], /background: var\(--experimental-fill\)/);
  assert.match(rule[1], /border-color: var\(--experimental\)/);
  assert.doesNotMatch(style, /font-hand/, '手写体只由 theme.css 的 .minimax-wordmark 提供');
});

test('构建能力与用户偏好分开判断，设置页不自己读构建旗子', () => {
  assert.match(
    script,
    /import \{ MCODE_BUILD_AVAILABLE, mcodeExperience, setMcodeExperience \} from '\.\.\/kernel\/experience\.js'/,
    '可见性判据只有 experience.js 一份',
  );
  // 直接读壳设置里的原始偏好会把「构建不含能力」显示成用户已启用，所以面板只认组合值。
  assert.doesNotMatch(script, /mcode_experience/, '设置页不许直接读 settings.mcode_experience');
  assert.doesNotMatch(script, /import\.meta\.env/, '构建旗子不在面板里再判一次');
  assert.doesNotMatch(tpl, /mcodeExperimental/, '旧的构建常量不许残留');
  // 构建不含能力：禁用并说明，已保存的偏好原样保留（开关读的就是组合值，不会覆写偏好）。
  const row = experienceRow();
  assert.match(row, /v-if="!MCODE_BUILD_AVAILABLE"[\s\S]*?当前版本不可用/,
    '构建不含能力时显示「当前版本不可用」');
  assert.match(row, /:disabled="!MCODE_BUILD_AVAILABLE \|\| globalBusy"/,
    '构建不含能力时开关禁用');
});

test('开关即时保存、不经过「保存配置」，失败时弹回 store 真值', () => {
  const toggle = functionBody(script, 'onToggleExperience');
  assert.match(toggle, /withLoading\('mcodeExperience'/, '在途状态用现有 loading key');
  assert.match(toggle, /await setMcodeExperience\(enabled\)/, '即时保存走 experience.js');
  assert.doesNotMatch(toggle, /saveSettings|saveMcodePorts/, '体验开关与端口表单是两套保存');
  assert.match(tpl, /:loading="isLoading\('mcodeExperience'\)"/, '保存中开关转圈');
  // 开关读 store 真值，本地不另存乐观值：保存失败时 setMcodeExperience 不回显，
  // 开关自然停在原值（它自己已提示失败）。
  assert.doesNotMatch(script, /mcodeExperience\.value\s*=/, '不许在本地改写开关状态');
  const save = functionBody(script, 'onSavePorts');
  assert.doesNotMatch(save, /setMcodeExperience/, '「保存配置」不许捎带写体验偏好');
});

test('关闭前确认：运行中 / 有共享授权 / 状态未知各有正文，取消不改状态', () => {
  assertCloseContract(script);
});

test('隐藏 MiniMax 端口组：保存配置不提交隐藏字段，草稿留到重新开启', () => {
  assertHiddenPorts(tpl, script);
});

test('两套端口各存各的、各自反馈，不合并成一条含糊提示', () => {
  const save = functionBody(script, 'onSavePorts');
  assert.match(save, /saveSettings\(port\.value, profile\.value\)/, 'DSH 一条命令');
  assert.match(save, /saveMcodePorts\(mcodePort\.value, mcodeMiniappPort\.value\)/, 'MiniMax 另一条命令');
  // 成功 / 失败提示由两条命令各自发出（saveSettings / saveMcodePorts），面板不另拼
  // 一条「保存失败」把一半成功也描述成没保存。
  assert.doesNotMatch(save, /toast/, '面板不合并提示');
});

test('判据能抓住真实回退（反向验证）', () => {
  // ① 摘掉关闭确认：运行中会直接隐藏入口。
  const noConfirm = script.replace(/\s*if \(detail && !\(await confirmDialog[\s\S]*?return;\n/, '\n');
  assert.notEqual(noConfirm, script, '反向验证必须改到真实确认分支');
  assert.throws(() => assertCloseContract(noConfirm), /关闭要先确认/);

  // ② 状态读不出来时当成「没在跑、没授权」直接放行。
  const noUnknown = script.replace(
    'if (unknown) return EXPERIENCE_CLOSE_BASE + EXPERIENCE_CLOSE_UNKNOWN;',
    'if (unknown) return null;',
  );
  assert.notEqual(noUnknown, script, '反向验证必须改到真实未知分支');
  assert.throws(() => assertCloseContract(noUnknown), /读不出状态时必须按「无法确认」提示/);

  // ③ 端口组不再按体验隐藏。
  const alwaysShown = tpl.replace('<fieldset v-if="mcodeExperience"', '<fieldset');
  assert.notEqual(alwaysShown, tpl, '反向验证必须改到真实端口组');
  assert.throws(() => assertHiddenPorts(alwaysShown, script), /体验关闭时 MiniMax 端口组必须 v-if 掉/);

  // ④ 隐藏期间「保存配置」仍提交 MiniMax 端口。
  const submitsHidden = script.replace(/\s*mcodeExperience\.value &&\n/, '\n');
  assert.notEqual(submitsHidden, script, '反向验证必须改到真实保存条件');
  assert.throws(() => assertHiddenPorts(tpl, submitsHidden), /不得提交 MiniMax 端口/);
});
