import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

import {
  ALLOW_MARKER,
  findProfileViolations,
  findProfileViolationsInFiles,
} from './lib/probe-profile-isolation.mjs';

const read = (path) => readFileSync(new URL(path, import.meta.url), 'utf8');

// 事故现场那一行（2026-10-20 official-chat 首站探测的用法示例）。
const INCIDENT_LINE =
  '//!   --profile "C:\\Users\\zxx\\.dsh-xlink\\dsh\\desktop-dev\\webview-official-chat"';

test('真实 profile 路径字面量被判违规，且报得出行号', () => {
  const hits = findProfileViolations(INCIDENT_LINE, 'probe.rs');
  assert.equal(hits.length, 1);
  assert.equal(hits[0].file, 'probe.rs');
  assert.equal(hits[0].line, 1);
  assert.ok(hits[0].text.includes('webview-official-chat'));
});

test('路径是拼出来的也算违规（产品 profile 目录名）', () => {
  const hits = findProfileViolations(
    'const p = join(home, "webview-official-chat");',
    'probe.rs',
  );
  assert.equal(hits.length, 1);
});

test('真实家目录与浏览器 profile 标记同行也算违规', () => {
  const hits = findProfileViolations(
    'builder.data_directory("C:\\\\Users\\\\zxx\\\\.dsh-xlink");',
    'probe.rs',
  );
  assert.equal(hits.length, 1);
});

test('只读路径解析放行：读内核 active.txt 不算违规', () => {
  const src =
    "const active = join(process.env.HOME ?? '', '.dsh-xlink', mode, 'desktop', 'active.txt');";
  assert.deepEqual(findProfileViolations(src, 'drive-stub.mjs'), []);
});

test('内核安装树路径（证据基线）放行', () => {
  const src =
    '* 现役 release 树：`C:\\Users\\zxx\\.dsh-xlink\\dsh\\desktop\\kernels\\0.2.1-rc.2`';
  assert.deepEqual(findProfileViolations(src, 'client.js'), []);
});

test('独占一行的放行标记放行整个文件（写在注释里也算）', () => {
  const src = [
    `// ${ALLOW_MARKER}`,
    INCIDENT_LINE,
  ].join('\n');
  // 放行标记独占一行 = 放行整个文件；行尾标记 = 只放行那一行。
  const wholeFile = findProfileViolations(src, 'probe.rs');
  assert.deepEqual(wholeFile, []);
});

test('行尾放行标记放行同行的违规，其余行仍然被抓', () => {
  const src = [
    `const p = "C:\\Users\\zxx\\.dsh-xlink\\webview-official-chat"; // ${ALLOW_MARKER}`,
    INCIDENT_LINE,
  ].join('\n');
  const hits = findProfileViolations(src, 'probe.rs');
  assert.equal(hits.length, 1);
  assert.equal(hits[0].line, 2);
});

test('批量扫描会带上文件名', () => {
  const hits = findProfileViolationsInFiles([
    { file: 'a.rs', text: 'let x = 1;\n' + INCIDENT_LINE },
    { file: 'b.rs', text: 'let y = 2;\n' },
  ]);
  assert.equal(hits.length, 1);
  assert.equal(hits[0].file, 'a.rs');
  assert.equal(hits[0].line, 2);
});

test('扫描范围只含测试/探针目录：生产代码不参与（否则壳自己的官网窗口会误红）', () => {
  const source = read('./check-invariants.mjs');
  const scope = source.match(/const probeRoots = \[([\s\S]*?)\];/);
  assert.ok(scope, 'check-invariants.mjs 里找不到 probeRoots，判据作用域失效');
  const listed = scope[1];
  assert.ok(listed.includes("'src-tauri', 'examples'"), 'examples 必须在内');
  assert.ok(listed.includes("'src-tauri', 'tests'"), 'tests 必须在内');
  assert.ok(listed.includes("'e2e'"), 'e2e 必须在内');
  assert.ok(!listed.includes("'src-tauri', 'src'"), '生产 Rust 目录不得进扫描范围');
  assert.ok(
    source.includes("join(pluginRoot, entry.name, 'test')"),
    '插件的 test 目录必须进扫描范围',
  );
});

test('scripts/ 刻意不进扫描范围：规则本体与自测必然含有被禁字面量', () => {
  const source = read('./check-invariants.mjs');
  const scope = source.match(/const probeRoots = \[([\s\S]*?)\];/)?.[1] ?? '';
  assert.ok(
    !scope.includes("'scripts'"),
    'scripts/ 不得进扫描范围——规则会开始抓自己（实测 14 条命中全来自规则本体与自测）',
  );
  const lib = read('./lib/probe-profile-isolation.mjs');
  assert.ok(
    lib.includes('规则会开始抓自己'),
    'lib 里必须写明为什么排除 scripts/，否则下一个人会「顺手」把它加回去',
  );
});
