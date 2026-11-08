// 目标能力只描述已有接入；不能把 DSH 的安装状态冒充 MiniMax 安装成功。
export function minimaxPluginTarget(id, state, hasInstance = false) {
  if (id !== 'ponytail') {
    const tip = id === 'memory' ? '请到记忆页授权 MiniMax 使用记忆；此处不重复修改记忆权限。'
      : '此插件尚未适配 MiniMax，不能直接安装 DSH 插件包。';
    return { enabled: false, disabled: true, text: id === 'memory' ? '记忆页授权' : '待适配', tip };
  }
  const enabled = !!state.view?.ponytail_enabled;
  const reason = state.error || state.view?.ponytail_error || state.catalogError;
  if (reason) return { enabled, disabled: true, text: '状态不可用', tip: String(reason) + '，请刷新后重试。' };
  if (state.loading || !state.view) return { enabled, disabled: true, text: '读取中', tip: '正在读取 MiniMax 接入状态。' };
  if (!hasInstance) return { enabled, disabled: true, text: '未安装内核', tip: '请先在内核版本页安装 MiniMax。' };
  if (state.view.recovery_snapshot) return { enabled, disabled: true, text: '需清理旧记录', tip: '请先重启电脑并确保其他窗口没有运行 MiniMax，再到概览页点「重启后清理记录」，之后修改插件接入。' };
  if (state.view.stop_required) return { enabled, disabled: true, text: '需先停止', tip: '请先停止 MiniMax 工作台，再修改插件接入。' };
  return { enabled, disabled: false, text: enabled ? '已接入' : '未接入', tip: '独立控制 MiniMax 的 Ponytail 接入，不影响 DSH；原生插件实际生效仍待验证。关闭仅撤销授权，不删除共享插件文件。' };
}
