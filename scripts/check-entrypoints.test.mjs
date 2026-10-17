import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const read = (path) => readFileSync(new URL(path, import.meta.url), 'utf8');
const { scripts } = JSON.parse(read('../package.json'));

function commands(name, definitions = scripts, parents = []) {
  assert.ok(!parents.includes(name), `循环检查入口：${[...parents, name].join(' → ')}`);
  assert.ok(definitions[name], `缺少检查入口：${name}`);
  return definitions[name].split(' && ').flatMap((command) => {
    const nested = command.match(/^npm run (\S+)$/);
    return nested ? commands(nested[1], definitions, [...parents, name]) : [command];
  });
}

function assertJavaScriptGates(definitions = scripts) {
  const actual = commands('check:js', definitions);
  for (const name of [
    'check:invariants', 'check:code-budget', 'check:webdriver-gating', 'check:builtin',
    'test:scripts', 'test:ui', 'test:smoke', 'test:plugin', 'build:ui', 'check:ui-bundle',
  ]) {
    for (const command of commands(name, definitions)) {
      assert.equal(actual.filter((item) => item === command).length, 1, `${name} 必须执行且只执行一次`);
    }
  }
  assert.ok(actual.indexOf(definitions['build:ui']) < actual.indexOf(definitions['check:ui-bundle']));
}

test('共用 JavaScript 清单覆盖每项保护，漏掉插件测试时会拒绝', () => {
  assertJavaScriptGates();
  assert.throws(() => assertJavaScriptGates({
    ...scripts, 'check:js': scripts['check:js'].replace('npm run test:plugin && ', ''),
  }), /test:plugin/);
});

test('静态检查不编译 Rust；发布总闸保留单测、零警告和 release 条件编译检查', () => {
  assert.ok(commands('check:static').every((command) => !command.includes('cargo ')));
  const ci = commands('check:rust:ci');
  assert.ok(ci.some((command) => /^cargo fmt\b/.test(command) && command.endsWith('--check')));
  assert.ok(ci.some((command) => /^cargo test\b/.test(command) && command.includes('--locked')));
  assert.ok(ci.some((command) => /^cargo clippy\b/.test(command) && command.includes('--locked') && command.endsWith('-- -D warnings')));
  const full = commands('check');
  for (const command of [...commands('check:js'), ...ci]) {
    assert.equal(full.filter((item) => item === command).length, 1, `发布总闸漏接或重复：${command}`);
  }
  assert.ok(full.some((command) => /^cargo check\b/.test(command) && command.includes('--release') && command.includes('--all-targets') && command.includes('--locked')));
  assert.equal(new Set(full).size, full.length, '总闸不应重复执行同一命令');
});

test('日常与发布的 quality job 共用清单，并先生成内置资源', () => {
  for (const name of ['desktop-ci', 'desktop-release']) {
    const source = read(`../.github/workflows/${name}.yml`);
    const quality = source.match(/^  quality:\n([\s\S]*?)(?=^  \S|$(?![\s\S]))/m)?.[1];
    assert.ok(quality, `${name} 缺少 quality job`);
    const runs = [...quality.matchAll(/^\s+run: (.+)$/gm)].map((match) => match[1]);
    for (const command of ['pnpm run prep:builtin', 'pnpm run check:js', 'pnpm run check:rust:ci']) {
      assert.equal(runs.filter((run) => run === command).length, 1, `${name}: ${command}`);
    }
    assert.ok(runs.indexOf('pnpm run prep:builtin') < runs.indexOf('pnpm run check:js'));
    assert.ok(runs.indexOf('pnpm run prep:builtin') < runs.indexOf('pnpm run check:rust:ci'));
    assert.ok(!runs.some((run) => /^cargo\b/.test(run)), 'Rust 清单应复用 package.json');
  }
});
