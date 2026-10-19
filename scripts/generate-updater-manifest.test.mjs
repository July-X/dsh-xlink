import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import test from 'node:test';
import { basename, join } from 'node:path';

import {
  buildReleaseMetadata,
  collectReleaseAssets,
  selectReleaseAssets,
  walkFiles,
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
    writeFileSync(join(windows, 'dsh-xlink_0.1.2-rc.5_x64-setup.exe.sig'), 'win-signature\n');
    // 清单只需要安装包文件名；测试无需落盘一个内容无效的可执行文件。
    const installer = join(windows, 'dsh-xlink_0.1.2-rc.5_x64-setup.exe');
    return callback(root, [...walkFiles(root), installer]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

test('selects updater assets by name and emits the expected platform keys', () => {
  withArtifacts((_root, files) => {
    const assets = selectReleaseAssets(files, '0.1.2-rc.5');
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
    assert.match(
      metadata.platforms['windows-x86_64'].url,
      /releases\/download\/desktop-v0\.1\.2-rc\.5\/dsh-xlink_0\.1\.2-rc\.5_x64-setup\.exe$/,
    );
  });
});

test('walks nested files without treating directories as assets or creating executable fixtures', () => {
  withArtifacts((root) => {
    mkdirSync(join(root, 'ignored.dmg'));
    assert.deepEqual(walkFiles(root).map((path) => basename(path)).sort(), [
      'dsh-xlink_0.1.2-rc.5_x64-setup.exe.sig',
      'dsh-xlink_0.1.2-rc.5_x64.app.tar.gz',
      'dsh-xlink_0.1.2-rc.5_x64.app.tar.gz.sig',
      'dsh-xlink_0.1.2-rc.5_x64.dmg',
    ]);
    assert.throws(
      () => collectReleaseAssets(root, '0.1.2-rc.5'),
      /Windows installer must contain exactly one file, found 0/,
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

test('rejects duplicate installers and missing signatures using in-memory paths', () => {
  withArtifacts((root, files) => {
    assert.throws(
      () => selectReleaseAssets([...files, join(root, 'second-0.1.2-rc.5-setup.exe')], '0.1.2-rc.5'),
      /Windows installer must contain exactly one file, found 2/,
    );
    assert.throws(
      () => selectReleaseAssets(files.filter((path) => !path.endsWith('-setup.exe.sig')), '0.1.2-rc.5'),
      /Windows installer signature must contain exactly one file, found 0/,
    );
    assert.throws(
      () => selectReleaseAssets(files, '0.1.2-rc.6'),
      /release asset does not contain version 0\.1\.2-rc\.6/,
    );
  });
});
