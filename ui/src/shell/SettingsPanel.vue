<script setup>
// 设置：Web UI 端口、任务完成通知，以及一张「环境回退与诊断」。
// 套餐用量查询不设卡：「用量分析」页的套餐卡已提供数据展示 / 刷新 / 测试入口，
// 凭据本身在工作台模型设置里维护。内置补丁（内核补丁 / 小插件）入口已隐藏——最新
// 补丁的「应用 / 撤销」机制已于 2026-10-11 整体删除：唯一内置的 dsh-file-perf
// 自 0.1.2-alpha.2 起已被官方内核采纳（适用窗口只剩 0.1.1-rc.1/rc.2 两个版本），
// 而这个页面的应用入口更早就没了。`plugins/patches.rs` 只留 `applied_patch_ids`
// 一条只读缝——打过分的老用户磁盘上的 state.json 还在，环境指纹要把它算进去。
//
// 卡片顺序：设置 → 任务通知 → 环境回退与诊断。
// 后一张是**故障时才用得上的兜底**，不是日常要看的东西：概览页是开机第一屏，
// 在那儿常年摆着两个「一切正常」的空态卡既占地方又稀释真正要看的内容（当前内核、
// 用量、额度）。环境回退点、深入排查与启动诊断集中在这张卡里。
import PageHeadIcon from './PageHeadIcon.vue';
import { mcode, refreshMcodeStatus, saveMcodePorts } from '../kernel/mcode.js';
import { MCODE_BUILD_AVAILABLE, mcodeExperience, setMcodeExperience } from '../kernel/experience.js';
import { invoke } from './bridge.js';
import { confirmDialog } from './notify.js';
import { computed, onMounted, ref, watch } from 'vue';
import { Bell, Brush, Check, Clock, Headset, Lock, Monitor, QuestionFilled } from '@element-plus/icons-vue';
import { theme, themeOptions, setTheme } from './theme.js';
import { Sun as SunIcon, Moon as MoonIcon, Monitor as MonitorIcon } from '@lucide/vue';
import { store, saveSettings } from '../store.js';
import EnvironmentDiagnosticsCard from './EnvironmentDiagnosticsCard.vue';
import {
  notificationStore,
  refreshNotificationStatus,
  saveNotificationSettings,
  markNotificationsRead,
  sendTestNotification,
  testNotificationSound,
  formatNotifyTime,
  formatNotifyDuration,
} from '../incidents/notifications.js';
import { globalBusy, isLoading, withLoading } from './loading.js';
import {
  autostartStore,
  refreshAutostartStatus,
  setAutostartEnabled,
  setAutostartKernel,
} from './autostart.js';
import { avTrustStore, refreshAvTrustStatus, setAvExclusion } from './avTrust.js';
import { tildePath } from './labels.js';
import { adoptDraft } from './portDrafts.js';

const themeIcons = { light: SunIcon, dark: MoonIcon, system: MonitorIcon };
// 这一页是公共页：不读内核状态（内核维度已从页面级降到控件级）。体验入口开启时
// 两个内核的端口都在同一张卡上；关掉只隐藏 MiniMax 那一组，DSH 照旧。
watch(() => [store.activePanel, mcodeExperience.value], ([panel, experience]) => {
  if (panel === 'settings' && experience) refreshMcodeStatus();
}, { immediate: true });
// Defender 排除项卡只在 Windows 出现（后端 av_trust_status 的 supported）。这里用
// main.js 同步打在 body 上的平台类决定「环境回退与诊断」落哪栏——不用异步的
// avTrustStore.supported，免得首帧先画在一栏、状态回来后整张卡跳到另一栏。
const isWindowsPlatform = document.body.classList.contains('platform-windows');
const port = ref(undefined);
// 固定值（默认 web）：保存时仍要原样回传，否则 Rust 侧的合并会把 profile 覆盖。
const profile = ref('');
// MiniMax 的两个回环端口属于当前实例；与 DSH 的 port 分开存，避免两套语义串台。
const mcodePort = ref(undefined);
const mcodeMiniappPort = ref(undefined);
// 后端回填的那份值：只有输入框里的值真的变了才再存一次，否则用户改的是 DSH 的
// 端口、却因为顺带把 MiniMax 那份也写了一遍而多弹一句「MiniMax 端口已保存」。
const savedMcodePorts = ref('');
// 逐字段草稿标记：焦点不是「放弃修改」的信号，未保存的输入只由保存/快照解除。
const portDirty = ref(false);
const mcodePortDirty = ref(false);
const mcodeMiniappDirty = ref(false);

// 初始与外部变化时回写输入框；有草稿的字段跳过，避免轮询把未保存的输入覆盖掉。
watch(
  () => store.view && store.view.settings,
  (settings) => {
    if (!settings) return;
    profile.value = settings.profile || '';
    adoptDraft(port, portDirty, settings.port);
  },
  { immediate: true }
);

// MiniMax 端口跟着实例状态走；保存后重新拉状态同样靠这条回填。体验入口藏起来时
// 表单整组 v-if 掉，但输入框里的草稿还在——这段期间不拿轮询值覆盖它。
watch(
  [() => mcode.view, mcodeExperience],
  ([view, experience]) => {
    if (!view) return;
    savedMcodePorts.value = `${view.port}/${view.miniapp_port}`;
    if (!experience) return;
    adoptDraft(mcodePort, mcodePortDirty, view.port);
    adoptDraft(mcodeMiniappPort, mcodeMiniappDirty, view.miniapp_port);
  },
  { immediate: true }
);

// 一张卡上摆着两个内核的端口，一个保存按钮把两套各存各的：DSH 存端口 +
// profile，MiniMax 存它自己实例的两个端口，互不覆盖。两条命令各发各的提示，
// 不合并：一半成功一半失败时，成功的那个不会被说成没保存。
async function onSavePorts() {
  // 失败保留草稿（用户可重试或放弃）；成功时后端已是这个值，草稿作废。
  const saved = await saveSettings(port.value, profile.value);
  if (saved) portDirty.value = false;
  // MiniMax 那两个端口还没取到值时不硬存：没装 MiniMax 的用户点一次保存就弹一句
  // 「端口需为 1024–65535 的整数」，那是天天见的一句噪音。改过才存：没改就别多弹
  // 一句「MiniMax 端口已保存」。
  const next = `${mcodePort.value}/${mcodeMiniappPort.value}`;
  if (
    mcodeExperience.value &&
    next !== savedMcodePorts.value &&
    Number.isInteger(mcodePort.value) &&
    Number.isInteger(mcodeMiniappPort.value)
  ) {
    const done = await saveMcodePorts(mcodePort.value, mcodeMiniappPort.value);
    if (done) {
      mcodePortDirty.value = false;
      mcodeMiniappDirty.value = false;
    }
  }
}

// 开关的语义只写在 ⓘ 里，面板上常驻的只有名称、状态词与开关。
const EXPERIENCE_NOTE = '只控制 MiniMax 相关入口的显示，不自动安装、启动或停止内核，也不修改授权。';

// 关闭入口本身没有副作用，但「什么都不会发生」只在工作台没跑、也没有共享授权时
// 成立：任一项读出来为真就说明仍可能生效；读不出来时按同一方向提示「无法确认」，
// 不能显示成已停止或没有授权。确认才保存 false，取消不动状态。
const EXPERIENCE_CLOSE_TITLE = '关闭 MiniMax 体验入口？';
const EXPERIENCE_CLOSE_BASE = '关闭后只隐藏相关入口，不会停止工作台，也不会删除数据或撤销已有授权。';
const EXPERIENCE_CLOSE_ACTIVE = 'MiniMax 工作台仍在运行，已授权的共享记忆仍可能被使用。取消后可到「概览」停止工作台或到「记忆」管理授权；隐藏后可重新开启体验入口。';
const EXPERIENCE_CLOSE_UNKNOWN = '无法确认当前状态，仍可能生效；取消后可到「概览」停止工作台或到「记忆」管理授权，隐藏后可重新开启体验入口。';

async function experienceCloseDetail() {
  let active = false;
  let unknown = false;
  await refreshMcodeStatus();
  if (mcode.error) unknown = true;
  else if (mcode.view?.running) active = true;
  try {
    const module = await invoke('memory_module_status');
    // `warning` 非空 = 授权落盘但当前实例 / profile 已不匹配：enabled 是 false，
    // 旧授权却仍在盘上，关闭入口同样要按「仍可能生效」提示。
    if (module?.enabled || module?.warning) active = true;
  } catch {
    unknown = true;
  }
  try {
    const shares = await invoke('list_all_memory_shares');
    if (Array.isArray(shares) && shares.length > 0) active = true;
  } catch {
    unknown = true;
  }
  if (active) return EXPERIENCE_CLOSE_BASE + EXPERIENCE_CLOSE_ACTIVE;
  if (unknown) return EXPERIENCE_CLOSE_BASE + EXPERIENCE_CLOSE_UNKNOWN;
  return null;
}

// 体验开关即时保存，不经过「保存配置」。setMcodeExperience 只在成功后回显，失败
// 已由它自己提示，开关读的是 store 真值，会自己弹回原值。
function onToggleExperience(enabled) {
  return withLoading('mcodeExperience', async () => {
    if (!enabled) {
      const detail = await experienceCloseDetail();
      if (detail && !(await confirmDialog(EXPERIENCE_CLOSE_TITLE, detail, '仅隐藏入口'))) return;
    }
    await setMcodeExperience(enabled);
  });
}

// 进入设置页时刷新通知与自启状态（事件流连接可能已经变化，登录项也可能被
// 用户在系统设置里改过）。Defender 排除项同理：启动时那次自动加入是异步的，
// 用户可能在它跑完之后才切到设置页。
watch(
  () => store.activePanel,
  (panel) => {
    if (panel === 'settings') {
      refreshNotificationStatus();
      refreshAutostartStatus();
      refreshAvTrustStatus();
    }
  },
  { immediate: true }
);

// 角标画在哪：Rust 回报的平台字符串决定文案。
const badgeTarget = computed(() => {
  if (notificationStore.platform === 'macos') return 'Dock 图标';
  if (notificationStore.platform === 'windows') return '任务栏图标';
  return '应用图标';
});

// dev 专用自检入口（`store.devUi`）：release 预览打开后这个字段也是 false。
const isDevBuild = computed(() => store.devUi);
const testNote = ref('');

async function onTestNotification() {
  testNote.value = '';
  const ok = await sendTestNotification();
  if (ok) {
    testNote.value =
      `已模拟一次任务完成：${badgeTarget.value}上的角标已更新，系统通知已发出。` +
      '没看到气泡时，请在系统通知设置里允许 dsh-xlink。';
  }
}

// 当前状态的一句话。四种结果要说清，因为用户下一步的动作不同：已加入 → 什么
// 都不用做；被拒 / 已移除 → 不会再自动加，要加得自己点；失败 → 隔一天再自动试。
// 2026-11-02 精简：时间戳与结果压到一行，逗号替掉括号。
const avSummary = computed(() => {
  if (!avTrustStore.supported) return avTrustStore.platformNote || '本平台不需要。';
  if (!avTrustStore.defenderPresent) return '本机没有 Microsoft Defender，排除项没有意义。';
  if (!avTrustStore.defenderRealtime) return 'Defender 未启用（可能已被第三方杀软接管），排除项不会起作用；请改用该杀软的信任区。';
  const last = avTrustStore.last;
  if (!last) return '还没加入：启动后会自动试一次（会弹管理员授权）。';
  switch (last.outcome) {
    case 'applied':
      return `已加入 · ${last.at}`;
    case 'removed':
      return `已移除 · ${last.at}，之后不会再自动加入。`;
    case 'declined':
      return `上次未获得管理员授权 · ${last.at}，不会再自动尝试；要加请点下面的按钮。`;
    default:
      return `上次加入失败 · ${last.at}：${last.detail}`;
  }
});

// 每个目录的排除状态。**只能按后端记下的账说**：Defender 的排除项清单要管理员
// 才读得到，普通用户读不动，所以这里不说「我查了系统」，只说「本程序加过没有」。
function avTargetState(target) {
  if (!target.exists) return '目录还不存在';
  const paths = (avTrustStore.last && avTrustStore.last.paths) || [];
  return paths.includes(target.path) ? '已加入' : '未加入';
}

// 两颗动作按钮的可用性。都亮着会让人以为可以反复点，实际重复点只是把同一个值
// 再写一遍（还要再弹一次 UAC）。判据只认后端记的账（avTargetState），不在前端
// 另存一份「加过没有」。
const avCanAct = computed(
  () =>
    avTrustStore.supported &&
    avTrustStore.defenderPresent &&
    avTrustStore.defenderRealtime &&
    !globalBusy.value,
);
const avTargetMissing = computed(
  () => !avTrustStore.targets.length || !avTrustStore.targets[0].exists,
);
const avAddedCount = computed(
  () => avTrustStore.targets.filter((target) => avTargetState(target) === '已加入').length,
);
const avAllExcluded = computed(
  () => avTrustStore.targets.length > 0 && avAddedCount.value === avTrustStore.targets.length,
);
const avAddDisabled = computed(() => !avCanAct.value || avTargetMissing.value || avAllExcluded.value);
const avRemoveDisabled = computed(() => !avCanAct.value || avAddedCount.value === 0);
const avAddHint = computed(() => {
  if (avTargetMissing.value) return '数据主目录还未创建，暂时没有可加入排除项的路径';
  if (avAllExcluded.value) return '数据主目录已经在 Defender 排除项里，不需要重复加入';
  if (!avCanAct.value) return avSummary.value;
  return '会弹出管理员授权窗口，请在弹窗里点「是」';
});
const avRemoveHint = computed(() => {
  if (avAddedCount.value === 0) return '本程序当前没有加入过排除项，无需移除';
  if (!avCanAct.value) return avSummary.value;
  return '移除后会恢复 Defender 对整个数据主目录的实时扫描';
});
</script>

<template>
  <section class="panel">
    <div class="page-head">
      <div class="page-head__text"><PageHeadIcon :page="'settings'" />
        <h1 class="page-title">设置</h1>
        <p class="page-desc">外观、端口、后台常驻、自启动与任务通知。</p>
      </div>
    </div>
    <!-- 宽版（1040）下单列会让每一张卡右边空掉半屏，所以按设计稿分两列：
         左列是「这台壳本身怎么跑」（工作台端口、后台常驻、自启、Defender 排除项），
         右列是「对外的表现 + 出问题才动」（外观、任务通知、环境回退与诊断）。
         用的是全局的 .page-layout / .page-layout__col 原语。

         「环境回退与诊断」的位置按平台分——Defender 排除项卡只在 Windows 出现：
           · Windows：左栏已有三张，它留在右栏末尾（2026-11-02 的配平：左三右三）。
           · 非 Windows：左栏少一张、底部空出一大截（2026-11-05 用户报），它改落
             左栏末尾补位（2026-10-08 曾按「排查入口别被通知告警压走」放在同一位置）。
         它因此抽成 `EnvironmentDiagnosticsCard.vue`，两栏各有一个互斥的条件挂载点；
         位置经过记在 docs/ui/panel-decisions.md。
         **入口一个没少**：恢复 / 开始排查 / 停止仍留在自己那一行的动作区。 -->
    <div class="page-layout">
      <div class="page-layout__col">
    <!-- 工作台卡两个内核共用：同一张卡、同一个 el-form 与保存按钮。
         端口字段两套都在——DSH 是 Web UI 端口，MiniMax 是它自己实例的工作台端口
         + App 端口（后两行只在体验入口开启时出现）。这一页是公共页，不读内核状态，
         所以两套端口不跟着选中项分叉。整卡 v-if 换掉会逼出两套重复表单。 -->
    <div class="card">
      <h2 class="card-title-with-tip">
        <span class="card-title-icon" aria-hidden="true"><el-icon><Monitor /></el-icon></span>
        工作台
        <el-tooltip placement="bottom-start" :show-after="80">
          <template #content>
            <div class="card-info-tooltip">
              {{ mcodeExperience
                ? 'profile 固定为 web，随端口保存；Node 环境与「重新检测」在概览页。MiniMax 的两个端口只作用于当前 MiniMax 实例，保存后重启工作台生效。'
                : 'profile 固定为 web，随端口保存；Node 环境与「重新检测」在概览页。' }}
            </div>
          </template>
          <el-icon class="card-info-icon"><QuestionFilled /></el-icon>
        </el-tooltip>
      </h2>
      <el-form
        class="settings-form settings-ports-form"
        label-width="150px"
        label-position="left"
      >
        <!-- 体验开关即时保存，和下面这颗「保存配置」不是同一件事：它只管入口
             露不露出来。构建不包含能力时禁用并说明，已存的偏好原样保留。 -->
        <div class="settings-experience">
          <span class="settings-experience__name">
            <span class="minimax-wordmark">MiniMax</span> 内核体验
            <span class="origin-chip settings-experience-chip">实验</span>
            <el-tooltip placement="bottom-start" :show-after="80">
              <template #content>
                <div class="card-info-tooltip">{{ EXPERIENCE_NOTE }}</div>
              </template>
              <el-icon class="card-info-icon" tabindex="0" :aria-label="EXPERIENCE_NOTE"><QuestionFilled /></el-icon>
            </el-tooltip>
          </span>
          <span class="settings-experience__action">
            <span v-if="!MCODE_BUILD_AVAILABLE" class="muted settings-experience-state">当前版本不可用</span>
            <el-switch
              :model-value="mcodeExperience"
              :loading="isLoading('mcodeExperience')"
              :disabled="!MCODE_BUILD_AVAILABLE || globalBusy"
              aria-label="MiniMax 内核体验"
              @change="onToggleExperience"
            />
          </span>
        </div>
        <fieldset class="settings-port-group">
          <legend><span class="deepseek-wordmark">DeepSeek</span> <span class="settings-port-kind">稳定内核</span></legend>
          <el-form-item label="Web UI 端口">
            <div class="btn-row settings-port-row">
              <el-input-number
                v-model="port"
                v-on:update:model-value="portDirty = true"
                aria-label="DeepSeek Web UI 端口"
                class="settings-port"
                :min="1024"
                :max="65535"
                :precision="0"
                controls-position="right"
              />
            </div>
          </el-form-item>
        </fieldset>
        <fieldset v-if="mcodeExperience" class="settings-port-group settings-port-group--experimental">
          <legend>MiniMax <span class="settings-port-kind">实验内核</span></legend>
          <el-form-item label="Web UI 端口">
            <div class="btn-row settings-port-row">
              <el-input-number
                v-model="mcodePort"
                v-on:update:model-value="mcodePortDirty = true"
                aria-label="MiniMax Web UI 端口"
                class="settings-port"
                :min="1024"
                :max="65535"
                :precision="0"
                controls-position="right"
              />
            </div>
          </el-form-item>
          <el-form-item label="App 端口">
            <div class="btn-row settings-port-row">
              <el-input-number
                v-model="mcodeMiniappPort"
                v-on:update:model-value="mcodeMiniappDirty = true"
                aria-label="MiniMax App 端口"
                class="settings-port"
                :min="1024"
                :max="65535"
                :precision="0"
                controls-position="right"
              />
            </div>
          </el-form-item>
        </fieldset>
        <!-- 文案是「保存配置」而不是「保存设置」（设计说明 §5）：它保存的是
             **这张卡上摆着的端口**（DSH 另带 profile 名）。叫「保存设置」会让人
             以为它管的是整个设置页，而下面那些开关是各自即时生效的。
             按钮单独占一行：它存的是上面两套端口，挂在其中一行的格子里会让人
             以为它只存那一个。 -->
        <el-form-item label-width="0" class="settings-port-actions">
          <div class="btn-row">
            <el-button
              type="primary"
              :icon="Check"
              :loading="isLoading('saveSettings') || isLoading('saveMcodePorts')"
              :disabled="globalBusy"
              @click="onSavePorts"
            >
              保存配置
            </el-button>
          </div>
        </el-form-item>
      </el-form>
    </div>

    <div class="card">
      <h2 class="card-title-with-tip">
        <span class="card-title-icon" aria-hidden="true"><el-icon><Clock /></el-icon></span>
        后台常驻
        <el-tooltip placement="bottom-start" :show-after="80">
          <template #content>
            <div class="card-info-tooltip">
              关窗只是收进后台，内核与工作台继续运行；重新打开与退出在菜单栏 / 托盘图标的
              右键菜单里，退出前若内核在跑会先确认。
              <br />
              「开机自启动」只把程序拉进后台，不显示面板，也不启动内核；打开「开机启动工作台」
              才开机即起，占端口并订阅事件。
            </div>
          </template>
          <el-icon class="card-info-icon"><QuestionFilled /></el-icon>
        </el-tooltip>
      </h2>
      <el-form class="notify-form" label-width="152px" label-position="left">
        <el-form-item label="关窗后留在后台">
          <el-switch :model-value="true" disabled />
        </el-form-item>
        <el-form-item label="开机自启动">
          <el-switch
            :model-value="autostartStore.enabled"
            :loading="isLoading('autostartSet')"
            @change="(value) => setAutostartEnabled(value)"
          />
        </el-form-item>
        <el-form-item label="开机启动工作台">
          <el-switch
            :model-value="autostartStore.kernel"
            :loading="isLoading('autostartSetKernel')"
            @change="(value) => setAutostartKernel(value)"
          />
        </el-form-item>
      </el-form>
      <el-alert
        v-if="autostartStore.note"
        :title="autostartStore.note"
        type="warning"
        :closable="false"
        show-icon
      />
    </div>

    <!-- Defender 排除项：只在 Windows 显示。启动后由 Rust 侧自动把整个
         `~/.dsh-xlink` 数据主目录加入一次（`shell::av_trust::spawn_auto`），
         这张卡只负责说清现状 + 撤销。路径读后端返回的真值，前端不写死目录。 -->
    <div v-if="avTrustStore.supported" class="card">
      <h2 class="card-title-with-tip">
        <span class="card-title-icon" aria-hidden="true"><el-icon><Lock /></el-icon></span>
        Defender 排除项
        <el-tooltip placement="bottom-start" :show-after="80">
          <template #content>
            <div class="card-info-tooltip">
              内核冷启动要打开大量依赖文件，Defender 的首次扫描是主要开销。启动后自动把
              数据主目录加入排除项，冷启动更快；加的时候会弹一次管理员授权，点「是」才生效。
              整个主目录（含会话、凭据、设置）都不再被实时扫描。
            </div>
          </template>
          <el-icon class="card-info-icon"><QuestionFilled /></el-icon>
        </el-tooltip>
      </h2>
      <div class="av-trust-body">
        <p class="av-trust-state">{{ avSummary }}</p>
        <div class="av-trust-paths">
          <div class="av-trust-path" v-for="target in avTrustStore.targets" :key="target.path">
            <span class="av-trust-path__label">{{ target.label }}</span>
            <span class="av-trust-path__value" :title="target.path">{{ tildePath(target.path) }}</span>
            <span class="av-trust-path__state">{{ avTargetState(target) }}</span>
          </div>
          <p v-if="!avTrustStore.targets.length" class="muted av-trust-empty">
            数据主目录还未创建。
          </p>
        </div>
        <div class="btn-row av-trust-actions">
          <el-button
            size="small"
            :loading="isLoading('defenderExclusion')"
            :disabled="avAddDisabled"
            :title="avAddHint"
            @click="setAvExclusion(true)"
          >
            加入排除项
          </el-button>
          <el-button
            size="small"
            :loading="isLoading('defenderExclusion')"
            :disabled="avRemoveDisabled"
            :title="avRemoveHint"
            @click="setAvExclusion(false)"
          >
            移除排除项
          </el-button>
        </div>
      </div>
      <div class="av-trust-notes">
        <!-- 代价必须常驻可见，不收进 tooltip：整个主目录（含会话与凭据）不再被实时扫描，
             这是用户拿速度换来的东西，藏在悬停里等于没说过。 -->
        <p class="muted av-trust-note">
          排除后，整个数据主目录（含会话、凭据、设置）不再被 Defender 实时扫描；其中的文件被篡改也不会当场发现。
        </p>
        <p class="muted av-trust-note">
          只对 Microsoft Defender 生效；360 等第三方杀软不受影响，需要在各自的信任区里手动添加。
        </p>
      </div>
    </div>

      <!-- 非 Windows：左栏补位（见上方布局注释）。 -->
      <EnvironmentDiagnosticsCard v-if="!isWindowsPlatform" />

      </div>

      <div class="page-layout__col">
    <div class="card">
      <h2>
        <span class="card-title-icon" aria-hidden="true"><el-icon><Brush /></el-icon></span>
        外观
      </h2>
      <div class="appearance-options" role="group" aria-label="外观模式">
        <button v-for="option in themeOptions" :key="option.value" type="button" :aria-pressed="theme === option.value" @click="setTheme(option.value)">
          <component :is="themeIcons[option.value]" :size="22" />
          <span>{{ option.label }}</span>
        </button>
      </div>
    </div>
    <div class="card">
      <h2 class="card-title-with-tip">
        <span class="card-title-icon" aria-hidden="true"><el-icon><Bell /></el-icon></span>
        任务通知
        <el-tooltip placement="bottom-start" :show-after="80">
          <template #content>
            <div class="card-info-tooltip">
              DSH 与 MiniMax 工作台的任务完成都会通知；列表标出来源，并带最近一问一答与完成时间。
            </div>
          </template>
          <el-icon class="card-info-icon"><QuestionFilled /></el-icon>
        </el-tooltip>
      </h2>
      <el-form class="notify-form" label-width="152px" label-position="left">
        <el-form-item label="任务完成后通知我">
          <el-switch
            :model-value="notificationStore.enabled"
            :loading="isLoading('notificationSave')"
            @change="(value) => saveNotificationSettings({ enabled: value })"
          />
        </el-form-item>
        <el-form-item label="工作台不在前台才通知">
          <div class="notify-inline">
            <el-switch
              :model-value="notificationStore.notifyAwayOnly"
              :disabled="!notificationStore.enabled"
              :loading="isLoading('notificationSave')"
              @change="(value) => saveNotificationSettings({ notifyAwayOnly: value })"
            />
            <span class="muted">切走或窗口被遮挡时才提醒</span>
          </div>
        </el-form-item>
        <el-form-item label="通知声音">
          <div class="notify-inline">
            <el-switch
              :model-value="notificationStore.sound"
              :disabled="!notificationStore.enabled"
              :loading="isLoading('notificationSave')"
              @change="(value) => saveNotificationSettings({ sound: value })"
            />
            <!-- 声音关着时不试听——否则开关与听到的结果自相矛盾。 -->
            <el-button
              text
              size="small"
              :icon="Headset"
              :loading="isLoading('notificationSoundTest')"
              :disabled="!notificationStore.enabled || !notificationStore.sound || globalBusy"
              @click="testNotificationSound()"
            >
              试听
            </el-button>
          </div>
        </el-form-item>
      </el-form>
      <div class="btn-row">
        <template v-if="notificationStore.unread > 0">
          <span class="notify-unread">{{ notificationStore.unread }} 条未读</span>
          <el-button text size="small" :loading="isLoading('notificationMarkRead')"
            :disabled="globalBusy" @click="markNotificationsRead()">
            全部已读
          </el-button>
        </template>
        <template v-if="isDevBuild">
          <el-button type="primary" size="small" :icon="Bell" :loading="isLoading('notificationTest')"
            :disabled="globalBusy" @click="onTestNotification">
            模拟一次任务完成
          </el-button>
          <span class="muted notify-test-hint">dev 专用。</span>
        </template>
      </div>
      <p v-if="testNote" class="muted notify-hint">{{ testNote }}</p>
      <p v-if="!notificationStore.watching" class="muted notify-hint">
        未连接 DSH 事件流，DSH 任务完成后不会提醒。
      </p>
      <p
        v-if="mcodeExperience && mcode.view?.running && !notificationStore.watchingMcode"
        class="muted notify-hint"
      >
        未连接 MiniMax 事件流，MiniMax 任务完成后不会提醒<template v-if="notificationStore.mcodeError">（{{ notificationStore.mcodeError }}）</template>。
      </p>
      <p v-if="notificationStore.notificationsBlocked && notificationStore.environmentNote" class="muted notify-hint">
        {{ notificationStore.environmentNote }}
      </p>
      <el-alert
        v-if="notificationStore.lastError"
        :title="notificationStore.lastError"
        type="warning"
        :closable="false"
        show-icon
      />

      <!-- 最近完成记录：每条带完成时刻与最近一轮对话（提问 + 回复预览）。
           系统通知气泡里是同一段文案；这里让用户回来也能翻到。 -->
      <ul v-if="notificationStore.items.length" class="notify-items">
        <li
          v-for="item in notificationStore.items"
          :key="item.sessionId + '-' + item.finishedAtMs"
          class="notify-item"
        >
          <div class="notify-item-head">
            <span v-if="item.kernel === 'mcode'" class="origin-chip origin-chip-kernel"><span class="minimax-wordmark">MiniMax</span></span>
            <span v-else class="origin-chip origin-chip-kernel">DSH</span>
            <span class="notify-item-title" :title="item.cwd">{{ item.title }}</span>
            <span class="notify-item-time">
              {{ formatNotifyTime(item.finishedAtMs) || '时间未知'
                }}<template v-if="formatNotifyDuration(item.durationMs)">
                · 用时 {{ formatNotifyDuration(item.durationMs) }}</template>
            </span>
          </div>
          <p v-if="item.lastPrompt" class="notify-item-line" :title="item.lastPrompt">
            <span class="notify-item-role">问</span>{{ item.lastPrompt }}
          </p>
          <p v-if="item.lastResponse" class="notify-item-line" :title="item.lastResponse">
            <span class="notify-item-role">答</span>{{ item.lastResponse }}
          </p>
        </li>
      </ul>
      <p v-else class="muted notify-hint">
        还没有完成记录；完成后会显示会话名、最近问答与完成时间。
      </p>
    </div>

      <!-- Windows：Defender 卡占左栏第三张，这张卡留右栏末尾（见上方布局注释）。 -->
      <EnvironmentDiagnosticsCard v-if="isWindowsPlatform" />

      </div>
    </div>
  </section>
</template>

<style scoped>
.appearance-options { display: flex; gap: 8px; }
/* 账页语言（2026-11-02 两套主题统一）：方角、透明底；选中态用 accent 下划线
   而不是填色块。 */
.appearance-options button { flex: 1; display: grid; justify-items: center; gap: 8px; padding: 14px 0; border: 1px solid var(--border); border-radius: var(--radius-control); background: transparent; color: var(--text); cursor: pointer; }
.appearance-options button[aria-pressed='true'] { border-color: var(--accent); color: var(--accent); box-shadow: inset 0 -2px 0 var(--accent); }
.appearance-options button:hover { background: var(--overlay-faint); }

/* 任务通知列表：来源 chip 是标题前的固定宽项，标题吃掉剩余宽度再省略——
   行头原来是「标题 | 时间」两栏的 space-between，多一个 chip 不加这条标题会
   被挤到中间。 */
.notify-item-head .origin-chip { flex: none; }
.notify-item-head .notify-item-title { flex: 1 1 auto; }

/* 体验开关：名称 / 实验 chip / ⓘ 在左，状态词与开关在右。与端口分组共用
   表单的 12px 行距，不另起一张卡。 */
.settings-experience { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
.settings-experience__name { display: inline-flex; align-items: center; gap: 8px; font-size: var(--fs-5); }
.settings-experience__action { display: inline-flex; align-items: center; gap: 8px; }
.settings-experience-state { font-size: var(--fs-4); }
/* 「实验」chip 复用来源 chip 的形状，颜色换成实验档（与端口分组的粉色同一语义）。 */
.settings-experience-chip {
  padding: 1px 6px;
  font-size: var(--fs-3);
  line-height: 1.5;
  color: var(--experimental);
  background: var(--experimental-fill);
  border-color: var(--experimental);
}

/* 原生分组为读屏提供内核归属；粉色只标记实验端口，不圈入共用保存按钮。 */
.settings-ports-form { display: grid; gap: 12px; container-type: inline-size; }
.settings-port-actions :deep(.el-form-item__content) { justify-content: flex-end; }
.settings-port-group {
  min-inline-size: 0;
  margin: 0;
  padding: 10px 12px 12px;
  border: 1px solid var(--border);
  border-radius: var(--radius-control);
}
.settings-port-group legend {
  padding: 0 6px;
  color: var(--text-secondary);
  font-size: var(--fs-5);
  font-weight: 600;
}
.settings-port-kind { margin-left: 6px; font-size: var(--fs-3); font-weight: 500; }
.settings-port-group--experimental { border-color: var(--experimental); }
.settings-port-group--experimental legend { color: var(--experimental); }
.settings-port-group--experimental .settings-port-kind {
  padding: 2px 6px;
  background: var(--experimental-fill);
  border-radius: var(--radius-control);
}

/* 窄栏改为上下排列，避免长标签挤压五位端口号和步进按钮。 */
@container (max-width: 310px) {
  .settings-port-group :deep(.el-form-item) { flex-direction: column; align-items: flex-start; }
  .settings-port-group :deep(.el-form-item__content) { width: 100%; }
}

/* MiniMax 两个端口各占一行：卡片里 form-item 的默认下距已清零，补回行距。 */
.settings-form .el-form-item + .el-form-item { margin-top: 8px; }

/* Defender 卡：状态 / 目录 / 动作 / 说明四段。路径走三列栅格——标签与状态列按
   内容取宽，中间的长路径在自己的列里折行，不会把「加入 / 移除」挤到下一行。
   字号自己写：`.muted` 只管颜色（全仓 23 处在用），往里加字号会连带改掉六个面板。 */
.av-trust-body { display: flex; flex-direction: column; gap: 8px; }
.av-trust-state { margin: 0; color: var(--text-secondary); font-size: var(--fs-5); line-height: 1.5; }
.av-trust-paths { display: flex; flex-direction: column; gap: 4px; }
.av-trust-path {
  display: grid;
  grid-template-columns: 64px minmax(0, 1fr) auto;
  align-items: baseline;
  gap: 8px;
  font-size: var(--fs-5);
  line-height: 1.5;
}
.av-trust-path__label { color: var(--text-muted); }
.av-trust-path__value { color: var(--text-secondary); overflow-wrap: anywhere; }
.av-trust-path__state { color: var(--text-muted); white-space: nowrap; }
.av-trust-empty { margin: 0; font-size: var(--fs-5); }
.av-trust-actions { margin-top: 2px; }
.av-trust-notes { display: flex; flex-direction: column; gap: 4px; }
.av-trust-note { margin: 0; font-size: var(--fs-5); line-height: 1.5; }
</style>
