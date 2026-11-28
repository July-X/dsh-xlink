import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import test from 'node:test';

// 运行期不重算原生装饰；窗口外观与内容主题各自同步。
const read = (p) => readFileSync(new URL(p, import.meta.url), 'utf8');

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

function fnBody(source, name) {
  const m = source.match(new RegExp(`function ${name}\\s*\\([^)]*\\)\\s*\\{([\\s\\S]*?)\\n\\}`));
  assert.ok(m, `必须能找到 ${name} 的定义`);
  return stripJs(m[1]);
}

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

test('窗口装饰只在建窗期给：不许运行时 set_decorations', () => {
  // tao 的
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

// ---- 壳偏好交给工作台（2026-11-02 用户定案「B」） --------------------------
//
// 内核 `ui-theme = system` 的含义是「跟随外壳」。主壳把偏好报给 Rust，工作台的
// 注入脚本只在系统档采用它；内核固定 light/dark 时用户的选择优先。

test('主壳偏好上报：setTheme 每次报、启动由主壳报一次、副窗不报', () => {
  const setTheme = fnBody(read('../src/shell/theme.js'), 'setTheme');
  assert.match(
    setTheme,
    /reportThemePreference\(\)/,
    '切主题要把偏好交给 Rust，否则工作台的 system 档跟不到外壳',
  );
  // 先落地、再上报、最后广播副窗：工作台不该等副窗那条事件。
  assert.match(
    setTheme,
    /applyTheme\(\);[\s\S]*?reportThemePreference\(\);[\s\S]*?broadcastTheme\(\s*next\s*\);/,
    '顺序是「applyTheme → 上报壳偏好 → 广播副窗」',
  );
  const report = fnBody(read('../src/shell/theme.js'), 'reportThemePreference');
  assert.match(
    report,
    /reportShellTheme\(theme\.value === 'system' \? null : theme\.value\)/,
    'system 必须传 null：钉住当时解析出来的那一档，会让系统级切换失效',
  );
  const bridge = fnBody(read('../src/shell/bridge.js'), 'reportShellTheme');
  assert.match(
    bridge,
    /invoke\('report_shell_theme',\s*\{\s*theme\s*\}\)/,
    '桥必须指到 Rust 里已注册的 report_shell_theme',
  );
  const main = read('../src/main.js');
  assert.match(main, /if \(isMainShell\) reportThemePreference\(\);/, '主壳启动要上报一次');
  assert.match(
    main,
    /const isMainShell = !isLogViewer && !isChatStrip && !isUsageViewer && !isSubscriptionViewer;/,
    '副窗 / 页签栏不许上报：它们的真值来自主壳广播，报了也是同一个值',
  );
});

test('工作台外观镜像：内核固定档优先，内核 system 时才用壳偏好', () => {
  const script = read('../../src-tauri/src/harness/workbench-appearance.js');
  assert.match(script, /getAttribute\(SOURCE_ATTRIBUTE\)/, '必须读内核发布的 ui-theme 偏好');
  assert.match(
    script,
    /if \(source !== 'system'\) \{\s*return source;/,
    '内核固定 light/dark 优先，壳主题不许覆盖用户在内核里的明确选择',
  );
  assert.match(
    script,
    /if \(shell === undefined\) \{\s*return undefined;/,
    '壳还没上报时不许动窗口：把「没答案」当成 system 会冻掉内核的固定档',
  );
  assert.match(script, /return shell;/, '内核 system 时采用壳偏好（null = 跟随系统）');
  assert.match(
    script,
    /invoke\('get_shell_theme', \{\}\)/,
    '要调用 get_shell_theme，且带对象参数——ACL 判据按 invoke(cmd, { ... }) 的字面形状扫',
  );
  assert.match(
    script,
    /addEventListener\('dsh-shell-theme'/,
    '壳切主题时靠 Rust eval 推来的 DOM 事件立即生效，不必等下一拍轮询',
  );
  assert.doesNotMatch(
    script,
    /__TAURI__\.event/,
    '工作台是 remote origin：Tauri event 的 listen 要 core:event 授权，漏了是静默失败',
  );
  assert.match(script, /invoke\('set_window_appearance', \{ theme: target \}\)/, '下发仍走 set_window_appearance');

  // 推送端与 Windows 的时序修正都在 appearance.rs：
  // 壳自有窗口在 Windows 上下发窗口主题也会改共享 WebView2 profile，把内核页面
  // 提前改色、标题栏却还没动——两半一前一后正是用户报的观感问题。
  const rust = read('../../src-tauri/src/shell/appearance.rs');
  assert.match(
    rust,
    /pub const SHELL_THEME_EVENT: &str = "dsh-shell-theme"/,
    '事件名两端必须是同一个字面量',
  );
  assert.match(
    rust,
    /for label in \[WORKBENCH_LABEL, MCODE_WORKBENCH_LABEL\]\s*\{\s*if let Some\(window\) = app\.get_webview\(label\) \{\s*let _ = window\.eval\(&script\)/,
    '壳偏好要按稳定 WebView label 直接推给两种工作台，不等轮询',
  );
  assert.match(
    rust,
    /CustomEvent\('\{SHELL_THEME_EVENT\}'/,
    '推送的事件名必须与脚本监听的名字一致',
  );
  assert.match(rust, /fn applies_native_theme\(label: &str, workbench_open: bool\)/, 'Windows 壳自有窗口的跳过判据');
  assert.match(
    rust,
    /cfg!\(target_os = "windows"\) && workbench_open && owns_appearance\(label\)/,
    '跳过条件必须同时限平台、工作台是否开着与窗口族，漏一个就会把工作台或启动时的对齐一起跳过',
  );
});

test('加载态工作台页跟随壳主题：同一个偏好键、同一个切换事件、深色是兜底', () => {
  const page = read('../public/kernel-loading.html');
  const key = /const STORAGE_KEY = '([^']+)'/.exec(read('../src/shell/theme.js'))?.[1];
  assert.ok(key, '必须能从 theme.js 读到偏好键');
  assert.ok(
    page.includes(`'${key}'`),
    '加载态页要读同一个偏好键：键名漂了就一直停在深色兜底，且没有任何报错',
  );
  assert.match(
    page,
    /addEventListener\('dsh-shell-theme'/,
    '运行中切主题靠 Rust 推来的 DOM 事件立即重画，与 workbench-appearance.js 同一个事件名',
  );
  assert.match(page, /prefers-color-scheme: dark/, 'system 档按系统配色解析');
  assert.match(page, /html\.light body\s*\{[^}]*background: #ffffff/, '浅色偏好要有自己的底色');
  assert.match(
    page,
    /body \{\s*background: #16171a/,
    '默认保持深色：脚本失败或存储不可用时退回现状，不能白屏',
  );
});

test('壳主题上报链路两端都有授权：命令已注册、本地白名单、工作台可读', () => {
  const lib = read('../../src-tauri/src/lib.rs');
  assert.match(lib, /shell::appearance::report_shell_theme/);
  assert.match(lib, /shell::appearance::get_shell_theme/);
  const perms = JSON.parse(read('../../src-tauri/permissions/app-commands.json')).permission;
  const local = perms.find((p) => p.identifier === 'allow-local-commands').commands.allow;
  for (const command of ['report_shell_theme', 'get_shell_theme']) {
    assert.ok(
      local.includes(command),
      `${command} 必须列入 allow-local-commands：check-invariants 第 1 项按它核对注册表`,
    );
  }
  const remote = perms.find((p) => p.identifier === 'allow-get-shell-theme');
  assert.ok(remote?.commands.allow.includes('get_shell_theme'), 'get_shell_theme 要有独立权限声明');
  const harness = JSON.parse(read('../../src-tauri/capabilities/harness-remote.json'));
  assert.ok(
    harness.permissions.includes('allow-get-shell-theme'),
    '工作台是 remote origin，漏授权就是 ACL 静默拒 + .catch 吞掉',
  );
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

test('官网页签栏独立挂载，首帧使用深色且不订阅壳主题广播', () => {
  const main = stripJs(read('../src/main.js'));
  assert.match(main, /const isChatStrip = params\.has\('chatstrip'\);/);
  assert.match(main, /:\s*isChatStrip\s*\?\s*OfficialChatTabs\b/);
  assert.match(main, /if \(isChatStrip\) setThemeValue\('dark'\);\s*applyTheme\(\);/);
  assert.match(main, /if \(!isChatStrip\) followThemeBroadcast\(\);/);
});

test('emit 封装存在且不吞调用方的判断（纯浏览器调试下 resolve 空）', () => {
  const body = fnBody(read('../src/shell/bridge.js'), 'emit');
  assert.match(body, /tauriEvent/, '必须走 Tauri 的全局 emit');
  assert.match(body, /Promise\.resolve\(\)/, '桥接缺失时 resolve 空而不是抛');
});

test('壳自有副窗与工作台拿到窗口外观权限，官网页签窗不拿', () => {
  // 壳自有：内容就是本应用的 SPA，主题由 localStorage 决定；工作台由
  // workbench-appearance.js 镜像外观——内核固定档听内核，内核 system 听壳上报的偏好。
  for (const cap of ['default', 'log-viewer', 'usage-viewer', 'subscription-viewer']) {
    const perms = JSON.parse(read(`../../src-tauri/capabilities/${cap}.json`)).permissions;
    assert.ok(
      perms.includes(cap === 'default' ? 'allow-local-commands' : 'allow-window-appearance'),
      `${cap}.json 缺窗口级外观授权`,
    );
  }
  const harnessPerms = JSON.parse(read('../../src-tauri/capabilities/harness-remote.json')).permissions;
  assert.ok(
    harnessPerms.includes('allow-window-appearance'),
    'harness-remote.json 缺工作台外观镜像所需的窗口级授权',
  );
  assert.ok(
    harnessPerms.includes('allow-get-shell-theme'),
    'harness-remote.json 缺读取壳偏好的授权：内核 system 档会跟不到外壳主题',
  );
  // 官网页签内容是别人的页面，恒深色；让它跟随本应用主题只会把割裂换个方向。
  for (const cap of ['official-chat-remote', 'official-chat-strip']) {
    const perms = JSON.parse(read(`../../src-tauri/capabilities/${cap}.json`)).permissions;
    assert.ok(
      !perms.includes('allow-window-appearance') && !perms.includes('core:window:allow-set-theme'),
      `${cap}.json 不该开放原生外观设置：这扇窗的内容不是本应用的页面`,
    );
  }
});

test('弹出宿主按文档标签选主题，两个工作台都注入外观镜像', () => {
  const surface = stripRs(read('../../src-tauri/src/harness/session_surface.rs'));
  assert.match(surface, /\.theme\(crate::shell::appearance::initial_theme\(kind\.label\(\)\)\)/);
  for (const file of ['harness_window', 'mcode_window']) {
    const source = stripRs(read(`../../src-tauri/src/harness/${file}.rs`));
    assert.match(source, /\.initialization_script\(include_str!\("workbench-appearance\.js"\)\)/, `${file} 缺外观镜像`);
  }
});
