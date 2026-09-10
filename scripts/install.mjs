#!/usr/bin/env node
// 检测后安装的包装脚本。
//
// 本仓库是自带 `pnpm-workspace.yaml`（为 esbuild 配置 allowBuilds）的
// 独立交付物，因此普通的 `pnpm install` 不会上溯到其他 checkout。
// 不要传入 `--ignore-workspace`：它会跳过本地 workspace 文件及其
// allowBuilds 白名单，使 esbuild 的 postinstall 变成硬安装错误
// （strictDepBuilds）。当 pnpm 缺失时回退到 npm（npm 默认会运行依赖
// 的 postinstall）。
//
// 请通过仓库根目录的 `npm run deps` 或 `pnpm run deps` 调用；切勿直接
// 执行本文件（它位于 scripts/ 下，并已接入 package.json 的 `scripts.deps`）。

import { execFileSync } from 'node:child_process';

const isWin = process.platform === 'win32';
// 在 Windows 上无法直接派生 `.cmd` shim（Node 会返回 EINVAL）；
// 本桌面项目中的包管理器脚本出于同样原因全部走 `%ComSpec% /C`。
const comspec = isWin ? (process.env.ComSpec || 'cmd.exe') : null;

// `opts` 透传给 `execFileSync`。探测版本号（`has`）用默认值把输出捕获丢弃；
// 真正的 `install` 必须传 `{ stdio: 'inherit' }`——否则依赖安装期间终端一片
// 空白，且 `execFileSync` 默认 1 MiB 的 `maxBuffer` 会在输出超限时以难以
// 理解的 `ENOBUFS` 失败。
function run(cmd, args, opts = {}) {
  if (isWin) {
    return execFileSync(comspec, ['/C', cmd, ...args], opts);
  }
  return execFileSync(cmd, args, opts);
}

function has(cmd) {
  try {
    run(cmd, ['--version']);
    return true;
  } catch {
    return false;
  }
}

const usePnpm = has('pnpm');
const pkgMgr = usePnpm ? 'pnpm' : 'npm';
const args = ['install'];

if (!usePnpm) {
  console.log('[install] pnpm 未检测到，回退到 npm');
}
console.log(`[install] 正在执行：${pkgMgr} ${args.join(' ')}`);

try {
  run(pkgMgr, args, { stdio: 'inherit' });
} catch (err) {
  // 上面已用 stdio: 'inherit'，子进程输出已经实时流到终端；这里负责暴露
  // 失败原因与退出码。不要吞掉 `err.message`：`ENOBUFS`（子进程输出超过
  // maxBuffer）与 `ENOENT`（包管理器不在 PATH 上）这类原因只存在于
  // message 里，只打印 `err.status` 会得到一句没有诊断价值的「退出码 ?」。
  console.error(`[install] ${pkgMgr} install 失败：${err.message ?? err}`);
  console.error('[install] 请先修复上面的报错后重试 `npm run deps`；');
  console.error('[install] 若报错提到网络或 registry，请检查代理设置后重试。');
  process.exit(err.status ?? 1);
}