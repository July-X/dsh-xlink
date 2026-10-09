import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const source = readFileSync(new URL('../src/shell/theme.js', import.meta.url), 'utf8');
const code = source
  .replace(/\/\*[\s\S]*?\*\//g, '')
  .replace(/\/\/.*$/gm, '');

test('主题只用 html.dark 作为暗色判据', () => {
  assert.match(
    code,
    /document\.documentElement\.classList\.toggle\('dark',\s*resolvedTheme\.value === 'dark'\)/,
    '主题切换必须落到 html.dark'
  );
  assert.doesNotMatch(
    code,
    /document\.documentElement\.dataset|data-theme/,
    '不能再写入第二套 data-theme 判据'
  );
});

// 隔离窗口与存储，验证系统变化、固定模式、持久化与跨窗事件。
import { runInNewContext } from 'node:vm';
function loadTheme(stored = 'dark', dark = false, nativeOverride = false) {
  let listener, subscriber;
  let mediaDark = dark;
  const media = { get matches() { return mediaDark; }, addEventListener: (_, cb) => { listener = cb; } };
  const colors = [], native = [], broadcasts = [], writes = [];
  const context = {
    ref: (value) => ({ value }),
    computed: (get) => ({ get value() { return get(); } }),
    window: {
      localStorage: { getItem: () => stored, setItem: (...args) => writes.push(args) },
      matchMedia: () => media,
    },
    document: { documentElement: { classList: { toggle: (_, value) => colors.push(value) } } },
    setWindowTheme: (value) => { native.push(value); if (nativeOverride) mediaDark = value === null ? dark : value === 'dark'; return Promise.resolve(); },
    broadcastTheme: (value) => broadcasts.push(value),
    subscribeThemeChanges: (cb) => { subscriber = cb; },
  };
  runInNewContext(source.replace(/^import .*$/gm, '').replace(/export /g, '') + '\nthis.api = { theme, resolvedTheme, applyTheme, setTheme, followThemeBroadcast };', context);
  return { ...context.api, colors, native, broadcasts, writes, change: (matches) => { mediaDark = matches; listener({ matches }); }, receive: (value) => subscriber(value) };
}

test('跟随系统读取启动配色，变化时页面与原生装饰同步，保留 system 偏好', () => {
  const app = loadTheme('system', true);
  app.applyTheme();
  assert.equal(app.colors.at(-1), true);
  app.change(false);
  assert.equal(app.colors.at(-1), false);
  assert.equal(app.native.at(-1), null);
  assert.equal(app.theme.value, 'system');
  assert.equal(app.writes.length, 0);
});

test('固定模式忽略系统切换，改为跟随系统立即采用最新系统配色并广播模式', () => {
  const app = loadTheme('dark');
  app.applyTheme();
  app.change(true);
  app.change(false);
  assert.equal(app.colors.length, 1);
  app.setTheme('system');
  assert.equal(app.native.at(-1), null);
  assert.equal(app.writes.at(-1)[1], 'system');
  assert.equal(app.broadcasts.at(-1), 'system');
  app.setTheme('invalid');
  assert.equal(app.theme.value, 'system');
});

test('副窗接收 system 后自行跟随配色，不再次广播；旧 light 偏好保留', () => {
  const app = loadTheme('light', true);
  assert.equal(app.theme.value, 'light');
  app.followThemeBroadcast();
  app.receive('system');
  assert.equal(app.native.at(-1), null);
  app.change(false);
  assert.equal(app.native.at(-1), null);
  assert.equal(app.broadcasts.length, 0);
});


test('原生窗口覆盖了媒体查询时，跟随系统先清除覆盖，再重新读取配色', async () => {
  const app = loadTheme('light', true, true);
  app.applyTheme();
  await Promise.resolve();
  app.setTheme('system');
  await Promise.resolve();
  assert.equal(app.native.at(-1), null, 'system 必须解除原生主题覆盖');
  assert.equal(app.colors.at(-1), true, '系统深色不能被先前的浅色覆盖');
  assert.equal(app.theme.value, 'system');
});


test('解除原生覆盖的异步响应不能覆盖随后选择的固定深色', async () => {
  const app = loadTheme('light', false, true);
  app.setTheme('system');
  app.setTheme('dark');
  await Promise.resolve();
  assert.equal(app.theme.value, 'dark');
  assert.equal(app.colors.at(-1), true);
  assert.equal(app.native.at(-1), 'dark');
});

// 建窗主题在 macOS 是进程级覆盖，会使所有媒体查询失去系统变化。
test('macOS 建窗与运行时主题必须经过窗口作用域适配器', () => {
  const bridge = readFileSync(new URL('../src/shell/bridge.js', import.meta.url), 'utf8');
  assert.match(bridge, /invoke\('set_window_appearance'/);
  const config = JSON.parse(readFileSync(new URL('../../src-tauri/tauri.conf.json', import.meta.url)));
  assert.equal(config.app.windows[0].theme, undefined);
  for (const path of ['commands.rs', 'harness/harness_window.rs', 'usage/local.rs', 'usage/subscription.rs']) {
    const rust = readFileSync(new URL('../../src-tauri/src/' + path, import.meta.url), 'utf8');
    assert.doesNotMatch(rust, /\.theme\(Some\(tauri::Theme::Dark\)\)/);
  }
});

test('外观命令只授权壳页面，禁止应用级 setTheme 入口', () => {
  for (const name of ['default', 'log-viewer', 'usage-viewer', 'subscription-viewer']) {
    const cap = JSON.parse(readFileSync(new URL('../../src-tauri/capabilities/' + name + '.json', import.meta.url)));
    assert(!cap.permissions.includes('core:window:allow-set-theme'));
  }
  const rust = readFileSync(new URL('../../src-tauri/src/shell/appearance.rs', import.meta.url), 'utf8');
  assert.match(rust, /setAppearance: nil/);
  assert.match(rust, /msg_send!\[native, setAppearance: appearance\]/);
});

test('前端主题桥接通过当前窗口命令设置固定和跟随模式，不调用应用级 API', async () => {
  const calls = [];
  const context = { window: { __TAURI__: {
    core: { invoke: (name, args) => { calls.push({ name, theme: args.theme }); return Promise.resolve(); } },
    window: { getCurrentWindow: () => ({ setTheme: () => { throw new Error('不得调用应用级主题 API'); } }) },
  } } };
  const bridge = readFileSync(new URL('../src/shell/bridge.js', import.meta.url), 'utf8');
  runInNewContext(bridge.replace(/export /g, '') + '\nthis.apply = setWindowTheme;', context);
  for (const theme of ['dark', 'light', null]) await context.apply(theme);
  assert.deepEqual(calls, ['dark', 'light', null].map(theme => ({ name: 'set_window_appearance', theme })));
});
