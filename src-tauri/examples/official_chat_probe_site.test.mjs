// 站点探测脚本的**行为**判据：Q1 / Q2 修完没有，靠这里钉住。
//
// ## 为什么不写「源码字符串断言」
//
// 计划 §4 明写「不只做源码字符串断言」。这两条判据尤其不能只靠正则：
//
//  · **Q1**：原来那句「第一个 /token/i 匹配的键」——正则是绿的，行为照样错
//    （真实 profile 命中 `userToken`，全新 profile 命中腾讯分析缓存键）。
//    只有把脚本**跑起来**、把两个键都摆进去，才知道它到底选了谁。
//  · **Q2**：`tokenUsable = Boolean(token)` 只判「值非空」。同样要跑起来、
//    让服务端回 200 + 空 items，才看得出「状态码 200」被当成了成功。
//
// ## 怎么跑
//
// 页面脚本现在还嵌在 Rust 源文件的 `format!` 里（花括号全部双写）。这里从
// 源文件里**原样取出**那段 raw string，按 `format!` 的规则还原（`{{`→`{`），
// 换掉端点占位符，然后在 `node:vm` 里造一个假页面跑它。
//
// 取不到定界符时判据自己先红——结构变了就得同步改这里，不能悄悄跳过。
//
// @module official-chat/probe-site-test

import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { createContext, runInContext } from 'node:vm';

const RUST_SOURCE = readFileSync(new URL('./official_chat_probe.rs', import.meta.url), 'utf8');

/** 从 Rust 源里取出页面脚本并还原成可执行 JS。 */
function extractSiteScript() {
  const open = 'r#"(async function () {{';
  const close = '}})()"#';
  const start = RUST_SOURCE.indexOf(open);
  const end = RUST_SOURCE.indexOf(close, start);
  assert.ok(start >= 0, '页面脚本的起始定界符不见了——判据自己先红（结构变了就同步改这里）');
  assert.ok(end > start, '页面脚本的结束定界符不见了——判据自己先红');
  let script = RUST_SOURCE.slice(start + 'r#"'.length, end + close.length - '"#'.length);
  // format! 的花括号还原：源码里 `{{` 表示一个字面 `{`。
  script = script.replace(/\{\{/g, '{').replace(/\}\}/g, '}');
  script = script.replace('{endpoint}', JSON.stringify('/api/v0/chat_session/fetch_page?count=5'));
  assert.ok(!/\{endpoint\}/.test(script), '端点占位符没被换掉——脚本跑的不是它');
  return script;
}

const SITE_SCRIPT = extractSiteScript();

/** 造一个假页面并跑脚本。`storage` 是 localStorage 内容，`respond` 决定 fetch 回什么。 */
async function runSiteScript({ storage = {}, respond }) {
  const requests = [];
  const sandbox = {
    console: { log() {}, warn() {}, error() {} },
    JSON, Object, Array, String, Number, Boolean, Error, Math, Date, RegExp, Promise,
    localStorage: {
      getItem: (key) => (Object.prototype.hasOwnProperty.call(storage, key) ? storage[key] : null),
    },
    sessionStorage: { getItem: () => null },
    // `node:vm` 的沙箱**不继承** Node 的定时器全局：页面脚本里只要出现
    // `await new Promise((resolve) => setTimeout(resolve, 200))`，就会撞上
    // `ReferenceError: setTimeout is not defined`——判据自己先红，而红的
    // 原因是沙箱缺东西，不是被测行为变了。这里把两个定时器注进去。
    setTimeout: (fn, ms) => setTimeout(fn, ms),
    clearTimeout: (id) => clearTimeout(id),
    location: { origin: 'https://chat.deepseek.com', href: 'https://chat.deepseek.com/chat' },
    document: { cookie: '' },
    fetch: async (url, options) => {
      requests.push({ url, headers: options?.headers ?? {} });
      const body = respond(requests.length);
      return {
        status: body.status,
        ok: body.status >= 200 && body.status < 300,
        headers: { get: (name) => (name === 'content-type' ? 'application/json' : null) },
        text: async () => body.text,
      };
    },
  };
  sandbox.globalThis = sandbox;
  createContext(sandbox);
  const out = await runInContext(SITE_SCRIPT, sandbox);
  return { out, requests };
}

test('Q1：token 按显式优先级取，不按枚举顺序（两个键都摆进去也只认优先级表里的那个）', async () => {
  // 这两个键**同时**在存储里：按枚举顺序取会命中腾讯分析的缓存键
  // `__tea_cache_tokens_20006317`，按优先级取才命中 `userToken`。
  const { out } = await runSiteScript({
    storage: {
      __tea_cache_tokens_20006317: 'analytics-cache-value',
      userToken: 'real-user-token',
    },
    respond: () => ({ status: 200, text: JSON.stringify({ code: 0, data: { biz_data: { chat_sessions: [{ id: 's1' }] } } }) }),
  });

  assert.equal(out.tokenKeyTried, 'userToken', '必须取优先级表里的第一个可用键');
  assert.ok(
    Array.isArray(out.tokenKeysTried) && out.tokenKeysTried.length > 0,
    '必须输出实际试过的键名，否则读结果的人不知道这次测的是哪一个',
  );
  assert.ok(!out.tokenKeysTried.includes('__tea_cache_tokens_20006317'), '不该去试分析缓存键');
  assert.equal(out.tokenKeyFound, undefined, '旧字段名 tokenKeyFound 必须已经删掉');
  assert.equal(out.tokenUsable, undefined, '旧字段名 tokenUsable 必须已经删掉');
});

test('Q1：优先级表里的键缺失时，才轮到下一个；全都不在时如实说没有', async () => {
  const { out } = await runSiteScript({
    storage: { accessToken: 'a-token' },
    respond: () => ({ status: 200, text: JSON.stringify({ code: 0, data: { biz_data: { chat_sessions: [{ id: 's1' }] } } }) }),
  });
  assert.equal(out.tokenKeyTried, 'accessToken', 'userToken 不在时应当顺延到 accessToken');
  assert.equal(out.tokenPresent, true);

  const empty = await runSiteScript({
    storage: { unrelated: 'x' },
    respond: () => ({ status: 200, text: '{}' }),
  });
  assert.equal(empty.out.tokenKeyTried, null, '一个都没有时不得编一个键名');
  assert.equal(empty.out.tokenPresent, false, '一个都没有时不得说凭据存在');
  assert.equal(empty.out.tokenAccepted, false);
});

test('Q2：「键在、值非空」不等于「服务端接受」（DeepSeek 40002 的那一路）', async () => {
  const { out } = await runSiteScript({
    storage: { userToken: 'real-user-token' },
    respond: () => ({ status: 200, text: JSON.stringify({ code: 40002, msg: 'Missing Token', data: null }) }),
  });

  assert.equal(out.tokenPresent, true, '凭据确实存在');
  assert.equal(out.tokenAccepted, false, '服务端回 40002 → 不算接受');
  assert.ok(out.tokenAcceptEvidence, '必须留下判定的依据（状态码 / 业务 code）');
  assert.equal(out.tokenAcceptEvidence.code, 40002);
});

test('Q2：状态码 200 不等于成功（ChatGPT 那种「200 但 items 为空」）', async () => {
  const { out } = await runSiteScript({
    storage: { userToken: 'real-user-token' },
    respond: () => ({ status: 200, text: JSON.stringify({ items: [], total: 0, limit: 5, offset: 0 }) }),
  });

  assert.equal(out.tokenPresent, true);
  assert.equal(out.tokenAccepted, false, '200 + 空列表 = 未登录，不能算接受');
});

test('Q2：真的被接受时才算接受（业务 code 成功且响应里有内容）', async () => {
  const { out } = await runSiteScript({
    storage: { userToken: 'real-user-token' },
    respond: () => ({ status: 200, text: JSON.stringify({ code: 0, data: { biz_data: { chat_sessions: [{ id: 's1', title: 't' }] } } }) }),
  });

  assert.equal(out.tokenPresent, true);
  assert.equal(out.tokenAccepted, true, 'code 0 + 有内容 → 接受');
});

test('Q1/Q2：裸 fetch 与带凭据的 fetch 都要留下记录（每站请求次数可核对）', async () => {
  const { out, requests } = await runSiteScript({
    storage: { userToken: 'real-user-token' },
    respond: () => ({ status: 200, text: JSON.stringify({ code: 0, data: { biz_data: { chat_sessions: [{ id: 's1' }] } } }) }),
  });

  // 三次，顺序是：裸 fetch → **回环 CSP 探测** → 带 Bearer。
  // 回环那次验的是「站点 CSP 放不放行页内 fetch 打本机回环」（调研 §7 #4
  // 与 R2 路线的前提），与站点取数无关，因此**不计入** `pageRequests`——
  // 但它确实发出了一次请求，所以「沙箱里总共发了几次」与「打了站点几次」
  // 是两个数，别混成一条断言。
  assert.equal(requests.length, 3, '三次：裸的、回环 CSP 探测、带 Bearer');
  assert.equal(requests[0].headers.authorization, undefined, '第一次是裸 fetch');
  assert.match(requests[1].url, /127\.0\.0\.1:9\/loopback-probe/, '第二次是回环 CSP 探测');
  assert.match(requests[2].headers.authorization, /^Bearer /, '第三次带 Bearer');
  assert.equal(out.pageRequests, 2, '只有打站点的那两次计入 pageRequests');
  assert.ok(Array.isArray(out.attempts) && out.attempts.length === 2);
});
