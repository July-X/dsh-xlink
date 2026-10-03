<!-- 版本号徽标：左「蓝底 + 深色 tag 图标」、右「暗底 + muted 文字」的分段式。
     侧栏品牌区与概览「当前内核」卡共用这一份——两处都在说「这是一个版本号」，
     就该长成同一张脸，抄一份就会漂（ui/AGENTS.md §图标的同一条纪律）。
     样式留在组件里而不进 theme.css：后者是反棘轮文件，只许越来越小，
     而这条规则只服务这一个组件。 -->
<template>
  <span class="version-badge">
    <span class="version-badge-icon"><TagIcon :size="11" :stroke-width="2.4" /></span>
    <span class="version-badge-text"><slot /></span>
  </span>
</template>

<script setup>
import { Tag as TagIcon } from '@lucide/vue';
</script>

<style scoped>
/* overflow:hidden + 单一圆角把两段切在中缝上。不给两段分别设圆角，
   否则中缝处会露出两个缺口。 */
.version-badge {
  display: inline-flex;
  height: 18px;
  border: 1px solid var(--border);
  border-radius: 4px;
  overflow: hidden;
  font-size: 11px;
}
/* 模板里必须写 <TagIcon>：<script setup> 只把**绑定名**暴露给模板，
   `import { Tag as TagIcon }` 绑的是 TagIcon，写 <Tag> 会被当成未知元素、
   svg 压根不渲染——表现是蓝底在、图标没有，不报错。 */
.version-badge > span {
  display: flex;
  align-items: center;
}
/* 图标段：Lucide 的 Tag 是 2px 描边的小挂牌，11px 下约 0.9px；蓝底上再按
   2.4 描边提一档，否则深色笔画在饱和蓝上几乎看不见。图标取 --bg（最深的
   档）而不是白色——参考图上笔画是深色压在亮蓝上，白描边反而糊。 */
.version-badge-icon {
  padding: 0 4px;
  color: var(--bg);
  background: var(--accent);
}
.version-badge-text {
  padding: 0 6px;
  color: var(--muted);
  background: var(--card);
}
</style>
