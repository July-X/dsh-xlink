<script setup>
// 用量分析（2026-10-28 用户要求）：侧栏「工作台」组的新入口，把原先分散在
// 概览页与两扇独立窗口里的用量信息聚合到一页。
//
// 页面顺序按用户原话：**上方是套餐用量，下方的详细页是原「模型用量」窗口的
// 内容**。独立窗口的组件、Rust 命令与 capability 全部保留（入口隐藏 ≠ 能力
// 删除），这里只借用 UsageWindow 的 inline 模式，不再从界面打开那扇窗。
import SubscriptionUsageCard from '../subscription/SubscriptionUsageCard.vue';
import UsageWindow from './UsageWindow.vue';
</script>

<template>
  <section class="panel usage-analysis-panel">
    <div class="page-head">
      <div class="page-head__text">
        <h1 class="page-title">用量分析</h1>
        <p class="page-desc">云端套餐剩余额度与本机模型 token 消耗：上方看额度，下方看趋势。</p>
      </div>
    </div>

    <!-- 上方：套餐用量（原概览页那张卡，2026-10-28 整体迁入）。 -->
    <SubscriptionUsageCard />

    <!-- 下方：模型用量详情（原独立窗口的详细页）。窗口壳退场后由这张卡
         提供表面与描边，内边距归零让窗口自己的 head / main / footer 贴边。 -->
    <div class="card usage-model-card">
      <UsageWindow inline />
    </div>
  </section>
</template>

<style scoped>
/* 卡只负责表面与描边：内嵌内容自带 head 的内边距与分隔线，这里再给 16px
   会得到「卡里再套一层卡头」的双层内边距。`overflow: hidden` 让圆角裁切
   内嵌内容自己的头部底边。 */
.usage-model-card {
  padding: 0;
  gap: 0;
  overflow: hidden;
  min-width: 0;
}
</style>
