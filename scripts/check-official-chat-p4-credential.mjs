// P4 协议 §10 判据**第 9 条**（凭据机制）—— 按源码形态断：9a 不自拼凭据头 / 9b 不读站点凭据 /
// 9c 页外不调站点 API / 9d 通道形态（**占位**，待 P4 实现落地后由 PM 通知再补断言）。
// ⚠️ 2026-10-21 16:1x 订正：9d 等的**不是** §3.1 —— §3.1（凭据机制）在 v0.7 **已定案**；
//    9d 等的是「**页内求值通道形态**」（槽位式 / CDP 零痕迹求值 / 替换 prototype 后还原），
//    协议 §10 第 9 条的指针是 §7.1 之外的这一项 ⇒ 与判据 2 的真机轨是**同一个待定项**。
//
// ## 依据
//
// · 协议 §3.1（凭据机制）、§7.2（凭据零落盘零外传）、§1 裁定 1（页内同源 fetch 由页内发起）。
// · PM 起草的 `p4-criterion9-draft.md`（素材区，不入库）—— 条文与四个合成用例照它落。
//
// ## 纪律（沿用本仓既有判据）
//
// · **自包含**：任意干净 checkout 上独立跑通，不依赖共享工作树在途改动。
// · **零污染**：夹具落 `%TEMP%` 一次性目录，跑完删自己建的；**不读用户真实数据目录**。
// · **只断形态**：只看**源码文本形态**（头名 / 取法 / 调用位置）⇒ **不发任何网络请求**、不碰凭据值。
// · **三态 verdict**：`pass` / `fail` / `skipped`。「没扫到可判的源码」必须显式报 `skipped`，
//   不能省略、也不能记成 `fail`（否则「这条没测到」和「这条测了没过」长得一样）。
//
// ## 三个「口径未定 ⇒ 判据不钉」的地方（避免假红）
//
// 1. **9d 的通道形态**：等的是「**页内求值通道形态**」（槽位式 / CDP 零痕迹求值 / 替换
//    prototype 后还原）⇒ 协议 §3.1（凭据机制）**已在 v0.7 定案**，它不是 9d 的待定项 ⇒ ⇒
//    9d **恒为 `skipped`**，不自行发明形态（**与判据 2 的真机轨是同一个待定项**）。
// 2. **`__appKit_userInfo` 不在凭据键清单里**：实测该键的形状是 `{value:{id}, __version}`
//    （**只含用户 id，不含 token**）⇒ 把它当凭据键会造一条**假红**。若协议 §3.1 裁定它是凭据，
//    把 `CRED_KEYS` 里加回 `'__appKit_userInfo'` 即可（**改一处常量，不改判定逻辑**）。
// 3. **9c 只断「页外」**：协议 §1 明写「API 调用在页内、同源 fetch」⇒ **页内**的
//    `fetch('/api/v0/…')` 是**协议要求的**，判它红是错的 ⇒ 9c 只断**页外**形态
//    （宿主侧代码里出现页外网络原语 ＋ 站点标识字面量 / 绝对主机名的请求调用）。
//
// ## 扫描面（按仓库现状校准，**待协议 §3.1 定稿后复核**）
//
// · `src-tauri/src/official_chat/**` —— P4 的页内取数实现主体（壳侧）。
// · `src-tauri/src/plugins/builtin/bridge.rs` —— 回环桥（§5 接线面）。
// · **明确排除**：`src-tauri/examples/**`（**取证探针**，它们自拼 `Authorization` 头是**探针的本职**，
//   不是生产取数路径）｜`plugins/official-chat/**`（**内核侧**插件，不是壳里的页内取数路径）。
//
// @module scripts/check-official-chat-p4-credential

import { mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, relative, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

/** verdict 三态。 */
export const PASS = 'pass';
export const FAIL = 'fail';
export const SKIPPED = 'skipped';

/**
 * **凭据头名**（9a 断这些）。
 *
 * **为什么是这三个**：协议 §3.1 记的站点鉴权头是 `Authorization: Bearer <token>`（站点自己的
 * 请求拦截器）＋ 另有 `x-auth-token` 这一类变体 ⇒ 三者合起来覆盖「手工拼凭据头」的常见形态。
 */
export const CRED_HEADERS = Object.freeze(['authorization', 'cookie', 'x-auth-token']);

/**
 * **凭据类键**（9b 断这些；**其它键不判**）。
 *
 * · `userToken` —— 站点把 API token 放在 localStorage 的这个键里（实测：站点自己的请求带的
 *   `Authorization: Bearer <64 字符>`，那 64 字符**含在** `userToken` 的值里）。
 * · `settingsJwt` —— 设置侧 JWT（站点用它派 `x-settings-token` 头）。
 * · `ds_session_id` —— 登录 cookie 名（HttpOnly ⇒ JS 读不到；但源码里出现**这个名字**就说明
 *   在打站点会话的主意 ⇒ 判红）。
 */
export const CRED_KEYS = Object.freeze(['userToken', 'settingsJwt', 'ds_session_id']);

/** **扫描面**（相对仓库根）。 */
export const SCAN_GLOBS = Object.freeze([
  'src-tauri/src/official_chat/**/*.rs',
  'src-tauri/src/plugins/builtin/bridge.rs',
]);

/** **明确排除**的路径前缀（附理由，理由进代码，不靠记忆）。 */
export const EXCLUDED_PREFIXES = Object.freeze([
  ['src-tauri/examples', '取证探针：自拼 Authorization 头是探针的本职，不是生产取数路径'],
  ['plugins/official-chat', '内核侧插件：不是壳里的页内取数路径'],
  ['scripts', '判据与工具自身'],
  ['docs', '文档不是实现'],
]);

/** **页外网络原语**（Rust / JS 两套）：出现它 ＋ 站点标识 ⇒ 9c 候选。 */
const OFFSITE_PRIMITIVES = Object.freeze([
  /\breqwest\b/i,
  /\bureq\b/i,
  /\bhyper::/i,
  /\bTcpStream\b/,
  /\bstd::net::/i,
  /\bnode:https?\b/,
  /\baxios\b/,
  /\bundici\b/,
]);

/** **站点标识**（出现在**字符串字面量**里才算，避免命中注释与标识符）。 */
const SITE_MARKERS = Object.freeze(['chat.deepseek.com', '/api/v0/']);

/**
 * 去掉注释，**保留字符串字面量内容**（9b 要读键名，所以不能像常规 strip 那样清空字符串）。
 *
 * @param {string} src - 源码文本。
 * @returns {string} 去注释后的文本（行数与原文对齐，便于给出行号）。
 */
export function stripComments(src) {
  const text = String(src ?? '');
  let out = '';
  let i = 0;
  const n = text.length;
  while (i < n) {
    const ch = text[i];
    // 字符串（' " `）—— 内部整体跳过（含转义），但**原样保留**内容
    if (ch === "'" || ch === '"' || ch === '`') {
      const quote = ch;
      out += ch;
      i += 1;
      while (i < n) {
        const c = text[i];
        out += c;
        if (c === '\\') {
          if (i + 1 < n) {
            out += text[i + 1];
            i += 2;
            continue;
          }
        }
        if (c === quote) {
          i += 1;
          break;
        }
        i += 1;
      }
      continue;
    }
    // 行注释
    if (ch === '/' && text[i + 1] === '/') {
      while (i < n && text[i] !== '\n') {
        out += ' ';
        i += 1;
      }
      continue;
    }
    // 块注释
    if (ch === '/' && text[i + 1] === '*') {
      out += '  ';
      i += 2;
      while (i < n && !(text[i] === '*' && text[i + 1] === '/')) {
        out += text[i] === '\n' ? '\n' : ' ';
        i += 1;
      }
      out += '  ';
      i += 2;
      continue;
    }
    out += ch;
    i += 1;
  }
  return out;
}

/**
 * 取出行号（1 起）—— 命中位置用它点名。
 *
 * @param {string} text - 文本。
 * @param {number} index - 字符下标。
 * @returns {number} 行号。
 */
function lineAt(text, index) {
  let line = 1;
  for (let i = 0; i < index && i < text.length; i += 1) {
    if (text[i] === '\n') line += 1;
  }
  return line;
}

/**
 * 在一段文本里找所有匹配（带行号）。
 *
 * @param {string} text - 文本。
 * @param {RegExp} re - **必须带 `g` 标志**。
 * @returns {Array<{index: number, line: number, text: string}>}
 */
function findAll(text, re) {
  const out = [];
  const rx = new RegExp(re.source, re.flags.includes('g') ? re.flags : `${re.flags}g`);
  let m = rx.exec(text);
  while (m !== null) {
    out.push({ index: m.index, line: lineAt(text, m.index), text: m[0].replace(/\s+/g, ' ').trim() });
    if (m.index === rx.lastIndex) rx.lastIndex += 1;
    m = rx.exec(text);
  }
  return out;
}

/**
 * 扫一份源码，返回 9a/9b/9c 的命中。
 *
 * @param {string} src - 源码文本。
 * @param {string} [file] - 文件名（写进命中信息）。
 * @returns {Array<{id: string, detail: string, line: number, file: string}>}
 */
export function scanSource(src, file = '<inline>') {
  const code = stripComments(String(src ?? ''));
  const hits = [];

  // ── 9a：headers 语境里出现凭据头名 ──────────────────────────────
  for (const h of CRED_HEADERS) {
    // ① 语境限定：headers / Header 附近出现该头名（**带引号或不带引号的键都算**）
    const re = new RegExp(
      String.raw`(headers?|setRequestHeader|headersList)\b[^\n]{0,160}['"\`]?\s*${h}\s*['"\`]?\s*[:=,}]`,
      'gi',
    );
    for (const m of findAll(code, re)) {
      hits.push({ id: '9a', detail: `自拼凭据头「${h}」`, line: m.line, file, snippet: m.text });
    }
    // ② 显式调用形态：setRequestHeader('authorization', …) / .header("authorization", …) / insert("authorization", …)
    const re2 = new RegExp(
      String.raw`(setRequestHeader|\.header|\.headers|insert|set|append)\s*\(\s*['"\`]\s*${h}\s*['"\`]`,
      'gi',
    );
    for (const m of findAll(code, re2)) {
      hits.push({ id: '9a', detail: `自拼凭据头「${h}」（显式调用）`, line: m.line, file, snippet: m.text });
    }
  }

  // ── 9b：读凭据类键 / 读 document.cookie ─────────────────────────
  for (const k of CRED_KEYS) {
    const re = new RegExp(
      String.raw`(localStorage|sessionStorage)\s*(\.\s*getItem\s*\(\s*['"\`]\s*${k}\s*['"\`]|\[\s*['"\`]\s*${k}\s*['"\`]\s*\])`,
      'gi',
    );
    for (const m of findAll(code, re)) {
      hits.push({ id: '9b', detail: `读取站点凭据键「${k}」`, line: m.line, file, snippet: m.text });
    }
  }
  // document.cookie 任何读取（HttpOnly 读不到，但源码里读了就是打站点会话的主意）
  for (const m of findAll(code, /document\s*\.\s*cookie\b/gi)) {
    hits.push({ id: '9b', detail: '读取 document.cookie', line: m.line, file, snippet: m.text });
  }

  // ── 9c：页外调站点 API ──────────────────────────────────────────
  const hasOffsitePrimitive = OFFSITE_PRIMITIVES.some((re) => re.test(code));
  if (hasOffsitePrimitive) {
    for (const marker of SITE_MARKERS) {
      // 站点标识必须出现在**字符串字面量**里（不命中注释与标识符）
      const re = new RegExp(String.raw`['"\`][^'"\`\n]{0,200}${marker.replace(/\//g, '\\/')}[^'"\`\n]{0,80}['"\`]`, 'gi');
      for (const m of findAll(code, re)) {
        hits.push({
          id: '9c',
          detail: `页外网络原语 ＋ 站点标识「${marker}」`,
          line: m.line,
          file,
          snippet: m.text,
        });
      }
    }
  }
  // 绝对主机名的请求调用（页外要打站点 API 必然得写绝对主机名；页内同源用相对路径）
  const absHost = /['"`][^'"`\n]{0,200}chat\.deepseek\.com[^'"`\n]{0,120}['"`]/gi;
  for (const m of findAll(code, absHost)) {
    const before = code.slice(Math.max(0, m.index - 120), m.index);
    if (/\b(fetch|axios|https?\.request|request|get|post)\s*\(/i.test(before)) {
      hits.push({ id: '9c', detail: '请求调用打站点绝对主机名（页外形态）', line: m.line, file, snippet: m.text });
    }
  }

  return hits;
}

/**
 * 判一条源码：9a / 9b / 9c 三条各给一个 verdict，9d 恒为 `skipped`（占位）。
 *
 * @param {string} src - 源码文本。
 * @param {string} [file] - 文件名。
 * @returns {Array<{name: string, status: string, detail: string}>}
 */
export function checkSource(src, file = '<inline>') {
  const hits = scanSource(src, file);
  const by = (id) => hits.filter((h) => h.id === id);
  const fmt = (list) =>
    list
      .slice(0, 5)
      .map((h) => `${h.file}:${h.line} ${h.detail}`)
      .join('；') + (list.length > 5 ? ` …共 ${list.length} 处` : '');

  const a = by('9a');
  const b = by('9b');
  const c = by('9c');
  return [
    {
      name: 'P4-9a 不自拼凭据头',
      status: a.length ? FAIL : PASS,
      detail: a.length ? `发现自拼凭据头：${fmt(a)}` : `未发现 Authorization / Cookie / X-Auth-Token 的头名赋值形态`,
    },
    {
      name: 'P4-9b 不读站点凭据',
      status: b.length ? FAIL : PASS,
      detail: b.length
        ? `发现读取站点凭据：${fmt(b)}`
        : `未发现读取凭据类键（${CRED_KEYS.join(' / ')}）或 document.cookie；其它键不判`,
    },
    {
      name: 'P4-9c 页外不调站点 API',
      status: c.length ? FAIL : PASS,
      detail: c.length ? `发现页外调站点 API 的形态：${fmt(c)}` : '未发现页外调站点 API 的形态',
    },
    {
      name: 'P4-9d 通道形态与协议一致',
      status: SKIPPED,
      detail:
        '占位：通道形态（页内求值通道：槽位式 / CDP 零痕迹求值 / 替换 prototype 后还原）待 P4 实现落地后由 PM 通知再补断言（按纪律不自行发明通道形态；与判据 2 的真机轨是同一个待定项）',
    },
  ];
}

/**
 * 按 glob 收集源码（**极简 glob**：只支持 `**` 与 `*`，够用即可，不引第三方依赖）。
 *
 * @param {string} root - 仓库根。
 * @param {string} pattern - 形如 `a/b/` 加 `**` 加 `/*.rs`（只支持 `**` 与 `*`）。
 * @returns {string[]} 命中的文件绝对路径。
 */
function globFiles(root, pattern) {
  const segs = pattern.split('/').filter((s) => s.length > 0);
  if (segs.length === 0) return [];

  /** 段名匹配：含 `*` 时按通配符匹配文件名。 */
  const matchName = (seg, name) => {
    if (!seg.includes('*')) return name === seg;
    const escaped = seg.replace(/[.+^${}()|[\]\\]/g, '\\$&').replace(/\*/g, '.*');
    return new RegExp(`^${escaped}$`).test(name);
  };

  /** 列一个目录下的子目录（**用 entry.name 取名字**，不靠路径切片 —— 根路径可能带尾分隔符）。 */
  const subDirs = (dir) => {
    try {
      return readdirSync(dir, { withFileTypes: true })
        .filter((e) => e.isDirectory())
        .map((e) => ({ name: e.name, path: join(dir, e.name) }));
    } catch {
      return [];
    }
  };

  /** 某目录的全部后代目录（`**` 匹配一层或多层时用）。 */
  const allDescDirs = (dir) => {
    const out = [];
    const stack = subDirs(dir).map((e) => e.path);
    while (stack.length) {
      const cur = stack.pop();
      out.push(cur);
      for (const d of subDirs(cur)) stack.push(d.path);
    }
    return out;
  };

  const fileSeg = segs[segs.length - 1];
  const dirSegs = segs.slice(0, -1);

  // ① 目录部分：逐段展开（`**` ＝ 零层或一层以上）
  let dirs = [root];
  for (const seg of dirSegs) {
    const next = [];
    for (const dir of dirs) {
      if (seg === '**') {
        next.push(dir);
        next.push(...allDescDirs(dir));
        continue;
      }
      for (const candidate of subDirs(dir)) {
        if (matchName(seg, candidate.name)) next.push(candidate.path);
      }
    }
    dirs = next;
  }

  // ② 文件部分：在每个目录里按名字模式取文件
  const out = [];
  for (const dir of dirs) {
    let entries;
    try {
      entries = readdirSync(dir, { withFileTypes: true });
    } catch {
      continue;
    }
    for (const entry of entries) {
      if (entry.isDirectory()) continue;
      if (fileSeg === '**' || matchName(fileSeg, entry.name)) out.push(join(dir, entry.name));
    }
  }
  return out;
}

/**
 * 收集扫描面内的源码。
 *
 * @param {string} root - 仓库根。
 * @param {string[]} [globs] - 覆盖默认扫描面。
 * @returns {Array<{path: string, code: string}>}
 */
export function collectSources(root, globs = SCAN_GLOBS) {
  const seen = new Set();
  const sources = [];
  for (const pattern of globs) {
    for (const file of globFiles(root, pattern)) {
      if (seen.has(file)) continue;
      seen.add(file);
      const rel = relative(root, file).split(sep).join('/');
      if (EXCLUDED_PREFIXES.some(([prefix]) => rel.startsWith(prefix))) continue;
      try {
        sources.push({ path: rel, code: readFileSync(file, 'utf8') });
      } catch {
        /* 读不动的文件跳过 */
      }
    }
  }
  return sources;
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
 * @returns {Promise<{results: Array<object>, summary: object}>}
 */
export async function main(argv = process.argv.slice(2)) {
  const repo = fileURLToPath(new URL('../', import.meta.url));
  const roundIndex = argv.findIndex((a) => a === '--round' || String(a).startsWith('--round='));
  let round = 'p1';
  if (roundIndex >= 0) {
    const inline = String(argv[roundIndex]).split('=')[1];
    const next = argv[roundIndex + 1];
    round = inline ?? (next !== undefined && !String(next).startsWith('--') ? next : 'p1');
  }
  const targetIndex = argv.findIndex((a) => a === '--target' || String(a).startsWith('--target='));
  // 根路径**去掉尾分隔符**（Windows 下 `fileURLToPath` 会带一个尾 `\` ⇒ 路径拼接会多一层）
  let root = repo.replace(/[\\/]+$/, '');
  if (targetIndex >= 0) {
    const inline = String(argv[targetIndex]).split('=')[1];
    const next = argv[targetIndex + 1];
    root = (inline ?? (next !== undefined && !String(next).startsWith('--') ? next : repo)).replace(
      /[\\/]+$/,
      '',
    );
  }
  const asJson = argv.includes('--json');

  const sources = collectSources(root);
  const results = [];
  if (sources.length === 0) {
    // **没扫到可判的源码 ⇒ 显式 skipped，不硬红**（实现尚未到位 ≠ 判据失败）
    for (const spec of [
      ['P4-9a 不自拼凭据头', '未发现可判的源码（扫描面为空）⇒ 判据未执行'],
      ['P4-9b 不读站点凭据', '未发现可判的源码（扫描面为空）⇒ 判据未执行'],
      ['P4-9c 页外不调站点 API', '未发现可判的源码（扫描面为空）⇒ 判据未执行'],
      ['P4-9d 通道形态与协议一致', '占位：通道形态（页内求值通道）待 P4 实现落地后由 PM 通知再补断言'],
    ]) {
      results.push({ name: spec[0], status: SKIPPED, detail: spec[1] });
    }
  } else {
    const merged = new Map();
    for (const source of sources) {
      for (const r of checkSource(source.code, source.path)) {
        const prev = merged.get(r.name);
        if (!prev) merged.set(r.name, { ...r });
        else if (r.status === FAIL && prev.status !== FAIL) merged.set(r.name, { ...r });
      }
    }
    results.push(...merged.values());
  }

  const summary = summarize(results);
  const evidenceDir = join(tmpdir(), 'dsh-p4-credential');
  mkdirSync(evidenceDir, { recursive: true });
  const outPath = join(evidenceDir, `results-${round}.json`);
  writeFileSync(
    outPath,
    `${JSON.stringify({ round, at: new Date().toISOString(), scanned: sources.map((s) => s.path), results, summary }, null, 2)}\n`,
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
        `扫了 ${sources.length} 个文件 · 证据：${outPath}\n`,
    );
  }
  return { results, summary };
}

// 直接跑（被 import 时不跑）
if (process.argv[1] && process.argv[1].endsWith('check-official-chat-p4-credential.mjs')) {
  await main();
}
