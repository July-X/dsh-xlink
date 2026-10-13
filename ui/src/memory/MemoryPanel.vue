<script setup>
// 记忆面板：保存 / 编辑 / 停用 / 删除本机的长期偏好与项目约定，
// 并在右边把「按当前输入会注入什么」摊开给用户看。
//
// 数据与动作全在 `memory.js` 里；这里只管画。字段名用后端下发的 snake_case
// （见该文件顶部的说明）。
import { computed, onMounted } from 'vue';
import { Refresh } from '@element-plus/icons-vue';
import { globalBusy, isLoading } from '../shell/loading.js';
import {
  memoryStore,
  refreshMemory,
  saveMemory,
  removeMemory,
  setMemoryState,
  setMemoryEnabled,
  decideCandidate,
  previewContext,
  editMemory,
  resetDraft,
  rememberProjectPath,
  canSaveDraft,
} from './memory.js';

const list = computed(() => (memoryStore.view && memoryStore.view.memories) || []);
const candidates = computed(() => (memoryStore.view && memoryStore.view.candidates) || []);
const status = computed(() => memoryStore.status);
const enabled = computed(() => !!(status.value && status.value.use_approved));
const draft = computed(() => memoryStore.draft);
const savable = computed(() => canSaveDraft());

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

    <div class="page-layout">
      <div class="page-layout__col">
        <div class="card">
          <div class="card-head">
            <h2>记忆（{{ list.length }}）</h2>
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
                  {{ scopeText(row) }} · 更新于 {{ shortTime(row.updated_at) }} · 已用 {{ row.use_count }} 次
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
            <el-input
              v-if="draft.scope === 'project'"
              :model-value="memoryStore.projectPath"
              placeholder="项目根目录，例如 /Users/你/code/项目（与工作台的工作目录一致）"
              @update:model-value="rememberProjectPath"
            />
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
