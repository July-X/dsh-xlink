<script setup>
// 安装预检报告：把沙盒里那次启动观测原样摊给用户看。
//
// 报告是三态而不是两态——通过 / 未通过 / 未能验证。把第三种画成「通过」
// 会让用户以为插件已经过检验；画成「失败」则会让用户去卸一个无辜的包。
// 所以这里的标题、配色、可执行动作都按三态分开，而不是拿一个布尔渲染两套。
import { computed, ref } from 'vue';
import { View } from '@element-plus/icons-vue';
import { store } from '../store.js';
import { showLogs } from '../logs.js';

const report = computed(() => store.precheckReport || {});

// 三态 → 展示元数据。`type` 交给 el-tag 的 type，语义与 el-tag 的
// success / danger / warning 一一对应。
const VERDICTS = {
  pass: { label: '预检通过', type: 'success' },
  fail: { label: '预检未通过', type: 'danger' },
  inconclusive: { label: '预检未能完成', type: 'warning' },
};

const verdict = computed(() => VERDICTS[report.value.verdict] || VERDICTS.inconclusive);

// 通过但带告警时，标题不能只说「通过」——那会把「启动日志里有可疑标记」
// 这件事藏起来，而这恰恰是最值得用户停一下看一眼的情况。
const title = computed(() =>
  report.value.verdict === 'pass' && (report.value.warnings || []).length
    ? '预检通过，但有告警'
    : verdict.value.label
);

const evidenceOpen = ref(false);

// 「查看日志」只在报告真的给出了落盘路径时才有意义（预检通过时为空串）。
const evidencePath = computed(() => report.value.evidencePath || '');

function openLog() {
  store.precheckVisible = false;
  showLogs();
}

function close() {
  store.precheckVisible = false;
}
</script>

<template>
  <el-dialog
    v-model="store.precheckVisible"
    :title="title"
    width="min(760px, 92vw)"
    append-to-body
    @closed="evidenceOpen = false"
  >
    <div class="precheck">
      <div class="precheck-head">
        <el-tag :type="verdict.type" effect="dark" size="large">{{ verdict.label }}</el-tag>
        <span v-if="report.pluginName" class="precheck-name">{{ report.pluginName }}</span>
        <span v-if="report.durationMs" class="precheck-cost">耗时 {{ (report.durationMs / 1000).toFixed(1) }} 秒</span>
      </div>

      <p v-if="report.summary" class="precheck-summary">{{ report.summary }}</p>

      <el-alert
        v-for="(warning, index) in report.warnings || []"
        :key="index"
        type="warning"
        :closable="false"
        show-icon
        class="precheck-warning"
      >
        <template #title>启动日志里的可疑标记：{{ warning }}</template>
      </el-alert>

      <p v-if="report.hint" class="precheck-hint">{{ report.hint }}</p>

      <div v-if="report.evidence" class="precheck-evidence">
        <el-button text :icon="View" @click="evidenceOpen = !evidenceOpen">
          {{ evidenceOpen ? '收起启动日志' : '查看启动日志证据' }}
        </el-button>
        <pre v-if="evidenceOpen" class="precheck-log">{{ report.evidence }}</pre>
      </div>
    </div>

    <template #footer>
      <el-button v-if="evidencePath" @click="openLog">打开日志</el-button>
      <el-button type="primary" @click="close">知道了</el-button>
    </template>
  </el-dialog>
</template>

<style scoped>
.precheck-head {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
}

.precheck-name {
  font-weight: 600;
  color: var(--text);
}

.precheck-cost {
  font-size: 12px;
  color: var(--text-muted);
  margin-left: auto;
}

.precheck-summary {
  margin: 14px 0 0;
  line-height: 1.7;
  color: var(--text);
}

.precheck-warning {
  margin-top: 12px;
}

.precheck-hint {
  margin: 14px 0 0;
  font-size: 13px;
  line-height: 1.7;
  color: var(--text-muted);
}

.precheck-evidence {
  margin-top: 12px;
}

/* 启动日志是原始输出：等宽 + 保留换行，宽度不够时按字符断行而不是把
   堆栈折成看不懂的形状。配色沿用主题里既有的 --muted / --border，
   不在这里新造一套（theme.css 里的变量是唯一定义处）。 */
.precheck-log {
  margin: 10px 0 0;
  max-height: 260px;
  overflow: auto;
  padding: 12px;
  border-radius: 10px;
  border: 1px solid var(--border);
  background: rgba(0, 0, 0, 0.28);
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: 11.5px;
  line-height: 1.6;
  white-space: pre-wrap;
  word-break: break-all;
  color: var(--muted);
}
</style>
