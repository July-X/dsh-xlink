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
  TopRight,
  Fold,
  Expand,
} from '@element-plus/icons-vue';
import { store, checkShellUpdate } from '../store.js';
import { globalBusy } from './loading.js';
import { pluginStore } from '../plugins/plugins.js';
import { skillStore } from '../skills/skills.js';
import { theme, themeOptions, setTheme } from './theme.js';
import { Sun as SunIcon, Moon as MoonIcon, Monitor as MonitorIcon } from '@lucide/vue';
const themeIcons = { light: SunIcon, dark: MoonIcon, system: MonitorIcon };
const themeLabel = computed(() => themeOptions.find((option) => option.value === theme.value)?.label);

const nextTheme = computed(() => themeOptions[(themeOptions.findIndex((option) => option.value === theme.value) + 1) % themeOptions.length]);
const themeTip = computed(() => '当前：' + themeLabel.value + '；点击切换到' + nextTheme.value.label);

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
      // 「数据迁移」是**常驻**菜单项，紧跟在「设置」下面（设计稿 2550 行的
      // 系统组就是这两项，无条件判断）。
      //
      // 它此前是条件渲染的——只在「从未迁移过 + 扫到遗留数据」时出现。维护者
      // 2026-10-07 改为常驻：这一条覆盖了设计说明 §侧栏「数据迁移入口受旧数据
      // 发现状态控制…不应把条件入口强行固定成常驻功能」那句话，理由是这个功能
      // **在 v0.6.0 之后整体移除**（见 ui/AGENTS.md 的「待移除」一节）——过渡期
      // 内它是一个要能被随时打开的正式功能，不该在大部分用户那台机器上凭空
      // 消失；而入口必须先存在，才谈得上"到时候删干净"。
      // 设置页仍保留一个轻量入口（设计说明同段那句话的后半句「迁移完成后，设置页
      // 仍保留进入迁移功能的入口」），那边是「环境出问题时才动」那一组里的一行。
      // 图标取 `TopRight`（右上箭头）而不是 `Right`（单向右）：设计稿 2550 行
      // 用的就是 `↗`。迁移是「把东西从旧处搬到新处」，右上是双向的视觉暗示；
      // 纯向右读起来像「进入下一层」，而它其实是搬走。
      { id: 'migration', label: '数据迁移', icon: TopRight },
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
        <!-- 2026-10-07 按设计稿改文案：主名「DeepSeek Harness」+ 副名「桌面管理台」。
             原先写的是「Dsh-Xlink / DeepSeek 内核桌面管理端」——那是仓库名与
             一句功能描述，副名 11px 下一行塞了 12 个字，在 224px 侧栏里挤到
             换行。主名说产品，副名说这是什么形态的端，两行都短。
             收起开关原先浮在这一行右端（`.brand__toggle`），2026-10-07 挪到了
             底部那一行，这里不再有按钮，主名独占整行宽度。 -->
        <div class="brand__name">DeepSeek Harness</div>
        <div class="brand__desc">桌面管理台</div>
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
      <!-- 设计稿的底部是一行：「桌面端」标签 + 版本徽标 + 刷新 + 主题开关。
           原先是两个整宽按钮（检查更新 / 深色主题）竖着堆，版本号跟在
           「检查更新」右侧——在 224px 侧栏里要占两行，且主题开关被写成
           「深色主题」这种和它自身无关的词（它切到的是另一侧）。
           现在三个控件各司其职，标签也只说它是什么。 -->
      <span class="nav-item__label sidebar__footer-label">桌面端</span>
      <span v-if="shellVersionText" class="version-badge">{{ shellVersionText }}</span>
      <!-- 收起开关。2026-10-07 用户要求从品牌行右端移到这里。
           **这是覆盖设计稿，不是稿子就这么画的**：稿子的 `.sidebar-footer` 只有
           「标签 + 版本 + 刷新 + 主题」四个元素，收起开关画在 `.brand` 右端
           （`docs/ui/dsh-xlink-ui-redesign-draft.html` 439-468 行）。用户的取舍是
           品牌行只留品牌——收起开关紧挨着它收起的侧栏右侧，和「底部一排控件」
           读起来是一件事。代价记在这：底部现在是三个控件，224px 侧栏里版本号
           省略得更多，收起态（64px）放不下必须竖排（见 theme.css 的
           `.sidebar.is-collapsed .sidebar__footer`）。
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
  </aside>
</template>

<style scoped>
/* 底部一行：标签 + 版本徽标 + 三个控件（收起 / 刷新 / 主题）。标签与徽标可
   压缩，方形控件固定 26×22 且不参与收缩（flex: 0 0 auto）——它们是动作，
   版本号才是附属信息，先压版本号而不是把按钮压成看不清。
   `.brand__toggle`（原品牌行右端那个无边框轻按钮）已于 2026-10-07 删除：
   收起开关改挂在底部一行后，它既没有调用方，也不再需要 `position: absolute`
   把主名让出来——品牌行现在只有 logo 与文字。 */
.sidebar__footer-label {
  color: var(--text-secondary);
  white-space: nowrap;
}
.sidebar__icon-btn {
  display: grid;
  place-items: center;
  flex: 0 0 auto;
  width: 26px;
  height: 22px;
  border: 1px solid var(--border);
  border-radius: 5px;
  background: transparent;
  color: var(--text-muted);
  cursor: pointer;
  font-size: 12px;
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
