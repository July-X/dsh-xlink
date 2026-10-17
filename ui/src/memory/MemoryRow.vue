<script setup>
// 一条记忆长什么样，以及点它会发生什么。
//
// 从 `MemoryPanel.vue` 抽出来：那个文件连着五轮长大（310 → 448），每次都靠上调代码预算过关，
// 而「一行记录」与「列表 / 候选 / 编辑器 / 预览」本来就是两件事。抽完面板只管列表与表单，
// 行自己管渲染。
//
// 组件**只读 props、只 emit**：这里 emit 出去的只是「用户点了哪一行」，动作（保存、停用、
// 删除）连同它们的 toast 与确认框都在 `memory.js` 与 `MemoryDetail.vue` 里。把动作搬进来
// 会让这个组件重新长成一个面板。
//
// 行自 2026-10-17 起**不再带动作按钮**（用户要求「默认精简、点开再操作」）：三枚文字按钮
// 常驻右列时，一条记忆还没读就把卡片撑长。动作搬进 `MemoryDetail.vue`，这里只留 `@open`。
// 整行可点用 `role="button"` + `tabindex="0"`，不换成原生 `<button>`：这一行的排版靠
// `.memory-row` 的 grid 加 `__main` 的 subgrid，而把根元素换成按钮会切到另一条实现路径上，
// 本屏已经因为排版坏过两次（正文被挤成左窄条、按钮压住标题）。
//
// 模板里那一族 `memory-row` 类名**没有**自己的样式块：面板也写着同一族类名，而 scoped
// 只对本组件模板里的元素生效，所以它们统一放在 `ui/src/memory/memory.css`（理由见那份文件）。
import { computed } from 'vue';
import { scopeText, shortTime, sourcesText, verificationText } from './memory.js';
import { baseName } from '../shell/labels.js';

const props = defineProps({
  row: { type: Object, required: true },
});
const emit = defineEmits(['open']);

/// 次行「哪个项目」只显示路径最后一段，完整路径进 `title`（2026-10-17 收官：整条路径
/// 让这一行在 312px 里折成两行、每条记忆多 18px）。与「最近注入」同一先例：截的是显示，
/// 不是信息。通用偏好没有路径，仍读 `scopeText` 那句「（所有项目）」。
const place = computed(() => {
  if (props.row.scope === 'general') return scopeText(props.row);
  const name = baseName(props.row.workspace_path);
  return name ? `本项目 · ${name}` : '本项目';
});
</script>

<template>
  <div
    class="page-row memory-row memory-row--clickable"
    role="button"
    tabindex="0"
    aria-haspopup="dialog"
    :aria-label="`查看记忆：${row.title || '（无标题）'}`"
    @click="emit('open', row)"
    @keydown.enter="emit('open', row)"
    @keydown.space.prevent="emit('open', row)"
  >
    <div class="memory-row__main">
      <div class="memory-row__title">
        <span class="memory-row__name">{{ row.title || '（无标题）' }}</span>
        <el-tag size="small" effect="plain" :type="row.scope === 'general' ? 'info' : 'primary'">
          {{ row.scope === 'general' ? '通用偏好' : '本项目' }}
        </el-tag>
        <el-tag v-if="row.pinned" size="small" effect="plain" type="warning">固定</el-tag>
        <el-tag v-if="row.state !== 'accepted'" size="small" effect="plain" type="info">
          已停用
        </el-tag>
      </div>
      <p class="memory-row__body memory-row__body--clamp">{{ row.body }}</p>
      <p class="memory-row__meta" :title="row.workspace_path || ''">
        {{ place }} · {{ sourcesText(row) }} ·
        {{ verificationText(row.verification) }} · 更新于 {{ shortTime(row.updated_at) }} ·
        已用 {{ row.use_count }} 次
      </p>
    </div>
  </div>
</template>
