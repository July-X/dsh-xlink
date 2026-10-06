<script setup>
// 内核状态诊断：内核在不在、装的是哪个版本、跑在哪个端口、数据在哪。
//
// **这一层不主动探测**。内核可能在跑、可能没跑，而「探测」本身有成本
// （一次网络往返 + 一次可能的看护触发）。用户点「查看状态」的语义是
// 「告诉我我看到的东西是什么意思」，不是「再跑一次」，所以全部读 store
// 里已有的状态快照。
//
// **字段只取后端真的返回的**（`kernel::lifecycle::KernelStatus`）：没有
// pid、没有 uptime、没有 profile。编一个「运行时长」出来会骗用户——
// 它拿不到 pid 是因为 pid 文件只在内核以受管方式启动时才写。
import { computed } from 'vue';
import { store } from '../store.js';
import { showLogs } from '../logs/logs.js';

const kernel = computed(() => store.view?.kernel || {});
const running = computed(() => !!kernel.value.running);

const rows = computed(() => {
  const list = [
    {
      key: 'status',
      label: '运行状态',
      value: running.value ? '运行中' : '未运行',
      tone: running.value ? 'ok' : 'warn',
    },
    {
      key: 'version',
      label: '内核版本',
      value: kernel.value.active || '未安装',
    },
    {
      key: 'port',
      label: '端口',
      value: kernel.value.port != null ? String(kernel.value.port) : '未设置',
    },
    {
      key: 'installed',
      label: '已安装版本',
      value: `${(kernel.value.installed || []).length} 个`,
    },
    {
      key: 'dataDir',
      label: '数据目录',
      value: kernel.value.dataDir || '未知',
    },
  ];
  return list;
});

/**
 * 设置被改过、而内核还跑在旧端口上时会有一条说明。
 * **非空时必须显示**：此时端口已无声回退到默认值，用户只会看到
 * 「工作台跑到别的端口去了」。
 */
const settingsWarning = computed(() => kernel.value.settings_warning || '');

function viewLogs() {
  showLogs();
}
</script>

<template>
  <div class="diag-card">
    <div class="diag-status">
      <span
        class="diag-status__dot"
        :class="running ? 'diag-status__dot--ok' : 'diag-status__dot--warn'"
        aria-hidden="true"
      ></span>
      <span>{{ running ? '运行中' : '未运行' }}</span>
    </div>
    <p class="diag-meta">
      这些是当前状态快照，不代表刚刚做过一次探测。要看内核自己怎么说，打开完整日志。
    </p>
    <p v-if="settingsWarning" class="diag-error">{{ settingsWarning }}</p>
  </div>

  <div class="diag-card">
    <h3 class="diag-card__title"><span>内核状态</span></h3>
    <div class="diag-rows">
      <div v-for="row in rows" :key="row.key" class="diag-row diag-row--static">
        <span class="diag-row__label">{{ row.label }}</span>
        <span class="diag-row__value" :class="row.tone ? `diag-row__value--${row.tone}` : ''">
          {{ row.value }}
        </span>
      </div>
    </div>
  </div>

  <div class="diag-actions">
    <el-button @click="viewLogs">查看完整日志</el-button>
  </div>
</template>