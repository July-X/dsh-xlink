<!--
  会话下钻弹窗（P1）：一条会话的注入 / 请求 / 压缩节点时间线，**只读、无正文、无标题**。

  数据来自后端 memory_effect_session（合同 §2.6），本组件只做展示与重试；状态与命令
  都在 effect.js 的 sessionStore / loadSessionTimeline。写法照 MemoryDetail / 
  MemoryCandidateDetail（同屏三个弹窗必须像同一个东西）：append-to-body、固定宽度、
  点遮罩不关（阅读用的弹层，误关要重新点一次），Esc 由 el-dialog 默认处理。
-->
<script setup>
import { computed, onBeforeUnmount, onMounted } from 'vue';
import { InfoFilled } from '@element-plus/icons-vue';
import {
  formatBytes,
  formatCount,
  formatTokens,
  loadSessionTimeline,
  resetSessionTimeline,
  sessionStore,
  shortSessionId,
  timestampShort,
} from './effect.js';

const props = defineProps({
  /// 内核会话 id（session-...）。必须原始 id：短串只是显示，命令要完整值。
  sessionId: { type: String, required: true },
});
const emit = defineEmits(['close']);

const loading = computed(() => sessionStore.loading);
const error = computed(() => sessionStore.error);
const timeline = computed(() => sessionStore.timeline);
const note = computed(() => (timeline.value && timeline.value.note) || '只读；不显示任何正文。');
const shortId = computed(() => shortSessionId(props.sessionId));

const NODE_KIND_LABELS = { injection: '注入', request: '请求', compaction: '压缩' };

/** 回合 / 步：插件侧的 pre-step 事件 step 为 -1，显示成「—」而不是「第 -1 步」。 */
function turnText(turn) {
  const value = Number(turn);
  return Number.isFinite(value) && value >= 0 ? '第 ' + value + ' 回合' : '回合 —';
}

function stepText(step) {
  const value = Number(step);
  return Number.isFinite(value) && value >= 0 ? '第 ' + value + ' 步' : '';
}

/** 一个节点压成一行「表头 + 明细」：明细按 kind 取该角色真正有的字段。 */
function nodeRows(node, index) {
  const kind = String((node && node.kind) || '');
  const exposure = node && node.exposure === true;
  const rows = [];
  if (kind === 'injection') {
    rows.push('ids ' + formatCount(node && node.ids) + ' 条');
    rows.push(formatBytes(node && node.bytes));
  } else if (kind === 'request') {
    rows.push('输入 ' + formatTokens(node && node.input) + ' / 输出 ' + formatTokens(node && node.output));
    rows.push('缓存读 ' + formatTokens(node && node.cache_read));
    rows.push('工具 ' + formatCount(node && node.tool_calls) + ' 次 / 错误 ' + formatCount(node && node.tool_errors));
  } else if (kind === 'compaction') {
    const reason = node && node.reason ? String(node.reason) : '';
    rows.push('压缩 / 剪枝' + (reason ? '：' + reason : ''));
  }
  if (node && node.reason && kind !== 'compaction') rows.push('原因 ' + node.reason);
  return {
    key: (node && node.at ? String(node.at) : 'node') + '-' + index,
    kind,
    kindLabel: NODE_KIND_LABELS[kind] || (kind || '—'),
    time: timestampShort(node && node.at),
    turn: turnText(node && node.turn),
    step: stepText(node && node.step),
    // 压缩节点本身没有「是否暴露」的语义，给「—」而不是假的「未暴露」。
    exposureText: kind === 'compaction' ? '—' : exposure ? '暴露' : '未暴露',
    exposure,
    detail: rows.join(' · '),
  };
}

const rows = computed(() => {
  const nodes = timeline.value && Array.isArray(timeline.value.nodes) ? timeline.value.nodes : [];
  return nodes.map(nodeRows);
});

onMounted(() => {
  // 面板点开时已经发过命令；这里兜底（直接挂载 / 面板忘了加载时不会永远空着）。
  if (!timeline.value || sessionStore.sessionId !== props.sessionId) {
    loadSessionTimeline(props.sessionId);
  }
});

// 关掉就清空：下次打开另一个会话时不许先闪一眼上一个会话的节点。
onBeforeUnmount(resetSessionTimeline);

function retry() {
  loadSessionTimeline(props.sessionId, { force: true });
}
</script>

<template>
  <el-dialog
    class="memory-dialog effect-session"
    :model-value="true"
    :title="'会话下钻 · ' + shortId"
    width="min(640px, 92vw)"
    append-to-body
    :close-on-click-modal="false"
    @update:model-value="(open) => !open && emit('close')"
  >
    <div class="effect-session__head">
      <p class="effect-session__note">
        {{ note }}
        <el-tooltip placement="bottom-start" :show-after="80">
          <template #content>
            <div class="card-info-tooltip">
              节点来自会话日志只读解析：注入 = 记忆段进入快照；请求 = 带 usage 的模型请求，暴露表示
              该请求上下文里有有效记忆快照；压缩 = compaction / prune。不返回任何正文或标题。
            </div>
          </template>
          <el-icon class="card-info-icon" tabindex="0" aria-label="下钻节点的口径">
            <InfoFilled />
          </el-icon>
        </el-tooltip>
      </p>
      <el-button
        text
        :loading="loading"
        :disabled="loading"
        title="重新读取这个会话的时间线"
        aria-label="重新读取这个会话的时间线"
        @click="retry"
      >
        重试
      </el-button>
    </div>

    <el-alert v-if="error" type="error" :closable="false" show-icon class="effect-session__error">
      <template #title>读取会话时间线失败：{{ error }}</template>
    </el-alert>

    <div v-loading="loading && !rows.length" class="effect-session__body">
      <p v-if="!rows.length && !loading && !error" class="effect-placeholder">
        这个会话没有可展示的节点（可能还没聊过、或日志已删除）。
      </p>
      <ol v-else class="effect-session__list">
        <li v-for="row in rows" :key="row.key" class="effect-session__row">
          <span class="effect-session__meta">
            <span class="effect-session__time">{{ row.time }}</span>
            <span class="effect-session__turn">{{ row.turn }} <template v-if="row.step">{{ row.step }}</template></span>
            <span class="effect-session__kind">{{ row.kindLabel }}</span>
            <span class="effect-session__exposure" :class="{ 'is-exposed': row.exposure }">{{ row.exposureText }}</span>
          </span>
          <span v-if="row.detail" class="effect-session__detail">{{ row.detail }}</span>
        </li>
      </ol>
    </div>
  </el-dialog>
</template>

<style scoped>
.effect-session__head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 8px;
}

.effect-session__note {
  margin: 0;
  min-width: 0;
  color: var(--text-muted);
  font-size: 12px;
  line-height: 1.5;
  overflow-wrap: anywhere;
}

.effect-session__error {
  margin: 0 0 8px;
}

.effect-session__body {
  min-height: 80px;
}

.effect-session__list {
  margin: 0;
  padding: 0 0 0 16px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.effect-session__row {
  display: flex;
  flex-direction: column;
  gap: 2px;
  font-size: 12px;
  color: var(--text-secondary);
}

.effect-session__row::marker {
  color: var(--text-muted);
  font-size: 10px;
}

.effect-session__meta {
  display: flex;
  flex-wrap: wrap;
  align-items: baseline;
  gap: 4px 10px;
  color: var(--text);
}

.effect-session__time {
  font-variant-numeric: tabular-nums;
  color: var(--text-muted);
}

.effect-session__turn {
  font-variant-numeric: tabular-nums;
}

.effect-session__kind {
  padding: 0 6px;
  border-radius: 8px;
  background: var(--overlay-faint);
  color: var(--text-secondary);
  font-size: 10.5px;
}

/* 暴露 / 未暴露：不给「好 / 坏」语义色——未暴露不等于失败（预算 / 零命中都算正常）。 */
.effect-session__exposure {
  font-size: 10.5px;
  color: var(--text-muted);
}

.effect-session__exposure.is-exposed {
  color: var(--accent);
}

.effect-session__detail {
  color: var(--text-secondary);
  font-size: 11px;
  line-height: 1.5;
  font-variant-numeric: tabular-nums;
  overflow-wrap: anywhere;
}

.effect-placeholder {
  margin: 0;
  color: var(--text-muted);
  font-size: 12px;
  line-height: 1.55;
  overflow-wrap: anywhere;
}
</style>
