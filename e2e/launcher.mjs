// E2E 统一启动器（2026-11-01 OPT-08）。
//
// 目的：`npm run e2e` 不再裸调 wdio，而是由本脚本创建本轮独占的测试根、
// 浏览器档案、单实例通道与证据目录，并在构建前注入编译期隔离配置。这样
// 连续两轮不会复用数据或档案，也不会把真实家目录交给测试壳。
//
// 隔离四项（缺一不可）：
//   1. 临时 DSH_XLINK_HOME：设置、实例、内核树、插件物化；
//   2. 编译期 TAURI_CONFIG.app.appDirectoriesOverride：浏览器档案与 app_*_dir；
//   3. 独立 DSH_XLINK_CHANNEL_SUFFIX：单实例通道，防止交接给已运行的安装版；
//   4. 独立内核端口（settings.json）：避免把宿主 3090/3091 上正在跑的内核误判成本进程的。
//
// 构建输出也按轮隔离（CARGO_TARGET_DIR），避免复用未注入本轮配置的旧 debug
// 二进制；代价是每轮首次编译变慢，这是为「隔离由代码保证」付的必要成本。
//
// 失败时保留本轮目录并把路径打出来；成功时只删除带本轮归属标记的目录。

import { createHash, randomBytes } from 'node:crypto';
import { spawn, spawnSync } from 'node:child_process';
import { appendFileSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { createServer } from 'node:net';
import { dirname, isAbsolute, join, relative, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, '..');
const RUN_SCHEMA = 1;
const MARKER_NAME = '.dsh-e2e-run.json';

export function debugBinaryName(platform = process.platform) {
  return platform === 'win32' ? 'dsh-xlink.exe' : 'dsh-xlink';
}

export function makeRunId(now = Date.now(), entropy) {
  const stamp = new Date(now).toISOString().replace(/[-:TZ.]/g, '').slice(0, 14);
  const suffix = entropy || randomBytes(3).toString('hex');
  return 'e2e-' + stamp + '-' + process.pid + '-' + suffix;
}

// 本轮独占的 loopback 端口：3090/3091 可能已有真实壳或另一个测试壳的内核在跑，
// 复用会把 status.kernel.running 误判为 true，启停类用例还会打到宿主内核。
// 从高位端口找第一个能 bind 的；并发轮次各自抢到不同端口。
export async function pickFreePort(start = 35000, attempts = 20) {
  for (let port = start; port < start + attempts; port += 1) {
    const free = await new Promise((resolvePort) => {
      const server = createServer();
      server.once('error', () => resolvePort(false));
      server.once('listening', () => server.close(() => resolvePort(true)));
      server.listen(port, '127.0.0.1');
    });
    if (free) return port;
  }
  throw new Error('找不到可用的隔离端口（' + start + '–' + (start + attempts - 1) + '）');
}

export function markerFor(runId, launcherPid, root) {
  return {
    schemaVersion: RUN_SCHEMA,
    runId,
    launcherPid,
    repoRoot: root,
    createdAt: new Date().toISOString(),
  };
}

// 合并已有 TAURI_CONFIG，只覆盖 app.appDirectoriesOverride，不丢别的键。
export function mergeTauriConfig(raw, overrideRoot) {
  let parsed = {};
  if (raw && String(raw).trim()) {
    try {
      parsed = JSON.parse(raw);
    } catch (error) {
      throw new Error('已有 TAURI_CONFIG 不是合法 JSON：' + error.message);
    }
  }
  if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
    throw new Error('已有 TAURI_CONFIG 必须是 JSON 对象');
  }
  const app = parsed.app && typeof parsed.app === 'object' ? parsed.app : {};
  parsed.app = { ...app, appDirectoriesOverride: overrideRoot };
  return JSON.stringify(parsed);
}

export function inside(root, candidate) {
  const rel = relative(resolve(root), resolve(candidate));
  return rel !== '' && !rel.startsWith('..') && !isAbsolute(rel);
}

// 启动前隔离自检：返回问题列表，空数组才允许继续。
export function isolationProblems(runRoot, marker, expectedRunId, paths) {
  const problems = [];
  if (!marker || marker.schemaVersion !== RUN_SCHEMA || marker.runId !== expectedRunId) {
    problems.push('缺少本轮归属标记或 runId 不匹配');
  }
  for (const [name, value] of Object.entries(paths)) {
    if (!value || !isAbsolute(value)) {
      problems.push(name + ' 必须是绝对路径：' + value);
      continue;
    }
    if (resolve(value) !== resolve(runRoot) && !inside(runRoot, value)) {
      problems.push(name + ' 不在本轮测试根内：' + value);
    }
  }
  const home = paths.homeDir;
  if (home && resolve(home) === resolve(process.env.HOME || process.env.USERPROFILE || '')) {
    problems.push('DSH_XLINK_HOME 不能指向真实家目录');
  }
  return problems;
}

export function buildRuntimeEnv(base, paths, binarySha) {
  const env = {
    ...base,
    DSH_XLINK_HOME: paths.homeDir,
    DSH_HOME: paths.dshHome,
    DSH_XLINK_CHANNEL_SUFFIX: paths.runId,
    DSH_E2E_RUN_ROOT: paths.runRoot,
    DSH_E2E_APP_BINARY: paths.binary,
    DSH_E2E_EXPECTED_HOME: paths.homeDir,
    DSH_E2E_EXPECTED_APP_DATA: paths.appDataDir,
    DSH_E2E_EXPECTED_CHANNEL: paths.runId,
    DSH_E2E_EXPECTED_PORT: String(paths.port),
    DSH_E2E_EVIDENCE_DIR: paths.evidenceDir,
    DSH_E2E_BINARY_SHA256: binarySha,
  };
  // 把用户家目录指到本轮 decoy：任何路径回退都落在测试根里，真实家目录不被读。
  env.HOME = paths.decoyHome;
  env.USERPROFILE = paths.decoyHome;
  // macOS 的 CoreFoundation / WKWebView 走 NSHomeDirectory，不认普通 HOME；
  // CFFIXED_USER_HOME 是让它们也落到 decoy 的已知入口。
  if (process.platform === 'darwin') {
    env.CFFIXED_USER_HOME = paths.decoyHome;
  }
  return env;
}

function sha256File(file) {
  const hash = createHash('sha256');
  hash.update(readFileSync(file));
  return hash.digest('hex');
}

function sha256Text(text) {
  return createHash('sha256').update(text).digest('hex');
}

function runChecked(label, command, args, options, log) {
  log('$ ' + command + ' ' + args.join(' '));
  const result = spawnSync(command, args, { stdio: 'inherit', ...options });
  if (result.error) {
    throw new Error(label + ' 无法执行：' + result.error.message);
  }
  if (result.status !== 0) {
    throw new Error(label + ' 失败（exit ' + (result.status ?? 'signal ' + result.signal) + '）');
  }
}

async function main() {
  const keep = process.argv.includes('--keep');
  const runId = makeRunId();
  const runRoot = resolve(tmpdir(), 'dsh-xlink-' + runId);
  const paths = {
    runRoot,
    runId,
    homeDir: join(runRoot, 'home'),
    appDataDir: join(runRoot, 'appdata'),
    evidenceDir: join(runRoot, 'evidence'),
    decoyHome: join(runRoot, 'decoy-home'),
    dshHome: join(runRoot, 'decoy-home', '.dsh'),
    targetDir: join(runRoot, 'target'),
    binary: join(runRoot, 'target', 'debug', debugBinaryName()),
  };
  mkdirSync(paths.homeDir, { recursive: true });
  mkdirSync(paths.appDataDir, { recursive: true });
  mkdirSync(paths.evidenceDir, { recursive: true });
  mkdirSync(paths.decoyHome, { recursive: true });
  const marker = markerFor(runId, process.pid, repoRoot);
  writeFileSync(join(runRoot, MARKER_NAME), JSON.stringify(marker, null, 2) + '\n');
  const logPath = join(paths.evidenceDir, 'launcher.log');
  const log = (line) => {
    const text = '[' + new Date().toISOString() + '] ' + line;
    process.stdout.write(text + '\n');
    appendFileSync(logPath, text + '\n');
  };
  const writeResult = (result) => {
    writeFileSync(join(paths.evidenceDir, 'result.json'), JSON.stringify(result, null, 2) + '\n');
  };

  // decoy 家目录里的哨兵：跑完必须原样存在，证明没有回退到真实家目录。
  const sentinels = {
    '.dsh-xlink/sentinel.txt': 'sentinel-xlink-' + runId,
    '.dsh/sentinel.txt': 'sentinel-dsh-' + runId,
    '.config/dsh-xlink/sentinel.txt': 'sentinel-config-' + runId,
  };
  for (const [rel, content] of Object.entries(sentinels)) {
    const file = join(paths.decoyHome, rel);
    mkdirSync(dirname(file), { recursive: true });
    writeFileSync(file, content + '\n');
  }

  // debug 构建读 shell/dev/settings.json；端口若仍是 3090/3091，
  // status.kernel.running 会把宿主上真实内核的状态算进来。
  paths.port = await pickFreePort();
  const settingsFile = join(paths.homeDir, 'shell', 'dev', 'settings.json');
  mkdirSync(dirname(settingsFile), { recursive: true });
  writeFileSync(settingsFile, JSON.stringify({ port: paths.port }) + '\n');
  log('隔离内核端口：' + paths.port + '（settings: ' + settingsFile + '）');

  const problems = isolationProblems(runRoot, marker, runId, {
    homeDir: paths.homeDir,
    appDataDir: paths.appDataDir,
    evidenceDir: paths.evidenceDir,
    decoyHome: paths.decoyHome,
    dshHome: paths.dshHome,
    targetDir: paths.targetDir,
    binary: paths.binary,
  });
  if (process.env.HOME && resolve(process.env.HOME) === resolve(paths.homeDir)) {
    problems.push('运行环境 HOME 与测试 home 相同，无法证明隔离');
  }
  if (problems.length) {
    throw new Error('E2E 隔离自检失败：' + problems.join('；'));
  }

  log('E2E 运行根：' + runRoot);
  log('DSH_XLINK_HOME=' + paths.homeDir);
  log('appDirectoriesOverride=' + paths.appDataDir);
  log('DSH_XLINK_CHANNEL_SUFFIX=' + runId);

  try {
    const prepBuiltin = join(repoRoot, 'scripts', 'prepare-builtin-plugins.mjs');
    runChecked('prep:builtin', process.execPath, [prepBuiltin], { cwd: repoRoot }, log);
    const tauriCli = join(repoRoot, 'node_modules', '@tauri-apps', 'cli', 'tauri.js');
    if (!existsSync(tauriCli)) throw new Error('找不到 tauri CLI：' + tauriCli + '（先跑 npm run deps）');
    const buildEnv = {
      ...process.env,
      TAURI_CONFIG: mergeTauriConfig(process.env.TAURI_CONFIG, paths.appDataDir),
      CARGO_TARGET_DIR: paths.targetDir,
    };
    const cargoCheck = spawnSync('cargo', ['--version'], { env: buildEnv, stdio: 'ignore' });
    if (cargoCheck.error || cargoCheck.status !== 0) {
      throw new Error(
        '找不到 cargo（tauri build 需要）。请把 cargo 所在目录（通常是 $HOME/.cargo/bin）加入 PATH 后重试；本轮目录保留，可先清理'
      );
    }
    runChecked('tauri build --debug', process.execPath, [tauriCli, 'build', '--debug', '--no-bundle', '--features', 'wdio'], { cwd: repoRoot, env: buildEnv }, log);

    if (!existsSync(paths.binary)) throw new Error('debug 二进制不存在：' + paths.binary);
    const binarySize = statSync(paths.binary).size;
    if (binarySize <= 0) throw new Error('debug 二进制为空：' + paths.binary);
    const binarySha = sha256File(paths.binary);
    log('debug 二进制：' + paths.binary + '（sha256 ' + binarySha.slice(0, 16) + '…）');

    const runtimeEnv = buildRuntimeEnv(process.env, paths, binarySha);
    const wdioCli = join(repoRoot, 'node_modules', '@wdio', 'cli', 'bin', 'wdio.js');
    if (!existsSync(wdioCli)) throw new Error('找不到 wdio CLI：' + wdioCli + '（先跑 npm run deps）');
    const wdioLog = join(paths.evidenceDir, 'wdio.log');
    writeFileSync(wdioLog, '');
    log('启动 wdio：' + wdioCli + ' run e2e/wdio.conf.mjs');
    const child = spawn(process.execPath, [wdioCli, 'run', 'e2e/wdio.conf.mjs'], {
      cwd: repoRoot,
      env: runtimeEnv,
      stdio: ['inherit', 'pipe', 'pipe'],
      detached: process.platform !== 'win32',
    });
    const pipe = (stream) => {
      stream.on('data', (chunk) => {
        process.stdout.write(chunk);
        appendFileSync(wdioLog, chunk);
      });
    };
    pipe(child.stdout);
    pipe(child.stderr);
    let aborted = false;
    const stopChild = () => {
      aborted = true;
      if (child.exitCode !== null) return;
      try {
        if (process.platform === 'win32') {
          spawnSync('taskkill', ['/PID', String(child.pid), '/T', '/F'], { stdio: 'ignore' });
        } else {
          process.kill(-child.pid, 'SIGTERM');
        }
      } catch {
        // 进程可能已经退出；不再兜底强杀，避免误伤别的进程。
      }
    };
    process.on('SIGINT', stopChild);
    process.on('SIGTERM', stopChild);
    const exitCode = await new Promise((resolveExit) => {
      child.on('exit', (code, signal) => resolveExit(code ?? (signal ? 1 : 0)));
      child.on('error', (error) => {
        log('无法启动 wdio：' + error.message);
        resolveExit(1);
      });
    });
    process.removeListener('SIGINT', stopChild);
    process.removeListener('SIGTERM', stopChild);

    const post = [];
    if (binarySha !== sha256File(paths.binary)) post.push('二进制在运行期间被替换');
    if (!existsSync(paths.homeDir)) post.push('测试 home 不存在：' + paths.homeDir);
    if (!existsSync(paths.appDataDir)) post.push('浏览器档案目录不存在：' + paths.appDataDir);
    for (const [rel, content] of Object.entries(sentinels)) {
      const file = join(paths.decoyHome, rel);
      if (!existsSync(file)) {
        post.push('decoy 家目录哨兵丢失：' + rel);
      } else if (sha256Text(readFileSync(file, 'utf8')) !== sha256Text(content + '\n')) {
        post.push('decoy 家目录哨兵被改动：' + rel);
      }
    }
    const profileEvidence = [];
    for (const dir of [paths.appDataDir, join(paths.decoyHome, 'Library', 'WebKit')]) {
      let entries = 0;
      if (existsSync(dir)) {
        try {
          entries = readdirSync(dir).length;
        } catch {
          entries = 0;
        }
      }
      profileEvidence.push({ dir, exists: existsSync(dir), entries });
    }
    if (!profileEvidence.some((entry) => entry.entries > 0)) {
      post.push('浏览器档案没有落在本轮 appdata 或 decoy Library/WebKit');
    }
    const result = {
      schemaVersion: RUN_SCHEMA,
      browserProfileEvidence: profileEvidence,
      runId,
      exitCode,
      aborted,
      binary: paths.binary,
      binarySha256: binarySha,
      isolatedHome: paths.homeDir,
      appDataDir: paths.appDataDir,
      channelSuffix: runId,
      postChecks: post,
      finishedAt: new Date().toISOString(),
    };
    writeResult(result);
    if (post.length) {
      log('运行后隔离核对失败：' + post.join('；'));
    }
    if (exitCode !== 0 || post.length || aborted) {
      log('E2E 失败，保留本轮目录：' + runRoot);
      process.exitCode = exitCode !== 0 ? exitCode : 1;
      return;
    }
    log('E2E 通过；本轮目录可清理：' + runRoot);
    if (!keep) {
      cleanup(runRoot, marker, log);
    } else {
      log('按要求保留本轮目录（--keep）：' + runRoot);
    }
  } catch (error) {
    log('E2E 启动失败：' + (error && error.message ? error.message : String(error)));
    writeResult({ schemaVersion: RUN_SCHEMA, runId, error: String(error), failedAt: new Date().toISOString() });
    log('失败证据保留在：' + paths.evidenceDir);
    process.exitCode = 1;
  }
}

export function cleanup(runRoot, marker, log) {
  // 只删带本轮标记的目录：脚本被误指向别的路径时不越界。
  const markerPath = join(runRoot, MARKER_NAME);
  if (!existsSync(markerPath)) throw new Error('拒绝清理无归属标记的目录：' + runRoot);
  const actual = JSON.parse(readFileSync(markerPath, 'utf8'));
  if (actual.schemaVersion !== RUN_SCHEMA || actual.runId !== marker.runId) {
    throw new Error('拒绝清理归属不符的目录：' + runRoot);
  }
  rmSync(runRoot, { recursive: true, force: true });
  if (log) log('已清理本轮目录：' + runRoot);
}

if (import.meta.url === pathToFileURL(process.argv[1] || '').href) {
  main().catch((error) => {
    console.error('E2E 未处理异常：' + (error && error.stack ? error.stack : error));
    process.exitCode = 1;
  });
}
