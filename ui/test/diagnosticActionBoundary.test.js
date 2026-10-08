// 诊断层的动作边界（审查 R2-P2-02）。
//
// `diagnostic-actions.js` 开头写着「组件只调这里的动作，不直接 import store /
// plugins 的动作」。规矩只写了一半时，剩下的那一半靠自觉——而下一个人一定会
// 直接 import。本文件把那条线**钉在源码上**：诊断页再直接 import 业务动作就红。
//
// ## 概览页是**例外，而且是刻意的**
//
// `ControlTower` 在 `diagnostics/` 目录下，但它是**概览页**：不建立运行记录
// 上下文，也不需要与诊断页共用 loading key，它要的只是「点这一行去看日志」。
// 为此让它依赖一个它不需要的层是纯负担。所以例外写在这里，而不是留给下个人
// 去猜「概览算不算诊断页」。
import assert from 'node:assert/strict';
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

// `fileURLToPath` 而不是 `.pathname`：后者在 Windows 上给出 `/C:/workdir/…`，
// `readdirSync` 把它当成本盘根下的相对段，路径变成 `C:\C:\workdir\…`，整条
// 判据 ENOENT 挂在读目录那一步——**一条断言都没跑**。`tooltipEmptyContent` 里
// 同一种写法有同样的问题，一起改了。
const UI_SRC = fileURLToPath(new URL('../src/', import.meta.url));
const DIAG_DIR = join(UI_SRC, 'diagnostics');

function read(file) {
  return readFileSync(file, 'utf8');
}
// `ControlTower` 被排除：它在 `diagnostics/` 目录下，但它是**概览页**——
// 不建立运行记录上下文，也不需要与诊断页共用 loading key。下方的测试专门
// 钉住「它是例外」这件事，所以排除是有记录的决定，不是漏洞。
const OVERVIEW_EXEMPT = new Set(['ControlTower.vue']);
const componentFiles = readdirSync(DIAG_DIR)
  .filter((f) => f.endsWith('.vue') && !OVERVIEW_EXEMPT.has(f))
  .map((f) => join(DIAG_DIR, f));

/** 这些业务模块的动作必须经代理中转，诊断组件不许直接 import。 */
const FORBIDDEN_IMPORTS = [
  { module: '../plugins/plugins.js', names: ['applyPluginChange', 'precheckPlugin'] },
  { module: '../logs/logs.js', names: ['showLogs', 'loadLogList'] },
  { module: './diagnostics.js', names: ['loadStartupDiagnosis', 'loadOperationDiagnosis', 'loadPluginRun', 'getLastRunId'] },
  { module: '../store.js', names: ['showIncident', 'openHarnessWindow', 'startWorkbench', 'refreshAll'] },
];

test('这个扫描本身抓得到东西（列表空说明扫错了目录）', () => {
  // 没有这条，一个写错的后缀就会让后面所有断言静默通过——「门禁在跑、一直绿、
  // 但什么都没查」比没有门禁更糟。
  assert.ok(componentFiles.length >= 5, `只扫到 ${componentFiles.length} 个诊断组件`);
});

test('诊断组件不直接 import 业务动作，一律走动作代理', () => {
  const offenders = [];
  for (const file of componentFiles) {
    const source = read(file);
    for (const { module, names } of FORBIDDEN_IMPORTS) {
      for (const name of names) {
        // 匹配 `import { a, name, b } from 'module'` 形式。
        const re = new RegExp(
          "import\\s*\\{[^}]*\\b" + name + "\\b[^}]*\\}\\s*from\\s*['\"]" +
            module.replace(/[.*+?^${}()|[\]\\]/g, '\\$&') + "['\"]"
        );
        if (re.test(source)) offenders.push(`${file.replace(UI_SRC, '')} → ${name} from ${module}`);
      }
    }
  }
  assert.deepEqual(offenders, [], '诊断组件的动作要走 ./diagnostic-actions.js');
});

test('四个诊断页与预检弹窗都真的在用代理', () => {
  // 只查「不许直调」不够：全改成不调也是「符合规矩」的。得钉住它们确实
  // 走代理，否则诊断页会退化成什么都不做。
  for (const [file, symbol] of [
    ['diagnostics/DiagnosisShell.vue', 'reloadStartupDiagnosis'],
    ['diagnostics/PluginDiagnosis.vue', 'applyPrecheckChange'],
    ['diagnostics/StartupDiagnosis.vue', 'reloadStartupDiagnosis'],
    ['diagnostics/OperationDiagnosis.vue', 'reloadOperationDiagnosis'],
    ['plugins/PrecheckDialog.vue', 'applyPrecheckChange'],
  ]) {
    const source = read(join(UI_SRC, file));
    assert.match(
      source,
      new RegExp('import\\s*\\{[^}]*\\b' + symbol + '\\b[^}]*\\}\\s*from\\s*[\'"][^\'"]*diagnostic-actions\\.js[\'"]'),
      `${file} 应当从 diagnostic-actions.js 取 ${symbol}`
    );
  }
});

test('概览控制塔直接用业务模块是被允许的例外，写清楚了为什么', () => {
  const source = read(join(DIAG_DIR, 'ControlTower.vue'));
  assert.match(source, /from '\.\.\/logs\/logs\.js'/, '概览页可以直接用日志模块的纯动作');
  // 例外必须写下来：留在代码里当隐形约定的话，下一个人会把它当成疏漏改掉，
  // 或者反过来把概览也塞进代理。
  const proxy = read(join(DIAG_DIR, 'diagnostic-actions.js'));
  assert.match(
    proxy,
    /概览页可以直接用业务模块的纯动作|概览不是诊断层/,
    '代理文件要写明「概览页是例外」，否则这条边界不可见'
  );
});
