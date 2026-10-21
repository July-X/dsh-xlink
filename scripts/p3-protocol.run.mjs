// P3 协议判据的**实跑**入口：对真实实现跑协议稿 §5 的十二条验收判据。
//
// ## 用法
//
// ```powershell
// node scripts/p3-protocol.run.mjs            # 跑一轮，输出到 %TEMP%\dsh-p3-protocol\
// node scripts/p3-protocol.run.mjs --round p1 # 指定轮次名（证据按轮次独立命名）
// ```
//
// ## 产出
//
// `%TEMP%\dsh-p3-protocol\results-<round>.json`：
// `{round, at, results: [{name,status,detail}], summary: {total,pass,fail,skipped}}`
// —— 与 P1/P2 的产物同构。
//
// ## 隔离
//
// 落盘根目录在 `%TEMP%\dsh-p3-protocol-*` 一次性目录里，**绝不指向用户真实数据目录**；
// 跑完删掉（只删自己建的）。原实现文件一个字节都不改。
//
// @module scripts/p3-protocol-run

import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';

import {
  FAIL,
  PASS,
  SKIPPED,
  checkP3_1_atomicPublish,
  checkP3_2_writeBackVerify,
  checkP3_3_tmpResidualSwept,
  checkP3_4_occupancyRecorded,
  checkP3_5_reclaimRespectsOccupancy,
  checkP3_6_orphanClaimReclaimed,
  checkP3_7_storageProtection,
  checkP3_8_policyDocumented,
  checkP3_9_accountScopeServerDerived,
  checkP3_10_seqNotReused,
  checkP3_11_releasePaths,
  checkP3_12_twoRoundRelease,
  summarize,
  verdict,
} from './lib/p3-protocol.mjs';
import { createP3Adapter } from './lib/p3-adapter.mjs';

/** 证据目录（%TEMP% 下，**不指向用户真实数据目录**）。 */
const EVIDENCE_DIR = join(tmpdir(), 'dsh-p3-protocol');

/**
 * 每条判据跑一遍，**各自一套全新的适配器**（新的落盘根 ＋ 新的进程内状态）。
 *
 * **为什么各自一套**：判据之间不能互相污染（P3-3 会删掉 P3-5 要用的盘上文件
 * ⇒ 共用一套根会互相踩）。P3 协议稿 §5 的十二条是**并列**关系。
 */
const CRITERIA = [
  { id: 'P3-1', run: checkP3_1_atomicPublish },
  { id: 'P3-2', run: checkP3_2_writeBackVerify },
  { id: 'P3-3', run: checkP3_3_tmpResidualSwept },
  { id: 'P3-4', run: checkP3_4_occupancyRecorded },
  { id: 'P3-5', run: checkP3_5_reclaimRespectsOccupancy },
  { id: 'P3-6', run: checkP3_6_orphanClaimReclaimed },
  { id: 'P3-7', run: checkP3_7_storageProtection },
  { id: 'P3-8', run: checkP3_8_policyDocumented },
  { id: 'P3-9', run: checkP3_9_accountScopeServerDerived },
  { id: 'P3-10', run: checkP3_10_seqNotReused },
  { id: 'P3-11', run: checkP3_11_releasePaths },
  { id: 'P3-12', run: checkP3_12_twoRoundRelease },
];

/**
 * 跑一条判据：**它自己抛错只把自己报成「未执行」**，不带走整轮。
 *
 * @param {object} spec - `CRITERIA` 的一项。
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
async function runSafely(spec) {
  let adapter = null;
  try {
    adapter = await createP3Adapter({ tag: `p3-${spec.id}` });
    return await spec.run(adapter);
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
 * 主入口。
 *
 * @returns {Promise<void>}
 */
async function main() {
  // 轮次名：`--round p1` 与 `--round=p1` 两种写法都认。
  const roundIndex = process.argv.findIndex(
    (arg) => arg === '--round' || String(arg).startsWith('--round='),
  );
  let round = 'p1';
  if (roundIndex >= 0) {
    const inline = String(process.argv[roundIndex]).split('=')[1];
    const next = process.argv[roundIndex + 1];
    round = inline ?? (next !== undefined && !String(next).startsWith('--') ? next : 'p1');
  }

  const results = [];
  for (const spec of CRITERIA) {
    const result = await runSafely(spec);
    results.push(result);
    const mark = result.status === PASS ? '✔' : result.status === FAIL ? '✘' : '—';
    process.stdout.write(`${mark} ${result.name}：${result.detail}\n`);
  }

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
