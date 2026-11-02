// E2E 隔离探针（2026-11 FIX-06）：Mocha 根级 beforeAll，在**任何用例执行前**
// 从运行中的 Tauri 后端读实际数据目录，canonicalize 后必须落在本轮 runRoot 内。
//
// 为什么必须在运行中的后端里问：`DSH_DESKTOP_DATA_DIR` 这类覆盖量是在**壳进程
// 内**生效的（`kernel::data_dir` 优先读它），launcher 只能保证自己传下去的环境
// 是干净的；只有问后端实际解析出的路径，才证明隔离真的成立。
//
// 探测手段：`get_status.kernel.data_dir` 是**展示用**字符串（home 缩成 `~`、
// 过长还会中间省略，见 `display_short`），没法 canonicalize；所以真正的路径证据
// 取 `disk_usage` 报表——它用后端自己的 `AppState.data_dir` / `xlink_home` 拼
// 绝对路径。E2E 隔离 home 从未装过内核，`<data_dir>/kernels` 必然不存在，
// 会出现在 `unreadable` 里，这就是「后端实际解析出的 data_dir」。
//
// 逃逸时这里抛错：根 suite 的 beforeAll 失败会让整轮用例一条都不跑，wdio 以
// 非零码退出；launcher 的失败路径保留 runRoot，证据在
// `runRoot/evidence/backend-isolation.json`。
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, isAbsolute, join, resolve } from 'node:path';
import { browser } from '@wdio/globals';
import {
  backendIsolationProblems,
  dataDirKernelsPath,
  diskUsageReportPaths,
} from './launcher.mjs';

const BRIDGE_TIMEOUT_MS = 60_000;

/** `get_status.kernel.xlink_home` 通常已是绝对路径；兼容 `~` 形态的兜底。 */
function absoluteBackendPath(display) {
  if (typeof display !== 'string' || !display) return null;
  const userHome = process.env.HOME || process.env.USERPROFILE || '';
  if (display === '~') return userHome;
  if (display.startsWith('~/')) return join(userHome, display.slice(2));
  return display;
}

function writeEvidence(evidenceDir, payload) {
  if (!evidenceDir) return;
  try {
    mkdirSync(evidenceDir, { recursive: true });
    writeFileSync(join(evidenceDir, 'backend-isolation.json'), JSON.stringify(payload, null, 2) + '\n');
  } catch (error) {
    // 证据写不进去不能盖住真正的隔离结论：原始值只落 stderr。
    process.stderr.write('写隔离证据失败：' + error.message + '；' + JSON.stringify(payload) + '\n');
  }
}

/** 在页面里 invoke 一条命令并拿回 { ok, value } / { ok:false, error }。 */
async function invokeInPage(command, args) {
  return browser.executeAsync(
    'var done = arguments[arguments.length - 1];' +
      'window.__TAURI__.core.invoke(' +
      JSON.stringify(command) +
      ',' +
      'JSON.parse(' +
      JSON.stringify(JSON.stringify(args || {})) +
      ')).then(' +
      '  (value) => done({ ok: true, value }),' +
      '  (error) => done({ ok: false, error: String(error) })' +
      ');'
  );
}

/** 等 Tauri 桥注入并等后端状态就绪（invoke 拿到真实响应才算就绪）。 */
async function waitForBackend() {
  const deadline = Date.now() + BRIDGE_TIMEOUT_MS;
  while (Date.now() < deadline) {
    try {
      if ((await browser.execute(() => typeof window.__TAURI__ !== 'undefined')) === true) {
        const status = await invokeInPage('get_status');
        if (status && status.ok) return status.value;
      }
    } catch {
      // 页面还在导航 / AppState 尚未就绪：等下一拍。
    }
    await browser.pause(250);
  }
  return null;
}

export const mochaHooks = {
  async beforeAll() {
    const runRoot = process.env.DSH_E2E_RUN_ROOT;
    const evidenceDir = process.env.DSH_E2E_EVIDENCE_DIR;
    const expectedHome = process.env.DSH_E2E_EXPECTED_HOME;
    if (!runRoot || !evidenceDir || !expectedHome) {
      throw new Error(
        'E2E 隔离探针缺少 DSH_E2E_RUN_ROOT / DSH_E2E_EVIDENCE_DIR / DSH_E2E_EXPECTED_HOME（必须经 npm run e2e 启动）'
      );
    }
    const checkedAt = new Date().toISOString();
    const evidenceBase = { runRoot, expectedHome, checkedAt };

    const status = await waitForBackend();
    if (!status) {
      writeEvidence(evidenceDir, { ...evidenceBase, bridged: false });
      throw new Error('等待 Tauri 桥 / 后端就绪超时，无法核对后端实际数据目录（运行根：' + runRoot + '）');
    }
    const getStatus = {
      dataDirDisplay: status?.kernel?.data_dir,
      xlinkHomeDisplay: status?.kernel?.xlink_home,
    };
    // `get_status.kernel.xlink_home` 在「xlink_home 不在 HOME 下」时走的是
    // **不省略的完整路径**（见 `display_xlink_home`）：E2E 正好把两者分开，
    // 所以这里拿到的是后端实际解析出的绝对路径。
    const backendXlinkHome = absoluteBackendPath(getStatus.xlinkHomeDisplay);
    const homeProblems = [];
    if (!backendXlinkHome || !isAbsolute(backendXlinkHome)) {
      homeProblems.push('后端 xlink_home 不是可核对的绝对路径：' + getStatus.xlinkHomeDisplay);
    } else if (resolve(backendXlinkHome) !== resolve(expectedHome)) {
      homeProblems.push(
        '后端 xlink_home 与 launcher 注入的 DSH_XLINK_HOME 不一致：' +
          backendXlinkHome +
          ' != ' +
          expectedHome
      );
    }

    const disk = await invokeInPage('disk_usage', { force: false });
    if (!disk || !disk.ok) {
      writeEvidence(evidenceDir, { ...evidenceBase, getStatus, disk });
      throw new Error(
        'disk_usage 失败，无法读取后端实际数据目录：' + ((disk && disk.error) || '未知错误')
      );
    }
    const report = disk.value?.report || {};
    const reportPaths = diskUsageReportPaths(report);
    // 先查报表里所有绝对路径：真正逃逸时（例如 DSH_DESKTOP_DATA_DIR 生效）
    // 数据目录下的 kernels 会是一个外部路径，这一步能直接点名它。
    const reported = {};
    reportPaths.forEach((value, index) => {
      reported['diskUsage.paths[' + index + ']'] = value;
    });
    if (backendXlinkHome && isAbsolute(backendXlinkHome)) {
      reported.xlinkHome = backendXlinkHome;
    }
    const escaped = backendIsolationProblems(runRoot, reported);
    if (escaped.length || homeProblems.length) {
      writeEvidence(evidenceDir, {
        ...evidenceBase,
        getStatus,
        reportPaths,
        problems: [...homeProblems, ...escaped],
      });
      throw new Error(
        'E2E 后端隔离核对失败，拒绝执行用例（证据：backend-isolation.json）：' +
          [...homeProblems, ...escaped].join('；')
      );
    }
    const kernelsPath = dataDirKernelsPath(expectedHome, reportPaths);
    if (!kernelsPath) {
      writeEvidence(evidenceDir, { ...evidenceBase, getStatus, reportPaths });
      throw new Error(
        'disk_usage 没有报出数据目录下的 kernels 路径，无法证明后端 data_dir 落在本轮测试根内，拒绝执行用例' +
          '（证据：backend-isolation.json）'
      );
    }
    const actual = { dataDir: dirname(kernelsPath), xlinkHome: backendXlinkHome };
    const problems = backendIsolationProblems(runRoot, actual);
    writeEvidence(evidenceDir, {
      ...evidenceBase,
      getStatus,
      actual,
      kernelsPath,
      reportPaths,
      problems,
    });
    if (problems.length) {
      throw new Error(
        'E2E 后端数据目录逃出本轮测试根，拒绝执行用例（证据：backend-isolation.json）：' +
          problems.join('；')
      );
    }
  },
};
