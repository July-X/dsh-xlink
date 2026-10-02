<script setup>
// 概览：当前内核状态、工作台启停单按钮状态机、首次运行引导、
// 外壳更新横幅与安装入口（手动检查在侧栏品牌区）以及启动容错横幅。
// 内核生命周期是实现细节，只暴露「打开/关闭工作台 / 打开/关闭官方对话 / 查看日志」；
// 「打开工作台窗口 / 打开官方对话窗口」在对应服务开启后作为次级入口从第二行动态浮现。
// 「当前内核」的 Node.js 行另带「重新检测」（探测本机环境，不改设置）；Node
// 环境结论悬浮在卡标题旁的 ℹ️ 上（原「桌面端设置」卡已并入这个 tooltip）。
// 活动版本徽标里的 tag 图标是刻意的：内核版本按 git tag 发版（tag 格式
// desktop-v<version>），标成「标签」比裸版本号贴切，也与「内核版本」页对得上。
import { computed, onMounted, ref, watch, onUnmounted } from 'vue';
import {
  InfoFilled,
  Timer,
  TopRight,
  Document,
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
} from '@element-plus/icons-vue';
// 版本 tag 图标改用 Lucide：EP 的 PriceTag 是实心票券造型，在 11px 下糊成一团
// 黑点，认不出是「标签」；Lucide 的 Tag 是 2px 描边的小挂牌，缩到这个尺寸仍读得
// 出轮廓。Lucide 授权 ISC，`@lucide/vue` 按图标名 tree-shake，只打进用到的那几枚。
// 别名成 TagIcon：同文件里还有一排 `el-tag`，光看 `<Tag />` 分不清说的是哪一个。
import { Tag as TagIcon } from '@lucide/vue';
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
import { openLogsWindow } from '../logs/logs.js';
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
  tierRow,
  balanceRow,
  queriedAtLabel,
  queriedAgeCompact,
} from '../subscription/subscription.js';
import { incidentBannerTitle, incidentDestination, incidentDestinationLabel } from '../incidents/incidents.js';
import { tildePath } from './labels.js';
import { confirmDialog } from './notify.js';

// 进度窗口是全局的（任何长任务都会让它可见），按钮的加载态必须绑定自己的
// key，否则任何别的长任务都会让这个按钮转圈（P2-42）。
const onInstallNode = () => withLoading('installNode', () => installNode());

// 模型用量（今日）：进入概览页就拉一次（60s 内复用，后端另有 45s 扫描
// 新鲜度窗口）；原始值与 90 天保留策略进 tooltip。
onMounted(() => {
  loadUsageSummary();
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
onMounted(() => {
  // 未配置时不发请求：Rust 侧对未配置 provider 也只做凭据解析，不产生网络调用，
  // 但首次进入概览总得拉一次才知道配置状态——由 loadSubscriptionSummary 自行决定。
  loadSubscriptionSummary();
});
// 工作台里改完凭据回到概览：数据拉取动作自带 TTL，这里不额外触发。
// planRows 额外滤掉「用户选择隐藏」的分区（key 配了但一直查不到数据）。
const planRows = computed(() =>
  ((subscription.data && subscription.data.providers) || [])
    .filter((provider) => provider.configured && !isProviderHidden(provider.id))
    .map((provider) => ({
      provider,
      tiers: provider.kind === 'plan' ? provider.tiers.map((tier) => tierRow(tier)).filter(Boolean) : [],
      balances: provider.kind === 'balance' ? provider.balances.map((balance) => balanceRow(balance)) : [],
      shortState: providerShortState(provider),
      queried: queriedAtLabel(provider),
      queriedCompact: queriedAgeCompact(provider),
    }))
);
// 余额类（DeepSeek）单独成块；套餐类（MiniMax / GLM…）进左右自适应栅格，
// 新增的 provider 依次往后排，flex-wrap 自动换行。
const balanceRows = computed(() => planRows.value.filter((row) => row.provider.kind === 'balance'));
const planBlockRows = computed(() => planRows.value.filter((row) => row.provider.kind === 'plan'));
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

const nodeText = computed(() => {
  const n = shownNode.value;
  if (!n) return '—';
  return n.ok ? [tildePath(n.path), n.version].filter(Boolean).join('  ') : '未检测到可用 Node（' + n.reason + '）';
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
    <!-- 首次运行引导：未安装任何内核时给出两条路径——去版本页挑选，
         或直接安装当前最新稳定版。 -->
    <Transition name="panel">
      <div v-if="noKernel && !store.starting && !running" class="callout callout-firstrun" role="alert">
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

    <div class="card">
      <h2 class="kernel-title">
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
        <span class="kernel-version" :title="'活动内核版本：' + ((kernel && kernel.active) || '未选择')">
          <TagIcon :size="11" />
          {{ (kernel && kernel.active) || '未选择' }}
        </span>
      </h2>
      <dl class="kv">
        <dt>运行状态</dt>
        <dd>
          <!-- 品牌区同款状态胶囊（圆点 + 文本），语义一致：运行中 / 已停止 / 未安装。 -->
          <span class="status-pill">
            <span class="dot" :class="kernelStatus.cls"></span>
            <span>{{ kernelStatus.text }}</span>
          </span>
        </dd>
        <dt>工作台地址</dt>
        <dd>{{ urlText }}</dd>
        <dt>Node.js</dt>
        <dd class="kv-with-action">
          <span>{{ nodeText }}</span>
          <el-button
            v-if="shownNode && !shownNode.ok"
            size="small"
            text
            type="primary"
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
        </dd>
        <dt>数据目录</dt>
        <dd class="kv-with-action">
          <span class="kv-path" :title="kernel && kernel.data_dir">{{ (kernel && kernel.data_dir) || '—' }}</span>
          <el-button
            size="small"
            text
            :icon="FolderOpened"
            :loading="isLoading('openDataDir')"
            title="在系统文件管理器中打开数据目录"
            @click="openDataDir"
          >
            打开
          </el-button>
        </dd>
        <dt>今日用量</dt>
        <dd class="kv-with-action">
          <span :title="usageDayTip">{{ usageDayText }}</span>
          <el-button
            size="small"
            text
            type="primary"
            :icon="TrendCharts"
            :loading="isLoading('openUsageWindow')"
            title="在独立窗口中查看模型用量（热力图 / 趋势 / 按模型统计，最近 90 天）"
            @click="openUsageWindow"
          >
            模型用量
          </el-button>
        </dd>
      </dl>

      <el-alert
        v-if="store.shellUpdateVersion"
        :title="store.shellUpdateText"
        type="warning"
        :closable="false"
        show-icon
      />

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
            <el-button size="small" text :icon="Connection" @click="goGuardDestination">
              {{ guardDestinationLabel }}
            </el-button>
          </div>
        </div>
      </div>

      <!-- 第一行：主操作三件套（工作台 / 官方对话 / 查看日志）+ 可选外壳更新。
           按钮只写名词不写「打开/关闭」：动作方向由 icon 表达——
           - 工作台：▶ 启动（VideoPlay）/ ⏸ 停止（VideoPause）
           - 官方对话：💬 打开（ChatDotRound）/ ⏹ 关闭（CircleClose）
           文字色仍随状态切换（关闭态淡红 btn-danger），查看日志淡青（只读）。
           全部用 type="text"（无底色无描边），仅靠文字色 + icon 区分。 -->
      <div class="btn-row">
        <el-button
          :class="{ 'btn-danger': running }"
          :icon="running ? VideoPause : VideoPlay"
          :loading="store.starting"
          :disabled="toggleDisabled"
          :title="running ? '停止工作台' : '启动工作台'"
          @click="onToggle"
        >
          工作台
        </el-button>
        <el-button
          :class="{ 'btn-chat': !officialChatOpen, 'btn-danger': officialChatOpen }"
          :icon="officialChatOpen ? CircleClose : ChatDotRound"
          :disabled="store.starting || globalBusy"
          :loading="isLoading('officialChat')"
          :title="officialChatOpen ? '关闭 DeepSeek 官方对话' : '打开 DeepSeek 官方对话'"
          @click="toggleOfficialChat"
        >
          官方对话
        </el-button>
        <el-button
          class="btn-view"
          :icon="Document"
          :loading="isLoading('openLogsWindow')"
          @click="openLogsWindow"
        >
          查看日志
        </el-button>
        <el-button
          v-if="store.shellUpdateVersion"
          type="warning"
          :icon="Refresh"
          :loading="isLoading('installShellUpdate')"
          :disabled="globalBusy"
           @click="installShellUpdate"
        >
          更新并重启
        </el-button>
      </div>

      <!-- 第二行：仅在对应服务开启后出现，作为窗口层的次级入口；
           视觉上压低权重（缩进 + ghost 风格），与第一行的主按钮做明显区分。
           「工作台窗口 / 官方对话窗口」只把窗口带到台前，「刷新工作台」是唯一的
           **动作**——它换掉整个工作台窗口（见 harness_cmd）。三者都不改变内核
           状态，所以按 AGENTS.md 的 IA 规则与「查看日志」同属次级入口这一排，
           不许往主按钮旁边堆会启停内核的动作。 -->
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
            title="唤起 / 聚焦 DeepSeek 官方对话窗口"
            @click="openOfficialChatWindow"
          >
            官方对话窗口
          </el-button>
        </div>
      </Transition>
      <p v-if="!store.starting && !running && !canStart" class="muted" style="margin: 0">
        尚未安装可用内核，请先到「内核版本」页安装。
      </p>
    </div>

    <!-- 安全网（环境回退点 / 深入排查）已挪到设置页，紧跟「数据迁移」下方。
         它们是故障时才用得上的兜底，常年摆在开机第一屏只会稀释真正要看的
         内容。 -->

    <!-- 套餐用量：独立只读卡，内容直接展示（无折叠）。MiniMax 双窗口进度 +
         DeepSeek 余额行；完整可操作错误文案只在顶部横幅出现，provider 分区
         仅用短状态词标注。凭据复用工作台模型设置，这张卡不带任何写操作。 -->
    <div class="card">
      <div class="card-head">
        <h2>
          套餐用量
          <el-tooltip placement="bottom-start" :show-after="80">
            <template #content>
              <div class="card-info-tooltip">
                MiniMax（国内站 / 国际站）与智谱的 5 小时 / 周窗口余额、DeepSeek 按量余额（多币种）。
                数据缓存 5 分钟，点「刷新」立即重新查询。凭据复用工作台模型设置；
                未在内核配置对应厂商时，相应分区自动隐藏。
              </div>
            </template>
            <el-icon class="card-info-icon"><InfoFilled /></el-icon>
          </el-tooltip>
        </h2>
        <span v-if="anyKeyConfigured" class="plan-head-actions">
          <el-button
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
        <div v-for="row in balanceRows" :key="row.provider.id" class="plan-provider">
          <div class="plan-provider-head">
            <span class="plan-provider-name">{{ row.provider.label }}</span>
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
            <div v-for="(item, index) in row.balances" :key="index" class="plan-balance" :title="item.tip">
              <span>{{ item.main }}</span>
              <!-- 赠金 / 充值与总额分开列（总额含赠金），独占一行。 -->
              <span v-if="item.detail" class="plan-balance-detail">{{ item.detail }}</span>
              <span v-if="row.provider.is_available === false" class="plan-balance-unavailable">
                余额不足，无法发起调用
              </span>
            </div>
          </template>
          <p
            v-if="row.shortState"
            class="plan-state"
            :class="{ 'plan-state-bad': row.provider.fetch_error || row.provider.credential_status === 'expired' || row.provider.error }"
          >
            {{ row.shortState }}
          </p>
        </div>
        <!-- 套餐类：左右自适应栅格，新增 provider 依次往后排。 -->
        <div class="plan-grid">
          <div v-for="row in planBlockRows" :key="row.provider.id" class="plan-provider">
            <div class="plan-provider-head">
              <span class="plan-provider-name">{{ row.provider.label }}</span>
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
            <div v-for="tier in row.tiers" :key="tier.name" class="plan-tier-col">
              <div class="plan-tier-head">
                <span class="plan-tier-name">{{ tier.name }}</span>
                <span v-if="tier.unlimited" class="plan-tier-unlimited">♾️ 无限周额度</span>
                <span class="muted plan-tier-reset" :title="tier.countdownTitle">
                  <el-icon v-if="tier.countdown" class="plan-reset-icon"><Timer /></el-icon>{{ tier.countdown || '' }}
                </span>
              </div>
              <div v-if="!tier.unlimited" class="plan-bar" role="img" :aria-label="tier.tip" :title="tier.tip">
                <i :class="'plan-bar-fill level-' + tier.level" :style="{ width: tier.percent + '%' }"></i>
                <!-- 剩余百分比居中显示在进度条上。 -->
                <span class="plan-bar-percent">{{ tier.percent }}%</span>
              </div>
            </div>
            <p
              v-if="row.shortState"
              class="plan-state"
              :class="{ 'plan-state-bad': row.provider.fetch_error || row.provider.credential_status === 'expired' || row.provider.error }"
            >
              {{ row.shortState }}
            </p>
          </div>
        </div>
        <p v-if="allHidden" class="muted" style="margin: 0">
          查不到数据的分区已按你的选择隐藏；修复凭据并成功查询（或重启后自动首查）后会自动恢复。
        </p>
        <p v-else-if="!planRows.length" class="muted" style="margin: 0">尚未查询，点击右上角「刷新」获取。</p>
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
.panel {
  gap: 6px;
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
/* 「当前内核」标题行：标题 + ℹ️ 在左，活动版本徽标独占最右。 */
.kernel-title {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
}
.kernel-version {
  margin-left: auto;
  font-size: 12px;
  font-weight: 500;
  color: var(--muted);
  font-family: var(--el-font-family, inherit);
  letter-spacing: 0.02em;
  padding: 1px 8px;
  border: 1px solid var(--el-border-color-extra-light);
  border-radius: 999px;
  /* 版本号前有 tag 图标，胶囊改用 .age-pill 同款 flex 结构（对齐方式与图标间距
     都对齐那一条，避免同文件里两种胶囊基线不一致）。图标尺寸走 Lucide 的 size
     属性而不是 CSS——它是带 width/height 属性的 svg，font-size 管不到它。 */
  display: inline-flex;
  align-items: center;
  gap: 3px;
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
/* 套餐用量卡内容：横幅 + provider 分区的纵向间距。 */
.plan-body {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.plan-error {
  --el-alert-padding: 6px 10px;
}
.plan-provider {
  display: flex;
  flex-direction: column;
  gap: 3px;
  /* 分块展示：描边 + 微底色 + 圆角，与独立窗口的 provider 分区同语言。 */
  border: 1px solid var(--el-border-color-extra-light);
  border-radius: 10px;
  padding: 6px 10px;
  background: var(--el-fill-color-light);
}
/* 套餐类：grid 自适应栅格。480 窗口稳定两列并排（MiniMax 与 GLM 同行），
   更宽窗口自动三列；新增 provider 依次往后排。 */
.plan-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(170px, 1fr));
  gap: 8px;
}
.plan-grid .plan-provider {
  min-width: 0;
}
/* 窄块内的 tier：名称 / 百分比 / 倒计时一行，进度条独占下一行。 */
.plan-tier-col {
  display: flex;
  flex-direction: column;
  gap: 3px;
}
.plan-tier-head {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 4px 6px;
  font-size: 12px;
  line-height: 1.5;
}
.plan-tier-head .plan-tier-name {
  color: var(--muted);
  font-weight: 600;
}
.plan-tier-head .plan-tier-reset {
  margin-left: auto;
}
.plan-provider-head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  flex-wrap: wrap;
  gap: 4px 8px;
  line-height: 1.4;
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
  color: var(--muted);
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
}
.plan-reset-icon {
  font-size: 12px;
}
.plan-tier-name {
  flex: none;
  width: 68px;
  color: var(--muted);
}
.plan-bar {
  position: relative;
  /* 不能写 flex: 1：父级 .plan-tier-col 是**纵向** flex，flex-basis 0% 作用于
     高度并压过 height: 10px，WebView2（Chromium）上进度条会被压成 0 高
     （WebKit 对自动最小尺寸的实现不同，macOS 上看不出来）。宽度交给
     纵向 flex 的默认 cross 拉伸即可。 */
  flex: none;
  width: 100%;
  height: 10px;
  border-radius: 5px;
  background: rgba(255, 255, 255, 0.08);
  overflow: hidden;
}
/* 剩余百分比：绝对定位水平垂直居中，白色文字（红档浅底上仍可读）。 */
.plan-bar-percent {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 10px;
  line-height: 1;
  font-weight: 600;
  color: #fff;
  pointer-events: none;
}
.plan-bar-fill {
  display: block;
  height: 100%;
  border-radius: 4px;
}
/* 进度条三档配色（剩余口径）：≥70 绿 / 40–69.99 橙 / <39.99 红。 */
.plan-bar-fill.level-ok {
  background: #15803d;
}
.plan-bar-fill.level-warning {
  background: var(--el-color-warning);
}
.plan-bar-fill.level-danger {
  background: var(--el-color-danger);
}
.plan-tier-reset {
  flex: none;
  min-width: 96px;
  /* inline-flex 容器不吃 text-align，用 justify-content 让图标+文字贴右缘。 */
  justify-content: flex-end;
  text-align: right;
}
.plan-balance {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 4px 8px;
  font-size: 12px;
}
/* 余额：总额与赠金 / 充值明细同行并排（gap 隔开），告警仍靠右。 */
.plan-balance-unavailable {
  margin-left: auto;
  color: var(--el-color-danger);
  font-weight: 600;
}
.plan-balance-detail {
  color: var(--muted);
}
.plan-state {
  margin: 0;
  font-size: 12px;
  color: var(--muted);
}
.plan-state-bad {
  color: var(--el-color-danger);
}
</style>
