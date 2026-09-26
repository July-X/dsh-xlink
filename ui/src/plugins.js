// 插件管理的共享状态与动作：中央仓列表、插件中心目录、安装 / 更新 /
// 卸载 / 同步 / 模式切换。长任务统一走 withProgress（进度面板 + 日志流）。
import { reactive, ref } from 'vue';
import { invoke } from './bridge.js';
import { toast, toastSuccess, toastActionError } from './notify.js';
import { withLoading } from './loading.js';
import { withProgress } from './progress.js';
import { createStatusSource, createUpdateChecker, singleFlight } from './async.js';
import { refreshAll } from './store.js';

// dshfind.com 分类 id → 中文标签，数组顺序即界面顺序，与
// https://dshfind.com/zh/plugins 上的筛选分组保持一致。
export const CATALOG_CATEGORIES = [
  ['skin', '皮肤主题'],
  ['ui', '面板增强'],
  ['agent', 'Agent 增强'],
  ['memory', '记忆上下文'],
  ['client', '客户端'],
  ['channel', '通道通知'],
  ['tools', '工具集成'],
  ['fun', '趣味互动'],
  ['resource', '资源导航'],
];
// 目录是「一页页翻」的枚举面：步长太大时「显示更多」一次追加 60 条等于
// 5000+ px 滚动，用户点完按钮就找不到自己刚才看到哪了。24 条 ≈ 3 屏，
// 追加后仍能一眼看见列表末尾。
export const CATALOG_PAGE = 24;

export const pluginStore = reactive({
  view: null,
  catalogItems: [],
  catalogLoaded: false,
  category: 'all',
  shown: CATALOG_PAGE,
  query: '',
  sort: 'stars',
  filter: 'all',
  spec: '',
});

// 目录加载中独立 ref：模板里用 v-loading 绑定它，让 Element Plus 指令的
// `updated` 钩子能跟着值翻转（之前用字面值 v-loading="true"，updated 钩子
// 永远不进 `!binding.value` 关闭分支，依赖 unmounted 卸载 mask——某些路径下
// unmounted 未触发导致 mask 残留，"目录加载中"卡住不消失）。把状态抽成 ref
// 后指令行为可预期，bug 不再依赖 unmounted 钩子。
export const catalogLoading = ref(false);

const catalogIndex = new WeakMap();
const UPDATE_CHECK_TTL_MS = 15 * 60 * 1000;

function catalogMeta(item) {
  const cached = catalogIndex.get(item);
  if (cached) return cached;
  const haystack = [item.name, item.description, item.repo, item.category, categoryLabel(item.category)]
    .concat(item.tags || [])
    .join(' ')
    .toLowerCase();
  const meta = {
    haystack,
    name: String(item.name || '').toLowerCase(),
    updatedAt: Date.parse(item.updated || '') || 0,
  };
  catalogIndex.set(item, meta);
  return meta;
}

// 搜索命中权重：名称前缀 > 名称中段 > 其余字段。原先只有 `haystack.includes`，
// 命中与否是二值的，16034 条里任何一个高频词（如 "agent"）都会把结果搅成
// 目录原序——用户搜 agent 期待先看到名为 agent 的，而不是描述里提了一句
// agent 的。0 表示不命中。
function matchScore(item, q) {
  const meta = catalogMeta(item);
  if (meta.name.startsWith(q)) return 3;
  if (meta.name.includes(q)) return 2;
  return meta.haystack.includes(q) ? 1 : 0;
}

// 名称命中的高亮分段，供模板渲染 <mark>。按第一个出现位置切，避免长名称
// 出现多次时产生几十个碎片。无命中时返回单段原文。
export function matchParts(text, query) {
  const raw = String(text || '');
  const q = String(query || '').trim().toLowerCase();
  const at = q ? raw.toLowerCase().indexOf(q) : -1;
  if (at < 0) return [{ text: raw, hit: false }];
  return [
    { text: raw.slice(0, at), hit: false },
    { text: raw.slice(at, at + q.length), hit: true },
    { text: raw.slice(at + q.length), hit: false },
  ].filter((part) => part.text);
}

export function categoryLabel(id) {
  const hit = CATALOG_CATEGORIES.find(([key]) => key === id);
  return hit ? hit[1] : id || '未分类';
}

export function formatCount(n) {
  return n >= 1000 ? (n / 1000).toFixed(1).replace(/\.0$/, '') + 'k' : String(n);
}

export function formatUpdated(iso) {
  const t = Date.parse(iso || '');
  if (!t) return '';
  const days = Math.floor((Date.now() - t) / 86400000);
  if (days <= 0) return '今天更新';
  if (days === 1) return '昨天更新';
  if (days < 30) return days + ' 天前更新';
  return '更新于 ' + new Date(t).toISOString().slice(0, 10);
}

// 已安装判定：store 名（git 时为仓库末段）或 repo 全名，均小写比较。
export function installedKeys() {
  const keys = new Set();
  const rows = (pluginStore.view && pluginStore.view.rows) || [];
  rows.forEach((row) => {
    keys.add(String(row.name || '').toLowerCase());
    if (row.repo_url) {
      keys.add(row.repo_url.replace(/^https?:\/\/github\.com\//i, '').replace(/\.git$/, '').toLowerCase());
    }
  });
  return keys;
}

export function isInstalled(item, keys) {
  if (keys.has(String(item.name || '').toLowerCase())) return true;
  return item.repo ? keys.has(item.repo.toLowerCase()) : false;
}

export function filteredCatalog(keys) {
  const q = pluginStore.query.trim().toLowerCase();
  let items = pluginStore.catalogItems.filter((item) => {
    if (pluginStore.category !== 'all' && item.category !== pluginStore.category) return false;
    if (pluginStore.filter === 'installed' && !isInstalled(item, keys)) return false;
    if (pluginStore.filter === 'not-installed' && isInstalled(item, keys)) return false;
    return true;
  });
  // 有关键词时按相关度排（同级保持目录原序，即 stars 序，sort 用 stable
  // 比较保证确定）；没有关键词时才让「最近更新」这个显式选择接管。
  if (q) {
    items = items
      .map((item) => ({ item, score: matchScore(item, q) }))
      .filter((hit) => hit.score > 0)
      .sort((a, b) => b.score - a.score)
      .map((hit) => hit.item);
  } else if (pluginStore.sort === 'updated') {
    items = items.slice().sort((a, b) => catalogMeta(b).updatedAt - catalogMeta(a).updatedAt);
  }
  return items;
}

// 筛选是否偏离默认态：决定「清除筛选」按钮显不显示、计数行怎么措辞。
export function hasActiveFilter() {
  return (
    !!pluginStore.query.trim() ||
    pluginStore.category !== 'all' ||
    pluginStore.filter !== 'all'
  );
}

export function resetCatalogFilters() {
  pluginStore.query = '';
  pluginStore.category = 'all';
  pluginStore.filter = 'all';
  pluginStore.shown = CATALOG_PAGE;
}

// 目录拉取只在插件面板激活或用户手动刷新时执行；相同请求共享一个
// Promise，避免面板切换和按钮连点同时占用网络与解析资源。
//
// 历史 bug（两轮修复）：
// ① 之前用 `withExclusive` 包裹整个 task——工作台启停（同样走
//    withExclusive）进行期间点刷新，exclusiveActive 为 true → task 根本
//    没跑 → catalogLoaded 永远 false。已去掉 withExclusive（singleFlight
//    本身已防并发）。
// ② 即使前端逻辑全对，后端最坏也要 30s（dshfind 主源超时）+ 30s
//    （market 回退源超时）= 60s 才报错，期间 mask 一直转，用户感知就是
//    「卡死」。加 75s 看门狗兜底：无论 IPC/后端发生什么，超时后强制
//    落到终态并提示重试，spinner 永远不可能无限转。
const CATALOG_WATCHDOG_MS = 75 * 1000;
const loadCatalogOnce = singleFlight((manual) => {
  const startRequest = () => {
    catalogLoading.value = true;
    pluginStore.catalogLoaded = false;
    const finalize = () => {
      catalogLoading.value = false;
      pluginStore.catalogLoaded = true;
    };
    let settled = false;
    return new Promise((resolve) => {
      const watchdog = setTimeout(() => {
        if (settled) return;
        settled = true;
        if (manual) {
          toastActionError(
            '插件目录加载超时',
            new Error(`等待后端响应超过 ${CATALOG_WATCHDOG_MS / 1000} 秒`),
            '请检查网络或代理设置后重试；已安装插件不受影响；若反复出现，点「查看日志」把最近的输出发给维护者',
            8000,
          );
        }
        resolve(null);
      }, CATALOG_WATCHDOG_MS);
      invoke('plugin_catalog', { force: !!manual })
        .then((items) => {
          // 看门狗先触发、数据迟到仍照常入库：列表静默刷新，比丢掉新鲜
          // 数据更好；只是不再重复 resolve / 弹错误。
          pluginStore.catalogItems = items || [];
          pluginStore.shown = CATALOG_PAGE;
          if (!settled) {
            settled = true;
            clearTimeout(watchdog);
            resolve(items || []);
          }
        })
        .catch((e) => {
          if (settled) return;
          settled = true;
          clearTimeout(watchdog);
          if (manual) toastActionError('插件目录加载失败', e, '请检查网络或代理设置后重试；已安装插件不受影响', 6000);
          resolve(null);
        })
        .finally(finalize);
    });
  };
  return manual ? withLoading('catalogReload', startRequest) : startRequest();
});

export function loadCatalog(manual = false) {
  if (!manual && pluginStore.catalogLoaded) {
    return Promise.resolve(pluginStore.catalogItems);
  }
  return loadCatalogOnce(manual);
}

// --- 安装 / 更新 / 卸载 / 同步 -----------------------------------------------

export function installPlugin(specFromCatalog) {
  const fromCatalog = !!(specFromCatalog || '').trim();
  const raw = (specFromCatalog || '').trim() || pluginStore.spec.trim();
  if (!raw) {
    toast('请先填写仓库地址或 npm 包名', 4000, 'warning');
    return Promise.resolve(false);
  }
  if (!fromCatalog) {
    pluginStore.spec = '';
  }
  // 物化模式默认走链接（plugin_install 的 mode 缺省回退到 link）；
  // 「切换为复制 / 切换为链接」按钮才是模式权威入口。
  return withProgress(
    {
      cmd: 'plugin_install',
      start: '正在安装插件 ' + raw + ' …',
      done: '插件 ' + raw + ' 已安装（重启内核后生效）',
      fail: '安装失败：' + raw,
    },
    (channel) => ({ spec: raw, onEvent: channel })
  );
}

export function updatePlugin(id) {
  return withProgress(
    { cmd: 'plugin_update', start: '正在更新插件 …' },
    (channel) => ({ id, onEvent: channel })
  ).then((ok) => {
    if (ok) {
      toastSuccess('插件已更新，重启内核后生效');
    }
  });
}

export function setPluginMode(id, mode) {
  const label = mode === 'copy' ? '复制' : '链接';
  return withProgress(
    {
      cmd: 'plugin_set_mode',
      start: '正在切换为' + label + '模式 …',
      done: '已切换为' + label + '模式',
    },
    (channel) => ({ id, mode, onEvent: channel })
  );
}

// 恢复启用（清除隔离记录并重新接线）或直接卸载被隔离的插件；
// 与事故面板共用 plugin_resolve 命令，恢复后需重启工作台生效。
export function resolvePluginQuarantine(id, action) {
  return withProgress(
    {
      cmd: 'plugin_resolve',
      start: action === 'remove' ? '正在卸载插件 …' : '正在恢复插件接线 …',
      done: action === 'remove' ? '插件已移除' : '插件已恢复，重启工作台后生效',
      fail: action === 'remove' ? '卸载失败' : '恢复失败',
    },
    (channel) => ({ id, action, onEvent: channel })
  );
}

export function syncPlugins() {
  return withProgress(
    { cmd: 'plugin_sync', start: '正在同步插件到所有内核 …', done: '插件已同步' },
    (channel) => ({ onEvent: channel })
  );
}

export function uninstallPlugin(id) {
  return withProgress(
    { cmd: 'plugin_uninstall', start: '正在卸载插件 …', done: '插件已卸载', fail: '卸载失败' },
    (channel) => ({ id, onEvent: channel })
  );
}

// 手动检查挂按钮 loading、有更新时提示；启动自检静默（失败不打扰用户）。
// 策略（TTL / 退避 / 互斥 / 去重 / 提示）见 async.js 的 createUpdateChecker。
export const checkPluginUpdates = createUpdateChecker({
  cmd: 'plugin_check_updates',
  loadingKey: 'checkPluginUpdates',
  noun: '插件',
  ttlMs: UPDATE_CHECK_TTL_MS,
  itemFailureHint: '请检查网络或代理设置后重试',
  after: () => refreshAll(),
});

// refreshAll 的插件侧钩子：与内核状态一起刷新插件卡片。
export const refreshPlugins = createStatusSource('plugin_status', (view) => {
  pluginStore.view = view;
});
