<script setup>
// 「套餐用量」卡（2026-10-28 从概览页整体迁到「用量分析」面板上方）。
//
// 卡片内容直接展示（无折叠）。套餐类画窗口进度（MiniMax / 智谱 / OpenAI /
// opencode），余额类画金额行（DeepSeek / 2api.store）；完整可操作错误文案只在
// 顶部横幅出现，provider 分区仅用短状态词标注。凭据复用工作台模型设置，这张
// 卡不带任何写操作。
//
// **「查看详情」入口已移除**：用户要求把弹窗里的详细页并进「用量分析」页，独立
// 「套餐用量」窗口的组件、Rust 命令与 capability 仍保留（入口隐藏 ≠ 能力删除）。
import CodexUsageConsent from './CodexUsageConsent.vue';
import PartitionErrorTip from './PartitionErrorTip.vue';
import { computed, onMounted, watch } from 'vue';
import { Coin, InfoFilled, Refresh, Setting, Timer } from '@element-plus/icons-vue';
import { openHarnessWindow } from '../store.js';
import {
  subscription,
  loadSubscriptionSummary,
  refreshSubscription,
  refreshSubscriptionProvider,
  isProviderRefreshing,
  isProviderHidden,
  hideProvider,
  providerShortState,
  failurePromptPending,
  markFailurePrompted,
  planTierRows,
  balanceRow,
  unavailableText,
  queriedAtLabel,
  queriedAgeCompact,
  CODEX_USAGE_CONSENT_TIP,
  providerLogo,
} from './subscription.js';
import { openaiBuiltin, loadBuiltinStatus } from '../plugins/builtin.js';
import { confirmDialog } from '../shell/notify.js';

// 凭据未配置时只给「前往模型设置」入口。
const anyKeyConfigured = computed(() =>
  ((subscription.data && subscription.data.providers) || []).some((provider) => provider.configured)
);
// 云端余额属于哪个内核实例 / profile：同一台机器可能有多份时，不写清就会把
// 「另一个实例的余额」当成当前账号的读数（2026-10-28 自独立窗口并入本卡）。
const instanceLabel = computed(() => {
  const info = subscription.data && subscription.data.instance;
  return info ? `${info.family}/${info.id} · profile ${info.profile}` : '';
});
// Codex 额度入口（开关 + 它的整宽说明句）是 OpenAI 插件的功能面，与 Rust 侧
// 分区门禁同源：插件停用时一并收起（2026-10-10）。开关组件自己按
// openaiBuiltin 收起，这里的说明句在卡头下面另起一行，得跟着同一个判据。
const codexConsentVisible = computed(() => openaiBuiltin?.requestedEnabled === true);
onMounted(() => {
  // 未配置时不发请求：Rust 侧对未配置 provider 也只做凭据解析，不产生网络调用，
  // 但首次进入总得拉一次才知道配置状态——由 loadSubscriptionSummary 自行决定。
  loadSubscriptionSummary();
  loadBuiltinStatus();
});
// 工作台里改完凭据回到本页：数据拉取动作自带 TTL，这里不额外触发。
// planRows 额外滤掉「用户选择隐藏」的分区（key 配了但一直查不到数据）。
// 余额分区在本卡的形状：设计稿 `.usage-balance-row`（「余额」标签 + 右侧
// 14px 粗体总额）与 `.usage-balance-detail`（赠金 / 充值两枚并排小字）分两行，
// 总额是这一格的主读数。`balanceRow` 返回的是给独立窗口用的单行文本
// （`余额：¥16.62` / `赠金 ¥8.00 · 充值 ¥8.62`），这里按它自己定下的分隔符
// 拆回结构化字段——分隔符与拆分同在 subscription.js，改那边要一起改这里。
function balanceCardRow(balance) {
  const row = balanceRow(balance);
  if (!row) return null;
  const [label, ...rest] = row.main.split('：');
  return {
    tip: row.tip,
    label,
    total: rest.join('：'),
    parts: row.detail ? row.detail.split(' · ') : [],
  };
}
const planRows = computed(() =>
  ((subscription.data && subscription.data.providers) || [])
    .filter((provider) => provider.configured && !isProviderHidden(provider.id))
    .map((provider) => ({
      provider,
      tiers: provider.kind === 'plan' ? planTierRows(provider) : [],
      balances:
        provider.kind === 'balance'
          ? provider.balances.map((balance) => balanceCardRow(balance)).filter(Boolean)
          : [],
      shortState: providerShortState(provider),
      queried: queriedAtLabel(provider),
      queriedCompact: queriedAgeCompact(provider),
    }))
);
// 2026-10-07：原先按 kind 拆成 balanceRows（竖列）与 planBlockRows（栅格）
// 两个 computed，模板里两个循环各写一遍 provider 头。设计稿的用法是「一格一家」，
// 两个循环合成 planRows 一个即可，balanceRows / planBlockRows 随之删除。
// 所有 configured 分区都被隐藏时给出一句说明，而不是误导性的「尚未查询」。
const allHidden = computed(
  () =>
    ((subscription.data && subscription.data.providers) || []).some((p) => p.configured) &&
    planRows.value.length === 0 &&
    ((subscription.data && subscription.data.providers) || []).some((p) => isProviderHidden(p.id))
);
function onRefreshPlan() {
  refreshSubscription();
}
// 分区标题旁的刷新 icon：只重新查询这一个 provider（在途去重 + 旋转加载态）。
function onRefreshProvider(id) {
  refreshSubscriptionProvider(id);
}
// 年龄胶囊的悬停说明：从未查成功过时不能写「查询于 null」，那会读成一条坏数据
// （独立窗口里本来就有一句「尚未查询成功」，并卡时漏掉了）。
function agePillTitle(row) {
  if (isProviderRefreshing(row.provider.id)) return '正在查询…';
  return row.queriedCompact
    ? `查询于 ${row.queried}，点击只刷新 ${row.provider.label}`
    : `尚未查询成功，点击查询 ${row.provider.label}`;
}

// 「查不到数据 → 是否隐藏」提示：configured 但处于失败态的分区，本会话内
// 只问一次；确认后隐藏（localStorage 记住，查询成功自动恢复），取消则不再问。
watch(
  () => subscription.data,
  async (view) => {
    for (const provider of (view && view.providers) || []) {
      if (!failurePromptPending(provider)) continue;
      markFailurePrompted(provider.id);
      const reason = providerShortState(provider) || '查询失败';
      const hide = await confirmDialog(
        '套餐用量',
        `${provider.label} 无法取到数据（${reason}）。是否隐藏该项？` +
          '隐藏后不会再展示与报错，下次查询成功（包括重启后的首次查询）会自动恢复显示。',
        '隐藏',
        '保留'
      );
      if (hide) hideProvider(provider.id);
    }
  }
);
</script>

<template>
  <div class="card usage-card">
    <div class="card-head">
      <h2>
        <span class="card-title-icon" aria-hidden="true"><el-icon><Coin /></el-icon></span>
        套餐用量
        <el-tooltip placement="bottom-start" :show-after="80">
          <template #content>
            <div class="card-info-tooltip">
              MiniMax（国内站 / 国际站）与智谱的 5 小时 / 周窗口余额、DeepSeek 按量余额（多币种）、
              opencode 的 Go 套餐三窗口余额，以及 2api.store 自定义 provider 的剩余额度（来自 /v1/usage）。
              数据缓存 5 分钟，点「刷新」立即重新查询。凭据复用工作台模型设置；
              未在内核配置对应厂商时，相应分区自动隐藏。
              OpenAI / Codex 额度分区与「使用本机 Codex 登录查询额度」开关，仅在启用 OpenAI-OAuth-Plugin 后显示。
              <template v-if="instanceLabel">
                当前查询实例：{{ instanceLabel }}；查询失败时保留上一次成功的数据。
              </template>
            </div>
          </template>
          <el-icon class="card-info-icon"><InfoFilled /></el-icon>
        </el-tooltip>
      </h2>
      <!-- Codex 额度开关进卡头：标题在左，开关与「刷新」同排。原先它独占卡头
           下面一整行，把「默认关闭…」那句许可说明顶到卡片中间、说明与开关之间
           隔着一段空白——而开关只有一个 `active-text`，脱离说明之后它读起来像
           「一个和额度有关的开关」，不知道开启意味着只读登录文件。许可句改由
           下面的整宽一行给出。 -->
      <CodexUsageConsent inline />
      <span v-if="anyKeyConfigured || subscription.initialLoading || subscription.errors.length" class="plan-head-actions">
        <el-button
          class="btn-action"
          round
          size="small"
          :icon="Refresh"
          :loading="subscription.loading || subscription.initialLoading"
          title="立即重新查询（越过 5 分钟缓存）"
          @click="onRefreshPlan"
        >
          刷新
        </el-button>
      </span>
      <el-button
        v-else
        text
        size="small"
        type="primary"
        :icon="Setting"
        title="凭据在工作台的模型设置里配置（外壳不保存任何 Key）"
        @click="openHarnessWindow"
      >
        前往模型设置
      </el-button>
    </div>
    <p v-if="codexConsentVisible" class="muted usage-consent-tip">{{ CODEX_USAGE_CONSENT_TIP }}</p>
    <el-alert
      v-for="(error, index) in subscription.errors"
      :key="index"
      :title="error"
      type="warning"
      :closable="false"
      show-icon
      class="plan-error"
    />
    <!-- 首次加载可能连着打几个云端接口：先摆骨架，等数据回来再淡入内容。
         没有这条时卡片会把「还没回答」画成「未配置凭据」，看起来像卡住了。 -->
    <Transition name="plan-swap" mode="out-in">
      <div
        v-if="subscription.initialLoading && !subscription.data"
        key="skeleton"
        class="plan-table plan-skeleton"
        aria-busy="true"
        aria-label="正在加载套餐用量"
      >
        <div v-for="i in 3" :key="i" class="plan-provider plan-skeleton-cell">
          <span class="plan-skel-line plan-skel-title"></span>
          <span class="plan-skel-line"></span>
          <span class="plan-skel-line plan-skel-short"></span>
        </div>
      </div>
      <div v-else-if="!anyKeyConfigured" key="empty" class="plan-empty">
        <p v-if="!subscription.errors.length" class="muted" style="margin: 0">
          当前实例未配置可查询的模型凭据；到工作台的模型设置配置后，这里展示套餐剩余额度与余额。
        </p>
      </div>
      <div v-else key="body" class="plan-body">
      <!-- 2026-10-07 按设计稿改成单一三列栅格：原先是「余额类竖着列一块 +
           套餐类再进一个自适应栅格」两个循环，同一张卡里出现两种排布规则，
           新增 provider 时还要决定它进哪个循环。planRows 本来就同时含两类，
           一个循环按 kind 分支渲染就够，视觉规则也统一成「一格一家」。 -->
      <div class="plan-table">
        <div v-for="row in planRows" :key="row.provider.id" class="plan-provider">
          <div class="plan-provider-vendor">
            <!-- 厂商标志是纯装饰，标题本身已经写明是谁，所以 alt 给空。 -->
            <span class="plan-provider-id">
              <span
                v-if="providerLogo(row.provider.id)"
                class="plan-provider-logo"
                :class="'plan-provider-logo--' + row.provider.id"
                aria-hidden="true"
              >
                <img class="plan-provider-mark" :src="providerLogo(row.provider.id)" alt="" />
              </span>
              <span class="plan-provider-name">{{ row.provider.label }}</span>
            </span>
            <!-- 实测（2026-11-10 用户截图）：刷新按钮跟厂商名同列读起来更顺，
                 右侧时间列只留重置/更新时间。 -->
            <button
              type="button"
              class="age-pill age-pill-btn plan-provider-refresh"
              :title="agePillTitle(row)"
              :disabled="isProviderRefreshing(row.provider.id)"
              @click="onRefreshProvider(row.provider.id)"
            >
              <el-icon :class="{ 'is-loading': isProviderRefreshing(row.provider.id) }"><Refresh /></el-icon>{{ row.queriedCompact || '尚未查询' }}
            </button>
          </div>
          <div class="plan-provider-content">
            <template v-if="row.provider.kind === 'balance'">
              <div v-for="(item, index) in row.balances" :key="index" :title="item.tip">
                <div class="plan-balance">
                  <span class="plan-balance-label">{{ item.label }}</span>
                  <strong class="plan-balance-total">{{ item.total }}</strong>
                  <span v-if="row.provider.is_available === false" class="plan-balance-unavailable">
                    {{ unavailableText(row.provider) }}
                  </span>
                </div>
                <div v-if="item.parts.length" class="plan-balance-detail">
                  <span v-for="part in item.parts" :key="part">{{ part }}</span>
                </div>
              </div>
            </template>
            <!-- 每个时间窗口独立一行：名称 / 进度条 / 剩余百分比。条高 6px，
                 百分比在条外；「已用」收进 title / aria-label，不再占第二行。 -->
            <div v-for="tier in row.tiers" :key="tier.name" class="plan-tier-col">
              <div class="plan-tier-row">
                <span class="plan-tier-name">{{ tier.name }}</span>
                <div
                  v-if="!tier.unlimited && !tier.missing"
                  class="plan-bar"
                  role="img"
                  :aria-label="tier.tip"
                  :title="tier.tip"
                >
                  <i :class="'plan-bar-fill level-' + tier.level" :style="{ width: tier.percent + '%' }"></i>
                </div>
                <span v-else-if="tier.unlimited" class="plan-tier-unlimited">♾️ 无限周额度</span>
                <span v-if="tier.missing" class="muted plan-tier-value" :title="tier.tip">暂无数据</span>
                <span v-else-if="!tier.unlimited" class="plan-tier-value" :title="tier.tip">剩余 {{ tier.percent }}%</span>
              </div>
            </div>
            <!-- 错误归属分区的 provider（OpenAI）的短状态 + ⓘ 点击 tooltip；
                 其余 provider 的完整文案仍走顶部横幅。 -->
            <PartitionErrorTip :provider="row.provider" />
          </div>
          <div class="plan-provider-time">
            <span
              v-for="tier in row.tiers"
              :key="'reset-' + tier.name"
              class="plan-provider-reset"
              :title="tier.countdownTitle"
            >
              {{ tier.name }}<template v-if="tier.countdown"> · {{ tier.countdown }}</template>
            </span>
          </div>
        </div>
      </div>
      <p v-if="allHidden" class="muted" style="margin: 0">
        查不到数据的分区已按你的选择隐藏；修复凭据并成功查询（或重启后自动首查）后会自动恢复。
      </p>
      <p v-else-if="!planRows.length" class="muted" style="margin: 0">尚未查询，点击右上角「刷新」获取。</p>
    </div>
    </Transition>
  </div>
</template>

<style scoped>
/* 本卡沿用概览页的紧凑内边距：整页只有这一张卡，默认 16px 会把它撑得比
   原概览里松一档，读数与进度条的密度会变。 */
.card {
  padding: 6px 8px;
  gap: 4px;
}
/* 分区刷新按钮：复用年龄胶囊外观，但可点击；禁用（查询中）降透明度。 */
.age-pill-btn {
  border: none;
  background: none;
  padding: 0;
  font: inherit;
  color: inherit;
  cursor: pointer;
}
.age-pill-btn:disabled {
  cursor: default;
  opacity: 0.6;
}
/* 套餐用量卡头右侧的按钮组（只剩刷新；「查看详情」已随入口移除删除）。 */
.plan-head-actions {
  margin-left: auto;
  display: inline-flex;
  align-items: center;
  gap: 4px;
}
/* 卡头下面整宽一行的许可说明。字号 11px（辅助说明档，规范 §2），与下面
   provider 卡片之间留 4px——它解释的是上面那个开关，不是下面这些读数，
   不该与读数共享一段纵向间距。行高 1.6 → 1.4（2026-10-10 收间距）。 */
.usage-consent-tip {
  margin: 0;
  font-size: var(--fs-5);
  line-height: 1.4;
}
/* 套餐用量卡内容：横幅 + provider 分区的纵向间距。8 → 6（2026-10-10 收间距）。 */
.plan-body {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.plan-error {
  --el-alert-padding: 6px 10px;
}
/* 合同 §8.2：整宽横向行，厂商 180 / 内容 1fr / 重置与更新时间 210。 */
.plan-provider {
  display: grid;
  grid-template-columns: 180px minmax(200px, 1fr) 210px;
  align-items: start;
  gap: 16px;
  min-height: 64px;
  padding: 12px 0;
  border-bottom: 1px solid var(--border-soft);
}
.plan-provider:last-child { border-bottom: 0; }
.plan-provider-vendor { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; min-width: 0; }
/* 刷新胶囊跟着厂商名，不抢右侧重置/时间列。 */
.plan-provider-refresh { flex: none; }
.plan-provider-content { display: flex; flex-direction: column; gap: 6px; min-width: 0; }
.plan-provider-time {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 2px;
  min-width: 0;
  color: var(--text-muted);
  font-size: var(--fs-4);
  text-align: right;
}
.plan-provider-reset { white-space: nowrap; }

@container (max-width: 719px) {
  .plan-provider { grid-template-columns: minmax(0, 1fr) auto; }
  .plan-provider-content { grid-column: 1 / -1; }
  .plan-provider-time { grid-column: 2; grid-row: 1; }
}
/* 设计稿 `.usage-grid` 是固定三列，不是自适应。1040 宽版下内容列约 790px，
   `auto-fill minmax(170px,1fr)` 会排成四列，一家的「余额 / 额度条」被摊薄，
   与稿子的三列节奏不符。固定三列 + 溢出换行，同一屏的横向对比关系才是
   设计稿画的那一屏。 */
.plan-table { display: flex; flex-direction: column; }
.usage-card { container-type: inline-size; }
/* 首次加载骨架：与三列 plan-grid 同构，数据回来时整块淡出、内容淡入。 */
.plan-skeleton-cell {
  gap: 8px;
}
.plan-skel-line {
  display: block;
  height: 10px;
  border-radius: 4px;
  background: var(--border-soft);
  animation: plan-skel-pulse 1.1s ease-in-out infinite;
}
.plan-skel-title {
  width: 46%;
  height: 12px;
}
.plan-skel-short {
  width: 64%;
}
.plan-swap-enter-active,
.plan-swap-leave-active {
  transition: opacity 0.18s ease, transform 0.18s ease;
}
.plan-swap-enter-from,
.plan-swap-leave-to {
  opacity: 0;
  transform: translateY(2px);
}
@keyframes plan-skel-pulse {
  0%,
  100% {
    opacity: 0.45;
  }
  50% {
    opacity: 0.9;
  }
}
@media (prefers-reduced-motion: reduce) {
  .plan-skel-line {
    animation: none;
    opacity: 0.7;
  }
  .plan-swap-enter-active,
  .plan-swap-leave-active {
    transition: none;
  }
}
/* 窄块内的 tier：设计稿 `.usage-tier-row` 的一行栅格。 */
.plan-tier-col {
  display: flex;
  flex-direction: column;
  gap: 1px;
}
.plan-tier-row {
  display: grid;
  grid-template-columns: 22px minmax(0, 1fr) auto;
  align-items: center;
  gap: 7px;
  min-height: 12px;
}
.plan-tier-row .plan-bar {
  margin-top: 0;
}
.plan-tier-name {
  color: var(--text-muted);
  font-size: var(--fs-4);
  font-weight: 600;
}
.plan-tier-unlimited {
  grid-column: 2 / -1;
  font-size: var(--fs-5);
  font-weight: 600;
}
.plan-tier-value {
  min-width: 34px;
  color: var(--text);
  font-size: var(--fs-4);
  font-weight: 650;
  text-align: right;
}
/* 标题与 logo 要占住卡头的左半格，所以外面套一层：没有 logo 的 provider（新增的
   厂商还没登记标志）也得让刷新药丸继续贴右边缘。 */
.plan-provider-id {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}
/* 设计稿每格标题前是一个品牌色圆形底：底色取各厂商自己的品牌色相，浅色下压到
   12% 混进卡片底、暗色下提到 22% 混进深色卡片底（暗色主题要更实一点才看得出
   「有一块底」）。色值以 `R, G, B` 三元组写在各家的修饰类里，是为了让同一套
   alpha 能被明暗两条规则共用——直接写 rgba 就得整份复制一遍。 */
.plan-provider-logo {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex: none;
  width: 26px;
  height: 26px;
  border-radius: 50%;
  background: rgba(var(--logo-rgb, 46, 118, 246), 0.12);
}
html.dark .plan-provider-logo {
  background: rgba(var(--logo-rgb, 74, 145, 255), 0.22);
}
/* 图形自己带类名、不写成 `.plan-provider-logo img`：主语只有 `img` 的规则会被
   「主体 token 全在元素身上就算命中」那条判据算到任何一个 <img> 头上，2026-10-10
   就因此把侧栏鲸鱼的 `.brand img { width: 42px }` 顶成了 16px。 */
.plan-provider-mark {
  display: block;
  width: 16px;
  height: 16px;
}
/* OpenAI 的原文件是 currentColor，经 <img> 引用时解析成黑色，暗色主题的深色
   chip 底上直接看不见；换成同路径的纯白版本。换图放在这里而不是模板的
   `:src` 上，是因为 src 换图要让整棵组件树跟着主题重渲染，而 content 只是
   改一条 CSS 声明——侧栏鲸鱼标（theme.css 的 .brand img）也是这么做的。 */
html.dark .plan-provider-logo--openai_codex .plan-provider-mark {
  content: url("/openai-logo-dark.svg");
}
/* opencode 的原文件同样是 currentColor（品牌本身也是黑白），暗色下换纯白那份。 */
html.dark .plan-provider-logo--opencode_go .plan-provider-mark {
  content: url("/opencode-logo-dark.svg");
}
/* 2api.store（TokenShop）的标志自带青色渐变方底（官网原图即完整 app 图标），
   圆底取同一档品牌青让它与图形边缘接得上；图形本体在明暗两套主题下都清晰。 */
.plan-provider-logo--api2store {
  --logo-rgb: 6, 182, 212;
}
.plan-provider-logo--deepseek {
  --logo-rgb: 77, 107, 254;
}
.plan-provider-logo--minimax_cn,
.plan-provider-logo--minimax_en {
  --logo-rgb: 226, 22, 126;
}
.plan-provider-logo--openai_codex {
  --logo-rgb: 109, 92, 255;
}
/* opencode 没有第二套品牌色（标志本身是黑白），圆底取中性石板灰：明暗两套
   都是「一层看不出色相的底」，让白色 / 近黑的图形自己立住。 */
.plan-provider-logo--opencode_go {
  --logo-rgb: 94, 100, 118;
}
.plan-provider-logo--zai_coding_cn {
  --logo-rgb: 56, 89, 255;
}
.plan-provider-name {
  font-weight: 600;
  font-size: var(--fs-5);
}
.age-pill {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  padding: 1px 8px;
  border: 1px solid var(--el-border-color-extra-light);
  border-radius: 999px;
  font-size: var(--fs-5);
  color: var(--text-secondary);
}
.age-pill .el-icon {
  font-size: var(--fs-5);
}
.plan-tier-unlimited {
  font-weight: 600;
}
.plan-bar {
  position: relative;
  /* 不能写 flex: 1：.plan-tier-row 是 grid，flex 属性不生效；这里只需要撑满
     栅格给的 1fr 列。高度给设计稿的 6px——百分比已经移到条外，条内无文字，
     10px 那条细带是给「条内压字」找的补偿，去掉压字后应当回到 6px。 */
  width: 100%;
  height: 6px;
  border-radius: 3px;
  background: var(--border-soft);
  overflow: hidden;
}
.plan-bar-fill {
  display: block;
  height: 100%;
  border-radius: 4px;
}
/* 进度条三档配色（剩余口径）：≥70 绿 / 40–69.99 橙 / <39.99 红。
   三档同一来源：写死的 #15803d 只在浅色下对，暗色主题下换成
   `--el-color-success`（与另两档的 --el-color-* 同一模式）。 */
.plan-bar-fill.level-ok {
  background: var(--el-color-success);
}
.plan-bar-fill.level-warning {
  background: var(--el-color-warning);
}
.plan-bar-fill.level-danger {
  background: var(--el-color-danger);
}
.plan-balance {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 8px;
  margin-top: 10px;
}
/* 总额是这一格的主读数：14px 粗体。标签与明细统一 10px 次级灰。 */
.plan-balance-label {
  color: var(--text-muted);
  font-size: var(--fs-4);
}
.plan-balance-total {
  color: var(--text);
  font-size: var(--fs-5);
  font-weight: 700;
}
/* 余额：总额与赠金 / 充值明细同行并排（gap 隔开），告警仍靠右。 */
.plan-balance-unavailable {
  margin-left: auto;
  color: var(--el-color-danger);
  font-weight: 600;
  font-size: var(--fs-4);
}
.plan-balance-detail {
  display: flex;
  justify-content: space-between;
  gap: 8px;
  margin-top: 7px;
  color: var(--text-secondary);
  font-size: var(--fs-4);
}
/* 分区短状态样式随「查询异常 ⓘ」收进 PartitionErrorTip.vue（2026-10-10），
   这里不再保留 .plan-state 一族，避免两份漂移。 */
</style>
