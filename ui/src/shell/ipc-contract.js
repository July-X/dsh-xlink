// 四组高频 IPC 的单一契约入口（2026-11 OPT-12）。
//
// 契约本体：contracts/ipc-contract.json（Rust DTO 为字段来源，Rust 测试用真实
// 序列化结果对照；前端这里校验请求、消费响应）。
//
// 分工保持原样：组件读 store 状态、store 调 ipcCall；非试点命令仍可直接
// bridge.invoke，不在这一轮整体迁移。
import contract from '../../../contracts/ipc-contract.json' with { type: 'json' };
import { invoke, hasBridge } from './bridge.js';
import {
  IpcCallError,
  classifyIpcFailure,
  errorCode,
  validateRequest,
  validateResponse,
} from './ipc-schema.js';

export const IPC_CONTRACT = contract;

export function ipcSpec(command) {
  return contract.commands[command] || null;
}

/// 调用试点命令。失败分三类：
///   contract —— 请求/响应不符契约，调用不会到达 Rust，或结果被拒收；
///   transport —— 桥接未注入（纯浏览器）或 invoke 根本没到 Rust；
///   business —— Rust 返回 Err(String)，保留后端原文。
export async function ipcCall(command, args = {}) {
  validateRequest(contract, command, args);
  try {
    const data = await invoke(command, args);
    validateResponse(contract, command, data);
    return data;
  } catch (error) {
    if (error && error.kind === 'contract') throw error;
    const kind = classifyIpcFailure(error, hasBridge());
    throw new IpcCallError(command, kind, errorCode(contract, kind), error);
  }
}
