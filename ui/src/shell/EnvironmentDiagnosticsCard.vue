<script setup>
// 「环境回退与诊断」：设置页的故障兜底卡（环境回退点 / 深入排查 / 启动诊断三行）。
// 抽成独立组件是因为它要按平台换栏：Defender 排除项卡只在 Windows 出现，非 Windows
// 上左栏只剩两张卡、底部空出一大截（2026-11-05 用户报），所以这张卡在非 Windows
// 落左栏末尾补位，Windows 仍留右栏末尾（2026-11-02 的配平）。落栏条件在两个挂载点
// 上，见 SettingsPanel.vue 的布局注释。
import { Tickets, Tools } from '@element-plus/icons-vue';
import SnapshotCard from '../diagnostics/SnapshotCard.vue';
import BisectPanel from '../diagnostics/BisectPanel.vue';
import { openStartupDiagnosis } from '../diagnostics/diagnostics.js';

/**
 * 「启动诊断」那一行去看最近一次启动过程。
 *
 * 与概览页同一套写法：id 交给 `openStartupDiagnosis` 去问后端「最近一条」，
 * 这里不塞任何具体 id——塞进去会让用户点进来看到上一次启动的记录，而按钮上
 * 写的是「查看」。
 *
 * 不挂 `isLoading`：`reloadStartupDiagnosis(runId, false)` 那次拉取是
 * **非手动**的，不经过 `withLoading`，挂上去就是一个永远不转的 loading。
 */
function openStartupRun() {
  // 按上面那条决定，这里**不塞**任何 id（包括事故关联的 runId）：这一行是
  // 「最近一次启动过程」的通用入口，与某一桩事故无关。
  openStartupDiagnosis('', 'settings');
}
</script>

<template>
  <!-- 「环境回退点 / 深入排查 / 启动诊断」三张并排的大卡收成一张
       （设计稿 draft 2844：「环境回退与诊断」，caption「出问题时使用」）。
       三者同属「环境出问题时才动」这一组，分成四张卡只是把同一个上下文摊成四段。
       三行各自是一个 `page-list-row`：左标题 + 状态说明，右动作。
       **入口一个没少**：恢复 / 开始排查 / 停止都留在自己那一行的动作区，
       「查看」展开的是明细；启动诊断直接打开诊断窗口。 -->
  <div class="card">
    <div class="card-head">
      <h2>
        <span class="card-title-icon" aria-hidden="true"><el-icon><Tools /></el-icon></span>
        环境回退与诊断
      </h2>
      <span class="head-meta"><span class="muted">出问题时使用</span></span>
    </div>
    <div class="page-list diagnostics-list">
      <!-- 前两行自带 Fragment：行是常驻的，明细与告警是它们的兄弟节点。 -->
      <SnapshotCard />
      <BisectPanel />

      <div class="page-list-row">
        <div class="page-list-main">
          <h3 class="page-list-title">启动诊断</h3>
          <p class="page-list-meta">这次启动停在哪一步、接下来做什么。</p>
        </div>
        <div class="page-list-actions">
          <el-button
            round
            size="small"
            :icon="Tickets"
            title="查看最近一次启动过程"
            @click="openStartupRun"
          >
            查看
          </el-button>
        </div>
      </div>

    </div>
  </div>
</template>

<style scoped>
/* Fragment 子组件的行也属于这张列表；收尾行不画分隔线。 */
.diagnostics-list > :deep(.page-list-row) {
  position: relative;
  border-bottom: 0;
}
.diagnostics-list > :deep(.page-list-row:not(:last-child))::after {
  content: '';
  position: absolute;
  left: 0;
  right: 0;
  bottom: 0;
  height: 2px;
  background: var(--divider-strong);
  pointer-events: none;
  clip-path: polygon(0 50%, 20% 0, 80% 0, 100% 50%, 80% 100%, 20% 100%);
}
</style>
