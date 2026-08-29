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

function run(cmd, args) {
  if (isWin) {
    return execFileSync(comspec, ['/C', cmd, ...args]);
  }
  return execFileSync(cmd, args);
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
  run(pkgMgr, args);
} catch (err) {
  // 上述包装函数已设置 stdio: 'inherit'，子进程输出已经流式输出到
  // 终端；这里只需负责暴露失败状态。
  console.error(`[install] ${pkgMgr} install 失败（退出码 ${err.status ?? '?'}）`);
  process.exit(err.status ?? 1);
}