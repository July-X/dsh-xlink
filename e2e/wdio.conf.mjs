// 用内嵌 WebDriver 驱动 **dsh-xlink 主壳** 的配置。
//
// 这条路存在的原因是 macOS 的 WKWebView 不给外部留调试附着点（没有 CDP、
// 没有 inspector 端口），而 `tauri-driver` 官方明确不支持 macOS。`embedded`
// provider 把 W3C WebDriver 服务直接跑在 app 进程内（`TAURI_WEBDRIVER_PORT`，
// 默认 4445，仅监听 127.0.0.1），于是主壳第一次变得可被程序驱动。
//
// 插件本身只在 debug 构建里注册（`src-tauri/Cargo.toml` 的
// `[target.'cfg(debug_assertions)'.dependencies]` + `lib.rs` 的
// `#[cfg(debug_assertions)]`），因此这里只能指向 debug 二进制；门禁见
// `scripts/check-webdriver-gating.mjs`。
//
// 二进制名来自 `Cargo.toml` 的 `package.name`（本 crate 没有 `[[bin]]`），
// **不是** `tauri.conf.json` 的 `productName`——后者是安装包显示名。

import { existsSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, '..');
const debugBinary = resolve(repoRoot, 'src-tauri/target/debug/dsh-xlink');

export const config = {
  runner: 'local',
  hostname: '127.0.0.1',
  port: 4444,
  // 路径相对于 **配置文件所在目录**（WDIO 的 rootDir 默认取 wdio.conf 所在处，
  // 即 `e2e/`），不是相对于 `npm run` 的 cwd。写 `./e2e/specs/…` 会被解析成
  // `e2e/e2e/specs/…`，报的是 `pattern did not match any file`。
  specs: ['./specs/**/*.spec.mjs'],
  maxInstances: 1,
  capabilities: [
    {
      browserName: 'tauri',
      'tauri:options': {
        application: debugBinary,
      },
    },
  ],
  services: [
    [
      '@wdio/tauri-service',
      {
        appBinaryPath: debugBinary,
        driverProvider: 'embedded',
      },
    ],
  ],
  reporters: ['spec'],
  framework: 'mocha',
  mochaOpts: {
    ui: 'bdd',
    // 首帧要跑完 setup()：建窗、数据目录搬迁、启动防护扫描，都是同步 IO。
    timeout: 120_000,
  },
  // **必须**用 `tauri build`，不能用裸 `cargo build`：后者编出来的二进制会去
  // 连 devUrl（`http://localhost:5174`），而 E2E 期间没有 vite 在跑，
  // webview 只会白屏——表现是 session 建起来了但页面上什么都没有。
  //
  // `prep:builtin` 也不能省：build script 会校验 `bundle.resources` 里登记的
  // 每一个路径，`resources/builtin-plugins` 是构建期产物（.gitignore 挡着），
  // 缺一步就在这里报 `resource path ... doesn't exist`（2026-10-09 发
  // v0.4.4 那次两个发布通道同时红，根因正是它）。
  onPrepare: () => {
    const build = spawnSync(
      'npm',
      ['run', 'prep:builtin', '--silent'],
      { cwd: repoRoot, stdio: 'inherit', shell: true },
    );
    if (build.status !== 0) throw new Error('prep:builtin 失败');

    const tauriBuild = spawnSync(
      'npx',
      // `--features wdio` 是这个插件唯一的启用方式：`default` 为空，
      // release 构建不带它，因此发布产物里不会有这个端口
      // （判据见 scripts/check-webdriver-gating.mjs）。
      ['tauri', 'build', '--debug', '--no-bundle', '--features', 'wdio'],
      { cwd: repoRoot, stdio: 'inherit', shell: true },
    );
    if (tauriBuild.status !== 0) throw new Error('tauri build --debug 失败');

    if (!existsSync(debugBinary)) {
      throw new Error(`debug 二进制不存在：${debugBinary}`);
    }
  },
};