import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import test from 'node:test';
import assert from 'node:assert/strict';
const source = readFileSync(new URL('../client/client.js', import.meta.url), 'utf8');
function fixture(phase = 'ready') {
  let api; let snapshot = { phase, ids: ['session-A', 'session-B'] };
  let subscriber; const events = new Map(); const opened = []; const acknowledged = [];
  let pending = null; let dispose;
  const window = {
    __ModuleLoader__: { load(module) { api = module.factory(); } },
    __TAURI_INTERNALS__: { async invoke(command, args) {
      if (command === 'notification_navigation_ready') return pending;
      acknowledged.push(args);
    } },
    addEventListener(name, callback) { events.set(name, callback); },
    removeEventListener(name) { events.delete(name); },
  };
  vm.runInNewContext(source, { window, console });
  api.apply({
    sessions: { list: { getSnapshot: () => snapshot, subscribe(callback) { subscriber = callback; return () => { subscriber = null; }; } } },
    uiWorkspace: { openSession(target) { assert.equal(typeof target, 'string', '公开接口只接收会话ID或子代理地址'); opened.push(target); } },
    on(name, callback) { assert.equal(name, 'dispose'); dispose = callback; },
  });
  return { opened, acknowledged, dispose,
    click(sequence, sessionId) { pending = { sequence, sessionId }; events.get('dsh-xlink-notification')?.({ detail: pending }); },
    ready() { snapshot = { ...snapshot, phase: 'ready' }; subscriber?.(); },
  };
}
const settle = () => new Promise((resolve) => setImmediate(resolve));
test('A/B通知按点击选择各自会话，旧点击不覆盖新点击', async () => {
  const f = fixture();
  f.click(1, 'B'); f.click(2, 'A'); f.click(1, 'B');
  await settle();
  assert.deepEqual(f.opened, ['session-B', 'session-A']);
  assert.equal(f.acknowledged.length, 2);
});
test('列表未就绪时保留目标，就绪后导航且只导航一次', async () => {
  const f = fixture('pending');
  f.click(1, 'A'); await settle(); assert.deepEqual(f.opened, []);
  f.ready(); await settle(); assert.deepEqual(f.opened, ['session-A']);
  f.ready(); await settle(); assert.deepEqual(f.opened, ['session-A']);
});
test('删除的会话报错，不创建新会话；卸载后忽略点击', async () => {
  const f = fixture();
  f.click(1, 'missing'); await settle();
  assert.deepEqual(f.opened, []);
  assert.match(f.acknowledged[0].error, /删除/);
  f.dispose(); f.click(2, 'A'); await settle(); assert.deepEqual(f.opened, []);
});
