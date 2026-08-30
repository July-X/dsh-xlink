import assert from 'node:assert/strict';
import {
  mkdtempSync,
  mkdirSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import test from 'node:test';
import { join } from 'node:path';

import { normalizeMacOSUpdaterAssets } from './normalize-release-assets.mjs';

function withBundle(callback) {
  const root = mkdtempSync(join(process.cwd(), '.normalize-release-assets-test-'));
  try {
    const dmg = join(root, 'dmg');
    const macos = join(root, 'macos');
    mkdirSync(dmg);
    mkdirSync(macos);
    writeFileSync(join(dmg, 'dsh-xlink_0.1.2-rc.6_x64.dmg'), 'dmg');
    writeFileSync(join(macos, 'dsh-xlink.app.tar.gz'), 'tar');
    writeFileSync(join(macos, 'dsh-xlink.app.tar.gz.sig'), 'signature');
    return callback(root, { dmg, macos });
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

test('renames generic macOS updater assets from the DMG version', () => {
  withBundle((root, directories) => {
    const result = normalizeMacOSUpdaterAssets(root, '0.1.2-rc.6');

    assert.deepEqual(readdirSync(directories.macos).sort(), [
      'dsh-xlink_0.1.2-rc.6_x64.app.tar.gz',
      'dsh-xlink_0.1.2-rc.6_x64.app.tar.gz.sig',
    ]);
    assert.equal(
      result.archive,
      join(directories.macos, 'dsh-xlink_0.1.2-rc.6_x64.app.tar.gz'),
    );
    assert.equal(
      result.signature,
      join(directories.macos, 'dsh-xlink_0.1.2-rc.6_x64.app.tar.gz.sig'),
    );
  });
});

test('leaves already versioned macOS updater assets unchanged', () => {
  withBundle((root, directories) => {
    writeFileSync(
      join(directories.macos, 'dsh-xlink_0.1.2-rc.6_x64.app.tar.gz'),
      'tar',
    );
    writeFileSync(
      join(directories.macos, 'dsh-xlink_0.1.2-rc.6_x64.app.tar.gz.sig'),
      'signature',
    );
    rmSync(join(directories.macos, 'dsh-xlink.app.tar.gz'));
    rmSync(join(directories.macos, 'dsh-xlink.app.tar.gz.sig'));

    const result = normalizeMacOSUpdaterAssets(root, '0.1.2-rc.6');

    assert.match(result.archive, /dsh-xlink_0\.1\.2-rc\.6_x64\.app\.tar\.gz$/);
    assert.match(result.signature, /dsh-xlink_0\.1\.2-rc\.6_x64\.app\.tar\.gz\.sig$/);
  });
});

test('rejects a macOS DMG without the requested version', () => {
  withBundle((root) => {
    assert.throws(
      () => normalizeMacOSUpdaterAssets(root, '0.1.2-rc.7'),
      /macOS DMG does not contain release version 0\.1\.2-rc\.7/,
    );
  });
});
