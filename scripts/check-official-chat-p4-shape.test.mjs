// P4 §10 判据**第 8 条**（字段面）的**自测**。
//
// ## 判什么
//
// · **场景表必须自证**：A（正确形态）通过 · B（`code=0` 但 `biz_data=null`）失败 ·
//   C（`has_more` 缺失）失败 · D（条目键名少一个）失败 · E（读到 0 条）失败 ·
//   F（`biz_data` 多一个未知键）**仍通过** · G（实测派生结构）通过 · H（条目多一个键）失败。
// · **防假红的三条固化**：**不断绝对条数**（`len=1` 与 `len=100` 都过）／
//   **不断 `biz_data` 键集合相等**（站点加字段仍过）／**不断条目字段类型**（PM 条文只断键名集合）。
// · **防假绿**：任一内层断言失败 ⇒ 整条判失败（`ok` 必须为 `false`），不许「无论成败都 true」。
// · **真实仓库对账**：§4.3 的键名与协议正文一致（`EXPECT` 没漂移）＋ 扫描面文件数达标。
//
// ## 纪律
//
// · 夹具全在内存里（不落盘）；**永不碰用户的真实数据目录**、不发任何网络请求。
//
// @module scripts/check-official-chat-p4-shape-test

import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

import {
  ARRAY_PATH,
  EXPECT,
  FAIL,
  PASS,
  SKIPPED,
  assertShape,
  buildScenarios,
  collectSources,
  main,
  measuredShapeFixture,
  protocolReconcile,
  runScenarios,
  summarize,
} from './check-official-chat-p4-shape.mjs';

const REPO = fileURLToPath(new URL('../', import.meta.url));

/**
 * 找某条判据的结果。
 *
 * @param {Array<{name: string, status: string, detail: string}>} results
 * @param {string} keyword - 判据名关键词。
 * @returns {{name: string, status: string, detail: string}}
 */
function pick(results, keyword) {
  const found = results.find((r) => r.name.includes(keyword));
  assert.ok(found, `判据结果里找不到「${keyword}」`);
  return found;
}

test('EXPECT 与协议 §4.3 写死的键名一致（不漂移）', () => {
  assert.equal(EXPECT.arrayKey, 'chat_sessions');
  assert.equal(EXPECT.hasMoreKey, 'has_more');
  assert.deepEqual(
    [...EXPECT.itemKeys].sort(),
    ['id', 'model_type', 'pinned', 'title', 'title_type', 'updated_at'],
  );
  assert.equal(EXPECT.cursorKey, null, '分支 B：响应侧无游标 ⇒ 游标不进本条断言');
  assert.equal(ARRAY_PATH, 'data.biz_data.chat_sessions');
});

test('场景表 · 8 个场景的判定全部符合预期', () => {
  const scenarios = buildScenarios();
  assert.equal(scenarios.length, 8);
  const results = runScenarios(scenarios);
  for (const r of results) {
    assert.equal(r.status, PASS, `${r.name} 未按预期：${r.detail}`);
  }
  const summary = summarize(results);
  assert.equal(summary.fail, 0, `不该有失败场景：${JSON.stringify(summary)}`);
});

test('场景 A · 正确形态 ⇒ 全部内层断言都过', () => {
  const { ok, checks } = assertShape(measuredShapeFixture({ count: 1 }));
  assert.equal(ok, true);
  for (const c of checks) assert.equal(c.ok, true, `${c.name} 没过：${c.detail}`);
  assert.ok(checks.some((c) => c.name.includes('has_more 存在且为布尔')));
  assert.ok(checks.some((c) => c.name.includes('首条键名集合相符')));
});

test('场景 B · 顶层 code=0 但 biz_data=null ⇒ 必须判失败（§4.2 反例）', () => {
  const res = { status: 200, code: 0, msg: '', data: { biz_code: 2, biz_msg: '', biz_data: null } };
  const { ok } = assertShape(res);
  assert.equal(ok, false, 'code=0 单独不构成成功 ⇒ 必须判失败');
});

test('场景 C · has_more 缺失 ⇒ 必须判失败', () => {
  const res = measuredShapeFixture({ count: 1 });
  delete res.data.biz_data[EXPECT.hasMoreKey];
  assert.equal(assertShape(res).ok, false);
});

test('场景 D · 条目键名少一个 ⇒ 必须判失败', () => {
  const res = measuredShapeFixture({ count: 1 });
  delete res.data.biz_data[EXPECT.arrayKey][0][EXPECT.itemKeys[0]];
  assert.equal(assertShape(res).ok, false);
});

test('场景 E · 读到 0 条 ⇒ 必须判失败', () => {
  const res = measuredShapeFixture({ count: 1 });
  res.data.biz_data[EXPECT.arrayKey] = [];
  assert.equal(assertShape(res).ok, false);
});

test('场景 H · 条目多一个键 ⇒ 必须判失败（少字段／多字段都要判失败）', () => {
  const res = measuredShapeFixture({ count: 1 });
  res.data.biz_data[EXPECT.arrayKey][0].some_new_field = 1;
  assert.equal(assertShape(res).ok, false);
});

test('防假红 · 不断绝对条数（len=1 与 len=100 都过）', () => {
  assert.equal(assertShape(measuredShapeFixture({ count: 1 })).ok, true);
  assert.equal(assertShape(measuredShapeFixture({ count: 100 })).ok, true);
});

test('防假红 · 不断 biz_data 键集合相等（站点加字段仍过）', () => {
  assert.equal(assertShape(measuredShapeFixture({ count: 1, extraBizKey: true })).ok, true);
});

test('防假红 · 不断条目字段类型（PM 条文只断键名集合）', () => {
  const res = measuredShapeFixture({ count: 1 });
  res.data.biz_data[EXPECT.arrayKey][0].pinned = 'false';
  res.data.biz_data[EXPECT.arrayKey][0].updated_at = '0';
  assert.equal(assertShape(res).ok, true, '类型不在本条断言里（PM 条文只断键名集合）');
});

test('防假绿 · 内层断言失败 ⇒ 整条判失败（不许「无论成败都 true」）', () => {
  const res = measuredShapeFixture({ count: 1 });
  res.data.biz_data[EXPECT.arrayKey][0].id = undefined;
  delete res.data.biz_data[EXPECT.arrayKey][0].id;
  const { ok, checks } = assertShape(res);
  assert.equal(ok, false, '键名不符 ⇒ ok 必须为 false');
  assert.ok(checks.some((c) => !c.ok && c.name.includes('首条键名集合相符')));
});

test('防假绿 · expectSuccess=false ⇒ 直接判失败且不产出断言', () => {
  const { ok, checks } = assertShape(measuredShapeFixture({ count: 1 }), { expectSuccess: false });
  assert.equal(ok, false);
  assert.equal(checks.length, 0);
});

test('真实仓库 · 扫描面文件数达标（防「扫了 0 个文件却报绿」）', () => {
  const sources = collectSources(REPO);
  assert.ok(
    sources.length >= 1,
    `真实仓库的扫描面必须扫到协议正文（实际 ${sources.length} 个）`,
  );
  assert.equal(sources[0].path, 'docs/features/official-chat/official-chat-p4-protocol.md');
});

test('真实仓库 · 协议正文键名与 EXPECT 一致（§4.3 对账）', () => {
  const verdict = protocolReconcile(REPO);
  assert.equal(verdict.status, PASS, `协议与判据漂移：${verdict.detail}`);
  assert.equal(verdict.scanned.length, 1);
});

test('主入口 · 全绿且退出码语义正确（场景级）', async () => {
  const { results, summary } = await main(['--round', 'p4c8-selftest', '--json']);
  assert.equal(summary.fail, 0, `不该有失败：${JSON.stringify(summary)}`);
  assert.ok(summary.total >= 9);
  const evidence = pick(results, '8a');
  assert.equal(evidence.status, PASS);
  assert.equal(pick(results, '8b').status, PASS);
  assert.ok(results.every((r) => [PASS, FAIL, SKIPPED].includes(r.status)));
});
