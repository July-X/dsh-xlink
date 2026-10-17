<script setup>
// 记忆面板：保存 / 编辑 / 停用 / 删除本机的长期偏好与项目约定，
// 并在右边把「按当前输入会注入什么」摊开给用户看。
//
// 数据与动作全在 `memory.js` 里；这里只管画。字段名用后端下发的 snake_case
// （见该文件顶部的说明）。
import { computed, onMounted, ref } from 'vue';
import { Download, InfoFilled, Plus, Refresh } from '@element-plus/icons-vue';
import MemoryRow from './MemoryRow.vue';
import MemoryConflict from './MemoryConflict.vue';
import MemorySuperseded from './MemorySuperseded.vue';
import MemoryEditor from './MemoryEditor.vue';
import MemoryDetail from './MemoryDetail.vue';
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
  decideCandidate,
  previewContext,
  exportMemory,
  openExports,
  openMemoryEditor,
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
const recentJobs = computed(() => (memoryStore.jobs && memoryStore.jobs.jobs) || []);
const enabled = computed(() => !!(status.value && status.value.use_approved));

/// 注入链路的内置插件（`xlink-memory`）。**按 id 取，不取第一项**：
/// 取第一项会在登记表里多一个插件时静默指向别人。
const injectPlugin = computed(() => builtinById('memory'));

/// 打开注入插件。成功后回读一次状态（后端返回的那一项已经是真值，回读是为了
/// 与插件页共享的列表保持一致——两处各显示一份状态就会分叉）。
function enableInjection() {
  return toggleBuiltin('memory', true).then(() => loadBuiltinStatus());
}

/// 生效语义里**「什么时候才生效」的那半**，收进 ⓘ。`switchEffectText` 那句一个字没改
/// （判据钉着它），这里只是把同一事实的条件那半摆到 tooltip 上——行内那句跟在开关右侧、
/// 占页头第二行，三行长文把页面往下推，而「预算用尽要等新会话」是想问时才问的。 */
function switchEffectDetail(status) {
  if (status && status.effect_pending) {
    return '插件每一步都会问一次（通常几秒内）；本会话注入次数用尽时要等新会话。';
  }
  if (status && !status.effect_pending) {
    return '插件已经按这个开关来过一次，不需要再做什么。';
  }
  return '改动通常下一步检索就生效；本会话注入次数用尽时要等新会话。';
}

/// 详情弹窗记的是**行的 id**，不是那个行对象。`setMemoryState` / `removeMemory` 之后都会
/// 回读一次视图，攥着旧对象会把停用前的状态一直画在弹窗里；按住 id 重算，读到的就是刷新后
/// 的那一份，而记忆被删掉时它自己变成 `null`、弹窗随之关掉。
const detailId = ref(null);

/// 从**未筛选**的 `allMemories` 里取，不从 `list` 取：读着一条记忆时切「本项目 / 通用偏好」
/// 那个筛选，它会把当前这条从 `list` 里筛出去，弹窗跟着凭空消失。
const detailRow = computed(() => allMemories.value.find((row) => row.id === detailId.value) || null);

/// 列表行现在整块可点（卡片压到两行、正文截断），点开才是完整正文与操作。
const openMemoryDetail = (row) => { detailId.value = row.id; };

/// 从详情转去编辑器：两个弹窗都 `append-to-body`，叠着开就是两层遮罩，先关详情再开编辑器。
function editFromDetail(row) {
  detailId.value = null;
  openMemoryEditor(row);
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
        <!-- 生效语义写在这（E6）：**不许说「已生效」**——面板不知道本会话还剩几次注入。
             行内仍是 `switchEffectText(status)` 原文，ⓘ 里是同一事实的条件那半。 -->
        <p class="memory-note memory-switch__status">
          {{ switchEffectText(status) }}
          <el-tooltip placement="bottom-start" :show-after="80">
            <template #content>
              <div class="card-info-tooltip">{{ switchEffectDetail(status) }}</div>
            </template>
            <el-icon class="card-info-icon" tabindex="0" aria-label="这个开关什么时候生效">
              <InfoFilled />
            </el-icon>
          </el-tooltip>
        </p>
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
            <!--
              「新建记忆」是这一屏的主操作，所以它是卡头里唯一带底色的那一枚。
              **它必须是 `.card-head` 的直接子级、不能塞进 `.memory-head__actions`**：
              那一排已有三枚文字按钮（导出 / 清空 / 刷新，合计约 287px），而卡内可用宽
              只有 348px——四枚一起约 390px，`.memory-head__actions` 自己不换行、EP 的
              按钮又是 `white-space: nowrap`，多出来的 50px 会被 `main` 的
              `overflow-x: hidden` 裁掉，最右的「刷新」直接看不见（2026-10-16 审计量过）。
              单独占卡头的一个子级（排在标题后面），靠 `.card-head` 自己的 wrap 与标题同行、贴右缘。
              **括号不能省**：直接写方法名会把 MouseEvent 当成 record 传进去
              （`openMemoryEditor(record = null)` 的第一个形参），「新建」就成了
              「编辑一个事件对象」。
            -->
            <el-button type="primary" :icon="Plus" :disabled="globalBusy" @click="openMemoryEditor()">
              新建记忆
            </el-button>
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
            还没有记忆。点上面的「新建记忆」写一条，或者在工作台里让模型提出候选，再到「待确认」里点保存。
          </p>
          <!-- 库里有、只是当前筛选没有——这是**两回事**（记忆内核那次评审第 2 条）：
               说成「还没有记忆」是假话，而右栏状态行还写着 accepted: N。 -->
          <p v-else-if="!list.length" class="memory-hint">
            当前筛选下没有记忆（库里有 {{ allMemories.length }} 条，切到「全部」能看到）。
          </p>
          <!-- `memory-list` 是这一屏唯一的内部滚动容器：卡片压到两行之后列表仍会长过
               右栏，页面级滚动条在 2026-10-16 被去掉，长出来的这一截只能在这里自己滚。 -->
          <div v-else class="page-list memory-list">
            <MemoryRow
              v-for="row in list"
              :key="row.id"
              :row="row"
              @open="openMemoryDetail"
            />
          </div>
        </div>

        <div v-if="candidates.length" class="card">
          <div class="card-head">
            <h2>
              待确认（{{ candidates.length }}）
              <!-- 常驻说明收进 ⓘ（理由见「最近注入」那处）。判据要的那句仍不折行。 -->
              <el-tooltip placement="bottom-start" :show-after="80">
                <template #content>
                  <!-- prettier-ignore -->
                  <div class="card-info-tooltip">
                    这些是模型或会话提出的候选：正文与来源都摆在这里，只有你点保存才会成为记忆。<br />
                    要存成「所有项目都算」的通用偏好时用右边那个按钮——先确认正文里没有路径、客户名之类的私有信息，通用偏好会跟着你去别的项目。
                  </div>
                </template>
                <el-icon class="card-info-icon" tabindex="0" aria-label="待确认候选怎么处理">
                  <InfoFilled />
                </el-icon>
              </el-tooltip>
            </h2>
          </div>
          <div class="page-list">
            <div v-for="item in candidates" :key="item.id" class="page-row memory-row">
              <div class="memory-row__main">
                <div class="memory-row__title">
                  <span class="memory-row__name">{{ item.title || '（无标题）' }}</span>
                  <el-tag size="small" effect="plain">{{ item.source.kind }}</el-tag>
                  <!-- H7：来源变了（会话被压缩/编辑之后重提同一段）。**只是提醒**——
                       「保存」与「丢弃」两个按钮照常在下面，标过期不是替用户处理掉。 -->
                  <el-tag v-if="item.verification === 'stale'" size="small" type="warning" effect="plain">
                    可能已过期
                  </el-tag>
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
                <!-- C6：模型说这句推翻了先前提出的哪几条。**壳没有替用户丢弃任何一条**，
                     那些候选还在下面那张待确认清单里，由用户自己按一次。 -->
                <MemorySuperseded
                  :supersedes="item.supersedes || []"
                  :candidates="candidates"
                  :busy="globalBusy"
                  @discard="(candidate) => decideCandidate(candidate, false)"
                />
                <!-- H7：与上面两个标记**同一个 `.memory-flag` 盒子**——三种关系都要用户按一次，
                     看起来不一样重就会暗示哪一种更严重，而它们其实一样。措辞里不许出现
                     「已自动作废」：那是「只标不执行」，说成「已经替你决定了」就反了。 -->
                <div v-if="item.verification === 'stale'" class="memory-flag">
                  <p class="memory-flag__lead">
                    这条是从会话里<strong>同一段话</strong>提的，而那段内容在这次提取之前被改动过
                    （压缩或编辑都会）。系统没有替你处理掉它——要不要留，按下面的「保存」决定。
                  </p>
                </div>
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
            <h2>
              最近注入
              <!-- 常驻说明收进标题旁的 ⓘ（2026-10-16 用户要求「提示文字能做 tooltip 的都用
                   tooltip 收纳起来」「尽量不出现纵向滚动条」）：它每屏都读、读完不改变操作，
                   却常年占四行；触发图标 `tabindex="0"` + `aria-label`，键盘也够得到。 -->
              <el-tooltip placement="bottom-start" :show-after="80">
                <template #content>
                  <!-- prettier-ignore -->
                  <div class="card-info-tooltip">
                    实况：插件报告它真的把哪几条放进了模型上下文。<br />
                    下面那个「检索预览」只是查询词由你填，检索是真跑的（同一份记忆库、同一套打分）。<br />
                    壳重启后这份记录会清空（它不是权威记录，权威记录是上面那份文件）。
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
              {{ shortTime(note.at) }} · {{ baseName(note.workspace) || '未知项目' }} · {{ injectionText(note) }}
            </p>
            <p v-if="recentInjections.length > 5" class="memory-note">
              只列最近 5 次（共 {{ recentInjections.length }} 条记录）。
            </p>
          </div>
        </div>

        <div class="card">
          <div class="card-head">
            <h2>
              提取任务
              <!-- 常驻说明收进 ⓘ，同「最近注入」。判据要的那句「一条记忆都没提取出来时
                   先看这里」在 tooltip 里，且**不折行**（判据是 assert.match）。 -->
              <el-tooltip placement="bottom-start" :show-after="80">
                <template #content>
                  <!-- prettier-ignore -->
                  <div class="card-info-tooltip">
                    自动提取（P2）的任务表：聊完一轮后插件排一条、壳发一个租约、模型提完再交回来。<br />
                    <strong>一条记忆都没提取出来时先看这里</strong>——失败那行会写明是哪一步（`read` / `model` / `parse` / `submit`）。<br />
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
            <h2>检索预览</h2>
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
                ? '用上一次编辑过的标题与关键词当查询，看这次会注入哪几条。'
                : '先在编辑器里填标题或关键词，再回来点「试一次」。'
            }}
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
      v-if="detailRow"
      :row="detailRow"
      @close="detailId = null"
      @edit="editFromDetail"
      @toggle="(state) => setMemoryState(detailRow, state)"
      @remove="removeMemory"
    />
  </section>
</template>

<style scoped>
/* 页头第二行：开关与它的效果说明。`.page-head` 是 space-between 的 flex 行，把开关和
   那段说明直接摆成它的第二、第三个子元素时，说明会被当成「右侧动作」，要么与开关并排
   挤成一列窄字，要么整行与标题抢宽度。收进一个 `flex: 1 0 100%` 的块里让它独占一行。

   **这条 basis 100% 只在「能换行」的页头里才是「占第二行」**：theme.css 的
   `.page-head` 原本没有 `flex-wrap`，于是它反过来把 `.page-head__text` 压到
   min-content——标题与开关叠在一起、说明一个字一行竖着排（2026-10-16 用户截图）。
   现在 wrap 补在原语上（theme.css 那一节写了为什么该在原语层），判据在
   `ui/test/designAlignment.test.js` 的 `justify-content` 旁边。 */
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
  font-size: 13px;
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
  font-size: 12px;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}
</style>
