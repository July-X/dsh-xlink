// 插件管理的共享状态与动作：中央库列表、手动安装 / 更新 / 卸载 /
// 同步 / 模式切换。长任务统一走 withProgress（进度面板 + 日志流）。
import { reactive } from 'vue';
import { invoke } from '../shell/bridge.js';
import { toast, toastSuccess, toastActionError } from '../shell/notify.js';
import { withProgress } from '../shell/progress.js';
import { createStatusSource, createUpdateChecker } from '../shell/async.js';
import { store, refreshAll } from '../store.js';

export const pluginStore = reactive({
  view: null,
  // 手动安装输入框里的 spec。安装动作只针对本机插件库，与目录/搜索无关。
  spec: '',
});

const UPDATE_CHECK_TTL_MS = 15 * 60 * 1000;

// --- 安装 / 更新 / 卸载 / 同步 -----------------------------------------------

export function installPlugin() {
  const raw = pluginStore.spec.trim();
  if (!raw) {
    toast('请先填写仓库地址或 npm 包名', 4000, 'warning');
    return Promise.resolve(false);
  }
  pluginStore.spec = '';
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
export function precheckPlugin(specOverride) {
  const override = String(specOverride || '').trim();
  const raw = override || pluginStore.spec.trim();
  if (!raw) {
    toast('请先填写仓库地址或 npm 包名', 4000, 'warning');
    return Promise.resolve(false);
  }
  // 面板自己的输入框在预检完成后清空；诊断页传入的 spec 不属于输入框，
  // 不能顺手把用户正在输入的内容清掉。
  if (!override) {
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
        '已安装，但装的来源与预检报告对不上：' + drift + '。请到「已安装」清单核对实际装了什么。',
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
