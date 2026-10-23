// P4 §10 判据**第 2 条**（零痕迹）的**自测**。
//
// ## 判什么（口径 ＝ v0.9 的「**页内什么都没碰**」）
//
// · **场景表必须自证**：T1（正常：页内什么都没碰）通过 · T2（window 多属性）失败 · T3（window 少属性）失败 ·
//   T4（prototype 少一项）失败 · T5（**页内 `open` 被换过**）失败 · T6（留 `__dsh*` 槽位）失败 ·
//   T7（实测组增量 2、对照组自然增长 2 ⇒ 相减为 0）**通过** · T8（无对照组）通过。
// · **反向不误伤**：对照组相减后为 0 ⇒ **不该红**（这是 §7.1 读数方法的要害）。
// · **探针自己不留痕**：在 `node:vm` 的假 `globalThis` 上跑页内探针 ⇒ 跑前跑后自有属性数不变。
// · **观测缺口如实登记**：「不留定时器」页内**不可枚举** ⇒ 恒报 `skipped`（不是通过、不是失败）。
// · **真机轨**：**绑定的形态已改**（不再是等「页内取数脚本」——那形态已被 v0.10 排除）⇒
//   绑的是「**壳侧 `Network` 域订阅跑一轮**」⇒ 给了读数就真判、没给就报 `skipped`
//   （**不硬红、不假装测过**）。
// · **口径护栏**：断言名里**不许**出现「还原后」（协议 §10 第 2 条明文不许写成「还原后一致」）。
//
// ## 纪律
//
// · 夹具全在内存里（不落盘）；**永不碰用户的真实数据目录**、不发任何网络请求。
//
// @module scripts/check-official-chat-p4-trace-test

import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';
import test from 'node:test';

import {
  DSH_PREFIX,
  FAIL,
  FORM_A,
  FORM_B,
  FORM_C,
  PASS,
  SEG_BOTH,
  SEG_COMMON,
  SEG_MACOS,
  SEG_OTHER,
  SEG_WINDOWS,
  SKIPPED,
  SLOT_PREFIX,
  TRACE_SNAPSHOT_JS,
  attributeByForm,
  buildScenarios,
  cfgSegmentKind,
  collectChainSources,
  evalCfgPredicate,
  fakeSnapshot,
  judgeFormBChain,
  judgeFormCChain,
  judgeTrace,
  main,
  realRunVerdict,
  runScenarios,
  segmentRustByCfg,
  summarize,
} from './check-official-chat-p4-trace.mjs';

const REPO = fileURLToPath(new URL('../', import.meta.url));

/** 合成快照的基准 `window` 自有属性。 */
const BASE_PROPS = ['location', 'navigator', 'XMLHttpRequest'];

/** 合成快照的槽位名（形如 `__dsh_rt_<hex>`）。 */
const SLOT = '__dsh_rt_9f3c';

/**
 * **形态 A 的既有八条断言**（改造前就有的那八条 —— 逐字钉住，不许被改口径改掉）。
 *
 * ⚠️ 形态 B 的断言集合必须**包含**这八条（取数结束后必须回到取数前）；
 * ⚠️ 形态 A 另有一条**形态专属**的「中途快照也无页内变化」—— 它按设计**不属于**形态 B
 *   （形态 B 的中途快照**就该**有那一个槽位）。
 */
const FORM_A_BASE_CHECKS = Object.freeze([
  'window 自有属性前后逐项一致',
  'window 属性增量 ＝ 0',
  '`__dsh*` 全局量 ＝ 0',
  'XMLHttpRequest.prototype 自有属性逐项一致',
  '页内 `open` 引用前后一致（页内没碰过）',
  '页内 `send` 引用前后一致（页内没碰过）',
  '对照组自然增长已相减（实测组 − 对照组）',
  '不留定时器（页内无法枚举活跃定时器 ⇒ 不可机检）',
]);

/**
 * **形态 A 八条断言的「基线 JSON」**（**逐字**钉住：名字 ＋ 顺序 ＋ 集合）。
 *
 * ⚠️ **判据本体不许改口径**：形态 A 的断言集合 ＝ 这八条 ＋ **恰好**一条形态专属的
 * 「中途快照也无页内变化」⇒ ⇒ **不许删、不许加、不许改名、不许改顺序**。
 *
 * · `FORM_A_BASELINE_JSON` ＝ 那八条的**规范 JSON**（`JSON.stringify(names, null, 2)`）。
 * · `FORM_A_BASELINE_SHA256` ＝ 上面那份 JSON 的 sha256（**再钉一道**：连空白都不许变）。
 */
const FORM_A_BASELINE_JSON = `[
  "window 自有属性前后逐项一致",
  "window 属性增量 ＝ 0",
  "\`__dsh*\` 全局量 ＝ 0",
  "XMLHttpRequest.prototype 自有属性逐项一致",
  "页内 \`open\` 引用前后一致（页内没碰过）",
  "页内 \`send\` 引用前后一致（页内没碰过）",
  "对照组自然增长已相减（实测组 − 对照组）",
  "不留定时器（页内无法枚举活跃定时器 ⇒ 不可机检）"
]`;
const FORM_A_BASELINE_SHA256 = '668125b3653403d9ace43f86edd01ca39120cb0aa598977f88e4f14ad1601742';

/** 形态 A 的**形态专属**那条（不属于形态 B：形态 B 的中途**就该**有那个槽位）。 */
const FORM_A_ONLY_CHECK = '形态 A · 中途快照也无页内变化（形态 A ＝ 页内零动作）';

/**
 * 某条断言的状态。
 *
 * @param {Array<{name: string, status: string, detail: string}>} checks
 * @param {string} keyword - 断言名关键词。
 * @returns {string}
 */
function statusOf(checks, keyword) {
  const hit = checks.find((c) => c.name.includes(keyword));
  assert.ok(hit, `断言里找不到「${keyword}」`);
  return hit.status;
}

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

test('场景表 · 20 个场景的判定全部符合预期（形态 A 九个 ＋ 形态 B 六个 ＋ 形态 C 五个）', () => {
  const scenarios = buildScenarios();
  assert.equal(scenarios.length, 20);
  const results = runScenarios(scenarios);
  for (const r of results) {
    assert.equal(r.status, PASS, `${r.name} 未按预期：${r.detail}`);
  }
  assert.equal(summarize(results).fail, 0);
});

test('T1 · 正常读数（页内什么都没碰）⇒ 可机检的断言全过', () => {
  const { ok, checks } = judgeTrace({ before: fakeSnapshot(), after: fakeSnapshot() });
  assert.equal(ok, true);
  assert.ok(checks.every((c) => c.status !== FAIL));
  assert.ok(checks.some((c) => c.name.includes('window 自有属性前后逐项一致')));
  assert.ok(checks.some((c) => c.name.includes('prototype 自有属性逐项一致')));
  assert.ok(checks.some((c) => c.name.includes('`open` 引用前后一致')));
  assert.ok(checks.some((c) => c.name.includes('`send` 引用前后一致')));
});

test('T2 · window 多了一个属性 ⇒ 必须判失败', () => {
  const after = fakeSnapshot({ props: ['location', 'navigator', 'XMLHttpRequest', 'extra'] });
  const { ok } = judgeTrace({ before: fakeSnapshot(), after });
  assert.equal(ok, false, '新增 window 自有属性必须判红');
});

test('T3 · window 少了一个属性 ⇒ 必须判失败（数量对得上也红：集合要逐项一致）', () => {
  const after = fakeSnapshot({ props: ['location', 'navigator'] });
  const { ok } = judgeTrace({ before: fakeSnapshot(), after });
  assert.equal(ok, false, '移除 window 自有属性必须判红');
});

test('T4 · prototype 少了一项 ⇒ 必须判失败', () => {
  const after = fakeSnapshot({ protoProps: ['constructor', 'open'] });
  const { ok } = judgeTrace({ before: fakeSnapshot(), after });
  assert.equal(ok, false, 'prototype 未逐项还原必须判红');
});

test('T5 · 页内 `open` 被换过（跑前 ≠ 跑后）⇒ 必须判失败', () => {
  const before = fakeSnapshot({ openText: 'function open() { [native code] }' });
  const after = fakeSnapshot({ openText: 'function open() { /* wrapper */ }' });
  const { ok } = judgeTrace({ before, after });
  assert.equal(ok, false, 'v0.9 口径：页内没碰过 `open` ⇒ 跑前跑后必须是同一个引用');
});

test('T6 · 留了 `__dsh*` 槽位 ⇒ 必须判失败', () => {
  const after = fakeSnapshot({ props: ['location', 'navigator', 'XMLHttpRequest', '__dsh_rt_1'] });
  const { ok, checks } = judgeTrace({ before: fakeSnapshot(), after });
  assert.equal(ok, false, '留 `__dsh*` 全局槽位必须判红');
  assert.ok(checks.some((c) => c.name.includes('__dsh*') && c.status === FAIL));
});

test('反向 · 对照组自然增长相减后为 0 ⇒ 不该红（§7.1 读数方法）', () => {
  const growth = ['location', 'navigator', 'XMLHttpRequest', 'site_growth_1', 'site_growth_2'];
  const readings = {
    before: fakeSnapshot(),
    after: fakeSnapshot({ props: growth }),
    baseline: {
      before: fakeSnapshot({ props: ['location', 'navigator', 'XMLHttpRequest'] }),
      after: fakeSnapshot({ props: growth }),
    },
  };
  const { ok, checks } = judgeTrace(readings);
  assert.equal(ok, true, '相减后为 0 ⇒ 不该判红');
  const sub = checks.find((c) => c.name.includes('对照组自然增长已相减'));
  assert.equal(sub.status, PASS);
});

test('T9 · 实测组多了一个对照组没有的属性 ⇒ 归因于实测 ⇒ 必须判失败', () => {
  const readings = {
    before: fakeSnapshot(),
    after: fakeSnapshot({ props: ['location', 'navigator', 'XMLHttpRequest', 'site_growth_1', 'leak_1'] }),
    baseline: {
      before: fakeSnapshot({ props: ['location', 'navigator', 'XMLHttpRequest'] }),
      after: fakeSnapshot({ props: ['location', 'navigator', 'XMLHttpRequest', 'site_growth_1'] }),
    },
  };
  const { ok } = judgeTrace(readings);
  assert.equal(ok, false, '实测独有的属性必须判红（不能被「对照组相减」掩盖）');
});

test('无对照组 ⇒ 「相减」那条报未执行（不硬红、不假装测过）', () => {
  const { checks } = judgeTrace({ before: fakeSnapshot(), after: fakeSnapshot() });
  const sub = checks.find((c) => c.name.includes('对照组自然增长已相减'));
  assert.equal(sub.status, SKIPPED, '缺对照组 ⇒ 必须报 skipped');
});

test('观测缺口 · 「不留定时器」恒报未执行（如实登记，不许记成通过）', () => {
  const { checks } = judgeTrace({ before: fakeSnapshot(), after: fakeSnapshot() });
  const timer = checks.find((c) => c.name.includes('定时器'));
  assert.ok(timer, '必须显式登记「不留定时器」这条');
  assert.equal(timer.status, SKIPPED, '页内无法枚举活跃定时器 ⇒ 只能报未执行');
  assert.match(timer.detail, /定时器/);
});

test('缺快照 ⇒ 判据无法执行 ⇒ 判失败（不许默默通过）', () => {
  const { ok, checks } = judgeTrace({ before: fakeSnapshot() });
  assert.equal(ok, false);
  assert.equal(checks[0].status, FAIL);
});

test('页内探针 · 自己不留痕（vm 假 globalThis：跑前跑后自有属性数不变）', () => {
  const sandbox = {
    XMLHttpRequest: class XMLHttpRequest {
      open() {}
      send() {}
    },
  };
  const before = Object.getOwnPropertyNames(sandbox).length;
  const snapshot = vm.runInNewContext(TRACE_SNAPSHOT_JS, sandbox);
  const after = Object.getOwnPropertyNames(sandbox).length;
  assert.equal(after, before, '探针不得新增任何全局量');
  assert.ok(Array.isArray(snapshot.windowProps), 'windowProps 必须是数组');
  assert.ok(Array.isArray(snapshot.protoProps), 'protoProps 必须是数组');
  assert.ok(snapshot.protoProps.includes('open'), 'protoProps 应含 open');
  assert.ok(snapshot.protoProps.includes('send'), 'protoProps 应含 send');
  assert.match(snapshot.openText, /open/);
  assert.match(snapshot.sendText, /send/);
});

// 真机轨用的**探针抄本**与判据导出必须逐字一致 ——
// 探针在 `src-tauri/examples/`（Rust），判据在 `scripts/`（JS）⇒ 跨文件、跨语言 ⇒
// 机器拦（不许只靠注释保证；也不许为了好写把抄本改成别的内容来「凑相等」）。
test('真机轨 · 探针里的快照脚本抄本与判据导出**逐字相等**（防抄本漂移）', () => {
  const probePath = join(
    fileURLToPath(new URL('../', import.meta.url)),
    'src-tauri/examples/official_chat_devtools_network_probe.rs',
  );
  const src = readFileSync(probePath, 'utf8');
  const m = /const TRACE_SNAPSHOT_JS: &str = r#"([\s\S]*?)"#;/.exec(src);
  assert.ok(m, `探针里找不到 TRACE_SNAPSHOT_JS 常量（${probePath}）`);
  assert.equal(
    m[1],
    TRACE_SNAPSHOT_JS,
    '探针里的抄本必须与判据导出逐字相等 —— 要改就改判据那份、再让两边一致',
  );
});

test('页内探针 · 源码里没有全局赋值（结构护栏）', () => {  assert.ok(!/globalThis\s*=[^=]/.test(TRACE_SNAPSHOT_JS), '探针不得写 globalThis');
  assert.ok(!/\bwindow\s*=/.test(TRACE_SNAPSHOT_JS), '探针不得写 window');
  // 整体必须是一个 IIFE ⇒ 内部的 const/let 都是**函数作用域** ⇒ 不会变成全局量
  const src = TRACE_SNAPSHOT_JS.trim();
  assert.ok(src.startsWith('(() => {'), '探针必须整体是一个 IIFE');
  assert.ok(src.endsWith('})()'), '探针必须整体是一个 IIFE');
});

test('真机轨 · 没给读数 ⇒ 报未执行（不是通过、不是失败），且指向「壳侧 Network 域订阅跑一轮」', () => {
  const verdict = realRunVerdict();
  assert.equal(verdict.status, SKIPPED);
  assert.match(verdict.detail, /未执行/);
  assert.match(verdict.detail, /Network/, '必须指向「壳侧 Network 域订阅跑一轮」这个被绑形态');
  assert.ok(
    !/页内取数脚本/.test(verdict.detail),
    '不许再等「P4 页内取数脚本」—— 那个形态已被 v0.10 点名排除',
  );
});

test('真机轨 · 给了读数 ⇒ 真判（页内零痕迹 ⇒ 通过；有痕迹 ⇒ 失败）', () => {
  const clean = realRunVerdict({ before: fakeSnapshot(), after: fakeSnapshot() });
  assert.equal(clean.status, PASS, `干净读数必须通过：${clean.detail}`);

  const dirty = realRunVerdict({
    before: fakeSnapshot(),
    after: fakeSnapshot({ props: ['location', 'navigator', 'XMLHttpRequest', '__dsh_rt_1'] }),
  });
  assert.equal(dirty.status, FAIL, '有 `__dsh*` 槽位的读数必须判失败');
});

test('口径护栏 · **形态 A** 的断言名里不许出现「还原后」（协议 §10 第 2 条在形态 A 下不许写成「还原后一致」）', () => {
  const { checks } = judgeTrace({ before: fakeSnapshot(), after: fakeSnapshot() }, FORM_A);
  for (const c of checks) {
    assert.ok(!c.name.includes('还原后'), `断言名不许带「还原后」：${c.name}`);
  }
});

test('口径护栏 · **形态 C** 的断言名里必须出现「还原」（它恰恰就是 v0.7 那条口径）', () => {
  const { checks } = judgeTrace(
    { before: fakeSnapshot(), after: fakeSnapshot() },
    FORM_C,
  );
  const names = checks.map((c) => c.name);
  assert.ok(
    names.some((n) => n.includes('还原后引用 ＝ 替换前引用')),
    `形态 C 必须有「还原后引用 ＝ 替换前引用」那条（实际：${names.join('；')}）`,
  );
});

test('DSH 前缀常量与协议 §7.1 一致', () => {
  assert.equal(DSH_PREFIX, '__dsh');
});

test('主入口 · 全绿且三态自洽', async () => {
  const { results, summary } = await main(['--round', 'p4c2-selftest', '--json']);
  assert.equal(summary.fail, 0, `不该有失败：${JSON.stringify(summary)}`);
  assert.ok(summary.total >= 9);
  assert.equal(pick(results, '2z').status, SKIPPED);
  assert.ok(results.every((r) => [PASS, FAIL, SKIPPED].includes(r.status)));
});

test('读数文件缺失或损坏仍能输出未执行结果', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'dsh-p4-invalid-readings-'));
  const previous = process.env.DSH_P4_TRACE_READINGS;
  try {
    const file = join(dir, 'readings.json');
    process.env.DSH_P4_TRACE_READINGS = file;
    for (const corrupt of [false, true]) {
      if (corrupt) writeFileSync(file, '{invalid');
      const { results } = await main(['--round', `invalid-${Date.now()}-${corrupt}`, '--json']);
      const real = pick(results, '2z');
      assert.equal(real.status, SKIPPED);
      assert.ok(real.detail.includes(file));
    }
  } finally {
    if (previous === undefined) delete process.env.DSH_P4_TRACE_READINGS;
    else process.env.DSH_P4_TRACE_READINGS = previous;
    rmSync(dir, { recursive: true, force: true });
  }
});

test('喂读数的入口 · `DSH_P4_TRACE_READINGS` 指向的 JSON 会被真判（不是占位）', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'dsh-p4-trace-selftest-'));
  const file = join(dir, 'real-readings.json');
  try {
    writeFileSync(
      file,
      JSON.stringify({ before: fakeSnapshot(), after: fakeSnapshot() }),
      'utf8',
    );
    process.env.DSH_P4_TRACE_READINGS = file;
    const { results } = await main(['--round', 'p4c2-env', '--json']);
    const real = pick(results, '2z');
    assert.equal(real.status, PASS, `给了干净读数就应真判通过：${real.detail}`);
  } finally {
    delete process.env.DSH_P4_TRACE_READINGS;
    rmSync(dir, { recursive: true, force: true });
  }
});

// ── 按形态分（izzy 2026-10-23 上午拍板）────────────────────────────────────
//
// 形态 A｜壳侧 DevTools `Network` 域订阅（页内零动作）｜
// 形态 B｜页内自取数（路线 B：页内脚本自己取自己回，**一个**随机名槽位、取到即删）｜
// 形态 C｜页内旁听（替换 `XMLHttpRequest.prototype`，**取到即还原成替换前那个引用**）

test('形态 A · 既有八条断言的名字逐字钉住（不许被改口径改掉）', () => {
  const { checks } = judgeTrace({ before: fakeSnapshot(), after: fakeSnapshot() }, FORM_A);
  const names = checks.map((c) => c.name);
  for (const expected of FORM_A_BASE_CHECKS) {
    assert.ok(names.includes(expected), `形态 A 的断言名不许变：缺「${expected}」`);
  }
});

test('形态 A · 八条断言 · **基线 JSON 逐字比对**（名字 ＋ 顺序 ＋ 集合 ＋ sha256）', () => {
  const { checks } = judgeTrace({ before: fakeSnapshot(), after: fakeSnapshot() }, FORM_A);
  const names = checks.map((c) => c.name);
  // ① 取那八条（**按它们在断言数组里的先后**）⇒ 顺序也被钉住。
  const picked = FORM_A_BASE_CHECKS.map((n) => {
    const at = names.indexOf(n);
    assert.ok(at >= 0, `形态 A 的断言名不许变：缺「${n}」`);
    return { n, at };
  });
  const ordered = [...picked].sort((a, b) => a.at - b.at).map((x) => x.n);
  const json = JSON.stringify(ordered, null, 2);
  assert.equal(json, FORM_A_BASELINE_JSON, '形态 A 的八条断言必须与基线 JSON 逐字一致');
  assert.equal(
    createHash('sha256').update(json).digest('hex'),
    FORM_A_BASELINE_SHA256,
    '基线 JSON 的 sha256 必须一致（连空白都不许变）',
  );
  // ② **集合**：除那八条 ＋ 形态专属那一条之外，**不许多出别的断言**（不许偷偷放宽）。
  const extra = names.filter((n) => !FORM_A_BASE_CHECKS.includes(n) && n !== FORM_A_ONLY_CHECK);
  assert.deepEqual(extra, [], `形态 A 不许多出断言：${extra.join('；')}`);
  assert.ok(names.includes(FORM_A_ONLY_CHECK), '形态 A 的形态专属那一条不许删');
});

test('形态 B · 断言集合**包含**形态 A 的全部断言（取数结束后必须回到取数前）', () => {
  const { checks } = judgeTrace(
    { before: fakeSnapshot(), mid: fakeSnapshot({ props: [...BASE_PROPS, SLOT] }), after: fakeSnapshot() },
    FORM_B,
  );
  const bNames = checks.map((c) => c.name);
  for (const n of FORM_A_BASE_CHECKS) {
    assert.ok(bNames.includes(n), `形态 B 必须包含形态 A 的断言：${n}`);
  }
  // ⚠️ **形态 A 那条「中途快照也无页内变化」按设计不属于形态 B** ——
  //   形态 B 的中途快照**就该**有那一个槽位 ⇒ ⇒ 它不是「形态 A 的断言」的那一份。
  assert.ok(
    !bNames.some((n) => n.includes('形态 A · 中途快照也无页内变化')),
    '形态 B 不许继承形态 A 的「中途零变化」那条（形态 B 的中途有槽位）',
  );
});

test('形态 B · 中途恰好那一个槽位、取数后消失 ⇒ 通过', () => {
  const { ok, checks } = judgeTrace(
    { before: fakeSnapshot(), mid: fakeSnapshot({ props: [...BASE_PROPS, SLOT] }), after: fakeSnapshot() },
    FORM_B,
  );
  assert.equal(ok, true, `形态 B 的正常读数必须通过：${JSON.stringify(checks)}`);
  assert.equal(statusOf(checks, '取数进行中：`__dsh*` 全局量恰好 1 个'), PASS);
  assert.equal(statusOf(checks, '取数进行中：那一个全局量形如'), PASS);
  assert.equal(statusOf(checks, '临时槽位已消失'), PASS);
  assert.equal(statusOf(checks, '`__dsh*` 全局量 ＝ 0'), PASS);
});

test('形态 B · 取数结束后槽位仍在 ⇒ 判红（点名的断言必须报红）', () => {
  const { ok, checks } = judgeTrace(
    {
      before: fakeSnapshot(),
      mid: fakeSnapshot({ props: [...BASE_PROPS, SLOT] }),
      after: fakeSnapshot({ props: [...BASE_PROPS, SLOT] }),
    },
    FORM_B,
  );
  assert.equal(ok, false, '取到即删没做到 ⇒ 必须判红');
  assert.equal(statusOf(checks, '临时槽位已消失'), FAIL);
  assert.equal(statusOf(checks, '`__dsh*` 全局量 ＝ 0'), FAIL);
});

test('形态 B · 缺 mid（中途）快照 ⇒ 判红（不许按「没喂就当没有」判绿）', () => {
  const { ok, checks } = judgeTrace({ before: fakeSnapshot(), after: fakeSnapshot() }, FORM_B);
  assert.equal(ok, false, '缺中途快照 ⇒ 形态 B 的中途约束没法验 ⇒ 必须判红');
  assert.equal(statusOf(checks, '取数进行中：`__dsh*` 全局量恰好 1 个'), FAIL);
});

test('形态 B · 中途有**两个** `__dsh*` ⇒ 判红（只允许那一个槽位）', () => {
  const { ok, checks } = judgeTrace(
    {
      before: fakeSnapshot(),
      mid: fakeSnapshot({ props: [...BASE_PROPS, SLOT, '__dsh_rt_dead'] }),
      after: fakeSnapshot(),
    },
    FORM_B,
  );
  assert.equal(ok, false, '中途多一个全局量 ⇒ 必须判红');
  assert.equal(statusOf(checks, '取数进行中：`__dsh*` 全局量恰好 1 个'), FAIL);
});

test('形态 B · 中途那一个**不是槽位形状** ⇒ 判红', () => {
  const { ok, checks } = judgeTrace(
    {
      before: fakeSnapshot(),
      mid: fakeSnapshot({ props: [...BASE_PROPS, '__dsh_other_1'] }),
      after: fakeSnapshot(),
    },
    FORM_B,
  );
  assert.equal(ok, false, '中途的全局量不是槽位 ⇒ 必须判红');
  assert.equal(statusOf(checks, '取数进行中：那一个全局量形如'), FAIL);
});

test('形态 B · 中途多了一个**非槽位**的 `window` 属性 ⇒ 判红', () => {
  const { ok, checks } = judgeTrace(
    {
      before: fakeSnapshot(),
      mid: fakeSnapshot({ props: [...BASE_PROPS, SLOT, 'leak_1'] }),
      after: fakeSnapshot(),
    },
    FORM_B,
  );
  assert.equal(ok, false, '中途新增别的 window 属性 ⇒ 必须判红');
  assert.equal(statusOf(checks, '除那一个槽位外 `window` 自有属性逐项一致'), FAIL);
});

test('形态 C · 换过但**已还原**（还原后引用 ＝ 替换前引用）⇒ 通过', () => {
  const { ok, checks } = judgeTrace(
    {
      before: fakeSnapshot(),
      mid: fakeSnapshot({ openText: 'function open() { /* wrapper */ }' }),
      after: fakeSnapshot(),
    },
    FORM_C,
  );
  assert.equal(ok, true, `形态 C 的正常读数必须通过：${JSON.stringify(checks)}`);
  assert.equal(statusOf(checks, '`open` 还原后引用 ＝ 替换前引用'), PASS);
  assert.equal(statusOf(checks, '`send` 还原后引用 ＝ 替换前引用'), PASS);
  assert.equal(statusOf(checks, 'window 自有属性前后逐项一致'), PASS);
  assert.equal(statusOf(checks, '`__dsh*` 全局量 ＝ 0'), PASS);
});

test('形态 C · 换过**没还原** ⇒ 判红（还原后引用 ≠ 替换前引用）', () => {
  const wrapper = 'function open() { /* wrapper */ }';
  const { ok, checks } = judgeTrace(
    {
      before: fakeSnapshot(),
      mid: fakeSnapshot({ openText: wrapper }),
      after: fakeSnapshot({ openText: wrapper }),
    },
    FORM_C,
  );
  assert.equal(ok, false, '换过但没还原 ⇒ 必须判红');
  assert.equal(statusOf(checks, '`open` 还原后引用 ＝ 替换前引用'), FAIL);
});

test('形态 C · 还原成**别的**引用（不是替换前那个）⇒ 判红', () => {
  const { ok, checks } = judgeTrace(
    {
      before: fakeSnapshot({ openText: 'function open() { /* site wrapper */ }' }),
      mid: fakeSnapshot({ openText: 'function open() { /* ours */ }' }),
      after: fakeSnapshot({ openText: 'function open() { [native code] }' }),
    },
    FORM_C,
  );
  assert.equal(ok, false, '还原成原生而不是替换前那个引用 ⇒ 必须判红');
  assert.equal(statusOf(checks, '`open` 还原后引用 ＝ 替换前引用'), FAIL);
});

test('形态 C · 留了 `__dsh*` 槽位 ⇒ 判红（形态 C 不许槽位）', () => {
  const { ok, checks } = judgeTrace(
    {
      before: fakeSnapshot(),
      mid: fakeSnapshot({ openText: 'function open() { /* wrapper */ }' }),
      after: fakeSnapshot({ props: [...BASE_PROPS, SLOT] }),
    },
    FORM_C,
  );
  assert.equal(ok, false, '形态 C 留槽位 ⇒ 必须判红');
  assert.equal(statusOf(checks, '`__dsh*` 全局量 ＝ 0'), FAIL);
});

test('形态 C · 中途新增了 `window` 自有属性 ⇒ 判红（不新增 window 自有属性）', () => {
  const { ok, checks } = judgeTrace(
    {
      before: fakeSnapshot(),
      mid: fakeSnapshot({
        openText: 'function open() { /* wrapper */ }',
        props: [...BASE_PROPS, 'leak_1'],
      }),
      after: fakeSnapshot(),
    },
    FORM_C,
  );
  assert.equal(ok, false, '中途新增 window 属性 ⇒ 必须判红');
  assert.equal(statusOf(checks, '取数进行中：允许换过的 `open`'), FAIL);
});

test('未知形态 ⇒ 判据无法执行 ⇒ 判失败（不许默默按某一形态判）', () => {
  const { ok, checks } = judgeTrace({ before: fakeSnapshot(), after: fakeSnapshot() }, 'z');
  assert.equal(ok, false);
  assert.equal(checks[0].status, FAIL);
});

// ── 离线源码形状（形态 B／C 的链；能离线跑、不用真机）────────────────────────

/** 形态 C 的**合成** hook 脚本（合成夹具，不是真身）。 */
const FORM_C_HOOK_JS = `(function () {
  var origOpen = XMLHttpRequest.prototype.open;
  var origSend = XMLHttpRequest.prototype.send;
  XMLHttpRequest.prototype.open = function () { return origOpen.apply(this, arguments); };
  XMLHttpRequest.prototype.send = function () { return origSend.apply(this, arguments); };
  try {
    hookBody();
  } finally {
    XMLHttpRequest.prototype.open = origOpen;
    XMLHttpRequest.prototype.send = origSend;
  }
})()`;

/**
 * 某段文本的 sha256（用来证明「原件一个字节都没动」）。
 *
 * @param {string} text - 文本。
 * @returns {string}
 */
function sha256(text) {
  return createHash('sha256').update(text).digest('hex');
}

test('离线源码形状 · 形态 B 的链（真身）：IIFE ／ 无定时器 ／ 取到即删 ⇒ 全过', () => {
  const sources = collectChainSources(REPO);
  const { ok, checks } = judgeFormBChain(sources);
  assert.equal(ok, true, `形态 B 的链必须全过：${checks.map((c) => `${c.name}=${c.status}`).join('；')}`);
  assert.equal(statusOf(checks, '注入脚本是 IIFE'), PASS);
  assert.equal(statusOf(checks, '注入脚本里没有定时器'), PASS);
  assert.equal(statusOf(checks, '取数结束后槽位被清'), PASS);
});

test('离线源码形状 · 形态 B 的链：反向自证（改坏 ⇒ 对应那条必须红；**原件逐字节不变**）', () => {
  const rel = 'src-tauri/src/official_chat/script_eval.rs';
  const original = readFileSync(join(REPO, rel), 'utf8');
  const before = sha256(original);
  const judged = (code) => judgeFormBChain([{ path: rel, code }]);

  // ① 注入脚本不再是 IIFE（顶层多了一句赋值）⇒ 「是 IIFE」那条必须红
  const notIife = original.replace('(function () {', 'var __dsh_leak = 1; (function () {');
  assert.notEqual(notIife, original, '改坏没生效 ⇒ 锚点不存在');
  {
    const { ok, checks } = judged(notIife);
    assert.equal(ok, false, '注入脚本不是 IIFE ⇒ 必须判红');
    assert.equal(statusOf(checks, '注入脚本是 IIFE'), FAIL);
  }

  // ② 注入脚本里出现定时器 ⇒ 「没有定时器」那条必须红
  const withTimer = original.replace('  put({{ state: "pending", startedAt: Date.now() }});',
    '  setInterval(function () {}, 1000);\n  put({{ state: "pending", startedAt: Date.now() }});');
  assert.notEqual(withTimer, original, '改坏没生效 ⇒ 锚点不存在');
  {
    const { ok, checks } = judged(withTimer);
    assert.equal(ok, false, '注入脚本留定时器 ⇒ 必须判红');
    assert.equal(statusOf(checks, '注入脚本里没有定时器'), FAIL);
  }

  // ③ `clear_slot` 不删那个槽位 ⇒ 「取到即删」那条必须红
  const noDelete = original.replace('try {{ delete window[{}]; }}', 'try {{ void window[{}]; }}');
  assert.notEqual(noDelete, original, '改坏没生效 ⇒ 锚点不存在');
  {
    const { ok, checks } = judged(noDelete);
    assert.equal(ok, false, '槽位没被删 ⇒ 必须判红');
    assert.equal(statusOf(checks, '取数结束后槽位被清'), FAIL);
  }

  // 恢复后逐字节一致（原件从头到尾没被写过）
  assert.equal(sha256(readFileSync(join(REPO, rel), 'utf8')), before, '原件必须逐字节不变');
  assert.equal(judged(original).ok, true, '恢复后必须重新变绿');
});

test('离线源码形状 · 形态 C 的链（合成夹具）：IIFE ／ 有还原 ／ 无定时器 ／ 不写站点数据 ⇒ 全过', () => {
  const { ok, checks } = judgeFormCChain([
    { path: 'src-tauri/src/official_chat/page_hook_script.js', code: FORM_C_HOOK_JS },
  ]);
  assert.equal(ok, true, `形态 C 的链必须全过：${checks.map((c) => `${c.name}=${c.status}`).join('；')}`);
  assert.equal(statusOf(checks, 'hook 脚本是 IIFE'), PASS);
  assert.equal(statusOf(checks, '有「还原」动作'), PASS);
  assert.equal(statusOf(checks, 'hook 脚本里没有定时器'), PASS);
  assert.equal(statusOf(checks, '不写站点数据'), PASS);
});

test('离线源码形状 · 形态 C 的链：反向自证（逐条改坏 ⇒ 对应那条必须红；夹具逐字节可还原）', () => {
  const judged = (code) =>
    judgeFormCChain([{ path: 'src-tauri/src/official_chat/page_hook_script.js', code }]);
  const original = FORM_C_HOOK_JS;
  const before = sha256(original);

  // ① 不是 IIFE ⇒ 「是 IIFE」那条必须红
  {
    const mutated = original.replace('(function () {', 'var __dsh_leak = 1; (function () {');
    assert.notEqual(mutated, original, '改坏没生效');
    const { ok, checks } = judged(mutated);
    assert.equal(ok, false, '不是 IIFE ⇒ 必须判红');
    assert.equal(statusOf(checks, 'hook 脚本是 IIFE'), FAIL);
  }

  // ② 没有还原动作 ⇒ 「有还原」那条必须红
  {
    const mutated = original
      .replace('    XMLHttpRequest.prototype.open = origOpen;\n', '')
      .replace('    XMLHttpRequest.prototype.send = origSend;\n', '');
    assert.notEqual(mutated, original, '改坏没生效');
    const { ok, checks } = judged(mutated);
    assert.equal(ok, false, '没还原 ⇒ 必须判红');
    assert.equal(statusOf(checks, '有「还原」动作'), FAIL);
  }

  // ③ 留定时器 ⇒ 「没有定时器」那条必须红
  {
    const mutated = original.replace('  try {\n    hookBody();', '  setTimeout(function () {}, 10);\n  try {\n    hookBody();');
    assert.notEqual(mutated, original, '改坏没生效');
    const { ok, checks } = judged(mutated);
    assert.equal(ok, false, '留定时器 ⇒ 必须判红');
    assert.equal(statusOf(checks, 'hook 脚本里没有定时器'), FAIL);
  }

  // ④ 写站点数据 ⇒ 「不写站点数据」那条必须红
  {
    const mutated = original.replace('    hookBody();', '    localStorage.setItem("x", "1");\n    hookBody();');
    assert.notEqual(mutated, original, '改坏没生效');
    const { ok, checks } = judged(mutated);
    assert.equal(ok, false, '写站点数据 ⇒ 必须判红');
    assert.equal(statusOf(checks, '不写站点数据'), FAIL);
  }

  // 还原后逐字节一致（夹具原文没被动过）
  assert.equal(sha256(original), before, '夹具必须逐字节不变');
  assert.equal(judged(original).ok, true, '恢复后必须重新变绿');
});

// ── 形态归属：按 **cfg 段** 切分（PM 2026-10-23 拍板）────────────────────────
//
// PM 的裁定：**形态归属按 `cfg` 块判，不引入新的标记约定** ⇒ ⇒ 判据自己得会
// 「这段代码属于哪个形态」⇒ ⇒ 这一组用例把那条切分规则**逐条钉住**。

test('cfg 谓词求值 · windows／macos 两侧的真值（`not`／`any`／`all`／`test`／判不了）', () => {
  assert.equal(evalCfgPredicate('windows', 'windows'), true);
  assert.equal(evalCfgPredicate('windows', 'macos'), false);
  assert.equal(evalCfgPredicate('target_os = "macos"', 'macos'), true);
  assert.equal(evalCfgPredicate('target_os = "macos"', 'windows'), false);
  assert.equal(evalCfgPredicate('not(windows)', 'macos'), true);
  assert.equal(evalCfgPredicate('not(windows)', 'windows'), false);
  assert.equal(evalCfgPredicate('any(windows, target_os = "macos")', 'windows'), true);
  assert.equal(evalCfgPredicate('any(windows, target_os = "macos")', 'macos'), true);
  assert.equal(evalCfgPredicate('not(any(windows, target_os = "macos"))', 'windows'), false);
  assert.equal(evalCfgPredicate('not(any(windows, target_os = "macos"))', 'macos'), false);
  // ⚠️ `test` ＝ **生产构建** ⇒ 判假（测试段**不进产品二进制** ⇒ 不是任何形态的运行路径）。
  assert.equal(evalCfgPredicate('test', 'windows'), false);
  assert.equal(evalCfgPredicate('all(test, windows)', 'windows'), false);
  // ⚠️ **判不了的 ⇒ `null`**（保守：调用方按「两边都算」处理，不许漏判）。
  assert.equal(evalCfgPredicate('feature = "x"', 'windows'), null);
  assert.equal(evalCfgPredicate('', 'windows'), null);
});

test('cfg 段的种类 · windows／macos／两边／测试／公共各归各的', () => {
  assert.equal(cfgSegmentKind('windows'), SEG_WINDOWS);
  assert.equal(cfgSegmentKind('target_os = "macos"'), SEG_MACOS);
  assert.equal(cfgSegmentKind('any(windows, target_os = "macos")'), SEG_BOTH);
  assert.equal(cfgSegmentKind('test'), SEG_OTHER);
  assert.equal(cfgSegmentKind('not(any(windows, target_os = "macos"))'), SEG_OTHER);
});

test('cfg 段切分 · 公共段 ＝「文件里没挂 cfg 的那部分」；测试段 ＝ 不属任何形态', () => {
  const src = [
    'const COMMON = 1;',
    '#[cfg(windows)]',
    'fn only_windows() {}',
    '#[cfg(target_os = "macos")]',
    'fn only_macos() {}',
    '#[cfg(any(windows, target_os = "macos"))]',
    'fn both() {}',
    '#[cfg(test)]',
    'mod tests { fn t() {} }',
  ].join('\n');
  const segs = segmentRustByCfg(src);
  const kindAt = (needle) => segs.find((s) => s.code.includes(needle))?.kind;
  assert.equal(kindAt('fn only_windows'), SEG_WINDOWS);
  assert.equal(kindAt('fn only_macos'), SEG_MACOS);
  assert.equal(kindAt('fn both'), SEG_BOTH);
  assert.equal(kindAt('mod tests'), SEG_OTHER);
  assert.equal(kindAt('const COMMON'), SEG_COMMON);
  // 段必须**首尾相接、覆盖全文**（不许有漏掉的字节 ⇒ 不许有「没被判到」的洞）。
  assert.equal(segs[0].start, 0);
  assert.equal(segs[segs.length - 1].end, src.length);
  for (let i = 1; i < segs.length; i += 1) {
    assert.equal(segs[i].start, segs[i - 1].end, `段 ${i} 与上一段之间不许有洞`);
  }
});

test('cfg 段切分 · 原始字符串里的花括号不许把段边界带偏（`r#"…"#` 里有 `{}`）', () => {
  const src = [
    '#[cfg(windows)]',
    'fn f() {',
    '    let s = r#"const r = await fetch(\'/api/v0/x\'); { let y = 1; }"#;',
    '}',
    '#[cfg(target_os = "macos")]',
    'fn g() {}',
  ].join('\n');
  const segs = segmentRustByCfg(src);
  const kindAt = (needle) => segs.find((s) => s.code.includes(needle))?.kind;
  assert.equal(kindAt('fn f'), SEG_WINDOWS, '原始字符串里的 `{` 不许把 windows 段截断');
  assert.equal(kindAt('fn g'), SEG_MACOS);
});

test('形态归属 · windows 段 → 形态 A；macos 段 → 形态 B；公共段 → 文件主人；**无主 ⇒ 形态 A**（fail-safe）', () => {
  const src = [
    'const COMMON = 1;',
    '#[cfg(windows)]',
    'fn w() {}',
    '#[cfg(target_os = "macos")]',
    'fn m() {}',
  ].join('\n');
  const owned = 'src-tauri/src/official_chat/session_structure.rs';
  const unowned = 'src-tauri/src/official_chat/whatever.rs';
  const sources = [
    { path: owned, code: src },
    { path: unowned, code: src },
  ];
  const { byForm, unattributed } = attributeByForm(sources, { [owned]: 'a' });
  const a = byForm.a.map((s) => s.code).join('\n');
  const b = byForm.b.map((s) => s.code).join('\n');
  assert.match(a, /fn w\(\)/, 'windows 段 ⇒ 形态 A');
  assert.match(b, /fn m\(\)/, 'macOS 段 ⇒ 形态 B');
  assert.doesNotMatch(a, /fn m\(\)/, 'macOS 段**不许**落进形态 A');
  assert.doesNotMatch(b, /fn w\(\)/, 'windows 段**不许**落进形态 B');
  // ⚠️ **无主文件的公共段 ⇒ 归属不明 ⇒ 按形态 A 判**（PM：故意的严格，不放行）。
  assert.ok(
    unattributed.some((s) => s.path === unowned && s.kind === SEG_COMMON),
    '无主文件的公共段必须登记成「归属不明」',
  );
});
