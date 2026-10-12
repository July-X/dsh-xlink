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

test('主窗口按钮在内置插件行，和内核共用相位命令与状态查询', () => {
  const panel = readFileSync('ui/src/plugins/PluginsPanel.vue', 'utf8');
  const component = readFileSync('ui/src/plugins/OpenaiAccountActions.vue', 'utf8');
  // 2026-10-12：内置插件行改成 v-for（不止 openai-oauth 一个），传的是**循环
  // 变量**。这里断言传的不是某个具体内置插件的视图——写成 `builtinView` 时
  // 这条判据会红，而那时面板里那个变量已经不存在了，症状是账号按钮整条不见。
  assert.match(panel, /<OpenaiAccountActions[^>]*:plugin-view="builtin"/);
  // 而且它必须**只挂在有账户卡的那个插件上**（后端下发的 hasAccountCard）：
  // 无条件渲染的话，ponytail 那一行会带出一组毫无意义的 ChatGPT 按钮。
  assert.match(panel, /v-if="builtin\.hasAccountCard"/);
  for (const command of ['openai_authorize_start', 'openai_authorize_cancel', 'openai_logout', 'openai_catalog_refresh']) {
    assert.ok(component.includes(command));
  }
  assert.match(component, /reauth-required/);
  assert.match(component, /isLoading\('openaiAccountAction'\)/);
  const client = readFileSync('plugins/openai-oauth/client/client.js', 'utf8');
  // 授权中 2s，其余 60s。60 不是随手取的：每次 refresh 都要经壳读一次系统
  // 凭据库，而那要 spawn 一个 `security` / PowerShell 子进程，固定 5s 等于
  // 页签常开一天一万七千多次进程创建。另一条 UI 侧 store 用的是同一个口径。
  assert.match(client, /setInterval\(refresh, authorizing \? 2000 : 60000\)/);
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

test('轮询间隔随相位走：稳态 60s，进入授权中立刻重排成 5s', async () => {
  // 不靠真实计时器：把 setTimeout 换掉，直接读「下一次排了多少毫秒」。
  const delays = [];
  const realSetTimeout = globalThis.setTimeout;
  const realClearTimeout = globalThis.clearTimeout;
  let cleared = 0;
  globalThis.setTimeout = (fn, ms) => { delays.push(ms); return { __fake: delays.length }; };
  globalThis.clearTimeout = () => { ++cleared; };
  listen = () => Promise.resolve(() => {});
  invoke = async () => ({ phase: 'authorized' });
  accountStore.status = { phase: 'authorized' };

  const stop = followAccount();
  try {
    await new Promise((resolve) => setImmediate(resolve));
    await new Promise((resolve) => setImmediate(resolve));
    assert.equal(delays.at(-1), 60_000, '稳态走长档——每次轮询都要 spawn 一次凭据库子进程');

    // 用户点下登录：相位变成 authorizing。
    accountStore.status = { phase: 'authorizing' };
    await new Promise((resolve) => setImmediate(resolve));
    await new Promise((resolve) => setImmediate(resolve));
    assert.ok(cleared > 0, '相位变化必须作废旧定时器，否则两个定时器并存 = 频率翻倍');
    assert.equal(
      delays.at(-1), 5000,
      '进入授权中必须**立刻**重排成 5s：只靠定时器到期再读相位的话，'
      + '授权进度会等到 60s 才更新一次，比降频前还慢',
    );
  } finally {
    stop();
    globalThis.setTimeout = realSetTimeout;
    globalThis.clearTimeout = realClearTimeout;
  }
});

test('面板在授权进行中被打开时，首轮就用短间隔', async () => {
  // 与上一条不同的场景：挂载时首读就返回 authorizing。此时 `accountStore.status`
  // 还是 null（refresh 是异步的），同步调用的 schedule() 只能排到 60s；必须靠
  // 首读落地后的相位变更把它拉回 5s，否则用户最长要等一分钟才看到授权进度。
  const delays = [];
  const realSetTimeout = globalThis.setTimeout;
  const realClearTimeout = globalThis.clearTimeout;
  globalThis.setTimeout = (fn, ms) => { delays.push(ms); return { __fake: delays.length }; };
  globalThis.clearTimeout = () => {};
  listen = () => Promise.resolve(() => {});
  invoke = async () => ({ phase: 'authorizing' });
  accountStore.status = null;

  const stop = followAccount();
  try {
    await new Promise((resolve) => setImmediate(resolve));
    await new Promise((resolve) => setImmediate(resolve));
    await new Promise((resolve) => setImmediate(resolve));
    assert.equal(delays[0], 60_000, '前置：首读没落地前只能按未知相位排长档');
    assert.equal(
      delays.at(-1), 5000,
      '首读落地为 authorizing 后必须重排成 5s，不能停在 60s',
    );
  } finally {
    stop();
    globalThis.setTimeout = realSetTimeout;
    globalThis.clearTimeout = realClearTimeout;
  }
});
