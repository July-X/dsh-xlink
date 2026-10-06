<script setup>
// 概览页控制塔：三块只读信息 + 诊断入口。
//
// **为什么单独一个组件**：概览页已经 900 行，而这三块与「当前内核」卡片
// 是并列关系而不是它的细节。塞进去只会让那个文件更难读，而它本来就
// 在代码预算的反棘轮上（只许越来越小）。
//
// 三块各自的取舍：
// - **需要关注**：只在真有东西要说时出现。空列表不占位置。
// - **系统健康**：只给读数，不给按钮。它回答「能不能跑」，处置在各自面板。
// - **最近操作**：回答「上次发生了什么」。失败 / 告警才可点进诊断；
//   成功记录不给按钮——它没什么可诊断的。
import { computed } from 'vue';
import { store } from '../store.js';
import { isLoading } from '../shell/loading.js';
import { entryTimeLabel, snapshotStore } from './snapshots.js';
import { skillStore } from '../skills/skills.js';
import { logModal } from '../logs/logs.js';
import { showLogs } from '../logs/logs.js';
import { incidentCauseLabel, incidentTitle } from '../incidents/incidents.js';
import {
  clearDiagnosticRuns,
  diagnosticStore,
  loadRecentRuns,
  openRunDiagnosis,
} from './diagnostics.js';
import { causeLabel, kindLabel, statusMeta } from './diagnostic-labels.js';

const emit = defineEmits(['open-incident', 'go-panel']);

const kernel = computed(() => store.view?.kernel || {});
const node = computed(() => store.view?.node || {});
const nodeOk = computed(() => !!node.value.ok);
const incident = computed(() => store.view?.incident || null);
const quarantined = computed(() => store.view?.quarantined || []);



/**
 * 需要关注：只有真有东西才列。
 *
 * 判据全部来自 store 已有状态，**不做任何探测**——概览是用户随手看的
 * 页面，一进来就发起三次网络往返会让它在慢机器上明显卡顿。
 */
const attention = computed(() => {
  const list = [];
  if (incident.value) {
    list.push({
      key: 'incident',
      label: incidentTitle(incident.value),
      detail: incidentCauseLabel(incident.value),
      tone: 'bad',
      action: 'incident',
    });
  }
  if (store.settingsWarning) {
    list.push({
      key: 'settings',
      label: '设置无法读取，已回退到默认值',
      detail: store.settingsWarning,
      tone: 'warn',
      action: 'settings',
    });
  }
  if (kernel.value.other_shell_workbench) {
    list.push({
      key: 'other-shell',
      label: '另一个桌面端的工作台正在运行',
      detail: '装 / 删内核与切换端口会影响它。',
      tone: 'warn',
      action: 'other-shell',
    });
  }
  // 上次预检没完成 / 没通过：这是**用户可能还没意识到**的一类问题——
  // 插件已经装上了，但「没验过」这件事不会自己跳出来提醒。
  const lastPrecheck = diagnosticStore.recentRuns.find((r) => r.kind === 'plugin-precheck');
  if (lastPrecheck && ['failure', 'inconclusive', 'warning'].includes(lastPrecheck.status)) {
    list.push({
      key: 'precheck',
      label: `插件预检${statusMeta(lastPrecheck.status).label}`,
      detail: lastPrecheck.summary || '点开看它验到了哪一步',
      tone: lastPrecheck.status === 'warning' ? 'warn' : 'bad',
      action: 'precheck',
      run: lastPrecheck,
    });
  }
  if (!nodeOk.value) {
    list.push({
      key: 'node',
      label: '未找到满足要求的 Node.js',
      detail: node.value.reason || '启动工作台前需要先准备 Node.js 环境。',
      tone: 'bad',
      action: 'node',
    });
  }
  return list;
});

/**
 * 最近快照的时间。
 *
 * **只在有 `lastKnownGood` 时显示**：随便挑一个最新的快照当"最近恢复点"
 * 会指向一个从来没被验证过的组合——恢复入口应该指向真正确认能跑的那次。
 */
const latestSnapshotLabel = computed(() => {
  const view = snapshotStore.view;
  if (!view || !view.lastKnownGoodId) return '';
  const entry = (view.entries || []).find((item) => item.id === view.lastKnownGoodId);
  return entry ? entryTimeLabel(entry) : '';
});

/** 系统健康：只给读数。 */
/**
 * 系统健康：四个**子系统的读数**。
 *
 * **为什么只剩四项**：内核版本、运行状态、Node.js 三行此前在这里各占一整行，
 * 而它们在正下方「当前内核」卡里逐字重复一次（标题旁的版本徽标 + 「运行状态」
 * 胶囊 + 「Node.js」行带完整路径与版本号），侧栏品牌区还有第三份状态胶囊。
 * 同一句话说三遍不是冗余的排版，是把用户真正没读过的信息挤出屏幕。真正的
 * 异常不该靠这些行来报——Node 不达标进「需要关注」（带跳转），内核没装进首屏
 * callout，运行状态在「当前内核」卡。
 *
 * **`unavailable` 仍是一等状态**：某个数据源读失败时**不能**显示成「正常」——
 * 用户点进来看到一片正常会以为系统没事，而真相是这一项压根没读成功。读不到
 * 就明说读不到。
 */
function cell(key, label, value, tone = '', action = null) {
  return { key, label, value, tone, action, unavailable: value === '读取失败' };
}

const health = computed(() => [
  cell('wiring', '插件接线', wiringText.value.text, wiringText.value.tone, 'plugins'),
  cell('skills', '技能注册', skillText.value.text, skillText.value.tone, 'skills'),
  cell('logs', '日志系统', logText.value.text, logText.value.tone, 'logs'),
  cell('snapshot', '最近快照', latestSnapshotLabel.value || '暂无'),
]);

// 插件接线：隔离数是唯一有意义的读数——「被看护停用过」直接决定插件
// 还能不能正常工作，而 store 里没有总启数（接线明细要另发命令）。
// 值为 0 时只说「无隔离」：绿色本身已经说了「正常」，再写一遍是同一句话。
const wiringText = computed(() => {
  const count = quarantined.value.length;
  if (count === 0) return { text: '无隔离', tone: 'ok' };
  return { text: `${count} 个被隔离`, tone: 'warn' };
});

// 技能：view 为 null = 还没拉过或拉失败，两种情况都不能说「正常」。
const skillText = computed(() => {
  if (!skillStore.view) return { text: '读取失败', tone: 'bad' };
  return { text: '正常', tone: 'ok' };
});

// 日志：只报**份数**。「正常（N 个文件）」里的「正常」在有文件时是废话，
// 而没有文件那一档说「暂无」就够了。
const logText = computed(() => {
  if (!logModal.files.length) return { text: '暂无', tone: '' };
  return { text: `${logModal.files.length} 份`, tone: 'ok' };
});


/** 最近操作：最近一条运行记录；失败 / 告警时可点进诊断。 */
const latestRun = computed(() => diagnosticStore.recentRuns[0] || null);

const runMeta = computed(() => statusMeta(latestRun.value?.status));
// 四种 kind 都要说出来：只写「启动 / 预检 / 恢复」时，用户在一条排查记录
// 上找不到「排查」两个字，只能猜自己看到的是不是同一件事（审查 P1-05）。
const runKindLabel = computed(() => (latestRun.value ? kindLabel(latestRun.value.kind) : ''));
const runDetail = computed(() => {
  const run = latestRun.value;
  if (!run) return '';
  const parts = [];
  const cause = causeLabel(run.cause);
  if (cause) parts.push(cause);
  if (run.summary) parts.push(run.summary);
  else if (run.eventCount) parts.push(`${run.eventCount} 条阶段记录`);
  return parts.join(' · ');
});

// 只有失败 / 告警 / 未能完成才可点：成功的记录没有可诊断的内容，
// 给个按钮只会让用户点进去看到一句「一切正常」。
const runActionable = computed(() =>
  ['failure', 'warning', 'inconclusive'].includes(String(latestRun.value?.status || ''))
);

function onAttention(item) {
  if (item.action === 'incident') emit('open-incident');
  // 预检项带的是**那一条运行记录**，不是空 spec：此前传 `{}` 过去，插件页
  // 只能显示「未知插件」，时间线也是空的（审查 P1-05）。
  else if (item.action === 'precheck') openRunDiagnosis(item.run, 'overview');
  else if (item.action === 'settings' || item.action === 'node') emit('go-panel', 'settings');
  else emit('go-panel', 'overview');
}

/** 系统健康行按设计 §7.2 逐项可跳转：读数本身解释不了「怎么修」。 */
function onHealth(row) {
  if (row.action === 'plugins') emit('go-panel', 'plugins');
  else if (row.action === 'skills') emit('go-panel', 'skills');
  else if (row.action === 'logs') showLogs();
}

// 按记录类型分派，不再一律当启动诊断打开（审查 P1-05）。
function openDiagnosis() {
  openRunDiagnosis(latestRun.value, 'overview');
  loadRecentRuns(null);
}
</script>

<template>
  <div v-if="attention.length" class="diag-card diag-card--tower">
    <h3 class="diag-card__title">
      <span>需要关注</span>
      <span class="diag-card__aside">{{ attention.length }} 项</span>
    </h3>
    <div class="diag-rows">
      <button
        v-for="item in attention"
        :key="item.key"
        type="button"
        class="diag-row"
        @click="onAttention(item)"
      >
        <span class="diag-row__label">
          {{ item.label }}
          <span v-if="item.detail" class="diag-row__value">· {{ item.detail }}</span>
        </span>
        <span class="diag-row__value" :class="`diag-row__value--${item.tone}`">处理</span>
        <span class="diag-row__arrow" aria-hidden="true">›</span>
      </button>
    </div>
  </div>

  <div class="diag-card diag-card--tower">
    <h3 class="diag-card__title"><span>系统健康</span></h3>
    <div class="diag-rows diag-rows--grid">
      <button
        v-for="row in health"
        :key="row.key"
        type="button"
        class="diag-row"
        :class="{ 'diag-row--static': !row.action }"
        @click="row.action && onHealth(row)"
      >
        <span class="diag-row__label">{{ row.label }}</span>
        <span class="diag-row__value" :class="row.tone ? `diag-row__value--${row.tone}` : ''">
          {{ row.value }}
        </span>
        <!-- 读不到时明确标出来。静默显示上次的值会让用户以为现在还是好的。 -->
        <span v-if="row.unavailable" class="diag-row__arrow" aria-hidden="true">点击重试</span>
        <span v-else-if="row.action" class="diag-row__arrow" aria-hidden="true">›</span>
      </button>
    </div>
  </div>

  <div class="diag-card diag-card--tower">
    <h3 class="diag-card__title">
      <span>最近操作</span>
      <span class="diag-card__aside">
        启动 / 预检 / 恢复 / 排查
        <!-- 清除入口挂在标题行而不是行内：它是**面向整段历史**的动作，
             不是对某一条记录的操作。放在记录旁边会让人以为点一下只删那一条。 -->
        <button
          v-if="latestRun"
          type="button"
          class="diag-card__action"
          :disabled="isLoading('diagnosticRunsClear')"
          @click="clearDiagnosticRuns"
        >
          {{ isLoading('diagnosticRunsClear') ? '清除中…' : '清除记录' }}
        </button>
      </span>
    </h3>
    <div v-if="latestRun" class="diag-rows">
      <button
        type="button"
        class="diag-row"
        :class="{ 'diag-row--static': !runActionable }"
        @click="runActionable && openDiagnosis()"
      >
        <span class="diag-row__label">
          {{ runKindLabel }} · {{ runMeta.label }}
          <span v-if="runDetail" class="diag-row__value">· {{ runDetail }}</span>
        </span>
        <span v-if="runActionable" class="diag-row__arrow" aria-hidden="true">›</span>
      </button>
    </div>
    <p v-else class="diag-empty">还没有运行记录。启动一次工作台后，这里会显示上次的结果。</p>
  </div>
</template>
