<script setup>
import { computed, ref } from 'vue';
import { mcodeExperience } from '../kernel/experience.js';
import { globalBusy } from '../shell/loading.js';
import { poolReview, reviewKey, decidePoolReview } from './pool.js';
import { baseName } from '../shell/labels.js';

const emit = defineEmits(['changed']);
const props = defineProps({ source: { type: String, default: 'all' }, records: { type: Array, default: () => [] } });
const detailKey = ref('');
const items = computed(() => [...poolReview.candidates, ...poolReview.legacyRecords]
  .filter(item => props.source === 'all' || item.origin_label === (props.source === 'dsh' ? 'DSH' : 'MiniMax')));
function relatedRecords(item) {
  return (value(item).conflicts_with || []).map(id => props.records.find(row => row.source_namespace === item.source_namespace && row.record.id === id)?.record || { id, title: '记录不可用', body: '' });
}
function supersededCandidates(item) {
  return (value(item).supersedes || []).map(id => poolReview.candidates.find(row => row.source_namespace === item.source_namespace && row.candidate.id === id)?.candidate || { id, title: '候选不可用', body: '' });
}
const detail = computed(() => items.value.find(item => reviewKey(item) === detailKey.value));
const value = item => item.candidate || item.record;
const kindText = item => ({ handoff: '交接 · 本来源使用', experience: '经验 · 本来源使用' })[value(item).kind] || '普通记忆 · 公共池';
const scopeText = item => value(item).scope === 'general' ? '通用偏好' : baseName(value(item).workspace_path) || '项目记忆';
function readonly(item) { return item.origin_label === 'MiniMax' && !mcodeExperience.value; }
async function decide(item, accept) {
  if (readonly(item)) return;
  if (await decidePoolReview(item, accept)) emit('changed');
}
</script>

<template>
  <el-alert v-if="poolReview.error" type="error" :closable="false" :title="poolReview.error" />
  <p v-if="!poolReview.loaded" class="memory-note">{{ poolReview.loading ? '正在读取待审核记忆…' : '待审核记忆暂时无法读取，请刷新重试。' }}</p>
  <p v-else-if="!items.length" class="memory-note">没有待审核的记忆。会话产生的候选会在这里显示来源，审核通过后进入记忆池。</p>
  <div v-else class="page-list memory-list">
    <!-- 行形状与记忆行 / 候选行同一族（2026-11-12 用户截图：待审核 27 条整片叠字）：
         正文占满整行、两行截断，动作排沉到行底贴右。复用 `.memory-row--candidate` +
         `.memory-row__main` 的 subgrid 而不是在这里另写一套——同一屏两种行语法，
         改一边漏一边是静默的。 -->
    <div v-for="item in items" :key="reviewKey(item)" class="page-row memory-row memory-row--candidate">
      <button type="button" class="memory-row__main pool-review-main" @click="detailKey = reviewKey(item)">
        <span class="memory-row__title"><span class="origin-chip origin-chip-kernel">{{ item.origin_label }}</span><strong class="memory-row__name">{{ value(item).title || '（无标题）' }}</strong></span>
        <span class="memory-row__body memory-row__body--clamp">{{ value(item).body || value(item).goal }}</span>
        <span class="memory-row__meta">{{ kindText(item) }} · {{ scopeText(item) }} · {{ item.record ? '旧版未审核记录' : '待审核' }}</span>
      </button>
      <div class="memory-actions memory-row__actions">
        <el-button class="memory-badge" :loading="poolReview.changing === reviewKey(item)" :disabled="globalBusy || poolReview.loading || !!poolReview.changing || readonly(item)" @click="decide(item, true)">审核通过</el-button>
        <el-button class="memory-badge memory-badge--danger" :disabled="globalBusy || poolReview.loading || !!poolReview.changing || readonly(item)" @click="decide(item, false)">{{ item.record ? '停用' : '忽略' }}</el-button>
      </div>
    </div>
  </div>
  <el-dialog :model-value="!!detail" :title="detail ? value(detail).title || '审核记忆' : '审核记忆'" width="min(620px, 92vw)" append-to-body :close-on-click-modal="false" @update:model-value="open => !open && (detailKey = '')">
    <template v-if="detail">
      <p class="memory-note">来源：{{ detail.origin_label }} · {{ kindText(detail) }} · {{ scopeText(detail) }}</p>
      <p class="memory-note">{{ detail.source_namespace }}</p>
      <p v-if="readonly(detail)" class="memory-note">MiniMax 体验已关闭，此来源仅供查看。</p>
      <p v-if="value(detail).workspace_path" class="memory-note">项目：{{ value(detail).workspace_path }}</p>
      <p class="pool-review-body">{{ value(detail).body || value(detail).goal }}</p>
      <template v-if="value(detail).kind === 'experience'">
        <p v-for="field in ['situation', 'action', 'outcome', 'lesson']" :key="field" class="pool-review-body">{{ ({ situation: '情境', action: '行动', outcome: '结果', lesson: '经验' })[field] }}：{{ value(detail)[field] || '未填写' }}</p>
      </template>
      <p v-if="value(detail).kind === 'handoff'" class="pool-review-body">目标：{{ value(detail).goal || '未填写' }}</p>
      <template v-for="field in ['done', 'todo', 'blocked']" :key="field">
        <p v-if="value(detail)[field]?.length" class="memory-note">{{ ({ done: '已完成', todo: '待办', blocked: '阻塞' })[field] }}：{{ value(detail)[field].join('；') }}</p>
      </template>
      <p v-if="value(detail).source?.session" class="memory-note">会话：{{ value(detail).source.session }}</p>
      <p v-if="value(detail).verification === 'stale'" class="memory-note">来源已变，请重新核对正文后再审核。</p>
      <div v-if="relatedRecords(detail).length"><h3>与这些记忆冲突</h3><p v-for="record in relatedRecords(detail)" :key="record.id" class="pool-review-body">{{ record.title }}：{{ record.body }}</p><p class="memory-note">审核不会替你停用旧记忆；请在记忆池核对并处理。</p></div>
      <div v-if="supersededCandidates(detail).length"><h3>提出取代这些候选</h3><p v-for="candidate in supersededCandidates(detail)" :key="candidate.id" class="pool-review-body">{{ candidate.title }}：{{ candidate.body }}</p><p class="memory-note">系统没有自动忽略这些候选，可在待审核列表分别处理。</p></div>
      <p v-if="value(detail).scope === 'general'" class="memory-note">通用偏好会用于其他项目，先确认正文里没有路径、客户名之类的私有信息。</p>
    </template>
    <template #footer>
      <el-button @click="detailKey = ''">关闭</el-button>
      <el-button v-if="detail" type="primary" :loading="poolReview.changing === reviewKey(detail)" :disabled="globalBusy || poolReview.loading || !!poolReview.changing || readonly(detail)" @click="decide(detail, true)">审核通过</el-button>
      <el-button v-if="detail" type="danger" plain :disabled="globalBusy || poolReview.loading || !!poolReview.changing || readonly(detail)" @click="decide(detail, false)">{{ detail.record ? '停用' : '忽略' }}</el-button>
    </template>
  </el-dialog>
</template>

<style scoped>
/* 排版交给 `.memory-row__main` 的 subgrid；这里只把它从 `<div>` 换成可点的
   `<button>` 时需要的复位。标题里的 `.memory-row__name` 给 0 基线：不给时它的
   flex 基线是 max-content，长标题会被整体挤到来源 chip 下一行（顶掉一整行）。 */
.pool-review-main { width: 100%; padding: 0; border: 0; background: transparent; color: var(--text); font: inherit; text-align: left; cursor: pointer; }
.pool-review-main:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
.pool-review-main .memory-row__name { flex: 1 1 0; min-width: 0; }
.pool-review-body { white-space: pre-wrap; overflow-wrap: anywhere; }
</style>
