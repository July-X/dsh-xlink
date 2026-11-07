import { reactive } from 'vue';
import { invoke } from '../shell/bridge.js';
import { withLoading } from '../shell/loading.js';
import { mcode } from './mcode.js';

export const mcodeMemory = reactive({ status: null, memories: [], target: null, loading: false, active: '', error: '' });

let sequence = 0;

export function resetMcodeMemory() {
  sequence += 1;
  mcodeMemory.status = null;
  mcodeMemory.memories = [];
  mcodeMemory.target = null;
  mcodeMemory.loading = false;
  mcodeMemory.active = '';
  mcodeMemory.error = '';
}

function selectedInstance() {
  const instanceId = mcode.view?.instance_id;
  const profile = mcode.view?.memory_profile;
  if (!instanceId || !profile) throw new Error('请先配置 MiniMax 记忆空间');
  return { instanceId, profile };
}

function stillCurrent(request, target) {
  if (request !== sequence) return false;
  if (mcode.view?.instance_id === target.instanceId && mcode.view?.memory_profile === target.profile) return true;
  mcodeMemory.status = null;
  mcodeMemory.memories = [];
  mcodeMemory.target = null;
  mcodeMemory.error = 'MiniMax 实例已改变，请刷新页面。';
  return false;
}

async function load(request, target) {
  const status = await invoke('mcode_memory_status', target);
  if (!stillCurrent(request, target)) return;
  const view = await invoke('mcode_memory_list', {
    ...target,
    filter: { include_disabled: true, include_candidates: false },
  });
  if (!stillCurrent(request, target)) return;
  mcodeMemory.status = view.status || status;
  mcodeMemory.memories = view.memories;
  mcodeMemory.target = target;
  mcodeMemory.error = '';
}

export function refreshMcodeMemory() {
  if (mcodeMemory.loading) return Promise.resolve();
  const request = ++sequence;
  return withLoading(`refreshMcodeMemory:${request}`, async () => {
    mcodeMemory.loading = true;
    mcodeMemory.active = 'refresh';
    try {
      await load(request, selectedInstance());
    } catch (error) {
      if (request === sequence) {
        mcodeMemory.status = null;
        mcodeMemory.memories = [];
        mcodeMemory.target = null;
        mcodeMemory.error = String(error);
      }
    } finally {
      if (request === sequence) {
        mcodeMemory.loading = false;
        mcodeMemory.active = '';
      }
    }
  });
}

function mutate(key, execute) {
  if (mcodeMemory.loading) return Promise.resolve(false);
  const request = ++sequence;
  return withLoading(`${key}:${request}`, async () => {
    mcodeMemory.loading = true;
    mcodeMemory.active = key;
    let saved = false;
    let target;
    try {
      target = mcodeMemory.target;
      if (!target || !stillCurrent(request, target)) throw new Error('记忆空间已改变，请刷新后重试');
      await execute(target);
      saved = true;
      if (request === sequence) await load(request, target);
    } catch (error) {
      if (request === sequence) {
        if (saved) {
          mcodeMemory.status = null;
          mcodeMemory.memories = [];
          mcodeMemory.target = null;
        }
        mcodeMemory.error = saved ? `操作已保存，但刷新失败：${String(error)}` : String(error);
      }
    } finally {
      if (request === sequence) {
        mcodeMemory.loading = false;
        mcodeMemory.active = '';
      }
    }
    return saved && request === sequence && mcode.view?.instance_id === target?.instanceId && mcode.view?.memory_profile === target?.profile;
  });
}

export function setMcodeMemoryEnabled(enabled) {
  return mutate('mcodeMemoryEnabled', target => invoke('mcode_memory_set_enabled', { ...target, useApproved: enabled }));
}

export function saveMcodeMemory(title, body, record = null) {
  const input = record ? {
    kind: 'memory', id: record.id, expected_revision: record.revision,
    scope: record.scope, workspace_path: record.workspace_path,
    keywords: record.keywords, pinned: record.pinned, verification: record.verification,
    title, body,
  } : { kind: 'memory', scope: 'general', title, body };
  return mutate('mcodeMemorySave', target => invoke('mcode_memory_save', {
    ...target, input,
  }));
}

export function setMcodeMemoryState(id, state) {
  return mutate(`mcodeMemoryState:${id}`, target => invoke('mcode_memory_set_state', { ...target, id, state }));
}

export function deleteMcodeMemory(id) {
  return mutate(`mcodeMemoryDelete:${id}`, target => invoke('mcode_memory_delete', { ...target, id }));
}
