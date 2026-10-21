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
// · **9d 恒为 `skipped`**（占位）：协议 §3.1 v0.7 未定案 ⇒ 判据**不自己发明通道形态**。
// · **真机冒烟**：对真实扫描面跑一遍，结果**自洽**（红绿**不硬断言** —— 实现会演进）。
//
// ## 纪律
//
// · 夹具落 `%TEMP%` 一次性目录，**跑完删自己建的**；**永不碰用户的真实数据目录**。
// · 判据**不发任何网络请求**、不碰凭据值。
//
// @module scripts/check-official-chat-p4-credential-test

import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

import {
  FAIL,
  PASS,
  SKIPPED,
  checkSource,
  collectSources,
  scanSource,
  summarize,
} from './check-official-chat-p4-credential.mjs';

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

test('用例 4 · 干净夹具 ⇒ 9a/9b/9c 全绿（9d 恒 skipped）', () => {
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
  const d = pick(results, '9d');
  assert.equal(d.status, SKIPPED, '9d 是占位，必须报 skipped 而不是 pass/fail');
  assert.match(d.detail, /占位/);
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
