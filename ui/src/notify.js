// 轻提示与确认框：统一走 Element Plus 的 ElMessage / ElMessageBox。
// WKWebView 没有原生 confirm()，ElMessageBox 是页内实现，天然可用。
import { ElMessage, ElMessageBox } from 'element-plus';

export function toast(message, ms = 3200, type = 'info') {
  ElMessage({ message, duration: ms, type, grouping: true });
}

export function toastSuccess(message, ms = 3200) {
  toast(message, ms, 'success');
}

export function toastError(message, ms = 5000) {
  toast(message, ms, 'error');
}

// Promise 化确认框：用户点「确认」resolve(true)，取消 / 关闭 resolve(false)。
export function confirmDialog(title, text, okLabel) {
  return ElMessageBox.confirm(text, title, {
    confirmButtonText: okLabel || '确认',
    cancelButtonText: '取消',
    type: 'warning',
    distinguishCancelAndClose: true,
  })
    .then(() => true)
    .catch(() => false);
}
