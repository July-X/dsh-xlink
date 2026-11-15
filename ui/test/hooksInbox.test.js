import assert from 'node:assert/strict';
import test from 'node:test';

// Hook 收件箱的异步竞态：切筛选 / 连点两行 / 关窗后迟到的响应不许落到界面上。
// 这里只钉 store 的请求序号语义，不测渲染；真实 IPC 由 Rust 侧用例覆盖。

const deferred = () => {
  let resolve;
  const promise = new Promise((res) => {
    resolve = res;
  });
  return { promise, resolve };
};

const lists = [];
const details = [];
const invocations = [];

const core = {
  invoke(command, args) {
    invocations.push({ command, args });
    if (command === 'hooks_inbox_list') return lists.shift().promise;
    if (command === 'hooks_event_detail') return details.shift().promise;
    return Promise.reject(new Error('unexpected command: ' + command));
  },
  Channel: class {
    onmessage = null;
  },
};

globalThis.window = { __TAURI__: { core }, navigator: { userAgent: 'node' } };
globalThis.document = {
  hidden: false,
  createElement: () => ({}),
  createElementNS: () => ({}),
  createTextNode: () => ({}),
  createComment: () => ({}),
  querySelector: () => null,
  addEventListener() {},
  removeEventListener() {},
  body: { classList: { toggle() {} } },
};

const hooks = await import('../src/hooks/hooks.js');

test('a stale inbox response never overwrites a newer filter result', async () => {
  const first = deferred();
  const second = deferred();
  lists.push(first, second);

  hooks.hookStore.inbox.filter.endpoint_id = 'ep_a';
  const stale = hooks.loadInbox();
  hooks.hookStore.inbox.filter.endpoint_id = 'ep_b';
  const fresh = hooks.loadInbox();

  second.resolve({ items: [{ id: 'evt_b' }], total: 1 });
  await fresh;
  first.resolve({ items: [{ id: 'evt_a' }], total: 1 });
  await stale;

  assert.equal(hooks.hookStore.inbox.items[0].id, 'evt_b');
  assert.equal(hooks.hookStore.inbox.total, 1);
  assert.equal(hooks.hookStore.inbox.loading, false);
});

test('a stale detail response cannot land on the next opened event', async () => {
  const first = deferred();
  const second = deferred();
  details.push(first, second);

  const stale = hooks.openEvent('evt_a');
  const fresh = hooks.openEvent('evt_b');
  second.resolve({ event: { id: 'evt_b' }, attachments: [] });
  await fresh;
  first.resolve({ event: { id: 'evt_a' }, attachments: [] });
  await stale;

  assert.equal(hooks.hookStore.detail.event.id, 'evt_b');
  assert.equal(hooks.hookStore.detailLoading, false);
});

test('closing the detail discards a late response', async () => {
  const late = deferred();
  details.push(late);

  const pending = hooks.openEvent('evt_late');
  hooks.closeDetail();
  late.resolve({ event: { id: 'evt_late' }, attachments: [] });
  await pending;

  assert.equal(hooks.hookStore.detail, null);
  assert.equal(hooks.hookStore.detailLoading, false);
});

test('send address is built from endpoint id and port, never contains the token', () => {
  hooks.hookStore.status = { config: { port: 17890 } };
  const address = hooks.endpointAddress({ id: 'ep_1' });
  assert.equal(address, 'http://127.0.0.1:17890/hooks/v1/endpoints/ep_1');
  hooks.hookStore.created = { token: 'tok'.repeat(20), endpoint: { id: 'ep_1' } };
  assert.equal(hooks.endpointAddress(hooks.hookStore.created.endpoint), address);
  assert.doesNotMatch(address, new RegExp(hooks.hookStore.created.token));
});

test('receiving events show stored-over-total instead of a single classification', () => {
  assert.equal(
    hooks.attachmentSummary({ state: 'receiving', stored_attachments: 1, attachment_count: 2 }),
    '接收中（1/2）'
  );
  assert.equal(
    hooks.attachmentSummary({ state: 'ready', stored_attachments: 2, attachment_count: 2, attachment_bytes: 2048 }),
    '2 个附件 · 2.0 KB'
  );
});
