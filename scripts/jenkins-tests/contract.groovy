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
String receiptMode = ''
server.createContext('/', { exchange ->
    byte[] bytes = exchange.requestBody.bytes
    requests.add([path:exchange.requestURI.path, method:exchange.requestMethod, bytes:bytes,
                  auth:exchange.requestHeaders.getFirst('Authorization')])
    Map result = [:]
    int code = 200
    String path = exchange.requestURI.path
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
    } else if (path.endsWith('/receipt')) {
        result = [event_id:receiptMode == 'id' ? 'wrong' : 'evt_test',state:receiptMode == 'state' ? 'draft' : 'ready',attachments:payload.attachments.collect {
            byte[] content = uploaded['att_' + it.client_id]
            [client_id:it.client_id,state:'stored',sha256:receiptMode == 'hash' ? 'wrong' : hook.sha(content),size_bytes:receiptMode == 'size' ? 999 : content.length]
        }]
    }
    byte[] response = JsonOutput.toJson(result).getBytes('UTF-8')
    exchange.sendResponseHeaders(code, response.length)
    exchange.responseBody.write(response)
    exchange.close()
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
    check(requests.size() == throttledStart + 4)
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
} finally { server.stop(0) }
println("Groovy contract fixture: ${checks} checks passed (not a real Jenkins integration test)")
