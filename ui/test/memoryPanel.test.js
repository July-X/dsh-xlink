// 记忆面板的两条判据：注入插件的「未启用」提示不许误报，以及它确实接在模板上。
//
// 这两条是一件事的两半：`injectionDisabled` 是纯函数、测起来容易，而它**没被模板
// 用上**的话一切照旧——本仓为此栽过（helper 有测试、接线没有，纯函数测试全绿而
// 真机上功能没接）。所以下面第二条读模板原文，钉住那个 v-if。
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const read = (p) => readFileSync(new URL(p, import.meta.url), 'utf8');

// `memory.js` 顶层 import 了 `bridge.js`，后者在模块求值期就读 `window.__TAURI__`
// （与 builtinPlugins.test.js 同一套桩）。
globalThis.window = {
  __TAURI__: { core: { invoke: () => Promise.resolve([]), Channel: class { onmessage = null; } } },
  addEventListener() {},
  removeEventListener() {},
};
Object.defineProperty(globalThis, 'navigator', { value: { userAgent: 'node' }, configurable: true });

/**
 * `.vue` 的模板块，HTML 注释已剥。
 *
 * 必须剥注释：本仓的模板里正是一段解释「为什么要判 `injectPlugin`」的中文注释，
 * 里面原样引用了被判的那个表达式——按整份文件找会把自己写的解释当成命中。
 */
function templateOf(src) {
  const start = src.indexOf('<template>');
  assert.ok(start >= 0, '找不到 <template>');
  const styleAt = src.indexOf('<style', start);
  const end = styleAt > start ? styleAt : src.length;
  return src.slice(start, end).replace(/<!--[\s\S]*?-->/g, '');
}

test('状态还没读回来时不算「没启用」——那句提示会误报', async () => {
  const { injectionDisabled } = await import('../src/memory/memory.js');
  // null = `builtin_status` 还没回来（或这次读失败）：不知道就不说。
  assert.equal(injectionDisabled(null), false, '不知道就不能说它没启用');
  assert.equal(injectionDisabled(undefined), false);
  // 读回来了：按启用意图与状态文件是否损坏判。
  assert.equal(injectionDisabled({ requestedEnabled: true }), false);
  assert.equal(injectionDisabled({ requestedEnabled: false }), true);
  assert.equal(
    injectionDisabled({ requestedEnabled: true, stateError: '状态文件异常' }),
    true,
    '状态文件坏了时开关拨不动，这条提示是对的'
  );
});

test('面板的告警 v-if 用这个判据，而不是只判 !injectReady', () => {
  const tpl = templateOf(read('../src/memory/MemoryPanel.vue'));
  assert.match(
    tpl,
    /v-if="enabled && injectionDisabled\(injectPlugin\)"/,
    '告警必须在「确定没启用」时才出现'
  );
  assert.doesNotMatch(
    tpl,
    /!injectReady/,
    '只判 `!injectReady` 会在状态读回来之前挂一句假警报（读失败时永久挂着）'
  );
});
