// 常用模型偏好的**变更通知**回归。
//
// 用户勾完模型，会话里的选择器必须按新列表列。内核 webui 的模型目录是按
// 「Host 代次」缓存的（只在 `llm/adapters-updated` 等事件上重拉），而壳写
// 偏好文件时没法直接通知内核——所以 Host 盯着那只文件，stat 一变就 `replace`
// 一次适配器注册，把这枚事件广播出去。
//
// 两条用例各管一半：① 通知本身（含停机后不再通知）；② **接线**——`apply()`
// 真的用了壳传进来的 env 名。只验 ① 的话，把 env 名改错（症状是「勾完要手动
// 刷新页面」）照样全绿。
//
// **两条用例共用同一个等待策略，这不是巧合**：观察走 `fs.watchFile` 的 stat 轮询，
// 回调要等事件循环腾出时间片才可能跑；满负载门禁里本进程被饿住是实测过的。哪条
// 用例只写一次就干等短上限，哪条就还在把成败押在「事件循环什么时候醒」上。详见
// `WATCH_TIMEOUT_MS` 与 `waitForNotify`。
import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { pathToFileURL } from 'node:url';
import {
  SELECTION_FILE_ENV,
  SELECTION_WATCH_DEBOUNCE_MS,
  SELECTION_WATCH_INTERVAL_MS,
} from '../host/constants.js';
import { stagePlugin } from './fixtures.mjs';

/** 桩 peer 的暂存副本：`host/index.js` 静态 import `@deepseek-ai/dsh-llm`，
 *  直接 import 仓库里的那份会 ERR_MODULE_NOT_FOUND（peer 由内核提供）。 */
async function stagedHost(scratch) {
  const dir = stagePlugin(scratch);
  return import(pathToFileURL(join(dir, 'host', 'index.js')));
}

/** 等一次变更通知的上限：2s 轮询 × 15 = 30s。**两条用例共用。**
 *
 *  这是**等待上限**，与传给 `watchSelectionFile` 的 `interval` 不是一回事：
 *  `interval` 是 stat 轮询的节拍（单测压到毫秒级），上限管的是「最多等多久」。
 *
 *  为什么放大到 30s：满负载门禁下本进程的事件循环会被饿住，stat 轮询与本文件
 *  自己的 `delay` 一起被推迟。本文件为此返工过两轮——6s 打满过一次，放大到 30s
 *  后**又一次**耗光。而第一条用例当时还留着 3s 的旧上限没跟着改，2026-10-18 在 CI
 *  上**又一次**打满（3026ms；同一份代码本地单跑只要 887ms，而紧邻的接线用例在
 *  CI 上仍是 2428ms 绿——只有第一个用例卡在上限里）。
 *
 *  上限只影响**失败路径**的长度：观察真的坏了时再等多久都不会有通知，判据不会
 *  因此变松。真正的免疫手段是 `waitForNotify` 的持续写入。
 */
const WATCH_TIMEOUT_MS = SELECTION_WATCH_INTERVAL_MS * 15;

/**
 * 等到 `check()` 成立为止；等不到就**一边等一边继续写**。
 *
 *  持续写入是唯一有效的免疫手段：stat 轮询被推迟多久都不要紧，事件循环一拿到
 *  时间片，下一次轮询看到的就是刚刚写进去的状态。观察真的没装上、env 名或路径
 *  真的接错时，写多少次都不会有通知——判据仍然是 `check()` 那一刻的读数。
 *
 * @param check 判据：读通知次数那一类副作用（`replaces`）
 * @param write  等待期间反复落盘的动作；保持同一份内容即可（mtime 每次都在动）
 */
async function waitForNotify(check, write) {
  const deadline = Date.now() + WATCH_TIMEOUT_MS;
  while (!check() && Date.now() < deadline) {
    await delay(400);
    if (!check()) write();
  }
  return check();
}

/** 让上一拍的合并窗口彻底落定，再读计数。
 *
 *  合并窗口（150ms）里排队的 `replace` 会在**这条等待返回之后**才落到计数上。
 *  不等它落定就取基准，第二段断言就可能数到上一次变更留下的尾巴，从而在「第二次
 *  变更根本没被看见」时也判过。
 */
async function settleDebounce() {
  await delay(SELECTION_WATCH_DEBOUNCE_MS + 80);
}

test('变更通知：偏好文件一改就 replace 一次，停机后不再通知', async () => {
  const scratch = mkdtempSync(join(tmpdir(), 'oauth-watch-'));
  const { watchSelectionFile } = await stagedHost(scratch);
  const file = join(scratch, 'model-selection.json');
  const warnings = [];
  let replaces = 0;
  const stop = watchSelectionFile(
    { replace: () => { replaces += 1; } },
    file,
    (message) => warnings.push(message),
    25,
  );
  assert.equal(typeof stop, 'function');
  try {
    const first = '{"selected":["gpt-a"]}';
    writeFileSync(file, first);
    assert.ok(
      await waitForNotify(() => replaces > 0, () => writeFileSync(file, first)),
      '写入偏好文件后必须通知一次',
    );
    await settleDebounce();
    const settled = replaces;
    // 再写一次（原子写的 stat 可能连跳两拍）：节流窗口把同一拍合并成一次。
    const second = '{"selected":["gpt-a","gpt-b"]}';
    writeFileSync(file, second);
    assert.ok(
      await waitForNotify(() => replaces > settled, () => writeFileSync(file, second)),
      '第二次变更也必须通知',
    );
    await settleDebounce();
    stop();
    const afterStop = replaces;
    writeFileSync(file, '{"selected":["gpt-c"]}');
    await delay(200);
    assert.equal(replaces, afterStop, '停机后不该再有通知');
    assert.deepEqual(warnings, []);
  } finally {
    stop();
    rmSync(scratch, { recursive: true, force: true });
  }
});

test('接线：apply 读壳传进来的 env 名并观察那只文件', async () => {
  const scratch = mkdtempSync(join(tmpdir(), 'oauth-apply-'));
  const pluginDir = stagePlugin(scratch);
  const file = join(scratch, 'model-selection.json');
  let replaces = 0;
  const ctx = {
    fiber: { entry: { options: { id: 'xlink-openai-oauth', version: '0.0.0-test' } } },
    llm: { registerAdapter: () => ({ replace: () => { replaces += 1; } }) },
    logger: { warn: () => {}, info: () => {} },
    on: () => {},
  };
  const previous = process.env[SELECTION_FILE_ENV];
  process.env[SELECTION_FILE_ENV] = file;
  try {
    const { apply } = await import(pathToFileURL(join(pluginDir, 'host', 'index.js')));
    await apply(ctx, {});
    const body = '{"selected":["gpt-a"]}';
    writeFileSync(file, body);
    assert.ok(
      await waitForNotify(() => replaces > 0, () => writeFileSync(file, body)),
      'apply 必须在 env 指出的路径上起观察（env 名或路径接错时这里会超时）',
    );
  } finally {
    if (previous === undefined) delete process.env[SELECTION_FILE_ENV];
    else process.env[SELECTION_FILE_ENV] = previous;
    rmSync(scratch, { recursive: true, force: true });
  }
});
