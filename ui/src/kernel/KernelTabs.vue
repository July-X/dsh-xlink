<script setup>
import { nextTick } from 'vue';
import { globalBusy, isLoading } from '../shell/loading.js';
import { memoryStore } from '../memory/memory.js';
import { instanceStore, loadKernelFamily, selectKernelFamily } from './instance.js';

async function move(event) {
  if (!['ArrowLeft', 'ArrowRight'].includes(event.key)) return;
  if (memoryStore.editorVisible || isLoading('memorySave')) return;
  event.preventDefault();
  const tabs = event.currentTarget;
  const target = instanceStore.family === 'dsh' ? 'mcode' : 'dsh';
  if (await selectKernelFamily(target)) {
    await nextTick();
    tabs.querySelector('[aria-selected="true"]')?.focus();
  }
}
</script>

<template>
  <div class="kernel-tabs">
    <div role="tablist" aria-label="选择工作台" @keydown="move($event)">
      <button v-for="item in [{ id: 'dsh', label: 'DSH' }, { id: 'mcode', label: 'MiniMax' }]"
        :key="item.id" type="button" role="tab" :aria-selected="instanceStore.family === item.id"
        :disabled="globalBusy || memoryStore.editorVisible || isLoading('memorySave') || !instanceStore.familyLoaded || instanceStore.familySwitching || !!instanceStore.familyError"
        @click="selectKernelFamily(item.id)">{{ item.label }}</button>
    </div>
    <div v-if="instanceStore.familyError" role="alert" class="kernel-tabs__error">
      {{ instanceStore.familyError }} <button type="button" @click="loadKernelFamily">重试</button>
    </div>
  </div>
</template>

<style scoped>
.kernel-tabs { padding: 8px 20px 0; }
.kernel-tabs [role="tablist"] { display: flex; gap: 8px; }
.kernel-tabs [role="tab"] { border: 0; border-bottom: 2px solid transparent; padding: 6px 12px; background: transparent; color: var(--text-secondary); cursor: pointer; }
.kernel-tabs [aria-selected="true"] { border-color: var(--accent); color: var(--text); }
.kernel-tabs [role="tab"]:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
.kernel-tabs [role="tab"]:disabled { opacity: .55; cursor: not-allowed; }
.kernel-tabs__error { color: var(--danger); font-size: var(--fs-4); }
</style>
