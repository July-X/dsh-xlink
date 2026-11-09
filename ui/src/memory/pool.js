// 共享池只有每核一个使用开关；旧许可不自动升级为跨来源许可。
import { reactive } from 'vue';
import { invoke } from '../shell/bridge.js';
import { builtinStore, builtinById, loadBuiltinStatus, toggleBuiltin } from '../plugins/builtin.js';

export const memoryPool = reactive({ status: null, targets: [], busy: false, changing: '', error: '', confirmation: null, dshConnection: 'unknown', connectionNote: '' });
let sequence = 0;

export function resetMemoryPool() {
  sequence += 1;
  Object.assign(memoryPool, { status: null, targets: [], busy: false, changing: '', error: '', confirmation: null, dshConnection: 'unknown', connectionNote: '' });
}

async function load(request) {
  const status = await invoke('memory_module_status');
  const targets = await invoke('memory_target_use_status');
  await loadBuiltinStatus();
  const latest = await invoke('memory_module_status');
  if (request !== sequence) return;
  if (latest.pair_key !== status.pair_key) throw new Error('记忆来源已改变，请刷新后重试。');
  memoryPool.status = latest;
  memoryPool.targets = targets;
  const plugin = builtinById('memory');
  memoryPool.dshConnection = builtinStore.error || !plugin || plugin.stateError ? 'unknown'
    : !plugin.wired || !plugin.requestedEnabled ? 'missing' : plugin.loadState === 'incompatible' ? 'restart' : 'ready';
}

export async function refreshMemoryPool() {
  if (memoryPool.changing) return;
  const request = ++sequence;
  memoryPool.busy = true;
  memoryPool.error = '';
  try { await load(request); }
  catch (error) {
    if (request === sequence) {
      memoryPool.status = null;
      memoryPool.targets = [];
      memoryPool.error = `读取记忆模块失败：${String(error)} 请刷新重试。`;
    }
  } finally { if (request === sequence) memoryPool.busy = false; }
}

export function poolTarget(target) { return memoryPool.targets.find(item => item.target === target); }

export function requestPoolTarget(target, enabled) {
  if (memoryPool.busy || memoryPool.changing) return;
  memoryPool.confirmation = null;
  if (!enabled) return save(target, false, '');
  const status = memoryPool.status;
  const view = poolTarget(target);
  if (!status?.pair_key || !view) return;
  if (!view.needs_consent) return save(target, true, status.pair_key);
  // 确认期间固定来源；后台刷新或实例切换不会扩大用户看到的范围。
  memoryPool.confirmation = { target, pair_key: status.pair_key, sources: status.sources.map(item => ({ ...item })) };
}

export function cancelPoolTarget() { memoryPool.confirmation = null; }
export function confirmPoolTarget() {
  const pending = memoryPool.confirmation;
  if (!pending || memoryPool.busy || memoryPool.changing) return;
  memoryPool.confirmation = null;
  return save(pending.target, true, pending.pair_key);
}

async function save(target, enabled, expectedPair) {
  const request = ++sequence;
  memoryPool.changing = target;
  memoryPool.error = '';
  try {
    if (enabled && target === 'dsh') {
      // 接入插件和池许可是一项用户操作，不能让用户再到插件页找第二个开关。
      const current = await invoke('memory_module_status');
      if (request !== sequence) return;
      if (current.pair_key !== expectedPair) throw new Error('记忆来源已改变，请重新确认。');
      await loadBuiltinStatus();
      if (request !== sequence) return;
      const plugin = builtinById('memory');
      if (builtinStore.error || !plugin || plugin.stateError) throw new Error(builtinStore.error || plugin?.stateError || '记忆插件状态不可读，请刷新重试。');
      if (!plugin.wired || !plugin.requestedEnabled) {
        const result = await toggleBuiltin('memory', true);
        if (request !== sequence) return;
        if (!result?.ok || !result.view?.wired) throw new Error(result?.error || '记忆插件尚未接入，请停止 DSH 工作台后重试。');
        memoryPool.connectionNote = 'DSH 记忆插件已接入，下次启动工作台生效；当前会话需要重新打开。';
      }
    }
    if (request !== sequence) return;
    await invoke('memory_set_pool_target', { target, enabled, expectedPair });
    if (request !== sequence) return;
    await load(request);
  } catch (error) {
    if (request === sequence) {
      // 写入或回读失败都不能继续展示旧的「已启用」。
      memoryPool.status = null;
      memoryPool.targets = [];
      memoryPool.error = `记忆开关状态未确认：${String(error)} 请刷新核对。${memoryPool.connectionNote ? '插件接入已完成，但池开关仍需核对。' : ''}`;
    }
  } finally { if (request === sequence) memoryPool.changing = ''; }
}

export const poolReview = reactive({ candidates: [], legacyRecords: [], loading: false, loaded: false, error: '', changing: '' });
let reviewSequence = 0;
export function reviewKey(item) { return JSON.stringify([item.source_namespace, (item.candidate || item.record).id]); }
export function resetPoolReview() {
  reviewSequence += 1;
  Object.assign(poolReview, { candidates: [], legacyRecords: [], loading: false, loaded: false, error: '', changing: '' });
}
export async function refreshPoolReview() {
  if (poolReview.changing) return;
  const request = ++reviewSequence;
  poolReview.loading = true;
  poolReview.error = '';
  try {
    const result = await invoke('memory_pool_review_list');
    if (request !== reviewSequence) return;
    poolReview.candidates = result.candidates;
    poolReview.legacyRecords = result.legacy_records;
    poolReview.loaded = true;
  } catch (error) {
    if (request === reviewSequence) {
      poolReview.loaded = false;
      poolReview.error = `读取待审核记忆失败：${String(error)}`;
    }
  } finally { if (request === reviewSequence) poolReview.loading = false; }
}
export async function decidePoolReview(item, accept) {
  if (poolReview.loading || poolReview.changing) return false;
  const request = ++reviewSequence;
  const record = item.candidate || item.record;
  poolReview.changing = reviewKey(item);
  poolReview.error = '';
  try {
    await invoke(item.candidate ? 'memory_pool_decide_candidate' : 'memory_pool_review_legacy', item.candidate
      ? { sourceNamespace: item.source_namespace, id: record.id, accept, scope: null }
      : { sourceNamespace: item.source_namespace, id: record.id, expectedRevision: record.revision, accept });
    if (request !== reviewSequence) return false;
    poolReview.changing = '';
    await refreshPoolReview();
    return true;
  } catch (error) {
    if (request === reviewSequence) poolReview.error = `审核未完成：${String(error)} 请刷新后重试。`;
    return false;
  } finally { if (request === reviewSequence) poolReview.changing = ''; }
}
