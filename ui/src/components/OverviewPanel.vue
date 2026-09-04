<script setup>
// 概览：当前内核状态、工作台启停单按钮状态机、首次运行引导、
// 外壳更新横幅与安装入口（手动检查在侧栏品牌区）以及启动容错横幅。
// 内核生命周期是实现细节，只暴露「打开/关闭工作台 / 打开/关闭官方对话 / 查看日志」；
// 「打开工作台窗口 / 打开官方对话窗口」在对应服务开启后作为次级入口从第二行动态浮现。
import { computed } from 'vue';
import {
  SwitchButton,
  TopRight,
  Document,
  ChatDotRound,
  Refresh,
  FolderOpened,
  Download,
  Box,
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
} from '../store.js';
import { progress } from '../progress.js';
import { globalBusy, isLoading } from '../loading.js';
import { showLogs } from '../logs.js';

const kernel = computed(() => store.view && store.view.kernel);
const node = computed(() => store.view && store.view.node);

const running = computed(() => !!(kernel.value && kernel.value.running));
const officialChatOpen = computed(() => !!(store.view && store.view.official_chat_open));
const officialChatLabel = computed(() => (officialChatOpen.value ? '关闭官方对话' : '打开官方对话'));
const canStart = computed(() => !!(kernel.value && kernel.value.active && kernel.value.active_installed));
const noKernel = computed(() => !!(kernel.value && (!kernel.value.installed || kernel.value.installed.length === 0)));

const nodeText = computed(() => {
  const n = node.value;
  if (!n) return '—';
  return n.ok ? [n.path, n.version].filter(Boolean).join('  ') : '未检测到可用 Node（' + n.reason + '）';
});

const urlText = computed(() => (running.value ? 'http://127.0.0.1:' + kernel.value.port : '—'));

const shellVersionText = computed(() =>
  store.view ? 'v' + store.view.shell_version + (store.view.dev_build ? '（dev）' : '') : '—'
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
  if (value.cause === 'plugin' || value.cause === 'kernel' || value.cause === 'unknown') return value.cause;
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
            v-if="node && !node.ok"
            size="small"
            text
            type="primary"
            :loading="progress.visible"
            :disabled="globalBusy"
            title="自动下载并安装官方 Node.js 到数据目录（需联网）"
            @click="installNode"
          >
            自动安装
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
          <h3>启动容错已介入</h3>
          <p>{{ guardText }}</p>
          <div class="btn-row">
            <el-button size="small" type="warning" plain :icon="View" @click="showIncident(store.lastIncident)">
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
  </section>
</template>
