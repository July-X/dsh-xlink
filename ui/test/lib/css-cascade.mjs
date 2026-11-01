// 扫 `ui/src` 的 CSS，算某条声明在给定类集合上**层叠后的生效值**。
//
// 为什么需要它：静态判据最容易写成「某条规则写了什么」，而出事那次正是
// 栽在这里——块自己带 `margin-bottom: 6px`、容器 `gap: 6px`，每处的值单看都
// 合理，叠在一起才成了 12px。真要判「这个元素的这条声明最终是什么」，就得
// 走一遍层叠：按花括号配对拆规则、算特异度、按源码顺序决胜。
//
// 三个曾经栽过的点，都在下面各有注释：
//   · `/([^{}]+)\{([^}]*)\}/g` 拆不干净 `@media`（见 ruleBlocks）
//   · 后代选择器里的类不是元素自身的规则（见 subjectClasses）
//   · 覆盖只赢它**声明过**的属性，不声明就落回基线（见 effectiveDeclaration）
//
// 支持的语法边界（2026-11-01 OPT-10 校准）：
//   · 选择器列表按顶层逗号拆开，逐选择器匹配与算特异度；
//   · 条件 at-rule（@media / @supports / @container）**默认不参与**层叠，
//     必须由调用方用 opts.conditions 明确声明当前条件才计入——当前实现只是
//     静态检查器，不知道运行期视口与特性支持，不能把条件规则当成无条件的；
//   · 规则体内重复声明按**最后一条**取值（浏览器语义）；
//   · 主题祖先 `html.dark` 默认视为**浅色**，只有 tokens 里带 `dark` 或显式
//     传 opts.theme='dark' 才计入深色规则；其它祖先条件（`.sidebar.is-collapsed`
//     之类）仍不建模，调用方必须用稳定的元素 token 集合。
//   · 仍是近似：`:is()` / `:not()` 内部的简单选择器特异度、`!important`、
//     简写属性的展开没有建模。需要这些语义的判据应改走真实 WebView 的
//     computedStyle，而不是继续扩展本工具。
import { readFileSync, readdirSync } from 'node:fs';
import { extname, resolve } from 'node:path';

export const SRC = resolve('ui/src');

/** `ui/src` 下所有 `.css`，递归。 */
export function cssFiles(dir = SRC, out = []) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const p = resolve(dir, entry.name);
    if (entry.isDirectory()) cssFiles(p, out);
    else if (extname(entry.name) === '.css') out.push(p);
  }
  return out;
}

/** `ui/src` 下所有 `.vue` 的 `<style>` 块内容，递归。返回 `{ file, css }`。 */
export function vueStyleBlocks(dir = SRC, out = []) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const p = resolve(dir, entry.name);
    if (entry.isDirectory()) vueStyleBlocks(p, out);
    else if (extname(entry.name) === '.vue') {
      const src = readFileSync(p, 'utf8');
      // 正则只留一个捕获组（`<style>` 的属性不分组），`[, css]` 才取到块内容；
      // 写成 `[, , css]` 会拿到 undefined，报错点在 ruleBlocks 里的 `css.length`
      // ——离真正的原因十万八千里。
      for (const [, css] of src.matchAll(/<style[^>]*>([\s\S]*?)<\/style>/g)) out.push({ file: p, css });
    }
  }
  return out;
}

/**
 * 全仓规则表：`.css` 文件 **加上** `.vue` 的 `<style>` 块。
 *
 * 为什么必须带后者：面板级样式大多写在组件的 scoped 块里而不是 theme.css。
 * 只扫 `.css` 时 `.kernel-summary`、`.plan-grid`、`.sidebar__theme-btn` 这些规则
 * 一条都查不到，「查层叠后的生效值」于是退化成「查全局基线」——判据看着在算
 * 层叠，实际永远只看得到 theme.css 那一半。
 *
 * scoped 的属性后缀（`.foo[data-v-abc]`）按原样参与匹配：`subjectTokens` 会把
 * `[data-v-abc]` 收成一个 token，而调用方传的类集合里没有它，
 * `[...need].every(...)` 会跳过该规则——**这正是 scoped 的语义**（那条规则只
 * 作用于本组件），工具不该替它放宽，所以这里不做任何特殊处理。
 */
export function allRulesIncludingVue() {
  const rules = [];
  for (const file of cssFiles()) {
    for (const rule of ruleBlocks(readFileSync(file, 'utf8'))) rules.push({ ...rule, file });
  }
  for (const { file, css } of vueStyleBlocks()) {
    for (const rule of ruleBlocks(css)) rules.push({ ...rule, file });
  }
  return rules;
}

/**
 * 剥掉 CSS 注释。**必须先剥**：`ruleBlocks` 按 `{` 找规则起点，而本仓大量规则
 * 都带一段解释「为什么这么写」的长注释。注释里只要出现一个 `{`（贴了段旧 CSS
 * 片段就会）或换行，prelude 就会从注释开头一路吃到花括号，选择器与主体全部
 * 错位——表现是「明明写了 `.plan-grid` 却查不到它的 grid-template-columns」。
 */
export function stripComments(css) {
  return css.replace(/\/\*[\s\S]*?\*\//g, '');
}

/**
 * 把一段 CSS 拆成 `{selector, body}` 列表。**必须按花括号配对**，不能用
 * `/([^{}]+)\{([^}]*)\}/g`：那种写法遇到 `@media { .a { … } }` 会把内层规则
 * 整条吞掉（选择器截到 `@media (…)`，body 截到内层的第一个 `}`），媒体查询里
 * 的声明一条都查不到。
 */
export function ruleBlocks(css, conditions = []) {
  const out = [];
  const text = stripComments(css);
  let i = 0;
  while (i < text.length) {
    const open = text.indexOf('{', i);
    if (open < 0) break;
    const prelude = text.slice(i, open).trim();
    let depth = 1;
    let j = open + 1;
    while (j < text.length && depth > 0) {
      if (text[j] === '{') depth++;
      else if (text[j] === '}') depth--;
      j++;
    }
    const inner = text.slice(open + 1, j - 1);
    // 条件 at-rule（@media / @supports / @container / @layer）里仍是规则；
    // @keyframes / @font-face 的内层是声明块，不是选择器，不能当规则收。
    if (prelude.startsWith('@')) {
      if (/^@(media|supports|container)\b/.test(prelude)) {
        out.push(...ruleBlocks(inner, [...conditions, prelude]));
      } else if (/^@layer\b/.test(prelude)) {
        out.push(...ruleBlocks(inner, conditions));
      }
    } else {
      out.push({ selector: prelude, body: inner, conditions: [...conditions] });
    }
    i = j;
  }
  return out;
}

/** 全仓规则表（含文件名，报错时好指认来源）。 */
export function allRules() {
  const rules = [];
  for (const file of cssFiles()) {
    for (const rule of ruleBlocks(readFileSync(file, 'utf8'))) rules.push({ ...rule, file });
  }
  return rules;
}

/**
 * 按**顶层**逗号拆选择器列表。逗号可能出现在 `:is(a, b)` / `:not(a, b)` /
 * 属性选择器的引号里，直接 `selector.split(',')` 会把一个选择器拆坏，特异度
 * 与主语 token 都跟着错。这里只按括号深度 0 的逗号拆。
 */
export function splitSelectors(selector) {
  const parts = [];
  let depth = 0;
  let start = 0;
  for (let i = 0; i < selector.length; i += 1) {
    const ch = selector[i];
    if (ch === '(' || ch === '[') depth += 1;
    else if (ch === ')' || ch === ']') depth = Math.max(0, depth - 1);
    else if (ch === ',' && depth === 0) {
      parts.push(selector.slice(start, i).trim());
      start = i + 1;
    }
  }
  parts.push(selector.slice(start).trim());
  return parts.filter(Boolean);
}

/**
 * CSS 特异度 (a,b,c)，够用即可。选择器列表取各分支最高值；逐分支匹配时
 * 调用方应传单个选择器（effectiveDeclaration 内部就是这么做的）。
 */
export function specificity(selector) {
  let best = 0;
  for (const part of splitSelectors(selector)) {
    const s = part.replace(/\([^)]*\)/g, '');
    const ids = (s.match(/#[\w-]+/g) || []).length;
    const classes = (s.match(/\.[\w-]+|\[[^\]]*\]|:[a-z-]+/gi) || []).length;
    const elements = (s.match(/(^|[\s>+~,])\s*[a-z][\w-]*/gi) || []).length;
    best = Math.max(best, ids * 10000 + classes * 100 + elements);
  }
  return best;
}

/**
 * 选择器的**主语部分**（最后一个简单选择器）里的 token：类名与裸标签名。
 *
 * 两个坑都在这里：
 *   · 后代里的 token 不是元素自身的：`.diag-card--tower .diag-card__title` 里的
 *     `.diag-card--tower` 是祖先，按它判会把一堆假阳性带进来。
 *   · **标签也要收**。`.card > h2 { border-bottom: … }` 的主语里一个类都没有，
 *     只按类判会整条规则被跳过——而「概览页的裸 h2 卡有没有线」问的正是它。
 *     裸 h2 只多一个 token `h2`，元素侧传 `['card', 'kernel-title', 'h2']` 即可。
 */
export function subjectTokens(selector) {
  const out = new Set();
  for (const part of splitSelectors(selector)) {
    const subject = part.trim().split(/\s+/).pop();
    if (!subject || /^[:[]/.test(subject)) continue;
    for (const m of subject.matchAll(/\.([A-Za-z][\w-]*)|(^|[\s>+~])([a-z][\w-]*)/g)) {
      out.add(m[1] || m[3]);
    }
  }
  return out;
}

/**
 * 选择器各分段**主语尾部**的伪类名（`pseudo-class` 与 `pseudo-element` 都收，
 * 不含 `::` 前缀）。
 *
 * 只认主语尾部：`.a:hover` 与 `.a .b:hover` 都算（主语分别是这两个），
 * 而 `.a:hover .b` 不算——那是祖先的状态，跟「`.b` 处于 hover 时是什么值」
 * 不是一回事。
 */
function subjectPseudos(selector) {
  const out = new Set();
  for (const part of splitSelectors(selector)) {
    const subject = part.trim().split(/\s+/).pop() || '';
    const m = /:{1,2}([a-z-]+)(?:\([^)]*\))?$/i.exec(subject);
    if (m) out.add(m[1]);
  }
  return out;
}

/**
 * `tokens`（元素身上实际有的类与标签）上，`prop` 最终生效的值（没有则 null）。
 * 适用判据是「主语的 token 全部都在元素身上」——子串匹配不算数。
 *
 * `opts.pseudo` 把候选规则限定到主语带这个伪类的那些，用来问「hover 时是什么
 * 色」「focus-visible 时是什么色」。它不是可选的装饰：**不给**时反而要**排除**
 * 带伪类的规则——`.a:hover { color: X }` 的主语 token 与基线 `.a` 完全相同，
 * 混进来就等于把「悬停时的值」当成「静止时的值」回答出去。这条是实打实踩过的：
 * 问 `.head-tip-icon--warning` 的基线颜色时返回了 hover 的加深色。
 */
function declarationValue(body, prop) {
  const re = new RegExp(`(?<![-\\w])${prop}:\\s*([^;}]+)`, 'g');
  let value;
  for (const match of body.matchAll(re)) value = match[1];
  return value === undefined ? undefined : value.trim();
}

export function effectiveDeclaration(tokens, rules, prop, opts = {}) {
  const have = tokens instanceof Set ? tokens : new Set(tokens);
  const activeConditions = new Set(opts.conditions || []);
  const pseudo = opts.pseudo ? opts.pseudo.replace(/^:+/, '') : null;
  // 静态工具不建模祖先链，主题只能按显式约定：默认浅色；tokens 带 dark 或
  // opts.theme='dark' 时纳入 html.dark 规则（基线与深色都参与，深色特异度更高）。
  const theme = opts.theme || (have.has('dark') ? 'dark' : 'light');
  let best = null;
  for (const rule of rules) {
    // 条件规则默认不参与：静态检查器不知道运行期视口 / 特性支持，把它当成
    // 无条件规则回答就是假确定。调用方要用它，必须显式给 opts.conditions。
    const conditions = rule.conditions || [];
    if (conditions.some((condition) => !activeConditions.has(condition))) continue;
    if (theme !== 'dark' && /(^|[\s,])html\.dark\b/.test(rule.selector)) continue;
    const value = declarationValue(rule.body, prop);
    if (value === undefined) continue;
    // 选择器列表逐分支匹配：每个分支的主语与特异度独立算，取命中分支里
    // 特异度最高的那个；顶层逗号已经在 splitSelectors 里拆开。
    for (const part of splitSelectors(rule.selector)) {
      const pseudos = subjectPseudos(part);
      if (pseudo) {
        if (!pseudos.has(pseudo)) continue;
      } else if (pseudos.size > 0) {
        continue;
      }
      const need = subjectTokens(part);
      if (need.size === 0) continue;
      if (![...need].every((t) => have.has(t))) continue;
      const spec = specificity(part);
      // 同特异度按源码顺序决胜：后出现的赢（`>=` 而不是 `>`）。
      if (!best || spec >= best.spec) best = { spec, value, rule };
    }
  }
  return best && best.value;
}

/** 从 `class="a b c"` 里取出类名数组。 */
export function classesOf(attrs) {
  const raw = (attrs.match(/class="([^"]*)"/) || [])[1] || '';
  // 用 `m[0]` 而不是解构 `[, cls]`：这里没有捕获组，matchAll 产出长度 1 的
  // 数组，`[, cls]` 拿到 undefined。收上来是一组 undefined，Set 仍非空，
  // 判据照样显示「识别出 1 个子元素」，报错离真正的原因十万八千里。
  return [...raw.matchAll(/[\w-]+/g)].map((m) => m[0]);
}
