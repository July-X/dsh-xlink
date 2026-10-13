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

/**
 * 极简 hooks 运行时：让组件能被反复调用，并看到自己 setState 之后的样子。
 *
 * 为什么需要它：本次要钉的语义是「勾选只改草稿、点保存才落盘」，而那条链路
 * 必须经过 `setDraft` 才走得到——空 setter 的桩会让"勾完再保存"永远停在初始
 * 状态，测试于是只能验到"点了勾选框发了请求"这个**相反**的行为。
 */
function hookRuntime(initial) {
  const states = [...initial];
  let cursor = 0;
  const runtime = {
    createElement: stubReact.createElement,
    useCallback: (fn) => fn,
    effects: [],
    useEffect(effect) { runtime.effects.push(effect); },
    useState(initialValue) {
      const index = cursor;
      cursor += 1;
      if (index >= states.length) states[index] = initialValue;
      return [
        states[index],
        (value) => {
          states[index] = typeof value === 'function' ? value(states[index]) : value;
        },
      ];
    },
    /** 从第 offset 个状态位开始渲染（多个组件共用一个 factory 时用得上）。 */
    render(component, props = {}, offset = 0) {
      cursor = offset;
      runtime.effects = [];
      return component(props);
    },
    /** 跑本次渲染登记的 effect（真实 React 在挂载后跑一次；依赖不变不重跑）。 */
    runEffects() {
      const list = runtime.effects;
      runtime.effects = [];
      for (const effect of list) effect();
    },
  };
  return runtime;
}

/** 让已 resolve 的 promise 继续跑（invoke 桩是 async，状态回写在下一次微任务）。 */
const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

/** 深度优先找元素（数组子节点展开——`models.map(...)` 会原样嵌进 children）。 */
function findNodes(root, predicate, found = []) {
  if (Array.isArray(root)) {
    for (const child of root) findNodes(child, predicate, found);
    return found;
  }
  if (!root || typeof root !== 'object') return found;
  if (predicate(root)) found.push(root);
  for (const child of root.children ?? []) findNodes(child, predicate, found);
  return found;
}

test('client: 常用模型默认收起成一行，勾选只改草稿、点保存才落盘', async () => {
  const { loaded, context } = loadClient();
  const strings = vm.runInContext('STRINGS', context).zh;
  const modelsFor = (selected) => [
    { id: 'gpt-a', name: 'GPT A', capability: 'codex-catalog', selected: selected.includes('gpt-a') },
    { id: 'gpt-b', name: 'GPT B', capability: 'unknown', selected: selected.includes('gpt-b') },
    { id: 'gpt-c', name: 'GPT C', capability: 'codex-catalog', selected: selected.includes('gpt-c') },
  ];
  const viewFor = (selected) => ({ models: modelsFor(selected), recommended: ['gpt-a'], note: null });
  let saved = ['gpt-a'];
  const invokes = [];
  context.window.__TAURI__ = {
    core: {
      invoke: async (command, args) => {
        invokes.push({ command, args });
        if (command === 'openai_models_selection_set') saved = [...args.ids];
        return viewFor(saved);
      },
    },
  };

  // 一个 factory、一份运行时、两组状态位：AccountCard 用 [0..4]（status / busy /
  // lastCommand / error / nonce），ModelSection 用 [5..9]（view / draft / expanded /
  // busy / error）。渲染时显式给 offset —— 两套 state 位混在一起会让 AccountCard 的
  // status 被当成模型列表（那次踩过的症状是"勾选框一个都不画"）。
  const runtime = hookRuntime([
    { phase: 'authorized' }, false, '', '', 0,
    null, null, false, false, '',
  ]);
  const SECTION_OFFSET = 5;
  let footer;
  loaded.factory(() => runtime).apply({ slots: {
    inject(_name, register) { register(); },
    register(_descriptor, component) { footer = component; },
  } });
  const section = findNodes(
    runtime.render(footer().children[1].type, {}, 0),
    (node) => typeof node.type === 'function' && node.props?.phase !== undefined,
  )[0];
  assert.ok(section, '账户卡里必须有「常用模型」区');
  const render = () => runtime.render(section.type, section.props, SECTION_OFFSET);

  const boxes = () => findNodes(render(), (node) => node.type === 'input');
  const buttonWith = (className) =>
    findNodes(render(), (node) => node.type === 'button' && String(node.props?.className ?? '').includes(className))[0];
  const hasText = (text) => findNodes(render(), (node) => (node.children ?? []).includes(text)).length > 0;

  // 首帧：读列表（读回来之前只有加载文案）。
  render();
  runtime.runEffects();
  await flush();
  assert.equal(invokes.length, 1);
  assert.equal(invokes[0].command, 'openai_models_selection_get');

  // 默认**收起**：一行摘要，不画勾选框（2026-10-13 用户反馈「占用过大」）。
  assert.equal(boxes().length, 0, '收起时不该画几十行勾选框');
  assert.equal(hasText(strings.modelsSummary(['GPT A'])), true, '摘要行要说清现在选了哪些');
  assert.equal(buttonWith('xlink-oauth-save'), undefined, '收起且无改动时不摆一个永远灰着的保存按钮');
  assert.equal(hasText(strings.modelsHint), false, '说明文字只在展开编辑时出现');

  // 展开 → 勾选框出现（此时还没有改动，保存存在但不可点）。
  buttonWith('xlink-oauth-toggle').props.onClick();
  let inputs = boxes();
  assert.equal(inputs.length, 3, '目录里的每条模型一个勾选框');
  assert.deepEqual(inputs.map((input) => input.props.checked), [true, false, false]);
  assert.equal(hasText(strings.modelsHint), true, '展开时把后果说清楚');
  assert.equal(hasText(strings.modelsDirty), false, '与服务端一致时不该显示「未保存」');
  assert.equal(buttonWith('xlink-oauth-save').props.disabled, true);

  // 勾两下：只改草稿，一次请求都不发。**每次点击都重新取元素**——handler 闭包
  // 捕获的是它那一帧的草稿，拿旧帧的 handler 连点两次，第二次会从旧草稿重算
  // （那是测试桩的时序问题，真实 React 每帧重渲染会给新 handler）。
  boxes()[1].props.onChange({ target: { checked: true } });
  boxes()[2].props.onChange({ target: { checked: true } });
  assert.equal(invokes.length, 1, '勾选不许直接落盘（2026-10-13 用户要求）');
  inputs = boxes();
  assert.deepEqual(inputs.map((input) => input.props.checked), [true, true, true]);
  assert.equal(hasText(strings.modelsDirty), true, '有未保存的改动要说出来');
  assert.equal(buttonWith('xlink-oauth-save').props.disabled, false);
  // 未保存时不许收起（收起就看不见勾了什么），但「放弃改动」是出口。
  assert.equal(buttonWith('xlink-oauth-toggle').props.disabled, true, '未保存时收起必须禁用');
  buttonWith('xlink-oauth-discard').props.onClick();
  assert.equal(hasText(strings.modelsDirty), false);
  assert.deepEqual(boxes().map((input) => input.props.checked), [true, false, false]);

  // 再勾、点保存：整份集合交回壳（不是增量），保存成功后自动收起。
  boxes()[1].props.onChange({ target: { checked: true } });
  await buttonWith('xlink-oauth-save').props.onClick();
  assert.equal(invokes.length, 2);
  assert.equal(invokes[1].command, 'openai_models_selection_set');
  assert.deepEqual([...invokes[1].args.ids], ['gpt-a', 'gpt-b'], '未勾选的模型不许出现在集合里');
  assert.equal(boxes().length, 0, '保存成功后收起，把版面还回去');
  assert.equal(buttonWith('xlink-oauth-save'), undefined, '收起且无改动 → 不摆灰按钮');
  assert.equal(hasText(strings.modelsSummary(['GPT A', 'GPT B'])), true);

  // 「恢复默认」也只改草稿（同样要点保存），不直接写盘。
  buttonWith('xlink-oauth-toggle').props.onClick();
  buttonWith('xlink-oauth-restore').props.onClick();
  assert.equal(invokes.length, 2, '恢复默认同样不许直接落盘');
  assert.deepEqual(boxes().map((input) => input.props.checked), [true, false, false]);
  await buttonWith('xlink-oauth-save').props.onClick();
  assert.deepEqual([...invokes[2].args.ids], ['gpt-a']);

  // 至少留一条：只剩一条勾选时取消它 → 不发请求（提供方会整组从选择器里消失）。
  buttonWith('xlink-oauth-toggle').props.onClick();
  const single = boxes()[0];
  assert.equal(single.props.disabled, true, '最后一条已勾选项必须禁用');
  single.props.onChange({ target: { checked: false } });
  assert.equal(invokes.length, 3, '空集合必须被挡住，不能落到壳里');
});
