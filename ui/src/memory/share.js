import { reactive } from 'vue';
import { invoke } from '../shell/bridge.js';
import { confirmDialog, toastActionError } from '../shell/notify.js';
import { mcode } from '../kernel/mcode.js';
import { pageFamilies } from '../kernel/instance.js';

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
    if (pageFamilies.memory !== 'dsh' || mcode.view?.instance_id !== instanceId || mcode.view?.memory_profile !== profile) return;
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
