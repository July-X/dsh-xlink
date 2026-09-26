<script setup>
// 独立套餐用量窗口：概览卡「套餐用量 → 查看详情」按钮经 open_subscription_window
// 弹出（URL ?subscription=1 挂载本组件，capability `subscription-viewer.json`
// 只授予 `get_subscription_usage` 与 `open_usage_window`）。
// 两个 provider 分区：MiniMax Token Plan 双窗口进度（剩余口径，三档配色）+
// DeepSeek 按量余额（多币种逐行，is_available=false 标红）。窗口打开即 force
// 全量刷新（subscription.js 的 refreshSubscription）；查询失败保留上次数据，
// 错误以横幅内联呈现。底部一行入口跳「模型用量」窗口（本地 token 统计）。
// 能力边界（设计稿）：两家云端 API 都不提供绝对剩余 token 数——MiniMax 只给
// 剩余百分比，DeepSeek 只有货币余额，不虚构任何 token 数字。
import { computed, onMounted, watchEffect } from 'vue';
import { Refresh, InfoFilled, TopRight, Setting, Timer } from '@element-plus/icons-vue';
import { ioActive, isLoading } from './loading.js';
import { invoke } from './bridge.js';
import { toastActionError } from './notify.js';
import { withLoading } from './loading.js';
import {
  subscription,
  refreshSubscription,
  providerShortState,
  tierRow,
  balanceText,
  queriedAtLabel,
} from './subscription.js';
import { openUsageWindow } from './usage.js';

const CAPABILITY_TIP =
  'MiniMax 展示 Token Plan 的 5 小时 / 周窗口剩余百分比（云端 API 不提供绝对剩余 token 数）；' +
  'DeepSeek 展示按量计费账户的货币余额；智谱 GLM 展示编程套餐的 5 小时 / 周窗口剩余百分比' +
  '（组织 / 项目上下文未配置时按错误提示补配）。数据约 5 分钟更新一次，「刷新」立即重新查询。';

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
    tiers: provider.kind === 'plan' ? provider.tiers.map((tier) => tierRow(tier)).filter(Boolean) : [],
    balances:
      provider.kind === 'balance' ? provider.balances.map((balance) => balanceText(balance)) : [],
    shortState: providerShortState(provider),
    queried: queriedAtLabel(provider),
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
  <div class="subwin">
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
            <h3>{{ row.provider.label }}{{ row.provider.kind === 'plan' ? ' Token Plan' : ' 按量余额' }}</h3>
            <span class="sub-section-hint">
              {{ row.queried ? `查询于 ${row.queried}` : '尚未查询成功' }}
            </span>
          </div>

          <!-- MiniMax：5h / 周窗口进度。周窗口未激活的套餐不渲染（避免恒满格假数据）。 -->
          <template v-if="row.provider.kind === 'plan'">
            <div v-for="tier in row.tiers" :key="tier.name" class="sub-tier">
              <span class="sub-tier-name">{{ tier.name }}</span>
              <div class="sub-bar" role="img" :aria-label="tier.tip" :title="tier.tip">
                <i :class="'sub-bar-fill level-' + tier.level" :style="{ width: tier.percent + '%' }"></i>
              </div>
              <span class="sub-tier-percent">剩余 {{ tier.percent }}%</span>
              <span class="muted sub-tier-reset">
                {{ tier.countdown ? tier.countdown + '后重置' : '重置时间未知' }}
              </span>
            </div>
            <p v-if="!row.tiers.length && !row.shortState" class="muted sub-empty">暂无窗口数据。</p>
          </template>

          <!-- DeepSeek：多币种余额逐行；is_available=false 时单独标红。 -->
          <template v-else-if="row.provider.kind === 'balance'">
            <div v-for="(text, index) in row.balances" :key="index" class="sub-balance">
              <span class="sub-balance-text">{{ text }}</span>
              <span v-if="row.provider.is_available === false" class="sub-balance-unavailable">
                余额不足，无法发起调用
              </span>
            </div>
            <p v-if="!row.balances.length && !row.shortState" class="muted sub-empty">未查询到余额数据。</p>
          </template>

          <!-- 短状态词：完整可操作文案在上面错误横幅，这里不重复铺长文。 -->
          <p
            v-if="row.shortState"
            class="sub-state"
            :class="{
              'sub-state-bad':
                row.provider.fetch_error || row.provider.credential_status === 'expired' || row.provider.error,
            }"
          >
            {{ row.shortState }}
          </p>
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
  </div>
</template>

<style scoped>
.subwin {
  height: 100vh;
  display: flex;
  flex-direction: column;
  background: var(--bg);
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
  font-size: 15px;
}
.subwin-info {
  color: var(--muted);
  cursor: help;
}
.subwin-spacer {
  flex: 1;
}
.subwin-instance {
  color: var(--muted);
  font-size: 12px;
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
  font-size: 14px;
}
.sub-section-hint {
  color: var(--muted);
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
  width: 76px;
  color: var(--muted);
}
.sub-bar {
  flex: 1;
  height: 10px;
  border-radius: 5px;
  background: rgba(255, 255, 255, 0.08);
  overflow: hidden;
}
.sub-bar-fill {
  display: block;
  height: 100%;
  border-radius: 5px;
}
/* 进度条三档配色：按「剩余」百分比（与已用口径相反）。 */
.sub-bar-fill.level-ok {
  background: var(--accent);
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
  justify-content: space-between;
  gap: 10px;
  font-size: 13px;
  padding: 2px 0;
}
.sub-balance-text {
  font-weight: 600;
}
.sub-balance-unavailable {
  color: var(--el-color-danger);
  font-weight: 600;
}
.sub-empty {
  margin: 0;
  font-size: 12px;
}
.sub-state {
  margin: 0;
  font-size: 12px;
  color: var(--muted);
}
.sub-state-bad {
  color: var(--el-color-danger);
}
.subwin-footer {
  flex: none;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  padding: 7px 14px;
  border-top: 1px solid var(--border);
  background: rgba(0, 0, 0, 0.18);
  color: var(--muted);
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
