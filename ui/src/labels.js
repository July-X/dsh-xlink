// 跨面板共用的展示映射。
//
// 这些映射此前散落在各面板里（技能页有中文映射、插件页直接渲染英文
// `npm`/`git`/`local`），同一个概念在两个页面显示不同语言（P2-41）。

import { ref } from 'vue';

/** 插件/技能来源的中文标签。未知来源原样返回，便于定位新增来源。 */
export function originLabel(origin) {
  if (origin === 'local') return '本地';
  if (origin === 'git') return 'git';
  if (origin === 'npm') return 'npm';
  return origin ? String(origin) : '未知';
}

// 面向用户的路径显示把 home 前缀折叠成 `~`：绝对路径里出现
// /Users/<name>/… 对用户是噪音，还有隐私顾虑（截图求助时暴露用户名）。
// home 由 main.js 启动时经 bridge.homeDir() 异步注入；到达前 tildePath
// 原样返回，不阻塞首屏。
const displayHomeDir = ref('');

export function setDisplayHomeDir(home) {
  displayHomeDir.value = String(home || '').replace(/[/\\]+$/, '');
}

/** home 前缀折叠成 `~`；home 未知或路径不在 home 下时原样返回。 */
export function tildePath(p) {
  const s = String(p ?? '');
  const home = displayHomeDir.value;
  if (!s || !home || !s.startsWith(home)) return s;
  return '~' + s.slice(home.length);
}

/** 「查询于 X 前」：毫秒时间戳 → 刚刚 / N 分钟前 / N 小时前 / N 天前。 */
export function relativeTimeLabel(ms) {
  const value = Number(ms);
  if (!Number.isFinite(value) || value <= 0) return '';
  const seconds = Math.max(0, Math.floor((Date.now() - value) / 1000));
  if (seconds < 60) return '刚刚';
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes} 分钟前`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours} 小时前`;
  const days = Math.floor(hours / 24);
  return `${days} 天前`;
}

/** 重置倒计时：剩余毫秒 → 「3 小时 47 分」 / 「4 天 11 小时」；已过期为「已重置」。 */
export function countdownLabel(msLeft) {
  const value = Number(msLeft);
  if (!Number.isFinite(value) || value <= 0) return '已重置';
  const minutes = Math.ceil(value / 60000);
  if (minutes < 60) return `${minutes} 分钟`;
  const hours = Math.floor(minutes / 60);
  const restMinutes = minutes % 60;
  if (hours < 24) return restMinutes > 0 ? `${hours} 小时 ${restMinutes} 分` : `${hours} 小时`;
  const days = Math.floor(hours / 24);
  const restHours = hours % 24;
  return restHours > 0 ? `${days} 天 ${restHours} 小时` : `${days} 天`;
}
