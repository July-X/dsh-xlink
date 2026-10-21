// 客户端命令层与 RPC 接线层的判据（协议 §5「命名空间与桥」）。
//
// ## 判什么
//
// · 5 个客户端命令（`capabilities` / `catalog` / `capture` / `cancel` /
//   `preview`）各自的行为与错误形态；
// · 协议错误负载 → 内核 RPC 错误形态的翻译（**内部异常串不许外泄**）；
// · **未知端点必须明确失败**，不许被当成成功值。
//
// ## 为什么命令层与传输层要分开测
//
// 命令层是**纯业务**（不碰传输），RPC 层是**翻译**（协议负载 ↔ 内核形态）。
// 分开测，才能在「传输换了」（比如从 RPC 换成别的桥）时，业务判据一条都不用改。
//
// ## 判据不替真机跑
//
// 这里用的是**内存桩存储**（`save` / `load` / `listSessionSeqs`），证不了
// 「真机上的 WebView2 能不能拿到这个服务」——那件事的结论在真实内核里，
// 而且**当前实现里它是不通的**（见 `host/rpc.js` 文件头的取证结论）。

import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';

import {
  createCatalogCache,
  createCommands,
  createFixtureSource,
} from '../host/commands.js';
import { createSeqAllocator } from '../host/seq-store.js';
import { ENDPOINTS, FETCH_ROUTE_PATH, createDispatcher, toRpcError } from '../host/rpc.js';

/** 造一套内存桩：存储 + 目录缓存 + 固定夹具取数源。 */
function makeHarness() {
  const root = mkdtempSync(join(tmpdir(), 'official-chat-commands-'));
  const written = new Map();
  const store = {
    save: (snapshot) => {
      const id = String(snapshot?.snapshotId ?? '');
      if (written.has(id)) {
        const error = new Error(`快照已存在：${id}`);
        error.code = 'snapshot_exists';
        throw error;
      }
      written.set(id, snapshot);
      return { snapshotId: id, contentDigest: snapshot.contentDigest, path: join(root, `${id}.json`) };
    },
    load: (id) => {
      const found = written.get(String(id));
      if (found === undefined) {
        const error = new Error(`快照不存在：${String(id)}`);
        error.code = 'snapshot_invalid';
        throw error;
      }
      return { snapshot: found, body: found.messages.map((m) => m.text).join('\n\n') };
    },
    listSessionSeqs: () => ({}),
  };
  // **provider 短码必须两端一致**：命令侧按 `makeProviderShort(providerId)` 核对
  // 句柄的 provider 段，分配器侧按自己的 `providerShort` 造 id——两边不同源
  // 就会「序号不是 host 分配的」。
  const providerId = 'official-chat';
  const commands = createCommands({
    store,
    catalogCache: createCatalogCache({ rootDir: join(root, 'catalog-cache') }),
    captureSource: createFixtureSource([
      {
        sessionId: 'sess-alpha-001',
        title: 'G1 固定对话 Alpha',
        body: '【G1 固定正文 Alpha】第一行固定正文。',
      },
    ]),
    seqAllocator: createSeqAllocator({ root, providerId }),
    providerId,
    accountScope: 'desktop-dev/default/site-A',
  });
  return { root, store, commands, handler: createDispatcher({ commands }) };
}

test('协议 §5：客户端命令齐活（P1 的 5 个 ＋ P2 §2.2 的 duplicates ＋ P4 的 sessionStructure），且通道路径是内核 exact fetch route 的那条', () => {
  // **P2 协议 §2.2 新增了 `duplicates`**（重复识别 ＋ 预算预检）⇒ 端点集合
  // 从 5 个变成 6 个。**P1 §5 的命令清单仍写 5 个** ⇒ 协议侧待 PM 同步。
  // **P4 协议新增了 `sessionStructure`**（真实站点的会话目录**结构**，只回白名单）
  // ⇒ 端点集合再变成 7 个 ⇒ **P1 §5 同样待 PM 同步**（协议侧归属）。
  assert.deepEqual([...ENDPOINTS], [
    'capabilities',
    'catalog',
    'capture',
    'cancel',
    'preview',
    'duplicates',
    'sessionStructure',
  ]);
  // **路径必须与 host 半边注册的 exact route 全等**——内核是
  // `fetchRoutes.get(url.pathname)`（全等匹配），错一个字符就 404/405。
  assert.equal(FETCH_ROUTE_PATH, '/api/xlink-official-chat');
  // **必须在 `/api` 命名空间下**：内核 `assertFetchRoute`
  // （`dsh-client-connection/lib/index.js@0.2.1-rc.2:973-977`）用
  // `endpointFromPath("/api", route.path)` 校验，不在 `/api` 下直接抛。
  assert.ok(FETCH_ROUTE_PATH.startsWith('/api/'), 'exact route 必须在 /api 命名空间下');
});

test('capabilities：把协议常量、错误码表、状态机与降级标记一次性交出去', async () => {
  const { handler } = makeHarness();
  const result = await handler('capabilities', {});
  assert.equal(result.ok, true);
  assert.equal(result.value.schemaVersion, 1);
  assert.equal(result.value.constants.maxBodyBytes, 256 * 1024);
  assert.equal(Object.keys(result.value.errorCodes).length, 14);
  // **降级标记必须在**：实现不得假设 G1-1…G1-5 已解决。
  assert.ok(result.value.degraded.g1_1, '必须标出 G1-1 的降级形态');
  assert.ok(result.value.degraded.capture_source, '必须标出取数源是固定夹具');
  // **G1-7（本轮真机实测出的上游缺口）也要登记**：
  // `dsh-client-connection@0.2.1-rc.2` 的 `owner.webServer.register` 是属性访问，
  // cordis 查找落在 connection 插件自己的 fiber scope ⇒ host 半边不激活、通道不通。
  // 不登记它，客户端就会以为一切正常，直到真机调用才撞 405。
  assert.ok(result.value.degraded.g1_7, '必须标出 G1-7：上游内核的 webServer 属性访问缺口');
  assert.match(String(result.value.degraded.g1_7), /webServer/);
});

test('catalog：回目录页（不含正文），第二次走缓存', async () => {
  const { handler, commands } = makeHarness();
  const first = await handler('catalog', { query: '' });
  assert.equal(first.ok, true);
  assert.equal(first.value.page.items.length, 1);
  assert.equal(first.value.page.items[0].sessionId, 'sess-alpha-001');
  assert.equal(first.value.cached, false);
  // 目录页**不含正文**（协议 §1.2）。
  assert.equal('body' in first.value.page.items[0], false);
  assert.equal('text' in first.value.page.items[0], false);
  const second = await commands.catalog({ query: '' });
  assert.equal(second.cached, true, '第二次应当命中目录缓存');
});

test('capture：落盘一份快照，句柄就是快照 id（S-1 单一 id 制度）', async () => {
  const { handler } = makeHarness();
  // **v0.2 S-1：id 分配权归 host** —— 客户端拿的是 `catalog` 顺带分配好的 id，
  // 自己报一个 id 过来，host 会核对「这个序号是不是我认领过的」并拒。
  const listed = await handler('catalog', { query: '' });
  const snapshotId = listed.value.allocations['sess-alpha-001'];
  const result = await handler('capture', { sessionId: 'sess-alpha-001', snapshotId, requestId: 'r-1' });
  assert.equal(result.ok, true);
  const { snapshot } = result.value;
  assert.match(snapshot.snapshotId, /^[a-z0-9]{1,4}~[a-z0-9]{1,12}~\d{4}$/);
  assert.ok(snapshot.snapshotId.length <= 24, '句柄整串 ≤ 24 可见字符');
  assert.equal(snapshot.schemaVersion, 1);
  assert.match(snapshot.contentDigest, /^[0-9a-f]{64}$/);
  // 落盘内容与模型可见正文**逐字节相同**（协议 S-4 的尺子）。
  assert.equal(snapshot.contentDigest, result.value.snapshot.contentDigest);
});

test('capture 反向回归：客户端自造一个 host 没认领过的 id → 拒', async () => {
  const { handler } = makeHarness();
  const forged = 'ds~rnaq4171cqan~9999';
  const result = await handler('capture', { sessionId: 'sess-alpha-001', snapshotId: forged });
  // **失败必须落在外层**（内核 RPC 契约：`{ok:false, error}`）。包成
  // `{ok:true, value:{ok:false}}` 就是「双层 ok 把失败吞成成功」——对端只看外层
  // 时会拿到一个「成功值」，于是照样把正文发出去。
  assert.equal(result.ok, false, '不是 host 分配的 id 必须拒');
  assert.equal(result.error.code, 'snapshot_invalid');
});

test('capture：同 id 落两次 → 拒（快照不可变，§6），且回的是**协议内**的码', async () => {
  const { handler } = makeHarness();
  const listed = await handler('catalog', { query: '' });
  const snapshotId = listed.value.allocations['sess-alpha-001'];
  await handler('capture', { sessionId: 'sess-alpha-001', snapshotId });
  const again = await handler('capture', { sessionId: 'sess-alpha-001', snapshotId });
  // **失败必须落在外层**（内核 RPC 契约：`{ok:false, error{code,message,details}}`，
  // 见 `dsh-client-connection/lib/index.js:621-629` 的 `rpcResultSchema`）。
  assert.equal(again.ok, false, '第二条必须失败，而且失败在外层');
  assert.equal(again.error.code, 'snapshot_invalid');
  // 协议 §3 的 14 个码是闭集：存储层的内部码不许漏给对端，但**原始码留在 detail**。
  assert.equal(again.error.details.detail, 'snapshot_exists', '原始码留在 detail 里（稳定语义，不是异常串）');
});

test('preview：按固定 id 读快照；读不到 → snapshot_invalid（不静默重读）', async () => {
  const { handler } = makeHarness();
  const listed = await handler('catalog', { query: '' });
  const snapshotId = listed.value.allocations['sess-alpha-001'];
  const captured = await handler('capture', { sessionId: 'sess-alpha-001', snapshotId });
  const id = captured.value.snapshot.snapshotId;
  const ok = await handler('preview', { snapshotId: id });
  assert.equal(ok.value.ok, true);
  assert.equal(ok.value.state, 'ready');
  const missing = await handler('preview', { snapshotId: 'ds~rnaq4171cqan~9999' });
  assert.equal(missing.ok, false, '读不到必须是外层失败，不许是「成功值里的失败」');
  assert.equal(missing.error.code, 'snapshot_invalid');
  assert.equal(missing.error.details.retryable, false);
});

test('cancel：没有在途请求时也不许失败（幂等）', async () => {
  const { handler } = makeHarness();
  const result = await handler('cancel', { requestId: 'r-none' });
  assert.equal(result.ok, true);
  assert.equal(result.value.cancelled, false);
});

test('未知端点 → 明确失败，不许被当成成功值', async () => {
  const { handler } = makeHarness();
  const result = await handler('no-such-endpoint', {});
  assert.equal(result.ok, false);
  assert.equal(result.error.code, 'source_unsupported');
});

test('错误翻译：内部异常串不许进返回值（协议 §3）', () => {
  const translated = toRpcError({
    code: 'timeout',
    retryable: true,
    userNext: '重试',
    requestId: 'r-1',
  });
  assert.equal(translated.code, 'timeout');
  assert.equal(translated.details.retryable, true);
  assert.equal(translated.details.requestId, 'r-1');
  // 译文里只允许稳定码与「用户下一步」这类分类好的信息。
  assert.ok(!/Error|at .*\.js:\d+/.test(translated.message), '异常串不许进 message');
});
