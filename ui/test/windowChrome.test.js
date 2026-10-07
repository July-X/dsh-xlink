import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

// 窗口「外壳」的两条纪律：原生装饰（标题栏）跟主题走，自绘交通灯跟 macOS 原生尺寸走。
//
// 2026-10-07 用户截图同时报出两处：
//   · 独立窗口「模型用量」是浅色内容，却顶着一条深色原生标题栏（标题文字几乎看不见）；
//   · 主界面的自绘红绿灯「大小不符合实际」，而弹出 window（原生装饰）的那一组才是对的。
// 两条都曾是肉眼可见的割裂，且都不会报错——dev server、typecheck、build 全绿。
const read = (p) => readFileSync(new URL(p, import.meta.url), 'utf8');

// 判据必须先剥注释再断言：本文件描述的两个根因里，「`border: 1px` 配 border-box
// 吃掉两侧」这句话本身就会出现在 CSS 注释里，不剥就会把自己写的说明当成命中。
// （与 designAlignment.test.js 的 templateOf() 同一个坑，已踩四次。）
const stripCss = (s) => s.replace(/\/\*[\s\S]*?\*\//g, '').replace(/\/\/.*$/gm, '');
const stripJs = (s) => s.replace(/\/\*[\s\S]*?\*\//g, '').replace(/^\s*\/\/.*$/gm, '');

function cssRule(css, selector) {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  // 选择器后面必须紧跟空白 + `{`，否则 `.mac-titlebar__light` 会误命中
  // `.mac-titlebar__light--close`（`--` 不是空白）。
  const m = css.match(new RegExp(`(?:^|[},])\\s*${escaped}\\s*\\{([^}]*)\\}`, 's'));
  assert.ok(m, `必须能找到 ${selector} 的规则`);
  return stripCss(m[1]);
}

function px(body, prop) {
  const m = body.match(new RegExp(`(?:^|\\s|;)${prop}\\s*:\\s*(\\d+(?:\\.\\d+)?)px`));
  assert.ok(m, `必须能从规则里读到 ${prop} 的 px 声明值`);
  return Number(m[1]);
}

function fnBody(source, name) {
  const m = source.match(new RegExp(`function ${name}\\s*\\([^)]*\\)\\s*\\{([\\s\\S]*?)\\n\\}`));
  assert.ok(m, `必须能找到 ${name} 的定义`);
  return stripJs(m[1]);
}

// ---- 副窗原生 chrome 跟随应用主题 ------------------------------------------

test('applyTheme 把同一个主题真值同时推给 html.dark 与窗口原生装饰', () => {
  const body = fnBody(read('../src/shell/theme.js'), 'applyTheme');
  assert.match(
    body,
    /setWindowTheme\(\s*theme\.value\s*\)/,
    '原生标题栏必须用 theme.value，不能写死字面量——写死的话 setTheme("light") 只换页面不换标题栏',
  );
});

test('setTheme 走 applyTheme，运行中切换也同时改到原生 chrome', () => {
  const body = fnBody(read('../src/shell/theme.js'), 'setTheme');
  assert.match(body, /applyTheme\(\)/, '切换主题必须重新落一次 applyTheme');
});

test('原生主题只作用于本窗口，不批量改其他窗口', () => {
  const body = fnBody(read('../src/shell/bridge.js'), 'setWindowTheme');
  assert.match(body, /currentWindow\(\)/, '必须拿当前窗口句柄');
  assert.doesNotMatch(
    body,
    /getAll|webview_windows|\bwindows\(\)|forEach/,
    '批量改会反向造出「内容深色 + 标题栏浅色」的同一道割裂：已开着的副窗不会因主面板切换而重绘',
  );
});

test('壳自有副窗都拿到 set-theme 权限，内核页面与官网页签窗不拿', () => {
  // 壳自有：内容就是本应用的 SPA，主题由 localStorage 决定。
  for (const cap of ['default', 'log-viewer', 'usage-viewer', 'subscription-viewer']) {
    const perms = JSON.parse(read(`../../src-tauri/capabilities/${cap}.json`)).permissions;
    assert.ok(
      perms.includes('core:window:allow-set-theme'),
      `${cap}.json 缺 core:window:allow-set-theme：页面调 window.setTheme 会被 ACL 拒，副窗标题栏永远停在 Rust 钉死的深色`,
    );
  }
  // 内容是别人的页面（内核 webui / chat.deepseek.com 等），恒深色：
  // 让它们跟随本应用主题只会把割裂换个方向。
  for (const cap of ['harness-remote', 'official-chat-remote', 'official-chat-strip']) {
    const perms = JSON.parse(read(`../../src-tauri/capabilities/${cap}.json`)).permissions;
    assert.ok(
      !perms.includes('core:window:allow-set-theme'),
      `${cap}.json 不该有 core:window:allow-set-theme：这扇窗的内容不是本应用的页面`,
    );
  }
});

test('工作台窗口仍钉死深色（内容是内核 webui，不跟本应用主题）', () => {
  assert.match(
    read('../../src-tauri/src/harness/harness_window.rs'),
    /\.theme\(Some\(tauri::Theme::Dark\)\)/,
    '工作台装的是内核自己的 webui，改成跟随主题会让内核深色内容配浅色标题栏',
  );
});

// ---- 自绘交通灯对齐 macOS 原生尺寸 ------------------------------------------
//
// 参照物是同一张截图里那扇走原生装饰的窗口，1:1 像素量得：
//   原生：直径 12px、中心间距 23px
//   本地：外框 12px（1px 边框把着色吃到 10px）、中心间距 20px
// 直径本来就一致，「偏大」的是**挤**：间距少 3px 让三点连成一坨。

test('交通灯中心间距等于 macOS 原生实测的 23px', () => {
  const css = stripCss(read('../src/theme.css'));
  const diameter = px(cssRule(css, '.mac-titlebar__light'), 'width');
  const gap = px(cssRule(css, '.mac-titlebar__controls'), 'gap');
  assert.equal(
    diameter + gap,
    23,
    `直径 ${diameter}px + 间距 ${gap}px = ${diameter + gap}px，原生实测 23px（12 + 11）`,
  );
});

test('交通灯描边用 inset 阴影，不吃 border-box 的 12px', () => {
  const body = cssRule(stripCss(read('../src/theme.css')), '.mac-titlebar__light');
  assert.doesNotMatch(
    body,
    /border:\s*(?!none)[^;]*\d+px/,
    '全局 `* { box-sizing: border-box }` 下 1px 边框会让着色区缩到 10px，比原生的 12px 色块小一圈',
  );
  assert.doesNotMatch(body, /border-color:/, '描边同样不许走 border-color');
  assert.match(body, /border-radius:\s*50%/, '圆形是这套交通灯的形状契约');
});

test('三盏灯都带 inset 描边与填充色', () => {
  const css = stripCss(read('../src/theme.css'));
  for (const [variant, background] of [
    ['--close', '#ff5f57'],
    ['--minimize', '#febc2e'],
    ['--zoom', '#28c840'],
  ]) {
    const body = cssRule(css, `.mac-titlebar__light${variant}`);
    assert.match(body, /box-shadow:\s*inset\s+0\s+0\s+0\s+1px\s+#[0-9a-f]{6}/i, `${variant} 缺 inset 描边`);
    assert.equal(
      body.match(/background:\s*(#[0-9a-f]{6})/i)[1].toLowerCase(),
      background,
      `${variant} 的填充色必须还是 macOS 的 ${background}`,
    );
  }
});