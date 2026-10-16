// 跨面板共用的展示映射。
//
// 这些映射此前散落在各面板里（技能页有中文映射、插件页直接渲染英文
// `npm`/`git`/`local`），同一个概念在两个页面显示不同语言（P2-41）。

import { ref } from 'vue';

// 字节 → 人类可读。原是 VersionsPanel.vue 的私有函数；磁盘用量的下钻树
// （DiskUsageTree.vue）展开后的每一行也要格式化字节，同一份格式化搬来共用
// ——单位口径（1024 进制、按 1000 归一，与 Finder / `du -h` 对得上数）只有
// 一处定义，两张表才不会各说各话。
const BYTE_UNITS = ['B', 'KB', 'MB', 'GB', 'TB'];

/** 字节 → 「495.3 MB」这类展示串；非有限数与非正数都落「0 B」。 */
export function formatBytes(bytes) {
  const value = Number(bytes);
  if (!Number.isFinite(value) || value <= 0) return '0 B';
  let index = 0;
  let scaled = value;
  while (scaled >= 1024 && index < BYTE_UNITS.length - 1) {
    scaled /= 1024;
    index += 1;
  }
  // 字节本身不显示小数（12 B 不会写成 "12.0 B"），其余保留一位。
  const digits = index === 0 ? 0 : 1;
  return `${scaled.toFixed(digits)} ${BYTE_UNITS[index]}`;
}

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

/** 路径的最后一段（`/a/b/proj` → `proj`）；认不出（空值、只有根）就原样返回。
 * 给「哪个项目」这类窄列用：整条路径进 `title`，截的是这一行，不是这条信息。 */
export function baseName(p) {
  const text = String(p ?? '').trim();
  const parts = text.replace(/[/\\]+$/, '').split(/[/\\]+/);
  return parts[parts.length - 1] || text;
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

/** 数据年龄（紧凑）：`<1min` / `12min` / `3h` / `2d`，配合刷新 icon 组成胶囊。
 *  完整中文描述用 [`relativeTimeLabel`]。 */
export function relativeAgeCompact(ms) {
  const value = Number(ms);
  if (!Number.isFinite(value) || value <= 0) return '';
  const seconds = Math.max(0, Math.floor((Date.now() - value) / 1000));
  if (seconds < 60) return '<1min';
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}min`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h`;
  return `${Math.floor(hours / 24)}d`;
}

/** 重置倒计时（紧凑）：剩余毫秒 → `1d15h` / `3h47m` / `45m`；已过期为「已重置」。
 *  完整中文描述用 [`countdownFullLabel`]，放 hover title。 */
export function countdownLabel(msLeft) {
  const value = Number(msLeft);
  if (!Number.isFinite(value) || value <= 0) return '已重置';
  const totalMinutes = Math.floor(value / 60000);
  const days = Math.floor(totalMinutes / 1440);
  const hours = Math.floor((totalMinutes % 1440) / 60);
  const minutes = totalMinutes % 60;
  if (days > 0) return hours > 0 ? `${days}d${hours}h` : `${days}d`;
  if (hours > 0) return minutes > 0 ? `${hours}h${minutes}m` : `${hours}h`;
  return `${minutes}m`;
}

/** 重置倒计时（完整中文）：`1 天 15 小时后重置`，供 hover title 使用。 */
export function countdownFullLabel(msLeft) {
  const value = Number(msLeft);
  if (!Number.isFinite(value) || value <= 0) return '已重置';
  const minutes = Math.ceil(value / 60000);
  if (minutes < 60) return `${minutes} 分钟后重置`;
  const hours = Math.floor(minutes / 60);
  const restMinutes = minutes % 60;
  if (hours < 24) return restMinutes > 0 ? `${hours} 小时 ${restMinutes} 分后重置` : `${hours} 小时后重置`;
  const days = Math.floor(hours / 24);
  const restHours = hours % 24;
  return restHours > 0 ? `${days} 天 ${restHours} 小时后重置` : `${days} 天后重置`;
}
