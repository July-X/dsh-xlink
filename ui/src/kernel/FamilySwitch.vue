<script setup>
import { computed, nextTick, ref, watch } from 'vue';
import { globalBusy, isLoading } from '../shell/loading.js';
import { memoryStore } from '../memory/memory.js';
import { instanceStore, loadKernelFamily } from './instance.js';

// 内核族切换器（受控组件）。
//
// **它不再是页面级页签**：内核维度是那几个与内核指向性绑定的控件的属性，不是
// 整页的属性。用量分析、插件、技能、设置这些页的数据与内核无关——让整页跟着
// 内核翻，等于把「公共数据」画成「另一个内核的数据」。
//
// 选中值由页面持有：三个绑定页各有一份（`instance.js` 的 pageFamilies），在概览
// 切内核不会动记忆页和内核版本页。落盘、并发与错误状态仍走 instanceStore。
//
// 形状是分段控件而不是页签条：页签条读起来是「下面这一页换内容」，这里是
// 「这一页里这几张卡换数据源」。
const MCODE_EXPERIMENTAL = import.meta.env.VITE_MCODE_EXPERIMENTAL === '1';

const props = defineProps({
  // 本页当前选中的内核族。
  modelValue: { type: String, required: true },
});
const emit = defineEmits(['update:modelValue']);

const options = computed(() =>
  MCODE_EXPERIMENTAL
    ? [
        { id: 'dsh', label: 'DSH' },
        { id: 'mcode', label: 'MiniMax', experimental: true },
      ]
    : []
);

const disabled = computed(
  () =>
    globalBusy ||
    memoryStore.editorVisible ||
    isLoading('memorySave') ||
    !instanceStore.familyLoaded ||
    instanceStore.familySwitching ||
    !!instanceStore.familyError,
);

const tabsEl = ref(null);

// 方向键在两个内核之间移动（分段控件的键盘形态：左右键换选中项，焦点跟着走）。
function move(event) {
  if (!['ArrowLeft', 'ArrowRight'].includes(event.key)) return;
  event.preventDefault();
  if (disabled.value) return;
  const index = options.value.findIndex((item) => item.id === props.modelValue);
  const step = event.key === 'ArrowRight' ? 1 : -1;
  const target = options.value[(index + step + options.value.length) % options.value.length];
  if (target) pick(target.id);
}

function pick(id) {
  if (disabled.value) return;
  emit('update:modelValue', id);
}

// 切换成功后把焦点跟到新选中项；切换失败时 modelValue 不变，焦点原地不动。
watch(
  () => props.modelValue,
  async () => {
    await nextTick();
    tabsEl.value?.querySelector('[aria-selected="true"]')?.focus();
  }
);
</script>

<template>
  <div v-if="options.length > 1" class="family-switch">
    <div ref="tabsEl" role="tablist" aria-label="选择内核" class="family-switch__options" @keydown="move($event)">
      <button
        v-for="item in options"
        :key="item.id"
        type="button"
        role="tab"
        class="family-switch__option"
        :aria-selected="modelValue === item.id"
        :disabled="disabled"
        @click="pick(item.id)"
      >{{ item.label }}<span v-if="item.experimental" class="family-switch__badge">实验</span></button>
    </div>
    <span v-if="instanceStore.familyError" role="alert" class="family-switch__error">
      {{ instanceStore.familyError }}
      <button type="button" class="family-switch__retry" @click="loadKernelFamily">重试</button>
    </span>
  </div>
</template>

<style scoped>
/* 分段控件：圆角药丸，不画页签条那种下划线——下划线会让人读成
   「下面这一页换内容」。 */
.family-switch { display: inline-flex; align-items: center; gap: 8px; }
.family-switch__options { display: inline-flex; gap: 2px; padding: 2px; border: 1px solid var(--border); border-radius: 6px; }
.family-switch__option {
  border: 0; border-radius: 4px; padding: 3px 10px;
  background: transparent; color: var(--text-secondary);
  font-size: var(--fs-5); line-height: 1.5; cursor: pointer;
}
.family-switch__option[aria-selected="true"] { background: var(--accent-fill); color: var(--accent-strong); }
.family-switch__option:focus-visible { outline: 2px solid var(--accent); outline-offset: 1px; }
.family-switch__option:disabled { opacity: .55; cursor: not-allowed; }
/* MiniMax 那一路还没验收完（见接入计划的验收表），挂一枚徽记，
   别让人把它当成已交付的内核。颜色走 warning 语义档。 */
.family-switch__badge {
  margin-left: 6px; padding: 1px 6px;
  border: 1px solid var(--warning-line); border-radius: 4px;
  background: var(--warning-fill); color: var(--warning);
  font-size: var(--fs-3); line-height: 1.5; vertical-align: middle;
}
.family-switch__error { color: var(--danger); font-size: var(--fs-4); }
.family-switch__retry {
  border: 0; padding: 0 4px; margin-left: 4px;
  background: transparent; color: inherit;
  font-size: inherit; text-decoration: underline; cursor: pointer;
}
</style>
