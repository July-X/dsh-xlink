// 序号分配器的判据（协议 v0.2 §2 S-1「id 分配权归 host」）。
//
// ## 判什么
//
// · **序号由 host 分配并持久化**——客户端自造的序号一律不认；
// · **跨进程单调**：两个「进程」（两个分配器实例，同一个实例根）分配出来的
//   序号**不撞**，且合起来单调；
// · 进程重启后（= 新建一个分配器实例）**接着上次往下发**，不回到 1。
//
// ## 为什么必须专门测「两个实例」
//
// 「读计数器 → 加一 → 写回」在**单进程**里看着是对的；只有**两个实例同时
// 分配**才会暴露：两边都读到 `next=5`，都写回 `next=6` ⇒ 5 被发了两遍 ⇒
// 两份快照抢同一个 `snapshotId` ⇒ host 按「同 id 不重写」拒 ⇒ 用户看到
// 「这次选择没生效」。而 dev 壳与 release 壳**可以同时开着**，这不是理论风险。
//
// ## 隔离
//
// 全程用**临时目录**当实例根，不碰用户真实数据目录；用完删掉。

import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { existsSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { promisify } from 'node:util';

import { makeSessionShort } from '../host/protocol.js';
import { MAX_SEQ, createSeqAllocator } from '../host/seq-store.js';

const execFileAsync = promisify(execFile);

/** 造一个临时实例根，返回 `{root, cleanup}`。 */
function tempRoot() {
  const dir = mkdtempSync(join(tmpdir(), 'dsh-official-chat-seq-'));
  return { root: dir, cleanup: () => rmSync(dir, { recursive: true, force: true }) };
}

test('序号由 host 分配：第一个是 0001，形态符合协议 S-1', () => {
  const { root, cleanup } = tempRoot();
  try {
    const allocator = createSeqAllocator({ root, providerId: 'ds' });
    const first = allocator.reserve('sess-alpha-001');
    assert.equal(first.seq, 1);
    assert.match(first.snapshotId, /^ds~[a-z0-9]{12}~\d{4}$/, 'id 形态 = <provider>~<sessionShort>~<seq>');
    assert.ok(first.snapshotId.length <= 24, '整串 ≤ 24 可见字符（chip 宽度）');
    // 会话段必须就是该官网会话 id 的稳定短码（协议 §6 的首版算法）。
    assert.ok(first.snapshotId.includes(makeSessionShort('sess-alpha-001')));
  } finally {
    cleanup();
  }
});

test('跨进程单调：两个分配器实例（= 两个进程）分配出来的序号不撞', () => {
  const { root, cleanup } = tempRoot();
  try {
    const a = createSeqAllocator({ root, providerId: 'ds' });
    const b = createSeqAllocator({ root, providerId: 'ds' });
    const fromA = a.reserve('sess-alpha-001');
    const fromB = b.reserve('sess-alpha-001');
    assert.notEqual(fromA.seq, fromB.seq, '两个进程不许拿到同一个序号');
    // 合起来单调：两份序号构成一个递增序列。
    const sorted = [fromA.seq, fromB.seq].sort((x, y) => x - y);
    assert.deepEqual(sorted, [1, 2]);
    // 两边都认这两个序号（认领是**共享**的事实，不是各自内存里的计数）。
    const short = makeSessionShort('sess-alpha-001');
    assert.equal(a.isClaimed(short, fromA.seq), true);
    assert.equal(a.isClaimed(short, fromB.seq), true, 'B 认领的序号 A 也要认');
    assert.equal(b.isClaimed(short, fromA.seq), true, 'A 认领的序号 B 也要认');
  } finally {
    cleanup();
  }
});

test('进程重启后接着上次往下发（不回到 1）', () => {
  const { root, cleanup } = tempRoot();
  try {
    const before = createSeqAllocator({ root, providerId: 'ds' });
    before.reserve('sess-alpha-001');
    before.reserve('sess-alpha-001');
    // 「重启」= 全新实例（内存里什么都没有）。
    const after = createSeqAllocator({ root, providerId: 'ds' });
    const next = after.reserve('sess-alpha-001');
    assert.equal(next.seq, 3, '重启后必须接着已认领的最大值往下发');
  } finally {
    cleanup();
  }
});

test('客户端自造的序号不认（isClaimed 只认 host 认领过的）', () => {
  const { root, cleanup } = tempRoot();
  try {
    const allocator = createSeqAllocator({ root, providerId: 'ds' });
    const claimed = allocator.reserve('sess-alpha-001');
    const short = makeSessionShort('sess-alpha-001');
    assert.equal(allocator.isClaimed(short, claimed.seq), true);
    assert.equal(allocator.isClaimed(short, claimed.seq + 1), false, '没认领过的不算');
    assert.equal(allocator.isClaimed(short, 9999), false);
    assert.equal(allocator.isClaimed(makeSessionShort('别的会话'), claimed.seq), false);
  } finally {
    cleanup();
  }
});

test('usedSeqs 报出每个会话键已认领的最大序号', () => {
  const { root, cleanup } = tempRoot();
  try {
    const allocator = createSeqAllocator({ root, providerId: 'ds' });
    allocator.reserve('sess-alpha-001');
    allocator.reserve('sess-alpha-001');
    allocator.reserve('sess-beta-002');
    const used = allocator.usedSeqs();
    const alpha = `ds~${makeSessionShort('sess-alpha-001')}`;
    const beta = `ds~${makeSessionShort('sess-beta-002')}`;
    assert.equal(used[alpha], 2);
    assert.equal(used[beta], 1);
  } finally {
    cleanup();
  }
});

test('实例根不可用（没有 DSH_HOME）→ 如实报不可用，不猜默认位置', () => {
  const allocator = createSeqAllocator({ root: null, providerId: 'ds' });
  assert.equal(allocator.available, false);
  assert.throws(
    () => allocator.reserve('sess-alpha-001'),
    (error) => error.code === 'snapshot_store_unavailable',
    '不可用时要带稳定码，不是抛一个说不清的异常',
  );
});

test('序号认领落在实例根下（不碰用户真实目录）', () => {
  const { root, cleanup } = tempRoot();
  try {
    const allocator = createSeqAllocator({ root, providerId: 'ds' });
    allocator.reserve('sess-alpha-001');
    const short = makeSessionShort('sess-alpha-001');
    const claim = join(root, 'seq-claims', 'ds', short, '0001.json');
    assert.ok(existsSync(claim), `认领文件应当落在这里：${claim}`);
    assert.ok(!claim.includes('.dsh-xlink'), '不许落到用户真实数据目录');
  } finally {
    cleanup();
  }
});

test('序号上限就是协议的 4 位（MAX_SEQ = 9999）', () => {
  assert.equal(MAX_SEQ, 9999, '协议 S-1：seq 是 4 位，`0001` 起');
});

test('跨进程（真进程）并发分配：不撞（协议 v0.2 S-1「跨进程单调」）', async () => {
  // **这一条必须是真进程**：同一进程里两段分配是**串行**的（Node 单线程），
  // 串行下「读目录 → 跳过已认领」就够用了，撞车只可能发生在**两个进程同时
  // 分配**时——而 dev 壳与 release 壳**可以同时开着**。
  //
  // 子进程各自 `import` 同一个 `seq-store.js`，共享同一个实例根（= 同一个实例
  // 的 home），同时抢同一个会话的序号。
  const { root, cleanup } = tempRoot();
  try {
    const moduleUrl = new URL('../host/seq-store.js', import.meta.url).href;
    const worker = `
      const { createSeqAllocator } = await import(process.argv[1]);
      const allocator = createSeqAllocator({ root: process.argv[2], providerId: 'ds' });
      const reserved = allocator.reserve('sess-alpha-001');
      process.stdout.write(String(reserved.seq));
    `;
    const workers = await Promise.all(
      Array.from({ length: 4 }, () =>
        execFileAsync(process.execPath, ['--input-type=module', '-e', worker, moduleUrl, root]),
      ),
    );
    const seqs = workers.map((result) => Number(result.stdout));
    assert.deepEqual(
      [...new Set(seqs)].sort((a, b) => a - b),
      [1, 2, 3, 4],
      `四个进程必须拿到四个不同的序号：${JSON.stringify(seqs)}`,
    );
  } finally {
    cleanup();
  }
});
