import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import { assertReleaseVersionAdvances } from './check-release-version.mjs';

const script = fileURLToPath(new URL('./check-release-version.mjs', import.meta.url));
const run = (...args) => spawnSync(process.execPath, [script, ...args], { encoding: 'utf8' });

test('双方版本都必须是完整、类型正确的 SemVer', () => {
  for (const bad of [undefined, null, {}, ['0.7.7'], 7, false, '', 'not-a-version', '1.2', 'v1.2.3', '01.2.3', '1.02.3', '1.2.03', '1.2.3-', '1.2.3+', '1.2.3-01', '1.2.3-rc.01', '1.2.3-rc..1', '1.2.3+build..1', ' 1.2.3', '1.2.3\n']) {
    assert.throws(() => assertReleaseVersionAdvances('1.2.3', bad), /线上版本不是合法的 SemVer/);
    assert.throws(() => assertReleaseVersionAdvances(bad, '0.0.0'), /待发布版本不是合法的 SemVer/);
  }
});

test('正式版、RC、数字与文本标识均按 SemVer 排序，保留整数精度', () => {
  const ordered = ['1.0.0-alpha', '1.0.0-alpha.1', '1.0.0-alpha.beta', '1.0.0-beta', '1.0.0-beta.2', '1.0.0-beta.11', '1.0.0-rc.1', '1.0.0'];
  const pairs = ordered.slice(1).map((next, i) => [next, ordered[i]]);
  pairs.push(['0.7.8-rc.1', '0.7.7'], ['1.2.3-alpha-beta.2', '1.2.3-alpha-beta.1'], ['1.2.3-01a', '1.2.3-0'], ['1.2.3-rc.2+build.001', '1.2.3-rc.1+build.999'], ['9007199254740993.0.0', '9007199254740992.0.0'], ['1.2.3-9007199254740993', '1.2.3-9007199254740992']);
  for (const [next, current] of pairs) {
    assert.doesNotThrow(() => assertReleaseVersionAdvances(next, current));
    assert.throws(() => assertReleaseVersionAdvances(current, next), /不大于/);
  }
});

test('相等或只变化 build 元数据都不算升级', () => {
  for (const [a, b] of [['1.2.3', '1.2.3'], ['1.2.3+one', '1.2.3+two'], ['1.2.3-rc.1+x', '1.2.3-rc.1+y']]) {
    assert.throws(() => assertReleaseVersionAdvances(a, b), /不大于/);
  }
});

test('CLI 读取原始清单，不把数组、对象、缺字段或坏 JSON 转成可比较版本', async (t) => {
  const dir = await mkdtemp(join(tmpdir(), 'xlink-release-version-'));
  t.after(() => rm(dir, { recursive: true, force: true }));
  const manifest = join(dir, 'latest.json');
  for (const data of [{ version: 'not-a-version' }, { version: ['0.7.7'] }, { version: {} }, { version: 7 }, {}, null]) {
    await writeFile(manifest, JSON.stringify(data));
    const result = run('0.7.8-rc.1', manifest);
    assert.equal(result.status, 1, result.stderr);
    assert.match(result.stderr, /线上版本不是合法的 SemVer/);
  }
  await writeFile(manifest, '{broken');
  assert.equal(run('0.7.8-rc.1', manifest).status, 1);
  assert.equal(run('0.7.8-rc.1', join(dir, 'missing.json')).status, 1);
  await writeFile(manifest, JSON.stringify({ version: '0.7.8-rc.1' }));
  assert.equal(run('0.7.8-rc.1', manifest).status, 1);
  await writeFile(manifest, JSON.stringify({ version: '0.7.7' }));
  const accepted = run('0.7.8-rc.1', manifest);
  assert.equal(accepted.status, 0, accepted.stderr);
  assert.match(accepted.stdout, /0\.7\.7 → 0\.7\.8-rc\.1/);
});

test('首次发布不比较线上版本，但仍须验证待发版本格式', () => {
  assert.equal(run('--format-only', '0.7.8-rc.1').status, 0);
  assert.equal(run('--format-only', 'not-a-version').status, 1);
  assert.equal(run('0.7.8-rc.1').status, 1);
  assert.equal(run('0.7.8-rc.1', '').status, 1);
});

test('发布 workflow 接入共享判据；删除真实调用会被此测试挡住', async () => {
  const workflow = await readFile(new URL('../.github/workflows/desktop-release.yml', import.meta.url), 'utf8');
  const compareCall = 'node scripts/check-release-version.mjs "$version" "$body_file"';
  const validateCall = 'node scripts/check-release-version.mjs --format-only "$version"\n';
  const assertWiring = (source) => {
    assert.ok(source.includes(compareCall), 'workflow 必须使用共享版本检查读取原始清单');
    assert.ok(source.indexOf(validateCall) >= 0 && source.indexOf(validateCall) < source.indexOf('latest_url='), '首次发布也要先检查待发版本');
    assert.ok(!source.includes('CANDIDATE="$version" PUBLISHED='), '不得保留另一份内联版本比较');
    assert.match(source, /node scripts\/confirm-first-release\.mjs/);
  };
  assertWiring(workflow);
  assert.throws(() => assertWiring(workflow.replace(compareCall, 'echo skipped')), /共享版本检查/);
});
