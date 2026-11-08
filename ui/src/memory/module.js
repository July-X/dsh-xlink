// 通用记忆模块独立授权；原有实例开关和逐条共享授权不在这里修改。
import { reactive, computed } from 'vue';
import { invoke } from '../shell/bridge.js';
import { withLoading } from '../shell/loading.js';

export const memoryModule = reactive({
  status: null, records: [], error: '', busy: false,
  confirmation: null, listVisible: false, detailKey: '',
});
let sequence = 0;

export function moduleRecordKey(item) {
  return JSON.stringify([item.source_namespace, item.record.id]);
}
export const moduleDetail = computed(() =>
  memoryModule.records.find(item => moduleRecordKey(item) === memoryModule.detailKey) || null);

export function resetMemoryModule() {
  sequence += 1;
  Object.assign(memoryModule, { status: null, records: [], error: '', busy: false,
    confirmation: null, listVisible: false, detailKey: '' });
}

async function load(request) {
  const status = await invoke('memory_module_status');
  if (request !== sequence) return;
  memoryModule.status = status;
  // 来源或授权损坏时保留可撤销的状态，不让后续列表失败覆盖诊断。
  if (status.warning && !status.pair_key) {
    memoryModule.records = [];
    return;
  }
  const records = await invoke('memory_module_list');
  const latest = await invoke('memory_module_status');
  if (request !== sequence) return;
  memoryModule.status = latest;
  // 列表与状态分开读取，切换默认实例时不能把旧来源画成新来源。
  if (latest.pair_key !== status.pair_key) throw new Error('记忆来源已改变，请刷新后重试。');
  memoryModule.records = records;
}

export function refreshMemoryModule() {
  if (memoryModule.busy) return Promise.resolve();
  const request = ++sequence;
  return withLoading('memoryModuleRefresh', async () => {
    memoryModule.busy = true;
    memoryModule.error = '';
    memoryModule.records = [];
    try { await load(request); }
    catch (error) {
      if (request === sequence) {
        memoryModule.status = null;
        memoryModule.error = `读取记忆模块失败：${String(error)} 请刷新重试。`;
      }
    } finally { if (request === sequence) memoryModule.busy = false; }
  });
}

export function requestMemoryModuleEnabled(enabled) {
  const status = memoryModule.status;
  if (memoryModule.busy) return;
  // 撤销不依赖来源可读，也不能沿用尚未确认的启用请求。
  if (enabled === false) {
    memoryModule.confirmation = null;
    return setMemoryModuleEnabled(false, status?.pair_key || '');
  }
  if (enabled !== true || !status?.pair_key) return;
  // 固定本次确认的来源，后台刷新不得悄悄扩大确认范围。
  memoryModule.confirmation = { pair_key: status.pair_key, sources: status.sources.map(source => ({ ...source })) };
}
export function cancelMemoryModuleEnable() { memoryModule.confirmation = null; }
export function confirmMemoryModuleEnable() {
  const pending = memoryModule.confirmation;
  if (!pending || memoryModule.busy) return;
  memoryModule.confirmation = null;
  return setMemoryModuleEnabled(true, pending.pair_key);
}
async function setMemoryModuleEnabled(enabled, expectedPair) {
  if (memoryModule.busy) return;
  const request = ++sequence;
  return withLoading('memoryModuleEnable', async () => {
    memoryModule.busy = true;
    memoryModule.error = '';
    try {
      const status = await invoke('memory_module_set_enabled', { enabled, expectedPair });
      if (request !== sequence) return;
      memoryModule.status = status;
      memoryModule.records = [];
      await load(request);
    } catch (error) {
      if (request === sequence) {
        // 写入可能成功但回读失败，不把旧状态当成权威结果。
        memoryModule.status = null;
        memoryModule.records = [];
        memoryModule.error = `记忆模块状态未确认：${String(error)} 请刷新核对。`;
      }
    } finally { if (request === sequence) memoryModule.busy = false; }
  });
}
export function showModuleList() { memoryModule.listVisible = true; }
export function closeModuleList() { memoryModule.listVisible = false; }
export function openModuleDetail(item) { memoryModule.detailKey = moduleRecordKey(item); }
export function closeModuleDetail() { memoryModule.detailKey = ''; }
