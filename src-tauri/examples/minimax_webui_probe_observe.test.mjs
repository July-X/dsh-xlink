import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { runInNewContext } from 'node:vm';

const script = readFileSync(new URL('./minimax_webui_probe_observe.js', import.meta.url), 'utf8');

function scene() {
  const state = { phase: undefined, competingDialog: false, dialogVisible: true, focusInside: true, keys: [] };
  const phaseElement = { dataset: {} };
  Object.defineProperty(phaseElement.dataset, 'webuiAccountLoginPhase', { get: () => state.phase });
  const dialog = {
    getClientRects: () => state.dialogVisible ? [1] : [],
    querySelector: () => phaseElement,
    contains: () => state.focusInside,
  };
  const window = { WebSocket: class extends EventTarget {
    constructor(url) { super(); this.url = url; }
  } };
  window.top = window;
  let onMutation;
  runInNewContext(script, {
    URL, window, location: new URL('http://127.0.0.1:40001'),
    MutationObserver: class { constructor(callback) { onMutation = callback; } observe() {} },
    localStorage: { getItem: () => 'system' }, matchMedia: () => ({ matches: true }),
    KeyboardEvent: class { constructor(type, options) { Object.assign(this, options, { type }); } },
    document: {
      querySelector: (selector) => selector === '#webui-root' ? { childElementCount: 1 } : dialog,
      querySelectorAll: () => state.competingDialog ? [dialog, { getClientRects: () => [1] }] : [dialog],
      dispatchEvent: (event) => state.keys.push(event.key),
      activeElement: {},
    },
  });
  return { state, sample: window.__mcodeProbe, changed: (records = []) => onMutation(records), phaseElement, WebSocket: window.WebSocket };
}

test('a same-origin frame still proves connection after the socket becomes idle', () => {
  const { sample, WebSocket } = scene();
  const foreign = new WebSocket('ws://127.0.0.1:40002');
  foreign.dispatchEvent(new Event('message'));
  assert.equal(sample().connected, false);
  const owned = new WebSocket('ws://127.0.0.1:40001');
  owned.dispatchEvent(new Event('message'));
  owned.dispatchEvent(new Event('close'));
  assert.equal(sample().connected, true);
});

test('a fresh successful login dismisses its overlay once', () => {
  const { state, sample, changed } = scene();
  state.phase = 'awaiting';
  changed();
  state.phase = 'authenticated';
  changed();
  assert.equal(sample().loginDismissRequested, true);
  assert.equal(sample().loginDismissed, false);
  state.dialogVisible = false;
  changed();
  assert.equal(sample().loginDismissed, true);
  sample();
  assert.deepEqual(state.keys, ['Escape']);
});

test('an existing login or a competing modal never receives automatic Escape', () => {
  const { state, sample } = scene();
  state.phase = 'authenticated';
  sample();
  state.phase = 'opening';
  sample();
  state.competingDialog = true;
  state.phase = 'authenticated';
  assert.equal(sample().loginDismissRequested, false);
  assert.deepEqual(state.keys, []);
});

test('a batched pending-to-authenticated change still closes the login overlay once', () => {
  const { state, sample, changed, phaseElement } = scene();
  state.phase = 'awaiting';
  sample();
  state.phase = 'authenticated';
  sample();
  changed([{ type: 'attributes', target: phaseElement, oldValue: 'awaiting' }]);
  assert.equal(sample().loginDismissRequested, true);
  assert.deepEqual(state.keys, ['Escape']);
});

test('a batched cancellation or focus outside the login overlay never dispatches Escape', () => {
  const { state, sample, changed, phaseElement } = scene();
  state.phase = 'awaiting';
  sample();
  state.phase = 'authenticated';
  changed([
    { type: 'attributes', target: phaseElement, oldValue: 'awaiting' },
    { type: 'attributes', target: phaseElement, oldValue: 'signed-out' },
  ]);
  assert.equal(sample().loginDismissRequested, false);
  state.phase = 'awaiting';
  sample();
  state.focusInside = false;
  state.phase = 'authenticated';
  changed([{ type: 'attributes', target: phaseElement, oldValue: 'awaiting' }]);
  assert.equal(sample().loginDismissRequested, false);
  assert.deepEqual(state.keys, []);
});
