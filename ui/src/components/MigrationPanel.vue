<script setup>
// 旧版数据迁移向导（dev plan §P6 step 5）+ 主动询问入口（commit 37d7d70）。
//
// 4 步 el-steps：发现 → 选择 → 运行 → 完成 / 回滚
// 后端命令已可用：migration_preview / migration_run(policy, on_progress Channel)
// / migration_rollback / migration_list / migration_skip_{get,set,clear}。
//
// 与 [MigrationPrompt.vue]（主窗口 mount 弹窗）并存：弹窗负责首次发现
// 提示，面板负责反复操作；面板 step-actions 加「再次询问迁移」按钮让
// 拒绝过的用户也能主动重跳。
//
// 保守默认：ConflictPolicy::SkipIfNewer + 凭据与会话不纳入 + 旧源永不删除
import { computed, onMounted, ref, watch } from 'vue';
import { ArrowDown, Check, Refresh, RefreshLeft } from '@element-plus/icons-vue';
import {
  migrationStore,
  resetMigrationStore,
  loadMigrationPreview,
  runMigration,
  rollbackMigration,
  clearMigrationSkip,
  reopenMigrationPrompt,
  sourceDisplayName,
  sourceBackupKey,
  conflictPolicyDisplayName,
  formatBytes,
  canProceedFromStep1,
  canProceedFromStep2,
  CONFLICT_POLICIES,
  SOURCES,
} from '../migration.js';
import { globalBusy, isLoading, withLoading } from '../loading.js';

const previewItems = computed(() =>
  (migrationStore.preview && migrationStore.preview.items) || []
);

const historyList = computed(() => migrationStore.history || []);

const runResultItems = computed(() => {
  if (!migrationStore.runResult) return [];
  // MigrationReport.per_source items（per_source 字段名约定）
  return migrationStore.runResult.per_source || migrationStore.runResult.items || [];
});

// 进入面板时刷新
onMounted(async () => {
  resetMigrationStore();
  await withLoading('migrationPreview', () => loadMigrationPreview());
});

async function refreshPreview() {
  await withLoading('migrationPreview', () => loadMigrationPreview());
}

async function onRun() {
  await withLoading('migrationRun', () => runMigration());
  migrationStore.activeStep = 3;
  await loadMigrationPreview();
}

async function onRollback(migrationId) {
  if (!confirm(`确认回滚迁移 ${migrationId}？回滚会从 backup 恢复目标，迁移前的旧源不会被删除。`)) {
    return;
  }
  await withLoading('migrationRollback', () => rollbackMigration(migrationId));
}

function next() {
  if (migrationStore.activeStep < 3) migrationStore.activeStep += 1;
}
function prev() {
  if (migrationStore.activeStep > 0) migrationStore.activeStep -= 1;
}

// 用户在「数据迁移」面板主动重跳：先清掉之前的 skip 标记（如果有），
// 再触发弹窗。MigrationPrompt 的 promptStore.open = true 之后弹窗就
// 走 ask / running / done 三阶段，跟首次启动的提示同一条路径。
async function onReopenPrompt() {
  await clearMigrationSkip();
  if (!migrationStore.preview) {
    await loadMigrationPreview();
  }
  if (!migrationStore.hasMigratable) {
    // 没有可迁移内容时给个轻提示，不开弹窗
    return;
  }
  reopenMigrationPrompt();
}

function toggleSource(src) {
  const set = migrationStore.selectedSources;
  if (set.has(src)) set.delete(src);
  else set.add(src);
  // 触发响应式更新（Set 本身不是深响应）
  migrationStore.selectedSources = new Set(set);
}
</script>

<template>
  <section class="panel migration">
    <header>
      <h2>数据迁移</h2>
      <p class="subtitle">把旧版 dsh-xlink 的插件 / 技能导入多实例布局。旧源不会被删除，可随时回滚。</p>
    </header>

    <el-steps :active="migrationStore.activeStep" finish-status="success" simple>
      <el-step title="发现" description="扫描旧版数据" />
      <el-step title="选择" description="选源 + 冲突策略" />
      <el-step title="运行" description="复制 + 备份" />
      <el-step title="完成" description="结果 + 回滚入口" />
    </el-steps>

    <!-- Step 0：发现 -->
    <div v-show="migrationStore.activeStep === 0" class="step-body">
      <template v-if="!migrationStore.preview">
        <p>正在扫描旧版数据…</p>
      </template>
      <template v-else-if="!migrationStore.hasMigratable">
        <el-empty description="未检测到旧版数据，无需迁移。" />
      </template>
      <template v-else>
        <table class="preview-table">
          <thead>
            <tr><th>来源</th><th>旧路径</th><th>新目标</th><th>文件数</th><th>大小</th></tr>
          </thead>
          <tbody>
            <tr v-for="item in previewItems" :key="item.source">
              <td>{{ sourceDisplayName(item.source) }}</td>
              <td class="path">{{ item.legacy_path }}</td>
              <td class="path">{{ item.target_path }}</td>
              <td>{{ item.file_count }}</td>
              <td>{{ formatBytes(item.total_bytes) }}</td>
            </tr>
          </tbody>
        </table>
        <p class="hint">旧源不会被删除，可随时回滚。</p>
      </template>
      <div v-if="historyList.length > 0" class="history">
        <h3>历史迁移</h3>
        <el-table :data="historyList" stripe>
          <el-table-column prop="migration_id" label="ID" />
          <el-table-column prop="started_at" label="开始时间" />
          <el-table-column prop="status" label="状态" />
          <el-table-column label="操作">
            <template #default="scope">
              <el-button
                v-if="scope && scope.row"
                size="small"
                :icon="RefreshLeft"
                :loading="isLoading('migrationRollback') && migrationStore.rollbackInFlight"
                @click="onRollback(scope.row.migration_id)"
              >
                回滚
              </el-button>
            </template>
          </el-table-column>
        </el-table>
      </div>
      <footer class="step-actions">
        <el-button @click="refreshPreview" :icon="Refresh" :loading="isLoading('migrationPreview')">
          重新扫描
        </el-button>
        <el-button @click="onReopenPrompt">
          再次询问迁移
        </el-button>
        <el-button
          type="primary"
          :disabled="!canProceedFromStep1 || !migrationStore.hasMigratable"
          @click="next"
        >
          下一步
        </el-button>
      </footer>
    </div>

    <!-- Step 1：选择 -->
    <div v-show="migrationStore.activeStep === 1" class="step-body">
      <h3>选择来源</h3>
      <el-checkbox-group>
        <el-checkbox
          v-for="item in previewItems"
          :key="item.source"
          :model-value="migrationStore.selectedSources.has(item.source)"
          :disabled="item.file_count === 0"
          @change="toggleSource(item.source)"
        >
          {{ sourceDisplayName(item.source) }}
          <small>（{{ item.file_count }} 个文件，{{ formatBytes(item.total_bytes) }}）</small>
        </el-checkbox>
      </el-checkbox-group>

      <h3>冲突策略</h3>
      <el-radio-group v-model="migrationStore.conflictPolicy">
        <el-radio v-for="p in CONFLICT_POLICIES" :key="p" :value="p">
          {{ conflictPolicyDisplayName(p) }}
        </el-radio>
      </el-radio-group>

      <div class="credentials-note">
        <p><strong>说明</strong>：凭据与会话<strong>不</strong>纳入首版迁移。如需迁移，参考内核升级指南手动复制。</p>
      </div>

      <footer class="step-actions">
        <el-button @click="prev">上一步</el-button>
        <el-button type="primary" :disabled="!canProceedFromStep2" @click="next">
          下一步
        </el-button>
      </footer>
    </div>

    <!-- Step 2：运行 -->
    <div v-show="migrationStore.activeStep === 2" class="step-body">
      <h3>准备运行</h3>
      <p>
        将复制
        <strong>{{ migrationStore.selectedSources.size }}</strong>
        个来源到多实例布局，先备份到
        <code>{{ migrationStore.preview && migrationStore.preview.xlink_home }}/backups/&lt;migration_id&gt;/</code>
        再覆盖目标，旧源不会被删除。
      </p>
      <p>冲突策略：{{ conflictPolicyDisplayName(migrationStore.conflictPolicy) }}</p>
      <footer class="step-actions">
        <el-button @click="prev">上一步</el-button>
        <el-button
          type="primary"
          :icon="ArrowDown"
          :loading="isLoading('migrationRun')"
          @click="onRun"
        >
          开始迁移
        </el-button>
      </footer>
    </div>

    <!-- Step 3：完成 -->
    <div v-show="migrationStore.activeStep === 3" class="step-body">
      <template v-if="!migrationStore.runResult">
        <el-empty description="尚未运行迁移。" />
      </template>
      <template v-else>
        <el-result icon="success" title="数据已迁移">
          <template #sub-title>
            <p>
              迁移 ID：<code>{{ migrationStore.runResult.migration_id }}</code><br />
              已迁移 {{ runResultItems.length }} 个来源。
            </p>
          </template>
        </el-result>
        <table v-if="runResultItems.length > 0" class="preview-table">
          <thead>
            <tr><th>来源</th><th>已迁移</th><th>备份</th><th>冲突</th></tr>
          </thead>
          <tbody>
            <tr v-for="item in runResultItems" :key="item.source">
              <td>{{ sourceDisplayName(item.source) }}</td>
              <td>{{ item.copied_files || '—' }}</td>
              <td class="path">{{ item.backup_path || '—' }}</td>
              <td>{{ item.conflicts || 0 }}</td>
            </tr>
          </tbody>
        </table>
      </template>
      <footer class="step-actions">
        <el-button @click="prev">上一步</el-button>
        <el-button @click="resetMigrationStore(); migrationStore.activeStep = 0">
          完成
        </el-button>
      </footer>
    </div>
  </section>
</template>

<style scoped>
.panel { padding: 16px 20px; }
.subtitle { color: var(--text-muted); margin-top: 4px; }
.step-body { margin-top: 24px; }
.step-actions { margin-top: 24px; display: flex; gap: 8px; justify-content: flex-end; }
.preview-table { width: 100%; border-collapse: collapse; margin: 12px 0; }
.preview-table th, .preview-table td { padding: 8px 12px; text-align: left; border-bottom: 1px solid var(--border); }
.preview-table .path { font-family: ui-monospace, monospace; font-size: 12px; color: var(--text-muted); }
.hint { color: var(--text-muted); font-size: 13px; }
.history { margin-top: 32px; }
.history h3 { margin-bottom: 12px; }
.credentials-note { margin-top: 24px; padding: 12px 16px; background: var(--surface-soft); border-radius: 6px; color: var(--text-muted); }
</style>
