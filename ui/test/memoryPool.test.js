import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readFileSync } from 'node:fs';
let handler;
const calls = [];
Object.defineProperty(globalThis, 'navigator', { value: { userAgent: 'node' }, configurable: true });
globalThis.window = { navigator: { userAgent: 'node' }, addEventListener() {}, removeEventListener() {}, __TAURI__: { core: { invoke: async (command, args) => { calls.push([command, args]); return handler(command, args); } } } };
const { memoryPool, resetMemoryPool, refreshMemoryPool, requestPoolTarget, confirmPoolTarget, cancelPoolTarget,
  poolReview, resetPoolReview, refreshPoolReview, decidePoolReview, reviewKey } = await import('../src/memory/pool.js');
const status = { pair_key: 'pair-a', sources: [{ label: 'DSH', namespace: 'dsh/a/default' }, { label: 'MiniMax', namespace: 'mcode/b/default' }] };
const readyPlugin = { id: 'memory', displayName: '本地记忆', requestedEnabled: true, wired: true, loadState: 'prepared' };
const target = key => ({ target: key, effective: false, pool_enabled: false, needs_consent: true });
function setup() {
  resetMemoryPool(); resetPoolReview(); calls.length = 0;
  handler = command => command === 'memory_target_use_status' ? [target('dsh'), target('mcode')]
    : command === 'builtin_status' ? [readyPlugin]
    : command === 'memory_pool_review_list' ? { candidates: [], legacy_records: [] } : status;
}
test('第一次启用固定确认来源，取消不写，确认只写目标开关', async () => {
  setup(); await refreshMemoryPool(); calls.length = 0;
  requestPoolTarget('dsh', true); cancelPoolTarget(); await confirmPoolTarget(); assert.equal(calls.length, 0);
  requestPoolTarget('mcode', true); memoryPool.status = { ...status, pair_key: 'new' }; await confirmPoolTarget();
  assert.deepEqual(calls[0], ['memory_set_pool_target', { target: 'mcode', enabled: true, expectedPair: 'pair-a' }]);
  assert.equal(calls.filter(([command]) => command.startsWith('memory_set')).length, 1);
});
test('来源已授权后再次启用不重复确认，关闭不需要有效来源', async () => {
  setup(); await refreshMemoryPool(); memoryPool.targets[0].needs_consent = false;
  await requestPoolTarget('dsh', true); assert.equal(memoryPool.confirmation, null);
  assert.ok(calls.some(([command, args]) => command === 'memory_set_pool_target' && args.enabled));
  memoryPool.status = null; await requestPoolTarget('dsh', false);
  assert.ok(calls.some(([command, args]) => command === 'memory_set_pool_target' && args.expectedPair === '' && !args.enabled));
});
test('刷新配对变化时清空状态，不能混合来源快照', async () => {
  setup(); let n = 0;
  handler = command => command === 'memory_target_use_status' ? [target('dsh')] : command === 'builtin_status' ? [readyPlugin] : { ...status, pair_key: ++n === 1 ? 'a' : 'b' };
  await refreshMemoryPool(); assert.equal(memoryPool.status, null); assert.match(memoryPool.error, /来源已改变/);
});
test('启用失败不显示为成功，并允许刷新恢复', async () => {
  setup(); await refreshMemoryPool(); requestPoolTarget('dsh', true);
  handler = () => { throw Error('disk'); }; await confirmPoolTarget();
  assert.equal(memoryPool.status, null); assert.deepEqual(memoryPool.targets, []); assert.match(memoryPool.error, /状态未确认/);
  handler = command => command === 'memory_target_use_status' ? [target('dsh')] : command === 'builtin_status' ? [readyPlugin] : status;
  await refreshMemoryPool(); assert.equal(memoryPool.error, ''); assert.equal(memoryPool.status.pair_key, 'pair-a');
});
test('重复点击不重复写入，离页迟到响应不恢复旧状态', async () => {
  setup(); await refreshMemoryPool(); let resolve;
  handler = () => new Promise(done => { resolve = done; });
  const first = requestPoolTarget('dsh', false); await requestPoolTarget('dsh', false);
  assert.equal(calls.filter(([command]) => command === 'memory_set_pool_target').length, 1);
  resetMemoryPool(); resolve([]); await first; assert.equal(memoryPool.status, null); assert.equal(memoryPool.changing, '');
});
test('候选审核按来源隔离同名 id，旧记录提交版本避免覆盖并发修改', async () => {
  setup(); const a = { source_namespace: 'dsh/a/default', candidate: { id: 'same' } };
  const b = { source_namespace: 'mcode/b/default', record: { id: 'same', revision: 4 } };
  assert.notEqual(reviewKey(a), reviewKey(b));
  await decidePoolReview(a, true); await decidePoolReview(b, false);
  assert.deepEqual(calls.find(([command]) => command === 'memory_pool_decide_candidate')[1], { sourceNamespace: a.source_namespace, id: 'same', accept: true, scope: null });
  assert.deepEqual(calls.find(([command]) => command === 'memory_pool_review_legacy')[1], { sourceNamespace: b.source_namespace, id: 'same', expectedRevision: 4, accept: false });
});
test('审核失败保留队列与错误，离页迟到列表不能恢复', async () => {
  setup(); const a = { source_namespace: 'dsh/a/default', candidate: { id: 'a' } };
  poolReview.candidates = [a]; handler = () => { throw Error('stale'); };
  assert.equal(await decidePoolReview(a, true), false); assert.equal(poolReview.candidates.length, 1); assert.match(poolReview.error, /审核未完成/);
  let resolve; handler = () => new Promise(done => { resolve = done; }); const pending = refreshPoolReview(); resetPoolReview();
  resolve({ candidates: [a], legacy_records: [] }); await pending; assert.deepEqual(poolReview.candidates, []);
});
test('新 UI 只有目标开关，审核和已生效池分开，候选保留来源', () => {
  const read = name => readFileSync(new URL(`../src/memory/${name}`, import.meta.url), 'utf8');
  const header = read('MemoryModulePanel.vue');
  assert.equal((header.match(/<el-switch/g) || []).length, 1, '由两项循环生成唯一类型的开关');
  assert.match(header, /mcodeExperience.value/); assert.match(header, /沿用旧范围/);
  assert.doesNotMatch(header, /双向普通记忆互通|原有实例接入|原有记忆与自动提取/);
  assert.match(header, /首次启用后需要重启/);
  const panel = read('MemoryPanel.vue');
  assert.match(panel, /记忆池/); assert.match(panel, /<PoolReviewList/); assert.match(panel, /verification === 'verified'/);
  assert.doesNotMatch(panel, /@share=|:share-available=/);
  const review = read('PoolReviewList.vue'); assert.match(review, /item.origin_label/); assert.match(review, /旧版未审核记录/); assert.match(review, /审核通过/);
});

test('一个 DSH 开关先完成插件接入再保存池，关闭不删除插件', async () => {
  setup(); let plugin = { ...readyPlugin, wired: false, requestedEnabled: false };
  handler = command => {
    if (command === 'builtin_status') return [plugin];
    if (command === 'builtin_set_enabled') { plugin = readyPlugin; return plugin; }
    if (command === 'memory_target_use_status') return [target('dsh')];
    return status;
  };
  await refreshMemoryPool(); assert.equal(memoryPool.dshConnection, 'missing');
  requestPoolTarget('dsh', true); await confirmPoolTarget();
  const connect = calls.findIndex(([command]) => command === 'builtin_set_enabled');
  const authorize = calls.findIndex(([command]) => command === 'memory_set_pool_target');
  assert.ok(connect >= 0 && authorize > connect);
  assert.deepEqual(calls[connect][1], { plugin: 'memory', enabled: true });
  assert.match(memoryPool.connectionNote, /下次启动/);
  calls.length = 0; await requestPoolTarget('dsh', false);
  assert.equal(calls.some(([command]) => command === 'builtin_set_enabled'), false);
});
test('运行中插件接入被拒绝时不继续授权并保留原因', async () => {
  setup(); handler = command => {
    if (command === 'builtin_status') return [{ ...readyPlugin, wired: false, requestedEnabled: false }];
    if (command === 'builtin_set_enabled') throw Error('请先停止工作台');
    if (command === 'memory_target_use_status') return [target('dsh')];
    return status;
  };
  await refreshMemoryPool(); requestPoolTarget('dsh', true); await confirmPoolTarget();
  assert.equal(calls.some(([command]) => command === 'memory_set_pool_target'), false);
  assert.match(memoryPool.error, /请先停止工作台/);
});
test('确认来源已变时不接入插件，不写旧来源许可', async () => {
  setup(); await refreshMemoryPool(); requestPoolTarget('dsh', true); calls.length = 0;
  handler = () => ({ ...status, pair_key: 'new-pair' }); await confirmPoolTarget();
  assert.deepEqual(calls.map(([command]) => command), ['memory_module_status']);
  assert.match(memoryPool.error, /来源已改变/);
});
