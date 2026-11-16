import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';
import { reactive, ref, computed } from 'vue';

// 执行真实 store / 面板脚本，仅替换 IPC、通知及生命周期边界。
const source = readFileSync(new URL('../src/hooks/hooks.js', import.meta.url), 'utf8');
const panel = readFileSync(new URL('../src/hooks/HooksPanel.vue', import.meta.url), 'utf8').split('<script setup>')[1].split('</script>')[0];
function setup() {
  const calls = [];
  const handlers = {};
  const errors = [];
  const body = source.replace(/^import .*;\n/gm, '').replace(/^export /gm, '');
  const names = [...source.matchAll(/export (?:async )?(?:function|const) (\w+)/g)].map((m) => m[1]);
  const api = new Function('reactive', 'invoke', 'withLoading', 'toastActionError', 'toastSuccess', 'formatBytes', body + '\nreturn {' + names.join(',') + '};')(
    reactive,
    async (cmd, args) => { calls.push({ cmd, args }); return handlers[cmd]?.(args); },
    (_, fn) => fn(), (...args) => errors.push(args), () => {}, String
  );
  return { ...api, calls, handlers, errors };
}
function deferred() {
  let resolve;
  const promise = new Promise((r) => { resolve = r; });
  return { promise, resolve };
}

function loadPanel(h, overrides = {}) {
  const importedNames = panel.match(/import \{([^}]+)\} from '\.\/hooks\.js';/s)[1].split(',').map(s => s.trim()).filter(Boolean);
  let unmount;
  const scope = { computed, ref, watch() {}, onMounted() {}, onUnmounted(fn) { unmount = fn; }, isLoading() {}, confirmDialog: async () => true, toast() {}, formatBytes: String, ...overrides };
  for (const name of importedNames) scope[name] = h[name];
  const body = panel.replace(/^import [\s\S]*?;\n/gm, '');
  const actions = new Function(...Object.keys(scope), body + '\nreturn { removeEndpoint, rotateToken, rotateNotification };')(...Object.values(scope));
  return { ...actions, unmount };
}

test('删除末页或容量清理后回退到有效页并重新加载', async () => {
  const h = setup();
  h.hookStore.inbox.offset = 20;
  h.handlers.hooks_inbox_list = ({ filter }) => ({ total: 20, items: filter.offset ? [] : [{ id: 'first' }] });
  await h.deleteEvent('last');
  assert.deepEqual(h.calls.filter(c => c.cmd === 'hooks_inbox_list').map(c => c.args.filter.offset), [20, 0]);
  assert.equal(h.hookStore.inbox.items[0].id, 'first');
  assert.equal(h.hookStore.inbox.offset, 0);
  assert.equal(h.hookStore.inbox.loading, false);
});

test('回退页的迟到响应不能覆盖新筛选', async () => {
  const h = setup();
  const late = deferred();
  h.hookStore.inbox.offset = 40;
  h.handlers.hooks_inbox_list = ({ filter }) => filter.offset ? { total: 1, items: [] } : late.promise;
  const old = h.loadInbox();
  await Promise.resolve(); await Promise.resolve();
  h.handlers.hooks_inbox_list = () => ({ total: 1, items: [{ id: 'new' }] });
  await h.setInboxFilter({ q: 'new' });
  late.resolve({ total: 1, items: [{ id: 'old' }] });
  await old;
  assert.equal(h.hookStore.inbox.items[0].id, 'new');
});

for (const [method, command, field] of [
  ['createEndpoint', 'hooks_create_endpoint', 'created'],
  ['rotateEndpointToken', 'hooks_rotate_endpoint_token', 'created'],
  ['rotateNotificationToken', 'hooks_rotate_endpoint_notification_token', 'notification'],
]) {
  test(method + ' 完成前离开页面不会回填秘密，但返回真实操作结果', async () => {
    const h = setup(); const late = deferred();
    h.handlers[command] = () => late.promise;
    const view = loadPanel(h);
    const pending = h[method]('ep1', ['text']);
    view.unmount();
    const result = { endpoint: { id: 'ep1' }, token: 'secret' };
    late.resolve(result);
    assert.equal(await pending, result);
    assert.equal(h.hookStore[field], null);
    assert.equal(h.errors.length, 0);
  });
}

test('首次预览、切换附件、关详情均有准确 loading 目标', async () => {
  const h = setup(); const first = deferred(); const second = deferred();
  h.hookStore.detail = { event: { id: 'e' } };
  h.handlers.hooks_attachment_preview = ({ attachmentId }) => attachmentId === 'a' ? first.promise : second.promise;
  const a = h.loadAttachmentPreview({ id: 'a' });
  assert.equal(h.hookStore.previewAttachmentId, 'a');
  const b = h.loadAttachmentPreview({ id: 'b' });
  first.resolve({}); await a;
  assert.equal(h.hookStore.previewAttachmentId, 'b');
  assert.equal(h.hookStore.previewLoading, true);
  h.closeDetail(); second.resolve({}); await b;
  assert.equal(h.hookStore.preview, null);
  assert.equal(h.hookStore.previewAttachmentId, null);
  assert.equal(h.hookStore.previewLoading, false);
});

test('刷新同时更新列表、接入点和容量状态', async () => {
  const h = setup();
  h.handlers.hooks_inbox_list = () => ({ total: 0, items: [] });
  h.handlers.hooks_status = () => ({ storage_bytes: 42 });
  await h.refreshInbox();
  assert.equal(h.hookStore.status.storage_bytes, 42);
  assert.deepEqual(h.calls.map(c => c.cmd).sort(), ['hooks_inbox_list', 'hooks_list_endpoints', 'hooks_status']);
});

test('面板确认删除实际调用导入的动作，取消不调用，失败由 store 报告', async () => {
  const h = setup();
  let approved = true;
  const { removeEndpoint: remove } = loadPanel(h, { confirmDialog: async () => approved });
  await remove({ id: 'ep', name: 'test' });
  assert.equal(h.calls.filter(c => c.cmd === 'hooks_delete_endpoint').length, 1);
  approved = false; await remove({ id: 'ep' });
  assert.equal(h.calls.filter(c => c.cmd === 'hooks_delete_endpoint').length, 1);
  approved = true; h.handlers.hooks_delete_endpoint = () => { throw Error('offline'); };
  await remove({ id: 'ep' });
  assert.equal(h.errors[0][0], '删除接入点失败');
});

test('全部事件被清理后回到首页空态', async () => {
  const h = setup(); h.hookStore.inbox.offset = 60;
  h.handlers.hooks_inbox_list = () => ({ total: 0, items: [] });
  await h.loadInbox();
  assert.equal(h.hookStore.inbox.offset, 0);
  assert.equal(h.hookStore.inbox.total, 0);
  assert.deepEqual(h.hookStore.inbox.items, []);
  assert.equal(h.calls.length, 2);
});

test('预览失败清理 loading，允许重试；模板使用在途附件而非上次结果', async () => {
  const h = setup(); h.hookStore.detail = { event: { id: 'e' } };
  h.handlers.hooks_attachment_preview = () => { throw Error('offline'); };
  await h.loadAttachmentPreview({ id: 'a' });
  assert.equal(h.hookStore.previewLoading, false);
  assert.equal(h.hookStore.previewAttachmentId, null);
  assert.match(h.hookStore.previewError, /offline/);
  h.handlers.hooks_attachment_preview = () => ({ text: 'ok' });
  await h.loadAttachmentPreview({ id: 'a' });
  assert.equal(h.hookStore.preview.text, 'ok');
  assert.equal(h.hookStore.previewError, '');
  const detail = readFileSync(new URL('../src/hooks/HookDetail.vue', import.meta.url), 'utf8');
  assert.match(detail, /:loading="hookStore.previewLoading && hookStore.previewAttachmentId === attachment.id"/);
});

for (const [action, command] of [
  ['removeEndpoint', 'hooks_delete_endpoint'],
  ['rotateToken', 'hooks_rotate_endpoint_token'],
  ['rotateNotification', 'hooks_rotate_endpoint_notification_token'],
]) {
  test(action + ' 离页后才确认不启动操作，新实例仍可操作', async () => {
    const h = setup(); const confirmation = deferred();
    const oldView = loadPanel(h, { confirmDialog: () => confirmation.promise });
    const row = { id: 'ep', name: 'test', notification_enabled: true };
    const pending = oldView[action](row);
    oldView.unmount();
    confirmation.resolve(true); await pending;
    assert.equal(h.calls.some(c => c.cmd === command), false);
    h.handlers[command] = () => ({ endpoint: { id: 'ep' }, token: 'fresh' });
    const newView = loadPanel(h);
    await newView[action](row);
    assert.equal(h.calls.filter(c => c.cmd === command).length, 1);
  });
}
