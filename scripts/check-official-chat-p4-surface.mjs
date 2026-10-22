#!/usr/bin/env node
// P4 协议 §10 判据**第 5 条**（能力面）—— 按**源码形态**断。
//
// ## 依据（协议 **v0.10** §10 第 5 条的逐字口径）
//
// > 该路由**只对 official-chat 的 webview** 订阅/求值、**不接受任意 JS**
// > （页内求值**只用于观测**、不进数据通路）、**不写站点数据** ⇒ 按源码形态断。
//
// 配套条款：§2.1（两条通道：订阅 ＋ 观测；观测只用于量痕迹面）· §1.2（页内求值
// 通道已裁 ＝ `cdp_eval`，**只用于观测、不进数据通路**）· §2.3（数据通路上**没有
// 脚本**）· §5.2 第 3 条（**最小能力面**：只对 official-chat 的 webview 求值 ·
// 只允许白名单命令 / 只读操作 · 不许内核传任意 JS · 不许写站点数据 · 不许触碰
// 非 official-chat 的窗口）· §7.3（只读）。
//
// ## 断的六件事
//
// ① **只对 official-chat 的 webview 订阅** —— 建窗的 label 带 `official-chat-`
//    **且**实现里**不遍历窗口**（不遍历 ⇒ 不可能碰到非 official-chat 的窗口）；
// ② **桥上只收一个命令名**（白名单）—— 路由里有**命令名比较**，且**没有**任何
//    「把传入文本当脚本执行」的入口；
// ③ **数据通路不走页内求值** —— 取数模块里 `Runtime.evaluate`／`cdp_eval(` **0 次**
//    （§1.2：页内求值**只用于观测**）；
// ④ **页内求值通道本身存在**（`cdp_eval.rs` 里 `Runtime.evaluate` ≥ 1 处）——
//    观测通道不许被删掉（§1.2 裁定的通道）；
// ⑤ **不写站点数据** —— 扫描面里没有 `localStorage.setItem` / `document.cookie`
//    / Cookie 写入 / `execute_script` 写形态；
// ⑥ **数据通路上没有页内脚本** —— 没有 `addScriptToEvaluateOnNewDocument`，
//    且取数窗的 `initialization_script` **只有**仓库存量的族规脚本
//    （`no-context-menu.js`）。
//
// @module scripts/check-official-chat-p4-surface

import { tmpdir } from 'node:os';
import { join } from 'node:path';

import {
  BODY_CAPTURE_REL,
  CLIENT_REL,
  COLD_PROFILE_REL,
  FAIL,
  PASS,
  P4_SCAN_FILES,
  SKIPPED,
  argValue,
  emit,
  judgeChecks,
  matchBrace,
  readSources,
  repoRoot,
  runScenarios,
  stripComments,
} from './lib/p4-source-shape.mjs';

// 三态常量**从共用原语转出去**（自测文件从本模块取它们）。
export { FAIL, PASS, SKIPPED };

/** 壳侧取数模块（相对仓库根）。 */
export const RUST_REL = 'src-tauri/src/official_chat/session_structure.rs';
/** 页内求值通道（相对仓库根）。 */
export const EVAL_REL = 'src-tauri/src/official_chat/cdp_eval.rs';
/** 现役回环桥（相对仓库根）。 */
export const BRIDGE_REL = 'src-tauri/src/plugins/builtin/bridge.rs';

/** 官方对话族的 label 前缀（协议 §5.2 第 3 条：只对 official-chat 的 webview）。 */
export const OFFICIAL_CHAT_LABEL = 'official-chat-';

/** **遍历**窗口的形态（遍历 ⇒ 可能碰到非 official-chat 的窗口）。 */
export const WINDOW_ENUMERATION_TOKENS = [
  'webview_windows(',
  'get_all_webview_windows',
  'WebviewWindowBuilder::new(app, &format!',
];

/** **写站点数据**的形态（协议 §7.3：不写站点数据）。 */
export const SITE_WRITE_TOKENS = [
  'localStorage.setItem',
  'document.cookie =',
  'document.cookie=',
  'set_cookie',
  'CookieManager',
  'delete_cookie',
];

/** **任意 JS 执行**的入口（协议 §5.2 第 3 条：不许内核传任意 JS）。 */
export const ARBITRARY_JS_TOKENS = ['Runtime.evaluate', 'cdp_eval(', 'execute_script'];

/** 仓库存量的族规脚本（**唯一**允许出现在取数窗上的页内脚本）。 */
export const ALLOWED_INIT_SCRIPT = 'no-context-menu.js';

/**
 * **5d（协议 v0.23 §4.5）· 冷档案**：给「正文那条链」用的那份**一次性副本**。
 *
 * ## 为什么单开一条轨（不混进既有六条断点的扫描面）
 *
 * 既有六条断点是**按 `file.path` 专属判定**的（`session_structure.rs` 里的
 * `Runtime.evaluate` 算「数据通路走了观测通道」，`cdp_eval.rs` 里的算「观测通道
 * 存在」）⇒ ⇒ 把 `cold_profile.rs`／`session_body.rs` **塞进同一个 sources**，
 * 就会改到既有断点的判定面 ⇒ ⇒ **零假红优先** ⇒ ⇒ 5d 自己读自己那三个文件。
 *
 * ## 三条断点
 *
 * ① **白名单里不许出现 `IndexedDB`** —— 那棵就是「冷」的定义（站点那份增量缓存
 *    就在那）⇒ ⇒ 一旦有人把它加进白名单，整条修复**静默失效**（站点继续回增量）
 *    ⇒ ⇒ **没有任何用例会红**。
 * ② **白名单必须带上 `Local Storage` 与 `Network`**（Cookies）—— 站点把**登录态**
 *    放在 `Local Storage` ⇒ ⇒ 少了它 ⇒ 副本里**没登录** ⇒ 站点直接跳扫码登录页
 *    ⇒ ⇒ 「冷」的定义是「**登录态在、增量缓存不在**」，两半都要守住。
 * ③ **冷档案只服务「正文那条链」** —— 目录那条链（`fetch_page`，**不是增量**）
 *    **继续用生产档案** ⇒ ⇒ 目录链里出现 `ColdProfile` ⇒ 红。
 */

/** 冷档案的白名单里**不许**出现的那棵（站点那份增量缓存）。 */
export const COLD_FORBIDDEN = 'IndexedDB';

/** 冷档案的白名单里**必须**有的两半（登录态）。 */
export const COLD_REQUIRED = Object.freeze(['Local Storage', 'Network']);

/**
 * 从 Rust 源码里抽一个**常量字符串数组**（`const X: &[&str] = &["a", "b"];`）。
 *
 * @param {string} src - 源码文本。
 * @param {string} name - 常量名。
 * @returns {string[]} 数组里的字符串（抽不到 ⇒ 空数组）。
 */
export function rustStringArray(src, name) {
  const re = new RegExp(`const\\s+${name}[^=]*=\\s*&?\\[([^\\]]*)\\]`);
  const m = re.exec(String(src ?? ''));
  if (!m) return [];
  return [...m[1].matchAll(/"([^"]*)"/g)].map((x) => x[1]);
}

/**
 * 读 5d 的读数（**独立轨**：只读冷档案模块 ＋ 两条链各自的取数模块）。
 *
 * @param {string} root - 仓库根。
 * @returns {object} 读数。
 */
export function readColdProfileReading(root) {
  const read = (rel) => {
    try {
      return readSources(root, [rel])[0]?.code ?? '';
    } catch {
      return '';
    }
  };
  const cold = read(COLD_PROFILE_REL);
  const body = read(BODY_CAPTURE_REL);
  const structure = read(RUST_REL);
  return {
    found: cold.length > 0,
    whitelistFiles: rustStringArray(cold, 'WHITELIST_FILES'),
    whitelistDirs: rustStringArray(cold, 'WHITELIST_DIRS'),
    coldInBody: body.includes('ColdProfile'),
    coldInStructure: structure.includes('ColdProfile'),
    structureUsesProfileDir: structure.includes('profile_dir('),
  };
}

/**
 * 5d 的判定内核。
 *
 * @param {object} reading - 读数。
 * @returns {{ok: boolean, checks: Array<object>}}
 */
export function judgeColdProfile(reading) {
  const checks = [];
  const add = (name, status, detail) => checks.push({ name, status, detail });
  const files = Array.isArray(reading?.whitelistFiles) ? reading.whitelistFiles : [];
  const dirs = Array.isArray(reading?.whitelistDirs) ? reading.whitelistDirs : [];
  const all = [...files, ...dirs];

  if (reading?.found !== true) {
    add(
      'P4-5d 冷档案的白名单里**没有 `IndexedDB`**（那是「冷」的定义）',
      SKIPPED,
      `没读到冷档案模块（${COLD_PROFILE_REL}）⇒ 判据未执行（**不假绿**）`,
    );
    add('P4-5d 白名单带上了 `Local Storage` 与 `Network`（登录态在）', SKIPPED, '同上 ⇒ 判据未执行');
    add('P4-5d 冷档案**只服务「正文那条链」**（目录那条链仍用生产档案）', SKIPPED, '同上 ⇒ 判据未执行');
    return judgeChecks(checks);
  }

  const forbidden = all.filter((p) => p.includes(COLD_FORBIDDEN));
  add(
    'P4-5d 冷档案的白名单里**没有 `IndexedDB`**（那是「冷」的定义）',
    forbidden.length === 0 ? PASS : FAIL,
    forbidden.length === 0
      ? `白名单 ${all.length} 项，没有 ${COLD_FORBIDDEN}（站点那份增量缓存不在副本里）`
      : `白名单里混进了 ${forbidden.join('、')} ⇒ 站点会继续回增量 ⇒ 整条修复静默失效`,
  );

  const missing = COLD_REQUIRED.filter((need) => !all.some((p) => p.includes(need)));
  add(
    'P4-5d 白名单带上了 `Local Storage` 与 `Network`（登录态在、增量缓存不在）',
    missing.length === 0 ? PASS : FAIL,
    missing.length === 0
      ? `白名单：${all.join('、')}`
      : `白名单少了 ${missing.join('、')} ⇒ 副本里没登录 ⇒ 站点直接跳扫码登录页`,
  );

  const okChain =
    reading?.coldInBody === true && reading?.coldInStructure === false && reading?.structureUsesProfileDir === true;
  add(
    'P4-5d 冷档案**只服务「正文那条链」**（目录那条链仍用生产档案）',
    okChain ? PASS : FAIL,
    okChain
      ? '正文链用冷档案；目录链没有冷档案、仍走生产档案（profile_dir）'
      : `正文链用冷档案＝${reading?.coldInBody === true}／目录链出现冷档案＝${reading?.coldInStructure === true}／目录链用生产档案（profile_dir）＝${reading?.structureUsesProfileDir === true}`,
  );

  return judgeChecks(checks);
}

/**
 * **5e（PM 2026-10-22 15:0x 派单）· 客户端那半边到底有没有把 `site` 发出去**。
 *
 * ## 为什么值得补这一格
 *
 * 「@ 不到 MiniMax／ChatGPT」那个主诉的根因 ＝ 客户端**从来没把 `site` 放进请求
 * 载荷** ⇒ ⇒ host 侧 `request.site` 恒为空串 ⇒ ⇒ 壳侧按「没给就默认」走 DeepSeek ⇒
 * ⇒ 另两站的会话根本不在候选里 ⇒ ⇒ 而**那一整套都通过了门禁** ⇒ ⇒ 因为
 * **判据看得见「壳侧认得三个站」，看不见「界面一个站都没选」** ⇒ ⇒ 这是**覆盖面
 * 缺口**，不是实现缺陷。
 *
 * ## 三条断点（口径按事实源写死，不发明）
 *
 * ① **站表 ＝ 壳侧那三个常量（双向对账）** —— 短码是**契约**（与壳侧 Rust 的
 *    `SITE`／`MINIMAX_SITE`／`CHATGPT_SITE` 逐项相等），**不是**界面文案 ⇒ ⇒
 *    断的是**短码集合**，不碰 `label`。
 * ② **请求载荷里 `site` 是「可选键」** —— 选过站 ⇒ 键必须在；**没选过站 ⇒ 这个键
 *    整个不出现**（**不是** `site: ''`、**不是** `site: null`）⇒ ⇒ 这一格保的是
 *    「**既有用户零变化**」⇒ ⇒ 判法：**客户端不许直接写 `site:` 这个字面键**（正确
 *    写法是 `...sitePayload(x)` 展开 ⇒ 没选站时键整个不出现）。
 * ③ **取正文那次带的是「这条引用当初那个站」** —— 载荷里的 `site` 必须来自
 *    `siteOfRef(…)`（**不是** `currentSite()`）⇒ ⇒ 用户切过站的老引用仍按老站取
 *    正文 ⇒ ⇒ 这一条是本轮真正抓到过的那个真缺陷的守卫。
 *
 * ⚠️ **不许用「上游中文措辞」当判据** ⇒ ⇒ 断短码，不断文案。
 */

/** 客户端站表（`SITE_OPTIONS`）里的短码。 */
function clientSiteCodes(plain) {
  const block = /SITE_OPTIONS\s*=\s*Object\.freeze\(\[(.*?)\]\)/s.exec(plain);
  if (!block) return [];
  return [...block[1].matchAll(/code:\s*'([^']+)'/g)].map((m) => m[1]);
}

/** 壳侧那三个站点常量（`SITE`／`MINIMAX_SITE`／`CHATGPT_SITE`）。 */
function rustSiteCodes(plain) {
  return [
    ...plain.matchAll(/pub\(crate\)\s+const\s+(?:SITE|MINIMAX_SITE|CHATGPT_SITE):\s*&str\s*=\s*"([^"]*)"/g),
  ].map((m) => m[1]);
}

/** 站点短码的**形态**（与 host 侧 `parseSite` 同一口径）。 */
export const SITE_CODE_SHAPE = /^[a-z0-9][a-z0-9-]{0,31}$/;

/**
 * 取某个调用点后面那个**对象字面量**的文本（`{` 到配平的 `}`）。
 *
 * @param {string} text - 源码文本。
 * @param {number} at - `{` 的下标。
 * @returns {string} 对象字面量文本（配不上 ⇒ 空串）。
 */
function objectLiteralAt(text, at) {
  const end = matchBrace(text, at);
  return end < 0 ? '' : text.slice(at + 1, end);
}

/**
 * 读 5e 的读数（**独立轨**：只读客户端那半边 ＋ 壳侧常量那一个文件）。
 *
 * @param {string} root - 仓库根。
 * @returns {object} 读数。
 */
export function readClientSiteReading(root) {
  const clientCode = readSources(root, [CLIENT_REL])[0]?.code ?? '';
  const rustCode = readSources(root, [RUST_REL])[0]?.code ?? '';
  const client = stripComments(clientCode).join('\n');
  const rust = stripComments(rustCode).join('\n');

  // 目录请求的载荷（`scheduleCatalog(host, { … })`）—— 判「`site` 是不是字面键」
  const catalogPayloads = [];
  for (const m of client.matchAll(/scheduleCatalog\(\s*host\s*,\s*\{/g)) {
    const at = client.indexOf('{', m.index);
    const text = objectLiteralAt(client, at);
    catalogPayloads.push({
      line: client.slice(0, m.index).split(/\r?\n/).length,
      hasSiteKey: /(^|[^.\w])site\s*:/.test(text),
      usesSitePayload: /sitePayload\(/.test(text),
    });
  }

  // 取正文那次的载荷（`host.call('capture', { … })`）—— 判「带的是不是引用当初那个站」
  const capturePayloads = [];
  for (const m of client.matchAll(/host\.call\(\s*'capture'\s*,\s*\{/g)) {
    const at = client.indexOf('{', m.index);
    const text = objectLiteralAt(client, at);
    capturePayloads.push({
      line: client.slice(0, m.index).split(/\r?\n/).length,
      usesSiteOfRef: /siteOfRef\(/.test(text),
      usesCurrentSite: /currentSite\(/.test(text),
    });
  }

  return {
    found: clientCode.length > 0,
    clientCodes: clientSiteCodes(client),
    rustCodes: rustSiteCodes(rust),
    catalogPayloads,
    capturePayloads,
  };
}

/**
 * 5e 的判定内核。
 *
 * @param {object} reading - 读数。
 * @returns {{ok: boolean, checks: Array<object>}}
 */
export function judgeClientSite(reading) {
  const checks = [];
  const add = (name, status, detail) => checks.push({ name, status, detail });

  if (reading?.found !== true) {
    add('P4-5e 客户端站表 ＝ 壳侧那三个常量（双向对账）', SKIPPED, `没读到 ${CLIENT_REL} ⇒ 判据未执行（**不假绿**）`);
    add('P4-5e 目录请求载荷里 `site` 是**可选键**（没选过站 ⇒ 键整个不出现）', SKIPPED, '同上 ⇒ 判据未执行');
    add('P4-5e 取正文那次带的是「这条引用**当初那个站**」', SKIPPED, '同上 ⇒ 判据未执行');
    return judgeChecks(checks);
  }

  const clientCodes = Array.isArray(reading?.clientCodes) ? reading.clientCodes : [];
  const rustCodes = Array.isArray(reading?.rustCodes) ? reading.rustCodes : [];
  const sameSet =
    clientCodes.length > 0 &&
    clientCodes.length === rustCodes.length &&
    clientCodes.every((c) => rustCodes.includes(c));
  const badShape = clientCodes.filter((c) => !SITE_CODE_SHAPE.test(c));
  add(
    'P4-5e 客户端站表 ＝ 壳侧那三个常量（双向对账）',
    sameSet && badShape.length === 0 ? PASS : FAIL,
    sameSet && badShape.length === 0
      ? `两边一致：${clientCodes.join('、')}`
      : `客户端 ${clientCodes.join('、') || '（没读到）'}／壳侧 ${rustCodes.join('、') || '（没读到）'}／形态不合的：${badShape.join('、') || '（无）'}`,
  );

  const catalogs = Array.isArray(reading?.catalogPayloads) ? reading.catalogPayloads : [];
  const literalKey = catalogs.filter((c) => c.hasSiteKey);
  add(
    'P4-5e 目录请求载荷里 `site` 是**可选键**（没选过站 ⇒ 键整个不出现）',
    catalogs.length > 0 && literalKey.length === 0 ? PASS : FAIL,
    catalogs.length === 0
      ? '没找到目录请求的载荷组装处 ⇒ 判据未执行（**不假绿**）'
      : literalKey.length === 0
        ? `${catalogs.length} 处目录请求载荷都没有字面 \`site:\` 键（走 sitePayload 展开 ⇒ 没选站时键整个不出现）`
        : `目录请求载荷里有**字面** \`site:\` 键（${literalKey.map((c) => `第 ${c.line} 行`).join('、')}）⇒ 没选过站时载荷里会多一个 \`site: ''\`（派单口径：**键整个不出现**）`,
  );

  const captures = Array.isArray(reading?.capturePayloads) ? reading.capturePayloads : [];
  const badCaptures = captures.filter((c) => !c.usesSiteOfRef || c.usesCurrentSite);
  add(
    'P4-5e 取正文那次带的是「这条引用**当初那个站**」',
    captures.length > 0 && badCaptures.length === 0 ? PASS : FAIL,
    captures.length === 0
      ? '没找到取正文的载荷组装处 ⇒ 判据未执行（**不假绿**）'
      : badCaptures.length === 0
        ? `${captures.length} 处取正文载荷都用 siteOfRef(…)（不是当前站）`
        : `取正文载荷有 ${badCaptures.map((c) => `第 ${c.line} 行`).join('、')} 没走 siteOfRef(…) 或用了当前站`,
  );

  return judgeChecks(checks);
}

/**
 * 读一份**读数**。
 *
 * @param {object} reading
 * @param {Array<{path: string, code: string}>} [reading.sources] - 扫描面。
 * @returns {object} 归一后的读数。
 */
export function normalizeReading(reading) {
  const sources = Array.isArray(reading?.sources) ? reading.sources : [];
  const hits = {
    dataPathEval: [],
    siteWrite: [],
    windowEnumeration: [],
    pageScriptInjection: [],
    initScripts: [],
    reload: [],
    navigate: [],
  };
  let evalChannel = 0;
  let officialChatLabel = 0;
  let commandGate = 0;

  for (const file of sources) {
    const lines = stripComments(file.code);
    lines.forEach((line, index) => {
      const at = `${file.path}:${index + 1}`;
      if (file.path === RUST_REL) {
        if (line.includes('Runtime.evaluate') || line.includes('cdp_eval(')) {
          hits.dataPathEval.push(at);
        }
        if (line.includes('initialization_script')) hits.initScripts.push(at);
      }
      if (file.path === EVAL_REL && line.includes('Runtime.evaluate')) {
        evalChannel += 1;
      }
      if (file.path === BRIDGE_REL && line.includes('session_structure::COMMAND')) {
        commandGate += 1;
      }
      if (line.includes(OFFICIAL_CHAT_LABEL)) officialChatLabel += 1;
      if (WINDOW_ENUMERATION_TOKENS.some((token) => line.includes(token))) {
        hits.windowEnumeration.push(at);
      }
      if (SITE_WRITE_TOKENS.some((token) => line.includes(token))) hits.siteWrite.push(at);
      if (line.includes('addScriptToEvaluateOnNewDocument')) {
        hits.pageScriptInjection.push(at);
      }
      // **壳侧 reload** 与**重复导航**（协议 v0.11 §8.5.2 G4-13 ②：`reload` 会让
      // 站点重跑初始化批次 ⇒ 登录态下站点自己会 `POST chat_session/create`
      // ⇒ `reload` **不是只读动作** ⇒ 不许有）。
      if (line.includes('Page.reload') || line.includes('.reload(')) {
        hits.reload.push(at);
      }
      if (line.includes('.navigate(')) hits.navigate.push(at);
    });
  }

  // 取数窗的 `initialization_script` **只准**是族规脚本。
  const foreignInitScripts = [];
  for (const file of sources) {
    if (file.path !== RUST_REL) continue;
    const lines = stripComments(file.code);
    lines.forEach((line, index) => {
      if (!line.includes('initialization_script')) return;
      if (!line.includes(ALLOWED_INIT_SCRIPT)) {
        foreignInitScripts.push(`${file.path}:${index + 1}`);
      }
    });
  }

  return {
    fileCount: sources.length,
    dataPathEval: hits.dataPathEval,
    siteWrite: hits.siteWrite,
    windowEnumeration: hits.windowEnumeration,
    pageScriptInjection: hits.pageScriptInjection,
    foreignInitScripts,
    reload: hits.reload,
    navigate: hits.navigate,
    evalChannel,
    officialChatLabel,
    commandGate,
  };
}

/**
 * 判定内核。
 *
 * @param {object} reading - 读数。
 * @returns {{ok: boolean, checks: Array<object>}}
 */
export function judgeSurface(reading) {
  const checks = [];
  const add = (name, status, detail) => checks.push({ name, status, detail });
  const r = normalizeReading(reading);

  if (r.fileCount === 0) {
    add('P4-5 读到了实现源码', SKIPPED, '扫描面为空 ⇒ 判据未执行');
    return judgeChecks(checks);
  }

  add(
    'P4-5 **只对 official-chat 的 webview 订阅**（label 带 official-chat- ＋ 不遍历窗口，§5.2 第 3 条）',
    r.officialChatLabel > 0 && r.windowEnumeration.length === 0 ? PASS : FAIL,
    r.officialChatLabel > 0 && r.windowEnumeration.length === 0
      ? `official-chat 族 label ${r.officialChatLabel} 处；没有遍历窗口的形态`
      : `official-chat label ${r.officialChatLabel} 处；遍历窗口 ${r.windowEnumeration.length} 处`,
  );

  add(
    'P4-5 **桥上只收一个命令名**（白名单，§5.2 第 3 条）',
    r.commandGate > 0 ? PASS : FAIL,
    r.commandGate > 0
      ? `路由里有命令名比较 ${r.commandGate} 处`
      : '路由里没找到命令名比较（白名单门没落）',
  );

  add(
    'P4-5 **数据通路不走页内求值**（页内求值只用于观测，§1.2／§10 第 5 条）',
    r.dataPathEval.length === 0 ? PASS : FAIL,
    r.dataPathEval.length === 0
      ? '取数模块里没有 Runtime.evaluate／cdp_eval 调用'
      : `取数模块里出现了页内求值：${r.dataPathEval.slice(0, 5).join('、')}`,
  );

  add(
    'P4-5 **页内求值通道本身存在**（§1.2 裁定的观测通道不许被删）',
    r.evalChannel > 0 ? PASS : FAIL,
    r.evalChannel > 0
      ? `cdp_eval.rs 里 Runtime.evaluate ${r.evalChannel} 处`
      : '观测通道不见了（判据第 2 条的真机轨要靠它）',
  );

  add(
    'P4-5 **不写站点数据**（§7.3）',
    r.siteWrite.length === 0 ? PASS : FAIL,
    r.siteWrite.length === 0
      ? '扫描面里没有写 localStorage／cookie／DOM 的形态'
      : `出现了写站点数据：${r.siteWrite.slice(0, 5).join('、')}`,
  );

  add(
    'P4-5 **数据通路上没有页内脚本**（§2.3：页内动作清单为空）',
    r.pageScriptInjection.length === 0 && r.foreignInitScripts.length === 0 ? PASS : FAIL,
    r.pageScriptInjection.length === 0 && r.foreignInitScripts.length === 0
      ? `没有页内脚本注入；取数窗只注入族规脚本（${ALLOWED_INIT_SCRIPT}）`
      : `页内脚本注入 ${r.pageScriptInjection.length} 处；非族规初始化脚本 ${r.foreignInitScripts.length} 处`,
  );

  add(
    'P4-5 **不存在壳侧 `reload`／重复导航路径**（v0.11 §8.5.2 G4-13 ②：`reload` 不是只读动作）',
    r.reload.length === 0 && r.navigate.length <= 1 ? PASS : FAIL,
    r.reload.length === 0 && r.navigate.length <= 1
      ? `没有 reload 形态；导航只发生 ${r.navigate.length} 次（只首次）`
      : `reload ${r.reload.length} 处／导航 ${r.navigate.length} 次`,
  );

  return judgeChecks(checks);
}

/**
 * 场景表（合成读数）。
 *
 * @returns {Array<object>}
 */
export function buildScenarios() {
  const clean = {
    sources: [
      {
        path: RUST_REL,
        code: [
          'let label = format!("official-chat-capture-{site}");',
          '.initialization_script(include_str!("../harness/no-context-menu.js"))',
          'cdp_call(window, "Network.enable", json!({}), CDP_TIMEOUT).await?;',
        ].join('\n'),
      },
      {
        path: EVAL_REL,
        code: 'cdp_call(webview, "Runtime.evaluate", params, timeout).await',
      },
      {
        path: BRIDGE_REL,
        code: 'if command != crate::official_chat::session_structure::COMMAND { return; }',
      },
    ],
  };
  return [
    { id: 'A', name: '场景A 真身能力面（只订阅自己的窗 ＋ 只收命令名 ＋ 不写站点数据）⇒ 应通过', expect: PASS, reading: clean },
    {
      id: 'B',
      name: '场景B 取数模块里出现页内求值（数据通路走了观测通道）⇒ 应失败',
      expect: FAIL,
      reading: {
        ...clean,
        sources: [
          { ...clean.sources[0], code: `${clean.sources[0].code}\ncdp_eval(window, "x", timeout).await?;` },
          clean.sources[1],
          clean.sources[2],
        ],
      },
    },
    {
      id: 'C',
      name: '场景C 遍历窗口（可能碰到非 official-chat 的窗口）⇒ 应失败',
      expect: FAIL,
      reading: {
        ...clean,
        sources: [
          { ...clean.sources[0], code: `${clean.sources[0].code}\nfor w in app.webview_windows() {}` },
          clean.sources[1],
          clean.sources[2],
        ],
      },
    },
    {
      id: 'D',
      name: '场景D 写站点数据（§7.3）⇒ 应失败',
      expect: FAIL,
      reading: {
        ...clean,
        sources: [
          { ...clean.sources[0], code: `${clean.sources[0].code}\nlocalStorage.setItem("x", "y");` },
          clean.sources[1],
          clean.sources[2],
        ],
      },
    },
    {
      id: 'E',
      name: '场景E 注入页内脚本（§2.3）⇒ 应失败',
      expect: FAIL,
      reading: {
        ...clean,
        sources: [
          {
            ...clean.sources[0],
            code: `${clean.sources[0].code}\ncdp_call(window, "Page.addScriptToEvaluateOnNewDocument", json!({}), t).await?;`,
          },
          clean.sources[1],
          clean.sources[2],
        ],
      },
    },
    {
      id: 'F',
      name: '场景F 桥上不校验命令名（能力面没关死）⇒ 应失败',
      expect: FAIL,
      reading: {
        ...clean,
        sources: [clean.sources[0], clean.sources[1], { path: BRIDGE_REL, code: 'let _ = command;' }],
      },
    },
    {
      id: 'G',
      name: '场景G 出现壳侧 `Page.reload`（§8.5.2 G4-13 ②：不是只读动作）⇒ 应失败',
      expect: FAIL,
      reading: {
        ...clean,
        sources: [
          {
            ...clean.sources[0],
            code: `${clean.sources[0].code}\ncdp_call(window, "Page.reload", json!({}), t).await?;`,
          },
          clean.sources[1],
          clean.sources[2],
        ],
      },
    },
    {
      id: 'H',
      name: '场景H 导航两次（重复导航 ＝ 等价 reload）⇒ 应失败',
      expect: FAIL,
      reading: {
        ...clean,
        sources: [
          { ...clean.sources[0], code: `${clean.sources[0].code}\nwindow.navigate(url)?;\nwindow.navigate(url)?;` },
          clean.sources[1],
          clean.sources[2],
        ],
      },
    },
  ];
}

/**
 * 读真实仓库的读数。
 *
 * @param {string} root - 仓库根。
 * @returns {{sources: Array<object>}}
 */
export function readReading(root) {
  return { sources: readSources(root, P4_SCAN_FILES) };
}

/**
 * 主入口。
 *
 * @param {string[]} [argv] - 命令行参数。
 * @returns {Promise<{results: Array<object>, summary: object}>}
 */
export async function main(argv = process.argv.slice(2)) {
  const root = argValue(argv, '--target') ?? repoRoot(import.meta.url);
  const round = argValue(argv, '--round') ?? 'p4c5';
  const asJson = argv.includes('--json');

  const sources = readSources(root, P4_SCAN_FILES);
  const reading = readReading(root);
  const { ok, checks } = judgeSurface(reading);

  const results = [];
  for (const c of checks) results.push({ name: c.name, status: c.status, detail: c.detail });
  results.push({
    name: 'P4-5 真实仓库读数的判定',
    status: ok ? PASS : FAIL,
    detail: ok ? '真实读数全过' : '真实读数未全过（见上）',
  });
  for (const r of runScenarios(buildScenarios(), judgeSurface, 'P4-5a')) {
    results.push({ name: r.name, status: r.status, detail: r.detail });
  }

  // --- 5d（协议 v0.23 §4.5）：冷档案的形态（**独立轨**）---
  const coldReading = readColdProfileReading(root);
  const { ok: coldOk, checks: coldChecks } = judgeColdProfile(coldReading);
  for (const c of coldChecks) {
    results.push({ name: `P4-5d ${c.name.replace(/^P4-5d /, '')}`, status: c.status, detail: c.detail });
  }
  results.push({
    name: 'P4-5d 真实读数的判定',
    status: coldOk ? PASS : FAIL,
    detail: coldOk
      ? '冷档案的形态全过（白名单没有 IndexedDB／带上了登录态那两半／只服务正文那条链）'
      : '冷档案的形态有红 ⇒ **冷被稀释了或链用错了**（协议 v0.23 §4.5）',
  });

  // --- 5e（PM 15:0x 派单）：客户端那半边到底有没有把 `site` 发出去（**独立轨**）---
  const clientSiteReading = readClientSiteReading(root);
  const { ok: clientSiteOk, checks: clientSiteChecks } = judgeClientSite(clientSiteReading);
  for (const c of clientSiteChecks) {
    results.push({ name: c.name, status: c.status, detail: c.detail });
  }
  results.push({
    name: 'P4-5e 真实读数的判定',
    status: clientSiteOk ? PASS : FAIL,
    detail: clientSiteOk
      ? '客户端那半边的 `site` 全对（站表与壳侧对账／载荷里是可选键／取正文带引用当初那个站）'
      : '客户端那半边的 `site` 有红 ⇒ **界面那一站没真的发出去**（协议 P1 §1.2）',
  });

  return emit({
    round,
    // **扫描面 ＝ 既有十份 ＋ 5d 那两个文件 ＋ 5e 那个客户端文件**
    // ⇒ ⇒ 「扫了 N 个文件」与实际读取面一致。
    sources: [
      ...sources,
      ...readSources(root, [COLD_PROFILE_REL, BODY_CAPTURE_REL, CLIENT_REL]),
    ],
    scanLabel:
      'P4 实现（壳侧取数模块 ＋ 现役回环桥 ＋ 插件 host 半边 ＋ 5d 冷档案模块 ＋ 5e 客户端那半边）',
    reading,
    results,
    evidenceDir: join(tmpdir(), 'dsh-p4-surface'),
    asJson,
  });
}

// 直接跑（被 import 时不跑）
if (process.argv[1] && process.argv[1].endsWith('check-official-chat-p4-surface.mjs')) {
  await main();
}
