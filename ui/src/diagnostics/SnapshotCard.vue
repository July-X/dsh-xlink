<script setup>
// 环境快照卡（安全网 P0）：让用户看得见「昨天那套配置是什么」。
//
// 这张卡**只读**。恢复要等 P1 的差异预览 + 二次确认——提前放一个「一键回退」
// 会让用户在没看清将要发生什么的情况下丢配置。
//
// 三种状态必须说三句不同的话：
//   · 还没有任何快照 → 说「怎么才会产生」，而不是渲染一个空列表
//   · 有快照但从没有一次成功启动 → 说清"还不是良好证据"
//   · 文档损坏 → 如实说「读不出来」，**不能**说成「从来没有过回退点」
import { computed, onMounted } from 'vue';
import { InfoFilled, Refresh, RefreshLeft } from '@element-plus/icons-vue';
import {
  snapshotStore,
  loadSnapshots,
  previewRestore,
  reasonLabel,
  reasonHint,
  entrySummary,
  entryTimeLabel,
  headline,
} from './snapshots.js';
import { globalBusy, isLoading, withLoading } from '../shell/loading.js';

const view = computed(() => snapshotStore.view);
const entries = computed(() => (view.value && view.value.entries) || []);

// 至少有一条不是"此刻仍生效"，才值得提示"当前环境已被手工改过"。
const drifted = computed(
  () => entries.value.length > 0 && entries.value.every((entry) => !entry.isCurrent)
);

// 「回到上一个能跑起来的组合」：默认目标就是 last-known-good，不让用户在
// 一堆时间戳里挑——那正是"我昨天还能用"这句话想表达的东西。
const canRestore = computed(
  () => !!(view.value && view.value.hasLastKnownGood && view.value.lastKnownGoodId)
);

function restoreLastGood() {
  const id = view.value && view.value.lastKnownGoodId;
  if (!id) return Promise.resolve(null);
  return withLoading('snapshotPreview', () => previewRestore(id));
}

onMounted(() => {
  if (!view.value) {
    loadSnapshots();
  }
});
</script>

<template>
  <div class="card snapshot-card">
    <div class="card-head">
      <h2>
        环境回退点
        <el-tooltip placement="bottom-start" :show-after="80">
          <template #content>
            <div class="card-info-tooltip">
              每次成功启动工作台、以及你改动配置（装 / 卸插件、切内核版本、切物化模式）
              之前，桌面端都会记下「当时那套配置长什么样」：内核版本、插件集与物化模式、
              启用的技能、已应用的补丁。
              <br />
              「回到良好状态」会先把将要发生的改动列给你看，确认后才动手；动手前还会自动
              把当前环境另存一份。插件与技能只会被停用，不会被卸载或删除。
              快照里不含任何凭据或 API Key。
            </div>
          </template>
          <el-icon class="card-info-icon"><InfoFilled /></el-icon>
        </el-tooltip>
      </h2>
      <span class="snapshot-head-actions">
        <el-button
          v-if="canRestore"
          round
          size="small"
          type="primary"
          :icon="RefreshLeft"
          :loading="isLoading('snapshotPreview')"
          :disabled="globalBusy"
          title="回到最近一次被成功启动验证过的那套配置"
          @click="restoreLastGood"
        >
          回到良好状态
        </el-button>
        <el-button
          round
          size="small"
          :icon="Refresh"
          :loading="snapshotStore.loading"
          title="重新读取回退点"
          @click="loadSnapshots(true)"
        >
          刷新
        </el-button>
      </span>
    </div>

    <el-alert
      v-if="snapshotStore.warning"
      :title="snapshotStore.warning"
      type="warning"
      :closable="false"
      show-icon
      class="snapshot-warning"
    />

    <p v-if="!view" class="muted" style="margin: 0">正在读取…</p>
    <p v-else class="snapshot-headline">{{ headline(view) }}</p>

    <el-alert
      v-if="drifted"
      title="当前环境的配置与每一个回退点都不同（可能是你手工改过插件目录）。恢复时会按你确认的时刻计算差异。"
      type="info"
      :closable="false"
      show-icon
      class="snapshot-warning"
    />

    <ul v-if="entries.length" class="snapshot-list">
      <li v-for="entry in entries" :key="entry.id" class="snapshot-item">
        <div class="snapshot-item-main">
          <span class="snapshot-item-time">{{ entryTimeLabel(entry) }}</span>
          <el-tooltip placement="top" effect="dark" :content="reasonHint(entry.reason)">
            <span class="snapshot-item-reason">{{ reasonLabel(entry.reason) }}</span>
          </el-tooltip>
          <el-tag v-if="entry.lastKnownGood" type="success" size="small" effect="plain">
            启动验证过
          </el-tag>
          <el-tag v-if="entry.isCurrent" type="info" size="small" effect="plain">
            当前仍生效
          </el-tag>
        </div>
        <span class="snapshot-item-detail">{{ entrySummary(entry) }}</span>
      </li>
    </ul>
  </div>
</template>

<style scoped>
.snapshot-head-actions {
  display: inline-flex;
  align-items: center;
  gap: 8px;
}

.snapshot-headline {
  margin: 0 0 10px;
  color: var(--text-muted);
  font-size: 13px;
  line-height: 1.6;
}

.snapshot-warning {
  margin-bottom: 10px;
}

.snapshot-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.snapshot-item {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 9px 11px;
  border: 1px solid var(--border);
  border-radius: 9px;
  background: var(--bg-soft);
}

.snapshot-item-main {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}

.snapshot-item-time {
  font-size: 13px;
  color: var(--text);
}

.snapshot-item-reason {
  font-size: 12px;
  color: var(--text-muted);
  cursor: help;
}

.snapshot-item-detail {
  font-size: 12px;
  color: var(--text-muted);
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
}
</style>
