import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import {
  incidentBannerTitle,
  incidentCause,
  incidentCauseLabel,
  incidentDestination,
  incidentDestinationLabel,
  incidentHealthKindLabel,
  incidentHealthPlainText,
  incidentHealthSections,
  bundleRouteMembers,
  incidentTitle,
} from '../src/incidents.js';

// Rust 侧 guard.rs 会产出 cause = "env"（端口被占用 / 权限 / 磁盘等环境类失败）。
// 前端两个组件此前各自维护一份白名单，都漏了 env，于是后端给出的明确环境原因在面板上
// 回落成「暂未能归因」，把用户引向插件与内核重装（P2-3/P2-4 的文案修复只在后端生效）。
test('环境类事故是头等归因，不再回落成「暂未能归因」', () => {
  const env = { cause: 'env', message: '工作台无法启动：内核进程没有被拉起来：端口被占用' };
  assert.equal(incidentCause(env), 'env');
  assert.match(incidentTitle(env), /运行环境问题/);
  assert.match(incidentCauseLabel(env), /运行环境问题/);
  assert.doesNotMatch(incidentTitle(env), /暂未能归因/);
  assert.doesNotMatch(incidentCauseLabel(env), /暂未能归因/);
});

test('每种已知归因都有自己的标题与判断标签', () => {
  const causes = ['plugin', 'kernel', 'kernel-boot', 'frontend', 'env', 'unknown'];
  const titles = causes.map((cause) => incidentTitle({ cause }));
  const labels = causes.map((cause) => incidentCauseLabel({ cause }));
  assert.equal(new Set(titles).size, causes.length, '标题不能重复');
  assert.equal(new Set(labels).size, causes.length, '判断标签不能重复');
  assert.equal(incidentTitle({ cause: 'unknown' }), '工作台异常：暂未能归因');
});

test('内核启动顺序未就绪单独成类，不与「内核组件出错」混为一谈', () => {
  // 两者归因都是内核，但第一动作不同：一个是刷新工作台多半就好了，一个是查
  // 日志 / 换版本。混成一条，「先刷新」就会被稀释成又一条泛泛建议。
  const boot = { cause: 'kernel-boot' };
  assert.equal(incidentCause(boot), 'kernel-boot');
  assert.match(incidentTitle(boot), /启动顺序/);
  assert.match(incidentCauseLabel(boot), /非插件问题/);
  assert.doesNotMatch(incidentCauseLabel(boot), /暂未能归因/);
  assert.equal(incidentDestination(boot), 'versions', '落点仍是内核版本页');
});

test('老记录缺 cause 时按嫌疑对象回退，未知取值回落到 unknown', () => {
  assert.equal(incidentCause({ suspects: [{ kind: 'plugin' }] }), 'plugin');
  assert.equal(incidentCause({ suspects: [{ kind: 'kernel' }] }), 'kernel');
  assert.equal(incidentCause({ suspects: [] }), 'unknown');
  assert.equal(incidentCause(null), '');
  // 后端将来新增归因值时，前端必须显示成「暂未能归因」而不是空白。
  assert.equal(incidentCause({ cause: 'brand-new-cause' }), 'unknown');
  assert.equal(incidentTitle({ cause: 'brand-new-cause' }), '工作台异常：暂未能归因');
});

test('安全模式恢复与横幅标题各走各的口径', () => {
  assert.equal(incidentTitle({ recovered: true, cause: 'plugin' }), '已在安全模式下启动工作台');
  assert.match(incidentBannerTitle({ cause: 'frontend' }), /前端异常（页面正常）/);
  assert.match(incidentBannerTitle({ cause: 'env' }), /运行环境问题/);
  assert.equal(incidentBannerTitle({ cause: 'kernel' }), '启动容错已介入');
});

test('环境类事故的下一步是设置页，而不是内核版本页', () => {
  // 端口占用/权限问题的出路在设置页与日志，去内核版本页会把用户引向重装内核。
  assert.equal(incidentDestination({ cause: 'env' }), 'settings');
  assert.equal(incidentDestinationLabel('settings'), '前往设置页');
  assert.equal(incidentDestination({ cause: 'plugin' }), 'plugins');
  assert.equal(incidentDestination({ cause: 'plugin', recovered: true }, 2), 'plugins');
  assert.equal(incidentDestination({ cause: 'unknown' }), 'versions');
  assert.equal(incidentDestination({ cause: 'kernel' }), 'versions');
  // 有插件被隔离时永远先去插件页，即使归因是别的（隔离本身要用户裁决）。
  assert.equal(incidentDestination({ cause: 'env' }, 1), 'plugins');
  assert.equal(incidentDestinationLabel('versions'), '前往内核版本页');
});

test('两个组件共用同一份归因口径，不再各写一份白名单', () => {
  for (const file of ['ui/src/components/OverviewPanel.vue', 'ui/src/components/IncidentModal.vue']) {
    const source = readFileSync(file, 'utf8');
    assert.match(source, /from '\.\.\/incidents\.js'/, `${file} 必须从共享层取归因口径`);
    // 白名单如果又抄回组件里，这里立刻变红（这正是当初漏掉 env 的形态）。
    assert.doesNotMatch(
      source,
      /'plugin',\s*'kernel',\s*'frontend'/,
      `${file} 又出现了本地 cause 白名单，请改用 incidents.js`,
    );
  }
});

test('自检类型印成人话，后端新增的字面量原样透出而不是空白', () => {
  assert.equal(incidentHealthKindLabel('bundle-load-failure'), '内核客户端模块 bundle 加载失败');
  assert.equal(incidentHealthKindLabel('runtime-error'), '运行时错误');
  assert.equal(incidentHealthKindLabel('unhandled-rejection'), '未处理的 Promise 异常');
  assert.equal(incidentHealthKindLabel('blank'), '页面空白（无可见内容）');
  assert.equal(incidentHealthKindLabel('brand-new-kind'), 'brand-new-kind');
  assert.equal(incidentHealthKindLabel(''), '');
});

test('组合路由里的包名被拆出来给人看', () => {
  // 真实事故形态：一行 57 个成员的组合地址，此前整段塞进一个 <pre>，没人读得动。
  const combo =
    'http://127.0.0.1:3090/plugins/??@deepseek-ai/dsh-client-ui-open-in-app/client.js,' +
    '@deepseek-ai/dsh-api-gateway/client.js,dsh-opencode-session/client.js&rev=1473b6fd8f68';
  assert.deepEqual(bundleRouteMembers(combo), [
    '@deepseek-ai/dsh-client-ui-open-in-app',
    '@deepseek-ai/dsh-api-gateway',
    'dsh-opencode-session',
  ]);
  // 单资源形态（内核回退到自己的一个资源 URL 时）同样要认。
  assert.deepEqual(bundleRouteMembers('http://127.0.0.1:3090/plugins/dsh-sidebar/client.chat.js?rev=ab'), [
    'dsh-sidebar',
  ]);
  // 重复出现只保留一份，顺序按首次出现。
  assert.deepEqual(bundleRouteMembers(`${combo}\n${combo}`).length, 3);
  assert.deepEqual(bundleRouteMembers(''), []);
  assert.deepEqual(bundleRouteMembers(null), []);
  assert.deepEqual(bundleRouteMembers('at RootOutlet (http://127.0.0.1:3090/plugins/:148814:26)'), []);
});

test('证据分段把最有用的一项顶出来，其余原样保留', () => {
  const health = {
    kind: 'bundle-load-failure',
    message:
      '内核客户端模块 bundle 加载失败：http://127.0.0.1:3090/plugins/??dsh-a/client.js,dsh-b/client.js&rev=1',
    stack: 'http://127.0.0.1:3090/plugins/??dsh-a/client.js,dsh-b/client.js&rev=1',
    page_url: 'http://127.0.0.1:3090/',
  };
  const sections = incidentHealthSections(health);
  assert.deepEqual(
    sections.map((section) => section.label),
    ['类型', '涉及的客户端模块（2 个）', '原始证据', '页面']
  );
  assert.equal(sections[0].text, '内核客户端模块 bundle 加载失败');
  assert.deepEqual(sections[1].members, ['dsh-a', 'dsh-b']);
  // 消息只是「前缀 + 那个地址」，地址已单列成一节、证据里也原样在，不重复印。
  assert.equal(sections.some((section) => section.label === '消息'), false);
  assert.deepEqual(incidentHealthSections(null), []);
});

test('普通运行时错误的「消息」不会被这条去重规则吃掉', () => {
  const sections = incidentHealthSections({
    kind: 'runtime-error',
    message: '组件初始化失败',
    stack: 'Error: 组件初始化失败\n    at mount (http://127.0.0.1:3090/plugins/ghost/main.js:1:1)',
    page_url: 'http://127.0.0.1:3090/',
  });
  assert.deepEqual(
    sections.map((section) => section.label),
    ['类型', '消息', '原始证据', '页面']
  );
  assert.equal(sections[1].text, '组件初始化失败');
});

test('复制证据给出的是分段后的完整文本', () => {
  const text = incidentHealthPlainText({
    kind: 'bundle-load-failure',
    message: '加载失败：http://127.0.0.1:3090/plugins/??dsh-a/client.js&rev=1',
    stack: 'http://127.0.0.1:3090/plugins/??dsh-a/client.js&rev=1',
    page_url: 'http://127.0.0.1:3090/',
  });
  assert.match(text, /类型：内核客户端模块 bundle 加载失败/);
  assert.match(text, /涉及的客户端模块（1 个）：dsh-a/);
  assert.match(text, /页面：http:\/\/127\.0\.0\.1:3090\//);
  assert.equal(incidentHealthPlainText(null), '');
});
