#!/usr/bin/env node
// P4 协议 §10 判据**第 2 条**（零痕迹）—— **按形态分**（izzy 2026-10-23 上午拍板）。
//
// ## 为什么改成「按形态分」
//
// 协议 v0.27 正文仍写着「取数形态唯一 ＝ 壳侧 DevTools `Network` 域订阅」「页内动作清单 ＝ 空」，
// 但**实现已经走在协议前面**：macOS 上已落地**第二种取数形态**（页内自取数，路线 B），
// MiniMax 还要用**第三种**（页内旁听）⇒ ⇒ izzy 拍板：协议升版，**「页内动作清单 ＝ 空」改成按形态分**
// ⇒ ⇒ **判据脚本必须跟着改**（不改就会被新形态打穿，或被新形态逼成假绿）。
//
// ## 三种形态（本判据要区分的全部内容）
//
// | 形态 | 平台/站点 | 页内发生了什么 | 允许的页内痕迹 |
// | --- | --- | --- | --- |
// | **A｜壳侧 Network 域订阅** | Windows（DeepSeek 主形态） | **零动作**：不注入脚本、不改 `XMLHttpRequest.prototype`、不留槽位、不留定时器 | **无** |
// | **B｜页内自取数（路线 B）** | macOS（DeepSeek/ChatGPT） | 壳在**不可见取数窗**里注入一段脚本，**由页内脚本自己发请求**；结果经**一个随机名临时槽位**回传，**取到即删**（机制：`src-tauri/src/official_chat/script_eval.rs` 的 `round_trip` ＋ `build_slot_script` ＋ `clear_slot`；槽位名形如 `__dsh_rt_<hex>`，**前缀固定**） | **只允许那一个临时槽位**，且**取到即删** ⇒ 取数**结束后**的快照必须回到取数前；**不许**留定时器、**不许**留常驻钩子、**不许**新增其他 `window` 自有属性 |
// | **C｜页内旁听** | macOS（MiniMax） | 页内**替换** `XMLHttpRequest.prototype` 的 `open`／`send`（必要时同 `fetch`）以截住**站点自己那一发**，**取到即还原成「替换前那个引用」** | 允许替换，但**三条约束**必须成立：① **不新增 `window` 自有属性** ② **取到即还原**（**含不留钩子、不留定时器、不留槽位**）③ **不写站点数据**（只读） |
//
// ## ⚠️ 两条依据不许互相顶替（协议 v0.10 的原话）
//
// · **形态 B 之所以允许**，是因为它的值是**页内脚本自己取自己回**（**不需要**「页内→壳」的回程通道）；
// · **形态 C 之所以需要单独裁定**，是因为它**替换 prototype**（有可被站点发现的函数文本差异）。
// ⇒ ⇒ 这两条依据是**分开的** ⇒ ⇒ 判据**按形态选断言集合**，**不许**互相顶替。
//
// ## 判定内核按形态选断言集合
//
// · **形态 A**：三条主断言**一字不动**（`window` 逐项一致／`XMLHttpRequest.prototype` 逐项一致／
//   `__dsh*` ＝ 0）＋ 两条配套加强项（属性增量 ＝ 0、`open`／`send` 引用前后一致）。
// · **形态 B**：断言集合**包含形态 A 的全部断言**（取数**结束后**槽位已删 ⇒ 快照必须回到取数前）
//   ＋ **形态专属**的一条：取数**进行中**的快照里 `__dsh*` **恰好**是那一个槽位。
//   ⚠️ **不许**把形态 B 放宽成「`__dsh*` 允许存在」—— 那是假绿。
// · **形态 C**：断言集合**包含**形态 A 的「`window` 逐项一致」＋「`__dsh*` ＝ 0」，
//   而「`XMLHttpRequest.prototype` 逐项一致」与「`open`／`send` 引用前后一致」两条**按 v0.7 口径改判**：
//   允许**换过**，但要求**还原后引用 ＝ 替换前那个引用** ⇒ ⇒ **「换过但没还原」必须判红**。
//   （协议 §7.1 的成员侧两轮实测读数作参照：`window` 1257→1257、`XHR.prototype` 27→27、
//   还原后 `open`／`send` 引用 ＝ 替换前引用、对照组 3 秒自然增长 ＝ 0、**站点仍正常发请求**。）
//
// ## 读数契约
//
// `{ form: 'a' | 'b' | 'c', before, mid?, after, baseline? }`
//
// · `before` ／ `after` ＝ 跑前／跑后快照（**必需**）。
// · `mid` ＝ 取数**进行中**的快照 —— **形态 B 必需**（形态 A／C 可省）。
// · `baseline` ＝ 对照组（没跑那一轮的那个 tab，用来扣站点自然增长）。
// · ⚠️ **`before` 必须在「装钩子之前」取** —— 否则形态 C 的「还原后 ＝ 替换前」就没有意义。
//
// ## 真机轨的喂读数入口
//
// `DSH_P4_TRACE_READINGS` 环境变量指向一个 JSON 文件（内容 ＝ 上面那个 `readings` 对象）
// ⇒ 给了就**真判**（通过/失败），没给就报「**未执行**」（不是通过、也不是失败）。
//
// ## 离线源码形状（形态 B／C 的链；**能离线跑、不用真机**）
//
// 真机轨量的是**痕迹面**；形态 B／C 的**链本身**还有几处**静态**性质可以离线断：
//
// · **形态 B 的链**：注入脚本必须是 **IIFE**（不留顶层全局量）＋ **不得出现 `setInterval`／
//   `setTimeout`**（形态 B 不许留定时器）＋ 取数结束后槽位必须被清（`clear_slot` 调用存在）。
// · **形态 C 的链**：hook 脚本必须是 **IIFE** ＋ **必须出现「还原」动作**（把 `open`／`send`
//   恢复成**替换前那个引用**的代码路径必须存在）＋ **不得出现 `setInterval`／`setTimeout`**。
// · **不写站点数据**（形态 C 的第三条约束）：hook 脚本里不许出现写站点存储的原语。
//
// ## 如实登记的观测局限
//
// · **函数引用用「函数文本」代理**（`Function.prototype.toString`）：快照要跨两次页内求值
//   比较「是不是同一个引用」，**不能**在页内留槽位 ⇒ 只能用文本代理。局限：两个**文本相同
//   但不是同一个**的函数分辨不出。实测中 `open`／`send` 均为 `[native code]` ⇒ 在本站该口径下
//   文本代理与引用相等**等价**。
// · **「不留定时器」在真机轨里测不了**：页内**没有**枚举活跃定时器的标准 API ⇒ 定时器残留不落在
//   `window` 自有属性上 ⇒ 这一条在**真机轨**不可机检（如实登记为观测缺口）；**离线源码形状**
//   那侧由「页内脚本里不许出现定时器 API」兜着（那是**源码形态**，不是运行期残留）。
//
// @module scripts/check-official-chat-p4-trace

import { mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { fnBody, stripComments as stripCommentLines } from './lib/p4-source-shape.mjs';

/** verdict 三态。 */
export const PASS = 'pass';
export const FAIL = 'fail';
export const SKIPPED = 'skipped';

/** §7.1：页内全局量的禁止前缀。 */
export const DSH_PREFIX = '__dsh';

/**
 * **形态 B 的临时槽位前缀**（`script_eval.rs::new_slot_name` 生成的槽位名形如
 * `__dsh_rt_<hex>` ⇒ **前缀固定**）。
 *
 * ⚠️ 它是**判据自己的常量** ⇒ 源码里前缀一改（⇒ 真机轨的中途快照判不出「那一个槽位」）
 * ⇒ ⇒ 离线那条「槽位名前缀」会红 ⇒ ⇒ 改源码的人必须同步改这里。
 */
export const SLOT_PREFIX = '__dsh_rt_';

/** 三种取数形态（izzy 2026-10-23 拍板：按形态分）。 */
export const FORM_A = 'a';
export const FORM_B = 'b';
export const FORM_C = 'c';
export const TRACE_FORMS = Object.freeze([FORM_A, FORM_B, FORM_C]);

/** 形态的标签（结果名与文案里用；**别处不许另写一套**）。 */
export const FORM_LABELS = Object.freeze({
  a: '形态 A｜壳侧 DevTools `Network` 域订阅（页内零动作）',
  b: '形态 B｜页内自取数（路线 B：页内脚本自己取自己回）',
  c: '形态 C｜页内旁听（替换 prototype，取到即还原）',
});

// ── 形态归属：按 **cfg 段** 切分（PM 2026-10-23 拍板）────────────────────────
//
// ## 为什么按 cfg 判归属（PM 一手核过的布局，**事实**）
//
// · **形态 A 的链全部**在 `session_structure.rs` 的 `#[cfg(windows)]` 段
//   （`build_capture_window`／`arm_capture_window`／`capture_in`／`subscribe_network_events`）；
// · **形态 B 的链全部**在 `#[cfg(target_os = "macos")]` 段
//   （…／`page_ready_script`／`page_fetch_script`／`read_page_ready`／`capture_via_page`）。
//
// ⇒ ⇒ **平台 cfg 本身就是形态分界** ⇒ ⇒ **不需要另加标记约定**
// （加标记 ＝ 给实现侧塞新负担，且可被漏写 ⇒ 漏写会被 fail-safe 归到形态 A
// ⇒ **看起来更严、实际是误红**）。
//
// ## 归属表
//
// | 段 | 归属 |
// | --- | --- |
// | `#[cfg(windows)]` 段 | 形态 A |
// | `#[cfg(target_os = "macos")]` 段 | 形态 B |
// | 两边都编译的段（`cfg(any(windows, target_os = "macos"))`） | 形态 A **＋** 形态 B |
// | **公共段**（文件里没挂 cfg 的那一部分） | 该**文件的主人**；文件没有主人 ⇒ **归属不明 ⇒ 按形态 A 判**（fail-safe，不放行） |
// | `#[cfg(test)]`／只在别的平台编译的段 | **不属任何形态**（不是产品路径，见下） |
// | 页内脚本文件（`.js`，整文件一段） | 声明的形态 → 引用它的 cfg 段 → 都没有 ⇒ **归属不明 ⇒ 形态 A** |
//
// ## fail-safe 的用意（PM 原话：**故意的严格**）
//
// 「归属不明 ⇒ 按形态 A 判」保证「**有人把形态 B 的代码挪到公共段**」时判据**变红**而不是放行
// （形态 A 是三条形状断言里最严的那一档 ⇒ 公共段按它判 ⇒ 挪进去就响）。
//
// ## 为什么 `#[cfg(test)]` **不算**「归属不明」
//
// 测试段**不进产品二进制**（只有 `cargo test` 才编）⇒ ⇒ 它既不是形态 A 的运行路径、
// 也不是形态 B 的 ⇒ ⇒ 把它按形态 A 判只会造**假红**（测试里写 `fetch('/api/v0/…')` 是常见夹具，
// 本仓 `session_structure.rs` 的测试段里就有）⇒ ⇒ **排除**。
//
// ⚠️ **同一条纪律在本仓已有先例**：9e 的冷档案判据用 `stripTestModule` **只断生产段**
// （「测试里会删自己造的假源档案」⇒ 不截 ⇒ 把测试代码当生产代码 ⇒ 假红）。

/** 段的平台归属种类。 */
export const SEG_COMMON = 'common';
export const SEG_WINDOWS = 'windows';
export const SEG_MACOS = 'macos';
export const SEG_BOTH = 'both';
export const SEG_OTHER = 'other';
export const SEG_FILE = 'file';

/**
 * 跳过一个 Rust **块注释**（Rust 的块注释**可嵌套**）—— 返回结束后的下标。
 *
 * @param {string} src - 源码文本。
 * @param {number} i - `/*` 的下标。
 * @returns {number}
 */
function skipBlockComment(src, i) {
  let depth = 0;
  let p = i;
  const n = src.length;
  while (p < n) {
    if (src[p] === '/' && src[p + 1] === '*') {
      depth += 1;
      p += 2;
      continue;
    }
    if (src[p] === '*' && src[p + 1] === '/') {
      depth -= 1;
      p += 2;
      if (depth === 0) return p;
      continue;
    }
    p += 1;
  }
  return n;
}

/**
 * 跳过一个 Rust **字面量**（普通字符串／字符／原始字符串 `r#"…"#`／字节串）。
 *
 * ⚠️ **生命周期不是字面量**（`&'a str`）⇒ ⇒ `'` 后面**不是**立刻闭合的引号时**只跳一个字符**。
 *
 * @param {string} src - 源码文本。
 * @param {number} i - 候选起始下标。
 * @returns {number|null} 结束后的下标；**不是**字面量 ⇒ `null`。
 */
function skipRustLiteral(src, i) {
  const n = src.length;
  let p = i;
  if (src[p] === 'b') p += 1;
  if (src[p] === 'r') {
    p += 1;
    let hashes = 0;
    while (src[p] === '#') {
      hashes += 1;
      p += 1;
    }
    if (src[p] !== '"') return null;
    p += 1;
    const close = `"${'#'.repeat(hashes)}`;
    const at = src.indexOf(close, p);
    return at < 0 ? n : at + close.length;
  }
  if (src[p] === '"') {
    p += 1;
    while (p < n) {
      if (src[p] === '\\') {
        p += 2;
        continue;
      }
      if (src[p] === '"') return p + 1;
      p += 1;
    }
    return n;
  }
  if (src[p] === "'") {
    // 字符字面量：`'x'`／`'\n'`；**生命周期**（`'a`）不是。
    if (src[p + 1] === '\\') {
      const at = src.indexOf("'", p + 2);
      return at < 0 ? n : at + 1;
    }
    if (src[p + 2] === "'") return p + 3;
    return null;
  }
  return null;
}

/**
 * 拆 **顶层**逗号（`any(a, b)` 的参数表）—— 括号里的逗号不算。
 *
 * @param {string} text - 参数表文本。
 * @returns {string[]}
 */
function splitTopLevel(text) {
  const out = [];
  let depth = 0;
  let quote = null;
  let cur = '';
  for (let i = 0; i < text.length; i += 1) {
    const ch = text[i];
    if (quote) {
      cur += ch;
      if (ch === '\\') {
        cur += text[i + 1] ?? '';
        i += 1;
        continue;
      }
      if (ch === quote) quote = null;
      continue;
    }
    if (ch === '"' || ch === "'") {
      quote = ch;
      cur += ch;
      continue;
    }
    if (ch === '(' || ch === '[') {
      depth += 1;
      cur += ch;
      continue;
    }
    if (ch === ')' || ch === ']') {
      depth -= 1;
      cur += ch;
      continue;
    }
    if (ch === ',' && depth === 0) {
      out.push(cur);
      cur = '';
      continue;
    }
    cur += ch;
  }
  if (cur.trim()) out.push(cur);
  return out.map((s) => s.trim()).filter(Boolean);
}

/**
 * **cfg 谓词对某个平台的真值**。
 *
 * · 支持：`windows`／`unix`／`target_os = "…"`／`not(…)`／`any(…)`／`all(…)`。
 * · `test` ＝ **生产构建**（测试代码不进产品二进制 ⇒ 判为假）。
 * · 其余（`feature = "…"`／`debug_assertions`／看不懂的）⇒ **`null`（判不了）**。
 *
 * @param {string} predicate - 谓词文本（`cfg(...)` 里部分）。
 * @param {string} platform - `'windows'` 或 `'macos'`。
 * @returns {boolean|null}
 */
export function evalCfgPredicate(predicate, platform) {
  const text = String(predicate ?? '').trim();
  if (!text) return null;
  const call = /^([A-Za-z_][A-Za-z0-9_]*)\s*\(([\s\S]*)\)$/.exec(text);
  if (call) {
    const name = call[1];
    const args = splitTopLevel(call[2]);
    if (name === 'not') {
      const inner = evalCfgPredicate(args[0] ?? '', platform);
      return inner === null ? null : !inner;
    }
    if (name === 'any' || name === 'all') {
      const values = args.map((a) => evalCfgPredicate(a, platform));
      if (values.some((v) => v === null)) return null;
      return name === 'any' ? values.some((v) => v === true) : values.every((v) => v === true);
    }
    return null;
  }
  const kv = /^([A-Za-z_][A-Za-z0-9_]*)\s*(?:=\s*"([^"]*)")?$/.exec(text);
  if (!kv) return null;
  const name = kv[1];
  const value = kv[2];
  if (name === 'windows') return platform === 'windows';
  if (name === 'unix') return platform !== 'windows';
  if (name === 'target_os') return value === undefined ? null : value === platform;
  if (name === 'test') return false;
  return null;
}

/**
 * 一段源码的**平台归属种类**。
 *
 * ⚠️ **判不了 ⇒ 保守按「两边都算」**（fail-safe：宁可多判，不许漏判）。
 *
 * @param {string} predicate - cfg 谓词文本。
 * @returns {string} 段的种类（[`SEG_WINDOWS`]／[`SEG_MACOS`]／[`SEG_BOTH`]／[`SEG_OTHER`]）。
 */
export function cfgSegmentKind(predicate) {
  const w = evalCfgPredicate(predicate, 'windows');
  const m = evalCfgPredicate(predicate, 'macos');
  if (w === false && m === false) return SEG_OTHER;
  if (w === true && m === false) return SEG_WINDOWS;
  if (w === false && m === true) return SEG_MACOS;
  return SEG_BOTH;
}

/**
 * 找 `#[...]` 属性文本的结束位置（**括号配对**、跳过字面量）。
 *
 * @param {string} src - 源码文本。
 * @param {number} i - `#` 的下标。
 * @returns {number} 结束后的下标。
 */
function findAttributeEnd(src, i) {
  const n = src.length;
  let p = i;
  let depth = 0;
  while (p < n) {
    const ch = src[p];
    if (ch === '/' && src[p + 1] === '/') {
      while (p < n && src[p] !== '\n') p += 1;
      continue;
    }
    if (ch === '/' && src[p + 1] === '*') {
      p = skipBlockComment(src, p);
      continue;
    }
    if (ch === '"' || ch === "'" || ch === 'r' || ch === 'b') {
      const next = skipRustLiteral(src, p);
      if (next !== null) {
        p = next;
        continue;
      }
    }
    if (ch === '[') {
      depth += 1;
      p += 1;
      continue;
    }
    if (ch === ']') {
      depth -= 1;
      p += 1;
      if (depth === 0) return p;
      continue;
    }
    p += 1;
  }
  return n;
}

/**
 * 找 `#[cfg(…)]` **所挂的那一项**的结束位置。
 *
 * · 项是个**块**（`fn`／`impl`／`mod`…）⇒ 它的匹配 `}`；
 * · 项是**语句**（`use`／`let`…）⇒ 它的 `;`；
 * · 项是**结构体字段** ⇒ 撞上外层块的 `}` 就停（**不越界**，宁可少算不可多算）。
 *
 * @param {string} src - 源码文本。
 * @param {number} from - 属性之后的下标。
 * @returns {number}
 */
function findItemEnd(src, from) {
  const n = src.length;
  let i = from;
  let depth = 0;
  while (i < n) {
    const ch = src[i];
    if (ch === '/' && src[i + 1] === '/') {
      while (i < n && src[i] !== '\n') i += 1;
      continue;
    }
    if (ch === '/' && src[i + 1] === '*') {
      i = skipBlockComment(src, i);
      continue;
    }
    if (ch === '"' || ch === "'" || ch === 'r' || ch === 'b') {
      const next = skipRustLiteral(src, i);
      if (next !== null) {
        i = next;
        continue;
      }
    }
    if (ch === '(' || ch === '[') {
      depth += 1;
      i += 1;
      continue;
    }
    if (ch === ')' || ch === ']') {
      depth -= 1;
      i += 1;
      continue;
    }
    if (depth === 0 && ch === '{') return findMatchingBrace(src, i);
    if (depth === 0 && ch === ';') return i + 1;
    if (depth === 0 && ch === '}') return i;
    i += 1;
  }
  return n;
}

/**
 * 找 `{` 的匹配 `}`（跳过字面量与注释；**Rust 的块注释可嵌套**）。
 *
 * @param {string} src - 源码文本。
 * @param {number} open - `{` 的下标。
 * @returns {number} 匹配 `}` 之后的下标。
 */
function findMatchingBrace(src, open) {
  const n = src.length;
  let i = open;
  let depth = 0;
  while (i < n) {
    const ch = src[i];
    if (ch === '/' && src[i + 1] === '/') {
      while (i < n && src[i] !== '\n') i += 1;
      continue;
    }
    if (ch === '/' && src[i + 1] === '*') {
      i = skipBlockComment(src, i);
      continue;
    }
    if (ch === '"' || ch === "'" || ch === 'r' || ch === 'b') {
      const next = skipRustLiteral(src, i);
      if (next !== null) {
        i = next;
        continue;
      }
    }
    if (ch === '{') depth += 1;
    if (ch === '}') {
      depth -= 1;
      if (depth === 0) return i + 1;
    }
    i += 1;
  }
  return n;
}

/**
 * 把一段 Rust 源码**按 cfg 段切开**。
 *
 * @param {string} src - 源码文本。
 * @returns {Array<{start: number, end: number, kind: string, cfg: string|null}>}
 */
export function segmentRustByCfg(src) {
  const text = String(src ?? '');
  const n = text.length;
  const regions = [];
  let i = 0;
  while (i < n) {
    const ch = text[i];
    if (ch === '/' && text[i + 1] === '/') {
      while (i < n && text[i] !== '\n') i += 1;
      continue;
    }
    if (ch === '/' && text[i + 1] === '*') {
      i = skipBlockComment(text, i);
      continue;
    }
    if (ch === '"' || ch === "'" || ch === 'r' || ch === 'b') {
      const next = skipRustLiteral(text, i);
      if (next !== null) {
        i = next;
        continue;
      }
    }
    if (ch === '#' && text[i + 1] === '[') {
      const end = findAttributeEnd(text, i);
      const attr = text.slice(i, end);
      const m = /^#\s*\[\s*cfg\s*\(([\s\S]*)\)\s*\]$/.exec(attr);
      if (m) {
        const predicate = m[1].trim();
        regions.push({
          start: i,
          end: findItemEnd(text, end),
          kind: cfgSegmentKind(predicate),
          cfg: predicate,
        });
      }
      i = end;
      continue;
    }
    i += 1;
  }
  return buildSegments(text, regions);
}

/**
 * 把「带 cfg 的区域」铺成**连续的段**（区域之间的空隙 ＝ 公共段）。
 *
 * @param {string} src - 源码文本。
 * @param {Array<{start: number, end: number, kind: string, cfg: string|null}>} regions - 区域。
 * @returns {Array<{start: number, end: number, kind: string, cfg: string|null}>}
 */
function buildSegments(src, regions) {
  const n = src.length;
  const segs = [];
  const stack = [];
  let pos = 0;
  /** 落一段（带原文，方便调用方直接判形状）。 */
  const push = (start, end, kind, cfg) => {
    if (end <= start) return;
    segs.push({ start, end, kind, cfg, code: src.slice(start, end) });
  };
  for (const region of regions) {
    while (stack.length && stack[stack.length - 1].end <= region.start) {
      const done = stack.pop();
      if (pos < done.end) {
        push(pos, done.end, done.kind, done.cfg);
        pos = done.end;
      }
    }
    const curKind = stack.length ? stack[stack.length - 1].kind : SEG_COMMON;
    if (pos < region.start) {
      push(pos, region.start, curKind, null);
      pos = region.start;
    }
    stack.push({ end: region.end, kind: region.kind, cfg: region.cfg });
  }
  while (stack.length) {
    const done = stack.pop();
    if (pos < done.end) {
      push(pos, done.end, done.kind, done.cfg);
      pos = done.end;
    }
  }
  if (pos < n) push(pos, n, SEG_COMMON, null);
  return segs;
}

/**
 * 取一段的**起始行号**（1 起）。
 *
 * @param {string} src - 源码文本。
 * @param {number} index - 字符下标。
 * @returns {number}
 */
function lineOf(src, index) {
  let line = 1;
  for (let i = 0; i < index && i < src.length; i += 1) {
    if (src[i] === '\n') line += 1;
  }
  return line;
}

/**
 * **页内脚本文件**里引用的其它页内脚本（`include_str!("…")`）。
 *
 * ⚠️ **只认同目录的相对引用**（`../…` 指向扫描面外 ⇒ 不认）。
 *
 * @param {string} code - 文件内容。
 * @param {string} dir - 文件所在目录（相对仓库根，`/` 分隔）。
 * @returns {string[]} 被引用的相对路径。
 */
function includeStrRefs(code, dir) {
  const out = [];
  const re = /include_str!\s*\(\s*"([^"]+)"\s*\)/g;
  let m = re.exec(code);
  while (m !== null) {
    const ref = m[1];
    if (!ref.startsWith('../') && !ref.startsWith('/')) {
      out.push(dir ? `${dir}/${ref}` : ref);
    }
    if (m.index === re.lastIndex) re.lastIndex += 1;
    m = re.exec(code);
  }
  return out;
}

/**
 * **把扫描面按形态归属**（PM 2026-10-23 拍板：按 cfg 段判归属）。
 *
 * ## 归属规则（**照 PM 裁定**，不另发明标记约定）
 *
 * | 段 | 归属 |
 * | --- | --- |
 * | `#[cfg(windows)]` 段 | 形态 A |
 * | `#[cfg(target_os = "macos")]` 段 | 形态 B |
 * | 两边都编译的段 | 形态 A ＋ 形态 B |
 * | **公共段** | **该文件的主人**；没有主人 ⇒ **归属不明 ⇒ 按形态 A 判**（fail-safe） |
 * | `#[cfg(test)]`／别的平台 | **不属任何形态**（不是产品路径） |
 * | `.js` 页内脚本文件 | 声明的形态 → 引用它的 cfg 段 → 都没有 ⇒ **归属不明 ⇒ 形态 A** |
 *
 * @param {Array<{path: string, code: string}>} sources - 扫描面。
 * @param {Record<string, string>} owners - 路径 → 形态（`'a'`／`'b'`／`'c'`）；没有 ⇒ 归属不明。
 * @returns {{byForm: {a: Array<object>, b: Array<object>, c: Array<object>}, unattributed: Array<object>}}
 */
export function attributeByForm(sources, owners = {}) {
  const list = Array.isArray(sources) ? sources : [];
  const byForm = { a: [], b: [], c: [] };
  const unattributed = [];

  /** 段的最终形态归属（公共段看文件主人；没有主人 ⇒ 形态 A＝fail-safe）。 */
  const formsOf = (seg, owner) => {
    if (seg.kind === SEG_WINDOWS) return ['a'];
    if (seg.kind === SEG_MACOS) return ['b'];
    if (seg.kind === SEG_BOTH) return ['a', 'b'];
    if (seg.kind === SEG_OTHER) return [];
    // 公共段（`.rs` 的无 cfg 部分）／整文件（`.js`）
    return owner ? [owner] : ['a'];
  };

  // ① `.rs` 文件 ⇒ 按 cfg 段切
  const rustSegments = [];
  for (const source of list) {
    if (!source.path.endsWith('.rs')) continue;
    const owner = owners[source.path] ?? null;
    for (const seg of segmentRustByCfg(source.code)) {
      const forms = formsOf(seg, owner);
      const item = {
        path: source.path,
        line: lineOf(source.code, seg.start),
        kind: seg.kind,
        cfg: seg.cfg,
        owner,
        code: seg.code,
      };
      rustSegments.push({ item, forms });
      if (forms.length === 0) continue;
      if (forms.every((f) => f === 'a') && owner === null && seg.kind === SEG_COMMON) {
        unattributed.push(item);
      }
    }
  }

  // ② `.js` 页内脚本文件 ⇒ 整文件一段；主人 ＝ 声明 → 引用它的 cfg 段 → 都没有 ⇒ 归属不明
  const jsFiles = list.filter((s) => !s.path.endsWith('.rs'));
  const referencedBy = new Map();
  for (const { item, forms } of rustSegments) {
    const dir = item.path.includes('/') ? item.path.slice(0, item.path.lastIndexOf('/')) : '';
    for (const ref of includeStrRefs(item.code, dir)) {
      if (!referencedBy.has(ref)) referencedBy.set(ref, new Set());
      for (const f of forms) referencedBy.get(ref).add(f);
    }
  }
  for (const source of jsFiles) {
    const owner = owners[source.path] ?? null;
    const derived = owner ? null : [...(referencedBy.get(source.path) ?? [])];
    const item = {
      path: source.path,
      line: 1,
      kind: SEG_FILE,
      cfg: null,
      owner,
      code: source.code,
    };
    const forms = owner ? [owner] : derived && derived.length ? derived : ['a'];
    if (!owner && (!derived || derived.length === 0)) unattributed.push(item);
    for (const f of forms) byForm[f].push(item);
  }

  // ③ `.rs` 的段 ⇒ 归到各自的形态
  for (const { item, forms } of rustSegments) {
    for (const f of forms) byForm[f].push(item);
  }

  return { byForm, unattributed };
}

/**
 * **页内快照探针**（零副作用）。
 *
 * · 必须是 **IIFE** 且**不写任何全局量** ⇒ 探针自己**不留痕**（自测里用 `node:vm` 在假
 *   `globalThis` 上验过：跑前跑后自有属性数不变）。
 * · 只回**结构**：属性名数组 ＋ 函数文本。**不回任何值**、不回正文、不回凭据。
 */
export const TRACE_SNAPSHOT_JS = `(() => {
  const own = (o) => Object.getOwnPropertyNames(o);
  const text = (f) => (typeof f === 'function' ? Function.prototype.toString.call(f) : null);
  const proto = globalThis.XMLHttpRequest ? globalThis.XMLHttpRequest.prototype : null;
  return {
    windowProps: own(globalThis),
    protoProps: proto ? own(proto) : null,
    openText: proto ? text(proto.open) : null,
    sendText: proto ? text(proto.send) : null,
  };
})()`;

/**
 * 造一份**合成快照**（**不是**真实读数；真实读数形态见 §7.1 的实测数字）。
 *
 * @param {{props?: string[], protoProps?: string[], openText?: string, sendText?: string}} [opts]
 * @returns {{windowProps: string[], protoProps: string[], openText: string, sendText: string}}
 */
export function fakeSnapshot({
  props = ['location', 'navigator', 'XMLHttpRequest'],
  protoProps = ['constructor', 'open', 'send'],
  openText = 'function open() { [native code] }',
  sendText = 'function send() { [native code] }',
} = {}) {
  return { windowProps: [...props], protoProps: [...protoProps], openText, sendText };
}

/**
 * 取一份快照里的 `__dsh*` 全局量名。
 *
 * @param {object} snapshot - 快照。
 * @returns {string[]}
 */
function dshNames(snapshot) {
  const props = Array.isArray(snapshot?.windowProps) ? snapshot.windowProps : [];
  return props.filter((n) => String(n).startsWith(DSH_PREFIX));
}

/**
 * **判定内核**：按**形态**判「**跑前 → 跑后**」（＋形态 B 的**中途**）快照。
 *
 * ⚠️ **形态 A 的三条主断言一字不动**（`window` 逐项一致／`prototype` 逐项一致／`__dsh*` ＝ 0）
 * —— 本次改造**只加形态、不放宽形态 A**。
 * ⚠️ 形态 C 的「prototype 逐项一致」与「`open`／`send` 引用前后一致」两条**按 v0.7 口径改判**
 * （允许换过、必须还原成替换前那个引用）⇒ ⇒ **「换过但没还原」判红**。
 *
 * @param {{form?: string, before?: object, mid?: object, after?: object, baseline?: {before: object, after: object}}} readings
 * @param {string} [form] - 形态（`a`／`b`／`c`）；缺省取 `readings.form`，再缺省 `a`。
 * @returns {{ok: boolean, checks: Array<{name: string, status: string, detail: string}>}}
 */
export function judgeTrace(readings, form = readings?.form ?? FORM_A) {
  const checks = [];
  const add = (name, status, detail) => {
    checks.push({ name, status, detail });
  };

  const f = TRACE_FORMS.includes(form) ? form : null;
  if (!f) {
    add(
      '形态已知（a / b / c 三选一）',
      FAIL,
      `读到未知形态 ${JSON.stringify(form ?? null)} ⇒ 判据无法执行（不许默默按某一形态判）`,
    );
    return { ok: false, checks };
  }

  const before = readings?.before ?? null;
  const after = readings?.after ?? null;
  const mid = readings?.mid ?? null;
  if (!before || !after) {
    add('读数齐全（跑前 / 跑后两份快照）', FAIL, '缺 before 或 after 快照 ⇒ 判据无法执行');
    return { ok: false, checks };
  }

  const bProps = Array.isArray(before.windowProps) ? before.windowProps : [];
  const aProps = Array.isArray(after.windowProps) ? after.windowProps : [];
  const bSet = new Set(bProps);
  const aSet = new Set(aProps);
  const added = aProps.filter((x) => !bSet.has(x));
  const removed = bProps.filter((x) => !aSet.has(x));

  // 对照组（§7.1 的读数方法用「对照组相减」剔除站点自然增长）
  // ⚠️ **v0.9 形态下的角色**：实测组 ＝ **跑了壳侧 `Network` 域订阅**那个 tab；
  //   对照组 ＝ **没跑订阅**那个 tab ⇒ 扣掉的是站点自己的自然增长，不是别的形态。
  const ctrlBefore = readings?.baseline?.before ?? null;
  const ctrlAfter = readings?.baseline?.after ?? null;
  const hasControl = !!ctrlBefore && !!ctrlAfter;
  const ctrlB = hasControl && Array.isArray(ctrlBefore.windowProps) ? ctrlBefore.windowProps : [];
  const ctrlA = hasControl && Array.isArray(ctrlAfter.windowProps) ? ctrlAfter.windowProps : [];
  const ctrlAdded = hasControl ? ctrlA.filter((x) => !ctrlB.includes(x)) : [];
  const ctrlRemoved = hasControl ? ctrlB.filter((x) => !ctrlA.includes(x)) : [];
  const ctrlDelta = hasControl ? ctrlA.length - ctrlB.length : null;

  // 「实测独有」＝ 实测组出现/移除、而对照组没有出现/移除的 ⇒ 归因于实测（站点自然增长之外的痕迹）
  const unexplainedAdded = hasControl ? added.filter((x) => !ctrlAdded.includes(x)) : added;
  const unexplainedRemoved = hasControl ? removed.filter((x) => !ctrlRemoved.includes(x)) : removed;
  add(
    'window 自有属性前后逐项一致',
    unexplainedAdded.length === 0 && unexplainedRemoved.length === 0 ? PASS : FAIL,
    `实测组新增 ${added.length} 个／移除 ${removed.length} 个` +
      (hasControl
        ? `；对照组自然新增 ${ctrlAdded.length} 个／自然移除 ${ctrlRemoved.length} 个 ⇒ 实测独有 新增 ${unexplainedAdded.length} 个／移除 ${unexplainedRemoved.length} 个` +
          (unexplainedAdded.length ? `（${unexplainedAdded.slice(0, 5).join('、')}）` : '')
        : '；本轮无对照组 ⇒ 实测组的新增/移除必须为空') +
      (unexplainedRemoved.length ? `（移除：${unexplainedRemoved.slice(0, 5).join('、')}）` : ''),
  );

  const delta = aProps.length - bProps.length;
  const attributable = hasControl ? delta - ctrlDelta : delta;
  add(
    'window 属性增量 ＝ 0',
    attributable === 0 ? PASS : FAIL,
    hasControl
      ? `实测组增量=${delta} − 对照组自然增长=${ctrlDelta} ⇒ 归因增量=${attributable}`
      : `增量=${attributable}（${bProps.length} → ${aProps.length}；本轮无对照组）`,
  );

  const dshBefore = dshNames(before);
  const dshAfter = dshNames(after);
  add(
    '`__dsh*` 全局量 ＝ 0',
    dshBefore.length === 0 && dshAfter.length === 0 ? PASS : FAIL,
    `跑前 ${dshBefore.length} 个／跑后 ${dshAfter.length} 个${dshAfter.length ? `（${dshAfter.slice(0, 5).join('、')}）` : ''}`,
  );

  const bProto = Array.isArray(before.protoProps) ? before.protoProps : [];
  const aProto = Array.isArray(after.protoProps) ? after.protoProps : [];
  const protoAdded = aProto.filter((x) => !bProto.includes(x));
  const protoRemoved = bProto.filter((x) => !aProto.includes(x));

  if (f === FORM_A) {
    // ── 形态 A：三条主断言**一字不动** ＋ 两条配套加强项 ──────────────────
    // ⚠️ **不是「还原后一致」**（协议 §10 第 2 条在**形态 A** 下明文不许写成「还原后一致」）
    //   —— 形态 A 下页内**压根没碰过** `open`／`send` ⇒ 断的是「页内没碰过」：跑前跑后**同一个引用**。
    // ⚠️ 这一条**不可省**：上面那条「prototype 自有属性逐项一致」只比**属性名** ⇒ 「同名换了个
    //   函数」（＝改写 `open`）它抓不到 ⇒ 这一条才是断「没碰过方法」的那条。
    add(
      'XMLHttpRequest.prototype 自有属性逐项一致',
      protoAdded.length === 0 && protoRemoved.length === 0 ? PASS : FAIL,
      `新增 ${protoAdded.length} 个／移除 ${protoRemoved.length} 个（${bProto.length} → ${aProto.length}）`,
    );
    add(
      '页内 `open` 引用前后一致（页内没碰过）',
      before.openText !== null && before.openText === after.openText ? PASS : FAIL,
      `跑前=${before.openText ?? 'null'}／跑后=${after.openText ?? 'null'}`,
    );
    add(
      '页内 `send` 引用前后一致（页内没碰过）',
      before.sendText !== null && before.sendText === after.sendText ? PASS : FAIL,
      `跑前=${before.sendText ?? 'null'}／跑后=${after.sendText ?? 'null'}`,
    );
    // 形态 A 的加强项：**连中途都不许有页内变化**（形态 A 是页内零动作）。
    if (mid) {
      const midDsh = dshNames(mid);
      const midProps = Array.isArray(mid.windowProps) ? mid.windowProps : [];
      const midAdded = midProps.filter((x) => !bSet.has(x));
      const midRemoved = bProps.filter((x) => !midProps.includes(x));
      const midOpen = mid.openText === before.openText;
      const midSend = mid.sendText === before.sendText;
      add(
        '形态 A · 中途快照也无页内变化（形态 A ＝ 页内零动作）',
        midDsh.length === 0 &&
          midAdded.length === 0 &&
          midRemoved.length === 0 &&
          midOpen &&
          midSend
          ? PASS
          : FAIL,
        `中途 \`__dsh*\` ${midDsh.length} 个；window 新增 ${midAdded.length} 个／移除 ${midRemoved.length} 个；` +
          `\`open\` ${midOpen ? '未换过' : '已换过'}／\`send\` ${midSend ? '未换过' : '已换过'}`,
      );
    } else {
      add(
        '形态 A · 中途快照也无页内变化（形态 A ＝ 页内零动作）',
        SKIPPED,
        '本轮没给 mid（中途）快照 ⇒ 这条未执行（形态 A 的跑前/跑后两条独立兜着）',
      );
    }
  } else if (f === FORM_B) {
    // ── 形态 B：**包含形态 A 的全部断言** ＋ 形态专属的「中途只允许那一个槽位」──
    // ⚠️ 形态 B **不许**改 `XMLHttpRequest.prototype` ⇒ 那三条与形态 A **逐字同款**。
    add(
      'XMLHttpRequest.prototype 自有属性逐项一致',
      protoAdded.length === 0 && protoRemoved.length === 0 ? PASS : FAIL,
      `新增 ${protoAdded.length} 个／移除 ${protoRemoved.length} 个（${bProto.length} → ${aProto.length}）`,
    );
    add(
      '页内 `open` 引用前后一致（页内没碰过）',
      before.openText !== null && before.openText === after.openText ? PASS : FAIL,
      `跑前=${before.openText ?? 'null'}／跑后=${after.openText ?? 'null'}`,
    );
    add(
      '页内 `send` 引用前后一致（页内没碰过）',
      before.sendText !== null && before.sendText === after.sendText ? PASS : FAIL,
      `跑前=${before.sendText ?? 'null'}／跑后=${after.sendText ?? 'null'}`,
    );

    const midDsh = mid ? dshNames(mid) : [];
    if (!mid) {
      add(
        '形态 B · 取数进行中：`__dsh*` 全局量恰好 1 个（那一个临时槽位）',
        FAIL,
        '缺 mid（取数进行中）快照 ⇒ 无法验证「中途只允许那一个临时槽位」⇒ 判据无法执行' +
          '（**不许**按「没喂中途快照就当没有」判绿）',
      );
    } else {
      add(
        '形态 B · 取数进行中：`__dsh*` 全局量恰好 1 个（那一个临时槽位）',
        midDsh.length === 1 ? PASS : FAIL,
        `中途快照里 \`__dsh*\` 全局量 ${midDsh.length} 个` +
          (midDsh.length ? `（${midDsh.slice(0, 5).join('、')}）` : ''),
      );
      add(
        '形态 B · 取数进行中：那一个全局量形如 `__dsh_rt_*`（临时槽位）',
        midDsh.length === 1 && String(midDsh[0]).startsWith(SLOT_PREFIX) ? PASS : FAIL,
        `中途快照里 \`__dsh*\` 全局量 ${midDsh.length} 个：${midDsh.join('、') || '（无）'}；` +
          `槽位名前缀必须是 ${SLOT_PREFIX}`,
      );
      const midProps = Array.isArray(mid.windowProps) ? mid.windowProps : [];
      const midWithoutSlot = midProps.filter((n) => !String(n).startsWith(SLOT_PREFIX));
      const midAdded = midWithoutSlot.filter((x) => !bSet.has(x));
      const midRemoved = bProps.filter((x) => !midWithoutSlot.includes(x));
      add(
        '形态 B · 取数进行中：除那一个槽位外 `window` 自有属性逐项一致（不新增其他属性）',
        midAdded.length === 0 && midRemoved.length === 0 ? PASS : FAIL,
        `中途快照（去掉槽位后）新增 ${midAdded.length} 个／移除 ${midRemoved.length} 个`,
      );
    }
    add(
      '形态 B · 取数结束后：临时槽位已消失（取到即删）',
      dshAfter.length === 0 ? PASS : FAIL,
      `取数结束后 \`__dsh*\` 全局量 ${dshAfter.length} 个` +
        (dshAfter.length ? `（${dshAfter.slice(0, 5).join('、')}）` : ''),
    );
  } else {
    // ── 形态 C：v0.7 口径（允许换过、必须还原成替换前那个引用）──────────────
    // ⚠️ **两条依据不许互相顶替**：形态 C 之所以要单独裁定，是因为它**替换 prototype**
    //   （有可被站点发现的函数文本差异）⇒ ⇒ 这两条断言按 v0.7 口径改判。
    add(
      '形态 C · `XMLHttpRequest.prototype` 自有属性逐项一致（还原后口径：允许换过、必须还原）',
      protoAdded.length === 0 && protoRemoved.length === 0 ? PASS : FAIL,
      `还原后新增 ${protoAdded.length} 个／移除 ${protoRemoved.length} 个（${bProto.length} → ${aProto.length}）`,
    );
    add(
      '形态 C · 页内 `open` 还原后引用 ＝ 替换前引用（允许换过、必须还原）',
      before.openText !== null && before.openText === after.openText ? PASS : FAIL,
      `替换前=${before.openText ?? 'null'}／还原后=${after.openText ?? 'null'}` +
        (before.openText !== null && before.openText !== after.openText
          ? ' ⇒ **换过但没还原**（或还原成了别的引用）'
          : ''),
    );
    add(
      '形态 C · 页内 `send` 还原后引用 ＝ 替换前引用（允许换过、必须还原）',
      before.sendText !== null && before.sendText === after.sendText ? PASS : FAIL,
      `替换前=${before.sendText ?? 'null'}／还原后=${after.sendText ?? 'null'}` +
        (before.sendText !== null && before.sendText !== after.sendText
          ? ' ⇒ **换过但没还原**（或还原成了别的引用）'
          : ''),
    );
    if (mid) {
      const midDsh = dshNames(mid);
      const midProps = Array.isArray(mid.windowProps) ? mid.windowProps : [];
      const midAdded = midProps.filter((x) => !bSet.has(x));
      const midRemoved = bProps.filter((x) => !midProps.includes(x));
      const midOpenChanged = mid.openText !== before.openText;
      const midSendChanged = mid.sendText !== before.sendText;
      add(
        '形态 C · 取数进行中：允许换过的 `open`／`send`，但不许新增 `window` 自有属性、不许留 `__dsh*` 槽位',
        midDsh.length === 0 && midAdded.length === 0 && midRemoved.length === 0 ? PASS : FAIL,
        `中途 \`__dsh*\` ${midDsh.length} 个；window 新增 ${midAdded.length} 个／移除 ${midRemoved.length} 个；` +
          `中途 \`open\` ${midOpenChanged ? '已换过' : '未换过'}／\`send\` ${midSendChanged ? '已换过' : '未换过'}`,
      );
    } else {
      add(
        '形态 C · 取数进行中：允许换过的 `open`／`send`，但不许新增 `window` 自有属性、不许留 `__dsh*` 槽位',
        SKIPPED,
        '本轮没给 mid（中途）快照 ⇒ 这条未执行（形态 C 的「取到即还原」由上面两条「还原后 ＝ 替换前」独立兜着）',
      );
    }
  }

  // 对照组：§7.1 的读数方法用「对照组相减」剔除站点自然增长
  if (hasControl) {
    add(
      '对照组自然增长已相减（实测组 − 对照组）',
      delta - ctrlDelta === 0 ? PASS : FAIL,
      `实测组增量=${delta}／对照组自然增长=${ctrlDelta}／相减=${delta - ctrlDelta}`,
    );
  } else {
    add(
      '对照组自然增长已相减（实测组 − 对照组）',
      SKIPPED,
      '本轮读数未带对照组 ⇒ 未做自然增长相减（§7.1 的读数方法用对照组相减剔除站点自然增长）',
    );
  }

  // 如实登记的观测缺口（不是通过、也不是失败）
  add(
    '不留定时器（页内无法枚举活跃定时器 ⇒ 不可机检）',
    SKIPPED,
    '页内没有枚举活跃定时器的标准 API ⇒ 定时器残留不落在 window 自有属性上 ⇒ 本条不可机检（观测缺口，如实登记）',
  );

  const judged = checks.filter((c) => c.status !== SKIPPED);
  return { ok: judged.length > 0 && judged.every((c) => c.status === PASS), checks };
}

/**
 * **场景表**（合成读数）：每个场景声明「应当通过」还是「应当失败」。
 *
 * · **T1–T9** ＝ 形态 A 的既有九个场景（**一条不许删、不许放宽**）。
 * · **B1–B6** ＝ 形态 B 的六个场景（含「中途有槽位但取数后必须消失」＋「取数后槽位仍在 ⇒ 红」）。
 * · **C1–C5** ＝ 形态 C 的五个场景（含「换过但已还原 ⇒ 绿」＋「换过没还原 ⇒ 红」＋
 *   「还原后引用 ≠ 替换前引用 ⇒ 红」）。
 *
 * @returns {Array<{id: string, name: string, expect: string, form: string, readings: object}>}
 */
export function buildScenarios() {
  const clean = fakeSnapshot();

  // T1 正常：页内什么都没碰（跑前跑后逐项一致、无 `__dsh*`）
  const t1 = { before: clean, after: fakeSnapshot() };

  // T2 window 多了一个属性 ⇒ 红
  const t2 = { before: clean, after: fakeSnapshot({ props: ['location', 'navigator', 'XMLHttpRequest', 'extra'] }) };

  // T3 window 少了一个属性 ⇒ 红（数量对得上也红：集合必须逐项一致）
  const t3 = { before: clean, after: fakeSnapshot({ props: ['location', 'navigator'] }) };

  // T4 prototype 少了一项 ⇒ 红
  const t4 = { before: clean, after: fakeSnapshot({ protoProps: ['constructor', 'open'] }) };

  // T5 页内 `open` 被换过（跑前 ≠ 跑后）⇒ 红
  const t5 = {
    before: fakeSnapshot({ openText: 'function open() { [native code] }' }),
    after: fakeSnapshot({ openText: 'function open() { /* wrapper */ }' }),
  };

  // T6 留了 `__dsh*` 槽位 ⇒ 红
  const t6 = { before: clean, after: fakeSnapshot({ props: ['location', 'navigator', 'XMLHttpRequest', '__dsh_rt_1'] }) };

  // T7 实测组增了 2，**对照组也自然增长了 2（同样的属性名，站点自己长出来的）** ⇒ 相减后 0 ⇒ **不该红**
  const t7 = {
    before: clean,
    after: fakeSnapshot({ props: ['location', 'navigator', 'XMLHttpRequest', 'site_growth_1', 'site_growth_2'] }),
    baseline: {
      before: fakeSnapshot({ props: ['location', 'navigator', 'XMLHttpRequest'] }),
      after: fakeSnapshot({ props: ['location', 'navigator', 'XMLHttpRequest', 'site_growth_1', 'site_growth_2'] }),
    },
  };

  // T8 无对照组、实测组增量 0 ⇒ 「相减」那条报 skipped，其余全过
  const t8 = { before: clean, after: fakeSnapshot() };

  // T9 实测组多了一个**对照组没有**的属性 ⇒ 归因于实测 ⇒ 必须判红
  const t9 = {
    before: clean,
    after: fakeSnapshot({ props: ['location', 'navigator', 'XMLHttpRequest', 'site_growth_1', 'leak_1'] }),
    baseline: {
      before: fakeSnapshot({ props: ['location', 'navigator', 'XMLHttpRequest'] }),
      after: fakeSnapshot({ props: ['location', 'navigator', 'XMLHttpRequest', 'site_growth_1'] }),
    },
  };

  // ── 形态 B ──────────────────────────────────────────────────────────────
  const slot = '__dsh_rt_9f3c';
  const cleanProps = ['location', 'navigator', 'XMLHttpRequest'];
  const withSlot = fakeSnapshot({ props: [...cleanProps, slot] });

  // B1 中途恰好那一个槽位、取数结束后消失 ⇒ 绿
  const b1 = { before: clean, mid: withSlot, after: fakeSnapshot() };
  // B2 取数结束后槽位仍在 ⇒ 红（取到即删没做到）
  const b2 = { before: clean, mid: withSlot, after: withSlot };
  // B3 中途有**两个** `__dsh*` ⇒ 红（只允许那一个槽位）
  const b3 = {
    before: clean,
    mid: fakeSnapshot({ props: [...cleanProps, slot, '__dsh_rt_dead'] }),
    after: fakeSnapshot(),
  };
  // B4 中途那一个**不是槽位形状** ⇒ 红
  const b4 = {
    before: clean,
    mid: fakeSnapshot({ props: [...cleanProps, '__dsh_other_1'] }),
    after: fakeSnapshot(),
  };
  // B5 中途多了一个**非槽位**的 window 属性 ⇒ 红
  const b5 = {
    before: clean,
    mid: fakeSnapshot({ props: [...cleanProps, slot, 'leak_1'] }),
    after: fakeSnapshot(),
  };
  // B6 缺 mid 快照 ⇒ 红（形态 B 的中途约束没法验 ⇒ 不许假绿）
  const b6 = { before: clean, after: fakeSnapshot() };

  // ── 形态 C ──────────────────────────────────────────────────────────────
  const nativeOpen = 'function open() { [native code] }';
  const nativeSend = 'function send() { [native code] }';
  const wrappedOpen = 'function open() { /* wrapper */ }';
  const wrappedSend = 'function send() { /* wrapper */ }';

  // C1 换过但**已还原**（还原后引用 ＝ 替换前引用）⇒ 绿
  const c1 = {
    before: fakeSnapshot(),
    mid: fakeSnapshot({ openText: wrappedOpen, sendText: wrappedSend }),
    after: fakeSnapshot(),
  };
  // C2 换过**没还原**（跑后仍是替换后的那个引用）⇒ 红
  const c2 = {
    before: fakeSnapshot(),
    mid: fakeSnapshot({ openText: wrappedOpen, sendText: wrappedSend }),
    after: fakeSnapshot({ openText: wrappedOpen, sendText: wrappedSend }),
  };
  // C3 还原后引用 **≠** 替换前引用（还原成了原生、而替换前是站点自己包过的）⇒ 红
  const c3 = {
    before: fakeSnapshot({ openText: 'function open() { /* site wrapper */ }' }),
    mid: fakeSnapshot({ openText: 'function open() { /* ours */ }' }),
    after: fakeSnapshot({ openText: nativeOpen }),
  };
  // C4 形态 C 留了 `__dsh*` 槽位 ⇒ 红
  const c4 = {
    before: clean,
    mid: fakeSnapshot({ openText: wrappedOpen }),
    after: fakeSnapshot({ props: [...cleanProps, slot] }),
  };
  // C5 中途新增了 `window` 自有属性 ⇒ 红
  const c5 = {
    before: clean,
    mid: fakeSnapshot({ openText: wrappedOpen, props: [...cleanProps, 'leak_1'] }),
    after: fakeSnapshot(),
  };

  return [
    { id: 'T1', name: '正常：页内什么都没碰（跑前跑后逐项一致、无 `__dsh*`）', expect: PASS, form: FORM_A, readings: t1 },
    { id: 'T2', name: 'window 多了一个属性 ⇒ 应失败', expect: FAIL, form: FORM_A, readings: t2 },
    { id: 'T3', name: 'window 少了一个属性 ⇒ 应失败（数量对得上也红）', expect: FAIL, form: FORM_A, readings: t3 },
    { id: 'T4', name: 'prototype 少了一项 ⇒ 应失败', expect: FAIL, form: FORM_A, readings: t4 },
    { id: 'T5', name: '页内 `open` 被换过（跑前 ≠ 跑后）⇒ 应失败', expect: FAIL, form: FORM_A, readings: t5 },
    { id: 'T6', name: '留了 `__dsh*` 槽位 ⇒ 应失败', expect: FAIL, form: FORM_A, readings: t6 },
    { id: 'T7', name: '实测组与对照组同样自然增长 2 ⇒ 相减为 0 ⇒ 应通过', expect: PASS, form: FORM_A, readings: t7 },
    { id: 'T8', name: '无对照组、增量 0 ⇒ 应通过（「相减」那条报未执行）', expect: PASS, form: FORM_A, readings: t8 },
    { id: 'T9', name: '实测组多了一个对照组没有的属性 ⇒ 归因于实测 ⇒ 应失败', expect: FAIL, form: FORM_A, readings: t9 },
    { id: 'B1', name: '形态 B：中途恰好那一个临时槽位、取数结束后消失 ⇒ 应通过', expect: PASS, form: FORM_B, readings: b1 },
    { id: 'B2', name: '形态 B：取数结束后槽位仍在 ⇒ 应失败（取到即删没做到）', expect: FAIL, form: FORM_B, readings: b2 },
    { id: 'B3', name: '形态 B：中途有**两个** `__dsh*` ⇒ 应失败（只允许那一个槽位）', expect: FAIL, form: FORM_B, readings: b3 },
    { id: 'B4', name: '形态 B：中途那一个**不是槽位形状** ⇒ 应失败', expect: FAIL, form: FORM_B, readings: b4 },
    { id: 'B5', name: '形态 B：中途多了一个非槽位的 `window` 属性 ⇒ 应失败', expect: FAIL, form: FORM_B, readings: b5 },
    { id: 'B6', name: '形态 B：缺 mid（中途）快照 ⇒ 应失败（不许按「没喂就当没有」判绿）', expect: FAIL, form: FORM_B, readings: b6 },
    { id: 'C1', name: '形态 C：换过但**已还原**（还原后引用 ＝ 替换前引用）⇒ 应通过', expect: PASS, form: FORM_C, readings: c1 },
    { id: 'C2', name: '形态 C：换过**没还原** ⇒ 应失败', expect: FAIL, form: FORM_C, readings: c2 },
    { id: 'C3', name: '形态 C：还原后引用 ≠ 替换前引用 ⇒ 应失败', expect: FAIL, form: FORM_C, readings: c3 },
    { id: 'C4', name: '形态 C：留了 `__dsh*` 槽位 ⇒ 应失败', expect: FAIL, form: FORM_C, readings: c4 },
    { id: 'C5', name: '形态 C：中途新增了 `window` 自有属性 ⇒ 应失败', expect: FAIL, form: FORM_C, readings: c5 },
  ];
}

/**
 * 跑场景表：核「实际判定」是否等于「应当的判定」。
 *
 * @param {Array<object>} [scenarios] - 场景表。
 * @returns {Array<{name: string, status: string, detail: string, checks: Array<object>}>}
 */
export function runScenarios(scenarios = buildScenarios()) {
  return scenarios.map((sc) => {
    const { ok, checks } = judgeTrace(sc.readings, sc.form);
    const behavedAsExpected = sc.expect === PASS ? ok : !ok;
    const bad = checks.filter((c) => c.status === FAIL).map((c) => c.name);
    const skipped = checks.filter((c) => c.status === SKIPPED).map((c) => c.name);
    return {
      name: `P4-2a ${sc.name}`,
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
 * **真机轨**：真实页面 × 被测实现。
 *
 * · **按形态分**：读数里带 `form`（`a`／`b`／`c`）⇒ 按该形态的断言集合判。
 * · **给了读数就真判**（通过 / 失败）；**没给 ⇒ 如实报「未执行」**（不是通过、也不是失败）。
 *
 * @param {object} [readings] - 真机读数（`{form, before, mid?, after, baseline?}`）。
 * @returns {{name: string, status: string, detail: string, checks?: Array<object>}}
 */
export function realRunVerdict(readings = null) {
  const form = TRACE_FORMS.includes(readings?.form) ? readings.form : FORM_A;
  const name = `P4-2z 真机轨（${FORM_LABELS[form]}）`;
  if (!readings || (!readings.before && !readings.after)) {
    return {
      name,
      status: SKIPPED,
      detail:
        '本轮没有真机读数 ⇒ 真机轨未执行（不是通过、也不是失败）｜' +
        '**待跑的那一轮（按形态分）**：' +
        '① **形态 A** ＝ 壳侧 DevTools `Network` 域订阅跑一轮（页内零注入 ⇒ 页内不该有任何变化）；' +
        '② **形态 B** ＝ 页内自取数跑一轮（页内脚本自己取自己回，**中途**允许**且仅允许**那一个 ' +
        `${SLOT_PREFIX}* 临时槽位、**结束后必须消失**）；` +
        '③ **形态 C** ＝ 页内旁听跑一轮（允许换过 `open`／`send`，但**必须还原成替换前那个引用**）｜' +
        '⇒ 用壳内 `cdp_eval`（`cdp_eval::cdp_eval`）读快照（跑前／中途／跑后）｜' +
        '**读数契约**：`{form, before, mid?, after, baseline?}`（`baseline` ＝ 没跑那一轮那个 tab 的' +
        '跑前/跑后，用来扣站点自然增长；**形态 B 的 `mid` 必需**）｜' +
        '**喂读数的入口**：把该 JSON 放到 `DSH_P4_TRACE_READINGS` 指向的路径，再跑本脚本',
    };
  }
  const { ok, checks } = judgeTrace(readings, form);
  return {
    name,
    status: ok ? PASS : FAIL,
    detail: ok
      ? `真机读数已喂入 ⇒ **${FORM_LABELS[form]}** 的痕迹面成立`
      : `真机读数已喂入 ⇒ 存在页内痕迹（未过的断言：${
          checks
            .filter((c) => c.status === FAIL)
            .map((c) => c.name)
            .join('、') || '（见 checks）'
        }）`,
    checks,
  };
}

// ── 离线源码形状（形态 B／C 的链）────────────────────────────────────────

/** 离线源码形状的扫描面（相对仓库根）：壳内官方对话实现目录。 */
export const CHAIN_SCAN_DIR = 'src-tauri/src/official_chat';

/** 扫描面里要读的后缀（`.rs` ＝ 壳侧，`.js` ＝ 页内脚本）。 */
export const CHAIN_SCAN_EXTS = Object.freeze(['.rs', '.js']);

/** **页内定时器 API**（形态 B／C 都不许留 ⇒ 页内脚本里不许出现）。 */
const TIMER_APIS = Object.freeze(['setInterval', 'setTimeout']);

/** **写站点数据**的原语（形态 C 的第三条约束：只读）。 */
const SITE_WRITE_PATTERNS = Object.freeze([
  { pattern: /localStorage\s*\.\s*setItem\b/, label: '写 localStorage' },
  { pattern: /localStorage\s*\.\s*removeItem\b/, label: '删 localStorage 的键' },
  { pattern: /localStorage\s*\.\s*clear\b/, label: '清 localStorage' },
  { pattern: /sessionStorage\s*\.\s*(setItem|removeItem|clear)\b/, label: '写 sessionStorage' },
  { pattern: /document\s*\.\s*cookie\s*=/, label: '写 document.cookie' },
  { pattern: /document\s*\.\s*write(ln)?\s*\(/, label: '写 document' },
  { pattern: /indexedDB\b/, label: '碰 indexedDB' },
  { pattern: /\.submit\s*\(\s*\)/, label: '提交表单' },
  { pattern: /\.click\s*\(\s*\)/, label: '点按钮' },
]);

/**
 * 去注释（保留行数对齐）——**共享层**的 `stripComments` 返回的是**行数组**。
 *
 * @param {string} src - 源码文本。
 * @returns {string}
 */
function code(src) {
  return stripCommentLines(src).join('\n');
}

/**
 * 抽 Rust 源码里的**原始字符串字面量**（`r#"…"#`）。
 *
 * @param {string} text - 源码文本。
 * @returns {string[]}
 */
export function extractRawStrings(text) {
  const out = [];
  const re = /r(#+)"([\s\S]*?)"\1/g;
  let m = re.exec(text);
  while (m !== null) {
    out.push(m[2]);
    if (m.index === re.lastIndex) re.lastIndex += 1;
    m = re.exec(text);
  }
  return out;
}

/**
 * 判断一段页内脚本是不是 **IIFE**（不留顶层全局量）。
 *
 * @param {string} script - 页内脚本文本。
 * @returns {boolean}
 */
export function isIifeScript(script) {
  const text = String(script ?? '').trim();
  if (!text) return false;
  const head = [
    /^\(\s*function\b/,
    /^\(\s*async\s+function\b/,
    /^\(\s*\(\s*\)?\s*=>/,
    /^\(\s*async\s*\(\s*\)?\s*=>/,
  ];
  const tail = [/\)\s*\(\s*\)\s*;?\s*$/, /\}\s*\)\s*\(\s*\)\s*;?\s*$/];
  return head.some((re) => re.test(text)) && tail.some((re) => re.test(text));
}

/**
 * 形态 C 里**允许被替换**的目标（协议 §7.1：替换 `XMLHttpRequest.prototype` 的 `open`／`send`，
 * 必要时同 `fetch`；实现上多替换一个 `setRequestHeader` 也在这条规则里）。
 *
 * · `source` ＝ 「从哪儿取替换前那个引用」的正则。
 * · `target` ＝ 「被替换的是谁」的正则。
 */
const HOOK_TARGETS = Object.freeze([
  {
    id: 'XMLHttpRequest.prototype.open',
    source: String.raw`(?:globalThis\.|window\.)?XMLHttpRequest\s*\.\s*prototype\s*\.\s*open\b`,
    target: String.raw`(?:globalThis\.|window\.)?XMLHttpRequest\s*\.\s*prototype\s*\.\s*open`,
  },
  {
    id: 'XMLHttpRequest.prototype.send',
    source: String.raw`(?:globalThis\.|window\.)?XMLHttpRequest\s*\.\s*prototype\s*\.\s*send\b`,
    target: String.raw`(?:globalThis\.|window\.)?XMLHttpRequest\s*\.\s*prototype\s*\.\s*send`,
  },
  {
    id: 'XMLHttpRequest.prototype.setRequestHeader',
    source: String.raw`(?:globalThis\.|window\.)?XMLHttpRequest\s*\.\s*prototype\s*\.\s*setRequestHeader\b`,
    target: String.raw`(?:globalThis\.|window\.)?XMLHttpRequest\s*\.\s*prototype\s*\.\s*setRequestHeader`,
  },
  {
    id: 'window.fetch',
    source: String.raw`(?:globalThis\.|window)\s*\.\s*fetch\b`,
    target: String.raw`(?:globalThis\.|window)\s*\.\s*fetch`,
  },
]);

/**
 * 归一化一个绑定文本（去掉空白 ⇒ `prevXhr . open` 与 `prevXhr.open` 同一个）。
 *
 * @param {string} text - 绑定文本。
 * @returns {string}
 */
function normalizeBinding(text) {
  return String(text ?? '').replace(/\s+/g, '');
}

/**
 * 某个目标「替换前那个引用」被存进了哪些**绑定**（变量名／属性路径）。
 *
 * 认三种写法（真身三种都用过）：
 * ① `var origOpen = XMLHttpRequest.prototype.open;` ⇒ 绑定 `origOpen`
 * ② `prevXhr = { open: XMLHttpRequest.prototype.open, … };` ⇒ 绑定 `prevXhr.open`
 * ③ `prevXhr.open = XMLHttpRequest.prototype.open;` ⇒ 绑定 `prevXhr.open`
 *
 * @param {string} script - 页内脚本文本。
 * @param {{source: string}} target - 目标。
 * @returns {Set<string>} 绑定集合。
 */
export function capturedBindings(script, target) {
  const bindings = new Set();
  const src = target.source;
  // ① 变量
  const re1 = new RegExp(String.raw`\b([A-Za-z_$][\w$]*)\s*=\s*${src}`, 'g');
  let m = re1.exec(script);
  while (m !== null) {
    bindings.add(normalizeBinding(m[1]));
    m = re1.exec(script);
  }
  // ② 对象字面量里的属性
  const re2 = new RegExp(String.raw`\b([A-Za-z_$][\w$]*)\s*=\s*\{([^}]*)\}`, 'g');
  m = re2.exec(script);
  while (m !== null) {
    const holder = m[1];
    const body = m[2];
    const reProp = new RegExp(String.raw`([A-Za-z_$][\w$]*)\s*:\s*${src}`, 'g');
    let p = reProp.exec(body);
    while (p !== null) {
      bindings.add(normalizeBinding(`${holder}.${p[1]}`));
      p = reProp.exec(body);
    }
    m = re2.exec(script);
  }
  // ③ 属性路径
  const re3 = new RegExp(
    String.raw`\b([A-Za-z_$][\w$]*)\s*\.\s*([A-Za-z_$][\w$]*)\s*=\s*${src}`,
    'g',
  );
  m = re3.exec(script);
  while (m !== null) {
    bindings.add(normalizeBinding(`${m[1]}.${m[2]}`));
    m = re3.exec(script);
  }
  return bindings;
}

/**
 * 某个目标被**赋值**过几次（⇒ 它被替换过）。
 *
 * @param {string} script - 页内脚本文本。
 * @param {{target: string}} target - 目标。
 * @returns {number}
 */
export function assignmentCount(script, target) {
  const re = new RegExp(String.raw`${target.target}\s*=(?!=)`, 'g');
  let count = 0;
  let m = re.exec(script);
  while (m !== null) {
    count += 1;
    m = re.exec(script);
  }
  return count;
}

/**
 * 某个目标被**赋回去**的绑定文本（还原动作的右值）。
 *
 * 认两种写法：① `XMLHttpRequest.prototype.open = prevXhr.open;`
 *            ② `Object.defineProperty(XMLHttpRequest.prototype, 'open', { value: prevXhr.open })`
 *
 * @param {string} script - 页内脚本文本。
 * @param {{target: string}} target - 目标。
 * @returns {Set<string>} 右值文本集合（**归一化**后）。
 */
export function restoreBindings(script, target) {
  const out = new Set();
  const rhs = String.raw`([A-Za-z_$][\w$]*(?:\s*\.\s*[A-Za-z_$][\w$]*)?)`;
  const re = new RegExp(String.raw`${target.target}\s*=(?!=)\s*${rhs}`, 'g');
  let m = re.exec(script);
  while (m !== null) {
    out.add(normalizeBinding(m[1]));
    m = re.exec(script);
  }
  const re2 = new RegExp(
    String.raw`Object\s*\.\s*defineProperty\s*\(\s*${target.target}\s*,\s*['"][^'"]+['"]\s*,\s*\{[^}]*?value\s*:\s*${rhs}`,
    'g',
  );
  m = re2.exec(script);
  while (m !== null) {
    out.add(normalizeBinding(m[1]));
    m = re2.exec(script);
  }
  return out;
}

/**
 * 收集离线扫描面里的源码（**只读**；读不动的跳过）。
 *
 * @param {string} root - 仓库根。
 * @param {string} [dir] - 相对目录。
 * @returns {Array<{path: string, code: string}>}
 */
export function collectChainSources(root, dir = CHAIN_SCAN_DIR) {
  const out = [];
  const walk = (absDir, relDir) => {
    let entries = [];
    try {
      entries = readdirSync(absDir, { withFileTypes: true });
    } catch {
      return;
    }
    for (const entry of entries) {
      if (entry.name.startsWith('.')) continue;
      const abs = join(absDir, entry.name);
      const rel = relDir ? `${relDir}/${entry.name}` : entry.name;
      if (entry.isDirectory()) {
        walk(abs, rel);
        continue;
      }
      if (!CHAIN_SCAN_EXTS.some((ext) => entry.name.endsWith(ext))) continue;
      try {
        out.push({ path: `${dir}/${rel}`, code: readFileSync(abs, 'utf8') });
      } catch {
        /* 读不动的文件跳过（判据会因此报「没测到」而不是「测了没过」） */
      }
    }
  };
  walk(join(root, dir), '');
  return out;
}

/**
 * **本文件里定义的函数名**（`fn name(`）—— 用来跟**调用链**。
 *
 * @param {string} plain - 去注释后的源码。
 * @returns {string[]}
 */
export function localFnNames(plain) {
  const out = new Set();
  const re = /\bfn\s+([A-Za-z_][A-Za-z0-9_]*)\s*[<(]/g;
  let m = re.exec(plain);
  while (m !== null) {
    out.add(m[1]);
    if (m.index === re.lastIndex) re.lastIndex += 1;
    m = re.exec(plain);
  }
  return [...out];
}

/**
 * **回程链**：从 `root` 出发、经**本文件内**的函数调用**能到达**的那些函数体。
 *
 * ## 为什么要有它（2026-10-23 实测踩到）
 *
 * 实现把 `round_trip` 的槽位逻辑**抽成了** `round_trip_with`（一份实现、两种读回策略）
 * ⇒ ⇒ 「`round_trip` 的函数体里有 `clear_slot(`」这条**按函数名查**的判据会**假红**
 * （调用搬到了被委托的那个函数里）⇒ ⇒ 判据必须**跟调用链**，而不是跟某个函数名。
 *
 * ⚠️ **只跟本文件内定义的函数**（不跟外部 crate／不跟方法调用）⇒ ⇒ 范围可控。
 *
 * @param {string} plain - 去注释后的源码。
 * @param {string} root - 起点函数名。
 * @param {string[]} names - 本文件里定义的函数名。
 * @returns {Map<string, string>} 函数名 → 函数体。
 */
export function reachBodies(plain, root, names) {
  const bodies = new Map();
  const seen = new Set();
  const stack = [root];
  while (stack.length > 0) {
    const name = stack.pop();
    if (seen.has(name)) continue;
    seen.add(name);
    const body = fnBody(plain, name);
    if (!body) continue;
    bodies.set(name, body);
    for (const other of names) {
      if (other === name || seen.has(other)) continue;
      if (new RegExp(`\\b${other}\\s*[<(]`).test(body)) stack.push(other);
    }
  }
  return bodies;
}

/**
 * **形态 B 的链**（离线源码形状）：注入脚本 IIFE ／ 无定时器 ／ 槽位取到即删。
 *
 * @param {Array<{path: string, code: string}>} sources - 扫描面。
 * @returns {{ok: boolean, checks: Array<{name: string, status: string, detail: string}>}}
 */
export function judgeFormBChain(sources) {
  const list = Array.isArray(sources) ? sources : [];
  const checks = [];
  const add = (name, status, detail) => checks.push({ name, status, detail });

  // 作用域 ＝ **槽位机制**所在的那个文件（按机制函数名认，不按前缀认 ⇒ 前缀一改也认得出）。
  const scope = list.filter(
    (s) => /fn\s+build_slot_script\s*[<(]/.test(code(s.code)) || /fn\s+clear_slot\s*[<(]/.test(code(s.code)),
  );
  if (scope.length === 0) {
    add(
      '形态 B · 链已落地（页内槽位机制）',
      SKIPPED,
      `扫描面里没有槽位机制（\`build_slot_script\`／\`clear_slot\`）⇒ 形态 B 的链还没落地 ⇒ 判据未执行（不假绿）`,
    );
    return { ok: false, checks };
  }

  const file = scope[0];
  const plain = code(file.code);
  const body = fnBody(plain, 'build_slot_script');
  const raw = extractRawStrings(body)
    .filter((s) => s.includes('window'))
    .sort((a, b) => b.length - a.length)[0] ?? '';
  // Rust `format!` 模板里的 `{{`／`}}` 是转义 ⇒ 先还原成真的括号再判形状。
  const script = raw.replace(/\{\{/g, '{').replace(/\}\}/g, '}').trim();

  add(
    '形态 B · 注入脚本是 IIFE（不留顶层全局量）',
    script && isIifeScript(script) ? PASS : FAIL,
    script
      ? `注入脚本开头=${JSON.stringify(script.slice(0, 24))}／结尾=${JSON.stringify(script.slice(-12))}`
      : `抽不到注入脚本（${file.path} 的 \`build_slot_script\` 里没有页内脚本字面量）`,
  );

  const timers = TIMER_APIS.filter((t) => script.includes(t));
  add(
    '形态 B · 注入脚本里没有定时器（不许留定时器）',
    timers.length === 0 ? PASS : FAIL,
    timers.length
      ? `注入脚本里出现 ${timers.join('、')} ⇒ 会留定时器（形态 B 不许）`
      : '注入脚本里没有 setInterval／setTimeout',
  );

  // ⚠️ **跟调用链**（不是跟某个函数名）：实现把槽位逻辑抽成了 `round_trip_with`
  //   （一份实现、两种读回策略）⇒ ⇒ 按函数名查会假红（调用搬到了被委托的那个函数里）。
  const chain = reachBodies(plain, 'round_trip', localFnNames(plain));
  const clearBody = fnBody(plain, 'clear_slot');
  const calls = [...chain.values()].some((body) => /clear_slot\s*\(/.test(body));
  const deletes = /delete\s+window\[/.test(clearBody);
  add(
    '形态 B · 取数结束后槽位被清（`clear_slot` 被调用且删的是那个槽位）',
    calls && deletes ? PASS : FAIL,
    `回程链（round_trip 及其委托的实现：${[...chain.keys()].join('、')}）里 clear_slot 调用：` +
      `${calls ? '有' : '无'}；clear_slot 里 delete window[…]：${deletes ? '有' : '无'}`,
  );

  const prefixOk = plain.includes(`"${SLOT_PREFIX}`) || plain.includes(SLOT_PREFIX);
  add(
    `形态 B · 槽位名前缀固定 \`${SLOT_PREFIX}\`（与判据常量一致）`,
    prefixOk ? PASS : FAIL,
    prefixOk
      ? `槽位名仍带 ${SLOT_PREFIX} 前缀（真机轨的中途快照靠它认出「那一个槽位」）`
      : `源码里找不到 ${SLOT_PREFIX} 前缀 ⇒ 真机轨认不出槽位 ⇒ 判据常量与实现脱节`,
  );

  const judged = checks.filter((c) => c.status !== SKIPPED);
  return { ok: judged.length > 0 && judged.every((c) => c.status === PASS), checks };
}

/**
 * **形态 C 的链**（离线源码形状）：hook 脚本 IIFE ／ 有还原动作 ／ 无定时器 ／ 不写站点数据。
 *
 * @param {Array<{path: string, code: string}>} sources - 扫描面。
 * @returns {{ok: boolean, checks: Array<{name: string, status: string, detail: string}>}}
 */
export function judgeFormCChain(sources) {
  const list = Array.isArray(sources) ? sources : [];
  const checks = [];
  const add = (name, status, detail) => checks.push({ name, status, detail });

  // 作用域 ＝ **页内 hook 形状**所在的那个文件（按形状认 ⇒ 文件名待实现落地后不用改这里）。
  const scope = list.filter((s) => code(s.code).includes('XMLHttpRequest.prototype'));
  if (scope.length === 0) {
    add(
      '形态 C · 链已落地（页内 hook 脚本）',
      SKIPPED,
      '扫描面里没有页内 hook 形状（`XMLHttpRequest.prototype`）⇒ 形态 C 的链还没落地 ⇒ 判据未执行（不假绿）',
    );
    return { ok: false, checks };
  }

  const file = scope[0];
  const plain = code(file.code);
  const script = file.path.endsWith('.js')
    ? plain
    : extractRawStrings(plain)
        .filter((s) => s.includes('XMLHttpRequest.prototype'))
        .sort((a, b) => b.length - a.length)[0] ?? '';

  add(
    '形态 C · hook 脚本是 IIFE（不留顶层全局量）',
    script && isIifeScript(script) ? PASS : FAIL,
    script
      ? `hook 脚本开头=${JSON.stringify(script.trim().slice(0, 24))}／结尾=${JSON.stringify(script.trim().slice(-12))}`
      : `抽不到 hook 脚本（${file.path} 里没有含 \`XMLHttpRequest.prototype\` 的脚本字面量）`,
  );

  const replaced = [];
  const missing = [];
  const shape = [];
  for (const target of HOOK_TARGETS) {
    const assigns = assignmentCount(script, target);
    if (assigns === 0) continue; // 没被替换 ⇒ 这条规则不管它
    replaced.push(target.id);
    const captures = capturedBindings(script, target);
    const restores = restoreBindings(script, target);
    const hit = [...restores].find((r) => captures.has(r));
    if (!hit) missing.push(target.id);
    shape.push(
      `${target.id}：捕获 ${[...captures].join('/') || '（无）'}／赋回 ${[...restores].join('/') || '（无）'}`,
    );
  }
  add(
    '形态 C · 有「还原」动作（`open`／`send` 恢复成**替换前那个引用**）',
    replaced.length > 0 && missing.length === 0 ? PASS : FAIL,
    replaced.length === 0
      ? '脚本里没有对 `XMLHttpRequest.prototype`／`window.fetch` 的替换 ⇒ 它不是形态 C 的 hook'
      : `替换了 ${replaced.join('、')}；${shape.join('；')}` +
        (missing.length ? ` ⇒ **${missing.join('、')} 没有还原成替换前那个引用**` : ''),
  );

  const timers = TIMER_APIS.filter((t) => script.includes(t));
  add(
    '形态 C · hook 脚本里没有定时器（不许留定时器）',
    timers.length === 0 ? PASS : FAIL,
    timers.length
      ? `hook 脚本里出现 ${timers.join('、')} ⇒ 会留定时器（形态 C 不许）`
      : 'hook 脚本里没有 setInterval／setTimeout',
  );

  const writes = SITE_WRITE_PATTERNS.filter((p) => p.pattern.test(script));
  add(
    '形态 C · 不写站点数据（只读）',
    writes.length === 0 ? PASS : FAIL,
    writes.length
      ? `hook 脚本里出现写站点数据的原语：${writes.map((w) => w.label).join('、')}`
      : 'hook 脚本里没有写站点存储/文档/表单的原语',
  );

  const judged = checks.filter((c) => c.status !== SKIPPED);
  return { ok: judged.length > 0 && judged.every((c) => c.status === PASS), checks };
}

/**
 * 汇总计数（与 P1/P2/P3 判据的产物同构）。
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
 * 主入口。
 *
 * @param {string[]} [argv] - 命令行参数（默认 `process.argv.slice(2)`）。
 * @returns {{results: Array<object>, summary: object}}
 */
export async function main(argv = process.argv.slice(2)) {
  const roundIndex = argv.findIndex((a) => a === '--round' || String(a).startsWith('--round='));
  let round = 'p4c2';
  if (roundIndex >= 0) {
    const inline = String(argv[roundIndex]).split('=')[1];
    const next = argv[roundIndex + 1];
    round = inline ?? (next !== undefined && !String(next).startsWith('--') ? next : 'p4c2');
  }
  const asJson = argv.includes('--json');

  // **真机读数入口**（协议 §10 第 2 条：「真机轨 ＝ 壳内 `cdp_eval` 读前后快照」）
  // ⇒ ⇒ 给了读数就真判；没给就报「未执行」⇒ **不许**因为没读数就假绿、也不许硬红。
  const readingsPath = process.env.DSH_P4_TRACE_READINGS || '';
  let realReadings = null;
  let realReadingsNote = '';
  if (readingsPath) {
    try {
      realReadings = JSON.parse(readFileSync(readingsPath, 'utf8'));
    } catch (err) {
      realReadingsNote = `｜⚠️ 读数文件读不动（${readingsPath}）：${err?.message ?? err}`;
    }
  }

  const results = runScenarios();

  // **离线源码形状**（形态 B／C 的链）—— 真机轨之外的那一半，能离线跑。
  const repoRoot = fileURLToPath(new URL('../', import.meta.url)).replace(/[\\/]+$/, '');
  const chainSources = collectChainSources(repoRoot);
  for (const chain of [judgeFormBChain(chainSources), judgeFormCChain(chainSources)]) {
    for (const c of chain.checks) {
      results.push({ name: c.name, status: c.status, detail: c.detail });
    }
  }

  const realVerdict = realRunVerdict(realReadings);
  if (realReadingsNote) realVerdict.detail += realVerdictNote;
  results.push(realVerdict);
  const summary = summarize(results);

  const evidenceDir = join(tmpdir(), 'dsh-p4-trace');
  mkdirSync(evidenceDir, { recursive: true });
  const outPath = join(evidenceDir, `results-${round}.json`);
  writeFileSync(
    outPath,
    `${JSON.stringify({ round, at: new Date().toISOString(), scanned: chainSources.map((s) => s.path), results, summary }, null, 2)}\n`,
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
        `扫描面：\`${CHAIN_SCAN_DIR}/**\`（离线**源码形状**：形态 B／C 的链）＋ 真机轨（运行期判据，源码里量不出来）\n` +
        `扫了 ${chainSources.length} 个文件 · 证据：${outPath}\n`,
    );
  }
  return { results, summary };
}

// 直接跑（被 import 时不跑）
if (process.argv[1] && process.argv[1].endsWith('check-official-chat-p4-trace.mjs')) {
  await main();
}
