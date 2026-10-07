<script setup>
// 安装预检报告：把沙盒里那次启动观测原样摊给用户看。
//
// 报告是三态而不是两态——通过 / 未通过 / 未能验证。把第三种画成「通过」
// 会让用户以为插件已经过检验；画成「失败」则会让用户去卸一个无辜的包。
// 所以这里的标题、配色、可执行动作都按三态分开，而不是拿一个布尔渲染两套。
import { computed, ref } from 'vue';
import { Check, View } from '@element-plus/icons-vue';
import { store } from '../store.js';
import { isLoading } from '../shell/loading.js';
import { applyPrecheckChange } from '../diagnostics/diagnostic-actions.js';
import { openLogsForReport } from '../logs/logs.js';
import { openPluginDiagnosis } from '../diagnostics/diagnostics.js';
import PluginCleanupButton from '../diagnostics/PluginCleanupButton.vue';

const report = computed(() => store.precheckReport || {});

// 三态 → 展示元数据。`type` 交给 el-tag 的 type，语义与 el-tag 的
// success / danger / warning 一一对应。
const VERDICTS = {
  pass: { label: '预检通过', type: 'success' },
  fail: { label: '预检未通过', type: 'danger' },
  inconclusive: { label: '预检未能完成', type: 'warning' },
};

const verdict = computed(() => VERDICTS[report.value.verdict] || VERDICTS.inconclusive);

/** 打开插件安全诊断，并把本次报告一并带过去（诊断层不自己再跑一次预检）。 */
function openFullDiagnosis() {
  openPluginDiagnosis(
    { id: report.value.pluginId, name: report.value.pluginName, report: report.value },
    store.activePanel
  );
  close();
}

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
  // 带上这份报告的证据路径，不带就是打开日志目录里的随便哪一份。
  openLogsForReport(report.value);
}

/** 能不能应用：**只有预检通过**，且**还没装上**。 */
const canApply = computed(() => report.value.verdict === 'pass' && !report.value.installed);

const applyDisabledReason = computed(() => {
  if (report.value.installed) return '已经装到当前实例了，不用再应用一次。';
  if (report.value.verdict === 'fail')
    return '预检没通过，应用会把这个插件装进一个已经确认起不来的实例。要装的话请先到插件中心关闭安装预检。';
  if (report.value.verdict === 'inconclusive')
    return '预检没能完成，这次没有拿到「装上去能起来」的证据。要装的话请先到插件中心关闭安装预检。';
  return '预检还没跑，先做一次安装预检。';
});

function apply() {
  return applyPrecheckChange(report.value);
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

      <!-- 「验证通过」与「已经装上」是两件事，必须各占一行大字。用户看到
           「预检通过」就以为插件在里面了，点「知道了」之后去工作台里找半天，
           是这套工具最容易犯的错。 -->
      <p v-if="!report.installed" class="precheck-notinstalled">
        这次预检<b>没有改动</b>当前实例。插件只有在你点「应用变更」之后才会装上。
      </p>

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
      <!-- 完整的分阶段时间线（建沙盒 → 基线 → 装候选 → 探测）在诊断层里。
           这里保留这个弹窗：它是一次性的「刚装完」告知，而诊断层是可以随时
           回看的记录页，两者职责不同。

           「查看完整诊断」**只留 footer 这一处**（2026-10-07 用户：按钮重复）。
           同一个动作原来在正文里还有一个文字按钮，与 footer 那枚并排出现在
           同一屏，用户的箭头一开始指的正是正文那枚。动作级按钮统一放 footer，
           正文只留说明文字——正文里再挂一枚同款按钮，用户会以为是两个不同的
           动作（一个看时间线、一个只是「跳过去」），而它们其实完全等价。 -->
      <el-button v-if="report.runId" @click="openFullDiagnosis">查看完整诊断</el-button>
      <el-button v-if="evidencePath" @click="openLog">打开日志</el-button>
      <!-- 普通预检失败的 installed=false 已经回滚下载；只有旧的 fail-open 报告才可清理。 -->
      <PluginCleanupButton
        v-if="report.pluginId && report.installed === true && report.verdict !== 'pass'"
        :plugin-id="report.pluginId"
        @cleaned="close"
      />
      <el-button @click="close">知道了</el-button>
      <!-- 「应用变更」是这次预检报告里的安装动作，只有通过且尚未安装时可用。
           「移除并清理」是失败旧报告的危险动作，单独放在确认框里。
           其余禁用情况必须说清为什么而不是灰着就完事——
           一个不说理由的灰按钮会让人以为界面坏了。
           `:disabled="canApply"` 而不是把 content 传成空串：空 content 的
           el-tooltip **照样弹出一个空的 popper**，用户看到的是按钮上方一个
           没有字的气泡（2026-10-07 实机）。要「什么都不说」就得让 tooltip
           整个不出现。`PluginDiagnosis.vue` 那一处是同一份，两边要一起改。 -->
      <el-tooltip :content="applyDisabledReason" :disabled="canApply" placement="top">
        <span>
          <el-button
            v-if="!report.installed"
            type="primary"
            :icon="Check"
            :disabled="!canApply"
            :loading="isLoading('pluginPrecheckApply')"
            @click="apply"
          >
            应用变更
          </el-button>
        </span>
      </el-tooltip>
    </template>
  </el-dialog>
</template>

<style scoped>
.precheck-notinstalled {
  margin: 8px 0 0;
  padding: 8px 10px;
  border-left: 3px solid var(--accent, #409eff);
  background: var(--surface-soft, rgba(64, 158, 255, 0.08));
  font-size: 13px;
  line-height: 1.5;
}

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
  background: var(--overlay-faint);
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: 11.5px;
  line-height: 1.6;
  white-space: pre-wrap;
  word-break: break-all;
  color: var(--text-secondary);
}
</style>
