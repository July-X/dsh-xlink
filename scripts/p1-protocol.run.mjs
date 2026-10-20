// P1 协议判据的**实跑**入口：对真实实现跑协议稿 §8 的 12 条验收项。
//
// ## 用法
//
// ```powershell
// node scripts/p1-protocol.run.mjs            # 跑一轮，输出到 %TEMP%\dsh-p1-protocol\
// node scripts/p1-protocol.run.mjs --round r2 # 指定轮次名（证据按轮次独立命名）
// ```
//
// ## 产出
//
// `%TEMP%\dsh-p1-protocol\results-<round>.json`：
// `{round, at, results: [{name,status,detail}], summary: {total,pass,fail,skipped}}`
// —— 与既有产物（内核 `results.<轮次>.json`）同构。
//
// ## 隔离
//
// 落盘根目录在 `%TEMP%\dsh-p1-protocol-*` 一次性目录里，**绝不指向用户真实数据目录**；
// 跑完删掉（只删自己建的）。原实现文件一个字节都不改。
//
// @module scripts/p1-protocol-run

import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';

import {
  PASS,
  FAIL,
  SKIPPED,
  checkCatalogCodesSeparated,
  checkChipInvalidNode,
  checkConstants,
  checkCrossProcess,
  checkErrorCodeTable,
  checkErrorNoInternalLeak,
  checkFixtureCoverage,
  checkFixtureExpectationMatch,
  checkFixtureExpectations,
  checkFixtureParity,
  checkGapRegistry,
  checkInvalidRejection,
  checkIsolationSurface,
  checkNoBodyDedup,
  checkNamespaceIsolation,
  checkPickTwiceDistinct,
  checkRefLengthBoundary,
  checkRequestMatchesDisk,
  checkSingleSourceOfTruth,
  summarize,
  verdict,
} from './lib/p1-protocol.mjs';
import { createAdapter } from './lib/p1-adapter.mjs';
import {
  findDataDirectoryViolationsInFiles,
  findProfileViolationsInFiles,
} from './lib/probe-profile-isolation.mjs';

const HERE = fileURLToPath(new URL('.', import.meta.url));
const REPO = fileURLToPath(new URL('../', import.meta.url));

/** 协议夹具目录（协议 §6 的「同一组 JSON 夹具」）。 */
const FIXTURE_DIR = join(REPO, 'plugins', 'official-chat', 'fixtures', 'protocol');

/** 本次新增的文件（协议 §8-11：新增代码不得让判据扫描面外泄）。 */
const NEW_FILES = [
  'scripts/lib/p1-protocol.mjs',
  'scripts/lib/p1-adapter.mjs',
  'scripts/p1-protocol.test.mjs',
  'scripts/p1-protocol.run.mjs',
  'src-tauri/examples/p1_protocol_fixture.rs',
];

/**
 * 读协议夹具集。
 *
 * @returns {Array<object>}
 */
function readFixtures() {
  if (!existsSync(FIXTURE_DIR)) return [];
  const indexPath = join(FIXTURE_DIR, 'index.json');
  const names = existsSync(indexPath)
    ? (JSON.parse(readFileSync(indexPath, 'utf8')).cases ?? [])
    : [];
  const list = names.length > 0 ? names : [];
  const out = [];
  for (const name of list) {
    const path = join(FIXTURE_DIR, name);
    if (existsSync(path)) out.push(JSON.parse(readFileSync(path, 'utf8')));
  }
  return out;
}

/**
 * A0｜**通道可达性**（client 半边 ↔ host 半边）——当前**不可机检 → 未执行**。
 *
 * 为什么要有这条（别让它悄悄不存在）：
 *   判据适配器现在把 `connection.rpc.call` **直接接到** host 的 handler
 *   （`scripts/lib/p1-adapter.mjs:236-242`），**绕过了内核 `connection` 服务的
 *   真实路由注册**（`ctx.connection.rpc.handle(channel, handler)`）。而真机
 *   （devloper p1-r2）恰恰死在那一步：`cannot get property "webServer" without
 *   inject`（`host/index.js:486`）→ 路由没注册 → 菜单空、正文没进模型。
 *   ⇒ **判据现在看不到这个真洞**：真机全红、判据不报。
 *   这正是「桩把两条本该跨进程的调用接上了」那个坑——**桩会骗过自己**。
 *
 * 通道形态（上游修一处 / 插件自注册 / 走壳的 Tauri 命令）定案后，本条要
 * 按**真实装配口径**接上，再判红绿。在那之前，**如实报「未执行」**，不装作通过。
 *
 * @returns {{name: string, status: string, detail: string}}
 */
function checkChannelReachability() {
  return verdict(
    'A0-channel-reachability',
    SKIPPED,
    '未执行：适配器当前把 `connection.rpc.call` 直连 host handler，绕过了内核 ' +
      '`connection` 服务的真实路由注册（p1-adapter.mjs:236-242）→ 判据看不到真机 ' +
      '「cannot get property \"webServer\" without inject」（host/index.js:486）那个真洞。' +
      '通道形态定案后按真实装配口径接上再判。',
  );
}

/**
 * 跑仓库级隔离门禁（协议 §8-11：`probe-profile-isolation` / `probe-data-directory` 仍绿）。
 *
 * **判据自身的两个坑（已修）**：
 *   1. 门禁输出里**没有** `probe-profile-isolation` / `probe-data-directory`
 *      这些分区名——它们是**失败分区名**，成功时打的是中文文案（`✓ 测试/探针代码
 *      没有指向用户真实数据目录下的浏览器 profile（已扫 N 个文件）`）。原来按
 *      分区名去 grep，**永远找不到** → 永远走「未找到但退出码为 0」的 PASS 分支
 *      ⇒ 这条判据**从来没真正验证过那两行**，是个假绿。
 *   2. 现在按**稳定的中文文案片段**匹配，且**找不到就报「未执行」**而不是通过
 *      ——「没测到」与「测了没过」必须能分开。
 *
 * @returns {{name: string, status: string, detail: string}}
 */
function checkRepoIsolationGate() {
  try {
    const out = execFileSync(process.execPath, [join(REPO, 'scripts', 'check-invariants.mjs')], {
      cwd: REPO,
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'pipe'],
      maxBuffer: 64 * 1024 * 1024,
    });
    // 判据 30 与判据 31 的成功文案片段（两个都要找到才算这条判据真的跑到了）。
    const markers = [
      '没有指向用户真实数据目录下的浏览器 profile',
      '都无条件给了 data_directory',
    ];
    const found = markers.filter((marker) => out.includes(marker));
    if (found.length < markers.length) {
      return verdict(
        'A11b-repo-isolation-gate',
        SKIPPED,
        `门禁输出里只找到 ${found.length}/${markers.length} 条判据 30/31 的成功文案` +
          '（输出格式可能变了）——本条**未执行**，不判通过',
      );
    }
    const lines = out
      .split('\n')
      .filter((line) => markers.some((marker) => line.includes(marker)))
      .map((l) => l.trim().replace(/^✓\s*/, ''));
    return verdict('A11b-repo-isolation-gate', PASS, `仓库级门禁绿：${lines.join('；')}`);
  } catch (error) {
    const output = `${error?.stdout ?? ''}${error?.stderr ?? ''}`.trim();
    return verdict('A11b-repo-isolation-gate', FAIL, `仓库级门禁红了：${output.slice(0, 800)}`);
  }
}

/**
 * JS 端跑同一组夹具（协议 §6 的「各跑一遍」之一）。
 *
 * @param {Array<object>} fixtures
 * @returns {Promise<Array<{id: string, result: unknown}>>}
 */
async function runJsFixtures(fixtures) {
  const protocol = await import(
    new URL('../plugins/official-chat/host/protocol.js', import.meta.url).href
  );
  if (typeof protocol.resolveCase !== 'function') {
    return undefined;
  }
  return fixtures.map((fixture) => ({
    id: String(fixture?.id ?? ''),
    result: protocol.resolveCase(fixture?.input ?? {}),
  }));
}

/**
 * Rust 端跑同一组夹具（`src-tauri/examples/p1_protocol_fixture.rs` 的产物）。
 *
 * @returns {Array<{id: string, result: unknown}>|undefined}
 */
function runRustFixtures() {
  const exe = process.platform === 'win32' ? 'p1_protocol_fixture.exe' : 'p1_protocol_fixture';
  const bin = join(REPO, 'src-tauri', 'target', 'debug', 'examples', exe);
  if (!existsSync(bin)) return undefined;
  try {
    const stdout = execFileSync(bin, [], {
      cwd: REPO,
      encoding: 'utf8',
      maxBuffer: 64 * 1024 * 1024,
    });
    const parsed = JSON.parse(stdout.trim());
    return Array.isArray(parsed?.cases) ? parsed.cases : undefined;
  } catch {
    return undefined;
  }
}

async function main() {
  const args = process.argv.slice(2);
  const roundIndex = args.indexOf('--round');
  const round = roundIndex >= 0 ? args[roundIndex + 1] : `r${Date.now().toString(36)}`;
  const outDir = join(tmpdir(), 'dsh-p1-protocol');
  mkdirSync(outDir, { recursive: true });

  const results = [];

  // —— 验收项 1：协议夹具 ——
  const fixtures = readFixtures();
  // **先报「没测到的」**：通道可达性当前不可机检，放最前面，别让它藏在绿里。
  results.push(checkChannelReachability());
  results.push(checkFixtureCoverage(fixtures));
  results.push(checkFixtureExpectations(fixtures));
  const jsFixtures = await runJsFixtures(fixtures);
  const rustFixtures = runRustFixtures();
  results.push(checkFixtureParity({ js: jsFixtures, rust: rustFixtures }));
  // 夹具的 `expected` 就是契约：实现算出来的结果必须与它一致。
  // **两端各判一条、名字必须带端标识**——同名两条会让验收人分不清是哪端不一致。
  results.push(
    checkFixtureExpectationMatch(
      {
        expected: fixtures.map((fixture) => ({ id: String(fixture?.id ?? ''), result: fixture?.expected })),
        actual: jsFixtures,
      },
      'A1d-fixture-expectation-match-js',
    ),
  );
  results.push(
    checkFixtureExpectationMatch(
      {
        expected: fixtures.map((fixture) => ({ id: String(fixture?.id ?? ''), result: fixture?.expected })),
        actual: rustFixtures,
      },
      'A1d-fixture-expectation-match-rust',
    ),
  );

  // —— 判据 11：隔离面（本次新增文件）+ 仓库级门禁 ——
  const newFiles = NEW_FILES.filter((file) => existsSync(join(REPO, file))).map((file) => ({
    file,
    text: readFileSync(join(REPO, file), 'utf8'),
  }));
  results.push(
    checkIsolationSurface({
      files: newFiles,
      findProfileViolationsInFiles,
      findDataDirectoryViolationsInFiles,
    }),
  );
  results.push(checkRepoIsolationGate());

  // —— 行为判据：对真实实现跑 ——
  //
  // **客户端模块加载失败时**（实现正在并行改动、处于中间态），
  // 行为判据一律报「未执行」并写明原因——**不能整轮崩掉**：
  // 一崩就把静态判据的结果也一起盖住了，验收人就看不到别的了。
  const BEHAVIOUR_CHECKS = [
    ['A9c-error-no-internal-leak', checkErrorNoInternalLeak],
    ['A2-pick-twice-distinct', checkPickTwiceDistinct],
    ['A3-ref-length-boundary', checkRefLengthBoundary],
    ['A4-single-source-of-truth', checkSingleSourceOfTruth],
    ['A5-request-matches-disk', checkRequestMatchesDisk],
    ['A6a-invalid-reject', checkInvalidRejection],
    ['A6b-chip-invalid-node', checkChipInvalidNode],
    ['A7-cross-process', checkCrossProcess],
    ['A8-no-body-dedup', checkNoBodyDedup],
    ['A10-namespace-isolation', checkNamespaceIsolation],
  ];

  let adapter = null;
  let adapterError = '';
  try {
    adapter = await createAdapter({ tag: `run-${round}` });
  } catch (error) {
    adapterError = String(error?.message ?? error);
  }

  if (adapter === null) {
    for (const [name] of BEHAVIOUR_CHECKS) {
      results.push(
        verdict(name, SKIPPED, `客户端模块加载失败，本条未执行：${adapterError}`),
      );
    }
  } else {
    try {
      const capabilities = adapter.capabilities();
      results.push(checkErrorCodeTable(capabilities));
      results.push(checkCatalogCodesSeparated(capabilities));
      results.push(checkConstants(capabilities));
      results.push(checkGapRegistry(capabilities));

      for (const [name, check] of BEHAVIOUR_CHECKS) {
        results.push(await check(adapter));
      }
    } finally {
      adapter.cleanup();
    }
  }

  const summary = summarize(results);
  const payload = {
    round,
    at: new Date().toISOString(),
    fixtureDir: FIXTURE_DIR,
    results,
    summary,
  };
  const outPath = join(outDir, `results-${round}.json`);
  writeFileSync(outPath, `${JSON.stringify(payload, null, 2)}\n`, 'utf8');

  for (const item of results) {
    const mark = item.status === PASS ? '✔' : item.status === FAIL ? '✖' : '–';
    process.stdout.write(`${mark} [${item.status}] ${item.name}\n    ${item.detail}\n`);
  }
  process.stdout.write(
    `\nsummary: total=${summary.total} pass=${summary.pass} fail=${summary.fail} skipped=${summary.skipped}\n` +
      `证据：${outPath}\n`,
  );
  process.exitCode = summary.fail > 0 ? 1 : 0;
}

/**
 * Rust 端跑同一组夹具的结果。
 *
 * @returns {unknown|undefined}
 */
function readRustFixtureResult() {
  return runRustFixtures();
}

await main();
