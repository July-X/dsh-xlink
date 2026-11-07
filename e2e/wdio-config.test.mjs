import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';

test('MiniMax 长任务使用足够的等待时间，且不会由 WebDriver 自动重复提交', () => {
  const root = mkdtempSync(join(tmpdir(), 'mcode-wdio-options-'));
  try {
    const binary = join(root, 'app.exe');
    const home = join(root, 'home');
    const appData = join(root, 'appdata');
    mkdirSync(home);
    mkdirSync(appData);
    writeFileSync(binary, '');
    const env = {
      ...process.env,
      DSH_E2E_APP_BINARY: binary,
      DSH_E2E_RUN_ROOT: root,
      DSH_E2E_EXPECTED_HOME: home,
      DSH_E2E_EXPECTED_APP_DATA: appData,
      DSH_XLINK_HOME: home,
      DSH_XLINK_CHANNEL_SUFFIX: 'e2e-options',
      DSH_E2E_MCODE_PACKAGE: join(root, 'candidate'),
    };
    const source = "import { config } from './e2e/wdio.conf.mjs'; process.stdout.write(JSON.stringify([config.connectionRetryTimeout, config.connectionRetryCount]));";
    const options = JSON.parse(execFileSync(process.execPath, ['--input-type=module', '-e', source], { env, encoding: 'utf8' }));
    assert.ok(options[0] > 900_000);
    assert.equal(options[1], 0);
    delete env.DSH_E2E_MCODE_PACKAGE;
    const defaults = JSON.parse(execFileSync(process.execPath, ['--input-type=module', '-e', source], { env, encoding: 'utf8' }));
    assert.deepEqual(defaults, [null, null]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
