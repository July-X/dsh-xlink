<script setup>
// 记忆面板：保存 / 编辑 / 停用 / 删除本机的长期偏好与项目约定，
// 并在右边把「按当前输入会注入什么」摊开给用户看。
//
// 数据与动作全在 `memory.js` 里；这里只管画。字段名用后端下发的 snake_case
// （见该文件顶部的说明）。
import { computed, onMounted } from 'vue';
import { Download, Refresh } from '@element-plus/icons-vue';
import MemoryRow from './MemoryRow.vue';
import MemoryConflict from './MemoryConflict.vue';
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
  scopeText,
  shortTime,
  setListScope,
  jobsSummaryText,
  jobLine,
  switchEffectText,
  byteLength,
  MAX_BODY_BYTES,
} from './memory.js';

const list = computed(() => visibleMemories(memoryStore.view, memoryStore.listScope));
/// **未筛选**的库：导出、清空、空态判据都用它——那些动作作用在整份文档上，
/// 与「当前视图看哪一档」无关。原先这三处读的是筛选后的 `list`，于是库里只有通用偏好时
/// 点「本项目」，卡片会说「还没有记忆」而导出与清空同时变灰点不动（评审第 2 条）。
const allMemories = computed(() => (memoryStore.view && memoryStore.view.memories) || []);
const candidates = computed(() => (memoryStore.view && memoryStore.view.candidates) || []);
const status = computed(() => memoryStore.status);
const recentInjections = computed(() => memoryStore.recentInjections || []);
/// 提取任务卡片：摘要那句 + 最近几条（后端已经截到 5 条，前端不再截）。
const jobsText = computed(() => jobsSummaryText(memoryStore.jobs));
/// 正文字节数：与后端 `MAX_BODY_BYTES` 同一个数，**按字节**算（中文一个字 3 字节）。
const bodyBytes = computed(() => byteLength(draft.value.body));
const recentJobs = computed(() => (memoryStore.jobs && memoryStore.jobs.jobs) || []);
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
      <!--
        开关与生效说明包成一层。`.page-head` 是 `space-between` 的两端布局，另外六个
        页面都只有一个 `.page-head__text` 子元素；此前这里是三个直接子级，说明那句
        被当成「右侧动作」顶到页面右缘、只剩一个字宽竖着排三行，开关也被夹在中间。
        包一层之后 `.page-head` 回到「一个子元素 + 一整行开关」的形状（这一层自己
        `flex: 1 0 100%`，靠页头的 `flex-wrap` 落到第二行），说明跟在开关右边按整行宽排。
      -->
      <div class="memory-switch">
        <el-switch
          :model-value="enabled"
          :loading="isLoading('memoryEnabled')"
          :disabled="globalBusy"
          active-text="使用已批准的记忆"
          @change="setMemoryEnabled"
        />
        <!-- 生效语义写在这（E6）：**不许说「已生效」**——面板不知道本会话还剩几次注入。 -->
        <p class="memory-note memory-switch__status">{{ switchEffectText(status) }}</p>
      </div>
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
              <el-button
                text
                :icon="Download"
                :loading="isLoading('memoryExport')"
                :disabled="globalBusy || !allMemories.length"
                @click="exportMemory"
              >
                导出 Markdown
              </el-button>
              <el-button
                text
                type="danger"
                :loading="isLoading('memoryClear')"
                :disabled="globalBusy || (!allMemories.length && !candidates.length)"
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
          <!--
            筛选切的是下面这份清单的视图，此前它挤在卡头那一排：卡内可用宽 340px，
            三档单选约 234px 加三枚文字按钮约 303px，于是「通用偏好」折到第二行、
            按钮再折到第三行，三样东西各自换行。挪出来单独占一行后一行放得下（余量
            约 100px）；卡头只剩标题与按钮两栏，放不下时按钮折到第二行、靠右对齐。
          -->
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
          <p v-if="memoryStore.lastExport" class="memory-hint">
            最近导出：{{ memoryStore.lastExport.path }}（{{ memoryStore.lastExport.memories }} 条记忆、{{
              memoryStore.lastExport.candidates
            }}
            条候选）
            <el-button text type="primary" @click="openExports">打开导出目录</el-button>
          </p>
          <p v-if="!allMemories.length" class="memory-hint">
            还没有记忆。在右边写一条，或者在工作台里让模型提出候选，再到「待确认」里点保存。
          </p>
          <!-- 库里有、只是当前筛选没有——这是**两回事**（记忆内核那次评审第 2 条）：
               说成「还没有记忆」是假话，而右栏状态行还写着 accepted: N。 -->
          <p v-else-if="!list.length" class="memory-hint">
            当前筛选下没有记忆（库里有 {{ allMemories.length }} 条，切到「全部」能看到）。
          </p>
          <div v-else class="page-list">
            <MemoryRow
              v-for="row in list"
              :key="row.id"
              :row="row"
              @edit="editMemory"
              @toggle="(state) => setMemoryState(row, state)"
              @remove="removeMemory"
            />
          </div>
        </div>

        <div v-if="candidates.length" class="card">
          <div class="card-head">
            <h2>待确认（{{ candidates.length }}）</h2>
          </div>
          <p class="memory-hint">
            这些是模型或会话提出的候选：正文与来源都摆在这里，只有你点保存才会成为记忆。
            要存成「所有项目都算」的通用偏好时用右边那个按钮——先确认正文里没有路径、客户名
            之类的私有信息，通用偏好会跟着你去别的项目。
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
                <!-- C5：并排摆出冲突的那几条，**由用户自己留哪一条**（不由模型裁决）。 -->
                <MemoryConflict
                  :conflicts="item.conflicts_with || []"
                  :records="allMemories"
                  :busy="globalBusy"
                  @disable="(record) => setMemoryState(record, 'disabled')"
                  @remove="removeMemory"
                />
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
                  v-if="item.scope !== 'general'"
                  size="small"
                  text
                  :loading="isLoading('memoryCandidate:' + item.id)"
                  :disabled="globalBusy"
                  @click="decideCandidate(item, true, 'general')"
                >
                  存为通用偏好
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
            <!--
              正文**不设 maxlength**：它数的是 UTF-16 码元，而后端闸的是 8192 **字节**——
              中文 4000 字约 12000 字节，用户按面板允许的长度写完会拿到「超过单条上限」，
              而面板一句提示都没有（记忆内核那次评审第 4 条）。改成按字节算的计数器 + 保存时拦住。
            -->
            <div class="memory-body">
              <el-input
                v-model="draft.body"
                type="textarea"
                :rows="6"
                placeholder="写清约定本身与适用条件；可能过期的事实请标出来"
              />
              <p class="memory-note" :class="{ 'memory-over-limit': bodyBytes > MAX_BODY_BYTES }">
                正文 {{ bodyBytes }} / {{ MAX_BODY_BYTES }} 字节（中文一个字 3 字节；超了就保存不了）
              </p>
            </div>
            <el-input v-model="draft.keywordsText" placeholder="关键词，用逗号分隔（用于命中检索）" />
            <el-checkbox v-model="draft.pinned" :disabled="globalBusy">固定项（每次都带上）</el-checkbox>
            <!--
              验证范围**不是**置信分数（§6.1）：只三档。给「已过期」一个入口，是因为
              一条过时的技术事实既不该继续当权威，也不该被悄悄删掉——标出来，让它排在
              检索结果的后面，用户自己决定什么时候重验或删掉。
            -->
            <el-radio-group v-model="draft.verification" :disabled="globalBusy">
              <el-radio value="verified">已验证</el-radio>
              <el-radio value="unverified">未验证</el-radio>
              <el-radio value="stale">已过期（事实可能过时）</el-radio>
            </el-radio-group>
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
          <p
            v-for="(note, index) in recentInjections.slice(0, 5)"
            :key="`${note.at}-${index}`"
            class="memory-note"
          >
            {{ shortTime(note.at) }} · {{ note.workspace || '未知项目' }} · {{ injectionText(note) }}
          </p>
          <p v-if="recentInjections.length > 5" class="memory-note">
            只列最近 5 次（共 {{ recentInjections.length }} 条记录）。
          </p>
        </div>

        <div class="card">
          <div class="card-head">
            <h2>提取任务</h2>
            <el-button text :loading="isLoading('memoryList')" @click="refreshMemory()">刷新</el-button>
          </div>
          <p class="memory-hint">
            自动提取（P2）的任务表：聊完一轮后插件排一条、壳发一个租约、模型提完再交回来。
            <strong>一条记忆都没提取出来时先看这里</strong>——失败那行会写明是哪一步
            （`read` / `model` / `parse` / `submit`）。完整留痕在「查看日志」的 memory-jobs 标签下。
          </p>
          <p class="memory-note">{{ jobsText }}</p>
          <p
            v-for="(job, index) in recentJobs"
            :key="`${job.key?.session}-${job.key?.cursor}-${index}`"
            class="memory-note"
          >
            {{ jobLine(job) }}
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
          <p class="memory-note">
            记录文件：{{ status ? status.file : '读取中…' }}（{{ status ? status.accepted : 0 }} 条启用 /
            {{ status ? status.disabled : 0 }} 条停用）
          </p>
        </div>
      </div>
    </div>
  </section>
</template>

<style scoped>
/* 页头第二行：开关与它的效果说明。`.page-head` 是 space-between 的 flex 行，把开关和
   那段说明直接摆成它的第二、第三个子元素时，说明会被当成「右侧动作」，要么与开关并排
   挤成一列窄字，要么整行与标题抢宽度。收进一个 `flex: 1 0 100%` 的块里让它独占一行。 */
.memory-switch {
  display: flex;
  align-items: baseline;
  gap: 12px;
  flex: 1 0 100%;
}

.memory-switch__status {
  flex: 1 1 auto;
  min-width: 0;
}

.memory-form {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

/* 正文输入框与它的字节计数器是一件事：计数器贴着输入框（4px），与下一个字段之间走
   表单的 10px。不留这一层时两条缝都是 10 + 4 = 14px，计数器读起来像「关键词」那一栏
   的前置说明。 */
.memory-body {
  display: flex;
  flex-direction: column;
  gap: 4px;
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

/* 卡头放不下时标题与按钮折成两行，折下去的那一行靠右——`space-between` 只管同一行内
   的两端，单独落一行时它会默认贴左。 */
.memory-head__actions {
  display: flex;
  gap: 4px;
  margin-left: auto;
}

.memory-project__title {
  margin-right: 8px;
}

.memory-project__path {
  color: var(--text-muted);
  font-size: 12px;
}

/* 说明文字与卡里其它灰字同档；块之间的缝由 `.card` 的 gap 给，这里不再自带外边距。 */
.memory-hint {
  margin: 0;
  color: var(--text-secondary);
  font-size: 13px;
  overflow-wrap: anywhere;
}

/* 卡内一整段灰字（字节计数器、最近注入、提取任务、记录文件）。此前这几处借的是
   `memory-row__meta`——那是**行内次行**，自带 4px 上外边距，在 `.card` 的 10px gap
   之上又叠一层，同一列里于是出现 10px 与 14px 两种缝。 */
.memory-note {
  margin: 0;
  font-size: 12px;
  color: var(--text-muted);
  overflow-wrap: anywhere;
}

/* 正文超过字节上限时给一个字色信号：此时保存按钮已经禁用（`memory.js::canSaveDraft`），
   而计数器原先只是一段 12px 的灰色小字，读起来像「一切正常」。选 `--danger-strong`
   而不是 `--danger`：12px 正文的对比度门槛是 4.5:1，浅色下 `--danger` 压在白卡上只有
   3.68:1，`--danger-strong` 是 5.14:1（暗色那两档是 5.53 与 7 上下）。 */
.memory-note.memory-over-limit {
  color: var(--danger-strong);
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
