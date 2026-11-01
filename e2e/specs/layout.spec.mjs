// 真实 WebView 布局检查（2026-11-01 OPT-10）。
//
// 静态 CSS 检查器只能回答「层叠后写了什么」，回答不了「元素实际在哪、有没有
// 被裁掉、按钮可不可点」。这里用真实壳的 computedStyle / getBoundingClientRect
// 看固定窗口（1040×748）下的代表性页面：概览主面板、记忆编辑弹窗。
//
// 失败必须带元素、期望值与实际值；不做整页像素快照，避免维护一大片会漂的图。

import { browser, expect } from '@wdio/globals';

const runAsync = (body) =>
  browser.executeAsync(
    'var done = arguments[arguments.length - 1]; (async () => {' +
      body +
      '})().then(done, (e) => done({ __error: String(e) }));'
  );

function failWith(label, expected, actual) {
  throw new Error(label + '：期望 ' + JSON.stringify(expected) + '，实际 ' + JSON.stringify(actual));
}

describe('真实 WebView 布局', function () {
  before(async () => {
    await browser.waitUntil(
      async () =>
        (await browser.execute(
          () => typeof window.__TAURI__ !== 'undefined' && typeof window.document !== 'undefined'
        )) === true,
      { timeout: 30_000, timeoutMsg: '等待壳页面加载超时' }
    );
    await browser.execute(() => {
      window.requestAnimationFrame = (callback) => window.setTimeout(() => callback(Date.now()), 16);
      window.cancelAnimationFrame = (id) => window.clearTimeout(id);
    });
  });

  async function clickNav(label) {
    const clicked = await runAsync(
      'const items = [...document.querySelectorAll(".nav-item")];' +
        'const target = items.find((el) => el.textContent.trim().startsWith(' + JSON.stringify(label) + '));' +
        'if (!target) return { labels: items.map((el) => el.textContent.trim()) };' +
        'target.click();' +
        'return { clicked: target.textContent.trim() };'
    );
    expect(clicked?.__error).toBeUndefined();
    expect(clicked?.clicked).toBe(label);
  }

  async function waitTitle(title) {
    await browser.waitUntil(
      async () =>
        (await runAsync("return document.querySelector('h1.page-title')?.textContent.trim();")) ===
        title,
      { timeout: 30_000, timeoutMsg: '页面标题没有挂载：' + title }
    );
  }

  it('概览主面板在 1040×748 内没有横向溢出，关键动作可达', async () => {
    await clickNav('概览');
    await waitTitle('概览');
    await browser.waitUntil(
      async () =>
        (await runAsync('return !!document.querySelector(".overview-grid");')) === true,
      { timeout: 30_000, timeoutMsg: '概览主栅格没有挂载' }
    );
    // 面板切换是 Transition out-in：栅格先出现时卡片子树还在下一拍挂载，
    // 直接量会拿到 null（2026-11-01 E2E 实测）。等卡片真的在 DOM 里再量。
    await browser.waitUntil(
      async () =>
        (await runAsync('return document.querySelectorAll(".overview-grid .card").length > 0;')) ===
        true,
      { timeout: 30_000, timeoutMsg: '概览卡片没有挂载' }
    );

    const measure = await runAsync(
      'const main = document.querySelector("main");' +
        'const card = document.querySelector(".overview-grid .card");' +
        'const actions = document.querySelector(".overview-grid .kernel-header-actions");' +
        'const rect = (el) => {' +
        ' if (!el) return null;' +
        ' const r = el.getBoundingClientRect();' +
        ' return { x: r.x, y: r.y, width: r.width, height: r.height, top: r.top, right: r.right, bottom: r.bottom, left: r.left };' +
        '};' +
        'actions?.scrollIntoView({ block: "center" });' +
        'const a = rect(actions);' +
        'return {' +
        ' viewport: { w: window.innerWidth, h: window.innerHeight },' +
        ' main: { scrollWidth: main.scrollWidth, clientWidth: main.clientWidth },' +
        ' cardsNow: document.querySelectorAll(".overview-grid .card").length,' +
        ' gridsNow: document.querySelectorAll(".overview-grid").length,' +
        ' titleNow: document.querySelector("h1.page-title")?.textContent,' +
        ' card: rect(card),' +
        ' actions: a,' +
        ' actionsVisible: !!a && a.width > 0 && a.height > 0 && a.right <= window.innerWidth + 1 && a.bottom <= window.innerHeight + 1' +
        '};'
    );
    expect(measure?.__error).toBeUndefined();
    expect(measure.viewport.w).toBeGreaterThanOrEqual(1000);
    // 主区域横向溢出 = 右侧内容被固定窗口裁掉，用户既看不到也点不到。
    if (measure.main.scrollWidth > measure.main.clientWidth + 1) {
      failWith('main 横向溢出', { scrollWidth: measure.main.clientWidth }, measure.main);
    }
    if (!measure.card) failWith('概览卡片缺失', '存在 .overview-grid .card', null);
    if (measure.card.right > measure.viewport.w + 1) {
      failWith('卡片右缘超出窗口', measure.viewport.w, measure.card);
    }
    if (!measure.actionsVisible) {
      failWith('内核动作按钮不可达', '按钮完整落在视口内', measure.actions);
    }
  });

  it('浅色 / 深色主题都应用 token，且切换不改变卡片宽度', async () => {
    const result = await runAsync(
      'const html = document.documentElement;' +
        'const wasDark = html.classList.contains("dark");' +
        'const card = document.querySelector(".overview-grid .card");' +
        'const beforeWidth = card?.getBoundingClientRect().width || 0;' +
        'html.classList.remove("dark");' +
        'const lightColor = getComputedStyle(card).color;' +
        'const lightBg = getComputedStyle(card).backgroundColor;' +
        'html.classList.add("dark");' +
        'const darkColor = getComputedStyle(card).color;' +
        'const darkBg = getComputedStyle(card).backgroundColor;' +
        'const darkWidth = card?.getBoundingClientRect().width || 0;' +
        'html.classList.toggle("dark", wasDark);' +
        'return { lightColor, lightBg, darkColor, darkBg, beforeWidth, darkWidth };'
    );
    expect(result?.__error).toBeUndefined();
    if (!(result.lightColor !== result.darkColor || result.lightBg !== result.darkBg)) {
      failWith('深浅主题的卡片颜色没有变化', '至少一项 token 生效', result);
    }
    if (Math.abs(result.beforeWidth - result.darkWidth) > 1) {
      failWith('切换主题改变了卡片宽度', result.beforeWidth, result.darkWidth);
    }
  });

  it('记忆编辑弹窗的页脚按钮在视口内可点，长文本不横向溢出', async () => {
    await clickNav('记忆');
    await waitTitle('记忆');
    const opened = await runAsync(
      'const button = [...document.querySelectorAll("button")].find((el) => el.textContent.trim() === "新建记忆");' +
        'if (!button) return { found: false };' +
        'button.click();' +
        'return { found: true };'
    );
    expect(opened?.__error).toBeUndefined();
    expect(opened?.found).toBe(true);
    await browser.waitUntil(
      async () => (await runAsync('return !!document.querySelector(".memory-dialog");')) === true,
      { timeout: 30_000, timeoutMsg: '记忆编辑弹窗没有挂载' }
    );
    const dialog = await runAsync(
      'const el = document.querySelector(".memory-dialog");' +
        'const footer = el.querySelector(".el-dialog__footer");' +
        'el.scrollIntoView({ block: "nearest" });' +
        'const rect = footer.getBoundingClientRect();' +
        'const buttons = [...footer.querySelectorAll("button")].map((b) => {' +
        ' const r = b.getBoundingClientRect();' +
        ' return { text: b.textContent.trim(), right: r.right, bottom: r.bottom, width: r.width };' +
        '});' +
        'return { dialogRight: el.getBoundingClientRect().right, footer: { right: rect.right, bottom: rect.bottom }, buttons, viewport: { w: window.innerWidth, h: window.innerHeight } };'
    );
    expect(dialog?.__error).toBeUndefined();
    if (dialog.dialogRight > dialog.viewport.w + 1) {
      failWith('弹窗右缘超出窗口', dialog.viewport.w, dialog.dialogRight);
    }
    if (dialog.buttons.length < 2) failWith('弹窗页脚按钮不足', 2, dialog.buttons);
    for (const button of dialog.buttons) {
      if (button.right > dialog.viewport.w + 1 || button.bottom > dialog.viewport.h + 1 || button.width <= 0) {
        failWith('弹窗页脚按钮不可达：' + button.text, '完整落在视口内', button);
      }
    }
    // 关闭弹窗，避免影响后续用例。
    await runAsync(
      'const button = [...document.querySelectorAll(".el-dialog__footer button")].find((el) => el.textContent.trim() === "取消");' +
        'button?.click();' +
        'return { closed: !!button };'
    );
  });

  it('布局检测器能抓到故意注入的横向溢出（反向验证）', async () => {
    const detected = await runAsync(
      'const main = document.querySelector("main");' +
        'const probe = document.createElement("div");' +
        'probe.id = "layout-probe";' +
        'probe.style.width = "3000px";' +
        'probe.style.height = "20px";' +
        'main.appendChild(probe);' +
        'const overflow = main.scrollWidth > main.clientWidth + 1;' +
        'probe.remove();' +
        'return { overflow, scrollWidth: main.scrollWidth, clientWidth: main.clientWidth };'
    );
    expect(detected?.__error).toBeUndefined();
    if (!detected.overflow) {
      failWith('检测器没有发现注入的横向溢出', 'overflow=true', detected);
    }
  });
});
