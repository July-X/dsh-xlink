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

/**
 * 壳模式：`dev`（`tauri dev` 起的调试壳）或 `release`（自编 release 二进制）。
 *
 * **它不是运行时开关，是构建产物之间的差异**（壳源码 `shell/paths.rs` 的
 * `ShellMode::current()` 由 `cfg!(debug_assertions)` 决定），于是它连带决定
 * 三件事：运行时子目录（`desktop` / `desktop-dev`）、默认实例 id
 * （`default` / `default-dev`）、以及面板 URL（vite dev server / 内嵌资源）。
 * 依据：`src-tauri/src/shell/paths.rs:96-102`（`runtime_subdir`）与
 * `src-tauri/src/shell/instance.rs:95-106`（`default_instance_id_for`）。
 */
export const SHELL_MODE = (process.env.G1_SHELL_MODE ?? 'dev').toLowerCase();
export const IS_RELEASE = SHELL_MODE === 'release';

export const SHELL_HOME = process.env.DSH_XLINK_HOME
  ?? join(TEMP_ROOT, IS_RELEASE ? 'shell-home-release' : 'shell-home');
export const APP_DIRS = join(TEMP_ROOT, IS_RELEASE ? 'app-dirs-release' : 'app-dirs');
/** 运行时子目录（release=`desktop`、dev=`desktop-dev`）。 */
export const RUNTIME_SUBDIR = IS_RELEASE ? 'desktop' : 'desktop-dev';
/** 默认实例 id（release=`default`、dev=`default-dev`）。 */
export const INSTANCE_ID = IS_RELEASE ? 'default' : 'default-dev';
/** 壳的数据目录（`<home>/dsh/<desktop[-dev]>/`）。 */
export const DATA_DIR = join(SHELL_HOME, 'dsh', RUNTIME_SUBDIR);
/** 壳的实例家目录（插件物化产物、profile patch 都在这里）。 */
export const INSTANCE_HOME = join(SHELL_HOME, 'kernels', 'dsh', 'instances', INSTANCE_ID, 'home');
/**
 * 壳记内核 pid 的文件。
 *
 * **留着已死进程的 pid 时，壳的 `start_kernel` 不会真起内核**（端口不通，
 * 驱动脚本会一直等下去）。复验前必须清掉它。
 */
export const KERNEL_PID_FILE = join(DATA_DIR, 'kernel.pid');
/**
 * 壳**真正**起内核的位置：`<data_dir>/kernels/<版本>/`。
 *
 * 依据（现役 release 树 `~/.dsh-xlink/dsh/desktop/kernels/0.2.1-rc.2` 与
 * 壳源码对得上）：`src-tauri/src/kernel/lifecycle.rs:322-328`
 * （`kernels_dir(data_dir)` = `data_dir/kernels`，`kernel_dir` 再拼版本号）
 * 与 `src-tauri/src/commands.rs:2817/2836`（`kernel_install_root` 就是
 * `AppState.data_dir`，即 `<home>/dsh/<desktop[-dev]>/`）。
 *
 * **注意**：`<home>/kernels/dsh/versions/<版本>/` 是内核安装树**将来**要搬到的
 * 位置（P3），现在壳**不去那里找**——写错会让壳报「版本未安装或安装不完整」。
 */
export const KERNEL_STORE = join(DATA_DIR, 'kernels');

export const CDP_PORT = Number(process.env.G1_SHELL_CDP_PORT ?? (IS_RELEASE ? 9336 : 9335));
export const KERNEL_PORT = Number(process.env.G1_SHELL_PORT ?? (IS_RELEASE ? 3211 : 3210));
export const STUB_PORT = Number(process.env.G1_STUB_PORT ?? (IS_RELEASE ? 3224 : 3223));
/** vite dev server 端口（**只有 dev 模式用得上**：release 面板走内嵌资源）。 */
export const DEV_PORT = Number(process.env.G1_DEV_PORT ?? 5174);

/**
 * 现役内核安装树（**只读**；本脚本只从它拷一份到临时目录）。
 *
 * ## 布局（2026-10-20 实测纠正）
 *
 * 现役树是 `<家目录>/.dsh-xlink/<家族>/desktop[-dev]/`：
 * 家族恒为 `dsh`，`desktop` = release（实例 id `default`）、
 * `desktop-dev` = debug（实例 id `default-dev`）。
 *
 * **不是** `.dsh-xlink/dsh-dev/desktop/`——那个组合在真实树里根本不存在，
 * 写错会让这个分支永远走不到（PM 验收时指出的第 ④ 处）。
 * 依据：`src-tauri/src/lib.rs:197-211`（data_dir 按（注册表里）默认实例的
 * 内核族解析到 `<xlink_home>/<family>/desktop[-dev]/`）与
 * `src-tauri/src/kernel/lifecycle.rs:2864`（data_dir 须以 desktop 或 desktop-dev 结尾）。
 */
export function activeReleaseKernel() {
  if (process.env.G1_KERNEL_DIR) {
    return { version: process.env.G1_KERNEL_VERSION ?? 'custom', dir: process.env.G1_KERNEL_DIR, mode: 'custom' };
  }
  const home = process.env.USERPROFILE ?? process.env.HOME ?? '';
  for (const flavor of ['desktop', 'desktop-dev']) {
    const active = join(home, '.dsh-xlink', 'dsh', flavor, 'active.txt');
    if (!existsSync(active)) continue;
    const version = readFileSync(active, 'utf8').trim();
    const dir = join(home, '.dsh-xlink', 'dsh', flavor, 'kernels', version);
    if (existsSync(dir)) return { version, dir, flavor };
  }
  throw new Error('读不到 active.txt：用 G1_KERNEL_DIR 显式指定内核目录');
}
