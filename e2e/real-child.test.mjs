import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, rmSync, symlinkSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, toNamespacedPath } from 'node:path';
import { test } from 'node:test';
import { isRealChild } from './real-child.mjs';

test('真实子路径兼容 Windows 长路径，同时拒绝目录本身、同级与越界链接', () => {
  const root = mkdtempSync(join(tmpdir(), 'mcode-real-child-'));
  try {
    const home = join(root, 'home');
    const child = join(home, 'installed');
    const outside = join(root, 'outside');
    mkdirSync(child, { recursive: true });
    mkdirSync(outside);
    symlinkSync(outside, join(home, 'link'), 'junction');
    assert.equal(isRealChild(home, child), true);
    assert.equal(isRealChild(home, home), false);
    assert.equal(isRealChild(home, outside), false);
    assert.equal(isRealChild(home, join(home, 'link')), false);
    if (process.platform === 'win32') {
      assert.equal(isRealChild(home, toNamespacedPath(child)), true);
      assert.equal(isRealChild(toNamespacedPath(home), child), true);
      assert.equal(isRealChild(home, toNamespacedPath(outside)), false);
      assert.equal(isRealChild(home, toNamespacedPath(join(home, 'link'))), false);
    }
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
