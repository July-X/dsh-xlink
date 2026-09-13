import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import {
  incidentBannerTitle,
  incidentCause,
  incidentCauseLabel,
  incidentDestination,
  incidentDestinationLabel,
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
  const causes = ['plugin', 'kernel', 'frontend', 'env', 'unknown'];
  const titles = causes.map((cause) => incidentTitle({ cause }));
  const labels = causes.map((cause) => incidentCauseLabel({ cause }));
  assert.equal(new Set(titles).size, causes.length, '标题不能重复');
  assert.equal(new Set(labels).size, causes.length, '判断标签不能重复');
  assert.equal(incidentTitle({ cause: 'unknown' }), '工作台异常：暂未能归因');
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
