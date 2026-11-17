# 真实 Jenkins 集成测试

`run-postbuild.py` 使用 Java 21+、Python 3，在临时目录创建新的 Jenkins Home、实际 Freestyle Job、真实 Groovy Postbuild 插件和独立 Remoting Agent JVM。只监听 `127.0.0.1`，凭据为合成字符串，不使用现有 Jenkins Home。测试实例跳过首次安装向导，脚本由测试管理员预批准；这不是要求在生产关闭脚本安全。

```sh
python3 scripts/tests/jenkins/run-postbuild.py \
  --war /path/to/jenkins.war \
  --plugins /path/to/dedicated-downloaded-plugins
```

Linux/Unix 测试启动器使用 `sh`、`ln` 和进程组清理；它不要求用户在生产 Jenkins 安装 Python。生产发送器仅使用 Jenkins 自带 JVM/Groovy。成功后打印证据目录；失败也保留日志，退出时停止测试 JVM。依赖不提交仓库。

本次测试版本：Jenkins 2.591、Java 21.0.13、Groovy Postbuild `335.vf10eb_c1b_5e14`；完整插件版本在 `tested-plugins.json`。WAR 和插件从 Jenkins 官方更新中心下载，按更新中心给出的 SHA-256 校验。可从 [Jenkins 下载](https://www.jenkins.io/download/) 与 [插件更新中心](https://updates.jenkins.io/update-center.actual.json) 获取匹配版本及必需依赖。不要将最新插件盲目安装到旧 Jenkins。

覆盖真实插件 `manager` 绑定、Job 上下文凭据查询、未设置 Jenkins Root URL、成功/失败原始结果、Controller 本地文件、真实 Remoting Agent 文件、符号链接拒绝、短日志真实快照（1 MiB 截断由 Groovy contract 夹具覆盖）、缺失附件降级、缺失凭据、HTTP 重定向拒绝和 503 重试使用完全相同的事件字节。实际 Jenkins 在这些 Publisher 用例中已给出 SUCCESS/FAILURE；`null → UNKNOWN` 边界另由 Groovy 夹具验证，不伪称真实 Jenkins 本轮产生了 null 状态。

**边界**：接收端为严格校验上传摘要/长度的本机 HTTP 协议夹具，不是生产 Rust 接收器；同机两个 JVM 的 Remoting 不是跨物理机器/Windows Agent 验收。脚本实际执行不经过模拟 Jenkins API，但不代表用户现有 Jenkins/插件版本或真实局域网已经验收。
