# Jenkins Freestyle：Groovy 构建后发送 Hook

使用固定的 [`jenkins-hook-postbuild.groovy`](../../../scripts/jenkins-hook-postbuild.groovy) 发送消息、构建结果快照、日志与 workspace 文件。无需 Node.js 或 Python，不依赖老旧 Notification 插件；直接使用通用 Hook Bearer 接口，不使用 `/jenkins-notification` 路由。

## 1. 准备一次，以后只改五项配置

1. 在 dsh-xlink「Hook → 设置」确认可信局域网、开启接收；在「接入点」新建来源时选择 **Jenkins**，界面直接提供 Groovy 接入指引。创建后先保存只显示一次的 **Bearer 令牌**。已有来源也可从列表的 **Jenkins 接入指导** 打开指引，不需要重新创建来源。复制**接入点根地址**。地址示例：`http://192.168.1.50:17890/hooks/v1/endpoints/ep_xxx`，不加 `/events`。不要使用通知密码。
2. Jenkins 管理员安装与当前 Jenkins / Java 兼容的 **Groovy Postbuild** 插件及其依赖，确保支持 **Secret text** 凭据。机器仅有 Python 不影响此方案，Groovy 在 Jenkins Controller 的 JVM 中执行。
3. 在该 Job 可访问的 Jenkins Credentials 范围创建 Secret text，Secret 填 Bearer 令牌，ID 例如 `xlink-hook-token`。令牌不放脚本、参数、SCM、URL 或控制台输出。
4. 在 dsh-xlink 的 Jenkins 接入指引中填写凭据 ID、消息、文件和日志策略，再复制完整 Groovy 脚本。当前接入点 URL 自动填入，但**不包含令牌**。接收服务未运行或地址不可用时，先去「设置」恢复服务后再复制。Freestyle Job 的「构建后操作」添加 **Groovy Postbuild**，粘贴已配置的脚本。直接使用仓库原始脚本时，可修改顶部五项配置。先小范围试投递，再加入文件。
5. 管理员审核脚本及必要的 Script Approval。构建对象、凭据、远程文件和 HTTP 操作不是普通沙盒脚本权限。不要为方便禁用全局 Script Security。配置变化若改变脚本文本，需要再次审批；令牌轮换只更新同一个 Credentials 项，无须把新令牌贴入脚本。

仓库原始脚本配置例子（界面复制版会把同样五项配置安全编码到配置块，无需再手工改 Groovy）：

```groovy
def HOOK = [
    url: 'http://192.168.1.50:17890/hooks/v1/endpoints/ep_xxx',
    credentialsId: 'xlink-hook-token',
    message: '每日构建，请查看结果及附件',
    files: ['reports/summary.txt', 'screenshots/result.png'],
    log: 'on_failure'
]
```

| 配置 | 用法 |
| --- | --- |
| `url` | 接入点根地址。仅明确私网 IPv4 / 回环 HTTP(S)，不接受公网、域名、userinfo、查询参数、片段或重定向 |
| `credentialsId` | 此 Job 有权使用的 Secret text ID，不是令牌本身 |
| `message` | 固定说明文字；脚本自动补任务、构建号、状态快照及构建链接，不执行模板表达式 |
| `files` | workspace 相对文件路径数组；不支持 glob、绝对路径、父目录跳转或符号链接。无需附件用 `[]` |
| `log` | `off` 不附日志；`on_failure` 异常状态附有界快照；`always` 总是附快照 |

收件箱标题固定为「任务名 #构建号 · 状态」，用来在列表和详情里直接区分是哪个 Job 的哪次构建；详情标题旁另显示服务端观测到的投递方（Controller）IP，旧版本收到的事件显示「未记录」。

URL 与凭据 ID 只能由可信 Job 管理员配置，**不要改成任意触发者可填的构建参数**，也不要从不可信 SCM 下载脚本再以管理员批准权限执行。否则可把凭据发往攻击者控制的另一台内网机器。该方案面向内部可信网络和可信构建工作区，不是恶意多租户隔离机制。

## 2. 网络与运行位置

请求从 **Controller** 发出，不是执行构建命令的 Agent。接收器允许来源网段与防火墙应覆盖 Controller 地址；Agent 能访问接收器不代表此脚本能访问。Controller 也必须能经 Jenkins Remoting 读取 Agent workspace。

HTTP 明文传输令牌和素材，只能用于已经确认可信的网络。HTTPS 必须正常校验证书，IP 地址需在证书 SAN 中；脚本不接受域名、不绕过证书校验、不跟随任何重定向。私网地址本身不证明网络可信，VPN/路由变化后重新检查。接收器部署与反向代理见[局域网部署指南](lan-deployment.md)。

## 3. 结果与日志的准确含义

将 Groovy Postbuild 放在 JUnit、结果分析及其他会改变构建结果的步骤之后，workspace 清理之前。插件执行顺序还受 Jenkins publisher 生命周期影响，不能把页面上的顺序当作严格最终执行保证。

脚本读取 `build.getResult()` 的**当前构建后快照**。`null` 标成未知，不能当作成功；后续 publisher 仍可能把结果改为失败。通知不会声称严格 `FINALIZED` 或「已核验」。如果必须拿不可再变的最终结果，需要独立 Jenkins 插件的 `RunListener` 生命周期方案，不要在 Job 脚本里动态注册监听器或创建后台线程。

日志同样是执行发送脚本时已有的有界快照（保留开头，含标记最多 1 MiB，不是末尾日志，因此失败关键行可能不在快照中），不包含之后的 publisher 和 Jenkins 最终收尾。通过 Jenkins 日志 API 读取，不猜 `JENKINS_HOME/jobs/.../log` 路径，也不调用可能等到日志关闭才返回的 `writeWholeLogTo()`。截断会标明，并附构建链接供查看完整控制台；Jenkins 未配置 Root URL 时使用相对控制台地址，不因此阻断投递。默认仅异常状态附日志，避免每次成功构建都上传大量内容。Jenkins 的密码掩码不是绝对脱敏保证，上传前仍应确认日志允许离开 Jenkins；有敏感输出时先设 `off`。

## 4. 文件、限制和失败行为

文件经 Jenkins `FilePath` 从当前构建 workspace 读取，支持远程 Agent；不是在 Controller 用 `new File(WORKSPACE)` 猜文件位置。路径必须留在 workspace 内，不允许符号链接。工作区应由可信构建使用；不要并发恶意替换路径或链接，路径检查不能提供对抗主动攻击者的原子文件隔离。

文件快照保存在 Controller 内存中，同时运行多个发送任务时应控制文件大小与并发数。

支持 PNG/JPEG/WebP 图片（每个最多 10 MiB）与 `.log/.txt` 日志（每个最多 50 MiB），不支持 zip/pak/exe。每条最多 10 个附件、总量 100 MiB，自动日志也占附件名额。文本日志按 UTF-8 发送，其他编码请在构建中先转换成单独 UTF-8 文件。文件读取失败、Agent 离线或 workspace 已清理时，脚本报告附件缺失，不把缺失文件伪报上传成功。

发送先固定本次事件 ID、时间、正文与附件快照，再登记事件、逐个上传、提交并核对回执中的大小和 SHA-256。只有 `ready` 且附件一致才报告成功。有限重试沿用同一快照，不重新读取变化中的文件或生成新事件。重试耗尽、401/403、冲突、超限和错误回执都不是成功。每个请求最多尝试 4 次，连接超时 10 秒、读取空闲超时 30 秒，并按传输体积设置 30–300 秒的单次截止时间；多附件及重试累计仍可能耗时较长，会占用当前 publisher 的执行时间。

脚本发送失败只写脱敏警告，**不调用构建结果修改 API**。同时将 Groovy Postbuild 自身的失败行为设为不影响构建结果（以所装版本界面选项为准），避免脚本加载/审批阶段的异常由插件改写状态。不要打印凭据、请求头、服务端原始响应或完整异常堆栈进行排错。

没有持久离线队列、断点续传或跨执行自动重放。Controller 重启、构建强制中止、接收器长期离线都可能丢失通知。不要把每次手工执行当成原事件重试；新的执行是新通知。需要可靠补发时另行保存脱敏业务事件与附件，并设计队列，不要仅重复点击构建。

## 5. 首次验收与排错

先用 `files: []`、`log: 'off'` 验证文字，再加入一个短 `.txt` 文件，最后验证失败构建日志。逐项检查：

- 成功、失败、不稳定构建的状态与当时快照一致，发送失败不改变原构建结果。
- 接收端确实出现消息，附件可下载，大小与内容匹配；控制台仅报告投递结果，不出现令牌。
- 远程 Agent 文件可读；缺失文件、离线 Agent、workspace 清理有明确告警。
- 令牌错误、接入点暂停、来源网段拒绝、HTTP 重定向均拒绝，不显示「成功」。
- 接收端关机后有界失败；恢复后不承诺自动补发。

连接失败首先检查 **Controller → 接收器** 路由与端口；401 检查 Bearer Secret text；403 检查接入点状态和允许来源；审批错误由管理员核对脚本，不能关闭安全检查绕过。没有 Groovy Postbuild 菜单时先安装兼容插件，不使用 Notification 插件替代。

开发者可用 Jenkins WAR 中的 Groovy 2.4.21 jar 运行合约测试：

```sh
java -cp /path/to/groovy-all-2.4.21.jar groovy.ui.GroovyMain scripts/jenkins-tests/contract.groovy
```

该测试使用 Jenkins API 替身与本机 HTTP fixture，不是 Jenkins 实测。真实 Jenkins 隔离集成测试：

```sh
python3 scripts/tests/jenkins/run-postbuild.py --war /path/to/jenkins.war --plugins /path/to/plugins
```

该开发测试启动器在 Linux 上运行（用 shell 构建步骤与进程组清理），需要 Java 21+、Python 3 及预先下载的兼容插件目录，只启动临时 home 和回环端口，用合成凭据、不碰真实 Job；测试过程会在此隔离实例中批准待测脚本。接收端仍是协议 fixture，不是桌面应用。测试依赖不随脚本部署，使用脚本的 Jenkins 机器无需 Python。

本机测试版本、用例和未覆盖项见[验证记录](../../reviews/jenkins-groovy-postbuild.md)及 PR。自动 fixture、真实本机 Jenkins、真实 dsh-xlink 接收器、用户现场双机网络是不同层次，不可互相代替。用户现网版本、TLS 与 Agent 拓扑仍需按上述步骤验收。

## 依据

- [Groovy Postbuild 插件](https://plugins.jenkins.io/groovy-postbuild/)
- [Run.getResult / 日志 API](https://javadoc.jenkins.io/hudson/model/Run.html)
- [FilePath 远程文件 API](https://javadoc.jenkins.io/hudson/FilePath.html)
- [RunListener 完成与 finalized 生命周期](https://javadoc.jenkins.io/hudson/model/listeners/RunListener.html)
- [CredentialsProvider 按构建上下文查找凭据](https://javadoc.jenkins.io/plugin/credentials/com/cloudbees/plugins/credentials/CredentialsProvider.html)
