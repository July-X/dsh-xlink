// 用内嵌 WebDriver 驱动 **dsh-xlink 主壳** 的配置。
//
// 这条路存在的原因是 macOS 的 WKWebView 不给外部留调试附着点（没有 CDP、
// 没有 inspector 端口），而 `tauri-driver` 官方明确不支持 macOS。`embedded`
// provider 把 W3C WebDriver 服务直接跑在 app 进程内（`TAURI_WEBDRIVER_PORT`，
// 默认 4445，仅监听 127.0.0.1），于是主壳第一次变得可被程序驱动。
//
// **本文件不再自己构建**（2026-11-01 OPT-08）：隔离 home / 浏览器档案 /
// 单实例通道与构建输出全部由 `e2e/launcher.mjs` 创建并注入。直接 `wdio run`
// 会因为没有隔离环境而拒绝启动——这是刻意的，裸跑会把真实家目录交给测试壳。
//
// 插件只在 debug 构建里注册（Cargo optional 依赖 + lib.rs 的 wdio feature），
// 因此这里只能指向 debug 二进制；门禁见 `scripts/check-webdriver-gating.mjs`。

import { existsSync } from 'node:fs';
import { resolve, sep } from 'node:path';

function requireEnv(name) {
  const value = process.env[name];
  if (!value) {
    throw new Error('E2E 必须通过 npm run e2e 启动：缺少隔离变量 ' + name);
  }
  return value;
}

const appBinary = requireEnv('DSH_E2E_APP_BINARY');
const runRoot = resolve(requireEnv('DSH_E2E_RUN_ROOT'));
const expectedHome = resolve(requireEnv('DSH_E2E_EXPECTED_HOME'));
const expectedAppData = resolve(requireEnv('DSH_E2E_EXPECTED_APP_DATA'));
const channelSuffix = requireEnv('DSH_XLINK_CHANNEL_SUFFIX');

if (!existsSync(appBinary)) {
  throw new Error('launcher 指定的 debug 二进制不存在：' + appBinary);
}
if (resolve(process.env.DSH_XLINK_HOME) !== expectedHome) {
  throw new Error('DSH_XLINK_HOME 与 launcher 记录的实例根不一致');
}
for (const [name, value] of Object.entries({
  DSH_E2E_RUN_ROOT: runRoot,
  DSH_E2E_EXPECTED_HOME: expectedHome,
  DSH_E2E_EXPECTED_APP_DATA: expectedAppData,
})) {
  if (value !== runRoot && !value.startsWith(runRoot + sep)) {
    throw new Error(name + ' 不在本轮测试根内：' + value);
  }
}
if (!channelSuffix.startsWith('e2e-')) {
  throw new Error('单实例通道没有使用 launcher 生成的 runId：' + channelSuffix);
}

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
      'tauri:options': { application: appBinary },
    },
  ],
  services: [
    [
      '@wdio/tauri-service',
      { appBinaryPath: appBinary, driverProvider: 'embedded' },
    ],
  ],
  reporters: ['spec'],
  framework: 'mocha',
  mochaOpts: {
    ui: 'bdd',
    // 首帧要跑完 setup()：建窗、数据目录搬迁、启动防护扫描，都是同步 IO。
    timeout: 120_000,
    // 根级 beforeAll：在**任何用例执行前**从运行中的后端读实际 data_dir
    // （disk_usage 报表里的绝对路径；get_status 里的 data_dir 是展示字符串，
    // 带 `~` 与中间省略，没法 canonicalize），canonicalize 后必须落在本轮
    // runRoot 内；逃逸就抛错，整轮直接失败（2026-11 FIX-06）。路径相对本配置
    // 文件所在目录（rootDir）。
    require: ['./isolation-probe.mjs'],
  },
  // launcher 已构建并核对过二进制与隔离目录；这里只做最后的启动前确认，
  // 不再自己跑 tauri build——否则会绕开 CARGO_TARGET_DIR 与 TAURI_CONFIG。
  onPrepare: () => {
    if (!existsSync(expectedAppData)) {
      throw new Error('launcher 创建的浏览器档案目录不存在：' + expectedAppData);
    }
  },
};
