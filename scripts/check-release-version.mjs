import { readFile } from 'node:fs/promises';
import { pathToFileURL } from 'node:url';

function parseVersion(value, label) {
  const match = typeof value === 'string' && /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$/.exec(value);
  const prerelease = match && match[4]?.split('.');
  if (!match || match[0] !== value || prerelease?.some((part) => /^0\d+$/.test(part))) {
    throw new Error(label + '不是合法的 SemVer：' + JSON.stringify(value));
  }
  return { core: match.slice(1, 4).map(BigInt), prerelease };
}

function compare(a, b) {
  for (let i = 0; i < 3; i += 1) {
    if (a.core[i] !== b.core[i]) return a.core[i] < b.core[i] ? -1 : 1;
  }
  const x = a.prerelease;
  const y = b.prerelease;
  if (!x || !y) return x ? -1 : y ? 1 : 0;
  for (let i = 0; i < Math.max(x.length, y.length); i += 1) {
    if (x[i] === undefined) return -1;
    if (y[i] === undefined) return 1;
    if (x[i] === y[i]) continue;
    const nx = /^\d+$/.test(x[i]);
    const ny = /^\d+$/.test(y[i]);
    if (nx && ny) return BigInt(x[i]) < BigInt(y[i]) ? -1 : 1;
    if (nx !== ny) return nx ? -1 : 1;
    return x[i] < y[i] ? -1 : 1;
  }
  return 0;
}

export function assertReleaseVersionAdvances(candidate, published) {
  const next = parseVersion(candidate, '待发布版本');
  const current = parseVersion(published, '线上版本');
  if (compare(next, current) <= 0) {
    throw new Error('版本单调性检查失败：待发布 ' + candidate + ' 不大于线上最新 ' + published
      + '。releases/latest 取最近创建的正式 Release，版本回退会让用户收不到更新提示。');
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    const args = process.argv.slice(2);
    if (args.length !== 2) {
      throw new Error('必须提供待发版本和线上清单文件；仅检查格式时使用 --format-only <version>');
    }
    const formatOnly = args[0] === '--format-only';
    const candidate = formatOnly ? args[1] : args[0];
    parseVersion(candidate, '待发布版本');
    if (formatOnly) {
      console.log('待发布版本格式检查通过：' + candidate);
    } else {
      const manifest = JSON.parse(await readFile(args[1], 'utf8'));
      assertReleaseVersionAdvances(candidate, manifest?.version);
      console.log('版本单调性检查通过：' + manifest.version + ' → ' + candidate);
    }
  } catch (error) {
    console.error('发布版本检查失败：' + error.message + '；请检查版本文件及线上更新清单后重试。');
    process.exitCode = 1;
  }
}
