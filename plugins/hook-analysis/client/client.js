/**
 * Hook 分析交接的客户端半边（工作台页面里运行）。
 *
 * 职责只有三件：按壳派发的交接目标找到或创建工作空间、把会话接出来、把
 * 材料引用块与建议问题写进该会话的草稿。**它不发送**——唯一的模型调用
 * 仍是用户在工作台输入框里按发送。壳侧权威状态在
 * src-tauri/src/hooks/analysis.rs；两半的合同见
 * docs/features/hooks/deepseek-analysis-design.md。
 *
 * 内核能力（0.2.2-rc.4 阶段 A 核实，行号见设计文档「已核实与待验证」）：
 *  - workspaces.create({ path }) 幂等注册工作空间，不改实例 cwd；
 *  - uiWorkspace.connectWorkspace(workspaceId) 建会话并**复用同空间空白会话**；
 *  - conversation.input.requestDraftInitialization(binding, { prompt,
 *    whenDraftExists }) 在会话装配前写入草稿：keep 保留已有草稿（不拼接问题）、
 *    append 追加材料引用块。
 *
 * 束纪律与 memory/shell-navigation 相同：单文件自包含；**全部声明在 factory
 * 闭包里**——client-hmr 重求值落在页面全局词法作用域，顶层 const 第二次求值
 * 就是 SyntaxError、整条 entry 静默死掉。
 *
 * shortcut: 材料以 Markdown 引用块进入草稿（requestDraftInitialization 的唯一
 * 跨插件内容通道），恢复判据也读草稿文本标记；可折叠 quote chip 待实机验证后升级。
 */
window.__ModuleLoader__.load({
  id: 'xlink-hook-analysis',
  factory: () => ({
    // 服务名（不是包名）：runner 按它决定 ctx 上给不给，漏声明当场拒。
    inject: ['uiWorkspace', 'sessions', 'workspaces', 'conversation'],
    apply(ctx) {
      const invoke = window.__TAURI_INTERNALS__?.invoke;
      if (typeof invoke !== 'function') return;
      let latest = 0;
      let disposed = false;

      const markerOf = (handoffId) => '【Hook 材料 ' + handoffId;

      async function report(sequence, handoffId, sessionId, error) {
        try {
          await invoke('hooks_analysis_report', {
            sequence,
            handoffId,
            sessionId: sessionId == null ? null : sessionId,
            error: error == null ? '' : String(error),
          });
        } catch (reason) {
          console.error('hook analysis report failed', reason);
        }
      }

      function sessionsReady() {
        return ctx.sessions.list.getSnapshot().phase === 'ready';
      }

      // 幂等：路径已在注册表就复用；create 对已注册路径也是幂等解析。
      function findOrBuildWorkspace(path) {
        const existing = ctx.workspaces.list.getSnapshot().items
          .find((item) => item.path === path);
        if (existing) return Promise.resolve(existing);
        return ctx.workspaces.create({ path });
      }

      // 持引用期间操作会话绑定；失败原样上抛给调用方落账。
      async function withBinding(sessionId, operation) {
        await ctx.sessions.using(sessionId, { source: 'xlinkHookAnalysis' }, async (reference) => {
          await operation(reference.binding);
        });
      }

      async function renameSession(sessionId, title) {
        await ctx.sessions.using(sessionId, { source: 'xlinkHookAnalysis' }, (reference) =>
          reference.binding.session.rename(title),
        );
      }

      /** 追加材料引用块；'blocked' 表示会话正在提交或生成。 */
      function appendMaterials(binding, materials) {
        const result = ctx.conversation.input.requestDraftInitialization(binding, {
          prompt: materials,
          whenDraftExists: 'append',
        });
        if (result === 'blocked') {
          throw new Error('该会话正在提交或生成回复，稍后从「查看分析」重试，不打断当前任务');
        }
      }

      /** 读取当前草稿文本；装配前读不到时按“无标记”处理。 */
      function currentDraft(binding) {
        try {
          const input = ctx.conversation.input.for(binding.ctx);
          return String(input.state.getSnapshot().draft ?? '');
        } catch {
          return '';
        }
      }

      async function handoff(target) {
        if (!target || disposed || typeof target.sequence !== 'number' || target.sequence <= latest) return;
        latest = target.sequence;
        try {
          if (!sessionsReady()) throw new Error('会话列表尚未就绪；请等工作台加载完成后从「查看分析」重试');
          if (target.action === 'prepare-new') {
            const workspace = await findOrBuildWorkspace(target.workspacePath);
            const sessionId = await ctx.uiWorkspace.connectWorkspace(workspace.workspaceId);
            if (disposed || latest !== target.sequence) return;
            if (target.sessionTitle) await renameSession(sessionId, target.sessionTitle).catch(() => {});
            await withBinding(sessionId, (binding) => {
              // 草稿为空才预填建议问题；复用的空白会话已有草稿时保留原文、不拼接问题。
              if (target.question) {
                ctx.conversation.input.requestDraftInitialization(binding, {
                  prompt: target.question,
                  whenDraftExists: 'keep',
                });
              }
              appendMaterials(binding, target.materials);
            });
            if (disposed || latest !== target.sequence) return;
            ctx.uiWorkspace.openSession(sessionId);
            await report(target.sequence, target.handoffId, sessionId, '');
            return;
          }
          // prepare-existing / reopen：目标会话必须仍在，不在就如实报不可用，
          // 绝不静默新建替代会话（设计 §4.2）。
          const sessionId = target.sessionId;
          const summary = ctx.sessions.list.getSnapshot().byId[sessionId];
          if (!summary) throw new Error('session-unavailable: 目标会话已删除或当前实例不可见，不新建替代会话');
          if (target.action === 'reopen') {
            await withBinding(sessionId, (binding) => {
              // 恢复只重建未注入过的引用：草稿里已有本交接的标记就不再追加。
              if (!currentDraft(binding).includes(markerOf(target.handoffId))) {
                appendMaterials(binding, target.materials);
              }
            });
          } else {
            await withBinding(sessionId, (binding) => appendMaterials(binding, target.materials));
          }
          if (disposed || latest !== target.sequence) return;
          ctx.uiWorkspace.openSession(sessionId);
          await report(target.sequence, target.handoffId, sessionId, '');
        } catch (reason) {
          const message = String(reason?.message ?? reason);
          console.error('hook analysis handoff failed:', message);
          await report(target.sequence, target.handoffId, null, message);
        }
      }

      // 会话列表桥：主面板「选择已有会话」只列当前实例可见的会话；
      // running 原样回传，由准备页标注「正在生成回复，不可交接」。
      const listSessions = (event) => {
        const snapshot = ctx.sessions.list.getSnapshot();
        const rows = snapshot.phase === 'ready'
          ? snapshot.ids.map((id) => snapshot.byId[id]).filter((row) => row && !row.blank)
          : [];
        void invoke('hooks_analysis_sessions_report', {
          queryId: event.detail?.queryId,
          items: rows.map((row) => ({
            id: row.id,
            title: row.displayTitle,
            cwd: row.cwd == null ? '' : row.cwd,
            updatedAt: row.updatedAt,
            running: row.running === true,
          })),
        }).catch((reason) => console.error('hook analysis sessions report failed', reason));
      };
      window.addEventListener('dsh-xlink-hook-analysis-sessions', listSessions);

      const receive = (event) => { void handoff(event.detail).catch((reason) => console.error('hook analysis event', reason)); };
      window.addEventListener('dsh-xlink-hook-analysis', receive);

      // 工作台后加载 / 重载后拉取未消费的交接目标（事件可能在页面装配前派发）。
      const ready = () => {
        if (!sessionsReady()) return;
        void invoke('hooks_analysis_take')
          .then((target) => { if (target) void handoff(target); })
          .catch((reason) => console.error('hook analysis take failed', reason));
      };
      const stop = ctx.sessions.list.subscribe(() => { ready(); });
      void ready();

      ctx.on('dispose', () => {
        disposed = true;
        stop?.();
        window.removeEventListener('dsh-xlink-hook-analysis', receive);
        window.removeEventListener('dsh-xlink-hook-analysis-sessions', listSessions);
      });
    },
  }),
});
