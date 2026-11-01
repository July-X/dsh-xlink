// IPC 契约的通用校验器（2026-11 OPT-12）。
//
// 契约本体只有 contracts/ipc-contract.json 一份：Rust 侧 ipc_contract.rs 序列化真实
// DTO 对比它，前端 ipc-contract.js 用它校验请求/响应，e2e 用例读同一份文件。
// 这里不 import JSON，便于 Node 测试与 E2E 在没有构建器的环境直接复用：
//   validateRequest(contract, command, args) / validateResponse(contract, command, data)
//
// 语法是刻意的子集：type / required / properties / additionalProperties / items /
// enum / minimum / maximum / optional / channel。不是 JSON Schema 实现，够这四组用。

export class IpcContractError extends Error {
  constructor(command, message) {
    super(`IPC 契约校验失败（${command}）：${message}`);
    this.name = 'IpcContractError';
    this.kind = 'contract';
    this.command = command;
  }
}

export class IpcCallError extends Error {
  constructor(command, kind, code, cause) {
    const detail = cause && cause.message ? cause.message : String(cause);
    super(kind === 'transport' ? detail : `${detail}`);
    this.name = 'IpcCallError';
    this.kind = kind;
    this.code = code;
    this.command = command;
    this.cause = cause;
  }
}

function valueType(value) {
  if (value === null) return 'null';
  if (Array.isArray(value)) return 'array';
  if (typeof value === 'number' && Number.isInteger(value)) return 'integer';
  return typeof value;
}

function matchesType(expected, value) {
  const actual = valueType(value);
  if (expected === 'number') return actual === 'number' || actual === 'integer';
  if (expected === 'object') return actual === 'object';
  if (expected === 'channel') return actual === 'object' || actual === 'function';
  return actual === expected;
}

function fail(command, path, message) {
  throw new IpcContractError(command, `${path}: ${message}`);
}

export function validateSchema(schema, value, path, command) {
  const types = Array.isArray(schema.type) ? schema.type : [schema.type];
  if (!types.some((type) => matchesType(type, value))) {
    fail(command, path, `期望 ${types.join(' | ')}，实际 ${valueType(value)}`);
  }
  if (Array.isArray(schema.enum) && !schema.enum.includes(value)) {
    fail(command, path, `期望枚举 ${schema.enum.join(' / ')}，实际 ${JSON.stringify(value)}`);
  }
  if (schema.type === 'integer' || types.includes('integer')) {
    if (typeof schema.minimum === 'number' && value < schema.minimum) fail(command, path, `小于最小值 ${schema.minimum}`);
    if (typeof schema.maximum === 'number' && value > schema.maximum) fail(command, path, `大于最大值 ${schema.maximum}`);
  }
  if (types.includes('object') && valueType(value) === 'object') {
    const properties = schema.properties || {};
    for (const key of schema.required || []) {
      if (!(key in value)) fail(command, `${path}.${key}`, '缺少必需字段');
    }
    if (schema.additionalProperties === false) {
      for (const key of Object.keys(value)) {
        if (!(key in properties)) fail(command, `${path}.${key}`, '契约未声明的额外字段');
      }
    }
    for (const [key, child] of Object.entries(properties)) {
      if (key in value) validateSchema(child, value[key], `${path}.${key}`, command);
    }
  }
  if (types.includes('array') && Array.isArray(value) && schema.items) {
    value.forEach((item, index) => validateSchema(schema.items, item, `${path}[${index}]`, command));
  }
}

function commandSpec(contract, command) {
  const spec = contract && contract.commands && contract.commands[command];
  if (!spec) throw new IpcContractError(command, '契约里没有这个命令');
  return spec;
}

export function validateRequest(contract, command, args) {
  const spec = commandSpec(contract, command);
  validateSchema(spec.request, args === undefined ? {} : args, `${command}.request`, command);
}

export function validateResponse(contract, command, data) {
  const spec = commandSpec(contract, command);
  validateSchema(spec.response, data, `${command}.response`, command);
}

/// 把失败归为契约 / 传输 / 业务三类。桥接未注入时 invoke 根本没到 Rust，必须先归类；
/// 其余 invoke 拒绝都是后端 Err(String)，保留原文给用户读。
export function classifyIpcFailure(error, bridgeAvailable) {
  if (error && error.kind === 'contract') return 'contract';
  if (!bridgeAvailable) return 'transport';
  return 'business';
}

export function errorCode(contract, kind) {
  return contract && contract.errorCodes && contract.errorCodes[kind] ? contract.errorCodes[kind].code : 'ipc.' + kind;
}
