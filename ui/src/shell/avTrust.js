// Defender 排除项的状态与动作。
//
// 真相全在 Rust 侧：排除项写在系统注册表里，而**普通用户读不到那棵子树**
// （Windows 把 Defender 的 Exclusions 收给了管理员）。所以「排没排」由后端
// 自己记的账（`shell/<mode>/defender-exclusions.json`）说了算，前端只读后端的
// 状态并原样呈现——两边各存一份真相必然漂，而这里漂了还是**用户看得见的**
// （界面上写着已加入，实际没加）。
import { reactive } from 'vue';
import { invoke } from './bridge.js';
import { toastActionError, toastSuccess } from './notify.js';
import { withLoading } from './loading.js';
import { createStatusSource } from './async.js';

// 加入与移除共用一个 key：两者互斥，共用 key 才能防止用户连点起两个提权进程。
const SET_KEY = 'defenderExclusion';

export const avTrustStore = reactive({
  // 平台是否支持（只有 Windows 的 Defender 有「排除项」这回事）。
  // 默认 false：非 Windows 上这张卡整块不渲染；Windows 等后端状态回来再显示。
  supported: false,
  platformNote: '',
  defenderPresent: false,
  defenderRealtime: false,
  targets: [],
  // 上次加入 / 移除的结果。null = 还没试过（启动后会自动试一次）。
  last: null,
});

function normalizeStatus(raw) {
  if (!raw || typeof raw !== 'object') return null;
  const last = raw.last && typeof raw.last === 'object' ? raw.last : null;
  return {
    supported: typeof raw.supported === 'boolean' ? raw.supported : true,
    platformNote: typeof raw.platformNote === 'string' ? raw.platformNote : '',
    defenderPresent: !!raw.defenderPresent,
    defenderRealtime: !!raw.defenderRealtime,
    targets: Array.isArray(raw.targets) ? raw.targets : [],
    last: last
      ? {
          outcome: typeof last.outcome === 'string' ? last.outcome : '',
          at: typeof last.at === 'string' ? last.at : '',
          detail: typeof last.detail === 'string' ? last.detail : '',
          paths: Array.isArray(last.paths) ? last.paths : [],
        }
      : null,
  };
}

function applyStatus(raw) {
  const next = normalizeStatus(raw);
  if (!next) return false;
  Object.assign(avTrustStore, next);
  return true;
}

const loadStatus = createStatusSource('av_trust_status', applyStatus);

/// 读取排除项现状。进入设置页时拉一次，不轮询。
export function refreshAvTrustStatus() {
  return loadStatus();
}

/// 手动加入 / 移除 Defender 排除项（加入由启动时自动做一次，这里是重试与撤销）。
///
/// 会弹 UAC 管理员授权窗口，**由用户点「是」**；后端返回生效后的状态，
/// 界面据此回显。
export function setAvExclusion(enabled) {
  return withLoading(SET_KEY, async () => {
    try {
      const status = await invoke('av_trust_set_exclusion', { enabled });
      applyStatus(status);
      toastSuccess(
        enabled ? '已把数据主目录加入 Defender 排除项' : '已从 Defender 排除项里移除数据主目录',
      );
      return true;
    } catch (e) {
      // 后端的错误信息自带「下一步」（UAC 被拒 / Defender 没运行 / 篡改防护…），
      // 原样透出即可，不要在这里再叠一层含糊的提示。
      toastActionError(
        enabled ? '加入 Defender 排除项失败' : '移除 Defender 排除项失败',
        e,
        '请按提示里的下一步处理；若反复失败，打开「查看日志」把 av-trust 那几行发给维护者',
        9000,
      );
      return false;
    }
  });
}
