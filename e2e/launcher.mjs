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
// 运行时环境同样按白名单重建（本文件 INHERITED_ENV，2026-11 FIX-06）：用户
// shell 里的目录覆盖变量（DSH_DESKTOP_DATA_DIR / CODEX_HOME / …）不能跟着
// 继承，否则上面四项会被绕过；HOME/TEMP/APPDATA 一类目录变量重定向到 runRoot。
// wdio 启动后、用例执行前，`e2e/isolation-probe.mjs` 再从运行中的后端读一次
// 实际 data_dir（disk_usage 报表里的绝对路径），canonicalize 后必须仍在 runRoot 内。
//
// 失败时保留本轮目录并把路径打出来；成功时只删除带本轮归属标记的目录，且
// 删之前先关闭 launcher.log（它就在 runRoot 里），删完不再写任何文件（FIX-07）。

import { createHash, randomBytes } from 'node:crypto';
import { spawn, spawnSync } from 'node:child_process';
import {
  appendFileSync,
  closeSync,
  existsSync,
  mkdirSync,
  openSync,
  readdirSync,
  readFileSync,
  realpathSync,
  rmSync,
  statSync,
  writeFileSync,
  writeSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { createServer } from 'node:net';
import { basename, dirname, isAbsolute, join, relative, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, '..');
const RUN_SCHEMA = 1;
const MARKER_NAME = '.dsh-e2e-run.json';

export function isMcodeInstanceId(value) {
  return typeof value === 'string' && /^mcode-(?:[0-9a-f]{32}|[A-Za-z0-9_-]{22})$/.test(value);
}

// Explicit arguments only: never inherit external package paths through the environment.
export function parseRunOptions(args, root = repoRoot) {
  const result = { keep: false, spec: null, mcodePackage: null, mcodeNode: null };
  const flags = { '--spec': 'spec', '--mcode-package': 'mcodePackage', '--mcode-node': 'mcodeNode' };
  for (let i = 0; i < args.length; i += 1) {
    const arg = args[i];
    if (arg === '--keep' && !result.keep) { result.keep = true; continue; }
    const key = flags[arg];
    if (!key || result[key] !== null || !args[i + 1] || args[i + 1].startsWith('--')) {
      throw new Error('E2E 参数无效：' + arg);
    }
    result[key] = args[++i];
  }
  if (!!result.mcodePackage !== !!result.mcodeNode) throw new Error('MiniMax 包和 Node 路径必须同时指定');
  if (result.spec) {
    const allowed = realpathSync(join(root, 'e2e'));
    const spec = realpathSync(resolve(root, result.spec));
    const rel = relative(allowed, spec);
    if (!rel || rel.startsWith('..') || isAbsolute(rel) || !spec.endsWith('.spec.mjs') || !statSync(spec).isFile()) {
      throw new Error('E2E spec 必须是 e2e 目录内的 .spec.mjs 文件');
    }
    result.spec = spec;
  }
  if (result.mcodePackage) {
    if (!result.spec || !isAbsolute(result.mcodePackage) || !isAbsolute(result.mcodeNode)) {
      throw new Error('MiniMax smoke 需要指定 spec 与绝对路径');
    }
    result.mcodePackage = realpathSync(result.mcodePackage);
    result.mcodeNode = realpathSync(result.mcodeNode);
    if (!statSync(result.mcodePackage).isDirectory() || !statSync(result.mcodeNode).isFile()) {
      throw new Error('MiniMax 包或 Node 路径无效');
    }
    const metadata = JSON.parse(readFileSync(join(result.mcodePackage, 'package.json'), 'utf8'));
    if (metadata.name !== '@minimax-webui/minimax-code' || metadata.version !== '0.1.7') {
      throw new Error('MiniMax smoke 只接受 @minimax-webui/minimax-code 0.1.7');
    }
  }
  return result;
}

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

// 允许继承到 wdio 子进程的环境变量白名单（2026-11 FIX-06）。
//
// 只放运行测试所必需的 OS 级变量：进程创建 / 动态库与命令解析、终端编码、
// 用户标识（后者只用于日志，不参与路径解析）。
//
// **不能改成 `{ ...process.env }`**：壳的数据目录解析链里有一串覆盖量，用户
// 终端里随手设置一个就能让测试壳读写真数据——`DSH_DESKTOP_DATA_DIR`（完整
// 覆盖 `kernel::data_dir`）、`DSH_HOME`（内核实例 home）、`DSH_XLINK_HOME`、
// `CODEX_HOME`（外部用量来源）、`DSH_NPM_REGISTRY` / `DSH_NPM_TARBALL_HOSTS`
// （插件取源）、`*_PROXY` 与 `NODE_OPTIONS`。目录类变量（HOME/TEMP/APPDATA…）
// 不继承，由 `buildRuntimeEnv` 显式重定向到本轮 runRoot；其余一律不传。
const INHERITED_ENV = [
  'PATH',
  'PATHEXT',
  'SYSTEMROOT',
  'WINDIR',
  'SYSTEMDRIVE',
  'COMSPEC',
  'NUMBER_OF_PROCESSORS',
  'PROCESSOR_ARCHITECTURE',
  'PROCESSOR_IDENTIFIER',
  'OS',
  'LANG',
  'LC_ALL',
  'LC_CTYPE',
  'LC_MESSAGES',
  'TERM',
  'TERM_PROGRAM',
  'DISPLAY',
  '__CF_USER_TEXT_ENCODING',
  // macOS GUI 进程通常从 launchd 拿到这几个；不是路径覆盖，实测环境里保留更安全。
  'XPC_FLAGS',
  'XPC_SERVICE_NAME',
  'SECURITYSESSIONID',
  '__CFBundleIdentifier',
  'TMPDIR',
  'USER',
  'USERNAME',
  'USERDOMAIN',
  'COMPUTERNAME',
  'CI',
];

// Windows 的环境变量名大小写不敏感，`Path` 与 `PATH` 是同一个变量。
function envKey(name) {
  return process.platform === 'win32' ? name.toUpperCase() : name;
}

const INHERITED_ENV_KEYS = new Set(INHERITED_ENV.map(envKey));

export function buildRuntimeEnv(base, paths, binarySha) {
  const env = {};
  for (const [name, value] of Object.entries(base || {})) {
    if (value !== undefined && INHERITED_ENV_KEYS.has(envKey(name))) env[name] = value;
  }
  Object.assign(env, {
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
    // 把用户家目录指到本轮 decoy：任何路径回退都落在测试根里，真实家目录不被读。
    HOME: paths.decoyHome,
    USERPROFILE: paths.decoyHome,
    // 临时 / 漫游 / 本地目录也全部重定向：测试壳和它的子进程写什么都落在 runRoot，
    // 清理时随本轮目录一起消失，不会污染用户真实目录。
    TEMP: paths.tempDir,
    TMP: paths.tempDir,
    // macOS 的 TMPDIR 保留系统值（WKWebView / CoreFoundation 依赖它的格式），
    // 只把它放进白名单；Windows 上这个变量没人读，重定向到 runRoot 即可。
    APPDATA: paths.roamingDir,
    LOCALAPPDATA: paths.localDir,
  });
  if (paths.mcodePackage && paths.mcodeNode) {
    env.DSH_E2E_MCODE_PACKAGE = paths.mcodePackage;
    env.DSH_E2E_MCODE_NODE = paths.mcodeNode;
  }
  if (process.platform !== 'darwin') env.TMPDIR = paths.tempDir;
  // macOS 的 CoreFoundation / WKWebView 走 NSHomeDirectory，不认普通 HOME；
  // CFFIXED_USER_HOME 是让它们也落到 decoy 的已知入口。
  if (process.platform === 'darwin') {
    env.CFFIXED_USER_HOME = paths.decoyHome;
  }
  return env;
}

// 从运行中的后端读到的 data_dir / xlink_home 必须 canonicalize 后都落在本轮
// runRoot 内（2026-11 FIX-06）。canonicalize 不能省：符号链接、Windows 8.3
// 短名、大小写都能让字面前缀检查骗人；canonicalize 失败 = 无法证明隔离，
// 按逃逸处理（宁可失败，不许带着不确定跑测试）。
//
// 允许路径**尚不存在**（E2E 隔离 home 里 data_dir 可能还没落盘）：对不存在的
// 尾巴取最近一个存在的祖先做 realpath 再拼回去——这就是 `realpath -m` 的语义，
// 不存在的目录同样要能判断归属。
function canonicalizeMissing(target) {
  let current = resolve(target);
  const tail = [];
  for (;;) {
    try {
      return tail.length ? join(realpathSync(current), ...tail) : realpathSync(current);
    } catch (error) {
      const parent = dirname(current);
      if (parent === current) throw error;
      tail.unshift(basename(current));
      current = parent;
    }
  }
}

export function backendIsolationProblems(runRoot, actualPaths) {
  const problems = [];
  let realRoot;
  try {
    realRoot = realpathSync(runRoot);
  } catch (error) {
    return ['本轮测试根无法 canonicalize：' + runRoot + '（' + error.message + '）'];
  }
  for (const [name, value] of Object.entries(actualPaths)) {
    if (typeof value !== 'string' || !isAbsolute(value)) {
      problems.push(name + ' 不是绝对路径：' + value);
      continue;
    }
    let real;
    try {
      real = canonicalizeMissing(value);
    } catch (error) {
      problems.push(name + ' 无法 canonicalize：' + value + '（' + error.message + '）');
      continue;
    }
    if (real === realRoot || !inside(realRoot, real)) {
      problems.push(name + ' 逃出本轮测试根：后端=' + real + '，runRoot=' + realRoot);
    }
  }
  return problems;
}

// 后端 `disk_usage` 报表里所有绝对路径：`unreadable` 是扫不到的目录（不存在 /
// 无权限，E2E 里 data_dir/kernels 就在这一栏），条目的 `path` 是存在的目录。
// 这两处都是后端用**自己的** AppState.data_dir / xlink_home 拼出来的真实路径。
export function diskUsageReportPaths(report) {
  const paths = [];
  const push = (value) => {
    if (typeof value === 'string' && isAbsolute(value)) paths.push(value);
  };
  for (const value of report?.unreadable || []) push(value);
  for (const group of report?.groups || []) {
    for (const entry of group?.entries || []) push(entry?.path);
  }
  return paths;
}

// 从报表路径里认出**数据目录**：E2E 隔离 home 从没装过内核，`<data_dir>/kernels`
// 必然不存在 → `disk_usage` 把它列进 `unreadable`。形态是
// `<DSH_XLINK_HOME>/<family>/desktop[-dev]/kernels`。
// 返回 kernels 路径（没有则返回 null：不允许在「没观察到数据目录」时假装通过）。
export function dataDirKernelsPath(expectedHome, reportPaths) {
  const home = resolve(expectedHome);
  for (const candidate of reportPaths) {
    const parts = relative(home, resolve(candidate)).split(/[\\/]+/);
    if (parts.length === 3 && /^desktop(-dev)?$/.test(parts[1]) && parts[2] === 'kernels') {
      return resolve(candidate);
    }
  }
  return null;
}

// 同步文件日志。成功路径必须在删除 runRoot 之前 close：日志文件就在
// runRoot/evidence 里，不先关闭就去删，之后再写就是 ENOENT（2026-11 FIX-07）。
// close 之后只写 stdout——异常处理自己也可能发生在目录已删之后，不能再碰路径。
export function createFileLogger(file) {
  const fd = openSync(file, 'a');
  let closed = false;
  const log = (line) => {
    const text = '[' + new Date().toISOString() + '] ' + line;
    process.stdout.write(text + '\n');
    if (!closed) writeSync(fd, text + '\n');
  };
  return {
    log,
    close() {
      if (closed) return;
      closed = true;
      closeSync(fd);
    },
    get closed() {
      return closed;
    },
  };
}

// 结果证据落盘。成功清理之后 evidence 目录已经不存在，异常路径不能反过来
// 依赖它（2026-11 FIX-07）：写失败只落 stderr，不能把真实错误盖成 ENOENT。
export function writeEvidenceResult(evidenceDir, result) {
  try {
    writeFileSync(join(evidenceDir, 'result.json'), JSON.stringify(result, null, 2) + '\n');
  } catch (error) {
    process.stderr.write(
      '写结果证据失败（' + error.message + '）；原始结果：' + JSON.stringify(result) + '\n'
    );
  }
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
  const options = parseRunOptions(process.argv.slice(2));
  // Real-service evidence must survive even a successful smoke run.
  const keep = options.keep || !!options.mcodePackage;
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
    tempDir: join(runRoot, 'temp'),
    roamingDir: join(runRoot, 'appdata-roaming'),
    localDir: join(runRoot, 'appdata-local'),
    targetDir: join(runRoot, 'target'),
    binary: join(runRoot, 'target', 'debug', debugBinaryName()),
    mcodePackage: options.mcodePackage,
    mcodeNode: options.mcodeNode,
  };
  mkdirSync(paths.homeDir, { recursive: true });
  mkdirSync(paths.appDataDir, { recursive: true });
  mkdirSync(paths.evidenceDir, { recursive: true });
  mkdirSync(paths.decoyHome, { recursive: true });
  mkdirSync(paths.tempDir, { recursive: true });
  mkdirSync(paths.roamingDir, { recursive: true });
  mkdirSync(paths.localDir, { recursive: true });
  const marker = markerFor(runId, process.pid, repoRoot);
  writeFileSync(join(runRoot, MARKER_NAME), JSON.stringify(marker, null, 2) + '\n');
  const logger = createFileLogger(join(paths.evidenceDir, 'launcher.log'));
  const log = (line) => logger.log(line);
  const writeResult = (result) => writeEvidenceResult(paths.evidenceDir, result);

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
    tempDir: paths.tempDir,
    roamingDir: paths.roamingDir,
    localDir: paths.localDir,
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
    if (options.mcodePackage) buildEnv.VITE_MCODE_EXPERIMENTAL = '1';
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
    // 用户 shell 里预设的目录覆盖量必须被白名单处理掉：DSH_DESKTOP_DATA_DIR /
    // CODEX_HOME / NODE_OPTIONS 丢弃，HOME 类变量重定向到 runRoot（FIX-06）。
    // 把处理结果写进证据，隔离失败时一眼看得出是哪个变量被带进来的。
    const sanitizedEnv = [];
    if (process.env.DSH_DESKTOP_DATA_DIR) sanitizedEnv.push('DSH_DESKTOP_DATA_DIR（丢弃）');
    if (process.env.CODEX_HOME) sanitizedEnv.push('CODEX_HOME（丢弃）');
    if (process.env.NODE_OPTIONS) sanitizedEnv.push('NODE_OPTIONS（丢弃）');
    if (process.env.DSH_XLINK_HOME) sanitizedEnv.push('DSH_XLINK_HOME（重定向到 runRoot）');
    if (process.env.DSH_HOME) sanitizedEnv.push('DSH_HOME（重定向到 runRoot）');
    if (sanitizedEnv.length) log('已清理会逃出 runRoot 的覆盖变量：' + sanitizedEnv.join('、'));
    const wdioCli = join(repoRoot, 'node_modules', '@wdio', 'cli', 'bin', 'wdio.js');
    if (!existsSync(wdioCli)) throw new Error('找不到 wdio CLI：' + wdioCli + '（先跑 npm run deps）');
    const wdioLog = join(paths.evidenceDir, 'wdio.log');
    writeFileSync(wdioLog, '');
    log('启动 wdio：' + wdioCli + ' run e2e/wdio.conf.mjs');
    const wdioArgs = [wdioCli, 'run', 'e2e/wdio.conf.mjs'];
    if (options.spec) wdioArgs.push('--spec', options.spec);
    const child = spawn(process.execPath, wdioArgs, {
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
      // 等 `close` 而不是 `exit`：`exit` 只表示进程退出，stdio 可能还有一拍没读完；
      // 成功路径随后就会删掉 runRoot（wdio.log 在里面），迟到的一拍再 append 就是
      // ENOENT（FIX-07）。
      child.on('close', (code, signal) => resolveExit(code ?? (signal ? 1 : 0)));
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
    process.exitCode = completeRun({ runRoot, marker, keep, logger });
  } catch (error) {
    log('E2E 启动失败：' + (error && error.message ? error.message : String(error)));
    writeResult({ schemaVersion: RUN_SCHEMA, runId, error: String(error), failedAt: new Date().toISOString() });
    // 异常可能发生在成功清理之后（例如 cleanup 归属校验失败）：这时证据目录
    // 已经不在了，别报一句「证据保留在」骗人（FIX-07）。
    if (existsSync(paths.evidenceDir)) {
      log('失败证据保留在：' + paths.evidenceDir);
    } else {
      process.stderr.write('失败证据目录已被清理：' + paths.evidenceDir + '\n');
    }
    logger.close();
    process.exitCode = 1;
  }
}

// 成功收尾（2026-11 FIX-07）。顺序是判据的一部分：launcher.log 就在
// runRoot/evidence 里，必须先 close（flush）再删 runRoot；删除后不得再调用
// logger——旧实现在 rmSync 之后又写一行「已清理」，成功路径以 ENOENT 收场、
// 退出码变 1、证据还被一起删掉。返回进程退出码，单测直接钉「无 --keep = 0」。
export function completeRun({ runRoot, marker, keep, logger }) {
  logger.log('E2E 通过；本轮目录可清理：' + runRoot);
  if (keep) {
    logger.log('按要求保留本轮目录（--keep）：' + runRoot);
    logger.close();
    return 0;
  }
  logger.close();
  cleanup(runRoot, marker);
  process.stdout.write('已清理本轮目录：' + runRoot + '\n');
  return 0;
}

export function cleanup(runRoot, marker) {
  // 只删带本轮标记的目录：脚本被误指向别的路径时不越界。外部目录（例如用户
  // 预设的 DSH_DESKTOP_DATA_DIR）永远不在删除目标里——逃逸由
  // `backendIsolationProblems` 在执行测试前拦下。
  const markerPath = join(runRoot, MARKER_NAME);
  if (!existsSync(markerPath)) throw new Error('拒绝清理无归属标记的目录：' + runRoot);
  const actual = JSON.parse(readFileSync(markerPath, 'utf8'));
  if (actual.schemaVersion !== RUN_SCHEMA || actual.runId !== marker.runId) {
    throw new Error('拒绝清理归属不符的目录：' + runRoot);
  }
  rmSync(runRoot, { recursive: true, force: true });
}

if (import.meta.url === pathToFileURL(process.argv[1] || '').href) {
  main().catch((error) => {
    console.error('E2E 未处理异常：' + (error && error.stack ? error.stack : error));
    process.exitCode = 1;
  });
}
