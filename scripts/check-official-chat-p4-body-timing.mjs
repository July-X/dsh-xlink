#!/usr/bin/env node
// P4 协议 §10 判据**第 10 条**（取体时序）—— 按**源码形态**断。
//
// ## 依据
//
// > **取体时序**（v0.9 新增）: 实现里 `Network.getResponseBody` **只在收到
// > `Network.loadingFinished` 之后**调用 ⇒ **不存在**「收到 `Network.responseReceived`
// > 就取体」的代码路径 ⇒ **按源码形态断** ⇒ ⇒ **理由**：取早有**两种失败形态**
// > （`E_INVALIDARG (0x80070057)` ＋ **静默拿到长度 0 的 body 且不报错**），且
// > **空 body 会被误当成「站点没数据」**（§8.5.1 硬约束 1）。
//
// 配套：§6.3（**回程失败 ≠ 站点返回空** —— 空体归回程失败那一类）。
//
// ## 断的四件事
//
// ① **等得到 `Network.loadingFinished`** —— 取数模块里处理了这个事件；
// ② **不存在「收到 responseReceived 就取体」的路径** —— 取体函数体里**没有**
//    `Network.responseReceived`；
// ③ **取体在等待之后** —— 调用顺序上「等 loadingFinished」**先于**「取体」
//    （源码行序）；
// ④ **空体有专门处理** —— 取体函数体里有空体判定（空体 ⇒ 回程失败，
//    **不许**被当成「站点没数据」）。
//
// @module scripts/check-official-chat-p4-body-timing

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

/** 等 `loadingFinished` 的那个函数（**实现侧约定**：等待与取体分属两步）。 */
export const WAIT_FN = 'await_target_hit';
/** 取体的那个函数。 */
export const TAKE_FN = 'take_response_body';
/** 编排两者的那个函数。 */
export const ORCHESTRATE_FN = 'capture_in';

/**
 * 读一份**读数**（取数模块源码）。
 *
 * @param {object} reading
 * @param {string} [reading.rust] - 取数模块源码。
 * @returns {object} 归一后的读数。
 */
export function normalizeReading(reading) {
  // ⚠️ **只看代码行** —— 注释里写的字样**不算数**。
  const rust = stripComments(String(reading?.rust ?? '')).join('\n');
  const wait = fnBody(rust, WAIT_FN);
  const take = fnBody(rust, TAKE_FN);
  const orchestrate = fnBody(rust, ORCHESTRATE_FN);
  return {
    hasRust: rust.trim() !== '',
    waitHandlesFinished: wait.includes('Network.loadingFinished'),
    takeMentionsResponseReceived: take.includes('Network.responseReceived'),
    takeHasEmptyGuard: /is_empty\(\)/.test(take),
    waitCallLine: lineOf(orchestrate, `${WAIT_FN}(`),
    takeCallLine: lineOf(orchestrate, `${TAKE_FN}(`),
  };
}

/**
 * 判定内核。
 *
 * @param {object} reading - 读数。
 * @returns {{ok: boolean, checks: Array<object>}}
 */
export function judgeBodyTiming(reading) {
  const checks = [];
  const add = (name, status, detail) => checks.push({ name, status, detail });
  const r = normalizeReading(reading);

  if (!r.hasRust) {
    add('P4-10 读到了取数模块源码', SKIPPED, `没读到 ${RUST_REL} ⇒ 判据未执行`);
    return judgeChecks(checks);
  }

  add(
    'P4-10 **等得到 `Network.loadingFinished`**（取体的前置条件，§8.5.1 硬约束 1）',
    r.waitHandlesFinished ? PASS : FAIL,
    r.waitHandlesFinished
      ? `${WAIT_FN}() 处理了 Network.loadingFinished`
      : `${WAIT_FN}() 里没有 Network.loadingFinished`,
  );

  add(
    'P4-10 **不存在「收到 responseReceived 就取体」的代码路径**',
    r.takeMentionsResponseReceived ? FAIL : PASS,
    r.takeMentionsResponseReceived
      ? `${TAKE_FN}() 里出现了 Network.responseReceived`
      : `${TAKE_FN}() 里没有 Network.responseReceived`,
  );

  const ordered =
    r.waitCallLine > 0 && r.takeCallLine > 0 && r.waitCallLine < r.takeCallLine;
  add(
    'P4-10 **取体在等待之后**（源码行序：先等 loadingFinished，再取体）',
    ordered ? PASS : FAIL,
    ordered
      ? `等待在第 ${r.waitCallLine} 行、取体在第 ${r.takeCallLine} 行`
      : `等待行=${r.waitCallLine}／取体行=${r.takeCallLine}`,
  );

  add(
    'P4-10 **空体有专门处理**（空体 ⇒ 回程失败，不是站点没数据，§6.3）',
    r.takeHasEmptyGuard ? PASS : FAIL,
    r.takeHasEmptyGuard
      ? `${TAKE_FN}() 里有空体判定`
      : `${TAKE_FN}() 里没有空体判定（空 body 会被当成「站点没数据」）`,
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
      'async fn await_target_hit(events: &mut EventStream) -> Result<TargetHit, CaptureFailure> {',
      '    match event.name.as_str() {',
      '        "Network.loadingFinished" => { return Ok(hit); }',
      '        _ => {}',
      '    }',
      '}',
      'async fn take_response_body(window: &WebviewWindow, id: &str) -> Result<String, CaptureFailure> {',
      '    let envelope = cdp_call(window, "Network.getResponseBody", json!({ "requestId": id }), t).await?;',
      '    let body = cdp_field(&envelope, "body").and_then(|v| v.as_str()).unwrap_or_default().to_string();',
      '    if body.is_empty() { return Err(CaptureFailure::Return("空体".into())); }',
      '    Ok(body)',
      '}',
      'async fn capture_in(window: &WebviewWindow, events: EventStream) -> Result<SessionStructure, CaptureFailure> {',
      '    let hit = await_target_hit(&mut events).await?;',
      '    let body = take_response_body(window, &hit.request_id).await?;',
      '    Ok(SessionStructure::from_body(&body, hit.status)?)',
      '}',
    ].join('\n'),
  };
  return [
    { id: 'A', name: '场景A 真身时序（等 loadingFinished → 再取体）⇒ 应通过', expect: PASS, reading: clean },
    {
      id: 'B',
      name: '场景B 取体函数里改成「收到 responseReceived 就取体」⇒ 应失败',
      expect: FAIL,
      reading: {
        ...clean,
        rust: clean.rust.replace(
          'let envelope = cdp_call(window, "Network.getResponseBody"',
          'if event == "Network.responseReceived" { }\n    let envelope = cdp_call(window, "Network.getResponseBody"',
        ),
      },
    },
    {
      id: 'C',
      name: '场景C 取体排在等待之前 ⇒ 应失败',
      expect: FAIL,
      reading: {
        ...clean,
        rust: clean.rust.replace(
          '    let hit = await_target_hit(&mut events).await?;\n    let body = take_response_body(window, &hit.request_id).await?;',
          '    let body = take_response_body(window, "x").await?;\n    let hit = await_target_hit(&mut events).await?;',
        ),
      },
    },
    {
      id: 'D',
      name: '场景D 没有空体判定（空 body 会被当成站点没数据）⇒ 应失败',
      expect: FAIL,
      reading: {
        ...clean,
        rust: clean.rust.replace('    if body.is_empty() { return Err(CaptureFailure::Return("空体".into())); }\n', ''),
      },
    },
  ];
}

/**
 * 读真实仓库的读数。
 *
 * @param {string} root - 仓库根。
 * @returns {{rust: string}}
 */
export function readReading(root) {
  const sources = readSources(root, P4_SCAN_FILES);
  return { rust: sources.find((s) => s.path === RUST_REL)?.code ?? '' };
}

/**
 * 主入口。
 *
 * @param {string[]} [argv] - 命令行参数。
 * @returns {Promise<{results: Array<object>, summary: object}>}
 */
export async function main(argv = process.argv.slice(2)) {
  const root = argValue(argv, '--target') ?? repoRoot(import.meta.url);
  const round = argValue(argv, '--round') ?? 'p4c10';
  const asJson = argv.includes('--json');

  const sources = readSources(root, P4_SCAN_FILES);
  const reading = readReading(root);
  const { ok, checks } = judgeBodyTiming(reading);

  const results = [];
  for (const c of checks) results.push({ name: c.name, status: c.status, detail: c.detail });
  results.push({
    name: 'P4-10 真实仓库读数的判定',
    status: ok ? PASS : FAIL,
    detail: ok ? '真实读数全过' : '真实读数未全过（见上）',
  });
  for (const r of runScenarios(buildScenarios(), judgeBodyTiming, 'P4-10a')) {
    results.push({ name: r.name, status: r.status, detail: r.detail });
  }

  return emit({
    round,
    sources,
    scanLabel: 'P4 实现（壳侧取数模块 ＋ 现役回环桥 ＋ 插件 host 半边）',
    reading,
    results,
    evidenceDir: join(tmpdir(), 'dsh-p4-body-timing'),
    asJson,
  });
}

// 直接跑（被 import 时不跑）
if (process.argv[1] && process.argv[1].endsWith('check-official-chat-p4-body-timing.mjs')) {
  await main();
}
