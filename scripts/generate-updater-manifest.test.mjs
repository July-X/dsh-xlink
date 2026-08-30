import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import test from 'node:test';
import { join } from 'node:path';

import {
  buildReleaseMetadata,
  collectReleaseAssets,
} from './generate-updater-manifest.mjs';

function withArtifacts(callback) {
  const root = mkdtempSync(join(process.cwd(), '.release-manifest-test-'));
  try {
    const mac = join(root, 'macos');
    const windows = join(root, 'windows');
    mkdirSync(mac);
    mkdirSync(windows);
    writeFileSync(join(mac, 'dsh-xlink_0.1.2-rc.5_x64.dmg'), 'dmg');
    writeFileSync(join(mac, 'dsh-xlink_0.1.2-rc.5_x64.app.tar.gz'), 'tar');
    writeFileSync(join(mac, 'dsh-xlink_0.1.2-rc.5_x64.app.tar.gz.sig'), 'mac-signature\n');
    writeFileSync(join(windows, 'dsh-xlink_0.1.2-rc.5_x64-setup.exe'), 'exe');
    writeFileSync(join(windows, 'dsh-xlink_0.1.2-rc.5_x64-setup.exe.sig'), 'win-signature\n');
    return callback(root);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

test('collects exactly the updater assets and emits the expected platform keys', () => {
  withArtifacts((root) => {
    const assets = collectReleaseAssets(root, '0.1.2-rc.5');
    const metadata = buildReleaseMetadata({
      version: '0.1.2-rc.5',
      tag: 'desktop-v0.1.2-rc.5',
      repository: 'July-X/dsh-xlink',
      assets,
      pubDate: '2026-08-30T00:00:00.000Z',
    });

    assert.deepEqual(Object.keys(metadata.platforms), [
      'darwin-x86_64',
      'darwin-x86_64-app',
      'windows-x86_64',
      'windows-x86_64-nsis',
    ]);
    assert.equal(metadata.platforms['darwin-x86_64'].signature, 'mac-signature');
    assert.equal(
      metadata.platforms['darwin-x86_64-app'].url,
      metadata.platforms['darwin-x86_64'].url,
    );
    assert.equal(metadata.platforms['windows-x86_64'].signature, 'win-signature');
    assert.equal(
      metadata.platforms['windows-x86_64-nsis'].url,
      metadata.platforms['windows-x86_64'].url,
    );
    assert.match(
      metadata.platforms['darwin-x86_64'].url,
      /releases\/download\/desktop-v0\.1\.2-rc\.5\/dsh-xlink_0\.1\.2-rc\.5_x64\.app\.tar\.gz$/,
    );
  });
});

test('rejects duplicate release assets instead of publishing an ambiguous manifest', () => {
  withArtifacts((root) => {
    writeFileSync(join(root, 'macos', 'second-0.1.2-rc.5.dmg'), 'dmg');
    assert.throws(
      () => collectReleaseAssets(root, '0.1.2-rc.5'),
      /macOS DMG must contain exactly one file, found 2/,
    );
  });
});
