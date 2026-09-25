<script setup>
// 日志文件分类侧栏：LogModal 与 LogViewerWindow 共用；只渲染与转发点击，
// 切签逻辑留给父组件。宽度由父组件通过 CSS 变量 `--sidebar-width` 注入，
// 既支持固定值（如主面板 168px），也支持可拖拽分隔条动态调整
// （如独立窗口）。`.collapsed` 走 34px 细轨，覆盖变量。
import { Expand, Fold } from '@element-plus/icons-vue';
import { formatLogSize } from '../logs.js';

defineProps({
  groups: { type: Array, required: true },
  activeName: { type: String, default: null },
  railCollapsed: { type: Boolean, required: true },
});

const emit = defineEmits(['select', 'toggle-rail']);
</script>

<template>
  <aside
    class="log-tabs"
    :class="{ collapsed: railCollapsed }"
    role="tablist"
    aria-orientation="vertical"
  >
    <button
      type="button"
      class="rail-toggle"
      :title="railCollapsed ? '展开日志列表' : '收起日志列表'"
      @click="emit('toggle-rail')"
    >
      <el-icon><Expand v-if="railCollapsed" /><Fold v-else /></el-icon>
    </button>
    <template v-if="!railCollapsed">
      <span v-if="!groups.length" class="log-tab-size">（暂无日志文件）</span>
      <div v-for="group in groups" :key="group.id" class="log-group">
        <div class="log-group-title">{{ group.label }}</div>
        <button
          v-for="f in group.files"
          :key="f.name"
          type="button"
          class="log-tab"
          role="tab"
          :aria-selected="f.name === activeName ? 'true' : 'false'"
          :title="f.name"
          @click="emit('select', f.name)"
        >
          <span class="log-tab-name">{{ f.name }}</span>
          <span v-if="typeof f.size === 'number'" class="log-tab-size">{{ formatLogSize(f.size) }}</span>
        </button>
      </div>
    </template>
  </aside>
</template>
