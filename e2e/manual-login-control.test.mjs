import assert from 'node:assert/strict';
import { test } from 'node:test';
import { classifyWorkbench, parseLoginAcknowledgment } from './manual-login-control.mjs';

test('人工登录验收只接受预定义状态，不接收账号、凭据或地址', () => {
  const fields = ['login', 'callback'];
  assert.deepEqual(parseLoginAcknowledgment('\uFEFF{"login":"pass","callback":"not_run"}', fields),
    { login: 'pass', callback: 'not_run' });
  for (const raw of [
    '{"login":"pass"}',
    '{"login":"pass","callback":"pass","token":"secret"}',
    '{"login":"https://example.test/?token=secret","callback":"pass"}',
    '{"login":"pass","callback":"ok"}',
    'x'.repeat(257),
  ]) assert.throws(() => parseLoginAcknowledgment(raw, fields));
});

test('区分工作台已关闭、驱动未枚举与服务停止，不推断关闭原因', () => {
  const handles = ['main', 'mcode-workbench'];
  assert.equal(classifyWorkbench({ running: true, window_open: true }, handles), 'ready');
  assert.equal(classifyWorkbench({ running: true, window_open: true }, ['main']), 'driver_missing');
  assert.equal(classifyWorkbench({ running: true, window_open: false }, ['main']), 'closed');
  assert.equal(classifyWorkbench({ running: true, window_open: false }, handles), 'inconsistent');
  assert.equal(classifyWorkbench({ running: false, window_open: false }, ['main']), 'service_stopped');
  assert.throws(() => classifyWorkbench({ running: true }, handles));
});
