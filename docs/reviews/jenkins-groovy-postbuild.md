# Jenkins Groovy Postbuild 发送器验证

日期：2026-11-17。基线：`bd4faefc86e33497e51b134d890994a7d727f83e`。用户入口见[使用手册](../features/hooks/jenkins-groovy.md)。

## 设计与范围

- 固定单文件 Groovy，五项管理员配置，不依赖 Node.js/Python/Notification 插件。
- Controller JVM 取 Job 上下文 Secret text，发送可信私网 IPv4 Hook；不跟随重定向、不使用 JVM 代理。
- 结果为 Postbuild 当前快照，null 为 UNKNOWN；不伪称 FINALIZED，不改变构建结果。
- 经 FilePath 读取 Agent 文件，拒绝路径逃逸和符号链接；不可用文件明确跳过。日志只保留有界开头快照。
- 本次执行固定 ID、时间、正文、附件字节；登记、上传、提交、核对回执。无持久离线队列。

## 验证分层

| 层次 | 结果与覆盖 |
| --- | --- |
| 实际 Groovy 2.4.21 执行的合约测试 | 48 项通过；Jenkins API 为替身，本机 HTTP 为协议 fixture。覆盖私网限制、类型大小、Unicode 分块、日志截断、429/503 固定负载重试、401/403、重定向、附件全流程、错误回执、UNKNOWN、Root URL 缺失、构建结果保留与凭据不输出 |
| 真实 Jenkins + 独立 Remoting Agent JVM | 6 个 Freestyle 任务通过：成功、失败、远程文件、缺失文件、缺失凭据、重定向。实际收到本机/Agent 文件及失败日志；上传大小/摘要正确；外部符号链接哨兵未上传；503 重试字节不变；凭据不出现在构建日志；原构建结果不变。接收端仍为 HTTP 协议 fixture |
| 既有 Node 发送器回归 | `node --test scripts/hooks-send.test.mjs scripts/hooks-send-example.test.mjs`，18 项通过；未修改 Node 发送器 |
| 仓库静态门禁 | `npm run check:invariants`、`npm run check:duplicates`、`git diff --check` 通过；复用已有依赖，不提交 node_modules |

真实 Jenkins 集成使用 Linux、Java 21.0.13、Jenkins 2.591、Groovy Postbuild `335.vf10eb_c1b_5e14`、Credentials `1514.vb_01eb_014e7d4`、Plain Credentials `199.v9f8e1f741799`、Script Security `1436.v1082791135e4`。官方 WAR 与插件在隔离临时 home 内运行，测试审批只作用于此临时实例；使用合成凭据，不访问用户 Job。重现入口为 `scripts/tests/jenkins/run-postbuild.py`，不提交 WAR、插件或测试运行产物。

最终实测发送器 SHA-256：`8c405fb52b106c9281db56d372c8664b13efd9aef362e6757e369d26fe651ce2`。测试直接加载此源码，只替换顶部配置，不保留调试插桩。

## 实测发现并修复

1. 日志 UTF-16 代理对恰好跨读取块边界时，独立 UTF-8 编码会破坏 emoji。已保留末尾高代理项到下一块，增加反向用例。
2. Jenkins 未配置 Root URL 时，`build.getAbsoluteUrl()` 直接抛出 `IllegalStateException`，`?: build.url` 不能回退。已显式捕获并使用相对控制台地址，合约及真实 Jenkins 都保留 Root URL 未配置场景。

3. Jenkins `FilePath` 没有实例无参 `isSymlink()`；mock 未覆盖这一点，实际工作区附件全部被跳过。真实集成的上传内容断言捕获后改用 `readLink()`，不能把只有文字通知成功视作文件成功。

## 未覆盖及非承诺

- HTTP 接收端为协议 fixture，不是 Rust 生产 Hook 或桌面 UI；协议已按当前生产源码对照审查，不能把这一层写成桌面端到端通过。
- 未验证用户现场 Jenkins / 插件版本、Windows Agent、跨机器防火墙、TLS、VPN/网络切换、真实 Agent 断线及 Controller 崩溃。UNSTABLE、长日志截断和写阻塞超时未作真实 Jenkins 用例；UNKNOWN 与 1 MiB 截断由合约层覆盖，不冒称真实 Jenkins 已验。
- 不保证严格最终结果、完整最终日志、持久队列或中止后必达。接口成功必须收到并验证 ready 回执。
- 本轮未修改 Rust/UI，因此未执行桌面全量构建与发布门禁；不是版本发布。
