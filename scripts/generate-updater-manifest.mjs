import {
  readFileSync,
  readdirSync,
  writeFileSync,
} from 'node:fs';
import { resolve, join, basename } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

export function releaseNotes(version) {
  return `## dsh-xlink v${version}

支持平台：
- Intel macOS（.dmg）
- Windows x86_64（NSIS 安装包 .exe）

使用方式：安装后在「内核版本」页安装并切换 DeepSeek Harness 内核版本即可使用。`;
}

function walkFiles(root) {
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
  const files = walkFiles(resolve(artifactsDir));
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
