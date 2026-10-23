// **真机复验：@ 菜单的二级菜单（站路径）**。
//
// ## 这一轮验的是哪几件事
//
//   ① **顶层 `@` 菜单**：「来源站」那三行**每行都带下钻入口**（内核原生的
//      `drill` ⇒ 行尾出现一枚箭头 ＋ 「Tab」提示）——那是「二级菜单」的第一层。
//   ② **钻进去**（Tab／箭头）：query 原地改写成 `@站名/` ⇒ 草稿里出现
//      `@站名/` ⇒ 菜单**不关**（内核的 `{text, continue:true}` 语义）。
//   ③ **面包屑**：第二层顶上给出「Web Chat › 站名」，且当前那一格不可点
//      （内核的 `current` 语义）⇒ 用户看得见「我在哪、怎么回去」。
//      ⚠️ **断言不写死站名**：内核的 Tab 只作用于**高亮那一行** ⇒ ⇒ 写死
//      「必须是 MiniMax」会把合成鼠标事件不生效记成产品毛病 ⇒ ⇒ 验的是
//      **「草稿里的站 ＝ 面包屑标 current 的那个站」**这个不变量。
//   ④ **第二层里没有「来源站」那三行**了（那三行是顶层的东西；回去的路是面包屑）。
//   ⑤ （证据，不作判据）插件探针里那条 `candidates` 事件带 `path`／`site`
//      ⇒ **这一层取的是那个站的目录**。
//
// ## 为什么单独一个脚本
//
// `shell-verify.mjs` 验的是 G1 那四条（chip / 发送门控），与站路径无关；本轮
// 只关心上面这五件事 ⇒ ⇒ 不去改那份脚本（改它会让两轮的读数混在一处）。
//
// ## 隔离与新鲜度
//
// 走的是**仓库现成的隔离 harness**（`shell-lib.mjs`）：
//   · `DSH_XLINK_HOME` → 临时目录；`appDirectoriesOverride` → 临时目录；
//     WebView2 的 CDP 端口 → 本机一个空闲端口（用户真实目录一个字节不碰）；
//   · **清物化**（`clearMaterializedPlugin`）⇒ **跑的一定是当前仓库代码**
//     （壳的物化按「插件版本 ＋ 内核指纹」去重 ⇒ 不清就会跑旧插件）。
//
// ## 用法
//
//   # 终端 A：起隔离真壳
//   node plugins/official-chat/test/drive/shell/shell-dev.mjs
//   # 终端 B：跑复验
//   node plugins/official-chat/test/drive/shell/shell-verify-path.mjs
import {
  out,
  step,
  verdict,
  focusComposer,
  readDraft,
  clearDraft,
  prepareShell,
} from './shell-lib.mjs';
import { evaluate, sleep } from './cdp.mjs';
import { EVIDENCE_DIR, ROUND } from './stub-model.mjs';
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';

/** 三个站名（客户端自己那份站表的显示名；短码不上屏）。 */
const SITE_LABELS = ['DeepSeek', 'MiniMax', 'ChatGPT'];

/** 这一轮钻进去的那个站（**探针要盯的就是它** ⇒ ⇒ 固定写死，别让用例自己挑）。 */
const DRILL_SITE = 'MiniMax';

/**
 * 读**菜单表面**（内核的 `[data-trigger-menu]` 容器）。
 *
 * **为什么按这个容器取**：页面里到处是 `role="option"`／`nav`（工作台自己有侧栏
 * 导航）⇒ ⇒ 不圈定容器就会把别处的节点算进来 ⇒ ⇒ 读数会是假的。
 *
 * @returns {Promise<object>} `{open, rows, sections, crumbs}`。
 */
async function readMenu(cdp, sessionId) {
  return evaluate(
    cdp,
    sessionId,
    `(() => {
       const surface = document.querySelector('[data-trigger-menu]');
       if (!surface) return { open: false, rows: [], sections: [], crumbs: [], pending: false };
       const rows = [...surface.querySelectorAll('[role="option"]')].map((el) => {
         const chevron = el.querySelector('span[role="button"]');
         return {
           text: (el.innerText || '').trim().replace(/\\s+/g, ' '),
           // **下钻入口**：内核在 drill === true 的行尾渲染一枚箭头
           // （span[role="button"]，aria-label 来自 drill.aria）。
           drill: chevron !== null && chevron !== undefined,
           drillLabel: chevron ? chevron.getAttribute('aria-label') : null,
         };
       });
       const sections = [...surface.querySelectorAll('[role="presentation"]')]
         .map((el) => (el.innerText || '').trim())
         .filter((t) => t !== '');
       const crumbs = [...surface.querySelectorAll('nav')].map((nav) => ({
         aria: nav.getAttribute('aria-label'),
         text: (nav.innerText || nav.textContent || '').trim().replace(/\\s+/g, ' '),
         buttons: [...nav.querySelectorAll('button')].map((b) => ({
           text: (b.innerText || b.textContent || '').trim(),
           current: b.getAttribute('aria-current') === 'location',
           disabled: b.disabled === true,
         })),
       }));
       // **还在 loading 吗**：内核在候选项没到时渲染一行骨架（skeletonRow）。
       //   这一格很重要：**菜单开着 ≠ 候选项到了** ⇒ ⇒ 断言要等「ready」，
       //   否则会把「还没取到」读成「没有那些行」。
       const pending = surface.querySelector('[class*="skeleton"]') !== null;
       return { open: true, rows, sections, crumbs, pending };
     })()`,
  );
}

/**
 * 站点行（按显示名认；内核渲染的是 `role="option"` 的按钮）。
 *
 * **口径**：一行的可见文本是「站名 ＋ 描述 ＋ （高亮时）下钻提示」拼起来的
 * （实测：`DeepSeek 这个站的会话 进入目录 Tab`）⇒ ⇒ **不能拿整行文本做等值比较**
 * ⇒ ⇒ 认的是「**以站名开头**」＋「**带下钻入口**」（只有站行带）。
 */
function siteRowsOf(menu) {
  return (menu?.rows ?? []).filter(
    (row) => SITE_LABELS.some((label) => row.text === label || row.text.startsWith(`${label} `))
      && row.drill === true,
  );
}

/**
 * 悬停到某一行（内核靠 `onHover` 移动高亮；**Tab 只作用于高亮那一行**）。
 *
 * **为什么用真实鼠标事件**：菜单是 React 渲染的，`.hover()` 之类的合成事件不会
 * 触发内核的 `onMouseMove` ⇒ ⇒ 高亮不会动 ⇒ ⇒ Tab 打不到那一行。
 *
 * **顺带等「高亮真的落到那一行」**：内核只给**当前高亮**的那一行显示下钻提示
 * （`Tab` 那个 kbd，平时是 `display:none` ⇒ 不在 `innerText` 里）⇒ ⇒
 * 「那一行的文本里出现了 `Tab`」就是「高亮已经在它身上」的 DOM 证据。
 *
 * @returns {Promise<object>} `{hovered, active}`。
 */
async function hoverRow(cdp, sessionId, label) {
  const found = await evaluate(
    cdp,
    sessionId,
    `(() => {
       const surface = document.querySelector('[data-trigger-menu]');
       if (!surface) return { ok: false, why: '菜单没开' };
       const rows = [...surface.querySelectorAll('[role="option"]')];
       // ⚠️ **口径**：一行的文本是「站名 ＋ 描述 ＋ 下钻提示」拼起来的 ⇒ ⇒
       //   认的是「以站名开头」，不是整行等值。
       // ⚠️ **还要归一空白**：innerText 里站名与描述之间是**换行**（不是空格）
       //   ⇒ ⇒ 不归一就 startsWith('MiniMax ') 永远不成立 ⇒ ⇒ 上一轮读到的
       //   「hovered:false」是**这个口径错**，不是产品问题。
       const hit = rows.find((el) => {
         const t = (el.innerText || '').trim().replace(/\\s+/g, ' ');
         return t === ${JSON.stringify(label)} || t.startsWith(${JSON.stringify(label)} + ' ');
       });
       if (!hit) {
         return { ok: false, why: '没有那一行', rows: rows.map((el) => (el.innerText || '').trim()) };
       }
       const r = hit.getBoundingClientRect();
       return { ok: true, x: r.left + r.width / 2, y: r.top + r.height / 2 };
     })()`,
  );
  if (!found?.ok) return { hovered: false, why: found?.why ?? 'unknown', rows: found?.rows ?? [] };
  await cdp.send(
    'Input.dispatchMouseEvent',
    { type: 'mouseMoved', x: found.x, y: found.y, button: 'left', buttons: 0 },
    sessionId,
  );
  await sleep(200);
  // **等高亮落到那一行**（最多 8 秒）。
  let active = false;
  const activeText = [];
  for (let i = 0; i < 16; i += 1) {
    const state = await evaluate(
      cdp,
      sessionId,
      `(() => {
         const surface = document.querySelector('[data-trigger-menu]');
         if (!surface) return { ok: false, texts: [] };
         const texts = [...surface.querySelectorAll('[role="option"]')].map((el) => (el.innerText || '').trim().replace(/\\s+/g, ' '));
         return { ok: true, texts };
       })()`,
    );
    activeText.length = 0;
    activeText.push(...(state?.texts ?? []));
    const target = (state?.texts ?? []).find((t) => t.startsWith(label));
    if (target !== undefined && target.includes('Tab')) { active = true; break; }
    await sleep(500);
  }
  return { hovered: true, active, x: found.x, y: found.y, rowTexts: activeText };
}

/** 按一次真实 Tab（内核把 Tab 当「下钻」；它只在高亮那一行带 `drill` 时下钻）。 */
async function pressTab(cdp, sessionId) {
  await cdp.send('Input.dispatchKeyEvent', {
    type: 'rawKeyDown', key: 'Tab', code: 'Tab', windowsVirtualKeyCode: 9, nativeVirtualKeyCode: 9,
  }, sessionId);
  await sleep(60);
  await cdp.send('Input.dispatchKeyEvent', {
    type: 'keyUp', key: 'Tab', code: 'Tab', windowsVirtualKeyCode: 9, nativeVirtualKeyCode: 9,
  }, sessionId);
  await sleep(1200);
}

/** 读插件探针里那条 `candidates` 事件（**只作证据，不作判据**）。 */
async function readCandidatesEvents(cdp, sessionId) {
  const probe = await evaluate(
    cdp,
    sessionId,
    `(() => {
       const p = window.__DSH_XLINK_OFFICIAL_CHAT_PROBE__;
       if (!p) return [];
       return (p.events ?? []).filter((e) => e.kind === 'candidates').map((e) => ({
         query: e.query ?? null,
         site: e.site ?? null,
         path: e.path ?? null,
         count: e.count ?? null,
       }));
     })()`,
  );
  return Array.isArray(probe) ? probe : [];
}

/* ------------------------------------------------------------------ 主流程 */

let cdp;
let stub;
const evidence = { round: ROUND, evidenceDir: EVIDENCE_DIR, checks: {} };

try {
  const prepared = await prepareShell();
  cdp = prepared.cdp;
  stub = prepared.stub;
  const { wb } = prepared;

  // ── A：顶层 `@` 菜单：「来源站」那三行都带**下钻入口** ──────────────────
  //
  // **等的是「候选项到了」不是「菜单开着」**：内核在候选项没到时渲染一行骨架
  // （`skeletonRow`）⇒ ⇒ 菜单开着但一行都没有，是**还在取数**，不是「没有那些行」。
  // 真机上第一刀带 `refresh`（壳侧重取一轮，实测要好几秒，冷档案首取更久）⇒ ⇒
  // 这里给足等待（默认 180 秒），等不到就把「当时的状态」原样记进证据。
  const waitMs = Number(process.env.G1_PATH_WAIT_MS ?? 180000);
  await clearDraft(cdp, wb);
  await focusComposer(cdp, wb);
  await evaluate(cdp, wb, `(() => { document.execCommand('insertText', false, '@'); return true; })()`);
  let menu = { open: false, rows: [], sections: [], crumbs: [], pending: false };
  const waited = [];
  const t0 = Date.now();
  while (Date.now() - t0 < waitMs) {
    menu = await readMenu(cdp, wb);
    waited.push({ atMs: Date.now() - t0, pending: menu.pending === true, rows: (menu.rows ?? []).length });
    if (siteRowsOf(menu).length === SITE_LABELS.length) break;
    await sleep(1000);
  }
  const sites = siteRowsOf(menu);
  const aDetail = {
    open: menu.open === true,
    pending: menu.pending === true,
    waitedMs: Date.now() - t0,
    samples: waited.filter((s, i) => i === 0 || i === waited.length - 1 || s.rows > 0),
    sections: menu.sections,
    siteRows: sites.map((row) => ({ text: row.text, drill: row.drill, drillLabel: row.drillLabel })),
    allRowTexts: (menu.rows ?? []).map((row) => row.text),
  };
  step('A-site-rows-have-drill', aDetail);
  const aOk =
    menu.open === true
    && sites.length === SITE_LABELS.length
    && sites.every((row) => row.drill === true);
  verdict(
    '断言 A：顶层「来源站」三行都带下钻入口（内核 drill ⇒ 行尾箭头）',
    aOk ? '通过' : '失败',
    JSON.stringify(aDetail),
  );
  evidence.checks.A = { ok: aOk, detail: aDetail };

  // ── B：钻进去（Tab）⇒ query 原地改写成 `@站名/`、菜单不关 ───────────────
  const hover = await hoverRow(cdp, wb, DRILL_SITE);
  step('B-hover-station-row', hover);
  await pressTab(cdp, wb);
  const draftAfterDrill = await readDraft(cdp, wb);
  // **面包屑是跟着 query 走的**（源侧 `header()` 同步给）⇒ ⇒ 它不等目录取数
  //   ⇒ 但内核重开菜单时才会重新问一次 `header()` ⇒ ⇒ 这里等它出现（最多 30 秒）。
  let menuAfterDrill = { open: false, rows: [], sections: [], crumbs: [], pending: false };
  const drillT0 = Date.now();
  while (Date.now() - drillT0 < 30000) {
    menuAfterDrill = await readMenu(cdp, wb);
    const hasCrumb = (menuAfterDrill.crumbs ?? []).some((c) => (c.buttons ?? []).length > 0);
    if (hasCrumb) break;
    await sleep(500);
  }
  /**
   * **钻进去的是哪个站**：从**草稿**反推（`@DeepSeek/` ⇒ DeepSeek）。
   *
   * ⚠️ **为什么断言不写死站名**：内核的 Tab 只作用于**高亮那一行** ⇒ ⇒ 高亮落在
   *    哪一行取决于指针/键盘 ⇒ ⇒ 合成鼠标事件不生效时会钻进**首行** ⇒ ⇒ 写死
   *    「必须是 MiniMax」会把**脚本口径**的毛病记成**产品**的毛病。
   *    ⇒ ⇒ 真要验的不变量是：**草稿里的站 ＝ 面包屑标 current 的那个站**（断言 C）。
   */
  const drilled = String(draftAfterDrill.draft ?? '').trim().match(/^@([^/]+)\/$/);
  const drilledLabel = drilled === null ? '' : drilled[1];
  const bDetail = {
    hovered: hover.hovered === true,
    /** **高亮真的落到那一行了**（那一行的文本里出现了下钻提示 `Tab`）。 */
    active: hover.active === true,
    /** **悬停那台站**（脚本想钻的那台；高亮没生效时内核会钻进首行）。 */
    wanted: DRILL_SITE,
    /** **实际钻进去的站**（从草稿反推）。 */
    drilled: drilledLabel,
    draft: draftAfterDrill.draft,
    menuOpen: menuAfterDrill.open === true,
    waitedMs: Date.now() - drillT0,
    sections: menuAfterDrill.sections,
    rowTexts: (menuAfterDrill.rows ?? []).map((row) => row.text),
  };
  step('B-drill-rewrites-query', bDetail);
  const bOk =
    drilled !== null
    && SITE_LABELS.includes(drilledLabel)
    && menuAfterDrill.open === true;
  verdict(
    '断言 B：钻进去 ⇒ 草稿变成 `@站名/` 且菜单不关',
    bOk ? '通过' : '失败',
    JSON.stringify(bDetail),
  );
  evidence.checks.B = { ok: bOk, detail: bDetail };

  // ── C：面包屑（第二层顶上那一条） ────────────────────────────────────────
  //
  // ⚠️ **内核自带 `@file` 来源也有自己的面包屑**（实测两条并存：「工作区 DeepSeek」
  //    是它的，「Web Chat DeepSeek」是本插件的）⇒ ⇒ **按内容认**，不能拿第一条。
  const crumb = (menuAfterDrill.crumbs ?? []).find(
    (c) => (c.buttons ?? []).some((b) => b.text === 'Web Chat'),
  );
  /** 面包屑上**标 current 的那一格**（应当就是刚钻进去的那个站）。 */
  const currentBtn = (crumb?.buttons ?? []).find((b) => b.current === true);
  const cDetail = {
    crumbs: menuAfterDrill.crumbs ?? [],
    chosen: crumb ?? null,
    /** 刚钻进去的站（从草稿反推）⇒ ⇒ 面包屑要跟它对得上。 */
    drilled: drilledLabel,
  };
  step('C-breadcrumb', cDetail);
  const cOk =
    crumb !== undefined
    && crumb !== null
    && (crumb.buttons ?? []).some((b) => b.text === 'Web Chat' && b.disabled !== true && b.current !== true)
    && currentBtn !== undefined
    && currentBtn.text === drilledLabel
    && currentBtn.disabled === true;
  verdict(
    '断言 C：第二层给面包屑「Web Chat › 站名」（当前那格标 current 且不可点）',
    cOk ? '通过' : '失败',
    JSON.stringify(cDetail),
  );
  evidence.checks.C = { ok: cOk, detail: cDetail };

  // ── D：第二层里**没有**「来源站」那三行了 ────────────────────────────────
  const sitesAfterDrill = siteRowsOf(menuAfterDrill);
  const dDetail = {
    siteRowsInSecondLevel: sitesAfterDrill.map((row) => row.text),
    rowTexts: (menuAfterDrill.rows ?? []).map((row) => row.text),
    sections: menuAfterDrill.sections,
  };
  step('D-no-site-rows-in-second-level', dDetail);
  const dOk = sitesAfterDrill.length === 0;
  verdict(
    '断言 D：第二层里不再列「来源站」那三行（回去的路是面包屑）',
    dOk ? '通过' : '失败',
    JSON.stringify(dDetail),
  );
  evidence.checks.D = { ok: dOk, detail: dDetail };

  // ── E：（证据，不作判据）探针里那次目录取数带了 `path` ＋ `site` ──────────
  //
  // **按「刚钻进去的那个站」找**（不是写死 minimax）：探针里 `path`/`site` 记的是
  // **短码**（deepseek/minimax/chatgpt）⇒ ⇒ 这里把显示名映射回短码。
  const SHORT_CODES = { DeepSeek: 'deepseek', MiniMax: 'minimax', ChatGPT: 'chatgpt' };
  const candidatesEvents = await readCandidatesEvents(cdp, wb);
  const pathEvent =
    candidatesEvents.find((e) => e.path === SHORT_CODES[drilledLabel]) ?? null;
  step('E-candidates-event-has-path', { events: candidatesEvents, pathEvent });
  evidence.checks.E = { note: '只作证据', pathEvent, events: candidatesEvents };

  // ── F：（证据）物化产物里那份 client.js 的指纹（证明跑的是当前仓库代码）──
  const fingerprints = await evaluate(
    cdp,
    wb,
    `(() => {
       const rows = [...document.querySelectorAll('[role="option"]')];
       return { optionCount: rows.length };
     })()`,
  );
  step('F-menu-state', fingerprints);
  evidence.checks.F = fingerprints;

  const all = Object.values(evidence.checks).filter((c) => typeof c.ok === 'boolean');
  const failed = all.filter((c) => c.ok !== true);
  evidence.summary = {
    total: all.length,
    passed: all.length - failed.length,
    failed: failed.length,
  };
  writeFileSync(join(EVIDENCE_DIR, `shell-verify-path.${ROUND}.json`), `${JSON.stringify(evidence, null, 2)}\n`, 'utf8');
  console.log(`[shell-verify-path] 证据：${join(EVIDENCE_DIR, `shell-verify-path.${ROUND}.json`)}`);
  console.log(`[shell-verify-path] 汇总：${JSON.stringify(evidence.summary)}`);
  if (failed.length > 0) {
    console.error('[shell-verify-path] 有断言没过：', failed.map((f) => f.ok).join(','));
  }
  // **桩模型是个 HTTP 服务**（不关它，这个进程不会退出）⇒ ⇒ 显式收摊。
  try { stub?.stop?.(); } catch { /* ignore */ }
  process.exit(failed.length === 0 ? 0 : 1);
} catch (error) {
  evidence.error = String(error?.message ?? error);
  writeFileSync(join(EVIDENCE_DIR, `shell-verify-path.${ROUND}.json`), `${JSON.stringify(evidence, null, 2)}\n`, 'utf8');
  console.error('[shell-verify-path] 失败：', evidence.error);
  console.log(JSON.stringify(out, null, 2));
  try { stub?.stop?.(); } catch { /* ignore */ }
  process.exit(1);
}
