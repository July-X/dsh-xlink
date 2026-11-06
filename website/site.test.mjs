import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { runInNewContext } from 'node:vm';

test('theme comparison updates the split and accessible value', () => {
  const values = {};
  const range = {
    value: '50',
    setAttribute(name, value) { values[name] = value; },
    addEventListener(name, handler) { values[name] = handler; },
  };
  const compare = {
    querySelector: () => range,
    style: { setProperty(name, value) { values[name] = value; } },
  };
  const document = { querySelectorAll: () => [compare] };
  runInNewContext(readFileSync(new URL('./site.js', import.meta.url), 'utf8'), { document });
  assert.equal(values['--split'], '50%');
  for (const light of [0, 37, 100]) {
    range.value = String(light);
    values.input();
    assert.equal(values['--split'], `${light}%`);
    assert.equal(values['aria-valuetext'], `浅色界面 ${light}%，深色界面 ${100 - light}%`);
  }
});
