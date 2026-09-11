// 覆盖签名密钥成对校验的解析与验签逻辑（P2-51）。
// 用 node:crypto 现场生成一对 Ed25519 密钥，按 minisign 的字节布局包装成
// Tauri 的公钥/签名形态，因此不需要 minisign，也不需要真实的 CI 私钥。
import assert from 'node:assert/strict';
import { createHash, generateKeyPairSync, sign as cryptoSign } from 'node:crypto';
import test from 'node:test';

import {
  decodePublicKey,
  decodeSignature,
  normalizePublicKey,
  normalizeSignature,
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

// 按**真实** minisign/Tauri 结构造签名：
// 内层 Ed25519 签的是 BLAKE2b-512(payload)，包成 [2 字节算法][8 字节 key id][64 字节签名]，
// 再写成 minisign 签名文件文本（untrusted comment + 折行的 base64 + trusted comment +
// 全局签名），最后整个文本再 base64 一次 —— 与已发布 rc.18 制品的 `.sig` 完全同构。
function signPayload(privateKey, keyId, payload) {
  const digest = createHash('blake2b512').update(payload).digest();
  const signature = cryptoSign(null, digest, privateKey);
  const blob = Buffer.concat([Buffer.from([0x45, 0x64]), Buffer.from(keyId, 'hex'), signature]);
  const inner = blob.toString('base64');
  const folded = inner.match(/.{1,76}/g).join('\n');
  const text =
    `untrusted comment: signature from tauri secret key\n${folded}\n` +
    `trusted comment: timestamp:1789000120\tfile:probe\n${'A'.repeat(86)}\n`;
  return Buffer.from(text).toString('base64');
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

// 把 74 字节签名结构包成 Tauri 的 `.sig`（双层 base64 + minisign 文本），
// 便于对"外层被改动"的场景构造输入。
function wrapSignature(blob) {
  const folded = blob.toString('base64').match(/.{1,76}/g).join('\n');
  const text =
    `untrusted comment: signature from tauri secret key\n${folded}\n` +
    `trusted comment: timestamp:1789000120\tfile:probe\n${'A'.repeat(86)}\n`;
  return Buffer.from(text).toString('base64');
}

function signatureBlob(privateKey, keyId, payload) {
  const digest = createHash('blake2b512').update(payload).digest();
  return Buffer.concat([
    Buffer.from([0x45, 0x64]),
    Buffer.from(keyId, 'hex'),
    cryptoSign(null, digest, privateKey),
  ]);
}

test('the real .sig shape (double base64 + minisign text) is understood', () => {
  // 已发布 rc.18 制品的 `.sig` 是**双层** base64：外层解码出 minisign 签名文件
  // 文本（untrusted comment + 折行的签名 base64 + trusted comment + 全局签名），
  // 内层才是 74 字节结构。首次真实发布（desktop-v0.1.2-rc.19）先后踩了两个坑：
  // 把整段当 base64（解出 294 字节）、以及只取最后一行（trusted comment 的
  // 全局签名，解出 64 字节）。这条用例把真实结构钉住。
  const { privateKey, publicKeyBase64, keyId } = makeKeyPair();
  const payload = Buffer.from('release payload\n');
  const blob = signatureBlob(privateKey, keyId, payload);
  const sigFile = wrapSignature(blob);

  const normalized = normalizeSignature(sigFile);
  assert.equal(normalized, blob.toString('base64'), '应当取出第一段签名 base64');
  assert.equal(Buffer.from(normalized, 'base64').length, 74);
  assert.equal(decodeSignature(sigFile).keyId, keyId);
  assert.equal(verifyPayload({ publicKeyBase64, payload, signatureBase64: sigFile }).ok, true);

  // 直接给 74 字节结构的单行 base64（本脚本早期形态）也必须继续可用。
  const singleLine = blob.toString('base64');
  assert.equal(normalizeSignature(singleLine), singleLine);
  assert.equal(verifyPayload({ publicKeyBase64, payload, signatureBase64: singleLine }).ok, true);
});

test('the published rc.18 signature matches the configured updater pubkey', async () => {
  // 用真实数据（已发布制品的签名是公开信息）做**离线**核对：签名里的 key id 必须
  // 等于 tauri.conf.json 里公钥的 key id。密钥轮换只换一半时这里立刻失配 ——
  // 这正是 P2-51 要拦的事故，而且不依赖网络或 CI secret。
  const { readFileSync } = await import('node:fs');
  const sig = readFileSync(new URL('./fixtures/tauri-signature-sample.sig', import.meta.url), 'utf8');
  const config = JSON.parse(
    readFileSync(new URL('../src-tauri/tauri.conf.json', import.meta.url), 'utf8'),
  );
  const blob = Buffer.from(normalizeSignature(sig), 'base64');
  assert.equal(blob.length, 74, '应当解析出 74 字节的 minisign 签名结构');
  assert.equal(
    blob.subarray(2, 10).toString('hex'),
    decodePublicKey(config.plugins.updater.pubkey).keyId,
    '已发布制品的签名 key id 必须与配置里的公钥一致（否则下一次更新会验签失败）',
  );
});

test('a signature carrying a foreign key id is rejected even when the math checks out', () => {
  // key id 是"这两把钥匙是一对"的显式声明，Tauri 的验证器也会比对它；这里
  // 保留正确的密钥材料但把 id 字段改掉，只有真的比较 id 才会拒绝。
  const { privateKey, publicKeyBase64, keyId } = makeKeyPair();
  const payload = Buffer.from('release payload\n');
  const good = signatureBlob(privateKey, keyId, payload);
  Buffer.from('0011223344556677', 'hex').copy(good, 2);

  const result = verifyPayload({
    publicKeyBase64,
    payload,
    signatureBase64: wrapSignature(good),
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
