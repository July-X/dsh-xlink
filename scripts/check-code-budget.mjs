#!/usr/bin/env node
/**
 * 代码膨胀门禁：把「代码在变胖」变成 CI 里看得见的失败，而不是半年后的一次
 * 大扫除。
 *
 * 用法：
 *   node scripts/check-code-budget.mjs            # 校验，超预算以非零退出码结束
 *   node scripts/check-code-budget.mjs --report   # 只打印度量，不判失败
 *
 * 两类度量：
 *
 *   1. **生产代码行数**（按文件）。刻意排除 `#[cfg(test)] mod ...` 整块：测试
 *      该长就长，让测试行数掩盖生产代码的膨胀才是问题。新模块要给新预算，
 *      改预算必须和代码出现在同一个提交里——这就是「要么拆，要么显式承认」。
 *
 *   2. **重复块**。归一化每行（去空白、去注释、丢掉纯括号行）后取 10 行滑窗，
 *      把命中 ≥2 次的窗口按文件并成「最长重复区间」，只统计长度 ≥12 行的区间。
 *      滑窗会互相重叠（一个 20 行的复制粘贴会产生 11 个窗口），所以必须先合并
 *      再计数，否则指标会随块长线性放大、没法当门禁。指标是**重复区间个数**
 *      （每份拷贝各算一处）：跨模块逐字复制的样板都在这个尺度上，而
 *      `let x = 1;` 这类单行巧合不会。
 *
 * 阈值取当前值 + 余量：目标是拦住持续增长，不是要求立刻删代码。真要涨，
 * 在同一个提交里改这里的数字，让 review 看到代价。
 */

import { readFileSync, readdirSync, statSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const reportOnly = process.argv.includes('--report');

/** 生产代码行数预算：文件 → 上限。包含注释以外的所有代码行。 */
const FILE_BUDGETS = {
  'ui/src/theme.css': 2960,
  // P4 step 3：物化路径切到实例 extensions/plugins/<id>/，抽出 materialize_inner
  // 共享逻辑、新增 materialize_one_for_instance / remove_materialized_for_instance
  // / sweep_instance_orphans / default_instance_key / seed_default_instance_for_tests
  // 等 helper + is_managed_spec 兼容新旧两种路径模式。净增约 30 行。
  // P4 step 4：4 条实例范围顶层函数 install_for_instance / update_for_instance /
  // uninstall_for_instance / set_mode_for_instance / sync_for_instance /
  // status_for_instance + sync_kernels_for_instance / ensure_wiring_for_instance
  // 共约 +235 行（install_unlocked / uninstall_unlocked / set_mode_unlocked /
  // update_unlocked / sync_all_unlocked 接收 family/instance_id 形参，
  // 旧 API 委托到新 API，加上 step 3 注释 / 测试 setup helper 的尾段）。
  // 物化/卸载/同步/状态/模式切换是同一概念的同步代码，留在同一文件比
  // 拆出去更易维护。
  'src-tauri/src/plugins.rs': 2980,
  // P4 step 4：4 条实例范围 Tauri 命令 plugin_install_instance /
  // plugin_uninstall_instance / plugin_sync_instance / plugin_status_instance
  // + 共享 run_plugin_command_instance 主体，约 +75 行。
  // B 类日志 family/instance_id 接入：install_version / GuardDeps literal /
  // kernel_workbench_url_from_log / diagnose_runtime 等 6+ caller 加 family +
  // instance_id 形参（约 +28 行）。每个 caller 都同时给默认实例硬编码
  // (DSH, "default")——P8 UI 决策后由真实 instance_id 替换。
  'src-tauri/src/commands.rs': 1900,
  // P6 step 2+3+4：迁移向导后端——ConflictPolicy / MigrationStatus /
  // MigrationItemReport / MigrationReport / run_migration / migrate_one /
  // decide_entry / backup_existing / copy_one / copy_tree_inner +
  // RollbackStatus / RollbackItemReport / RollbackReport / rollback_migration
  // / restore_directory / find_backup_root + 12 个新测试，约 +600 行。
  // copy_tree_inner 与 plugins.rs / skills.rs 的 copy_tree 是已知重复；
  // AGENTS.md §3 要求提共享层到 pkg.rs，留到独立重构处理。
  // 740 → 800：store.json 清单按 id 合并（EntryDecision::MergeManifest +
  // is_store_manifest + merge_store_manifest）+ 合并回归测试 + 共享 env
  // 守卫 scoped_xlink_home_unset 的调用方改造。合并语义是修复「目标清单
  // 被泄漏数据顶新导致源记录永远迁不进来」的根因，逻辑必须留在迁移层。
  'src-tauri/src/migration.rs': 800,
  'src-tauri/src/skills.rs': 1490,
  'src-tauri/src/patches.rs': 1250,
  // B 类日志 family/instance_id 接入：kernel_log_spec / install_log_spec /
  // current_kernel_log_path / install_version / install_version_into 加形参；
  // attach_log_drainers 改用 family + id。start() legacy 单实例路径硬编码
  // (DSH, "default")——P8 UI 决策后由真实 instance_id 替换。约 +11 行。
  'src-tauri/src/kernel.rs': 1290,
  'src-tauri/src/process.rs': 1180,
  // 1050 → 1090：会话标题改为订阅 `session/control`（baseline 播种 + 标题投影帧
  // 保鲜 + 老内核退回 session/list 快照），这部分逻辑与 Center 同生共死，拆出去
  // 只会把状态机切成两半。详见 docs/notification-design.md §3.3。
  'src-tauri/src/notify.rs': 1090,
  'src-tauri/src/guard.rs': 940,
  'ui/src/store.js': 430,
  // 多内核改造 P0：新路径模块（paths.rs）。包含 ShellMode、xlink_home、shell
  // /kernels/skills/state/cache 解析、legacy resolver、id 校验与基础数据
  // 模型——是后续 P2–P8 的依赖根，必须单独占预算，避免被 plugins/skills
  // 这两个大文件吞噬。
  'src-tauri/src/paths.rs': 600,
  // P2：实例注册表 + 锁 + 端口分配 + runtime/pid 文件读写 + DSH home
  // 子目录创建 + 默认实例迁移钩子。约 470 行（含 11 个测试 setup 与
  // 路径解析注释）。
  'src-tauri/src/instance.rs': 470,
  // P3：KernelAdapter trait + AdapterCapabilities + DshAdapter 首实现
  // （DSH_HOME / DSH_PROFILE 注入、profile/package.json 与 cordis.patch.yml
  // 模板、resolve_install_dir 双查找）。约 430 行（含 9 个测试）。
  'src-tauri/src/kernel_adapter.rs': 620,
};
/** 全部受检文件的合计预算（Tauri 生产代码 + 前端 js/vue/css）。 */
// 20400 → 20500：技能面板接线「启用 / 停用单个技能」（skill_set_enabled 此前只有
// 后端实现，面板从未调用过：弹层 + 开关 + 动作 + 样式约 30 行），加上归因口径抽到
// ui/src/incidents.js 共享层。两处都是新增能力而非复制粘贴，重复区间数仍为 3。
// 20500 → 20560：下载校验改成 fail-closed 并回退老 packument 的 shasum（sha1）——
// 新增 sha1 回退分支、`strongest_integrity` 抽取与判据注释。安全策略收紧带来的
// 行数是必要的，不该为了卡预算而少写一条校验路径。
// 20560 → 21060：新增 paths.rs（500 行）+ settings.rs 拆分 shell-aware / legacy
// 双入口（≈ 60 行）+ commands.rs 暴露 shell_mode（≈ 5 行）+ store.js 加
// shellMode 计算属性（≈ 4 行）。多内核改造 P0+P1 的最小可用代码量。
// 21060 → 21660：P2 落地 instance 模块（注册表 / 锁 / 端口 / runtime /
// 迁移钩子，约 470 行）+ commands.rs 增加 8 条实例命令与 InstanceSummary
// 类型（≈ 180 行）+ kernel.rs 新增 InstanceStartReport 与
// start_instance / stop_instance / set_instance_active_version 接口
// （≈ 50 行）。路径解析都在 paths.rs，重复区间数仍为 4。
// 21660 → 22160：P3 新增 kernel_adapter.rs（约 430 行）+ kernel.rs
// start_instance 切到 DshAdapter（约 30 行）。所有 DSH 专有路径
// （DSH_HOME / profiles/<name>/ / cordis.patch.yml）现在只出现在
// kernel_adapter.rs，不再散落在 kernel.rs / commands.rs 里。
// 22160 → 22190：P4 step 3 物化路径切到实例 extensions/plugins/<id>/，
// 在 plugins.rs 内新增 materialize_inner / materialize_one_for_instance /
// remove_materialized_for_instance / sweep_instance_orphans 等共享 helper
// （约 +30 行）。物化与清扫是同一概念的同步代码，留在同一文件比拆出去
// 更易维护；FILE_BUDGETS 里 plugins.rs 也对应上调 30 行。
// 22190 → 22500：P4 step 4 新增 6 条实例范围顶层函数（install_for_instance /
// update_for_instance / uninstall_for_instance / set_mode_for_instance /
// sync_for_instance / status_for_instance）+ sync_kernels_for_instance /
// ensure_wiring_for_instance（约 +235 行落在 plugins.rs）；commands.rs
// 增加 4 条实例范围 Tauri 命令 + run_plugin_command_instance 共享主体
// （约 +75 行）。总计 +310 行。
// 22500 → 23070：P6 step 2+3+4 + P7 + P5 step 3 + copy_tree 共享。
// P6 step 2+3+4 新增 migration.rs（≈ 660 行）+ lib.rs ENV_LOCK 拆分 +
// scoped_dsh_home（≈ 60 行新增）+ commands.rs 增加 4 条迁移向导命令
// （约 +35 行）。P5 step 3 KernelAdapter::custom_skill_dirs 接口预留 +
// DshAdapter 返回 paths::skills_active_root() + start 注入
// DSH_CUSTOM_SKILL_DIRS env + ENV_PATH_SEP 常量 + 2 个测试，约 +91 行
// 落在 kernel_adapter.rs。P7 McodeAdapter mock + KERNEL_FAMILY_MCODE
// 常量 + 8 个测试，约 +100 行同样落在 kernel_adapter.rs。copy_tree_inner
// 与 plugins.rs / skills.rs 的 copy_tree 是已知重复——AGENTS.md §3 要求
// 提到 pkg.rs，留到独立重构处理；FILE_BUDGETS 里 kernel_adapter.rs /
// migration.rs / commands.rs 也对应上调。
// B 类日志 family/instance_id 接入全链路（commit 158ced3）:
// commands.rs +28 + kernel.rs +11 + guard.rs（kernel_log_path 重构 +
// GuardDeps 加字段 + diagnose_runtime 加形参）+ kernel_adapter.rs +2 +
// notify.rs +24 ≈ +89 行；测试 fixture 调整不计入生产预算但同样生效。
// 下次 reset 预算时考虑把日志相关 caller 提到单独 helper（参考 AGENTS.md
// §3 要求），避免散落到 5 个 module 的硬编码 (DSH, "default")。
//
// P6 step 5 迁移向导 UI（commit ...）：ui/src/migration.js (~150 行) +
// ui/src/components/MigrationPanel.vue (~250 行) + App.vue / SideBar.vue
// 接入 ~10 行 ≈ +410 行（实际 +260 是因为 UI 行的预算口径不计模板 style
// 块里的 CSS——纯 <template> + <script setup> + state 加 invoke 包装）。
// P8 #1 顶部实例 dropdown（commit 25cd376）：ui/src/instance.js (~50 行)
// + WindowTitleBar.vue 注入 chip + dropdown script/template 块（计入
// theme.css；vue 模板不计入）+ theme.css 实例 chip / menu 样式 ~95 行
// （+15 落在 theme.css 预算边沿）+ document-level click 关闭菜单 +
// aria-haspopup / aria-expanded / aria-current 标注。23500 → 23800。
//
// P8 #2 PluginRow per-instance 视图（commit ...）：plugins.rs 加
// `PluginInstanceState` 结构（materialized / actual_mode / synced /
// wired / quarantined 五个字段）+ PluginRow.instances: BTreeMap<id,
// PluginInstanceState> + `read_profile_json_for_instance` helper +
// status_for_instance 内部循环 instance::load_registry() 各实例（~+65
// 行）+ 3 个回归测试（~+150 行）。plugins.rs 预算 2915 → 2980（+65）。
// 总预算 23800 → 24050（+250，叠加 +65 与给后续 P8 #2 UI 留 buffer）。
//
// 历史数据迁移 UX 改造（commit 37d7d70 / eb2afb8）：migration.rs 加
// `MigrationProgress` 结构 + `run_migration_with_progress<F>` 闭包版 +
// `MigrationSkip` 结构 + is_migration_skipped / set_migration_skipped /
// clear_migration_skipped 三个 helper（~+107 行）。migration.rs 预算
// 670 → 740（+70）。总预算 24050 → 24400（+350 给后续 UI 改造留 buffer）。
//
// dev 复测 4 项 UX 修复（commit ...）：kernel.rs `data_dir` 切到
// `xlink_home + desktop[-dev]/` + 删旧 SHELL_SUBDIR 常量（~+50）；
// instance.rs 加 `ensure_default_registered` sync helper（~+50）。总
// 预算 24400 → 24500（+100 buffer）。
const TOTAL_BUDGET = 24500;
/** 重复区间数上限。 */
const DUPLICATE_BUDGET = 6;
/** 归一化滑窗宽度。 */
const WINDOW = 10;
/** 计入重复区间的最小长度（归一化行数）。 */
const MIN_SPAN = 12;

const failures = [];
const notes = [];

function walk(dir, suffixes, skip = new Set(['node_modules', 'dist', 'target', '.git'])) {
  const out = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (skip.has(entry.name)) continue;
    const full = join(dir, entry.name);
    if (entry.isDirectory()) out.push(...walk(full, suffixes, skip));
    else if (suffixes.some((suffix) => entry.name.endsWith(suffix))) out.push(full);
  }
  return out;
}

const show = (path) => relative(root, path).split('\\').join('/');

/// 去掉 Rust 的测试模块整块（`#[cfg(test)]` 与 `#[cfg(all(test, …))]` 都算）。
function stripRustTests(text) {
  const lines = text.split('\n');
  const kept = [];
  for (let i = 0; i < lines.length; i += 1) {
    if (!/^#\[cfg\(.*\btest\b.*\)\]$/.test(lines[i].trim())) {
      kept.push(lines[i]);
      continue;
    }
    // 跳过属性行 + 紧跟的 mod 块（花括号配对；字符串里的括号不参与计数——
    // 这里的近似只会让「多跳过几行」，不会漏掉测试块）。
    let depth = 0;
    let seen = false;
    for (i += 1; i < lines.length; i += 1) {
      const code = lines[i].replace(/\/\/.*$/, '');
      depth += (code.match(/\{/g) || []).length - (code.match(/\}/g) || []).length;
      if (code.includes('{')) seen = true;
      if (seen && depth <= 0) break;
    }
  }
  return kept.join('\n');
}

/// 归一化后仍然「有信息量」的行：注释、空行、纯括号行都不算。
function significantLines(text) {
  const out = [];
  text
    .replace(/\/\*[\s\S]*?\*\//g, '')
    .split('\n')
    .forEach((raw, index) => {
      const line = raw
        .replace(/\/\/.*$/, '')
        .replace(/\s+/g, ' ')
        .trim();
      if (!line) return;
      if (/^[{}()[\];,]+$/.test(line)) return;
      if (/^<\/?[a-z-]+>$/.test(line)) return;
      // `line` 是原文件行号：重复块报告要能直接跳到那一行。
      out.push({ text: line, line: index + 1 });
    });
  return out;
}

/// 只算「代码行」：空行与纯注释行不计入预算，否则文档写得越多，指标越像膨胀。
function codeLineCount(text) {
  let inBlock = false;
  let count = 0;
  for (const raw of text.split('\n')) {
    let line = raw;
    if (inBlock) {
      const end = line.indexOf('*/');
      if (end === -1) continue;
      line = line.slice(end + 2);
      inBlock = false;
    }
    const start = line.indexOf('/*');
    if (start !== -1 && line.indexOf('*/', start) === -1) {
      inBlock = true;
      line = line.slice(0, start);
    }
    const trimmed = line.replace(/\/\/.*$/, '').trim();
    if (trimmed) count += 1;
  }
  return count;
}

// --- 1. 生产代码行数 ---------------------------------------------------------

const rustFiles = walk(join(root, 'src-tauri/src'), ['.rs']);
const uiFiles = walk(join(root, 'ui/src'), ['.js', '.vue', '.css']);
const files = [...rustFiles, ...uiFiles].sort();

let total = 0;
const sizes = [];
for (const file of files) {
  const raw = readFileSync(file, 'utf8');
  const text = file.endsWith('.rs') ? stripRustTests(raw) : raw;
  const count = codeLineCount(text);
  sizes.push([show(file), count]);
  total += count;
}

for (const [path, budget] of Object.entries(FILE_BUDGETS)) {
  const hit = sizes.find(([name]) => name === path);
  if (!hit) {
    failures.push(`[预算] ${path} 不存在（文件被移动或改名？请同步 FILE_BUDGETS）`);
    continue;
  }
  if (hit[1] > budget) {
    failures.push(
      `[预算] ${path} 生产代码 ${hit[1]} 行，超过预算 ${budget} 行：请拆分模块，` +
        `或在同一个提交里上调 FILE_BUDGETS 里的数字`
    );
  }
}
if (total > TOTAL_BUDGET) {
  failures.push(
    `[预算] 生产代码合计 ${total} 行，超过总预算 ${TOTAL_BUDGET} 行：` +
      `新增能力应当优先复用既有模块（pkg.rs / state.rs / async.js 是既有的共享层）`
  );
}
notes.push(`生产代码合计 ${total} 行（预算 ${TOTAL_BUDGET}）`);

// --- 2. 重复块 ---------------------------------------------------------------

const linesByFile = new Map();
const keyToHits = new Map();
for (const file of files) {
  const raw = readFileSync(file, 'utf8');
  const text = file.endsWith('.rs') ? stripRustTests(raw) : raw;
  const lines = significantLines(text);
  linesByFile.set(show(file), lines);
  for (let i = 0; i + WINDOW <= lines.length; i += 1) {
    const key = lines
      .slice(i, i + WINDOW)
      .map((entry) => entry.text)
      .join('\n');
    const hits = keyToHits.get(key);
    const hit = { file: show(file), start: i, line: lines[i].line };
    if (hits) hits.push(hit);
    else keyToHits.set(key, [hit]);
  }
}

const at = (file, start) => {
  const lines = linesByFile.get(file);
  if (start < 0 || start + WINDOW > lines.length) return null;
  return lines
    .slice(start, start + WINDOW)
    .map((entry) => entry.text)
    .join('\n');
};

// 把「出现 ≥2 次的窗口」按出现位置向后延伸，得到最长重复块。直接数窗口是不行
// 的：一个 30 行的复制粘贴会产生 21 个互相重叠的命中窗口，指标会随块长线性放大。
const covered = new Set();
const duplicateSamples = [];
let duplicates = 0;
for (const hits of keyToHits.values()) {
  if (hits.length < 2) continue;
  if (hits.some((hit) => covered.has(`${hit.file}:${hit.start}`))) continue;
  let extra = 0;
  for (;;) {
    const next = hits.map((hit) => at(hit.file, hit.start + extra + 1));
    if (next.some((key) => key === null)) break;
    if (!next.every((key) => key === next[0])) break;
    extra += 1;
  }
  for (const hit of hits) {
    for (let step = 0; step <= extra; step += 1) covered.add(`${hit.file}:${hit.start + step}`);
  }
  const length = WINDOW + extra;
  if (length < MIN_SPAN) continue;
  duplicates += 1;
  if (duplicateSamples.length < 20) {
    duplicateSamples.push(
      `${hits[0].file}:${hits[0].line}（${length} 行 × ${hits.length} 份）`
    );
  }
}

notes.push(`重复区间（≥${MIN_SPAN} 行归一化代码）${duplicates} 处（预算 ${DUPLICATE_BUDGET}）`);
if (duplicates > DUPLICATE_BUDGET) {
  failures.push(
    `[重复] 发现 ${duplicates} 处重复区间，超过预算 ${DUPLICATE_BUDGET}：` +
      `请把共同部分提到共享层（Rust: pkg.rs / state.rs；前端: async.js）\n` +
      duplicateSamples.map((sample) => `      ${sample}`).join('\n')
  );
}

// --- 输出 -------------------------------------------------------------------

const biggest = sizes.sort((a, b) => b[1] - a[1]).slice(0, 6);
console.log('生产代码最大的几个文件：');
for (const [path, count] of biggest) {
  const budget = FILE_BUDGETS[path];
  console.log(`  ${String(count).padStart(5)} 行  ${path}${budget ? `（预算 ${budget}）` : ''}`);
}
for (const note of notes) console.log(`• ${note}`);

if (reportOnly) {
  if (duplicateSamples.length) {
    console.log('重复区间（前几处）：');
    for (const sample of duplicateSamples) console.log(`  ${sample}`);
  }
  console.log('\n（--report：只度量，不判失败）');
  process.exit(0);
}
if (failures.length) {
  console.error('');
  for (const failure of failures) console.error(failure);
  console.error('\n代码膨胀门禁未通过。');
  process.exit(1);
}
console.log('\n代码膨胀门禁通过。');
