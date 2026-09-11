// `scripts/check-ui-bindings.mjs` 的用例：判据本身要能被反证，否则它只是一段
// "永远打印 ✓"的装饰。
import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';

import { checkProject, unresolvedIdentifiers } from './check-ui-bindings.mjs';

test('unresolvedIdentifiers 只挑出 _ctx 上的普通标识符', () => {
  const code = '_ctx.globalBusy + $setup.isLoading + _ctx.$slots + _ctx.foo + _ctx._bar';
  assert.deepEqual(unresolvedIdentifiers(code).sort(), ['foo', 'globalBusy']);
  assert.deepEqual(unresolvedIdentifiers('$setup.onlyBinding'), []);
});

test('checkProject 对已导入的绑定放行、对漏导入的报错', async () => {
  const root = mkdtempSync(join(tmpdir(), 'dsh-ui-bindings-'));
  try {
    mkdirSync(join(root, 'components'), { recursive: true });
    writeFileSync(
      join(root, 'App.vue'),
      `<script setup>
import { isLoading } from './loading.js';
</script>
<template><el-button :loading="isLoading('x')">ok</el-button></template>
`,
    );
    writeFileSync(
      join(root, 'components', 'Broken.vue'),
      `<script setup>
import { isLoading } from '../loading.js';
</script>
<template><el-button :loading="isLoading('x')" :disabled="globalBusy">ok</el-button></template>
`,
    );
    // 选项式 API 没有 setup 绑定可枚举：跳过而不是误报。
    writeFileSync(
      join(root, 'Legacy.vue'),
      `<script>export default { data: () => ({ label: 'x' }) };</script>
<template><span>{{ label }}{{ anythingGoes }}</span></template>
`,
    );

    const result = await checkProject(root);
    assert.equal(result.checked, 2, '两个 setup 组件被检查');
    assert.equal(result.skipped, 1, '选项式组件被跳过');
    assert.equal(result.failures.length, 1, '只有一个文件应当失败');
    assert.match(result.failures[0].file, /Broken\.vue$/);
    assert.deepEqual(result.failures[0].names, ['globalBusy']);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('真实组件树当前全部可解析', async () => {
  const result = await checkProject('ui/src');
  assert.deepEqual(
    result.failures.map((f) => `${f.file}:${f.names.join(',')}`),
    [],
    'UI 模板里不得出现未定义标识符',
  );
  assert.ok(result.checked >= 10, `应当检查到全部组件，实际 ${result.checked}`);
});
