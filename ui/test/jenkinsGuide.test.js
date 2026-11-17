import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';
import { computed, reactive, ref } from 'vue';
import { jenkinsScript } from '../src/hooks/jenkinsGuide.js';
const source = readFileSync(new URL('../../scripts/jenkins-hook-postbuild.groovy', import.meta.url), 'utf8');
const component = readFileSync(new URL('../src/hooks/JenkinsHookGuide.vue', import.meta.url), 'utf8');
const options = { url: 'http://10.1.2.3:17890/hooks/v1/endpoints/ep_ci', credentialsId: 'ci-secret-id', message: '构建通知', files: 'reports/a.txt\nscreenshots/a.png', log: 'on_failure', token: 'DO-NOT-INCLUDE' };
function configFrom(script) {
  const literal = script.match(/^Map HOOK = new groovy\.json\.JsonSlurper\(\)\.parseText\('(.*)'\)$/m)[1];
  return JSON.parse(literal.replace(/\\(['\\])/g, '$1'));
}
test('复制使用仓库单源脚本，只替换配置，所有业务代码逐字一致且不包含令牌', () => {
  assert.match(component, /import scriptSource from '\.\.\/\.\.\/\.\.\/scripts\/jenkins-hook-postbuild\.groovy\?raw'/);
  const script = jenkinsScript(source, options);
  assert.equal(script.slice(script.indexOf('class XlinkHookFailure')), source.slice(source.indexOf('class XlinkHookFailure')));
  assert.deepEqual(configFrom(script), { url: options.url, credentialsId: options.credentialsId, message: options.message, files: ['reports/a.txt', 'screenshots/a.png'], log: 'on_failure' });
  assert.doesNotMatch(script, /DO-NOT-INCLUDE/);
});
test('单引号编码保留美元插值、引号、换行与反斜杠为数据', () => {
  const values = { ...options, message: "${System.exit(1)} O'Reilly\nC:\\work", credentialsId: "a'b\\c" };
  assert.equal(configFrom(jenkinsScript(source, values)).message, values.message);
  assert.equal(configFrom(jenkinsScript(source, values)).credentialsId, values.credentialsId);
});
test('缺失/非LAN地址及脚本锚点变化拒绝复制，不回退演示目标', () => {
  for (const url of ['', 'http://example.com/hooks/v1/endpoints/e', 'http://8.8.8.8/hooks/v1/endpoints/e', 'http://u:p@10.1.2.3/hooks/v1/endpoints/e', options.url + '?token=x', options.url + '/events']) {
    assert.throws(() => jenkinsScript(source, { ...options, url }));
  }
  assert.throws(() => jenkinsScript(source.replace('Map HOOK = [', 'Map OTHER = ['), options));
  assert.throws(() => jenkinsScript(source, { ...options, credentialsId: '' }));
});
test('真实指导组件复制动作随URL和配置更新，停服时禁用，且只将安全生成内容送clipboard', () => {
  const props = reactive({ address: options.url });
  const copied = [];
  const body = component.split('<script setup>')[1].split('</script>')[0].replace(/^import .*;\n/gm, '');
  const state = new Function('computed', 'ref', 'scriptSource', 'jenkinsScript', 'copyText', 'defineProps', body + '\nreturn {copyScript,prepared,credentialsId,message,files,log};')(computed, ref, source, jenkinsScript, (...args) => copied.push(args), () => props);
  state.credentialsId.value = options.credentialsId; state.files.value = options.files;
  state.copyScript(); assert.equal(copied.length, 1); assert.equal(configFrom(copied[0][0]).credentialsId, options.credentialsId);
  state.message.value = 'new'; state.copyScript(); assert.equal(configFrom(copied[1][0]).message, 'new');
  props.address = ''; state.copyScript(); assert.equal(copied.length, 2); assert.match(state.prepared.value.error, /启用可信局域网/);
  props.address = 'http://10.1.2.4:17890/hooks/v1/endpoints/ep2';
  assert.equal(configFrom(state.prepared.value.script).url, props.address);
  state.credentialsId.value = ''; state.copyScript(); assert.equal(copied.length, 2); assert.ok(state.prepared.value.error);
});

test('指导说明准确标出Bearer、Controller、Postbuild与开头日志局限', () => {
  const template = component.split('<template>')[1].split('</template>')[0].replace(/<!--[\s\S]*?-->/g, '');
  for (const text of ['Bearer 令牌', 'Controller', 'Postbuild 快照', '开头最多 1 MiB', '末尾失败信息可能不在其中', '构建链接查看完整控制台']) assert.ok(template.includes(text), text);
});
