<script setup>
// 一份完整插件清单：内置在前、自装在后，同一张表、同一套五列；每行按内核展示
// 真实接入状态——可操作的才给可用复选框，不支持的写「待适配」。
import PageHeadIcon from '../shell/PageHeadIcon.vue';
import { mcodeExperience } from '../kernel/experience.js';
import { loadMcodeBuiltins, mcode, refreshMcodeStatus, checkMcodePonytailUpdate, upgradeMcodePonytail } from '../kernel/mcode.js';
import { computed, ref, watch } from 'vue';
import {
  Refresh,
  Switch,
  Delete,
  TopRight,
  Download,
  RefreshLeft,
  Box,
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
import MinimaxPluginTarget from './MinimaxPluginTarget.vue';
import { globalBusy, isLoading, withLoading } from '../shell/loading.js';
import { openExternalLink } from '../shell/notify.js';
import { store } from '../store.js';

const view = computed(() => pluginStore.view);
// MiniMax 目标入口跟「体验 MiniMax」开关走，不跟随当前工作台筛选清单。

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

// 当前内核的社区插件行：与内置行同表，内置在前、自装在后。
const communityRows = computed(() => (view.value && view.value.rows) || []);

function reloadTargets() {
  return withLoading('pluginTargets', () => Promise.all([
    loadBuiltinStatus(),
    ...(mcodeExperience.value ? [refreshMcodeStatus(), loadMcodeBuiltins()] : []),
  ]));
}
// 体验偏好可能在首个 get_status 之后才到：只做 onMounted 一次会让 MiniMax
// 目标列出现而 catalog / 状态为空。跟偏好走，开启时补拉一次。
watch(mcodeExperience, () => reloadTargets(), { immediate: true });

// 内置插件行只有一枚「检查更新」，它更新的是**上游 Ponytail 本身**，不是两个
// 内核各一份的接入层。MiniMax 侧因此不再挂第二枚更新按钮：同一次检查在 MiniMax
// 的 Ponytail 已启用、工作台已停止时，把同一份上游内容一并应用过去；运行中
// （需先停止）不能替换实例里的插件文件，等停止后再点这一枚即可。
async function checkBuiltinUpstreamAll(builtin) {
  await checkBuiltinUpstream(builtin.id);
  if (builtin.id !== 'ponytail' || !mcodeExperience.value) return;
  const mcodeView = mcode.view;
  if (!mcode.view?.instance_id || !mcodeView.ponytail_enabled || mcodeView.ponytail_error || mcodeView.running || mcodeView.stop_required || mcode.error) return;
  if (!(await checkMcodePonytailUpdate())) return;
  if (mcode.ponytailUpdate?.update_available) await upgradeMcodePonytail();
}

// 存储位置与生效规则原本占整整一段正文（窄窗口下换行成两三行），收进卡片头左
// 侧的小图标气泡里——与技能页保持一致；下方各区块标题自身即可承担章节名，
// 卡片头不再单独挂「已安装」标题。
// 存储位置读后端返回的真实路径（`PluginStatus.store_root`），与技能页的
// `storeTip` 同一做法。此前这里写死 `~/.dsh-xlink/dsh-plugins/`——那是
// legacy 路径，`store_relocate` 早已把中央库整体搬进 `plugins/dsh/`，用户
// 照着提示去找会找到一个不存在的目录。同一份路径在 `paths.rs` 与 Vue 里各写
// 一遍，前端那份不参与编译也不会报错，只能靠人看出来。
const installTip = computed(() => {
  const store = tildePath(view.value && view.value.store_root) || '~/.dsh-xlink/plugins/dsh/';
  return (
    '插件统一存放于 ' + store +
    '，切换内核无需重装；勾选内嵌插件的目标即启用接入，取消勾选只停用，不卸载。手动安装仅支持 DSH，完成后重启内核生效。'
  );
});

// 第三方来源免责提示的文案。提到常量而不是写在模板里：它是「标题旁那个
// ⚠ 是什么」的答案，tooltip 的 `content` 与图标的可访问名（`aria-label`）要用
// 同一份字符串——两处各写一遍，改了其中一处就会出现「图标说有提示，读屏说没提示」。
const THIRD_PARTY_NOTICE = '第三方插件由社区提供，本工具不对其安全性负责，请自行甄别。';
const PAGE_DESC = '查看内置插件，手动安装插件，并管理各内核已安装插件的更新与同步。';

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
  '开启后，点「安装」会先做兼容性预检：在一次性沙盒实例里真的装一次、真的启动一次内核，' +
  '确认能装上并能起来才允许写入插件库。' +
  '沙盒只是临时实例与目录隔离，不是操作系统级沙盒；Git 来源若声明 prepare，会以当前用户权限执行依赖安装与构建脚本，可能访问该用户可读的文件。' +
  '预检覆盖取源校验、构建、插件规范、物化接线与内核启动探测；工作台页面加载后的运行时异常不在其中。' +
  '分阶段结果会在报告里逐项列出，未执行的阶段不会画成通过。';

// --- 当前内核插件行的展示函数 ---

function quarantineNote(row) {
  const reason = String(row.quarantined.reason || '');
  return '已隔离：' + (reason.length > 60 ? reason.slice(0, 57) + '…' : reason);
}

// 待同步 / 待接线的原因：只在有活动内核时才成立（没有内核时状态 chip 已经
// 说明「无活动内核」），返回文案用于状态格里的警示点 tooltip。
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


// 描述性文案统一收进 tooltip：安装面板的规则、同步/更新的作用范围不常驻界面，
// 避免把列表铺成说明文（用户 2026-10-31 要求）。
const installPanelTip =
  '支持 npm 包名（可带 @版本）与 GitHub 地址 / owner/repo。' +
  '开启兼容性预检时先在一次性沙盒实例里试装，确认后才写入插件库；安装完成后重启工作台生效。';
const installActionsTip =
  '同步内核会把中央插件库重新物化到所有已安装内核；检查更新只检查手动安装的插件。';
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
           顶到页面右缘去。六个页面共用这个结构，别只改这一个。 -->
      <div class="page-head__text"><PageHeadIcon :page="'plugins'" />
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
        <el-tooltip placement="bottom-start" effect="dark" :content="PAGE_DESC">
          <el-icon class="head-tip-icon" tabindex="0" role="note" :aria-label="PAGE_DESC"><InfoFilled /></el-icon>
        </el-tooltip>
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
        <!-- 列表标题右侧：兼容性检查与只作用于自装插件的同步 / 检查更新。
             空间不足时整组换行，不挤标题。 -->
        <div class="plugin-head-actions">
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
            <el-icon class="head-tip-icon" :aria-label="precheckTip" tabindex="0">
              <InfoFilled />
            </el-icon>
          </el-tooltip>
          <span class="plugin-head-action-btns">
            <el-button size="small" :icon="Switch" :disabled="globalBusy" @click="syncPlugins">
              同步内核
            </el-button>
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
          <el-tooltip placement="top" effect="dark" :content="installActionsTip">
            <el-icon class="head-tip-icon" :aria-label="installActionsTip" tabindex="0">
              <InfoFilled />
            </el-icon>
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

      <el-alert v-if="builtinStore.error" type="error" :closable="false" :title="'DSH 状态读取失败：' + builtinStore.error" />
      <el-alert v-if="mcodeExperience && (mcode.error || mcode.catalogError)" type="error" :closable="false" :title="'MiniMax 状态读取失败：' + (mcode.error || mcode.catalogError)" />
      <el-button v-if="builtinStore.error || (mcodeExperience && (mcode.error || mcode.catalogError))"
        :loading="isLoading('pluginTargets')" :disabled="globalBusy" @click="reloadTargets">重新读取状态</el-button>

      <!-- 统一列表（2026-11-10 用户明确要求）：内置在前、自装在后，同一张表。
           五列按内容宽分配（名称及用途 260 / 状态与版本 minmax(120,1fr) /
           DeepSeek 104 / MiniMax 104 / 操作 180，列宽含左右各 8px 内边距）；
           内容宽 ≤767 时改为两行，不隐藏任何一个内核列。
           自装为 0 时只剩内置行，不再画「暂无自装插件」空框。 -->
      <div class="plugin-table" :class="{ 'has-minimax': mcodeExperience }">
        <div class="plugin-table-head">
          <span>名称及用途</span>
          <span>状态与版本</span>
          <span><span class="deepseek-wordmark">DeepSeek</span></span>
          <span v-if="mcodeExperience"><span class="minimax-wordmark">MiniMax</span></span>
          <span>操作</span>
        </div>
        <div class="entity-list">
          <!-- 内置插件行（随应用交付，不进社区中央库）：文案与能力全部来自后端。
               内置插件不止一个，所以是 v-for；按 id 取状态，不许退化成「第一项」。 -->
          <div
            v-for="builtin in builtinList"
            :key="'builtin:' + builtin.id"
            class="entity-row plugin-row builtin-row"
          >
            <div class="plugin-name-cell">
              <span class="entity-name">{{ builtin.displayName }}</span>
              <el-tooltip placement="top" effect="dark" :content="builtin.note">
                <el-icon class="head-tip-icon" :aria-label="builtin.note" tabindex="0"
                  ><InfoFilled /></el-icon>
              </el-tooltip>
              <span class="plugin-builtin-badge">内置</span>
            </div>
            <div class="plugin-status-cell">
              <el-tooltip placement="top" effect="dark" :content="builtinStateTip(builtin)">
                <span class="plugin-status-text">{{ builtinStateText(builtin) }}</span>
              </el-tooltip>
              <!-- 本内核版本不带某个**可选** peer 时多一枚 ⚠：插件装得上、技能照常
                   注册，只是少一项能力。文案由后端下发（`missingPeers[].note`），
                   前端不拼——拼一份就与登记表漂移。 -->
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
              <!-- 有上游的插件（ponytail）显示版本号：它不在任何一个已有字段里，
                   「我用的到底是哪一版」是这类插件最常被问的问题。 -->
              <el-tooltip v-if="builtin.upstream" placement="top" effect="dark"
                :content="builtinUpstreamText(builtin)">
                <span class="plugin-version">{{ builtinUpstreamText(builtin) }}</span>
              </el-tooltip>
            </div>
            <div class="plugin-target-cell">
              <el-checkbox
                size="small"
                :model-value="builtin.requestedEnabled"
                :disabled="globalBusy || isLoading('builtinToggle:' + builtin.id) || !!builtinStore.error || !builtin.pluginSourceAvailable || !!builtin.stateError"
                :aria-label="'DeepSeek 启用或停用 ' + builtin.displayName"
                @change="(value) => toggleBuiltin(builtin.id, value)"
              ><span class="deepseek-wordmark">{{ isLoading('builtinToggle:' + builtin.id) ? 'DeepSeek · 处理中' : 'DeepSeek' }}</span></el-checkbox>
            </div>
            <!-- MiniMax 目标控件自带 `v-if="mcodeExperience"`：体验关着时整列都没渲染，
                 不会在表里留下一个「可用但没勾」的空白格。 -->
            <MinimaxPluginTarget v-if="mcodeExperience" :plugin-id="builtin.id" :name="builtin.displayName" />
            <div class="plugin-ops-cell">
              <OpenaiAccountActions v-if="builtin.hasAccountCard" :plugin-view="builtin" />
              <!-- 记忆权限统一在记忆页修改：这一行只给一个入口。 -->
              <el-button v-if="builtin.id === 'memory'" size="small" plain @click="store.activePanel = 'memory'">记忆设置</el-button>
              <el-button
                v-if="builtin.upstream"
                size="small"
                plain
                :disabled="globalBusy"
                :loading="isLoading('builtinUpstream:' + builtin.id)"
                :aria-label="'检查 ' + builtin.displayName + ' 的上游更新'"
                @click="checkBuiltinUpstreamAll(builtin)"
              >
                检查更新
              </el-button>
              <span v-if="!builtin.hasAccountCard && !builtin.upstream && builtin.id !== 'memory'" class="plugin-none">—</span>
            </div>
          </div>

          <!-- 首次状态未返回时显示骨架：view===null 是「加载中」而不是
               「尚未安装」，画成空态会让用户以为插件全丢了。 -->
          <template v-if="!view">
            <div v-for="i in 2" :key="'skeleton-' + i" class="entity-row plugin-row plugin-row--skeleton">
              <el-skeleton :rows="1" animated style="width: 55%" />
            </div>
          </template>

          <!-- 自装插件行（DSH 中央库）：紧接内置行、同一张表。
               自装 DeepSeek 目前只有只读的「已安装」状态——没有通用停用动作，
               取消勾选绝不可调用卸载；移除仍只走右侧卸载按钮 + 确认。 -->
          <div
            v-for="row in view ? view.rows : []"
            :key="'community:' + row.id"
            class="entity-row plugin-row"
            :class="{ 'is-warn': !!row.quarantined }"
          >
            <div class="plugin-name-cell">
              <span class="entity-name">{{ row.name }}</span>
              <el-tooltip v-if="row.description" placement="top" effect="dark" :content="row.description">
                <el-icon class="head-tip-icon" :aria-label="row.description" tabindex="0"
                  ><InfoFilled /></el-icon>
              </el-tooltip>
              <span class="origin-chip" :class="'origin-chip-' + row.origin">
                <el-icon class="origin-chip-icon">
                  <Box v-if="row.origin === 'npm'" />
                  <Link v-else />
                </el-icon>
                <span class="origin-chip-label">{{ originLabel(row.origin) }}</span>
              </span>
            </div>
            <div class="plugin-status-cell">
              <el-tooltip v-if="row.quarantined" placement="top" effect="dark" :content="quarantineNote(row)">
                <span class="entity-warn"><el-icon><WarningFilled /></el-icon> 已停用</span>
              </el-tooltip>
              <span class="plugin-version">{{ row.installed_version }}</span>
              <span v-if="row.latest_version" class="meta-upgrade">→ {{ row.latest_version }}</span>
              <span v-if="row.pinned" class="meta-pinned">已锁定版本</span>
              <el-tag v-if="!view || !view.active_kernel" type="warning" size="small" effect="plain">无活动内核</el-tag>
              <el-tag v-else-if="row.synced && row.wired" type="success" size="small" effect="plain">已同步</el-tag>
              <!-- 待同步 / 待接线只在状态格里点一个警示点，原因走 tooltip。 -->
              <span v-if="syncWarning(row)" class="state-dot">
                <el-tooltip placement="top" effect="dark" :content="syncWarning(row)">
                  <el-icon><WarningFilled /></el-icon>
                </el-tooltip>
              </span>
            </div>
            <div class="plugin-target-cell">
              <el-tooltip placement="top" effect="dark" content="已安装到 DeepSeek；暂不支持单独停用，移除请用本行右侧的卸载按钮并确认。">
                <span class="target-control" tabindex="0" aria-label="已安装到 DeepSeek；移除需确认">
                  <el-checkbox size="small" :model-value="true" disabled aria-label="DeepSeek 已安装"><span class="deepseek-wordmark">DeepSeek</span></el-checkbox>
                  <span class="target-state">已安装</span>
                </span>
              </el-tooltip>
            </div>
            <MinimaxPluginTarget v-if="mcodeExperience" :plugin-id="'community:' + row.id" :name="row.name" />
            <div class="plugin-ops-cell entity-actions">
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
      </div>

      <!-- 安装插件：只作用于插件库，必须留在列表下方的安装区块里。
           标题行右侧不再放同步 / 检查更新（它们属列表的维护动作，已上移到卡头）。
           说明性文字全部收进标题与动作旁的 ⓘ tooltip。 -->
      <section class="plugin-install-panel">
        <div class="plugin-install-heading">
          <span class="card-title-icon" aria-hidden="true"><el-icon><Download /></el-icon></span>
          <h3 class="plugin-install-title">
            安装插件
            <span class="origin-chip origin-chip-kernel deepseek-wordmark">DeepSeek</span>
            <el-tooltip placement="top" effect="dark" :content="installPanelTip">
              <el-icon class="head-tip-icon" :aria-label="installPanelTip" tabindex="0">
                <InfoFilled />
              </el-icon>
            </el-tooltip>
          </h3>
        </div>
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
                :title="precheckOn ? '按 Enter 先做兼容性预检，确认后才会装上' : '按 Enter 开始安装'"
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
            {{ precheckOn ? '兼容性预检并安装' : '安装' }}
          </el-button>
        </div>
      </section>
    </div>
  </section>
</template>

<style scoped>
/* 插件页专属样式：五列表与安装面板都是单页关注点，按 ui/AGENTS.md
   「单页专用样式留在 scoped」的约定放这里；theme.css 是反棘轮文件（只许下调），
   不再往全局加这些规则。选择器经 scoped 编译后自带 data-v 属性（(0,2,0) 起步），
   压得过 theme.css 的全局基线 (0,1,0)。 */

/* --- 五列表头与行 ---------------------------------------------------------
   列宽按 Kami 合同 §7.1：名称及用途 260 / 状态与版本 minmax(120,1fr) /
   DeepSeek 104 / MiniMax 104 / 操作 180，列宽含左右各 8px 内边距（border-box），
   不再额外叠加 grid-gap；最小总宽 768。体验关着时不渲染 MiniMax 列。 */
.plugin-table {
  container-type: inline-size;
}

.plugin-table-head,
.plugin-row {
  grid-template-columns: 260px minmax(120px, 1fr) 104px 180px;
}

.plugin-table.has-minimax .plugin-table-head,
.plugin-table.has-minimax .plugin-row {
  grid-template-columns: 260px minmax(120px, 1fr) 104px 104px 180px;
}

.plugin-table-head {
  display: grid;
  align-items: center;
  min-height: 32px;
  border-bottom: 1px dotted var(--border);
  color: var(--text-muted);
  font-size: var(--fs-4);
}

.plugin-table-head > span {
  padding: 0 8px;
  min-width: 0;
}

/* 行：网格覆盖 .entity-row 的 flex 布局，hover 底板与行分隔线仍由它提供。
   min-height 56 是合同下限，内容多时自然长高，不裁字。 */
.plugin-row {
  display: grid;
  align-items: center;
  gap: 0;
  padding: 0;
  min-height: 56px;
}

.plugin-row > * {
  padding: 8px;
  min-width: 0;
}

/* 首次读取的骨架不参与五列：整行一条灰条即可。 */
.plugin-row--skeleton {
  display: block;
  padding: 12px 8px;
  min-height: 0;
}

/* --- 各列内容 -------------------------------------------------------------
   名称及用途：名称 + 用途 ⓘ + 「内置」徽标；自装行另带来源 chip。
   状态与版本：行内状态词、版本 / 升级、隔离与同步异常都在这一格。 */
.plugin-name-cell {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 6px;
}

.plugin-name-cell .entity-name {
  flex-shrink: 1;
  overflow-wrap: anywhere;
  /* Kami 合同 §3.1：数据行 14px。 */
  font-size: var(--fs-5);
}

.plugin-status-cell {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 4px 8px;
  color: var(--text-secondary);
  font-size: var(--fs-4);
}

.plugin-status-text {
  min-width: 0;
}

.plugin-version {
  color: var(--text-muted);
  font-family: var(--font-mono);
  font-size: var(--fs-4);
}

/* 「内置」徽标：只给内置项，走 --builtin / --builtin-fill（两套主题各一份），
   字号 12px 是 Kami 合同的徽标档。自装项不另加「自装」徽标。 */
.plugin-builtin-badge {
  flex-shrink: 0;
  padding: 1px 6px;
  border: 1px solid var(--builtin);
  border-radius: var(--radius-control);
  background: var(--builtin-fill);
  color: var(--builtin);
  font-size: var(--fs-3);
  font-weight: 600;
  line-height: 1.5;
}

.plugin-target-cell {
  display: flex;
  align-items: center;
  min-width: 0;
}

/* 自装行的只读 DeepSeek 目标：复选框 + 「已安装」短状态同行。 */
.plugin-target-cell .target-control {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}

.plugin-target-cell .target-state {
  color: var(--text-muted);
  font-size: var(--fs-3);
}

.plugin-ops-cell {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 4px;
}

.plugin-none {
  color: var(--text-muted);
  font-size: var(--fs-4);
}

/* --- 窄内容（≤767）：每行两排 --------------------------------------------
   第一排名称 + 状态 + 操作，第二排两个内核目标；不隐藏任何目标列。
   生产窗口内容宽 835（≥768）走五列，这条只在窗口/侧栏收窄时生效。 */
@container (max-width: 767px) {
  .plugin-table-head {
    display: none;
  }

  /* 第一行名称 + 状态 + 操作，第二行两个内核目标；两个目标列都在，不隐藏。 */
  .plugin-row,
  .plugin-table.has-minimax .plugin-row {
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr) auto;
    grid-template-areas:
      'name status ops'
      'deepseek deepseek deepseek';
    row-gap: 4px;
  }

  .plugin-table.has-minimax .plugin-row {
    grid-template-areas:
      'name status ops'
      'deepseek minimax minimax';
  }

  .plugin-name-cell {
    grid-area: name;
  }

  .plugin-status-cell {
    grid-area: status;
  }

  .plugin-target-cell {
    grid-area: deepseek;
  }

  .plugin-row > .minimax-target {
    grid-area: minimax;
  }

  .plugin-ops-cell {
    grid-area: ops;
    align-self: start;
    min-width: 0;
  }
}

/* --- 安装面板（列表下方）--------------------------------------------------
   标题行 + 输入行；与上方列表之间用一条点线分隔。 */
.plugin-install-panel {
  display: flex;
  flex-direction: column;
  padding: 14px 0;
  border: 0;
  border-top: 1px dotted var(--border);
  border-radius: 0;
  background: transparent;
}

.plugin-install-heading {
  display: flex;
  align-items: center;
  gap: 10px;
  min-width: 0;
  flex-wrap: wrap;
  row-gap: 8px;
}

.plugin-install-heading .card-title-icon {
  flex: none;
}

.plugin-install-title {
  display: flex;
  align-items: center;
  gap: 6px;
  margin: 0;
}

.plugin-install-panel .install-row {
  margin-top: 14px;
}

/* 卡头右侧：兼容性检查 + 自装范围的同步 / 检查更新。空间不足时整组换行。 */
.plugin-head-actions {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
  justify-content: flex-end;
}

.plugin-head-action-btns {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}
</style>
