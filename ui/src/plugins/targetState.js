// 目标能力只描述已有接入；不能把 DSH 的安装状态冒充 MiniMax 安装成功。
//
// 每个内置插件在 MiniMax 侧的「接入」形态不同：
// · ponytail：装一棵 MiniMax 原生插件树（外壳持有归属与升级事务）；
// · openai-oauth：只打开 MiniMax **自带**的 Codex 登录（config.yaml 的
//   beta.codexOAuth），登录在 MiniMax 工作台里做，账号与 DSH 相互独立；
// · official-chat：装一份 MiniMax 本地插件（MCP + skill）去读官网会话正文。
//   MiniMax 没有 DSH 那个 @ 菜单，也没有发送前固定快照——文案如实说清。
// · memory：权限统一在记忆页，这里只反映现状。
const TARGETS = {
  // MiniMax 0.1.7 起自带 Codex（ChatGPT）登录，默认可用：壳没有开关可拨，
  // 也不该替用户登录。这一格只回答「这个内核有没有这条能力、去哪儿登录」。
  'openai-oauth': {
    builtin: true,
    supportedKey: 'openai_oauth_supported',
    tip: 'MiniMax 自带 ChatGPT（Codex）登录，默认可用。打开 MiniMax 工作台 → 设置 → 用量与模型 → 自定义模型，点「连接 Codex OAuth」登录；账号与 DSH 的 OpenAI 登录相互独立，DSH 这边不用退出。',
    unsupported: '当前 MiniMax 版本没有内置 ChatGPT 登录，请先在内核版本页升级到 0.1.7 或更新版本。',
  },
  'official-chat': {
    enabledKey: 'official_chat_enabled',
    errorKey: 'official_chat_error',
    supportedKey: 'official_chat_supported',
    tip: '开启后，MiniMax 工作台里的模型可以按需读取你在 DeepSeek / MiniMax / ChatGPT 官网的会话正文。MiniMax 没有 @ 菜单与发送前固定快照，引用由模型在对话中按需工具读取，正文可能不完整。',
    unsupported: '当前 MiniMax 版本不能接入官网对话引用，请先在内核版本页升级 MiniMax。',
  },
};

// 两个内核共用的阻塞态：状态读不到、实例没装、有旧记录、工作台在跑都不能改接线。
function unavailable(state, hasInstance, errorKey) {
  const reason = state.error || (errorKey ? state.view?.[errorKey] : null) || state.catalogError;
  if (reason) return { disabled: true, text: '状态不可用', tip: String(reason) + '，请刷新后重试。' };
  if (state.loading || !state.view) return { disabled: true, text: '读取中', tip: '正在读取 MiniMax 接入状态。' };
  if (!hasInstance) return { disabled: true, text: '未安装内核', tip: '请先在内核版本页安装 MiniMax。' };
  if (state.view.recovery_snapshot) return { disabled: true, text: '需清理旧记录', tip: '请先重启电脑并确保其他窗口没有运行 MiniMax，再到概览页点「重启后清理记录」，之后修改插件接入。' };
  if (state.view.stop_required) return { disabled: true, text: '需先停止', tip: '请先停止 MiniMax 工作台，再修改插件接入。' };
  return null;
}

function ponytailTarget(state, hasInstance) {
  const enabled = !!state.view?.ponytail_enabled;
  const blocked = unavailable(state, hasInstance, 'ponytail_error');
  if (blocked) return { enabled, ...blocked };
  return {
    enabled,
    disabled: false,
    text: enabled ? '已接入' : '未接入',
    tip: '独立控制 MiniMax 的 Ponytail 接入，不影响 DSH；原生插件实际生效仍待验证。关闭仅撤销授权，不删除共享插件文件。',
  };
}

function builtinTarget(spec, state, hasInstance) {
  if (spec.builtin) {
    // 内置能力：没有壳可拨的开关，只如实报能力是否在这个版本里。
    if (state.loading || !state.view) return { enabled: false, disabled: true, text: '读取中', tip: '正在读取 MiniMax 接入状态。' };
    if (!hasInstance) return { enabled: false, disabled: true, text: '未安装内核', tip: '请先在内核版本页安装 MiniMax。' };
    const supported = state.view?.[spec.supportedKey] !== false;
    return { enabled: supported, disabled: true, text: supported ? '已内置' : '需升级内核', tip: supported ? spec.tip : spec.unsupported };
  }
  const enabled = !!state.view?.[spec.enabledKey];
  const recoverable = state.view?.official_chat_recoverable === true;
  const blocked = unavailable(state, hasInstance, recoverable ? null : spec.errorKey);
  if (blocked) return { enabled, ...blocked };
  if (recoverable && state.view?.official_chat_enabled === false) {
    return { enabled, disabled: false, retryDisable: true, text: '需清理', tip: state.view.official_chat_error };
  }
  if (state.view?.[spec.supportedKey] === false && !enabled) {
    return { enabled, disabled: true, text: '需升级内核', tip: spec.unsupported };
  }
  if (recoverable) return { enabled, disabled: false, text: '需修复', tip: state.view.official_chat_error };
  if (state.view?.[spec.supportedKey] === false) {
    return { enabled, disabled: false, text: '需升级内核', tip: spec.unsupported + '可先关闭已有接入。' };
  }
  return { enabled, disabled: false, text: enabled ? '已接入' : '未接入', tip: spec.tip };
}

export function minimaxPluginTarget(id, state, hasInstance = false, memoryGranted = null) {
  if (id === 'memory') {
    // 记忆能力没有「给 MiniMax 装一份插件包」这回事：权限统一在记忆页（页顶的
    // 接收方开关）维护。这里只把那份授权的现状带过来——已授权时显示成
    // 「已接入」，否则用户在记忆页开完了回到这里，看到的仍是「未授权」，
    // 会以为那次操作没生效。改权限仍只回记忆页，不在这条行上开第二个入口。
    if (memoryGranted === null) {
      return { enabled: false, disabled: true, text: '读取中', tip: '正在读取记忆页的授权状态。' };
    }
    return {
      enabled: memoryGranted,
      disabled: true,
      text: memoryGranted ? '已接入' : '未接入',
      tip: memoryGranted
        ? '已在记忆页允许 MiniMax 使用记忆；记忆权限统一在记忆页修改。'
        : '请到记忆页允许 MiniMax 使用记忆；此处不重复修改记忆权限。',
    };
  }
  if (id === 'ponytail') return ponytailTarget(state, hasInstance);
  const spec = TARGETS[id];
  if (spec) return builtinTarget(spec, state, hasInstance);
  return { enabled: false, disabled: true, text: '待适配', tip: '此插件尚未适配 MiniMax，不能直接安装 DSH 插件包。' };
}
