// 四组试点 IPC 契约的判据（2026-11 OPT-12）。
//
// 契约本体只有 contracts/ipc-contract.json 一份；这里同时钉住：
//   ① 正常请求/响应能通过、试点命令都在同一份契约里；
//   ② 改动参数名、必需字段或枚举后判据必须失败（反向验证）；
//   ③ contract / transport / business 三类失败能区分；
//   ④ 空数据（0 / []）是合法响应，不把「没有数据」当失败。
import assert from 'node:assert/strict';
import test from 'node:test';
import contract from '../../contracts/ipc-contract.json' with { type: 'json' };
import {
  IpcCallError,
  IpcContractError,
  classifyIpcFailure,
  errorCode,
  validateRequest,
  validateResponse,
} from '../src/shell/ipc-schema.js';
import { memoryListView, startReport, statusView, usageView } from './lib/ipc-fixtures.mjs';

const clone = () => JSON.parse(JSON.stringify(contract));

test('契约覆盖四组试点且每条命令都有来源', () => {
  assert.deepEqual(Object.keys(contract.commands).sort(), [
    'get_model_usage',
    'get_status',
    'memory_list',
    'start_kernel',
    'stop_kernel',
  ]);
  const groups = new Set(Object.values(contract.commands).map((spec) => spec.group));
  assert.deepEqual([...groups].sort(), ['kernel_lifecycle', 'memory_list', 'status', 'usage_query']);
  for (const [command, spec] of Object.entries(contract.commands)) {
    assert.ok(spec.source && spec.source.rust, `${command} 缺 Rust DTO 来源`);
    assert.ok(spec.source.consumer, `${command} 缺前端消费方`);
  }
  for (const kind of ['transport', 'business', 'contract']) {
    assert.ok(contract.errorCodes[kind] && contract.errorCodes[kind].code, `${kind} 缺错误码`);
  }
});

test('正常请求与响应通过契约（含空数据）', () => {
  validateRequest(contract, 'get_status', { source: 'refresh' });
  validateRequest(contract, 'start_kernel', { onEvent: {} });
  validateRequest(contract, 'stop_kernel', {});
  validateRequest(contract, 'memory_list', {
    filter: { scope: '', include_disabled: true, include_candidates: true },
  });
  validateRequest(contract, 'get_model_usage', {
    force: true,
    external: { codex: false, minimax: false },
    externalForce: false,
  });

  validateResponse(contract, 'get_status', statusView());
  validateResponse(contract, 'start_kernel', startReport());
  validateResponse(contract, 'stop_kernel', null);
  validateResponse(contract, 'memory_list', memoryListView());
  validateResponse(contract, 'get_model_usage', usageView());
});

test('缺少必需字段 / 参数名漂移 / 枚举漂移都会失败（反向验证）', () => {
  const renamedRequired = clone();
  renamedRequired.commands.get_status.response.required[0] = 'shellVersion';
  assert.throws(
    () => validateResponse(renamedRequired, 'get_status', statusView()),
    (error) => error instanceof IpcContractError && /shellVersion/.test(error.message),
    '把必需字段改名后必须失败'
  );

  const renamedRequest = clone();
  delete renamedRequest.commands.get_model_usage.request.properties.force;
  assert.throws(
    () => validateRequest(renamedRequest, 'get_model_usage', { force: true }),
    (error) => error instanceof IpcContractError && /force/.test(error.message),
    '请求参数名漂移后必须失败'
  );

  // FIX-08：Tauri command 参数的线上名是 camelCase `externalForce`，Rust 的
  // `external_force` 只是本地变量名。契约若写成 snake_case，前端会照着发，
  // 后端却按 `externalForce` 找参数 → 拿到 None → 强制刷新静默退化成吃缓存。
  assert.throws(
    () => validateRequest(contract, 'get_model_usage', { external_force: true }),
    (error) => error instanceof IpcContractError && /external_force/.test(error.message),
    'wire name 必须是 externalForce：snake_case 必须被契约拒绝'
  );

  const changedEnum = clone();
  changedEnum.commands.get_status.response.properties.shell_mode.enum = ['staging'];
  assert.throws(
    () => validateResponse(changedEnum, 'get_status', statusView({ shell_mode: 'dev', dev_build: true })),
    (error) => error instanceof IpcContractError && /枚举/.test(error.message),
    '枚举漂移后必须失败'
  );

  const missingField = clone();
  const data = statusView();
  delete data.kernel.port;
  assert.throws(
    () => validateResponse(contract, 'get_status', data),
    (error) => error instanceof IpcContractError && /kernel\.port/.test(error.message),
    '缺少必需字段必须失败'
  );

  const extraData = memoryListView({ extra: 'drift' });
  assert.throws(
    () => validateResponse(contract, 'memory_list', extraData),
    (error) => error instanceof IpcContractError && /extra/.test(error.message),
    '后端多出契约未声明字段时也要失败'
  );
});

test('失败分为 contract / transport / business，错误码来自同一份契约', () => {
  assert.equal(classifyIpcFailure(new IpcContractError('x', 'bad'), true), 'contract');
  assert.equal(classifyIpcFailure(new Error('network down'), false), 'transport');
  assert.equal(classifyIpcFailure(new Error('业务失败'), true), 'business');
  assert.equal(errorCode(contract, 'transport'), 'ipc.transport');
  assert.equal(errorCode(contract, 'business'), 'ipc.business');
  assert.equal(errorCode(contract, 'contract'), 'ipc.contract');

  const business = new IpcCallError('get_status', 'business', 'ipc.business', new Error('数据目录不可读'));
  assert.equal(business.kind, 'business');
  assert.equal(business.code, 'ipc.business');
  assert.equal(business.message, '数据目录不可读');
});

test('可选字段可缺席、可空字段可为 null，不算失败', () => {
  validateResponse(contract, 'start_kernel', startReport({ incident: null }));
  validateResponse(contract, 'get_status', statusView({ last_incident: null, kernel: { active: null } }));
  validateResponse(
    contract,
    'get_model_usage',
    usageView({ top_model: null, external: [], days: [] })
  );
});
