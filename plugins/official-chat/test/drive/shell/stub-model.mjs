// 最小 OpenAI 形状的桩模型（壳内复验用）。
//
// ## 它要提供什么
//
// 让「读取未完成时按发送」这件事有一个**可判别的终点**：
//   · 请求到达了 → 内核没有漏发，而是等/拒；
//   · 请求没到达 → 发送被拦下了（草稿还在、也没有用户消息）。
//
// 不接任何真实厂商，不带任何凭据；回包是固定字符串。
//
// ## 证据纪律
//
//  1. **日志按轮次独立命名**（`model-requests.<round>.jsonl`），**不复用同名文件**：
//     2026-10-20 第 4 轮的空跑把上一轮的 `model.log` 覆盖成 0 字节，
//     「请求体含夹具正文」那条结论的原始依据就此不可复核。
//  2. **不截断旧日志**：启动时只新建本轮的文件。
//  3. 日志写在**会被删的临时 home 之外**，否则收尾清理会把证据一起带走。
import { createServer } from 'node:http';
import { appendFileSync, mkdirSync } from 'node:fs';
import { join } from 'node:path';

import { TEMP_ROOT } from './paths.mjs';

export const ROUND = process.env.G1_ROUND ?? `r${new Date().toISOString().replace(/[-:.TZ]/g, '').slice(0, 14)}`;
export const EVIDENCE_DIR = process.env.G1_EVIDENCE_DIR ?? join(TEMP_ROOT, `evidence-${ROUND}`);
export const LOG_FILE = join(EVIDENCE_DIR, `model-requests.${ROUND}.jsonl`);
export const STUB_PORT = Number(process.env.G1_STUB_PORT ?? 3223);
export const STUB_MODEL_ID = 'g1-stub-model';

const REPLY = 'G1 桩模型回包：收到了请求。';

/** 启动桩模型。返回 {port, requests(), stop}。 */
export async function startStubModel({ port = STUB_PORT, modelId = STUB_MODEL_ID } = {}) {
  const requests = [];
  mkdirSync(EVIDENCE_DIR, { recursive: true });

  const server = createServer((req, res) => {
    const chunks = [];
    req.on('data', (c) => chunks.push(c));
    req.on('end', () => {
      const raw = Buffer.concat(chunks).toString('utf8');
      let body = null;
      try { body = raw ? JSON.parse(raw) : null; } catch { /* 非 JSON 也记下来 */ }
      const entry = {
        at: new Date().toISOString(),
        method: req.method,
        url: req.url,
        body,
      };
      requests.push(entry);
      try {
        appendFileSync(
          LOG_FILE,
          `${JSON.stringify({ at: entry.at, method: entry.method, url: entry.url, body })}\n`,
          'utf8',
        );
      } catch { /* 日志失败不致命 */ }

      if (req.method === 'GET' && (req.url ?? '').endsWith('/models')) {
        const payload = {
          object: 'list',
          data: [{ id: modelId, object: 'model', created: 0, owned_by: 'g1-stub' }],
        };
        res.writeHead(200, { 'content-type': 'application/json' });
        res.end(JSON.stringify(payload));
        return;
      }

      if (req.method === 'POST' && (req.url ?? '').includes('/chat/completions')) {
        const stream = body?.stream === true;
        const created = 1700000000;
        if (!stream) {
          const payload = {
            id: 'chatcmpl-g1-stub',
            object: 'chat.completion',
            created,
            model: body?.model ?? modelId,
            choices: [
              {
                index: 0,
                message: { role: 'assistant', content: REPLY },
                finish_reason: 'stop',
              },
            ],
            usage: { prompt_tokens: 1, completion_tokens: 1, total_tokens: 2 },
          };
          res.writeHead(200, { 'content-type': 'application/json' });
          res.end(JSON.stringify(payload));
          return;
        }
        res.writeHead(200, {
          'content-type': 'text/event-stream',
          'cache-control': 'no-cache',
          connection: 'keep-alive',
        });
        const base = { id: 'chatcmpl-g1-stub', object: 'chat.completion.chunk', created, model: body?.model ?? modelId };
        const write = (delta, finish = null) => {
          res.write(`data: ${JSON.stringify({ ...base, choices: [{ index: 0, delta, finish_reason: finish }] })}\n\n`);
        };
        write({ role: 'assistant', content: '' });
        write({ content: REPLY });
        write({}, 'stop');
        res.write('data: [DONE]\n\n');
        res.end();
        return;
      }

      res.writeHead(404, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ error: { message: 'stub: no route' } }));
    });
  });

  await new Promise((resolveListen, rejectListen) => {
    server.once('error', rejectListen);
    server.listen(port, '127.0.0.1', resolveListen);
  });

  return {
    port,
    modelId,
    baseURL: `http://127.0.0.1:${port}/v1`,
    logFile: LOG_FILE,
    requests: () => requests.slice(),
    stop: () => { try { server.close(); } catch { /* ignore */ } },
  };
}
