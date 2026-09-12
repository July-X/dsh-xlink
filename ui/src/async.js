// 面板模块共用的异步原语：在途去重（single flight）、静默刷新源、更新检查策略。
//
// 每个面板模块原先各自手写「同一请求共享一个 Promise」的样板（8 处），每处都要
// 维护自己的 in-flight 变量、在 finally 里比对后再清空。写漏一处，要么连点重复
// 发请求、要么把后续请求永久卡住。更新检查更是已经漂移成两套策略：插件侧手动
// 点击会被 15 分钟 TTL 吞掉（点了没反应），技能侧手动点击总是真的探测，且逐包
// 失败有退避。策略写在工厂里，两个模块只剩参数差异。
import { invoke } from './bridge.js';
import { toast, toastActionError } from './notify.js';
import { withLoading, withExclusive, isExclusiveBusy } from './loading.js';

/**
 * 单飞：同名请求在途时复用同一个 Promise；成功、失败都会释放。
 *
 * 返回的函数带 `busy()`，供轮询之类的调用方判断「现在有没有人在读」。
 */
export function singleFlight(run) {
  let inFlight = null;
  const flight = (...args) => {
    if (inFlight) return inFlight;
    const tracked = Promise.resolve()
      .then(() => run(...args))
      .finally(() => {
        if (inFlight === tracked) inFlight = null;
      });
    inFlight = tracked;
    return tracked;
  };
  flight.busy = () => inFlight !== null;
  return flight;
}

/**
 * 静默刷新源：`invoke(cmd)` → `apply(view)`，同一时刻只保留一次读取，
 * 失败默认静默（保留旧卡片，下次刷新再试）。
 *
 * 需要把失败讲清楚的模块（通知面板的手动刷新）传 `onError`：它拿得到原始的
 * `manual` 标记，因此自动路径仍然安静。
 */
export function createStatusSource(cmd, apply, onError) {
  return singleFlight(async (manual) => {
    try {
      apply(await invoke(cmd));
      return true;
    } catch (e) {
      if (onError) onError(e, !!manual);
      return false;
    }
  });
}

/**
 * 更新检查策略：TTL + 逐包失败退避 + 全局互斥 + 在途去重 + 提示。
 *
 * - 手动点击（`{ busy: true }`）绕过 TTL 与退避，必须真的探测；
 * - 自动路径（切页 / 回焦 / 长任务结束）受两者限制：git 来源的探测会真的
 *   起子进程，不能每次触发都重跑；
 * - 逐包失败（后端把单个来源的错误放在条目上）不推进成功 TTL，但推进退避；
 * - 每次调用都返回「本次是否拿到新结果」：无新结果时解析为 `null`，
 *   调用方不必区分「被跳过」与「被互斥挡住」。
 *
 * `after(infos)` 决定成功时解析成什么：插件面板刷新全部卡片后返回 `undefined`，
 * 技能面板返回逐包结果。
 */
export function createUpdateChecker({
  cmd,
  loadingKey,
  noun,
  ttlMs,
  failureBackoffMs = 0,
  itemFailureHint,
  after = (infos) => infos,
}) {
  let lastSuccessAt = 0;
  let lastFailureAt = 0;

  const run = async (manual, opts) => {
    if (!manual && lastSuccessAt && Date.now() - lastSuccessAt < ttlMs) return null;
    if (
      !manual &&
      failureBackoffMs &&
      lastFailureAt &&
      Date.now() - lastFailureAt < failureBackoffMs
    ) {
      return null;
    }
    if (isExclusiveBusy()) return null;

    const flight = withExclusive(async () => {
      let infos;
      try {
        infos = (await invoke(cmd)) || [];
      } catch (e) {
        if (manual) {
          toastActionError('检查' + noun + '更新失败', e, '请检查网络或代理设置后重试', 6000);
        }
        return null;
      }
      const failed = infos.filter((i) => i.error);
      if (failed.length) {
        // 只有手动点击才弹提示：自动路径下持续失败的包会让用户每次切页 / 回焦
        // 都吃一个 8 秒提示，而他不一定关心（P2-12）。
        if (manual) {
          toastActionError(
            failed.length + ' 个' + noun + '检查更新失败',
            failed[0].error,
            itemFailureHint,
            8000
          );
        }
        lastFailureAt = Date.now();
      } else {
        lastSuccessAt = Date.now();
      }
      const n = infos.filter((i) => i.latest).length;
      if (n > 0 && opts.toastOnUpdates) {
        toast('有 ' + n + ' 个' + noun + '可更新', 5000, 'warning');
      }
      return after(infos);
    });
    // withExclusive 在别的互斥任务进行中时返回 undefined。
    return flight === undefined ? null : flight;
  };

  const flight = singleFlight((manual, opts) => run(manual, opts));
  return (opts = {}) => {
    const manual = !!opts.busy;
    return manual ? withLoading(loadingKey, () => flight(true, opts)) : flight(false, opts);
  };
}
