import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import * as Vue from 'vue';

const require = createRequire(import.meta.url);
const { parse } = require(require.resolve('@vue/compiler-sfc', { paths: [require.resolve('vue')] }));
const { compile } = require(require.resolve('@vue/compiler-dom', { paths: [require.resolve('vue')] }));
const { renderToString } = require(require.resolve('@vue/server-renderer', { paths: [require.resolve('vue')] }));

let invoke = async () => ({ phase: 'signed-out', email: null });
let listen = async () => () => {};
globalThis.window = { __TAURI__: {
  core: { invoke: (...args) => invoke(...args) }, event: { listen: (...args) => listen(...args) },
} };
globalThis.document = { createElement: () => ({}), body: { classList: { toggle() {} } } };
const { accountStore, refreshAccount, runAccountAction, accountText, followAccount } = await import('../src/plugins/openaiAccount.js');

test('账号动作复用内核命令，退出失败不能假报未登录', async () => {
  const calls = [];
  invoke = async (command) => {
    calls.push(command);
    if (command === 'openai_logout') throw new Error('vault unavailable');
    return { phase: 'authorized', email: 'z***@example.com' };
  };
  await refreshAccount();
  assert.equal(accountText(), '已登录 · z***@example.com');
  await runAccountAction('openai_logout');
  assert.equal(accountStore.status.phase, 'authorized');
  assert.match(accountStore.error, /vault unavailable/);
  assert.ok(calls.includes('openai_logout'));
  assert.ok(calls.every((cmd) => ['openai_logout', 'openai_account_status'].includes(cmd)));
});

test('在途旧状态不能覆盖登录动作返回的新状态', async () => {
  let finish;
  invoke = (command) => command === 'openai_account_status'
    ? new Promise((resolve) => { finish = resolve; })
    : Promise.resolve({ phase: 'authorizing' });
  const old = refreshAccount();
  await runAccountAction('openai_authorize_start');
  finish({ phase: 'signed-out' });
  await old;
  assert.equal(accountStore.status.phase, 'authorizing');
  assert.match(accountText(), /登录进行中/);
});

test('状态读取失败保留原状态并给出错误，不伪装成未登录', async () => {
  invoke = async () => { throw new Error('bridge denied'); };
  await refreshAccount();
  assert.equal(accountStore.status.phase, 'authorizing');
  assert.match(accountStore.readError, /bridge denied/);
});

test('主窗口按钮在内嵌插件行，和内核共用相位命令与状态查询', () => {
  const panel = readFileSync('ui/src/plugins/PluginsPanel.vue', 'utf8');
  const component = readFileSync('ui/src/plugins/OpenaiAccountActions.vue', 'utf8');
  assert.match(panel, /<OpenaiAccountActions[^>]*:plugin-view="builtinView"/);
  for (const command of ['openai_authorize_start', 'openai_authorize_cancel', 'openai_logout', 'openai_catalog_refresh']) {
    assert.ok(component.includes(command));
  }
  assert.match(component, /reauth-required/);
  assert.match(component, /isLoading\('openaiAccountAction'\)/);
  const client = readFileSync('plugins/openai-oauth/client/client.js', 'utf8');
  assert.match(client, /setInterval\(refresh, authorizing \? 2000 : 5000\)/);
});

test('真实账号组件模板按四种相位显示按钮，不显示重复登录入口', async () => {
  const { descriptor } = parse(readFileSync('ui/src/plugins/OpenaiAccountActions.vue', 'utf8'));
  const { code } = compile(descriptor.template.content, { mode: 'function', prefixIdentifiers: true, isCustomElement: () => true });
  const render = new Function('Vue', code)(Vue);
  for (const [phase, labels] of [
    ['signed-out', ['登录 ChatGPT']],
    ['authorizing', ['取消登录']],
    ['authorized', ['刷新模型列表', '退出登录']],
    ['reauth-required', ['登录 ChatGPT', '退出登录']],
  ]) {
    const html = await renderToString(Vue.createSSRApp({ render, data: () => ({
      phase, busy: false, accountStore: { status: { phase }, error: '' },
      pluginView: { pluginSourceAvailable: true }, accountText: () => phase,
      isLoading: () => false, refreshAccount() {}, runAccountAction() {},
    }) }));
    for (const label of ['登录 ChatGPT', '取消登录', '刷新模型列表', '退出登录']) {
      assert.equal(html.includes(label), labels.includes(label), `${phase}: ${label}`);
    }
  }
});

test('工作台账号事件触发权威回读；卸载后晚到的监听和旧查询均被清理', async () => {
  let handler;
  let offCount = 0;
  let register;
  let finish;
  listen = (event, callback) => {
    assert.equal(event, 'openai-account-changed');
    handler = callback;
    return new Promise((resolve) => { register = resolve; });
  };
  invoke = async () => ({ phase: 'authorized', email: 'm***@example.com' });
  const stop = followAccount();
  try {
    await new Promise((resolve) => setImmediate(resolve));
    assert.equal(accountStore.status.phase, 'authorized');
    invoke = async () => ({ phase: 'signed-out' });
    await handler({ payload: { phase: 'authorized' } });
    assert.equal(accountStore.status.phase, 'signed-out', '事件负载不能冒充查询结果');
    invoke = () => new Promise((resolve) => { finish = resolve; });
    const pending = handler();
    stop();
    register(() => { ++offCount; });
    finish({ phase: 'authorized' });
    await pending;
    await new Promise((resolve) => setImmediate(resolve));
    assert.equal(offCount, 1);
    assert.equal(accountStore.status.phase, 'signed-out');
  } finally {
    stop();
  }
});
