// 跨面板共用的展示映射。
//
// 这些映射此前散落在各面板里（技能页有中文映射、插件页直接渲染英文
// `npm`/`git`/`local`），同一个概念在两个页面显示不同语言（P2-41）。

/** 插件/技能来源的中文标签。未知来源原样返回，便于定位新增来源。 */
export function originLabel(origin) {
  if (origin === 'local') return '本地';
  if (origin === 'git') return 'git';
  if (origin === 'npm') return 'npm';
  return origin ? String(origin) : '未知';
}
