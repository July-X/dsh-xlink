<script setup>
// 侧栏：品牌区（logo + 桌面端版本 + 更新胶囊）+ 主菜单。菜单激活项由
// store.activePanel 驱动，切换带指示条与背景动效。内核运行状态胶囊已移到
// 概览「当前内核」卡内（品牌区不再展示）。
//
// 第三方来源的安全提示不在这里：它是插件页的语境提示，挂在全局侧栏会常驻
// 占位，现由 PluginsPanel 顶部渲染（见 theme.css 的 .panel-notice）。
import { computed } from 'vue';
import { Odometer, Box, Connection, MagicStick, SetUp, Refresh, Right } from '@element-plus/icons-vue';
// 桌面端版本号前的 tag 图标，与概览「当前内核」卡里那枚同一个 Lucide 图标——
// 两处都在说「这是一个版本号」，就该长成同一张脸。别名理由见 OverviewPanel。
import { Tag as TagIcon } from '@lucide/vue';
import { store, checkShellUpdate } from '../store.js';
import { globalBusy, isLoading } from './loading.js';
import { pluginStore } from '../plugins/plugins.js';
import { skillStore } from '../skills/skills.js';
import { migrationStore } from '../migration/migration.js';

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
        <!-- 「桌面管理台」与 Harness 同行而不是另起一行：它是副标不是第二行
             标题，摆成两行会让品牌区读起来像两个并列的名字（2026-10-03 用户
             要求）。仍然挂在 .subtitle 上，沿用全局 muted 小字样式；外面这层
             <p> 继续吃 .brand div > p:not(.subtitle) 的 14px/700，所以 Harness
             的字重字号一个字没动。窄窗（应用固定 480px）下两者约 118px，远小于
             品牌区可用宽度；宽布局（侧栏 208px）放不下时靠 flex-wrap 退回两行，
             与改动前一致，不会把 logo 挤下去。 -->
        <p class="brand-tagline">
          <span>Harness</span>
          <span class="subtitle">桌面管理台</span>
        </p>
      </div>
      <!-- 桌面端自更新检查入口（原概览页按钮）：业务逻辑不变，仍走
           checkShellUpdate(true)，发现新版本时在概览页横幅里安装。 -->
      <div class="brand-actions">
        <span class="brand-version" :title="'桌面端版本 ' + (shellVersionText || '未知')">
          <TagIcon :size="11" />
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
/* 品牌区第二行：「Harness」+ 副标「桌面管理台」同行。gap 6px 足以分开主副而
   不会让两者读成两个词组（更紧会像「Harness桌面管理台」是一个名字）。 */
.brand-tagline {
  display: flex;
  align-items: baseline;
  gap: 6px;
  /* 宽布局侧栏只有 208px，放不下时整体退回两行——这正是改动前的排法，
     降级而不是把 logo 顶走。 */
  flex-wrap: wrap;
}
/* 副标必须自己把字重压回常规：它继承外层 <p> 的 700，那是给 Harness 的。
   限定在 .brand-tagline 之内，不去动全局 .subtitle——那个类名还被
   MigrationPanel 的副标复用（见 theme.css 的 .subtitle）。 */
.brand-tagline .subtitle {
  font-weight: 400;
}
/* 版本号 + 「更新」胶囊按钮上下堆叠（真机反馈横向一排太宽）。 */
.brand-actions {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 3px;
  margin-left: auto;
}
/* 品牌区的桌面端版本号：muted 小字，替代概览卡的版本行。图标与文字同行居中，
   尺寸同样走 Lucide 的 size 属性——svg 带 width/height，font-size 缩不动它。 */
.brand-version {
  display: inline-flex;
  align-items: center;
  gap: 3px;
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
