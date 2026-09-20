// 旧版数据迁移向导（dev plan §P6 step 5）+ 主窗口弹窗（commit 37d7d70）。
//
// 后端命令：
// - migration_preview / migration_rollback / migration_list
// - migration_run(policy, on_progress) — on_progress 是 Tauri Channel，
//   每完成一源 emit MigrationProgress，前端用它更新进度条 + 文字步骤
// - migration_skip_get / _set / _clear — 用户拒绝状态持久化到
//   <xlink_home>/migration-skipped.json（用户点过一次「否」后不再弹）
//
// UI 形态：
// - 主窗口 mount 后检测遗留数据 + skip 状态 → 满足条件弹 el-dialog
//   居中 modal（[MigrationPrompt.vue]），是/否二选一
// - 「数据迁移」侧栏 panel 保留作为：手动重跳 / 查历史 / rollback
//
// 保守默认（dev plan §P6）：
// - ConflictPolicy::SkipIfNewer 默认（保留用户后来修改的文件）
// - 凭据与会话不纳入首版迁移（sessions / credentials）
// - 旧源永不被删除（rollback 路径依赖）

import { reactive, computed } from 'vue';
import { invoke, makeChannel } from './bridge.js';
import { toastActionError } from './notify.js';

/** LegacySource 枚举（与后端 migration::LegacySource 对齐）。 */
export const SOURCES = ['plugins', 'skills-store', 'skills-active'];

/** 显示名（简体中文）。 */
export function sourceDisplayName(src) {
  if (src === 'plugins') return '旧插件中央库';
  if (src === 'skills-store') return '旧技能中央库';
  if (src === 'skills-active') return '旧技能活动视图';
  return src;
}

/** 备份目录 slug（与后端 migration::LegacySource::backup_key 对齐）。 */
export function sourceBackupKey(src) {
  if (src === 'plugins') return 'plugins';
  if (src === 'skills-store') return 'skills-store';
  if (src === 'skills-active') return 'skills-active';
  return src;
}

/** 冲突策略（与后端 migration::ConflictPolicy 对齐）。 */
export const CONFLICT_POLICIES = ['skip-if-newer', 'backup-and-overwrite'];

export function conflictPolicyDisplayName(p) {
  if (p === 'skip-if-newer') return '跳过更新的文件（保留用户修改）';
  if (p === 'backup-and-overwrite') return '备份旧目标后覆盖';
  return p;
}

/** 单源预览条目（与后端 MigrationItemPreview 对齐）。 */
export function emptyItem(source) {
  return {
    source,
    file_count: 0,
    total_bytes: 0,
    target_path: '',
    legacy_path: '',
    exists: false,
  };
}

/** 全局 wizard 状态。单一 store，组件实例之间共享。 */
export const migrationStore = reactive({
  // 当前 step（0-based，与 el-steps 的 active 一致）
  activeStep: 0,
  // step 1：preview 结果
  preview: null,
  hasMigratable: false,
  // step 2：用户选择
  selectedSources: new Set(SOURCES),
  conflictPolicy: 'skip-if-newer',
  // step 3：run 结果
  runResult: null,
  // step 4：rollback 入口
  rollbackInFlight: false,
  // 历史迁移列表
  history: [],
  // 当前选中历史（rollback 用）
  selectedHistoryId: '',
});

/** 主窗口 mount 弹窗专用：是否处于「用户拒绝迁移」状态。 */
export const migrationSkip = reactive({
  /** 后端 migration_skip_get 拿到的当前值。null = 还没查过。 */
  skipped: null,
});

/** 主窗口 mount 弹窗专用 store（与 MigrationPanel 的 migrationStore 分离）。 */
export const promptStore = reactive({
  /** 弹窗是否打开。 */
  open: false,
  /** 弹窗上下文：'initial' = 启动期首次提示，'re-prompt' = 用户在侧栏面板
   * 主动重跳（措辞不同：初始时强调「是否要迁移」，重跳时强调「再次询问」）。 */
  context: 'initial',
  /** 当前 migration preview（migrationStore.preview 是同一个对象的引用）。 */
  preview: null,
  /** 弹窗的子状态：'ask'（等用户选）/ 'running'（迁移中 + 进度条）/ 'done'（完成 + summary）。 */
  phase: 'ask',
  /** 'running' 期间的进度（来自 Channel onmessage）。 */
  progress: { step: 0, total: 0, sourceLabel: '', stage: '' },
  /** 'done' 后的 run result。 */
  runResult: null,
});

/** 默认重置（每次进入向导时调用）。 */
export function resetMigrationStore() {
  migrationStore.activeStep = 0;
  migrationStore.preview = null;
  migrationStore.hasMigratable = false;
  migrationStore.selectedSources = new Set(SOURCES);
  migrationStore.conflictPolicy = 'skip-if-newer';
  migrationStore.runResult = null;
  migrationStore.rollbackInFlight = false;
  // 历史与选中不动——它们是跨 session 的有用信息
}

/** Step 1：发现。调 migration_preview + migration_list。 */
export async function loadMigrationPreview() {
  try {
    const [preview, history] = await Promise.all([
      invoke('migration_preview'),
      invoke('migration_list'),
    ]);
    migrationStore.preview = preview;
    migrationStore.history = history || [];
    // has_migratable 是后端 MigrationPreview 上的方法；前端做等价计算
    migrationStore.hasMigratable =
      preview && preview.items && preview.items.some((it) => it.file_count > 0 || it.total_bytes > 0);
    return preview;
  } catch (e) {
    toastActionError('扫描旧版数据失败', e, '检查日志确认原因后重试', 6000);
    throw e;
  }
}

/** Step 3：执行迁移。`onProgress` 是 Channel<MigrationProgress> 的
 *  onmessage 回调，每完成一源触发一次——用于实时更新进度条 + 文字步骤。 */
export async function runMigration(onProgress) {
  try {
    const channel = makeChannel(onProgress || (() => {}));
    const result = await invoke('migration_run', {
      // 后端只接 policy + Channel；sources 由后端 preview 自动枚举。
      // 旧版把 sources 也当参数传了，后端命令签名在 commit 37d7d70 已简化。
      conflict_policy: migrationStore.conflictPolicy,
      on_progress: channel,
    });
    migrationStore.runResult = result;
    return result;
  } catch (e) {
    toastActionError('迁移运行失败', e, '可回滚本次迁移（backup 保留在 Xlink home），或重试', 8000);
    throw e;
  }
}

/** Step 4 / 历史列表：回滚。 */
export async function rollbackMigration(migrationId) {
  migrationStore.rollbackInFlight = true;
  try {
    try {
      const result = await invoke('migration_rollback', { migration_id: migrationId });
      // 回滚后刷新历史与 preview
      await loadMigrationPreview();
      return result;
    } catch (e) {
      toastActionError('回滚失败', e, '查看日志确认 backup 路径是否仍可访问', 8000);
      throw e;
    }
  } finally {
    migrationStore.rollbackInFlight = false;
  }
}

/** 后端查「用户是否拒绝过迁移」。失败视同 false（不弹窗但也不阻拦后续扫描）。 */
export async function loadMigrationSkip() {
  try {
    migrationSkip.skipped = await invoke('migration_skip_get');
  } catch (e) {
    migrationSkip.skipped = false;
    toastActionError('读取迁移跳过状态失败', e, '本次会按未跳过处理', 5000);
  }
  return migrationSkip.skipped;
}

/** 用户在弹窗点「否」：调后端写 skip 标记 + 前端 cache 同步。 */
export async function setMigrationSkip(sources) {
  try {
    await invoke('migration_skip_set', { sources: sources || [] });
    migrationSkip.skipped = true;
  } catch (e) {
    toastActionError('记录跳过状态失败', e, '下次启动仍会再问', 6000);
    throw e;
  }
}

/** 清除 skip 标记（用户在「数据迁移」侧栏面板手动重跳时调）。 */
export async function clearMigrationSkip() {
  try {
    await invoke('migration_skip_clear');
    migrationSkip.skipped = false;
  } catch (e) {
    toastActionError('清除跳过状态失败', e, '请稍后重试', 6000);
    throw e;
  }
}

/** 弹窗：用户选「是 → 迁移」。`sources` 是 preview 里 file_count > 0 的项。 */
export async function runPromptMigration() {
  promptStore.phase = 'running';
  promptStore.progress = { step: 0, total: 0, sourceLabel: '', stage: '准备' };
  try {
    // conflict_policy 默认走 SkipIfNewer——和原 MigrationPanel 一致；
    // 弹窗里没暴露 policy 选项（保守默认 + dev plan §6），用户拒绝策略
    // 也是「保留用户后来修改的文件」最贴近直觉。
    const result = await runMigration((progress) => {
      promptStore.progress = progress;
    });
    promptStore.phase = 'done';
    promptStore.runResult = result;
    return result;
  } catch (e) {
    promptStore.phase = 'ask';
    throw e;
  }
}

/** 弹窗：用户选「否 → 不迁移」。 */
export async function declinePromptMigration(sources) {
  await setMigrationSkip(sources);
  promptStore.open = false;
}

/** 主窗口 mount 时调用：扫 preview + 读 skip 状态，决定要不要弹。
 * 由 App.vue onMounted 在 refreshAll() 后调一次。 */
export async function maybeOpenMigrationPrompt() {
  // 后端 preview 失败（比如 data_dir 不可读）→ 不弹
  let preview;
  try {
    preview = await invoke('migration_preview');
  } catch (e) {
    return false;
  }
  migrationStore.preview = preview;
  const hasMigration =
    preview && preview.items && preview.items.some(
      (it) => it.file_count > 0 || it.total_bytes > 0
    );
  migrationStore.hasMigratable = hasMigration;
  if (!hasMigration) return false;
  // 后端 skip 状态失败 → 跟没拒绝过一样，不阻拦弹窗
  const skipped = await loadMigrationSkip();
  if (skipped) return false;
  promptStore.context = 'initial';
  promptStore.preview = preview;
  promptStore.phase = 'ask';
  promptStore.progress = { step: 0, total: 0, sourceLabel: '', stage: '' };
  promptStore.runResult = null;
  promptStore.open = true;
  return true;
}

/** 在「数据迁移」侧栏面板手动重跳。 */
export function reopenMigrationPrompt() {
  if (!migrationStore.preview || !migrationStore.hasMigratable) return;
  promptStore.context = 're-prompt';
  promptStore.preview = migrationStore.preview;
  promptStore.phase = 'ask';
  promptStore.progress = { step: 0, total: 0, sourceLabel: '', stage: '' };
  promptStore.runResult = null;
  promptStore.open = true;
}

/** 文件大小格式化（B / KiB / MiB / GiB）。 */
export function formatBytes(n) {
  if (!n) return '0 B';
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KiB`;
  if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MiB`;
  return `${(n / 1024 / 1024 / 1024).toFixed(2)} GiB`;
}

/** 是否所有选中源都已准备好（preview 已加载且至少一个可选）。 */
export const canProceedFromStep1 = computed(() => migrationStore.preview !== null);

/** Step 1 → 2：至少一个源存在且非空。 */
export const canProceedFromStep2 = computed(
  () => migrationStore.selectedSources.size > 0 && migrationStore.conflictPolicy !== ''
);