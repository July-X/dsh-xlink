import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

// 凡是 `main.js` 里 import 的 Element Plus 组件，都必须**同时** import 它的
// `style/css.mjs`。
//
// 2026-10-06 实测踩中：`main.js` import 了 `dropdown/index.mjs`（菜单能弹出来），
// 却漏了 `dropdown/style/css.mjs`。`el-dropdown.css` 因此从没进过产物——
// `.el-dropdown-menu` / `.el-dropdown-menu__item` 的 padding、hover 底色、
// 文字色、transition 全部缺失。症状是「菜单是一条没有任何交互反馈的裸文字
// 列表」；而 dev server、typecheck、`vite build`、甚至单测全绿，没有任何一处
// 会报错，只能靠看界面发现。
//
// 为什么这条必须钉死而不是靠自觉：**没有别处的 style 会传递地拉进它**。
// tooltip / select / popconfirm 各自的 style/css.mjs 只 import `base` +
// `popper`，谁都不 import dropdown。所以「反正别的组件会带上」这个想当然在此
// 恰好是错的，而且加新组件时同样会错。
const main = readFileSync('ui/src/main.js', 'utf8');

// 多行 import 形式：import {\n  A,\n  B,\n} from '.../dropdown/index.mjs';
const componentImports = new Set(
  Array.from(
    main.matchAll(/from\s*'element-plus\/es\/components\/([^/']+)\/index\.mjs'/g),
    (m) => m[1]
  )
);
const styleImports = new Set(
  Array.from(
    main.matchAll(/import\s*'element-plus\/es\/components\/([^/']+)\/style\/css\.mjs'/g),
    (m) => m[1]
  )
);

// **唯一的例外要写清楚为什么**，不能笼统放行「没有样式的组件」——那条豁免一旦
// 泛化，就正好把 dropdown 这类真缺样式的情况一起放过去了。
// `config-provider` 只做 provide（渲染 slot、注入全局配置），theme-chalk 里
// 它的 css 是 **0 字节**，import 它什么也不会变。
const STYLE_LESS_COMPONENTS = new Set(['config-provider']);

test('main.js 里的每个 Element Plus 组件都配了同目录的 style 导入', () => {
  assert.ok(componentImports.size > 0, '解析失败：一条组件导入都没读到');

  const missing = [...componentImports].filter(
    (name) => !styleImports.has(name) && !STYLE_LESS_COMPONENTS.has(name)
  );
  assert.deepEqual(
    missing,
    [],
    `这些组件 import 了却没 import style/css.mjs：${missing.join('、')}。` +
      '症状是该组件的基础样式（padding / hover / 文字色）整体缺失，' +
      '而构建与测试都不会报错——只能看界面发现。'
  );
});

test('豁免名单只列真正无样式的组件，且它们在豁免之前确实没被加进 style', () => {
  for (const name of STYLE_LESS_COMPONENTS) {
    assert.ok(componentImports.has(name), `${name} 已经不被 import 了，豁免可以删`);
    assert.ok(
      !styleImports.has(name),
      `${name} 现在有 style 导入了，豁免就该删掉（否则真缺样式时会被放过）`
    );
  }
});

test('dropdown 的样式导入确实在，且曾经漏过（防止有人把这条删掉）', () => {
  assert.ok(
    componentImports.has('dropdown'),
    'main.js 仍在用 el-dropdown；若组件已不用，这条判据需要一并更新'
  );
  assert.ok(
    styleImports.has('dropdown'),
    'el-dropdown 的样式导入被删了：菜单会退回成没有 padding / hover 的裸文字列表'
  );
});

test('菜单项 hover 底色落在暗色档，不会闪成一块浅色', () => {
  // EP 的 hover 底色是 `--el-color-primary-light-9`。这个变量在产物里**同时**
  // 存在三个取值：EP 浅色默认 `:root` 上的 `#ecf5ff`（近白）、EP 暗色与本仓
  // `html.dark` 覆写上的 `#1b2540` / `#18222b`。靠的是 `html.dark`（0,1,1）
  // 压过 `:root`（0,1,0）——一旦 `html.dark` 那条不生效，hover 会变成一块
  // 近白色块，而菜单底色此时仍是暗的：**很难一眼归因**，所以钉住取值本身。
  const theme = readFileSync('ui/src/theme.css', 'utf8');
  const dark = theme.match(/html\.dark\s*\{([^}]*)\}/);
  assert.ok(dark, '必须能找到 theme.css 的 html.dark 覆写块');

  const light9 = dark[1].match(/--el-color-primary-light-9:\s*(#[0-9a-fA-F]{6})/);
  assert.ok(light9, 'html.dark 必须覆写 --el-color-primary-light-9（EP 菜单 hover 底色）');

  const [r, g, b] = [1, 3, 5].map((i) => parseInt(light9[1].slice(i, i + 2), 16));
  const lum = 0.2126 * r + 0.7152 * g + 0.0722 * b;
  assert.ok(lum < 90, `--el-color-primary-light-9 = ${light9[1]}（亮度 ${lum.toFixed(0)}）偏亮，` +
    '菜单 hover 会在暗底上闪出一块浅色');

  // 前提本身：EP 的暗色变量靠 html 上的 class 生效。
  const html = readFileSync('ui/index.html', 'utf8');
  assert.match(html, /<html[^>]*class="dark"/, 'html 上必须有 class="dark"（EP 暗色变量的开关）');
});

test('「更多」菜单项都带图标，且复制类用的是本仓统一的 CopyDocument', () => {
  const menu = readFileSync('ui/src/diagnostics/diagnosis-more-menu.js', 'utf8');

  // 每一项都必须带 icon。少一项就是那一项退回纯文字，而这一排里三项有两项
  // 是复制，只靠文字区分极易点错——点错的代价是剪贴板多一段没用的文本，
  // 用户要等到粘贴时才发现。
  const items = Array.from(
    menu.matchAll(/\{\s*key:\s*'[a-z-]+',\s*label:\s*'[^']+'(,\s*icon:\s*\w+)?\s*\}/g),
    (m) => m[0]
  );
  assert.ok(items.length >= 5, `应读到 5 条菜单项定义，实际 ${items.length}`);
  for (const item of items) {
    assert.match(item, /icon:\s*\w+/, `${item} 缺 icon`);
  }

  // 复制类统一用 CopyDocument：IncidentModal 的「复制证据」已经用它表示复制，
  // 同一语义不许出现第二种长相。
  const copyItems = items.filter((i) => /label:\s*'复制/.test(i));
  assert.ok(copyItems.length >= 2, '至少两项是复制');
  for (const item of copyItems) {
    assert.match(item, /icon:\s*CopyDocument/, `${item} 该用 CopyDocument`);
  }

  // 渲染侧要把 icon 画出来，否则映射表改了也没人看得见。
  const shell = readFileSync('ui/src/diagnostics/DiagnosisShell.vue', 'utf8');
  assert.match(shell, /<component\s+:is="item\.icon"/);
  // 尺寸走组件属性：Lucide 的 svg 带 width/height，字体缩不动它；EP 的
  // el-icon 同样走 size，别用 CSS 的 font-size。
  assert.match(shell, /<el-icon[^>]*:size="14"/);
});

// --- 内嵌加载遮罩（`v-loading`）的底色：不能与全屏遮罩共用变量 ----------------
//
// EP 的 `.el-loading-mask`（内嵌，指令盖在一个区域上）与 dialog 的 `.el-overlay`
// （全屏）都读 `--el-mask-color`。本仓把那个变量调成「两套主题同向压暗」
// （theme.css 的 Element Plus 浅色覆写块），对全屏遮罩是对的，对内嵌遮罩是错
// 的：插件中心搜索目录时，浅色主题下整块加载区被盖成深灰，和白卡片打架
// （2026-10-09 用户报）。
//
// 为什么要钉测试而不靠自觉：改回去的动机太顺理成章——「同一语义一份实现」这条
// 仓库纪律会**推着人**把两条规则合并回同一个变量，而合并之后没有任何一个自动化
// 环节会报错（CSS 合法、构建绿、单测全绿），只有看界面才发现。

/** 解析 `rgba(...)` 字面量；不是字面量（含 `var()` 自引用）就返回 null，不返回 NaN。 */
function parseRgba(raw) {
  const m = /rgba?\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*(?:,\s*([\d.]+)\s*)?\)/.exec(raw ?? '');
  return m
    ? { rgb: [Number(m[1]), Number(m[2]), Number(m[3])], alpha: m[4] === undefined ? 1 : Number(m[4]) }
    : null;
}

const luminance = ({ rgb }) => 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2];

test('内嵌加载遮罩盖的是所在表面而不是压暗（两套主题各一份底色）', () => {
  const css = readFileSync('ui/src/theme.css', 'utf8');

  // ① 规则：`.el-loading-mask` 读自己的变量，且不得再碰 `--el-mask-color`。
  const maskBodies = Array.from(css.matchAll(/\.el-loading-mask\s*\{([^}]*)\}/g), (m) => m[1]);
  assert.ok(
    maskBodies.length >= 1,
    'theme.css 里找不到 .el-loading-mask 规则：遮罩退回 EP 默认的 var(--el-mask-color)，浅色主题下整块变深灰'
  );
  for (const body of maskBodies) {
    assert.match(
      body,
      /background(?:-color)?:\s*var\(--surface-loading\)/,
      `.el-loading-mask 必须声明 background-color: var(--surface-loading)，实际读到：${body.trim()}`
    );
    assert.doesNotMatch(
      body,
      /--el-mask-color/,
      '.el-loading-mask 又绑回 --el-mask-color 了：那是全屏遮罩的压暗档，内嵌遮罩用它会在浅色主题下把整块加载区画成深灰'
    );
  }

  // ② token 两套主题各一份。浅色与暗色的卡面不同（#ffffff / #2c2b30），只写
  // 一份必然有一边失真。位置用 `^html.dark {` 定位而不是 indexOf('html.dark')：
  // 注释里出现过这个词（`:root` 那段就在解释它），indexOf 会命中注释里的。
  const darkAt = css.search(/^html\.dark\s*\{/m);
  assert.ok(darkAt > 0, '必须能找到 theme.css 的 html.dark 块');
  const defs = Array.from(css.matchAll(/--surface-loading:\s*([^;]+);/g), (m) => ({
    at: m.index,
    raw: m[1],
  }));
  assert.equal(
    defs.length,
    2,
    `--surface-loading 应在 :root 与 html.dark 各定义一次，实际读到 ${defs.length} 处`
  );

  // ③ 值的方向：浅色必须亮、暗色必须暗，且两边 alpha 都够高（太低会让底下的旧
  // 内容透出来，看着像没遮住）。按亮度判、不按字符串判，改成别的浅色也拦得住。
  const light = parseRgba(defs.find((d) => d.at < darkAt).raw);
  const dark = parseRgba(defs.find((d) => d.at > darkAt).raw);
  assert.ok(light, `浅色 --surface-loading 不是 rgba 字面量（读到：${defs[0].raw}）`);
  assert.ok(dark, `暗色 --surface-loading 不是 rgba 字面量（读到：${defs[1].raw}）`);
  assert.ok(
    luminance(light) > 200,
    `浅色加载遮罩亮度 ${luminance(light).toFixed(0)} 偏暗：浅色主题下会盖成灰块`
  );
  assert.ok(
    luminance(dark) < 90,
    `暗色加载遮罩亮度 ${luminance(dark).toFixed(0)} 偏亮：暗色主题下会闪出一块浅色`
  );
  for (const [label, v] of [
    ['浅色', light],
    ['暗色', dark],
  ]) {
    assert.ok(v.alpha >= 0.85, `${label} --surface-loading 的 alpha 是 ${v.alpha}：太透，底下内容会透出来像没遮住`);
  }
});

test('全屏遮罩仍是压暗档（修内嵌遮罩不许顺手把它改亮）', () => {
  // 与上一条互为反向判据。`.el-overlay` / `.progress-overlay` 压暗是对的；有人
  // 为「统一两种遮罩」把 `--el-mask-color` 调亮，弹窗就会失去与背景的分离，
  // 而那正是它在 `:root` 里被写成两套主题同向深色的理由。
  const css = readFileSync('ui/src/theme.css', 'utf8');
  const defs = Array.from(css.matchAll(/--el-mask-color:\s*([^;]+);/g), (m) => m[1]);
  assert.equal(
    defs.length,
    2,
    `--el-mask-color 应在 :root 与 html.dark 各一份，实际读到 ${defs.length} 处`
  );
  for (const [i, label] of [
    [0, '浅色'],
    [1, '暗色'],
  ]) {
    const v = parseRgba(defs[i]);
    assert.ok(v, `${label} --el-mask-color 不是 rgba 字面量（读到：${defs[i]}）`);
    assert.ok(
      v.alpha >= 0.4 && v.alpha <= 0.75,
      `${label} --el-mask-color 的 alpha 是 ${v.alpha}：低于 0.4 弹窗与背景分不开，高于 0.75 背后的页面基本看不见`
    );
  }
});