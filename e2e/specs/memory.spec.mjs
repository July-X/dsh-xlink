// 记忆面板的冒烟用例：面板能在真壳里渲染，且它的命令真的通到 Rust。
//
// 与 `shell.spec.mjs` 同一条纪律：**不拿「面板渲染出来了」当判据**。记忆面板的每
// 一块内容都来自命令（状态 / 列表 / 工作区），桥断了的话页面照样画出来、只是全空
// ——所以这里直接在 webview 里调那三条命令并要求解析成功。
//
// 只覆盖「渲染 + 命令通」：检索质量、注入行为在 Rust 单测与内置插件的用例里，
// 这一层再测一遍会变成第二份实现。

import { browser, expect, $ } from '@wdio/globals';

/** 在页面里跑一段 async 脚本并拿回值（跨 WebDriver 边界只能这么取值）。 */
const runAsync = (body) =>
  browser.executeAsync(
    `var done = arguments[arguments.length - 1]; (async () => { ${body} })().then(done, (e) => done({ __error: String(e) }));`
  );

describe('记忆面板', function () {
  it('侧栏里有记忆项，点它会被标成当前项', async () => {
    const nav = await $('.sidebar__nav');
    await nav.waitForExist({ timeout: 60_000 });
    // 在页面里按文案点：WDIO 的元素句柄在面板切换后会指向被移除的节点，而侧栏项
    // 是 `v-for` 出来的——用一次 `executeAsync` 找元素并点击最稳。
    const clicked = await runAsync(`
      const items = [...document.querySelectorAll('.nav-item')];
      const target = items.find((el) => el.textContent.trim().startsWith('记忆'));
      if (!target) return { labels: items.map((el) => el.textContent.trim()) };
      target.click();
      return { clicked: target.textContent.trim() };
    `);
    expect(clicked?.__error).toBeUndefined();
    // 断言不成立时把侧栏现有文案带出来：`expect-webdriverio` 的 expect 不收第二个
    // 参数（多给一个会报「Expect takes at most one argument」），所以自己抛。
    if (!clicked.clicked) {
      throw new Error(`侧栏里没有「记忆」项，现有：${JSON.stringify(clicked.labels)}`);
    }
    await browser.waitUntil(
      async () =>
        (
          await runAsync(
            `return [...document.querySelectorAll('.nav-item.is-active')].map((el) => el.textContent.trim());`
          )
        ).includes('记忆'),
      { timeout: 20_000, timeoutMsg: '点了侧栏「记忆」之后它没有被标成当前项' }
    );

    // **面板内容在这条通道里验不了，原因已实测**：WebDriver 拉起的窗口在后台，
    // WKWebView 不跑 requestAnimationFrame，而 `App.vue` 的面板切换是
    // `<Transition mode="out-in">`——leave 永不结束，新面板就不挂载。**每个面板
    // 都一样**（设置 / 插件实测同样停在 `panel-leave-active`、标题仍是「概览」），
    // 所以这不是记忆面板的问题；注入「禁用过渡」的样式、手动派发 transitionend、
    // 用 osascript 把窗口提到前台，三种办法都试过，都不解决。
    // 内容渲染因此由下面那条命令断言 + Rust 侧的检索用例覆盖，不在这里假装验过。
  });

  it('记忆命令在真壳里都能解析（桥真的通，参数个数也对得上）', async () => {
    const status = await runAsync(`return await window.__TAURI__.core.invoke('memory_status');`);
    expect(status?.__error).toBeUndefined();
    expect(typeof status.file).toBe('string');
    expect(status.file).toContain('memory.json');

    const list = await runAsync(
      `return await window.__TAURI__.core.invoke('memory_list', { filter: { include_candidates: true, include_disabled: true } });`
    );
    expect(list?.__error).toBeUndefined();
    expect(Array.isArray(list.memories)).toBe(true);
    expect(typeof list.status.generation).toBe('number');

    const workspaces = await runAsync(`return await window.__TAURI__.core.invoke('memory_workspaces');`);
    expect(workspaces?.__error).toBeUndefined();
    expect(Array.isArray(workspaces)).toBe(true);

    // 「最近注入」那条命令也要真的通：它读的是**壳进程内**的环形记录（不是盘上那份），
    // 而面板的卡片就挂在它上面——ACL 漏一条时前端只会 `.catch` 静默吞掉，面板上
    // 那片区域永远是空的，且没有任何报错。
    const recent = await runAsync(
      `return await window.__TAURI__.core.invoke('memory_recent_injections');`
    );
    expect(recent?.__error).toBeUndefined();
    expect(Array.isArray(recent)).toBe(true);

    // 「提取任务」那条命令（2026-10-15 新增）：同样是**只有真壳能验**的一条——
    // 它要过 `allow-local-commands` 的 ACL，漏登记时前端只会 `.catch` 静默吞掉，
    // 面板那张卡片永远停在「还没读到提取任务的状态」。
    const jobs = await runAsync(`return await window.__TAURI__.core.invoke('memory_jobs');`);
    expect(jobs?.__error).toBeUndefined();
    expect(typeof jobs.summary.pending).toBe('number');
    expect(Array.isArray(jobs.jobs)).toBe(true);

    // 开关的「处理中」判据（E6）也要真的下发：`effect_pending` 由壳侧两个进程内时刻算出来，
    // 字段名对不上时面板会一直显示「还没读到状态」，而那看起来像正常的初始态。
    expect(typeof status.effect_pending).toBe('boolean');

    // 批准那条命令的**参数个数**也要在真壳里对一次：它加了 `scope`（§6.1 由用户当场选范围），
    // 而参数名对不上时前端只会 `.catch` 吞掉。用一个不存在的 id 调用——要的是「找不到候选」
    // 这类**业务**错误，不是「参数不对」或「命令未授权」，而且它不产生任何副作用。
    const decided = await runAsync(
      `return await window.__TAURI__.core.invoke('memory_decide_candidate', { id: 'no-such-candidate', accept: false, scope: null });`
    );
    expect(String(decided?.__error || '')).toContain('找不到候选');
  });

  it('记忆页在控制台里没扔错误（面板挂不上时如实跳过，不留恒真判据）', async function () {
    // 通道本身必须先可用：`dump()` 给的是 `{ items, dropped }`（不是数组）——外部改
    // 不动内部状态是它刻意的形状。拿不到它说明 `consoleLog.js` 没装上，这条要红。
    const dump = await runAsync(
      `return window.__DSH_CONSOLE__ ? window.__DSH_CONSOLE__.dump().items : null;`
    );
    expect(Array.isArray(dump)).toBe(true);

    // **判据先挂在「面板真的挂载了」上**。本通道里 `<Transition mode="out-in">` 的
    // leave 永不结束（见第一条用例的实测说明），记忆面板根本没挂载——没有面板就没有
    // 记忆相关的控制台错误，于是「打开记忆页没有错误」在这个条件下**恒真**。恒真的
    // 判据比没有判据更坏（本仓在 `builtin-peer-imports` 上栽过同一个形状：绿着，而
    // 插件已经整条死掉），所以这里探一次面板标题：挂不上就**响亮地跳过**，挂上了才
    // 继续断言——将来过渡那条路修好、或有人在真前台的窗口里跑这条 spec，它就自动
    // 变回一条真判据。
    const mounted = await runAsync(
      `return document.querySelector('h1.page-title')?.textContent.trim() === '记忆';`
    );
    if (!mounted) {
      console.warn(
        '[e2e] 记忆面板在本通道挂载不了（Transition leave 不结束）→ 跳过这条：' +
          '控制台错误数此刻恒为零，验不了任何东西。'
      );
      this.skip();
    }

    // 只看与记忆有关的那些：别的面板可能各有历史噪声，这条用例不该替它们背书。
    const errors = (dump || []).filter((entry) => entry?.level === 'error');
    const mine = errors.filter((entry) => JSON.stringify(entry).includes('memory'));
    expect(mine).toEqual([]);
  });
});
