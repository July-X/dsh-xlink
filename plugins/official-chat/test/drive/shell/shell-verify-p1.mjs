// **P1 壳内真机复验**：驱动真壳（tauri dev）的工作台窗口，证明
// 「client → host 经内核 `connection` 服务的**真实路由注册**」这条链在真机上成立，
// 并把 P1 的三条硬口径落到实测证据上：
//
//   ① **A0（真机级）**：catalog 拿到候选与 **host 预分配的 id**；capture 落盘；
//      发送时**落盘正文与请求体逐字节相同**（S-4）。
//   ② **缺口①**：把快照**删掉**再发送 → **请求数 0** + **可见错误** +
//      客户端**抛错**（不是空正文照发）。
//   ③ **S-1**：同一条对话选两次 → **两个 `snapshotId`**，第二个不覆盖第一个。
//
// ## 为什么另起一个脚本（而不是改 G1 那份）
//
// G1 轮判的是 G1 时代的口径（读取门控、pending 句柄）。P1 的口径完全不同
// （落盘后才算就绪、id 归 host、失效一律拒）。两套判据混在一份脚本里，
// 失败时说不清是哪套的问题。
//
// **公共部分（驱动层）在 `shell-lib.mjs`**：报告状态、CDP 助手、起壳前置流程。
// 判据只在本文件里——判据不许混进驱动层。
//
// ## 判据怎么取证据
//
// · **探针**（client 半边自己记的 `record(...)`）：`capabilities` / `candidates` /
//   `onPick` / `snapshot-saved` / `serialize-*` / `notify`。它是**时间线**，不单独作判据。
// · **盘上的快照文件**（隔离实例 home 里）：落盘是否真的发生、正文是什么。
// · **桩模型收到的请求体**：请求是否真的发出、正文是不是逐字节相同。
//
// ## 隔离
//
// 全部在 `%TEMP%\dsh-xlink-g1\` 下的隔离壳里（见 `paths.mjs`）：壳 home、实例 home、
// WebView2 档案、桩模型端口全在临时目录。**不碰用户真实数据目录。**
//
// ## 用法
//
//   # 终端 A：起隔离真壳
//   node plugins/official-chat/test/drive/shell/shell-dev.mjs
//   # 终端 B：跑本轮
//   node plugins/official-chat/test/drive/shell/shell-verify-p1.mjs
import { existsSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { createHash } from 'node:crypto';

import { evaluate, sleep } from './cdp.mjs';
import { EVIDENCE_DIR, ROUND } from './stub-model.mjs';
import { DATA_DIR, INSTANCE_HOME } from './paths.mjs';
import {
  BODY_ALPHA,
  FIXTURE_ALPHA,
  clearDraft,
  focusComposer,
  out,
  pickFixture,
  pressEnterSubmit,
  prepareShell,
  readBody,
  readDraft,
  readProbe,
  setConfig,
  step,
  verdict,
} from './shell-lib.mjs';

/**
 * 快照落盘根目录的**候选位置**。
 *
 * host 侧的口径是 `<DSH_HOME>/official-chat`（`host/index.js` 的
 * `resolveSnapshotRoot`），而 DSH_HOME 是壳给内核进程注入的**实例 home**。
 * 这里把候选都列上：**不猜**——找到有 `snapshots/` 的那个才算数。
 */
const SNAPSHOT_ROOT_CANDIDATES = [
  join(INSTANCE_HOME, 'official-chat'),
  join(DATA_DIR, 'official-chat'),
  join(INSTANCE_HOME, 'dsh', 'official-chat'),
];

/**
 * 快照落盘根目录（**按需解析**，不是启动时解析一次）。
 *
 * host 侧的口径是 `<DSH_HOME>/official-chat`（`host/index.js` 的
 * `resolveSnapshotRoot`），而 DSH_HOME 是壳给内核进程注入的**实例 home**。
 *
 * **为什么必须按需解析**：快照目录是**第一次落盘时**才建出来的——启动时（或
 * pick 之前）去问，目录还不存在 ⇒ 找不到 ⇒ 后面全判「没落盘」。那是**假红**。
 *
 * 这里把候选都列上：**不猜**——找到有 `snapshots/` 的那个才算数。
 *
 * @returns {string|null} 快照根目录；找不到就是 null。
 */
function findSnapshotRoot() {
  for (const root of SNAPSHOT_ROOT_CANDIDATES) {
    if (existsSync(join(root, 'snapshots'))) return root;
  }
  // 还没落过盘时，认领目录（`catalog` 就会建）也是同一个根的证据。
  for (const root of SNAPSHOT_ROOT_CANDIDATES) {
    if (existsSync(join(root, 'seq-claims'))) return root;
  }
  return null;
}

/** 列出盘上所有快照 id（文件��不带扩展名）。 */
function listSnapshotIds(root) {
  if (root === null) return [];
  const out = [];
  for (const provider of readdirSync(join(root, 'snapshots'), { withFileTypes: true })) {
    if (!provider.isDirectory()) continue;
    const dir = join(root, 'snapshots', provider.name);
    for (const file of readdirSync(dir)) {
      if (file.endsWith('.json')) out.push({ provider: provider.name, id: file.slice(0, -'.json'.length) });
    }
  }
  return out;
}

/**
 * 读盘上一份快照，返回**独立算出来的**正文。
 *
 * **正文口径不 import 实现的 `joinMessages`**——那样就成了「自己判自己」。
 * 这里按协议 §1.3 的字面规则自己拼：消息按顺序、**空行分隔**（`\n\n`）。
 * 摘要也自己算（SHA-256），与盘上 `contentDigest` 对照。
 *
 * @param {string} root - 快照根目录。
 * @param {string} id - 快照 id。
 * @returns {object|null} 快照与自算正文。
 */
function readSnapshot(root, id) {
  const providers = root === null ? [] : readdirSync(join(root, 'snapshots'));
  for (const provider of providers) {
    const path = join(root, 'snapshots', provider, `${id}.json`);
    if (!existsSync(path)) continue;
    const parsed = JSON.parse(readFileSync(path, 'utf8'));
    const messages = Array.isArray(parsed.messages) ? parsed.messages : [];
    const body = messages.map((m) => String(m?.text ?? '')).join('\n\n');
    const digest = createHash('sha256').update(body, 'utf8').digest('hex');
    return {
      path,
      parsed,
      body,
      selfDigest: digest,
      digestMatches: digest === String(parsed.contentDigest ?? ''),
    };
  }
  return null;
}

/**
 * 在桩模型收到的请求里找**对话请求**（带系统提示的那条）。
 *
 * @param {object[]} requests - 桩模型记下的请求。
 * @returns {object|null} 对话请求。
 */
function findChatRequest(requests) {
  return requests.find((r) => JSON.stringify(r.body ?? {}).includes('You are an AI agent')) ?? null;
}

/**
 * 在一条对话请求里数「盘上正文」出现了几次，并记下每次的**精确位置**。
 *
 * **必须在「未转义的字符串叶子」里找，不能在 `JSON.stringify(...)` 的结果里找。**
 *
 * 原因（p1-r5 踩过）：`JSON.stringify` 会把正文里的**真换行**写成字面 `\n`
 * （反斜杠 + n）。拿「含真换行的盘上正文」去 `indexOf` 一个**已转义**的字符串，
 * **永远匹配不上** ⇒ 判「正文没进请求体」。而请求体里其实**有**正文——
 * 真机上它长这样：`【G1 固定正文 Alpha】第一行固定正文。\n第二行固定正文。 请看这个`
 * （盘上正文 + 用户在输入框里打的字）。
 *
 * 所以这里**深度遍历请求体**，在每个**字符串叶子**（原样，未经 JSON 转义）里找，
 * 并逐段核对该段**与盘上正文逐字节相同**。
 *
 * @param {object} request - 对话请求。
 * @param {string} body - 盘上正文。
 * @returns {{count: number, hits: object[]}} 出现情况。
 */
function locateBody(request, body) {
  // **空正文必须先挡住**：`indexOf('', from)` 永远返回一个合法位置 ⇒
  // 空正文会让这个循环**永不结束**，把 indexes 撑到「Invalid array length」。
  if (body === '') return { count: 0, hits: [] };
  const hits = [];
  const walk = (node, path) => {
    if (typeof node === 'string') {
      let from = 0;
      for (;;) {
        const at = node.indexOf(body, from);
        if (at < 0) break;
        hits.push({
          path,
          offset: at,
          chars: body.length,
          bytes: Buffer.byteLength(body, 'utf8'),
          // **逐字节**：切出来的那一段与盘上正文**完全一致**。
          exact: node.slice(at, at + body.length) === body,
        });
        from = at + body.length;
      }
      return;
    }
    if (Array.isArray(node)) {
      node.forEach((value, index) => walk(value, `${path}[${index}]`));
      return;
    }
    if (node !== null && typeof node === 'object') {
      for (const key of Object.keys(node)) {
        walk(node[key], path === '' ? key : `${path}.${key}`);
      }
    }
  };
  walk(request.body ?? {}, '');
  return { count: hits.length, hits };
}

/**
 * **判据自检**：先证明「找正文」这件事本身是对的，再拿它去判实现。
 *
 * 造四种请求体：① 含**带真换行**的原正文 ② 正文被改一个字 ③ 完全没有正文
 * ④ 空正文。期望：①命中且逐字节相同；②③④一个都不命中。
 *
 * **为什么必须自检**：判据自己写错时，它给出的「红」和「绿」同样不可信。
 * p1-r5 就踩过这个：判据在 JSON 转义后的字符串里找含换行的正文 ⇒ 一条都找不到
 * ⇒ 把「请求体里明明有正文」报成了**失败**。自检把「判据坏了」与「实现坏了」
 * 分成两件事。
 *
 * @returns {{ok: boolean, detail: object}} 自检结果。
 */
function selfCheckLocate() {
  // 盘上正文是**一条消息内含单换行**（76 字节）——这里照那个形态造夹具。
  const body = `${BODY_ALPHA}\n第二行固定正文。`;
  const good = locateBody({ body: { messages: [{ role: 'user', content: `前缀\n${body} 后缀` }] } }, body);
  const tampered = locateBody(
    { body: { messages: [{ role: 'user', content: body.replace('第一行', '第一行x') }] } },
    body,
  );
  const absent = locateBody({ body: { messages: [{ role: 'user', content: '完全无关的内容' }] } }, body);
  const empty = locateBody({ body: { messages: [{ role: 'user', content: 'x' }] } }, '');
  const detail = {
    bodyBytes: Buffer.byteLength(body, 'utf8'),
    goodHits: good.count,
    goodExact: good.hits.length > 0 && good.hits.every((h) => h.exact),
    tamperedHits: tampered.count,
    absentHits: absent.count,
    emptyHits: empty.count,
  };
  const ok = detail.goodHits === 1
    && detail.goodExact
    && detail.tamperedHits === 0
    && detail.absentHits === 0
    && detail.emptyHits === 0;
  return { ok, detail };
}

/**
 * 密集采样页面上的**可见错误出口**（错误级 Toast / role=alert / aria-live）。
 *
 * **必须密集采样**：内核把 `error` 级 notice 渲染成 Toast，带 `onDone` 自动消失
 * （`ui-conversation/client.js@0.2.1-rc.2:18808`）——单点快照会踩在它消失之后。
 *
 * @param {object} cdp - CDP。
 * @param {string} sessionId - 工作台 session。
 * @returns {Promise<object>} 采样结果。
 */
async function scanVisibleError(cdp, sessionId) {
  return evaluate(
    cdp,
    sessionId,
    `(() => {
       const body = document.body;
       const inner = body ? (body.innerText || '') : '';
       const content = body ? (body.textContent || '') : '';
       const live = [...document.querySelectorAll('[role="alert"],[role="status"],[aria-live]')]
         .map((el) => ({
           role: el.getAttribute('role'),
           live: el.getAttribute('aria-live'),
           text: (el.textContent || '').trim().slice(0, 200),
         }))
         .filter((x) => x.text.length > 0)
         .slice(0, 8);
       const deep = [];
       for (const el of document.querySelectorAll('*')) {
         const t = el.textContent || '';
         if (t && t.length < 400 && (t.includes('引用') || t.includes('快照') || t.includes('失败') || t.includes('拒绝'))) {
           deep.push({
             tag: el.tagName,
             cls: String(el.getAttribute('class') || '').slice(0, 90),
             role: el.getAttribute('role'),
             text: t.trim().slice(0, 200),
           });
         }
       }
       return { inner, content, live, deep: deep.slice(0, 8) };
     })()`,
  );
}

let cdp;
let stub;
try {
  // ── 判据自检：先证明「找正文」这件事本身是对的，再开真机跑 ──────────────
  // 判据坏了就别开壳——那样出来的红/绿都不可信（p1-r5 的教训）。
  const selfCheck = selfCheckLocate();
  verdict(
    '判据自检：能在请求体里定位盘上正文（含真换行），且篡改/缺失/空正文都不误命中',
    selfCheck.ok ? '通过' : '失败',
    JSON.stringify(selfCheck.detail),
  );
  if (!selfCheck.ok) throw new Error(`判据自检未通过：${JSON.stringify(selfCheck.detail)}`);

  const prepared = await prepareShell();
  cdp = prepared.cdp;
  stub = prepared.stub;
  const { wb } = prepared;

  // ── 断言 1：A0 真机级——client → host 经内核 connection 服务的真实路由 ──────
  //
  // **证据是什么**：client 半边只有一条路能问 host（协议 §5，**v0.4 裁定**）：
  // **自己发 same-origin fetch** 到 `/api/xlink-official-chat`，命中内核
  // `connection` 服务注册的 **exact fetch route**（`registerFetchRoute`，
  // `dsh-client-connection/lib/index.js@0.2.1-rc.2:840-854`——**完全不碰 webServer**）。
  // 所以「client 拿到了 host 的能力回包」= **那条路由真的注册上了并服务了一次**。
  // 反面信号：`capabilities-failed` / `catalog-unavailable` = 路由没注册上。
  //
  // **为什么不能用浏览器侧内核自带的 `connection.rpc.call`**：它的 URL 是
  // `` `${channel}/${endpoint}`.slice(1) ``（相对路径、**不带 `/api`**，
  // `dsh-client-connection/lib/client.browser.js@0.2.1-rc.2:1221`）
  // ⇒ 打不到 `/api` 命名空间下的 exact route。
  //
  // **这一轮验的是「新通道绕开了 `rpc.handle` 那一行上游缺口」**——所以它必须跑在
  // **未打补丁的现役内核**上（隔离副本与现役内核逐字节一致，SHA-256 已核）。
  await sleep(3000);
  const probe0 = await readProbe(cdp, wb);
  const events0 = probe0?.events ?? [];
  const capsEvent = events0.find((e) => e.kind === 'capabilities') ?? null;
  const capsFailed = events0.find((e) => e.kind === 'capabilities-failed') ?? null;
  const a0Detail = {
    capabilitiesOk: capsEvent !== null,
    capabilitiesFailed: capsFailed,
    schemaVersion: capsEvent?.schemaVersion ?? null,
    providerId: capsEvent?.providerId ?? null,
    hostServiceUnavailable: events0.some((e) => e.kind === 'host-service-unavailable'),
    probeKinds: events0.map((e) => e.kind),
  };
  step('A0-capabilities-over-exact-fetch', a0Detail);
  verdict(
    '断言 1（A0 真机级）：client 经内核 connection 服务的 exact fetch route 拿到 host 的 capabilities（路由真的注册上了）',
    a0Detail.capabilitiesOk && a0Detail.hostServiceUnavailable === false ? '通过' : '失败',
    JSON.stringify(a0Detail),
  );

  // ── 断言 2：catalog 拿到候选 + host 预分配的 id ──────────────────────────
  await setConfig(cdp, wb, { readDelayMs: 0, failRead: false });
  await clearDraft(cdp, wb);
  await focusComposer(cdp, wb);
  await evaluate(cdp, wb, `(() => { document.execCommand('insertText', false, '@'); return true; })()`);
  await sleep(3000);
  const menuBody = await readBody(cdp, wb);
  const probe2 = await readProbe(cdp, wb);
  const candEvent = (probe2?.events ?? []).filter((e) => e.kind === 'candidates').pop() ?? null;
  const catalogFailed = (probe2?.events ?? []).find((e) => e.kind === 'catalog-failed') ?? null;
  const catalogUnavailable = (probe2?.events ?? []).find((e) => e.kind === 'catalog-unavailable') ?? null;
  const menuDetail = {
    hasGroup: menuBody.includes('官方对话'),
    hasFixtureRow: menuBody.includes(FIXTURE_ALPHA),
    candidateCount: candEvent?.count ?? null,
    allocated: candEvent?.allocated ?? null,
    catalogFailed,
    catalogUnavailable,
  };
  step('A0-catalog-with-allocations', menuDetail);
  verdict(
    '断言 2（A0 真机级）：catalog 经真实路由返回候选，并带 host 预分配的 id',
    menuDetail.hasGroup && menuDetail.hasFixtureRow && (menuDetail.candidateCount ?? 0) > 0
      && (menuDetail.allocated ?? 0) > 0 && catalogFailed === null && catalogUnavailable === null
      ? '通过' : '失败',
    JSON.stringify(menuDetail),
  );
  await clearDraft(cdp, wb);

  // ── 断言 3：pick → 用 host 分配的 id → 落盘 ───────────────────────────────
  const pick1 = await pickFixture(cdp, wb, FIXTURE_ALPHA);
  await sleep(2500);
  const probe3 = await readProbe(cdp, wb);
  const pickEvent = (probe3?.events ?? []).filter((e) => e.kind === 'onPick').pop() ?? null;
  const savedEvent = (probe3?.events ?? []).filter((e) => e.kind === 'snapshot-saved').pop() ?? null;
  const id1 = String(pickEvent?.snapshotId ?? '');
  // **根目录按需解析，且落盘是异步的** ⇒ 等它出现（等不到就如实报「没落盘」，
  // 不把「还没写完」当成「没写」）。
  let root = null;
  let snapshot1 = null;
  for (let i = 0; i < 20; i += 1) {
    root = findSnapshotRoot();
    snapshot1 = id1 === '' ? null : readSnapshot(root, id1);
    if (snapshot1 !== null) break;
    await sleep(300);
  }
  const draft3 = await readDraft(cdp, wb);
  const pickDetail = {
    clicked: pick1.clicked === true,
    snapshotId: id1,
    idShapeOk: /^[a-z0-9]{1,4}~[a-z0-9]{1,12}~\d{4}$/.test(id1),
    savedEvent: savedEvent,
    snapshotRoot: root,
    fileExists: snapshot1 !== null,
    filePath: snapshot1?.path ?? null,
    digestMatches: snapshot1?.digestMatches ?? false,
    selfDigest: snapshot1?.selfDigest ?? null,
    fileDigest: snapshot1?.parsed?.contentDigest ?? null,
    bodyBytes: snapshot1 === null ? 0 : Buffer.byteLength(snapshot1.body, 'utf8'),
    bodyHead: snapshot1 === null ? null : snapshot1.body.slice(0, 80),
    chipInDraft: (draft3.html ?? '').includes('data-composer-chip="official-chat"'),
    bodyNotInDraft: (draft3.html ?? '').includes(BODY_ALPHA) === false,
  };
  step('A0-pick-and-land', pickDetail);
  verdict(
    '断言 3（A0 真机级）：pick 用的是 host 分配的 id，且快照真的落盘（自算摘要对得上）',
    pickDetail.clicked && pickDetail.idShapeOk && pickDetail.fileExists && pickDetail.digestMatches
      && pickDetail.chipInDraft ? '通过' : '失败',
    JSON.stringify(pickDetail),
  );

  // ── 断言 4：S-4——发送时落盘正文与请求体逐字节相同 ──────────────────────
  await evaluate(cdp, wb, `(() => { document.execCommand('insertText', false, '请看这个'); return true; })()`);
  const beforeSend = stub.requests().length;
  const t0 = Date.now();
  await pressEnterSubmit(cdp, wb);
  let arrived = null;
  for (let i = 0; i < 60; i += 1) {
    if (stub.requests().length > beforeSend) { arrived = Date.now() - t0; break; }
    await sleep(500);
  }
  const reqs = stub.requests().slice(beforeSend);
  const chatReq = findChatRequest(reqs);
  const bodyOnDisk = snapshot1?.body ?? '';
  const locate = chatReq === null ? { count: 0, hits: [] } : locateBody(chatReq, bodyOnDisk);
  const draft4 = await readDraft(cdp, wb);
  const s4Detail = {
    waitedMs: arrived,
    newRequests: reqs.length,
    chatRequestFound: chatReq !== null,
    bodyBytes: Buffer.byteLength(bodyOnDisk, 'utf8'),
    bodyOnDiskHead: bodyOnDisk.slice(0, 120),
    occurrencesInChatRequest: locate.count,
    occurrenceHits: locate.hits,
    // **逐字节**：请求体里那几段与盘上正文**逐字节相同**——
    // 不是「长度非 0」这种恒真判断（p1-r5 那版 `indexes.every(() => …)` 恒真）。
    byteExact: locate.count > 0
      && locate.hits.every((h) => h.exact && h.bytes === Buffer.byteLength(bodyOnDisk, 'utf8')),
    draftCleared: (draft4.draft ?? '').trim() === '',
    serializeOk: (await readProbe(cdp, wb))?.events?.some((e) => e.kind === 'serialize-ok') ?? false,
  };
  step('S4-request-matches-disk', s4Detail);
  verdict(
    '断言 4（S-4）：发送时请求体与盘上正文逐字节相同（且请求真的发出）',
    s4Detail.newRequests > 0 && s4Detail.chatRequestFound && s4Detail.occurrencesInChatRequest > 0
      && s4Detail.byteExact && s4Detail.serializeOk ? '通过' : '失败',
    JSON.stringify(s4Detail),
  );

  // ── 断言 5：S-1——同一条对话选两次 → 两个 id，第二个不覆盖第一个 ─────────
  await clearDraft(cdp, wb);
  const pick2 = await pickFixture(cdp, wb, FIXTURE_ALPHA);
  await sleep(2500);
  const probe5 = await readProbe(cdp, wb);
  const pickEvents = (probe5?.events ?? []).filter((e) => e.kind === 'onPick');
  const id2 = String(pickEvents[pickEvents.length - 1]?.snapshotId ?? '');
  const root5 = findSnapshotRoot();
  const snapshot2 = id2 === '' ? null : readSnapshot(root5, id2);
  const snapshot1Again = id1 === '' ? null : readSnapshot(root5, id1);
  const s1Detail = {
    firstId: id1,
    secondId: id2,
    distinct: id1 !== '' && id2 !== '' && id1 !== id2,
    bothFilesExist: snapshot1Again !== null && snapshot2 !== null,
    firstStillThere: snapshot1Again !== null && snapshot1Again.digestMatches,
    firstDigestUnchanged:
      snapshot1Again !== null && snapshot1Again.selfDigest === snapshot1?.selfDigest,
    secondDigestMatches: snapshot2?.digestMatches ?? false,
    allOnDisk: listSnapshotIds(root5),
  };
  step('S1-two-distinct-ids', s1Detail);
  verdict(
    '断言 5（S-1）：同一条对话选两次 → 两个 snapshotId，第二个不覆盖第一个',
    s1Detail.distinct && s1Detail.bothFilesExist && s1Detail.firstStillThere
      && s1Detail.firstDigestUnchanged ? '通过' : '失败',
    JSON.stringify(s1Detail),
  );

  // ── 断言 6：缺口①——快照被删 → 请求数 0 + 可见错误 + 客户端抛错 ──────────
  //
  // **删的是刚落的盘**（模拟「快照被清理」）：host 的 `preview` 会按固定 id 读不到
  // ⇒ 拒（`snapshot_invalid`）⇒ 客户端 serialize 抛错 ⇒ 内核拒绝发送 ⇒ 请求数 0。
  await clearDraft(cdp, wb);
  const pick3 = await pickFixture(cdp, wb, FIXTURE_ALPHA);
  await sleep(2500);
  const probe6a = await readProbe(cdp, wb);
  const pickEvents6 = (probe6a?.events ?? []).filter((e) => e.kind === 'onPick');
  const id3 = String(pickEvents6[pickEvents6.length - 1]?.snapshotId ?? '');
  const root6 = findSnapshotRoot();
  let snapshot3 = id3 === '' ? null : readSnapshot(root6, id3);
  for (let i = 0; i < 20 && snapshot3 === null; i += 1) {
    await sleep(300);
    snapshot3 = readSnapshot(root6, id3);
  }
  let deleted = false;
  if (snapshot3 !== null) {
    rmSync(snapshot3.path, { force: true });
    deleted = !existsSync(snapshot3.path);
  }
  await evaluate(cdp, wb, `(() => { document.execCommand('insertText', false, '再来一次'); return true; })()`);
  const beforeReject = stub.requests().length;
  await pressEnterSubmit(cdp, wb);
  let errorHit = null;
  let errorEvidence = null;
  const samples = [];
  for (let i = 0; i < 40; i += 1) {
    const scan = await scanVisibleError(cdp, wb);
    const hit =
      (scan?.live ?? []).some((x) => /引用|快照|失败|拒绝|失效/.test(x.text ?? ''))
      || (scan?.deep ?? []).some((x) => /引用|快照|失败|拒绝|失效/.test(x.text ?? ''));
    samples.push({ atMs: i * 250, hit, liveCount: (scan?.live ?? []).length, deepCount: (scan?.deep ?? []).length });
    if (hit && errorHit === null) {
      errorHit = { atMs: i * 250, live: scan?.live ?? [], deep: scan?.deep ?? [] };
      errorEvidence = scan;
    }
    if (errorHit !== null) break;
    await sleep(250);
  }
  const reqsReject = stub.requests().slice(beforeReject);
  const draft6 = await readDraft(cdp, wb);
  const probe6 = await readProbe(cdp, wb);
  const rejectDetail = {
    picked: pick3.clicked === true,
    snapshotId: id3,
    snapshotLandedBefore: snapshot3 !== null,
    snapshotDeleted: deleted,
    newRequests: reqsReject.length,
    errorVisible: errorHit !== null,
    errorHit,
    serializeFailed: (probe6?.events ?? []).some((e) => e.kind === 'serialize-failed'),
    draftRetained: (draft6.draft ?? '').includes('再来一次'),
    samples: samples.filter((s, i) => s.hit || i === 0 || i === samples.length - 1),
  };
  step('gap1-deleted-snapshot-rejected', rejectDetail);
  verdict(
    '断言 6（缺口①）：快照被删 → 请求数 0 + 可见错误 + 客户端抛错（不是空正文照发）',
    rejectDetail.picked && rejectDetail.snapshotLandedBefore && rejectDetail.snapshotDeleted
      && rejectDetail.newRequests === 0 && rejectDetail.errorVisible && rejectDetail.serializeFailed
      ? '通过' : '失败',
    JSON.stringify(rejectDetail),
  );
} catch (error) {
  step('failed', { error: String(error?.message ?? error), stack: String(error?.stack ?? '') });
  verdict('实跑', '失败', String(error?.message ?? error));
} finally {
  try { cdp?.close(); } catch { /* ignore */ }
  try { stub?.stop(); } catch { /* ignore */ }
  const summary = { 通过: 0, 失败: 0, 未执行: 0 };
  for (const r of out.results) summary[r.status] = (summary[r.status] ?? 0) + 1;
  out.summary = summary;
  const file = join(EVIDENCE_DIR, `shell-verify-p1.${ROUND}.json`);
  writeFileSync(file, JSON.stringify(out, null, 2), 'utf8');
  console.log(`\n汇总 ${JSON.stringify(summary)}`);
  console.log(`结果 ${file}`);
}
