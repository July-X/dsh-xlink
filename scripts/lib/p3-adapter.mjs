// P3 协议判据的**实跑适配器**：把真实实现（`plugins/official-chat/**`）接到
// P3 判据库的 adapter 契约上。
//
// ## 边界（与 `p1-adapter.mjs` 同款纪律）
//
// 适配器只做三件事：
//  1. **造上下文**（host 侧真落盘 store、真 sweeper、真命令层）——**不替实现做协议判断**；
//  2. **把实现的可观测口翻译成判据要的形状**（盘上清单、占用记录、清理次序、mode）；
//  3. **如实报告观测不到的东西**（返回 `undefined` / `null`），让判据报「未执行」
//     而不是假绿或假红。
//
// ## 与 P1 adapter 的关系
//
// **另起一套**，不改 `p1-adapter.mjs`（P1/P2 的判据一行不动）：
//  · P1 adapter 的观测面是「客户端 + 正文 + 提示」（P2 的领域）；
//  · P3 的观测面是「**盘上布局**」（快照目录清单、占用记录 `refs/`、清理次序、mode）。
//
// ## 三个「必须另起子进程」的地方（**不是偏好，是 Node 的语义**）
//
// `node:fs` 的 ESM facade 在**首次被任何 ESM 模块 import 时**就快照了 CJS 导出值
// ⇒ 同进程里 patch `fs.writeFileSync` **不生效**（本模块自己就静态 import 了
// `node:fs`）⇒ 三处注入/观测都在**子进程**里做：
//
//  1. `runInjectedCapture`：patch `writeFileSync` ⇒ 造「写到一半失败」的半截文件
//     （P3-1①／P3-2 用）；
//  2. `runSweep`：patch `readdirSync` / `readFileSync` / `renameSync` / `unlinkSync`
//     ⇒ **观测清理过程真实的调用序列**（P3-3④／P3-5③ 用），并可**注入一次删除失败**
//     （EACCES）来验「清理失败只记日志、不阻塞整轮」；
//  3. 两处共用同一个 `runInSubprocess` 骨架（仓库既有先例：`p1-adapter.mjs` 的
//     `reserveInChildProcess` 用 `execFileSync` 跑 `--input-type=module -e`）。
//
// ## 隔离
//
// 落盘根目录一律在 `%TEMP%` 下的一次性目录里（`DSH_XLINK_OFFICIAL_CHAT_ROOT`
// 显式注入，host 半边的根入口认这个覆盖项，见 `host/index.js:255`），
// **绝不指向用户真实数据目录**。每次 `createP3Adapter` 用一个新的一次性目录。
//
// @module scripts/lib/p3-adapter

import { execFileSync } from 'node:child_process';
import {
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  rmSync,
  statSync,
  utimesSync,
  writeFileSync,
} from 'node:fs';
import { join } from 'node:path';
import { pathToFileURL, fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';

const HOST_DIR_URL = new URL('../../plugins/official-chat/host/', import.meta.url);
const REPO_ROOT_URL = new URL('../../', import.meta.url);

/** 协议 §4 的常量（**逐条转写自协议稿**，不 import 实现）。 */
export const TMP_RESIDUAL_MS = 24 * 60 * 60 * 1000;
/** 占用记录兜底释放阈值（R2）＝ 快照保留期（7 天）。 */
export const OCCUPANCY_RETENTION_MS = 7 * 24 * 60 * 60 * 1000;
/** 快照保留期（P1 §4，沿用）。 */
export const SNAPSHOT_RETENTION_MS = 7 * 24 * 60 * 60 * 1000;

/**
 * 一次性临时目录（判据专用，绝不指向用户真实数据目录）。
 *
 * @param {string} tag - 目录名后缀，便于事后辨认。
 * @returns {string}
 */
export function makeTempRoot(tag = 'p3') {
  const dir = join(
    tmpdir(),
    `dsh-p3-protocol-${tag}-${Date.now()}-${Math.floor(Math.random() * 1e6)}`,
  );
  mkdirSync(dir, { recursive: true });
  return dir;
}

/**
 * 删掉一次性临时目录（**只删自己创建的**）。
 *
 * @param {string} dir
 */
export function removeTempRoot(dir) {
  if (typeof dir !== 'string' || dir === '') return;
  if (!dir.includes('dsh-p3-protocol-')) {
    throw new Error(`拒绝删除不是判据自己建的目录：${dir}`);
  }
  rmSync(dir, { recursive: true, force: true });
}

/**
 * 快照目录里**最终名**的清单（`*.json` 且 id 形状合法）。
 *
 * ## 为什么按「形态」而不是「点开头」过滤
 *
 * 协议 §S-9 说「临时文件**不进清单**」，但同时说「协议要的是**性质**，不是命名」
 * ⇒ 判据只断「清单里有几个**最终名**／几个**临时名**」这个**可观测事实**。
 * ⇒ 这里的「最终名」判据 = **文件名以 `.json` 结尾**（实现侧目录扫描的口径）
 * **且**去掉 `.json` 后的 id 形状合法（`host/protocol.js` 的 `SNAPSHOT_ID_RE` 口径）。
 *
 * @param {string} root - 落盘根。
 * @param {string} providerId - provider 短码。
 * @returns {string[]} 最终名文件名（不含目录）。
 */
export function listFinalNames(root, providerId) {
  const dir = join(root, 'snapshots', providerId);
  if (!existsSync(dir)) return [];
  const out = [];
  for (const name of readdirSync(dir)) {
    if (!name.endsWith('.json')) continue;
    const id = name.slice(0, -'.json'.length);
    if (id.split('~').length !== 3) continue;
    out.push(name);
  }
  return out.sort();
}

/**
 * 占用记录 `refs/<snapshotId>.json` 是否存在。
 *
 * @param {string} root
 * @param {string} snapshotId
 * @returns {boolean}
 */
export function occupancyRecordExists(root, snapshotId) {
  return existsSync(join(root, 'refs', `${snapshotId}.json`));
}

/**
 * 读占用记录（**读不动就是读不动**——不猜、不兜成空对象）。
 *
 * @param {string} root
 * @param {string} snapshotId
 * @returns {{ok: true, value: object}|{ok: false, error: string}}
 */
export function readOccupancyRecord(root, snapshotId) {
  const path = join(root, 'refs', `${snapshotId}.json`);
  try {
    return { ok: true, value: JSON.parse(readFileSync(path, 'utf8')) };
  } catch (error) {
    return { ok: false, error: String(error?.message ?? error) };
  }
}

/**
 * 在**子进程**里跑一段脚本，脚本里可以先 patch `node:fs` 再动态 import 插件。
 *
 * ## 为什么必须另起进程
 *
 * `node:fs` 的 ESM facade 在**首次被任何 ESM 模块 import 时**就快照了 CJS 导出值
 * ⇒ 同进程里只要有别的模块先 import 过 `node:fs`（本仓库的 `p1-adapter.mjs:28`
 * 就是），patch 就不生效 ⇒ 「半截文件」造不出来 ⇒ P3-1① 会**假绿**。
 * ⇒ 仓库既有先例：`p1-adapter.mjs` 的 `reserveInChildProcess` 用 `execFileSync`
 * 跑 `--input-type=module -e`（理由同样写在那个函数的注释里）。
 *
 * @param {string} script - ESM 脚本正文（`await import` 可用）。
 * @param {object} [options]
 * @param {number} [options.timeoutMs] - 子进程超时（默认 60s）。
 * @returns {{status: number, stdout: string, stderr: string}}
 */
export function runInSubprocess(script, { timeoutMs = 60_000 } = {}) {
  const result = { status: 0, stdout: '', stderr: '' };
  try {
    result.stdout = execFileSync(
      process.execPath,
      ['--input-type=module', '-e', script],
      { encoding: 'utf8', timeout: timeoutMs, stdio: ['ignore', 'pipe', 'pipe'] },
    );
  } catch (error) {
    result.status = Number(error?.status ?? 1);
    result.stdout = String(error?.stdout ?? '');
    result.stderr = String(error?.stderr ?? String(error?.message ?? error));
  }
  return result;
}

/**
 * 造一个**子进程脚本的公共前导**（patch `node:fs` 的 `writeFileSync`）。
 *
 * ## 注入语义
 *
 * `mode` 取值：
 *  · `'throw-before'`：**一个字节都不写**就抛错（模拟「连写都没开始」）；
 *  · `'throw-mid'`：**写一半**再抛错（模拟「进程在写正文时死掉」⇒ 盘上留半截文件）。
 *
 * @param {object} options
 * @param {string} options.root - 落盘根（传给 `DSH_XLINK_OFFICIAL_CHAT_ROOT`）。
 * @param {'throw-before'|'throw-mid'} options.mode - 注入形态。
 * @param {string} [options.match] - 只对**含这段串**的路径注入（默认 `snapshots`）。
 * @param {number} [options.keepBytes] - `'throw-mid'` 时写多少字节（默认 20）。
 * @returns {string} 可拼在子进程脚本前面的 ESM 代码。
 */
export function fsPatchPreamble({ root, mode, match = 'snapshots', keepBytes = 20 }) {
  const modeExpr = mode === 'throw-mid' ? 'REAL(path, String(data).slice(0, KEEP), opts);' : '';
  return [
    "import { createRequire } from 'node:module';",
    "const require = createRequire('file:///' + process.cwd().replace(/\\\\/g, '/') + '/');",
    "const fs = require('node:fs');",
    'const REAL = fs.writeFileSync;',
    `const MATCH = ${JSON.stringify(match)};`,
    `const KEEP = ${Number(keepBytes)};`,
    'const CALLS = [];',
    `fs.writeFileSync = (path, data, opts) => {`,
    '  CALLS.push(String(path));',
    `  if (String(path).includes(MATCH)) {`,
    `    ${modeExpr}`,
    "    const e = new Error('P3_INJECTED_MID_WRITE');",
    "    e.code = 'EIO';",
    '    throw e;',
    '  }',
    '  return REAL(path, data, opts);',
    '};',
    `process.env.DSH_XLINK_OFFICIAL_CHAT_ROOT = ${JSON.stringify(root)};`,
  ].join('\n');
}

/**
 * 造一个**子进程脚本的公共前导**：patch `node:fs` 的四个读写原语，
 * **记录清理过程真实的调用序列**。
 *
 * ## 为什么这是「观测」而不是「替实现做判断」
 *
 * 判据要断的两件事都是**实现自己做过什么**：
 *  · **有没有扫描占用记录**（§S-12③「回收动作之前，必须先完成一次占用记录的全量扫描」）
 *    ⇒ 看它有没有**读** `refs/` 目录；
 *  · **扫描有没有发生在删除之前** ⇒ 看调用序列的先后。
 * ⇒ 这里只**记录**调用（不改变行为），判定由判据自己做。
 *
 * ## 注入语义
 *
 * `failRemove` 里的路径 ⇒ `renameSync` 抛 `EACCES`（模拟「删不掉」）⇒
 * 用来验「清理失败**只记日志**、**不阻塞整轮**」（§S-10⑤ ＋ P1 §4.1 第 3 条）。
 *
 * @param {object} options
 * @param {string[]} [options.failRemove] - 要注入删除失败的**绝对路径**。
 * @returns {string} 可拼在子进程脚本前面的 ESM 代码。
 */
export function sweepProbePreamble({ failRemove = [] } = {}) {
  return [
    "import { createRequire } from 'node:module';",
    "const require = createRequire('file:///' + process.cwd().replace(/\\\\/g, '/') + '/');",
    "const fs = require('node:fs');",
    'const TRACE = [];',
    'const LOGS = [];',
    `const FAIL = new Set(${JSON.stringify(failRemove.map(String))});`,
    'const realReaddir = fs.readdirSync;',
    'fs.readdirSync = function (dir, ...rest) {',
    "  TRACE.push({ kind: 'readdir', path: String(dir) });",
    '  return realReaddir.call(fs, dir, ...rest);',
    '};',
    'const realReadFile = fs.readFileSync;',
    'fs.readFileSync = function (p, ...rest) {',
    "  TRACE.push({ kind: 'readfile', path: String(p) });",
    '  return realReadFile.call(fs, p, ...rest);',
    '};',
    'const realRename = fs.renameSync;',
    'fs.renameSync = function (from, to, ...rest) {',
    "  TRACE.push({ kind: 'rename', path: String(from) });",
    '  if (FAIL.has(String(from))) {',
    "    const e = new Error('P3_INJECTED_REMOVE_FAILURE');",
    "    e.code = 'EACCES';",
    '    throw e;',
    '  }',
    '  return realRename.call(fs, from, to, ...rest);',
    '};',
    'const realUnlink = fs.unlinkSync;',
    'fs.unlinkSync = function (p, ...rest) {',
    "  TRACE.push({ kind: 'unlink', path: String(p) });",
    '  return realUnlink.call(fs, p, ...rest);',
    '};',
  ].join('\n');
}

/**
 * 子进程前导：patch `node:fs` 的**三个写盘原语**，记录调用序列。
 *
 * ## 为什么只 patch 这三个
 *
 * 判据要断的是「**水位写入早于认领写入**」这个**次序** ⇒ 需要看到
 * 实现真实调了哪些写盘原语、什么顺序 ⇒ `writeFileSync` / `appendFileSync` /
 * `renameSync`（两步法删除里的「先改名」也算一次落盘动作）。
 *
 * @returns {string} 可拼在子进程脚本前面的 ESM 代码。
 */
export function writeTracePreamble() {
  return [
    "import { createRequire } from 'node:module';",
    "const require = createRequire('file:///' + process.cwd().replace(/\\\\/g, '/') + '/');",
    "const fs = require('node:fs');",
    'const TRACE = [];',
    'const realWrite = fs.writeFileSync;',
    'const realAppend = fs.appendFileSync;',
    'const realRename = fs.renameSync;',
    'fs.writeFileSync = function (p, d, o) {',
    '  TRACE.push({ kind: "write", path: String(p) });',
    '  return realWrite.call(fs, p, d, o);',
    '};',
    'fs.appendFileSync = function (p, d, o) {',
    '  TRACE.push({ kind: "append", path: String(p) });',
    '  return realAppend.call(fs, p, d, o);',
    '};',
    'fs.renameSync = function (f, t, o) {',
    '  TRACE.push({ kind: "rename", path: String(f) });',
    '  return realRename.call(fs, f, t, o);',
    '};',
  ].join('\n');
}

/**
 * 子进程前导：把「认领目录」补成**满 range**，并让认领写入**撞 EEXIST**
 * ⇒ 用来把分配器**驱动到上限**（§S-13 裁定 4④）。
 *
 * ## 为什么用「补目录 ＋ 撞 EEXIST」而不是真的造 9999 个文件
 *
 * ① **成本**：本机实测造 9999 个空文件要 **6.3s**（Windows）⇒ 太贵。
 * ② **形态无关**：水位记录的**文件名与格式由实现方定**（协议 §2.5）⇒ 判据
 *    **不能**去读它、也就**没法**把它推到 MAX_SEQ ⇒ 只能靠**认领侧**驱动上限：
 *    · `readdirSync(认领目录)` 返回**合成的满 range**（⇒ 「全都占着」）；
 *    · 认领写入一律抛 **EEXIST**（⇒ 走实现真实处理「这个序号已被占」的分支）。
 * ⇒ **基于认领目录**判上限的实现：**能判定**；
 *    **基于水位**判上限的实现：**也能判定**（它必须处理 EEXIST ⇒ 会一路往后试）。
 *
 * @param {object} options
 * @param {string} options.claimsDir - 认领目录（绝对路径）。
 * @param {number} options.maxSeq - 上限（认领文件名用 4 位补零）。
 * @returns {string} 可拼在子进程脚本前面的 ESM 代码。
 */
export function limitProbePreamble({ claimsDir, maxSeq }) {
  return [
    writeTracePreamble(),
    `const CLAIMS_DIR = ${JSON.stringify(claimsDir)};`,
    `const MAX_SEQ = ${Number(maxSeq)};`,
    // **range 在子进程里现生成**（不塞进命令行 ⇒ 不会撞 `ENAMETOOLONG`：
    // 9999 个文件名约 100KB，超过 Windows 的命令行上限）。
    'const FULL_RANGE = Array.from({ length: MAX_SEQ }, (_, i) => String(i + 1).padStart(4, "0") + ".json");',
    'const realReaddir = fs.readdirSync;',
    'fs.readdirSync = function (dir, o) {',
    '  if (String(dir) === CLAIMS_DIR) {',
    '    TRACE.push({ kind: "readdir", path: String(dir) });',
    '    return FULL_RANGE.slice();',
    '  }',
    '  return realReaddir.call(fs, dir, o);',
    '};',
    'fs.writeFileSync = function (p, d, o) {',
    '  TRACE.push({ kind: "write", path: String(p) });',
    '  if (String(p).startsWith(CLAIMS_DIR) && /^\\d{4}\\.json$/.test(String(p).split(/[\\\\/]/).pop())) {',
    "    const e = new Error('EEXIST');",
    "    e.code = 'EEXIST';",
    '    throw e;',
    '  }',
    '  return realWrite.call(fs, p, d, o);',
    '};',
  ].join('\n');
}

/**
 * 子进程前导：让**删除**这一步**真失败**（`renameSync` / `unlinkSync` 抛 EACCES）。
 *
 * ## 为什么用 patch 制造失败，而不是改实现
 *
 * 协议 §S-11 R1 的释放路径要**真的删掉**占用记录 ⇒ 要验「删不掉时判据不装绿」，
 * 就得造一个**真的失败**。按裁定，判据侧用 `node:fs` 的 patch 制造它
 * （**不要求实现加测试钩子、不改签名**）。
 *
 * ## 为什么 patch 在**子进程**里才生效
 *
 * `node:fs` 的 ESM facade 在首次 import 时**快照**了 CJS 导出值 ⇒ 同进程 patch
 * 不生效（同 `runInjectedCapture` 的理由）⇒ 释放命令**必须在子进程里调**。
 *
 * @param {object} options
 * @param {string} [options.failUnder] - 只让这个目录**下面**的删除失败（绝对路径）。
 * @returns {string} 可拼在子进程脚本前面的 ESM 代码。
 */
export function releaseDeleteFailPreamble({ failUnder } = {}) {
  return [
    "import { createRequire } from 'node:module';",
    "const require = createRequire('file:///' + process.cwd().replace(/\\\\/g, '/') + '/');",
    "const fs = require('node:fs');",
    `const FAIL_UNDER = ${JSON.stringify(failUnder ?? '')};`,
    'const realRename = fs.renameSync;',
    'const realUnlink = fs.unlinkSync;',
    'const deny = (p) => {',
    '  const e = new Error("EACCES: permission denied, injected");',
    "  e.code = 'EACCES';",
    '  throw e;',
    '};',
    'fs.renameSync = function (from, to, o) {',
    '  if (FAIL_UNDER && String(from).startsWith(FAIL_UNDER)) deny(from);',
    '  return realRename.call(fs, from, to, o);',
    '};',
    'fs.unlinkSync = function (p, o) {',
    '  if (FAIL_UNDER && String(p).startsWith(FAIL_UNDER)) deny(p);',
    '  return realUnlink.call(fs, p, o);',
    '};',
  ].join('\n');
}

/**
 * 「把 `0600` 当加密」的**肯定式**措辞（扫描实现代码，**不含协议稿**）。
 *
 * ## 为什么只扫实现代码、且只用肯定式
 *
 * 协议稿自己就写着「**不**把 `0600` 等同于完整跨平台加密」（§S-14 的裁定原文）
 * ⇒ 扫全仓会命中那些**否定句**而假红 ⇒ 只扫 `plugins/**`，
 * 且只认「**声称**加密」的说法（不含「不加密/未加密/不声称加密」这类否定式）。
 *
 * @returns {string[]} 命中的表述（`文件:行号：原文`）。
 */
export function scanEncryptionClaims() {
  const hits = [];
  const root = fileURLToPath(REPO_ROOT_URL);
  const dir = join(root, 'plugins', 'official-chat');
  const files = [];
  const walk = (current) => {
    for (const name of readdirSync(current, { withFileTypes: true })) {
      const full = join(current, name.name);
      if (name.isDirectory()) walk(full);
      else if (name.isFile() && name.name.endsWith('.js')) files.push(full);
    }
  };
  if (existsSync(dir)) walk(dir);
  // **肯定式**措辞：只认「已加密 / 加密存储 / 加密落盘」这类**声称**。
  const pattern = /(已加密|加密存储|加密落盘|密文存储|经加密)/;
  for (const file of files) {
    const lines = readFileSync(file, 'utf8').split('\n');
    for (const [index, line] of lines.entries()) {
      if (!pattern.test(line)) continue;
      // **否定式不算**（协议稿与代码注释里的「不加密/不声称加密」是**正确**表述）。
      if (/(不|未|非|别|禁止|不得|never|not)\s*[^，。；]{0,6}$/.test(line.slice(0, Math.max(0, line.indexOf(pattern.exec(line)[0]))))) {
        continue;
      }
      hits.push(`${file.replace(root, '')}:${index + 1}：${line.trim()}`);
    }
  }
  return hits;
}

/**
 * 造一个 **P3 判据适配器**。
 *
 * @param {object} [options]
 * @param {string} [options.home] - 落盘根。不给就用一次性临时目录。
 * @param {string} [options.tag] - 临时目录后缀。
 * @param {string} [options.hostDir] - host 半边目录（活体注入时指向**副本**）。
 * @param {string} [options.providerId] - provider 短码（默认 `ds`）。
 * @param {number} [options.now] - **冻结时钟**（毫秒）。不给就是实时。
 * @param {number} [options.retentionMs] - 快照保留期（默认 7 天）。
 * @param {number} [options.tmpResidualMs] - 临时残留阈值（默认 24 小时）。
 * @param {number} [options.occupancyRetentionMs] - 占用保留期（默认 7 天）。
 * @returns {Promise<object>} adapter。
 */
export async function createP3Adapter({
  home,
  tag = 'adapter',
  hostDir,
  providerId = 'ds',
  now,
  retentionMs = SNAPSHOT_RETENTION_MS,
  tmpResidualMs = TMP_RESIDUAL_MS,
  occupancyRetentionMs = OCCUPANCY_RETENTION_MS,
  /**
   * **注入**：让释放那一步的**删除真失败**（子进程里 patch `node:fs`）。
   *
   * 用途：自测的「活体注入」⇒ 证明判据**不是空跑**（实现删不掉时判据必须红）。
   * ⇒ 这是**判据侧**的注入，**不要求实现加测试钩子、不改签名**。
   */
  failReleaseDelete = false,
} = {}) {
  const ownsRoot = home === undefined;
  const root = home ?? makeTempRoot(tag);
  const hostRoot = hostDir === undefined ? HOST_DIR_URL : pathToFileURL(`${hostDir}/`);
  const hostModule = await import(new URL('index.js', hostRoot).href);
  const sweepModule = await import(new URL('sweep.js', hostRoot).href);
  const commandsModule = await import(new URL('commands.js', hostRoot).href);
  const seqModule = await import(new URL('seq-store.js', hostRoot).href);
  const protocolModule = await import(new URL('protocol.js', hostRoot).href);

  /** 注入的日志桩（观测「清理失败有留痕」）。 */
  const logLines = [];
  const logger = {
    warn: (message) => logLines.push(String(message)),
    error: (message) => logLines.push(String(message)),
    info: (message) => logLines.push(String(message)),
  };

  const env = { ...process.env, DSH_XLINK_OFFICIAL_CHAT_ROOT: root, DSH_HOME: root };

  /** 造一套真命令层（`store` 落盘、`sweeper` 清理、`seqAllocator` 认领）。 */
  function buildCommands(overrides = {}) {
    const store = hostModule.createSnapshotStore(env);
    const seqAllocator = seqModule.createSeqAllocator({ root, providerId, now: () => Date.now() });
    const catalogCache = commandsModule.createCatalogCache({
      rootDir: join(root, 'catalog-cache'),
      now: () => Date.now(),
    });
    // **固定夹具取数源**（实现侧自带的那个：`host/commands.js` 的
    // `createFixtureSource`）⇒ 判据不自己编一个来源，**用实现自己的那一套**
    // ⇒ 走的是真实链路（catalog → 分配 id → capture）。
    const captureSource = commandsModule.createFixtureSource([
      { sessionId: 'p3-session', title: 'P3 判据会话', body: 'P3 判据正文' },
      { sessionId: 'p3-session-2', title: 'P3 判据会话 2', body: 'P3 判据正文 2' },
    ]);
    return commandsModule.createCommands({
      store,
      seqAllocator,
      sweeper: sweepModule.createSweeper({
        root,
        providerId,
        now: now === undefined ? () => Date.now() : () => Number(now),
        logger,
        retentionMs,
      }),
      catalogCache,
      captureSource,
      providerId,
      accountScope: 'p3-server-derived-account',
      now: now === undefined ? () => Date.now() : () => Number(now),
      ...overrides,
    });
  }

  /**
   * 走**真实链路**拿一个可 capture 的快照 id（catalog → 分配 → 返回 id）。
   *
   * ## 为什么判据要走真实链路
   *
   * `capture` 要求 id 里的**会话段**与来源的会话 id 对得上、且那个 `seq`
   * **已被 host 认领**（`host/commands.js` 的两道校验）⇒ 判据**不能自造 id**
   * ⇒ 必须先跑一次 `catalog` 让 host 分配。
   *
   * @returns {Promise<string>} 可用的快照 id。
   */
  async function allocateSnapshotId(sessionId = 'p3-session') {
    const commands = buildCommands();
    const page = await commands.catalog({ requestId: 'p3-allocate', query: '' });
    const allocation = page?.allocations?.[sessionId];
    if (typeof allocation !== 'string' || allocation === '') {
      throw new Error(`catalog 没有为会话 ${sessionId} 分配出快照 id`);
    }
    return allocation;
  }

  const store = hostModule.createSnapshotStore(env);

  /**
   * 造一套**新的**序号分配器（**模拟「重启」**：不继承任何进程内状态，
   * 一切从盘上重新读）。
   *
   * @returns {object} 分配器（`reserve` / `isClaimed` / `usedSeqs`）。
   */
  function buildSeqAllocator() {
    return seqModule.createSeqAllocator({ root, providerId, now: () => Date.now() });
  }

  /**
   * 认领一个序号（走**真实分配器**）。
   *
   * @param {string} sessionId - 官网会话 id。
   * @returns {{seq: number, snapshotId: string}}
   */
  function allocateSeq(sessionId) {
    return buildSeqAllocator().reserve(sessionId);
  }

  /**
   * 某个会话的**认领记录**清单（解析后的；路径是**相对落盘根**的）。
   *
   * ## 为什么要按会话取而不是扫全树
   *
   * 不同会话的 `seq` 会重号（每个会话都从 1 起）⇒ 判据按 `seq` 找认领时，
   * 全树扫会**认错会话**的那一份 ⇒ 这里按**实现自己的会话目录口径**取。
   *
   * @param {string} sessionId - 官网会话 id。
   * @returns {Array<{path: string, providerShort: string, sessionShort: string, seq: number, claimedAt: string}>}
   */
  function claimsOf(sessionId) {
    const dir = claimsDirOf(sessionId);
    const out = [];
    if (!existsSync(dir)) return out;
    for (const name of readdirSync(dir)) {
      if (!name.endsWith('.json')) continue;
      const full = join(dir, name);
      let parsed = null;
      try {
        parsed = JSON.parse(readFileSync(full, 'utf8'));
      } catch {
        parsed = null;
      }
      out.push({
        path: full.slice(root.length + 1).split('\\').join('/'),
        providerShort: String(parsed?.providerShort ?? ''),
        sessionShort: String(parsed?.sessionShort ?? ''),
        seq: Number(parsed?.seq ?? Number(name.slice(0, -'.json'.length))),
        claimedAt: String(parsed?.claimedAt ?? ''),
      });
    }
    return out;
  }

  /**
   * 认领目录的**绝对路径**（`<root>/seq-claims/<providerShort>/<sessionShort>`）。
   *
   * @param {string} sessionId - 官网会话 id。
   * @returns {string}
   */
  function claimsDirOf(sessionId) {
    return join(
      root,
      'seq-claims',
      protocolModule.makeProviderShort(providerId),
      protocolModule.makeSessionShort(sessionId),
    );
  }

  return {
    /** 落盘根（本实例的 `official-chat` 根）。 */
    root,
    providerId,
    now,
    retentionMs,
    tmpResidualMs,
    occupancyRetentionMs,
    logger,
    /** 注入的日志行（观测「留痕」）。 */
    logLines,
    /** 本机是不是 Unix（`mode` 只在 Unix 侧有意义）。 */
    isUnix: process.platform !== 'win32',

    /** 真落盘 store（`save` / `load` / `listSessionSeqs`）。 */
    store,
    /** host 半边模块（`createSnapshotStore` / `instanceScope` 等）。 */
    hostModule,
    sweepModule,
    commandsModule,
    seqModule,

    /**
     * 造一套命令层。
     *
     * @param {object} [overrides]
     * @returns {object} commands。
     */
    buildCommands,
    /**
     * 走真实链路拿一个可 capture 的快照 id。
     *
     * @param {string} [sessionId]
     * @returns {Promise<string>}
     */
    allocateSnapshotId,
    /**
     * 造一套**新的**序号分配器（模拟「重启」）。
     *
     * @returns {object}
     */
    buildSeqAllocator,
    /**
     * 认领一个序号（真实分配器）。
     *
     * @param {string} sessionId
     * @returns {{seq: number, snapshotId: string}}
     */
    allocateSeq,
    /**
     * 某个会话的认领记录清单（相对路径）。
     *
     * @param {string} sessionId
     * @returns {Array<object>}
     */
    claimsOf,
    /**
     * 认领目录的绝对路径。
     *
     * @param {string} sessionId
     * @returns {string}
     */
    claimsDirOf,

    /**
     * 在**子进程**里跑一次 `reserve`，记录**写盘次序**（水位 vs 认领）。
     *
     * ## 为什么必须另起进程
     *
     * 同 `runInjectedCapture`：`node:fs` 的 ESM facade 在首次 import 时快照
     * CJS 导出值 ⇒ 同进程 patch 不生效。
     *
     * @param {string} sessionId - 官网会话 id。
     * @returns {Promise<{trace: Array<object>, seq: number|null, threw: boolean, outcome: string}>}
     */
    runReserveWithTrace: async (sessionId) => {
      const script = [
        writeTracePreamble(),
        `const { createSeqAllocator } = await import(${JSON.stringify(new URL('seq-store.js', hostRoot).href)});`,
        `const allocator = createSeqAllocator({ root: ${JSON.stringify(root)}, providerId: ${JSON.stringify(providerId)} });`,
        'let seq = null;',
        'let snapshotId = null;',
        'let threw = false;',
        'let outcome = null;',
        'let outcomeCode = "";',
        'let outcomeMessage = "";',
        'try {',
        `  const r = allocator.reserve(${JSON.stringify(sessionId)});`,
        '  seq = r.seq;',
        '  snapshotId = r.snapshotId;',
        '  outcome = "ok";',
        '} catch (error) {',
        '  threw = true;',
        '  outcomeCode = String(error?.code ?? "");',
        '  outcomeMessage = String(error?.message ?? error);',
        '  outcome = outcomeCode || outcomeMessage;',
        '}',
        'process.stdout.write(JSON.stringify({ trace: TRACE, seq, snapshotId, threw, outcome, outcomeCode, outcomeMessage }));',
      ].join('\n');
      const result = runInSubprocess(script);
      let parsed = null;
      try {
        parsed = JSON.parse(result.stdout.trim().split('\n').pop() ?? 'null');
      } catch {
        parsed = null;
      }
      if (parsed === null) {
        return {
          trace: [],
          seq: null,
          snapshotId: null,
          threw: true,
          outcome: `子进程异常：${result.stderr.slice(0, 200)}`,
          outcomeCode: '',
          outcomeMessage: '',
          broken: true,
        };
      }
      return {
        trace: Array.isArray(parsed.trace) ? parsed.trace : [],
        seq: parsed.seq ?? null,
        snapshotId: parsed.snapshotId ?? null,
        threw: parsed.threw === true,
        outcome: parsed.outcome ?? null,
        outcomeCode: parsed.outcomeCode ?? '',
        outcomeMessage: parsed.outcomeMessage ?? '',
        broken: false,
      };
    },

    /**
     * 在**子进程**里把分配器**驱动到上限**，看它报什么（§S-13 裁定 4④）。
     *
     * @param {string} sessionId - 官网会话 id。
     * @param {number} maxSeq - 上限（由判据按协议给 ⇒ adapter 不自带期望值）。
     * @returns {Promise<{seq: number|null, threw: boolean, outcomeCode: string, outcomeMessage: string, broken: boolean}>}
     */
    runAllocateAtLimit: async (sessionId, maxSeq) => {
      const claimsDir = claimsDirOf(sessionId);
      const script = [
        limitProbePreamble({ claimsDir, maxSeq }),
        `const { createSeqAllocator } = await import(${JSON.stringify(new URL('seq-store.js', hostRoot).href)});`,
        `const allocator = createSeqAllocator({ root: ${JSON.stringify(root)}, providerId: ${JSON.stringify(providerId)} });`,
        'let seq = null;',
        'let threw = false;',
        'let outcomeCode = "";',
        'let outcomeMessage = "";',
        'try {',
        `  const r = allocator.reserve(${JSON.stringify(sessionId)});`,
        '  seq = r.seq;',
        '} catch (error) {',
        '  threw = true;',
        '  outcomeCode = String(error?.code ?? "");',
        '  outcomeMessage = String(error?.message ?? error);',
        '}',
        'process.stdout.write(JSON.stringify({ seq, threw, outcomeCode, outcomeMessage }));',
      ].join('\n');
      const result = runInSubprocess(script);
      let parsed = null;
      try {
        parsed = JSON.parse(result.stdout.trim().split('\n').pop() ?? 'null');
      } catch {
        parsed = null;
      }
      if (parsed === null) {
        return {
          seq: null,
          threw: true,
          outcomeCode: '',
          outcomeMessage: `子进程异常：${result.stderr.slice(0, 200)}`,
          broken: true,
        };
      }
      return {
        seq: parsed.seq ?? null,
        threw: parsed.threw === true,
        outcomeCode: parsed.outcomeCode ?? '',
        outcomeMessage: parsed.outcomeMessage ?? '',
        broken: false,
      };
    },

    /** 快照目录里**最终名**的清单。 */
    finalNames: () => listFinalNames(root, providerId),
    /** 某个最终名是否存在。 */
    hasFinalName: (snapshotId) => existsSync(join(root, 'snapshots', providerId, `${snapshotId}.json`)),
    /** 占用记录是否存在。 */
    occupancyExists: (snapshotId) => occupancyRecordExists(root, snapshotId),
    /**
     * 调用 host 命令 **`release`**（协议 §S-11 R1 写死的释放入口）。
     *
     * ## 为什么在**子进程**里调
     *
     * ① 释放路径要能被**注入真失败**（`node:fs` patch：删除那一步抛 EACCES）
     *    ⇒ 必须在**能 patch 底层原语**的进程里跑（同进程 patch `node:fs` 不生效）。
     * ② 释放路径的观测面是**盘上状态**（占用记录还在不在）⇒ 父进程直接读盘即可。
     *
     * ## 调的是**哪一层**
     *
     * 调的是 **host 的命令层**（`host/commands.js` 的 `createCommands(...)` 返回的
     * 那个命令对象）——协议自己把「现成的 `cancel`」定位在
     * `host/commands.js:676-689`，⇒ 「host 命令」这一族说的是**这一层**。
     * ⚠️ **不是** rpc 的客户端端点表（`host/rpc.js` 的 `ENDPOINTS`）：那是
     * 「客户端命令」，协议 §S-11 R1 明写「**client 侧本批不接线**」。
     *
     * @param {object} options
     * @param {string} options.snapshotId - 要释放的快照 id。
     * @param {boolean} [options.failDelete] - 让删除那一步**真失败**（注入用）。
     * @returns {Promise<{available: boolean, alias: string|null, ok: boolean|null, released: unknown, errorCode: string, errorMessage: string, threw: boolean, broken: boolean}>}
     */
    callRelease: async ({ snapshotId, failDelete = false } = {}) => {
      const injectDeleteFailure = failDelete === true || failReleaseDelete === true;
      const script = [
        injectDeleteFailure ? releaseDeleteFailPreamble({ failUnder: join(root, 'refs') }) : '',
        `const { createCommands } = await import(${JSON.stringify(new URL('commands.js', hostRoot).href)});`,
        `const { createSnapshotStore } = await import(${JSON.stringify(new URL('index.js', hostRoot).href)});`,
        `const env = { ...process.env, DSH_XLINK_OFFICIAL_CHAT_ROOT: ${JSON.stringify(root)}, DSH_HOME: ${JSON.stringify(root)} };`,
        'const store = createSnapshotStore(env);',
        `const commands = createCommands({ store, providerId: ${JSON.stringify(providerId)}, now: () => ${Number(now ?? Date.now())} });`,
        // **协议写死的名字是 `release`**（§S-11 R1）⇒ 这里只认它。
        // 若实现只给了别名（如 `releaseOccupancy`）⇒ 如实报出来，让判据点名。
        'const available = typeof commands.release === "function";',
        'const alias = available ? null : (typeof commands.releaseOccupancy === "function" ? "releaseOccupancy" : null);',
        'let ok = null;',
        'let released = null;',
        'let errorCode = "";',
        'let errorMessage = "";',
        'let threw = false;',
        'if (available) {',
        '  try {',
        `    const r = commands.release({ snapshotId: ${JSON.stringify(snapshotId)} });`,
        '    ok = r?.ok === true;',
        '    released = r?.released;',
        '    errorCode = String(r?.error?.code ?? r?.code ?? "");',
        '    errorMessage = String(r?.error?.message ?? r?.detail ?? "");',
        '  } catch (error) {',
        '    threw = true;',
        '    errorCode = String(error?.code ?? "");',
        '    errorMessage = String(error?.message ?? error);',
        '  }',
        '}',
        'process.stdout.write(JSON.stringify({ available, alias, ok, released, errorCode, errorMessage, threw }));',
      ].join('\n');
      const result = runInSubprocess(script);
      let parsed = null;
      try {
        parsed = JSON.parse(result.stdout.trim().split('\n').pop() ?? 'null');
      } catch {
        parsed = null;
      }
      if (parsed === null) {
        return {
          available: false,
          alias: null,
          ok: null,
          released: null,
          errorCode: '',
          errorMessage: `子进程异常：${result.stderr.slice(0, 300)}`,
          threw: true,
          broken: true,
        };
      }
      return {
        available: parsed.available === true,
        alias: parsed.alias ?? null,
        ok: parsed.ok ?? null,
        released: parsed.released ?? null,
        errorCode: parsed.errorCode ?? '',
        errorMessage: parsed.errorMessage ?? '',
        threw: parsed.threw === true,
        broken: false,
      };
    },
    /** 读占用记录（读不动就是读不动）。 */
    readOccupancy: (snapshotId) => readOccupancyRecord(root, snapshotId),
    /** 读盘上某个文件（**读不出来就如实报错**，不吞）。 */
    readFile: (relativePath) => readFileSync(join(root, relativePath), 'utf8'),
    /** 读盘上某个文件的存在性（**相对路径**）。 */
    exists: (relativePath) => existsSync(join(root, relativePath)),
    /** 绝对路径的存在性（**认领文件、根外哨兵**用 —— 绝对路径不能拿 `join` 拼）。 */
    existsAbsolute: (absolutePath) => existsSync(absolutePath),
    /** 删掉绝对路径（只删判据自己造的）。 */
    removeAbsolute: (absolutePath) => rmSync(absolutePath, { force: true }),
    /** 某个路径的 mtime（毫秒）。 */
    mtimeMs: (relativePath) => statSync(join(root, relativePath)).mtimeMs,
    /** 落盘根下的绝对路径。 */
    pathOf: (relativePath) => join(root, relativePath),
    /** 目录清单（相对路径，**只看文件**）。 */
    listDir: (relativePath) => {
      const dir = join(root, relativePath);
      if (!existsSync(dir)) return [];
      return readdirSync(dir).sort();
    },
    /** 建目录（递归）。 */
    ensureDir: (relativePath) => mkdirSync(join(root, relativePath), { recursive: true }),
    /** 写盘上文件（**判据自己造的**，内容自己定）。 */
    writeFile: (relativePath, content) => {
      const full = join(root, relativePath);
      mkdirSync(join(full, '..'), { recursive: true });
      writeFileSync(full, String(content), 'utf8');
      return full;
    },
    /** 写绝对路径文件（根外哨兵用）。 */
    writeFileAbsolute: (absolutePath, content) => {
      writeFileSync(absolutePath, String(content), 'utf8');
      return absolutePath;
    },
    /**
     * 把某个文件的 mtime 改老（造「超过阈值」）。
     *
     * @param {string} relativePath
     * @param {number} ageMs - 距今多少毫秒算「老」。
     */
    setMtimeOld: (relativePath, ageMs) => {
      const full = join(root, relativePath);
      const when = new Date(Date.now() - Number(ageMs));
      utimesSync(full, when, when);
    },
    /** 某个路径的权限位（`statSync().mode`）。 */
    modeOf: (relativePath) => statSync(join(root, relativePath)).mode,
    /** 读仓库里的一个文件（相对仓库根；读不到返回 null）。 */
    readRepoFile: (relativePath) => {
      const full = join(fileURLToPath(REPO_ROOT_URL), relativePath);
      try {
        return readFileSync(full, 'utf8');
      } catch {
        return null;
      }
    },
    /**
     * 读 host 半边的一份**源码**（读的是**当前被测的那一份** —— 活体注入时是副本）。
     *
     * ## 为什么需要它
     *
     * Windows 上 `mode` 是 no-op ⇒ 盘上权限**断不了** ⇒ 判据改断**源码形态**
     * （写盘点有没有带权限参数）⇒ 读到的必须是**被测的那一份**源码
     * （读仓库原件会把注入副本的改动漏掉）。
     *
     * @param {string} fileName - 文件名（如 `commands.js`）。
     * @returns {string} 源码正文；读不到返回空串。
     */
    readHostFile: (fileName) => {
      try {
        return readFileSync(new URL(String(fileName), hostRoot), 'utf8');
      } catch {
        return '';
      }
    },
    /** 扫「把 0600 当加密」的肯定式措辞。 */
    scanEncryptionClaims: () => scanEncryptionClaims(),

    /**
     * 写一份快照（**判据自造**，内容按协议 §2 落）。
     *
     * @param {string} snapshotId
     * @param {object} [fields]
     */
    writeSnapshot: (snapshotId, fields = {}) => {
      const full = join(root, 'snapshots', providerId, `${snapshotId}.json`);
      mkdirSync(join(full, '..'), { recursive: true });
      const stored = {
        schemaVersion: 1,
        snapshotId,
        providerId,
        accountScope: 'p3-account',
        sessionId: 'p3-session',
        branch: 'main',
        messages: [{ text: 'P3 判据正文' }],
        contentDigest: '',
        capturedAt: '2026-10-21T00:00:00.000Z',
        ...fields,
      };
      writeFileSync(full, `${JSON.stringify(stored, null, 2)}\n`, { mode: 0o600 });
      return full;
    },
    /**
     * 写一份占用记录（**判据自造**，形状按协议 §2.4）。
     *
     * @param {string} snapshotId
     * @param {object|string} value - 对象则序列化；字符串则**原样**（造「写坏」）。
     */
    writeOccupancy: (snapshotId, value) => {
      const full = join(root, 'refs', `${snapshotId}.json`);
      mkdirSync(join(full, '..'), { recursive: true });
      const content = typeof value === 'string' ? value : `${JSON.stringify(value, null, 2)}\n`;
      writeFileSync(full, content, { mode: 0o600 });
      return full;
    },
    /**
     * 写一份**过期**的占用记录（协议 §S-11 R2 的夹具）。
     *
     * ## 为什么**三个时间信号都调旧**
     *
     * 实现判「过期」时可能看：`createdAt` → `capturedAt` → 文件 mtime
     * （业务时间优先、mtime 兜底）⇒ 只改一个信号 ⇒ 另一种口径的实现会以为
     * 「还没过期」⇒ 判据拿到**假红**。⇒ 三个信号一起调旧 ⇒ 任何一种口径都成立。
     *
     * @param {string} snapshotId
     * @param {number} ageMs - 记录老了多少毫秒（比保留期大即可）。
     * @returns {string} 绝对路径。
     */
    writeOccupancyAged: (snapshotId, ageMs) => {
      const iso = new Date(Date.now() - Number(ageMs)).toISOString();
      const full = join(root, 'refs', `${snapshotId}.json`);
      mkdirSync(join(full, '..'), { recursive: true });
      writeFileSync(
        full,
        `${JSON.stringify(
          { snapshotId, capturedAt: iso, createdAt: iso, sessionId: 'p3-session' },
          null,
          2,
        )}\n`,
        { mode: 0o600 },
      );
      const seconds = (Date.now() - Number(ageMs)) / 1000;
      utimesSync(full, seconds, seconds);
      return full;
    },
    /**
     * 写一份**过期**的快照（协议 §S-11 R2 的「随后可被回收」那一步的夹具）。
     *
     * @param {string} snapshotId
     * @param {number} ageMs - 快照老了多少毫秒。
     * @returns {string} 绝对路径。
     */
    writeSnapshotAged: (snapshotId, ageMs) => {
      const iso = new Date(Date.now() - Number(ageMs)).toISOString();
      const full = join(root, 'snapshots', providerId, `${snapshotId}.json`);
      mkdirSync(join(full, '..'), { recursive: true });
      writeFileSync(
        full,
        `${JSON.stringify(
          {
            schemaVersion: 1,
            snapshotId,
            providerId,
            accountScope: 'p3-account',
            sessionId: 'p3-session',
            branch: 'main',
            messages: [{ text: 'P3 判据正文' }],
            contentDigest: '',
            capturedAt: iso,
          },
          null,
          2,
        )}\n`,
        { mode: 0o600 },
      );
      const seconds = (Date.now() - Number(ageMs)) / 1000;
      utimesSync(full, seconds, seconds);
      return full;
    },
    /**
     * 造一份认领文件（**判据自造**，形状按 `host/seq-store.js` 的口径）。
     *
     * @param {string} snapshotId
     * @param {object} [options]
     * @param {boolean} [options.orphan] - 孤儿（有认领、无快照）。
     * @returns {string} 认领文件的**绝对路径**。
     */
    writeClaim: (snapshotId, { orphan = true } = {}) => {
      const parts = snapshotId.split('~');
      const providerShort = parts[0];
      const sessionShort = parts[1];
      const seq = parts[2];
      const full = join(root, 'seq-claims', providerShort, sessionShort, `${seq.padStart(4, '0')}.json`);
      mkdirSync(join(full, '..'), { recursive: true });
      writeFileSync(
        full,
        `${JSON.stringify(
          {
            providerShort,
            sessionShort,
            seq: Number(seq),
            claimedAt: '2026-01-01T00:00:00.000Z',
          },
          null,
          2,
        )}\n`,
        { mode: 0o600 },
      );
      void orphan;
      return full;
    },

    /**
     * 跑一次清理并**观测它真实的调用序列**（子进程内 patch `node:fs`）。
     *
     * ## 返回的 trace 是什么
     *
     * `{kind:'readdir'|'readfile'|'rename'|'unlink', path}` 的**调用序列** ⇒
     * 判据据此断「有没有扫描占用记录」「扫描有没有发生在删除之前」
     * （**不断实现形态**：实现用 `rename` 还是 `unlink` 删除都算「删除」）。
     *
     * ## 注入删除失败
     *
     * `failRemove` 里的**绝对路径** ⇒ 该路径的 `renameSync` 抛 `EACCES`
     * ⇒ 用来验「清理失败**只记日志**、**不阻塞整轮**」。
     *
     * @param {object} [options]
     * @param {string[]} [options.failRemove] - 要注入删除失败的绝对路径。
     * @returns {Promise<{trace: Array<object>, logs: string[], threw: boolean, outcome: unknown}>}
     */
    runSweep: async ({ failRemove = [] } = {}) => {
      const script = [
        sweepProbePreamble({ failRemove }),
        `const { createSweeper } = await import(${JSON.stringify(new URL('sweep.js', hostRoot).href)});`,
        'const sweeper = createSweeper({',
        `  root: ${JSON.stringify(root)},`,
        `  providerId: ${JSON.stringify(providerId)},`,
        `  now: () => ${Number(now ?? Date.now())},`,
        `  retentionMs: ${Number(retentionMs)},`,
        '  logger: {',
        '    warn: (m) => LOGS.push(String(m)),',
        '    error: (m) => LOGS.push(String(m)),',
        '    info: (m) => LOGS.push(String(m)),',
        '  },',
        '});',
        'let threw = false;',
        'let outcome = null;',
        'try {',
        '  outcome = sweeper.sweep();',
        '} catch (error) {',
        '  threw = true;',
        '  outcome = String(error?.message ?? error);',
        '}',
        'process.stdout.write(JSON.stringify({ trace: TRACE, logs: LOGS, threw, outcome }));',
      ].join('\n');
      const result = runInSubprocess(script);
      let parsed = null;
      try {
        parsed = JSON.parse(result.stdout.trim().split('\n').pop() ?? 'null');
      } catch {
        parsed = null;
      }
      if (parsed === null) {
        return {
          trace: [],
          logs: [],
          threw: true,
          outcome: `子进程异常：${result.stderr.slice(0, 300)}`,
          broken: true,
        };
      }
      logLines.push(...(parsed.logs ?? []));
      return {
        trace: Array.isArray(parsed.trace) ? parsed.trace : [],
        logs: Array.isArray(parsed.logs) ? parsed.logs : [],
        threw: parsed.threw === true,
        outcome: parsed.outcome ?? null,
        broken: false,
      };
    },

    /**
     * 在**子进程**里造「写到一半失败」并观察盘上结果。
     *
     * ## 为什么必须在子进程
     *
     * `node:fs` 的 ESM facade 在首次被任何 ESM 模块 import 时就快照了 CJS 导出值
     * ⇒ 本模块（以及 `p1-adapter.mjs`）静态 import 了 `node:fs`
     * ⇒ 同进程 patch **不生效** ⇒ 必须另起进程（`runInSubprocess`）。
     *
     * @param {object} options
     * @param {'throw-before'|'throw-mid'} options.mode - 注入形态。
     * @param {string} options.snapshotId - 要落的快照 id。
     * @param {boolean} [options.corrupt] - **写坏内容**（P3-2 用）。
     * @returns {Promise<{threw: boolean, outcome: string, files: string[]}>}
     */
    runInjectedCapture: async ({ mode, snapshotId, corrupt = false }) => {
      const script = [
        fsPatchPreamble({
          root,
          mode: corrupt ? 'throw-mid' : mode,
          keepBytes: corrupt ? 40 : 20,
        }),
        `const { createSnapshotStore } = await import(${JSON.stringify(new URL('index.js', hostRoot).href)});`,
        'const store = createSnapshotStore({ DSH_XLINK_OFFICIAL_CHAT_ROOT: process.env.DSH_XLINK_OFFICIAL_CHAT_ROOT });',
        'let outcome = "no-throw";',
        'try {',
        '  store.save({',
        `    snapshotId: ${JSON.stringify(snapshotId)},`,
        `    providerId: ${JSON.stringify(providerId)},`,
        "    sessionId: 'p3-session',",
        "    branch: 'main',",
        "    accountScope: 'p3-account',",
        "    capturedAt: '2026-10-21T00:00:00.000Z',",
        `    messages: [{ text: ${JSON.stringify(corrupt ? 'P3 被写坏的正文' : 'P3 判据正文')} }],`,
        '  });',
        '} catch (error) {',
        '  outcome = String(error?.code ?? error?.message ?? error);',
        '}',
        'process.stdout.write(JSON.stringify({ outcome, calls: CALLS }));',
      ].join('\n');
      const result = runInSubprocess(script);
      let parsed = null;
      try {
        parsed = JSON.parse(result.stdout.trim().split('\n').pop() ?? 'null');
      } catch {
        parsed = null;
      }
      const threw = parsed !== null && parsed.outcome !== 'no-throw';
      return {
        threw,
        outcome: parsed?.outcome ?? `子进程异常：${result.stderr.slice(0, 200)}`,
        files: Array.isArray(parsed?.calls) ? parsed.calls : [],
      };
    },

    /** 收尾：删掉一次性临时目录。 */
    cleanup() {
      if (ownsRoot) removeTempRoot(root);
    },
  };
}
