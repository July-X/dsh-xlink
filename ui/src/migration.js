// 旧版数据迁移向导（dev plan §P6 step 5）—— 后端 4 条命令已可用：
// migration_preview / migration_run / migration_rollback / migration_list
// （commit b24e68e / 89d76df / 9615901 / 5416bca）。
//
// UI 形态：嵌入式向导页（侧栏新 panel），4 步 el-steps：
// 发现 → 选择 → 运行 → 完成/回滚。
//
// 保守默认（dev plan §P6）：
// - ConflictPolicy::SkipIfNewer 默认（保留用户后来修改的文件）
// - 凭据与会话不纳入首版迁移（sessions / credentials）
// - 旧源永不被删除（rollback 路径依赖）

import { reactive, computed } from 'vue';
import { invoke } from './bridge.js';
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

/** Step 3：执行迁移。 */
export async function runMigration() {
  const sources = Array.from(migrationStore.selectedSources);
  try {
    const result = await invoke('migration_run', {
      sources,
      conflict_policy: migrationStore.conflictPolicy,
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