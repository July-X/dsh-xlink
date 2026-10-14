// 日志侧栏的分组折叠（2026-10-14 用户要求：「日志分组，增加点击收起分组的功能」）。
//
// 两层判据：
//   · **状态层**（logs.js）：折叠集合、持久化、动作的幂等性。这是唯一一份实现，
//     两个宿主（日志弹层 / 独立阅读窗口）共用——谁自己再存一份，就会出现
//     「弹层里收起了、独立窗口里又全展开」。
//   · **形态层**（LogSidebar.vue 模板 + theme.css）：标题是不是一个可点的
//     按钮、收起的对象是不是**文件列**（标题必须留着）、箭头与 aria 属性齐不齐、
//     按钮重置有没有做。这些都不会让 build 或 dev server 报错。
//
// 判据写法遵循「判据怎么写」一节：模板先剥 HTML 注释再匹配（本仓模板里大段
// 解释「为什么这么写」的中文注释会原样引用被判据的词）；CSS 先剥注释再找规则，
// 且**祖先条件下的规则按规则文本读**（`.log-group.is-collapsed .log-group-title`
// 这类，css-cascade 工具只认主语、会把祖先条件当成无关，读出来的值会答非所问）。
import assert from 'node:assert/strict';
import test from 'node:test';
import { readdirSync, readFileSync } from 'node:fs';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { resolve } from 'node:path';
import { ruleBlocks, SRC } from './lib/css-cascade.mjs';

// logs.js 会在模块加载期读一次 localStorage；这个文件里的每个用例都先装好
// stub 再动态 import（同 logPanelLayout.test.js 的处置）。
const core = {
  invoke() {
    return Promise.reject(new Error('not used'));
  },
  Channel: class {
    onmessage = null;
  },
};

globalThis.window = { __TAURI__: { core }, navigator: { userAgent: 'node' } };
globalThis.document = {
  hidden: false,
  createElement: () => ({}),
  createElementNS: () => ({}),
  createTextNode: () => ({}),
  createComment: () => ({}),
  querySelector: () => null,
  addEventListener() {},
  removeEventListener() {},
  body: { classList: { toggle() {} } },
};
Object.defineProperty(globalThis, 'navigator', {
  configurable: true,
  value: { userAgent: 'node' },
});

function makeLocalStorageStub() {
  const stores = {};
  return {
    writes: 0,
    getItem(key) {
      return Object.prototype.hasOwnProperty.call(stores, key) ? stores[key] : null;
    },
    setItem(key, value) {
      this.writes += 1;
      stores[key] = String(value);
    },
    removeItem(key) {
      delete stores[key];
    },
    _stores: stores,
  };
}

const KEY = 'dsh.logs.collapsedGroups';

/** 装一个新的 localStorage stub 并把共享状态清空，返回 `{ logs, ls }`。 */
async function freshLogs() {
  const ls = makeLocalStorageStub();
  globalThis.localStorage = ls;
  const logs = await import('../src/logs/logs.js');
  logs.collapsedLogGroups.clear();
  return { logs, ls };
}

// --- 1. 状态层 ---------------------------------------------------------------

test('点标题收起 / 展开一组：集合与 localStorage 一起变', async () => {
  const { logs, ls } = await freshLogs();
  const { collapsedLogGroups, toggleLogGroup } = logs;

  assert.equal(collapsedLogGroups.has('other'), false, '默认是展开的');
  toggleLogGroup('other');
  assert.equal(collapsedLogGroups.has('other'), true, '第一次点击应收起');
  assert.equal(ls.getItem(KEY), JSON.stringify(['other']), '收起要落盘（下次打开还记得）');

  toggleLogGroup('other');
  assert.equal(collapsedLogGroups.has('other'), false, '再点一次应展开');
  assert.equal(ls.getItem(KEY), JSON.stringify([]), '展开同样要落盘，否则会「回弹」');

  // 多组各自独立：收了两组就该存两个 id，不是「有一个收着」这一个布尔
  toggleLogGroup('other');
  toggleLogGroup('plugin');
  assert.deepEqual(JSON.parse(ls.getItem(KEY)).sort(), ['other', 'plugin']);
});

test('expandLogGroup 只对收起的组写盘（切一次文件不该同步写一次盘）', async () => {
  const { logs, ls } = await freshLogs();
  const { collapsedLogGroups, expandLogGroup, toggleLogGroup } = logs;

  expandLogGroup('other');
  assert.equal(collapsedLogGroups.has('other'), false);
  assert.equal(ls.writes, 0, '本来就展开的组，展开是一次空操作，不该产生写盘');

  toggleLogGroup('other');
  const afterToggle = ls.writes;
  expandLogGroup('other');
  assert.equal(collapsedLogGroups.has('other'), false, '收起的组要真的展开');
  assert.equal(ls.writes, afterToggle + 1, '真变了才写一次');
});

test('折叠集合是响应式的：`.has()` 变了，读它的计算属性必须重算', async () => {
  // 这条拦的是「把 `reactive(new Set())` 写回裸 `Set`」：状态照样读写、单测照样
  // 全绿，只有界面**纹丝不动**——点一下标题，那一组既不收也不展开，也没有任何
  // 报错。组件里那一行的 `collapsed` 就是这条 computed 的形状。
  const { logs } = await freshLogs();
  const { computed, isReactive } = await import('vue');
  assert.ok(
    isReactive(logs.collapsedLogGroups),
    '折叠集合必须是 reactive(new Set())，裸 Set 改了不会触发重渲染'
  );

  const groups = [{ id: 'other', files: [{ name: 'x.log' }] }];
  const rows = computed(() =>
    groups.map((g) => ({ ...g, collapsed: logs.collapsedLogGroups.has(g.id) }))
  );
  assert.equal(rows.value[0].collapsed, false);
  logs.toggleLogGroup('other');
  assert.equal(rows.value[0].collapsed, true, '点一下标题，行的收起态必须跟着变');
  logs.toggleLogGroup('other');
  assert.equal(rows.value[0].collapsed, false);
});

test('readCollapsedLogGroups 丢掉不认识的 id、并容忍坏值', async () => {
  const { logs, ls } = await freshLogs();
  const { readCollapsedLogGroups } = logs;

  // 旧版本收起过、如今已经不存在的分类：留着它既没有渲染方，又会让
  // 「这一组是收着的吗」永远为真
  ls.setItem(KEY, JSON.stringify(['other', 'ghost-category', 42, null]));
  assert.deepEqual(readCollapsedLogGroups(), ['other'], '只认 LOG_CATEGORIES 里还活着的 id');

  ls.setItem(KEY, 'not-json{');
  assert.deepEqual(readCollapsedLogGroups(), [], '不是 JSON 时当作都没收起，不能抛');

  ls.setItem(KEY, JSON.stringify({ other: true }));
  assert.deepEqual(readCollapsedLogGroups(), [], '不是数组同样当作都没收起');

  ls.removeItem(KEY);
  assert.deepEqual(readCollapsedLogGroups(), []);
});

test('localStorage 不可用时折叠动作照样生效、不抛', async () => {
  const { logs } = await freshLogs();
  globalThis.localStorage = null;
  assert.doesNotThrow(() => logs.toggleLogGroup('other'));
  assert.equal(logs.collapsedLogGroups.has('other'), true, '存不下不代表这次点击无效');
  logs.expandLogGroup('other');
  assert.equal(logs.collapsedLogGroups.has('other'), false);
});

test('模块加载时把持久化的折叠状态读进来', async () => {
  const ls = makeLocalStorageStub();
  ls.setItem(KEY, JSON.stringify(['other', 'ghost-category']));
  globalThis.localStorage = ls;
  // 换一个带查询串的模块实例，重新走一遍模块加载期的那次读取
  const fresh = await import(pathToFileURL(fileURLToPath(new URL('../src/logs/logs.js', import.meta.url))).href + '?hydrate=1');

  assert.deepEqual([...fresh.collapsedLogGroups], ['other'], '启动时读回收起的组，且过滤掉死 id');
  const { collapsedLogGroups } = await import('../src/logs/logs.js');
  assert.deepEqual([...collapsedLogGroups], [], '两个模块实例互不影响（上面那条用的是新实例）');
});

test('折叠状态只有一份实现：logs.js 定义、LogSidebar 消费', async () => {
  // 「弹层与独立窗口各记一份」是这个功能最容易走到的反例：两个宿主各写一个
  // Set，就会一边收起、另一边全展开。按引用方数一遍，比逐个断言两个组件
  // 「没有自己实现」更能拦住第三种写法。
  const files = [];
  const walk = (dir) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const path = resolve(dir, entry.name);
      if (entry.isDirectory()) walk(path);
      else if (path.endsWith('.vue') || path.endsWith('.js')) files.push(path);
    }
  };
  walk(SRC);
  const referencing = files
    .filter((path) => readFileSync(path, 'utf8').includes('collapsedLogGroups'))
    .map((path) => path.slice(SRC.length + 1))
    .sort();
  assert.deepEqual(referencing, ['logs/LogSidebar.vue', 'logs/logs.js']);

  // 两个宿主都只挂共享组件：LogSidebar 自己被改坏时，两边一起坏——这正是要的
  for (const host of ['logs/LogModal.vue', 'logs/LogViewerWindow.vue']) {
    assert.match(
      readFileSync(resolve(SRC, host), 'utf8'),
      /<LogSidebar/,
      `${host} 必须继续用共享侧栏渲染分组`
    );
  }
});

// --- 2. 模板形态 -------------------------------------------------------------

const stripHtmlComments = (s) => s.replace(/<!--[\s\S]*?-->/g, '');

/** `.vue` 的模板块（HTML 注释已剥），收尾边界取 `<style`（同 designAlignment）。 */
function templateOf(src) {
  const start = src.indexOf('<template>');
  assert.ok(start >= 0, '找不到 <template>');
  const styleAt = src.indexOf('<style', start);
  return stripHtmlComments(src.slice(start, styleAt > start ? styleAt : src.length));
}

const sidebarSrc = readFileSync(resolve(SRC, 'logs/LogSidebar.vue'), 'utf8');
const sidebarTpl = templateOf(sidebarSrc);

test('分组标题是一个可点的按钮，aria 属性与动作都在', () => {
  const hit = sidebarTpl.match(/<button\b[^>]*class="log-group-title"[^>]*>/);
  assert.ok(hit, '分组标题必须是一个 <button>（可点、可聚焦、读屏读得出）');
  const attrs = hit[0];

  assert.match(attrs, /type="button"/, '不写 type 的按钮在表单里会被当成提交键');
  assert.match(
    attrs,
    /:aria-expanded="!group\.collapsed"/,
    'aria-expanded 要说清这一组现在是展开还是收起'
  );
  assert.match(
    attrs,
    /:aria-controls="panelId\(group\.id\)"/,
    'aria-controls 必须指向被它收起的那一块'
  );
  assert.match(
    attrs,
    /@click="toggleLogGroup\(group\.id\)"/,
    '点击必须调 logs.js 的动作，而不是组件里自己 toggle 一份状态'
  );
});

test('收起的是文件列：标题与份数常驻，v-show 挂在文件列上', () => {
  const filesBlock = sidebarTpl.match(/<div[^>]*v-show="!group\.collapsed"[^>]*>/);
  assert.ok(filesBlock, '被收起的那一层必须由 v-show 挂在 group.collapsed 上');
  assert.match(
    filesBlock[0],
    /:id="panelId\(group\.id\)"/,
    'aria-controls 指过来的 id 必须落在这个容器上'
  );

  // 文件签必须在被收起的那一层**里面**：挂到外面就成了「收起了标题、文件还在」
  assert.equal(
    (sidebarTpl.match(/class="log-tab"/g) || []).length,
    1,
    '文件签只有一处渲染（v-for 一个模板）'
  );
  assert.ok(
    sidebarTpl.indexOf('class="log-tab"') > sidebarTpl.indexOf('v-show="!group.collapsed"'),
    '文件签必须排在收起容器的开标签之后'
  );

  // 收起态要看得见份数：不然一组日志收起来之后完全看不出里面有没有东西
  assert.match(
    sidebarTpl,
    /v-if="group\.collapsed"[\s\S]{0,60}group\.files\.length/,
    '收起态要显示这一组还剩几份日志'
  );

  // 箭头是唯一的方向指示，收起 / 展开共用一个元素（旋转靠 CSS）
  assert.equal(
    (sidebarTpl.match(/class="log-group-caret"/g) || []).length,
    1,
    '箭头只该有一处，两侧状态由 CSS 旋转表达'
  );
  // 基态必须是**朝下**那枚：收起态那条 `rotate(-90deg)` 把它转成朝右；
  // 换成 ArrowRight 时同一个角度会得到朝上的箭头（「返回」的意思），而界面
  // 不会报任何错——这一处就是 SSR 渲染时靠肉眼才发现的。
  assert.match(
    sidebarTpl,
    /<el-icon class="log-group-caret"><ArrowDown \/><\/el-icon>/,
    '箭头基态用 ArrowDown（已展开朝下、收起朝右），别换成 ArrowRight'
  );
});

test('激活的文件落在收起的组里时，会把那一组展开', () => {
  assert.match(
    sidebarSrc,
    /watch\(\s*\(\) => \[props\.activeName, props\.groups\]/,
    '要跟着「当前文件 / 分组」变化判断'
  );
  assert.match(
    sidebarSrc,
    /expandLogGroup\(owner\.id\)/,
    '自动展开必须走共享动作（自己删 Set 会漏掉落盘）'
  );
  assert.match(
    sidebarSrc,
    /name === expandedFor/,
    '只在「换文件」那一刻纠正：少了这个闸门，用户收起的组会被一次列表刷新顶回来'
  );
  assert.match(
    sidebarSrc,
    /if \(!name\)[\s\S]{0,80}expandedFor = null/,
    '文件被清空时要忘掉记下的名字，否则「关掉弹层再打开同一份证据」不会再展开'
  );
});

// --- 3. 样式 -----------------------------------------------------------------

const themeCss = readFileSync(resolve(SRC, 'theme.css'), 'utf8');
const RULES = ruleBlocks(themeCss);

function rule(selector) {
  const hit = RULES.find((r) => r.selector === selector);
  assert.ok(hit, `theme.css 里必须能找到 ${selector}`);
  return hit.body.replace(/\s+/g, ' ');
}

test('分组标题按按钮重置（裸 button 的灰底 / 居中 / 另一套字体）', () => {
  const body = rule('.log-group-title');
  assert.match(body, /cursor: pointer/, '不给手型就看不出这一行能点');
  assert.match(body, /background: transparent/, '不然是一块 UA 灰底');
  assert.match(body, /(?:^|;|\s)border: 0(?=;|\s|$)/, '不然是一圈 UA 边框');
  assert.match(body, /font-family: inherit/, '不然标题会跳回 UA 的控件字体');
  assert.match(
    body,
    /color: var\(--text-muted\)/,
    '颜色仍走 token：写死的那个值是按另一套主题调的（见 logWindowTheme.test.js）'
  );
});

test('收起态：标题下边距清零、箭头转 -90°、文件列接过 2px 间隙', () => {
  assert.match(
    rule('.log-group.is-collapsed .log-group-title'),
    /padding-bottom: 0/,
    '收起后标题下面什么都没有，6px 的下边距只剩一条空缝'
  );
  assert.match(
    rule('.log-group.is-collapsed .log-group-caret'),
    /transform: rotate\(-90deg\)/,
    '箭头要跟着状态转，否则收起与展开看起来一模一样'
  );
  const files = rule('.log-group-files');
  assert.match(files, /display: flex/, '文件列的容器要撑起纵向排布');
  assert.match(
    files,
    /gap: 2px/,
    '文件签之间那 2px 随容器搬家：留在 .log-group 上就变成「组标题↔文件列」的间隙'
  );
});
