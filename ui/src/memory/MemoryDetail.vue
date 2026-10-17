<script setup>
// 一条记忆的详情，以及它的三枚动作。
//
// 2026-10-17 用户要求「列表默认精简收缩、点击后弹出操作界面、操作按钮显示成 badge」：
// 行里常驻三枚文字按钮时，一条记忆的正文还没读就把卡片撑长。动作搬到这里之后，列表只
// 负责读，操作在点开的那一层。
//
// 为什么独立成组件：面板已经连着五轮长大，而详情与列表的范围筛选、候选、预览、导出没有
// 一处共享状态，塞回 `MemoryPanel.vue` 只会让它再长一轮。
//
// 正文那一族类名复用 `memory-row*`（跨组件共用，样式在 `memory.css`）：详情里的标题、
// 正文、次行必须与列表行一模一样，另抄一份近似排版就是两处各漂各的。它们带的
// `grid-column` / `grid-row` 在这里是惰性的——`__main` 不再是 subgrid 的子项，落成一个
// 单列网格，标题、正文、次行按序占三行，读数与列表一致。
//
// 动作三枚是**徽章按钮**（`memory-badge`）：带边框、带底色、胶囊形，与列表行尾那族徽章
// 同一套语言（`diagnostics.css` 的 `.diag-row__cta` 是这套语言的出处）。这里不复刻它那条
// 11px / `padding: 1px 8px` 的读数档，因为这三枚是弹窗里唯一的动作、不是读数旁的装饰。
import { computed } from 'vue';
import { globalBusy, isLoading } from '../shell/loading.js';
import { scopeText, shortTime, sourcesText, verificationText } from './memory.js';

const props = defineProps({
  row: { type: Object, required: true },
});
const emit = defineEmits(['close', 'edit', 'toggle', 'remove']);
const accepted = computed(() => props.row.state === 'accepted');
</script>

<template>
  <el-dialog
    :model-value="true"
    :title="row.title || '（无标题）'"
    width="min(560px, 92vw)"
    append-to-body
    :close-on-click-modal="false"
    @update:model-value="(open) => !open && emit('close')"
  >
    <div class="memory-row__main">
      <div class="memory-row__title">
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
    <!-- 动作排在 `#footer` 里，容器挂 `.memory-actions`（flex + 8px 缝）与 `.dialog-actions`
         （共享类，只管靠右）。EP 给 `#footer` 的 `text-align: right` 对行内内容有效，按钮
         被包进 div 之后块级 `text-align` 管不到，靠 `justify-content` 补回来——
         `MigrationPrompt` 的 `.prompt-footer-ask` 是同一个道理。 -->
    <template #footer>
      <div class="memory-actions dialog-actions">
        <el-button class="memory-badge" :disabled="globalBusy" @click="emit('edit', row)">
          编辑
        </el-button>
        <el-button
          class="memory-badge memory-badge--warn"
          :loading="isLoading('memoryState:' + row.id)"
          :disabled="globalBusy"
          @click="emit('toggle', accepted ? 'disabled' : 'accepted')"
        >
          {{ accepted ? '停用' : '启用' }}
        </el-button>
        <el-button
          class="memory-badge memory-badge--danger"
          :loading="isLoading('memoryDelete:' + row.id)"
          :disabled="globalBusy"
          @click="emit('remove', row)"
        >
          删除
        </el-button>
      </div>
    </template>
  </el-dialog>
</template>
