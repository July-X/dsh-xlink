<script setup>
// 插件页：单 card 双 tab——
//   · 当前内核：顶栏选中内核的插件视图——状态取自 PluginRow 的 legacy 字段
//     （wired / synced / actual_mode / quarantined），沿用旧渲染路径；只做管理
//     （同步 / 接线 / 模式切换 / 卸载），不带任何安装入口。
//   · 已安装：本机插件库清单（每个插件 + 每个实例一枚 chip，按 instance_id
//     在 PluginRow.instances map 里查状态）+ 全部获取入口（手动安装 +
//     插件中心）——安装动作针对的是插件库，不属于某个内核。
import { computed, onMounted, ref } from 'vue';
import {
  Refresh,
  Switch,
  Delete,
  TopRight,
  Download,
  RefreshLeft,
  ArrowDown,
  Box,
  Link,
  InfoFilled,
  WarningFilled,
  Warning,
  Search,
} from '@element-plus/icons-vue';
import {
  pluginStore,
  catalogLoading,
  CATALOG_PAGE,
  catalogCategories,
  categoryLabel,
  formatCount,
  formatUpdated,
  installedKeys,
  isInstalled,
  searchCatalog,
  submitCatalogSearch,
  refineCatalog,
  installPlugin,
  precheckPlugin,
  setPrecheckEnabled,
  updatePlugin,
  setPluginMode,
  resolvePluginQuarantine,
  syncPlugins,
  uninstallPlugin,
  checkPluginUpdates,
} from './plugins.js';
import { originLabel, tildePath } from '../shell/labels.js';
import { builtinStore, loadBuiltinStatus, toggleBuiltin } from './builtin.js';
import { globalBusy, isLoading, withLoading } from '../shell/loading.js';
import { openExternalLink } from '../shell/notify.js';
import { store } from '../store.js';
import { instanceStore, familyLabel } from '../kernel/instance.js';

const view = computed(() => pluginStore.view);

// 安装预检开关。默认开启（后端 `plugin_precheck` 为 null 时按 true 解释），
// 关掉后「安装」不再起临时内核——用户明确表示信任这个来源时才需要。
//
// 键名是 **snake_case**：Rust 的 `Settings` 只挂了 `#[serde(default)]`、没有
// `rename_all = "camelCase"`，而它同时还要按原样读写磁盘上的 settings.json，
// 改名会连持久化格式一起改。读成 `pluginPrecheck` 时拿到的是 undefined，
// 判据里的「undefined 按 true 解释」正好把它变成**恒真**——开关点了没反应，
// 而且目录里每条插件的「预检并安装」也跟着一直走预检那一条路。
const precheckOn = computed(() => {
  const value = store.view && store.view.settings && store.view.settings.plugin_precheck;
  return value === undefined || value === null ? true : !!value;
});

const precheckBusy = ref(false);

async function togglePrecheck(value) {
  precheckBusy.value = true;
  try {
    await setPrecheckEnabled(value);
  } finally {
    precheckBusy.value = false;
  }
}

// 内嵌 OpenAI 对话插件（openai-oauth）：随应用交付，不进社区中央库。
// 状态常驻这一行、开关是唯一动作；「内核运行中不能改」由后端按实例级
// 判据拒绝，UI 不预判（预判一份就是第二份判据，会和后端漂移）。
const builtinView = computed(() => builtinStore.view);

const builtinStateText = computed(() => {
  const v = builtinView.value;
  if (!v) return '';
  if (v.stateError) return '状态文件异常，悬停查看';
  if (v.note) return v.note;
  if (v.loadState === 'prepared') return '已接线，下次启动工作台生效';
  if (v.loadState === 'incompatible') return '内核已切换，重新拨一次开关修复';
  return '未启用';
});

const builtinStateTip = computed(() => {
  const v = builtinView.value;
  return (
    (v && (v.stateError || v.note)) ||
    '随应用交付的 OpenAI 套餐接入；启用不需要联网安装，关闭不删除账号'
  );
});

onMounted(() => {
  loadBuiltinStatus();
});

// 存储位置与生效规则原本占整整一段正文（窄窗口下换行成两三行），收进卡片头左
// 侧的小图标气泡里——与技能页保持一致；下方 tab 文字「已安装」与「当前内核」
// 自身即可承担章节名，卡片头不再单独挂「已安装」标题。
// 存储位置读后端返回的真实路径（`PluginStatus.store_root`），与技能页的
// `storeTip` 同一做法。此前这里写死 `~/.dsh-xlink/dsh-plugins/`——那是
// legacy 路径，`store_relocate` 早已把中央库整体搬进 `plugins/dsh/`，用户
// 照着提示去找会找到一个不存在的目录。同一份路径在 `paths.rs` 与 Vue 里各写
// 一遍，前端那份不参与编译也不会报错，只能靠人看出来。
const installTip = computed(() => {
  const store = tildePath(view.value && view.value.store_root) || '~/.dsh-xlink/plugins/dsh/';
  return (
    '插件统一存放于 ' + store +
    '，切换内核无需重装；安装完成后自动校验是否符合 dsh 插件规范，内核重启后生效。'
  );
});

// 第三方来源免责提示的文案。提到常量而不是写在模板里：它是「标题旁那个
// ⚠ 是什么」的答案，tooltip 的 `content` 与图标的可访问名（`aria-label`）要用
// 同一份字符串——两处各写一遍，改了其中一处就会出现「图标说有提示，读屏说没提示」。
const THIRD_PARTY_NOTICE = '第三方插件由社区提供，本工具不对其安全性负责，请自行甄别。';

// 预检开关的说明。必须写清「预检到底验了什么、没验什么」——用户把它当成
// 安全保证是最危险的误解：它只覆盖内核启动阶段，页面加载后的运行时异常
// 仍由工作台窗口的健康自检负责。
const precheckTip =
  '开启后，点「安装」会先在一个一次性沙盒实例里真的装一次、真的启动一次内核，' +
  '确认没问题才装到当前实例；装坏了会原样撤销，你的环境不受影响。代价是多花十几秒。' +
  '注意：预检只覆盖内核启动阶段（进程存活、端口监听、HTTP 应答、启动日志），' +
  '工作台页面加载后的运行时异常不在其中。';

// --- 已安装列表 ---

function quarantineNote(row) {
  const reason = String(row.quarantined.reason || '');
  return '已隔离：' + (reason.length > 60 ? reason.slice(0, 57) + '…' : reason);
}

// 待同步 / 待接线的原因：只在有活动内核时才成立（没有内核时状态 chip 已经
// 说明「无活动内核」），返回文案用于动作区的警示点 tooltip。
function syncWarning(row) {
  if (!view.value || !view.value.active_kernel) return '';
  if (!row.synced) return '待同步：活动内核中的插件副本与当前版本不一致，点「同步到所有内核」修复';
  if (!row.wired) return '待接线：活动内核的 profile 尚未加载该插件，点「同步到所有内核」修复';
  return '';
}

// 当前物化模式：活动核心里已落地时以实际模式为准，否则回落到中央库记录的
// 期望模式（此时 actual_mode 为空，旧 UI 在这种情况下不显示任何模式标签）。
function currentMode(row) {
  if (row.synced && row.actual_mode) return row.actual_mode;
  return row.desired_mode === 'copy' ? 'copy' : 'link';
}

function modeTip(row) {
  return currentMode(row) === 'link' ? '当前为链接模式，点击改为复制' : '当前为复制模式，点击改为链接';
}

function togglePluginMode(row) {
  // 每个按钮绑自己的 key（P2-10，与 OverviewPanel 的 P2-42 同一条约定）：
  // 用全局 `globalBusy` 当 loading 会让列表里**每一行**的模式徽章在任何长任务
  // 期间一起转圈并被禁用，看起来像每行都在切模式，也拿不到自己的进度语义。
  return withLoading('pluginMode:' + row.id, () =>
    setPluginMode(row.id, currentMode(row) === 'link' ? 'copy' : 'link')
  );
}

// --- 插件仓库 ---
//
// 这一段列的是**远端**目录（dshfind.com），上方那段是本机已装的。列表本身
// 由后端搜好送回一页（见 plugins.js 的 searchCatalog），所以这里没有一行
// 筛选逻辑，只有「把三个控件的选择交出去」。

const keys = computed(() => installedKeys());
// 已经是后端筛好、排序好的一页了：直接就是这一页，不再二次过滤。
const items = computed(() => pluginStore.catalogItems);
const hasMore = computed(() => pluginStore.catalogTotal > pluginStore.shown);
const catChips = computed(() => catalogCategories());

// 搜索框里的草稿。**只有回车才提交**：一次搜索要走一次 IPC 并在远端目录
// （1.7 万条）上扫一遍，逐字触发等于用户每敲一个字母就发一次。草稿不进
// store——只有提交后的 query 才是「当前这次搜索的条件」。
const searchText = ref(pluginStore.query);

function search() {
  return submitCatalogSearch(searchText.value);
}

// el-select 的 v-model 直接改 pluginStore.category / sort，但改动必须同时
// 触发一次远端搜索并把分页拉回第一页，否则会留下「翻到第 3 页切分类仍卡在
// 第 3 页」这种隐形 bug。
function pickCategory(id) {
  refineCatalog({ category: id, shown: CATALOG_PAGE });
}

function pickSort(id) {
  refineCatalog({ sort: id, shown: CATALOG_PAGE });
}

function showMore() {
  refineCatalog({ shown: pluginStore.shown + CATALOG_PAGE });
}

// 没命中时怎么措辞：是自己收窄了条件（换关键词或分类），还是目录本身就没
// 拉到（网络 / 代理），两者的下一步完全不同。
const emptyHint = computed(() => {
  if (pluginStore.catalogTotal > 0) return '';
  return pluginStore.query || pluginStore.category !== 'all'
    ? '没有匹配的插件，换个关键词或分类试试。'
    : '目录为空或加载失败，点「刷新数据」重试。';
});

function detailUrl(item) {
  return item.detail_url || (item.repo ? 'https://github.com/' + item.repo : '');
}

// 描述的截断分两层：这里按 90 字硬切一次（控制 DOM 里的文本长度），CSS 再按
// 行数 clamp 一次（控制视觉行高）。原先只靠这里的字数切，结果每张卡固定占
// 三行、描述长短不影响行数，一屏只看得到三四条。
function descText(item) {
  const d = item.description || '';
  return d.length > 90 ? d.slice(0, 87) + '…' : d;
}

// 目录条目原先单独占一行摆 4 个 tag，行高直接翻倍。tag 本身是搜索命中字段
// （见 plugins.js 的 haystack），值得留在信息里但不值得各占一行——挂到分类
// 标签的 tooltip 上，悬停即得。
function tagsTip(item) {
  const tags = (item.tags || []).map((tag) => String(tag).trim()).filter(Boolean);
  return tags.length ? '标签：' + tags.join(' · ') : '';
}

function statsText(item) {
  const parts = [];
  if (item.stars > 0) parts.push('★ ' + formatCount(item.stars));
  if (item.forks > 0) parts.push('Fork ' + formatCount(item.forks));
  const updated = formatUpdated(item.updated);
  if (updated) parts.push(updated);
  return parts.join(' · ');
}

// --- P8 #2 双 tab ---
//
// 「本实例」tab 与旧渲染路径共用同一个 entity-row 模板——状态走 legacy
// 字段（wired / synced / actual_mode / quarantined）。「所有实例」tab
// 走 row.instances map，把每个实例的 chip 摆出来，方便对比哪个实例装了
// 哪个没装。
const installedTab = ref('all');
const sourceTab = ref('catalog');
// 「当前内核」是个相对概念，用户未必知道它指什么：标签里带上当前内核身份
// 与其活动版本号（与概览页「活动版本」同源）。注册表实例 id（如 default）
// 是实现细节，与顶栏内核 tab 同口径不对外展示。两个 tab 的 tooltip 各讲
// 各的职责，一句话说完。
const currentInstanceLabel = computed(() => {
  const def = instanceStore.list.find((item) => item.is_default);
  if (!def) return '';
  const family = familyLabel(def.record.kernel_family);
  const active = store.view && store.view.kernel && store.view.kernel.active;
  return active ? `${family} · ${active}` : family;
});
// 版本号单独拆出来用小字号渲染，避免 tab 标签过长与右侧动作按钮挤在一起。
const currentTabSub = computed(() => currentInstanceLabel.value);
const currentTabTip = '只管理顶栏当前选中的内核：同步、模式与卸载都只作用于它。';
const installedTabTip = '本机插件库：安装新插件，对比各内核的装载状态。';
// 排序规则：把默认实例（is_default=true）排第一个，其余按 id 升序——
// 让用户一眼看出默认实例在所有实例中的差异位置。
const sortedInstances = computed(() => {
  const list = instanceStore.list.slice();
  list.sort((a, b) => {
    if (a.is_default !== b.is_default) return a.is_default ? -1 : 1;
    return a.record.id.localeCompare(b.record.id);
  });
  return list;
});
// 某行在指定实例上的状态：instances map 没有 key 说明注册表加载失败
// （P8 #2 后端降级路径），按「无数据」处理，UI 上标「—」。
function instanceStateFor(row, instanceId) {
  if (!row.instances) return null;
  return row.instances[instanceId] || null;
}
function instanceChipLabel(row, instanceId) {
  const state = instanceStateFor(row, instanceId);
  if (!state) return '—';
  if (state.quarantined) return '已隔离';
  if (!state.materialized) return '未同步';
  if (!state.synced) return '版本不一致';
  if (!state.wired) return '未接线';
  return '已接线';
}
// 状态词的原话：chip 上只放短词，悬停讲清「这是什么、怎么修」。
function instanceChipTip(row, instanceId) {
  const state = instanceStateFor(row, instanceId);
  if (!state) return '该实例的状态暂不可用（实例注册表未加载）';
  if (state.quarantined) return '因启动故障被隔离停用；可在「本实例」页恢复启用';
  if (!state.materialized) return '插件尚未同步到该实例；点上方「同步到所有内核」即可装载';
  if (!state.synced) return '该实例中的插件副本与当前安装版本不一致；点「同步到所有内核」更新';
  if (!state.wired) return '该实例的内核配置尚未接入此插件；同步后重启工作台生效';
  return '插件已在该实例装载并接入内核';
}
function instanceChipType(row, instanceId) {
  const state = instanceStateFor(row, instanceId);
  if (!state || !state.materialized) return 'info';
  if (state.quarantined) return 'danger';
  if (!state.synced) return 'warning';
  if (!state.wired) return 'warning';
  return 'success';
}
</script>

<template>
  <section class="panel">
    <!-- 第三方来源的免责提示：原先是页头下面一条整宽的 notice 条。2026-10-07
         用户要求收成标题旁的警告图标 —— 那条文案只有一句、不含任何随状态变化
         的信息，却常驻占掉一整行，把真正的页头内容（插件管理那排控件）往下挤；
         而它的**语境**就是「插件」这个标题本身，图标挂在标题旁边反而更贴。
         触发方式用 hover 而不是常驻：免责提示要能被主动读到，但不要求它在用户
         没问的时候一直占屏。`role="note"` 换成 `tabindex="0"`——原先 div 是
         纯展示，读屏能过；现在是可聚焦的触发元素，键盘要够得着，
         否则 hover-only 的信息对键盘用户就是不存在。 -->
    <div class="page-head">
      <!-- 标题行外面还要再包一层 `<div>`：`.page-head` 是 `space-between` 的
           flex 行，标题与说明必须同属一个子项，否则说明会被当成右侧的「动作」
           顶到页面右缘去。六个页面共用这个结构，别只改这一个。 -->
      <div>
        <div class="page-title-row">
          <h1 class="page-title">插件</h1>
          <el-tooltip placement="top" effect="dark" :content="THIRD_PARTY_NOTICE">
            <el-icon
              class="head-tip-icon head-tip-icon--warning"
              tabindex="0"
              role="note"
              :aria-label="THIRD_PARTY_NOTICE"
            ><Warning /></el-icon>
          </el-tooltip>
        </div>
        <p class="page-desc">查看当前实例生效的插件，并从插件中心安装或升级。</p>
      </div>
    </div>
    <div class="card entity-card">
      <div class="card-head plugin-center-head">
        <div class="plugin-center-title-row">
          <!-- 设计稿（2746 行）这张卡叫「插件管理」，不是「插件中心」：它是整页的
               容器——页签、左栏的本机插件库、右栏的远端目录都在里面。而「插件中心」
               是**右栏**那份 dshfind.com 目录的名字。原先两处一个叫「插件中心」
               一个叫「插件仓库」，指的却是同一份远端目录，同一页里两个名字。 -->
          <span class="plugin-center-title">插件管理</span>
          <!-- ⓘ 只剩图标：原先可见文字写「数据来源于 dshfind.com」而 tooltip 里讲的是
               插件存放路径与生效规则，**说的不是同一件事**——鼠标停在字上弹出的是
               另一段话。与技能页同一个处理（见 ui/AGENTS.md「提示收成图标」）。 -->
          <el-tooltip placement="top" effect="dark" :content="installTip">
            <el-icon class="head-tip-icon"><InfoFilled /></el-icon>
          </el-tooltip>
        </div>
        <!-- 预检开关独占卡头的第二行（在标题 + ⓘ 之下）。它带一整行灰字说明，
             与标题挤在同一行会把卡头撑成三行。「刷新数据」原先也在这一行，
             2026-10-07 已按用户要求搬去右栏「插件中心」——它刷的是那份远端目录，
             不是本机插件库，两者本来就不是一件事。 -->
        <div class="precheck-toggle-row">
          <el-switch
            :model-value="precheckOn"
            :loading="precheckBusy"
            :disabled="globalBusy"
            inline-prompt
            active-text="预检"
            inactive-text="直装"
            @update:model-value="togglePrecheck"
          />
          <el-tooltip placement="top" effect="dark" :content="precheckTip">
            <span class="precheck-toggle-label">
              <el-icon><InfoFilled /></el-icon>
              安装前先在沙盒实例里试装并启动一次
            </span>
          </el-tooltip>
        </div>
      </div>
      <el-alert
        v-if="view && view.warning"
        :title="view.warning + '（可在「日志」侧查看 plugin-wiring.log）'"
        type="warning"
        :closable="false"
        show-icon
      />

      <!-- 单 card 双 tab，默认激活「当前内核」：「已安装」（本机插件库清单 +
           获取入口：手动安装、插件中心）排在左侧第一位，「当前内核」随后——
           安装针对插件库，不属于某个内核，故不放在当前内核 tab。两个 tab
           共用 pluginStore.view.rows；顶栏切默认实例后，当前内核 tab 自动
           跟随新默认实例。 -->
      <div class="installed-tabs-wrap">
        <span class="tabs-head-actions">
          <el-button size="small" :icon="Switch" :disabled="globalBusy" @click="syncPlugins">同步内核</el-button>
          <el-button
            size="small"
            :icon="Refresh"
            :loading="isLoading('checkPluginUpdates')"
            :disabled="globalBusy"
            @click="checkPluginUpdates({ busy: true, toastOnUpdates: true })"
          >
            检查更新
          </el-button>
        </span>
        <el-tabs v-model="installedTab" class="installed-tabs">
        <el-tab-pane name="all">
          <template #label>
            <el-tooltip placement="bottom-start" effect="dark" :content="installedTabTip">
              <span class="tab-label">已安装</span>
            </el-tooltip>
          </template>
          <!-- 「已安装」视图：本机插件库清单 + 获取入口。清单是每个插件
               一行、列上每个实例一枚 chip（不复用 entity-row 是因为这里
               没有「更新 / 模式切换 / 卸载」等 per-kernel 动作——写动作
               留在「当前内核」tab）；手动安装与插件中心也归这页：安装
               针对的是插件库，不属于某个内核。

               宽版（1040）下这一页按设计稿分两列：左列是**本机**那一份
               （已装清单），右列是手动安装与 **dshfind.com 上的远端目录**。
               两列的搜索对象根本不是一回事（本机 vs 远端），并排摆比让用户
               在一屏里上下找要直接得多；窄窗退化成单列时仍靠下面那道分组
               标题切开。 -->
          <div class="page-layout">
            <div class="page-layout__col">
          <div class="entity-list all-instances-list" :class="{ 'is-empty': !view || !view.rows || view.rows.length === 0 }">
            <el-empty
              v-if="!view || !view.rows || view.rows.length === 0"
              description="本机插件库为空；用右侧「手动安装」或插件中心获取插件。"
              :image-size="48"
            />
            <div
              v-for="row in view ? view.rows : []"
              :key="row.id"
              class="entity-row entity-row--instance-grid"
              :class="{ 'is-warn': !!row.quarantined }"
            >
              <div class="entity-head">
                <span class="entity-name">{{ row.name }}</span>
                <span class="origin-chip" :class="'origin-chip-' + row.origin">
                  <el-icon class="origin-chip-icon">
                    <Box v-if="row.origin === 'npm'" />
                    <Link v-else />
                  </el-icon>
                  <span class="origin-chip-label">{{ originLabel(row.origin) }}</span>
                </span>
              </div>
              <div class="entity-foot--instance-grid">
                <div class="instance-chip-row">
                  <el-tag
                    v-for="inst in sortedInstances"
                    :key="inst.record.id"
                    :type="instanceChipType(row, inst.record.id)"
                    size="small"
                    effect="plain"
                    class="instance-state-chip"
                    :title="instanceChipTip(row, inst.record.id)"
                  >
                    <span class="instance-state-chip__id">{{ familyLabel(inst.record.kernel_family) }} · {{ inst.record.id }}</span>
                    <span class="instance-state-chip__sep" aria-hidden="true">·</span>
                    <span class="instance-state-chip__label">{{ instanceChipLabel(row, inst.record.id) }}</span>
                  </el-tag>
                  <span
                    v-if="sortedInstances.length === 0"
                    class="instance-state-empty"
                  >未加载实例列表（实例注册表加载失败）</span>
                </div>
              </div>
            </div>
          </div>

            </div>

            <div class="page-layout__col plugin-center-col">
          <el-tabs v-model="sourceTab" class="installed-tabs source-tabs">
            <el-tab-pane label="插件中心" name="catalog">
          <h3 class="section-divider">
            插件中心
            <span class="muted section-divider__note">
              来自
              <a href="https://dshfind.com/zh" target="_blank" rel="noreferrer">dshfind.com</a>
            </span>
            <!-- 「刷新数据」原先挂在整张卡的卡头靠右（2026-10-07 用户要求搬过来）。
                 它刷的是 **dshfind.com 那份远端目录**，不是本机插件库——挂在整张卡的
                 头上等于让一个作用在右栏的按钮出现在左栏。搬过来之后，下面那句
                 「目录为空或加载失败，点「刷新数据」重试」才有个近处的按钮可指。
                 按钮放在 h3 里不是新发明：内核版本页的「官方版本」标题行里本来就有
                 两枚图标按钮（`.list-head-with-logo`）。 -->
            <el-button
              class="plugin-center-refresh btn-action"
              size="small"
              :icon="Refresh"
              :loading="isLoading('catalogReload')"
              :disabled="globalBusy"
              @click="searchCatalog({ force: true, loud: true })"
            >
              刷新数据
            </el-button>
          </h3>
          <!-- 搜索框独占一行，放在分类下拉之前：这一列是插件页的右半栏，
               三个控件挤一行时每个只剩 150px 出头，「全部（17.5k）」这类
               带计数的选项会先被截断。关键词也最常用，占主位合理。 -->
          <div class="install-row">
            <el-input
              v-model="searchText"
              placeholder="搜索插件名 / 描述 / 标签，回车搜索"
              spellcheck="false"
              clearable
              @keyup.enter="search"
              @clear="search"
            >
              <template #prefix>
                <el-icon><Search /></el-icon>
              </template>
              <template #suffix>
                <span class="muted" title="按 Enter 在插件中心里搜索">↵</span>
              </template>
            </el-input>
          </div>

          <!-- 分类筛选用下拉与排序并列：原本是单行横滚的 chip，10 个分类 +
               「全部」在半栏宽度里横滚只能看到三四个，渐隐遮罩又挡掉
               选项前的数字，用户压根看不到完整列表。改成下拉后所有分类 +
               计数都明确展示，排序下拉占主位、分类筛选收同一行右侧。
               三个控件任一变动都重新搜一次（后端在那份 1.7 万条的远端
               目录上筛），下拉里的计数是**本次关键词下**各类还剩多少。 -->
          <div class="catalog-subbar">
            <el-select
              v-model="pluginStore.category"
              class="catalog-category"
              title="分类筛选"
              @change="pickCategory"
            >
              <el-option
                v-for="chip in catChips"
                :key="chip.id"
                :value="chip.id"
                :label="chip.count ? `${chip.label}（${formatCount(chip.count)}）` : chip.label"
              />
            </el-select>
            <el-select v-model="pluginStore.sort" class="catalog-sort" title="排序" @change="pickSort">
              <el-option value="stars" label="Star 最多" />
              <el-option value="updated" label="最近更新" />
            </el-select>
          </div>

          <!-- 加载态分两种，别混成一种：首次打开时手上**没有**结果可留，
               才用 120px 的空槽 + 遮罩；重新搜索（回车 / 切分类 / 排序 /
               显示更多）时旧结果留在原地、盖一层遮罩——整页塌成一个 120px
               的块再撑回来（一页 24 条约 1700px），每次按键闪一下比转圈
               更难受。条件必须带上 `items.length`：只判 `!catalogLoaded`
               的话，重新搜索也会走空槽那条，遮罩等于白写。 -->
          <div
            v-if="!pluginStore.catalogLoaded && items.length === 0"
            v-loading="catalogLoading"
            style="min-height: 120px"
            element-loading-text="正在搜索插件中心…"
          ></div>
          <p v-else-if="items.length === 0" class="muted" style="margin: 0">
            {{ emptyHint }}
          </p>
          <div v-else v-loading="catalogLoading" class="catalog-list">
          <TransitionGroup name="catalog" tag="div" class="catalog-items">
            <!-- 行式条目：三段定高——「标题 / 描述 / 底部」。版本徽标 2026-10-08
                 按用户要求从标题行挪到**底部那行**（`.catalog-meta` 组，与热度
                 数据同排）：长版本串（open-design-v0.19.2 这类）挤在名称旁边
                 会把名称压成省略号，挪下去之后标题行只剩「名称 + 分类 +
                 已验证」，而版本信息仍然在名称正下方可见。 -->
            <div
              v-for="(item, index) in items"
              :key="item.spec || item.name"
              class="catalog-row"
              :style="{ '--i': index }"
            >
              <div class="catalog-row-title">
                <span class="catalog-name">{{ item.name }}</span>
                <el-tooltip
                  v-if="item.category"
                  placement="top"
                  effect="dark"
                  :disabled="!tagsTip(item)"
                  :content="tagsTip(item)"
                >
                  <el-tag class="catalog-cat-tag" type="info" size="small" effect="plain">
                    {{ categoryLabel(item.category) }}
                  </el-tag>
                </el-tooltip>
                <span v-if="item.verified" class="catalog-verified">已验证</span>
              </div>
              <p v-if="item.description" class="catalog-desc">{{ descText(item) }}</p>
              <div class="catalog-row-foot">
                <div class="catalog-meta">
                  <span v-if="item.version" class="catalog-version">{{ item.version }}</span>
                  <span class="catalog-stats">{{ statsText(item) }}</span>
                </div>
                <span class="catalog-actions">
                  <el-tooltip placement="top" effect="dark" content="在浏览器打开插件详情页">
                    <el-button
                      v-if="detailUrl(item)"
                      size="small"
                      text
                      :icon="TopRight"
                      @click="openExternalLink(detailUrl(item), '插件详情页')"
                    />
                  </el-tooltip>
                  <el-button v-if="isInstalled(item, keys)" size="small" disabled>已安装</el-button>
                  <template v-else>
                    <!-- 预检开启时按**两阶段**走：先在沙盒里起一次临时内核
                         验证（点了不装），用户在报告对话框里点「应用变更」
                         才真的装上。因此预检开启时这枚按钮不能还叫「安装」——
                         用户会以为点完就装好了。旁边那枚「预检」在预检关闭
                         时才有意义：那时它是「不装只看一眼」的唯一入口。 -->
                    <el-button
                      v-if="!precheckOn"
                      size="small"
                      text
                      :icon="InfoFilled"
                      :disabled="globalBusy"
                      @click="precheckPlugin(item.spec)"
                    >
                      预检
                    </el-button>
                    <el-button
                      size="small"
                      :type="precheckOn ? 'default' : 'primary'"
                      :icon="Download"
                      :disabled="globalBusy"
                      @click="precheckOn ? precheckPlugin(item.spec) : installPlugin(item.spec)"
                    >
                      {{ precheckOn ? '预检并安装' : '安装' }}
                    </el-button>
                  </template>
                </span>
              </div>
            </div>
          </TransitionGroup>

          <!-- 「还有 N 个」按**后端报的总命中数**算，不是本页长度减已显示：
               列表现在是后端分页送回来的，本页长度永远等于 shown。 -->
          <div v-if="pluginStore.catalogLoaded && hasMore" class="catalog-more">
            <el-button text :icon="ArrowDown" :loading="catalogLoading" :disabled="globalBusy" @click="showMore">
              显示更多（还有 {{ pluginStore.catalogTotal - pluginStore.shown }} 个）
            </el-button>
          </div>
          </div>
            </el-tab-pane>
            <el-tab-pane label="手动安装" name="manual">
          <h3 class="section-divider">手动安装</h3>
          <div class="install-row">
            <el-input
              v-model="pluginStore.spec"
              placeholder="npm i @scope/pkg · 也支持 owner/repo、dsh add"
              spellcheck="false"
              clearable
              @keyup.enter="precheckOn ? precheckPlugin('') : installPlugin('')"
            >
              <template #suffix>
                <span
                  class="muted"
                  :title="precheckOn ? '按 Enter 先做预检，确认后才会装上' : '按 Enter 开始安装'"
                  >↵</span
                >
              </template>
            </el-input>
          </div>
            </el-tab-pane>
          </el-tabs>
            </div>
          </div>
        </el-tab-pane>

        <el-tab-pane name="current">
          <template #label>
            <el-tooltip placement="bottom-start" effect="dark" :content="currentTabTip">
              <span class="tab-label">
                当前内核
                <span v-if="currentTabSub" class="tab-label-sub">（{{ currentTabSub }}）</span>
              </span>
            </el-tooltip>
          </template>
          <!-- 内嵌插件区（设计 §3.1）：独立于下方社区插件列表——它没有
               安装 / 卸载 / 更新语义，只有启用意图一个开关。接线行 id
               同时是内核模型设置页账户卡的 settingsNs，两侧靠它对上。 -->
          <div v-if="builtinView" class="builtin-strip">
            <span class="builtin-name">OpenAI 对话</span>
            <span class="muted">内嵌</span>
            <el-tooltip placement="top" effect="dark" :content="builtinStateTip">
              <span class="builtin-state">{{ builtinStateText }}</span>
            </el-tooltip>
            <el-switch
              class="builtin-switch"
              size="small"
              :model-value="builtinView.requestedEnabled"
              :loading="isLoading('builtinOpenaiToggle')"
              :disabled="!builtinView.pluginSourceAvailable || !!builtinView.stateError"
              aria-label="启用或停用 OpenAI 对话"
              @change="toggleBuiltin"
            />
          </div>
          <div class="entity-list" :class="{ 'is-empty': !view || !view.rows || view.rows.length === 0 }">
        <!-- 首次状态未返回时显示骨架：view===null 是「加载中」而不是
             「尚未安装」，画成空态会让用户以为插件全丢了。 -->
        <template v-if="!view">
          <div v-for="i in 2" :key="'skeleton-' + i" class="entity-row">
            <el-skeleton :rows="1" animated style="width: 55%" />
          </div>
        </template>
        <el-empty v-else-if="!view.rows || view.rows.length === 0" description="当前内核尚未接入任何插件；先到「已安装」页签安装。" :image-size="48" />
        <!-- 单行布局（2026-10-08 用户要求「移动到右侧 + 单行显示每一个插件」）：
             entity-foot 解散，版本元数据 / 状态 / 警示点 / 动作成为行的直接子级，
             整行只有一条基线；布局覆盖见下方 scoped 块的 .entity-row--single。 -->
        <div
          v-for="row in view ? view.rows : []"
          :key="row.id"
          class="entity-row entity-row--single"
          :class="{ 'is-warn': !!row.quarantined }"
        >
          <div class="entity-head">
            <el-tooltip v-if="row.quarantined" placement="top" effect="dark" :content="quarantineNote(row)">
              <span class="entity-warn"><el-icon><WarningFilled /></el-icon> 已停用</span>
            </el-tooltip>
            <span class="entity-name">{{ row.name }}</span>
            <span class="origin-chip" :class="'origin-chip-' + row.origin">
              <el-icon class="origin-chip-icon">
                <Box v-if="row.origin === 'npm'" />
                <Link v-else />
              </el-icon>
              <span class="origin-chip-label">{{ originLabel(row.origin) }}</span>
            </span>
            <el-tooltip v-if="row.description" placement="top" effect="dark" :content="row.description">
              <span class="entity-desc">{{ row.description }}</span>
            </el-tooltip>
          </div>
          <!-- 版本元数据按用户要求移到右侧：紧跟状态 tag 与动作按钮，不再单独
               占第二行左侧。整行因此只剩一条基线（.entity-row--single）。 -->
          <div class="entity-meta">
            <span class="meta-version">{{ row.installed_version }}</span>
            <span v-if="row.latest_version" class="meta-upgrade">→ {{ row.latest_version }}</span>
            <span v-if="row.pinned" class="meta-pinned">已锁定版本</span>
          </div>
          <span class="entity-states">
            <el-tag v-if="!view || !view.active_kernel" type="warning" size="small" effect="plain">无活动内核</el-tag>
            <el-tag v-else-if="row.synced && row.wired" type="success" size="small" effect="plain">已同步</el-tag>
          </span>
          <!-- 待同步 / 待接线只在动作区点一个警示点，原因走 tooltip：原先把状态
               铺成整枚文字标签，一行挤三四枚，把行高和右半区一起顶满。 -->
          <span v-if="syncWarning(row)" class="state-dot">
            <el-tooltip placement="top" effect="dark" :content="syncWarning(row)">
              <el-icon><WarningFilled /></el-icon>
            </el-tooltip>
          </span>
          <div class="entity-actions">
            <!-- 物化模式：徽章文案即当前模式，点击切到另一种。切换走
                 plugin_set_mode 长任务，状态以 row.desired_mode 为准，
                 命令完成刷新后才翻转（未落地时回落到中央库记录的模式）。 -->
            <el-tooltip placement="top" effect="dark" :content="modeTip(row)">
              <el-button
                class="entity-mode"
                :class="{ 'is-link': currentMode(row) === 'link' }"
                size="small"
                :loading="isLoading('pluginMode:' + row.id)"
                :disabled="globalBusy"
                @click="togglePluginMode(row)"
              >
                {{ currentMode(row) === 'link' ? '链接' : '复制' }}
              </el-button>
            </el-tooltip>
            <el-tooltip v-if="row.quarantined" content="恢复启用" placement="top" effect="dark">
              <el-button
                class="entity-action"
                size="small"
                circle
                :icon="RefreshLeft"
                :aria-label="'恢复启用 ' + row.name"
                :disabled="globalBusy"
                @click="resolvePluginQuarantine(row.id, 'enable')"
              />
            </el-tooltip>
            <el-tooltip v-if="row.latest_version && !row.pinned" :content="'更新到 ' + row.latest_version" placement="top" effect="dark">
              <el-button
                class="entity-action entity-action-update"
                size="small"
                type="primary"
                circle
                :icon="Download"
                :aria-label="'更新插件 ' + row.name + ' 到 ' + row.latest_version"
                :disabled="globalBusy"
                @click="updatePlugin(row.id)"
              />
            </el-tooltip>
            <el-tooltip v-if="row.repo_url" content="打开仓库" placement="top" effect="dark">
              <el-button
                class="entity-action"
                size="small"
                circle
                :icon="TopRight"
                :aria-label="'在浏览器打开 ' + row.name + ' 的仓库'"
                :disabled="globalBusy"
                @click="openExternalLink(row.repo_url, '仓库地址')"
              />
            </el-tooltip>
            <span class="entity-action-sep" aria-hidden="true"></span>
            <el-popconfirm
              title="确认卸载该插件？"
              confirm-button-text="卸载"
              cancel-button-text="取消"
              width="200"
              @confirm="uninstallPlugin(row.id)"
            >
              <template #reference>
                <el-button
                  class="entity-action entity-action-danger"
                  size="small"
                  circle
                  :icon="Delete"
                  :aria-label="'卸载插件 ' + row.name"
                  :disabled="globalBusy"
                />
              </template>
            </el-popconfirm>
          </div>
        </div>
      </div>
        </el-tab-pane>
      </el-tabs>
      </div>
    </div>
  </section>
</template>

<style scoped>
/* 内嵌插件行：与下方社区插件的 entity-row 同一条基线，但自带留白与
   下缘 2px 刻蚀线（复用 --divider-strong 语义色，几何从简——一行内容
   不值得引六边形）。颜色一律走 token（明暗两主题各取其值）。 */
.builtin-strip {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 4px 2px 10px;
}

.builtin-name {
  font-size: 14px;
  font-weight: 600;
}

.builtin-state {
  font-size: 12px;
  color: var(--text-muted);
  overflow-wrap: anywhere;
}

.builtin-switch {
  margin-left: auto;
}

/* 插件页专属样式。此前 PluginsPanel 没有自己的 scoped 块，插件中心目录
   （.catalog-*）的样式寄在 theme.css 的「插件中心」一节；2026-10-08 用户
   三连要求（版本号挪到条目下方 / 插件中心再紧凑 / 目录列表内部滚动）要动
   这批样式的每一个数，而 theme.css 是反棘轮文件（只许下调），于是整段搬进
   这里，theme.css 只留一条指路注释。
   选择器经 scoped 编译后自带 data-v 属性（(0,2,0) 起步），压得过 theme.css
   的全局基线 (0,1,0)；Element Plus 的内部结构（.el-tabs__content /
   .el-tab-pane）拿不到 data-v，走 :deep()。 */

/* --- 高度链：纵向滚动只发生在列表内部（2026-10-08 用户要求）--------------
   「已安装」页签右栏那份 dshfind.com 目录动辄几千条，此前整列摊开，把
   外层 main 撑出大滚动条。做法与内核版本页（.kernel-panel 一族）同一条：
   面板钉满 main 可视高度 → 卡片吃掉剩余高度 → 页签区伸展 → 双列栅格拉成
   等高两栏 → 列表 overflow-y: auto。min-height: 0 必须一路铺到滚动容器：
   flex 子项默认拒绝收缩到内容高以下，缺任何一环整条链就断，页面照样被
   撑出外层滚动条。
   兜底与内核版本页同一条：窗口矮到静态内容本身都放不下时，列表收缩到
   内容高、main 的滚动条照常出现——可达性优先于「无外层滚动条」。 */
.panel {
  height: 100%;
  min-height: 0;
}

.entity-card {
  flex: 1 1 auto;
  min-height: 0;
}

.installed-tabs-wrap {
  flex: 1 1 auto;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.installed-tabs {
  flex: 1 1 auto;
  min-height: 0;
  /* EP 的 .el-tabs 默认是块级盒（header + content 竖排），改成 flex 列只是
     给 content 一个能伸能量的身份；页签行内部照旧是普通流。 */
  display: flex;
  flex-direction: column;
}

.installed-tabs :deep(.el-tabs__content) {
  flex: 1 1 auto;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.installed-tabs :deep(.el-tab-pane) {
  flex: 1 1 auto;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

/* 单行栅格撑满整个 pane：两栏等高，右栏的目录列表才有一块**确定**的高度
   可以内部滚。全局基线 .page-layout 是 align-items: start（列按内容高收），
   这里必须改回 stretch，否则栅格行高塌到内容高、列表永远滚不起来。 */
.page-layout {
  flex: 1 1 auto;
  min-height: 0;
  grid-template-rows: minmax(0, 1fr);
  align-items: stretch;
}

/* 两份插件清单（左栏「已安装」与「当前内核」页签）放不下时各自内部滚动，
   不把外层撑出去。刻意不给 flex-grow：列表短的时候保持内容高，不会画出
   一个空荡荡的大边框。 */
.entity-list {
  min-height: 0;
  overflow-y: auto;
  overscroll-behavior: contain;
}

/* --- 插件中心右栏：紧凑档（2026-10-08 用户要求）-------------------------- */

/* 两种获取入口各占一个页签，标题沿用刻蚀线。 */
.plugin-center-col .section-divider {
  position: relative;
  margin-top: 0;
  padding-top: 0;
  padding-bottom: 9px;
  border-top: 0;
}

.plugin-center-col .section-divider::before {
  content: '';
  position: absolute;
  left: 0;
  right: 0;
  bottom: 0;
  height: 2px;
  background: var(--divider-strong);
  pointer-events: none;
  clip-path: polygon(0 50%, 20% 0, 80% 0, 100% 50%, 80% 100%, 20% 100%);
}

/* 两栏分界落在 12px 栅格缝中，两端收细，与概览指标的竖线一致。 */
.plugin-center-col {
  position: relative;
}
.plugin-center-col::before {
  content: '';
  position: absolute;
  top: 0;
  bottom: 0;
  left: -7px;
  width: 2px;
  background: var(--divider-strong);
  pointer-events: none;
  clip-path: polygon(50% 0, 100% 22%, 100% 78%, 50% 100%, 0 78%, 0 22%);
}

/* 标题 → 搜索框 → 筛选行 → 列表 的纵向间距统一收到 8px（原 14 / 12 / 6）：
   这一列每省一像素都直接变成列表的可视行数。 */
.plugin-center-col .install-row {
  margin-top: 8px;
}

.catalog-subbar {
  display: flex;
  align-items: center;
  gap: 8px;
  /* 分类 / 排序下拉与搜索框同一档高（30px）：EP 默认 32px，同一列里两种
     盒高差 2px，肉眼读成「没对齐」。 */
  --el-component-size: 30px;
  margin-top: 8px;
}

/* 分类 / 排序 6:4 分宽：分类带计数（「全部（7.6k）」）更占字，排序只有两三个
   固定选项。都不写死 width，让 el-select 自适应伸缩。 */
.catalog-category {
  flex: 6 1 0;
  min-width: 0;
}

.catalog-sort {
  flex: 4 1 0;
  min-width: 0;
}

/* --- 目录列表：内部滚动的本体 -------------------------------------------- */

.catalog-items {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.catalog-list {
  margin-top: 8px;
  /* 结果与「显示更多」共用滚动容器，按钮跟在最后一条结果后面。 */
  flex: 0 1 auto;
  min-height: 0;
  overflow-y: auto;
  /* 触控板甩到列表尽头时不要连带触发外层滚动——否则在列表底部再滚一下，
     页面会跟着跳，用户以为列表没到底。 */
  overscroll-behavior: contain;
}

/* 行式条目：紧凑档——内边距 12 → 8/10，行内间距 5 → 4，条目间距 10 → 6。
   行式条目不做上浮：密集列表里整列 hover 时跳动比省下的高度更抢眼
   （原决定保留）。 */
.catalog-row {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 8px 10px;
  border-radius: 10px;
  border: 1px solid var(--border);
  background: var(--overlay-faint);
  transition: border-color 0.16s ease, background 0.16s ease;
}

.catalog-row:hover {
  border-color: var(--accent-line);
  background: var(--accent-soft);
}

.catalog-row-title {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
}

/* 名称是标题行里唯一会长到溢出的元素，flex-shrink 只给它：分类标签与
   「已验证」徽标内容都短，压它们只会把标签挤成省略号。版本徽标已挪去
   底部（见 .catalog-meta），标题行不再有长版本串挤占名称的宽度。 */
.catalog-name {
  font-weight: 700;
  font-size: 13px;
  flex: 0 1 auto;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* 「已验证」徽标：走 success token——mint 淡彩在浅底上不达对比度的老问题
   已随 2026-10-08 那批清理换成语义色。 */
.catalog-verified {
  flex: none;
  font-size: 10.5px;
  color: var(--success);
  border: 1px solid var(--success);
  border-radius: 999px;
  padding: 0 6px;
}

.catalog-desc {
  margin: 0;
  color: var(--text-secondary);
  font-size: 12px;
  line-height: 1.45;
  display: -webkit-box;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 1;
  line-clamp: 1;
  overflow: hidden;
}

.catalog-row-foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

/* 底部左组：版本徽标 + 热度数据（2026-10-08 用户要求「版本号移动到下方
   显示」）。min-width: 0 是省略号的承重墙——没有它，nowrap 文本撑住
   min-content，长版本串会把右侧的动作按钮挤出去。版本徽标的外观走
   theme.css 的 .meta-version / .catalog-version 共享规则，这里只管排布。 */
.catalog-meta {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  flex: 0 1 auto;
  min-width: 0;
}

.catalog-meta .catalog-version {
  flex: none;
}

.catalog-stats {
  color: var(--text-secondary);
  font-size: 11px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  min-width: 0;
}

.catalog-actions {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  flex: none;
}

/* 目录条目入场：交错淡入上浮（--i 由模板按序号写入，封顶 12 档）。 */
.catalog-enter-active {
  transition: opacity 0.25s ease, transform 0.25s ease;
  transition-delay: calc(min(var(--i, 0), 12) * 22ms);
}
.catalog-enter-from {
  opacity: 0;
  transform: translateY(10px);
}
.catalog-leave-active {
  transition: opacity 0.12s ease;
}
.catalog-leave-to {
  opacity: 0;
}

.catalog-more {
  display: flex;
  justify-content: center;
  margin-top: 4px;
}

/* --- 「当前内核」页签：插件行单行布局（2026-10-08 用户要求）--------------
   版本元数据移到右侧（紧跟状态 tag 与动作按钮），entity-foot 解散后整行只有
   一条基线：名称 + 来源 chip + 描述占左侧吃掉剩余宽度（描述过长照旧省略号
   收尾、悬停看全文），右侧依次是版本、状态、警示点、动作。只覆盖挂了
   .entity-row--single 的行——「已安装」页签仍是名称行 + 实例 chip 行，
   技能页的 .skill-entity-row 是自己的网格，都不受影响。 */
.entity-row--single {
  flex-direction: row;
  align-items: center;
  gap: 10px;
}

.entity-row--single .entity-head {
  flex: 1 1 auto;
}
</style>
