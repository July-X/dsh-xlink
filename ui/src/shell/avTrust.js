// 杀软排除项的状态与动作。
//
// 真相全在 Rust 侧：Defender 的排除项写在系统注册表里，第三方杀软的信任区
// 更是只能由用户自己在它自己的界面里加。前端不自己判断「排没排」，只读后端
// 报回的状态并原样呈现——两边各存一份真相必然漂，而这里漂了还是**用户看得见
// 的**（界面上写着已排除，实际没排）。
import { reactive } from 'vue';
import { invoke } from './bridge.js';
import { toastActionError, toastSuccess } from './notify.js';
import { withLoading } from './loading.js';
import { createStatusSource } from './async.js';

// 加与移除共用一个 key：两者互斥，共用 key 才能防止用户连点起两个提权进程。
const SET_KEY = 'avTrustSet';
const MEASURE_KEY = 'avTrustMeasure';

export const avTrustStore = reactive({
  // 平台是否支持（只有 Windows 有「排除项 / 信任区」这回事）。
  supported: true,
  // 非 Windows 平台的一句话说明，直接显示。
  platformNote: '',
  defenderPresent: false,
  defenderRealtime: false,
  thirdParty: [],
  exclusions: [],
  // 排除项清单**读到了没有**。Windows 把 Defender 的 Exclusions 子树收给
  // 管理员了，普通用户读不动——读不到时清单是空的，界面必须说「读不到」，
  // 不能让它看起来像「一条排除项都没有」。
  exclusionsReadable: true,
  targets: [],
  // 本机「首次打开一个文件」的实测耗时（毫秒/个）。null = 还没测过。
  // **不许把某个数字写死进界面**：每台机器、每套杀软配置都不一样。
  openCostMs: null,
});

function normalizeStatus(raw) {
  if (!raw || typeof raw !== 'object') return null;
  return {
    supported: typeof raw.supported === 'boolean' ? raw.supported : true,
    platformNote: typeof raw.platformNote === 'string' ? raw.platformNote : '',
    defenderPresent: !!raw.defenderPresent,
    defenderRealtime: !!raw.defenderRealtime,
    thirdParty: Array.isArray(raw.thirdParty) ? raw.thirdParty : [],
    exclusions: Array.isArray(raw.exclusions) ? raw.exclusions : [],
    exclusionsReadable:
      typeof raw.exclusionsReadable === 'boolean' ? raw.exclusionsReadable : true,
    targets: Array.isArray(raw.targets) ? raw.targets : [],
  };
}

function applyStatus(raw) {
  const next = normalizeStatus(raw);
  if (!next) return false;
  Object.assign(avTrustStore, next);
  return true;
}

const loadStatus = createStatusSource('av_trust_status', applyStatus);

/// 读取杀软与排除项现状。进入设置页时拉一次，不轮询——排除项不会自己变。
export function refreshAvTrustStatus() {
  return loadStatus();
}

/// 加入 / 移除 Defender 排除项。
///
/// 会弹 UAC 管理员授权窗口，**由用户点「是」**；后端返回生效后的状态，
/// 界面据此回显「已排除 / 未排除」。
export function setAvExclusion(enabled) {
  return withLoading(SET_KEY, async () => {
    try {
      const status = await invoke('av_trust_set_exclusion', { enabled });
      applyStatus(status);
      toastSuccess(
        enabled
          ? '已把上面两个目录加入 Defender 排除项'
          : '已从 Defender 排除项里移除上面两个目录',
      );
      return true;
    } catch (e) {
      // 后端的错误信息自带「下一步」（UAC 被拒 / Defender 没运行 / 篡改防护…），
      // 原样透出即可，不要在这里再叠一层含糊的提示。
      toastActionError(
        enabled ? '加入杀软排除项失败' : '移除杀软排除项失败',
        e,
        '请按提示里的下一步处理；若反复失败，打开「查看日志」把 av-trust 那几行发给维护者',
        9000,
      );
      return false;
    }
  });
}

/// 现测本机「首次打开一个文件」的耗时（毫秒/个）。
///
/// 造一批当场新建的临时文件，每个只 open 一次就关，取平均。加完排除项再测
/// 一次就能看出有没有降下来。
export function measureAvOpenCost() {
  return withLoading(MEASURE_KEY, async () => {
    try {
      const value = await invoke('av_trust_measure_open_cost');
      avTrustStore.openCostMs =
        typeof value === 'number' && Number.isFinite(value) ? value : null;
      return avTrustStore.openCostMs;
    } catch (e) {
      toastActionError('测量首次打开文件耗时失败', e, '请确认系统临时目录可写后重试', 6000);
      return null;
    }
  });
}
