// 「模型用量」窗口的**外部来源**能力：勾选哪些外部 Agent 工具（Codex、
// MiniMax Desktop）的对话 token 用量也算进来。
//
// 这条链路上有三件事单测能钉住、别处钉不住：
//  ① **默认不勾**——不勾就不读那些工具留在本机的目录（后端按参数决定读什么，
//     前端不能偷偷传 true）；
//  ② **勾选状态的真值在 Rust 侧**——前端那份只是镜像，每次拉取都以
//     `external_sources` 为准；
//  ③ **概览卡片不扫外部来源**——它不显示那些数据，不该为它付扫描成本。
import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';

const calls = [];
let nextData = null;

const core = {
  invoke(cmd, args) {
    calls.push({ cmd, args });
    if (cmd === 'get_model_usage') return Promise.resolve(nextData);
    if (cmd === 'set_usage_external_sources') {
      return Promise.resolve(args.sources);
    }
    return Promise.resolve(null);
  },
  Channel: class {
    onmessage = null;
  },
};

globalThis.window = { __TAURI__: { core }, navigator: { userAgent: 'node' } };
globalThis.document = {
  hidden: false,
  createElement: () => ({}),
  body: { classList: { toggle() {} } },
};
Object.defineProperty(globalThis, 'navigator', {
  configurable: true,
  value: { userAgent: 'node' },
});

const { usage, refreshUsage, loadUsageSummary, setExternalSources } = await import(
  '../src/usage/usage.js'
);

const windowSource = readFileSync(new URL('../src/usage/UsageWindow.vue', import.meta.url), 'utf8');
// 模板与样式都带大段解释「为什么这么写」的中文注释，注释里常原样引用被判据
// 的词——按整份文件找会把自己写的解释当成命中。只取模板段。
const template = windowSource.slice(
  windowSource.indexOf('<template>'),
  windowSource.indexOf('<style')
);

function reset() {
  calls.length = 0;
  nextData = null;
}

test('默认不勾选：概览卡片与窗口首读都不带任何外部来源', async () => {
  reset();
  nextData = { external_sources: { codex: false, minimax: false }, external: [] };
  await loadUsageSummary();
  assert.equal(calls.length, 1);
  assert.equal(calls[0].cmd, 'get_model_usage');
  assert.deepEqual(
    calls[0].args.external,
    { codex: false, minimax: false },
    '概览卡片显式传全 false：它不显示外部来源，也不该为它付扫描成本'
  );

  reset();
  await refreshUsage();
  assert.equal(calls[0].args.external, null, '省略 = 用后端已保存的偏好');
});

test('勾选状态的真值在 Rust 侧：前端那份只是镜像', async () => {
  reset();
  nextData = { external_sources: { codex: true, minimax: false }, external: [] };
  await refreshUsage();
  assert.deepEqual(
    usage.externalSources,
    { codex: true, minimax: false },
    '勾选状态从后端返回的 external_sources 回填，不在前端另存一份'
  );
});

test('勾选后先持久化再重扫，两步的顺序不能反', async () => {
  reset();
  nextData = { external_sources: { codex: true, minimax: false }, external: [] };
  await setExternalSources({ codex: true, minimax: false });
  assert.equal(calls.length, 2);
  assert.equal(calls[0].cmd, 'set_usage_external_sources');
  assert.deepEqual(calls[0].args.sources, { codex: true, minimax: false });
  assert.equal(calls[1].cmd, 'get_model_usage');
  assert.deepEqual(
    calls[1].args.external,
    { codex: true, minimax: false },
    '重扫时只扫刚勾上的那些来源'
  );
});

test('取消勾选也要落盘，并让重扫不再带那个来源', async () => {
  reset();
  nextData = { external_sources: { codex: false, minimax: false }, external: [] };
  await setExternalSources({ codex: false, minimax: false });
  assert.equal(calls[0].cmd, 'set_usage_external_sources');
  assert.deepEqual(calls[0].args.sources, { codex: false, minimax: false });
  assert.deepEqual(calls[1].args.external, { codex: false, minimax: false });
});

test('窗口模板上有两个 checkbox，且都接了勾选变化后的重扫', () => {
  const boxes = template.match(/<el-checkbox\b[\s\S]*?\/>/g) || [];
  assert.equal(boxes.length, 2, 'Codex 与 MiniMax Desktop 各一个');
  assert.ok(boxes.some((b) => b.includes('codexChecked')), 'Codex 的 checkbox');
  assert.ok(boxes.some((b) => b.includes('minimaxChecked')), 'MiniMax Desktop 的 checkbox');
  // 两个都必须在变化时触发重扫——只改本地状态不落盘，关掉窗口就丢。
  for (const box of boxes) {
    assert.match(box, /@change="onSourceToggle"/, '勾选变化必须走 onSourceToggle');
  }
  assert.match(template, /v-model="codexChecked"/);
  assert.match(template, /v-model="minimaxChecked"/);
});

test('外部来源在详情页里是独立一块，不与内核那份用量相加', () => {
  assert.match(template, /外部 Agent 工具用量/, '独立一块');
  assert.match(template, /口径与上面那份不同，不相加/, '说清为什么不相加');
  // 外部那一块**只读自己那份**的合计（`src.tokens`），不碰内核那份的
  // `rangeSummary`——两套口径加到一起没有意义。
  const start = template.indexOf('外部 Agent 工具用量');
  assert.ok(start > 0, '外部来源那一块必须存在');
  const section = template.slice(start);
  assert.doesNotMatch(section, /rangeSummary/, '外部用量不进内核那份的汇总');
  assert.match(section, /src\.tokens/, '外部那一块用自己的合计');
});
