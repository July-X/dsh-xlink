/**
 * cfg 门禁配套自检的测试。
 *
 * 最重要的一条是**反向验证**：把修复整段删掉，判据必须转红。
 * 恒绿的判据等于没有判据。
 */

import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import test from 'node:test';

import {
  REPO_ROOT,
  analyzeRepo,
  buildEnvironments,
  evalCfg,
  maskRust,
  parseCfgPredicate,
  parseImplHeader,
  parseRustFile,
} from './check-cfg-gates.mjs';

const LOCALTIME = 'src-tauri/src/shell/localtime.rs';

test('环境空间穷举到位：含 macOS × 非 test 那一格', () => {
  const envs = buildEnvironments();
  assert.equal(envs.length, 80);
  // macOS × x86_64 × 非 test × 非 debug_assertions：feature 维度还剩开关两档。
  const macosRelease = envs.filter(
    (e) => e.os === 'macos' && !e.test && !e.debug_assertions && e.arch === 'x86_64',
  );
  assert.equal(macosRelease.length, 2, 'macOS 非 test 的那一格必须在环境空间里');
  assert.equal(envs.filter((e) => e.os === 'windows').length, 16);
});

test('cfg 谓词求值：跨平台原子按目标 OS 走', () => {
  const envs = buildEnvironments();
  const mac = envs.find((e) => e.os === 'macos' && !e.test);
  const win = envs.find((e) => e.os === 'windows' && !e.test);
  const anyWinTest = parseCfgPredicate('any(windows, test)');
  assert.equal(evalCfg(anyWinTest, mac), false);
  assert.equal(evalCfg(anyWinTest, win), true);
  const macosOnly = parseCfgPredicate('target_os = "macos"');
  assert.equal(evalCfg(macosOnly, mac), true);
  assert.equal(evalCfg(macosOnly, win), false);
  const notWin = parseCfgPredicate('not(windows)');
  assert.equal(evalCfg(notWin, mac), true);
  assert.equal(evalCfg(notWin, win), false);
});

test('遮罩不吞属性里的值：#[path] 与 target_os 都要看得见', () => {
  const source = [
    '#[path = "child.rs"]',
    'mod child;',
    '',
    '#[cfg(target_os = "macos")]',
    'struct OnlyMacos;',
  ].join('\n');
  const parsed = parseRustFile(source);
  assert.equal(parsed.mods[0].pathAttr, 'child.rs');
  const gate = parsed.items.find((i) => i.name === 'OnlyMacos');
  assert.equal(gate.ownRaw.length, 1);
  assert.equal(gate.ownRaw[0], '#[cfg(target_os = "macos")]');
});

test('遮罩不把注释里的 #[cfg(...)] 当真属性', () => {
  const masked = maskRust('// #[cfg(windows)]\nfn f() {}');
  assert.equal(masked.includes('cfg(windows)'), false);
});

test('impl 头解析：trait impl 与泛型都要取对主体类型', () => {
  assert.deepEqual(parseImplHeader(' Foo<T> '), { traitName: null, subject: 'Foo' });
  assert.deepEqual(parseImplHeader(' Display for Foo '), { traitName: 'Display', subject: 'Foo' });
  assert.deepEqual(parseImplHeader(`<'a> Display for Foo<'a>`), {
    traitName: 'Display',
    subject: 'Foo',
  });
});

test('真实仓库：impl 块的门控不得比被实现的类型更宽', () => {
  const { violations } = analyzeRepo(REPO_ROOT);
  assert.deepEqual(
    violations.map((v) => `${v.file}:${v.line} ${v.kindLabel}`),
    [],
  );
});

test('反向验证：摘掉 impl TransitionRule 的门控，判据必须转红', () => {
  const original = readFileSync(join(REPO_ROOT, LOCALTIME), 'utf8');
  const broken = original.replace(
    /#\[cfg\(any\(windows, test\)\)\]\nimpl TransitionRule \{/,
    'impl TransitionRule {',
  );
  assert.notEqual(broken, original, '变异必须真的落到源码上，否则这条测试是恒绿的');

  const { violations } = analyzeRepo(REPO_ROOT, {
    overrides: new Map([[LOCALTIME, broken]]),
  });
  const hit = violations.filter((v) => v.subject === 'TransitionRule');
  assert.equal(hit.length, 1, '摘掉门控后必须且只报 TransitionRule 一条');
  assert.equal(hit[0].file, LOCALTIME);

  // 报出的环境里必须有 macOS × 非 test：当初把 CI 拉红的就是那一格。
  const envs = buildEnvironments();
  const macosNonTest = envs.find(
    (e) => e.os === 'macos' && !e.test && e.arch === 'x86_64' && !e.debug_assertions,
  );
  assert.ok(
    hit[0].broken.some((e) => e.index === macosNonTest.index),
    '必须覆盖 macOS 非 test 那一格',
  );

  // 修复放回去，违例必须消失。
  const restored = analyzeRepo(REPO_ROOT).violations;
  assert.deepEqual(restored, []);
});
