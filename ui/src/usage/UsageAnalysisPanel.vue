<script setup>
// 用量分析（2026-10-28 用户要求）：侧栏「工作台」组的新入口，把原先分散在
// 概览页与两扇独立窗口里的用量信息聚合到一页。
//
// 页面顺序按用户原话：**上方是套餐用量，下方的详细页是原「模型用量」窗口的
// 内容**。独立窗口的组件、Rust 命令与 capability 全部保留（入口隐藏 ≠ 能力
// 删除），这里只借用 UsageWindow 的 inline 模式，不再从界面打开那扇窗。
//
// 多内核（2026-11-07 用户要求）：这一页是**两个内核共用**的。上方套餐卡是壳
// 自己的云端额度（凭据由壳维护），与内核无关；下方模型用量卡读的是壳记的账，
// 也与内核无关。内核维度已从页面级降到控件级，这一页不再读内核状态。
// MiniMax 工作台自身的用量还没接入——那句话降成卡里的一行说明。
import PageHeadIcon from '../shell/PageHeadIcon.vue';
import SubscriptionUsageCard from '../subscription/SubscriptionUsageCard.vue';
import UsageWindow from './UsageWindow.vue';

// 这一页的数据与内核无关：套餐额度五张卡两个内核下完全一样，模型用量那份读数
// 也一直都在（它读的是壳自己记的账）。所以整页不再按内核状态分叉，也不读
// instanceStore.family。MiniMax 工作台自身的用量还没接入——那句话降成一行
// 说明，只在实验构建里出现。
const mcodeExperimental = import.meta.env.VITE_MCODE_EXPERIMENTAL === '1';
</script>

<template>
  <section class="panel usage-analysis-panel">
    <div class="page-head">
      <div class="page-head__text"><PageHeadIcon :page="'usage'" />
        <h1 class="page-title">用量分析</h1>
        <p class="page-desc">云端套餐剩余额度与本机模型 token 消耗：上方看额度，下方看趋势。</p>
      </div>
    </div>

    <!-- 上方：套餐用量（原概览页那张卡，2026-10-28 整体迁入）。 -->
    <SubscriptionUsageCard />

    <!-- 下方：模型用量详情（原独立窗口的详细页）。窗口壳退场后由这张卡
         提供表面与描边，内边距归零让窗口自己的 head / main / footer 贴边。
         这张卡不按内核分叉：内核那份读数一直都在，MiniMax 工作台自身的用量
         还没接入——那只是一行说明，不占掉一整块，也不另起一页。 -->
    <div class="card usage-model-card">
      <p v-if="mcodeExperimental" class="memory-note usage-model-note">MiniMax 工作台自身的用量尚未接入：已有的 MiniMax Desktop 外部统计是独立来源，需要单独授权，不能代表此工作台的用量。</p>
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

/* 那行说明：小字沿用记忆页那一族（.memory-note 是全局类），这里只给位置与
   内边距——卡本身不留内边距（见上）。 */
.usage-model-note {
  padding: 12px 20px;
}
</style>
