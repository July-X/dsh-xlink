<script setup>
// 候选的三枚动作：保存 / 存为通用偏好 / 忽略。
//
// 2026-10-19 从 `MemoryCandidateRow.vue` 抽出来：候选行整块可点、点开是一个详情弹窗，
// 而**行与弹窗都要能当场裁决同一条候选**。三枚按钮各写一份的结局是两边迟早不同步
// （改一边的 loading key、漏一边的 `'general'`，都不报任何错），而它们点下去做的事
// 本来就必须一模一样。
//
// 样式复用 `memory.css` 的 `.memory-actions` + `.memory-badge`：记忆详情底部那三枚也是
// 同一套语言（都是「这条记录接下来怎么办」的裁决），所以候选这两处也用徽章，而不是
// 行里一套文字按钮、弹窗里一套徽章。根元素就是 `.memory-actions`（flex + 8px 缝），
// 调用方只补**位置**：行上补 `memory-row__actions`（沉到行底、贴右），弹窗里补
// `dialog-actions`（靠右）。**不要**在这里写死 `justify-content`：两处要的落点不同。
//
// 动作直接落在 `memory.js` 的 store 动作上，不往调用方再转一层事件：这三枚按钮就是
// 「裁决这条候选」这一件事的全部语义，行与详情都不需要知道它们点了什么。
//
// loading key 仍是 `memoryCandidate:<id>`：同一个 id 在行与弹窗里同时可见时，一边在跑
// 另一边也该跟着转（`withLoading` 记的就是这个 key）。
import { globalBusy, isLoading } from '../shell/loading.js';
import { decideCandidate } from './memory.js';

defineProps({
  item: { type: Object, required: true },
});
</script>

<template>
  <div class="memory-actions">
    <el-button
      class="memory-badge"
      :loading="isLoading('memoryCandidate:' + item.id)"
      :disabled="globalBusy"
      @click="decideCandidate(item, true)"
    >
      保存
    </el-button>
    <!-- 已经是通用偏好的候选没有第二个范围可选，再给这个按钮只会点出一个同名结果。 -->
    <el-button
      v-if="item.scope !== 'general'"
      class="memory-badge"
      :loading="isLoading('memoryCandidate:' + item.id)"
      :disabled="globalBusy"
      @click="decideCandidate(item, true, 'general')"
    >
      存为通用偏好
    </el-button>
    <!-- 「忽略」是队列里唯一让这条消失的动作，所以它带危险色；上面两枚都是「接受」，
         同一档配色（语气跟着动作走，与本仓控制塔行尾徽章同一条）。 -->
    <el-button
      class="memory-badge memory-badge--danger"
      :loading="isLoading('memoryCandidate:' + item.id)"
      :disabled="globalBusy"
      @click="decideCandidate(item, false)"
    >
      忽略
    </el-button>
  </div>
</template>
