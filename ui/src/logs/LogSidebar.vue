<script setup>
// 日志文件分类侧栏：LogModal 与 LogViewerWindow 共用；只渲染与转发点击，
// 切签逻辑留给父组件。宽度由父组件通过 CSS 变量 `--sidebar-width` 注入，
// 既支持固定值（如主面板 168px），也支持可拖拽分隔条动态调整
// （如独立窗口）。`.collapsed` 走 34px 细轨，覆盖变量。
//
// 分组标题可点（2026-10-14）：收起 / 展开这一组。折叠状态与持久化在 logs.js
// （两个宿主共用一份，键也共用），本组件只负责渲染与调动作。
import { computed, watch } from 'vue';
import { ArrowDown, Expand, Fold } from '@element-plus/icons-vue';
import {
  collapsedLogGroups,
  expandLogGroup,
  formatLogSize,
  toggleLogGroup,
} from './logs.js';

const props = defineProps({
  groups: { type: Array, required: true },
  activeName: { type: String, default: null },
  railCollapsed: { type: Boolean, required: true },
});

const emit = defineEmits(['select', 'toggle-rail']);

// 收起态算在分组对象上，而不是在模板里散三处 `isCollapsed(group.id)`（标题的
// aria-expanded、计数是否显示、文件列是否显示）：判断只写一遍，日后改判据不
// 会漏掉其中一处。依赖 reactive Set 的 `.has()`，折叠 / 展开会自动重算。
const rows = computed(() =>
  props.groups.map((group) => ({ ...group, collapsed: collapsedLogGroups.has(group.id) }))
);

/** 被收起的文件列 id：标题用 `aria-controls` 指过去。 */
function panelId(id) {
  return `log-group-panel-${id}`;
}

/**
 * 激活的文件换了、而它落在收起的那一组里时，把那一组展开。
 *
 * **只在「换文件」这一刻纠正，不在每次渲染时纠正**：用户自己把某一组收起是
 * 明确的意图，列表重读（`groups` 是一个新数组）不该把它顶回去——那会变成
 * 「点了收起，一刷新又冒出来」。反过来完全不管也不行：带着某一份证据打开
 * 日志时，它所在的那一组可能正收着，用户看不到当前显示的是哪一份。
 */
let expandedFor = null;
watch(
  () => [props.activeName, props.groups],
  () => {
    const name = props.activeName;
    // 文件被清空（弹层关闭、证据已被轮转清理）就把记下的名字忘掉：否则「关掉
    // 弹层 → 再从诊断入口打开同一份证据」时那一组不会再展开，因为名字没变。
    if (!name) {
      expandedFor = null;
      return;
    }
    if (name === expandedFor) return;
    const owner = props.groups.find((group) => group.files.some((f) => f.name === name));
    // 列表是异步到的：还没分好组时先不记，等 groups 更新后再判一次。
    if (!owner) return;
    expandedFor = name;
    expandLogGroup(owner.id);
  },
  { immediate: true }
);
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
      :aria-label="railCollapsed ? '展开日志列表' : '收起日志列表'"
      @click="emit('toggle-rail')"
    >
      <el-icon><Expand v-if="railCollapsed" /><Fold v-else /></el-icon>
    </button>
    <template v-if="!railCollapsed">
      <span v-if="!rows.length" class="log-tab-size">（暂无日志文件）</span>
      <div
        v-for="group in rows"
        :key="group.id"
        class="log-group"
        :class="{ 'is-collapsed': group.collapsed }"
      >
        <!-- 标题是按钮：点一下收起 / 展开这一组。收起的是**文件列**，标题本身
             留着并显示份数——否则收起来之后连「这里还压着一类日志」都看不见。 -->
        <button
          type="button"
          class="log-group-title"
          :aria-expanded="!group.collapsed"
          :aria-controls="panelId(group.id)"
          :title="group.collapsed ? `展开「${group.label}」` : `收起「${group.label}」`"
          @click="toggleLogGroup(group.id)"
        >
          <span class="log-group-label">{{ group.label }}</span>
          <span v-if="group.collapsed" class="log-group-count">{{ group.files.length }}</span>
          <el-icon class="log-group-caret"><ArrowDown /></el-icon>
        </button>
        <div v-show="!group.collapsed" :id="panelId(group.id)" class="log-group-files">
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
      </div>
    </template>
  </aside>
</template>
