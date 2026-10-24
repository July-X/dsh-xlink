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

// --- 跨语言契约：模板读的每个字段，后端都得真有 --------------------------------
//
// 上一条只钉住「模板里出现了 `src.tokens`」，**没钉住后端真有这个字段**。
// 后端把 `tokens` 改名成 `total`，模板不会报任何错——`{{ formatTokens(undefined) }}`
// 渲染成 "0"，界面看着「用量是 0」，实则字段对不上。这是**假门禁**：判据绿着，
// 功能已经坏了。
//
// 判据取「前端读的字段 ⊆ 后端结构体的字段」这一个方向：前端少读一个字段没关系
// （可能是后端多给了），但**前端读的那个字段后端必须真有**。
test('模板读的每个外部字段，后端结构体里都得真有', () => {
  const rustPath = new URL('../../src-tauri/src/usage/external/mod.rs', import.meta.url);
  const rust = readFileSync(rustPath, 'utf8');

  // 取一个结构体的 `pub <字段>:` 列表。
  function rustFields(structName) {
    const begin = rust.indexOf(`pub struct ${structName} {`);
    assert.ok(begin > 0, `后端得真有 ${structName} 结构体`);
    const body = rust.slice(begin, rust.indexOf('\n}', begin));
    const fields = new Set();
    for (const m of body.matchAll(/pub\s+(\w+)\s*:/g)) fields.add(m[1]);
    return fields;
  }

  const tool = rustFields('ExternalToolUsage');
  const model = rustFields('ExternalModelUsage');
  const day = rustFields('ExternalDayUsage');

  // 模板里外部来源那一块（`externalList` 那个 section；它内部没有嵌套 section，
  // 所以第一个 </section> 就是它的收尾）。
  const secStart = template.indexOf('<section v-if="externalList.length"');
  assert.ok(secStart > 0, '外部来源那一块必须存在');
  const section = template.slice(secStart, template.indexOf('</section>', secStart));

  // 模板里出现的 `src.X` / `m.X` / `row.X`。
  function accesses(prefix) {
    return new Set([...section.matchAll(new RegExp(`\\b${prefix}\\.(\\w+)`, 'g'))].map((m) => m[1]));
  }

  // `row.*` 来自 externalDays()：它把 `source.days`（ExternalDayUsage）映射成
  // {date, tokens, requests, height}——height 是前端自己算的，不在后端。
  const dayExtra = new Set(['height']);

  for (const [prefix, fields, structName] of [
    ['src', tool, 'ExternalToolUsage'],
    ['m', model, 'ExternalModelUsage'],
    ['row', day, 'ExternalDayUsage'],
  ]) {
    for (const field of accesses(prefix)) {
      if (prefix === 'row' && dayExtra.has(field)) continue;
      assert.ok(
        fields.has(field),
        `模板读了 ${prefix}.${field}，但后端 ${structName} 没有这个字段——` +
          '字段对不上时界面渲染成 0 且不报任何错'
      );
    }
  }

  // 反向也钉一下：后端给了的这几个关键字段，模板必须真的用上（否则多半是
  // 模板写错了变量名，只是碰巧没崩）。
  for (const field of ['source', 'label', 'tokens', 'requests', 'active_days', 'tracked_files', 'note']) {
    assert.ok(accesses('src').has(field), `模板没读 src.${field}——后端给了却没用`);
  }
});
