<script setup>
// 插件页：单 card 双 tab——
//   · 当前内核：顶栏选中内核的插件视图——状态取自 PluginRow 的 legacy 字段
//     （wired / synced / actual_mode / quarantined），沿用旧渲染路径；只做管理
//     （同步 / 接线 / 模式切换 / 卸载），不带任何安装入口。
//   · 插件库：手动安装入口 + 本机插件库清单（每个插件 + 每个实例一枚 chip，
//     按 instance_id 在 PluginRow.instances map 里查状态）——安装动作针对的是
//     插件库，不属于某个内核。
import { computed, onMounted, ref } from 'vue';
import {
  Refresh,
  Switch,
  Delete,
  TopRight,
  Download,
  RefreshLeft,
  Box,
  Cpu,
  Link,
  InfoFilled,
  WarningFilled,
  Warning,
  Grid,
} from '@element-plus/icons-vue';
import {
  pluginStore,
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
import {
  builtinStore,
  builtinDegradedNote,
  builtinStateText,
  builtinStateTip,
  builtinUpstreamText,
  checkBuiltinUpstream,
  loadBuiltinStatus,
  toggleBuiltin,
} from './builtin.js';
import OpenaiAccountActions from './OpenaiAccountActions.vue';
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

// 手动安装的唯一动作入口：输入框回车与「预检并安装 / 安装」按钮都走这里。
// 空输入由 installPlugin / precheckPlugin 自己的守卫提示，按钮另做 disabled。
function installFromSpec() {
  return precheckOn.value ? precheckPlugin('') : installPlugin('');
}

async function togglePrecheck(value) {
  precheckBusy.value = true;
  try {
    await setPrecheckEnabled(value);
  } finally {
    precheckBusy.value = false;
  }
}

// 内置插件：随应用交付，不进社区中央库。状态常驻这些行；启停开关的
// 「内核运行中不能改」由后端按实例级判据拒绝，UI 不预判（预判一份就是
// 第二份判据，会和后端漂移）。
//
// **显示名与说明文案全部来自后端**（`displayName` / `summary` / `note`，
// 见 `plugins/builtin/builtins.rs` 的登记表）：前端组件里抄一份的话，改名要
// 同时想起两个地方，而抄的那份不参与任何 Rust 侧测试。`hasAccountCard` 同理——
// 「要不要在工作台里画账户卡」是插件自己的能力，由后端说，模板只画拿到的。
const builtinList = computed(() => builtinStore.list || []);

/**
 * 「当前内核」页签那张列表空不空。
 *
 * 内置插件行现在也是**这个容器里的第一行**，所以判据不能只看社区插件：只有
 * 「社区插件为空 **且** 没有内置插件」才算空。否则会出现一个容器里明明列着
 * 一行、却在下面写着「当前内核尚未接入任何插件」的自相矛盾画面。
 */
const currentKernelListEmpty = computed(() => {
  const rows = view.value && view.value.rows;
  return (!rows || rows.length === 0) && builtinList.value.length === 0;
});

onMounted(() => {
  loadBuiltinStatus();
});

// 存储位置与生效规则原本占整整一段正文（窄窗口下换行成两三行），收进卡片头左
// 侧的小图标气泡里——与技能页保持一致；下方 tab 文字「插件库」与「当前内核」
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
//
// **分成四句而不是一整段**（2026-10-10 用户报「tooltip 要缩减、换行显示、
// 注意宽度控制」）：一整段 100 多字会被 EP 按容器宽度硬折，行尾随时断在
// 「进程存活、端口」这种词的中间，读起来像一句被拦腰截断的话。按语义断成
// 四句之后，即便宽度受限，断点也只会落在句号上——**换行这件事交给宽度控制，
// 但断在哪由文案决定**。段内不再用 `\n`：EP 的 `content` 是纯文本，
// 换行符不会生效（要真换行得走默认插槽塞元素，那是另一套结构）。
const precheckTip =
  '开启后，点「安装」会先在一个一次性沙盒实例里真的装一次、真的启动一次内核，' +
  '确认没问题才装到当前实例。装坏了会原样撤销，你的环境不受影响。' +
  '代价是多花十几秒。' +
  '注意：预检只覆盖内核启动阶段（进程存活、端口监听、HTTP 应答、启动日志），' +
  '工作台页面加载后的运行时异常不在其中。';

// --- 插件库清单（当前内核 tab 的社区插件行也复用这套函数）---

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

// --- P8 #2 双 tab ---
//
// 「当前内核」tab 与旧渲染路径共用同一个 entity-row 模板——状态走 legacy
// 字段（wired / synced / actual_mode / quarantined）。「插件库」tab
// 走 row.instances map，把每个实例的 chip 摆出来，方便对比哪个实例装了
// 哪个没装。
const installedTab = ref('all');
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
const installedTabTip = '手动安装插件，对比各内核的装载状态。';
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
      <!-- 标题行外面还要再包一层 `div.page-head__text`：`.page-head` 是 `space-between` 的
           flex 行，标题与说明必须同属一个子项，否则说明会被当成右侧的「动作」
           顶到页面右缘去。六个页面共用这个结构，别只改这一个。那一层同时承载
           主副标题同行的排布，所以插件页这里是「标题行 → 副标题」两级 flex，
           与其余五页的「主标题 → 副标题」不同，但都收敛到同一条 `.page-head__text`
           规则上。 -->
      <div class="page-head__text">
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
        <p class="page-desc">手动安装插件、检查更新，并按内核同步与启用。</p>
      </div>
    </div>
    <div class="card entity-card">
      <div class="card-head">
        <h2 class="plugin-card-title">
          <span class="card-title-icon" aria-hidden="true"><el-icon><Grid /></el-icon></span>
          <span>插件管理</span>
          <el-tooltip placement="top" effect="dark" :content="installTip">
            <el-icon class="head-tip-icon"><InfoFilled /></el-icon>
          </el-tooltip>
        </h2>
        <!-- 预检开关与说明整体靠在卡头同一行的右端（2026-10-10 用户要求从
             独占的第二行「整体移动到右侧」；`.card-head` 是 space-between，
             标题行左、预检行右）。宽窗下放得下；极窄时靠 .card-head 的
             flex-wrap 自然折回第二行，不需要另写断点。 -->
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
          <!-- 限宽与换行由 theme.css 的 `.el-popper[role='tooltip']` 全局兜住：
               这里曾挂过一个 precheck 专用的 popper 类名，那条规则与全局的逐条同值、
               特异度还更低，是一个看着在管事、实际不生效的死规则，已连同样式一起删。 -->
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

      <!-- 单 card 双 tab，默认激活「插件库」：安装入口 + 本机插件库清单排在
           左侧第一位，「当前内核」随后——安装针对插件库，不属于某个内核。
           两个 tab 共用 pluginStore.view.rows；顶栏切默认实例后，当前内核
           tab 自动跟随新默认实例。 -->
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
              <span class="tab-label">插件库</span>
            </el-tooltip>
          </template>
          <!-- 「插件库」视图：安装入口 + 本机插件库清单。安装是这一页唯一的
               获取动作；写动作（同步 / 模式 / 更新 / 卸载）留在「当前内核」
               tab。清单每行一个插件，按实例列状态 chip，便于对比哪个实例
               装了哪个。 -->
          <div class="plugin-install-block">
            <h3 class="section-divider plugin-install-head">
              <span class="card-title-icon" aria-hidden="true"><el-icon><Download /></el-icon></span>
              安装插件
              <span class="muted section-divider__note">npm 包名 / GitHub 地址</span>
            </h3>
            <div class="install-row">
              <el-input
                v-model="pluginStore.spec"
                placeholder="npm 包名 · @scope/pkg@版本 · GitHub 地址 / owner/repo"
                spellcheck="false"
                clearable
                @keyup.enter="installFromSpec"
              >
                <template #suffix>
                  <span
                    class="muted"
                    :title="precheckOn ? '按 Enter 先做预检，确认后才会装上' : '按 Enter 开始安装'"
                    >↵</span
                  >
                </template>
              </el-input>
              <!-- 显式按钮与回车走同一个函数：动作不再藏在一次按键里。长任务由
                   withProgress 表达在途状态，按钮不挂假 loading。 -->
              <el-button
                class="btn-action"
                type="primary"
                :disabled="globalBusy || !pluginStore.spec.trim()"
                @click="installFromSpec"
              >
                {{ precheckOn ? '预检并安装' : '安装' }}
              </el-button>
            </div>
            <p class="plugin-install-hint">
              支持 npm 包名（可带 @版本）与 GitHub 地址 / owner/repo；开启预检时先在一次性沙盒实例里试装，确认后才写入插件库。安装完成后重启工作台生效。
            </p>
          </div>

          <div class="plugin-library-head">
            <span class="plugin-library-title">已安装</span>
            <span class="muted plugin-library-note">{{ view && view.rows ? view.rows.length : 0 }} 个插件 · 按实例列出装载状态</span>
          </div>
          <div class="entity-list all-instances-list" :class="{ 'is-empty': !view || !view.rows || view.rows.length === 0 }">
            <el-empty
              v-if="!view || !view.rows || view.rows.length === 0"
              description="本机插件库为空；在上方填写 npm 包名或 Git 地址安装第一个插件。"
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
          <div class="entity-list" :class="{ 'is-empty': currentKernelListEmpty }">
        <!-- 内置插件：与社区插件**同一张列表里的第一行**（2026-10-09
             用户截图「让 UI 符合插件安装后的风格」）。此前它是列表外面一条自带
             下缘刻蚀线的裸行，没有卡片的边框与圆角，看上去像另一个东西；并进来
             之后它与下面那些行共用同一套外壳：边框、圆角、行分隔线、名称字号与
             等宽字体、来源 chip、左右节奏。
             差别只剩**右侧动作**：它没有安装 / 更新 / 卸载，提供账号操作与启停开关——它随
             应用交付、不进社区中央库。接线行 id 同时是内核模型设置页账户卡的
             settingsNs，两侧靠它对上。

             2026-10-12 起这里是 **v-for**：内置插件不止一个了。`hasAccountCard`
             决定要不要在工作台那一侧再画账户动作——那是插件自己的能力，模板不
             认得哪个 id 是哪个。 -->
        <div
          v-for="builtin in builtinList"
          :key="builtin.id"
          class="entity-row entity-row--single builtin-row"
        >
          <div class="entity-head">
            <span class="entity-name">{{ builtin.displayName }}</span>
            <!-- 来源 chip 挂 `origin-chip-builtin`：紫色 + 放大一档的「内嵌」标记，
                 后端登记表里每加一个内置插件都自动带上这一枚（用户 2026-10-13 定，
                 此后内置插件统一用这个颜色标记）。样式在 theme.css，
                 **不要在这里写内联色**——两套主题各一份值，抄一份就必然漏一套。 -->
            <span class="origin-chip origin-chip-builtin">
              <el-icon class="origin-chip-icon"><Cpu /></el-icon>
              <span class="origin-chip-label">内嵌</span>
            </span>
            <el-tooltip placement="top" effect="dark" :content="builtin.note">
              <el-icon class="head-tip-icon" :aria-label="builtin.note" tabindex="0"
                ><InfoFilled /></el-icon
              >
            </el-tooltip>
            <el-tooltip placement="top" effect="dark" :content="builtinStateTip(builtin)">
              <span class="entity-desc">{{ builtinStateText(builtin) }}</span>
            </el-tooltip>
            <!-- 本内核版本不带某个**可选** peer（如 ponytail 的 hook 桥）时多一枚 ⚠：
                 插件装得上、技能照常注册，只是少一项能力。做成图标 tooltip 而不是
                 常驻整行文字——这一行的主状态仍是左边那句，降级是「有件事你该知道」。
                 文案由后端下发（`missingPeers[].note`），前端不拼：拼一份就与登记表
                 漂移，而漂移的那份不参与任何 Rust 侧测试。 -->
            <el-tooltip
              v-if="builtinDegradedNote(builtin)"
              placement="top"
              effect="dark"
              :content="builtinDegradedNote(builtin)"
            >
              <el-icon
                class="head-tip-icon head-tip-icon--warning"
                :aria-label="builtinDegradedNote(builtin)"
                tabindex="0"
                ><WarningFilled
              /></el-icon>
            </el-tooltip>
          </div>
          <div class="entity-actions">
            <OpenaiAccountActions v-if="builtin.hasAccountCard" :plugin-view="builtin" />
            <!-- 上游内容不随应用走的插件（ponytail）多两件东西：版本号与一个
                 「检查更新」。**版本号常显**是因为「我用的到底是哪一版」是这类
                 插件最常被问的问题，而它不在任何一个已有字段里。 -->
            <el-tooltip v-if="builtin.upstream" placement="top" effect="dark"
              :content="builtinUpstreamText(builtin)">
              <span class="entity-desc entity-desc--upstream">{{ builtinUpstreamText(builtin) }}</span>
            </el-tooltip>
            <el-button
              v-if="builtin.upstream"
              size="small"
              plain
              :loading="isLoading('builtinUpstream:' + builtin.id)"
              :aria-label="'检查 ' + builtin.displayName + ' 的上游更新'"
              @click="checkBuiltinUpstream(builtin.id)"
            >
              检查更新
            </el-button>
            <el-switch
              size="small"
              :model-value="builtin.requestedEnabled"
              :loading="isLoading('builtinToggle:' + builtin.id)"
              :disabled="!builtin.pluginSourceAvailable || !!builtin.stateError"
              :aria-label="'启用或停用 ' + builtin.displayName"
              @change="(value) => toggleBuiltin(builtin.id, value)"
            />
          </div>
        </div>
        <!-- 首次状态未返回时显示骨架：view===null 是「加载中」而不是
             「尚未安装」，画成空态会让用户以为插件全丢了。 -->
        <template v-if="!view">
          <div v-for="i in 2" :key="'skeleton-' + i" class="entity-row">
            <el-skeleton :rows="1" animated style="width: 55%" />
          </div>
        </template>
        <el-empty v-else-if="currentKernelListEmpty" description="当前内核尚未接入任何插件；先到「插件库」页签安装。" :image-size="48" />
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
                :title="'恢复启用 ' + row.name"
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
                :title="'更新插件 ' + row.name + ' 到 ' + row.latest_version"
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
                :title="'在浏览器打开 ' + row.name + ' 的仓库'"
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
                  :title="'卸载插件 ' + row.name"
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
/* 内嵌插件行现在就是一条普通的 .entity-row（见模板里的 builtin-row），视觉
   全由 theme.css 的 entity-* 提供——这里曾经有一整套 .builtin-strip /
   .builtin-name / .builtin-state（自带 padding + 下缘刻蚀线 + 自己的字号），
   那是「它不属于这个列表」的实现。并进列表后那套规则就成了**没有任何作用方的
   声明**：留着只会让下一个人以为内嵌行另有样式。 */

/* 插件页专属样式：高度链、安装区块与插件库块头都是单页关注点，按
   ui/AGENTS.md「单页专用样式留在 scoped」的约定放这里；theme.css 是反棘轮
   文件（只许下调），不再往全局加这些规则。
   选择器经 scoped 编译后自带 data-v 属性（(0,2,0) 起步），压得过 theme.css
   的全局基线 (0,1,0)；Element Plus 的内部结构（.el-tabs__content /
   .el-tab-pane）拿不到 data-v，走 :deep()。 */

/* --- 高度链：纵向滚动只发生在清单内部 ------------------------------------
   做法与内核版本页（.kernel-panel 一族）同一条：面板钉满 main 可视高度 →
   卡片吃掉剩余高度 → 页签区伸展 → 安装区块固定高、插件库清单
   overflow-y: auto。min-height: 0 必须一路铺到滚动容器：flex 子项默认拒绝
   收缩到内容高以下，缺任何一环整条链就断，页面照样被撑出外层滚动条。
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

/* 安装区块固定高，清单吃剩余空间；清单仍保持内容高、超长时内部滚动。
   刻意不给 .entity-list 加 flex-grow：行少时保持内容高，不画一个空荡荡
   的大边框（与「当前内核」页签一致）。 */
.entity-list {
  min-height: 0;
  overflow-y: auto;
  overscroll-behavior: contain;
}

/* --- 插件库页签：安装区块 ----------------------------------------------- */

.plugin-install-block {
  flex: none;
  display: flex;
  flex-direction: column;
}

/* 块头沿用 .section-divider 的 flex 排布，但去掉它默认的虚线上边框，改用
   卡片头同款的 2px 六边形收细刻蚀线（线在标题下方，不压住页签栏）。 */
.plugin-install-head {
  position: relative;
  margin-top: 0;
  padding-top: 0;
  padding-bottom: 9px;
  border-top: 0;
}

.plugin-install-head::before {
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

.plugin-install-block .section-divider__note {
  margin-left: auto;
}

.plugin-install-block .install-row {
  margin-top: 12px;
}

.plugin-install-hint {
  margin: 9px 0 0;
  color: var(--text-muted);
  font-size: 12px;
  line-height: 1.6;
}

/* --- 插件库页签：已安装清单块头 ----------------------------------------- */

.plugin-library-head {
  flex: none;
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 16px;
  padding-top: 12px;
  border-top: 1px solid var(--border-soft);
}

.plugin-library-title {
  font-size: var(--fs-subtitle);
  font-weight: 600;
  color: var(--text-secondary);
}

.plugin-library-note {
  font-size: 12px;
}

/* --- 「当前内核」页签：插件行单行布局（2026-10-08 用户要求）--------------
   版本元数据移到右侧（紧跟状态 tag 与动作按钮），entity-foot 解散后整行只有
   一条基线：名称 + 来源 chip + 描述占左侧吃掉剩余宽度（描述过长照旧省略号
   收尾、悬停看全文），右侧依次是版本、状态、警示点、动作。只覆盖挂了
   .entity-row--single 的行——「插件库」页签仍是名称行 + 实例 chip 行，
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
