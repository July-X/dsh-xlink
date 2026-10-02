import assert from 'node:assert/strict';
import test from 'node:test';

let pluginChecks = 0;
let skillChecks = 0;
// 挂起钩子：置为一个带 promise/resolve 的对象时，下一次对应命令的 invoke
// 挂住不返回，让测试能在「探测进行中」观察全局按钮状态。
let pluginCheckGate = null;
let fetchReleasesGate = null;
// 静默路径的两条分支要用：一份带 upgrade 的成功结果、一份整体失败。
let fetchReleasesResult = { releases: [], warning: '' };
let fetchReleasesFail = false;
const deferred = () => {
  let resolve;
  const promise = new Promise((res) => {
    resolve = res;
  });
  return { promise, resolve };
};
const status = {
  kernel: { running: false, active: '0.1.1', active_installed: true, installed: ['0.1.1'] },
  node: { ok: true, path: '/node', version: '22.19.0' },
  settings: { port: 3090, profile: 'web' },
  shell_version: '0.1.1-rc.10',
  dev_build: false,
  quarantined: [],
  last_incident: null,
  official_chat_open: false,
};

globalThis.window = {
  __TAURI__: {
    core: {
      invoke(command) {
        if (command === 'plugin_check_updates') {
          pluginChecks += 1;
          if (pluginCheckGate) {
            const gate = pluginCheckGate;
            pluginCheckGate = null;
            return gate.promise;
          }
          if (pluginChecks === 1) return Promise.reject(new Error('temporary failure'));
          return Promise.resolve([]);
        }
        if (command === 'fetch_releases') {
          if (fetchReleasesGate) {
            const gate = fetchReleasesGate;
            fetchReleasesGate = null;
            return gate.promise;
          }
          if (fetchReleasesFail) return Promise.reject(new Error('registry unreachable'));
          return Promise.resolve(fetchReleasesResult);
        }
        if (command === 'skill_check_updates') {
          skillChecks += 1;
          // 逐包失败：至少一个包带 error（P2-12 的失败退避与静默路径都由它触发）。
          return Promise.resolve([{ id: 'ghost', error: 'network unreachable' }]);
        }
        if (command === 'get_status') return Promise.resolve(status);
        if (command === 'plugin_status') return Promise.resolve({ rows: [] });
        if (command === 'skill_status') return Promise.resolve({ rows: [] });
        throw new Error(`unexpected command: ${command}`);
      },
      Channel: class {
        onmessage = null;
      },
    },
  },
  navigator: { userAgent: 'node' },
  addEventListener() {},
  removeEventListener() {},
  getComputedStyle() {
    return { transitionDuration: '0s', animationDuration: '0s', transitionDelay: '0s', animationDelay: '0s' };
  },
};
Object.defineProperty(globalThis, 'navigator', {
  configurable: true,
  value: { userAgent: 'node' },
});
const makeElement = () => ({
  ownerDocument: globalThis.document,
  style: {},
  classList: { add() {}, remove() {}, contains() { return false; }, toggle() {} },
  addEventListener() {},
  removeEventListener() {},
  setAttribute() {},
  removeAttribute() {},
  appendChild() {},
  removeChild() {},
  insertBefore() {},
});
const body = makeElement();
// 提示（toast）最终由 Element 的 ElMessage 渲染进 body。数着往 body 挂节点的
// 次数就能判断「这次到底弹没弹提示」——静默路径的价值全在「没出声」上，
// 只断言 store 状态证明不了这一点。
let bodyAppends = 0;
body.appendChild = () => {
  bodyAppends += 1;
};
globalThis.document = {
  createElement: makeElement,
  createElementNS: makeElement,
  createTextNode: makeElement,
  createComment: makeElement,
  body,
  documentElement: makeElement(),
  addEventListener() {},
  removeEventListener() {},
};

globalThis.requestAnimationFrame = (callback) => {
  callback();
  return 1;
};
globalThis.cancelAnimationFrame = () => {};

test('failed plugin update checks do not consume the success TTL', async () => {
  const { checkPluginUpdates } = await import('../src/plugins/plugins.js');

  // 第一次探测整体失败：不推进成功 TTL，所以第二次仍然真的跑。
  assert.equal(await checkPluginUpdates({ busy: true }), null);
  assert.equal(await checkPluginUpdates({ busy: true }), undefined);
  assert.equal(pluginChecks, 2);
  // 第二次成功推进了 TTL，自动路径被拦下。
  assert.equal(await checkPluginUpdates({ busy: false }), null);
  assert.equal(pluginChecks, 2);
  // 手动点击不受 TTL 限制（与技能侧同策略：用户点了就必须真的探测，
  // 否则 15 分钟内点「检查更新」是完全没有反馈的死按钮）。
  assert.equal(await checkPluginUpdates({ busy: true }), undefined);
  assert.equal(pluginChecks, 3);
});

test('failed skill update checks back off, and manual checks bypass the backoff', async () => {
  // P2-12：逐包失败不推进成功 TTL（后端语义如此），但自动路径必须有一个退避窗口，
  // 否则切页 / 回焦 / 长任务结束都会重跑全量探测（git 来源会真的起子进程）。
  const { checkSkillUpdates } = await import('../src/skills/skills.js');

  const before = skillChecks;
  const first = await checkSkillUpdates({ busy: false });
  assert.equal(skillChecks, before + 1, '第一次自动检查应当真的跑');
  assert.ok(Array.isArray(first) && first.length === 1, '结果原样返回');

  // 自动路径的第二次调用命中失败退避：不再探测。
  const second = await checkSkillUpdates({ busy: false });
  assert.equal(second, null);
  assert.equal(skillChecks, before + 1, '退避窗口内不得重跑');

  // 手动点击（busy=true）绕过退避。
  const manual = await checkSkillUpdates({ busy: true });
  assert.ok(Array.isArray(manual), '手动检查必须真的跑');
  assert.equal(skillChecks, before + 2, '手动检查不受退避限制');
});

test('手动更新检查不置全局 busy、不挡互斥任务', async () => {
  const { checkPluginUpdates } = await import('../src/plugins/plugins.js');
  const { globalBusy, isExclusiveBusy, isLoading, withExclusive } = await import('../src/shell/loading.js');

  // 挂住探测，观察「检查进行中」的全局状态。
  const gate = deferred();
  pluginCheckGate = gate;

  const pending = checkPluginUpdates({ busy: true });
  // 放过一个微任务：让 singleFlight 的探测体真正启动并越过让路检查、
  // 进入挂起的 invoke——之后的互斥任务才不会把这次探测挡成 null。
  await Promise.resolve();
  assert.equal(isLoading('checkPluginUpdates'), true, '手动检查必须仍挂自己的按钮 loading');
  assert.equal(globalBusy.value, false, '检查更新不得置全局 busy（否则全界面按钮一起禁用）');
  assert.equal(isExclusiveBusy(), false, '检查更新不得持有互斥租约');

  // 探测进行中，真正的互斥任务照常执行（不会被排队拒绝成 undefined）。
  assert.equal(await withExclusive(async () => 'ran'), 'ran');

  gate.resolve([]);
  assert.equal(await pending, undefined, '成功路径 after() 的返回值照旧');
  assert.equal(isLoading('checkPluginUpdates'), false);
});

test('内核「检查更新」同样只挂按钮 loading，不进互斥租约', async () => {
  const { store, checkUpdates } = await import('../src/store.js');
  const { globalBusy, isExclusiveBusy, isLoading } = await import('../src/shell/loading.js');

  const gate = deferred();
  fetchReleasesGate = gate;
  const pending = checkUpdates();

  assert.equal(isLoading('checkUpdates'), true);
  assert.equal(globalBusy.value, false);
  assert.equal(isExclusiveBusy(), false);

  gate.resolve({ releases: [{ version: '9.9.9', prerelease: false }], warning: '' });
  await pending;
  assert.equal(isLoading('checkUpdates'), false);
  assert.equal(store.releases.length, 1);
});

test('内核发布列表的启动自检静默：挂 loading、失败清空列表、失败弹提示', async () => {
  const { store, checkUpdates } = await import('../src/store.js');
  const { isLoading } = await import('../src/shell/loading.js');

  // 先摆一份「已经拿到的」列表在 store 里：静默路径失败时它必须原样留下。
  fetchReleasesResult = { releases: [{ version: '0.1.7-rc.2', prerelease: false }], warning: '' };
  fetchReleasesFail = false;
  await checkUpdates(false);
  assert.equal(store.releases.length, 1);

  const gate = deferred();
  fetchReleasesGate = gate;
  const pending = checkUpdates(false);
  assert.equal(isLoading('checkUpdates'), false, '启动自检没有按钮可挂，绝不能占着 loading');
  gate.resolve(fetchReleasesResult);
  await pending;

  // 整体失败：不弹提示，也不把上一份好数据抹掉。抹掉的话页面会从「有列表」
  // 跳回「点击获取」，比没检查更像故障。
  const before = bodyAppends;
  fetchReleasesFail = true;
  await checkUpdates(false);
  assert.equal(bodyAppends, before, '静默路径失败不许弹提示');
  assert.equal(store.releases.length, 1, '静默路径失败不许清空已有列表');
  fetchReleasesFail = false;
});

test('启动自检发现有可升级版本时提示一次，手动点击则不重复提示', async () => {
  const { checkUpdates } = await import('../src/store.js');

  const upgradeResult = {
    releases: [{ version: '0.2.0-rc.2', prerelease: true }],
    warning: '',
    upgrade: '0.2.0-rc.2',
  };

  // 启动自检：人不在内核版本页，不说就没人知道。
  fetchReleasesResult = upgradeResult;
  const before = bodyAppends;
  await checkUpdates(false);
  assert.equal(bodyAppends, before + 1, '启动自检发现新版本必须提示一次');

  // 手动点击：用户正盯着列表，那一行的「安装」按钮就在眼前，再弹是重复。
  await checkUpdates(true);
  assert.equal(bodyAppends, before + 1, '手动点击不重复提示');

  // 没有可升级版本时启动自检也必须安静。
  fetchReleasesResult = { releases: upgradeResult.releases, warning: '', upgrade: null };
  await checkUpdates(false);
  assert.equal(bodyAppends, before + 1, '已是最新时启动自检不许提示');
});

test('提示与确认框显式抬到进度浮层之上', async () => {
  // P2-11：Element Plus 的默认 z-index 基线是 2000 + 自增计数，恒低于进度浮层的
  // 3000。长任务进行中弹出的确认框（托盘「退出」的二次确认、补丁的「清除记录」
  // 确认）会被浮层盖住且点不到，而任务未失败时浮层没有关闭按钮——用户看到的是
  // "点了没反应"。这里钉住两侧的关系：notify 显式给 zIndex，且高于浮层。
  const fs = await import('node:fs');
  const notify = fs.readFileSync('ui/src/shell/notify.js', 'utf8');
  assert.match(notify, /zIndex: NOTIFY_Z_INDEX/, 'ElMessage 必须显式指定 zIndex');
  assert.match(notify, /NOTIFY_Z_INDEX = PROGRESS_OVERLAY_Z_INDEX \+ 1000/);

  const css = fs.readFileSync('ui/src/theme.css', 'utf8');
  const overlayMatch = css.match(/\.progress-overlay\s*\{[^}]*z-index:\s*(\d+)/s);
  assert.ok(overlayMatch, '必须能从 theme.css 读到 .progress-overlay 的 z-index');
  const notifyMatch = notify.match(/PROGRESS_OVERLAY_Z_INDEX = (\d+)/);
  assert.ok(notifyMatch, 'notify.js 必须声明浮层 z-index 常量');
  assert.ok(
    Number(notifyMatch[1]) + 1000 > Number(overlayMatch[1]),
    '提示层级必须高于浮层'
  );
});
