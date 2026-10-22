// **壳内 workbench 复验**：驱动真壳（tauri dev）的 webview，证明插件在
// **壳自己起的工作台窗口**里同样成立。
//
// ## 驱动的是哪个窗口
//
// Windows 上壳的 webview 是 WebView2，而 WebView2 认
// `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=<port>`，
// 于是壳的面板与**工作台**能用同一套 CDP 代码驱动（本机没有 chromedriver /
// playwright / puppeteer，仓库 wdio 那条路缺 tauri-driver）。
// 驱动的是**壳里那个工作台窗口**（真 Tauri webview，载入内核 Web UI），
// 不是内核自带的工作台页面，也不是壳的管理面板。
//
// ## 三件事
//
//   ① `@` 菜单里出现「Web Chat」这一组；
//   ② 选中后 chip 落地且是**内核自己的节点**（不是 DOM 覆盖编辑器），
//      且**正文不在草稿里**（pending 句柄不冒充就绪）；
//   ③ 读取延迟时按发送 → 桩模型在读取完成前 0 请求、完成后请求体含**文件指针**
//      （正文以文件形式引入，**不再内联**）；读取失败 → 0 请求 + 可见错误 + 草稿保留。
//
// ## 插件是怎么接进去的
//
// 由**壳自己的物化链**：面板里调 `builtin_set_enabled`（壳的命令层 →
// `ensure_wired` → 物化 + 写接线行），启动工作台时 `runtime::refresh_enabled`
// 再刷一遍。脚本末尾核对实例里的物化产物与接线行，证明这条链真的走过。
//
// ## 踩过的坑（都在下面标注了，别退回旧写法）
//
//  ① **只能 attach 主窗口**。壳里有两个同源 webview：主窗口与官方对话条
//     （URL 带 `chatstrip=1`）。壳的能力检查只允许主窗口跑本地命令——
//     连错窗口时 `builtin_set_enabled` 被拒：
//       "builtin_set_enabled not allowed on window \"official-chat\",
//        webview \"official-chat-strip\" … allowed on: [windows: \"main\"]"
//     （2026-10-20 14:28 那轮就是这么失败的：脚本按 `localhost:5174` 找面板，
//     第一个命中的是对话条。）
//  ② **过期的 kernel.pid 会让 `start_kernel` 不起内核**。pid 文件里留着已死
//     进程的 pid 时，壳认为内核还在跑，内核端口永远不通，脚本就一直等。
//     复验前先清掉它。
//  ③ **等的是「可编辑」的编辑器**，不是 composer 卡片。`[data-composer-card]`
//     页面一开就有，但内核的 input 状态没建好时，编辑器是
//     `contenteditable="false" data-phase="inert"`（内核
//     `ui-conversation/client.js@0.2.1-rc.2:19028`：
//     `phase: input?.phase ?? "inert"`）。
//  ④ **弹窗是延迟出现的**，且要用**真实鼠标事件**关——否则弹窗盖在 composer
//     上面，`@` 打不进去，断言 ① 假红。
//  ⑤ **`@` 前面必须是行首或空白**（内核触发词正则
//     `/(?:^|\s)(@([^\s]*))$/`，`input-trigger/client.js@0.2.1-rc.2:725`）。
//
// ## 用法
//
//   # 终端 A：起隔离真壳
//   node plugins/official-chat/test/drive/shell/shell-dev.mjs
//   # 终端 B：跑复验
//   node plugins/official-chat/test/drive/shell/shell-verify.mjs
import { out, step, verdict, focusComposer, readDraft, readBody, readProbe, setConfig, pickFixture, clearDraft, pressEnterSubmit, inspectShellArtifacts, prepareShell, FIXTURE_ALPHA, FIXTURE_BETA, BODY_ALPHA } from './shell-lib.mjs';
import { evaluate, sleep } from './cdp.mjs';
import { EVIDENCE_DIR, ROUND } from './stub-model.mjs';
import { INSTANCE_HOME } from './paths.mjs';
import { readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

/* ------------------------------------------------------------------ 主流程 */

let cdp;
let stub;
try {
  const prepared = await prepareShell();
  cdp = prepared.cdp;
  stub = prepared.stub;
  const { panelSession, wb } = prepared;

  // ⑥ ①：@ 菜单里那一组
  //
  // ⚠️ **等的是「菜单开了」，不是「打完 @ 的下一刻」**：真源那一刀要开冷档案窗
  // （实测第一刀 30 秒量级，见 harness README 第 7 条）⇒ ⇒ 固定睡 2.5 秒会把
  // 「还在取数」读成「没有那些行」（2026-10-22 bodyfile-r1 实测：四条断言全红，
  // 菜单其实只是没开成 —— 读到的「菜单行」是工作台工具栏那一串）。
  await clearDraft(cdp, wb);
  await focusComposer(cdp, wb);
  await evaluate(cdp, wb, `(() => { document.execCommand('insertText', false, '@'); return true; })()`);
  const menuWaitMs = Number(process.env.G1_MENU_WAIT_MS ?? 90000);
  const menuT0 = Date.now();
  let menuBody = '';
  while (Date.now() - menuT0 < menuWaitMs) {
    menuBody = await readBody(cdp, wb);
    if (menuBody.includes('Web Chat')) break;
    await sleep(1000);
  }
  const aDetail = {
    hasGroup: menuBody.includes('Web Chat'),
    /**
     * **固定夹具那两行在不在**（**决定 B/D/E 能不能跑**，见下面那段说明）。
     */
    hasFixtureRow: menuBody.includes(FIXTURE_ALPHA),
    waitedMs: Date.now() - menuT0,
    snippet: menuBody.split('\n').filter((l) => l.includes('Web Chat') || l.includes(FIXTURE_ALPHA)).slice(0, 5),
  };
  step('A-menu-in-shell', aDetail);
  verdict('断言 A：@ 菜单里出现「Web Chat」分组', aDetail.hasGroup ? '通过' : '失败', JSON.stringify(aDetail));
  await clearDraft(cdp, wb);

  /**
   * **壳里能不能跑那几条「要挑夹具会话」的断言**。
   *
   * ⚠️ **壳里永远走真实源**：host 半边在产品装配里**一定**注入 `siteSource`
   * （`host/index.js`）⇒ ⇒ 目录与正文都走那条回环桥 ⇒ ⇒ **固定夹具那两条会话
   * 根本不在菜单里**（夹具只在**没有真实源**时回退，`host/commands.js` 那条
   * 分支明写「只服务判据与单测」）⇒ ⇒ 本机没登官网时，菜单里只有三个站行
   * ＋ 一行「官网引用不可用」。
   *
   * ⇒ ⇒ 那几条断言在壳里**跑不了** ⇒ ⇒ 记**未执行**（**不是失败**：那是**环境
   * 条件**，不是产品毛病）⇒ ⇒ **G1 那条完整链路在内核层跑**
   * （`test/drive/run-g1.mjs`：那里有个「假官网桥」把官网那半替掉）。
   */
  const hasFixtureRows = aDetail.hasFixtureRow === true;
  const SKIP_REASON = '壳里走真实源 ⇒ 固定夹具会话不可达 ⇒ 这一条只能在内核层跑（run-g1.mjs）';

  // ⑦ ②：chip 落地且是内核节点（正文不在草稿里）
  if (!hasFixtureRows) {
    step('B-chip-in-shell', { skipped: SKIP_REASON });
    verdict('断言 B：chip 落地且是内核节点（正文不在草稿里）', '未执行', SKIP_REASON);
  } else {
    await setConfig(cdp, wb, { readDelayMs: 8000, failRead: false });
    const pick = await pickFixture(cdp, wb, FIXTURE_ALPHA);
    const draftNow = await readDraft(cdp, wb);
    const bDetail = {
      picked: pick.clicked === true,
      menuHasRow: pick.menuHasRow === true,
      menuRows: pick.menuRows ?? [],
      chipIsKernelNode: (draftNow.html ?? '').includes('data-composer-chip="official-chat"'),
      bodyInDraft: (draftNow.html ?? '').includes(BODY_ALPHA),
      draftHtml: (draftNow.html ?? '').slice(0, 220),
    };
    step('B-chip-in-shell', bDetail);
    verdict(
      '断言 B：chip 落地且是内核节点（正文不在草稿里）',
      bDetail.picked && bDetail.chipIsKernelNode && bDetail.bodyInDraft === false ? '通过' : '失败',
      JSON.stringify(bDetail),
    );
  }

  // ⑧ ③：发送门控 —— 等待
  if (!hasFixtureRows) {
    step('D-send-gate-in-shell', { skipped: SKIP_REASON });
    verdict(
      '断言 D：读取门控（读取完成前 0 请求，完成后请求体含**文件指针**、正文不再内联）',
      '未执行',
      SKIP_REASON,
    );
  } else {
    await clearDraft(cdp, wb);
    await setConfig(cdp, wb, { readDelayMs: 8000, failRead: false });
    const pickD = await pickFixture(cdp, wb, FIXTURE_ALPHA);
    await evaluate(cdp, wb, `(() => { document.execCommand('insertText', false, '请看这个'); return true; })()`);
    const beforeD = stub.requests().length;
    const t0 = Date.now();
    await pressEnterSubmit(cdp, wb);
    const timeline = [];
    let arrived = null;
    for (let i = 0; i < 40; i += 1) {
      const probe = await readProbe(cdp, wb);
      const kinds = (probe?.events ?? []).map((e) => e.kind);
      const reqs = stub.requests().slice(beforeD);
      timeline.push({
        t: Date.now() - t0,
        reqs: reqs.length,
        hasBody: reqs.some((r) => JSON.stringify(r.body ?? {}).includes(BODY_ALPHA)),
        kinds: kinds.slice(-3),
      });
      if (reqs.length > 0) { arrived = Date.now() - t0; break; }
      await sleep(500);
    }
    const reqsD = stub.requests().slice(beforeD);
    const draftAfter = await readDraft(cdp, wb);
    const bodyAfter = await readBody(cdp, wb);
    /**
     * **正文不再内联**（PM 2026-10-22 21:5x 裁：「引用内容以文件的形式引入」）
     * ⇒ ⇒ 请求里给模型的**只是文件指针** ⇒ ⇒ 判据从「请求体含夹具正文」改成
     * **「请求体含指针」＋「正文一个字都不许在里面」＋「正文文件在盘上」**。
     *
     * **为什么按文件名找指针**：请求体是 JSON ⇒ ⇒ 路径里的反斜杠被转义 ⇒ ⇒
     * 按整条路径找**必然找不到** ⇒ ⇒ 按**文件名**找（里面没有反斜杠）⇒ ⇒ 在
     * JSON 里原样出现。
     */
    const snapshotDir = join(INSTANCE_HOME, 'official-chat', 'snapshots', 'ds');
    let bodyFiles = [];
    try {
      bodyFiles = readdirSync(snapshotDir).filter((name) => name.endsWith('.body.txt'));
    } catch {
      bodyFiles = [];
    }
    const pointerNeedle = bodyFiles[0] ?? '';
    const hasPointer = (r) =>
      pointerNeedle !== '' && JSON.stringify(r.body ?? {}).includes(pointerNeedle);
    const bodyFileContents = bodyFiles.map((name) => {
      try {
        return { name, content: readFileSync(join(snapshotDir, name), 'utf8') };
      } catch (error) {
        return { name, content: `(读不出来：${String(error?.message ?? error)})` };
      }
    });
    // **口径**：指针「只出现一次」指的是**这一次发送**在**对话请求**里出现一次。
    // 会话是跨轮复用的时，历史里本来就躺着上一轮那条 → 同一个请求里会出现两次。
    // 所以这里把「本次那条」与「历史那条」分开数，别让人拿总数当重复外发。
    const chatRequest = reqsD.find((r) => JSON.stringify(r.body ?? {}).includes('You are an AI agent')) ?? null;
    const chatMessages = chatRequest?.body?.messages ?? [];
    const alphaIdx = chatMessages
      .map((m, i) => ({ i, m, c: typeof m.content === 'string' ? m.content : JSON.stringify(m.content ?? '') }))
      .filter((x) => x.c.includes(BODY_ALPHA))
      .map((x) => x.i);
    const pointerIdx = pointerNeedle === '' ? [] : chatMessages
      .map((m, i) => ({ i, m, c: typeof m.content === 'string' ? m.content : JSON.stringify(m.content ?? '') }))
      .filter((x) => x.c.includes(pointerNeedle))
      .map((x) => x.i);
    const dDetail = {
      picked: pickD.clicked === true,
      waitedMs: arrived,
      newRequests: reqsD.length,
      /** **请求体里有指针**（正文改走文件后，给模型的是这个）。 */
      requestHasPointer: reqsD.some(hasPointer),
      /** **正文本体不许再进请求**（这一格必须是 false）。 */
      requestHasBody: reqsD.some((r) => JSON.stringify(r.body ?? {}).includes(BODY_ALPHA)),
      /** 盘上的正文文件（名字 ＋ 内容）。 */
      bodyFiles: bodyFileContents,
      serializeOk: (await readProbe(cdp, wb))?.events?.some((e) => e.kind === 'serialize-ok') ?? false,
      draftCleared: (draftAfter.draft ?? '').trim() === '',
      sentText: bodyAfter.includes('请看这个'),
      // ↓ 口径字段：对话请求里指针出现在哪几条消息、历史里有没有上一轮的 assistant 回包
      chatRequestCount: reqsD.filter((r) => JSON.stringify(r.body ?? {}).includes('You are an AI agent')).length,
      pointerOccurrenceIndexes: pointerIdx,
      pointerOccurrencesInChatRequest: pointerIdx.length,
      bodyOccurrenceIndexes: alphaIdx,
      bodyOccurrencesInChatRequest: alphaIdx.length,
      thisTurnIndex: pointerIdx.length > 0 ? pointerIdx[pointerIdx.length - 1] : null,
      historyAssistantReplies: chatMessages.filter((m) => m.role === 'assistant').length,
      otherRequestPurposes: reqsD
        .filter((r) => !JSON.stringify(r.body ?? {}).includes('You are an AI agent'))
        .map((r) => (JSON.stringify(r.body ?? {}).includes('Create a concise title') ? 'session-title' : 'other')),
      timeline: timeline.filter((x, i) => i === 0 || i === timeline.length - 1 || x.reqs > 0),
    };
    step('D-send-gate-in-shell', dDetail);
    verdict(
      '断言 D：读取门控（读取完成前 0 请求，完成后请求体含**文件指针**、正文不再内联）',
      dDetail.picked && dDetail.waitedMs !== null && dDetail.requestHasPointer
        && dDetail.requestHasBody === false
        && bodyFileContents.some((one) => one.content === BODY_ALPHA)
        && dDetail.serializeOk && dDetail.draftCleared ? '通过' : '失败',
      JSON.stringify(dDetail),
    );
  }

  // ⑨ ③：发送门控 —— 拒绝（错误提示是**瞬态 Toast**，必须密集采样）
  if (!hasFixtureRows) {
    step('E-reject-in-shell', { skipped: SKIP_REASON });
    verdict(
      '断言 E：读取失败则 0 请求、草稿保留、可见错误出口（节点级）',
      '未执行',
      SKIP_REASON,
    );
  } else {
    //
    //    **节点级断言**（与内核 r14/r16 同一口径）：光有文本命中不算数——文本级
    //    命中证明不了「有一个可访问的、用户看得见的错误出口」。所以这里同时扫
    //    `role="alert"|role="status"|aria-live` 的节点、含错误文案的深层节点
    //    （带 tag/class 路径），并在命中时**截图**留证。
    //    内核把 `error` 级 notice 渲染成 Toast（`ui-conversation/client.js@0.2.1-rc.2:18808`
    //    `if (notice?.level === "error") showToast(notice.text)`，带 `onDone` → 自动消失），
    //    `info` 级才是内联 `role="status"`（`:18982-18986`）→ 必须密集采样。
    await clearDraft(cdp, wb);
    await setConfig(cdp, wb, { readDelayMs: 2500, failRead: true });
    const pickE = await pickFixture(cdp, wb, FIXTURE_BETA);
    await evaluate(cdp, wb, `(() => { document.execCommand('insertText', false, '请看这个'); return true; })()`);
    const beforeE = stub.requests().length;
    await pressEnterSubmit(cdp, wb);
    let errorHit = null;
    let noticeEvidence = null;
    const eSamples = [];
    for (let i = 0; i < 24; i += 1) {
      const scan = await evaluate(
        cdp,
        wb,
        `(() => {
           const body = document.body;
           const inner = body ? (body.innerText || '') : '';
           const content = body ? (body.textContent || '') : '';
           const live = [...document.querySelectorAll('[role="alert"],[role="status"],[aria-live]')]
             .map((el) => ({
               role: el.getAttribute('role'),
               live: el.getAttribute('aria-live'),
               text: (el.textContent || '').trim().slice(0, 160),
             }))
             .filter((x) => x.text.length > 0)
             .slice(0, 6);
           const deep = [];
           for (const el of document.querySelectorAll('*')) {
             const t = el.textContent || '';
             if (t && t.length < 400 && (t.includes('读取失败') || t.includes('发送时会拒绝'))) {
               deep.push({
                 tag: el.tagName,
                 cls: String(el.getAttribute('class') || '').slice(0, 100),
                 role: el.getAttribute('role'),
                 text: t.trim().slice(0, 160),
               });
             }
           }
           return { innerHas: inner.includes('读取失败'), contentHas: content.includes('读取失败'), live, deep: deep.slice(0, 6) };
         })()`,
      );
      const hit = scan?.innerHas === true || scan?.contentHas === true || (scan?.deep?.length ?? 0) > 0;
      eSamples.push({ atMs: i * 250, hit, liveCount: scan?.live?.length ?? 0, deepCount: scan?.deep?.length ?? 0 });
      if (hit && !errorHit) {
        errorHit = { atMs: i * 250, text: scan?.deep?.[0]?.text ?? scan?.live?.[0]?.text ?? '读取失败' };
        noticeEvidence = scan;
      }
      // 截图留证：命中那一张必拍，其余每 8 拍一张。
      if ((hit && noticeEvidence?.shot === undefined) || i % 8 === 0) {
        try {
          const shot = await cdp.send('Page.captureScreenshot', { format: 'png' }, wb);
          const file = join(EVIDENCE_DIR, `assert-e.toast.${String(i * 250).padStart(5, '0')}ms.png`);
          writeFileSync(file, Buffer.from(shot.data, 'base64'));
          if (noticeEvidence && noticeEvidence.shot === undefined) noticeEvidence.shot = file;
        } catch { /* 截图失败不影响判据 */ }
      }
      if (errorHit) break;
      await sleep(250);
    }
    const draftE = await readDraft(cdp, wb);
    const probeE = await readProbe(cdp, wb);
    const liveNodes = noticeEvidence?.live ?? [];
    const deepNodes = noticeEvidence?.deep ?? [];
    const nodeLevelVisible = liveNodes.some((x) => /读取失败|发送时会拒绝/.test(x.text ?? ''))
      || deepNodes.some((x) => /读取失败|发送时会拒绝/.test(x.text ?? ''));
    const eDetail = {
      picked: pickE.clicked === true,
      newRequests: stub.requests().length - beforeE,
      errorVisible: errorHit !== null,
      errorHit,
      nodeLevelVisible,
      liveNodes,
      deepNodes,
      screenshot: noticeEvidence?.shot ?? null,
      draftRetained: (draftE.draft ?? '').includes('请看这个'),
      serializeFailed: (probeE?.events ?? []).some((e) => e.kind === 'serialize-failed'),
      samples: eSamples.filter((s, i) => s.hit || i === 0 || i === eSamples.length - 1),
    };
    step('E-reject-in-shell', eDetail);
    verdict(
      '断言 E：读取失败则 0 请求、草稿保留、可见错误出口（节点级）',
      eDetail.picked && eDetail.newRequests === 0 && eDetail.errorVisible && eDetail.nodeLevelVisible
        && eDetail.draftRetained && eDetail.serializeFailed ? '通过' : '失败',
      JSON.stringify(eDetail),
    );
  }

  // ⑩ 壳自己的物化链产物
  const artifacts = inspectShellArtifacts();
  step('shell-artifacts', artifacts);
  verdict(
    '物化链产物核对（物化目录 / 三份文件 / 标记 / 接线行）',
    artifacts.materializedExists && artifacts.entries.every((e) => e.exists)
      && artifacts.markerExists && artifacts.patchHasRow ? '通过' : '失败',
    JSON.stringify(artifacts),
  );
} catch (error) {
  step('failed', { error: String(error?.message ?? error), stack: String(error?.stack ?? '') });
  verdict('实跑', '失败', String(error?.message ?? error));
} finally {
  try { cdp?.close(); } catch { /* ignore */ }
  try { stub?.stop(); } catch { /* ignore */ }
  // 汇总：通过 / 失败 / 未执行各多少条（**未执行要单列**——那是「这条机检这条
  // 路径没被压过」的唯一信号，混进「通过」里就看不见了）。
  const summary = { 通过: 0, 失败: 0, 未执行: 0 };
  for (const r of out.results) summary[r.status] = (summary[r.status] ?? 0) + 1;
  out.summary = summary;
  const file = join(EVIDENCE_DIR, `shell-verify.${ROUND}.json`);
  writeFileSync(file, JSON.stringify(out, null, 2), 'utf8');
  console.log(`\n汇总 ${JSON.stringify(summary)}`);
  console.log(`结果 ${file}`);
}
