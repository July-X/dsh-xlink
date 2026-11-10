import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

// Kami 合同：docs/ui/kami-implementation-2026-11/tokens.json 是 token 的唯一数值来源。
// 这条把「合同里有、CSS 里没有」与「值漂了」都变成红灯——只读文档永远发现不了后者。
const theme = readFileSync(new URL('../src/theme.css', import.meta.url), 'utf8');
const tokens = JSON.parse(
  readFileSync(new URL('../../docs/ui/kami-implementation-2026-11/tokens.json', import.meta.url), 'utf8'),
);

function declarations(scope) {
  const out = new Map();
  for (const match of scope.matchAll(/(--[a-z0-9-]+)\s*:\s*([^;]+);/g)) {
    const value = match[2].trim();
    // 选择器里的 `--warning:hover { … }` 不是声明；值里出现花括号就跳过。
    if (!/[{}]/.test(value)) out.set(match[1], value);
  }
  return out;
}

const darkAt = theme.indexOf('\nhtml.dark {');
assert.ok(darkAt > 0, 'theme.css 必须有 html.dark 块');
const light = declarations(theme.slice(0, darkAt));
const dark = declarations(theme.slice(darkAt));

for (const [mode, actual, expected] of [
  ['浅色', light, tokens.themes.light],
  ['深色', dark, tokens.themes.dark],
]) {
  test(`${mode}主题：tokens.json 每个键在 theme.css 都存在且值一致`, () => {
    const missing = Object.keys(expected).filter((name) => !actual.has(name));
    const drifted = Object.keys(expected).filter(
      (name) => actual.has(name) && actual.get(name) !== expected[name],
    );
    assert.deepEqual(missing, [], `${mode}缺少 token：${missing.join(', ')}`);
    assert.deepEqual(
      drifted,
      [],
      `${mode} token 值漂了：${drifted.map((name) => `${name}=${actual.get(name)}（合同 ${expected[name]}）`).join('；')}`,
    );
  });
}

test('geometry 只声明一次，不写进 html.dark（否则切主题整页重排）', () => {
  const darkBody = theme.slice(darkAt);
  for (const declaration of [
    '--radius-card:',
    '--radius-control:',
    '--size-page-icon:',
    '--action-h:',
    '--sidebar-width:',
    '--titlebar-height:',
  ]) {
    assert.ok(!darkBody.includes(declaration), `${declaration} 不许出现在 html.dark 里`);
  }
});

test('页面几何按合同 §3：控件 6 / 浮层 8、页头 48、双列 24、块缝 16', () => {
  assert.match(theme, /--radius-control:\s*6px/);
  assert.match(theme, /--radius-card:\s*8px/);
  assert.match(theme, /\.page-head \{[^}]*min-height:\s*48px/s);
  assert.match(theme, /\.page-layout \{[^}]*gap:\s*0 24px/s);
  assert.match(theme, /\.panel \{[^}]*row-gap:\s*16px[^}]*column-gap:\s*16px/s);
  assert.match(theme, /\.main-view > main \{[^}]*padding:\s*20px 24px 24px/s);
});
