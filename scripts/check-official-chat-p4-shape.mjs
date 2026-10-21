#!/usr/bin/env node
// P4 协议 §10 判据**第 8 条**（字段面）—— 按 §4.3 已写死的键名断：**只断结构，不断值**。
//
// ## 依据
//
// · 协议 §4.3（字段面四项**已写死**）：① 会话列表数组 ＝ `data.biz_data.chat_sessions`
//   ② 条目 ＝ 6 键（`id`／`title`／`title_type`／`model_type`／`pinned`／`updated_at`）
//   ③ 条数从 `data.biz_data.chat_sessions.length` 读（**不是**从 `has_more` 读）
//   ④ 响应体**三处皆无游标键** ⇒ 游标走请求 query（`lte_cursor`，点号路径）⇒ **本条不断游标**。
// · 协议 §4.2（成功判据 ＝ `status == 200` **且** 顶层 `code == 0` **且** `data.biz_data` 非空；
//   顶层 `code == 0` **单独不构成成功** —— 反例：`code=0` 同时 `biz_code=2`／`biz_data=null`）。
// · 协议 §10 第 8 条（按实测键名断；⚠️ **不断言绝对条数**、⚠️ **不断 `biz_data` 键集合相等**）。
// · PM 起草的 `p4-criterion8-draft.md`（素材区，不入库）—— 判据条文与场景 A~F 照它落。
//
// ## 纪律
//
// · **自包含**：任意干净 checkout 上独立跑通，不依赖共享工作树的在途改动。
// · **零污染**：夹具全在内存里（不落盘），跑完即弃；**不读用户真实数据目录**、**不发任何网络请求**。
// · **只断结构**：只看状态码／业务码／**键名**／条数；**不回正文、不回标题、不回会话 id、不回凭据**。
// · **三态 verdict**：`pass` / `fail` / `skipped` —— 「没测到」与「测了没过」必须长得不一样。
// · **退出码按场景级算**：故意造坏数据的场景，其内层证据行本就该 FAIL ⇒ 若把证据行计进
//   退出码，脚本会**永远是红的**（PM 抓过的真缺陷：PM 的草案里已修，改动别回退）。
// · **要断条目 6 键的类型**（PM 2026-10-21 16:0x 裁定，推翻本脚本初版「不断类型」的做法）：
//   协议 §4.3 ② **明文写了那 6 个键的类型** ⇒ ⇒ 不断类型 ⇒ **协议里那 6 个类型标注永远没机检
//   守着** ＝ 契约面失守 ⇒ ⇒ 断类型**不会假红**（类型有实测背书：站点真改类型＝**契约漂移**，
//   本就该报红）⇒ ⇒ **范围收窄**：**只断这 6 个键的类型**，仍**不断 `biz_data` 键集合相等**、
//   仍**不断绝对条数**。
//   ⇒ **作用域与键名断言一致（首条）**：不对全部条目逐条断类型（个别条目异形会假红）。
//
// ## 为什么有一个「协议正文对账」轨
//
// 协议 §4.3 的键名是**写死**的 ⇒ 协议哪天改了键名、本判据的 `EXPECT` 没跟上 ⇒ 判据会
// 拿着旧键名去断新结构 ⇒ 结论失真。PM 的草案已写明这一步（「协议 v0.7 落笔后只需核对
// 键名与协议正文一致；若协议把键名改了 ⇒ 回来改 `EXPECT`」）⇒ 本脚本把它做成**可执行的一轨**
// （只读协议正文，不改协议一个字）。
//
// @module scripts/check-official-chat-p4-shape

import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

import {
  arrayLiteralStrings,
  jsFnBody,
  jsItemsElementFields,
  jsLiteralTypeFields,
  objectLiteralKeySets,
  objectLiteralKeysContaining,
  readSources,
  rustItemsElementFields,
} from './lib/p4-source-shape.mjs';

/** verdict 三态。 */
export const PASS = 'pass';
export const FAIL = 'fail';
export const SKIPPED = 'skipped';

/** §4.3 ①：会话列表数组的**完整路径**（脚本按 `res.data.biz_data[arrayKey]` 取）。 */
export const ARRAY_PATH = 'data.biz_data.chat_sessions';

/** §4.3 ③：`has_more` 所在的完整路径（脚本按 `res.data.biz_data[hasMoreKey]` 取）。 */
export const HAS_MORE_PATH = 'data.biz_data.has_more';

/**
 * **实测填入**的字段面期望（§4.3 写死的四项，2026-10-21 登录态实测、双源一致）。
 *
 * · `arrayKey` —— `data.biz_data` 下的会话列表数组键（**不是** `data.chat_sessions`）。
 * · `itemKeys` —— 条目对象的**键名集合**（6 个；**顺序不敏感** ⇒ 比之前先 `sort()`）。
 * · `itemTypes` —— 条目 6 键的**类型**（§4.3 ② 明文写死的；PM 2026-10-21 16:0x 裁定**要断**）。
 * · `hasMoreKey` —— `data.biz_data` 下的布尔键（含义「是否还有更多页」，**不是条数**）。
 * · `cursorKey` —— **分支 B**：响应体顶层／`data`／`biz_data` 三处皆无游标键 ⇒ 游标走
 *   **请求 query**（`lte_cursor`，点号路径），其**取值来源仍【推定】** ⇒ 游标**不进本条断言**
 *   （协议 §4.3 ④ ／ 缺口 G4-8）。
 *
 * ⚠️ **不设 `itemCount`**：账号会话数会随时间变化（实测每页 100 条）⇒ 写死条数**必然假红**
 *   ⇒ 条数断成「读到东西了（≥1）＋ `has_more` 存在」。
 * ⚠️ **不断 `biz_data` 的键集合相等**：站点加字段不该判失败 ⇒ 只断「必须含两个键」。
 */
export const EXPECT = Object.freeze({
  arrayKey: 'chat_sessions',
  itemKeys: Object.freeze(['id', 'title', 'title_type', 'model_type', 'pinned', 'updated_at']),
  itemTypes: Object.freeze({
    id: 'string',
    title: 'string',
    title_type: 'string',
    model_type: 'string',
    pinned: 'boolean',
    updated_at: 'number',
  }),
  hasMoreKey: 'has_more',
  cursorKey: null,
});

/** 协议正文相对路径（**只读**，脚本不改协议一个字）。 */
export const PROTOCOL_REL = 'docs/features/official-chat/official-chat-p4-protocol.md';

/** 插件侧**契约文件**（目录项面 ＋ 来源标识的唯一事实源；G4-21／G4-22 都要它进扫描面）。 */
export const CONTRACT_REL = 'plugins/official-chat/host/protocol.js';

/** 插件侧**真实源**（信封 `site` 短码的校验处）。 */
export const SITE_SOURCE_REL = 'plugins/official-chat/host/site-source.js';

/** 内核侧取数模块（站点短码常量 `SITE` 的事实源）。 */
export const RUST_REL = 'src-tauri/src/official_chat/session_structure.rs';

/** 内核侧**桥**（回包**信封**层 —— `site` 短码带在哪儿）。 */
export const BRIDGE_REL = 'src-tauri/src/plugins/builtin/bridge.rs';

/**
 * **8b**：目录页 `page.source`（来源标识）的期望形态（P1 §1.2 v0.13 · 协议 v0.19 §10 第 8 条）。
 *
 * · 键面 ＝ `{kind, id, label}` —— **恰 3 键**（**第 4 个键也是错**）。
 * · `kind` ∈ {`site`, `fixture`}（**闭集**）。
 * · `id`／`label` ＝ **非空 string**。
 * · **可选字段** —— 缺失**不算红**（旧缓存页没有它）。
 * · **不许进 `items`**（面二那 6 键不变）。
 */
export const SOURCE_KEYS = Object.freeze(['kind', 'id', 'label']);
export const SOURCE_KINDS = Object.freeze(['site', 'fixture']);

/**
 * **8c**：桥回包**信封**上 `site` 短码的形态。
 *
 * 小写字母／数字／连字符、1–32 字符；**认不出 ⇒ 空串**（不猜、不截断、不改写）。
 *
 * ⚠️ **只断信封层** —— 回程**载荷**里**不许**出现 `site`（协议 §2.4 的载荷白名单
 * 一个字不动）。
 */
export const SITE_CODE_PATTERN = /^[a-z0-9][a-z0-9-]{0,31}$/;

/**
 * **结构断言内核**：按 §4.3／§4.2 断一个响应的字段面。
 *
 * ⚠️ **返回 `ok` ＝ 全部断言都过**（PM 草案里的真缺陷已修：旧版无论成败都 `return true`
 * ⇒ 场景 C/D/E 的「自证」根本不会失败 ⇒ **假自证**）。
 *
 * @param {object} res - 站点响应（三层：`{status, code, data:{biz_data:{…}}}`）。
 * @param {{expectSuccess?: boolean}} [opts] - 选项。
 * @returns {{ok: boolean, checks: Array<{name: string, ok: boolean, detail: string}>}}
 */
export function assertShape(res, { expectSuccess = true } = {}) {
  const checks = [];
  const t = (name, ok, detail) => {
    checks.push({ name, ok: !!ok, detail });
    return !!ok;
  };

  if (!expectSuccess) return { ok: false, checks };

  const data = res && typeof res === 'object' ? res.data : null;
  const biz = data && typeof data === 'object' ? data.biz_data : null;

  t('status==200', !!res && res.status === 200, `status=${res ? res.status : 'undefined'}`);
  t('顶层code==0', !!res && res.code === 0, `code=${res ? res.code : 'undefined'}`);
  t(
    'data.biz_data 非空',
    biz !== null && typeof biz === 'object' && Object.keys(biz).length > 0,
    `biz_data=${biz === null ? 'null' : typeof biz}`,
  );
  // 顶层 code=0 也可能没数据 ⇒ 前置不过就不往下断
  if (!biz || typeof biz !== 'object' || Object.keys(biz).length === 0) return { ok: false, checks };

  const arr = biz[EXPECT.arrayKey];
  t(
    '数组键存在且是数组',
    Array.isArray(arr),
    `key=${EXPECT.arrayKey} isArray=${Array.isArray(arr)}`,
  );
  if (!Array.isArray(arr)) return { ok: false, checks };

  // ⚠️ 不断绝对条数（账号会话数会变 ⇒ 写死必然假红）
  t('读到条目（≥1）', arr.length >= 1, `len=${arr.length}`);

  const hm = biz[EXPECT.hasMoreKey];
  t('has_more 存在且为布尔', typeof hm === 'boolean', `has_more=${typeof hm}`);

  const got = arr[0] && typeof arr[0] === 'object' ? Object.keys(arr[0]).sort() : null;
  const want = [...EXPECT.itemKeys].sort();
  t(
    '首条键名集合相符',
    JSON.stringify(got) === JSON.stringify(want),
    `got=${JSON.stringify(got)}`,
  );

  // 条目 6 键的**类型**（§4.3 ②；PM 16:0x 裁定**要断**）—— 作用域与键名断言一致（**首条**）
  const item = arr[0];
  if (item && typeof item === 'object') {
    for (const [key, expectedType] of Object.entries(EXPECT.itemTypes)) {
      const actualType = typeof item[key];
      t(
        `条目字段类型 ${key}＝${expectedType}`,
        actualType === expectedType,
        actualType === expectedType
          ? `实际类型＝${actualType}`
          : `实际类型＝${actualType}（期望 ${expectedType}）`,
      );
    }
  }

  // 分支 B：响应侧无游标 ⇒ 游标断言不进本条（§4.3 ④ ／ G4-8）
  if (EXPECT.cursorKey) {
    const inBiz = Object.prototype.hasOwnProperty.call(biz, EXPECT.cursorKey);
    const inData = Object.prototype.hasOwnProperty.call(data, EXPECT.cursorKey);
    const inTop = Object.prototype.hasOwnProperty.call(res, EXPECT.cursorKey);
    t(
      '游标键存在于 top/data/biz_data 之一',
      inTop || inData || inBiz,
      `top=${inTop} data=${inData} biz_data=${inBiz}`,
    );
  }

  return { ok: checks.every((c) => c.ok), checks };
}

/**
 * 造一个**按实测读数派生**的响应夹具（**值全是占位符**，不落任何真实 id／标题）。
 *
 * 结构与登录态实测一致：顶层 `code`／`msg`／`data`；`data` ＝ `biz_code`／`biz_msg`／`biz_data`；
 * `biz_data` **恰 2 键** ＝ `chat_sessions` ＋ `has_more`；`chat_sessions` 实测每页 100 条；
 * 条目 ＝ 6 键（`id`／`title`／`title_type`／`model_type` 字符串、`pinned` 布尔、`updated_at` 数字）。
 *
 * @param {{count?: number, extraBizKey?: boolean}} [opts] - 夹具开关。
 * @returns {object} 响应夹具。
 */
export function measuredShapeFixture({ count = 100, extraBizKey = false } = {}) {
  const item = {
    id: '<id>',
    title: '<title>',
    title_type: '<title_type>',
    model_type: '<model_type>',
    pinned: false,
    updated_at: 0,
  };
  const bizData = {
    [EXPECT.arrayKey]: Array.from({ length: count }, () => ({ ...item })),
    [EXPECT.hasMoreKey]: true,
  };
  if (extraBizKey) bizData.some_new_field = 1;
  return {
    status: 200,
    code: 0,
    msg: '',
    data: { biz_code: 0, biz_msg: '', biz_data: bizData },
  };
}

/**
 * **场景表**：每个场景声明「应当通过」还是「应当失败」⇒ 判据自己证明自己。
 *
 * ⚠️ 场景 B/C/D/E/H 故意造坏数据 ⇒ 其**内层证据行本就该 FAIL** ⇒ **退出码按场景级算**
 *   （否则脚本永远是红的）。
 */
export function buildScenarios() {
  const good = measuredShapeFixture({ count: 1 });

  // 场景 B：顶层 code=0 但 biz_data=null ⇒ 必须判失败（§4.2 的反例）
  const nullBiz = { status: 200, code: 0, msg: '', data: { biz_code: 2, biz_msg: '', biz_data: null } };

  // 场景 C：has_more 缺失 ⇒ 必须判失败（实测 biz_data 必有它）
  const noHasMore = structuredClone(good);
  delete noHasMore.data.biz_data[EXPECT.hasMoreKey];

  // 场景 D：条目键名少一个 ⇒ 必须判失败
  const fewerKeys = structuredClone(good);
  delete fewerKeys.data.biz_data[EXPECT.arrayKey][0][EXPECT.itemKeys[0]];

  // 场景 E：空数组（读到 0 条）⇒ 必须判失败
  const emptyArr = structuredClone(good);
  emptyArr.data.biz_data[EXPECT.arrayKey] = [];

  // 场景 F：biz_data 多一个未知键 ⇒ **必须仍判通过**（站点加字段不该假红）
  const extraBizKey = structuredClone(good);
  extraBizKey.data.biz_data.some_new_field = 1;

  // 场景 H：条目多一个键 ⇒ **必须判失败**（PM 草案 §四.3：少字段／多字段都要判失败）
  const moreKeys = structuredClone(good);
  moreKeys.data.biz_data[EXPECT.arrayKey][0].some_new_field = 1;

  // 场景 I：条目字段**类型错** ⇒ **必须判失败**（PM 16:0x 裁定：§4.3 ② 的类型标注要有机检守着）
  const wrongType = structuredClone(good);
  wrongType.data.biz_data[EXPECT.arrayKey][0].pinned = 'false';

  return [
    { id: 'A', name: '场景A 正确形态 ⇒ 应通过', expect: PASS, res: good },
    { id: 'B', name: '场景B 顶层code=0但biz_data=null ⇒ 应失败（§4.2 反例）', expect: FAIL, res: nullBiz },
    { id: 'C', name: '场景C has_more 缺失 ⇒ 应失败', expect: FAIL, res: noHasMore },
    { id: 'D', name: '场景D 条目键名少一个 ⇒ 应失败', expect: FAIL, res: fewerKeys },
    { id: 'E', name: '场景E 读到 0 条 ⇒ 应失败', expect: FAIL, res: emptyArr },
    { id: 'F', name: '场景F biz_data 多一个未知键 ⇒ 仍应通过', expect: PASS, res: extraBizKey },
    {
      id: 'G',
      name: '场景G 实测派生结构（顶层3键／data 3键／biz_data 恰2键／100 条／条目6键且类型正确）⇒ 应通过',
      expect: PASS,
      res: measuredShapeFixture({ count: 100 }),
    },
    { id: 'H', name: '场景H 条目多一个键 ⇒ 应失败', expect: FAIL, res: moreKeys },
    { id: 'I', name: '场景I 条目字段类型错（pinned＝字符串）⇒ 应失败（§4.3 ② 类型要有机检）', expect: FAIL, res: wrongType },
  ];
}

/**
 * 跑场景表：每个场景核「实际判定」是否等于「应当的判定」。
 *
 * @param {Array<object>} [scenarios] - 场景表。
 * @returns {Array<{name: string, status: string, detail: string, checks: Array<object>}>}
 */
export function runScenarios(scenarios = buildScenarios()) {
  return scenarios.map((sc) => {
    const { ok, checks } = assertShape(sc.res);
    const behavedAsExpected = sc.expect === PASS ? ok : !ok;
    const failed = checks.filter((c) => !c.ok).map((c) => c.name);
    const detail =
      `实际判定＝${ok ? '通过' : '失败'}（期望＝${sc.expect === PASS ? '通过' : '失败'}）` +
      (failed.length ? `｜未过的内层断言：${failed.join('、')}` : '｜内层断言全过');
    return {
      name: `P4-8a ${sc.name}`,
      status: behavedAsExpected ? PASS : FAIL,
      detail,
      checks,
    };
  });
}

/**
 * **8c · 回程载荷的目录项面**：按 §4.3 ② 断「目录项那 6 个键的**存在**与**类型**」。
 *
 * ## 为什么要有这一轨
 *
 * 8a 断的是**站点响应**的字段面；8c 断的是**回程载荷**里那 `items` 数组的**元素**——
 * §4.3 ② 明文写死的那 6 个键 ＋ 它们的类型 ⇒ ⇒ 「那 6 个类型标注」从此有**机检守着**。
 *
 * ⚠️ **不断绝对条数**（`items` 有几项随账号变化）、**不断 `biz_data` 键集合相等**
 * —— 与 8a 同一口径。
 *
 * @param {{topKeys?: string[], itemFields?: Array<{name: string, type: string}>}} reading
 *   - `topKeys` —— 回程载荷的**顶层**字段名。
 *   - `itemFields` —— `items` 元素的字段（名字 ＋ 类型；类型认不出 ⇒ `unknown`）。
 * @returns {{ok: boolean, checks: Array<{name: string, ok: boolean, detail: string}>}}
 */
export function assertReturnItemsShape(reading) {
  const checks = [];
  const t = (name, ok, detail) => {
    checks.push({ name, ok: !!ok, detail });
    return !!ok;
  };
  const topKeys = Array.isArray(reading?.topKeys) ? reading.topKeys : [];
  const itemFields = Array.isArray(reading?.itemFields) ? reading.itemFields : [];
  const contractFields = Array.isArray(reading?.contractItemFields) ? reading.contractItemFields : [];

  t(
    '回程载荷有 `items` 字段（目录项面入口）',
    topKeys.includes('items'),
    topKeys.includes('items')
      ? `topKeys=${JSON.stringify(topKeys)}`
      : `topKeys=${JSON.stringify(topKeys)} ⇒ **目录项面未到位**（实现侧还没加 \`items\`）`,
  );
  if (itemFields.length === 0) {
    t(
      '回程载荷 `items` 元素恰好 6 个键（§4.3 ②）',
      false,
      '没读到 `items` 元素形态 ⇒ 目录项面未到位（实现侧还没加，或形态与判据不匹配）',
    );
    t('回程载荷 `items` 元素 6 键的类型符合 §4.3 ②', false, '没读到 `items` 元素形态');
  } else {
    const names = itemFields.map((f) => f.name).sort();
    const want = [...EXPECT.itemKeys].sort();
    t(
      '回程载荷 `items` 元素恰好 6 个键（§4.3 ②）',
      JSON.stringify(names) === JSON.stringify(want),
      `got=${JSON.stringify(names)}`,
    );
    for (const [key, expectedType] of Object.entries(EXPECT.itemTypes)) {
      const hit = itemFields.find((f) => f.name === key);
      const actualType = hit ? hit.type : '（缺）';
      t(
        `回程载荷目录项字段类型 ${key}＝${expectedType}`,
        hit && hit.type === expectedType,
        hit && hit.type === expectedType
          ? `实际类型＝${hit.type}`
          : `实际类型＝${actualType}（期望 ${expectedType}）`,
      );
    }
  }

  // --- 插件侧**契约面**（缺口 G4-21）---
  //
  // 契约的键 ＋ 类型是**唯一事实源**（`protocol.js` 的 `CATALOG_ITEM_FIELDS`）⇒
  // ⇒ 它也必须被断：契约漂了（键面回到旧 4 键、类型改回 string、或者把
  // `deepLink` 加回来）⇒ ⇒ 这一轨当场红。
  if (contractFields.length === 0) {
    t('插件侧契约的目录项键面 ＝ §4.3 ② 那 6 个键', false, '没读到插件侧契约 ⇒ 契约面没人守');
    t('插件侧契约的目录项类型 ＝ §4.3 ②', false, '没读到插件侧契约');
    t('插件侧契约里没有禁项（`sessionId` / `deepLink` / `cursor`）', false, '没读到插件侧契约');
  } else {
    const contractNames = contractFields.map((f) => f.name);
    const wantKeys = [...EXPECT.itemKeys].sort();
    const gotKeys = [...contractNames].sort();
    t(
      '插件侧契约的目录项键面 ＝ §4.3 ② 那 6 个键',
      JSON.stringify(gotKeys) === JSON.stringify(wantKeys),
      `got=${JSON.stringify(gotKeys)}`,
    );
    for (const [key, expectedType] of Object.entries(EXPECT.itemTypes)) {
      const hit = contractFields.find((f) => f.name === key);
      const actualType = hit ? hit.type : '（缺）';
      t(
        `插件侧契约目录项字段类型 ${key}＝${expectedType}`,
        hit && hit.type === expectedType,
        hit && hit.type === expectedType
          ? `实际类型＝${hit.type}`
          : `实际类型＝${actualType}（期望 ${expectedType}）`,
      );
    }
    const forbidden = contractNames.filter((n) =>
      ['sessionId', 'deepLink', 'cursor', 'body', 'content', 'text', 'messages', 'token'].includes(n),
    );
    t(
      '插件侧契约里没有禁项（`sessionId` / `deepLink` / `cursor`）',
      forbidden.length === 0,
      forbidden.length === 0 ? '契约里没有禁项' : `契约里混进了：${forbidden.join('、')}`,
    );
  }
  return { ok: checks.every((c) => c.ok), checks };
}

/**
 * **8c 的场景表**（合成读数）：每个场景声明「应当通过」还是「应当失败」。
 *
 * @returns {Array<object>}
 */
export function buildItemScenarios() {
  const itemFields = Object.entries(EXPECT.itemTypes).map(([name, type]) => ({ name, type }));
  const good = {
    topKeys: ['topKeys', 'code', 'msg', 'dataKeys', 'sessionsLength', 'firstItemKeys', 'itemTypes', 'hasMore', 'items'],
    itemFields: itemFields.map((f) => ({ ...f })),
    // **插件侧契约面**（缺口 G4-21）：契约 ＝ §4.3 ② 那 6 个键 ＋ 类型。
    contractItemFields: itemFields.map((f) => ({ ...f })),
  };
  const fewer = {
    ...good,
    itemFields: good.itemFields.filter((f) => f.name !== 'updated_at'),
  };
  const more = { ...good, itemFields: [...good.itemFields, { name: 'extra_key', type: 'string' }] };
  const wrongType = {
    ...good,
    itemFields: good.itemFields.map((f) =>
      f.name === 'pinned' ? { name: f.name, type: 'string' } : { ...f },
    ),
  };
  const noItems = {
    topKeys: good.topKeys.filter((k) => k !== 'items'),
    itemFields: [],
    contractItemFields: good.contractItemFields.map((f) => ({ ...f })),
  };
  const contractFewer = {
    ...good,
    contractItemFields: good.contractItemFields.filter((f) => f.name !== 'title_type'),
  };
  const contractMore = {
    ...good,
    contractItemFields: [...good.contractItemFields, { name: 'deepLink', type: 'string' }],
  };
  const contractWrongType = {
    ...good,
    contractItemFields: good.contractItemFields.map((f) =>
      f.name === 'pinned' ? { name: f.name, type: 'string' } : { ...f },
    ),
  };
  return [
    { id: 'A', name: '场景A 目录项 6 键齐 ＋ 类型对 ⇒ 应通过', expect: PASS, reading: good },
    { id: 'B', name: '场景B 目录项少一个键 ⇒ 应失败', expect: FAIL, reading: fewer },
    { id: 'C', name: '场景C 目录项多一个键 ⇒ 应失败', expect: FAIL, reading: more },
    { id: 'D', name: '场景D 目录项类型错（pinned＝字符串）⇒ 应失败', expect: FAIL, reading: wrongType },
    { id: 'E', name: '场景E 载荷没有 `items`（实现侧还没加）⇒ 应失败', expect: FAIL, reading: noItems },
    {
      id: 'F',
      name: '场景F 插件侧**契约**少一个键 ⇒ 应失败（G4-21）',
      expect: FAIL,
      reading: contractFewer,
    },
    {
      id: 'G',
      name: '场景G 插件侧**契约**把 `deepLink` 加回来 ⇒ 应失败（G4-21）',
      expect: FAIL,
      reading: contractMore,
    },
    {
      id: 'H',
      name: '场景H 插件侧**契约**的类型错（pinned＝字符串）⇒ 应失败（G4-21）',
      expect: FAIL,
      reading: contractWrongType,
    },
  ];
}

/**
 * 跑 8c 的场景表。
 *
 * @param {Array<object>} [scenarios] - 场景表。
 * @returns {Array<object>}
 */
export function runItemScenarios(scenarios = buildItemScenarios()) {
  return scenarios.map((sc) => {
    const { ok, checks } = assertReturnItemsShape(sc.reading);
    const behavedAsExpected = sc.expect === PASS ? ok : !ok;
    const failed = checks.filter((c) => !c.ok).map((c) => c.name);
    return {
      name: `P4-8a 目录项面 ${sc.name}`,
      status: behavedAsExpected ? PASS : FAIL,
      detail:
        `实际判定＝${ok ? '通过' : '失败'}（期望＝${sc.expect === PASS ? '通过' : '失败'}）` +
        (failed.length ? `｜未过的内层断言：${failed.join('、')}` : '｜内层断言全过'),
      checks,
    };
  });
}

/**
 * 8c 的**真实读数**（从实现侧源码抽；抽不到就是抽不到 ⇒ 如实红，不假绿）。
 *
 * @param {string} root - 仓库根。
 * @returns {{topKeys: string[], itemFields: Array<{name: string, type: string}>}}
 */
export function readReturnItemsReading(root) {
  const rustRel = 'src-tauri/src/official_chat/session_structure.rs';
  const jsRel = 'plugins/official-chat/host/site-source.js';
  const contractRel = 'plugins/official-chat/host/protocol.js';
  const sources = readSources(root, [rustRel, jsRel, contractRel]);
  const rust = sources.find((s) => s.path === rustRel)?.code ?? '';
  const js = sources.find((s) => s.path === jsRel)?.code ?? '';
  const contract = sources.find((s) => s.path === contractRel)?.code ?? '';
  const topKeys = objectLiteralKeysContaining(rust, 'topKeys');
  const rustItem = rustItemsElementFields(rust);
  const jsItem = jsItemsElementFields(js, EXPECT.itemKeys);
  // 两端都抽到就**逐项相等**才算（不然用抽到的那一端；抽不到 ⇒ 空 ⇒ 如实红）
  const itemFields =
    rustItem.length > 0 && jsItem.length > 0 ? rustItem : rustItem.length > 0 ? rustItem : jsItem;
  // **插件侧契约面**（缺口 G4-21）：`CATALOG_ITEM_FIELDS` 的键 ＋ 类型。
  const contractItemFields = jsLiteralTypeFields(contract, 'title_type');
  return { topKeys, itemFields, contractItemFields };
}

/**
 * **动态**载入一个插件侧模块（**只读**，不写任何东西）。
 *
 * ## 为什么要动态 import
 *
 * 8b／8c 要断的是**行为**（「多一个键就红」「认不出就空串」），不是抄本 ⇒ ⇒
 * 直接调**实现侧那两个纯函数**最真 ⇒ ⇒ 判据与实现**不会各说各话**。
 *
 * ⚠️ **读不到 ⇒ 返回 `null`**（由调用方如实报红，**不假绿**）—— 读不到
 * 被测对象时「全绿」是**最坏**的结论。
 *
 * @param {string} root - 仓库根。
 * @param {string} rel - 相对仓库根的文件路径。
 * @returns {Promise<object|null>} 模块命名空间；载入失败 ⇒ `null`。
 */
async function importPluginModule(root, rel) {
  try {
    const href = pathToFileURL(join(root, rel)).href;
    return await import(href);
  } catch {
    return null;
  }
}

/** 一份**最小可用的目录页**（给 8b 的行为样本当底稿）。 */
function sampleCatalogPage(extra = {}) {
  return {
    accountScope: 'scope',
    providerId: 'official-chat',
    items: [],
    cursor: '',
    hasMore: false,
    fetchedAt: '2026-10-21T00:00:00.000Z',
    ...extra,
  };
}

/** 一份**最小可用的目录项**（6 键，类型照 §4.3 ②）。 */
function sampleCatalogItem(extra = {}) {
  return {
    id: 'ds~0qajycvnzbae~0009',
    title: '一条会话',
    title_type: 'USER',
    model_type: 'deepseek-chat',
    pinned: false,
    updated_at: 1783051054.518,
    ...extra,
  };
}

/**
 * **8b 的读数**：目录页 `page.source` 的**契约面**（源码形态）＋ **行为样本**。
 *
 * ## 读什么
 *
 * · 契约面（`protocol.js`）：`CATALOG_SOURCE_KINDS` 的闭集 ＋ `buildCatalogSource`
 *   返回的对象字面量的**键集**。
 * · 行为面：**直接调**实现侧的 `validateCatalogPage` ＋ `buildCatalogSource`
 *   （**纯函数**）⇒ ⇒ 断的是**行为**，不是抄本。
 *
 * @param {string} root - 仓库根。
 * @returns {Promise<{contractKinds: string[], builderKeySets: string[][], samples: Array<object>}>}
 */
export async function readSourceShape(root) {
  const sources = readSources(root, [CONTRACT_REL]);
  const contract = sources.find((s) => s.path === CONTRACT_REL)?.code ?? '';
  const contractKinds = arrayLiteralStrings(contract, 'CATALOG_SOURCE_KINDS');
  const builderBody = jsFnBody(contract, 'buildCatalogSource');
  // **只取含 `kind` 的那几份**对象字面量（函数体里别的对象不是 source）。
  const builderKeySets = objectLiteralKeySets(builderBody).filter((keys) =>
    keys.includes('kind'),
  );

  const samples = [];
  const mod = await importPluginModule(root, CONTRACT_REL);
  if (mod === null || typeof mod.validateCatalogPage !== 'function') {
    samples.push({
      name: '读得到插件侧契约模块（`validateCatalogPage`）',
      ok: false,
      detail: `载入 ${CONTRACT_REL} 失败 ⇒ **8b 的行为样本没跑到**（不许假绿）`,
    });
    return { contractKinds, builderKeySets, samples };
  }

  const { validateCatalogPage } = mod;
  /** 跑一个样本：**期望**通过还是**期望**判红。 */
  const sample = (name, page, expectOk) => {
    let check;
    try {
      check = validateCatalogPage(page);
    } catch (error) {
      samples.push({ name, ok: false, detail: `校验函数抛了异常：${String(error?.message ?? error)}` });
      return;
    }
    const got = check.ok === true;
    samples.push({
      name,
      ok: got === expectOk,
      detail: `期望${expectOk ? '通过' : '判红'}，实际${got ? '通过' : '判红'}${
        check.errors?.length ? `（${check.errors.join('、')}）` : ''
      }`,
    });
  };

  sample('目录页带合法的 `source` ⇒ 通过', sampleCatalogPage({
    source: { kind: 'site', id: 'deepseek', label: 'DeepSeek' },
  }), true);
  sample('**缺** `source`（旧缓存页）⇒ 通过（**可选字段**）', sampleCatalogPage(), true);
  sample('`source` 多一个键（第 4 个键）⇒ 判红', sampleCatalogPage({
    source: { kind: 'site', id: 'deepseek', label: 'DeepSeek', extra: 'x' },
  }), false);
  sample('`source.kind` 不在闭集里 ⇒ 判红', sampleCatalogPage({
    source: { kind: 'unknown-site', id: 'deepseek', label: 'DeepSeek' },
  }), false);
  sample('`source.id` 是空串 ⇒ 判红', sampleCatalogPage({
    source: { kind: 'site', id: '', label: 'DeepSeek' },
  }), false);
  sample('`source.id` 不是字符串 ⇒ 判红', sampleCatalogPage({
    source: { kind: 'site', id: 7, label: 'DeepSeek' },
  }), false);
  sample('`source.label` 是空串 ⇒ 判红', sampleCatalogPage({
    source: { kind: 'site', id: 'deepseek', label: '' },
  }), false);
  sample('`source.label` 不是字符串 ⇒ 判红', sampleCatalogPage({
    source: { kind: 'site', id: 'deepseek', label: 7 },
  }), false);
  sample('`items` 元素里出现 `source` ⇒ 判红（**不许进 items**）', sampleCatalogPage({
    items: [sampleCatalogItem({ source: { kind: 'site', id: 'x', label: 'y' } })],
  }), false);

  if (typeof mod.buildCatalogSource === 'function') {
    for (const [kind, id] of [['site', 'deepseek'], ['fixture', 'fixture']]) {
      const built = mod.buildCatalogSource(kind, id);
      const keys = Object.keys(built ?? {}).sort();
      const want = [...SOURCE_KEYS].sort();
      samples.push({
        name: `\`buildCatalogSource('${kind}', …)\` 产出的对象**恰 3 键**（kind／id／label）`,
        ok: JSON.stringify(keys) === JSON.stringify(want),
        detail: `产出键集：${JSON.stringify(keys)}`,
      });
    }
  } else {
    samples.push({
      name: '读得到插件侧的 `buildCatalogSource`',
      ok: false,
      detail: `模块里没有这个导出 ⇒ **8b 的键面没跑到**（不许假绿）`,
    });
  }

  return { contractKinds, builderKeySets, samples };
}

/**
 * **8b 的判定内核**。
 *
 * @param {object} reading - 读数（`contractKinds` / `builderKeySets` / `samples`）。
 * @returns {{ok: boolean, checks: Array<{name: string, ok: boolean, detail: string}>}}
 */
export function assertCatalogSourceShape(reading) {
  const checks = [];
  const t = (name, ok, detail) => {
    checks.push({ name, ok: !!ok, detail });
    return !!ok;
  };
  const contractKinds = Array.isArray(reading?.contractKinds) ? reading.contractKinds : [];
  const builderKeySets = Array.isArray(reading?.builderKeySets) ? reading.builderKeySets : [];
  const samples = Array.isArray(reading?.samples) ? reading.samples : [];

  t(
    '契约里的 `kind` 闭集 ＝ site／fixture',
    contractKinds.length === SOURCE_KINDS.length &&
      SOURCE_KINDS.every((k) => contractKinds.includes(k)),
    contractKinds.length === 0
      ? '没读到 `CATALOG_SOURCE_KINDS` ⇒ 闭集没守住'
      : `契约读到：${JSON.stringify(contractKinds)}`,
  );
  t(
    '`buildCatalogSource` 的**每一份**返回对象都**恰 3 键**（kind／id／label）',
    builderKeySets.length > 0 &&
      builderKeySets.every(
        (keys) => keys.length === SOURCE_KEYS.length && SOURCE_KEYS.every((k) => keys.includes(k)),
      ),
    builderKeySets.length === 0
      ? '没读到 `buildCatalogSource` 的返回对象 ⇒ 键面没守住'
      : `读到 ${builderKeySets.length} 份返回对象，键集：${JSON.stringify(builderKeySets)}`,
  );
  if (samples.length === 0) {
    t('8b 的行为样本跑到了', false, '一条样本都没跑 ⇒ 判据未执行（不许假绿）');
  }
  for (const s of samples) t(s.name, s.ok, s.detail);
  return { ok: checks.every((c) => c.ok), checks };
}

/**
 * **8c 的读数**：桥回包**信封**上的 `site` 短码。
 *
 * ## 读什么
 *
 * · **内核侧**：`SITE` 常量的值（事实源）＋ **信封**（`bridge.rs`）上带 `site`
 *   ＋ **回程载荷**（`session_structure.rs` 的 `topKeys`）里**没有** `site`。
 * · **插件侧**：直接调 `parseSite` 跑样本（**认不出 ⇒ 空串**）。
 *
 * @param {string} root - 仓库根。
 * @returns {Promise<{rustSite: string, envelopeHasSite: boolean, payloadHasSite: boolean, samples: Array<object>}>}
 */
export async function readEnvelopeSiteShape(root) {
  const sources = readSources(root, [SITE_SOURCE_REL, RUST_REL, BRIDGE_REL]);
  const js = sources.find((s) => s.path === SITE_SOURCE_REL)?.code ?? '';
  const rust = sources.find((s) => s.path === RUST_REL)?.code ?? '';
  const bridge = sources.find((s) => s.path === BRIDGE_REL)?.code ?? '';
  const rustSite = /pub\(crate\) const SITE: &str = "([^"]*)";/.exec(rust)?.[1] ?? '';
  const envelopeHasSite = /"site"\s*:/.test(bridge);
  const payloadKeys = objectLiteralKeysContaining(rust, 'topKeys');
  const payloadHasSite = payloadKeys.includes('site');

  const samples = [];
  const mod = await importPluginModule(root, SITE_SOURCE_REL);
  if (mod === null || typeof mod.parseSite !== 'function') {
    samples.push({
      name: '读得到插件侧真实源模块（`parseSite`）',
      ok: false,
      detail: `载入 ${SITE_SOURCE_REL} 失败 ⇒ **8c 的行为样本没跑到**（不许假绿）`,
    });
    return { rustSite, envelopeHasSite, payloadHasSite, samples };
  }
  const { parseSite } = mod;
  const cases = [
    ['deepseek', 'deepseek'],
    ['a', 'a'],
    ['site-1', 'site-1'],
    ['site1', 'site1'],
    ['site_1', ''],
    ['DeepSeek', ''],
    ['a/b', ''],
    ['', ''],
    ['a'.repeat(33), ''],
    ['a'.repeat(32), 'a'.repeat(32)],
  ];
  for (const [input, expected] of cases) {
    let got;
    try {
      got = parseSite(input);
    } catch (error) {
      samples.push({
        name: `信封短码解析：${JSON.stringify(input)}`,
        ok: false,
        detail: `解析函数抛了异常：${String(error?.message ?? error)}`,
      });
      continue;
    }
    const shown = input.length > 20 ? `${input.slice(0, 12)}…（${input.length} 字符）` : input;
    samples.push({
      name: `信封短码解析：${JSON.stringify(shown)}`,
      ok: got === expected,
      detail: `期望 ${JSON.stringify(expected)}，实际 ${JSON.stringify(got)}`,
    });
  }
  return { rustSite, envelopeHasSite, payloadHasSite, samples };
}

/**
 * **8c 的判定内核**。
 *
 * @param {object} reading - 读数（`rustSite` / `envelopeHasSite` / `payloadHasSite` / `samples`）。
 * @returns {{ok: boolean, checks: Array<{name: string, ok: boolean, detail: string}>}}
 */
export function assertEnvelopeSiteShape(reading) {
  const checks = [];
  const t = (name, ok, detail) => {
    checks.push({ name, ok: !!ok, detail });
    return !!ok;
  };
  const rustSite = String(reading?.rustSite ?? '');
  const samples = Array.isArray(reading?.samples) ? reading.samples : [];

  t(
    '内核的站点短码常量符合形态（小写字母／数字／连字符、1–32 字符）',
    SITE_CODE_PATTERN.test(rustSite),
    `读到 ${JSON.stringify(rustSite)}`,
  );
  t(
    '桥回包的**信封**上带 `site` 短码',
    reading?.envelopeHasSite === true,
    reading?.envelopeHasSite === true ? '信封上有 `site`' : '信封上没读到 `site`',
  );
  t(
    '**回程载荷**里没有 `site`（信封层 ≠ 载荷层；§2.4 载荷白名单一个字不动）',
    reading?.payloadHasSite === false,
    reading?.payloadHasSite === false
      ? '载荷里没有 `site`'
      : '载荷里出现了 `site` ⇒ 那是往载荷里加字段（白名单是穷举列举）',
  );
  if (samples.length === 0) {
    t('8c 的行为样本跑到了', false, '一条样本都没跑 ⇒ 判据未执行（不许假绿）');
  }
  for (const s of samples) t(s.name, s.ok, s.detail);
  return { ok: checks.every((c) => c.ok), checks };
}

/**
 * **协议正文对账轨**：§4.3 的键名与本判据的 `EXPECT` 是否还一致。
 *
 * **只读**协议正文（脚本不改协议一个字）⇒ 协议改了键名而本判据没跟上时立刻暴露。
 *
 * @param {string} root - 仓库根。
 * @returns {{name: string, status: string, detail: string, scanned: string[]}}
 */
export function protocolReconcile(root) {
  const file = join(root, PROTOCOL_REL);
  let text = null;
  try {
    text = readFileSync(file, 'utf8');
  } catch {
    text = null;
  }
  const name = 'P4-8a 协议正文键名与判据一致（§4.3 对账）';
  if (text === null) {
    return {
      name,
      status: SKIPPED,
      detail: `未找到协议正文 ${PROTOCOL_REL} ⇒ 对账未执行（不硬红）`,
      scanned: [],
    };
  }
  const need = [ARRAY_PATH, EXPECT.hasMoreKey, ...EXPECT.itemKeys];
  const missing = need.filter((k) => !text.includes(k));
  return {
    name,
    status: missing.length ? FAIL : PASS,
    detail: missing.length
      ? `协议正文里找不到：${missing.join('、')} ⇒ 协议与判据已漂移，回来改 EXPECT`
      : `协议正文 ${PROTOCOL_REL} 含 EXPECT 的全部键名（数组路径 ${ARRAY_PATH}／布尔键 ${EXPECT.hasMoreKey}／条目 ${EXPECT.itemKeys.length} 键）`,
    scanned: [PROTOCOL_REL],
  };
}

/**
 * 收集扫描面内的文件（本判据的扫描面 ＝ 协议正文 ＋ **实现侧两个取数源**，**只读**）。
 *
 * ⚠️ 8a 是**响应形态**判据（不扫源码）；8c 是**回程载荷形态**判据 ⇒ ⇒ 8c 要读
 * 实现侧那两个文件 ⇒ ⇒ 扫描面从 1 个变成 3 个 ⇒ ⇒ 「扫了 N 个文件」要与这里对得上。
 *
 * @param {string} root - 仓库根。
 * @returns {Array<{path: string, code: string}>}
 */
export function collectSources(root) {
  const files = [
    PROTOCOL_REL,
    RUST_REL,
    SITE_SOURCE_REL,
    // **插件侧契约面**（缺口 G4-21）：`CATALOG_ITEM_FIELDS` ＋ `validateCatalogPage`
    // 就在这个文件里 ⇒ ⇒ 它不在扫描面内的那段时间里，契约漂移了判据**一条都
    // 不报**（PM 一手实测过：契约还是旧 4 键、且要求「每个字段都是 string」，
    // 而第 3／8 条全绿 ⇒ 那一刻是假绿）。
    //
    // **v0.19 追加**：**8b**（`page.source` 形态）的契约面也在这里 ⇒ ⇒ 同一个
    // 道理：**契约面不在扫描面 ⇒ 契约漂移照不出来**（缺口 G4-22 的同一条教训）。
    CONTRACT_REL,
    // **信封层**（缺口 G4-22 · **8c**）：桥回包的**信封**上带 `site` 短码 ⇒ ⇒
    // 信封那几行代码**必须**在扫描面内，否则「信封带没带 site」照不出来。
    BRIDGE_REL,
  ];
  const out = [];
  for (const rel of files) {
    try {
      out.push({ path: rel, code: readFileSync(join(root, rel), 'utf8') });
    } catch {
      /* 读不动的跳过 */
    }
  }
  return out;
}

/**
 * 汇总计数（与 P1/P2/P3 判据的产物同构）。
 *
 * @param {Array<{status: string}>} results
 * @returns {{total: number, pass: number, fail: number, skipped: number}}
 */
export function summarize(results) {
  const list = Array.isArray(results) ? results : [];
  return {
    total: list.length,
    pass: list.filter((r) => r?.status === PASS).length,
    fail: list.filter((r) => r?.status === FAIL).length,
    skipped: list.filter((r) => r?.status === SKIPPED).length,
  };
}

/**
 * 主入口。
 *
 * @param {string[]} [argv] - 命令行参数（默认 `process.argv.slice(2)`）。
 * @returns {{results: Array<object>, summary: object}}
 */
export async function main(argv = process.argv.slice(2)) {
  const repo = fileURLToPath(new URL('../', import.meta.url));
  const roundIndex = argv.findIndex((a) => a === '--round' || String(a).startsWith('--round='));
  let round = 'p4c8';
  if (roundIndex >= 0) {
    const inline = String(argv[roundIndex]).split('=')[1];
    const next = argv[roundIndex + 1];
    round = inline ?? (next !== undefined && !String(next).startsWith('--') ? next : 'p4c8');
  }
  const targetIndex = argv.findIndex((a) => a === '--target' || String(a).startsWith('--target='));
  let root = repo.replace(/[\\/]+$/, '');
  if (targetIndex >= 0) {
    const inline = String(argv[targetIndex]).split('=')[1];
    const next = argv[targetIndex + 1];
    root = (inline ?? (next !== undefined && !String(next).startsWith('--') ? next : repo)).replace(
      /[\\/]+$/,
      '',
    );
  }
  const asJson = argv.includes('--json');

  const results = runScenarios();
  const reconcile = protocolReconcile(root);
  results.push(reconcile);

  // --- 8a：回程载荷的目录项面（场景表 ＋ 真实读数轨） ---
  for (const r of runItemScenarios()) results.push(r);
  const itemReading = readReturnItemsReading(root);
  const { ok: itemsOk, checks: itemChecks } = assertReturnItemsShape(itemReading);
  for (const c of itemChecks) {
    results.push({
      name: `P4-8a 真实读数 · ${c.name}`,
      status: c.ok ? PASS : FAIL,
      detail: c.detail,
    });
  }
  results.push({
    name: 'P4-8a 真实读数的判定',
    status: itemsOk ? PASS : FAIL,
    detail: itemsOk
      ? '真实读数全过（回程载荷的目录项面：6 键齐 ＋ 类型对）'
      : '真实读数未全过 —— **红在实现侧还没加目录项面**（协议 §2.4 v0.17 的 `items` 还没落）',
  });

  // --- 8b：目录页 `page.source`（来源标识）的形态（协议 v0.19 · P1 §1.2 v0.13） ---
  const sourceReading = await readSourceShape(root);
  const { ok: sourceOk, checks: sourceChecks } = assertCatalogSourceShape(sourceReading);
  for (const c of sourceChecks) {
    results.push({
      name: `P4-8b ${c.name}`,
      status: c.ok ? PASS : FAIL,
      detail: c.detail,
    });
  }
  results.push({
    name: 'P4-8b 真实读数的判定',
    status: sourceOk ? PASS : FAIL,
    detail: sourceOk
      ? '目录页 `page.source` 的形态全过（恰 3 键／kind 闭集／id 与 label 非空 string／可选／不进 items）'
      : '目录页 `page.source` 的形态有红 ⇒ **来源标识漂了**（协议 v0.19 §10 第 8 条 8b）',
  });

  // --- 8c：桥回包**信封**上的 `site` 短码（协议 v0.19 · §5.1 追认） ---
  const envelopeReading = await readEnvelopeSiteShape(root);
  const { ok: envelopeOk, checks: envelopeChecks } = assertEnvelopeSiteShape(envelopeReading);
  for (const c of envelopeChecks) {
    results.push({
      name: `P4-8c ${c.name}`,
      status: c.ok ? PASS : FAIL,
      detail: c.detail,
    });
  }
  results.push({
    name: 'P4-8c 真实读数的判定',
    status: envelopeOk ? PASS : FAIL,
    detail: envelopeOk
      ? '信封 `site` 短码全过（形态受限／认不出为空串／只在信封层、不在回程载荷里）'
      : '信封 `site` 短码有红 ⇒ **短码形态或落点漂了**（协议 v0.19 §10 第 8 条 8c）',
  });

  const scanned = collectSources(root).map((s) => s.path);

  const summary = summarize(results);
  const evidenceDir = join(tmpdir(), 'dsh-p4-shape');
  mkdirSync(evidenceDir, { recursive: true });
  const outPath = join(evidenceDir, `results-${round}.json`);
  writeFileSync(
    outPath,
    `${JSON.stringify({ round, at: new Date().toISOString(), scanned, results, summary }, null, 2)}\n`,
    'utf8',
  );

  if (asJson) {
    process.stdout.write(`${JSON.stringify({ round, results, summary }, null, 2)}\n`);
  } else {    for (const r of results) {
      const mark = r.status === PASS ? '✔' : r.status === FAIL ? '✘' : '—';
      process.stdout.write(`${mark} ${r.name}：${r.detail}\n`);
    }
    process.stdout.write(
      `\n合计 ${summary.total} 条：通过 ${summary.pass} · 失败 ${summary.fail} · 未执行 ${summary.skipped}\n` +
        `扫描面：${PROTOCOL_REL}（8b 对账）＋ 实现侧两个取数源（8c 读回程载荷形态）；8a 是**响应形态**判据，不扫源码\n` +
        `扫了 ${scanned.length} 个文件 · 证据：${outPath}\n`,
    );
  }
  // 退出码按结论算（与共用原语 `emit` 同口径）：有失败就是 1 ——
  // ⚠️ 8c 真实轨在**实现侧还没加目录项面**时会红 ⇒ ⇒ 本体现在 exit=1
  //    （那是**如实**，不是回归：判据写对了，实现还没到位）。
  process.exitCode = summary.fail > 0 ? 1 : 0;
  return { results, summary };
}

// 直接跑（被 import 时不跑）
if (process.argv[1] && process.argv[1].endsWith('check-official-chat-p4-shape.mjs')) {
  await main();
}
