// 内置插件资源管线：从 plugins/<id>/ 产出随应用打包的资源目录
// src-tauri/resources/builtin-plugins/<id>/。
//
// 产物目录不进 git（dev plan §11：内嵌产物生成目录不得作为普通源码提交），
// 由构建生成并收进安装包（tauri.conf.json 的 `resources` 映射**整目录**，
// 所以加第二个内置插件时不用改 tauri.conf.json —— 那条「新增目录要先确认
// 它在全新 checkout 里存在」的纪律在这里仍然成立，但它的前置是 prep 生成）。
//
// 两个模式：
//   node scripts/prepare-builtin-plugins.mjs           # 生成 / 重新生成
//   node scripts/prepare-builtin-plugins.mjs --check    # 校验（npm run check 用）
//
// --check 失败即 exit 1，逐插件校验三条——① 资源目录里每个文件的 sha256 与
// manifest 一致（完整性）；② manifest 记录的源摘要与当前 plugins/ 源码一致
// （新鲜度：源码改了没重新生成，装进实例的就是旧插件）；③ 资源目录下出现的
// 每个子目录都在登记表里（孤儿：把一个内置插件从登记表删掉却没重新生成，
// 打包进去的仍然是它，而界面上已经没有它的开关了）。
// 产物不存在时同样 exit 1 并说清前置命令——静默跳过等于假门禁。
import { createHash } from 'node:crypto';
import { cpSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const outputBase = join(root, 'src-tauri', 'resources', 'builtin-plugins');
const checkMode = process.argv.includes('--check');

/**
 * 内置插件登记表：每个内置插件一份。
 *
 * `entries` 是**运行时文件集**（进安装包、进实例的那部分），与
 * `src-tauri/src/plugins/builtin/registry.rs` 的 `BuiltinSpec::entries` 一一
 * 对应。两份各管一门语言，改一边必须改另一边——`--check` 的新鲜度比对
 * （资源清单 vs 当前源码）只能证明「产物与本文件一致」，证明不了「本文件
 * 与 Rust 那份一致」，所以那份一致性由 `scripts/dev-builtin.test.mjs` 钉。
 *
 * 为什么按插件各列各的：文件集随插件而变（ponytail 没有工作台卡片、也不带
 * 本地化文案），写成一份共用清单就得为不存在的目录编造条目，而物化时找不到
 * 目录会报「应用资源可能不完整，请重新安装」——把源码侧的错误报成安装包坏了。
 */
const BUILTINS = [
  {
    id: 'openai-oauth',
    // host/ + 工作台账户卡 + 中英文案 + 清单。test/ 与 README 不进实例。
    entries: ['host', 'client', 'locales', 'package.json'],
  },
  {
    id: 'ponytail',
    // **只有适配层**（host + adapter + i18n）与许可：上游的技能正文与 hook 脚本**不随包
    // 走**（2026-10-12 改）。它们由壳在启用时从上游拉取并物化进实例——这样上游
    // 改一次不必重发一版 dsh-xlink，也不会出现「仓库里那份与用户手上的那份不是
    // 同一个版本」。
    //
    // `i18n` 是中文覆盖层（上游只有英文版），它依据哪一版上游由
    // `i18n/zh-CN/manifest.json` 逐文件记着 sha256；壳物化它，host 启动时按指纹
    // 覆盖物化目录里那六个 SKILL.md，上游一改就对不上而回落英文。
    entries: ['host', 'adapter', 'i18n', 'package.json', 'LICENSE'],
  },
];

function fail(message) {
  console.error(`[builtin-plugins] ${message}`);
  process.exit(1);
}

function sha256(path) {
  return createHash('sha256').update(readFileSync(path)).digest('hex');
}

/** 递归收集目录内全部普通文件的相对路径（POSIX 分隔）。 */
function walkFiles(dir, base = dir) {
  const out = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) out.push(...walkFiles(full, base));
    else if (entry.isFile()) out.push(relative(base, full).split('\\').join('/'));
  }
  return out.sort();
}

function sourceDirOf(builtin) {
  return join(root, 'plugins', builtin.id);
}

function buildManifest(builtin) {
  const sourceDir = sourceDirOf(builtin);
  const pkg = JSON.parse(readFileSync(join(sourceDir, 'package.json'), 'utf8'));
  const files = [];
  for (const entry of builtin.entries) {
    const from = join(sourceDir, entry);
    if (!existsSync(from)) fail(`插件 ${builtin.id} 源码缺 ${entry}（${from}）；资源管线中止`);
    if (statSync(from).isDirectory()) {
      for (const rel of walkFiles(from)) {
        const path = `${entry}/${rel}`;
        files.push({ path, sha256: sha256(join(sourceDir, path)), bytes: statSync(join(sourceDir, path)).size });
      }
    } else {
      files.push({ path: entry, sha256: sha256(from), bytes: statSync(from).size });
    }
  }
  return { pluginVersion: pkg.version, files };
}

function checkBuiltin(builtin) {
  const outputRoot = join(outputBase, builtin.id);
  const manifestPath = join(outputRoot, 'manifest.json');
  if (!existsSync(manifestPath)) {
    fail(`资源清单不存在（${manifestPath}）；先执行 npm run prep:builtin`);
  }
  const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
  if (!Array.isArray(manifest.files) || manifest.files.length === 0) {
    fail(`资源清单没有文件条目（${manifestPath}）；重新执行 npm run prep:builtin`);
  }
  for (const file of manifest.files) {
    const target = join(outputRoot, file.path);
    if (!existsSync(target)) fail(`资源 ${file.path} 缺失；重新执行 npm run prep:builtin`);
    const digest = sha256(target);
    if (digest !== file.sha256) {
      fail(`资源 ${file.path} 与清单摘要不一致（有 ${digest}，清单记 ${file.sha256}）；重新执行 npm run prep:builtin`);
    }
  }
  // 新鲜度：清单必须等于「现在从源码重算的那份」。源码改了没重新生成时，
  // 打包进安装包的是旧插件，而界面版本号还是新的——这种失配必须在这里红。
  const expected = buildManifest(builtin);
  if (JSON.stringify(expected) !== JSON.stringify(manifest)) {
    fail(`资源清单与当前源码不一致（${builtin.id} 的源码已变更，未重新生成）；执行 npm run prep:builtin 后重试`);
  }
  return manifest.files.length;
}

if (checkMode) {
  if (!existsSync(outputBase)) {
    fail(`资源目录不存在（${outputBase}）；先执行 npm run prep:builtin`);
  }
  // 登记表里的每一个都要有产物，**产物里的每一个都要在登记表里**：只查前半
  // 的话，把一个内置插件删掉却忘了重新生成，打包进去的仍然是它。
  const known = new Set(BUILTINS.map((b) => b.id));
  for (const entry of readdirSync(outputBase, { withFileTypes: true })) {
    if (entry.isDirectory() && !known.has(entry.name)) {
      fail(`资源目录里有未登记的内置插件 ${entry.name}；执行 npm run prep:builtin 重新生成（或确认删除后重跑）`);
    }
  }
  let total = 0;
  for (const builtin of BUILTINS) total += checkBuiltin(builtin);
  console.log(`builtin-plugins 资源校验通过（${BUILTINS.length} 个插件，${total} 个文件）`);
} else {
  // 全量重建而不是增量拷贝：孤儿文件（源码里删掉的 host/xxx.js）留在资源
  // 目录里会被 manifest 的完整性校验放过——清单只校验「列出的都在」。
  rmSync(outputBase, { recursive: true, force: true });
  for (const builtin of BUILTINS) {
    const outputRoot = join(outputBase, builtin.id);
    const sourceDir = sourceDirOf(builtin);
    for (const entry of builtin.entries) {
      const from = join(sourceDir, entry);
      if (!existsSync(from)) fail(`插件 ${builtin.id} 源码缺 ${entry}（${from}）；资源管线中止`);
      if (statSync(from).isDirectory()) {
        mkdirSync(join(outputRoot, entry), { recursive: true });
        cpSync(from, join(outputRoot, entry), { recursive: true });
      } else {
        mkdirSync(outputRoot, { recursive: true });
        cpSync(from, join(outputRoot, entry));
      }
    }
    const manifest = buildManifest(builtin);
    writeFileSync(join(outputRoot, 'manifest.json'), `${JSON.stringify(manifest, null, 2)}\n`);
    console.log(`  ${builtin.id}: ${manifest.files.length} 个文件（v${manifest.pluginVersion}）`);
  }
  console.log(`builtin-plugins 资源已生成（${BUILTINS.length} 个插件）`);
}