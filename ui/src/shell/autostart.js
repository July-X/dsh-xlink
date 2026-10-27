// 登录自启与「后台常驻」的状态与动作。
//
// 真相全在 Rust 侧：登录项写在系统里（macOS 的 LaunchAgent、Windows 的
// HKCU\...\Run），前端不自己判断「开没开」，只读后端报回的状态并原样呈现。
// 这与通知开关同一个道理——两边各存一份真相必然漂，而这里漂了还是**用户
// 看得见的**（开关显示已开、开机却没启动）。
//
// 三个开关的关系（Rust 侧 `should_launch_kernel_on_autostart` 是最终判据）：
//   · 后台常驻：关窗不退出，内核继续跑。**默认开**，无开关（行为已是新的）。
//   · 开机自启动：注册系统登录项。默认关——开机就多一个后台程序。
//   · 开机启动工作台：登录项拉起后连内核一起起。默认关，且**依赖上一条**。
//   · 启动时后台预启动：普通启动时把内核冷启动挪到后台。**默认开**，理由见
//     `autostartStore.prestart` 那段注释——它不注册任何系统级东西。
import { reactive } from 'vue';
import { invoke } from '../shell/bridge.js';
import { toastActionError, toastSuccess } from '../shell/notify.js';
import { withLoading } from '../shell/loading.js';
import { createStatusSource } from '../shell/async.js';

// 与 Rust 侧 `AutostartStatus` 的 `#[serde(default)]` 缺省形态对齐：读不到时
// 一律按「没开」呈现，而不是乐观地显示成已开启。
const FALLBACKS = Object.freeze({
  enabled: false,
  supported: true,
  stale: false,
  note: null,
  // 启动时后台预启动：默认**开**，与 Rust 侧 `Settings::prestart_kernel` 的
  // 「`None` 读作 true」对齐。这一项与其余几项不同，读不到时不能按「没开」
  // 兜底，否则默认行为会被前端显示反。
  prestart: true,
});

const SET_KEY = 'autostartSet';
const SET_KERNEL_KEY = 'autostartSetKernel';
const SET_PRESTART_KEY = 'prestartSetKernel';

export const autostartStore = reactive({
  ...FALLBACKS,
  // 「开机启动工作台」是壳自己的设置（不是系统登录项），由
  // autostart_status 顺带带回，省掉一次单独的命令往返。
  kernel: false,
  // 「启动时后台预启动」同理：壳自己的设置，由 status 带回。**默认开**，
  // 与另外两个自启开关默认关不同——它不注册任何系统级东西，只在普通启动
  // 时 2s 后在后台先预热内核树、再拉起内核（不弹窗、不抢焦点），把「隔离
  // 冷启动约 1 分钟」挪到用户点开工作台之前。登录项拉起不参与，那是
  // 「开机启动工作台」开关的事。
  prestart: true,
  // 本次进程是不是由登录项拉起来的。用来给一句实话：此刻没有面板，
  // 「刚才那个开关怎么没反应」的答案就在这里。
  startedByAutostart: false,
});

function boolOr(value, fallback) {
  return typeof value === 'boolean' ? value : fallback;
}

function normalizeStatus(raw) {
  if (!raw || typeof raw !== 'object') return null;
  return {
    enabled: boolOr(raw.enabled, FALLBACKS.enabled),
    supported: boolOr(raw.supported, FALLBACKS.supported),
    stale: boolOr(raw.stale, FALLBACKS.stale),
    note:
      typeof raw.note === 'string' && raw.note.trim() ? raw.note.trim() : null,
    // kernel 字段由 Rust 的 status 命令带上（见下）；缺失时保留现值而不是
    // 悄悄改成 false——那会让一个已经打开的开关自己弹回去。
    ...(typeof raw.kernel === 'boolean' ? { kernel: raw.kernel } : {}),
    // prestart 与 kernel 同一形态：缺失时保留现值（默认 true），不能按
    // FALLBACKS 兜底，否则一次不带该字段的状态回读会把开关显示成关。
    ...(typeof raw.prestart === 'boolean' ? { prestart: raw.prestart } : {}),
  };
}

function applyStatus(raw) {
  const next = normalizeStatus(raw);
  if (!next) return null;
  Object.assign(autostartStore, next);
  return next;
}

const loadStatus = createStatusSource('autostart_status', applyStatus);

/// 读取自启状态。进入设置页时拉一次，不轮询——登录项不会自己变。
export function refreshAutostartStatus() {
  return loadStatus();
}

/// 打开 / 关闭「登录时自动启动」。
///
/// **乐观回写**：开关必须在点击那一帧就变色，等一个 Rust 往返再动会像
/// 「没点动」。写失败时回滚并说明——登录项是系统资源，写失败有真实原因
/// （权限、沙箱、只读主目录），吞掉它等于骗用户。
export function setAutostartEnabled(enabled) {
  return withLoading(SET_KEY, async () => {
    const previous = { ...autostartStore };
    autostartStore.enabled = enabled;
    try {
      const status = await invoke('autostart_set', { enabled });
      if (!applyStatus(status)) autostartStore.enabled = enabled;
      if (enabled) {
        toastSuccess(
          '已设置开机自启动：下次开机后 dsh-xlink 会在后台运行，点菜单栏 / 托盘图标打开'
        );
      } else {
        toastSuccess('已关闭开机自启动');
      }
      return true;
    } catch (e) {
      Object.assign(autostartStore, previous);
      toastActionError(
        '设置开机自启动失败',
        e,
        '请检查系统是否允许本应用修改登录项（macOS：系统设置 → 隐私与安全性；Windows：登录项权限）',
        8000
      );
      return false;
    }
  });
}

/// 打开 / 关闭「登录时启动工作台」。
///
/// 与上一条分开是刻意的：登录项归系统管，删应用时会一并消失；这个开关
/// 只是壳自己的一条设置。合成一个开关会逼用户在「开机不启动」和
/// 「开机必须起内核」之间二选一，而这两件事的代价完全不同。
export function setAutostartKernel(enabled) {
  return withLoading(SET_KERNEL_KEY, async () => {
    const previous = autostartStore.kernel;
    autostartStore.kernel = enabled;
    try {
      const result = await invoke('autostart_set_kernel', { enabled });
      autostartStore.kernel = boolOr(result, enabled);
      return true;
    } catch (e) {
      autostartStore.kernel = previous;
      toastActionError('保存设置失败', e, '开关已还原，可稍后重试', 6000);
      return false;
    }
  });
}

/// 打开 / 关闭「启动时后台预启动工作台」。
///
/// 默认开、用于把内核冷启动移到后台；登录项拉起不参与（那是「开机启动工作
/// 台」开关）。与上一条一样是壳自己的设置、命令返回生效值，写失败回滚。
export function setPrestartKernel(enabled) {
  return withLoading(SET_PRESTART_KEY, async () => {
    const previous = autostartStore.prestart;
    autostartStore.prestart = enabled;
    try {
      const result = await invoke('prestart_set_kernel', { enabled });
      autostartStore.prestart = boolOr(result, enabled);
      return true;
    } catch (e) {
      autostartStore.prestart = previous;
      toastActionError('保存设置失败', e, '开关已还原，可稍后重试', 6000);
      return false;
    }
  });
}
