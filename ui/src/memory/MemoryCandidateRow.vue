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
// 动作直接落在 `memory.js` 的 store 动作上（与 `MemoryEditor.vue` 同一写法），
// 不往面板再转一层事件：这三枚按钮就是这一行自己的语义。
import MemoryConflict from './MemoryConflict.vue';
import MemorySuperseded from './MemorySuperseded.vue';
import { globalBusy, isLoading } from '../shell/loading.js';
import {
  decideCandidate,
  removeMemory,
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
</script>

<template>
  <div class="page-row memory-row memory-row--candidate">
    <div class="memory-row__main">
      <div class="memory-row__title">
        <span class="memory-row__name">{{ item.title || '（无标题）' }}</span>
        <el-tag size="small" effect="plain">{{ sourceText(item.source.kind) }}</el-tag>
        <!-- H7：来源变了（会话被压缩/编辑之后重提同一段）。**只是提醒**——
             「保存」与「丢弃」两个按钮照常在下面，标过期不是替用户处理掉。 -->
        <el-tag v-if="item.verification === 'stale'" size="small" type="warning" effect="plain">
          可能已过期
        </el-tag>
      </div>
      <p class="memory-row__body memory-row__body--clamp">{{ item.body }}</p>
      <p class="memory-row__meta">
        来自 {{ item.source.session || '本机' }} · {{ shortTime(item.created_at) }}
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
</template>
