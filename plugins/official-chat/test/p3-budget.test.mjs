// P3 G3-2a（协议 §S-16「缓存总量预算的强制点」＋ P3-13）的自测。
//
// ## 判什么
//
// · **核算范围**（裁定 1）：计入 `snapshots/<providerId>/**`（**逐个 provider
//   目录**）＋ `catalog-cache/**`（**平铺**）；**不计入** `seq-claims/`、
//   `refs/`、`logs/`、`sweeps/`。
// · **核算时机**（裁定 2）：排在「占用关系全量扫描 ＋ 常规清理」**之后**
//   ⇒ 读数是**清理后**的总量。
// · **额外淘汰**（裁定 3）：超预算 ⇒ **TTL 内的目录缓存也删**（唯一新增删除动作）；
//   **被占用的快照不删**（②）、**未到期的快照不删**（③）。
// · **上限的诚实读法**（裁定 5）：删到底仍超 ⇒ **只记日志**、不抛异常、整轮继续。
//
// ## 怎么造出「超预算」
//
// 512 MB 太大 ⇒ 造不出真体积；`CONSTANTS` 是 **frozen**（`host/protocol.js:156`）
// ⇒ 改不了常量；Windows 上 `ftruncate` 造出来的**不是稀疏文件**（真占盘）。
// ⇒ 唯一既不改实现（**不加测试钩子/可选参数**）、又不占磁盘的办法是：
// **在子进程里 patch `node:fs` 的 `statSync`**，只把**核算范围内**的文件的
// `size` 放大 ⇒ 核算逻辑被**真实执行**（遍历、排除项、逐 provider 求和都照常），
// 而「超预算」这个场景可达；**删盘是真删**（`unlink`/`rename` 没被 patch）。
// ⇒ 期望值由**父进程独立算**（自己写的夹具 ⇒ 自己知道每个文件的真实字节数）。
//
// ## 不断什么
//
// · 不断「总量具体是多少字节」（那是实现细节）；断的是**哪些算进、哪些不算**。
// · 不断删除用了哪个原语；断的是**哪些文件还在/不在**。

import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readdirSync, statSync, utimesSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';

const NOW = Date.parse('2026-10-21T09:00:00.000Z');
const MINUTE = 60 * 1000;
const DAY = 24 * 60 * MINUTE;

/** 协议 §4 的上限（判定用；实现自己从 `CONSTANTS` 读）。 */
const BUDGET_BYTES = 512 * 1024 * 1024;

/** 放大后必然超预算的量（每个核算范围内文件都加这么多）。 */
const SCALE = 600 * 1024 * 1024;

/** 目录缓存 TTL（§4：5 分钟）。 */
const CACHE_TTL_MS = 5 * MINUTE;

/** 造一个临时 home（**绝不指向用户真实数据目录**）。 */
function makeHome() {
  const dir = mkdtempSync(join(tmpdir(), 'dsh-p3budget-'));
  return { dir, root: join(dir, 'official-chat') };
}

/**
 * 写一个快照文件（**文件内**带 `capturedAt` —— 过期判定看业务时间，mtime 只兜底）。
 *
 * @returns {number} 文件的真实字节数。
 */
function writeSnapshot(root, provider, id, capturedAt) {
  const dir = join(root, 'snapshots', provider);
  mkdirSync(dir, { recursive: true });
  const path = join(dir, `${id}.json`);
  const payload = `${JSON.stringify(
    { schemaVersion: 1, snapshotId: id, capturedAt, messages: [{ role: 'user', text: 'x'.repeat(64) }] },
    null,
    2,
  )}\n`;
  writeFileSync(path, payload, 'utf8');
  return statSync(path).size;
}

/**
 * 写一个目录缓存文件（**平铺**在 `catalog-cache/` 下）。
 *
 * @param {number} ageMs - 文件 mtime 距今多久（决定它在不在 TTL 内）。
 * @returns {number} 文件的真实字节数。
 */
function writeCache(root, name, ageMs) {
  const dir = join(root, 'catalog-cache');
  mkdirSync(dir, { recursive: true });
  const path = join(dir, name);
  writeFileSync(path, `${JSON.stringify({ fetchedAt: new Date(NOW - ageMs).toISOString(), page: {} })}\n`, 'utf8');
  const stamp = new Date(NOW - ageMs);
  utimesSync(path, stamp, stamp);
  return statSync(path).size;
}

/** 写一条占用记录（协议 §S-11 的四字段闭集）。 */
function writeOccupancy(root, id) {
  const dir = join(root, 'refs');
  mkdirSync(dir, { recursive: true });
  writeFileSync(
    join(dir, `${id}.json`),
    `${JSON.stringify({ snapshotId: id, capturedAt: new Date(NOW).toISOString(), createdAt: new Date(NOW).toISOString(), sessionId: 's' }, null, 2)}\n`,
    'utf8',
  );
}

/** 写一个「被排除」的目录里的文件（`seq-claims/`／`refs/`／`logs/`／`sweeps/`）。 */
function writeExcluded(root, relative) {
  const path = join(root, relative);
  mkdirSync(join(path, '..'), { recursive: true });
  writeFileSync(path, 'x'.repeat(4096), 'utf8');
  return statSync(path).size;
}

/**
 * 在**子进程**里跑一次清理。
 *
 * 子进程里 patch `node:fs` 的 `statSync`（只放大核算范围内文件的 `size`）
 * ＋ 记录 `rename`/`unlink` 的真实调用（观测**实际删盘**）。
 *
 * @param {string} root - 实例根。
 * @param {number} scale - 每个核算范围内文件的 `size` 放大值（0 ＝ 不放大）。
 * @returns {{result: object, warns: string[], deleted: object[]}}
 */
function runSweepInChild(root, scale) {
  const script = [
    "import { createRequire } from 'node:module';",
    "const require = createRequire('file:///' + process.cwd().replace(/\\\\/g, '/') + '/');",
    'const fs = require("node:fs");',
    'const DELETED = [];',
    'const realRename = fs.renameSync;',
    'fs.renameSync = function (f, t, ...rest) { DELETED.push({ kind: "rename", path: String(f), to: String(t) }); return realRename.call(fs, f, t, ...rest); };',
    'const realUnlink = fs.unlinkSync;',
    'fs.unlinkSync = function (p, ...rest) { DELETED.push({ kind: "unlink", path: String(p) }); return realUnlink.call(fs, p, ...rest); };',
    `const SCALE = ${scale};`,
    'if (SCALE > 0) {',
    '  const realStat = fs.statSync;',
    '  fs.statSync = function (p, ...rest) {',
    '    const st = realStat.call(fs, p, ...rest);',
    '    const s = String(p);',
    '    if (!s.includes("snapshots") && !s.includes("catalog-cache")) return st;',
    '    return new Proxy(st, { get(t, k) { if (k === "size") return t.size + SCALE; const v = t[k]; return typeof v === "function" ? v.bind(t) : v; } });',
    '  };',
    '}',
    `const { createSweeper } = await import(${JSON.stringify(new URL('../host/sweep.js', import.meta.url).href)});`,
    'const WARNS = [];',
    'const sweeper = createSweeper({',
    '  root: process.env.DSH_BUDGET_ROOT,',
    "  providerId: 'ds',",
    `  now: () => ${NOW},`,
    '  logger: { warn: (message) => WARNS.push(String(message)) },',
    '});',
    'const result = sweeper.sweep();',
    'process.stdout.write(JSON.stringify({ result, warns: WARNS, deleted: DELETED }));',
  ].join('\n');
  const stdout = execFileSync(process.execPath, ['--input-type=module', '-e', script], {
    encoding: 'utf8',
    timeout: 120_000,
    env: { ...process.env, DSH_BUDGET_ROOT: root },
  });
  return JSON.parse(stdout.trim().split('\n').pop() ?? 'null');
}

/** 列出实例根下某目录里的文件名（相对该目录）。 */
function listDir(root, relative) {
  const dir = join(root, relative);
  if (!existsSync(dir)) return [];
  return readdirSync(dir).sort();
}

// ---------------------------------------------------------------------------
// §S-16 裁定 1｜核算范围
// ---------------------------------------------------------------------------

test('§S-16①：核算范围 —— seq-claims/、refs/、logs/、sweeps/ 里的文件**不计入**总量', () => {
  const a = makeHome();
  const snapshotSize = writeSnapshot(a.root, 'ds', 'ds~aaaa~0001', new Date(NOW).toISOString());
  const cacheSize = writeCache(a.root, 'ds__scope__q.json', 0);
  const base = runSweepInChild(a.root, 0);
  assert.equal(base.result.budget.totalBytes, snapshotSize + cacheSize, '总量只含快照 ＋ 目录缓存');

  const b = makeHome();
  const snapshotSize2 = writeSnapshot(b.root, 'ds', 'ds~aaaa~0001', new Date(NOW).toISOString());
  const cacheSize2 = writeCache(b.root, 'ds__scope__q.json', 0);
  const excluded = [
    writeExcluded(b.root, join('seq-claims', 'ds', 'sess', '0001.json')),
    writeExcluded(b.root, join('refs', 'ds~aaaa~0001.json')),
    writeExcluded(b.root, join('logs', 'official-chat.log')),
    writeExcluded(b.root, join('sweeps', 'last-sweep-2026-10-21.json')),
  ];
  assert.ok(excluded.every((size) => size > 0), '被排除目录里的文件确实有体积');
  const withExcluded = runSweepInChild(b.root, 0);
  assert.equal(
    withExcluded.result.budget.totalBytes,
    snapshotSize2 + cacheSize2,
    '加了 seq-claims/refs/logs/sweeps 里的文件后，总量读数**不变**（那些不计入）',
  );
});

test('§S-16①：快照按 **provider 目录**逐个计入（不只当前 provider）', () => {
  const home = makeHome();
  const first = writeSnapshot(home.root, 'ds', 'ds~aaaa~0001', new Date(NOW).toISOString());
  const before = runSweepInChild(home.root, 0);
  assert.equal(before.result.budget.totalBytes, first, '先只有一个 provider 目录');

  const second = writeSnapshot(home.root, 'other', 'other~bbbb~0001', new Date(NOW).toISOString());
  const after = runSweepInChild(home.root, 0);
  assert.equal(
    after.result.budget.totalBytes,
    first + second,
    '另一个 provider 目录里的快照**也要计入**（上限是 512 MB / **实例**）',
  );
});

test('§S-16①：目录缓存**平铺**计入（文件名里的 `__` 不构成子目录）', () => {
  const home = makeHome();
  const snapshotSize = writeSnapshot(home.root, 'ds', 'ds~aaaa~0001', new Date(NOW).toISOString());
  const before = runSweepInChild(home.root, 0);
  assert.equal(before.result.budget.totalBytes, snapshotSize);

  // **provider 名里带 `__`** ⇒ 文件名形如 `ds__scope__query.json`（平铺，不是目录）。
  const cacheSize = writeCache(home.root, 'ds__self-test-account__some query.json', 0);
  const after = runSweepInChild(home.root, 0);
  assert.equal(
    after.result.budget.totalBytes,
    snapshotSize + cacheSize,
    '目录缓存按平铺文件计入（`catalog-cache/` 下没有 provider 子目录）',
  );
});

// ---------------------------------------------------------------------------
// §S-16 裁定 2｜核算时机
// ---------------------------------------------------------------------------

test('§S-16②：核算排在**常规清理之后**（读数是清理后的总量）', () => {
  const home = makeHome();
  // **已到期**的快照 ⇒ 常规清理会把它删掉 ⇒ 核算读数**不该**再含它。
  const expiredSize = writeSnapshot(home.root, 'ds', 'ds~aaaa~0001', new Date(NOW - 8 * DAY).toISOString());
  const freshSize = writeSnapshot(home.root, 'ds', 'ds~aaaa~0002', new Date(NOW).toISOString());
  const cacheSize = writeCache(home.root, 'ds__scope__q.json', 0);
  // **TTL 已过**的目录缓存 ⇒ 常规清理（`sweepCache`）会把它删掉。
  // ⚠️ **两个家族都要有**：只放快照 ⇒ 断言只对「快照清理」敏感，
  //    核算被提到 `sweepCache` 之前时照样绿 ⇒ 断不出时机。
  const staleCacheSize = writeCache(home.root, 'ds__scope__old.json', CACHE_TTL_MS + MINUTE);

  const out = runSweepInChild(home.root, 0);
  assert.equal(out.result.snapshots.removed, 1, '常规清理确实删掉了那份已到期快照');
  assert.equal(out.result.cache.removed, 1, '常规清理确实删掉了那份 TTL 已过的目录缓存');
  assert.equal(existsSync(join(home.root, 'snapshots', 'ds', 'ds~aaaa~0001.json')), false, '它已被删');
  assert.equal(existsSync(join(home.root, 'catalog-cache', 'ds__scope__old.json')), false, '它已被删');
  assert.equal(
    out.result.budget.totalBytes,
    freshSize + cacheSize,
    '核算读数**不含**已被常规清理删掉的那些（若核算排在清理之前，读数会多出它们）',
  );
  assert.ok(
    expiredSize > 0 && staleCacheSize > 0 && freshSize > 0 && cacheSize > 0,
    '四个文件都确实有体积（否则这条断言没有区分力）',
  );
});

// ---------------------------------------------------------------------------
// §S-16 裁定 3｜额外淘汰
// ---------------------------------------------------------------------------

test('§S-16③①：超预算 ⇒ **TTL 内**的目录缓存被删、总量回落', () => {
  const home = makeHome();
  const snapshotSize = writeSnapshot(home.root, 'ds', 'ds~aaaa~0001', new Date(NOW).toISOString());
  // **TTL 内**的缓存（mtime = 现在）⇒ 常规清理不动它 ⇒ 只可能被额外淘汰删掉。
  const cacheSize = writeCache(home.root, 'ds__scope__q.json', 0);
  const out = runSweepInChild(home.root, SCALE);

  assert.equal(out.result.budget.over, true, '**核算总量**确实超预算');
  assert.ok(
    out.result.budget.evictedCacheFiles >= 1,
    `额外淘汰应当删掉 TTL 内的目录缓存（evictedCacheFiles=${out.result.budget.evictedCacheFiles}）`,
  );
  assert.equal(
    existsSync(join(home.root, 'catalog-cache', 'ds__scope__q.json')),
    false,
    'TTL 内的目录缓存被删了',
  );
  // **核算总量**（清理后、淘汰前）＝ 快照 ＋ 缓存（都还在的那一瞬间）。
  // ⚠️ `SCALE` 是**每个核算范围内文件各加一份**（两个文件 ⇒ 两份）。
  assert.equal(
    out.result.budget.totalBytes,
    snapshotSize + cacheSize + 2 * SCALE,
    '核算总量是「清理后、淘汰前」的总量（裁定 2 ＋ 裁定 5 的「核算总量」要素）',
  );
  // **淘汰后总量**（另算一个字段）＝ 缓存没了 ⇒ 回落。
  assert.equal(
    out.result.budget.totalBytesAfterEviction,
    snapshotSize + SCALE,
    '淘汰后总量回落到「只剩快照」（裁定 5 的「淘汰后总量」要素）',
  );
  assert.ok(
    out.result.budget.totalBytesAfterEviction < out.result.budget.totalBytes,
    '总量确实**回落**了（不是恒等）',
  );
  assert.equal(out.result.budget.stillOver, true, '快照那份仍被放大 ⇒ 淘汰后仍超（尽力而为）');
  assert.ok(cacheSize > 0 && snapshotSize > 0, '两个文件都确实有体积');
});

test('§S-16③②：被占用的快照在超预算时**仍在**（硬约束，不为腾空间绕开 §S-11）', () => {
  const home = makeHome();
  const id = 'ds~aaaa~0001';
  writeSnapshot(home.root, 'ds', id, new Date(NOW).toISOString());
  writeOccupancy(home.root, id);
  writeCache(home.root, 'ds__scope__q.json', 0);

  const out = runSweepInChild(home.root, SCALE);
  assert.equal(out.result.budget.over, true, '总量确实超预算');
  assert.equal(
    existsSync(join(home.root, 'snapshots', 'ds', `${id}.json`)),
    true,
    '被占用的快照**仍在**（哪怕超预算、哪怕它还没到期）',
  );
});

test('§S-16③③：未到期且无占用的快照在超预算时**仍在**（不许按最旧删未到期快照）', () => {
  const home = makeHome();
  const id = 'ds~aaaa~0001';
  writeSnapshot(home.root, 'ds', id, new Date(NOW).toISOString());
  writeCache(home.root, 'ds__scope__q.json', 0);

  const out = runSweepInChild(home.root, SCALE);
  assert.equal(out.result.budget.over, true, '总量确实超预算');
  assert.equal(
    existsSync(join(home.root, 'snapshots', 'ds', `${id}.json`)),
    true,
    '未到期的快照**仍在**（P1 §4.1 第 3 条「只删超过 7 天的」不许被绕开）',
  );
});

test('§S-16③：额外淘汰**只删目录缓存**，一次快照都不删（子进程观测真实删盘）', () => {
  const home = makeHome();
  const id = 'ds~aaaa~0001';
  writeSnapshot(home.root, 'ds', id, new Date(NOW).toISOString());
  writeOccupancy(home.root, id);
  writeCache(home.root, 'ds__scope__q.json', 0);

  const out = runSweepInChild(home.root, SCALE);
  const deletedPaths = out.deleted.map((item) => item.path);
  assert.ok(
    deletedPaths.some((path) => path.includes('catalog-cache')),
    `额外淘汰必须真的删盘（观测到的删除：${JSON.stringify(deletedPaths)}）`,
  );
  assert.equal(
    deletedPaths.filter((path) => path.includes(join('snapshots', 'ds'))).length,
    0,
    '**不许**删任何快照（被占用的不删、未到期的不删）',
  );
});

// ---------------------------------------------------------------------------
// §S-16 裁定 4 ＋ 5｜上限的诚实读法
// ---------------------------------------------------------------------------

test('§S-16④⑤：删到底仍超 ⇒ 有留痕、不抛异常、整轮继续', () => {
  const home = makeHome();
  // **未到期的快照顶住总量** ＋ **没有目录缓存可删** ⇒ 删到底仍超。
  const id = 'ds~aaaa~0001';
  writeSnapshot(home.root, 'ds', id, new Date(NOW).toISOString());

  const out = runSweepInChild(home.root, SCALE);
  assert.equal(out.result.budget.over, true, '核算总量超预算（尽力而为 ⇒ 做不到就如实说）');
  assert.equal(out.result.budget.stillOver, true, '删到底**仍超**（缺口 G3-5：不得假装解决）');
  assert.ok(
    out.warns.some((line) => line.includes('超预算')),
    `超预算必须**留痕**（日志里看得到）：${JSON.stringify(out.warns)}`,
  );
  // **整轮继续**：六个家族的清理结果都在（不是中途抛异常）。
  for (const family of ['refs', 'snapshots', 'tmp', 'claims', 'cache', 'log']) {
    assert.ok(out.result[family] !== undefined, `清理整轮继续（缺 ${family} 的结果）`);
  }
  assert.equal(
    existsSync(join(home.root, 'snapshots', 'ds', `${id}.json`)),
    true,
    '未到期的快照仍在（不假装解决 ⇒ 缺口 G3-5）',
  );
});

test('§S-16⑤：超预算的**留痕含五要素**（核算总量/预算值/是否超预算/淘汰了什么/淘汰后总量）', () => {
  const home = makeHome();
  writeSnapshot(home.root, 'ds', 'ds~aaaa~0001', new Date(NOW).toISOString());
  const cacheSize = writeCache(home.root, 'ds__scope__q.json', 0);

  const out = runSweepInChild(home.root, SCALE);
  const line = out.warns.find((item) => item.includes('超预算'));
  assert.ok(line, `有一条超预算留痕：${JSON.stringify(out.warns)}`);

  // ① **核算总量**（清理后、淘汰前）。
  assert.ok(
    line.includes(String(out.result.budget.totalBytes)),
    `留痕里有核算总量（${out.result.budget.totalBytes}）：${line}`,
  );
  // ② **预算值**。
  assert.ok(line.includes(String(out.result.budget.budgetBytes)), `留痕里有预算值：${line}`);
  // ③ **是否超预算**（字面「超预算」＋ 两个数值可比）。
  assert.ok(line.includes('超预算'), `留痕明说是否超预算：${line}`);
  // ④ **淘汰了什么**（数量 ＋ 类别）——用**短语 ＋ 数字**匹配，不靠裸子串。
  assert.ok(
    line.includes(`已淘汰目录缓存 ${out.result.budget.evictedCacheFiles} 个`),
    `留痕里有淘汰的数量与类别（evictedCacheFiles=${out.result.budget.evictedCacheFiles}）：${line}`,
  );
  // ⑤ **淘汰后总量**。
  assert.ok(
    line.includes(String(out.result.budget.totalBytesAfterEviction)),
    `留痕里有淘汰后总量（${out.result.budget.totalBytesAfterEviction}）：${line}`,
  );
  // **不得**声称总量已被强制在上限内。
  assert.ok(
    !/已被?强制|已限制在|保证不超过/.test(line),
    `不得声称总量已被强制在上限内：${line}`,
  );
  assert.ok(cacheSize > 0, '夹具里的目录缓存确实有体积');
});
