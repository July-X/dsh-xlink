// P4 真实取数源（`host/site-source.js`）与 `sessionStructure` 命令的判据。
//
// ## 判什么
//
// · **桥客户端**：只发一个命令名（**不接受任意 JS** —— 协议 §5.2 第 3 条）、
//   失败不抛、错误码**留在闭集内**（协议 §6.1）；
// · **回包形状**：结构字段少一个 ⇒ `schema_changed`（协议 §4.3）；
// · **命令层**：真实源失败 ⇒ 稳定码 + 脱敏 detail；站内码**原样上报**
//   （协议 §6.2）；真实源没接上 ⇒ `source_unsupported`（**不静默降级**）。
//
// ## 判据不替真机跑
//
// 这里用**注入的传输桩**（`transport`），证不了「真机上那条桥路由真的在」——
// 那是壳那一侧的事（`src-tauri/src/plugins/builtin/bridge.rs`）。

import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';

import { createCatalogCache, createCommands, createFixtureSource } from '../host/commands.js';
import { createSeqAllocator } from '../host/seq-store.js';
import { createDispatcher } from '../host/rpc.js';
import {
  COMMAND,
  ROUTE_PATH,
  buildBodyRequestBody,
  buildRequestBody,
  createSiteStructureSource,
  fetchSessionBody,
  fetchSessionStructure,
  readCredentials,
  validateStructure,
} from '../host/site-source.js';

/** 一份**成功档**的结构（协议 §4.3 的字段面 ＋ §2.4 的白名单**逐字列举**那几项）。 */
function okStructure() {
  return {
    topKeys: ['code', 'data', 'msg'],
    code: 0,
    msg: '',
    dataKeys: ['biz_code', 'biz_data', 'biz_msg'],
    sessionsLength: 100,
    firstItemKeys: ['id', 'model_type', 'pinned', 'title', 'title_type', 'updated_at'],
    itemTypes: {
      id: 'string',
      model_type: 'string',
      pinned: 'boolean',
      title: 'string',
      title_type: 'string',
      updated_at: 'number',
    },
    hasMore: true,
    // **目录项面**（协议 §2.4 面二 / §4.3 ②）：每项**恰好 6 个键**，类型照 §4.3 ②
    // ——`pinned` 是 boolean、`updated_at` 是 number（**不是** string）。
    items: [
      {
        id: 'session-1',
        title: '会话 1',
        title_type: 'USER',
        model_type: 'deepseek-chat',
        pinned: false,
        updated_at: 1783051054.518,
      },
    ],
  };
}

/** 造一个**传输桩**：回固定的一包。 */
function stubTransport(payload, { status = 200 } = {}) {
  const seen = [];
  const transport = async (options) => {
    seen.push(options);
    return { status, body: typeof payload === 'string' ? payload : JSON.stringify(payload) };
  };
  transport.seen = seen;
  return transport;
}

test('桥凭据：两样缺一就当作没有桥', () => {
  assert.equal(readCredentials({}), null);
  assert.equal(readCredentials({ DSH_XLINK_BUILTIN_BRIDGE_URL: 'http://127.0.0.1:1' }), null);
  assert.equal(readCredentials({ DSH_XLINK_BUILTIN_BRIDGE_TOKEN: 't' }), null);
  assert.deepEqual(
    readCredentials({
      DSH_XLINK_BUILTIN_BRIDGE_URL: 'http://127.0.0.1:1',
      DSH_XLINK_BUILTIN_BRIDGE_TOKEN: 't',
    }),
    { url: 'http://127.0.0.1:1', token: 't' },
  );
});

test('请求体**只有一个命令名**（不许内核传任意 JS）', () => {
  assert.equal(COMMAND, 'sessionStructure');
  assert.equal(buildRequestBody(), '{"command":"sessionStructure"}');
  // 请求体里不许出现任何「像脚本」的东西。
  const body = buildRequestBody();
  assert.ok(!/function|=>|eval|XMLHttpRequest/i.test(body), `请求体里混进了脚本：${body}`);
});

test('`refresh` 只在**显式为真**时多带那一个布尔（协议 §4.6 修法 2）', async () => {
  // 缺省形态**一个字没变**。
  assert.equal(buildRequestBody(), '{"command":"sessionStructure"}');
  assert.equal(buildRequestBody(COMMAND, false), '{"command":"sessionStructure"}');
  // 显式为真 ⇒ 多带 `refresh:true`。
  assert.equal(
    buildRequestBody(COMMAND, true),
    '{"command":"sessionStructure","refresh":true}',
  );
  // **别的值不进请求体**（能力面不扩）。
  assert.equal(buildRequestBody(COMMAND, 'true'), '{"command":"sessionStructure"}');
  assert.equal(buildRequestBody(COMMAND, 1), '{"command":"sessionStructure"}');
});

test('`fetchSessionStructure` 把 `refresh` 透传进请求体', async () => {
  const structure = okStructure();
  const transport = stubTransport({ ok: true, structure });
  await fetchSessionStructure({
    credentials: { url: 'http://127.0.0.1:1', token: 't' },
    transport,
    refresh: true,
  });
  assert.equal(transport.seen.length, 1);
  assert.equal(transport.seen[0].body, '{"command":"sessionStructure","refresh":true}');
});

test('**站点短码**随请求体给壳侧（PM 2026-10-22 13:4x 裁：请求体加一个 `site` 键）', () => {
  // ⚠️ **缺省形态一个字没变**（既有调用方不带这个键 ⇒ 壳侧按默认站走）。
  assert.equal(buildRequestBody(), '{"command":"sessionStructure"}');
  // **认得出的短码** ⇒ ⇒ 多带那一个键。
  assert.equal(
    buildRequestBody(COMMAND, false, 'chatgpt'),
    '{"command":"sessionStructure","site":"chatgpt"}',
  );
  // **与 `refresh` 并存**（键序：命令名 → `refresh` → `site`）。
  assert.equal(
    buildRequestBody(COMMAND, true, 'minimax'),
    '{"command":"sessionStructure","refresh":true,"site":"minimax"}',
  );
  // ⚠️ **认不出的短码不带**（**不猜**）—— 壳侧拿到没登记的短码会明确报错。
  assert.equal(buildRequestBody(COMMAND, false, '别的站'), '{"command":"sessionStructure"}');
  assert.equal(buildRequestBody(COMMAND, false, ''), '{"command":"sessionStructure"}');
  assert.equal(buildRequestBody(COMMAND, false, 'ChatGPT'), '{"command":"sessionStructure"}');
});

test('正文那条的请求体**同样带站点短码**（缺省形态不变）', () => {
  assert.equal(buildBodyRequestBody('s-1'), '{"command":"sessionBody","sessionId":"s-1"}');
  assert.equal(
    buildBodyRequestBody('s-1', 'chatgpt'),
    '{"command":"sessionBody","sessionId":"s-1","site":"chatgpt"}',
  );
  // 认不出 ⇒ 不带。
  assert.equal(buildBodyRequestBody('s-1', '??'), '{"command":"sessionBody","sessionId":"s-1"}');
});

test('`fetchSessionStructure` / `fetchSessionBody` 把**站点短码**透传进请求体', async () => {
  const transport = stubTransport({ ok: true, structure: okStructure() });
  await fetchSessionStructure({
    credentials: { url: 'http://127.0.0.1:1', token: 't' },
    transport,
    site: 'chatgpt',
  });
  assert.equal(
    transport.seen[0].body,
    '{"command":"sessionStructure","site":"chatgpt"}',
    '目录那条必须带上站点短码',
  );

  const bodyTransport = stubTransport({ ok: true, messages: [] });
  await fetchSessionBody({
    credentials: { url: 'http://127.0.0.1:1', token: 't' },
    transport: bodyTransport,
    sessionId: 's-1',
    site: 'chatgpt',
  });
  assert.equal(
    bodyTransport.seen[0].body,
    '{"command":"sessionBody","sessionId":"s-1","site":"chatgpt"}',
    '正文那条必须带上站点短码',
  );
});

test('成功档：回包原样带回，**不做任何解释**', async () => {
  const structure = okStructure();
  const transport = stubTransport({ ok: true, structure });
  const result = await fetchSessionStructure({
    credentials: { url: 'http://127.0.0.1:1', token: 't' },
    transport,
  });
  assert.equal(result.ok, true);
  assert.deepEqual(result.structure, structure);
  // 传输层：POST ＋ 路径 ＋ 令牌 ＋ 体。
  assert.equal(transport.seen.length, 1);
  assert.equal(transport.seen[0].path, ROUTE_PATH);
  assert.equal(transport.seen[0].body, '{"command":"sessionStructure"}');
});

test('信封上的**站点短码**读得出来；形态不对 ⇒ 空串（**不猜**）', async () => {
  const structure = okStructure();
  const withSite = await fetchSessionStructure({
    credentials: { url: 'http://127.0.0.1:1', token: 't' },
    transport: stubTransport({ ok: true, structure, site: 'deepseek' }),
  });
  assert.equal(withSite.ok, true);
  assert.equal(withSite.site, 'deepseek', '内核给的站点短码要读得出来');

  const missing = await fetchSessionStructure({
    credentials: { url: 'http://127.0.0.1:1', token: 't' },
    transport: stubTransport({ ok: true, structure }),
  });
  assert.equal(missing.site, '', '没有就是没有 ⇒ 空串（不猜）');

  const bad = await fetchSessionStructure({
    credentials: { url: 'http://127.0.0.1:1', token: 't' },
    transport: stubTransport({ ok: true, structure, site: 'DeepSeek 官网/带斜杠' }),
  });
  assert.equal(bad.site, '', '形态不受控的串不许进目录页（它会进缓存与界面）');
});

test('站内码**原样上报**（不解释语义，码留在闭集内）', async () => {
  const transport = stubTransport({
    ok: false,
    error: { code: 'catalog_failed', detail: '站内码 40003 INVALID_TOKEN' },
  });
  const result = await fetchSessionStructure({
    credentials: { url: 'http://127.0.0.1:1', token: 't' },
    transport,
  });
  assert.equal(result.ok, false);
  assert.equal(result.code, 'catalog_failed');
  assert.match(result.detail, /40003/);
});

test('闭集外的码不许漏出去（归 `catalog_failed`，原码留在 detail）', async () => {
  const transport = stubTransport({ ok: false, error: { code: 'not_a_code', detail: 'x' } });
  const result = await fetchSessionStructure({
    credentials: { url: 'http://127.0.0.1:1', token: 't' },
    transport,
  });
  assert.equal(result.ok, false);
  assert.equal(result.code, 'catalog_failed');
});

test('回包形状不对 ⇒ `schema_changed`（协议 §4.3）', async () => {
  const broken = okStructure();
  delete broken.sessionsLength;
  const transport = stubTransport({ ok: true, structure: broken });
  const result = await fetchSessionStructure({
    credentials: { url: 'http://127.0.0.1:1', token: 't' },
    transport,
  });
  assert.equal(result.ok, false);
  assert.equal(result.code, 'schema_changed');
});

test('传输失败与非法回包都归 `catalog_failed`（失败不抛）', async () => {
  const thrown = await fetchSessionStructure({
    credentials: { url: 'http://127.0.0.1:1', token: 't' },
    transport: async () => {
      throw new Error('socket hang up');
    },
  });
  assert.equal(thrown.ok, false);
  assert.equal(thrown.code, 'catalog_failed');

  const garbage = await fetchSessionStructure({
    credentials: { url: 'http://127.0.0.1:1', token: 't' },
    transport: stubTransport('not json'),
  });
  assert.equal(garbage.ok, false);
  assert.equal(garbage.code, 'catalog_failed');
});

test('没有桥 ⇒ `source_unsupported`（不静默降级）', async () => {
  const source = createSiteStructureSource({ env: {} });
  const result = await source.structure();
  assert.equal(result.ok, false);
  assert.equal(result.code, 'source_unsupported');
});

test('结构校验：字段少一个都要说清是哪个', () => {
  assert.equal(validateStructure(okStructure()).ok, true);
  const check = validateStructure({ ...okStructure(), hasMore: 'yes' });
  assert.equal(check.ok, false);
  assert.ok(check.errors.some((e) => e.includes('hasMore')), check.errors.join('、'));
});

test('结构校验：白名单之外的字段也不许流下去（协议 §2.4「白名单之外即红」）', () => {
  const check = validateStructure({ ...okStructure(), title: '不该出现的标题' });
  assert.equal(check.ok, false);
  assert.ok(check.errors.some((e) => e.includes('title')), check.errors.join('、'));
});

test('结构校验：目录项**恰好 6 个键**，多一个（第 7 个）都红', () => {
  // 协议 §2.4 面二：「不许出现第 7 个键」⇒ ⇒ 目录项里多带一个键就是漂移。
  const base = okStructure();
  const extra = { ...base, items: [{ ...base.items[0], body: '正文不该出现' }] };
  const check = validateStructure(extra);
  assert.equal(check.ok, false);
  assert.ok(
    check.errors.some((e) => e.includes('第 7 个键') && e.includes('body')),
    check.errors.join('、'),
  );
});

test('结构校验：目录项的**类型**照 §4.3 ②（`pinned` 布尔、`updated_at` 数）', () => {
  const base = okStructure();
  const wrong = {
    ...base,
    items: [{ ...base.items[0], pinned: 'false', updated_at: '1783051054.518' }],
  };
  const check = validateStructure(wrong);
  assert.equal(check.ok, false);
  assert.ok(check.errors.some((e) => e.includes('pinned')), check.errors.join('、'));
  assert.ok(check.errors.some((e) => e.includes('updated_at')), check.errors.join('、'));
});

test('结构校验：目录项**少一个键**也红（穷举列举，不是「差不多」）', () => {
  const base = okStructure();
  const { title_type: _omitted, ...withoutTitleType } = base.items[0];
  const check = validateStructure({ ...base, items: [withoutTitleType] });
  assert.equal(check.ok, false);
  assert.ok(check.errors.some((e) => e.includes('title_type')), check.errors.join('、'));
});

/** 造一套命令层（带一个**可控的真实源**）。 */
function makeHarness(siteSource) {
  const root = mkdtempSync(join(tmpdir(), 'official-chat-site-source-'));
  const store = {
    save: () => {
      throw new Error('这个判据不落盘');
    },
    load: () => {
      throw new Error('这个判据不读盘');
    },
    listSessionSeqs: () => ({}),
  };
  const providerId = 'official-chat';
  const commands = createCommands({
    store,
    catalogCache: createCatalogCache({ rootDir: join(root, 'catalog-cache') }),
    captureSource: createFixtureSource([]),
    siteSource,
    seqAllocator: createSeqAllocator({ root, providerId }),
    providerId,
    accountScope: 'desktop-dev/default/site-A',
  });
  return { commands, handler: createDispatcher({ commands }) };
}

test('命令层：真实源没接上 ⇒ `source_unsupported`（**不静默降级成夹具**）', async () => {
  const { handler } = makeHarness(null);
  const result = await handler('sessionStructure', {});
  assert.equal(result.ok, false);
  assert.equal(result.error.code, 'source_unsupported');
});

test('命令层：成功 ⇒ 回结构；失败 ⇒ 稳定码 ＋ 脱敏 detail', async () => {
  const structure = okStructure();
  const good = makeHarness({ structure: async () => ({ ok: true, structure }) });
  const okResult = await good.handler('sessionStructure', {});
  assert.equal(okResult.ok, true);
  assert.deepEqual(okResult.value.structure, structure);

  const bad = makeHarness({
    structure: async () => ({ ok: false, code: 'catalog_empty', detail: '站点返回了空目录' }),
  });
  const badResult = await bad.handler('sessionStructure', {});
  assert.equal(badResult.ok, false);
  assert.equal(badResult.error.code, 'catalog_empty');
  assert.equal(badResult.error.details.detail, '站点返回了空目录');
});

test('命令层：账户范围不一致 ⇒ `account_changed`（服务端推导，不采信客户端）', async () => {
  const { handler } = makeHarness({ structure: async () => ({ ok: true, structure: okStructure() }) });
  const result = await handler('sessionStructure', { accountScope: '别的范围' });
  assert.equal(result.ok, false);
  assert.equal(result.error.code, 'account_changed');
});
