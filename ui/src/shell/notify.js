// 轻提示与确认框：统一走 Element Plus 的 ElMessage / ElMessageBox。
// WKWebView 没有原生 confirm()，ElMessageBox 是页内实现，天然可用。
import { h, ref } from 'vue';
import { ElMessage } from 'element-plus/es/components/message/index.mjs';
import { ElCheckbox } from 'element-plus/es/components/checkbox/index.mjs';
import { ElMessageBox } from 'element-plus/es/components/message-box/index.mjs';
import { openExternal } from './bridge.js';

/**
 * 进度浮层的 z-index，**从 CSS 阶梯读**而不是在这里写死一个 3000。
 *
 * 之前这里是字面量，`.progress-overlay` 在 theme.css 里也是字面量，两份各改
 * 各的——改了一边另一边就静默失配，而症状是「提示被浮层盖住」或「浮层被提示
 * 盖住」，都不带任何报错。阶梯的唯一出处是 `diagnostics/diagnostics.css` 的
 * `:root`（那里有整条顺序的说明），这里只读它。
 *
 * 读不到时（无 DOM 的单测环境）回落到同样的字面量：宁可与 CSS 漂一次，
 * 也不能让整个模块 import 就炸。
 */
const PROGRESS_OVERLAY_Z_INDEX = (() => {
  const fallback = 3000;
  try {
    const raw = getComputedStyle(document.documentElement)
      .getPropertyValue('--z-progress')
      .trim();
    const value = Number.parseInt(raw, 10);
    return Number.isFinite(value) && value > 0 ? value : fallback;
  } catch {
    return fallback;
  }
})();

// 提示与确认框必须**高于**浮层：Element Plus 的默认基线是 2000 + 自增计数，
// 恒低于浮层，于是长任务进行中弹的确认框（例如托盘「退出」的二次确认、
// 补丁的「清除记录」确认）会被浮层遮住且点不到，而任务未失败时浮层没有关闭
// 按钮——用户看到的是"点了没反应"（P2-11）。
const NOTIFY_Z_INDEX = PROGRESS_OVERLAY_Z_INDEX + 1000;

export function toast(message, ms = 3200, type = 'info') {
  ElMessage({ message, duration: ms, type, grouping: true, zIndex: NOTIFY_Z_INDEX });
}

export function toastSuccess(message, ms = 3200) {
  toast(message, ms, 'success');
}

export function toastError(message, ms = 5000) {
  toast(message, ms, 'error');
}

/// 带勾选项的长提示（2026-10-05，「收进后台」toast 的「不再提示」专用）：
/// 文案后跟一个 checkbox，勾选**即时**回调 `onCheck`（调用方自行持久化）
/// 并立刻收起 toast——勾了还挂着，等于邀请用户再读一遍不想看的内容。
/// 时长给到 8s：比普通 toast 长，用户得有时间注意到并勾选。
export function toastWithCheckbox(message, checkboxLabel, onCheck, ms = 8000) {
  const checked = ref(false);
  const instance = ElMessage({
    message: h('span', { class: 'toast-with-checkbox' }, [
      h('span', null, message),
      h(
        ElCheckbox,
        {
          modelValue: checked.value,
          'onUpdate:modelValue': (value) => {
            checked.value = value;
            if (value) {
              onCheck();
              instance.close();
            }
          },
        },
        { default: () => checkboxLabel },
      ),
    ]),
    duration: ms,
    zIndex: NOTIFY_Z_INDEX,
  });
}

/// 把后端/桥接错误统一成「发生了什么 + 下一步 + 去哪里看日志」的提示。
///
/// 项目约定是错误信息必须包含可操作的下一步与日志路径，但此前每个调用点都
/// 自己拼 `'XX失败：' + e`：reqwest / ureq 的英文原始错误直出，用户既不知道
/// 该做什么，也不知道去哪看详情（P2-36）。后端抛出的中文信息通常已经带了
/// 下一步（例如「…请重新安装该内核版本」「…详情见日志：<路径>」），因此优先
/// 原样展示；只有当它看起来是英文原始错误时才补一句通用指引。
export function formatActionError(prefix, error, nextStep) {
  const raw = error && error.message ? error.message : String(error ?? '');
  const detail = raw.trim() || '未知错误';
  // 后端的中文文案已经自带指引，直接用，避免叠加两层"下一步"。
  const hasGuidance = /日志|请|重试|设置|重新|检查/.test(detail);
  if (hasGuidance) {
    return `${prefix}：${detail}`;
  }
  const hint = nextStep || '请重试；若反复失败，打开「查看日志」把最近的日志发给维护者';
  return `${prefix}：${detail}。${hint}`;
}

/// 统一的动作失败提示：`prefix` 说明发生了什么，`nextStep` 给出出路。
export function toastActionError(prefix, error, nextStep, ms = 6000) {
  toastError(formatActionError(prefix, error, nextStep), ms);
}

/// 用系统浏览器打开外部链接，失败时给出可操作提示。
///
/// `bridge.js` 的 `openExternal` 按 P2-39 的约定把错误抛给调用方，但四个按钮
/// （插件仓库、插件详情、技能仓库）各自写一遍 catch 只会让其中某个漏掉——失败在
/// 这里统一收口：说明打不开的是什么，并给出手动复制的出路。
export function openExternalLink(url, label = '链接') {
  return openExternal(url).catch((error) => {
    toastActionError(
      `无法打开${label}`,
      error,
      '请复制地址到浏览器手动打开，或检查系统默认浏览器设置',
    );
    return undefined;
  });
}

// Promise 化确认框：用户点「确认」resolve(true)，取消 / 关闭 resolve(false)。
export function confirmDialog(title, text, okLabel, cancelLabel = '取消') {
  return ElMessageBox.confirm(text, title, {
    confirmButtonText: okLabel || '确认',
    cancelButtonText: cancelLabel || '取消',
    type: 'warning',
    distinguishCancelAndClose: true,
    // 与 toast 同理：确认框也必须浮在进度浮层之上（P2-11）。
    zIndex: NOTIFY_Z_INDEX,
  })
    .then(() => true)
    .catch(() => false);
}
