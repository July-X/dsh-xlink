import assert from 'node:assert/strict';
import test from 'node:test';

// 与既有 UI 测试同一套做法：先注入最小可用的 __TAURI__ 桥与 document，再动态
// import 前端模块。notifications.js 只经 bridge.js 的 invoke 与 Rust 通信，
// 因此拦下 invoke 就能在没有桌面外壳的 node 环境里跑完整条链路。
const calls = [];
let nextStatus = null;
let failNext = null;

function statusPayload(overrides = {}) {
  return {
    enabled: true,
    notifyAwayOnly: true,
    sound: true,
    unread: 0,
    items: [],
    watching: true,
    lastError: null,
    environmentNote: null,
    platform: 'macos',
    ...overrides,
  };
}

const core = {
  invoke(command, args) {
    calls.push({ command, args });
    if (failNext && failNext.command === command) {
      const error = failNext.error;
      failNext = null;
      return Promise.reject(error);
    }
    if (command === 'notification_save_settings') {
      // Rust 保存后回显完整状态：这里用入参覆盖，模拟"保存生效"。
      return Promise.resolve({ ...nextStatus, ...args });
    }
    if (
      command === 'notification_status' ||
      command === 'notification_mark_read' ||
      command === 'notification_test'
    ) {
      return Promise.resolve(nextStatus);
    }
    throw new Error(`unexpected command: ${command}`);
  },
  Channel: class {
    onmessage = null;
  },
};

globalThis.window = {
  __TAURI__: { core },
  navigator: { userAgent: 'node' },
  addEventListener() {},
  removeEventListener() {},
  getComputedStyle() {
    return {
      transitionDuration: '0s',
      animationDuration: '0s',
      transitionDelay: '0s',
      animationDelay: '0s',
    };
  },
};
Object.defineProperty(globalThis, 'navigator', {
  configurable: true,
  value: { userAgent: 'node' },
});

const makeElement = () => ({
  ownerDocument: globalThis.document,
  style: {},
  classList: { add() {}, remove() {}, contains() { return false; }, toggle() {} },
  addEventListener() {},
  removeEventListener() {},
  setAttribute() {},
  removeAttribute() {},
  appendChild() {},
  removeChild() {},
  insertBefore() {},
});
const body = makeElement();
globalThis.document = {
  createElement: makeElement,
  createElementNS: makeElement,
  createTextNode: makeElement,
  createComment: makeElement,
  body,
  documentElement: makeElement(),
  addEventListener() {},
  removeEventListener() {},
};
// ElMessage 的进出场过渡会排队 requestAnimationFrame：缺了它，失败提示的动画
// 会在测试结束后抛 unhandledRejection，把整个文件判红。
globalThis.requestAnimationFrame = (callback) => {
  callback();
  return 1;
};
globalThis.cancelAnimationFrame = () => {};

test('状态规范化：字段缺失回落默认值', async () => {
  const { normalizeNotificationStatus } = await import('../src/notifications.js');

  assert.deepEqual(normalizeNotificationStatus(null), {
    enabled: true,
    notifyAwayOnly: true,
    sound: false,
    unread: 0,
    items: [],
    watching: false,
    lastError: null,
    environmentNote: null,
    platform: '',
  });
  // 非布尔 / 非数字 / 空白字符串一律回落到可解释的默认值，而不是把 undefined
  // 直接塞进开关。
  const messy = normalizeNotificationStatus({
    enabled: 'false',
    notifyAwayOnly: 0,
    sound: null,
    unread: -3,
    watching: 'yes',
    lastError: '   ',
    platform: 7,
  });
  assert.equal(messy.enabled, true);
  assert.equal(messy.notifyAwayOnly, true);
  assert.equal(messy.sound, false, '提示音默认关闭，与 Rust 侧默认值一致');
  assert.equal(messy.unread, 0, '负数未读按 0 处理');
  assert.equal(messy.watching, false);
  assert.equal(messy.lastError, null, '只有空白的 lastError 不算错误');
  assert.equal(messy.platform, '');
});

test('状态规范化：完成记录裁剪到 8 条并补齐字段', async () => {
  const { normalizeNotificationStatus } = await import('../src/notifications.js');

  const items = Array.from({ length: 10 }, (_, i) => ({
    sessionId: 's' + i,
    title: '会话 ' + i,
    cwd: '/tmp/' + i,
    finishedAtMs: 1700000000000 + i,
    durationMs: 1000 + i,
  }));

  const status = normalizeNotificationStatus({ items });
  assert.equal(status.items.length, 8, '最多保留 8 条');
  assert.equal(status.items[0].sessionId, 's0', '新的在前，顺序保持');
  assert.equal(status.items[7].sessionId, 's7');

  const blank = normalizeNotificationStatus({
    items: [{ sessionId: 'blank', title: '   ', cwd: 12, finishedAtMs: 'x', durationMs: -5 }],
  }).items[0];
  assert.equal(blank.title, '未命名会话', '空标题回退成未命名会话');
  assert.equal(blank.cwd, '');
  assert.equal(blank.finishedAtMs, 0);
  assert.equal(blank.durationMs, 0, '未知时长按 0，不显示负数');
});

test('refreshNotificationStatus 读取状态并写进 store', async () => {
  const { notificationStore, refreshNotificationStatus } = await import('../src/notifications.js');

  nextStatus = statusPayload({ unread: 3, enabled: false, platform: 'windows' });
  await refreshNotificationStatus();

  assert.equal(calls.at(-1).command, 'notification_status');
  assert.equal(notificationStore.unread, 3);
  assert.equal(notificationStore.enabled, false);
  assert.equal(notificationStore.platform, 'windows');
  assert.equal(notificationStore.watching, true, 'watching 决定设置页是否显示未连接提示');
});

test('保存开关把三个 camelCase 字段一起传给 notification_save_settings', async () => {
  const { notificationStore, refreshNotificationStatus, saveNotificationSettings } = await import(
    '../src/notifications.js'
  );

  nextStatus = statusPayload({ enabled: true, notifyAwayOnly: true, sound: true });
  await refreshNotificationStatus();

  // 只改一个开关时，另外两个字段必须带上当前值：Rust 侧是整份设置写入，
  // 缺字段会被当成默认值，把用户之前的开关冲掉。
  nextStatus = statusPayload({ enabled: true, notifyAwayOnly: false, sound: true });
  assert.equal(await saveNotificationSettings({ notifyAwayOnly: false }), true);
  assert.equal(calls.at(-1).command, 'notification_save_settings');
  assert.deepEqual(calls.at(-1).args, { enabled: true, notifyAwayOnly: false, sound: true });
  assert.equal(notificationStore.notifyAwayOnly, false, '保存后以 Rust 回显的状态为准');

  // 关掉总开关同样整份传参。
  nextStatus = statusPayload({ enabled: false, notifyAwayOnly: false, sound: true });
  assert.equal(await saveNotificationSettings({ enabled: false }), true);
  assert.deepEqual(calls.at(-1).args, { enabled: false, notifyAwayOnly: false, sound: true });
  assert.equal(notificationStore.enabled, false);
});

test('保存失败时开关回滚，不停在没生效的状态', async () => {
  const { notificationStore, refreshNotificationStatus, saveNotificationSettings } = await import(
    '../src/notifications.js'
  );

  nextStatus = statusPayload({ enabled: true, notifyAwayOnly: true, sound: true });
  await refreshNotificationStatus();

  failNext = { command: 'notification_save_settings', error: new Error('kernel not running') };
  assert.equal(await saveNotificationSettings({ sound: false }), false);
  assert.equal(notificationStore.sound, true, 'Rust 拒绝后必须回滚');
  assert.equal(notificationStore.enabled, true);
});

test('markNotificationsRead 后未读归零', async () => {
  const { notificationStore, refreshNotificationStatus, markNotificationsRead } = await import(
    '../src/notifications.js'
  );

  nextStatus = statusPayload({ unread: 5 });
  await refreshNotificationStatus();
  assert.equal(notificationStore.unread, 5);

  nextStatus = statusPayload({ unread: 0 });
  assert.equal(await markNotificationsRead(), true);
  assert.equal(calls.at(-1).command, 'notification_mark_read');
  assert.equal(notificationStore.unread, 0);

  // 载荷异常时也要把角标归零：用户点「全部已读」的意图是明确的。
  nextStatus = statusPayload({ unread: 4 });
  await refreshNotificationStatus();
  nextStatus = null;
  assert.equal(await markNotificationsRead(), true);
  assert.equal(notificationStore.unread, 0);
});

test('模拟任务完成：命令成功但 lastError 非空按失败处理', async () => {
  const { notificationStore, sendTestNotification } = await import('../src/notifications.js');

  nextStatus = statusPayload({
    lastError: '系统拒绝了通知权限，请在「系统设置 - 通知」里允许 dsh-xlink 后重试',
  });
  assert.equal(await sendTestNotification(), false);
  assert.equal(calls.at(-1).command, 'notification_test');
  assert.match(notificationStore.lastError, /通知权限/, '失败原因要留在 store 里供设置页展示');

  nextStatus = statusPayload({ lastError: null });
  assert.equal(await sendTestNotification(), true);
  assert.equal(notificationStore.lastError, null);
});

test('环境限制说明（未打包构建）被规范化成只读提示', async () => {
  const { notificationStore, refreshNotificationStatus } = await import('../src/notifications.js');

  nextStatus = statusPayload({ environmentNote: '  当前是未打包的开发构建  ' });
  await refreshNotificationStatus();
  assert.equal(notificationStore.environmentNote, '当前是未打包的开发构建', '应去掉首尾空白');

  nextStatus = statusPayload({ environmentNote: '' });
  await refreshNotificationStatus();
  assert.equal(notificationStore.environmentNote, null, '空串按"没有限制"处理');
});

test('读取失败时静默保留旧值，手动刷新才提示', async () => {
  const { notificationStore, refreshNotificationStatus } = await import('../src/notifications.js');

  nextStatus = statusPayload({ unread: 2, watching: true });
  await refreshNotificationStatus();

  failNext = { command: 'notification_status', error: new Error('bridge missing') };
  await refreshNotificationStatus();
  assert.equal(notificationStore.unread, 2, '自动刷新失败不得清空已读到的状态');

  failNext = { command: 'notification_status', error: new Error('bridge missing') };
  await refreshNotificationStatus(true);
  assert.equal(notificationStore.unread, 2, '手动刷新失败同样保留旧值');
});

test('设置页的 loading key 与模块内登记的一致', async () => {
  // key 写错时按钮永远不会转圈，而且没有任何报错——这类"看起来点了没反应"
  // 只能靠文本断言钉住（与 updateChecks.test.js 钉 notify.js 的做法一致）。
  const fs = await import('node:fs');
  const panel = fs.readFileSync('ui/src/components/SettingsPanel.vue', 'utf8');
  for (const key of [
    'notificationRefresh',
    'notificationMarkRead',
    'notificationTest',
    'notificationSave',
  ]) {
    assert.ok(panel.includes(`isLoading('${key}')`), `SettingsPanel 必须为 ${key} 绑定 loading`);
  }
});
