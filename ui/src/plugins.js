// 插件管理的共享状态与动作：中央仓列表、插件中心目录、安装 / 更新 /
// 卸载 / 同步 / 模式切换。长任务统一走 withProgress（进度面板 + 日志流）。
import { reactive } from 'vue';
import { invoke, openExternal } from './bridge.js';
import { toast, toastSuccess, toastError } from './notify.js';
import { withLoading } from './loading.js';
import { withProgress } from './progress.js';
import { refreshAll } from './store.js';

// dsh-plugin.org 分类 id → 中文标签，数组顺序即界面顺序。
export const CATALOG_CATEGORIES = [
  ['interface', '界面体验'],
  ['session', '会话消息'],
  ['memory', '记忆上下文'],
  ['tools', '工具能力'],
  ['agent', '技能智能体'],
  ['workflow', '工作流'],
  ['integration', '集成连接'],
  ['model', '模型推理'],
  ['dev', '开发运维'],
  ['knowledge', '数据知识'],
  ['fun', '娱乐'],
];
export const CATALOG_PAGE = 60;

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
    if (!q) return true;
    const hay = [item.name, item.description, item.repo, item.category, categoryLabel(item.category)]
      .concat(item.tags || [])
      .join(' ')
      .toLowerCase();
    return hay.includes(q);
  });
  if (pluginStore.sort === 'updated') {
    items = items.slice().sort((a, b) => Date.parse(b.updated || '') - Date.parse(a.updated || ''));
  }
  return items;
}

// 目录拉取是纯网络读取：只给刷新按钮本身挂 loading，不锁整个外壳，
// 用户在拉取期间仍可操作侧栏、启停工作台、搜索 / 排序 / 翻页。
export function loadCatalog(manual) {
  pluginStore.catalogLoaded = false;
  const run = () =>
    invoke('plugin_catalog', { force: !!manual })
      .then((items) => {
        pluginStore.catalogItems = items || [];
        pluginStore.catalogLoaded = true;
        pluginStore.shown = CATALOG_PAGE;
      })
      .catch((e) => {
        pluginStore.catalogLoaded = true;
        toastError('目录加载失败：' + e, 6000);
      });
  return manual ? withLoading('catalogReload', run) : run();
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
export function checkPluginUpdates(opts) {
  const run = () =>
    invoke('plugin_check_updates')
      .then((infos) => {
        const n = (infos || []).filter((i) => i.latest).length;
        if (n > 0 && opts.toastOnUpdates) {
          toast('有 ' + n + ' 个插件可更新', 5000, 'warning');
        }
        return refreshAll();
      })
      .catch((e) => {
        if (opts.busy) {
          toastError('检查插件更新失败：' + e, 6000);
        }
      });
  return opts.busy ? withLoading('checkPluginUpdates', run) : run();
}

// refreshAll 的插件侧钩子：与内核状态一起刷新插件卡片。
export function refreshPlugins() {
  return invoke('plugin_status')
    .then((view) => {
      pluginStore.view = view;
    })
    .catch(() => {
      // 静默刷新：读取失败时保留旧卡片，下次 refreshAll 再试。
    });
}

export { openExternal };
