/**
 * dsh-xlink 官网对话引用插件（Host 侧）。
 *
 * ## 它在 G1 里的唯一职责：让客户端半边被内核发现
 *
 * 内核的 `dsh-client-modules` 是**扫 host Loader 的 entries** 找带 `dsh.client`
 * 声明的包（`dsh-client-modules/index.js@0.2.1-rc.2:105-119`、
 * `dsh-client-modules/index.js@0.2.1-rc.2:720-750`、
 * `dsh-client-modules/index.js@0.2.1-rc.2:762-780`；该文件 SHA-256
 * `D2D89461E1E7F15D6E24CD8AC3FAAA2B931E066D5B5300F3C38AEA29DEF38BCF`，
 * 现役 release 树
 * `C:\Users\zxx\.dsh-xlink\dsh\desktop\kernels\0.2.1-rc.2`）：Loader 行的
 * `name` 指向哪个文件，kernel 就沿它向上找最近的 `package.json`，那份清单里
 * 必须有 `dsh.client.platform === "web"` 与 `exports["./client"]`。
 *
 * 也就是说：**没有 host 半边，这个插件的 client/client.js 根本不会进浏览器的
 * 模块图**。本文件因此是「客户端半边存在」的锚点，不是一个空壳——它必须
 * 真的被内核 import 成功（import 失败时内核只记一行 warning，插件的开关在
 * 界面上照常显示「已启用」；那次事故记在 `builtins.rs` 里 memory 那条 spec 的
 * 注释上）。
 *
 * ## 它明确不做的事
 *
 * G1 只验「选择 / 异步读取 / 发送门控」这条链路，host 侧的 mention 展开
 * （`agent/pre-step` 展开 `official-chat:` scheme、快照落盘、user 消息级
 * 上下文）是 P1/P2 的事。这里不预写：写一份没有真实快照来源的展开逻辑，
 * 只会让 G1 的结论里混进一条没被验证的链路。
 *
 * ## 为什么这里不打日志
 *
 * Host 侧在 G1 里没有可观测的行为要报。真正要观察的东西在客户端半边
 * （`client/client.js` 的探针）与内核日志里。
 *
 * @module xlink-official-chat
 */

/** 内核按这个名字认这个插件（也是 settingsNs 的来源）。 */
const name = 'xlink-official-chat';

/**
 * 这个插件的 host 半边不消费任何内核服务。
 *
 * **写 `[]` 而不是留空**：留空时 cordis 会按「无 inject」处理，而本文件
 * 一个服务都不碰——写出来是为了让「它确实什么都不依赖」这件事在代码里是
 * 一句可以被读到的断言，而不是一个待填的空位。
 */
const inject = [];

/**
 * 插件入口。内核 Loader import 这个文件时调用它。
 *
 * **这里不抛**：host 半边只挂一个空的上下文贡献槽，插件是否真的在客户端
 * 半边生效，由 `client/client.js` 的探针与内核日志回答，不由这里自报。
 *
 * @param {import('@deepseek-ai/cordis').Context} ctx - Host 根上下文。
 */
function apply(ctx) {
  // 刻意留空，但把 ctx 读一次：注入一个不消费任何服务的插件时 cordis 仍会
  // 建一条 fiber，这里确认 fiber 建起来了（不读则这条分支在 lint 下是死代码）。
  void ctx;
}

export { apply, inject, name };
