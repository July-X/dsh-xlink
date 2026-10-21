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
  assert.deepEqual(
    { ...EXPECT.itemTypes },
    {
      id: 'string',
      title: 'string',
      title_type: 'string',
      model_type: 'string',
      pinned: 'boolean',
      updated_at: 'number',
    },
    '§4.3 ② 的 6 个类型要进 EXPECT（PM 16:0x 裁定：要断类型）',
  );
  assert.equal(EXPECT.cursorKey, null, '分支 B：响应侧无游标 ⇒ 游标不进本条断言');
  assert.equal(ARRAY_PATH, 'data.biz_data.chat_sessions');
});

test('场景表 · 9 个场景的判定全部符合预期', () => {
  const scenarios = buildScenarios();
  assert.equal(scenarios.length, 9);
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

test('类型 · 条目 6 键的类型要断（PM 16:0x 裁定：§4.3 ② 的类型标注要有机检守着）', () => {
  // 六个键逐个塞错类型 ⇒ **每一个**都必须判红，且断言要点名是哪个键
  const wrong = {
    id: 1,
    title: 1,
    title_type: 1,
    model_type: 1,
    pinned: 'false',
    updated_at: '0',
  };
  for (const [key, badValue] of Object.entries(wrong)) {
    const res = measuredShapeFixture({ count: 1 });
    res.data.biz_data[EXPECT.arrayKey][0][key] = badValue;
    const { ok, checks } = assertShape(res);
    assert.equal(ok, false, `键「${key}」类型错必须判红`);
    assert.ok(
      checks.some((c) => c.name.includes(`条目字段类型 ${key}＝`) && !c.ok),
      `类型断言必须点名键「${key}」`,
    );
  }
});

test('类型 · 正确类型必须通过（别把类型断成永红）', () => {
  const { ok, checks } = assertShape(measuredShapeFixture({ count: 1 }));
  assert.equal(ok, true, '实测类型（pinned 布尔／updated_at 数字／其余字符串）必须全过');
  for (const key of Object.keys(EXPECT.itemTypes)) {
    assert.ok(
      checks.some((c) => c.name.includes(`条目字段类型 ${key}＝`) && c.ok),
      `键「${key}」的类型断言必须存在且通过`,
    );
  }
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

test('主入口 · 8a/8b 全绿；8c **场景表**全绿，8c 真实轨在实现侧到位前红（红有出处）', async () => {
  // ⚠️ `main()` 会按结论设 `process.exitCode`（8c 真实轨在实现侧到位前红 ⇒ 1）
  // ⇒ ⇒ **自测里必须把它还原**，否则 `node --test` 会把**整个测试文件**判成失败
  //    （本体直接跑仍按结论 exit 1 —— 那才是「如实」）。
  const savedExitCode = process.exitCode;
  let results;
  let summary;
  try {
    ({ results, summary } = await main(['--round', 'p4c8-selftest', '--json']));
  } finally {
    process.exitCode = savedExitCode;
  }
  const evidence = pick(results, '8a');
  assert.equal(evidence.status, PASS);
  assert.equal(pick(results, '8b').status, PASS);
  assert.ok(results.every((r) => [PASS, FAIL, SKIPPED].includes(r.status)));

  // 8c 的**场景表**（合成读数）必须全绿 —— 那是判据自身工作正常的证据。
  const itemScenarios = results.filter((r) => r.name.startsWith('P4-8c 场景'));
  assert.ok(itemScenarios.length >= 5, '8c 要有场景表');
  const badScenarios = itemScenarios.filter((r) => r.status !== PASS);
  assert.equal(badScenarios.length, 0, `8c 场景表必须全绿：${badScenarios.map((r) => r.name).join('；')}`);

  // 8c 的**真实读数轨**：实现侧还没加 `items` ⇒ 红 ⇒ 但红必须点名「目录项面未到位」。
  const realChecks = results.filter((r) => r.name.startsWith('P4-8c 真实读数'));
  const realBad = realChecks.filter((r) => r.status === FAIL);
  if (realBad.length > 0) {
    assert.equal(
      realBad.length,
      realChecks.length,
      '8c 真实轨要么全绿、要么全红（不许半红半绿）',
    );
    for (const r of realBad) {
      assert.match(
        r.detail,
        /没读到|未到位|没加/,
        `8c 真实轨的红必须点名「没读到 items 形态／实现侧还没加」：${r.name}：${r.detail}`,
      );
    }
  }
  // 主入口的失败**只允许**来自 8c 真实轨（实现侧未到位）⇒ 其余必须全绿。
  const otherBad = results.filter(
    (r) => r.status === FAIL && !r.name.startsWith('P4-8c 真实读数'),
  );
  assert.equal(otherBad.length, 0, `除 8c 真实轨外不许有失败：${otherBad.map((r) => r.name).join('；')}`);
  assert.ok(summary.total >= 9);
});
