import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
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
  const actions = new Function(...Object.keys(scope), body + '\nreturn { removeEndpoint, rotateToken, rotateNotification, submitCreate, sourceType, draftName, saveNetwork, networkDraft, subnetsDraft, portDraft };')(...Object.values(scope));
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
  assert.doesNotMatch(command, /secret-token/);
  assert.match(h.testCommand(endpoint, 'bash'), /"schema_version":"1.0"/);
  h.hookStore.status.running = false;
  assert.equal(h.testCommand(endpoint), '');
});


test('Jenkins 模板一次创建来源并显示独立通知凭据，隐藏 Bearer', async () => {
  const h = setup();
  h.handlers.hooks_create_endpoint = () => ({ endpoint: { id: 'ep', name: 'CI' }, token: 'bearer' });
  h.handlers.hooks_rotate_endpoint_notification_token = () => ({ endpoint: { id: 'ep', notification_enabled: true }, token: 'basic' });
  const panel = loadPanel(h);
  panel.draftName.value = 'CI'; panel.sourceType.value = 'jenkins';
  await panel.submitCreate();
  assert.equal(h.hookStore.created, null);
  assert.equal(h.hookStore.notification.token, 'basic');
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
