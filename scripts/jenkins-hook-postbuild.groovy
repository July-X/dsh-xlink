// Jenkins Freestyle / Groovy Postbuild：只修改此配置，不从构建参数读取 URL 或凭据 ID。
// Secret text 凭据必须对当前 Job 可用。脚本在 Controller JVM 执行，不需要 Node/Python。
import groovy.json.JsonOutput
import groovy.json.JsonSlurper
import com.cloudbees.plugins.credentials.CredentialsProvider
import org.jenkinsci.plugins.plaincredentials.StringCredentials
import java.security.MessageDigest
import java.time.Instant

Map HOOK = [
    url: 'http://192.168.1.10:17890/hooks/v1/endpoints/ep_example',
    credentialsId: 'dsh-hook-token',
    message: '构建通知',
    files: [], // workspace 相对路径，例如 ['reports/result.txt']，不支持 glob
    log: 'on_failure' // off / on_failure / always；最多附带 1 MiB 日志快照
]

class XlinkHookFailure extends RuntimeException {
    XlinkHookFailure(String message) { super(message) }
}

class XlinkPostbuildHook {
    static final int MIB = 1024 * 1024
    static final Map IMAGES = ['png':'image/png', 'jpg':'image/jpeg', 'jpeg':'image/jpeg', 'webp':'image/webp']

    static URI endpoint(String value) {
        URI u = new URI(value)
        String host = u.host ?: ''
        def parts = host.split('\\.', -1)
        boolean ipv4 = parts.size() == 4 && parts.every { it ==~ /0|[1-9][0-9]{0,2}/ && it.toInteger() <= 255 }
        boolean local = ipv4 && (parts[0] in ['10', '127'] || (parts[0] == '192' && parts[1] == '168') ||
            (parts[0] == '172' && parts[1].toInteger() >= 16 && parts[1].toInteger() <= 31))
        if (!local || !(u.scheme in ['http', 'https']) || u.userInfo || u.query || u.fragment ||
            !(u.rawPath ==~ /\/hooks\/v1\/endpoints\/[A-Za-z0-9_-]+/) || u.port == 0 || u.port > 65535) {
            throw new IllegalArgumentException('URL 必须为私网 IPv4/回环的完整接入点地址（不含 /events、查询参数或凭据）')
        }
        u
    }

    static byte[] bounded(InputStream stream, int limit) {
        try {
            ByteArrayOutputStream out = new ByteArrayOutputStream()
            byte[] buffer = new byte[8192]
            int n
            while ((n = stream.read(buffer)) != -1) {
                if (out.size() + n > limit) throw new IOException('内容超过大小限制')
                out.write(buffer, 0, n)
            }
            out.toByteArray()
        } finally { stream.close() }
    }

    static String sha(byte[] bytes) { MessageDigest.getInstance('SHA-256').digest(bytes).encodeHex().toString() }

    static Map item(String name, byte[] bytes, int index) {
        String ext = name.tokenize('.').last().toLowerCase(Locale.ROOT)
        String media = IMAGES[ext]
        if (!media && !(ext in ['log', 'txt'])) throw new IllegalArgumentException('附件仅支持 PNG/JPEG/WebP 或 .log/.txt')
        if (!bytes.length || bytes.length > (media ? 10 : 50) * MIB) throw new IllegalArgumentException('附件为空或超限')
        Map spec = [client_id: 'file_' + index, kind: media ? 'image' : 'log', filename: name,
                    media_type: media ?: 'text/plain', size_bytes: bytes.length]
        if (!media) spec.encoding = 'utf-8'
        [spec: spec, bytes: bytes, hash: sha(bytes)]
    }

    static Map workspaceItem(def workspace, String relative, int index) {
        if (!relative || relative.contains('\\') || relative.startsWith('/') || relative.contains(':') ||
            relative.split('/', -1).any { it in ['', '.', '..'] }) throw new IllegalArgumentException('附件必须是安全的 workspace 相对路径')
        if (workspace == null || !workspace.isDescendant(relative)) throw new IOException('workspace 或附件不可用')
        def current = workspace
        relative.split('/').each { segment ->
            current = current.child(segment)
            if (current.readLink() != null) throw new IOException('不允许符号链接附件')
        }
        String name = relative.tokenize('/').last()
        String ext = name.tokenize('.').last().toLowerCase(Locale.ROOT)
        if (!IMAGES[ext] && !(ext in ['log', 'txt'])) throw new IllegalArgumentException('附件类型不支持')
        int limit = (IMAGES[ext] ? 10 : 50) * MIB
        if (current.isDirectory() || current.length() > limit) throw new IOException('附件不可读或超限')
        item(name, bounded(current.read(), limit), index)
    }

    // 不等待构建结束；只读本次打开时可见的前 1 MiB。严禁 writeWholeLogTo()。
    static Map logItem(def build, int index) {
        Reader reader = build.getLogReader()
        ByteArrayOutputStream out = new ByteArrayOutputStream()
        boolean truncated = false
        try {
            char[] chars = new char[4096]
            int n
            String carry = ''
            while ((n = reader.read(chars)) != -1) {
                String part = carry + new String(chars, 0, n)
                carry = ''
                if (part && Character.isHighSurrogate(part.charAt(part.length() - 1))) {
                    carry = part.substring(part.length() - 1)
                    part = part.substring(0, part.length() - 1)
                }
                byte[] chunk = part.getBytes('UTF-8')
                if (out.size() + chunk.length > MIB - 128) { truncated = true; break }
                out.write(chunk)
            }
        } finally { reader.close() }
        out.write(('\n[Postbuild 日志快照；非最终完整日志' + (truncated ? '；已截断' : '') + ']\n').getBytes('UTF-8'))
        item('jenkins-console-snapshot.log', out.toByteArray(), index)
    }

    static Map request(String url, String method, String token, byte[] bytes = null, String media = null, String hash = null) {
        for (int attempt = 0; attempt < 4; attempt++) {
            HttpURLConnection connection = null
            Timer deadline = new Timer(true)
            try {
                connection = (HttpURLConnection) new URL(url).openConnection(Proxy.NO_PROXY)
                connection.instanceFollowRedirects = false
                connection.connectTimeout = 10000
                connection.readTimeout = 30000
                connection.requestMethod = method
                final HttpURLConnection timedConnection = connection
                long timeout = bytes == null ? 30000L : Math.min(300000L, Math.max(30000L, (long) Math.ceil(bytes.length / 100000.0) * 1000L))
                deadline.schedule(new TimerTask() { void run() { timedConnection.disconnect() } }, timeout)
                connection.setRequestProperty('Authorization', 'Bearer ' + token)
                if (media) connection.setRequestProperty('Content-Type', media)
                if (hash) connection.setRequestProperty('X-Content-SHA256', hash)
                if (bytes != null) {
                    connection.doOutput = true
                    connection.setFixedLengthStreamingMode(bytes.length)
                    connection.outputStream.withCloseable { it.write(bytes) }
                }
                int status = connection.responseCode
                if (status in [429, 503] && attempt < 3) { Thread.sleep(300L * (1 << attempt)); continue }
                if (status < 200 || status >= 300) throw new XlinkHookFailure('Hook HTTP ' + status + '（不跟随重定向）')
                byte[] response = bounded(connection.inputStream, MIB)
                def parsed = new JsonSlurper().parseText(new String(response, 'UTF-8'))
                if (!(parsed instanceof Map)) throw new IllegalArgumentException('Hook 响应格式无效')
                return (Map) parsed
            } catch (IOException ignored) {
                if (attempt == 3) throw new XlinkHookFailure('Hook 网络失败或超时；投递结果可能未知')
                Thread.sleep(300L * (1 << attempt))
            } finally { deadline.cancel(); connection?.disconnect() }
        }
        throw new IllegalStateException('Hook 请求未完成')
    }

    static String safeId(def value) {
        if (!(value instanceof String) || !(value ==~ /[A-Za-z0-9_-]{1,200}/)) throw new IllegalArgumentException('Hook 响应标识无效')
        value
    }

    static Map send(String root, String token, String text, String title, List items) {
        if (!token || token =~ /[\r\n]/) throw new IllegalArgumentException('Secret text 令牌无效')
        if (text.getBytes('UTF-8').length > 64 * 1024 || title.getBytes('UTF-8').length > 512) throw new IllegalArgumentException('消息或标题超限')
        if (items.size() > 10 || items.sum(0) { it.bytes.length } > 100 * MIB) throw new IllegalArgumentException('附件数量或总大小超限')
        String externalId = 'jenkins-' + UUID.randomUUID().toString()
        String occurredAt = Instant.now().toString()
        byte[] payload = JsonOutput.toJson([schema_version: '1.0', external_id: externalId, event_type: 'message',
            occurred_at: occurredAt, title: title, text: text, commit: items.isEmpty(), attachments: items.collect { it.spec }]).getBytes('UTF-8')
        Map created = request(root + '/events', 'POST', token, payload, 'application/json')
        String eventId = safeId(created.event_id)
        String eventUrl = root + '/events/' + eventId
        if (created.state != 'ready') {
            items.each { attachment ->
                def target = created.attachments?.find { it.client_id == attachment.spec.client_id }
                String attachmentId = safeId(target?.attachment_id)
                request(eventUrl + '/attachments/' + attachmentId, 'PUT', token, attachment.bytes, attachment.spec.media_type, attachment.hash)
            }
            if (items) request(eventUrl + '/commit', 'POST', token)
        }
        Map receipt = request(eventUrl + '/receipt', 'GET', token)
        if (receipt.event_id != eventId || receipt.state != 'ready') throw new IllegalArgumentException('Hook 回执未就绪或 ID 不匹配')
        items.each { attachment ->
            def stored = receipt.attachments?.find { it.client_id == attachment.spec.client_id }
            if (stored?.state != 'stored' || stored.sha256 != attachment.hash || stored.size_bytes != attachment.bytes.length)
                throw new IllegalArgumentException('Hook 回执附件摘要不匹配')
        }
        [eventId: eventId, externalId: externalId, occurredAt: occurredAt]
    }

    static String consoleUrl(def build) {
        // Jenkins Root URL 未配置时 getAbsoluteUrl() 会抛异常，而不只是返回 null。
        String base
        try { base = build.getAbsoluteUrl() }
        catch (IllegalStateException ignored) { base = null }
        (base ?: build.getUrl()) + 'console'
    }

    static void run(def manager, Map config) {
        def logger = manager.listener.logger
        try {
            String root = endpoint(config.url as String).toString()
            if (!(config.log in ['off', 'on_failure', 'always']) || !(config.files instanceof List) || config.files.size() > 10) throw new IllegalArgumentException('Hook 配置无效')
            def build = manager.build
            String status = build.result?.toString() ?: 'UNKNOWN'
            List items = []
            List warnings = []
            config.files.each { relative ->
                try {
                    Map attachment = workspaceItem(build.workspace, relative as String, items.size())
                    if (items.sum(0) { it.bytes.length } + attachment.bytes.length > 100 * MIB) throw new IOException('附件总大小超限')
                    items.add(attachment)
                }
                catch (InterruptedException e) { throw e }
                catch (Exception ignored) { warnings.add('一个配置附件不可用（路径、类型、大小、Agent 或 workspace 状态），已跳过') }
            }
            if (config.log == 'always' || (config.log == 'on_failure' && status != 'SUCCESS')) {
                try {
                    Map snapshot = logItem(build, items.size())
                    if (items.size() >= 10 || items.sum(0) { it.bytes.length } + snapshot.bytes.length > 100 * MIB) throw new IOException('日志附件超限')
                    items.add(snapshot)
                }
                catch (InterruptedException e) { throw e }
                catch (Exception ignored) { warnings.add('构建日志快照不可用，已跳过') }
            }
            String text = (config.message ?: '').toString() + '\n任务：' + build.project.fullName + '\n构建：#' + build.number +
                '\nPostbuild 状态快照：' + status + '（非严格最终状态）' + '\n控制台：' + consoleUrl(build) +
                (warnings ? '\n提示：' + warnings.join('；') : '')
            def credential = CredentialsProvider.findCredentialById(config.credentialsId as String, StringCredentials.class, build)
            if (credential == null) throw new XlinkHookFailure('找不到当前 Job 可用的 Secret text 凭据')
            // 标题固定为「任务名 #构建号 · 状态」：收件箱详情靠它对上任务与结果。
            Map result = send(root, credential.secret.plainText, text, build.project.fullName + ' #' + build.number + ' · ' + status, items)
            logger.println('[DSH Hook] 已验证接收回执：' + result.eventId)
            warnings.each { logger.println('[DSH Hook] ' + it) }
        } catch (XlinkHookFailure failure) {
            logger.println('[DSH Hook] ' + failure.message + '；构建结果未由脚本修改。')
        } catch (InterruptedException ignored) {
            Thread.currentThread().interrupt()
            logger.println('[DSH Hook] 发送中断；构建结果未由脚本修改')
        } catch (Exception ignored) {
            // 不输出异常、服务端响应、URL、令牌或用户消息，避免凭据及日志泄漏。
            logger.println('[DSH Hook] 发送失败或结果未知；请检查接收器、来源网络、配置和凭据。构建结果未由脚本修改，请勿盲目重发。')
        }
    }
}

XlinkPostbuildHook.run(manager, HOOK)
