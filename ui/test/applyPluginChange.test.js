// 「应用变更」（两阶段预检的第二阶段）的接线：发给后端的 spec 是**报告带来的
// applySpec**，成功后存进 store 的是**后端回传的完整报告**。
//
// 这两条都是审查 R2-P1-01 / R2-P1-02 指出的实缺陷：
//   · 过去 UI 拿 `report.pluginId`（中央库 id，`@scope/pkg` → `@scope__pkg`）
//     当 spec 回传，后端重新解析后可能装上别的东西；
//   · `withProgress` 只 resolve 布尔值，而代码把 `true` 当报告存进了 store，
//     于是诊断页读 `verdict` / `installed` / `preChangeSnapshotId` 全是 undefined。
//
// 第二条特别值得钉：`true` 存进去不会立刻炸，只会让「恢复变更前状态」按钮
// 悄悄退化成「查看快照列表」——用户看到的是一个不报错但明显不对劲的界面。
import assert from 'node:assert/strict';
import test from 'node:test';

const calls = [];
// `plugin_precheck_apply` 下一次要回的报告；置 null 表示那次失败。
let applyResult = {
  verdict: 'pass',
  pluginId: 'deepseek-pet',
  pluginName: 'dsh-pet',
  installed: true,
  summary: '已安装到当前实例',
  applySpec: 'https://github.com/owner/repo.git',
  sourceKind: 'git',
  sourceLabel: 'repo',
  pin: '',
  preChangeSnapshotId: 'snap-1',
  verifiedAtMs: 1700000000000,
};
let failNext = false;

globalThis.window = {
  __TAURI__: {
    core: {
      invoke(command, args) {
        calls.push({ command, args });
        if (command === 'plugin_precheck_apply') {
          if (failNext) {
            failNext = false;
            return Promise.reject(new Error('apply failed'));
          }
          return Promise.resolve(applyResult);
        }
        return Promise.resolve([]);
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
let bodyAppends = 0;
const body = makeElement();
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

const { applyPluginChange, pluginStore } = await import('../src/plugins/plugins.js');
const { store } = await import('../src/store.js');

/** 一份「预检通过」的报告。`applySpec` 是后端给的、能原样装回去的来源。 */
function passReport(extra = {}) {
  return {
    verdict: 'pass',
    pluginId: 'elysia395__dsh-wallpaper-engine',
    pluginName: 'wallpaper',
    installed: false,
    applySpec: 'elysia395/dsh-wallpaper-engine',
    sourceKind: 'github',
    sourceLabel: 'dsh-wallpaper-engine',
    pin: '',
    preChangeSnapshotId: '',
    verifiedAtMs: 1700000000000,
    ...extra,
  };
}

const applyCall = () => calls.filter((c) => c.command === 'plugin_precheck_apply').pop();

function reset() {
  calls.length = 0;
  pluginStore.spec = '';
  store.precheckReport = null;
  store.precheckVisible = false;
}

test('发给后端的是报告里的 applySpec，不是中央库 id', async () => {
  reset();
  await applyPluginChange(passReport());
  const args = applyCall().args;
  // 这两条值刻意不同：`@scope/pkg` 的 id 是 `@scope__pkg`。
  assert.equal(
    args.spec,
    'elysia395/dsh-wallpaper-engine',
    'spec 必须来自 applySpec；拿 pluginId 去解析会装上别的东西'
  );
  assert.notEqual(args.spec, 'elysia395__dsh-wallpaper-engine', 'pluginId 不是 spec');
  assert.equal(args.verifiedAtMs, 1700000000000, '验证时刻随报告一起带过去');
});

test('成功后存的是后端回传的完整报告，不是 true', async () => {
  reset();
  await applyPluginChange(passReport());
  assert.notEqual(store.precheckReport, true, 'R2-P1-02：布尔值不是报告');
  assert.equal(store.precheckReport.installed, true, '恢复入口要靠 installed');
  assert.equal(
    store.precheckReport.preChangeSnapshotId,
    'snap-1',
    '「恢复变更前状态」要靠 preChangeSnapshotId，丢了就退化成快照列表'
  );
  assert.equal(store.precheckVisible, true);
});

test('应用失败时不动 store：保留上一份报告，不假装已安装', async () => {
  reset();
  const stale = passReport({ installed: false });
  store.precheckReport = stale;
  failNext = true;
  await applyPluginChange(stale);
  // `store` 是 reactive，读回来是代理而不是原对象，所以断字段而不是断引用。
  assert.equal(store.precheckReport.installed, false, '失败时保留旧报告，别覆盖成半成品');
  assert.equal(
    store.precheckReport.preChangeSnapshotId,
    '',
    '失败绝不能凭空长出一个恢复点'
  );
  assert.equal(store.precheckVisible, false, '失败不该弹出「已安装」那份');
});

test('装的来源与预检报告对不上时要出声', async () => {
  reset();
  const appendsBefore = bodyAppends;
  applyResult = { ...passReport(), installed: true, preChangeSnapshotId: 'snap-x', sourceLabel: '别的仓库' };
  await applyPluginChange(passReport());
  assert.ok(
    bodyAppends > appendsBefore,
    '「装的不是验过的那个」是用户必须看见的事，不能被一句「已安装」盖过去'
  );
  // 装**已经发生**了，报告照实换成已安装那份，只是不给「一切正常」的错觉。
  assert.equal(store.precheckReport.installed, true);
  applyResult = { ...passReport(), installed: true, preChangeSnapshotId: 'snap-1', sourceLabel: 'repo' };
});

test('来源一致时不打扰用户', async () => {
  reset();
  const appendsBefore = bodyAppends;
  await applyPluginChange(
    passReport({ sourceKind: 'git', sourceLabel: 'repo', pin: '' })
  );
  const warnAppends = bodyAppends - appendsBefore;
  // 成功那一下本来就有一条「已安装」的提示；漂移告警是**额外**的一条。
  assert.ok(warnAppends <= 1, '来源一致时不该多出一条漂移告警');
});
