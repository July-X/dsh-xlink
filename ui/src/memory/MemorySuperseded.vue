<script setup>
/**
 * 一条候选的**取代标记**（C6，设计 §7-2「用户纠正优先于旧候选」）：模型说用户这次说的话
 * 推翻了先前提出的那几条，这里把它们列出来，让用户决定丢不丢。
 *
 * ## 三条不许破的纪律
 *
 * 1. **只标记、不执行**：壳**不会**因为这个标记拒收任何候选——那几条还在「待确认」里，
 *    要用户自己按一次「丢弃这条」。所以文案里不许出现「已自动作废前面的」这类话：它会把
 *    「模型标了一下」说成「已经替你决定了」。
 * 2. **与冲突块长得一样**：共用 `memory-flag*` 一族类名（样式在 `ui/src/memory/memory.css`，
 *    不在本组件——scoped 的样式组件之间不共享，复制一份会被门禁的重复区间扫描算成重复
 *    代码）。两种关系在界面上必须看起来一样重：不一样会暗示哪一种更严重，而它们其实
 *    一样——都要用户按一次。
 * 3. **引用的那条不在待确认里了要如实说**：它可能刚被保存或丢弃了。**不许静默隐藏**——
 *    「模型说推翻了、面板上什么都没有」看起来就像模型判断错了。
 *
 * ## 为什么与 C5 的冲突块是两个组件而不是一个
 *
 * 两者形状几乎相同（id 列表 → 解析成看得见的条目 → 给动作），但**解析对象不同**：冲突指向
 * 已生效的**记忆**，取代指向还没生效的**候选**；而候选不在面板的记忆列表里，它在待确认
 * 那一份里。合成一个组件就要它接两份 prop 并在每处判断「这份 id 来自哪里」，那种组件
 * 读起来比两个各管一件事的更难验。
 */
import { computed } from 'vue';

const props = defineProps({
  /** 模型标的待确认候选 id（C6 的 `supersedes`）。 */
  supersedes: { type: Array, default: () => [] },
  /** 当前面板的待确认候选（用来把 id 变成看得见的标题与正文）。 */
  candidates: { type: Array, default: () => [] },
  busy: { type: Boolean, default: false },
});

const emit = defineEmits(['discard']);

/** id → 候选。找不到的 id 保留下来并标成「已不在」，不整条丢掉。 */
const rows = computed(() =>
  props.supersedes.map((id) => {
    const candidate = props.candidates.find((row) => row.id === id);
    return {
      id,
      candidate,
      missing: !candidate,
      reason: !candidate ? '（这条已经不在「待确认」里了——你可能刚保存或丢弃了它。）' : candidate.body || '（无正文）',
    };
  })
);
</script>

<template>
  <div v-if="rows.length" class="memory-flag">
    <p class="memory-flag__lead">
      你说这句时推翻了先前提出的 {{ rows.length }} 条。它们还在「待确认」里，由你决定丢弃哪几条——系统没有替你作废任何一条。
    </p>
    <div v-for="row in rows" :key="row.id" class="memory-flag__pair">
      <p class="memory-flag__body">
        {{ row.missing ? row.reason : row.candidate.title || '（无标题）' }}
      </p>
      <p class="memory-flag__meta">{{ row.missing ? row.id : row.reason }}</p>
      <div v-if="!row.missing && !busy" class="memory-flag__actions">
        <el-button size="small" text @click="emit('discard', row.candidate)">丢弃这条</el-button>
      </div>
    </div>
  </div>
</template>
