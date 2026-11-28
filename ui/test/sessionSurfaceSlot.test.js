import assert from 'node:assert/strict';
import test from 'node:test';
import { createSessionSlotWriter, HIDDEN_SESSION_SLOT, sessionSlotRequest } from '../src/shell/sessionSurfaceSlot.js';

test('会话坐标按逻辑像素向内收齐，离开视口时请求隐藏', () => {
  const viewport = { width: 800, height: 600 };
  assert.deepEqual(sessionSlotRequest('official-chat', { left: 174.5, top: 104.25, right: 801, bottom: 599.75 }, viewport), {
    kind: 'official-chat', bounds: { x: 175, y: 105, width: 625, height: 494 },
  });
  assert.equal(sessionSlotRequest('harness', { left: 810, top: 100, right: 920, bottom: 500 }, viewport), HIDDEN_SESSION_SLOT);
});

test('原生坐标 IPC 在途时，切页隐藏覆盖排队的旧坐标', async () => {
  const sent = [];
  let release;
  const setSlot = createSessionSlotWriter((slot) => {
    sent.push(slot);
    if (sent.length === 1) return new Promise((resolve) => { release = resolve; });
  });
  const original = { kind: 'harness', bounds: { x: 200, y: 120, width: 560, height: 420 } };
  const first = setSlot(original);
  await setSlot({ ...original, bounds: { ...original.bounds, x: 180 } });
  await setSlot(HIDDEN_SESSION_SLOT);
  assert.deepEqual(sent, [original]);
  release();
  await first;
  await setSlot(HIDDEN_SESSION_SLOT);
  assert.deepEqual(sent, [original, HIDDEN_SESSION_SLOT]);
});
