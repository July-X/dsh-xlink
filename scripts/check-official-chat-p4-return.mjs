#!/usr/bin/env node
// P4 协议 §10 判据**第 3 条**（回程只回结构）—— 按**回传字段清单**断（白名单之外即红）。
//
// ## 依据
//
// · 协议 §2.4（回程白名单）：**允许回传** ＝ 顶层键名、`code`/`msg`（原样字符串、
//   不解释语义）、`data` 的键名数组、数组的长度、数组首元素的键名数组、条目的
//   类型/布尔量；**不许回传** ＝ **正文**、**标题**、**会话 id**、**任何
//   token/cookie/凭据**、**站点的完整响应体**。
// · 协议 §8.5.1 末段：`Network` 域拿到的是**站点的完整响应体**（里面**含**
//   `title` / `id`）⇒ ⇒ **不许**把 `getResponseBody` 的原文直接回传
//   ⇒ ⇒ 白名单裁剪**在壳侧**做（协议 §1.1 的「资产不浪费条款」）。
// · 协议 §7.2：凭据零落盘、零外传。
//
// ## 白名单是**逐字列举**（不是「差不多」）
//
// §2.4 的「允许回传」是**列举式**的 ⇒ ⇒ 白名单之外即红 ⇒ ⇒ 本判据的白名单
// **就是 §2.4 那几项，一项不多**：
//
// | 回程字段 | §2.4 的哪一条 |
// | --- | --- |
// | `topKeys` | 顶层键名 |
// | `code` / `msg` | `code` / `msg`（原样字符串，不解释语义） |
// | `dataKeys` | `data` 的键名数组 |
// | `sessionsLength` | 数组的长度 |
// | `firstItemKeys` | 数组首元素的键名数组 |
// | `itemTypes` | 条目的类型 |
// | `hasMore` | 条目的布尔量（例如「是否有更多页」） |
//
// ⚠️ **`status`（HTTP 状态码）与 `bizCode`（业务层码）不在白名单里** ⇒ ⇒
// **不进回程载荷**：它们只在**壳侧**参与判断（§4.2 的成功判据要 `status == 200`；
// §6.2 的「两层都要看」要 `biz_code`）⇒ ⇒ 站内码要上报时走**失败 detail**
// （§6.2「原样上报」），那**不是结构回程**。
//
// ## 断的五件事
//
// ① 壳侧回程载荷的字段名 **⊆ 白名单**（白名单之外即红）；
// ② 插件侧回包校验的字段名 **⊆ 白名单**（两端口径必须一致）；
// ③ 两端字段清单**逐项相等**；
// ④ 白名单里**没有**禁回传的那几类（正文/标题/会话 id/凭据）；
// ⑤ 回程载荷**不是**站点的完整响应体（没有「整包带走」的字段）。
//
// @module scripts/check-official-chat-p4-return

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
  objectLiteralKeysContaining,
  readSources,
  repoRoot,
  runScenarios,
} from './lib/p4-source-shape.mjs';

// 三态常量**从共用原语转出去**（自测文件从本模块取它们）。
export { FAIL, PASS, SKIPPED };

/** 回程白名单（协议 §2.4 **逐字列举**那几项）。 */
export const WHITELIST = Object.freeze([
  'topKeys',
  'code',
  'msg',
  'dataKeys',
  'sessionsLength',
  'firstItemKeys',
  'itemTypes',
  'hasMore',
]);

/** 禁回传的字段名（协议 §2.4：正文／标题／会话 id／凭据）。 */
export const FORBIDDEN = Object.freeze([
  'title',
  'id',
  'body',
  'content',
  'text',
  'messages',
  'sessionId',
  'deepLink',
  'token',
  'cookie',
  'credential',
  'userToken',
]);

/** 壳侧取数模块（相对仓库根）。 */
export const RUST_REL = 'src-tauri/src/official_chat/session_structure.rs';
/** 插件侧真实取数源（相对仓库根）。 */
export const JS_REL = 'plugins/official-chat/host/site-source.js';

/**
 * 抽一个**数组字面量**里的字符串元素（引号或裸标识符都认）。
 *
 * ## 为什么要有它
 *
 * 插件侧的「回包校验字段清单」是**数组字面量**（`const keys = [...]`）⇒ ⇒
 * `objectLiteralKeysContaining` 那套（专挑 `{…}`）抽不到 ⇒ ⇒ 这里补一个。
 *
 * @param {string} src - 源码文本。
 * @param {string} anchor - 定位用的锚点（数组就在它后面）。
 * @returns {string[]} 数组里的元素（抽不到 ⇒ 空数组）。
 */
export function arrayLiteralStrings(src, anchor) {
  const text = String(src ?? '');
  const at = text.indexOf(anchor);
  if (at < 0) return [];
  const start = text.indexOf('[', at + anchor.length - 1);
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
    if (ch === '[') {
      depth += 1;
      continue;
    }
    if (ch === ']') {
      depth -= 1;
      if (depth === 0) {
        const body = text.slice(start + 1, i);
        const out = [];
        for (const seg of body.split(',')) {
          const trimmed = seg.trim();
          const m = /^["'`]([A-Za-z_$][A-Za-z0-9_$]*)["'`]/.exec(trimmed);
          if (m) out.push(m[1]);
        }
        return out;
      }
    }
  }
  return [];
}

/**
 * 判定内核。
 *
 * @param {object} reading - 读数（`rustKeys` / `jsKeys` / `whitelist`）。
 * @returns {{ok: boolean, checks: Array<object>}}
 */
export function judgeReturn(reading) {
  const checks = [];
  const add = (name, status, detail) => checks.push({ name, status, detail });
  const whitelist = Array.isArray(reading?.whitelist) ? reading.whitelist : WHITELIST;
  const rustKeys = Array.isArray(reading?.rustKeys) ? reading.rustKeys : [];
  const jsKeys = Array.isArray(reading?.jsKeys) ? reading.jsKeys : [];

  if (rustKeys.length === 0 && jsKeys.length === 0) {
    add('P4-3 回程载荷的字段清单', SKIPPED, '没读到任何一侧的载荷 ⇒ 判据未执行');
    return judgeChecks(checks);
  }

  for (const [label, keys] of [
    ['壳侧', rustKeys],
    ['插件侧', jsKeys],
  ]) {
    if (keys.length === 0) {
      add(`P4-3 ${label}回程字段 ⊆ 白名单`, SKIPPED, `没读到${label}载荷 ⇒ 判据未执行`);
      continue;
    }
    const outside = keys.filter((k) => !whitelist.includes(k));
    add(
      `P4-3 ${label}回程字段 ⊆ 白名单`,
      outside.length === 0 ? PASS : FAIL,
      outside.length === 0
        ? `${label}载荷 ${keys.length} 个字段全在白名单内`
        : `${label}载荷出现白名单之外的字段：${outside.join('、')}`,
    );
  }

  if (rustKeys.length > 0 && jsKeys.length > 0) {
    const onlyRust = rustKeys.filter((k) => !jsKeys.includes(k));
    const onlyJs = jsKeys.filter((k) => !rustKeys.includes(k));
    add(
      'P4-3 两端载荷字段一致（壳侧 ＝ 插件侧）',
      onlyRust.length === 0 && onlyJs.length === 0 ? PASS : FAIL,
      onlyRust.length === 0 && onlyJs.length === 0
        ? '两端字段清单一致'
        : `壳侧多 ${onlyRust.join('、') || '无'}／插件侧多 ${onlyJs.join('、') || '无'}`,
    );
  }

  const forbiddenInWhitelist = whitelist.filter((k) => FORBIDDEN.includes(k));
  add(
    'P4-3 白名单里没有禁回传的字段（正文/标题/会话 id/凭据）',
    forbiddenInWhitelist.length === 0 ? PASS : FAIL,
    forbiddenInWhitelist.length === 0
      ? '白名单里没有正文、标题、会话 id、凭据'
      : `白名单里混进了：${forbiddenInWhitelist.join('、')}`,
  );

  const allKeys = [...rustKeys, ...jsKeys];
  const rawBody = allKeys.filter((k) => ['raw', 'rawBody', 'response', 'responseText', 'body'].includes(k));
  add(
    'P4-3 回程载荷**不是**站点的完整响应体',
    rawBody.length === 0 ? PASS : FAIL,
    rawBody.length === 0 ? '载荷里没有整包字段' : `载荷里出现了整包字段：${rawBody.join('、')}`,
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
    rustKeys: [...WHITELIST],
    jsKeys: [...WHITELIST],
    whitelist: [...WHITELIST],
  };
  return [
    { id: 'A', name: '场景A 真身载荷（逐字等于白名单）⇒ 应通过', expect: PASS, reading: clean },
    {
      id: 'B',
      name: '场景B 载荷里混进 `title`（标题禁止回程）⇒ 应失败',
      expect: FAIL,
      reading: { ...clean, rustKeys: [...WHITELIST, 'title'] },
    },
    {
      id: 'C',
      name: '场景C 载荷里混进 `token`（凭据禁止回程）⇒ 应失败',
      expect: FAIL,
      reading: { ...clean, jsKeys: [...WHITELIST, 'token'] },
    },
    {
      id: 'D',
      name: '场景D 载荷里混进 `body`（正文禁止回程）⇒ 应失败',
      expect: FAIL,
      reading: { ...clean, rustKeys: [...WHITELIST, 'body'] },
    },
    {
      id: 'E',
      name: '场景E 两端字段不一致 ⇒ 应失败',
      expect: FAIL,
      reading: { ...clean, jsKeys: WHITELIST.filter((k) => k !== 'hasMore') },
    },
    {
      id: 'F',
      name: '场景F 载荷里混进 `status`（白名单之外 ⇒ 红）⇒ 应失败',
      expect: FAIL,
      reading: { ...clean, rustKeys: [...WHITELIST, 'status'] },
    },
  ];
}

/**
 * 读真实仓库的读数。
 *
 * @param {string} root - 仓库根。
 * @returns {{rustKeys: string[], jsKeys: string[]}}
 */
export function readReading(root) {
  const sources = readSources(root, P4_SCAN_FILES);
  const rust = sources.find((s) => s.path === RUST_REL)?.code ?? '';
  const js = sources.find((s) => s.path === JS_REL)?.code ?? '';
  return {
    rustKeys: objectLiteralKeysContaining(rust, 'topKeys'),
    jsKeys: arrayLiteralStrings(js, 'const keys = ['),
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
  const round = argValue(argv, '--round') ?? 'p4c3';
  const asJson = argv.includes('--json');

  const sources = readSources(root, P4_SCAN_FILES);
  const reading = readReading(root);
  const { ok, checks } = judgeReturn(reading);

  const results = [];
  for (const c of checks) results.push({ name: c.name, status: c.status, detail: c.detail });
  results.push({
    name: 'P4-3 真实仓库读数的判定',
    status: ok ? PASS : FAIL,
    detail: ok ? '真实读数全过' : '真实读数未全过（见上）',
  });
  for (const r of runScenarios(buildScenarios(), judgeReturn, 'P4-3a')) {
    results.push({ name: r.name, status: r.status, detail: r.detail });
  }

  return emit({
    round,
    sources,
    scanLabel: 'P4 实现（壳侧取数模块 ＋ 插件 host 半边）',
    reading,
    results,
    evidenceDir: join(tmpdir(), 'dsh-p4-return'),
    asJson,
  });
}

// 直接跑（被 import 时不跑）
if (process.argv[1] && process.argv[1].endsWith('check-official-chat-p4-return.mjs')) {
  await main();
}
