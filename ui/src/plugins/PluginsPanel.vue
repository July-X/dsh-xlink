<script setup>
// 插件页（单视图，无页签）：
//   · 顶部工具条：当前内核身份 + 同步 / 检查更新；
//   · 内置插件：随应用交付、不可卸载，单独一块；
//   · 安装插件：本页唯一的获取入口，紧接在内置插件下方；
//   · 已安装插件：当前内核的社区插件行——状态取自 PluginRow 的 legacy 字段
//     （wired / synced / actual_mode / quarantined），只做管理
//     （同步 / 接线 / 模式切换 / 卸载）。
// 2026-10-31 二次调整：用户要求移除「插件库」页签，安装入口直接落到内置插件下方。
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

// 当前内核的社区插件行（内置插件单独一块，不在这个列表里）。
const communityRows = computed(() => (view.value && view.value.rows) || []);
const communityListEmpty = computed(() => communityRows.value.length === 0);

onMounted(() => {
  loadBuiltinStatus();
});

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

// --- 当前内核插件行的展示函数 ---

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

// 「当前内核」是个相对概念：内置插件块头的 tooltip 里带上身份与活动版本号
// （与概览页「活动版本」同源）。注册表实例 id（如 default）是实现细节，
// 与顶栏内核 tab 同口径不对外展示。
const currentInstanceLabel = computed(() => {
  const def = instanceStore.list.find((item) => item.is_default);
  if (!def) return '';
  const family = familyLabel(def.record.kernel_family);
  const active = store.view && store.view.kernel && store.view.kernel.active;
  return active ? `${family} · ${active}` : family;
});

// 描述性文案统一收进 tooltip：安装面板的规则、同步/更新的作用范围、内置插件
// 的交付方式都不常驻界面，避免把列表铺成说明文（用户 2026-10-31 要求）。
const builtinSectionTip = computed(() => {
  const kernel = currentInstanceLabel.value ? `当前内核：${currentInstanceLabel.value}。` : '';
  return `${kernel}内置插件随应用交付、不占中央插件库；启停后重启工作台生效。`;
});
const installPanelTip =
  '支持 npm 包名（可带 @版本）与 GitHub 地址 / owner/repo。' +
  '开启预检时先在一次性沙盒实例里试装，确认后才写入插件库；安装完成后重启工作台生效。';
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
        <p class="page-desc">查看内置插件，手动安装插件，并管理当前内核的更新与同步。</p>
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
        <!-- 预检开关靠在卡头右端；「安装前先在沙盒实例里试装」这类说明收进
             开关旁的 ⓘ tooltip，界面不再铺一整句描述。 -->
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
            <el-icon class="head-tip-icon" :aria-label="precheckTip" tabindex="0">
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

      <!-- 内置插件：随应用交付、不占中央库，单独一块；安装入口紧接其下。
           当前内核身份从顶部工具条挪到这一行的说明里——它说明的是「这些内置
           插件属于哪个内核」，而不是安装动作的语境。 -->
      <template v-if="builtinList.length">
        <div class="plugin-section-head">
          <span class="plugin-section-title">内置插件</span>
          <el-tooltip placement="top" effect="dark" :content="builtinSectionTip">
            <el-icon class="head-tip-icon" :aria-label="builtinSectionTip" tabindex="0">
              <InfoFilled />
            </el-icon>
          </el-tooltip>
        </div>
        <div class="entity-list plugin-builtin-list">
        <!-- 内置插件（2026-10-31 二次调整后单独成块，紧接其后就是安装入口）：
             行外壳复用 .entity-row--single 与社区插件同一套（边框、圆角、行分隔
             线、名称字号与等宽字体、来源 chip、左右节奏），差别只在右侧动作：
             没有安装 / 更新 / 卸载，提供账号操作与启停开关——它随应用交付、
             不进社区中央库。接线行 id 同时是内核模型设置页账户卡的 settingsNs，
             两侧靠它对上。

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
        </div>
      </template>

      <!-- 安装插件（2026-10-31 三次调整 + 微调）：用户指出安装区块高度太低，且
           「同步内核 / 检查更新」针对的是手动安装的插件，不该挂在页面顶部工具条上。
           这里把安装做成一个完整的浅色面板：标题行（右侧是两枚动作）+ 输入行；
           说明性文字全部收进标题与动作旁的 ⓘ tooltip。 -->
      <section class="plugin-install-panel">
        <div class="plugin-install-heading">
          <span class="card-title-icon" aria-hidden="true"><el-icon><Download /></el-icon></span>
          <h3 class="plugin-install-title">
            安装插件
            <el-tooltip placement="top" effect="dark" :content="installPanelTip">
              <el-icon class="head-tip-icon" :aria-label="installPanelTip" tabindex="0">
                <InfoFilled />
              </el-icon>
            </el-tooltip>
          </h3>
          <span class="plugin-install-head-actions">
            <span class="plugin-install-action-btns">
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
          </span>
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
      </section>

      <div class="plugin-section-head">
        <span class="plugin-section-title">已安装插件</span>
        <span class="muted plugin-section-note">{{ communityRows.length }} 个 · 只管理当前内核</span>
      </div>
      <div class="entity-list" :class="{ 'is-empty': view && communityListEmpty }">
        <!-- 首次状态未返回时显示骨架：view===null 是「加载中」而不是
             「尚未安装」，画成空态会让用户以为插件全丢了。 -->
        <template v-if="!view">
          <div v-for="i in 2" :key="'skeleton-' + i" class="entity-row">
            <el-skeleton :rows="1" animated style="width: 55%" />
          </div>
        </template>
        <p v-else-if="communityListEmpty" class="plugin-empty-hint muted">
          当前内核尚未接入其他插件。
        </p>
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
    </div>
  </section>
</template>

<style scoped>
/* 内嵌插件行现在就是一条普通的 .entity-row（见模板里的 builtin-row），视觉
   全由 theme.css 的 entity-* 提供——这里曾经有一整套 .builtin-strip /
   .builtin-name / .builtin-state（自带 padding + 下缘刻蚀线 + 自己的字号），
   那是「它不属于这个列表」的实现。并进列表后那套规则就成了**没有任何作用方的
   声明**：留着只会让下一个人以为内嵌行另有样式。 */

/* 插件页专属样式：区块标题、安装面板与清单高度链都是单页关注点，
   按 ui/AGENTS.md「单页专用样式留在 scoped」的约定放这里；theme.css 是反棘轮
   文件（只许下调），不再往全局加这些规则。选择器经 scoped 编译后自带 data-v
   属性（(0,2,0) 起步），压得过 theme.css 的全局基线 (0,1,0)。 */

/* --- 高度链：纵向滚动只发生在清单内部 ------------------------------------
   做法与内核版本页（.kernel-panel 一族）同一条：面板钉满 main 可视高度 →
   卡片吃掉剩余高度 → 内置插件 / 安装 / 清单按内容依次排，社区插件清单
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

/* 卡片直接子级：区块标题 / 内置清单 → 安装面板 → 区块标题 / 社区清单 */

/* 区块标题：内置插件 / 已安装插件。字号用 --fs-subtitle，与安装区块那个
   块头同一档；右侧辅助信息 12px muted。 */
.plugin-section-head {
  flex: none;
  display: flex;
  align-items: baseline;
  gap: 8px;
}

.plugin-section-title {
  font-size: var(--fs-subtitle);
  font-weight: 600;
  color: var(--text);
}

.plugin-section-note {
  font-size: 12px;
}

/* 清单保持内容高、超长时内部滚动；刻意不给 flex-grow，行少时不画空框。 */
.entity-list {
  min-height: 0;
  overflow-y: auto;
  overscroll-behavior: contain;
}

/* 内置插件清单：行数固定（随应用交付），不参与剩余高度分配。 */
.plugin-builtin-list {
  flex: none;
}

/* --- 安装面板（紧接内置插件下方）----------------------------------------
   用户 2026-10-31 三次调整：原区块只有「块头 + 输入 + 一句说明」，高度太低，
   且同步 / 检查更新挂在页面顶部工具条上，读起来像在管内置插件。现在收成一个
   完整面板：浅色底 + 1px 描边 + 内边距把这一块立起来；标题 / 输入 / 动作行
   在面板内，描述性文字（格式、预检、作用范围）全部收进 ⓘ tooltip。 */
.plugin-install-panel {
  flex: none;
  display: flex;
  flex-direction: column;
  padding: 16px 18px;
  border: 1px solid var(--border);
  border-radius: 10px;
  background: var(--overlay-faint);
}

.plugin-install-heading {
  display: flex;
  align-items: center;
  gap: 10px;
  min-width: 0;
  flex-wrap: wrap;
  row-gap: 8px;
}

/* 同步 / 检查更新跟在标题行右侧：它们和安装输入同属「手动安装的插件」，不该
   在面板底部另起一行（用户 2026-10-31 指的位置）。空间不够时整组折到下一行。 */
.plugin-install-head-actions {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  margin-left: auto;
  flex-wrap: wrap;
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

.plugin-install-action-btns {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}

/* 社区插件空态：安装面板已经给了完整入口，这里只留一行提示，不再画一张
   带插图的空态卡（插图会把安装区往下挤，正是用户这次指出的高度问题）。 */
.plugin-empty-hint {
  margin: 0;
  padding: 18px 16px;
  font-size: 12px;
  line-height: 1.6;
  text-align: center;
}

/* --- 当前内核插件行：单行布局（2026-10-08 用户要求）-----------------------
   版本元数据移到右侧（紧跟状态 tag 与动作按钮），entity-foot 解散后整行只有
   一条基线：名称 + 来源 chip + 描述占左侧吃掉剩余宽度（描述过长照旧省略号
   收尾、悬停看全文），右侧依次是版本、状态、警示点、动作。只覆盖挂了
   .entity-row--single 的行；内置插件行与技能页的 .skill-entity-row 各有自己
   的网格，都不受影响。 */
.entity-row--single {
  flex-direction: row;
  align-items: center;
  gap: 10px;
}

.entity-row--single .entity-head {
  flex: 1 1 auto;
}
</style>
