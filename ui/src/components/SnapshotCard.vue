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
import { InfoFilled, Refresh } from '@element-plus/icons-vue';
import {
  snapshotStore,
  loadSnapshots,
  reasonLabel,
  reasonHint,
  entrySummary,
  entryTimeLabel,
  headline,
} from '../snapshots.js';

const view = computed(() => snapshotStore.view);
const entries = computed(() => (view.value && view.value.entries) || []);

// 至少有一条不是"此刻仍生效"，才值得提示"当前环境已被手工改过"。
const drifted = computed(
  () => entries.value.length > 0 && entries.value.every((entry) => !entry.is_current)
);

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
              现在只能查看，还不能一键回退——回退会先给你看差异再动手，避免在没看清的
              情况下丢配置。快照里不含任何凭据或 API Key。
            </div>
          </template>
          <el-icon class="card-info-icon"><InfoFilled /></el-icon>
        </el-tooltip>
      </h2>
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
      title="当前环境的配置与每一个回退点都不同（可能是你手工改过插件目录）。回退功能上线后，恢复前会先让你确认差异。"
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
          <el-tag v-if="entry.last_known_good" type="success" size="small" effect="plain">
            启动验证过
          </el-tag>
          <el-tag v-if="entry.is_current" type="info" size="small" effect="plain">
            当前仍生效
          </el-tag>
        </div>
        <span class="snapshot-item-detail">{{ entrySummary(entry) }}</span>
      </li>
    </ul>
  </div>
</template>

<style scoped>
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
