// 试点 IPC 契约的测试夹具（2026-11 OPT-12）。
//
// 契约要求响应是真实 DTO 形状；单元测试里的最小 mock 长期只写关注字段，
// 直接喂给 ipcCall 会被契约拒收。这里给出完整的最小合法响应，测试只覆盖
// 自己关心的字段，避免每处手抄 14 个字段。

function merge(base, patch) {
  if (!patch || typeof patch !== 'object' || Array.isArray(patch)) return patch === undefined ? base : patch;
  const out = Array.isArray(base) ? [...base] : { ...base };
  for (const [key, value] of Object.entries(patch)) {
    out[key] =
      value && typeof value === 'object' && !Array.isArray(value) && out[key] && typeof out[key] === 'object'
        ? merge(out[key], value)
        : value;
  }
  return out;
}

export function statusView(overrides = {}) {
  const view = merge(
    {
      shell_version: '0.0.0-test',
      dev_build: false,
      shell_mode: 'release',
      kernel: {
        installed: [],
        active: null,
        active_installed: false,
        running: false,
        port: 3090,
        data_dir: '~/.dsh-xlink/dsh/desktop',
        xlink_home: '~/.dsh-xlink',
      },
      node: { path: '/usr/bin/node', version: 'v24.0.0', ok: true, reason: '' },
      settings: { port: 3090, profile: 'web' },
      quarantined: [],
      last_incident: null,
      official_chat_open: false,
    },
    overrides
  );
  // 旧用例把内核版本写成字符串（只关心数量）；真实 DTO 里是对象，夹具顺手归一。
  if (Array.isArray(view.kernel?.installed)) {
    view.kernel.installed = view.kernel.installed.map((item) =>
      typeof item === 'string'
        ? { version: item, active: false, size_bytes: 0, shared_storage: false }
        : item
    );
  }
  return view;
}

export function usageView(overrides = {}) {
  return merge(
    {
      retention_days: 90,
      last_scanned_at_ms: 0,
      tracked_files: 0,
      total_tokens: 0,
      total_requests: 0,
      today_tokens: 0,
      today_requests: 0,
      today_models: [],
      active_days: 0,
      top_model: null,
      models: [],
      days: [],
      external_sources: { codex: false, minimax: false },
      external: [],
    },
    overrides
  );
}

export function memoryListView(overrides = {}) {
  return merge(
    {
      status: {
        namespace: 'dsh/desktop/web',
        dir: '~/.dsh-xlink/dsh/desktop/memory',
        file: 'memory.json',
        schema_version: 6,
        generation: 0,
        use_approved: false,
        auto_extract: false,
        accepted: 0,
        disabled: 0,
        pending_candidates: 0,
        max_hits: 6,
        total_bytes: 0,
        warning: null,
        effect_pending: false,
      },
      memories: [],
      candidates: [],
      handoffs: [],
      experiences: [],
    },
    overrides
  );
}

export function startReport(overrides = {}) {
  return merge({ port: 0, running: false, safe_mode: false, incident: null }, overrides);
}
