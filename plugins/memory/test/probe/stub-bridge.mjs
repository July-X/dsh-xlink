// 桩桥：记下插件发来的每一条请求（这就是我们要观察的那条链），按路由回合法 JSON。
//
// ## 它替代的是谁
//
// 真桥在壳的 Rust 进程里（`src-tauri/src/plugins/builtin/bridge.rs`），跑真桥要起
// `cargo test --lib manual_bridge_for_the_node_probe`。桩桥的价值是**零构建、零额度**，
// 适合「先看这条链长什么样」的场合；**要验业务闸就必须用真桥**——桩这里一个闸都没有。
//
// ## 环境变量
//
// - `PROBE_HOME`（默认 `/tmp/p2probe`）——只往这里写。
// - `PROBE_BRIDGE_PORT`（默认 8792）——桩桥的端口。
// - `PROBE_BRIDGE_TOKEN`（默认 `probe-token`）——**必须与驱动器传进来的那个一致**。
//
// ## 两条踩过的坑（贴在代码旁边，改之前先读）
//
// 1. **忘了定义 `params` 会让进程崩**：桩里 `req.url.searchParams` 取参数，而第一版在第一个
//    jobs 请求上就 `params.get(...)` —— 不，那次崩的原因是**桩自己收到未定义的变量**。崩了之后
//    **所有请求静默消失**（端口没人听），插件那边只会看到「什么都没发生」，而桥日志停在最后
//    一条。**排障先看这份日志的最后一行**，不要先怀疑插件。
// 2. **桩自己发明的形状会让插件静默不动**：`/jobs/claim` 的真形状是
//    `{token, key:{session,cursor,extractor}, attempts, expires_at_ms}`（`jobs.rs` 的
//    `Claimed`）。第一版让桩回了个扁平形状，插件按错字段读 → **什么都不做且没有报错**。
//    照抄真形状，不要发明。
//
// 还有一条更要命的（它让 G12 的结论假绿过一轮）：
//
// 3. **`/context` 必须回真命中**：回 `hits: []` 时插件永远不「应用」任何东西，`injections`
//    一直是 0，于是每回合都走 refresh 路径——**任何依赖注入预算的 A/B 都会得到假绿**。
//    下面那个 `hits` 里那一条是故意的。
import { createServer } from 'node:http';
import { appendFileSync, mkdirSync } from 'node:fs';
import { join } from 'node:path';

const PROBE_HOME = process.env.PROBE_HOME ?? '/tmp/p2probe';
const PORT = Number(process.env.PROBE_BRIDGE_PORT ?? 8792);
const TOKEN = process.env.PROBE_BRIDGE_TOKEN ?? 'probe-token';

mkdirSync(PROBE_HOME, { recursive: true });
const log = (line) => appendFileSync(join(PROBE_HOME, 'bridge.log'), `${line}\n`);

/** 桩的任务表：一个 pending + 一个已领出的租约，够插件走完一轮提取。 */
let pending = null;
let claimed = null;

const reply = (res, status, payload) => {
  const body = JSON.stringify(payload);
  res.writeHead(status, {
    'content-type': 'application/json',
    'content-length': Buffer.byteLength(body),
  });
  res.end(body);
};

createServer((req, res) => {
  log(`REQ ${req.method} ${req.url} auth=${req.headers.authorization === `Bearer ${TOKEN}`}`);
  if (req.headers.authorization !== `Bearer ${TOKEN}`) {
    return reply(res, 401, { error: 'bad token' });
  }
  const url = new URL(req.url, 'http://127.0.0.1');
  const path = url.pathname;
  const params = url.searchParams;

  if (path === '/v1/memory/jobs/enqueue') {
    pending = {
      session: params.get('session') ?? '',
      cursor: Number(params.get('cursor') ?? 0),
      extractor: params.get('extractor') ?? '',
    };
    return reply(res, 200, { outcome: 'added' });
  }
  if (path === '/v1/memory/jobs/claim') {
    if (!pending) return reply(res, 200, { token: null });
    const job = pending;
    pending = null;
    claimed = { token: 'lease-1', key: job, attempts: 1, expires_at_ms: Date.now() + 120000 };
    return reply(res, 200, claimed);
  }
  if (path === '/v1/memory/jobs/renew') return reply(res, 200, { ok: true });
  if (path === '/v1/memory/jobs/submit') {
    claimed = null;
    return reply(res, 200, { submitted: true, id: `mem-${params.get('title') ?? 'x'}` });
  }
  if (path === '/v1/memory/jobs/finish') {
    claimed = null;
    return reply(res, 200, { finished: true });
  }
  if (path === '/v1/memory/jobs/fail') {
    claimed = null;
    return reply(res, 200, { retryable: false });
  }
  if (path === '/v1/memory/context') {
    const query = decodeURIComponent(params.get('q') ?? '');
    return reply(res, 200, {
      enabled: true,
      // 见文件头第 3 条：**必须有真命中**，否则 injections 恒为 0，A/B 假绿。
      hits: [
        {
          id: 'mem-stub-1',
          title: '提交前跑检查',
          body: '提交前先跑 npm run check',
          keywords: [],
          scope: 'general',
          pinned: false,
          verification: 'verified',
          use_count: 0,
          updated_at: '',
          disabled: false,
          kind: 'user-manual',
        },
      ],
      text: `[记忆] 提交前跑检查：提交前先跑 npm run check（查询：${query}）`,
      // C6 起这条路**只有提取那一次**会带候选；桩这里回空数组（形状要与真桥一致，
      // 否则插件读不到那个字段时是 `undefined` 而不是 `[]`——那种区别在桩上验不出来，
      // 但会让「插件有没有读它」这件事在真机上分不清）。
      pending: [],
    });
  }
  if (path === '/v1/memory/note') return reply(res, 200, { ok: true });
  if (path === '/v1/memory/read') return reply(res, 404, { error: 'stub has no such memory' });
  if (path === '/v1/memory/propose') return reply(res, 200, { id: 'cand-1' });
  return reply(res, 404, { error: `stub bridge has no route for ${path}` });
}).listen(PORT, '127.0.0.1', () =>
  console.log(`[stub-bridge] 127.0.0.1:${PORT}，日志 ${PROBE_HOME}/bridge.log`)
);
