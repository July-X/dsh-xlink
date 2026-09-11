// 轻提示与确认框：统一走 Element Plus 的 ElMessage / ElMessageBox。
// WKWebView 没有原生 confirm()，ElMessageBox 是页内实现，天然可用。
import { ElMessage } from 'element-plus/es/components/message/index.mjs';
import { ElMessageBox } from 'element-plus/es/components/message-box/index.mjs';

export function toast(message, ms = 3200, type = 'info') {
  ElMessage({ message, duration: ms, type, grouping: true });
}

export function toastSuccess(message, ms = 3200) {
  toast(message, ms, 'success');
}

export function toastError(message, ms = 5000) {
  toast(message, ms, 'error');
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

// Promise 化确认框：用户点「确认」resolve(true)，取消 / 关闭 resolve(false)。
export function confirmDialog(title, text, okLabel, cancelLabel = '取消') {
  return ElMessageBox.confirm(text, title, {
    confirmButtonText: okLabel || '确认',
    cancelButtonText: cancelLabel || '取消',
    type: 'warning',
    distinguishCancelAndClose: true,
  })
    .then(() => true)
    .catch(() => false);
}
