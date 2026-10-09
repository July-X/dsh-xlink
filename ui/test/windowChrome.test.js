import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
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
// Rust 生产代码：剥掉 `#[cfg(test)] mod tests { … }` 整块（测试里为了构造各种
// 场景会写上装饰相关的字样）与两种注释。判据扫生产代码时少剥一种，就会把
// 测试夹具当成生产用法——本仓在 Rust 侧踩过同样的坑（用现成的 productionRust()）。
const stripRs = (s) =>
  s
    .replace(/#\[cfg\(test\)\][\s\S]*?\n}\s*$/m, '')
    .replace(/\/\*[\s\S]*?\*\//g, '')
    .replace(/^\s*\/\/.*$/gm, '');

/** `src-tauri/src` 下所有 `.rs`（递归），绝对路径。 */
function rustFiles(dir = new URL('../../src-tauri/src/', import.meta.url), out = []) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const p = new URL(entry.name + (entry.isDirectory() ? '/' : ''), dir);
    if (entry.isDirectory()) rustFiles(p, out);
    else if (entry.name.endsWith('.rs')) out.push(p);
  }
  return out;
}

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

// ---- 窗口外壳：圆角与副窗自绘标题栏（2026-10-08） ------------------------
//
// 用户指出「window 的边角的圆角保持一致」与「红绿灯大小 / hover 要匹配系统」。
// 查证结果是两件都**不由 CSS 画**：圆角与原生交通灯都由 macOS 窗口系统按窗口
// 类型决定。统一它们只能把窗口改成透明、自绘。
//
// 「入口的 import 必须真的解析得到」这条不是凑数：拆分 themeSync 时 `main.js`
// 的 import 指向了错误模块，`npm run build:ui` 才报出来——UI 测试全绿、Rust 侧
// 零警告。**一个连构建都过不去的改动，居然没被任何一条既有判据拦下**，于是
// 补上这条最基本的接线判据。
test('入口 main.js 的每个具名 import 都真的被导出（构建能过不是判据能过）', () => {
  const main = read('../src/main.js');
  const importRe = /import\s*\{([^}]+)\}\s*from\s*'([^']+)'/g;
  let checked = 0;
  for (const m of main.matchAll(importRe)) {
    const names = m[1].split(',').map((s) => s.trim()).filter(Boolean);
    const spec = m[2];
    if (!spec.startsWith('.')) continue; // 包导入不归本判据管
    const target = new URL(spec, new URL('../src/main.js', import.meta.url));
    let source;
    try {
      source = readFileSync(target, 'utf8');
    } catch {
      // 目录导入（`./foo` → `foo/index.js`）在本仓没有，出现即视为漏判。
      assert.fail(`main.js 导入了 ${spec}，但它解析不到文件`);
    }
    for (const name of names) {
      const re = new RegExp(`export\\s+(const|function|let|class)\\s+${name}\\b|export\\s*\\{[^}]*\\b${name}\\b`);
      assert.match(
        source,
        re,
        `main.js 从 ${spec} 导入了 ${name}，但那个模块没有导出它——这类错只有构建才会报`,
      );
      checked += 1;
    }
  }
  // 自检：`main.js` 的 import 绝大多数是 Element Plus 的默认导入，真正的**本地
  // 具名导入**只有几个（reportRenderError / ElMessage 之类）。阈值定 4 而不是
  // 「> 10」：后者是照着记忆写的，实际只有 6 个，于是这条判据在正确代码上一直红——
  // 而一条永远红的判据等于没有判据。
  assert.ok(checked >= 4, `只检查了 ${checked} 个具名导入，判据多半没抓到东西`);
});

test('窗口装饰只在建窗期给：不许运行时 set_decorations（黄灯会变死按钮）', () => {
  // 根因（见 lib.rs 的 check_main_window_minimizable 文档注释）：tao 的
  // set_decorations(false) 会把 NSWindowStyleMask 重算成 Borderless | Resizable，
  // **丢掉 Miniaturizable**，于是 miniaturize: 静默失败、minimize() 还返回 Ok()。
  // 与之相对，transparent 走的是另一条路径（只 setOpaque(false) +
  // setBackgroundColor(clearColor)），不碰样式位——这一点 2026-10-08 已核对
  // tao 0.35 的 platform_impl/macos/window.rs 确认，不是推断。
  //
  // 扫**整个生产 Rust**（逐文件递归读，不 spawn rg）：子进程 rg 在某些环境下
  // 不在 PATH，而这条判据红不红取决于它能不能跑起来——那种红是没有信息量的。
  //
  // **只拦 set_decorations**，不连带 set_always_on_top / set_shadow：
  // 前者是 z-order（`resident.rs` 的「先置顶再解除」让窗口真浮到最前，完全
  // 无害，与样式位无关），后者虽也属窗口属性但目前无调用。把无害的调用圈进来
  // 只会逼人加豁免名单，判据也就跟着松了——**判据要拦的是那个具体危害，
  // 不是所有长得像的东西**。
  const hits = [];
  for (const file of rustFiles()) {
    const body = stripRs(readFileSync(file, 'utf8'));
    for (const line of body.split('\n')) {
      if (/set_decorations/.test(line)) {
        hits.push(`${file.pathname.split('/src-tauri/src/').pop()}: ${line.trim()}`);
      }
    }
  }
  assert.deepEqual(
    hits,
    [],
    '不许在运行期改窗口装饰：它会抹掉 Miniaturizable 样式位，黄灯随即变成点了没反应的死按钮。要改就在建窗期给（decorations / transparent / shadow）。',
  );
});

test('四扇壳自有窗口都建窗期透明 + 无阴影 + 无边框', () => {
  const conf = JSON.parse(read('../../src-tauri/tauri.conf.json'));
  const main = conf.app.windows.find((w) => w.label === 'main');
  // 三项缺一不可：`transparent` 让窗口变成透明画布（圆角才有意义），
  // `shadow: false` 去掉系统投影——它在透明 + 圆角后会画在圆角之外、读作一圈
  // 黑色矩形光晕（投影改由 CSS 补），`decorations: false` 让标题栏自绘。
  assert.equal(main.transparent, true, '主壳必须透明，否则系统不给圆角');
  assert.equal(main.shadow, false, '必须关系统阴影，否则圆角外有一圈黑边');
  assert.equal(main.decorations, false, '必须无边框，标题栏才是自绘的');
  assert.equal(
    main.backgroundColor,
    '#00000000',
    '窗口底色必须全透明——填了暗色就把圆角填死了',
  );

  // 三扇副窗在 Rust builder 里给同一套。**判据查共享函数，不查三处字面量**：
  // 三份各写一遍必然漂（本次实现就先各写了一遍、被代码预算门禁顶回来才改成共享）。
  // 共享函数本身查一次就够——三处 caller 只要都调它，就不可能漂。
  const win = stripRs(read('../../src-tauri/src/shell/window.rs'));
  const decorate = win.match(/pub fn decorate_transparent[\s\S]*?\n}/);
  assert.ok(decorate, '找不到 decorate_transparent：三扇副窗的装饰片段必须收在一处');
  assert.match(decorate[0], /\.decorations\(false\)/, '副窗必须无边框（自绘标题栏）');
  assert.match(decorate[0], /\.shadow\(false\)/, '副窗必须关系统阴影，否则圆角外有黑边');
  assert.match(decorate[0], /\.transparent\(true\)/, '副窗必须透明，否则圆角不一致');

  // 三扇窗都得真的调它（漏一处 = 那扇窗没装饰 = 交通灯消失且不报错）。
  for (const [p, label] of [
    ['../../src-tauri/src/usage/local.rs', '模型用量'],
    ['../../src-tauri/src/usage/subscription.rs', '套餐用量'],
    ['../../src-tauri/src/commands.rs', '日志'],
  ]) {
    assert.match(
      stripRs(read(p)),
      /decorate_transparent\(/,
      `${label}窗口必须走共享的装饰片段`,
    );
  }
});

test('底色跟着圆角一起搬到 #app，body 必须透明', () => {
  const css = stripCss(read('../src/theme.css'));
  // 圆角画在 #app 上，而**底色必须跟着它一起搬**：留在 body 上的话，圆角之外的
  // 四角仍是一块实色方块，圆角等于白做。
  assert.match(css, /body\s*\{[^}]*background-color:\s*transparent/, 'body 必须透明');
  const app = cssRule(css, '#app');
  assert.match(app, /border-radius:\s*var\(--window-radius\)/, '#app 负责圆角');
  assert.match(app, /overflow:\s*hidden/, '内容必须按圆角裁切');
  assert.match(app, /background-color:\s*var\(--window\)/, '窗口底色在 #app 上');
  // 投影画在 ::after 上而不是 #app 本身：overflow: hidden 会把它一起裁掉。
  assert.match(cssRule(css, '#app::after'), /box-shadow/, '投影要画在伪元素上');
});

test('三扇壳自有副窗共用一个自绘外壳，官网页签栏不用（它承载别人的页面）', () => {
  const main = read('../src/main.js');
  // 两头都要钉：**只钉「排除谁」不够**——把某扇副窗从条件里漏掉（交通灯消失）
  // 时，那条判据照样绿。反向验逼出过这一版：它只查 `!isChatStrip`，于是
  // 「日志窗恢复成原生标题栏」完全抓不到。
  const cond = main.match(/if \(\s*usesCustomTitlebar([^)]*)\)/);
  assert.ok(cond, '找不到自绘标题栏的条件');
  const exclusions = [...cond[1].matchAll(/!\s*(is[A-Z]\w+)/g)].map((m) => m[1]);
  // 只许排除官网页签栏；日志 / 用量 / 套餐三扇自��副窗都要走自绘。
  assert.deepEqual(
    exclusions.filter((name) => name !== 'isChatStrip'),
    [],
    '只许排除官网页签栏——它承载 chat.deepseek.com 等别人的页面，装饰不能动',
  );
  assert.ok(exclusions.includes('isChatStrip'), '官网页签栏必须被排除');

  // 三扇窗各自都要真的用上共享外壳。
  for (const p of [
    '../src/logs/LogViewerWindow.vue',
    '../src/usage/UsageWindow.vue',
    '../src/subscription/SubscriptionWindow.vue',
  ]) {
    const src = read(p);
    assert.match(src, /import ViewerShell from/, `${p} 应当 import 共享外壳`);
    assert.match(src, /<ViewerShell\s+shell-class=/, `${p} 应当用共享外壳包住内容`);
  }
  const shell = read('../src/shell/ViewerShell.vue');
  assert.match(shell, /import WindowTitleBar/, '外壳里必须有自绘标题栏');
  assert.match(shell, /<WindowTitleBar\s+:title="title"/, '外壳必须把功能标题传进标题栏');
  assert.match(shell, /var\(--window-radius\)/, '外壳的圆角要与 #app 同一个 token');
  // 自绘标题栏需要拖拽与最小化权限：三扇窗的能力文件里都要有。
  for (const cap of ['log-viewer', 'usage-viewer', 'subscription-viewer']) {
    const perms = JSON.parse(read(`../../src-tauri/capabilities/${cap}.json`)).permissions;
    assert.ok(
      perms.includes('core:window:allow-start-dragging'),
      `${cap} 缺 start-dragging：没有它标题栏拖不动窗口（且不会报错）`,
    );
    assert.ok(perms.includes('core:window:allow-minimize'), `${cap} 缺 minimize 权限`);
  }
});

test('副窗标题栏显示「功能名@应用名」，主壳只显示应用名（三扇各一个功能名）', () => {
  // 2026-10-08 用户实机跑完报「副窗的功能标题需要保留」。副窗的 head 行
  // （品牌图标 + 功能名 + 刷新等动作）是**内容区**的页头，在标题栏**下面**；
  // 标题栏若只写「Dsh-Xlink」，两行连着读就是「窗口叫什么 + 这窗装什么」，
  // 而标题栏该回答的「我现在开着哪个功能」没有答案。
  const shell = read('../src/shell/ViewerShell.vue');
  assert.match(shell, /title:\s*\{\s*type:\s*String,\s*required:\s*true\s*\}/, '外壳要求传标题');

  // 三扇窗各自传什么，一一对上。日志窗要跟着当前文件走（与它 head 那行同名），
  // 所以它是绑定不是字面量——但绑定里**必须有兜底**，否则没选中文件时标题会退成
  // 空串，最终显示成 `@Dsh-Xlink`（功能名那半截没了）。这条在本仓被反向验逼出过一次：
  // 判据只查「传了 title 时格式对不对」，漏掉「传的值本身可能为空」。
  const expected = [
    ['../src/logs/LogViewerWindow.vue', /:title="activeName \|\| '日志'"/],
    ['../src/usage/UsageWindow.vue', /title="模型用量"/],
    ['../src/subscription/SubscriptionWindow.vue', /title="套餐用量"/],
  ];
  for (const [p, re] of expected) {
    assert.match(read(p), re, `${p} 应当把自己的功能标题传给外壳`);
  }

  // 标题栏组件：**真的求值**那一行 caption，而不是比字符串。只比字符串的话，
  // 改个分隔符、或者副窗那一支拼错了，判据照样绿——「不传 title 时显示应用名」
  // 只钉住了两支中的一支。
  const bar = read('../src/shell/WindowTitleBar.vue');
  const expr = bar.match(/const caption = ([\s\S]*?);\n/);
  assert.ok(expr, '找不到 `const caption = …`');
  const captionOf = new Function('props', `return ${expr[1]};`);
  assert.equal(
    captionOf({ title: '' }),
    'Dsh-Xlink',
    '主壳不传 title，显示的就是应用名',
  );
  // 格式由用户 2026-10-08 定：`功能名@应用名`。
  assert.equal(captionOf({ title: '日志' }), '日志@Dsh-Xlink', '副窗应是「功能名@应用名」');
  assert.match(bar, /\{\{ caption }\}/, '标题栏渲染的是那个值，不能写死字符串');
  assert.doesNotMatch(
    stripHtmlComments(bar),
    /<span>Dsh-Xlink<\/span>/,
    '标题不能写死在模板里——那样副窗传什么都不会生效，且不报错',
  );
});

test('交通灯 hover 是「整组浮现」，底色不变；且不漏进 Windows', () => {
  const css = stripCss(read('../src/theme.css'));
  // **组 hover**：macOS 上符号是悬停整组时一起浮现的。此前写的是单颗
  // `.light:hover`，指针压在红上就只亮红——另外两颗会读成「不可点」。
  assert.match(
    css,
    /\.mac-titlebar__controls:hover\s+\.mac-titlebar__light::before\s*\{[^}]*opacity:\s*1/,
    '符号必须是整组 hover 时浮现',
  );
  assert.doesNotMatch(
    css,
    /\.mac-titlebar__light:hover\s*\{/,
    '不许退回单颗 hover',
  );
  // 底色在 hover 时不变：此前是 `filter: brightness(1.08)`，那是「灯变亮」，
  // 原生不做这件事——灯一变亮，整排读成「有一枚被选中了」。
  assert.doesNotMatch(css, /brightness\(/, '灯的底色不许在 hover 时变化');
  // 三枚符号各自的颜色：原生红 #460804 / 黄 #90591d / 绿 #2a6218。统一成一个
  // 深灰会在黄绿上偏紫——那三处偏紫正是「不像 macOS」最显眼的地方。
  for (const [cls, color] of [
    ['close', '#460804'],
    ['minimize', '#90591d'],
    ['zoom', '#2a6218'],
  ]) {
    const rule = cssRule(css, `.mac-titlebar__light--${cls}::before`);
    assert.match(rule, new RegExp(`color:\\s*${color}`), `${cls} 的符号颜色应是 ${color}`);
  }
  // 不漏进 Windows：那组按钮带 `v-if="isMacTitlebar"`，Windows 上根本不渲染。
  // 这里钉住模板里那个判定——模板一改（比如去掉 v-if），交通灯就会出现在
  // Windows 的右侧标题栏上，而那不是 Windows 的语言。
  const bar = stripHtmlComments(read('../src/shell/WindowTitleBar.vue'));
  assert.match(
    bar,
    /<div v-if="isMacTitlebar" class="mac-titlebar__controls"/,
    '交通灯那组只许在 macOS 渲染',
  );
  assert.match(
    bar,
    /<div v-if="isWindowsTitlebar" class="win-caption"/,
    'Windows 用自己的右侧标题栏按钮，不许与交通灯混用',
  );
});

/** 剥掉 HTML 注释（模板里解释「为什么这样写」那段常原样引用被判据的字面量）。 */
function stripHtmlComments(s) {
  return s.replace(/<!--[\s\S]*?-->/g, '');
}

// ---- 副窗原生 chrome 跟随应用主题 ------------------------------------------

test('applyTheme 把同一个主题真值同时推给 html.dark 与窗口原生装饰', () => {
  const body = fnBody(read('../src/shell/theme.js'), 'applyTheme');
  assert.match(
    body,
    /setWindowTheme\(theme\.value === 'system' \? null : resolvedTheme\.value\)/,
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

// ---- 主壳切主题时，已开着的副窗跟着换 ------------------------------------
//
// 2026-10-08 用户报「切换主题时，弹出的 window 也要跟着改变」。
// 根因不是样式，是**链路缺失**：主题真值在 localStorage，而 localStorage 的
// `storage` 事件**不跨 webview 生效**——四个副窗各自是独立 webview，主壳改完
// 存储，它们已经加载好的那份 `html.dark` 不会动。此前 `theme.js` 的注释把它
// 写成了「下一次绘制就跟着变」，那句只在**窗口还没开**时才成立。

test('切主题会广播给所有窗口（已开着的副窗靠它刷新）', () => {
  const body = fnBody(read('../src/shell/theme.js'), 'setTheme');
  assert.match(
    body,
    /broadcastTheme\(\s*next\s*\)/,
    'setTheme 必须广播新主题，否则已开着的副窗永远停在旧主题',
  );
  // 广播必须在 applyTheme 之后：先把自己这扇窗改对，再通知别人。
  assert.match(
    body,
    /applyTheme\(\);[\s\S]*?broadcastTheme\(\s*next\s*\);/,
    '顺序是「先 applyTheme 再广播」——反过来的话主窗会慢一帧',
  );
});

test('副窗收到广播后跑 applyTheme，内容与原生 chrome 同一步换', () => {
  const body = fnBody(read('../src/shell/theme.js'), 'followThemeBroadcast');
  assert.match(body, /subscribeThemeChanges\(/, '必须订阅主题广播');
  assert.match(body, /applyTheme\(\)/, '副窗要自己跑一遍 applyTheme');
  // 三步落地：真值、内容、原生 chrome。只推原生主题会只换标题栏、留下旧内容，
  // 正是上面那条判据警告的割裂，只是方向反过来。
  assert.match(body, /setThemeValue\(\s*next\s*\)/, '必须先改真值，否则内容与真值脱节');
  assert.doesNotMatch(
    body,
    /setWindowTheme\(\s*next\s*\)/,
    '不能只推原生主题：那样只换标题栏、内容留在旧主题',
  );
  // 事件可被同页脚本构造：非法值会让整窗落到既非 dark 也非浅色的主题上。
  assert.match(body, /isKnownTheme\(\s*next\s*\)/, '必须校验收到的主题值');
});

test('传输层不许碰主题真值（theme.js 与 themeSync.js 不能互相 import 成环）', () => {
  // theme.js 要调 themeSync 的 broadcastTheme；反向再 import 就是环。ESM 靠函数
  // 声明提升能扛住，但真值 `theme` 是 const（TDZ），谁先谁后哪天换个顺序，
  // 症状是「一进副窗就白屏」——那类 bug 极难定位，从结构上断掉。
  const sync = read('../src/shell/themeSync.js');
  assert.doesNotMatch(
    sync.replace(/\/\*[\s\S]*?\*\//g, '').replace(/^\s*\/\/.*$/gm, ''),
    /from '\.\/theme\.js'/,
    'themeSync 是传输层，不该 import 主题真值',
  );
  // 传输层也不该自己判断什么算合法主题——那是真值层的知识。
  assert.doesNotMatch(
    sync,
    /THEMES|isKnownTheme|'dark'|'light'/,
    '传输层不许自带主题白名单，否则与 theme.js 各有一份、必然漂',
  );
});

test('广播只有主壳发，副窗不调 setTheme（否则两扇副窗互相触发）', () => {
  const main = read('../src/main.js');
  assert.match(main, /followThemeBroadcast\(\)/, '入口必须订阅广播');
  // 订阅必须早于 createApp：晚一步副窗首帧会先画一帧旧主题再跳色。
  const subAt = main.indexOf('followThemeBroadcast()');
  const mountAt = main.indexOf('createApp(');
  assert.ok(subAt > 0 && subAt < mountAt, '订阅必须早于 createApp');
  // 四个副窗共用这一个入口，所以只调一次就覆盖所有窗口类型。
  assert.equal((main.match(/followThemeBroadcast\(\)/g) || []).length, 1);
  // setTheme（会广播）只在真值层导出，副窗拿不到；这一条防的是有人把
  // followThemeBroadcast 换成调 setTheme。
  assert.doesNotMatch(
    fnBody(read('../src/shell/theme.js'), 'followThemeBroadcast'),
    /setTheme\(/,
    '副窗落地不许走 setTheme（它会广播，形成回环）',
  );
});

test('emit 封装存在且不吞调用方的判断（纯浏览器调试下 resolve 空）', () => {
  const body = fnBody(read('../src/shell/bridge.js'), 'emit');
  assert.match(body, /tauriEvent/, '必须走 Tauri 的全局 emit');
  assert.match(body, /Promise\.resolve\(\)/, '桥接缺失时 resolve 空而不是抛');
});

test('壳自有副窗拿到窗口外观权限，内核页面与官网页签窗不拿', () => {
  // 壳自有：内容就是本应用的 SPA，主题由 localStorage 决定。
  for (const cap of ['default', 'log-viewer', 'usage-viewer', 'subscription-viewer']) {
    const perms = JSON.parse(read(`../../src-tauri/capabilities/${cap}.json`)).permissions;
    assert.ok(
      perms.includes(cap === 'default' ? 'allow-local-commands' : 'allow-window-appearance'),
      `${cap}.json 缺窗口级外观授权`,
    );
  }
  // 内容是别人的页面（内核 webui / chat.deepseek.com 等），恒深色：
  // 让它们跟随本应用主题只会把割裂换个方向。
  for (const cap of ['harness-remote', 'official-chat-remote', 'official-chat-strip']) {
    const perms = JSON.parse(read(`../../src-tauri/capabilities/${cap}.json`)).permissions;
    assert.ok(
      !perms.includes('allow-window-appearance') && !perms.includes('core:window:allow-set-theme'),
      `${cap}.json 不该开放原生外观设置：这扇窗的内容不是本应用的页面`,
    );
  }
});

test('工作台窗口仍钉死深色（内容是内核 webui，不跟本应用主题）', () => {
  assert.match(
    read('../../src-tauri/src/harness/harness_window.rs'),
    /\.theme\(crate::shell::appearance::initial_theme\(\)\)/,
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