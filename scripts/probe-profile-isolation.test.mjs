import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

import {
  ALLOW_MARKER,
  findDataDirectoryViolations,
  findDataDirectoryViolationsInFiles,
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

// --- 判据 ③：建 webview 必须**无条件**给 data_directory --------------------
//
// 2026-10-20 13:48 那次写入用户真实目录的真正原因：探针把 profile 传成
// `Option<&Path>`，`None` 时**不设** `data_directory`，WebView2 就用**默认**
// 数据目录。源码里没有路径字面量 → 判据 ① ②（扫写死路径）抓不到。

/** 压根没给 data_directory（浏览器会落回默认目录）。 */
const NO_DATA_DIRECTORY = [
  'fn build_window(handle: &tauri::AppHandle, label: &str, url: &str) -> Result<(), String> {',
  '    let parsed = url.parse().map_err(|e| format!("URL 非法：{e}"))?;',
  '    tauri::WebviewWindowBuilder::new(handle, label, tauri::WebviewUrl::External(parsed))',
  '        .title("探针")',
  '        .build()',
  '        .map_err(|e| format!("无法创建探针窗口：{e}"))?;',
  '    Ok(())',
  '}',
].join('\n');

/** 13:48 那次的写法：profile 是 Option，条件成立才给 data_directory。 */
const CONDITIONAL_DATA_DIRECTORY = [
  'fn build_window_with(',
  '    handle: &tauri::AppHandle,',
  '    profile: Option<&std::path::Path>,',
  ') -> Result<(), String> {',
  '    let mut builder = tauri::WebviewWindowBuilder::new(handle, "x", url)',
  '        .title("探针");',
  '    if let Some(profile) = profile {',
  '        builder = builder.data_directory(profile.to_path_buf());',
  '    }',
  '    builder.build()?;',
  '    Ok(())',
  '}',
].join('\n');

/** 修好之后：data_directory 在建窗链上，无条件。 */
const UNCONDITIONAL_DATA_DIRECTORY = [
  'fn build_window_with(',
  '    handle: &tauri::AppHandle,',
  '    profile: &std::path::Path,',
  ') -> Result<(), String> {',
  '    tauri::WebviewWindowBuilder::new(handle, "x", url)',
  '        .title("探针")',
  '        .data_directory(profile.to_path_buf())',
  '        .build()?;',
  '    Ok(())',
  '}',
].join('\n');

test('建 webview 压根没给 data_directory → 报红，并指到建窗那一行', () => {
  const hits = findDataDirectoryViolations(NO_DATA_DIRECTORY, 'probe.rs');
  assert.equal(hits.length, 1);
  assert.equal(hits[0].file, 'probe.rs');
  assert.equal(hits[0].reason, 'no-data-directory');
  assert.equal(hits[0].line, 3);
  assert.ok(hits[0].text.includes('WebviewWindowBuilder'));
});

test('data_directory 只在条件分支里给 → 报红（「有才给」= 没给）', () => {
  const hits = findDataDirectoryViolations(CONDITIONAL_DATA_DIRECTORY, 'probe.rs');
  assert.equal(hits.length, 1);
  assert.equal(hits[0].reason, 'conditional-data-directory');
  // 行号指到**建窗**那一行（不是 data_directory 那一行）：那是这条判据的锚点。
  assert.equal(hits[0].line, 5);
  assert.ok(hits[0].detail.includes('条件分支'));
});

test('data_directory 在建窗链上无条件给 → 放行', () => {
  assert.deepEqual(findDataDirectoryViolations(UNCONDITIONAL_DATA_DIRECTORY, 'probe.rs'), []);
});

test('data_directory 在无条件语句里给（不在任何条件块内）→ 放行', () => {
  const src = [
    'fn build_window_with(handle: &tauri::AppHandle, profile: &std::path::Path) {',
    '    let mut builder = tauri::WebviewWindowBuilder::new(handle, "x", url);',
    '    builder = builder.data_directory(profile.to_path_buf());',
    '    builder.build()?;',
    '}',
  ].join('\n');
  assert.deepEqual(findDataDirectoryViolations(src, 'probe.rs'), []);
});

test('作用域不串味：后一个函数无条件给了 data_directory，前一个函数的违规仍要报', () => {
  const src = [
    'fn first(handle: &tauri::AppHandle, url: &str) {',
    '    tauri::WebviewWindowBuilder::new(handle, "a", url).build()?;',
    '}',
    '',
    'fn second(handle: &tauri::AppHandle, url: &str, profile: &std::path::Path) {',
    '    let mut b = tauri::WebviewWindowBuilder::new(handle, "b", url);',
    '    b = b.data_directory(profile.to_path_buf());',
    '    b.build()?;',
    '}',
  ].join('\n');
  const hits = findDataDirectoryViolations(src, 'probe.rs');
  assert.equal(hits.length, 1, '只应报 first 里的那一个');
  assert.equal(hits[0].line, 2);
});

test('match 分支里给 data_directory 也算「有才给」→ 报红', () => {
  const src = [
    'fn build_window_with(handle: &tauri::AppHandle, profile: Option<&std::path::Path>) {',
    '    let mut builder = tauri::WebviewWindowBuilder::new(handle, "x", url);',
    '    match profile {',
    '        Some(p) => { builder = builder.data_directory(p.to_path_buf()); }',
    '        None => {}',
    '    }',
    '    builder.build()?;',
    '}',
  ].join('\n');
  const hits = findDataDirectoryViolations(src, 'probe.rs');
  assert.equal(hits.length, 1);
  assert.equal(hits[0].reason, 'conditional-data-directory');
});

test('if/else 两个分支都给了 data_directory → 每条路径都有目录 → 放行', () => {
  const src = [
    'fn build_window_with(handle: &tauri::AppHandle, pick: bool) {',
    '    let mut builder = tauri::WebviewWindowBuilder::new(handle, "x", url);',
    '    if pick {',
    '        builder = builder.data_directory(dir_a.to_path_buf());',
    '    } else {',
    '        builder = builder.data_directory(dir_b.to_path_buf());',
    '    }',
    '    builder.build()?;',
    '}',
  ].join('\n');
  assert.deepEqual(findDataDirectoryViolations(src, 'probe.rs'), []);
});

test('建窗点本身就在条件块里（那条路径压根不建窗）→ 不算漏 → 放行', () => {
  const src = [
    'fn maybe_build(handle: &tauri::AppHandle, yes: bool, profile: &std::path::Path) {',
    '    if yes {',
    '        tauri::WebviewWindowBuilder::new(handle, "x", url)',
    '            .data_directory(profile.to_path_buf())',
    '            .build()?;',
    '    }',
    '}',
  ].join('\n');
  assert.deepEqual(findDataDirectoryViolations(src, 'probe.rs'), []);
});

test('if / else 里 else 分支没给 → 有一条路径漏 → 报红', () => {
  const src = [
    'fn build_window_with(handle: &tauri::AppHandle, pick: bool) {',
    '    let mut builder = tauri::WebviewWindowBuilder::new(handle, "x", url);',
    '    if pick {',
    '        builder = builder.data_directory(dir_a.to_path_buf());',
    '    } else {',
    '        builder = builder.title("探针");',
    '    }',
    '    builder.build()?;',
    '}',
  ].join('\n');
  const hits = findDataDirectoryViolations(src, 'probe.rs');
  assert.equal(hits.length, 1);
  assert.equal(hits[0].reason, 'conditional-data-directory');
});

test('if / else if / else 三个分支都给了 → 每条路径都有目录 → 放行', () => {
  const src = [
    'fn build_window_with(handle: &tauri::AppHandle, pick: u8) {',
    '    let mut builder = tauri::WebviewWindowBuilder::new(handle, "x", url);',
    '    if pick == 0 {',
    '        builder = builder.data_directory(dir_a.to_path_buf());',
    '    } else if pick == 1 {',
    '        builder = builder.data_directory(dir_b.to_path_buf());',
    '    } else {',
    '        builder = builder.data_directory(dir_c.to_path_buf());',
    '    }',
    '    builder.build()?;',
    '}',
  ].join('\n');
  assert.deepEqual(findDataDirectoryViolations(src, 'probe.rs'), []);
});

test('if / else if 都给了但没有最终 else → 还有隐式空路径 → 报红', () => {
  const src = [
    'fn build_window_with(handle: &tauri::AppHandle, pick: u8) {',
    '    let mut builder = tauri::WebviewWindowBuilder::new(handle, "x", url);',
    '    if pick == 0 {',
    '        builder = builder.data_directory(dir_a.to_path_buf());',
    '    } else if pick == 1 {',
    '        builder = builder.data_directory(dir_b.to_path_buf());',
    '    }',
    '    builder.build()?;',
    '}',
  ].join('\n');
  const hits = findDataDirectoryViolations(src, 'probe.rs');
  assert.equal(hits.length, 1);
  assert.equal(hits[0].reason, 'conditional-data-directory');
});

test('建窗无条件、profile 条件给（13:48 那种）→ 报红', () => {
  const src = [
    'fn build_window_with(handle: &tauri::AppHandle, profile: Option<&std::path::Path>) {',
    '    let mut builder = tauri::WebviewWindowBuilder::new(handle, "x", url);',
    '    if let Some(p) = profile {',
    '        builder = builder.data_directory(p.to_path_buf());',
    '    }',
    '    builder.build()?;',
    '}',
  ].join('\n');
  const hits = findDataDirectoryViolations(src, 'probe.rs');
  assert.equal(hits.length, 1);
  assert.equal(hits[0].reason, 'conditional-data-directory');
});

test('注释与字符串里提到 data_directory 不算数（没建 webview 就不报）', () => {
  const src = [
    '// 判据提醒：建 webview 时必须给 data_directory',
    'const NOTE = "data_directory 是必填的";',
    'fn nothing() {}',
  ].join('\n');
  assert.deepEqual(findDataDirectoryViolations(src, 'probe.rs'), []);
});

test('批量扫描带上文件名', () => {
  const hits = findDataDirectoryViolationsInFiles([
    { file: 'a.rs', text: NO_DATA_DIRECTORY },
    { file: 'b.rs', text: UNCONDITIONAL_DATA_DIRECTORY },
  ]);
  assert.equal(hits.length, 1);
  assert.equal(hits[0].file, 'a.rs');
});

test('check-invariants 接了这条判据，并且报的是 file:line', () => {
  const source = read('./check-invariants.mjs');
  assert.ok(
    source.includes('findDataDirectoryViolationsInFiles'),
    'check-invariants.mjs 必须调用新判据',
  );
  assert.ok(
    source.includes("'probe-data-directory'"),
    '新判据要有自己的失败分区，别混进判据 30 里',
  );
  assert.ok(
    source.includes('${o.file}:${o.line}'),
    '失败信息必须带 file:line',
  );
});

test('两个判据共用同一份扫描范围（collectProbeScopeFiles）', () => {
  const source = read('./check-invariants.mjs');
  const calls = source.match(/collectProbeScopeFiles\(\)/g) ?? [];
  // 一次定义 + 两条判据各调一次 = 3 次。
  assert.equal(calls.length, 3, '扫描范围必须只有一处定义，两条判据共用');
});

// --- 判据 ③ 的合成用例集（2026-10-20 tester 实测的真值） --------------------
//
// 下面这组用例是当时用**临时探针逐条实跑**出来的真值（不是照着实现写的期望）。
// 当时判据有**两处**不符——PM 16:22 指派修的就是这两处：
//   · D（嵌套 if）判**放行** = 真漏判：`a` 成立、`b2` 不成立时**没给目录**；
//   · G（逃生阀）判**违规** = 第 31 条**根本没有出口**（`ALLOW_MARKER` 零引用）。
// 判据修完后：D 必须判违规，G 必须放行——下面就是钉住这两条的回归。
//
// **合成用例必须含建窗触发点**（`WebviewWindowBuilder::new`）：没有触发点，
// 判据整段跳过、返回「无违规」，看起来像「判据没抓到」，其实是用例写错了
// （2026-10-20 踩过：第一版 4 个用例全「放行」就是这个原因）。

/** A. `if / else` 两个分支都给 → 每条路径都有目录 → 放行。 */
const CASE_A_IF_ELSE = [
  'fn build(app, p, q) {',
  '    let mut b = WebviewWindowBuilder::new(app, "x", url);',
  '    if cond {',
  '        b = b.data_directory(p);',
  '    } else {',
  '        b = b.data_directory(q);',
  '    }',
  '    b.build()',
  '}',
].join('\n');

/**
 * B. `match` 穷尽、两个臂都给 → 放行。
 *
 * 注意它放行的**真正原因**：建窗点在 **match 的臂里**——每个臂各自建一次
 * webview、各自带目录（「建窗点本身就在条件块里 → 那条路径压根不建窗」→
 * 不算漏）。**不是**因为「match 穷尽」。建窗点在 match **外面**、两臂都给
 * 的形状仍按「有才给」判红（见上面那条「match 分支里给 → 报红」）。
 */
const CASE_B_MATCH_EXPR = [
  'fn build(app, p, q) {',
  '    let b = match profile {',
  '        Some(x) => WebviewWindowBuilder::new(app, "x", url).data_directory(x),',
  '        None => WebviewWindowBuilder::new(app, "x", url).data_directory(q),',
  '    };',
  '    b.build()',
  '}',
].join('\n');

/** C. 同 B，但 match 的臂用花括号块 → 放行（理由同 B）。 */
const CASE_C_MATCH_BLOCK = [
  'fn build(app, p, q) {',
  '    let b = match profile {',
  '        Some(x) => { WebviewWindowBuilder::new(app, "x", url).data_directory(x) }',
  '        None => { WebviewWindowBuilder::new(app, "x", url).data_directory(q) }',
  '    };',
  '    b.build()',
  '}',
].join('\n');

/**
 * D. **嵌套 if** → 违规（真漏判的回归钉子）。
 *
 * `a` 成立、`b2` 不成立时**没给目录** → 浏览器照样用默认数据目录。
 * 命中点在**更深的条件**里，不算这条分支「给了」。
 */
const CASE_D_NESTED_IF = [
  'fn build(app, p, q) {',
  '    let mut b = WebviewWindowBuilder::new(app, "x", url);',
  '    if a {',
  '        if b2 { b = b.data_directory(p); }',
  '    } else {',
  '        b = b.data_directory(q);',
  '    }',
  '    b.build()',
  '}',
].join('\n');

/** E. 无条件给 → 放行。 */
const CASE_E_UNCONDITIONAL = [
  'fn build(app, p) {',
  '    let b = WebviewWindowBuilder::new(app, "x", url).data_directory(p);',
  '    b.build()',
  '}',
].join('\n');

/** F. 压根没给 → 违规。 */
const CASE_F_NO_DIRECTORY = [
  'fn build(app) {',
  '    let b = WebviewWindowBuilder::new(app, "x", url);',
  '    b.build()',
  '}',
].join('\n');

/** G. 不需要 user-data 的 webview + **文件级**逃生阀标记 → 放行。 */
const CASE_G_FILE_MARKER = [
  `// ${ALLOW_MARKER}`,
  'fn build_about_blank(app) {',
  '    let b = WebviewWindowBuilder::new(app, "x", url);',
  '    b.build()',
  '}',
].join('\n');

/** H. 单行 `if / else`（C# switch 风格）两个分支都给 → 放行。 */
const CASE_H_INLINE_IF_ELSE = [
  'fn build(app, p, q) {',
  '    let mut b = WebviewWindowBuilder::new(app, "x", url);',
  '    if cond { b = b.data_directory(p); } else { b = b.data_directory(q); }',
  '    b.build()',
  '}',
].join('\n');

/** G2. **行尾**逃生阀标记 → 放行那个建窗点（PM 裁定：两种形态都认）。 */
const CASE_G2_LINE_MARKER = [
  'fn build_about_blank(app) {',
  `    let b = WebviewWindowBuilder::new(app, "x", url); // ${ALLOW_MARKER}`,
  '    b.build()',
  '}',
].join('\n');

test('合成用例 A：if / else 两个分支都给 → 放行', () => {
  assert.deepEqual(findDataDirectoryViolations(CASE_A_IF_ELSE, 'probe.rs'), []);
});

test('合成用例 B：match 穷尽、两个臂都给（建窗点在臂里）→ 放行', () => {
  assert.deepEqual(findDataDirectoryViolations(CASE_B_MATCH_EXPR, 'probe.rs'), []);
});

test('合成用例 C：match 的臂用花括号块、两个臂都给 → 放行', () => {
  assert.deepEqual(findDataDirectoryViolations(CASE_C_MATCH_BLOCK, 'probe.rs'), []);
});

test('合成用例 D：嵌套 if（命中在更深的条件里）→ 判违规（真漏判的回归钉子）', () => {
  const hits = findDataDirectoryViolations(CASE_D_NESTED_IF, 'probe.rs');
  assert.equal(hits.length, 1, 'a 成立、b2 不成立时没给目录 → 必须报红');
  assert.equal(hits[0].reason, 'conditional-data-directory');
  // 行号指到**建窗**那一行（不是 data_directory 那一行）：那是这条判据的锚点。
  assert.equal(hits[0].line, 2, '建窗行是第 2 行（本用例常量没有前导空行）');
});

test('合成用例 E：无条件给 → 放行', () => {
  assert.deepEqual(findDataDirectoryViolations(CASE_E_UNCONDITIONAL, 'probe.rs'), []);
});

test('合成用例 F：压根没给 → 判违规 no-data-directory', () => {
  const hits = findDataDirectoryViolations(CASE_F_NO_DIRECTORY, 'probe.rs');
  assert.equal(hits.length, 1);
  assert.equal(hits[0].reason, 'no-data-directory');
});

test('合成用例 G：文件级逃生阀标记 → 放行整个文件（第 31 条认同一个标记）', () => {
  assert.deepEqual(findDataDirectoryViolations(CASE_G_FILE_MARKER, 'probe.rs'), []);
});

test('合成用例 H：单行 if / else 两个分支都给 → 放行', () => {
  assert.deepEqual(findDataDirectoryViolations(CASE_H_INLINE_IF_ELSE, 'probe.rs'), []);
});

test('合成用例 G2：行尾逃生阀标记 → 放行那个建窗点', () => {
  assert.deepEqual(findDataDirectoryViolations(CASE_G2_LINE_MARKER, 'probe.rs'), []);
});

test('第 31 条的行尾标记只放行那个建窗点，同文件另一个建窗点仍报红', () => {
  const src = [
    'fn build(app) {',
    `    let a = WebviewWindowBuilder::new(app, "a", url); // ${ALLOW_MARKER}`,
    '    a.build()',
    '}',
    'fn build2(app) {',
    '    let b = WebviewWindowBuilder::new(app, "b", url);',
    '    b.build()',
    '}',
  ].join('\n');
  const hits = findDataDirectoryViolations(src, 'probe.rs');
  assert.equal(hits.length, 1, '放行粒度是「建窗点」，不是「整个文件」');
  assert.equal(hits[0].line, 6);
});

test('两条判据各自独立：第 30 条的行尾标记放行那一行，第 31 条仍报红', () => {
  const src = [
    `const p = "C:\\Users\\zxx\\.dsh-xlink\\webview-official-chat"; // ${ALLOW_MARKER}`,
    'fn build(app) {',
    '    let b = WebviewWindowBuilder::new(app, "x", url);',
    '    b.build()',
    '}',
  ].join('\n');
  assert.deepEqual(findProfileViolations(src, 'probe.rs'), [], '第 30 条放行那一行');
  const hits = findDataDirectoryViolations(src, 'probe.rs');
  assert.equal(hits.length, 1, '第 30 条放行 ≠ 第 31 条放行：第 31 条仍要报红');
  assert.equal(hits[0].reason, 'no-data-directory');
});

test('两条判据各自独立：第 31 条的行尾标记放行那个建窗点，第 30 条仍报红', () => {
  const src = [
    'fn build(app) {',
    `    let b = WebviewWindowBuilder::new(app, "x", url); // ${ALLOW_MARKER}`,
    '    b.build()',
    '}',
    'const p = "C:\\Users\\zxx\\.dsh-xlink\\webview-official-chat";',
  ].join('\n');
  const hits = findDataDirectoryViolations(src, 'probe.rs');
  assert.deepEqual(hits, [], '第 31 条放行那个建窗点');
  const profile = findProfileViolations(src, 'probe.rs');
  assert.equal(profile.length, 1, '第 31 条放行 ≠ 第 30 条放行：第 30 条仍要报红');
  assert.equal(profile[0].line, 5);
});

// --- 判据 ③ 的 match 边界（2026-10-20 PM 裁定：match 链天然闭合） ------------
//
// 事实依据：Rust 的穷尽性检查保证**任何能编译的 `match` 都覆盖所有可能取值**
// （整数/浮点必须写 `_`、枚举要列全 variant、`bool` 两个都要、`Option` 要
// `Some`/`None` 都写）——**没有**「条件全不成立」那条隐式路径。所以 match 链
// 与 `if/else` **同构**：每臂都给 → 放行；有一臂没给 → 违规。
//
// 之前把 match 当「非闭合链」= 把 `if` 的隐式路径规则**错套**到 match 上，
// 于是「建窗点在 match 外面、每臂都给」被报红——那是**假红**。而第 31 条的
// 逃生阀语义已被写死成「**我承认这次要写用户真实安装目录**」，假红会把
// 开发者推向那个最重的承认，所以假红必须修。
//
// 实现要三件事同时做（少一件都会把错换到另一个方向）：
//   1. match 链**天然闭合**（`closed: true`）——没有隐式空分支；
//   2. match 的**臂**也算条件块、入同一条链——这样每臂**单独**判覆盖，
//      「一臂没给」才会被报红；
//   3. match 块**自己不入 members**——否则 `covered(match)` 自引用 → 恒 false。

/** match 在外·**每臂都给** → 放行（链穷尽 + 每臂都给了目录）。 */
const CASE_MATCH_ALL_ARMS_GIVE = [
  'fn build(app, p, q) {',
  '    let mut b = WebviewWindowBuilder::new(app, label, url);',
  '    match profile {',
  '        Some(x) => { b = b.data_directory(p); }',
  '        None => { b = b.data_directory(q); }',
  '    }',
  '    b.build()',
  '}',
].join('\n');

/**
 * match 在外·**有一臂没给** → 仍报红。
 *
 * 这条是「放宽 match 判据」的**反向回归**：只把 match 链标成闭合、不让臂
 * 单独判覆盖的话，这条会从报红变成放行（= 漏判），两个方向的错一起换位置。
 */
const CASE_MATCH_ONE_ARM_MISSING = [
  'fn build(app, p) {',
  '    let mut b = WebviewWindowBuilder::new(app, label, url);',
  '    match profile {',
  '        Some(x) => { b = b.data_directory(p); }',
  '        None => {}',
  '    }',
  '    b.build()',
  '}',
].join('\n');

/** match 在外·**三臂都给** → 放行（臂数不是 2 也要成立）。 */
const CASE_MATCH_THREE_ARMS_GIVE = [
  'fn build(app, p, q, r) {',
  '    let mut b = WebviewWindowBuilder::new(app, label, url);',
  '    match profile {',
  '        Some(x) => { b = b.data_directory(p); }',
  '        None => { b = b.data_directory(q); }',
  '        Other => { b = b.data_directory(r); }',
  '    }',
  '    b.build()',
  '}',
].join('\n');

/**
 * match 头**带赋值前缀**（`b = match …`）·每臂都给 → 放行。
 *
 * 识别 match 头时**不能锚定行首**：`let b = match x {` 的块头是
 * `let b = match x `，锚定 `^\s*match` 会漏判 → 假红原样留着。
 */
const CASE_MATCH_ASSIGN_PREFIX = [
  'fn build(app, p, q) {',
  '    let mut b = WebviewWindowBuilder::new(app, label, url);',
  '    b = match profile {',
  '        Some(x) => { b = b.data_directory(p); }',
  '        None => { b = b.data_directory(q); }',
  '    };',
  '    b.build()',
  '}',
].join('\n');

test('match 在外·每臂都给 → 放行（match 链天然闭合，不是假红）', () => {
  assert.deepEqual(findDataDirectoryViolations(CASE_MATCH_ALL_ARMS_GIVE, 'probe.rs'), []);
});

test('match 在外·有一臂没给 → 仍报红（放宽判据的反向回归）', () => {
  const hits = findDataDirectoryViolations(CASE_MATCH_ONE_ARM_MISSING, 'probe.rs');
  assert.equal(hits.length, 1, '有一臂没给 → 有一条路径落回默认目录 → 必须报红');
  assert.equal(hits[0].reason, 'conditional-data-directory');
  assert.equal(hits[0].line, 2);
});

test('match 在外·三臂都给 → 放行', () => {
  assert.deepEqual(findDataDirectoryViolations(CASE_MATCH_THREE_ARMS_GIVE, 'probe.rs'), []);
});

test('match 头带赋值前缀（b = match …）·每臂都给 → 放行（识别不能锚定行首）', () => {
  assert.deepEqual(findDataDirectoryViolations(CASE_MATCH_ASSIGN_PREFIX, 'probe.rs'), []);
});

test('match 的臂自己带块时仍按「每臂单独判覆盖」：一臂没给 → 报红', () => {
  // 臂体是**嵌套 match** 的罕见形状：那个块既是父 match 的一个臂，又是
  // 内层 match 的头——两件事都要记，否则父链会漏掉这个臂。
  const src = [
    'fn build(app, p, q) {',
    '    let mut b = WebviewWindowBuilder::new(app, label, url);',
    '    match profile {',
    '        Some(x) => match other {',
    '            Y => { b = b.data_directory(p); }',
    '        },',
    '        None => { b = b.data_directory(q); }',
    '    }',
    '    b.build()',
    '}',
  ].join('\n');
  assert.deepEqual(findDataDirectoryViolations(src, 'probe.rs'), []);
});

