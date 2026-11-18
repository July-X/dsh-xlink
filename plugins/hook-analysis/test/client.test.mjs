import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import test from 'node:test';
import assert from 'node:assert/strict';

const source = readFileSync(new URL('../client/client.js', import.meta.url), 'utf8');
const settle = () => new Promise((resolve) => setImmediate(resolve));
function deferred() {
  let resolve;
  const promise = new Promise((done) => { resolve = done; });
  return { promise, resolve };
}

function fixture({ beforeBind = async () => {}, beforeRename = async () => {}, take = null } = {}) {
  let api;
  let phase = 'ready';
  let workspacePhase = 'ready';
  let workspaceSubscriber;
  let subscriber;
  let dispose;
  const rows = { A: { id: 'A', displayTitle: 'A 标题', cwd: '/known', updatedAt: 5, running: false, blank: false } };
  const drafts = {};
  const workspaces = [{ workspaceId: 'ws-known', path: '/known', sessionIds: ['A'] }];
  const calls = { spaces: [], create: [], rename: [], draft: [], opened: [], reports: [], sessions: [], checkpoints: [] };
  const events = new Map();
  const workspaceList = { getSnapshot: () => ({ phase: workspacePhase, items: workspaces }),
    subscribe(callback) { workspaceSubscriber = callback; return () => { workspaceSubscriber = null; }; } };
  const window = {
    __ModuleLoader__: { load(module) { api = module.factory(); } },
    __TAURI_INTERNALS__: { async invoke(command, args) {
      if (command === 'hooks_analysis_take') return take;
      if (command === 'hooks_analysis_checkpoint') calls.checkpoints.push(args);
      else if (command === 'hooks_analysis_report') calls.reports.push(args);
      else if (command === 'hooks_analysis_sessions_report') calls.sessions.push(args);
      else throw new Error('unexpected command ' + command);
    } },
    addEventListener(name, callback) { events.set(name, callback); },
    removeEventListener(name) { events.delete(name); },
  };
  vm.runInNewContext(source, { window, console });
  api.apply({
    sessions: {
      list: { getSnapshot: () => ({ phase, ids: Object.keys(rows), byId: rows }),
        subscribe(callback) { subscriber = callback; return () => { subscriber = null; }; } },
      async create({ workspaceId, sessionId }) {
        calls.create.push([workspaceId, sessionId]);
        const workspace = workspaces.find((row) => row.workspaceId === workspaceId);
        workspace.sessionIds.push(sessionId);
        rows[sessionId] = { id: sessionId, cwd: workspace.path, blank: true };
        return sessionId;
      },
      async using(id, _options, operation) {
        await beforeBind(id);
        await operation({ binding: {
          id, ctx: { id }, session: {
            getSnapshot: () => ({ running: rows[id]?.running, removed: !rows[id] }),
            async rename(title) {
              calls.rename.push([id, title]);
              await beforeRename(id);
              rows[id].displayTitle = title;
            },
          },
        } });
      },
    },
    uiWorkspace: { openSession(id) { calls.opened.push(id); } },
    workspaces: {
      list: workspaceList,
      async create({ path }) {
        calls.spaces.push(path);
        const workspace = { workspaceId: 'ws-created', path, sessionIds: [] };
        workspaces.push(workspace);
        return workspace;
      },
    },
    conversation: {
      input: {
        requestDraftInitialization(binding, { prompt, whenDraftExists }) {
          const id = binding.id;
          calls.draft.push([id, whenDraftExists, prompt]);
          if (whenDraftExists === 'keep' && drafts[id]) return 'preserved';
          drafts[id] = drafts[id] ? drafts[id] + '\n' + prompt : prompt;
          return 'applied';
        },
        for: ({ id }) => ({ state: { getSnapshot: () => ({ draft: drafts[id] }) } }),
      },
    },
    on(name, callback) { assert.equal(name, 'dispose'); dispose = callback; },
  });
  return { calls, drafts, rows, setWorkspacePhase(value) { workspacePhase = value; workspaceSubscriber?.(); }, setPhase(value) { phase = value; subscriber?.(); }, dispose,
    dispatch(detail) { events.get('dsh-xlink-hook-analysis')?.({ detail: { expiresAt: Date.now() + 60_000, ...detail } }); },
    list(queryId) { events.get('dsh-xlink-hook-analysis-sessions')?.({ detail: { queryId } }); },
  };
}

function fresh(sequence, id, workspacePath = '/known') {
  const handoffId = 'ha_' + id;
  return { sequence, handoffId, action: 'prepare-new', workspacePath,
    reservedSessionId: id, sessionTitle: 'Hook ' + id, question: 'question ' + id,
    materials: '> 【Hook 材料 ' + handoffId + ' · 正文】' };
}

test('两次新建使用独立的预分配会话，同一交接重试保留草稿', async () => {
  const f = fixture();
  const first = fresh(1, 'first', '/new');
  f.dispatch(first);
  await settle();
  f.dispatch({ ...first, sequence: 2, sessionId: 'first' });
  await settle();
  f.dispatch(fresh(3, 'second', '/new'));
  await settle();
  assert.deepEqual(f.calls.spaces, ['/new']);
  assert.deepEqual(f.calls.create, [['ws-created', 'first'], ['ws-created', 'second']]);
  assert.deepEqual(f.calls.rename, [['first', 'Hook first'], ['second', 'Hook second']]);
  assert.deepEqual(f.calls.opened, ['first', 'first', 'second']);
  assert.equal(f.drafts.first, 'question first\n' + first.materials);
  assert.equal(f.drafts.second, 'question second\n' + fresh(3, 'second').materials);
  assert.equal(f.calls.reports.length, 3);
  assert.equal(f.calls.reports[0].error, '');
});

test('过期或被更新序号取代的改名等待无法写草稿', async () => {
  const hold = deferred();
  const f = fixture({ beforeRename: () => hold.promise });
  f.dispatch(fresh(1, 'first'));
  await settle();
  const latest = { sequence: 2, handoffId: 'ha_b', action: 'prepare-existing', sessionId: 'A', materials: '> new' };
  f.dispatch(latest);
  await settle();
  hold.resolve();
  await settle();
  assert.equal(f.drafts.first, undefined);
  assert.equal(f.drafts.A, '> new');
  assert.deepEqual(f.calls.opened, ['A']);
  assert.deepEqual(f.calls.reports.map((row) => row.sequence), [2]);
  f.dispatch({ ...latest, sequence: 3, expiresAt: Date.now() - 1 });
  await settle();
  assert.equal(f.drafts.A, '> new');
});

test('等待会话绑定时更新目标，旧任务不会改草稿', async () => {
  const hold = deferred();
  const f = fixture({ beforeBind: (id) => id === 'A' ? hold.promise : undefined });
  f.dispatch({ sequence: 1, handoffId: 'ha_a', action: 'prepare-existing', sessionId: 'A', materials: '> old' });
  await settle();
  f.dispatch(fresh(2, 'fresh'));
  await settle();
  hold.resolve();
  await settle();
  assert.equal(f.drafts.A, undefined);
  assert.deepEqual(f.calls.opened, ['fresh']);
});

test('最终写入前拦截变忙的会话，原草稿保留', async () => {
  const hold = deferred();
  const f = fixture({ beforeBind: () => hold.promise });
  f.drafts.A = 'already typed';
  f.dispatch({ sequence: 1, handoffId: 'ha_a', action: 'prepare-existing', sessionId: 'A', workspacePath: '/known', materials: '> new' });
  await settle();
  f.rows.A.running = true;
  hold.resolve();
  await settle();
  assert.equal(f.drafts.A, 'already typed');
  assert.deepEqual(f.calls.opened, []);
  assert.match(f.calls.reports[0].error, /正在生成/);
});

test('普通打开保留空白草稿，显式恢复只追加一次', async () => {
  const f = fixture();
  const target = { sequence: 1, handoffId: 'ha_a', action: 'open', sessionId: 'A', workspacePath: '/known' };
  f.dispatch(target);
  await settle();
  assert.equal(f.drafts.A, undefined);
  f.drafts.A = 'typed';
  const restore = { ...target, sequence: 2, action: 'prepare-existing', materials: '> 【Hook 材料 ha_a · 正文】' };
  f.dispatch(restore);
  await settle();
  f.dispatch({ ...restore, sequence: 3 });
  await settle();
  assert.equal(f.drafts.A, 'typed\n' + restore.materials);
  assert.deepEqual(f.calls.opened, ['A', 'A', 'A']);
});

test('删除、空间不符或卸载时不替换会话或写入', async () => {
  const f = fixture();
  const target = { sequence: 1, handoffId: 'ha_a', action: 'open', sessionId: 'A', workspacePath: '/other' };
  f.dispatch(target);
  await settle();
  assert.match(f.calls.reports[0].error, /原工作空间/);
  delete f.rows.A;
  f.dispatch({ ...target, sequence: 2 });
  await settle();
  assert.match(f.calls.reports[1].error, /原会话已删除/);
  f.dispose();
  f.dispatch(fresh(3, 'fresh'));
  await settle();
  assert.deepEqual(f.calls.create, []);
  assert.deepEqual(f.calls.opened, []);
});

test('会话清单区分未就绪和空列表，忽略空白会话', async () => {
  const f = fixture();
  f.rows.A.running = true;
  f.rows.B = { id: 'B', blank: true };
  f.list('ready');
  f.setPhase('loading');
  f.list('loading');
  await settle();
  assert.equal(f.calls.sessions[0].items.length, 1);
  assert.equal(f.calls.sessions[0].items[0].running, true);
  assert.equal(f.calls.sessions[1].items, null);
});

test('早到的工作台事件等待会话就绪，同序号事件不重复执行', async () => {
  const f = fixture();
  f.setPhase('loading');
  const target = fresh(1, 'fresh');
  f.dispatch(target);
  await settle();
  assert.deepEqual(f.calls.create, []);
  f.setPhase('ready');
  f.dispatch(target);
  await settle();
  assert.equal(f.calls.create.length, 1);
  assert.equal(f.calls.reports.length, 1);
});

test('工作空间挂接失败留下的未归组会话不能接收材料', async () => {
  const f = fixture();
  f.rows.orphan = { id: 'orphan', blank: true };
  f.dispatch(fresh(1, 'orphan'));
  await settle();
  assert.deepEqual(f.calls.draft, []);
  assert.deepEqual(f.calls.create, []);
  assert.match(f.calls.reports[0].error, /原工作空间尚未登记/);
});

test('重开已有 Hook 会话会等待工作空间流就绪', async () => {
  const f = fixture();
  f.setWorkspacePhase('loading');
  f.dispatch({ sequence: 1, handoffId: 'ha_a', action: 'open', sessionId: 'A', reservedSessionId: 'A', workspacePath: '/known' });
  await settle();
  assert.deepEqual(f.calls.opened, []);
  assert.deepEqual(f.calls.reports, []);
  f.setWorkspacePhase('ready');
  await settle();
  assert.deepEqual(f.calls.opened, ['A']);
  assert.equal(f.calls.reports[0].error, '');
});
