import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';

globalThis.window = {
  __TAURI__: { core: { invoke: () => Promise.resolve([]), Channel: class {} } },
  addEventListener() {}, removeEventListener() {},
};
Object.defineProperty(globalThis, 'navigator', { value: { userAgent: 'node' }, configurable: true });

const { memoryStore, visibleMemories, openMemoryEditor, closeMemoryEditor, canSaveDraft, saveMemory } = await import('../src/memory/memory.js');

test('候选点击与空格处理器执行时读取当前 props，嵌套动作不触发详情', () => {
  const source = readFileSync(new URL('../src/memory/MemoryCandidateRow.vue', import.meta.url), 'utf8');
  const functions = [...source.matchAll(/function (?:openFromEvent|openFromSpace)\(event\) \{[\s\S]*?\n\}/g)].map(([body]) => body);
  assert.equal(functions.length, 2);
  const props = { item: { id: 'first' } };
  const emitted = [];
  const handlers = new Function('props', 'emit', functions.join('\n') + '\nreturn {openFromEvent,openFromSpace};')(props, (...args) => emitted.push(args));
  let prevented = 0;
  const event = { target: { closest: () => null }, preventDefault() { prevented++; } };
  handlers.openFromEvent(event);
  props.item = { id: 'current' };
  handlers.openFromSpace(event);
  assert.deepEqual(emitted, [['open', { id: 'first' }], ['open', { id: 'current' }]]);
  assert.equal(prevented, 1);
  handlers.openFromSpace({ target: { closest: () => ({}) }, preventDefault() { throw new Error('不应拦截按钮空格'); } });
  assert.equal(emitted.length, 2);
});

test('交接与经验视图不混入记忆列表，活动交接排在归档之前', () => {
  const view = {
    memories: [{ id: 'm', scope: 'general' }],
    handoffs: [{ id: 'h1', state: 'archived' }, { id: 'h2', state: 'active' }],
    experiences: [{ id: 'e', situation: '并发' }],
  };
  assert.deepEqual(visibleMemories(view, 'all'), view.memories);
  assert.deepEqual(visibleMemories(view, 'handoff').map((row) => row.id), ['h2', 'h1']);
  assert.equal(visibleMemories(view, 'experience')[0].kind, 'experience');
  assert.deepEqual(visibleMemories(null, 'handoff'), []);
});

test('同一编辑器能往原有保存命令提交完整交接和经验，超长字段在提交前拦住', async () => {
  const original = window.__TAURI__.core.invoke;
  let captured;
  window.__TAURI__.core.invoke = async (command, args) => {
    if (command === 'memory_save') { captured = args.input; return { ...args.input, id: 'saved' }; }
    if (command === 'memory_list') return { status: {}, memories: [], candidates: [], experiences: [], handoffs: [] };
    return [];
  };
  try {
    openMemoryEditor(null, 'handoff');
    memoryStore.projectPath = '/project-a';
    Object.assign(memoryStore.draft, { title: '交接', body: '进度', goal: '交付', doneText: '一\n二', todoText: '待办', blockedText: '' });
    assert.equal(canSaveDraft(), true);
    await saveMemory().catch(() => undefined);
    assert.equal(captured.kind, 'handoff');
    assert.equal(captured.scope, 'project');
    assert.deepEqual(captured.done, ['一', '二']);
    assert.deepEqual(captured.blocked, []);
    openMemoryEditor(null, 'experience');
    Object.assign(memoryStore.draft, { title: '经验', situation: '并发', action: '加锁', outcome: '无丢失', lesson: '整个事务加锁' });
    assert.equal(memoryStore.draft.scope, 'general');
    assert.equal(canSaveDraft(), true);
    memoryStore.draft.lesson = '中'.repeat(1000);
    assert.equal(canSaveDraft(), false);
    memoryStore.draft.lesson = '整个事务加锁';
    await saveMemory().catch(() => undefined);
    assert.equal(captured.kind, 'experience');
    assert.equal(captured.situation, '并发');
    assert.equal(captured.workspace_path, '');
    openMemoryEditor({ ...captured, id: 'e', revision: 3, keywords: [], kind: 'experience' });
    assert.equal(memoryStore.draft.expectedRevision, 3);
    assert.equal(memoryStore.draft.lesson, '整个事务加锁');
  } finally {
    closeMemoryEditor();
    window.__TAURI__.core.invoke = original;
  }
});
