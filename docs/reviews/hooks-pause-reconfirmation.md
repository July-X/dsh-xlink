# Hook 复审修复验收记录（2026-11-17）

基线：`c4dcff625b33ee2f89215144e46b0d2e91f7a940`。本次仅提交草稿 PR，不合并、不发布。

## 修复与生产接线

- 网络确认升级为配置 schema 3 的确认 ID + 独立运行标记。监听前写未安全结束；正常退出回收线程且网络未撤销才写安全结束。配置读取和换代串行，旧线程不能覆盖新配置；启动发现接口变化同样撤销自动恢复许可。
- 启动/退出继续走应用既有 `service::auto_start` / `service::stop`；显式保存重新确认，关闭通过兼容可选参数 `disableOnly` 使用已保存配置，不校验/保存错误草稿。关闭状态下普通保存仍保留设置能力。
- 来源拒绝响应加入短写超时和有界排空；真实 TCP 用例覆盖 403、持续慢发送和网络撤销时正在读取半截请求的连接关闭。
- Jenkins 原生用例走可见控件展开“更多”并点击“接入 Jenkins”，另外覆盖 Jenkins 模板、独立 Secret text 和正确顶层通知负载。此轮未运行原生用例，也没有真实 Jenkins。
- 备份说明统一要求完全退出应用并确认进程结束；关闭接收不停止后台维护。

## 实测

| 检查 | 结果与范围 |
| --- | --- |
| `npm run test:ui` | 879 通过，0 失败/跳过，自然退出；Node 24.22.1 |
| Hook UI 定向测试 | 38 通过，使用 Vue watcher 和模拟 IPC，不是原生 WebView |
| `node --test scripts/hooks-send.test.mjs` | 13 通过，真实本机 HTTP fixture |
| 生产源隔离 Rust harness | 102 通过；直接引用 Hook config/network/server/service/store 等生产文件与真实 HTTP，壳路径/日志/设置依赖替身，网卡枚举注入 |
| harness Clippy / Cargo fmt | 通过，Clippy `-D warnings` |
| `build:ui` / invariants / duplicates / bindings / bundle | 通过；重复 7 处（允许 8），58 个组件绑定检查，JS 837965 B / CSS 257920 B |
| `node --check e2e/specs/hooks.spec.mjs` / `git diff --check` | 通过 |
| 完整原生 `cargo test --locked --lib hooks` | 阻塞：本环境缺少 GTK/ATK/Pango 开发库，未执行原生用例，不算通过 |

### UI 计数更正

既有 `test:ui` 使用 `--test-force-exit`。Node 24.22.1 的实测中，旧轮报告 825 / 866，同一当前文件集使用该选项串行报告 847，末尾部分用例没有出现在结果中；去掉该选项后完整报告 879 且自然退出。单独 `designAlignment.test.js` 无强制退出时完整 108 项通过。本 PR 移除此参数；CI workflow 仍使用 Node 24。此前通过数不能解释为完整且同范围的覆盖，本轮以不强制退出的完整结果为准。

## 必须披露的边界

- 暂停跨重启保证基于已成功写入的运行标记。若启动前写标记在替换前失败，旧安全结束标记可能仍在；本次拒绝启动，但存储恢复后后续启动可能自动重试。测试显式注入替换前写失败以证明此边界，不伪称物理磁盘故障。错误提示要求修复目录后手动重新确认。
- 断网测试注入接口发现结果，HTTP/worker/accept/启停为生产路径；不代表真实 Windows/macOS 网卡切换、休眠或双机/VLAN 验收。异常退出标记通过隔离生命周期模拟，不是强杀原生桌面进程。
- 未运行真实双机、真实 Jenkins、原生 WebView E2E 与完整平台构建。独立审查及隔离测试不能替代 CI 和实机验收。

## 首轮 CI 反馈与修复

精确提交 `39b0e7e0a5f77ae3172ebe878353f09af447d962` 的 [Desktop CI 44015623987](https://github.com/July-X/dsh-xlink/actions/runs/44015623987) 结果：JavaScript / 协议 / UI 门禁成功；macOS 原生 Rust 单测 1725 通过、16 忽略，随后 Clippy 因只供测试使用的 `ServerHandle::stop` 触发 dead_code 失败；Windows 原生 Rust 1697 通过、1 失败、17 忽略。

Windows 失败为 `network_loss_closes_inflight_slow_request_without_ui_polling`：客户端已经观察到连接关闭且运行标志已撤销，但线程回收耗时约 30 秒，达到该用例设定的请求头读超时。修复不放宽断言、不跳过 Windows：接收读取改用短超时片段并检查取消，重试片段时保留原每次读取的空闲超时预算；请求头、JSON 与附件读取继续复用相同解析逻辑。`stop` 测试便利方法仅在测试构建保留。最终平台效果仍以新提交 CI 为准，不能把本机隔离回归当成 Windows 验证。

Windows 延迟的底层原因仍是依据时序的推断：[Microsoft shutdown 文档](https://learn.microsoft.com/en-us/windows/win32/api/winsock/nf-winsock-shutdown)说明后续 `recv` 的行为，但没有承诺跨克隆句柄能取消已经挂起的读取。本次消除对此行为的依赖，而非把 Linux 测试说成 Windows 原因复现。

本轮修复后隔离 harness 全部 107 项通过（其中 Hook 104 项，另有共享写文件测试 3 项）；新增真实本机 TCP 分片读取与不依赖 shutdown 的取消测试，原来的 3 秒网络撤销断言保持不变。此取消语义针对等待网络读取，不是业务事务回滚：已经读齐或进入数据库/文件提交的在途请求仍可能完成，不能承诺撤销瞬间所有在途业务都没有副作用。
