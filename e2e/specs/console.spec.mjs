// 控制台日志通道的端到端判据。
//
// 主壳的 console 对 agent 曾是完全盲的：embedded WKWebView 上没有
// `browser.getLogs`（W3C 已移除），而壳的诊断信息大量只进 console。通道本体
// 是 `ui/src/shell/consoleLog.js`（`window.__DSH_CONSOLE__` 环形缓冲），这里
// 钉三件事：钩子在、页面 console 进得来、异步炸点（未处理拒绝）也进得来。
import { browser, expect } from '@wdio/globals';

describe('控制台日志通道（window.__DSH_CONSOLE__）', () => {
  it('入口装好缓冲：壳窗口里全局钩子存在且可 dump', async () => {
    const shape = await browser.execute(() => ({
      installed: typeof window.__DSH_CONSOLE__ === 'object',
      hasDump: typeof window.__DSH_CONSOLE__?.dump === 'function',
      hasItems: Array.isArray(window.__DSH_CONSOLE__?.items),
    }));
    expect(shape.installed).toBe(true);
    expect(shape.hasDump).toBe(true);
    expect(shape.hasItems).toBe(true);
  });

  it('页面 console 进得来：实参、层级、透传三对齐', async () => {
    // execute 跑在页面主世界，console 就是页面那个——组件报错走的是同一条路。
    await browser.execute(() => console.error('e2e-console-canary', { code: 7 }));

    const entry = await browser.execute(() => {
      const { items } = window.__DSH_CONSOLE__.dump();
      return items.filter((item) => item.message.includes('e2e-console-canary')).pop() || null;
    });
    expect(entry).toBeTruthy();
    expect(entry.level).toBe('error');
    expect(entry.count).toBe(1);
    expect(entry.message).toContain('{"code":7}');
  });

  it('未处理拒绝也进通道：agent 看得见异步炸点', async () => {
    // 只制造拒绝、不返回被拒的 Promise（否则 execute 自己会 reject）。
    await browser.execute(() => {
      void Promise.reject(new Error('e2e-unhandled-canary'));
    });
    // unhandledrejection 异步到达，等一拍再读。
    await browser.pause(300);

    const found = await browser.execute(() => {
      const { items } = window.__DSH_CONSOLE__.dump();
      return items.some((item) => item.message.includes('e2e-unhandled-canary'));
    });
    expect(found).toBe(true);
  });
});
