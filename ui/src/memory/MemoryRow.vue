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
import { shortTime, sourcesText, verificationText } from './memory.js';
import { baseName } from '../shell/labels.js';

const props = defineProps({
  row: { type: Object, required: true },
});
const emit = defineEmits(['open']);

/// 标签上写**具体项目名**（用户 2026-10-17 第二问：笼统的项目叫法等于没说）。
/// 名字只取路径最后一段（`baseName`）：整条路径在这一行会折成两行、每条记忆多 18px；
/// 完整路径进 `title`，一个字不丢。通用偏好没有项目，仍是「通用偏好」。
/// 路径缺失时兜底写**范围名 + 缺了什么**（「项目记忆（未记路径）」）而不是「当前项目」：
/// 后者既与列表筛选那一档撞名，也是猜的——没有路径就不知道它是哪个项目。补上「未记路径」
/// 是因为这一格平时显示的是**项目名**，只写范围名会让同一个位置出现两种语义层级，
/// 用户分不清「项目记忆」是在说范围还是在说这个项目的名字（用户 2026-10-18 要求语义精确）。
const projectName = computed(() => baseName(props.row.workspace_path) || '项目记忆（未记路径）');
/// 标签的悬浮说明：项目记忆给完整路径，通用偏好说清它是「所有项目都算」。
/// 路径缺失时与标签取同一个兜底，否则悬停说明与可见文字各说一个名字。
const scopeTitle = computed(() =>
  props.row.scope === 'general' ? '所有项目都算' : props.row.workspace_path || '项目记忆（未记路径）'
);
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
        <el-tag
          size="small"
          effect="plain"
          :type="row.scope === 'general' ? 'info' : 'primary'"
          :title="scopeTitle"
        >
          {{ row.scope === 'general' ? '通用偏好' : projectName }}
        </el-tag>
        <el-tag v-if="row.pinned" size="small" effect="plain" type="warning">固定</el-tag>
        <el-tag v-if="row.state !== 'accepted'" size="small" effect="plain" type="info">
          {{ row.kind === 'handoff' ? row.state === 'active' ? '进行中' : '已归档' : '已停用' }}
        </el-tag>
      </div>
      <p class="memory-row__body memory-row__body--clamp">{{ row.body }}</p>
      <!-- 末尾那个次数是 `use_count`，而它**只由 `memory_read` 增加**
           （`memory/store.rs::read_and_note_use`）：检索命中不算、面板把正文画出来也不算。
           所以写「模型读过」而不是「用过」——后者会被读成「被注入过几次」，而那条信息在
           「最近注入」卡上，与本行这个数不是一回事（用户 2026-10-18 要求消歧义）。 -->
      <p v-if="row.kind === 'handoff'" class="memory-row__meta">更新于 {{ shortTime(row.updated_at) }}</p>
      <p v-else class="memory-row__meta">
        {{ sourcesText(row) }} ·
        {{ verificationText(row.verification) }} · 更新于 {{ shortTime(row.updated_at) }} ·
        模型读过 {{ row.use_count }} 次
      </p>
    </div>
  </div>
</template>
