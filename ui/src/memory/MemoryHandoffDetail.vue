<script setup>
import { globalBusy, isLoading } from '../shell/loading.js';
import { MEMORY_COUNT_TIP, shortTime } from './memory.js';
defineProps({ row: { type: Object, required: true } });
const emit = defineEmits(['close', 'edit', 'archive', 'remove']);
const lists = [{ key: 'done', label: '已完成' }, { key: 'todo', label: '待办' }, { key: 'blocked', label: '阻塞' }];
</script>

<template>
  <el-dialog class="memory-dialog" :model-value="true" :title="row.title || '工作交接'" width="min(560px, 92vw)" append-to-body :close-on-click-modal="false" @update:model-value="(open) => !open && emit('close')">
    <div class="memory-row__main">
      <!-- 交接也会被注入（插件回报里带 handoff id），所以「被注入」在这里同义。 -->
      <p class="memory-row__meta" :title="MEMORY_COUNT_TIP">
        {{ row.workspace_path }} · {{ row.state === 'active' ? '进行中' : '已归档' }} ·
        {{ shortTime(row.updated_at) }} · 被注入 {{ row.recall_count || 0 }} 次
      </p>
      <h3 class="memory-handoff-heading">目标</h3>
      <p class="memory-row__body">{{ row.goal || '未填写' }}</p>
      <p class="memory-row__body">{{ row.body }}</p>
      <section v-for="list in lists" :key="list.key">
        <h3 class="memory-handoff-heading">{{ list.label }}</h3>
        <ul v-if="row[list.key]?.length" class="memory-handoff-list"><li v-for="(text, index) in row[list.key]" :key="index">{{ text }}</li></ul>
        <p v-else class="memory-row__meta">无</p>
      </section>
    </div>
    <template #footer>
      <div class="memory-actions dialog-actions">
        <el-button v-if="row.state === 'active'" class="memory-badge" :disabled="globalBusy" @click="emit('edit', row)">编辑</el-button>
        <el-button v-if="row.state === 'active'" class="memory-badge" :disabled="globalBusy" :loading="isLoading('memoryState:' + row.id)" @click="emit('archive', row)">归档</el-button>
        <el-button class="memory-badge memory-badge--danger" :disabled="globalBusy" :loading="isLoading('memoryDelete:' + row.id)" @click="emit('remove', row)">删除</el-button>
      </div>
    </template>
  </el-dialog>
</template>
