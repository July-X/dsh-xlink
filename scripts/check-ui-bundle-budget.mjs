// UI 产物预算：管理面板的 JS / CSS 字节数上限。
//
// 2026-10-01 从两个 workflow 的内联 heredoc 里抽出来——同一段脚本各写一遍
// 必然漂移，而且**已经漂移过一次**：desktop-ci.yml 那份带着「2026-09 实测 CSS
// 205076 字节，180k 会把所有 CI 都拦下」的来历注释，desktop-release.yml 那份
// 没有（docs/reviews/code-review-2026-09-27.md M12 预言的正是这件事）。
//
// 同时进了 `npm run check`：此前它只在 CI 跑，本地提交前那条总闸管不到它，
// 于是「预算超标」这种纯本地就能发现的事要等到 CI 才发现。
//
// 本地加载的桌面壳，gzip 后 34 KB 的 CSS 解析约 1ms——**这份预算不是启动指标，
// 是回归护栏**：防止某次改动悄悄把整包 Element Plus 拖进来。

import { existsSync, readdirSync, statSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const JS_BUDGET = 700_000;
// 230000 → 236000：2026-10-07「对齐设计稿 + 弹窗副窗主题适配」一轮。
// 这 6 KB 是**真实需求的重量**，不是膨胀：
//   · 概览页落位重排（当前内核 / 系统健康 / 套餐用量 / 需要关注+最近操作四块）
//   · 套餐用量卡改成设计稿的固定三列 + 额度行一行栅格（名称|条|百分比）
//   · 系统健康卡头汇总句、侧栏品牌区与底部拨杆、内核版本页「官方版本」卡头
//   · 四个叠色 token（--overlay-faint/soft/strong + --surface-sunken）两套主题
//     各一份，替换掉全仓 45 处写死的 rgba(255,255,255,…) / rgba(0,0,0,…)：
//     **这批改动反而把压缩后的产物体积减了一截**（字面量长、变量名也长，
//     但重复的字面量不再各自占一个压缩表项）。
// 上一轮的基线是 227583（HEAD），本轮 231406。同期 gzip 后只多 0.5 KB。
// 236200 → 256000（2026-10-09）：余量只剩 22 字节，下一处真实的样式需求
// （哪怕只是几行规则）就会把门禁顶红，而红的原因与被拦的东西无关——用户
// 「弹窗操作按钮统一靠右」那一次就已经踩到（236178 / 236200）。这 2 万字节
// 是给「下一批真实样式」留的，不是给膨胀留的：JS 侧仍有 700k 的余量、gzip
// 后 CSS 只有约 37 KB，本地解析成本可以忽略，这份预算的职责是拦住「整包
// 拖进来」（例如 Element Plus 全量进产物），不是逐字节抠样式。
const CSS_BUDGET = 256_000;

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const assets = join(root, 'ui', 'dist', 'assets');

// 产物不存在时**必须报出来**。静默跳过等于给出一道「通过」的假门禁——那比
// 没有门禁更坏：它让人以为预算被守着。
if (!existsSync(assets)) {
  console.error(`ui/dist/assets 不存在：先跑 \`npm run build:ui\` 再验预算。`);
  process.exit(1);
}

const totalBytes = (suffix) =>
  readdirSync(assets)
    .filter((name) => name.endsWith(suffix))
    .reduce((total, name) => total + statSync(join(assets, name)).size, 0);

const jsBytes = totalBytes('.js');
const cssBytes = totalBytes('.css');

// 2026-09 多内核改版后实测 CSS 205076 字节，早先的 180k 会把所有 CI 都拦下。
console.log(`UI JavaScript: ${jsBytes} bytes (budget: ${JS_BUDGET})`);
console.log(`UI CSS: ${cssBytes} bytes (budget: ${CSS_BUDGET})`);

if (jsBytes > JS_BUDGET || cssBytes > CSS_BUDGET) {
  process.exitCode = 1;
}
