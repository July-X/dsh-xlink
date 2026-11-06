import test from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { createHash } from 'node:crypto';
import { gzipSync } from 'node:zlib';
import { spawn } from 'node:child_process';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, relative } from 'node:path';
import { createRequire } from 'node:module';

// Explicitly select the already-installed CLI. Corepack must not fetch a tool during this fixture.
const pnpm = process.env.DSH_TEST_PNPM_JS;
const pnpmExe = process.env.DSH_TEST_PNPM_EXE;
function archive(files) {
  const blocks = [];
  for (const [name, value] of Object.entries(files)) {
    const body = Buffer.from(value); const header = Buffer.alloc(512);
    header.write(`package/${name}`); header.write('0000644\0', 100);
    header.write('0000000\0', 108); header.write('0000000\0', 116);
    header.write(`${body.length.toString(8).padStart(11, '0')}\0`, 124);
    header.write('00000000000\0', 136); header.fill(32, 148, 156); header[156] = 48;
    header.write('ustar\0', 257); header.write('00', 263);
    const checksum = header.reduce((sum, byte) => sum + byte, 0);
    header.write(`${checksum.toString(8).padStart(6, '0')}\0 `, 148);
    blocks.push(header, body, Buffer.alloc((512 - body.length % 512) % 512));
  }
  return gzipSync(Buffer.concat([...blocks, Buffer.alloc(1024)]));
}

test('precheck profile install builds approved dependencies in its local node_modules', { skip: !pnpm && !pnpmExe, timeout: 90_000 }, async () => {
  const root = mkdtempSync(join(tmpdir(), 'xlink-prepared-cache-'));
  let requests = 0;
  const manifest = { name: 'fixture-dependency', version: '1.0.0', main: 'generated.cjs', scripts: { postinstall: 'node build.cjs' } };
  const tarball = archive({
    'package.json': JSON.stringify(manifest),
    'build.cjs': 'require("node:fs").writeFileSync("generated.cjs", "module.exports=42")',
  });
  const server = createServer((req, res) => {
    requests += 1;
    const body = req.url.endsWith('.tgz') ? tarball : Buffer.from(JSON.stringify({
      name: 'fixture-dependency', 'dist-tags': { latest: '1.0.0' }, versions: { '1.0.0': {
        ...manifest, dist: {
          tarball: `http://127.0.0.1:${server.address().port}/dependency.tgz`,
          integrity: `sha512-${createHash('sha512').update(tarball).digest('base64')}`,
        },
      } },
    }));
    res.setHeader('content-type', req.url.endsWith('.tgz') ? 'application/octet-stream' : 'application/json');
    res.end(body);
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  const registry = `http://127.0.0.1:${server.address().port}`;
  const inherited = Object.fromEntries(Object.entries(process.env).filter(([key]) => !/^npm_config_/i.test(key) && !['NPM_TOKEN', 'NODE_AUTH_TOKEN'].includes(key)));
  const env = { ...inherited, HOME: join(root, 'home'), USERPROFILE: join(root, 'home'),
    XDG_CACHE_HOME: join(root, 'global-cache'), XDG_DATA_HOME: join(root, 'global-data'), XDG_CONFIG_HOME: join(root, 'global-config'), APPDATA: join(root, 'appdata'), LOCALAPPDATA: join(root, 'local-appdata'), PNPM_HOME: join(root, 'pnpm-home'),
    npm_config_userconfig: join(root, 'empty.npmrc'), npm_config_globalconfig: join(root, 'empty-global.npmrc'),
    COREPACK_ENABLE_NETWORK: '0', CI: 'true',
  };
  for (const key of ['NODE_OPTIONS', 'HTTP_PROXY', 'HTTPS_PROXY', 'ALL_PROXY', 'http_proxy', 'https_proxy', 'all_proxy']) delete env[key];
  mkdirSync(env.HOME); writeFileSync(env.npm_config_userconfig, ''); writeFileSync(env.npm_config_globalconfig, '');
  async function run(cwd, args) {
    let output = '';
    const child = spawn(pnpmExe || process.execPath, pnpmExe ? args : [pnpm, ...args], {
      cwd, env, timeout: 30_000, stdio: ['ignore', 'pipe', 'pipe'],
    });
    child.stdout.on('data', (chunk) => { output += chunk; }); child.stderr.on('data', (chunk) => { output += chunk; });
    const code = await new Promise((resolve, reject) => { child.on('error', reject); child.on('close', resolve); });
    assert.equal(code, 0, output);
    return output.trim();
  }
  try {
    const profile = join(root, 'sandbox', 'profile'); const extension = join(root, 'sandbox', 'extensions', 'plugin');
    mkdirSync(profile, { recursive: true }); mkdirSync(extension, { recursive: true });
    writeFileSync(join(extension, 'package.json'), JSON.stringify({ name: 'fixture-plugin', version: '1.0.0', main: 'index.js', dependencies: { 'fixture-dependency': '1.0.0' } }));
    writeFileSync(join(extension, 'index.js'), 'module.exports=require("fixture-dependency")');
    writeFileSync(join(profile, 'package.json'), JSON.stringify({ private: true, dependencies: { 'fixture-plugin': `file:${relative(profile, extension).replaceAll('\\', '/')}` } }));
    const major = Number((await run(profile, ['--version'])).split('.')[0]);
    writeFileSync(join(profile, 'pnpm-workspace.yaml'),
      (major >= 11 ? 'allowBuilds:\n  fixture-dependency: true\n' : 'onlyBuiltDependencies:\n  - fixture-dependency\n')
      + 'minimumReleaseAge: 0\n');
    await run(profile, ['install', '--reporter=append-only', '--config.strict-dep-builds=false',
      '--config.enable-global-virtual-store=false', `--registry=${registry}`]);
    assert.equal(createRequire(join(profile, 'package.json'))('fixture-plugin'), 42);
    assert.ok(requests > 0, 'precheck must install from the fixture registry');
  } finally {
    if (server.listening) await new Promise((resolve) => server.close(resolve));
    rmSync(root, { recursive: true, force: true });
  }
});
