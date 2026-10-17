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
    /// 验证范围。面板默认「已验证」——用户自己写下的是他自己确认过的；
    /// 「已过期」是给「这条事实可能过时了」用的（检索里排后面，但不会消失）。
    verification: 'verified',
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
  /// 「检索预览」拿什么当查询。**必须与草稿分开存**：编辑器是 modal 弹窗，弹窗开着时
  /// 面板上那枚「试一次」被遮罩挡着点不到，而弹窗一关 `closeMemoryEditor` 就把草稿清了
  /// ——挂在草稿上的话，用户能点到它的时候查询必然是空串，而后端对空查询是「所有条目
  /// 0 分」（`query.rs` 的 `score`），预览会稳定报出一批与输入无关的命中。
  previewQuery: '',
  /// 最近一次导出的结果（`{ path, bytes, memories, candidates, generated_at }`）。
  lastExport: null,
  /// 列表的范围筛选：`all` / `project` / `general`（§2 的「本项目」与「此实例的通用偏好」两个视图）。
  listScope: 'all',
  /// 提取任务表：`{ summary: {pending,running,done,failed}, jobs: [...] }`（`memory_jobs`）。
  ///
  /// **这是「为什么这个会话一条记忆都没提取出来」的答案所在**（独立评审第 12 条）：
  /// 每条任务带着状态与失败原因（`stage：reason`）。没有它，用户只能手动打开 `jobs.json`。
  jobs: null,
  /// 最近几次「模型实际看到了哪些记忆」（`memory_recent_injections`，新的在前）。
  /// 与 `preview` 的区别：那是**模拟**（拿你填的查询算一遍），这是**实况**。
  recentInjections: [],
  /// 编辑器弹窗开着没有。**弹窗可见性放 store，不放组件里的 `ref`**——与
  /// `plugins/precheck.js` 的 `precheckVisible`、`diagnostics` 的 `restoreVisible`
  /// 同一个约定：面板只画那枚触发按钮，动作与状态都在这里，判据不必挂载 Vue。
  editorVisible: false,
});

/** 某个路径对应的内核工作区 id；不是已知工作区就回空串（后端按路径兜底）。 */
export function workspaceIdFor(path) {
  const match = memoryStore.workspaces.find((item) => item.path === path);
  return match ? match.id : '';
}

/**
 * 注入插件**确定**没启用时才回 true。
 *
 * `view` 为 null 表示状态还没读回来（或这次读失败了）——那时**不能**说「没启用」：
 * 面板会在打开的一瞬间给已启用的用户挂一条假警报（「保存的记忆不会进入任何会话」），
 * 读失败时更会永久挂着，而用户跑到插件页看到的是「已启用」。不知道就不说。
 *
 * @param {object|null} view - `builtin_status` 里 id 为 `memory` 的那一项。
 * @returns {boolean}
 */
export function injectionDisabled(view) {
  if (!view) return false;
  // **判接线事实，不判启用意图**（记忆内核那次评审第 3 条）：内核读的是 `cordis.patch.yml`
  // 的接线行（`wired`），而 `requestedEnabled` 只是壳里那份 `state.json`。只看后者两个方向都会错：
  // · 启用是「先写接线行、后写 state.json」，两步之间失败 → `wired=false`，内核确实没加载它；
  // · 切内核版本后接线行还指着旧指纹（`loadState='incompatible'`）→ 内核照旧加载着它，而重接线
  //   由下次启动工作台在拉起内核**之前**自动做（`kernel_adapter::start` → `runtime::refresh_enabled`），
  //   此刻内核也没在跑——按它报「没启用」是一条假警报。
  if (view.stateError) return false; // 状态文件异常：不知道就不说
  // `wired` 缺失（旧壳/字段没下发）时**也不说**——「不知道」不能当成「没接线」，
  // 那正是这条判据第一版的老毛病（挂一句假警报，让用户去修一个没坏的东西）。
  if (typeof view.wired !== 'boolean') return false;
  return !view.wired;
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

/**
 * 读「最近注入」。
 *
 * **失败保留上一次成功值、不弹窗**（与工作区列表同一条理由）：它是一条辅助信号，
 * 为它弹一个红字会让用户去修一个没坏的东西。
 */
export function loadRecentInjections() {
  return invoke('memory_recent_injections')
    .then((list) => {
      memoryStore.recentInjections = Array.isArray(list) ? list : [];
    })
    .catch(() => undefined);
}

/**
 * 列表的范围筛选。**在客户端做**：列表本来就整份读回来了（`memory_list` 一次给全），
 * 再为切视图跑一趟 IPC 只会让状态与计数分叉。
 *
 * 默认 `all` 而不是「当前项目」：面板**不假定**「当前项目」是哪一个——手填路径为空时按
 * 项目过滤会给出一个空列表，而用户看到的是「我的记忆没了」。项目与通用偏好各是一个视图，
 * 想看哪个点哪个。
 */
export function visibleMemories(view, scope) {
  const rows = (view && view.memories) || [];
  if (scope === 'project') return rows.filter((row) => row.scope !== 'general');
  if (scope === 'general') return rows.filter((row) => row.scope === 'general');
  return rows;
}

/// 时间戳的短格式（列表里用）。认不出的值原样返回——**不要显示「Invalid Date」**。
export function shortTime(value) {
  if (!value) return '未知时间';
  const parsed = new Date(value);
  if (Number.isNaN(parsed.getTime())) return value;
  const pad = (n) => String(n).padStart(2, '0');
  return `${parsed.getFullYear()}-${pad(parsed.getMonth() + 1)}-${pad(parsed.getDate())} ${pad(parsed.getHours())}:${pad(parsed.getMinutes())}`;
}

/// 范围的中文标签：通用偏好带一句「所有项目」，项目记忆带上路径（路径是它的身份）。
export function scopeText(record) {
  if (!record || record.scope === 'general') return '通用偏好（所有项目）';
  return record.workspace_path ? `本项目 · ${record.workspace_path}` : '本项目';
}

/// 来源种类的中文标签（§2：列表要显示来源）。
export function sourceText(kind) {
  if (kind === 'user-manual') return '面板手动';
  if (kind === 'session') return '会话提出';
  if (kind === 'extract') return '自动提取';
  return kind || '未知来源';
}

/// 一条记录的全部来源（一条可以不止一个：面板手动、会话里提出、自动提取）。
/// **两种形状都认**：新库是 `sources` 数组，读到旧形状的 `source` 时按单条算——
/// 面板不该因为后端形状没换过来就显示「未知来源」。
export function sourcesText(row) {
  const list = row?.sources?.length ? row.sources : row?.source ? [row.source] : [];
  return list.map((item) => sourceText(item.kind)).join(' + ') || '未知来源';
}

/// 验证范围的中文标签（§2：列表要显示验证状态）。**不是置信分数**，只三档。
export function verificationText(value) {
  if (value === 'verified') return '已验证';
  if (value === 'unverified') return '未验证';
  if (value === 'stale') return '已过期';
  return value || '未标注';
}

/**
 * 「最近注入」一行里那句人话。三种状态必须分得开：开关关着 / 一条没命中 / 注入了哪几条。
 *
 * 放在这里而不是组件里：它是**判据能直接跑**的那种逻辑，写在 `.vue` 的 `<script setup>`
 * 里就只能靠读源码文本断言（本仓栽过：helper 有测试、接线没有）。
 */
export function injectionText(note) {
  if (!note || !note.enabled) return '开关关着，这一回合没有注入';
  if (!note.hits || !note.hits.length) return '没有命中';
  return `注入了 ${note.hits.length} 条：${note.hits.join('、')}`;
}

export function setListScope(scope) {
  memoryStore.listScope = scope;
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
}

/// 开关改动的**生效语义**（设计 §9 / 清单 E6：**不许说「已生效」**）。
///
/// 事实是分层的，所以这句话是**条件式**的：
/// · 插件在**每一步**都会问一次 `/context`（`pre-step`），所以开关的改动**通常下一轮就生效**；
/// · 但注入次数有会话级预算（`MAX_INJECTIONS_PER_SESSION`），**预算用尽**之后它不再问了——
///   那时改动要等**新会话**才被看见。
/// 面板不知道这个会话还剩几次，所以**两个都写上**：与其给一句听起来干脆但可能不真的话，
/// 不如给一句用户能自己对照的。精确的「处理中」需要记住「设置改动的时间」与「插件最后一次
/// 调用的时间」，前者是 schema 字段（见清单 E6 与「schema 变更窗口」），在那之前保守措辞更诚实。
export function switchEffectText(status) {
  // **有了确切的判据就用确切的**（E6）：壳侧记着「插件最后一次调用」与「开关最后一次改动」
  // 两个时刻，`effect_pending` 就是「改过、而插件还没再来问」。
  if (status && status.effect_pending) {
    return '处理中：改动还没被运行中的工作台确认。插件每一步都会问一次（通常几秒内），但本会话注入次数用尽时要等新会话。';
  }
  if (status && !status.effect_pending) {
    return '已生效：插件已经按这个开关来过一次。';
  }
  return '还没读到状态：改动通常下一步检索就生效，本会话注入次数用尽时要等新会话。';
}

/// 提取任务卡片上那句话。**纯函数**，判据直接测它（面板只负责把它放上去）。
///
/// 四种状态分开数：`pending` 是「排了还没轮到」（含静默期里的），`running` 是「正在提取」，
/// `failed` 是「试到上限也没成」。**全零也要说话**——「一条任务都没有」本身就是答案：
/// 说明这一轮压根没排（回合没正常结束、没有新的人类消息、或者开关没开）。
export function jobsSummaryText(view) {
  const summary = view?.summary;
  if (!summary) return '还没读到提取任务的状态。';
  const { pending = 0, running = 0, done = 0, failed = 0 } = summary;
  if (!pending && !running && !done && !failed) {
    return '还没有提取任务。聊一轮之后这里会出现「排了、领了、成了还是没成」。';
  }
  return `待跑 ${pending} · 在跑 ${running} · 完成 ${done} · 失败 ${failed}`;
}

/// 一条任务在卡片上的那行字：状态 + 冻结游标 + 失败原因。
export function jobLine(job) {
  const state = job?.state ?? '未知';
  const cursor = job?.key?.cursor ?? 0;
  const error = job?.error ? `：${job.error}` : '';
  return `#${cursor} ${state}${error}`;
}

/// 读提取任务表。**失败不打扰用户**（与工作区列表、最近注入同一条纪律）：它是辅助显示。
export function loadJobs() {
  return invoke('memory_jobs')
    .then((view) => {
      memoryStore.jobs = view;
      return view;
    })
    .catch(() => undefined);
}

export function refreshMemory({ silent = false } = {}) {
  // 工作区列表与「最近注入」跟着一起取：都只影响辅助显示，失败不打扰用户。
  void loadWorkspaces();
  void loadRecentInjections();
  void loadJobs();
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

/// 「检索预览」的查询文本：标题 + 关键词，与用户填的表单同源。
function queryTextFor(record) {
  return [record.title, (record.keywords || []).join('，')].filter(Boolean).join(' ').trim();
}

export function editMemory(record) {
  memoryStore.previewQuery = queryTextFor(record);
  memoryStore.draft = {
    id: record.id,
    expectedRevision: record.revision,
    scope: record.scope,
    title: record.title,
    body: record.body,
    keywordsText: (record.keywords || []).join('，'),
    pinned: !!record.pinned,
    verification: record.verification || 'verified',
  };
  if (record.scope === 'project' && record.workspace_path) rememberProjectPath(record.workspace_path);
}

export function resetDraft() {
  memoryStore.draft = emptyDraft();
}

/**
 * 打开记忆编辑器：给一条记录就是**编辑**它，不给就是**新建**一条。
 *
 * 两种模式共用同一个弹窗（同一份字段与校验）——分成两个界面迟早一边少字段、
 * 一边少校验。新建时先清草稿：上一次取消留下的半截正文会跟着进来，而
 * 「检索预览」正是拿草稿当查询的。
 */
export function openMemoryEditor(record = null) {
  if (record) editMemory(record);
  else {
    resetDraft();
    memoryStore.previewQuery = '';
  }
  memoryStore.editorVisible = true;
}

/// 关掉编辑器并**丢弃草稿**（取消的语义）。「取消」按钮与 `MemoryEditor` 的 `beforeClose`
/// （✕，用户确认「放弃」之后）都走这里——**遮罩与 ESC 不关**，它们在弹窗的 prop 上就被拦住了。
/// 保留了 `previewQuery`：它记的是「上一次编辑过的查询」，关窗不该把它一起抹掉。
export function closeMemoryEditor() {
  memoryStore.editorVisible = false;
  resetDraft();
}

/// 正文字节上限。**与后端的 `store::MAX_BODY_BYTES` 逐字对齐**——改一边要两边一起改。
///
/// 面板原先写 `maxlength="4000"`，那数的是 UTF-16 码元，而后端闸的是字节：中文 4000 字
/// 约 12000 字节，用户按面板允许的长度写完，保存时才拿到「超过单条上限 8192 字节」
/// （记忆内核那次评审第 4 条）。现在面板按**字节**算并提前拦住。
export const MAX_BODY_BYTES = 8192;

/// 一段文本的 UTF-8 字节数（中文一个字 3 字节）。
export function byteLength(text) {
  return new TextEncoder().encode(text || '').length;
}

export function canSaveDraft() {
  const draft = memoryStore.draft;
  if (!draft.title.trim() && !draft.body.trim()) return false;
  if (draft.scope === 'project' && !memoryStore.projectPath.trim()) return false;
  // 超了就保存不了——**提前拦住**，而不是让用户写完再被后端拒。
  if (byteLength(draft.body) > MAX_BODY_BYTES) return false;
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
    verification: draft.verification || 'verified',
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
        // **只有成功了才关弹窗，而且要在报喜之前收摊**。失败那条路（下面的 catch）
        // 一个字都不动：关掉一个装着用户刚写完正文的弹窗，等于把那段文字从他手里拿走。
        // 顺序也有讲究——`toast` 万一抛（或哪天通知换实现），关窗与清草稿不该跟着一起丢。
        memoryStore.editorVisible = false;
        resetDraft();
        toast(editing ? '记忆已更新' : '记忆已保存', 3000, 'success');
        return refreshMemory({ silent: true });
      })
      .catch((error) => {
        toastError('保存失败：' + error, 6000);
      })
  ).then(() => true);
}

/**
 * 清空全部记忆与候选。
 *
 * 确认文案必须把三件事说清（§7.3）：**清掉的范围**（多少条记忆、多少条候选，含停用的）、
 * **清不掉的东西**（已导出的文件、已经发给模型的旧请求）、**防再生**（正文指纹留下来，
 * 模型提不回同一条）。只说「确定要清空吗」等于让用户在一个不完整的承诺上点确认。
 */
export function clearMemory() {
  const view = memoryStore.view;
  const memories = view ? view.memories.length : 0;
  const candidates = view ? view.candidates.length : 0;
  if (!memories && !candidates) {
    toast('现在没有可清空的记忆', 3000, 'warning');
    return Promise.resolve(false);
  }
  const detail =
    `将清空 ${memories} 条记忆与 ${candidates} 条待确认候选（含已停用的）。` +
    '删除后不再参与检索，正文会被移除，并记下正文指纹防止模型把同一条重新提回来；' +
    '已导出的文件与已经发给模型的旧请求不会被撤回——那些要另行处理会话历史。';
  return confirmDialog('清空全部记忆？', detail, '清空').then((ok) => {
    if (!ok) return false;
    return withLoading('memoryClear', () =>
      invoke('memory_clear')
        .then((outcome) => {
          toast(
            `已清空 ${outcome.memories} 条记忆与 ${outcome.candidates} 条候选`,
            4000,
            'success'
          );
          resetDraft();
          return refreshMemory({ silent: true });
        })
        .catch((error) => toastError('清空失败：' + error, 6000))
    );
  });
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

/**
 * 批准或忽略一条候选。`scope` 是**用户当场选的范围**（§6.1：「由用户明确选择通用范围」）：
 * 传 `'general'` 存成通用偏好（后端会清掉项目坐标），不传就沿用候选自带的范围。
 */
export function decideCandidate(candidate, accept, scope = null) {
  return withLoading('memoryCandidate:' + candidate.id, () =>
    invoke('memory_decide_candidate', { id: candidate.id, accept, scope })
      .then(() => {
        toast(
          accept ? (scope === 'general' ? '已存为通用偏好' : '已保存为记忆') : '已忽略这条候选',
          2500,
          'success'
        );
        return refreshMemory({ silent: true });
      })
      .catch((error) => toastError((accept ? '保存' : '忽略') + '失败：' + error, 6000))
  );
}

/// 按**上一次编辑过的**标题 + 关键词试一次检索，把「会注入什么」摊开给用户看。
///
/// 查询读 `memoryStore.previewQuery` 而不是草稿（原因见 store 里那段：弹窗的遮罩与
/// 「关窗丢草稿」两条合起来让草稿在这里恒为空）。
export function previewContext() {
  const query = memoryStore.previewQuery.trim();
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
