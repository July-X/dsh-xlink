// 校验「CI 私钥」与「tauri.conf.json 里的 updater 公钥」是否成对。
//
// 密钥轮换时只换一半是很容易发生的事故：签出来的更新包所有客户端都验签失败，
// 而且直到用户点「检查更新」才会暴露（P2-51）。发布前用私钥签一个测试载荷、
// 再用配置里的公钥验签，就能在打包之前拦住。
//
// 不依赖 minisign：Tauri 的签名是 Ed25519，node:crypto 直接可验。公钥/签名都是
// minisign 的 base64 结构：`[2 字节算法][8 字节 key id][32 字节公钥]` 与
// `[2 字节算法][8 字节 key id][64 字节签名]`。
import { createPublicKey, verify as cryptoVerify } from 'node:crypto';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const PUBLIC_KEY_PREFIX_BYTES = 10; // 算法(2) + key id(8)
const SIGNATURE_PREFIX_BYTES = 10;
const ED25519_KEY_BYTES = 32;
const ED25519_SIGNATURE_BYTES = 64;
// Ed25519 公钥的 SPKI 头（RFC 8410）：node 只能从 DER 构造 KeyObject。
const ED25519_SPKI_PREFIX = Buffer.from('302a300506032b6570032100', 'hex');

/// 归一化公钥输入。
///
/// `tauri.conf.json` 的 `plugins.updater.pubkey` 是**整个 minisign 公钥文件**
/// 再经一次 base64：解码后是两行文本（`untrusted comment: …` 与真正的公钥），
/// 所以要先剥掉外层。也接受直接给出的 42 字节 base64（本脚本的单测就是这么
/// 构造的），因此两种形态都要认。
export function normalizePublicKey(input) {
  const decoded = Buffer.from(input, 'base64').toString('utf8');
  if (decoded.includes('untrusted comment:')) {
    const line = decoded
      .split('\n')
      .map((entry) => entry.trim())
      .find((entry) => entry && !entry.startsWith('untrusted comment:'));
    if (!line) throw new Error('minisign 公钥文件里找不到 base64 行');
    return line;
  }
  return input;
}

/// 解析 minisign 形态的公钥，返回 { keyId, keyObject }。
export function decodePublicKey(input) {
  const raw = Buffer.from(normalizePublicKey(input), 'base64');
  if (raw.length !== PUBLIC_KEY_PREFIX_BYTES + ED25519_KEY_BYTES) {
    throw new Error(`公钥长度异常（${raw.length} 字节，期望 42）`);
  }
  const keyId = raw.subarray(2, PUBLIC_KEY_PREFIX_BYTES).toString('hex');
  const der = Buffer.concat([ED25519_SPKI_PREFIX, raw.subarray(PUBLIC_KEY_PREFIX_BYTES)]);
  return { keyId, keyObject: createPublicKey({ key: der, format: 'der', type: 'spki' }) };
}

/// 归一化签名输入。
///
/// `tauri signer sign` 写出的 `.sig` 是**两行**的 minisign 签名文件：
/// `untrusted comment: …` 加一行 base64。直接把这整段当 base64 解码会得到一个
/// 294 字节的垃圾（首次真实发布就是这么失败的），所以先取最后一行非空文本。
/// 只给单行 base64 的调用方（例如本脚本的单测）同样接受。
export function normalizeSignature(input) {
  const lines = String(input)
    .split('\n')
    .map((line) => line.trim())
    .filter(Boolean);
  if (lines.length === 0) throw new Error('签名内容为空');
  return lines[lines.length - 1];
}

/// 解析 minisign 形态的签名，返回 { keyId, signature }。
export function decodeSignature(input) {
  const raw = Buffer.from(normalizeSignature(input), 'base64');
  if (raw.length !== SIGNATURE_PREFIX_BYTES + ED25519_SIGNATURE_BYTES) {
    throw new Error(`签名长度异常（${raw.length} 字节，期望 74）`);
  }
  return {
    keyId: raw.subarray(2, SIGNATURE_PREFIX_BYTES).toString('hex'),
    signature: raw.subarray(SIGNATURE_PREFIX_BYTES),
  };
}

/// 用公钥验证载荷签名。key id 不一致直接判失败：那说明公私钥根本不是一对。
export function verifyPayload({ publicKeyBase64, payload, signatureBase64 }) {
  const { keyId, keyObject } = decodePublicKey(publicKeyBase64);
  const { keyId: sigKeyId, signature } = decodeSignature(signatureBase64);
  if (keyId !== sigKeyId) {
    return { ok: false, reason: `key id 不一致（公钥 ${keyId}，签名 ${sigKeyId}）` };
  }
  return cryptoVerify(null, payload, keyObject, signature)
    ? { ok: true }
    : { ok: false, reason: 'Ed25519 验签失败（公私钥不匹配或载荷被改动）' };
}

function run(command, args, options = {}) {
  return spawnSync(command, args, { encoding: 'utf8', ...options });
}

/// 用 CI 私钥签一份测试载荷并返回签名内容。
export function signWithPrivateKey({ privateKey, password, payload, cwd }) {
  const dir = mkdtempSync(join(tmpdir(), 'dsh-signing-check-'));
  try {
    const target = join(dir, 'probe.bin');
    writeFileSync(target, payload);
    const result = run('pnpm', ['exec', 'tauri', 'signer', 'sign', target], {
      cwd,
      env: {
        ...process.env,
        TAURI_SIGNING_PRIVATE_KEY: privateKey,
        TAURI_SIGNING_PRIVATE_KEY_PASSWORD: password ?? '',
      },
    });
    if (result.status !== 0) {
      throw new Error(
        `tauri signer sign 失败（退出码 ${result.status}）：${(result.stderr || result.stdout || '').trim().slice(0, 400)}`,
      );
    }
    // 返回 `.sig` 的原文（含 `untrusted comment:` 行）：归一化由
    // `verifyPayload` → `normalizeSignature` 统一负责。
    return readFileSync(`${target}.sig`, 'utf8');
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

function main() {
  const cwd = process.cwd();
  const privateKey = process.env.TAURI_SIGNING_PRIVATE_KEY;
  if (!privateKey) {
    console.log('! 未提供 TAURI_SIGNING_PRIVATE_KEY，跳过签名密钥成对校验');
    return;
  }
  const config = JSON.parse(readFileSync(join(cwd, 'src-tauri/tauri.conf.json'), 'utf8'));
  const publicKeyBase64 = config?.plugins?.updater?.pubkey;
  if (!publicKeyBase64) {
    console.error('✗ tauri.conf.json 缺少 plugins.updater.pubkey，无法校验密钥对');
    process.exitCode = 1;
    return;
  }

  const payload = Buffer.from(`dsh-xlink signing probe ${new Date().toISOString()}\n`);
  let signatureBase64;
  try {
    signatureBase64 = signWithPrivateKey({
      privateKey,
      password: process.env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD,
      payload,
      cwd,
    });
  } catch (error) {
    console.error(`✗ 无法用 CI 私钥签名测试载荷：${error.message}`);
    process.exitCode = 1;
    return;
  }

  const result = verifyPayload({ publicKeyBase64, payload, signatureBase64 });
  if (!result.ok) {
    console.error(
      `✗ CI 私钥与 tauri.conf.json 的 updater 公钥不是一对：${result.reason}。` +
        '请同步轮换两处（公钥在 src-tauri/tauri.conf.json 的 plugins.updater.pubkey，私钥在仓库 secret TAURI_SIGNING_PRIVATE_KEY）',
    );
    process.exitCode = 1;
    return;
  }
  console.log('✓ 签名密钥成对：CI 私钥签出的载荷能被配置里的公钥验证');
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main();
}
