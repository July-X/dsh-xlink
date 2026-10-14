<script setup>
// 记忆面板：保存 / 编辑 / 停用 / 删除本机的长期偏好与项目约定，
// 并在右边把「按当前输入会注入什么」摊开给用户看。
//
// 数据与动作全在 `memory.js` 里；这里只管画。字段名用后端下发的 snake_case
// （见该文件顶部的说明）。
import { computed, onMounted } from 'vue';
import { Download, Refresh } from '@element-plus/icons-vue';
import { globalBusy, isLoading } from '../shell/loading.js';
import { builtinById, loadBuiltinStatus, toggleBuiltin } from '../plugins/builtin.js';
import {
  memoryStore,
  refreshMemory,
  saveMemory,
  removeMemory,
  setMemoryState,
  setMemoryEnabled,
  clearMemory,
  decideCandidate,
  previewContext,
  exportMemory,
  openExports,
  editMemory,
  resetDraft,
  rememberProjectPath,
  canSaveDraft,
  injectionDisabled,
  injectionText,
  visibleMemories,
  sourceText,
  verificationText,
  setListScope,
} from './memory.js';

const list = computed(() => visibleMemories(memoryStore.view, memoryStore.listScope));
const candidates = computed(() => (memoryStore.view && memoryStore.view.candidates) || []);
const status = computed(() => memoryStore.status);
const recentInjections = computed(() => memoryStore.recentInjections || []);
const enabled = computed(() => !!(status.value && status.value.use_approved));
const draft = computed(() => memoryStore.draft);
const savable = computed(() => canSaveDraft());

/// 注入链路的内置插件（`xlink-memory`）。**按 id 取，不取第一项**：
/// 取第一项会在登记表里多一个插件时静默指向别人。
const injectPlugin = computed(() => builtinById('memory'));

/// 打开注入插件。成功后回读一次状态（后端返回的那一项已经是真值，回读是为了
/// 与插件页共享的列表保持一致——两处各显示一份状态就会分叉）。
function enableInjection() {
  return toggleBuiltin('memory', true).then(() => loadBuiltinStatus());
}

function shortTime(value) {
  if (!value) return '未知时间';
  const parsed = new Date(value);
  if (Number.isNaN(parsed.getTime())) return value;
  const pad = (n) => String(n).padStart(2, '0');
  return `${parsed.getFullYear()}-${pad(parsed.getMonth() + 1)}-${pad(parsed.getDate())} ${pad(parsed.getHours())}:${pad(parsed.getMinutes())}`;
}

function scopeText(record) {
  if (record.scope === 'general') return '通用偏好（所有项目）';
  return record.workspace_path ? `本项目 · ${record.workspace_path}` : '本项目';
}

onMounted(() => {
  refreshMemory({ silent: true });
  loadBuiltinStatus();
});
</script>

<template>
  <section class="panel">
    <div class="page-head">
      <div class="page-head__text">
        <h1 class="page-title">记忆</h1>
        <p class="page-desc">
          保存长期有效的偏好与项目约定。打开右侧开关后，工作台的相关会话会带上这些内容；模型只能读取与提出候选，不能自己批准。
        </p>
      </div>
      <el-switch
        :model-value="enabled"
        :loading="isLoading('memoryEnabled')"
        :disabled="globalBusy"
        active-text="使用已批准的记忆"
        @change="setMemoryEnabled"
      />
    </div>

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
        记忆注入插件还没启用：现在保存的记忆不会进入任何会话。
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
        <div class="card">
          <div class="card-head">
            <h2>记忆（{{ list.length }}）</h2>
            <div class="memory-head__actions">
              <el-radio-group
                :model-value="memoryStore.listScope"
                size="small"
                :disabled="globalBusy"
                @change="setListScope"
              >
                <el-radio value="all">全部</el-radio>
                <el-radio value="project">本项目</el-radio>
                <el-radio value="general">通用偏好</el-radio>
              </el-radio-group>
              <el-button
                text
                :icon="Download"
                :loading="isLoading('memoryExport')"
                :disabled="globalBusy || !list.length"
                @click="exportMemory"
              >
                导出 Markdown
              </el-button>
              <el-button
                text
                type="danger"
                :loading="isLoading('memoryClear')"
                :disabled="globalBusy || (!list.length && !candidates.length)"
                @click="clearMemory"
              >
                清空
              </el-button>
              <el-button
                text
                :icon="Refresh"
                :loading="isLoading('memoryList')"
                :disabled="globalBusy"
                @click="refreshMemory()"
              >
                刷新
              </el-button>
            </div>
          </div>
          <p v-if="memoryStore.lastExport" class="memory-hint">
            最近导出：{{ memoryStore.lastExport.path }}（{{ memoryStore.lastExport.memories }} 条记忆、{{
              memoryStore.lastExport.candidates
            }}
            条候选）
            <el-button text type="primary" @click="openExports">打开导出目录</el-button>
          </p>
          <p v-if="!list.length" class="memory-hint">
            还没有记忆。在右边写一条，或者在工作台里让模型提出候选，再到「待确认」里点保存。
          </p>
          <div v-else class="page-list">
            <div v-for="row in list" :key="row.id" class="page-row memory-row">
              <div class="memory-row__main">
                <div class="memory-row__title">
                  <span class="memory-row__name">{{ row.title || '（无标题）' }}</span>
                  <el-tag size="small" effect="plain" :type="row.scope === 'general' ? 'info' : 'primary'">
                    {{ row.scope === 'general' ? '通用偏好' : '本项目' }}
                  </el-tag>
                  <el-tag v-if="row.pinned" size="small" effect="plain" type="warning">固定</el-tag>
                  <el-tag v-if="row.state !== 'accepted'" size="small" effect="plain" type="info">已停用</el-tag>
                </div>
                <p class="memory-row__body">{{ row.body }}</p>
                <p class="memory-row__meta">
                  {{ scopeText(row) }} · {{ sourceText(row.source.kind) }} ·
                  {{ verificationText(row.verification) }} · 更新于 {{ shortTime(row.updated_at) }} ·
                  已用 {{ row.use_count }} 次
                </p>
              </div>
              <div class="memory-row__actions">
                <el-button size="small" text :disabled="globalBusy" @click="editMemory(row)">编辑</el-button>
                <el-button
                  size="small"
                  text
                  :loading="isLoading('memoryState:' + row.id)"
                  :disabled="globalBusy"
                  @click="setMemoryState(row, row.state === 'accepted' ? 'disabled' : 'accepted')"
                >
                  {{ row.state === 'accepted' ? '停用' : '启用' }}
                </el-button>
                <el-button
                  size="small"
                  text
                  type="danger"
                  :loading="isLoading('memoryDelete:' + row.id)"
                  :disabled="globalBusy"
                  @click="removeMemory(row)"
                >
                  删除
                </el-button>
              </div>
            </div>
          </div>
        </div>

        <div v-if="candidates.length" class="card">
          <div class="card-head">
            <h2>待确认（{{ candidates.length }}）</h2>
          </div>
          <p class="memory-hint">
            这些是模型或会话提出的候选：正文与来源都摆在这里，**只有你点保存才会成为记忆**。
          </p>
          <div class="page-list">
            <div v-for="item in candidates" :key="item.id" class="page-row memory-row">
              <div class="memory-row__main">
                <div class="memory-row__title">
                  <span class="memory-row__name">{{ item.title || '（无标题）' }}</span>
                  <el-tag size="small" effect="plain">{{ item.source.kind }}</el-tag>
                </div>
                <p class="memory-row__body">{{ item.body }}</p>
                <p class="memory-row__meta">
                  来自 {{ item.source.session || '本机' }} · {{ shortTime(item.created_at) }}
                </p>
              </div>
              <div class="memory-row__actions">
                <el-button
                  size="small"
                  type="primary"
                  :loading="isLoading('memoryCandidate:' + item.id)"
                  :disabled="globalBusy"
                  @click="decideCandidate(item, true)"
                >
                  保存
                </el-button>
                <el-button
                  size="small"
                  text
                  :loading="isLoading('memoryCandidate:' + item.id)"
                  :disabled="globalBusy"
                  @click="decideCandidate(item, false)"
                >
                  忽略
                </el-button>
              </div>
            </div>
          </div>
        </div>
      </div>

      <div class="page-layout__col">
        <div class="card">
          <div class="card-head">
            <h2>{{ draft.id ? '编辑记忆' : '新建记忆' }}</h2>
          </div>
          <div class="memory-form">
            <el-radio-group v-model="draft.scope" :disabled="globalBusy">
              <el-radio value="project">本项目</el-radio>
              <el-radio value="general">通用偏好</el-radio>
            </el-radio-group>
            <el-select
              v-if="draft.scope === 'project'"
              :model-value="memoryStore.projectPath"
              class="memory-project"
              filterable
              allow-create
              default-first-option
              placeholder="选一个工作台里用过的项目，或直接输入目录"
              @update:model-value="rememberProjectPath"
            >
              <el-option
                v-for="item in memoryStore.workspaces"
                :key="item.id"
                :label="`${item.title} · ${item.path}`"
                :value="item.path"
              >
                <span class="memory-project__title">{{ item.title }}</span>
                <span class="memory-project__path">{{ item.path }}</span>
              </el-option>
            </el-select>
            <el-input v-model="draft.title" maxlength="80" placeholder="一句话标题，例如「提交前先跑 npm run check」" />
            <el-input
              v-model="draft.body"
              type="textarea"
              :rows="6"
              maxlength="4000"
              placeholder="写清约定本身与适用条件；可能过期的事实请标出来"
            />
            <el-input v-model="draft.keywordsText" placeholder="关键词，用逗号分隔（用于命中检索）" />
            <el-checkbox v-model="draft.pinned" :disabled="globalBusy">固定项（每次都带上）</el-checkbox>
            <div class="memory-form__actions">
              <el-button
                type="primary"
                :loading="isLoading('memorySave')"
                :disabled="!savable || globalBusy"
                @click="saveMemory"
              >
                保存
              </el-button>
              <el-button v-if="draft.id" :disabled="globalBusy" @click="resetDraft">取消编辑</el-button>
            </div>
          </div>
        </div>

        <div class="card">
          <div class="card-head">
            <h2>最近注入</h2>
          </div>
          <p class="memory-hint">
            实况：插件报告它真的把哪几条放进了模型上下文。下面那个「检索预览」是模拟——拿你填的查询算一遍。
            壳重启后这份记录会清空（它不是权威记录，权威记录是上面那份文件）。
          </p>
          <p v-if="!recentInjections.length" class="memory-hint">
            还没有记录。在工作台聊一轮，这里会出现「哪一回合、哪个项目、注入了哪几条」。
          </p>
          <div
            v-for="(note, index) in recentInjections.slice(0, 5)"
            :key="`${note.at}-${index}`"
            class="memory-row"
          >
            <p class="memory-row__meta">
              {{ shortTime(note.at) }} · {{ note.workspace || '未知项目' }} · {{ injectionText(note) }}
            </p>
          </div>
          <p v-if="recentInjections.length > 5" class="memory-row__meta">
            只列最近 5 次（共 {{ recentInjections.length }} 条记录）。
          </p>
        </div>

        <div class="card">
          <div class="card-head">
            <h2>检索预览</h2>
            <el-button
              text
              :loading="isLoading('memoryPreview')"
              :disabled="globalBusy"
              @click="previewContext"
            >
              试一次
            </el-button>
          </div>
          <p class="memory-hint">
            用编辑器里的标题与关键词当作查询，看这次会注入哪几条。
            {{ memoryStore.preview ? `命中 ${memoryStore.preview.hits.length} 条，共 ${memoryStore.preview.total_bytes} 字节。` : '' }}
          </p>
          <pre v-if="memoryStore.preview && memoryStore.preview.text" class="memory-preview">{{ memoryStore.preview.text }}</pre>
          <p v-else-if="memoryStore.preview && !memoryStore.preview.enabled" class="memory-hint">
            开关是关着的：本次不会注入任何记忆。
          </p>
          <p v-else class="memory-hint">还没有预览结果。</p>
          <p class="memory-row__meta">
            记录文件：{{ status ? status.file : '读取中…' }}（{{ status ? status.accepted : 0 }} 条启用 /
            {{ status ? status.disabled : 0 }} 条停用）
          </p>
        </div>
      </div>
    </div>
  </section>
</template>

<style scoped>
.memory-layout {
  align-items: start;
}

.memory-row {
  display: flex;
  gap: 12px;
  align-items: flex-start;
}

.memory-row__main {
  flex: 1;
  min-width: 0;
}

.memory-row__title {
  display: flex;
  gap: 8px;
  align-items: center;
  flex-wrap: wrap;
}

.memory-row__name {
  font-size: 15px;
  font-weight: 600;
}

.memory-row__body {
  margin: 4px 0 0;
  color: var(--text-secondary);
  overflow-wrap: anywhere;
  white-space: pre-wrap;
}

.memory-row__meta {
  margin: 4px 0 0;
  font-size: 12px;
  color: var(--text-muted);
  overflow-wrap: anywhere;
}

.memory-row__actions {
  display: flex;
  gap: 4px;
  flex: 0 0 auto;
}

.memory-form {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.memory-form__actions {
  display: flex;
  gap: 8px;
}

/* 项目下拉与表单里其它输入同宽；选项里标题与路径各占一行（路径可能很长，
   挤在一行会把标题挤没）。 */
.memory-project {
  width: 100%;
}

/* 卡头两个动作按钮同排；`.card-head` 是 space-between，包一层才不会被标题挤开。 */
.memory-head__actions {
  display: flex;
  gap: 4px;
}

.memory-project__title {
  margin-right: 8px;
}

.memory-project__path {
  color: var(--text-muted);
  font-size: 12px;
}

.memory-hint {
  margin: 0 0 8px;
  color: var(--text-secondary);
  font-size: 13px;
  overflow-wrap: anywhere;
}

.memory-preview {
  margin: 0;
  max-height: 220px;
  overflow: auto;
  padding: 10px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--surface-subtle);
  color: var(--text-secondary);
  font-size: 12px;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}
</style>
