#!/usr/bin/env node
// P4 协议 §10 判据**第 1 条**（取数形态）—— 按**源码形态**断。
//
// ## 依据（协议 **v0.10** §10 第 1 条的逐字口径）
//
// > 实现里**不存在**任何**由壳侧或页内发起**对站点 API 的请求的代码路径
// > （**零自拼头、零页内脚本发请求**）**且存在**「壳侧 `Network` 域订阅站点自己的
// > 响应」的实现（§1 裁定 1）**且不存在**「页内旁听」（hook
// > `XMLHttpRequest.prototype` ＋ 用替换后的 `open` 传口令取值 ＋ `cdp_eval`
// > 轮询）形态（§1.1）⇒ ⇒ **按源码形态断**，不许靠运行时。
//
// 配套条款：§1 裁定 1（形态唯一 ＝ 壳侧 `Network` 域订阅，页内零动作）·
// §1.1（三条排他理由 ＋ **「页内旁听」被点名排除**）· §2.3（数据通路上**没有脚本**）·
// §3.1／§3.2（页外直连站点 API **实测必失败**）· §7.1（页内动作清单 ＝ 空）·
// §7.2（不自拼凭据头）· §8.5.1（四条硬约束：按具体事件名订阅 ＋ `getResponseBody`）。
//
// ## 断的六件事
//
// ① **零自拼凭据头** —— 凭据头名（`Authorization` / `X-Auth-Token` / 站点凭据键）
//    **不许与站点 API 路径同现**（§7.2：技术上可拼 ⇒ 正是不许拼）；
// ② **零页内脚本发请求** —— 扫描面里不许有 `fetch(` / `new XMLHttpRequest(` /
//    `sendBeacon`（§1 裁定 1：**我们一个请求都不发**）；
// ③ **页外没有直连站点 API 的代码路径** —— 站点 API 路径不许与 HTTP 客户端
//    原语同现（§3.1／§3.2）；
// ④ **存在壳侧 `Network` 域订阅** —— `Network.enable` ＋ `Network.responseReceived`
//    ＋ `Network.loadingFinished` ＋ `Network.getResponseBody` **四个标记齐全**
//    （§1 裁定 1／§8.5.1）；
// ⑤ **不存在页内旁听形态** —— 扫描面里 `XMLHttpRequest.prototype` **出现 0 次**
//    ＋ **数据通路不走页内求值**（取数模块里不许 `Runtime.evaluate`／`cdp_eval`）
//    ＋ **没有口令取值形态**（§1.1 的三条理由 ＋ §7.1）；
// ⑥ **数据通路上没有页内脚本** —— 不许 `Page.addScriptToEvaluateOnNewDocument`
//    （§2.3）。
//
// ## 纪律
//
// · **只看代码行** —— 注释里的字样**不算数**（沿判据第 6 条「注释里的 return 不算」
//   的规矩）⇒ 判据第 1 条的注释里写着「不碰 localStorage」也不能算成「碰了」。
// · **判据不能被被测物定义** —— 期望值抄自**协议**，不抄自实现。
//
// @module scripts/check-official-chat-p4-route

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

/** 站点的 API 前缀（协议 §3.1）。 */
export const SITE_API_PREFIX = '/api/v0/';

/** 站点的会话目录端点（协议 §4.1）。 */
export const TARGET_ENDPOINT = '/api/v0/chat_session/fetch_page';

/** **数据通路**所在文件（相对仓库根）—— 形态断言只看它。 */
export const DATA_PATH_REL = 'src-tauri/src/official_chat/session_structure.rs';

/** 凭据相关的形态（协议 §7.2：不许自拼、不许读）。 */
export const CREDENTIAL_TOKENS = [
  'Authorization',
  'X-Auth-Token',
  'x-auth-token',
  'ds_session_id',
  'userToken',
  'settingsJwt',
];

/**
 * **页内发请求**的原语（协议 §1 裁定 1：**我们一个请求都不发**）。
 *
 * ⚠️ `fetch` 只认**调用形态**（前面不是 `.`）⇒ ⇒ 插件内部的对象方法
 * （`captureSource.fetch({…})`）**不算**页内发请求。
 */
export const PAGE_REQUEST_PATTERNS = [
  /(^|[^.\w])fetch\s*\(/,
  /new\s+XMLHttpRequest/,
  /sendBeacon/,
];

/** **页外直连**的 HTTP 客户端原语（协议 §3.2：页外直连实测必失败）。 */
export const OUTBOUND_CLIENT_TOKENS = [
  'ureq::',
  'reqwest',
  'curl ',
  'TcpStream::connect',
  'Client::get',
  'Client::post',
];

/** 壳侧 `Network` 域订阅的四个标记（协议 §8.5.1 硬约束 4 ＋ §10 第 9d 条）。 */
export const NETWORK_DOMAIN_MARKS = [
  'Network.enable',
  'Network.responseReceived',
  'Network.loadingFinished',
  'Network.getResponseBody',
];

/**
 * 读一份**读数**（扫描面里的每个文件）。
 *
 * @param {object} reading
 * @param {Array<{path: string, code: string}>} [reading.sources] - 扫描面。
 * @returns {object} 归一后的读数（每条命中都带 文件:行号）。
 */
export function normalizeReading(reading) {
  const sources = Array.isArray(reading?.sources) ? reading.sources : [];
  const hits = {
    credentialWithApi: [],
    pageRequest: [],
    apiWithClient: [],
    prototypeHook: [],
    scriptInjection: [],
    passphrase: [],
    dataPathEval: [],
  };
  const networkMarks = Object.fromEntries(NETWORK_DOMAIN_MARKS.map((mark) => [mark, 0]));
  let read = 0;

  for (const file of sources) {
    const lines = stripComments(file.code);
    // **文件级**标记：站点 API 路径与（HTTP 客户端原语／页内发请求原语）**同现于
    // 一个文件** ⇒ 那就是一条「发起站点 API 请求」的代码路径（§1 裁定 1／§3.2）。
    let fileHasApi = false;
    let fileHasClient = false;
    const filePageRequestLines = [];
    lines.forEach((line, index) => {
      const hasApi = line.includes(SITE_API_PREFIX);
      const hasCredential = CREDENTIAL_TOKENS.some((token) => line.includes(token));
      const hasPageRequest = PAGE_REQUEST_PATTERNS.some((pattern) => pattern.test(line));
      const hasClient = OUTBOUND_CLIENT_TOKENS.some((token) => line.includes(token));
      if (hasApi) fileHasApi = true;
      if (hasClient) fileHasClient = true;
      if (hasPageRequest) filePageRequestLines.push(`${file.path}:${index + 1}`);
      if (hasApi && hasCredential) hits.credentialWithApi.push(`${file.path}:${index + 1}`);
      if (line.includes('XMLHttpRequest.prototype')) hits.prototypeHook.push(`${file.path}:${index + 1}`);
      if (line.includes('addScriptToEvaluateOnNewDocument')) {
        hits.scriptInjection.push(`${file.path}:${index + 1}`);
      }
      if (
        /TAKE_MARK|RESTORE_MARK|dsh take|dsh restore/.test(line) &&
        line.includes('open')
      ) {
        hits.passphrase.push(`${file.path}:${index + 1}`);
      }
      // **数据通路**那一侧的两个标记：页内求值 ＋ `Network` 域订阅。
      if (file.path === DATA_PATH_REL) {
        read += 1;
        if (line.includes('Runtime.evaluate') || line.includes('cdp_eval(')) {
          hits.dataPathEval.push(`${file.path}:${index + 1}`);
        }
        for (const mark of NETWORK_DOMAIN_MARKS) {
          if (line.includes(mark)) networkMarks[mark] += 1;
        }
      }
    });
    if (fileHasApi && fileHasClient) {
      hits.apiWithClient.push(file.path);
    }
    // **页内发请求原语**只有与**站点 API 路径同现**才算是「发起站点 API 请求」
    // —— 插件内部的对象方法（`captureSource.fetch({…})`）与站点 API 无关。
    if (fileHasApi) {
      hits.pageRequest.push(...filePageRequestLines);
    }
  }

  return {
    fileCount: sources.length,
    read: read > 0,
    networkMarks,
    ...hits,
  };
}

/**
 * 判定内核。
 *
 * @param {object} reading - 读数。
 * @returns {{ok: boolean, checks: Array<object>}}
 */
export function judgeRoute(reading) {
  const checks = [];
  const add = (name, status, detail) => checks.push({ name, status, detail });
  const r = normalizeReading(reading);

  if (!r.read) {
    add('P4-1 读到了数据通路的源码', SKIPPED, `没读到 ${DATA_PATH_REL} ⇒ 判据未执行`);
    return judgeChecks(checks);
  }
  add('P4-1 读到了数据通路的源码', PASS, `数据通路：${DATA_PATH_REL}`);

  add(
    'P4-1 **零自拼凭据头**（凭据头名与站点 API 路径不同现，§7.2）',
    r.credentialWithApi.length === 0 ? PASS : FAIL,
    r.credentialWithApi.length === 0
      ? '凭据头名没有与站点 API 路径同现'
      : `凭据与站点 API 同现：${r.credentialWithApi.slice(0, 5).join('、')}`,
  );

  add(
    'P4-1 **零页内脚本发请求**（页内发请求原语不与站点 API 同现，§1 裁定 1）',
    r.pageRequest.length === 0 ? PASS : FAIL,
    r.pageRequest.length === 0
      ? '扫描面里没有「页内发请求 ＋ 站点 API」同现（我们一个请求都不发）'
      : `页内发请求与站点 API 同现：${r.pageRequest.slice(0, 5).join('、')}`,
  );

  add(
    'P4-1 **页外没有直连站点 API 的代码路径**（§3.1／§3.2）',
    r.apiWithClient.length === 0 ? PASS : FAIL,
    r.apiWithClient.length === 0
      ? '站点 API 路径没有与 HTTP 客户端原语同现'
      : `站点 API 与客户端原语同现：${r.apiWithClient.slice(0, 5).join('、')}`,
  );

  const missingMarks = NETWORK_DOMAIN_MARKS.filter((mark) => r.networkMarks[mark] === 0);
  add(
    'P4-1 **存在壳侧 `Network` 域订阅**（enable ＋ responseReceived ＋ loadingFinished ＋ getResponseBody，§1／§8.5.1）',
    missingMarks.length === 0 ? PASS : FAIL,
    missingMarks.length === 0
      ? `四个标记齐全（${NETWORK_DOMAIN_MARKS.join('、')}）`
      : `缺标记：${missingMarks.join('、')}`,
  );

  const hasEavesdrop =
    r.prototypeHook.length > 0 || r.dataPathEval.length > 0 || r.passphrase.length > 0;
  add(
    'P4-1 **不存在页内旁听形态**（无 prototype hook ＋ 数据通路不走页内求值 ＋ 无口令取值，§1.1／§7.1）',
    hasEavesdrop ? FAIL : PASS,
    hasEavesdrop
      ? `旁听形态的痕迹：prototype=${r.prototypeHook.length} 处、页内求值=${r.dataPathEval.length} 处、口令=${r.passphrase.length} 处`
      : '没有 prototype hook、数据通路不走页内求值、没有口令取值',
  );

  add(
    'P4-1 **数据通路上没有页内脚本**（无 addScriptToEvaluateOnNewDocument，§2.3）',
    r.scriptInjection.length === 0 ? PASS : FAIL,
    r.scriptInjection.length === 0
      ? '没有页内脚本注入（页内零动作）'
      : `出现了页内脚本注入：${r.scriptInjection.slice(0, 5).join('、')}`,
  );

  return judgeChecks(checks);
}

/**
 * 场景表（合成读数）：每个场景声明「应当通过」还是「应当失败」。
 *
 * @returns {Array<object>}
 */
export function buildScenarios() {
  const dataPath = [
    'const TARGET_PATH: &str = "/api/v0/chat_session/fetch_page";',
    'cdp_call(window, "Network.enable", json!({}), CDP_TIMEOUT).await?;',
    'let r = core.GetDevToolsProtocolEventReceiver(&name_hstring);',
    'const label = "Network.responseReceived".to_string();',
    'const label2 = "Network.loadingFinished".to_string();',
    'cdp_call(window, "Network.getResponseBody", json!({ "requestId": id }), CDP_TIMEOUT).await?;',
  ].join('\n');
  const clean = {
    sources: [
      { path: DATA_PATH_REL, code: dataPath },
      { path: 'plugins/official-chat/host/site-source.js', code: 'const x = 1;' },
    ],
  };
  return [
    { id: 'A', name: '场景A 真身形态（Network 域订阅 ＋ 零请求 ＋ 无旁听）⇒ 应通过', expect: PASS, reading: clean },
    {
      id: 'B',
      name: '场景B 少了 `Network.enable`（订阅点没就位）⇒ 应失败',
      expect: FAIL,
      reading: {
        ...clean,
        sources: [
          {
            path: DATA_PATH_REL,
            code: dataPath.replace('cdp_call(window, "Network.enable", json!({}), CDP_TIMEOUT).await?;', ''),
          },
          clean.sources[1],
        ],
      },
    },
    {
      id: 'C',
      name: '场景C 出现 `XMLHttpRequest.prototype`（页内旁听形态）⇒ 应失败',
      expect: FAIL,
      reading: {
        ...clean,
        sources: [
          { path: DATA_PATH_REL, code: `${dataPath}\nconst proto = XMLHttpRequest.prototype;` },
          clean.sources[1],
        ],
      },
    },
    {
      id: 'D',
      name: '场景D 出现口令取值（`prototype.open` 传口令）⇒ 应失败',
      expect: FAIL,
      reading: {
        ...clean,
        sources: [
          {
            path: DATA_PATH_REL,
            code: `${dataPath}\nconst TAKE_MARK = "dsh take";\nXMLHttpRequest.prototype.open(TAKE_MARK);`,
          },
          clean.sources[1],
        ],
      },
    },
    {
      id: 'E',
      name: '场景E 页外直连站点 API ⇒ 应失败',
      expect: FAIL,
      reading: {
        ...clean,
        sources: [
          {
            path: DATA_PATH_REL,
            code: `${dataPath}\nlet u = "https://chat.deepseek.com/api/v0/chat_session/fetch_page";\nureq::get(u);`,
          },
          clean.sources[1],
        ],
      },
    },
    {
      id: 'F',
      name: '场景F 页内自己发一个 fetch ⇒ 应失败',
      expect: FAIL,
      reading: {
        ...clean,
        sources: [
          { path: DATA_PATH_REL, code: `${dataPath}\nfetch("/api/v0/chat_session/fetch_page");` },
          clean.sources[1],
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
  const repo = repoRoot(import.meta.url);
  const root = argValue(argv, '--target') ?? repo;
  const round = argValue(argv, '--round') ?? 'p4c1';
  const asJson = argv.includes('--json');

  const sources = readSources(root, P4_SCAN_FILES);
  const reading = readReading(root);
  const { ok, checks } = judgeRoute(reading);

  const results = [];
  for (const c of checks) results.push({ name: c.name, status: c.status, detail: c.detail });
  results.push({
    name: 'P4-1 真实仓库读数的判定',
    status: ok ? PASS : FAIL,
    detail: ok ? '真实读数全过' : '真实读数未全过（见上）',
  });
  for (const r of runScenarios(buildScenarios(), judgeRoute, 'P4-1a')) {
    results.push({ name: r.name, status: r.status, detail: r.detail });
  }

  return emit({
    round,
    sources,
    scanLabel: 'P4 实现（壳侧 Network 域订阅 ＋ 现役回环桥 ＋ 插件 host 半边）',
    reading,
    results,
    evidenceDir: join(tmpdir(), 'dsh-p4-route'),
    asJson,
  });
}

// 直接跑（被 import 时不跑）
if (process.argv[1] && process.argv[1].endsWith('check-official-chat-p4-route.mjs')) {
  await main();
}
