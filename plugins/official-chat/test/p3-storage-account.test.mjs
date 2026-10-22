// P3 第一批实现（S-14 存储保护 ＋ S-15 账户维度服务端化）的自测。
//
// ## 判什么
//
// · **S-14**：目录缓存的**目录 `0700`、文件 `0600`**（Unix 侧断**盘上真实
//   权限**；Windows 上 `mode` 是 no-op ⇒ 只断「参数在位」，那边的实际保护
//   来自**用户 profile 的默认 ACL** ⇒ **不声称跨平台加密**）。
// · **S-15**：账户维度**由服务端推导** —— 请求里的 `accountScope` **只做
//   一致性校验**：与服务端推导值不符 ⇒ 拒（`account_changed`）、**不静默
//   改写**（静默改写会让用户以为切了账户其实没切）；**没传就放行**。
//   **目录缓存键**与**快照的 `accountScope` 字段**都不受请求参数影响。
//
// ## 不判什么
//
// · **Windows ACL / 跨平台加密**：协议 §S-14 明确「本批只做如实记录」，
//   不做跨平台加密断言。
// · **真机上的呈现**：这里判的是 Node 落盘那一层与命令层的业务口径。
// · **P3 的其余条款**（S-8…S-13）：本文件只覆盖已落地的 S-14 / S-15。

import assert from 'node:assert/strict';
import { chmodSync, existsSync, mkdtempSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

import { createCatalogCache, createCommands, createFixtureSource } from '../host/commands.js';
import { createSeqAllocator } from '../host/seq-store.js';

/** 服务端推导的账户范围（`createCommands` 的入参 ⇒ 闭包里的服务端变量）。 */
const ACCOUNT_SCOPE = 'desktop-dev/default/site-A';

/** 实现源码（Windows 侧只能断「mode 参数在位」⇒ 需要读源码）。 */
const HOST_SOURCE = fileURLToPath(new URL('../host/commands.js', import.meta.url));

/**
 * 取 `createCatalogCache` 里 `write()` 的源码正文。
 *
 * **为什么要圈出这一段**：Windows 上 `mode` 是 no-op（Node 语义）⇒ 盘上权限
 * **断不了** ⇒ 只能断**源码形态**；而断**全文件**会把别处的同名字符串也算
 * 进去 ⇒ 圈出 `write()` 的正文 ⇒ 断言的是「**这一处**确实带了收口」。
 *
 * @returns {string} `write()` 的源码正文。
 */
function writeFunctionSource() {
  const source = readFileSync(HOST_SOURCE, 'utf8');
  // ⚠️ **签名跟着键一起长了一段站**（`write(provider, site, accountScope, query, page)`）
  // ⇒ ⇒ 这个 `indexOf` 的**字面量必须跟着改**，否则它会 `-1` ⇒ ⇒ 断言变成
  // 「实现里应当有 write()」那条**假红**（不是真的少了收口）。
  const start = source.indexOf('function write(provider, site, accountScope, query, page) {');
  assert.notEqual(start, -1, '实现里应当有 createCatalogCache 的 write()');
  const end = source.indexOf('\n  }', start);
  return source.slice(start, end === -1 ? start + 1200 : end);
}

/**
 * 造一套内存桩：存储 ＋ 目录缓存 ＋ 固定夹具取数源。
 *
 * 与 `host-commands.test.mjs` 同款，但**多留两个观测口**：
 * `saves`（落盘过几次、落了什么）与 `cacheDir`（缓存目录）。
 */
function makeHarness() {
  const root = mkdtempSync(join(tmpdir(), 'official-chat-p3-'));
  const cacheDir = join(root, 'catalog-cache');
  const written = new Map();
  const saves = [];
  const store = {
    // **失败路径要写日志**（`logFailure` 按 `store.root` 找位置）⇒ 给一个
    // 真实的临时根 ⇒ 被拒的请求也会留下一条可查的日志。
    root,
    save: (snapshot) => {
      saves.push(snapshot);
      written.set(String(snapshot?.snapshotId ?? ''), snapshot);
      return { snapshotId: snapshot.snapshotId, contentDigest: snapshot.contentDigest };
    },
    load: (id) => {
      const found = written.get(String(id));
      if (found === undefined) {
        const error = new Error(`快照不存在：${String(id)}`);
        error.code = 'snapshot_invalid';
        throw error;
      }
      return { snapshot: found, body: found.messages.map((m) => m.text).join('\n\n') };
    },
    listSessionSeqs: () => ({}),
  };
  const commands = createCommands({
    store,
    catalogCache: createCatalogCache({ rootDir: cacheDir }),
    captureSource: createFixtureSource([
      { sessionId: 'sess-alpha-001', title: '夹具对话', body: '夹具正文。' },
    ]),
    seqAllocator: createSeqAllocator({ root, providerId: 'official-chat' }),
    providerId: 'official-chat',
    accountScope: ACCOUNT_SCOPE,
  });
  return { root, cacheDir, saves, store, commands };
}

/** 缓存目录里现在有几个文件（目录还没建 ⇒ 0 个）。 */
function cacheFileCount(cacheDir) {
  return existsSync(cacheDir) ? readdirSync(cacheDir).length : 0;
}

test('S-14：目录缓存的目录 0700、文件 0600（Unix 侧断盘上真实权限）', () => {
  const root = mkdtempSync(join(tmpdir(), 'official-chat-p3-mode-'));
  const rootDir = join(root, 'catalog-cache');
  const cache = createCatalogCache({ rootDir });
  cache.write('official-chat', '', ACCOUNT_SCOPE, '关键词', { items: [], providerId: 'official-chat' });
  const files = readdirSync(rootDir);
  assert.equal(files.length, 1, '应当只落一个缓存文件');
  const file = join(rootDir, files[0]);
  if (process.platform === 'win32') {
    // **Windows 上 `mode` 是 no-op**（Node 语义）⇒ 盘上权限位**不可断言**
    // ⇒ 这里只断「参数在位」（协议 §S-14 裁定 2：本批只做 Unix 侧的可机检
    // 断言，Windows 侧只做如实记录）。**断在 `write()` 的正文里** ⇒ 断言的是
    // 「**这一处**确实带了 mode」，不是全文件里有同名字符串就算。
    const body = writeFunctionSource();
    assert.match(body, /mkdirSync\(rootDir, \{ recursive: true, mode: 0o700 \}\)/, '缓存目录必须带 mode 0o700');
    assert.match(body, /\{ encoding: 'utf8', mode: 0o600 \}/, '缓存文件必须带 mode 0o600');
  } else {
    assert.equal(statSync(rootDir).mode & 0o777, 0o700, '缓存目录必须是 0700');
    assert.equal(statSync(file).mode & 0o777, 0o600, '缓存文件必须是 0600');
  }
});

test('S-14：存量缓存文件也被收进同一口径（mode 只在创建时生效 ⇒ 写后要显式收）', () => {
  const root = mkdtempSync(join(tmpdir(), 'official-chat-p3-mode2-'));
  const rootDir = join(root, 'catalog-cache');
  const cache = createCatalogCache({ rootDir });
  const page = { items: [], providerId: 'official-chat' };
  cache.write('official-chat', '', ACCOUNT_SCOPE, '关键词', page);
  const file = join(rootDir, readdirSync(rootDir)[0]);
  // **造一个「宽权限」的存量**（模拟本条落地前写下的文件）⇒ 再写一次。
  chmodSync(file, 0o666);
  cache.write('official-chat', '', ACCOUNT_SCOPE, '关键词', page);
  if (process.platform === 'win32') {
    // **Windows 上 `mode`/`chmod` 的权限位是 no-op**（Node 语义）⇒ 盘上权限
    // **断不了** ⇒ 断**源码形态**：**这一处**在写完之后确实显式收了一次
    // （`mode` 只在创建时生效 ⇒ 存量收口那两行必须在位）。
    //
    // **不许留空断言**：Windows 上断言体为空 ＝ **空跑通过** ＝ 假绿 ⇒ 这里
    // 必须有一件真能失败的事。
    const body = writeFunctionSource();
    assert.match(body, /chmodQuiet\(rootDir, 0o700\)/, '存量目录的收口必须在位');
    assert.match(body, /chmodQuiet\(path, 0o600\)/, '存量文件的收口必须在位');
  } else {
    assert.equal(statSync(file).mode & 0o777, 0o600, '存量文件也要被收成 0600');
    assert.equal(statSync(rootDir).mode & 0o777, 0o700, '存量目录也要被收成 0700');
  }
});

test('S-15：catalog 的账户维度由服务端推导 —— 请求里的 accountScope 不符即拒', async () => {
  const { commands, cacheDir } = makeHarness();
  const bad = await commands.catalog({ query: '', accountScope: 'site-B' });
  assert.equal(bad.ok, false, '不符必须拒');
  assert.equal(bad.code, 'account_changed');
  assert.equal(bad.detail, '账户范围与服务端推导值不一致');
  // **不许留痕**：缓存键是服务端推导的 ⇒ 被拒的请求不该写缓存。
  assert.equal(cacheFileCount(cacheDir), 0, '被拒的 catalog 不得落缓存');
});

test('S-15：capture 的账户维度由服务端推导 —— 不符即拒，且不落盘', async () => {
  const { commands, saves } = makeHarness();
  const listed = await commands.catalog({ query: '' });
  const snapshotId = listed.allocations['sess-alpha-001'];
  const bad = await commands.capture({
    sessionId: 'sess-alpha-001',
    snapshotId,
    accountScope: 'site-B',
  });
  assert.equal(bad.ok, false, '不符必须拒');
  assert.equal(bad.code, 'account_changed');
  assert.equal(saves.length, 0, '被拒的 capture 不得落盘');
});

test('S-15：快照的 accountScope 字段写服务端推导值（请求参数不影响落盘）', async () => {
  const { commands, saves } = makeHarness();
  const listed = await commands.catalog({ query: '' });
  const snapshotId = listed.allocations['sess-alpha-001'];
  const ok = await commands.capture({ sessionId: 'sess-alpha-001', snapshotId });
  assert.equal(ok.ok, true);
  assert.equal(ok.snapshot.accountScope, ACCOUNT_SCOPE, '落盘字段必须是服务端推导值');
  // **两个不同的请求参数 ⇒ 落盘字段相同**（P3-9①）：两个不符值都被拒 ⇒
  // 盘上不会多出「按请求参数分的」快照。
  const other = await commands.capture({
    sessionId: 'sess-alpha-001',
    snapshotId,
    accountScope: 'site-C',
  });
  assert.equal(other.ok, false);
  assert.equal(other.code, 'account_changed');
  assert.equal(saves.length, 1, '只有被接受的那次落了盘');
  assert.equal(saves[0].accountScope, ACCOUNT_SCOPE);
});

test('S-15（协议 v0.2 可机检形态②）：显式传服务端同值 ⇒ 落盘字段与「省略」时相同', async () => {
  const { commands, saves } = makeHarness();
  // **第一次：省略 `accountScope`** ⇒ 放行（客户端本来就不传这个字段）。
  const first = await commands.catalog({ query: '' });
  const omitted = await commands.capture({
    sessionId: 'sess-alpha-001',
    snapshotId: first.allocations['sess-alpha-001'],
  });
  assert.equal(omitted.ok, true);
  // **第二次：显式传服务端同值** ⇒ 同样放行（一致性校验不是拒绝一切）。
  // （再调一次 `catalog` 拿**新** id：每次 catalog 都新分配，撞不上第一次。）
  const second = await commands.catalog({ query: '' });
  const same = await commands.capture({
    sessionId: 'sess-alpha-001',
    snapshotId: second.allocations['sess-alpha-001'],
    accountScope: ACCOUNT_SCOPE,
  });
  assert.equal(same.ok, true);
  // **两次落盘字段相同** ⇒ 落盘字段与请求参数的**形态**无关
  // （省略 vs 显式同值）—— 这就是 v0.2 的可机检形态②。
  assert.equal(omitted.snapshot.accountScope, same.snapshot.accountScope);
  assert.equal(same.snapshot.accountScope, ACCOUNT_SCOPE);
  assert.equal(saves.length, 2);
});

test('S-15：目录缓存键不受请求参数影响（不同请求参数 ⇒ 不落到不同缓存文件）', async () => {
  const { commands, cacheDir } = makeHarness();
  const first = await commands.catalog({ query: '' });
  assert.equal(first.ok, true);
  assert.equal(first.page.accountScope, ACCOUNT_SCOPE, '目录页的账户维度也是服务端推导值');
  const files = readdirSync(cacheDir);
  assert.equal(files.length, 1);
  assert.ok(
    files[0].includes('desktop-dev_default_site-A'),
    `缓存键必须含服务端推导的账户维度：${files[0]}`,
  );
  // **不符即拒 ⇒ 不会多出一个按请求参数分的缓存文件**。
  const bad = await commands.catalog({ query: '', accountScope: 'site-B' });
  assert.equal(bad.ok, false);
  assert.equal(readdirSync(cacheDir).length, 1, '被拒的请求不得落第二个缓存文件');
});

test('S-15：请求里没带 accountScope ⇒ 放行（客户端本来就不传这个字段）', async () => {
  const { commands } = makeHarness();
  const ok = await commands.catalog({ query: '' });
  assert.equal(ok.ok, true, '没传就放行');
  assert.equal(ok.page.accountScope, ACCOUNT_SCOPE);
  const listed = await commands.catalog({ query: '' });
  const captured = await commands.capture({
    sessionId: 'sess-alpha-001',
    snapshotId: listed.allocations['sess-alpha-001'],
  });
  assert.equal(captured.ok, true);
  assert.equal(captured.snapshot.accountScope, ACCOUNT_SCOPE);
});

test('S-15：客户端传的值与服务端推导值一致 ⇒ 放行（一致性校验不是拒绝一切）', async () => {
  const { commands } = makeHarness();
  const ok = await commands.catalog({ query: '', accountScope: ACCOUNT_SCOPE });
  assert.equal(ok.ok, true, '一致的值必须放行');
  assert.equal(ok.page.accountScope, ACCOUNT_SCOPE);
});
