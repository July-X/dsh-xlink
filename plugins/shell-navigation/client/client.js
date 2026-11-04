// 使用内核导航服务；不读写会话正文，不操作会话列表 DOM。
window.__ModuleLoader__.load({
  id: 'xlink-shell-navigation',
  factory: () => ({
    inject: ['uiWorkspace', 'sessions'],
    apply(ctx) {
      const invoke = window.__TAURI_INTERNALS__?.invoke;
      if (typeof invoke !== 'function') return;
      let latest = 0;
      let disposed = false;
      const normalize = (id) => String(id).replace(/^session-/, '');
      async function navigate(target) {
        if (!target || disposed || target.sequence <= latest) return;
        if (ctx.sessions.list.getSnapshot().phase === 'pending') return;
        latest = target.sequence;
        let error = '';
        try {
          const sessions = ctx.sessions.list.getSnapshot();
          const matches = sessions.ids.filter((id) => normalize(id) === normalize(target.sessionId));
          if (matches.length !== 1) throw new Error('目标会话已删除或尚未同步，请在工作台确认会话列表后重试');
          if (disposed || latest !== target.sequence) return;
          ctx.uiWorkspace.openSession(matches[0]);
        } catch (reason) {
          error = String(reason?.message ?? reason);
        }
        await invoke('notification_navigation_result', { sequence: target.sequence, error });
      }
      const receive = (event) => { void navigate(event.detail).catch((error) => console.error('notification navigation', error)); };
      window.addEventListener('dsh-xlink-notification', receive);
      // 列表加载完成后才认定会话不存在；使用公开快照订阅，不猜加载延时。
      const ready = () => invoke('notification_navigation_ready').then(navigate).catch((error) => console.error('notification navigation ready', error));
      const stop = ctx.sessions.list.subscribe(() => { void ready(); });
      void ready();
      ctx.on('dispose', () => { disposed = true; stop?.(); window.removeEventListener('dsh-xlink-notification', receive); });
    },
  }),
});
