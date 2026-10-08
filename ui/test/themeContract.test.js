import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const source = readFileSync(new URL('../src/shell/theme.js', import.meta.url), 'utf8');
const code = source
  .replace(/\/\*[\s\S]*?\*\//g, '')
  .replace(/\/\/.*$/gm, '');

test('主题只用 html.dark 作为暗色判据', () => {
  assert.match(
    code,
    /document\.documentElement\.classList\.toggle\('dark',\s*theme\.value === 'dark'\)/,
    '主题切换必须落到 html.dark'
  );
  assert.doesNotMatch(
    code,
    /document\.documentElement\.dataset|data-theme/,
    '不能再写入第二套 data-theme 判据'
  );
});
