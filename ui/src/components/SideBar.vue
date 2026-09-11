<script setup>
// 侧栏：品牌区（logo + 状态胶囊）+ 主菜单。菜单激活项由 store.activePanel
// 驱动，切换带指示条与背景动效。
//
// 第三方来源的安全提示不在这里：它是插件页的语境提示，挂在全局侧栏会常驻
// 占位，现由 PluginsPanel 顶部渲染（见 theme.css 的 .panel-notice）。
import { computed } from 'vue';
import { Odometer, Box, Connection, MagicStick, SetUp, Refresh } from '@element-plus/icons-vue';
import { store, checkShellUpdate } from '../store.js';
import { globalBusy, isLoading } from '../loading.js';
import { pluginStore } from '../plugins.js';
import { skillStore } from '../skills.js';

const MENU = [
  { id: 'overview', label: '概览', icon: Odometer },
  { id: 'versions', label: '内核版本', icon: Box },
  { id: 'plugins', label: '插件', icon: Connection, badge: () => (pluginStore.view && pluginStore.view.updates) || 0 },
  { id: 'skills', label: '技能', icon: MagicStick, badge: () => (skillStore.view && skillStore.view.updates) || 0 },
  { id: 'settings', label: '设置', icon: SetUp },
];

const status = computed(() => {
  const k = store.view && store.view.kernel;
  if (!k) return { text: '加载中…', cls: '' };
  if (k.running) return { text: '运行中', cls: 'ok' };
  if (k.active && k.active_installed) return { text: '已停止', cls: 'bad' };
  return { text: '未安装', cls: '' };
});
</script>

<template>
  <aside class="sidebar">
    <div class="brand">
      <img src="/whale-icon.png" alt="" width="64" height="64" />
      <div>
        <h2>DeepSeek</h2>
        <p>Harness</p>
        <p class="subtitle">桌面管理台</p>
      </div>
      <div class="status-pill">
        <span class="dot" :class="status.cls"></span>
        <span>{{ status.text }}</span>
      </div>
      <!-- 桌面端自更新检查入口（原概览页按钮）：业务逻辑不变，仍走
           checkShellUpdate(true)，发现新版本时在概览页横幅里安装。 -->
      <el-button
        class="brand-update"
        text
        size="small"
        :icon="Refresh"
        :loading="isLoading('checkShellUpdate')"
        :disabled="globalBusy"
         title="检查桌面端更新"
        @click="checkShellUpdate(true)"
      >
        更新
      </el-button>
    </div>

    <nav class="menu" aria-label="主菜单">
      <button
        v-for="item in MENU"
        :key="item.id"
        type="button"
        class="menu-item"
        :class="{ active: store.activePanel === item.id }"
        @click="store.activePanel = item.id"
      >
        <el-icon><component :is="item.icon" /></el-icon>
        <span>{{ item.label }}</span>
        <span v-if="item.badge && item.badge() > 0" class="menu-badge">{{ item.badge() }} 个更新</span>
      </button>
    </nav>
  </aside>
</template>
