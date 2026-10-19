<script setup>
// 一条待确认候选：正文、来源、模型标出的冲突 / 取代关系，以及三枚动作。
//
// 2026-10-17 从 `MemoryPanel.vue` 抽出来，两个原因叠在一起：
// ① 候选从「另一张卡」并进了记忆卡的同一条列表视图——两张列表卡在 748px 高的窗口里
//    放不下，第二张卡把整页顶出纵向滚动条（用户截图指的就是那一段），面板要腾预算；
// ② 它与 `MemoryRow.vue` 是**两种角色**：那一行只读、点开才是操作，这一行自带
//    保存 / 存为通用偏好 / 忽略，还带两种关系标记。并成一个组件会让「默认精简、
//    点开再操作」那条规则失去唯一的形状。
//
// 2026-10-19 用户反馈「待确认的候选行正文被截成两行，点行没有任何反应」：这边补上
// 与记忆行相同的形状——**正文那半整块可点**，点开是 `MemoryCandidateDetail.vue`，
// 在那里读完整正文并直接裁决。可点的只有 `__main`：根还挂着动作排，把 `role="button"`
// 挂到根上会让「点忽略」也去开弹窗。
//
// 三枚动作 2026-10-19 抽进 `MemoryCandidateActions.vue`：行与详情弹窗都要裁决同一条
// 候选，各写一份迟早一边漏了 scope 或 loading key（静默）。行这边只保留动作排的
// **位置**（`.memory-row__actions` 让它沉到行底、贴右）。
import MemoryCandidateActions from './MemoryCandidateActions.vue';
import MemoryConflict from './MemoryConflict.vue';
import MemorySuperseded from './MemorySuperseded.vue';
import { globalBusy } from '../shell/loading.js';
import {
  decideCandidate,
  removeMemory,
  sessionText,
  setMemoryState,
  shortTime,
  sourceText,
} from './memory.js';

defineProps({
  item: { type: Object, required: true },
  /// 冲突的那几条要从**全库**（未筛选）解析 id → 标题与正文。
  records: { type: Array, default: () => [] },
  /// 取代的那几条是**候选**，不在记忆列表里。
  candidates: { type: Array, default: () => [] },
});
/// 只发「用户点了这一行」：动作（保存 / 忽略）由 `MemoryCandidateActions` 直接落在
/// store 上，行与面板都不需要知道点了什么。
const emit = defineEmits(['open']);

// 可点区域里还嵌着三个关系块的按钮（`MemoryConflict` 的停用 / 删除、
// `MemorySuperseded` 的丢弃这条），它们的事件会冒泡到这里——点它们时**不能顺手把
// 详情弹窗也打开**（一次点击做两件事）。这个闸门点击与回车共用。
//
// 模板里必须写成 `openFromEvent($event)`（显式传事件）：裸引用 `@click="openFromEvent"`
// 也会传 MouseEvent，但 `ui/test/clickHandlerArity.test.js` 把「裸引用 + 有形参」
// 一律当 bug（本仓栽过 `@click="showLogs"` 把 Event 当业务参数的形状），显式传参才
// 不被误判、也让读的人一眼看出这里要的是事件对象。
function openFromEvent(event) {
  if (event.target.closest('button, a')) return;
  emit('open', item);
}

/// 空格要 `preventDefault`（这一屏的滚动容器就是列表自己，不拦会先滚一屏），但只在
/// **自己身上**拦：关系块里按钮的空格激活同样冒泡到这里，一起拦掉的话键盘就点不动
/// 那三枚按钮了。先过闸门，再 prevent。
function openFromSpace(event) {
  if (event.target.closest('button, a')) return;
  event.preventDefault();
  emit('open', item);
}
</script>

<template>
  <div class="page-row memory-row memory-row--candidate">
    <!-- 可点的只有正文这一块（`memory-row--clickable` 提供 cursor / hover / 键盘焦点，
         与记忆行同一族规则），动作排是另一个语义，留在它外面。 -->
    <div
      class="memory-row__main memory-row--clickable"
      role="button"
      tabindex="0"
      aria-haspopup="dialog"
      :aria-label="`查看候选详情：${item.title || '（无标题）'}`"
      @click="openFromEvent($event)"
      @keydown.enter="openFromEvent($event)"
      @keydown.space="openFromSpace($event)"
    >
      <div class="memory-row__title">
        <span class="memory-row__name">{{ item.title || '（无标题）' }}</span>
        <el-tag size="small" effect="plain">{{ sourceText(item.source.kind) }}</el-tag>
        <!-- H7：**来源变了**（会话被压缩/编辑之后重提同一段）。**只是提醒**——
             「保存」与「忽略」两个按钮照常在下面，标记不是替用户处理掉。
             **标签不能写「已过期」**：那是记忆行上 `verificationText('stale')` 的意思
             （这条事实可能过时），而候选被批准时后端会把 verification 重置成 verified
             （`store.rs::decide_candidate`），两个 `stale` 一个词两个意思——按「事实过期」
             去理解会白担心一场。 -->
        <el-tag v-if="item.verification === 'stale'" size="small" type="warning" effect="plain">
          来源已变
        </el-tag>
      </div>
      <p class="memory-row__body memory-row__body--clamp">{{ item.body }}</p>
      <!-- 「来自 session-fbb5df89-a57a-4f93-a645-8bc184dfa4b4」这种原文没人读得下去，
           而设计 §2 又要求候选追得回是哪一次会话提的：短 id 在行里，完整 id 进 title。
           完整 id 在详情弹窗里也有一份（那一层就是拿来追查的）。 -->
      <p class="memory-row__meta" :title="item.source.session || ''">
        来自{{ sessionText(item.source.session) }} · {{ shortTime(item.created_at) }}
      </p>
      <!-- C5：并排摆出冲突的那几条，**由用户自己留哪一条**（不由模型裁决）。 -->
      <MemoryConflict
        :conflicts="item.conflicts_with || []"
        :records="records"
        :busy="globalBusy"
        @disable="(record) => setMemoryState(record, 'disabled')"
        @remove="removeMemory"
      />
      <!-- C6：模型说这句推翻了先前提出的哪几条。**壳没有替用户丢弃任何一条**，
           那些候选还在「待确认」那一档里，由用户自己按一次。 -->
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
    <MemoryCandidateActions class="memory-row__actions" :item="item" />
  </div>
</template>
