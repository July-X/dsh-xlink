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
  // 非授权相位是 60s 而非 5s：每次轮询都要经壳读一次系统凭据库，而那要 spawn
  // 一个 `security` / PowerShell 子进程；页签常开一天固定 5s 意味着一万七
  // 千多次进程创建（与 ui/src/plugins/openaiAccount.js 同一口径）。
  assert.equal(interval.ms, 60000);
  await interval.callback();
  assert.equal(lastStatus.phase, 'signed-out');
  cleanup();
  assert.equal(cleared, 123);
});

test('client: 授权进行中仍用短间隔轮询', async () => {
  const { loaded, context } = loadClient();
  const effects = [];
  let stateIndex = 0;
  let interval;
  context.setInterval = (callback, ms) => { interval = { callback, ms }; return 1; };
  context.clearInterval = () => {};
  context.window.__TAURI__ = { core: { invoke: async () => ({ phase: 'authorizing' }) } };
  const react = { ...stubReact,
    useState(initial) {
      return stateIndex++ === 0
        ? [{ phase: 'authorizing' }, () => {}]
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
  effects[1]();
  // 降频不能连授权中一起降掉：那一段本来就短，用户在浏览器里完成授权的
  // 过程中需要看到进度推进。
  assert.equal(interval.ms, 2000);
});

test('client: 常用模型区列出目录条目，勾选把整份集合交回壳', async () => {
  const { loaded, context } = loadClient();
  const view = {
    models: [
      { id: 'gpt-a', name: 'GPT A', capability: 'codex-catalog', selected: false },
      { id: 'gpt-b', name: 'GPT B', capability: 'unknown', selected: true },
    ],
    recommended: ['gpt-a'],
    note: null,
  };
  const invokes = [];
  context.window.__TAURI__ = {
    core: { invoke: async (command, args) => { invokes.push({ command, args }); return view; } },
  };
  // 一次 factory 调用里两个组件共用一份 React 桩，按调用顺序供状态：
  // AccountCard 先取 5 个（status / busy / lastCommand / error / nonce），
  // 随后 ModelSection 取 3 个（view / busy / error）。
  const states = [{ phase: 'authorized' }, false, '', '', 0, view, false, ''];
  let stateCalls = 0;
  const react = {
    ...stubReact,
    useState(initial) {
      const value = stateCalls < states.length ? states[stateCalls] : initial;
      stateCalls += 1;
      return [value, () => {}];
    },
    // 只有 AccountCard 的 effect 需要跑；ModelSection 的读列表交给直接调用。
    useEffect() {},
  };
  let footer;
  loaded.factory(() => react).apply({ slots: {
    inject(_name, register) { register(); },
    register(_descriptor, component) { footer = component; },
  } });

  const findSection = (node) => {
    if (!node || typeof node !== 'object') return null;
    if (typeof node.type === 'function' && node.props?.phase !== undefined) return node;
    for (const child of node.children ?? []) {
      const hit = findSection(child);
      if (hit) return hit;
    }
    return null;
  };
  const accountTree = footer().children[1].type();
  const section = findSection(accountTree);
  assert.ok(section, '账户卡里必须有「常用模型」区');
  const rendered = section.type(section.props);

  const inputs = [];
  const collect = (node) => {
    // 列表是 `models.map(...)` 的数组：桩 createElement 会把它原样嵌进
    // children（React 那边也一样），不展开就会数到 0 个勾选框。
    if (Array.isArray(node)) {
      for (const child of node) collect(child);
      return;
    }
    if (!node || typeof node !== 'object') return;
    if (node.type === 'input') inputs.push(node);
    for (const child of node.children ?? []) collect(child);
  };
  collect(rendered);
  assert.equal(inputs.length, 2, '目录里的每条模型一个勾选框');
  assert.deepEqual(inputs.map((input) => input.props.checked), [false, true]);
  // 至少留一条：只剩一条已勾选时它是禁用的（取消它会让整个提供方从会话
  // 选择器里消失）。
  assert.equal(inputs[1].props.disabled, true);

  // 勾上未勾选的那条 → 交回**整份**集合（不是增量）。参数在 vm 的 realm 里
  // 造出来，逐个取成同 realm 的基本值再比（deepStrictEqual 会比原型）。
  await inputs[0].props.onChange({ target: { checked: true } });
  assert.equal(invokes.length, 1);
  assert.equal(invokes[0].command, 'openai_models_selection_set');
  assert.deepEqual([...invokes[0].args.ids], ['gpt-a', 'gpt-b']);
  // 取消勾选**最后一条**已勾选 → 不发请求（空集合会让提供方从会话选择器里
  // 整组消失；UI 上那一条是禁用的，这里是第二道闸）。
  await inputs[1].props.onChange({ target: { checked: false } });
  assert.equal(invokes.length, 1, '空集合必须被挡住，不能落到壳里');
});
