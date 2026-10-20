// 起**隔离的 release 壳**（自编 release 二进制），用来验「release 壳自己的
// 工作台窗口」——dev 壳验过的那条链路，在 release 壳里必须同样成立。
//
// ## 为什么 release 壳要单独一套
//
// release 与 dev 的差别是**构建产物**层面的（壳源码 `ShellMode::current()` 由
// `cfg!(debug_assertions)` 决定），连带三件事：
//
//   1. 运行时子目录 `desktop`（dev 是 `desktop-dev`）；
//   2. 默认实例 id `default`（dev 是 `default-dev`）；
//   3. 面板走**内嵌资源**（`tauri://localhost`），不是 vite dev server。
//
// 依据：`src-tauri/src/shell/paths.rs:96-102`（`runtime_subdir`）、
// `src-tauri/src/shell/instance.rs:95-106`（`default_instance_id_for`）。
//
// ## 三处隔离（+ 一条通道隔离）
//
//   1. `DSH_XLINK_HOME` → 临时目录：壳的实例、内核树、设置、插件物化都在这里，
//      用户真实的壳数据目录一个字节都不碰；
//   2. `app.appDirectoriesOverride` → 临时目录：**编译期**烘焙进二进制的配置，
//      覆盖 `app_local_data_dir`，于是 **WebView2 的浏览器档案**也落在这个临时
//      目录里（Windows 上它默认在用户真实安装目录下）。**这一步只能编译期做**：
//      `dirs::data_local_dir()` 在 Windows 上走 `SHGetKnownFolderPath`，读
//      `LOCALAPPDATA` 环境变量**没用**（`dirs-7.0.0/src/win.rs:11` →
//      `dirs-sys-0.5.0` 的 `known_folder_local_app_data`）。Tauri 支持用
//      `TAURI_CONFIG` 环境变量把配置合并进 `tauri.conf.json`
//      （`tauri-build-2.7.2/src/lib.rs:682-685`），本脚本用的就是它；
//   3. WebView2 的 CDP 端口：让壳自己的 webview 能被驱动；
//   4. `DSH_XLINK_CHANNEL_SUFFIX`：让本实例走**自己的**单实例通道。
//
// ## 第 4 条为什么必须有（2026-10-20 实测）
//
// 单实例守卫按**通道名**判「是不是第一个」，而通道名只按壳模式分
// （`notify/activate.rs` 的 `channel_name()`）。用户机器上正跑着安装版 release 壳
// 时，它持有 `Local\dsh-xlink-shell-release`；本实例是**另一个** release 二进制、
// 另一套 data dir、内核端口、浏览器档案，却会因为同名通道被静默「交接后退出」
// ——用户看不到任何提示，只看到第二个壳没起来。加了后缀，两个实例各走各的通道。
//
// ## 用法
//
//   node plugins/official-chat/test/drive/shell/shell-release.mjs
//   # 然后另开一个终端：
//   $env:G1_SHELL_MODE = 'release'
//   node plugins/official-chat/test/drive/shell/shell-verify.mjs
import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { activeReleaseKernel } from './paths.mjs';
import { APP_DIRS, CDP_PORT, DATA_DIR, KERNEL_PORT, SHELL_HOME } from './paths.mjs';

const here = dirname(fileURLToPath(import.meta.url));
/** 仓库根：从本文件位置往上数（plugins/official-chat/test/drive/shell → 根）。 */
const REPO = resolve(here, '..', '..', '..', '..', '..');
const CARGO_MANIFEST = join(REPO, 'src-tauri', 'Cargo.toml');
const RELEASE_EXE = join(REPO, 'src-tauri', 'target', 'release', 'dsh-xlink.exe');

/** 隔离实例的单实例通道后缀（与用户正在跑的安装版各走各的通道）。 */
const CHANNEL_SUFFIX = process.env.G1_CHANNEL_SUFFIX ?? 'g1release';

mkdirSync(APP_DIRS, { recursive: true });

/**
 * 编译期配置覆盖：把 `app_local_data_dir` 指到临时目录。
 *
 * 形状依据：Tauri v2 的 `app.appDirectoriesOverride` 收一个字符串时表示
 * 「一个根目录管全部 app 目录」（`tauri-utils-2.10.1/src/config.rs:3222-3238`
 * 的 `AppDirectoriesOverride::Root`），cache/log 各落到 `<根>/caches`、
 * `<根>/logs`；解析成「相对二进制目录」还是「绝对路径」由
 * `tauri-2.12.3/src/path/mod.rs` 的 `resolve_override_path` 决定，**绝对路径
 * 原样使用**。
 */
const tauriConfig = JSON.stringify({ app: { appDirectoriesOverride: APP_DIRS } });

console.log('[shell-release] 仓库 =', REPO);
console.log('[shell-release] DSH_XLINK_HOME =', SHELL_HOME);
console.log('[shell-release] data_dir =', DATA_DIR);
console.log('[shell-release] appDirectoriesOverride =', APP_DIRS);
console.log('[shell-release] WebView2 CDP 端口 =', CDP_PORT);
console.log('[shell-release] 内核端口 =', KERNEL_PORT);
console.log('[shell-release] 单实例通道后缀 =', CHANNEL_SUFFIX);

// ① 编 release 二进制（带上面的配置覆盖）。cargo 增量：源码没变时几秒就完。
//
//    **`--features tauri/custom-protocol` 不能少**：tauri 的 build.rs 按
//    `dev = !custom_protocol` 设 `cfg(dev)`（`tauri-2.12.3/build.rs` 的
//    `has_feature("custom-protocol")`）。少了它，编出来的是「release 路径 +
//    dev 前端」的**混血二进制**：面板会去连 `http://localhost:5174/`
//    （vite dev server），而 vite 没起 → 面板一片空白，target 却在——
//    症状是「面板存在但没内容」，很容易被误判成驱动问题。
console.log('[shell-release] 编译 release 二进制（TAURI_CONFIG 已注入，custom-protocol 已开）…');
const build = spawnSync(
  'cargo',
  [
    'build',
    '--locked',
    '--release',
    '--features',
    'tauri/custom-protocol',
    '--manifest-path',
    CARGO_MANIFEST,
  ],
  {
    cwd: REPO,
    env: { ...process.env, TAURI_CONFIG: tauriConfig },
    stdio: 'inherit',
    shell: process.platform === 'win32',
  },
);
if (build.status !== 0) {
  console.error(`[shell-release] 编译失败（exit=${build.status}）`);
  process.exit(1);
}
if (!existsSync(RELEASE_EXE)) {
  console.error(`[shell-release] 编译产物不存在：${RELEASE_EXE}`);
  process.exit(1);
}

// ② 隔离 home 的壳设置：内核端口必须与用户正在跑的那套**不同**（默认端口
//    release 是 3090，用户装好的壳多半就用着它）。
const settingsDir = join(SHELL_HOME, 'shell', 'release');
mkdirSync(settingsDir, { recursive: true });
const settingsFile = join(settingsDir, 'settings.json');
if (!existsSync(settingsFile)) {
  writeFileSync(
    settingsFile,
    `${JSON.stringify({ port: KERNEL_PORT, profile: 'web' }, null, 2)}\n`,
    'utf8',
  );
  console.log('[shell-release] 已写壳设置：', settingsFile);
} else {
  console.log('[shell-release] 壳设置已存在，不覆盖：', settingsFile);
}

// ③ 激活指针：壳据此认定「这个版本已安装」。
const { version } = activeReleaseKernel();
mkdirSync(DATA_DIR, { recursive: true });
const activeFile = join(DATA_DIR, 'active.txt');
if (!existsSync(activeFile)) {
  writeFileSync(activeFile, `${version}\n`, 'utf8');
  console.log('[shell-release] 已写 active.txt：', activeFile, '=', version);
} else {
  console.log('[shell-release] active.txt 已存在，不覆盖：', activeFile);
}

// ④ 起壳。
const child = spawn(RELEASE_EXE, [], {
  cwd: REPO,
  env: {
    ...process.env,
    DSH_XLINK_HOME: SHELL_HOME,
    // 桩模型的凭据：壳起的内核**继承**壳进程的环境变量，所以这里带上，
    // 内核侧的 llm-pi-ai provider 才认得 apiKeyEnv=G1_STUB_KEY。
    G1_STUB_KEY: process.env.G1_STUB_KEY ?? 'g1-stub-key',
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${CDP_PORT}`,
    DSH_XLINK_CHANNEL_SUFFIX: CHANNEL_SUFFIX,
  },
  stdio: 'inherit',
  detached: false,
});
for (const signal of ['SIGINT', 'SIGTERM']) {
  process.on(signal, () => child.kill(signal));
}
child.on('exit', (code, signal) => process.exit(signal ? 1 : (code ?? 0)));
child.on('error', (error) => {
  console.error('[shell-release] 无法启动 release 壳：', error.message);
  process.exit(1);
});
