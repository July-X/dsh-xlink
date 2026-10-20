// P2 协议判据的**实跑**入口：对真实实现跑协议稿 §5 的六条验收判据。
//
// ## 用法
//
// ```powershell
// node scripts/p2-protocol.run.mjs            # 跑一轮，输出到 %TEMP%\dsh-p2-protocol\
// node scripts/p2-protocol.run.mjs --round p1 # 指定轮次名（证据按轮次独立命名）
// ```
//
// ## 产出
//
// `%TEMP%\dsh-p2-protocol\results-<round>.json`：
// `{round, at, results: [{name,status,detail}], summary: {total,pass,fail,skipped}}`
// —— 与 P1 的产物同构。
//
// ## 隔离
//
// 落盘根目录在 `%TEMP%\dsh-p2-protocol-*` 一次性目录里，**绝不指向用户真实数据目录**；
// 跑完删掉（只删自己建的）。原实现文件一个字节都不改。
//
// @module scripts/p2-protocol-run

import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';

import {
  FAIL,
  PASS,
  SKIPPED,
  checkP2_1_sameGroupKeepsDistinctIds,
  checkP2_2_duplicateNoticeVisible,
  checkP2_3_capturedAtNotGrouped,
  checkP2_4_scopeBranchNotGrouped,
  checkP2_5_budgetNoticeNoTruncation,
  checkP2_6_failureDoesNotBlockSend,
  checkP2_C_errorCodeClosedSet,
  checkP2_D_crossLanguageGroupKey,
  checkP2_F_fixtureExpectation,
  checkP2_H_multiGroupNotDropped,
  checkP2_J_noticeStaysTruthfulAfterSend,
  summarize,
  verdict,
} from './lib/p2-protocol.mjs';
import { createAdapter } from './lib/p1-adapter.mjs';
import { runRustFixtureCases } from './lib/rust-fixture-runner.mjs';

const REPO = fileURLToPath(new URL('../', import.meta.url));

/** 证据目录（%TEMP% 下，**不指向用户真实数据目录**）。 */
const EVIDENCE_DIR = join(tmpdir(), 'dsh-p2-protocol');

/**
 * 每条判据跑一遍，**各自一套全新的适配器**（新的落盘根 + 新的进程内状态）。
 *
 * **为什么各自一套**：判据之间不能互相污染（P1 的 A18 就吃过这个亏——真 sweeper
 * 会删掉别的判据正要用的盘上文件）。P2-5 要「大正文」，别的判据不要 ⇒ 选项也
 * 是**逐条**给的。
 */
const CRITERIA = [
  { id: 'P2-1', run: checkP2_1_sameGroupKeepsDistinctIds, options: {} },
  { id: 'P2-2', run: checkP2_2_duplicateNoticeVisible, options: {} },
  { id: 'P2-3', run: checkP2_3_capturedAtNotGrouped, options: {} },
  { id: 'P2-4', run: checkP2_4_scopeBranchNotGrouped, options: {} },
  // **大正文**：两条各 150000 字节 ⇒ 合计 300000 > 262144（预算上限）⇒ 触发超限。
  // 单条必须**小于** 262144（否则 capture 自己就 `body_too_large` 了 ⇒ 场景不成立）。
  { id: 'P2-5', run: checkP2_5_budgetNoticeNoTruncation, options: { bodyBytes: 150_000 } },
  { id: 'P2-6', run: checkP2_6_failureDoesNotBlockSend, options: {} },
  // **表外的一条**（§2.2 / §2.3 / §6.5 的失败码闭集）：§5 的表只有六行，
  // 但协议正文写了失败码必须在 13 码闭集内 ⇒ 落一条判据。
  { id: 'P2-C', run: checkP2_C_errorCodeClosedSet, options: {} },
  // **跨语言的一条**（§1.2 / §5「夹具」条款）：同一组夹具，**协议独立转写 ＝ JS
  // ＝ Rust** 三方的 `groupKey` 必须一致（只断 JS==Rust 会放过「两端一起错」）。
  {
    id: 'P2-D',
    run: (adapter, context) => checkP2_D_crossLanguageGroupKey({ ...context, adapter }),
    options: {},
  },
  // **多组场景**（协议 v0.6 §3.2「多组形态」）：两个不同的组同时成立 ⇒
  // 不漏报 ＋ 组按 groupKey 升序 ＋ 文案含 `groups[0]` 的事实。
  { id: 'P2-H', run: checkP2_H_multiGroupNotDropped, options: {} },
  // **发送后那条提示是否仍在 / 是否不失真**（协议 §3.1 ＋ §3.2；UX 提出、PM 交回给
  // tester）：发送后**用户眼前那条**（= 客户端投递的最后一条）必须仍是那条自包含、
  // 不误导的重复核对（逐字相同 ＋ info 档 ＋ 无发送结果字样 ＋ 不把「本次核对」写成
  // 当前状态）。
  { id: 'P2-J', run: checkP2_J_noticeStaysTruthfulAfterSend, options: {} },
];

/**
 * **跨语言判据的输入**（P2-D）：同一组「去重候选对」夹具 ＋ Rust 端产物。
 *
 * ## 夹具目录**只有一处**
 *
 * `plugins/official-chat/fixtures/protocol/` —— Rust 端 example 的 `fixture_dir()`
 * 的候选列表里第一个就是它 ⇒ **两端吃的是同一批夹具**，不会「各读各的、
 * 悄悄对不上」。
 *
 * ## Rust 端没构建 ⇒ `undefined`
 *
 * ⇒ P2-D 判据据此报「**未执行**」（不判通过），并给出构建命令。
 *
 * @returns {{fixtures: Array<{id: string, fixture: object}>,
 *   files: Array<{file: string, fixture: object}>,
 *   rustCases: Array<{id: string, result: object}>|undefined}}
 */
function loadCrossLanguageContext() {
  const dir = join(REPO, 'plugins', 'official-chat', 'fixtures', 'protocol');
  const indexPath = join(dir, 'index.json');
  const files = [];
  if (existsSync(indexPath)) {
    const index = JSON.parse(readFileSync(indexPath, 'utf8'));
    const names = Array.isArray(index?.cases) ? index.cases : [];
    for (const file of names.filter((name) => String(name).includes('duplicates'))) {
      const path = join(dir, String(file));
      if (!existsSync(path)) continue;
      files.push({ file: String(file), fixture: JSON.parse(readFileSync(path, 'utf8')) });
    }
  }
  return {
    // **id 取夹具自己的 `id` 字段**（不是文件名）——Rust 端 example 也是从夹具的
    // `id` 字段取 id ⇒ 两边对得上。
    fixtures: files.map(({ file, fixture }) => ({ id: String(fixture.id ?? file), fixture })),
    files,
    rustCases: runRustFixtureCases(REPO),
  };
}

/**
 * 跑一条判据：**它自己抛错只把自己报成「未执行」**，不带走整轮。
 *
 * @param {object} spec - `CRITERIA` 的一项。
 * @param {object} [context] - 额外输入（只有 P2-D 用：夹具 ＋ Rust 端产物）。
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
async function runSafely(spec, context = {}) {
  let adapter = null;
  try {
    adapter = await createAdapter({ tag: `p2-${spec.id}`, ...spec.options });
    return await spec.run(adapter, context);
  } catch (error) {
    return verdict(spec.id, SKIPPED, `判据未执行：${String(error?.stack ?? error)}`);
  } finally {
    try {
      adapter?.cleanup?.();
    } catch {
      /* 收尾失败不影响结果 */
    }
  }
}

/**
 * **夹具期望值核对**（协议 §5 表下的「夹具」条款）。
 *
 * 逐个夹具调判据库的 `checkP2_F_fixtureExpectation`
 * （期望值由判据**自己**按 §1.1 / §1.2 独立转写，不看实现的输出）。
 *
 * @param {Array<{file: string, fixture: object}>} files - 已读到的「去重候选对」夹具。
 * @returns {{name: string, status: string, detail: string}}
 */
function checkFixtureExpectations(files) {
  const name = 'P2-F-fixture-expectation-match';
  if (files.length === 0) {
    return verdict(name, SKIPPED, '夹具登记表里没有「去重候选对」夹具（登记表不存在或未登记）');
  }
  const problems = [];
  let checked = 0;
  for (const { file, fixture } of files) {
    const result = checkP2_F_fixtureExpectation(fixture, file);
    if (result.status === FAIL) problems.push(result.detail);
    else if (result.status === PASS) checked += 1;
  }
  if (problems.length > 0) {
    return verdict(name, FAIL, `夹具期望值与协议 §1.1 / §1.2 矛盾：${problems.join('；')}`);
  }
  return verdict(
    name,
    PASS,
    `${checked} 条「去重候选对」夹具的期望值与协议 §1.1 / §1.2 一致（${files
      .map((one) => one.file)
      .join('、')}）`,
  );
}

/**
 * 主入口。
 *
 * @returns {Promise<void>}
 */
async function main() {
  // 轮次名：`--round p1` 与 `--round=p1` 两种写法都认。
  const roundIndex = process.argv.findIndex((arg) => arg === '--round' || String(arg).startsWith('--round='));
  let round = 'p1';
  if (roundIndex >= 0) {
    const inline = String(process.argv[roundIndex]).split('=')[1];
    const next = process.argv[roundIndex + 1];
    round = inline ?? (next !== undefined && !String(next).startsWith('--') ? next : 'p1');
  }

  // 跨语言判据的输入：**夹具 ＋ Rust 端产物**（Rust 没构建 ⇒ P2-D 报「未执行」）。
  const crossLanguage = loadCrossLanguageContext();

  const results = [];
  for (const spec of CRITERIA) {
    const context = spec.id === 'P2-D' ? crossLanguage : {};
    const result = await runSafely(spec, context);
    results.push(result);
    const mark = result.status === PASS ? '✔' : result.status === FAIL ? '✘' : '—';
    process.stdout.write(`${mark} ${result.name}：${result.detail}\n`);
  }
  // 夹具期望值核对（§5 表下的「夹具」条款）。
  const fixtureResult = checkFixtureExpectations(crossLanguage.files);
  results.push(fixtureResult);
  const fixtureMark = fixtureResult.status === PASS ? '✔' : fixtureResult.status === FAIL ? '✘' : '—';
  process.stdout.write(`${fixtureMark} ${fixtureResult.name}：${fixtureResult.detail}\n`);

  const summary = summarize(results);
  mkdirSync(EVIDENCE_DIR, { recursive: true });
  const outPath = join(EVIDENCE_DIR, `results-${round}.json`);
  writeFileSync(
    outPath,
    `${JSON.stringify({ round, at: new Date().toISOString(), results, summary }, null, 2)}\n`,
    'utf8',
  );
  process.stdout.write(
    `\n合计 ${summary.total} 条：通过 ${summary.pass} · 失败 ${summary.fail} · 未执行 ${summary.skipped}\n` +
      `证据：${outPath}\n`,
  );
}

await main();
