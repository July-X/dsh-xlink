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
import { Refresh } from '@element-plus/icons-vue';
import { store } from '../store.js';
import { isLoading } from '../shell/loading.js';
import { diagnosticStore } from './diagnostics.js';
import { loadKernelStatusDiagnosis, openEvidence } from './diagnostic-actions.js';

const kernel = computed(() => store.view?.kernel || {});
const running = computed(() => !!kernel.value.running);

/**
 * 这一页读的是 store 的状态快照，**不是**一次刚做完的探测。
 *
 * 上一版把这句话原样写进页面，却在头部放了个「刷新状态」——那个按钮当时
 * 什么都不做（审查 P1-06）。用户点了没反应，却以为自己已经拿到新状态：
 * 那比没有这个按钮更糟。现在它真的去重读，而「读到的到底是哪一刻」必须
 * 说出来：用户改完端口再点刷新，页面上没有任何东西能证明它已经变了。
 *
 * 时间戳记在诊断状态里而不是后端：后端那份 `KernelStatus` 描述的是环境，
 * 加一个「我什么时候读的」是前端自己的读数事实，塞进去会让每次调用都带一
 * 个只有某一处关心的字段。
 */
const readAtLabel = computed(() => {
  const at = Number(diagnosticStore.kernelReadAt || 0);
  return at ? new Date(at).toLocaleTimeString() : '';
});

// 读取失败时**保留**上一次的值并说明它可能过期，而不是把整页清空。
// 清空会被读成「内核没了」，而真相只是这一次没读到。
const stale = computed(() => !!diagnosticStore.error);

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
  // 走诊断层的 `openEvidence`：它会带上这次记录里的内核日志路径，定位到
  // 那一份而不是日志列表的第一份（审查 P1-02）。
  openEvidence();
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
      这些是状态快照，不代表刚刚做过一次探测。
      <template v-if="readAtLabel">读取时刻 {{ readAtLabel }}。</template>
      要看内核自己怎么说，打开完整日志。
    </p>
    <!-- 读失败时**不隐藏**上一次的值，而是明说它可能已经过期（设计 §9.4）。
         静默保留会让用户把旧值当成现在的状态。 -->
    <p v-if="stale" class="diag-error">
      {{ diagnosticStore.error }}——下面是上一次读到的值，可能已经过期。
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
    <el-button
      :icon="Refresh"
      :loading="isLoading('kernelStatusReload')"
      @click="loadKernelStatusDiagnosis(true)"
    >
      刷新状态
    </el-button>
    <el-button @click="viewLogs">查看完整日志</el-button>
  </div>
</template>