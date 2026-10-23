#!/usr/bin/env node
// P4 协议 §10 判据**第 11 条**（订阅时序）—— 按**源码形态**断。
//
// ## 依据
//
// > **订阅时序**（v0.9 新增）: 实现里 `Network.enable` 的调用点**在建窗/导航链上**，
// > **不在**「用户发起读取」的处理路径上 ⇒ **按源码形态断** ⇒ ⇒ **理由**：晚订阅
// > ⇒ 站点首屏 `/api/` 响应**全丢**（对照臂：导航完成后才 enable ⇒ 站点 `/api/`
// > 命中 **0**，§8.5.1 硬约束 3）。
//
// 配套：§1 裁定 1（形态唯一 ＝ 壳侧 `Network` 域订阅）· §8.5.1 硬约束 3
// （**`Network.enable` 必须在导航前**；两条接点都【已实测】成立：① 在
// `NavigationStarting` 回调里 enable ② 先建 `about:blank` → 订阅 → enable →
// 再 `navigate` ⇒ ⇒ **接点由实现侧选**，但必须**早于站点首个请求**）。
//
// ## 断的五件事
//
// ① **`Network.enable` 的调用点存在**（在壳侧取数模块里）；
// ② **enable 与导航分属两步** —— enable 所在函数体里**没有**导航调用
//    （⇒ 不是「导航完了才 enable」）；
// ③ **导航那一步不含 enable** —— 导航所在函数体里**没有** `Network.enable`；
// ④ **enable 在导航之前** —— 编排函数里「建窗＋订阅＋enable」的调用行**先于**
//    「导航」的调用行；
// ⑤ **订阅不挂在「用户发起读取」的处理路径上** —— 现役桥那条路由里**没有**
//    `Network.enable`（读取路径只负责**问**，不负责**订阅**）。
//
// ## 关于「函数名约定」
//
// 判据第 10／11 条要断的是**调用点之间的关系** ⇒ ⇒ 得能按函数切开源码
// ⇒ ⇒ 这里按**实现侧的函数名约定**切：`arm_capture_window`（建窗＋订阅＋enable）
// · `capture_in`（导航＋等＋取体）· `capture`（编排）。这三个名字写在本脚本里，
// **实现改名就要同步改这里**（判据与实现不同名 ⇒ 判据会报「未执行」，不会假绿）。
//
// @module scripts/check-official-chat-p4-enable-timing

import { tmpdir } from 'node:os';
import { join } from 'node:path';

import {
  FAIL,
  PASS,
  P4_SCAN_FILES,
  SKIPPED,
  argValue,
  emit,
  fnBody,
  judgeChecks,
  lineOf,
  readSources,
  repoRoot,
  runScenarios,
  stripComments,
} from './lib/p4-source-shape.mjs';

// 三态常量**从共用原语转出去**（自测文件从本模块取它们）。
export { FAIL, PASS, SKIPPED };

/** 壳侧取数模块（相对仓库根）。 */
export const RUST_REL = 'src-tauri/src/official_chat/session_structure.rs';
/** 现役回环桥（相对仓库根）。 */
export const BRIDGE_REL = 'src-tauri/src/plugins/builtin/bridge.rs';

/** 建窗＋订阅＋enable 的那个函数（**实现侧约定**）。 */
export const ARM_FN = 'arm_capture_window';
/** 导航＋等＋取体的那个函数（**实现侧约定**）。 */
export const NAV_FN = 'capture_in';
/** 编排两者的那个函数（**实现侧约定**）。 */
export const ORCHESTRATE_FN = 'ensure_started';

/**
 * 读一份**读数**。
 *
 * @param {object} reading
 * @param {string} [reading.rust] - 取数模块源码。
 * @param {string} [reading.bridge] - 现役桥源码。
 * @returns {object} 归一后的读数。
 */
export function normalizeReading(reading) {
  // ⚠️ **只看代码行** —— 注释里写的 `Network.enable` **不算数**（沿判据第 6 条
  // 「注释里的 return 不算」的规矩）。
  const rust = stripComments(String(reading?.rust ?? '')).join('\n');
  const bridge = stripComments(String(reading?.bridge ?? '')).join('\n');
  const arm = fnBody(rust, ARM_FN);
  const nav = fnBody(rust, NAV_FN);
  const orchestrate = fnBody(rust, ORCHESTRATE_FN);
  const directNav = lineOf(orchestrate, `${NAV_FN}(`);
  const deadlineCallsNav = lineOf(fnBody(rust, 'capture_with_deadline'), `${NAV_FN}(`) > 0;
  return {
    hasRust: rust.trim() !== '',
    armHasEnable: arm.includes('Network.enable'),
    armHasNavigate: arm.includes('navigate'),
    navHasEnable: nav.includes('Network.enable'),
    armCallLine: lineOf(orchestrate, `${ARM_FN}(`),
    navCallLine: directNav > 0 ? directNav
      : deadlineCallsNav ? lineOf(orchestrate, 'capture_with_deadline(') : -1,
    bridgeHasEnable: bridge.includes('Network.enable'),
  };
}

/**
 * 判定内核。
 *
 * @param {object} reading - 读数。
 * @returns {{ok: boolean, checks: Array<object>}}
 */
export function judgeEnableTiming(reading) {
  const checks = [];
  const add = (name, status, detail) => checks.push({ name, status, detail });
  const r = normalizeReading(reading);

  if (!r.hasRust) {
    add('P4-11 读到了取数模块源码', SKIPPED, `没读到 ${RUST_REL} ⇒ 判据未执行`);
    return judgeChecks(checks);
  }

  add(
    'P4-11 **`Network.enable` 的调用点存在**（订阅必须显式打开 Network 域）',
    r.armHasEnable ? PASS : FAIL,
    r.armHasEnable
      ? `${ARM_FN}() 里有 Network.enable`
      : `${ARM_FN}() 里没有 Network.enable`,
  );

  add(
    'P4-11 **enable 与导航分属两步**（enable 所在函数体里没有导航调用）',
    r.armHasEnable && !r.armHasNavigate ? PASS : FAIL,
    r.armHasNavigate
      ? `${ARM_FN}() 里也有导航调用 ⇒ enable 可能落在导航之后`
      : `${ARM_FN}() 里没有导航调用`,
  );

  add(
    'P4-11 **导航那一步不含 enable**（不是「导航完了才 enable」）',
    r.navHasEnable ? FAIL : PASS,
    r.navHasEnable ? `${NAV_FN}() 里出现了 Network.enable` : `${NAV_FN}() 里没有 Network.enable`,
  );

  const ordered = r.armCallLine > 0 && r.navCallLine > 0 && r.armCallLine < r.navCallLine;
  add(
    'P4-11 **enable 在导航之前**（编排里的行序：先建窗订阅，再导航）',
    ordered ? PASS : FAIL,
    ordered
      ? `建窗订阅在第 ${r.armCallLine} 行、导航在第 ${r.navCallLine} 行`
      : `建窗订阅行=${r.armCallLine}／导航行=${r.navCallLine}`,
  );

  add(
    'P4-11 **订阅不挂在「用户发起读取」的处理路径上**（现役桥那条路由里没有 enable）',
    r.bridgeHasEnable ? FAIL : PASS,
    r.bridgeHasEnable
      ? '现役桥那条路由里出现了 Network.enable ⇒ 订阅被挂在读取路径上'
      : '现役桥那条路由里没有 Network.enable（读取只负责问，不负责订阅）',
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
    rust: [
      'async fn arm_capture_window(app: &AppHandle, site: &str) -> Result<(WebviewWindow, EventStream), CaptureFailure> {',
      '    let window = build_capture_window(app, site)?;',
      '    let events = subscribe_network_events(&window)?;',
      '    cdp_call(&window, "Network.enable", json!({}), CDP_TIMEOUT).await?;',
      '    Ok((window, events))',
      '}',
      'async fn capture_in(window: &WebviewWindow, events: EventStream) -> Result<SessionStructure, CaptureFailure> {',
      '    window.navigate(url)?;',
      '    let hit = await_target_hit(&mut events).await?;',
      '    Ok(hit)',
      '}',
      'async fn ensure_started(app: &AppHandle, state: &CaptureState) -> Result<(), CaptureFailure> {',
      '    let (window, events) = arm_capture_window(app, "deepseek", &dir).await?;',
      '    tauri::async_runtime::spawn(async move {',
      '        let outcome = capture_in(&window, events).await;',
      '    });',
      '    Ok(())',
      '}',
    ].join('\n'),
    bridge: 'if command != crate::official_chat::session_structure::COMMAND { return; }',
  };
  return [
    { id: 'A', name: '场景A 真身订阅时序（建窗订阅在导航之前）⇒ 应通过', expect: PASS, reading: clean },
    {
      id: 'B',
      name: '场景B enable 挪进导航那一步（晚订阅 ⇒ 首屏全丢）⇒ 应失败',
      expect: FAIL,
      reading: {
        ...clean,
        rust: clean.rust
          .replace('    cdp_call(&window, "Network.enable", json!({}), CDP_TIMEOUT).await?;\n', '')
          .replace(
            '    window.navigate(url)?;',
            '    cdp_call(&window, "Network.enable", json!({}), CDP_TIMEOUT).await?;\n    window.navigate(url)?;',
          ),
      },
    },
    {
      id: 'C',
      name: '场景C 建窗订阅与导航同函数（enable 位置不可证早于导航）⇒ 应失败',
      expect: FAIL,
      reading: {
        ...clean,
        rust: clean.rust.replace(
          '    cdp_call(&window, "Network.enable", json!({}), CDP_TIMEOUT).await?;\n    Ok((window, events))',
          '    Ok((window, events))',
        ),
      },
    },
    {
      id: 'D',
      name: '场景D 订阅被挂到「用户发起读取」的路径上（现役桥里出现 enable）⇒ 应失败',
      expect: FAIL,
      reading: { ...clean, bridge: 'cdp_call(&window, "Network.enable", json!({}), t).await?;' },
    },
  ];
}

/**
 * 读真实仓库的读数。
 *
 * @param {string} root - 仓库根。
 * @returns {{rust: string, bridge: string}}
 */
export function readReading(root) {
  const sources = readSources(root, P4_SCAN_FILES);
  return {
    rust: sources.find((s) => s.path === RUST_REL)?.code ?? '',
    bridge: sources.find((s) => s.path === BRIDGE_REL)?.code ?? '',
  };
}

/**
 * 主入口。
 *
 * @param {string[]} [argv] - 命令行参数。
 * @returns {Promise<{results: Array<object>, summary: object}>}
 */
export async function main(argv = process.argv.slice(2)) {
  const root = argValue(argv, '--target') ?? repoRoot(import.meta.url);
  const round = argValue(argv, '--round') ?? 'p4c11';
  const asJson = argv.includes('--json');

  const sources = readSources(root, P4_SCAN_FILES);
  const reading = readReading(root);
  const { ok, checks } = judgeEnableTiming(reading);

  const results = [];
  for (const c of checks) results.push({ name: c.name, status: c.status, detail: c.detail });
  results.push({
    name: 'P4-11 真实仓库读数的判定',
    status: ok ? PASS : FAIL,
    detail: ok ? '真实读数全过' : '真实读数未全过（见上）',
  });
  for (const r of runScenarios(buildScenarios(), judgeEnableTiming, 'P4-11a')) {
    results.push({ name: r.name, status: r.status, detail: r.detail });
  }

  return emit({
    round,
    sources,
    scanLabel: 'P4 实现（壳侧取数模块 ＋ 现役回环桥 ＋ 插件 host 半边）',
    reading,
    results,
    evidenceDir: join(tmpdir(), 'dsh-p4-enable-timing'),
    asJson,
  });
}

// 直接跑（被 import 时不跑）
if (process.argv[1] && process.argv[1].endsWith('check-official-chat-p4-enable-timing.mjs')) {
  await main();
}
