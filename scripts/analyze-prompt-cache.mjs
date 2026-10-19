// 前缀缓存诊断：从内核会话日志里读出每次模型调用的缓存事实。
//
// 为什么要有这个脚本：2026-10-19 的命中率事故里，「为什么没命中」只能靠手工
// 翻会话日志的 JSON，而结论（每次请求回显的 `prompt_cache_key` 都是新的随机
// 键；`instructions` / `tools` 逐字不变；上游给的保留期 24h）恰恰是可重复
// 测量的。把它变成一条命令，下一次真实会话复核就不用再靠回忆——
// 「命中率到底有没有回升」从此是可判定的，不是可期待的。
//
// 读的是内核的会话日志（zstd 压缩的 JSONL），**只读**：不写、不删、不改用户
// 任何数据。内核把上游 Responses 响应原样存进 assistant 消息的
// `replayState.response`，里面有这次调用的 `usage`（含
// `attribution.request_fields` 的分字段缓存量）与回显的 `instructions` /
// `tools` / `prompt_cache_key`——三个问题一次读完：命中率多少、键稳不稳、
// 前缀变没变。
//
// 用法：
//   node scripts/analyze-prompt-cache.mjs <会话日志 .zstd | 目录>
//   目录时取其中最新的会话日志。报告只印指纹与 token 数，**不印用户正文**。
//
// 也能读**原生 Codex 的落盘日志**（`~/.codex/sessions/**/rollout-*.jsonl`）：
// 那边的事件是 `token_usage_record`，用量在 `payload.usage` 里（每次请求的
// `input_tokens` / `cached_input_tokens`）。**要的是基线**——同一个账号下
// 原生客户端命中多少，壳里就得做到多少；两侧用同一条命令量，才算同一把尺子。
// 原生落盘日志不存请求体，所以「前缀变没变」那一问在那边量不了（只印得到
// 命中率与键稳不稳）。
//
// 这条路线（SIWC 套餐）**没有** `prompt_cache_diagnostics` 字段，所以「这次
// 为什么没命中」只能从 `attribution.request_fields` 推：它分字段给出
// `instructions` / `tools` 各自的缓存量——前缀整体命中时这两个字段会等于
// 各自的 token 数，整体未命中时都是 0。

import { createHash } from 'node:crypto';
import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import * as zlib from 'node:zlib';

const digest = (value) =>
  createHash('sha256').update(String(value ?? '')).digest('hex').slice(0, 8);

/** 从一条 assistant 消息事件里取出上游 Responses 响应对象。 */
function upstreamResponse(event) {
  const stream = event?.data?.stream;
  if (!Array.isArray(stream)) return undefined;
  let response;
  for (const entry of stream) {
    const candidate = entry?.chunk?.replayState?.response;
    if (candidate?.object === 'response') response = candidate;
  }
  return response;
}

/**
 * 会话日志文本 → 每次模型调用的缓存事实。
 *
 * @param {string} text 解压后的 JSONL 全文
 * @returns {object[]} 每次调用一条
 */
export function parseCacheFacts(text) {
  const facts = [];
  for (const line of text.split('\n')) {
    if (!line.trim()) continue;
    let event;
    try {
      event = JSON.parse(line);
    } catch {
      continue; // 坏行跳过：日志尾部可能被截断
    }
    if (event?.type !== 'assistant/message') continue;
    const response = upstreamResponse(event);
    if (!response) continue;
    const usage = response.usage ?? {};
    const details = usage.input_tokens_details ?? {};
    const fields = usage.attribution?.request_fields ?? {};
    facts.push({
      seq: event.seq ?? null,
      turn: event.data?.turn ?? null,
      step: event.data?.step ?? null,
      model: response.model ?? null,
      key: response.prompt_cache_key ?? null,
      retention: response.prompt_cache_retention ?? null,
      inputTokens: usage.input_tokens ?? 0,
      cachedTokens: details.cached_tokens ?? 0,
      cacheWriteTokens: details.cache_write_tokens ?? 0,
      instructionsTokens: fields.instructions?.input_tokens ?? null,
      instructionsCached: fields.instructions?.cached_tokens ?? null,
      toolsTokens: fields.tools?.input_tokens ?? null,
      toolsCached: fields.tools?.cached_tokens ?? null,
      instructionsHash: digest(response.instructions ?? ''),
      toolsHash: digest(JSON.stringify(response.tools ?? null)),
      source: 'dsh',
    });
  }
  return facts;
}

/**
 * 原生 Codex 落盘日志 → 每次模型调用的缓存事实。
 *
 * Codex 的 `token_usage_record` 事件带 `payload.usage`（**这次请求**的用量，
 * 不是累计——累计在 `turn_token_usage` / `thread_token_usage` 里，别拿错）：
 * `input_tokens` 与 `cached_input_tokens`。键是 `payload.session_id`（会话 id，
 * 原生实现拿它当缓存亲和身份）。
 *
 * 它**不存请求体**，所以前缀指纹量不了——这里不编，指纹留空。
 *
 * @param {string} text rollout JSONL 全文
 * @returns {object[]} 每次调用一条
 */
export function parseCodexFacts(text) {
  const facts = [];
  for (const line of text.split('\n')) {
    if (!line.trim()) continue;
    let event;
    try {
      event = JSON.parse(line);
    } catch {
      continue; // 坏行跳过
    }
    if (event?.type !== 'token_usage_record') continue;
    const usage = event.payload?.usage;
    if (!usage) continue;
    facts.push({
      seq: event.ordinal ?? null,
      turn: null,
      step: null,
      model: null,
      key: event.payload?.session_id ?? null,
      retention: null,
      inputTokens: usage.input_tokens ?? 0,
      cachedTokens: usage.cached_input_tokens ?? 0,
      cacheWriteTokens: usage.cache_write_input_tokens ?? 0,
      instructionsTokens: null,
      instructionsCached: null,
      toolsTokens: null,
      toolsCached: null,
      instructionsHash: null,
      toolsHash: null,
      source: 'codex',
    });
  }
  return facts;
}

/**
 * 文本 → 缓存事实（两种日志形状自动分辨）。
 *
 * 原生 Codex 的事件类型（`token_usage_record`）不会出现在内核会话日志里，
 * 反之亦然，所以「先认 Codex，认不出再认内核」就够，不用让调用方选格式。
 *
 * @param {string} text 日志全文
 * @returns {object[]} 每次调用一条
 */
export function parseFacts(text) {
  const codex = parseCodexFacts(text);
  if (codex.length > 0) return codex;
  return parseCacheFacts(text);
}

/**
 * 一次调用的缓存事实 → 汇总。
 *
 * @param {object[]} facts parseCacheFacts 的结果
 */
export function summarizeCacheFacts(facts) {
  const totalInput = facts.reduce((sum, fact) => sum + (fact.inputTokens ?? 0), 0);
  const totalCached = facts.reduce((sum, fact) => sum + (fact.cachedTokens ?? 0), 0);
  const keys = new Set(facts.map((fact) => fact.key).filter(Boolean));
  const instructions = new Set(facts.map((fact) => fact.instructionsHash));
  const tools = new Set(facts.map((fact) => fact.toolsHash));
  const source = facts[0]?.source ?? 'dsh';
  return {
    calls: facts.length,
    source,
    // 前缀指纹只有内核会话日志有（原生 Codex 落盘日志不存请求体）。
    prefixKnown: source === 'dsh',
    distinctKeys: keys.size,
    distinctInstructions: instructions.size,
    distinctTools: tools.size,
    totalInput,
    totalCached,
    ratio: totalInput > 0 ? totalCached / totalInput : 0,
    hitCalls: facts.filter((fact) => (fact.cachedTokens ?? 0) > 0).length,
    missCalls: facts.filter((fact) => (fact.cachedTokens ?? 0) === 0).length,
    retention: [...new Set(facts.map((fact) => fact.retention).filter(Boolean))],
  };
}

/**
 * 汇总 → 判读。
 *
 * 三件事分开说，因为它们指向不同的修法：
 * - **键每次都不同** → 路由亲和性没建立（请求散落在不同缓存分片）；
 * - **前缀中途变过** → 那次之后的前缀必然未命中（改写，不是路由）；
 * - **前缀不变、键也稳，却仍然大量未命中** → 不是前缀也不是键，查保留期与分片。
 *
 * @param {object} summary summarizeCacheFacts 的结果
 * @returns {string[]} 判读行
 */
export function verdictLines(summary) {
  if (summary.calls === 0) {
    return [
      summary.source === 'codex'
        ? '这份日志里没有 token_usage_record（不是原生 Codex 的落盘日志？）'
        : '这份日志里没有带上游响应的模型调用（走的不是套餐路线？）',
    ];
  }
  const lines = [];
  if (summary.distinctKeys > 1) {
    lines.push(
      `键每次都不同（${summary.distinctKeys}/${summary.calls} 个不同值）→ 路由亲和性没建立：` +
        '请求散落在不同缓存分片，同一段前缀只有落到还留着它的那台机器才命中。',
    );
  } else {
    lines.push(
      summary.source === 'codex'
        ? '键（Codex 会话 id）在整场会话里稳定 → 亲和性有锚（原生实现就是这么发的）。'
        : '键在整场会话里稳定 → 路由亲和性已建立。',
    );
  }
  if (!summary.prefixKnown) {
    lines.push('前缀变没变在这份日志里量不了（原生落盘日志不存请求体），只能看命中率。');
  } else if (summary.distinctInstructions > 1 || summary.distinctTools > 1) {
    lines.push(
      `前缀在会话中途变过（instructions ${summary.distinctInstructions} 种、` +
        `tools ${summary.distinctTools} 种）→ 变化之后的前缀必然未命中。`,
    );
  } else {
    lines.push('前缀逐字不变（instructions / tools 指纹全程唯一）→ 丢的不是缓存内容。');
  }
  lines.push(
    `命中 ${summary.hitCalls} 次 / 未命中 ${summary.missCalls} 次，` +
      `合计 ${summary.totalCached}/${summary.totalInput} token = ${(summary.ratio * 100).toFixed(1)}%` +
      (summary.retention.length > 0 ? `，上游保留期 ${summary.retention.join('/')}` : ''),
  );
  return lines;
}

/**
 * 事实 + 汇总 → 人读的报告。
 *
 * @param {object[]} facts parseCacheFacts 的结果
 * @param {object} summary summarizeCacheFacts 的结果
 */
export function renderReport(facts, summary) {
  const rows = facts.map((fact) => {
    const ratio = fact.inputTokens > 0 ? Math.round((100 * fact.cachedTokens) / fact.inputTokens) : 0;
    return [
      String(fact.seq ?? '').padStart(5),
      `${fact.turn ?? '?'}/${fact.step ?? '?'}`.padEnd(7),
      String(fact.inputTokens ?? 0).padStart(9),
      String(fact.cachedTokens ?? 0).padStart(9),
      `${ratio}%`.padStart(5),
      String(fact.key ?? '-').slice(0, 8).padEnd(9),
      `${fact.instructionsTokens ?? '-'}/${fact.instructionsCached ?? '-'}`.padEnd(9),
      `${fact.toolsTokens ?? '-'}/${fact.toolsCached ?? '-'}`.padEnd(9),
      String(fact.instructionsHash ?? '-'),
      String(fact.toolsHash ?? '-'),
    ].join(' ');
  });
  const header = [
    '   seq', 'turn/stp', '  in_tok', '  cached', ' ratio', '   key',
    'instr', 'tools', 'instr#', 'tools#',
  ].join(' ');
  return [
    `来源：${summary.source === 'codex' ? '原生 Codex 落盘日志' : '内核会话日志'}`,
    header,
    '─'.repeat(header.length),
    ...rows,
    '',
    ...verdictLines(summary),
  ].join('\n');
}

/**
 * 目录 → 其中最新的会话日志；文件 → 原样返回。
 *
 * 两种形状都收：内核的 `*.jsonl.zstd`（zstd 压缩）与原生 Codex 的
 * `*.jsonl`（明文 rollout）。收在一处是因为它们回答同一个问题
 * （命中率多少），要的是同一把尺子。
 */
export function resolveSessionLog(inputPath) {
  const target = resolve(inputPath);
  if (!existsSync(target)) return undefined;
  if (!statSync(target).isDirectory()) return target;
  const candidates = [];
  const walk = (dir) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const full = join(dir, entry.name);
      if (entry.isDirectory()) walk(full);
      else if (entry.name.endsWith('.jsonl.zstd') || entry.name.endsWith('.jsonl')) {
        candidates.push({ path: full, mtime: statSync(full).mtimeMs });
      }
    }
  };
  walk(target);
  if (candidates.length === 0) return undefined;
  candidates.sort((a, b) => b.mtime - a.mtime);
  return candidates[0].path;
}

const invokedPath = process.argv[1] && pathToFileURL(resolve(process.argv[1])).href;
if (invokedPath === import.meta.url) {
  const input = process.argv[2];
  if (!input) {
    console.error('用法：node scripts/analyze-prompt-cache.mjs <会话日志 .zstd | 目录>');
    process.exit(1);
  }
  const logPath = resolveSessionLog(input);
  if (logPath === undefined) {
    console.error(`找不到会话日志：${input}`);
    process.exit(1);
  }
  if (typeof zlib.zstdDecompressSync !== 'function' && logPath.endsWith('.zstd')) {
    console.error('当前 Node 没有 zstd 支持（需要 22.15+）：换 Node 或手工解压后再跑。');
    process.exit(1);
  }
  const text = logPath.endsWith('.zstd')
    ? zlib.zstdDecompressSync(readFileSync(logPath)).toString('utf8')
    : readFileSync(logPath, 'utf8');
  const facts = parseFacts(text);
  const summary = summarizeCacheFacts(facts);
  console.log(`会话日志：${logPath}`);
  console.log(renderReport(facts, summary));
  if (summary.calls === 0) process.exit(1);
}
