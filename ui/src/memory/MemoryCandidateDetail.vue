<script setup>
// 一条待确认候选的详情：完整正文、范围、来源、时间、验证状态，以及模型标出的
// 冲突 / 取代关系与「来源变了」提示；页脚可以直接裁决这条候选。
//
// 2026-10-19 用户反馈：「待确认」的候选行正文被截成两行，点行没有任何反应，读不到
// 完整信息。行的定位与 `MemoryRow.vue` 一致（列表只负责读，点开才是详情），这一层
// 拿来看全并当场操作。
//
// 为什么独立成组件：行是列表里的一格（`page-row` + 栅格），弹窗是 `el-dialog`
// （append-to-body、固定宽度、页脚），两种容器没有一处共享；塞回行组件就得在一份
// 模板里切换两套布局。`MemoryPanel.vue` 也不该再长：它只管列表与筛选。
//
// 写法照 `MemoryDetail.vue`（同屏两个详情弹窗必须长得一样）：`append-to-body`、
// 宽度 `min(560px, 92vw)`、点遮罩不关（这层是阅读用的，误关要重新点一次）。
// 正文那族类名复用 `memory-row*`（跨组件共用，样式在 `memory.css`）：详情里的标题、
// 正文、次行必须与行上同一套排版，另抄一份近似排版就是两处各漂各的。它们带的
// `grid-column` / `grid-row` 在这里是惰性的——`__main` 不再是 subgrid 的子项，落成
// 一个单列网格，标题、正文、次行按序占行（与 `MemoryDetail.vue` 同一个理由）。
//
// 三种关系块（冲突 / 取代 / 来源已变）是行上**同一批组件、同一族类名**：它们不是
// 详情独有的信息，只是行里被压短了，点开要看全。
import { computed } from 'vue';
import MemoryCandidateActions from './MemoryCandidateActions.vue';
import MemoryConflict from './MemoryConflict.vue';
import MemorySuperseded from './MemorySuperseded.vue';
import { globalBusy } from '../shell/loading.js';
import { baseName } from '../shell/labels.js';
import {
  decideCandidate,
  removeMemory,
  setMemoryState,
  shortTime,
  sourceText,
  verificationText,
} from './memory.js';

const props = defineProps({
  item: { type: Object, required: true },
  /// 冲突的那几条要从**全库**（未筛选）解析 id → 标题与正文。
  records: { type: Array, default: () => [] },
  /// 取代的那几条是**候选**，不在记忆列表里。
  candidates: { type: Array, default: () => [] },
});
const emit = defineEmits(['close']);

/// 与行同一口径（见 `MemoryRow.vue` 同名 computed）：项目名只取路径最后一段，
/// 完整路径进 `title`；路径缺失时兜底写**范围名 + 缺了什么**，不猜一个项目名字。
const projectName = computed(() => baseName(props.item.workspace_path) || '项目记忆（未记路径）');
/// 悬浮说明与标签取同一个兜底，否则悬停说明与可见文字各说一个名字。
const scopeTitle = computed(() =>
  props.item.scope === 'general' ? '所有项目都算' : props.item.workspace_path || '项目记忆（未记路径）'
);

/// 候选上的 `stale` 是 H7「来源那段话被改过」，**不是**记忆那条「事实可能过时」：
/// 候选被批准时后端会把 verification 重置成 verified（`store.rs::decide_candidate`）。
/// 两个 `stale` 用同一个词，用户会按错的那个去理解——所以这里不能直接打印
/// `verificationText` 的「已过期」，要用行上那个词。
const verificationLabel = computed(() =>
  props.item.verification === 'stale' ? '来源已变' : verificationText(props.item.verification)
);

/// 交接候选的四个字段（goal/done/todo/blocked）：与 `MemoryHandoffDetail` 同一套
/// 标签顺序，候选详情是审阅草稿的那一层，两边读起来必须是同一张表。
const handoffLists = computed(() => [
  { label: '已完成', rows: props.item.done || [] },
  { label: '待办', rows: props.item.todo || [] },
  { label: '阻塞', rows: props.item.blocked || [] },
]);
</script>

<template>
  <el-dialog
    class="memory-dialog"
    :model-value="true"
    :title="item.title || '（无标题）'"
    width="min(560px, 92vw)"
    append-to-body
    :close-on-click-modal="false"
    @update:model-value="(open) => !open && emit('close')"
  >
    <div class="memory-row__main">
      <div class="memory-row__title">
        <el-tag v-if="item.kind === 'experience'" size="small" effect="plain">经验候选 · 审核后生效</el-tag>
        <el-tag v-if="item.kind === 'handoff'" size="small" effect="plain">交接候选 · 审核后生效</el-tag>
        <!-- 范围标签与记忆行同一口径：项目名取最后一段（完整路径进 title），
             通用偏好写「通用偏好」——候选在待确认里没有别的字段说明它属于哪个项目。 -->
        <el-tag
          size="small"
          effect="plain"
          :type="item.scope === 'general' ? 'info' : 'primary'"
          :title="scopeTitle"
        >
          {{ item.scope === 'general' ? '通用偏好' : projectName }}
        </el-tag>
        <el-tag size="small" effect="plain">{{ sourceText(item.source.kind) }}</el-tag>
        <!-- 「来源已变」不再单独挂一枚标签：这一层已经有次行的「验证：来源已变」与下面
             那段黄框说明，同一句话在 560px 里出现三次是噪音（行上那枚标签仍留着，
             那是列表里唯一的标记）。 -->
      </div>
      <p class="memory-row__body">{{ item.body }}</p>
      <!-- 交接草稿的结构化字段：审的是这份草稿，不能只给一句自由正文。 -->
      <template v-if="item.kind === 'handoff'">
        <h3 class="memory-handoff-heading">目标</h3>
        <p class="memory-row__body">{{ item.goal || '未填写' }}</p>
        <section v-for="list in handoffLists" :key="list.label">
          <h3 class="memory-handoff-heading">{{ list.label }}</h3>
          <ul v-if="list.rows.length" class="memory-handoff-list">
            <li v-for="(text, index) in list.rows" :key="index">{{ text }}</li>
          </ul>
          <p v-else class="memory-row__meta">无</p>
        </section>
      </template>
      <!-- 行里为了读得下去把会话 id 压成短的那个；详情是追查用的那一层，给完整 id。 -->
      <p class="memory-row__meta">
        会话 id：{{ item.source.session || '（本机，这一条没有会话 id）' }} · 创建于
        {{ shortTime(item.created_at) }} · 验证：{{ verificationLabel }}
      </p>
      <!-- H7 与行上同一句话、同一个 `.memory-flag` 盒子：三种关系看起来一样重，
           看起来不一样就会暗示哪一种更严重，而它们其实都要用户按一次。 -->
      <div v-if="item.verification === 'stale'" class="memory-flag">
        <p class="memory-flag__lead">
          这条是从会话里<strong>同一段话</strong>提的，而那段内容在这次提取之前被改动过
          （压缩或编辑都会）。系统没有替你处理掉它——要不要留，按下面的「保存」决定。
        </p>
      </div>
      <!-- 两个关系块直接读候选字段：`MemoryConflict` / `MemorySuperseded` 的 prop 默认值
           就是空数组，老数据没有这两个键时落到默认值（行上写的 `|| []` 是同一件事）。 -->
      <MemoryConflict
        :conflicts="item.conflicts_with"
        :records="records"
        :busy="globalBusy"
        @disable="(record) => setMemoryState(record, 'disabled')"
        @remove="removeMemory"
      />
      <MemorySuperseded
        :supersedes="item.supersedes"
        :candidates="candidates"
        :busy="globalBusy"
        @discard="(candidate) => decideCandidate(candidate, false)"
      />
    </div>
    <template #footer>
      <!-- 动作排与列表行共用一份（`MemoryCandidateActions`）：行上点「忽略」与在详情里
           点「忽略」必须做同一件事。`dialog-actions` 只负责把这一排靠右。 -->
      <MemoryCandidateActions class="dialog-actions" :item="item" />
    </template>
  </el-dialog>
</template>
