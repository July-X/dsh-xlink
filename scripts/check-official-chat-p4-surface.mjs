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
  FAIL,
  PASS,
  P4_SCAN_FILES,
  SKIPPED,
  argValue,
  emit,
  judgeChecks,
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

  return emit({
    round,
    sources,
    scanLabel: 'P4 实现（壳侧取数模块 ＋ 现役回环桥 ＋ 插件 host 半边）',
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
