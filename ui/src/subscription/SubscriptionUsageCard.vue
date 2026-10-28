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
      <span v-if="anyKeyConfigured" class="plan-head-actions">
        <el-button
          class="btn-action"
          round
          size="small"
          :icon="Refresh"
          :loading="subscription.loading"
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
    <p v-if="!anyKeyConfigured" class="muted" style="margin: 0">
      当前实例未配置可查询的模型凭据；到工作台的模型设置配置后，这里展示套餐剩余额度与余额。
    </p>
    <div v-else class="plan-body">
      <el-alert
        v-for="(error, index) in subscription.errors"
        :key="index"
        :title="error"
        type="warning"
        :closable="false"
        show-icon
        class="plan-error"
      />
      <!-- 2026-10-07 按设计稿改成单一三列栅格：原先是「余额类竖着列一块 +
           套餐类再进一个自适应栅格」两个循环，同一张卡里出现两种排布规则，
           新增 provider 时还要决定它进哪个循环。planRows 本来就同时含两类，
           一个循环按 kind 分支渲染就够，视觉规则也统一成「一格一家」。 -->
      <div class="plan-grid">
        <div v-for="row in planRows" :key="row.provider.id" class="plan-provider">
          <div class="plan-provider-head">
            <!-- 厂商标志是纯装饰，标题本身已经写明是谁，所以 alt 给空：让读屏
                 的人只听到一次「MiniMax-CN」，而不是先听一遍 logo 的
                  aria-label 再听一遍标题。圆形底的底色与每个 provider 的品牌
                 色相位在下方 .plan-provider-logo 家族里。 -->
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
            <button
              type="button"
              class="age-pill age-pill-btn"
              :title="isProviderRefreshing(row.provider.id) ? '正在查询…' : '查询于 ' + row.queried + '，点击只刷新 ' + row.provider.label"
              :disabled="isProviderRefreshing(row.provider.id)"
              @click="onRefreshProvider(row.provider.id)"
            >
              <el-icon :class="{ 'is-loading': isProviderRefreshing(row.provider.id) }"><Refresh /></el-icon>{{ row.queriedCompact }}
            </button>
          </div>
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
          <!-- 设计稿 `.usage-tier-row`：`名称 22px | 进度条 1fr | 百分比 34px`
               一行栅格，条高 6px、百分比在条外右对齐。原先这里是「名称/倒计时
               一行 + 10px 进度条独占一行 + 百分比压在条中央」，两行占的高度
               是设计稿的两倍多。倒计时保留，但降级成条下的一行小字——
               它是次要读数，不该和额度条抢同一行的视觉权重。 -->
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
              <!-- 缺席窗口：写明「暂无数据」，不补进度条也不补 100%。 -->
              <span v-if="tier.missing" class="muted plan-tier-value" :title="tier.tip">暂无数据</span>
              <span v-else-if="!tier.unlimited" class="plan-tier-value">{{ tier.percent }}%</span>
            </div>
            <!-- 条下这行除了倒计时，还给出「已用 X%」：进度条与主数字是**剩余**
                 口径，而厂商自己的界面基本都写已用（opencode 的界面上就是
                 `1% used`）。只给剩余时，用户拿自家界面一比就会以为我们算错了
                 ——2026-10-13 用户报的就是这个：他看的是界面的「已用 1%」，
                 卡上写的是「100%」。（opencode 端点的整数截断见 subscription.js
                 里那条 OPENCODE_INTEGER_PERCENT_TIP。） -->
            <p
              v-if="tier.countdown || tier.usedPercent != null"
              class="plan-tier-reset"
              :title="tier.countdownTitle"
            >
              <el-icon v-if="tier.countdown" class="plan-reset-icon"><Timer /></el-icon>
              <span v-if="tier.countdown">{{ tier.countdown }}</span>
              <span v-if="tier.countdown && tier.usedPercent != null">·</span>
              <span v-if="tier.usedPercent != null">已用 {{ tier.usedPercent }}%</span>
            </p>
          </div>
          <!-- 错误归属分区的 provider（OpenAI）的短状态 + ⓘ 点击 tooltip；
               其余 provider 的完整文案仍走顶部横幅。 -->
          <PartitionErrorTip :provider="row.provider" />
        </div>
      </div>
      <p v-if="allHidden" class="muted" style="margin: 0">
        查不到数据的分区已按你的选择隐藏；修复凭据并成功查询（或重启后自动首查）后会自动恢复。
      </p>
      <p v-else-if="!planRows.length" class="muted" style="margin: 0">尚未查询，点击右上角「刷新」获取。</p>
    </div>
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
  font-size: 11px;
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
.plan-provider {
  display: flex;
  flex-direction: column;
  gap: 2px;
  /* 分块展示：描边 + 微底色 + 圆角，与独立窗口的 provider 分区同语言。
     8px 圆角 / 8px 内边距：设计稿 `.usage-item` 原为 6px 圆角 + 10px 内边距，
     2026-10-10 收间距时内边距 10 → 8（第四家 provider 折行后整页超高）。 */
  border: 1px solid var(--el-border-color-extra-light);
  border-radius: 6px;
  padding: 8px;
  background: var(--el-fill-color-light);
}
/* 设计稿 `.usage-grid` 是固定三列，不是自适应。1040 宽版下内容列约 790px，
   `auto-fill minmax(170px,1fr)` 会排成四列，一家的「余额 / 额度条」被摊薄，
   与稿子的三列节奏不符。固定三列 + 溢出换行，同一屏的横向对比关系才是
   设计稿画的那一屏。 */
.plan-grid {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 6px;
}
.plan-grid .plan-provider {
  min-width: 0;
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
  font-size: 10px;
  font-weight: 600;
}
.plan-tier-unlimited {
  grid-column: 2 / -1;
  font-size: 11px;
  font-weight: 600;
}
.plan-tier-value {
  min-width: 34px;
  color: var(--text);
  font-size: 10px;
  font-weight: 650;
  text-align: right;
}
.plan-provider-head {
  display: flex;
  /* 圆形 logo 底与文字标题不是基线关系：按 baseline 对齐会把圆形块的底边压到
     文字基线上，整块看上去往下坠了一截。圆形是居中对齐的，标题与刷新药丸各自
     贴它两侧。 */
  align-items: center;
  justify-content: space-between;
  flex-wrap: wrap;
  gap: 4px 8px;
  line-height: 1.4;
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
  font-size: 13px;
}
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
.plan-tier-unlimited {
  font-weight: 600;
}
.plan-tier-reset {
  display: inline-flex;
  align-items: center;
  gap: 2px;
  margin: 0;
  /* 行高钉 1.25：10px 的倒计时小字按继承行高渲染约 15px，四个倒计时行
     （MiniMax / Codex 各两层）累计把分区顶高近 10px（2026-10-10 收间距）。 */
  line-height: 1.25;
  color: var(--text-muted);
  font-size: 10px;
}
.plan-reset-icon {
  font-size: 11px;
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
  font-size: 10px;
}
.plan-balance-total {
  color: var(--text);
  font-size: 14px;
  font-weight: 700;
}
/* 余额：总额与赠金 / 充值明细同行并排（gap 隔开），告警仍靠右。 */
.plan-balance-unavailable {
  margin-left: auto;
  color: var(--el-color-danger);
  font-weight: 600;
  font-size: 10px;
}
.plan-balance-detail {
  display: flex;
  justify-content: space-between;
  gap: 8px;
  margin-top: 7px;
  color: var(--text-secondary);
  font-size: 10px;
}
/* 分区短状态样式随「查询异常 ⓘ」收进 PartitionErrorTip.vue（2026-10-10），
   这里不再保留 .plan-state 一族，避免两份漂移。 */
</style>
