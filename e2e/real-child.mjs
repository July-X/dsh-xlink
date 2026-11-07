import { realpathSync } from 'node:fs';
import { isAbsolute, relative, sep, toNamespacedPath } from 'node:path';

// Windows realpath preserves the caller's \\?\ prefix; normalize both sides.
export function isRealChild(parent, child) {
  const root = realpathSync(parent);
  const target = realpathSync(child);
  const normalize = value => process.platform === 'win32' ? toNamespacedPath(value) : value;
  const rel = relative(normalize(root), normalize(target));
  return !!rel && rel !== '..' && !rel.startsWith(`..${sep}`) && !isAbsolute(rel);
}
