<script setup>
// 内核版本：左列已安装（切换 / 删除），右列 npm 发布（仅安装）。
// 「切换」必须基于本地已安装版本，避免误把尚未安装的远端版本当成可立刻启用的内核。
// 「检查更新」从 npm registry 拉取版本列表。
//
// 面板挂载时主动调一次 refreshAll()，让「已安装」列表在用户进到这一页时就是最新的，
// 而不是要等启动阶段的 get_status，或者「检查更新」之后才看到本地版本。
import { computed, onBeforeUnmount, onMounted, reactive } from 'vue';
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
import VersionPluginsTip from './VersionPluginsTip.vue';

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

// 每个已安装内核的插件快照只在 Tooltip 即将显示时读取，避免页面初次
// 渲染就为所有内核发起 IPC。已成功读取的版本会复用缓存。
const versionPlugins = reactive({});

function versionPluginSlot(version) {
  if (!versionPlugins[version]) {
    versionPlugins[version] = {
      loading: false,
      loaded: false,
      error: null,
      rows: [],
    };
  }
  return versionPlugins[version];
}

async function loadVersionPlugins(version) {
  const slot = versionPluginSlot(version);
  if (slot.loaded || slot.loading) return;

  slot.loading = true;
  slot.error = null;
  try {
    slot.rows = (await invoke('kernel_plugin_list', { version })) || [];
    slot.loaded = true;
  } catch (e) {
    // 失败**不**置 `loaded`：旧实现把它一起置真，于是这次失败被永久缓存，
    // 之后每次悬浮都直接命中"已加载"分支，tooltip 永远不会重试（P2-37）。
    slot.error = e && e.message ? e.message : String(e);
    slot.loaded = false;
  } finally {
    slot.loading = false;
  }
}

const emptyPluginSnapshot = Object.freeze({
  loading: false,
  loaded: false,
  error: null,
  rows: [],
});

function pluginSnapshot(version) {
  return versionPlugins[version] || emptyPluginSnapshot;
}

const installedVersions = computed(() => {
  const set = new Set();
  if (kernel.value) {
    kernel.value.installed.forEach((v) => set.add(v.version));
  }
  return set;
});

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

// 磁盘占用：两段式加载（用户 2026-10-03 拍板）。
//
// `invoke('disk_usage')` **立刻返回**——有缓存就返回缓存（哪怕一天前的），
// 没有才同步扫一次。缓存陈旧时后端另外起线程重扫，扫完通过
// `disk-usage-refreshed` 事件回填。于是进面板时数字马上在（不用等那
// 290ms 的全盘 walk），随后自己更新到最新值。参照 `usage.js` 的
// `loadUsageSummary` + `setUsageAutoRefresh` 那一套「先给上次结果、
// 静默跟上」的形状。
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

async function loadDiskUsage() {
  if (diskUsage.loading) return;
  diskUsage.loading = true;
  diskUsage.error = null;
  try {
    applyDiskReport(await invoke('disk_usage'));
  } catch (e) {
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
</script>

<template>
  <section class="panel">
    <div class="card">
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
                <el-tooltip
                  effect="dark"
                  popper-class="kernel-plugin-tooltip"
                  placement="right-start"
                  :fallback-placements="['left-start', 'bottom-start', 'top-start']"
                  :boundaries-padding="12"
                  trigger="hover"
                  :show-after="160"
                  :hide-after="120"
                  :offset="8"
                  :show-arrow="true"
                  @before-show="loadVersionPlugins(v.version)"
                >
                  <button
                    type="button"
                    class="installed-tip-trigger"
                    :aria-label="'查看 ' + v.version + ' 的插件'"
                  >
                    <el-icon class="installed-tip-icon"><InfoFilled /></el-icon>
                  </button>
                  <template #content>
                    <VersionPluginsTip :snapshot="pluginSnapshot(v.version)" :version="v.version" />
                  </template>
                </el-tooltip>
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
              <el-button class="release-check-button" text :icon="Refresh" :loading="isLoading('checkUpdates')" :disabled="globalBusy" @click="checkUpdates">
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
          <div class="release-list">
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
        </div>
      </div>

      <!-- 磁盘占用：只读视图，**没有任何删除入口**（2026-10-02 拍板）。
           理由是这里没有安全边界可守——最大的两块是内核 node_modules 与
           实例 DSH home（后者装的是用户会话与附件），壳无法替用户判断
           哪块该删。给只读数字，用户自己用 Finder 处理，壳就不必在
           「删错了」和「不敢删」之间二选一。 -->
      <div class="disk-usage">
        <div class="card-head">
          <h2>
            磁盘占用
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
              @click="loadDiskUsage"
            >
              刷新
            </el-button>
          </span>
        </div>

        <div v-if="diskUsage.loaded" class="usage-groups">
          <div v-for="group in diskUsage.groups" :key="group.id" class="usage-tile">
            <div class="usage-tile-head">
              <span class="usage-tile-label" :title="group.label">{{ group.label }}</span>
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
                <span class="usage-entry-name" :title="entry.path">{{ entry.label }}</span>
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
          <el-button size="small" text :icon="Refresh" @click="loadDiskUsage">
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
   于是变成两层滚动条，鼠标滚轮归属变得难猜。限高到 3 条 + 内部滚动后，
   外层滚动条的位置与行为不变，只把长列表收进自己的容器里。

   91px ≈ 恰好 3 条。行距收紧后行高 ≈ 30px（`.release-row` 的 5+5 padding
   + ~20px 内容 + 2px border），`30 × 3 + 2 × 2（gap 从 6 收到 2）= 94`，
   留 3px 余量取 91。**宁可少露半行也不裁半行**——被裁掉半截的那一条会被
   读成「这一条被压扁了」，而它其实是滚动区里正常的一部分。 */

.release-list {
  max-height: 91px;
  overflow-y: auto;
  /* 触屏 / 触控板甩到列表尽头时不要连带触发页面级手势——否则在列表底部
     再滚一下，页面会跟着跳，用户以为列表没到底。 */
  overscroll-behavior: contain;
  /* 滚动条默认贴在容器右缘，而外层卡片有内边距，短列表时看不出来，
     长列表时那条「悬空的轨道」很显眼。留 4px 把它拉近卡片边框。 */
  padding-right: 4px;
}

/* 已安装的版本通常只有一两条（用户很少囤），不设限高——加了反而让
   唯一一行上方多出一段空白。只约束 npm 那一列。 */

/* 磁盘占用报表的样式**放在这里而不是 theme.css**：theme.css 是反棘轮
   大文件（预算只许下调），而这份样式只被本组件用。拆出去的同时也让
   「哪段样式属于哪张卡片」变得可查。变量沿用 theme.css 的全局色板
   （--text / --muted / --bg-soft / --accent / --bad），scoped 不会改
   变它们的取值。 */

.disk-usage {
  /* 18+16 → 10+12：这一段是「版本列表」与「磁盘占用」两个功能区之间的
     分隔。上面已经有 1px 边框在划界，34px 的内外边距让那道线看起来是
     「浮」在半空中的，两段之间反而缺了紧密度。 */
  margin-top: 10px;
  padding-top: 12px;
  border-top: 1px solid var(--border);
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
  gap: 8px;
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
     每多 1px，两列并排的收益就被吃掉一点。 */
  padding: 6px 9px;
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

.usage-tile-total {
  /* 15 → 12.5：瓦片只有 186px 宽，主数字占到 15px 时「内核版本」四字
     与「878.2 MB」之间只剩 6px，选标题就只能截到「内核版…」。收到 12.5
     后两者都在一屏里读得全，而这行本来就是扫一眼比大小，不是逐位核对。 */
  font-size: 12.5px;
  font-weight: 700;
  color: var(--text);
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
  /* 5 → 3：标题与占比条紧挨着，它们说的是同一件事（这块多大）。 */
  margin: 3px 0 0;
}

.usage-tile-percent {
  font-size: 11px;
  color: var(--muted);
  white-space: nowrap;
  font-variant-numeric: tabular-nums;
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
  background: var(--accent);
  /* 极小的分类也要看得见：宽度按 sharePercent，但设下限，否则 0.02% 的
     备份会渲染成一条看不见的线。 */
  min-width: 2px;
}

.usage-entries {
  list-style: none;
  margin: 6px 0 0;
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
</style>
