<script setup>
// 一条记忆长什么样、以及它的三个动作。
//
// 从 `MemoryPanel.vue` 抽出来：那个文件连着五轮长大（310 → 448），每次都靠上调代码预算过关，
// 而「一行记录」与「列表 / 候选 / 编辑器 / 预览」本来就是两件事。抽完面板只管列表与表单，
// 行自己管渲染。
//
// 组件**只读 props、只 emit**：动作（保存、停用、删除）以及它们的 toast 与确认框都在
// `memory.js` 和面板里。这里 emit 出去的只是「用户点了哪一个」——把动作也搬进来会让这个
// 组件重新长成一个面板。
//
// 模板里那一族 `memory-row` 类名**没有**自己的样式块：面板也写着同一族类名，而 scoped
// 只对本组件模板里的元素生效，所以它们统一放在 `ui/src/memory/memory.css`（理由见那份文件）。
import { computed } from 'vue';
import { globalBusy, isLoading } from '../shell/loading.js';
import { scopeText, shortTime, sourcesText, verificationText } from './memory.js';

const props = defineProps({
  row: { type: Object, required: true },
});
const emit = defineEmits(['edit', 'toggle', 'remove']);

/// 停用/启用按钮上那个动词由状态决定，所以下一个状态也在这里算——面板不必知道这些。
const accepted = computed(() => props.row.state === 'accepted');
</script>

<template>
  <div class="page-row memory-row">
    <div class="memory-row__main">
      <div class="memory-row__title">
        <span class="memory-row__name">{{ row.title || '（无标题）' }}</span>
        <el-tag size="small" effect="plain" :type="row.scope === 'general' ? 'info' : 'primary'">
          {{ row.scope === 'general' ? '通用偏好' : '本项目' }}
        </el-tag>
        <el-tag v-if="row.pinned" size="small" effect="plain" type="warning">固定</el-tag>
        <el-tag v-if="!accepted" size="small" effect="plain" type="info">已停用</el-tag>
      </div>
      <p class="memory-row__body">{{ row.body }}</p>
      <p class="memory-row__meta">
        {{ scopeText(row) }} · {{ sourcesText(row) }} ·
        {{ verificationText(row.verification) }} · 更新于 {{ shortTime(row.updated_at) }} ·
        已用 {{ row.use_count }} 次
      </p>
    </div>
    <div class="memory-row__actions">
      <el-button size="small" text :disabled="globalBusy" @click="emit('edit', row)">编辑</el-button>
      <el-button
        size="small"
        text
        :loading="isLoading('memoryState:' + row.id)"
        :disabled="globalBusy"
        @click="emit('toggle', accepted ? 'disabled' : 'accepted')"
      >
        {{ accepted ? '停用' : '启用' }}
      </el-button>
      <el-button
        size="small"
        text
        type="danger"
        :loading="isLoading('memoryDelete:' + row.id)"
        :disabled="globalBusy"
        @click="emit('remove', row)"
      >
        删除
      </el-button>
    </div>
  </div>
</template>
