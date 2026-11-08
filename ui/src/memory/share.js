import { reactive } from 'vue';
import { invoke } from '../shell/bridge.js';
import { confirmDialog, toastActionError } from '../shell/notify.js';
import { mcode } from '../kernel/mcode.js';

export const memoryShares = reactive({ items: [], target: null, loading: false, loaded: false, error: '', changing: false, all: [], allLoading: false, allError: '' });
let sequence = 0;
let allSequence = 0;

export function resetMemoryShares() {
  sequence += 1;
  allSequence += 1;
  memoryShares.items = [];
  memoryShares.target = null;
  memoryShares.loading = false;
  memoryShares.loaded = false;
  memoryShares.error = '';
  memoryShares.all = [];
  memoryShares.allLoading = false;
  memoryShares.allError = '';
}

export async function refreshMemoryShares() {
  const instanceId = mcode.view?.instance_id;
  const profile = mcode.view?.memory_profile;
  const request = ++sequence;
  memoryShares.items = [];
  memoryShares.target = null;
  memoryShares.loaded = false;
  memoryShares.error = '';
  memoryShares.loading = false;
  if (!instanceId || !profile) return;
  memoryShares.loading = true;
  try {
    const items = await invoke('list_mcode_memory_shares', { instanceId, profile });
    if (request === sequence && mcode.view?.instance_id === instanceId && mcode.view?.memory_profile === profile) {
      memoryShares.items = items;
      memoryShares.target = { instanceId, profile };
      memoryShares.loaded = true;
    }
  } catch (error) {
    if (request === sequence) memoryShares.error = String(error);
  } finally {
    if (request === sequence) memoryShares.loading = false;
  }
}

export function isShared(record, namespace) {
  const [family, sourceInstance, sourceProfile] = String(namespace || '').split('/');
  return family === 'dsh' && memoryShares.items.some(item => item.source_instance === sourceInstance &&
    item.source_profile === sourceProfile && item.record_id === record.id);
}

export function shareNeedsReview(record, namespace) {
  const [family, sourceInstance, sourceProfile] = String(namespace || '').split('/');
  return family === 'dsh' && memoryShares.items.some(item => item.source_instance === sourceInstance &&
    item.source_profile === sourceProfile && item.record_id === record.id && !item.scope_valid);
}

export async function setMemoryShare(record, namespace, shared) {
  if (memoryShares.changing) return;
  const [family, sourceInstance, sourceProfile] = String(namespace || '').split('/');
  const { instanceId, profile } = memoryShares.target || {};
  if (family !== 'dsh' || !sourceInstance || !sourceProfile || !instanceId || !profile) return;
  if (mcode.view?.instance_id !== instanceId || mcode.view?.memory_profile !== profile) return;
  memoryShares.changing = true;
  try {
    if (shared && !await confirmDialog('授权 MiniMax 使用这条记忆？',
      `「${record.title || '（无标题）'}」会在适用范围内参与 MiniMax 检索；记录仍保存在 DSH，可随时撤销。`, '授权')) return;
    if (memoryShares.target?.instanceId !== instanceId || memoryShares.target?.profile !== profile || mcode.view?.instance_id !== instanceId || mcode.view?.memory_profile !== profile) return;
    await invoke('set_mcode_memory_share', { instanceId, profile, sourceInstance, sourceProfile, recordId: record.id, revision: record.revision, shared });
    await refreshMemoryShares();
  } catch (error) {
    toastActionError('修改记忆共享授权失败', error);
  } finally {
    memoryShares.changing = false;
  }
}

export async function revokeMemoryShare(item) {
  if (memoryShares.changing) return;
  const { instanceId, profile } = memoryShares.target || {};
  if (!instanceId || !profile) return;
  if (mcode.view?.instance_id !== instanceId || mcode.view?.memory_profile !== profile) return;
  memoryShares.changing = true;
  try {
    await invoke('revoke_mcode_memory_share', { instanceId, profile, sourceInstance: item.source_instance,
      sourceProfile: item.source_profile, recordId: item.record_id });
    await refreshMemoryShares();
  } catch (error) {
    toastActionError('撤销记忆共享授权失败', error);
  } finally {
    memoryShares.changing = false;
  }
}

export async function loadAllMemoryShares() {
  const request = ++allSequence;
  memoryShares.allLoading = true;
  memoryShares.allError = '';
  try {
    const items = await invoke('list_all_memory_shares');
    if (request === allSequence) memoryShares.all = items;
  } catch (error) {
    if (request === allSequence) memoryShares.allError = String(error);
  } finally {
    if (request === allSequence) memoryShares.allLoading = false;
  }
}

export async function revokeAnyMemoryShare(item) {
  if (memoryShares.changing) return;
  memoryShares.changing = true;
  try {
    await invoke('revoke_memory_share', { targetInstance: item.target_instance, targetProfile: item.target_profile,
      sourceInstance: item.source_instance, sourceProfile: item.source_profile, recordId: item.record_id });
    await Promise.all([loadAllMemoryShares(), refreshMemoryShares()]);
  } catch (error) {
    toastActionError('撤销记忆共享授权失败', error);
  } finally {
    memoryShares.changing = false;
  }
}

// —— 接收方使用开关 ——
// 每个内核作为接收方还能不能用记忆。与模块授权、逐条授权是**三件事**：这里只写
// memory_set_target_use，不碰任何授权；关掉也不删除记录（后端保证另一个目标照常）。
export const memoryTargetUse = reactive({ views: [], changing: '', error: '' });
let targetUseSequence = 0;

export function resetMemoryTargetUse() {
  targetUseSequence += 1;
  Object.assign(memoryTargetUse, { views: [], changing: '', error: '' });
}

export function targetUseView(target) {
  return memoryTargetUse.views.find(view => view.target === target) || null;
}

// 状态词只读后端给的三种事实：显式选择、跟随旧设置、状态不可读。没选过时不能
// 按 effective 猜成开或关——那只是旧设置现在的效果。
export function targetUseText(view) {
  if (!view) return '读取中';
  if (view.warning) return '状态未知';
  if (view.follows_legacy) return '跟随原设置';
  return view.preference ? '已开启' : '已关闭';
}

export async function refreshMemoryTargetUse() {
  const request = ++targetUseSequence;
  memoryTargetUse.error = '';
  try {
    const views = await invoke('memory_target_use_status');
    if (request !== targetUseSequence) return;
    memoryTargetUse.views = views;
  } catch (error) {
    if (request === targetUseSequence) {
      memoryTargetUse.views = [];
      memoryTargetUse.error = `读取记忆使用开关失败：${String(error)} 请刷新重试。`;
    }
  }
}

export async function setMemoryTargetUse(target, enabled) {
  if (memoryTargetUse.changing) return false;
  const previous = memoryTargetUse.views;
  const request = ++targetUseSequence;
  memoryTargetUse.changing = target;
  memoryTargetUse.error = '';
  // 开关是受控的（model-value 读这里）：先按点击回显，失败再原样还原——否则
  // 写失败后开关会停在用户点的那一侧，看起来像已经生效。
  memoryTargetUse.views = previous.map(view => view.target === target
    ? { ...view, preference: enabled, effective: enabled, follows_legacy: false }
    : view);
  try {
    const views = await invoke('memory_set_target_use', { target, enabled });
    if (request !== targetUseSequence) return false;
    memoryTargetUse.views = views;
    return true;
  } catch (error) {
    if (request === targetUseSequence) {
      memoryTargetUse.views = previous;
      memoryTargetUse.error = `保存记忆使用开关失败，状态需要重新读取：${String(error)} 请刷新后重试。`;
    }
    return false;
  } finally {
    memoryTargetUse.changing = '';
  }
}
