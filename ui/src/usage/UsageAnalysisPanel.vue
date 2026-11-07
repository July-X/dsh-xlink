<script setup>
// 用量分析（2026-10-28 用户要求）：侧栏「工作台」组的新入口，把原先分散在
// 概览页与两扇独立窗口里的用量信息聚合到一页。
//
// 页面顺序按用户原话：**上方是套餐用量，下方的详细页是原「模型用量」窗口的
// 内容**。独立窗口的组件、Rust 命令与 capability 全部保留（入口隐藏 ≠ 能力
// 删除），这里只借用 UsageWindow 的 inline 模式，不再从界面打开那扇窗。
import PageHeadIcon from '../shell/PageHeadIcon.vue';
import SubscriptionUsageCard from '../subscription/SubscriptionUsageCard.vue';
import UsageWindow from './UsageWindow.vue';
import { computed } from 'vue';
import { instanceStore } from '../kernel/instance.js';

const isMcode = computed(() => import.meta.env.VITE_MCODE_EXPERIMENTAL === '1' && instanceStore.family === 'mcode');
</script>

<template>
  <section class="panel usage-analysis-panel">
    <div class="page-head">
      <div class="page-head__text"><PageHeadIcon :page="'usage'" />
        <h1 class="page-title">用量分析</h1>
        <p class="page-desc">{{ isMcode ? '当前 MiniMax 实例的用量状态。' : '云端套餐剩余额度与本机模型 token 消耗：上方看额度，下方看趋势。' }}</p>
      </div>
    </div>

    <!-- 上方：套餐用量（原概览页那张卡，2026-10-28 整体迁入）。 -->
    <div v-if="isMcode" class="card">
      <h2>当前实例用量暂不可用</h2>
      <p>尚未接入可靠的 MiniMax WebUI 实例用量来源。已有的 MiniMax Desktop 外部统计是独立来源，需要单独授权，不能代表此工作台的用量。</p>
    </div>
    <SubscriptionUsageCard v-else />

    <!-- 下方：模型用量详情（原独立窗口的详细页）。窗口壳退场后由这张卡
         提供表面与描边，内边距归零让窗口自己的 head / main / footer 贴边。 -->
    <div v-if="!isMcode" class="card usage-model-card">
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
