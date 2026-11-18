// 内部客户端模块的加载锚点：Hook 分析交接不消费任何内核 host 包，
// 也不暴露账号、模型或用户开关。工作台那一半在 client/client.js。
export const name = 'xlink-hook-analysis';
export function apply() {}
