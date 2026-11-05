import test from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { createHash } from 'node:crypto';
import { gzipSync } from 'node:zlib';
import { spawn } from 'node:child_process';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, renameSync, rmSync, cpSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, relative } from 'node:path';
import { createRequire } from 'node:module';

// Explicitly select the already-installed CLI. Corepack must not fetch a tool during this fixture.
const pnpm = process.env.DSH_TEST_PNPM_JS;
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

test('private pnpm material survives relocation and an offline copy-mode application', { skip: !pnpm, timeout: 90_000 }, async () => {
  const root = mkdtempSync(join(tmpdir(), 'xlink-prepared-cache-'));
  let requests = 0;
  const tarball = archive({ 'package.json': JSON.stringify({ name: 'fixture-dependency', version: '1.0.0', main: 'index.js' }), 'index.js': 'module.exports=42' });
  const server = createServer((req, res) => {
    requests += 1;
    const body = req.url.endsWith('.tgz') ? tarball : Buffer.from(JSON.stringify({
      name: 'fixture-dependency', 'dist-tags': { latest: '1.0.0' }, versions: { '1.0.0': {
        name: 'fixture-dependency', version: '1.0.0', dist: {
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
    COREPACK_ENABLE_NETWORK: '0', CI: 'true', XLINK_FIXTURE_BUILDS: join(root, 'builds'),
  };
  for (const key of ['NODE_OPTIONS', 'HTTP_PROXY', 'HTTPS_PROXY', 'ALL_PROXY', 'http_proxy', 'https_proxy', 'all_proxy']) delete env[key];
  mkdirSync(env.HOME); writeFileSync(env.npm_config_userconfig, ''); writeFileSync(env.npm_config_globalconfig, '');
  async function run(cwd, args) {
    let output = '';
    const child = spawn(process.execPath, [pnpm, ...args], { cwd, env, timeout: 30_000, stdio: ['ignore', 'pipe', 'pipe'] });
    child.stdout.on('data', (chunk) => { output += chunk; }); child.stderr.on('data', (chunk) => { output += chunk; });
    const code = await new Promise((resolve, reject) => { child.on('error', reject); child.on('close', resolve); });
    assert.equal(code, 0, output);
  }
  const source = join(root, 'candidate'); mkdirSync(source);
  writeFileSync(join(source, 'package.json'), JSON.stringify({ name: 'fixture-plugin', version: '1.0.0', main: 'index.js', dependencies: { 'fixture-dependency': '1.0.0' }, scripts: { prepare: 'node build.cjs' } }));
  writeFileSync(join(source, 'index.js'), 'module.exports=require("fixture-dependency")');
  writeFileSync(join(source, 'build.cjs'), 'require("node:fs").appendFileSync(process.env.XLINK_FIXTURE_BUILDS,"build\\n")');
  const options = ['install', '--reporter=append-only', '--ignore-workspace', '--config.node-linker=hoisted', '--config.enable-pre-post-scripts=true', `--registry=${registry}`];
  const draft = join(root, '.pending-plan', 'pnpm'); mkdirSync(draft, { recursive: true });
  const cacheArgs = (cache) => [`--store-dir=${join(cache, 'store')}`, `--cache-dir=${join(cache, 'cache')}`, '--force'];
  try {
    await run(source, [...options, `--store-dir=${join(root, 'global-store')}`]);
    const profile = join(root, 'sandbox', 'profile'); const extension = join(root, 'sandbox', 'extensions', 'plugin');
    mkdirSync(profile, { recursive: true }); cpSync(source, extension, { recursive: true });
    writeFileSync(join(profile, 'package.json'), JSON.stringify({ private: true, dependencies: { 'fixture-plugin': `file:${relative(profile, extension).replaceAll('\\', '/')}` } }));
    await run(profile, [...options, ...cacheArgs(draft)]);
    writeFileSync(join(draft, 'registry'), registry);
    const beforeBuilds = readFileSync(env.XLINK_FIXTURE_BUILDS, 'utf8'); const beforeRequests = requests;
    const published = join(root, 'published-plan'); renameSync(join(root, '.pending-plan'), published);
    await new Promise((resolve) => server.close(resolve));
    rmSync(join(root, 'global-cache'), { recursive: true, force: true }); rmSync(join(root, 'global-store'), { recursive: true, force: true });
    const applied = join(root, 'applied'); const appliedProfile = join(applied, 'profile'); const appliedExtension = join(applied, 'extensions', 'plugin');
    mkdirSync(appliedProfile, { recursive: true }); cpSync(source, appliedExtension, { recursive: true });
    writeFileSync(join(appliedProfile, 'package.json'), JSON.stringify({ private: true, dependencies: { 'fixture-plugin': `file:${relative(appliedProfile, appliedExtension).replaceAll('\\', '/')}` } }));
    const cache = join(published, 'pnpm');
    await run(appliedProfile, ['install', '--reporter=append-only', '--ignore-workspace', '--config.node-linker=hoisted', '--offline', '--ignore-scripts', `--registry=${readFileSync(join(cache, 'registry'), 'utf8')}`, ...cacheArgs(cache)]);
    assert.equal(createRequire(join(appliedProfile, 'package.json'))('fixture-plugin'), 42);
    assert.equal(readFileSync(env.XLINK_FIXTURE_BUILDS, 'utf8'), beforeBuilds, 'application must not rebuild');
    assert.equal(requests, beforeRequests, 'application must not make another registry request');
  } finally {
    if (server.listening) await new Promise((resolve) => server.close(resolve));
    rmSync(root, { recursive: true, force: true });
  }
});
