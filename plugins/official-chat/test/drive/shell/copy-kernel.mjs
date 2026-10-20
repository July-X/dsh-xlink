// 把现役内核树**拷贝**一份到隔离 shell home 的内核仓库。
//
// ## 为什么需要它
//
// 隔离 shell home 起初是空的：壳起内核时找不到内核树，于是面板报
// 「当前尚未安装任何 dsh 内核版本 / 工作台 <版本> 未安装」，
// `start_kernel` 之后内核端口起不来，驱动脚本就一直等。
//
// 实测壳真正起内核的位置是 `<data_dir>/kernels/<版本>/`
// （`data_dir` = `<DSH_XLINK_HOME>/dsh/<desktop[-dev]>/`，dev 壳是 `desktop-dev`、
// release 壳是 `desktop`），内核进程的命令行指向
// `…/kernels/<版本>/node_modules/@deepseek-ai/dsh/lib/bin.js web --no-open --port …`。
// 依据：`src-tauri/src/kernel/lifecycle.rs:322-328`（`kernel_dir(data_dir, version)`）
// 与 `src-tauri/src/commands.rs:2836`（`kernel_install_root` = `AppState.data_dir`）。
// **别写成 `<home>/kernels/dsh/versions/`**——那是内核树**将来**要搬到的位置
// （P3），现在壳不去那里找，写错只会得到「版本未安装或安装不完整」。
//
// ## 隔离纪律
//
//  · 源是**用户现役内核安装树**，只读——拷的过程中不写源目录任何字节；
//  · 目标是 `%TEMP%` 下的隔离 home；
//  · **逐文件拷**：源树 528MB / 3000+ 文件，可能有文件正被运行中的进程独占
//    （Windows 文件锁）。整树直拷会因那一个文件**整次抛错**，留下一棵半拷贝的树
//    ——比「少拷一个文件」糟得多。独占的**跳过并记账**，最后照实报出来，
//    不假装完整（PM 验收时指出：旧版注释说「跳过并记账」，实现却是整树直拷）。
//
// ## 用法
//
//   node plugins/official-chat/test/drive/shell/copy-kernel.mjs
import { copyFileSync, existsSync, mkdirSync, readdirSync, statSync } from 'node:fs';
import { join } from 'node:path';

import { KERNEL_STORE, activeReleaseKernel } from './paths.mjs';

const { version, dir: source, flavor } = activeReleaseKernel();
const target = join(KERNEL_STORE, version);

/**
 * 逐文件拷贝一棵树，返回记账。
 *
 * @returns {{copied: number, skipped: number, bytes: number, skippedFiles: {file: string, why: string}[]}}
 */
function copyTree(src, dst) {
  let copied = 0;
  let skipped = 0;
  let bytes = 0;
  const skippedFiles = [];
  const walk = (from, to) => {
    mkdirSync(to, { recursive: true });
    for (const entry of readdirSync(from, { withFileTypes: true })) {
      const s = join(from, entry.name);
      const d = join(to, entry.name);
      if (entry.isDirectory()) { walk(s, d); continue; }
      try {
        copyFileSync(s, d);
        copied += 1;
        bytes += statSync(d).size;
      } catch (error) {
        // 源文件被运行中的进程独占：跳过这一个，其余照拷。
        skipped += 1;
        if (skippedFiles.length < 20) {
          skippedFiles.push({ file: entry.name, why: error.code ?? String(error?.message ?? error) });
        }
      }
    }
  };
  walk(src, dst);
  return { copied, skipped, bytes, skippedFiles };
}

if (!existsSync(join(source, 'node_modules'))) {
  throw new Error(`源内核不完整：${source}`);
}

if (existsSync(join(target, 'node_modules'))) {
  console.log(`已存在，不重复拷贝：${target}`);
} else {
  mkdirSync(target, { recursive: true });
  console.log(`拷贝中：${source}（${flavor}）→ ${target}`);
  const t0 = Date.now();
  const result = copyTree(source, target);
  const mb = (result.bytes / 1024 / 1024).toFixed(1);
  console.log(
    `拷贝完成，用时 ${Math.round((Date.now() - t0) / 1000)}s：`
    + `成功 ${result.copied} 个文件、跳过 ${result.skipped} 个、合计 ${mb} MB`,
  );
  if (result.skipped > 0) {
    console.log('被独占而跳过的文件（前 20 个）：');
    for (const item of result.skippedFiles) console.log(`  ${item.file} —— ${item.why}`);
    console.log('（这棵树不完整：被跳过的文件对应的模块可能加载失败）');
  }
}

// 复核目标树：文件数与总大小（**报实际值**，不拿文件数冒充实测大小）
let files = 0;
let size = 0;
const measure = (dir) => {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) { measure(full); continue; }
    try { files += 1; size += statSync(full).size; } catch { /* 读不到就跳过 */ }
  }
};
if (existsSync(target)) measure(target);
console.log(`目标：${target}`);
console.log(`目标文件数 ${files}、合计 ${(size / 1024 / 1024).toFixed(1)} MB`);
process.exit(0);
