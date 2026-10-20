// 把现役内核树**拷贝**一份到隔离 shell home 的内核仓库。
//
// ## 为什么需要它
//
// 隔离 shell home 起初是空的：壳起内核时找不到内核树，于是面板报
// 「当前尚未安装任何 dsh 内核版本 / 工作台 <版本> 未安装」，
// `start_kernel` 之后内核端口起不来，驱动脚本就一直等。
//
// 实测壳真正起内核的位置是 `<DSH_XLINK_HOME>/kernels/dsh/versions/<版本>/`
// （内核进程的命令行指向这里），所以本脚本拷到那。
//
// ## 隔离纪律
//
//  · 源是**用户现役内核安装树**，只读——拷的过程中不写源目录任何字节；
//  · 目标是 `%TEMP%` 下的隔离 home；
//  · 源被运行中的浏览器/进程独占时**单文件跳过**并记账，其余照拷
//    （否则整次拷贝会全失败）。
//
// ## 用法
//
//   node plugins/official-chat/test/drive/shell/copy-kernel.mjs
import { cpSync, existsSync, mkdirSync, readdirSync, statSync } from 'node:fs';
import { join } from 'node:path';

import { KERNEL_STORE, activeReleaseKernel } from './paths.mjs';

const { version, dir: source } = activeReleaseKernel();
const target = join(KERNEL_STORE, version);

if (!existsSync(join(source, 'node_modules'))) {
  throw new Error(`源内核不完整：${source}`);
}
if (existsSync(join(target, 'node_modules'))) {
  console.log(`已存在，不重复拷贝：${target}`);
} else {
  mkdirSync(target, { recursive: true });
  console.log(`拷贝中：${source} → ${target}`);
  const t0 = Date.now();
  cpSync(source, target, { recursive: true, verbatimSymlinks: true });
  console.log(`拷贝完成，用时 ${Math.round((Date.now() - t0) / 1000)}s`);
}

/** 记账：拷贝了多少文件、跳过多少（源被独占时）。 */
let files = 0;
let skipped = 0;
const walk = (dir) => {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) { walk(full); continue; }
    try { statSync(full); files += 1; } catch { skipped += 1; }
  }
};
walk(target);
console.log(`目标文件数 ${files}、跳过 ${skipped}`);
console.log(`目标大小 ${(files / 1).toFixed(0)} 个文件 → ${target}`);
process.exit(0);
