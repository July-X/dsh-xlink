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

/// 自检类型的可读名。`health.kind` 是后端契约字面量，直接印在面板上等于把契约
/// 丢给用户读；未知值原样透出，后端加了新类型也不至于显示成空白。
const HEALTH_KIND_LABELS = {
  blank: '页面空白（无可见内容）',
  'runtime-error': '运行时错误',
  'unhandled-rejection': '未处理的 Promise 异常',
  'bundle-load-failure': '内核客户端模块 bundle 加载失败',
};

export function incidentHealthKindLabel(kind) {
  const key = String(kind || '').trim();
  if (!key) return '';
  return HEALTH_KIND_LABELS[key] || key;
}

/// 从文本里抽出内核客户端模块路由里的包名，两种形态都认（与 Rust 侧
/// `bundle_member` 的锚定规则同源）：`/plugins/??<包名>/client.js,…&rev=…` 的
/// **多成员**组合，与 `/plugins/<包名>/client.<chunk>.js?rev=…` 的**单资源**。
///
/// 这不是归因——多成员组合里的帧属于整批脚本，按成员逐个匹配会把旁观者写进隔离
/// 清单（`guard.rs` 的 `is_ambiguous_combo_line` 因此拒绝）。它的用处是**给人看**：
/// 「组合路由共 57 个成员」没人能定位，把包名列出来一眼就知道多了谁、少了谁。
export function bundleRouteMembers(text) {
  const source = String(text || '');
  if (!source) return [];
  const found = new Set();
  const scan = (pattern, split) => {
    let matched = pattern.exec(source);
    while (matched) {
      for (const part of split(matched[1])) {
        const at = part.indexOf('/client');
        if (at > 0) found.add(part.slice(0, at));
      }
      matched = pattern.exec(source);
    }
  };
  scan(/\/plugins\/\?\?([^&\s"'()]+)/g, (value) => value.split(','));
  scan(/\/plugins\/([^?&\s"'()]+?\/client\.[A-Za-z0-9._-]*\.js)/g, (value) => [value]);
  return [...found];
}

/// 把一份自检证据拆成带标签的分段。此前面板把类型 / 消息 / 堆栈 / 页面拼成一个大
/// `<pre>`，组合路由那种一行几十个包名的地址挤在里面谁也读不出来；分段后最有用
/// 的那几项单独一行，其余原样保留，不丢任何原始信息。
export function incidentHealthSections(health) {
  if (!health) return [];
  const sections = [];
  const kind = incidentHealthKindLabel(health.kind);
  if (kind) sections.push({ label: '类型', text: kind });
  const members = bundleRouteMembers(`${health.message || ''}\n${health.stack || ''}`);
  if (members.length) sections.push({ label: `涉及的客户端模块（${members.length} 个）`, text: '', members });
  const message = String(health.message || '').trim();
  const stack = String(health.stack || '').trim();
  // bundle 加载失败时消息是「前缀 + 那个地址」，地址已单列成一节、证据里也原样
  // 在——按「包含」而非「相等」去重，否则带前缀的那份会把重复项漏出去。
  if (message && !(stack && message.includes(stack))) sections.push({ label: '消息', text: message });
  if (stack) sections.push({ label: '原始证据', text: stack });
  const page = String(health.page_url || '').trim();
  if (page) sections.push({ label: '页面', text: page });
  return sections;
}

/// 复制用的纯文本证据（分段拼接），让人能整段贴进反馈或工单。
export function incidentHealthPlainText(health) {
  return incidentHealthSections(health)
    .map((section) => (section.members ? `${section.label}：${section.members.join('、')}` : `${section.label}：${section.text}`))
    .join('\n');
}
