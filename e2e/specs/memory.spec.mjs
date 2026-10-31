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

    // **面板内容默认挂不上，原因已实测**：WebDriver 拉起的窗口在后台，不跑
    // `requestAnimationFrame`，而 `App.vue` 的面板切换是 `<Transition mode="out-in">`
    // ——leave 永不结束，新面板就不挂载。**每个面板都一样**（设置 / 插件实测同样停在
    // `panel-leave-active`、标题仍是「概览」），所以这不是记忆面板的问题；注入「禁用过渡」
    // 的样式、手动派发 transitionend、用 osascript 把窗口提到前台，三种办法都试过，不解决。
    //
    // **但缺的就是那一个全局函数，补上就能挂**（2026-10-16 实测）：在页面里
    // `window.requestAnimationFrame ??= (cb) => setTimeout(() => cb(Date.now()), 16)`
    // （`cancelAnimationFrame` 同办）之后点侧栏，`h1.page-title` 会变成「记忆」，面板
    // 高度也就能真的量出来——那一轮用这个办法量到卡片高度与「有没有溢出」，比读 CSS 推
    // 可靠得多。要在这条 spec 里验面板内容时照这个来；**别**再写「本通道永远验不了」。
    //
    // 内容渲染目前仍由下面那条命令断言 + Rust 侧的检索用例覆盖，不在这里假装验过。
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

  it('memory_effect_report 在真壳里能解析（效果页的数据源真的通）', async () => {
    // 效果页签的全部数字只来自这一条命令：ACL 漏登记 / 命令名飘了时，前端只会 .catch
    // 吞掉、页面停在空态——所以这里在真壳的 webview 里直接调，要求拿回合同 §2.2 的对象。
    const report = await runAsync(
      "return await window.__TAURI__.core.invoke('memory_effect_report', { rangeDays: 7 });"
    );
    expect(report?.__error).toBeUndefined();
    expect(typeof report).toBe('object');
    expect(report.range_days).toBe(7);
    // 窗口、覆盖率、KPI、漏斗、趋势、榜单、成本、健康都是 P0 合同字段。
    expect(typeof report.window?.from).toBe('string');
    expect(typeof report.window?.to).toBe('string');
    expect(typeof report.coverage?.sessions_total).toBe('number');
    expect(typeof report.kpi?.injection_snapshots).toBe('number');
    expect(typeof report.funnel?.candidates_created).toBe('number');
    expect(Array.isArray(report.trend)).toBe(true);
    expect(Array.isArray(report.leaderboard)).toBe(true);
    expect(typeof report.cost?.injected_bytes).toBe('number');
    expect(typeof report.health?.has_enough_requests).toBe('boolean');
    // 参数只接受 7 / 30 / 90，其它值由后端 clamp 到最近的合法值（合同 §2.1）。
    // 真壳里对一次：前端传界外值时若命令直接 reject，面板会静默停在旧数据上。
    const odd = await runAsync(
      "return await window.__TAURI__.core.invoke('memory_effect_report', { rangeDays: 13 });"
    );
    expect(odd?.__error).toBeUndefined();
    expect([7, 30, 90].includes(odd.range_days)).toBe(true);
  });

  it('记忆页有「效果」页签，切过去能看到效果页（本通道挂不上时如实跳过）', async function () {
    // **先探面板挂没挂**，与上一条控制台用例同一策略：WebDriver 拉起的窗口不跑
    // requestAnimationFrame，App.vue 的 Transition mode="out-in" 的 leave 永不结束，
    // 面板默认挂不上（见第一条用例的实测说明）。没挂载时「页签存在」这类断言验不了
    // 任何东西，所以响亮跳过而不是写一条恒真判据——将来过渡那条路修好、或有人在真前台
    // 的窗口里跑这条 spec，它自动变回真判据（可绕开的办法见第一条用例注释）。
    const mounted = await runAsync(
      "return document.querySelector('h1.page-title')?.textContent.trim() === '记忆';"
    );
    if (!mounted) {
      console.warn(
        '[e2e] 记忆面板在本通道挂载不了（Transition leave 不结束）→ 跳过效果页签用例：' +
          '面板没挂载时页签与效果页都验不了。'
      );
      this.skip();
    }

    // 两个页签都在，且是 role=tab（可访问性契约，合同 §3.2）。
    const tabs = await runAsync(
      "return [...document.querySelectorAll('[role=tab]')].map((el) => el.textContent.trim());"
    );
    expect(Array.isArray(tabs)).toBe(true);
    expect(tabs.includes('记忆库')).toBe(true);
    expect(tabs.includes('效果')).toBe(true);

    // 切到「效果」：效果页是 v-else 渲染的独立组件，记忆库的 .page-layout 应随之让位。
    const switched = await runAsync(
      "const tab = [...document.querySelectorAll('[role=tab]')].find((el) => el.textContent.trim() === '效果');" +
        "if (!tab) return { found: false };" +
        "tab.click();" +
        "return { found: true };"
    );
    expect(switched?.found).toBe(true);

    // 效果页壳（数据来源那一行）挂出来；真壳里这条命令会真扫一遍本机会话日志，等首扫。
    await browser.waitUntil(
      async () =>
        (await runAsync("return !!document.querySelector('.memory-effect .effect-source');")) === true,
      { timeout: 60_000, timeoutMsg: '切到「效果」页签后效果页没有挂出来' }
    );

    const layout = await runAsync(
      "return {" +
        " source: document.querySelector('.memory-effect .effect-source')?.textContent.trim() || ''," +
        " kpis: document.querySelectorAll('.memory-effect .effect-kpi').length," +
        " cards: [...document.querySelectorAll('.memory-effect .card-head h2')].map((el) => el.textContent.trim())," +
        " libraryGone: !document.querySelector('.page-layout')" +
        "};"
    );
    expect(layout.source).toContain('数据来源');
    expect(layout.kpis).toBe(5);
    expect(layout.cards.some((text) => text.startsWith('效果漏斗'))).toBe(true);
    expect(layout.cards.some((text) => text.startsWith('记忆榜单'))).toBe(true);
    expect(layout.cards.some((text) => text.startsWith('数据健康'))).toBe(true);
    // 记忆库主内容确实让位了（不是两层叠着画）。
    expect(layout.libraryGone).toBe(true);
  });
});
