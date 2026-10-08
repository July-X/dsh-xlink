// 内嵌插件资源管线（P1）：从 plugins/openai-oauth/ 产出随应用打包的
// 资源目录 src-tauri/resources/builtin-plugins/openai-oauth/。
//
// 产物目录不进 git（dev plan §11：内嵌产物生成目录不得作为普通源码提交），
// 由构建生成并收进安装包（tauri.conf.json 的 resources 映射）。
//
// 两个模式：
//   node scripts/prepare-builtin-plugins.mjs           # 生成 / 重新生成
//   node scripts/prepare-builtin-plugins.mjs --check    # 校验（npm run check 用）
//
// --check 失败即 exit 1：校验两条——① 资源目录里每个文件的 sha256 与
// manifest 一致（完整性）；② manifest 记录的源摘要与当前 plugins/ 源码
// 一致（新鲜度：源码改了没重新生成，装进实例的就是旧插件）。
// 产物不存在时同样 exit 1 并说清前置命令——静默跳过等于假门禁。
import { createHash } from 'node:crypto';
import { cpSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const sourceDir = join(root, 'plugins', 'openai-oauth');
const outputRoot = join(root, 'src-tauri', 'resources', 'builtin-plugins', 'openai-oauth');
const checkMode = process.argv.includes('--check');

// 运行时文件集：与 src-tauri/src/plugins/builtin/materialize.rs 的
// RUNTIME_ENTRIES 一一对应（两份清单各管一门语言，改一边必须改另一边；
// test/ 与 README 不进实例，也不进安装包）。
const RUNTIME_ENTRIES = ['host', 'client', 'locales', 'package.json'];

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

function buildManifest() {
  const pkg = JSON.parse(readFileSync(join(sourceDir, 'package.json'), 'utf8'));
  const files = [];
  for (const entry of RUNTIME_ENTRIES) {
    const from = join(sourceDir, entry);
    if (!existsSync(from)) fail(`插件源码缺 ${entry}（${from}）；资源管线中止`);
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

if (checkMode) {
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
    if (!existsSync(target)) fail(`资源缺文件 ${file.path}；重新执行 npm run prep:builtin`);
    const digest = sha256(target);
    if (digest !== file.sha256) {
      fail(`资源 ${file.path} 与清单摘要不一致（有 ${digest}，清单记 ${file.sha256}）；重新执行 npm run prep:builtin`);
    }
  }
  // 新鲜度：清单必须等于「现在从源码重算的那份」。源码改了没重新生成时，
  // 打包进安装包的是旧插件，而界面版本号还是新的——这种失配必须在这里红。
  const expected = buildManifest();
  if (JSON.stringify(expected) !== JSON.stringify(manifest)) {
    fail('资源清单与当前源码不一致（源码已变更，未重新生成）；执行 npm run prep:builtin 后重试');
  }
  console.log(`builtin-plugins 资源校验通过（${manifest.files.length} 个文件，v${manifest.pluginVersion}）`);
} else {
  // 全量重建而不是增量拷贝：孤儿文件（源码里删掉的 host/xxx.js）留在资源
  // 目录里会被 manifest 的完整性校验放过——清单只校验「列出的都在」。
  rmSync(outputRoot, { recursive: true, force: true });
  for (const entry of RUNTIME_ENTRIES) {
    const from = join(sourceDir, entry);
    if (!existsSync(from)) fail(`插件源码缺 ${entry}（${from}）；资源管线中止`);
    if (statSync(from).isDirectory()) {
      mkdirSync(join(outputRoot, entry), { recursive: true });
      cpSync(from, join(outputRoot, entry), { recursive: true });
    } else {
      mkdirSync(outputRoot, { recursive: true });
      cpSync(from, join(outputRoot, entry));
    }
  }
  const manifest = buildManifest();
  writeFileSync(join(outputRoot, 'manifest.json'), `${JSON.stringify(manifest, null, 2)}\n`);
  console.log(`builtin-plugins 资源已生成（${manifest.files.length} 个文件，v${manifest.pluginVersion}）`);
}
