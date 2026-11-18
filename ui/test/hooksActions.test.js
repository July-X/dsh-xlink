import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { reactive, ref, computed, watch, nextTick, effectScope } from 'vue';

// 执行真实 store / 面板脚本，仅替换 IPC、通知及生命周期边界。
const source = readFileSync(new URL('../src/hooks/hooks.js', import.meta.url), 'utf8');
const panel = readFileSync(new URL('../src/hooks/HooksPanel.vue', import.meta.url), 'utf8').split('<script setup>')[1].split('</script>')[0];
const detail = readFileSync(new URL('../src/hooks/HookDetail.vue', import.meta.url), 'utf8').split('<script setup>')[1].split('</script>')[0];
function setup() {
  const calls = [];
  const handlers = {};
  const errors = [];
  const messages = [];
  const body = source.replace(/^import .*;\n/gm, '').replace(/^export /gm, '');
  const names = [...source.matchAll(/export (?:async )?(?:function|const) (\w+)/g)].map((m) => m[1]);
  const api = new Function('reactive', 'invoke', 'withLoading', 'toastActionError', 'toastSuccess', 'formatBytes', 'toast', body + '\nreturn {' + names.join(',') + '};')(
    reactive,
    async (cmd, args) => { calls.push({ cmd, args }); return handlers[cmd]?.(args); },
    (_, fn) => fn(), (...args) => errors.push(args), () => {}, String, (message) => messages.push(message)
  );
  return { ...api, calls, handlers, errors, messages };
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
  const actions = new Function(...Object.keys(scope), body + '\nreturn { removeEndpoint, rotateToken, rotateNotification, submitCreate, sourceType, draftName, saveNetwork, networkDraft, subnetsDraft, portDraft, settingsDirty, toggleService, restoreSavedSettings, createdForJenkins, guideEndpoint, guideAddress, openJenkinsGuide, closeJenkinsGuide, closeCreated, flowSteps, selectTab };')(...Object.values(scope));
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

test('删除结束只关闭仍在显示的被删事件', async () => {
  const h = setup();
  const closed = [];
  const scope = { ...h, computed, formatBytes: String, confirmDialog: async () => true,
    defineProps: () => ({}), defineEmits: () => (...args) => closed.push(args) };
  const body = detail.replace(/^import [\s\S]*?;\n/gm, '');
  const remove = new Function(...Object.keys(scope), body + '\nreturn remove;')(...Object.values(scope));
  const pending = deferred();
  h.handlers.hooks_delete_event = () => pending.promise;
  h.handlers.hooks_event_detail = ({ eventId }) => ({ event: { id: eventId } });
  h.handlers.hooks_inbox_list = () => ({ items: [], total: 0 });

  await h.openEvent('first');
  const deleting = remove();
  await Promise.resolve();
  h.closeDetail();
  await h.openEvent('second');
  pending.resolve();
  await deleting;
  assert.deepEqual(closed, []);
  assert.equal(h.hookStore.detail.event.id, 'second');

  await remove();
  assert.deepEqual(closed, [['update:modelValue', false]]);
  assert.equal(h.hookStore.detail, null);
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


test('后台新消息不挪动正在查看的行，手动更新后清除提示', async () => {
  const h = setup();
  h.hookStore.inbox.items = [{ id: 'old' }];
  h.hookStore.inbox.total = 1;
  h.handlers.hooks_inbox_list = () => ({ total: 2, items: [{ id: 'new' }, { id: 'old' }] });
  await h.pollInbox();
  assert.equal(h.hookStore.inbox.items[0].id, 'old');
  assert.equal(h.hookStore.inbox.pendingUpdates, true);
  await h.loadInbox();
  assert.equal(h.hookStore.inbox.items[0].id, 'new');
  assert.equal(h.hookStore.inbox.pendingUpdates, false);
});

test('空收件箱自动出现首条测试，迟到轮询不能覆盖新筛选或离页状态', async () => {
  const h = setup();
  h.handlers.hooks_inbox_list = () => ({ total: 1, items: [{ id: 'test' }] });
  await h.pollInbox();
  assert.equal(h.hookStore.inbox.items[0].id, 'test');
  const late = deferred();
  h.handlers.hooks_inbox_list = () => late.promise;
  const pending = h.pollInbox();
  h.cancelInboxRequests();
  late.resolve({ total: 1, items: [{ id: 'late' }] });
  await pending;
  assert.equal(h.hookStore.inbox.items[0].id, 'test');
  assert.equal(h.hookStore.inbox.pendingUpdates, false);
});

test('轮询期间改变筛选文字，不应用旧结果', async () => {
  const h = setup(); const late = deferred();
  h.handlers.hooks_inbox_list = () => late.promise;
  const pending = h.pollInbox();
  h.hookStore.inbox.filter.q = 'new';
  late.resolve({ total: 1, items: [{ id: 'old' }] });
  await pending;
  assert.equal(h.hookStore.inbox.items.length, 0);
});

test('新消息提示在变化消失后自动清掉', async () => {
  const h = setup();
  h.hookStore.inbox.items = [{ id: 'old' }];
  h.hookStore.inbox.total = 1;
  h.hookStore.inbox.pendingUpdates = true;
  h.handlers.hooks_inbox_list = () => ({ total: 1, items: [{ id: 'old' }] });
  await h.pollInbox();
  assert.equal(h.hookStore.inbox.pendingUpdates, false);
});

test('保存传递明确网络参数，旧状态响应不能覆盖保存结果', async () => {
  const h = setup(); const late = deferred();
  h.handlers.hooks_status = () => late.promise;
  const pending = h.refreshHooksStatus();
  h.handlers.hooks_save_config = () => ({ running: true, config: { bind_ip: '10.1.2.3' } });
  await h.saveHooksConfig(true, 17890, { interfaceName: 'Ethernet', bindIp: '10.1.2.3', netmask: '255.255.255.0', allowedSubnets: ['10.2.0.0/16'] });
  late.resolve({ running: false }); await pending;
  assert.equal(h.hookStore.status.running, true);
  assert.deepEqual(h.calls.find(c => c.cmd === 'hooks_save_config').args, { enabled: true, port: 17890, interfaceName: 'Ethernet', bindIp: '10.1.2.3', netmask: '255.255.255.0', allowedSubnets: ['10.2.0.0/16'] });
});

test('测试命令使用实际事件入口与协议版本，不包含一次性令牌', () => {
  const h = setup(); const endpoint = { id: 'ep_1' };
  h.hookStore.created = { endpoint, token: 'secret-token' };
  h.hookStore.status = { running: true, lan_ip: '10.1.2.3', config: { port: 17890 } };
  const command = h.testCommand(endpoint);
  assert.match(command, /schema_version = '1.0'/);
  assert.match(command, /commit = \$true/);
  assert.match(command, /Read-Host 'Hook token' -AsSecureString/);
  assert.match(command, /http:\/\/10.1.2.3:17890\/hooks\/v1\/endpoints\/ep_1\/events/);
  assert.match(command, /-NoProxy|DefaultWebProxy/);
  assert.doesNotMatch(command, /secret-token/);
  const bash = h.testCommand(endpoint, 'bash');
  assert.match(bash, /"schema_version":"1.0"/);
  assert.match(bash, /--noproxy '\*'/);
  assert.match(bash, /--fail /);
  h.hookStore.status.running = false;
  assert.equal(h.testCommand(endpoint), '');
});

test('旧的接入点列表响应不能覆盖轮换后的通知凭据状态', async () => {
  const h = setup();
  const late = deferred();
  h.hookStore.endpoints = [{ id: 'ep_1', notification_enabled: false }];
  h.handlers.hooks_list_endpoints = () => late.promise;
  const pending = h.loadEndpoints();
  h.handlers.hooks_rotate_endpoint_notification_token = () => ({ endpoint: { id: 'ep_1', notification_enabled: true }, token: 'basic' });
  await h.rotateNotificationToken('ep_1');
  assert.equal(h.hookStore.endpoints[0].notification_enabled, true);
  late.resolve([{ id: 'ep_1', notification_enabled: false }]);
  await pending;
  assert.equal(h.hookStore.endpoints[0].notification_enabled, true);
});


test('Jenkins 模板保留一次性 Bearer 并直接显示 Groovy 指导，不生成旧 Basic', async () => {
  const h = setup();
  h.handlers.hooks_create_endpoint = () => ({ endpoint: { id: 'ep', name: 'CI' }, token: 'bearer' });
  h.handlers.hooks_rotate_endpoint_notification_token = () => ({ endpoint: { id: 'ep', notification_enabled: true }, token: 'basic' });
  const panel = loadPanel(h);
  panel.draftName.value = 'CI'; panel.sourceType.value = 'jenkins';
  await panel.submitCreate();
  assert.equal(h.hookStore.created.token, 'bearer');
  assert.equal(panel.createdForJenkins.value, true);
  assert.equal(h.hookStore.notification, null);
  assert.equal(h.calls.some(c => c.cmd === 'hooks_rotate_endpoint_notification_token'), false);
  panel.closeCreated();
  assert.equal(h.hookStore.created, null);
  assert.equal(panel.createdForJenkins.value, false);
  assert.deepEqual(h.calls.find(c => c.cmd === 'hooks_create_endpoint').args.allowedKinds, ['text', 'parameters']);
});

test('创建完成前离页不再自动生成 Jenkins 凭据', async () => {
  const h = setup(); const late = deferred();
  h.handlers.hooks_create_endpoint = () => late.promise;
  const panel = loadPanel(h);
  panel.draftName.value = 'CI'; panel.sourceType.value = 'jenkins';
  const pending = panel.submitCreate(); panel.unmount();
  late.resolve({ endpoint: { id: 'ep' }, token: 'secret' });
  await pending;
  assert.equal(h.calls.some(c => c.cmd === 'hooks_rotate_endpoint_notification_token'), false);
  assert.equal(h.hookStore.created, null);
});

test('端口草稿在输入时即标脏，避免五秒状态刷新覆盖', () => {
  const view = readFileSync(new URL('../src/hooks/HooksPanel.vue', import.meta.url), 'utf8');
  assert.match(
    view,
    /<el-input-number v-model="portDraft"[^>]*@update:model-value="settingsDirty = true"/,
    '端口输入必须在输入阶段标脏，不能等失焦的 @change'
  );
});

test('没有明确选择网络不能开启服务，保存选中的真实地址和额外网段', async () => {
  const h = setup();
  const network = { name: 'eth0', address: '10.0.0.10', netmask: '255.255.255.0', cidr: '10.0.0.0/24' };
  h.hookStore.status = { config: { port: 17890 }, interfaces: [network] };
  h.handlers.hooks_save_config = () => h.hookStore.status;
  const panel = loadPanel(h); panel.portDraft.value = 17890;
  await panel.saveNetwork(true);
  assert.equal(h.calls.length, 0);
  panel.networkDraft.value = JSON.stringify([network.name, network.address, network.netmask]);
  panel.subnetsDraft.value = '10.20.0.0/16\n172.16.1.0/24';
  await panel.saveNetwork(true);
  assert.deepEqual(h.calls[0].args.allowedSubnets, ['10.20.0.0/16', '172.16.1.0/24']);
  assert.equal(h.calls[0].args.bindIp, network.address);
});

test('复制的 Bash 命令能生成有效真实协议 JSON，无网络调用', { skip: process.platform === 'win32' }, () => {
  const h = setup();
  h.hookStore.status = { running: true, lan_ip: '10.0.0.10', config: { port: 17890 } };
  const result = spawnSync('bash', ['-c', 'curl() { cat; }; ' + h.testCommand({ id: 'ep' }, 'bash')], { input: 'test-secret\n', encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
  const body = JSON.parse(result.stdout.trim());
  assert.equal(body.schema_version, '1.0');
  assert.equal(body.commit, true);
  assert.equal(body.text, 'LAN connection test');
  assert.ok(Number.isFinite(Date.parse(body.occurred_at)));
  assert.doesNotMatch(result.stdout, /test-secret/);
});


test('来源模板使用已注册的 Element Plus 单选组件', () => {
  const view = readFileSync(new URL('../src/hooks/HooksPanel.vue', import.meta.url), 'utf8');
  const main = readFileSync(new URL('../src/main.js', import.meta.url), 'utf8');
  assert.match(view, /<el-radio value="generic">通用脚本<\/el-radio>/);
  assert.match(view, /<el-radio value="jenkins">Jenkins<\/el-radio>/);
  assert.doesNotMatch(view, /<el-radio-button/);
  assert.match(main, /import \{ ElRadio, ElRadioGroup \}/);
  assert.match(main, /  ElRadio,\n  ElRadioGroup,/);
});


test('无效未保存草稿不阻止停服，也不随停止写入；真实 watcher 保留草稿直至明确恢复', async () => {
  const h = setup();
  const saved = { enabled: true, port: 17890, interface_name: 'eth0', bind_ip: '10.1.2.3', netmask: '255.255.255.0', allowed_subnets: ['10.2.0.0/16'] };
  h.hookStore.status = { running: true, config: saved, interfaces: [] };
  h.handlers.hooks_save_config = ({ enabled, port }) => ({ running: false, config: { ...saved, enabled, port }, interfaces: [] });
  h.handlers.hooks_status = () => h.hookStore.status;
  const scope = effectScope();
  const panel = scope.run(() => loadPanel(h, { watch }));
  try {
    assert.equal(panel.portDraft.value, saved.port);
    panel.portDraft.value = 0;
    panel.networkDraft.value = 'offline';
    panel.subnetsDraft.value = '10.20.0.0/33';
    panel.settingsDirty.value = true;
    await panel.toggleService(false);
    await nextTick();
    assert.deepEqual(h.calls.find(c => c.cmd === 'hooks_save_config').args, { enabled: false, port: saved.port, disableOnly: true });
    assert.equal(h.hookStore.status.running, false);
    assert.deepEqual(h.hookStore.status.config.allowed_subnets, saved.allowed_subnets);
    assert.equal(panel.portDraft.value, 0);
    assert.equal(panel.subnetsDraft.value, '10.20.0.0/33');
    assert.equal(panel.networkDraft.value, 'offline');
    assert.equal(panel.settingsDirty.value, true);
    // 模拟后台轮询拿到新的对象：脏草稿仍不被覆盖。
    h.hookStore.status = { ...h.hookStore.status, config: { ...h.hookStore.status.config } };
    await nextTick();
    assert.equal(panel.subnetsDraft.value, '10.20.0.0/33');
    panel.restoreSavedSettings();
    assert.equal(panel.portDraft.value, saved.port);
    assert.equal(panel.subnetsDraft.value, '10.2.0.0/16');
    assert.equal(panel.networkDraft.value, JSON.stringify(['eth0', '10.1.2.3', '255.255.255.0']));
    assert.equal(panel.settingsDirty.value, false);
  } finally { panel.unmount(); scope.stop(); }
});

test('停服请求失败以刷新后的状态为准，仍保留无效草稿', async () => {
  const h = setup();
  const status = { running: true, config: { enabled: true, port: 17890, allowed_subnets: [] }, interfaces: [] };
  h.hookStore.status = status;
  h.handlers.hooks_save_config = () => { throw Error('write failed'); };
  h.handlers.hooks_status = () => status;
  const scope = effectScope();
  const panel = scope.run(() => loadPanel(h, { watch }));
  try {
    panel.subnetsDraft.value = 'invalid'; panel.settingsDirty.value = true;
    assert.equal(await panel.toggleService(false), false);
    await nextTick();
    assert.equal(h.hookStore.status.running, true);
    assert.equal(panel.subnetsDraft.value, 'invalid');
    assert.equal(panel.settingsDirty.value, true);
    assert.equal(h.errors.length, 1);
  } finally { panel.unmount(); scope.stop(); }
});

test('启用仍校验草稿，恢复按钮接线且保存期间不能修改草稿', async () => {
  const h = setup();
  h.hookStore.status = { config: { enabled: false, port: 17890 }, interfaces: [] };
  const panel = loadPanel(h);
  panel.portDraft.value = 0;
  await panel.toggleService(true);
  assert.equal(h.calls.length, 0);
  const view = readFileSync(new URL('../src/hooks/HooksPanel.vue', import.meta.url), 'utf8');
  assert.match(view, /@change="toggleService"/);
  assert.match(view, /@click="restoreSavedSettings\(\)"/);
  assert.match(view, /v-if="settingsDirty"/);
  assert.match(view, /v-model="subnetsDraft" :disabled="isLoading\('hooksSaveConfig'\)"/);
});


test('停止监听但配置持久化失败时显示后端停止状态，不回填未保存网段', async () => {
  const h = setup();
  h.hookStore.status = { running: true, config: { enabled: true, port: 17890, allowed_subnets: [] }, interfaces: [] };
  h.handlers.hooks_save_config = () => { throw Error('持久化失败，接收器已停止'); };
  h.handlers.hooks_status = () => ({ ...h.hookStore.status, running: false, last_error: '持久化失败，接收器已停止' });
  const scope = effectScope();
  const panel = scope.run(() => loadPanel(h, { watch }));
  try {
    panel.subnetsDraft.value = '10.20.0.0/33'; panel.settingsDirty.value = true;
    assert.equal(await panel.toggleService(false), false);
    await nextTick();
    assert.equal(h.hookStore.status.running, false);
    assert.match(h.hookStore.status.last_error, /已停止/);
    assert.match(String(h.errors[0][1]), /持久化失败/);
    assert.match(h.errors[0][2], /重试停用/);
    assert.equal(panel.subnetsDraft.value, '10.20.0.0/33');
    assert.equal(panel.settingsDirty.value, true);
    assert.deepEqual(h.hookStore.status.config.allowed_subnets, []);
  } finally { panel.unmount(); scope.stop(); }
});


test('服务关闭时保存按钮仍保存网络草稿，不与仅停止操作混淆', async () => {
  const h = setup();
  const network = { name: 'eth0', address: '10.0.0.10', netmask: '255.255.255.0', cidr: '10.0.0.0/24' };
  h.hookStore.status = { running: false, config: { enabled: false, port: 17890, allowed_subnets: [] }, interfaces: [network] };
  h.handlers.hooks_save_config = () => h.hookStore.status;
  const panel = loadPanel(h);
  panel.portDraft.value = 17891;
  panel.networkDraft.value = JSON.stringify([network.name, network.address, network.netmask]);
  panel.subnetsDraft.value = '10.20.0.0/16'; panel.settingsDirty.value = true;
  await panel.saveNetwork();
  const args = h.calls.find(c => c.cmd === 'hooks_save_config').args;
  assert.equal(args.enabled, false);
  assert.equal(args.port, 17891);
  assert.equal(args.bindIp, network.address);
  assert.deepEqual(args.allowedSubnets, ['10.20.0.0/16']);
  assert.equal(args.disableOnly, undefined);
  assert.equal(panel.settingsDirty.value, false);
});


test('既有来源指导反复打开与关闭，只保留非秘密身份且地址跟随当前网络', () => {
  const h = setup(); const view = loadPanel(h);
  h.hookStore.status = { running: true, lan_ip: '10.1.2.3', config: { bind_ip: '10.1.2.3', port: 17890 } };
  view.openJenkinsGuide({ id: 'ep', name: 'CI', token: 'must-not-copy' });
  assert.deepEqual(view.guideEndpoint.value, { id: 'ep', name: 'CI' });
  assert.match(view.guideAddress.value, /10\.1\.2\.3/);
  view.closeJenkinsGuide(); assert.equal(view.guideEndpoint.value, null);
  view.openJenkinsGuide({ id: 'ep2', name: 'Other' });
  assert.match(view.guideAddress.value, /ep2$/);
  h.hookStore.status.lan_ip = '10.2.3.4';
  assert.match(view.guideAddress.value, /10\.2\.3\.4/);
});

test('事件详情标题区给出投递方 IP，旧事件如实标未记录', () => {
  const view = readFileSync(new URL('../src/hooks/HookDetail.vue', import.meta.url), 'utf8');
  assert.match(view, /hook-detail-source/);
  assert.match(view, /event\.source_ip \|\| '未记录'/);
});

test('接入流程按真实状态推导三步进度：当前步可点，末步只作指引', () => {
  const h = setup();
  h.hookStore.status = { running: false, config: { enabled: false }, interfaces: [] };
  h.hookStore.endpoints = [];
  const view = loadPanel(h);
  assert.deepEqual(view.flowSteps.value.map(step => step.state), ['current', 'pending', 'pending']);
  assert.equal(typeof view.flowSteps.value[0].action, 'function');
  assert.equal(view.flowSteps.value[2].action, null);
  h.hookStore.status = { running: true, lan_ip: '10.1.2.3', config: { enabled: true } };
  h.hookStore.endpoints = [{ id: 'ep', last_event_at: null }];
  assert.deepEqual(view.flowSteps.value.map(step => step.state), ['done', 'done', 'current']);
  h.hookStore.endpoints = [{ id: 'ep', last_event_at: 1_700_000_000_000 }];
  assert.deepEqual(view.flowSteps.value.map(step => step.state), ['done', 'done', 'done']);
});

test('新投递提示不受筛选和翻页影响，首次加载不误报，同一投递不重复提醒', async () => {
  const h = setup();
  let latest = { id: 'old', received_at: 100 };
  h.handlers.hooks_inbox_list = () => ({ items: [latest], total: 1 });
  h.hookStore.inbox.filter.q = 'not-matching';
  h.hookStore.inbox.offset = 40;
  await h.pollHookNotifications();
  assert.equal(h.messages.length, 0);
  latest = { id: 'new', received_at: 200 };
  await h.pollHookNotifications();
  await h.pollHookNotifications();
  assert.equal(h.messages.length, 1);
  assert.equal(h.hookStore.hasNewMessages, true);
  assert.deepEqual(h.calls.at(-1).args.filter, { limit: 1, offset: 0 });
  assert.equal(h.hookStore.inbox.offset, 40);
  assert.deepEqual(h.hookStore.inbox.items, []);
  latest = { id: 'older-after-delete', received_at: 90 };
  await h.pollHookNotifications();
  assert.equal(h.messages.length, 1, '删除或容量清理后露出的旧事件不应提醒');
});

test('空收件箱的首条投递也提醒，同毫秒不同投递能提醒，刷新成功才清除标记', async () => {
  const h = setup();
  let items = [];
  h.handlers.hooks_inbox_list = () => ({ items, total: items.length });
  await h.pollHookNotifications();
  items = [{ id: 'a', received_at: 100 }];
  await h.pollHookNotifications();
  items = [{ id: 'b', received_at: 100 }];
  await h.pollHookNotifications();
  assert.equal(h.messages.length, 2);
  h.handlers.hooks_inbox_list = () => { throw new Error('offline'); };
  await h.loadInbox();
  assert.equal(h.hookStore.hasNewMessages, true);
  h.handlers.hooks_inbox_list = () => ({ items, total: 1 });
  await h.loadInbox();
  assert.equal(h.hookStore.hasNewMessages, false);
});

test('通知轮询失败保留基线，离页后的迟到响应不弹提示，重进页面不补报旧投递', async () => {
  const h = setup();
  h.handlers.hooks_inbox_list = () => ({ items: [] });
  await h.pollHookNotifications();
  h.handlers.hooks_inbox_list = () => { throw new Error('offline'); };
  await h.pollHookNotifications();
  const late = deferred();
  h.handlers.hooks_inbox_list = () => late.promise;
  const pending = h.pollHookNotifications();
  h.cancelHookNotifications();
  late.resolve({ items: [{ id: 'late', received_at: 100 }] });
  await pending;
  assert.deepEqual(h.messages, []);
  h.handlers.hooks_inbox_list = () => ({ items: [{ id: 'late', received_at: 100 }] });
  await h.pollHookNotifications();
  assert.deepEqual(h.messages, []);
});

test('Hook 页在设置页签仍轮询通知，隐藏时暂停，离页时清理', async () => {
  const h = setup();
  let latest = { id: 'old', received_at: 100 };
  h.handlers.hooks_inbox_list = () => ({ items: [latest], total: 1 });
  h.handlers.hooks_status = () => ({ running: true });
  h.handlers.hooks_list_endpoints = () => [];
  const mounts = [];
  const timers = [];
  const cleared = [];
  const document = { hidden: false };
  const view = loadPanel(h, {
    onMounted(fn) { mounts.push(fn); },
    setInterval(fn, ms) { assert.equal(ms, 5000); timers.push(fn); return 'hook-poll'; },
    clearInterval(id) { cleared.push(id); },
    document,
  });
  mounts.forEach(fn => fn());
  view.selectTab('settings');
  await Promise.resolve();
  latest = { id: 'new', received_at: 200 };
  await timers[0]();
  assert.equal(h.hookStore.hasNewMessages, true);
  assert.equal(h.messages.length, 1);
  const before = h.calls.length;
  document.hidden = true;
  await timers[0]();
  assert.equal(h.calls.length, before);
  view.unmount();
  assert.deepEqual(cleared, ['hook-poll']);
});

test('Hook 三个页签均带装饰图标，新消息标记可读，网卡选项有独立换行样式', () => {
  const view = readFileSync(new URL('../src/hooks/HooksPanel.vue', import.meta.url), 'utf8');
  const template = view.split('<template>')[1].split('</template>')[0].replace(/<!--[\s\S]*?-->/g, '');
  const tabs = template.match(/<div class="hook-tabs"[\s\S]*?<\/div>/)[0];
  assert.equal((tabs.match(/Icon :size="16" aria-hidden="true"/g) || []).length, 3);
  assert.match(tabs, /v-if="hookStore.hasNewMessages"[^>]*role="status">新消息/);
  assert.match(template, /popper-class="hook-network-options"/);
  assert.match(template, /class="hook-network-name"/);
  assert.match(template, /class="hook-network-address"/);
  const styles = view.split('<style scoped>')[1];
  assert.match(styles, /:global\(\.hook-network-options \.el-select-dropdown__item\)[^{]*\{[^}]*height: auto;[^}]*white-space: normal;[^}]*overflow-wrap: anywhere;/);
  assert.match(styles, /is-hovering\)\s*\{ background: var\(--surface-subtle\)/);
  assert.match(styles, /is-selected\)\s*\{ background: var\(--accent-soft\)/);
});
