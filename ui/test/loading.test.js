import assert from 'node:assert/strict';
import test from 'node:test';

const deferred = () => {
  let resolve;
  const promise = new Promise((res) => {
    resolve = res;
  });
  return { promise, resolve };
};

test('exclusive lease rejects overlap and releases after completion', async () => {
  const { globalBusy, isExclusiveBusy, withExclusive, withExclusiveLoading, isLoading } = await import('../src/loading.js');
  const gate = deferred();
  const first = withExclusiveLoading('test-exclusive', () => gate.promise);

  assert.equal(isExclusiveBusy(), true);
  assert.equal(globalBusy.value, true);
  assert.equal(isLoading('test-exclusive'), true);
  assert.equal(withExclusive(() => Promise.resolve('ignored')), undefined);

  gate.resolve('done');
  assert.equal(await first, 'done');
  assert.equal(isExclusiveBusy(), false);
  assert.equal(globalBusy.value, false);
  assert.equal(isLoading('test-exclusive'), false);

  await assert.rejects(withExclusive(() => Promise.reject(new Error('expected'))), /expected/);
  assert.equal(isExclusiveBusy(), false);
  assert.equal(globalBusy.value, false);
});
