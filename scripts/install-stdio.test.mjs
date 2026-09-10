// `scripts/install.mjs`（`npm run deps` 的实现）必须把包管理器的输出**实时**
// 透传到终端：用 `execFileSync` 的默认捕获模式会让依赖安装期间终端一片空白，
// 且默认 1 MiB 的 `maxBuffer` 会在输出超限时以难以理解的 `ENOBUFS` 失败。
//
// 这里不安装任何东西：PATH 前置一个桩 `pnpm`，它只打印版本号和 1.8 MiB 文本，
// 用真实的 `install.mjs` 进程验证输出完整到达、且失败时原因可见。
import assert from 'node:assert/strict';
import { chmodSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import test from 'node:test';
import { join } from 'node:path';
import { tmpdir } from 'node:os';

const STUB_LINE_COUNT = 16000;
// 单行约 112 字节：16000 行 ≈ 1.8 MB，稳稳超过 execFileSync 默认的 1 MiB。
const STUB_LINE =
  'line: 0123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789';
const CAPTURE_MAX_BUFFER = 64 * 1024 * 1024;

/** 在临时目录里放一个 `pnpm` 桩，返回 { root, env }。 */
function withStubPnpm(installBody, callback) {
  const root = mkdtempSync(join(tmpdir(), 'dsh-install-stdio-'));
  const stub = join(root, 'pnpm');
  writeFileSync(
    stub,
    `#!/bin/sh\nif [ "$1" = "--version" ]; then echo "9.0.0"; exit 0; fi\n${installBody}\n`,
  );
  chmodSync(stub, 0o755);
  const env = { ...process.env, PATH: `${root}:${process.env.PATH ?? ''}` };
  try {
    return callback(env);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

function runInstall(env) {
  return spawnSync(process.execPath, [join(process.cwd(), 'scripts', 'install.mjs')], {
    env,
    encoding: 'utf8',
    maxBuffer: CAPTURE_MAX_BUFFER,
  });
}

const posixOnly = process.platform === 'win32' ? '仅 POSIX：桩使用 sh 脚本' : false;

test('install.mjs 把超过 1 MiB 的包管理器输出完整透传', { skip: posixOnly }, () => {
  withStubPnpm(
    `i=0\nwhile [ $i -lt ${STUB_LINE_COUNT} ]; do echo "${STUB_LINE}"; i=$((i+1)); done`,
    (env) => {
      const result = runInstall(env);
      const output = `${result.stdout ?? ''}${result.stderr ?? ''}`;
      assert.equal(result.status, 0, `应成功退出，实际 ${result.status}：${output.slice(-400)}`);
      assert.ok(
        !output.includes('ENOBUFS'),
        '不应出现 ENOBUFS —— 说明仍在用捕获模式，未设 stdio: "inherit"',
      );
      const lines = output.split('\n').filter((line) => line.startsWith('line: '));
      assert.equal(lines.length, STUB_LINE_COUNT, '每一行桩输出都应到达父进程的流');
      assert.ok(lines.length * STUB_LINE.length > 1024 * 1024, '样本量必须真的超过 1 MiB');
    },
  );
});

test('install.mjs 找不到包管理器时说明原因，而不是打印「退出码 ?」', { skip: posixOnly }, () => {
  // PATH 里既没有 pnpm 也没有 npm —— 走 `spawnSync ... ENOENT` 分支，此时
  // `err.status` 是 undefined。旧实现只打印 `err.status ?? '?'`，用户看到的
  // 是一句没有诊断价值的「退出码 ?」；这里要求把底层原因带出来。
  const emptyPath = mkdtempSync(join(tmpdir(), 'dsh-install-nopath-'));
  try {
    const result = runInstall({ ...process.env, PATH: emptyPath });
    assert.notEqual(result.status, 0, '找不到包管理器必须以非零退出');
    const stderr = result.stderr ?? '';
    assert.match(stderr, /install 失败/, '应说明是安装失败');
    assert.match(stderr, /ENOENT/, `应带出底层原因（ENOENT），实际：${stderr}`);
    assert.ok(
      !stderr.includes('退出码 ?'),
      '不该再出现「退出码 ?」这类没有诊断价值的占位符',
    );
    assert.match(stderr, /npm run deps/, '应给出可操作的下一步');
  } finally {
    rmSync(emptyPath, { recursive: true, force: true });
  }
});

test('install.mjs 把包管理器的非零退出码原样透出', { skip: posixOnly }, () => {
  withStubPnpm('echo "桩：registry 不可达" >&2\nexit 7', (env) => {
    const result = runInstall(env);
    assert.equal(result.status, 7, '应把子进程退出码原样透出');
    assert.match(result.stderr ?? '', /registry 不可达/, '子进程 stderr 应实时透传到终端');
  });
});
