<script setup>
// 单个已安装内核版本的插件物化快照 tooltip 内容：
// - 复用 `installed-tip` 样式与 VersionsPanel 标题样式保持一致。
// - 仅渲染 shell 返回的快照，加载中、加载失败、空结果都有独立文案。
import { computed } from 'vue';

const props = defineProps({
  snapshot: { type: Object, required: true },
  version: { type: String, required: true },
});

function modeLabel(mode) {
  return mode === 'link' ? '链接' : mode === 'copy' ? '拷贝' : mode || '手工';
}

function pluginVersionLabel(version) {
  const value = String(version || '').trim();
  return value ? (value.startsWith('v') ? value : `v${value}`) : '—';
}

const title = computed(() => {
  const rows = props.snapshot.rows || [];
  return `${props.version} 已安装 ${rows.length} 个插件`;
});
</script>

<template>
  <div class="installed-tip">
    <p class="installed-tip-title">{{ title }}</p>
    <p v-if="snapshot.loading" class="installed-tip-loading">正在读取 {{ version }} 的插件…</p>
    <p v-else-if="snapshot.error" class="installed-tip-error">读取失败：{{ snapshot.error }}</p>
    <p v-else-if="!snapshot.rows || snapshot.rows.length === 0" class="muted" style="margin: 0">
      该内核未物化任何插件。
    </p>
    <ul v-else class="installed-tip-list">
      <li v-for="row in snapshot.rows" :key="row.id" class="installed-tip-row">
        <span class="installed-tip-name">{{ row.name }}</span>
        <span class="installed-tip-version">{{ pluginVersionLabel(row.version) }}</span>
        <span class="installed-tip-tags">
          <el-tag size="small" effect="plain">{{ modeLabel(row.mode) }}</el-tag>
          <el-tag v-if="!row.in_store" type="warning" size="small" effect="plain">中央库已移除</el-tag>
          <el-tag v-else-if="!row.synced" type="info" size="small" effect="plain">未同步</el-tag>
        </span>
      </li>
    </ul>
  </div>
</template>
