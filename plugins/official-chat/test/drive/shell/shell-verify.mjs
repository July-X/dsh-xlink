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
//   ③ 读取延迟时按发送 → 桩模型在读取完成前 0 请求、完成后请求体含夹具正文；
//      读取失败 → 0 请求 + 可见错误 + 草稿保留。
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
import { writeFileSync } from 'node:fs';
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
  await clearDraft(cdp, wb);
  await focusComposer(cdp, wb);
  await evaluate(cdp, wb, `(() => { document.execCommand('insertText', false, '@'); return true; })()`);
  await sleep(2500);
  const menuBody = await readBody(cdp, wb);
  const aDetail = {
    hasGroup: menuBody.includes('Web Chat') && menuBody.includes(FIXTURE_ALPHA),
    snippet: menuBody.split('\n').filter((l) => l.includes('Web Chat') || l.includes(FIXTURE_ALPHA)).slice(0, 5),
  };
  step('A-menu-in-shell', aDetail);
  verdict('断言 A：@ 菜单里出现「Web Chat」分组', aDetail.hasGroup ? '通过' : '失败', JSON.stringify(aDetail));
  await clearDraft(cdp, wb);

  // ⑦ ②：chip 落地且是内核节点（正文不在草稿里）
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

  // ⑧ ③：发送门控 —— 等待
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
  // **口径**：正文「只出现一次」指的是**这一次发送**在**对话请求**里出现一次。
  // 会话是跨轮复用的时，历史里本来就躺着上一轮那条 → 同一个请求里会出现两次。
  // 所以这里把「本次那条」与「历史那条」分开数，别让人拿总数当重复外发。
  const chatRequest = reqsD.find((r) => JSON.stringify(r.body ?? {}).includes('You are an AI agent')) ?? null;
  const chatMessages = chatRequest?.body?.messages ?? [];
  const alphaIdx = chatMessages
    .map((m, i) => ({ i, m, c: typeof m.content === 'string' ? m.content : JSON.stringify(m.content ?? '') }))
    .filter((x) => x.c.includes(BODY_ALPHA))
    .map((x) => x.i);
  const dDetail = {
    picked: pickD.clicked === true,
    waitedMs: arrived,
    newRequests: reqsD.length,
    requestHasBody: reqsD.some((r) => JSON.stringify(r.body ?? {}).includes(BODY_ALPHA)),
    serializeOk: (await readProbe(cdp, wb))?.events?.some((e) => e.kind === 'serialize-ok') ?? false,
    draftCleared: (draftAfter.draft ?? '').trim() === '',
    sentText: bodyAfter.includes('请看这个'),
    // ↓ 口径字段：对话请求里正文出现在哪几条消息、历史里有没有上一轮的 assistant 回包
    chatRequestCount: reqsD.filter((r) => JSON.stringify(r.body ?? {}).includes('You are an AI agent')).length,
    bodyOccurrenceIndexes: alphaIdx,
    bodyOccurrencesInChatRequest: alphaIdx.length,
    thisTurnIndex: alphaIdx.length > 0 ? alphaIdx[alphaIdx.length - 1] : null,
    historyAssistantReplies: chatMessages.filter((m) => m.role === 'assistant').length,
    otherRequestPurposes: reqsD
      .filter((r) => !JSON.stringify(r.body ?? {}).includes('You are an AI agent'))
      .map((r) => (JSON.stringify(r.body ?? {}).includes('Create a concise title') ? 'session-title' : 'other')),
    timeline: timeline.filter((x, i) => i === 0 || i === timeline.length - 1 || x.reqs > 0),
  };
  step('D-send-gate-in-shell', dDetail);
  verdict(
    '断言 D：读取门控（读取完成前 0 请求，完成后请求体含夹具正文）',
    dDetail.picked && dDetail.waitedMs !== null && dDetail.requestHasBody
      && dDetail.serializeOk && dDetail.draftCleared ? '通过' : '失败',
    JSON.stringify(dDetail),
  );

  // ⑨ ③：发送门控 —— 拒绝（错误提示是**瞬态 Toast**，必须密集采样）
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
