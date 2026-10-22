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
  assertReturnItemsShape,
  assertCatalogSourceShape,
  assertEnvelopeSiteShape,
  assertShape,
  buildItemScenarios,
  buildScenarios,
  collectSources,
  main,
  measuredShapeFixture,
  protocolReconcile,
  readEnvelopeSiteShape,
  readReturnItemsReading,
  readSourceShape,
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

test('插件侧契约面 · 逐项「改坏一处」⇒ **契约**那几条断言必须红（G4-21 自证）', () => {
  const good = buildItemScenarios()[0].reading;
  const cases = [
    {
      what: '契约少一个键（回到旧形态）',
      expect: '键面',
      mutate: (r) => ({
        ...r,
        contractItemFields: r.contractItemFields.filter((f) => f.name !== 'pinned'),
      }),
    },
    {
      what: '契约里 deepLink 类型错（给成 number）',
      expect: 'deepLink',
      mutate: (r) => ({
        ...r,
        contractItemFields: [
          ...r.contractItemFields.map((f) => ({ ...f })),
          { name: 'deepLink', type: 'number' },
        ],
      }),
    },
    {
      what: '契约的类型错（pinned 给成 string）',
      expect: '类型',
      mutate: (r) => ({
        ...r,
        contractItemFields: r.contractItemFields.map((f) =>
          f.name === 'pinned' ? { name: f.name, type: 'string' } : { ...f },
        ),
      }),
    },
    {
      what: '契约整个读不到（文件不在扫描面／表被改名）',
      expect: '契约',
      mutate: (r) => ({ ...r, contractItemFields: [] }),
    },
  ];
  for (const c of cases) {
    const { ok, checks } = assertReturnItemsShape(c.mutate(good));
    assert.equal(ok, false, '改坏「' + c.what + '」后整条必须判红');
    const hit = checks.find((x) => !x.ok && x.name.includes('契约') && x.name.includes(c.expect));
    assert.ok(
      hit,
      '改坏「' + c.what + '」后，必须由含「契约」且含「' + c.expect + '」的断言报红（实际：' +
        checks.map((x) => x.name + '=' + (x.ok ? '过' : '红')).join('；') +
        '）',
    );
  }
});

test('目录项面 · `deepLink`（可选第 7 键）**string 过／别的类型红／别的第 7 键仍红**', () => {
  const good = buildItemScenarios()[0].reading;
  const withField = (fields, name, type) => [...fields, { name, type }];

  // ① 类型是 string ⇒ 全绿（载荷面 ＋ 契约面都过）。
  const ok = assertReturnItemsShape({
    ...good,
    itemFields: withField(good.itemFields, 'deepLink', 'string'),
    contractItemFields: withField(good.contractItemFields, 'deepLink', 'string'),
  });
  assert.equal(ok.ok, true, '`deepLink` 是 string 时必须通过：' + JSON.stringify(ok.checks));

  // ② 类型不是 string ⇒ **必须**由 `deepLink` 那条断言报红（不许靠别的断言顶红）。
  for (const badType of ['number', 'boolean', 'object']) {
    const { ok: passed, checks } = assertReturnItemsShape({
      ...good,
      itemFields: withField(good.itemFields, 'deepLink', badType),
    });
    assert.equal(passed, false, `\`deepLink\` 类型为 ${badType} 时必须判红`);
    const hit = checks.find((c) => !c.ok && c.name.includes('deepLink'));
    assert.ok(hit, `必须由 deepLink 那条断言报红（实际：${checks.map((c) => c.name).join('；')}）`);
  }

  // ③ 别的第 7 个键 ⇒ 仍然红（「放宽一处」≠「放水」）。
  const other = assertReturnItemsShape({
    ...good,
    itemFields: withField(good.itemFields, 'extra_key', 'string'),
  });
  assert.equal(other.ok, false, '别的第 7 个键必须仍然判红');
});

test('真实读数 · 插件侧契约面全绿（契约面有人守：G4-21 已闭合）', () => {
  const { ok, checks } = assertReturnItemsShape(readReturnItemsReading(REPO));
  const contract = checks.filter((c) => c.name.includes('插件侧契约'));
  assert.ok(contract.length > 0, '契约面必须产出断言');
  for (const c of contract) {
    assert.equal(c.ok, true, '契约面断言必须全绿：' + c.name + '：' + c.detail);
  }
  assert.equal(ok, true, '真实读数整条必须过');
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

test('主入口 · 8a／8b／8c 全绿（场景表 ＋ 真实读数轨都绿）', async () => {
  // ⚠️ `main()` 会按结论设 `process.exitCode`（有红就 1）
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
  assert.ok(results.every((r) => [PASS, FAIL, SKIPPED].includes(r.status)));

  // **8a**（本条本体：响应形态场景表 ＋ 协议对账 ＋ 目录项面）
  assert.equal(pick(results, 'P4-8a 协议正文键名与判据一致').status, PASS);
  const itemScenarios = results.filter((r) => r.name.startsWith('P4-8a 目录项面 场景'));
  assert.ok(itemScenarios.length >= 3, '8a 要有目录项面的场景表');
  const badScenarios = itemScenarios.filter((r) => r.status !== PASS);
  assert.equal(badScenarios.length, 0, `8a 场景表必须全绿：${badScenarios.map((r) => r.name).join('；')}`);

  // **8b**（目录页 `page.source` 形态）＝ 协议 v0.19 §10 第 8 条的新子项
  const eightB = results.filter((r) => r.name.startsWith('P4-8b '));
  assert.ok(eightB.length >= 8, `8b 要有那几条断点（实际 ${eightB.length}）`);
  const badB = eightB.filter((r) => r.status === FAIL);
  assert.equal(badB.length, 0, `8b 必须全绿：${badB.map((r) => `${r.name}：${r.detail}`).join('；')}`);

  // **8c**（桥回包信封上的 `site` 短码）＝ 协议 v0.19 §10 第 8 条的新子项
  const eightC = results.filter((r) => r.name.startsWith('P4-8c '));
  assert.ok(eightC.length >= 8, `8c 要有那几条断点（实际 ${eightC.length}）`);
  const badC = eightC.filter((r) => r.status === FAIL);
  assert.equal(badC.length, 0, `8c 必须全绿：${badC.map((r) => `${r.name}：${r.detail}`).join('；')}`);

  assert.ok(summary.total >= 9);
});

test('8b · 逐项「改坏一处」⇒ **对应那条**必须红（来源标识漂了照不出来就是假绿）', () => {
  const base = {
    contractKinds: ['site', 'fixture'],
    builderKeySets: [['kind', 'id', 'label']],
    samples: [],
  };
  const cases = [
    {
      what: '契约里的 kind 闭集被改了',
      expect: '闭集',
      mutate: (r) => ({ ...r, contractKinds: ['site', 'fixture', 'other'] }),
    },
    {
      what: 'buildCatalogSource 的返回对象多了一个键',
      expect: '恰 3 键',
      mutate: (r) => ({ ...r, builderKeySets: [['kind', 'id', 'label', 'extra']] }),
    },
    {
      what: '行为样本：多一个键没被拒',
      expect: '样本',
      mutate: (r) => ({
        ...r,
        samples: [{ name: '样本：source 多一个键', ok: false, detail: '实际通过' }],
      }),
    },
    {
      what: '一条行为样本都没跑（读不到被测对象）',
      expect: '样本',
      mutate: (r) => ({ ...r, samples: [] }),
    },
  ];
  for (const c of cases) {
    const { ok, checks } = assertCatalogSourceShape(c.mutate(base));
    assert.equal(ok, false, `改坏「${c.what}」后整条必须判红`);
    const hit = checks.find((x) => !x.ok && x.name.includes(c.expect));
    assert.ok(
      hit,
      `改坏「${c.what}」后必须由含「${c.expect}」的断言报红（实际：${checks
        .map((x) => `${x.name}=${x.ok ? '过' : '红'}`)
        .join('；')}）`,
    );
  }
});

test('8c · 逐项「改坏一处」⇒ **对应那条**必须红（信封短码漂了照不出来就是假绿）', () => {
  const base = {
    rustSite: 'deepseek',
    envelopeHasSite: true,
    payloadHasSite: false,
    samples: [],
  };
  const cases = [
    {
      what: '内核的短码常量形态不对（大写）',
      expect: '常量',
      mutate: (r) => ({ ...r, rustSite: 'DeepSeek' }),
    },
    {
      what: '信封上没带 site',
      expect: '信封',
      mutate: (r) => ({ ...r, envelopeHasSite: false }),
    },
    {
      what: 'site 混进了回程载荷',
      expect: '载荷',
      mutate: (r) => ({ ...r, payloadHasSite: true }),
    },
    {
      what: '行为样本：认不出却没给空串',
      expect: '样本',
      mutate: (r) => ({
        ...r,
        samples: [{ name: '样本：解析 DeepSeek', ok: false, detail: '实际没给空串' }],
      }),
    },
  ];
  for (const c of cases) {
    const { ok, checks } = assertEnvelopeSiteShape(c.mutate(base));
    assert.equal(ok, false, `改坏「${c.what}」后整条必须判红`);
    const hit = checks.find((x) => !x.ok && x.name.includes(c.expect));
    assert.ok(
      hit,
      `改坏「${c.what}」后必须由含「${c.expect}」的断言报红（实际：${checks
        .map((x) => `${x.name}=${x.ok ? '过' : '红'}`)
        .join('；')}）`,
    );
  }
});

test('8b／8c · 真实读数（插件侧契约 ＋ 内核侧信封）', async () => {
  const sourceReading = await readSourceShape(REPO);
  const source = assertCatalogSourceShape(sourceReading);
  assert.equal(
    source.ok,
    true,
    `8b 真实读数必须全绿：${source.checks
      .filter((c) => !c.ok)
      .map((c) => `${c.name}：${c.detail}`)
      .join('；')}`,
  );
  const envelopeReading = await readEnvelopeSiteShape(REPO);
  const envelope = assertEnvelopeSiteShape(envelopeReading);
  assert.equal(
    envelope.ok,
    true,
    `8c 真实读数必须全绿：${envelope.checks
      .filter((c) => !c.ok)
      .map((c) => `${c.name}：${c.detail}`)
      .join('；')}`,
  );
});
