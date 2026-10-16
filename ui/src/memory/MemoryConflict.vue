<script setup>
/**
 * 一条候选的**冲突标记**：模型说它与库里哪几条「意义冲突」，这里把两边**并排**摆出来，
 * 让用户自己留哪一条（设计 §7-2「意义冲突的记忆标『待处理冲突』，不能由模型悄悄选择胜者」）。
 *
 * ## 为什么单独一个组件
 *
 * 面板已经贴着 470 行预算，而这段是**一块独立形状**：它有自己的两种状态（引用的那条还在 /
 * 已经不在了）、自己的两个动作（停用 / 删除）、以及一条必须说清的边界。挤进候选卡片只会
 * 让两边都在预算线上，而且这类「并排对照」的样式一改就要动整张卡片。
 *
 * ## 两条不许破的纪律
 *
 * 1. **只摆事实，不代做判断**：文案不许出现「建议保留新的」这类话——裁决权在用户手里，
 *    界面上任何暗示都会变成「系统已经替我选好了，只是让我确认一下」。
 * 2. **引用的那条不在了就如实说**：它可能刚被删了，或不在当前筛选范围内。**不许静默隐藏**
 *    ——「模型说冲突、面板上什么都没有」看起来就像模型判断错了。
 *
 * ## 样式为什么不在这个文件里
 *
 * 这一族类名放在 `ui/src/memory/memory.css`：C6 的取代块（`MemorySuperseded.vue`）是同一件
 * 事的另一个形状，两者共用一份样式——scoped 的样式组件之间互不共享，复制一份会被门禁的
 * 重复区间扫描算成重复代码，而两种关系在界面上必须长得一样（长得不一样会暗示哪一种更
 * 严重，而它们其实一样重：都要用户按一次）。
 */
import { computed } from 'vue';

const props = defineProps({
  /** 模型标的既有记忆 id（C5 的 `conflicts_with`）。 */
  conflicts: { type: Array, default: () => [] },
  /** 当前面板已经拿到的记录（用来把 id 变成看得见的标题与正文）。 */
  records: { type: Array, default: () => [] },
  busy: { type: Boolean, default: false },
});

const emit = defineEmits(['disable', 'remove']);

/** id → 记录。找不到的 id 保留下来并标成「不在」，不整条丢掉。 */
const rows = computed(() =>
  props.conflicts.map((id) => {
    const record = props.records.find((row) => row.id === id);
    return {
      id,
      record,
      missing: !record,
      reason: !record ? '（这条已不在库里，或不在当前筛选范围内。）' : record.body || '（无正文）',
    };
  })
);
</script>

<template>
  <div v-if="rows.length" class="memory-flag">
    <p class="memory-flag__lead">
      与既有 {{ rows.length }} 条记忆意义冲突：它们说的不是同一件事，或者说的相反。由你决定留哪一条。
    </p>
    <div v-for="row in rows" :key="row.id" class="memory-flag__pair">
      <p class="memory-flag__body">
        {{ row.missing ? row.reason : row.record.title || '（无标题）' }}
      </p>
      <p class="memory-flag__meta">{{ row.missing ? row.id : row.reason }}</p>
      <div v-if="!row.missing && !busy" class="memory-flag__actions">
        <el-button size="small" text @click="emit('disable', row.record)">停用这条</el-button>
        <el-button size="small" text @click="emit('remove', row.record)">删除这条</el-button>
      </div>
    </div>
  </div>
</template>
