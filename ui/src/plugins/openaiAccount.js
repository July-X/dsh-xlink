// 主面板和工作台共用 Rust 账号服务；这里不读取、保存或返回令牌。
import { reactive } from 'vue';
import { invoke, listen } from '../shell/bridge.js';
import { isLoading, withLoading } from '../shell/loading.js';

export const accountStore = reactive({ status: null, error: '', readError: '' });
let revision = 0;
const ACTIONS = new Set(['openai_authorize_start', 'openai_authorize_cancel', 'openai_logout', 'openai_catalog_refresh']);

export function accountText() {
  const status = accountStore.status;
  if (!status) return accountStore.readError ? '账号状态不可用' : '正在读取账号状态…';
  if (status.phase === 'authorizing') return '登录进行中，请在系统浏览器完成授权';
  if (status.phase === 'reauth-required') return '需要重新登录';
  if (status.phase === 'authorized') return `已登录${status.email ? ' · ' + status.email : ''}`;
  return '未登录';
}

export async function refreshAccount() {
  return withLoading('openaiAccountRefresh', async () => {
    const ticket = ++revision;
    try {
      const status = await invoke('openai_account_status');
      if (ticket !== revision) return;
      accountStore.status = status;
      accountStore.readError = '';
    } catch (error) {
      if (ticket === revision) accountStore.readError = `读取账号状态失败：${error}；请重试或查看日志`;
    }
  });
}

export async function runAccountAction(command) {
  if (!ACTIONS.has(command) || isLoading('openaiAccountAction')) return;
  return withLoading('openaiAccountAction', async () => {
    const ticket = ++revision;
    accountStore.error = '';
    try {
      const status = await invoke(command);
      if (ticket === revision) accountStore.status = status;
    } catch (error) {
      if (ticket === revision) accountStore.error = `账号操作失败：${error}；请重试或查看日志`;
    }
  });
}

// 事件只用于唤醒读取，状态仍以 Rust 查询为准。轮询兜底覆盖工作台发起登录
// 的 authorizing 阶段（后端目前只在流程完成时广播）及错过的事件。
export function followAccount() {
  let disposed = false;
  let unlisten;
  const refresh = () => { if (!disposed && !isLoading('openaiAccountAction')) return refreshAccount(); };
  const timer = setInterval(refresh, 5000);
  listen('openai-account-changed', refresh).then((off) => {
    if (disposed) off?.();
    else unlisten = off;
  }).catch((error) => {
    if (!disposed) accountStore.error = `账号通知连接失败：${error}；已改为定期刷新`;
  });
  refresh();
  return () => { if (disposed) return; disposed = true; clearInterval(timer); unlisten?.(); ++revision; };
}
