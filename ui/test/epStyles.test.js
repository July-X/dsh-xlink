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