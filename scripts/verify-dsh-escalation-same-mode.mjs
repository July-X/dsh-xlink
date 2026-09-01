#!/usr/bin/env node
// 验证 dsh-escalation-same-mode 补丁的清单、目标 / 载荷哈希、行为正确性。
//该补丁走"最朴素的 copy/replace"语义：apply = fs::copy(payload, target) + 备份原文件，
//revert = fs::copy(backup, target) + 删除备份。验证脚本只看"读写结果"，不关心匹配机制。
//
// 用法：
//   node scripts/verify-dsh-escalation-same-mode.mjs                  # 自动定位激活内核
//   node scripts/verify-dsh-escalation-same-mode.mjs <内核根目录>
//   node scripts/verify-dsh-escalation-same-mode.mjs --require-applied
//
// 默认只读：不会修改内核目录或 ~/.dsh。行为测试把载荷文件复制到临时目录，
// 并通过临时 node_modules 最小依赖桩加载载荷，避免依赖当前用户内核版本。

import { existsSync, readFileSync } from 'node:fs';
import { mkdtemp, mkdir, rm, readFile, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { homedir, tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { spawnSync } from 'node:child_process';

const DSH_HOME = process.env.DSH_HOME ?? join(homedir(), '.dsh');
const PATCH_ID = 'dsh-escalation-same-mode';
const PATCH_VERSION = '1.0.0';
const KERNEL_VERSION = '0.1.1-rc.2';
const TARGET = 'node_modules/@deepseek-ai/dsh-sandbox/lib/index.js';
const PATCH_DIR = resolve('src-tauri/resources/patches/dsh-escalation-same-mode');
const MANIFEST = join(PATCH_DIR, 'manifest.json');
const PAYLOAD = join(PATCH_DIR, 'files/dsh-sandbox/index.js');
// 与 manifest.expectSha256 严格一致：这是「原始 dist 文件」的 SHA，apply 必须以这个
// 安全闸确认目标未被第三方工具改过。
const ORIGINAL_SHA256 = '63ee2a10873a336162acd9a0d7da7f5f3dc59d072456a0b5271da277565e324f';
// 与 PATCHED_SHA 一致：载荷本身的 SHA，apply 之后目标文件应等于这个。
const PATCHED_SHA256 = 'dafc42d296d5757dbe0f626d2dbd79de7db444d3927833318f68afb831122c5e';
const SEARCH_MARKER = 'if (mode === effectiveMode) return mode;';

const requireApplied = process.argv.includes('--require-applied');
const positional = process.argv.slice(2).find((arg) => !arg.startsWith('--'));

let failures = 0;
const check = (name, ok, detail = '') => {
  console.log(`${ok ? '✓' : '✗'} ${name}${detail ? `  ${detail}` : ''}`);
  if (!ok) failures += 1;
};

function sha256(bytes) {
  return createHash('sha256').update(bytes).digest('hex');
}

function resolveKernelRoot() {
  if (positional) return resolve(positional);
  for (const variant of ['desktop', 'desktop-dev']) {
    const activeFile = join(DSH_HOME, variant, 'active.txt');
    try {
      const version = readFileSync(activeFile, 'utf8').trim();
      const root = join(DSH_HOME, variant, 'kernels', version);
      if (existsSync(root)) return root;
    } catch {
      // Try the next DSH data variant.
    }
  }
  throw new Error(`找不到激活内核：请显式传入内核根目录，或检查 ${DSH_HOME}/desktop/active.txt`);
}

async function loadPatch(kernelRoot) {
  const manifest = JSON.parse(await readFile(MANIFEST, 'utf8'));
  check('manifest schemaVersion=1', manifest.schemaVersion === 1);
  const patch = manifest.patches?.find((candidate) => candidate.id === PATCH_ID);
  check(`manifest 包含 ${PATCH_ID}`, patch !== undefined);
  if (patch === undefined) throw new Error(`manifest 中缺少 ${PATCH_ID}`);
  check(`补丁版本为 ${PATCH_VERSION}`, patch.version === PATCH_VERSION);
  check(
    `补丁版本范围精确覆盖 ${KERNEL_VERSION}`,
    patch.minKernelVersion === KERNEL_VERSION && patch.maxKernelVersion === KERNEL_VERSION,
  );
  check(
    '补丁只声明一个 copy + expectSha256 文件',
    patch.files?.length === 1 && patch.files[0]?.mode === 'copy' && patch.files[0]?.to === TARGET,
  );
  const file = patch.files?.[0];
  if (file === undefined || file.mode !== 'copy' || typeof file.from !== 'string') {
    throw new Error('manifest 中缺少 copy 文件或 from 字段');
  }
  check('manifest expectSha256 与原始 dist 一致', file.expectSha256 === ORIGINAL_SHA256);

  // 载荷本体：copy 模式的「真值」就是 payload 文件本身
  const payloadSource = await readFile(PAYLOAD, 'utf8');
  const payloadSha = sha256(payloadSource);
  check('载荷文件存在', existsSync(PAYLOAD));
  check('载荷 SHA-256 与 PATCHED_SHA 常量一致', payloadSha === PATCHED_SHA256, `sha256=${payloadSha}`);
  check('载荷包含同模式短路', payloadSource.includes(SEARCH_MARKER));

  // 目标文件：显式传入内核根目录或 --require-applied 时严格校验；
  // 自动发现到不适用的其它内核版本时只报告并跳过，避免发布门禁依赖开发机状态。
  const targetPath = join(kernelRoot, TARGET);
  const targetSource = await readFile(targetPath, 'utf8');
  const targetSha = sha256(targetSource);
  const isPatched = targetSha === PATCHED_SHA256;
  const targetMatchesPatch = targetSha === ORIGINAL_SHA256 || isPatched;
  if (targetMatchesPatch || requireApplied || positional) {
    check(
      '目标文件为原始版本或补丁后版本',
      targetMatchesPatch,
      'sha256=' + targetSha,
    );
  } else {
    console.log('! 自动发现的内核不适用 ' + PATCH_ID + '（sha256=' + targetSha + '），跳过目标文件校验；显式传入 0.1.1-rc.2 内核根目录或追加 --require-applied 进行严格检查');
  }
  if (requireApplied) check('目标文件已应用补丁', isPatched);

  // copy 模式最朴素的保证：若目标已是补丁后版本，字节级应等于 payload
  //（不靠任何搜索串比对，而是直接覆盖后的字节一致性）
  if (isPatched) {
    check(
      '已应用的目标文件字节级等于载荷文件',
      targetSource === payloadSource,
      '差异字节数 = ' + (Buffer.from(targetSource).equals(Buffer.from(payloadSource)) ? '0' : '>0'),
    );
  }
  return { patch, targetPath, payloadSource, targetSha, targetMatchesPatch };
}

async function syntaxCheck(source) {
  const tempRoot = await mkdtemp(join(tmpdir(), 'dsh-escalation-same-mode-syntax-'));
  const tempFile = join(tempRoot, 'index.js');
  try {
    await writeFile(tempFile, source, 'utf8');
    const result = spawnSync(process.execPath, ['--check', tempFile], { encoding: 'utf8' });
    check('载荷通过 Node 语法检查', result.status === 0, result.stderr?.trim().split('\n')[0] ?? '');
  } finally {
    await rm(tempRoot, { recursive: true, force: true });
  }
}

async function behaviorCheck(source) {
  const tempRoot = await mkdtemp(join(tmpdir(), 'dsh-escalation-same-mode-module-'));
  const packageRoot = join(tempRoot, 'node_modules', '@deepseek-ai', 'dsh-sandbox');
  const moduleDir = join(packageRoot, 'lib');
  const moduleFile = join(moduleDir, 'index.js');
  try {
    await mkdir(moduleDir, { recursive: true });
    await writeFile(join(packageRoot, 'package.json'), JSON.stringify({ type: 'module', main: 'index.js', exports: './index.js' }), 'utf8');

    // 载荷只需要 Service、HarnessError、assertNever；用最小桩固定行为测试的依赖契约。
    const dependencyRoot = join(tempRoot, 'node_modules', '@deepseek-ai');
    const cordisRoot = join(dependencyRoot, 'cordis');
    const llmRoot = join(dependencyRoot, 'dsh-llm');
    await mkdir(cordisRoot, { recursive: true });
    await mkdir(llmRoot, { recursive: true });
    await writeFile(join(cordisRoot, 'package.json'), JSON.stringify({ type: 'module', main: 'index.js', exports: './index.js' }), 'utf8');
    await writeFile(join(cordisRoot, 'index.js'), 'export class Service {}', 'utf8');
    await writeFile(join(llmRoot, 'package.json'), JSON.stringify({ type: 'module', main: 'index.js', exports: './index.js' }), 'utf8');
    const llmSource = [
      "export class HarnessError extends Error { constructor(message, code) { super(message); this.code = code; } }",
      "export function assertNever(value, label) { throw new Error(String(label ?? 'Unexpected value') + ': ' + String(value)); }",
    ].join(String.fromCharCode(10));
    await writeFile(join(llmRoot, 'index.js'), llmSource, 'utf8');
    await writeFile(moduleFile, source, 'utf8');
    const moduleUrl = `${pathToFileURL(moduleFile).href}?dshEscalationSameMode=${Date.now()}`;
    const sandbox = await import(moduleUrl);

    check('导出 WIDER_MODES 表', sandbox.WIDER_MODES !== undefined && typeof sandbox.WIDER_MODES === 'object');
    check('导出 approveEscalation 函数', typeof sandbox.approveEscalation === 'function');
    check('导出 SandboxUnavailableError 错误类', typeof sandbox.SandboxUnavailableError === 'function');

    const baseRequest = { requestedMode: 'workspace-write', justification: 'unit test', subject: 'command' };

    const same = await sandbox.approveEscalation(
      { ...baseRequest, requestedMode: 'danger-full-access', effectiveMode: 'danger-full-access' },
      { approver: undefined, agent: undefined, toolName: 'bash', callId: 't1' },
    );
    check('effectiveMode === requestedMode 直接返回 requestedMode', same === 'danger-full-access');

    const sameWorkspace = await sandbox.approveEscalation(
      { ...baseRequest, requestedMode: 'workspace-write', effectiveMode: 'workspace-write' },
      { approver: undefined, agent: undefined, toolName: 'bash', callId: 't2' },
    );
    check('workspace-write 同模式同样短路', sameWorkspace === 'workspace-write');

    let narrowing = '';
    try {
      await sandbox.approveEscalation(
        { ...baseRequest, requestedMode: 'workspace-write', effectiveMode: 'danger-full-access' },
        { approver: undefined, agent: undefined, toolName: 'bash', callId: 't3' },
      );
    } catch (e) {
      narrowing = String(e?.message ?? e);
    }
    check(
      '缩小请求仍按原路径抛 not strictly wider',
      narrowing.includes('not strictly wider') && narrowing.includes('workspace-write') && narrowing.includes('danger-full-access'),
      narrowing,
    );

    let sameAfterPatch = '';
    try {
      await sandbox.approveEscalation(
        { ...baseRequest, requestedMode: 'read-only', effectiveMode: 'workspace-write' },
        { approver: undefined, agent: undefined, toolName: 'bash', callId: 't4' },
      );
    } catch (e) {
      sameAfterPatch = String(e?.message ?? e);
    }
    check('workspace-write → read-only 仍按原路径抛错', sameAfterPatch.includes('not strictly wider'));

    let noApprover = '';
    try {
      await sandbox.approveEscalation(
        { ...baseRequest, requestedMode: 'danger-full-access', effectiveMode: 'workspace-write' },
        { approver: undefined, agent: { id: 'agent-1', session: {} }, toolName: 'bash', callId: 't5' },
      );
    } catch (e) {
      noApprover = String(e?.message ?? e);
    }
    check('无 approver 仍按原路径抛错', noApprover.includes('no approval service'));

    let noAgent = '';
    try {
      await sandbox.approveEscalation(
        { ...baseRequest, requestedMode: 'danger-full-access', effectiveMode: 'workspace-write' },
        { approver: { request: async () => 'allowed-once' }, agent: undefined, toolName: 'bash', callId: 't6' },
      );
    } catch (e) {
      noAgent = String(e?.message ?? e);
    }
    check('无 agent 仍按原路径抛错', noAgent.includes('no agent to route it through'));

    let calls = 0;
    const approver = {
      request: async () => {
        calls += 1;
        return 'allowed-once';
      },
    };
    const granted = await sandbox.approveEscalation(
      { ...baseRequest, requestedMode: 'danger-full-access', effectiveMode: 'workspace-write' },
      { approver, agent: { id: 'agent-1', session: {} }, toolName: 'bash', callId: 't7' },
    );
    check('workspace-write → danger-full-access 仍走审批流', granted === 'danger-full-access' && calls === 1);

    let cancelMessage = '';
    const canceller = {
      request: async () => 'cancelled',
    };
    try {
      await sandbox.approveEscalation(
        { ...baseRequest, requestedMode: 'danger-full-access', effectiveMode: 'workspace-write' },
        { approver: canceller, agent: { id: 'agent-1', session: {} }, toolName: 'bash', callId: 't8' },
      );
    } catch (e) {
      cancelMessage = String(e?.message ?? e);
    }
    check('审批取消仍按原路径抛错', cancelMessage.includes('was cancelled'));
  } finally {
    await rm(tempRoot, { recursive: true, force: true });
  }
}

async function main() {
  const kernelRoot = resolveKernelRoot();
  console.log(`内核根目录：${kernelRoot}`);
  const { payloadSource, targetSha } = await loadPatch(kernelRoot);
  await syntaxCheck(payloadSource);
  await behaviorCheck(payloadSource);
  const targetState = targetSha === ORIGINAL_SHA256 ? '未应用（载荷验证模式）' : targetSha === PATCHED_SHA256 ? '已应用' : '不适用（目标 SHA 与补丁不匹配）';
  console.log(`\n当前目标状态：${targetState}`);
  if (requireApplied && failures === 0) console.log('补丁已应用且验证通过');
  else if (!requireApplied && failures === 0) console.log('载荷验证通过；应用后追加 --require-applied 检查目标文件');
  process.exitCode = failures === 0 ? 0 : 1;
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  await main();
}
