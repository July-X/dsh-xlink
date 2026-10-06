// 诊断相关的展示映射：stage / status / cause 的中文名与语义色。
//
// **未知取值必须显示出来，不能悄悄丢弃。** 后端允许写入任意 stage /
// status / cause（原样保留是 `run.rs` 的纪律），旧壳读到新取值时如果
// 直接丢掉，用户看到的时间线会「缺一块」——而缺的那块恰恰是新版才有
// 的信息，正是他最想知道的。所以三张表都走「查不到 → 显式标未知」。
import { tildePath } from '../shell/labels.js';

/** 启动 / 预检 / 恢复 / 排查的阶段。顺序即时间线默认展示顺序。 */
export const STAGE_LABELS = {
  'resolve-instance': '解析实例',
  'detect-node': '检查 Node.js',
  'resolve-pnpm': '准备 pnpm',
  'prepare-wiring': '准备插件接线',
  'spawn-kernel': '启动内核',
  'wait-ready': '等待端口就绪',
  'health-check': '检查工作台状态',
  'record-startup-ok': '保存成功快照',
  retry: '自动重试',
  'sandbox-create': '创建沙盒环境',
  baseline: '建立环境基线',
  'install-candidate': '安装候选插件',
  'probe-candidate': '启动沙盒并探测',
  report: '生成预检报告',
  // 恢复（后端 `operation_run::restore_stage`）
  prepare: '检查恢复条件',
  apply: '写入回退点内容',
  verify: '恢复后自检',
  backup: '保存恢复前快照',
  // 排查（后端 `operation_run::bisect_stage`）
  select: '选定候选',
  probe: '逐轮试探',
  conclude: '收出结论',
  abort: '中止排查',
};

/**
 * 运行记录 / 事件状态 → 文案 + 语义色。
 *
 * **时间线上的事件**用短标签（「已完成」），**运行记录的头部**用整句
 * （`RUN_HEADLINE`）——后者出现在卡片顶部，用户第一眼读的是那一行，
 * 短标签在那里会读成「已完成」而不是「工作台已启动，但有告警」。
 */
export const STATUS_META = {
  running: { label: '进行中', tone: 'active' },
  success: { label: '已完成', tone: 'ok' },
  warning: { label: '有告警', tone: 'warn' },
  failure: { label: '失败', tone: 'bad' },
  inconclusive: { label: '未能完成', tone: 'unknown' },
  canceled: { label: '已取消', tone: 'muted' },
};

/** 运行记录卡片头部的一行结论（设计 §5.2 的状态表）。 */
export const RUN_HEADLINE = {
  running: '正在启动工作台',
  success: '工作台已启动',
  warning: '已启动，有告警',
  failure: '工作台未能启动',
  inconclusive: '证据不足，暂时无法判断失败原因',
  canceled: '用户中止了本次操作',
};

/** 该状态需要给「重试启动」这个动作。`inconclusive` 不给：它不是失败。 */
export function isRetryable(status) {
  return ['failure', 'warning', 'canceled', 'inconclusive'].includes(String(status || ''));
}

/** 归因 → 中文名。空值单独处理：它表示「后端没给归因」，不是「归因未知」。 */
export const CAUSE_LABELS = {
  environment: '环境问题',
  kernel: '内核',
  plugin: '插件',
  skill: '技能',
  frontend: '工作台前端',
  unknown: '未知原因',
};

/** 运行类型 → 页面标题用语。 */
export const KIND_LABELS = {
  startup: '启动诊断',
  'plugin-precheck': '插件安全诊断',
  restore: '恢复诊断',
  bisect: '排查诊断',
};

/**
 * 恢复 / 排查的卡片头部结论，按 kind 分开。
 *
 * **为什么不给这两种 kind 复用 `RUN_HEADLINE`**：那张表的每句话主语都是
 * 「工作台」，而恢复做的是改配置、排查做的是缩小插件范围——套过去会写出
 * 「工作台已启动」这种与操作毫无关系的结论。查不到就退回短标签，而不是
 * 编一句。
 *
 * `bisect.success` 写「已收窄到最小可疑组合」而不是「已找到根因」：组合
 * 效应会让二分停在一个不可修的答案上（设计 §11.1）。
 */
export const OPERATION_HEADLINE = {
  restore: {
    running: '正在恢复到所选回退点',
    success: '已恢复到所选回退点',
    warning: '已恢复，但有条目被跳过',
    failure: '恢复没有完成',
    inconclusive: '恢复已尝试，但证据不足以确认环境可用',
    canceled: '已取消恢复',
  },
  bisect: {
    running: '正在排查插件组合',
    success: '已收窄到最小可疑组合',
    warning: '排查收出了结论，但有轮次没能试成',
    failure: '排查没能进行下去',
    inconclusive: '证据不足，暂时无法判断',
    canceled: '已取消排查',
  },
};

/**
 * 运行记录卡片头部的那一句结论。
 *
 * 四种 kind 各查各的表——这是**故意不合并**的：合并成一张就等于让「恢复
 * 成功」复用「工作台已启动」的措辞，而那句话对恢复没有任何意义。
 */
export function headlineFor(run) {
  const status = String(run?.status ?? '');
  const kind = String(run?.kind ?? '');
  const table = kind === 'startup' ? RUN_HEADLINE : OPERATION_HEADLINE[kind];
  if (table && table[status]) return table[status];
  return statusMeta(status).label;
}

/**
 * 一次运行**应该经过**的阶段（按 kind）。
 *
 * 存在的理由只有一个：「完成 N / M 个阶段」里的 M 必须来自这份集合。
 * 拿事件条数当分母时，一个阶段推三条事件就会显示成「完成 3 / 6」而实际只
 * 走了两步——数字看着在动，用户却对不上它指的是什么（审查 P2-03）。
 *
 * **与后端的 stage 常量逐条对应**（`startup_run` / `precheck` /
 * `operation_run` 各自那组）。这里多写一份是因为「M」是**用户看到的承诺**，
 * 后端那份是「实际会推的」；两者不一致时该改的是这里——M 少一个会让
 * 永远到不了 100%，多一个会让永远差一步。
 */
export const STAGE_SEQUENCES = {
  startup: [
    'resolve-instance',
    'detect-node',
    'resolve-pnpm',
    'prepare-wiring',
    'spawn-kernel',
    'wait-ready',
    'health-check',
    'record-startup-ok',
  ],
  'plugin-precheck': [
    'sandbox-create',
    'baseline',
    'install-candidate',
    'probe-candidate',
    'report',
  ],
  restore: ['prepare', 'apply', 'verify', 'backup'],
  bisect: ['select', 'probe', 'conclude'],
};

/**
 * 阶段进度。
 *
 * `done` 只数**真正走到过**的阶段（出现过任一事件即算），`failed` 单列出来
 * ——把失败也算进「完成」会让「完成 6 / 6」与一个失败结论同时出现。
 * 未知 kind 退回按事件里出现过的 distinct stage 统计，`total` 为实际发生
 * 过的数量：不知道序列时不编一个分母。
 */
export function stageProgress(kind, events) {
  const list = Array.isArray(events) ? events : [];
  const seen = new Set(list.map((event) => String(event?.stage ?? '')));
  const sequence = STAGE_SEQUENCES[kind];
  if (!sequence) {
    return { total: seen.size, done: seen.size, failed: 0, running: 0, pending: 0 };
  }
  // 序列里没列过的 stage（新版新增、旧壳未知）**不计入分母**，但也别让它
  // 从「完成」里消失：多一个分子会显示成 7/6。
  const total = sequence.length;
  const done = sequence.filter((stage) => seen.has(stage)).length;
  const failed = list.filter((event) => String(event?.status) === 'failure').length;
  const running = sequence.filter(
    (stage) => list.some((event) => String(event?.stage) === stage && event.status === 'running')
  ).length;
  return { total, done, failed, running, pending: Math.max(0, total - done) };
}

/** 阶段中文名；未知阶段显式说明它是什么值，便于定位新版行为。 */
export function stageLabel(stage) {
  const key = String(stage ?? '');
  return STAGE_LABELS[key] || (key ? `未知阶段（${key}）` : '未知阶段');
}

/**
 * 状态 → 展示元数据。
 *
 * `inconclusive` 刻意不叫「失败」：它表示证据不足（基线就没起来、
 * 日志没写成），把它画成失败会让用户去处置无辜的插件。
 */
export function statusMeta(status) {
  const key = String(status ?? '');
  if (STATUS_META[key]) return { ...STATUS_META[key], raw: key };
  return { label: key ? `未知状态（${key}）` : '未知状态', tone: 'unknown', raw: key };
}

/** 归因中文名；空值返回空串（调用方自行决定显示什么）。 */
export function causeLabel(cause) {
  const key = String(cause ?? '');
  if (!key) return '';
  return CAUSE_LABELS[key] || `未知原因（${key}）`;
}

/** 运行类型 → 标题。 */
export function kindLabel(kind) {
  const key = String(kind ?? '');
  return KIND_LABELS[key] || (key ? `运行记录（${key}）` : '运行记录');
}

/** 耗时（毫秒）→ 「12.4 秒」/「340 毫秒」。 */
export function durationLabel(ms) {
  const value = Number(ms);
  if (!Number.isFinite(value) || value <= 0) return '';
  if (value < 1000) return `${Math.round(value)} 毫秒`;
  return `${(value / 1000).toFixed(1)} 秒`;
}

/**
 * 归因 → 用户下一步能做什么。
 *
 * 这段是「失败归因」这一层存在的意义：不给出下一步的失败提示，用户只能
 * 去翻日志或重装。文案只说证据支持的事——二分 / 归因结果是「当前证据
 * 指向」，不是「根因」。
 */
export function nextStepFor(run) {
  const status = String(run?.status ?? '');
  const cause = String(run?.cause ?? '');
  if (status === 'success') return '';
  if (status === 'inconclusive') {
    return '证据不足，暂时无法判断失败原因。请先打开完整日志确认现场；若日志也不完整，重启一次工作台会产生新的运行记录。';
  }
  if (status === 'canceled') return '你中止了这次操作，可以重新发起一次。';
  if (cause === 'environment') {
    return '端口被占用就换一个端口（设置页）或结束占用该端口的进程；权限 / 磁盘问题请检查数据目录是否可写、磁盘是否已满。';
  }
  if (cause === 'plugin') {
    return '当前证据指向插件。请打开事故面板逐个查看嫌疑插件的处置动作，或用「插件安全诊断」验证某一个插件。';
  }
  if (cause === 'kernel') {
    return '当前证据指向内核本身。可在「内核版本」页切换到其他已安装版本，或重新安装当前版本。';
  }
  if (cause === 'frontend') {
    return '当前证据指向工作台页面。先关闭并重新打开工作台窗口；反复出现时打开完整日志并把自检消息一并反馈。';
  }
  if (cause === 'skill') {
    return '当前证据指向技能。可在「技能」页逐个停用后重启工作台验证。';
  }
  return '暂时无法归因。请打开完整日志查看内核侧的记录，再决定换版本、换端口还是回退配置。';
}

/** 证据路径 → 折叠 home 后的显示文本。 */
export function evidenceLabel(path) {
  const value = String(path ?? '');
  return value ? tildePath(value) : '';
}

/**
 * 找出时间线上第一个失败阶段。
 *
 * 启动诊断默认只展开它：默认全开会把 6 个阶段的说明一次性推下去，
 * 用户反而找不到出问题的那一行。
 */
export function firstFailedEvent(events) {
  const list = Array.isArray(events) ? events : [];
  for (const event of list) {
    if (String(event?.status) === 'failure') return event;
  }
  return null;
}

/** 时间线按 seq 排序后的副本。**绝不按字符串排**：`seq` 是数字，
 * 字符串排序会把 10 排到 2 前面。 */
export function sortedBySeq(events) {
  return (Array.isArray(events) ? events.slice() : []).sort(
    (a, b) => Number(a?.seq ?? 0) - Number(b?.seq ?? 0)
  );
}