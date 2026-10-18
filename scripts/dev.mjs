// `pnpm run dev` 的启动包装：把「用哪个端口」收敛到一处。
//
// vite 的 `server.port` 与 tauri 的 `build.devUrl` 在两个文件里各写了一份
// 5173，两者必须一致——不一致的后果比「端口冲突」难查得多：vite 在
// `strictPort` 下不会自动顺延（顺延了 tauri 的 devUrl 就指向一个没有本项目
// 的页面），而 tauri 侧读不到环境变量，`tauri.conf.json` 又是静态 JSON。
// 所以 devUrl 只能在这里运行时用 `-c` 合并覆盖，vite 那边则通过
// `DSH_DEV_PORT` 继承同一个值。
//
//   pnpm run dev                  → 5174
//   pnpm run dev 5190             → 5190
//   DSH_DEV_PORT=5190 pnpm run dev → 5190
//
// 默认端口从 5173 挪到 5174：5173 是 vite 的首选项，本机常年被别的工程的
// vite 占着（`strictPort` 下 vite 不会顺延，每次都要手动传参）。`dev.mjs` /
// `vite.config.mjs` / `tauri.conf.json` 三处的数字必须一致——不一致时 tauri
// 会去连一个并不提供本项目的页面，症状比冲突本身难查；`dev-port.test.mjs`
// 把这条钉死。
//
// 端口被占用时不要自动顺延、不要自动 kill：占用者多半是同机的另一个工程
//（实测就撞上过 D:\...\frontend 的 vite），该由用户决定停哪一个。
//
// 直接 spawn node + CLI 的 js 入口，而不是 `pnpm exec tauri`：覆盖配置是一段
// JSON 字符串，经 shell（Windows 上是 cmd）转发时双引号会被吃掉，实测 tauri
// 收到的是 `{build:{devUrl:http://…}}` 并报「key must be a string」。不走 shell
// 才能把 JSON 原样送到 argv。
import { spawn, spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const DEFAULT_PORT = 5174;
const repoRoot = dirname(dirname(fileURLToPath(import.meta.url)));

// tauri.conf.json 把 resources/builtin-plugins 登记进了 bundle.resources
//（openai-oauth P1 资源管线），tauri 的 build script 会先检查它存在与否；
// 产物不进 git，`npm run build*` 都先跑 prep:builtin 再起 tauri，dev 这条
// 链此前漏了同一前置——新 clone（或清掉产物）上 `pnpm run dev` 直接死在
// 「resource path `resources\builtin-plugins` doesn't exist」。与 build 走
// 同一条生成命令：产物就 9 个文件、全量重建幂等，顺带保证 dev 每次拿到
// 的内嵌插件与 plugins/ 源码同样新鲜（build 的语义，dev 不该更旧）。
const prepareBuiltin = join(repoRoot, 'scripts', 'prepare-builtin-plugins.mjs');
const prep = spawnSync(process.execPath, [prepareBuiltin], { stdio: 'inherit', cwd: repoRoot });
if (prep.error) {
  console.error(`无法执行内嵌插件资源生成（${prepareBuiltin}）：${prep.error.message}`);
  process.exit(1);
}
if (prep.status !== 0) {
  console.error(`内嵌插件资源生成失败（exit ${prep.status ?? `signal ${prep.signal}`}）；tauri 的资源检查过不去，dev 中止`);
  process.exit(prep.status ?? 1);
}

function resolvePort() {
  // 过滤掉透传给 runner 的参数（`-c` / `--no-watch` 之类以 - 开头的东西）。
  const positional = process.argv.slice(2).filter((arg) => !arg.startsWith('-'));
  const raw = positional[0] || process.env.DSH_DEV_PORT || DEFAULT_PORT;
  const port = Number(raw);
  if (!Number.isInteger(port) || port < 1024 || port > 65535) {
    console.error(`端口形态非法：${raw}。用法：pnpm run dev 5190（1024–65535 的整数）`);
    process.exit(1);
  }
  return port;
}

const tauriCli = join(repoRoot, 'node_modules', '@tauri-apps', 'cli', 'tauri.js');
if (!existsSync(tauriCli)) {
  console.error(`找不到 tauri CLI 入口（${tauriCli}），请先跑 npm run deps`);
  process.exit(1);
}

const port = resolvePort();
const override = JSON.stringify({
  build: {
    devUrl: `http://localhost:${port}`,
    beforeDevCommand: `npm run dev:ui -- --port ${port}`,
  },
});

// Windows + windows-gnu 工具链下，`gcc` / `dlltool` / `windres` 来自 MinGW，
// cargo 只从 PATH 里找它们。而 PATH 是**进程启动时的快照**：在 MinGW 写进 PATH
// （用户/安装器改注册表）之前打开的终端、以及复用旧环境的 IDE/终端宿主新开的
// 标签页，跑 `npm run dev` 会死在 `error calling dlltool` 或 tauri-winres 的
// `NotAttempted("windres")` panic——即使 `where.exe windres` 在新窗口里能通过。
// 这里在 spawn 前给**本次 dev 的子进程**兜底，不改用户环境：PATH 里已有 windres
// 就不动；否则从 DSH_MINGW_BIN 或常见安装位置补一条。
function ensureMingwOnPath(env) {
  if (process.platform !== 'win32') {
    return env;
  }
  const pathKey = Object.keys(env).find((name) => name.toUpperCase() === 'PATH') ?? 'Path';
  const current = env[pathKey] ?? '';
  if (current.split(';').some((dir) => dir && existsSync(join(dir, 'windres.exe')))) {
    return env;
  }
  const candidates = [
    env.DSH_MINGW_BIN,
    join(env.USERPROFILE ?? '', 'tools', 'winlibs', 'mingw64', 'bin'),
    'C:\\winlibs\\mingw64\\bin',
    'C:\\msys64\\mingw64\\bin',
  ].filter(Boolean);
  const found = candidates.find((dir) => existsSync(join(dir, 'windres.exe')));
  if (!found) {
    return env;
  }
  console.error(`dev: PATH 里没有 windres（Windows-gnu 工具链需要），本次 dev 临时加入 ${found}`);
  return { ...env, [pathKey]: `${found};${current}` };
}

// `--features wdio` 把内嵌 WebDriver 服务端编进 dev 壳。这是 release 与 dev
// 唯一的构建差异，判据见 scripts/check-webdriver-gating.mjs：release 的
// `default` 是空的，插件不参与编译。
const child = spawn(process.execPath, [tauriCli, 'dev', '-c', override, '--features', 'wdio'], {
  stdio: 'inherit',
  cwd: repoRoot,
  env: ensureMingwOnPath({ ...process.env, DSH_DEV_PORT: String(port) }),
});

for (const signal of ['SIGINT', 'SIGTERM']) {
  process.on(signal, () => child.kill(signal));
}
child.on('exit', (code, signal) => process.exit(signal ? 1 : (code ?? 0)));
child.on('error', (error) => {
  console.error(`无法启动 tauri dev：${error.message}`);
  process.exit(1);
});
