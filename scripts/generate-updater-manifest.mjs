import {
  readFileSync,
  readdirSync,
  writeFileSync,
} from 'node:fs';
import { resolve, join, basename } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

// 固定模板：同时写进 GitHub Release 正文与 latest.json 的 notes。
// 「从旧版本升级」须知是长期有效的——删掉它等于让极旧版本用户升级后静默从零配置；
// 逐版变更请写发版 commit 正文，不要往这里加。
export function releaseNotes(version) {
  return `## dsh-xlink v${version}

支持平台：
- Intel macOS（.dmg）
- Windows x86_64（NSIS 安装包 .exe）

使用方式：安装后在「内核版本」页安装并切换 DeepSeek Harness 内核版本即可使用。

从旧版本升级：
- 从 ≤0.1.2（dsh-desktop）升级：已装内核清单、活动版本、端口与外壳设置存放在系统应用数据目录（macOS 的 ~/Library/Application Support/com.zhongxingxing.dsh-desktop，Windows 的 %APPDATA% 下同名目录），当前版本不再读取，旧目录原样保留。不需要卸载重装本应用；升级后请重新安装一次内核版本并核对端口 / profile。内核自己的数据（会话、凭据、模型配置）会从 ~/.dsh 自动并入，不会丢。
- 旧方式安装的内核版本（「内核版本」页带「共享存储」标记）：卸载后重新安装一次即可隔离，标记消失。
- 升级前壳异常退出（崩溃 / 强退）：旧内核进程可能仍在运行。壳不会结束身份无法确认的进程；若工作台启动提示端口被占用，请在活动监视器 / 任务管理器中结束它再重试。`;
}

export function walkFiles(root) {
  const files = [];
  const visit = (directory) => {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const path = join(directory, entry.name);
      if (entry.isDirectory()) {
        visit(path);
      } else if (entry.isFile()) {
        files.push(path);
      }
    }
  };
  visit(root);
  return files;
}

function exactlyOne(files, predicate, label) {
  const matches = files.filter((path) => predicate(basename(path)));
  if (matches.length !== 1) {
    throw new Error(
      `${label} must contain exactly one file, found ${matches.length}: ${matches
        .map((path) => basename(path))
        .join(', ')}`,
    );
  }
  return matches[0];
}

export function collectReleaseAssets(artifactsDir, version) {
  return selectReleaseAssets(walkFiles(resolve(artifactsDir)), version);
}

export function selectReleaseAssets(files, version) {
  const assets = {
    darwinDmg: exactlyOne(files, (name) => name.endsWith('.dmg'), 'macOS DMG'),
    darwinUpdater: exactlyOne(
      files,
      (name) => name.endsWith('.app.tar.gz'),
      'macOS updater archive',
    ),
    darwinSignature: exactlyOne(
      files,
      (name) => name.endsWith('.app.tar.gz.sig'),
      'macOS updater signature',
    ),
    windowsInstaller: exactlyOne(
      files,
      (name) => name.endsWith('-setup.exe'),
      'Windows installer',
    ),
    windowsSignature: exactlyOne(
      files,
      (name) => name.endsWith('-setup.exe.sig'),
      'Windows installer signature',
    ),
  };

  for (const path of Object.values(assets)) {
    if (!basename(path).includes(version)) {
      throw new Error(`release asset does not contain version ${version}: ${basename(path)}`);
    }
  }
  return assets;
}

function signature(path) {
  const value = readFileSync(path, 'utf8').trim();
  if (!value) {
    throw new Error(`updater signature is empty: ${path}`);
  }
  return value;
}

function downloadUrl(repository, tag, path) {
  return `https://github.com/${repository}/releases/download/${encodeURIComponent(tag)}/${encodeURIComponent(
    basename(path),
  )}`;
}

export function buildReleaseMetadata({
  version,
  tag,
  repository,
  assets,
  pubDate = new Date().toISOString(),
}) {
  const darwin = {
    signature: signature(assets.darwinSignature),
    url: downloadUrl(repository, tag, assets.darwinUpdater),
  };
  const windows = {
    signature: signature(assets.windowsSignature),
    url: downloadUrl(repository, tag, assets.windowsInstaller),
  };
  return {
    version,
    notes: releaseNotes(version),
    pub_date: pubDate,
    platforms: {
      // Tauri emits both the generic architecture key and the bundle-specific
      // key. Keep both aliases so updater clients can match the same artifact.
      'darwin-x86_64': darwin,
      'darwin-x86_64-app': darwin,
      'windows-x86_64': windows,
      'windows-x86_64-nsis': windows,
    },
  };
}

function required(options, name) {
  const value = options[name];
  if (!value) {
    throw new Error(`missing required option --${name}`);
  }
  return value;
}

function parseArgs(argv) {
  const options = {};
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (!argument.startsWith('--')) {
      throw new Error(`unexpected argument: ${argument}`);
    }
    const name = argument.slice(2);
    const value = argv[index + 1];
    if (!value || value.startsWith('--')) {
      throw new Error(`missing value for --${name}`);
    }
    options[name] = value;
    index += 1;
  }
  return options;
}

export function writeReleaseFiles({
  artifactsDir,
  version,
  tag,
  repository,
  manifestPath,
  notesPath,
  pubDate,
}) {
  const assets = collectReleaseAssets(artifactsDir, version);
  const metadata = buildReleaseMetadata({
    version,
    tag,
    repository,
    assets,
    pubDate,
  });
  writeFileSync(manifestPath, `${JSON.stringify(metadata, null, 2)}\n`);
  writeFileSync(notesPath, `${metadata.notes}\n`);
  return assets;
}

function main() {
  const options = parseArgs(process.argv.slice(2));
  const version = required(options, 'version');
  const tag = required(options, 'tag');
  const repository = required(options, 'repository');
  const artifactsDir = required(options, 'artifacts-dir');
  const manifestPath = resolve(required(options, 'manifest'));
  const notesPath = resolve(required(options, 'notes'));
  const assets = writeReleaseFiles({
    artifactsDir,
    version,
    tag,
    repository,
    manifestPath,
    notesPath,
  });
  console.log(`prepared ${Object.values(assets).map((path) => basename(path)).join(', ')}`);
}

const invokedPath = process.argv[1] && pathToFileURL(resolve(process.argv[1])).href;
if (invokedPath === import.meta.url) {
  try {
    main();
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 1;
  }
}
