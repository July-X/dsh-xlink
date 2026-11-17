# Hook 局域网部署与发送指南

日期：2026-11-17 · 状态：本轮自动验证见 PR；两机直连 / TLS、网络变化与真实 Jenkins 待验收

本文讲接收器默认的直接 LAN HTTP 用法、可选的反向代理加固，以及发送与排错。协议字段与安全边界在[设计契约](lan-inbox-design.md)维护；阶段状态见[开发计划](development-plan.md)。

## 1. 架构与前提

    Jenkins / 脚本 / 任意 HTTP 发送方
            │  ① 直连 HTTP（默认，明文）
            ▼
    dsh-xlink Hook 接收器（监听已选私网 IP:<端口> + 127.0.0.1）

    或需要加密 / 来源限制时：

    Jenkins / 脚本 / 任意 HTTP 发送方
            │  ② HTTPS（LAN）
            ▼
    同机反向代理（TLS 终止、Host 改写、只转发 /hooks/v1/*）
            │  HTTP 127.0.0.1:<端口>
            ▼
    dsh-xlink Hook 接收器

- 应用只绑定明确选择的私网 IPv4 地址和回环。首次在「Hook → 设置」确认网卡并启用；地址不存在时暂停，绝不扩大为所有网卡。支持 192.168、10、172.16–31 内网。
- 默认允许所选接口实际掩码对应子网；跨 VLAN Jenkins 可在高级设置添加明确的可信私网 CIDR。不能把私网地址等同可信证明，网卡/VPN/路由变动后需重新确认；应用不自动改防火墙。
- release 默认端口 17890，dev 默认 17891。服务实际运行后才复制发送地址。运行中在接收循环后台定期复核接口地址、掩码与在线状态；发现变化后暂停，不自动恢复，按面板提示重新确认并启用。
- 推荐流程：确认网络并开启接收 → 创建普通/Jenkins 来源 → 复制配置或测试命令 → 在发送机器执行 → 接收端看到该消息。本机测试不等于双机已连通。
- HTTP 明文传输令牌和素材，只适用于可信内部网络；不可信 Wi-Fi、跨不可信链路或敏感数据需要另行加密。应用不能防止管理员自行做 NAT 或反代转发。
- 管理面（创建接入点、读收件箱、生成凭据）只走桌面 IPC，不在这个 HTTP 端口上。代理只需转发 /hooks/v1/。
- 代理必须把上游 Host 改写成 IP:<实际端口>（例如 127.0.0.1:<实际端口>）：接收器只接受 IP 字面量与 localhost 的 Host，域名仍拒绝以防 DNS rebinding，不信任 X-Forwarded-*。

## 2. 可选高级配置：反向代理示例

两个例子都只做 TLS 终止、路径白名单与 Host 改写；证书、域名与来源地址按实际替换。

### Caddy

    hook.example.com {
        tls /etc/caddy/certs/hook.pem /etc/caddy/certs/hook-key.pem

        @hooks path /hooks/v1/*
        handle @hooks {
            reverse_proxy 127.0.0.1:17890 {
                header_up Host 127.0.0.1:17890
                flush_interval -1
            }
        }
        respond 404
    }

只允许 Jenkins 主机时，把 @hooks 改成带来源条件的命名匹配：

    @hooks {
        path /hooks/v1/*
        remote_ip 10.20.30.40
    }

### nginx

    server {
        listen 443 ssl;
        server_name hook.example.com;

        ssl_certificate     /etc/nginx/certs/hook.pem;
        ssl_certificate_key /etc/nginx/certs/hook-key.pem;

        # 单个附件最大 50 MiB；给请求头和并发留余量。
        client_max_body_size 60m;

        location /hooks/v1/ {
            allow 10.20.30.40;   # 只允许 Jenkins / 发送方
            deny all;

            proxy_pass http://127.0.0.1:17890;
            proxy_http_version 1.1;
            proxy_set_header Host 127.0.0.1:17890;
            proxy_request_buffering off;
            proxy_set_header X-Forwarded-For "";
        }

        location / { return 404; }
    }

- flush_interval -1 / proxy_request_buffering off 让上传边到边写，避免代理先把 50 MiB 落到自己的临时盘。
- 代理访问日志不要记录 Authorization 头或完整敏感 URL；本应用不记录令牌，但代理是另一层。至少确认日志可共享、可轮转，或在 Hook 路由上关闭访问日志。
- 代理跑在 Docker / WSL 时，127.0.0.1 指容器内部；必须让上游真正到达宿主机的回环，并把 Host 仍改写为 127.0.0.1:17890。

## 3. 证书与来源

- 用可信证书（公网 CA 或自建 CA）。发送方机器必须信任该证书；不要用 curl -k 或关闭校验来「先跑通」。
- 自建 CA 的验收方式：

    curl --cacert hook-ca.pem https://hook.example.com/hooks/v1/endpoints/ep_xxx/receipt/evt_xxx

- 证书不可信、域名不匹配都应失败；失败是功能的一部分，不是配置问题。直连模式没有 TLS，不要把它当成加密通道。
- 防火墙只放行实际入口的端口（直连的 Hook 端口，或代理的 443）与来源地址。不要关闭系统防火墙，也不要在应用里自动改防火墙规则；改动由用户明确决定并自行执行。
- 应用忽略 X-Forwarded-For / X-Real-IP；直接连接按 TCP peer 校验来源；代理连接的来源表现为代理本机，因此代理仍须自行限制真实发送方。

## 4. 发送文本、图片与日志

令牌仅创建/轮换时显示一次。正式发送工具是 `scripts/hooks-send.mjs`，复制该单文件到发送机器即可使用，不需要克隆仓库或安装 npm 依赖；需要受支持的 Node.js。令牌通过环境变量传入，配置文件只保存非秘密地址和环境变量名。

该工具只接受可信私网 IPv4 或回环直连；需要走 HTTPS 反向代理时，请用 curl 或其他支持代理域名的客户端，不要用 `hooks-send.mjs`。

    # PowerShell：交互读取，避免令牌写入命令历史；使用完清理环境变量
    $env:DSH_HOOK_TOKEN = Read-Host "Hook 令牌"
    node hooks-send.mjs --base http://192.168.1.50:17890 --endpoint ep_xxx --text "构建完成"
    node hooks-send.mjs --base http://192.168.1.50:17890 --endpoint ep_xxx --title "构建失败" --file build.log
    Remove-Item Env:DSH_HOOK_TOKEN

    # bash：隐藏令牌输入
    read -rs DSH_HOOK_TOKEN; export DSH_HOOK_TOKEN
    node hooks-send.mjs --base http://192.168.1.50:17890 --endpoint ep_xxx --text "构建完成"
    unset DSH_HOOK_TOKEN

也可用完整接入点根地址 `--url http://192.168.1.50:17890/hooks/v1/endpoints/ep_xxx`；用 `--stdin` 从标准输入读正文；重复 `--file` 发送多个支持的图片/日志。配置文件/profile 的精确格式和所有参数见 `node hooks-send.mjs --help`。

客户端自动补时间和 ID、登记与上传附件、提交并确认回执；有限重试中复用同一 ID、时间与内容，不重新生成事件。重复发送不是可靠持久队列：重启进程后是否重发由调用方决定，需跨进程幂等时提供相同 `--external-id` 与 `--occurred-at` 及原始内容。返回 ready 才算成功；终态错误、配额拒绝和连接失败均非成功。

`scripts/hooks-send-example.mjs` 仍保留为合成数据演示，不替代正式发送工具。发送方负责脱敏，不能把凭据、敏感日志直接写进配置/仓库/构建日志；现有 v1 事件与上传接口保持兼容。

## 5. Jenkins 接入

新接入在界面选择「Jenkins」，或从已有来源打开「Jenkins 接入指导」，即可查看步骤并复制已填地址的完整脚本（不含令牌）。推荐使用 [Freestyle Groovy 固定脚本与手册](jenkins-groovy.md)：不依赖 Node.js 或 Notification 插件，只配置地址、凭据 ID、消息、workspace 文件和日志策略。它发送的是构建后状态与日志快照，并非严格最终状态。

### 旧 Notification 兼容适配（不建议新部署）

接收器提供独立路由：

    POST /hooks/v1/endpoints/<endpoint_id>/jenkins-notification
    Authorization: Basic base64("jenkins:<通知密码>")
    Content-Type: application/json

- 通知密码与 Bearer 令牌分开生成：Hook 页「接入点 → 生成/轮换通知凭据」，只显示一次；轮换会让旧密码立即失效。
- 该适配只接受 phase = FINALIZED 的通知；STARTED / COMPLETED / QUEUED 明确返回 400 unsupported_phase，不会混进收件箱。
- 插件 1.17 的 HTTP 发送器会把 URL 里的 user:password@ 转成 Basic 头，因此密码可以放在 Secret text 凭据的 URL 里，不需要 Public URL。插件配置建议：Protocol HTTP、Format JSON、Event FINALIZED、Log lines 0；默认 URL 用 http://jenkins:<密码>@<所选内网IP>:17890/hooks/v1/endpoints/<id>/jenkins-notification（明文，仅可信内网），存入 Jenkins Secret text，不使用公开 URL 或输出到日志。可选 HTTPS 代理时才替换为代理域名并改写上游 Host。
- 只映射元数据：Job 短名、显示名、job.url（含文件夹路径）、构建号、状态、耗时、时间、参数、SCM/测试摘要与备注。不下载日志、不抓取工件、不访问负载里的任何 URL；插件里的 log lines 保持 0。
- reported_result 只代表 Jenkins 报告的结果，收件箱不会写 verified_result，也没有「已核验」标签。需要真实构建状态时另立业务任务回查，不在接收层做。
- Job 身份优先用 job.url（区分文件夹下同名 Job）；缺失时保留事件并标记 identity_resolved=false，不用短名猜完整路径。同一构建同一终态重试返回原事件，状态变化返回 409。身份未解析且显示名相同的 Job 会共用幂等键，内容不同时保守返回 409。

> 源码依据：notification-plugin 1.17 的 Phase.java 与 Protocol.java（URL userinfo → Basic、JSON 字段下划线命名、最终事件 FINALIZED）。这是静态适配，没有真实 Jenkins 实机证据；记录实际 Jenkins / Java / 插件版本后才算兼容验收。

## 6. 验收与排错

| 场景 | 期望 |
| --- | --- |
| 允许来源 + 正确令牌 | 201/200，回执可查 |
| 错误令牌 / 撤销令牌 | 401，不泄露对象是否存在 |
| 暂停接入点 | 403，提示先在面板恢复 |
| 来源不在允许网段 | 403，错误信息说明来源网段 |
| 跨接入点路径 | 404 |
| 请求体超限 | 413；不会写半条事件 |
| 通知阶段不是 FINALIZED | 400 unsupported_phase |
| 代理未改写 Host / 用域名访问 | 400，提示 Host 必须是本机 IP 或 localhost |
| 证书不可信 / 域名不匹配 | 客户端失败；不要绕过校验 |

排错顺序：先确认应用「设置」显示运行中与实际端口，再用本机 curl http://127.0.0.1:17890/... 验证接收器；内网直连再从另一台机器 curl http://<本机IP>:17890/...，确认防火墙放行。最后才加代理与证书。代理层的 502/504 与接收器的 4xx/5xx 要分开看。

## 7. 停用、回滚与备份

- 停用：Hook → 设置关闭接收服务；代理侧删除或停用对应站点。两者互不依赖。
- 回滚：旧版不认识更高数据库版本时会拒绝打开，不要删数据库降级。回滚前先完全退出应用并备份。
- 备份：先完全退出应用并确认进程已结束，再复制整个 `shell/<mode>/hooks/` 目录（包括配置、运行安全标记、数据库、存在的 WAL/SHM 与 `blobs/`）。仅关闭接收开关不够：后台维护仍会清理数据库与附件。不要在应用运行时逐个复制文件；只复制 DB、不复制存在的 WAL 或附件会得到不一致的备份。
- 恢复演练：完全退出应用并确认进程结束 → 备份整个 hooks 目录 → 将原目录保留到另一位置 → 放回备份 → 启动 → 按提示重新确认可信网卡 → 抽查一条事件的正文、参数与附件预览。自动化用例 `backup_and_restore_drill_keeps_event_and_attachment` 仅覆盖隔离存储层的停库、合并 WAL、复制 DB+blobs 与恢复读回，不代表真实桌面退出或双机恢复验收；不要删除唯一原件。
- 凭据撤销：接入点页轮换 Bearer 令牌，旧令牌立即 401；Jenkins 通知密码单独轮换；删除接入点会让两种凭据一起失效。撤销不删除已入箱事件。
- 日志定位：管理面板「查看日志」按 hooks 逻辑名过滤；请求失败另有 request_audit（只存请求 ID、对象 ID、错误码与时间，不存正文/令牌）；数据库锁住时审计写不进去，回退到壳日志的「收件箱不可用」。
- 清理与配额：全局预算固定 5 GiB（包含正文、参数、元数据预留和附件），最多 10,000 条事件，没有运行时调优项；超出时自动清理最早的历史事件（正文与附件删除，墓碑最多保留 30 天，条数压力下优先回收空墓碑），已入箱内容不按时间删除，接收中 24 小时过期。只有接收中的事件占满且没有可清理历史事件时，新事件才会被拒绝。

### 网络变化后的重新确认

接口变化后接收会暂停，重启应用不会自动解除。应用使用配置确认 ID 与独立运行标记：监听前必须成功写入运行标记；正常停止并回收所有线程、且网络未变化时才记录安全结束。标记缺失、损坏、不匹配或显示上次未安全结束时拒绝自动监听，需在设置重新确认网卡并启用。异常退出同样保守处理；旧配置升级后需确认一次，已有来源、凭据与收件箱数据保留。写标记失败会报告错误并拒绝本次启动。但若“未安全结束”在替换前写入失败，旧的安全结束标记可能仍在；此时不能保证跨重启仍需重新确认，存储恢复后再次启动可能自动重试。应修复数据目录问题并手动重新确认，不把写入失败当作已持久锁定。运行中的接收器已在启动前成功写入未安全结束标记，因此通常的网络变化无需事后写盘。磁盘损坏、手动回滚/篡改文件不在这项保证内。

关闭接收开关使用已保存配置，无效未保存的网段草稿不会阻止关闭，也不会被顺带保存。

## 8. 已知边界

- 网卡名称/IP/掩码与在线状态不是物理网络身份凭证；如果网络切换后这些值完全相同，应用不能辨认是另一个网络。用户必须确保所选网络可信，尤其是移动设备、VPN与公共 Wi-Fi。

- 应用不做原生 TLS；绑定已确认私网接口 + 回环，来源必须在已选子网或显式额外可信网段。代理的 TLS 不关闭原生 HTTP 直连，要求强制加密时必须额外收紧防火墙。
- 没有断点续传、没有多节点、没有可靠投递队列；重试由发送方负责，Groovy / Node 发送器仅做本次执行内的有限重试，接收器只保证幂等与明确错误。
- 只有 Windows 本机（回环与同机 HTTP）自动验证；两机直连、两机 TLS、撤销、网卡变化与休眠恢复仍是 P5 的实机验收项。
- 通知适配是静态映射，未在真实 Jenkins 上验证；适配器不抓取日志/工件，也不替插件承诺投递成功。
