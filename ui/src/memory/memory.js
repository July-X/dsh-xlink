// 本地记忆的共享状态与动作：列表 / 保存 / 停用 / 删除 / 批准候选 / 检索预览。
//
// 与 Rust 的 `memory/*.rs` 一一对应。**字段名沿用后端下发的 snake_case**：
// 前端的 IPC 判据（check-invariants 的 ipc-fields）就是为「Rust 侧改了序列化名、
// 前端还在读旧名」这类静默 undefined 设的——这里不留第二套命名。
import { reactive } from 'vue';
import { invoke } from '../shell/bridge.js';
import { toast, toastError, confirmDialog } from '../shell/notify.js';
import { withLoading } from '../shell/loading.js';

/// 项目路径是用户输入的（后端目前没有目录选择器）。记在本机，免得每次重打。
const PROJECT_KEY = 'dsh-xlink:memory-project';

function readProjectPath() {
  try {
    return localStorage.getItem(PROJECT_KEY) || '';
  } catch {
    return '';
  }
}

function emptyDraft() {
  return {
    id: '',
    expectedRevision: 0,
    scope: 'project',
    title: '',
    body: '',
    keywordsText: '',
    pinned: false,
  };
}

export const memoryStore = reactive({
  /// `memory_list` 的结果：`{ status, memories, candidates }`。
  view: null,
  /// 最近一次读到的状态摘要（`view.status` 的副本，便于面板单独刷新）。
  status: null,
  draft: emptyDraft(),
  projectPath: readProjectPath(),
  /// 内核已知的工作区（`memory_workspaces` 的结果，只读内核的注册表）。
  /// 下拉里选一个就同时拿到真实的 `workspace_id` 与路径——只按路径字符串
  /// 匹配的话，项目改名或换盘一次就静默失配。
  workspaces: [],
  /// 检索预览：`{ enabled, text, total_bytes, hits }`。
  preview: null,
  /// 最近一次导出的结果（`{ path, bytes, memories, candidates, generated_at }`）。
  lastExport: null,
  /// 文档读不出来时的原文（后端给的 warning 已经带下一步，前端不再改写）。
  failure: '',
});

/** 某个路径对应的内核工作区 id；不是已知工作区就回空串（后端按路径兜底）。 */
export function workspaceIdFor(path) {
  const match = memoryStore.workspaces.find((item) => item.path === path);
  return match ? match.id : '';
}

/**
 * 读内核的工作区列表。
 *
 * **失败保留上一次成功值、不弹窗**：这个列表是给下拉用的辅助信息，读不出来
 * 面板仍然能手填路径（后端那一侧也刻意回空列表而不是报错）。为它弹一个红字，
 * 用户会去修一个没坏的东西。
 */
export function loadWorkspaces() {
  return invoke('memory_workspaces')
    .then((list) => {
      memoryStore.workspaces = Array.isArray(list) ? list : [];
    })
    .catch(() => undefined);
}

export function rememberProjectPath(path) {
  memoryStore.projectPath = path;
  try {
    localStorage.setItem(PROJECT_KEY, path);
  } catch {
    // 偏好写不进去不影响功能，最多下次要重填。
  }
}

function applyView(view) {
  memoryStore.view = view;
  memoryStore.status = view.status;
  memoryStore.failure = view.status.warning || '';
}

export function refreshMemory({ silent = false } = {}) {
  // 工作区列表跟着一起取：它只影响下拉与 id 解析，失败不打扰用户。
  void loadWorkspaces();
  return withLoading('memoryList', () =>
    invoke('memory_list', { filter: { include_candidates: true, include_disabled: true } })
      .then((view) => {
        applyView(view);
        return view;
      })
      .catch((error) => {
        if (!silent) toastError('读取记忆失败：' + error, 6000);
        throw error;
      })
  ).catch(() => undefined);
}

export function editMemory(record) {
  memoryStore.draft = {
    id: record.id,
    expectedRevision: record.revision,
    scope: record.scope,
    title: record.title,
    body: record.body,
    keywordsText: (record.keywords || []).join('，'),
    pinned: !!record.pinned,
  };
  if (record.scope === 'project' && record.workspace_path) rememberProjectPath(record.workspace_path);
}

export function resetDraft() {
  memoryStore.draft = emptyDraft();
}

export function canSaveDraft() {
  const draft = memoryStore.draft;
  if (!draft.title.trim() && !draft.body.trim()) return false;
  if (draft.scope === 'project' && !memoryStore.projectPath.trim()) return false;
  return true;
}

function draftPayload() {
  const draft = memoryStore.draft;
  const keywords = draft.keywordsText
    .split(/[,，\s]+/)
    .map((item) => item.trim())
    .filter(Boolean);
  return {
    id: draft.id,
    scope: draft.scope,
    // 下拉里选过的工作区就带上真实 id（内核侧按 id 精确匹配），手填的路径留空、
    // 由后端的路径兜底匹配。
    workspace_id: draft.scope === 'project' ? workspaceIdFor(memoryStore.projectPath.trim()) : '',
    workspace_path: draft.scope === 'project' ? memoryStore.projectPath.trim() : '',
    title: draft.title.trim(),
    body: draft.body.trim(),
    keywords,
    pinned: !!draft.pinned,
    verification: '',
    expected_revision: draft.id ? draft.expectedRevision : null,
    source: null,
  };
}

export function saveMemory() {
  if (!canSaveDraft()) {
    toast('先写一句内容；项目记忆还需要填项目目录', 4000, 'warning');
    return Promise.resolve(false);
  }
  const payload = draftPayload();
  const editing = !!payload.id;
  return withLoading('memorySave', () =>
    invoke('memory_save', { input: payload })
      .then((record) => {
        // 后端把项目路径规范化成内核那种 realpath（符号链接、结尾斜杠），
        // 回填一次，免得面板显示的还是手填的那串、下次保存又原样送回去。
        if (record && record.scope === 'project' && record.workspace_path) {
          rememberProjectPath(record.workspace_path);
        }
        toast(editing ? '记忆已更新' : '记忆已保存', 3000, 'success');
        resetDraft();
        return refreshMemory({ silent: true });
      })
      .catch((error) => {
        toastError('保存失败：' + error, 6000);
      })
  ).then(() => true);
}

export function setMemoryState(record, state) {
  const verb = state === 'disabled' ? '停用' : '启用';
  return withLoading('memoryState:' + record.id, () =>
    invoke('memory_set_state', { id: record.id, state })
      .then(() => {
        toast(`已${verb}这条记忆`, 2500, 'success');
        return refreshMemory({ silent: true });
      })
      .catch((error) => toastError(`${verb}失败：` + error, 6000))
  );
}

export function removeMemory(record) {
  const scope = record.scope === 'general' ? '通用偏好' : '项目记忆';
  const detail =
    `将删除「${record.title || '（无标题）'}」（${scope}）。删除后不再参与检索，并记下正文指纹防止自动重建；` +
    '已导出的文件与已经发给模型的旧请求不会被撤回——那些要另行处理会话历史。';
  return confirmDialog('删除这条记忆？', detail, '删除').then((ok) => {
    if (!ok) return false;
    return withLoading('memoryDelete:' + record.id, () =>
      invoke('memory_delete', { id: record.id })
        .then(() => {
          toast('已删除', 2500, 'success');
          if (memoryStore.draft.id === record.id) resetDraft();
          return refreshMemory({ silent: true });
        })
        .catch((error) => toastError('删除失败：' + error, 6000))
    );
  });
}

export function setMemoryEnabled(enabled) {
  return withLoading('memoryEnabled', () =>
    invoke('memory_set_enabled', { useApproved: enabled })
      .then((status) => {
        if (memoryStore.view) memoryStore.view.status = status;
        memoryStore.status = status;
        toast(enabled ? '已打开：新会话会带上相关记忆' : '已关闭：不再注入记忆', 3000, 'success');
      })
      .catch((error) => toastError('切换开关失败：' + error, 6000))
  );
}

export function decideCandidate(candidate, accept) {
  return withLoading('memoryCandidate:' + candidate.id, () =>
    invoke('memory_decide_candidate', { id: candidate.id, accept })
      .then(() => {
        toast(accept ? '已保存为记忆' : '已忽略这条候选', 2500, 'success');
        return refreshMemory({ silent: true });
      })
      .catch((error) => toastError((accept ? '保存' : '忽略') + '失败：' + error, 6000))
  );
}

/// 按当前编辑器里的标题 + 关键词试一次检索，把「会注入什么」摊开给用户看。
export function previewContext() {
  const draft = memoryStore.draft;
  const query = [draft.title, draft.keywordsText].filter(Boolean).join(' ').trim();
  return withLoading('memoryPreview', () =>
    invoke('memory_preview_context', {
      queryText: query,
      workspaceId: workspaceIdFor(memoryStore.projectPath.trim()),
      workspacePath: memoryStore.projectPath.trim(),
    })
      .then((preview) => {
        memoryStore.preview = preview;
      })
      .catch((error) => toastError('预览失败：' + error, 6000))
  );
}

/// 导出一次 Markdown 快照。
///
/// **一次性**：不参与运行时检索，手工改那份文件也不会反向入库——这句话同时写在
/// 导出文件自己的头部（用户拿到文件、过一阵子再打开时也看得到）。
export function exportMemory() {
  return withLoading('memoryExport', () =>
    invoke('memory_export')
      .then((result) => {
        memoryStore.lastExport = result;
        toast(`已导出 ${result.memories} 条记忆（${result.candidates} 条候选）到 ${result.path}`, 9000, 'success');
      })
      .catch((error) => toastError('导出失败：' + error, 7000))
  );
}

/// 在文件管理器里打开导出目录（导出文件在磁盘上，用户得找得到它）。
export function openExports() {
  return invoke('memory_open_exports').catch((error) => toastError('打开导出目录失败：' + error, 6000));
}
