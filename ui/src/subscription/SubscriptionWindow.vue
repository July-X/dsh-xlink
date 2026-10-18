<script setup>
import CodexUsageConsent from './CodexUsageConsent.vue';
// 独立套餐用量窗口：概览卡「套餐用量 → 查看详情」按钮经 open_subscription_window
// 弹出（URL ?subscription=1 挂载本组件，capability `subscription-viewer.json`
// 只授予 `get_subscription_usage` 与 `open_usage_window`）。
// 各 provider 分区：套餐类画窗口进度（剩余口径，三档配色），余额类逐行金额；
// 2api.store 这类**用户自定义 provider** 展示该密钥在 `/v1/usage` 的剩余额度
// （is_available=false 的文案按 provider 区分，见 subscription.js 的
// unavailableText）。窗口打开即 force 全量刷新（subscription.js 的
// refreshSubscription）；查询失败保留上次数据，错误以横幅内联呈现。底部一行
// 入口跳「模型用量」窗口（本地 token 统计）。
// 能力边界（设计稿）：各云端 API 都不提供绝对剩余 token 数——套餐类只给
// 剩余百分比、余额类只有货币金额，不虚构任何 token 数字。
import ViewerShell from '../shell/ViewerShell.vue';
import { computed, onMounted, watchEffect } from 'vue';
import { Refresh, InfoFilled, TopRight, Setting, Timer } from '@element-plus/icons-vue';
import { ioActive, isLoading } from '../shell/loading.js';
import { invoke } from '../shell/bridge.js';
import { toastActionError } from '../shell/notify.js';
import { withLoading } from '../shell/loading.js';
import {
  subscription,
  refreshSubscription,
  providerShortState,
  planTierRows,
  balanceRow,
  unavailableText,
  queriedAtLabel,
  queriedAgeCompact,
  tierFullName,
} from './subscription.js';
import { openUsageWindow } from '../usage/usage.js';
import PartitionErrorTip from './PartitionErrorTip.vue';

const CAPABILITY_TIP =
  'MiniMax 展示 Token Plan 的 5 小时 / 周窗口剩余百分比（云端 API 不提供绝对剩余 token 数）；' +
  'DeepSeek 展示按量计费账户的货币余额；智谱 GLM 展示编程套餐的 5 小时 / 周窗口剩余百分比' +
  '（组织 / 项目上下文未配置时按错误提示补配）；opencode（厂商）展示它的 Go 套餐：滚动 5 小时 /' +
  ' 周 / 月三窗口的剩余百分比（密钥在 opencode.ai 生成）；2api.store 这类自定义 provider' +
  '（Base URL 指向该中转的模型 provider）展示 `/v1/usage` 里的密钥剩余额度。OpenAI / Codex 额度分区与' +
  '「使用本机 Codex 登录查询额度」开关，仅在启用 OpenAI-OAuth-Plugin 后显示。数据约 5 分钟' +
  '更新一次，「刷新」立即重新查询。';

// 与管理壳一致：本窗口内 IO 进行中点亮标题栏鲸眼脉冲。
watchEffect(() => {
  document.body.classList.toggle('pulse-active', ioActive.value);
});

onMounted(() => {
  refreshSubscription();
});

const providers = computed(() =>
  ((subscription.data && subscription.data.providers) || []).filter((provider) => provider.configured)
);
const rows = computed(() =>
  providers.value.map((provider) => ({
    provider,
    tiers: provider.kind === 'plan' ? planTierRows(provider) : [],
    balances:
      provider.kind === 'balance'
        ? provider.balances.map((balance) => balanceRow(balance)).filter(Boolean)
        : [],
    shortState: providerShortState(provider),
    queried: queriedAtLabel(provider),
    queriedCompact: queriedAgeCompact(provider),
  }))
);
const hasConfigured = computed(() => rows.value.length > 0);
// 当前实例 / profile：余额属于哪个账号由它决定，展示出来避免误解。
const instanceText = computed(() => {
  const info = subscription.data && subscription.data.instance;
  return info ? `${info.family}/${info.id} · profile ${info.profile}` : '';
});

// 「前往模型设置」：打开工作台窗口（凭据的唯一编辑入口在工作台模型设置里）。
function openModelSettings() {
  withLoading('openModelSettings', () =>
    invoke('open_harness').catch((e) =>
      toastActionError('打开工作台失败', e, '请先启动工作台，再从工作台的模型设置更新凭据', 5000)
    )
  );
}
const lastFetched = computed(() => {
  const stamps = providers.value.map((provider) => provider.queried_at_ms).filter(Boolean);
  if (!stamps.length) return null;
  const latest = Math.max(...stamps);
  return queriedAtLabel({ queried_at_ms: latest });
});
</script>

<template>
  <ViewerShell shell-class="subwin" title="套餐用量">
    <header class="subwin-head">
      <img src="/whale-icon.png" alt="" width="22" height="22" />
      <span class="subwin-title">套餐用量</span>
      <el-tooltip :content="CAPABILITY_TIP" placement="bottom-start">
        <el-icon class="subwin-info"><InfoFilled /></el-icon>
      </el-tooltip>
      <span v-if="instanceText" class="subwin-instance">{{ instanceText }}</span>
      <span class="subwin-spacer"></span>
      <el-button
        text
        :icon="Refresh"
        :loading="subscription.loading"
        title="立即重新查询（越过 5 分钟缓存；Key 已失效时也会重试）"
        @click="refreshSubscription()"
      >
        刷新
      </el-button>
    </header>

    <main v-loading="subscription.loading && !subscription.data" class="subwin-main">
      <CodexUsageConsent />
      <el-empty v-if="subscription.data && !hasConfigured" description="当前内核未配置可查询的模型凭据">
        <el-button type="primary" @click="openModelSettings">前往模型设置</el-button>
      </el-empty>
      <template v-else>
        <el-alert
          v-for="(error, index) in subscription.errors"
          :key="index"
          :title="error"
          type="warning"
          :closable="false"
          show-icon
        />

        <section v-for="row in rows" :key="row.provider.id" class="sub-section">
          <div class="sub-section-head">
            <h3>{{ row.provider.label }}{{ row.provider.id === 'openai_codex' ? '' : row.provider.kind === 'plan' ? ' Token Plan' : ' 按量余额' }}</h3>
            <span
              v-if="row.queriedCompact"
              class="age-pill"
              :title="'查询于 ' + row.queried"
            >
              <el-icon><Refresh /></el-icon>{{ row.queriedCompact }}
            </span>
            <span v-else class="sub-section-hint">尚未查询成功</span>
          </div>

          <!-- MiniMax：5h / 周窗口进度。无周限额套餐的周层带 unlimited 标记，
               以 ♾️ 文案呈现、不渲染进度条（避免「剩余 null%」与恒满格假数据）。 -->
          <template v-if="row.provider.kind === 'plan'">
            <div v-for="tier in row.tiers" :key="tier.name" class="sub-tier">
              <span class="sub-tier-name">{{ tierFullName(tier.name) }}</span>
              <!-- 缺席窗口：写明「暂无数据」，不补进度条也不补 100%。 -->
              <span v-if="tier.missing" class="muted sub-tier-reset" :title="tier.tip">暂无数据</span>
              <span v-else-if="tier.unlimited" class="sub-tier-unlimited">♾️ 无限周额度</span>
              <template v-else>
                <div class="sub-bar" role="img" :aria-label="tier.tip" :title="tier.tip">
                  <i :class="'sub-bar-fill level-' + tier.level" :style="{ width: tier.percent + '%' }"></i>
                </div>
                <span class="sub-tier-percent">剩余 {{ tier.percent }}%</span>
                <span class="muted sub-tier-reset">
                  {{ tier.countdown ? tier.countdown + '后重置' : '重置时间未知' }}
                </span>
              </template>
            </div>
            <p v-if="!row.tiers.length && !row.shortState" class="muted sub-empty">暂无窗口数据。</p>
          </template>

          <!-- 余额类（DeepSeek 多币种 / 2api.store 单行剩余额度）：主行总额 +
               赠金 / 充值明细行（2api 不返回明细，detail 为空）；is_available=false
               时单独标红，文案按 provider 语义区分（unavailableText）。 -->
          <template v-else-if="row.provider.kind === 'balance'">
            <div v-for="(item, index) in row.balances" :key="index" class="sub-balance" :title="item.tip">
              <span class="sub-balance-text">{{ item.main }}</span>
              <span v-if="item.detail" class="sub-balance-detail">{{ item.detail }}</span>
              <span v-if="row.provider.is_available === false" class="sub-balance-unavailable">
                {{ unavailableText(row.provider) }}
              </span>
            </div>
            <p v-if="!row.balances.length && !row.shortState" class="muted sub-empty">未查询到余额数据。</p>
          </template>

          <!-- 短状态词：完整可操作文案在上面错误横幅，这里不重复铺长文。
               OpenAI 例外（见 PartitionErrorTip）：长解释收进 ⓘ 点击弹出。 -->
          <PartitionErrorTip :provider="row.provider" />
        </section>
      </template>
    </main>

    <!-- 窗口状态条：吸附底部。查询时间坐标 + 模型用量窗口互跳入口。 -->
    <footer class="subwin-footer">
      <span class="subwin-footer-text">
        <template v-if="instanceText">{{ instanceText }} · </template>
        {{
          lastFetched
            ? `本次查询于 ${lastFetched}；失败时保留上次成功数据`
            : '失败时保留上次成功数据；凭据复用工作台模型设置'
        }}
      </span>
      <span class="subwin-footer-actions">
        <el-button text size="small" :icon="Setting" :loading="isLoading('openModelSettings')" @click="openModelSettings">
          前往模型设置
        </el-button>
        <el-button text size="small" :icon="TopRight" :loading="isLoading('openUsageWindow')" @click="openUsageWindow">
          本地 token 用量见模型用量窗口
        </el-button>
      </span>
    </footer>
  </ViewerShell>
</template>

<style scoped>
.age-pill {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  padding: 1px 8px;
  border: 1px solid var(--el-border-color-extra-light);
  border-radius: 999px;
  font-size: 11px;
  color: var(--text-secondary);
}
.age-pill .el-icon {
  font-size: 11px;
}

.subwin {
  height: 100vh;
  display: flex;
  flex-direction: column;
  background: var(--window);
}
.subwin-head {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 14px;
  border-bottom: 1px solid var(--border);
  flex: none;
}
.subwin-title {
  font-weight: 700;
  font-size: var(--fs-subtitle);
}
.subwin-info {
  color: var(--text-secondary);
  cursor: help;
}
.subwin-spacer {
  flex: 1;
}
.subwin-instance {
  color: var(--text-secondary);
  font-size: var(--fs-desc);
  margin-left: 8px;
  white-space: nowrap;
}
.subwin-footer-text {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.subwin-footer-actions {
  flex: none;
  display: inline-flex;
  align-items: center;
  gap: 4px;
}
.subwin-main {
  flex: 1;
  overflow-y: auto;
  padding: 14px 16px;
  display: flex;
  flex-direction: column;
  gap: 14px;
}
.sub-section {
  border: 1px solid var(--el-border-color-extra-light);
  border-radius: 10px;
  padding: 10px 12px;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.sub-section-head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 10px;
}
.sub-section-head h3 {
  margin: 0;
  font-size: var(--fs-subtitle);
}
.sub-section-hint {
  color: var(--text-secondary);
  font-size: 12px;
}
.sub-tier {
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 13px;
}
.sub-tier-name {
  flex: none;
  width: 90px;
  color: var(--text-secondary);
}
.sub-bar {
  flex: 1;
  height: 10px;
  border-radius: 5px;
  background: var(--overlay-soft);
  overflow: hidden;
}
.sub-bar-fill {
  display: block;
  height: 100%;
  border-radius: 5px;
}
/* 进度条三档配色（剩余口径）：≥70 绿 / 40–69.99 橙 / <39.99 红。 */
.sub-bar-fill.level-ok {
  background: var(--el-color-success);
}
.sub-bar-fill.level-warning {
  background: var(--el-color-warning);
}
.sub-bar-fill.level-danger {
  background: var(--el-color-danger);
}
.sub-tier-percent {
  flex: none;
  min-width: 64px;
  text-align: right;
  font-weight: 600;
}
.sub-tier-reset {
  flex: none;
  min-width: 108px;
  text-align: right;
}
.sub-tier-unlimited {
  flex: 1;
  font-weight: 600;
}
.sub-balance {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 4px 10px;
  font-size: 13px;
  padding: 2px 0;
}
.sub-balance-text {
  font-weight: 600;
}
/* 余额：总额与赠金 / 充值明细同行并排，告警仍靠右。 */
.sub-balance-detail {
  color: var(--text-secondary);
  font-size: 12px;
}
.sub-balance-unavailable {
  margin-left: auto;
  color: var(--el-color-danger);
  font-weight: 600;
}
.sub-empty {
  margin: 0;
  font-size: 12px;
}
/* 分区短状态样式随「查询异常 ⓘ」收进 PartitionErrorTip.vue（2026-10-10），
   这里不再保留 .sub-state 一族，避免两份漂移。 */
.subwin-footer {
  flex: none;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  padding: 7px 14px;
  border-top: 1px solid var(--border);
  background: var(--overlay-faint);
  color: var(--text-secondary);
  font-size: 12px;
}
.sub-reset-icon {
  font-size: 12px;
  vertical-align: -2px;
}
.sub-tier-reset {
  display: inline-flex;
  align-items: center;
  gap: 2px;
}
</style>
