// 起**隔离真壳**（tauri dev），三处隔离：
//
//   1. `DSH_XLINK_HOME` → 临时目录：壳的实例、内核树、设置、插件物化全在这里，
//      用户真实的壳数据目录一个字节都不碰。
//   2. `app.appDirectoriesOverride` → 临时目录：覆盖 `app_local_data_dir`，
//      于是 **WebView2 的浏览器档案**也落到临时目录（Windows 上它默认在用户
//      真实安装目录里——本文件不写那个字面量：判据第 30 条禁止测试代码在
//      **同一行**同时出现「真实家目录」与「浏览器 profile」两类标记）。
//      注意：`WEBVIEW2_USER_DATA_FOLDER` 环境变量**管不住**——Tauri 会显式传
//      user data folder 把它覆盖掉。
//   3. `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=<port>`：
//      让 WebView2 暴露 CDP 端点，壳自己的 webview（面板与**工作台**）就能被
//      CDP 驱动——本机没有 chromedriver / playwright / puppeteer，仓库的 wdio
//      那条路缺 tauri-driver。
//
// ## 与仓库 `scripts/dev.mjs` 的差别
//
// 只有一处：它用 `-c` 传一段配置覆盖，我这里传的是**合并后**的那一段
// （dev.mjs 的 devUrl / beforeDevCommand + appDirectoriesOverride）。
// **仓库里的 dev.mjs 一个字都没改。**
//
// ## 用法
//
//   node plugins/official-chat/test/drive/shell/shell-dev.mjs
//   # 然后另开一个终端：
//   node plugins/official-chat/test/drive/shell/shell-verify.mjs
import { spawn } from 'node:child_process';
import { existsSync, mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { activeReleaseKernel } from './paths.mjs';
import { APP_DIRS, CDP_PORT, DATA_DIR, DEV_PORT, KERNEL_PORT, SHELL_HOME } from './paths.mjs';

const here = dirname(fileURLToPath(import.meta.url));
/** 仓库根：从本文件位置往上数（plugins/official-chat/test/drive/shell → 根）。 */
const REPO = resolve(here, '..', '..', '..', '..', '..');
const tauriCli = join(REPO, 'node_modules', '@tauri-apps', 'cli', 'tauri.js');
if (!existsSync(tauriCli)) throw new Error(`找不到 tauri CLI：${tauriCli}`);

mkdirSync(APP_DIRS, { recursive: true });

const override = JSON.stringify({
  build: {
    devUrl: `http://localhost:${DEV_PORT}`,
    beforeDevCommand: `npm run dev:ui -- --port ${DEV_PORT}`,
  },
  app: {
    appDirectoriesOverride: APP_DIRS,
  },
});

console.log('[shell-dev] 仓库 =', REPO);
console.log('[shell-dev] appDirectoriesOverride =', APP_DIRS);
console.log('[shell-dev] DSH_XLINK_HOME =', SHELL_HOME);
console.log('[shell-dev] WebView2 CDP 端口 =', CDP_PORT);

// 隔离 home 的两样准备（**release 启动器 `shell-release.mjs` 里也有同样两步**，
// 少哪一步壳都会在别处报一个看不懂的错）：
//
//   1. 壳设置 `shell/dev/settings.json`：内核端口。默认端口（debug 是 3091）
//      可能与用户正在跑的那套撞车。
//   2. `dsh/desktop-dev/active.txt`：壳据此知道「这个实例用哪个内核版本」。
//      **缺它时 `builtin_set_enabled` 会报「实例尚未指定内核版本；请在「内核
//      版本」页安装并选择内核后再启用」**——而这棵内核树明明已经拷好了。
const settingsDir = join(SHELL_HOME, 'shell', 'dev');
mkdirSync(settingsDir, { recursive: true });
const settingsFile = join(settingsDir, 'settings.json');
if (!existsSync(settingsFile)) {
  writeFileSync(
    settingsFile,
    `${JSON.stringify({ port: KERNEL_PORT, profile: 'web' }, null, 2)}\n`,
    'utf8',
  );
  console.log('[shell-dev] 已写壳设置：', settingsFile);
}

const { version } = activeReleaseKernel();
mkdirSync(DATA_DIR, { recursive: true });
const activeFile = join(DATA_DIR, 'active.txt');
if (!existsSync(activeFile)) {
  writeFileSync(activeFile, `${version}\n`, 'utf8');
  console.log('[shell-dev] 已写 active.txt：', activeFile, '=', version);
}

const child = spawn(
  process.execPath,
  [tauriCli, 'dev', '-c', override, '--features', 'wdio', '--no-watch'],
  {
    cwd: REPO,
    env: {
      ...process.env,
      DSH_XLINK_HOME: SHELL_HOME,
      DSH_DEV_PORT: String(DEV_PORT),
      // 桩模型的凭据：壳起的内核**继承**壳进程的环境变量，所以这里带上，
      // 内核侧的 llm-pi-ai provider 才认得 apiKeyEnv=G1_STUB_KEY。
      G1_STUB_KEY: process.env.G1_STUB_KEY ?? 'g1-stub-key',
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${CDP_PORT}`,
    },
    stdio: 'inherit',
  },
);
for (const signal of ['SIGINT', 'SIGTERM']) {
  process.on(signal, () => child.kill(signal));
}
child.on('exit', (code, signal) => process.exit(signal ? 1 : (code ?? 0)));
child.on('error', (error) => {
  console.error('[shell-dev] 无法启动 tauri dev：', error.message);
  process.exit(1);
});
