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
  misplacedStore,
  resetMigrationStore,
  loadMigrationPreview,
  runMigration,
  rollbackMigration,
  clearMigrationSkip,
  reopenMigrationPrompt,
  loadMisplacedScan,
  recoverMisplacedHome,
  misplacedFileCount,
  misplacedTotalBytes,
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
import { confirmDialog, toast } from '../notify.js';
import { store } from '../store.js';
import { tildePath } from '../labels.js';

const previewItems = computed(() =>
  (migrationStore.preview && migrationStore.preview.items) || []
);

// 历史列表只展示最近一次：更早的迁移极少回查，全列出来只会拉长页面。
// 后端按 mtime 倒序返回，取第一条即最近一次；旧备份目录仍完整保留在
// backups/ 下（回滚路径依赖，只是不再逐条列进 UI）。
const historyList = computed(() => (migrationStore.history || []).slice(0, 1));
const historyHiddenCount = computed(() =>
  Math.max((migrationStore.history || []).length - historyList.value.length, 0)
);

const runResultItems = computed(() => {
  if (!migrationStore.runResult) return [];
  // MigrationReport.items（字段名与 Rust MigrationItemReport 对齐）
  return migrationStore.runResult.per_source || migrationStore.runResult.items || [];
});

// 进入面板时刷新
onMounted(async () => {
  resetMigrationStore();
  await withLoading('migrationPreview', () => loadMigrationPreview());
  // 「搬错实例」的历史会话扫描：只读，且失败不打扰用户（见 migration.js
  // loadMisplacedScan 的注释）。不并进上面那个 withLoading——它有自己的
  // loading key，失败时也不会把迁移预览一起拖成失败态。
  await loadMisplacedScan();
});

// 「找回历史会话」卡片：别的实例 home 里有本实例缺的东西时才出现。
// 两类内容——会话目录（正文）与工作区注册表（工作台的会话清单来源）。
// 只搬目录不合并清单，会话就在磁盘上却永远不显示（2026-09-29 本机实测）。
const misplacedDirs = computed(() => {
  const scan = misplacedStore.scan;
  if (!scan || !Array.isArray(scan.dirs)) return [];
  return scan.dirs;
});
const misplacedWorkspaces = computed(() => {
  const scan = misplacedStore.scan;
  if (!scan || !Array.isArray(scan.workspaces)) return [];
  return scan.workspaces;
});
const misplacedVisible = computed(
  () => misplacedDirs.value.length > 0 || misplacedWorkspaces.value.length > 0
);

async function onRecoverMisplaced() {
  const scan = misplacedStore.scan;
  if (!scan) return;
  const holders = Array.from(
    new Set([
      ...misplacedDirs.value.map((d) => d.holder),
      ...misplacedWorkspaces.value.map((w) => w.holder),
    ])
  );
  const wsSessions = misplacedWorkspaces.value.reduce(
    (acc, w) => acc + (w.session_count || 0),
    0
  );
  // 恢复前的确认必须说清「会失去什么」：源侧数据**不删**，因此这里能承诺
  // 「随时可以再删一次本实例的副本退回原状」，而不是像回滚那样先备份后覆盖。
  const ok = await confirmDialog(
    '找回历史会话？',
    `把实例 ${holders.join('、')} 里的 ${misplacedFileCount(scan)} 个会话 / 附件文件` +
      `（${formatBytes(misplacedTotalBytes(scan))}）复制到本实例 ${tildePath(scan.home)}，` +
      `并把 ${wsSessions} 个会话登记进本实例的会话清单（不登记的话文件在磁盘上、工作台里也不显示）。` +
      '源目录原样保留、不删除；本实例已有的同名会话不会被覆盖。' +
      '需要先关闭工作台：工作台运行时内核会用自己的内存缓存覆盖会话清单。',
    '复制到本实例'
  );
  if (!ok) return;
  let result;
  try {
    result = await withLoading('homeRecovery', () => recoverMisplacedHome());
  } catch (e) {
    toast(`找回历史会话失败：${e && e.message ? e.message : e}。可在「数据迁移」面板重新扫描后重试`, 8000);
    return;
  }
  const failed = (result && result.failed) || [];
  if (failed.length > 0) {
    toast(`部分条目没能收编（${failed.length} 条）：${failed[0]}。其余已完成，源目录未改动`, 9000);
  } else {
    const copied = ((result && result.copied) || []).length;
    const ws = ((result && result.workspaces) || []).length;
    toast(
      `已复制 ${copied} 个条目、收编 ${ws} 个工作区到本实例；源目录未改动。重新打开工作台窗口即可看到`,
      7000
    );
  }
}

async function refreshPreview() {
  await withLoading('migrationPreview', () => loadMigrationPreview());
}

// 「完成」收尾并回到概览主界面：迁移结果留在历史列表里可随时回查 / 回滚；
// 面板经 :key 切换会卸载重挂，下次进来自然从「发现」重新扫描。
function onFinish() {
  resetMigrationStore();
  store.activePanel = 'overview';
}

// 后端给的是 epoch 秒字符串（与商店清单时间戳约定一致），这里转本地时间。
function formatHistoryTime(epochSecs) {
  if (!epochSecs) return '—';
  const date = new Date(Number(epochSecs) * 1000);
  if (Number.isNaN(date.getTime())) return '—';
  const pad = (n) => String(n).padStart(2, '0');
  return (
    `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ` +
    `${pad(date.getHours())}:${pad(date.getMinutes())}`
  );
}

async function onRun() {
  await withLoading('migrationRun', () => runMigration());
  migrationStore.activeStep = 3;
  await loadMigrationPreview();
}

async function onRollback(migrationId) {
  // 不用原生 confirm()：WKWebView 未实现 runJavaScriptConfirmPanelWithMessage
  // 时它不弹 UI 且直接返回 false，于是 macOS 上点「回滚」是纯空操作，
  // migration_rollback 根本不会发出（见 notify.js 顶部同一约束）。
  const ok = await confirmDialog(
    '回滚迁移？',
    `确认回滚迁移 ${migrationId}？回滚会从 backup 恢复目标，迁移前的旧源不会被删除。`,
    '回滚'
  );
  if (!ok) return;
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
        <p class="hint">正在扫描旧版数据…</p>
      </template>
      <template v-else-if="!migrationStore.hasMigratable">
        <p class="empty-state">未检测到旧版数据，无需迁移。</p>
      </template>
      <template v-else>
        <table class="preview-table">
          <thead>
            <tr><th>来源</th><th>旧路径</th><th>新目标</th><th>文件数</th><th>大小</th></tr>
          </thead>
          <tbody>
            <tr v-for="item in previewItems" :key="item.source">
              <td>{{ sourceDisplayName(item.source) }}</td>
              <td class="path">{{ tildePath(item.legacy_path) }}</td>
              <td class="path">{{ tildePath(item.target_path) }}</td>
              <td>{{ item.file_count }}</td>
              <td>{{ formatBytes(item.total_bytes) }}</td>
            </tr>
          </tbody>
        </table>
        <p class="hint">旧源不会被删除，可随时回滚。</p>
      </template>
      <div v-if="historyList.length > 0" class="history">
        <h3>历史迁移</h3>
        <div class="history-list">
          <div v-for="row in historyList" :key="row.migration_id" class="history-row">
            <span class="history-cell history-id" :title="row.migration_id">{{ row.migration_id }}</span>
            <span class="history-cell">{{ formatHistoryTime(row.created_at) }}</span>
            <span class="history-cell">{{ Array.isArray(row.sources) ? `${row.sources.length} 个来源` : '—' }}</span>
            <span class="history-cell history-actions">
              <el-button
                size="small"
                :icon="RefreshLeft"
                :loading="isLoading('migrationRollback') && migrationStore.rollbackInFlight"
                @click="onRollback(row.migration_id)"
              >
                回滚
              </el-button>
            </span>
          </div>
        </div>
        <p v-if="historyHiddenCount > 0" class="hint">
          仅显示最近一次迁移；更早的 {{ historyHiddenCount }} 次备份仍保留在 backups/ 目录，未删除。
        </p>
      </div>
      <div v-if="misplacedVisible" class="misplaced">
        <h3>找回历史会话</h3>
        <p class="hint">
          检测到其它实例的 home 里有本实例没有的会话数据（通常是旧版本把
          <code>~/.dsh</code> 并进了错误的实例）。复制过来<strong>并登记进本实例的会话清单</strong>后，
          重新打开工作台窗口即可看到；源目录不会被删除。
        </p>
        <ul class="misplaced-list">
          <li
            v-for="dir in misplacedDirs"
            :key="`${dir.holder}:${dir.name}`"
            class="misplaced-item"
          >
            <span class="misplaced-name">{{ dir.name }}</span>
            <span class="misplaced-meta">
              在实例 {{ dir.holder }} · {{ dir.entries.length }} 个条目 ·
              {{ dir.file_count }} 个文件 · {{ formatBytes(dir.total_bytes) }}
            </span>
          </li>
          <li
            v-for="ws in misplacedWorkspaces"
            :key="ws.holder"
            class="misplaced-item"
          >
            <span class="misplaced-name">会话清单</span>
            <span class="misplaced-meta">
              实例 {{ ws.holder }} 有 {{ ws.session_count }} 个会话未登记在本实例 ·
              {{ ws.paths.length }} 个工作区
            </span>
          </li>
        </ul>
        <footer class="step-actions">
          <el-button
            type="primary"
            :loading="isLoading('homeRecovery')"
            :icon="RefreshLeft"
            @click="onRecoverMisplaced"
          >
            复制到本实例
          </el-button>
        </footer>
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
      <!-- 不用 el-checkbox-group：EP 的 checkbox 一旦处于 group 内就走组模式，
           子项自己的 :model-value 会被忽略、点击改的是 group 的模型——而这里
           的选中态是 Set（migrationStore.selectedSources），不适合硬套数组
           v-model。独立 checkbox 走受控用法（:model-value + @change）即可。 -->
      <div class="source-options">
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
      </div>

      <h3>冲突策略</h3>
      <el-radio-group v-model="migrationStore.conflictPolicy">
        <el-radio v-for="p in CONFLICT_POLICIES" :key="p" :value="p">
          {{ conflictPolicyDisplayName(p) }}
        </el-radio>
      </el-radio-group>

      <div class="credentials-note">
        <p><strong>说明</strong>：凭据与会话<strong>不</strong>纳入上面这套旧布局迁移（那套处理的是「上一代壳留下的目录」）。如果历史会话是落在<strong>另一个实例</strong>的 home 里，请用「发现」步里的「找回历史会话」卡片——它走的是只复制、不删除的回收路径。</p>
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
            <code>{{ tildePath(migrationStore.preview && migrationStore.preview.xlink_home) }}/backups/&lt;migration_id&gt;/</code>
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
    <div v-show="migrationStore.activeStep === 3" class="step-body finish-body">
      <template v-if="!migrationStore.runResult">
        <p class="empty-state">尚未运行迁移。</p>
      </template>
      <template v-else>
        <div class="result-summary">
          <strong>数据已迁移</strong>
          <span class="hint">
            迁移 ID：<code>{{ migrationStore.runResult.migration_id }}</code> ·
            已迁移 {{ runResultItems.length }} 个来源。
          </span>
        </div>
        <table v-if="runResultItems.length > 0" class="preview-table">
          <thead>
            <tr><th>来源</th><th>已迁移</th><th>跳过</th><th>备份</th></tr>
          </thead>
          <tbody>
            <tr v-for="item in runResultItems" :key="item.source">
              <td>{{ sourceDisplayName(item.source) }}</td>
              <td>{{ item.files_copied ?? 0 }}</td>
              <!-- 跳过 = SkipIfNewer 下保留目标侧更新内容的条目；迁移报告
                   没有「冲突」计数字段，原列读 conflicts 恒为空 -->
              <td>{{ item.files_skipped ?? 0 }}</td>
              <td class="path">{{ tildePath(item.backup_path) || '—' }}</td>
            </tr>
          </tbody>
        </table>
      </template>
      <footer class="step-actions">
        <el-button @click="prev">上一步</el-button>
        <el-button type="primary" @click="onFinish">
          完成
        </el-button>
      </footer>
    </div>
  </section>
</template>

<style scoped>
/* 紧凑布局：向导是一次性流程，页内留白按工具页收紧，减少纵向滚动。
   ElSteps / el-empty / el-result / el-table 都自带较多垂直留白，
   在 480×1600 的窄窗口里加起来会触发滚动条；这里能省的省掉，
   不能省的（必须保留可读性的标题、按钮行）就只压 padding。 */
.panel { padding: 12px 16px; }
.migration header h2 { margin: 0; font-size: 18px; }
.subtitle { color: var(--text-muted); margin: 2px 0 0; font-size: 12.5px; }
.step-body { margin-top: 10px; }
.step-body h3 { margin: 10px 0 6px; font-size: 13px; }
.step-actions { margin-top: 12px; display: flex; gap: 8px; justify-content: flex-end; }
.preview-table { width: 100%; border-collapse: collapse; margin: 6px 0; font-size: 12px; }
.preview-table th, .preview-table td { padding: 4px 8px; text-align: left; border-bottom: 1px solid var(--border); }
.preview-table td { vertical-align: top; }
.preview-table .path { font-family: ui-monospace, monospace; font-size: 11.5px; color: var(--text-muted); }
.hint { color: var(--text-muted); font-size: 12px; margin: 6px 0 0; }
.empty-state {
  margin: 16px 0;
  padding: 18px 16px;
  border: 1px dashed var(--border);
  border-radius: 8px;
  text-align: center;
  color: var(--text-muted);
  font-size: 12.5px;
  background: rgba(255, 255, 255, 0.015);
}
.result-summary {
  display: flex;
  align-items: baseline;
  flex-wrap: wrap;
  gap: 6px 12px;
  margin: 6px 0 8px;
  padding: 8px 12px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: rgba(255, 255, 255, 0.02);
}
.result-summary .hint { margin: 0; }
.history { margin-top: 16px; }
.history h3 { margin: 0 0 6px; font-size: 13px; }
/* 「找回历史会话」卡片：与迁移主体是两条独立路径（一个搬旧布局目录，一个把
   误入他处的会话复制回来），所以用一块独立底色而不是塞进 preview-table——
   混在一张表里会让人以为点「下一步」也会把它一起搬。 */
.misplaced {
  margin-top: 16px;
  padding: 10px 12px;
  border: 1px solid var(--border);
  border-left: 3px solid var(--accent);
  border-radius: 6px;
  background: var(--surface-soft);
}
.misplaced h3 { margin: 0 0 6px; font-size: 13px; }
.misplaced-list { list-style: none; margin: 0 0 8px; padding: 0; }
.misplaced-item {
  display: flex;
  justify-content: space-between;
  align-items: baseline;
  gap: 12px;
  padding: 2px 0;
  font-size: 12px;
}
.misplaced-name { font-weight: 600; }
.misplaced-meta { color: var(--text-muted); font-variant-numeric: tabular-nums; }
/* 历史迁移本来用 el-table，但表头 + 单元格 padding 在窄列里把整行顶到 36px+
   高，且自带一些不能改的垂直留白。改用 grid 布局直接控制紧凑度。 */
.history-list {
  border: 1px solid var(--border);
  border-radius: 6px;
  overflow: hidden;
}
.history-row {
  display: grid;
  grid-template-columns: minmax(0, 1.4fr) minmax(0, 1fr) minmax(0, 0.8fr) auto;
  align-items: center;
  gap: 12px;
  padding: 4px 10px;
  font-size: 12px;
}
.history-row + .history-row {
  border-top: 1px solid var(--border);
}
.history-cell {
  min-width: 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  color: var(--text-muted);
}
.history-id {
  font-family: ui-monospace, monospace;
  color: var(--text);
}
.history-actions {
  display: flex;
  justify-content: flex-end;
}
.credentials-note { margin-top: 12px; padding: 8px 12px; background: var(--surface-soft); border-radius: 6px; color: var(--text-muted); font-size: 12px; }
/* 来源勾选纵向排布（原先靠 el-checkbox-group 的布局习惯，去掉 group 后自己排） */
.source-options { display: flex; flex-direction: column; gap: 4px; margin-top: 6px; }
/* ElSteps 的 ::v-deep 收紧：simple 模式默认步骤块高度约 40px+，这里压到 ~28px */
.migration :deep(.el-step__head) { margin-bottom: 2px; }
.migration :deep(.el-step__title) { font-size: 13px; }
.migration :deep(.el-step__description) { font-size: 11.5px; }
.migration :deep(.el-step.is-simple .el-step__arrow) { margin: 0 8px; }
</style>
