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
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';
import test from 'node:test';

import {
  DSH_PREFIX,
  FAIL,
  PASS,
  SKIPPED,
  TRACE_SNAPSHOT_JS,
  buildScenarios,
  fakeSnapshot,
  judgeTrace,
  main,
  realRunVerdict,
  runScenarios,
  summarize,
} from './check-official-chat-p4-trace.mjs';

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

test('场景表 · 9 个场景的判定全部符合预期', () => {
  const scenarios = buildScenarios();
  assert.equal(scenarios.length, 9);
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

test('口径护栏 · 断言名里不许出现「还原后」（协议 §10 第 2 条明文不许写成「还原后一致」）', () => {
  const { checks } = judgeTrace({ before: fakeSnapshot(), after: fakeSnapshot() });
  for (const c of checks) {
    assert.ok(!c.name.includes('还原后'), `断言名不许带「还原后」：${c.name}`);
  }
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
