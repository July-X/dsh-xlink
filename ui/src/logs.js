// 日志查看面板的状态与动作：<data_dir>/logs/ 下每个 *.log 一个侧签，
// 打开时读最新文件（按文件名排序，kernel.log 是实时输出），切签按需读取，
// 「刷新」重读当前签。
import { reactive } from 'vue';
import { invoke } from './bridge.js';
import { toastActionError } from './notify.js';
import { withLoading } from './loading.js';
import { stripAnsi } from './progress.js';

export const logModal = reactive({
  visible: false,
  files: [],
  activeName: null,
  content: '',
  // 正在读取的文件名（读一个签时只有那个签显示加载态）。旧实现是一个全局
  // boolean，切签或刷新时整块面板都在转圈，且 `withLoading` 对同一个 key
  // 的重入直接忽略 —— 大日志还没读完时再点「刷新」什么都不发生（P2-38）。
  loadingName: null,
});

/// 日志文件按用途分类；侧栏按此顺序自上而下渲染。`id` 是文件名 `name` 段
/// 的判据，详见 [`categorizeLogFile`]。
export const LOG_CATEGORIES = [
  { id: 'kernel', label: '内核日志' },
  { id: 'install', label: '内核安装' },
  { id: 'plugin-wiring', label: '插件接线' },
  { id: 'plugin', label: '插件构建' },
  { id: 'pnpm-install', label: 'pnpm 安装' },
  { id: 'node-install', label: 'Node 安装' },
  { id: 'update-cleanup', label: '更新清理' },
  { id: 'other', label: '其它日志' },
];

// 已知内核族（与 `src-tauri/src/registry.rs::KERNEL_FAMILY_*` 对齐），
// 用这份白名单区分实例感知格式与壳级格式，避免按 `-` 段数判定时把
// `release-install-0.1.2-rc.6-...` 这种 name 自带 `-` 的壳级日志误归类。
const KNOWN_FAMILIES = new Set(['dsh', 'mcode']);

// 文件名格式（定义在 `src-tauri/src/process.rs::log_file_name`）：
// - 壳级：`<kind>-<name>-<date>.log`（name 可含 `-`）
// - 实例感知：`<kind>-<family>-<instance_id>-<name>-<date>.log`
// 同时支持旧轮转备份 `<base>.log.<n>` 与新轮转备份 `<base>.<n>.log` 两种命名。
export function parseLogFilename(filename) {
  let stem = filename.replace(/\.log(?:\.\d+)?$/i, '');
  const genMatch = stem.match(/\.(\d+)$/);
  if (genMatch) stem = stem.slice(0, -genMatch[0].length);
  const parts = stem.split('-');
  if (parts.length < 4) return { name: stem };
  // 最后 3 段必须形如 `YYYY-MM-DD`，否则整体回退
  const [year, month, day] = parts.slice(-3);
  if (!/^\d{4}$/.test(year) || !/^\d{2}$/.test(month) || !/^\d{2}$/.test(day)) {
    return { name: stem };
  }
  const middle = parts.slice(0, -3);
  // middle[0] = kind；实例感知格式 middle[1] 是已知内核族
  if (middle.length >= 3 && KNOWN_FAMILIES.has(middle[1])) {
    return { name: middle.slice(3).join('-') };
  }
  return { name: middle.slice(1).join('-') };
}

// 把日志文件名分到 [`LOG_CATEGORIES`] 中的一类。判据只看 `name` 段。
export function categorizeLogFile(filename) {
  const { name } = parseLogFilename(filename);
  if (name === 'kernel') return 'kernel';
  if (name.startsWith('install-')) return 'install';
  if (name === 'plugin-wiring') return 'plugin-wiring';
  // 必须在 plugin-wiring 之后，否则会被这条通吃
  if (name.startsWith('plugin-')) return 'plugin';
  if (name.startsWith('pnpm-install-')) return 'pnpm-install';
  if (name === 'node-install') return 'node-install';
  if (name === 'shell-update-cleanup') return 'update-cleanup';
  return 'other';
}

// 把平铺的文件列表按 [`LOG_CATEGORIES`] 的顺序分组，组内保留原顺序
// （list_log_files 已按「基名逆序 + 代次升序」排好）。空类不渲染。
export function groupLogFiles(files) {
  const buckets = new Map();
  for (const category of LOG_CATEGORIES) buckets.set(category.id, []);
  for (const file of files || []) {
    buckets.get(categorizeLogFile(file.name)).push(file);
  }
  return LOG_CATEGORIES
    .map((category) => ({ ...category, files: buckets.get(category.id) }))
    .filter((group) => group.files.length > 0);
}

// 请求序号：只有最后一次发起的读取可以落内容，避免慢的旧响应盖掉新结果。
let logReadSeq = 0;

export function formatLogSize(bytes) {
  if (bytes < 1024) return bytes + ' B';
  if (bytes < 1024 * 1024) return (bytes / 1024).toFixed(1) + ' KB';
  return (bytes / (1024 * 1024)).toFixed(2) + ' MB';
}

// 日志展示的统一口径：磁盘保留 pnpm/tsdown 的原始终端输出（含 ANSI 颜色码），
// 展示前一律剥离；空文件给一句人话而不是空白。日志面板、全屏日志窗口与进度
// 浮层的实时流共用这一条，别再各写一遍 `stripAnsi(text || '') || '（暂无内容）'`。
export function displayLogText(text) {
  return stripAnsi(text || '') || '（暂无内容）';
}

export function loadActiveLog() {
  if (!logModal.activeName) {
    logModal.content = '（暂无日志）';
    return Promise.resolve();
  }
  const target = logModal.activeName;
  const seq = ++logReadSeq;
  logModal.content = '读取中…';
  logModal.loadingName = target;
  // 这里**不**用 `withLoading`：它对同一个 key 的重入直接忽略，会导致
  // 「正在读大日志时再点刷新毫无反应」。改为每次真正发起读取，用序号保证
  // 只有最新一次的结果落地（P2-38）。标题栏脉冲仍由 `withLoading` 的
  // 全局语义提供，这里用同一个 key 的空壳调用即可。
  const pulse = withLoading('logRead', () => Promise.resolve());
  return invoke('read_log_file', { name: target })
    .then((text) => {
      // 读取期间用户可能已切签，或有更新的请求在飞：只有最新一次落内容。
      if (seq !== logReadSeq || logModal.activeName !== target) return;
      logModal.content = displayLogText(text);
    })
    .catch((e) => {
      if (seq !== logReadSeq) return;
      logModal.content = '读取失败：' + e + '。可点击「刷新」重试，或打开数据目录查看 logs/。';
    })
    .finally(() => {
      if (seq === logReadSeq) logModal.loadingName = null;
      return pulse;
    });
}

export function switchLogTab(name) {
  if (name === logModal.activeName) {
    return loadActiveLog();
  }
  logModal.activeName = name;
  return loadActiveLog();
}

// 重新列文件，让新安装日志与轮转后的 kernel.log 出现；当前签还在就留在
// 原签，否则退回第一个签（或空态）。
export function refreshLogTabs() {
  return invoke('list_log_files')
    .then((files) => {
      logModal.files = files || [];
      const names = logModal.files.map((f) => f.name);
      const keep = logModal.activeName && names.includes(logModal.activeName) ? logModal.activeName : null;
      logModal.activeName = keep || names[0] || null;
      if (logModal.activeName) {
        return loadActiveLog();
      }
      logModal.content = '（暂无日志文件）';
      return null;
    })
    .catch((e) =>
      toastActionError('读取日志列表失败', e, '请点击「刷新」重试，或打开数据目录查看 logs/', 4000)
    );
}

export function showLogs() {
  logModal.visible = true;
  logModal.activeName = null;
  refreshLogTabs();
}

export function hideLogs() {
  logModal.visible = false;
}

/// 侧栏宽度持久化（localStorage）——主面板弹窗与独立全屏窗口共用，逻辑一致
/// 不应该各自实现一份。`globalThis.localStorage` 而不是裸 `localStorage`：
/// 浏览器里都解析得到，但单元测试在 Node 里跑，裸引用会抛 ReferenceError
/// 被 try/catch 吞掉，从而把所有调用都退化到默认值。`globalThis` 也允许
/// 测试用一个内存版的 stub 替换它。localStorage 可能被禁用（隐私模式 /
/// 磁盘满），读取与写入都吞异常，落到默认值不阻塞 UI。
const SIDEBAR_WIDTH_DEFAULTS = {
  modal: { default: 220, min: 180, max: 420 },
  window: { default: 240, min: 180, max: 560 },
};

export function loadSidebarWidth(storageKey, variant) {
  const range = SIDEBAR_WIDTH_DEFAULTS[variant] || SIDEBAR_WIDTH_DEFAULTS.modal;
  try {
    const ls = globalThis.localStorage;
    if (!ls) return range.default;
    const raw = ls.getItem(storageKey);
    if (!raw) return range.default;
    const parsed = parseInt(raw, 10);
    if (!Number.isFinite(parsed)) return range.default;
    return Math.max(range.min, Math.min(range.max, parsed));
  } catch {
    return range.default;
  }
}

export function saveSidebarWidth(storageKey, next) {
  try {
    const ls = globalThis.localStorage;
    if (!ls) return;
    ls.setItem(storageKey, String(next));
  } catch {
    // 吞掉：localStorage 不可用不该让 UI 报错
  }
}

/// 给一个滚动容器挂「滚动期间才显滚动条」行为：scroll 事件加 `.is-scrolling`，
/// 800ms 内无新滚动则移除（自然淡出）。返回解绑函数（unmount / 容器消失时调）。
/// 与 theme.css 的 `.log-content.is-scrolling` / `.log-tabs.is-scrolling`
/// 配套使用（两者默认 scrollbar-color 透明，滚动期间切到可见色）。
export function bindScrollAutoHide(el) {
  if (!el) return () => {};
  let timer = null;
  const onScroll = () => {
    el.classList.add('is-scrolling');
    if (timer) clearTimeout(timer);
    timer = setTimeout(() => el.classList.remove('is-scrolling'), 800);
  };
  el.addEventListener('scroll', onScroll, { passive: true });
  return () => {
    el.removeEventListener('scroll', onScroll);
    if (timer) clearTimeout(timer);
  };
}

/// 直接拉起独立分类日志窗口（跳过「先弹小窗再点全屏」的中间步骤）：
/// 概览页的「查看日志」、事故面板的「打开日志」都走这里。`open_log_window`
/// 必须给一个有效文件名，所以先列文件再挑一个——优先 kernel 分类的最新
/// 一条（用户排障的第一诉求），没有 kernel 日志就退回任意第一个；连
/// 一个日志文件都没有时给一句中文提示而不是静默开窗空跑。
///
/// 失败用 `toastActionError` 给下一步（重试 / 改用弹窗 / 看 logs/），
/// 不吞——与其它 IO 入口一致（P2-36）。
export function openLogsWindow() {
  return withLoading('openLogsWindow', () =>
    invoke('list_log_files')
      .then((files) => {
        const entries = files || [];
        if (!entries.length) {
          toastActionError(
            '暂无日志文件',
            '当前 logs/ 目录下没有任何 .log 文件',
            '可先启动内核或安装插件产生日志后再查看',
            4000,
          );
          return null;
        }
        const kernelGroup = groupLogFiles(entries).find((g) => g.id === 'kernel');
        const target = (kernelGroup && kernelGroup.files[0]) || entries[0];
        return invoke('open_log_window', { name: target.name }).catch((e) => {
          // 后端已经给出「下一步」文案，优先原样展示
          toastActionError(
            '打开日志窗口失败',
            e,
            '可改用主面板的「查看日志」弹窗，或重试',
            4000,
          );
          return null;
        });
      })
      .catch((e) =>
        toastActionError(
          '读取日志列表失败',
          e,
          '可点击「查看日志」打开主面板弹窗，或到数据目录查看 logs/',
          4000,
        )
      )
  );
}
