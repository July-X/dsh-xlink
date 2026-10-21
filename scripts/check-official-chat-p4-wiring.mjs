#!/usr/bin/env node
// P4 协议 §10 判据**第 4 条**（接线）—— 按**源码形态**断。
//
// ## 依据
//
// · 协议 §5.1：「**P4 只在现役桥上加一条路由** ⇒ **复用现役服务，不是新造** ⇒
//   **不撞**计划 §4「不再造一个监听服务」的否决」。
// · 协议 §5.2 第 1 条：「**必须 POST ＋ 支持读请求体**……**并写死 body 上限**」；
//   第 2 条：「**不能走查询串**」（现役客户端 `MAX_PATH_BYTES` ＝ 7000）。
// · 协议 §5.3：**外挂 CDP 不可行** ⇒ 调用方必须是壳那个进程自己 ⇒ 路由必须在壳里。
//
// ## 断的五件事
//
// ① 路由挂在**现役**回环桥上（`src-tauri/src/plugins/builtin/bridge.rs` 里命中）；
// ② **两端路径全等**（壳侧 ＝ 插件侧）——错一个字符就是 404，且症状是「插件没接上」；
// ③ 方法是 **POST**（§5.2 第 1 条）；
// ④ **读请求体 ＋ 写死上限**（§5.2 第 1 条）；
// ⑤ **没有新监听端口**（扫描面里除现役桥之外不许再出现 `TcpListener::bind`）——
//    这条正是「不新造监听服务」的可机检形态。
//
// @module scripts/check-official-chat-p4-wiring

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

/** 现役回环桥（相对仓库根）。 */
export const BRIDGE_REL = 'src-tauri/src/plugins/builtin/bridge.rs';
/** 插件侧的桥客户端（相对仓库根）。 */
export const CLIENT_REL = 'plugins/official-chat/host/site-source.js';

/**
 * 判定内核。
 *
 * @param {object} reading - 读数。
 * @returns {{ok: boolean, checks: Array<object>}}
 */
export function judgeWiring(reading) {
  const checks = [];
  const add = (name, status, detail) => checks.push({ name, status, detail });
  const bridge = String(reading?.bridge ?? '');
  const client = String(reading?.client ?? '');
  const others = Array.isArray(reading?.others) ? reading.others : [];

  if (bridge.trim() === '') {
    add('P4-4 路由挂在现役回环桥上', SKIPPED, '没读到现役桥 ⇒ 判据未执行');
    return judgeChecks(checks);
  }

  const routeMatch = /\(\s*"POST"\s*,\s*"([^"]+)"\s*\)/.exec(bridge);
  const bridgeRoute = routeMatch ? routeMatch[1] : '';
  add(
    'P4-4 路由挂在现役回环桥上（§5.1：不新造服务）',
    bridgeRoute !== '' ? PASS : FAIL,
    bridgeRoute !== '' ? `现役桥上的 POST 路由：${bridgeRoute}` : '现役桥里没有 POST 路由',
  );

  const clientMatch = /ROUTE_PATH\s*=\s*'([^']+)'/.exec(client);
  const clientRoute = clientMatch ? clientMatch[1] : '';
  add(
    'P4-4 两端路径**全等**（壳侧 ＝ 插件侧）',
    bridgeRoute !== '' && clientRoute !== '' && bridgeRoute === clientRoute
      ? PASS
      : FAIL,
    `壳侧=${bridgeRoute || '（没抽到）'}／插件侧=${clientRoute || '（没抽到）'}`,
  );

  add(
    'P4-4 方法是 POST（§5.2 第 1 条）',
    bridgeRoute !== '' ? PASS : FAIL,
    bridgeRoute !== '' ? '路由臂是 ("POST", …)' : '没抽到 POST 路由',
  );

  const hasReadBody = bridge.includes('read_body(');
  const hasCap = /const\s+MAX_[A-Z_]*BODY_BYTES\s*:\s*usize\s*=/.test(bridge);
  const capUsed = /read_body\([\s\S]{0,200}MAX_[A-Z_]*BODY_BYTES/.test(bridge);
  add(
    'P4-4 读请求体 ＋ **写死 body 上限**（§5.2 第 1 条）',
    hasReadBody && hasCap && capUsed ? PASS : FAIL,
    `读体=${hasReadBody}／上限常量=${hasCap}／上限被用上=${capUsed}`,
  );

  const newListeners = [];
  for (const file of others) {
    stripComments(file.code).forEach((line, index) => {
      if (line.includes('TcpListener::bind')) newListeners.push(`${file.path}:${index + 1}`);
    });
  }
  add(
    'P4-4 **没有新监听端口**（§5.1：不新造服务）',
    newListeners.length === 0 ? PASS : FAIL,
    newListeners.length === 0
      ? '扫描面里（现役桥之外）没有第二处 TcpListener::bind'
      : `出现了新的监听：${newListeners.join('、')}`,
  );

  add(
    'P4-4 现役桥仍然是**回环**（127.0.0.1）',
    bridge.includes('127.0.0.1') ? PASS : FAIL,
    bridge.includes('127.0.0.1') ? '桥仍绑在回环地址上' : '桥的绑定地址里没有 127.0.0.1',
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
    bridge: [
      'const MAX_SESSION_STRUCTURE_BODY_BYTES: usize = 1024;',
      '("POST", "/v1/official-chat/session-structure") => {',
      '  let body = crate::openai::http::read_body(stream, &request, MAX_SESSION_STRUCTURE_BODY_BYTES);',
      '}',
      'let listener = TcpListener::bind("127.0.0.1:0").ok()?;',
    ].join('\n'),
    client: "export const ROUTE_PATH = '/v1/official-chat/session-structure';",
    others: [],
  };
  return [
    { id: 'A', name: '场景A 真身接线（现役桥 ＋ POST ＋ 读体 ＋ 上限）⇒ 应通过', expect: PASS, reading: clean },
    {
      id: 'B',
      name: '场景B 两端路径不一致 ⇒ 应失败',
      expect: FAIL,
      reading: { ...clean, client: "export const ROUTE_PATH = '/v1/official-chat/session-structure-2';" },
    },
    {
      id: 'C',
      name: '场景C 没有写死 body 上限 ⇒ 应失败',
      expect: FAIL,
      reading: {
        ...clean,
        bridge: clean.bridge
          .replace('const MAX_SESSION_STRUCTURE_BODY_BYTES: usize = 1024;', '')
          .replace(', MAX_SESSION_STRUCTURE_BODY_BYTES', ''),
      },
    },
    {
      id: 'D',
      name: '场景D 另起了一个监听端口 ⇒ 应失败',
      expect: FAIL,
      reading: { ...clean, others: [{ path: 'x.rs', code: 'let l = TcpListener::bind("127.0.0.1:0")?;' }] },
    },
    {
      id: 'E',
      name: '场景E 路由不在现役桥上（没抽到 POST 路由）⇒ 应失败',
      expect: FAIL,
      reading: { ...clean, bridge: 'fn nothing() {}' },
    },
  ];
}

/**
 * 读真实仓库的读数。
 *
 * @param {string} root - 仓库根。
 * @returns {{bridge: string, client: string, others: Array<object>}}
 */
export function readReading(root) {
  const sources = readSources(root, P4_SCAN_FILES);
  const bridge = sources.find((s) => s.path === BRIDGE_REL)?.code ?? '';
  const client = sources.find((s) => s.path === CLIENT_REL)?.code ?? '';
  return {
    bridge,
    client,
    others: sources.filter((s) => s.path !== BRIDGE_REL),
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
  const round = argValue(argv, '--round') ?? 'p4c4';
  const asJson = argv.includes('--json');

  const sources = readSources(root, P4_SCAN_FILES);
  const reading = readReading(root);
  const { ok, checks } = judgeWiring(reading);

  const results = [];
  for (const c of checks) results.push({ name: c.name, status: c.status, detail: c.detail });
  results.push({
    name: 'P4-4 真实仓库读数的判定',
    status: ok ? PASS : FAIL,
    detail: ok ? '真实读数全过' : '真实读数未全过（见上）',
  });
  for (const r of runScenarios(buildScenarios(), judgeWiring, 'P4-4a')) {
    results.push({ name: r.name, status: r.status, detail: r.detail });
  }

  return emit({
    round,
    sources,
    scanLabel: 'P4 实现（现役回环桥 ＋ 插件桥客户端 ＋ 壳侧取数模块）',
    reading: { bridge: '（已读）', client: '（已读）', others: reading.others.map((o) => o.path) },
    results,
    evidenceDir: join(tmpdir(), 'dsh-p4-wiring'),
    asJson,
  });
}

// 直接跑（被 import 时不跑）
if (process.argv[1] && process.argv[1].endsWith('check-official-chat-p4-wiring.mjs')) {
  await main();
}
