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
