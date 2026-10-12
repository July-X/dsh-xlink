import assert from 'node:assert/strict';
import { chmodSync, mkdtempSync, mkdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';

import {
  credentialPath,
  removeBridgeCredential,
  TOKEN_ENV,
  URL_ENV,
  writeBridgeCredential,
} from '../host/bridge-credential.js';

/**
 * 造一棵「物化树」：`<tmp>/<fingerprint>/compat-a`。
 *
 * 凭据要落在 `<fingerprint>/` 那一级，所以夹具必须**真的分两级** ——
 * 造一层的话「写在上一级」与「写在自己身上」在测试里长得一模一样。
 */
function fixture() {
  const fingerprint = mkdtempSync(join(tmpdir(), 'ponytail-cred-'));
  const root = join(fingerprint, 'compat-a');
  mkdirSync(root, { recursive: true });
  return {
    fingerprint,
    root,
    path: join(fingerprint, '.xlink-bridge.json'),
    env: { [URL_ENV]: 'http://127.0.0.1:9999', [TOKEN_ENV]: 'token-1' },
    cleanup: () => rmSync(fingerprint, { recursive: true, force: true }),
  };
}

test('内核环境里没有桥凭据：不写文件、不报错', () => {
  const f = fixture();
  const result = writeBridgeCredential(f.root, {});
  assert.equal(result.written, false);
  assert.equal(result.failed, undefined, '没凭据不是失败：不是壳启动的内核就没有它');
  assert.equal(result.path, null);
  assert.throws(() => statSync(f.path), '不该凭空造一份凭据出来');
  f.cleanup();
});

test('有凭据：落在物化根的上一级，0600，内容就是那两个值', () => {
  const f = fixture();
  const result = writeBridgeCredential(f.root, f.env);
  assert.equal(result.written, true);
  assert.equal(result.path, f.path);
  // 落在上一级：物化根被壳 remove_dir_all 重建时，这一份不会被带走。
  assert.equal(credentialPath(f.root), f.path);
  assert.equal(statSync(f.path).mode & 0o777, 0o600, '凭据必须只有本人可读');
  const raw = JSON.parse(readFileSync(f.path, 'utf8'));
  assert.equal(raw.url, f.env[URL_ENV]);
  assert.equal(raw.token, f.env[TOKEN_ENV]);
  assert.equal(typeof raw.pid, 'number');
  f.cleanup();
});

test('盘上已经是同一代凭据：不重写（连时间戳都不动）', () => {
  const f = fixture();
  writeBridgeCredential(f.root, f.env);
  // 手写一份同令牌、不同排版的：重写过的话内容会变回规范形状。
  const handWritten = `{"url":"${f.env[URL_ENV]}","token":"${f.env[TOKEN_ENV]}"}\n`;
  writeFileSync(f.path, handWritten, 'utf8');
  const result = writeBridgeCredential(f.root, f.env);
  assert.equal(result.written, false);
  assert.equal(readFileSync(f.path, 'utf8'), handWritten, '同一代凭据不该被重写');
  f.cleanup();
});

test('盘上是上一代的令牌（壳重启过、桥换了一代）：覆盖', () => {
  const f = fixture();
  writeBridgeCredential(f.root, { [URL_ENV]: 'http://127.0.0.1:1', [TOKEN_ENV]: 'old' });
  const result = writeBridgeCredential(f.root, f.env);
  assert.equal(result.written, true);
  assert.equal(JSON.parse(readFileSync(f.path, 'utf8')).token, f.env[TOKEN_ENV]);
  f.cleanup();
});

test('写不下去（上一级目录只读）：返回 failed 而不是抛', () => {
  const f = fixture();
  chmodSync(f.fingerprint, 0o500);
  try {
    const result = writeBridgeCredential(f.root, f.env);
    assert.equal(result.written, false);
    assert.equal(result.failed, true, '失败要被标出来，调用方才好记一行 warn');
  } finally {
    chmodSync(f.fingerprint, 0o700);
    f.cleanup();
  }
});

test('收走凭据只删自己这一代', () => {
  const f = fixture();
  writeBridgeCredential(f.root, f.env);
  // 换了壳：盘上这枚不是本次环境里的那一枚，不许删。
  assert.equal(removeBridgeCredential(f.root, { [TOKEN_ENV]: 'another' }), false);
  assert.equal(JSON.parse(readFileSync(f.path, 'utf8')).token, f.env[TOKEN_ENV]);
  // 同一代：正常停止时收走。
  assert.equal(removeBridgeCredential(f.root, f.env), true);
  assert.throws(() => statSync(f.path));
  // 已经没有文件时再收一次：返回 false，不抛。
  assert.equal(removeBridgeCredential(f.root, f.env), false);
  f.cleanup();
});
