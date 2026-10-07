<script setup>
// 概览页控制塔：三块只读信息 + 诊断入口。
//
// **为什么单独一个组件**：概览页已经 900 行，而这三块与「当前内核」卡片
// 是并列关系而不是它的细节。塞进去只会让那个文件更难读，而它本来就
// 在代码预算的反棘轮上（只许越来越小）。
//
// 三块各自的取舍：
// - **需要关注**：只在真有东西要说时出现。空列表不占位置。
// - **系统健康**：以读数为主，处置在各自面板。唯一的例外是「日志系统」——
//   全应用的日志入口就落在这一行（2026-10-07 从概览主操作排迁来），所以它
//   的右侧提示写「查看」而不是 `›`，自己说得出自己是入口。
// - **最近操作**：回答「上次发生了什么」。失败 / 告警才可点进诊断；
//   成功记录不给按钮——它没什么可诊断的。
import { computed, onMounted, ref } from 'vue';
import { store } from '../store.js';
import { isLoading } from '../shell/loading.js';
import { entryTimeLabel, snapshotStore } from './snapshots.js';
import { skillStore } from '../skills/skills.js';
import { loadLogList, logModal, openLogWindow } from '../logs/logs.js';
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
 *
 * ## 优先级是显式的，不是 push 的顺序（审查 R2-P2-04）
 *
 * 首屏只渲染 `ATTENTION_FIRST_SCREEN` 条，其余收在「查看全部」后面。给定
 * 上限之后，**顺序就成了这件事本身**：过去按代码里的 push 顺序排，于是
 * 「未找到 Node.js」排在最后——一旦事故、设置告警、另一壳工作台三件事同时
 * 出现，最该处理的那条恰好被挤掉，剩下三条都是次要的。
 *
 * rank 越小越先显示：
 *   1 事故      工作台压根没起来，什么都做不了
 *   2 Node      缺运行时，同样起步不了
 *   3 预检      插件装上了但「没验过」，是**用户可能还没意识到**的一类
 *   4 设置      回退到默认值了，不影响启动
 *   5 另一壳    只是提醒：动内核会互相影响
 */
const attentionAll = computed(() => {
  const list = [];
  if (incident.value) {
    list.push({
      key: 'incident',
      rank: 1,
      label: incidentTitle(incident.value),
      detail: incidentCauseLabel(incident.value),
      tone: 'bad',
      action: 'incident',
    });
  }
  if (!nodeOk.value) {
    list.push({
      key: 'node',
      rank: 2,
      label: '未找到满足要求的 Node.js',
      detail: node.value.reason || '启动工作台前需要先准备 Node.js 环境。',
      tone: 'bad',
      action: 'node',
    });
  }
  // 上次预检没完成 / 没通过：这是**用户可能还没意识到**的一类问题——
  // 插件已经装上了，但「没验过」这件事不会自己跳出来提醒。
  const lastPrecheck = diagnosticStore.recentRuns.find((r) => r.kind === 'plugin-precheck');
  if (lastPrecheck && ['failure', 'inconclusive', 'warning'].includes(lastPrecheck.status)) {
    list.push({
      key: 'precheck',
      rank: 3,
      label: `插件预检${statusMeta(lastPrecheck.status).label}`,
      detail: lastPrecheck.summary || '点开看它验到了哪一步',
      tone: lastPrecheck.status === 'warning' ? 'warn' : 'bad',
      action: 'precheck',
      run: lastPrecheck,
    });
  }
  if (store.settingsWarning) {
    list.push({
      key: 'settings',
      rank: 4,
      label: '设置无法读取，已回退到默认值',
      detail: store.settingsWarning,
      tone: 'warn',
      action: 'settings',
    });
  }
  if (kernel.value.other_shell_workbench) {
    list.push({
      key: 'other-shell',
      rank: 5,
      label: '另一个桌面端的工作台正在运行',
      detail: '装 / 删内核与切换端口会影响它。',
      tone: 'warn',
      action: 'other-shell',
    });
  }
  // rank 相同不可能（同 key 只会进一次），但 tie-break 写清楚：顺序不依赖
  // 「sort 稳定」这个实现细节。
  return list.sort((a, b) => a.rank - b.rank || a.key.localeCompare(b.key));
});

/** 首屏条数。设计 §7.2 给的是「最多三条」——再多就把系统健康与最近操作
 *  推到首屏以下，而那两块是用户进这一页最常看的内容。窗口 1040×748 里
 * 右上那一列放三张卡仍然不用滚，这个数因此不随窗口宽度变。 */
const ATTENTION_FIRST_SCREEN = 3;
const attentionExpanded = ref(false);
const attention = computed(() =>
  attentionExpanded.value ? attentionAll.value : attentionAll.value.slice(0, ATTENTION_FIRST_SCREEN)
);
const attentionHidden = computed(() => Math.max(0, attentionAll.value.length - ATTENTION_FIRST_SCREEN));


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
 * **读不到仍是一等状态**：某个数据源读失败时**不能**显示成「正常」——用户点
 * 进来看到一片正常会以为系统没事，而真相是这一项压根没读成功。读不到就明说
 * 读不到（`value` 直接写「读取失败」，右侧提示给「点击重试」）。
 *
 * **右侧提示（`hint`）在这里一次算好**，模板不再分支。三个取值：
 *   读不到 → 「点击重试」；可点的普通格 → `›`；不可点 → 空（不渲染）。
 *
 * **日志那一格例外，给「查看」**：它是全应用唯一的日志入口（2026-10-07 从概览
 * 主操作排迁来），而读数样式（「47 份 ›」）会把唯一的入口藏起来——用户想看日志
 * 时扫过去只读到份数，看不出这里能点开。既然入口只剩这一处，这一格就必须自己
 * 说得出自己是入口。
 *
 * **这一格点开的是独立窗口，不是页内弹层**（2026-10-07 用户要求）。主壳固定
 * 1040×748 且不可缩放，在里面读日志永远只有这么大；独立窗口可缩放、能拖到大
 * 屏、标题栏跟主题。事故 / 预检 / 诊断页那些带**具体某一份证据**的入口仍然
 * 走弹层——那不是「去看看有什么日志」，是「去看这一份」，弹层里能对照着切签。
 * 两种容器各有各的用途，所以这次统一的是**入口唯一**，不是**容器唯一**。
 *
 * `unavailable` 这个字段因此没有了：它的唯一用途就是在模板里挑「点击重试」还是
 * `›`，而这件事现在由 `hint` 一次做完。
 *
 * `bad` 是给卡头那句汇总用的：四格全绿给「全部正常」，否则报异常格数。
 * **只数 `bad` / `warn`**——「尚未读取」「暂无」是中性态，它们说的是「没有可
 * 报的东西」，算进异常会让一张健康的卡写出「1 项异常」。放在这里而不是另立
 * 一个 computed，是因为它与 rows 出自同一次遍历：分开算就得再遍历一次，
 * 而两份数据一旦不同源，汇总句就会和下面的行对不上。
 */
function cell(key, label, value, tone = '', action = null, detail = '') {
  const unavailable = value === '读取失败';
  const hint = unavailable ? '点击重试' : !action ? '' : key === 'logs' ? '查看' : '›';
  return { key, label, value, tone, action, detail, hint, bad: tone === 'bad' || tone === 'warn' };
}

const health = computed(() => [
  cell('wiring', '插件接线', wiringText.value.text, wiringText.value.tone, 'plugins'),
  cell('skills', '技能注册', skillText.value.text, skillText.value.tone, 'skills'),
  // 读失败时把原因挂在 title 上：只显示「读取失败」的话，用户点了重试还是
  // 失败，却不知道是磁盘满了还是文件被轮转掉了。
  cell('logs', '日志系统', logText.value.text, logText.value.tone, logRowAction.value, logModal.listState === 'failed' ? logModal.listError : ''),
  cell('snapshot', '最近快照', latestSnapshotLabel.value || '暂无'),
]);

const healthCaption = computed(() => {
  const bad = health.value.filter((row) => row.bad).length;
  return bad ? { text: `${bad} 项异常`, tone: 'bad' } : { text: '全部正常', tone: 'ok' };
});

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
//
// 但「没有文件」必须先确认过（审查 R2-P2-03）：`logModal.files` 初始为空，
// 而它过去只有 `showLogs()` 之后才被填——于是用户第一次打开概览看到的是
// 「暂无」，哪怕机器上有二十份日志、只是他还没打开过弹层。「暂无」是一个
// **结论**，不能由「没问过」冒充。
const logText = computed(() => {
  if (logModal.listState === 'unloaded') return { text: '尚未读取', tone: '' };
  if (logModal.listState === 'loading') return { text: '读取中…', tone: '' };
  if (logModal.listState === 'failed') return { text: '读取失败', tone: 'bad' };
  if (!logModal.files.length) return { text: '暂无', tone: '' };
  return { text: `${logModal.files.length} 份`, tone: 'ok' };
});

/** 清单没读到时点这一行就是重试——静默加载失败后，页面不该停在「读取失败」
 *  这三个字上不给一条出路。 */
function retryLogList() {
  return loadLogList();
}

/**
 * 日志那一格**有没有可点的动作**，与它显示什么读数分开。
 *
 * 两种情况可点：读失败了（点了是重试），或者确实读到了至少一份（点了开独立
 * 日志窗口）。另外两种不可点——还没问过、正在问、以及确认过一份都没有。
 * 最后一种尤其要挡住：一份日志都没有时开出来的是一个空窗口（左侧空列表 +
 * 「请从左侧选择日志文件」），点了只会让人以为窗口坏了。「暂无」这一格
 * 本来就在说「这里没有东西可看」，让它不可点比让它开空窗诚实。
 */
const logRowAction = computed(() => {
  if (logModal.listState === 'failed') return 'logs';
  if (logModal.listState === 'ready' && logModal.files.length) return 'logs';
  return null;
});

onMounted(() => {
  // 一次静默清单读取。这是控制塔**唯一**的额外请求，而且换来的是「系统健康」
  // 那一行能说真话；概览其余判据仍然只读 store 已有状态，不做探测。
  if (logModal.listState === 'unloaded') loadLogList();
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
  else if (row.action === 'logs') {
    // 清单读取失败时这一行是「点击重试」，那就**只重试清单**：用户要的是把
    // 这一行修好，不是被拉进一个窗口。成功之后它变回普通行，再点才开窗。
    if (logModal.listState === 'failed') return retryLogList();
    // 不传文件名：这一格手里只有份数，没有「当前签」。空名让窗口自己挑
    // （先 kernel 分组，再任意第一份），比在这里替它猜一份更可靠——
    // 概览页显示的是全部日志的集合，未必就是用户此刻想读的那一类。
    return openLogWindow();
  }
}

// 按记录类型分派，不再一律当启动诊断打开（审查 P1-05）。

function openDiagnosis() {
  openRunDiagnosis(latestRun.value, 'overview');
  loadRecentRuns(null);
}
</script>

<template>
  <div v-if="attentionAll.length" class="diag-card diag-card--tower diag-card--attention">
    <h3 class="diag-card__title">
      <span>需要关注</span>
      <span class="diag-card__aside">{{ attentionAll.length }} 项</span>
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
      <!-- 首屏上限之外的项不静默丢弃，也不塞进首屏。收起时说清还剩几项，
           免得用户以为「只有 3 个问题」而漏掉真正要看的那一条。 -->
      <button
        v-if="attentionHidden > 0 || attentionExpanded"
        type="button"
        class="diag-row diag-row--more"
        @click="attentionExpanded = !attentionExpanded"
      >
        <span class="diag-row__label">
          {{ attentionExpanded ? '收起' : `还有 ${attentionHidden} 项` }}
        </span>
        <span class="diag-row__arrow" aria-hidden="true">
          {{ attentionExpanded ? '⌃' : '⌄' }}
        </span>
      </button>
    </div>
  </div>

  <div class="diag-card diag-card--tower diag-card--health">
    <h3 class="diag-card__title">
      <span>系统健康</span>
      <!-- 设计稿 `card-caption health-ok`：一句话结论，别让用户逐行读四遍。 -->
      <span class="diag-card__aside" :class="{ 'diag-card__aside--bad': healthCaption.tone === 'bad' }">{{ healthCaption.text }}</span>
    </h3>
    <div class="diag-rows diag-rows--grid">
      <button
        v-for="row in health"
        :key="row.key"
        type="button"
        class="diag-row"
        :class="{ 'diag-row--static': !row.action }"
        :title="row.detail || ''"
        @click="row.action && onHealth(row)"
      >
        <span class="diag-row__label">{{ row.label }}</span>
        <span class="diag-row__value" :class="row.tone ? `diag-row__value--${row.tone}` : ''">
          {{ row.value }}
        </span>
        <!-- 右侧提示由 cell() 一次算好（读不到说「点击重试」，日志格说「查看」）。 -->
        <span v-if="row.hint" class="diag-row__arrow" aria-hidden="true">{{ row.hint }}</span>
      </button>
    </div>
  </div>

  <div class="diag-card diag-card--tower diag-card--activity">
    <!-- 空态：说明收进标题行右侧，`--bare` 去掉标题下的线与留白（依据见 diagnostics.css）。 -->
    <h3 class="diag-card__title" :class="{ 'diag-card__title--bare': !latestRun }">
      <span>最近操作</span>
      <span v-if="latestRun" class="diag-card__aside">
        启动 / 预检 / 恢复 / 排查
        <!-- 清除入口挂在标题行而不是行内：它是**面向整段历史**的动作，
             不是对某一条记录的操作。放在记录旁边会让人以为点一下只删那一条。 -->
        <button
          type="button"
          class="diag-card__action"
          :disabled="isLoading('diagnosticRunsClear')"
          @click="clearDiagnosticRuns"
        >
          {{ isLoading('diagnosticRunsClear') ? '清除中…' : '清除记录' }}
        </button>
      </span>
      <span v-else class="diag-card__aside">还没有运行记录 · 启动工作台后显示上次结果</span>
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
  </div>
</template>
