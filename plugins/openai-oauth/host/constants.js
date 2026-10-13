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

/**
 * 常用模型偏好文件的路径（壳经子进程环境传入，与 Rust 的
 * `plugins/builtin::SELECTION_FILE_ENV` 同名）。
 *
 * Host **不读它的内容**：勾选状态的唯一读法是 `/v1/models` 载荷里的
 * `selected`（由 Rust 按偏好与当前目录算好）。这里只用来做变更通知——
 * 文件一变就重新广播一次适配器注册，内核据此让已打开的页面重拉目录。
 */
export const SELECTION_FILE_ENV = "DSH_XLINK_OPENAI_SELECTION_FILE";

/** 偏好文件的观察节奏与合并窗口（毫秒）。一次原子写会先落临时文件再改名，
 *  stat 可能连跳两拍；2s 的 stat 轮询对内核进程可以忽略，而 150ms 的合并
 *  窗口够把同一拍的重复通知压成一次。 */
export const SELECTION_WATCH_INTERVAL_MS = 2000;
export const SELECTION_WATCH_DEBOUNCE_MS = 150;

/** 桥接协议版本（开发计划 §5：首版为 1，握手核对）。 */
export const BRIDGE_PROTOCOL_VERSION = 1;
