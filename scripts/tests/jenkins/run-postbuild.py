#!/usr/bin/env python3
"""Actual Jenkins/Groovy Postbuild integration, isolated loopback instance.
Usage: python3 run-postbuild.py --war /path/jenkins.war --plugins /path/plugins
Requires Java 21+, Python 3; plugin directory must contain groovy-postbuild,
plain-credentials and their dependencies. No real user data or secrets used.
The HTTP peer is a protocol fixture, NOT the production Rust receiver.
"""
import signal
import argparse, hashlib, json, os, pathlib, subprocess, tempfile, threading, time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

parser=argparse.ArgumentParser(); parser.add_argument('--war',required=True); parser.add_argument('--plugins',required=True)
a=parser.parse_args(); root=pathlib.Path(tempfile.mkdtemp(prefix='xlink-jenkins-integration-')); home=root/'home'; (home/'init.groovy.d').mkdir(parents=True)
import shutil
shutil.copytree(a.plugins,home/'plugins')
source=(pathlib.Path(__file__).resolve().parents[2]/'jenkins-hook-postbuild.groovy').read_text()
(root/'sender.groovy').write_text(source)
(root/'outside.txt').write_text('synthetic-outside-sentinel')
events={}; requests=[]; create_bodies=[]
class Receiver(BaseHTTPRequestHandler):
 def log_message(self,*args): pass
 def do_GET(self): self.handle_request()
 def do_POST(self): self.handle_request()
 def do_PUT(self): self.handle_request()
 def handle_request(self):
  body=self.rfile.read(int(self.headers.get('Content-Length','0'))); requests.append((self.command,self.path))
  if self.headers.get('Authorization')!='Bearer synthetic-test-token': return self.reply(401,{})
  if '/redirect/' in self.path:
   self.send_response(302); self.send_header('Location',f'http://127.0.0.1:{self.server.server_port}/leaked'); self.end_headers(); return
  if self.path.endswith('/events'):
   create_bodies.append(body)
   if len(create_bodies)==1: return self.reply(503,{})
   p=json.loads(body); eid='evt_'+str(len(events)); ev={'event_id':eid,'state':'ready' if p['commit'] else 'pending','payload':p,'attachments':[dict(s,attachment_id='att_'+str(i)) for i,s in enumerate(p['attachments'])]}; events[eid]=ev; return self.reply(200,ev)
  bits=self.path.split('/'); eid=bits[6]; ev=events[eid]
  if self.command=='PUT':
   att=next(x for x in ev['attachments'] if x['attachment_id']==bits[-1]); assert len(body)==att['size_bytes']; assert hashlib.sha256(body).hexdigest()==self.headers['X-Content-SHA256']; att.update(state='stored',sha256=hashlib.sha256(body).hexdigest(),body=body.decode('utf8','replace')); return self.reply(200,{})
  if bits[-1]=='commit': ev['state']='ready'
  self.reply(200,ev)
 def reply(self,status,data):
  b=json.dumps(data).encode(); self.send_response(status); self.send_header('Content-Type','application/json'); self.send_header('Content-Length',str(len(b))); self.end_headers(); self.wfile.write(b)
server=ThreadingHTTPServer(('127.0.0.1',0),Receiver); threading.Thread(target=server.serve_forever,daemon=True).start()
# Let Jenkins choose a free controller port reserved immediately before launch.
import socket
s=socket.socket(); s.bind(('127.0.0.1',0)); port=s.getsockname()[1]; s.close()
import zipfile
with zipfile.ZipFile(a.war) as war: remoting=next(n for n in war.namelist() if n.startswith('WEB-INF/lib/remoting-') and n.endswith('.jar')); (root/'remoting.jar').write_bytes(war.read(remoting))
init=r'''
import jenkins.model.Jenkins
import hudson.model.*
import hudson.tasks.Shell
import org.jvnet.hudson.plugins.groovypostbuild.GroovyPostbuildRecorder
import org.jenkinsci.plugins.scriptsecurity.sandbox.groovy.SecureGroovyScript
import org.jenkinsci.plugins.scriptsecurity.scripts.ScriptApproval
import org.jenkinsci.plugins.scriptsecurity.scripts.languages.GroovyLanguage
import com.cloudbees.plugins.credentials.*
import com.cloudbees.plugins.credentials.domains.Domain
import org.jenkinsci.plugins.plaincredentials.impl.StringCredentialsImpl
import hudson.util.Secret
import groovy.json.JsonOutput
Thread.start {
 try {
  while (Jenkins.get().initLevel.name() != 'COMPLETED') Thread.sleep(200)
  def j=Jenkins.get()
  j.setNumExecutors(2)
  def node=new hudson.slaves.DumbSlave('test-agent','@ROOT@/agent',new hudson.slaves.JNLPLauncher())
  node.setNumExecutors(1); node.setMode(Node.Mode.EXCLUSIVE); j.addNode(node)
  def agent=new ProcessBuilder('java','-jar','@ROOT@/remoting.jar','-url','http://127.0.0.1:@PORT@/','-secret',node.computer.jnlpMac,'-name','test-agent','-webSocket','-workDir','@ROOT@/agent-work').redirectErrorStream(true).redirectOutput(new File('@ROOT@/agent.log')).start()
  long until=System.currentTimeMillis()+45000
  while (node.computer.offline && System.currentTimeMillis()<until) Thread.sleep(200)
  assert !node.computer.offline : 'agent did not connect'

  SystemCredentialsProvider.getInstance().store.addCredentials(Domain.global(), new StringCredentialsImpl(CredentialsScope.GLOBAL,'test-hook','synthetic only',Secret.fromString('synthetic-test-token')))
  String source=new File('@ROOT@/sender.groovy').getText('UTF-8')
  def results=[]
  [
    [name:'success', command:'mkdir -p reports; echo fixture-file > reports/result.txt; echo console-success', files:['reports/result.txt'], log:'always', credential:'test-hook', endpoint:'test', expected:'SUCCESS'],
    [name:'failure', command:'echo console-failure; exit 1', files:[], log:'on_failure', credential:'test-hook', endpoint:'test', expected:'FAILURE'],
    [name:'remote', command:'mkdir -p reports; echo fixture-remote > reports/remote.txt; ln -s @ROOT@/outside.txt escape.txt; echo console-remote', files:['reports/remote.txt','escape.txt'], log:'off', credential:'test-hook', endpoint:'test', expected:'SUCCESS'],
    [name:'missing', command:'echo console-missing', files:['missing.txt'], log:'off', credential:'test-hook', endpoint:'test', expected:'SUCCESS'],
    [name:'credential', command:'echo console-credential', files:[], log:'off', credential:'not-found', endpoint:'test', expected:'SUCCESS'],
    [name:'redirect', command:'echo console-redirect', files:[], log:'off', credential:'test-hook', endpoint:'redirect', expected:'SUCCESS']
  ].each { c ->
    def p=j.createProject(FreeStyleProject,c.name)
    if(c.name=='remote') p.setAssignedLabel(j.getLabel('test-agent'))
    p.buildersList.add(new Shell(c.command))
    Map config=[url:'http://127.0.0.1:@RECEIVER@/hooks/v1/endpoints/'+c.endpoint, credentialsId:c.credential, message:'integration '+c.name, files:c.files, log:c.log]
    String script=source.replaceFirst(/(?s)Map HOOK = \[.*?\n\]/,java.util.regex.Matcher.quoteReplacement('Map HOOK = new groovy.json.JsonSlurper().parseText('+JsonOutput.toJson(JsonOutput.toJson(config))+')'))
    ScriptApproval.get().preapprove(script,GroovyLanguage.get())
    p.publishersList.add(new GroovyPostbuildRecorder(new SecureGroovyScript(script,false,null),0,false))
    p.save()
    def b=p.scheduleBuild2(0).get()
    String log=b.getLog(1000).join('\n')
    assert b.result.toString()==c.expected
    assert !log.contains('synthetic-test-token')
    new File('@ROOT@/'+c.name+'.log').setText(log,'UTF-8')
    results << [name:c.name,result:b.result.toString(),sent:log.contains('已验证接收回执')]
  }
  new File('@ROOT@/result.json').text=JsonOutput.toJson(results)
 } catch(Throwable t) {
  new File('@ROOT@/error.txt').text=t.toString()+'\n'+t.stackTrace.join('\n')
 }
}
'''.replace('@ROOT@',str(root)).replace('@RECEIVER@',str(server.server_port)).replace('@PORT@',str(port))
(home/'init.groovy.d/test.groovy').write_text(init)
log=open(root/'jenkins.log','w')
proc=subprocess.Popen(['java','-Djenkins.install.runSetupWizard=false','-jar',str(pathlib.Path(a.war).resolve()),'--httpListenAddress=127.0.0.1',f'--httpPort={port}'],env={**os.environ,'JENKINS_HOME':str(home)},stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
try:
 deadline=time.monotonic()+180
 while time.monotonic()<deadline and not (root/'result.json').exists() and not (root/'error.txt').exists() and proc.poll() is None: time.sleep(1)
 if (root/'error.txt').exists(): raise AssertionError((root/'error.txt').read_text())
 assert (root/'result.json').exists(), 'Jenkins did not complete: '+str(root/'jenkins.log')
 results=json.loads((root/'result.json').read_text()); assert [x['sent'] for x in results]==[True,True,True,True,False,False], results
 assert not any(p=='/leaked' for _,p in requests),'redirect followed'
 assert len(events)==4, events
 assert create_bodies[0]==create_bodies[1], 'retry mutated event ID/time/payload'
 assert any('FAILURE' in ev['payload']['text'] for ev in events.values()), 'failure snapshot missing'
 assert all('非严格最终状态' in ev['payload']['text'] for ev in events.values())
 for ev in events.values(): assert ev['state']=='ready'
 assert any('fixture-file' in x.get('body','') for ev in events.values() for x in ev['attachments'])
 assert any('fixture-remote' in x.get('body','') for ev in events.values() for x in ev['attachments'])
 assert 'synthetic-outside-sentinel' not in json.dumps(events)
 assert not any(x['filename']=='escape.txt' for ev in events.values() for x in ev['attachments'])
 assert any('console-failure' in x.get('body','') for ev in events.values() for x in ev['attachments'])
 (root/'source.sha256').write_text(hashlib.sha256(source.encode()).hexdigest())
 (root/'http-evidence.json').write_text(json.dumps({'events':events,'requests':requests},ensure_ascii=False,indent=2))
 print(json.dumps({'passed':results,'evidence':str(root)},ensure_ascii=False))
finally:
 try: os.killpg(proc.pid,signal.SIGTERM)
 except ProcessLookupError: pass
 try: proc.wait(timeout=20)
 except subprocess.TimeoutExpired: proc.kill();proc.wait()
 server.shutdown();log.close()
