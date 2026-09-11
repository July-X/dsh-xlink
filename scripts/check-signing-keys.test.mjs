// 覆盖签名密钥成对校验的解析与验签逻辑（P2-51）。
// 用 node:crypto 现场生成一对 Ed25519 密钥，按 minisign 的字节布局包装成
// Tauri 的公钥/签名形态，因此不需要 minisign，也不需要真实的 CI 私钥。
import assert from 'node:assert/strict';
import { generateKeyPairSync, sign as cryptoSign } from 'node:crypto';
import test from 'node:test';

import {
  decodePublicKey,
  decodeSignature,
  normalizePublicKey,
  verifyPayload,
} from './check-signing-keys.mjs';

const ED25519_SPKI_PREFIX = Buffer.from('302a300506032b6570032100', 'hex');

function makeKeyPair(keyId = '188a15e36ba131b1') {
  const { publicKey, privateKey } = generateKeyPairSync('ed25519');
  const rawPublic = publicKey.export({ format: 'der', type: 'spki' }).subarray(12);
  const publicKeyBase64 = Buffer.concat([
    Buffer.from([0x45, 0x64]), // "Ed" —— minisign 的签名算法标识
    Buffer.from(keyId, 'hex'),
    rawPublic,
  ]).toString('base64');
  return { privateKey, publicKeyBase64, keyId };
}

function signPayload(privateKey, keyId, payload) {
  const signature = cryptoSign(null, payload, privateKey);
  return Buffer.concat([
    Buffer.from([0x45, 0x64]),
    Buffer.from(keyId, 'hex'),
    signature,
  ]).toString('base64');
}

test('a signature made by the matching private key verifies', () => {
  const { privateKey, publicKeyBase64, keyId } = makeKeyPair();
  const payload = Buffer.from('release payload\n');
  const signatureBase64 = signPayload(privateKey, keyId, payload);

  assert.equal(verifyPayload({ publicKeyBase64, payload, signatureBase64 }).ok, true);
  assert.equal(decodePublicKey(publicKeyBase64).keyId, keyId);
  assert.equal(decodeSignature(signatureBase64).keyId, keyId);
});

test('a rotated public key is reported as a mismatch, not silently accepted', () => {
  // 这正是 P2-51 的场景：私钥换了、tauri.conf.json 的公钥没换。
  const signing = makeKeyPair();
  const configured = makeKeyPair();
  const payload = Buffer.from('release payload\n');
  const signatureBase64 = signPayload(signing.privateKey, signing.keyId, payload);

  const result = verifyPayload({
    publicKeyBase64: configured.publicKeyBase64,
    payload,
    signatureBase64,
  });
  assert.equal(result.ok, false);
  assert.match(result.reason, /key id 不一致|验签失败/);
});

test('the real tauri.conf.json pubkey shape (wrapped minisign file) is understood', () => {
  // tauri.conf.json 里存的是整个 minisign 公钥文件再 base64；直接当 42 字节
  // 解析会得到 114 字节并报错，必须先把外层剥掉。
  const { privateKey, publicKeyBase64, keyId } = makeKeyPair();
  const wrapped = Buffer.from(
    `untrusted comment: minisign public key: ${keyId.toUpperCase()}\n${publicKeyBase64}\n`,
  ).toString('base64');
  const payload = Buffer.from('release payload\n');
  const signatureBase64 = signPayload(privateKey, keyId, payload);

  assert.equal(normalizePublicKey(wrapped), publicKeyBase64);
  assert.equal(verifyPayload({ publicKeyBase64: wrapped, payload, signatureBase64 }).ok, true);
});

test('a signature carrying a foreign key id is rejected even when the math checks out', () => {
  // key id 是"这两把钥匙是一对"的显式声明，Tauri 的验证器也会比对它；这里
  // 保留正确的密钥材料但把 id 字段改掉，只有真的比较 id 才会拒绝。
  const { privateKey, publicKeyBase64, keyId } = makeKeyPair();
  const payload = Buffer.from('release payload\n');
  const good = Buffer.from(signPayload(privateKey, keyId, payload), 'base64');
  const foreignKeyId = Buffer.from('0011223344556677', 'hex');
  foreignKeyId.copy(good, 2);

  const result = verifyPayload({
    publicKeyBase64,
    payload,
    signatureBase64: good.toString('base64'),
  });
  assert.equal(result.ok, false);
  assert.match(result.reason, /key id 不一致/);
});

test('tampered payloads and malformed keys are rejected', () => {
  const { privateKey, publicKeyBase64, keyId } = makeKeyPair();
  const payload = Buffer.from('release payload\n');
  const signatureBase64 = signPayload(privateKey, keyId, payload);

  const tampered = verifyPayload({
    publicKeyBase64,
    payload: Buffer.from('release payload!\n'),
    signatureBase64,
  });
  assert.equal(tampered.ok, false);

  assert.throws(() => decodePublicKey(Buffer.alloc(8).toString('base64')), /公钥长度异常/);
  assert.throws(() => decodeSignature(Buffer.alloc(8).toString('base64')), /签名长度异常/);
});
