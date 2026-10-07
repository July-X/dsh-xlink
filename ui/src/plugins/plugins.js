// 插件管理的共享状态与动作：中央仓列表、插件中心目录、安装 / 更新 /
// 卸载 / 同步 / 模式切换。长任务统一走 withProgress（进度面板 + 日志流）。
import { reactive, ref } from 'vue';
import { invoke } from '../shell/bridge.js';
import { toast, toastSuccess, toastActionError } from '../shell/notify.js';
import { withLoading } from '../shell/loading.js';
import { withProgress } from '../shell/progress.js';
import { createStatusSource, createUpdateChecker, singleFlight } from '../shell/async.js';
import { store, refreshAll } from '../store.js';

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
// 追加后仍能一眼看见列表末尾。它同时是**一次搜索要回多少条**（后端上限
// 200，见 `plugins/catalog.rs` 的 MAX_LIMIT）。
export const CATALOG_PAGE = 24;

// 分类中文名随搜索请求一起发给后端：dshfind 目录里分类 id 是英文
// （memory / ui / …），中文名只在这里有。不带过去的话，用户搜「记忆」只能
// 命中描述里恰好带这两个字的少数条目，而他明明能在下拉里看到「记忆上下文」。
// **唯一定义是上面那张表**，后端不另抄一份。
const CATEGORY_LABELS = CATALOG_CATEGORIES.reduce((labels, [id, label]) => {
  labels[id] = label;
  return labels;
}, {});

export const pluginStore = reactive({
  view: null,
  // 「插件仓库」当前这一页（后端搜完直接回它，不再整份搬进浏览器）。
  catalogItems: [],
  // 本次条件下的命中总数——「显示更多（还有 N 个）」据此判断。
  catalogTotal: 0,
  // 分类 id → 命中条数，分类下拉的计数。后端在关键词范围内统计，所以
  // 「全部（128）」与「记忆上下文（37）」讲的是同一次搜索的两面。
  catalogCounts: {},
  catalogLoaded: false,
  category: 'all',
  // 已显示的条数；它同时是下一次搜索的 limit（「显示更多」累加它）。
  shown: CATALOG_PAGE,
  // **已提交**的关键词。输入框里的草稿不发出去，回车才提交——用户要的是
  // 「敲完这一串再搜一次」，不是每敲一个字就重扫远端目录。
  query: '',
  sort: 'stars',
  spec: '',
});

// 目录加载中独立 ref：模板里用 v-loading 绑定它，让 Element Plus 指令的
// `updated` 钩子能跟着值翻转（之前用字面值 v-loading="true"，updated 钩子
// 永远不进 `!binding.value` 关闭分支，依赖 unmounted 卸载 mask——某些路径下
// unmounted 未触发导致 mask 残留，"目录加载中"卡住不消失）。把状态抽成 ref
// 后指令行为可预期，bug 不再依赖 unmounted 钩子。
export const catalogLoading = ref(false);

const UPDATE_CHECK_TTL_MS = 15 * 60 * 1000;

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

// 分类下拉的选项：后端给的分类计数 + 上面那张中文名表。计数里没有的分类
// 不出现（这一轮搜索一条都没命中），但**表外的分类 id** 仍然要列出来——
// 回退数据源（参考市场）的分类不受 dshfind 那九个 id 约束。
export function catalogCategories() {
  const counts = pluginStore.catalogCounts || {};
  const chips = [{ id: 'all', label: '全部', count: pluginStore.catalogTotal }];
  CATALOG_CATEGORIES.forEach(([id, label]) => {
    if (counts[id]) chips.push({ id, label, count: counts[id] });
  });
  Object.keys(counts).forEach((id) => {
    if (id && !CATALOG_CATEGORIES.some(([key]) => key === id)) {
      chips.push({ id, label: id, count: counts[id] });
    }
  });
  return chips;
}

// 搜索只在插件面板打开、切分类 / 切排序、输入框回车、「显示更多」与手动
// 刷新时发起；相同请求共享一个 Promise，避免连点同时占用网络与解析资源。
//
// 为什么带着「关键词 + 分类 + 排序」三个参数去后端，而不是把 1.7 万条目录
// 搬进 webview 再筛：那份 JSON 有 9 MB，整份过一次 IPC、再常驻成 JS 对象
// 图、再在每次按键时重扫一遍——三样都在主线程上。挪到后端之后，一次搜索
// 过 IPC 的只有当前这一页（见 src-tauri/src/plugins/catalog.rs）。
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
const searchCatalogOnce = singleFlight((opts) => {
  const force = !!(opts && opts.force);
  // 静默 / 报错由**谁发起的这次搜索**决定，不是「手动 vs 自动」：面板打开
  // 时那次首屏搜索失败不该弹提示（用户没做任何操作），而他敲了回车、切了
  // 分类还看不到结果时必须知道为什么。
  const loud = !!(opts && opts.loud);
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
        // 看门狗必须自己落终态：finalize 原本只挂在 invoke 链的 .finally
        // 上，若 invoke 因 IPC 异常永远不 settle，spinner 仍会无限转——
        // 看门狗就白设了。这里直接关 spinner、置 catalogLoaded，迟到的
        // invoke 结果由下方 then/catch 静默入库（catalogLoaded 已是 true，
        // 列表照常刷新）。
        finalize();
        if (loud) {
          toastActionError(
            '插件目录加载超时',
            new Error(`等待后端响应超过 ${CATALOG_WATCHDOG_MS / 1000} 秒`),
            '请检查网络或代理设置后重试；已安装插件不受影响；若反复出现，点「查看日志」把最近的输出发给维护者',
            8000,
          );
        }
        resolve(null);
      }, CATALOG_WATCHDOG_MS);
      invoke('plugin_catalog_search', {
        query: pluginStore.query,
        category: pluginStore.category,
        sort: pluginStore.sort,
        limit: pluginStore.shown,
        labels: CATEGORY_LABELS,
        force,
      })
        .then((page) => {
          // 看门狗先触发、数据迟到仍照常入库：列表静默刷新，比丢掉新鲜
          // 数据更好；只是不再重复 resolve / 弹错误。
          if (page) {
            pluginStore.catalogItems = page.items || [];
            pluginStore.catalogTotal = page.total || 0;
            pluginStore.catalogCounts = page.counts || {};
          }
          if (!settled) {
            settled = true;
            clearTimeout(watchdog);
            resolve(page || null);
          }
        })
        .catch((e) => {
          if (settled) return;
          settled = true;
          clearTimeout(watchdog);
          if (loud) toastActionError('插件目录加载失败', e, '请检查网络或代理设置后重试；已安装插件不受影响', 6000);
          resolve(null);
        })
        .finally(finalize);
    });
  };
  return force ? withLoading('catalogReload', startRequest) : startRequest();
});

// 按当前的关键词 / 分类 / 排序搜一页。
//   force  跳过目录缓存窗口重新拉取（「刷新数据」）
//   loud   失败时弹提示（用户自己发起的搜索）
export function searchCatalog(opts = {}) {
  return searchCatalogOnce(opts);
}

// 面板第一次打开时载入第一页；已经载过就不重复打后端。
export function loadCatalog() {
  if (pluginStore.catalogLoaded) return Promise.resolve(null);
  return searchCatalog();
}

// 输入框回车 / 点清除：把草稿提交成一次新的搜索，并回到第一页。
export function submitCatalogSearch(text) {
  pluginStore.query = String(text == null ? pluginStore.query : text).trim();
  pluginStore.shown = CATALOG_PAGE;
  return searchCatalog({ loud: true });
}

// 切分类 / 切排序：三个控件里的任意一个变了就重搜一次，分页回到第一页。
// 「显示更多」也走这里——它不重置 `shown`，而是先把它累加上去再搜。
export function refineCatalog(patch) {
  Object.assign(pluginStore, patch);
  return searchCatalog({ loud: true });
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

// 安装前先在一次性沙盒实例里真的装一次、真的起一次内核，通过了才安装到
// 当前实例。报告有三态，通过与否由 `PrecheckDialog` 如实呈现，不在这里
// 折成一句「完成 / 失败」——那会把「没能验证」说成「没问题」。
export function precheckPlugin(specFromCatalog) {
  const fromCatalog = !!(specFromCatalog || '').trim();
  const raw = (specFromCatalog || '').trim() || pluginStore.spec.trim();
  if (!raw) {
    toast('请先填写仓库地址或 npm 包名', 4000, 'warning');
    return Promise.resolve(false);
  }
  if (!fromCatalog) {
    pluginStore.spec = '';
  }
  return withProgress(
    {
      cmd: 'plugin_precheck_install',
      // 会起两次临时内核（一次基线、一次带插件），文案必须让用户知道
      // 这不是卡住了。
      start: '正在预检 ' + raw + '（要启动两次临时内核，请稍候）…',
      // **跑完什么都没装**（两阶段契约，2026-10-06 用户拍板）。这句 done
      // 刻意不说「已安装」——预检只证明「装上去能起来」，装不装由用户点
      // 「应用变更」决定。含糊的文案会让用户以为插件已经在里面了。
      done: '预检完成 · ' + raw + ' 还没有装进当前实例',
      onResult: showPrecheckReport,
    },
    (channel) => ({ spec: raw, onEvent: channel })
  );
}

/**
 * 把预检通过的插件应用到当前实例（两阶段契约的第二阶段）。
 *
 * 收**整份预检报告**而不是一个 spec 字符串：第二阶段要用的来源必须来自报告
 * 的 `applySpec`（后端给的、剥过凭据的原始 spec），不能由 UI 拿 `pluginId`
 * 猜（审查 R2-P1-01）。`pluginId` 是中央库 id，与 spec 语义不同——
 * `@scope/pkg` 的 id 可能是 `@scope__pkg`，`owner/repo#v1` 还带 pin，
 * 拿它去重新解析会装上别的东西。
 *
 * 成功后用**回传的**报告替换 `store.precheckReport`，并核对它的来源与预检
 * 报告逐字对应。对不上就是「装的不是验过的那个」，必须让用户看见而不是
 * 一句「已安装」盖过去。
 *
 * 成功文案必须带上「不再重新验证」这一层：后端不会重跑沙盒，而用户在报告
 * 确认之后可能过了几分钟。这两件事都在界面上说出来，比让用户自己推断
 * 「刚才验的应该还算数」要诚实。
 */
export function applyPluginChange(precheckReport) {
  const report = precheckReport || null;
  const raw = String(report?.applySpec || '').trim() || pluginStore.spec.trim();
  if (!raw) {
    toast('没有可应用的插件来源，请先做一次安装预检', 4000, 'warning');
    return Promise.resolve(false);
  }
  // `withProgress` 只 resolve 布尔值，真正的报告从 `onResult` 拿（审查
  // R2-P1-02）。过去把 `true` 当报告存进了 store，诊断页读
  // `report.verdict` / `installed` / `preChangeSnapshotId` 全是 undefined。
  let applied = null;
  return withProgress(
    {
      cmd: 'plugin_precheck_apply',
      start: '正在把 ' + raw + ' 装到当前实例 …',
      done: '已安装 ' + raw + ' 到当前实例',
      onResult: (result) => {
        applied = result;
      },
    },
    (channel) => ({
      spec: raw,
      mode: 'link',
      verifiedAtMs: report?.verifiedAtMs || 0,
      onEvent: channel,
    })
  ).then((ok) => {
    if (!ok || !applied) return false;
    const drift = sourceDrift(report, applied);
    // 报告换成「已安装」这份：诊断页上「恢复变更前状态」要拿这里的
    // preChangeSnapshotId，没有它那个按钮只能把用户丢到快照列表。
    store.precheckReport = applied;
    store.precheckVisible = true;
    if (drift) {
      // 装**已经发生**了，不能说成「没装上」；要说清装上的是不是验过的那个。
      toast(
        '已安装，但装的来源与预检报告对不上：' + drift + '。请到插件中心核对实际装了什么。',
        9000,
        'warning'
      );
    }
    return true;
  });
}

/** 应用后回传的来源与预检报告不一致时，说清是哪一项对不上。 */
function sourceDrift(precheckReport, applied) {
  if (!precheckReport) return '';
  const fields = [
    ['来源类型', precheckReport.sourceKind, applied.sourceKind],
    ['来源', precheckReport.sourceLabel, applied.sourceLabel],
    ['版本', precheckReport.pin, applied.pin],
  ];
  const diffs = fields
    .filter(([, before, after]) => String(before || '') !== String(after || ''))
    .map(([label, before, after]) => label + ' ' + (before || '（无）') + ' → ' + (after || '（无）'));
  return diffs.join('；');
}

function showPrecheckReport(report) {
  if (!report) return;
  store.precheckReport = report;
  store.precheckVisible = true;
}

// 安装预检开关。关闭后「安装」直接装、不再起临时内核。
export function setPrecheckEnabled(enabled) {
  return invoke('plugin_set_precheck', { enabled })
    .then((value) => {
      toastSuccess(value ? '已开启安装预检' : '已关闭安装预检');
      return refreshAll();
    })
    .catch((e) => {
      toastActionError('切换安装预检失败', e, '请稍后重试；若持续失败请查看日志');
      return false;
    });
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

export function uninstallPlugin(id, options = {}) {
  const cleanup = options.cleanup === true;
  return withProgress(
    {
      cmd: 'plugin_uninstall',
      start: cleanup ? '正在移除并清理插件 …' : '正在卸载插件 …',
      done: cleanup ? '插件及其本地文件已清理' : '插件已卸载',
      fail: cleanup ? '清理插件失败' : '卸载失败',
    },
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
