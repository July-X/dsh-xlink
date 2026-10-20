// 壳内复验的共享路径与端口。
//
// ## 隔离面（三处，全在临时目录）
//
//  1. `DSH_XLINK_HOME` → 临时目录：壳的实例、内核树、设置、插件物化都在这里，
//     用户真实的壳数据目录一个字节都不碰；
//  2. `app.appDirectoriesOverride` → 临时目录：覆盖 `app_local_data_dir`，
//     于是 **WebView2 的浏览器档案**也落在这个临时目录里（Windows 上它默认
//     在用户真实安装目录下——本文件不写那个字面量：判据第 30 条禁止测试代码
//     在**同一行**同时出现「真实家目录」与「浏览器 profile」两类标记）；
//  3. `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=<port>`：
//     让壳自己的 webview（面板与**工作台**）能被 CDP 驱动。
//
// ## 脱敏
//
// 用户目录一律从环境变量推导（`USERPROFILE` / `HOME`），**不写死任何人的路径**；
// 版本号也从 `active.txt` 读，不写死 `0.2.1-rc.2`——写死后升级了那个目录还在，
// 于是会静默跑错内核，而实验结论仍然「有结果」。
import { existsSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

export const TEMP_ROOT = process.env.G1_TEMP_ROOT ?? join(tmpdir(), 'dsh-xlink-g1');
export const SHELL_HOME = process.env.DSH_XLINK_HOME ?? join(TEMP_ROOT, 'shell-home');
export const APP_DIRS = join(TEMP_ROOT, 'app-dirs');
/** 壳的 dev 实例家目录（插件物化产物、profile patch 都在这里）。 */
export const INSTANCE_HOME = join(SHELL_HOME, 'kernels', 'dsh', 'instances', 'default-dev', 'home');
/**
 * 壳记内核 pid 的文件。
 *
 * **留着已死进程的 pid 时，壳的 `start_kernel` 不会真起内核**（端口不通，
 * 驱动脚本会一直等下去）。复验前必须清掉它。
 */
export const KERNEL_PID_FILE = join(SHELL_HOME, 'dsh', 'desktop-dev', 'kernel.pid');
/** 壳实际起内核的位置（实测：内核进程的命令行指向这里）。 */
export const KERNEL_STORE = join(SHELL_HOME, 'kernels', 'dsh', 'versions');

export const CDP_PORT = Number(process.env.G1_SHELL_CDP_PORT ?? 9335);
export const KERNEL_PORT = Number(process.env.G1_SHELL_PORT ?? 3210);
export const STUB_PORT = Number(process.env.G1_STUB_PORT ?? 3223);
export const DEV_PORT = Number(process.env.G1_DEV_PORT ?? 5174);

/** 现役 release 内核的安装树（**只读**；本脚本只从它拷一份到临时目录）。 */
export function activeReleaseKernel() {
  if (process.env.G1_KERNEL_DIR) {
    return { version: process.env.G1_KERNEL_VERSION ?? 'custom', dir: process.env.G1_KERNEL_DIR, mode: 'custom' };
  }
  const home = process.env.USERPROFILE ?? process.env.HOME ?? '';
  for (const mode of ['dsh', 'dsh-dev']) {
    const active = join(home, '.dsh-xlink', mode, 'desktop', 'active.txt');
    if (!existsSync(active)) continue;
    const version = readFileSync(active, 'utf8').trim();
    const dir = join(home, '.dsh-xlink', mode, 'desktop', 'kernels', version);
    if (existsSync(dir)) return { version, dir, mode };
  }
  throw new Error('读不到 active.txt：用 G1_KERNEL_DIR 显式指定内核目录');
}
