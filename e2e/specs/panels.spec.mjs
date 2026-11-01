// 面板切换 + 真实 IPC 的集成路径（2026-11-01 OPT-09）。
//
// 只留少量稳定路径：切到「用量分析」后标题、内容区与 get_model_usage 的返回值
// 必须同时成立。任一条件不满足就失败，不用面板未挂载时的 skip 蒙混过去。

import { browser, expect } from '@wdio/globals';

const runAsync = (body) =>
  browser.executeAsync(
    'var done = arguments[arguments.length - 1]; (async () => {' +
      body +
      '})().then(done, (e) => done({ __error: String(e) }));'
  );

describe('面板切换与真实 IPC', function () {
  // 与 memory.spec 相同的测试通道修复：WebDriver 窗口不调度 rAF，Transition
  // mode="out-in" 的 leave 不结束。这里只在 E2E webview 里换成 setTimeout，
  // 产品代码与正式构建的动画路径不变。
  before(async () => {
    await browser.waitUntil(
      async () =>
        (await browser.execute(
          () =>
            typeof window.__TAURI__ !== 'undefined' && typeof window.document !== 'undefined'
        )) === true,
      { timeout: 30_000, timeoutMsg: '等待壳页面加载超时' }
    );
    await browser.execute(() => {
      window.requestAnimationFrame = (callback) => window.setTimeout(() => callback(Date.now()), 16);
      window.cancelAnimationFrame = (id) => window.clearTimeout(id);
    });
  });

  it('切到用量分析：标题、内容区与真实 IPC 数据都挂载', async () => {
    const clicked = await runAsync(
      "const items = [...document.querySelectorAll('.nav-item')];" +
        "const target = items.find((el) => el.textContent.trim() === '用量分析');" +
        'if (!target) return { labels: items.map((el) => el.textContent.trim()) };' +
        'target.click();' +
        'return { clicked: target.textContent.trim() };'
    );
    expect(clicked?.__error).toBeUndefined();
    expect(clicked?.clicked).toBe('用量分析');

    await browser.waitUntil(
      async () =>
        (await runAsync("return document.querySelector('h1.page-title')?.textContent.trim();")) ===
        '用量分析',
      { timeout: 30_000, timeoutMsg: '切到用量分析后页面标题没有挂载' }
    );
    await browser.waitUntil(
      async () =>
        (await runAsync('return !!document.querySelector(".usage-analysis-panel .usagewin");')) ===
        true,
      { timeout: 30_000, timeoutMsg: '用量分析里的模型用量内容没有挂载' }
    );
    // 数据区要等首扫；只等「壳挂上」会读到空加载态。
    await browser.waitUntil(
      async () =>
        (await runAsync('return !!document.querySelector(".usage-analysis-panel .usage-overview");')) ===
        true,
      { timeout: 60_000, timeoutMsg: '用量概览数据区没有挂载出来' }
    );

    const ipc = await runAsync(
      "return await window.__TAURI__.core.invoke('get_model_usage', { force: true, external: { codex: false, minimax: false }, externalForce: false });"
    );
    expect(ipc?.__error).toBeUndefined();
    expect(ipc).toBeTruthy();
    expect(typeof ipc.retention_days).toBe('number');
    expect(typeof ipc.total_tokens).toBe('number');
    expect(typeof ipc.today_tokens).toBe('number');
    expect(Array.isArray(ipc.days)).toBe(true);

    const dom = await runAsync(
      'return {' +
        ' title: document.querySelector(".usagewin-title")?.textContent.trim() || "",' +
        ' summaryCards: document.querySelectorAll(".usage-analysis-panel .usage-summary-card").length,' +
        ' sourceChips: document.querySelectorAll(".usage-analysis-panel .usage-source-chip").length' +
        '};'
    );
    expect(dom?.__error).toBeUndefined();
    expect(dom.title).toContain('模型用量');
    expect(dom.summaryCards).toBeGreaterThan(0);
    expect(dom.sourceChips).toBeGreaterThan(0);
  });
});
