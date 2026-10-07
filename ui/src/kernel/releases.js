// 内核官方发布列表：拉取、缓存口径与「最近一次成功检查」的时机。
//
// **为什么从 store.js 拆出来**：发布列表是一个独立关注点（只服务内核版本页与
// 首次运行引导），而 store.js 是全局共享状态，已经在代码预算的反棘轮上（只许
// 越来越小）。2026-10-07 为对齐设计稿的「最近检查 2 分钟前」加一个时间戳字段
// 时，门禁当场把 store.js 顶出预算——那说明该拆，不该调数字。
//
// 三条口径（搬过来时一并保留，别顺手改）：
//   · 手动（`manual = true`）：只挂按钮 loading，不持互斥租约、不置 globalBusy
//     ——探测期间其他面板的按钮照常可用。失败**清空**列表并弹提示：用户刚点的，
//     他需要知道这次没拿到。
//   · 启动自检（`manual = false`）：静默，且失败**不清空**列表。启动时网络抖一下
//     就把上一份好数据抹掉，页面会从「有列表」跳回「点击获取」，比没检查更像故障。
//   · `checkedAt` 只在**成功拿到非空列表**时写。失败不更新——否则「最近检查
//     2 分钟前」会变成「2 分钟前那次失败」，而列表还是上上次的。
import { reactive } from 'vue';
import { invoke } from '../shell/bridge.js';
import { toast, toastActionError } from '../shell/notify.js';
import { withLoading } from '../shell/loading.js';

/** 官方发布列表的状态。`VersionsPanel` 只读它，动作走 `checkUpdates`。 */
export const releases = reactive({
  // `fetch_releases` 的 releases 列表：每个 rc / alpha 一条。
  list: [],
  // 后端带上来的提示（取源降级等），由面板内联展示。
  warning: '',
  // 最近一次**成功**拿到非空列表的时刻（epoch ms，0 = 还没成功过一次）。
  checkedAt: 0,
});

/**
 * 拉一次官方发布列表并写入 `releases`。
 *
 * **返回布尔值而不是列表**：`checkUpdates`（手动 / 静默两条路）要的是「拿到没有」，
 * 首次运行引导要的是「列表本身好挑一个稳定版」。让这一处统一负责写入，两条调用
 * 路径就不会各自漏掉 `checkedAt`——首次运行引导当初就漏过（2026-10-07 修的）。
 *
 * 抛出的异常交给调用方决定怎么处理：手动路径要弹提示，静默路径要吞掉。
 */
export async function fetchReleaseList() {
  const reply = await invoke('fetch_releases');
  releases.list = reply.releases || [];
  releases.warning = reply.warning || '';
  // 与 checkUpdates 同一个口径：成功拿到**非空**列表才算「检查过」。
  if (releases.list.length) releases.checkedAt = Date.now();
  return reply;
}

/**
 * 拉一次官方发布列表。
 *
 * `upgrade` 只在静默路径上报：手动点的人正盯着列表，「安装」按钮就在那一行，
 * 再弹一次是重复；启动自检时人不在这一页，不说就没人知道。
 */
export function checkUpdates(manual = true) {
  const run = async () => {
    try {
      const reply = await fetchReleaseList();
      if (!releases.list.length) {
        if (manual) toast('没有获取到官方发布，请稍后再试', 4000, 'warning');
      } else if (!manual && reply.upgrade) {
        toast('内核有新版本 ' + reply.upgrade + '，可到「内核版本」页安装', 6000);
      }
    } catch (e) {
      if (!manual) return;
      releases.list = [];
      toastActionError('获取发布失败', e, '请检查网络或代理设置后重试；也可到 GitHub Releases 手动下载', 6000);
    }
  };
  return manual ? withLoading('checkUpdates', run) : run();
}