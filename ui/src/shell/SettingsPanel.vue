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
import { computed, onMounted, ref, watch } from 'vue';
import { Bell, Brush, Check, Clock, Headset, Lock, Monitor, QuestionFilled, Tickets, Tools } from '@element-plus/icons-vue';
import { theme, themeOptions, setTheme } from './theme.js';
import { Sun as SunIcon, Moon as MoonIcon, Monitor as MonitorIcon } from '@lucide/vue';
import { store, saveSettings } from '../store.js';
import { openStartupDiagnosis } from '../diagnostics/diagnostics.js';
import SnapshotCard from '../diagnostics/SnapshotCard.vue';
import BisectPanel from '../diagnostics/BisectPanel.vue';
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
import { globalBusy, isLoading } from './loading.js';
import {
  autostartStore,
  refreshAutostartStatus,
  setAutostartEnabled,
  setAutostartKernel,
} from './autostart.js';
import { avTrustStore, refreshAvTrustStatus, setAvExclusion } from './avTrust.js';
import { tildePath } from './labels.js';

const themeIcons = { light: SunIcon, dark: MoonIcon, system: MonitorIcon };
const port = ref(undefined);
// 固定值（默认 web）：保存时仍要原样回传，否则 Rust 侧的合并会把 profile 覆盖。
const profile = ref('');
const editing = ref(false);

// 初始与外部变化时回写输入框；用户正在编辑（editing）时跳过，避免输入被回滚。
watch(
  () => store.view && store.view.settings,
  (settings) => {
    if (!settings || editing.value) return;
    port.value = settings.port;
    profile.value = settings.profile || '';
  },
  { immediate: true }
);

function onSave() {
  saveSettings(port.value, profile.value);
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

/**
 * 「环境回退与诊断」那一行去看最近一次启动过程。
 *
 * 与概览页同一套写法：id 交给 `openStartupDiagnosis` 去问后端「最近一条」，
 * 这里不塞任何具体 id——塞进去会让用户点进来看到上一次启动的记录，而按钮上
 * 写的是「查看」。
 *
 * 不挂 `isLoading`：`reloadStartupDiagnosis(runId, false)` 那次拉取是
 * **非手动**的，不经过 `withLoading`，挂上去就是一个永远不转的 loading。
 */
function openStartupRun() {
  // 按上面那条决定，这里**不塞**任何 id（包括事故关联的 runId）：这一行是
  // 「最近一次启动过程」的通用入口，与某一桩事故无关。
  openStartupDiagnosis('', 'settings');
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
</script>

<template>
  <section class="panel">
    <div class="page-head">
      <div class="page-head__text" data-page="07">
        <h1 class="page-title">设置</h1>
        <p class="page-desc">外观、端口、后台常驻、自启动与任务通知。</p>
      </div>
    </div>
    <!-- 宽版（1040）下单列会让每一张卡右边空掉半屏，所以按设计稿分两列：
         左列是「这台壳本身怎么跑」（工作台端口、后台常驻、自启、Defender 排除项），
         右列是「对外的表现 + 出问题才动」（外观、任务通知、环境回退与诊断）。
         用的是全局的 .page-layout / .page-layout__col 原语。

         「环境回退与诊断」的位置搬过两次，账都记在这里：
           · 2026-10-08 从右栏移到左栏——当时右栏是「任务通知 + 环境回退点 /
             深入排查 / 启动诊断」好几张卡，连不上内核事件流时右栏被通知告警
             拉到近两屏高，而用户是来排查问题的，视线先撞上的却是通知告警。
           · 2026-11-02 又移回右栏末尾——「Defender 排除项」进左栏之后反过来了：
             左栏四张、右栏两张，右栏底部空出约 300px。现在左栏三张（工作台 /
             后台常驻 / Defender 排除项）、右栏三张（外观 / 任务通知 /
             环境回退与诊断），两栏总高接近。
         **入口一个没少**：恢复 / 开始排查 / 停止仍留在自己那一行的动作区。 -->
    <div class="page-layout">
      <div class="page-layout__col">
    <div class="card">
      <h2 class="card-title-with-tip">
        <span class="card-title-icon" aria-hidden="true"><el-icon><Monitor /></el-icon></span>
        工作台
        <el-tooltip placement="bottom-start" :show-after="80">
          <template #content>
            <div class="card-info-tooltip">
              profile 固定为 web，随端口保存；Node 环境与「重新检测」在概览页。
            </div>
          </template>
          <el-icon class="card-info-icon"><QuestionFilled /></el-icon>
        </el-tooltip>
      </h2>
      <el-form class="settings-form" label-width="100px" label-position="left" @focusin="editing = true" @focusout="editing = false">
        <el-form-item label="Web UI 端口">
          <div class="btn-row settings-port-row">
            <el-input-number
              v-model="port"
              class="settings-port"
              :min="1024"
              :max="65535"
              :precision="0"
              controls-position="right"
            />
            <!-- 文案是「保存配置」而不是「保存设置」（设计说明 §5）：它保存的是
                 **这一栏** —— 工作台 Web UI 端口 + profile 名。叫「保存设置」会
                 让人以为它管的是整个设置页，而下面那些开关是各自即时生效的。 -->
            <el-button
              type="primary"
              :icon="Check"
              :loading="isLoading('saveSettings')"
              :disabled="globalBusy"
              @click="onSave"
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

    <!-- Defender 排除项：内核冷启动的等待主要花在 Defender 对这些文件的**首次
         打开扫描**上（实测：首次打开一个没读过的文件 7.7ms，内核冷启动 55 秒
         ≈ 7,700 次这样的 open，而 stat 上几乎没有）。启动后由 Rust 侧自动加入
         一次（`shell::av_trust::spawn_auto`），这张卡只负责说清现状 + 撤销。
         路径一律读后端返回的真值（Rust 侧经 paths::* / kernel::data_dir 解析），
         前端不写死任何目录。 -->
    <div class="card">
      <h2 class="card-title-with-tip">
        <span class="card-title-icon" aria-hidden="true"><el-icon><Lock /></el-icon></span>
        Defender 排除项
        <el-tooltip placement="bottom-start" :show-after="80">
          <template #content>
            <div class="card-info-tooltip">
              内核冷启动要打开大量依赖文件，Defender 的首次扫描是主要开销。启动后自动把下面
              两个目录加入排除项，冷启动更快；加的时候会弹一次管理员授权，点「是」才生效。
              防护本身不关闭，也不排除用户目录或临时目录。
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
            内核尚未安装，暂无可排除的目录。
          </p>
        </div>
        <div class="btn-row av-trust-actions">
          <el-button
            size="small"
            :loading="isLoading('defenderExclusion')"
            :disabled="!avTrustStore.supported || !avTrustStore.defenderPresent || !avTrustStore.defenderRealtime || globalBusy"
            title="会弹出管理员授权窗口，请在弹窗里点「是」"
            @click="setAvExclusion(true)"
          >
            加入排除项
          </el-button>
          <el-button
            size="small"
            :loading="isLoading('defenderExclusion')"
            :disabled="!avTrustStore.supported || !avTrustStore.defenderPresent || !avTrustStore.defenderRealtime || globalBusy"
            @click="setAvExclusion(false)"
          >
            移除排除项
          </el-button>
        </div>
      </div>
      <div class="av-trust-notes">
        <!-- 代价必须常驻可见，不收进 tooltip：排除之后这些目录就不再被实时扫描，
             这是用户拿速度换来的东西，藏在悬停里等于没说过。 -->
        <p class="muted av-trust-note">
          排除后，这两个目录不再被 Defender 实时扫描；装进来的第三方包被篡改也不会当场发现。
        </p>
        <p class="muted av-trust-note">
          只对 Microsoft Defender 生效；360 等第三方杀软不受影响，需要在各自的信任区里手动添加。
        </p>
        <p v-if="avTrustStore.platformNote" class="muted av-trust-note">{{ avTrustStore.platformNote }}</p>
      </div>
    </div>

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
              任务完成后挂未读角标并发系统通知；列表带最近一问一答与完成时间。
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
        未连接内核事件流，任务完成后不会提醒。
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

    <!-- 「环境回退点 / 深入排查 / 启动诊断」三张并排的大卡收成一张
         （设计稿 draft 2844：「环境回退与诊断」，caption「出问题时使用」）。
         原先右栏四张、左栏两张，右栏被拉到两屏高而左栏空一大截——四者同属
         「环境出问题时才动」这一组，分成四张卡只是把同一个上下文摊成四段。
         三行各自是一个 `page-list-row`：左标题 + 状态说明，右动作。
         **入口一个没少**：恢复 / 开始排查 / 停止都留在自己那一行的动作区，
         「查看」展开的是明细；启动诊断直接打开诊断窗口。
         2026-11-02 起它在**右栏**末尾（Defender 排除项进左栏后两栏配平）。 -->
    <div class="card">
      <div class="card-head">
        <h2>
          <span class="card-title-icon" aria-hidden="true"><el-icon><Tools /></el-icon></span>
          环境回退与诊断
        </h2>
        <span class="head-meta"><span class="muted">出问题时使用</span></span>
      </div>
      <div class="page-list diagnostics-list">
        <!-- 前两行自带 Fragment：行是常驻的，明细与告警是它们的兄弟节点。 -->
        <SnapshotCard />
        <BisectPanel />

        <div class="page-list-row">
          <div class="page-list-main">
            <h3 class="page-list-title">启动诊断</h3>
            <p class="page-list-meta">这次启动停在哪一步、接下来做什么。</p>
          </div>
          <div class="page-list-actions">
            <el-button
              round
              size="small"
              :icon="Tickets"
              title="查看最近一次启动过程"
              @click="openStartupRun"
            >
              查看
            </el-button>
          </div>
        </div>

      </div>
    </div>

      </div>
    </div>
  </section>
</template>

<style scoped>
.appearance-options { display: flex; gap: 8px; }
.appearance-options button { flex: 1; display: grid; justify-items: center; gap: 8px; padding: 14px 0; border: 1px solid var(--border); border-radius: 8px; background: var(--surface); color: var(--text); cursor: pointer; }
.appearance-options button[aria-pressed='true'] { border-color: var(--accent); background: var(--accent-soft); }
.appearance-options button:hover { background: var(--surface-raised); }

/* Defender 卡：状态 / 目录 / 动作 / 说明四段。路径走三列栅格——标签与状态列按
   内容取宽，中间的长路径在自己的列里折行，不会把「加入 / 移除」挤到下一行。
   字号自己写：`.muted` 只管颜色（全仓 23 处在用），往里加字号会连带改掉六个面板。 */
.av-trust-body { display: flex; flex-direction: column; gap: 8px; }
.av-trust-state { margin: 0; color: var(--text-secondary); font-size: 12px; line-height: 1.5; }
.av-trust-paths { display: flex; flex-direction: column; gap: 4px; }
.av-trust-path {
  display: grid;
  grid-template-columns: 64px minmax(0, 1fr) auto;
  align-items: baseline;
  gap: 8px;
  font-size: 12px;
  line-height: 1.5;
}
.av-trust-path__label { color: var(--text-muted); }
.av-trust-path__value { color: var(--text-secondary); overflow-wrap: anywhere; }
.av-trust-path__state { color: var(--text-muted); white-space: nowrap; }
.av-trust-empty { margin: 0; font-size: 12px; }
.av-trust-actions { margin-top: 2px; }
.av-trust-notes { display: flex; flex-direction: column; gap: 4px; }
.av-trust-note { margin: 0; font-size: 11px; line-height: 1.5; }

/* Fragment 子组件的行也属于这张列表；收尾行不画分隔线。 */
.diagnostics-list > :deep(.page-list-row) {
  position: relative;
  border-bottom: 0;
}
.diagnostics-list > :deep(.page-list-row:not(:last-child))::after {
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
</style>
