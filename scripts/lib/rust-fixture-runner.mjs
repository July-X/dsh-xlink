// Rust 端夹具 runner 的**共享入口**。
//
// ## 它是什么
//
// `src-tauri/examples/p1_protocol_fixture.rs` 的产物：跑**同一组协议夹具**，
// 输出 `{schemaVersion, constants, errorCodes, cases:[{id,result}]}`。
//
// ## 为什么抽成共享模块
//
// P1 的 A1c（双端一致）与 P2 的跨语言判据（两端 `groupKey` 相等）都要读它。
// **产物路径只能有一处定义** —— 两份拷贝迟早漂移，而漂移的表现是
// 「某一端静默地没被判到」（最坏的一种假绿）。
//
// ## 没构建怎么办
//
// **产物不存在 ⇒ 返回 `undefined`**（不是空数组）⇒ 调用方据此报「未执行」，
// **不许当成功**（P1 A1c 的既有口径：Rust 侧没有产物 ⇒ 本条未执行，不判通过）。
//
// @module scripts/lib/rust-fixture-runner

import { execFileSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { join } from 'node:path';

/**
 * 跑 Rust 端的夹具 runner，拿它的**完整产物**。
 *
 * @param {string} repoRoot - 仓库根目录。
 * @returns {{schemaVersion?: number, constants?: object, errorCodes?: object,
 *   cases?: Array<{id: string, result: unknown}>}|undefined}
 *   产物不存在 / 跑不动 ⇒ `undefined`（调用方报「未执行」）。
 */
export function runRustFixturePayload(repoRoot) {
  const exe = process.platform === 'win32' ? 'p1_protocol_fixture.exe' : 'p1_protocol_fixture';
  const bin = join(repoRoot, 'src-tauri', 'target', 'debug', 'examples', exe);
  if (!existsSync(bin)) return undefined;
  try {
    const stdout = execFileSync(bin, [], {
      cwd: repoRoot,
      encoding: 'utf8',
      maxBuffer: 64 * 1024 * 1024,
    });
    const parsed = JSON.parse(stdout.trim());
    return parsed !== null && typeof parsed === 'object' ? parsed : undefined;
  } catch {
    return undefined;
  }
}

/**
 * 只取 Rust 端跑夹具的结果（`[{id, result}]`）。
 *
 * @param {string} repoRoot - 仓库根目录。
 * @returns {Array<{id: string, result: unknown}>|undefined}
 */
export function runRustFixtureCases(repoRoot) {
  return runRustFixturePayload(repoRoot)?.cases;
}
