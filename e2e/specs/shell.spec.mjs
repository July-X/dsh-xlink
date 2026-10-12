// 主壳可调试性的冒烟用例。
//
// 这一组断言针对的是 2026-10-11 实测到的那个事实：在纯浏览器里打开 `ui/`
// （`npm run dev:ui`），面板能渲染、能点，但 **69 个 Tauri 命令全部 reject**，
// `bridge.js` 把失败吞成 toast，console 里一条痕迹都没有——于是「面板起来了」
// 与「面板拿到了数据」是两件完全不同的事，而前者会骗过所有只看截图的检查。
//
// 所以这里不复用「面板渲染出来了」这种弱判据，而是直接问后端要数据：
// 在真实 app 的 webview 里调 `invoke('get_status')`，要求它**解析成功**并带回
// 真实字段。桥要是没通，这一条就红。

import { browser, expect, $ } from '@wdio/globals';

/** 在页面里跑一段 async 脚本并拿回值（跨 WebDriver 边界只能这么取值）。 */
const runAsync = (body) =>
  browser.executeAsync(`var done = arguments[arguments.length - 1]; (async () => { ${body} })().then(done, (e) => done({ __error: String(e) }));`);

describe('主壳管理面板', function () {
  it('渲染出侧栏导航', async () => {
    const nav = await $('.sidebar__nav');
    await nav.waitForExist({ timeout: 60_000 });
    // 读容器文本而不是 `$$(...)` 后遍历：WDIO 的 `$$` 返回的元素数组不是
    // 普通数组（直接 `Promise.all(els.map(…))` 会报 "object is not iterable"）。
    const text = await nav.getText();
    expect(text).toContain('概览');
    expect(text).toContain('内核版本');
  });

  it('Tauri 桥真的通：get_status 能从 Rust 侧取回真实数据', async () => {
    const status = await runAsync(`
      return await window.__TAURI__.core.invoke('get_status');
    `);

    // 桥没通时这里拿到的是 reject 文案，不是对象。
    expect(status?.__error).toBeUndefined();
    expect(status).toBeTruthy();
    expect(typeof status.kernel).toBe('object');
    expect(typeof status.node).toBe('object');

    // shell_version 来自 tauri.conf.json，非空即证明读到的是真配置。
    expect(typeof status.shell_version).toBe('string');
    expect(status.shell_version.length).toBeGreaterThan(0);

    // dev_build 为 true 同时钉住两件事：跑的是 debug 构建（插件存在的前提），
    // 以及面板拿到的是**本进程**的状态而不是某个缓存快照。
    expect(status.dev_build).toBe(true);
  });

  it('没有把 IPC 失败吞成 toast', async () => {
    // bridge.js:21 的 reject 文案。桥断了它会连着弹好几条；这里读 DOM 而不读
    // console——那些失败是被 catch 掉的，console 本来就什么都没有。
    const body = await browser.execute(() => document.body.innerText);
    expect(body).not.toContain('Tauri bridge 未注入');
  });
});