import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { checkQualityResults } from './check-quality-results.mjs';

const successful = {
  'quality-js': { result: 'success' },
  'quality-rust': { result: 'success' },
};

test('quality aggregate requires both jobs and rejects missing, skipped, cancelled and failed results', () => {
  assert.deepEqual(checkQualityResults(successful), []);
  for (const name of Object.keys(successful)) {
    for (const result of ['failure', 'skipped', 'cancelled', undefined]) {
      assert.deepEqual(checkQualityResults({ ...successful, [name]: { result } }), [name]);
    }
    const missing = { ...successful };
    delete missing[name];
    assert.deepEqual(checkQualityResults(missing), [name]);
  }
  assert.equal(checkQualityResults(null).length, 2);
});

test('quality aggregate CLI fails closed on absent and malformed input', () => {
  for (const input of ['', '{', 'null', '{}', JSON.stringify(successful)]) {
    const result = spawnSync(process.execPath, ['scripts/check-quality-results.mjs'], {
      cwd: new URL('../', import.meta.url),
      env: { ...process.env, QUALITY_RESULTS: input },
      encoding: 'utf8',
    });
    assert.equal(result.status, input === JSON.stringify(successful) ? 0 : 1, result.stderr);
  }
});

function jobs(source) {
  const blocks = new Map();
  for (const match of source.matchAll(/^  ([\w-]+):\n([\s\S]*?)(?=^  [\w-]+:\n|$(?![\s\S]))/gm)) {
    blocks.set(match[1], match[2].split('\n').filter((line) => !line.trimStart().startsWith('#')).join('\n'));
  }
  return blocks;
}

test('CI and release keep complete parallel quality gates and trusted cache namespaces', () => {
  const workflows = ['desktop-ci.yml', 'desktop-release.yml'].map((name) => {
    const source = readFileSync(new URL(`../.github/workflows/${name}`, import.meta.url), 'utf8');
    assert.doesNotMatch(source, /^\s+paths(?:-ignore)?:/m, 'Do not silently skip required checks');
    return jobs(source);
  });
  for (const workflow of workflows) {
    const aggregate = workflow.get('quality');
    assert.match(aggregate, /name: Quality gates\n/);
    assert.match(aggregate, /if: \$\{\{ always\(\) \}\}/);
    assert.match(aggregate, /needs: \[quality-js, quality-rust\]/);
    assert.match(aggregate, /QUALITY_RESULTS: \$\{\{ toJSON\(needs\) \}\}/);
    assert.match(aggregate, /run: node scripts\/check-quality-results\.mjs/);

    const js = workflow.get('quality-js');
    const rust = workflow.get('quality-rust');
    assert.match(js, /run: pnpm run check:js\n/);
    assert.doesNotMatch(js, /needs:.*quality-rust/);
    assert.doesNotMatch(rust, /needs:.*quality-js/);
    assert.match(rust, /run: npm run check:rust:fmt/);
    assert.match(rust, /run: npm run check:rust:tests/);
    assert.ok(rust.indexOf('run: npm run check:rust:fmt') < rust.indexOf('uses: Swatinem/rust-cache@'));
    assert.match(rust, /shared-key: ci-\$\{\{ runner.os \}\}-\$\{\{ runner.arch \}\}/);
    for (const block of [js, rust]) {
      assert.match(block, /runs-on: macos-15-intel/);
      assert.match(block, /save-if: \$\{\{ github.ref == 'refs\/heads\/main' \}\}/);
    }
    assert.match(js, /shared-key: protocol-fixture-/);
  }
  const windows = workflows[0].get('windows-compile');
  assert.match(windows, /name: Windows compile \+ tests/);
  assert.match(windows, /run: cargo test --manifest-path src-tauri\/Cargo.toml --locked/);
  const releaseBuild = workflows[1].get('build');
  assert.match(releaseBuild, /shared-key: release-\$\{\{ runner.os \}\}-\$\{\{ runner.arch \}\}/);
  assert.match(workflows[1].get('publish'), /needs:.*quality/);
});
