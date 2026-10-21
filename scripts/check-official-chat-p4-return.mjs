#!/usr/bin/env node
// P4 协议 §10 判据**第 3 条**（回程只回结构）—— 按**回传字段清单**断（白名单之外即红）。
//
// ## 依据
//
// · 协议 §2.4（回程白名单，**v0.17 起是「两面」**）：回程载荷**分两层**——
//   **结构面**（顶层键名、`code`/`msg`、`data` 的键名数组、数组的长度、数组首元素的
//   键名数组、条目的类型/布尔量）＋ **目录项面**（载荷多一个 `items` 数组；**每项
//   恰好 6 个键**：`id`/`title`/`title_type`/`model_type` 字符串、`pinned` 布尔、
//   `updated_at` 数字 —— 协议 §4.3 ② 已写死并有实测出处）。
//   **不许回传**：**正文**、**凭据**、**站点的完整响应体**、**白名单外任何键**。
// · 协议 §8.5.1 末段：`Network` 域拿到的是**站点的完整响应体**（里面**含**
//   `title` / `id`）⇒ ⇒ **不许**把 `getResponseBody` 的原文直接回传
//   ⇒ ⇒ 白名单裁剪**在壳侧**做（协议 §1.1 的「资产不浪费条款」）。
// · 协议 §7.2：凭据零落盘、零外传。
//
// ## 为什么是「两面」而不是「一面」
//
// P1 §1.2 的目录页**必然**要「会话 id、标题、更新时间」⇒ ⇒ 一面白名单把 `title`/`id`
// 禁掉 ⇒ ⇒ 实现侧只能让目录页继续用固定夹具 ⇒ ⇒ **协议侧 v0.17 改成两面**：
// **键名与值**在**目录项内部**才允许（那 6 个键是**契约面**，实测写死），
// **顶层**仍然只回结构 ⇒ ⇒ **本判据按两层分别断**。
//
// ## 两份白名单（**穷举列举**，不许放宽成「差不多」）
//
// | 面 | 字段 | 依据 |
// | --- | --- | --- |
// | **结构面**（顶层） | `topKeys` / `code` / `msg` / `dataKeys` / `sessionsLength` / `firstItemKeys` / `itemTypes` / `hasMore` | §2.4「允许回传」逐字列举 |
// | **目录项面**（顶层） | `items` | §2.4 v0.17 的目录项面入口 |
// | **目录项面**（`items` 的每一项） | `id` / `title` / `title_type` / `model_type` / `pinned` / `updated_at` | §4.3 ②（键 **与** 类型都写死） |
//
// ⚠️ **本轮不回 `deepLink`、不回 `cursor`**（缺口 G4-18 / G4-8）⇒ ⇒ 两份白名单里
// **都不许**出现这两个键 ⇒ 出现了就是「实现编了未实测的字段」⇒ **红**。
// ⚠️ **`status`（HTTP 状态码）与 `bizCode`（业务层码）不在白名单里** ⇒ ⇒
// **不进回程载荷**：它们只在**壳侧**参与判断（§4.2 的成功判据要 `status == 200`；
// §6.2 的「两层都要看」要 `biz_code`）⇒ ⇒ 站内码要上报时走**失败 detail**
// （§6.2「原样上报」），那**不是结构回程**。
//
// ## 断的十三件事
//
// **结构面**（既有五条，一字不改其口径）：
// ① 壳侧回程载荷的字段名 **⊆ 白名单**（白名单之外即红）；
// ② 插件侧回包校验的字段名 **⊆ 白名单**（两端口径必须一致）；
// ③ 两端**结构面**字段清单**逐项相等**；
// ④ 白名单里**没有**禁回传的那几类（正文/标题/会话 id/凭据）；
// ⑤ 回程载荷**不是**站点的完整响应体（没有「整包带走」的字段）。
//
// **目录项面**（新增八条）：
// ⑥ 壳侧载荷**有** `items` 字段；
// ⑦ 插件侧回包**有** `items` 字段；
// ⑧ 壳侧 `items` 的**每一项恰好 6 个键**（＝ §4.3 ② 逐项）；
// ⑨ 插件侧 `items` 的**每一项恰好 6 个键**；
// ⑩ 两端**目录项面逐项相等**（键 ＋ 类型）；
// ⑪ 目录项 6 键的**类型**符合 §4.3 ②（`id`/`title`/`title_type`/`model_type`
// ＝ string、`pinned` ＝ boolean、`updated_at` ＝ number）；
// ⑫ `items` 的**每一项内部**没有禁项（正文/凭据类；`body`/`token`/`messages`/
// `content`/`text` …）—— 顶层那条断不到数组内部 ⇒ 这一条是**新断点**；
// ⑬ 白名单里**没有** `deepLink` / `cursor`（本轮不回 ⇒ 出现即红）。
//
// @module scripts/check-official-chat-p4-return

import { tmpdir } from 'node:os';
import { join } from 'node:path';

import {
  FAIL,
  PASS,
  P4_SCAN_FILES,
  SKIPPED,
  argValue,
  arrayLiteralStrings,
  emit,
  judgeChecks,
  jsItemsElementFields,
  jsLiteralFields,
  jsLiteralTypeFields,
  objectLiteralKeysContaining,
  readSources,
  repoRoot,
  runScenarios,
  rustItemsElementFields,
  rustItemsElementType,
  rustStructFields,
  rustTypeToJsonType,
} from './lib/p4-source-shape.mjs';

// 三态常量**从共用原语转出去**（自测文件从本模块取它们）。
export { FAIL, PASS, SKIPPED };

/** **结构面**白名单（协议 §2.4 逐字列举那几项；**顶层**）。 */
export const WHITELIST = Object.freeze([
  'topKeys',
  'code',
  'msg',
  'dataKeys',
  'sessionsLength',
  'firstItemKeys',
  'itemTypes',
  'hasMore',
]);

/** **目录项面**的顶层入口（载荷多一个 `items` 数组）。 */
export const ITEM_ENTRY_KEY = 'items';

/** **目录项面**的**元素键**（协议 §4.3 ②，6 个；顺序不敏感）。 */
export const ITEM_KEYS = Object.freeze([
  'id',
  'title',
  'title_type',
  'model_type',
  'pinned',
  'updated_at',
]);

/** **目录项面**的**元素类型**（协议 §4.3 ② 明文写死）。 */
export const ITEM_TYPES = Object.freeze({
  id: 'string',
  title: 'string',
  title_type: 'string',
  model_type: 'string',
  pinned: 'boolean',
  updated_at: 'number',
});

/** **顶层**白名单 ＝ 结构面 ∪ 目录项面入口。 */
export const TOP_WHITELIST = Object.freeze([...WHITELIST, ITEM_ENTRY_KEY]);

/**
 * **顶层**禁回传的字段名（协议 §2.4：正文／标题／会话 id／凭据 ＋ 本轮不回的
 * `deepLink` / `cursor`）。
 *
 * ⚠️ `title` / `id` 在**顶层**仍然禁 —— 它们只允许出现在 `items` 的**元素内部**
 * （§4.3 ② 的 6 键）⇒ ⇒ 顶层出现就是「值被提到了顶层」⇒ 红。
 */
export const FORBIDDEN = Object.freeze([
  'title',
  'id',
  'body',
  'content',
  'text',
  'messages',
  'sessionId',
  'deepLink',
  'cursor',
  'token',
  'cookie',
  'credential',
  'userToken',
]);

/**
 * **目录项元素内部**禁出现的字段名。
 *
 * ⚠️ 与 {@link FORBIDDEN} 的差别：`title` / `id` **不在**这里（它们是 §4.3 ②
 * 明文允许的 6 键之一）⇒ ⇒ 正文/凭据类禁项才是元素内部的禁项。
 */
export const FORBIDDEN_ITEM = Object.freeze([
  'body',
  'content',
  'text',
  'messages',
  'sessionId',
  'deepLink',
  'cursor',
  'token',
  'cookie',
  'credential',
  'userToken',
]);

/** 壳侧取数模块（相对仓库根）。 */
export const RUST_REL = 'src-tauri/src/official_chat/session_structure.rs';
/** 插件侧真实取数源（相对仓库根）。 */
export const JS_REL = 'plugins/official-chat/host/site-source.js';
/**
 * **插件侧契约文件**（相对仓库根）—— `CATALOG_ITEM_FIELDS` ＋ `validateCatalogPage`
 * 就在这里。
 *
 * ## 为什么它必须进扫描面（缺口 G4-21，PM 一手实测）
 *
 * 判据第 3／8 条原先只读**壳侧两个源**（`session_structure.rs` ＋
 * `site-source.js`）⇒ ⇒ **插件侧契约不在面内** ⇒ ⇒ 契约漂移了（键面回到旧
 * 4 键、或类型改回 string）**判据一条都不报** —— PM 在干净 HEAD 上实测过：
 * 契约还是旧 4 键、且 `validateCatalogPage` 要求「每个字段都是 string」，
 * 而判据第 3 条 32/32、第 8 条 24/24 **全绿** ⇒ ⇒ 那一刻是**假绿**。
 *
 * ⇒ ⇒ 契约的**键 ＋ 类型**从现在起也是**被断的**。
 */
export const CONTRACT_REL = 'plugins/official-chat/host/protocol.js';

/**
 * 判定内核。
 *
 * @param {object} reading - 读数（`rustKeys` / `jsKeys` / `rustItemFields` /
 *   `jsItemFields`；`whitelist` / `itemKeys` 供合成读数覆盖）。
 * @returns {{ok: boolean, checks: Array<object>}}
 */
export function judgeReturn(reading) {
  const checks = [];
  const add = (name, status, detail) => checks.push({ name, status, detail });
  const whitelist = Array.isArray(reading?.whitelist) ? reading.whitelist : TOP_WHITELIST;
  const itemKeys = Array.isArray(reading?.itemKeys) ? reading.itemKeys : ITEM_KEYS;
  const rustKeys = Array.isArray(reading?.rustKeys) ? reading.rustKeys : [];
  const jsKeys = Array.isArray(reading?.jsKeys) ? reading.jsKeys : [];
  const rustItem = Array.isArray(reading?.rustItemFields) ? reading.rustItemFields : [];
  const jsItem = Array.isArray(reading?.jsItemFields) ? reading.jsItemFields : [];

  if (rustKeys.length === 0 && jsKeys.length === 0) {
    add('P4-3 回程载荷的字段清单', SKIPPED, '没读到任何一侧的载荷 ⇒ 判据未执行');
    return judgeChecks(checks);
  }

  // --- 结构面 ---
  for (const [label, keys] of [
    ['壳侧', rustKeys],
    ['插件侧', jsKeys],
  ]) {
    if (keys.length === 0) {
      add(`P4-3 ${label}回程字段 ⊆ 白名单`, SKIPPED, `没读到${label}载荷 ⇒ 判据未执行`);
      continue;
    }
    const outside = keys.filter((k) => !whitelist.includes(k));
    add(
      `P4-3 ${label}回程字段 ⊆ 白名单（结构面 ∪ 目录项面）`,
      outside.length === 0 ? PASS : FAIL,
      outside.length === 0
        ? `${label}载荷 ${keys.length} 个字段全在白名单内`
        : `${label}载荷出现白名单之外的字段：${outside.join('、')}`,
    );
  }

  if (rustKeys.length > 0 && jsKeys.length > 0) {
    const structFace = (keys) => keys.filter((k) => k !== ITEM_ENTRY_KEY);
    const onlyRust = structFace(rustKeys).filter((k) => !structFace(jsKeys).includes(k));
    const onlyJs = structFace(jsKeys).filter((k) => !structFace(rustKeys).includes(k));
    add(
      'P4-3 两端**结构面**字段一致（壳侧 ＝ 插件侧）',
      onlyRust.length === 0 && onlyJs.length === 0 ? PASS : FAIL,
      onlyRust.length === 0 && onlyJs.length === 0
        ? '两端结构面字段清单一致'
        : `壳侧多 ${onlyRust.join('、') || '无'}／插件侧多 ${onlyJs.join('、') || '无'}`,
    );
  }

  // 结构面必须**恰好**是白名单的结构面全集（少一项也红）—— 白名单是**穷举列举**：
  // 该有的都要有，不该有的不能有（⊆ 那条管「不该有的」）。
  for (const [label, keys] of [
    ['壳侧', rustKeys],
    ['插件侧', jsKeys],
  ]) {
    if (keys.length === 0) continue;
    const got = keys.filter((k) => k !== ITEM_ENTRY_KEY).sort();
    const want = [...WHITELIST].sort();
    const missing = want.filter((k) => !got.includes(k));
    add(
      `P4-3 ${label}**结构面**字段清单 ＝ 白名单的结构面全集（${want.length} 项）`,
      missing.length === 0 ? PASS : FAIL,
      missing.length === 0
        ? `${label}结构面 ${got.length} 项与白名单一致`
        : `${label}结构面少：${missing.join('、')}`,
    );
  }

  const forbiddenInWhitelist = whitelist.filter((k) => FORBIDDEN.includes(k));
  add(
    'P4-3 白名单里没有禁回传的字段（正文/标题/会话 id/凭据）',
    forbiddenInWhitelist.length === 0 ? PASS : FAIL,
    forbiddenInWhitelist.length === 0
      ? '白名单里没有正文、标题、会话 id、凭据'
      : `白名单里混进了：${forbiddenInWhitelist.join('、')}`,
  );

  const allKeys = [...rustKeys, ...jsKeys];
  const rawBody = allKeys.filter((k) => ['raw', 'rawBody', 'response', 'responseText', 'body'].includes(k));
  add(
    'P4-3 回程载荷**不是**站点的完整响应体',
    rawBody.length === 0 ? PASS : FAIL,
    rawBody.length === 0 ? '载荷里没有整包字段' : `载荷里出现了整包字段：${rawBody.join('、')}`,
  );

  // --- 目录项面 ---
  for (const [label, keys] of [
    ['壳侧', rustKeys],
    ['插件侧', jsKeys],
  ]) {
    if (keys.length === 0) continue;
    add(
      `P4-3 ${label}载荷有 \`${ITEM_ENTRY_KEY}\` 字段（目录项面入口）`,
      keys.includes(ITEM_ENTRY_KEY) ? PASS : FAIL,
      keys.includes(ITEM_ENTRY_KEY)
        ? `${label}载荷有 ${ITEM_ENTRY_KEY}`
        : `${label}载荷还没有 ${ITEM_ENTRY_KEY} ⇒ **目录项面未到位**（实现侧还没加？见协议 §2.4 v0.17）`,
    );
  }

  for (const [label, fields] of [
    ['壳侧', rustItem],
    ['插件侧', jsItem],
  ]) {
    if (fields.length === 0) {
      add(
        `P4-3 ${label} \`${ITEM_ENTRY_KEY}\` 元素恰好 ${itemKeys.length} 个键`,
        FAIL,
        `没读到${label}的 ${ITEM_ENTRY_KEY} 元素形态 ⇒ **目录项面未到位**（实现侧还没加，或形态与判据不匹配）`,
      );
      continue;
    }
    const names = fields.map((f) => f.name);
    const missing = itemKeys.filter((k) => !names.includes(k));
    const extra = names.filter((k) => !itemKeys.includes(k));
    add(
      `P4-3 ${label} \`${ITEM_ENTRY_KEY}\` 元素恰好 ${itemKeys.length} 个键`,
      missing.length === 0 && extra.length === 0 ? PASS : FAIL,
      missing.length === 0 && extra.length === 0
        ? `${label}元素键集合与 §4.3 ② 一致（${names.length} 键）`
        : `缺 ${missing.join('、') || '无'}／多 ${extra.join('、') || '无'}`,
    );
  }

  {
    const keyOf = (fields) => fields.map((f) => f.name).sort().join('、');
    const typeOf = (fields) =>
      fields
        .map((f) => `${f.name}:${f.type}`)
        .sort()
        .join('、');
    const same = keyOf(rustItem) === keyOf(jsItem) && typeOf(rustItem) === typeOf(jsItem);
    const bothEmpty = rustItem.length === 0 && jsItem.length === 0;
    add(
      'P4-3 两端**目录项面**逐项相等（键 ＋ 类型）',
      same && !bothEmpty ? PASS : FAIL,
      bothEmpty
        ? '两端都没读到目录项形态 ⇒ **目录项面未到位**（实现侧还没加，或形态与判据不匹配）'
        : same
          ? '两端目录项的键与类型一致'
          : `壳侧 ${typeOf(rustItem) || '（无）'}／插件侧 ${typeOf(jsItem) || '（无）'}`,
    );
  }

  for (const [label, fields] of [
    ['壳侧', rustItem],
    ['插件侧', jsItem],
  ]) {
    if (fields.length === 0) {
      add(
        `P4-3 ${label} 目录项 6 键的类型符合 §4.3 ②`,
        FAIL,
        `没读到${label}的目录项形态 ⇒ **目录项面未到位**`,
      );
      continue;
    }
    const bad = [];
    for (const [key, expected] of Object.entries(ITEM_TYPES)) {
      const hit = fields.find((f) => f.name === key);
      if (!hit) {
        bad.push(`${key}（缺）`);
        continue;
      }
      if (hit.type !== expected) bad.push(`${key}＝${hit.type}（期望 ${expected}）`);
    }
    add(
      `P4-3 ${label} 目录项 6 键的类型符合 §4.3 ②`,
      bad.length === 0 ? PASS : FAIL,
      bad.length === 0
        ? '6 个键的类型都与 §4.3 ② 一致'
        : `类型不符：${bad.join('、')}`,
    );
  }

  for (const [label, fields] of [
    ['壳侧', rustItem],
    ['插件侧', jsItem],
  ]) {
    if (fields.length === 0) {
      add(
        `P4-3 ${label} 目录项内部没有禁项（正文/凭据类）`,
        FAIL,
        `没读到${label}的目录项形态 ⇒ **目录项面未到位**`,
      );
      continue;
    }
    const hit = fields.map((f) => f.name).filter((n) => FORBIDDEN_ITEM.includes(n));
    add(
      `P4-3 ${label} 目录项内部没有禁项（正文/凭据类）`,
      hit.length === 0 ? PASS : FAIL,
      hit.length === 0 ? '目录项内部没有正文/凭据类字段' : `目录项内部出现禁项：${hit.join('、')}`,
    );
  }

  const notThisRound = whitelist.filter((k) => k === 'deepLink' || k === 'cursor');
  add(
    'P4-3 白名单里没有 `deepLink` / `cursor`（本轮不回 ⇒ 出现即红）',
    notThisRound.length === 0 ? PASS : FAIL,
    notThisRound.length === 0
      ? '白名单里没有 deepLink / cursor（G4-18 / G4-8 未回）'
      : `白名单里混进了：${notThisRound.join('、')}`,
  );

  // --- 插件侧**契约面**（缺口 G4-21：契约本身也要有人守）---
  //
  // 契约的键面 ＝ §4.3 ② 那 6 个键 ＋ 类型写死 ⇒ ⇒ 并且**不许**出现禁项
  // （`sessionId` / `deepLink` / `cursor` / 正文 / 凭据 …）⇒ ⇒ 旧那套
  // 「4 键 ＋ 全 string」的契约会被这三条当场抓住（PM 实测过：那时候判据
  // 全绿，而契约与真实载荷正面冲突）。
  const contract = Array.isArray(reading?.jsContractItemFields) ? reading.jsContractItemFields : [];
  if (contract.length === 0) {
    add(
      'P4-3 插件侧契约的目录项键面 ＝ §4.3 ② 那 6 个键',
      FAIL,
      `没读到插件侧契约（${CONTRACT_REL} 的 CATALOG_ITEM_FIELDS）⇒ **契约面没人守**`,
    );
    add('P4-3 插件侧契约的目录项类型 ＝ §4.3 ②', FAIL, '没读到插件侧契约');
    add(
      'P4-3 插件侧契约里没有禁项（`sessionId` / `deepLink` / `cursor` / 正文 / 凭据）',
      FAIL,
      '没读到插件侧契约',
    );
  } else {
    const contractNames = contract.map((f) => f.name);
    const missing = itemKeys.filter((k) => !contractNames.includes(k));
    const extra = contractNames.filter((k) => !itemKeys.includes(k));
    add(
      'P4-3 插件侧契约的目录项键面 ＝ §4.3 ② 那 6 个键',
      missing.length === 0 && extra.length === 0 ? PASS : FAIL,
      missing.length === 0 && extra.length === 0
        ? `契约的键面与 §4.3 ② 一致（${contractNames.length} 键）`
        : `缺 ${missing.join('、') || '无'}／多 ${extra.join('、') || '无'}`,
    );
    const badTypes = [];
    for (const [key, expected] of Object.entries(ITEM_TYPES)) {
      const hit = contract.find((f) => f.name === key);
      if (!hit) {
        badTypes.push(`${key}（缺）`);
        continue;
      }
      if (hit.type !== expected) badTypes.push(`${key}＝${hit.type}（期望 ${expected}）`);
    }
    add(
      'P4-3 插件侧契约的目录项类型 ＝ §4.3 ②',
      badTypes.length === 0 ? PASS : FAIL,
      badTypes.length === 0
        ? '6 个键的类型都与 §4.3 ② 一致'
        : `类型不符：${badTypes.join('、')}`,
    );
    const forbiddenInContract = contractNames.filter((n) => FORBIDDEN_ITEM.includes(n));
    add(
      'P4-3 插件侧契约里没有禁项（`sessionId` / `deepLink` / `cursor` / 正文 / 凭据）',
      forbiddenInContract.length === 0 ? PASS : FAIL,
      forbiddenInContract.length === 0
        ? '契约里没有禁项'
        : `契约里混进了：${forbiddenInContract.join('、')}`,
    );
  }

  return judgeChecks(checks);
}

/**
 * 场景表（合成读数）。
 *
 * @returns {Array<object>}
 */
export function buildScenarios() {
  const itemFields = Object.entries(ITEM_TYPES).map(([name, type]) => ({ name, type }));
  const contractFields = itemFields.map((f) => ({ ...f }));
  const clean = {
    rustKeys: [...TOP_WHITELIST],
    jsKeys: [...TOP_WHITELIST],
    rustItemFields: itemFields.map((f) => ({ ...f })),
    jsItemFields: itemFields.map((f) => ({ ...f })),
    // **插件侧契约面**（G4-21）：真身契约 ＝ §4.3 ② 那 6 个键 ＋ 类型。
    jsContractItemFields: contractFields.map((f) => ({ ...f })),
    whitelist: [...TOP_WHITELIST],
    itemKeys: [...ITEM_KEYS],
  };
  const withItem = (fields, extraKey) => [...fields, { name: extraKey, type: 'string' }];
  return [
    { id: 'A', name: '场景A 真身载荷（两面都齐）⇒ 应通过', expect: PASS, reading: clean },
    {
      id: 'B',
      name: '场景B 载荷里混进 `title`（标题禁止在**顶层**回程）⇒ 应失败',
      expect: FAIL,
      reading: { ...clean, rustKeys: [...TOP_WHITELIST, 'title'] },
    },
    {
      id: 'C',
      name: '场景C 载荷里混进 `token`（凭据禁止回程）⇒ 应失败',
      expect: FAIL,
      reading: { ...clean, jsKeys: [...TOP_WHITELIST, 'token'] },
    },
    {
      id: 'D',
      name: '场景D 载荷里混进 `body`（正文禁止回程）⇒ 应失败',
      expect: FAIL,
      reading: { ...clean, rustKeys: [...TOP_WHITELIST, 'body'] },
    },
    {
      id: 'E',
      name: '场景E 两端**结构面**字段不一致 ⇒ 应失败',
      expect: FAIL,
      reading: { ...clean, jsKeys: TOP_WHITELIST.filter((k) => k !== 'hasMore') },
    },
    {
      id: 'F',
      name: '场景F 载荷里混进 `status`（白名单之外 ⇒ 红）⇒ 应失败',
      expect: FAIL,
      reading: { ...clean, rustKeys: [...TOP_WHITELIST, 'status'] },
    },
    {
      id: 'G',
      name: '场景G 目录项里塞 `body`（正文进数组内部 ⇒ 红）⇒ 应失败',
      expect: FAIL,
      reading: {
        ...clean,
        rustItemFields: withItem(itemFields, { body: 'string' }),
        jsItemFields: withItem(itemFields, { body: 'string' }),
      },
    },
    {
      id: 'H',
      name: '场景H 目录项多一个第 7 键 ⇒ 应失败',
      expect: FAIL,
      reading: {
        ...clean,
        rustItemFields: withItem(itemFields, { extra_key: 'string' }),
        jsItemFields: withItem(itemFields, { extra_key: 'string' }),
      },
    },
    {
      id: 'I',
      name: '场景I 载荷多出 `deepLink`（本轮不回 ⇒ 红）⇒ 应失败',
      expect: FAIL,
      reading: { ...clean, rustKeys: [...TOP_WHITELIST, 'deepLink'] },
    },
    {
      id: 'J',
      name: '场景J 载荷多出 `cursor`（本轮不回 ⇒ 红）⇒ 应失败',
      expect: FAIL,
      reading: { ...clean, jsKeys: [...TOP_WHITELIST, 'cursor'] },
    },
    {
      id: 'K',
      name: '场景K 结构面少一项（`hasMore` 没了）⇒ 应失败',
      expect: FAIL,
      reading: {
        ...clean,
        rustKeys: TOP_WHITELIST.filter((k) => k !== 'hasMore'),
        jsKeys: TOP_WHITELIST.filter((k) => k !== 'hasMore'),
      },
    },
    {
      id: 'L',
      name: '场景L 两端**目录项面**不一致（类型不同）⇒ 应失败',
      expect: FAIL,
      reading: {
        ...clean,
        jsItemFields: itemFields.map((f) =>
          f.name === 'pinned' ? { name: f.name, type: 'string' } : { ...f },
        ),
      },
    },
    {
      id: 'M',
      name: '场景M 目录项类型不符（`pinned` 给成 string）⇒ 应失败',
      expect: FAIL,
      reading: {
        ...clean,
        rustItemFields: itemFields.map((f) =>
          f.name === 'pinned' ? { name: f.name, type: 'string' } : { ...f },
        ),
        jsItemFields: itemFields.map((f) =>
          f.name === 'pinned' ? { name: f.name, type: 'string' } : { ...f },
        ),
      },
    },
    {
      id: 'N',
      name: '场景N 实现侧还没加目录项面（没有 `items`）⇒ 应失败（红在实现侧未到位）',
      expect: FAIL,
      reading: {
        ...clean,
        rustKeys: WHITELIST.slice(),
        jsKeys: WHITELIST.slice(),
        rustItemFields: [],
        jsItemFields: [],
      },
    },
    {
      id: 'O',
      name: '场景O 插件侧**契约**少一个键（回到旧形态）⇒ 应失败',
      expect: FAIL,
      reading: {
        ...clean,
        jsContractItemFields: contractFields
          .filter((f) => f.name !== 'title_type')
          .map((f) => ({ ...f })),
      },
    },
    {
      id: 'P',
      name: '场景P 插件侧**契约**把 `deepLink` 加回来（本轮不解禁）⇒ 应失败',
      expect: FAIL,
      reading: {
        ...clean,
        jsContractItemFields: [...contractFields.map((f) => ({ ...f })), { name: 'deepLink', type: 'string' }],
      },
    },
    {
      id: 'Q',
      name: '场景Q 插件侧**契约**的类型错（`pinned` 给成 string）⇒ 应失败',
      expect: FAIL,
      reading: {
        ...clean,
        jsContractItemFields: contractFields.map((f) =>
          f.name === 'pinned' ? { name: f.name, type: 'string' } : { ...f },
        ),
      },
    },
  ];
}

/**
 * 读真实仓库的读数。
 *
 * @param {string} root - 仓库根。
 * @returns {{rustKeys: string[], jsKeys: string[], rustItemFields: Array<object>, jsItemFields: Array<object>}}
 */
export function readReading(root) {
  const sources = readSources(root, P4_SCAN_FILES);
  const rust = sources.find((s) => s.path === RUST_REL)?.code ?? '';
  const js = sources.find((s) => s.path === JS_REL)?.code ?? '';
  const contract = sources.find((s) => s.path === CONTRACT_REL)?.code ?? '';
  return {
    rustKeys: objectLiteralKeysContaining(rust, 'topKeys'),
    jsKeys: arrayLiteralStrings(js, 'const keys = ['),
    rustItemFields: rustItemsElementFields(rust),
    jsItemFields: jsItemsElementFields(js, ITEM_KEYS),
    // **插件侧契约面**（G4-21）：`CATALOG_ITEM_FIELDS` 的键 ＋ 类型。
    jsContractItemFields: jsLiteralTypeFields(contract, 'title_type'),
  };
}

/**
 * 主入口。
 *
 * @param {string[]} [argv] - 命令行参数。
 * @returns {Promise<{results: Array<object>, summary: object}>}
 */
export async function main(argv = process.argv.slice(2)) {
  const root = argValue(argv, '--target') ?? repoRoot(import.meta.url);
  const round = argValue(argv, '--round') ?? 'p4c3';
  const asJson = argv.includes('--json');

  const sources = readSources(root, P4_SCAN_FILES);
  const reading = readReading(root);
  const { ok, checks } = judgeReturn(reading);

  const results = [];
  for (const c of checks) results.push({ name: c.name, status: c.status, detail: c.detail });
  results.push({
    name: 'P4-3 真实仓库读数的判定',
    status: ok ? PASS : FAIL,
    detail: ok ? '真实读数全过' : '真实读数未全过（见上）',
  });
  for (const r of runScenarios(buildScenarios(), judgeReturn, 'P4-3a')) {
    results.push({ name: r.name, status: r.status, detail: r.detail });
  }

  return emit({
    round,
    sources,
    scanLabel: 'P4 实现（壳侧取数模块 ＋ 插件 host 半边）',
    reading,
    results,
    evidenceDir: join(tmpdir(), 'dsh-p4-return'),
    asJson,
  });
}

// 直接跑（被 import 时不跑）
if (process.argv[1] && process.argv[1].endsWith('check-official-chat-p4-return.mjs')) {
  await main();
}
