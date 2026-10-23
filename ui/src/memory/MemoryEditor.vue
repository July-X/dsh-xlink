<script setup>
// 记忆编辑器：**新建与编辑共用这一个弹窗**。
//
// 两种模式只差标题与草稿来源（`openMemoryEditor(record)` 填草稿），字段与校验是同一份。
// 分成两个界面迟早一边少字段、一边少校验。
//
// **为什么是一个独立组件，不是面板里的一段模板**：① 仓库里五个弹窗
// （`PrecheckDialog` / `SnapshotRestoreDialog` / `IncidentModal` / `LogModal` /
// `MigrationPrompt`）都是独立组件，弹窗有自己的生命周期（开关、关闭时丢弃草稿）；
// ② `MemoryPanel.vue` 的生产代码贴着 495 行预算，表单（模板 + 样式约 60 行）留在里面
// 就得连预算一起抬，而它本该是「读」的界面、编辑器是「写」的界面；
// ③ 字段与动作的判据可以对着这一个文件写，不必再从面板模板里切区间。
//
// 状态都在 `memory.js`：可见性是 `memoryStore.editorVisible`，开关走
// `openMemoryEditor` / `closeMemoryEditor`——与 `PrecheckDialog` 的
// `precheckVisible`、`SnapshotRestoreDialog` 的 `restoreVisible` 同一个约定。
import { computed, onUnmounted } from 'vue';
import { globalBusy, isLoading } from '../shell/loading.js';
import { confirmDialog } from '../shell/notify.js';
import {
  memoryStore,
  saveMemory,
  closeMemoryEditor,
  rememberProjectPath,
  canSaveDraft,
  byteLength,
  MAX_BODY_BYTES,
} from './memory.js';

const draft = computed(() => memoryStore.draft);
const savable = computed(() => canSaveDraft());
/// 保存**在飞**。这是关窗闸门该用的信号：`globalBusy` 只由 `withExclusive` 抬，
/// 而保存走的是 `withLoading('memorySave')`——保存期间它恒为 false，
/// 拿它当闸门等于没闸门（关掉装着刚写完正文的弹窗，等于把那段文字拿走）。
const saving = computed(() => isLoading('memorySave'));
/// 正文字节数：与后端 `MAX_BODY_BYTES` 同一个数，**按字节**算（中文一个字 3 字节）。
const bodyBytes = computed(() => byteLength(draft.value.body));
const kindName = computed(() => draft.value.kind === 'handoff' ? '交接' : draft.value.kind === 'experience' ? '经验' : '记忆');
const experienceFields = [{ key: 'situation', label: '适用情境' }, { key: 'action', label: '实际做法' }, { key: 'outcome', label: '观察到的结果（包括失败）' }, { key: 'lesson', label: '可复用教训' }];
const handoffLists = [{ key: 'doneText', label: '已完成' }, { key: 'todoText', label: '待办' }, { key: 'blockedText', label: '阻塞' }];

/// ✕ 关窗前的闸门。遮罩与 ESC 已由 prop 拦住，剩下这一条是**明确的**点击——但「明确」
/// 说的是意图，不是「他愿意丢掉刚写的三段话」。有内容就问一句，空表单直接关。
/// 用 `before-close` 而不是 `@update:model-value`：后者在 EP 里是「已经关了再通知你」
/// （`handleClose` 先置 `visible = false` 再 emit），拿它做确认框只能假装拦一下。
///
/// **关的那一下只走 `closeMemoryEditor()`，不接 EP 递进来的 `done`**（用户 2026-10-16 报
/// 「切走菜单回来弹窗自己又弹出来」）：`done` 就是 EP 的 `hide`，它只把 EP 内部那份
/// `visible` 置 false（`use-dialog.mjs::handleClose`），store 里的 `editorVisible` 仍是
/// true。而面板在 `App.vue` 里是 `<component :key="activePanel">`——切走再切回记忆页会
/// 重新挂载本组件，EP 见 `modelValue` 还是 true，就在 `onMounted` 里自己 `open()` 一次，
/// 弹窗凭空回来，正是「错误地绑定」的形态。反过来，store 置 false 时 EP 的
/// `watch(() => props.modelValue)` 会走 `doClose()`，动的还是同一个 `visible`，
/// 画面与 `done` 完全一致（`handleClose` 在调 beforeClose 之前不置任何状态；`dialog.vue`
/// 全程没读 `closed`），于是 store 成了唯一的写者，两边不会再分家。
function beforeClose() {
  if (!['title', 'body', 'situation', 'action', 'outcome', 'lesson', 'goal', 'doneText', 'todoText', 'blockedText'].some((key) => draft.value[key]?.trim())) {
    closeMemoryEditor();
    return;
  }
  confirmDialog(`放弃这条${kindName.value}？`, '弹窗里的内容还没保存，关掉就没了。', '放弃').then((ok) => {
    if (ok) closeMemoryEditor();
  });
}

/// 可见性不许活过这个组件：切走菜单时 `MemoryPanel` 连同本组件一起卸载（`mode="out-in"`
/// 的 `:key` 换的就是它），store 里的 `editorVisible` 却不会自己回 false——留着这个 true，
/// 下次挂载 EP 就照着它再弹一次。收尾只有 `closeMemoryEditor()` 一份实现，这里不另写。
onUnmounted(() => { if (memoryStore.editorVisible) closeMemoryEditor(); });
</script>

<template>
  <!--
    宽度必须显式给：EP 的 `.el-dialog` 默认 `width: 50%`，在 1040px 的窗口里会得到一个
    520px 的框，而这个表单里最长的输入是项目路径。`append-to-body` 与其余五个弹窗一致
    ——面板的祖先带 `overflow`，就地渲染会被裁掉。
    **遮罩点击与 ESC 都不关**：这里装着用户刚写的正文，两者都是「手滑」的路径，而关掉
    就等于丢（草稿没有历史、恢复不了）。要放弃就按 ✕ 或「取消」——那两次点击是明确的。
    只拦一半（拦遮罩、放 ESC）等于把同一条理由兑现一半。
  -->
  <el-dialog
    class="memory-dialog"
    :model-value="memoryStore.editorVisible"
    :title="(draft.id ? '编辑' : '新建') + kindName"
    width="min(560px, 92vw)"
    append-to-body
    :close-on-click-modal="false"
    :close-on-press-escape="false"
    :show-close="!saving"
    :before-close="beforeClose"
  >
    <div class="memory-form">
      <!-- 范围档叫「项目记忆」，不跟着「当前项目」命名：下面那个下拉让用户**任选**一个工作区
           路径（也能手填目录），选中的未必是他此刻所在的项目；而列表筛选那一档「按项目」列的
           是**所有**项目范围的记忆。同一屏里两个名字指同一件事是 ui/AGENTS.md 禁的形状
           （用户 2026-10-18 要求消歧义）。详情弹窗的范围文案同属这套叫法。 -->
      <el-radio-group v-model="draft.scope" :disabled="globalBusy || draft.kind === 'handoff'">
        <el-radio value="project">项目记忆</el-radio>
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
      <div v-if="draft.kind !== 'experience'" class="memory-body">
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
      <template v-if="draft.kind === 'experience'">
        <p class="memory-note">通用经验会用于所有项目。保存前确认情境和正文不含项目专有路径、客户名或凭据；项目专用做法请选项目范围。</p>
        <label v-for="field in experienceFields" :key="field.key" class="memory-body">
          <span>{{ field.label }}（{{ byteLength(draft[field.key]) }} / 2048 字节）</span>
          <el-input v-model="draft[field.key]" type="textarea" :rows="2" :aria-label="field.label" />
        </label>
      </template>
      <template v-if="draft.kind === 'handoff'">
        <label class="memory-body">
          <span>目标（{{ byteLength(draft.goal) }} / 2048 字节）</span>
          <el-input v-model="draft.goal" type="textarea" :rows="2" aria-label="交接目标" />
        </label>
        <label v-for="field in handoffLists" :key="field.key" class="memory-body">
          <span>{{ field.label }}（每行一条，最多 32 条，每条 512 字节）</span>
          <el-input v-model="draft[field.key]" type="textarea" :rows="2" :aria-label="field.label" />
        </label>
      </template>
      <!-- 判据是 `memory/query.rs::score`：查询串里**包含**这个词就 +100（不切词、不分大小写），
           而检索用的查询就是用户这一步说过的话（`plugins/memory/host/index.js` 的
           `userText(messages)`）。「用于命中检索」是内部说法，用户拿它对照不了自己的话；
           换成一句他能自己验的（用户 2026-10-18 要求消歧义）。 -->
      <el-input
        v-model="draft.keywordsText"
        placeholder="关键词，用逗号分隔（用户的话里出现这些词时，这条会被优先挑中）"
      />
      <!-- 「优先带上」不是「一定带上」：固定项只影响排序，而注入有**条数上限**
           （`query.rs::MAX_HITS`），固定项与命中项一起占那几个名额——pin 超过上限时排在
           最后的那条固定项进不去。另外项目范围的固定项只在它自己那个工作区里生效
           （`query.rs::scope_matches_fields`）。写「每次都带上」是把有条件的事说成无条件。 -->
      <el-checkbox v-if="draft.kind === 'memory'" v-model="draft.pinned" :disabled="globalBusy">固定项（优先带上）</el-checkbox>
      <!--
        验证范围**不是**置信分数（§6.1）：只三档。给「已过期」一个入口，是因为
        一条过时的技术事实既不该继续当权威，也不该被悄悄删掉——标出来，让它排在
        检索结果的后面，用户自己决定什么时候重验或删掉。
      -->
      <!-- 三档的名字**必须与注入给模型的说法逐字相同**（`query.rs::verification_label` 与
           `memory.js::verificationText` 是一套词）：模型眼里叫「已过期」、面板上叫别的，
           用户就没法拿面板上的话解释「它为什么照做了一条我自己标过期的约定」。
           括号只补在「已验证」上——它的默认值是用户自己写下或批准候选时给的
           （`memory/input.rs` 的手动默认、批准候选那条路），不点明会被读成「系统验过」；
           另外两档自身已经说清。三档一起加括号会把这一排顶成两行：弹窗 560px 减两侧 16px
           内边距只放得下约 26 个汉字（用户 2026-10-18 要求消歧义）。 -->
      <el-radio-group v-if="draft.kind !== 'handoff'" v-model="draft.verification" :disabled="globalBusy">
        <el-radio value="verified">已验证（你确认过）</el-radio>
        <el-radio value="unverified">未验证</el-radio>
        <el-radio value="stale">已过期（事实可能过时）</el-radio>
      </el-radio-group>
    </div>
    <!-- 动作排在 `#footer` 里：EP 的 footer 本来就右对齐，不必挂 `.dialog-actions`。 -->
    <template #footer>
      <el-button :disabled="saving" @click="closeMemoryEditor">取消</el-button>
      <el-button
        type="primary"
        :loading="isLoading('memorySave')"
        :disabled="!savable || globalBusy"
        @click="saveMemory"
      >
        保存
      </el-button>
    </template>
  </el-dialog>
</template>

<style scoped>
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

/* 项目下拉与表单里其它输入同宽；选项里标题与路径各占一行（路径可能很长，
   挤在一行会把标题挤没）。 */
.memory-project {
  width: 100%;
}

.memory-project__title {
  margin-right: 8px;
}

.memory-project__path {
  color: var(--text-muted);
  font-size: 12px;
}
</style>
