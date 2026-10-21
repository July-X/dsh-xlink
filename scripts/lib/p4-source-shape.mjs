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
  return topLevelEntriesOf(body).map((e) => e.name);
}

/**
 * 抽一段**对象字面量正文**的顶层「键 ＋ 值表达式」（引号或裸键都认）。
 *
 * ⚠️ **键名与值必须成对** —— 判据第 3／8 条要断**类型**（协议 §4.3 ②）⇒ 只拿到键名不够。
 *
 * @param {string} body - 字面量正文（**不含**最外层的大括号）。
 * @returns {Array<{name: string, value: string}>}
 */
export function topLevelEntriesOf(body) {
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
    const quoted = /^["'`]([A-Za-z_$][A-Za-z0-9_$]*)["'`]\s*:\s*([\s\S]*)$/.exec(text);
    if (quoted) {
      keys.push({ name: quoted[1], value: quoted[2].trim() });
      continue;
    }
    const bare = /^([A-Za-z_$][A-Za-z0-9_$]*)\s*:\s*([\s\S]*)$/.exec(text);
    if (bare) keys.push({ name: bare[1], value: bare[2].trim() });
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
 * 抽 **Rust 侧** `items` 数组的**元素形态**（键 ＋ 类型）。
 *
 * 认 `items: Vec<X>` ⇒ 找 `struct X` ⇒ 取其字段 ⇒ Rust 类型映射成 JSON 侧类型
 * （认不出 ⇒ `unknown`，**不猜**）。
 *
 * @param {string} src - 源码文本。
 * @returns {Array<{name: string, type: string}>}
 */
export function rustItemsElementFields(src) {
  const structName = rustItemsElementType(src);
  if (!structName) return [];
  return rustStructFields(src, structName).map((f) => ({
    name: f.name,
    type: rustTypeToJsonType(f.type),
  }));
}

/**
 * 抽 **JS 侧** `items` 数组的**元素形态**（键 ＋ 类型）。
 *
 * ⚠️ **启发式**：在源码里找 `items` 出现处 → 其后第一个 `{` → 抽字段 ⇒
 * **只认「含目录项那 6 个键之一」的候选** ⇒ ⇒ 抽到无关字面量（箭头函数体等）
 * 一律丢弃 ⇒ ⇒ **抽不到就回空数组**（判据据此报红并点名，**不假绿**）。
 *
 * @param {string} src - 源码文本。
 * @param {string[]} itemKeys - 用来认「这个字面量像不像目录项」的键名。
 * @returns {Array<{name: string, type: string}>}
 */
export function jsItemsElementFields(src, itemKeys = []) {
  const text = String(src ?? '');
  const candidates = [];
  let idx = text.indexOf('items');
  while (idx >= 0) {
    const braceAt = text.indexOf('{', idx);
    if (braceAt < 0) break;
    const fields = jsLiteralFields(text, braceAt);
    if (fields.length > 0) {
      const names = fields.map((f) => f.name);
      if (itemKeys.length === 0 || itemKeys.some((k) => names.includes(k))) {
        candidates.push(fields);
      }
    }
    idx = text.indexOf('items', idx + 'items'.length);
  }
  if (candidates.length === 0) return [];
  return candidates.sort((a, b) => b.length - a.length)[0];
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
 * **Rust 类型 → JSON 侧类型**（协议 §4.3 ② 那几个类型）。
 *
 * ⚠️ **只认明确映射** ⇒ 认不出就回 `unknown`（**不猜**）—— 判据据此报红并点名，
 * 绝不把「读不出类型」当成「类型正确」。
 *
 * @param {string} type - Rust 类型文本。
 * @returns {'string'|'boolean'|'number'|'unknown'}
 */
export function rustTypeToJsonType(type) {
  const t = String(type ?? '').trim().replace(/\s+/g, '');
  if (t === 'bool') return 'boolean';
  if (/^(String|&str|str|Cow<str>|Box<str>)$/.test(t)) return 'string';
  if (/^(f32|f64|i8|i16|i32|i64|isize|u8|u16|u32|u64|usize)$/.test(t)) return 'number';
  const opt = /^(?:Option|Vec|Box)<(.+)>$/.exec(t);
  if (opt) return rustTypeToJsonType(opt[1]);
  return 'unknown';
}

/**
 * 抽一个 **Rust struct** 的字段（名字 ＋ 类型文本）。
 *
 * @param {string} src - 源码文本。
 * @param {string} structName - struct 名。
 * @returns {Array<{name: string, type: string}>}（抽不到 ⇒ 空数组）。
 */
export function rustStructFields(src, structName) {
  const text = String(src ?? '');
  const m = new RegExp(`\\bstruct\\s+${structName}\\s*\\{`).exec(text);
  if (!m) return [];
  const start = text.indexOf('{', m.index);
  const end = matchBrace(text, start);
  if (end < 0) return [];
  const body = text.slice(start + 1, end);
  const out = [];
  for (const raw of body.split('\n')) {
    const line = raw.trim();
    if (line === '' || line.startsWith('//') || line.startsWith('#[') || line.startsWith('///')) continue;
    const mm = /^([a-z_][A-Za-z0-9_]*)\s*:\s*([^,]+?),?$/.exec(line);
    if (mm) out.push({ name: mm[1], type: mm[2].trim() });
  }
  return out;
}

/**
 * 找「载荷里 `items` 字段的**元素类型名**」（Rust 侧）。
 *
 * 认 `items: Vec<X>` ／ `items: Option<Vec<X>>` ／ `pub items: Vec<X>`。
 *
 * @param {string} src - 源码文本。
 * @returns {string|null} 元素类型名（找不到 ⇒ null）。
 */
export function rustItemsElementType(src) {
  const text = String(src ?? '');
  const re = /\bitems\s*:\s*(?:Option\s*<\s*)?Vec\s*<\s*([A-Za-z_][A-Za-z0-9_]*)/g;
  const m = re.exec(text);
  return m ? m[1] : null;
}

/**
 * 抽一个 **JS 对象字面量**的字段（名字 ＋ **类型猜测**）。
 *
 * ⚠️ **类型只认明确线索**（`String(…)` / `Number(…)` / `Boolean(…)` / `!!x` /
 * `x === true` / `.toString()` …）⇒ 猜不出回 `unknown`（**不猜**）。
 *
 * @param {string} src - 源码文本。
 * @param {string|number} anchor - 定位用的锚点：**字符串** ＝ 锚点文本（字面量就在它
 *   后面）；**数字** ＝ 字面量起始下标（从该处往后找第一个 `{`）。
 * @returns {Array<{name: string, type: string}>}
 */
export function jsLiteralFields(src, anchor) {
  const text = String(src ?? '');
  const at = typeof anchor === 'number' ? anchor : text.indexOf(anchor);
  if (at < 0) return [];
  const start = text.indexOf('{', typeof anchor === 'number' ? at : at + anchor.length - 1);
  if (start < 0) return [];
  const end = matchBrace(text, start);
  if (end < 0) return [];
  return topLevelEntriesOf(text.slice(start + 1, end)).map((e) => ({
    name: e.name,
    type: guessJsValueType(e.value),
  }));
}

/**
 * 猜一个 **JS 值表达式**的类型（认不出 ⇒ `unknown`）。
 *
 * @param {string} expr - 值表达式文本。
 * @returns {'string'|'boolean'|'number'|'unknown'}
 */
export function guessJsValueType(expr) {
  const e = String(expr ?? '').trim();
  if (e === '') return 'unknown';
  if (/^String\s*\(/.test(e) || /\.toString\s*\(\s*\)/.test(e)) return 'string';
  if (/^Number\s*\(/.test(e) || /^parseInt\s*\(/.test(e) || /^parseFloat\s*\(/.test(e)) {
    return 'number';
  }
  if (/^Boolean\s*\(/.test(e) || /^!!/.test(e) || /===\s*(true|false)\b/.test(e)) {
    return 'boolean';
  }
  return 'unknown';
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
