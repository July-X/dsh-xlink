// 插件仓库搜索的**接线**：面板上那三个控件（关键词 / 分类 / 排序）一起发给
// 后端，回来的一页落进 store，分类下拉的计数由后端给的分类统计拼出来。
//
// 筛选与排序的规则本身（相关度三档、updated 何时接管、limit 硬顶）在 Rust 侧
// `plugins/catalog.rs` 里，那边有对应的单测；这里钉的是**浏览器这一侧**：
// 发了什么、存了什么、什么时候才发。相关度排序曾经在前端也有一份实现，
// 两份规则迟早会漂——搬走之后前端就不该再有第二份。
import assert from 'node:assert/strict';
import test from 'node:test';

const calls = [];
// 下一次 plugin_catalog_search 的返回；置 null 表示那次要失败。
let nextPage = { items: [], total: 0, counts: {} };
let failNext = false;

globalThis.window = {
  __TAURI__: {
    core: {
      invoke(command, args) {
        calls.push({ command, args });
        if (command === 'plugin_catalog_search') {
          if (failNext) {
            failNext = false;
            return Promise.reject(new Error('catalog unreachable'));
          }
          return Promise.resolve(nextPage);
        }
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
// 提示（toast）最终由 Element 的 ElMessage 渲染进 body。数着往 body 挂节点的
// 次数就能判断「这次到底弹没弹提示」——用户自己发起的搜索失败必须出声，
// 面板打开时那次静默搜索失败则不该打扰他。
let bodyAppends = 0;
body.appendChild = () => {
  bodyAppends += 1;
};
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

const mod = await import('../src/plugins/plugins.js');
const {
  pluginStore,
  searchCatalog,
  submitCatalogSearch,
  refineCatalog,
  loadCatalog,
  catalogCategories,
  CATALOG_CATEGORIES,
  CATALOG_PAGE,
} = mod;

const lastCall = () => calls[calls.length - 1].args;
const searchCalls = () => calls.filter((c) => c.command === 'plugin_catalog_search');

function reset() {
  calls.length = 0;
  pluginStore.query = '';
  pluginStore.category = 'all';
  pluginStore.sort = 'stars';
  pluginStore.shown = CATALOG_PAGE;
  pluginStore.catalogItems = [];
  pluginStore.catalogTotal = 0;
  pluginStore.catalogCounts = {};
  pluginStore.catalogLoaded = false;
}

test('三个参数一起发给后端，另带分页与分类中文名', async () => {
  reset();
  pluginStore.query = 'agent';
  pluginStore.category = 'agent';
  pluginStore.sort = 'updated';
  nextPage = { items: [{ name: 'a' }], total: 7, counts: { agent: 7 } };
  await searchCatalog();

  const args = lastCall();
  assert.equal(args.query, 'agent');
  assert.equal(args.category, 'agent');
  assert.equal(args.sort, 'updated');
  assert.equal(args.limit, CATALOG_PAGE, 'limit 就是已显示条数，「显示更多」靠累加它翻页');
  assert.equal(args.force, false);
  // 分类中文名随请求带过去，后端才搜得到「记忆」这类中文关键词。
  assert.equal(args.labels.memory, '记忆上下文');
  assert.equal(Object.keys(args.labels).length, CATALOG_CATEGORIES.length);
});

test('回的一页落进 store：items / total / counts 各归各位', async () => {
  reset();
  nextPage = { items: [{ name: 'a' }, { name: 'b' }], total: 128, counts: { agent: 96, skin: 32 } };
  await searchCatalog();
  assert.equal(pluginStore.catalogItems.length, 2);
  assert.equal(pluginStore.catalogTotal, 128, 'total 是命中总数，不是本页长度');
  assert.deepEqual(pluginStore.catalogCounts, { agent: 96, skin: 32 });
  assert.equal(pluginStore.catalogLoaded, true);
});

test('输入框回车才提交：修剪空白并回到第一页', async () => {
  reset();
  pluginStore.shown = CATALOG_PAGE * 3;
  await submitCatalogSearch('  memory  ');
  assert.equal(pluginStore.query, 'memory');
  assert.equal(pluginStore.shown, CATALOG_PAGE);
  assert.equal(lastCall().query, 'memory');
});

test('切分类 / 切排序带上新选择并重置分页', async () => {
  reset();
  pluginStore.shown = CATALOG_PAGE * 2;
  await refineCatalog({ category: 'skin', shown: CATALOG_PAGE });
  assert.equal(lastCall().category, 'skin');
  assert.equal(lastCall().limit, CATALOG_PAGE);

  await refineCatalog({ sort: 'updated', shown: CATALOG_PAGE });
  assert.equal(lastCall().sort, 'updated');
  assert.equal(lastCall().category, 'skin', '上一次选的分类还在，不是被重置回 all');
});

test('「显示更多」把 limit 累加上去，分页条数跟着涨', async () => {
  reset();
  nextPage = { items: [{ name: 'a' }], total: 100, counts: {} };
  await refineCatalog({ shown: pluginStore.shown + CATALOG_PAGE });
  assert.equal(lastCall().limit, CATALOG_PAGE * 2);
});

test('刷新数据带 force（跳过目录缓存窗口重新拉取）', async () => {
  reset();
  await searchCatalog({ force: true, loud: true });
  assert.equal(lastCall().force, true);
});

test('面板已经载过目录就不再打后端', async () => {
  reset();
  nextPage = { items: [{ name: 'a' }], total: 1, counts: {} };
  await loadCatalog();
  assert.equal(searchCalls().length, 1);
  await loadCatalog();
  assert.equal(searchCalls().length, 1, '重复打开面板不该重搜');
  await searchCatalog();
  assert.equal(searchCalls().length, 2, '但用户自己触发的搜索照发');
});

test('用户发起的搜索失败会出声，且不让转圈停不下来', async () => {
  reset();
  nextPage = { items: [{ name: 'stale' }], total: 1, counts: { skin: 1 } };
  await searchCatalog();
  const appendsBefore = bodyAppends;

  failNext = true;
  await submitCatalogSearch('boom');
  assert.ok(bodyAppends > appendsBefore, '失败必须弹提示，不能让用户对着不动的一页发呆');
  assert.equal(pluginStore.catalogLoaded, true, '看门狗式的终态：spinner 一定要关');
  assert.deepEqual(
    pluginStore.catalogItems.map((i) => i.name),
    ['stale'],
    '失败时保留上一轮结果，比清空成空列表好'
  );
});

test('面板打开时那次静默搜索失败不弹提示', async () => {
  reset();
  const appendsBefore = bodyAppends;
  failNext = true;
  await loadCatalog();
  assert.equal(bodyAppends, appendsBefore);
  assert.equal(pluginStore.catalogLoaded, true);
});

test('分类下拉由后端计数拼出：中文名 + 表外分类兜底', async () => {
  reset();
  nextPage = { items: [], total: 128, counts: { agent: 96, skin: 32, weird: 4 } };
  await searchCatalog();

  const chips = catalogCategories();
  // 顺序跟着 CATALOG_CATEGORIES（皮肤主题在 Agent 增强前面），不是按计数大小。
  assert.deepEqual(
    chips.map((c) => [c.id, c.count]),
    [['all', 128], ['skin', 32], ['agent', 96], ['weird', 4]]
  );
  assert.equal(chips[1].label, '皮肤主题');
  assert.equal(chips[2].label, 'Agent 增强');
  // 回退数据源（参考市场）的分类不在 dshfind 那九类里，id 本身就是标签。
  assert.equal(chips[3].label, 'weird');
  // 本轮没命中的分类不列出来（下拉里点它只会得到空列表）。
  assert.ok(!chips.some((c) => c.id === 'memory'));
});
