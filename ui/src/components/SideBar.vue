<script setup>
// 侧栏：品牌区（logo + 桌面端版本 + 更新胶囊）+ 主菜单。菜单激活项由
// store.activePanel 驱动，切换带指示条与背景动效。内核运行状态胶囊已移到
// 概览「当前内核」卡内（品牌区不再展示）。
//
// 第三方来源的安全提示不在这里：它是插件页的语境提示，挂在全局侧栏会常驻
// 占位，现由 PluginsPanel 顶部渲染（见 theme.css 的 .panel-notice）。
import { computed } from 'vue';
import { Odometer, Box, Connection, MagicStick, SetUp, Refresh, Right } from '@element-plus/icons-vue';
import { store, checkShellUpdate } from '../store.js';
import { globalBusy, isLoading } from '../loading.js';
import { pluginStore } from '../plugins.js';
import { skillStore } from '../skills.js';
import { migrationStore } from '../migration.js';

const MENU = [
  { id: 'overview', label: '概览', icon: Odometer },
  { id: 'versions', label: '内核版本', icon: Box },
  { id: 'plugins', label: '插件', icon: Connection, badge: () => (pluginStore.view && pluginStore.view.updates) || 0 },
  { id: 'skills', label: '技能', icon: MagicStick, badge: () => (skillStore.view && skillStore.view.updates) || 0 },
  { id: 'settings', label: '设置', icon: SetUp },
  // 「数据迁移」只在「从未迁移过 + 扫描到遗留数据」时显示：迁移设计是
  // 旧源永不删除，旧数据永远扫得到，只看 hasMigratable 会让迁移过的用户
  // 每次重启都看到入口。判断「迁移过」看历史记录（migration_list）非空。
  // 迁移过的用户要走重跑 / 回滚 / 历史，用设置页的「数据迁移」入口。
  { id: 'migration', label: '数据迁移', icon: Right, show: () => migrationStore.hasMigratable === true && migrationStore.history.length === 0 },
];

// 桌面端版本号：「（dev）」后缀是 release-only 钩子（dev 构建里标出来，
// 开了 release 预览就按正式版隐藏）。展示在品牌区「更新」按钮旁，
// 概览卡不再重复一行。
const shellVersionText = computed(() => {
  if (!store.view) return '';
  return 'v' + store.view.shell_version + (store.devUi ? '（dev）' : '');
});

// 过滤掉 `show()` 返回 false 的菜单项——数据迁移默认隐藏，扫描到遗留
// 数据才显示。
const visibleMenu = computed(() =>
  MENU.filter((it) => (typeof it.show === 'function' ? it.show() : true))
);
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
      <!-- 桌面端自更新检查入口（原概览页按钮）：业务逻辑不变，仍走
           checkShellUpdate(true)，发现新版本时在概览页横幅里安装。 -->
      <div class="brand-actions">
        <span class="brand-version" :title="'桌面端版本 ' + (shellVersionText || '未知')">
          {{ shellVersionText || '…' }}
        </span>
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
    </div>

    <nav class="menu" aria-label="主菜单">
      <button
        v-for="item in visibleMenu"
        :key="item.id"
        type="button"
        class="menu-item"
        :class="{ active: store.activePanel === item.id }"
        @click="store.activePanel = item.id"
      >
        <el-icon><component :is="item.icon" /></el-icon>
        <span>{{ item.label }}</span>
        <!-- 只显示数量：具体含义收进悬停提示，侧栏行内保持轻。 -->
        <span
          v-if="item.badge && item.badge() > 0"
          class="menu-badge"
          :title="item.label + '有 ' + item.badge() + ' 个可更新'"
        >{{ item.badge() > 99 ? '99+' : item.badge() }}</span>
      </button>
    </nav>
  </aside>
</template>

<style scoped>
/* 版本号 + 「更新」胶囊按钮上下堆叠（真机反馈横向一排太宽）。 */
.brand-actions {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 3px;
  margin-left: auto;
}
/* 品牌区的桌面端版本号：muted 小字，替代概览卡的版本行。 */
.brand-version {
  font-size: 11px;
  color: var(--muted);
  white-space: nowrap;
  line-height: 1;
}
/* 「更新」胶囊：描边 + 半透明底，与状态胶囊同一视觉语言。 */
.brand-update {
  height: auto;
  padding: 3px 10px;
  border: 1px solid var(--border);
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.06);
}
</style>
