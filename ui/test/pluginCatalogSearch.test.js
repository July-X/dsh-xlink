// 插件中心搜索枚举：相关度排序、命中高亮、筛选状态复位。
//
// 这一块此前**完全没有测试**，因为搜索功能本身是坏的——pluginStore.query
// 有状态、有过滤、有防抖 watcher，却没有输入框绑定它。补上入口后这里把
// 行为钉住，防止又退回「命中与否不分先后」的二值过滤。
import assert from 'node:assert/strict';
import test from 'node:test';

globalThis.window = {
  __TAURI__: {
    core: {
      invoke() {
        return Promise.resolve([]);
      },
      Channel: class {
        onmessage = null;
      },
    },
  },
  navigator: { userAgent: 'node' },
  addEventListener() {},
  removeEventListener() {},
  getComputedStyle() {
    return { transitionDuration: '0s', animationDuration: '0s', transitionDelay: '0s', animationDelay: '0s' };
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
globalThis.requestAnimationFrame = (callback) => {
  callback();
  return 1;
};
globalThis.cancelAnimationFrame = () => {};

// 目录夹具刻意按「相关度与星数相反」的顺序摆放：只有真的按相关度排序，
// 下面的断言才会通过；沿用目录原序则会失败。
// 三档相关度各一：名称前缀 agent-toolkit、名称中段 my-agent-plugin、
// 仅描述命中 zzz-mention-model（名字里刻意不带 agent）。
const catalog = [
  {
    name: 'zzz-mention-model',
    description: '一个只在描述里提了一句 agent 的插件',
    category: 'tools',
    tags: ['misc'],
    stars: 900,
    updated: '2026-01-05T00:00:00Z',
  },
  {
    name: 'agent-toolkit',
    description: '给 agent 用的工具箱',
    category: 'agent',
    tags: ['agent'],
    stars: 10,
    updated: '2026-01-01T00:00:00Z',
  },
  {
    name: 'my-agent-plugin',
    description: '名字里带 agent',
    category: 'agent',
    tags: [],
    stars: 5,
    updated: '2026-01-02T00:00:00Z',
  },
  {
    name: 'theme-dark',
    description: '深色皮肤',
    category: 'skin',
    tags: ['theme'],
    stars: 800,
    updated: '2026-02-01T00:00:00Z',
  },
];

const mod = await import('../src/plugins/plugins.js');
const { pluginStore, filteredCatalog, matchParts, hasActiveFilter, resetCatalogFilters, CATALOG_PAGE } = mod;

function reset() {
  resetCatalogFilters();
  pluginStore.sort = 'stars';
  pluginStore.catalogItems = catalog;
  pluginStore.view = null;
}

const names = (items) => items.map((item) => item.name);

test('搜索按相关度排序：名称前缀 > 名称中段 > 仅描述命中', () => {
  reset();
  pluginStore.query = 'agent';
  // 星数最高的 zzz-mention-model 只在描述里命中 agent，排最后。
  assert.deepEqual(names(filteredCatalog(new Set())), [
    'agent-toolkit',
    'my-agent-plugin',
    'zzz-mention-model',
  ]);
});

test('相关度同级时保持目录原序（后端已按 star 排好）', () => {
  reset();
  // 两个条目都只在描述里命中 agent（同级 = 1），且刻意让 aaa 排在 zzz 前面：
  // filteredCatalog 不再对同级做二次排序——目录顺序本身来自后端（star 序），
  // 任何额外重排都会把这层含义弄掉。
  pluginStore.catalogItems = [
    { name: 'aaa-mention-model', description: '描述里提到 agent', category: 'tools', stars: 20, updated: '2026-01-05T00:00:00Z' },
    { name: 'zzz-mention-model', description: '描述里提到 agent', category: 'tools', stars: 900, updated: '2026-01-06T00:00:00Z' },
  ];
  pluginStore.query = 'agent';
  assert.deepEqual(names(filteredCatalog(new Set())), ['aaa-mention-model', 'zzz-mention-model']);
});

test('有关键词时「最近更新」不接管排序', () => {
  reset();
  pluginStore.sort = 'updated';
  pluginStore.query = 'agent';
  // 若让 updated 接管，updated 最晚的 my-agent-plugin 会排到第一位。
  assert.deepEqual(names(filteredCatalog(new Set())), [
    'agent-toolkit',
    'my-agent-plugin',
    'zzz-mention-model',
  ]);
});

test('没有关键词时「最近更新」按时间倒序', () => {
  reset();
  pluginStore.sort = 'updated';
  assert.deepEqual(names(filteredCatalog(new Set())), [
    'theme-dark',
    'zzz-mention-model',
    'my-agent-plugin',
    'agent-toolkit',
  ]);
});

test('未命中的条目被剔除，不是排在末尾', () => {
  reset();
  pluginStore.query = 'theme';
  assert.deepEqual(names(filteredCatalog(new Set())), ['theme-dark']);
});

test('关键词大小写与首尾空白不敏感', () => {
  reset();
  pluginStore.query = '  AGENT  ';
  assert.equal(filteredCatalog(new Set()).length, 3);
});

test('分类与安装状态筛选和搜索取交集', () => {
  reset();
  pluginStore.query = 'agent';
  pluginStore.category = 'agent';
  assert.deepEqual(names(filteredCatalog(new Set())), ['agent-toolkit', 'my-agent-plugin']);

  // 已安装筛选：只留命中且已装的。
  const keys = new Set(['my-agent-plugin']);
  pluginStore.category = 'all';
  pluginStore.filter = 'installed';
  assert.deepEqual(names(filteredCatalog(keys)), ['my-agent-plugin']);
  pluginStore.filter = 'not-installed';
  assert.deepEqual(names(filteredCatalog(keys)), ['agent-toolkit', 'zzz-mention-model']);
});

test('matchParts 只切第一处命中，空查询原样返回', () => {
  assert.deepEqual(matchParts('agent', ''), [{ text: 'agent', hit: false }]);
  assert.deepEqual(matchParts('agent', 'agent'), [{ text: 'agent', hit: true }]);
  assert.deepEqual(matchParts('my-agent-plugin', 'AGENT'), [
    { text: 'my-', hit: false },
    { text: 'agent', hit: true },
    { text: '-plugin', hit: false },
  ]);
  assert.deepEqual(matchParts('nothing-here', 'zzz'), [{ text: 'nothing-here', hit: false }]);
  // 名称缺字段时不能抛：渲染成一段空文本即可。
  assert.deepEqual(matchParts(null, 'a'), [{ text: '', hit: false }]);
  assert.deepEqual(matchParts(undefined, undefined), [{ text: '', hit: false }]);
});

test('hasActiveFilter 反映三类筛选，resetCatalogFilters 全部复位', () => {
  reset();
  assert.equal(hasActiveFilter(), false);
  pluginStore.query = '  ';
  assert.equal(hasActiveFilter(), false, '纯空白不算筛选');

  pluginStore.query = 'agent';
  assert.equal(hasActiveFilter(), true);
  resetCatalogFilters();
  assert.equal(hasActiveFilter(), false);
  assert.equal(pluginStore.query, '');
  assert.equal(pluginStore.shown, CATALOG_PAGE);

  pluginStore.category = 'skin';
  assert.equal(hasActiveFilter(), true);
  resetCatalogFilters();
  assert.equal(pluginStore.category, 'all');

  pluginStore.filter = 'installed';
  assert.equal(hasActiveFilter(), true);
  resetCatalogFilters();
  assert.equal(pluginStore.filter, 'all');
});
