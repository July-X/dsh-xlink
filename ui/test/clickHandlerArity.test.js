// `@click="fn"` 里的 fn 必须不带参数（2026-10-07）。
//
// Vue 模板里 `@click="showLogs"` 是**方法引用**：它把 MouseEvent 当第一个实参
// 传给 `showLogs`。而 `showLogs(preferredEvidencePath)` 会把那个 Event 对象
// 存进 `logModal.askedEvidence`，`resolvePreferred` 随即判定成 `'missing'`——
// 于是从事故面板点「打开日志」，弹层报「点名的证据找不到」，而用户根本没点过
// 任何证据。**全程没有任何报错**：传进去的东西类型合法，只是值错了。
//
// 这类 bug 靠读单个调用点看不出来（同一个文件里另外两处 `showLogs()` 是对的），
// 也不该靠人扫 100 多个 `@click`。下面这条判据做的是机械检查：把模板里
// **裸引用**的处理器名解析回它的定义（含跨文件 import），只要那个定义声明了
// 形参就报出来。
//
// ## 只查手势事件，不查 `@change` / `@input`
//
// `@click` / `@keyup` / `@keydown` 派发的是 DOM 事件（MouseEvent /
// KeyboardEvent），**永远不会是业务参数**——想传参的写法一定是 `fn(item)`。
// 所以裸引用遇到带形参的定义，一律是 bug。
//
// `@change` / `@input` 不一样：el-select 派发选中的值、el-switch 派发新的
// 布尔、el-input 派发输入的文本。`@change="pickCategory"` 让
// `pickCategory(id)` 直接拿到选中项是**正确且地道**的写法，不能报——
// PluginsPanel 与 DebugPanel 各有两处这样的写法，是有意为之。
//
// 三种写法的区别：
//   · `@click="showLogs"`        —— 传了 Event（本次修掉的三个）
//   · `@click="showLogs()"`      —— 显式空参，正确
//   · `@change="pickCategory"`   —— 组件派发值，正确，不在判据范围内
import assert from 'node:assert/strict';
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import test from 'node:test';

const ROOT = resolve('.');
const SRC = resolve('ui/src');

const walk = (dir) =>
  readdirSync(dir).flatMap((name) => {
    const full = join(dir, name);
    return statSync(full).isDirectory() ? walk(full) : [full];
  });

const VUE_FILES = walk(SRC).filter((f) => f.endsWith('.vue'));

/**
 * 一个文件里声明的函数，各自的**形参**（字符串，无参为 ''）。
 *
 * 只数"有没有形参"，不区分它有没有默认值：`manual = true` 被 Event 填进去
 * 恰好等价于默认值，今天没事，但它是靠巧合对的——下一个人改默认值或加一句
 * `if (manual instanceof PointerEvent) return;` 就会静默变坏。机械检查只管
 * 一律显式传参，不替它判断这次是不是巧合。
 */
function definitions(source) {
  const out = new Map();
  const record = (name, params) => {
    if (name && !out.has(name)) out.set(name, params);
  };
  // function foo(a, b) {  /  export async function foo(a)
  for (const m of source.matchAll(/(?:export\s+)?(?:async\s+)?function\s+([\w$]+)\s*\(([^)]*)\)/g)) {
    record(m[1], m[2].trim());
  }
  // const foo = (a) => …  /  const foo = async function (a) {  /  export const foo = …
  for (const m of source.matchAll(
    /(?:export\s+)?const\s+([\w$]+)\s*=\s*(?:async\s*)?(?:function\s*)?\(([^)]*)\)\s*(?:=>|\{)/g
  )) {
    record(m[1], m[2].trim());
  }
  return out;
}

/** 这个文件 import 了哪些名字，分别来自哪个模块。 */
function imports(source) {
  const out = new Map();
  for (const m of source.matchAll(/import\s*\{([^}]+)\}\s*from\s*['"]([^'"]+)['"]/g)) {
    for (const part of m[1].split(',')) {
      const name = part.trim().split(/\s+as\s+/).pop().trim();
      if (name) out.set(name, m[2]);
    }
  }
  return out;
}

const JS_CACHE = new Map();
const readJs = (file) => {
  if (!JS_CACHE.has(file)) JS_CACHE.set(file, definitions(readFileSync(file, 'utf8')));
  return JS_CACHE.get(file);
};

/** 把裸引用的处理器名解析回"它有没有形参"。解析不到（props / 未定义）返回 null。 */
function resolveParams(vueSource, baseDir, handler) {
  const local = definitions(vueSource).get(handler);
  if (local !== undefined) return local;

  const from = imports(vueSource).get(handler);
  if (!from || !from.startsWith('.')) return null;
  const target = resolve(baseDir, from);
  if (!target.endsWith('.js')) return null;
  const params = readJs(target).get(handler);
  return params === undefined ? null : params;
}

/** 扫一份 .vue 源码，返回违规点 `{ line, handler, params }`。 */
function scan(source, baseDir) {
  const at = source.indexOf('<template>');
  if (at === -1) return [];
  const template = source.slice(at);
  const found = [];
  for (const m of template.matchAll(/@(?:click|keyup|keydown)="([\w$]+)"/g)) {
    const params = resolveParams(source, baseDir, m[1]);
    if (params) {
      found.push({ line: source.slice(0, at + m.index).split('\n').length, handler: m[1], params });
    }
  }
  return found;
}

test('模板里的手势处理器不得声明形参', () => {
  const all = [];
  for (const file of VUE_FILES) {
    for (const v of scan(readFileSync(file, 'utf8'), dirname(file))) {
      all.push(`${relative(ROOT, file)}:${v.line} @click="${v.handler}" → 形参 (${v.params})`);
    }
  }
  assert.equal(
    all.length,
    0,
    `以下处理器会把 MouseEvent 当业务参数传进去（症状是「值错了但没报错」）：\n  ${all.join('\n  ')}\n` +
      '改法：写成 `fn()`（显式空参）。@change / @input 不在判据范围内——' +
      'el-select 与 el-switch 派发的就是值，裸引用是对的。'
  );
});

test('判据本身能扫出问题，且不误报正确写法', () => {
  // 反向验：一份带违规的样本必须只命中一处。少了这条，上面那条判据可能因为
  // 匹配写错而恒为 0 —— 一个从不报错的检查比没有检查更糟。
  const sample = [
    '<template>',
    '  <el-button @click="showLogs">打开日志</el-button>',
    '  <el-button @click="showLogs()">打开日志</el-button>',
    '  <el-button @click="showLogs($event.path)">打开日志</el-button>',
    '  <el-button @click="openModal">x</el-button>',
    '  <el-select @change="pickCategory">y</el-select>',
    '  <el-switch @change="togglePreview" />',
    '</template>',
    '<script setup>',
    'import { showLogs } from "../logs/logs.js";',
    'const openModal = () => {};',
    'const pickCategory = (id) => id;',
    'const togglePreview = (value) => value;',
    '</script>',
  ].join('\n');
  // baseDir 取 incidents/：样本里那条 import 写的是 `../logs/logs.js`，
  // 与真实的 IncidentModal.vue 同处一个目录，相对路径才落在同一个文件上。
  const found = scan(sample, join(SRC, 'incidents'));
  assert.equal(found.length, 1, `只应命中 showLogs 一处，实际：${JSON.stringify(found)}`);
  assert.equal(found[0].handler, 'showLogs');
});

test('判据的依赖解析真的跨到了 import 的模块', () => {
  // 上一条能命中，靠的正是它把 `showLogs` 解析回了 logs.js 里的定义。
  // 解析若退化成「只看本文件的定义」，这条会静默失效（命中数变成 0）。
  const params = readJs(join(SRC, 'logs/logs.js')).get('showLogs');
  assert.ok(params && params.length > 0, 'logs.js 里应当能解析出 showLogs 的形参');
});
