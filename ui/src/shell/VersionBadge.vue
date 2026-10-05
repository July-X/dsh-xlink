<!-- 版本号徽标：左「纯色底 + tag 图标」、右「暗底 + muted 文字」的分段式。
     侧栏品牌区与概览「当前内核」卡共用这一份——两处都在说「这是一个版本号」，
     就该长成同一张脸，抄一份就会漂（ui/AGENTS.md §图标的同一条纪律）。
     图标段有两档语气（icon prop）：accent（默认，蓝底深描边，侧栏品牌区）、
     black（黑底浅描边，概览「当前内核」卡，2026-10-05 用户要求）。
     样式留在组件里而不进 theme.css：后者是反棘轮文件，只许越来越小，
     而这条规则只服务这一个组件。 -->
<template>
  <span class="version-badge" :class="{ 'version-badge--black': icon === 'black' }">
    <span class="version-badge-icon"><TagIcon :size="13" :stroke-width="2.2" /></span>
    <span class="version-badge-text"><slot /></span>
  </span>
</template>

<script setup>
import { Tag as TagIcon } from '@lucide/vue';

defineProps({
  /** 图标段配色：'accent'（蓝底，默认）| 'black'（黑底，概览当前内核卡）。 */
  icon: {
    type: String,
    default: 'accent',
    validator: (value) => ['accent', 'black'].includes(value),
  },
});
</script>

<style scoped>
/* overflow:hidden + 单一圆角把两段切在中缝上。不给两段分别设圆角，
   否则中缝处会露出两个缺口。 */
.version-badge {
  display: inline-flex;
  height: 22px;
  border: 1px solid var(--border);
  border-radius: 5px;
  overflow: hidden;
  font-size: 13px;
}
/* 模板里必须写 <TagIcon>：<script setup> 只把**绑定名**暴露给模板，
   `import { Tag as TagIcon }` 绑的是 TagIcon，写 <Tag> 会被当成未知元素、
   svg 压根不渲染——表现是蓝底在、图标没有，不报错。 */
.version-badge > span {
  display: flex;
  align-items: center;
}
/* 图标段：Lucide 的 Tag 是 2px 描边的小挂牌，缩到 13px 约 1.1px；蓝底上再按
   2.2 描边提一档，否则深色笔画在饱和蓝上几乎看不见。图标取 --bg（最深的
   档）而不是白色——参考图上笔画是深色压在亮蓝上，白描边反而糊。
   两段 padding 不对称：图标只有 13px 见方，文字段要留出读字的呼吸。 */
.version-badge-icon {
  padding: 0 5px;
  color: var(--bg);
  background: var(--accent);
}
/* 黑色变体（概览「当前内核」卡）：纯黑方块的语气比品牌蓝更中性，描边换成
   浅色（--text）才读得出图形——深描边压在黑底上就是一团糊。 */
.version-badge--black .version-badge-icon {
  color: var(--text);
  background: #000;
}
.version-badge-text {
  padding: 0 8px;
  color: var(--muted);
  background: var(--card);
}
</style>
