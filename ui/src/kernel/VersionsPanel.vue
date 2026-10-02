<script setup>
// 内核版本：左列已安装（切换 / 删除），右列 npm 发布（仅安装）。
// 「切换」必须基于本地已安装版本，避免误把尚未安装的远端版本当成可立刻启用的内核。
// 「检查更新」从 npm registry 拉取版本列表。
//
// 面板挂载时主动调一次 refreshAll()，让「已安装」列表在用户进到这一页时就是最新的，
// 而不是要等启动阶段的 get_status，或者「检查更新」之后才看到本地版本。
import { computed, onMounted, reactive } from 'vue';
import { Refresh, Download, Promotion, Delete, InfoFilled, TopRight } from '@element-plus/icons-vue';
import {
  store,
  refreshAll,
  checkUpdates,
  installVersion,
  activateVersion,
  removeVersion,
  workbenchActiveNow,
} from '../store.js';
import { invoke } from '../shell/bridge.js';
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

// 进版本面板就重新扫描本地内核列表，与 npm 发布列解耦——
onMounted(() => {
  refreshAll();
  // 占用报表**不**跟着自动算：一次全量扫描要走 2.5 万个文件（本机实测
  // 290ms），放在进面板时算会让「点进版本页」这件事变慢，而用户未必想
  // 看占用。按需加载（点按钮），读过一次就缓存。
});

// 磁盘占用：按需加载 + 缓存。
//
// 沿用上方插件快照那套「槽位」写法（`loaded` 与 `error` 分开），但语义有
// 一处**故意不同**：插件快照失败时不置 `loaded`，以便下次悬浮重试；而占用
// 失败就**保持失败态**并显示原因——它是一次全盘扫描，重试三次也还是失败
// 的（多半是权限问题），让按钮反复可点只会诱导用户空转。真要重来请刷新页面。
const diskUsage = reactive({
  loading: false,
  loaded: false,
  error: null,
  total: 0,
  groups: [],
  unreadable: [],
});

async function loadDiskUsage() {
  if (diskUsage.loading) return;
  diskUsage.loading = true;
  diskUsage.error = null;
  try {
    const report = await invoke('disk_usage');
    // 数值一律过一遍 Number：后端字段缺失时拿到 undefined，会在
    // formatBytes 里被 `Number.isFinite` 兜成 0，但 `total` 若直接
    // 参与模板拼接就会渲染成 "NaN" 或 "undefined"。
    diskUsage.total = Number(report.total) || 0;
    diskUsage.groups = report.groups || [];
    diskUsage.unreadable = report.unreadable || [];
    diskUsage.loaded = true;
  } catch (e) {
    diskUsage.error = e && e.message ? e.message : String(e);
  } finally {
    diskUsage.loading = false;
  }
}

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
                  硬链接复用也无法在纯目录遍历层面识别。
                </div>
              </template>
              <el-icon class="card-info-icon"><InfoFilled /></el-icon>
            </el-tooltip>
          </h2>
          <span class="head-meta">
            <span v-if="diskUsage.loaded" class="usage-total">
              合计 {{ formatBytes(diskUsage.total) }}
            </span>
            <el-button
              v-if="!diskUsage.loaded"
              size="small"
              text
              :icon="Refresh"
              :loading="diskUsage.loading"
              @click="loadDiskUsage"
            >
              统计
            </el-button>
            <el-button
              v-else
              size="small"
              text
              :icon="Refresh"
              :loading="diskUsage.loading"
              :disabled="diskUsage.error !== null"
              title="重新统计目录占用"
              @click="loadDiskUsage"
            >
              刷新
            </el-button>
          </span>
        </div>

        <p v-if="diskUsage.error" class="muted usage-error">
          统计失败：{{ diskUsage.error }}
        </p>

        <div v-else-if="diskUsage.loaded" class="usage-groups">
          <div v-for="group in diskUsage.groups" :key="group.id" class="usage-group">
            <div class="usage-group-head">
              <span class="usage-group-label">{{ group.label }}</span>
              <span class="muted">
                {{ formatBytes(group.bytes) }} · {{ group.sharePercent }}%
              </span>
            </div>
            <ul v-if="group.entries.length" class="usage-entries">
              <li v-for="entry in group.entries" :key="entry.id" class="usage-entry">
                <span class="usage-entry-name" :title="entry.path">{{ entry.label }}</span>
                <span class="usage-bar" aria-hidden="true">
                  <span class="usage-bar-fill" :style="{ width: entry.sharePercent + '%' }"></span>
                </span>
                <span class="usage-entry-bytes">{{ formatBytes(entry.bytes) }}</span>
              </li>
            </ul>
            <p v-else class="muted usage-empty">这一类当前没有内容。</p>
          </div>

          <p v-if="diskUsage.unreadable.length" class="muted usage-unreadable">
            以下目录读不到，未计入合计：{{ diskUsage.unreadable.join('、') }}
          </p>
        </div>

        <p v-else-if="!diskUsage.loading" class="muted usage-idle">
          点「统计」扫描本机占用。目录较多时需要几百毫秒。
        </p>
      </div>
    </div>
  </section>
</template>

<style scoped>
/* npm 发布列表可能几十上百条（每个 rc / alpha 都是一条）。不加约束时整张
   卡片会长到把页面撑出**外层**滚动条——而面板外层本来就有自己的滚动，
   于是变成两层滚动条，鼠标滚轮归属变得难猜。限高到 3 条 + 内部滚动后，
   外层滚动条的位置与行为不变，只把长列表收进自己的容器里。

   3 条按「行高 ≈ 37px（8+8 padding + 13px 字）+ 2 个 6px gap」算，留
   4px 余量避免最后一条被裁掉半行。 */

.release-list {
  max-height: 123px;
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
  margin-top: 18px;
  padding-top: 16px;
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
.usage-error,
.usage-empty,
.usage-unreadable {
  font-size: 11.5px;
  margin: 0;
  line-height: 1.6;
}

.usage-error {
  color: var(--bad);
}

.usage-unreadable {
  margin-top: 10px;
  /* 路径可能很长，允许断行而不是撑破卡片。 */
  word-break: break-all;
}

.usage-group {
  margin-top: 12px;
}

.usage-group-head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 8px;
  font-size: 12px;
}

.usage-group-label {
  font-weight: 600;
  color: var(--text);
}

/* 数字用等宽字形：逐行对齐才好比较大小。 */
.usage-group-head .muted,
.usage-entry-bytes {
  font-variant-numeric: tabular-nums;
}

.usage-entries {
  list-style: none;
  margin: 6px 0 0;
  padding: 0;
}

.usage-entry {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 88px 68px;
  align-items: center;
  gap: 8px;
  padding: 3px 0;
  font-size: 11.5px;
}

.usage-entry-name {
  color: var(--text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.usage-bar {
  height: 5px;
  border-radius: 3px;
  background: var(--bg-soft);
  overflow: hidden;
}

.usage-bar-fill {
  display: block;
  height: 100%;
  border-radius: 3px;
  background: var(--accent);
  /* 极小的条目也要看得见：宽度按 sharePercent，但设下限，
     否则 0.01% 的插件库会渲染成一条看不见的线。 */
  min-width: 2px;
}

.usage-entry-bytes {
  text-align: right;
  color: var(--muted);
}
</style>
