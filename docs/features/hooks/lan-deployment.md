# Hook 局域网部署与发送指南

日期：2026-11-15 · 状态：Windows 本机自动验证；两机 TLS 与真实 Jenkins 待验收

本文只讲怎么把已有的回环接收器安全地接到局域网，以及怎么发送与排错。协议字段与安全边界在[设计契约](lan-inbox-design.md)维护；阶段状态见[开发计划](development-plan.md)。

## 1. 架构与前提

    Jenkins / 脚本 / 任意 HTTP 发送方
            │  HTTPS（LAN，可选来源限制）
            ▼
    同机反向代理（TLS 终止、Host 改写、只转发 /hooks/v1/*）
            │  HTTP 127.0.0.1:17890
            ▼
    dsh-xlink Hook 接收器（只绑回环，不直接暴露局域网）

- 应用本身只监听 127.0.0.1；不要改代码让它绑 0.0.0.0，也不要把 17890 直接映射到局域网。
- 接收器默认关闭，先在「Hook → 设置」启用并确认端口（release 默认 17890，dev 默认 17891）。
- 管理面（创建接入点、读收件箱、生成凭据）只走桌面 IPC，不在这个 HTTP 端口上。代理只需转发 /hooks/v1/。
- 代理必须把上游 Host 改写成 127.0.0.1:<实际端口>：接收器用 Host 校验防 DNS rebinding，不信任 X-Forwarded-*。改写后发送方原始的域名不会传给应用，这是有意的。

## 2. 反向代理示例

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

- 证书不可信、域名不匹配、代理直连回环端口，都应失败；失败是功能的一部分，不是配置问题。
- 防火墙只放行代理的 443（或你选定的端口）与来源地址。不要关闭系统防火墙，也不要在应用里自动改防火墙规则；改动由用户明确决定并自行执行。
- 应用忽略 X-Forwarded-For / X-Real-IP；来源限制只能做在代理或防火墙上。

## 4. 发送文本、图片与日志

令牌只在创建接入点时显示一次；发送方用 Authorization: Bearer <令牌>。通用发送示例：

    # Windows PowerShell：令牌只放进当前进程环境，别写进脚本或命令历史
    $env:DSH_HOOK_TOKEN = Read-Host "Hook 令牌"
    node scripts/hooks-send-example.mjs --base https://hook.example.com --endpoint ep_xxx --kind image
    Remove-Item Env:DSH_HOOK_TOKEN

    # macOS / Linux
    read -s DSH_HOOK_TOKEN; export DSH_HOOK_TOKEN
    node scripts/hooks-send-example.mjs --base https://hook.example.com --endpoint ep_xxx --kind log
    unset DSH_HOOK_TOKEN

脚本行为：文本一次提交；图片/日志走「创建 → 上传附件 → commit → 查回执」；429/503 与网络错误有界退避，409 不盲重试。它只发合成内容，不负责脱敏。断线后先查回执：同接入点 + 同 external_id 的提交是幂等的，内容不同会得到 409。

## 5. Jenkins Notification 适配

接收器提供独立路由：

    POST /hooks/v1/endpoints/<endpoint_id>/jenkins-notification
    Authorization: Basic base64("jenkins:<通知密码>")
    Content-Type: application/json

- 通知密码与 Bearer 令牌分开生成：Hook 页「接入点 → 生成/轮换通知凭据」，只显示一次；轮换会让旧密码立即失效。
- 该适配只接受 phase = FINALIZED 的通知；STARTED / COMPLETED / QUEUED 明确返回 400 unsupported_phase，不会混进收件箱。
- 插件 1.17 的 HTTP 发送器会把 URL 里的 user:password@ 转成 Basic 头，因此密码可以放在 Secret text 凭据的 URL 里，不需要 Public URL。插件配置建议：Protocol HTTP、Format JSON、Event FINALIZED、Log lines 0；URL 用 https://jenkins:<密码>@hook.example.com/hooks/v1/endpoints/<id>/jenkins-notification，并确认代理把 Host 改写成 127.0.0.1:17890。
- 只映射元数据：Job 短名、显示名、job.url（含文件夹路径）、构建号、状态、耗时、时间、参数、SCM/测试摘要与备注。不下载日志、不抓取工件、不访问负载里的任何 URL；插件里的 log lines 保持 0。
- reported_result 只代表 Jenkins 报告的结果，收件箱不会写 verified_result，也没有「已核验」标签。需要真实构建状态时另立业务任务回查，不在接收层做。
- Job 身份优先用 job.url（区分文件夹下同名 Job）；缺失时保留事件并标记 identity_resolved=false，不用短名猜完整路径。同一构建同一终态重试返回原事件，状态变化返回 409。身份未解析且显示名相同的 Job 会共用幂等键，内容不同时保守返回 409。

> 源码依据：notification-plugin 1.17 的 Phase.java 与 Protocol.java（URL userinfo → Basic、JSON 字段下划线命名、最终事件 FINALIZED）。这是静态适配，没有真实 Jenkins 实机证据；记录实际 Jenkins / Java / 插件版本后才算兼容验收。

## 6. 验收与排错

| 场景 | 期望 |
| --- | --- |
| 可信证书 + 正确令牌 | 201/200，回执可查 |
| 错误令牌 / 撤销令牌 | 401，不泄露对象是否存在 |
| 暂停接入点 | 403，提示先在面板恢复 |
| 跨接入点路径 | 404 |
| 请求体超限 | 413；不会写半条事件 |
| 通知阶段不是 FINALIZED | 400 unsupported_phase |
| 代理未改写 Host | 400，提示 Host 与监听地址不匹配 |
| 证书不可信 / 域名不匹配 | 客户端失败；不要绕过校验 |

排错顺序：先确认应用「设置」显示运行中与实际端口，再用本机 curl http://127.0.0.1:17890/... 验证接收器，最后加上代理与证书。代理层的 502/504 与接收器的 4xx/5xx 要分开看。

## 7. 停用、回滚与备份

- 停用：Hook → 设置关闭接收服务；代理侧删除或停用对应站点。两者互不依赖。
- 回滚：旧版不认识更高数据库版本时会拒绝打开，不要删数据库降级。回滚前先停服务并备份。
- 备份：停服务后复制 shell/<mode>/hooks/config.json、inbox.db、inbox.db-wal、inbox.db-shm 与整个 hooks/blobs/；只复制 db 不复制 WAL 或文件会得到不一致的备份。恢复后启动应用并抽查一条事件与附件。
- 清理与配额：全局预算 5 GiB，超出时自动清理最早的历史事件（正文与附件删除，墓碑保留 30 天）；已入箱内容不按时间删除，接收中 24 小时过期。只有接收中的事件占满且没有可清理历史事件时，新事件才会被拒绝。

## 8. 已知边界

- 应用不做原生 TLS，也不直接绑局域网；安全前提全在代理配置与证书信任上。
- 没有断点续传、没有多节点、没有可靠投递队列；Jenkins 插件自身的重试才是唯一重试来源，接收器只保证幂等与明确错误。
- 只有回环与 Windows 本机自动验证；两机 TLS、撤销、网卡变化与休眠恢复仍是 P5 的实机验收项。
- 通知适配是静态映射，未在真实 Jenkins 上验证；适配器不抓取日志/工件，也不替插件承诺投递成功。
