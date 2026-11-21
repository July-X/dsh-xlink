/**
 * cfg 门控配套自检：impl 块的门控不得比它实现的类型更宽。
 *
 * ## 为什么需要这道门禁
 *
 * 2026-11-21 的 macOS CI 红灯（run 44859923080）就是这么来的：
 * `src-tauri/src/shell/localtime.rs` 里 `struct TransitionRule` 带
 * `#[cfg(any(windows, test))]`，紧邻的 `impl TransitionRule` 块却没带。
 *
 *     error[E0425]: cannot find type `TransitionRule` in this scope
 *
 * Windows 上 `cfg(windows)` 恒真，看不出来；`cargo test` 因为 `cfg(test)`
 * 也带着这个类型，本机测试照样全绿；只有 macOS 上编 lib（非 test 构建）才炸。
 * **判据必须跨平台穷举，不能只跑宿主平台。**
 *
 * ## 这道门禁判什么
 *
 * 穷举「OS × 架构 × test × debug_assertions × feature」共 80 个环境，
 * 对每个 `impl` 块断言：
 *
 *     **凡是 impl 生效的环境，被实现的类型也必须存在。**
 *
 * 只报「impl 比类型更宽」这一个方向——impl 比类型窄是合法写法
 * （类型存在，但 impl 只在部分平台编译）。
 *
 * ## 为什么它覆盖了 macOS
 *
 * 环境空间里显式枚举了 macOS × 非 test × release/debug_assertions 那一格，
 * 所以在 Windows 开发机上跑也能覆盖当初把 CI 拉红的那一格。
 * 结论与宿主平台无关。
 *
 * ## 保守边界
 *
 * 解析不到的一律跳过并计数上报（跨 crate 类型、同名多候选、未知 cfg 原子、
 * 模块树够不着的文件）：**宁可漏报，不可误报。**
 */

import { readdirSync, readFileSync } from 'node:fs';
import { dirname, join, resolve as resolvePath } from 'node:path';
import { fileURLToPath } from 'node:url';

export const REPO_ROOT = resolvePath(dirname(fileURLToPath(import.meta.url)), '..');

// ── 环境模型 ────────────────────────────────────────────────────────────────

/** 目标 OS 的枚举取值。非 windows/macos/linux 的 unix 家族单列一档。 */
export const OS_VARIANTS = [
  { os: 'windows', target_os: 'windows', unix: false, windows: true },
  { os: 'macos', target_os: 'macos', unix: true, windows: false },
  { os: 'linux', target_os: 'linux', unix: true, windows: false },
  { os: 'other', target_os: null, unix: false, windows: false },
  { os: 'other_unix', target_os: null, unix: true, windows: false },
];

export function buildEnvironments() {
  const envs = [];
  for (const o of OS_VARIANTS) {
    for (const arch of ['x86_64', 'aarch64']) {
      for (const test of [false, true]) {
        for (const debug of [false, true]) {
          for (const wdio of [false, true]) {
            envs.push({
              index: envs.length,
              os: o.os,
              target_os: o.target_os,
              unix: o.unix,
              windows: o.windows,
              arch,
              test,
              debug_assertions: debug,
              features: new Set(wdio ? ['wdio'] : []),
            });
          }
        }
      }
    }
  }
  return envs;
}

// ── cfg 谓词 ────────────────────────────────────────────────────────────────

const KNOWN_FLAGS = new Set(['windows', 'unix', 'test', 'debug_assertions']);
const KNOWN_KEYS = new Set(['target_os', 'target_arch', 'feature']);

export function parseCfgPredicate(text) {
  const s = String(text);
  let pos = 0;
  const skipWs = () => {
    while (pos < s.length && /\s/.test(s[pos])) pos += 1;
  };
  const readIdent = () => {
    skipWs();
    const start = pos;
    while (pos < s.length && /[A-Za-z0-9_]/.test(s[pos])) pos += 1;
    return s.slice(start, pos);
  };
  const readString = () => {
    skipWs();
    if (s[pos] !== '"') return null;
    pos += 1;
    const start = pos;
    while (pos < s.length && s[pos] !== '"') pos += 1;
    const value = s.slice(start, pos);
    pos += 1;
    return value;
  };

  function parseExpr() {
    const name = readIdent();
    skipWs();
    if (name === 'all' || name === 'any') {
      if (s[pos] !== '(') return { kind: 'unknown', name };
      pos += 1;
      const children = [];
      skipWs();
      if (s[pos] === ')') {
        pos += 1;
        return { kind: name, children };
      }
      for (;;) {
        children.push(parseExpr());
        skipWs();
        if (s[pos] === ',') {
          pos += 1;
          continue;
        }
        if (s[pos] === ')') pos += 1;
        break;
      }
      return { kind: name, children };
    }
    if (name === 'not') {
      if (s[pos] !== '(') return { kind: 'unknown', name };
      pos += 1;
      const child = parseExpr();
      skipWs();
      if (s[pos] === ')') pos += 1;
      return { kind: 'not', child };
    }
    if (!name) return { kind: 'unknown', name: '<空>' };
    skipWs();
    if (s[pos] === '=') {
      pos += 1;
      const value = readString();
      if (value === null) return { kind: 'unknown', name };
      if (!KNOWN_KEYS.has(name)) return { kind: 'unknown', name: `${name} = "${value}"` };
      return { kind: 'key', key: name, value };
    }
    if (!KNOWN_FLAGS.has(name)) return { kind: 'unknown', name };
    return { kind: 'flag', name };
  }

  return parseExpr();
}

export function evalCfg(node, env) {
  switch (node.kind) {
    case 'all': {
      let unknown = false;
      for (const child of node.children) {
        const value = evalCfg(child, env);
        if (value === false) return false;
        if (value === 'unknown') unknown = true;
      }
      return unknown ? 'unknown' : true;
    }
    case 'any': {
      let unknown = false;
      for (const child of node.children) {
        const value = evalCfg(child, env);
        if (value === true) return true;
        if (value === 'unknown') unknown = true;
      }
      return unknown ? 'unknown' : false;
    }
    case 'not': {
      const value = evalCfg(node.child, env);
      return value === 'unknown' ? 'unknown' : !value;
    }
    case 'flag':
      if (node.name === 'windows') return env.windows;
      if (node.name === 'unix') return env.unix;
      if (node.name === 'test') return env.test;
      if (node.name === 'debug_assertions') return env.debug_assertions;
      return 'unknown';
    case 'key':
      if (node.key === 'target_os') {
        return env.target_os !== null && env.target_os === node.value;
      }
      if (node.key === 'target_arch') return env.arch === node.value;
      if (node.key === 'feature') return env.features.has(node.value);
      return 'unknown';
    default:
      return 'unknown';
  }
}

// ── Rust 源码遮罩 ───────────────────────────────────────────────────────────

/**
 * 把注释、字符串、字符字面量涂掉，**保留字符偏移与换行**。
 * 没有这一步，注释里写的 `#[cfg(...)]` 会被当成真属性，字符串里的 `{`
 * 会把括号配平数错、误判 impl 块归属的模块。
 */
export function maskRust(source) {
  const out = source.split('');
  const n = source.length;
  const blank = (from, to) => {
    for (let k = from; k < to && k < n; k += 1) if (source[k] !== '\n') out[k] = ' ';
  };
  let i = 0;
  while (i < n) {
    const c = source[i];
    if (c === '/' && source[i + 1] === '/') {
      let j = i;
      while (j < n && source[j] !== '\n') j += 1;
      blank(i, j);
      i = j;
      continue;
    }
    if (c === '/' && source[i + 1] === '*') {
      let depth = 1;
      let j = i + 2;
      while (j < n && depth > 0) {
        if (source[j] === '/' && source[j + 1] === '*') {
          depth += 1;
          j += 2;
        } else if (source[j] === '*' && source[j + 1] === '/') {
          depth -= 1;
          j += 2;
        } else {
          j += 1;
        }
      }
      blank(i, j);
      i = j;
      continue;
    }
    if (c === 'r' && (source[i + 1] === '"' || source[i + 1] === '#')) {
      let j = i + 1;
      let hashes = 0;
      while (source[j] === '#') {
        hashes += 1;
        j += 1;
      }
      if (source[j] === '"') {
        j += 1;
        const term = '"' + '#'.repeat(hashes);
        const end = source.indexOf(term, j);
        const stop = end < 0 ? n : end + term.length;
        blank(i, stop);
        i = stop;
        continue;
      }
    }
    if (c === '"' || c === "'") {
      const quote = c;
      if (quote === "'") {
        const m = /^'(\\.|[^\\'])'/.exec(source.slice(i));
        if (!m) {
          out[i] = ' ';
          i += 1;
          continue;
        }
        blank(i, i + m[0].length);
        i += m[0].length;
        continue;
      }
      let j = i + 1;
      while (j < n && source[j] !== '"') {
        if (source[j] === '\\') j += 1;
        j += 1;
      }
      j = Math.min(j + 1, n);
      blank(i, j);
      i = j;
      continue;
    }
    i += 1;
  }
  return out.join('');
}

// ── 单文件解析 ──────────────────────────────────────────────────────────────

const countChar = (text, ch) => {
  let total = 0;
  for (const c of text) if (c === ch) total += 1;
  return total;
};

/**
 * 把跨行的属性块并成一条逻辑行（其余行原样保留）。
 *
 * **遮罩版与原文版并行**：遮罩版用来判断「这一行是不是属性」，
 * 原文版用来取属性里的值——`#[path = "x.rs"]` 的路径和
 * `target_os = "macos"` 的目标名都在字符串里，遮罩会把它们抹成空白。
 */
function mergeEntries(maskedLines, rawLines) {
  const entries = [];
  for (let i = 0; i < maskedLines.length; i += 1) {
    let text = maskedLines[i];
    let raw = rawLines[i] ?? '';
    const line = i + 1;
    if (text.trim().startsWith('#[')) {
      let opens = countChar(text, '[');
      let closes = countChar(text, ']');
      while (opens > closes && i + 1 < maskedLines.length) {
        i += 1;
        text += ' ' + maskedLines[i].trim();
        raw += ' ' + (rawLines[i] ?? '').trim();
        opens += countChar(maskedLines[i], '[');
        closes += countChar(maskedLines[i], ']');
      }
    }
    entries.push({ text, raw, line });
  }
  return entries;
}

function stripLeadingGenerics(text) {
  const t = text.trim();
  if (!t.startsWith('<')) return t;
  let depth = 0;
  for (let i = 0; i < t.length; i += 1) {
    if (t[i] === '<') depth += 1;
    else if (t[i] === '>') {
      depth -= 1;
      if (depth === 0) return t.slice(i + 1).trim();
    }
  }
  return t;
}

function lastSegment(text) {
  let t = stripLeadingGenerics(text.trim());
  t = t.replace(/^[&*]+/, '');
  t = t.replace(/<.*$/, '');
  t = t.trim();
  const seg = t.split('::').pop() ?? '';
  return seg.replace(/[^A-Za-z0-9_]/g, '');
}

export function parseImplHeader(rawHeader) {
  let header = stripLeadingGenerics(rawHeader.trim());
  const whereIdx = header.search(/\bwhere\b/);
  if (whereIdx >= 0) header = header.slice(0, whereIdx);
  let angle = 0;
  let forIdx = -1;
  for (let i = 0; i < header.length; i += 1) {
    const c = header[i];
    if (c === '<') angle += 1;
    else if (c === '>') angle -= 1;
    else if (c === 'f' && angle === 0 && header.startsWith('for', i)) {
      const prev = header[i - 1] ?? ' ';
      const next = header[i + 3] ?? ' ';
      if (/\s/.test(prev) && /\s/.test(next)) {
        forIdx = i;
        break;
      }
    }
  }
  if (forIdx >= 0) {
    return {
      traitName: lastSegment(header.slice(0, forIdx)) || null,
      subject: lastSegment(header.slice(forIdx + 3)),
    };
  }
  return { traitName: null, subject: lastSegment(header) };
}

export function parseRustFile(source) {
  const entries = mergeEntries(maskRust(source).split('\n'), source.split('\n'));
  const items = [];
  const mods = [];
  const stack = [];
  let depth = 0;

  /** 往上收紧邻的属性：cfg 进 gates，path 记下来，其余属性只当行号占位。 */
  const collectAttrs = (idx) => {
    const gates = [];
    const raws = [];
    let path = null;
    for (let j = idx - 1; j >= 0; j -= 1) {
      const t = entries[j].text.trim();
      const raw = entries[j].raw.trim();
      if (t === '') continue;
      if (t.startsWith('///') || t.startsWith('//!')) continue;
      if (t.startsWith('#[cfg(')) {
        const open = raw.indexOf('(');
        const close = raw.lastIndexOf(')');
        if (open >= 0 && close > open) {
          gates.unshift(parseCfgPredicate(raw.slice(open + 1, close)));
          raws.unshift(raw);
        }
        continue;
      }
      if (t.startsWith('#[')) {
        const pm = /^#\[path\s*=\s*"([^"]*)"\]$/.exec(raw);
        if (pm) path = pm[1];
        continue;
      }
      break;
    }
    return { gates, raws, path };
  };

  for (let i = 0; i < entries.length; i += 1) {
    const { text, line } = entries[i];
    const depthBefore = depth;
    const scopeGates = stack.flatMap((frame) => frame.gates);
    const scopePath = stack.map((frame) => frame.name);
    const attrs = collectAttrs(i);

    const modMatch = /^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+([A-Za-z_]\w*)\s*([{;])/.exec(text);
    if (modMatch) {
      const inline = modMatch[2] === '{';
      mods.push({
        name: modMatch[1],
        inline,
        pathAttr: attrs.path,
        ownGates: attrs.gates,
        ownRaw: attrs.raws,
        scopeGates,
        scopePath,
        line,
      });
      if (inline) stack.push({ name: modMatch[1], gates: attrs.gates, depth: depthBefore });
    } else {
      const typeMatch = /^\s*(?:pub(?:\([^)]*\))?\s+)?(?:unsafe\s+)?(struct|enum|trait|union)\s+([A-Za-z_]\w*)/.exec(text);
      if (typeMatch) {
        items.push({
          kind: 'type',
          typeKind: typeMatch[1],
          name: typeMatch[2],
          ownGates: attrs.gates,
          ownRaw: attrs.raws,
          scopeGates,
          scopePath,
          line,
        });
      }
      if (/^\s*(?:unsafe\s+)?impl\b/.test(text)) {
        let header = text.replace(/^\s*(?:unsafe\s+)?impl\b/, '');
        let j = i;
        while (!header.includes('{') && j + 1 < entries.length) {
          j += 1;
          header += ' ' + entries[j].text.trim();
        }
        const braceIdx = header.indexOf('{');
        if (braceIdx >= 0) header = header.slice(0, braceIdx);
        const { traitName, subject } = parseImplHeader(header);
        if (subject) {
          items.push({
            kind: 'impl',
            name: subject,
            subject,
            traitName,
            ownGates: attrs.gates,
            ownRaw: attrs.raws,
            scopeGates,
            scopePath,
            line,
          });
        }
      }
    }

    depth += countChar(text, '{') - countChar(text, '}');
    while (stack.length && stack[stack.length - 1].depth >= depth) stack.pop();
  }

  return { items, mods };
}

// ── 模块树 ──────────────────────────────────────────────────────────────────

function dirnameOf(rel) {
  const idx = rel.lastIndexOf('/');
  return idx < 0 ? '' : rel.slice(0, idx);
}

/** 相对路径拼接（仓库内部一律用正斜杠相对路径，不碰 CWD）。 */
function joinRel(base, rel) {
  const parts = base ? base.split('/') : [];
  for (const seg of String(rel).split('/')) {
    if (seg === '' || seg === '.') continue;
    if (seg === '..') parts.pop();
    else parts.push(seg);
  }
  return parts.join('/');
}

function resolveModTarget(relFile, mod, moduleDir, known) {
  if (mod.pathAttr) {
    const target = joinRel(dirnameOf(relFile), mod.pathAttr);
    return known.has(target) ? target : null;
  }
  const flat = `${moduleDir}${mod.name}.rs`;
  if (known.has(flat)) return flat;
  const nested = `${moduleDir}${mod.name}/mod.rs`;
  if (known.has(nested)) return nested;
  return null;
}

export function buildModuleTree(parsedByPath, roots) {
  const fileChains = new Map();
  const modulePaths = new Map();
  const moduleDirs = new Map();
  const known = new Set(parsedByPath.keys());
  const queue = [];

  for (const rootPath of roots) {
    if (!known.has(rootPath)) continue;
    fileChains.set(rootPath, [[]]);
    modulePaths.set(rootPath, new Set(['']));
    moduleDirs.set(rootPath, `${dirnameOf(rootPath)}/`);
    queue.push({ file: rootPath, chain: [], modPath: '' });
  }

  const seen = new Set();
  while (queue.length) {
    const current = queue.shift();
    const key = `${current.file}|${current.modPath}|${current.chain.length}`;
    if (seen.has(key)) continue;
    seen.add(key);
    const parsed = parsedByPath.get(current.file);
    if (!parsed) continue;
    const moduleDir = moduleDirs.get(current.file) ?? `${dirnameOf(current.file)}/`;
    for (const mod of parsed.mods) {
      if (mod.inline) continue;
      const target = resolveModTarget(current.file, mod, moduleDir, known);
      if (!target) continue;
      const childChain = [...current.chain, ...mod.ownGates];
      const childModPath = current.modPath ? `${current.modPath}::${mod.name}` : mod.name;
      const chains = fileChains.get(target) ?? [];
      chains.push(childChain);
      fileChains.set(target, chains);
      const paths = modulePaths.get(target) ?? new Set();
      paths.add(childModPath);
      modulePaths.set(target, paths);
      const dir = mod.pathAttr
        ? `${dirnameOf(target)}/${target.slice(target.lastIndexOf('/') + 1).replace(/\.rs$/, '')}/`
        : `${moduleDir}${mod.name}/`;
      moduleDirs.set(target, dir);
      queue.push({ file: target, chain: childChain, modPath: childModPath });
    }
  }

  return { fileChains, modulePaths, moduleDirs };
}

// ── 存在性 ──────────────────────────────────────────────────────────────────

const evalConjunction = (gates, env) => {
  let unknown = false;
  for (const gate of gates) {
    const value = evalCfg(gate, env);
    if (value === false) return false;
    if (value === 'unknown') unknown = true;
  }
  return unknown ? 'unknown' : true;
};

/** 一个条目在每个环境里「存在 / 不存在 / 说不准」。 */
function presenceOf(item, chains, envs) {
  const result = new Map();
  for (const env of envs) {
    let state = 'no';
    for (const chain of chains) {
      const value = evalConjunction([...chain, ...item.scopeGates, ...item.ownGates], env);
      if (value === true) {
        state = 'yes';
        break;
      }
      if (value === 'unknown') state = 'unknown';
    }
    result.set(env.index, state);
  }
  return result;
}

// ── 类型解析 ────────────────────────────────────────────────────────────────

const scopeKey = (item) => item.scopePath.join('::');

function modulePathSet(item, modulePaths) {
  const base = modulePaths.get(item.file) ?? new Set(['']);
  const out = new Set();
  for (const mp of base) {
    out.add(item.scopePath.length ? `${mp}::${item.scopePath.join('::')}` : mp);
  }
  return out;
}

function resolveTypeDef(candidates, implItem, modulePaths) {
  if (candidates.length === 0) return null;
  const sameScope = candidates.filter(
    (c) => c.file === implItem.file && scopeKey(c) === scopeKey(implItem),
  );
  if (sameScope.length === 1) return sameScope[0];
  const sameFile = candidates.filter((c) => c.file === implItem.file);
  if (sameFile.length === 1) return sameFile[0];
  const implPaths = modulePathSet(implItem, modulePaths);
  const byModule = candidates.filter((c) => {
    const paths = modulePathSet(c, modulePaths);
    for (const p of paths) if (implPaths.has(p)) return true;
    return false;
  });
  if (byModule.length === 1) return byModule[0];
  if (byModule.length > 1) return null;
  if (candidates.length === 1) return candidates[0];
  return null;
}

// ── 主分析 ──────────────────────────────────────────────────────────────────

function collectRustFiles(repoRoot) {
  const rels = [];
  const walk = (dir, prefix) => {
    for (const entry of readdirSync(join(repoRoot, dir), { withFileTypes: true })) {
      const rel = prefix ? `${prefix}/${entry.name}` : entry.name;
      if (entry.isDirectory()) walk(`${dir}/${entry.name}`, rel);
      else if (entry.name.endsWith('.rs')) rels.push(rel);
    }
  };
  walk('src-tauri/src', 'src-tauri/src');
  const examplesDir = join(repoRoot, 'src-tauri/examples');
  for (const entry of readdirSync(examplesDir, { withFileTypes: true })) {
    if (entry.isFile() && entry.name.endsWith('.rs')) rels.push(`src-tauri/examples/${entry.name}`);
  }
  return rels.sort();
}

export function analyzeRepo(repoRoot = REPO_ROOT, options = {}) {
  const overrides = options.overrides ?? new Map();
  const envs = buildEnvironments();
  const rels = collectRustFiles(repoRoot);
  const parsedByPath = new Map();
  for (const rel of rels) {
    const source = overrides.has(rel)
      ? overrides.get(rel)
      : readFileSync(join(repoRoot, rel), 'utf8');
    parsedByPath.set(rel, parseRustFile(source));
  }

  const roots = ['src-tauri/src/lib.rs', 'src-tauri/src/main.rs'];
  for (const rel of rels) {
    if (rel.startsWith('src-tauri/examples/')) roots.push(rel);
  }
  const tree = buildModuleTree(parsedByPath, roots);

  const allItems = [];
  for (const [rel, parsed] of parsedByPath) {
    for (const item of parsed.items) allItems.push({ ...item, file: rel });
  }
  const typeDefs = allItems.filter((i) => i.kind === 'type');
  const impls = allItems.filter((i) => i.kind === 'impl');
  const typesByName = new Map();
  for (const t of typeDefs) {
    const list = typesByName.get(t.name) ?? [];
    list.push(t);
    typesByName.set(t.name, list);
  }

  const stats = {
    files: rels.length,
    types: typeDefs.length,
    impls: impls.length,
    envs: envs.length,
    unreachableFiles: 0,
    skippedExternalType: 0,
    skippedAmbiguous: 0,
    skippedUnknownGate: 0,
  };

  const violations = [];
  const notes = [];

  const reachOf = (item) => tree.fileChains.get(item.file);
  for (const item of allItems) {
    if (!reachOf(item)) stats.unreachableFiles += 1;
  }

  const presenceCache = new Map();
  const presenceFor = (item) => {
    const chains = reachOf(item);
    if (!chains) return null;
    if (!presenceCache.has(item)) {
      presenceCache.set(item, presenceOf(item, chains, envs));
    }
    return presenceCache.get(item);
  };

  for (const impl of impls) {
    const implPresence = presenceFor(impl);
    if (!implPresence) {
      stats.skippedUnknownGate += 1;
      continue;
    }
    const candidates = typesByName.get(impl.subject) ?? [];
    if (candidates.length === 0) {
      stats.skippedExternalType += 1;
      continue;
    }
    const typeDef = resolveTypeDef(candidates, impl, tree.modulePaths);
    if (!typeDef) {
      stats.skippedAmbiguous += 1;
      continue;
    }
    const typePresence = presenceFor(typeDef);
    if (!typePresence) {
      stats.skippedUnknownGate += 1;
      continue;
    }
    const broken = [];
    for (const env of envs) {
      if (implPresence.get(env.index) !== 'yes') continue;
      const state = typePresence.get(env.index);
      if (state === 'no') broken.push(env);
    }
    if (broken.length === 0) continue;

    const kindLabel = impl.traitName ? `impl ${impl.traitName} for ${impl.subject}` : `impl ${impl.subject}`;
    violations.push({
      file: impl.file,
      line: impl.line,
      kindLabel,
      subject: impl.subject,
      typeFile: typeDef.file,
      typeLine: typeDef.line,
      typeGates: typeDef.ownRaw.slice(),
      broken,
    });
  }

  const seen = new Set();
  const uniqueViolations = violations.filter((v) => {
    const key = `${v.file}:${v.line}`;
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });

  notes.push(
    `${stats.impls} 个 impl 块、${stats.types} 个类型定义，穷举 ${stats.envs} 个环境`,
  );
  if (stats.unreachableFiles > 0) {
    notes.push(`${stats.unreachableFiles} 个文件不在模块树里，相关配对跳过`);
  }
  if (stats.skippedExternalType > 0) {
    notes.push(`${stats.skippedExternalType} 个 impl 的类型不在本 crate，跳过`);
  }
  if (stats.skippedAmbiguous > 0) {
    notes.push(`${stats.skippedAmbiguous} 个 impl 的类型同名多候选，跳过`);
  }
  if (stats.skippedUnknownGate > 0) {
    notes.push(`${stats.skippedUnknownGate} 个 impl 因门控无法判定而跳过`);
  }

  return { violations: uniqueViolations, notes, stats, envs };
}

// ── 命令行入口 ──────────────────────────────────────────────────────────────

const describeEnv = (env) => {
  const bits = [env.os];
  if (env.arch === 'aarch64') bits.push('aarch64');
  if (env.test) bits.push('test');
  if (env.debug_assertions) bits.push('debug_assertions');
  if (env.features.has('wdio')) bits.push('feature=wdio');
  return bits.join('/');
};

export function formatViolation(violation) {
  const envs = violation.broken.slice(0, 6).map(describeEnv).join('、');
  const more = violation.broken.length > 6 ? ` 等 ${violation.broken.length} 个环境` : '';
  const gate = violation.typeGates.length ? violation.typeGates.join(' ') : '（无门控）';
  return (
    `${violation.file}:${violation.line}  ${violation.kindLabel} 的门控比它实现的类型更宽：\n` +
    `  类型 ${violation.subject}（${violation.typeFile}:${violation.typeLine}）只在 ${gate} 下存在，\n` +
    `  而 impl 在 ${envs}${more} 下也生效 ⇒ 那些平台编译期报 E0425（cannot find type）。\n` +
    `  修法：给 impl 块补上与类型一致的门控 ${gate}。`
  );
}

export function runCli(repoRoot = REPO_ROOT) {
  const { violations, notes } = analyzeRepo(repoRoot);
  if (violations.length > 0) {
    console.error('✗ cfg 门控配套检查失败：');
    console.error('');
    for (const violation of violations) {
      console.error(`  ${formatViolation(violation)}\n`);
    }
    console.error(`${violations.length} 处 impl 块的门控比被实现的类型更宽。`);
    process.exitCode = 1;
    return violations.length;
  }
  console.log(`✓ cfg 门控配套：${notes.join('；')}`);
  return 0;
}

if (process.argv[1] && resolvePath(process.argv[1]) === fileURLToPath(import.meta.url)) {
  runCli();
}
