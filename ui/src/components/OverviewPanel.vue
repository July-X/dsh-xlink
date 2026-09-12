<script setup>
// 概览：当前内核状态、工作台启停单按钮状态机、首次运行引导、
// 外壳更新横幅与安装入口（手动检查在侧栏品牌区）以及启动容错横幅。
// 内核生命周期是实现细节，只暴露「打开/关闭工作台 / 打开/关闭官方对话 / 查看日志」；
// 「打开工作台窗口 / 打开官方对话窗口」在对应服务开启后作为次级入口从第二行动态浮现。
// 「当前内核」的 Node.js 行另带「重新检测」（探测本机环境，不改设置），卡片底部是
// 「桌面端设置」只读摘要（端口 / 接线 profile / Node 环境结论）。
import { computed, ref } from 'vue';
import {
  SwitchButton,
  TopRight,
  Document,
  ChatDotRound,
  Refresh,
  FolderOpened,
  Download,
  Box,
  Monitor,
  Setting,
  Warning,
  Connection,
  View,
} from '@element-plus/icons-vue';
import {
  store,
  showIncident,
  startWorkbench,
  stopWorkbench,
  openHarnessWindow,
  toggleOfficialChat,
  openOfficialChatWindow,
  openDataDir,
  installShellUpdate,
  installLatestRelease,
  checkUpdates,
  installNode,
  detectNode,
} from '../store.js';
import { progress } from '../progress.js';
import { globalBusy, isLoading, withLoading } from '../loading.js';
import { showLogs } from '../logs.js';

// 进度窗口是全局的（任何长任务都会让它可见），按钮的加载态必须绑定自己的
// key，否则任何别的长任务都会让这个按钮转圈（P2-42）。
const onInstallNode = () => withLoading('installNode', () => installNode());

const kernel = computed(() => store.view && store.view.kernel);
const node = computed(() => store.view && store.view.node);

// 「重新检测」（由设置页搬来）探测的是本机环境，而 detect_node 不会让 Rust 侧的状态
// 缓存失效，所以探测结果就地覆盖 Node 两处显示；离开概览页再回来即回到状态里的值。
const detectedNode = ref(null);
const shownNode = computed(() => detectedNode.value || node.value);

const running = computed(() => !!(kernel.value && kernel.value.running));
const officialChatOpen = computed(() => !!(store.view && store.view.official_chat_open));
const officialChatLabel = computed(() => (officialChatOpen.value ? '关闭官方对话' : '打开官方对话'));
const canStart = computed(() => !!(kernel.value && kernel.value.active && kernel.value.active_installed));
const noKernel = computed(() => !!(kernel.value && (!kernel.value.installed || kernel.value.installed.length === 0)));

const nodeText = computed(() => {
  const n = shownNode.value;
  if (!n) return '—';
  return n.ok ? [n.path, n.version].filter(Boolean).join('  ') : '未检测到可用 Node（' + n.reason + '）';
});

const urlText = computed(() => (running.value ? 'http://127.0.0.1:' + kernel.value.port : '—'));

// 「桌面端设置」摘要卡：设置页那张卡的核心值只读罗列在这里，省去为了确认端口 /
// 接线 profile / Node 是否达标而切页；改端口只在设置页，Node 重新检测在「当前内核」
// 的 Node.js 行，这张卡不带任何写操作。
const settings = computed(() => (store.view && store.view.settings) || null);

const portText = computed(() => {
  const value = settings.value && settings.value.port;
  return value ? String(value) : '—';
});

const profileText = computed(() => (settings.value && settings.value.profile) || 'web');

// Node 结论与设置页同口径（是否满足 dsh 的 ^22.19 || >=24）。不达标时只给一句结论：
// 具体原因与「自动安装」入口就在上面的「当前内核」卡里，这里不重复一遍。
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

function goSettings() {
  store.activePanel = 'settings';
}

// 「（dev）」后缀是 release-only 钩子：dev 构建里标出来，开了 release 预览
// 就按正式版隐藏（store.devUi 已经把预览算进去）。
const shellVersionText = computed(() =>
  store.view ? 'v' + store.view.shell_version + (store.devUi ? '（dev）' : '') : '—'
);

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

function incidentCause(value) {
  if (!value) return '';
  if (['plugin', 'kernel', 'frontend', 'unknown'].includes(value.cause)) return value.cause;
  const suspects = value.suspects || [];
  if (suspects.some((suspect) => suspect.kind === 'plugin')) return 'plugin';
  if (suspects.some((suspect) => suspect.kind === 'kernel')) return 'kernel';
  return 'unknown';
}

const guardDestination = computed(() => {
  const cause = incidentCause(store.lastIncident);
  return cause === 'plugin' || quarantined.value.length > 0 ? 'plugins' : 'versions';
});
const guardDestinationLabel = computed(() =>
  guardDestination.value === 'plugins' ? '前往插件页' : '检查内核版本'
);
// 非致命前端异常不弹模态框（见 store.js 的 showIncident）：横幅是它唯一的入口，
// 因此这里必须显式要求打开面板，否则「查看详情」会变成空操作。
const guardTitle = computed(() =>
  incidentCause(store.lastIncident) === 'frontend' ? '工作台自检：前端异常（页面正常）' : '启动容错已介入'
);
function openIncidentDetails() {
  showIncident(store.lastIncident, { force: true });
}
function goGuardDestination() {
  store.activePanel = guardDestination.value;
}

const toggleLabel = computed(() => {
  if (store.starting) return '正在启动…';
  return running.value ? '关闭工作台' : '打开工作台';
});

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
      <h2>当前内核</h2>
      <dl class="kv">
        <dt>运行状态</dt>
        <dd>{{ running ? '运行中' : '未运行' }}</dd>
        <dt>活动版本</dt>
        <dd>{{ (kernel && kernel.active) || '（未选择）' }}</dd>
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
        <dt>桌面端版本</dt>
        <dd>{{ shellVersionText }}</dd>
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
           文字色随状态切换：
           - 打开态（打开工作台 / 打开官方对话）：白 / 淡绿
           - 关闭态（关闭工作台 / 关闭官方对话）：淡红（btn-danger）
           - 查看日志：淡青（始终只读）
           全部用 type="text"（无底色无描边），仅靠文字色 + icon 区分。 -->
      <div class="btn-row">
        <el-button
          :class="{ 'btn-danger': running }"
          :icon="SwitchButton"
          :loading="store.starting"
          :disabled="toggleDisabled"
          @click="onToggle"
        >
          {{ toggleLabel }}
        </el-button>
        <el-button
          :class="{ 'btn-chat': !officialChatOpen, 'btn-danger': officialChatOpen }"
          :icon="ChatDotRound"
          :disabled="store.starting || globalBusy"
          :loading="isLoading('officialChat')"
          title="打开或关闭 DeepSeek 官方对话"
          @click="toggleOfficialChat"
        >
          {{ officialChatLabel }}
        </el-button>
        <el-button
          class="btn-view"
          :icon="Document"
          @click="showLogs"
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

      <!-- 第二行：仅在对应服务开启后出现，作为「打开 X 窗口」的次级入口；
           视觉上压低权重（缩进 + ghost 风格），与第一行的主按钮做明显区分。 -->
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
            打开工作台窗口
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
            打开官方对话窗口
          </el-button>
        </div>
      </Transition>
      <p v-if="!store.starting && !running && !canStart" class="muted" style="margin: 0">
        尚未安装可用内核，请先到「内核版本」页安装。
      </p>
    </div>

    <!-- 桌面端设置摘要：只读。端口输入、保存与 Node 重新检测都不在这里
         （设置页只留端口，检测在「当前内核」的 Node.js 行），这张卡只把当前取值
         与结论放到一眼可见的位置。 -->
    <div class="card">
      <div class="card-head">
        <h2>桌面端设置</h2>
        <el-button
          text
          size="small"
          :icon="Setting"
          title="修改 Web UI 端口"
          @click="goSettings"
        >
          前往设置
        </el-button>
      </div>
      <dl class="kv">
        <dt>Web UI 端口</dt>
        <dd>{{ portText }}</dd>
        <dt>插件接线 profile 名</dt>
        <dd><code>{{ profileText }}</code></dd>
        <dt>Node.js 环境</dt>
        <dd>{{ nodeRequirementText }}</dd>
      </dl>
    </div>
  </section>
</template>
