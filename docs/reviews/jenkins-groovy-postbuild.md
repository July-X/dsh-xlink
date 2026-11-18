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
- 本轮未修改 Rust，未执行桌面全量构建与发布门禁；不是版本发布。UI 接入指引的追加验证见下节。

## UI 内接入指引（追加）

- 创建来源选择 Jenkins 后，在一次性 Bearer 凭据窗口直接展示 Groovy 步骤与配置；不再自动生成旧 Notification Basic 密码，也不丢弃 Bearer。已有来源均可打开「Jenkins 接入指导」，旧 Notification 只保留已启用来源的兼容轮换。
- UI 通过 Vite `?raw` 从生产 Groovy 文件单源加载，只替换唯一配置块；自动使用当前 URL，配置凭据 ID、消息、文件和日志，复制完整脚本且不读取/嵌入令牌。停服或地址无效时禁复制。
- 最终 UI 全套测试自然退出：886/886 通过、0 失败、0 跳过；`build:ui`、`check:invariants`、`check:duplicates`（7 ≤ 8）、`check:ui-bundle`、`git diff --check` 均通过。
- 独立审查通过；实际 Groovy 2.4.21 验证生成配置中的美元插值、引号、反斜杠、换行和 Unicode 分隔符仍为数据，不执行用户输入。生产发送器 SHA-256 未变，之前真实 Jenkins 证据继续对应同一生产源码。
- 浏览器视觉 / 原生壳验收 **not_run**：当前环境 Chromium 普通及审批执行均因 `socket() failed: Operation not permitted` 无法启动；云浏览器与执行器不共享 localhost，预览返回 `ERR_CONNECTION_REFUSED`。已检查环境并重连云电脑，无可用截图，不能据静态样式宣称 1040×748 明暗主题、滚动和真实点击已通过。行为测试和构建不替代这项验收。

## 内网快速失败（2026-11-18）

- 将连接 / 读取空闲超时收紧为 3 / 5 秒，每请求最多 2 次尝试；创建、全部附件、提交、回执和重试传递同一个单调时钟截止点，共用 30 秒网络预算，不再按大附件另给数分钟。保留幂等字节、摘要回执核验、脱敏和中断处理。
- Groovy 2.4.21 + Windows Java 26.0.2 合约测试 **68 项通过**：真实回环 HTTP 验证过期预算不发请求、中断不发请求、无响应头、持续滴流、串行步骤共用剩余时间、预算不足不重试；直接调用生产 `send`，每个上传延迟 4 秒，累计在 29–35 秒窗口失败，未传完 10 个附件且未发送 commit/receipt。Jenkins API 仍为替身。
- Jenkins 指引与 Hook 动作定向 UI 测试 40 项，39 通过、1 个 Bash 用例在 Windows 跳过；`build:ui`、`check:invariants`、`check:duplicates`、`git diff --check` 通过。界面复制仍直接引用生产单源脚本，只替换配置。
- 未重跑真实 Jenkins / Agent、跨机网络、TLS、写阻塞及原生壳点击；此前真实 Jenkins 的源码摘要不覆盖本轮修改。30 秒不含 Agent 文件、日志读取与凭据查询，定时断开也不是 JVM/操作系统级硬终止，因此不承诺整个 Job 在 30 秒内结束。需要重新复制脚本并在 Job 替换、按需重新审批，旧脚本不自动更新。
