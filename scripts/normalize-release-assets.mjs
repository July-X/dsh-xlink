import {
  readdirSync,
  renameSync,
} from 'node:fs';
import { resolve, join, basename } from 'node:path';
import { pathToFileURL } from 'node:url';

function matchingFiles(directory, suffix) {
  return readdirSync(directory, { withFileTypes: true })
    .filter((entry) => entry.isFile() && entry.name.endsWith(suffix))
    .map((entry) => join(directory, entry.name));
}

function exactlyOne(directory, suffix, label) {
  const matches = matchingFiles(directory, suffix);
  if (matches.length !== 1) {
    throw new Error(
      `${label} must contain exactly one file, found ${matches.length}: ${matches
        .map((path) => basename(path))
        .join(', ')}`,
    );
  }
  return matches[0];
}

export function normalizeMacOSUpdaterAssets(bundleDir, version) {
  if (!version) {
    throw new Error('release version must not be empty');
  }

  const root = resolve(bundleDir);
  const dmg = exactlyOne(join(root, 'dmg'), '.dmg', 'macOS DMG');
  const macosDir = join(root, 'macos');
  const archive = exactlyOne(macosDir, '.app.tar.gz', 'macOS updater archive');
  const signature = exactlyOne(
    macosDir,
    '.app.tar.gz.sig',
    'macOS updater signature',
  );
  const stem = basename(dmg, '.dmg');

  if (!stem.includes(version)) {
    throw new Error(
      `macOS DMG does not contain release version ${version}: ${basename(dmg)}`,
    );
  }

  const versionedArchive = join(macosDir, `${stem}.app.tar.gz`);
  const versionedSignature = `${versionedArchive}.sig`;
  if (archive !== versionedArchive) {
    renameSync(archive, versionedArchive);
  }
  if (signature !== versionedSignature) {
    renameSync(signature, versionedSignature);
  }

  return {
    archive: versionedArchive,
    signature: versionedSignature,
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

function main() {
  const options = parseArgs(process.argv.slice(2));
  const bundleDir = required(options, 'bundle-dir');
  const version = required(options, 'version');
  const assets = normalizeMacOSUpdaterAssets(bundleDir, version);
  console.log(
    `normalized ${basename(assets.archive)} and ${basename(assets.signature)}`,
  );
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
