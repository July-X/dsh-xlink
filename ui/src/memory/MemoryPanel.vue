<script setup>
// 记忆面板：保存 / 编辑 / 停用 / 删除本机的长期偏好与项目约定，
// 并在右边把「按当前输入会注入什么」摊开给用户看。
//
// 数据与动作全在 `memory.js` 里；这里只管画。字段名用后端下发的 snake_case
// （见该文件顶部的说明）。
import PageHeadIcon from '../shell/PageHeadIcon.vue';
import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
import { mcode, refreshMcodeStatus } from '../kernel/mcode.js';
import { deleteMcodeMemory, mcodeMemory, refreshMcodeMemory, resetMcodeMemory, setMcodeMemoryEnabled, setMcodeMemoryState } from '../kernel/mcodeMemory.js';
import { MCODE_BUILD_AVAILABLE, mcodeExperience } from '../kernel/experience.js';
import { isShared, loadAllMemoryShares, memoryShares, memoryTargetUse, refreshMemoryShares, refreshMemoryTargetUse, resetMemoryShares, resetMemoryTargetUse, revokeAnyMemoryShare, revokeMemoryShare, setMemoryShare, setMemoryTargetUse, shareNeedsReview, targetUseText, targetUseView } from './share.js';
import { confirmDialog } from '../shell/notify.js';
import {
  ArrowDown,
  Collection,
  Download,
  InfoFilled,
  MagicStick,
  Plus,
  Promotion,
  Refresh,
  Search,
  WarningFilled,
} from '@element-plus/icons-vue';
import MemoryRow from './MemoryRow.vue';
import MemoryModulePanel from './MemoryModulePanel.vue';
import { memoryModule, refreshMemoryModule } from './module.js';
import MemoryCandidateRow from './MemoryCandidateRow.vue';
import MemoryEditor from './MemoryEditor.vue';
import MemoryDetail from './MemoryDetail.vue';
import MemoryHandoffDetail from './MemoryHandoffDetail.vue';
import MemoryCandidateDetail from './MemoryCandidateDetail.vue';
import { baseName } from '../shell/labels.js';
import { globalBusy, isLoading } from '../shell/loading.js';
import { builtinById, loadBuiltinStatus, toggleBuiltin } from '../plugins/builtin.js';
import {
  memoryStore,
  refreshMemory,
  removeMemory,
  setMemoryState,
  setMemoryEnabled,
  clearMemory,
  previewContext,
  exportMemory,
  openExports,
  openMemoryEditor,
  injectionDisabled,
  injectionText,
  visibleMemories,
  shortTime,
  setListScope,
  jobsSummaryText,
  jobLine,
  switchEffectText,
} from './memory.js';
import MemoryEffectPanel from './MemoryEffectPanel.vue';
import { loadEffectReport } from './effect.js';

const sourceFilter = ref('all');
// 不包含 MiniMax 操作入口的构建仍可从模块的只读视图保留历史来源。
const mcodeRecords = computed(() => MCODE_BUILD_AVAILABLE ? mcodeMemory.memories
  : memoryModule.records.filter(item => item.origin_label === 'MiniMax').map(item => item.record));
const regularScope = computed(() => ['all', 'project', 'general'].includes(memoryStore.listScope));
const list = computed(() => regularScope.value && sourceFilter.value === 'mcode' ? [] : visibleMemories(memoryStore.view, memoryStore.listScope));
const mcodeList = computed(() => regularScope.value && sourceFilter.value !== 'dsh'
  ? visibleMemories({ memories: mcodeRecords.value }, memoryStore.listScope) : []);
const sourceFilterText = computed(() => ({ all: '全部来源', dsh: 'DSH 来源', mcode: 'MiniMax 来源' })[sourceFilter.value]);
const canShare = computed(() => MCODE_BUILD_AVAILABLE && !!mcode.view?.instance_id && memoryShares.loaded && !memoryShares.loading && !memoryShares.error);

// 页顶「允许使用记忆」：按接收目标分。与下方的通用模块授权、逐条授权是三件事——
// 这里只写目标开关，授权面板的各开关也只写各自的命令，互不代改。
const dshTargetUse = computed(() => targetUseView('dsh'));
const mcodeTargetUse = computed(() => targetUseView('mcode'));
const targetUseBusy = computed(() => !!memoryTargetUse.changing);
const mcodeConfigured = computed(() => !!mcode.view?.instance_id);
const mcodeTargetUseText = computed(() =>
  memoryTargetUse.error ? '状态未知' : !mcodeConfigured.value && !mcodeTargetUse.value?.warning ? '未配置实例' : targetUseText(mcodeTargetUse.value)
);
const TARGET_USE_TIP = '每个开关只决定该内核还能不能用记忆：关掉后它的自动检索和注入都收不到记忆，但记录不会删除，已有授权也不会撤销，另一个内核照常使用。开启本身不创建跨内核授权。没选过时显示「跟随原设置」，沿用旧的实例开关与通用记忆模块授权。';
const MCODE_LEGACY_TIP = '旧实例开关不控制已授权的普通记忆互通；要停止 MiniMax 接收记忆，请关闭顶部的目标开关。';
// 来源实例 / profile 不常驻：目标名的 title 里给（弹窗里也仍有完整坐标）。
const sourceNamespace = (label) => memoryModule.status?.sources?.find(source => source.label === label)?.namespace || '';

/// 旧的实例开关会改变「跟随原设置」的效值（旧语义 = 旧开关或模块授权），改完重读一次
/// 目标状态，免得同一屏上方的开关已经关了、下面那枚还显示开。
function changeMemoryEnabled(enabled) {
  return setMemoryEnabled(enabled).then(() => refreshMemoryTargetUse());
}
function changeMcodeMemoryEnabled(enabled) {
  return setMcodeMemoryEnabled(enabled).then(() => refreshMemoryTargetUse());
}
function refreshMemoryPage() {
  refreshMemory();
  refreshMemoryModule();
  if (!memoryTargetUse.changing) refreshMemoryTargetUse();
  if (MCODE_BUILD_AVAILABLE) {
    refreshMcodeStatus();
    if (mcode.view?.instance_id && mcode.view?.memory_profile) {
      refreshMcodeMemory();
      refreshMemoryShares();
    }
  }
}
const mcodeDetailId = ref(null);
const mcodeDetail = computed(() => mcodeRecords.value.find(row => row.id === mcodeDetailId.value) || null);
const mcodeMemoryRuntime = computed(() => {
  if (mcode.error) return '工作台运行状态读取失败，请刷新重试。';
  if (mcode.view?.running === false) return '工作台未运行；记忆接线状态需在启动后确认。';
  if (mcode.view?.running !== true) return '工作台运行状态未知。';
  switch (mcode.view.memory_state) {
    case 'pending_verification': return '记忆桥已启动，真实对话注入尚待验证；修改记忆空间后需要重启。';
    case 'disabled': return '当前工作台未接入记忆；启用后需要重启工作台。';
    case 'failed': return '当前工作台的记忆接线失败，请查看错误并重启。';
    case 'platform_unavailable': return '当前平台暂不支持记忆接线。';
    default: return '当前工作台的记忆接线状态不可用。';
  }
});
const mcodeRuntimeShort = computed(() => {
  if (mcode.error) return '状态未知';
  if (mcode.view?.running === false) return '未运行';
  if (mcode.view?.running !== true) return '状态未知';
  return ({ pending_verification: '待验证', disabled: '未接入', failed: '接线失败', platform_unavailable: '暂不支持' })[mcode.view.memory_state] || '状态未知';
});
const sharedDetailKey = ref('');
const sharesVisible = ref(false);
const sharedDetail = computed(() => memoryShares.items.find(item => item.record && `${item.source_instance}/${item.source_profile}/${item.record_id}` === sharedDetailKey.value) || null);

function openShareManagement() {
  sharesVisible.value = true;
  loadAllMemoryShares();
  if (MCODE_BUILD_AVAILABLE && mcode.view?.instance_id && mcode.view?.memory_profile) refreshMemoryShares();
}
const otherMemoryShares = computed(() => memoryShares.loaded
  ? memoryShares.all.filter(item => item.target_instance !== memoryShares.target?.instanceId || item.target_profile !== memoryShares.target?.profile)
  : memoryShares.all);
async function revokeCurrentMemoryShare(item) {
  await revokeMemoryShare(item);
  await loadAllMemoryShares();
}

function openMcodeEditor(row = null) {
  const { instanceId, profile } = mcodeMemory.target || {};
  if (!mcodeExperience.value || !instanceId || !profile || mcode.view?.instance_id !== instanceId || mcode.view?.memory_profile !== profile || mcodeMemory.loading || !mcodeMemory.status || mcodeMemory.status.warning) return;
  mcodeDetailId.value = null;
  openMemoryEditor(row, 'memory', { instanceId, profile });
}

async function removeMcodeMemory(row) {
  const { instanceId, profile } = mcodeMemory.target || {};
  const confirmed = await confirmDialog('删除这条记忆？', '这会删除当前 MiniMax 实例中的记录。', '删除');
  if (confirmed && mcodeExperience.value && instanceId && profile && mcode.view?.instance_id === instanceId && mcode.view?.memory_profile === profile) {
    await deleteMcodeMemory(row.id);
  }
}
/// **未筛选**的库：导出、清空、空态判据都用它——那些动作作用在整份文档上，
/// 与「当前视图看哪一档」无关。原先这三处读的是筛选后的 `list`，于是库里只有通用偏好时
/// 点「本项目」，卡片会说「还没有记忆」而导出与清空同时变灰点不动（评审第 2 条）。
const allMemories = computed(() => (memoryStore.view && memoryStore.view.memories) || []);
const totalMemories = computed(() => (sourceFilter.value === 'mcode' ? 0 : allMemories.value.length) + (sourceFilter.value === 'dsh' ? 0 : mcodeRecords.value.length));
const moduleStatusText = computed(() => {
  if (memoryModule.error || memoryModule.status?.warning) return memoryModule.status?.pair_key ? '需重新授权' : '状态未知';
  if (!memoryModule.status) return '读取中…';
  return memoryModule.status.enabled ? '已授权' : '未授权';
});
const allRecords = computed(() => [...allMemories.value, ...visibleMemories(memoryStore.view, 'handoff'), ...visibleMemories(memoryStore.view, 'experience')]);
const artifactKind = computed(() => ['handoff', 'experience'].includes(memoryStore.listScope) ? memoryStore.listScope : '');
const artifactName = computed(() => artifactKind.value === 'handoff' ? '交接' : '经验');
const candidates = computed(() => (memoryStore.view && memoryStore.view.candidates) || []);
const status = computed(() => memoryStore.status);
const recentInjections = computed(() => memoryStore.recentInjections || []);
/// 提取任务卡片：摘要那句 + 最近几条（后端已经截到 5 条，前端不再截）。
const jobsText = computed(() => jobsSummaryText(memoryStore.jobs));
const recentJobs = computed(() => (memoryStore.jobs && memoryStore.jobs.jobs) || []);
const enabled = computed(() => !!(status.value && status.value.use_approved));

/// 记忆库与待确认共用列表布局，effect 是只读的记忆效果页（合同 §3.2）。
const activeTab = ref(memoryStore.listScope === 'candidates' ? 'candidates' : 'library');

/** 切页签。效果页首次进入 / 时间窗变化时才真的拉数据（loadEffectReport 自带缓存），
 *  来回切不会反复重扫会话日志；手动刷新走面板里的「刷新」。 */
function selectTab(tab) {
  if (!['library', 'candidates', 'effect'].includes(tab)) return;
  if (tab === 'candidates') setListScope('candidates');
  else if (memoryStore.listScope === 'candidates') setListScope('all');
  activeTab.value = tab;
  if (tab === 'effect') loadEffectReport();
}

/// 「一次最多注入几条」的那半句。**数来自后端**（`MemoryStatus.max_hits` ←
/// `query.rs::MAX_HITS`），前端不抄：抄一个数就会在它变的时候静默说谎，而没有任何判据
/// 会因此变红——同一个形状刚在插件的注入预算上发生过一次（3 → 8）。状态还没读回来时
/// 只说「条数有上限」，不猜一个数。
const injectLimitText = computed(() =>
  status.value && status.value.max_hits ? `最多 ${status.value.max_hits} 条` : '条数有上限'
);

/// 页头说明（2026-11-01 用户要求：顶部的说明性文字都收进 tooltip，不再铺成两行
/// 小字）。内容随页签切换，和原先两段 `.page-desc` 逐字一致；挂到标题「记忆」旁的
/// ⓘ 上，界面只留标题。
const pageIntro = computed(() =>
  activeTab.value !== 'effect'
    ? `统一管理不同来源的记录，查看不等于授权模型读取，记录仍保存在各自实例。DSH 工作台按当前对话挑选相关记忆（${injectLimitText.value}），固定项优先带上；没被挑中的，模型可以自己按关键词查。模型只能读取与提出候选；自动提取的长期约定会直接存成记忆（未验证），只有与既有记忆冲突或推翻旧候选的才进「待确认」由你裁决。自动提取由 DSH 原有开关控制。`
    : '下面的读数全部来自本机会话日志的只读扫描与记忆库聚合：记忆有没有被注入、被主动读取，成本多大，这些数可不可信。不采集任何正文。'
);

/// 卡头标题里的计数：`memory_list` 是带 `include_disabled` 读回来的，所以总数**含停用条目**
/// ——不写出来的话「记忆（12）」会被读成「启用了 12 条」，而右栏那行才是分开数的。
/// 一条都没有停用时就不写这半句（常态下少四个字）。
const disabledCount = computed(() => (sourceFilter.value === 'mcode' ? 0 : status.value?.disabled || 0) + (sourceFilter.value === 'dsh' ? 0 : mcodeRecords.value.filter(row => row.state === 'disabled').length));

/// 注入链路的内置插件（`xlink-memory`）。**按 id 取，不取第一项**：
/// 取第一项会在登记表里多一个插件时静默指向别人。
const injectPlugin = computed(() => builtinById('memory'));

/// 打开注入插件。成功后回读一次状态（后端返回的那一项已经是真值，回读是为了
/// 与插件页共享的列表保持一致——两处各显示一份状态就会分叉）。
function enableInjection() {
  return toggleBuiltin('memory', true).then(() => loadBuiltinStatus());
}

/// 原有实例开关还控制 DSH 自动提取；跨来源授权另算。
function switchEffectDetail(status) {
  return '此旧开关控制 DSH 原有记忆注入和自动提取，不撤销跨内核授权；停止 DSH 接收请使用顶部目标开关。' + switchEffectText(status);
}

/// 详情弹窗记的是**行的 id**，不是那个行对象。`setMemoryState` / `removeMemory` 之后都会
/// 回读一次视图，攥着旧对象会把停用前的状态一直画在弹窗里；按住 id 重算，读到的就是刷新后
/// 的那一份，而记忆被删掉时它自己变成 `null`、弹窗随之关掉。
const detailId = ref(null);

/// 从**未筛选**的 `allMemories` 里取，不从 `list` 取：读着一条记忆时切「本项目 / 通用偏好」
/// 那个筛选，它会把当前这条从 `list` 里筛出去，弹窗跟着凭空消失。
const detailRow = computed(() => allRecords.value.find((row) => row.id === detailId.value) || null);

/// 列表行现在整块可点（卡片压到两行、正文截断），点开才是完整正文与操作。
const openMemoryDetail = (row) => { detailId.value = row.id; };

/// 候选详情与记忆详情同一个形状：记 **id**，弹窗自己从 `candidates` 里现取。
/// 那个列表是每次 `memory_list` 回读的新数组——候选在弹窗里被保存 / 忽略之后，后端
/// 回读会把它从 `pending_candidates` 里摘掉，这个 computed 随之变 `null`、弹窗自动关，
/// 不必再手动同步一次「关没关」。
const candidateDetailId = ref(null);
const candidateDetailItem = computed(
  () => candidates.value.find((item) => item.id === candidateDetailId.value) || null
);

/// 候选行整块可点（正文截成两行），点开在这里读完整正文与来源，并直接裁决。
const openCandidateDetail = (item) => { candidateDetailId.value = item.id; };

/// 从详情转去编辑器：两个弹窗都 `append-to-body`，叠着开就是两层遮罩，先关详情再开编辑器。
function editFromDetail(row) {
  detailId.value = null;
  openMemoryEditor(row);
}

watch(() => [mcode.view?.instance_id, mcode.view?.memory_profile], ([id, profile], previous) => {
  if (previous && id === previous[0] && profile === previous[1]) return;
  resetMemoryShares();
  resetMcodeMemory();
  mcodeDetailId.value = null;
  sharedDetailKey.value = '';
  if (previous) refreshMemoryTargetUse();
  if (!id || !profile) sourceFilter.value = 'all';
  if (!MCODE_BUILD_AVAILABLE || !id || !profile) return;
  refreshMemoryShares();
  refreshMcodeMemory();
}, { immediate: true });
onMounted(() => {
  refreshMemory({ silent: true });
  loadBuiltinStatus();
  refreshMemoryTargetUse();
  if (MCODE_BUILD_AVAILABLE) refreshMcodeStatus();
});
onUnmounted(() => { resetMcodeMemory(); resetMemoryShares(); resetMemoryTargetUse(); });
</script>

<template>
  <section class="panel">
    <div class="page-head">
      <div class="page-head__text"><PageHeadIcon :page="'memory'" />
        <div class="page-title-row">
          <h1 class="page-title">记忆</h1>
          <el-tooltip placement="bottom-start" :show-after="80" :content="pageIntro">
            <el-icon class="head-tip-icon" tabindex="0" :aria-label="pageIntro"><InfoFilled /></el-icon>
          </el-tooltip>
        </div>
      </div>
      <div class="memory-create">
        <el-dropdown v-if="mcodeExperience" trigger="click" :disabled="globalBusy" @command="target => target === 'mcode' ? openMcodeEditor() : openMemoryEditor()">
          <el-button type="primary" :icon="Plus" :disabled="globalBusy">新建记忆</el-button>
          <template #dropdown>
            <el-dropdown-menu>
              <el-dropdown-item command="dsh">DSH</el-dropdown-item>
              <el-dropdown-item command="mcode" :disabled="mcodeMemory.loading || !mcodeMemory.status || !!mcodeMemory.status.warning || !mcodeConfigured"><span class="minimax-wordmark">MiniMax</span></el-dropdown-item>
            </el-dropdown-menu>
          </template>
        </el-dropdown>
        <el-button v-else type="primary" :icon="Plus" :disabled="globalBusy" @click="openMemoryEditor()">新建记忆</el-button>
      </div>
    </div>
    <MemoryModulePanel @manage-shares="openShareManagement">
      <template #usage>
        <span class="memory-target">
          <span class="memory-target__name" :title="sourceNamespace('DSH') || memoryStore.status?.namespace">DSH</span>
          <span class="memory-target__state">{{ memoryTargetUse.error ? '状态未知' : targetUseText(dshTargetUse) }}</span>
          <el-switch v-if="!dshTargetUse?.warning" :model-value="!!dshTargetUse?.effective" size="small"
            :loading="memoryTargetUse.changing === 'dsh'" :disabled="globalBusy || targetUseBusy || !!memoryTargetUse.error || !dshTargetUse"
            aria-label="允许 DSH 使用记忆" @change="(enabled) => setMemoryTargetUse('dsh', enabled)" />
          <el-tooltip v-else placement="top-start" :show-after="80" :content="dshTargetUse.warning">
            <el-icon class="head-tip-icon head-tip-icon--warning" tabindex="0" :aria-label="dshTargetUse.warning"><WarningFilled /></el-icon>
          </el-tooltip>
        </span>
        <span v-if="mcodeExperience" class="memory-target">
          <span class="memory-target__name minimax-wordmark" :title="sourceNamespace('MiniMax')">MiniMax</span>
          <span class="memory-target__state">{{ mcodeTargetUseText }}</span>
          <el-switch v-if="!mcodeTargetUse?.warning" :model-value="!!mcodeTargetUse?.effective" size="small"
            :loading="memoryTargetUse.changing === 'mcode'" :disabled="globalBusy || targetUseBusy || !!memoryTargetUse.error || !mcodeTargetUse || !mcodeConfigured"
            aria-label="允许 MiniMax 使用记忆" @change="(enabled) => setMemoryTargetUse('mcode', enabled)" />
          <el-tooltip v-else placement="top-start" :show-after="80" :content="mcodeTargetUse.warning">
            <el-icon class="head-tip-icon head-tip-icon--warning" tabindex="0" :aria-label="mcodeTargetUse.warning"><WarningFilled /></el-icon>
          </el-tooltip>
        </span>
        <el-tooltip placement="top-start" :show-after="80" :content="TARGET_USE_TIP">
          <el-icon class="head-tip-icon" tabindex="0" :aria-label="TARGET_USE_TIP"><InfoFilled /></el-icon>
        </el-tooltip>
      </template>
      <template #legacy>
        <div class="memory-legacy-setting">
          <span>DSH 原有记忆与自动提取</span>
          <el-switch :model-value="enabled" :loading="isLoading('memoryEnabled')" :disabled="globalBusy || !status || !!status.warning"
            aria-label="DSH 原有记忆与自动提取" @change="changeMemoryEnabled" />
          <el-tooltip placement="top-start" :show-after="80" :content="switchEffectDetail(status)">
            <el-icon class="card-info-icon" tabindex="0" :aria-label="switchEffectDetail(status)"><InfoFilled /></el-icon>
          </el-tooltip>
        </div>
        <div v-if="mcodeExperience" class="memory-legacy-setting">
          <span><span class="minimax-wordmark">MiniMax</span> 原有实例接入</span>
          <el-switch :model-value="!!mcodeMemory.status?.use_approved" :loading="mcodeMemory.active === 'mcodeMemoryEnabled'" :disabled="globalBusy || mcodeMemory.loading || !mcodeMemory.status || !!mcodeMemory.status.warning"
            aria-label="MiniMax 原有实例记忆接入" @change="changeMcodeMemoryEnabled" />
          <el-tooltip placement="top-start" :show-after="80" :content="MCODE_LEGACY_TIP + ' ' + mcodeMemoryRuntime">
            <el-icon class="card-info-icon" tabindex="0" :aria-label="MCODE_LEGACY_TIP + ' ' + mcodeMemoryRuntime"><InfoFilled /></el-icon>
          </el-tooltip>
        </div>
      </template>
    </MemoryModulePanel>
    <p v-if="memoryTargetUse.error" class="memory-hint">{{ memoryTargetUse.error }}</p>
    <el-alert v-if="memoryStore.listError" type="error" :closable="false" :title="memoryStore.listError" />
    <el-alert v-if="mcodeMemory.error || mcodeMemory.status?.warning || mcode.view?.memory_error || memoryShares.error"
      type="error" :closable="false" :title="mcodeMemory.error || mcodeMemory.status?.warning || mcode.view?.memory_error || memoryShares.error" />

    <div class="memory-tabs" role="tablist" aria-label="记忆页签">
      <button
        type="button"
        class="plugin-tab"
        role="tab"
        :class="{ 'is-active': activeTab === 'library' }"
        :aria-selected="activeTab === 'library'"
        @click="selectTab('library')"
      >
        记忆库
      </button>
      <button
        type="button"
        class="plugin-tab"
        role="tab"
        :class="{ 'is-active': activeTab === 'candidates' }"
        :aria-selected="activeTab === 'candidates'"
        @click="selectTab('candidates')"
      >
        待确认<span v-if="candidates.length">（{{ candidates.length }}）</span>
      </button>
      <button
        type="button"
        class="plugin-tab"
        role="tab"
        :class="{ 'is-active': activeTab === 'effect' }"
        :aria-selected="activeTab === 'effect'"
        @click="selectTab('effect')"
      >
        效果
      </button>
    </div>

    <!-- 记忆库与候选共用同一布局；效果页使用独立的只读统计。 -->
    <template v-if="activeTab !== 'effect'">

    <el-alert
      v-if="status && status.warning"
      :title="status.warning"
      type="warning"
      :closable="false"
      show-icon
    />

    <!--
      开关打开但注入插件**确定**没启用时，记忆一条也不会进会话——这是两个开关，
      而第二个在插件页。不说的话，用户会以为「开了却没生效」是记忆坏了。
      「确定」由 `injectionDisabled` 判：状态还没读回来时它回 false，不挂假警报。
      启用意图由后端判（内核跑着时它会拒绝并给出原因，前端不预判）。
    -->
    <el-alert
      v-if="enabled && injectionDisabled(injectPlugin)"
      type="warning"
      :closable="false"
      show-icon
    >
      <template #title>
        DSH 的记忆注入插件还没启用：记忆不会通过该插件进入 DSH 会话。
        <el-button
          text
          type="primary"
          :loading="isLoading('builtinToggle:memory')"
          :disabled="globalBusy"
          @click="enableInjection"
        >
          启用「本地记忆」
        </el-button>
      </template>
    </el-alert>

    <div class="page-layout">
      <div class="page-layout__col">
        <div class="card memory-library">
          <div class="card-head">
            <h2>
              <span class="card-title-icon" aria-hidden="true"><el-icon><Collection /></el-icon></span>
              {{
                artifactKind ? `${artifactName}（${list.length}）` : memoryStore.listScope === 'candidates'
                  ? `待确认（${candidates.length}）`
                  : memoryStore.listScope === 'all'
                    ? disabledCount
                      ? `记忆（${totalMemories}，含停用 ${disabledCount}）`
                      : `记忆（${totalMemories}）`
                    : `记忆（${list.length + mcodeList.length} / 共 ${totalMemories}）`
              }}
              <span v-if="artifactKind || memoryStore.listScope === 'candidates'" class="origin-chip origin-chip-kernel">DSH</span>
              <!--
                候选那一档的常驻说明收进 ⓘ：它只在切到「待确认」时需要，常驻四行会把
                卡头撑成两行。判据要的那句「先确认正文里没有路径、客户名」仍在这里，
                且**不折行**（assert.match）。
              -->
              <el-tooltip
                v-if="memoryStore.listScope === 'candidates'"
                placement="bottom-start"
                :show-after="80"
              >
                <template #content>
                  <!-- prettier-ignore -->
                  <div class="card-info-tooltip">
                    待确认里是模型在会话里主动提出的候选，以及自动提取里与既有记忆冲突、或推翻旧候选、需要你裁决的条目：正文与来源都摆在这里，只有你点保存才会成为记忆。<br />
                    要存成「所有项目都算」的通用偏好时用右边那个按钮——先确认正文里没有路径、客户名之类的私有信息，通用偏好会跟着你去别的项目。
                  </div>
                </template>
                <el-icon class="card-info-icon" tabindex="0" aria-label="待确认候选怎么处理">
                  <InfoFilled />
                </el-icon>
              </el-tooltip>
            </h2>
            <el-dropdown v-if="regularScope && (mcodeConfigured || mcodeRecords.length)" trigger="click" @command="sourceFilter = $event">
              <el-button text>
                <template v-if="sourceFilter === 'mcode'"><span class="minimax-wordmark">MiniMax</span> 来源</template>
                <template v-else>{{ sourceFilterText }}</template>
                <el-icon><ArrowDown /></el-icon>
              </el-button>
              <template #dropdown>
                <el-dropdown-menu>
                  <el-dropdown-item command="all">全部来源</el-dropdown-item>
                  <el-dropdown-item command="dsh">DSH 来源</el-dropdown-item>
                  <el-dropdown-item command="mcode"><span class="minimax-wordmark">MiniMax</span> 来源</el-dropdown-item>
                </el-dropdown-menu>
              </template>
            </el-dropdown>
            <el-button v-if="artifactKind" type="primary" :icon="Plus" :disabled="globalBusy" @click="openMemoryEditor(null, artifactKind)">新建{{ artifactName }}</el-button>
            <div class="memory-head__actions">
              <el-button
                text
                :icon="Download"
                :loading="isLoading('memoryExport')"
                :disabled="globalBusy || (!allRecords.length && !candidates.length)"
                @click="exportMemory"
              >
                导出 DSH
              </el-button>
              <el-button
                text
                type="danger"
                :loading="isLoading('memoryClear')"
                :disabled="globalBusy || (!allRecords.length && !candidates.length)"
                @click="clearMemory"
              >
                清空 DSH
              </el-button>
              <el-button
                text
                :icon="Refresh"
                :loading="isLoading('memoryList') || mcodeMemory.loading || memoryModule.busy"
                :disabled="globalBusy"
                @click="refreshMemoryPage"
              >
                刷新
              </el-button>
            </div>
          </div>
          <!--
            筛选切的是下面这份清单的视图，此前它挤在卡头那一排：卡内可用宽 340px，
            三档单选约 234px 加三枚文字按钮约 303px，于是「通用偏好」折到第二行、
            按钮再折到第三行，三样东西各自换行。挪出来单独占一行后一行放得下（余量
            约 100px）；卡头只剩标题与按钮两栏，放不下时按钮折到第二行、靠右对齐。
          -->
          <el-radio-group
            v-if="activeTab === 'library'"
            :model-value="memoryStore.listScope"
            size="small"
            :disabled="globalBusy"
            @change="setListScope"
          >
            <el-radio value="all">全部</el-radio>
            <!-- 「按项目」而不是「本项目」：这一档列的是**所有**项目范围的记忆，
                 不是「当前这个项目」——写「本项目」会让人以为它按当前目录过滤
                 （用户 2026-10-17 第二问）。 -->
            <el-radio value="project">按项目</el-radio>
            <el-radio value="general">通用偏好</el-radio>
            <el-radio value="handoff">交接</el-radio>
            <el-radio value="experience">经验</el-radio>
          </el-radio-group>
          <p v-if="memoryStore.lastExport" class="memory-hint">
            最近导出 DSH：{{ memoryStore.lastExport.path }}（{{ memoryStore.lastExport.memories }} 条记忆、{{
              memoryStore.lastExport.candidates
            }}
            条候选、{{ memoryStore.lastExport.handoffs || 0 }} 条交接、{{ memoryStore.lastExport.experiences || 0 }} 条经验；一次性快照，不参与检索）
            <el-button text type="primary" @click="openExports">打开导出目录</el-button>
          </p>
          <!--
            候选那一档：行还是原来那张卡里的行（正文、来源、冲突 / 取代标记、三枚动作
            全在），只是换了个容器。列表容器与已保存记忆共用 `memory-list`——这一屏
            唯一会滚的就是它，滚动条不再出现在页面上。
            行整块可点（2026-10-19）：正文截成两行，点开是 `MemoryCandidateDetail`，
            在那里读完整正文与来源并直接裁决。
          -->
          <p v-if="memoryStore.listScope === 'candidates' && !memoryStore.view" class="memory-hint">
            {{ isLoading('memoryList') ? '正在读取 DSH 待确认…' : 'DSH 待确认暂时无法读取，请刷新重试。' }}
          </p>
          <p v-else-if="memoryStore.listScope === 'candidates' && !candidates.length" class="memory-hint">
            没有待确认的候选。模型在会话里主动提出候选、或自动提取的结果需要你裁决时会出现在这里，只有点「保存」才成为记忆。
          </p>
          <div
            v-else-if="memoryStore.listScope === 'candidates'"
            class="page-list memory-list"
          >
            <MemoryCandidateRow
              v-for="item in candidates"
              :key="item.id"
              :item="item"
              :records="allMemories"
              :candidates="candidates"
              @open="openCandidateDetail"
            />
          </div>
          <p v-else-if="artifactKind && !memoryStore.view" class="memory-hint">
            {{ isLoading('memoryList') ? `正在读取 DSH ${artifactName}…` : `DSH ${artifactName}暂时无法读取，请刷新重试。` }}
          </p>
          <p v-else-if="artifactKind && !list.length" class="memory-hint">还没有{{ artifactName }}。点上面的「新建{{ artifactName }}」创建；经验也可由模型提出后在待确认中审核。</p>
          <p v-else-if="!artifactKind && !totalMemories && (isLoading('memoryList') || mcodeMemory.loading)" class="memory-hint">正在读取记忆…</p>
          <p v-else-if="!artifactKind && !memoryStore.view && (sourceFilter === 'dsh' || (!mcodeMemory.target && !mcodeRecords.length))" class="memory-hint">记忆暂时无法读取，请刷新重试。</p>
          <p v-else-if="!artifactKind && !totalMemories" class="memory-hint">
            {{ sourceFilter === 'all' ? '还没有记忆。点上面的「新建记忆」写一条，或者在工作台里让模型提出候选，再切到「待确认」里点保存。' : '这个来源还没有记忆。' }}
          </p>
          <p v-else-if="!list.length && !mcodeList.length" class="memory-hint">
            当前筛选下没有记忆（此来源共有 {{ totalMemories }} 条，切到「全部」能看到）。
          </p>
          <!-- `memory-list` 是这一屏唯一的滚动容器，高度由卡片剩余空间给（2026-10-17 起
               整页固定在窗口高度内，页面级滚动条不再出现）：条目多过可见区时在这里滚。 -->
          <div v-else class="page-list memory-list">
            <MemoryRow
              v-for="row in list"
              :key="`dsh:${row.id}`"
              :row="row"
              source="DSH"
              :source-namespace="status?.namespace || ''"
              @open="openMemoryDetail"
            />
            <MemoryRow
              v-for="row in mcodeList"
              :key="`mcode:${row.id}`"
              :row="row"
              source="MiniMax"
              :source-namespace="mcodeMemory.status?.namespace || sourceNamespace('MiniMax')"
              @open="mcodeDetailId = row.id"
            />
          </div>
        </div>
      </div>

      <div class="page-layout__col">
        <div class="card">
          <div class="card-head"><h2>记忆使用</h2></div>
          <div class="memory-summary-row"><span>跨来源普通记忆互通</span><span :title="memoryModule.error || memoryModule.status?.warning || ''">{{ moduleStatusText }}</span></div>
          <div class="memory-summary-row"><span>历史逐条授权</span><el-button text :loading="memoryShares.allLoading" :disabled="globalBusy" @click="openShareManagement">查看与管理</el-button></div>
          <div class="memory-summary-row"><span>模块授权范围</span><span>通用 / 同项目</span></div>
          <div v-if="mcodeExperience" class="memory-summary-row">
            <span><span class="minimax-wordmark">MiniMax</span> 工作台</span><span>{{ mcodeRuntimeShort }}</span>
            <el-tooltip :content="mcodeMemoryRuntime" placement="bottom-start"><el-icon class="card-info-icon" tabindex="0" :aria-label="mcodeMemoryRuntime"><InfoFilled /></el-icon></el-tooltip>
          </div>
        </div>
        <div v-if="activeTab === 'library'" class="card">
          <div class="card-head"><h2>待确认 <span class="origin-chip origin-chip-kernel">DSH</span></h2><el-button text @click="selectTab('candidates')">查看</el-button></div>
          <p v-if="!memoryStore.view" class="memory-hint">{{ isLoading('memoryList') ? '正在读取待确认…' : '待确认暂时无法读取。' }}</p>
          <p v-else-if="!candidates.length" class="memory-hint">没有待确认的候选。</p>
          <button v-for="item in candidates.slice(0, 2)" :key="item.id" type="button" class="memory-review-item" @click="openCandidateDetail(item)">{{ item.title || '（无标题）' }}</button>
          <p v-if="candidates.length > 2" class="memory-note">共 {{ candidates.length }} 条待确认</p>
        </div>
        <div class="card">
          <div class="card-head">
            <h2>
              <span class="card-title-icon" aria-hidden="true"><el-icon><Promotion /></el-icon></span>
              最近注入 <span class="origin-chip origin-chip-kernel">DSH</span>
              <!-- 常驻说明收进标题旁的 ⓘ（2026-10-16 用户要求「提示文字能做 tooltip 的都用
                   tooltip 收纳起来」「尽量不出现纵向滚动条」）：它每屏都读、读完不改变操作，
                   却常年占四行；触发图标 `tabindex="0"` + `aria-label`，键盘也够得到。 -->
              <el-tooltip placement="bottom-start" :show-after="80">
                <template #content>
                  <!-- prettier-ignore -->
                  <div class="card-info-tooltip">
                    实况：插件报告它真的把哪几条放进了模型上下文。<br />
                    下面那个「检索预览」只是查询词由你填，检索是真跑的（同一份记忆库、同一套打分）。<br />
                    壳重启后这份记录会清空（它不是权威记录——权威记录是磁盘上的 memory.json，「检索预览」卡片底部写着它的位置）。
                  </div>
                </template>
                <el-icon class="card-info-icon" tabindex="0" aria-label="最近注入是什么">
                  <InfoFilled />
                </el-icon>
              </el-tooltip>
            </h2>
          </div>
          <p v-if="!recentInjections.length" class="memory-hint">
            还没有记录。在工作台聊一轮，这里会出现「哪一回合、哪个项目、注入了哪几条」。
          </p>
          <!-- 列表加上限：这是**会长的那一半**（每聊一轮多一行），而页面级滚动条正是它顶出来的；
               等高到上限后在卡内滚。每行只留项目名最后一段（完整路径进 title，一个字没少）。 -->
          <div v-if="recentInjections.length" class="memory-scroll">
            <p
              v-for="(note, index) in recentInjections.slice(0, 5)"
              :key="`${note.at}-${index}`"
              class="memory-note"
              :title="note.workspace"
            >
              {{ shortTime(note.at) }} · {{ baseName(note.workspace) || '未记录项目' }} · {{ injectionText(note) }}
            </p>
            <p v-if="recentInjections.length > 5" class="memory-note">
              只列最近 5 次（共 {{ recentInjections.length }} 条记录）。
            </p>
          </div>
        </div>

        <div class="card">
          <div class="card-head">
            <h2>
              <span class="card-title-icon" aria-hidden="true"><el-icon><MagicStick /></el-icon></span>
              提取任务 <span class="origin-chip origin-chip-kernel">DSH</span>
              <!-- 常驻说明收进 ⓘ，同「最近注入」。判据要的那句「一条记忆都没提取出来时
                   先看这里」在 tooltip 里，且**不折行**（判据是 assert.match）。 -->
              <el-tooltip placement="bottom-start" :show-after="80">
                <template #content>
                  <!-- prettier-ignore -->
                  <div class="card-info-tooltip">
                    自动提取的任务表：聊完一轮之后，模型会把这一段里值得长期保留的东西提出来，每一行是聊过的一段；提取出来的长期约定会直接存成项目记忆；与既有记忆冲突或推翻旧候选的才进「待确认」由你裁决。它由「授权范围」中的 DSH 原有记忆与自动提取开关控制。<br />
                    <strong>一条记忆都没提取出来时先看这里</strong>——失败那行会写明卡在哪一步（读取会话 / 切分片段 / 模型调用 / 解析输出 / 提交候选 / 已取消）。<br />
                    完整留痕在「查看日志」的 memory-jobs 标签下。
                  </div>
                </template>
                <el-icon class="card-info-icon" tabindex="0" aria-label="提取任务是什么">
                  <InfoFilled />
                </el-icon>
              </el-tooltip>
            </h2>
            <el-button text :loading="isLoading('memoryList')" @click="refreshMemory()">刷新</el-button>
          </div>
          <p class="memory-note">{{ jobsText }}</p>
          <div v-if="recentJobs.length" class="memory-scroll">
            <p
              v-for="(job, index) in recentJobs"
              :key="`${job.key?.session}-${job.key?.cursor}-${index}`"
              class="memory-note"
            >
              {{ jobLine(job) }}
            </p>
          </div>
        </div>

        <div class="card">
          <div class="card-head">
            <h2><span class="card-title-icon" aria-hidden="true"><el-icon><Search /></el-icon></span>检索预览 <span class="origin-chip origin-chip-kernel">DSH</span></h2>
            <el-button
              text
              :loading="isLoading('memoryPreview')"
              :disabled="globalBusy || !memoryStore.previewQuery"
              @click="previewContext"
            >
              试一次
            </el-button>
          </div>
          <!-- 没编辑过任何一条时按钮是禁用的：查询取自**上一次编辑过的**标题与关键词，
               空查询在后端是「所有条目 0 分」，点下去会报出一批与输入无关的命中。 -->
          <p class="memory-hint">
            {{
              memoryStore.previewQuery
                ? '拿你上次在编辑器里填的标题与关键词当查询，看这次会注入哪几条（固定项排在最前）。'
                : '先在编辑器里填标题或关键词，再回来点「试一次」。'
            }}
            {{ memoryStore.preview ? `会注入 ${memoryStore.preview.hits.length} 条，共 ${memoryStore.preview.total_bytes} 字节。` : '' }}
          </p>
          <pre v-if="memoryStore.preview && memoryStore.preview.text" class="memory-preview">{{ memoryStore.preview.text }}</pre>
          <p v-else-if="memoryStore.preview && !memoryStore.preview.enabled" class="memory-hint">
            开关是关着的：本次不会注入任何记忆。
          </p>
          <p v-else class="memory-hint">还没有预览结果。</p>
          <p class="memory-note" :title="status ? status.file : ''">
            记录文件：<template v-if="status">{{ baseName(status.file) }}（{{ status.accepted }} 条启用 / {{ status.disabled }} 条停用）</template>
            <template v-else>{{ memoryStore.listError ? '读取失败' : '读取中…' }}</template>
          </p>
        </div>
      </div>
    </div>
    </template>

    <MemoryEffectPanel v-else />

    <!--
      编辑器在弹窗里（`MemoryEditor.vue`）：新建与编辑共用同一个，开关与草稿都在
      `memory.js`（`editorVisible` / `openMemoryEditor` / `closeMemoryEditor`）。
      挂在这里而不是挂在某个按钮上：`el-dialog` 是 `append-to-body` 的，
      位置只影响它读 store 的时机，不影响渲染到哪里。
    -->
    <MemoryEditor />

    <!--
      记忆详情：列表行只留标题与两行正文，整块可点，点开在这里读完整正文、改状态、删除。
      挂在 `detailRow` 上而不是自己记「开没开」——记忆被删掉时那个 computed 变 `null`，
      弹窗自己关掉，不必再同步一次开关状态。
    -->
    <MemoryDetail
      v-if="detailRow && detailRow.kind !== 'handoff'"
      :row="detailRow"
      source-label="DSH"
      :source-namespace="status?.namespace || ''"
      :share-available="canShare && (mcodeExperience || isShared(detailRow, memoryStore.status?.namespace))"
      :shared="isShared(detailRow, memoryStore.status?.namespace)"
      :share-stale="shareNeedsReview(detailRow, memoryStore.status?.namespace)"
      :share-busy="memoryShares.changing"
      @close="detailId = null"
      @edit="editFromDetail"
      @toggle="(state) => setMemoryState(detailRow, state)"
      @remove="removeMemory"
      @share="(shared) => setMemoryShare(detailRow, memoryStore.status?.namespace, shared)"
    />
    <MemoryDetail v-if="mcodeDetail" :row="mcodeDetail" source-label="MiniMax" :source-namespace="mcodeMemory.status?.namespace || sourceNamespace('MiniMax')"
      :readonly="!mcodeExperience" :busy="mcodeMemory.loading || !!mcodeMemory.status?.warning" :active-action="mcodeMemory.active"
      @close="mcodeDetailId = null" @edit="openMcodeEditor" @toggle="(state) => setMcodeMemoryState(mcodeDetail.id, state)" @remove="removeMcodeMemory" />
    <MemoryDetail v-if="sharedDetail" :row="sharedDetail.record" readonly source-label="DSH"
      :source-namespace="`dsh/${sharedDetail.source_instance}/${sharedDetail.source_profile}`" @close="sharedDetailKey = ''" />
    <MemoryHandoffDetail
      v-if="detailRow && detailRow.kind === 'handoff'"
      :row="detailRow"
      @close="detailId = null"
      @edit="editFromDetail"
      @archive="(row) => setMemoryState(row, 'archived')"
      @remove="removeMemory"
    />

    <!--
      候选详情：待确认那一档的候选行正文截成两行，点开在这里读完整正文与来源凭据
      （完整会话 id、创建时间、验证状态、冲突 / 取代 / 来源已变三种关系），页脚直接
      保存 / 存为通用偏好 / 忽略。挂法同记忆详情：按 id 从 `candidates` 现取，
      处理完回读把这条摘掉，computed 变 `null`，弹窗自己关。
    -->
    <MemoryCandidateDetail
      v-if="candidateDetailItem"
      :item="candidateDetailItem"
      :records="allMemories"
      :candidates="candidates"
      @close="candidateDetailId = null"
    />
    <el-dialog v-model="sharesVisible" title="记忆共享授权" width="min(680px, 92vw)" append-to-body>
      <p class="memory-hint">所有实例的逐条授权都在这里；来源或目标已不存在时仍可撤销。</p>
      <el-alert v-if="memoryShares.allError || memoryShares.error" type="error" :closable="false" :title="memoryShares.allError || memoryShares.error" />
      <template v-if="memoryShares.loading || memoryShares.loaded">
        <h3>当前 <span class="minimax-wordmark">MiniMax</span> 目标的授权记录</h3>
        <p v-if="memoryShares.loading" class="memory-hint">正在读取授权…</p>
        <p v-else-if="!memoryShares.items.length" class="memory-hint">没有已授权的 DSH 记忆。</p>
        <div v-for="item in memoryShares.items" :key="`${item.source_instance}/${item.source_profile}/${item.record_id}`" class="memory-share-item">
          <p class="memory-note">来源：DSH · {{ item.source_instance }} · {{ item.source_profile }}</p>
          <p v-if="!item.record" class="memory-note">来源记录不可用或已删除；仍可撤销这项授权。</p>
          <p v-else-if="!item.scope_valid" class="memory-note">来源范围已扩大或更换项目，需回到 DSH 重新授权后才会用于检索。</p>
          <MemoryRow v-if="item.record" :row="item.record" source="DSH"
            :source-namespace="`dsh/${item.source_instance}/${item.source_profile}`"
            @open="sharedDetailKey = `${item.source_instance}/${item.source_profile}/${item.record_id}`; sharesVisible = false" />
          <el-button text type="danger" :loading="memoryShares.changing" :disabled="globalBusy || memoryShares.changing" @click="revokeCurrentMemoryShare(item)">撤销授权</el-button>
        </div>
      </template>
      <h3>{{ memoryShares.loaded ? '其他目标的授权' : '所有实例的授权' }}</h3>
      <p v-if="!otherMemoryShares.length" class="memory-hint">{{ memoryShares.allLoading ? '正在读取授权…' : memoryShares.allError ? '授权读取失败，请重试。' : '没有其他逐条授权。' }}</p>
      <div v-for="item in otherMemoryShares" :key="`${item.source_instance}/${item.source_profile}/${item.target_instance}/${item.target_profile}/${item.record_id}`" class="memory-share-item">
        <p class="memory-note">DSH {{ item.source_instance }} / {{ item.source_profile }} · 记忆 {{ item.record_id }}</p>
        <p class="memory-note"><span class="minimax-wordmark">MiniMax</span> {{ item.target_instance }} / {{ item.target_profile }} · {{ item.scope === 'general' ? '通用偏好' : item.workspace_path }}</p>
        <el-button text type="danger" :loading="memoryShares.changing" :disabled="globalBusy || memoryShares.changing" @click="revokeAnyMemoryShare(item)">撤销授权</el-button>
      </div>
    </el-dialog>
  </section>
</template>

<style scoped>
/*
 * 整页固定在窗口高度内（2026-10-17 用户要求「尽量不要出现纵向滚动条」）：
 * `.panel` 拿满 `main` 的内容高度，栅格吃掉页头以下的全部空间，左栏那张卡再
 * 把剩余高度交给 `memory-list`。于是**页面级滚动条没有出现的余地**——列表长了
 * 在卡内滚（`memory.css` 的 `.memory-list`），而不是把整页顶长。
 *
 * 高度链每一级都要有 `min-height: 0`：flex / grid 子项的默认下限是 min-content，
 * 少一级就会撑回内容高度、页面照样滚（这是「设了 flex 却没用」最常见的原因）。
 * 窗口固定 1040×748（`tauri.conf.json`），所以这里是确定值，不是 vh 估算。
 */
.panel {
  height: 100%;
  min-height: 0;
}

.page-layout {
  flex: 1 1 auto;
  min-height: 0;
  /* 栅格默认 `align-items: start`：两列各自按内容高度收，左栏的卡就不会被拉到底。
     改成 stretch 后两列同高，卡片才有「剩余空间」可分配。 */
  align-items: stretch;
  grid-template-rows: minmax(0, 1fr);
}

.page-layout__col {
  min-height: 0;
}

/* 左栏第二列（右栏）在内容超长时自己滚：它是兜底，正常数据（≈563px）够放。 */
.page-layout__col:last-child {
  overflow-y: auto;
}

/* 页签行的缝由容器给。 */
.memory-tabs {
  display: flex;
  gap: 4px;
}

/* 页顶「允许使用记忆」：标签 + 两个目标并排。它与页头分属两块，所以不塞进
   `.page-head`（那里只有「标题块 + 开关块」两个直接子级）。状态词常驻、解释在 ⓘ。 */
/* 目标开关并排一行，落在通用记忆模块卡里（不再单独一条带边框的横带）。 */
.memory-target {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}

.memory-target__name {
  font-size: var(--fs-5);
}
/* 品牌词颜色只由 .minimax-wordmark 给：同元素的 scoped 规则特异度更高。 */
.memory-target__name:not(.minimax-wordmark) {
  color: var(--text);
}

/* 状态词跟开关同排：已开启 / 已关闭 / 跟随原设置 / 状态未知 / 未配置实例。 */
.memory-target__state {
  font-size: var(--fs-4);
  color: var(--text-muted);
}

/* 记忆库那张卡：**高度跟着内容走**（`flex: 0 1 auto`，与右栏三张卡同一形状），
   内容长了再被栅格的高度上限压住——`min-height: 0` 允许它压到内容高度以下，
   压出来的那截交给 `.memory-list` 自己滚。写成 `flex: 1 1 auto` 会让内容只有
   三四条时卡片也拉到底、下面空出一大块（2026-10-17 截图对比过）。 */
.memory-library {
  flex: 0 1 auto;
  min-height: 0;
  overflow: hidden;
}

.memory-legacy-setting, .memory-summary-row {
  display: flex;
  align-items: center;
  gap: 8px;
}
.memory-legacy-setting { margin-top: 12px; }
.memory-legacy-setting > span:first-child, .memory-summary-row > span:first-child { flex: 1; min-width: 0; }
.memory-review-item {
  display: block;
  width: 100%;
  padding: 4px 0;
  border: 0;
  background: transparent;
  color: var(--text);
  font: inherit;
  text-align: left;
  overflow-wrap: anywhere;
  cursor: pointer;
}
.memory-review-item:hover, .memory-review-item:focus-visible { color: var(--accent); }
.memory-review-item:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
.memory-share-item { padding: 8px 0; border-top: 1px solid var(--border-soft); }

/* 卡头放不下时标题与按钮折成两行，折下去的那一行靠右——`space-between` 只管同一行内
   的两端，单独落一行时它会默认贴左。 */
.memory-head__actions {
  display: flex;
  gap: 4px;
  margin-left: auto;
}

/* 说明文字与卡里其它灰字同档；块之间的缝由 `.card` 的 gap 给，这里不再自带外边距。 */
.memory-hint {
  margin: 0;
  color: var(--text-secondary);
  font-size: var(--fs-5);
  overflow-wrap: anywhere;
}

/* `.memory-note` / `.memory-over-limit` 已搬进 `memory.css`（2026-10-16）：编辑器
   独立成组件之后这两个类名被面板与 `MemoryEditor.vue` 同时使用，而 scoped 样式跨不过
   组件——正是那个文件顶部写着的落点规则。 */

.memory-preview {
  margin: 0;
  max-height: 220px;
  overflow: auto;
  padding: 10px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--surface-subtle);
  color: var(--text-secondary);
  font-size: var(--fs-5);
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}
</style>
