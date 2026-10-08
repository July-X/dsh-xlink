/**
 * 内嵌插件的稳定标识。
 *
 * 三个标识各有归属，不能混用：
 * - 包名 / loader 行 id / settingsNs 三者共用 `xlink-openai-oauth`：
 *   接线行的 id 经 `ctx.fiber.entry?.options.id` 成为设置命名空间，
 *   client 卡片也按它 keyed 注册（见 P0 调查 §6）。
 * - 提供方路由是 `xlink-openai-chatgpt`，进入 dsh 的会话与配置。
 * - 显示名按设计文档 §3.3 固定为「OpenAI · ChatGPT 套餐」。
 */
export const PACKAGE_ID = "xlink-openai-oauth";
export const PROVIDER_ID = "xlink-openai-chatgpt";
export const PROVIDER_DISPLAY_NAME = "OpenAI · ChatGPT 套餐";

/**
 * Rust 桥接服务的子进程环境变量（开发计划 §5：地址与令牌只经环境传入，
 * 禁止写入命令行、profile、日志或浏览器初始化数据）。
 */
export const BRIDGE_URL_ENV = "DSH_XLINK_OPENAI_BRIDGE_URL";
export const BRIDGE_TOKEN_ENV = "DSH_XLINK_OPENAI_BRIDGE_TOKEN";

/** 冒烟测试用的状态文件落点；生产启动不设此变量，不写任何文件。 */
export const TEST_MARKER_ENV = "DSH_XLINK_OPENAI_TEST_MARKER";

/** 桥接协议版本（开发计划 §5：首版为 1，握手核对）。 */
export const BRIDGE_PROTOCOL_VERSION = 1;
