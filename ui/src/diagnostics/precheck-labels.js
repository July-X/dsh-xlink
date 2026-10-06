// 预检的展示映射：来源类型与完整性摘要。**纯函数，无副作用、无依赖。**
//
// **与 `diagnostic-labels.js` 分开**：那一层是运行记录的通用词表（stage /
// status / cause），这里只服务预检。混在一起会让「启动诊断」也要 import
// 一份它用不到的插件语义。
//
// 数据加载（拉运行记录）不在这里——那是 `diagnostics.js` 的职责。把它塞进
// 一个「映射表」文件会让这份表变成半个 store：下次有人加字段就会发现
// 「反正这里已经能 invoke 了」，于是三个职责缠在一起。
//
// 完整性分四档而不是「有 / 无」两档（设计 §6.4 / §6.5）：`sha1` 存在但抗碰撞
// 已破，`none` 意味着**根本没校验**。两者都叫「已校验」会让用户以为拿到了
// 和 npm 官方同样的保证。
/** 取源类型 → 中文名。未知取值原样透出。 */
export const SOURCE_LABELS = {
  npm: 'npm 包',
  github: 'GitHub Release',
  git: 'git 仓库',
  path: '本地目录',
};

/** 完整性摘要 → 文案 + 语义色 + 是否为弱保证。 */
export const INTEGRITY_META = {
  sha512: { label: '已通过 sha512 校验', tone: 'ok', weak: false },
  sha256: { label: '已通过 sha256 校验', tone: 'ok', weak: false },
  // sha1 存在但抗碰撞已破：显示成「已校验」是误导，措辞必须带上这句。
  sha1: { label: '已通过 sha1 校验（摘要算法较弱）', tone: 'warn', weak: true },
  // 没能校验：不是「通过」，是「没验」。
  none: { label: '未能校验完整性', tone: 'bad', weak: true },
};
