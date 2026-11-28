<script setup>
import { onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { InfoFilled } from '@element-plus/icons-vue';
import { AppWindow as AppWindowIcon, ArrowUpRight as ArrowUpRightIcon } from '@lucide/vue';
import { isLoading } from './loading.js';
import {
  activeSession, activeSessionPoppedOut, visibleSessions,
  hideSessionSlot, selectSessionSurface, syncSessionSlot, toggleSessionPlacement, SESSION_PLACEMENT_LOADING,
} from './sessionSurfaces.js';

const PAGE_DESC = '在管理面板中打开会话，也可以弹出为独立窗口，再归位到面板。';
const slot = ref(null);
let resizeObserver;
let overlayObserver;

function syncSlot() {
  syncSessionSlot(slot.value);
}

watch([activeSession, activeSessionPoppedOut], syncSlot, { flush: 'post' });

onMounted(() => {
  resizeObserver = new ResizeObserver(syncSlot);
  resizeObserver.observe(slot.value);
  // 全局对话框、提示与下拉菜单会挂到 body，不一定是面板的子节点。
  overlayObserver = new MutationObserver(syncSlot);
  overlayObserver.observe(document.body, {
    subtree: true, childList: true, attributes: true, attributeFilter: ['class', 'style', 'hidden'],
  });
  window.addEventListener('resize', syncSlot);
  document.addEventListener('visibilitychange', syncSlot);
  syncSlot();
});

onBeforeUnmount(() => {
  resizeObserver?.disconnect();
  overlayObserver?.disconnect();
  window.removeEventListener('resize', syncSlot);
  document.removeEventListener('visibilitychange', syncSlot);
  hideSessionSlot();
});
</script>

<template>
  <section class="session-panel" aria-label="会话">
    <header class="session-toolbar">
      <div class="session-picker" role="group" aria-label="已打开的会话">
        <button
          v-for="session in visibleSessions"
          :key="session.kind"
          type="button"
          class="btn btn--ghost"
          :aria-pressed="session.kind === activeSession?.kind"
          @click="selectSessionSurface(session.kind)"
        >
          {{ session.label }}
        </button>
        <span v-if="!activeSession" class="session-toolbar__label">会话</span>
        <el-tooltip :content="PAGE_DESC" placement="bottom-start" :show-after="80">
          <el-icon class="head-tip-icon" tabindex="0" role="note" :aria-label="PAGE_DESC"><InfoFilled /></el-icon>
        </el-tooltip>
      </div>
      <el-button
        v-if="activeSession"
        class="icon-btn"
        :title="activeSessionPoppedOut ? '归位到面板' : '弹出为窗口'"
        :aria-label="activeSessionPoppedOut ? '归位到面板' : '弹出为窗口'"
        :loading="isLoading(SESSION_PLACEMENT_LOADING)"
        @click="toggleSessionPlacement"
      >
        <AppWindowIcon v-if="activeSessionPoppedOut" :size="16" aria-hidden="true" />
        <ArrowUpRightIcon v-else :size="16" aria-hidden="true" />
      </el-button>
    </header>

    <div class="session-content">
      <div ref="slot" class="session-slot"></div>
      <p v-if="!activeSession" class="session-hint">暂无会话，请在概览中打开工作台或官网网页版。</p>
      <p v-else-if="activeSessionPoppedOut" class="session-hint">{{ activeSession.label }}正在独立窗口中运行。</p>
    </div>
  </section>
</template>

<style scoped>
.session-panel {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-width: 0;
  min-height: 0;
}

.session-toolbar {
  display: flex;
  flex: none;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  min-height: 40px;
  padding: 5px 12px;
  border-bottom: 1px solid var(--border);
}

.session-toolbar > .icon-btn {
  flex: none;
}

.session-picker {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 4px 6px;
  min-width: 0;
}

.session-toolbar__label {
  font-size: var(--fs-5);
  font-weight: 600;
}

.session-picker [aria-pressed="true"] {
  background: var(--accent-fill);
  border-color: var(--accent-line);
  color: var(--accent-strong);
}

.session-content {
  position: relative;
  flex: 1;
  min-height: 0;
}

.session-slot {
  position: absolute;
  inset: 0;
}

.session-hint {
  position: absolute;
  inset: 0;
  display: grid;
  place-items: center;
  margin: 0;
  padding: 16px;
  text-align: center;
  color: var(--text-secondary);
  font-size: var(--fs-5);
}
</style>
