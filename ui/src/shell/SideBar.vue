<script setup>
// 侧栏：品牌区 + 三组导航（工作台 / 资源 / 系统）+ 可收起 + 底部工具区
// （版本徽标、更新、主题开关）。
//
// 分组是按「用户为什么来这里」分的，不是按模块大小：概览回答「现在什么状态」，
// 内核版本 / 插件 / 技能回答「往里装了什么」，设置与数据迁移是「改行为」。
// 收起态只留 icon 列——宽窗 1040 下 224px 侧栏够放，但内容多的用户会想把
// 横向空间还给主区，此时仍要靠 icon + 悬停提示认得出每一项。
//
// 菜单激活项由 store.activePanel 驱动。第三方来源的安全提示不在这里：它是
// 插件页的语境提示，由 PluginsPanel 顶部渲染（见 theme.css 的 .panel-notice）。
import { computed, ref, watch } from 'vue';
import {
  Odometer,
  Box,
  Connection,
  MagicStick,
  SetUp,
  Refresh,
  Right,
  Fold,
  Expand,
  Moon,
  Sunny,
} from '@element-plus/icons-vue';
import { store, checkShellUpdate } from '../store.js';
import { globalBusy, isLoading } from './loading.js';
import { pluginStore } from '../plugins/plugins.js';
import { skillStore } from '../skills/skills.js';
import { migrationStore } from '../migration/migration.js';
import { theme, toggleTheme } from './theme.js';
import VersionBadge from './VersionBadge.vue';

const MENU_GROUPS = [
  {
    label: '工作台',
    items: [{ id: 'overview', label: '概览', icon: Odometer }],
  },
  {
    label: '资源',
    items: [
      { id: 'versions', label: '内核版本', icon: Box },
      {
        id: 'plugins',
        label: '插件',
        icon: Connection,
        badge: () => (pluginStore.view && pluginStore.view.updates) || 0,
      },
      {
        id: 'skills',
        label: '技能',
        icon: MagicStick,
        badge: () => (skillStore.view && skillStore.view.updates) || 0,
      },
    ],
  },
  {
    label: '系统',
    items: [
      { id: 'settings', label: '设置', icon: SetUp },
      // 「数据迁移」只在「从未迁移过 + 扫描到遗留数据」时显示：迁移设计是
      // 旧源永不删除，旧数据永远扫得到，只看 hasMigratable 会让迁移过的用户
      // 每次重启都看到入口。判断「迁移过」看历史记录（migration_list）非空。
      // 迁移过的用户要走重跑 / 回滚 / 历史，用设置页的「数据迁移」入口。
      {
        id: 'migration',
        label: '数据迁移',
        icon: Right,
        show: () =>
          migrationStore.hasMigratable === true && migrationStore.history.length === 0,
      },
    ],
  },
];

// 桌面端版本号：「（dev）」后缀是 release-only 钩子（dev 构建里标出来，
// 开了 release 预览就按正式版隐藏）。展示在侧栏底部工具区，概览卡不再重复
// 一行。不带「v」前缀（2026-10-05 用户要求）：后端给的就是纯版本号。
const shellVersionText = computed(() => {
  if (!store.view) return '';
  return store.view.shell_version + (store.devUi ? '（dev）' : '');
});

// 过滤掉 `show()` 返回 false 的菜单项——数据迁移默认隐藏，扫描到遗留数据才显示。
const visibleGroups = computed(() =>
  MENU_GROUPS.map((group) => ({
    ...group,
    items: group.items.filter((it) => (typeof it.show === 'function' ? it.show() : true)),
  })).filter((group) => group.items.length > 0)
);

// 收起状态存在 localStorage：窗口尺寸固定，收起是长期偏好而不是一次性操作，
// 每次启动都退回展开会让「收起」这件事失去意义。
const COLLAPSE_KEY = 'dsh-xlink:sidebar-collapsed';
const collapsed = ref(readCollapsed());

function readCollapsed() {
  try {
    return window.localStorage?.getItem(COLLAPSE_KEY) === '1';
  } catch {
    return false;
  }
}

function toggleCollapsed() {
  collapsed.value = !collapsed.value;
}

// 写盘放在 watch 里而不是 toggle 里：将来若别处也要能改收起态（设置项、
// 快捷键），只有一处 watch 才会把持久化补上。
watch(collapsed, (value) => {
  try {
    window.localStorage?.setItem(COLLAPSE_KEY, value ? '1' : '0');
  } catch {
    // 存不下去只丢持久化，本次会话的收起照样生效。
  }
});

const themeLabel = computed(() => (theme.value === 'dark' ? '深色' : '浅色'));
</script>

<template>
  <aside class="sidebar" :class="{ 'is-collapsed': collapsed }">
    <div class="brand">
      <img src="/whale-icon.png" alt="" width="28" height="28" />
      <div class="brand__text">
        <div class="brand__name">Dsh-Xlink</div>
        <div class="brand__desc">DeepSeek 内核桌面管理端</div>
      </div>
    </div>

    <nav class="sidebar__nav" aria-label="主菜单">
      <div v-for="group in visibleGroups" :key="group.label" class="sidebar__section">
        <div class="sidebar__section-label">{{ group.label }}</div>
        <button
          v-for="item in group.items"
          :key="item.id"
          type="button"
          class="nav-item"
          :class="{ 'is-active': store.activePanel === item.id }"
          :title="collapsed ? item.label : undefined"
          @click="store.activePanel = item.id"
        >
          <el-icon><component :is="item.icon" /></el-icon>
          <span class="nav-item__label">{{ item.label }}</span>
          <!-- 只显示数量：具体含义收进悬停提示，行内保持轻。 -->
          <span
            v-if="item.badge && item.badge() > 0"
            class="nav-item__badge"
            :title="item.label + '有 ' + item.badge() + ' 个可更新'"
          >{{ item.badge() > 99 ? '99+' : item.badge() }}</span>
        </button>
      </div>
    </nav>

    <div class="rail-toggle">
      <button
        type="button"
        class="btn btn--ghost btn--icon"
        :title="collapsed ? '展开侧栏' : '收起侧栏'"
        @click="toggleCollapsed"
      >
        <el-icon><component :is="collapsed ? Expand : Fold" /></el-icon>
      </button>
    </div>

    <div class="sidebar__footer">
      <!-- 桌面端自更新检查入口：业务逻辑不变，仍走 checkShellUpdate(true)，
           发现新版本时在概览页横幅里安装。 -->
      <button
        type="button"
        class="theme-switch"
        title="检查桌面端更新"
        :disabled="globalBusy"
        @click="checkShellUpdate(true)"
      >
        <el-icon><Refresh /></el-icon>
        <span class="nav-item__label">检查更新</span>
        <span v-if="shellVersionText" class="brand-version">{{ shellVersionText }}</span>
      </button>
      <button
        type="button"
        class="theme-switch"
        :title="'切换到' + (theme === 'dark' ? '浅色' : '深色') + '主题'"
        @click="toggleTheme"
      >
        <el-icon><component :is="theme === 'dark' ? Moon : Sunny" /></el-icon>
        <span class="nav-item__label">{{ themeLabel }}主题</span>
      </button>
    </div>
  </aside>
</template>

<style scoped>
/* 版本号跟在「检查更新」右侧：它是这条按钮的附属信息，不另起一行，
   否则底部工具区在 224px 侧栏里要占三行。 */
.brand-version {
  margin-left: auto;
  color: var(--text-muted);
  font-size: 11px;
}
/* 两个底部按钮共用 .theme-switch 的排版（见 theme.css），但「检查更新」在
   拉取期间要给出禁用反馈——用 :disabled 而不是换成 loading 图标，避免它在
   侧栏里跳动。 */
.theme-switch:disabled {
  opacity: 0.5;
  cursor: default;
}
.sidebar.is-collapsed .brand-version {
  display: none;
}
</style>