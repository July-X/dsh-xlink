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

/**
 * 把一段 CSS 拆成 `{selector, body}` 列表。**必须按花括号配对**，不能用
 * `/([^{}]+)\{([^}]*)\}/g`：那种写法遇到 `@media { .a { … } }` 会把内层规则
 * 整条吞掉（选择器截到 `@media (…)`，body 截到内层的第一个 `}`），媒体查询里
 * 的声明一条都查不到。
 */
export function ruleBlocks(css) {
  const out = [];
  let i = 0;
  while (i < css.length) {
    const open = css.indexOf('{', i);
    if (open < 0) break;
    const prelude = css.slice(i, open).trim();
    let depth = 1;
    let j = open + 1;
    while (j < css.length && depth > 0) {
      if (css[j] === '{') depth++;
      else if (css[j] === '}') depth--;
      j++;
    }
    const inner = css.slice(open + 1, j - 1);
    // 条件 at-rule（@media / @supports / @container / @layer）里仍是规则；
    // @keyframes / @font-face 的内层是声明块，不是选择器，不能当规则收。
    if (prelude.startsWith('@')) {
      if (/^@(media|supports|container|layer)\b/.test(prelude)) out.push(...ruleBlocks(inner));
    } else {
      out.push({ selector: prelude, body: inner });
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

/** CSS 特异度 (a,b,c)，够用即可。 */
export function specificity(selector) {
  const s = selector.replace(/\([^)]*\)/g, '');
  const ids = (s.match(/#[\w-]+/g) || []).length;
  const classes = (s.match(/\.[\w-]+|\[[^\]]*\]|:[a-z-]+/gi) || []).length;
  const elements = (s.match(/(^|[\s>+~,])\s*[a-z][\w-]*/gi) || []).length;
  return ids * 10000 + classes * 100 + elements;
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
  for (const part of selector.split(',')) {
    const subject = part.trim().split(/\s+/).pop();
    if (!subject || /^[:[]/.test(subject)) continue;
    for (const m of subject.matchAll(/\.([A-Za-z][\w-]*)|(^|[\s>+~])([a-z][\w-]*)/g)) {
      out.add(m[1] || m[3]);
    }
  }
  return out;
}

/**
 * `tokens`（元素身上实际有的类与标签）上，`prop` 最终生效的值（没有则 null）。
 * 适用判据是「主语的 token 全部都在元素身上」——子串匹配不算数。
 */
export function effectiveDeclaration(tokens, rules, prop) {
  const have = tokens instanceof Set ? tokens : new Set(tokens);
  const re = new RegExp(`(?<![-\\w])${prop}:\\s*([^;}]+)`);
  let best = null;
  for (const rule of rules) {
    const value = (rule.body.match(re) || [])[1];
    if (value === undefined) continue;
    const need = subjectTokens(rule.selector);
    if (need.size === 0) continue;
    if (![...need].every((t) => have.has(t))) continue;
    const spec = specificity(rule.selector);
    // 同特异度按源码顺序决胜：后出现的赢（`>=` 而不是 `>`）。
    if (!best || spec >= best.spec) best = { spec, value: value.trim(), rule };
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
