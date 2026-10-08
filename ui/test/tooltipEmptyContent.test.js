import assert from 'node:assert/strict';
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

// 「没什么要说」不能靠给 el-tooltip 传空 content 表达。
//
// 2026-10-07 实机：预检通过后把鼠标放到「应用变更」上，按钮上方弹出一个
// **没有字的气泡**。原因是 element-plus 的可见性判据只看 `disabled` 与
// `open`，**完全不看 content 是否为空**（node_modules/element-plus/es/
// components/tooltip/src/content.vue_…mjs 里 `shouldShow = disabled ? false
// : open`）。所以 `:content="canApply ? '' : reason"` 在 canApply 为真时
// 照样弹一个空壳；要「什么都不说」只能用 `:disabled`。
//
// 这条测试扫全 ui/src：空 content 是一种**沉默的错误**——页面上看不出哪里
// 写错了，也不会报任何错，只有用户盯着那个气泡才知道。抄一份也不会有人
// 立刻发现（这个 bug 就在 PrecheckDialog 与 PluginDiagnosis 两处各存在过
// 一次，是同一份代码）。

// `fileURLToPath` 而不是 `.pathname`：后者在 Windows 上给出 `/C:/workdir/…`，
// `readdirSync` 把它当成本盘根下的相对段，路径变成 `C:\C:\workdir\…` ——
// 整条判据 ENOENT 挂在读目录那一步，**一条断言都没跑**（`diagnosticActionBoundary`
// 里同一种写法有同样的问题，一起改了）。
const UI_SRC = fileURLToPath(new URL('../src/', import.meta.url));

/** 递归列出 ui/src 下的所有 .vue。 */
function vueFiles(dir = UI_SRC) {
  const out = [];
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) out.push(...vueFiles(full));
    else if (entry.endsWith('.vue')) out.push(full);
  }
  return out;
}

/** 把源码里所有 <el-tooltip …> 开标签切出来（跨行也算）。 */
function tooltipOpenTags(source) {
  return source.match(/<el-tooltip\b[^>]*>/g) || [];
}

const ALL_TOOLTIPS = vueFiles().flatMap((file) =>
  tooltipOpenTags(readFileSync(file, 'utf8')).map((tag) => ({ file, tag })),
);

test('这个扫描本身抓得到东西（空标签列表说明正则写错了）', () => {
  // 没有这条，某个拼错的选择器会让后面所有断言静默通过——「门禁在跑、
  // 一直是绿的、但什么都没查」正是本文件要防的那类假通过。
  assert.ok(ALL_TOOLTIPS.length > 20, `只扫到 ${ALL_TOOLTIPS.length} 个 tooltip，正则多半坏了`);
});

test('没有任何 el-tooltip 用空 content 表达「没什么要说」', () => {
  const offenders = ALL_TOOLTIPS.filter(
    ({ tag }) =>
      // 三元落到空串：`x ? '' : y` / `x ? y : ''` / 只有 `''`
      /:content\s*=\s*"[^"]*['"]['"][^"]*"/.test(tag) ||
      /\?[^"]*['"]['"]/.test(tag) ||
      /:\s*['"]['"]/.test(tag) ||
      /^\s*content\s*=\s*['"]['"]/.test(tag),
  );
  assert.deepEqual(
    offenders.map(({ file, tag }) => `${file.replace(UI_SRC, '')}: ${tag}`),
    [],
    '空 content 仍会弹出空壳气泡，改用 :disabled',
  );
});

test('「应用变更」两处都用 :disabled 而非空 content 表达「可点」', () => {
  // 这两个按钮是同一份逻辑的两处落点（预检弹窗 / 插件诊断页），两处都要改。
  for (const [file, expected] of [
    ['plugins/PrecheckDialog.vue', 'canApply'],
    ['diagnostics/PluginDiagnosis.vue', 'canApply'],
  ]) {
    const source = readFileSync(join(UI_SRC, file), 'utf8');
    const applyTooltip = tooltipOpenTags(source).find((tag) => tag.includes('applyDisabledReason'));
    assert.ok(applyTooltip, `${file} 找不到带 applyDisabledReason 的 tooltip`);
    assert.match(
      applyTooltip,
      /:disabled\s*=\s*"canApply"/,
      `${file}：可点时必须让 tooltip 整个不出现（:disabled="canApply"）`,
    );
    assert.doesNotMatch(
      applyTooltip,
      /\?[^"]*['"]['"][^"]*"/,
      `${file}：不要再用三元把 content 落成空串`,
    );
    assert.equal(expected, 'canApply');
  }
});
