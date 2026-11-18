import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import test from 'node:test';
import assert from 'node:assert/strict';
const source = readFileSync(new URL('../client/client.js', import.meta.url), 'utf8');

/**
 * 与 shell-navigation 的夹具同款：vm 沙箱 + 可编程的内核服务面。
 * binding 形状对齐内核 SessionBinding：.ctx 是会话作用域，.session 有 rename。
 */
function fixture() {
  let api;
  let phase = 'ready';
  let byId = { 'session-A': { id: 'session-A', displayTitle: 'A 标题', cwd: '/w/a', updatedAt: 5, running: false, blank: false } };
  let subscriber;
  const calls = { create: [], connect: [], rename: [], draftInits: [], opened: [], reports: [], sessionsReports: [] };
  const workspaces = [{ workspaceId: 'ws-known', path: '/known' }];
  let bindingDraft = '';
  const binding = {
    ctx: {},
    session: { rename(title) { calls.rename.push(title); return Promise.resolve(); } },
  };
  const window = {
    __ModuleLoader__: { load(module) { api = module.factory(); } },
    __TAURI_INTERNALS__: { async invoke(command, args) {
      if (command === 'hooks_analysis_take') return null;
      if (command === 'hooks_analysis_report') { calls.reports.push(args); return; }
      if (command === 'hooks_analysis_sessions_report') { calls.sessionsReports.push(args); return; }
      throw new Error('unexpected command ' + command);
    } },
    addEventListener(name, callback) { (this._events ||= new Map()).set(name, callback); },
    removeEventListener(name) { this._events.delete(name); },
  };
  vm.runInNewContext(source, { window, console });
  let disposeCallback;
  api.apply({
    sessions: {
      list: {
        getSnapshot: () => ({ phase, ids: Object.keys(byId), byId }),
        subscribe(callback) { subscriber = callback; return () => { subscriber = null; }; },
      },
      using(_id, _options, operation) { return operation({ binding }); },
    },
    uiWorkspace: {
      async connectWorkspace(workspaceId) { calls.connect.push(workspaceId); return 'session-new'; },
      openSession(id) { assert.equal(typeof id, 'string', 'openSession 只接收会话 id'); calls.opened.push(id); },
    },
    workspaces: {
      list: { getSnapshot: () => ({ items: workspaces }) },
      async create(input) { calls.create.push(input); return { workspaceId: 'ws-created', path: input.path }; },
    },
    conversation: {
      input: {
        requestDraftInitialization(b, options) { calls.draftInits.push(options); return 'applied'; },
        for(_actx) { return { state: { getSnapshot: () => ({ draft: bindingDraft }) } }; },
      },
    },
    on(name, callback) { assert.equal(name, 'dispose'); disposeCallback = callback; },
  });
  return {
    calls,
    setPhase(value) { phase = value; },
    removeSession(id) { delete byId[id]; },
    setDraft(text) { bindingDraft = text; },
    dispatch(detail) { window._events.get('dsh-xlink-hook-analysis')?.({ detail }); },
    listRequest(queryId) { window._events.get('dsh-xlink-hook-analysis-sessions')?.({ detail: { queryId } }); },
    dispose() { disposeCallback?.(); },
  };
}
const settle = () => new Promise((resolve) => setImmediate(resolve));

test('prepare-new：建空间、接会话、改名、问题 keep + 材料 append、开窗、报成功', async () => {
  const f = fixture();
  f.dispatch({ sequence: 1, handoffId: 'ha_1', action: 'prepare-new', workspacePath: '/analysis/workspace', sessionTitle: 'Hook · 失败', question: '分析这次构建', materials: '> 材料甲' });
  await settle();
  assert.deepEqual(f.calls.create.map((c) => c.path), ['/analysis/workspace'], '未注册路径要 create');
  assert.deepEqual(f.calls.connect, ['ws-created']);
  assert.deepEqual(f.calls.rename, ['Hook · 失败'], '新会话按事件标题改名');
  assert.deepEqual(f.calls.draftInits.map((o) => [o.prompt, o.whenDraftExists]), [
    ['分析这次构建', 'keep'],
    ['> 材料甲', 'append'],
  ], '问题 keep（已有草稿不拼接），材料 append');
  assert.deepEqual(f.calls.opened, ['session-new']);
  assert.equal(f.calls.reports.length, 1);
  assert.equal(f.calls.reports[0].error, '');
  assert.equal(f.calls.reports[0].sessionId, 'session-new');
});

test('已知工作空间复用注册，不再 create；同 sequence 重复派发只交接一次', async () => {
  const f = fixture();
  const target = { sequence: 2, handoffId: 'ha_2', action: 'prepare-new', workspacePath: '/known', sessionTitle: null, question: null, materials: '> m' };
  f.dispatch(target);
  await settle();
  assert.deepEqual(f.calls.create, [], '/known 已注册，直接复用');
  assert.deepEqual(f.calls.connect, ['ws-known']);
  assert.deepEqual(f.calls.draftInits.map((o) => o.whenDraftExists), ['append'], '无问题时只追加材料');
  f.dispatch(target);
  f.dispatch({ ...target, sequence: 1 });
  await settle();
  assert.equal(f.calls.reports.length, 1, '同 sequence 与旧 sequence 都不重复交接');
});

test('reopen：草稿已有本交接标记则不重复注入；没有标记才追加', async () => {
  const f = fixture();
  f.setDraft('已有内容 【Hook 材料 ha_3 · 正文】来源 X');
  f.dispatch({ sequence: 3, handoffId: 'ha_3', action: 'reopen', sessionId: 'session-A', materials: '> 材料' });
  await settle();
  assert.deepEqual(f.calls.draftInits, [], '标记在，禁止重复');
  assert.deepEqual(f.calls.opened, ['session-A']);
  assert.equal(f.calls.reports[0].error, '');

  const g = fixture();
  g.setDraft('');
  g.dispatch({ sequence: 4, handoffId: 'ha_4', action: 'reopen', sessionId: 'session-A', materials: '> 材料' });
  await settle();
  assert.deepEqual(g.calls.draftInits.map((o) => o.whenDraftExists), ['append'], '无标记时重建未发送引用');
  assert.deepEqual(g.calls.opened, ['session-A']);
});

test('目标会话已删除：报 session-unavailable，不打开、不新建', async () => {
  const f = fixture();
  f.removeSession('session-A');
  f.dispatch({ sequence: 5, handoffId: 'ha_5', action: 'reopen', sessionId: 'session-A', materials: '> m' });
  await settle();
  assert.deepEqual(f.calls.opened, []);
  assert.match(f.calls.reports[0].error, /session-unavailable/);
  assert.equal(f.calls.reports[0].sessionId, null);
});

test('会话列表桥：跳过空白会话，running 原样回传', async () => {
  const f = fixture();
  f.listRequest('q-1');
  await settle();
  assert.equal(f.calls.sessionsReports.length, 1);
  const reported = f.calls.sessionsReports[0];
  assert.equal(reported.queryId, 'q-1');
  // vm 沙箱里的对象跨 realm，deepStrictEqual 会因原型不同而失败：按字段比。
  assert.equal(reported.items.length, 1);
  assert.equal(reported.items[0].id, 'session-A');
  assert.equal(reported.items[0].title, 'A 标题');
  assert.equal(reported.items[0].cwd, '/w/a');
  assert.equal(reported.items[0].running, false);
});

test('卸载后忽略后续派发', async () => {
  const f = fixture();
  f.dispose();
  f.dispatch({ sequence: 6, handoffId: 'ha_6', action: 'reopen', sessionId: 'session-A', materials: '> m' });
  await settle();
  assert.deepEqual(f.calls.opened, []);
  assert.equal(f.calls.reports.length, 0);
});
