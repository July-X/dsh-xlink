// CSS 层叠检查器的语法边界判据（2026-11-01 OPT-10）。
//
// 这些用例钉的是检查器**自己的**行为：选择器列表逐分支匹配、重复声明取最后
// 一条、条件 at-rule 默认不参与、主题祖先按显式约定。它们不替代真实 WebView
// 的 computedStyle；静态检查通过不能写成「UI 渲染已通过」。
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { effectiveDeclaration, splitSelectors, specificity } from './lib/css-cascade.mjs';

test('选择器列表按顶层逗号拆分，括号里的逗号不拆', () => {
  assert.deepEqual(splitSelectors('.a, .b > .c'), ['.a', '.b > .c']);
  assert.deepEqual(splitSelectors(':is(.a, .b) .c'), [':is(.a, .b) .c']);
  assert.deepEqual(splitSelectors('.a[data-x="1,2"], .b'), ['.a[data-x="1,2"]', '.b']);
});

test('选择器列表逐分支匹配：命中分支自己算特异度', () => {
  const rules = [
    { selector: '.card-head, .card > h2', body: 'padding-bottom: 9px;' },
    { selector: '.card-head', body: 'padding-bottom: 10px;' },
  ];
  // .card-head 命中两条，同特异度后出现的赢；.card > h2 那条里的 h2 不该把
  // card-head 元素挡掉（旧实现把所有分支 token 取并集，会漏掉整条规则）。
  assert.equal(effectiveDeclaration(['card-head'], rules, 'padding-bottom'), '10px');
  assert.equal(effectiveDeclaration(['card', 'h2'], rules, 'padding-bottom'), '9px');
});

test('规则体内重复声明取最后一条（浏览器语义）', () => {
  const rules = [{ selector: '.x', body: 'color: red; color: blue;' }];
  assert.equal(effectiveDeclaration(['x'], rules, 'color'), 'blue');
});

test('条件 at-rule 默认不参与，显式声明条件才计入', () => {
  const media = '@media (prefers-reduced-motion: reduce)';
  const rules = [
    { selector: '.x', body: 'transition: all 200ms;', conditions: [] },
    { selector: '.x', body: 'transition: none;', conditions: [media] },
  ];
  assert.equal(effectiveDeclaration(['x'], rules, 'transition'), 'all 200ms');
  assert.equal(
    effectiveDeclaration(['x'], rules, 'transition', { conditions: [media] }),
    'none'
  );
});

test('主题祖先默认浅色；tokens 带 dark 时才计入 html.dark 规则', () => {
  const rules = [
    { selector: '.title', body: 'padding-bottom: 9px;' },
    { selector: 'html.dark .title', body: 'padding-bottom: 10px;' },
  ];
  assert.equal(effectiveDeclaration(['title'], rules, 'padding-bottom'), '9px');
  assert.equal(effectiveDeclaration(['title', 'dark'], rules, 'padding-bottom'), '10px');
  assert.equal(
    effectiveDeclaration(['title'], rules, 'padding-bottom', { theme: 'dark' }),
    '10px'
  );
});

test('主语 token 不存在时仍然返回 null（缺失 token 不会被当成命中）', () => {
  const rules = [{ selector: '.card-head', body: 'padding-bottom: 9px;' }];
  assert.equal(effectiveDeclaration(['card'], rules, 'padding-bottom'), null);
  assert.equal(specificity('.a, .b > .c'), 200);
});
