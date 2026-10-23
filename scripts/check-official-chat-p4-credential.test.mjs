// P4 §10 判据**第 9 条**（凭据机制）的**自测**。
//
// ## 判什么
//
// · **四个合成用例必须自证**（PM 的 `p4-criterion9-draft.md` §2.3 定的验收口径）：
//   ① 写 `Authorization` ⇒ **9a 红**（点名 `Authorization`）
//   ② `localStorage.getItem('userToken')` ⇒ **9b 红**（点名 `userToken`）
//   ③ `localStorage.getItem('some_theme')` ⇒ **9b 绿**（**别的键不判**）
//   ④ 干净夹具 ⇒ **全绿**
// · **反向不误伤**：注释里提到 `userToken` 不该红；页内 `fetch('/api/v0/…')` 是**协议 §1 要求的**
//   （页内同源取数）⇒ 不该被 9c 判红。
// · **9d 已转真判据**（协议 v0.9 起不再占位）：**形态唯一 ＝ 壳侧 `Network` 域订阅、页内零动作**
//   ⇒ ⇒ 9d 断五项（三项「存在」＋ 两项「不存在」）⇒ **全局判定**（`judgeChannelShape`，
//   因为「存在性」要跨文件合看）⇒ ⇒ **四条自证场景**：真身通过／删掉壳侧形态必红／
//   塞入页内取数形态必红／塞入页内旁听形态必红。
// · **真机冒烟**：对真实扫描面跑一遍，结果**自洽**（红绿**不硬断言** —— 实现会演进）。
//
// ## 纪律
//
// · 夹具落 `%TEMP%` 一次性目录，**跑完删自己建的**；**永不碰用户的真实数据目录**。
// · 判据**不发任何网络请求**、不碰凭据值。
//
// @module scripts/check-official-chat-p4-credential-test

import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

import {
  FAIL,
  FORM_A,
  FORM_B,
  FORM_C,
  FORM_TRACE_SCOPE,
  PASS,
  SKIPPED,
  TRACE_SHAPE_SCAN_GLOBS,
  attributeForms,
  checkSource,
  collectSources,
  judgeChannelShape,
  judgeColdCredential,
  judgeCredentialSurface,
  judgeInpageTraceShape,
  readColdCredentialReading,
  scanSource,
  summarize,
} from './check-official-chat-p4-credential.mjs';
import { COLD_PROFILE_REL } from './lib/p4-source-shape.mjs';
import {
  FORM_A as TRACE_FORM_A,
  fakeSnapshot,
  judgeTrace,
} from './check-official-chat-p4-trace.mjs';

/**
 * **形态 A 八条断言的「基线 sha256」——第二道钉子**（**故意与 `trace.test.mjs` 里的那份分开存放**）。
 *
 * ## 为什么要有这一道
 *
 * PM 的要求：「**硬编码名单自证不算保证**」⇒ ⇒ 得是「**清单 vs 实际断言**逐条比对，
 * 比对失败即红」⇒ ⇒ 本文件**不复制名单**（复制会被重复代码判据当重复块拦下），
 * 只放**一个 sha256**：它 ＝ 那八条断言名的规范 JSON 的摘要。
 *
 * ⇒ ⇒ 有人同时改判据**和** `trace.test.mjs` 里那份名单时，**本文件这一处仍会红**。
 *
 * @see ./check-official-chat-p4-trace.test.mjs 的 `FORM_A_BASE_CHECKS`（可读的那份名单）
 */
const FORM_A_BASELINE_SHA256_MIRROR = '668125b3653403d9ace43f86edd01ca39120cb0aa598977f88e4f14ad1601742';

/** 形态 A 的**形态专属**那一条（**不在这八条里**）—— 算基线 JSON 时要剔掉。 */
const FORM_A_ONLY_CHECK_NAME = '形态 A · 中途快照也无页内变化（形态 A ＝ 页内零动作）';

const REPO = fileURLToPath(new URL('../', import.meta.url));

/**
 * 在 `%TEMP%` 下建一个一次性目录，**用完删自己建的**。
 *
 * @param {string} prefix - 目录名前缀。
 * @returns {string} 目录绝对路径。
 */
function tempDir(prefix) {
  return mkdtempSync(join(tmpdir(), prefix));
}

/**
 * 找某条判据的结果。
 *
 * @param {Array<{name: string, status: string, detail: string}>} results
 * @param {string} keyword - 判据名关键词。
 * @returns {{name: string, status: string, detail: string}}
 */
function pick(results, keyword) {
  const found = results.find((r) => r.name.includes(keyword));
  assert.ok(found, `判据结果里找不到「${keyword}」`);
  return found;
}

test('用例 1 · 写 Authorization ⇒ 9a 红（点名 Authorization）', () => {
  const code = [
    "const headers = { Authorization: 'Bearer x', 'Content-Type': 'application/json' };",
    "await fetch('/api/v0/chat_session/fetch_page', { headers });",
  ].join('\n');
  const results = checkSource(code, 'fixture-1');
  const a = pick(results, '9a');
  assert.equal(a.status, FAIL, '9a 必须红');
  assert.match(a.detail, /authorization/i, '9a 必须点名凭据头');
  // 9b / 9c 不该被这一个夹具带红
  assert.equal(pick(results, '9b').status, PASS);
  assert.equal(pick(results, '9c').status, PASS);
});

test('用例 2 · localStorage.getItem(\'userToken\') ⇒ 9b 红（点名 userToken）', () => {
  const code = [
    "const token = localStorage.getItem('userToken');",
    "if (token) window.__dsToken = token;",
  ].join('\n');
  const results = checkSource(code, 'fixture-2');
  const b = pick(results, '9b');
  assert.equal(b.status, FAIL, '9b 必须红');
  assert.match(b.detail, /userToken/, '9b 必须点名凭据键');
  assert.equal(pick(results, '9a').status, PASS);
});

test('用例 3 · localStorage.getItem(\'some_theme\') ⇒ 9b 绿（别的键不判）', () => {
  const code = "const theme = localStorage.getItem('some_theme');";
  const results = checkSource(code, 'fixture-3');
  assert.equal(pick(results, '9b').status, PASS, '非凭据键不得判红');
  assert.equal(pick(results, '9a').status, PASS);
});

test('用例 4 · 干净夹具 ⇒ 9a/9b/9c 全绿（9d 已移到全局层）', () => {
  const code = [
    '// 复用站点自己的请求：拦截 XHR 读响应结构',
    'const origOpen = XMLHttpRequest.prototype.open;',
    'const origSend = XMLHttpRequest.prototype.send;',
    'XMLHttpRequest.prototype.open = function (method, url) {',
    '  window.__dsRequests = window.__dsRequests || [];',
    '  window.__dsRequests.push({ method, url });',
    '  return origOpen.apply(this, arguments);',
    '};',
    'XMLHttpRequest.prototype.send = function () {',
    '  return origSend.apply(this, arguments);',
    '};',
  ].join('\n');
  const results = checkSource(code, 'fixture-4');
  assert.equal(pick(results, '9a').status, PASS);
  assert.equal(pick(results, '9b').status, PASS);
  assert.equal(pick(results, '9c').status, PASS);
  // ⚠️ **9d 已不在 `checkSource()` 里** —— 它是**全局**判定（存在性要跨文件合看）。
  assert.equal(results.length, 3, 'checkSource 只判 9a/9b/9c 三条');
  // ⚠️ 这条夹具在 **9d 的口径下属于「页内旁听形态」** ⇒ 9d 判它红（见下面场景 ④）。
});

// ── 9d 的四条自证场景（协议 §10 第 9 条 9d ＋ PM 派单：**各点名是哪个形态**）─────

/**
 * **壳侧取数链的「真身」夹具**（形态唯一 ＝ 壳侧 `Network` 域订阅、页内零动作）。
 *
 * ⚠️ **路径必须是 P4 取数链上的真实路径** —— 9d 判「页内形态」时**只扫这条链**
 * （见判据里 `CHANNEL_SCOPE_FILES` 的理由）。
 *
 * @param {string[]} [extraLines] - 追加的行（用来塞违规形态）。
 * @returns {{path: string, code: string}}
 */
function shellFixture(extraLines = []) {
  return {
    path: 'src-tauri/src/official_chat/session_structure.rs',
    code: [
      'const SUBSCRIBED_EVENTS: [&str; 2] = ["Network.responseReceived", "Network.loadingFinished"];',
      'cdp_call(&window, "Network.enable", json!({}), CDP_TIMEOUT).await?;',
      'let envelope = cdp_call(window, "Network.getResponseBody", json!({ "requestId": request_id }), CDP_TIMEOUT).await?;',
      ...extraLines,
    ].join('\n'),
  };
}

test('9d 场景 ① · 真身（壳侧 Network 域订阅、页内零动作）⇒ 通过', () => {
  const d = judgeChannelShape([shellFixture()]);
  assert.equal(d.status, PASS, `真身必须通过：${d.detail}`);
  assert.match(d.detail, /五项全过/);
});

test('9d 场景 ② · 删掉壳侧形态（没有 Network.enable）⇒ 必红（点名 Network.enable）', () => {
  const src = shellFixture();
  const stripped = {
    path: src.path,
    code: src.code.split('\n').filter((l) => !l.includes('Network.enable')).join('\n'),
  };
  const d = judgeChannelShape([stripped]);
  assert.equal(d.status, FAIL, '缺壳侧形态必须红');
  assert.match(d.detail, /Network\.enable/, '必须点名缺的是 Network.enable');
});

test('9d 场景 ③ · 塞入页内取数形态（页内脚本发请求取结构）⇒ 必红（点名页内取数形态）', () => {
  const d = judgeChannelShape([
    shellFixture(['let injected = r#"const r = await fetch(\'/api/v0/chat_session/fetch_page\');"#;']),
  ]);
  assert.equal(d.status, FAIL, '页内取数形态必须红');
  assert.match(d.detail, /页内发请求\/取数形态/, '必须点名是页内取数形态');
});

test('9d 场景 ④ · 塞入页内旁听形态（hook prototype ＋ 留槽位）⇒ 必红（点名页内旁听形态）', () => {
  const d = judgeChannelShape([
    shellFixture([
      'const origOpen = XMLHttpRequest.prototype.open;',
      'XMLHttpRequest.prototype.open = function () { return origOpen.apply(this, arguments); };',
      'window.__dsh_rt_1 = 0;',
    ]),
  ]);
  assert.equal(d.status, FAIL, '页内旁听形态必须红');
  assert.match(d.detail, /页内旁听形态/, '必须点名是页内旁听形态');
});

test('真机冒烟 · 真实扫描面上的 9d 必须通过（形态唯一 ＝ 壳侧 Network 域订阅）', () => {
  const sources = collectSources(REPO);
  const d = judgeChannelShape(sources);
  assert.equal(d.status, PASS, `真实扫描面的 9d 必须通过：${d.detail}`);
  assert.ok(d.checks.length === 5, '9d 必须真的断五项，不能退化成空断言');
});

test('反向 · 注释里提到凭据键不得判红（判据只断代码形态）', () => {
  const code = [
    '// 不许读 localStorage.getItem("userToken")',
    '// 也不许手工拼 Authorization / Cookie 头',
    "const theme = localStorage.getItem('some_theme');",
  ].join('\n');
  const results = checkSource(code, 'fixture-comment');
  assert.equal(pick(results, '9a').status, PASS, '注释里的 Authorization 不该判红');
  assert.equal(pick(results, '9b').status, PASS, '注释里的 userToken 不该判红');
});

test('反向 · 页内同源 fetch(\'/api/v0/…\') 不该被 9c 判红（协议 §1 要求页内取数）', () => {
  const code = [
    "const res = await fetch('/api/v0/chat_session/fetch_page?lte_cursor.pinned=false');",
    'const body = await res.json();',
  ].join('\n');
  const results = checkSource(code, 'fixture-inpage');
  assert.equal(pick(results, '9c').status, PASS, '页内相对路径取数不得判红');
});

test('反向 · 页外 reqwest 打站点 API ⇒ 9c 红', () => {
  const code = [
    'let client = reqwest::Client::new();',
    'let resp = client.get("https://chat.deepseek.com/api/v0/chat_session/fetch_page").send().await?;',
  ].join('\n');
  const results = checkSource(code, 'fixture-offsite');
  const c = pick(results, '9c');
  assert.equal(c.status, FAIL, '页外调站点 API 必须红');
  assert.match(c.detail, /9c|页外/);
});

test('扫描面 · 夹具落 %TEMP%、跑完删自己建的', () => {
  const dir = tempDir('dsh-p4-credential-fixture-');
  try {
    const target = join(dir, 'src-tauri/src/official_chat');
    // 只建目录结构 + 一个带违规形态的文件
    mkdirSync(target, { recursive: true });
    writeFileSync(join(target, 'x.rs'), "const t = localStorage.getItem('userToken');\n", 'utf8');
    const sources = collectSources(dir, ['src-tauri/src/official_chat/**/*.rs']);
    assert.equal(sources.length, 1, '应只扫到夹具里的那一个文件');
    const results = checkSource(sources[0].code, sources[0].path);
    assert.equal(pick(results, '9b').status, FAIL);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('汇总 · summarize 的计数与 P1/P2/P3 同构', () => {
  const s = summarize([
    { status: PASS },
    { status: FAIL },
    { status: SKIPPED },
    { status: PASS },
  ]);
  assert.deepEqual(s, { total: 4, pass: 2, fail: 1, skipped: 1 });
});

test('真机冒烟 · 对真实扫描面跑一遍，结果自洽（红绿不硬断言）', () => {
  const sources = collectSources(REPO);
  assert.ok(Array.isArray(sources), '扫描面必须可枚举');
  for (const source of sources) {
    const results = checkSource(source.code, source.path);
    for (const r of results) {
      assert.ok(
        [PASS, FAIL, SKIPPED].includes(r.status),
        `判据状态只能是三态（${source.path} / ${r.name} ⇒ ${r.status}）`,
      );
    }
  }
  const hits = sources.flatMap((s) => scanSource(s.code, s.path));
  assert.ok(Array.isArray(hits));
});

test('9e：真实仓库读数全过（冷档案的凭据纪律）', () => {
  const { ok, checks } = judgeColdCredential(readColdCredentialReading(REPO));
  const bad = checks.filter((c) => c.status !== PASS);
  assert.equal(ok, true, bad.map((c) => `${c.name}：${c.detail}`).join('；'));
});

test('9e：反向自证 · 逐条「改坏一处」⇒ **对应那条**必须红', () => {
  const cases = [
    {
      what: '生产段里删了源档案（写用户档案 ⇒ 违 §7.3）',
      expect: '源档案只读',
      mutate: (r) => ({ ...r, removeCalls: [...r.removeCalls, '&source_webview'] }),
    },
    {
      what: '删副本之前没有「只删带自己标记的」守卫',
      expect: '用完即删',
      mutate: (r) => ({ ...r, markGuards: 0 }),
    },
    {
      what: '拷贝时解析了凭据（读了 userToken）',
      expect: '字节级复制',
      mutate: (r) => ({ ...r, credHits: ['userToken'] }),
    },
  ];
  for (const c of cases) {
    const { ok, checks } = judgeColdCredential(c.mutate(readColdCredentialReading(REPO)));
    assert.equal(ok, false, `改坏「${c.what}」后整条必须判红`);
    const hit = checks.find((x) => x.status === 'fail' && x.name.includes(c.expect));
    assert.ok(
      hit,
      `改坏「${c.what}」后必须由含「${c.expect}」的断言报红（实际：${checks
        .map((x) => `${x.name}=${x.status}`)
        .join('；')}）`,
    );
  }
});

test('9e：集成自证 · 影子目录里放一份**会删源档案**的冷档案 ⇒ 9e 必须红', () => {
  const cold = readFileSync(join(REPO, COLD_PROFILE_REL), 'utf8');
  // 在**生产段**里插一句「删掉源档案」
  const patched = cold.replace(
    'fn cleanup_stale(current: &Path) {',
    'fn cleanup_stale(current: &Path) {\n    let _ = std::fs::remove_dir_all(&source_webview);',
  );
  assert.notEqual(patched, cold, '改坏没生效 ⇒ 冷档案模块的形态变了（请同步本测试）');
  const dir = tempDir('dsh-p4-9e-shadow-');
  try {
    const abs = join(dir, COLD_PROFILE_REL);
    mkdirSync(join(abs, '..'), { recursive: true });
    writeFileSync(abs, patched, 'utf8');
    const { ok, checks } = judgeColdCredential(readColdCredentialReading(dir));
    assert.equal(ok, false, '会删源档案 ⇒ 9e 必须判红（不许假绿）');
    const hit = checks.find((x) => x.status === 'fail' && x.name.includes('源档案只读'));
    assert.ok(hit, `必须由「源档案只读」那条报红（实际：${checks.map((x) => x.name).join('；')}）`);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

// ── 按形态分（izzy 2026-10-23 上午拍板）────────────────────────────────────
//
// 形态 A｜壳侧 DevTools `Network` 域订阅（页内零动作）｜
// 形态 B｜页内自取数（路线 B：页内脚本自己取自己回，**一个**随机名槽位、取到即删）｜
// 形态 C｜页内旁听（替换 `XMLHttpRequest.prototype`，**取到即还原成替换前那个引用**）

const FORM_A_SCOPE_FILE = 'src-tauri/src/official_chat/session_structure.rs';
const FORM_B_SCOPE_FILE = 'src-tauri/src/official_chat/script_eval.rs';
const FORM_C_SCOPE_FILE = 'src-tauri/src/official_chat/page_hook_script.js';

/** 形态 A 的链：页内零动作 ⇒ 两个形状都不许出现。 */
const FORM_A_CLEAN = 'cdp_call(&window, "Network.enable", json!({}), CDP_TIMEOUT).await?;';
/** 形态 B 的链：回程**就是**那个随机名槽位。 */
const FORM_B_SLOT = 'let slot = format!("__dsh_rt_{:x}{:x}", nanos, n);';
/** 形态 C 的链：替换 `XMLHttpRequest.prototype`（**还原动作在真身里由同一个 hook 脚本带**）。 */
const FORM_C_HOOK = 'XMLHttpRequest.prototype.open = function () { return origOpen.apply(this, arguments); };';

test('形态 A · 两个形状都禁：hook 出现 ⇒ 判红', () => {
  const d = judgeInpageTraceShape(
    [{ path: FORM_A_SCOPE_FILE, code: `${FORM_A_CLEAN}\n${FORM_C_HOOK}` }],
    FORM_A,
  );
  assert.equal(d.ok, false, '形态 A 不许出现页内 hook');
  const hit = d.checks.find((c) => c.status === 'fail');
  assert.ok(hit, '必须由某条断言报红');
  assert.match(hit.name, /形态 A · 不存在页内 hook 形状/);
});

test('形态 A · 两个形状都禁：槽位出现 ⇒ 判红', () => {
  const d = judgeInpageTraceShape(
    [{ path: FORM_A_SCOPE_FILE, code: `${FORM_A_CLEAN}\n${FORM_B_SLOT}` }],
    FORM_A,
  );
  assert.equal(d.ok, false, '形态 A 不许出现页内槽位');
  const hit = d.checks.find((c) => c.status === 'fail');
  assert.ok(hit, '必须由某条断言报红');
  assert.match(hit.name, /形态 A · 不存在页内 R1 槽位形状/);
});

test('形态 B · 允许槽位回程、但禁页内 hook', () => {
  const ok = judgeInpageTraceShape([{ path: FORM_B_SCOPE_FILE, code: FORM_B_SLOT }], FORM_B);
  assert.equal(ok.ok, true, `形态 B 的槽位回程必须被允许：${ok.checks.map((c) => `${c.name}=${c.status}`).join('；')}`);
  assert.ok(
    ok.checks.some((c) => c.name.includes('形态 B · 有槽位回程通道') && c.status === 'pass'),
    '必须点名「这条链用了槽位回程」',
  );

  // 反向：形态 B 的链里出现页内 hook ⇒ 红
  const bad = judgeInpageTraceShape(
    [{ path: FORM_B_SCOPE_FILE, code: `${FORM_B_SLOT}\n${FORM_C_HOOK}` }],
    FORM_B,
  );
  assert.equal(bad.ok, false, '形态 B 不许替换 `XMLHttpRequest.prototype`');
  const hit = bad.checks.find((c) => c.status === 'fail');
  assert.ok(hit, '必须由某条断言报红');
  assert.match(hit.name, /形态 B · 不存在页内 hook 形状/);
});

test('形态 C · 允许页内 hook、但禁槽位', () => {
  const ok = judgeInpageTraceShape([{ path: FORM_C_SCOPE_FILE, code: FORM_C_HOOK }], FORM_C);
  assert.equal(ok.ok, true, `形态 C 的页内 hook 必须被允许：${ok.checks.map((c) => `${c.name}=${c.status}`).join('；')}`);
  assert.ok(
    ok.checks.some((c) => c.name.includes('形态 C · 有页内 hook 形状') && c.status === 'pass'),
    '必须点名「这条链用了页内 hook」',
  );

  // 反向：形态 C 的链里出现槽位 ⇒ 红
  const bad = judgeInpageTraceShape(
    [{ path: FORM_C_SCOPE_FILE, code: `${FORM_C_HOOK}\n${FORM_B_SLOT}` }],
    FORM_C,
  );
  assert.equal(bad.ok, false, '形态 C 不许留 `__dsh_rt_*` 槽位');
  const hit = bad.checks.find((c) => c.status === 'fail');
  assert.ok(hit, '必须由某条断言报红');
  assert.match(hit.name, /形态 C · 不存在页内 R1 槽位形状/);
});

test('每种形态都有一条「另一形状出现就红」的断言（不许两个形状都放开）', () => {
  const expect = {
    a: [/形态 A · 不存在页内 hook 形状/, /形态 A · 不存在页内 R1 槽位形状/],
    b: [/形态 B · 不存在页内 hook 形状/, /形态 B · 有槽位回程通道/],
    c: [/形态 C · 不存在页内 R1 槽位形状/, /形态 C · 有页内 hook 形状/],
  };
  for (const [form, patterns] of Object.entries(expect)) {
    const scopeFile =
      form === 'a' ? FORM_A_SCOPE_FILE : form === 'b' ? FORM_B_SCOPE_FILE : FORM_C_SCOPE_FILE;
    const d = judgeInpageTraceShape(
      [{ path: scopeFile, code: `${FORM_A_CLEAN}\n${FORM_B_SLOT}\n${FORM_C_HOOK}` }],
      form,
    );
    for (const p of patterns) {
      assert.ok(
        d.checks.some((c) => p.test(c.name)),
        `形态 ${form} 必须有断言 ${p}（实际：${d.checks.map((c) => c.name).join('；')}）`,
      );
    }
  }
});

test('真机冒烟 · 真实扫描面上：形态 A／B 全绿、形态 C **不许红**（链没落地 ⇒ 未执行；落地了 ⇒ 必须全过）', () => {
  const sources = collectSources(REPO, TRACE_SHAPE_SCAN_GLOBS);
  const a = judgeInpageTraceShape(sources, FORM_A);
  assert.equal(a.ok, true, `形态 A 必须全绿：${a.checks.map((c) => `${c.name}=${c.status}`).join('；')}`);
  const b = judgeInpageTraceShape(sources, FORM_B);
  assert.equal(b.ok, true, `形态 B 必须全绿：${b.checks.map((c) => `${c.name}=${c.status}`).join('；')}`);
  const c = judgeInpageTraceShape(sources, FORM_C);
  // ⚠️ 形态 C 的链**落地与否**取决于实现进度 ⇒ ⇒ 两条路都算过：
  //   ① 还没落地 ⇒ 报「未执行」（**不假绿**）；② 落地了 ⇒ 必须全过（**不许红**）。
  const allSkipped = c.checks.every((x) => x.status === SKIPPED);
  assert.ok(
    allSkipped || c.ok,
    `形态 C 不许红（实际：${c.checks.map((x) => `${x.name}=${x.status}`).join('；')}）`,
  );
});

test('事实可核 · 形态 A 的取数链**不用**那个槽位（作用域里搜不到 `__dsh_rt_`）', () => {
  const sources = collectSources(REPO, TRACE_SHAPE_SCAN_GLOBS);
  const scope = sources.filter((s) => FORM_TRACE_SCOPE.a.includes(s.path));
  assert.ok(scope.length > 0, '形态 A 的作用域必须非空');
  for (const s of scope) {
    assert.ok(
      !s.code.includes('__dsh_rt_'),
      `形态 A 的取数链不用那个槽位 ⇒ ${s.path} 里不该出现 \`__dsh_rt_\``,
    );
  }
  // ⚠️ **这条事实按形态分之后仍然成立** —— 形态 B 的链**用**它，但那是**另一条链**。
  const bScope = sources.filter((s) => FORM_TRACE_SCOPE.b.includes(s.path));
  assert.ok(
    bScope.some((s) => s.code.includes('__dsh_rt_')),
    '形态 B 的链**用**那个槽位 ⇒ 两依据不许互相顶替',
  );
});

// ── 形态归属：按 **cfg 段** 切分（PM 2026-10-23 拍板）────────────────────────
//
// PM 的裁定：**形态归属按 `cfg` 块判**（`#[cfg(windows)]` ⇒ 形态 A、
// `#[cfg(target_os = "macos")]` ⇒ 形态 B）⇒ ⇒ **公共段归属不明 ⇒ 按形态 A 判**
// （**故意的严格**：有人把形态 B 的代码挪进公共段 ⇒ 判据要响）。

/** 形态 A 自己的链文件（真身）。 */
const REAL_CHAIN_FILE = 'src-tauri/src/official_chat/session_structure.rs';

test('形态归属 · 真实仓库：形态 A 的作用域里**没有**形态 B 的 macOS 段（反之亦然）', () => {
  const sources = collectSources(REPO, TRACE_SHAPE_SCAN_GLOBS);
  const { byForm } = attributeForms(sources);
  const a = byForm.a.map((s) => s.code).join('\n');
  const b = byForm.b.map((s) => s.code).join('\n');
  // 形态 B 的两个标志函数（`page_ready_script`／`capture_via_page`）**只在** macOS 段里。
  assert.match(b, /fn page_ready_script/, '形态 B 必须看得见 `page_ready_script`');
  assert.match(b, /fn capture_via_page/, '形态 B 必须看得见 `capture_via_page`');
  assert.doesNotMatch(a, /fn capture_via_page/, '形态 B 的 macOS 段**不许**落进形态 A');
  // 形态 A 的标志函数（壳侧 `Network` 域订阅）**只在** windows 段里。
  assert.match(a, /fn subscribe_network_events/, '形态 A 必须看得见 `subscribe_network_events`');
  assert.doesNotMatch(b, /fn subscribe_network_events/, '形态 A 的 windows 段**不许**落进形态 B');
});

test('形态归属 · 归属不明 ⇒ **按形态 A 判**（fail-safe：无主文件的公共段进形态 A）', () => {
  const sources = collectSources(REPO, TRACE_SHAPE_SCAN_GLOBS);
  const { byForm, unattributed } = attributeForms(sources);
  assert.ok(unattributed.length > 0, '真实仓库里必须有「归属不明」的段（否则 fail-safe 没被验到）');
  for (const seg of unattributed) {
    assert.ok(
      byForm.a.some((s) => s.path === seg.path && s.line === seg.line),
      `归属不明的段必须落进形态 A：${seg.path}:${seg.line}`,
    );
  }
});

test('形态归属 · 槽位机制在 `script_eval.rs` 的**公共段**里 ⇒ 归形态 B（形态 A 看不见它）', () => {
  const sources = collectSources(REPO, TRACE_SHAPE_SCAN_GLOBS);
  const { byForm } = attributeForms(sources);
  const b = byForm.b.map((s) => s.code).join('\n');
  const a = byForm.a.map((s) => s.code).join('\n');
  assert.match(b, /__dsh_rt_/, '形态 B 必须看得见槽位机制（它在公共段里）');
  assert.doesNotMatch(a, /__dsh_rt_/, '形态 A 看不见形态 B 的槽位机制（它归形态 B）');
});

test('形态归属 · **反向自证**：把形态 B 的 macOS 段摘掉 cfg（⇒ 变成公共段）⇒ 形态 A 必须判红（fail-safe 真的在响）', () => {
  const rel = REAL_CHAIN_FILE;
  const original = readFileSync(join(REPO, rel), 'utf8');
  const before = createHash('sha256').update(original).digest('hex');

  // ① 真身：形态 B 的那段带 `cfg` ⇒ 形态 A **不**判红（这一格此前是误红）。
  const real = judgeChannelShape(collectSources(REPO));
  assert.equal(real.status, PASS, `真身必须通过：${real.detail}`);

  // ② 改坏：**摘掉那段测试模块的 cfg** ⇒ 那段变成**公共段** ⇒ 按形态 A 判（fail-safe）
  //    ⇒ 形态 A 的「零页内请求」必须判红（证明这一格不是「不看」，而是「按形态分开看」）。
  const marker = '#[cfg(all(test, target_os = "macos"))]';
  const mutated = original.replace(marker, '// 改坏：cfg 被摘掉了（这一段变成公共段）');
  assert.notEqual(mutated, original, '改坏没生效 ⇒ 锚点不存在（请同步本测试）');
  const bad = judgeChannelShape([{ path: rel, code: mutated }]);
  assert.equal(bad.status, FAIL, '公共段按形态 A 判 ⇒ 形态 A 的「零页内请求」必须判红');
  assert.match(bad.detail, /页内发请求\/取数形态/, '必须点名是「页内取数形态」那一格');

  // ③ 恢复后**逐字节一致**（原件从头到尾没被写过）。
  assert.equal(
    createHash('sha256').update(readFileSync(join(REPO, rel), 'utf8')).digest('hex'),
    before,
    '原件必须逐字节不变',
  );
  assert.equal(judgeChannelShape(collectSources(REPO)).status, PASS, '恢复后必须重新变绿');
});

test('形态 A 的八条断言 · **基线 sha256 复核**（跨文件第二道钉子：名单与判据同时被改也会红）', () => {
  const { checks } = judgeTrace({ before: fakeSnapshot(), after: fakeSnapshot() }, TRACE_FORM_A);
  const names = checks.map((c) => c.name).filter((n) => n !== FORM_A_ONLY_CHECK_NAME);
  const json = JSON.stringify(names, null, 2);
  assert.equal(
    createHash('sha256').update(json).digest('hex'),
    FORM_A_BASELINE_SHA256_MIRROR,
    '形态 A 的八条断言（剔掉形态专属那一条后）必须与基线 sha256 一致 —— 名单与判据都不许动',
  );
});

// ── P4-9a／9b：**按形态归属过滤**（PM 2026-10-23 裁定）─────────────────────────
//
// · **形态 A**（壳侧 `Network` 域订阅）＝ 壳侧零请求／不读凭据／不自拼头 ⇒ ⇒ **9a／9b 断它**；
// · **形态 B**（页内自取数）**合法**在页内拼 `authorization` 头（协议 §7.2 v0.28 补条：
//   「凭据全程留在页内」）⇒ ⇒ **形态 B 名下不判这两条**；
// · **形态 C**（页内旁听）靠截到的签名头 ⇒ ⇒ 同样不判；
// · **测试段**（`cfg(test)`／`cfg(all(test, …))`）**一律不参与**。

/** 往源码末尾追加一段（`#[cfg(…)]` 门控）—— 用它造「形态 A 段」与「形态 B 段」。 */
function appendSegment(code, gate, body) {
  return `${code}\n${gate}\n${body}\n`;
}

/** **自拼凭据头**的注入体（`headers` 集合里出现凭据头名 ⇒ 9a 的「拼头动作」形态）。 */
const INJECT_SELF_HEADER = 'fn __dsh_probe() { let headers = { authorization: "Bearer x" }; }';
/** **读站点凭据**的注入体（读 `document.cookie` ⇒ 9b 的形态）。 */
const INJECT_READ_COOKIE = 'fn __dsh_probe() { let c = document.cookie; }';

/**
 * 用**真实仓库的扫描面**判一次（形态 A 的段从真实文件里来）。
 *
 * @param {string} code - 注入后的 `session_structure.rs` 文本。
 * @returns {Array<{name: string, status: string, detail: string}>}
 */
function judgeCredentialOn(code) {
  const source = { path: REAL_CHAIN_FILE, code };
  return judgeCredentialSurface([source], [source]);
}

test('P4-9a／9b · 真实仓库：形态 A 名下**有覆盖**（扫到东西）且**判绿**', () => {
  const verdicts = judgeCredentialSurface(
    collectSources(REPO),
    collectSources(REPO, TRACE_SHAPE_SCAN_GLOBS),
  );
  const a = pick(verdicts, '9a');
  const b = pick(verdicts, '9b');
  assert.equal(a.status, PASS, `形态 A 名下的 9a 必须绿：${a.detail}`);
  assert.equal(b.status, PASS, `形态 A 名下的 9b 必须绿：${b.detail}`);
  // ⚠️ **覆盖口径**：扫到的段数 **> 0** 即可（**不写死条数**）⇒ 「扫到了东西、判它合规」。
  const m = /只判形态 A 名下\*\*的 (\d+) 段/.exec(a.detail);
  assert.ok(m, `9a 的文案必须报出形态 A 的覆盖段数：${a.detail}`);
  assert.ok(Number(m[1]) > 0, '形态 A 名下必须有覆盖（扫到的段数 > 0）');
});

test('P4-9a／9b · **覆盖守卫**：形态 A 的作用域扫不出段 ⇒ 那两条必须判红（不许「没东西所以过」）', () => {
  const verdicts = judgeCredentialSurface([], []);
  const a = pick(verdicts, '9a');
  const b = pick(verdicts, '9b');
  assert.equal(a.status, FAIL, '形态 A 段为 0 ⇒ 9a 必须判红（不许因为「没东西」就判过）');
  assert.equal(b.status, FAIL, '形态 A 段为 0 ⇒ 9b 必须判红（不许因为「没东西」就判过）');
  assert.match(a.detail, /敏感度/, '必须说清是「失去敏感度」');
});

test('P4-9a／9b · **反向自证**：形态 A 段塞 ⇒ 红；形态 B 段塞 ⇒ 仍绿（**原件逐字节不变**）', () => {
  const rel = REAL_CHAIN_FILE;
  const original = readFileSync(join(REPO, rel), 'utf8');
  const before = createHash('sha256').update(original).digest('hex');

  // ① 形态 A 段（`#[cfg(windows)]`）塞自拼凭据头 ⇒ **9a 必须红**。
  const aSeg = appendSegment(original, '#[cfg(windows)]', INJECT_SELF_HEADER);
  const aVerdict = pick(judgeCredentialOn(aSeg), '9a');
  assert.equal(aVerdict.status, FAIL, '形态 A 段里自拼凭据头 ⇒ 9a 必须判红');
  assert.match(aVerdict.detail, /authorization/, '必须点名是哪个凭据头');

  // ② 形态 A 段里读 `document.cookie` ⇒ **9b 必须红**。
  const bSeg = appendSegment(original, '#[cfg(windows)]', INJECT_READ_COOKIE);
  const bVerdict = pick(judgeCredentialOn(bSeg), '9b');
  assert.equal(bVerdict.status, FAIL, '形态 A 段里读站点凭据 ⇒ 9b 必须判红');
  assert.match(bVerdict.detail, /document\.cookie/, '必须点名是读 `document.cookie`');

  // ③ **形态 B 段**（`#[cfg(target_os = "macos")]`）塞**同样的东西** ⇒ **必须仍然绿**
  //    （协议 §7.2 v0.28 补条：形态 B 的凭据全程留在页内 ⇒ 壳侧不判 9a／9b）。
  const macSeg = appendSegment(original, '#[cfg(target_os = "macos")]', INJECT_SELF_HEADER);
  assert.equal(
    pick(judgeCredentialOn(macSeg), '9a').status,
    PASS,
    '形态 B 名下不判 9a（页内拼头是形态 B 的合法做法）',
  );
  const macCookie = appendSegment(original, '#[cfg(target_os = "macos")]', INJECT_READ_COOKIE);
  assert.equal(
    pick(judgeCredentialOn(macCookie), '9b').status,
    PASS,
    '形态 B 名下不判 9b（凭据不出页面）',
  );

  // ④ **测试段**塞同样的东西 ⇒ **仍然绿**（测试段不是产品路径）。
  const testSeg = appendSegment(original, '#[cfg(test)]', INJECT_SELF_HEADER);
  assert.equal(
    pick(judgeCredentialOn(testSeg), '9a').status,
    PASS,
    '测试段不参与判定（`cfg(test)` 不属任何形态）',
  );

  // ⑤ 恢复后**逐字节一致**（原件从头到尾没被写过）。
  assert.equal(
    createHash('sha256').update(readFileSync(join(REPO, rel), 'utf8')).digest('hex'),
    before,
    '原件必须逐字节不变',
  );
});

test('P4-9a · **真拼头的各种形态一个都不许漏**（收窄语境词不许把真动作放过）', () => {
  // ⚠️ **只列历史上就被抓的形状**（收窄语境词**不许**把任何一个漏掉）⇒ ⇒
  //   `header("authorization", …)`（单数**方法**）是**收窄后新增**的（旧的 `\.header` 漏它）。
  const shapes = [
    'let headers = { authorization: "Bearer x" };',
    'headers.insert("authorization", token);',
    'setRequestHeader("authorization", token);',
    'header("authorization", token);',
    'set("authorization", token);',
    'append("authorization", token);',
  ];
  for (const shape of shapes) {
    const code = appendSegment('const X: u32 = 0;', '#[cfg(windows)]', `fn __p() { ${shape} }`);
    const verdict = pick(judgeCredentialOn(code), '9a');
    assert.equal(verdict.status, FAIL, `这一种拼头形态必须被抓红：${shape}`);
  }
});

test('P4-9a · **规格字段不算「壳拼了凭据」**（`header: "authorization"` 是字面量，不是拼头动作）', () => {
  const code = appendSegment(
    'const X: u32 = 0;',
    '#[cfg(target_os = "macos")]',
    'fn __p() { let spec = Spec { header: "authorization", prefix: "Bearer " }; }',
  );
  const verdict = pick(judgeCredentialOn(code), '9a');
  assert.equal(
    verdict.status,
    PASS,
    '规格字段（声明页内该拼哪个头）不是「壳侧拼了凭据」⇒ 不该判红',
  );
});
