import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import test from 'node:test';

// 内核状态诊断与概览那两个动作在**两个内核页签下都可用**（2026-11-08 用户要求：
// 切换页签只该变「当前内核」与「系统健康」两块，其余 UI 共用——「查看状态」与
// 「重新检测」属于共用卡里的动作，不该在 MiniMax 下消失）。
//
// 这几条是源码判据：漏掉一个 v-if 不会让任何行为测试变红，只会让用户切到
// MiniMax 页签后发现按钮没了。
const readSrc = (p) => readFileSync(resolve('ui/src', p), 'utf8');
const readRepo = (p) => readFileSync(resolve(p), 'utf8');

test('概览的「查看状态」与「重新检测」在两个内核页签下都渲染', () => {
  const overview = readSrc('shell/OverviewPanel.vue');

  // 「查看状态」不再挂 !isMcode 守卫，且把当前页签交给诊断页。
  assert.doesNotMatch(
    overview,
    /v-if="!isMcode"\s*\n\s*class="kernel-status-link"/,
    '「查看状态」属于共用卡里的动作，MiniMax 下也要在'
  );
  assert.match(
    overview,
    /openKernelStatusDiagnosis\(store\.activePanel, isMcode \? 'mcode' : 'dsh'\)/,
    '「查看状态」要把当前内核族交给诊断页，否则 MiniMax 下会读到 DSH 的快照'
  );

  // 「重新检测」的容器不再按内核隐藏；「自动安装」仍只属于 DSH（MiniMax 用托管 Node）。
  assert.match(overview, /<span class="metric-actions">/, '「重新检测」的容器不许按内核隐藏');
  assert.match(
    overview,
    /v-if="!isMcode && shownNode && !shownNode\.ok"/,
    '「自动安装」仍只属于 DSH：MiniMax 的 Node 由壳托管'
  );
  assert.match(
    overview,
    /if \(isMcode\.value\) \{\s*refreshMcodeNode\(\);\s*return;\s*\}/,
    'MiniMax 下的「重新检测」要重探它自己那份 Node 缓存'
  );
});

test('内核状态诊断按打开时的内核族读各自的快照', () => {
  const view = readSrc('diagnostics/KernelStatusDiagnosis.vue');
  assert.match(
    view,
    /diagnosticStore\.active\?\.family === 'mcode'/,
    '诊断页要能从打开时记下的 family 认出 MiniMax'
  );
  assert.match(view, /mcode\.view/, 'MiniMax 的快照在 mcode.view');
  assert.match(view, /mcode\.installedVersions/, 'MiniMax 的已安装版本数走它自己的列表');
  assert.match(view, /store\.view\?\.kernel/, 'DSH 的快照仍在 store.view.kernel');

  const diag = readSrc('diagnostics/diagnostics.js');
  assert.match(
    diag,
    /export function openKernelStatusDiagnosis\(sourcePanel, family = 'dsh'\)/,
    '打开动作要收 family，缺省 dsh 保住旧调用方'
  );

  const actions = readSrc('diagnostics/diagnostic-actions.js');
  assert.match(
    actions,
    /diagnosticStore\.active\?\.family === 'mcode'/,
    '刷新要按 family 分派到 MiniMax 的读取'
  );
  assert.match(actions, /refreshMcodeStatus\(\)/, 'MiniMax 的刷新走 mcode 自己的状态读取');
});

test('MiniMax 的「重新检测」先清后端解析缓存再拉状态', () => {
  const mcode = readSrc('kernel/mcode.js');
  assert.match(mcode, /export function refreshMcodeNode\(\)/);
  assert.match(mcode, /invoke\('refresh_mcode_node'\)/, '前端要调新增的清缓存命令');

  // 命令三件套（实现 / 注册 / ACL）任何一处漏掉都是真机静默失败：
  // 前端拿到 "Command not found"，按钮点了没反应。
  assert.match(
    readRepo('src-tauri/src/commands.rs'),
    /pub async fn refresh_mcode_node\(\)/,
    'Rust 侧要实现 refresh_mcode_node'
  );
  assert.match(
    readRepo('src-tauri/src/lib.rs'),
    /commands::refresh_mcode_node/,
    'generate_handler! 要注册 refresh_mcode_node'
  );
  assert.match(
    readRepo('src-tauri/permissions/app-commands.json'),
    /"refresh_mcode_node"/,
    '本地窗口的 ACL 要放行 refresh_mcode_node'
  );
});
