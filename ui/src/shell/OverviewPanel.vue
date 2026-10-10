<script setup>
import CodexUsageConsent from '../subscription/CodexUsageConsent.vue';
import PartitionErrorTip from '../subscription/PartitionErrorTip.vue';
// 概览：当前内核状态、工作台启停单按钮状态机、首次运行引导、
// 外壳更新横幅与安装入口（手动检查在侧栏品牌区）以及启动容错横幅。
// 内核生命周期是实现细节，只暴露「打开/关闭工作台 / 打开/关闭官方对话」；
// 日志入口不在这里，在「系统健康 → 日志系统」那一行（ControlTower）。
// 「打开工作台窗口 / 打开官方对话窗口」在对应服务开启后作为次级入口从第二行动态浮现。
// 「当前内核」的 Node.js 行另带「重新检测」（探测本机环境，不改设置）；Node
// 环境结论悬浮在卡标题旁的 ℹ️ 上（原「桌面端设置」卡已并入这个 tooltip）。
// 活动版本徽标里的 tag 图标是刻意的：内核版本按 git tag 发版（tag 格式
// desktop-v<version>），标成「标签」比裸版本号贴切，也与「内核版本」页对得上。
import { computed, onMounted, ref, watch, onUnmounted } from 'vue';
import ControlTower from '../diagnostics/ControlTower.vue';
import {
  loadRecentRuns,
  openKernelStatusDiagnosis,
  openStartupDiagnosis,
} from '../diagnostics/diagnostics.js';
import {
  InfoFilled,
  Timer,
  TopRight,
  ChatDotRound,
  CircleClose,
  VideoPlay,
  VideoPause,
  Refresh,
  RefreshRight,
  FolderOpened,
  Download,
  Box,
  Monitor,
  Setting,
  Warning,
  Connection,
  View,
  TrendCharts,
  Tickets,
  Coin,
} from '@element-plus/icons-vue';
// 版本号这里原本还挂着一枚 Lucide Tag 图标（EP 的 PriceTag 在 11px 下糊成一团
// 黑点，认不出是「标签」）。2026-10-08 起概览这一处改为纯文字：设计稿的
// `.kernel-version` 没有前置图形，图形段加左右内边距还会把首字推离左缘。
// 本文件已不再引 Lucide，图标全部来自 `@element-plus/icons-vue`。
import {
  store,
  showIncident,
  startWorkbench,
  stopWorkbench,
  openHarnessWindow,
  forceReloadHarnessWindow,
  toggleOfficialChat,
  openOfficialChatWindow,
  openDataDir,
  installShellUpdate,
  installLatestRelease,
  checkUpdates,
  installNode,
  detectNode,
} from '../store.js';
import { progress } from './progress.js';
import { globalBusy, isLoading, withLoading } from './loading.js';
import {
  loadUsageSummary,
  setUsageAutoRefresh,
  openUsageWindow,
  usage,
  formatTokens,
  RETENTION_DAYS,
} from '../usage/usage.js';
import {
  subscription,
  loadSubscriptionSummary,
  openSubscriptionWindow,
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
  queriedAtLabel,
  queriedAgeCompact,
  CODEX_USAGE_CONSENT_TIP,
  providerLogo,
} from '../subscription/subscription.js';
import { incidentBannerTitle, incidentDestination, incidentDestinationLabel } from '../incidents/incidents.js';
import { builtinStore, loadBuiltinStatus } from '../plugins/builtin.js';
import { tildePath } from './labels.js';
import { confirmDialog } from './notify.js';

// 进度窗口是全局的（任何长任务都会让它可见），按钮的加载态必须绑定自己的
// key，否则任何别的长任务都会让这个按钮转圈（P2-42）。
const onInstallNode = () => withLoading('installNode', () => installNode());

// 模型用量（今日）：进入概览页就拉一次（60s 内复用，后端另有 45s 扫描
// 新鲜度窗口）；原始值与 90 天保留策略进 tooltip。
onMounted(() => {
  loadUsageSummary();
  // 控制塔的「最近操作」读这条记录。它是纯本地读取（后端只读索引文件），
  // 不发网络请求，失败也只是这块显示「暂无」，因此不参与概览的慢网兜底。
  loadRecentRuns(null);
  // 挂载期间定期对账：此前这张卡片只在挂载时拉一次就再也不更新，而独立的
  // 「模型用量」窗口每次打开都重扫还带手动刷新——同一份统计于是长期对不上。
  setUsageAutoRefresh(true);
});
onUnmounted(() => setUsageAutoRefresh(false));
const usageDayText = computed(() =>
  usage.data ? formatTokens(usage.data.today_tokens) + ' tokens' : '—'
);
// 数据是异步到达的：tooltip 也要跟着 usage.data 走，用 computed 而不是常量。
const usageDayTip = computed(() =>
  usage.data
    ? `今日 ${usage.data.today_tokens} tokens；模型用量统计只保留最近 ${RETENTION_DAYS} 天，更早的记录自动丢弃`
    : `统计今日的模型用量，历史记录保留最近 ${RETENTION_DAYS} 天，更早的自动丢弃`
);

// --- 套餐用量（云端额度 / 余额，与模型用量并列但互相独立） -------------------
// 卡片内容直接展示（无折叠）；凭据未配置时只给「前往模型设置」入口。
const anyKeyConfigured = computed(() =>
  ((subscription.data && subscription.data.providers) || []).some((provider) => provider.configured)
);
// Codex 额度入口（开关 + 它的整宽说明句）是 OpenAI 插件的功能面，与 Rust 侧
// 分区门禁同源：插件停用时一并收起（2026-10-10）。开关组件自己按
// builtinStore 收起，这里的说明句在卡头下面另起一行，得跟着同一个判据。
const codexConsentVisible = computed(() => builtinStore.view?.requestedEnabled === true);
onMounted(() => {
  // 未配置时不发请求：Rust 侧对未配置 provider 也只做凭据解析，不产生网络调用，
  // 但首次进入概览总得拉一次才知道配置状态——由 loadSubscriptionSummary 自行决定。
  loadSubscriptionSummary();
  loadBuiltinStatus();
});
// 工作台里改完凭据回到概览：数据拉取动作自带 TTL，这里不额外触发。
// planRows 额外滤掉「用户选择隐藏」的分区（key 配了但一直查不到数据）。
// 余额分区在概览卡的形状：设计稿 `.usage-balance-row`（「余额」标签 + 右侧
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

const kernel = computed(() => store.view && store.view.kernel);
const node = computed(() => store.view && store.view.node);

// 「重新检测」（由设置页搬来）探测的是本机环境，而 detect_node 不会让 Rust 侧的状态
// 缓存失效，所以探测结果就地覆盖 Node 两处显示；离开概览页再回来即回到状态里的值。
const detectedNode = ref(null);
const shownNode = computed(() => detectedNode.value || node.value);

const running = computed(() => !!(kernel.value && kernel.value.running));

// 运行状态胶囊（品牌区同款视觉）：运行中（绿）/ 已停止（红）/ 未安装（中性）。
const kernelStatus = computed(() => {
  const k = kernel.value;
  if (!k) return { text: '加载中…', cls: '' };
  if (k.running) return { text: '运行中', cls: 'ok' };
  if (k.active && k.active_installed) return { text: '已停止', cls: 'bad' };
  return { text: '未安装', cls: '' };
});
const officialChatOpen = computed(() => !!(store.view && store.view.official_chat_open));
const canStart = computed(() => !!(kernel.value && kernel.value.active && kernel.value.active_installed));
const noKernel = computed(() => !!(kernel.value && (!kernel.value.installed || kernel.value.installed.length === 0)));

// 原先这里有 `kernelCaption`（卡头右侧的「DSH / default-dev」），2026-10-07
// 用户要求删除——它回答的是「这张卡在说哪个内核」，而卡里已经有一个 18px+ 的
// 活动内核版本号和它旁边的状态胶囊，族名与实例 id 在这里是把实现细节摆到了
// 第一屏。多实例是开发与过渡期的事实，绝大多数用户这台机器上只有一个实例。
//
// **这不是「顺手清死代码」**：它连着一条曾被门禁抓出来的教训，写在这里免得下次
// 又照着 `store.view.kernel` 写一遍——`KernelStatus` 结构体里既没有 `family`
// 也没有实例 id 字段（它的字段是 installed / active / active_installed …），
// 从 kernel 上读那个字段取到的是 undefined，而 `undefined || …` 会静默落到
// 下一个兜底分支。门禁 `check:invariants` 的 [ipc-fields] 项抓的就是这个。
//
// 要看实例上下文，插件页的「所有实例」逐条列着 `族名 · 实例 id`，那是它该在
// 的地方——按用户真正会去那儿找的地方摆，不是在概览卡头上常驻一行。

const nodeText = computed(() => {
  const n = shownNode.value;
  if (!n) return '—';
  return n.ok ? [tildePath(n.path), n.version].filter(Boolean).join('  ') : '未检测到可用 Node（' + n.reason + '）';
});

// 指标格里只放短值，完整内容进 title。
// 2026-10-07：设计稿的 metrics 是三等分格，每格只有约 1/3 卡宽。原来竖列时
// 「/usr/local/bin/node v25.9.0」这种全路径 + 版本号塞进去会把这一格撑到换行，
// 而换行后的指标行高低不齐，比省略更难看。路径仍在 title 里，一个字都没丢。
const nodeShortText = computed(() => {
  const n = shownNode.value;
  if (!n) return '—';
  return n.ok ? n.version || '已就绪' : '未检测到';
});
// 数据目录那一格显示的是**应用数据根目录** `~/.dsh-xlink`（2026-10-08 用户
// 定的），不是当前实例目录 `~/.dsh-xlink/<family>/desktop/`。
//
// 理由是那一格只有约 141px 宽，而实例目录经 `display_short` 的 38 字截断后
// 变成 `~/.dsh-xlink/dsh/…` —— 尾巴上挂一个省略号，看起来像一条坏掉的路径，
// 而不是像一条路径。「这个应用的数据放在哪」的答案就是根目录那一层，而实例
// 层是更细的事实，挂在 title 上，一个字都没丢。
//
// `tildePath` 在这里几乎总是原样返回：Rust 已经把 home 折成了 `~`。留着它
// 是为了 `DSH_XLINK_HOME` 被指到 home 之下时仍能折叠。
const dataDirText = computed(() => {
  const dir = kernel.value && kernel.value.xlink_home;
  return dir ? tildePath(dir) : '—';
});

const urlText = computed(() => (running.value ? 'http://127.0.0.1:' + kernel.value.port : '—'));

// Node 环境结论（自动检测口径：是否满足 dsh 的 ^22.19 || >=24）。悬浮在
// 「当前内核」标题旁的 ℹ️ 上展示；不达标时的具体原因与「自动安装」入口在
// 本卡的 Node.js 行，这里只给一句结论。
const nodeRequirementText = computed(() => {
  const n = shownNode.value;
  if (!n) return '—';
  return n.ok
    ? 'node ' + n.version + ' 满足 dsh 要求（^22.19 || >=24）'
    : '未检测到满足 dsh 要求（^22.19 || >=24）的 Node.js';
});

// 重新探测本机环境里的 Node（不读也不写 settings.node_path，探到什么都原样显示）。
function onDetectNode() {
  const info = detectNode();
  if (info) {
    info.then((result) => {
      if (result) detectedNode.value = result;
    });
  }
}

// 容错横幅：有被看护停用的插件，或上次启动事故未恢复时保持可见。
const quarantined = computed(() => (store.view && store.view.quarantined) || []);
const hasUnrecovered = computed(() => !!(store.lastIncident && !store.lastIncident.recovered));
const guardVisible = computed(() => quarantined.value.length > 0 || hasUnrecovered.value);
const guardText = computed(() => {
  if (quarantined.value.length > 0) {
    return (
      '为保证工作台可以启动，启动看护已停用以下插件：' +
      quarantined.value.map((q) => q.name).join('、') +
      '。请查看错误原因并决定移除或恢复。'
    );
  }
  return (store.lastIncident && store.lastIncident.message) || '上次工作台启动失败。';
});

// 归因口径（cause → 标题 / 判断 / 下一步去哪个面板）收在 `incidents.js`：
// 它此前与 IncidentModal 各有一份，两边各漏过一次 `env`。
const guardDestination = computed(() =>
  incidentDestination(store.lastIncident, quarantined.value.length)
);
const guardDestinationLabel = computed(() => incidentDestinationLabel(guardDestination.value));
// 非致命前端异常不弹模态框（见 store.js 的 showIncident）：横幅是它唯一的入口，
// 因此这里必须显式要求打开面板，否则「查看详情」会变成空操作。
const guardTitle = computed(() => incidentBannerTitle(store.lastIncident));
function openIncidentDetails() {
  showIncident(store.lastIncident, { force: true });
}

/** 从启动失败横幅进启动诊断（设计 §2.5.1 的三个入口之一）。 */
function openStartupRun() {
  openStartupDiagnosis(store.lastIncident?.runId || '', 'overview');
}
function goGuardDestination() {
  store.activePanel = guardDestination.value;
}

const toggleDisabled = computed(() => {
  if (store.starting) return true;
  if (running.value) return globalBusy.value;
  return !canStart.value || globalBusy.value;
});

function onToggle() {
  if (running.value) {
    stopWorkbench();
  } else {
    startWorkbench();
  }
}

// 「选择并安装内核」跳到版本页并顺手拉取发布列表，
// 用户到达时 npm 版本列表已就位。
function goVersions() {
  store.activePanel = 'versions';
  checkUpdates();
}
</script>

<template>
  <section class="panel">
    <!-- 页头：标题 + 一句话说明这一页回答什么问题。宽版下主区已经有足够
         横向空间，不必再靠卡片堆叠来暗示层次。 -->
    <div class="page-head">
      <div class="page-head__text">
        <h1 class="page-title">概览</h1>
        <p class="page-desc">当前内核、插件与技能接线状态，以及这个实例最近的使用与用量。</p>
      </div>
    </div>

    <!-- 概览主栅格：设计稿的 `.grid`（1.25fr / 0.75fr），行序由各卡的
         `order` 决定，详见下面 `<ControlTower />` 那段注释。 -->
    <div class="overview-grid">
      <!-- 首次运行引导：未安装任何内核时给出两条路径——去版本页挑选，
           或直接安装当前最新稳定版。横跨整行，因为它比下面任何一张卡都优先。 -->
      <Transition name="panel">
        <div
          v-if="noKernel && !store.starting && !running"
          class="callout callout-firstrun"
          role="alert"
        >
          <div class="callout-icon" aria-hidden="true">
            <el-icon><Warning /></el-icon>
          </div>
          <div class="callout-body">
            <h3>欢迎使用 DeepSeek Harness 桌面端</h3>
            <p>当前尚未安装任何 dsh 内核版本。请先选择一个版本安装，再启动工作台。</p>
            <div class="btn-row">
              <el-button type="primary" :icon="Box" :disabled="globalBusy" @click="goVersions">
                选择并安装内核
              </el-button>
              <el-button :icon="Download" :loading="isLoading('firstRunLatest')" :disabled="globalBusy" @click="installLatestRelease">
                安装最新版本
              </el-button>
            </div>
          </div>
        </div>
      </Transition>

      <!-- 左列：当前内核。宽版下它是这一页最需要横向空间的一张卡——
           地址、路径与用量条并排放得下，不必再一条一条竖着列。
           2026-10-07 按设计稿重排：主操作（工作台 / 官网网页版）从卡底那排
           提到卡头（它们是这一页最高频的两个动作，紧贴标题才不用视线下移），
           读数从「一串 dt/dd 竖列」改成「大版本号 + 状态行 + 三格指标」——
           竖列时每条读数独占一行，五行就把卡拉到和右栏一样高。 -->
        <div class="card kernel-card">
          <div class="card-head">
            <h2 class="kernel-title">
              <span class="card-title-icon" aria-hidden="true"><el-icon><Monitor /></el-icon></span>
              当前内核
              <el-tooltip placement="bottom-start" :show-after="80">
                <template #content>
                  <div class="card-info-tooltip">
                    <div>Node.js 环境</div>
                    <div>{{ nodeRequirementText }}</div>
                  </div>
                </template>
                <el-icon class="card-info-icon"><InfoFilled /></el-icon>
              </el-tooltip>
            </h2>
            <!-- 主操作在卡头右侧，与标题同一行（2026-10-07 用户要求：原先两个
                 按钮窝在标题下面那一行左侧，视线要往下再折一次才找到）。原先
                 占着这个位置的实例 caption「DSH / default-dev」已按同一要求
                 删除——多实例是实现细节，绝大多数用户只有一个实例。 -->
            <div class="kernel-header-actions">
              <el-button
                class="btn-action"
                :class="{ 'btn-danger': running }"
                size="small"
                :icon="running ? VideoPause : VideoPlay"
                :loading="store.starting"
                :disabled="toggleDisabled"
                :title="running ? '停止工作台' : '启动工作台'"
                @click="onToggle"
              >
                工作台
              </el-button>
              <el-button
                class="btn-action"
                :class="{ 'btn-chat': !officialChatOpen, 'btn-danger': officialChatOpen }"
                size="small"
                :icon="officialChatOpen ? CircleClose : ChatDotRound"
                :disabled="store.starting || globalBusy"
                :loading="isLoading('officialChat')"
                :title="officialChatOpen ? '关闭 DeepSeek 官网网页版' : '打开 DeepSeek 官网网页版'"
                @click="toggleOfficialChat"
              >
                官网网页版
              </el-button>
            </div>
          </div>

          <div class="kernel-summary">
            <!-- 版本号、状态行与次级入口同属一个竖向块（设计稿 `.kernel-summary`
                 里那层 div）。2026-10-08 用户要求「操作按钮迁移到版本号右侧，
                 靠右显示」，此前那一排三枚次级入口（工作台窗口 / 刷新工作台 /
                 官网网页版窗口）在三格指标**下方**独占一行，读数与动作被一段
                 竖线分开，视线要往下再折一次。

                 为什么这一块内部还是竖排、且状态行要单独占第二行：横排时状态
                 胶囊会随内核状态换行到不同位置，读法不稳定（这条 2026-10-07
                 就定过，本轮只是把第三个元素接进来，理由没变）。而实测宽度
                 （用户 1040px 宽窗截图）显示三样东西**不能挤在一行**：版本号
                 `0.2.1-alpha.1` 占 98px，三枚按钮含间隙占 354px，状态行
                 （状态胶囊 + 工作台地址 + 查看状态）占 287px，三者合计 739px，
                 远超卡内可用宽 482px——真挤在一行，状态行只剩约 110px，会折成
                 三四行。所以落位是**版本号 + 按钮同一行（右对齐）、状态行
                 独占第二行**，不是三者平铺。 -->
            <div class="kernel-summary-main">
              <!-- 纯文字，不再是「tag 图标 + 药丸」的徽标（2026-10-08 用户
                   截图要求：移除 tag icon、放大、靠最左）。设计稿的
                   `.kernel-version` 就是一段纯文字——18px / 700 / 主文字色，
                   没有边框、没有底、没有前置图形。此前的 VersionBadge 把
                   它关进一个 22px 的胶囊里：图形段与左右各 5/8px 内边距把
                   首字推离左缘，13px 的灰字（`--text-secondary`）也让这条
                   「我现在跑的是哪个内核」读起来像一行脚注。这里放大到
                   20px 并用主文字色，它才是这一卡真正的主读数。 -->
              <div
                class="kernel-version"
                :title="'活动内核版本：' + ((kernel && kernel.active) || '未选择')"
              >
                {{ (kernel && kernel.active) || '未选择' }}
              </div>
              <!-- 次级入口：仅在对应服务开启后出现，作为窗口层的入口，视觉上压低
                   权重（ghost 风格），与卡头的主按钮做明显区分。
                   「工作台窗口 / 官网网页版窗口」只把窗口带到台前，「刷新工作台」
                   是唯一的**动作**——它换掉整个工作台窗口（见 harness_cmd）。
                   三者都不改变内核状态，所以按 AGENTS.md 的 IA 规则同属次级入口
                   这一排，不许往主按钮旁边堆会启停内核的动作。

                   2026-10-08 从三格指标下方搬到版本号这一行的右侧（靠右对齐）。
                   「查看日志」不在这一排（早于本轮就已移除）：它与下面「系统健康 →
                   日志系统」是同一件事的两个入口，而两者做的事还不一样——那个按钮
                   直接开独立全屏窗口（跳过列表），那一行走日志弹层。同一屏上两个
                   日志入口、点开结果还不一致，用户没法建立预期。
                   **独立日志窗口的能力因此没有丢失，反而回到了它该在的地方**：现在
                   「系统健康」那一格自己就开独立窗口，弹层里的「全屏」按钮是第二条
                   通路（`logs.js::openLogWindow`，两个入口共用这一份实现）。
                   设计说明 §4 要求的「保留刷新、折叠侧栏和独立日志窗口入口」三项都在
                   弹层里。 -->
              <Transition name="subrow">
                <div v-if="running || officialChatOpen" class="btn-row btn-row-sub">
                  <el-button
                    v-if="running"
                    class="btn-sub"
                    size="small"
                    :icon="TopRight"
                    :loading="isLoading('openHarness')"
                    :disabled="globalBusy"
                    title="在独立窗口中打开工作台 webview"
                    @click="openHarnessWindow"
                  >
                    工作台窗口
                  </el-button>
                  <el-button
                    v-if="running"
                    class="btn-sub"
                    size="small"
                    :icon="RefreshRight"
                    :loading="isLoading('forceReloadHarness')"
                    :disabled="globalBusy"
                    title="黑屏 / 卡死时的手动出路：重建工作台窗口，渲染进程会换掉。窗口内的滚动位置、侧栏与终端回到初始状态，会话不受影响"
                    @click="forceReloadHarnessWindow"
                  >
                    刷新工作台
                  </el-button>
                  <el-button
                    v-if="officialChatOpen"
                    class="btn-sub"
                    size="small"
                    :icon="TopRight"
                    :loading="isLoading('openOfficialChatWindow')"
                    :disabled="globalBusy"
                    title="唤起 / 聚焦 DeepSeek 官网网页版窗口"
                    @click="openOfficialChatWindow"
                  >
                    官网网页版窗口
                  </el-button>
                </div>
              </Transition>
              <div class="kernel-status-row">
                <span class="status-pill">
                  <span class="dot" :class="kernelStatus.cls"></span>
                  <span>{{ kernelStatus.text }}</span>
                </span>
                <span class="kernel-detail">工作台地址 {{ urlText }}</span>
                <!-- 「查看状态」进内核状态诊断：版本 / 运行 / 端口 / 数据目录的
                     一张读数表。它不探测，只解释用户眼前这份快照。 -->
                <el-button
                  class="kernel-status-link"
                  text
                  size="small"
                  @click="openKernelStatusDiagnosis(store.activePanel)"
                >
                  查看状态
                </el-button>
              </div>
            </div>
          </div>

          <div class="metrics">
            <div class="metric">
              <div class="metric-label metric-label-with-action">
                <span>Node.js</span>
                <span class="metric-actions">
                  <el-button
                    v-if="shownNode && !shownNode.ok"
                    size="small"
                    text
                    :loading="isLoading('installNode')"
                    :disabled="globalBusy"
                    title="自动下载并安装官方 Node.js 到数据目录（需联网）"
                    @click="onInstallNode"
                  >
                    自动安装
                  </el-button>
                  <el-button
                    size="small"
                    text
                    :icon="Monitor"
                    :loading="isLoading('detectNode')"
                    :disabled="globalBusy"
                    title="重新探测本机环境里的 Node.js（不改设置；刚装完 Node 时用它刷新）"
                    @click="onDetectNode"
                  >
                    重新检测
                  </el-button>
                </span>
              </div>
              <div class="metric-value" :title="nodeText">{{ nodeShortText }}</div>
            </div>
            <div class="metric">
              <div class="metric-label metric-label-with-action">
                <span>今日用量</span>
                <el-button
                  size="small"
                  text
                  :icon="TrendCharts"
                  :loading="isLoading('openUsageWindow')"
                  title="在独立窗口中查看模型用量（热力图 / 趋势 / 按模型统计，最近 90 天）"
                  @click="openUsageWindow"
                >
                  模型用量
                </el-button>
              </div>
              <div class="metric-value" :title="usageDayTip">{{ usageDayText }}</div>
            </div>
            <div class="metric">
              <div class="metric-label metric-label-with-action">
                <span>数据目录</span>
                <el-button
                  size="small"
                  text
                  :icon="FolderOpened"
                  :loading="isLoading('openDataDir')"
                  title="在系统文件管理器中打开应用数据根目录 ~/.dsh-xlink"
                  @click="openDataDir"
                >
                  打开
                </el-button>
              </div>
              <!-- title 挂实例目录：格子里显示根，悬浮给的是「当前这一份
                   实例实际在哪儿」。两者都要有——只给一个，用户就只能二选一。 -->
              <div
                class="metric-value metric-value--path"
                :title="'应用数据根目录：' + dataDirText + '\n当前实例目录：' + (kernel && kernel.data_dir)"
              >
                {{ dataDirText }}
              </div>
            </div>
          </div>

      <!-- 更新提示与它的动作**同行**（2026-10-08 用户要求「按钮和更新信息一行显示」）。
           此前 `el-alert` 与装按钮的 `.btn-row` 是两个兄弟节点：alert 占满一整行，
           按钮掉到下一行，卡片因此凭空高一块，而两块之间没有任何语义关联。 -->
      <div v-if="store.shellUpdateVersion" class="update-banner">
        <el-alert :title="store.shellUpdateText" type="warning" :closable="false" show-icon />
        <el-button
          class="btn-action"
          type="warning"
          :icon="Refresh"
          :loading="isLoading('installShellUpdate')"
          :disabled="globalBusy"
          @click="installShellUpdate"
        >
          更新并重启
        </el-button>
      </div>

      <div v-if="guardVisible" class="callout" role="alert">
        <div class="callout-icon" aria-hidden="true">
          <el-icon><Warning /></el-icon>
        </div>
        <div class="callout-body">
          <h3>{{ guardTitle }}</h3>
          <p>{{ guardText }}</p>
          <div class="btn-row">
            <el-button size="small" type="warning" plain :icon="View" @click="openIncidentDetails">
              查看详情
            </el-button>
            <!-- 事故面板给的是「处置」——被停用了哪些插件、下一步做什么；
                 启动诊断给的是「过程」——停在哪一步、每个阶段耗时多少。
                 两者职责不同，不能用一个顶掉另一个（设计 §2.5.1 入口表）。 -->
            <el-button size="small" text :icon="Tickets" @click="openStartupRun">
              查看启动诊断
            </el-button>
            <el-button size="small" text :icon="Connection" @click="goGuardDestination">
              {{ guardDestinationLabel }}
            </el-button>
          </div>
        </div>
      </div>

      <!-- 主操作两件套（工作台 / 官网网页版）已于 2026-10-07 提到卡头，与标题
           同排——它们是这一页最高频的两个动作，贴在标题边上不用视线下移。
           这里只留「更新并重启」：它条件出现（查到新版本才有），塞进卡头会
           平时占位、偶尔把标题挤掉。按钮只写名词不写「打开/关闭」：动作方向
           由 icon 表达——工作台 ▶ 启动（VideoPlay）/ ⏸ 停止（VideoPause），
           官网网页版 💬 打开（ChatDotRound）/ ⏹ 关闭（CircleClose），文字色
           随状态切换（关闭态淡红 btn-danger）。

           「查看日志」原先也在这一排（2026-10-07 迁走）：它与「系统健康 → 日志系统」
           是同一件事的两个入口，而两者做的事还不一样——按钮直接开独立全屏窗口
           （跳过列表），那一行走日志弹层。同一屏上两个日志入口、点开结果还
           不一致，用户没法建立预期。原先那条捷径已随之删除。现在全应用的日志
           入口统一走弹层（事故 / 预检 / 诊断页本来也都是同一个），要更大屏就在
           弹层里点「全屏」。 -->
      <p v-if="!store.starting && !running && !canStart" class="muted" style="margin: 0">
        尚未安装可用内核，请先到「内核版本」页安装。
      </p>
    </div>

      <!-- 2026-10-07 按设计稿改骨架。原先是「左右两列各自竖着堆」：左列只有
           当前内核，右列堆控制塔三张卡 + 套餐用量，右列因此比左列长出一大截，
           而左列下方空着半屏。设计稿的排布是按**行**走的：

             第 1 行  当前内核（1.25fr） | 系统健康（0.75fr）
             第 2 行  套餐用量（整宽）
             第 3 行  需要关注 | 最近操作（左右并排）

           「需要关注」移到末行是 2026-10-07 用户改的：异常清单是**要处理**的
           东西，不该占住首行把「当前内核」挤到第二眼。

           实现方式：`<ControlTower />` 是 Vue 3 fragment（需要关注 / 系统健康 /
           最近操作三张卡是它的三个并列根节点），放进这个 grid 后三张卡直接成为
           栅格子项，**不需要拆成三个组件、也不会触发三次数据拉取**。
           但它们的 DOM 顺序是「三张塔卡在前、内核卡在后」，与视觉顺序不一致，
           所以每张卡显式给 `order` + `grid-column`：栅格按 order 排序后自动排布，
           某张卡缺席时其余各归其位（`order` 方案对「需要关注」不存在的情况
           免疫，写死 grid-row 就不会）。 -->
      <ControlTower
        @open-incident="openIncidentDetails"
        @go-panel="(name) => (store.activePanel = name)"
      />

        <!-- 套餐用量：独立只读卡，内容直接展示（无折叠）。MiniMax 双窗口进度 +
         DeepSeek 余额行；完整可操作错误文案只在顶部横幅出现，provider 分区
         仅用短状态词标注。凭据复用工作台模型设置，这张卡不带任何写操作。 -->
        <div class="card usage-card">
      <div class="card-head">
        <h2>
          <span class="card-title-icon" aria-hidden="true"><el-icon><Coin /></el-icon></span>
          套餐用量
          <el-tooltip placement="bottom-start" :show-after="80">
            <template #content>
              <div class="card-info-tooltip">
                MiniMax（国内站 / 国际站）与智谱的 5 小时 / 周窗口余额、DeepSeek 按量余额（多币种）。
                数据缓存 5 分钟，点「刷新」立即重新查询。凭据复用工作台模型设置；
                未在内核配置对应厂商时，相应分区自动隐藏。
                OpenAI / Codex 额度分区与「使用本机 Codex 登录查询额度」开关，仅在启用 OpenAI-OAuth-Plugin 后显示。
              </div>
            </template>
            <el-icon class="card-info-icon"><InfoFilled /></el-icon>
          </el-tooltip>
        </h2>
        <!-- Codex 额度开关进卡头（设计稿 `.card-head` 的位置）：标题在左，开关与
             「刷新 / 查看详情」同排。原先它独占卡头下面一整行，把「默认关闭…」那句
             许可说明顶到卡片中间、说明与开关之间隔着一段空白——而开关只有一个
             `active-text`，脱离说明之后它读起来像「一个和额度有关的开关」，
             不知道开启意味着只读登录文件。许可句改由下面的整宽一行给出。 -->
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
          <el-button
            class="btn-action"
            round
            size="small"
            :icon="TopRight"
            :loading="isLoading('openSubscriptionWindow')"
            title="在独立窗口中查看套餐用量"
            @click="openSubscriptionWindow"
          >
            查看详情
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
                    余额不足，无法发起调用
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
              <p v-if="tier.countdown" class="plan-tier-reset" :title="tier.countdownTitle">
                <el-icon class="plan-reset-icon"><Timer /></el-icon>{{ tier.countdown }}
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
    </div>
  </section>
</template>

<style scoped>
/* 概览两张卡收紧行内留白：卡片内边距与卡片间距减半（真机反馈纵向过散、
   整窗出滚动条）。只作用于本面板，其他页的卡片不受影响。 */
.card {
  padding: 6px 8px;
  gap: 4px;
}
/* 概览这一个面板的 `.panel` 要占满 `main` 的内容高度，末行才有「剩余空间」
   可吃。`main` 是 `flex: 1` 的 flex item、高度确定，所以这里的 `100%` 解得开；
   换成 `height: auto` 的普通块就解不开——百分比高度落在高度为 auto 的祖先上会
   当成 auto，整条规则静默失效。 */
.panel {
  gap: 4px;
  min-height: 100%;
}
/* 信息行文本行高居中：胶囊 / 按钮与文本垂直对齐（grid 行默认顶对齐）。 */
.kv dt,
.kv dd {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
}
/* 信息行的纵向行距大头不在 gap，在行盒本身。带操作按钮的三行（Node.js /
   数据目录 / 今日用量）被 Element 的 small 按钮 24px 外框顶高，而按钮文字
   只有 12px——上下各 6px 是纯空气。裸文本行按 13px × 1.5 只有 19.5px，
   于是同一张卡里五行行高参差约 4px，gap 再怎么调也只是在错高上补空气。
   收到 20px 后五行统一等于文本行盒，gap 才真的成了唯一的行距来源。
   横向 padding 保持 Element 默认的 11px 不动：这些是无底色 text 按钮，
   padding 看不见、只负责可点面积，砍它等于悄悄缩小热区。 */
.kv {
  row-gap: 4px;
}
.kv .el-button {
  height: 20px;
}
/* 信息行里的状态胶囊比品牌区小一号；并抵消窄窗媒体查询的
   `.status-pill { margin-left: auto }`——那条规则是为品牌行右推准备的，
   在信息行里会把胶囊推到整行最右（与下方文本列错位）。 */
.kv .status-pill {
  margin-left: 0;
  padding: 1px 8px;
  gap: 4px;
  font-size: 11px;
}
.kv .status-pill .dot {
  width: 6px;
  height: 6px;
}
/* 「当前内核」标题行：标题 + ℹ️ + 主操作按钮在左，实例 caption 在最右。 */
.kernel-title {
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 0;
}
/* 卡头里的主操作：与标题同排，撑开可点区域之外不额外占位。盒高 / 内边距 /
   字号 / 图标槽**不在这里**——它们按角色而不是按页面划分，已收进 theme.css 的
   `.card-head .el-button`，六个面板共用一份（理由见那里的注释：写在概览里就只有
   概览是这一档，而改之前同一屏里已经有两种大小的卡头按钮）。 */
.kernel-header-actions {
  display: flex;
  align-items: center;
  gap: 6px;
}

/* 更新提示与它的动作同行（2026-10-08 用户要求「按钮和更新信息一行显示」）。 */
.update-banner {
  display: flex;
  align-items: center;
  gap: 10px;
}
/* alert 吃掉余下宽度、按钮不换行：提示文字变长时先压缩提示而不是把按钮挤下去。 */
.update-banner :deep(.el-alert) {
  flex: 1;
  min-width: 0;
}

/* 概览主栅格。比例取设计稿 `.grid` 的 1.25fr / 0.75fr：左边的当前内核要放
   大版本号 + 状态行 + 三格指标，右边的系统健康只有「名称 / 读数」两段，
   1:1 会把左边挤到换行。右列的 280px 下限保证「技能注册 + 读取失败 +
   点击重试」放得下（2026-10-07 由两列改单列就是为了这个，见 diagnostics.css）。 */
.overview-grid {
  display: grid;
  grid-template-columns: minmax(0, 1.25fr) minmax(280px, 0.75fr);
  /* 前两行按内容高，**末行吃掉剩余纵向空间**（2026-10-08 用户要求「两个功能块
     都增加高度，用满纵向高度」）。
     原来是 `align-content: start` + 不写行高：末行两张卡有多高就多高，于是
     内容少的时候下半屏空一大片。
     用 `minmax(min-content, 1fr)` 而不是 `minmax(0, 1fr)`：后者会在空间不够时
     把末行压扁，卡片内容溢出自己的格子（看着像布局坏了）；`min-content` 让
     末行至少装得下自己的内容，装不下就交给 `main` 的 `overflow-y: auto` 去滚，
     这是本仓的既定纪律——内容纵向滚动，横向溢出才是 bug。 */
  grid-template-rows: auto auto minmax(min-content, 1fr);
  /* 12 → 8（2026-10-10）：第四家 provider 进场后套餐用量卡折成两行，整页刚好
     超出默认高度一截、纵向滚动条回来了。行间缝从设计稿的 12 收到 8（列间同
     收，gap 一值），配合 main / plan-provider 的内边距收紧把「最近操作」拉回
     首屏。designAlignment 对这条值的断言已同步。 */
  gap: 8px;
  align-content: start;
  /* `min-height: 100%` 的百分比要解到 `.panel` 上，所以那一层也必须是确定高度
     （见下面 `.panel` 那条）。栅格是 `.panel` 的 flex 子项，`flex: 1` 让它去
     领剩余空间，而不是靠 `min-height` 把 `.panel` 顶高。 */
  flex: 1;
  min-height: 0;
}
/* 行序。栅格按 `order` 而非 DOM 顺序排布——`<ControlTower />` 是 fragment，
   它的三张卡在 DOM 里连在一起，与视觉顺序不同。写成 `order` 而不是
   `grid-row` 是因为「需要关注」缺席时行号会整体前移，order 对此免疫。
   末行是「需要关注 | 最近操作」左右并排（2026-10-07 用户定的落位，两张卡的
   order / grid-column 在 diagnostics.css 里）。 */
.kernel-card { order: 1; grid-column: 1; }
.usage-card { order: 3; grid-column: 1 / -1; }
/* 首次运行引导比下面任何一张卡都优先，占第一行整宽。 */
.callout-firstrun { order: 0; grid-column: 1 / -1; }

/* 大版本号 + 状态行 + 次级入口落位。版本号是这一页字号最大的一处读数：它回答「我现在跑的是
   哪个内核」，而这条信息此前只是标题行右侧一个小徽标。
   `padding: 13px 0 12px` 取设计稿 `.kernel-summary` / `.kernel-version`。
   版本号与状态行在稿子里是**同一个竖向块**（版本号一行、状态行下一行），
   此前这里把它们横排并允许换行，于是状态胶囊有时贴到版本号右侧、有时掉到下一行，
   每种内核状态下这一行的读法都不一样。

   2026-10-08：第三样东西（次级入口那一排三枚按钮）也接进这个块。落位是
   **两行网格**而不是 flex 兄弟项 —— 理由是实测宽度，注释见模板里那段：
   版本号 84px + 按钮 347px + 状态行 287px = 718px > 卡内可用宽 482px，
   三者平铺必然把状态行挤到折行。所以：版本号与按钮同占第一行（按钮靠右），
   状态行 `grid-column: 1 / -1` 独占第二行、拿回整幅宽度。

   用网格而不是 `.kernel-summary` 的 flex + `margin-left: auto`：flex 里
   状态行与按钮仍是同一层的兄弟项，`flex-wrap` 会在宽度不够时把状态行挤到
   折行，而 `margin-left: auto` 那条靠右只能作用在「整块不换行」的前提上。 */
.kernel-summary {
  padding: 13px 0 12px;
}
.kernel-summary-main {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  align-items: center;
  column-gap: 16px;
  min-width: 0;
}
/* 版本号是一段**纯文字**主读数，不再是徽标。设计稿 `.kernel-version` 给的是
   18px / 700 / `letter-spacing: -0.02em` / 主文字色，没有边框也没有底；本处放大到
   20px（用户 2026-10-08 明确要求「文字放大」，稿子的 18px 在这一格里并不突出）。
   `margin-left: 0` 是「靠最左边」的落点——它此前挂在 VersionBadge 的图形段右侧，
   靠胶囊自己的 5px 内边距离左缘，去掉徽标后必须显式归零，否则会继承外层缩进。
   版本号不允许折行：它是一段无空格的版本串，wrap 会在点号处断开成
   「0.2.1-」/「alpha.1」两行，比占宽更难读。 */
.kernel-version {
  grid-column: 1;
  grid-row: 1;
  min-width: 0;
  white-space: nowrap;
  margin-left: 0;
  color: var(--text);
  font-size: 20px;
  font-weight: 700;
  letter-spacing: -0.02em;
}
/* 次级入口：版本号右侧、靠右对齐。`justify-self: end` 是「靠右」的落点——
   网格项默认 `stretch`，不钉住的话这一格会被拉满整列，按钮贴着版本号而不是右边。
   三枚按钮合计约 347px（收小前 379px），版本号「0.4.1-rc.1」约 84px，间隙 16px，
   卡内可用宽 482px（实测）→ 余量约 36px。收小前只剩约 3px：三枚按钮几乎贴着版本号，
   版本串再长一点（`0.4.10-beta.2` 一类）就直接压上或折行，用户报的「都显示后会有点挤」
   就是这 3px。**版本号那格是 `minmax(0,1fr)`，它会把余量吃掉**——所以量余量必须量版本号
   的**文字**宽度（Range），拿格子宽度算出来的 need 恒等于可用宽，余量永远显示 0。 */
.kernel-summary-main .btn-row-sub {
  grid-column: 2;
  grid-row: 1;
  justify-self: end;
}
.kernel-status-row {
  grid-column: 1 / -1;
  grid-row: 2;
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 6px 10px;
  margin-top: 8px;
  min-width: 0;
}
.kernel-detail {
  color: var(--text-secondary);
  font-size: 12px;
}
.kernel-status-row .kernel-status-link {
  margin-left: 0;
  padding: 0;
  height: auto;
}

/* 这一屏的分割线走 theme.css 的全局刻蚀线标准（.card-head 的底线已在全局
   换成「2px / 两端收细 / --divider-strong」，这张卡不再需要 scoped 覆写）。
   这里只剩概览私有的两处：三格指标的上边线，以及格与格之间的竖线。
   竖线横过来，六边形跟着转 90°（左右收到 0、上下张开），且**收细比横线短**
   （22% vs 20%）：这一格只有几十像素高，按横线那 20% 去收，两端各削掉十几
   像素，出来是一片叶子而不是一道线——那正是中途返工过一次的地方。
   完整的 rationale（为什么不能用渐变、为什么否掉「一深一浅两条」的刻蚀槽）
   见 theme.css 刻蚀线那段与 ui/AGENTS.md 的设计标准，不在这里重复。 */
.metrics {
  position: relative;
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 8px;
  padding-top: 14px;
  border-top: 0;
}
.metrics::before {
  content: '';
  position: absolute;
  left: 0;
  right: 0;
  top: 0;
  height: 2px;
  background: var(--divider-strong);
  pointer-events: none;
  clip-path: polygon(0 50%, 20% 0, 80% 0, 100% 50%, 80% 100%, 20% 100%);
}
.metric { position: relative; min-width: 0; }
.metric + .metric {
  padding-left: 15px;
}
/* 竖线横过来：六边形跟着转 90°（左右收到 0、上下张开）。
   **收细比横线短**：这一格只有几十像素高，按横线那 20% 去收，两端各削掉十几
   像素，出来是一片叶子而不是一道线——那正是中途返工过一次的地方。 */
.metric + .metric::before {
  content: '';
  position: absolute;
  top: 0;
  bottom: 0;
  left: 7px;
  width: 2px;
  background: var(--divider-strong);
  pointer-events: none;
  clip-path: polygon(50% 0, 100% 22%, 100% 78%, 50% 100%, 0 78%, 0 22%);
}
.metric-label {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 4px 8px;
  /* 放不下时让**动作**换行，而不是把「今日用量」拆成两行——指标名被拆开后
     三格的标题高度就对不齐了，而那正是这一排要避免的 ragged 感。 */
  flex-wrap: wrap;
  min-height: 22px;
  color: var(--text-muted);
  font-size: 11px;
}
.metric-label > span:first-child {
  white-space: nowrap;
}
/* Node.js 格可能要同时挂「自动安装」和「重新检测」两个动作（前者只在
   没探测到时出现）。挤不下就让它们换行，不要压字号或藏掉一个入口。 */
.metric-actions {
  display: flex;
  align-items: center;
  gap: 4px;
  flex-wrap: wrap;
  justify-content: flex-end;
}
.metric-value {
  overflow: hidden;
  margin-top: 3px;
  color: var(--text);
  font-size: 12px;
  font-weight: 600;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.metric-value--path {
  font-family: ui-monospace, monospace;
  font-weight: 400;
}
/* 指标格里的动作按钮：设计稿的 `.button.text-action` 是无底色、近零内边距、
   `color: var(--accent)` / 11px / 600 的一行小字。
   **颜色此前不一致**：「模型用量」带 `type="primary"` 走 accent 蓝，而
   「重新检测」与「打开」没带，于是三枚按钮里一枚蓝两枚黑——同一排三个同级
   入口、两套颜色。这里统一由 CSS 给，不再依赖模板上谁记得加 `type`。 */
.metrics :deep(.el-button) {
  height: auto;
  padding-left: 4px;
  padding-right: 4px;
  color: var(--accent);
  font-size: 11px;
  font-weight: 600;
}
.metrics :deep(.el-button:hover),
.metrics :deep(.el-button:focus-visible) {
  color: var(--accent-strong);
}
/* 图标槽：设计稿 `.button-icon` 是 16×16 的方格（`.button-icon.ui-icon` 里
   `flex: 0 0 16px`），而 EP 的 `.el-icon` 默认 `font-size: inherit`——按钮
   11px 把图标一起缩到 11px，三个入口的图形小到认不出是检测 / 图表 / 文件夹。 */
.metrics :deep(.el-button .el-icon) {
  font-size: 16px;
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
/* 套餐用量卡头右侧的按钮组（刷新 / 查看详情）。 */
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
