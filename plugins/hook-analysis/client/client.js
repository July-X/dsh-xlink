/**
 * 工作台客户端只准备草稿。发送仍由用户在原生输入框触发。
 * DSH 0.2.2-rc.4 的 sessions.create 接受预分配 ID；壳先落盘同一个 ID，
 * 再允许工作台创建或重试。所有声明留在 factory 内，供 client-hmr 重新求值。
 */
window.__ModuleLoader__.load({
  id: 'xlink-hook-analysis',
  factory: () => ({
    inject: ['uiWorkspace', 'sessions', 'workspaces', 'conversation'],
    apply(ctx) {
      const invoke = window.__TAURI_INTERNALS__?.invoke;
      if (typeof invoke !== 'function') return;
      let latest = 0;
      let disposed = false;
      let deferredTarget = null;
      let taking = false;

      const sessionsReady = () => ctx.sessions.list.getSnapshot().phase === 'ready';
      const workspacesReady = () => ctx.workspaces.list.getSnapshot().phase === 'ready';
      const current = (target) => !disposed && target.sequence === latest && Date.now() < target.expiresAt;
      const marker = (target) => '【Hook 材料 ' + target.handoffId + ' ·';
      const summary = (sessionId) => ctx.sessions.list.getSnapshot().byId[sessionId];

      function checkpoint(target, sessionId = null, writingDraft = false) {
        return invoke('hooks_analysis_checkpoint', {
          sequence: target.sequence,
          handoffId: target.handoffId,
          sessionId,
          writingDraft,
        });
      }

      function listSessions(event) {
        const snapshot = ctx.sessions.list.getSnapshot();
        const items = snapshot.phase !== 'ready' ? null
          : snapshot.ids.map((id) => snapshot.byId[id]).filter((row) => row && !row.blank)
            .map((row) => ({
              id: row.id,
              title: row.displayTitle,
              cwd: row.cwd ?? '',
              updatedAt: row.updatedAt,
              running: row.running === true,
            }));
        void invoke('hooks_analysis_sessions_report', { queryId: event.detail?.queryId, items })
          .catch((error) => console.error('hook analysis sessions report failed', error));
      }

      function workspaceKey(path) {
        if (!/^(?:[A-Za-z]:[\\/]|[\\/]{2})/.test(path)) return path;
        return path.replace(/\\/g, '/').replace(/^[A-Z]:/, (drive) => drive.toLowerCase()).replace(/\/+$/, '');
      }

      function checkSession(target, sessionId, snapshot) {
        const row = summary(sessionId);
        const cwd = row?.cwd;
        if (target.workspacePath && cwd && workspaceKey(cwd) !== workspaceKey(target.workspacePath)) {
          throw new Error('session-unavailable: 目标会话已不在原工作空间');
        }
        if (snapshot?.removed === true) throw new Error('session-unavailable: 原会话已删除');
        if (target.action !== 'open' && (snapshot?.running === true || row?.running === true
          || snapshot?.awaitingFirstTurn === true || snapshot?.pendingSubmissions?.length > 0)) {
          throw new Error('该会话正在生成回复，请等待完成后重试');
        }
      }

      async function renameBestEffort(session, title) {
        try {
          const result = await session.rename(title);
          if (!result.ok) console.warn('hook analysis rename failed:', result.error);
        } catch (error) {
          console.warn('hook analysis rename failed:', error);
        }
      }

      function targetWorkspace(target) {
        if (!target.workspacePath) return undefined;
        return ctx.workspaces.list.getSnapshot().items
          .find((item) => workspaceKey(item.path) === workspaceKey(target.workspacePath));
      }

      async function prepare(target) {
        let sessionId = target.sessionId;
        let createdHere = false;
        if (!sessionId && target.action === 'prepare-new') {
          sessionId = target.reservedSessionId;
          if (!sessionId) throw new Error('交接缺少预分配会话 ID');
          await checkpoint(target);
          if (!current(target)) return null;
          if (!summary(sessionId)) {
            const workspace = targetWorkspace(target) ?? await ctx.workspaces.create({ path: target.workspacePath });
            if (!current(target)) return null;
            await checkpoint(target);
            if (!current(target)) return null;
            const created = await ctx.sessions.create({ workspaceId: workspace.workspaceId, sessionId });
            if (created !== sessionId) throw new Error('内核未返回预分配会话，草稿未写入');
            createdHere = true;
          }
        } else if (!sessionId || !summary(sessionId)) {
          throw new Error('session-unavailable: 原会话已删除或当前实例不可见');
        }
        if (!createdHere && target.reservedSessionId
          && !targetWorkspace(target)?.sessionIds?.includes(sessionId)) {
          throw new Error('原工作空间尚未登记该会话，请刷新工作台后重试');
        }
        if (!current(target)) return null;
        checkSession(target, sessionId);

        let opened = false;
        await ctx.sessions.using(sessionId, { source: 'xlinkHookAnalysis' }, async ({ binding }) => {
          if (!current(target)) return;
          checkSession(target, sessionId, binding.session.getSnapshot());
          // 会话绑定和网络请求可等待任意时长，必须在最后一次 await 后再检查。
          await checkpoint(target, sessionId);
          if (!current(target)) return;
          checkSession(target, sessionId, binding.session.getSnapshot());
          if (target.action !== 'open') {
            const input = ctx.conversation.input.for(binding.ctx);
            if (!String(input.state.getSnapshot().draft ?? '').includes(marker(target))) {
              await checkpoint(target, sessionId, true);
              if (!current(target)) return;
              checkSession(target, sessionId, binding.session.getSnapshot());
            }
            if (!String(input.state.getSnapshot().draft ?? '').includes(marker(target))) {
              const initialize = (prompt, whenDraftExists) => {
                const result = ctx.conversation.input.requestDraftInitialization(binding, { prompt, whenDraftExists });
                if (result === 'blocked') throw new Error('该会话正在提交或生成回复，请等待完成后重试');
              };
              if (target.question) initialize(target.question, 'keep');
              initialize(target.materials, 'append');
            }
          }
          ctx.uiWorkspace.openSession(sessionId);
          opened = true;
          if (current(target) && target.action === 'prepare-new' && target.sessionTitle && !summary(sessionId)?.title) {
            // 在原会话绑定仍有效时发出改名，不等待 RPC，也不让它改写交接回执。
            void renameBestEffort(binding.session, target.sessionTitle);
          }
        });
        return opened ? sessionId : null;
      }

      async function deliver(target) {
        let sessionId;
        let error = '';
        try {
          sessionId = await prepare(target);
        } catch (reason) {
          error = String(reason?.message ?? reason);
          console.error('hook analysis handoff failed:', error);
        }
        if (!current(target) || (!sessionId && !error)) return;
        try {
          await invoke('hooks_analysis_report', {
            sequence: target.sequence,
            handoffId: target.handoffId,
            sessionId: sessionId ?? null,
            error,
          });
        } catch (reason) {
          // 壳保留未持久化的回执；不要把落盘失败误报为另一次工作台失败。
          console.error('hook analysis report failed:', reason);
        }
      }

      function accept(target) {
        if (!target || disposed || !Number.isSafeInteger(target.sequence)
          || target.sequence <= latest || !(Date.now() < target.expiresAt)) return;
        if (!deferredTarget || deferredTarget.sequence < target.sequence) deferredTarget = target;
        startReadyTarget();
      }

      function startReadyTarget() {
        const target = deferredTarget;
        if (!target || disposed || !sessionsReady()
          || (target.reservedSessionId && !workspacesReady())) return;
        deferredTarget = null;
        if (target.sequence <= latest || !(Date.now() < target.expiresAt)) return;
        latest = target.sequence;
        void deliver(target);
      }

      function ready() {
        startReadyTarget();
        if (disposed || taking || !sessionsReady()) return;
        taking = true;
        void invoke('hooks_analysis_take').then(accept)
          .catch((error) => console.error('hook analysis take failed', error))
          .finally(() => { taking = false; });
      }

      const receive = (event) => accept(event.detail);
      window.addEventListener('dsh-xlink-hook-analysis', receive);
      window.addEventListener('dsh-xlink-hook-analysis-sessions', listSessions);
      const stopSessions = ctx.sessions.list.subscribe(ready);
      const stopWorkspaces = ctx.workspaces.list.subscribe(startReadyTarget);
      ready();

      ctx.on('dispose', () => {
        disposed = true;
        stopSessions?.();
        stopWorkspaces?.();
        window.removeEventListener('dsh-xlink-hook-analysis', receive);
        window.removeEventListener('dsh-xlink-hook-analysis-sessions', listSessions);
      });
    },
  }),
});
