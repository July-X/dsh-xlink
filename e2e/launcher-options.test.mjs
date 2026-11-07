import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { buildRuntimeEnv, isMcodeInstanceId, parseRunOptions } from './launcher.mjs';

test('MiniMax 新旧实例标识均可验证且拒绝路径穿越', () => {
  assert.equal(isMcodeInstanceId('mcode-0123456789abcdef0123456789abcdef'), true);
  assert.equal(isMcodeInstanceId('mcode-AbCdEfGhIjKlMnOpQrSt-_'), true);
  for (const value of ['mcode-../test', 'mcode-ABCDEFGHIJKLMNOPQRSTUV/', 'mcode-短路径', 'mcode-', null]) {
    assert.equal(isMcodeInstanceId(value), false);
  }
});

test('MiniMax 入口只接受显式的完整参数和受控 spec', () => {
  const root = mkdtempSync(join(tmpdir(), 'mcode-launcher-options-'));
  try {
    const spec = join(root, 'e2e', 'smoke', 'candidate.spec.mjs');
    const outside = join(root, 'outside.spec.mjs');
    const pkg = join(root, 'candidate');
    const node = join(root, 'node.exe');
    mkdirSync(join(root, 'e2e', 'smoke'), { recursive: true });
    mkdirSync(pkg);
    writeFileSync(spec, '');
    writeFileSync(outside, '');
    writeFileSync(node, '');
    writeFileSync(join(pkg, 'package.json'), JSON.stringify({ name: '@minimax-webui/minimax-code', version: '0.1.7' }));
    const args = ['--spec', 'e2e/smoke/candidate.spec.mjs', '--mcode-package', pkg, '--mcode-node', node];
    const options = parseRunOptions(args, root);
    assert.equal(options.spec, realpathSync(spec));
    assert.equal(options.mcodePackage, realpathSync(pkg));
    assert.equal(options.mcodeNode, realpathSync(node));
    assert.throws(() => parseRunOptions(['--mcode-package', pkg], root));
    assert.throws(() => parseRunOptions(['--spec', outside], root));
    assert.throws(() => parseRunOptions(['--spec', 'e2e/smoke/candidate.spec.mjs', '--spec', 'x'], root));
    assert.throws(() => parseRunOptions(['--mcode-package', 'relative', '--mcode-node', node, '--spec', spec], root));
    writeFileSync(join(pkg, 'package.json'), JSON.stringify({ name: '@minimax-webui/minimax-code', version: '0.1.8' }));
    assert.throws(() => parseRunOptions(args, root));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('运行环境仅传递显式候选路径，丢弃外部注入', () => {
  const paths = { homeDir: '/run/home', dshHome: '/run/decoy/.dsh', runId: 'e2e-test', runRoot: '/run', binary: '/run/app', appDataDir: '/run/appdata', port: 35000, evidenceDir: '/run/evidence', decoyHome: '/run/decoy', tempDir: '/run/temp', roamingDir: '/run/roaming', localDir: '/run/local' };
  const injected = { DSH_E2E_MCODE_PACKAGE: '/unsafe/package', DSH_E2E_MCODE_NODE: '/unsafe/node', NODE_OPTIONS: '--unsafe', MAVIS_BUILD_ENV: 'prod', PATH: 'path' };
  const base = buildRuntimeEnv(injected, paths, 'hash');
  assert.equal(base.DSH_E2E_MCODE_PACKAGE, undefined);
  assert.equal(base.DSH_E2E_MCODE_NODE, undefined);
  assert.equal(base.NODE_OPTIONS, undefined);
  assert.equal(base.MAVIS_BUILD_ENV, undefined);
  const explicit = buildRuntimeEnv(injected, { ...paths, mcodePackage: '/candidate', mcodeNode: '/node' }, 'hash');
  assert.equal(explicit.DSH_E2E_MCODE_PACKAGE, '/candidate');
  assert.equal(explicit.DSH_E2E_MCODE_NODE, '/node');
  assert.equal(explicit.MAVIS_BUILD_ENV, undefined);
  const memorySpec = realpathSync(fileURLToPath(new URL('./smoke/minimax-memory.spec.mjs', import.meta.url)));
  const memory = buildRuntimeEnv(injected, { ...paths, mcodePackage: '/candidate', mcodeNode: '/node', spec: memorySpec }, 'hash');
  assert.equal(memory.MAVIS_REGION, 'cn');
  assert.equal(memory.MAVIS_BUILD_ENV, 'test');
  assert.equal(memory.MAVIS_LOCAL_RUNTIME_DISABLE_TOOLS, '1');
  assert.equal(buildRuntimeEnv(injected, { ...paths, spec: memorySpec }, 'hash').MAVIS_BUILD_ENV, undefined);
});
