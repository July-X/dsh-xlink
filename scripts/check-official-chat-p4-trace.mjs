#!/usr/bin/env node
// P4 协议 §10 判据**第 2 条**（零痕迹）—— 按 §7.1 最新口径：机检断
// 「**还原后 prototype 逐项一致**」＋「**`window` 属性增量为 0**」。
//
// ## 依据
//
// · 协议 §7.1：页内**不留全局槽位 / 全局变量 / 定时器** ⇒ 判据口径 ＝
//   `getOwnPropertyNames(window)` 前后**逐项一致**、`__dsh*` 为 0。
// · 协议 §7.1（v0.7 新增裁定）：**允许的页内痕迹形态** ＝ 替换 `XMLHttpRequest.prototype` 的
//   `open` / `send` ＋ **取到即还原**；三条约束 ① **不新增 `window` 自有属性**
//   ② **取到即还原**（不留钩子、不留定时器、不留槽位）③ **不写站点数据**。
// · 协议 §7.1 已【已实测】（成员侧两轮，注入组 ＋ 对照组各 1 tab）：`window` 自有属性
//   **1257 → 1257**（未登录态）／**1250 → 1250**（登录态），新增与移除集合均空；
//   `XMLHttpRequest.prototype` 自有属性 **27 → 27 逐项一致**；还原后 `open`／`send` 的
//   **引用 ＝ 替换前引用**（**不是还原成原生**）；对照组 3 秒自然增长 ＝ 0。
//
// ## 形态说明（这不是「发明通道形态」）
//
// §10 第 2 条的机检**就是运行期量**：痕迹面只存在于真实页面里 ⇒ **源码里量不出来**
// ⇒ 本脚本交付三件套：
//
// ① **页内探针** `TRACE_SNAPSHOT_JS` —— 一个**零副作用**的快照脚本（IIFE，**不建任何全局量**）：
//    读 `window` 自有属性名、`XMLHttpRequest.prototype` 自有属性名、以及 `open`／`send`
//    的**函数文本**。取「替换前」与「还原后」两份快照即可。
// ② **判定内核** `judgeTrace(readings)` —— 按 §7.1 口径把两份快照（＋可选对照组）判成
//    三态结论。
// ③ **合成夹具自证** —— 用合成读数证明判定内核**该红就红、该绿就绿**（含反向用例）。
//
// ⚠️ **真机轨恒 `skipped`**：被测实现（P4 页内取数脚本）**尚未落地** ⇒ 真机读数产生不了
//   ⇒ 工具已就位，等实现落地后把两次快照喂进 `judgeTrace` 即可（按纪律不自行发明实现形态）。
//
// ## 两条如实登记的观测局限
//
// · **函数引用用「函数文本」代理**（`Function.prototype.toString`）：快照要跨两次页内求值
//   比较「是不是同一个引用」，**不能**在页内留槽位（§7.1 明令禁止）⇒ 只能用文本代理。
//   局限：两个**文本相同但不是同一个**的函数分辨不出。§7.1 实测中替换前后 `open`／`send`
//   均为 `[native code]` ⇒ 在本站该口径下文本代理与引用相等**等价**。
// · **「不留定时器」测不了**：页内**没有**枚举活跃定时器的标准 API ⇒ 定时器残留不落在
//   `window` 自有属性上 ⇒ 这一条**不可机检**，如实登记为观测缺口（不是通过、也不是失败）。
//
// @module scripts/check-official-chat-p4-trace

import { mkdirSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

/** verdict 三态。 */
export const PASS = 'pass';
export const FAIL = 'fail';
export const SKIPPED = 'skipped';

/** §7.1：页内全局量的禁止前缀。 */
export const DSH_PREFIX = '__dsh';

/**
 * **页内快照探针**（零副作用）。
 *
 * · 必须是 **IIFE** 且**不写任何全局量** ⇒ 探针自己**不留痕**（自测里用 `node:vm` 在假
 *   `globalThis` 上验过：跑前跑后自有属性数不变）。
 * · 只回**结构**：属性名数组 ＋ 函数文本。**不回任何值**、不回正文、不回凭据。
 */
export const TRACE_SNAPSHOT_JS = `(() => {
  const own = (o) => Object.getOwnPropertyNames(o);
  const text = (f) => (typeof f === 'function' ? Function.prototype.toString.call(f) : null);
  const proto = globalThis.XMLHttpRequest ? globalThis.XMLHttpRequest.prototype : null;
  return {
    windowProps: own(globalThis),
    protoProps: proto ? own(proto) : null,
    openText: proto ? text(proto.open) : null,
    sendText: proto ? text(proto.send) : null,
  };
})()`;

/**
 * 造一份**合成快照**（**不是**真实读数；真实读数形态见 §7.1 的实测数字）。
 *
 * @param {{props?: string[], protoProps?: string[], openText?: string, sendText?: string}} [opts]
 * @returns {{windowProps: string[], protoProps: string[], openText: string, sendText: string}}
 */
export function fakeSnapshot({
  props = ['location', 'navigator', 'XMLHttpRequest'],
  protoProps = ['constructor', 'open', 'send'],
  openText = 'function open() { [native code] }',
  sendText = 'function send() { [native code] }',
} = {}) {
  return { windowProps: [...props], protoProps: [...protoProps], openText, sendText };
}

/**
 * **判定内核**：按 §7.1 口径判「替换前 → 还原后」两份快照。
 *
 * @param {{before?: object, after?: object, baseline?: {before: object, after: object}}} readings
 * @returns {{ok: boolean, checks: Array<{name: string, status: string, detail: string}>}}
 */
export function judgeTrace(readings) {
  const checks = [];
  const add = (name, status, detail) => {
    checks.push({ name, status, detail });
  };

  const before = readings?.before ?? null;
  const after = readings?.after ?? null;
  if (!before || !after) {
    add('读数齐全（替换前 / 还原后两份快照）', FAIL, '缺 before 或 after 快照 ⇒ 判据无法执行');
    return { ok: false, checks };
  }

  const bProps = Array.isArray(before.windowProps) ? before.windowProps : [];
  const aProps = Array.isArray(after.windowProps) ? after.windowProps : [];
  const bSet = new Set(bProps);
  const aSet = new Set(aProps);
  const added = aProps.filter((x) => !bSet.has(x));
  const removed = bProps.filter((x) => !aSet.has(x));

  // 对照组（§7.1 的读数方法用「对照组相减」剔除站点自然增长）
  const ctrlBefore = readings?.baseline?.before ?? null;
  const ctrlAfter = readings?.baseline?.after ?? null;
  const hasControl = !!ctrlBefore && !!ctrlAfter;
  const ctrlB = hasControl && Array.isArray(ctrlBefore.windowProps) ? ctrlBefore.windowProps : [];
  const ctrlA = hasControl && Array.isArray(ctrlAfter.windowProps) ? ctrlAfter.windowProps : [];
  const ctrlAdded = hasControl ? ctrlA.filter((x) => !ctrlB.includes(x)) : [];
  const ctrlRemoved = hasControl ? ctrlB.filter((x) => !ctrlA.includes(x)) : [];
  const ctrlDelta = hasControl ? ctrlA.length - ctrlB.length : null;

  // 「注入独有」＝ 注入组出现/移除、而对照组没有出现/移除的 ⇒ 归因于注入（站点自然增长之外的痕迹）
  const unexplainedAdded = hasControl ? added.filter((x) => !ctrlAdded.includes(x)) : added;
  const unexplainedRemoved = hasControl ? removed.filter((x) => !ctrlRemoved.includes(x)) : removed;
  add(
    'window 自有属性前后逐项一致',
    unexplainedAdded.length === 0 && unexplainedRemoved.length === 0 ? PASS : FAIL,
    `注入组新增 ${added.length} 个／移除 ${removed.length} 个` +
      (hasControl
        ? `；对照组自然新增 ${ctrlAdded.length} 个／自然移除 ${ctrlRemoved.length} 个 ⇒ 注入独有 新增 ${unexplainedAdded.length} 个／移除 ${unexplainedRemoved.length} 个` +
          (unexplainedAdded.length ? `（${unexplainedAdded.slice(0, 5).join('、')}）` : '')
        : '；本轮无对照组 ⇒ 注入组的新增/移除必须为空') +
      (unexplainedRemoved.length ? `（移除：${unexplainedRemoved.slice(0, 5).join('、')}）` : ''),
  );

  const delta = aProps.length - bProps.length;
  const attributable = hasControl ? delta - ctrlDelta : delta;
  add(
    'window 属性增量 ＝ 0',
    attributable === 0 ? PASS : FAIL,
    hasControl
      ? `注入组增量=${delta} − 对照组自然增长=${ctrlDelta} ⇒ 归因增量=${attributable}`
      : `增量=${attributable}（${bProps.length} → ${aProps.length}；本轮无对照组）`,
  );

  const dshBefore = bProps.filter((n) => String(n).startsWith(DSH_PREFIX));
  const dshAfter = aProps.filter((n) => String(n).startsWith(DSH_PREFIX));
  add(
    '`__dsh*` 全局量 ＝ 0',
    dshBefore.length === 0 && dshAfter.length === 0 ? PASS : FAIL,
    `替换前 ${dshBefore.length} 个／还原后 ${dshAfter.length} 个${dshAfter.length ? `（${dshAfter.slice(0, 5).join('、')}）` : ''}`,
  );

  const bProto = Array.isArray(before.protoProps) ? before.protoProps : [];
  const aProto = Array.isArray(after.protoProps) ? after.protoProps : [];
  const protoAdded = aProto.filter((x) => !bProto.includes(x));
  const protoRemoved = bProto.filter((x) => !aProto.includes(x));
  add(
    'XMLHttpRequest.prototype 自有属性逐项一致',
    protoAdded.length === 0 && protoRemoved.length === 0 ? PASS : FAIL,
    `新增 ${protoAdded.length} 个／移除 ${protoRemoved.length} 个（${bProto.length} → ${aProto.length}）`,
  );

  add(
    '还原后 open 引用 ＝ 替换前引用',
    before.openText !== null && before.openText === after.openText ? PASS : FAIL,
    `替换前=${before.openText ?? 'null'}／还原后=${after.openText ?? 'null'}`,
  );
  add(
    '还原后 send 引用 ＝ 替换前引用',
    before.sendText !== null && before.sendText === after.sendText ? PASS : FAIL,
    `替换前=${before.sendText ?? 'null'}／还原后=${after.sendText ?? 'null'}`,
  );

  // 对照组：§7.1 的读数方法用「对照组相减」剔除站点自然增长
  if (hasControl) {
    add(
      '对照组自然增长已相减（注入组 − 对照组）',
      delta - ctrlDelta === 0 ? PASS : FAIL,
      `注入组增量=${delta}／对照组自然增长=${ctrlDelta}／相减=${delta - ctrlDelta}`,
    );
  } else {
    add(
      '对照组自然增长已相减（注入组 − 对照组）',
      SKIPPED,
      '本轮读数未带对照组 ⇒ 未做自然增长相减（§7.1 的读数方法用对照组相减剔除站点自然增长）',
    );
  }

  // 如实登记的观测缺口（不是通过、也不是失败）
  add(
    '不留定时器（页内无法枚举活跃定时器 ⇒ 不可机检）',
    SKIPPED,
    '页内没有枚举活跃定时器的标准 API ⇒ 定时器残留不落在 window 自有属性上 ⇒ 本条不可机检（观测缺口，如实登记）',
  );

  const judged = checks.filter((c) => c.status !== SKIPPED);
  return { ok: judged.length > 0 && judged.every((c) => c.status === PASS), checks };
}

/**
 * **场景表**（合成读数）：每个场景声明「应当通过」还是「应当失败」。
 *
 * @returns {Array<{id: string, name: string, expect: string, readings: object}>}
 */
export function buildScenarios() {
  const clean = fakeSnapshot();

  // T1 正常：还原成替换前那个引用、window 增量 0、prototype 逐项一致、无 `__dsh*`
  const t1 = { before: clean, after: fakeSnapshot() };

  // T2 window 多了一个属性 ⇒ 红
  const t2 = { before: clean, after: fakeSnapshot({ props: ['location', 'navigator', 'XMLHttpRequest', 'extra'] }) };

  // T3 window 少了一个属性 ⇒ 红（数量对得上也红：集合必须逐项一致）
  const t3 = { before: clean, after: fakeSnapshot({ props: ['location', 'navigator'] }) };

  // T4 prototype 少了一项 ⇒ 红
  const t4 = { before: clean, after: fakeSnapshot({ protoProps: ['constructor', 'open'] }) };

  // T5 还原成了原生（≠ 替换前那个引用）⇒ 红
  const t5 = {
    before: fakeSnapshot({ openText: 'function open() { [native code] }' }),
    after: fakeSnapshot({ openText: 'function open() { /* wrapper */ }' }),
  };

  // T6 留了 `__dsh*` 槽位 ⇒ 红
  const t6 = { before: clean, after: fakeSnapshot({ props: ['location', 'navigator', 'XMLHttpRequest', '__dsh_rt_1'] }) };

  // T7 注入组增了 2，**对照组也自然增长了 2（同样的属性名，站点自己长出来的）** ⇒ 相减后 0 ⇒ **不该红**
  const t7 = {
    before: clean,
    after: fakeSnapshot({ props: ['location', 'navigator', 'XMLHttpRequest', 'site_growth_1', 'site_growth_2'] }),
    baseline: {
      before: fakeSnapshot({ props: ['location', 'navigator', 'XMLHttpRequest'] }),
      after: fakeSnapshot({ props: ['location', 'navigator', 'XMLHttpRequest', 'site_growth_1', 'site_growth_2'] }),
    },
  };

  // T8 无对照组、注入组增量 0 ⇒ 「相减」那条报 skipped，其余全过
  const t8 = { before: clean, after: fakeSnapshot() };

  // T9 注入组多了一个**对照组没有**的属性 ⇒ 归因于注入 ⇒ 必须判红
  const t9 = {
    before: clean,
    after: fakeSnapshot({ props: ['location', 'navigator', 'XMLHttpRequest', 'site_growth_1', 'leak_1'] }),
    baseline: {
      before: fakeSnapshot({ props: ['location', 'navigator', 'XMLHttpRequest'] }),
      after: fakeSnapshot({ props: ['location', 'navigator', 'XMLHttpRequest', 'site_growth_1'] }),
    },
  };

  return [
    { id: 'T1', name: '正常：还原成替换前引用、window 增量 0、prototype 逐项一致', expect: PASS, readings: t1 },
    { id: 'T2', name: 'window 多了一个属性 ⇒ 应失败', expect: FAIL, readings: t2 },
    { id: 'T3', name: 'window 少了一个属性 ⇒ 应失败（数量对得上也红）', expect: FAIL, readings: t3 },
    { id: 'T4', name: 'prototype 少了一项 ⇒ 应失败', expect: FAIL, readings: t4 },
    { id: 'T5', name: '还原成原生而非替换前那个引用 ⇒ 应失败', expect: FAIL, readings: t5 },
    { id: 'T6', name: '留了 `__dsh*` 槽位 ⇒ 应失败', expect: FAIL, readings: t6 },
    { id: 'T7', name: '注入组与对照组同样自然增长 2 ⇒ 相减为 0 ⇒ 应通过', expect: PASS, readings: t7 },
    { id: 'T8', name: '无对照组、增量 0 ⇒ 应通过（「相减」那条报未执行）', expect: PASS, readings: t8 },
    { id: 'T9', name: '注入组多了一个对照组没有的属性 ⇒ 归因于注入 ⇒ 应失败', expect: FAIL, readings: t9 },
  ];
}

/**
 * 跑场景表：核「实际判定」是否等于「应当的判定」。
 *
 * @param {Array<object>} [scenarios] - 场景表。
 * @returns {Array<{name: string, status: string, detail: string, checks: Array<object>}>}
 */
export function runScenarios(scenarios = buildScenarios()) {
  return scenarios.map((sc) => {
    const { ok, checks } = judgeTrace(sc.readings);
    const behavedAsExpected = sc.expect === PASS ? ok : !ok;
    const bad = checks.filter((c) => c.status === FAIL).map((c) => c.name);
    const skipped = checks.filter((c) => c.status === SKIPPED).map((c) => c.name);
    return {
      name: `P4-2a ${sc.name}`,
      status: behavedAsExpected ? PASS : FAIL,
      detail:
        `实际判定＝${ok ? '通过' : '失败'}（期望＝${sc.expect === PASS ? '通过' : '失败'}）` +
        (bad.length ? `｜未过的断言：${bad.join('、')}` : '｜可机检的断言全过') +
        (skipped.length ? `｜未执行：${skipped.join('、')}` : ''),
      checks,
    };
  });
}

/**
 * **真机轨**：真实页面 × 被测实现 ⇒ **恒 `skipped`**（被测实现未落地）。
 *
 * @returns {{name: string, status: string, detail: string}}
 */
export function realRunVerdict() {
  return {
    name: 'P4-2z 真机轨（真实页面 × 被测实现）',
    status: SKIPPED,
    detail:
      '被测实现（P4 页内取数脚本）尚未落地 ⇒ 真机读数产生不了 ⇒ 本判据已交付「页内探针 ＋ 判定内核 ＋ 合成夹具自证」，等实现落地后把「替换前 / 还原后」两份快照喂进 judgeTrace 即可',
  };
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
  const roundIndex = argv.findIndex((a) => a === '--round' || String(a).startsWith('--round='));
  let round = 'p4c2';
  if (roundIndex >= 0) {
    const inline = String(argv[roundIndex]).split('=')[1];
    const next = argv[roundIndex + 1];
    round = inline ?? (next !== undefined && !String(next).startsWith('--') ? next : 'p4c2');
  }
  const asJson = argv.includes('--json');

  const results = runScenarios();
  results.push(realRunVerdict());
  const summary = summarize(results);

  const evidenceDir = join(tmpdir(), 'dsh-p4-trace');
  mkdirSync(evidenceDir, { recursive: true });
  const outPath = join(evidenceDir, `results-${round}.json`);
  writeFileSync(
    outPath,
    `${JSON.stringify({ round, at: new Date().toISOString(), scanned: [], results, summary }, null, 2)}\n`,
    'utf8',
  );

  if (asJson) {
    process.stdout.write(`${JSON.stringify({ round, results, summary }, null, 2)}\n`);
  } else {
    for (const r of results) {
      const mark = r.status === PASS ? '✔' : r.status === FAIL ? '✘' : '—';
      process.stdout.write(`${mark} ${r.name}：${r.detail}\n`);
    }
    process.stdout.write(
      `\n合计 ${summary.total} 条：通过 ${summary.pass} · 失败 ${summary.fail} · 未执行 ${summary.skipped}\n` +
        `扫描面：无（判据 2 是**运行期**判据 —— 痕迹面只存在于真实页面里，源码里量不出来）\n` +
        `扫了 0 个文件 · 证据：${outPath}\n`,
    );
  }
  return { results, summary };
}

// 直接跑（被 import 时不跑）
if (process.argv[1] && process.argv[1].endsWith('check-official-chat-p4-trace.mjs')) {
  await main();
}
