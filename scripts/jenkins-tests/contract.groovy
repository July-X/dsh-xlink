// 纯 Groovy 合约测试：使用 Jenkins WAR 的 groovy-all jar；Jenkins API 由测试替身提供。
// 不替代真实 Jenkins / 远程 Agent 验收。示例命令见使用手册。
import com.sun.net.httpserver.HttpServer
import groovy.json.JsonOutput
import groovy.json.JsonSlurper

File source = new File(args ? args[0] : 'scripts/jenkins-hook-postbuild.groovy')
GroovyClassLoader loader = new GroovyClassLoader(getClass().classLoader)
loader.parseClass('''package com.cloudbees.plugins.credentials
class CredentialsProvider { static findCredentialById(id,type,build) { [secret:[plainText:'fixture-secret']] } }
''')
loader.parseClass('''package org.jenkinsci.plugins.plaincredentials
interface StringCredentials {}
''')
loader.parseClass(source)
def hook = loader.loadClass('XlinkPostbuildHook')
def workspaceType = loader.parseClass('''class FixtureFilePath {
    File file
    FixtureFilePath(File file) { this.file = file }
    boolean isDescendant(String relative) { new File(file, relative).canonicalPath.startsWith(file.canonicalPath + File.separator) }
    FixtureFilePath child(String relative) { new FixtureFilePath(new File(file, relative)) }
    String readLink() { java.nio.file.Files.isSymbolicLink(file.toPath()) ? java.nio.file.Files.readSymbolicLink(file.toPath()).toString() : null }
    boolean isDirectory() { file.isDirectory() }
    long length() { file.length() }
    InputStream read() { new FileInputStream(file) }
}''')
def noRootBuild = loader.parseClass('''class NoRootBuild {
    String getAbsoluteUrl() { throw new IllegalStateException('Jenkins root URL is not set') }
    String getUrl() { 'job/test/12/' }
}''').newInstance()
int checks = 0
def check = { boolean value -> assert value; checks++ }
def rejected = { Closure action ->
    boolean failed = false
    try { action() } catch (Exception ignored) { failed = true }
    check(failed)
}
File workspaceRoot = java.nio.file.Files.createTempDirectory('xlink-groovy-fixture-').toFile()
try {
    new File(workspaceRoot, 'report.txt').text = 'workspace bytes'
    def workspace = workspaceType.newInstance(workspaceRoot)
    def fileItem = hook.workspaceItem(workspace, 'report.txt', 0)
    check(new String(fileItem.bytes, 'UTF-8') == 'workspace bytes')
    java.nio.file.Files.createSymbolicLink(new File(workspaceRoot, 'link.txt').toPath(), new File(workspaceRoot, 'report.txt').toPath())
    rejected { hook.workspaceItem(workspace, 'link.txt', 0) }
} finally { workspaceRoot.deleteDir() }
check(hook.consoleUrl(noRootBuild) == 'job/test/12/console')
['http://127.0.0.1:1234/hooks/v1/endpoints/e', 'https://10.0.0.1/hooks/v1/endpoints/e'].each { check(hook.endpoint(it) != null) }
['http://example.com/hooks/v1/endpoints/e', 'http://8.8.8.8/hooks/v1/endpoints/e',
 'http://127.0.0.1/hooks/v1/endpoints/e?q=x', 'http://user:pw@127.0.0.1/hooks/v1/endpoints/e',
 'http://127.0.0.1/hooks/v1/endpoints/e/events', 'http://0177.0.0.1/hooks/v1/endpoints/e'].each { bad -> rejected { hook.endpoint(bad) } }
rejected { hook.bounded(new ByteArrayInputStream(new byte[5]), 4) }
rejected { hook.item('a.zip', [1] as byte[], 0) }
rejected { hook.item('a.log', new byte[0], 0) }
rejected { hook.item('a.png', new byte[10 * 1024 * 1024 + 1], 0) }
rejected { hook.item('a.txt', new byte[50 * 1024 * 1024 + 1], 0) }
['../x.log','/tmp/x.log','C:/x.log','a\\x.log','a//x.log'].each { path -> rejected { hook.workspaceItem(null, path, 0) } }
def snapshot = hook.logItem([getLogReader:{ new StringReader('x' * (2 * 1024 * 1024)) }], 0)
check(snapshot.bytes.length <= 1024 * 1024)
check(new String(snapshot.bytes, 'UTF-8').contains('已截断'))
String unicodeLog = ('x' * 4095) + '😀' + '尾部'
def unicodeSnapshot = hook.logItem([getLogReader:{ new StringReader(unicodeLog) }], 0)
check(new String(unicodeSnapshot.bytes, 'UTF-8').startsWith(unicodeLog))

HttpServer server = HttpServer.create(new InetSocketAddress('127.0.0.1', 0), 0)
List requests = Collections.synchronizedList([])
Map payload = [:]
Map uploaded = [:]
int createCount = 0
boolean redirect = false
int forcedCode = 0
int uploadDelay = 0
def httpWorkers = java.util.concurrent.Executors.newCachedThreadPool()
server.executor = httpWorkers
String receiptMode = ''
server.createContext('/', { exchange ->
    byte[] bytes = exchange.requestBody.bytes
    requests.add([path:exchange.requestURI.path, method:exchange.requestMethod, bytes:bytes,
                  auth:exchange.requestHeaders.getFirst('Authorization')])
    Map result = [:]
    int code = 200
    String path = exchange.requestURI.path
    if (path.endsWith('/stall') || path.endsWith('/drip') || path.endsWith('/delay')) {
        try {
            if (path.endsWith('/stall')) Thread.sleep(5000)
            if (path.endsWith('/delay')) Thread.sleep(250)
            exchange.sendResponseHeaders(200, 0)
            if (path.endsWith('/drip')) {
                100.times { exchange.responseBody.write(32); exchange.responseBody.flush(); Thread.sleep(30) }
            } else exchange.responseBody.write('{}'.getBytes('UTF-8'))
        } catch (IOException ignored) {
            // 客户端按预算断开是本用例的预期结果。
        } catch (InterruptedException ignored) {
            Thread.currentThread().interrupt()
        } finally { exchange.close() }
        return
    }
    if (forcedCode) { code = forcedCode }
    else if (redirect) { code = 302; exchange.responseHeaders.set('Location', 'http://127.0.0.1:1/leak') }
    else if (path.endsWith('/events')) {
        createCount++
        payload = new JsonSlurper().parseText(new String(bytes, 'UTF-8'))
        if (createCount == 1) code = 503
        else result = [event_id:'evt_test', state:'draft', attachments:payload.attachments.collect { [client_id:it.client_id, attachment_id:'att_' + it.client_id] }]
    } else if (path.contains('/attachments/')) {
        uploaded[exchange.requestURI.path.tokenize('/').last()] = bytes
        assert exchange.requestHeaders.getFirst('X-Content-SHA256') == hook.sha(bytes)
        if (uploadDelay) Thread.sleep(uploadDelay)
    } else if (path.endsWith('/receipt')) {
        result = [event_id:receiptMode == 'id' ? 'wrong' : 'evt_test',state:receiptMode == 'state' ? 'draft' : 'ready',attachments:payload.attachments.collect {
            byte[] content = uploaded['att_' + it.client_id]
            [client_id:it.client_id,state:'stored',sha256:receiptMode == 'hash' ? 'wrong' : hook.sha(content),size_bytes:receiptMode == 'size' ? 999 : content.length]
        }]
    }
    byte[] response = JsonOutput.toJson(result).getBytes('UTF-8')
    try {
        exchange.sendResponseHeaders(code, response.length)
        exchange.responseBody.write(response)
    } catch (IOException ignored) {
        // 总预算耗尽后，客户端可能已关闭连接。
    } finally { exchange.close() }
} as com.sun.net.httpserver.HttpHandler)
server.start()
try {
    String root = 'http://127.0.0.1:' + server.address.port + '/hooks/v1/endpoints/e'
    def attachment = hook.item('result.log', 'hello'.getBytes('UTF-8'), 0)
    def result = hook.send(root, 'fixture-secret', '消息', '测试', [attachment])
    check(result.eventId == 'evt_test')
    check(createCount == 2)
    check(Arrays.equals(requests[0].bytes, requests[1].bytes))
    check(requests.every { it.auth == 'Bearer fixture-secret' })
    check(new String(uploaded.att_file_0, 'UTF-8') == 'hello')
    check(requests*.method == ['POST','POST','PUT','POST','GET'])
    redirect = true
    int before = requests.size()
    rejected { hook.send(root, 'fixture-secret', 'x', 'x', []) }
    check(requests.size() == before + 1)
    redirect = false
    [401, 403].each { status ->
        forcedCode = status
        int count = requests.size()
        rejected { hook.send(root, 'fixture-secret', 'x', 'x', []) }
        check(requests.size() == count + 1)
    }
    forcedCode = 429
    int throttledStart = requests.size()
    rejected { hook.send(root, 'fixture-secret', 'x', 'x', []) }
    check(requests.size() == throttledStart + 2)
    check(requests.subList(throttledStart, requests.size()).every { Arrays.equals(it.bytes, requests[throttledStart].bytes) })
    forcedCode = 0
    ['id','state','hash','size'].each { mode ->
        receiptMode = mode
        rejected { hook.send(root, 'fixture-secret', 'x', 'x', [attachment]) }
    }
    receiptMode = ''
    ByteArrayOutputStream logs = new ByteArrayOutputStream()
    def build = [result:null, project:[fullName:'job'], number:12, getAbsoluteUrl:{ 'http://jenkins/job/test/12/' }, workspace:null]
    def manager = [build:build,listener:[logger:new PrintStream(logs)]]
    hook.run(manager, [url:root,credentialsId:'fixture',message:'done',files:[],log:'off'])
    check(payload.text.contains('UNKNOWN'))
    check(build.result == null)
    check(!logs.toString('UTF-8').contains('fixture-secret'))
    hook.run(manager, [url:'not-valid',credentialsId:'fixture',message:'private',files:[],log:'off'])
    check(build.result == null)
    check(!logs.toString('UTF-8').contains('private'))

    int count = requests.size()
    rejected { hook.request(root + '/expired', 'GET', 'fixture-secret', System.nanoTime() - 1L) }
    check(requests.size() == count)
    Thread.currentThread().interrupt()
    try {
        boolean interrupted = false
        try { hook.request(root + '/cancelled', 'GET', 'fixture-secret', System.nanoTime() + 1000000000L) }
        catch (InterruptedException expected) { interrupted = true }
        check(interrupted)
    } finally { Thread.interrupted() }
    check(requests.size() == count)

    // 无响应头、持续滴流都必须由总预算中断，不能等到 5 秒空闲超时。
    ['stall', 'drip'].each { mode ->
        int beforeTimeout = requests.size()
        long began = System.nanoTime()
        rejected { hook.request(root + '/' + mode, 'GET', 'fixture-secret', began + 300000000L) }
        check(System.nanoTime() - began < 2000000000L)
        check(requests.size() == beforeTimeout + 1)
    }
    // 两个串行步骤传同一截止点，第二步只能使用第一步剩下的时间。
    long began = System.nanoTime()
    long until = began + 450000000L
    int beforeSequence = requests.size()
    check(hook.request(root + '/delay', 'GET', 'fixture-secret', until) == [:])
    rejected { hook.request(root + '/delay', 'GET', 'fixture-secret', until) }
    check(System.nanoTime() - began < 2000000000L)
    check(requests.size() == beforeSequence + 2)

    // 预算连重试等待都不够时，不得开启第二次尝试。
    forcedCode = 503
    int beforeRetry = requests.size()
    rejected { hook.request(root + '/retry-budget', 'POST', 'fixture-secret', System.nanoTime() + 100000000L) }
    check(requests.size() == beforeRetry + 1)

    // 直接跑生产 send：每个上传均小于 5 秒，但累计超过 30 秒时必须停止。
    forcedCode = 0
    uploadDelay = 4000
    List many = (0..<10).collect { hook.item('part' + it + '.txt', [120] as byte[], it) }
    int beforeSend = requests.size()
    long sendBegan = System.nanoTime()
    rejected { hook.send(root, 'fixture-secret', 'budget', 'budget', many) }
    long elapsed = System.nanoTime() - sendBegan
    check(elapsed >= 29000000000L && elapsed < 35000000000L)
    def limitedRequests = requests.subList(beforeSend, requests.size())
    check(limitedRequests.count { it.method == 'PUT' } < 10)
    check(!limitedRequests.any { it.path.endsWith('/commit') || it.path.endsWith('/receipt') })
} finally { server.stop(0); httpWorkers.shutdownNow() }
println("Groovy contract fixture: ${checks} checks passed (not a real Jenkins integration test)")
