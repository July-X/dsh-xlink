// client.js 的加载回归测试：在桩环境里执行整份脚本并驱动 apply()。
//
// 背景（2026-10-09 用户实测）：apply 曾注册了不存在的标识符
// `ProviderCard`（组件实际叫 `AccountCard`），束在内核客户端求值即抛
// `ReferenceError`，插件的设置卡从未挂上、OAuth 入口「消失」，且每次
// 启动工作台都报前端 bundle 异常。host 侧纯函数测试罩不住这类错误——
// client.js 必须真跑一遍 apply 才能现形。
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import vm from 'node:vm';

const here = dirname(fileURLToPath(import.meta.url));
const source = readFileSync(join(here, '../client/client.js'), 'utf8');

/** 在桩环境里执行 client.js，返回注册体与脚本上下文（供取顶层 const）。 */
function loadClient() {
  let loaded = null;
  const context = {
    navigator: { language: 'zh' },
    __ModuleLoader_target: null,
  };
  context.window = {
    __ModuleLoader__: {
      load(definition) {
        loaded = definition;
      },
    },
  };
  vm.createContext(context);
  vm.runInContext(source, context, { filename: 'client.js' });
  assert.ok(loaded, 'client.js 必须调用 __ModuleLoader__.load 注册自己');
  assert.equal(loaded.id, 'xlink-openai-oauth');
  return { loaded, context };
}

/** 桩 React：createElement 只需要返回可断言的描述对象。 */
const stubReact = {
  createElement(type, props, ...children) {
    return { type, props, children };
  },
  useState(initial) {
    return [typeof initial === 'function' ? initial() : initial, () => {}];
  },
  useCallback(fn) {
    return fn;
  },
  useEffect() {},
};

test('client: factory + apply 求值不抛错，注册的插槽组件是已定义的函数', () => {
  const { loaded } = loadClient();
  const injected = [];
  const registered = [];
  const ctx = {
    slots: {
      // inject 的第二个参数是宿主稍后调用的注册回调——照真实时序调用它。
      inject(_name, factory) {
        injected.push(factory);
      },
      register(descriptor, component) {
        registered.push({ descriptor, component });
      },
    },
  };
  const exports = loaded.factory(() => stubReact);
  assert.equal(typeof exports.apply, 'function');
  exports.apply(ctx);
  assert.equal(injected.length, 1);
  // 卡片挂页尾 footer（list 插槽，注册 id = 插件 id）——provider 行卡是
  // 内核渲染的，自带无意义的「编辑」按钮（2026-10-09 用户反馈移除）。
  injected[0]();
  assert.equal(registered.length, 1);
  assert.equal(registered[0].descriptor.name, 'settings.models.footer');
  assert.equal(registered[0].descriptor.id, 'xlink-openai-oauth');
  assert.equal(
    typeof registered[0].component,
    'function',
    '注册进插槽的组件必须是已定义的函数（undefined 会把崩溃推迟到内核求值时）',
  );
});

test('client: 语言字典 zh/en 键集合一致（内联孪生不许漂移）', () => {
  const { context } = loadClient();
  const strings = vm.runInContext('STRINGS', context);
  const zh = Object.keys(strings.zh).sort();
  const en = Object.keys(strings.en).sort();
  assert.deepEqual(zh, en);
});

test('client: 已登录卡回读主窗口退出后的状态，并在卸载时停止轮询', async () => {
  const { loaded, context } = loadClient();
  const effects = [];
  let stateIndex = 0;
  let lastStatus;
  let interval;
  let cleared;
  context.setInterval = (callback, ms) => { interval = { callback, ms }; return 123; };
  context.clearInterval = (id) => { cleared = id; };
  context.window.__TAURI__ = { core: { invoke: async (command) => {
    assert.equal(command, 'openai_account_status');
    return { phase: 'signed-out' };
  } } };
  const react = { ...stubReact,
    useState(initial) {
      return stateIndex++ === 0
        ? [{ phase: 'authorized' }, (status) => { lastStatus = status; }]
        : [initial, () => {}];
    },
    useEffect(effect) { effects.push(effect); },
  };
  let footer;
  loaded.factory(() => react).apply({ slots: {
    inject(_name, register) { register(); },
    register(_descriptor, component) { footer = component; },
  } });
  const accountCard = footer().children[1].type;
  accountCard();
  const cleanup = effects[1]();
  assert.equal(interval.ms, 5000);
  await interval.callback();
  assert.equal(lastStatus.phase, 'signed-out');
  cleanup();
  assert.equal(cleared, 123);
});
