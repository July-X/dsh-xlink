<script setup>
// 内核版本：左列已安装（切换 / 删除），右列 npm 发布（仅安装）。
// 「切换」必须基于本地已安装版本，避免误把尚未安装的远端版本当成可立刻启用的内核。
// 「检查更新」从 npm registry 拉取版本列表。
//
// 面板挂载时主动调一次 refreshAll()，让「已安装」列表在用户进到这一页时就是最新的，
// 而不是要等启动阶段的 get_status，或者「检查更新」之后才看到本地版本。
import { computed, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue';
import { Refresh, Download, Promotion, Delete, InfoFilled, TopRight, Loading } from '@element-plus/icons-vue';
import {
  store,
  refreshAll,
  checkUpdates,
  installVersion,
  activateVersion,
  removeVersion,
  workbenchActiveNow,
} from '../store.js';
import { invoke, listen } from '../shell/bridge.js';
import { openExternalLink } from '../shell/notify.js';
import { globalBusy, isLoading, withLoading } from '../shell/loading.js';

const KERNEL_RELEASES_URL = 'https://github.com/deepseek-ai/deepseek-harness/releases';
const kernel = computed(() => store.view && store.view.kernel);
// 后端 `KernelStatus.other_shell_workbench`（该结构体是 snake_case 序列化，
// 与 settings_warning 同一个约定）。`notice` 是后端生成的完整文案——横幅只
// 渲染它，不再自己拼：这份文案的真相源在 `instance::other_shell_workbench_notice`，
// 措辞契约由 Rust 侧测试钉住。旧版后端没有这个字段时横幅整体隐藏（空串），
// 而不是渲染一个空框。
const otherShellWorkbench = computed(
  () => (kernel.value && kernel.value.other_shell_workbench) || null
);
const otherShellWorkbenchText = computed(() => {
  const other = otherShellWorkbench.value;
  return (other && other.notice) || '';
});

function openKernelReleases() {
  return withLoading('openKernelReleases', () => openExternalLink(KERNEL_RELEASES_URL, '内核发布页'));
}

const installedVersions = computed(() => {
  const set = new Set();
  if (kernel.value) {
    kernel.value.installed.forEach((v) => set.add(v.version));
  }
  return set;
});

// 发布列表的上下淡出带**只在真正溢出时渲染**（2026-10-05 用户要求加大范围
// 与力度）。ResizeObserver 兜住窗口 / flex 引起的容器尺寸变化，watch 兜住
// 发布列表内容变化；覆盖带 absolute 定位、不参与布局，两个来源交替不会
// 振荡。
const releaseListEl = ref(null);
const releasesScrollable = ref(false);
function measureReleaseOverflow() {
  const el = releaseListEl.value;
  releasesScrollable.value = !!el && el.scrollHeight > el.clientHeight + 1;
}
let releaseResizeObserver = null;
onMounted(() => {
  measureReleaseOverflow();
  releaseResizeObserver = new ResizeObserver(measureReleaseOverflow);
  if (releaseListEl.value) releaseResizeObserver.observe(releaseListEl.value);
});
onBeforeUnmount(() => releaseResizeObserver?.disconnect());
watch(
  () => store.releases,
  () => measureReleaseOverflow(),
);

// 扫描时刻 → 「今天 14:20」/「10 月 1 日 14:20」。
//
// 只显示**日期 + 时刻**、不显示秒：这张表的精度是「天」，给到秒会让人
// 误以为数字在秒级变化。超过一天的直接标日期——「3 天前统计」比一个
// 具体日期更需要被看见。
function formatStamp(ms) {
  const at = new Date(Number(ms));
  if (Number.isNaN(at.getTime()) || !ms) return '';
  const now = Date.now();
  const days = Math.floor((now - ms) / 86400000);
  const hm = `${String(at.getHours()).padStart(2, '0')}:${String(at.getMinutes()).padStart(2, '0')}`;
  if (days >= 1) return `${days} 天前（${at.getMonth() + 1} 月 ${at.getDate()} 日 ${hm}）`;
  if (days === 0) return `今天 ${hm}`;
  return `昨天 ${hm}`;
}

// 进面板就取一次：有缓存立刻显示，缓存陈旧时后端后台重扫并回填。
// **不再需要用户点「统计」**——那 290ms 的全盘 walk 已经挪到后台线程，
// 挡住首屏没有任何理由。
onMounted(() => {
  refreshAll();
  loadDiskUsage();
});

// 磁盘占用：两段式加载（用户 2026-10-03 拍板），「刷新」按钮走强制重扫
// （用户 2026-10-07 报「刷新按钮没有正确扫描磁盘占用」）。
//
// `invoke('disk_usage')` **立刻返回**——有缓存就返回缓存（哪怕一天前的），
// 没有才同步扫一次。缓存陈旧时后端另外起线程重扫，扫完通过
// `disk-usage-refreshed` 事件回填。于是进面板时数字马上在（不用等那
// 290ms 的全盘 walk），随后自己更新到最新值。参照 `usage.js` 的
// `loadUsageSummary` + `setUsageAutoRefresh` 那一套「先给上次结果、
// 静默跟上」的形状。
//
// **自动加载与「刷新」按钮必须分开**：走的是同一个命令，但语义相反。
// 自动加载要的是「有缓存就别扫」——每次进面板都重扫 2.5 万个文件没道理。
// 按钮要的是「用户明说现在就要新数字」——此时回缓存等于没听见。缓存一天才
// 刷一次，意味着点按钮几乎永远落在「缓存新鲜」这条分支上，不传 `force`
// 的话按钮就永远是个空动作，而界面上完全看不出它没生效。
const diskUsage = reactive({
  loading: false,
  loaded: false,
  error: null,
  total: 0,
  groups: [],
  unreadable: [],
  // 扫描时刻（毫秒）。显示出来是为了让用户知道眼前这组数字有多旧——
  // 一张不标注时间的占用表，用户无从判断该信几分。
  measuredAt: 0,
  // 后台重扫中：只在已有数据时显示，不遮住已经显示出来的缓存。
  refreshing: false,
});

function applyDiskReport(report) {
  // 数值一律过一遍 Number：后端字段缺失时拿到 undefined，会在
  // formatBytes 里被 `Number.isFinite` 兜成 0，但 `total` 若直接
  // 参与模板拼接就会渲染成 "NaN" 或 "undefined"。
  diskUsage.total = Number(report.total) || 0;
  diskUsage.groups = report.groups || [];
  diskUsage.unreadable = report.unreadable || [];
  diskUsage.measuredAt = Number(report.measuredAt) || 0;
  diskUsage.loaded = true;
}

async function loadDiskUsage(force = false) {
  if (diskUsage.loading) return;
  diskUsage.loading = true;
  diskUsage.error = null;
  try {
    // 返回的是 `{ report, backgroundRefresh }` 两段，不是报表本身。拆开是
    // 因为「这次有没有后台重扫在跑」只有后端知道：缓存新鲜与重扫刚结束这两条
    // 路径在前端长得一模一样，光看返回值猜不出来。
    const reply = await invoke('disk_usage', { force });
    // 拿不到报表就当失败：宁可留着旧数字并报错，也不要把界面清成一片 0 B。
    if (!reply || !reply.report) throw new Error('磁盘用量返回为空');
    diskUsage.refreshing = Boolean(reply.backgroundRefresh);
    applyDiskReport(reply.report);
  } catch (e) {
    diskUsage.refreshing = false;
    diskUsage.error = e && e.message ? e.message : String(e);
  } finally {
    diskUsage.loading = false;
  }
}

// 后台重扫完成：新数字自己替换旧数字，界面不跳一下、不闪一下。
// Tauri 的事件把数据放在 `e.payload` 上（与 `harness-fault` 同一约定），
// 所以先取 payload 再判空——直接把 `e` 当报表用会读到 undefined，
// 于是「静默失败」看起来像「后台没扫」。
//
// 监听放在**组件**里而不是 App.vue：这段逻辑只有本面板关心，而 App.vue 的
// `registerAppListener` 会攒到应用销毁才统一退订；面板可以被反复挂载
// （切页签），组件级订阅随卸载释放才不漏。
let diskUnlisten = null;
listen('disk-usage-refreshed', (e) => {
  diskUsage.refreshing = false;
  const report = e && e.payload;
  if (!diskUsage.loaded || !report) return;
  applyDiskReport(report);
})
  .then((unlisten) => {
    if (typeof unlisten === 'function') diskUnlisten = unlisten;
  })
  // 订阅失败不该让整张卡不可用：数字已经在界面上了，只是不会自动刷新。
  .catch(() => {});

onBeforeUnmount(() => {
  if (diskUnlisten) diskUnlisten();
});

// 字节 → 人类可读。**不在 Rust 侧格式化**：单位与小数位是显示决策，而这张
// 表还要按字节排序、按百分比画条——后端给原始字节，前端怎么排都不会让
// 「排序用字符串比较」这种错重新长出来。
//
// 用 1024 进制并按 1000 归一（507MB 显示成 495 MB）——文件系统的真实语义，
// 与 Finder / `du -h` 一致，用户拿另一个工具对得上数。
const UNITS = ['B', 'KB', 'MB', 'GB', 'TB'];
function formatBytes(bytes) {
  const value = Number(bytes);
  if (!Number.isFinite(value) || value <= 0) return '0 B';
  let index = 0;
  let scaled = value;
  while (scaled >= 1024 && index < UNITS.length - 1) {
    scaled /= 1024;
    index += 1;
  }
  // 字节本身不显示小数（12 B 不会写成 "12.0 B"），其余保留一位。
  const digits = index === 0 ? 0 : 1;
  return `${scaled.toFixed(digits)} ${UNITS[index]}`;
}

// --- 四个占用类型的配色与说明 ---
//
// 两件事按 group.id 走，不按 label：**label 是文案，会改**（「实例数据」改成
// 「会话数据」不该让配色和说明一起失配），id 是契约，后端 build_group 的第一个
// 实参，一一对应且不会随手改。
//
// 配色取自 usage.js 的 MODEL_COLORS 同源色板（同一个图表家族里用同一套色，
// 两张图并排时「蓝=哪一类」不会各说各话），但**不复用那个导出**：那是模型色，
// 语义是「哪个 LLM」；这里是占用类型，两边各自增删条目时互不牵动。
// 未知 id 回落 --accent：新加一类忘了配色时仍然看得见，而不是渲染成空白。
const USAGE_TYPES = {
  kernels: {
    color: '#4f8cff',
    tip: '各个已安装的内核版本及其依赖。删掉不用的版本即可回收，不影响在用的那个。',
  },
  instances: {
    color: '#22d3ee',
    tip: '实例的数据目录，装的是你的会话与附件。这是你的数据，删了就没了，壳不提供删除。',
  },
  stores: {
    color: '#34d399',
    tip: '插件、技能与备份的中央库。删掉会在下次需要时重新下载。',
  },
  logs: {
    color: '#fbbf24',
    tip: '壳自身的运行日志，出问题排查时要看。可以随时清空。',
  },
};

function usageColor(group) {
  const found = USAGE_TYPES[group && group.id];
  return found ? found.color : 'var(--accent)';
}

// 说明文案。`detail`（后端给的缩写全名，如「实例数据（会话与附件）」）只在上面
// 那句没提到它时补一行——「内核版本」这种 detail 为空的分类不会多出一行废话。
function groupTip(group) {
  const found = USAGE_TYPES[group && group.id];
  const base = found ? found.tip : 'dsh-xlink 占用的一部分。';
  const extra = group && group.detail && !base.includes(group.detail) ? group.detail : '';
  return extra ? base + '（' + extra + '）' : base;
}
</script>

<template>
  <section class="panel kernel-panel">
    <div class="card kernel-card">
      <div class="card-head">
        <h2>内核版本</h2>
        <span class="head-meta">
          <span class="muted">已安装 {{ kernel ? kernel.installed.length : '—' }} 个</span>
        </span>
      </div>

      <el-alert v-if="store.settingsWarning" :title="store.settingsWarning" type="warning" :closable="false" show-icon />

      <!-- 另一个壳的工作台在跑：**不拦**装 / 删内核（双壳并行是壳存在的理由），
           但后果必须在点之前说清楚。文案真相源在 Rust（other_shell_workbench_notice，
           措辞契约有测试钉住），这里只渲染 `notice`；旧版后端没有该字段时整条
           隐藏（空串），不渲染空框。 -->
      <el-alert
        v-if="otherShellWorkbenchText"
        :title="otherShellWorkbenchText"
        type="warning"
        :closable="false"
        show-icon
      />

      <el-alert v-if="store.releaseWarning" :title="store.releaseWarning" type="warning" :closable="false" show-icon />

      <div class="updates-lists">
        <div class="list-group">
          <h3>已安装</h3>
          <div class="installed-list">
            <el-empty v-if="!kernel || kernel.installed.length === 0" description="尚未安装任何内核。" :image-size="64" />
            <div v-for="v in kernel ? kernel.installed : []" :key="v.version" class="installed-row">
              <span class="release-ver">{{ v.version }}</span>
              <!-- 旧版硬链接安装的树（文件仍与其他目录共享存储）：删除 / 重装它会
                   短暂惊动对面正在用的工作台（会自愈）。卸载后重装一次即隔离，
                   标记随之消失。旧后端没有该字段时标记隐藏。 -->
              <el-tooltip
                v-if="v.shared_storage"
                content="旧版方式安装：文件仍与其他目录共享存储，删除或重装会短暂惊动对面正在用的工作台（会自动恢复）。卸载后重装一次即彻底隔离。"
                placement="top"
              >
                <el-tag
                  size="small"
                  type="warning"
                  effect="plain"
                  style="margin-left: 8px"
                >
                  共享存储
                </el-tag>
              </el-tooltip>
              <span class="release-actions">
                <el-tag v-if="v.active" type="success" size="small" effect="dark">当前使用</el-tag>
                <template v-else>
                  <el-button
                    size="small"
                    :icon="Promotion"
                    :loading="isLoading('activate:' + v.version)"
                    :disabled="globalBusy || workbenchActiveNow()"
                    title="工作台启动或运行期间不能切换内核"
                    @click="activateVersion(v.version)"
                  >
                    切换
                  </el-button>
                <el-popconfirm
                  title="确认删除该版本？"
                  confirm-button-text="删除"
                  cancel-button-text="取消"
                  width="200"
                  @confirm="removeVersion(v.version)"
                >
                  <template #reference>
                    <!-- 删除同样会大面积改动内核安装目录（一个 450 MB、几万个文件
                         的 remove_dir_all），所以与「切换」共用同一条禁令：工作台
                         启动或运行期间不许动。切换那条守卫是加过的，删除这条原先
                         漏了——面板上也不该让按钮看起来比实际允许的更宽松。 -->
                    <el-button
                      size="small"
                      type="danger"
                      plain
                      :icon="Delete"
                      :loading="isLoading('remove:' + v.version)"
                      :disabled="globalBusy || workbenchActiveNow()"
                      title="工作台启动或运行期间不能删除内核版本"
                    >
                      删除
                    </el-button>
                  </template>
                </el-popconfirm>
                </template>
              </span>
            </div>
          </div>
        </div>

        <div class="list-group">
          <h3 class="list-head-with-logo">
            <img class="brand-logo" src="/npm-logo.svg" alt="npm" />
            <span>npm 发布</span>
            <span class="release-list-actions">
              <!-- `checkUpdates()` 带括号：`checkUpdates(manual = true)` 被裸引用时
                   收到的是 MouseEvent，恰好与默认值同义所以今天看不出坏——但它是靠
                   巧合对的，改默认值就会静默变坏。 -->
              <el-button class="release-check-button" text :icon="Refresh" :loading="isLoading('checkUpdates')" :disabled="globalBusy" @click="checkUpdates()">
                检查更新
              </el-button>
              <el-button
                text
                :icon="TopRight"
                :loading="isLoading('openKernelReleases')"
                :disabled="globalBusy"
                @click="openKernelReleases"
              >
                打开发布页
              </el-button>
            </span>
          </h3>
          <!-- 边框与底色画在**外层** `.release-list-box`，滚动区在里层。原因是
               mask 只该作用在「内容」上：边框若与滚动容器是同一个元素，它会被
               mask 一起淡掉，那恰好推翻「把滚动区域的边框显示出来」这条要求。
               底色用 --bg-soft（实色 #121831），比卡片自身的 rgba(255,255,255,.05)
               深一档，与上方「已安装」那片裸行区分开——那片是直接铺在卡片上的
               行，这片是一个可滚动的子区域，长得不一样才读得出「这块能滚」。 -->
          <div class="release-list-box">
            <div
              ref="releaseListEl"
              class="release-list"
              :class="{ 'release-list--bleed': releasesScrollable }"
            >
              <p v-if="store.releases.length === 0" class="muted" style="margin: 0">
                点击「检查更新」获取官方发布列表。
              </p>
              <div v-for="r in store.releases" :key="r.version" class="release-row">
                <span class="release-ver">{{ r.version }}</span>
                <span class="release-actions">
                  <el-tag v-if="installedVersions.has(r.version)" size="small" effect="plain">已安装</el-tag>
                  <el-button
                    v-if="!installedVersions.has(r.version)"
                    size="small"
                    type="primary"
                    :icon="Download"
                    :disabled="globalBusy || workbenchActiveNow()"
                    title="工作台启动或运行期间不能安装内核版本"
                    @click="installVersion(r.version)"
                  >
                    安装
                  </el-button>
                  <el-tag v-if="r.prerelease" type="info" size="small" effect="plain">预发布</el-tag>
                </span>
              </div>
            </div>
            <!-- 边缘淡出带（2026-10-05 用户两轮收敛后的最终形态）：滚动区域内
                 的内容**完全正常显示**，半透明只发生在上下两条**静止的覆盖带**
                 上——行滚到底下时被渐变底色逐渐盖住，而不是内容自身被 mask
                 淡化。只在真正溢出时渲染（短列表不挂带子）；pointer-events
                 必须关掉，否则带子会挡住底下行的点击与滚轮。 -->
            <div v-if="releasesScrollable" class="release-fade release-fade--top" aria-hidden="true"></div>
            <div v-if="releasesScrollable" class="release-fade release-fade--bottom" aria-hidden="true"></div>
          </div>
        </div>
      </div>

      <!-- 磁盘用量：只读视图，**没有任何删除入口**（2026-10-02 拍板）。
           理由是这里没有安全边界可守——最大的两块是内核 node_modules 与
           实例 DSH home（后者装的是用户会话与附件），壳无法替用户判断
           哪块该删。给只读数字，用户自己用 Finder 处理，壳就不必在
           「删错了」和「不敢删」之间二选一。 -->
      <div class="disk-usage">
        <div class="card-head">
          <h2>
            磁盘用量
            <el-tooltip placement="bottom-start" :show-after="80">
              <template #content>
                <div class="card-info-tooltip">
                  dsh-xlink 在本机占用的磁盘分布，只读。<br />
                  按目录树展开统计，不跟随软链（内核树里 pnpm 的软链指向
                  store，跟随会把同一份字节数数两遍），因此这是上界。
                  硬链接复用也无法在纯目录遍历层面识别。<br />
                  <!-- 「多久扫一次、这组数字什么时候的」都收在这里：它们是
                       解释「该信几分」的注脚，不是每屏都要读的主信息。
                       占一行位置只为说「一天刷一次」，不值。扫描时刻尤其
                       不能丢——它挪进来后仍要能被随时问到。 -->
                  <span v-if="diskUsage.loaded && diskUsage.measuredAt">
                    {{ formatStamp(diskUsage.measuredAt) }}统计，每 24 小时后台自动刷新一次。
                  </span>
                </div>
              </template>
              <el-icon class="card-info-icon"><InfoFilled /></el-icon>
            </el-tooltip>
          </h2>
          <span class="head-meta">
            <span v-if="diskUsage.loaded" class="usage-total">
              合计 {{ formatBytes(diskUsage.total) }}
            </span>
            <el-icon v-if="diskUsage.refreshing" class="usage-refreshing" :title="'后台正在重新扫描…'">
              <Loading />
            </el-icon>
            <el-button
              v-if="diskUsage.loaded"
              size="small"
              text
              :icon="Refresh"
              :loading="diskUsage.loading"
              :title="'立即重新统计（平时每天自动扫一次）'"
              @click="loadDiskUsage(true)"
            >
              刷新
            </el-button>
          </span>
        </div>

        <div v-if="diskUsage.loaded" class="usage-groups">
          <div
            v-for="group in diskUsage.groups"
            :key="group.id"
            class="usage-tile"
            :style="{ '--usage-color': usageColor(group) }"
          >
            <div class="usage-tile-head">
              <!-- 标题挂 tooltip 而不是原生 title：四类占用的**可回收性完全不同**
                   （内核能删、实例数据是用户会话、插件技能可重下、日志随时可清），
                   而瓦片标题只有 11px、宽 186px，装不下这句话。原生 title 要悬停
                   1s 才出、样式也跟界面不一致，这里改用 EP tooltip。
                   内容优先级：**这句「这是什么、能不能删」> 缩写全名**——前者是
                   用户点进来真正要答的问题。detail（后端给的缩写全名）只在与
                   标题不同且上面没覆盖到时才补一行。 -->
              <el-tooltip placement="bottom-start" :show-after="80">
                <template #content>
                  <div class="card-info-tooltip usage-type-tip">
                    {{ groupTip(group) }}
                  </div>
                </template>
                <span class="usage-tile-label">{{ group.label }}</span>
              </el-tooltip>
              <span class="usage-tile-total">{{ formatBytes(group.bytes) }}</span>
            </div>
            <div class="usage-tile-share">
              <span class="usage-bar" aria-hidden="true">
                <span
                  class="usage-bar-fill"
                  :style="{ width: group.sharePercent + '%' }"
                ></span>
              </span>
              <span class="usage-tile-percent">{{ group.sharePercent }}%</span>
            </div>
            <ul v-if="group.entries.length" class="usage-entries">
              <li v-for="entry in group.entries" :key="entry.id" class="usage-entry">
                <!-- 悬停先给人话全名，再给路径：缩写（正式版）在同一张瓦片里
                     可能重名，真正能区分它们的是内核族与目录。 -->
                <span
                  class="usage-entry-name"
                  :title="entry.detail ? entry.detail + '\n' + entry.path : entry.path"
                >{{ entry.label }}</span>
                <span class="usage-entry-bytes">{{ formatBytes(entry.bytes) }}</span>
              </li>
            </ul>
            <p v-else class="muted usage-empty">这一类当前没有内容。</p>
          </div>

          <p v-if="diskUsage.unreadable.length" class="muted usage-unreadable">
            以下目录读不到，未计入合计：{{ diskUsage.unreadable.join('、') }}
          </p>
        </div>

        <p v-else-if="diskUsage.loading" class="muted usage-idle">正在统计本机占用…</p>
        <div v-else class="usage-idle">
          <p class="muted" style="margin: 0">
            统计失败：{{ diskUsage.error || '未知原因' }}
          </p>
          <el-button size="small" text :icon="Refresh" @click="loadDiskUsage()">
            重试
          </el-button>
        </div>
      </div>
    </div>
  </section>
</template>

<style scoped>
/* npm 发布列表可能几十上百条（每个 rc / alpha 都是一条）。不加约束时整张
   卡片会长到把页面撑出**外层**滚动条——而面板外层本来就有自己的滚动，
   于是变成两层滚动条，鼠标滚轮归属变得难猜。限高 + 内部滚动后，
   外层滚动条的位置与行为不变，只把长列表收进自己的容器里。

   91 → 120px（2026-10-03，真机反馈）：行高实测 ≈ 36px，91px 只装得下 2 整行
   + 53%，第三行被硬切掉一半。120px 装 3 整行 + 33%。

   **120px 是被另一条要求钉死的上限**：同一轮用户要求「不能出现页面级纵向
   滚动条」，并用红箭头指着窗口右边缘那条。真机实测（498×815 窗口、可视高
   787px）里，120px 限高时内容底在 y≈797、**溢出约 10px**——所以「加高」与
   「不滚动」在 120px 处差一点，需要由下面的 `.disk-usage` 间距压缩补上。 */

/* 外层：边框 + 底色。它是**不参与滚动、也不参与 mask** 的一层——用户要看到
   完整的框，而 mask 只该让「内容」在边缘淡出。圆角 10px 与 `.installed-row`、
   `.usage-tile` 同一档。 */
.release-list-box {
  border: 1px solid var(--border);
  border-radius: 10px;
  background: var(--bg-soft);
  /* 边缘淡出带的定位基准：带子 absolute 盖在滚动区的上下边界上（见下方
     `.release-fade`）。 */
  position: relative;
  /* 外溢/淡出的统一深度（2026-10-05：20 → 15 → 12px，20/15 都会压到上下
     文本）。外溢视口 padding/margin 与带子高度/偏移全部引用这一个变量，
     调深度只改这一行。 */
  --release-bleed: 12px;
}

.release-list {
  overflow-y: auto;
  /* 触屏 / 触控板甩到列表尽头时不要连带触发页面级手势——否则在列表底部
     再滚一下，页面会跟着跳，用户以为列表没到底。 */
  overscroll-behavior: contain;
  /* 滚动条默认贴在容器右缘，而外层卡片有内边距，短列表时看不出来，
     长列表时那条「悬空的轨道」很显眼。留 4px 把它拉近卡片边框。 */
  padding-right: 4px;
}

/* 边缘淡出带（2026-10-05 第四轮收敛的最终形态）：带子在盒子的**外侧**，
   方向朝滚动区域外扩——行滚出盒子边框后并不消失，而是进入外溢区，被
   弧形带逐渐盖住直至隐去。滚动区域内自始至终完全正常。

   外溢区的来历：`.release-list--bleed` 用「等量负 margin + padding」把
   滚动窗口上下各外推（布局尺寸不变——负 margin 恰好抵消 padding），行
   因此能滚出边框仍可见；外侧带子接手遮盖。带子只在真正溢出时渲染
   （模板侧 v-if），pointer-events: none 防止挡住底下内容的点击。

   弧形（用户手绘示意）：覆盖力沿水平方向向两侧衰减（mask 90deg 渐变），
   中间外扩最深、两端收敛——而不是上下两条等宽直线。

   **带子本身必须半透明、渐变单调向外加重**（2026-10-05 用户两轮纠正）：
   背景是网格纹理 + 半透明玻璃卡片，不透明实色盖上去就是一块纹理消失的
   异质矩形——所以峰值只到 0.9，纹理隐约透出，与玻璃 UI 匹配；方向为
   盒边处全透明（内容正常）、**越往外遮得越重，外缘最重**（曾在外缘回落
   到 0.15，视觉上成了「往外越来越淡」，方向反了，用户指出）。遮盖色的
   色相取带子落点处的背景合成色：上带落在卡片内（白 5% 叠 --bg ≈
   #171c2b），下带落在面板底（--bg = #0b1020）；主题是固定深色（无浅色
   变体），字面量与注释配对，主题改动时这里要跟着改。 */
.release-fade {
  position: absolute;
  left: 1px;
  right: 1px;
  height: var(--release-bleed, 12px);
  pointer-events: none;
  z-index: 1;
  -webkit-mask: linear-gradient(90deg, transparent 0, #000 14%, #000 86%, transparent 100%);
  mask: linear-gradient(90deg, transparent 0, #000 14%, #000 86%, transparent 100%);
}

.release-fade--top {
  top: calc(-1 * var(--release-bleed, 12px));
  background: linear-gradient(to top, transparent 0, rgba(23, 28, 43, 0.9) 100%);
}

.release-fade--bottom {
  bottom: calc(-1 * var(--release-bleed, 12px));
  background: linear-gradient(to bottom, transparent 0, rgba(11, 16, 32, 0.9) 100%);
}

/* 外溢视口：只在真正溢出时展开（不溢出时盒子尺寸与改前一致）。 */
.release-list--bleed {
  padding-top: var(--release-bleed, 15px);
  padding-bottom: var(--release-bleed, 15px);
  margin-top: calc(-1 * var(--release-bleed, 15px));
  margin-bottom: calc(-1 * var(--release-bleed, 15px));
}

/* 「npm 发布」标题行抬到外溢视口与上侧淡出带（z:1）之上：行滚出上边框的
   残影会进入标题行的地界，标题与「检查更新 / 打开发布页」按钮必须可见
   可点（z 抬高 = 命中测试也归它，按钮不会被外溢区挡住）。 */
.list-head-with-logo {
  position: relative;
  z-index: 2;
}

/* 已安装的版本通常只有一两条（用户很少囤），不设限高——加了反而让
   唯一一行上方多出一段空白。只约束 npm 那一列。 */

/* 磁盘占用报表的样式**放在这里而不是 theme.css**：theme.css 是反棘轮
   大文件（预算只许下调），而这份样式只被本组件用。拆出去的同时也让
   「哪段样式属于哪张卡片」变得可查。变量沿用 theme.css 的全局色板
   （--text / --muted / --bg-soft / --accent / --bad），scoped 不会改
   变它们的取值。 */

.disk-usage {
  /* 18+16 → 10+12 → **6+9**：这一段是「版本列表」与「磁盘占用」两个功能区
     之间的分隔，上面已经有 1px 边框在划界。10+12 又把整页顶出一屏——真机
     实测 120px 限高时溢出约 10px，箭头指着窗口右边缘那条滚动条。9 是在
     边框与标题之间留的呼吸，6 是它与上一段的距离；再往下压标题就贴线了。 */
  margin-top: 6px;
  padding-top: 9px;
  border-top: 1px solid var(--border);
  /* 抬到发布列表的外溢视口与淡出带（z:1）之上：列表行滚出下边框后的残影
     从这块的边缘底下穿过，而「磁盘占用」标题与「刷新」按钮必须可见可点。 */
  position: relative;
  z-index: 2;
}

/* `.card-head` 是 theme.css 的全局类（6px 下边距 + 1px 边框），这里在
   scoped 里覆盖成 2px：本组件有两处（内核版本 / 磁盘占用），各省 4px。
   2px 而不是 0，是为了给「npm 发布列表新加的那圈边框」腾出它需要的 2px——
   那圈边框让整页又高 2px，这一处不收就又要冒出页面滚动条。
   scoped 选择器多带一个属性选择器，权重高于全局那条，只有本组件受影响。 */
.card-head {
  padding-bottom: 2px;
}

.disk-usage h2 {
  display: flex;
  align-items: center;
  gap: 4px;
  margin: 0;
  font-size: 14px;
}

.usage-total {
  font-size: 12px;
  font-weight: 600;
  color: var(--accent);
}

.usage-idle,
.usage-empty,
.usage-unreadable {
  font-size: 11.5px;
  margin: 0;
  line-height: 1.6;
}

.usage-unreadable {
  margin-top: 2px;
  /* 路径可能很长，允许断行而不是撑破卡片。 */
  word-break: break-all;
  /* 它不是「某一类」，是横跨全表的一句提醒。网格里不给它跨列，它会被
     塞进第一格、与「内核版本」并排，读起来像是内核版本读不到。 */
  grid-column: 1 / -1;
}

/* 四个分类**左右各一个**，宽屏并排两列。参照「套餐用量」窗口里
   `.usage-overview-stats` 的做法：瓦片卡片（浅底 + 细边框 + 圆角 10）
   比裸文字列表更容易扫读——每格的「标题 / 主数字」自成一块。

   **下限从 280 收到 186 不是随手调的**：本机面板在 480 CSS px 宽的窗口里
   只有约 424px 可用（截图 960px @2x 减去面板与卡片内边距），而
   `280 × 2 + 8 = 568 > 424` ——auto-fit 因此判定「放不下两列」，直接回落
   成一列，于是用户看到的还是竖排（这正是 2026-10-03 反馈的现象：代码里
   已经是两列，界面上却仍是一列）。186 是能让 `186 × 2 + 8 = 380 ≤ 424`
   在最小窗口成立的下限：再小瓦片里的「标题 + 数字」就会挤在一行而省略，
   那还不如一列。

   用 auto-fit 而不是断点：面板宽度由用户拖窗口决定，断点猜不准；
   auto-fit 让它自己数能塞下几列，窗口再窄就回落一列。 */
.usage-groups {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(186px, 1fr));
  /* 8 → 6：两行瓦片之间的横缝。瓦片自己有边框和底色，6px 足够分得开，
     而 2px 刚好是这一轮从页面里挤出来的高度的一部分。 */
  gap: 6px;
  /* `stretch`（默认）而不是 `start`：四格条目数不同（内核 2 条、实例
     2 条、日志 2 条、商店 3 条），`start` 会让每格按自己的内容收高，
     同一行里两格的底边就错开——那不叫「左右对称」。拉齐之后右侧多出
     的空白留在格内，视觉上反而是齐的。 */
  align-items: stretch;
}

/* 四格内容长短不一时，让条目区从底部往上排，短的那一格也不会把瓦片
   撑得比邻居高——底边对齐比「顶部对齐 + 各自高度」更接近对称。 */
.usage-tile {
  background: var(--bg-soft);
  border: 1px solid var(--border);
  border-radius: 10px;
  /* 6/9/6：比上一版再收 1px。瓦片只有标题、条形、条目三块，padding
     每多 1px，两列并排的收益就被吃掉一点。
     6 → 3（纵向）：这一轮把整页压回一屏，两行瓦片各省 6px。横向的 9px
     不动——那 9px 正是「内核版本 878.2 MB / 实例数据 350.1 MB」两格
     并排时的可读下限（见 .usage-tile-head 的取舍说明）。 */
  padding: 3px 9px;
  min-width: 0;
  /* 条目多的那格把邻居撑到同样高，底边因此对齐（见 .usage-groups 的
     align-items 注释）。 */
  display: flex;
  flex-direction: column;
}

/* 标题与主数字**同一行**：标题在左吃掉剩余空间，数字靠右且永不收缩。
   中间试过上下两行（186px 并排放不下长标题），但那让每张瓦片多占一行
   高度——而两列并排省纵向正是这里的目的。同行的前提是**数字不许被挤**：
   `flex-shrink: 0` + 标题 `ellipsis`，长标题（`实例数据（会话与附件）`）
   截断成「实例数据（会话…」而 `350.1 MB` 完整可读。取舍明确：宁可少看
   几个字（点瓦片有 title），不可看不清主数字。 */
.usage-tile-head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 6px;
  min-width: 0;
}

.usage-tile-label {
  font-weight: 600;
  color: var(--text);
  /* 11.5 → 11：与缩小后的主数字（12.5）拉开一级差，标题才不会与数字
     抢同一档的视觉重量。 */
  font-size: 11px;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* 标题前的小圆点：四个占用类型各一色，与占比条、容量胶囊共用同一个
   `--usage-color`。**不是装饰**——四张瓦片的标题都是「内核版本 / 实例数据 /
   插件… / 壳日志」这类中性词，光看文字分不出「哪块能删、哪块是你的数据」，
   而这正是用户悬停之前就该先看到的区分。 */
.usage-tile-label::before {
  content: '';
  display: inline-block;
  width: 6px;
  height: 6px;
  border-radius: 50%;
  margin-right: 5px;
  /* `vertical-align: 1px` 让圆点与 11px 文字的中线对齐：默认 baseline 会让圆点
     坐在基线上、视觉上偏下。 */
  vertical-align: 1px;
  background: var(--usage-color, var(--accent));
}

.usage-tile-total {
  /* 12.5 → 11.5：比标题还低半档，字面上就让「量出来的数」退到标题后面。
     高度不许因此长出来——正文 line-height 是 1.5，12.5px 的裸文本行盒 18.75px；
     这里 11.5 × 1.45 + 上下各 1px 描边 ≈ 18.7px，正好持平，瓦片头不增重。 */
  font-size: 11.5px;
  line-height: 1.45;
  font-weight: 600;
  /* 风格区分靠两件事，都不是「再调一次字号」：
     ① 颜色——标题是 --text（说这是什么），容量是 --accent（这是一个量出来的值）；
     ② 形状——标题是裸文本，容量是胶囊。两个 11px 上下的文本并排，不换底色
        就还是「标题 + 标题」，而这一行恰恰不该被读成一句标题。
     胶囊的写法沿用本仓已有的 chip 词汇（.status-pill / .brand-update）。 */
  color: var(--usage-color, var(--accent));
  background: rgba(255, 255, 255, 0.05);
  border: 1px solid var(--border);
  border-radius: 999px;
  padding: 0 6px;
  white-space: nowrap;
  /* 不许被标题挤掉——见 .usage-tile-head 上面的取舍说明。 */
  flex-shrink: 0;
  font-variant-numeric: tabular-nums;
}

/* 分类自己的占比条 + 百分比：回答「这块占全部的多少」。条目里不再重复
   画条——四张瓦片各画一次条、每张下面还有 N 个条目也画条，图会碎成一片
   蓝，而人只会先看分类级的那个。 */
.usage-tile-share {
  display: flex;
  align-items: center;
  gap: 6px;
  /* 5 → 3 → 2：标题与占比条紧挨着，它们说的是同一件事（这块多大）。 */
  margin: 2px 0 0;
}

.usage-tile-percent {
  font-size: 11px;
  color: var(--muted);
  white-space: nowrap;
  font-variant-numeric: tabular-nums;
}

/* 分类说明气泡。`.card-info-tooltip` 是 theme.css 的全局类（卡片标题 ℹ️ 用的
   那一套），这里只收一处宽度：那句话最长 40 多字，默认宽度会折成四五行，
   而 tooltip 挂在瓦片标题上、用户是「扫一眼确认能不能删」，不该读一段小字。 */
.usage-type-tip {
  max-width: 260px;
  line-height: 1.6;
}

.usage-bar {
  flex: 1;
  min-width: 0;
  height: 4px;
  border-radius: 2px;
  /* 白色低透明度而不是 `--bg-soft`：瓦片底色**就是** `--bg-soft`，用
     同一个值画轨道等于什么都没画，占据的百分比条看不出「还剩多少」。 */
  background: rgba(255, 255, 255, 0.08);
  overflow: hidden;
}

.usage-bar-fill {
  display: block;
  height: 100%;
  border-radius: 2px;
  /* 分类自己的颜色（`--usage-color` 由模板按 group.id 注入瓦片，四个占用类型
     各一色）。未知 id 由 usageColor() 回落成 --accent，CSS 里的第二层兜底是
     给「模板没注入」这种情况的——比如某个测试直接渲染瓦片。 */
  background: var(--usage-color, var(--accent));
  /* 极小的分类也要看得见：宽度按 sharePercent，但设下限，否则 0.02% 的
     备份会渲染成一条看不见的线。 */
  min-width: 2px;
}

.usage-entries {
  list-style: none;
  /* 6 → 4：占比条与条目之间已经有一道 border-top 在分界，4px 够读出
     「下面换了一层」，不必给到 6。 */
  margin: 4px 0 0;
  padding: 0;
  /* 顶部一道细线，把「分类自己的数」与「下面这些条目」分开。 */
  border-top: 1px solid var(--border);
  padding-top: 4px;
}

.usage-entry {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 10px;
  /* 2 → 1：瓦片里每条只有名称与字节数两段，2px 的行距在两列并排时
     显得比内容本身还松。 */
  padding: 1px 0;
  font-size: 11.5px;
}

.usage-entry-name {
  color: var(--muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.usage-entry-bytes {
  color: var(--text);
  white-space: nowrap;
  flex-shrink: 0;
  font-variant-numeric: tabular-nums;
}

.usage-refreshing {
  animation: usage-spin 1.1s linear infinite;
  color: var(--muted);
}

@keyframes usage-spin {
  to {
    transform: rotate(360deg);
  }
}

/* 「npm 发布」吃掉页面剩余高度（2026-10-05 用户要求）：面板钉满 main 的
   可视高度，卡片列弹性伸展，纵向滚动只发生在发布列表内部——日常状态下
   外层 main 不再出现纵向滚动条。选择器都挂 .kernel-card / .list-group 前缀
   压过 theme.css 窄窗媒体查询里的 `max-height: none; overflow-y: visible`
   （那里是「滚动只交给外层 main」的旧约定，本页改为「滚动只交给内层
   列表」，两页规则不同就得分开写，不能动全局）。min-height: 0 一路铺到
   列表，缺任何一环 flex 子项都会拒绝收缩、把页面重新撑出外层滚动条。
   兜底：窗口矮到上方静态内容（警示条 + 已安装列表 + 磁盘占用）本身都
   放不下时，列表收缩到内容高、main 的滚动条照常出现——可达性优先于
   「无外层滚动条」，那条规则只在正常窗口尺寸下成立。 */
.kernel-panel {
  height: 100%;
  min-height: 0;
}

.kernel-card {
  flex: 1 1 auto;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.kernel-card .updates-lists {
  flex: 1 1 auto;
  min-height: 0;
}

.kernel-card .list-group {
  min-height: 0;
}

.kernel-card .release-list-box {
  flex: 1 1 auto;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.kernel-card .list-group .release-list {
  flex: 1 1 auto;
  min-height: 0;
  max-height: none;
  overflow-y: auto;
}

/* 窄布局（应用固定 480px，主题的 760px 断点内）两列变一列：已安装组收成
   自然高（auto），npm 组拿走剩余高度（下限 120px，即旧的 max-height 档）。
   宽布局不设行模板——两列同排、行为与改前一致，滚动仍归外层 main。 */
@media (max-width: 760px) {
  .kernel-card .updates-lists {
    grid-template-rows: auto minmax(120px, 1fr);
  }
}
</style>
