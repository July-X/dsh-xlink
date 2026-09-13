// 启动容错事故的归因口径（共享层）。
//
// Rust 侧 `guard.rs` 为每次启动失败产出 `cause`：`plugin` / `kernel` / `frontend` /
// `env` / `unknown`。概览页横幅与事故弹层都要把同一个 cause 渲染成标题、判断标签和
// 「下一步去哪个面板」，这张表此前在 `OverviewPanel.vue` 与 `IncidentModal.vue`
// 各写了一份——于是两边各漏了一次 `env`：后端已经给出明确的环境原因（端口被占用、
// 权限、磁盘），前端却回落到「暂未能归因」，把用户引向插件与内核重装，正好抵消了
// guard 侧 P2-3/P2-4 的文案修复。口径收在这里，新增 cause 只需要改一处。

/// 已知归因。未知值按证据回退，最后落到 `unknown`。
const KNOWN_CAUSES = ['plugin', 'kernel', 'frontend', 'env', 'unknown'];

const TITLES = {
  plugin: '工作台异常：疑似插件问题',
  kernel: '工作台异常：疑似内核问题',
  frontend: '工作台异常：前端 bundle 异常',
  env: '工作台异常：运行环境问题',
  unknown: '工作台异常：暂未能归因',
};

const CAUSE_LABELS = {
  plugin: '判断：疑似插件问题',
  kernel: '判断：疑似内核问题',
  frontend: '判断：前端 bundle 异常（未定位到包名）',
  env: '判断：运行环境问题（本次未改动插件配置）',
  unknown: '判断：暂未能归因',
};

/// 事故的归因。缺字段时按嫌疑对象回退，避免老记录（无 `cause`）显示成空白。
export function incidentCause(value) {
  if (!value) return '';
  if (KNOWN_CAUSES.includes(value.cause)) return value.cause;
  const suspects = value.suspects || [];
  if (suspects.some((suspect) => suspect.kind === 'plugin')) return 'plugin';
  if (suspects.some((suspect) => suspect.kind === 'kernel')) return 'kernel';
  return 'unknown';
}

export function incidentTitle(value) {
  if (!value) return '工作台异常';
  if (value.recovered) return '已在安全模式下启动工作台';
  return TITLES[incidentCause(value)] || TITLES.unknown;
}

export function incidentCauseLabel(value) {
  return CAUSE_LABELS[incidentCause(value)] || CAUSE_LABELS.unknown;
}

/// 概览页横幅的标题。前端异常不弹模态框，横幅是它唯一的入口，所以单独一句。
export function incidentBannerTitle(value) {
  const cause = incidentCause(value);
  if (cause === 'frontend') return '工作台自检：前端异常（页面正常）';
  if (cause === 'env') return '工作台启动失败：运行环境问题';
  return '启动容错已介入';
}

/// 事故的「下一步」该去哪个面板：插件问题（或确有被隔离的插件）→ 插件页；
/// 环境问题 → 设置页（端口 / 数据目录都在那里）；其余 → 内核版本页。
export function incidentDestination(value, quarantinedCount = 0) {
  const cause = incidentCause(value);
  if (cause === 'plugin' || quarantinedCount > 0) return 'plugins';
  if (cause === 'env') return 'settings';
  return 'versions';
}

export function incidentDestinationLabel(destination) {
  if (destination === 'plugins') return '前往插件页';
  if (destination === 'settings') return '前往设置页';
  return '前往内核版本页';
}
