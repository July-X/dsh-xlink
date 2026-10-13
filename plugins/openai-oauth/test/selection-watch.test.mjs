// 常用模型偏好的**变更通知**回归。
//
// 用户勾完模型，会话里的选择器必须按新列表列。内核 webui 的模型目录是按
// 「Host 代次」缓存的（只在 `llm/adapters-updated` 等事件上重拉），而壳写
// 偏好文件时没法直接通知内核——所以 Host 盯着那只文件，stat 一变就 `replace`
// 一次适配器注册，把这枚事件广播出去。
//
// 两条用例各管一半：① 通知本身（含节流合并、停机后不再通知）；② **接线**
// ——`apply()` 真的用了壳传进来的那个 env 名。只验 ① 的话，把 env 名改错
// （症状是「勾完要手动刷新页面」）照样全绿。
import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { pathToFileURL } from 'node:url';
import { SELECTION_FILE_ENV, SELECTION_WATCH_INTERVAL_MS } from '../host/constants.js';
import { stagePlugin } from './fixtures.mjs';

/** 桩 peer 的暂存副本：`host/index.js` 静态 import `@deepseek-ai/dsh-llm`，
 *  直接 import 仓库里的那份会 ERR_MODULE_NOT_FOUND（peer 由内核提供）。 */
async function stagedHost(scratch) {
  const dir = stagePlugin(scratch);
  return import(pathToFileURL(join(dir, 'host', 'index.js')));
}

/** 等到条件成立或超时（避免用固定 sleep 赌时序）。 */
async function until(check, timeoutMs = 3000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (check()) return true;
    await delay(20);
  }
  return check();
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
    writeFileSync(file, '{"selected":["gpt-a"]}');
    assert.ok(await until(() => replaces > 0), '写入偏好文件后必须通知一次');
    const settled = replaces;
    // 再写一次（原子写的 stat 可能连跳两拍）：节流窗口把同一拍合并成一次。
    writeFileSync(file, '{"selected":["gpt-a","gpt-b"]}');
    assert.ok(await until(() => replaces > settled), '第二次变更也必须通知');
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
    writeFileSync(file, '{"selected":["gpt-a"]}');
    assert.ok(
      await until(() => replaces > 0, SELECTION_WATCH_INTERVAL_MS * 3),
      'apply 必须在 env 指出的路径上起观察（env 名或路径接错时这里会超时）',
    );
  } finally {
    if (previous === undefined) delete process.env[SELECTION_FILE_ENV];
    else process.env[SELECTION_FILE_ENV] = previous;
    rmSync(scratch, { recursive: true, force: true });
  }
});
