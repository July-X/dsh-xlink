// P4 判据**「源码形态」那一族**的共用机检原语（第 1／3／4／5／7 条）。
//
// ## 为什么有这一层
//
// 协议 §10 的第 1／3／4／5／7 条都写着「**按源码形态断**」⇒ 它们看的是
// **实现长什么样**，不是运行时读数 ⇒ 五条判据要读的是**同一片实现**、用**同一套**
// 「读文件 → 抽形态 → 判 → 场景自证 → 落证据 → 打结论」的动作。
//
// ⇒ 那些**每条判据都一样**的部分放在这里，**每条判据自己的口径**留在各自的
// 脚本里（`judge` / `buildScenarios` / `readReading`）。
//
// ## 纪律
//
// · **只读**：不写仓库、不写用户目录、不发网络请求；证据只落 `%TEMP%`。
// · **三态**：`pass` / `fail` / `skipped` —— 「没测到」与「测了没过」必须长得不一样。
// · **退出码按结论算**（沿 P1 判据的口径）：`summary.fail > 0` ⇒ 退出码 1。
// · **判据不能被被测物定义**：期望值抄自**协议**，不抄自实现。
//
// @module scripts/lib/p4-source-shape

import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

/** verdict 三态。 */
export const PASS = 'pass';
export const FAIL = 'fail';
export const SKIPPED = 'skipped';

/**
 * P4 实现的**扫描面**（相对仓库根）。
 *
 * ⚠️ **只列 P4 这一单动的文件** —— `src-tauri/src/official_chat/protocol.rs` 是 P1
 * 的资产（判据第 6 条管它），`src-tauri/examples/**` 是取证探针，**都不在这片面里**。
 *
 * ⚠️ **v0.10 之后这里没有页内脚本了** —— 取数形态是**壳侧 DevTools `Network` 域
 * 订阅**（协议 §1 裁定 1 / §2.3：数据通路上**没有脚本**）⇒ ⇒ 扫描面里**没有**
 * `session_structure_capture.js`。
 */
export const P4_SCAN_FILES = Object.freeze([
  'src-tauri/src/official_chat/session_structure.rs',
  'src-tauri/src/official_chat/cdp_eval.rs',
  'src-tauri/src/official_chat/mod.rs',
  'src-tauri/src/plugins/builtin/bridge.rs',
  'src-tauri/src/openai/http.rs',
  'plugins/official-chat/host/site-source.js',
  'plugins/official-chat/host/commands.js',
  'plugins/official-chat/host/rpc.js',
  'plugins/official-chat/host/index.js',
]);

/** 站点自己的会话目录端点（协议 §4.1）—— 「页外直连站点 API」那一条要认它。 */
export const SITE_API_PATH = '/api/v0/';

/**
 * 去掉**行注释**与**块注释**（**保留字符串字面量**）。
 *
 * @param {string} src - 源码文本。
 * @returns {string[]} 清理后的行（行号与原文对齐）。
 */
export function stripComments(src) {
  const lines = String(src ?? '').split('\n');
  const out = [];
  let inBlock = false;
  for (const raw of lines) {
    let line = raw;
    if (inBlock) {
      const end = line.indexOf('*/');
      if (end === -1) {
        out.push('');
        continue;
      }
      line = line.slice(end + 2);
      inBlock = false;
    }
    for (let guard = 0; guard < 32; guard += 1) {
      const b = line.indexOf('/*');
      if (b === -1) break;
      const e = line.indexOf('*/', b + 2);
      if (e === -1) {
        line = line.slice(0, b);
        inBlock = true;
        break;
      }
      line = line.slice(0, b) + line.slice(e + 2);
    }
    let quote = null;
    let cut = -1;
    for (let i = 0; i < line.length; i += 1) {
      const ch = line[i];
      if (quote) {
        if (ch === '\\') {
          i += 1;
          continue;
        }
        if (ch === quote) quote = null;
        continue;
      }
      if (ch === '"' || ch === "'" || ch === '`') {
        quote = ch;
        continue;
      }
      if (ch === '/' && line[i + 1] === '/') {
        cut = i;
        break;
      }
    }
    out.push(cut === -1 ? line : line.slice(0, cut));
  }
  return out;
}

/**
 * 读扫描面里的文件（**只读**；读不动的跳过）。
 *
 * @param {string} root - 仓库根。
 * @param {string[]} relPaths - 相对路径。
 * @returns {Array<{path: string, code: string}>}
 */
export function readSources(root, relPaths) {
  const out = [];
  for (const rel of relPaths) {
    try {
      out.push({ path: rel, code: readFileSync(join(root, rel), 'utf8') });
    } catch {
      /* 读不动的文件跳过（判据会因此报「没测到」而不是「测了没过」） */
    }
  }
  return out;
}

/**
 * 抽一个**对象字面量**的顶层键名。
 *
 * @param {string} src - 源码文本。
 * @param {string} anchor - 定位用的锚点（字面量就在它后面）。
 * @returns {string[]} 顶层键名（抽不到 ⇒ 空数组）。
 */
export function objectLiteralKeys(src, anchor) {
  const text = String(src ?? '');
  const at = text.indexOf(anchor);
  if (at < 0) return [];
  const start = text.indexOf('{', at + anchor.length - 1);
  if (start < 0) return [];
  let depth = 0;
  let quote = null;
  for (let i = start; i < text.length; i += 1) {
    const ch = text[i];
    if (quote) {
      if (ch === '\\') {
        i += 1;
        continue;
      }
      if (ch === quote) quote = null;
      continue;
    }
    if (ch === '"' || ch === "'" || ch === '`') {
      quote = ch;
      continue;
    }
    if (ch === '{') {
      depth += 1;
      continue;
    }
    if (ch === '}') {
      depth -= 1;
      if (depth === 0) {
        const body = text.slice(start + 1, i);
        return topLevelKeysOf(body);
      }
    }
  }
  return [];
}

/**
 * 抽一段**对象字面量正文**的顶层键名（引号或裸键都认）。
 *
 * @param {string} body - 字面量正文（**不含**最外层的大括号）。
 * @returns {string[]}
 */
export function topLevelKeysOf(body) {
  const keys = [];
  let depth = 0;
  let quote = null;
  let segment = '';
  const segments = [];
  for (let i = 0; i < body.length; i += 1) {
    const ch = body[i];
    if (quote) {
      if (ch === '\\') {
        segment += ch;
        i += 1;
        segment += body[i] ?? '';
        continue;
      }
      if (ch === quote) {
        quote = null;
        segment += ch;
        continue;
      }
      segment += ch;
      continue;
    }
    if (ch === '"' || ch === "'" || ch === '`') {
      quote = ch;
      segment += ch;
      continue;
    }
    if (ch === '{' || ch === '[') {
      depth += 1;
      segment += ch;
      continue;
    }
    if (ch === '}' || ch === ']') {
      depth -= 1;
      segment += ch;
      continue;
    }
    if (ch === ',' && depth === 0) {
      segments.push(segment);
      segment = '';
      continue;
    }
    segment += ch;
  }
  segments.push(segment);
  for (const seg of segments) {
    const text = seg.trim();
    if (text === '') continue;
    const m = /^["'`]([A-Za-z_$][A-Za-z0-9_$]*)["'`]/.exec(text);
    if (m) {
      keys.push(m[1]);
      continue;
    }
    const bare = /^([A-Za-z_$][A-Za-z0-9_$]*)\s*:/.exec(text);
    if (bare) keys.push(bare[1]);
  }
  return keys;
}

/**
 * 抽「**含某个标记的那个**对象字面量」的顶层键名（取**最内层**的那个）。
 *
 * ## 为什么要有它
 *
 * 回程载荷那段对象字面量**不是**「函数里第一个 `{`」——函数体本身也是个 `{`。
 * ⇒ ⇒ 用**标记**（载荷里的特征键，如 `topKeys`）定位，再取**最短**的那一层
 * （最内层）⇒ 拿到的就是载荷本身。
 *
 * @param {string} src - 源码文本。
 * @param {string} marker - 标记（载荷里的特征键名）。
 * @returns {string[]} 顶层键名（抽不到 ⇒ 空数组）。
 */
export function objectLiteralKeysContaining(src, marker) {
  const text = String(src ?? '');
  let best = null;
  for (let i = 0; i < text.length; i += 1) {
    if (text[i] !== '{') continue;
    const end = matchBrace(text, i);
    if (end < 0) continue;
    const body = text.slice(i + 1, end);
    if (!body.includes(marker)) continue;
    if (best === null || end - i < best.end - best.start) {
      best = { start: i, end, body };
    }
  }
  return best === null ? [] : topLevelKeysOf(best.body);
}

/**
 * 从 `{` 出发配对到对应的 `}`（**跳过字符串字面量**）。
 *
 * @param {string} text - 源码文本。
 * @param {number} start - `{` 的下标。
 * @returns {number} `}` 的下标（配不上 ⇒ -1）。
 */
export function matchBrace(text, start) {
  let depth = 0;
  let quote = null;
  for (let i = start; i < text.length; i += 1) {
    const ch = text[i];
    if (quote) {
      if (ch === '\\') {
        i += 1;
        continue;
      }
      if (ch === quote) quote = null;
      continue;
    }
    if (ch === '"' || ch === "'" || ch === '`') {
      quote = ch;
      continue;
    }
    if (ch === '{') {
      depth += 1;
      continue;
    }
    if (ch === '}') {
      depth -= 1;
      if (depth === 0) return i;
    }
  }
  return -1;
}

/**
 * 抽一个**具名函数**的函数体（Rust 的 `fn name(...) { … }`）。
 *
 * ## 为什么要有它
 *
 * 判据第 10／11 条要断的是**调用点之间的关系**（「取体在等 loadingFinished 之后」
 * 「enable 在导航之前」）⇒ ⇒ 得能按函数切开源码。
 *
 * @param {string} src - 源码文本。
 * @param {string} fnName - 函数名。
 * @returns {string} 函数体（抽不到 ⇒ 空串）。
 */
export function fnBody(src, fnName) {
  const text = String(src ?? '');
  // ⚠️ **要按函数名精确匹配**（后面跟 `(` 或 `<`）—— 否则 `fn capture` 会命中
  // `fn capture_in`（前缀相同）⇒ 抽出来的函数体是另一个函数的。
  const found = new RegExp(`fn\\s+${fnName}\\s*[<(]`).exec(text);
  if (!found) return '';
  const start = text.indexOf('{', found.index);
  if (start < 0) return '';
  const end = matchBrace(text, start);
  return end < 0 ? '' : text.slice(start, end + 1);
}

/**
 * 一个片段**第一次出现**的行号（1 起；找不到 ⇒ -1）。
 *
 * @param {string} src - 源码文本。
 * @param {string} needle - 要找的片段。
 * @returns {number} 行号。
 */
export function lineOf(src, needle) {
  const lines = String(src ?? '').split('\n');
  for (let i = 0; i < lines.length; i += 1) {
    if (lines[i].includes(needle)) return i + 1;
  }
  return -1;
}

/**
 * 汇总计数（与 P1／P2／P3 判据的产物同构）。
 *
 * @param {Array<{status: string}>} results
 * @returns {{total: number, pass: number, fail: number, skipped: number}}
 */
export function summarize(results) {
  const list = Array.isArray(results) ? results : [];
  return {
    total: list.length,
    pass: list.filter((r) => r?.status === PASS).length,
    fail: list.filter((r) => r?.status === FAIL).length,
    skipped: list.filter((r) => r?.status === SKIPPED).length,
  };
}

/**
 * 把一组断言判成「过／不过」。
 *
 * @param {Array<{name: string, status: string, detail: string}>} checks
 * @returns {{ok: boolean, checks: Array<object>}}
 */
export function judgeChecks(checks) {
  const judged = checks.filter((c) => c.status !== SKIPPED);
  return { ok: judged.length > 0 && judged.every((c) => c.status === PASS), checks };
}

/**
 * 跑场景表：核「实际判定」是否等于「应当的判定」。
 *
 * @param {Array<{id: string, name: string, expect: string, reading: object}>} scenarios
 * @param {Function} judge - 判定内核。
 * @param {string} label - 结果名前缀。
 * @returns {Array<{name: string, status: string, detail: string}>}
 */
export function runScenarios(scenarios, judge, label) {
  return scenarios.map((sc) => {
    const { ok, checks } = judge(sc.reading);
    // **全判「未执行」的读数** ＝ 「没测到」⇒ 结果行也是「未执行」，
    // **不是**「失败」（三态的规矩：没测到 ≠ 测了没过）。
    const allSkipped = checks.length > 0 && checks.every((c) => c.status === SKIPPED);
    if (allSkipped) {
      return {
        name: `${label} ${sc.name}`,
        status: SKIPPED,
        detail: `实际判定＝未执行（期望＝${sc.expect === SKIPPED ? '未执行' : '通过/失败'}）｜读数缺失`,
        checks,
      };
    }
    const behavedAsExpected =
      sc.expect === SKIPPED ? false : sc.expect === PASS ? ok : !ok;
    const bad = checks.filter((c) => c.status === FAIL).map((c) => c.name);
    const skipped = checks.filter((c) => c.status === SKIPPED).map((c) => c.name);
    return {
      name: `${label} ${sc.name}`,
      status: behavedAsExpected ? PASS : FAIL,
      detail:
        `实际判定＝${ok ? '通过' : '失败'}（期望＝${sc.expect === PASS ? '通过' : '失败'}）` +
        (bad.length ? `｜未过的断言：${bad.join('、')}` : '｜可机检的断言全过') +
        (skipped.length ? `｜未执行：${skipped.join('、')}` : ''),
      checks,
    };
  });
}

/**
 * 取一个命令行参数（`--name 值` 或 `--name=值`）。
 *
 * @param {string[]} argv - 参数表。
 * @param {string} name - 参数名。
 * @returns {string|null}
 */
export function argValue(argv, name) {
  const list = Array.isArray(argv) ? argv : [];
  const index = list.findIndex((a) => a === name || String(a).startsWith(`${name}=`));
  if (index < 0) return null;
  const inline = String(list[index]).split('=')[1];
  if (inline !== undefined) return inline;
  const next = list[index + 1];
  return next !== undefined && !String(next).startsWith('--') ? next : null;
}

/**
 * 仓库根（脚本在 `scripts/` 下 ⇒ 上一级）。
 *
 * @param {string} moduleUrl - `import.meta.url`。
 * @returns {string}
 */
export function repoRoot(moduleUrl) {
  return fileURLToPath(new URL('../', moduleUrl)).replace(/[\\/]+$/, '');
}

/**
 * 落证据 ＋ 打结论 ＋ 设退出码。
 *
 * @param {object} options
 * @param {string} options.round - 轮次名（进证据文件名）。
 * @param {Array<{path: string, code: string}>} options.sources - 扫描面。
 * @param {string} options.scanLabel - 扫描面的人话描述。
 * @param {object} options.reading - 读数（进证据）。
 * @param {Array<object>} options.results - 结果行。
 * @param {string} options.evidenceDir - 证据目录（`%TEMP%` 下）。
 * @param {boolean} options.asJson - 只打 JSON。
 * @returns {{results: Array<object>, summary: object, outPath: string}}
 */
export function emit({ round, sources, scanLabel, reading, results, evidenceDir, asJson }) {
  const summary = summarize(results);
  mkdirSync(evidenceDir, { recursive: true });
  const outPath = join(evidenceDir, `results-${round}.json`);
  writeFileSync(
    outPath,
    `${JSON.stringify(
      { round, at: new Date().toISOString(), scanned: sources.map((s) => s.path), reading, results, summary },
      null,
      2,
    )}\n`,
    'utf8',
  );
  if (asJson) {
    process.stdout.write(`${JSON.stringify({ round, results, summary }, null, 2)}\n`);
  } else {
    for (const r of results) {
      const mark = r.status === PASS ? '✔' : r.status === FAIL ? '✘' : '—';
      process.stdout.write(`${mark} ${r.name}：${r.detail}\n`);
    }
    process.stdout.write(
      `\n合计 ${summary.total} 条：通过 ${summary.pass} · 失败 ${summary.fail} · 未执行 ${summary.skipped}\n` +
        `扫描面：${scanLabel}\n` +
        `扫了 ${sources.length} 个文件 · 证据：${outPath}\n`,
    );
  }
  // 退出码按结论算（沿 P1 判据的口径）：有失败就是 1。
  process.exitCode = summary.fail > 0 ? 1 : 0;
  return { results, summary, outPath };
}
