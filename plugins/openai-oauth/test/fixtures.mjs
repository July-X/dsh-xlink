// 测试夹具：把 `host/` 复制进临时目录，并给它两个**桩 peer**。
//
// 为什么要有桩 peer：`host/adapter.js` 静态 import `@deepseek-ai/dsh-llm`
// （基类与错误类型），`host/config.js` 静态 import `@deepseek-ai/schemastery`
// ——ESM 的具名导入在链接期就要全部存在，所以桩模块必须把 adapter.js 用到的
// 每个名字都导出来。桩只提供**形状**：本文件的两条用例验的是桥接载荷到
// 模型列表的映射与偏好变更通知，与内核的真实语义无关；真实内核的兼容性由
// `adapter-images.test.mjs`（需要 DSH_OAUTH_TEST_KERNEL）与 `smoke.mjs` 盖。
import { cpSync, mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const LLM_STUB = `
export class LlmAdapter {}
export class LlmError extends Error {
  constructor(message, code, options) {
    super(message);
    this.code = code;
    this.options = options;
  }
}
export const QUOTA_EXCEEDED_CODE = 'QUOTA';
export const requestImageHandleText = (ref) => String(ref);
export const offloadedImageText = (ref) => String(ref);
export const resolveImageAttachmentAccess = async () => undefined;
export const requiredImageOffload = false;
`;

const SCHEMASTERY_STUB = `
const z = { object: () => ({ volatile: () => ({}) }) };
export default z;
`;

/** 复制插件 host/ 到 `scratch/plugin`，返回该目录（调用方负责清理 scratch）。 */
export function stagePlugin(scratch) {
  const dir = join(scratch, 'plugin');
  mkdirSync(dir, { recursive: true });
  cpSync(new URL('../host', import.meta.url), join(dir, 'host'), { recursive: true });
  writeFileSync(
    join(dir, 'package.json'),
    JSON.stringify({ name: 'xlink-openai-oauth', version: '0.0.0-test', type: 'module' }),
  );
  const peers = join(dir, 'node_modules', '@deepseek-ai');
  mkdirSync(peers, { recursive: true });
  for (const [name, source] of [['dsh-llm', LLM_STUB], ['schemastery', SCHEMASTERY_STUB]]) {
    const pkg = join(peers, name);
    mkdirSync(pkg, { recursive: true });
    writeFileSync(
      join(pkg, 'package.json'),
      JSON.stringify({ name: `@deepseek-ai/${name}`, version: '0.0.0-stub', type: 'module', main: 'index.js', exports: { '.': './index.js' } }),
    );
    writeFileSync(join(pkg, 'index.js'), source);
  }
  return dir;
}
