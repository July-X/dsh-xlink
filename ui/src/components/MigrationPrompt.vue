<script setup>
// 主窗口 mount 后弹窗：检测到遗留数据 + 用户未拒绝过时弹出，让用户主动
// 决定是否迁移。改写后走 el-dialog 居中 modal + el-progress 实时进度条
// + 步骤文字，不再依赖 setup() 自动跑 reconcile。
//
// 三个 phase：
// - ask：等用户选（是/否）
// - running：migration_run 进行中，progress 来自 Channel onmessage
// - done：迁移完成，显示 summary + 关闭入口）
import { computed } from 'vue';
import {
  promptStore,
  maybeOpenMigrationPrompt,
  reopenMigrationPrompt,
  runPromptMigration,
  declinePromptMigration,
  sourceDisplayName,
  formatBytes,
} from '../migration.js';
import { toast } from '../notify.js';

const dialogVisible = computed({
  get: () => promptStore.open,
  set: (v) => {
    if (!v) promptStore.open = false;
  },
});

// 「是/否二选一」的源列表：仅显示有内容的源（file_count > 0 或 bytes > 0），
// 让用户看到弹窗就知道「这次会搬哪些」——预览做在弹窗里就够了，不再开侧栏
// panel 重复一遍（MigrationPanel 仍然保留作为「手动重跳 / 查历史 / 回滚」
// 的入口）。
const itemsWithContent = computed(() => {
  const items = promptStore.preview && promptStore.preview.items;
  if (!items) return [];
  return items.filter((it) => it.file_count > 0 || it.total_bytes > 0);
});

const totalFiles = computed(() =>
  itemsWithContent.value.reduce((acc, it) => acc + (it.file_count || 0), 0)
);
const totalBytes = computed(() =>
  itemsWithContent.value.reduce((acc, it) => acc + (it.total_bytes || 0), 0)
);

// 进度百分比：progress.total > 0 时算 (step / total) * 100；step 从 1 开始
// （0 = 准备阶段，进度条仍显示 0% 直到第一项开始）。
const progressPercent = computed(() => {
  const p = promptStore.progress;
  if (!p || !p.total) return 0;
  if (p.step === 0) return 0;
  // 完成态走 100%，最后一源 stage='已完成' 时 step 已等于 total。
  return Math.min(100, Math.round((p.step / p.total) * 100));
});

// 已完成项数 + 当前项文字（用于"已完成 X / Y · 当前：xxx"）。
const doneCount = computed(() => {
  const p = promptStore.progress;
  if (!p || !p.total || p.step === 0) return 0;
  // stage='已完成' / '已跳过' / '失败' 都算一格 step 推进；step 已经是
  // 当前正在处理的（1-based），所以「已完成」= step - 1 + (stage 是结束态 ? 1 : 0)。
  const finished = p.stage === '已完成' || p.stage === '已跳过' || p.stage === '失败';
  return finished ? p.step : Math.max(0, p.step - 1);
});

async function onConfirm() {
  try {
    await runPromptMigration();
  } catch (e) {
    // toastActionError 已经在 runMigration 内部打过；弹窗保留在 ask 阶段
    // 让用户可以重试。
    promptStore.phase = 'ask';
  }
}

async function onDecline() {
  try {
    const sources = itemsWithContent.value.map((it) => it.source);
    await declinePromptMigration(sources);
    toast('已记住本次选择，不再询问。后续可在「数据迁移」侧栏面板手动重跳', 5000);
  } catch (e) {
    // setMigrationSkip 内部已经 toast；保留弹窗让用户再试
  }
}

function onDialogClose() {
  // 弹窗关闭：仅在 ask / done 阶段允许。running 阶段用户不能 X 掉（避免
  // 半完成态造成用户认知混乱）；el-dialog :before-close 也走这里。
  if (promptStore.phase === 'running') return false;
  promptStore.open = false;
  return true;
}

defineExpose({ maybeOpenMigrationPrompt, reopenMigrationPrompt });
</script>

<template>
  <el-dialog
    v-model="dialogVisible"
    :show-close="promptStore.phase !== 'running'"
    :close-on-click-modal="promptStore.phase !== 'running'"
    :close-on-press-escape="promptStore.phase !== 'running'"
    :before-close="onDialogClose"
    width="520"
    align-center
    :title="promptStore.context === 're-prompt' ? '再次询问：是否迁移旧版数据' : '检测到旧版数据，是否迁移'"
    class="migration-prompt-dialog"
  >
    <!-- ask 阶段：源列表 + 是/否 -->
    <div v-if="promptStore.phase === 'ask'" class="prompt-ask">
      <p class="prompt-intro">
        这次启动扫描到旧版布局（上一代 dsh 桌面壳留下的数据）里有可迁移内容，
        选「是」会按<strong>保留用户修改</strong>的策略逐源搬到当前 Xlink
        数据目录；旧源永不被删除，可在「数据迁移」侧栏面板里随时回滚。
      </p>
      <ul class="prompt-sources">
        <li
          v-for="it in itemsWithContent"
          :key="it.source"
          class="prompt-source-item"
        >
          <span class="prompt-source-name">{{ sourceDisplayName(it.source) }}</span>
          <span class="prompt-source-meta">
            {{ it.file_count }} 个文件 · {{ formatBytes(it.total_bytes) }}
          </span>
        </li>
      </ul>
      <p class="prompt-summary">
        共 <strong>{{ itemsWithContent.length }}</strong> 个来源 · {{ totalFiles }} 个文件 · {{ formatBytes(totalBytes) }}
      </p>
    </div>

    <!-- running 阶段：进度条 + 当前步骤文字 -->
    <div v-else-if="promptStore.phase === 'running'" class="prompt-running">
      <el-progress
        :percentage="progressPercent"
        :status="progressPercent === 100 ? 'success' : ''"
        :stroke-width="14"
        :text-inside="true"
      />
      <p class="prompt-current-step">
        <template v-if="promptStore.progress.sourceLabel">
          第 {{ doneCount + 1 }} / {{ promptStore.progress.total }} 步 ·
          <strong>{{ promptStore.progress.sourceLabel }}</strong> · {{ promptStore.progress.stage }}
        </template>
        <template v-else>准备中…</template>
      </p>
      <ul v-if="promptStore.preview && promptStore.preview.items" class="prompt-step-list">
        <li
          v-for="(it, idx) in promptStore.preview.items"
          :key="it.source"
          class="prompt-step-item"
          :class="{
            'is-done': idx < doneCount,
            'is-current': idx === doneCount,
          }"
        >
          <span class="prompt-step-dot"></span>
          <span class="prompt-step-name">{{ sourceDisplayName(it.source) }}</span>
        </li>
      </ul>
    </div>

    <!-- done 阶段：summary + 关闭按钮 -->
    <div v-else-if="promptStore.phase === 'done'" class="prompt-done">
      <p class="prompt-done-intro">迁移完成。</p>
      <ul v-if="promptStore.runResult && promptStore.runResult.items" class="prompt-done-list">
        <li
          v-for="it in promptStore.runResult.items"
          :key="it.source"
          class="prompt-done-item"
        >
          <span class="prompt-done-name">{{ sourceDisplayName(it.source) }}</span>
          <span class="prompt-done-status">
            <template v-if="it.status === 'copied'">已完成 · {{ it.files_copied }} 个文件</template>
            <template v-else-if="it.status === 'skipped'">已跳过（源为空）</template>
            <template v-else-if="it.status === 'partial-failure'">部分失败 · {{ it.error || '查看日志' }}</template>
            <template v-else>失败 · {{ it.error || '查看日志' }}</template>
          </span>
        </li>
      </ul>
    </div>

    <template #footer>
      <div v-if="promptStore.phase === 'ask'" class="prompt-footer-ask">
        <el-button @click="onDecline">否，重新配置</el-button>
        <el-button type="primary" :loading="false" @click="onConfirm">是，开始迁移</el-button>
      </div>
      <div v-else-if="promptStore.phase === 'done'" class="prompt-footer-done">
        <el-button type="primary" @click="promptStore.open = false">完成</el-button>
      </div>
    </template>
  </el-dialog>
</template>

<style scoped>
.prompt-intro {
  margin: 0 0 14px;
  font-size: 13px;
  line-height: 1.6;
  color: var(--text, rgba(247, 255, 238, 0.86));
}
.prompt-intro strong {
  color: var(--accent, rgba(238, 255, 216, 0.96));
}
.prompt-sources {
  list-style: none;
  margin: 0 0 12px;
  padding: 0;
  border: 1px solid var(--border, rgba(238, 255, 216, 0.18));
  border-radius: 6px;
  overflow: hidden;
}
.prompt-source-item {
  display: flex;
  justify-content: space-between;
  align-items: baseline;
  gap: 12px;
  padding: 8px 12px;
  border-bottom: 1px dashed var(--border, rgba(238, 255, 216, 0.12));
  font-size: 12.5px;
}
.prompt-source-item:last-child {
  border-bottom: 0;
}
.prompt-source-name {
  font-weight: 600;
  color: var(--text, rgba(247, 255, 238, 0.92));
}
.prompt-source-meta {
  font-variant-numeric: tabular-nums;
  color: var(--muted, rgba(247, 255, 238, 0.6));
}
.prompt-summary {
  margin: 0;
  font-size: 12px;
  color: var(--muted, rgba(247, 255, 238, 0.62));
}

.prompt-current-step {
  margin: 12px 0 14px;
  font-size: 12.5px;
  color: var(--text, rgba(247, 255, 238, 0.86));
}
.prompt-current-step strong {
  color: var(--accent, rgba(238, 255, 216, 0.96));
}

.prompt-step-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.prompt-step-item {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  color: var(--muted, rgba(247, 255, 238, 0.55));
}
.prompt-step-item.is-current {
  color: var(--text, rgba(247, 255, 238, 0.92));
  font-weight: 600;
}
.prompt-step-item.is-done {
  color: var(--muted, rgba(247, 255, 238, 0.72));
}
.prompt-step-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: rgba(238, 255, 216, 0.32);
  flex: 0 0 8px;
}
.prompt-step-item.is-done .prompt-step-dot {
  background: rgba(140, 220, 120, 0.85);
}
.prompt-step-item.is-current .prompt-step-dot {
  background: rgba(239, 255, 218, 0.96);
  box-shadow: 0 0 6px rgba(239, 255, 218, 0.6);
}

.prompt-done-intro {
  margin: 0 0 12px;
  font-size: 13px;
  color: var(--text, rgba(247, 255, 238, 0.92));
}
.prompt-done-list {
  list-style: none;
  margin: 0;
  padding: 0;
  border: 1px solid var(--border, rgba(238, 255, 216, 0.18));
  border-radius: 6px;
  overflow: hidden;
}
.prompt-done-item {
  display: flex;
  justify-content: space-between;
  align-items: baseline;
  gap: 12px;
  padding: 8px 12px;
  border-bottom: 1px dashed var(--border, rgba(238, 255, 216, 0.12));
  font-size: 12.5px;
}
.prompt-done-item:last-child {
  border-bottom: 0;
}
.prompt-done-name {
  font-weight: 600;
  color: var(--text, rgba(247, 255, 238, 0.92));
}
.prompt-done-status {
  color: var(--muted, rgba(247, 255, 238, 0.7));
}

.prompt-footer-ask,
.prompt-footer-done {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}
</style>