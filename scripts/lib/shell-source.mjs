// 按**文件名**在 `src-tauri/src` 下递归定位壳侧源码。
//
// 为什么不能写死完整路径：2026-10-01 `src-tauri/src` 从平铺改成按功能分目录
// （`harness/`、`diagnostics/`、`plugins/`…）之后，所有按路径读壳侧源码的判据
// 一次性出事——`check-invariants.mjs` 读 `no-context-menu.js` 直接 ENOENT 崩在
// 脚本启动阶段，4 个 UI 测试整体挂掉，`scripts/titlebar-pulse.test.mjs` 同样
// ENOENT，而它们要检查的契约一个字都没变。判据跟着目录结构走，就会在每次
// 重组时崩一次；崩得多了就没人信门禁了。
//
// 所以：**判据要问「哪个模块」，不是「文件在哪一层」**。basename 在本仓跨目录
// 唯一（十个同名 `mod.rs` 除外——重名时抛错，不猜）。这与
// `check-code-budget.mjs` 的 `isKnownBlob()` / `moduleId()` 用内容与模块标识
// 而非路径判搬移，是同一条纪律。
//
// 门禁脚本（Node，直接跑在仓库根）与 UI 测试（从 `ui/test/` 导入）共用这一份
// 实现：两套解析器迟早会在某次重组后只修其中一套。

import { readdirSync, readFileSync } from 'node:fs';
import { dirname, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');
const SHELL_SRC_ROOT = join(REPO_ROOT, 'src-tauri', 'src');

/** 只跳过构建产物与依赖目录——它们不在 `src-tauri/src` 下，留着是为了让这个
 *  解析器在将来被放宽搜索根时仍然安全。 */
const SKIP_DIRS = new Set(['node_modules', 'target', 'dist']);

function collect(dir, fileName, hits) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (SKIP_DIRS.has(entry.name)) continue;
    const full = join(dir, entry.name);
    if (entry.isDirectory()) {
      collect(full, fileName, hits);
    } else if (entry.name === fileName) {
      hits.push(full);
    }
  }
  return hits;
}

/** `src-tauri/src` 下唯一的同名文件。零个或多个都抛错，见文件头。 */
export function shellSourcePath(fileName) {
  const hits = collect(SHELL_SRC_ROOT, fileName, []);
  if (hits.length === 0) {
    throw new Error(
      `src-tauri/src 下找不到 ${fileName}：文件被删了或改名了。` +
        '要改的是这条判据指向的文件，不是这个解析器。',
    );
  }
  if (hits.length > 1) {
    throw new Error(
      `${fileName} 在 src-tauri/src 下不唯一，无法判断该读哪一份：\n  ${hits
        .map((hit) => repoRelative(hit))
        .join('\n  ')}`,
    );
  }
  return hits[0];
}

/** 读壳侧某一个源文件的文本内容。 */
export function readShellSource(fileName) {
  return readFileSync(shellSourcePath(fileName), 'utf8');
}

/** 仓库根相对路径，正斜杠分隔——给测试名与断言消息用，读者照着能直接找。 */
export function shellSourceLabel(fileName) {
  return repoRelative(shellSourcePath(fileName));
}

function repoRelative(absolute) {
  return relative(REPO_ROOT, absolute).split(sep).join('/');
}
