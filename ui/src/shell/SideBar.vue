<script setup>
// 侧栏：品牌区 + 三组导航（工作台 / 资源 / 系统）+ 可收起 + 底部工具区
// （版本徽标、更新、主题开关）。
//
// 分组是按「用户为什么来这里」分的，不是按模块大小：概览回答「现在什么状态」，
// 内核版本 / 插件 / 技能回答「往里装了什么」，设置是「改行为」。
// 收起态只留 icon 列——宽窗 1040 下 224px 侧栏够放，但内容多的用户会想把
// 横向空间还给主区，此时仍要靠 icon + 悬停提示认得出每一项。
//
// 菜单激活项由 store.activePanel 驱动。第三方来源的安全提示不在这里：它是
// 插件页的语境提示，由 PluginsPanel 顶部渲染（见 theme.css 的 .panel-notice）。
import { computed, ref, watch } from 'vue';
import {
  Odometer, Refresh, Fold, Expand } from '@element-plus/icons-vue';
import { PAGE_ICONS } from './pageIcons.js';
import { store, checkShellUpdate } from '../store.js';
import { globalBusy } from './loading.js';
import { pluginStore } from '../plugins/plugins.js';
import { skillStore } from '../skills/skills.js';
import { theme, themeOptions, setTheme } from './theme.js';
// 图标一律别名导入：面板里同时有 `el-tag` 组件与一排 `*.tag` 类名，光看
// `<Tag />` 认不出说的是哪一个（见 AGENTS.md §图标）。
import { Sun as SunIcon, Moon as MoonIcon, Monitor as MonitorIcon, Tag as TagIcon } from '@lucide/vue';
// 版本号前的 tag 图标走 Lucide：EP 的 `PriceTag` 缩到 13px 糊成一颗黑点，
// 认不出是「标签」（2026-10-08 概览版本号那处已踩过一次，注释留在
// OverviewPanel.vue）。它是纯装饰——版本字符串本身才是信息，所以图片对读屏隐藏。
const themeIcons = { light: SunIcon, dark: MoonIcon, system: MonitorIcon };
const themeLabel = computed(() => themeOptions.find((option) => option.value === theme.value)?.label);

const nextTheme = computed(() => themeOptions[(themeOptions.findIndex((option) => option.value === theme.value) + 1) % themeOptions.length]);
const themeTip = computed(() => '当前：' + themeLabel.value + '；点击切换到' + nextTheme.value.label);

const MENU_GROUPS = [
  {
    label: '工作台',
    items: [
      { id: 'overview', label: '概览', icon: PAGE_ICONS.overview },
      // 「用量分析」聚合套餐额度（云端）与本机模型 token 统计（2026-10-28 用户
      // 要求）：概览页只保留「今日用量」一个读数，详情集中到这一页。
      { id: 'usage', label: '用量分析', icon: PAGE_ICONS.usage },
    ],
  },
  {
    label: '资源',
    items: [
      { id: 'versions', label: '内核版本', icon: PAGE_ICONS.versions },
      {
        id: 'plugins',
        label: '插件',
        icon: PAGE_ICONS.plugins,
        badge: () => (pluginStore.view && pluginStore.view.updates) || 0,
      },
      {
        id: 'skills',
        label: '技能',
        icon: PAGE_ICONS.skills,
        badge: () => (skillStore.view && skillStore.view.updates) || 0,
      },
      // 「记忆」与插件 / 技能同属「往里装了什么」：它是本机的长期偏好与项目约定，
      // 用户来这里保存、纠正、停用与删除；开关打开后工作台的相关会话会带上它。
      { id: 'memory', label: '记忆', icon: PAGE_ICONS.memory },
    ],
  },
  {
    label: '系统',
    items: [
      { id: 'settings', label: '设置', icon: PAGE_ICONS.settings },
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

// 过滤掉 `show()` 返回 false 的菜单项，并隐藏没有可见项的分组。
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

</script>

<template>
  <aside class="sidebar" :class="{ 'is-collapsed': collapsed }">
    <div class="brand">
      <!-- 尺寸 34 → 42px（2026-10-08 用户要求放大鲸鱼 icon）。这两个属性与
           theme.css 的 `.brand img` 必须相等：它们是给读屏与布局的第一手尺寸，
           CSS 只覆盖绘制，属性留在旧值就是一句谎。判据钉住「两处相等」。 -->
      <img src="/whale-icon.png" alt="" width="42" height="42" />
      <div class="brand__text">
        <!-- 2026-11-08 用户要求：品牌区只留英文两行「Dsh-Xlink / Studio」，中文
             slogan「桌面管理平台」移除——红色标识由黑鲸的红眼本身承担，不再用红色
             文字补一句中文。长名字的换行问题仍由结构兜住：主名一行、副名固定
             第二行。收起开关原先浮在这一行右端（`.brand__toggle`），2026-10-07
             挪到了底部那一行，这里不再有按钮。 -->
        <div class="brand__name">Dsh-Xlink <span class="brand__name__sub">Studio</span></div>
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

    <div class="sidebar__footer">
      <!-- 底部两行：第一行三枚方钮（收起 / 刷新 / 主题），第二行只有版本号。
           「桌面端」这个文字标签 2026-11-12 用户要求删掉（截图圈着它）：这行
           数字是谁的版本不再用文字标注。

           2026-10-20 用户截图：底部曾是一行，224px 里「标签 + 三个 26px 方钮
           + 缝」占掉 ~142px，版本号被截成「0.5.8-r…」——dev 构建的
           「0.5.8-rc.2（dev）」必然读不全，而 dev 后缀正是这个构建里唯一
           区别 dev / release 的标记。于是拆成两行。
           2026-10-21 用户再改排布：三枚控件**左对齐贴到左边**（原先被
           space-between 推到右缘），标签下移与版本号同行；标签删掉后第二行
           只剩版本号。 -->
      <div class="sidebar__footer-actions">
        <!-- 收起开关。2026-10-07 用户要求从品牌行右端移到这里。
             **这是覆盖设计稿，不是稿子就这么画的**：稿子的 `.sidebar-footer` 只有
             「标签 + 版本 + 刷新 + 主题」四个元素，收起开关画在 `.brand` 右端
             （`docs/ui/dsh-xlink-ui-redesign-draft.html` 439-468 行）。用户的取舍是
             品牌行只留品牌——收起开关紧挨着它收起的侧栏右侧，和「底部一排控件」
             读起来是一件事。收起态（64px）放不下必须竖排（见 theme.css 的
             `.sidebar.is-collapsed .sidebar__footer-actions`）。
             形状复用 `.sidebar__icon-btn`（26×22 方钮），不另造一套——它是**控件**，
             不是品牌区那种无边框轻按钮。 -->
        <button
          type="button"
          class="sidebar__icon-btn"
          :title="collapsed ? '展开侧栏' : '收起侧栏'"
          :aria-label="collapsed ? '展开侧栏' : '收起侧栏'"
          :aria-expanded="!collapsed"
          @click="toggleCollapsed"
        >
          <el-icon><component :is="collapsed ? Expand : Fold" /></el-icon>
        </button>
        <button
          type="button"
          class="sidebar__icon-btn"
          title="检查桌面端是否有新版本"
          aria-label="检查桌面端是否有新版本"
          :disabled="globalBusy"
          @click="checkShellUpdate(true)"
        >
          <el-icon><Refresh /></el-icon>
        </button>
        <button type="button" class="sidebar__theme-btn sidebar__icon-btn" :title="themeTip" :aria-label="themeTip" @click="setTheme(nextTheme.value)">
          <component :is="themeIcons[theme]" :size="16" />
        </button>
      </div>
      <!-- 第二行：只有版本号（「桌面端」文字标签 2026-11-12 用户要求删除）。
           版本号从 2026-10-20 起不与控件同行：跟三枚方钮同行时 dev 后缀被截成
           「0.5.8-r…」。 -->
      <div class="sidebar__footer-meta">
        <!-- 版本号 + 前置 tag 图标（2026-10-20 用户要求「版本号前面加个 tag 的
             icon」）。图标是装饰性图形，语义由版本字符串承载，所以对读屏隐藏；
             文字包一层独立元素：图标不参与压缩，极端长度时被省略号收掉的是文字。 -->
        <span v-if="shellVersionText" class="version-badge">
          <TagIcon class="version-badge__icon" :size="13" aria-hidden="true" />
          <span class="version-badge__text">{{ shellVersionText }}</span>
        </span>
      </div>
    </div>
  </aside>
</template>

<style scoped>
/* 底部工具区两行：第一行三枚方钮（收起 / 刷新 / 主题）左对齐贴左缘，第二行
   只剩版本号（「桌面端」文字标签 2026-11-12 用户要求删除）。方形控件固定
   26×22 且不参与收缩（flex: 0 0 auto）——它们是动作。版本号不与控件同行
   （拆行前 dev 构建的版本号被截成「0.5.8-r…」，dev 后缀整段看不见），省略号
   只作极端长度的兜底，规则在 theme.css 的 `.sidebar__footer .version-badge`。
   `.brand__toggle`（原品牌行右端那个无边框轻按钮）已于 2026-10-07 删除：
   收起开关改挂在底部后，它既没有调用方，也不再需要 `position: absolute`
   把主名让出来——品牌行现在只有 logo 与文字。 */
.sidebar__icon-btn {
  display: grid;
  place-items: center;
  flex: 0 0 auto;
  width: 26px;
  height: 22px;
  border: 1px solid var(--border);
  border-radius: 0;
  background: transparent;
  color: var(--text-muted);
  cursor: pointer;
  font-size: var(--fs-5);
}
.sidebar__icon-btn:hover:not(:disabled) {
  background: var(--surface-raised);
  color: var(--text);
}
.sidebar__icon-btn:disabled {
  opacity: 0.5;
  cursor: default;
}
/* 三态主题入口复用底部方钮，图标表达选择的模式。 */
.sidebar__theme-btn { color: var(--accent); }
</style>
