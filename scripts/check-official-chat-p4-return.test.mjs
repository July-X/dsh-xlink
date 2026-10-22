// P4 判据**第 3 条**（回程只回结构 · **两面**）的自证。
//
// ## 判什么
//
// · **场景表必须自证**：真身通过 · 顶层混进禁项失败 · **目录项里塞 `body` 失败** ·
//   **目录项多一个第 7 键失败** · **顶层多出 `deepLink` 失败**（它只解禁在目录项面）·
//   **载荷多出 `cursor` 失败** · **结构面少一项失败** · **两端结构面不一致失败** ·
//   **两端目录项面不一致失败** · **目录项类型不符（`pinned` 给成 string）失败** ·
//   **实现侧还没加目录项面失败** · **目录项里 `deepLink`（string）通过／（number）失败** ·
//   **契约里 `deepLink`（string）通过／（number）失败**。
// · **白名单是穷举列举**：结构面 8 项 ＋ 目录项面入口 `items`；目录项元素 6 键
//   ＋ **可选第 7 键 `deepLink`（类型 string）**。`cursor` **不在**白名单里
//   （本轮不回 ⇒ 出现即红）；`deepLink` 是**目录项面**的键 ⇒ **不进顶层白名单**。
// · **真实仓库读数**：**结构面必须全绿**；**目录项面在实现侧到位前红**（红在
//   「实现侧还没加目录项面」，**不是**判据自身缺陷）⇒ ⇒ 实现落地后本条会自动转绿。
//
// @module scripts/check-official-chat-p4-return-test

import assert from 'node:assert/strict';
import test from 'node:test';

import {
  FORBIDDEN,
  FORBIDDEN_ITEM,
  ITEM_KEYS,
  ITEM_OPTIONAL_KEYS,
  ITEM_TYPES,
  PASS,
  TOP_WHITELIST,
  WHITELIST,
  buildScenarios,
  judgeReturn,
  readReading,
} from './check-official-chat-p4-return.mjs';
import { repoRoot } from './lib/p4-source-shape.mjs';

test('P4-3：场景表逐条自证（白名单之外即红 ＋ 目录项面新断点）', () => {
  const scenarios = buildScenarios();
  assert.ok(scenarios.length >= 12, '场景表要覆盖新断点');
  for (const sc of scenarios) {
    const { ok, checks } = judgeReturn(sc.reading);
    const behaved = sc.expect === PASS ? ok : !ok;
    assert.equal(
      behaved,
      true,
      `场景 ${sc.id} 的判定与期望不符：${checks
        .map((c) => `${c.name}=${c.status}（${c.detail}）`)
        .join('；')}`,
    );
  }
});

test('P4-3：每个新断点单独「改坏一处」⇒ **对应的那条断言**必须红（不许靠别的断言顶红）', () => {
  const clean = buildScenarios().find((s) => s.id === 'A').reading;
  const itemOf = (fields, name, type) => [...fields, { name, type }];
  const cases = [
    {
      what: 'items 里塞 body（正文进数组内部）',
      expect: '目录项内部',
      mutate: (r) => ({
        ...r,
        rustItemFields: itemOf(r.rustItemFields, 'body', 'string'),
        jsItemFields: itemOf(r.jsItemFields, 'body', 'string'),
      }),
    },
    {
      what: 'items 多一个第 7 键',
      expect: '元素恰好',
      mutate: (r) => ({
        ...r,
        rustItemFields: itemOf(r.rustItemFields, 'extra_key', 'string'),
        jsItemFields: itemOf(r.jsItemFields, 'extra_key', 'string'),
      }),
    },
    {
      what: '顶层多出 deepLink（它只解禁在目录项面）',
      expect: '⊆ 白名单',
      mutate: (r) => ({ ...r, rustKeys: [...r.rustKeys, 'deepLink'] }),
    },
    {
      what: '载荷多出 cursor',
      expect: '⊆ 白名单',
      mutate: (r) => ({ ...r, jsKeys: [...r.jsKeys, 'cursor'] }),
    },
    {
      what: '结构面少一项',
      expect: '结构面',
      mutate: (r) => ({
        ...r,
        rustKeys: r.rustKeys.filter((k) => k !== 'hasMore'),
        jsKeys: r.jsKeys.filter((k) => k !== 'hasMore'),
      }),
    },
    {
      what: '两端结构面不一致',
      expect: '结构面',
      mutate: (r) => ({ ...r, jsKeys: r.jsKeys.filter((k) => k !== 'hasMore') }),
    },
    {
      what: '目录项类型不符（pinned 给成 string）',
      expect: '6 键的类型',
      mutate: (r) => ({
        ...r,
        rustItemFields: r.rustItemFields.map((f) =>
          f.name === 'pinned' ? { name: f.name, type: 'string' } : { ...f },
        ),
        jsItemFields: r.jsItemFields.map((f) =>
          f.name === 'pinned' ? { name: f.name, type: 'string' } : { ...f },
        ),
      }),
    },
    {
      what: '两端目录项面不一致（只有一端有 items 元素）',
      expect: '目录项面',
      mutate: (r) => ({ ...r, jsItemFields: [] }),
    },
    // --- 插件侧**契约面**（缺口 G4-21：契约漂移也得有人守）---
    {
      what: '插件侧契约少一个键（回到旧形态）',
      expect: '键面',
      mutate: (r) => ({
        ...r,
        jsContractItemFields: r.jsContractItemFields.filter((f) => f.name !== 'pinned'),
      }),
    },
    {
      what: '目录项里 deepLink 类型错（给成 number）',
      expect: 'deepLink',
      mutate: (r) => ({
        ...r,
        rustItemFields: itemOf(r.rustItemFields, 'deepLink', 'number'),
        jsItemFields: itemOf(r.jsItemFields, 'deepLink', 'number'),
      }),
    },
    {
      what: '插件侧契约里 deepLink 类型错（给成 number）',
      expect: '类型',
      mutate: (r) => ({
        ...r,
        jsContractItemFields: [
          ...r.jsContractItemFields.map((f) => ({ ...f })),
          { name: 'deepLink', type: 'number' },
        ],
      }),
    },
    {
      what: '插件侧契约的类型错（pinned 给成 string）',
      expect: '类型',
      mutate: (r) => ({
        ...r,
        jsContractItemFields: r.jsContractItemFields.map((f) =>
          f.name === 'pinned' ? { name: f.name, type: 'string' } : { ...f },
        ),
      }),
    },
  ];
  for (const c of cases) {
    const { checks } = judgeReturn(c.mutate(clean));
    const hit = checks.find(
      (x) => x.status === 'fail' && x.name.replace(/\*/g, '').includes(c.expect),
    );
    assert.ok(hit, `改坏「${c.what}」后，必须由含「${c.expect}」的断言报红（实际：${checks
      .map((x) => `${x.name}=${x.status}`)
      .join('；')}）`);
  }
});

test('P4-3：顶层白名单里**没有**禁回传的字段（正文/标题/会话 id/凭据）', () => {
  for (const key of FORBIDDEN) {
    assert.ok(!WHITELIST.includes(key), `结构面白名单里混进了 ${key}`);
    assert.ok(!TOP_WHITELIST.includes(key), `顶层白名单里混进了 ${key}`);
  }
});

test('P4-3：目录项元素白名单里**没有**正文/凭据类（`title`/`id` 是 §4.3 ② 允许的）', () => {
  for (const key of FORBIDDEN_ITEM) {
    assert.ok(!ITEM_KEYS.includes(key), `目录项元素白名单里混进了 ${key}`);
  }
  // `title` / `id` 在**元素**里是允许的（协议 §4.3 ②）⇒ 不在元素禁项里。
  assert.ok(!FORBIDDEN_ITEM.includes('title'));
  assert.ok(!FORBIDDEN_ITEM.includes('id'));
});

test('P4-3：白名单是**穷举列举**（`cursor` 不在里面 ⇒ 本轮不回；`deepLink` 只在**目录项面**解禁）', () => {
  assert.ok(!TOP_WHITELIST.includes('cursor'));
  assert.ok(
    !TOP_WHITELIST.includes('deepLink'),
    '`deepLink` 是**目录项面**的键（逐会话的值）⇒ 不许进顶层白名单',
  );
  assert.deepEqual(
    [...TOP_WHITELIST].sort(),
    [...WHITELIST, 'items'].sort(),
    '顶层白名单＝结构面 8 项 ＋ 目录项面入口 `items`（不多不少）',
  );
  assert.equal(ITEM_KEYS.length, 6);
  assert.deepEqual(
    Object.keys(ITEM_TYPES).sort(),
    [...ITEM_KEYS].sort(),
    '§4.3 ② 的 6 个键 ＝ 类型表要一一对应',
  );
  // **可选第 7 键 `deepLink`**：只解禁在**目录项面**（类型 string）。
  assert.deepEqual({ ...ITEM_OPTIONAL_KEYS }, { deepLink: 'string' });
  assert.ok(
    !FORBIDDEN_ITEM.includes('deepLink'),
    '`deepLink` 已解禁 ⇒ 它不在目录项**元素**禁项里',
  );
  assert.ok(
    FORBIDDEN.includes('deepLink'),
    '`deepLink` 在**顶层**仍然禁（逐会话的值不许提到顶层）',
  );
});

test('P4-3：`deepLink`（可选第 7 键）**string 过／别的类型红**（放宽一处 ≠ 放水）', () => {
  const clean = buildScenarios().find((s) => s.id === 'A').reading;
  const withDeepLink = (type) => ({
    ...clean,
    rustItemFields: [...clean.rustItemFields, { name: 'deepLink', type }],
    jsItemFields: [...clean.jsItemFields, { name: 'deepLink', type }],
  });

  // ① 类型是 string ⇒ 全绿。
  const good = judgeReturn(withDeepLink('string'));
  assert.equal(good.ok, true, '`deepLink` 是 string 时必须通过');

  // ② 类型不是 string ⇒ **必须**由 `deepLink` 那条断言报红（不许靠别的断言顶红）。
  for (const badType of ['number', 'boolean', 'object']) {
    const { checks } = judgeReturn(withDeepLink(badType));
    const hit = checks.find(
      (c) => c.status === 'fail' && c.name.replace(/\*/g, '').includes('deepLink'),
    );
    assert.ok(hit, `\`deepLink\` 类型为 ${badType} 时必须由 deepLink 那条断言报红`);
  }

  // ③ 别的第 7 个键 ⇒ 仍然红（「放宽一处」≠「放水」）。
  const other = judgeReturn({
    ...clean,
    rustItemFields: [...clean.rustItemFields, { name: 'extra_key', type: 'string' }],
    jsItemFields: [...clean.jsItemFields, { name: 'extra_key', type: 'string' }],
  });
  assert.equal(other.ok, false, '别的第 7 个键必须仍然判红');
});

test('P4-3：真实读数里**插件侧契约面**三条断言全绿（缺口 G4-21 已闭合：契约漂移当场红）', () => {
  const { checks } = judgeReturn(readReading(repoRoot(import.meta.url)));
  const contract = checks.filter((c) => c.name.replace(/\*/g, '').includes('插件侧契约'));
  assert.equal(
    contract.length,
    3,
    `契约面应该正好三条断言（键面／类型／禁项）：实际 ${contract.map((c) => c.name).join('；')}`,
  );
  for (const c of contract) {
    assert.equal(
      c.status,
      PASS,
      '契约面断言必须全绿（真实 protocol.js 就是契约事实源）：' + c.name + '：' + c.detail,
    );
  }
});

test('P4-3：真实仓库读数 —— 结构面全绿；目录项面在实现侧到位前红（红有出处）', () => {
  const { checks } = judgeReturn(readReading(repoRoot(import.meta.url)));
  // 目录项面断言的识别：**按断言名逐条挑**，不按「含『目录项』三个字」挑
  // —— 否则「⊆ 白名单（结构面 ∪ 目录项面）」那条会被误归进来（它是结构面那条）。
  // ⚠️ 断言名里有 markdown 强调（`**`）⇒ 匹配前先去掉星号。
  const isItemFace = (name) => {
    const plain = name.replace(/\*/g, '');
    return /元素恰好|目录项面逐项相等|目录项 6 键|目录项内部|目录项面入口|deepLink/.test(plain);
  };
  const structFace = checks.filter((c) => !isItemFace(c.name));
  const itemFace = checks.filter((c) => isItemFace(c.name));

  const structBad = structFace.filter((c) => c.status !== PASS);
  assert.equal(
    structBad.length,
    0,
    `结构面必须全绿（实现侧已到位的那部分）：${structBad.map((c) => `${c.name}：${c.detail}`).join('；')}`,
  );

  // 目录项面：要么**全绿**（实现侧已加 `items`），要么**全红**且红因「没读到 items 形态」
  // ⇒ ⇒ 绝不允许「有的红有的绿」（那说明判据口径自身有问题）。
  const itemBad = itemFace.filter((c) => c.status === 'fail');
  const itemSkipped = itemFace.filter((c) => c.status === 'skipped');
  assert.equal(itemSkipped.length, 0, '目录项面的断言不许是「未执行」⇒ 那等于没测到');
  if (itemBad.length > 0) {
    assert.equal(
      itemBad.length,
      itemFace.length,
      '目录项面要么全绿、要么全红（不许半红半绿）',
    );
    for (const c of itemBad) {
      assert.match(
        c.detail,
        /目录项面未到位|没读到/,
        `目录项面的红必须点名「没读到 items 形态／实现侧还没加」：${c.name}：${c.detail}`,
      );
    }
  }
});
